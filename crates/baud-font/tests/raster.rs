//! The rasteriser against two oracles: `ab_glyph_rasterizer`, which is the
//! same algorithm and must agree **byte for byte**, and FreeType's unhinted
//! smooth rasteriser, which is a different exact-area implementation with
//! its own curve flattening and must agree within a tolerance.
//!
//! **Why exact against one and tolerant against the other.** baud's
//! `Canvas` is the font-rs accumulator as `ab_glyph_rasterizer` carries it
//! (`[[source: ab-glyph-raster]]`): the same line splat, the same quad
//! count, the same cubic subdivision, the same prefix sum. Two spellings of
//! one algorithm that disagree on a byte have a bug between them, so the
//! comparison is equality over every pixel of every glyph at four sizes.
//! FreeType (`[[source: freetype-ftgrays]]`) computes the same exact area
//! by a cell sweep, but it scales the outline to 26.6 fixed point first and
//! splits conics on its own tolerance, so its edges differ by a few counts
//! of 255 -- the comparison is a bounded per-pixel difference and a bounded
//! mean, with the distribution printed so the bound is a number rather than
//! a feeling. Measured on this Mac's corpus: 164,184 masks, 68% of pixels
//! identical, 92% within two counts, a mean difference of 1.20/255, and no
//! pixel further apart than 54; quantised by truncation, as font-rs does,
//! 63% were identical and the mean was 1.27.
//!
//! **One normalisation, counted: a flex.** The text says a flex shallower
//! than its threshold "must be rendered as a straight line"
//! (`[[source: adobe-5177-type2]]` § 4.1), and baud's rasteriser does;
//! `ab_glyph_rasterizer` has no flex. So the oracle's sink draws each flex
//! as `Flex::rendered` says at the same transform -- one line or the two
//! curves -- and the comparison stays the raster half alone; the lines are
//! counted, and STIX Two Math's flexes, 1 to 7 units deep, make some.
//!
//! **Corpus.** The three committed faces with outlines (Fira Code,
//! JetBrains Mono, Bungee Tint) always; on macOS, the monospace system faces
//! and two CFF faces, so the cubic path is driven by real charstrings rather
//! than only by the quad-to-cubic cases a TrueType face never produces.
//!
//! **The FreeType half is gated on `BAUD_FREETYPE_ORACLE`**: the path of
//! `tools/baud-oracle`'s `baud-oracle` bin, which links the pinned FreeType
//! 2.14.3 and answers `baud-oracle raster`. Build it with
//! `nix-shell tools/shell.nix --run 'cargo build --release --manifest-path
//! tools/Cargo.toml --bin baud-oracle'`. Unset, the test says so and
//! passes on the ab_glyph half alone; set and broken, it fails rather than
//! skipping.

use baud_domain::GlyphId;
use baud_font::bytes::Count;
use baud_font::{
    Bounds, Canvas, Extent, Face, FaceIndex, Fit, Flex, FlexDrawn, Point, Rasterizer, Scale, Sink,
    faces,
};
use std::path::{Path, PathBuf};

const SIZES: [f32; 4] = [12.0, 14.0, 16.0, 24.0];

fn committed() -> Vec<PathBuf> {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"));
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("tests/fonts exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ttf"))
        .collect();
    v.sort();
    v
}

fn corpus() -> Vec<PathBuf> {
    let mut v = committed();
    // The symbol face baud embeds: a 4,605-glyph CFF face, held here before
    // it draws anything in a terminal.
    v.push(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf"
    )));
    if cfg!(target_os = "macos") {
        for f in [
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/Monaco.ttf",
            "/System/Library/Fonts/SFNSMono.ttf",
            "/System/Library/Fonts/Courier.ttc",
            "/System/Library/Fonts/Supplemental/STIXGeneral.otf",
            "/System/Library/Fonts/Supplemental/NotoSansJavanese-Regular.otf",
        ] {
            let p = PathBuf::from(f);
            if p.exists() {
                v.push(p);
            }
        }
    }
    v
}

/// The oracle driven by baud's own outline reader through the same
/// transform, so the comparison is the raster half alone.
struct Ab {
    r: ab_glyph_rasterizer::Rasterizer,
    t: baud_font::Transform,
    start: (f32, f32),
    cur: (f32, f32),
    open: bool,
    /// Flexes drawn as one line, the normalisation's count.
    straight: Count,
}

impl Ab {
    fn pt(&self, x: f32, y: f32) -> ab_glyph_rasterizer::Point {
        let (px, py) = self.t.apply(x, y);
        ab_glyph_rasterizer::point(px, py)
    }
    fn close_open(&mut self) {
        if self.open && self.cur != self.start {
            let a = self.pt(self.cur.0, self.cur.1);
            let b = self.pt(self.start.0, self.start.1);
            self.r.draw_line(a, b);
        }
        self.open = false;
    }
}

impl Sink for Ab {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close_open();
        self.start = (x, y);
        self.cur = (x, y);
        self.open = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let a = self.pt(self.cur.0, self.cur.1);
        self.r.draw_line(a, self.pt(x, y));
        self.cur = (x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let a = self.pt(self.cur.0, self.cur.1);
        self.r.draw_quad(a, self.pt(x1, y1), self.pt(x, y));
        self.cur = (x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.pt(self.cur.0, self.cur.1);
        self.r
            .draw_cubic(a, self.pt(x1, y1), self.pt(x2, y2), self.pt(x, y));
        self.cur = (x, y);
    }
    fn close(&mut self) {
        self.close_open();
    }
    fn flex(&mut self, f: &Flex) {
        let [x1, y1, x2, y2, x, y] = f.first;
        let [x3, y3, x4, y4, x5, y5] = f.second;
        match f.rendered(&self.t) {
            FlexDrawn::Straight => {
                self.straight = self.straight.plus(Count::of(1u32)).unwrap();
                self.line_to(x5, y5);
            }
            FlexDrawn::Curved => {
                self.curve_to(x1, y1, x2, y2, x, y);
                self.curve_to(x3, y3, x4, y4, x5, y5);
            }
        }
    }
}

/// Counts what each face's outlines are made of, so the tally can say the
/// cubic path was reached.
#[derive(Default)]
struct Kinds {
    quads: usize,
    cubics: usize,
}

impl Sink for Kinds {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {}
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.quads += 1;
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.cubics += 1;
    }
    fn close(&mut self) {}
}

#[derive(Default, Debug)]
struct Tally {
    faces: usize,
    glyphs: usize,
    masks: usize,
    pixels: usize,
    covered: usize,
    quads: usize,
    cubics: usize,
    largest: (u32, u32),
    /// Flexes the oracle's sink drew as one line; `None` before the first
    /// face that has any.
    straight_flexes: Option<Count>,
}

/// baud's mask for `g` at `scale`, with the fit that placed it.
fn baud_mask(face: &Face<'_>, g: GlyphId, scale: Scale) -> Option<(Fit, Vec<u8>)> {
    let mut b = Bounds::new();
    face.outline(g, &mut b);
    let fit = Fit::of(&b, scale)?;
    let mut scratch = vec![0f32; fit.extent.scratch()?];
    let canvas = Canvas::new(fit.extent, &mut scratch)?;
    let mut r = Rasterizer::new(canvas, fit.transform);
    face.outline(g, &mut r);
    let canvas = r.finish();
    let mut out = vec![0u8; fit.extent.cells()?];
    canvas.coverage(&mut out)?;
    Some((fit, out))
}

fn compare_face(path: &Path, data: &[u8], i: u32, t: &mut Tally) {
    let Ok(face) = Face::parse_index(data, FaceIndex::of(i)) else {
        return;
    };
    t.faces += 1;
    let what = format!("{}#{i}", path.display());
    let glyphs = u32::from(face.glyph_count().value());
    for g in 0..glyphs {
        let gid = GlyphId::from_shaper(u16::try_from(g).unwrap());
        let mut kinds = Kinds::default();
        face.outline(gid, &mut kinds);
        t.quads += kinds.quads;
        t.cubics += kinds.cubics;
        for px in SIZES {
            t.glyphs += 1;
            let scale = Scale::of(px, face.units_per_em());
            let Some((fit, mine)) = baud_mask(&face, gid, scale) else {
                continue;
            };
            let (w, h) = (fit.extent.w.value(), fit.extent.h.value());
            if w * h > t.largest.0 * t.largest.1 {
                t.largest = (w, h);
            }
            let mut ab = Ab {
                r: ab_glyph_rasterizer::Rasterizer::new(
                    usize::try_from(w).unwrap(),
                    usize::try_from(h).unwrap(),
                ),
                t: fit.transform,
                start: (0.0, 0.0),
                cur: (0.0, 0.0),
                open: false,
                straight: Count::ZERO,
            };
            face.outline(gid, &mut ab);
            ab.close_open();
            if !ab.straight.is_zero() {
                let before = t.straight_flexes.unwrap_or(Count::ZERO);
                t.straight_flexes = before.plus(ab.straight);
            }
            let mut theirs = vec![0u8; mine.len()];
            ab.r.for_each_pixel(|idx, alpha| {
                // The reference leaves quantisation to its caller; this is
                // `Canvas::coverage`'s, to the nearest level, where
                // font-rs truncates and drew a whole pixel at 254.
                theirs[idx] = u8::try_from((255.0 * alpha.min(1.0) + 0.5) as i32).unwrap();
            });
            if mine != theirs {
                let first = mine.iter().zip(&theirs).position(|(a, b)| a != b).unwrap();
                panic!(
                    "{what} g{g} @{px}px {w}x{h}: first difference at {} ({}, {}): baud {} oracle {}",
                    first,
                    first % usize::try_from(w).unwrap(),
                    first / usize::try_from(w).unwrap(),
                    mine[first],
                    theirs[first]
                );
            }
            t.masks += 1;
            t.pixels += mine.len();
            t.covered += mine.iter().filter(|&&c| c != 0).count();
        }
    }
}

#[test]
fn every_mask_is_byte_identical_to_ab_glyph() {
    let mut t = Tally::default();
    for path in corpus() {
        let data = std::fs::read(&path).unwrap();
        let n = faces(&data).map(|n| n.value()).unwrap_or(1);
        for i in 0..n {
            compare_face(&path, &data, i, &mut t);
        }
    }
    eprintln!("{t:#?}");
    // Positive controls: the three committed faces alone are ~5,000 glyphs
    // at four sizes, and Bungee Tint's layered glyphs are quads while a
    // CFF face is cubics.
    assert!(t.faces >= 3, "{}", t.faces);
    assert!(t.masks >= 15_000, "{}", t.masks);
    assert!(t.covered >= 500_000, "{}", t.covered);
    assert!(t.quads >= 10_000, "{}", t.quads);
    if cfg!(target_os = "macos") {
        assert!(t.cubics >= 1_000, "{}", t.cubics);
    }
    // STIX Two Math is committed, and its flexes are straight at every size
    // here, so the normalisation is reached.
    assert!(t.straight_flexes.is_some(), "{:?}", t.straight_flexes);
}

#[test]
fn a_fit_puts_the_outline_inside_the_canvas_with_a_margin() {
    // The accumulator's contract is that the outline lies within the
    // canvas: the line splat may deposit one cell past an edge it touches
    // and the scratch has four cells of slack, so `Fit` leaves a pixel of
    // margin on every side. Checked over the committed faces at 14 px by
    // transforming every control point and testing it against the extent.
    struct Inside {
        t: baud_font::Transform,
        w: f32,
        h: f32,
        outside: usize,
        points: usize,
    }
    impl Inside {
        fn see(&mut self, x: f32, y: f32) {
            let (px, py) = self.t.apply(x, y);
            self.points += 1;
            if !(px >= 1.0 && px <= self.w - 1.0 && py >= 1.0 && py <= self.h - 1.0) {
                self.outside += 1;
            }
        }
    }
    impl Sink for Inside {
        fn move_to(&mut self, x: f32, y: f32) {
            self.see(x, y);
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.see(x, y);
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            self.see(x1, y1);
            self.see(x, y);
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            self.see(x1, y1);
            self.see(x2, y2);
            self.see(x, y);
        }
        fn close(&mut self) {}
    }
    let mut points = 0;
    for path in committed() {
        let data = std::fs::read(&path).unwrap();
        let face = Face::parse(&data).unwrap();
        let scale = Scale::of(14.0, face.units_per_em());
        for g in 0..u32::from(face.glyph_count().value()) {
            let gid = GlyphId::from_shaper(u16::try_from(g).unwrap());
            let mut b = Bounds::new();
            face.outline(gid, &mut b);
            let Some(fit) = Fit::of(&b, scale) else {
                continue;
            };
            let mut inside = Inside {
                t: fit.transform,
                w: fit.extent.w.value() as f32,
                h: fit.extent.h.value() as f32,
                outside: 0,
                points: 0,
            };
            face.outline(gid, &mut inside);
            assert_eq!(inside.outside, 0, "{} g{g}", path.display());
            points += inside.points;
        }
    }
    assert!(points > 100_000, "{points}");
}

#[test]
fn a_canvas_refuses_scratch_that_is_too_small() {
    let e = Extent::new(baud_font::Px::of(4), baud_font::Px::of(4));
    let mut short = vec![0f32; 16];
    assert!(Canvas::new(e, &mut short).is_none());
    let mut enough = vec![0f32; 20];
    let mut c = Canvas::new(e, &mut enough).unwrap();
    // A unit square covers its four cells fully and nothing else.
    c.line(Point::new(1.0, 1.0), Point::new(1.0, 3.0));
    c.line(Point::new(1.0, 3.0), Point::new(3.0, 3.0));
    c.line(Point::new(3.0, 3.0), Point::new(3.0, 1.0));
    c.line(Point::new(3.0, 1.0), Point::new(1.0, 1.0));
    let mut out = vec![0u8; 16];
    c.coverage(&mut out).unwrap();
    assert_eq!(
        out,
        [0, 0, 0, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 0, 0, 0],
        "{out:?}"
    );
    let mut short_out = vec![0u8; 15];
    assert!(c.coverage(&mut short_out).is_none());
}

/// The mask of a closed polygon `pts` on a `w` by `h` canvas.
fn polygon(w: u32, h: u32, pts: &[Point]) -> Vec<u8> {
    let e = Extent::new(baud_font::Px::of(w), baud_font::Px::of(h));
    let mut scratch = vec![0f32; e.scratch().unwrap()];
    let mut c = Canvas::new(e, &mut scratch).unwrap();
    for (i, &p) in pts.iter().enumerate() {
        c.line(p, pts[(i + 1) % pts.len()]);
    }
    let mut out = vec![0u8; e.cells().unwrap()];
    c.coverage(&mut out).unwrap();
    out
}

/// **A pixel the outline covers wholly is 255**, and every pixel is its
/// coverage rounded to the nearest of the 256 levels: a mask is that
/// coverage quantised, and quantising by truncation drew the inside of a
/// shape whose edges fall between pixels at 254, because the prefix sum
/// of the edges' deposits lands a float step under one. Over rectangles
/// at every quarter-pixel offset and a 48-sided disc, each pixel wholly
/// inside reads 255, and each edge pixel is its exact area to within half
/// a level. Perturbation: quantise by truncation again, `(255.0 * y) as
/// i32`, and the wholly covered pixels read 254.
#[test]
fn a_wholly_covered_pixel_is_255_and_each_pixel_its_rounded_area() {
    let (mut inside, mut wrong, mut first) = (0usize, [0usize; 3], None);
    for qx in 0..4u8 {
        for qy in 0..4u8 {
            for (dw, dh) in [(5.3f32, 3.7f32), (7.9, 6.1), (11.45, 9.85)] {
                let (x0, y0) = (1.0 + f32::from(qx) / 4.0, 1.0 + f32::from(qy) / 4.0);
                let (x1, y1) = (x0 + dw, y0 + dh);
                let out = polygon(
                    16,
                    16,
                    &[
                        Point::new(x0, y0),
                        Point::new(x0, y1),
                        Point::new(x1, y1),
                        Point::new(x1, y0),
                    ],
                );
                for (i, &c) in out.iter().enumerate() {
                    let (px, py) = ((i % 16) as f32, (i / 16) as f32);
                    let ax = (x1.min(px + 1.0) - x0.max(px)).max(0.0);
                    let ay = (y1.min(py + 1.0) - y0.max(py)).max(0.0);
                    let exact = 255.0 * ax * ay;
                    let wholly = ax * ay == 1.0;
                    inside += usize::from(wholly);
                    if (f32::from(c) - exact).abs() > 0.5 + 1e-3 {
                        wrong[usize::from(wholly)] += 1;
                        first.get_or_insert(format!(
                            "rect at ({x0}, {y0}) {dw}x{dh}, pixel ({px}, {py}): {c} for {exact:.3}"
                        ));
                    }
                }
            }
        }
    }
    assert!(inside > 1000, "{inside}");
    // A disc of radius 5.6 about (8.3, 8.3): every pixel whose four
    // corners lie inside the polygon is wholly covered.
    let (cx, cy, r) = (8.3f32, 8.3f32, 5.6f32);
    let n = 48;
    let pts: Vec<Point> = (0..n)
        .map(|k| {
            let t = core::f32::consts::TAU * k as f32 / n as f32;
            Point::new(cx + r * t.cos(), cy + r * t.sin())
        })
        .collect();
    let out = polygon(17, 17, &pts);
    // The polygon's inscribed radius bounds the corners that are inside.
    let inner = r * (core::f32::consts::PI / n as f32).cos();
    let mut wholly = 0usize;
    for (i, &c) in out.iter().enumerate() {
        let (px, py) = ((i % 17) as f32, (i / 17) as f32);
        let corners = [
            (px, py),
            (px + 1.0, py),
            (px, py + 1.0),
            (px + 1.0, py + 1.0),
        ];
        if corners.iter().all(|&(x, y)| (x - cx).hypot(y - cy) < inner) {
            wholly += 1;
            if c != 255 {
                wrong[2] += 1;
                first.get_or_insert(format!("disc pixel ({px}, {py}): {c}"));
            }
        }
    }
    assert!(wholly > 50, "{wholly}");
    assert_eq!(
        wrong,
        [0, 0, 0],
        "rectangle edge, rectangle inside and disc inside pixels off their \
         rounded area; first {first:?}"
    );
}

// ---------------------------------------------------------------------------
// FreeType, within a tolerance.

/// One glyph as `baud-oracle raster` prints it.
struct FtGlyph {
    gid: u32,
    px: f32,
    left: i32,
    top: i32,
    w: u32,
    h: u32,
    bytes: Vec<u8>,
}

fn freetype_oracle(oracle: &str, path: &Path, index: u32) -> Vec<FtGlyph> {
    let sizes: Vec<String> = SIZES.iter().map(|s| format!("{s}")).collect();
    let out = std::process::Command::new(oracle)
        .arg("raster")
        .arg(path)
        .arg(index.to_string())
        .args(&sizes)
        .output()
        .unwrap_or_else(|e| panic!("BAUD_FREETYPE_ORACLE={oracle} does not run: {e}"));
    assert!(
        out.status.success(),
        "baud-oracle raster failed on {}: {}",
        path.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let mut lines = text.lines();
    let mut v = Vec::new();
    while let Some(head) = lines.next() {
        let f: Vec<&str> = head.split_whitespace().collect();
        assert_eq!(f[0], "g", "{head}");
        let (w, h) = (f[5].parse::<u32>().unwrap(), f[6].parse::<u32>().unwrap());
        let hex = lines.next().unwrap_or("");
        let bytes: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        assert_eq!(bytes.len(), usize::try_from(w * h).unwrap(), "{head}");
        v.push(FtGlyph {
            gid: f[1].parse().unwrap(),
            px: f[2].parse().unwrap(),
            left: f[3].parse().unwrap(),
            top: f[4].parse().unwrap(),
            w,
            h,
            bytes,
        });
    }
    v
}

#[test]
fn every_mask_is_within_tolerance_of_freetype() {
    let Ok(oracle) = std::env::var("BAUD_FREETYPE_ORACLE") else {
        eprintln!(
            "BAUD_FREETYPE_ORACLE unset: the FreeType half of the raster differential did not run"
        );
        return;
    };
    let mut glyphs = 0usize;
    let mut pixels = 0usize;
    let mut hist = [0usize; 9]; // |diff| in 0, 1-2, 3-4, 5-8, 9-16, 17-32, 33-64, 65-128, 129-255
    let mut worst = (0u8, String::new());
    let mut margin_max = 0u8;
    let mut worst_mean = (0f64, String::new());
    let mut mean_sum = 0f64;
    for path in corpus() {
        let data = std::fs::read(&path).unwrap();
        let n = faces(&data).map(|n| n.value()).unwrap_or(1);
        for i in 0..n {
            let Ok(face) = Face::parse_index(&data, FaceIndex::of(i)) else {
                continue;
            };
            for ft in freetype_oracle(&oracle, &path, i) {
                if ft.w == 0 || ft.h == 0 {
                    continue;
                }
                // Rasterise into FreeType's frame -- its bitmap origin is
                // (left, top) in pixels -- with a pixel of margin all round,
                // then compare the interior. The margin is not cosmetic:
                // FreeType snaps the outline to 26.6 fixed point before it
                // takes the box, so an edge it puts at exactly 3.0 sits at
                // 2.995 in baud's floats, a hair outside FreeType's own
                // frame, and the accumulator's contract is that the outline
                // is inside the canvas. Whatever lands in the margin is
                // checked to be the sliver it should be.
                let scale = Scale::of(ft.px, face.units_per_em());
                let (wm, hm) = (ft.w + 2, ft.h + 2);
                let extent = Extent::new(baud_font::Px::of(wm), baud_font::Px::of(hm));
                let transform = scale.placing(1.0 - ft.left as f32, 1.0 + ft.top as f32);
                let mut scratch = vec![0f32; extent.scratch().unwrap()];
                let canvas = Canvas::new(extent, &mut scratch).unwrap();
                let mut r = Rasterizer::new(canvas, transform);
                face.outline(GlyphId::from_shaper(u16::try_from(ft.gid).unwrap()), &mut r);
                let canvas = r.finish();
                let mut with_margin = vec![0u8; extent.cells().unwrap()];
                canvas.coverage(&mut with_margin).unwrap();
                let (wmu, wu) = (usize::try_from(wm).unwrap(), usize::try_from(ft.w).unwrap());
                let mut mine = Vec::with_capacity(usize::try_from(ft.w * ft.h).unwrap());
                for (y, row) in with_margin.chunks(wmu).enumerate() {
                    let edge_row = y == 0 || y + 1 == usize::try_from(hm).unwrap();
                    for (x, &c) in row.iter().enumerate() {
                        if edge_row || x == 0 || x + 1 == wmu {
                            margin_max = margin_max.max(c);
                        } else {
                            mine.push(c);
                        }
                    }
                }
                assert_eq!(mine.len(), wu * usize::try_from(ft.h).unwrap());
                let mut sum = 0u64;
                for (k, (&a, &b)) in mine.iter().zip(&ft.bytes).enumerate() {
                    let d = a.abs_diff(b);
                    sum += u64::from(d);
                    let bucket = match d {
                        0 => 0,
                        1..=2 => 1,
                        3..=4 => 2,
                        5..=8 => 3,
                        9..=16 => 4,
                        17..=32 => 5,
                        33..=64 => 6,
                        65..=128 => 7,
                        _ => 8,
                    };
                    hist[bucket] += 1;
                    if d > worst.0 {
                        worst = (
                            d,
                            format!(
                                "{}#{i} g{} @{}px at {k} ({}x{}): baud {a} freetype {b}",
                                path.display(),
                                ft.gid,
                                ft.px,
                                ft.w,
                                ft.h
                            ),
                        );
                    }
                }
                let mean = sum as f64 / mine.len() as f64;
                mean_sum += mean;
                if mean > worst_mean.0 {
                    worst_mean = (
                        mean,
                        format!(
                            "{}#{i} g{} @{}px over {}x{}",
                            path.display(),
                            ft.gid,
                            ft.px,
                            ft.w,
                            ft.h
                        ),
                    );
                }
                glyphs += 1;
                pixels += mine.len();
            }
        }
    }
    eprintln!(
        "freetype: {glyphs} glyphs, {pixels} pixels, |diff| histogram (0, 1-2, 3-4, 5-8, 9-16, 17-32, 33-64, 65-128, 129-255) = {hist:?}, mean of means {:.3}/255, worst pixel {} ({})",
        mean_sum / glyphs as f64,
        worst.0,
        worst.1
    );
    eprintln!(
        "freetype: worst per-glyph mean {:.2}/255 ({}), most coverage in the margin {margin_max}",
        worst_mean.0, worst_mean.1
    );
    assert!(glyphs >= 5_000, "{glyphs}");
    // The bounds are the first run's figures with room for another face,
    // not a feeling: 115,456 glyphs, mean of means 1.34/255, worst pixel
    // 54/255 on a curve edge of Menlo at 24 px, worst per-glyph mean
    // 17.1/255 on a 3x3 glyph, and 3/255 the most that ever lands in the
    // margin. Edges differ by flattening and 26.6 quantisation; a pixel
    // disagreeing by more than a quarter, or a glyph disagreeing on
    // average by a tenth, is a placement or winding error rather than a
    // curve tolerance.
    let of_means = mean_sum / glyphs as f64;
    assert!(of_means <= 2.0, "mean of means {of_means}");
    assert_eq!(
        hist[7] + hist[8],
        0,
        "pixels differing by more than 64/255: {hist:?}"
    );
    assert!(worst_mean.0 <= 24.0, "{} ({})", worst_mean.0, worst_mean.1);
    assert!(margin_max <= 4, "{margin_max}");
}
