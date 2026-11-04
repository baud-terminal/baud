//! **The brand pictures: GitHub's limits, and outlines in place of text.**
//!
//! `scripts/make-brand-pictures.py` draws the organisation picture, the
//! repository's link picture, the Dock icon and the titlebar mark from
//! JetBrains Mono's own outlines, and its `--check` holds the committed
//! files to its output byte for byte. That check needs the network (the
//! 500 weight is read out of the font's release archive) and the pinned
//! nixpkgs, so it runs by hand. This file needs neither and runs with the
//! suite: each PNG within GitHub's limits and opaque by construction, and
//! each source made of paths and shapes, with no text a renderer could draw
//! in a fallback face. Before the generator, the Dock icon's ❯ was text
//! naming the font, and every renderer that drew it fell back.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

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

/// The Dock icon as the app embeds it (`baud-app/assets/logo/`, linked to
/// `assets/logo/`), the titlebar mark, and the two GitHub pictures' sources:
/// paths and shapes alone, the ❯ and every letter an outline.
#[test]
fn the_app_icon_and_the_brand_pictures_are_outlines_not_text() {
    let allowed: BTreeSet<String> = ["svg", "g", "path", "rect", "circle"]
        .map(String::from)
        .into();
    let mut bad = Vec::new();
    for rel in [
        "baud-app/assets/logo/baud-icon-dock.svg",
        "baud-app/assets/logo/baud-mark-amber-tight.svg",
        "assets/logo/baud-github-org.svg",
        "assets/readme/social.svg",
    ] {
        let text = fs::read_to_string(workspace().join(rel)).unwrap_or_default();
        let found = elements(&text);
        if !found.contains("path") || !found.is_subset(&allowed) {
            bad.push(format!("{rel}: {found:?}"));
        }
    }
    assert!(bad.is_empty(), "not outlines alone: {bad:#?}");

    // The control: the lockup names the font in text, and the same scan
    // finds it.
    let lockup = fs::read_to_string(workspace().join("assets/logo/baud-lockup-dark.svg"))
        .expect("the lockup");
    assert!(elements(&lockup).contains("text"), "the scan finds text");
}
