//! **Every logo file from one mark, baud's own drawing of `❯` with the
//! cursor in the next cell**, and the repository's link picture.
//!
//! **The mark is how baud draws it**: `❯` with its block cursor in the
//! very next cell, as a baud window shows them. So the ❯ is JetBrains Mono
//! Bold's own outline placed in a cell as baud's renderer places a glyph
//! -- its origin at the cell's left, its baseline [`BASELINE`] below the
//! cell's top, at [`FONT_PX`] -- and the cursor is the next cell, whole,
//! with no gap. The cell is [`CELL_W`] by [`CELL_H`]: baud's metrics at
//! 200 px, width round(0.6 px), height round(1.32 px) and baseline
//! round(1.02 px). The plasma example reports them from the renderer, and
//! this refuses to run if they differ; `baud-app/tests/brand_pictures.rs`
//! holds every file's ❯ and cursor to the renderer's own drawing of them.
//! Every file scales that one drawing to its cell; no file draws the ❯ any
//! other way.
//!
//! What it writes, each from the code below and nothing else:
//!
//! * `assets/logo/baud-lockup-{dark,light,mono-black,mono-white}.svg`:
//!   "baud" and the mark on one line, 300 by 130: the word where it always
//!   was, the ❯'s cell from x 192 with the block's top on the word's top
//!   ([`lined_up`]), the whole ink then centred across the box. The light forms' amber, here and in the
//!   marks and the title bar's lockup below, is the brand's, [`AMBER`],
//!   the dark forms' and the amber baud's cursor draws on both themes
//!   (`BaudCursor::light`, which the plasma example reports and the run
//!   holds to it), so the launch logo and the cursor match.
//! * `assets/logo/baud-lockup-{dark,light}-blinking.svg`: their fixed
//!   twins' bytes, under the names the brand sheet and `THIRD_PARTY.md`
//!   give them.
//! * `assets/logo/baud-mark-{dark,light,amber}.svg` (and `-dark-`,
//!   `-light-blinking`): the mark alone at 512, transparent.
//! * `assets/logo/baud-icon-512-{dark,light,amber}.svg` (and `-dark-`,
//!   `-light-blinking`) and `baud-icon-32-{dark,light}.svg`: the icons.
//! * `assets/logo/baud-icon-dock.svg` and `baud-icon-dock-<N>.png`: the
//!   Dock icon and its drawings at the iconset's sizes, which
//!   `scripts/bundle-macos.sh` packs into `baud.icns`;
//!   `baud-app/src/app.rs` embeds the SVG.
//! * `assets/logo/baud-mark-amber-tight.svg`: the mark alone, its box 300
//!   by 204, as the title bar showed it before the lockups.
//! * `assets/logo/baud-lockup-{dark,light}-tight.svg`: the title bar's
//!   lockups (`baud-app/src/mark.rs` embeds them), the dark and light
//!   lockups' word and mark, their cell the 168 of the mark above, on no
//!   ground, the box snug to the ink across and centred on the cell's
//!   rows.
//! * `assets/logo/baud-github-org.svg` and `.png`: the organisation
//!   picture, "baud" over the mark, `#F3F4F6` and `#F4A32C` on `#15171B`,
//!   500 by 500, the ground edge to edge so GitHub's own rounded corners
//!   apply; the PNG adds baud's plasma.
//! * `assets/readme/social.svg` and `.png`: the repository's link picture,
//!   1280 by 640, the lockup and the README's tagline on `#15171B`, over a
//!   strip of amber tape punched "baud" in the five-unit code of ITU-T S.1,
//!   with its feed holes.
//!
//! In every mark but the Dock's the cell is as tall as the cursor the file
//! had before (46 at the lockup's 300, 168 at 512, 10.5 at 32), the 512's
//! and the 32's on the same rows, and the mark's ink is centred across the
//! icon or under the word. **The Dock icon's mark is a prompt at the start of a terminal's
//! first line**: in the tile's upper left, in a terminal's own margin, 1.2
//! times the 512 icons' cell so it reads at the Dock's 32 pt, the ❯'s cell
//! corner one of their cell widths (76.36 units) in from the tile's left
//! and top and the mark scaled about it. The title bar's lockups keep the
//! 512 icons' cell, 168 units, so the mark beside the traffic lights keeps
//! its size with the word added before it.
//!
//! **Every PNG is its SVG drawn by resvg 0.48.1** ([`crate::raster`]) and
//! written as Pillow 12.3.0 wrote it ([`crate::png`]), but the
//! organisation picture's: that is its SVG drawn by resvg with two lights
//! of baud's plasma laid over it, on the word and on the mark, each drawn
//! by baud's CPU renderer (`baud-render/examples/brand_plasma.rs`, run
//! through cargo):
//!
//! * the word's: the launch splash's first pass at 0.0999 s, its seed
//!   `BA0D276F`, its box ending where the word does, placed by the
//!   similarity that takes the lockup's word onto the picture's, in the
//!   word's colour (the splash's rule: the light is the colour of the
//!   glyph it is over), under the glyphs;
//! * the mark's: the cursor's plasma trail 0.12 s into a move from the ❯'s
//!   cell into the cursor's, seed 3, drawn at 200 px and scaled to the
//!   picture's cell (Lanczos, [`crate::resample`]), in the amber, over the
//!   glyphs.
//!
//! Each light is taken out of a frame drawn alone on the ground as each
//! pixel's share of the way to the amber, which is how the CPU path blends
//! it (`Blend::Native`, source-over in encoded sRGB). With both lights at
//! zero the picture is its SVG's drawing to the pixel; the run checks that
//! before it writes, and prints the lights' provenance.
//!
//! Every letter is a path traced from the font ([`crate::outline`]), so no
//! renderer can draw it in another face. 500 for the wordmark and 700 for
//! the ❯ are the brand sheet's (`assets/logo/README.md`); the tagline is
//! 400, since the sheet names no weight for it. 400 and 700 are the
//! repository's own copies (`baud-app/fonts/jetbrains-mono/`), held here
//! to the hashes its `SOURCE` pins. 500 is in no committed file: it is
//! read out of the JetBrains Mono 2.304 release archive that `SOURCE`
//! names (`[[source: jbm-2304-release]]`), fetched by URL (or
//! `BRAND_ARCHIVE`, a copy of it) and refused unless the archive and the
//! face hash as pinned, and nothing of it is kept.
//!
//! GitHub's limits, from its help pages: the organisation picture is held
//! to the profile picture's, "less than 1 MB in size and smaller than 3000
//! by 3000 pixels ... about 500 by 500 pixels"
//! (`[[source: github-docs-profile-picture]]`; GitHub states no limit for
//! an organisation, so the profile's is applied); the link picture to
//! "under 1 MB ... at least 640 by 320 pixels (1280 by 640 pixels for best
//! display)", with a solid background where unsure
//! (`[[source: github-docs-social-preview]]`). 1 MB is read as 1,000,000
//! bytes, the smaller of its two meanings. Opaque is by construction: each
//! PNG is RGB (colour type 2, no alpha channel) with no chunk but IHDR,
//! IDAT and IEND, and every pixel resvg drew is checked opaque before the
//! alpha is dropped. The Dock PNGs keep their alpha: the icon's padding
//! is transparent.
//!
//! The tape: S.1's Table 1 gives each letter's five elements, and its
//! clause 3.2 "Condition Z corresponds to ... perforation in paper tape"
//! (`[[source: itu-t-s1]]`), so a Z is a hole. Element 1 is drawn on the
//! top track (S.1 gives no geometry). The holes are on one pitch across
//! the tape, the feed holes 0.046/0.072 of a code hole, the two diameters
//! ANSI X3.18 gives for one-inch tape (`[[source: ansi-x3-18-1967]]`). The
//! feed track is drawn between elements 2 and 3, as five-hole tape is
//! usually drawn; no text fetched here places it (S.1 gives no geometry,
//! and X3.18 is the eight-track tape, whose feed track has three code
//! tracks on one side). The check reads the tape back off the PNG it
//! wrote.
//!
//! **The tools are pinned, or the bytes move**: resvg by `Cargo.lock`,
//! zlib 1.3.2 by `tools/shell.nix` and refused at run time otherwise, the
//! fonts by hash; the plasma comes from the repository's own renderer,
//! built by the cargo on PATH. Measured on aarch64-darwin only: whether
//! the same bytes come out on another system is unmeasured, which is why
//! `--check` runs by hand and not in `devenv test` (it also needs the
//! network, for the archive). `baud-app/tests/brand_pictures.rs` holds the
//! committed files to the parts that need neither: the limits, outlines in
//! place of text, and the mark against baud's renderer.

use crate::json::{self, Value};
use crate::outline::{Bounds, Face, SvgPath, Transform};
use crate::png::{self, Image, Mode};
use crate::py::{self, num, round_to};
use crate::resample::Float;
use crate::sha256;
use std::path::{Path, PathBuf};

/// A refusal: the message the run stops with.
pub type Fail = String;

macro_rules! fail {
    ($($t:tt)*) => { return Err(format!($($t)*)) };
}

pub const JBM: &str = "baud-app/fonts/jetbrains-mono";
/// weight: (committed file, bytes, sha256), as `baud-app/fonts/jetbrains-mono/SOURCE`.
pub const COMMITTED: [(u16, &str, usize, &str); 2] = [
    (
        400,
        "JetBrainsMono-Regular.ttf",
        273_900,
        "a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f",
    ),
    (
        700,
        "JetBrainsMono-Bold.ttf",
        277_828,
        "5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb",
    ),
];
pub const ARCHIVE_URL: &str =
    "https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip";
pub const ARCHIVE: (usize, &str) = (
    5_622_857,
    "6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf",
);
pub const MEDIUM: (&str, usize, &str) = (
    "fonts/ttf/JetBrainsMono-Medium.ttf",
    273_860,
    "31c92d01a8a08528b718a43addf0ad3df0af2ca4b7b3290a452f70f358e14d3d",
);

/// baud's cell at 200 px: width round(0.6 px), height round(1.32 px), the
/// baseline round(1.02 px) below the cell's top.
pub const FONT_PX: f64 = 200.0;
pub const CELL_W: f64 = 120.0;
pub const CELL_H: f64 = 264.0;
pub const BASELINE: f64 = 204.0;

// The brand sheet's colours (assets/logo/README.md).
pub const GROUND: &str = "#15171B";
pub const TYPE_DARK: &str = "#F3F4F6";
pub const TYPE_LIGHT: &str = "#1E2128";
pub const AMBER: &str = "#F4A32C";

/// The README's tagline (line 10, between its `**`), a card line for each part.
pub const LINES: [&str; 4] = [
    "The terminal with character:",
    "its own font,",
    "its own cursor effects,",
    "and the speed to keep up.",
];

#[must_use]
pub fn tagline() -> String {
    LINES.join(" ")
}

pub const LOGO: &str = "assets/logo/";
pub const ORG: (&str, f64) = ("assets/logo/baud-github-org", 500.0);
pub const CARD: (&str, usize, usize) = ("assets/readme/social", 1280, 640);
pub const DOCK: &str = "assets/logo/baud-icon-dock";
/// The iconset's sizes, `icon_16x16` to `icon_512x512@2x` (`bundle-macos.sh`).
pub const DOCK_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];
/// The Dock's mark: 1.2 times the 512 icons' cell, scaled about the ❯'s
/// cell corner, which is one of their cell widths (168 * 120 / 264, 76.36
/// units of the 512 tile) in from the tile's left and top.
pub const DOCK_SCALE: f64 = 1.2;
#[must_use]
pub fn dock_corner() -> f64 {
    CELL_W * 168.0 / CELL_H
}
pub const TIGHT: &str = "assets/logo/baud-mark-amber-tight.svg";
/// The title bar's lockups, by the appearance each is for.
pub const TITLEBAR: [(&str, &str); 2] = [
    ("dark", "assets/logo/baud-lockup-dark-tight.svg"),
    ("light", "assets/logo/baud-lockup-light-tight.svg"),
];
pub const MB: usize = 1_000_000;

/// The lockup, 300 by 130: the word at x 40, baseline 88, size 62,
/// letter-spacing -2.8; the ❯'s cell from x 192, 46 tall, its top the top
/// of the word's `b` and `d`, 0.73 em above the baseline, so the block's
/// top sits on the word's top ([`lined_up`]).
pub const LOCKUP: (f64, f64) = (300.0, 130.0);
pub const WORD: (f64, f64, f64, f64) = (40.0, 88.0, 62.0, -2.8);
pub const LOCKUP_CELL: (f64, f64, f64) = (192.0, 42.74, 46.0);
/// **How far the block's top may sit from the word's top**, in the units
/// of the file: the two decimals each is written to.
pub const LINED_UP: f64 = 0.01;

/// The block's top on the word's top, within [`LINED_UP`].
///
/// # Errors
/// When it is not.
pub fn lined_up(word_top: f64, block_top: f64) -> Result<(), Fail> {
    if (word_top - block_top).abs() > LINED_UP {
        fail!(
            "the block's top is at {}, not on the word's top at {}",
            py::repr_float(block_top),
            py::repr_float(word_top)
        );
    }
    Ok(())
}

/// The organisation picture: the word at 160, and the word and mark rows
/// as they were, three quarters of the old 22-unit cursor apart.
pub const ORG_WORD: f64 = 160.0;
#[must_use]
pub fn org_gap() -> f64 {
    0.75 * 22.0
}

/// ITU-T S.1 Table 1, letter case: combination 2 B, 1 A, 21 U, 4 D, 31 Space.
pub const S1: [(char, &str); 5] = [
    ('b', "ZAAZZ"),
    ('a', "ZZAAA"),
    ('u', "ZZZAA"),
    ('d', "ZAAZA"),
    (' ', "AAZAA"),
];
#[must_use]
pub fn stream() -> String {
    "baud ".repeat(9)
}
/// Nine words, the first and the last whole.
pub const COLUMNS: usize = 44;
pub const PITCH_X: f64 = 28.0;
pub const PITCH_Y: f64 = 21.0;
pub const HOLE_R: f64 = 6.5;
pub const MARGIN: f64 = 20.0;
pub const BOTTOM: f64 = 616.0;
#[must_use]
pub fn feed_r() -> f64 {
    HOLE_R * 0.046 / 0.072
}
/// Tracks from the top: elements 1 and 2, the feed track, elements 3, 4 and 5.
pub const TRACKS: [&str; 6] = ["1", "2", "feed", "3", "4", "5"];

fn code(c: char) -> &'static str {
    S1.iter()
        .find(|(k, _)| *k == c)
        .map(|(_, v)| *v)
        .expect("a letter of the tape")
}

fn band_top() -> f64 {
    let band_h = (TRACKS.len() - 1) as f64 * PITCH_Y + 2.0 * MARGIN;
    BOTTOM - band_h
}

/// `round(v, 2)`.
fn r2(v: f64) -> f64 {
    round_to(v, 2)
}

/// v to two decimals, never above it: a box edge that cannot cut ink.
fn down2(v: f64) -> f64 {
    round_to(v * 100.0, 6).floor() / 100.0
}

fn up2(v: f64) -> f64 {
    round_to(v * 100.0, 6).ceil() / 100.0
}

// ---------------------------------------------------------------- the fonts

/// The three faces' bytes, read and held to their pins.
#[derive(Debug)]
pub struct Fonts {
    pub faces: Vec<(u16, Vec<u8>)>,
}

/// The faces at `root`, and 500 out of `archive`'s bytes.
///
/// # Errors
/// When a face or the archive is not the one pinned.
pub fn fonts(root: &Path, archive: &(String, Vec<u8>)) -> Result<Fonts, Fail> {
    let mut faces = Vec::new();
    for (weight, name, size, digest) in COMMITTED {
        let b =
            std::fs::read(root.join(JBM).join(name)).map_err(|e| format!("{JBM}/{name}: {e}"))?;
        if (b.len(), sha256::hex(&b).as_str()) != (size, digest) {
            fail!("{JBM}/{name} is not the file SOURCE pins");
        }
        faces.push((weight, b));
    }
    let (path, b) = archive;
    if (b.len(), sha256::hex(b).as_str()) != ARCHIVE {
        fail!("{path} is not the release archive pinned ({ARCHIVE_URL})");
    }
    let m = crate::zip::read(b, MEDIUM.0)?;
    medium_holds(&m)?;
    faces.push((500, m));
    Ok(Fonts { faces })
}

/// The 500 read out of the archive is the face pinned.
///
/// # Errors
/// When it is not.
pub fn medium_holds(m: &[u8]) -> Result<(), Fail> {
    if (m.len(), sha256::hex(m).as_str()) != (MEDIUM.1, MEDIUM.2) {
        fail!("{} in the archive is not the face pinned", MEDIUM.0);
    }
    Ok(())
}

/// Text to SVG path data, positioned as SVG text is: x the first glyph's
/// origin, y the baseline, letter-spacing after every glyph.
pub struct Type<'a> {
    faces: Vec<(u16, Face<'a>)>,
}

/// An ink box: (left, top, right, bottom).
pub type Box4 = [f64; 4];

impl<'a> Type<'a> {
    /// # Errors
    /// When a face cannot be read.
    pub fn new(fonts: &'a Fonts) -> Result<Type<'a>, Fail> {
        let faces = fonts
            .faces
            .iter()
            .map(|(w, b)| Face::new(b).map(|f| (*w, f)))
            .collect::<Result<_, _>>()?;
        Ok(Type { faces })
    }

    fn face(&self, weight: u16) -> &Face<'a> {
        &self
            .faces
            .iter()
            .find(|(w, _)| *w == weight)
            .expect("a weight loaded")
            .1
    }

    /// The path data and ink box of `s` set at (x, y).
    ///
    /// # Errors
    /// When the face has no glyph for a character.
    pub fn text(
        &self,
        s: &str,
        x: f64,
        y: f64,
        size: f64,
        weight: u16,
        ls: f64,
    ) -> Result<(String, Box4), Fail> {
        let face = self.face(weight);
        let k = size / f64::from(face.upem);
        let ntos = |v: f64| num(v);
        let mut pen = SvgPath::new(&ntos);
        let mut bx = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut cx = x;
        for ch in s.chars() {
            let Some(g) = face.glyph(ch) else {
                fail!(
                    "U+{:04X} is not in JetBrains Mono at {weight}",
                    u32::from(ch)
                );
            };
            face.draw(g, &mut pen, Some(Transform([k, 0.0, 0.0, -k, cx, y])));
            let mut bp = Bounds::default();
            face.draw(g, &mut bp, None);
            if let Some([a, b, c, d]) = bp.bounds {
                bx = [
                    py::min(bx[0], cx + a * k),
                    py::min(bx[1], y - d * k),
                    py::max(bx[2], cx + c * k),
                    py::max(bx[3], y - b * k),
                ];
            }
            cx += f64::from(face.advance(g)) * k + ls;
        }
        Ok((pen.commands(), bx))
    }

    /// # Errors
    /// As [`Type::text`].
    pub fn ink(&self, s: &str, size: f64, weight: u16, ls: f64) -> Result<Box4, Fail> {
        Ok(self.text(s, 0.0, 0.0, size, weight, ls)?.1)
    }
}

// ---------------------------------------------------------------- SVG

pub fn path(d: &str, fill: &str) -> String {
    format!("  <path d=\"{d}\" fill=\"{fill}\"/>\n")
}

pub fn rect(x: f64, y: f64, w: f64, h: f64, fill: &str) -> String {
    format!(
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\"/>\n",
        num(x),
        num(y),
        num(w),
        num(h)
    )
}

pub fn square(n: u32, rx: u32, fill: &str) -> String {
    format!("  <rect width=\"{n}\" height=\"{n}\" rx=\"{rx}\" fill=\"{fill}\"/>\n")
}

pub fn circle(cx: f64, cy: f64, r: f64, fill: &str) -> String {
    format!(
        "  <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{fill}\"/>\n",
        num(cx),
        num(cy),
        num(r)
    )
}

/// The comment every SVG carries.
pub const NOTE: &str = "  <!-- Drawn by tools/baud-pictures (brand): edit the tool, not this file. The\n       glyphs are JetBrains Mono 2.304's own outlines, under its OFL\n       (JetBrainsMono-OFL.txt beside this file). -->\n";

pub fn svg(w: &str, h: &str, body: &str, label: &str, view: Option<&str>) -> String {
    let view = view.map_or_else(|| format!("0 0 {w} {h}"), str::to_string);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"{view}\" role=\"img\" aria-label=\"{label}\">\n{NOTE}{body}</svg>\n"
    )
}

// ---------------------------------------------------------------- the mark

/// A mark: its body, the ❯'s ink box, the cursor's box.
pub struct Mark {
    pub body: String,
    pub ink: Box4,
    pub cursor: Box4,
}

/// baud's "❯" in the cell at (`cell_x`, `cell_top`), `h` tall, and the
/// cursor in the next cell: the ❯ as the renderer places a glyph, the
/// cursor the cell whole. The ❯'s path comes right before the cursor's
/// rect, in one fill, which is how `brand_pictures.rs` finds them. The
/// cell's edges are rounded to the SVG's two decimals first, so the
/// cursor's edges are the cell's as written.
///
/// # Errors
/// As [`Type::text`].
pub fn mark(t: &Type<'_>, cell_x: f64, cell_top: f64, h: f64, fill: &str) -> Result<Mark, Fail> {
    let k = h / CELL_H;
    let (top, bottom) = (r2(cell_top), r2(cell_top + h));
    let (x1, x2) = (r2(cell_x + CELL_W * k), r2(cell_x + 2.0 * CELL_W * k));
    let (d, ink) = t.text("❯", cell_x, top + BASELINE * k, FONT_PX * k, 700, 0.0)?;
    Ok(Mark {
        body: path(&d, fill) + &rect(x1, top, x2 - x1, bottom - top, fill),
        ink,
        cursor: [x1, top, x2, bottom],
    })
}

/// The mark's ink, left to right: the ❯'s left to the cursor's right.
pub fn span(m: &Mark) -> (f64, f64) {
    (
        py::min(m.ink[0], m.cursor[0]),
        py::max(m.ink[2], m.cursor[2]),
    )
}

/// The mark, `h` tall from `cell_top`, its ink centred on x = `cx`.
pub fn centred(t: &Type<'_>, cx: f64, cell_top: f64, h: f64, fill: &str) -> Result<Mark, Fail> {
    let (left, right) = span(&mark(t, 0.0, 0.0, h, fill)?);
    mark(t, cx - (left + right) / 2.0, cell_top, h, fill)
}

/// How far the lockup's glyphs move to centre their ink across its box.
///
/// # Errors
/// As [`Type::text`].
pub fn lockup_dx(t: &Type<'_>) -> Result<f64, Fail> {
    let (_, wb) = t.text("baud", WORD.0, WORD.1, WORD.2, 500, WORD.3)?;
    let m = mark(t, LOCKUP_CELL.0, LOCKUP_CELL.1, LOCKUP_CELL.2, GROUND)?;
    Ok(LOCKUP.0 / 2.0 - (py::min(wb[0], m.ink[0]) + py::max(wb[2], m.cursor[2])) / 2.0)
}

/// The lockup with its 300-wide box's origin at (ox, oy), scaled by s:
/// the body, its ink box, and the cursor's width.
fn lockup(
    t: &Type<'_>,
    ox: f64,
    oy: f64,
    s: f64,
    word_fill: &str,
    mark_fill: &str,
) -> Result<(String, Box4, f64), Fail> {
    let dx = lockup_dx(t)?;
    let (x, y, size, ls) = WORD;
    let (w, wb) = t.text("baud", ox + (x + dx) * s, oy + y * s, size * s, 500, ls * s)?;
    let (cx, cy, h) = LOCKUP_CELL;
    let m = mark(t, ox + (cx + dx) * s, oy + cy * s, h * s, mark_fill)?;
    let (ink, cur) = (m.ink, m.cursor);
    lined_up(wb[1], cur[1])?;
    let bx = [
        py::min(wb[0], ink[0]),
        py::min(py::min(wb[1], ink[1]), cur[1]),
        py::max(wb[2], cur[2]),
        py::max(py::max(wb[3], ink[3]), cur[3]),
    ];
    Ok((path(&w, word_fill) + &m.body, bx, cur[2] - cur[0]))
}

// ---------------------------------------------------------------- the files

/// The files the run draws, in the order it draws them.
pub type Files = Vec<(String, Vec<u8>)>;

fn put(out: &mut Vec<(String, String)>, name: impl Into<String>, body: String) {
    out.push((name.into(), body));
}

fn get<'o>(out: &'o [(String, String)], name: &str) -> &'o str {
    &out.iter().find(|(n, _)| n == name).expect("drawn above").1
}

/// Every SVG in `assets/logo` but the organisation picture's.
///
/// # Errors
/// As [`Type::text`].
pub fn logo_svgs(t: &Type<'_>) -> Result<Vec<(String, String)>, Fail> {
    let mut out = Vec::new();
    let (w, h) = LOCKUP;
    for (name, word, amber, ground) in [
        ("dark", TYPE_DARK, AMBER, Some(GROUND)),
        ("light", TYPE_LIGHT, AMBER, None),
        ("mono-black", "#000000", "#000000", None),
        ("mono-white", "#FFFFFF", "#FFFFFF", None),
    ] {
        let (body, _, _) = lockup(t, 0.0, 0.0, 1.0, word, amber)?;
        let bg = ground.map_or_else(String::new, |g| rect(0.0, 0.0, w, h, g));
        put(
            &mut out,
            format!("{LOGO}baud-lockup-{name}.svg"),
            svg(&num(w), &num(h), &(bg + &body), "baud", None),
        );
    }

    // At 512 the cell is 168 tall from 172, as the cursor was; at 32, 10.5
    // from 10.75. Light is the dark one reversed, and amber is light's.
    let (big, small) = ((256.0, 172.0, 168.0), (16.0, 10.75, 10.5));
    for (name, fill) in [("dark", AMBER), ("light", AMBER), ("amber", AMBER)] {
        let m = centred(t, big.0, big.1, big.2, fill)?;
        put(
            &mut out,
            format!("{LOGO}baud-mark-{name}.svg"),
            svg("512", "512", &m.body, "baud", None),
        );
    }
    for (name, sq, fill) in [
        ("dark", GROUND, AMBER),
        ("light", AMBER, GROUND),
        ("amber", AMBER, GROUND),
    ] {
        let m = centred(t, big.0, big.1, big.2, fill)?;
        put(
            &mut out,
            format!("{LOGO}baud-icon-512-{name}.svg"),
            svg(
                "512",
                "512",
                &(square(512, 112, sq) + &m.body),
                "baud",
                None,
            ),
        );
    }
    for (name, sq, fill) in [("dark", GROUND, AMBER), ("light", AMBER, GROUND)] {
        let m = centred(t, small.0, small.1, small.2, fill)?;
        put(
            &mut out,
            format!("{LOGO}baud-icon-32-{name}.svg"),
            svg("32", "32", &(square(32, 7, sq) + &m.body), "baud", None),
        );
    }
    for kind in ["lockup", "mark", "icon-512"] {
        for name in ["dark", "light"] {
            let twin = get(&out, &format!("{LOGO}baud-{kind}-{name}.svg")).to_string();
            put(
                &mut out,
                format!("{LOGO}baud-{kind}-{name}-blinking.svg"),
                twin,
            );
        }
    }

    // The Dock icon: the dark tile at 80% of the canvas, on the macOS
    // app-icon grid, the mark in its upper left at DOCK_SCALE times the
    // 512 icons' cell, the ❯'s cell corner dock_corner() in from its left
    // and top: a prompt at the start of a terminal's first line, in a
    // terminal's own margin, and big enough to read at the Dock's 32 pt.
    let d = mark(t, dock_corner(), dock_corner(), big.2 * DOCK_SCALE, AMBER)?.body;
    let inner: String = d
        .trim_end_matches('\n')
        .split('\n')
        .map(|l| format!("  {l}\n"))
        .collect();
    let dock = format!(
        "  <!-- macOS app-icon grid: the rounded square is ~80% of the canvas with\n       transparent padding, so it sits in the Dock like every other app.\n       Dark squircle, amber mark. -->\n  <g transform=\"translate(50,50) scale(0.8047)\">\n    <rect width=\"512\" height=\"512\" rx=\"115\" fill=\"#15171B\"/>\n{inner}  </g>\n"
    );
    put(
        &mut out,
        format!("{DOCK}.svg"),
        svg("512", "512", &dock, "baud", None),
    );
    // The titlebar's box keeps its 300 by 204 and its y, and centres the
    // mark's ink across it: the 512 icons' mark, centred.
    let m = centred(t, big.0, big.1, big.2, AMBER)?;
    let (left, right) = span(&m);
    let view = format!("{} 154 300 204", num((left + right) / 2.0 - 150.0));
    put(
        &mut out,
        TIGHT,
        svg("300", "204", &m.body, "baud", Some(&view)),
    );

    // The title bar's lockups: the dark and light lockups' word and mark,
    // scaled so the cell is the mark's 168 above, which keeps the mark the
    // size it was beside the traffic lights, and on no ground, since the
    // strip is the ground. The box is snug to the ink across, the word's
    // left to the cursor's right, so the image ends where the ink does; up
    // and down it is centred on the cell's rows, so a view the image's
    // shape centres the mark in the strip where the mark alone was.
    let s = big.2 / LOCKUP_CELL.2;
    for ((_, file), (word, amber)) in TITLEBAR
        .into_iter()
        .zip([(TYPE_DARK, AMBER), (TYPE_LIGHT, AMBER)])
    {
        let (body, bx, _) = lockup(t, 0.0, 0.0, s, word, amber)?;
        let top = r2(LOCKUP_CELL.1 * s);
        let centre = top + (r2(top + big.2) - top) / 2.0;
        let half = py::max(centre - bx[1], bx[3] - centre);
        let (x0, x1, y0, y1) = (
            down2(bx[0]),
            up2(bx[2]),
            down2(centre - half),
            up2(centre + half),
        );
        let (w, h) = (num(x1 - x0), num(y1 - y0));
        let view = format!("{} {} {w} {h}", num(x0), num(y0));
        put(&mut out, file, svg(&w, &h, &body, "baud", Some(&view)));
    }
    Ok(out)
}

/// The organisation picture: its SVG, its word's pen, the word's scale
/// from the lockup's, and its mark's cells (the ❯'s left, the top, the
/// cursor's left and right, the bottom).
pub struct Organisation {
    pub svg: String,
    pub pen: (f64, f64),
    pub s: f64,
    pub cells: [f64; 5],
}

/// # Errors
/// As [`Type::text`], and when the logo comes nearer an edge than a
/// cursor's width.
pub fn organisation(t: &Type<'_>) -> Result<Organisation, Fail> {
    let n = ORG.1;
    let s = ORG_WORD / WORD.2;
    let wx = t.ink("baud", ORG_WORD, 500, WORD.3 * s)?;
    let (ww, wh) = (wx[2] - wx[0], wx[3] - wx[1]);
    let h = LOCKUP_CELL.2 * s;
    let gap = org_gap() * s;
    let top = (n - (wh + gap + h)) / 2.0;
    let pen = (n / 2.0 - ww / 2.0 - wx[0], top - wx[1]);
    let (w, wb) = t.text("baud", pen.0, pen.1, ORG_WORD, 500, WORD.3 * s)?;
    let m = centred(t, n / 2.0, top + wh + gap, h, AMBER)?;
    let (ink, cur) = (m.ink, m.cursor);
    let bx = [
        py::min(wb[0], ink[0]),
        wb[1],
        py::max(wb[2], cur[2]),
        py::max(ink[3], cur[3]),
    ];
    let cw = cur[2] - cur[0];
    clear_space(bx, n, cw)?;
    let cells = [cur[0] - cw, cur[1], cur[0], cur[2], cur[3]];
    let body = rect(0.0, 0.0, n, n, GROUND) + &path(&w, TYPE_DARK) + &m.body;
    Ok(Organisation {
        svg: svg(&num(n), &num(n), &body, "baud", None),
        pen,
        s,
        cells,
    })
}

/// The organisation picture's clear space: more than a cursor's width to
/// every edge.
///
/// # Errors
/// When it is less.
pub fn clear_space(bx: Box4, n: f64, cw: f64) -> Result<(), Fail> {
    let clear = py::min(py::min(py::min(bx[0], bx[1]), n - bx[2]), n - bx[3]);
    if clear < cw {
        fail!(
            "the organisation picture's logo has {clear:.1} px to an edge, under a cursor width ({cw:.1})"
        );
    }
    Ok(())
}

/// The link picture's SVG.
///
/// # Errors
/// As [`Type::text`], and when its parts crowd each other or the crop.
pub fn card(t: &Type<'_>) -> Result<String, Fail> {
    let (_, w, h) = CARD;
    let (w, h) = (w as f64, h as f64);
    let band_h = (TRACKS.len() - 1) as f64 * PITCH_Y + 2.0 * MARGIN;
    let band_top = BOTTOM - band_h;
    let mid = band_top / 2.0;
    // The lockup 440 wide with its box at x 60, the tagline at x 560, both on mid.
    let s = 440.0 / LOCKUP.0;
    let (_, probe, _) = lockup(t, 0.0, 0.0, s, GROUND, GROUND)?;
    let (lb, lbox, cw) = lockup(
        t,
        60.0,
        mid - (probe[1] + probe[3]) / 2.0,
        s,
        TYPE_DARK,
        AMBER,
    )?;
    let (size, pitch, cap) = (38.0, 56.0, 0.73 * 38.0);
    let mut tb = String::new();
    let mut tbox = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (i, line) in LINES.iter().enumerate() {
        let y = mid + (i as f64 - (LINES.len() - 1) as f64 / 2.0) * pitch + cap / 2.0;
        let (d, b) = t.text(line, 560.0, y, size, 400, 0.0)?;
        tb += &path(&d, TYPE_DARK);
        tbox = [
            py::min(tbox[0], b[0]),
            py::min(tbox[1], b[1]),
            py::max(tbox[2], b[2]),
            py::max(tbox[3], b[3]),
        ];
    }
    card_space(lbox, tbox, cw, band_top, w)?;
    let mut tape = rect(-1.0, band_top, w + 2.0, band_h, AMBER);
    let x0 = (w - (COLUMNS - 1) as f64 * PITCH_X) / 2.0;
    let letters: Vec<char> = stream().chars().collect();
    for (c, &letter) in letters.iter().enumerate().take(COLUMNS) {
        let code = code(letter).as_bytes();
        let cx = x0 + c as f64 * PITCH_X;
        for (row, track) in TRACKS.iter().enumerate() {
            let cy = band_top + MARGIN + row as f64 * PITCH_Y;
            if *track == "feed" {
                tape += &circle(cx, cy, feed_r(), GROUND);
            } else if code[track.parse::<usize>().expect("a track") - 1] == b'Z' {
                tape += &circle(cx, cy, HOLE_R, GROUND);
            }
        }
    }
    let label = format!("baud: {}", tagline());
    Ok(svg(
        &num(w),
        &num(h),
        &(rect(0.0, 0.0, w, h, GROUND) + &lb + &tb + &tape),
        &label,
        None,
    ))
}

/// The card's clear space and crop.
///
/// # Errors
/// When the lockup and the tagline sit inside each other's clear space or
/// the tape's, or the tagline reaches the crop.
pub fn card_space(lbox: Box4, tbox: Box4, cw: f64, band_top: f64, w: f64) -> Result<(), Fail> {
    if lbox[2] + cw > tbox[0]
        || py::max(lbox[3], tbox[3]) + cw > band_top
        || py::min(lbox[1], tbox[1]) < cw
    {
        fail!("the card's lockup or tagline is inside the lockup's clear space of another element");
    }
    if tbox[2] > w - 29.0 {
        fail!("the tagline reaches the 29 px a 1.91:1 crop of the card would trim");
    }
    Ok(())
}

/// The letters the card's tape spells, read back off its pixels with S.1,
/// and how many feed holes it has.
#[must_use]
pub fn read_tape(im: &Image) -> (String, usize) {
    let band_top = band_top();
    let x0 = (im.w as f64 - (COLUMNS - 1) as f64 * PITCH_X) / 2.0;
    let hole = |x: f64, y: f64| -> bool {
        let (x, y) = (py::round(x), py::round(y));
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return false;
        };
        if x >= im.w || y >= im.h {
            return false;
        }
        // the ground shows through
        im.at(x, y)
            .iter()
            .take(3)
            .map(|&v| u32::from(v))
            .sum::<u32>()
            < 200
    };
    let (mut out, mut feed) = (String::new(), 0);
    for c in 0..COLUMNS {
        let cx = x0 + c as f64 * PITCH_X;
        let mut code = String::new();
        for (row, track) in TRACKS.iter().enumerate() {
            let cy = band_top + MARGIN + row as f64 * PITCH_Y;
            if *track == "feed" {
                // a hole at the column and tape on both sides of it, at the
                // code hole's radius: a feed hole, smaller than a code hole
                if hole(cx, cy) && !hole(cx - HOLE_R + 1.0, cy) && !hole(cx + HOLE_R - 1.0, cy) {
                    feed += 1;
                }
            } else {
                code.push(if hole(cx, cy) { 'Z' } else { 'A' });
            }
        }
        out.push(S1.iter().find(|(_, v)| *v == code).map_or('?', |(k, _)| *k));
    }
    (out, feed)
}

/// The tape check.
///
/// # Errors
/// When the tape does not spell the stream with a feed hole a column.
pub fn tape_reads(im: &Image) -> Result<(), Fail> {
    let (spelled, feed) = read_tape(im);
    let want: String = stream().chars().take(COLUMNS).collect();
    if spelled != want || feed != COLUMNS {
        fail!(
            "the tape reads {} with {feed} feed holes, not {} with {COLUMNS}",
            py::repr_str(&spelled),
            py::repr_str(&want)
        );
    }
    Ok(())
}

// ---------------------------------------------------------------- PNG

/// The image as RGB, refused if any pixel resvg drew is not opaque:
/// Pillow's `convert("RGB")`, which drops the alpha and composites
/// nothing.
///
/// # Errors
/// When a pixel's alpha is not 255.
pub fn opaque(im: &Image, name: &str) -> Result<Image, Fail> {
    if im.mode != Mode::Rgba || im.px.chunks(4).any(|p| p[3] != 255) {
        fail!("{name}: resvg drew a pixel that is not opaque");
    }
    Ok(rgb_of(im))
}

/// `convert("RGB")` of an RGBA image: the alpha dropped.
#[must_use]
pub fn rgb_of(im: &Image) -> Image {
    Image {
        w: im.w,
        h: im.h,
        mode: Mode::Rgb,
        px: im.px.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect(),
    }
}

/// What is wrong with the PNG as GitHub's picture, or nothing.
#[must_use]
pub fn github_limits(name: &str, b: &[u8], square: bool) -> Vec<String> {
    let Some(chunks) = png::chunks(b) else {
        return vec![format!("{name}: not a PNG")];
    };
    let kinds: Vec<String> = chunks.iter().map(|(k, _)| k.clone()).collect();
    let Some((_, ihdr)) = chunks.iter().find(|(k, _)| k == "IHDR") else {
        return vec![format!("{name}: not a PNG")];
    };
    if ihdr.len() < 10 {
        return vec![format!("{name}: an IHDR of {} bytes", ihdr.len())];
    }
    let w = u32::from_be_bytes(ihdr[0..4].try_into().expect("four bytes"));
    let h = u32::from_be_bytes(ihdr[4..8].try_into().expect("four bytes"));
    let (depth, colour) = (ihdr[8], ihdr[9]);
    let side = ORG.1 as u32;
    let mut bad = Vec::new();
    if b.len() >= MB {
        bad.push(format!("{name}: {} bytes, not under 1 MB", b.len()));
    }
    if square && !(w == h && h == side) {
        bad.push(format!(
            "{name}: {w}x{h}, not the {side} square GitHub recommends"
        ));
    }
    if square && w.max(h) >= 3000 {
        bad.push(format!("{name}: {w}x{h}, not smaller than 3000 by 3000"));
    }
    if !square && (w as usize, h as usize) != (CARD.1, CARD.2) {
        bad.push(format!(
            "{name}: {w}x{h}, not the 1280 by 640 GitHub recommends"
        ));
    }
    if (depth, colour) != (8, 2) {
        bad.push(format!(
            "{name}: bit depth {depth}, colour type {colour}: not 8-bit RGB, which has no alpha"
        ));
    }
    if kinds
        .iter()
        .any(|k| !["IHDR", "IDAT", "IEND"].contains(&k.as_str()))
        || kinds.first().map(String::as_str) != Some("IHDR")
        || kinds.last().map(String::as_str) != Some("IEND")
    {
        let mut set: Vec<String> = kinds.clone();
        set.sort();
        set.dedup();
        bad.push(format!(
            "{name}: chunks {}, not IHDR, IDAT and IEND alone",
            py::repr_list(&set)
        ));
    }
    bad
}

// ---------------------------------------------------------------- the plasma

fn rgb(hexa: &str) -> [i64; 3] {
    [1, 3, 5].map(|i| i64::from_str_radix(&hexa[i..i + 2], 16).expect("a hex colour"))
}

/// The plasma example's report and frames.
pub struct Plasma {
    pub rep: Value,
    pub frames: PathBuf,
}

/// Run the plasma example, the word's light placed so the lockup's word
/// (its pen at WORD's x moved by `lockup_dx`, and its baseline, as the
/// splash draws it) lands on the picture's (its pen at `pen`, `s` times
/// the size).
///
/// # Errors
/// When cargo fails, the report does not read, or [`plasma_holds`] refuses.
pub fn plasma(
    t: &Type<'_>,
    root: &Path,
    tmp: &Path,
    pen: (f64, f64),
    s: f64,
) -> Result<Plasma, Fail> {
    let out = tmp.join("plasma");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let dx = lockup_dx(t)?;
    let x0 = WORD.0 + dx;
    let at = (pen.0 - x0 * s, pen.1 - WORD.1 * s);
    let status = std::process::Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--release",
            "--locked",
            "-p",
            "baud-render",
            "--example",
            "brand_plasma",
            "--",
        ])
        .arg(&out)
        .args([
            py::repr_float(s),
            py::repr_float(at.0),
            py::repr_float(at.1),
        ])
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo did not run: {e}"))?;
    if !status.success() {
        fail!("the plasma example failed: {status}");
    }
    let text = std::fs::read_to_string(out.join("plasma.json")).map_err(|e| e.to_string())?;
    let rep = json::parse(&text)?;
    let cur = mark(t, LOCKUP_CELL.0 + dx, LOCKUP_CELL.1, LOCKUP_CELL.2, GROUND)?.cursor;
    plasma_holds(&rep, x0, cur)?;
    Ok(Plasma { rep, frames: out })
}

/// Refuses a splash whose word pen or block is not the lockup's (its pen
/// at `x0` on the word's baseline, its block the cursor `cur`), a
/// renderer whose cell is not this one, and a cursor on either theme's page
/// that is not the brand's amber.
///
/// # Errors
/// When one of those differs.
pub fn plasma_holds(rep: &Value, x0: f64, cur: Box4) -> Result<(), Fail> {
    let w = rep.get("word")?;
    let p = w.get("pen")?.nums()?;
    if p.len() != 2 || (p[0] - x0).abs() > 1e-3 || p[1] != WORD.1 {
        fail!(
            "the splash's word pen is {}, not the lockup's ({x0:.3}, {})",
            w.get("pen")?.py(),
            py::repr_float(WORD.1)
        );
    }
    let hole = w.get("hole")?.nums()?;
    let off = hole
        .iter()
        .zip(cur)
        .map(|(a, b)| (a - b).abs())
        .fold(f64::NEG_INFINITY, py::max);
    if hole.len() != 4 || off > 0.01 {
        fail!(
            "the splash's block is {}, not the lockup's cursor {}",
            w.get("hole")?.py(),
            py::repr_tuple(&cur)
        );
    }
    let m = rep.get("mark")?;
    let got = [
        m.get("px")?.num()?,
        m.get("cell_w")?.num()?,
        m.get("cell_h")?.num()?,
        m.get("baseline")?.num()?,
    ];
    if got != [FONT_PX, CELL_W, CELL_H, BASELINE] {
        fail!(
            "baud's renderer draws a {} by {} cell, baseline {}, at {} px",
            m.get("cell_w")?.py(),
            m.get("cell_h")?.py(),
            m.get("baseline")?.py(),
            m.get("px")?.py()
        );
    }
    let a = rep.get("amber")?;
    if a.get("dark")?.text()? != AMBER {
        fail!(
            "baud's cursor on the dark page is {}, not the brand's {AMBER}",
            a.get("dark")?.py()
        );
    }
    if a.get("light")?.text()? != AMBER {
        fail!(
            "baud's cursor on the light page is {}, not the brand's {AMBER}",
            a.get("light")?.py()
        );
    }
    Ok(())
}

/// A frame the example wrote: little-endian `0xAARRGGBB`, its RGB.
///
/// # Errors
/// When the file is missing or not `w` by `h`.
pub fn frame(d: &Path, name: &str, w: usize, h: usize) -> Result<Image, Fail> {
    let b = std::fs::read(d.join(format!("{name}.raw"))).map_err(|e| format!("{name}.raw: {e}"))?;
    if b.len() != w * h * 4 {
        fail!("{name}.raw is {} bytes, not {w} by {h}", b.len());
    }
    Ok(Image {
        w,
        h,
        mode: Mode::Rgb,
        px: b.chunks(4).flat_map(|p| [p[2], p[1], p[0]]).collect(),
    })
}

/// The frames' sizes and the mark's cell in its frame, from the report.
pub struct Frames {
    pub mark: (usize, usize),
    pub word: (usize, usize),
    pub col: f64,
    pub row: f64,
}

/// # Errors
/// When the report lacks them.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "canvas sizes"
)]
pub fn frames(rep: &Value) -> Result<Frames, Fail> {
    let (m, w) = (rep.get("mark")?, rep.get("word")?);
    let size = |v: &Value| -> Result<(usize, usize), Fail> {
        Ok((
            v.get("width")?.num()? as usize,
            v.get("height")?.num()? as usize,
        ))
    };
    Ok(Frames {
        mark: size(m)?,
        word: size(w)?,
        col: m.get("col")?.num()?,
        row: m.get("row")?.num()?,
    })
}

/// The organisation picture: its SVG's drawing (`base`, RGB) with the
/// word's light under the glyphs and the mark's over them, each taken
/// from its frame in `d`. Returns the picture, the lights' peak, and how
/// many pixels they changed.
///
/// # Errors
/// When a frame is missing.
#[allow(
    clippy::cast_sign_loss,
    reason = "sizes and offsets checked non-negative"
)]
pub fn org_picture(
    base: &Image,
    f: &Frames,
    d: &Path,
    cells: [f64; 5],
) -> Result<(Image, f64, usize), Fail> {
    let (g, c, t) = (rgb(GROUND), rgb(AMBER), rgb(TYPE_DARK));
    let dd: [i64; 3] = [0, 1, 2].map(|i| c[i] - g[i]);
    let n2: i64 = dd.iter().map(|v| v * v).sum();
    let cover = |p: &[u8]| -> f64 {
        let s: i64 = (0..3).map(|i| (i64::from(p[i]) - g[i]) * dd[i]).sum();
        py::max(0.0, py::min(1.0, s as f64 / n2 as f64))
    };
    let n = base.w;
    // The mark's light: the trail's frame against the frame without it,
    // none where the glyph or the cursor is all but whole, scaled from the
    // 200 px cell to the picture's.
    let (mw, mh) = f.mark;
    let (bp, tp) = (
        frame(d, "mark-base", mw, mh)?,
        frame(d, "mark-trail", mw, mh)?,
    );
    let mut alpha = Float {
        w: mw,
        h: mh,
        px: vec![0.0; mw * mh],
    };
    for y in 0..mh {
        for x in 0..mw {
            let (cb, ct) = (cover(bp.at(x, y)), cover(tp.at(x, y)));
            let v = if cb > 0.98 {
                0.0
            } else {
                py::max(0.0, py::min(1.0, (ct - cb) / (1.0 - cb)))
            };
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a mode F pixel is a C float"
            )]
            {
                alpha.px[y * mw + x] = v as f32;
            }
        }
    }
    let (cell_x, cell_top, _, _, cell_bottom) = (cells[0], cells[1], cells[2], cells[3], cells[4]);
    let k = (cell_bottom - cell_top) / CELL_H;
    let size = |v: usize| py::round(v as f64 * k).max(1) as usize;
    let lay = alpha.lanczos(size(mw), size(mh));
    let ox = py::round(cell_x - f.col * CELL_W * k);
    let oy = py::round(cell_top - f.row * CELL_H * k);
    let mut am = vec![0.0_f64; n * n];
    let ni = n as i64;
    for y in 0..lay.h {
        for x in 0..lay.w {
            let a = f64::from(lay.px[y * lay.w + x]);
            let (px, pyy) = (ox + x as i64, oy + y as i64);
            if a > 0.0 && (0..ni).contains(&px) && (0..ni).contains(&pyy) {
                am[pyy as usize * n + px as usize] = py::min(1.0, a);
            }
        }
    }
    // The word's light, under each pixel's glyph coverage: its share of the
    // way to its glyph's colour, the word's above the picture's middle and
    // the amber below.
    let wp = frame(d, "word", f.word.0, f.word.1)?;
    let mut out = Image {
        w: n,
        h: n,
        mode: Mode::Rgb,
        px: vec![0; n * n * 3],
    };
    let (mut peak, mut lit) = (0.0_f64, 0usize);
    for y in 0..n {
        let col = if (y as f64) < n as f64 / 2.0 { t } else { c };
        let dc: [i64; 3] = [0, 1, 2].map(|i| col[i] - g[i]);
        let n2c: i64 = dc.iter().map(|v| v * v).sum();
        for x in 0..n {
            let b = base.at(x, y);
            let aw = cover(wp.at(x, y));
            let s: i64 = (0..3).map(|i| (i64::from(b[i]) - g[i]) * dc[i]).sum();
            let cg = py::max(0.0, py::min(1.0, s as f64 / n2c as f64));
            let a = am[y * n + x];
            let v: [i64; 3] = [0, 1, 2]
                .map(|i| py::round(f64::from(b[i]) + ((1.0 - cg) * aw) * (t[i] - g[i]) as f64));
            let v: [u8; 3] = [0, 1, 2].map(|i| {
                let r = py::round(v[i] as f64 * (1.0 - a) + c[i] as f64 * a).clamp(0, 255);
                u8::try_from(r).expect("clamped to a byte")
            });
            out.px[(y * n + x) * 3..(y * n + x) * 3 + 3].copy_from_slice(&v);
            peak = py::max(py::max(peak, aw), a);
            lit += usize::from(v != [b[0], b[1], b[2]]);
        }
    }
    Ok((out, peak, lit))
}

/// The control: with both lights at zero, every frame the bare ground,
/// the same blend draws the SVG's drawing, every pixel; and the lights lit
/// something.
///
/// # Errors
/// When the control moved a pixel or nothing was lit.
pub fn lights_hold(base: &Image, zero: &Image, moved: usize, lit: usize) -> Result<(), Fail> {
    if moved != 0 || zero.px != base.px {
        fail!(
            "with no light the organisation picture is not its SVG's drawing: {moved} pixels differ"
        );
    }
    if lit == 0 {
        fail!("the plasma lit no pixel of the organisation picture");
    }
    Ok(())
}

/// Frames of bare ground for the control.
///
/// # Errors
/// When the files cannot be written.
pub fn dark_frames(dir: &Path, f: &Frames) -> Result<(), Fail> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let g = rgb(GROUND);
    let px = [g[2], g[1], g[0], 0xFF].map(|v| u8::try_from(v).expect("a byte"));
    for (name, (w, h)) in [
        ("word", f.word),
        ("mark-base", f.mark),
        ("mark-trail", f.mark),
    ] {
        std::fs::write(dir.join(format!("{name}.raw")), px.repeat(w * h))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------- the run

/// An outline source holds paths and shapes alone and names no font.
///
/// # Errors
/// When it holds another element or names a font.
pub fn outlines_only(name: &str, text: &str) -> Result<(), Fail> {
    let allowed = ["svg", "g", "path", "rect", "circle"];
    // re.sub(r"<!--.*?-->", "", text, flags=re.S)
    let mut bare = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("<!--") {
        match rest[i + 4..].find("-->") {
            Some(j) => {
                bare.push_str(&rest[..i]);
                rest = &rest[i + 4 + j + 3..];
            }
            None => break,
        }
    }
    bare.push_str(rest);
    // re.findall(r"<([A-Za-z][\w:-]*)", bare)
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == ':' || c == '-';
    let mut tags: Vec<String> = Vec::new();
    for (i, _) in bare.match_indices('<') {
        let after = &bare[i + 1..];
        if after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            let end = after.find(|c: char| !word(c)).unwrap_or(after.len());
            tags.push(after[..end].to_string());
        }
    }
    tags.sort();
    tags.dedup();
    if tags.iter().any(|t| !allowed.contains(&t.as_str())) || !tags.iter().any(|t| t == "path") {
        fail!(
            "{name}: elements {}: an outline source holds paths and shapes alone",
            py::repr_list(&tags)
        );
    }
    if names_a_font(&bare) {
        fail!("{name}: names a font");
    }
    Ok(())
}

/// `re.search(r"font(-[a-z]+)?\s*[=:]", bare, re.I)`.
fn names_a_font(bare: &str) -> bool {
    let low = bare.to_ascii_lowercase();
    let after_space = |s: &str| {
        let s = s.trim_start_matches(py::is_space);
        s.starts_with('=') || s.starts_with(':')
    };
    low.match_indices("font").any(|(i, _)| {
        let rest = &low[i + 4..];
        if after_space(rest) {
            return true;
        }
        if let Some(r) = rest.strip_prefix('-') {
            let n = r.find(|c: char| !c.is_ascii_lowercase()).unwrap_or(r.len());
            return n > 0 && after_space(&r[n..]);
        }
        false
    })
}

/// The README's eighth line is the card's tagline.
///
/// # Errors
/// When it is not.
pub fn tagline_holds(readme: &str) -> Result<(), Fail> {
    let lines = py::splitlines(readme);
    let want = format!("**{}**", tagline());
    match lines.get(9) {
        Some(l) if *l == want => Ok(()),
        Some(l) => fail!(
            "README.md line 10 is not the card's tagline: {}",
            py::repr_str(l)
        ),
        None => fail!("README.md has no line 10"),
    }
}

/// The release archive: `BRAND_ARCHIVE`, or fetched into `tmp`.
///
/// # Errors
/// When it cannot be read or fetched.
pub fn archive(tmp: &Path) -> Result<(String, Vec<u8>), Fail> {
    let path = match std::env::var("BRAND_ARCHIVE") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            let p = tmp.join("JetBrainsMono-2.304.zip");
            let ok = std::process::Command::new("curl")
                .args(["-fsSL", "-o"])
                .arg(&p)
                .arg(ARCHIVE_URL)
                .status()
                .map_err(|e| format!("curl did not run: {e}"))?;
            if !ok.success() {
                fail!("curl could not fetch {ARCHIVE_URL}");
            }
            p.to_string_lossy().into_owned()
        }
    };
    let b = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    Ok((path, b))
}

/// Draw every file, in memory.
///
/// # Errors
/// When any check refuses.
pub fn draw(root: &Path, tmp: &Path) -> Result<Files, Fail> {
    let readme =
        std::fs::read_to_string(root.join("README.md")).map_err(|e| format!("README.md: {e}"))?;
    tagline_holds(&readme)?;
    let fonts = fonts(root, &archive(tmp)?)?;
    let t = Type::new(&fonts)?;
    let org = organisation(&t)?;
    let p = plasma(&t, root, tmp, org.pen, org.s)?;
    let mut text = logo_svgs(&t)?;
    text.push((format!("{}.svg", ORG.0), org.svg.clone()));
    text.push((format!("{}.svg", CARD.0), card(&t)?));
    for (name, body) in &text {
        outlines_only(name, body)?;
    }
    let mut out: Files = text
        .iter()
        .map(|(n, b)| (n.clone(), b.clone().into_bytes()))
        .collect();

    let dock = get(&text, &format!("{DOCK}.svg"));
    for size in DOCK_SIZES {
        let im = crate::raster::draw(dock, Some(size))?;
        if (im.w, im.h) != (size as usize, size as usize) {
            fail!("the Dock icon drew ({}, {}) at {size}", im.w, im.h);
        }
        out.push((format!("{DOCK}-{size}.png"), png::encode(&im)));
    }
    let card = opaque(
        &crate::raster::draw(get(&text, &format!("{}.svg", CARD.0)), None)?,
        "social",
    )?;
    let card_png = png::encode(&card);
    tape_reads(&png::decode(&card_png)?)?;
    out.push((format!("{}.png", CARD.0), card_png));

    let base = opaque(&crate::raster::draw(&org.svg, None)?, "org")?;
    let f = frames(&p.rep)?;
    let (pic, peak, lit) = org_picture(&base, &f, &p.frames, org.cells)?;
    let dark = tmp.join("dark");
    dark_frames(&dark, &f)?;
    let (zero, _, moved) = org_picture(&base, &f, &dark, org.cells)?;
    lights_hold(&base, &zero, moved, lit)?;
    out.push((format!("{}.png", ORG.0), png::encode(&pic)));
    let (w, m) = (p.rep.get("word")?, p.rep.get("mark")?);
    println!(
        "plasma: the word's light is the splash's first pass at {} s, seed {}, its box {} and hole {} at scale {} from {}, the splash's own over {} columns; the mark's is the cursor's trail at {} s, seed {}, at {} px; {lit} pixels lit, peak {peak:.3}; with no light, the SVG's drawing to the pixel",
        w.get("t")?.py(),
        w.get("seed")?.py(),
        w.get("box")?.py(),
        w.get("hole")?.py(),
        w.get("scale")?.py(),
        w.get("origin")?.py(),
        w.get("gate_columns")?.py(),
        m.get("age")?.py(),
        m.get("seed")?.py(),
        m.get("px")?.py(),
    );
    Ok(out)
}

/// The logo files on disk, by their paths from `root`: every SVG and PNG
/// in `assets/logo/`, and the card's two.
#[must_use]
pub fn there(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root.join(LOGO))
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                // glob's `*` does not match a leading dot
                .filter(|n| !n.starts_with('.'))
                .filter(|n| {
                    Path::new(n)
                        .extension()
                        .is_some_and(|e| e == "svg" || e == "png")
                })
                .map(|n| format!("{LOGO}{n}"))
                .collect()
        })
        .unwrap_or_default();
    v.push(format!("{}.svg", CARD.0));
    v.push(format!("{}.png", CARD.0));
    v.sort();
    v.dedup();
    v
}

/// The files on disk the run does not draw.
#[must_use]
pub fn undrawn(there: &[String], out: &Files) -> Vec<String> {
    there
        .iter()
        .filter(|n| !out.iter().any(|(o, _)| o == *n))
        .map(|n| format!("{n} is a logo file the generator does not draw"))
        .collect()
}

/// The whole run: draw, then write or compare, then hold both GitHub
/// pictures to GitHub's limits. Returns the failures.
///
/// # Errors
/// When a check refuses before anything is compared.
pub fn run(root: &Path, check: bool) -> Result<Vec<String>, Fail> {
    let tmp = std::env::temp_dir().join(format!("baud-pictures-brand-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let drawn = draw(root, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    let out = drawn?;
    let mut bad = undrawn(&there(root), &out);
    for (name, b) in &out {
        let at = root.join(name);
        if check {
            let same = std::fs::read(&at).is_ok_and(|have| have == *b);
            let digest = sha256::hex(b);
            println!(
                "{} {name} ({} bytes, sha256 {})",
                if same { "same   " } else { "DIFFERS" },
                b.len(),
                &digest[..16]
            );
            if !same {
                bad.push(format!("{name} is not what the generator draws"));
            }
        } else {
            std::fs::write(&at, b).map_err(|e| format!("{name}: {e}"))?;
            println!("wrote {name} ({} bytes)", b.len());
        }
    }
    for (base, square) in [(ORG.0, true), (CARD.0, false)] {
        let name = format!("{base}.png");
        let b = std::fs::read(root.join(&name)).unwrap_or_default();
        bad.extend(github_limits(&name, &b, square));
    }
    Ok(bad)
}
