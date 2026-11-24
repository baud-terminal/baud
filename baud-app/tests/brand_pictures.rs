//! **The logo files: GitHub's limits, outlines in place of text, and the
//! mark as baud draws it.**
//!
//! `baud-pictures brand` (`tools/baud-pictures`) draws every file in
//! `assets/logo/` and the repository's link picture from one mark, baud's
//! own drawing of "❯" with the cursor in the next cell, in JetBrains
//! Mono's own outlines, and its `--check` holds the committed files to its
//! output byte for byte.
//! That check needs the network (the 500 weight is read out of the font's
//! release archive) and the pinned nixpkgs, so it runs by hand. This file
//! needs neither and runs with the suite: each GitHub picture within
//! GitHub's limits and opaque by construction; every logo SVG made of
//! paths and shapes, naming no font a renderer could fall back from; and
//! every logo's ❯ and cursor where baud's renderer draws them. Before the
//! generator, every lockup and icon named the font, so each viewer drew the
//! ❯ from whatever face it had: JetBrains Mono's rode high beside the
//! cursor, a Mac's fallback stood wider and centred.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Argb, Blend, Blink, Damage, PxSize, Renderer, Stack, Target, Theme};

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// GitHub's "under 1 MB" and "less than 1 MB", read as the smaller of the
/// two meanings of a megabyte.
const MB: usize = 1_000_000;

/// A PNG's header and the kinds of its chunks, in order.
struct Png {
    width: u32,
    height: u32,
    depth: u8,
    colour: u8,
    chunks: Vec<String>,
    bytes: usize,
}

fn parse(b: &[u8]) -> Option<Png> {
    if b.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut at = 8;
    let mut chunks = Vec::new();
    let mut header = None;
    while at + 8 <= b.len() {
        let len = u32::from_be_bytes(b[at..at + 4].try_into().ok()?) as usize;
        let kind = String::from_utf8_lossy(&b[at + 4..at + 8]).into_owned();
        if kind == "IHDR" {
            let d = b.get(at + 8..at + 18)?;
            header = Some((
                u32::from_be_bytes(d[0..4].try_into().ok()?),
                u32::from_be_bytes(d[4..8].try_into().ok()?),
                d[8],
                d[9],
            ));
        }
        chunks.push(kind);
        at += 12 + len;
    }
    let (width, height, depth, colour) = header?;
    Some(Png {
        width,
        height,
        depth,
        colour,
        chunks,
        bytes: b.len(),
    })
}

/// What keeps a PNG from going up as a GitHub picture of `want` pixels:
/// the size limits of GitHub's help pages, and opacity by construction --
/// 8-bit RGB, which has no alpha channel, and no chunk but IHDR, IDAT and
/// IEND, so neither a tRNS nor a text or metadata chunk is carried.
fn faults(p: &Png, want: (u32, u32)) -> Vec<String> {
    let mut out = Vec::new();
    if p.bytes >= MB {
        out.push(format!("{} bytes, not under 1 MB", p.bytes));
    }
    if (p.width, p.height) != want {
        out.push(format!(
            "{}x{}, not {}x{}",
            p.width, p.height, want.0, want.1
        ));
    }
    if p.width >= 3000 || p.height >= 3000 {
        out.push("not smaller than 3000 by 3000".to_owned());
    }
    if (p.depth, p.colour) != (8, 2) {
        out.push(format!(
            "bit depth {}, colour type {}: not 8-bit RGB",
            p.depth, p.colour
        ));
    }
    let extra: BTreeSet<_> = p
        .chunks
        .iter()
        .filter(|k| !matches!(k.as_str(), "IHDR" | "IDAT" | "IEND"))
        .collect();
    if !extra.is_empty() || p.chunks.first().map(String::as_str) != Some("IHDR") {
        out.push(format!("chunks besides IHDR, IDAT and IEND: {extra:?}"));
    }
    out
}

/// The organisation picture, at the 500 by 500 GitHub recommends for a
/// profile picture ("smaller than 3000 by 3000 ... about 500 by 500";
/// GitHub states nothing for an organisation, so the profile's limits are
/// applied), and the link picture at "1280 by 640 pixels for best display".
#[test]
fn the_github_pictures_meet_githubs_limits() {
    let mut bad = Vec::new();
    for (rel, want) in [
        ("assets/logo/baud-github-org.png", (500, 500)),
        ("assets/readme/social.png", (1280, 640)),
    ] {
        let b = fs::read(workspace().join(rel)).unwrap_or_default();
        match parse(&b) {
            None => bad.push(format!("{rel}: not a PNG")),
            Some(p) => bad.extend(faults(&p, want).into_iter().map(|f| format!("{rel}: {f}"))),
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");

    // The control: an RGBA header with a text chunk is refused by the same
    // check, once for each.
    let mut rgba = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    rgba.extend(500u32.to_be_bytes());
    rgba.extend(500u32.to_be_bytes());
    rgba.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    rgba.extend(b"\0\0\0\0tEXt\0\0\0\0\0\0\0\0IEND\0\0\0\0");
    let p = parse(&rgba).expect("the control parses");
    assert_eq!(
        faults(&p, (500, 500)).len(),
        2,
        "the control is refused twice"
    );
}

/// What keeps a PNG from being the Dock icon's drawing at `want`: its
/// size, 8-bit RGBA (colour type 6: the icon's padding is transparent),
/// and no chunk but IHDR, IDAT and IEND.
fn dock_faults(p: &Png, want: (u32, u32)) -> Vec<String> {
    let mut out = Vec::new();
    if (p.width, p.height) != want {
        out.push(format!(
            "{}x{}, not {}x{}",
            p.width, p.height, want.0, want.1
        ));
    }
    if (p.depth, p.colour) != (8, 6) {
        out.push(format!(
            "bit depth {}, colour type {}: not 8-bit RGBA",
            p.depth, p.colour
        ));
    }
    if p.chunks
        .iter()
        .any(|k| !matches!(k.as_str(), "IHDR" | "IDAT" | "IEND"))
    {
        out.push(format!("chunks {:?}", p.chunks));
    }
    out
}

/// **The Dock icon at every size the bundle packs**: each size
/// `scripts/bundle-macos.sh` copies into its iconset has its
/// `assets/logo/baud-icon-dock-<N>.png`, N by N, RGBA, with nothing but
/// the image in it. The generator draws them from the Dock icon's SVG
/// with the pinned resvg, and its `--check` holds their bytes, by hand.
#[test]
fn the_dock_iconset_is_there_at_every_size_the_bundle_packs() {
    let script =
        fs::read_to_string(workspace().join("scripts/bundle-macos.sh")).expect("the script");
    let sizes: BTreeSet<u32> = script
        .split_whitespace()
        .filter_map(|w| w.split_once(":icon_")?.0.parse().ok())
        .collect();
    assert_eq!(sizes.len(), 7, "the iconset's sizes, 16 to 1024: {sizes:?}");
    let mut bad = Vec::new();
    for n in &sizes {
        let rel = format!("assets/logo/baud-icon-dock-{n}.png");
        let b = fs::read(workspace().join(&rel)).unwrap_or_default();
        match parse(&b) {
            None => bad.push(format!("{rel}: not a PNG")),
            Some(p) => bad.extend(
                dock_faults(&p, (*n, *n))
                    .into_iter()
                    .map(|f| format!("{rel}: {f}")),
            ),
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");

    // The control: the 16 px drawing offered as the 32 is refused.
    let b = fs::read(workspace().join("assets/logo/baud-icon-dock-16.png")).expect("the 16");
    let p = parse(&b).expect("a PNG");
    assert_eq!(dock_faults(&p, (32, 32)).len(), 1, "the control is refused");
}

/// The element names in an SVG, comments left out.
fn elements(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        if let Some(after) = rest.strip_prefix("!--") {
            rest = after.split_once("-->").map_or("", |(_, r)| r);
            continue;
        }
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == ':' || *c == '-')
            .collect();
        if !name.is_empty() {
            out.insert(name);
        }
    }
    out
}

/// The logo SVGs: every one in `assets/logo/`, the three the app embeds
/// through `baud-app/assets/logo/` (links into it), and the link
/// picture's source.
fn logo_svgs() -> Vec<String> {
    let root = workspace();
    let mut out: Vec<String> = ["assets/logo", "baud-app/assets/logo"]
        .iter()
        .flat_map(|dir| fs::read_dir(root.join(dir)).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "svg"))
        .map(|p| rel(&root, &p))
        .collect();
    out.push("assets/readme/social.svg".to_owned());
    out.sort();
    out
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// `text` with its comments left out.
fn bare(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        rest = rest[i..].split_once("-->").map_or("", |(_, r)| r);
    }
    out.push_str(rest);
    out
}

/// Whether `text`, comments left out, names a font in an attribute or a
/// style: `font-family="..."`, `font: ...` and the like.
fn names_a_font(text: &str) -> bool {
    let text = bare(text).to_ascii_lowercase();
    text.match_indices("font").any(|(i, _)| {
        let after = text[i + 4..].trim_start_matches(|c: char| c.is_ascii_lowercase() || c == '-');
        matches!(after.trim_start().chars().next(), Some('=' | ':'))
    })
}

/// What is wrong with an SVG as an outline source, or nothing.
fn outline_faults(text: &str) -> Vec<String> {
    let allowed: BTreeSet<String> = ["svg", "g", "path", "rect", "circle"]
        .map(String::from)
        .into();
    let found = elements(text);
    let mut out = Vec::new();
    if !found.contains("path") || !found.is_subset(&allowed) {
        out.push(format!("elements {found:?}"));
    }
    if names_a_font(text) {
        out.push("names a font".to_owned());
    }
    out
}

/// Every logo SVG is paths and shapes alone, the ❯ and every letter an
/// outline, and names no font.
#[test]
fn every_logo_is_outlines_and_names_no_font() {
    let files = logo_svgs();
    assert!(files.len() > 20, "the logo files: {files:?}");
    let mut bad = Vec::new();
    for rel in &files {
        let text = fs::read_to_string(workspace().join(rel)).unwrap_or_default();
        for f in outline_faults(&text) {
            bad.push(format!("{rel}: {f}"));
        }
    }
    assert!(bad.is_empty(), "not outlines alone: {bad:#?}");

    // The controls: the lockup as the files spelled it before, text naming
    // the font, is refused on both counts; a font named only in a comment,
    // or a label that says "font", is not.
    let text =
        r#"<svg><text x="192" y="85" font-size="54" font-family="JetBrains Mono">❯</text></svg>"#;
    assert_eq!(outline_faults(text).len(), 2, "{:?}", outline_faults(text));
    let ok = r#"<svg aria-label="its own font,"><!-- JetBrains Mono, font-family --><path d="M0 0Z"/></svg>"#;
    assert!(outline_faults(ok).is_empty(), "{:?}", outline_faults(ok));
}

// ---------------------------------------------------------------- the mark

/// baud's cell at 200 px, as `tools/baud-pictures` takes it:
/// width, height and baseline.
const CELL: (u32, u32, u32) = (120, 264, 204);
const PX: f32 = 200.0;

/// **How close a file's mark must be to baud's drawing**: half a pixel of
/// the 200 px drawing, in the cell's units -- 1/528 of the cell's height,
/// 1/240 of its width. The renderer's ❯ and the font's outline drawn at
/// the same size differ by 0.06 px; a file's two-decimal coordinates move
/// its smallest mark, the 32 px icon's, by at most 0.25 px of this scale.
const TOLERANCE_PX: f64 = 0.5;

/// A mark's measures in its cells' units: the ❯'s ink top and bottom from
/// the cell's top over its height, the ❯'s centroid from the ❯'s cell's
/// top left over the cell's width and height, and the cursor's width over
/// its height.
#[derive(Debug, Clone, Copy)]
struct Mark {
    top: f64,
    bottom: f64,
    centroid: (f64, f64),
    aspect: f64,
}

impl Mark {
    /// The measures that differ from `want`'s by more than the tolerance.
    fn off(&self, want: &Mark) -> Vec<String> {
        let (w, h) = (f64::from(CELL.0), f64::from(CELL.1));
        let tol = TOLERANCE_PX;
        let mut out = Vec::new();
        for (name, have, want, scale) in [
            ("ink top", self.top, want.top, h),
            ("ink bottom", self.bottom, want.bottom, h),
            (
                "ink height",
                self.bottom - self.top,
                want.bottom - want.top,
                h,
            ),
            (
                "centre on the cursor's",
                (self.top + self.bottom) / 2.0 - 0.5,
                (want.top + want.bottom) / 2.0 - 0.5,
                h,
            ),
            ("centroid x", self.centroid.0, want.centroid.0, w),
            ("centroid y", self.centroid.1, want.centroid.1, h),
            ("cursor aspect", self.aspect, want.aspect, h),
        ] {
            let px = (have - want) * scale;
            if px.abs() > tol {
                out.push(format!("{name}: {px:+.3} px at 200 px"));
            }
        }
        out
    }
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// **baud's own drawing of the mark**: "❯" in JetBrains Mono Bold (the
/// built face the app embeds) at 200 px, a steady block cursor in the next
/// cell, white on black through the CPU path -- the block the cell's
/// colours swapped, not baud's amber -- so a pixel's coverage is its level
/// over 255. Measured from its pixels: the ❯'s level top and bottom edges
/// by the coverage of their rows, its centroid by coverage.
fn rendered() -> Mark {
    let ws = WinSize {
        ws_row: 3,
        ws_col: 5,
    };
    let bold = fs::read(workspace().join("baud-app/fonts/built/Bold.ttf")).expect("Bold.ttf");
    let faces = [Face::parse(&bold).expect("parse")];
    let mut theme = Theme::dark();
    theme.bg = Argb(0xFF00_0000);
    theme.fg = Argb(0xFFFF_FFFF);
    theme.cursor = None;
    theme.cursor_text = None;
    theme.cursor_made = baud_render::CursorMade::Named;
    let mut r = Renderer::new(
        Stack::new(&faces).expect("a stack"),
        PxSize::of(PX).expect("a size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .expect("a renderer");
    let m = r.metrics();
    assert_eq!(
        (m.cell_w.value(), m.cell_h.value(), m.baseline.value()),
        CELL,
        "baud's cell at 200 px is the generator's"
    );
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, "\x1b[2 q\x1b[2;2H\u{276F}".as_bytes());
    r.set_cursor_blink(Blink::Shown);
    let (pw, ph) = r.pixel_size();
    let width = usize::try_from(pw.value()).expect("a width");
    let mut px = vec![0u32; width * usize::try_from(ph.value()).expect("a height")];
    let mut target = Target::new(&mut px, pw, pw, ph).expect("a target");
    let seal = t.seal_writes();
    r.render(&t, seal, &mut target, Damage::Full);
    let (cw, ch) = (
        usize::try_from(CELL.0).expect("w"),
        usize::try_from(CELL.1).expect("h"),
    );
    let cover = |x: usize, y: usize| f64::from((px[y * width + x] >> 16) & 0xFF) / 255.0;
    // The cursor is the next cell, whole, and nothing past it.
    for y in ch..2 * ch {
        assert!(
            (2 * cw..3 * cw).all(|x| cover(x, y) == 1.0),
            "the cursor fills its cell"
        );
        assert!(
            (3 * cw..4 * cw).all(|x| cover(x, y) == 0.0),
            "and only its cell"
        );
    }
    let rows: Vec<(usize, f64)> = (ch..2 * ch)
        .map(|y| (y, (cw..2 * cw).map(|x| cover(x, y)).fold(0.0, f64::max)))
        .filter(|&(_, c)| c > 0.0)
        .collect();
    let (&(first, top_cover), &(last, bottom_cover)) =
        (rows.first().expect("ink"), rows.last().expect("ink"));
    let at = |v: usize| f64::from(u32::try_from(v).expect("a coordinate"));
    let (mut sum, mut sx, mut sy) = (0.0, 0.0, 0.0);
    for y in ch..2 * ch {
        for x in cw..2 * cw {
            let c = cover(x, y);
            sum += c;
            sx += c * (at(x) + 0.5);
            sy += c * (at(y) + 0.5);
        }
    }
    let (w, h) = (f64::from(CELL.0), f64::from(CELL.1));
    Mark {
        top: (at(first) + 1.0 - top_cover - h) / h,
        bottom: (at(last) + bottom_cover - h) / h,
        centroid: ((sx / sum - w) / w, (sy / sum - h) / h),
        aspect: w / h,
    }
}

/// The value of `name="..."` in an element's text.
fn attr(el: &str, name: &str) -> Option<f64> {
    let at = el.find(&format!(" {name}=\""))? + name.len() + 3;
    el[at..].split('"').next()?.parse().ok()
}

/// A path's vertices, if it is drawn with straight lines alone (`M`, `L`,
/// `H`, `V`, `Z`, absolute): the ❯'s outline is, and a letter's is not.
fn polygon(d: &str) -> Option<Vec<(f64, f64)>> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() || c == ' ' || c == ',' || (c == '-' && !cur.is_empty()) {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            if c.is_ascii_alphabetic() {
                toks.push(c.to_string());
            } else if c == '-' {
                cur.push(c);
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    let mut it = toks.iter();
    let (mut pts, mut p) = (Vec::new(), (0.0, 0.0));
    let num = |it: &mut std::slice::Iter<'_, String>| it.next()?.parse::<f64>().ok();
    while let Some(cmd) = it.next() {
        match cmd.as_str() {
            "M" | "L" => p = (num(&mut it)?, num(&mut it)?),
            "H" => p.0 = num(&mut it)?,
            "V" => p.1 = num(&mut it)?,
            "Z" => continue,
            _ => return None,
        }
        pts.push(p);
    }
    Some(pts)
}

/// A polygon's centroid, by the shoelace formula.
fn centroid(pts: &[(f64, f64)]) -> (f64, f64) {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for (i, &(x0, y0)) in pts.iter().enumerate() {
        let (x1, y1) = pts[(i + 1) % pts.len()];
        let k = x0 * y1 - x1 * y0;
        a += k;
        cx += (x0 + x1) * k;
        cy += (y0 + y1) * k;
    }
    (cx / (3.0 * a), cy / (3.0 * a))
}

/// Every mark in an SVG: a straight-lined path followed by a rect of its
/// fill, the ❯ and its cursor, measured in the cursor's cell's units.
fn marks(text: &str) -> Vec<Mark> {
    placed(text)
        .into_iter()
        .map(|(pts, (x, y, w, h))| {
            let top = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let bottom = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            let (cx, cy) = centroid(&pts);
            Mark {
                top: (top - y) / h,
                bottom: (bottom - y) / h,
                centroid: ((cx - (x - w)) / w, (cy - y) / h),
                aspect: w / h,
            }
        })
        .collect()
}

/// A mark where its file draws it: the ❯'s vertices, and its cursor's x,
/// y, width and height, in the file's own units.
type Placed = (Vec<(f64, f64)>, (f64, f64, f64, f64));

/// Every mark in an SVG, as [`marks`] finds them, where each is drawn.
fn placed(text: &str) -> Vec<Placed> {
    let text = bare(text);
    let els: Vec<&str> = text.split('<').skip(1).collect();
    els.windows(2)
        .filter_map(|w| {
            let (path, rect) = (w[0], w[1]);
            let fill = |el: &str| {
                el.split(" fill=\"")
                    .nth(1)?
                    .split('"')
                    .next()
                    .map(str::to_owned)
            };
            if !path.starts_with("path ")
                || !rect.starts_with("rect ")
                || fill(path)? != fill(rect)?
            {
                return None;
            }
            let d = path.split(" d=\"").nth(1)?.split('"').next()?;
            let pts = polygon(d)?;
            let (x, y) = (attr(rect, "x")?, attr(rect, "y")?);
            let (w, h) = (attr(rect, "width")?, attr(rect, "height")?);
            Some((pts, (x, y, w, h)))
        })
        .collect()
}

/// **Every logo's mark is baud's drawing of "❯" with the cursor in the
/// next cell**, as a baud window shows the prompt and cursor: in each
/// file, the ❯'s ink height, its
/// centre against the cursor's and its centroid in its cell, and the
/// cursor's proportions, within [`TOLERANCE_PX`] of the renderer's own.
#[test]
fn every_logo_mark_is_baud_drawing_the_prompt_and_cursor() {
    let want = rendered();
    // The renderer's ❯, by the font: ink 730 units tall from 204 - 0.73 of
    // the 200 px size, in a 264 cell.
    assert!((want.top - 58.0 / 264.0).abs() < 0.5 / 264.0, "{want:?}");
    assert!(
        (want.bottom - 204.0 / 264.0).abs() < 0.5 / 264.0,
        "{want:?}"
    );
    let mut bad = Vec::new();
    for rel in logo_svgs() {
        let text = fs::read_to_string(workspace().join(&rel)).unwrap_or_default();
        let found = marks(&text);
        if found.len() != 1 {
            bad.push(format!("{rel}: {} marks, not one", found.len()));
        }
        for m in found {
            for f in m.off(&want) {
                bad.push(format!("{rel}: {f}"));
            }
        }
    }
    assert!(bad.is_empty(), "not baud's mark: {bad:#?}");

    // The control: the Dock icon's mark as it was, the font's ❯ at x 130
    // and the cursor at 306, is refused.
    let before = r##"<path d="M193 314H159.4L186.4 241L159.4 168H193L220 241.4Z" fill="#F4A32C"/>
    <rect x="306" y="172" width="76" height="168" fill="#F4A32C"/>"##;
    let m = marks(before);
    assert_eq!(m.len(), 1, "the control is found");
    assert!(!m[0].off(&want).is_empty(), "and refused");
}

// ---------------------------------------------------------------- the Dock

/// The 512 icons' cell: 168 tall, as their cursor was, so one of its
/// cells is 168 * 120 / 264 wide, 76.36 units.
const ICON_CELL_H: f64 = 168.0;

/// How far the Dock's mark is scaled up from the 512 icons' cell.
const DOCK_SCALE: f64 = 1.2;

/// What keeps an SVG's mark from being the Dock icon's: drawn on the
/// 512 tile in the tile's units, the ❯'s cell corner one of the 512
/// icons' cell widths in from the tile's left and top, the cell
/// [`DOCK_SCALE`] times their height, and the ink's centre half such a
/// cell width or more inside the tile's upper-left quadrant. Each
/// coordinate is written to two decimals, and so is each width, so a
/// corner taken as x less width is good to 0.01.
fn dock_mark_faults(text: &str) -> Vec<String> {
    let tile = 512.0;
    let cell_w = ICON_CELL_H * f64::from(CELL.0) / f64::from(CELL.1);
    let mut out = Vec::new();
    if bare(text)
        .matches(r#"<rect width="512" height="512" rx="115""#)
        .count()
        != 1
    {
        out.push("no 512 tile".to_owned());
    }
    let found = placed(text);
    let [(pts, (x, y, w, h))] = found.as_slice() else {
        out.push(format!("{} marks, not one", found.len()));
        return out;
    };
    let (cx, cy) = (x - w, *y);
    if (cx - cell_w).abs() > 0.011 || (cy - cell_w).abs() > 0.011 {
        out.push(format!(
            "the ❯'s cell corner at ({cx:.2}, {cy:.2}), not ({cell_w:.2}, {cell_w:.2})"
        ));
    }
    let want = ICON_CELL_H * DOCK_SCALE;
    if (h - want).abs() > 0.011 {
        out.push(format!(
            "the cell {h:.2} tall, {:.3} of the icons', not {DOCK_SCALE}",
            h / ICON_CELL_H
        ));
    }
    let left = pts.iter().map(|p| p.0).fold(*x, f64::min);
    let top = pts.iter().map(|p| p.1).fold(*y, f64::min);
    let bottom = pts.iter().map(|p| p.1).fold(y + h, f64::max);
    let centre = ((left + x + w) / 2.0, (top + bottom) / 2.0);
    let inside = tile / 2.0 - cell_w / 2.0;
    if centre.0 > inside || centre.1 > inside {
        out.push(format!(
            "the ink's centre at ({:.2}, {:.2}), not within ({inside:.2}, {inside:.2})",
            centre.0, centre.1
        ));
    }
    out
}

/// **The Dock icon's mark is a prompt at the start of a terminal's first
/// line**: in the tile's upper left, in a terminal's own margin, and
/// [`DOCK_SCALE`] times the 512 icons' mark, big enough to read at the
/// Dock's 32 pt. Held in both the file the generator writes and the one
/// `baud-app/src/app.rs` embeds for a run outside a bundle, which is a
/// link to it.
#[test]
fn the_dock_mark_is_a_prompt_in_the_tiles_upper_left() {
    let mut bad = Vec::new();
    let mut texts = Vec::new();
    for rel in [
        "assets/logo/baud-icon-dock.svg",
        "baud-app/assets/logo/baud-icon-dock.svg",
    ] {
        let text = fs::read_to_string(workspace().join(rel)).unwrap_or_default();
        bad.extend(
            dock_mark_faults(&text)
                .into_iter()
                .map(|f| format!("{rel}: {f}")),
        );
        texts.push(text);
    }
    assert!(bad.is_empty(), "not the Dock's mark: {bad:#?}");
    assert_eq!(texts[0], texts[1], "the app embeds the generator's file");

    // The controls, each the generator's own drawing: the Dock's mark
    // centred at the 512 icons' size, as it was, is refused on all three
    // counts; in the upper left at 1.0 and at 1.3 times, on the scale
    // alone.
    let tile = r##"<rect width="512" height="512" rx="115" fill="#15171B"/>"##;
    let centred = format!(
        "{tile}{}",
        r##"<path d="M210.37 301.82H188.99L206.17 255.36L188.99 208.91H210.37L227.55 255.62Z" fill="#F4A32C"/>
    <rect x="246.64" y="172" width="76.37" height="168" fill="#F4A32C"/>"##
    );
    assert_eq!(
        dock_mark_faults(&centred).len(),
        3,
        "{:?}",
        dock_mark_faults(&centred)
    );
    for (mark, scale) in [
        (
            r##"<path d="M116.45 206.18H95.07L112.25 159.72L95.07 113.27H116.45L133.64 159.98Z" fill="#F4A32C"/>
    <rect x="152.73" y="76.36" width="76.36" height="168" fill="#F4A32C"/>"##,
            "1.000",
        ),
        (
            r##"<path d="M128.48 245.12H100.69L123.02 184.73L100.69 124.34H128.48L150.82 185.06Z" fill="#F4A32C"/>
    <rect x="175.64" y="76.36" width="99.27" height="218.4" fill="#F4A32C"/>"##,
            "1.300",
        ),
    ] {
        let f = dock_mark_faults(&format!("{tile}{mark}"));
        assert!(
            f.len() == 1 && f[0].contains(&format!("{scale} of the icons'")),
            "{f:?}"
        );
    }
}

// ---------------------------------------------------------------- the title bar

/// **How far the block's top may sit from the word's top, and how far the
/// ❯-to-block gap from [`GAP_OF_CELL`]**, in the units of the file: the two
/// decimals each number is written to.
const LINED_UP: f64 = 0.02;

/// **The ink gap from the ❯ to the block, as a share of the cell's height**:
/// baud's own drawing of them, 1.43 pt in the title bar, which lining the
/// mark up with the word leaves as it was.
const GAP_OF_CELL: f64 = 0.1134;

/// Every lockup the generator draws, the card's among them, with the index
/// of its block among the file's rects.
const LOCKUPS: [(&str, usize); 9] = [
    ("assets/logo/baud-lockup-dark.svg", 1),
    ("assets/logo/baud-lockup-dark-blinking.svg", 1),
    ("assets/logo/baud-lockup-light.svg", 0),
    ("assets/logo/baud-lockup-light-blinking.svg", 0),
    ("assets/logo/baud-lockup-mono-black.svg", 0),
    ("assets/logo/baud-lockup-mono-white.svg", 0),
    ("assets/logo/baud-lockup-dark-tight.svg", 0),
    ("assets/logo/baud-lockup-light-tight.svg", 0),
    ("assets/readme/social.svg", 1),
];

/// What keeps a lockup's block from standing on the word's top with the
/// gap as it was, measured off the file: its first path the word, its
/// second the ❯, and its `block`th rect the block.
fn lined_up_faults(text: &str, block: usize) -> Vec<String> {
    let (paths, rects) = drawn(text);
    let (Some((word, _)), Some((prompt, _)), Some((b, _))) =
        (paths.first(), paths.get(1), rects.get(block))
    else {
        return vec!["no word, ❯ and block".to_owned()];
    };
    let axis = |d: &str, x: bool| -> Vec<f64> {
        let (_, nums, axes) = tokens(d);
        nums.iter()
            .zip(&axes)
            .filter(|(_, a)| **a == x)
            .map(|(n, _)| *n)
            .collect()
    };
    let top = axis(word, false).into_iter().fold(f64::INFINITY, f64::min);
    let right = axis(prompt, true)
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);
    let mut out = Vec::new();
    if (top - b[1]).abs() > LINED_UP {
        out.push(format!(
            "the block's top {} is not on the word's top {top}",
            b[1]
        ));
    }
    let gap = b[0] - right;
    if (gap - GAP_OF_CELL * b[3]).abs() > LINED_UP.max(0.0005 * b[3]) {
        out.push(format!(
            "the gap {gap} is not {GAP_OF_CELL} of the cell's {}",
            b[3]
        ));
    }
    out
}

/// **The block's top sits on the word's top, and the gap stays**, on every
/// lockup as the generator wrote it. The controls: the block where it stood
/// before the two were lined up, 2.26 units lower in the 300 lockup, and the
/// ❯ a unit to the left, are each refused.
#[test]
fn every_lockups_block_stands_on_the_words_top() {
    let read = |rel: &str| fs::read_to_string(workspace().join(rel)).expect(rel);
    let mut bad = Vec::new();
    for (name, block) in LOCKUPS {
        bad.extend(
            lined_up_faults(&read(name), block)
                .into_iter()
                .map(|f| format!("{name}: {f}")),
        );
    }
    assert!(bad.is_empty(), "not lined up: {bad:#?}");
    let light = read("assets/logo/baud-lockup-light.svg");
    let (paths, rects) = drawn(&light);
    let y = rects[0].0[1];
    let lower = light.replace(&format!(" y=\"{y}\""), &format!(" y=\"{}\"", y + 2.26));
    assert_ne!(lower, light, "the control moved the block");
    let lower = lined_up_faults(&lower, 0);
    assert_eq!(lower.len(), 1, "{lower:?}");
    assert!(lower[0].starts_with("the block's top"), "{lower:?}");
    let shifted = rects[0].0[0] + 1.0;
    let nearer = light.replace(
        &format!(" x=\"{}\"", rects[0].0[0]),
        &format!(" x=\"{shifted}\""),
    );
    assert_ne!(nearer, light, "the control moved the block");
    let nearer = lined_up_faults(&nearer, 0);
    assert_eq!(nearer.len(), 1, "{nearer:?}");
    assert!(nearer[0].starts_with("the gap"), "{nearer:?}");
    assert_eq!(paths.len(), 2, "the word and the ❯");
}

/// **The README's `<picture>`, what a reader who asked for reduced motion
/// gets first, in each colour scheme, then the films**: each source's
/// `media` and `srcset` in order, and the `<img>`'s `src`.
const README_PICTURE: [(&str, &str); 4] = [
    (
        "(prefers-reduced-motion: reduce) and (prefers-color-scheme: dark)",
        "assets/logo/baud-lockup-dark-tight.svg",
    ),
    (
        "(prefers-reduced-motion: reduce)",
        "assets/logo/baud-lockup-light-tight.svg",
    ),
    (
        "(prefers-color-scheme: dark)",
        "assets/readme/logo-dark.gif",
    ),
    ("", "assets/readme/logo-light.gif"),
];

/// What keeps `readme`'s first `<picture>` from being [`README_PICTURE`].
fn picture_faults(readme: &str) -> Vec<String> {
    let inside = readme
        .split("<picture>")
        .nth(1)
        .and_then(|s| s.split("</picture>").next())
        .unwrap_or_default();
    let quoted = |el: &str, k: &str| {
        el.split(&format!(" {k}=\""))
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or_default()
            .to_owned()
    };
    let got: Vec<(String, String)> = inside
        .split('<')
        .skip(1)
        .map(|el| match el.starts_with("img") {
            true => (String::new(), quoted(el, "src")),
            false => (quoted(el, "media"), quoted(el, "srcset")),
        })
        .collect();
    let want: Vec<(String, String)> = README_PICTURE
        .iter()
        .map(|(m, f)| ((*m).to_owned(), (*f).to_owned()))
        .collect();
    let mut out = Vec::new();
    if got != want {
        out.push(format!("the picture says {got:#?}"));
    }
    for (_, file) in &got {
        if !workspace().join(file).is_file() {
            out.push(format!("{file} is not there"));
        }
    }
    out
}

/// **A reader who asked for reduced motion gets a still**: the film repeats
/// and a README can give it no pause control (WCAG 2.2.2), so the sources for
/// `prefers-reduced-motion` come first, a browser taking the first that
/// matches, and each names a file that is there and does not move. The
/// controls: the two films first, and a still that is not there, are refused.
#[test]
fn the_readmes_logo_gives_reduced_motion_a_still() {
    let readme = fs::read_to_string(workspace().join("README.md")).expect("README.md");
    let bad = picture_faults(&readme);
    assert!(bad.is_empty(), "{bad:#?}");
    for (_, file) in &README_PICTURE[..2] {
        let text = fs::read_to_string(workspace().join(file)).expect("the still");
        assert!(!text.contains("<animate"), "{file} moves");
    }
    let (reduced, film) = (
        "(prefers-reduced-motion: reduce) and (prefers-color-scheme: dark)",
        "(prefers-color-scheme: dark)",
    );
    let swapped = readme
        .replacen(reduced, "@@", 1)
        .replacen(&format!("\"{film}\""), &format!("\"{reduced}\""), 1)
        .replacen("@@", film, 1);
    assert_ne!(swapped, readme, "the control swapped them");
    assert!(!picture_faults(&swapped).is_empty(), "the film first");
    let gone = readme.replacen("baud-lockup-dark-tight.svg", "baud-lockup-dark-gone.svg", 1);
    assert_ne!(gone, readme, "the control named another file");
    assert!(
        picture_faults(&gone)
            .iter()
            .any(|f| f.contains("not there")),
        "a still that is not there"
    );
}

/// The title bar's lockups, each with the lockup it is cropped from: the
/// light word for a dark appearance, the dark word for a light one.
const TITLEBAR: [(&str, &str); 2] = [
    (
        "assets/logo/baud-lockup-dark-tight.svg",
        "assets/logo/baud-lockup-dark.svg",
    ),
    (
        "assets/logo/baud-lockup-light-tight.svg",
        "assets/logo/baud-lockup-light.svg",
    ),
];

/// A path's commands and numbers, in order, and which of the numbers are
/// x (`true`) and which y: absolute `M`, `L`, `Q`, `C`, `H`, `V` and `Z`,
/// what fontTools writes.
fn tokens(d: &str) -> (String, Vec<f64>, Vec<bool>) {
    let (mut cmds, mut nums, mut axes) = (String::new(), Vec::new(), Vec::new());
    let mut cur = String::new();
    let flush = |cur: &mut String, nums: &mut Vec<f64>| {
        if !cur.is_empty() {
            nums.push(cur.parse().unwrap_or(f64::NAN));
            cur.clear();
        }
    };
    // The numbers since the last command take its axes.
    let settle = |cmd: Option<char>, nums: &[f64], axes: &mut Vec<bool>| {
        let start = axes.len();
        for i in 0..nums.len() - start {
            axes.push(match cmd {
                Some('H') => true,
                Some('V') => false,
                _ => i % 2 == 0,
            });
        }
    };
    for c in d.chars() {
        if c.is_ascii_alphabetic() {
            flush(&mut cur, &mut nums);
            settle(cmds.chars().last(), &nums, &mut axes);
            cmds.push(c);
        } else if c == ' ' || c == ',' {
            flush(&mut cur, &mut nums);
        } else if c == '-' && !cur.is_empty() {
            flush(&mut cur, &mut nums);
            cur.push(c);
        } else {
            cur.push(c);
        }
    }
    flush(&mut cur, &mut nums);
    settle(cmds.chars().last(), &nums, &mut axes);
    (cmds, nums, axes)
}

/// An SVG's drawn elements, comments out: each `path`'s `d` and fill, and
/// each `rect`'s x, y, width, height and fill.
type Drawn = (Vec<(String, String)>, Vec<([f64; 4], String)>);

fn drawn(text: &str) -> Drawn {
    let text = bare(text);
    let fill = |el: &str| {
        el.split(" fill=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or_default()
            .to_owned()
    };
    let (mut paths, mut rects) = (Vec::new(), Vec::new());
    for el in text.split('<').skip(1) {
        if el.starts_with("path ") {
            let d = el.split(" d=\"").nth(1).and_then(|s| s.split('"').next());
            paths.push((d.unwrap_or_default().to_owned(), fill(el)));
        } else if el.starts_with("rect ") {
            let n = |k| attr(el, k).unwrap_or(f64::NAN);
            rects.push(([n("x"), n("y"), n("width"), n("height")], fill(el)));
        }
    }
    (paths, rects)
}

/// The root's `viewBox`, `width` and `height`.
fn view_box(text: &str) -> Option<([f64; 4], (f64, f64))> {
    let root = text.split("<svg").nth(1)?.split('>').next()?;
    let vb = root.split(" viewBox=\"").nth(1)?.split('"').next()?;
    let v: Vec<f64> = vb.split(' ').filter_map(|n| n.parse().ok()).collect();
    let el = format!(" {root}");
    Some((
        <[f64; 4]>::try_from(v).ok()?,
        (attr(&el, "width")?, attr(&el, "height")?),
    ))
}

/// What keeps `tight` from being `source` cropped for the title bar, in
/// `source`'s units, or nothing: the word, the ❯ and the cursor are
/// `source`'s scaled by one factor, each number within one unit, in its
/// colours and on no ground; the cursor `cell` units tall, the titlebar
/// mark's; the box from the word's ink to the cursor's right, within one
/// unit, and centred on the cursor's rows, every point inside it.
fn tight_faults(tight: &str, source: &str, cell: f64) -> Vec<String> {
    let mut out = Vec::new();
    let ((tp, tr), (sp, sr)) = (drawn(tight), drawn(source));
    // The source's cursor is its last rect; the dark one's ground is first.
    let (Some(tc), Some(sc)) = (tr.last(), sr.last()) else {
        return vec!["no cursor".to_owned()];
    };
    if tr.len() != 1 {
        out.push(format!("{} rects, not the cursor alone", tr.len()));
    }
    if (tc.0[3] - cell).abs() > 0.01 {
        out.push(format!("the cursor is {} tall, not {cell}", tc.0[3]));
    }
    let k = tc.0[3] / sc.0[3];
    if tp.len() != sp.len() || tp.len() != 2 {
        out.push(format!("{} paths against {}", tp.len(), sp.len()));
        return out;
    }
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for ((td, tf), (sd, sf)) in tp.iter().zip(&sp) {
        if tf != sf {
            out.push(format!("a path in {tf}, not {sf}"));
        }
        let ((tcmd, tn, axes), (scmd, sn, _)) = (tokens(td), tokens(sd));
        if tcmd != scmd || tn.len() != sn.len() {
            out.push("a path of another shape".to_owned());
            continue;
        }
        for (i, (a, b)) in tn.iter().zip(&sn).enumerate() {
            if (a / k - b).abs() > 1.0 {
                out.push(format!(
                    "a path's number {a} is {b} at {k:.4} off by more than one unit"
                ));
                break;
            }
            if axes[i] { xs.push(*a) } else { ys.push(*a) }
        }
    }
    if tc.1 != sc.1 {
        out.push(format!("the cursor in {}, not {}", tc.1, sc.1));
    }
    for (a, b) in tc.0.iter().zip(&sc.0) {
        if (a / k - b).abs() > 1.0 {
            out.push(format!("the cursor's {a} is not {b} at {k:.4}"));
        }
    }
    let Some(([vx, vy, vw, vh], (w, h))) = view_box(tight) else {
        out.push("no viewBox".to_owned());
        return out;
    };
    if (w, h) != (vw, vh) {
        out.push(format!("drawn {w} by {h}, its box {vw} by {vh}"));
    }
    let left = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let right = tc.0[0] + tc.0[2];
    if (vx - left).abs() / k > 1.0 || (vx + vw - right).abs() / k > 1.0 {
        out.push(format!(
            "the box {vx}..{} is not the ink {left}..{right}",
            vx + vw
        ));
    }
    let centre = tc.0[1] + tc.0[3] / 2.0;
    if (vy + vh / 2.0 - centre).abs() / k > 1.0 {
        out.push(format!(
            "the box's centre {} is not the cursor's {centre}",
            vy + vh / 2.0
        ));
    }
    let (top, bottom) = ys.iter().fold((tc.0[1], tc.0[1] + tc.0[3]), |(a, b), &y| {
        (a.min(y), b.max(y))
    });
    if top < vy - 0.01 || bottom > vy + vh + 0.01 || left < vx - 0.01 || right > vx + vw + 0.01 {
        out.push(format!(
            "ink {left},{top}..{right},{bottom} outside the box"
        ));
    }
    out
}

/// **The title bar's lockups are the lockups, cropped**: the word and the
/// mark of `baud-lockup-dark.svg` and `baud-lockup-light.svg`, scaled
/// so the cursor is the titlebar mark's 168 units -- the mark keeps the
/// size it had beside the traffic lights -- in their colours and on no
/// ground, the box snug to the ink across and centred on the cursor's rows
/// up and down, so a view the image's shape centres the mark in the strip.
/// Both forms are one box, so a swap of form moves nothing. The
/// app embeds the same files through `baud-app/assets/logo/`, links to
/// them. The controls: the source lockup itself, and the tight lockup with
/// its box a cursor's width wider, are refused.
#[test]
fn the_title_bars_lockups_are_the_lockups_cropped() {
    let read = |rel: &str| fs::read_to_string(workspace().join(rel)).unwrap_or_default();
    let mark = read("assets/logo/baud-mark-amber-tight.svg");
    let cell = drawn(&mark).1.last().map_or(f64::NAN, |r| r.0[3]);
    assert_eq!(cell, 168.0, "the titlebar mark's cursor");
    let mut bad = Vec::new();
    let mut boxes = Vec::new();
    for (tight, source) in TITLEBAR {
        let (t, s) = (read(tight), read(source));
        bad.extend(
            tight_faults(&t, &s, cell)
                .into_iter()
                .map(|f| format!("{tight}: {f}")),
        );
        boxes.push(view_box(&t));
        let embedded = read(&tight.replace("assets/", "baud-app/assets/"));
        if embedded != t {
            bad.push(format!("{tight}: the app does not embed it"));
        }
    }
    assert!(bad.is_empty(), "not the lockups cropped: {bad:#?}");
    assert_eq!(boxes[0], boxes[1], "both forms are one box");
    let (t, s) = (read(TITLEBAR[0].0), read(TITLEBAR[0].1));
    assert!(
        !tight_faults(&s, &s, cell).is_empty(),
        "the control: the lockup itself"
    );
    let ([x, y, w, h], _) = view_box(&t).expect("a box");
    let wide = t
        .replace(
            &format!("viewBox=\"{x} {y} {w} {h}\""),
            &format!("viewBox=\"{} {y} {} {h}\"", x - 76.37, w + 76.37),
        )
        .replace(
            &format!("width=\"{w}\""),
            &format!("width=\"{}\"", w + 76.37),
        );
    assert_ne!(wide, t, "the control is a change");
    assert!(
        !tight_faults(&wide, &s, cell).is_empty(),
        "the control: a wider box"
    );
}
