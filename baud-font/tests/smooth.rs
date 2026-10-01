//! Font smoothing against CoreText's own masks (note J).
//!
//! `tests/smooth/ct-smooth-*.bin` are CoreText's masks of JetBrains Mono
//! Regular, smoothing off and at each of the five levels, written by
//! `scripts/text-probes/smooth-fixtures.swift` on macOS 27. The gate is per
//! edge and model-free: on the flat edges of the block elements, how far
//! each edge moved between the undilated and the dilated mask, measured the
//! same way on baud's masks and on CoreText's, held to the formula.

use baud_domain::Codepoint;
use baud_font::outline::Sink;
use baud_font::{
    Bounds, Canvas, Dilation, Extent, Face, Fit, Phase, Px, Rasterizer, Scale, SmoothLevel,
    Smoother, Transform,
};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fonts/JetBrainsMono-Regular.ttf"
);
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/smooth");

/// The block elements, whose edges are flat at every size: full, upper
/// half, lower half, left half, right half.
const BLOCKS: [char; 5] = ['\u{2588}', '\u{2580}', '\u{2584}', '\u{258C}', '\u{2590}'];

fn is_block(cp: Codepoint) -> bool {
    BLOCKS.iter().any(|&c| Codepoint::from_char(c) == cp)
}

/// The last printable ASCII character: the fixtures' glyphs up to it are
/// text, the ones past it the block elements.
fn is_text(cp: Codepoint) -> bool {
    cp <= Codepoint::from_char('~')
}

/// A mask on a grid whose origin is the glyph origin: pixel `(x, y)` of
/// `px` is at `(x - ox, y - oy)` from it, `y` down.
struct Mask {
    w: i32,
    h: i32,
    ox: i32,
    oy: i32,
    px: Vec<u8>,
}

/// A pixel, counted from the glyph origin, `y` down.
#[derive(Clone, Copy)]
struct Pixel {
    x: i32,
    y: i32,
}

impl Mask {
    /// Coverage at `p`, zero outside the mask.
    fn at(&self, p: Pixel) -> f64 {
        let (mx, my) = (p.x + self.ox, p.y + self.oy);
        if mx < 0 || my < 0 || mx >= self.w || my >= self.h {
            return 0.0;
        }
        let i = usize::try_from(my * self.w + mx).unwrap();
        f64::from(self.px[i]) / 255.0
    }
}

/// One glyph of a fixture: smoothing off, then levels 0-4.
struct Fixed {
    cp: Codepoint,
    masks: Vec<Mask>,
}

struct Fixture {
    ppem: f32,
    glyphs: Vec<Fixed>,
}

/// Every fixture, refused when it was not made from the font the test
/// draws: CoreText's masks of another file say nothing about this one.
fn fixtures(font: &[u8]) -> Vec<Fixture> {
    let mut paths: Vec<_> = std::fs::read_dir(FIXTURES)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("ct-smooth-") && n.ends_with(".bin"))
        })
        .collect();
    paths.sort();
    let out: Vec<_> = paths
        .iter()
        .map(|p| parse(&std::fs::read(p).unwrap(), font))
        .collect();
    assert_eq!(
        out.len(),
        18,
        "the fixtures are 9.001-18.001 px and 20-48 px"
    );
    out
}

fn parse(b: &[u8], font: &[u8]) -> Fixture {
    let u16le = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32le = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    assert_eq!(&b[..4], b"BDIL");
    assert_eq!(u16le(4), 1, "fixture version");
    let count = usize::from(u16le(6));
    let ppem = f32::from_bits(u32le(8));
    assert_eq!(
        (u32le(12), u32le(16)),
        (
            u32::try_from(font.len()).unwrap(),
            font.iter().fold(0x811c_9dc5u32, |h, &b| {
                (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
            })
        ),
        "the fixtures were made from another font file; regenerate them"
    );
    assert_eq!(b[20], 5, "level count");
    let mut off = 29 + usize::from(u16le(27));
    let mut glyphs = Vec::new();
    for _ in 0..count {
        let cp = Codepoint::new(u32le(off)).unwrap();
        let (w, h) = (i32::from(u16le(off + 4)), i32::from(u16le(off + 6)));
        let cells = usize::try_from(w * h).unwrap();
        let ox = i32::from(i16::from_le_bytes([b[off + 8], b[off + 9]]));
        let oy = i32::from(i16::from_le_bytes([b[off + 10], b[off + 11]]));
        off += 12;
        let masks = (0..6)
            .map(|_| {
                let px = b[off..off + cells].to_vec();
                off += cells;
                Mask { w, h, ox, oy, px }
            })
            .collect();
        glyphs.push(Fixed { cp, masks });
    }
    assert_eq!(off, b.len());
    Fixture { ppem, glyphs }
}

/// baud's mask of `cp` at `ppem`: undilated through the 1x rasteriser when
/// `level` is `None`, dilated through the supersample otherwise.
fn baud(
    face: &Face<'_>,
    cp: Codepoint,
    ppem: f32,
    level: Option<SmoothLevel>,
    phase: Phase,
) -> Mask {
    let d = level.map(|l| Dilation::smoothing(l, ppem).unwrap());
    draw(face, cp, ppem, d, phase)
}

fn fit(face: &Face<'_>, cp: Codepoint, ppem: f32, phase: Phase) -> Fit {
    let g = face.glyph(cp).unwrap();
    let mut b = Bounds::new();
    face.outline(g, &mut b);
    Fit::at(&b, Scale::of(ppem, face.units_per_em()), phase).unwrap()
}

fn draw(face: &Face<'_>, cp: Codepoint, ppem: f32, d: Option<Dilation>, phase: Phase) -> Mask {
    draw_at(face, cp, fit(face, cp, ppem, phase), d)
}

fn draw_at(face: &Face<'_>, cp: Codepoint, fit: Fit, d: Option<Dilation>) -> Mask {
    let g = face.glyph(cp).unwrap();
    let mut px = vec![0u8; fit.extent.cells().unwrap()];
    match d {
        None => {
            let mut scratch = vec![0f32; fit.extent.scratch().unwrap()];
            let mut r = Rasterizer::new(
                Canvas::new(fit.extent, &mut scratch).unwrap(),
                fit.transform,
            );
            face.outline(g, &mut r);
            r.finish().coverage(&mut px).unwrap();
        }
        Some(d) => {
            let mut scratch = vec![0f32; Smoother::canvas(&fit).unwrap().scratch().unwrap()];
            let mut line = vec![0f32; Dilation::line(fit.extent).unwrap().cells().unwrap()];
            let mut r = Smoother::new(&fit, &mut scratch).unwrap();
            face.outline(g, &mut r);
            r.finish(d, &mut line, &mut px).unwrap();
        }
    }
    Mask {
        w: i32::try_from(fit.extent.w.value()).unwrap(),
        h: i32::try_from(fit.extent.h.value()).unwrap(),
        ox: -fit.left,
        oy: -fit.top,
        px,
    }
}

/// The four edges of a solid's ink, in pixels from the glyph origin,
/// measured through its middle row and column: an axis-aligned edge's
/// coverage sums to exactly the area inside it, so the edge is the middle
/// less the summed coverage on that side. `(left, right, top, bottom)`.
fn edges(m: &Mask) -> (f64, f64, f64, f64) {
    let (x0, x1) = (-m.ox, m.w - m.ox);
    let (y0, y1) = (-m.oy, m.h - m.oy);
    // The ink's middle, from where it is opaque.
    let solid: Vec<(i32, i32)> = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| m.at(Pixel { x, y }) >= 0.999)
        .collect();
    assert!(!solid.is_empty(), "no opaque pixel to measure from");
    let cx =
        (solid.iter().map(|p| p.0).min().unwrap() + solid.iter().map(|p| p.0).max().unwrap()) / 2;
    let cy =
        (solid.iter().map(|p| p.1).min().unwrap() + solid.iter().map(|p| p.1).max().unwrap()) / 2;
    let left = f64::from(cx) - (x0..cx).map(|x| m.at(Pixel { x, y: cy })).sum::<f64>();
    let right = f64::from(cx) + (cx..x1).map(|x| m.at(Pixel { x, y: cy })).sum::<f64>();
    let top = f64::from(cy) - (y0..cy).map(|y| m.at(Pixel { x: cx, y })).sum::<f64>();
    let bottom = f64::from(cy) + (cy..y1).map(|y| m.at(Pixel { x: cx, y })).sum::<f64>();
    (left, right, top, bottom)
}

/// How far each edge moved outward from `off` to `on`: left, right, top
/// (up, toward smaller `y`) and bottom.
fn growth(off: &Mask, on: &Mask) -> [f64; 4] {
    let (l0, r0, t0, b0) = edges(off);
    let (l1, r1, t1, b1) = edges(on);
    [l0 - l1, r1 - r0, t0 - t1, b1 - b0]
}

/// The formula's growth per edge: `d` on the sides, `2d` on top, none below.
fn predicted(level: SmoothLevel, ppem: f32) -> [f64; 4] {
    let d = f64::from(Dilation::smoothing(level, ppem).unwrap().per_edge());
    [d, d, 2.0 * d, 0.0]
}

/// One edge read from two u8 masks: each truncates, so a single site is
/// good to two steps of 255.
const EDGE: f64 = 2.0 / 255.0;

/// Holds every edge to [`EDGE`] and returns the signed errors, so a bias
/// below the per-site step still shows in their mean.
fn check(what: &str, got: [f64; 4], want: [f64; 4], errors: &mut Vec<f64>) {
    for (i, side) in ["left", "right", "top", "bottom"].iter().enumerate() {
        let e = got[i] - want[i];
        errors.push(e);
        assert!(
            e.abs() <= EDGE,
            "{what}: the {side} edge grew {:+.4} px, the formula says {:+.4}",
            got[i],
            want[i]
        );
    }
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// On every fixture size and level, each flat edge of each block element
/// grows by the formula, in CoreText's masks and in baud's: the formula is
/// CoreText's, and baud draws the formula.
#[test]
fn every_flat_edge_grows_by_the_formula_in_coretext_and_in_baud() {
    let font = std::fs::read(FONT).unwrap();
    let face = Face::parse(&font).unwrap();
    let mut sites = 0;
    let (mut ct, mut ours) = (Vec::new(), Vec::new());
    for fx in fixtures(&font) {
        for g in fx.glyphs.iter().filter(|g| is_block(g.cp)) {
            let off = baud(&face, g.cp, fx.ppem, None, Phase::ZERO);
            for (i, level) in SmoothLevel::ALL.into_iter().enumerate() {
                let want = predicted(level, fx.ppem);
                let what = format!("U+{:04X} at {} px, {level:?}", g.cp.value(), fx.ppem);
                check(
                    &format!("CoreText {what}"),
                    growth(&g.masks[0], &g.masks[1 + i]),
                    want,
                    &mut ct,
                );
                let on = baud(&face, g.cp, fx.ppem, Some(level), Phase::ZERO);
                check(&format!("baud {what}"), growth(&off, &on), want, &mut ours);
                sites += 1;
            }
        }
    }
    // Positive control: every block on every size and level was measured.
    assert_eq!(sites, 18 * 5 * 5, "{sites} sites");
    // No bias under the per-site step: the formula's d, not one near it.
    for (who, e) in [("CoreText", &ct), ("baud", &ours)] {
        assert!(
            mean(e).abs() <= 0.001,
            "{who}: mean edge error {:+.5}",
            mean(e)
        );
    }
}

/// The level follows the fill's linear luminance, with the steps where the
/// probes found them, and the growth is capped per edge.
#[test]
fn the_level_and_the_growth_are_coretexts() {
    use SmoothLevel::*;
    let cases = [
        (0.0, L0),
        (0.124, L0),
        (0.126, L1),
        (0.374, L1),
        (0.376, L2),
        (0.624, L2),
        (0.626, L3),
        (0.874, L3),
        (0.876, L4),
        (1.0, L4),
        (f32::NAN, L0),
    ];
    for (y, want) in cases {
        assert_eq!(SmoothLevel::of_luminance(y), want, "Y {y}");
    }
    let d = |l, p| Dilation::smoothing(l, p).unwrap().per_edge();
    assert!((d(L0, 10.0) - 0.058).abs() < 1e-6);
    assert!((d(L4, 10.0) - 0.158).abs() < 1e-6);
    assert_eq!(d(L4, 19.0), 0.30, "L4 caps from 18.99 px");
    assert!(
        d(L0, 51.0) < 0.30 && d(L0, 52.0) == 0.30,
        "L0 caps from 51.7 px"
    );
    assert_eq!(Dilation::smoothing(L0, 0.0), None);
}

/// With nothing to grow, the smoothing path draws a straight edge exactly
/// where the 1x rasteriser does: the scale, the registration and the
/// average agree. Curves are not compared here -- their chords are
/// CoreGraphics' on this path and baud's own on the other.
#[test]
fn an_undilated_supersample_draws_a_straight_edge_where_the_rasteriser_does() {
    let font = std::fs::read(FONT).unwrap();
    let face = Face::parse(&font).unwrap();
    for ppem in [9.001, 11.0, 14.0, 28.0, 48.0] {
        for cp in BLOCKS.map(Codepoint::from_char) {
            let one = baud(&face, cp, ppem, None, Phase::ZERO);
            let sup = draw(&face, cp, ppem, Some(Dilation::NONE), Phase::ZERO);
            assert_eq!(
                (one.w, one.h, one.ox, one.oy),
                (sup.w, sup.h, sup.ox, sup.oy)
            );
            for (i, (a, b)) in one.px.iter().zip(&sup.px).enumerate() {
                assert!(
                    a.abs_diff(*b) <= 2,
                    "U+{:04X} at {ppem} px, pixel {i}: {a} against {b}",
                    cp.value()
                );
            }
        }
    }
}

/// What baud's smoothing path draws against CoreText's mask of the same
/// glyph: the mean difference over touched pixels, the largest, and the
/// share that differ by 16 codes or more.
struct Against {
    mean: f64,
    max: f64,
    big: f64,
}

/// `level` names the CoreText mask -- smoothing off when `None` -- and the
/// growth baud draws.
fn against(face: &Face<'_>, fx: &Fixture, level: Option<SmoothLevel>) -> Against {
    let (mask, d) = match level {
        None => (0, Dilation::NONE),
        Some(l) => (
            1 + SmoothLevel::ALL.iter().position(|&x| x == l).unwrap(),
            Dilation::smoothing(l, fx.ppem).unwrap(),
        ),
    };
    let (mut sum, mut n, mut max, mut big) = (0.0, 0usize, 0.0f64, 0usize);
    for g in fx.glyphs.iter().filter(|g| is_text(g.cp)) {
        let m = draw(face, g.cp, fx.ppem, Some(d), Phase::ZERO);
        let ct = &g.masks[mask];
        let (x0, x1) = ((-m.ox).min(-ct.ox), (m.w - m.ox).max(ct.w - ct.ox));
        let (y0, y1) = ((-m.oy).min(-ct.oy), (m.h - m.oy).max(ct.h - ct.oy));
        for y in y0..y1 {
            for x in x0..x1 {
                let (a, b) = (m.at(Pixel { x, y }), ct.at(Pixel { x, y }));
                if a > 0.0 || b > 0.0 {
                    let e = (a - b).abs();
                    sum += e;
                    n += 1;
                    max = max.max(e);
                    if e >= 16.0 / 255.0 {
                        big += 1;
                    }
                }
            }
        }
    }
    Against {
        mean: sum / n as f64,
        max,
        big: big as f64 / n as f64,
    }
}

/// With nothing grown, the smoothing path is CoreText's mask with
/// smoothing off: CoreGraphics' chords, rasterised by baud. Measured at
/// 0.0011-0.0021 mean and 2 codes at most, except where two contours of
/// `$` and `8` overlap at 20 px and an accumulation rasteriser and a
/// winding fill take different areas (0.15 at worst, 0.08% of pixels).
#[test]
fn an_undilated_smoothing_path_is_coretext_with_smoothing_off() {
    let font = std::fs::read(FONT).unwrap();
    let face = Face::parse(&font).unwrap();
    for fx in fixtures(&font) {
        let a = against(&face, &fx, None);
        assert!(
            a.mean <= 0.0025 && a.max <= 0.16 && a.big <= 0.001,
            "{} px: mean {:.4}, max {:.3}, {:.3}% at 16 codes or more",
            fx.ppem,
            a.mean,
            a.max,
            100.0 * a.big
        );
    }
}

/// Grown, the smoothing path is CoreText's smoothed mask at every level:
/// measured at 0.0021-0.0075 mean, 0.28 at worst, and at most 1.93% of
/// touched pixels 16 codes or more apart at the eightfold supersample (at
/// sixteen, 0.0018-0.0052) -- the box model's residual, which the flat-edge
/// gate shows is not the growth.
#[test]
fn the_smoothing_path_is_coretext_with_smoothing_on() {
    let font = std::fs::read(FONT).unwrap();
    let face = Face::parse(&font).unwrap();
    for fx in fixtures(&font) {
        for level in SmoothLevel::ALL {
            let a = against(&face, &fx, Some(level));
            assert!(
                a.mean <= 0.008 && a.max <= 0.3 && a.big <= 0.025,
                "{} px {level:?}: mean {:.4}, max {:.3}, {:.2}% at 16 codes or more",
                fx.ppem,
                a.mean,
                a.max,
                100.0 * a.big
            );
        }
    }
}

/// A dilated mask under a phase is the dilated mask moved by the phase: a
/// stem's left edge moves by exactly that much.
#[test]
fn a_phase_moves_a_dilated_stem_by_exactly_that_much() {
    let font = std::fs::read(FONT).unwrap();
    let face = Face::parse(&font).unwrap();
    let left = |m: &Mask| -> f64 {
        let (l, _, _, _) = edges(m);
        l
    };
    let m = face
        .default_advance(face.glyph(Codepoint::from_char('M')).unwrap())
        .unwrap();
    for ppem in [13.0f32, 28.0] {
        let scale = Scale::of(ppem, face.units_per_em());
        let base = baud(
            &face,
            Codepoint::from_char('\u{2588}'),
            ppem,
            Some(SmoothLevel::L3),
            Phase::ZERO,
        );
        let w = u32::try_from(scale.px(600.0).round() as i32).unwrap();
        for span in [w - 1, w + 1, w + 2] {
            let p = Phase::centring(scale, m, Px::of(span)).unwrap();
            let want = (f64::from(span) - f64::from(scale.px(600.0))) / 2.0;
            let moved = baud(
                &face,
                Codepoint::from_char('\u{2588}'),
                ppem,
                Some(SmoothLevel::L3),
                p,
            );
            let got = left(&moved) - left(&base);
            assert!(
                (got - want).abs() < 0.006,
                "{ppem} px in {span}: moved {got:.4}, not {want:.4}"
            );
        }
    }
}

/// The largest growth is never clipped: the top grows 0.6 px, past a
/// pixel of margin when the edge is near a pixel's top, but [`Fit`] starts
/// the canvas a whole pixel above the pixel the ink starts in. Drawn into a
/// canvas two pixels larger on every side, the mask is the same, to the
/// truncation of one step.
#[test]
fn the_capped_growth_is_never_clipped() {
    let font = std::fs::read(FONT).unwrap();
    let face = Face::parse(&font).unwrap();
    for ppem in [13.0, 28.0, 48.0, 64.0] {
        for cp in ['\u{2588}', 'O', 'W', '_'].map(Codepoint::from_char) {
            let fit = fit(&face, cp, ppem, Phase::ZERO);
            let wide = Fit {
                extent: Extent::new(
                    Px::of(fit.extent.w.value() + 4),
                    Px::of(fit.extent.h.value() + 4),
                ),
                transform: Transform {
                    e: fit.transform.e + 2.0,
                    f: fit.transform.f + 2.0,
                    ..fit.transform
                },
                left: fit.left - 2,
                top: fit.top - 2,
            };
            let d = Dilation::smoothing(SmoothLevel::L4, ppem).unwrap();
            let (m, big) = (
                draw_at(&face, cp, fit, Some(d)),
                draw_at(&face, cp, wide, Some(d)),
            );
            for y in -big.oy..big.h - big.oy {
                for x in -big.ox..big.w - big.ox {
                    // Moving the outline two pixels moves its float
                    // arithmetic, which can flip a truncation; clipping
                    // would lose a whole edge's coverage.
                    assert!(
                        (m.at(Pixel { x, y }) - big.at(Pixel { x, y })).abs() <= 1.5 / 255.0,
                        "U+{:04X} at {ppem} px, ({x}, {y}) from the origin: {} against {}",
                        cp.value(),
                        m.at(Pixel { x, y }),
                        big.at(Pixel { x, y })
                    );
                }
            }
        }
    }
}

/// Cubic paths -- circles and random closed blobs -- drawn through the
/// smoothing path with nothing grown, against CoreGraphics' own fill of
/// them at flatness 1 (`scripts/text-probes/smooth-cubics.swift`). JetBrains
/// Mono's outlines are quadratics, so this is the only gate on the cubic
/// half of the flattening rule, which CFF faces -- most CJK fallbacks --
/// take.
#[test]
fn a_cubic_is_cut_where_coregraphics_cuts_it() {
    let b = std::fs::read(format!("{FIXTURES}/cg-cubics.bin")).unwrap();
    assert_eq!(&b[..4], b"BCUB");
    let u16le = |o: usize| usize::from(u16::from_le_bytes([b[o], b[o + 1]]));
    let f32le = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    assert_eq!(u16le(4), 1, "fixture version");
    let count = u16le(6);
    let mut off = 8;
    let (mut worst, mut sum, mut n, mut cubics) = (0u8, 0u64, 0u64, 0usize);
    for shape in 0..count {
        let (w, h, ops) = (u16le(off), u16le(off + 2), u16le(off + 4));
        off += 6;
        let fit = Fit {
            extent: Extent::new(
                Px::of(u32::try_from(w).unwrap()),
                Px::of(u32::try_from(h).unwrap()),
            ),
            transform: Transform::IDENTITY,
            left: 0,
            top: 0,
        };
        let mut scratch = vec![0f32; Smoother::canvas(&fit).unwrap().scratch().unwrap()];
        let mut line = vec![0f32; Dilation::line(fit.extent).unwrap().cells().unwrap()];
        let mut s = Smoother::new(&fit, &mut scratch).unwrap();
        for _ in 0..ops {
            let kind = b[off];
            off += 1;
            let p = |i: usize| f32le(off + 4 * i);
            match kind {
                0 => s.move_to(p(0), p(1)),
                1 => s.line_to(p(0), p(1)),
                3 => {
                    s.curve_to(p(0), p(1), p(2), p(3), p(4), p(5));
                    cubics += 1;
                }
                4 => s.close(),
                k => panic!("op {k}"),
            }
            off += 4 * [2, 2, 0, 6, 0][usize::from(kind)];
        }
        let mut mask = vec![0u8; w * h];
        s.finish(Dilation::NONE, &mut line, &mut mask).unwrap();
        let cg = &b[off..off + w * h];
        off += w * h;
        for (i, (a, c)) in mask.iter().zip(cg).enumerate() {
            let e = a.abs_diff(*c);
            if *a > 0 || *c > 0 {
                sum += u64::from(e);
                n += 1;
            }
            assert!(
                e <= 2,
                "shape {shape} ({w}x{h}), pixel {i}: baud {a}, CoreGraphics {c}"
            );
            worst = worst.max(e);
        }
    }
    assert_eq!(off, b.len());
    // Positive control: the fixture is cubics, and they were drawn.
    assert!(cubics > 100, "{cubics} cubics");
    // Measured: 2 codes at worst and 0.140 on average, over 9365 pixels.
    let mean = sum as f64 / n as f64;
    assert!(mean <= 0.2, "mean {mean:.3} codes, worst {worst}");
}

/// The level follows the fill's Rec.709 luminance, in colour as well as in
/// grey: CoreText's masks of a full block in 21 coloured fills, each edge's
/// growth against the level Rec.709 names. A grey cannot tell weightings
/// apart; these do -- pure green is level 3 under Rec.709 and 2 under
/// Rec.601, magenta 1 and 2, ANSI green 2 and 1 -- and Rec.601 misses 8 of
/// the 21 at 13 px, 8 at 20 px and 3 at 28 px, where levels 2-4 are capped
/// alike.
#[test]
fn the_level_is_the_fills_rec709_luminance() {
    let font = std::fs::read(FONT).unwrap();
    let b = std::fs::read(format!("{FIXTURES}/ct-colours.bin")).unwrap();
    assert_eq!(&b[..4], b"BCOL");
    let u16le = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32le = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    assert_eq!(u16le(4), 1, "fixture version");
    assert_eq!(
        u32le(8),
        u32::try_from(font.len()).unwrap(),
        "the colour fixtures were made from another font file"
    );
    let count = usize::from(u16le(6));
    let lin = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let mut off = 16;
    let (mut errors, mut apart) = (Vec::new(), 0);
    for _ in 0..count {
        let ppem = f32::from_le_bytes(b[off..off + 4].try_into().unwrap());
        let (r, g, bl) = (b[off + 4], b[off + 5], b[off + 6]);
        let (w, h) = (i32::from(u16le(off + 7)), i32::from(u16le(off + 9)));
        let ox = i32::from(i16::from_le_bytes([b[off + 11], b[off + 12]]));
        let oy = i32::from(i16::from_le_bytes([b[off + 13], b[off + 14]]));
        off += 15;
        let n = usize::try_from(w * h).unwrap();
        let mask = |o: usize| Mask {
            w,
            h,
            ox,
            oy,
            px: b[o..o + n].to_vec(),
        };
        let (dark, lit) = (mask(off), mask(off + n));
        off += 2 * n;
        let y709 = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(bl);
        let y601 = 0.299 * lin(r) + 0.587 * lin(g) + 0.114 * lin(bl);
        let level = SmoothLevel::of_luminance(y709);
        let other = SmoothLevel::of_luminance(y601);
        if predicted(level, ppem) != predicted(other, ppem) {
            apart += 1;
        }
        check(
            &format!("CoreText ({r},{g},{bl}) at {ppem} px, {level:?}"),
            growth(&dark, &lit),
            predicted(level, ppem),
            &mut errors,
        );
    }
    assert_eq!(off, b.len());
    // Positive control: the fills tell the two weightings apart.
    assert_eq!(
        apart, 19,
        "{apart} fills where Rec.601 and Rec.709 predict different growth"
    );
}
