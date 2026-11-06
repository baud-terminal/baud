//! **The Devanagari merge.** `baud-fontbuild merge BUILT.ttf NOTO.ttf STYLE
//! EPOCH OUT.ttf` merges Noto Sans Devanagari, instanced at the style's
//! weight with its width axis kept (`baud-fontbuild instance`), into one
//! built face (JetBrains Mono with baud's Hebrew, `baud-fontbuild stage`).
//! OUT gets every glyph, lookup and table of BUILT unchanged, and after
//! them:
//!
//! - Noto's glyphs: each one a code point BUILT does not map reaches, each
//!   one a Noto lookup names, and their components, in Noto's order and at
//!   Noto's scale (both faces are 1000 units to the em), with their
//!   advances, their classes in GDEF and their outline and advance
//!   variations;
//! - the code points BUILT does not map, in every cmap subtable that can
//!   hold them;
//! - Noto's dev2 and deva scripts in GSUB and GPOS, with Noto's lookups,
//!   its features for them and its mark filtering sets, appended after
//!   BUILT's, and its GPOS variation store as GDEF's;
//! - one single substitution more, under dev2's and deva's nukt: BUILT's
//!   dotted circle to Noto's. The Indic shapers put U+25CC before a mark
//!   that has no base after locl and ccmp, from the face's cmap, which
//!   gives BUILT's dotted circle; Noto's mark positioning names its own, so
//!   the substitution, in the first stage after that, hands it Noto's, and
//!   a mark sits on the circle it was placed for. BUILT's U+25CC is
//!   unchanged everywhere else;
//! - Noto's width axis: fvar (one axis, wdth 62.5-100, default 100, hidden
//!   from user interfaces: it is there for a program to fit a word, and
//!   only the Devanagari glyphs vary along it), avar, gvar and HVAR
//!   (BUILT's glyphs vary by nothing), and the STAT table OpenType requires
//!   of a variable font;
//! - the font-wide values the new glyphs change: OS/2's Devanagari range
//!   bit, usMaxContext and xAvgCharWidth, post's isFixedPitch (0: the
//!   Devanagari is proportionally spaced), and head's, hhea's and maxp's
//!   extents and maxima. The line metrics, the em and every existing
//!   advance are BUILT's.
//!
//! The italics carry the upright Devanagari at their weight: Noto has no
//! italic, and nothing is synthesised.
//!
//! Noto's glyph for U+0020 is not copied: Noto's one lookup naming it (a
//! chained context of `dist` for Vedic signs after a space or a dotted
//! circle) names BUILT's space instead, the glyph a space in the run
//! already is. Noto's other glyphs for code points BUILT maps are not
//! copied either: no lookup names them and BUILT's cmap keeps its own.
//!
//! **GSUB is written by hand around BUILT's own bytes** ([`spliced`]):
//! BUILT's lookups and every subtable they name are copied as one block,
//! byte for byte, so stock JetBrains Mono's lookups, which the stage keeps
//! the same way, stay its own bytes, with the appended lookups after them, under a ScriptList, FeatureList
//! and LookupList written again: the features sorted by tag, as OpenType's
//! common table formats ask, which moves BUILT's feature indices, so its
//! language systems are written again too. GPOS is written whole by
//! write-fonts, as the Hebrew stage writes it.
//!
//! **The merge refuses to write OUT when BUILT changed**
//! ([`unchanged`]): every glyph's stored bytes and metrics, every cmap
//! entry, every lookup of GSUB as bytes and of GPOS as the comparator
//! expands it, the features each existing language system names, GDEF's
//! classes and mark sets, and every name record but nameIDs 0 and 7.
//!
//! The values fontTools chose when the merge was Python are chosen the same
//! way here: a new name ID is `addMultilingualName`'s
//! (`[[source: fonttools-4-63-0-name]]`: an existing ID with the same
//! Windows and Macintosh English records, else one past the highest), the
//! STAT values are `buildStatTable`'s format 1 in its order
//! (`[[source: fonttools-4-63-0-otllib-builder]]`), usMaxContext is
//! `maxCtxFont`'s (`[[source: fonttools-4-63-0-maxcontextcalc]]`), and
//! xAvgCharWidth is Python's `round` of the mean, half to even.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use read_fonts::tables::glyf::Glyph as ReadGlyph;
use read_fonts::types::{GlyphId, GlyphId16, NameId, Tag};
use read_fonts::{FontRef, TableProvider};
use write_fonts::from_obj::ToOwnedTable;
use write_fonts::tables::cmap::Cmap;
use write_fonts::tables::gdef::{Gdef, MarkGlyphSets};
use write_fonts::tables::gpos::{
    CursivePos, Gpos, MarkBasePos, MarkLigPos, MarkMarkPos, PairPos, PositionLookup, SinglePos,
};
use write_fonts::tables::gsub::{
    Gsub, LigatureSubst, MultipleSubst, ReverseChainSingleSubst, SingleSubst, SingleSubstFormat2,
    SubstitutionLookup,
};
use write_fonts::tables::head::Head;
use write_fonts::tables::hhea::Hhea;
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::layout::{
    ChainedSequenceContext, ClassDef, CoverageTable, FeatureList, FeatureParams, FeatureRecord,
    LangSys, Lookup, LookupFlag, LookupList, ScriptList, ScriptRecord, SequenceContext,
};
use write_fonts::tables::maxp::Maxp;
use write_fonts::tables::name::{Name, NameRecord};
use write_fonts::tables::os2::Os2;
use write_fonts::tables::post::Post;
use write_fonts::{FontBuilder, dump_table};

type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> String {
    move |err| format!("{what}: {err}")
}

/// The notices nameID 0 and 7 gain, read from the Noto file and required
/// to be this text byte for byte ("Any trademark notices must remain in any
/// derivative fonts", OFL-FAQ 1.1-update7 Q3.7).
const NOTO_COPYRIGHT: &str =
    "Copyright 2022 The Noto Project Authors (https://github.com/notofonts/devanagari)";
const NOTO_TRADEMARK: &str = "Noto is a trademark of Google Inc.";
/// OS/2 ulUnicodeRange1 bit 15: Devanagari (OpenType OS/2, Unicode ranges).
const DEVANAGARI_RANGE_BIT: u32 = 15;
/// fvar's axis flag 0x0001 (OpenType fvar): "The axis should not be
/// exposed directly in user interfaces."
const HIDDEN_AXIS: u16 = 0x0001;
/// STAT's ELIDABLE_AXIS_VALUE_NAME.
const ELIDABLE: u16 = 0x0002;
/// What a Noto glyph whose name BUILT has already is called.
const SUFFIX: &str = ".deva";
/// LookupFlag's USE_MARK_FILTERING_SET.
const MARK_FILTERING: u16 = 0x0010;

/// A style's weight and slope, as STAT names them.
fn style(name: &str) -> Res<(u16, u16)> {
    Ok(match name {
        "Regular" => (400, 0),
        "Bold" => (700, 0),
        "Italic" => (400, 1),
        "BoldItalic" => (700, 1),
        other => return Err(format!("style {other:?}")),
    })
}

/// What the merge wrote, for the build's log.
#[derive(Debug)]
pub struct Merged {
    pub font: Vec<u8>,
    pub kept: usize,
    pub code_points: usize,
}

fn glyph_names(f: &FontRef) -> Res<Vec<String>> {
    let n = f.maxp().map_err(e("maxp"))?.num_glyphs();
    let post = f.post().map_err(e("post"))?;
    (0..n)
        .map(|g| {
            post.glyph_name(GlyphId16::new(g))
                .map(str::to_string)
                .ok_or_else(|| format!("glyph {g} has no post name"))
        })
        .collect()
}

/// The face's best cmap: format 12 or 4 under Windows, every mapping.
fn best_cmap(f: &FontRef) -> Res<BTreeMap<u32, GlyphId>> {
    let cmap = f.cmap().map_err(e("cmap"))?;
    let mut best: Option<BTreeMap<u32, GlyphId>> = None;
    for want in [
        (3u16, 10u16),
        (0, 6),
        (0, 4),
        (3, 1),
        (0, 3),
        (0, 2),
        (0, 1),
        (0, 0),
    ] {
        for rec in cmap.encoding_records() {
            if (rec.platform_id() as u16, rec.encoding_id()) != want {
                continue;
            }
            let sub = rec.subtable(cmap.offset_data()).map_err(e("cmap"))?;
            let mut m = BTreeMap::new();
            for (c, g) in sub.iter() {
                m.insert(c, g);
            }
            best = Some(m);
            break;
        }
        if best.is_some() {
            break;
        }
    }
    best.ok_or_else(|| "cmap: no Unicode subtable".to_string())
}

// ---- glyph-id remapping over write-fonts' layout tables

/// Every glyph a value names, visited mutably: the remap and the collector
/// of the glyphs a lookup names are the same walk.
trait Glyphs {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()>;
}

fn each(v: &mut [GlyphId16], f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
    v.iter_mut().try_for_each(f)
}

impl Glyphs for CoverageTable {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        let mut v: Vec<GlyphId16> = self
            .iter()
            .map(|g| GlyphId16::try_from(g).map_err(|_| "coverage: a 32-bit glyph".to_string()))
            .collect::<Res<_>>()?;
        each(&mut v, f)?;
        if v.windows(2).any(|w| w[0] >= w[1]) {
            return Err("coverage: the remap reorders a coverage".to_string());
        }
        *self = CoverageTable::from(v);
        Ok(())
    }
}

impl Glyphs for ClassDef {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        // Class 0 is every glyph a class def does not list: fontTools reads
        // only the others, and they are the only ones remapped.
        let mut v: Vec<(GlyphId16, u16)> = self
            .iter()
            .filter(|(_, c)| *c != 0)
            .map(|(g, c)| {
                Ok((
                    GlyphId16::try_from(g).map_err(|_| "class def: a 32-bit glyph".to_string())?,
                    u16::try_from(c).map_err(|_| "class def: a 32-bit class".to_string())?,
                ))
            })
            .collect::<Res<_>>()?;
        for (g, _) in v.iter_mut() {
            f(g)?;
        }
        *self = v.into_iter().collect();
        Ok(())
    }
}

impl Glyphs for SequenceContext {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        match self {
            SequenceContext::Format1(t) => {
                t.coverage.glyphs(f)?;
                for set in t.seq_rule_sets.iter_mut().filter_map(|s| s.as_mut()) {
                    for rule in set.seq_rules.iter_mut() {
                        each(&mut rule.input_sequence, f)?;
                    }
                }
            }
            SequenceContext::Format2(t) => {
                t.coverage.glyphs(f)?;
                t.class_def.glyphs(f)?;
            }
            SequenceContext::Format3(t) => {
                for c in t.coverages.iter_mut() {
                    c.glyphs(f)?;
                }
            }
            _ => return Err("a sequence context of a format past 3".to_string()),
        }
        Ok(())
    }
}

impl Glyphs for ChainedSequenceContext {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        match self {
            ChainedSequenceContext::Format1(t) => {
                t.coverage.glyphs(f)?;
                for set in t
                    .chained_seq_rule_sets
                    .iter_mut()
                    .filter_map(|s| s.as_mut())
                {
                    for rule in set.chained_seq_rules.iter_mut() {
                        each(&mut rule.backtrack_sequence, f)?;
                        each(&mut rule.input_sequence, f)?;
                        each(&mut rule.lookahead_sequence, f)?;
                    }
                }
            }
            ChainedSequenceContext::Format2(t) => {
                t.coverage.glyphs(f)?;
                t.backtrack_class_def.glyphs(f)?;
                t.input_class_def.glyphs(f)?;
                t.lookahead_class_def.glyphs(f)?;
            }
            ChainedSequenceContext::Format3(t) => {
                for c in t
                    .backtrack_coverages
                    .iter_mut()
                    .chain(t.input_coverages.iter_mut())
                    .chain(t.lookahead_coverages.iter_mut())
                {
                    c.glyphs(f)?;
                }
            }
            _ => return Err("a chained context of a format past 3".to_string()),
        }
        Ok(())
    }
}

impl Glyphs for SingleSubst {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        // Format 1's delta does not survive a remap: both formats become a
        // list of substitutes.
        let (mut from, mut to): (Vec<GlyphId16>, Vec<GlyphId16>) = match self {
            SingleSubst::Format1(t) => t
                .coverage
                .iter()
                .map(|g| {
                    let g = g.to_u32() as u16;
                    (
                        GlyphId16::new(g),
                        GlyphId16::new(g.wrapping_add(t.delta_glyph_id as u16)),
                    )
                })
                .unzip(),
            SingleSubst::Format2(t) => (
                t.coverage
                    .iter()
                    .map(|g| GlyphId16::new(g.to_u32() as u16))
                    .collect(),
                t.substitute_glyph_ids.clone(),
            ),
            _ => return Err("a single substitution of a format past 2".to_string()),
        };
        each(&mut from, f)?;
        each(&mut to, f)?;
        if from.windows(2).any(|w| w[0] >= w[1]) {
            return Err("single substitution: the remap reorders it".to_string());
        }
        *self = SingleSubst::Format2(SingleSubstFormat2::new(from.into(), to));
        Ok(())
    }
}

impl Glyphs for MultipleSubst {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        let MultipleSubst::Format1(t) = self else {
            return Err("a multiple substitution of a format past 1".to_string());
        };
        t.coverage.glyphs(f)?;
        for s in t.sequences.iter_mut() {
            each(&mut s.substitute_glyph_ids, f)?;
        }
        Ok(())
    }
}

impl Glyphs for LigatureSubst {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        let LigatureSubst::Format1(t) = self else {
            return Err("a ligature substitution of a format past 1".to_string());
        };
        t.coverage.glyphs(f)?;
        for set in t.ligature_sets.iter_mut() {
            for l in set.ligatures.iter_mut() {
                f(&mut l.ligature_glyph)?;
                each(&mut l.component_glyph_ids, f)?;
            }
        }
        Ok(())
    }
}

impl Glyphs for SinglePos {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        match self {
            SinglePos::Format1(t) => t.coverage.glyphs(f),
            SinglePos::Format2(t) => t.coverage.glyphs(f),
            _ => Err("a single positioning of a format past 2".to_string()),
        }
    }
}

impl Glyphs for PairPos {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        match self {
            PairPos::Format1(t) => {
                t.coverage.glyphs(f)?;
                for set in t.pair_sets.iter_mut() {
                    for r in set.pair_value_records.iter_mut() {
                        f(&mut r.second_glyph)?;
                    }
                    if set
                        .pair_value_records
                        .windows(2)
                        .any(|w| w[0].second_glyph >= w[1].second_glyph)
                    {
                        return Err("pair set: the remap reorders it".to_string());
                    }
                }
                Ok(())
            }
            PairPos::Format2(t) => {
                t.coverage.glyphs(f)?;
                t.class_def1.glyphs(f)?;
                t.class_def2.glyphs(f)
            }
            _ => Err("a pair positioning of a format past 2".to_string()),
        }
    }
}

impl Glyphs for MarkBasePos {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        let MarkBasePos::Format1(t) = self else {
            return Err("a mark-to-base of a format past 1".to_string());
        };
        t.mark_coverage.glyphs(f)?;
        t.base_coverage.glyphs(f)
    }
}

impl Glyphs for MarkMarkPos {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        let MarkMarkPos::Format1(t) = self else {
            return Err("a mark-to-mark of a format past 1".to_string());
        };
        t.mark1_coverage.glyphs(f)?;
        t.mark2_coverage.glyphs(f)
    }
}

impl Glyphs for MarkLigPos {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        let MarkLigPos::Format1(t) = self else {
            return Err("a mark-to-ligature of a format past 1".to_string());
        };
        t.mark_coverage.glyphs(f)?;
        t.ligature_coverage.glyphs(f)
    }
}

impl Glyphs for CursivePos {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        let CursivePos::Format1(t) = self else {
            return Err("a cursive positioning of a format past 1".to_string());
        };
        t.coverage.glyphs(f)
    }
}

impl Glyphs for ReverseChainSingleSubst {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        let ReverseChainSingleSubst::Format1(t) = self else {
            return Err("a reverse chain of a format past 1".to_string());
        };
        t.coverage.glyphs(f)?;
        for c in t
            .backtrack_coverages
            .iter_mut()
            .chain(t.lookahead_coverages.iter_mut())
        {
            c.glyphs(f)?;
        }
        each(&mut t.substitute_glyph_ids, f)
    }
}

fn lookup_glyphs<T: Glyphs>(
    l: &mut Lookup<T>,
    f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>,
) -> Res<()> {
    for s in l.subtables.iter_mut() {
        s.glyphs(f)?;
    }
    Ok(())
}

impl Glyphs for SubstitutionLookup {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        match self {
            SubstitutionLookup::Single(l) => lookup_glyphs(l, f),
            SubstitutionLookup::Multiple(l) => lookup_glyphs(l, f),
            SubstitutionLookup::Ligature(l) => lookup_glyphs(l, f),
            SubstitutionLookup::Contextual(l) => {
                for s in l.subtables.iter_mut() {
                    (**s).glyphs(f)?;
                }
                Ok(())
            }
            SubstitutionLookup::ChainContextual(l) => {
                for s in l.subtables.iter_mut() {
                    (**s).glyphs(f)?;
                }
                Ok(())
            }
            SubstitutionLookup::Reverse(l) => lookup_glyphs(l, f),
            SubstitutionLookup::Alternate(_) => {
                Err("GSUB: an alternate substitution, which the merge does not remap".into())
            }
            SubstitutionLookup::Extension(_) => {
                Err("GSUB: an extension lookup, which the merge does not remap".into())
            }
        }
    }
}

impl Glyphs for PositionLookup {
    fn glyphs(&mut self, f: &mut dyn FnMut(&mut GlyphId16) -> Res<()>) -> Res<()> {
        match self {
            PositionLookup::Single(l) => lookup_glyphs(l, f),
            PositionLookup::Pair(l) => lookup_glyphs(l, f),
            PositionLookup::Cursive(l) => lookup_glyphs(l, f),
            PositionLookup::MarkToBase(l) => lookup_glyphs(l, f),
            PositionLookup::MarkToLig(l) => lookup_glyphs(l, f),
            PositionLookup::MarkToMark(l) => lookup_glyphs(l, f),
            PositionLookup::Contextual(l) => {
                for s in l.subtables.iter_mut() {
                    (**s).glyphs(f)?;
                }
                Ok(())
            }
            PositionLookup::ChainContextual(l) => {
                for s in l.subtables.iter_mut() {
                    (**s).glyphs(f)?;
                }
                Ok(())
            }
            PositionLookup::Extension(_) => {
                Err("GPOS: an extension lookup, which the merge does not remap".into())
            }
        }
    }
}

/// A lookup's flag and mark filtering set, and its nested lookup indices,
/// for whichever of the two tables it is in.
trait Shift {
    fn flag_and_set(&mut self) -> (&mut LookupFlag, &mut Option<u16>);
    fn records(&mut self, f: &mut dyn FnMut(&mut u16));
}

fn context_records(c: &mut SequenceContext, f: &mut dyn FnMut(&mut u16)) {
    match c {
        SequenceContext::Format1(t) => {
            for set in t.seq_rule_sets.iter_mut().filter_map(|s| s.as_mut()) {
                for r in set.seq_rules.iter_mut() {
                    r.seq_lookup_records
                        .iter_mut()
                        .for_each(|x| f(&mut x.lookup_list_index));
                }
            }
        }
        SequenceContext::Format2(t) => {
            for set in t.class_seq_rule_sets.iter_mut().filter_map(|s| s.as_mut()) {
                for r in set.class_seq_rules.iter_mut() {
                    r.seq_lookup_records
                        .iter_mut()
                        .for_each(|x| f(&mut x.lookup_list_index));
                }
            }
        }
        SequenceContext::Format3(t) => {
            t.seq_lookup_records
                .iter_mut()
                .for_each(|x| f(&mut x.lookup_list_index));
        }
        _ => {}
    }
}

fn chain_records(c: &mut ChainedSequenceContext, f: &mut dyn FnMut(&mut u16)) {
    match c {
        ChainedSequenceContext::Format1(t) => {
            for set in t
                .chained_seq_rule_sets
                .iter_mut()
                .filter_map(|s| s.as_mut())
            {
                for r in set.chained_seq_rules.iter_mut() {
                    r.seq_lookup_records
                        .iter_mut()
                        .for_each(|x| f(&mut x.lookup_list_index));
                }
            }
        }
        ChainedSequenceContext::Format2(t) => {
            for set in t
                .chained_class_seq_rule_sets
                .iter_mut()
                .filter_map(|s| s.as_mut())
            {
                for r in set.chained_class_seq_rules.iter_mut() {
                    r.seq_lookup_records
                        .iter_mut()
                        .for_each(|x| f(&mut x.lookup_list_index));
                }
            }
        }
        ChainedSequenceContext::Format3(t) => {
            t.seq_lookup_records
                .iter_mut()
                .for_each(|x| f(&mut x.lookup_list_index));
        }
        _ => {}
    }
}

macro_rules! flag_and_set {
    ($l:expr) => {
        (&mut $l.lookup_flag, &mut $l.mark_filtering_set)
    };
}

impl Shift for SubstitutionLookup {
    fn flag_and_set(&mut self) -> (&mut LookupFlag, &mut Option<u16>) {
        match self {
            SubstitutionLookup::Single(l) => flag_and_set!(l),
            SubstitutionLookup::Multiple(l) => flag_and_set!(l),
            SubstitutionLookup::Alternate(l) => flag_and_set!(l),
            SubstitutionLookup::Ligature(l) => flag_and_set!(l),
            SubstitutionLookup::Contextual(l) => flag_and_set!(l),
            SubstitutionLookup::ChainContextual(l) => flag_and_set!(l),
            SubstitutionLookup::Extension(l) => flag_and_set!(l),
            SubstitutionLookup::Reverse(l) => flag_and_set!(l),
        }
    }

    fn records(&mut self, f: &mut dyn FnMut(&mut u16)) {
        match self {
            SubstitutionLookup::Contextual(l) => {
                l.subtables.iter_mut().for_each(|s| context_records(s, f))
            }
            SubstitutionLookup::ChainContextual(l) => {
                l.subtables.iter_mut().for_each(|s| chain_records(s, f))
            }
            _ => {}
        }
    }
}

impl Shift for PositionLookup {
    fn flag_and_set(&mut self) -> (&mut LookupFlag, &mut Option<u16>) {
        match self {
            PositionLookup::Single(l) => flag_and_set!(l),
            PositionLookup::Pair(l) => flag_and_set!(l),
            PositionLookup::Cursive(l) => flag_and_set!(l),
            PositionLookup::MarkToBase(l) => flag_and_set!(l),
            PositionLookup::MarkToLig(l) => flag_and_set!(l),
            PositionLookup::MarkToMark(l) => flag_and_set!(l),
            PositionLookup::Contextual(l) => flag_and_set!(l),
            PositionLookup::ChainContextual(l) => flag_and_set!(l),
            PositionLookup::Extension(l) => flag_and_set!(l),
        }
    }

    fn records(&mut self, f: &mut dyn FnMut(&mut u16)) {
        match self {
            PositionLookup::Contextual(l) => {
                l.subtables.iter_mut().for_each(|s| context_records(s, f))
            }
            PositionLookup::ChainContextual(l) => {
                l.subtables.iter_mut().for_each(|s| chain_records(s, f))
            }
            _ => {}
        }
    }
}

// ---- maxCtxFont

fn ctx_chain(c: &ChainedSequenceContext) -> usize {
    match c {
        ChainedSequenceContext::Format1(t) => t
            .chained_seq_rule_sets
            .iter()
            .filter_map(|s| s.as_ref())
            .flat_map(|s| s.chained_seq_rules.iter())
            .map(|r| r.input_sequence.len() + 1 + r.lookahead_sequence.len())
            .max()
            .unwrap_or(0),
        ChainedSequenceContext::Format2(t) => t
            .chained_class_seq_rule_sets
            .iter()
            .filter_map(|s| s.as_ref())
            .flat_map(|s| s.chained_class_seq_rules.iter())
            .map(|r| r.input_sequence.len() + 1 + r.lookahead_sequence.len())
            .max()
            .unwrap_or(0),
        ChainedSequenceContext::Format3(t) => t.input_coverages.len() + t.lookahead_coverages.len(),
        _ => 0,
    }
}

fn ctx_context(c: &SequenceContext) -> usize {
    match c {
        SequenceContext::Format1(t) => t
            .seq_rule_sets
            .iter()
            .filter_map(|s| s.as_ref())
            .flat_map(|s| s.seq_rules.iter())
            .map(|r| r.input_sequence.len() + 1)
            .max()
            .unwrap_or(0),
        SequenceContext::Format2(t) => t
            .class_seq_rule_sets
            .iter()
            .filter_map(|s| s.as_ref())
            .flat_map(|s| s.class_seq_rules.iter())
            .map(|r| r.input_sequence.len() + 1)
            .max()
            .unwrap_or(0),
        SequenceContext::Format3(t) => t.coverages.len(),
        _ => 0,
    }
}

/// fontTools' `maxCtxFont` over GSUB's and GPOS's lookups: 1 for a single,
/// multiple or alternate substitution or a single adjustment, 2 for a pair,
/// a ligature's component count, a context's glyph count, a chain's input
/// and lookahead counts, a reverse chain's lookahead plus one.
fn max_context(gsub: &[SubstitutionLookup], gpos: &[PositionLookup]) -> Res<u16> {
    let mut m = 0usize;
    for l in gsub {
        m = m.max(match l {
            SubstitutionLookup::Single(_)
            | SubstitutionLookup::Multiple(_)
            | SubstitutionLookup::Alternate(_) => 1,
            SubstitutionLookup::Ligature(l) => l
                .subtables
                .iter()
                .filter_map(|s| match &**s {
                    LigatureSubst::Format1(t) => Some(t),
                    _ => None,
                })
                .flat_map(|t| t.ligature_sets.iter())
                .flat_map(|s| s.ligatures.iter())
                .map(|l| l.component_glyph_ids.len() + 1)
                .max()
                .unwrap_or(0),
            SubstitutionLookup::Contextual(l) => l
                .subtables
                .iter()
                .map(|s| ctx_context(s))
                .max()
                .unwrap_or(0),
            SubstitutionLookup::ChainContextual(l) => {
                l.subtables.iter().map(|s| ctx_chain(s)).max().unwrap_or(0)
            }
            SubstitutionLookup::Reverse(l) => l
                .subtables
                .iter()
                .filter_map(|s| match &**s {
                    ReverseChainSingleSubst::Format1(t) => Some(t.lookahead_coverages.len() + 1),
                    _ => None,
                })
                .max()
                .unwrap_or(0),
            SubstitutionLookup::Extension(_) => return Err("maxp context: GSUB extension".into()),
        });
    }
    for l in gpos {
        m = m.max(match l {
            PositionLookup::Single(_) => 1,
            PositionLookup::Pair(_) => 2,
            PositionLookup::Contextual(l) => l
                .subtables
                .iter()
                .map(|s| ctx_context(s))
                .max()
                .unwrap_or(0),
            PositionLookup::ChainContextual(l) => {
                l.subtables.iter().map(|s| ctx_chain(s)).max().unwrap_or(0)
            }
            PositionLookup::Extension(_) => return Err("max context: GPOS extension".into()),
            _ => 0,
        });
    }
    u16::try_from(m).map_err(|_| "max context past 16 bits".to_string())
}

// ---- names

/// The name table being grown, as fontTools' `addMultilingualName` grows it.
struct Names {
    records: Vec<NameRecord>,
}

impl Names {
    /// The English name `text`'s ID: an existing ID at or past `min` whose
    /// Windows and Macintosh English records are exactly `text`, else one
    /// past the highest ID in the table (at least 256), with both records
    /// added.
    fn add(&mut self, text: &str, min: u16) -> NameId {
        let want: BTreeSet<(u16, u16, u16)> = [(3, 1, 0x409), (1, 0, 0)].into_iter().collect();
        let mut matching: BTreeMap<u16, BTreeSet<(u16, u16, u16)>> = BTreeMap::new();
        for r in &self.records {
            let key = (r.platform_id, r.encoding_id, r.language_id);
            if want.contains(&key) && r.string.as_str() == text && r.name_id.to_u16() >= min {
                matching.entry(r.name_id.to_u16()).or_default().insert(key);
            }
        }
        if let Some((id, _)) = matching.iter().find(|(_, set)| **set == want) {
            return NameId::new(*id);
        }
        let id = 1 + self
            .records
            .iter()
            .map(|r| r.name_id.to_u16())
            .chain([255])
            .max()
            .unwrap_or(255);
        let id = NameId::new(id);
        self.records
            .push(NameRecord::new(3, 1, 0x409, id, text.to_string().into()));
        self.records
            .push(NameRecord::new(1, 0, 0, id, text.to_string().into()));
        id
    }
}

/// fontTools' `getDebugName`: the first English record of `id`, Macintosh
/// or Windows, else any.
fn debug_name(f: &FontRef, id: NameId) -> Res<String> {
    let name = f.name().map_err(e("name"))?;
    let mut some = None;
    for r in name.name_record() {
        if r.name_id() != id {
            continue;
        }
        let Ok(s) = r.string(name.string_data()) else {
            continue;
        };
        let s: String = s.chars().collect();
        if matches!((r.platform_id(), r.language_id()), (1, 0) | (3, 0x409)) {
            return Ok(s);
        }
        some.get_or_insert(s);
    }
    some.ok_or_else(|| format!("Noto has no name {id}"))
}

// ---- GSUB's splice

fn u16_at(t: &[u8], at: usize) -> Res<usize> {
    t.get(at..at + 2)
        .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
        .ok_or_else(|| format!("GSUB: a field at {at} past the table"))
}

/// Where a GSUB table's lookups start, each lookup's offset, and the list
/// tables (Script, LangSys, Feature, FeatureParams) that lie past that
/// start, among the lookups. Every subtable lies after the table that names
/// it (an offset is unsigned), so the lookups and their subtables are all
/// in the block from that start to the table's end; the list tables found
/// in it are left there, named by nothing, when the block is copied under
/// lists written again.
/// A GSUB table's lookup block: where it starts, each lookup's offset, and
/// the list tables lying in it, as byte ranges.
type Region = (usize, Vec<usize>, Vec<(usize, usize)>);

fn lookup_region(t: &[u8]) -> Res<Region> {
    let (major, minor) = (u16_at(t, 0)?, u16_at(t, 2)?);
    if (major, minor) != (1, 0) {
        return Err(format!("GSUB version {major}.{minor}; want 1.0"));
    }
    let (sl, fl, ll) = (u16_at(t, 4)?, u16_at(t, 6)?, u16_at(t, 8)?);
    let lookups = (0..u16_at(t, ll)?)
        .map(|i| Ok(ll + u16_at(t, ll + 2 + 2 * i)?))
        .collect::<Res<Vec<_>>>()?;
    let start = *lookups.iter().min().ok_or("GSUB: no lookups")?;
    let mut ranges: BTreeSet<(usize, usize)> = BTreeSet::new();
    let langsys = |at: usize, ranges: &mut BTreeSet<(usize, usize)>| -> Res<()> {
        ranges.insert((at, at + 6 + 2 * u16_at(t, at + 4)?));
        Ok(())
    };
    for i in 0..u16_at(t, sl)? {
        let sc = sl + u16_at(t, sl + 6 + 6 * i)?;
        ranges.insert((sc, sc + 4 + 6 * u16_at(t, sc + 2)?));
        if u16_at(t, sc)? != 0 {
            langsys(sc + u16_at(t, sc)?, &mut ranges)?;
        }
        for k in 0..u16_at(t, sc + 2)? {
            langsys(sc + u16_at(t, sc + 4 + 6 * k + 4)?, &mut ranges)?;
        }
    }
    for i in 0..u16_at(t, fl)? {
        let ft = fl + u16_at(t, fl + 6 + 6 * i)?;
        ranges.insert((ft, ft + 4 + 2 * u16_at(t, ft + 2)?));
        if u16_at(t, ft)? != 0 {
            ranges.insert((ft + u16_at(t, ft)?, ft + u16_at(t, ft)? + 4));
        }
    }
    let inside = ranges.into_iter().filter(|(_, e)| *e > start).collect();
    Ok((start, lookups, inside))
}

fn langsys_bytes(ls: &LangSys) -> Vec<u8> {
    let mut b = vec![0, 0];
    b.extend(ls.required_feature_index.to_be_bytes());
    b.extend((ls.feature_indices.len() as u16).to_be_bytes());
    for i in &ls.feature_indices {
        b.extend(i.to_be_bytes());
    }
    b
}

fn script_bytes(sc: &write_fonts::tables::layout::Script) -> Vec<u8> {
    let systems: Vec<(Tag, Vec<u8>)> = sc
        .lang_sys_records
        .iter()
        .map(|r| (r.lang_sys_tag, langsys_bytes(&r.lang_sys)))
        .collect();
    let head = 4 + 6 * systems.len();
    let mut body = vec![];
    let mut dflt = 0usize;
    if let Some(ls) = sc.default_lang_sys.as_ref() {
        dflt = head;
        body.extend(langsys_bytes(ls));
    }
    let mut recs = vec![];
    for (tag, b) in &systems {
        recs.extend(tag.to_be_bytes());
        recs.extend(((head + body.len()) as u16).to_be_bytes());
        body.extend(b);
    }
    let mut out = (dflt as u16).to_be_bytes().to_vec();
    out.extend((systems.len() as u16).to_be_bytes());
    out.extend(recs);
    out.extend(body);
    out
}

fn feature_bytes(f: &write_fonts::tables::layout::Feature) -> Res<Vec<u8>> {
    let idx = &f.lookup_list_indices;
    let params = match f.feature_params.as_ref() {
        None => vec![],
        Some(FeatureParams::StylisticSet(p)) => {
            let mut b = 0u16.to_be_bytes().to_vec();
            b.extend(p.ui_name_id.to_u16().to_be_bytes());
            b
        }
        Some(_) => return Err("GSUB: feature params other than a stylistic set's".into()),
    };
    let off = if params.is_empty() {
        0
    } else {
        4 + 2 * idx.len()
    };
    let mut b = (off as u16).to_be_bytes().to_vec();
    b.extend((idx.len() as u16).to_be_bytes());
    for i in idx {
        b.extend(i.to_be_bytes());
    }
    b.extend(params);
    Ok(b)
}

/// GSUB's bytes: BUILT's lookups and their subtables as BUILT stores them,
/// byte for byte and at the same distances from each other, and after them
/// the appended lookups, under a ScriptList, FeatureList and LookupList
/// written again for the merged table. Answers the bytes and how many bytes
/// of BUILT's old list tables are left among its lookups.
pub(crate) fn spliced(
    old: &[u8],
    scripts: &[ScriptRecord],
    feats: &[FeatureRecord],
    appended: Vec<SubstitutionLookup>,
    nlook: usize,
) -> Res<(Vec<u8>, usize)> {
    let (start, lookups, stranded) = lookup_region(old)?;
    let region = &old[start..];
    let (tail, flookups) = if appended.is_empty() {
        (vec![], vec![])
    } else {
        appended_region(appended)?
    };
    let fstart = 0usize;
    let tail = &tail[..];
    let sl = 10usize;
    let fl = sl + 2 + 6 * scripts.len();
    let ll = fl + 2 + 6 * feats.len();
    let mut at = ll + 2 + 2 * nlook;
    let mut blobs: Vec<Vec<u8>> = vec![];
    let mut script_at = vec![];
    let mut seen: HashMap<Vec<u8>, usize> = HashMap::new();
    for sr in scripts {
        let b = script_bytes(&sr.script);
        // One table for identical scripts: DFLT and hebr share one.
        let o = *seen.entry(b.clone()).or_insert_with(|| {
            let o = at;
            at += b.len();
            blobs.push(b);
            o
        });
        script_at.push(o);
    }
    let mut feature_at = vec![];
    for fr in feats {
        let b = feature_bytes(&fr.feature)?;
        feature_at.push(at);
        at += b.len();
        blobs.push(b);
    }
    let region_at = at;
    let tail_at = region_at + region.len();
    let look_at: Vec<usize> = lookups
        .iter()
        .map(|o| region_at + (o - start))
        .chain(flookups.iter().map(|o| tail_at + (o - fstart)))
        .collect();
    if look_at.len() != nlook {
        return Err(format!(
            "GSUB: {} lookups placed, {nlook} in the table",
            look_at.len()
        ));
    }
    for (name, base, offs) in [
        ("script", sl, &script_at),
        ("feature", fl, &feature_at),
        ("lookup", ll, &look_at),
    ] {
        if offs.iter().any(|o| o - base > 0xFFFF) {
            return Err(format!("GSUB: a {name} offset overflows"));
        }
    }
    let mut out: Vec<u8> = vec![];
    for v in [1u16, 0, sl as u16, fl as u16, ll as u16] {
        out.extend(v.to_be_bytes());
    }
    out.extend((scripts.len() as u16).to_be_bytes());
    for (sr, o) in scripts.iter().zip(&script_at) {
        out.extend(sr.script_tag.to_be_bytes());
        out.extend(((o - sl) as u16).to_be_bytes());
    }
    out.extend((feats.len() as u16).to_be_bytes());
    for (fr, o) in feats.iter().zip(&feature_at) {
        out.extend(fr.feature_tag.to_be_bytes());
        out.extend(((o - fl) as u16).to_be_bytes());
    }
    out.extend((nlook as u16).to_be_bytes());
    for o in &look_at {
        out.extend(((o - ll) as u16).to_be_bytes());
    }
    for b in &blobs {
        out.extend(b);
    }
    if out.len() != region_at {
        return Err(format!(
            "GSUB: the lists are {} bytes, placed for {region_at}",
            out.len()
        ));
    }
    out.extend(region);
    out.extend(tail);
    if &out[region_at..region_at + region.len()] != region {
        return Err("GSUB: the old lookups moved".into());
    }
    Ok((out, stranded.iter().map(|(b, e)| e - b).sum()))
}

/// The appended lookups' block, laid out as fontTools' serializer lays a
/// GSUB out: every Lookup table first, in order, then their subtables.
/// write-fonts writes the lookups alone in a table of their own, sharing
/// what they share; its Lookup tables, which it places among their
/// subtables, are written again at the front with their offsets moved,
/// and the table follows them whole (its own copies, and its empty lists,
/// named by nothing). Answers the block and where in it each lookup is.
fn appended_region(lookups: Vec<SubstitutionLookup>) -> Res<(Vec<u8>, Vec<usize>)> {
    let alone = Gsub::new(
        ScriptList::default(),
        FeatureList::default(),
        write_fonts::tables::gsub::SubstitutionLookupList::new(lookups.into_iter().collect()),
    );
    let fresh = dump_table(&alone).map_err(e("GSUB (the appended lookups)"))?;
    let ll = u16_at(&fresh, 8)?;
    let n = u16_at(&fresh, ll)?;
    let mut heads = vec![];
    for i in 0..n {
        let at = ll + u16_at(&fresh, ll + 2 + 2 * i)?;
        let flag = u16_at(&fresh, at + 2)?;
        let subs = (0..u16_at(&fresh, at + 4)?)
            .map(|k| Ok(at + u16_at(&fresh, at + 6 + 2 * k)?))
            .collect::<Res<Vec<_>>>()?;
        let set = if flag & usize::from(MARK_FILTERING) != 0 {
            Some(u16_at(&fresh, at + 6 + 2 * subs.len())?)
        } else {
            None
        };
        heads.push((u16_at(&fresh, at)?, flag, subs, set));
    }
    let size: usize = heads
        .iter()
        .map(|h| 6 + 2 * h.2.len() + if h.3.is_some() { 2 } else { 0 })
        .sum();
    let mut out: Vec<u8> = vec![];
    let mut look_at = vec![];
    for (kind, flag, subs, set) in &heads {
        let here = out.len();
        look_at.push(here);
        for v in [*kind, *flag, subs.len()] {
            out.extend((v as u16).to_be_bytes());
        }
        for s in subs {
            let off = size + s - here;
            if off > 0xFFFF {
                return Err("GSUB: an appended subtable lies past its lookup's reach".into());
            }
            out.extend((off as u16).to_be_bytes());
        }
        if let Some(m) = set {
            out.extend((*m as u16).to_be_bytes());
        }
    }
    out.extend(&fresh);
    Ok((out, look_at))
}

/// BUILT's and Noto's feature records in one list sorted by tag ("The
/// FeatureRecord array should be sorted alphabetically by feature tag",
/// OpenType's common table formats), BUILT's before Noto's under one tag,
/// each list's own order kept; and where each input index went.
fn merged_features(
    built: Vec<FeatureRecord>,
    noto: Vec<FeatureRecord>,
) -> (Vec<FeatureRecord>, [HashMap<usize, u16>; 2]) {
    let mut tagged: Vec<(Tag, usize, usize, FeatureRecord)> = built
        .into_iter()
        .enumerate()
        .map(|(i, r)| (r.feature_tag, 0, i, r))
        .chain(
            noto.into_iter()
                .enumerate()
                .map(|(i, r)| (r.feature_tag, 1, i, r)),
        )
        .collect();
    tagged.sort_by_key(|(t, side, i, _)| (*t, *side, *i));
    let mut where_ = [HashMap::new(), HashMap::new()];
    let mut out = vec![];
    for (k, (_, side, i, r)) in tagged.into_iter().enumerate() {
        where_[side].insert(i, k as u16);
        out.push(r);
    }
    (out, where_)
}

/// Noto's script list for dev2 and deva, and the features they name.
struct NotoLayout {
    scripts: Vec<ScriptRecord>,
    used: Vec<usize>,
}

fn noto_scripts(list: &ScriptList) -> Res<NotoLayout> {
    let mut used: Vec<usize> = vec![];
    let mut scripts = vec![];
    for sr in &list.script_records {
        if sr.script_tag != Tag::new(b"dev2") && sr.script_tag != Tag::new(b"deva") {
            continue;
        }
        let sc = &sr.script;
        for ls in sc
            .default_lang_sys
            .as_ref()
            .into_iter()
            .chain(sc.lang_sys_records.iter().map(|r| &*r.lang_sys))
        {
            if ls.required_feature_index != 0xFFFF {
                return Err(format!("Noto {} has a required feature", sr.script_tag));
            }
            for i in &ls.feature_indices {
                if !used.contains(&usize::from(*i)) {
                    used.push(usize::from(*i));
                }
            }
        }
        scripts.push(sr.clone());
    }
    used.sort();
    Ok(NotoLayout { scripts, used })
}

/// One table's merge: BUILT's lookups, Noto's appended, the features and
/// scripts joined. Answers the scripts and features in their final order
/// and the lookups.
struct Joined<L> {
    scripts: Vec<ScriptRecord>,
    features: Vec<FeatureRecord>,
    lookups: Vec<L>,
}

#[allow(clippy::too_many_arguments)]
fn join<L: Glyphs + Shift>(
    tag: &str,
    built_scripts: &ScriptList,
    built_features: &FeatureList,
    built_lookups: Vec<L>,
    noto_scripts_list: &ScriptList,
    noto_features: &FeatureList,
    noto_lookups: Vec<L>,
    nsets: u16,
    remap: &mut dyn FnMut(&mut GlyphId16) -> Res<()>,
    extra: Option<(L, &str)>,
    names: &mut Names,
    noto: &FontRef,
) -> Res<Joined<L>> {
    let nl = u16::try_from(built_lookups.len()).map_err(|_| format!("{tag}: lookups"))?;
    let mut lookups = built_lookups;
    for mut lk in noto_lookups {
        lk.records(&mut |i| *i += nl);
        let (flag, set) = lk.flag_and_set();
        if flag.to_bits() & MARK_FILTERING != 0
            && let Some(s) = set.as_mut()
        {
            *s += nsets;
        }
        lk.glyphs(remap)?;
        lookups.push(lk);
    }
    let (extra_lookup, extra_feature) = match extra {
        Some((lk, f)) => (Some(lk), Some(f)),
        None => (None, None),
    };
    let mut extra_index = None;
    if let Some(lk) = extra_lookup {
        extra_index = Some(lookups.len() as u16);
        lookups.push(lk);
    }
    let nt = noto_scripts(noto_scripts_list)?;
    let mut nfeats = vec![];
    for &i in &nt.used {
        let mut fr = noto_features
            .feature_records
            .get(i)
            .ok_or_else(|| format!("Noto {tag} has no feature {i}"))?
            .clone();
        for j in fr.feature.lookup_list_indices.iter_mut() {
            *j += nl;
        }
        if let (Some(at), Some(feature)) = (extra_index, extra_feature)
            && fr.feature_tag == Tag::new(feature.as_bytes().try_into().map_err(|_| "tag")?)
        {
            fr.feature.lookup_list_indices.push(at);
        }
        if let Some(p) = fr.feature.feature_params.as_mut() {
            match p {
                FeatureParams::StylisticSet(s) => {
                    let text = debug_name(noto, s.ui_name_id)?;
                    s.ui_name_id = names.add(&text, 256);
                }
                _ => return Err(format!("Noto {tag} feature {} has params", fr.feature_tag)),
            }
        }
        nfeats.push(fr);
    }
    if extra_feature.is_some() && !nfeats.iter().any(|f| f.feature_tag == Tag::new(b"nukt")) {
        return Err("Noto names no nukt for dev2 or deva".into());
    }
    let (feats, where_) = merged_features(built_features.feature_records.clone(), nfeats);
    let nwhere: HashMap<usize, u16> = nt
        .used
        .iter()
        .enumerate()
        .map(|(k, i)| (*i, where_[1][&k]))
        .collect();
    let mut scripts = built_scripts.script_records.clone();
    let fix = |ls: &mut LangSys, w: &HashMap<usize, u16>| -> Res<()> {
        for i in ls.feature_indices.iter_mut() {
            *i = *w
                .get(&usize::from(*i))
                .ok_or("a feature index past the list")?;
        }
        if ls.required_feature_index != 0xFFFF {
            ls.required_feature_index = *w
                .get(&usize::from(ls.required_feature_index))
                .ok_or("a required feature past the list")?;
        }
        Ok(())
    };
    for sr in scripts.iter_mut() {
        let sc = &mut *sr.script;
        if let Some(ls) = sc.default_lang_sys.as_mut() {
            fix(ls, &where_[0])?;
        }
        for r in sc.lang_sys_records.iter_mut() {
            fix(&mut r.lang_sys, &where_[0])?;
        }
    }
    for mut sr in nt.scripts {
        let sc = &mut *sr.script;
        if let Some(ls) = sc.default_lang_sys.as_mut() {
            fix(ls, &nwhere)?;
        }
        for r in sc.lang_sys_records.iter_mut() {
            fix(&mut r.lang_sys, &nwhere)?;
        }
        if scripts.iter().any(|s| s.script_tag == sr.script_tag) {
            return Err(format!("BUILT already has a {} script", sr.script_tag));
        }
        scripts.push(sr);
    }
    scripts.sort_by_key(|r| r.script_tag);
    Ok(Joined {
        scripts,
        features: feats,
        lookups,
    })
}

// ---- glyphs

/// A composite's component glyph IDs, renumbered in place.
fn remap_composite(data: &mut [u8], map: &dyn Fn(u16) -> Res<u16>) -> Res<()> {
    let be = |d: &[u8], i: usize| -> Res<u16> {
        d.get(i..i + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .ok_or_else(|| "glyf: a composite ends early".to_string())
    };
    let mut at = 10;
    loop {
        let flags = be(data, at)?;
        let g = map(be(data, at + 2)?)?;
        data[at + 2..at + 4].copy_from_slice(&g.to_be_bytes());
        at += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
        at += if flags & 0x0008 != 0 {
            2
        } else if flags & 0x0040 != 0 {
            4
        } else if flags & 0x0080 != 0 {
            8
        } else {
            0
        };
        if flags & 0x0020 == 0 {
            return Ok(());
        }
    }
}

/// One glyph's bytes as `glyf` stores them, the range loca gives.
fn glyph_data<'a>(f: &FontRef<'a>, g: u32) -> Res<&'a [u8]> {
    let loca = f.loca(None).map_err(e("loca"))?;
    let glyf = f.glyf().map_err(e("glyf"))?;
    let (a, b) = (
        loca.get_raw(g as usize).ok_or("loca: past the table")?,
        loca.get_raw(g as usize + 1).ok_or("loca: past the table")?,
    );
    glyf.offset_data()
        .as_bytes()
        .get(a as usize..b as usize)
        .ok_or_else(|| "glyf: a glyph past the table".to_string())
}

/// The bounding box head's and hhea's recalculations read, for every
/// glyph of the merged `glyf`: its stored box, none for an empty glyph.
struct Shape {
    bbox: Option<(i16, i16, i16, i16)>,
}

fn shapes(glyf: &[u8], loca: &[u32]) -> Res<Vec<Shape>> {
    let mut out = vec![];
    for w in loca.windows(2) {
        let d = &glyf[w[0] as usize..w[1] as usize];
        if d.is_empty() {
            out.push(Shape { bbox: None });
            continue;
        }
        let be = |i: usize| i16::from_be_bytes([d[i], d[i + 1]]);
        let contours = be(0);
        out.push(Shape {
            bbox: (contours != 0).then(|| (be(2), be(4), be(6), be(8))),
        });
    }
    Ok(out)
}

/// maxp's maxima over the merged glyphs, as fontTools' `maxp.recalc`
/// counts them: a composite's points and contours are its leaves', its
/// depth the deepest nesting, its elements its own components.
fn maxp_values(f: &FontRef) -> Res<[u16; 6]> {
    let glyf = f.glyf().map_err(e("glyf"))?;
    let loca = f.loca(None).map_err(e("loca"))?;
    let n = f.maxp().map_err(e("maxp"))?.num_glyphs();
    let mut memo: HashMap<u32, (u32, u32, u32)> = HashMap::new();
    fn walk(
        g: u32,
        glyf: &read_fonts::tables::glyf::Glyf,
        loca: &read_fonts::tables::loca::Loca,
        memo: &mut HashMap<u32, (u32, u32, u32)>,
        depth: u32,
    ) -> Res<(u32, u32, u32)> {
        if depth > 64 {
            return Err("glyf: components nest too deep".into());
        }
        if let Some(v) = memo.get(&g) {
            return Ok(*v);
        }
        let v = match loca.get_glyf(GlyphId::new(g), glyf).map_err(e("glyf"))? {
            None => (0, 0, 0),
            Some(ReadGlyph::Simple(s)) => (s.num_points() as u32, s.number_of_contours() as u32, 0),
            Some(ReadGlyph::Composite(c)) => {
                let (mut p, mut k, mut d) = (0, 0, 0);
                for comp in c.components() {
                    let (cp, ck, cd) = walk(comp.glyph.to_u32(), glyf, loca, memo, depth + 1)?;
                    p += cp;
                    k += ck;
                    d = d.max(cd + 1);
                }
                (p, k, d)
            }
        };
        memo.insert(g, v);
        Ok(v)
    }
    let mut m = [0u32; 6];
    for g in 0..u32::from(n) {
        match loca.get_glyf(GlyphId::new(g), &glyf).map_err(e("glyf"))? {
            None => {}
            Some(ReadGlyph::Simple(s)) => {
                m[0] = m[0].max(s.num_points() as u32);
                m[1] = m[1].max(s.number_of_contours() as u32);
            }
            Some(ReadGlyph::Composite(c)) => {
                let (p, k, d) = walk(g, &glyf, &loca, &mut memo, 0)?;
                m[2] = m[2].max(p);
                m[3] = m[3].max(k);
                m[4] = m[4].max(c.components().count() as u32);
                m[5] = m[5].max(d);
            }
        }
    }
    let mut out = [0u16; 6];
    for (o, v) in out.iter_mut().zip(m) {
        *o = u16::try_from(v).map_err(|_| "maxp: a maximum past 16 bits")?;
    }
    Ok(out)
}

// ---- the guard

/// What must not change: BUILT's objects, read out of a face, glyph by
/// glyph and lookup by lookup, up to BUILT's own glyph count, lookup counts
/// and mark sets.
#[derive(PartialEq, Eq, Debug)]
struct Objects {
    lines: BTreeMap<String, String>,
}

/// The extent of BUILT the guard reads: its glyph count, its GSUB lookup
/// count and lookup block, its GPOS lookup count, its mark sets and its
/// scripts.
struct Extent<'a> {
    glyphs: u32,
    gsub: usize,
    region: &'a [u8],
    gpos: usize,
    sets: usize,
    scripts: BTreeSet<String>,
}

/// BUILT's extent, its lookup block `region` as BUILT's GSUB stores it.
fn extent_of<'a>(built: &FontRef, region: &'a [u8]) -> Res<Extent<'a>> {
    let mut scripts = BTreeSet::new();
    for t in [
        built.gsub().map_err(e("GSUB"))?.script_list(),
        built.gpos().map_err(e("GPOS"))?.script_list(),
    ] {
        for r in t.map_err(e("scripts"))?.script_records() {
            scripts.insert(r.script_tag().to_string());
        }
    }
    Ok(Extent {
        glyphs: u32::from(built.maxp().map_err(e("maxp"))?.num_glyphs()),
        gsub: built
            .gsub()
            .map_err(e("GSUB"))?
            .lookup_list()
            .map_err(e("GSUB"))?
            .lookup_count() as usize,
        region,
        gpos: built
            .gpos()
            .map_err(e("GPOS"))?
            .lookup_list()
            .map_err(e("GPOS"))?
            .lookup_count() as usize,
        sets: built
            .gdef()
            .map_err(e("GDEF"))?
            .mark_glyph_sets_def()
            .transpose()
            .map_err(e("GDEF"))?
            .map_or(0, |m| m.mark_glyph_set_count() as usize),
        scripts,
    })
}

fn objects(f: &FontRef, x: &Extent) -> Res<Objects> {
    let (n, gpos, sets) = (x.glyphs, x.gpos, x.sets);
    let mut lines = BTreeMap::new();
    let hmtx = f.hmtx().map_err(e("hmtx"))?;
    for g in 0..n {
        let d = glyph_data(f, g)?;
        let len = if d.is_empty() {
            0
        } else {
            crate::stage::unpadded_len(d)?
        };
        let gid = GlyphId::new(g);
        lines.insert(
            format!("glyph {g}"),
            format!(
                "{:02x?} {:?} {:?}",
                &d[..len],
                hmtx.advance(gid),
                hmtx.side_bearing(gid)
            ),
        );
    }
    let cmap = f.cmap().map_err(e("cmap"))?;
    for rec in cmap.encoding_records() {
        let sub = rec.subtable(cmap.offset_data()).map_err(e("cmap"))?;
        let key = format!("{}/{}", rec.platform_id() as u16, rec.encoding_id());
        for (c, g) in sub.iter() {
            if g.to_u32() < n {
                lines.insert(format!("cmap {key} U+{c:04X}"), g.to_u32().to_string());
            }
        }
    }
    // GSUB's lookups as bytes: the splice keeps them so.
    let (nsub, region) = (x.gsub, x.region);
    let t = f.table_data(Tag::new(b"GSUB")).ok_or("no GSUB")?.as_bytes();
    let (start, offs, _) = lookup_region(t)?;
    lines.insert(
        "GSUB lookups".into(),
        format!("{} {}", offs.len() >= nsub, &t[start..].starts_with(region)),
    );
    // GPOS's lookups and every language system's features, as the
    // comparator expands them.
    let samples = crate::var::Samples::of(f, f);
    for (tag, dump) in [
        ("GSUB", crate::layout::gsub(f, &samples).map_err(e("GSUB"))?),
        ("GPOS", crate::layout::gpos(f, &samples).map_err(e("GPOS"))?),
    ] {
        for (locus, value) in dump.entries() {
            let keep = if let Some(rest) = locus.strip_prefix("lookup ") {
                let i: usize = rest
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(usize::MAX);
                tag == "GPOS" && i < gpos
            } else if let Some(rest) = locus.strip_prefix("script ") {
                x.scripts.contains(rest.split(' ').next().unwrap_or(""))
            } else {
                false
            };
            if keep {
                lines.insert(format!("{tag} {locus}"), value);
            }
        }
    }
    let gdef = f.gdef().map_err(e("GDEF"))?;
    if let Some(c) = gdef.glyph_class_def() {
        let c = c.map_err(e("GDEF"))?;
        for g in 0..n {
            lines.insert(
                format!("class {g}"),
                c.get(GlyphId16::new(g as u16)).to_string(),
            );
        }
    }
    if let Some(sets_def) = gdef.mark_glyph_sets_def() {
        let sets_def = sets_def.map_err(e("GDEF"))?;
        for (i, c) in sets_def.coverages().iter().enumerate().take(sets) {
            let c = c.map_err(e("GDEF"))?;
            let gl: Vec<u32> = c.iter().map(|g| g.to_u32()).filter(|g| *g < n).collect();
            lines.insert(format!("mark set {i}"), format!("{gl:?}"));
        }
    }
    let name = f.name().map_err(e("name"))?;
    for r in name.name_record() {
        if [0, 7].contains(&r.name_id().to_u16()) {
            continue;
        }
        let s: String = r
            .string(name.string_data())
            .map_err(e("name"))?
            .chars()
            .collect();
        lines.insert(
            format!(
                "name {} {} {} {}",
                r.name_id(),
                r.platform_id(),
                r.encoding_id(),
                r.language_id()
            ),
            s,
        );
    }
    Ok(Objects { lines })
}

/// The objects of BUILT that differ in OUT, by name; empty when none does.
fn unchanged(built: &FontRef, out: &FontRef, x: &Extent) -> Res<Vec<String>> {
    let before = objects(built, x)?;
    let after = objects(out, x)?;
    let mut changed = vec![];
    for (k, v) in &before.lines {
        if after.lines.get(k) != Some(v) {
            changed.push(k.clone());
        }
    }
    Ok(changed)
}

/// A STAT design axis for [`merge`] to write: its tag, its name, its
/// ordering, and its values (each a value, a name and the flags).
type StatAxis = (Tag, String, u16, Vec<(f64, String, u16)>);

/// The merge, as the module documentation describes it.
pub fn merge(built_bytes: &[u8], noto_bytes: &[u8], style_name: &str, epoch: i64) -> Res<Merged> {
    let (weight, ital) = style(style_name)?;
    let built = FontRef::new(built_bytes).map_err(e("BUILT"))?;
    let noto = FontRef::new(noto_bytes).map_err(e("NOTO"))?;
    for (f, which) in [(&built, "BUILT"), (&noto, "NOTO")] {
        let upem = f.head().map_err(e("head"))?.units_per_em();
        if upem != 1000 {
            return Err(format!("{which}: unitsPerEm {upem}"));
        }
    }
    let nfvar = noto.fvar().map_err(e("Noto fvar"))?;
    let axes: Vec<_> = nfvar.axes().map_err(e("Noto fvar"))?.iter().collect();
    let axis_ok = axes.len() == 1
        && axes[0].axis_tag() == Tag::new(b"wdth")
        && axes[0].min_value().to_f64() == 62.5
        && axes[0].default_value().to_f64() == 100.0
        && axes[0].max_value().to_f64() == 100.0;
    if !axis_ok {
        return Err("Noto's axes are not wdth 62.5-100 alone".into());
    }
    for tag in [b"fvar", b"gvar", b"HVAR", b"avar", b"STAT", b"MVAR"] {
        if built.table_data(Tag::new(tag)).is_some() {
            return Err(format!("BUILT already has {}", Tag::new(tag)));
        }
    }
    for (f, which) in [(&built, "BUILT"), (&noto, "Noto")] {
        if f.gsub()
            .map_err(e("GSUB"))?
            .feature_variations_offset()
            .is_some()
            || f.gpos()
                .map_err(e("GPOS"))?
                .feature_variations_offset()
                .is_some()
        {
            return Err(format!("{which} has feature variations"));
        }
    }
    let windows_name = |id: u16| -> Res<Vec<String>> {
        let name = noto.name().map_err(e("Noto name"))?;
        name.name_record()
            .iter()
            .filter(|r| r.name_id().to_u16() == id && r.platform_id() == 3)
            .map(|r| {
                Ok(r.string(name.string_data())
                    .map_err(e("name"))?
                    .chars()
                    .collect())
            })
            .collect()
    };
    if windows_name(0)? != [NOTO_COPYRIGHT] || windows_name(7)? != [NOTO_TRADEMARK] {
        return Err("Noto's nameID 0 or 7 is not the pinned notice".into());
    }

    // ---- the glyphs to copy, and what each is called in OUT
    let border = glyph_names(&built)?;
    let nborder = border.len() as u32;
    let bnames: HashMap<&str, u32> = border
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let norder = glyph_names(&noto)?;
    let bcm = best_cmap(&built)?;
    let ncm = best_cmap(&noto)?;
    let mut keep: BTreeSet<u32> = ncm
        .iter()
        .filter(|(c, _)| !bcm.contains_key(c))
        .map(|(_, g)| g.to_u32())
        .collect();
    let read_nsub = noto.gsub().map_err(e("Noto GSUB"))?;
    let read_npos = noto.gpos().map_err(e("Noto GPOS"))?;
    let mut nsub: Gsub = read_nsub.to_owned_table();
    let mut npos: Gpos = read_npos.to_owned_table();
    let ncount = norder.len() as u32;
    {
        let mut named = |g: &mut GlyphId16| -> Res<()> {
            if g.to_u32() < ncount {
                keep.insert(g.to_u32());
            }
            Ok(())
        };
        let mut probe_sub = nsub.clone();
        let mut probe_pos = npos.clone();
        for lk in probe_sub
            .lookup_list
            .as_mut()
            .ok_or("Noto GSUB: no lookups")?
            .lookups
            .iter_mut()
        {
            lk.glyphs(&mut named)?;
        }
        for lk in probe_pos
            .lookup_list
            .as_mut()
            .ok_or("Noto GPOS: no lookups")?
            .lookups
            .iter_mut()
        {
            lk.glyphs(&mut named)?;
        }
    }
    let space = |order: &[String]| order.iter().position(|n| n == "space").map(|i| i as u32);
    let nspace = space(&norder);
    if let Some(s) = nspace {
        keep.remove(&s);
    }
    if let Some(nd) = norder.iter().position(|n| n == ".notdef") {
        keep.remove(&(nd as u32));
    }
    // Components, closed over.
    let mut stack: Vec<u32> = keep.iter().copied().collect();
    while let Some(g) = stack.pop() {
        let d = glyph_data(&noto, g)?;
        if d.len() >= 10 && i16::from_be_bytes([d[0], d[1]]) < 0 {
            let loca = noto.loca(None).map_err(e("loca"))?;
            let glyf = noto.glyf().map_err(e("glyf"))?;
            if let Some(ReadGlyph::Composite(c)) =
                loca.get_glyf(GlyphId::new(g), &glyf).map_err(e("glyf"))?
            {
                for comp in c.components() {
                    let k = comp.glyph.to_u32();
                    if keep.insert(k) {
                        stack.push(k);
                    }
                }
            }
        }
    }
    if nspace.is_some_and(|s| keep.contains(&s)) {
        return Err("Noto's space is a component; it cannot be BUILT's space too".into());
    }
    let kept: Vec<u32> = (0..ncount).filter(|g| keep.contains(g)).collect();
    let new_name: HashMap<u32, String> = kept
        .iter()
        .map(|&g| {
            let n = &norder[g as usize];
            (
                g,
                if bnames.contains_key(n.as_str()) {
                    format!("{n}{SUFFIX}")
                } else {
                    n.clone()
                },
            )
        })
        .collect();
    let bspace = *bnames
        .get("space")
        .ok_or("BUILT has no glyph named space")?;
    {
        let mut all: BTreeSet<&str> = border.iter().map(String::as_str).collect();
        for n in new_name.values() {
            if !all.insert(n.as_str()) {
                return Err(format!("a glyph name would repeat: {n}"));
            }
        }
    }
    let to_out: HashMap<u32, u16> = kept
        .iter()
        .enumerate()
        .map(|(k, g)| (*g, (nborder as usize + k) as u16))
        .chain(nspace.map(|s| (s, bspace as u16)))
        .collect();
    let total = nborder as usize + kept.len();
    if total > 0xFFFF {
        return Err("too many glyphs".into());
    }
    let mut remap = |g: &mut GlyphId16| -> Res<()> {
        *g = GlyphId16::new(*to_out.get(&g.to_u32()).ok_or_else(|| {
            format!(
                "a Noto lookup names a glyph not copied: {}",
                norder[g.to_u32() as usize]
            )
        })?);
        Ok(())
    };

    // ---- glyf, loca, hmtx
    let mut glyf: Vec<u8> = vec![];
    let mut loca: Vec<u32> = vec![0];
    for g in 0..nborder {
        glyf.extend(glyph_data(&built, g)?);
        loca.push(glyf.len() as u32);
    }
    for &g in &kept {
        let mut d = glyph_data(&noto, g)?.to_vec();
        if d.len() >= 10 && i16::from_be_bytes([d[0], d[1]]) < 0 {
            remap_composite(&mut d, &|c| {
                to_out
                    .get(&u32::from(c))
                    .copied()
                    .ok_or_else(|| format!("a component not copied: {c}"))
            })?;
        }
        // Without the bake's padding, as the stage stores its glyphs.
        if !d.is_empty() {
            d.truncate(crate::stage::unpadded_len(&d)?);
        }
        glyf.extend(d);
        loca.push(glyf.len() as u32);
    }
    let bh = built.hmtx().map_err(e("hmtx"))?;
    let nh = noto.hmtx().map_err(e("Noto hmtx"))?;
    let mut metrics: Vec<(u16, i16)> = vec![];
    for g in 0..nborder {
        let gid = GlyphId::new(g);
        metrics.push((
            bh.advance(gid).ok_or("hmtx: no advance")?,
            bh.side_bearing(gid).ok_or("hmtx: no side bearing")?,
        ));
    }
    for &g in &kept {
        let gid = GlyphId::new(g);
        metrics.push((
            nh.advance(gid).ok_or("Noto hmtx: no advance")?,
            nh.side_bearing(gid).ok_or("Noto hmtx: no side bearing")?,
        ));
    }
    let last = metrics.last().map_or(0, |m| m.0);
    let mut long = metrics.len();
    while long > 1 && metrics[long - 2].0 == last {
        long -= 1;
    }
    let out_hmtx = Hmtx::new(
        metrics[..long]
            .iter()
            .map(|(a, l)| LongMetric::new(*a, *l))
            .collect(),
        metrics[long..].iter().map(|(_, l)| *l).collect(),
    );
    let sh = shapes(&glyf, &loca)?;

    // ---- cmap
    let mut cmap = bcm.clone();
    let mut added = 0usize;
    for (c, g) in &ncm {
        if !bcm.contains_key(c) {
            let o = *to_out
                .get(&g.to_u32())
                .ok_or("cmap: a code point's glyph not copied")?;
            cmap.insert(*c, GlyphId::new(u32::from(o)));
            added += 1;
        }
    }
    let out_cmap = Cmap::from_mappings(
        cmap.iter()
            .map(|(u, g)| Ok((char::from_u32(*u).ok_or("cmap: not a char")?, *g)))
            .collect::<Res<Vec<_>>>()?,
    )
    .map_err(e("cmap"))?;

    // ---- names
    let mut names = Names {
        records: {
            let n: Name = built.name().map_err(e("name"))?.to_owned_table();
            n.name_record
        },
    };

    // ---- GDEF
    let read_bgdef = built.gdef().map_err(e("GDEF"))?;
    let mut gdef: Gdef = read_bgdef.to_owned_table();
    let read_ngdef = noto.gdef().map_err(e("Noto GDEF"))?;
    let ngdef: Gdef = read_ngdef.to_owned_table();
    if ngdef.attach_list.is_some()
        || ngdef.lig_caret_list.is_some()
        || ngdef.mark_attach_class_def.is_some()
    {
        return Err("Noto GDEF has an AttachList, LigCaretList or MarkAttachClassDef".into());
    }
    let mut classes: BTreeMap<u16, u16> = match gdef.glyph_class_def.as_ref() {
        Some(c) => c
            .iter()
            .filter(|(_, k)| *k != 0)
            .map(|(g, k)| (g.to_u32() as u16, k as u16))
            .collect(),
        None => BTreeMap::new(),
    };
    if let Some(c) = ngdef.glyph_class_def.as_ref() {
        for (g, k) in c.iter() {
            if k != 0
                && let (Some(o), true) = (to_out.get(&g.to_u32()), keep.contains(&g.to_u32()))
            {
                classes.insert(*o, k as u16);
            }
        }
    }
    gdef.glyph_class_def = Some(
        classes
            .into_iter()
            .map(|(g, k)| (GlyphId16::new(g), k))
            .collect::<ClassDef>(),
    )
    .into();
    let mut sets: MarkGlyphSets = gdef
        .mark_glyph_sets_def
        .as_ref()
        .cloned()
        .unwrap_or_default();
    let nsets = sets.coverages.len();
    if let Some(ns) = ngdef.mark_glyph_sets_def.as_ref() {
        for cov in &ns.coverages {
            let v: Vec<GlyphId16> = cov
                .iter()
                .filter(|g| keep.contains(&g.to_u32()))
                .map(|g| GlyphId16::new(to_out[&g.to_u32()]))
                .collect();
            sets.coverages.push(CoverageTable::from(v).into());
        }
    }
    gdef.mark_glyph_sets_def = Some(sets).into();
    gdef.item_var_store = ngdef.item_var_store.clone();

    // ---- GSUB and GPOS
    let read_bsub = built.gsub().map_err(e("GSUB"))?;
    let read_bpos = built.gpos().map_err(e("GPOS"))?;
    let bsub: Gsub = read_bsub.to_owned_table();
    let bpos: Gpos = read_bpos.to_owned_table();
    let nold_sub = bsub.lookup_list.as_ref().map_or(0, |l| l.lookups.len());
    let dotted = *bnames.get("uni25CC").ok_or("BUILT has no uni25CC")?;
    let ndotted = norder
        .iter()
        .position(|n| n == "uni25CC")
        .ok_or("Noto has no uni25CC")? as u32;
    let swap = SubstitutionLookup::Single(Lookup::new(
        LookupFlag::empty(),
        vec![SingleSubst::Format2(SingleSubstFormat2::new(
            vec![GlyphId16::new(dotted as u16)].into(),
            vec![GlyphId16::new(
                *to_out.get(&ndotted).ok_or("Noto's uni25CC is not copied")?,
            )],
        ))],
    ));
    let nsets16 = u16::try_from(nsets).map_err(|_| "mark sets")?;
    let empty_scripts = ScriptList::default();
    let empty_features = FeatureList::default();
    let take_lookups = |l: Option<&mut LookupList<SubstitutionLookup>>| -> Vec<SubstitutionLookup> {
        l.map(|l| l.lookups.drain(..).map(|o| o.into_inner()).collect())
            .unwrap_or_default()
    };
    let take_pos = |l: Option<&mut LookupList<PositionLookup>>| -> Vec<PositionLookup> {
        l.map(|l| l.lookups.drain(..).map(|o| o.into_inner()).collect())
            .unwrap_or_default()
    };
    let mut bsub_m = bsub.clone();
    let mut bpos_m = bpos.clone();
    let sub = join(
        "GSUB",
        bsub.script_list.as_ref().unwrap_or(&empty_scripts),
        bsub.feature_list.as_ref().unwrap_or(&empty_features),
        take_lookups(bsub_m.lookup_list.as_mut()),
        nsub.script_list.as_ref().unwrap_or(&empty_scripts),
        &nsub.feature_list.as_ref().cloned().unwrap_or_default(),
        take_lookups(nsub.lookup_list.as_mut()),
        nsets16,
        &mut remap,
        Some((swap, "nukt")),
        &mut names,
        &noto,
    )?;
    let pos = join(
        "GPOS",
        bpos.script_list.as_ref().unwrap_or(&empty_scripts),
        bpos.feature_list.as_ref().unwrap_or(&empty_features),
        take_pos(bpos_m.lookup_list.as_mut()),
        npos.script_list.as_ref().unwrap_or(&empty_scripts),
        &npos.feature_list.as_ref().cloned().unwrap_or_default(),
        take_pos(npos.lookup_list.as_mut()),
        nsets16,
        &mut remap,
        None,
        &mut names,
        &noto,
    )?;
    let max_ctx = max_context(&sub.lookups, &pos.lookups)?;
    let old_gsub = built
        .table_data(Tag::new(b"GSUB"))
        .ok_or("BUILT has no GSUB")?
        .as_bytes();
    let (start, _, _) = lookup_region(old_gsub)?;
    let old_region = old_gsub[start..].to_vec();
    let nlook = sub.lookups.len();
    let appended: Vec<SubstitutionLookup> = sub.lookups.into_iter().skip(nold_sub).collect();
    let (gsub_bytes, _) = spliced(old_gsub, &sub.scripts, &sub.features, appended, nlook)?;
    let mut gpos = bpos.clone();
    gpos.script_list = Some(ScriptList::new(pos.scripts)).into();
    gpos.feature_list = Some(FeatureList::new(pos.features)).into();
    gpos.lookup_list = Some(LookupList::new(pos.lookups)).into();

    // ---- variations
    let axis = &axes[0];
    let axis_name = names.add(&debug_name(&noto, axis.axis_name_id())?, 256);
    let instances: Vec<_> = nfvar
        .instances()
        .map_err(e("Noto fvar"))?
        .iter()
        .collect::<Result<_, _>>()
        .map_err(e("Noto fvar"))?;
    let inst = instances.first().ok_or("Noto has no named instance")?;
    if inst.coordinates.len() != 1 || inst.coordinates[0].get().to_f64() != 100.0 {
        return Err("Noto's named instance is not at wdth 100".into());
    }
    let mut fvar = vec![];
    for v in [1u16, 0, 16, 2, 1, 20, 1, 10] {
        fvar.extend(v.to_be_bytes());
    }
    fvar.extend(b"wdth");
    for v in [axis.min_value(), axis.default_value(), axis.max_value()] {
        fvar.extend(v.to_bits().to_be_bytes());
    }
    fvar.extend(HIDDEN_AXIS.to_be_bytes());
    fvar.extend(axis_name.to_u16().to_be_bytes());
    fvar.extend(2u16.to_be_bytes());
    fvar.extend(inst.flags.to_be_bytes());
    fvar.extend(inst.coordinates[0].get().to_bits().to_be_bytes());
    fvar.extend(6u16.to_be_bytes());

    let avar = noto
        .table_data(Tag::new(b"avar"))
        .ok_or("Noto has no avar")?
        .as_bytes()
        .to_vec();

    // gvar: Noto's shared tuples and each kept glyph's data as Noto
    // stores it; BUILT's glyphs vary by nothing.
    let ngvar = noto
        .table_data(Tag::new(b"gvar"))
        .ok_or("Noto has no gvar")?
        .as_bytes();
    let be16 = |d: &[u8], i: usize| -> Res<usize> { u16_at(d, i) };
    let be32 = |d: &[u8], i: usize| -> Res<usize> {
        d.get(i..i + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| "gvar: a field past the table".to_string())
    };
    if be16(ngvar, 4)? != 1 {
        return Err("Noto's gvar is not over one axis".into());
    }
    let shared_n = be16(ngvar, 6)?;
    let shared_at = be32(ngvar, 8)?;
    let nglyphs = be16(ngvar, 12)?;
    let long_offsets = be16(ngvar, 14)? & 1 != 0;
    let array_at = be32(ngvar, 16)?;
    let goff = |g: usize| -> Res<usize> {
        Ok(array_at
            + if long_offsets {
                be32(ngvar, 20 + 4 * g)?
            } else {
                2 * be16(ngvar, 20 + 2 * g)?
            })
    };
    if nglyphs != ncount as usize {
        return Err("Noto's gvar glyph count".into());
    }
    let shared = ngvar
        .get(shared_at..shared_at + 2 * shared_n)
        .ok_or("gvar: shared tuples past the table")?;
    let mut gvar = vec![];
    let offsets_len = 4 * (total + 1);
    let data_at = 20 + offsets_len + shared.len();
    for v in [1u16, 0, 1, shared_n as u16] {
        gvar.extend(v.to_be_bytes());
    }
    gvar.extend(((20 + offsets_len) as u32).to_be_bytes());
    gvar.extend((total as u16).to_be_bytes());
    gvar.extend(1u16.to_be_bytes());
    gvar.extend((data_at as u32).to_be_bytes());
    let mut data: Vec<u8> = vec![];
    let mut offs = vec![0u32; nborder as usize + 1];
    for &g in &kept {
        data.extend(
            ngvar
                .get(goff(g as usize)?..goff(g as usize + 1)?)
                .ok_or("gvar: data past the table")?,
        );
        offs.push(data.len() as u32);
    }
    for o in &offs {
        gvar.extend(o.to_be_bytes());
    }
    gvar.extend(shared);
    gvar.extend(data);

    // HVAR: Noto's, its advance map written again: BUILT's glyphs to a row
    // of the store whose deltas are all 0, which OpenType's common formats
    // call functionally the same as NO_VARIATION_INDEX and which takes one
    // byte an entry less; the kept glyphs to theirs.
    let nhvar = noto.hvar().map_err(e("Noto HVAR"))?;
    if !nhvar.lsb_mapping_offset().is_null() || !nhvar.rsb_mapping_offset().is_null() {
        return Err("Noto HVAR has side-bearing maps".into());
    }
    let amap = nhvar
        .advance_width_mapping()
        .ok_or("Noto HVAR has no advance map")?
        .map_err(e("Noto HVAR"))?;
    let store = nhvar.item_variation_store().map_err(e("Noto HVAR"))?;
    let mut zero: u32 = 0xFFFF_FFFF;
    'find: for (outer, vd) in store.item_variation_data().iter().enumerate() {
        let Some(vd) = vd else { continue };
        let vd = vd.map_err(e("Noto HVAR"))?;
        for inner in 0..vd.item_count() {
            if vd.delta_set(inner).all(|d| d == 0) {
                zero = ((outer as u32) << 16) | u32::from(inner);
                break 'find;
            }
        }
    }
    let mut entries: Vec<u32> = vec![zero; nborder as usize];
    for &g in &kept {
        let i = amap.get(g).map_err(e("Noto HVAR"))?;
        entries.push((u32::from(i.outer) << 16) | u32::from(i.inner));
    }
    // The map's entry format is the smallest that holds every entry, as
    // fontTools' buildVarIdxMap packs it.
    let hvar = write_fonts::tables::hvar::Hvar::new(
        nhvar
            .item_variation_store()
            .map_err(e("Noto HVAR"))?
            .to_owned_table(),
        Some(
            entries
                .iter()
                .copied()
                .collect::<write_fonts::tables::variations::DeltaSetIndexMap>(),
        ),
        None,
        None,
    );

    // STAT: each fvar axis and named instance has its axis record and
    // value (OpenType fvar), and the family's weight and slope beside them,
    // as fontTools' buildStatTable writes them.
    let nstat = noto.stat().map_err(e("Noto STAT"))?;
    let ndesign: Vec<_> = nstat
        .design_axes()
        .map_err(e("Noto STAT"))?
        .iter()
        .map(|a| a.axis_tag())
        .collect();
    let mut widths: Vec<(f64, String, u16)> = vec![];
    if let Some(vals) = nstat.offset_to_axis_values() {
        let vals = vals.map_err(e("Noto STAT"))?;
        for v in vals.axis_values().iter() {
            if let read_fonts::tables::stat::AxisValue::Format1(v) = v.map_err(e("Noto STAT"))?
                && ndesign.get(v.axis_index() as usize) == Some(&Tag::new(b"wdth"))
            {
                widths.push((
                    v.value().to_f64(),
                    debug_name(&noto, v.value_name_id())?,
                    v.flags().bits() & ELIDABLE,
                ));
            }
        }
    }
    if widths.iter().map(|w| w.0).collect::<Vec<_>>() != [62.5, 75.0, 87.5, 100.0] {
        return Err(format!("Noto's width values {widths:?}"));
    }
    let wdth_name = names
        .records
        .iter()
        .find(|r| r.name_id == axis_name && (r.platform_id, r.language_id) == (1, 0))
        .map(|r| r.string.as_str().to_string())
        .ok_or("the axis name")?;
    let stat_axes: Vec<StatAxis> = vec![
        (
            Tag::new(b"wght"),
            "Weight".into(),
            0,
            vec![(
                f64::from(weight),
                if weight == 400 { "Regular" } else { "Bold" }.into(),
                if weight == 400 { ELIDABLE } else { 0 },
            )],
        ),
        (Tag::new(b"wdth"), wdth_name, 1, widths),
        (
            Tag::new(b"ital"),
            "Italic".into(),
            2,
            vec![(
                f64::from(ital),
                if ital == 1 { "Italic" } else { "Upright" }.into(),
                if ital == 1 { 0 } else { ELIDABLE },
            )],
        ),
    ];
    let mut design = vec![];
    let mut values = vec![];
    for (k, (tag, name, ordering, vals)) in stat_axes.iter().enumerate() {
        let id = names.add(name, 256);
        design.extend(tag.to_be_bytes());
        design.extend(id.to_u16().to_be_bytes());
        design.extend(ordering.to_be_bytes());
        for (value, vname, flags) in vals {
            let vid = names.add(vname, 0);
            let mut v = vec![];
            v.extend(1u16.to_be_bytes());
            v.extend((k as u16).to_be_bytes());
            v.extend(flags.to_be_bytes());
            v.extend(vid.to_u16().to_be_bytes());
            v.extend(((value * 65536.0).round() as i32).to_be_bytes());
            values.push(v);
        }
    }
    let mut stat = vec![];
    for v in [1u16, 1, 8, stat_axes.len() as u16] {
        stat.extend(v.to_be_bytes());
    }
    stat.extend(20u32.to_be_bytes());
    stat.extend((values.len() as u16).to_be_bytes());
    stat.extend((20 + design.len() as u32).to_be_bytes());
    stat.extend(2u16.to_be_bytes());
    stat.extend(&design);
    let mut body = 2 * values.len();
    let mut tables: Vec<u8> = vec![];
    for v in &values {
        stat.extend((body as u16).to_be_bytes());
        body += v.len();
        tables.extend(v);
    }
    stat.extend(tables);

    // ---- the notices, and the summaries the new glyphs change
    for r in names.records.iter_mut() {
        if r.name_id.to_u16() == 0 {
            let s = format!(
                "{} Devanagari glyphs from Noto Sans Devanagari 2.007: {NOTO_COPYRIGHT}.",
                r.string.as_str()
            );
            r.string = s.into();
        } else if r.name_id.to_u16() == 7 {
            let s = format!("{} {NOTO_TRADEMARK}", r.string.as_str());
            r.string = s.into();
        }
    }
    names.records.sort_by_key(|r| {
        (
            r.platform_id,
            r.encoding_id,
            r.language_id,
            r.name_id.to_u16(),
        )
    });
    let out_name = Name::new(names.records.clone());

    let mut os2: Os2 = built.os2().map_err(e("OS/2"))?.to_owned_table();
    os2.ul_unicode_range_1 |= 1 << DEVANAGARI_RANGE_BIT;
    os2.us_max_context = Some(max_ctx);
    // "the arithmetic average of the escapement (width) of all non-zero
    // width glyphs in the font" (OpenType OS/2, xAvgCharWidth), rounded as
    // Python's round does: half to even.
    let widths: Vec<f64> = metrics
        .iter()
        .filter(|m| m.0 != 0)
        .map(|m| f64::from(m.0))
        .collect();
    os2.x_avg_char_width =
        (widths.iter().sum::<f64>() / widths.len() as f64).round_ties_even() as i16;

    let mut out_post = Post::new_v2(
        border
            .iter()
            .map(String::as_str)
            .chain(kept.iter().map(|g| new_name[g].as_str())),
    );
    let stock_post: Post = built.post().map_err(e("post"))?.to_owned_table();
    out_post.italic_angle = stock_post.italic_angle;
    out_post.underline_position = stock_post.underline_position;
    out_post.underline_thickness = stock_post.underline_thickness;
    // "Set to 0 if the font is proportionally spaced" (OpenType post,
    // isFixedPitch): the Devanagari is; PANOSE, which classifies the Latin
    // design, keeps Monospaced.
    out_post.is_fixed_pitch = 0;
    out_post.min_mem_type42 = stock_post.min_mem_type42;
    out_post.max_mem_type42 = stock_post.max_mem_type42;
    out_post.min_mem_type1 = stock_post.min_mem_type1;
    out_post.max_mem_type1 = stock_post.max_mem_type1;

    // head, hhea and maxp, recomputed as fontTools' save recomputes them.
    let mut head: Head = built.head().map_err(e("head"))?.to_owned_table();
    let drawn: Vec<(usize, (i16, i16, i16, i16))> = sh
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.bbox.map(|b| (i, b)))
        .collect();
    let (mut x0, mut y0, mut x1, mut y1) = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
    for (_, b) in &drawn {
        x0 = x0.min(b.0);
        y0 = y0.min(b.1);
        x1 = x1.max(b.2);
        y1 = y1.max(b.3);
    }
    if !drawn.is_empty() {
        (head.x_min, head.y_min, head.x_max, head.y_max) = (x0, y0, x1, y1);
    }
    let lsb_bit = write_fonts::tables::head::Flags::LSB_AT_X_0;
    if drawn.iter().all(|(i, b)| metrics[*i].1 == b.0) {
        head.flags |= lsb_bit;
    } else {
        head.flags &= !lsb_bit;
    }
    head.modified = read_fonts::types::LongDateTime::new(epoch + 2_082_844_800);
    head.index_to_loc_format = 1;

    let mut hhea: Hhea = built.hhea().map_err(e("hhea"))?.to_owned_table();
    hhea.number_of_h_metrics = long as u16;
    hhea.advance_width_max = metrics.iter().map(|m| m.0).max().unwrap_or(0).into();
    if !drawn.is_empty() {
        let w = |b: &(i16, i16, i16, i16)| i32::from(b.2) - i32::from(b.0);
        let lsb = drawn
            .iter()
            .map(|(i, _)| i32::from(metrics[*i].1))
            .min()
            .unwrap_or(0);
        let rsb = drawn
            .iter()
            .map(|(i, b)| i32::from(metrics[*i].0) - i32::from(metrics[*i].1) - w(b))
            .min()
            .unwrap_or(0);
        let ext = drawn
            .iter()
            .map(|(i, b)| i32::from(metrics[*i].1) + w(b))
            .max()
            .unwrap_or(0);
        hhea.min_left_side_bearing = (lsb as i16).into();
        hhea.min_right_side_bearing = (rsb as i16).into();
        hhea.x_max_extent = (ext as i16).into();
    }

    let loca_bytes: Vec<u8> = loca.iter().flat_map(|o| o.to_be_bytes()).collect();
    let assemble = |maxp: &Maxp| -> Res<Vec<u8>> {
        let mut fb = FontBuilder::new();
        fb.add_raw(Tag::new(b"glyf"), glyf.clone());
        fb.add_raw(Tag::new(b"loca"), loca_bytes.clone());
        fb.add_table(&head).map_err(e("head"))?;
        fb.add_table(&hhea).map_err(e("hhea"))?;
        fb.add_table(maxp).map_err(e("maxp"))?;
        fb.add_table(&out_hmtx).map_err(e("hmtx"))?;
        fb.add_table(&out_cmap).map_err(e("cmap"))?;
        fb.add_table(&os2).map_err(e("OS/2"))?;
        fb.add_table(&out_name).map_err(e("name"))?;
        fb.add_table(&out_post).map_err(e("post"))?;
        fb.add_table(&gdef).map_err(e("GDEF"))?;
        fb.add_raw(Tag::new(b"GSUB"), gsub_bytes.clone());
        fb.add_table(&gpos).map_err(e("GPOS"))?;
        fb.add_raw(Tag::new(b"fvar"), fvar.clone());
        fb.add_raw(Tag::new(b"avar"), avar.clone());
        fb.add_raw(Tag::new(b"gvar"), gvar.clone());
        fb.add_table(&hvar).map_err(e("HVAR"))?;
        fb.add_raw(Tag::new(b"STAT"), stat.clone());
        for t in [b"gasp", b"cvt ", b"fpgm", b"prep"] {
            if let Some(d) = built.table_data(Tag::new(t)) {
                fb.add_raw(Tag::new(t), d.as_bytes().to_vec());
            }
        }
        Ok(fb.build())
    };
    let mut maxp: Maxp = built.maxp().map_err(e("maxp"))?.to_owned_table();
    maxp.num_glyphs = total as u16;
    let first = assemble(&maxp)?;
    let m = maxp_values(&FontRef::new(&first).map_err(e("OUT"))?)?;
    maxp.max_points = Some(m[0]);
    maxp.max_contours = Some(m[1]);
    maxp.max_composite_points = Some(m[2]);
    maxp.max_composite_contours = Some(m[3]);
    maxp.max_component_elements = Some(m[4]);
    maxp.max_component_depth = Some(m[5]);
    let font = assemble(&maxp)?;
    let have: Vec<Tag> = built
        .table_directory()
        .table_records()
        .iter()
        .map(|r| r.tag())
        .collect();
    const WRITTEN: &[&[u8; 4]] = &[
        b"glyf", b"loca", b"head", b"hhea", b"maxp", b"hmtx", b"cmap", b"OS/2", b"name", b"post",
        b"GDEF", b"GSUB", b"GPOS", b"gasp", b"cvt ", b"fpgm", b"prep",
    ];
    if let Some(t) = have
        .iter()
        .find(|t| !WRITTEN.iter().any(|w| Tag::new(w) == **t))
    {
        return Err(format!("BUILT: table {t} is not one this merge writes"));
    }

    let out = FontRef::new(&font).map_err(e("OUT"))?;
    let extent = extent_of(&built, &old_region)?;
    let changed = unchanged(&built, &out, &extent)?;
    if !changed.is_empty() {
        return Err(format!(
            "{} objects of BUILT changed, nothing written: {:?}",
            changed.len(),
            &changed[..changed.len().min(10)]
        ));
    }
    Ok(Merged {
        font,
        kept: kept.len(),
        code_points: added,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path),
        )
        .unwrap()
    }

    /// The Regular stage, and the committed Regular, which the merge wrote
    /// from it (`tests/merge.rs` holds them so).
    fn inputs() -> (Vec<u8>, Vec<u8>) {
        let base = read("baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
        let src = String::from_utf8(read(
            "baud-app/fonts/glyphs/languages/hebrew/stage/Regular.txt",
        ))
        .unwrap();
        let stage = crate::stage::build(&base, &src).unwrap();
        (stage, read("baud-app/fonts/built/Regular.ttf"))
    }

    /// Where table `tag` lies in `font`.
    fn range(font: &[u8], tag: &[u8; 4]) -> std::ops::Range<usize> {
        let f = FontRef::new(font).unwrap();
        let r = f
            .table_directory()
            .table_records()
            .iter()
            .find(|r| r.tag() == Tag::new(tag))
            .unwrap();
        r.offset() as usize..r.offset() as usize + r.length() as usize
    }

    fn changed(stage: &[u8], out: &[u8]) -> Vec<String> {
        let built = FontRef::new(stage).unwrap();
        let t = built.table_data(Tag::new(b"GSUB")).unwrap().as_bytes();
        let (start, _, _) = lookup_region(t).unwrap();
        let x = extent_of(&built, &t[start..]).unwrap();
        unchanged(&built, &FontRef::new(out).unwrap(), &x).unwrap()
    }

    #[test]
    fn the_guard_reads_every_kind_of_object() {
        let (stage, _) = inputs();
        let built = FontRef::new(&stage).unwrap();
        let t = built.table_data(Tag::new(b"GSUB")).unwrap().as_bytes();
        let (start, _, _) = lookup_region(t).unwrap();
        let x = extent_of(&built, &t[start..]).unwrap();
        let o = objects(&built, &x).unwrap();
        for kind in [
            "glyph ",
            "cmap ",
            "GSUB lookups",
            "GPOS lookup ",
            "GSUB script ",
            "GPOS script ",
            "class ",
            "mark set ",
            "name ",
        ] {
            let n = o.lines.keys().filter(|k| k.starts_with(kind)).count();
            assert!(n > 0, "{kind}: no lines");
        }
        assert!(
            x.gsub > 400 && x.gpos > 30 && x.sets == 7,
            "{} {} {}",
            x.gsub,
            x.gpos,
            x.sets
        );
    }

    #[test]
    fn the_committed_regular_leaves_every_object_of_the_stage() {
        let (stage, merged) = inputs();
        assert!(changed(&stage, &merged).is_empty());
    }

    /// Each kind of object, changed in the merged font, is named: a glyph's
    /// bytes, a GPOS lookup's flag, a byte of the GSUB lookup block, and a
    /// name record's text.
    #[test]
    fn a_changed_object_of_the_stage_is_named() {
        let (stage, merged) = inputs();
        let f = FontRef::new(&stage).unwrap();
        let glyf = range(&merged, b"glyf");
        let a = f.cmap().unwrap().map_codepoint('A').unwrap().to_u32();
        let loca = FontRef::new(&merged).unwrap().loca(None).unwrap();
        let at = glyf.start + loca.get_raw(a as usize).unwrap() as usize;
        let mut out = merged.clone();
        out[at + 7] ^= 1;
        let c = changed(&stage, &out);
        assert!(c.contains(&format!("glyph {a}")), "{c:?}");

        let gpos = range(&merged, b"GPOS");
        let ll = gpos.start
            + u16::from_be_bytes([merged[gpos.start + 8], merged[gpos.start + 9]]) as usize;
        let l0 = ll + u16::from_be_bytes([merged[ll + 2], merged[ll + 3]]) as usize;
        let mut out = merged.clone();
        out[l0 + 3] ^= 0x08;
        let c = changed(&stage, &out);
        assert!(c.iter().any(|k| k.starts_with("GPOS lookup 0")), "{c:?}");

        let gsub = range(&merged, b"GSUB");
        let (_, offsets, _) = lookup_region(&merged[gsub.clone()]).unwrap();
        let mut out = merged.clone();
        // The first lookup's flag, low byte: a byte of the block that parses.
        out[gsub.start + offsets[0] + 3] ^= 1;
        let c = changed(&stage, &out);
        assert!(c.contains(&"GSUB lookups".to_string()), "{c:?}");

        let name = range(&merged, b"name");
        let storage = name.start
            + u16::from_be_bytes([merged[name.start + 4], merged[name.start + 5]]) as usize;
        let mut out = merged.clone();
        out[storage + 1] ^= 1;
        let c = changed(&stage, &out);
        assert!(c.iter().any(|k| k.starts_with("name ")), "{c:?}");
    }

    #[test]
    fn python_rounds_the_average_half_to_even() {
        assert_eq!(2.5f64.round_ties_even(), 2.0);
        assert_eq!(3.5f64.round_ties_even(), 4.0);
    }
}
