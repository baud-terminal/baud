//! A pointer and a keyboard for a headless wlroots compositor, from a script
//! (`scripts/linux-pointer.sh` drives it).
//!
//! It speaks the wire through `baud_wayland`'s codec and socket calls, and
//! binds two globals a wlroots compositor offers to clients:
//!
//! ```text
//! zwlr_virtual_pointer_manager_v1  (wlr-virtual-pointer-unstable-v1.xml)
//! zwp_virtual_keyboard_manager_v1  (virtual-keyboard-unstable-v1.xml)
//! ```
//!
//! Both XMLs are pinned in `docs/research/sources`, and not in `baud_wayland`:
//! the app binds neither. One process holds one pointer and one keyboard for
//! the whole script, so a drag is one press, its moves and one release on the
//! same device, and the modifiers a click is read with are the seat's while
//! it happens.
//!
//! ```text
//! linux-pointer <keymap.xkb> < script
//! ```
//!
//! One command a line:
//!
//! ```text
//! move X Y           motion_absolute to (X, Y) of the extent, then frame
//! extent W H         the extent later moves are given in: the layout's
//!                    size in logical pixels by default (POINTER_W and
//!                    POINTER_H, else 1024x768); twice it, to reach the
//!                    half pixels between
//! press B | release B | click B
//!                    B is left, right, middle, side, extra, forward, back
//!                    or task (BTN_SIDE to BTN_TASK, 0x113 to 0x117);
//!                    click is a press and a release
//! wheel N            N detents down (negative up): axis_discrete, value
//!                    15 a detent, as libinput reports a wheel, then frame
//! across N           the same on the horizontal axis
//! swipe PX           a touchpad's scroll, PX surface pixels down (negative
//!                    up): axis_source finger and one axis, no detents
//! mods M             the modifiers held: none, or shift, ctrl, alt joined
//!                    by '+'; sent as the keymap's bits, Shift 0, Control 2,
//!                    Mod1 3
//! key K              an evdev key code pressed and released
//! tick MS            advance the event clock; every event advances it 10
//! sleep MS           wait in real time, and advance the event clock as much
//! sh CMD             run CMD with bash, waiting for it
//! waitfor CMD        run CMD every 50 ms until it succeeds, 10 s at most
//! again              destroy both devices and make two new ones: the seat
//!                    loses its pointer and keyboard and gains new ones
//! ```
//!
//! The event clock is the time every request carries: it starts at 1000 ms
//! and moves only by the script, so a double click is the same double click
//! however long the machine took between its two halves, and two clicks the
//! script keeps apart are never one.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use baud_wayland::ArgKind;
use baud_wayland::sys;
use baud_wayland::wire::{self, Arg, Fixed, ObjectId, Opcode};

use super::text;
use crate::key_latency::py;

/// The buttons a script names, and their evdev codes.
const BUTTONS: [(&str, u32); 8] = [
    ("left", 0x110),
    ("right", 0x111),
    ("middle", 0x112),
    ("side", 0x113),
    ("extra", 0x114),
    ("forward", 0x115),
    ("back", 0x116),
    ("task", 0x117),
];
/// The modifiers a script names, as the keymap's bits.
const MODS: [(&str, u32); 3] = [("shift", 1 << 0), ("ctrl", 1 << 2), ("alt", 1 << 3)];

/// `wl_display`, the one object a connection starts with.
const DISPLAY: ObjectId = ObjectId(1);
// Opcodes, in each interface's order in its XML.
const DISPLAY_SYNC: u16 = 0;
const DISPLAY_GET_REGISTRY: u16 = 1;
const REGISTRY_BIND: u16 = 0;
/// `zwlr_virtual_pointer_manager_v1.create_virtual_pointer`.
const VPM_CREATE: u16 = 0;
const VP_MOTION_ABSOLUTE: u16 = 1;
const VP_BUTTON: u16 = 2;
const VP_AXIS: u16 = 3;
const VP_FRAME: u16 = 4;
const VP_AXIS_SOURCE: u16 = 5;
const VP_AXIS_DISCRETE: u16 = 7;
const VP_DESTROY: u16 = 8;
/// `zwp_virtual_keyboard_manager_v1.create_virtual_keyboard`.
const VKM_CREATE: u16 = 0;
const VK_KEYMAP: u16 = 0;
const VK_KEY: u16 = 1;
const VK_MODIFIERS: u16 = 2;
const VK_DESTROY: u16 = 3;
// wayland.xml's enums.
const AXIS_VERTICAL: u32 = 0;
const AXIS_HORIZONTAL: u32 = 1;
const AXIS_SOURCE_WHEEL: u32 = 0;
const AXIS_SOURCE_FINGER: u32 = 1;
const KEYMAP_XKB_V1: u32 = 1;

const OBJECT: ArgKind = ArgKind::Object { nullable: false };
const NEW_ID: ArgKind = ArgKind::NewId {
    interface: Some("a new object"),
};

/// Why a script stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    /// What a failure says on stderr, exit 1.
    Failed(String),
    /// Stdout could not take a line.
    Unflushed,
}

impl From<String> for Stop {
    fn from(why: String) -> Stop {
        Stop::Failed(why)
    }
}

impl From<&str> for Stop {
    fn from(why: &str) -> Stop {
        Stop::Failed(why.to_string())
    }
}

/// The bits a request carries of any integer: the low 32.
fn low32(v: i128) -> u32 {
    u32::try_from(v.rem_euclid(1 << 32)).unwrap_or(0)
}

/// A signed 32-bit argument, or the failure a larger value is.
fn int32(v: i128) -> Result<i32, Stop> {
    i32::try_from(v).map_err(|_| Stop::Failed(format!("{v} does not fit a signed 32-bit word")))
}

/// A float as 24.8 fixed point, rounded as `round` rounds: ties to even.
fn fixed(v: f64) -> Result<Fixed, Stop> {
    let scaled = text::round(v * 256.0).ok_or_else(|| format!("{v} has no integer"))?;
    Ok(Fixed(int32(scaled)?))
}

/// The socket, the objects' numbers and what the compositor has said.
#[derive(Debug)]
pub struct Wire {
    sock: UnixStream,
    next: ObjectId,
    inbox: Vec<u8>,
    globals: HashMap<String, (u32, u32)>,
    registry: ObjectId,
    waiting: HashSet<u32>,
    done: HashSet<u32>,
}

impl Wire {
    /// A client over a socket already connected.
    pub fn over(sock: UnixStream) -> Wire {
        Wire {
            sock,
            next: ObjectId(2),
            inbox: Vec::new(),
            globals: HashMap::new(),
            registry: ObjectId(0),
            waiting: HashSet::new(),
            done: HashSet::new(),
        }
    }

    fn new_id(&mut self) -> ObjectId {
        let n = self.next;
        self.next.0 += 1;
        n
    }

    fn send(&self, obj: ObjectId, op: u16, sig: &[ArgKind], args: &[Arg<'_>]) -> Result<(), Stop> {
        let mut buf = [0u8; 512];
        let mut fds = Vec::new();
        let n = wire::write(&mut buf, &mut fds, obj, Opcode(op), sig, args)
            .map_err(|e| format!("a request: {e:?}"))?;
        // A short send carries every fd with its first byte.
        let mut at =
            sys::send(&self.sock, &buf[..n], &fds).map_err(|e| format!("the socket: {e}"))?;
        while at < n {
            at +=
                sys::send(&self.sock, &buf[at..n], &[]).map_err(|e| format!("the socket: {e}"))?;
        }
        Ok(())
    }

    fn read(&mut self) -> Result<(), Stop> {
        let mut buf = [0u8; 65536];
        let mut fds: Vec<OwnedFd> = Vec::new();
        let n =
            sys::recv(&self.sock, &mut buf, &mut fds).map_err(|e| format!("the socket: {e}"))?;
        if n == 0 {
            return Err("linux-pointer: the compositor closed the connection".into());
        }
        self.inbox.extend_from_slice(&buf[..n]);
        while let Some(h) = wire::header(&self.inbox) {
            if self.inbox.len() < h.size {
                break;
            }
            let body: Vec<u8> = self.inbox[wire::HEADER_BYTES.min(h.size)..h.size].to_vec();
            self.inbox.drain(..h.size);
            self.event(h.sender.0, h.opcode.0, &body)?;
        }
        Ok(())
    }

    fn event(&mut self, obj: u32, op: u16, body: &[u8]) -> Result<(), Stop> {
        let word = |at: usize| -> Result<u32, Stop> {
            let b = body
                .get(at..at + 4)
                .ok_or_else(|| Stop::Failed(format!("an event of {} bytes", body.len())))?;
            Ok(u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
        };
        let text_at = |at: usize, n: usize| -> Result<String, Stop> {
            let end = at.saturating_add(n.saturating_sub(1)).min(body.len());
            let b = body.get(at.min(end)..end).unwrap_or(&[]);
            String::from_utf8(b.to_vec()).map_err(|e| Stop::Failed(format!("an event: {e}")))
        };
        if obj == 1 && op == 0 {
            // wl_display.error
            let (oid, code, n) = (word(0)?, word(4)?, word(8)?);
            let said = text_at(12, usize::try_from(n).unwrap_or(usize::MAX))?;
            return Err(
                format!("linux-pointer: error on object {oid}, code {code}: {said}").into(),
            );
        }
        if obj == self.registry.0 && op == 0 {
            // wl_registry.global
            let name = word(0)?;
            let n = usize::try_from(word(4)?).unwrap_or(usize::MAX);
            let iface = text_at(8, n)?;
            let version = word(8 + n + wire_pad(n))?;
            self.globals.insert(iface, (name, version));
        } else if op == 0 && self.waiting.contains(&obj) {
            // wl_callback.done
            self.done.insert(obj);
        }
        Ok(())
    }

    fn roundtrip(&mut self) -> Result<(), Stop> {
        let cb = self.new_id();
        self.waiting.insert(cb.0);
        self.send(DISPLAY, DISPLAY_SYNC, &[NEW_ID], &[Arg::NewId(cb)])?;
        while !self.done.contains(&cb.0) {
            self.read()?;
        }
        Ok(())
    }

    fn bind(&mut self, iface: &str, version: u32) -> Result<ObjectId, Stop> {
        let Some(&(name, offered)) = self.globals.get(iface) else {
            return Err(format!("linux-pointer: the compositor offers no {iface}").into());
        };
        let obj = self.new_id();
        self.send(
            self.registry,
            REGISTRY_BIND,
            &[ArgKind::Uint, ArgKind::NewId { interface: None }],
            &[
                Arg::Uint(name),
                Arg::NewIdWith {
                    interface: iface,
                    version: version.min(offered),
                    id: obj,
                },
            ],
        )?;
        Ok(obj)
    }
}

/// The bytes that pad a string of `n` to four.
fn wire_pad(n: usize) -> usize {
    n.wrapping_neg() % 4
}

/// A file only the compositor's mapping will read: the kernel's own anonymous
/// file, which is what the keymap goes in on Linux, the one place this runs
/// against a compositor.
#[cfg(target_os = "linux")]
fn anonymous_file() -> Result<std::fs::File, Stop> {
    use std::os::fd::FromRawFd;
    // SAFETY: the name is NUL-terminated and outlives the call.
    let fd = unsafe { libc::memfd_create(c"keymap".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!("memfd_create: {}", std::io::Error::last_os_error()).into());
    }
    // SAFETY: the kernel just made this descriptor and nothing else owns it.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

/// The same off Linux, where only this crate's tests reach it: a file with
/// no name left.
#[cfg(not(target_os = "linux"))]
fn anonymous_file() -> Result<std::fs::File, Stop> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = format!("baud-keymap-{}-{n}", std::process::id());
    let path = std::env::temp_dir().join(name);
    let file = fs::File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file)
}

/// The two devices and what the script has moved.
#[derive(Debug)]
struct Devices {
    seat: ObjectId,
    vpm: ObjectId,
    vkm: ObjectId,
    pointer: ObjectId,
    keyboard: ObjectId,
    extent: Vec<i128>,
    clock: i128,
    keymap: Vec<u8>,
}

impl Devices {
    fn make(&mut self, w: &mut Wire) -> Result<(), Stop> {
        let (pointer, keyboard) = (w.new_id(), w.new_id());
        let manager = |id: ObjectId| [Arg::Object(self.seat), Arg::NewId(id)];
        w.send(self.vpm, VPM_CREATE, &[OBJECT, NEW_ID], &manager(pointer))?;
        w.send(self.vkm, VKM_CREATE, &[OBJECT, NEW_ID], &manager(keyboard))?;
        let mut file = anonymous_file()?;
        file.write_all(&self.keymap)
            .map_err(|e| format!("the keymap: {e}"))?;
        let size = u32::try_from(self.keymap.len()).map_err(|e| format!("the keymap: {e}"))?;
        w.send(
            keyboard,
            VK_KEYMAP,
            &[ArgKind::Uint, ArgKind::Fd, ArgKind::Uint],
            &[
                Arg::Uint(KEYMAP_XKB_V1),
                Arg::Fd(file.as_fd()),
                Arg::Uint(size),
            ],
        )?;
        drop(file);
        w.roundtrip()?;
        self.pointer = pointer;
        self.keyboard = keyboard;
        Ok(())
    }

    /// The event clock's next value.
    fn at(&mut self) -> u32 {
        self.clock += 10;
        low32(self.clock)
    }

    fn frame(&self, w: &Wire) -> Result<(), Stop> {
        w.send(self.pointer, VP_FRAME, &[], &[])
    }

    fn scroll(&mut self, w: &Wire, axis: u32, n: i128) -> Result<(), Stop> {
        w.send(
            self.pointer,
            VP_AXIS_SOURCE,
            &[ArgKind::Uint],
            &[Arg::Uint(AXIS_SOURCE_WHEEL)],
        )?;
        let time = self.at();
        // A float's rounding, as the detent's value travels.
        let value = fixed(15.0 * n as f64)?;
        let detents = int32(n)?;
        w.send(
            self.pointer,
            VP_AXIS_DISCRETE,
            &[ArgKind::Uint, ArgKind::Uint, ArgKind::Fixed, ArgKind::Int],
            &[
                Arg::Uint(time),
                Arg::Uint(axis),
                Arg::Fixed(value),
                Arg::Int(detents),
            ],
        )?;
        self.frame(w)
    }

    fn button(&mut self, w: &Wire, code: u32, state: u32) -> Result<(), Stop> {
        let time = self.at();
        w.send(
            self.pointer,
            VP_BUTTON,
            &[ArgKind::Uint, ArgKind::Uint, ArgKind::Uint],
            &[Arg::Uint(time), Arg::Uint(code), Arg::Uint(state)],
        )?;
        self.frame(w)
    }

    fn key(&mut self, w: &Wire, code: u32, state: u32) -> Result<(), Stop> {
        let time = self.at();
        w.send(
            self.keyboard,
            VK_KEY,
            &[ArgKind::Uint, ArgKind::Uint, ArgKind::Uint],
            &[Arg::Uint(time), Arg::Uint(code), Arg::Uint(state)],
        )
    }
}

/// A script's integer, or the failure anything else is.
fn int(s: &str) -> Result<i128, Stop> {
    py::int(s)
        .ok_or_else(|| Stop::Failed(format!("invalid literal for int(): {}", py::repr_str(s))))
}

/// `CMD` through bash, its stdin and stdout this process's; whether it
/// succeeded.
fn bash(cmd: &str) -> Result<bool, Stop> {
    let status = Command::new("bash")
        .args(["-c", cmd])
        .status()
        .map_err(|e| format!("bash: {e}"))?;
    Ok(status.success())
}

/// One command.
fn command(
    w: &mut Wire,
    d: &mut Devices,
    cmd: &str,
    rest: &str,
    line: &str,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    match cmd {
        "move" => {
            let v = py::split(rest).map(int).collect::<Result<Vec<_>, _>>()?;
            let [x, y] = v[..] else {
                return Err(format!("move wants X Y, got {rest:?}").into());
            };
            let time = d.at();
            let (ew, eh) = match d.extent[..] {
                [ew, eh, ..] => (ew, eh),
                _ => return Err(format!("an extent of {} numbers", d.extent.len()).into()),
            };
            w.send(
                d.pointer,
                VP_MOTION_ABSOLUTE,
                &[
                    ArgKind::Uint,
                    ArgKind::Uint,
                    ArgKind::Uint,
                    ArgKind::Uint,
                    ArgKind::Uint,
                ],
                &[
                    Arg::Uint(time),
                    Arg::Uint(low32(x)),
                    Arg::Uint(low32(y)),
                    Arg::Uint(low32(ew)),
                    Arg::Uint(low32(eh)),
                ],
            )?;
            d.frame(w)?;
        }
        "press" | "release" | "click" => {
            let b = BUTTONS
                .iter()
                .find(|b| b.0 == rest)
                .ok_or_else(|| format!("no button {rest:?}"))?
                .1;
            if cmd != "release" {
                d.button(w, b, 1)?;
            }
            if cmd != "press" {
                d.button(w, b, 0)?;
            }
        }
        "wheel" => d.scroll(w, AXIS_VERTICAL, int(rest)?)?,
        "swipe" => {
            w.send(
                d.pointer,
                VP_AXIS_SOURCE,
                &[ArgKind::Uint],
                &[Arg::Uint(AXIS_SOURCE_FINGER)],
            )?;
            let time = d.at();
            let px = text::float(rest)
                .ok_or_else(|| format!("could not convert string to float: {rest:?}"))?;
            w.send(
                d.pointer,
                VP_AXIS,
                &[ArgKind::Uint, ArgKind::Uint, ArgKind::Fixed],
                &[
                    Arg::Uint(time),
                    Arg::Uint(AXIS_VERTICAL),
                    Arg::Fixed(fixed(px)?),
                ],
            )?;
            d.frame(w)?;
        }
        "across" => d.scroll(w, AXIS_HORIZONTAL, int(rest)?)?,
        "mods" => {
            let mut bits = 0;
            if rest != "none" {
                for m in rest.split('+') {
                    bits |= MODS
                        .iter()
                        .find(|k| k.0 == m)
                        .ok_or_else(|| format!("no modifier {m:?}"))?
                        .1;
                }
            }
            w.send(
                d.keyboard,
                VK_MODIFIERS,
                &[ArgKind::Uint, ArgKind::Uint, ArgKind::Uint, ArgKind::Uint],
                &[Arg::Uint(bits), Arg::Uint(0), Arg::Uint(0), Arg::Uint(0)],
            )?;
        }
        "key" => {
            let k = low32(int(rest)?);
            d.key(w, k, 1)?;
            d.key(w, k, 0)?;
        }
        "extent" => {
            d.extent = py::split(rest).map(int).collect::<Result<Vec<_>, _>>()?;
        }
        "tick" => d.clock += int(rest)?,
        "sleep" => {
            let ms = int(rest)?;
            let wait = Duration::try_from_secs_f64(ms as f64 / 1000.0)
                .map_err(|e| format!("sleep {ms}: {e}"))?;
            std::thread::sleep(wait);
            d.clock += ms;
        }
        "sh" => {
            bash(rest)?;
        }
        "waitfor" => {
            let mut met = false;
            for _ in 0..200 {
                if bash(rest)? {
                    met = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !met {
                writeln!(out, "linux-pointer: timed out waiting for: {rest}")
                    .and_then(|()| out.flush())
                    .map_err(|_| Stop::Unflushed)?;
            }
        }
        "again" => {
            w.send(d.pointer, VP_DESTROY, &[], &[])?;
            w.send(d.keyboard, VK_DESTROY, &[], &[])?;
            w.roundtrip()?;
            std::thread::sleep(Duration::from_millis(300));
            d.make(w)?;
        }
        _ => return Err(format!("linux-pointer: unknown command: {line}").into()),
    }
    w.roundtrip()
}

/// The next line of a script, taken as it arrives: `None` at the end. Only
/// `\n` ends one. A compositor's run reads one line at a time, so a command's
/// child reads the stdin this leaves it.
fn next_line(input: &mut dyn BufRead) -> Result<Option<String>, Stop> {
    let mut raw = Vec::new();
    let n = input
        .read_until(b'\n', &mut raw)
        .map_err(|e| format!("stdin: {e}"))?;
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&raw).into_owned()))
}

/// A script over a connected client: `size` is the layout's as `POINTER_W`
/// and `POINTER_H` say it, `keymap` the file's bytes, and `out` what a
/// command's own line goes to.
pub fn script(
    mut w: Wire,
    keymap: &[u8],
    size: (&str, &str),
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    let mut map = keymap.to_vec();
    map.push(0);
    w.registry = w.new_id();
    w.send(
        DISPLAY,
        DISPLAY_GET_REGISTRY,
        &[NEW_ID],
        &[Arg::NewId(w.registry)],
    )?;
    w.roundtrip()?;
    let seat = w.bind("wl_seat", 1)?;
    let vpm = w.bind("zwlr_virtual_pointer_manager_v1", 1)?;
    let vkm = w.bind("zwp_virtual_keyboard_manager_v1", 1)?;
    let mut d = Devices {
        seat,
        vpm,
        vkm,
        pointer: ObjectId(0),
        keyboard: ObjectId(0),
        extent: Vec::new(),
        clock: 1000,
        keymap: map,
    };
    d.make(&mut w)?;
    d.extent = vec![int(size.0)?, int(size.1)?];
    while let Some(raw) = next_line(input)? {
        let line = py::strip(&raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        command(&mut w, &mut d, cmd, rest, line, out)?;
    }
    Ok(())
}

/// The compositor's socket: `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`.
fn socket_path(env: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf, Stop> {
    let dir = env("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    let display = env("WAYLAND_DISPLAY").unwrap_or_else(|| "wayland-0".into());
    Ok(Path::new(&dir).join(display))
}

/// The tool over its arguments, an environment, its stdin and its stdout.
pub fn start(
    args: &[OsString],
    env: &dyn Fn(&str) -> Option<OsString>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    let path = args
        .first()
        .ok_or("usage: linux-pointer <keymap.xkb> < script")?;
    let keymap = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let socket = socket_path(env)?;
    let sock = UnixStream::connect(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    let lossy = |name: &str, default: &str| {
        env(name).map_or(default.to_string(), |v| v.to_string_lossy().into_owned())
    };
    let (pw, ph) = (lossy("POINTER_W", "1024"), lossy("POINTER_H", "768"));
    script(Wire::over(sock), &keymap, (&pw, &ph), input, out)
}

/// The whole tool, for the example: the code to exit with.
pub fn run(args: &[OsString]) -> i32 {
    let env = |name: &str| std::env::var_os(name);
    let mut input = std::io::stdin().lock();
    match start(args, &env, &mut input, &mut std::io::stdout()) {
        Ok(()) => 0,
        Err(Stop::Unflushed) => super::UNFLUSHED,
        Err(Stop::Failed(why)) => {
            eprintln!("{why}");
            1
        }
    }
}
