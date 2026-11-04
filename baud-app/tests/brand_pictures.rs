//! **The logo files: GitHub's limits, outlines in place of text, and the
//! mark as baud draws it.**
//!
//! `scripts/make-brand-pictures.py` draws every file in `assets/logo/` and
//! the repository's link picture from one mark, baud's own drawing of "❯"
//! with the cursor in the next cell, in JetBrains Mono's own outlines, and
//! its `--check` holds the committed files to its output byte for byte.
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

/// The logo SVGs: every one in `assets/logo/`, the two the app embeds
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

/// baud's cell at 200 px, as `scripts/make-brand-pictures.py` takes it:
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
/// cell, white on black through the CPU path, so a pixel's coverage is its
/// level over 255. Measured from its pixels: the ❯'s level top and bottom
/// edges by the coverage of their rows, its centroid by coverage.
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
    r.render(&mut t, &mut target, Damage::Full);
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
            let top = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let bottom = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            let (cx, cy) = centroid(&pts);
            Some(Mark {
                top: (top - y) / h,
                bottom: (bottom - y) / h,
                centroid: ((cx - (x - w)) / w, (cy - y) / h),
                aspect: w / h,
            })
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
