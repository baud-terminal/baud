//! **kitty frames kept as records** (`[[source: kitty-graphics]]`,
//! "Animation"): a kitty animation whose frames no gap holds whole is kept
//! as what each frame changed, and every frame is still the frame kitty's
//! composition makes -- on `kitten icat`'s real stream of README's logo
//! film made 140 pixels wide (`tools/tests/recordings/kitty/icat-logo-dark-140.raw`),
//! and against
//! frames kept whole through edits, compositions and deletes of the frames
//! others are made over. The expected pixels are computed here, from the
//! capture's own data, never by the code under test.

use baud_core::*;
use baud_domain::{
    AnimState, Animated, DecodeBudget, DecodeProgress, FrameNo, HostTime, ImageBudget, ImageId,
    ImageMotion, ImagePx, RowMap,
};

/// **The image memory the film is replayed in**: 4 MiB of words, which
/// holds 50 of its frames whole, as the app's 95 MiB holds 22 of the film's
/// at 840x580, and its 320 as records.
const ICAT_IMAGES: usize = 1 << 20;

/// The capture's pty: 120x40 cells of 14x32 pixels.
const ICAT_WS: WinSize = WinSize {
    ws_row: 40,
    ws_col: 120,
};

/// The copy's size, and its frames.
const LOGO: (usize, usize) = (140, 97);
const LOGO_FRAMES: u32 = 320;

/// `kitten icat --transfer-mode=stream` of README's logo film made 140
/// pixels wide, as recorded.
fn capture() -> Vec<u8> {
    let path = format!(
        "{}/../../tools/tests/recordings/kitty/icat-logo-dark-140.raw",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `bytes` fed in reads of `n`, each followed by every decode step, as the
/// app's loop takes a read; the replies.
fn feed_by(t: &mut Term<'_>, bytes: &[u8], n: usize) -> Vec<u8> {
    let mut replies = Vec::new();
    for read in bytes.chunks(n.max(1)) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
        while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
        replies.extend_from_slice(t.reply());
        t.clear_reply();
    }
    replies
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    feed_by(t, bytes, bytes.len())
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
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

/// Base64 read back, padded or not, as kitty reads a payload.
fn unb64(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in text {
        let Some(v) = B64.iter().position(|&b| b == c) else {
            continue;
        };
        acc = acc << 6 | u32::try_from(v).unwrap();
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).unwrap());
        }
    }
    out
}

fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend_from_slice(payload);
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

/// **One graphics command of the capture**: its keys and its payload, the
/// chunks of an `m=1` run joined, decoded and inflated where `o=z`.
struct Cmd {
    keys: Keys,
    data: Vec<u8>,
}

impl Cmd {
    fn key(&self, k: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    fn num(&self, k: &str) -> usize {
        self.key(k).map_or(0, |v| v.parse().unwrap())
    }
}

/// A command's keys, each name and value.
type Keys = Vec<(String, String)>;

/// The capture's graphics commands, in order.
fn commands(bytes: &[u8]) -> Vec<Cmd> {
    let mut out: Vec<Cmd> = Vec::new();
    let mut open: Option<(Keys, Vec<u8>)> = None;
    let mut at = 0;
    while let Some(s) = bytes[at..].windows(3).position(|w| w == b"\x1b_G") {
        let start = at + s + 3;
        let len = bytes[start..]
            .windows(2)
            .position(|w| w == b"\x1b\\")
            .expect("a command ends");
        let body = &bytes[start..start + len];
        at = start + len + 2;
        let (ctrl, payload) = match body.iter().position(|&b| b == b';') {
            Some(p) => (&body[..p], &body[p + 1..]),
            None => (body, &b""[..]),
        };
        let keys: Keys = String::from_utf8(ctrl.to_vec())
            .unwrap()
            .split(',')
            .filter(|kv| !kv.is_empty())
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap();
                (k.to_string(), v.to_string())
            })
            .collect();
        let more = keys.iter().any(|(k, v)| k == "m" && v == "1");
        let (keys, mut text) = match open.take() {
            Some((first, text)) => (first, text),
            None => (keys, Vec::new()),
        };
        text.extend_from_slice(payload);
        if more {
            open = Some((keys, text));
            continue;
        }
        let raw = unb64(&text);
        let zlib = keys.iter().any(|(k, v)| k == "o" && v == "z");
        let data = match zlib {
            true => miniz_oxide::inflate::decompress_to_vec_zlib(&raw).expect("zlib"),
            false => raw,
        };
        out.push(Cmd { keys, data });
    }
    out
}

/// **The logo's frames as kitty composes them**, each handed to `each`
/// with its number: the root `a=T` RGB, and each `a=f` its data over the
/// frame before -- the capture's every frame names the one before as its
/// base, and its data's alpha is 0 or 255, which the premises assert, so
/// over the opaque root a pixel is the data's where its alpha is 255 and
/// the base's where it is 0, whatever the blend's rounding.
fn icat_frames(cmds: &[Cmd], mut each: impl FnMut(u32, &[[u8; 4]])) {
    let (w, h) = LOGO;
    let root = cmds.iter().find(|c| c.key("a") == Some("T")).expect("a=T");
    assert_eq!(
        (root.num("s"), root.num("v")),
        (w, h),
        "the premise, its size"
    );
    assert_eq!(root.key("f"), Some("24"), "the premise, RGB");
    let mut frame: Vec<[u8; 4]> = root
        .data
        .chunks(3)
        .map(|p| [p[0], p[1], p[2], 255])
        .collect();
    assert_eq!(frame.len(), w * h);
    each(1, &frame);
    let mut n = 1u32;
    for c in cmds.iter().filter(|c| c.key("a") == Some("f")) {
        n += 1;
        assert_eq!(
            c.num("c"),
            usize::try_from(n - 1).unwrap(),
            "frame {n}: over the one before"
        );
        assert_eq!(c.key("f"), None, "frame {n}: RGBA, kitty's default");
        assert_eq!(
            (c.key("X"), c.key("Y"), c.key("r")),
            (None, None, None),
            "frame {n}"
        );
        let (s, v, x, y) = (c.num("s"), c.num("v"), c.num("x"), c.num("y"));
        assert_eq!(c.data.len(), s * v * 4, "frame {n}: its data");
        for row in 0..v {
            for col in 0..s {
                let p = &c.data[4 * (row * s + col)..4 * (row * s + col) + 4];
                match p[3] {
                    0 => {}
                    255 => frame[(y + row) * w + x + col] = [p[0], p[1], p[2], 255],
                    a => panic!("frame {n}: an alpha of {a}, past the premise"),
                }
            }
        }
        each(n, &frame);
    }
    assert_eq!(n, LOGO_FRAMES, "the premise, its frames");
}

/// The terminal the capture is replayed on: the capture's pty in
/// [`ICAT_IMAGES`], the capture fed in the tty's reads of 1024 bytes.
fn icat_term<R>(f: impl FnOnce(&mut Term<'_>, &[Cmd], ImageId, &str) -> R) -> R {
    let bytes = capture();
    let cmds = commands(&bytes);
    let mut owned = Owned::new(&ICAT_WS).with_images(ImageBudget::of(ICAT_IMAGES));
    let mut t = Term::new(owned.storage(), &ICAT_WS);
    t.set_cell_size(ImagePx::of(14), ImagePx::of(32));
    let replies = feed_by(&mut t, &bytes, 1024);
    assert!(
        replies.is_empty(),
        "q=2 throughout: {}",
        String::from_utf8_lossy(&replies)
    );
    let id = t.images().placements().next().expect("placed").1.image;
    let number = cmds[0].key("I").expect("an image number").to_string();
    f(&mut t, &cmds, id, &number)
}

/// The frame image `id` shows, as `[r, g, b, a]` a pixel, and its number.
fn shown(t: &Term<'_>, id: ImageId) -> (Vec<[u8; 4]>, FrameNo) {
    let v = t.images().image(id).expect("the image");
    let px = v
        .pixels
        .iter()
        .map(|p| {
            let (r, g, b) = p.rgb();
            [r.get(), g.get(), b.get(), p.alpha().get()]
        })
        .collect();
    (px, v.frame)
}

fn at(ms: u64) -> HostTime {
    HostTime::of_nanos(1_000_000_000 + ms * 1_000_000)
}

/// **kitten icat's logo film plays in 4 MiB**: every one of its 320 frames
/// stored and no command refused, and each frame, made the one shown by
/// `a=a,c=` and composed at the tick, the frame kitty's composition makes.
/// Fails on the store before frames were kept as records, which kept 50 of
/// them whole here and refused the other 270 with ENOSPC, unanswered under
/// `q=2`; and if a frame were kept as its data rather than the frame it
/// makes, the data's clear pixels then showing clear where the frame before
/// shows through.
#[test]
fn kitten_icats_logo_plays_in_4_mib() {
    icat_term(|t, cmds, id, number| {
        assert_eq!(t.kitty().refused, 0, "no command refused, ENOSPC none");
        assert_eq!(t.kitty().frames, 319, "every a=f made a frame");
        assert_eq!(t.images().images_refused, 0, "nothing refused");
        let (count, _, state) = t.images().kitty_frames(id).expect("the animation");
        assert_eq!(count, LOGO_FRAMES);
        assert_eq!(state, AnimState::Running, "s=3, last");
        let v = t.images().image(id).unwrap();
        assert_eq!((v.width.index(), v.height.index()), LOGO);
        let mut ms = 0;
        icat_frames(cmds, |n, want| {
            ms += 1;
            feed(t, &apc(&format!("a=a,I={number},c={n},q=2"), b""));
            t.animate_images(at(ms), ImageMotion::Hold);
            let (got, frame) = shown(t, id);
            assert_eq!(frame, FrameNo::of(n), "frame {n} shown");
            let wrong = got.iter().zip(want).filter(|(a, b)| a != b).count();
            assert_eq!(wrong, 0, "frame {n}: {wrong} pixels wrong");
        });
    });
}

/// **The film stores in 300,000 words or less**: its records, both
/// canvases, the row map and the room spare, 246,780 words read by the
/// `frame` bench, against 4,345,600 for its 320 frames whole. Fails if a
/// record kept icat's rectangle as ARGB, the rectangles alone 615,009
/// words counted from the capture, or if the frames were kept whole.
#[test]
fn kitten_icats_logo_stores_in_300_000_words() {
    icat_term(|t, _, id, _| {
        let words = t.images().stored_words(id).expect("stored");
        assert!(words <= 300_000, "{words} words stored");
    });
}

/// **A flood of `a=a,c=` composes once a drawn frame**: ten thousand jumps
/// across the film compose nothing as they arrive, the next frame is due at
/// once while the compose is owed, held as the clock is, and the tick
/// composes the last jump's frame once; a second tick composes none. Fails
/// if a jump composed as it arrived, ten thousand composes, or if a jump
/// on records left nothing due, the canvas then never composed.
#[test]
fn a_flood_of_icat_jumps_composes_once_a_drawn_frame() {
    icat_term(|t, cmds, id, number| {
        // Away from the first frame, the flood's last jump.
        feed(t, &apc(&format!("a=a,I={number},c=160,q=2"), b""));
        t.animate_images(at(0), ImageMotion::Hold);
        assert_eq!(shown(t, id).1, FrameNo::of(160), "the premise");
        let composed = t.images().frames_composed;
        let mut flood = Vec::new();
        for i in 0..10_000u32 {
            let n = (i * 37) % LOGO_FRAMES + 1;
            flood.extend(apc(&format!("a=a,I={number},c={n},q=2"), b""));
        }
        flood.extend(apc(&format!("a=a,I={number},c=1,q=2"), b""));
        assert!(feed_by(t, &flood, 4096).is_empty());
        assert_eq!(
            t.images().frames_composed,
            composed,
            "a jump composes nothing"
        );
        assert_eq!(
            t.next_image_frame(at(1)),
            Some(at(1)),
            "owed: due now, held"
        );
        assert_eq!(
            t.animate_images(at(1), ImageMotion::Hold),
            Animated::Changed
        );
        assert_eq!(
            t.images().frames_composed,
            composed + 1,
            "the tick composes once"
        );
        let mut root = Vec::new();
        icat_frames(cmds, |n, f| {
            if n == 1 {
                root = f.to_vec();
            }
        });
        assert!(shown(t, id).0 == root, "the last jump's frame, the first");
        assert_eq!(t.animate_images(at(1), ImageMotion::Hold), Animated::Same);
        assert_eq!(
            t.images().frames_composed,
            composed + 1,
            "nothing moved since"
        );
    });
}

/// An 8x8 image's commands: a root of 64 colours, then frames over the one
/// before, over one three back, over the background `Y`, of RGB data and
/// half-alpha data, at positions that move.
fn program() -> Vec<Vec<u8>> {
    let mut seed = 0x2545_f491_u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed.to_be_bytes()
    };
    let mut cmds = Vec::new();
    let root: Vec<u8> = (0..64u8)
        .flat_map(|i| [i * 4, 255 - i * 3, i ^ 0x5a, 255])
        .collect();
    cmds.push(apc("a=t,f=32,s=8,v=8,i=1,q=1", &b64(&root)));
    for k in 2..=48u32 {
        let (x, y) = ((k * 3) % 7, (k * 5) % 7);
        let alpha = match k % 4 {
            0 => 128,
            _ => 255,
        };
        let data: Vec<u8> = (0..4)
            .flat_map(|_| {
                let [r, g, b, _] = next();
                [r, g, b, alpha]
            })
            .collect();
        let keys = match k % 11 {
            // Over the background, only part of the image covered.
            5 => format!("a=f,f=32,s=2,v=2,x={x},y={y},Y=287454207,i=1,q=1"),
            // Over a frame three back, not the one before.
            7 => format!("a=f,f=32,s=2,v=2,x={x},y={y},c={},i=1,q=1", k - 3),
            _ => format!(
                "a=f,f=32,s=2,v=2,x={x},y={y},c={},z={},i=1,q=1",
                k - 1,
                10 + k
            ),
        };
        let cmd = match k % 13 {
            // RGB data.
            9 => {
                let rgb: Vec<u8> = data.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
                apc(&keys.replace("f=32", "f=24"), &b64(&rgb))
            }
            _ => apc(&keys, &b64(&data)),
        };
        cmds.push(cmd);
    }
    cmds
}

/// A terminal with `words` of image memory.
fn arena<R>(words: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(words));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    f(&mut t)
}

/// Every frame of image 1, each made the one shown and composed: the
/// frames whole show at the jump, the records at the tick.
fn every_frame(t: &mut Term<'_>) -> Vec<Vec<[u8; 4]>> {
    let id = t
        .images()
        .kitty_image(baud_domain::KittyId::of(1))
        .expect("stored");
    let (n, _, _) = t.images().kitty_frames(id).unwrap();
    (1..=n)
        .map(|k| {
            feed(t, &apc(&format!("a=a,i=1,c={k},q=2"), b""));
            t.animate_images(at(0), ImageMotion::Hold);
            let (px, frame) = shown(t, id);
            assert_eq!(frame, FrameNo::of(k));
            px
        })
        .collect()
}

/// **Frames kept as records are the frames kept whole**, through what
/// changes the frames others are made over: the same 48 frames in an
/// arena that holds them whole and in one that holds them only as records,
/// then `r=` editing frame 10, which frame 11 is made over, `a=c`
/// composing into frame 12, which 13 is made over, `d=f` deleting frame 20,
/// which 21 is made over, and the root, and a frame more -- every frame's
/// pixels equal after each. Fails if a frame made over another changed
/// when that one was edited, composed into or deleted, as kitty's do and
/// baud's must not, or if a record's base were numbered wrong once a frame
/// before it went.
#[test]
fn records_keep_every_frame_as_whole_frames_do() {
    let edits = [
        apc(
            "a=f,f=32,s=3,v=3,x=1,y=1,r=10,i=1,q=1",
            &b64(&[200, 30, 90, 255].repeat(9)),
        ),
        apc("a=c,i=1,r=30,c=12,w=3,h=2,X=4,Y=5,x=1,y=0,q=1", b""),
        apc("a=d,d=f,i=1,r=20", b""),
        apc("a=d,d=f,i=1", b""),
        apc(
            "a=f,f=32,s=2,v=2,x=6,y=6,c=3,i=1,q=1",
            &b64(&[9, 8, 7, 255].repeat(4)),
        ),
    ];
    let mut whole = Vec::new();
    arena(1 << 20, |t| {
        for c in program() {
            assert!(feed(t, &c).is_empty(), "{}", String::from_utf8_lossy(&c));
        }
        let id = t.images().kitty_image(baud_domain::KittyId::of(1)).unwrap();
        let words = t.images().stored_words(id).unwrap();
        assert!(words >= 48 * 64, "the premise: whole, {words} words");
        whole.push(every_frame(t));
        for e in &edits {
            assert!(feed(t, e).is_empty());
            whole.push(every_frame(t));
        }
    });
    let mut step = 0;
    arena(4096, |t| {
        for c in program() {
            let r = feed(t, &c);
            assert!(r.is_empty(), "{}", String::from_utf8_lossy(&r));
        }
        assert_eq!(t.kitty().frames, 47, "every frame made");
        let id = t.images().kitty_image(baud_domain::KittyId::of(1)).unwrap();
        let words = t.images().stored_words(id).unwrap();
        assert!(words < 48 * 64, "the premise: records, {words} words");
        assert!(every_frame(t) == whole[0], "as made");
        for e in &edits {
            step += 1;
            let r = feed(t, e);
            assert!(r.is_empty(), "{}", String::from_utf8_lossy(&r));
            assert!(
                every_frame(t) == whole[step],
                "after {}",
                String::from_utf8_lossy(e)
            );
        }
    });
    assert_eq!(step, edits.len());
}

/// Pixels from a seed: `n` of `[r, g, b, 255]`.
fn opaque(seed: &mut u32, n: usize) -> Vec<u8> {
    (0..n)
        .flat_map(|_| {
            *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [r, g, b, _] = seed.to_be_bytes();
            [r, g, b, 255]
        })
        .collect()
}

/// An 8x8 image put at the top left, cells of 2x2 pixels, its root of 64
/// colours, then 47 frames of 2x2 each over the one before, but frame 3,
/// which is over the root as frame 2 is.
fn rooted() -> Vec<Vec<u8>> {
    let mut seed = 0x9e37_79b9_u32;
    let root: Vec<u8> = (0..64u8)
        .flat_map(|i| [i * 4, 255 - i * 3, i ^ 0x5a, 255])
        .collect();
    let mut cmds = vec![
        b"\x1b[H".to_vec(),
        apc("a=T,f=32,s=8,v=8,i=1,q=1", &b64(&root)),
    ];
    for k in 2..=48u32 {
        let (x, y) = ((k * 3) % 7, (k * 5) % 7);
        let base = match k {
            3 => 1,
            _ => k - 1,
        };
        let keys = format!("a=f,f=32,s=2,v=2,x={x},y={y},c={base},i=1,q=1");
        cmds.push(apc(&keys, &b64(&opaque(&mut seed, 4))));
    }
    cmds
}

/// `r=48`'s `n`th edit: 3x3 of one colour at a place that moves.
fn edit_48(n: usize) -> Vec<u8> {
    let (x, y) = (n % 6, (n / 6) % 6);
    let c = u8::try_from(n % 251).unwrap();
    apc(
        &format!("a=f,f=32,s=3,v=3,x={x},y={y},r=48,i=1,q=1"),
        &b64(&[c, 255 - c, 90, 255].repeat(9)),
    )
}

/// A scroll under side margins through the image's columns: rows 1-10,
/// columns 3-10, up one row.
const MARGIN_SCROLL: &[u8] = b"\x1b[?69h\x1b[3;10s\x1b[1;10r\x1b[S";

/// **A delete takes its room from the dead records**: in an arena that
/// holds frame 48's `r=` edits until the next is refused -- each edit a
/// record after the last and the one before named by none -- `d=f` deletes
/// the root, which frames 2 and 3 are made over, so each needs a record of
/// its own pixels and the root's, the whole image, more than the room the
/// refused edit asked for and did not find. With the rows as they were,
/// and after a scroll under side margins has moved them, every frame left
/// equals the same frames kept whole, 47 of them, and no image is refused
/// or deleted for it. Fails, the delete refused and 48 frames left, if the
/// delete found its room in the arena alone (the break: `compact_records`
/// not called in `rebase_children`).
#[test]
fn a_delete_takes_its_room_from_the_dead_records() {
    let delete = apc("a=d,d=f,i=1", b"");
    for scroll in [false, true] {
        let mut edits = 0;
        let left = arena(4096, |t| {
            t.set_cell_size(ImagePx::of(2), ImagePx::of(2));
            for c in rooted() {
                let r = feed(t, &c);
                assert!(r.is_empty(), "{}", String::from_utf8_lossy(&r));
            }
            let id = t.images().kitty_image(baud_domain::KittyId::of(1)).unwrap();
            let words = t.images().stored_words(id).unwrap();
            assert!(words < 48 * 64, "the premise: records, {words} words");
            while feed(t, &edit_48(edits)).is_empty() {
                edits += 1;
                assert!(edits < 10_000, "the premise: the arena fills");
            }
            assert!(edits >= 8, "the premise: dead records, {edits} edits");
            if scroll {
                assert!(feed(t, MARGIN_SCROLL).is_empty());
            }
            let (refused, evicted) = (t.images().images_refused, t.images().images_evicted);
            assert!(feed(t, &delete).is_empty());
            assert_eq!(t.images().images_refused, refused, "the delete refused");
            assert_eq!(t.images().images_evicted, evicted, "an image deleted");
            assert_eq!(t.images().kitty_frames(id).unwrap().0, 47, "frames left");
            every_frame(t)
        });
        let whole = arena(1 << 20, |t| {
            t.set_cell_size(ImagePx::of(2), ImagePx::of(2));
            for c in rooted() {
                assert!(feed(t, &c).is_empty());
            }
            for n in 0..edits {
                assert!(feed(t, &edit_48(n)).is_empty());
            }
            if scroll {
                assert!(feed(t, MARGIN_SCROLL).is_empty());
            }
            assert!(feed(t, &delete).is_empty());
            every_frame(t)
        });
        assert!(left == whole, "scroll {scroll}: every frame as kept whole");
    }
}

/// **A side-margin scroll moves no kitty animation's rows**: 48 frames of
/// an 8x8 image kept as records, placed over cells 1 to 4, and scrolled up
/// a line inside the margins by `SU` under DECSLRM -- once with the left
/// margin at cell 3, across the placement, and once at cell 1, the
/// placement wholly inside -- keep their rows where the records put them,
/// `RowMap::Same`, and every frame its pixels: a kitty placement keeps the
/// text's rule (`ImageStore::scroll_rect`), so a frame command after the
/// scroll composes onto the records as they were made, which the frame
/// commands' debug assertion holds. Fails, across the margin, if a scroll
/// split or moved a kitty image within it (the break: `across` taking
/// kitty images).
#[test]
fn a_side_margin_scroll_moves_no_kitty_animations_rows() {
    for (what, margins) in [
        (
            "across the left margin",
            &b"\x1b[?69h\x1b[3;10s\x1b[1;10r"[..],
        ),
        ("wholly inside", b"\x1b[?69h\x1b[1;10s\x1b[1;10r"),
    ] {
        arena(4096, |t| {
            t.set_cell_size(ImagePx::of(2), ImagePx::of(2));
            for c in rooted() {
                assert!(feed(t, &c).is_empty());
            }
            let id = t.images().kitty_image(baud_domain::KittyId::of(1)).unwrap();
            let stored = t.images().stored_words(id).unwrap();
            assert!(
                stored < 48 * 64,
                "{what}: the premise, records: {stored} words"
            );
            assert_eq!(t.images().row_map(id), Some(RowMap::Same), "{what}: before");
            let before = every_frame(t);
            assert!(feed(t, margins).is_empty());
            assert!(feed(t, b"\x1b[S").is_empty());
            assert_eq!(t.images().images_refused, 0, "{what}: nothing refused");
            assert_eq!(
                t.images().row_map(id),
                Some(RowMap::Same),
                "{what}: the rows stay where the records put them"
            );
            assert!(every_frame(t) == before, "{what}: every frame as it was");
            assert!(
                t.images().placements().next().is_some(),
                "{what}: still placed"
            );
        });
    }
}
