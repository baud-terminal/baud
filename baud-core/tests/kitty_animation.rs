//! kitty's animation (`[[source: kitty-graphics]]`, "Animation"): frames
//! made by `a=f` on a canvas of the background `Y` or a base frame `c`,
//! edited in place by `r`, composed by `a=c`, deleted by `d=f` and `d=F`,
//! and run by `a=a` on the caller's clock (`Term::animate_images`), each
//! answered in kitty's shape -- an `a=f` with the frame's number in `r`,
//! an `a=a` only with an error (`[[source: kitty-graphics-c]]`
//! `finish_command_response`, `handle_animation_control_command()`). The
//! expected pixels are computed here, from kitty's blend
//! (`[[source: kitty-simd-string-h]]` `blend_pixel_over_straight()`) in
//! floating point as kitty computes it, never by the code under test.

use baud_core::*;
use baud_domain::{
    AnimState, Animated, DecodeBudget, DecodeProgress, FrameDepth, FrameNo, HostTime, ImageBudget,
    ImageMotion, ImagePx, KittyId, Pixel,
};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

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

/// RGBA bytes as base64, the control data `ctrl`.
fn rgba(ctrl: &str, px: &[[u8; 4]]) -> Vec<u8> {
    apc(ctrl, &b64(&px.concat()))
}

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

fn text(r: &[u8]) -> String {
    String::from_utf8_lossy(r).into_owned()
}

fn with_arena<R>(pixels: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(pixels));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    f(&mut t)
}

fn with<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    with_arena(1 << 16, f)
}

/// The image kitty id `id` names.
fn img(t: &Term<'_>, id: u32) -> baud_domain::ImageId {
    t.images().kitty_image(KittyId::of(id)).expect("stored")
}

/// Frame `n` of image `id`, as `[r, g, b, a]` a pixel.
fn frame(t: &Term<'_>, id: u32, n: u32) -> Vec<[u8; 4]> {
    t.images()
        .frame_pixels(img(t, id), FrameNo::of(n))
        .expect("a frame")
        .iter()
        .map(|p| {
            let (r, g, b) = p.rgb();
            [r.get(), g.get(), b.get(), p.alpha().get()]
        })
        .collect()
}

/// kitty's straight over, in floating point as kitty computes it.
fn over(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let (a, da) = (u32::from(src[3]), u32::from(dst[3]));
    let inv = 255 - a;
    let den = a * 255 + da * inv;
    if den == 0 {
        return dst;
    }
    let c = |k: usize| {
        let num = u32::from(src[k]) * a * 255 + u32::from(dst[k]) * da * inv;
        // Below 2^24, so exact as an `f32`, as kitty's `(float)num` is.
        let q = f64::from(num) as f32 / f64::from(den) as f32;
        (0..=255u8)
            .find(|&v| f32::from(v) == q.round_ties_even())
            .expect("a channel")
    };
    let y = den + 128;
    [c(0), c(1), c(2), u8::try_from((y + (y >> 8)) >> 8).unwrap()]
}

/// A 2x2 root, image `1`, its four pixels distinct and opaque.
const ROOT: [[u8; 4]; 4] = [
    [10, 20, 30, 255],
    [40, 50, 60, 255],
    [70, 80, 90, 255],
    [100, 110, 120, 255],
];

fn root(t: &mut Term<'_>) {
    assert!(feed(t, &rgba("a=t,f=32,s=2,v=2,i=1,q=2", &ROOT)).is_empty());
}

/// **A frame is made and answered with its number**, "`,r=`" as kitty's
/// `finish_command_response` adds it: a whole-image frame is the data as
/// sent, its gap 40 ms when `z` is not given, nothing when `z` is negative,
/// `z` otherwise; and a frame sent in `m=1` chunks of every size is the
/// same frame. On the commit before, nothing was answered and no frame
/// made.
#[test]
fn a_frame_is_made_and_answered_with_its_number() {
    let data = [
        [1, 2, 3, 200],
        [4, 5, 6, 255],
        [7, 8, 9, 0],
        [11, 12, 13, 128],
    ];
    let cmd = rgba("a=f,f=32,s=2,v=2,i=1", &data);
    for n in 1..=cmd.len() {
        with(|t| {
            root(t);
            let r = feed_by(t, &cmd, n);
            assert_eq!(text(&r), "\x1b_Gi=1,r=2;OK\x1b\\", "by {n}");
            assert_eq!(frame(t, 1, 2), data.to_vec(), "by {n}");
            assert_eq!(frame(t, 1, 1), ROOT.to_vec(), "by {n}: the root kept");
        });
    }
    let encoded = b64(&data.concat());
    for size in [4usize, 8, 12] {
        with(|t| {
            root(t);
            let mut v = Vec::new();
            let pieces: Vec<&[u8]> = encoded.chunks(size).collect();
            for (k, p) in pieces.iter().enumerate() {
                let more = if k + 1 == pieces.len() { 0 } else { 1 };
                let keys = match k {
                    0 => format!("a=f,f=32,s=2,v=2,i=1,m={more}"),
                    _ => format!("m={more}"),
                };
                v.extend(apc(&keys, p));
            }
            let r = feed(t, &v);
            assert_eq!(text(&r), "\x1b_Gi=1,r=2;OK\x1b\\", "chunks of {size}");
            assert_eq!(frame(t, 1, 2), data.to_vec(), "chunks of {size}");
        });
    }
    with(|t| {
        root(t);
        // The largest `z`, 2^31 - 1 ms, kept whole: a picture's frames take
        // a bit of the word for their input flag, a kitty frame's do not.
        let largest = ",z=2147483647";
        for (z, want, gap) in [
            ("", 2, 40),
            (",z=-5", 3, 0),
            (",z=7", 4, 7),
            (largest, 5, 2_147_483_647),
        ] {
            let r = feed(t, &rgba(&format!("a=f,f=32,s=2,v=2,i=1{z}"), &data));
            assert_eq!(text(&r), format!("\x1b_Gi=1,r={want};OK\x1b\\"));
            let g = t.images().frame_gap(img(t, 1), FrameNo::of(want)).unwrap();
            assert_eq!(g, (gap, FrameDepth::Rgba), "{z:?}");
        }
        let g = t.images().frame_gap(img(t, 1), FrameNo::ROOT).unwrap();
        assert_eq!(g.0, 0, "the root frame defaults to zero gap");
    });
}

/// **A partial frame is composed onto its canvas**: the background `Y`,
/// "a 32-bit RGBA color", where the data does not reach, and the data
/// blended over it where it does; or frame `c`'s pixels under it; or with
/// `X=1` replacing them. The expected pixels are kitty's blend computed
/// here.
#[test]
fn a_partial_frame_is_composed_onto_its_canvas() {
    let px = [200, 100, 50, 128];
    // Y: 0x112233ff, opaque.
    with(|t| {
        root(t);
        let r = feed(t, &rgba("a=f,f=32,s=1,v=1,x=1,y=1,Y=287454207,i=1", &[px]));
        assert_eq!(text(&r), "\x1b_Gi=1,r=2;OK\x1b\\");
        let bg = [0x11, 0x22, 0x33, 0xff];
        assert_eq!(frame(t, 1, 2), vec![bg, bg, bg, over(bg, px)]);
    });
    // No `Y`: transparent black, and the data over it is the data.
    with(|t| {
        root(t);
        feed(t, &rgba("a=f,f=32,s=1,v=1,x=1,i=1", &[px]));
        let clear = [0, 0, 0, 0];
        assert_eq!(frame(t, 1, 2), vec![clear, over(clear, px), clear, clear]);
    });
    // On frame 1, blended, and replaced with `X=1`.
    with(|t| {
        root(t);
        feed(t, &rgba("a=f,f=32,s=1,v=1,y=1,c=1,i=1", &[px]));
        let mut want = ROOT.to_vec();
        want[2] = over(ROOT[2], px);
        assert_eq!(frame(t, 1, 2), want, "blended over frame 1");
        feed(t, &rgba("a=f,f=32,s=1,v=1,y=1,c=1,X=1,i=1", &[px]));
        want[2] = px;
        assert_eq!(frame(t, 1, 3), want, "replacing frame 1's");
    });
    // An RGB root keeps no alpha: data replacing its pixels gives its
    // colour alone, as kitty's three-byte frame keeps none.
    with(|t| {
        let rgb: Vec<u8> = ROOT.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        feed(t, &apc("a=t,f=24,s=2,v=2,i=1,q=2", &b64(&rgb)));
        feed(t, &rgba("a=f,f=32,s=1,v=1,c=1,X=1,i=1", &[px]));
        assert_eq!(frame(t, 1, 2)[0], [200, 100, 50, 255]);
    });
}

/// **`r` edits a frame in place**, the root too, answered with its
/// number, its gap changed only by a `z` given.
#[test]
fn r_edits_a_frame_in_place() {
    with(|t| {
        root(t);
        let px = [9, 9, 9, 255];
        let r = feed(t, &rgba("a=f,f=32,s=1,v=1,x=1,r=1,i=1", &[px]));
        assert_eq!(text(&r), "\x1b_Gi=1,r=1;OK\x1b\\");
        let mut want = ROOT.to_vec();
        want[1] = px;
        assert_eq!(frame(t, 1, 1), want);
        let id = img(t, 1);
        assert_eq!(t.images().kitty_frames(id).unwrap().0, 1, "no frame added");
        assert_eq!(t.images().frame_gap(id, FrameNo::ROOT).unwrap().0, 0);
        feed(t, &rgba("a=f,f=32,s=1,v=1,r=1,z=30,i=1", &[px]));
        assert_eq!(t.images().frame_gap(id, FrameNo::ROOT).unwrap().0, 30);
    });
}

/// **Every frame refusal in kitty's code and shape**, nothing added: data
/// wider or taller than the image, a base frame that does not exist, too
/// little data, a payload that is not base64 and a PNG that does not
/// decode -- each reply's `r` the number the frame would have had, as kitty
/// resolves it before it loads anything -- and an image that does not
/// exist, whose reply keeps the client's `r`; naming no image is answered
/// nothing.
#[test]
fn every_frame_refusal_is_answered_in_kittys_shape() {
    let one = [[1u8, 2, 3, 255]];
    let three: Vec<[u8; 4]> = vec![[1, 2, 3, 255]; 3];
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (
            rgba("a=f,f=32,s=3,v=1,i=1", &three),
            "\x1b_Gi=1,r=2;EINVAL:Frame width larger than image width\x1b\\",
        ),
        (
            rgba("a=f,f=32,s=1,v=3,i=1", &three),
            "\x1b_Gi=1,r=2;EINVAL:Frame height larger than image height\x1b\\",
        ),
        (
            rgba("a=f,f=32,s=1,v=1,c=5,i=1", &one),
            "\x1b_Gi=1,r=2;EINVAL:No frame with that number found\x1b\\",
        ),
        (
            rgba("a=f,f=32,s=1,v=1,r=4,i=9", &one),
            "\x1b_Gi=9,r=4;ENOENT:Animation command refers to non-existent image\x1b\\",
        ),
        (rgba("a=f,f=32,s=1,v=1", &one), ""),
        // After the image is found, every error carries the frame's number.
        (
            rgba("a=f,f=32,s=2,v=2,i=1", &three),
            "\x1b_Gi=1,r=2;ENODATA:Insufficient image data\x1b\\",
        ),
        (
            apc("a=f,f=32,s=1,v=1,r=7,i=1", b"A!!A"),
            "\x1b_Gi=1,r=2;EINVAL:Payload is not valid base64\x1b\\",
        ),
        (
            apc("a=f,f=100,i=1", &b64(b"not a png at all")),
            "\x1b_Gi=1,r=2;EBADPNG:Failed to decode PNG data\x1b\\",
        ),
    ];
    for (cmd, want) in cases {
        for n in 1..=cmd.len() {
            with(|t| {
                root(t);
                let r = feed_by(t, &cmd, n);
                assert_eq!(text(&r), want, "by {n}");
                assert_eq!(t.images().kitty_frames(img(t, 1)).unwrap().0, 1, "{want}");
            });
        }
    }
}

/// **`a=c` composes a rectangle of one frame onto another**, kitty's
/// `handle_compose_command`: blended, or replaced with `C=1`; ENOENT for a
/// frame neither names, EINVAL for a rectangle out of bounds and for one
/// frame's two rectangles overlapping. Answered `OK`.
#[test]
fn a_composition_copies_a_rectangle_between_frames() {
    let half = [[200u8, 100, 50, 128]; 4];
    with(|t| {
        root(t);
        feed(t, &rgba("a=f,f=32,s=2,v=2,i=1,q=2", &half));
        let r = feed(t, &apc("a=c,i=1,r=2,c=1,w=1,h=1,X=1,Y=0,x=0,y=1", b""));
        assert_eq!(text(&r), "\x1b_Gi=1;OK\x1b\\");
        let mut want = ROOT.to_vec();
        want[2] = over(ROOT[2], half[1]);
        assert_eq!(frame(t, 1, 1), want, "blended");
        feed(t, &apc("a=c,i=1,r=2,c=1,w=1,h=1,C=1,q=2", b""));
        want[0] = half[0];
        assert_eq!(frame(t, 1, 1), want, "replaced");
        // One frame, rectangles apart: allowed.
        let r = feed(t, &apc("a=c,i=1,r=1,c=1,w=1,h=2,X=0,x=1", b""));
        assert_eq!(text(&r), "\x1b_Gi=1;OK\x1b\\");
        for (keys, want) in [
            ("r=5,c=1", "ENOENT:No source frame number exists in image"),
            (
                "r=1,c=5",
                "ENOENT:No destination frame number exists in image",
            ),
            (
                "r=1,c=2,w=2,x=1",
                "EINVAL:The destination rectangle is out of bounds",
            ),
            (
                "r=1,c=2,w=2,X=1",
                "EINVAL:The source rectangle is out of bounds",
            ),
            (
                "r=1,c=1,w=2,h=2",
                "EINVAL:The source and destination rectangles overlap and the src and destination frames are the same",
            ),
        ] {
            let before = (frame(t, 1, 1), frame(t, 1, 2));
            let r = feed(t, &apc(&format!("a=c,i=1,{keys}"), b""));
            assert_eq!(text(&r), format!("\x1b_Gi=1;{want}\x1b\\"), "{keys}");
            assert_eq!(
                (frame(t, 1, 1), frame(t, 1, 2)),
                before,
                "{keys}: untouched"
            );
        }
        let r = feed(t, &apc("a=c,i=7,r=1,c=1", b""));
        assert_eq!(
            text(&r),
            "\x1b_Gi=7;ENOENT:Animation command refers to non-existent image\x1b\\"
        );
    });
}

/// **`a=a`, `a=c` and `d=f` do the same at every read size**: each
/// command's reply and what it leaves -- every frame's pixels and gap, the
/// frame shown, the state -- fed whole and in reads of each size.
#[test]
fn animation_commands_are_the_same_at_every_read_size() {
    let state = |t: &Term<'_>| {
        let id = img(t, 1);
        let (n, shown, st) = t.images().kitty_frames(id).unwrap();
        let frames: Vec<_> = (1..=n)
            .map(|k| (frame(t, 1, k), t.images().frame_gap(id, FrameNo::of(k))))
            .collect();
        (frames, shown, st, t.images().frame_loops(id))
    };
    for cmd in [
        apc("a=a,i=1,c=3,r=2,z=70,s=2,v=3", b""),
        apc("a=a,i=9,r=2", b""),
        apc("a=c,i=1,r=2,c=1,w=1,h=2,X=1,x=0", b""),
        apc("a=c,i=1,r=1,c=1,w=2,h=2", b""),
        apc("a=d,d=f,i=1,r=3", b""),
        apc("a=d,d=F,i=1", b""),
    ] {
        let whole = with(|t| {
            four(t);
            let r = feed(t, &cmd);
            (r, state(t))
        });
        for n in 1..cmd.len() {
            let cut = with(|t| {
                four(t);
                let r = feed_by(t, &cmd, n);
                (r, state(t))
            });
            assert!(cut == whole, "{} by {n}", text(&cmd));
        }
    }
}

/// Image 1 with frames 2, 3 and 4, each a solid colour, gaps 10, 20, 30.
fn four(t: &mut Term<'_>) {
    root(t);
    for (k, z) in [(2u8, 10), (3, 20), (4, 30)] {
        let r = feed(
            t,
            &rgba(&format!("a=f,f=32,s=2,v=2,i=1,z={z},q=2"), &[[k; 4]; 4]),
        );
        assert!(r.is_empty());
    }
}

/// **`d=f` deletes a frame, `d=F` an image of one frame**: frame `r` goes
/// and the ones after it move down a number; no `r` is the root, whose
/// place frame 2 takes, gap and all; a frame past the last is the last.
/// An image of one frame keeps it under `d=f` and goes under `d=F`. No
/// reply, as no delete has one.
#[test]
fn a_frame_delete_takes_the_frame_it_names() {
    with(|t| {
        four(t);
        let r = feed(t, &apc("a=d,d=f,i=1,r=3", b""));
        assert!(r.is_empty());
        let id = img(t, 1);
        assert_eq!(t.images().kitty_frames(id).unwrap().0, 3);
        assert_eq!(frame(t, 1, 3), vec![[4; 4]; 4], "frame 4 is frame 3 now");
        assert_eq!(t.images().frame_gap(id, FrameNo::of(3)).unwrap().0, 30);
        feed(t, &apc("a=d,d=f,i=1", b""));
        assert_eq!(
            frame(t, 1, 1),
            vec![[2; 4]; 4],
            "frame 2 took the root's place"
        );
        assert_eq!(t.images().frame_gap(id, FrameNo::ROOT).unwrap().0, 10);
        feed(t, &apc("a=d,d=F,i=1,r=99", b""));
        assert_eq!(t.images().kitty_frames(id).unwrap().0, 1, "the last went");
        feed(t, &apc("a=d,d=f,i=1", b""));
        assert!(
            t.images().kitty_image(KittyId::of(1)).is_some(),
            "d=f keeps one"
        );
        feed(t, &apc("a=d,d=F,i=1", b""));
        assert!(
            t.images().kitty_image(KittyId::of(1)).is_none(),
            "d=F frees it"
        );
    });
}

/// **`a=a` controls the animation and answers only an error**: `c` makes
/// a frame current, which is what the image shows; `r` with `z` sets a
/// frame's gap; `s` and `v` set the state and the loops. An image that
/// does not exist is ENOENT, its `r` kept.
#[test]
fn animation_control_answers_only_an_error() {
    with(|t| {
        four(t);
        let id = img(t, 1);
        let r = feed(t, &apc("a=a,i=1,c=3", b""));
        assert!(r.is_empty(), "{}", text(&r));
        assert_eq!(t.images().kitty_frames(id).unwrap().1, FrameNo::of(3));
        let shown: Vec<Pixel> = t.images().image(id).unwrap().pixels.to_vec();
        assert!(
            shown.iter().all(|p| p.rgb().0.get() == 3),
            "frame 3 is what shows"
        );
        feed(t, &apc("a=a,i=1,r=2,z=-3", b""));
        assert_eq!(t.images().frame_gap(id, FrameNo::of(2)).unwrap().0, 0);
        feed(t, &apc("a=a,i=1,s=3,v=4", b""));
        assert_eq!(t.images().kitty_frames(id).unwrap().2, AnimState::Running);
        assert_eq!(t.images().frame_loops(id), Some((0, 3)));
        let r = feed(t, &apc("a=a,i=5,r=2,s=1", b""));
        assert_eq!(
            text(&r),
            "\x1b_Gi=5,r=2;ENOENT:Animation command refers to non-existent image\x1b\\"
        );
    });
}

fn at(ms: u64) -> HostTime {
    HostTime::of_nanos(1_000_000_000 + ms * 1_000_000)
}

/// **The clock runs a placed animation as kitty's
/// `scan_active_animations` does**: a frame stamped at the first tick,
/// the next shown once its gap has passed and not before, a gapless frame
/// skipped, a loop counted at the wrap and the last one played stopping
/// at the last frame. An image with no placement does not run, as kitty
/// runs only what it draws.
#[test]
fn the_clock_runs_a_placed_animation() {
    with(|t| {
        four(t);
        let id = img(t, 1);
        feed(t, &apc("a=a,i=1,r=1,z=50,s=3,v=2", b""));
        let shown = |t: &Term<'_>| t.images().kitty_frames(id).unwrap().1.raw();
        assert_eq!(t.animate_images(at(0), ImageMotion::Play), Animated::Same);
        assert_eq!(t.next_image_frame(at(0)), None, "not placed, not run");
        feed(t, &apc("a=p,i=1,q=2", b""));
        assert_eq!(
            t.animate_images(at(0), ImageMotion::Play),
            Animated::Same,
            "stamped"
        );
        assert_eq!(t.next_image_frame(at(0)), Some(at(50)));
        assert_eq!(t.animate_images(at(49), ImageMotion::Play), Animated::Same);
        assert_eq!(
            t.animate_images(at(50), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(shown(t), 2);
        // Frame 3 gapless: skipped to 4.
        feed(t, &apc("a=a,i=1,r=3,z=-1", b""));
        assert_eq!(
            t.animate_images(at(60), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(shown(t), 4);
        // The wrap: one loop of `v=2`'s one played, it stops at frame 4.
        assert_eq!(t.animate_images(at(90), ImageMotion::Play), Animated::Same);
        assert_eq!(shown(t), 4);
        assert_eq!(t.images().frame_loops(id), Some((1, 1)));
        assert_eq!(t.next_image_frame(at(90)), None, "nothing more to run");
        // `v=1`, for ever, its loops counted from none again: on from frame
        // 4, whose gap ended at 90, to the root.
        feed(t, &apc("a=a,i=1,s=3,v=1", b""));
        assert_eq!(
            t.animate_images(at(100), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(shown(t), 1);
    });
}

/// **A kitty image's last loop ends on its last frame with a gap**, a
/// gapless last frame passed over and not shown: kitty's
/// scan_active_animations() walks to it and leaves at the wrap before
/// update_current_frame() draws it. A picture's last loop is not this
/// (`a_gif_that_ends_on_a_zero_delay_ends_showing_it`).
#[test]
fn a_last_loop_passes_over_a_gapless_last_frame() {
    with(|t| {
        four(t);
        let id = img(t, 1);
        feed(t, &apc("a=a,i=1,r=1,z=50,s=3,v=2", b""));
        feed(t, &apc("a=a,i=1,r=4,z=-1", b""));
        feed(t, &apc("a=p,i=1,q=2", b""));
        let shown = |t: &Term<'_>| t.images().kitty_frames(id).unwrap().1.raw();
        t.animate_images(at(0), ImageMotion::Play);
        t.animate_images(at(50), ImageMotion::Play);
        t.animate_images(at(60), ImageMotion::Play);
        assert_eq!(shown(t), 3);
        assert_eq!(t.animate_images(at(80), ImageMotion::Play), Animated::Same);
        assert_eq!(shown(t), 3, "frame 4 passed over");
        assert_eq!(t.images().frame_loops(id), Some((1, 1)));
        assert_eq!(t.next_image_frame(at(80)), None, "nothing more to run");
    });
}

/// **Loading mode waits at the last frame for the next to arrive**, and
/// goes on to it when it does.
#[test]
fn loading_waits_at_the_last_frame() {
    with(|t| {
        four(t);
        let id = img(t, 1);
        feed(t, &apc("a=p,i=1,q=2", b""));
        feed(t, &apc("a=a,i=1,c=4,s=2", b""));
        t.animate_images(at(0), ImageMotion::Play);
        assert_eq!(
            t.animate_images(at(40), ImageMotion::Play),
            Animated::Same,
            "waits"
        );
        assert_eq!(t.images().kitty_frames(id).unwrap().1.raw(), 4);
        assert_eq!(t.next_image_frame(at(40)), None, "waiting is not due");
        feed(t, &rgba("a=f,f=32,s=2,v=2,i=1,q=2", &[[5; 4]; 4]));
        assert_eq!(
            t.animate_images(at(41), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(t.images().kitty_frames(id).unwrap().1.raw(), 5);
    });
}

/// **Held, nothing moves on the clock and a client's `c` still shows**;
/// run again, the frame shown gets its whole gap from then (Jev's lean,
/// HOLD_CURRENT).
#[test]
fn a_held_clock_moves_nothing_and_the_client_still_does() {
    with(|t| {
        four(t);
        let id = img(t, 1);
        feed(t, &apc("a=p,i=1,q=2", b""));
        feed(t, &apc("a=a,i=1,r=1,z=50,s=3", b""));
        t.animate_images(at(0), ImageMotion::Play);
        assert_eq!(t.animate_images(at(500), ImageMotion::Hold), Animated::Same);
        assert_eq!(t.next_image_frame(at(500)), None);
        assert_eq!(t.images().kitty_frames(id).unwrap().1.raw(), 1, "held");
        feed(t, &apc("a=a,i=1,c=2", b""));
        assert_eq!(
            t.images().kitty_frames(id).unwrap().1.raw(),
            2,
            "the client's"
        );
        assert_eq!(
            t.animate_images(at(600), ImageMotion::Play),
            Animated::Same,
            "stamped"
        );
        assert_eq!(t.animate_images(at(609), ImageMotion::Play), Animated::Same);
        assert_eq!(
            t.animate_images(at(610), ImageMotion::Play),
            Animated::Changed
        );
    });
}

/// **Frames are bounded by the arena**: a frame no gap holds is ENOSPC,
/// and an image with no placement is deleted for it first -- never the
/// animation's own image.
#[test]
fn frames_are_bounded_by_the_arena() {
    // 4 for the root, 4 for each frame and a word each, and a 2x2 frame's
    // data on its way: an arena of 24 holds the root and two frames.
    with_arena(24, |t| {
        root(t);
        let f = rgba("a=f,f=32,s=2,v=2,i=1", &[[7; 4]; 4]);
        let mut answers = Vec::new();
        for _ in 0..4 {
            answers.push(text(&feed(t, &f)));
        }
        assert!(answers[0].ends_with(";OK\x1b\\"), "{answers:?}");
        assert!(
            answers
                .iter()
                .any(|a| a.contains("ENOSPC:Failed to cache data for image frame")),
            "{answers:?}"
        );
        assert!(
            t.images().kitty_image(KittyId::of(1)).is_some(),
            "the root stays"
        );
    });
    // Image 2, 4x4 and placed nowhere, first in the arena: the second
    // frame's room is there once it goes.
    with_arena(40, |t| {
        assert!(feed(t, &rgba("a=t,f=32,s=4,v=4,i=2,q=2", &[[3; 4]; 16])).is_empty());
        root(t);
        let f = rgba("a=f,f=32,s=2,v=2,i=1", &[[7; 4]; 4]);
        let _ = feed(t, &f);
        let r = feed(t, &f);
        assert!(text(&r).ends_with(";OK\x1b\\"), "{}", text(&r));
        assert!(
            t.images().kitty_image(KittyId::of(2)).is_none(),
            "image 2 made room"
        );
        assert!(t.images().kitty_image(KittyId::of(1)).is_some());
    });
}

/// **Re-sending the image deletes its frames with it**: "When
/// re-transmitting image data for a specific id, the existing image and
/// all its placements must be deleted".
#[test]
fn a_retransmission_drops_the_frames() {
    with(|t| {
        four(t);
        root(t);
        assert_eq!(t.images().kitty_frames(img(t, 1)).unwrap().0, 1);
    });
}

/// A PNG frame, through the deferred decode, is a frame as raw data is.
#[test]
fn a_png_frame_is_a_frame() {
    let data = [
        [1u8, 2, 3, 200],
        [4, 5, 6, 255],
        [7, 8, 9, 0],
        [11, 12, 13, 128],
    ];
    let mut png = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png, 2, 2);
        e.set_color(png::ColorType::Rgba);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&data.concat()).unwrap();
    }
    with(|t| {
        root(t);
        let r = feed(t, &apc("a=f,f=100,i=1", &b64(&png)));
        assert_eq!(text(&r), "\x1b_Gi=1,r=2;OK\x1b\\");
        assert_eq!(frame(t, 1, 2), data.to_vec());
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&data.concat(), 6);
        let r = feed(t, &apc("a=f,f=32,s=2,v=2,o=z,i=1", &b64(&z)));
        assert_eq!(text(&r), "\x1b_Gi=1,r=3;OK\x1b\\");
        assert_eq!(frame(t, 1, 3), data.to_vec());
    });
}
