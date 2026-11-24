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
    Blend, Damage, Instance, Kind, Mix, Pt, PxSize, Renderer, Segment, Shape, Smear, Stack, Tally,
    Target, Theme, Trail, TrailLook, Transition, quad_coverage,
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
    let seal = t.seal_writes();
    r.render(t, seal, &mut target, damage);
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
/// `OSC 12` setting the cursor red, the ghost over empty cells draws red;
/// under a grey one it draws no red; and with no cursor colour set it draws
/// baud's amber, the colour the cursor defaults to. The ghost was built in
/// the foreground whatever `OSC 12` said.
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
        reddest(b"\x1b]12;#d8d8d8\x07") < 8,
        "under a grey cursor the ghost is not grey"
    );
    assert!(
        reddest(b"") > 20,
        "with no cursor colour the ghost is not baud's amber"
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
        r.set_screen_fade(Some(Transition::fade(0.25)));
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
        opacity: Smear::OPAQUE,
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

/// **A jump's smear repaints only the columns it covers**: a quad's and a
/// sweep's jump from the top-left cell to the bottom-right, every frame at
/// 60 Hz and the frame it is gone, over text: an incremental frame writes
/// no pixel outside, on each cell row, the columns from two cells left of
/// the leftmost pixel the smear covers on its rows and the one either side,
/// or leaves bare for the cursor in its bounds, this frame or the last, to
/// two cells right of the rightmost -- every other pixel is the last
/// frame's, so a pixel there is left as it was, even one no frame drew.
/// The smear's one record is its bounds, most of the window on this jump,
/// and a row repainting the record's columns wrote 29,646 pixels outside
/// over the quad's 23 frames and 117,990 over the sweep's 38 (note AJ
/// section 24). The control: a pixel inside changed on most frames.
#[test]
fn a_jump_repaints_only_the_columns_its_smear_covers() {
    const POISON: u32 = 0x1234_5678;
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    for shape in [Shape::Quad, Shape::Sweep] {
        with_text(|t| {
            let mut r = renderer(&faces);
            let (w, h) = r.pixel_size();
            let (width, height) = (px(w), px(h));
            let (cell_w, cell_h) = (px(r.metrics().cell_w), px(r.metrics().cell_h));
            let mut frame = vec![0u32; width * height];
            r.set_trail(None);
            paint(&mut r, t, &mut frame, Damage::Full);
            let lasts = {
                let style = baud_core::CursorStyle::SteadyBlock;
                let rect = |row: usize, col: usize| {
                    r.cursor_rect(Row::from_index(row), Col::at_index(col), style)
                };
                match shape {
                    Shape::Quad => Segment::new(rect(0, 1).corners(), rect(9, 28)).lasts(),
                    Shape::Sweep => Segment::sweep(rect(0, 1).corners(), rect(9, 28)).lasts(),
                }
            };
            let (mut outside, mut frames, mut drew) = (0usize, 0usize, 0usize);
            let mut last = None::<Smear>;
            for f in 0u16.. {
                let age = f32::from(f) / 60.0;
                let now = (age <= lasts).then(|| smear(&r, shape, (0, 1), (9, 28), age));
                let mut allowed = vec![false; width * height];
                for row in 0..usize::from(WS.ws_row) {
                    let ys = row * cell_h..(row + 1) * cell_h;
                    let near = ys.start.saturating_sub(1)..(ys.end + 1).min(height);
                    let (mut lo, mut hi) = (usize::MAX, 0usize);
                    for s in now.iter().chain(&last) {
                        let (x0, y0, x1, y1) = s.bounds();
                        let bounded = |x: usize, y: usize| {
                            (px(x0)..px(x1)).contains(&x) && (px(y0)..px(y1)).contains(&y)
                        };
                        for y in near.clone() {
                            for x in 0..width {
                                let p = Pt {
                                    x: x as f32 + 0.5,
                                    y: y as f32 + 0.5,
                                };
                                let bare = s.hole.contains(p) && bounded(x, y);
                                if quad_coverage(s, p) > 0.0 || bare {
                                    (lo, hi) = (lo.min(x), hi.max(x + 1));
                                }
                            }
                        }
                    }
                    if lo < hi {
                        let (l, r) = (lo.saturating_sub(2 * cell_w), (hi + 2 * cell_w).min(width));
                        for y in ys {
                            allowed[y * width + l..y * width + r].fill(true);
                        }
                    }
                }
                for (p, &inside) in frame.iter_mut().zip(&allowed) {
                    if !inside {
                        *p = POISON;
                    }
                }
                let before = frame.clone();
                r.set_trail(now.map(TrailLook::Smear));
                paint(&mut r, t, &mut frame, Damage::Incremental);
                outside += frame
                    .iter()
                    .zip(&allowed)
                    .filter(|&(&p, &inside)| !inside && p != POISON)
                    .count();
                let drawn = frame
                    .iter()
                    .zip(&before)
                    .zip(&allowed)
                    .any(|((a, b), &i)| i && a != b);
                drew += usize::from(drawn);
                frames += 1;
                if now.is_none() {
                    break;
                }
                last = now;
            }
            assert!(
                drew * 2 > frames,
                "the control: {shape:?} drew {drew} of {frames}"
            );
            assert_eq!(
                outside, 0,
                "{shape:?}: {outside} pixels repainted outside the columns it covers over {frames} frames"
            );
        });
    }
}

/// `src`, premultiplied and in channels 0 to 1, over the premultiplied
/// pixel `dst` by `mix`, rounded to bytes as the GPU's blend unit stores
/// them: `over` in `baud-present-macos/tests/cells.rs`, the rule the GPU
/// arms are held to, for source-over, and W3C Compositing's section 6 in
/// premultiplied numbers for each mode: `cs + cb (1 - as)` source-over,
/// `cs + cb - cs cb` by `screen`, `cs (1 - ab) + cb (1 - as) + cs cb` by
/// `multiply`, the alpha `as + ab (1 - as)` under each.
fn mixed_premultiplied(src: [f32; 4], dst: u32, mix: Mix) -> u32 {
    let ch = |k: u32| f32::from(u8::try_from((dst >> k) & 0xff).unwrap()) / 255.0;
    let (a, ab) = (src[3], ch(24));
    let byte = |v: f32| u32::try_from((v * 255.0).round().clamp(0.0, 255.0) as i64).unwrap();
    let colour = |s: f32, d: f32| {
        byte(match mix {
            Mix::Normal => s + d * (1.0 - a),
            Mix::Screen => s + d - s * d,
            Mix::Multiply => s * (1.0 - ab) + d * (1.0 - a) + s * d,
        })
    };
    (byte(a + ab * (1.0 - a)) << 24)
        | (colour(src[0], ch(16)) << 16)
        | (colour(src[1], ch(8)) << 8)
        | colour(src[2], ch(0))
}

/// **An effect's frame by its rules**, independent of the compositor: each
/// record over the premultiplied `bare` frame in order, a band's pixel at
/// the effect's light at its centre meeting the record's alpha by the
/// frame's cap ([`Vfx::cap`]) and a particle's at its shape times the
/// record's alpha, in its colour, each meeting what is under it by `mix`
/// (`Renderer::trail_mix`) -- the GPU's effect pipeline -- and clipped to
/// the frame as its viewport is.
fn effect_reference(
    vfx: &Vfx,
    records: &[Instance],
    bare: &[u32],
    width: Px,
    mix: Mix,
) -> Vec<u32> {
    let width = px(width);
    let mut out: Vec<u32> = bare.iter().map(|&p| baud_domain::premultiply(p)).collect();
    let height = out.len() / width;
    for r in records {
        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
        let (w, h) = (i32::from(r.w.0), i32::from(r.h.0));
        let unit = |v: u32| f32::from(u8::try_from(v & 0xff).unwrap()) / 255.0;
        let c = r.color.0;
        let color = [unit(c >> 16), unit(c >> 8), unit(c), unit(c >> 24)];
        for y in y0.max(0)..(y0 + h).min(i32::try_from(height).unwrap()) {
            for x in x0.max(0)..(x0 + w).min(i32::try_from(width).unwrap()) {
                let centre = Pt {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                let a = if r.kind == Kind::Particle {
                    color[3]
                        * vfx.particle(Pt {
                            x: (centre.x - x0 as f32) / w as f32 * 2.0 - 1.0,
                            y: (centre.y - y0 as f32) / h as f32 * 2.0 - 1.0,
                        })
                } else {
                    vfx.cap().of(vfx.light(centre), color[3])
                };
                let at = usize::try_from(y).unwrap() * width + usize::try_from(x).unwrap();
                out[at] = mixed_premultiplied(
                    [color[0] * a, color[1] * a, color[2] * a, a],
                    out[at],
                    mix,
                );
            }
        }
    }
    out
}

/// **The CPU path draws every effect by its light**, as the GPU's effect
/// pipeline does: over text, on the dark theme and the light, each on an
/// opaque page and on the glass at alpha 56, each effect at four ages --
/// the body burning, particles born, the fade -- the frame, premultiplied
/// as the presenter premultiplies it, is the records composited by the
/// effect's rules over the frame without it, by the frame's mix
/// ([`effect_reference`]; the plasma's `screen` on the dark theme and its
/// `multiply` on the light), every pixel, and some hundreds of pixels are
/// lit; and an effect moving between frames repaints incrementally what a
/// full frame paints, its damage being the reach of its records. Red
/// against a CPU path that paints the emitter's quad, as it did before
/// every effect was drawn there.
#[test]
fn the_cpu_path_draws_every_effect_by_its_light() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    for effect in Effect::ALL {
        for (theme, alpha) in [
            (Theme::dark(), 0xFF),
            (Theme::dark(), 56),
            (Theme::light(), 0xFF),
            (Theme::light(), 56),
        ] {
            with_text(|t| {
                let mut r = renderer(&faces);
                r.set_theme(Theme {
                    bg_alpha: baud_domain::Chan::from_u8(alpha),
                    ..theme.clone()
                });
                let (w, h) = r.pixel_size();
                let (width, height) = (px(w), px(h));
                let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
                let vfx = |r: &Renderer<'_, '_>, t: f32| {
                    let emitter = smear(r, Shape::Quad, (1, 2), (7, 20), t);
                    let from = emitter.corners.0;
                    let tail = Pt {
                        x: (from[2].x + from[0].x) * 0.5,
                        y: (from[0].y + from[1].y) * 0.5,
                    };
                    Vfx {
                        effect,
                        emitter,
                        ignition: Ignition::ignite(tail, emitter.hole, Seed(5), unit).at(t),
                        axis: Pt { x: 0.95, y: 0.31 },
                    }
                };
                let mut bare = vec![0u32; width * height];
                r.set_trail(None);
                paint(&mut r, t, &mut bare, Damage::Full);
                let end = vfx(&r, 0.0).end();
                for age in [0.08f32, 0.2, 0.4, 0.7].map(|k| k * end) {
                    let f = vfx(&r, age);
                    r.set_trail(Some(TrailLook::Vfx(f)));
                    let mut burning = vec![0u32; width * height];
                    paint(&mut r, t, &mut burning, Damage::Full);
                    let want = effect_reference(&f, r.trail(), &bare, w, r.trail_mix());
                    let got: Vec<u32> = burning
                        .iter()
                        .map(|&p| baud_domain::premultiply(p))
                        .collect();
                    assert!(
                        got == want,
                        "{effect:?} at {age}, alpha {alpha}: off its rules at {:?}",
                        diff_box(&got, &want, width)
                    );
                    let lit = burning.iter().zip(&bare).filter(|(a, b)| a != b).count();
                    // The ufo is a saucer a couple of cells wide early on,
                    // the oracle's least; every effect is fading by the last.
                    let least = match (age < 0.5 * end, effect) {
                        (false, _) => 0,
                        (true, Effect::Ufo) => 60,
                        (true, _) => 100,
                    };
                    assert!(
                        lit > least,
                        "{effect:?} at {age}, alpha {alpha}: only {lit} pixels lit"
                    );
                }
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
}

/// `look` drawn at `opacity`: a smear's own, an effect's emitter's.
fn at_opacity(look: TrailLook, opacity: baud_domain::Chan) -> TrailLook {
    match look {
        TrailLook::Smear(s) => TrailLook::Smear(Smear { opacity, ..s }),
        TrailLook::Vfx(v) => TrailLook::Vfx(Vfx {
            emitter: Smear {
                opacity,
                ..v.emitter
            },
            ..v
        }),
        TrailLook::Ghost(_) => look,
    }
}

/// The quad, the sweep and every effect, part way through a move from row
/// 1 to row 7 over the text, each drawn whole: the looks the fade tests
/// draw faded.
fn fading_looks(r: &Renderer<'_, '_>) -> Vec<(String, TrailLook)> {
    let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
    let mut looks = vec![
        (
            "the quad".to_string(),
            TrailLook::Smear(smear(r, Shape::Quad, (1, 2), (7, 20), 0.05)),
        ),
        (
            "the sweep".to_string(),
            TrailLook::Smear(smear(r, Shape::Sweep, (1, 2), (7, 20), 0.05)),
        ),
    ];
    for effect in Effect::ALL {
        let emitter = smear(r, Shape::Quad, (1, 2), (7, 20), 0.0);
        let from = emitter.corners.0;
        let tail = Pt {
            x: (from[2].x + from[0].x) * 0.5,
            y: (from[0].y + from[1].y) * 0.5,
        };
        let ig = Ignition::ignite(tail, emitter.hole, Seed(5), unit);
        let age = 0.2 * effect.end(&ig);
        looks.push((
            format!("{effect:?}"),
            TrailLook::Vfx(Vfx {
                effect,
                emitter: smear(r, Shape::Quad, (1, 2), (7, 20), age),
                ignition: ig.at(age),
                axis: Pt { x: 0.95, y: 0.31 },
            }),
        ));
    }
    looks
}

/// **A fading trail is drawn that share of the way from the page to the
/// whole trail, on the CPU path**: the quad, the sweep and every effect at
/// half opacity, against the same look drawn whole and the frame without
/// it. Every record the GPU paths draw has its alpha scaled by it; every
/// pixel the trail changes lies between the two frames on every channel;
/// the quad's and the sweep's, one record each, half way, to the rounding
/// of a step; and an effect's is its records composited by its light, as
/// the GPU's effect pipeline does ([`effect_reference`]). Perturbation:
/// the opacity left off the CPU path's quad fill -- the quad's and the
/// sweep's pixels are the whole trail's.
#[test]
fn a_fading_trail_is_drawn_part_way_on_the_cpu_path() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let half = baud_domain::Chan::from_u8(0x80);
    with_text(|t| {
        let mut r = renderer(&faces);
        let (w, h) = r.pixel_size();
        let (width, height) = (px(w), px(h));
        let mut bare = vec![0u32; width * height];
        r.set_trail(None);
        paint(&mut r, t, &mut bare, Damage::Full);
        for (what, look) in fading_looks(&r) {
            let mut whole = vec![0u32; width * height];
            r.set_trail(Some(look));
            paint(&mut r, t, &mut whole, Damage::Full);
            let alphas: Vec<_> = r.trail().iter().map(|i| i.color.a()).collect();
            let faded = at_opacity(look, half);
            let mut part = vec![0u32; width * height];
            r.set_trail(Some(faded));
            paint(&mut r, t, &mut part, Damage::Full);
            let scaled: Vec<_> = r.trail().iter().map(|i| i.color.a()).collect();
            let want: Vec<_> = alphas
                .iter()
                .map(|&a| baud_render::capped(a, half))
                .collect();
            assert_eq!(scaled, want, "{what}: the records' alphas");
            let (mut changed, mut moved) = (0usize, 0usize);
            for (i, ((&b, &f), &m)) in bare.iter().zip(&whole).zip(&part).enumerate() {
                if b == f {
                    assert_eq!(m, b, "{what}: pixel {i} is the trail's alone");
                    continue;
                }
                changed += 1;
                moved += usize::from(m != f && m != b);
                for k in [0u32, 8, 16] {
                    let ch = |p: u32| i32::try_from((p >> k) & 0xff).unwrap();
                    let (b, f, m) = (ch(b), ch(f), ch(m));
                    assert!(
                        b.min(f) <= m && m <= b.max(f),
                        "{what}: pixel {i} channel {k}: {m} is not between {b} and {f}"
                    );
                    if matches!(look, TrailLook::Smear(_)) && (f - b).abs() >= 16 {
                        assert!(
                            (2 * m - b - f).abs() <= 3,
                            "{what}: pixel {i} channel {k}: {m} is not half way from {b} to {f}"
                        );
                    }
                }
            }
            assert!(changed > 200, "{what}: the control, {changed} pixels drawn");
            assert!(
                moved * 2 > changed,
                "{what}: only {moved} of {changed} pixels faded"
            );
            if let TrailLook::Vfx(v) = faded {
                let got: Vec<u32> = part.iter().map(|&p| baud_domain::premultiply(p)).collect();
                let want = effect_reference(&v, r.trail(), &bare, w, r.trail_mix());
                assert!(
                    got == want,
                    "{what}: off its rules at {:?}",
                    diff_box(&got, &want, width)
                );
            }
        }
    });
}

/// **A fade alone redraws the trail's rows**: each look drawn whole, then
/// the same look at half opacity drawn incrementally over that frame, is
/// the frame drawn from nothing at half -- the opacity is part of the
/// trail's value, which the damage compares -- and differs from the whole
/// one.
#[test]
fn a_fade_alone_redraws_the_trails_rows() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let half = baud_domain::Chan::from_u8(0x80);
    with_text(|t| {
        let probe = renderer(&faces);
        let (w, h) = probe.pixel_size();
        let (width, height) = (px(w), px(h));
        for (what, look) in fading_looks(&probe) {
            let mut r = renderer(&faces);
            let mut frame = vec![0u32; width * height];
            r.set_trail(Some(look));
            paint(&mut r, t, &mut frame, Damage::Full);
            let whole = frame.clone();
            r.set_trail(Some(at_opacity(look, half)));
            paint(&mut r, t, &mut frame, Damage::Incremental);
            let mut fresh = renderer(&faces);
            let mut want = vec![0u32; width * height];
            fresh.set_trail(Some(at_opacity(look, half)));
            paint(&mut fresh, t, &mut want, Damage::Full);
            assert!(frame != whole, "{what}: the fade drew nothing");
            assert!(
                frame == want,
                "{what}: incremental off at {:?}",
                diff_box(&frame, &want, width)
            );
        }
    });
}

/// FNV-1a over a frame's pixel words.
fn digest(frame: &[u32]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
        p.to_le_bytes()
            .iter()
            .fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
    })
}

/// **Every trail draws the same pixels on every OS**: each effect, the
/// quad it rides and a repeat pass under the flash cap, at three ages, on
/// an opaque page and on the glass at alpha 56, over a blank screen --
/// no glyph, so what is pinned is the trail's -- is the frame pinned here,
/// taken on macOS and required of every other target: the Linux driver
/// draws with this path. The trails' math is `baud_render::det`'s, whose
/// bits are the same on every machine; std's `exp`, `sin` and the rest are
/// the platform's libm, which differ between macOS and glibc
/// (`the_trails_call_no_platform_math` bars them). A change to an effect's
/// look changes its line, which the failure prints whole. 92 of the 96
/// moved when the trails took baud's amber, the cursor's colour; the four
/// that stayed are repeats that draw nothing.
#[test]
fn every_trail_draws_the_same_pixels_on_every_os() {
    // Per page, opaque then the glass: per look, the fire to the liquid and
    // then the quad, three ages, each full and then a repeat. A repeat
    // whose caps are not yet scanned draws nothing: the fire's on the glass
    // at the first and last ages, and the plasma's at the first, scanned
    // under its own mix.
    const PINNED: &[u64] = &[
        0x61eb0958f112c5dd,
        0x0f07949d1ff684d7,
        0x1b4950b039bda27f,
        0xaceeeb013a3a34f8,
        0x6d09156f372b486e,
        0x3f5557cec5984125,
        0xbc773d9649d4952e,
        0xbd2219ab4ba565d5,
        0x0daa3d02473e74a1,
        0x9de0af9567df1a74,
        0xe60ee95ac73724c5,
        0xa142330c38fcd519,
        0x94657c24bd1792bb,
        0xdc667125dfbfc55d,
        0x9c587913a9c68433,
        0x6e6f9e5139e36d2b,
        0x6c68321c85871b65,
        0xbda4eaf44645b820,
        0x59a541af93ca3f24,
        0x245d1e9cfc657f9d,
        0x5a9ead915fbb594f,
        0x5e337cede4addc5a,
        0x82af4a6e1ad119ff,
        0xb732b34426334569,
        0x90d23e8a45e9b623,
        0xfa12351e59a7515f,
        0xb6ac278db937475a,
        0xa017796ee62122d8,
        0x1fa4b522a05a107e,
        0xce230afb864baad1,
        0xfec56328b0ac634b,
        0x33772945239a7986,
        0x679bbdf6857a9e33,
        0x93207db18f893424,
        0xe191fe64a1b44b8d,
        0xd55d5b67628f2e5b,
        0x548ea11bd3901923,
        0x056011a73da56274,
        0x03838aa3017bbb82,
        0xbb68fe3e94ab8d1b,
        0xc4348eb3f46c30db,
        0xc0302115f58dfde2,
        0x444cca2a672baf2a,
        0x02c5c1c238f162f0,
        0x9de38ccf8ce6dfda,
        0xa39e4e5f93542207,
        0xac6c2bd29076c807,
        0x6492a7f188b723ad,
        0x871877c99159cdb3,
        0x48b2d42d76073f25,
        0x876303ad261fc529,
        0x5a00bd42195686ef,
        0x69dabf24fe7eb65c,
        0x48b2d42d76073f25,
        0x2bd34a8e43424993,
        0x48b2d42d76073f25,
        0xd528286cd9e7bcfa,
        0xe9ed7212ab8dcf5d,
        0xb8028c8b6b06a549,
        0x7d20f76e10a5324d,
        0x23c850aa29cdcfe8,
        0xa5440fbad67ea19a,
        0x3cf2c9857cde86d3,
        0x173ce8f454f725e1,
        0x22e535c723b7221b,
        0xc958ca007b971068,
        0x2aa5545785c9cfbb,
        0xd6474376ca0129ec,
        0x05d5853ef27387d2,
        0x946609a24a2538b4,
        0xb273199f555466f6,
        0xe7c61538aceb60eb,
        0x7aacae3a8040edc2,
        0x4a1cbe0875fed39a,
        0xa710a9e4b69a73c8,
        0x16163134d77c2056,
        0xb27c22cca803695a,
        0x018a62a036edc899,
        0x5e2b7122196b5bf9,
        0x00952bd4804f5f09,
        0xd36195684d49b35b,
        0x3db86d37eee99b78,
        0x17bd72cf36b85a31,
        0x065258037c621e20,
        0x8204d91804cff83b,
        0x5071eea6da6d2454,
        0xc97b0614532b7928,
        0x06e7283a0189753d,
        0xea25b8e1367b1eea,
        0x1bfaa59a79827052,
        0x4885201c76c98147,
        0x6466a55b79a208b4,
        0x222e641a65fc4020,
        0xa6771015cf0bfebd,
        0x3583fbe8a2cf3238,
        0x683a89b937c47acd,
    ];
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    let mut got = Vec::new();
    for alpha in [0xFF, 56] {
        let mut r = renderer(&faces);
        r.set_theme(Theme {
            bg_alpha: baud_domain::Chan::from_u8(alpha),
            ..Theme::dark()
        });
        let (w, h) = r.pixel_size();
        let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
        let mut frame = vec![0u32; px(w) * px(h)];
        let looks = |r: &Renderer<'_, '_>, effect: Option<Effect>, k: f32| {
            let start = smear(r, Shape::Quad, (1, 2), (7, 20), 0.0).corners.0;
            let tail = Pt {
                x: (start[2].x + start[0].x) * 0.5,
                y: (start[0].y + start[1].y) * 0.5,
            };
            let to = smear(r, Shape::Quad, (1, 2), (7, 20), 0.0).hole;
            let end = Ignition::ignite(tail, to, Seed(9), unit);
            let age = k * effect.unwrap_or(Effect::Fire).end(&end);
            let emitter = smear(r, Shape::Quad, (1, 2), (7, 20), age);
            let repeat = Smear {
                strength: baud_render::Strength::UnderThreshold,
                ..emitter
            };
            let vfx = |e: Effect, s: Smear| Vfx {
                effect: e,
                emitter: s,
                ignition: Ignition::ignite(tail, s.hole, Seed(9), unit).at(age),
                axis: Pt { x: 0.95, y: 0.31 },
            };
            match effect {
                Some(e) => [
                    TrailLook::Vfx(vfx(e, emitter)),
                    TrailLook::Vfx(vfx(e, repeat)),
                ],
                None => [TrailLook::Smear(emitter), TrailLook::Smear(repeat)],
            }
        };
        for effect in Effect::ALL.map(Some).into_iter().chain([None]) {
            for k in [0.1f32, 0.3, 0.6] {
                for look in looks(&r, effect, k) {
                    r.set_trail(Some(look));
                    paint(&mut r, &mut t, &mut frame, Damage::Full);
                    got.push(digest(&frame));
                }
            }
        }
    }
    let table: String = got
        .iter()
        .map(|d| format!("        0x{d:016x},\n"))
        .collect();
    assert!(
        got == PINNED,
        "the trails' frames moved; the table this target draws:\n{table}"
    );
}

/// **Every effect's light is the same bits on every OS**, which the frames
/// above cannot show alone: a byte rounds away most one-ulp differences,
/// and std's libm put every one of those frames on the same bytes on macOS
/// and on glibc. Here each effect at three ages is its light's `f32` bits
/// at every pixel centre of the frame, its particles' centres, radii and
/// glows, and the quad's corners it rides, folded into one digest pinned
/// from macOS and required of every target. With std's functions in place
/// of `det`'s, 12 of these 21 digests differ between macOS and glibc 2.36;
/// with `det`'s, none does.
#[test]
fn every_effect_lights_the_same_bits_on_every_os() {
    // Per effect, the fire to the liquid: three ages.
    const PINNED: &[u64] = &[
        0xd2f9cd061225cc9c,
        0x9fe4ea707cada082,
        0xf658669247387018,
        0x27409749ff8bb9c4,
        0x400095ff6130f1c8,
        0xee1831ad8fb50ec2,
        0x82f006dc2494b89f,
        0x7291ae525b93e1d6,
        0xd7bf31dde8df5836,
        0x527c6a7a798f7386,
        0xf2d9328e89e576b0,
        0xde8a9600149871f9,
        0x7400831d1a3c47c2,
        0x72f0fcdd5cb50277,
        0x29855f3695078bb5,
        0xad1d86e2eacc5844,
        0xfb277c62189575c0,
        0xe72326daace212d1,
        0x306d1c5b031247c1,
        0xb33147d642fffad0,
        0x8d34fd38f3850e2b,
    ];
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let r = renderer(&faces);
    let (w, h) = r.pixel_size();
    let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
    let fold = |h: u64, bits: u32| {
        bits.to_le_bytes()
            .iter()
            .fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
    };
    let start = smear(&r, Shape::Quad, (1, 2), (7, 20), 0.0);
    let tail = Pt {
        x: (start.corners.0[2].x + start.corners.0[0].x) * 0.5,
        y: (start.corners.0[0].y + start.corners.0[1].y) * 0.5,
    };
    let mut got = Vec::new();
    for effect in Effect::ALL {
        let end = effect.end(&Ignition::ignite(tail, start.hole, Seed(9), unit));
        for k in [0.1f32, 0.3, 0.6] {
            let age = k * end;
            let emitter = smear(&r, Shape::Quad, (1, 2), (7, 20), age);
            let vfx = Vfx {
                effect,
                emitter,
                ignition: Ignition::ignite(tail, emitter.hole, Seed(9), unit).at(age),
                axis: Pt { x: 0.95, y: 0.31 },
            };
            let mut d = 0xcbf2_9ce4_8422_2325u64;
            for c in emitter.corners.0 {
                d = fold(fold(d, c.x.to_bits()), c.y.to_bits());
            }
            for y in 0..px(h) {
                for x in 0..px(w) {
                    let p = Pt {
                        x: f32::from(u16::try_from(x).unwrap()) + 0.5,
                        y: f32::from(u16::try_from(y).unwrap()) + 0.5,
                    };
                    d = fold(d, vfx.light(p).to_bits());
                }
            }
            vfx.particles(|q| {
                for v in [q.centre.x, q.centre.y, q.radius, q.glow] {
                    d = fold(d, v.to_bits());
                }
            });
            got.push(d);
        }
    }
    let table: String = got
        .iter()
        .map(|d| format!("        0x{d:016x},\n"))
        .collect();
    assert!(
        got == PINNED,
        "the effects' lights moved; the table this target computes:\n{table}"
    );
}

/// A screen of `X`s in the default colour, `ws` cells, the cursor hidden.
fn letters(ws: &WinSize, f: impl FnOnce(&mut Term<'_>)) {
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    let mut bytes = b"\x1b[?25l".to_vec();
    for row in 0..usize::from(ws.ws_row) {
        bytes.extend(format!("\x1b[{};1H", row + 1).bytes());
        bytes.extend(core::iter::repeat_n(b'X', usize::from(ws.ws_col)));
    }
    feed(&mut t, &bytes);
    f(&mut t);
}

/// WCAG's relative luminance of a pixel's three bytes.
fn luminance(p: u32) -> f64 {
    let [_, r, g, b] = p.to_be_bytes();
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// WCAG's contrast ratio of two luminances.
fn contrast(a: f64, b: f64) -> f64 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// **What a frame of the plasma over the letters did to them**: how many
/// of the letters' solid ink pixels under its light (0.1 or more) moved
/// more than two steps in some channel, and of those how many the wrong
/// way -- darker on a dark theme, lighter on a light one; and the least
/// contrast a cell under the light keeps, WCAG's ratio, two ways: its
/// worst pair, the dimmest ink pixel against the brightest ground pixel on
/// a dark theme (the other way about on a light one), and its means, the
/// ink's mean luminance against the ground's.
struct Letters {
    moved: Tally,
    wrong_way: Tally,
    under: Tally,
    worst: f64,
    mean: f64,
}

fn letters_under(
    r: &Renderer<'_, '_>,
    v: &Vfx,
    (bare, lit): (&[u32], &[u32]),
    theme: &Theme,
) -> Letters {
    let width = px(r.pixel_size().0);
    let (cw, ch) = (px(r.metrics().cell_w), px(r.metrics().cell_h));
    let (cols, rows) = (width / cw, bare.len() / width / ch);
    let (ink, page) = (theme.fg.0 & 0x00FF_FFFF, theme.bg.0 & 0x00FF_FFFF);
    let dark = luminance(theme.bg.0) < luminance(theme.fg.0);
    let mut out = Letters {
        moved: Tally::default(),
        wrong_way: Tally::default(),
        under: Tally::default(),
        worst: f64::INFINITY,
        mean: f64::INFINITY,
    };
    for (row, col) in (0..rows).flat_map(|r| (0..cols).map(move |c| (r, c))) {
        let (mut inks, mut grounds) = (Vec::new(), Vec::new());
        for y in row * ch..(row + 1) * ch {
            for x in col * cw..(col + 1) * cw {
                let light = v.light(Pt {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                });
                if light < 0.1 {
                    continue;
                }
                let i = y * width + x;
                let (b, p) = (bare[i] & 0x00FF_FFFF, lit[i] & 0x00FF_FFFF);
                if b == ink {
                    out.under.bump();
                    inks.push(luminance(lit[i]));
                    let step = |s: u32| {
                        i32::try_from((p >> s) & 0xff).unwrap()
                            - i32::try_from((b >> s) & 0xff).unwrap()
                    };
                    let d = [step(0), step(8), step(16)];
                    if d.iter().any(|v| v.abs() > 2) {
                        out.moved.bump();
                        if d.iter().any(|&v| v != 0 && (v < 0) == dark) {
                            out.wrong_way.bump();
                        }
                    }
                } else if b == page {
                    grounds.push(luminance(lit[i]));
                }
            }
        }
        if inks.is_empty() || grounds.is_empty() {
            continue;
        }
        let lo = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = |v: &[f64]| v.iter().copied().fold(0.0, f64::max);
        let worst = match dark {
            true => contrast(lo(&inks), hi(&grounds)),
            false => contrast(hi(&inks), lo(&grounds)),
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        out.worst = out.worst.min(worst);
        out.mean = out.mean.min(contrast(mean(&inks), mean(&grounds)));
    }
    out
}

/// **The plasma lights a letter it crosses on a dark theme and shades it on
/// a light one**: over a screen of `X`s in the default colour at 2x, at
/// three ages from its brightest, every one of the letters' solid ink
/// pixels under its light moves by more than two steps, brighter on the
/// dark theme and darker on the light -- the renderer draws it by `screen`
/// and `multiply`. The control is the same records composited source-over
/// ([`effect_reference`] with [`Mix::Normal`]), which moves none, because
/// the cursor's colour is the ink's. Red on a plasma drawn source-over.
///
/// And **what it leaves of a letter's contrast**, WCAG's ratio of the
/// text's luminance to its background's (1.4.3, and 1.4.11 for a letter
/// read as a graphic), printed beside source-over's and the bare frame's:
/// in front keeps more of it than source-over on both themes, on both
/// measures at every age, because it moves the ink with the ground. At its
/// brightest, 0.03 s in, the means are 5.68:1 on the dark theme and 7.93:1
/// on the light (source-over 5.21 and 7.05; bare 13.35 and 12.99); the
/// worst pair, a filament on the ground beside a letter's dimmest ink, is
/// 1.38 and 1.58 (source-over 1.33 and 1.51): the filament is the text's
/// own colour at up to 0.9 of its opacity. Over the bare page it draws as
/// source-over does within 3 steps of 255 on all but one pixel in a
/// hundred: where an ion lies over the tube, whose light `screen` adds to
/// rather than covers -- 26 of 8,755 at 0.1 s, by up to 25 steps.
#[test]
fn the_plasma_lights_a_letter_it_crosses_and_shades_it_on_a_light_theme() {
    const LETTERS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 8,
    };
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    // The cursor of no colour, whose trail is the letters' own colour.
    for (name, theme, mix) in [
        ("dark", support::classic(Theme::dark()), Mix::Screen),
        ("light", support::classic(Theme::light()), Mix::Multiply),
    ] {
        letters(&LETTERS, |t| {
            let mut r = Renderer::new(
                Stack::new(&faces).unwrap(),
                PxSize::of(28.0).unwrap(),
                theme.clone(),
                Blend::Native,
                Cols::of_index(usize::from(LETTERS.ws_col)),
                Rows::of_index(usize::from(LETTERS.ws_row)),
            )
            .unwrap();
            let (w, h) = r.pixel_size();
            let width = px(w);
            let mut bare = vec![0u32; width * px(h)];
            r.set_trail(None);
            paint(&mut r, t, &mut bare, Damage::Full);
            let style = baud_core::CursorStyle::SteadyBlock;
            let rect = |col: usize| r.cursor_rect(Row::from_index(3), Col::at_index(col), style);
            let (from, to) = (rect(4), rect(34));
            let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
            let lit_at = Ignition::ignite(
                Pt {
                    x: (from.left + from.right) * 0.5,
                    y: (from.top + from.bottom) * 0.5,
                },
                to,
                Seed(5),
                unit,
            );
            let bare_letters = letters_under(
                &r,
                &plasma_at(lit_at, from, to, 0.03),
                (&bare, &bare),
                &theme,
            );
            for age in [0.03f32, 0.06, 0.1] {
                let v = plasma_at(lit_at, from, to, age);
                r.set_trail(Some(TrailLook::Vfx(v)));
                let mut lit = vec![0u32; width * px(h)];
                paint(&mut r, t, &mut lit, Damage::Full);
                let got = letters_under(&r, &v, (&bare, &lit), &theme);
                let over = effect_reference(&v, r.trail(), &bare, w, Mix::Normal);
                let control = letters_under(&r, &v, (&bare, &over), &theme);
                // The page under the plasma, in front against source-over.
                let page = theme.bg.0 & 0x00FF_FFFF;
                let steps = |p: u32, q: u32| {
                    [0u32, 8, 16]
                        .map(|k| ((p >> k) & 0xff).abs_diff((q >> k) & 0xff))
                        .into_iter()
                        .max()
                        .unwrap()
                };
                let (mut ground, mut far, mut most) = (0usize, 0usize, 0u32);
                for (i, &b) in bare.iter().enumerate() {
                    if b & 0x00FF_FFFF != page || lit[i] == b {
                        continue;
                    }
                    let d = steps(lit[i], over[i]);
                    ground += 1;
                    far += usize::from(d > 3);
                    most = most.max(d);
                }
                eprintln!(
                    "{name} at {age}: of {ground} page pixels lit, {far} more than 3 steps from \
                     source-over, at most {most}"
                );
                assert!(
                    ground > 1000 && far * 100 <= ground,
                    "{name} at {age}: {far} of {ground} page pixels more than 3 steps from \
                     source-over"
                );
                eprintln!(
                    "{name} at {age}: {} of {} ink pixels moved; least contrast, worst pair \
                     {:.2}:1 (source-over {:.2}:1), means {:.2}:1 (source-over {:.2}:1); \
                     bare {:.2}:1",
                    got.moved.value(),
                    got.under.value(),
                    got.worst,
                    control.worst,
                    got.mean,
                    control.mean,
                    bare_letters.worst
                );
                assert!(
                    got.under.value() > 200,
                    "{name} at {age}: the control, only {} ink pixels under the light",
                    got.under.value()
                );
                assert_eq!(
                    got.moved,
                    got.under,
                    "{name} at {age}: of {} ink pixels under the plasma, {} moved",
                    got.under.value(),
                    got.moved.value()
                );
                assert_eq!(
                    got.wrong_way,
                    Tally::default(),
                    "{name} at {age}: a letter moved the wrong way"
                );
                assert_eq!(
                    control.moved,
                    Tally::default(),
                    "{name} at {age}: the control, source-over moved {} ink pixels",
                    control.moved.value()
                );
                assert!(
                    got.worst > control.worst && got.mean > control.mean,
                    "{name} at {age}: in front keeps no more contrast than source-over"
                );
                assert_eq!(r.trail_mix(), mix, "{name}: the plasma's mix");
            }
        });
    }
}

/// The plasma of the ignition `lit` riding kitty's quad from `from` to
/// `to`, `age` seconds in.
fn plasma_at(lit: Ignition, from: baud_render::Rect, to: baud_render::Rect, age: f32) -> Vfx {
    Vfx {
        effect: Effect::Plasma,
        emitter: Smear {
            corners: Segment::new(from.corners(), to).at(age),
            hole: to,
            shape: Shape::Quad,
            strength: baud_render::Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: lit.at(age),
        axis: lit.dir,
    }
}
