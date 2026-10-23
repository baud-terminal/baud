//! Contextual lookups, which `GSUB` (`[[source: ms-opentype-gsub]]`, type
//! 6) and `GPOS` (`[[source: ms-opentype-gpos]]`, type 8) lay out alike:
//! a chained rule matches a backtrack, an input and a lookahead sequence,
//! by glyph, by class or by coverage, and then runs other lookups of its
//! own table at positions of the input. The matching is one code for both
//! tables; what a nested lookup does is the table's, through [`Nested`].

use crate::buffer::{Buffer, Glyph, Ignorable};
use crate::bytes::{Array, Bytes, Count, Index, Len, Offset};
use crate::layout::{ClassDef, Coverage, Filter, Skip};
use baud_domain::GlyphId;

/// A table whose lookups a contextual rule can run: `nested` applies
/// lookup `lookup` at `pos`, the first of its subtables that applies
/// winning, and says what it did.
pub(crate) trait Nested<'a> {
    fn nested(
        &self,
        lookup: Index,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
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

/// What applying a subtable at a position did.
#[derive(Clone, Copy)]
pub(crate) struct Applied {
    /// The position to continue from.
    pub(crate) next: Index,
    /// How many glyphs the buffer lost before `next`.
    pub(crate) removed: Count,
}

impl Applied {
    pub(crate) fn after(pos: Index) -> Applied {
        Applied {
            next: pos.next(),
            removed: Count::ZERO,
        }
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
    while count.holds(k) {
        j = next_match(filter, buf, j.next(), |g| test(k, g))?;
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
    run_records(n, records, Count::of(rec_n), &m, buf, depth)
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
    while let Some(v) = pattern.get(k) {
        j = prev_match(filter, buf, j, |g| by(g, v))?;
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
    while let Some(v) = pattern.get(k) {
        j = next_match(filter, buf, j.next(), |g| by(g, v))?;
        k = k.next();
    }
    Some(())
}

/// Runs the sequence lookup records over a matched input, adjusting
/// later positions for glyphs a nested ligature removed.
pub(crate) fn run_records<'a, N: Nested<'a>>(
    n: &N,
    records: Bytes<'a>,
    count: Count,
    m: &Matched,
    buf: &mut Buffer<'_>,
    depth: Depth,
) -> Option<Applied> {
    if depth >= Depth::LIMIT {
        return None;
    }
    let mut at = *m;
    let mut removed_total = Count::ZERO;
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
        if let Some(a) = n.nested(Index::of(li), buf, p, depth.deeper()) {
            if !a.removed.is_zero() {
                removed_total = bump(removed_total, a.removed);
                let n = at.len.slot();
                for q in at.at.iter_mut().take(n) {
                    if *q > p {
                        *q = Index::of(q.value().saturating_sub(a.removed.value()));
                    }
                }
            }
        }
    }
    // Continue after the input sequence, wherever it ended up.
    Some(Applied {
        next: at.last().next(),
        removed: removed_total,
    })
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
    run_records(n, records, Count::of(rec_n), &m, buf, depth)
}
