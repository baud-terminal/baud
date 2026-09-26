//! The cursor trail and the screen fade on the CPU path. The pixel oracle
//! compares the two GPU arms, which both draw them from `instances()`; the
//! CPU compositor paints its own rows, so a CPU path that dirtied the rows
//! and never painted the overlay passed every gate.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme, Trail, Transition};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 10,
};

fn px(p: Px) -> usize {
    usize::try_from(p.value()).unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, pixels: &mut [u32], damage: Damage) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(pixels, w, w, h).unwrap();
    r.render(t, &mut target, damage);
}

fn trail(from: (usize, usize), to: (usize, usize), progress: f32) -> Trail {
    Trail {
        from: (Row::from_index(from.0), Col::at_index(from.1)),
        to: (Row::from_index(to.0), Col::at_index(to.1)),
        progress,
    }
}

/// A screen with text on every row, so an overlay has something to cover.
fn with_text(f: impl FnOnce(&mut Term<'_>)) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let lines: Vec<u8> = (0..9)
        .flat_map(|i| format!("row {i} abcdefghijklmnop\r\n").into_bytes())
        .collect();
    feed(&mut t, &lines);
    f(&mut t);
}

/// The pixels that differ between two frames, as a bounding box
/// `(x0, y0, x1, y1)` inclusive, or `None` when they are equal.
fn diff_box(a: &[u32], b: &[u32], width: usize) -> Option<(usize, usize, usize, usize)> {
    let mut out: Option<(usize, usize, usize, usize)> = None;
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x != y {
            let (px, py) = (i % width, i / width);
            out = Some(match out {
                None => (px, py, px, py),
                Some((x0, y0, x1, y1)) => (x0.min(px), y0.min(py), x1.max(px), y1.max(py)),
            });
        }
    }
    out
}

/// **The trail reaches the CPU path's pixels**, where the ghost is and
/// nowhere else: a frame with a mid-sweep ghost differs from the frame
/// without one inside the ghost's cell-sized box.
#[test]
fn the_cpu_path_draws_the_trail() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    with_text(|t| {
        let mut r = renderer(&faces);
        let (w, h) = r.pixel_size();
        let (width, height) = (px(w), px(h));
        let mut bare = vec![0u32; width * height];
        r.set_trail(None);
        paint(&mut r, t, &mut bare, Damage::Full);
        let mut ghosted = vec![0u32; width * height];
        r.set_trail(Some(trail((2, 2), (2, 12), 0.5)));
        paint(&mut r, t, &mut ghosted, Damage::Full);
        let m = r.metrics();
        let (cw, ch) = (px(m.cell_w), px(m.cell_h));
        let (x0, y0, x1, y1) = diff_box(&bare, &ghosted, width).expect("the trail drew nothing");
        // Halfway from column 2 to column 12 of row 2: column 7.
        assert!(
            x0 >= 7 * cw && x1 < 8 * cw && y0 >= 2 * ch && y1 < 3 * ch,
            "the trail drew outside its cell: {:?}, cell {cw}x{ch}",
            (x0, y0, x1, y1)
        );
    });
}

/// **The screen fade reaches the CPU path's pixels**, over the whole frame:
/// a frame under the veil differs from one without it from the first row of
/// ink to the last.
#[test]
fn the_cpu_path_draws_the_screen_fade() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    with_text(|t| {
        let mut r = renderer(&faces);
        let (w, h) = r.pixel_size();
        let (width, height) = (px(w), px(h));
        let mut bare = vec![0u32; width * height];
        r.set_screen_fade(None);
        paint(&mut r, t, &mut bare, Damage::Full);
        let mut veiled = vec![0u32; width * height];
        r.set_screen_fade(Some(Transition { progress: 0.25 }));
        paint(&mut r, t, &mut veiled, Damage::Full);
        // The veil is the default background, so over a default cell it
        // changes nothing: what it changes is the ink. Row 0's text and the
        // cursor on row 9 are the frame's first and last rows of ink.
        let m = r.metrics();
        let (cw, ch) = (px(m.cell_w), px(m.cell_h));
        let (x0, y0, _, y1) = diff_box(&bare, &veiled, width).expect("the fade drew nothing");
        assert!(y0 < ch, "the veil missed row 0: first change at y {y0}");
        assert!(y1 >= 9 * ch, "the veil missed row 9: last change at y {y1}");
        assert!(x0 < cw, "the veil missed column 0: first change at x {x0}");
    });
}

/// **A sweeping trail repaints incrementally what a full frame paints**, at
/// every step of a sweep across rows: the rows the ghost leaves are cleared
/// and the rows it reaches are drawn.
#[test]
fn a_sweeping_trail_repaints_incrementally_what_a_full_frame_paints() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    with_text(|t| {
        let mut inc = renderer(&faces);
        let mut full = renderer(&faces);
        let (w, h) = inc.pixel_size();
        let (width, height) = (px(w), px(h));
        let mut a = vec![0u32; width * height];
        let mut b = vec![0u32; width * height];
        inc.set_trail(None);
        paint(&mut inc, t, &mut a, Damage::Full);
        let steps = [0.0f32, 0.2, 0.4, 0.6, 0.8, 0.95, 1.0];
        for (i, &p) in steps.iter().enumerate() {
            let g = (p < 1.0).then(|| trail((1, 3), (8, 20), p));
            inc.set_trail(g);
            full.set_trail(g);
            paint(&mut inc, t, &mut a, Damage::Incremental);
            paint(&mut full, t, &mut b, Damage::Full);
            assert!(
                a == b,
                "step {i} (progress {p}): incremental differs from full at {:?}",
                diff_box(&a, &b, width)
            );
        }
        // The control: the sweep did draw something, so equality is not two
        // frames that both lost the trail.
        let mut bare = vec![0u32; width * height];
        full.set_trail(None);
        paint(&mut full, t, &mut bare, Damage::Full);
        let mut mid = vec![0u32; width * height];
        full.set_trail(Some(trail((1, 3), (8, 20), 0.5)));
        paint(&mut full, t, &mut mid, Damage::Full);
        assert!(bare != mid, "the sweep drew nothing");
    });
}
