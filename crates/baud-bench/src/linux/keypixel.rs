//! **Keystroke to pixel, from outside the app**, for any terminal on a
//! wlroots-protocol compositor: Typometer's method with the compositor's
//! own clock (KP1).
//!
//! ```text
//! key-pixel <keymap.xkb> <x> <y> <w> <h> <keys> <gap-ms> <seed> [<quiet-ms>]
//! ```
//!
//! It posts each key through the virtual keyboard `linux-pointer` makes
//! (`super::pointer`) and reads a rect of the output, in its logical
//! coordinates, through the global wlr-screencopy offers
//! (wlr-screencopy-unstable-v1.xml, pinned in `docs/research/sources`):
//!
//! ```text
//! zwlr_screencopy_manager_v1  version 3
//! ```
//!
//! - **A request is outstanding before the key goes out**, and a new one goes
//!   out after each frame served: Hyprland serves a request only at its next
//!   output commit, so a commit with none pending is never seen.
//! - **The key's frame is the first served whose bytes differ from the
//!   baseline**; its `ready` time minus the key's post is the latency. Damage
//!   is no signal, as Hyprland reports the whole buffer every frame.
//! - **`ready` is the commit, not the glass**: Hyprland stamps it with its
//!   steady clock right after it commits the frame to its backend, "not when
//!   page flip happens". Both stamps are CLOCK_MONOTONIC.
//! - **The baseline is a rect gone still**: BackSpace undoes each key, and
//!   once its commit is served the rect must hold its bytes for the quiet
//!   time. Commits that leave it alone are quiet (Hyprland commits the whole
//!   output on any damage); a change starts the wait again from its bytes, and
//!   more than a few stop the run, as a blinking cursor in the rect would fire
//!   the detection early.
//!
//! One line a key, and the waits:
//!
//! ```text
//! key <n> <char> latency_us <us> frames <k> post <ns>
//! key <n> <char> not-shown frames <k>
//! key <n> <char> not-idle frames <k>
//! settle <n> busy <b>
//! ```
//!
//! The keys cycle through the home row's letters; the gap after each settle
//! is key-latency's, jittered by a quarter from a SplitMix64 seeded by
//! `<seed>`.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use baud_wayland::ArgKind;
use baud_wayland::wire::{Arg, ObjectId};

pub use super::pointer::Stop;
use super::pointer::{Devices, Wire, anonymous_file, devices, monotonic};
use crate::key_latency::parse::Count;
use crate::key_latency::poster::{Evdev, code_of, gaps};
use crate::key_latency::stats::Ms;
use crate::key_latency::sweep::Pid;

/// A moment on CLOCK_MONOTONIC, or a span of it, in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Nanos(pub u128);

/// A frame the compositor served: when it committed it, and the rect's bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The frame's `ready` time.
    pub at: Nanos,
    /// The rect, as the buffer holds it.
    pub bytes: Vec<u8>,
}

/// The screen as the probe reads it.
pub trait Screen {
    /// Ask for the next frame the compositor commits, unless a request is
    /// already outstanding.
    fn request(&mut self) -> Result<(), Stop>;
    /// The frame asked for, if one is served within `within`; a request not
    /// served stays outstanding.
    fn served(&mut self, within: Duration) -> Result<Option<Frame>, Stop>;
}

/// The keyboard as the probe types on it.
pub trait Keys {
    /// Press and release `code`: CLOCK_MONOTONIC as the press was sent.
    fn post(&mut self, code: Evdev) -> Result<Nanos, Stop>;
}

/// Frames the compositor served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frames(pub u32);

/// How many changes of the rect in a row a settle allows before it stops
/// the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tries(pub u32);

/// What one key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A frame showed it: its commit minus the key's post, and the frames
    /// served up to and including it.
    Shown {
        /// When the key was posted.
        post: Nanos,
        /// Commit minus post.
        latency: Nanos,
        /// Frames served.
        frames: Frames,
    },
    /// No frame showed it within the limit.
    NotShown {
        /// Frames served.
        frames: Frames,
    },
    /// The rect changed in a commit from before the key: it moved on its own,
    /// so the key is not read.
    NotIdle {
        /// Frames served.
        frames: Frames,
    },
}

/// **One key**: a request outstanding, the key, then frames until one's bytes
/// differ from `baseline` or `within` runs out.
pub fn one_key<P: Screen + Keys>(
    p: &mut P,
    code: Evdev,
    baseline: &[u8],
    within: Duration,
) -> Result<Key, Stop> {
    p.request()?;
    let post = p.post(code)?;
    let end = Instant::now() + within;
    let mut frames = 0;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let Some(f) = p.served(left)? else {
            return Ok(Key::NotShown {
                frames: Frames(frames),
            });
        };
        frames += 1;
        if f.bytes != baseline {
            if f.at < post {
                return Ok(Key::NotIdle {
                    frames: Frames(frames),
                });
            }
            return Ok(Key::Shown {
                post,
                latency: Nanos(f.at.0 - post.0),
                frames: Frames(frames),
            });
        }
        p.request()?;
    }
}

/// **The baseline after a reset**: `reset` posted and its commit served,
/// then frames until the rect has held its bytes for `quiet`. Hyprland
/// commits the whole output on any damage anywhere, so a commit that leaves
/// the rect as it was is quiet; one that changes it starts the quiet time
/// again from its bytes, and more than `tries` such stop the run, as a
/// blinking cursor in the rect would fire the detection early. The baseline,
/// and the commits that changed the rect.
pub fn settle<P: Screen + Keys>(
    p: &mut P,
    reset: Evdev,
    quiet: Duration,
    within: Duration,
    tries: Tries,
) -> Result<(Vec<u8>, Frames), Stop> {
    p.request()?;
    p.post(reset)?;
    let Some(first) = p.served(within)? else {
        return Err("key-pixel: the reset showed no frame".into());
    };
    let mut last = first.bytes;
    let mut still_from = Instant::now();
    let mut changed = 0;
    loop {
        let left = quiet.saturating_sub(still_from.elapsed());
        if left.is_zero() {
            return Ok((last, Frames(changed)));
        }
        p.request()?;
        let Some(f) = p.served(left)? else {
            return Ok((last, Frames(changed)));
        };
        if f.bytes != last {
            changed += 1;
            if changed > tries.0 {
                return Err(format!(
                    "key-pixel: the rect did not hold still: {changed} changes in a row, none {} ms apart",
                    quiet.as_millis()
                )
                .into());
            }
            last = f.bytes;
            still_from = Instant::now();
        }
    }
}

const MANAGER_CAPTURE_OUTPUT_REGION: u16 = 1;
const FRAME_DESTROY: u16 = 1;
const FRAME_COPY_WITH_DAMAGE: u16 = 2;
const FRAME_BUFFER: u16 = 0;
const FRAME_READY: u16 = 2;
const FRAME_FAILED: u16 = 3;
const FRAME_BUFFER_DONE: u16 = 6;
const SHM_CREATE_POOL: u16 = 0;
const POOL_CREATE_BUFFER: u16 = 0;
const POOL_DESTROY: u16 = 1;
const OBJECT: ArgKind = ArgKind::Object { nullable: false };
// A typed new_id: its interface is the request's, so the wire carries the
// id alone (an untyped one is `bind`'s, with its interface and version).
const NEW_ID: ArgKind = ArgKind::NewId {
    interface: Some("a new object"),
};

/// The shm buffer frames are copied into, and the shape the compositor gave.
struct Buffer {
    id: ObjectId,
    file: File,
    /// format, width, height, stride
    shape: [u32; 4],
}

/// The compositor, read and typed on through one connection.
pub struct Probe {
    w: Wire,
    keys: Devices,
    manager: ObjectId,
    output: ObjectId,
    shm: ObjectId,
    rect: [i32; 4],
    buffer: Option<Buffer>,
    /// The request outstanding, if one is.
    frame: Option<ObjectId>,
    /// Its events read and not yet taken, oldest first.
    pending: VecDeque<(u16, Vec<u8>)>,
}

impl Probe {
    /// Everything bound over `w`, the keyboard made with `keymap`.
    fn open(mut w: Wire, keymap: &[u8], rect: [i32; 4]) -> Result<Probe, Stop> {
        w.open_registry()?;
        let keys = devices(&mut w, keymap)?;
        let shm = w.bind("wl_shm", 1)?;
        let output = w.bind("wl_output", 1)?;
        let manager = w.bind("zwlr_screencopy_manager_v1", 3)?;
        w.roundtrip()?;
        w.take_others();
        Ok(Probe {
            w,
            keys,
            manager,
            output,
            shm,
            rect,
            buffer: None,
            frame: None,
            pending: VecDeque::new(),
        })
    }

    /// A word of an event's body.
    fn word(body: &[u8], at: usize) -> Result<u32, Stop> {
        let b = body
            .get(at..at + 4)
            .ok_or_else(|| Stop::Failed(format!("key-pixel: an event of {} bytes", body.len())))?;
        Ok(u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// The next event `frame` was sent, waiting until `end`. Events for any
    /// other object are read and dropped.
    fn next_of(&mut self, frame: ObjectId, end: Instant) -> Result<Option<(u16, Vec<u8>)>, Stop> {
        loop {
            self.pending.extend(
                self.w
                    .take_others()
                    .into_iter()
                    .filter(|e| e.0 == frame.0)
                    .map(|(_, op, body)| (op, body)),
            );
            if let Some(e) = self.pending.pop_front() {
                return Ok(Some(e));
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() || !self.w.readable(left)? {
                return Ok(None);
            }
            self.w.read()?;
        }
    }

    /// The buffer for `shape`, made anew when the shape changes.
    fn buffer_for(&mut self, shape: [u32; 4]) -> Result<ObjectId, Stop> {
        if let Some(b) = self.buffer.as_ref().filter(|b| b.shape == shape) {
            return Ok(b.id);
        }
        let [format, width, height, stride] = shape;
        let size = u64::from(stride) * u64::from(height);
        let file = anonymous_file()?;
        file.set_len(size)
            .map_err(|e| format!("key-pixel: the buffer: {e}"))?;
        let int =
            |v: u64| i32::try_from(v).map_err(|_| Stop::from("key-pixel: a buffer too large"));
        let (pool, id) = (self.w.new_id(), self.w.new_id());
        self.w.send(
            self.shm,
            SHM_CREATE_POOL,
            &[NEW_ID, ArgKind::Fd, ArgKind::Int],
            &[
                Arg::NewId(pool),
                Arg::Fd(file.as_fd()),
                Arg::Int(int(size)?),
            ],
        )?;
        self.w.send(
            pool,
            POOL_CREATE_BUFFER,
            &[
                NEW_ID,
                ArgKind::Int,
                ArgKind::Int,
                ArgKind::Int,
                ArgKind::Int,
                ArgKind::Uint,
            ],
            &[
                Arg::NewId(id),
                Arg::Int(0),
                Arg::Int(int(u64::from(width))?),
                Arg::Int(int(u64::from(height))?),
                Arg::Int(int(u64::from(stride))?),
                Arg::Uint(format),
            ],
        )?;
        self.w.send(pool, POOL_DESTROY, &[], &[])?;
        self.buffer = Some(Buffer { id, file, shape });
        Ok(id)
    }
}

impl Screen for Probe {
    fn request(&mut self) -> Result<(), Stop> {
        if self.frame.is_some() {
            return Ok(());
        }
        let frame = self.w.new_id();
        let [x, y, w, h] = self.rect;
        self.w.send(
            self.manager,
            MANAGER_CAPTURE_OUTPUT_REGION,
            &[
                NEW_ID,
                ArgKind::Int,
                OBJECT,
                ArgKind::Int,
                ArgKind::Int,
                ArgKind::Int,
                ArgKind::Int,
            ],
            &[
                Arg::NewId(frame),
                Arg::Int(0),
                Arg::Object(self.output),
                Arg::Int(x),
                Arg::Int(y),
                Arg::Int(w),
                Arg::Int(h),
            ],
        )?;
        // The shm buffer's shape, then buffer_done (version 3).
        let end = Instant::now() + Duration::from_secs(2);
        let mut shape = None;
        loop {
            match self.next_of(frame, end)? {
                None => return Err("key-pixel: the frame gave no buffer_done".into()),
                Some((FRAME_BUFFER, body)) => {
                    shape = Some([
                        Self::word(&body, 0)?,
                        Self::word(&body, 4)?,
                        Self::word(&body, 8)?,
                        Self::word(&body, 12)?,
                    ]);
                }
                Some((FRAME_BUFFER_DONE, _)) => break,
                Some((FRAME_FAILED, _)) => {
                    return Err("key-pixel: the compositor failed the frame".into());
                }
                Some(_) => {}
            }
        }
        let shape = shape.ok_or("key-pixel: the frame offered no shm buffer")?;
        let buffer = self.buffer_for(shape)?;
        self.w.send(
            frame,
            FRAME_COPY_WITH_DAMAGE,
            &[OBJECT],
            &[Arg::Object(buffer)],
        )?;
        self.frame = Some(frame);
        Ok(())
    }

    fn served(&mut self, within: Duration) -> Result<Option<Frame>, Stop> {
        let Some(frame) = self.frame else {
            return Err("key-pixel: a frame waited for with none asked".into());
        };
        let end = Instant::now() + within;
        loop {
            match self.next_of(frame, end)? {
                None => return Ok(None),
                Some((FRAME_READY, body)) => {
                    let secs = (u128::from(Self::word(&body, 0)?) << 32)
                        | u128::from(Self::word(&body, 4)?);
                    let at = Nanos(secs * 1_000_000_000 + u128::from(Self::word(&body, 8)?));
                    let b = self.buffer.as_ref().ok_or("key-pixel: no buffer")?;
                    let [_, _, height, stride] = b.shape;
                    let mut bytes = vec![
                        0u8;
                        usize::try_from(u64::from(stride) * u64::from(height))
                            .unwrap_or(0)
                    ];
                    b.file
                        .read_exact_at(&mut bytes, 0)
                        .map_err(|e| format!("key-pixel: the buffer: {e}"))?;
                    self.w.send(frame, FRAME_DESTROY, &[], &[])?;
                    self.frame = None;
                    self.pending.clear();
                    return Ok(Some(Frame { at, bytes }));
                }
                Some((FRAME_FAILED, _)) => {
                    return Err("key-pixel: the compositor failed the copy".into());
                }
                Some(_) => {}
            }
        }
    }
}

impl Keys for Probe {
    fn post(&mut self, code: Evdev) -> Result<Nanos, Stop> {
        let at = monotonic()?;
        self.keys.key(&self.w, code.0, 1)?;
        self.keys.key(&self.w, code.0, 0)?;
        Ok(Nanos(at.0))
    }
}

/// BackSpace, which undoes each key.
const BACKSPACE: Evdev = Evdev(14);
/// The letters the keys cycle through.
const CHARS: &str = "asdfghjkl";
/// How long a key or a reset may take to show.
const LIMIT: Duration = Duration::from_secs(1);
/// How many changes of the rect in a row a settle allows.
const TRIES: Tries = Tries(5);

/// The tool over its arguments and an environment, writing to `out`.
pub fn start(
    args: &[OsString],
    env: &dyn Fn(&str) -> Option<OsString>,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    const USAGE: &str =
        "usage: key-pixel <keymap.xkb> <x> <y> <w> <h> <keys> <gap-ms> <seed> [<quiet-ms>]";
    if args.len() != 8 && args.len() != 9 {
        return Err(USAGE.into());
    }
    let text = |i: usize| args[i].to_str().ok_or(Stop::from(USAGE));
    let num =
        |i: usize| -> Result<i64, Stop> { text(i)?.parse::<i64>().map_err(|_| Stop::from(USAGE)) };
    let int32 = |i: usize| i32::try_from(num(i)?).map_err(|_| Stop::from(USAGE));
    let rect = [int32(1)?, int32(2)?, int32(3)?, int32(4)?];
    let keys = u32::try_from(num(5)?).map_err(|_| Stop::from(USAGE))?;
    let gap = Ms(i128::from(num(6)?));
    let seed = Pid(u32::try_from(num(7)?).map_err(|_| Stop::from(USAGE))?);
    let quiet = Duration::from_millis(if args.len() == 9 {
        u64::try_from(num(8)?).map_err(|_| Stop::from(USAGE))?
    } else {
        300
    });
    let keymap =
        std::fs::read(&args[0]).map_err(|e| format!("{}: {e}", args[0].to_string_lossy()))?;
    let dir = env("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    let display = env("WAYLAND_DISPLAY").unwrap_or_else(|| "wayland-0".into());
    let path = std::path::Path::new(&dir).join(display);
    let sock = UnixStream::connect(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut p = Probe::open(Wire::over(sock), &keymap, rect)?;

    let say = |out: &mut dyn Write, line: String| -> Result<(), Stop> {
        writeln!(out, "{line}").map_err(|_| Stop::Unflushed)?;
        out.flush().map_err(|_| Stop::Unflushed)
    };
    // A warm-up key; each key's settle undoes the key before it, so the
    // baseline is taken just before the key it is for, after the gap.
    let first = code_of('a').ok_or("key-pixel: the keymap types no a")?;
    one_key(&mut p, first, &[], LIMIT)?;
    let waits = gaps(gap, Count(usize::try_from(keys).unwrap_or(0) + 1), seed);
    let chars: Vec<char> = CHARS.chars().collect();
    for n in 1..=keys {
        let c = chars[usize::try_from(n - 1).unwrap_or(0) % chars.len()];
        let code = code_of(c).ok_or("key-pixel: a letter the keymap does not type")?;
        let wait = waits
            .get(usize::try_from(n - 1).unwrap_or(0))
            .map_or(0, |m| m.0);
        std::thread::sleep(Duration::from_millis(u64::try_from(wait).unwrap_or(0)));
        let (base, busy) = settle(&mut p, BACKSPACE, quiet, LIMIT, TRIES)?;
        say(out, format!("settle {n} busy {}", busy.0))?;
        match one_key(&mut p, code, &base, LIMIT)? {
            Key::Shown {
                post,
                latency,
                frames,
            } => say(
                out,
                format!(
                    "key {n} {c} latency_us {:.1} frames {} post {}",
                    latency.0 as f64 / 1000.0,
                    frames.0,
                    post.0
                ),
            )?,
            Key::NotShown { frames } => {
                say(out, format!("key {n} {c} not-shown frames {}", frames.0))?
            }
            Key::NotIdle { frames } => {
                say(out, format!("key {n} {c} not-idle frames {}", frames.0))?
            }
        }
    }
    let (_, busy) = settle(&mut p, BACKSPACE, quiet, LIMIT, TRIES)?;
    say(out, format!("settle {} busy {}", keys + 1, busy.0))?;
    Ok(())
}

/// The whole tool, for the example: the code to exit with.
pub fn run(args: &[OsString]) -> i32 {
    let env = |name: &str| std::env::var_os(name);
    match start(args, &env, &mut std::io::stdout()) {
        Ok(()) => 0,
        Err(Stop::Unflushed) => super::UNFLUSHED,
        Err(Stop::Failed(why)) => {
            eprintln!("{why}");
            1
        }
    }
}
