//! **`kitten icat --hold` playing README's logo GIF**, as kitten sent it to
//! a baud-core `Term` on a pty (`kitten_icat/stream.txt`): the image, its
//! 319 frames and three animation controls, then kitten's wait for a key.
//!
//! The GIF (`assets/readme/logo-dark.gif`, read here by
//! `kitten_icat/gif.rs`, not by baud's decoder) is 840 by 580: 320 images,
//! each at Disposal Method 1, "Do not dispose" (`[[source: gif89a]]` 23),
//! the first covering the screen with no transparent index and each after
//! it a rectangle whose transparent pixels leave the frame before showing;
//! delays of 3 hundredths, every eighth 4; a NETSCAPE loop count of 0, for
//! ever. kitten (`[[source: kitty-icat-transmit]]` `gc_for_image`,
//! `transmit_image`) sends the first image as RGB with `a=T` and each
//! after it with `a=f`: the image's rectangle as `x, y, s, v`, its pixels
//! as RGBA with the transparent ones `0, 0, 0, 0`, `c` the frame before,
//! alpha blended, `z` the delay in milliseconds. Its controls are
//! `a=a,v=1,r=1,z=30` after the first, `s=2` after the second and `s=3`
//! after the last. `v=1` is not the GIF's count: kitten's `--loop`
//! defaults to -1, "Negative values loop forever" (`[[source:
//! kitty-icat-options]]`), and sets `v=1` for any negative value. Here the
//! two agree.
//!
//! Each payload is made here from the GIF and checked, before it is sent,
//! against the digest of the payload kitten sent, so the stream fed is
//! kitten's to the pixel. The expected frames are ffmpeg's decode
//! (`kitten_icat/ffmpeg.txt`), never baud's. The arena is the app's
//! default, 95 MiB (`baud-app/src/host.rs`), too small for 320 whole
//! frames, so the animation is stored as records.

#[path = "kitten_icat/gif.rs"]
mod gif;

use baud_core::*;
use baud_domain::{
    Animated, DecodeBudget, DecodeProgress, FrameNo, HostTime, ImageBudget, ImageId, ImageMotion,
    ImagePx, KeyCode, KeyEvent, KittyFlags, KittyNumber, Modifiers,
};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// baud-app's default image arena, in pixels.
const APP_ARENA: usize = 95 << 18;

const STREAM: &str = include_str!("kitten_icat/stream.txt");
const FFMPEG: &str = include_str!("kitten_icat/ffmpeg.txt");

/// What kitten wrote before its first command: a carriage return and the
/// columns that centre a 50-column image on 80.
const BEFORE: &[u8] = b"\r\x1b[15C";

/// What kitten wrote after its last command, through the prompt
/// (`[[source: kitty-icat-main]]`: `fmt.Print("\r")`, then
/// `tui.HoldTillEnter`): the newline after the image, its loop's reset of
/// modes, focus and in-band resize reporting, the kitty keyboard flags 29
/// pushed, and the prompt.
const HOLD: &[u8] = b"\r\n\r\x1b[?s\x1b[*x\x1b[4l\x1b[?1l\x1b[?5l\x1b[?2004l\x1b[?1000l\
\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?8h\x1b[?7h\x1b[?25h\x1b[?1004h\x1b[?2048h\
\x1b[>29u\x1b[?25l\x1b[1;32mPress Enter or Esc to exit\x1b[m";

/// What kitten wrote after Enter, as it exited: its loop's finalizer, the
/// keyboard flags popped, and the Primary DA it waits on before leaving;
/// nothing that deletes the image.
const EXIT: &[u8] = b"\x1b[?25h\x1b[<u\x1b7\x1b[?2048l\x1b[?r\x1b8\x1b[c";

/// FNV-1a 64, the digest the fixtures carry.
fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325, |h, &x| {
        (h ^ u64::from(x)).wrapping_mul(0x0100_0000_01b3)
    })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 3 * 4 + 4);
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

/// A command of the capture: its control data and its payload's digest
/// and length.
struct Command {
    keys: String,
    payload: Option<(u64, usize)>,
}

fn commands() -> Vec<Command> {
    STREAM
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let w: Vec<&str> = l.split(' ').collect();
            Command {
                keys: w[0].to_owned(),
                payload: (w[1] != "-").then(|| {
                    (
                        u64::from_str_radix(w[1], 16).unwrap(),
                        w[2].parse().unwrap(),
                    )
                }),
            }
        })
        .collect()
}

fn key(keys: &str, name: &str) -> Option<i64> {
    keys.split(',')
        .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
        .map(|v| v.parse().unwrap())
}

fn logo() -> gif::Gif {
    gif::read(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/readme/logo-dark.gif"
        ))
        .unwrap(),
    )
}

/// **The payload kitten sent for `cmd`, made from the GIF**: the first
/// image's pixels as RGB for `a=T`, and for an `a=f` over frame `c` the
/// image after it, its rectangle the command's `x, y, s, v`.
fn payload(g: &gif::Gif, cmd: &Command) -> Vec<u8> {
    let k = |n| key(&cmd.keys, n);
    match &cmd.keys[..3] {
        "a=T" => g.images[0]
            .rgba
            .chunks(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect(),
        a => {
            assert_eq!(a, "a=f", "only a=T and a=f carry data");
            let im = &g.images[usize::try_from(k("c").unwrap()).unwrap()];
            let at = |n| usize::try_from(k(n).unwrap_or(0)).unwrap();
            assert_eq!(
                (at("x"), at("y"), at("s"), at("v")),
                (im.x, im.y, im.w, im.h),
                "kitten's rectangle is the image's"
            );
            im.rgba.clone()
        }
    }
}

/// kitten's stream, every payload checked against the capture's digest.
fn stream(g: &gif::Gif) -> Vec<u8> {
    let mut out = BEFORE.to_vec();
    for cmd in commands() {
        out.extend_from_slice(b"\x1b_G");
        out.extend_from_slice(cmd.keys.as_bytes());
        if let Some((digest, len)) = cmd.payload {
            let data = payload(g, &cmd);
            assert_eq!((fnv(&data), data.len()), (digest, len), "{}", cmd.keys);
            out.push(b';');
            out.extend_from_slice(&b64(&miniz_oxide::deflate::compress_to_vec_zlib(&data, 1)));
        }
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

/// `bytes` in reads of 4096, as `READ_CHUNK` reads a pty, every image
/// decode run, and what the terminal answered.
fn feed(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    let mut replies = Vec::new();
    for read in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
        while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
        replies.extend_from_slice(t.reply());
        t.clear_reply();
    }
    replies
}

/// The GIF and kitten's stream, made once for every test here.
fn logo_and_stream() -> &'static (gif::Gif, Vec<u8>) {
    static MADE: std::sync::OnceLock<(gif::Gif, Vec<u8>)> = std::sync::OnceLock::new();
    MADE.get_or_init(|| {
        let g = logo();
        let bytes = stream(&g);
        (g, bytes)
    })
}

/// The logo sent as kitten sent it, to a terminal of the app's arena and
/// cell (17 by 34 device pixels), and the image it made.
fn played<R>(f: impl FnOnce(&mut Term<'_>, ImageId, &gif::Gif) -> R) -> R {
    let (g, bytes) = logo_and_stream();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(APP_ARENA));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
    assert!(feed(&mut t, bytes).is_empty(), "q=2: nothing answered");
    let number = key(&commands()[0].keys, "I").unwrap();
    let id = t
        .images()
        .kitty_newest(KittyNumber::of(u32::try_from(number).unwrap()))
        .expect("the image is stored");
    f(&mut t, id, g)
}

fn at(ms: u64) -> HostTime {
    HostTime::of_nanos(1_000_000_000 + ms * 1_000_000)
}

fn shown(t: &Term<'_>, id: ImageId) -> u32 {
    t.images().kitty_frames(id).unwrap().1.raw()
}

/// The frame on show as RGBA bytes.
fn canvas(t: &Term<'_>, id: ImageId) -> Vec<u8> {
    t.images()
        .frame_pixels(id, FrameNo::of(shown(t, id)))
        .expect("the frame on show")
        .iter()
        .flat_map(|p| {
            let (r, g, b) = p.rgb();
            [r.get(), g.get(), b.get(), p.alpha().get()]
        })
        .collect()
}

/// **The GIF's own facts**, which the rest of this file leans on, read by
/// the reader here.
#[test]
fn the_logo_is_320_frames_kept_in_place_looping_for_ever() {
    let g = &logo_and_stream().0;
    assert_eq!((g.width, g.height), (840, 580));
    assert_eq!(g.images.len(), 320);
    assert_eq!(g.loops, Some(0), "NETSCAPE2.0: for ever");
    assert!(g.images.iter().all(|im| im.disposal == 1 && !im.interlaced));
    let delays: Vec<u16> = g.images.iter().map(|im| im.delay_cs).collect();
    for (k, d) in delays.iter().enumerate() {
        assert_eq!(*d, if (k + 1) % 8 == 0 { 4 } else { 3 }, "image {}", k + 1);
    }
    assert_eq!(g.images[0].transparent, 0);
    assert_eq!(
        (g.images[0].x, g.images[0].y, g.images[0].w, g.images[0].h),
        (0, 0, 840, 580)
    );
    assert!(g.images[1..].iter().all(|im| im.transparent > 0));
    assert_eq!(g.images[1].transparent, 52_460);
    assert_eq!(g.images[319].transparent, 470_575);
}

/// **baud keeps every frame kitten sent, each with its gap.**
/// "the frame *gap* can also be specified using the ``z`` key. The gap is
/// the number of milliseconds to wait before displaying the next frame"
/// (`[[source: kitty-graphics-rst]]`, "Transferring animation frame
/// data"); "the first frame or *root frame* is created with the base
/// image data and has no gap, so its gap must be set using this control
/// code" ("Controlling animations"), which kitten's first `a=a,r=1,z=30`
/// does. kitten's `z` is the GIF's delay in milliseconds, unrounded: Go's
/// hundredths times ten.
#[test]
fn every_frame_kitten_sent_is_kept_with_the_gifs_delay() {
    played(|t, id, g| {
        let (count, _, _) = t.images().kitty_frames(id).unwrap();
        assert_eq!(count, 320);
        assert_eq!(t.kitty().frames, 319);
        assert_eq!(t.kitty().refused, 0);
        for (k, im) in g.images.iter().enumerate() {
            let n = u32::try_from(k + 1).unwrap();
            let (gap, _) = t.images().frame_gap(id, FrameNo::of(n)).unwrap();
            assert_eq!(gap, u32::from(im.delay_cs) * 10, "frame {n}");
        }
        let sent: Vec<i64> = commands()
            .iter()
            .filter(|c| c.keys.starts_with("a=f"))
            .map(|c| key(&c.keys, "z").unwrap())
            .collect();
        let gif: Vec<i64> = g.images[1..]
            .iter()
            .map(|im| i64::from(im.delay_cs) * 10)
            .collect();
        assert_eq!(sent, gif, "kitten's z is the delay, unrounded");
    });
}

/// **The clock shows each frame once its gap has passed,
/// never before, and after the last goes back to the first, for ever.**
/// "``s=3`` runs the animation normally, after the last frame, the
/// terminal loops back to the first frame. The number of loops can be
/// controlled by the ``v`` key. ... ``v=1`` is loop infinitely"
/// (`[[source: kitty-graphics-rst]]`, "Controlling animations"). The
/// clock is baud's own, `Term::animate_images`, at times given here: two
/// whole loops, each frame looked at a millisecond before it is due and
/// when it is.
#[test]
fn the_clock_runs_each_frame_its_gap_and_loops_for_ever() {
    played(|t, id, g| {
        assert_eq!(t.images().frame_loops(id), Some((0, 0)), "v=1: for ever");
        assert_eq!(
            t.animate_images(at(0), ImageMotion::Play),
            Animated::Same,
            "stamped"
        );
        assert_eq!(shown(t, id), 1);
        let mut due = 0u64;
        for lap in 0..2u32 {
            for (k, im) in g.images.iter().enumerate() {
                due += u64::from(im.delay_cs) * 10;
                let frame = u32::try_from(k + 1).unwrap();
                let next = frame % 320 + 1;
                assert_eq!(
                    t.next_image_frame(at(due - 1)),
                    Some(at(due)),
                    "frame {frame}"
                );
                assert_eq!(
                    t.animate_images(at(due - 1), ImageMotion::Play),
                    Animated::Same
                );
                assert_eq!(shown(t, id), frame, "lap {lap}: not before its gap");
                assert_eq!(
                    t.animate_images(at(due), ImageMotion::Play),
                    Animated::Changed
                );
                assert_eq!(shown(t, id), next, "lap {lap}: at its gap");
            }
            assert_eq!(due, 10_000 * u64::from(lap + 1), "ten seconds a loop");
            assert_eq!(t.images().frame_loops(id), Some((lap + 1, 0)));
        }
    });
}

/// **Every frame shown is ffmpeg's decode of the GIF, to
/// the byte, and opaque.** "Frames are created by composing the
/// transmitted data onto a background canvas. This canvas can be either a
/// single color, or the pixels from a previous frame. The composition can
/// be of two types, either a simple replacement (``X=1``) key or a full
/// alpha blend (the default)" (`[[source: kitty-graphics-rst]]`,
/// "Transferring animation frame data"). kitten sends no `X`, so each
/// transparent pixel of a frame's data -- 52,460 of the second's 67,184 --
/// shows frame `c` beneath it, which is GIF89a's "Do not dispose": the
/// frame before is where the transparent pixels show through. The first
/// frame is opaque, so no frame has a pixel through which the page
/// beneath the image could show. A pixel changed in a canvas changes its
/// digest, the control on the comparison.
#[test]
fn every_frame_is_ffmpegs_decode_and_shows_the_frame_before_through_its_transparency() {
    let ffmpeg: Vec<u64> = FFMPEG
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| u64::from_str_radix(l.split(' ').nth(1).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(ffmpeg.len(), 320);
    played(|t, id, g| {
        t.animate_images(at(0), ImageMotion::Play);
        let mut before = canvas(t, id);
        let mut due = 0u64;
        for k in 0..320 {
            let frame = canvas(t, id);
            assert_eq!(shown(t, id), u32::try_from(k + 1).unwrap());
            assert!(
                frame.chunks(4).all(|p| p[3] == 255),
                "frame {} opaque",
                k + 1
            );
            assert_eq!(fnv(&frame), ffmpeg[k], "frame {} is ffmpeg's", k + 1);
            assert_ne!(
                fnv(&frame),
                ffmpeg[(k + 1) % 320],
                "frame {} is not the next",
                k + 1
            );
            if k > 0 {
                let im = &g.images[k];
                let mut seen = 0;
                for (j, p) in im.rgba.chunks(4).enumerate() {
                    if p[3] == 0 {
                        let at = 4 * ((im.y + j / im.w) * 840 + im.x + j % im.w);
                        assert_eq!(frame[at..at + 4], before[at..at + 4], "frame {}", k + 1);
                        seen += 1;
                    }
                }
                assert_eq!(seen, im.transparent);
            }
            due += u64::from(g.images[k].delay_cs) * 10;
            t.animate_images(at(due), ImageMotion::Play);
            before = frame;
        }
        let mut one_off = before.clone();
        one_off[4 * (300 * 840 + 420)] ^= 1;
        assert_ne!(fnv(&one_off), fnv(&before), "one pixel moves the digest");
        assert_eq!(shown(t, id), 1, "back to the first");
    });
}

/// **Enter ends kitten's hold, and the image stays.** kitten
/// pushes keyboard flags 29 and waits for "enter", "kp_enter", "esc",
/// "ctrl+c" or "ctrl+d" (`[[source: kitty-tui-hold]]` `HoldTillEnter`);
/// under flag 8, "report all keys as escape codes", Enter is `CSI 13 u`
/// (`[[source: kitty-keyboard]]`). Turning on in-band resize reports
/// answers with the size at once. At exit kitten pops its flags, sends a
/// Primary DA and leaves once an answer of the shape
/// `\x1b\[\?[0-9:;]+c` arrives (`[[source: kitty-tui-loop-read]]`
/// `has_da1_response`); it sends no delete, so the image and its
/// placement stay, still playing.
#[test]
fn enter_ends_the_hold_and_the_image_stays_placed() {
    played(|t, id, _| {
        let replies = feed(t, HOLD);
        assert_eq!(
            replies, b"\x1b[48;24;80;816;1360t",
            "the in-band size, at once"
        );
        assert_eq!(t.kitty_flags().bits(), 29, "kitten's flags pushed");
        assert!(t.key(KeyEvent::press(KeyCode::Enter, Modifiers::NONE)));
        assert_eq!(t.reply(), b"\x1b[13u");
        t.clear_reply();
        let da = feed(t, EXIT);
        let body = da
            .strip_prefix(b"\x1b[?")
            .and_then(|r| r.strip_suffix(b"c"));
        assert!(
            body.is_some_and(
                |b| !b.is_empty() && b.iter().all(|c| c.is_ascii_digit() || b":;".contains(c))
            ),
            "{:?} is the DA kitten's exit waits for",
            String::from_utf8_lossy(&da)
        );
        assert_eq!(t.kitty_flags(), KittyFlags::NONE, "popped");
        assert!(t.images().kitty_frames(id).is_some(), "the image stays");
        let placed: Vec<_> = t
            .images()
            .placements()
            .filter(|(_, p)| p.image == id)
            .collect();
        assert_eq!(placed.len(), 1, "with its placement");
        assert_eq!(placed[0].1.col, Col::at_index(15), "where kitten put it");
        t.animate_images(at(0), ImageMotion::Play);
        assert_eq!(t.next_image_frame(at(0)), Some(at(30)), "still playing");
    });
}
