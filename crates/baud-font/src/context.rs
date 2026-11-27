//! Contextual lookups, which `GSUB` (`[[source: ms-opentype-gsub]]`, types
//! 5 and 6) and `GPOS` (`[[source: ms-opentype-gpos]]`, type 8) lay out
//! alike: a chained rule matches a backtrack, an input and a lookahead
//! sequence, by glyph, by class or by coverage, a plain contextual rule an
//! input sequence alone, and then runs other lookups of its own table at
//! positions of the input. The matching is one code for both tables; what
//! a nested lookup does is the table's, through [`Nested`].

use crate::buffer::{Buffer, Glyph, Ignorable};
use crate::bytes::{Array, Bytes, Count, Index, Len, Offset};
use crate::layout::{ClassDef, Coverage, Filter, Joiners, PerSyllable, Skip, Stage};
use crate::syllabic::{Category, Syllable};
use baud_domain::GlyphId;

/// A table whose lookups a contextual rule can run: `nested` applies
/// lookup `lookup` at `pos`, the first of its subtables that applies
/// winning, and says what it did. A nested lookup runs in its caller's
/// syllabic `stage`, with its own flags, as HarfBuzz's `recurse()` keeps
/// the calling lookup's mask and joiners (`[[source:
/// harfbuzz-14-5-1-gsubgpos]]`).
pub(crate) trait Nested<'a> {
    fn nested(
        &self,
        lookup: Index,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
        stage: Option<Stage>,
    ) -> Option<Applied>;
}

/// The coverage table whose offset sits at `at` in `sub`.
pub(crate) fn coverage_at(sub: Bytes<'_>, at: Offset) -> Option<Coverage<'_>> {
    let off: u16 = sub.read(at)?;
    Coverage::parse(sub.tail(Offset::of(u32::from(off)))?)
}

/// How deep nested lookups may recurse; a contextual lookup naming
/// another contextual lookup naming another is unusual past two.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Depth(u8);

impl Depth {
    pub(crate) const TOP: Depth = Depth(0);
    pub(crate) const LIMIT: Depth = Depth(8);

    pub(crate) fn deeper(self) -> Depth {
        Depth(self.0.saturating_add(1))
    }
}

/// The longest input, backtrack or lookahead sequence matched; longer
/// rules are declined rather than matched partially.
pub(crate) const SEQ_MAX: usize = 16;

/// Positions in the buffer of a matched sequence.
#[derive(Clone, Copy)]
pub(crate) struct Matched {
    pub(crate) at: [Index; SEQ_MAX],
    pub(crate) len: Count,
}

impl Matched {
    pub(crate) fn first(pos: Index) -> Matched {
        let mut m = Matched {
            at: [Index::ZERO; SEQ_MAX],
            len: Count::of(1u32),
        };
        m.at[0] = pos;
        m
    }

    pub(crate) fn positions(&self) -> &[Index] {
        self.at.get(..self.len.slot()).unwrap_or(&[])
    }

    pub(crate) fn last(&self) -> Index {
        self.positions().last().copied().unwrap_or(Index::ZERO)
    }

    pub(crate) fn push(&mut self, pos: Index) {
        let n = self.len.slot();
        if let Some(slot) = self.at.get_mut(n) {
            *slot = pos;
            self.len = Count::of(u32::try_from(n + 1).unwrap_or(u32::MAX));
        }
    }
}

/// What applying a subtable at a position did. Eight bytes, as it was
/// before glyphs could be put in: every lookup call returns one, and a
/// wider one is returned through memory.
#[derive(Clone, Copy)]
pub(crate) struct Applied {
    /// The position to continue from.
    pub(crate) next: Index,
    /// How many glyphs the buffer lost, or gained, after the position the
    /// subtable was applied at.
    pub(crate) delta: Delta,
}

impl Applied {
    pub(crate) fn after(pos: Index) -> Applied {
        Applied {
            next: pos.next(),
            delta: Delta::ZERO,
        }
    }
}

/// How many glyphs a substitution took out of the buffer, below zero --
/// a ligature's components -- or put in, above it -- a multiple
/// substitution's sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Delta(i32);

impl Delta {
    pub(crate) const ZERO: Delta = Delta(0);

    pub(crate) fn removed(n: Count) -> Delta {
        Delta(0i32.saturating_sub_unsigned(n.value()))
    }

    pub(crate) fn added(n: Count) -> Delta {
        Delta(0i32.saturating_add_unsigned(n.value()))
    }

    fn plus(self, other: Delta) -> Delta {
        Delta(self.0.saturating_add(other.0))
    }
}

/// One position's glyph, seen through a lookup's filter.
pub(crate) fn seen(filter: &Filter<'_>, g: Glyph) -> Skip {
    filter.skip_class(g.class, g.id)
}

pub(crate) fn same(g: GlyphId, v: u16) -> bool {
    g.index() == usize::from(v)
}

pub(crate) fn bump(c: Count, by: Count) -> Count {
    Count::of(c.value().saturating_add(by.value()))
}

/// Which of a chained rule's three sequences a test is for.
#[derive(Clone, Copy)]
pub(crate) enum Which {
    Backtrack,
    Input,
    Lookahead,
}

/// Walks forward from `from` to the next glyph the filter does not
/// skip, applying the default-ignorable rule against `wanted`.
pub(crate) fn next_match(
    filter: &Filter<'_>,
    buf: &Buffer<'_>,
    from: Index,
    wanted: impl Fn(GlyphId) -> bool,
) -> Option<Index> {
    let mut j = from;
    while let Some(g) = buf.get(j) {
        if seen(filter, g) == Skip::Yes {
            j = j.next();
            continue;
        }
        if wanted(g.id) {
            return Some(j);
        }
        if g.ignorable == Ignorable::Yes {
            j = j.next();
            continue;
        }
        return None;
    }
    None
}

/// The same, walking backward from `from` exclusive.
pub(crate) fn prev_match(
    filter: &Filter<'_>,
    buf: &Buffer<'_>,
    from: Index,
    wanted: impl Fn(GlyphId) -> bool,
) -> Option<Index> {
    let mut j = from;
    while let Some(p) = j.prev() {
        j = p;
        let g = buf.get(j)?;
        if seen(filter, g) == Skip::Yes {
            continue;
        }
        if wanted(g.id) {
            return Some(j);
        }
        if g.ignorable == Ignorable::Yes {
            continue;
        }
        return None;
    }
    None
}

/// Which of a lookup's sequences a staged match is walking: the input,
/// whose glyphs must carry the stage's masks, or a backtrack or lookahead.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Input,
    Context,
}

/// [`next_match`] in a syllabic shaper's stage ([`Stage`]), for a sequence
/// whose first glyph is in `start`.
fn next_staged(
    filter: &Filter<'_>,
    stage: Stage,
    buf: &Buffer<'_>,
    from: Index,
    (role, start): (Role, Syllable),
    wanted: impl Fn(GlyphId) -> bool,
) -> Option<Index> {
    let mut j = from;
    while let Some(g) = buf.get(j) {
        match staged(filter, stage, g, (role, start), &wanted) {
            Step::Match => return Some(j),
            Step::Skip => j = j.next(),
            Step::Stop => return None,
        }
    }
    None
}

/// [`prev_match`] in a stage: the backtrack, walking back from `from`
/// exclusive.
fn prev_staged(
    filter: &Filter<'_>,
    stage: Stage,
    buf: &Buffer<'_>,
    from: Index,
    start: Syllable,
    wanted: impl Fn(GlyphId) -> bool,
) -> Option<Index> {
    let mut j = from;
    while let Some(p) = j.prev() {
        j = p;
        let g = buf.get(j)?;
        match staged(filter, stage, g, (Role::Context, start), &wanted) {
            Step::Match => return Some(j),
            Step::Skip => continue,
            Step::Stop => return None,
        }
    }
    None
}

/// What a staged walk does with one glyph.
enum Step {
    Match,
    Skip,
    Stop,
}

/// One glyph seen by a staged walk: skipped by the lookup's flags; matched
/// when it carries the stage's masks (in the input), is in the first
/// glyph's syllable (where the stage says) and is the glyph wanted;
/// otherwise skipped when it is a default-ignorable the stage may skip,
/// and the end of the match when it is not.
fn staged(
    filter: &Filter<'_>,
    stage: Stage,
    g: Glyph,
    (role, start): (Role, Syllable),
    wanted: &impl Fn(GlyphId) -> bool,
) -> Step {
    if seen(filter, g) == Skip::Yes {
        return Step::Skip;
    }
    let masked = role == Role::Context || g.syllabic.masks.meets(stage.masks);
    let syllable =
        stage.syllables == PerSyllable::No || start.is_none() || g.syllabic.syllable == start;
    if masked && syllable && wanted(g.id) {
        return Step::Match;
    }
    let (zwnj, zwj) = match role {
        Role::Input => (false, stage.joiners == Joiners::Auto),
        Role::Context => (stage.joiners == Joiners::Auto, true),
    };
    let category = g.syllabic.category;
    let skips = g.ignorable == Ignorable::Yes
        && (zwnj || category != Category::ZWNJ)
        && (zwj || category != Category::ZWJ);
    match skips {
        true => Step::Skip,
        false => Step::Stop,
    }
}

/// Matches `count` pattern elements starting at `pos` (the first is
/// the caller's), each later one against `test(k, glyph)` for `k` in
/// `1..count`.
pub(crate) fn match_input(
    filter: &Filter<'_>,
    buf: &Buffer<'_>,
    pos: Index,
    count: Count,
    test: impl Fn(Index, GlyphId) -> bool,
) -> Option<Matched> {
    if count.is_zero() || count.slot() > SEQ_MAX {
        return None;
    }
    let mut m = Matched::first(pos);
    let mut j = pos;
    let mut k = Index::ZERO.next();
    let start = buf.get(pos)?.syllabic.syllable;
    while count.holds(k) {
        j = match filter.stage {
            None => next_match(filter, buf, j.next(), |g| test(k, g))?,
            Some(stage) => next_staged(filter, stage, buf, j.next(), (Role::Input, start), |g| {
                test(k, g)
            })?,
        };
        m.push(j);
        k = k.next();
    }
    Some(m)
}

/// Type 6: chained contexts, in the format the subtable says.
pub(crate) fn chained<'a, N: Nested<'a>>(
    n: &N,
    sub: Bytes<'a>,
    filter: &Filter<'_>,
    buf: &mut Buffer<'_>,
    pos: Index,
    depth: Depth,
) -> Option<Applied> {
    let format: u16 = sub.read(Offset::ZERO)?;
    match format {
        1 => chained_glyphs(n, sub, filter, buf, pos, depth),
        2 => chained_classes(n, sub, filter, buf, pos, depth),
        3 => chained_coverage(n, sub, filter, buf, pos, depth),
        _ => None,
    }
}

/// A rule's three sequences and its lookup records, as laid out in a
/// `ChainedSequenceRule` or `ChainedClassSequenceRule`: counts and
/// `u16` arrays back to back.
pub(crate) fn match_rule<'a, N: Nested<'a>>(
    n: &N,
    rule: Bytes<'a>,
    filter: &Filter<'_>,
    buf: &mut Buffer<'_>,
    pos: Index,
    depth: Depth,
    by: impl Fn(Which, GlyphId, u16) -> bool,
) -> Option<Applied> {
    let back_n: u16 = rule.read(Offset::ZERO)?;
    let back: Array<'a, u16> = rule.array(Offset::of(2u32), Count::of(back_n))?;
    let at = Offset::of(2u32).plus(Len::of(2).times(Count::of(back_n))?)?;
    let input_n: u16 = rule.read(at)?;
    let rest = Count::of(input_n.checked_sub(1)?);
    let input: Array<'a, u16> = rule.array(at.plus(Len::of(2))?, rest)?;
    let at = at.plus(Len::of(2))?.plus(Len::of(2).times(rest)?)?;
    let ahead_n: u16 = rule.read(at)?;
    let ahead: Array<'a, u16> = rule.array(at.plus(Len::of(2))?, Count::of(ahead_n))?;
    let at = at
        .plus(Len::of(2))?
        .plus(Len::of(2).times(Count::of(ahead_n))?)?;
    let rec_n: u16 = rule.read(at)?;
    let records = rule.tail(at.plus(Len::of(2))?)?;
    let m = match_input(filter, buf, pos, Count::of(input_n), |k, g| {
        k.prev()
            .and_then(|c| input.get(c))
            .is_some_and(|v| by(Which::Input, g, v))
    })?;
    match_back(filter, buf, pos, &back, |g, v| by(Which::Backtrack, g, v))?;
    match_ahead(filter, buf, m.last(), &ahead, |g, v| {
        by(Which::Lookahead, g, v)
    })?;
    run_records(n, records, (Count::of(rec_n), filter.stage), &m, buf, depth)
}

pub(crate) fn match_back(
    filter: &Filter<'_>,
    buf: &Buffer<'_>,
    pos: Index,
    pattern: &Array<'_, u16>,
    by: impl Fn(GlyphId, u16) -> bool,
) -> Option<()> {
    let mut j = pos;
    let mut k = Index::ZERO;
    let start = buf.get(pos)?.syllabic.syllable;
    while let Some(v) = pattern.get(k) {
        j = match filter.stage {
            None => prev_match(filter, buf, j, |g| by(g, v))?,
            Some(stage) => prev_staged(filter, stage, buf, j, start, |g| by(g, v))?,
        };
        k = k.next();
    }
    Some(())
}

pub(crate) fn match_ahead(
    filter: &Filter<'_>,
    buf: &Buffer<'_>,
    last_input: Index,
    pattern: &Array<'_, u16>,
    by: impl Fn(GlyphId, u16) -> bool,
) -> Option<()> {
    let mut j = last_input;
    let mut k = Index::ZERO;
    let start = buf.get(last_input)?.syllabic.syllable;
    while let Some(v) = pattern.get(k) {
        j = match filter.stage {
            None => next_match(filter, buf, j.next(), |g| by(g, v))?,
            Some(stage) => {
                next_staged(filter, stage, buf, j.next(), (Role::Context, start), |g| {
                    by(g, v)
                })?
            }
        };
        k = k.next();
    }
    Some(())
}

/// Runs the sequence lookup records over a matched input, adjusting
/// later positions for glyphs a nested ligature removed or a nested
/// multiple substitution put in. Glyphs put in after the last input
/// position are passed over with it: the walk goes on after them, as
/// HarfBuzz's `apply_lookup` moves to the match's end grown by the
/// glyphs added.
pub(crate) fn run_records<'a, N: Nested<'a>>(
    n: &N,
    records: Bytes<'a>,
    (count, stage): (Count, Option<Stage>),
    m: &Matched,
    buf: &mut Buffer<'_>,
    depth: Depth,
) -> Option<Applied> {
    if depth >= Depth::LIMIT {
        return None;
    }
    let mut at = *m;
    let mut total = Delta::ZERO;
    let mut past_last = Count::ZERO;
    let mut r = Index::ZERO;
    while count.holds(r) {
        let rec = Offset::ZERO.at(r, Len::of(4))?;
        let seq: u16 = records.read(rec)?;
        let li: u16 = records.read(rec.plus(Len::of(2))?)?;
        r = r.next();
        let k = Index::of(seq);
        if !at.len.holds(k) {
            continue;
        }
        let p = at.at[k.slot()];
        if let Some(a) = n.nested(Index::of(li), buf, p, depth.deeper(), stage) {
            let by = a.delta.0.unsigned_abs();
            if a.delta != Delta::ZERO {
                total = total.plus(a.delta);
            }
            if a.delta.0 > 0 && p == at.last() {
                past_last = bump(past_last, Count::of(by));
            } else if a.delta != Delta::ZERO {
                let n = at.len.slot();
                for q in at.at.iter_mut().take(n) {
                    if *q > p {
                        *q = match a.delta.0 > 0 {
                            true => Index::of(q.value().saturating_add(by)),
                            false => Index::of(q.value().saturating_sub(by)),
                        };
                    }
                }
            }
        }
    }
    // Continue after the input sequence, wherever it ended up.
    Some(Applied {
        next: at.last().next().plus(past_last).unwrap_or(at.last().next()),
        delta: total,
    })
}

/// **Type 5: a context with no backtrack or lookahead**, in the format the
/// subtable says (`[[source: ms-opentype-chapter2]]`, "Sequence context
/// format 1", 2 and 3): an input sequence by glyph, by class or by
/// coverage, then its lookup records, as a chained rule's.
#[inline(never)]
pub(crate) fn contextual<'a, N: Nested<'a>>(
    n: &N,
    sub: Bytes<'a>,
    filter: &Filter<'_>,
    buf: &mut Buffer<'_>,
    pos: Index,
    depth: Depth,
) -> Option<Applied> {
    let format: u16 = sub.read(Offset::ZERO)?;
    let first = buf.get(pos)?;
    match format {
        1 | 2 => {
            let coverage = coverage_at(sub, Offset::of(2u32))?;
            let ci = coverage.index(first.id)?;
            let class_def = match format {
                1 => None,
                _ => {
                    let off: u16 = sub.read(Offset::of(4u32))?;
                    Some(ClassDef::parse(sub.tail(Offset::of(u32::from(off)))?)?)
                }
            };
            let sets_at = match format {
                1 => Offset::of(4u32),
                _ => Offset::of(6u32),
            };
            let set_count: u16 = sub.read(sets_at)?;
            let sets: Array<'a, u16> =
                sub.array(sets_at.plus(Len::of(2))?, Count::of(set_count))?;
            let set_off = match class_def {
                None => sets.get(ci)?,
                Some(def) => sets.get(Index::of(def.class(first.id).0))?,
            };
            if set_off == 0 {
                return None;
            }
            let set = sub.tail(Offset::of(u32::from(set_off)))?;
            let rule_count: u16 = set.read(Offset::ZERO)?;
            let rules: Array<'a, u16> = set.array(Offset::of(2u32), Count::of(rule_count))?;
            let mut ri = Index::ZERO;
            while let Some(off) = rules.get(ri) {
                ri = ri.next();
                let Some(rule) = set.tail(Offset::of(u32::from(off))) else {
                    continue;
                };
                let applied =
                    match_sequence_rule(n, rule, filter, buf, pos, depth, |g, v| match class_def {
                        None => same(g, v),
                        Some(def) => def.class(g).0 == v,
                    });
                if applied.is_some() {
                    return applied;
                }
            }
            None
        }
        3 => {
            let input_n: u16 = sub.read(Offset::of(2u32))?;
            let rec_n: u16 = sub.read(Offset::of(4u32))?;
            let input: Array<'a, u16> = sub.array(Offset::of(6u32), Count::of(input_n))?;
            let records =
                sub.tail(Offset::of(6u32).plus(Len::of(2).times(Count::of(input_n))?)?)?;
            let covers = |off: u16, g: GlyphId| -> bool {
                sub.tail(Offset::of(u32::from(off)))
                    .and_then(Coverage::parse)
                    .is_some_and(|c| c.covers(g))
            };
            if !covers(input.get(Index::ZERO)?, first.id) {
                return None;
            }
            let m = match_input(filter, buf, pos, Count::of(input_n), |k, g| {
                input.get(k).is_some_and(|off| covers(off, g))
            })?;
            run_records(n, records, (Count::of(rec_n), filter.stage), &m, buf, depth)
        }
        _ => None,
    }
}

/// A `SequenceRule` or `ClassSequenceRule`: its input count, its record
/// count, the input after the first position, and the records.
fn match_sequence_rule<'a, N: Nested<'a>>(
    n: &N,
    rule: Bytes<'a>,
    filter: &Filter<'_>,
    buf: &mut Buffer<'_>,
    pos: Index,
    depth: Depth,
    by: impl Fn(GlyphId, u16) -> bool,
) -> Option<Applied> {
    let input_n: u16 = rule.read(Offset::ZERO)?;
    let rec_n: u16 = rule.read(Offset::of(2u32))?;
    let rest = Count::of(input_n.checked_sub(1)?);
    let input: Array<'a, u16> = rule.array(Offset::of(4u32), rest)?;
    let records = rule.tail(Offset::of(4u32).plus(Len::of(2).times(rest)?)?)?;
    let m = match_input(filter, buf, pos, Count::of(input_n), |k, g| {
        k.prev()
            .and_then(|c| input.get(c))
            .is_some_and(|v| by(g, v))
    })?;
    run_records(n, records, (Count::of(rec_n), filter.stage), &m, buf, depth)
}

/// Format 1: sequences of glyph ids.
pub(crate) fn chained_glyphs<'a, N: Nested<'a>>(
    n: &N,
    sub: Bytes<'a>,
    filter: &Filter<'_>,
    buf: &mut Buffer<'_>,
    pos: Index,
    depth: Depth,
) -> Option<Applied> {
    let coverage = coverage_at(sub, Offset::of(2u32))?;
    let first = buf.get(pos)?;
    let ci = coverage.index(first.id)?;
    let set_count: u16 = sub.read(Offset::of(4u32))?;
    let sets: Array<'a, u16> = sub.array(Offset::of(6u32), Count::of(set_count))?;
    let set_off = sets.get(ci)?;
    if set_off == 0 {
        return None;
    }
    let set = sub.tail(Offset::of(u32::from(set_off)))?;
    let rule_count: u16 = set.read(Offset::ZERO)?;
    let rules: Array<'a, u16> = set.array(Offset::of(2u32), Count::of(rule_count))?;
    let mut ri = Index::ZERO;
    while let Some(off) = rules.get(ri) {
        ri = ri.next();
        let Some(rule) = set.tail(Offset::of(u32::from(off))) else {
            continue;
        };
        let applied = match_rule(n, rule, filter, buf, pos, depth, |_, g, v| same(g, v));
        if applied.is_some() {
            return applied;
        }
    }
    None
}

/// Format 2: sequences of classes, three class definitions.
pub(crate) fn chained_classes<'a, N: Nested<'a>>(
    n: &N,
    sub: Bytes<'a>,
    filter: &Filter<'_>,
    buf: &mut Buffer<'_>,
    pos: Index,
    depth: Depth,
) -> Option<Applied> {
    let coverage = coverage_at(sub, Offset::of(2u32))?;
    let first = buf.get(pos)?;
    coverage.index(first.id)?;
    let class_def = |at: Offset| -> Option<ClassDef<'a>> {
        let off: u16 = sub.read(at)?;
        ClassDef::parse(sub.tail(Offset::of(u32::from(off)))?)
    };
    let back_def = class_def(Offset::of(4u32))?;
    let input_def = class_def(Offset::of(6u32))?;
    let ahead_def = class_def(Offset::of(8u32))?;
    let set_count: u16 = sub.read(Offset::of(10u32))?;
    let sets: Array<'a, u16> = sub.array(Offset::of(12u32), Count::of(set_count))?;
    let class = input_def.class(first.id);
    let set_off = sets.get(Index::of(class.0))?;
    if set_off == 0 {
        return None;
    }
    let set = sub.tail(Offset::of(u32::from(set_off)))?;
    let rule_count: u16 = set.read(Offset::ZERO)?;
    let rules: Array<'a, u16> = set.array(Offset::of(2u32), Count::of(rule_count))?;
    let mut ri = Index::ZERO;
    while let Some(off) = rules.get(ri) {
        ri = ri.next();
        let Some(rule) = set.tail(Offset::of(u32::from(off))) else {
            continue;
        };
        let applied = match_rule(n, rule, filter, buf, pos, depth, |which, g, v| {
            let def = match which {
                Which::Backtrack => back_def,
                Which::Input => input_def,
                Which::Lookahead => ahead_def,
            };
            def.class(g).0 == v
        });
        if applied.is_some() {
            return applied;
        }
    }
    None
}

/// Format 3: one rule, every position its own coverage table.
pub(crate) fn chained_coverage<'a, N: Nested<'a>>(
    n: &N,
    sub: Bytes<'a>,
    filter: &Filter<'_>,
    buf: &mut Buffer<'_>,
    pos: Index,
    depth: Depth,
) -> Option<Applied> {
    let back_n: u16 = sub.read(Offset::of(2u32))?;
    let back: Array<'a, u16> = sub.array(Offset::of(4u32), Count::of(back_n))?;
    let at = Offset::of(4u32).plus(Len::of(2).times(Count::of(back_n))?)?;
    let input_n: u16 = sub.read(at)?;
    let input: Array<'a, u16> = sub.array(at.plus(Len::of(2))?, Count::of(input_n))?;
    let at = at
        .plus(Len::of(2))?
        .plus(Len::of(2).times(Count::of(input_n))?)?;
    let ahead_n: u16 = sub.read(at)?;
    let ahead: Array<'a, u16> = sub.array(at.plus(Len::of(2))?, Count::of(ahead_n))?;
    let at = at
        .plus(Len::of(2))?
        .plus(Len::of(2).times(Count::of(ahead_n))?)?;
    let rec_n: u16 = sub.read(at)?;
    let records = sub.tail(at.plus(Len::of(2))?)?;
    let covers = |off: u16, g: GlyphId| -> bool {
        sub.tail(Offset::of(u32::from(off)))
            .and_then(Coverage::parse)
            .is_some_and(|c| c.covers(g))
    };
    // The first input position is this glyph.
    let first = buf.get(pos)?;
    if !covers(input.get(Index::ZERO)?, first.id) {
        return None;
    }
    let m = match_input(filter, buf, pos, Count::of(input_n), |k, g| {
        input.get(k).is_some_and(|off| covers(off, g))
    })?;
    match_back(filter, buf, pos, &back, |g, off| covers(off, g))?;
    match_ahead(filter, buf, m.last(), &ahead, |g, off| covers(off, g))?;
    run_records(n, records, (Count::of(rec_n), filter.stage), &m, buf, depth)
}

/// **Whether a context subtable's rule takes exactly `glyphs` as its
/// input**, for [`crate::Gsub`]'s `would_substitute`: format 1 by glyph,
/// format 2 by class and format 3 by coverage, the first glyph in the
/// subtable's coverage, a rule's input as long as `glyphs` and matching it
/// glyph for glyph, and, for a chained rule asked with
/// [`crate::gsub::Context::Zero`], no backtrack and no lookahead. Nothing
/// is skipped: the question is about these glyphs side by side.
pub(crate) fn would_match(sub: Bytes<'_>, glyphs: &[GlyphId], chain: crate::gsub::Chain) -> bool {
    would_match_at(sub, glyphs, chain).unwrap_or(false)
}

fn would_match_at(sub: Bytes<'_>, glyphs: &[GlyphId], chain: crate::gsub::Chain) -> Option<bool> {
    use crate::gsub::{Chain, Context};
    let first = *glyphs.first()?;
    let n = glyphs.len();
    let format: u16 = sub.read(Offset::ZERO)?;
    let chained = matches!(chain, Chain::Yes(_));
    let zero = matches!(chain, Chain::Yes(Context::Zero));
    // A rule's input after its first glyph, as values `by` reads.
    let input_is = |input: &Array<'_, u16>, by: &dyn Fn(GlyphId, u16) -> bool| {
        glyphs
            .get(1..)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .all(|(k, &g)| {
                input
                    .get(Index::of(u32::try_from(k).unwrap_or(u32::MAX)))
                    .is_some_and(|v| by(g, v))
            })
    };
    match format {
        1 | 2 => {
            let coverage = coverage_at(sub, Offset::of(2u32))?;
            let ci = coverage.index(first)?;
            // Format 2's class definitions: one, or a chained rule's three.
            let class_at = |at: u32| -> Option<ClassDef<'_>> {
                let off: u16 = sub.read(Offset::of(at))?;
                ClassDef::parse(sub.tail(Offset::of(u32::from(off)))?)
            };
            let (input_def, sets_at) = match (format, chained) {
                (1, false) => (None, 4u32),
                (1, true) => (None, 4u32),
                (_, false) => (Some(class_at(4)?), 6u32),
                (_, true) => (Some(class_at(6)?), 10u32),
            };
            let set_count: u16 = sub.read(Offset::of(sets_at))?;
            let sets: Array<'_, u16> =
                sub.array(Offset::of(sets_at).plus(Len::of(2))?, Count::of(set_count))?;
            let set_off = match input_def {
                None => sets.get(ci)?,
                Some(def) => sets.get(Index::of(def.class(first).0))?,
            };
            if set_off == 0 {
                return Some(false);
            }
            let set = sub.tail(Offset::of(u32::from(set_off)))?;
            let rule_count: u16 = set.read(Offset::ZERO)?;
            let rules: Array<'_, u16> = set.array(Offset::of(2u32), Count::of(rule_count))?;
            let by = |g: GlyphId, v: u16| match input_def {
                None => same(g, v),
                Some(def) => def.class(g).0 == v,
            };
            let mut ri = Index::ZERO;
            while let Some(off) = rules.get(ri) {
                ri = ri.next();
                let Some(rule) = set.tail(Offset::of(u32::from(off))) else {
                    continue;
                };
                let (input_n, input_at, back_n, ahead_n) = match chained {
                    false => (
                        rule.read::<u16>(Offset::ZERO)?,
                        Offset::of(4u32),
                        0u16,
                        0u16,
                    ),
                    true => {
                        let back_n: u16 = rule.read(Offset::ZERO)?;
                        let at = Offset::of(2u32).plus(Len::of(2).times(Count::of(back_n))?)?;
                        let input_n: u16 = rule.read(at)?;
                        let rest = Count::of(input_n.saturating_sub(1));
                        let ahead_at = at.plus(Len::of(2))?.plus(Len::of(2).times(rest)?)?;
                        let ahead_n: u16 = rule.read(ahead_at)?;
                        (input_n, at.plus(Len::of(2))?, back_n, ahead_n)
                    }
                };
                if usize::from(input_n) != n || (zero && (back_n != 0 || ahead_n != 0)) {
                    continue;
                }
                let input: Array<'_, u16> =
                    rule.array(input_at, Count::of(input_n.saturating_sub(1)))?;
                if input_is(&input, &by) {
                    return Some(true);
                }
            }
            Some(false)
        }
        3 => {
            let covers = |off: u16, g: GlyphId| -> bool {
                sub.tail(Offset::of(u32::from(off)))
                    .and_then(Coverage::parse)
                    .is_some_and(|c| c.covers(g))
            };
            let (input, back_n, ahead_n) = match chained {
                false => {
                    let input_n: u16 = sub.read(Offset::of(2u32))?;
                    let input: Array<'_, u16> = sub.array(Offset::of(6u32), Count::of(input_n))?;
                    (input, 0u16, 0u16)
                }
                true => {
                    let back_n: u16 = sub.read(Offset::of(2u32))?;
                    let at = Offset::of(4u32).plus(Len::of(2).times(Count::of(back_n))?)?;
                    let input_n: u16 = sub.read(at)?;
                    let input: Array<'_, u16> =
                        sub.array(at.plus(Len::of(2))?, Count::of(input_n))?;
                    let ahead_at = at
                        .plus(Len::of(2))?
                        .plus(Len::of(2).times(Count::of(input_n))?)?;
                    (input, back_n, sub.read::<u16>(ahead_at)?)
                }
            };
            if input.count().slot() != n || (zero && (back_n != 0 || ahead_n != 0)) {
                return Some(false);
            }
            Some(glyphs.iter().enumerate().all(|(k, &g)| {
                input
                    .get(Index::of(u32::try_from(k).unwrap_or(u32::MAX)))
                    .is_some_and(|off| covers(off, g))
            }))
        }
        _ => Some(false),
    }
}
