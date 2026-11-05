//! GDEF, GSUB and GPOS, expanded.
//!
//! The script, language and feature graph is compared with feature
//! indices resolved to a feature's tag and lookups, and each language's
//! features as a set. Lookups are compared in order, each by its type
//! (an Extension unwrapped), its flags and its mark filtering set by the
//! glyphs in it. A lookup's subtables are expanded to one mapping in
//! which the first subtable to match a glyph wins, as a shaper applies
//! them, so a lookup split into more subtables, or reordered where no
//! glyph sees the order, is not a difference:
//!
//! - single, multiple and alternate substitution: glyph to output;
//! - ligature: per first glyph, its ligatures in the order they are
//!   tried, one shadowed by an earlier one with the same components
//!   dropped;
//! - contextual and chained contextual (both tables) and reverse
//!   chaining: per first glyph, the rules in the order they are tried,
//!   every class and coverage as the glyph set it stands for;
//! - single and pair positioning: glyph (pair) to value records, a pair
//!   format 2 subtable ending the search for its first glyph with its
//!   class 0 value as the default;
//! - cursive: glyph to entry and exit anchors, the first subtable to
//!   cover a glyph winning. A shaper attaches two glyphs only through one
//!   subtable, so a cursive lookup split between two glyphs that attach
//!   is not seen; the built fonts have no cursive lookup;
//! - mark to base, ligature and mark: (mark, base) to both anchors, a
//!   null base anchor not matching, as HarfBuzz lets the next subtable
//!   try it.
//!
//! Formats outside OpenType 1.9.1 (the 24-bit glyph id ones) are
//! compared by their bytes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use read_fonts::tables::gdef::CaretValue;
use read_fonts::tables::gpos::{
    AnchorTable, CursivePos, MarkBasePos, MarkLigPos, MarkMarkPos, PairPos, PositionSubtables,
    SinglePos, ValueRecord,
};
use read_fonts::tables::gsub::{
    AlternateSubst, LigatureSubst, MultipleSubst, ReverseChainSingleSubst, SingleSubst,
    SubstitutionSubtables,
};
use read_fonts::tables::layout::{
    ChainedSequenceContext, ClassDef, Condition, CoverageTable, FeatureList, FeatureParams,
    FeatureVariations, ScriptList, SequenceContext, SequenceLookupRecord,
};
use read_fonts::tables::variations::ItemVariationStore;
use read_fonts::{FontData, FontRef, ReadError, TableProvider};

use crate::dump::{Dump, Tokens};
use crate::var::{Samples, f2};

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A subtable this module does not expand, compared by its bytes from
/// its start to the end of the table.
fn unmodelled(kind: &str, format: u16, data: FontData) -> String {
    format!(
        "unmodelled:{kind}-format-{format}:fnv64={:016x}",
        fnv(data.as_bytes())
    )
}

/// A glyph set, sorted, runs written as ranges: `{3-9,12}`.
fn set(glyphs: impl IntoIterator<Item = u32>) -> String {
    let sorted: BTreeSet<u32> = glyphs.into_iter().collect();
    let mut parts = vec![];
    let mut it = sorted.into_iter().peekable();
    while let Some(start) = it.next() {
        let mut end = start;
        while it.peek() == Some(&(end + 1)) {
            end = it.next().unwrap();
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
    }
    format!("{{{}}}", parts.join(","))
}

fn cov(c: &CoverageTable) -> Vec<u32> {
    c.iter().map(|g| g.to_u32()).collect()
}

fn coverages<'a>(
    list: read_fonts::ArrayOfOffsets<'a, CoverageTable<'a>>,
) -> Result<Vec<Vec<u32>>, ReadError> {
    list.iter().map(|c| c.map(|c| cov(&c))).collect()
}

/// What every expansion needs from GDEF: its variation store and its mark
/// glyph sets.
struct Ctx<'a, 's> {
    samples: &'s Samples,
    store: Option<ItemVariationStore<'a>>,
    mark_sets: Vec<String>,
}

impl<'a, 's> Ctx<'a, 's> {
    fn new(font: &FontRef<'a>, samples: &'s Samples) -> Ctx<'a, 's> {
        let gdef = font.gdef().ok();
        let store = gdef
            .as_ref()
            .and_then(|g| g.item_var_store())
            .and_then(Result::ok);
        let mut mark_sets = vec![];
        if let Some(Some(Ok(sets))) = gdef.as_ref().map(|g| g.mark_glyph_sets_def()) {
            for c in sets.coverages().iter() {
                mark_sets.push(match c {
                    Ok(c) => set(cov(&c)),
                    Err(e) => format!("unreadable:{e}"),
                });
            }
        }
        Ctx {
            samples,
            store,
            mark_sets,
        }
    }

    fn device(
        &self,
        dev: Option<Result<read_fonts::tables::layout::DeviceOrVariationIndex, ReadError>>,
    ) -> String {
        self.samples.device(self.store.as_ref(), dev)
    }

    fn anchor(&self, a: Option<Result<AnchorTable, ReadError>>) -> String {
        match a {
            None => "-".to_string(),
            Some(Err(e)) => format!("unreadable:{e}"),
            Some(Ok(AnchorTable::Format1(a))) => {
                format!("({},{})", a.x_coordinate(), a.y_coordinate())
            }
            Some(Ok(AnchorTable::Format2(a))) => {
                format!(
                    "({},{})@{}",
                    a.x_coordinate(),
                    a.y_coordinate(),
                    a.anchor_point()
                )
            }
            Some(Ok(AnchorTable::Format3(a))) => {
                let mut s = format!("({},{})", a.x_coordinate(), a.y_coordinate());
                for (label, dev) in [("dx", a.x_device()), ("dy", a.y_device())] {
                    let d = self.device(dev);
                    if !d.is_empty() {
                        s.push_str(&format!("+{label}:{d}"));
                    }
                }
                s
            }
        }
    }

    /// The four adjustments (absent is 0) and any devices.
    fn value(&self, v: &ValueRecord) -> String {
        let n = |x: Option<i16>| x.unwrap_or(0);
        let mut s = format!(
            "{},{},{},{}",
            n(v.x_placement()),
            n(v.y_placement()),
            n(v.x_advance()),
            n(v.y_advance())
        );
        for (label, dev) in [
            ("xpd", v.x_placement_device()),
            ("ypd", v.y_placement_device()),
            ("xad", v.x_advance_device()),
            ("yad", v.y_advance_device()),
        ] {
            let d = self.device(dev);
            if !d.is_empty() {
                s.push_str(&format!("+{label}:{d}"));
            }
        }
        s
    }
}

pub fn gdef(font: &FontRef, samples: &Samples) -> Result<Dump, ReadError> {
    let gdef = font.gdef()?;
    let ctx = Ctx::new(font, samples);
    let mut d = Dump::default();
    d.put(
        "version",
        format!("{}.{}", gdef.version().major, gdef.version().minor),
    );
    let classes = |c: Option<Result<ClassDef, ReadError>>| -> Result<String, ReadError> {
        let mut t = Tokens::default();
        if let Some(c) = c {
            let pairs: BTreeMap<u32, u32> = c?.iter().map(|(g, k)| (g.to_u32(), k)).collect();
            for (g, k) in pairs.into_iter().filter(|(_, k)| *k != 0) {
                t.push(format!("{g}={k}"));
            }
        } else {
            t.push("absent");
        }
        Ok(t.done())
    };
    d.put("glyph classes", classes(gdef.glyph_class_def())?);
    let mut t = Tokens::default();
    match gdef.attach_list() {
        None => t.push("absent"),
        Some(list) => {
            let list = list?;
            for (g, points) in cov(&list.coverage()?)
                .into_iter()
                .zip(list.attach_points().iter())
            {
                let p: Vec<String> = points?
                    .point_indices()
                    .iter()
                    .map(|p| p.get().to_string())
                    .collect();
                t.push(format!("{g}={}", p.join(",")));
            }
        }
    }
    d.put("attach points", t.done());
    let mut t = Tokens::default();
    match gdef.lig_caret_list() {
        None => t.push("absent"),
        Some(list) => {
            let list = list?;
            let mut carets = BTreeMap::new();
            for (g, lig) in cov(&list.coverage()?)
                .into_iter()
                .zip(list.lig_glyphs().iter())
            {
                let mut c = vec![];
                for v in lig?.caret_values().iter() {
                    c.push(match v? {
                        CaretValue::Format1(v) => format!("x{}", v.coordinate()),
                        CaretValue::Format2(v) => format!("pt{}", v.caret_value_point_index()),
                        CaretValue::Format3(v) => {
                            format!("x{}+{}", v.coordinate(), ctx.device(Some(v.device())))
                        }
                    });
                }
                carets.entry(g).or_insert(c.join("|"));
            }
            for (g, c) in carets {
                t.push(format!("{g}={c}"));
            }
        }
    }
    d.put("ligature carets", t.done());
    d.put(
        "mark attachment classes",
        classes(gdef.mark_attach_class_def())?,
    );
    for (i, s) in ctx.mark_sets.iter().enumerate() {
        d.put(format!("mark glyph set {i}"), s.clone());
    }
    Ok(d)
}

/// A feature's parameters, every field: the size feature's, a stylistic
/// set's UI name, a character variant's names and characters.
fn params(p: Option<Result<FeatureParams, ReadError>>) -> String {
    match p {
        None => String::new(),
        Some(Err(e)) => format!("+params:unreadable:{e}"),
        Some(Ok(FeatureParams::Size(s))) => format!(
            "+size(design={},id={},name={},range={}-{})",
            s.design_size(),
            s.identifier(),
            s.name_entry(),
            s.range_start(),
            s.range_end()
        ),
        Some(Ok(FeatureParams::StylisticSet(s))) => {
            format!(
                "+ss(version={},uiName={})",
                s.version(),
                s.ui_name_id().to_u16()
            )
        }
        Some(Ok(FeatureParams::CharacterVariant(c))) => {
            let chars: Vec<String> = c
                .character()
                .iter()
                .map(|u| format!("{:04X}", u.get().to_u32()))
                .collect();
            format!(
                "+cv(format={},label={},tooltip={},sample={},params={}from{},chars={})",
                c.format(),
                c.feat_ui_label_name_id().to_u16(),
                c.feat_ui_tooltip_text_name_id().to_u16(),
                c.sample_text_name_id().to_u16(),
                c.num_named_parameters(),
                c.first_param_ui_label_name_id().to_u16(),
                chars.join("/")
            )
        }
    }
}

/// The script, language and feature graph, and the feature variations.
fn graph(
    d: &mut Dump,
    scripts: &ScriptList,
    features: &FeatureList,
    variations: Option<Result<FeatureVariations, ReadError>>,
) -> Result<(), ReadError> {
    let records = features.feature_records();
    let feature = |i: u16| -> String {
        let Some(r) = records.get(i as usize) else {
            return format!("feature#{i}-missing");
        };
        match r.feature(features.offset_data()) {
            Ok(f) => {
                let l: Vec<String> = f
                    .lookup_list_indices()
                    .iter()
                    .map(|l| l.get().to_string())
                    .collect();
                let params = params(f.feature_params());
                format!("{}[{}]{params}", r.feature_tag(), l.join(","))
            }
            Err(e) => format!("{}:unreadable:{e}", r.feature_tag()),
        }
    };
    let lang = |d: &mut Dump, locus: String, sys: read_fonts::tables::layout::LangSys| {
        let required = match sys.required_feature_index() {
            0xFFFF => "-".to_string(),
            i => feature(i),
        };
        let mut f: Vec<String> = sys
            .feature_indices()
            .iter()
            .map(|i| feature(i.get()))
            .collect();
        f.sort();
        d.put(
            locus,
            format!("required={required} features={}", f.join(" ")),
        );
    };
    for r in scripts.script_records() {
        let script = r.script(scripts.offset_data())?;
        if let Some(sys) = script.default_lang_sys() {
            lang(d, format!("script {} default", r.script_tag()), sys?);
        }
        for l in script.lang_sys_records() {
            lang(
                d,
                format!("script {} language {}", r.script_tag(), l.lang_sys_tag()),
                l.lang_sys(script.offset_data())?,
            );
        }
    }
    let Some(fv) = variations else { return Ok(()) };
    let fv = fv?;
    for (i, r) in fv.feature_variation_records().iter().enumerate() {
        let mut t = Tokens::default();
        match r.condition_set(fv.offset_data()) {
            None => t.push("conditions=none"),
            Some(set) => {
                for c in set?.conditions().iter() {
                    t.push(match c? {
                        Condition::Format1AxisRange(c) => format!(
                            "axis{}:{}..{}",
                            c.axis_index(),
                            f2(c.filter_range_min_value()),
                            f2(c.filter_range_max_value())
                        ),
                        other => unmodelled("condition", other.format(), other.offset_data()),
                    });
                }
            }
        }
        if let Some(sub) = r.feature_table_substitution(fv.offset_data()) {
            let sub = sub?;
            for s in sub.substitutions() {
                let alt = s.alternate_feature(sub.offset_data())?;
                let l: Vec<String> = alt
                    .lookup_list_indices()
                    .iter()
                    .map(|l| l.get().to_string())
                    .collect();
                t.push(format!("{}=>[{}]", feature(s.feature_index()), l.join(",")));
            }
        }
        d.put(format!("feature variation {i}"), t.done());
    }
    if let Some(records) = fv.lookup_variation_records()
        && !records.is_empty()
    {
        d.put(
            "lookup variations",
            unmodelled("lookup-variations", 0, fv.offset_data()),
        );
    }
    Ok(())
}

fn header(d: &mut Dump, ctx: &Ctx, i: usize, kind: u16, flag: u16, filter: Option<u16>) {
    let filter = match filter {
        None => "-".to_string(),
        Some(s) => ctx
            .mark_sets
            .get(s as usize)
            .cloned()
            .unwrap_or(format!("set#{s}-missing")),
    };
    d.put(
        format!("lookup {i}"),
        format!("type={kind} flag={flag:#06x} filter={filter}"),
    );
}

/// A rule's lookups, `seq@lookup`.
fn actions(records: &[SequenceLookupRecord]) -> String {
    let a: Vec<String> = records
        .iter()
        .map(|r| format!("{}@{}", r.sequence_index(), r.lookup_list_index()))
        .collect();
    format!("do=[{}]", a.join(","))
}

/// Each class of a class definition as the glyphs it stands for, class 0
/// as the complement of the classed ones.
struct Classes {
    sets: HashMap<u32, String>,
    rest: String,
}

impl Classes {
    fn of(cd: &ClassDef) -> Classes {
        let mut by: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (g, c) in cd.iter() {
            if c != 0 {
                by.entry(c).or_default().push(g.to_u32());
            }
        }
        let rest = format!("~{}", set(by.values().flatten().copied()));
        Classes {
            sets: by.into_iter().map(|(c, g)| (c, set(g))).collect(),
            rest,
        }
    }

    fn get(&self, c: u32) -> String {
        if c == 0 {
            self.rest.clone()
        } else {
            self.sets.get(&c).cloned().unwrap_or("{}".to_string())
        }
    }
}

fn seq(sets: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", sets.into_iter().collect::<Vec<_>>().join(","))
}

/// Rules per first glyph, in the order a shaper tries them.
#[derive(Default)]
struct Rules(BTreeMap<u32, Vec<Rc<str>>>);

impl Rules {
    fn add(&mut self, g: u32, rule: &Rc<str>) {
        self.0.entry(g).or_default().push(rule.clone());
    }

    /// Each distinct rule once, numbered by where it first appears, then
    /// every glyph's list of them.
    fn emit(self, d: &mut Dump, i: usize) {
        let mut ids: HashMap<Rc<str>, usize> = HashMap::new();
        let mut order: Vec<Rc<str>> = vec![];
        let mut t = Tokens::default();
        for (g, rules) in &self.0 {
            let mut names = vec![];
            for r in rules {
                let id = *ids.entry(r.clone()).or_insert_with(|| {
                    order.push(r.clone());
                    order.len() - 1
                });
                names.push(format!("r{id}"));
            }
            t.push(format!("{g}:{}", names.join(",")));
        }
        for (k, r) in order.iter().enumerate() {
            d.put(format!("lookup {i} rule r{k}"), r.to_string());
        }
        d.put(format!("lookup {i} glyphs"), t.done());
    }
}

fn sequence(rules: &mut Rules, s: SequenceContext) -> Result<Option<String>, ReadError> {
    match s {
        SequenceContext::Format1(f) => {
            for (g, rs) in cov(&f.coverage()?)
                .into_iter()
                .zip(f.seq_rule_sets().iter())
            {
                let Some(rs) = rs else { continue };
                for r in rs?.seq_rules().iter() {
                    let r = r?;
                    let input = seq(r.input_sequence().iter().map(|g| set([g.get().to_u32()])));
                    let rule: Rc<str> =
                        format!("in={input} {}", actions(r.seq_lookup_records())).into();
                    rules.add(g, &rule);
                }
            }
        }
        SequenceContext::Format2(f) => {
            let cd = f.class_def()?;
            let classes = Classes::of(&cd);
            let mut by_class: HashMap<u32, Vec<Rc<str>>> = HashMap::new();
            for (c, rs) in f.class_seq_rule_sets().iter().enumerate() {
                let Some(rs) = rs else { continue };
                let mut list = vec![];
                for r in rs?.class_seq_rules().iter() {
                    let r = r?;
                    let input = seq(r
                        .input_sequence()
                        .iter()
                        .map(|c| classes.get(c.get() as u32)));
                    list.push(format!("in={input} {}", actions(r.seq_lookup_records())).into());
                }
                by_class.insert(c as u32, list);
            }
            for g in cov(&f.coverage()?) {
                for rule in by_class
                    .get(&cd.get(read_fonts::types::GlyphId::new(g)))
                    .into_iter()
                    .flatten()
                {
                    rules.add(g, rule);
                }
            }
        }
        SequenceContext::Format3(f) => {
            let mut sets = vec![];
            for c in f.coverages().iter() {
                sets.push(cov(&c?));
            }
            let Some(first) = sets.first().cloned() else {
                return Ok(None);
            };
            let input = seq(sets[1..].iter().map(|s| set(s.iter().copied())));
            let rule: Rc<str> = format!("in={input} {}", actions(f.seq_lookup_records())).into();
            for g in first {
                rules.add(g, &rule);
            }
        }
        other => {
            return Ok(Some(unmodelled(
                "sequence-context",
                other.format(),
                other.offset_data(),
            )));
        }
    }
    Ok(None)
}

fn chained(rules: &mut Rules, s: ChainedSequenceContext) -> Result<Option<String>, ReadError> {
    let glyphs = |s: &[read_fonts::types::BigEndian<read_fonts::types::GlyphId16>]| {
        seq(s.iter().map(|g| set([g.get().to_u32()])))
    };
    match s {
        ChainedSequenceContext::Format1(f) => {
            for (g, rs) in cov(&f.coverage()?)
                .into_iter()
                .zip(f.chained_seq_rule_sets().iter())
            {
                let Some(rs) = rs else { continue };
                for r in rs?.chained_seq_rules().iter() {
                    let r = r?;
                    let rule: Rc<str> = format!(
                        "back={} in={} ahead={} {}",
                        glyphs(r.backtrack_sequence()),
                        glyphs(r.input_sequence()),
                        glyphs(r.lookahead_sequence()),
                        actions(r.seq_lookup_records())
                    )
                    .into();
                    rules.add(g, &rule);
                }
            }
        }
        ChainedSequenceContext::Format2(f) => {
            let input_cd = f.input_class_def()?;
            let (back, input, ahead) = (
                Classes::of(&f.backtrack_class_def()?),
                Classes::of(&input_cd),
                Classes::of(&f.lookahead_class_def()?),
            );
            let classes = |c: &Classes, s: &[read_fonts::types::BigEndian<u16>]| {
                seq(s.iter().map(|k| c.get(k.get() as u32)))
            };
            let mut by_class: HashMap<u32, Vec<Rc<str>>> = HashMap::new();
            for (c, rs) in f.chained_class_seq_rule_sets().iter().enumerate() {
                let Some(rs) = rs else { continue };
                let mut list = vec![];
                for r in rs?.chained_class_seq_rules().iter() {
                    let r = r?;
                    list.push(
                        format!(
                            "back={} in={} ahead={} {}",
                            classes(&back, r.backtrack_sequence()),
                            classes(&input, r.input_sequence()),
                            classes(&ahead, r.lookahead_sequence()),
                            actions(r.seq_lookup_records())
                        )
                        .into(),
                    );
                }
                by_class.insert(c as u32, list);
            }
            for g in cov(&f.coverage()?) {
                for rule in by_class
                    .get(&input_cd.get(read_fonts::types::GlyphId::new(g)))
                    .into_iter()
                    .flatten()
                {
                    rules.add(g, rule);
                }
            }
        }
        ChainedSequenceContext::Format3(f) => {
            let back = coverages(f.backtrack_coverages())?;
            let input = coverages(f.input_coverages())?;
            let ahead = coverages(f.lookahead_coverages())?;
            let Some(first) = input.first().cloned() else {
                return Ok(None);
            };
            let s = |v: &[Vec<u32>]| seq(v.iter().map(|s| set(s.iter().copied())));
            let rule: Rc<str> = format!(
                "back={} in={} ahead={} {}",
                s(&back),
                s(&input[1..]),
                s(&ahead),
                actions(f.seq_lookup_records())
            )
            .into();
            for g in first {
                rules.add(g, &rule);
            }
        }
        other => {
            return Ok(Some(unmodelled(
                "chained-context",
                other.format(),
                other.offset_data(),
            )));
        }
    }
    Ok(None)
}

/// One lookup's mapping lines, and the subtables it could not expand.
#[derive(Default)]
struct Expansion {
    unmodelled: Vec<String>,
}

impl Expansion {
    fn note(&mut self, u: Option<String>) {
        if let Some(u) = u {
            self.unmodelled.push(u);
        }
    }

    fn emit(self, d: &mut Dump, i: usize) {
        for (k, u) in self.unmodelled.into_iter().enumerate() {
            d.put(format!("lookup {i} unexpanded {k}"), u);
        }
    }
}

fn map_line(d: &mut Dump, i: usize, map: BTreeMap<u32, String>) {
    let mut t = Tokens::default();
    for (g, v) in map {
        t.push(format!("{g}>{v}"));
    }
    d.put(format!("lookup {i} map"), t.done());
}

pub fn gsub(font: &FontRef, samples: &Samples) -> Result<Dump, ReadError> {
    let gsub = font.gsub()?;
    let ctx = Ctx::new(font, samples);
    let mut d = Dump::default();
    d.put(
        "version",
        format!("{}.{}", gsub.version().major, gsub.version().minor),
    );
    graph(
        &mut d,
        &gsub.script_list()?,
        &gsub.feature_list()?,
        gsub.feature_variations(),
    )?;
    let lookups = gsub.lookup_list()?.lookups();
    d.put("lookup count", lookups.len().to_string());
    for (i, lookup) in lookups.iter().enumerate() {
        let lookup = lookup?;
        let subs = lookup.subtables()?;
        let kind = match &subs {
            SubstitutionSubtables::Single(_) => 1,
            SubstitutionSubtables::Multiple(_) => 2,
            SubstitutionSubtables::Alternate(_) => 3,
            SubstitutionSubtables::Ligature(_) => 4,
            SubstitutionSubtables::Contextual(_) => 5,
            SubstitutionSubtables::ChainContextual(_) => 6,
            SubstitutionSubtables::Reverse(_) => 8,
            SubstitutionSubtables::EmptyExtension => 7,
        };
        header(
            &mut d,
            &ctx,
            i,
            kind,
            lookup.lookup_flag().to_bits(),
            lookup.mark_filtering_set(),
        );
        let mut x = Expansion::default();
        let mut map: BTreeMap<u32, String> = BTreeMap::new();
        match subs {
            SubstitutionSubtables::Single(subs) => {
                for s in subs.iter() {
                    match s? {
                        SingleSubst::Format1(f) => {
                            let delta = f.delta_glyph_id() as i32;
                            for g in cov(&f.coverage()?) {
                                let out = (g as i32 + delta).rem_euclid(65536);
                                map.entry(g).or_insert(out.to_string());
                            }
                        }
                        SingleSubst::Format2(f) => {
                            for (g, out) in cov(&f.coverage()?)
                                .into_iter()
                                .zip(f.substitute_glyph_ids())
                            {
                                map.entry(g).or_insert(out.get().to_u32().to_string());
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "single",
                            other.subst_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                map_line(&mut d, i, map);
            }
            SubstitutionSubtables::Multiple(subs) => {
                for s in subs.iter() {
                    match s? {
                        MultipleSubst::Format1(f) => {
                            for (g, sq) in cov(&f.coverage()?).into_iter().zip(f.sequences().iter())
                            {
                                let out: Vec<String> = sq?
                                    .substitute_glyph_ids()
                                    .iter()
                                    .map(|g| g.get().to_u32().to_string())
                                    .collect();
                                map.entry(g).or_insert(if out.is_empty() {
                                    "-".to_string()
                                } else {
                                    out.join(",")
                                });
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "multiple",
                            other.subst_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                map_line(&mut d, i, map);
            }
            SubstitutionSubtables::Alternate(subs) => {
                for s in subs.iter() {
                    match s? {
                        AlternateSubst::Format1(f) => {
                            for (g, alt) in cov(&f.coverage()?)
                                .into_iter()
                                .zip(f.alternate_sets().iter())
                            {
                                let out: Vec<String> = alt?
                                    .alternate_glyph_ids()
                                    .iter()
                                    .map(|g| g.get().to_u32().to_string())
                                    .collect();
                                map.entry(g).or_insert(out.join("|"));
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "alternate",
                            other.subst_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                map_line(&mut d, i, map);
            }
            SubstitutionSubtables::Ligature(subs) => {
                let mut ligs: BTreeMap<u32, Vec<(Vec<u32>, u32)>> = BTreeMap::new();
                for s in subs.iter() {
                    match s? {
                        LigatureSubst::Format1(f) => {
                            for (g, set) in cov(&f.coverage()?)
                                .into_iter()
                                .zip(f.ligature_sets().iter())
                            {
                                for lig in set?.ligatures().iter() {
                                    let lig = lig?;
                                    let comps: Vec<u32> = lig
                                        .component_glyph_ids()
                                        .iter()
                                        .map(|c| c.get().to_u32())
                                        .collect();
                                    let list = ligs.entry(g).or_default();
                                    if !list.iter().any(|(c, _)| *c == comps) {
                                        list.push((comps, lig.ligature_glyph().to_u32()));
                                    }
                                }
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "ligature",
                            other.subst_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                let mut t = Tokens::default();
                for (g, list) in ligs {
                    for (comps, lig) in list {
                        let c: Vec<String> = comps.iter().map(|c| c.to_string()).collect();
                        t.push(format!("{g}+{}>{lig}", c.join("+")));
                    }
                }
                d.put(format!("lookup {i} ligatures"), t.done());
            }
            SubstitutionSubtables::Contextual(subs) => {
                let mut rules = Rules::default();
                for s in subs.iter() {
                    x.note(sequence(&mut rules, s?)?);
                }
                rules.emit(&mut d, i);
            }
            SubstitutionSubtables::ChainContextual(subs) => {
                let mut rules = Rules::default();
                for s in subs.iter() {
                    x.note(chained(&mut rules, s?)?);
                }
                rules.emit(&mut d, i);
            }
            SubstitutionSubtables::Reverse(subs) => {
                let mut rules = Rules::default();
                for s in subs.iter() {
                    match s? {
                        ReverseChainSingleSubst::Format1(f) => {
                            let sets = |v: Vec<Vec<u32>>| seq(v.into_iter().map(set));
                            let context = format!(
                                "back={} ahead={}",
                                sets(coverages(f.backtrack_coverages())?),
                                sets(coverages(f.lookahead_coverages())?)
                            );
                            for (g, out) in cov(&f.coverage()?)
                                .into_iter()
                                .zip(f.substitute_glyph_ids())
                            {
                                let rule: Rc<str> =
                                    format!("{context} >{}", out.get().to_u32()).into();
                                rules.add(g, &rule);
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "reverse",
                            other.subst_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                rules.emit(&mut d, i);
            }
            SubstitutionSubtables::EmptyExtension => {}
        }
        x.emit(&mut d, i);
    }
    Ok(d)
}

/// Pair adjustments for one first glyph: explicit second glyphs, and the
/// default a format 2 subtable gives every other one.
#[derive(Default)]
struct Pairs {
    second: BTreeMap<u32, String>,
    default: Option<String>,
}

fn pair_value(ctx: &Ctx, v1: &ValueRecord, v2: &ValueRecord, has_second: bool) -> String {
    // A non-zero second value format consumes the second glyph, so its
    // presence is a behaviour and not only a value.
    let second = if has_second {
        ctx.value(v2)
    } else {
        "-".to_string()
    };
    format!("{}/{}", ctx.value(v1), second)
}

/// (mark, base) to (mark anchor, base anchor), first match wins.
type Attach = BTreeMap<u32, BTreeMap<(u32, Option<usize>), String>>;

fn attach_line(d: &mut Dump, i: usize, map: Attach) {
    for (m, bases) in map {
        let mut t = Tokens::default();
        for ((b, component), v) in bases {
            t.push(match component {
                None => format!("{b}={v}"),
                Some(k) => format!("{b}#{k}={v}"),
            });
        }
        d.glyph(m, format!("lookup {i} attachments"), t.done());
    }
}

pub fn gpos(font: &FontRef, samples: &Samples) -> Result<Dump, ReadError> {
    let gpos = font.gpos()?;
    let ctx = Ctx::new(font, samples);
    let mut d = Dump::default();
    d.put(
        "version",
        format!("{}.{}", gpos.version().major, gpos.version().minor),
    );
    graph(
        &mut d,
        &gpos.script_list()?,
        &gpos.feature_list()?,
        gpos.feature_variations(),
    )?;
    let lookups = gpos.lookup_list()?.lookups();
    d.put("lookup count", lookups.len().to_string());
    for (i, lookup) in lookups.iter().enumerate() {
        let lookup = lookup?;
        let subs = lookup.subtables()?;
        let kind = match &subs {
            PositionSubtables::Single(_) => 1,
            PositionSubtables::Pair(_) => 2,
            PositionSubtables::Cursive(_) => 3,
            PositionSubtables::MarkToBase(_) => 4,
            PositionSubtables::MarkToLig(_) => 5,
            PositionSubtables::MarkToMark(_) => 6,
            PositionSubtables::Contextual(_) => 7,
            PositionSubtables::ChainContextual(_) => 8,
            PositionSubtables::EmptyExtension => 9,
        };
        header(
            &mut d,
            &ctx,
            i,
            kind,
            lookup.lookup_flag().to_bits(),
            lookup.mark_filtering_set(),
        );
        let mut x = Expansion::default();
        match subs {
            PositionSubtables::Single(subs) => {
                let mut map: BTreeMap<u32, String> = BTreeMap::new();
                for s in subs.iter() {
                    match s? {
                        SinglePos::Format1(f) => {
                            let v = ctx.value(&f.value_record());
                            for g in cov(&f.coverage()?) {
                                map.entry(g).or_insert(v.clone());
                            }
                        }
                        SinglePos::Format2(f) => {
                            for (g, v) in cov(&f.coverage()?)
                                .into_iter()
                                .zip(f.value_records().iter())
                            {
                                map.entry(g).or_insert(ctx.value(&v?));
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "single-pos",
                            other.pos_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                map_line(&mut d, i, map);
            }
            PositionSubtables::Pair(subs) => {
                let mut pairs: BTreeMap<u32, Pairs> = BTreeMap::new();
                for s in subs.iter() {
                    match s? {
                        PairPos::Format1(f) => {
                            let has_second = !f.value_format2().is_empty();
                            for (g1, ps) in
                                cov(&f.coverage()?).into_iter().zip(f.pair_sets().iter())
                            {
                                let p = pairs.entry(g1).or_default();
                                if p.default.is_some() {
                                    continue;
                                }
                                for r in ps?.pair_value_records().iter() {
                                    let r = r?;
                                    let v = pair_value(
                                        &ctx,
                                        r.value_record1(),
                                        r.value_record2(),
                                        has_second,
                                    );
                                    p.second.entry(r.second_glyph().to_u32()).or_insert(v);
                                }
                            }
                        }
                        PairPos::Format2(f) => {
                            let has_second = !f.value_format2().is_empty();
                            let (cd1, cd2) = (f.class_def1()?, f.class_def2()?);
                            let records = f.class_value_records();
                            let classed: Vec<(u32, u16)> = cd2
                                .iter()
                                .map(|(g, c)| (g.to_u32(), c as u16))
                                .filter(|(_, c)| *c != 0)
                                .collect();
                            let mut cache: HashMap<(u16, u16), Option<String>> = HashMap::new();
                            let mut value = |c1: u16, c2: u16| -> Option<String> {
                                cache
                                    .entry((c1, c2))
                                    .or_insert_with(|| {
                                        records
                                            .get(c1, c2)
                                            .map(|[v1, v2]| pair_value(&ctx, &v1, &v2, has_second))
                                    })
                                    .clone()
                            };
                            for g1 in cov(&f.coverage()?) {
                                let c1 = cd1.get(read_fonts::types::GlyphId::new(g1)) as u16;
                                let Some(default) = value(c1, 0) else {
                                    continue;
                                };
                                let p = pairs.entry(g1).or_default();
                                if p.default.is_some() {
                                    continue;
                                }
                                for &(g2, c2) in &classed {
                                    if let Some(v) = value(c1, c2) {
                                        p.second.entry(g2).or_insert(v);
                                    }
                                }
                                p.default = Some(default);
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "pair-pos",
                            other.pos_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                for (g1, mut p) in pairs {
                    if let Some(def) = &p.default {
                        p.second.retain(|_, v| v != def);
                    }
                    let mut t = Tokens::default();
                    for (g2, v) in p.second {
                        t.push(format!("{g2}={v}"));
                    }
                    if let Some(def) = p.default {
                        t.push(format!("*={def}"));
                    }
                    d.glyph(g1, format!("lookup {i} pairs"), t.done());
                }
            }
            PositionSubtables::Cursive(subs) => {
                let mut map: BTreeMap<u32, String> = BTreeMap::new();
                for s in subs.iter() {
                    match s? {
                        CursivePos::Format1(f) => {
                            for (g, r) in cov(&f.coverage()?).into_iter().zip(f.entry_exit_record())
                            {
                                let v = format!(
                                    "{}/{}",
                                    ctx.anchor(r.entry_anchor(f.offset_data())),
                                    ctx.anchor(r.exit_anchor(f.offset_data()))
                                );
                                map.entry(g).or_insert(v);
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "cursive",
                            other.pos_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                map_line(&mut d, i, map);
            }
            PositionSubtables::MarkToBase(subs) => {
                let mut map = Attach::new();
                for s in subs.iter() {
                    match s? {
                        MarkBasePos::Format1(f) => {
                            let marks = f.mark_array()?;
                            let bases = f.base_array()?;
                            let base_glyphs = cov(&f.base_coverage()?);
                            let records: Vec<_> =
                                bases.base_records().iter().collect::<Result<_, _>>()?;
                            for (m, r) in cov(&f.mark_coverage()?)
                                .into_iter()
                                .zip(marks.mark_records())
                            {
                                let ma = ctx.anchor(Some(r.mark_anchor(marks.offset_data())));
                                let class = r.mark_class() as usize;
                                for (b, rec) in base_glyphs.iter().zip(&records) {
                                    let Some(ba) = rec.base_anchors(bases.offset_data()).get(class)
                                    else {
                                        continue;
                                    };
                                    let ba = ctx.anchor(Some(ba));
                                    map.entry(m)
                                        .or_default()
                                        .entry((*b, None))
                                        .or_insert(format!("{ma};{ba}"));
                                }
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "mark-base",
                            other.pos_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                attach_line(&mut d, i, map);
            }
            PositionSubtables::MarkToLig(subs) => {
                let mut map = Attach::new();
                for s in subs.iter() {
                    match s? {
                        MarkLigPos::Format1(f) => {
                            let marks = f.mark_array()?;
                            let ligs = f.ligature_array()?;
                            let lig_glyphs = cov(&f.ligature_coverage()?);
                            let attaches: Vec<_> =
                                ligs.ligature_attaches().iter().collect::<Result<_, _>>()?;
                            for (m, r) in cov(&f.mark_coverage()?)
                                .into_iter()
                                .zip(marks.mark_records())
                            {
                                let ma = ctx.anchor(Some(r.mark_anchor(marks.offset_data())));
                                let class = r.mark_class() as usize;
                                for (l, att) in lig_glyphs.iter().zip(&attaches) {
                                    for (k, comp) in att.component_records().iter().enumerate() {
                                        let comp = comp?;
                                        let Some(la) =
                                            comp.ligature_anchors(att.offset_data()).get(class)
                                        else {
                                            continue;
                                        };
                                        let la = ctx.anchor(Some(la));
                                        map.entry(m)
                                            .or_default()
                                            .entry((*l, Some(k)))
                                            .or_insert(format!("{ma};{la}"));
                                    }
                                }
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "mark-lig",
                            other.pos_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                attach_line(&mut d, i, map);
            }
            PositionSubtables::MarkToMark(subs) => {
                let mut map = Attach::new();
                for s in subs.iter() {
                    match s? {
                        MarkMarkPos::Format1(f) => {
                            let marks = f.mark1_array()?;
                            let bases = f.mark2_array()?;
                            let base_glyphs = cov(&f.mark2_coverage()?);
                            let records: Vec<_> =
                                bases.mark2_records().iter().collect::<Result<_, _>>()?;
                            for (m, r) in cov(&f.mark1_coverage()?)
                                .into_iter()
                                .zip(marks.mark_records())
                            {
                                let ma = ctx.anchor(Some(r.mark_anchor(marks.offset_data())));
                                let class = r.mark_class() as usize;
                                for (b, rec) in base_glyphs.iter().zip(&records) {
                                    let Some(ba) =
                                        rec.mark2_anchors(bases.offset_data()).get(class)
                                    else {
                                        continue;
                                    };
                                    let ba = ctx.anchor(Some(ba));
                                    map.entry(m)
                                        .or_default()
                                        .entry((*b, None))
                                        .or_insert(format!("{ma};{ba}"));
                                }
                            }
                        }
                        other => x.note(Some(unmodelled(
                            "mark-mark",
                            other.pos_format(),
                            other.offset_data(),
                        ))),
                    }
                }
                attach_line(&mut d, i, map);
            }
            PositionSubtables::Contextual(subs) => {
                let mut rules = Rules::default();
                for s in subs.iter() {
                    x.note(sequence(&mut rules, s?)?);
                }
                rules.emit(&mut d, i);
            }
            PositionSubtables::ChainContextual(subs) => {
                let mut rules = Rules::default();
                for s in subs.iter() {
                    x.note(chained(&mut rules, s?)?);
                }
                rules.emit(&mut d, i);
            }
            PositionSubtables::EmptyExtension => {}
        }
        x.emit(&mut d, i);
    }
    Ok(d)
}
