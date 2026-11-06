//! **baud's font-build tools.** Four: `compare`, the check a rebuilt font
//! is held to when its bytes cannot be; [`stage`], which builds the Hebrew
//! stage of baud's embedded font from its committed sources; [`instance`],
//! which instances Noto Sans Devanagari through baud-font's partial bake;
//! and [`merge`], which merges that instance into each stage.
//!
//! ```text
//! cargo run --release --manifest-path tools/Cargo.toml \
//!   -p baud-fontbuild -- compare A.ttf B.ttf
//! ```
//!
//! Two fonts are equal when every one of these is: the glyph order and
//! post names; every cmap subtable's encoding record, format and mapping;
//! each glyph's contours, points, flags, instructions, components and
//! bounding box; each gvar tuple's region and the delta of every point,
//! the unreferenced ones inferred as the OpenType gvar chapter's
//! "Inferred deltas for un-referenced point numbers" infers them
//! (`[[source: ms-opentype-gvar]]`), so a different choice of explicit
//! points that infers the same deltas is not a difference; hmtx's advance
//! and side bearing per glyph and HVAR's deltas resolved at sampled axis
//! positions; head, hhea, maxp, OS/2, post, name, gasp, cvt, fpgm, prep,
//! fvar, avar and STAT field by field, `checkSumAdjustment` excepted; and
//! GDEF, GSUB and GPOS expanded: the script, language and feature graph
//! with feature indices resolved to tags and lookups, each lookup's type,
//! flags and mark filtering set by its glyphs, and its subtables expanded
//! to one mapping in which the first subtable to match wins, so where a
//! serializer splits a subtable is not a difference. Anchors, value
//! records and caret values with variation devices are resolved at the
//! sampled positions.
//!
//! The sampled positions are -1, 0 and 1 on every fvar axis, plus every
//! start, peak and end coordinate that a variation region of either font
//! names. Each axis is sampled with the others at their defaults, plus the
//! two corners. On a single axis a resolved delta is linear between
//! consecutive samples, so being equal at every sample means being equal
//! everywhere. With two or more axes that argument does not hold, and the
//! comparison checks only the samples.
//!
//! A table neither font models here (CFF, CFF2, sbix, CBDT, ...) or one
//! that does not parse is compared byte for byte, and the report says
//! so. loca is not compared on its own: glyf is read through it.
//!
//! The report names the first difference in each table, with the glyph
//! or lookup and the field, and the exit code is 0 for equal, 1 for
//! different and 2 for a file that cannot be read as a font.

use std::collections::BTreeSet;

use read_fonts::types::Tag;
use read_fonts::{FontRef, TableProvider};

mod cmap;
mod dump;
mod fixed;
mod glyphs;
pub mod instance;
mod layout;
pub mod merge;
pub mod stage;
mod var;

pub use dump::Dump;
pub use glyphs::iup_contour;

/// The tables expanded here, in report order. Every other tag is compared
/// byte for byte.
const MODELLED: &[&[u8; 4]] = &[
    b"cmap", b"glyf", b"loca", b"gvar", b"hmtx", b"HVAR", b"head", b"hhea", b"maxp", b"OS/2",
    b"post", b"name", b"gasp", b"cvt ", b"fpgm", b"prep", b"fvar", b"avar", b"STAT", b"GDEF",
    b"GSUB", b"GPOS",
];

/// What `compare` found: one line per table that differs, empty when the
/// fonts are equal.
#[derive(Debug, Default)]
pub struct Report {
    pub differences: Vec<String>,
}

impl Report {
    pub fn is_equal(&self) -> bool {
        self.differences.is_empty()
    }
}

/// The reason a file is not a font at all.
#[derive(Debug)]
pub struct NotAFont(pub String);

/// Compares two fonts table by table; see the crate's documentation for
/// what is compared.
pub fn compare(a: &[u8], b: &[u8]) -> Result<Report, NotAFont> {
    let fa = FontRef::new(a).map_err(|e| NotAFont(format!("A: {e}")))?;
    let fb = FontRef::new(b).map_err(|e| NotAFont(format!("B: {e}")))?;
    let ctx = var::Samples::of(&fa, &fb);
    let names = glyphs::GlyphNames::of(&fa);
    let mut report = Report::default();

    let tags_a: BTreeSet<Tag> = fa
        .table_directory()
        .table_records()
        .iter()
        .map(|r| r.tag())
        .collect();
    let tags_b: BTreeSet<Tag> = fb
        .table_directory()
        .table_records()
        .iter()
        .map(|r| r.tag())
        .collect();
    for tag in tags_a.symmetric_difference(&tags_b) {
        let side = if tags_a.contains(tag) { "A" } else { "B" };
        report
            .differences
            .push(format!("{tag}: present in {side} only"));
    }

    let order = |f: &FontRef| glyphs::glyph_order(f);
    push(
        &mut report,
        "glyph order",
        dump::diff(&order(&fa), &order(&fb), &names),
    );

    for tag in MODELLED {
        let tag = Tag::new(tag);
        if tag == Tag::new(b"loca") || !(tags_a.contains(&tag) && tags_b.contains(&tag)) {
            continue;
        }
        let da = expand(&fa, tag, &ctx);
        let db = expand(&fb, tag, &ctx);
        let found = match (&da, &db) {
            (Ok(da), Ok(db)) => dump::diff(da, db, &names),
            (Err(ea), Err(eb)) => raw(&fa, &fb, tag)
                .map(|d| format!("unreadable in both (A: {ea}; B: {eb}); bytes {d}")),
            (Err(e), Ok(_)) => Some(format!("A does not parse ({e}); B does")),
            (Ok(_), Err(e)) => Some(format!("B does not parse ({e}); A does")),
        };
        push(&mut report, &tag.to_string(), found);
    }

    for tag in tags_a.intersection(&tags_b) {
        if MODELLED.iter().any(|m| Tag::new(m) == *tag) {
            continue;
        }
        let found = raw(&fa, &fb, *tag).map(|d| format!("not modelled here; bytes {d}"));
        push(&mut report, &tag.to_string(), found);
    }
    Ok(report)
}

/// What [`reach`] found.
#[derive(Debug)]
pub struct Reach {
    /// Each expanded table and its line count.
    pub tables: Vec<(String, usize)>,
    /// Each table or subtable compared only as bytes.
    pub bytes_only: Vec<String>,
}

/// How much of one font the comparison reaches: each modelled table it
/// carries with the number of lines it expands to, and every subtable or
/// table that is compared only as bytes. A test holds the built fonts to
/// no bytes-only part, so a comparison of them cannot pass by not looking.
pub fn reach(bytes: &[u8]) -> Result<Reach, NotAFont> {
    let font = FontRef::new(bytes).map_err(|e| NotAFont(e.to_string()))?;
    let ctx = var::Samples::of(&font, &font);
    let tags: BTreeSet<Tag> = font
        .table_directory()
        .table_records()
        .iter()
        .map(|r| r.tag())
        .collect();
    let (mut tables, mut bytes_only) = (vec![], vec![]);
    tables.push(("glyph order".to_string(), glyphs::glyph_order(&font).len()));
    for tag in &tags {
        if *tag == Tag::new(b"loca") {
            continue;
        }
        if !MODELLED.iter().any(|m| Tag::new(m) == *tag) {
            bytes_only.push(format!("{tag}: not modelled"));
            continue;
        }
        match expand(&font, *tag, &ctx) {
            Ok(d) => {
                bytes_only.extend(d.unexpanded().into_iter().map(|u| format!("{tag}: {u}")));
                tables.push((tag.to_string(), d.len()));
            }
            Err(e) => bytes_only.push(format!("{tag}: unreadable: {e}")),
        }
    }
    Ok(Reach { tables, bytes_only })
}

fn push(report: &mut Report, table: &str, found: Option<String>) {
    if let Some(found) = found {
        report.differences.push(format!("{table}: {found}"));
    }
}

fn expand(font: &FontRef, tag: Tag, ctx: &var::Samples) -> Result<Dump, String> {
    let r = match &tag.to_be_bytes() {
        b"cmap" => cmap::dump(font),
        b"glyf" => glyphs::glyf(font),
        b"gvar" => glyphs::gvar(font),
        b"hmtx" => glyphs::hmtx(font),
        b"HVAR" => glyphs::hvar(font, ctx),
        b"head" => fixed::head(font),
        b"hhea" => fixed::hhea(font),
        b"maxp" => fixed::maxp(font),
        b"OS/2" => fixed::os2(font),
        b"post" => fixed::post(font),
        b"name" => fixed::name(font),
        b"gasp" => fixed::gasp(font),
        b"cvt " => fixed::cvt(font),
        b"fpgm" => font.fpgm().map(|b| fixed::bytes("fpgm", b)),
        b"prep" => font.prep().map(|b| fixed::bytes("prep", b)),
        b"fvar" => fixed::fvar(font),
        b"avar" => fixed::avar(font),
        b"STAT" => fixed::stat(font),
        b"GDEF" => layout::gdef(font, ctx),
        b"GSUB" => layout::gsub(font, ctx),
        b"GPOS" => layout::gpos(font, ctx),
        _ => unreachable!("{tag} is not modelled"),
    };
    r.map_err(|e| e.to_string())
}

/// Byte comparison of one table: `None` when equal, else where the bytes
/// first part.
fn raw(fa: &FontRef, fb: &FontRef, tag: Tag) -> Option<String> {
    let a = fa.table_data(tag).map(|d| d.as_bytes()).unwrap_or_default();
    let b = fb.table_data(tag).map(|d| d.as_bytes()).unwrap_or_default();
    if a == b {
        return None;
    }
    let at = a
        .iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    let byte = |s: &[u8]| s.get(at).map_or("end".to_string(), |v| format!("{v:#04x}"));
    Some(format!(
        "differ at byte {at} ({} vs {}); lengths {} and {}",
        byte(a),
        byte(b),
        a.len(),
        b.len()
    ))
}
