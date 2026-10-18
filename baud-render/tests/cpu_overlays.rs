//! The cursor trail and the screen fade on the CPU path. The pixel oracle
//! compares the two GPU arms, which both draw them from `instances()`; the
//! CPU compositor paints its own rows, so a CPU path that dirtied the rows
//! and never painted the overlay passed every gate.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{
    Blend, Damage, Pt, PxSize, Renderer, Segment, Shape, Smear, Stack, Target, Theme, Trail,
    TrailLook, Transition, quad_coverage,
};
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

fn trail(from: (usize, usize), to: (usize, usize), progress: f32) -> TrailLook {
    TrailLook::Ghost(Trail {
        from: (Row::from_index(from.0), Col::at_index(from.1)),
        to: (Row::from_index(to.0), Col::at_index(to.1)),
        progress,
        origin_dy: 0.0,
        lift: Px::of(0),
        strength: baud_render::Strength::Full,
    })
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

/// **The ghost is the cursor's colour**, as every other trail is: under
/// `OSC 12` setting the cursor red, the ghost over empty cells draws red, and
/// with no cursor colour set it draws the foreground -- the colour the cursor
/// defaults to. The ghost was built in the foreground whatever `OSC 12` said.
#[test]
fn the_ghost_is_the_cursors_colour() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    // Red's share of the ghost's pixels over empty cells, right of the text.
    let reddest = |set: &[u8]| {
        let mut out = 0u32;
        with_text(|t| {
            feed(t, set);
            let mut r = renderer(&faces);
            let (w, h) = r.pixel_size();
            let (width, height) = (px(w), px(h));
            let mut bare = vec![0u32; width * height];
            r.set_trail(None);
            paint(&mut r, t, &mut bare, Damage::Full);
            let mut ghosted = vec![0u32; width * height];
            r.set_trail(Some(trail((2, 22), (2, 28), 0.5)));
            paint(&mut r, t, &mut ghosted, Damage::Full);
            let ch = |p: u32, k: u32| (p >> k) & 0xff;
            let mut drawn = 0;
            for (a, b) in bare.iter().zip(&ghosted) {
                if a != b {
                    drawn += 1;
                    out = out.max(ch(*b, 16).saturating_sub(ch(*b, 8).max(ch(*b, 0))));
                }
            }
            assert!(drawn > 100, "the ghost drew {drawn} pixels");
        });
        out
    };
    assert!(
        reddest(b"\x1b]12;#ff0000\x07") > 40,
        "under a red cursor the ghost is not red"
    );
    assert!(
        reddest(b"") < 8,
        "with no cursor colour the ghost is not the foreground"
    );
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

/// A quad or a sweep `t` seconds into a move from `from` to the block
/// cursor at `to`, on the renderer's own cells.
fn smear(
    r: &Renderer<'_, '_>,
    shape: Shape,
    from: (usize, usize),
    to: (usize, usize),
    t: f32,
) -> Smear {
    let style = baud_core::CursorStyle::SteadyBlock;
    let rect =
        |(row, col): (usize, usize)| r.cursor_rect(Row::from_index(row), Col::at_index(col), style);
    let hole = rect(to);
    let seg = match shape {
        Shape::Quad => Segment::new(rect(from).corners(), hole),
        Shape::Sweep => Segment::sweep(rect(from).corners(), hole),
    };
    Smear {
        corners: seg.at(t),
        hole,
        shape,
        strength: baud_render::Strength::Full,
    }
}

/// **The CPU path draws the quad by the rule**: every pixel the quad covers
/// whole changes, every one it leaves bare -- the cursor's own rectangle
/// included -- does not.
#[test]
fn the_cpu_path_draws_the_quad_by_its_rule() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    with_text(|t| {
        let mut r = renderer(&faces);
        let (w, h) = r.pixel_size();
        let (width, height) = (px(w), px(h));
        let mut bare = vec![0u32; width * height];
        r.set_trail(None);
        paint(&mut r, t, &mut bare, Damage::Full);
        let s = smear(&r, Shape::Quad, (8, 25), (1, 2), 0.05);
        let mut drawn = vec![0u32; width * height];
        r.set_trail(Some(TrailLook::Smear(s)));
        paint(&mut r, t, &mut drawn, Damage::Full);
        let (x0, y0, x1, y1) = s.bounds();
        let (mut inside, mut outside, mut changed) = (0usize, 0usize, 0usize);
        let mut fills = None;
        for y in px(y0)..px(y1).min(height) {
            for x in px(x0)..px(x1).min(width) {
                let at = y * width + x;
                let p = Pt {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                let c = quad_coverage(&s, p);
                if c >= 1.0 {
                    // Covered whole, opaque: the fill, whatever was under it
                    // -- a glyph's ink is the same colour, so compare the
                    // fill rather than a change.
                    inside += 1;
                    let fill = *fills.get_or_insert(drawn[at]);
                    assert_eq!(drawn[at], fill, "({x}, {y}) is inside and not the fill");
                    changed += usize::from(drawn[at] != bare[at]);
                } else if c <= 0.0 {
                    outside += 1;
                    assert_eq!(drawn[at], bare[at], "({x}, {y}) is outside and drawn");
                }
                if s.hole.contains(p) {
                    assert_eq!(drawn[at], bare[at], "({x}, {y}) is the cursor's and drawn");
                }
            }
        }
        assert!(inside > 300 && outside > 300, "{inside} in, {outside} out");
        assert!(changed > 300, "the quad changed {changed} pixels");
    });
}

/// **The CPU path draws the sweep by sfte's rule**, over the empty columns
/// right of the text: every pixel away from its hard edge that the rule
/// draws changes, by an amount in proportion to the rule's alpha -- none at
/// the tail, half at the cursor -- and every one it leaves bare, the
/// cursor's own cell among them, does not.
#[test]
fn the_cpu_path_draws_the_sweep_by_its_rule() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    with_text(|t| {
        let mut r = renderer(&faces);
        let (w, h) = r.pixel_size();
        let (width, height) = (px(w), px(h));
        let mut bare = vec![0u32; width * height];
        r.set_trail(None);
        paint(&mut r, t, &mut bare, Damage::Full);
        let s = smear(&r, Shape::Sweep, (0, 24), (8, 28), 0.0);
        let mut drawn = vec![0u32; width * height];
        r.set_trail(Some(TrailLook::Smear(s)));
        paint(&mut r, t, &mut drawn, Damage::Full);
        let alpha = |x: usize, y: usize| {
            quad_coverage(
                &s,
                Pt {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                },
            ) * 255.0
        };
        let delta = |p: u32, q: u32| -> f32 {
            (0..3)
                .map(|k| ((p >> (8 * k)) & 0xff).abs_diff((q >> (8 * k)) & 0xff) as f32)
                .sum()
        };
        let (x0, y0, x1, y1) = s.bounds();
        let (mut inside, mut outside, mut hole) = (0usize, 0usize, 0usize);
        let (mut faint, mut strong) = ((0.0f32, 0u32), (0.0f32, 0u32));
        for y in px(y0) + 1..(px(y1) - 1).min(height - 1) {
            for x in px(x0) + 1..(px(x1) - 1).min(width - 1) {
                let c = alpha(x, y);
                let near = [
                    alpha(x - 1, y),
                    alpha(x + 1, y),
                    alpha(x, y - 1),
                    alpha(x, y + 1),
                ];
                if near.iter().any(|&n| (n > 0.0) != (c > 0.0)) {
                    continue;
                }
                let at = y * width + x;
                if c >= 4.0 {
                    inside += 1;
                    let d = delta(drawn[at], bare[at]);
                    assert!(d > 0.0, "({x}, {y}) is inside and not drawn");
                    if (16.0..40.0).contains(&c) {
                        faint = (faint.0 + d / c, faint.1 + 1);
                    } else if c >= 100.0 {
                        strong = (strong.0 + d / c, strong.1 + 1);
                    }
                } else if c <= 0.0 {
                    outside += 1;
                    let p = Pt {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    };
                    hole += usize::from(s.hole.contains(p));
                    assert_eq!(drawn[at], bare[at], "({x}, {y}) is outside and drawn");
                }
            }
        }
        assert!(
            inside > 300 && outside > 300 && hole > 100,
            "{inside} in, {outside} out, {hole} in the cursor's cell"
        );
        let (f, g) = (faint.0 / faint.1 as f32, strong.0 / strong.1 as f32);
        assert!(
            faint.1 > 20 && strong.1 > 20 && (f / g - 1.0).abs() < 0.15,
            "the change is not in proportion to the alpha: {f} a step faint, {g} strong"
        );
    });
}

/// **A moving quad or sweep repaints incrementally what a full frame
/// paints**, at every step of a move across rows and back: the rows it
/// leaves are cleared, the rows it reaches drawn.
#[test]
fn a_moving_smear_repaints_incrementally_what_a_full_frame_paints() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    for shape in [Shape::Quad, Shape::Sweep] {
        with_text(|t| moving_smear(&faces, t, shape));
    }
}

fn moving_smear(faces: &[Face<'_>], t: &mut Term<'_>, shape: Shape) {
    let mut inc = renderer(faces);
    let mut full = renderer(faces);
    let (w, h) = inc.pixel_size();
    let (width, height) = (px(w), px(h));
    let mut a = vec![0u32; width * height];
    let mut b = vec![0u32; width * height];
    inc.set_trail(None);
    paint(&mut inc, t, &mut a, Damage::Full);
    let mut drew = false;
    for (i, &time) in [0.0f32, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5].iter().enumerate() {
        let s = smear(&inc, shape, (8, 26), (1, 3), time);
        let look = (time < 0.5).then_some(TrailLook::Smear(s));
        inc.set_trail(look);
        full.set_trail(look);
        paint(&mut inc, t, &mut a, Damage::Incremental);
        paint(&mut full, t, &mut b, Damage::Full);
        assert!(
            a == b,
            "{shape:?} step {i} ({time} s): incremental differs from full at {:?}",
            diff_box(&a, &b, width)
        );
        let mut bare = vec![0u32; width * height];
        let mut plain = renderer(faces);
        plain.set_trail(None);
        paint(&mut plain, t, &mut bare, Damage::Full);
        drew |= look.is_some() && b != bare;
    }
    assert!(drew, "the {shape:?} move drew nothing");
}

/// **On the CPU an effect is its emitter**, for every effect: the effect is
/// the GPU's, so the CPU path paints exactly the quad the effect rides --
/// the same pixels as a quad trail with those corners -- and an effect
/// moving between frames repaints incrementally what a full frame paints,
/// its damage being the reach of its records.
#[test]
fn the_cpu_path_draws_an_effect_as_its_emitter() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    for effect in Effect::ALL {
        with_text(|t| {
            let mut r = renderer(&faces);
            let (w, h) = r.pixel_size();
            let (width, height) = (px(w), px(h));
            let vfx = |r: &Renderer<'_, '_>, t: f32| {
                let emitter = smear(r, Shape::Quad, (1, 2), (7, 20), t);
                let from = emitter.corners.0;
                let tail = Pt {
                    x: (from[2].x + from[0].x) * 0.5,
                    y: (from[0].y + from[1].y) * 0.5,
                };
                let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
                Vfx {
                    effect,
                    emitter,
                    ignition: Ignition::ignite(tail, emitter.hole, Seed(5), unit).at(t),
                    axis: Pt { x: 1.0, y: 0.0 },
                }
            };
            let mut quad = vec![0u32; width * height];
            let mut burning = vec![0u32; width * height];
            let f = vfx(&r, 0.05);
            r.set_trail(Some(TrailLook::Smear(f.emitter)));
            paint(&mut r, t, &mut quad, Damage::Full);
            r.set_trail(Some(TrailLook::Vfx(f)));
            paint(&mut r, t, &mut burning, Damage::Full);
            assert!(
                quad.iter().zip(&burning).all(|(a, b)| a == b),
                "{effect:?}: the frame is its emitter's, off at {:?}",
                diff_box(&quad, &burning, width)
            );
            let mut bare = vec![0u32; width * height];
            r.set_trail(None);
            paint(&mut r, t, &mut bare, Damage::Full);
            assert!(
                bare != burning,
                "{effect:?}: the emitter reached the pixels"
            );
            // Moving, frame by frame: incremental equals full.
            let mut inc = bare.clone();
            for step in [0.02f32, 0.06, 0.12, 0.3] {
                r.set_trail(Some(TrailLook::Vfx(vfx(&r, step))));
                paint(&mut r, t, &mut inc, Damage::Incremental);
                let mut full = vec![0u32; width * height];
                paint(&mut r, t, &mut full, Damage::Full);
                assert!(
                    inc == full,
                    "{effect:?} at {step}: incremental off at {:?}",
                    diff_box(&inc, &full, width)
                );
            }
            r.set_trail(None);
            paint(&mut r, t, &mut inc, Damage::Incremental);
            assert!(inc == bare, "{effect:?}: gone, it leaves nothing");
        });
    }
}
