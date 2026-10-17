//! Box drawing, block elements and braille are drawn by baud at exactly one
//! cell, and the tests here hold the three claims that makes: every
//! codepoint in the three ranges is procedural and masks to the cell; the
//! descriptor table agrees with the pinned Unicode names, read off the mask
//! itself -- an arm the name lists reaches its edge at the weight the name
//! gives, and an arm it does not list leaves that edge blank; and a frame
//! drawn across cells has no seam, on the CPU compositor, at every cell
//! size tried.
//!
//! The names are the oracle the way the rules were for the cluster table:
//! the table in `boxes.rs` and the grammar here are two spellings of one
//! fact, written apart, and this file is where they meet.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::boxes::{self, Cell, Shape, Weight};
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::font;

/// The pinned `UnicodeData.txt`, `[sources.unicode-data]`.
fn unicode_data() -> String {
    std::fs::read_to_string(format!(
        "{}/../docs/research/sources/unicode-unicodedata.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the pinned UnicodeData.txt")
}

/// The names of `lo..=hi`, from the pinned file.
fn names(lo: u32, hi: u32) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for line in unicode_data().lines() {
        let mut f = line.split(';');
        let (Some(code), Some(name)) = (f.next(), f.next()) else {
            continue;
        };
        let Ok(cp) = u32::from_str_radix(code, 16) else {
            continue;
        };
        if (lo..=hi).contains(&cp) {
            out.push((cp, name.to_string()));
        }
    }
    out
}

fn cp(v: u32) -> Codepoint {
    Codepoint::new(v).expect("a codepoint")
}

/// A mask of `cell` for `v`, row-major.
fn mask(v: u32, cell: Cell) -> Vec<u8> {
    let mut scratch = Vec::new();
    let mut coverage = Vec::new();
    let extent = boxes::draw(cp(v), cell, &mut scratch, &mut coverage).expect("procedural");
    assert_eq!(
        (extent.w, extent.h),
        (cell.w, cell.h),
        "U+{v:04X} is not one cell"
    );
    let n = usize::try_from(cell.w.value() * cell.h.value()).unwrap();
    coverage.truncate(n);
    coverage
}

const CELLS: [(u32, u32); 5] = [(8, 16), (9, 18), (17, 34), (11, 23), (16, 33)];

fn cell(w: u32, h: u32) -> Cell {
    Cell {
        w: Px::of(w),
        h: Px::of(h),
    }
}

#[test]
fn every_codepoint_in_the_three_ranges_is_procedural_and_masks_to_one_cell() {
    let mut count = 0;
    for v in (0x2500..=0x259F).chain(0x2800..=0x28FF) {
        assert!(boxes::covers(cp(v)), "U+{v:04X} should be procedural");
        for &(w, h) in &CELLS {
            let m = mask(v, cell(w, h));
            // Every glyph but U+2800 (blank braille) draws something.
            if v != 0x2800 {
                assert!(
                    m.iter().any(|&c| c > 0),
                    "U+{v:04X} at {w}x{h} drew nothing"
                );
            }
        }
        count += 1;
    }
    assert_eq!(count, 0xA0 + 0x100);
    // And the neighbours on either side are a font's.
    for v in [0x24FF, 0x25A0, 0x27FF, 0x2900, 0x41] {
        assert!(!boxes::covers(cp(v)), "U+{v:04X} should not be procedural");
    }
}

/// The four directions an arm can take, in the order the mask is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// What the name says about each arm: its weight, or none.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Named {
    up: Option<Weight>,
    down: Option<Weight>,
    left: Option<Weight>,
    right: Option<Weight>,
}

impl Named {
    fn get(self, d: Dir) -> Option<Weight> {
        match d {
            Dir::Up => self.up,
            Dir::Down => self.down,
            Dir::Left => self.left,
            Dir::Right => self.right,
        }
    }

    fn set(&mut self, d: Dir, w: Weight) {
        match d {
            Dir::Up => self.up = Some(w),
            Dir::Down => self.down = Some(w),
            Dir::Left => self.left = Some(w),
            Dir::Right => self.right = Some(w),
        }
    }
}

/// The form a box-drawing name describes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    /// Straight arms; `double` when any is a double line.
    Arms(Named),
    /// `n` dashes, horizontal or not, light or heavy.
    Dashed(bool, Weight, u8),
    /// A rounded corner with these two arms.
    Arc(Named),
    /// Diagonals: falling (upper right to lower left), rising.
    Diagonal(bool, bool),
}

/// The grammar of `BOX DRAWINGS ...`: groups joined by `AND`, each a
/// weight word and direction words, the weight carried forward when a
/// group names none -- `LIGHT DOWN AND RIGHT` is light both ways,
/// `DOWN LIGHT AND RIGHT HEAVY` is one each, `VERTICAL SINGLE AND LEFT
/// DOUBLE` likewise with `SINGLE` the light stroke of the double family.
fn parse_box(name: &str) -> Form {
    let rest = name.strip_prefix("BOX DRAWINGS ").expect("a box name");
    let words: Vec<&str> = rest.split(' ').collect();
    if words.contains(&"DASH") {
        let horizontal = words.contains(&"HORIZONTAL");
        let weight = if words[0] == "HEAVY" {
            Weight::Heavy
        } else {
            Weight::Light
        };
        let n = match words[1] {
            "DOUBLE" => 2,
            "TRIPLE" => 3,
            "QUADRUPLE" => 4,
            w => panic!("dash count {w}"),
        };
        return Form::Dashed(horizontal, weight, n);
    }
    if words.contains(&"DIAGONAL") {
        if words.contains(&"CROSS") {
            return Form::Diagonal(true, true);
        }
        let falling = rest.contains("UPPER RIGHT TO LOWER LEFT");
        return Form::Diagonal(falling, !falling);
    }
    let arc = words.contains(&"ARC");
    let mut named = Named::default();
    let mut carried: Option<Weight> = None;
    for group in rest.split(" AND ") {
        let mut weight = None;
        let mut dirs = Vec::new();
        for w in group.split(' ') {
            match w {
                "LIGHT" | "SINGLE" => weight = Some(Weight::Light),
                "HEAVY" => weight = Some(Weight::Heavy),
                "DOUBLE" => weight = Some(Weight::Double),
                "UP" => dirs.push(Dir::Up),
                "DOWN" => dirs.push(Dir::Down),
                "LEFT" => dirs.push(Dir::Left),
                "RIGHT" => dirs.push(Dir::Right),
                "VERTICAL" => dirs.extend([Dir::Up, Dir::Down]),
                "HORIZONTAL" => dirs.extend([Dir::Left, Dir::Right]),
                "ARC" => {}
                w => panic!("{name}: unknown word {w}"),
            }
        }
        let weight = weight.or(carried).expect("a weight in the first group");
        carried = Some(weight);
        for d in dirs {
            named.set(d, weight);
        }
    }
    if arc {
        Form::Arc(named)
    } else {
        Form::Arms(named)
    }
}

/// The ink along one edge of the mask: how many pixels are covered and in
/// how many separate runs -- a light stroke is `t` pixels in one run, a
/// heavy `2t` in one, a double `2t` in two.
fn edge(m: &[u8], c: Cell, d: Dir) -> (u32, u32) {
    let (w, h) = (
        usize::try_from(c.w.value()).unwrap(),
        usize::try_from(c.h.value()).unwrap(),
    );
    let line: Vec<u8> = match d {
        Dir::Up => m[..w].to_vec(),
        Dir::Down => m[(h - 1) * w..].to_vec(),
        Dir::Left => (0..h).map(|y| m[y * w]).collect(),
        Dir::Right => (0..h).map(|y| m[y * w + w - 1]).collect(),
    };
    let mut covered = 0;
    let mut runs = 0;
    let mut inside = false;
    for &px in &line {
        // A stroke edge is full coverage; anti-aliased spill at a corner is
        // not an arm.
        let on = px >= 0x80;
        covered += u32::from(on);
        if on && !inside {
            runs += 1;
        }
        inside = on;
    }
    (covered, runs)
}

/// **The floor's lines are the Box Drawing block, and only it**
/// (`baud_render::Line`): every character in `Line::FIRST..=Line::LAST`
/// is named `BOX DRAWINGS` in the pinned file, and the characters either
/// side of it -- `U+24FF`, and `U+2580` UPPER HALF BLOCK, the first of the
/// Block Elements btop draws graphs with -- are not.
#[test]
fn the_floors_lines_are_the_box_drawing_block() {
    use baud_render::Line;
    let inside = names(Line::FIRST, Line::LAST);
    assert_eq!(inside.len(), 128);
    assert!(
        inside.iter().all(|(_, n)| n.starts_with("BOX DRAWINGS ")),
        "every name in the range is a box drawing"
    );
    for v in [Line::FIRST - 1, Line::LAST + 1] {
        let (_, name) = names(v, v).remove(0);
        assert!(!name.starts_with("BOX DRAWINGS"), "U+{v:04X} {name}");
    }
    assert_eq!(names(0x2580, 0x2580)[0].1, "UPPER HALF BLOCK");
}

#[test]
fn the_box_descriptors_agree_with_the_unicode_names() {
    let table = names(0x2500, 0x257F);
    assert_eq!(table.len(), 128, "the pinned file carries the whole block");
    let c = cell(17, 34);
    let t = c.light().value();
    assert_eq!(t, 2);
    let mut arms_checked = 0;
    for (v, name) in table {
        let m = mask(v, c);
        match parse_box(&name) {
            Form::Arms(named) | Form::Arc(named) => {
                // What the table says, for the straight and double glyphs.
                if let Some(Shape::Arms(a)) | Some(Shape::Double(a)) = boxes::shape(cp(v)) {
                    assert_eq!(
                        (a.up, a.down, a.left, a.right),
                        (named.up, named.down, named.left, named.right),
                        "U+{v:04X} {name}: the table disagrees with the name"
                    );
                    arms_checked += 1;
                }
                // What the mask says: an arm reaches its edge at its
                // weight and a missing arm leaves the edge blank.
                for d in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
                    let (covered, runs) = edge(&m, c, d);
                    let want = match named.get(d) {
                        None => (0, 0),
                        Some(Weight::Light) => (t, 1),
                        Some(Weight::Heavy) => (2 * t, 1),
                        Some(Weight::Double) => (2 * t, 2),
                    };
                    assert_eq!(
                        (covered, runs),
                        want,
                        "U+{v:04X} {name}: edge {d:?} reads {covered} px in {runs} runs"
                    );
                }
            }
            Form::Dashed(horizontal, weight, n) => {
                let Some(Shape::Dashed(axis, wt, dashes)) = boxes::shape(cp(v)) else {
                    panic!("U+{v:04X} {name} is not dashed in the table");
                };
                assert_eq!(axis == boxes::Axis::Horizontal, horizontal, "U+{v:04X}");
                assert_eq!(wt, weight, "U+{v:04X}");
                let count = match dashes {
                    boxes::Dashes::Two => 2,
                    boxes::Dashes::Three => 3,
                    boxes::Dashes::Four => 4,
                };
                assert_eq!(count, n, "U+{v:04X}");
                // The dashes tile: a gap at each edge, so no edge ink along
                // the line's axis, and `n` runs along the centre.
                let (w, h) = (17usize, 34usize);
                let runs = if horizontal {
                    let y = usize::try_from((c.h.value() - t) / 2).unwrap();
                    (0..w).map(|x| m[y * w + x] >= 0x80).collect::<Vec<_>>()
                } else {
                    let x = usize::try_from((c.w.value() - t) / 2).unwrap();
                    (0..h).map(|y| m[y * w + x] >= 0x80).collect::<Vec<_>>()
                };
                let mut seen = 0;
                for i in 0..runs.len() {
                    if runs[i] && (i == 0 || !runs[i - 1]) {
                        seen += 1;
                    }
                }
                assert_eq!(seen, n, "U+{v:04X} {name}: {seen} dashes");
                assert!(
                    !runs[0] && !runs[runs.len() - 1],
                    "U+{v:04X}: a dash touches the edge"
                );
            }
            Form::Diagonal(falling, rising) => {
                let Some(Shape::Diagonal(d)) = boxes::shape(cp(v)) else {
                    panic!("U+{v:04X} {name} is not diagonal in the table");
                };
                assert_eq!((d.falling, d.rising), (falling, rising), "U+{v:04X}");
                // A diagonal reaches its two corners.
                let (w, h) = (17usize, 34usize);
                let at = |x: usize, y: usize| m[y * w + x] > 0;
                if falling {
                    assert!(
                        at(w - 1, 0) && at(0, h - 1),
                        "U+{v:04X}: the falling diagonal misses a corner"
                    );
                }
                if rising {
                    assert!(
                        at(0, 0) && at(w - 1, h - 1),
                        "U+{v:04X}: the rising diagonal misses a corner"
                    );
                }
            }
        }
    }
    assert_eq!(
        arms_checked,
        128 - 12 - 4 - 3,
        "every straight and double glyph was checked"
    );
}

/// The block names: `UPPER HALF BLOCK`, `LOWER THREE EIGHTHS BLOCK`, `LEFT
/// ONE QUARTER BLOCK`, `FULL BLOCK`, the shades and the quadrants.
#[test]
fn the_block_descriptors_agree_with_the_unicode_names() {
    let table = names(0x2580, 0x259F);
    assert_eq!(table.len(), 32);
    let c = cell(16, 32);
    let (w, h) = (16usize, 32usize);
    let fraction = |words: &[&str]| -> u32 {
        match words {
            ["HALF"] => 4,
            ["ONE", "EIGHTH"] => 1,
            ["ONE", "QUARTER"] => 2,
            ["THREE", "EIGHTHS"] => 3,
            ["FIVE", "EIGHTHS"] => 5,
            ["THREE", "QUARTERS"] => 6,
            ["SEVEN", "EIGHTHS"] => 7,
            w => panic!("fraction {w:?}"),
        }
    };
    for (v, name) in table {
        let m = mask(v, c);
        let ink = |x: usize, y: usize| m[y * w + x] >= 0x80;
        let words: Vec<&str> = name.split(' ').collect();
        let covered = m.iter().filter(|&&p| p >= 0x80).count();
        match words.as_slice() {
            ["FULL", "BLOCK"] => assert_eq!(covered, w * h, "U+{v:04X}"),
            [side, rest @ .., "BLOCK"] => {
                let k = usize::try_from(fraction(rest)).unwrap();
                // The named side's `k` eighths are filled and the rest is
                // blank: check the two extreme pixels on each side.
                let (want, expect_pixels) = match *side {
                    "UPPER" => (k * w * h / 8, (ink(0, 0), !ink(0, h - 1))),
                    "LOWER" => (k * w * h / 8, (ink(0, h - 1), !ink(0, 0))),
                    "LEFT" => (k * w * h / 8, (ink(0, 0), !ink(w - 1, 0))),
                    "RIGHT" => (k * w * h / 8, (ink(w - 1, 0), !ink(0, 0))),
                    s => panic!("side {s}"),
                };
                assert_eq!(covered, want, "U+{v:04X} {name}");
                assert_eq!(expect_pixels, (true, true), "U+{v:04X} {name}");
            }
            [_, "SHADE"] => {
                let level = boxes::shade_coverage(match boxes::shape(cp(v)) {
                    Some(Shape::Shade(s)) => s,
                    _ => panic!("U+{v:04X} is not a shade"),
                })
                .0;
                assert!(m.iter().all(|&p| p == level), "U+{v:04X} {name}");
                let want = match words[0] {
                    "LIGHT" => 0x40,
                    "MEDIUM" => 0x80,
                    "DARK" => 0xc0,
                    s => panic!("shade {s}"),
                };
                assert_eq!(level, want, "U+{v:04X}");
            }
            ["QUADRANT", rest @ ..] => {
                let text = rest.join(" ");
                let quads = text.split(" AND ");
                let mut want = 0u8;
                for q in quads {
                    want |= match q {
                        "UPPER LEFT" => boxes::Quadrants::UL,
                        "UPPER RIGHT" => boxes::Quadrants::UR,
                        "LOWER LEFT" => boxes::Quadrants::LL,
                        "LOWER RIGHT" => boxes::Quadrants::LR,
                        q => panic!("quadrant {q}"),
                    };
                }
                let Some(Shape::Quadrants(q)) = boxes::shape(cp(v)) else {
                    panic!("U+{v:04X} is not a quadrant glyph");
                };
                assert_eq!(q.0, want, "U+{v:04X} {name}");
                // And the mask agrees: each quadrant's centre pixel.
                let probe = [
                    (boxes::Quadrants::UL, w / 4, h / 4),
                    (boxes::Quadrants::UR, 3 * w / 4, h / 4),
                    (boxes::Quadrants::LL, w / 4, 3 * h / 4),
                    (boxes::Quadrants::LR, 3 * w / 4, 3 * h / 4),
                ];
                for (bit, x, y) in probe {
                    assert_eq!(ink(x, y), want & bit != 0, "U+{v:04X} {name} at {x},{y}");
                }
            }
            w => panic!("U+{v:04X}: unparsed {w:?}"),
        }
    }
}

/// Braille: the dot count is the popcount of the low byte, and dot 1 is
/// top left, dot 8 bottom right -- the block's own layout.
#[test]
fn braille_dots_come_from_the_low_byte() {
    let c = cell(16, 32);
    let (w, h) = (16usize, 32usize);
    // Dot centres: two columns, four rows.
    let centre = |col: usize, row: usize| (col * w / 2 + w / 4, row * h / 4 + h / 8);
    let at = [
        centre(0, 0),
        centre(0, 1),
        centre(0, 2),
        centre(1, 0),
        centre(1, 1),
        centre(1, 2),
        centre(0, 3),
        centre(1, 3),
    ];
    for bits in 0u32..0x100 {
        let m = mask(0x2800 + bits, c);
        for (bit, &(x, y)) in at.iter().enumerate() {
            let want = bits & (1 << bit) != 0;
            assert_eq!(
                m[y * w + x] > 0,
                want,
                "U+{:04X} dot {}",
                0x2800 + bits,
                bit + 1
            );
        }
    }
    assert!(mask(0x2800, c).iter().all(|&p| p == 0));
    assert!(
        mask(0x28FF, c).iter().any(|&p| p >= 0xE0),
        "a dot is near-opaque at its centre"
    );
}

/// A 3x3 frame through the CPU compositor: the top and bottom rules run
/// unbroken across all three cells and the sides down all three rows, with
/// no pixel at the background anywhere along a stroke's centre line.
#[test]
fn a_frame_drawn_across_cells_has_no_seam() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    for size in [10.0f32, 14.0, 17.0, 23.0] {
        let ws = WinSize {
            ws_row: 3,
            ws_col: 3,
        };
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(size).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(3),
            Rows::of_index(3),
        )
        .unwrap();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        // Hide the cursor: a block cursor on the last cell reverses its
        // strokes, which is not a seam.
        let bytes = "\x1b[?25l┌─┐\r\n│ │\r\n└─┘".as_bytes();
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let mut canvas = support::Canvas::for_renderer(&r);
        support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
        if std::env::var("BOXES_DUMP").is_ok() {
            dump_ppm(&canvas, &format!("/tmp/baud-frame-{size}.ppm"));
        }
        let m = r.metrics();
        let (cw, ch) = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let c = cell(m.cell_w.value(), m.cell_h.value());
        let t_px = usize::try_from(c.light().value()).unwrap();
        let fg = r.theme().fg.0;
        let bg = r.theme().bg.0;
        let px = |x: usize, y: usize| canvas.pixels[y * canvas.width + x];
        // The horizontal stroke's rows in a cell, and the vertical's columns.
        let hy = (ch - t_px) / 2;
        let vx = (cw - t_px) / 2;
        // Top rule: from the left corner's vertical stroke to the right
        // corner's, every pixel of every stroke row is the foreground.
        for row in [hy, 2 * ch + hy] {
            for x in vx..(2 * cw + vx + t_px) {
                let p = px(x, row);
                assert_eq!(
                    p, fg,
                    "size {size}: seam at ({x}, {row}) on a horizontal rule"
                );
            }
        }
        // Sides: from the top rule down to the bottom rule.
        for col in [vx, 2 * cw + vx] {
            for y in hy..(2 * ch + hy + t_px) {
                let p = px(col, y);
                assert_eq!(
                    p, fg,
                    "size {size}: seam at ({col}, {y}) on a vertical rule"
                );
            }
        }
        // And the interior is untouched: the centre cell is background.
        assert_eq!(
            px(cw + cw / 2, ch + ch / 2),
            bg,
            "size {size}: the interior"
        );
        // A positive control: a pixel just outside the top rule is not fg.
        assert_ne!(
            px(cw + cw / 2, hy + t_px + 1),
            fg,
            "size {size}: the control"
        );
        assert!(
            r.atlas_stats().synthesised.value() >= 6,
            "the masks came from the procedural source"
        );
    }
}

/// A sampler of the whole range, for looking at: `BOXES_DUMP=1`.
#[test]
fn the_sampler_renders() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    // Box drawing (0x2500..0x25A0, 5 rows) then braille (0x2800..0x2900, 8
    // rows): 13 rows on a 14-row screen so nothing scrolls off.
    let (cols, rows) = (32usize, 13usize);
    let ws = WinSize {
        ws_row: 14,
        ws_col: 32,
    };
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(cols),
        Rows::of_index(rows),
    )
    .unwrap();
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let mut text = String::from("\x1b[?25l");
    let push_range = |text: &mut String, base: u32, rows: u32| {
        for row in 0..rows {
            for i in 0..32u32 {
                text.push(char::from_u32(base + row * 32 + i).unwrap());
            }
            if row + 1 < rows {
                text.push_str("\r\n");
            }
        }
    };
    push_range(&mut text, 0x2500, 5);
    text.push_str("\r\n");
    push_range(&mut text, 0x2800, 7);
    let bytes = text.as_bytes();
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    let n = r.atlas_stats().synthesised.value();
    // 160 box/block plus 224 braille, each a mask, none scrolled off.
    assert!(n >= 380, "only {n} masks");
    if std::env::var("BOXES_DUMP").is_ok() {
        dump_ppm(&canvas, "/tmp/baud-sampler.ppm");
    }
}

fn dump_ppm(c: &support::Canvas, path: &str) {
    let mut out = format!("P6\n{} {}\n255\n", c.width, c.height).into_bytes();
    for &p in &c.pixels {
        let b = p.to_be_bytes();
        out.extend_from_slice(&[b[1], b[2], b[3]]);
    }
    std::fs::write(path, out).unwrap();
}
