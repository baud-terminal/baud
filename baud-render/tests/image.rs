//! The CPU compositor draws a Sixel placement, and it moves with the scroll.
//!
//! A dedicated test rather than a row in `golden.rs`, deliberately: that
//! harness provisions no image memory (`Owned::new`), so a Sixel there would
//! pin the digest of a *refused* image -- a gate that passes while proving the
//! opposite of what it claims. This one provisions the store and asserts the
//! pixels drew before trusting any digest.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Chan, Codepoint, Col, Cols, HostTime, ImageAlpha, ImageBudget, ImageMotion, ImagePx,
    Pixel, ReadLen, Row, Rows,
};
use baud_font::{Face, Px};
use baud_render::counters::{self, Counter, Site};
use baud_render::{
    Blend, Damage, Field, FindLook, Frame, Hints, ImagePlacement, ImageScale, LinkLook, PxSize,
    Renderer, Stack, Theme, Trail, TrailLook,
};
use support::{Canvas, WS, font, renderer};

/// A 1x6 red Sixel, the smallest well-formed image, for the second-image case.
const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";

const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// How many pixels in the 64x48 rect at the top-left differ from the theme
/// background -- the image's own pixels, drawn over the cells.
fn painted_in_image_rect(c: &Canvas) -> usize {
    let bg = Theme::dark().bg.0;
    let mut n = 0;
    for y in 0..48 {
        for x in 0..64 {
            if c.pixels[y * c.width + x] != bg {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn a_sixel_is_drawn_over_the_grid() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);

    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }

    // The 64x48 swirl anchors at the cursor (row 0, col 0) and fills most of
    // its rect -- assert the pixels *before* any digest, so "the image drew"
    // is distinguished from "the frame is uniformly background".
    let painted = painted_in_image_rect(&c);
    assert!(
        painted > 64 * 48 / 2,
        "a gradient swirl paints most of its 3072 pixels, not {painted}"
    );
}

#[test]
fn a_sixel_scrolls_off_and_the_frame_changes() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);

    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let before = painted_in_image_rect(&c);
    let digest_before = c.digest();
    assert!(before > 64 * 48 / 2, "the image drew first, {before}");

    // Scroll the screen well past the image: move the cursor to the bottom and
    // scroll far enough that the image (anchored at line 0) is above the top.
    feed(&mut t, &[b'\n'; 200]);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let after = painted_in_image_rect(&c);
    let digest_after = c.digest();

    // The image scrolled off, so its rect is background again -- and the frame
    // changed, which a blit that silently drew nothing could not produce.
    assert_eq!(after, 0, "the image scrolled off its rect, {after} left");
    assert_ne!(
        digest_before, digest_after,
        "the frame changed with the scroll"
    );
}

/// The frame `swirl.raw` paints at home, as a digest -- `s-sixel` gate 2,
/// the regression net the painted-pixel count above cannot be: a count holds
/// through a flipped image, a register read one entry off or a band drawn a
/// row late, and a digest does not. Taken on the first run and not an
/// oracle, which is the decoder's own gate; regenerate with
/// `GOLDEN_PRINT=1`, and `GOLDEN_DUMP=<dir>` writes the frame to look at.
const SWIRL_FRAME: u64 = 0x22cfc4f36d316b7b;

/// Writes the frame as a binary PPM: `golden.rs`'s shape, its three bytes
/// taken whole rather than cast one at a time.
fn dump_ppm(c: &Canvas, path: &str) {
    let mut out = format!("P6\n{} {}\n255\n", c.width, c.height).into_bytes();
    for &p in &c.pixels {
        out.extend_from_slice(&p.to_be_bytes()[1..]);
    }
    std::fs::write(path, out).unwrap();
}

#[test]
fn a_sixel_paints_to_its_pinned_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    // The pixels before the digest, so a pinned frame of a refused image --
    // a background and a cursor -- cannot be what this pins.
    let painted = painted_in_image_rect(&c);
    assert!(painted > 64 * 48 / 2, "the swirl drew, {painted} pixels");
    if let Ok(dir) = std::env::var("GOLDEN_DUMP") {
        dump_ppm(&c, &format!("{dir}/sixel-swirl.ppm"));
    }
    let got = c.digest();
    if std::env::var("GOLDEN_PRINT").is_ok() {
        println!("const SWIRL_FRAME: u64 = 0x{got:016x};");
    } else {
        assert_eq!(got, SWIRL_FRAME, "the swirl's frame digest 0x{got:016x}");
    }
}

/// An image placed on row 5 and the screen scrolled one line draws one cell
/// row higher, pixel for pixel -- `s-sixel` gate 3's first clause. The test
/// above scrolls the image clean off, which a placement that ignored the
/// scroll by a row would also pass once enough lines went by; this is the
/// one-line case. The cursor is hidden and parked on the bottom row, so the
/// only thing in the image's rect is the image.
#[test]
fn a_sixel_scrolled_one_line_draws_one_row_higher() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[6;1H");
    feed(&mut t, SWIRL);
    feed(&mut t, b"\x1b[50;1H");
    let rect = |c: &Canvas, top: usize| -> Vec<u32> {
        (0..48)
            .flat_map(|y| c.pixels[(top + y) * c.width..][..64].to_vec())
            .collect()
    };
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let before = rect(&c, 5 * ch);
    let above_before = rect(&c, 4 * ch);
    let bg = Theme::dark().bg.0;
    let painted = before.iter().filter(|&&p| p != bg).count();
    assert!(
        painted > 64 * 48 / 2,
        "the swirl drew on row 5, {painted} pixels"
    );

    feed(&mut t, b"\n");
    assert_eq!(
        t.image_origin(Rows::ZERO).top.value(),
        1,
        "one line feed on the bottom row is one scroll"
    );
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    // A comparison that cannot fail by being the same frame twice: the rect
    // one row up held something else before the scroll.
    assert_ne!(
        above_before, before,
        "the rect one row up was not the image already"
    );
    assert_eq!(
        rect(&c, 4 * ch),
        before,
        "the image did not move exactly one cell row up with its line"
    );
}

/// The text of a view row, blanks trimmed.
fn row_text(t: &Term, back: Rows, y: usize) -> String {
    t.view(back)
        .row(baud_domain::Row::from_index(y))
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|p| char::from_u32(p.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// **A shorter screen retires the rows above the cursor into the history, and
/// a reader scrolled back to them finds the image on the row it was drawn
/// on.** The history here is deeper than the count of scrolls -- sixty scrolls,
/// then thirty rows retired by the shrink -- which is the case a renderer
/// computing its own `scrolled - back` put thirty rows off, because a shrink
/// retires rows without scrolling.
#[test]
fn a_scrolled_back_image_after_a_shrink_draws_on_its_row() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &[b'\n'; 60]);
    feed(&mut t, b"\x1b[1;1HMARK\r\n");
    feed(&mut t, SWIRL);
    feed(&mut t, b"\x1b[50;1H");
    let short = baud_domain::WinSize {
        ws_row: 20,
        ws_col: WS.ws_col,
    };
    assert_eq!(t.resize(&short), baud_core::Resized::Ok);
    let back = Rows::of_count(30);
    assert_eq!(
        row_text(&t, back, 0),
        "MARK",
        "the view 30 up starts at MARK"
    );
    r.set_images(&t, back);
    let p = r.image_placements();
    assert_eq!(p.len(), 1, "the image is in that view");
    assert_eq!(i32::from(p[0].y), ch, "one row below MARK");
}

/// **Each screen draws only its own images**, and the editor's go when it
/// leaves: the shell's image is not drawn over a full-screen program, and is
/// back where it was when the program exits.
#[test]
fn the_shells_image_is_not_drawn_over_the_editor() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 128));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    r.set_images(&t, Rows::ZERO);
    assert_eq!(r.image_placements().len(), 1, "the shell's image");
    feed(&mut t, b"\x1b[?1049h");
    r.set_images(&t, Rows::ZERO);
    assert!(
        r.image_placements().is_empty(),
        "the shell's image is not drawn on the editor's screen"
    );
    feed(&mut t, b"\x1b[10;1H");
    feed(&mut t, TINY);
    r.set_images(&t, Rows::ZERO);
    let p = r.image_placements();
    assert_eq!(p.len(), 1, "the editor's own image");
    assert_eq!(i32::from(p[0].y), 9 * ch, "on the row the editor put it");
    feed(&mut t, b"\x1b[?1049l");
    r.set_images(&t, Rows::ZERO);
    let p = r.image_placements();
    assert_eq!(p.len(), 1, "the editor's image went with its screen");
    assert_eq!((p[0].y, p[0].w, p[0].h), (0, 64, 48), "the shell's is back");
}

#[test]
fn a_static_image_is_snapshotted_once_not_per_frame() {
    // `set_images` runs every frame; a large image's per-frame pixel snapshot
    // (a `Vec` allocation plus a per-pixel conversion) is real cost while it is
    // on screen. The snapshot cache is kept across frames and rebuilt only for
    // a new `(id, epoch)`, so an unchanged image on screen is snapshotted once
    // however many frames it draws -- which a clear-every-frame renderer is not.
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 128));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);

    for _ in 0..5 {
        r.set_images(&t, Rows::ZERO);
    }
    assert_eq!(
        r.image_snapshots().value(),
        1,
        "the static image was snapshotted once over five frames, not five times"
    );

    // A new image bumps the store epoch, which drops the snapshot cache, so the
    // next frame re-snapshots what is referenced -- the count rises.
    feed(&mut t, TINY);
    r.set_images(&t, Rows::ZERO);
    assert!(
        r.image_snapshots().value() > 1,
        "a store's epoch change re-snapshots, not {}",
        r.image_snapshots().value()
    );
    assert!(
        !r.image_uploads().is_empty(),
        "the images are cached on screen"
    );

    // Scroll both images off the top. A build counter cannot see a leak, so the
    // cache length is the observable: an off-screen image's snapshot is dropped
    // rather than held until the next epoch change, which on a terminal showing
    // one image is forever.
    feed(&mut t, &[b'\n'; 60]);
    r.set_images(&t, Rows::ZERO);
    assert!(
        r.image_placements().is_empty(),
        "the images scrolled off screen"
    );
    assert!(
        r.image_uploads().is_empty(),
        "and their snapshots were dropped, not retained"
    );
}

/// The frame an incremental render leaves, against a fresh full repaint of
/// the same terminal by a second renderer: the two must be pixel for pixel
/// equal, or incremental damage left something stale or drew something
/// twice. The rule `incremental_frames_equal_full_repaints_at_every_read_boundary`
/// holds the CPU path to, with an image.
fn equals_a_full_repaint(t: &mut Term, c: &Canvas, faces: &[Face]) -> bool {
    c.pixels == drawn_whole(t, faces)
}

/// **The screen `t` shows now, drawn whole by a renderer of its own.** What
/// a change's premise "the frame changed" is measured on, before the change
/// and after it: measured on the renderer under test, a break that leaves
/// its incremental frame unchanged went red at the premise, before the
/// claim it breaks.
fn drawn_whole(t: &mut Term, faces: &[Face]) -> Vec<u32> {
    let mut fresh = renderer(faces);
    let mut f = Canvas::for_renderer(&fresh);
    {
        let mut target = f.target();
        fresh.render(t, &mut target, Damage::Full);
    }
    f.pixels
}

/// **An image that arrives mid-session is drawn on the next incremental
/// frame.** The CPU path drew images only on a full frame, and a live
/// terminal renders incrementally after its first frame -- so on Linux, whose
/// driver is CPU-only, and on the macOS CPU path, a Sixel a program printed
/// did not appear until a resize.
#[test]
fn an_image_that_arrives_mid_session_is_drawn_on_an_incremental_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    assert_eq!(painted_in_image_rect(&c), 0, "the control: nothing yet");
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Incremental);
    }
    let painted = painted_in_image_rect(&c);
    assert!(
        painted > 64 * 48 / 2,
        "the swirl drew on the incremental frame, {painted} pixels"
    );
    assert!(equals_a_full_repaint(&mut t, &c, &faces));
}

/// **And an image that moves or goes is repainted where it was.** Scrolling
/// the swirl up a row, then off the screen, on incremental frames: each frame
/// must equal a full repaint, so the rows it left are the cells again and the
/// rows it is on show it once, not composited over its last position.
#[test]
fn an_image_moving_on_incremental_frames_equals_a_full_repaint() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[6;1Hunder\x1b[6;1H");
    feed(&mut t, SWIRL);
    feed(&mut t, b"\x1b[50;1H");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let bg = Theme::dark().bg.0;
    let drawn = (0..48)
        .flat_map(|y| c.pixels[(5 * ch + y) * c.width..][..64].to_vec())
        .filter(|&p| p != bg)
        .count();
    assert!(
        drawn > 64 * 48 / 2,
        "the control: the swirl drew on row 5 before it moves, {drawn} pixels"
    );
    for step in 0..12 {
        feed(&mut t, b"\n");
        {
            let mut target = c.target();
            r.render(&mut t, &mut target, Damage::Incremental);
        }
        assert!(
            equals_a_full_repaint(&mut t, &c, &faces),
            "after {} line feeds the incremental frame is not the full one",
            step + 1
        );
    }
}

/// **A still image costs no repaint.** The rows under a placement are
/// repainted when it arrives, leaves or moves; a frame whose only change is
/// elsewhere repaints that change and not the image's rows, which is what a
/// keystroke's echo beside a preview is, and blits none of the image: it
/// is blitted only into the rows a frame repaints. The pixels stay equal
/// to a full repaint. The control: the full frame blits it.
#[test]
fn a_still_image_costs_no_repaint() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    feed(&mut t, SWIRL);
    let _ = blits();
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    assert!(blits() > 0, "the control: the full frame blits the swirl");
    feed(&mut t, b"\x1b[40;1Hx");
    let frame = {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Incremental)
    };
    let blitted = blits();
    assert_eq!(
        frame.rows_dirty.value(),
        1,
        "only the row that changed, not the swirl's"
    );
    assert_eq!(blitted, 0, "no pixel of the swirl blitted");
    assert!(equals_a_full_repaint(&mut t, &c, &faces));
}

/// The image pixels this thread's frames blitted since the last call
/// (`Counter::ImagePixelsBlitted`): read before [`equals_a_full_repaint`],
/// whose full frame blits on this thread too.
fn blits() -> u64 {
    counters::take()
        .at(Site::Images, Counter::ImagePixelsBlitted)
        .value()
}

/// `p`'s pixel rows that meet screen row `row`'s band, `ch` pixels a row.
fn rows_on(p: &ImagePlacement, row: i64, ch: i64) -> i64 {
    let (top, end) = (i64::from(p.y), i64::from(p.y) + i64::from(p.h));
    (end.min((row + 1) * ch) - top.max(row * ch)).max(0)
}

/// The first and last screen rows `p` covers, `ch` pixels a row.
fn rows_of(p: &ImagePlacement, ch: i64) -> (i64, i64) {
    let top = i64::from(p.y);
    (
        top.div_euclid(ch),
        (top + i64::from(p.h) - 1).div_euclid(ch),
    )
}

/// **A cell written under an image blits that row's band of it again, and
/// no more** (FS-C8): the written row is repainted, its cells drawn again
/// over the image's last blit, so the image is blitted into that row --
/// and into no other, which a blit of the whole placement would be. The
/// swirl over a row of text, a cell written under its middle row: the
/// frame equals a full repaint, and blits the swirl's width times its
/// pixel rows on that row. Named break: a row blits the images only when
/// one arrived on it, which leaves the written cell over the swirl.
#[test]
fn a_cell_written_under_an_image_blits_its_row_of_it_again() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[6;1Hunder\x1b[6;1H");
    feed(&mut t, SWIRL);
    feed(&mut t, b"\x1b[50;1H");
    let _ = blits();
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    assert!(blits() > 0, "the control: the full frame blits the swirl");
    let ch = i64::from(r.metrics().cell_h.value());
    let swirl = r.image_placements()[0];
    let (first, last) = rows_of(&swirl, ch);
    assert!(
        last - first >= 2,
        "the premise: the swirl covers three rows or more, {first}..={last}"
    );
    let mid = (first + last) / 2;
    let want = u64::try_from(rows_on(&swirl, mid, ch) * i64::from(swirl.w)).unwrap();
    assert!(
        want < u64::from(swirl.w) * u64::from(swirl.h),
        "the premise: the row's band is less than the swirl"
    );
    feed(&mut t, format!("\x1b[{};2HZ\x1b[50;1H", mid + 1).as_bytes());
    let frame = {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Incremental)
    };
    let blitted = blits();
    assert_eq!(
        frame.rows_dirty.value(),
        1,
        "the premise: the written row alone"
    );
    assert_eq!(blitted, want, "the written row's band of the swirl");
    assert!(equals_a_full_repaint(&mut t, &c, &faces));
}

/// **And the CPU path keeps no upload snapshots.** It blits from the
/// terminal's own pixels, so a converted copy of each visible image, which a
/// GPU uploads, is a second copy it never reads.
#[test]
fn the_cpu_path_keeps_no_upload_snapshots() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    for damage in [Damage::Full, Damage::Incremental, Damage::Incremental] {
        let mut target = c.target();
        r.render(&mut t, &mut target, damage);
    }
    assert!(
        painted_in_image_rect(&c) > 64 * 48 / 2,
        "the control: it drew"
    );
    assert_eq!(r.image_snapshots().value(), 0);
    assert!(r.image_uploads().is_empty());
}

/// The partly transparent image the alpha tests place: 40x30, its alpha
/// running through every value from clear to opaque across the rect, its
/// colour varying with position, so a blend that read the alpha a step
/// wrong, or not at all, misses on most pixels.
const PW: usize = 40;
const PH: usize = 30;

fn partial_pixel(x: usize, y: usize) -> Pixel {
    let c = |v: usize| Chan::from_u8(u8::try_from(v % 256).unwrap());
    Pixel::straight(c(x * 7 + y * 13), c(x * 6), c(255 - y * 8), c(90 + x))
}

fn fill_partial(px: &mut [Pixel]) {
    for (i, p) in px.iter_mut().enumerate() {
        *p = partial_pixel(i % PW, i / PW);
    }
}

/// Coloured text on a set, opaque background under the image's rect, and
/// the cursor at row 3, column 4, where the image goes.
const UNDER: &[u8] = b"\x1b[?25l\x1b[48;2;20;40;160m\x1b[38;2;250;220;30m\
\x1b[2;1HHHHHHHHHHHHHHHHH\x1b[3;1HWWWWWWWWWWWWWWWW\x1b[4;1HMMMMMMMMMMMMMMMM\
\x1b[5;1H################\x1b[6;1H@@@@@@@@@@@@@@@@\x1b[0m\x1b[3;4H";

/// What straight-alpha OVER gives `src` over the opaque `dst`, in the sRGB
/// numbers `Blend::Native` blends in: `(s a + d (255 - a)) / 255`, rounded
/// to nearest -- which no exact half can tie, 255 being odd.
fn over(src: Pixel, dst: u32) -> u32 {
    let a = u32::from(src.alpha().get());
    let (r, g, b) = src.rgb();
    let mix = |s: Chan, shift: u32| {
        let d = (dst >> shift) & 0xFF;
        (u32::from(s.get()) * a + d * (255 - a) + 127) / 255
    };
    0xFF00_0000 | mix(r, 16) << 16 | mix(g, 8) << 8 | mix(b, 0)
}

/// **A partly transparent image blends by its alpha.** Every pixel of its
/// rect is the straight-alpha OVER of the image's pixel on the frame drawn
/// without it -- so a path that drew a partial pixel whole (the opaque-or-
/// clear blit), or premultiplied it twice, fails on most of them.
#[test]
fn a_partial_image_blends_by_its_alpha() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let (cw, ch) = (
        usize::try_from(r.metrics().cell_w.value()).unwrap(),
        usize::try_from(r.metrics().cell_h.value()).unwrap(),
    );
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(PW * PH));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let under = c.pixels.clone();
    let placed = t.put_image(
        ImagePx::of(u16::try_from(PW).unwrap()),
        ImagePx::of(u16::try_from(PH).unwrap()),
        ImageAlpha::Partial,
        fill_partial,
    );
    assert!(!placed.is_none(), "the image placed");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let (x0, y0) = (3 * cw, 2 * ch);
    let (mut wrong, mut mid, mut text) = (0, 0, 0);
    for y in 0..PH {
        for x in 0..PW {
            let i = (y0 + y) * c.width + x0 + x;
            assert_eq!(under[i] >> 24, 0xFF, "the frame under the image is opaque");
            let src = partial_pixel(x, y);
            if !matches!(src.alpha().get(), 0 | 0xFF) {
                mid += 1;
            }
            if under[i] != under[(y0 + y) * c.width + x0] {
                text += 1;
            }
            if c.pixels[i] != over(src, under[i]) {
                wrong += 1;
            }
        }
    }
    assert!(mid > PW * PH * 9 / 10, "the control: {mid} partial pixels");
    assert!(text > 0, "the control: glyphs lie under the image");
    assert_eq!(
        wrong, 0,
        "pixels not the OVER of the image on the frame under it"
    );
}

/// Default-coloured text under the image's rect on the default page, the
/// cursor at row 3, column 4, where the image goes: a backdrop translucent
/// between the glyphs once the theme dims the window.
const UNDER_GLASS: &[u8] = b"\x1b[?25l\x1b[2;1HHHHHHHHHHHHHHHHH\x1b[3;1HWWWWWWWWWWWWWWWW\
\x1b[4;1H\x1b[31mMMMMMMMM\x1b[0mMMMMMMMMM\x1b[5;1H################\x1b[3;4H";

/// A straight channel premultiplied as both presenters do it,
/// `baud_domain::premultiply_chan`: `(C a + 127) / 255`, the form a GPU's
/// store holds -- the backdrop as the GPU would hold it.
fn present(c: u32, a: u32) -> u32 {
    let byte = |v: u32| Chan::from_u8(u8::try_from(v).unwrap());
    u32::from(baud_domain::premultiply_chan(byte(c), byte(a)).get())
}

/// **Over the glass a partial image is source-over on the page's alpha**
/// (`[[source: w3c-compositing-1]]` 5.1): each pixel of its rect,
/// presented, is `co = Cs as + cb (1 - as)` and `ao = as + ab (1 - as)`
/// formed over the frame under it presented, each rounded to a byte as a
/// GPU's 8-bit premultiplied store rounds it. The CPU-only gate, and it
/// presents each pixel through `baud_domain::premultiply`, the function
/// the macOS and the Wayland presenter each call, so it holds the CPU arm
/// as either system shows it and runs on Linux; the macOS pixel oracle
/// holds the same frame to the GPU arms. A blend that mixed
/// the page's colour as if it were opaque misses on nearly every pixel.
#[test]
fn a_partial_image_over_the_glass_is_source_over() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(support::PX).unwrap(),
        Theme {
            bg_alpha: Chan::from_u8(56),
            ..Theme::dark()
        },
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut c = Canvas::for_renderer(&r);
    let (cw, ch) = (
        usize::try_from(r.metrics().cell_w.value()).unwrap(),
        usize::try_from(r.metrics().cell_h.value()).unwrap(),
    );
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(PW * PH));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER_GLASS);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let under = c.pixels.clone();
    let placed = t.put_image(
        ImagePx::of(u16::try_from(PW).unwrap()),
        ImagePx::of(u16::try_from(PH).unwrap()),
        ImageAlpha::Partial,
        fill_partial,
    );
    assert!(!placed.is_none(), "the image placed");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let (x0, y0) = (3 * cw, 2 * ch);
    let (mut wrong, mut glass, mut text) = (0, 0, 0);
    for y in 0..PH {
        for x in 0..PW {
            let i = (y0 + y) * c.width + x0 + x;
            let [ab, rb, gb, bb] = under[i].to_be_bytes().map(u32::from);
            if ab < 0xFF {
                glass += 1;
            }
            if under[i] != under[(y0 + y) * c.width + x0] {
                text += 1;
            }
            let src = partial_pixel(x, y);
            let a = u32::from(src.alpha().get());
            let (sr, sg, sb) = src.rgb();
            let ao = (a * 255 + ab * (255 - a) + 127) / 255;
            let ch =
                |s: Chan, b: u32| (u32::from(s.get()) * a + present(b, ab) * (255 - a) + 127) / 255;
            let want = [ao, ch(sr, rb), ch(sg, gb), ch(sb, bb)];
            // The word as both presenters put it in their surface.
            let got = baud_domain::premultiply(c.pixels[i])
                .to_be_bytes()
                .map(u32::from);
            if got != want {
                wrong += 1;
            }
        }
    }
    assert!(
        glass > PW * PH / 2,
        "the premise: {glass} translucent pixels under it"
    );
    assert!(text > 0, "the control: glyphs lie under the image");
    assert_eq!(
        wrong,
        0,
        "pixels not source-over on the glass, of {}",
        PW * PH
    );
}

/// **A still partial image costs no repaint either** (FS-C8): an image
/// over the text is blitted only into the rows a frame repaints, so it is
/// never blitted over its own last blit, which would blend a partly
/// transparent one twice. Ten frames whose change is a row away from it
/// each repaint that row alone, blit none of it, and equal a full repaint
/// -- where every frame blitted it over the whole frame and so repainted
/// its rows on every frame. `a_still_image_costs_no_repaint` is the opaque
/// side. The controls: the image is partly transparent, and the full frame
/// blits it. Named break: every image blitted over the whole frame again.
#[test]
fn a_still_partial_image_costs_no_repaint() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(PW * PH));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    let _ = t.put_image(
        ImagePx::of(u16::try_from(PW).unwrap()),
        ImagePx::of(u16::try_from(PH).unwrap()),
        ImageAlpha::Partial,
        fill_partial,
    );
    let _ = blits();
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    assert!(blits() > 0, "the control: the full frame blits the image");
    assert_eq!(
        r.image_placements()[0].alpha,
        ImageAlpha::Partial,
        "the premise: the image is partly transparent"
    );
    for step in 0..10u8 {
        feed(
            &mut t,
            &[0x1b, b'[', b'4', b'0', b';', b'1', b'H', b'a' + step],
        );
        // The last step's full repaint blitted on this thread too.
        let _ = blits();
        let frame = {
            let mut target = c.target();
            r.render(&mut t, &mut target, Damage::Incremental)
        };
        let blitted = blits();
        assert_eq!(
            frame.rows_dirty.value(),
            1,
            "frame {step}: the row that changed alone"
        );
        assert_eq!(blitted, 0, "frame {step}: no pixel of the image blitted");
        assert!(
            equals_a_full_repaint(&mut t, &c, &faces),
            "frame {step}: the incremental frame is not the full one"
        );
    }
}

/// **The upload a GPU takes is the store's straight pixels**, unconverted:
/// the fragment premultiplies (`glyph_fragment`'s colour branch), so a
/// snapshot that premultiplied too would darken every partial pixel, and
/// one that forced the alpha opaque would draw it whole.
#[test]
fn the_upload_is_the_straight_pixels() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(PW * PH));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    let _ = t.put_image(
        ImagePx::of(u16::try_from(PW).unwrap()),
        ImagePx::of(u16::try_from(PH).unwrap()),
        ImageAlpha::Partial,
        fill_partial,
    );
    r.set_images(&t, Rows::ZERO);
    let ups = r.image_uploads();
    assert_eq!(ups.len(), 1, "one image on screen");
    let want: Vec<u32> = (0..PW * PH)
        .map(|i| partial_pixel(i % PW, i / PW).bits())
        .collect();
    assert_eq!(ups[0].pixels, want);
}

/// **A Sixel's clear pixels show the cells under them** on the CPU path,
/// under both blends, each its own opaque loop: an 8x6 Sixel whose data
/// sets only its second row draws that row red -- pure red, which `Linear`'s
/// tables carry through unchanged -- and leaves the five others as the
/// frame without it. The swirl paints every pixel, so nothing above sees a
/// clear one.
#[test]
fn a_sixels_clear_pixels_show_the_cells_under_them() {
    for blend in [Blend::Native, Blend::Linear] {
        clear_pixels_under(blend);
    }
}

fn clear_pixels_under(blend: Blend) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(support::PX).unwrap(),
        Theme::dark(),
        blend,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut c = Canvas::for_renderer(&r);
    let (cw, ch) = (
        usize::try_from(r.metrics().cell_w.value()).unwrap(),
        usize::try_from(r.metrics().cell_h.value()).unwrap(),
    );
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let under = c.pixels.clone();
    feed(&mut t, b"\x1bP0;1;0q\"1;1;8;6#0;2;100;0;0AAAAAAAA\x1b\\");
    assert_eq!(t.images().placements().count(), 1, "the Sixel placed");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let (x0, y0) = (3 * cw, 2 * ch);
    for y in 0..6 {
        for x in 0..8 {
            let i = (y0 + y) * c.width + x0 + x;
            let want = if y == 1 { 0xFFFF_0000 } else { under[i] };
            assert_eq!(c.pixels[i], want, "{blend:?}: pixel ({x}, {y})");
        }
    }
}

/// **The CPU compositor draws a ReGIS input cursor**: the crosshair
/// `S(C(I2))` selects for a one-shot `R(P(I))`, its stroke in the VT340's
/// map entry 0 and its border in entry 15 (`baud-core`'s `regis_input`
/// tests hold the page's own pixels), over the text, and gone from the
/// frame once a key has ended the input. The page is the VT300's 800 by
/// 480 here, no cell size being set, drawn a pixel to a pixel from the
/// grid's corner.
#[test]
fn a_regis_input_cursor_is_drawn_and_taken_away() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_regis_hold(baud_core::RegisHold::On);
    feed(&mut t, b"\x1bPpS(C(I2))P[100,50]R(P(I))\x1b\\");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let at = |c: &Canvas, x: usize, y: usize| c.pixels[y * c.width + x];
    let bg = Theme::dark().bg.0;
    assert_eq!(at(&c, 300, 50), 0xFF00_0000, "the stroke");
    assert_eq!(at(&c, 300, 49), 0xFFCC_CCCC, "its border");
    assert_eq!(at(&c, 100, 300), 0xFF00_0000, "the vertical line");
    assert_eq!(at(&c, 300, 40), bg, "the text area's background elsewhere");
    t.key(baud_domain::KeyEvent::press(
        baud_domain::KeyCode::Char(baud_domain::Codepoint::from_char('a')),
        baud_domain::Modifiers::NONE,
    ));
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    assert_eq!(at(&c, 300, 50), bg, "the crosshair gone");
    assert_eq!(at(&c, 100, 300), bg);
}

/// **An image's rows are in the painted bands on the frame it arrives**:
/// its blit lands on rows no cell moved, so a presenter copying only the
/// repainted rows would leave the swirl unshown. Every row the frame
/// changed is named, and every row the screen drawn whole by a renderer of
/// its own changed ([`drawn_whole`]), so a frame that drew nothing new
/// fails at the claim. The premise, measured on the screen drawn whole
/// before and after, and not on the frame under test: the swirl drew.
#[test]
fn an_image_that_arrives_is_in_the_painted_bands() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let was = Canvas {
        pixels: c.pixels.clone(),
        width: c.width,
        height: c.height,
    };
    let (width, height) = (c.width, c.height);
    let whole = |t: &mut Term| Canvas {
        pixels: drawn_whole(t, &faces),
        width,
        height,
    };
    let screen_was = whole(&mut t);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Incremental);
    }
    let screen = whole(&mut t);
    assert!(
        screen_was.pixels != screen.pixels,
        "the premise: the swirl drew"
    );
    let bands: Vec<_> = r.painted().collect();
    // The rows the frame under test changed, and the rows the screen drawn
    // whole changed, which a frame that drew nothing new does not show.
    let mut missed = support::uncovered(&was, &c, &bands);
    missed.extend(support::uncovered(&screen_was, &screen, &bands));
    assert!(missed.is_empty(), "rows {missed:?} changed and are unnamed");
}

/// **A shared Sixel recoloured by a later one is drawn in its new colour
/// on the next incremental frame**, the frame a full repaint draws: the
/// recolour moves the store's epoch, and the placement keyed on it is
/// drawn again. Under `?1070` reset a 64x48 Sixel of register 1's red is
/// drawn at home, then a shared body with no data defines register 1 blue.
#[test]
fn a_recoloured_sixel_is_drawn_on_an_incremental_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4 * 64 * 48));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_sixel_claim(baud_domain::SixelClaim::Yes);
    let mut red = b"\x1b[?25l\x1b[?1070l\x1bP9;1q#1;2;100;0;0#1".to_vec();
    for _ in 0..8 {
        red.extend_from_slice(b"!64~-");
    }
    red.extend_from_slice(b"\x1b\\\x1b[H");
    feed(&mut t, &red);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let at = |c: &Canvas| c.pixels[10 * c.width + 10];
    assert_eq!(at(&c), 0xFFFF_0000, "the control: drawn red");
    feed(&mut t, b"\x1bP9;1q#1;2;0;0;100\x1b\\");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Incremental);
    }
    assert_eq!(at(&c), 0xFF00_00FF, "drawn blue on the incremental frame");
    assert!(equals_a_full_repaint(&mut t, &c, &faces));
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `data` in base64, padded.
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

/// A kitty graphics command: the keys `ctrl`, and `px`'s RGBA pixels as
/// its payload when there are any.
fn kitty(ctrl: &str, px: &[[u8; 4]]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !px.is_empty() {
        v.push(b';');
        v.extend_from_slice(&b64(&px.concat()));
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

const RED: [u8; 4] = [200, 40, 40, 255];
const GREEN: [u8; 4] = [40, 200, 40, 255];
const BLUE: [u8; 4] = [40, 40, 200, 255];

/// A renderer, its canvas and a term with room for images, the cursor
/// hidden and the term told the renderer's cell size, for `f`.
fn with_images(f: impl FnOnce(&mut Renderer<'_, '_>, &mut Canvas, &mut Term<'_>, &[Face<'_>])) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &WS);
    let m = r.metrics();
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    feed(&mut t, b"\x1b[?25l");
    f(&mut r, &mut c, &mut t, &faces);
}

/// One frame of `t` onto `c` at `damage`.
fn frame_of(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, c: &mut Canvas, damage: Damage) -> Frame {
    let mut target = c.target();
    r.render(t, &mut target, damage)
}

/// **An image deleted with nothing arriving is taken off the next
/// incremental frame** (FS-C8): the rows a placement left are repainted
/// for it, and nothing arrives to repaint them. A kitty image over a row of
/// text, then deleted (`a=d`): the frame changed, blits nothing, and
/// equals a full repaint. Named break: the rows a placement left not
/// repainted.
#[test]
fn an_image_deleted_is_taken_off_an_incremental_frame() {
    with_images(|r, c, t, faces| {
        feed(t, b"\x1b[4;1Hunder the picture\x1b[4;3H");
        feed(t, &kitty("a=T,f=32,s=16,v=16,i=1,C=1,q=2", &[RED; 256]));
        let _ = blits();
        frame_of(r, t, c, Damage::Full);
        assert!(blits() > 0, "the control: the full frame blits the image");
        let was = drawn_whole(t, faces);
        // The frame before the change blits the image too.
        let _ = blits();
        feed(t, &kitty("a=d,d=i,i=1,q=2", &[]));
        frame_of(r, t, c, Damage::Incremental);
        let blitted = blits();
        assert!(
            r.image_placements().is_empty(),
            "the premise: nothing placed, so nothing arrived"
        );
        assert_eq!(blitted, 0, "nothing blitted");
        assert!(
            was != drawn_whole(t, faces),
            "the premise: the image was taken off"
        );
        assert!(equals_a_full_repaint(t, c, faces));
    });
}

/// A ghost of the cursor's trail on `row`, from column `from` to `to`,
/// `progress` of the way.
fn ghost(row: usize, from: usize, to: usize, progress: f32) -> TrailLook {
    TrailLook::Ghost(Trail {
        from: (Row::from_index(row), Col::at_index(from)),
        to: (Row::from_index(row), Col::at_index(to)),
        progress,
        origin_dy: 0.0,
        lift: Px::of(0),
        strength: baud_render::Strength::Full,
    })
}

/// **A row the trail alone repaints blits an image into the trail's
/// columns alone** (FS-C8): a row dirty only for the trail repaints the
/// columns its records reach, and the rest of it is the last frame's, the
/// image's blit included; a blit across the whole row would blend a partly
/// transparent image twice there. The partial image over a coloured row, a
/// ghost moving a column over it: the frame equals a full repaint with the
/// same ghost, and blits fewer pixels than the image's band on the row.
/// The premises: the image is partly transparent, the ghost's row is the
/// one row repainted, and the image reaches past the ghost's columns on
/// both sides. Named break: the blit into the row's band, not its columns.
#[test]
fn a_row_the_trail_repaints_blits_an_image_into_its_columns_alone() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(PW * PH));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    let _ = t.put_image(
        ImagePx::of(u16::try_from(PW).unwrap()),
        ImagePx::of(u16::try_from(PH).unwrap()),
        ImageAlpha::Partial,
        fill_partial,
    );
    r.set_trail(Some(ghost(2, 4, 8, 0.25)));
    let _ = blits();
    frame_of(&mut r, &mut t, &mut c, Damage::Full);
    assert!(blits() > 0, "the control: the full frame blits the image");
    r.set_trail(Some(ghost(2, 4, 8, 0.5)));
    let frame = frame_of(&mut r, &mut t, &mut c, Damage::Incremental);
    let blitted = blits();
    let m = r.metrics();
    let (cw, ch) = (i64::from(m.cell_w.value()), i64::from(m.cell_h.value()));
    let image = r.image_placements()[0];
    assert_eq!(
        image.alpha,
        ImageAlpha::Partial,
        "the premise: partly transparent"
    );
    assert_eq!(
        frame.rows_dirty.value(),
        1,
        "the premise: the ghost's row alone"
    );
    let (x0, x1) = (i64::from(image.x), i64::from(image.x) + i64::from(image.w));
    assert!(
        x0 < 5 * cw && x1 > 7 * cw,
        "the premise: the image, {x0}..{x1}, reaches past the ghost's columns 5 and 6"
    );
    let band = u64::try_from(rows_on(&image, 2, ch) * i64::from(image.w)).unwrap();
    assert!(
        blitted > 0 && blitted < band,
        "the ghost's columns of the image's band ({band}) blitted: {blitted}"
    );
    let mut fresh = renderer(&faces);
    fresh.set_trail(Some(ghost(2, 4, 8, 0.5)));
    let mut f = Canvas::for_renderer(&fresh);
    frame_of(&mut fresh, &mut t, &mut f, Damage::Full);
    assert!(
        c.pixels == f.pixels,
        "the incremental frame is not the full one"
    );
}

/// A field of baud's own on `row`: the find field's, or the link bar's.
fn field(row: i64) -> Field {
    let text: Vec<Codepoint> = "find".chars().map(Codepoint::from_char).collect();
    Field {
        row: Row::from_index(usize::try_from(row).unwrap()),
        col: Col::at_index(2),
        hints: vec![Hints::default(); text.len()],
        caret: Col::at_index(6),
        text,
    }
}

/// Which of baud's overlays an arm draws over the image.
#[derive(Clone, Copy, Debug)]
enum Chrome {
    Find,
    LinkBar,
}

/// `chrome` on `row`, or none, set on `r` for its next frame.
fn set_chrome(r: &mut Renderer<'_, '_>, t: &Term<'_>, chrome: Chrome, row: Option<i64>) {
    match chrome {
        Chrome::Find => r.set_find(
            t,
            row.map(|row| FindLook {
                marks: Vec::new(),
                field: field(row),
            }),
        ),
        Chrome::LinkBar => r.set_link(row.map(|row| LinkLook {
            spans: Vec::new(),
            bar: field(row),
        })),
    }
}

/// **One of baud's overlays over an image is never covered by its blit**
/// (FS-C8): the find field and the link bar, each on the swirl's middle
/// row. The image is blitted into a row before its overlays, and only into
/// a row a frame repaints, so five frames whose change is elsewhere blit
/// none of it and keep the overlay drawn, where the blit over the whole
/// frame on every frame covered it unless the row was repainted on every
/// frame; and once the overlay goes, its row is repainted with the swirl
/// blitted again. Each frame equals a full repaint with the same overlay.
/// Named breaks: every image blitted over the whole frame again (the
/// steady frames), and a row blitting the images only when one arrived on
/// it (the frame the overlay goes). Each arm stops at its first failure,
/// and every arm's is asserted once, so one arm failing leaves the other
/// run.
#[test]
fn an_overlay_over_an_image_is_never_covered_by_its_blit() {
    let mut wrong = Vec::new();
    for chrome in [Chrome::Find, Chrome::LinkBar] {
        let mut got = Ok(());
        with_images(|r, c, t, faces| got = overlay_arm(chrome, r, c, t, faces));
        if let Err(e) = got {
            wrong.push(e);
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// `Ok` when `ok`, else the failure `what` says.
fn check(ok: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    match ok {
        true => Ok(()),
        false => Err(what()),
    }
}

/// [`an_overlay_over_an_image_is_never_covered_by_its_blit`]'s arm for
/// `chrome`: its first failure, if any.
fn overlay_arm(
    chrome: Chrome,
    r: &mut Renderer<'_, '_>,
    c: &mut Canvas,
    t: &mut Term<'_>,
    faces: &[Face<'_>],
) -> Result<(), String> {
    feed(t, b"\x1b[6;1Hunder\x1b[6;1H");
    feed(t, SWIRL);
    feed(t, b"\x1b[50;1H");
    let _ = blits();
    frame_of(r, t, c, Damage::Full);
    check(blits() > 0, || {
        format!("{chrome:?}: the control: the full frame blits")
    })?;
    let ch = i64::from(r.metrics().cell_h.value());
    let swirl = r.image_placements()[0];
    let (first, last) = rows_of(&swirl, ch);
    let mid = (first + last) / 2;
    let band = u64::try_from(rows_on(&swirl, mid, ch) * i64::from(swirl.w)).unwrap();
    let full = |t: &mut Term<'_>, c: &Canvas, on: Option<i64>| {
        let mut fresh = renderer(faces);
        set_chrome(&mut fresh, t, chrome, on);
        let mut f = Canvas::for_renderer(&fresh);
        frame_of(&mut fresh, t, &mut f, Damage::Full);
        c.pixels == f.pixels
    };
    set_chrome(r, t, chrome, Some(mid));
    frame_of(r, t, c, Damage::Incremental);
    let _ = blits();
    check(full(&mut *t, &*c, Some(mid)), || {
        format!("{chrome:?}: the overlay drawn over the swirl")
    })?;
    for step in 0..5u8 {
        feed(t, &[0x1b, b'[', b'4', b'0', b';', b'1', b'H', b'a' + step]);
        set_chrome(r, t, chrome, Some(mid));
        // The last full repaint blitted on this thread too.
        let _ = blits();
        let frame = frame_of(r, t, c, Damage::Incremental);
        let blitted = blits();
        let dirty = frame.rows_dirty.value();
        check(dirty == 1, || {
            format!("{chrome:?} frame {step}: the row that changed alone, not {dirty}")
        })?;
        check(blitted == 0, || {
            format!("{chrome:?} frame {step}: nothing blitted, not {blitted}")
        })?;
        check(full(&mut *t, &*c, Some(mid)), || {
            format!("{chrome:?} frame {step}: the overlay kept over the swirl")
        })?;
    }
    set_chrome(r, t, chrome, None);
    let _ = blits();
    frame_of(r, t, c, Damage::Incremental);
    let blitted = blits();
    check(blitted == band, || {
        format!(
            "{chrome:?}: the overlay's row blits its band of the swirl again, {band}, not {blitted}"
        )
    })?;
    check(full(&mut *t, &*c, None), || {
        format!("{chrome:?}: the swirl drawn again under it")
    })
}

/// The placement `w` pixels wide on `r`'s last frame.
fn placed(r: &Renderer<'_, '_>, w: u16) -> ImagePlacement {
    *r.image_placements()
        .iter()
        .find(|p| p.w == w)
        .expect("placed")
}

/// **An image under another, shown in its next frame, is drawn under it**
/// (FS-C8): a row an image changed on is repainted, and every image over
/// the text on the row is blitted into it in draw order -- not only the one
/// that changed, which would leave it over the image above it. A kitty
/// animation of two frames at `z=0`, its second frame stored before the
/// first full frame, and a smaller image at `z=1` over part of it; the
/// animation shown in its second frame (`a=a,c=2`, which moves the frame
/// shown and not the store's epoch). The frame equals a full repaint. The
/// premises: the upper placement is the same in both frames, and the lower
/// one is in another frame. Named break: a row blits only the placements
/// that arrived on it.
#[test]
fn an_image_under_another_shown_in_its_next_frame_is_drawn_under_it() {
    with_images(|r, c, t, faces| {
        feed(t, b"\x1b[4;3H");
        feed(
            t,
            &kitty("a=T,f=32,s=32,v=32,i=1,z=0,C=1,q=2", &[RED; 1024]),
        );
        feed(t, &kitty("a=f,f=32,s=32,v=32,i=1,q=2", &[GREEN; 1024]));
        feed(t, b"\x1b[4;5H");
        feed(
            t,
            &kitty("a=T,f=32,s=16,v=16,i=2,z=1,C=1,q=2", &[BLUE; 256]),
        );
        frame_of(r, t, c, Damage::Full);
        let (upper, lower) = (placed(r, 16), placed(r, 32));
        let was = drawn_whole(t, faces);
        feed(t, &kitty("a=a,i=1,c=2,q=2", &[]));
        let _ = t.animate_images(HostTime::of_nanos(1), ImageMotion::Hold);
        frame_of(r, t, c, Damage::Incremental);
        assert!(
            placed(r, 16) == upper,
            "the premise: the upper placement is the one drawn before"
        );
        assert!(
            placed(r, 32).frame != lower.frame,
            "the premise: the lower one shows another frame"
        );
        assert!(
            was != drawn_whole(t, faces),
            "the premise: the frame changed"
        );
        assert!(equals_a_full_repaint(t, c, faces));
    });
}

/// A 16x16 image's RGBA pixels: its left half `left`, its right half
/// `right`.
fn halves(left: [u8; 4], right: [u8; 4]) -> Vec<[u8; 4]> {
    (0..256)
        .map(|i| match i % 16 < 8 {
            true => left,
            false => right,
        })
        .collect()
}

/// **A placement deleted and made again in one read is drawn over the one
/// it was under** (FS-C8): two placements of one image at one `z`, on one
/// cell, each drawing a half of it; the one under deleted and a new one
/// made with its keys, in one read. The new one takes a new placement
/// serial (`ImageStore::take_serial`), which stacks it over the other, and
/// nothing else of it differs from the one deleted, so the CPU path finds
/// it changed by its serial alone, repaints its rows and blits both into
/// them in the new order. (A put that replaces a placement keeps its
/// serial, `ImageStore::place_kitty`, so it cannot restack one.) The
/// premises: two placements each frame; the new one differs from the one
/// deleted in its serial alone; the frame changed. It equals a full
/// repaint. Named break: the serial left out of a placement's key.
#[test]
fn a_placement_made_again_over_another_of_its_image_is_drawn_over_it() {
    with_images(|r, c, t, faces| {
        feed(t, &kitty("a=t,f=32,s=16,v=16,i=1,q=2", &halves(RED, GREEN)));
        feed(t, b"\x1b[4;3H");
        feed(t, &kitty("a=p,i=1,p=1,x=0,y=0,w=8,h=16,C=1,q=2", &[]));
        feed(t, b"\x1b[4;3H");
        feed(t, &kitty("a=p,i=1,p=2,x=8,y=0,w=8,h=16,C=1,q=2", &[]));
        frame_of(r, t, c, Damage::Full);
        assert_eq!(r.image_placements().len(), 2, "the premise: two placements");
        let under = *r
            .image_placements()
            .iter()
            .min_by_key(|p| p.serial)
            .expect("placed");
        let was = drawn_whole(t, faces);
        let mut read = kitty("a=d,d=i,i=1,p=1,q=2", &[]);
        read.extend_from_slice(b"\x1b[4;3H");
        read.extend_from_slice(&kitty("a=p,i=1,p=3,x=0,y=0,w=8,h=16,C=1,q=2", &[]));
        feed(t, &read);
        frame_of(r, t, c, Damage::Incremental);
        assert_eq!(
            r.image_placements().len(),
            2,
            "the premise: two placements again"
        );
        let now = *r
            .image_placements()
            .iter()
            .find(|p| p.draw == under.draw)
            .expect("made again");
        assert!(now.serial != under.serial, "the premise: a new serial");
        let mut but_serial = now;
        but_serial.serial = under.serial;
        assert!(
            but_serial == under,
            "the premise: every other key the one deleted"
        );
        assert!(
            was != drawn_whole(t, faces),
            "the premise: the frame changed"
        );
        assert!(equals_a_full_repaint(t, c, faces));
    });
}

/// What an arm of [`an_image_put_again_with_other_keys_is_drawn_on_an_incremental_frame`]
/// changes of a placement over the text.
#[derive(Clone, Copy, Debug)]
enum Rekey {
    /// The source rectangle it draws, its size and place the same.
    Source,
    /// Its `z`, over another image on its cell.
    Restack,
    /// Its scale: the image drawn over two cells by four.
    Scale,
}

/// **An image put again with other keys is drawn on the next incremental
/// frame** (FS-C8): a placement over the text replaced by a put of the same
/// image and placement id, drawn the same as before but for its source
/// rectangle, its `z` over another image, or its scale. The CPU path blits
/// an image only into the rows a frame repaints, so each of these keys has
/// to move its rows; master blitted every image on every frame. A
/// replacement keeps the placement's serial (`ImageStore::place_kitty`), so
/// the source arm and the restack arm each move one key alone, which their
/// premises assert: the placement equals the one before with that key put
/// back. The scale arm moves the drawn size too. Each arm: the frame
/// changed and equals a full repaint. Each arm stops at its first failure,
/// and every arm's is asserted once. Named break (the source arm): the
/// source rectangle left out of a placement's key.
#[test]
fn an_image_put_again_with_other_keys_is_drawn_on_an_incremental_frame() {
    let mut wrong = Vec::new();
    for rekey in [Rekey::Source, Rekey::Restack, Rekey::Scale] {
        let mut got = Ok(());
        with_images(|r, c, t, faces| got = rekey_arm(rekey, r, c, t, faces));
        if let Err(e) = got {
            wrong.push(e);
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// [`an_image_put_again_with_other_keys_is_drawn_on_an_incremental_frame`]'s
/// arm for `rekey`: its first failure, if any.
fn rekey_arm(
    rekey: Rekey,
    r: &mut Renderer<'_, '_>,
    c: &mut Canvas,
    t: &mut Term<'_>,
    faces: &[Face<'_>],
) -> Result<(), String> {
    feed(t, &kitty("a=t,f=32,s=16,v=16,i=1,q=2", &halves(RED, GREEN)));
    let (first, again) = match rekey {
        // Drawn over two cells by one, so through its tables: a placement
        // drawn at its own size names its source's corner in its scale
        // (`ImageScale::Unit`), and the source would move the scale too.
        Rekey::Source => (
            "a=p,i=1,p=1,x=0,y=0,w=8,h=16,c=2,r=1,C=1,q=2",
            "a=p,i=1,p=1,x=8,y=0,w=8,h=16,c=2,r=1,C=1,q=2",
        ),
        Rekey::Restack => ("a=p,i=1,p=1,z=0,C=1,q=2", "a=p,i=1,p=1,z=2,C=1,q=2"),
        Rekey::Scale => ("a=p,i=1,p=1,C=1,q=2", "a=p,i=1,p=1,c=4,r=2,C=1,q=2"),
    };
    feed(t, b"\x1b[4;3H");
    feed(t, &kitty(first, &[]));
    if matches!(rekey, Rekey::Restack) {
        feed(t, &kitty("a=t,f=32,s=16,v=16,i=2,q=2", &[BLUE; 256]));
        feed(t, b"\x1b[4;3H");
        feed(t, &kitty("a=p,i=2,p=1,z=1,C=1,q=2", &[]));
    }
    frame_of(r, t, c, Damage::Full);
    // Image 1's placement: the image stored first.
    let mine = |r: &Renderer<'_, '_>| r.image_placements().iter().min_by_key(|p| p.born).copied();
    let before = mine(r).ok_or_else(|| format!("{rekey:?}: the premise: placed"))?;
    let was = drawn_whole(t, faces);
    feed(t, b"\x1b[4;3H");
    feed(t, &kitty(again, &[]));
    frame_of(r, t, c, Damage::Incremental);
    let after = mine(r).ok_or_else(|| format!("{rekey:?}: the premise: placed again"))?;
    // The placement with the key named put back as it was before.
    let mut put_back = after;
    let moved = match rekey {
        Rekey::Source => {
            put_back.draw = before.draw;
            before.scale == ImageScale::Mapped && after.draw != before.draw && put_back == before
        }
        Rekey::Restack => {
            put_back.z = before.z;
            after.z != before.z && put_back == before
        }
        Rekey::Scale => after.scale != before.scale,
    };
    check(moved, || {
        format!("{rekey:?}: the premise: that key moved, and alone where named")
    })?;
    check(was != drawn_whole(t, faces), || {
        format!("{rekey:?}: the premise: the frame changed")
    })?;
    check(equals_a_full_repaint(t, c, faces), || {
        format!("{rekey:?}: equal to a full repaint")
    })
}

/// **A frame edited in place is drawn on the next incremental frame**
/// (FS-C8): the frame a kitty image shows, edited by `a=f` with `r=1`,
/// changes its pixels with no placement moved, so the store's epoch the
/// placement is keyed on moves, and its rows are repainted. The frame
/// changed and equals a full repaint.
#[test]
fn a_frame_edited_in_place_is_drawn_on_an_incremental_frame() {
    with_images(|r, c, t, faces| {
        feed(t, b"\x1b[4;3H");
        feed(t, &kitty("a=T,f=32,s=16,v=16,i=1,C=1,q=2", &[RED; 256]));
        frame_of(r, t, c, Damage::Full);
        let was = drawn_whole(t, faces);
        feed(
            t,
            &kitty("a=f,f=32,s=8,v=8,x=0,y=0,r=1,i=1,q=2", &[GREEN; 64]),
        );
        frame_of(r, t, c, Damage::Incremental);
        assert!(was != drawn_whole(t, faces), "the premise: the edit drew");
        assert!(equals_a_full_repaint(t, c, faces));
    });
}

/// **A composition onto the frame shown is drawn on the next incremental
/// frame** (FS-C8): `a=c` copying a rectangle of the second frame onto the
/// first, which shows, changes its pixels with no placement moved. The
/// frame changed and equals a full repaint.
#[test]
fn a_composition_onto_the_frame_shown_is_drawn_on_an_incremental_frame() {
    with_images(|r, c, t, faces| {
        feed(t, b"\x1b[4;3H");
        feed(t, &kitty("a=T,f=32,s=16,v=16,i=1,C=1,q=2", &[RED; 256]));
        feed(t, &kitty("a=f,f=32,s=16,v=16,i=1,q=2", &[GREEN; 256]));
        frame_of(r, t, c, Damage::Full);
        let was = drawn_whole(t, faces);
        feed(t, &kitty("a=c,i=1,r=2,c=1,w=8,h=8,C=1,q=2", &[]));
        let _ = t.animate_images(HostTime::of_nanos(1), ImageMotion::Hold);
        frame_of(r, t, c, Damage::Incremental);
        assert!(
            was != drawn_whole(t, faces),
            "the premise: the composition drew"
        );
        assert!(equals_a_full_repaint(t, c, faces));
    });
}

/// **A ReGIS page drawn on again is drawn on the next incremental frame**
/// (FS-C8): a second vector on the page one body has open repaints the
/// page's pixels in place, one placement throughout. The frame changed and
/// equals a full repaint. The premise: one page placed.
#[test]
fn a_regis_page_drawn_on_again_is_drawn_on_an_incremental_frame() {
    with_images(|r, c, t, faces| {
        feed(t, b"\x1bPpP[10,10]V[300,10]");
        frame_of(r, t, c, Damage::Full);
        assert_eq!(
            r.image_placements().len(),
            1,
            "the premise: the page is placed before the second vector"
        );
        let was = drawn_whole(t, faces);
        feed(t, b"P[10,40]V[300,40]\x1b\\");
        frame_of(r, t, c, Damage::Incremental);
        assert_eq!(r.image_placements().len(), 1, "the premise: one page");
        assert!(was != drawn_whole(t, faces), "the premise: the vector drew");
        assert!(equals_a_full_repaint(t, c, faces));
    });
}

/// **A ReGIS page scrolled within a region is drawn moved on the next
/// incremental frame** (FS-C8): a scroll inside the margins moves the
/// page's pixels in the band with no cell of the blank screen changed. The
/// frame changed and equals a full repaint.
#[test]
fn a_regis_page_scrolled_in_a_region_is_drawn_moved_on_an_incremental_frame() {
    with_images(|r, c, t, faces| {
        feed(t, b"\x1bPpP[10,60]V[300,60]\x1b\\");
        frame_of(r, t, c, Damage::Full);
        let was = drawn_whole(t, faces);
        feed(t, b"\x1b[2;12r\x1b[S");
        frame_of(r, t, c, Damage::Incremental);
        assert!(was != drawn_whole(t, faces), "the premise: the page moved");
        assert!(equals_a_full_repaint(t, c, faces));
    });
}

/// **Each use of `blit_image` in `src`'s code** -- a method call, a path
/// call, or the function named as a value; its definition is not one --
/// as the function it is in and the `impl` that function is in, and each
/// `RowTarget` made: comment lines left out.
fn blit_sites(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (mut func, mut imp) = (String::new(), String::new());
    for line in src.lines() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        if code.starts_with("impl") {
            imp = code.to_string();
        }
        let named = code
            .split_once("fn ")
            .filter(|(before, _)| before.trim().is_empty() || before.trim().starts_with("pub"))
            .map(|(_, rest)| {
                rest.split(['(', '<'])
                    .next()
                    .unwrap_or_default()
                    .to_string()
            });
        if let Some(name) = named {
            func = name;
        }
        let uses = code.matches("blit_image").count() - code.matches("fn blit_image").count();
        for _ in 0..uses {
            out.push((func.clone(), imp.clone()));
        }
        // `Self {` as a value, not a return type before a body.
        let by_self = code.matches("Self {").count() > code.matches("-> Self {").count();
        let made = (code.contains("RowTarget {") && !code.contains("struct"))
            || (by_self && imp.contains("RowTarget"));
        if made {
            out.push((func.clone(), "makes a RowTarget".to_string()));
        }
    }
    out
}

/// The `.rs` files under `dir`, recursively.
fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// **An image is blitted only into a repainted row** (FS-C8):
/// `Target::blit_image` is the crate's own, and is reached, in every
/// crate's sources, from two places alone -- `RowTarget::blit`, an image
/// over the text blitted into a repainted row's band and columns, and
/// `Renderer::paint_under`, one under the text blitted into a row being
/// repainted -- whether called as a method, by its path or named as a
/// value. A `RowTarget` is made in `Renderer::repainted` alone: the type
/// holds that (its fields are its module's, and `repainted` is the one
/// maker there), and the sources say it too. The controls: both calls and
/// the one making are found, and a source with a method call, a path call
/// or a `RowTarget` made by `Self` spliced in is read with it.
#[test]
fn an_image_is_blitted_only_into_a_repainted_row() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for e in std::fs::read_dir(&root).unwrap().flatten() {
        let src = e.path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    assert!(files.len() > 100, "the control: the sources are read");
    let mut sites = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for (func, imp) in blit_sites(&text) {
            let name = f.strip_prefix(&root).unwrap_or(f).display().to_string();
            sites.push((name, func, imp));
        }
    }
    sites.sort();
    let renderer = "baud-render/src/renderer.rs".to_string();
    assert_eq!(
        sites.len(),
        3,
        "two blits and one RowTarget made, and no more: {sites:?}"
    );
    assert!(
        sites.iter().all(|(f, _, _)| *f == renderer),
        "each in the renderer: {sites:?}"
    );
    let found: Vec<(&str, &str)> = sites
        .iter()
        .map(|(_, func, imp)| (func.as_str(), imp.as_str()))
        .collect();
    assert!(
        found.contains(&("blit", "impl RowTarget<'_> {")),
        "RowTarget::blit blits: {found:?}"
    );
    assert!(
        found.iter().any(|&(func, _)| func == "paint_under"),
        "paint_under blits: {found:?}"
    );
    assert!(
        found.contains(&("repainted", "makes a RowTarget")),
        "repainted makes the RowTarget: {found:?}"
    );
    let text = std::fs::read_to_string(root.join(&renderer)).unwrap();
    // A method call, a path call and a RowTarget made by `Self` in its
    // own impl, each spliced in alone.
    let rogues = [
        "fn rogue(t: &mut Target<'_>) {\n    t.blit_image(&b, &v, g, Blend::Native, p);\n}\n",
        "fn rogue(t: &mut Target<'_>) {\n    Target::blit_image(t, &b, &v, g, Blend::Native, p);\n}\n",
        "impl RowTarget<'_> {\n    fn rogue(target: Target<'_>) -> Self {\n        Self { target, row, band }\n    }\n}\n",
    ];
    for rogue in rogues {
        let spliced = format!("{text}\n{rogue}");
        assert_eq!(
            blit_sites(&spliced).len(),
            blit_sites(&text).len() + 1,
            "the control: {rogue:?} spliced in is read"
        );
        assert!(
            blit_sites(&spliced).iter().any(|(func, _)| func == "rogue"),
            "the control: {rogue:?} in the function it is in"
        );
    }
}
