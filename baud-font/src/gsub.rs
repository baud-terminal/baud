//! `GSUB` (`[[source: ms-opentype-gsub]]`): the substitution lookups a
//! grid needs, and no others.
//!
//! **Types 1, 4, 6 and 7.** Single substitution (both formats), ligature
//! substitution, chained contexts in all three formats, and the extension
//! wrapper -- the set the register (D8) fixed from what programming-ligature
//! fonts and Noto's emoji build actually use: Fira Code's `calt` is chained
//! contexts driving single substitutions, Noto's `ccmp` is ligatures. Types
//! 2, 3, 5 and 8 answer nothing; a font that leans on them shapes as if the
//! feature were absent, which is the honest form of not implementing it.
//!
//! **Matching skips what the lookup flags say to skip**, through
//! [`Filter`], and treats a default-ignorable glyph the way HarfBuzz's
//! `may_skip` does: it matches when the pattern names it and is stepped
//! over when it does not, so a ZWJ inside an emoji sequence is consumed by
//! the ligature that expects it and invisible to a lookup that does not.
//!
//! **The buffer only shrinks**, so every position bookkeeping problem is
//! one of glyphs removed *before* a position: a nested lookup that forms a
//! ligature inside a matched input sequence reports how many glyphs it
//! took, and the positions after it move down by that many.

use crate::buffer::{Buffer, Cluster, Glyph, Ignorable};
use crate::bytes::{Array, Bytes, Count, Index, Len, Offset};
use crate::layout::{
    Class, ClassDef, Coverage, Digest, Features, Filter, Gdef, Kind, Layout, Lookup, LookupSet,
    MAX_LOOKUPS, ScriptTag, Skip,
};
use crate::script::Script;
use baud_domain::GlyphId;

/// How deep nested lookups may recurse; a contextual lookup naming
/// another contextual lookup naming another is unusual past two.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Depth(u8);

impl Depth {
    const TOP: Depth = Depth(0);
    const LIMIT: Depth = Depth(8);

    fn deeper(self) -> Depth {
        Depth(self.0.saturating_add(1))
    }
}

/// The longest input, backtrack or lookahead sequence matched; longer
/// rules are declined rather than matched partially.
const SEQ_MAX: usize = 16;

/// Positions in the buffer of a matched sequence.
#[derive(Clone, Copy)]
struct Matched {
    at: [Index; SEQ_MAX],
    len: Count,
}

impl Matched {
    fn first(pos: Index) -> Matched {
        let mut m = Matched {
            at: [Index::ZERO; SEQ_MAX],
            len: Count::of(1u32),
        };
        m.at[0] = pos;
        m
    }

    fn positions(&self) -> &[Index] {
        self.at.get(..self.len.slot()).unwrap_or(&[])
    }

    fn last(&self) -> Index {
        self.positions().last().copied().unwrap_or(Index::ZERO)
    }

    fn push(&mut self, pos: Index) {
        let n = self.len.slot();
        if let Some(slot) = self.at.get_mut(n) {
            *slot = pos;
            self.len = Count::of(u32::try_from(n + 1).unwrap_or(u32::MAX));
        }
    }
}

/// Whether a lookup substituted anything, which is when the buffer's
/// digest has to be taken again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Changed {
    No,
    Yes,
}

/// What applying a subtable at a position did.
#[derive(Clone, Copy)]
struct Applied {
    /// The position to continue from.
    next: Index,
    /// How many glyphs the buffer lost before `next`.
    removed: Count,
}

impl Applied {
    fn after(pos: Index) -> Applied {
        Applied {
            next: pos.next(),
            removed: Count::ZERO,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Gsub<'a> {
    layout: Layout<'a>,
}

/// One position's glyph, seen through a lookup's filter.
fn seen(filter: &Filter<'_>, g: Glyph) -> Skip {
    filter.skip_class(g.class, g.id)
}

fn same(g: GlyphId, v: u16) -> bool {
    g.index() == usize::from(v)
}

fn bump(c: Count, by: Count) -> Count {
    Count::of(c.value().saturating_add(by.value()))
}

impl<'a> Gsub<'a> {
    pub fn parse(data: Bytes<'a>, gdef: Gdef<'a>) -> Option<Gsub<'a>> {
        Some(Gsub {
            layout: Layout::parse(data, gdef)?,
        })
    }

    /// Applies every lookup the features name for text in `script`, in
    /// lookup order, over the whole buffer. Answers how many named lookups
    /// lay past what the set can hold and so did not run -- zero for any
    /// font this crate has met.
    pub fn apply(&self, buf: &mut Buffer<'_>, script: Script, features: Features) -> Count {
        self.apply_with(buf, script, features, &[])
    }

    /// The lookups the features name for text in `script`: two scripts with
    /// equal sets shape alike on this face.
    pub fn lookups(&self, script: Script, features: Features) -> LookupSet {
        self.layout.lookups_for(script, features)
    }

    /// The tag of the Script table text in `script` is shaped with here.
    pub fn chosen(&self, script: Script) -> Option<ScriptTag> {
        self.layout.chosen(script)
    }

    /// [`Gsub::apply`] with a digest per lookup, as [`Gsub::digests`]
    /// fills them: a lookup whose digest cannot meet the buffer's is not
    /// opened. An empty slice accelerates nothing, and a shorter one
    /// accelerates the lookups it covers.
    pub fn apply_with(
        &self,
        buf: &mut Buffer<'_>,
        script: Script,
        features: Features,
        digests: &[Digest],
    ) -> Count {
        let set = self.layout.lookups_for(script, features);
        let mut have = Gsub::buffer_digest(buf);
        for i in set.iter() {
            if let Some(d) = digests.get(i.slot()) {
                if !d.may_have(have) {
                    continue;
                }
            }
            if let Some(lookup) = self.layout.lookup(i) {
                if self.apply_lookup(&lookup, buf, Depth::TOP, digests.get(i.slot()).copied())
                    == Changed::Yes
                {
                    have = Gsub::buffer_digest(buf);
                }
            }
        }
        set.declined()
    }

    fn buffer_digest(buf: &Buffer<'_>) -> Digest {
        let mut d = Digest::EMPTY;
        for g in buf.as_slice() {
            d.add(g.id);
        }
        d
    }

    /// Fills `out[i]` with the digest of the glyphs lookup `i` can start
    /// on, for every lookup `features` name under any script, so
    /// [`Gsub::apply_with`] can skip the rest. A digest belongs to its lookup
    /// whichever script selected it, so one slice per face serves every
    /// script. `out` is indexed by lookup and holds at most [`MAX_LOOKUPS`];
    /// a lookup past its end is simply not accelerated.
    pub fn digests(&self, features: Features, out: &mut [Digest]) {
        let set = self.layout.lookups_any(features);
        for i in set.iter().take(MAX_LOOKUPS) {
            if let Some(slot) = out.get_mut(i.slot()) {
                *slot = self.lookup_digest(i);
            }
        }
    }

    /// The glyph pairs the grid features' lookups can begin a match on,
    /// derived from the tables and handed to `sink` as (first glyph, a
    /// digest of what may follow it): a prefilter for a path that shapes
    /// only where a substitution can change a run. A ligature needs its
    /// first two components adjacent; a chained rule needs its backtrack's
    /// last and input's first, or its first two inputs, or its input and
    /// lookahead's first. A rule with no context is a single substitution
    /// the codepoint map already applies, and adds nothing. `Unreadable`
    /// means a subtable this reader could not walk, so every pair must be
    /// admitted -- the way an unreadable lookup's digest admits every
    /// glyph.
    pub fn pairs(&self, features: Features, sink: &mut dyn FnMut(GlyphId, Digest)) -> Walked {
        // Every script's lookups: a prefilter that admits a pair one script
        // could begin is right for all of them.
        for i in self.layout.lookups_any(features).iter() {
            let Some(lookup) = self.layout.lookup(i) else {
                return Walked::Unreadable;
            };
            let mut k = Index::ZERO;
            while lookup.subtable_count().holds(k) {
                let Some((kind, sub)) = lookup.subtable(k, Kind::GSUB_EXTENSION) else {
                    return Walked::Unreadable;
                };
                let read = match kind {
                    Kind::LIGATURE => Gsub::ligature_pairs(sub, sink),
                    Kind::CHAINED => Gsub::chained_pairs(sub, sink),
                    _ => Some(()),
                };
                if read.is_none() {
                    return Walked::Unreadable;
                }
                k = k.next();
            }
        }
        Walked::Derived
    }

    /// The longest sequence a substitution the grid features name can
    /// match, in glyphs: a ligature's component count, a chained rule's
    /// backtrack, input and lookahead together. A window of adjacent
    /// cells this wide on either side of a pair holds every rule that
    /// pair could begin, so the window shapes to what the row shapes to.
    /// `None` when a subtable could not be walked and no bound is known.
    pub fn extent(&self, features: Features) -> Option<Count> {
        let mut widest = Count::of(1u16);
        // The widest rule under any script, so a window is wide enough
        // whichever script shapes it.
        for i in self.layout.lookups_any(features).iter() {
            let lookup = self.layout.lookup(i)?;
            let mut k = Index::ZERO;
            while lookup.subtable_count().holds(k) {
                let (kind, sub) = lookup.subtable(k, Kind::GSUB_EXTENSION)?;
                let here = match kind {
                    Kind::LIGATURE => Gsub::ligature_extent(sub)?,
                    Kind::CHAINED => Gsub::chained_extent(sub)?,
                    _ => Count::of(1u16),
                };
                if here.value() > widest.value() {
                    widest = here;
                }
                k = k.next();
            }
        }
        Some(widest)
    }

    fn ligature_extent(sub: Bytes<'a>) -> Option<Count> {
        let format: u16 = sub.read(Offset::ZERO)?;
        if format != 1 {
            return None;
        }
        let set_count: u16 = sub.read(Offset::of(4u32))?;
        let sets: Array<'a, u16> = sub.array(Offset::of(6u32), Count::of(set_count))?;
        let mut widest = Count::of(1u16);
        let mut si = Index::ZERO;
        while let Some(off) = sets.get(si) {
            si = si.next();
            let set = sub.tail(Offset::of(u32::from(off)))?;
            let lig_count: u16 = set.read(Offset::ZERO)?;
            let ligs: Array<'a, u16> = set.array(Offset::of(2u32), Count::of(lig_count))?;
            let mut li = Index::ZERO;
            while let Some(off) = ligs.get(li) {
                li = li.next();
                let lig = set.tail(Offset::of(u32::from(off)))?;
                let count: u16 = lig.read(Offset::of(2u32))?;
                if u32::from(count) > widest.value() {
                    widest = Count::of(count);
                }
            }
        }
        Some(widest)
    }

    fn chained_extent(sub: Bytes<'a>) -> Option<Count> {
        let format: u16 = sub.read(Offset::ZERO)?;
        match format {
            1 | 2 => {
                let sets_at = match format {
                    1 => Offset::of(4u32),
                    _ => Offset::of(10u32),
                };
                let set_count: u16 = sub.read(sets_at)?;
                let sets: Array<'a, u16> =
                    sub.array(sets_at.plus(Len::of(2))?, Count::of(set_count))?;
                let mut widest = Count::of(1u16);
                let mut si = Index::ZERO;
                while let Some(off) = sets.get(si) {
                    si = si.next();
                    if off == 0 {
                        continue;
                    }
                    let set = sub.tail(Offset::of(u32::from(off)))?;
                    let rule_count: u16 = set.read(Offset::ZERO)?;
                    let rules: Array<'a, u16> =
                        set.array(Offset::of(2u32), Count::of(rule_count))?;
                    let mut ri = Index::ZERO;
                    while let Some(off) = rules.get(ri) {
                        ri = ri.next();
                        let rule = set.tail(Offset::of(u32::from(off)))?;
                        let back_n: u16 = rule.read(Offset::ZERO)?;
                        let at = Offset::of(2u32).plus(Len::of(2).times(Count::of(back_n))?)?;
                        let input_n: u16 = rule.read(at)?;
                        let rest = Count::of(input_n.checked_sub(1)?);
                        let at = at.plus(Len::of(2))?.plus(Len::of(2).times(rest)?)?;
                        let ahead_n: u16 = rule.read(at)?;
                        let span = back_n.saturating_add(input_n).saturating_add(ahead_n);
                        if u32::from(span) > widest.value() {
                            widest = Count::of(span);
                        }
                    }
                }
                Some(widest)
            }
            3 => {
                let back_n: u16 = sub.read(Offset::of(2u32))?;
                let at = Offset::of(4u32).plus(Len::of(2).times(Count::of(back_n))?)?;
                let input_n: u16 = sub.read(at)?;
                let at = at
                    .plus(Len::of(2))?
                    .plus(Len::of(2).times(Count::of(input_n))?)?;
                let ahead_n: u16 = sub.read(at)?;
                Some(Count::of(
                    back_n
                        .saturating_add(input_n)
                        .saturating_add(ahead_n)
                        .max(1),
                ))
            }
            _ => None,
        }
    }

    fn ligature_pairs(sub: Bytes<'a>, sink: &mut dyn FnMut(GlyphId, Digest)) -> Option<()> {
        let format: u16 = sub.read(Offset::ZERO)?;
        if format != 1 {
            return None;
        }
        let coverage = Gsub::coverage_at(sub, Offset::of(2u32))?;
        let set_count: u16 = sub.read(Offset::of(4u32))?;
        let sets: Array<'a, u16> = sub.array(Offset::of(6u32), Count::of(set_count))?;
        for (ci, first) in coverage.glyphs().enumerate() {
            let Some(off) = u32::try_from(ci).ok().and_then(|i| sets.get(Index::of(i))) else {
                break;
            };
            let set = sub.tail(Offset::of(u32::from(off)))?;
            let lig_count: u16 = set.read(Offset::ZERO)?;
            let ligs: Array<'a, u16> = set.array(Offset::of(2u32), Count::of(lig_count))?;
            let mut li = Index::ZERO;
            while let Some(off) = ligs.get(li) {
                li = li.next();
                let lig = set.tail(Offset::of(u32::from(off)))?;
                let count: u16 = lig.read(Offset::of(2u32))?;
                if count >= 2 {
                    let second: u16 = lig.read(Offset::of(4u32))?;
                    sink(first, Digest::of(GlyphId::from_shaper(second)));
                }
            }
        }
        Some(())
    }

    fn chained_pairs(sub: Bytes<'a>, sink: &mut dyn FnMut(GlyphId, Digest)) -> Option<()> {
        let format: u16 = sub.read(Offset::ZERO)?;
        match format {
            1 => {
                let coverage = Gsub::coverage_at(sub, Offset::of(2u32))?;
                let set_count: u16 = sub.read(Offset::of(4u32))?;
                let sets: Array<'a, u16> = sub.array(Offset::of(6u32), Count::of(set_count))?;
                for (ci, first) in coverage.glyphs().enumerate() {
                    let Some(off) = u32::try_from(ci).ok().and_then(|i| sets.get(Index::of(i)))
                    else {
                        break;
                    };
                    if off == 0 {
                        continue;
                    }
                    let set = sub.tail(Offset::of(u32::from(off)))?;
                    Gsub::rule_set_pairs(set, &Values::Glyphs, first, sink)?;
                }
                Some(())
            }
            2 => {
                let coverage = Gsub::coverage_at(sub, Offset::of(2u32))?;
                let class_def = |at: Offset| -> Option<ClassDef<'a>> {
                    let off: u16 = sub.read(at)?;
                    ClassDef::parse(sub.tail(Offset::of(u32::from(off)))?)
                };
                let values = Values::Classes {
                    back: class_def(Offset::of(4u32))?,
                    input: class_def(Offset::of(6u32))?,
                    ahead: class_def(Offset::of(8u32))?,
                };
                let set_count: u16 = sub.read(Offset::of(10u32))?;
                let sets: Array<'a, u16> = sub.array(Offset::of(12u32), Count::of(set_count))?;
                for first in coverage.glyphs() {
                    let Values::Classes { input, .. } = &values else {
                        return None;
                    };
                    let class = input.class(first);
                    let Some(off) = sets.get(Index::of(class.0)) else {
                        continue;
                    };
                    if off == 0 {
                        continue;
                    }
                    let set = sub.tail(Offset::of(u32::from(off)))?;
                    Gsub::rule_set_pairs(set, &values, first, sink)?;
                }
                Some(())
            }
            3 => {
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
                let cov = |off: u16| -> Option<Coverage<'a>> {
                    Coverage::parse(sub.tail(Offset::of(u32::from(off)))?)
                };
                let firsts = cov(input.get(Index::ZERO)?)?;
                if back_n > 0 {
                    let backs = cov(back.get(Index::ZERO)?)?;
                    let d = firsts.digest();
                    for b in backs.glyphs() {
                        sink(b, d);
                    }
                }
                let next = match input_n >= 2 {
                    true => input.get(Index::of(1u32)),
                    false => ahead.get(Index::ZERO),
                };
                if let Some(off) = next {
                    let d = cov(off)?.digest();
                    for f in firsts.glyphs() {
                        sink(f, d);
                    }
                }
                Some(())
            }
            _ => None,
        }
    }

    /// Every rule of a rule set, as the pairs it can begin on.
    fn rule_set_pairs(
        set: Bytes<'a>,
        values: &Values<'a>,
        first: GlyphId,
        sink: &mut dyn FnMut(GlyphId, Digest),
    ) -> Option<()> {
        let rule_count: u16 = set.read(Offset::ZERO)?;
        let rules: Array<'a, u16> = set.array(Offset::of(2u32), Count::of(rule_count))?;
        let mut ri = Index::ZERO;
        while let Some(off) = rules.get(ri) {
            ri = ri.next();
            let rule = set.tail(Offset::of(u32::from(off)))?;
            let back_n: u16 = rule.read(Offset::ZERO)?;
            let back: Array<'a, u16> = rule.array(Offset::of(2u32), Count::of(back_n))?;
            let at = Offset::of(2u32).plus(Len::of(2).times(Count::of(back_n))?)?;
            let input_n: u16 = rule.read(at)?;
            let rest = Count::of(input_n.checked_sub(1)?);
            let input: Array<'a, u16> = rule.array(at.plus(Len::of(2))?, rest)?;
            let at = at.plus(Len::of(2))?.plus(Len::of(2).times(rest)?)?;
            let ahead_n: u16 = rule.read(at)?;
            let ahead: Array<'a, u16> = rule.array(at.plus(Len::of(2))?, Count::of(ahead_n))?;
            if let Some(b) = back.get(Index::ZERO) {
                let d = Digest::of(first);
                values.each(Which::Backtrack, Value(b), &mut |g| sink(g, d));
            }
            let next = match input.get(Index::ZERO) {
                Some(v) => Some(values.digest(Which::Input, Value(v))),
                None => ahead
                    .get(Index::ZERO)
                    .map(|v| values.digest(Which::Lookahead, Value(v))),
            };
            if let Some(d) = next {
                sink(first, d);
            }
        }
        Some(())
    }

    /// The glyphs lookup `i` can act at: the union of its subtables'
    /// first-position coverages. Anything unreadable widens to everything,
    /// so an odd table is slow rather than skipped.
    fn lookup_digest(&self, i: Index) -> Digest {
        let Some(lookup) = self.layout.lookup(i) else {
            return Digest::ALL;
        };
        let mut d = Digest::EMPTY;
        let mut k = Index::ZERO;
        while lookup.subtable_count().holds(k) {
            let Some((kind, sub)) = lookup.subtable(k, Kind::GSUB_EXTENSION) else {
                return Digest::ALL;
            };
            let cov = match kind {
                Kind::SINGLE | Kind::LIGATURE => Gsub::coverage_at(sub, Offset::of(2u32)),
                Kind::CHAINED => Gsub::chained_first_coverage(sub),
                _ => None,
            };
            match cov {
                Some(c) => d = d.union(c.digest()),
                None => return Digest::ALL,
            }
            k = k.next();
        }
        d
    }

    /// The coverage a chained subtable's current glyph must be in: the
    /// subtable's own for formats 1 and 2, the first input coverage for
    /// format 3.
    fn chained_first_coverage(sub: Bytes<'a>) -> Option<Coverage<'a>> {
        let format: u16 = sub.read(Offset::ZERO)?;
        match format {
            1 | 2 => Gsub::coverage_at(sub, Offset::of(2u32)),
            3 => {
                let back_n: u16 = sub.read(Offset::of(2u32))?;
                let at = Offset::of(4u32).plus(Len::of(2).times(Count::of(back_n))?)?;
                let input_n: u16 = sub.read(at)?;
                if input_n == 0 {
                    return None;
                }
                Gsub::coverage_at(sub, at.plus(Len::of(2))?)
            }
            _ => None,
        }
    }

    /// Every position the filter admits, offered to the lookup -- except
    /// where `digest`, the glyphs the lookup can start on, cannot hold
    /// the glyph there, which is HarfBuzz's per-position test and what
    /// keeps a face with hundreds of contextual lookups from searching
    /// every subtable's coverage at every glyph.
    fn apply_lookup(
        &self,
        lookup: &Lookup<'a>,
        buf: &mut Buffer<'_>,
        depth: Depth,
        digest: Option<Digest>,
    ) -> Changed {
        let mut changed = Changed::No;
        let mut pos = Index::ZERO;
        while let Some(g) = buf.get(pos) {
            if seen(&lookup.filter, g) == Skip::Yes
                || digest.is_some_and(|d| !d.may_have(Digest::of(g.id)))
            {
                pos = pos.next();
                continue;
            }
            pos = match self.apply_at(lookup, buf, pos, depth) {
                Some(a) => {
                    changed = Changed::Yes;
                    a.next
                }
                None => pos.next(),
            };
        }
        changed
    }

    /// Tries each subtable at `pos`; the first that applies wins.
    fn apply_at(
        &self,
        lookup: &Lookup<'a>,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
    ) -> Option<Applied> {
        let mut i = Index::ZERO;
        while lookup.subtable_count().holds(i) {
            let (kind, sub) = lookup.subtable(i, Kind::GSUB_EXTENSION)?;
            let applied = match kind {
                Kind::SINGLE => self.single(sub, buf, pos),
                Kind::LIGATURE => self.ligature(sub, &lookup.filter, buf, pos),
                Kind::CHAINED => self.chained(sub, &lookup.filter, buf, pos, depth),
                _ => None,
            };
            if applied.is_some() {
                return applied;
            }
            i = i.next();
        }
        None
    }

    fn coverage_at(sub: Bytes<'a>, at: Offset) -> Option<Coverage<'a>> {
        let off: u16 = sub.read(at)?;
        Coverage::parse(sub.tail(Offset::of(u32::from(off)))?)
    }

    /// Type 1: one glyph for another, by delta or by table.
    fn single(&self, sub: Bytes<'a>, buf: &mut Buffer<'_>, pos: Index) -> Option<Applied> {
        let format: u16 = sub.read(Offset::ZERO)?;
        let coverage = Gsub::coverage_at(sub, Offset::of(2u32))?;
        let g = buf.get(pos)?;
        let ci = coverage.index(g.id)?;
        let out = match format {
            1 => {
                let delta: i16 = sub.read(Offset::of(4u32))?;
                let gid = u16::try_from(g.id.index()).ok()?;
                // "Addition of deltaGlyphID is modulo 65536."
                gid.wrapping_add(u16::from_ne_bytes(delta.to_ne_bytes()))
            }
            2 => {
                let count: u16 = sub.read(Offset::of(4u32))?;
                let subs: Array<'a, u16> = sub.array(Offset::of(6u32), Count::of(count))?;
                subs.get(ci)?
            }
            _ => return None,
        };
        let g = buf.get_mut(pos)?;
        g.id = GlyphId::from_shaper(out);
        g.class = self.layout.gdef().class(g.id);
        Some(Applied::after(pos))
    }

    /// Walks forward from `from` to the next glyph the filter does not
    /// skip, applying the default-ignorable rule against `wanted`.
    fn next_match(
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
    fn prev_match(
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
    fn match_input(
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
            j = Gsub::next_match(filter, buf, j.next(), |g| test(k, g))?;
            m.push(j);
            k = k.next();
        }
        Some(m)
    }

    /// Type 4: a sequence of components becomes one glyph.
    fn ligature(
        &self,
        sub: Bytes<'a>,
        filter: &Filter<'_>,
        buf: &mut Buffer<'_>,
        pos: Index,
    ) -> Option<Applied> {
        let format: u16 = sub.read(Offset::ZERO)?;
        if format != 1 {
            return None;
        }
        let coverage = Gsub::coverage_at(sub, Offset::of(2u32))?;
        let first = buf.get(pos)?;
        let ci = coverage.index(first.id)?;
        let set_count: u16 = sub.read(Offset::of(4u32))?;
        let sets: Array<'a, u16> = sub.array(Offset::of(6u32), Count::of(set_count))?;
        let set = sub.tail(Offset::of(u32::from(sets.get(ci)?)))?;
        let lig_count: u16 = set.read(Offset::ZERO)?;
        let ligs: Array<'a, u16> = set.array(Offset::of(2u32), Count::of(lig_count))?;
        let mut li = Index::ZERO;
        while let Some(off) = ligs.get(li) {
            li = li.next();
            let Some(lig) = set.tail(Offset::of(u32::from(off))) else {
                continue;
            };
            let Some(out) = lig.read::<u16>(Offset::ZERO) else {
                continue;
            };
            let Some(count) = lig.read::<u16>(Offset::of(2u32)) else {
                continue;
            };
            let Some(components) =
                lig.array::<u16>(Offset::of(4u32), Count::of(count.saturating_sub(1)))
            else {
                continue;
            };
            let Some(m) = Gsub::match_input(filter, buf, pos, Count::of(count), |k, g| {
                k.prev()
                    .and_then(|c| components.get(c))
                    .is_some_and(|c| same(g, c))
            }) else {
                continue;
            };
            return Some(Gsub::form_ligature(
                buf,
                &m,
                GlyphId::from_shaper(out),
                &self.layout.gdef(),
            ));
        }
        None
    }

    /// Replaces the first matched glyph with `out`, spanning every matched
    /// cell, and removes the other matched components. Glyphs the match
    /// skipped over -- marks, ignorables -- stay where they are.
    fn form_ligature(buf: &mut Buffer<'_>, m: &Matched, out: GlyphId, gdef: &Gdef<'_>) -> Applied {
        let first = m.at[0];
        let mut last = Cluster(0);
        for &p in m.positions() {
            if let Some(g) = buf.get(p) {
                last = last.max(g.last);
            }
        }
        if let Some(g) = buf.get_mut(first) {
            g.id = out;
            g.class = gdef.class(out);
            g.last = last.max(g.last);
        }
        // Remove from the back so earlier positions stay valid.
        let mut removed = Count::ZERO;
        for &p in m.positions().iter().skip(1).rev() {
            if buf.remove(p).is_some() {
                removed = bump(removed, Count::of(1u32));
            }
        }
        Applied {
            next: first.next(),
            removed,
        }
    }

    /// Type 6: chained contexts, in the format the subtable says.
    fn chained(
        &self,
        sub: Bytes<'a>,
        filter: &Filter<'_>,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
    ) -> Option<Applied> {
        let format: u16 = sub.read(Offset::ZERO)?;
        match format {
            1 => self.chained_glyphs(sub, filter, buf, pos, depth),
            2 => self.chained_classes(sub, filter, buf, pos, depth),
            3 => self.chained_coverage(sub, filter, buf, pos, depth),
            _ => None,
        }
    }

    /// A rule's three sequences and its lookup records, as laid out in a
    /// `ChainedSequenceRule` or `ChainedClassSequenceRule`: counts and
    /// `u16` arrays back to back.
    fn match_rule(
        &self,
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
        let m = Gsub::match_input(filter, buf, pos, Count::of(input_n), |k, g| {
            k.prev()
                .and_then(|c| input.get(c))
                .is_some_and(|v| by(Which::Input, g, v))
        })?;
        Gsub::match_back(filter, buf, pos, &back, |g, v| by(Which::Backtrack, g, v))?;
        Gsub::match_ahead(filter, buf, m.last(), &ahead, |g, v| {
            by(Which::Lookahead, g, v)
        })?;
        self.run_records(records, Count::of(rec_n), &m, buf, depth)
    }

    fn match_back(
        filter: &Filter<'_>,
        buf: &Buffer<'_>,
        pos: Index,
        pattern: &Array<'a, u16>,
        by: impl Fn(GlyphId, u16) -> bool,
    ) -> Option<()> {
        let mut j = pos;
        let mut k = Index::ZERO;
        while let Some(v) = pattern.get(k) {
            j = Gsub::prev_match(filter, buf, j, |g| by(g, v))?;
            k = k.next();
        }
        Some(())
    }

    fn match_ahead(
        filter: &Filter<'_>,
        buf: &Buffer<'_>,
        last_input: Index,
        pattern: &Array<'a, u16>,
        by: impl Fn(GlyphId, u16) -> bool,
    ) -> Option<()> {
        let mut j = last_input;
        let mut k = Index::ZERO;
        while let Some(v) = pattern.get(k) {
            j = Gsub::next_match(filter, buf, j.next(), |g| by(g, v))?;
            k = k.next();
        }
        Some(())
    }

    /// Runs the sequence lookup records over a matched input, adjusting
    /// later positions for glyphs a nested ligature removed.
    fn run_records(
        &self,
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
            let Some(lookup) = self.layout.lookup(Index::of(li)) else {
                continue;
            };
            let p = at.at[k.slot()];
            if let Some(a) = self.apply_at(&lookup, buf, p, depth.deeper()) {
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
    fn chained_glyphs(
        &self,
        sub: Bytes<'a>,
        filter: &Filter<'_>,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
    ) -> Option<Applied> {
        let coverage = Gsub::coverage_at(sub, Offset::of(2u32))?;
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
            let applied = self.match_rule(rule, filter, buf, pos, depth, |_, g, v| same(g, v));
            if applied.is_some() {
                return applied;
            }
        }
        None
    }

    /// Format 2: sequences of classes, three class definitions.
    fn chained_classes(
        &self,
        sub: Bytes<'a>,
        filter: &Filter<'_>,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
    ) -> Option<Applied> {
        let coverage = Gsub::coverage_at(sub, Offset::of(2u32))?;
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
            let applied = self.match_rule(rule, filter, buf, pos, depth, |which, g, v| {
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
    fn chained_coverage(
        &self,
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
        let m = Gsub::match_input(filter, buf, pos, Count::of(input_n), |k, g| {
            input.get(k).is_some_and(|off| covers(off, g))
        })?;
        Gsub::match_back(filter, buf, pos, &back, |g, off| covers(off, g))?;
        Gsub::match_ahead(filter, buf, m.last(), &ahead, |g, off| covers(off, g))?;
        self.run_records(records, Count::of(rec_n), &m, buf, depth)
    }
}

/// Which of a chained rule's three sequences a test is for.
#[derive(Clone, Copy)]
enum Which {
    Backtrack,
    Input,
    Lookahead,
}

/// Whether `Gsub::pairs` walked every subtable, or met one it could not
/// read and the caller must admit every pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Walked {
    Derived,
    Unreadable,
}

/// A rule's value at one position: a glyph id under a glyph rule, a class
/// number under a class rule. Which it is, `Values` knows.
#[derive(Clone, Copy)]
struct Value(u16);

/// How a chained rule's values name glyphs: as the glyph ids themselves
/// (format 1) or as classes of three class tables (format 2).
enum Values<'a> {
    Glyphs,
    Classes {
        back: ClassDef<'a>,
        input: ClassDef<'a>,
        ahead: ClassDef<'a>,
    },
}

impl Values<'_> {
    fn digest(&self, which: Which, v: Value) -> Digest {
        match self {
            Values::Glyphs => Digest::of(GlyphId::from_shaper(v.0)),
            Values::Classes { back, input, ahead } => {
                let def = match which {
                    Which::Backtrack => back,
                    Which::Input => input,
                    Which::Lookahead => ahead,
                };
                let mut d = Digest::EMPTY;
                for g in def.glyphs_of(Class(v.0)) {
                    d.add(g);
                }
                d
            }
        }
    }

    fn each(&self, which: Which, v: Value, f: &mut dyn FnMut(GlyphId)) {
        match self {
            Values::Glyphs => f(GlyphId::from_shaper(v.0)),
            Values::Classes { back, input, ahead } => {
                let def = match which {
                    Which::Backtrack => back,
                    Which::Input => input,
                    Which::Lookahead => ahead,
                };
                for g in def.glyphs_of(Class(v.0)) {
                    f(g);
                }
            }
        }
    }
}
