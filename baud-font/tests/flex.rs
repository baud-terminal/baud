//! A flex shallower than its threshold draws straight, through a face.
//!
//! Both charstring texts say a flex's two curves "must be rendered as a
//! straight line when the flex depth is less than" its threshold in device
//! pixels -- `fd`/100 for `flex`, 0.5 for `hflex`, `hflex1` and `flex1`
//! (`[[source: adobe-5177-type2]]` § 4.1, `[[source: ms-opentype-cff2]]`,
//! "Flex mechanism operators"). The reader hands the sink a `Flex`, and the
//! rasteriser decides at its own pixels. These tests drive two committed
//! faces through `Face::outline` -- STIX Two Math, whose CFF charstrings and
//! subroutines carry `hflex` and `hflex1`, and the CFF2 cases font, which
//! carries all four operators -- so the face's own wrapper must pass a flex on
//! whole for the rasteriser to see one.
//!
//! **The expected masks are replays, not a second copy of the rule.** A
//! recording sink keeps each flex whole, and each test picks two sizes from
//! the depths it recorded: one where every recorded flex is under half a
//! pixel, one where none is. The face's mask at the first must be the replay
//! with every flex a line, and differ from the replay with every flex
//! curved -- the guard that says the straightening is visible at all -- and
//! at the second it must be the curved replay.

use baud_domain::GlyphId;
use baud_font::{Bounds, Canvas, Face, Fit, Flex, Rasterizer, Scale, Sink};

/// One outline command, a flex kept whole.
#[derive(Clone, Debug)]
enum Cmd {
    Move(f32, f32),
    Line(f32, f32),
    Quad([f32; 4]),
    Curve([f32; 6]),
    Flex(Flex),
    Close,
}

#[derive(Default)]
struct Record(Vec<Cmd>);

impl Sink for Record {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Cmd::Move(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Cmd::Line(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push(Cmd::Quad([x1, y1, x, y]));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(Cmd::Curve([x1, y1, x2, y2, x, y]));
    }
    fn close(&mut self) {
        self.0.push(Cmd::Close);
    }
    fn flex(&mut self, f: &Flex) {
        self.0.push(Cmd::Flex(*f));
    }
}

/// How a replay draws every flex.
#[derive(Clone, Copy)]
enum Every {
    Straight,
    Curved,
}

fn replay<S: Sink>(cmds: &[Cmd], every: Every, s: &mut S) {
    let n = flexes(cmds).len();
    replay_each(cmds, &vec![every; n], s);
}

/// A replay that draws the `k`th flex as `each[k]` says.
fn replay_each<S: Sink>(cmds: &[Cmd], each: &[Every], s: &mut S) {
    let mut k = 0;
    for c in cmds {
        match *c {
            Cmd::Move(x, y) => s.move_to(x, y),
            Cmd::Line(x, y) => s.line_to(x, y),
            Cmd::Quad([a, b, x, y]) => s.quad_to(a, b, x, y),
            Cmd::Curve([a, b, c, d, x, y]) => s.curve_to(a, b, c, d, x, y),
            Cmd::Close => s.close(),
            Cmd::Flex(f) => {
                let [a, b, c, d, x, y] = f.first;
                let [e, g, h, i, x2, y2] = f.second;
                let every = each[k];
                k += 1;
                match every {
                    Every::Straight => s.line_to(x2, y2),
                    Every::Curved => {
                        s.curve_to(a, b, c, d, x, y);
                        s.curve_to(e, g, h, i, x2, y2);
                    }
                }
            }
        }
    }
}

/// The mask `draw` makes at `fit`.
fn mask(fit: &Fit, draw: impl FnOnce(&mut Rasterizer<'_>)) -> Vec<u8> {
    let mut scratch = vec![0f32; fit.extent.scratch().unwrap()];
    let canvas = Canvas::new(fit.extent, &mut scratch).unwrap();
    let mut r = Rasterizer::new(canvas, fit.transform);
    draw(&mut r);
    let canvas = r.finish();
    let mut out = vec![0u8; fit.extent.cells().unwrap()];
    canvas.coverage(&mut out).unwrap();
    out
}

/// The flexes `g` hands its sink.
fn flexes(cmds: &[Cmd]) -> Vec<Flex> {
    cmds.iter()
        .filter_map(|c| match c {
            Cmd::Flex(f) => Some(*f),
            _ => None,
        })
        .collect()
}

fn depth(f: &Flex) -> f32 {
    f.depth[0].hypot(f.depth[1])
}

/// Checks `g` at a size where every flex is under its threshold and at one
/// where none is, and says which sizes it used.
fn straddles(face: &Face<'_>, g: GlyphId) -> (u32, u32) {
    let mut rec = Record::default();
    face.outline(g, &mut rec);
    let fs = flexes(&rec.0);
    assert!(!fs.is_empty(), "{g:?} hands its sink no flex");
    let upem = f32::from(face.units_per_em().value());
    // Every threshold here is 0.5: px * depth / upem < 0.5.
    assert!(fs.iter().all(|f| f.threshold == 0.5), "{fs:?}");
    let deepest = fs.iter().map(depth).fold(0.0, f32::max);
    let shallowest = fs.iter().map(depth).fold(f32::MAX, f32::min);
    let low = (1..2000u32)
        .rev()
        .find(|&px| px as f32 * deepest / upem < 0.45)
        .unwrap();
    let high = (1..2000u32)
        .find(|&px| px as f32 * shallowest / upem > 0.55)
        .unwrap();
    for (px, every) in [(low, Every::Straight), (high, Every::Curved)] {
        let scale = Scale::of(px as f32, face.units_per_em());
        let mut b = Bounds::new();
        face.outline(g, &mut b);
        let fit = Fit::of(&b, scale).unwrap();
        let drawn = mask(&fit, |r| {
            face.outline(g, r);
        });
        let straight = mask(&fit, |r| replay(&rec.0, Every::Straight, r));
        let curved = mask(&fit, |r| replay(&rec.0, Every::Curved, r));
        match every {
            Every::Straight => {
                assert_ne!(straight, curved, "the guard: {g:?} at {px} px");
                assert_eq!(
                    drawn, straight,
                    "{g:?} at {px} px draws every flex straight"
                );
            }
            Every::Curved => assert_eq!(drawn, curved, "{g:?} at {px} px draws every flex curved"),
        }
    }
    (low, high)
}

fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Every glyph of `face` whose outline hands its sink a flex.
fn flexed(face: &Face<'_>) -> Vec<GlyphId> {
    (0..face.glyph_count().value())
        .map(GlyphId::from_shaper)
        .filter(|&g| {
            let mut rec = Record::default();
            face.outline(g, &mut rec);
            !flexes(&rec.0).is_empty()
        })
        .collect()
}

/// CFF: STIX Two Math's `hflex` and `hflex1`, in charstrings and in
/// subroutines, each at a size below half a pixel and one above.
#[test]
fn a_cff_flex_shallower_than_half_a_pixel_draws_straight() {
    let data = read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf"
    ));
    let face = Face::parse(&data).unwrap();
    let gs = flexed(&face);
    let mut sizes = Vec::new();
    for &g in &gs {
        sizes.push(straddles(&face, g));
    }
    eprintln!("CFF: {} glyphs with a flex, sizes {sizes:?}", gs.len());
    // fontTools counts 22 flex operators in 20 charstrings and 8 more in
    // subroutines; a glyph that calls one of those has a flex too. Their
    // depths are 1 to 7 units of 1,000 (81 of 83 are 1 or 2), so every one
    // is straight at 71 px and below, every terminal size.
    assert!(gs.len() >= 20, "{}", gs.len());
}

/// CFF2: the cases font's `hflex`, `hflex1` and both `flex1` branches, and
/// a `flex` whose start and end differ in both `x` and `y`, which carries
/// its depth to the end point's own line: at 11 px it too is under half a
/// pixel, and straight.
#[test]
fn a_cff2_flex_shallower_than_half_a_pixel_draws_straight() {
    let data = read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/cff2-cases.otf"
    ));
    let face = Face::parse(&data).unwrap();
    let gs = flexed(&face);
    assert_eq!(gs.len(), 1, "the cases font's one flex glyph");
    let g = gs[0];
    let mut rec = Record::default();
    face.outline(g, &mut rec);
    let depths: Vec<[f32; 2]> = flexes(&rec.0).iter().map(|f| f.depth).collect();
    // The `flex` first, from (100, 300) through (250.5, 320.25) to
    // (401.25, 300.25): the end's own line passes 20 under the joining
    // point, and the start-end line about 20.125. Then flex1 horizontal
    // (the joining point 40 up), flex1 vertical (20 across), hflex (30
    // up), hflex1 (30 up).
    assert_eq!(
        depths,
        [
            [0.0, 20.0],
            [0.0, 40.0],
            [20.0, 0.0],
            [0.0, 30.0],
            [0.0, 30.0]
        ],
        "{rec:?}",
        rec = rec.0
    );
    let sizes = straddles(&face, g);
    eprintln!("CFF2: sizes {sizes:?}");
}

/// **The slanted `flex` is measured to its end's line, and the rasteriser
/// sees it**: at 24.9 px the cases font's `flex` is 0.498 px deep to the
/// end point's own line and straight, where the start-end line's 20.125
/// units would put it at 0.501 px and curved. The other four flexes of the
/// glyph sit on their own side of half a pixel at that size: flex1
/// vertical, 20 units, straight; the 30- and 40-unit ones curved. The
/// guard: the two drawings of the first flex differ in the mask.
#[test]
fn a_slanted_flex_is_measured_to_its_end_points_line() {
    use Every::{Curved, Straight};
    let data = read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/cff2-cases.otf"
    ));
    let face = Face::parse(&data).unwrap();
    let g = flexed(&face)[0];
    let mut rec = Record::default();
    face.outline(g, &mut rec);
    let scale = Scale::of(24.9, face.units_per_em());
    let mut b = Bounds::new();
    face.outline(g, &mut b);
    let fit = Fit::of(&b, scale).unwrap();
    let drawn = mask(&fit, |r| {
        face.outline(g, r);
    });
    let to_end = mask(&fit, |r| {
        replay_each(&rec.0, &[Straight, Curved, Straight, Curved, Curved], r)
    });
    let to_line = mask(&fit, |r| {
        replay_each(&rec.0, &[Curved, Curved, Straight, Curved, Curved], r)
    });
    assert_ne!(to_end, to_line, "the guard");
    assert_eq!(drawn, to_end);
}
