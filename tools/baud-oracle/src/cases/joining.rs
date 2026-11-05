//! A face whose Arabic letters are wider than a terminal cell.
//!
//! Writes `baud-font/tests/var/joining-cases.ttf`. A proportional Arabic
//! face draws a letter at its own advance, and the grid gives every letter
//! one cell: Noto Sans Arabic's isolated sad is 1306 units at an em of 1000
//! and its medial sad 949, against JetBrains Mono's cell of 600. This face
//! holds such letters with boxes for outlines, so where their ink lies is
//! exact, and the joining forms and a mark placed by an anchor, so a
//! renderer's joined cells and its marked ones are reached too.
//!
//! Glyphs, all boxes from y 0 unless said (x from, x to; advance):
//!
//! ```text
//! space         empty; 600
//! beh  U+0628   dual-joining: 0-500; 500, and beh.init, beh.medi, beh.fina
//!               the same
//! reh  U+0631   right-joining: 50-350, 0-300 high; 400, and reh.fina the
//!               same
//! sad  U+0635   dual-joining and wider than a cell: isolated 0-1300
//!               (1300), sad.init 0-1000, sad.medi 0-950, sad.fina 0-1100,
//!               each its advance its right edge
//! fatha U+064E  a mark, -50 to 50 across, 600-700 high; no advance
//! ```
//!
//! GSUB `init`, `medi` and `fina` (scripts `DFLT` and `arab`) take a letter
//! to its form, and `calt` has a rule that begins on a sad before a reh and
//! puts the sad back as it was, so a renderer that gathers the pairs its
//! rules begin on finds one, and draws nothing other for it. GPOS `mark`
//! hangs the fatha from an anchor on each base: at 1100 on the isolated
//! sad, 800 on sad.init, 750 on sad.medi, 900 on sad.fina -- past a
//! 600-unit cell, so a mark that is not moved with its fitted base lands
//! outside the base's cell -- and over the middle of every other letter.
//! GDEF classes the fatha a mark and every letter a base.
//!
//! The lookups are the subtables the feature text compiled to: `init` and
//! `medi` a single substitution by a glyph delta (format 1), `fina` by a
//! list (format 2), `calt` a chained context by coverage (format 3)
//! calling a single substitution of sad by itself, and `mark` one
//! mark-to-base subtable. Deterministic: the head timestamps are fixed,
//! at 3,600,000,000 seconds after 1904. baud's own font, under the
//! repository's licence (`baud-font/tests/licences.rs` lists it).

use write_fonts::tables::gdef::Gdef;
use write_fonts::tables::glyf::Glyph;
use write_fonts::tables::gpos::{
    AnchorFormat1, AnchorTable, BaseArray, BaseRecord, Gpos, MarkArray, MarkBasePos,
    MarkBasePosFormat1, MarkRecord, PositionLookup, PositionLookupList,
};
use write_fonts::tables::gsub::{
    Gsub, SingleSubst, SingleSubstFormat1, SingleSubstFormat2, SubstitutionLookup,
    SubstitutionLookupList,
};
use write_fonts::tables::layout::{
    ChainedSequenceContext, ClassDef, CoverageTable, Feature, FeatureList, FeatureRecord, LangSys,
    Lookup, LookupFlag, Script, ScriptList, ScriptRecord, SequenceLookupRecord,
};
use write_fonts::types::{GlyphId16, Tag};

use super::{
    Extremes, Totals, bbox, bytes, cmap, font, gid, glyf_loca, glyph_box, head, hhea, hmtx, maxp,
    name, os2, os2_bytes, poly, post_v2,
};

/// Each glyph: its code point, its box, its advance.
type Spec = (&'static str, Option<u32>, Option<(i16, i16, i16, i16)>, u16);

const GLYPHS: [Spec; 13] = [
    (".notdef", None, None, 600),
    ("space", Some(0x20), None, 600),
    ("beh", Some(0x0628), Some((0, 0, 500, 200)), 500),
    ("beh.init", None, Some((0, 0, 500, 200)), 500),
    ("beh.medi", None, Some((0, 0, 500, 200)), 500),
    ("beh.fina", None, Some((0, 0, 500, 200)), 500),
    ("reh", Some(0x0631), Some((50, 0, 350, 300)), 400),
    ("reh.fina", None, Some((50, 0, 350, 300)), 400),
    ("sad", Some(0x0635), Some((0, 0, 1300, 300)), 1300),
    ("sad.init", None, Some((0, 0, 1000, 300)), 1000),
    ("sad.medi", None, Some((0, 0, 950, 300)), 950),
    ("sad.fina", None, Some((0, 0, 1100, 300)), 1100),
    ("fatha", Some(0x064E), Some((-50, 600, 50, 700)), 0),
];

const LETTERS: [&str; 10] = [
    "beh", "beh.init", "beh.medi", "beh.fina", "reh", "reh.fina", "sad", "sad.init", "sad.medi",
    "sad.fina",
];

fn order() -> Vec<&'static str> {
    GLYPHS.iter().map(|g| g.0).collect()
}

fn g16(order: &[&str], n: &str) -> GlyphId16 {
    GlyphId16::new(gid(order, n).to_u32() as u16)
}

fn cov(order: &[&str], names: &[&str]) -> CoverageTable {
    names.iter().map(|n| g16(order, n)).collect()
}

/// The scripts, each with every feature on its default language system.
fn scripts(features: u16) -> ScriptList {
    ScriptList::new(
        [b"DFLT", b"arab"]
            .iter()
            .map(|t| {
                ScriptRecord::new(
                    Tag::new(t),
                    Script::new(Some(LangSys::new((0..features).collect())), vec![]),
                )
            })
            .collect(),
    )
}

fn single(s: impl Into<SingleSubst>) -> SubstitutionLookup {
    SubstitutionLookup::Single(Lookup::new(LookupFlag::empty(), vec![s.into()]))
}

fn gsub(o: &[&str]) -> Gsub {
    let by_delta = |pairs: &[(&str, &str)]| {
        let delta = gid(o, pairs[0].1).to_u32() as i16 - gid(o, pairs[0].0).to_u32() as i16;
        for (from, to) in pairs {
            assert_eq!(
                gid(o, to).to_u32() as i16 - gid(o, from).to_u32() as i16,
                delta,
                "one delta"
            );
        }
        let from: Vec<&str> = pairs.iter().map(|p| p.0).collect();
        single(SingleSubstFormat1::new(cov(o, &from), delta))
    };
    let fina = [
        ("beh", "beh.fina"),
        ("reh", "reh.fina"),
        ("sad", "sad.fina"),
    ];
    let lookups = vec![
        // init, medi and fina.
        by_delta(&[("beh", "beh.init"), ("sad", "sad.init")]),
        by_delta(&[("beh", "beh.medi"), ("sad", "sad.medi")]),
        single(SingleSubstFormat2::new(
            cov(o, &fina.map(|p| p.0)),
            fina.iter().map(|p| g16(o, p.1)).collect(),
        )),
        // calt: `sub sad' reh by sad`.
        SubstitutionLookup::ChainContextual(Lookup::new(
            LookupFlag::empty(),
            vec![
                ChainedSequenceContext::format_3(
                    vec![],
                    vec![cov(o, &["sad"])],
                    vec![cov(o, &["reh"])],
                    vec![SequenceLookupRecord::new(0, 4)],
                )
                .into(),
            ],
        )),
        by_delta(&[("sad", "sad")]),
    ];
    let features = [("calt", 3), ("fina", 2), ("init", 0), ("medi", 1)];
    Gsub::new(
        scripts(features.len() as u16),
        FeatureList::new(
            features
                .iter()
                .map(|(t, l)| {
                    FeatureRecord::new(
                        Tag::new(t.as_bytes().try_into().expect("a tag")),
                        Feature::new(None, vec![*l]),
                    )
                })
                .collect(),
        ),
        SubstitutionLookupList::new(lookups),
    )
}

/// GPOS: one mark-to-base lookup under `mark`, the fatha's anchor at (0,
/// 600) and each base's in coverage order.
fn gpos(o: &[&str]) -> Gpos {
    let anchor = |x, y| AnchorTable::Format1(AnchorFormat1::new(x, y));
    let at = |n: &str| match n {
        "sad" => (1100, 400),
        "sad.init" => (800, 400),
        "sad.medi" => (750, 400),
        "sad.fina" => (900, 400),
        "reh" | "reh.fina" => (200, 400),
        _ => (250, 300),
    };
    let subtable = MarkBasePos::Format1(MarkBasePosFormat1::new(
        cov(o, &["fatha"]),
        cov(o, &LETTERS),
        MarkArray::new(vec![MarkRecord::new(0, anchor(0, 600))]),
        BaseArray::new(
            LETTERS
                .iter()
                .map(|n| {
                    let (x, y) = at(n);
                    BaseRecord::new(vec![Some(anchor(x, y))])
                })
                .collect(),
        ),
    ));
    Gpos::new(
        scripts(1),
        FeatureList::new(vec![FeatureRecord::new(
            Tag::new(b"mark"),
            Feature::new(None, vec![0]),
        )]),
        PositionLookupList::new(vec![PositionLookup::MarkToBase(Lookup::new(
            LookupFlag::empty(),
            vec![subtable],
        ))]),
    )
}

/// GDEF: every letter a base (class 1), the fatha a mark (class 3).
fn gdef(o: &[&str]) -> Gdef {
    let classes: ClassDef = LETTERS
        .iter()
        .map(|n| (g16(o, n), 1))
        .chain([(g16(o, "fatha"), 3)])
        .collect();
    Gdef::new(Some(classes), None, None, None)
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    let o = order();
    // Each box drawn up its left edge, across the top and down its right.
    let glyphs: Vec<Glyph> = GLYPHS
        .iter()
        .map(|(_, _, b, _)| match *b {
            Some((x0, y0, x1, y1)) => {
                Glyph::Simple(poly(&[(x0, y0), (x0, y1), (x1, y1), (x1, y0)]))
            }
            None => Glyph::Empty,
        })
        .collect();
    let metrics: Vec<(u16, i16)> = GLYPHS
        .iter()
        .zip(&glyphs)
        .map(|((_, _, _, advance), g)| (*advance, glyph_box(g).map_or(0, |b| b.x_min)))
        .collect();
    let (glyf, loca, format) = glyf_loca(&glyphs);
    let (hmtx, long) = hmtx(&metrics);
    let map: Vec<(u32, _)> = GLYPHS
        .iter()
        .filter_map(|(n, c, _, _)| c.map(|c| (c, gid(&o, n))))
        .collect();
    // hhea's extremes and maxp's totals as fontTools recalculated them.
    let extremes = Extremes {
        advance_width_max: 1300,
        min_lsb: -50,
        min_rsb: -50,
        x_max_extent: 1300,
    };
    let totals = Totals {
        points: 4,
        contours: 1,
        ..super::ZERO_TOTALS
    };
    // Basic Latin and Arabic; a context of two glyphs, calt's.
    let mut os2 = os2(600, 0x20, 0x64E, 0);
    os2.ul_unicode_range_1 = 0x2001;
    os2.us_max_context = Some(2);
    let font = font(vec![
        (
            b"head",
            bytes(&head(
                0x0003,
                3_600_000_000,
                bbox(-50, 0, 1300, 700),
                Some(format),
            )),
        ),
        (b"hhea", bytes(&hhea(800, -200, &extremes, long))),
        (b"maxp", bytes(&maxp(o.len() as u16, &totals))),
        (b"OS/2", os2_bytes(&os2)),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (b"name", bytes(&name("baud joining cases", "Regular", &[]))),
        (b"post", bytes(&post_v2(&o))),
        (b"GDEF", bytes(&gdef(&o))),
        (b"GPOS", bytes(&gpos(&o))),
        (b"GSUB", bytes(&gsub(&o))),
    ]);
    vec![("baud-font/tests/var/joining-cases.ttf", font)]
}
