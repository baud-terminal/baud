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

use crate::buffer::{Buffer, Cluster};
use crate::bytes::{Array, Bytes, Count, Index, Len, Offset};
use crate::context::{
    Applied, Depth, Matched, Nested, Which, bump, chained, coverage_at, match_input, same, seen,
};
use crate::layout::{
    Class, ClassDef, Coverage, Digest, Features, Filter, Gdef, Kind, Layout, Lookup, LookupSet,
    MAX_LOOKUPS, ScriptTag, Skip,
};
use crate::script::Script;
use baud_domain::GlyphId;

/// Whether a lookup substituted anything, which is when the buffer's
/// digest has to be taken again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Changed {
    No,
    Yes,
}

#[derive(Clone, Copy)]
pub struct Gsub<'a> {
    layout: Layout<'a>,
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
        let coverage = coverage_at(sub, Offset::of(2u32))?;
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
                let coverage = coverage_at(sub, Offset::of(2u32))?;
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
                let coverage = coverage_at(sub, Offset::of(2u32))?;
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
                Kind::SINGLE | Kind::LIGATURE => coverage_at(sub, Offset::of(2u32)),
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
            1 | 2 => coverage_at(sub, Offset::of(2u32)),
            3 => {
                let back_n: u16 = sub.read(Offset::of(2u32))?;
                let at = Offset::of(4u32).plus(Len::of(2).times(Count::of(back_n))?)?;
                let input_n: u16 = sub.read(at)?;
                if input_n == 0 {
                    return None;
                }
                coverage_at(sub, at.plus(Len::of(2))?)
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
                Kind::CHAINED => chained(self, sub, &lookup.filter, buf, pos, depth),
                _ => None,
            };
            if applied.is_some() {
                return applied;
            }
            i = i.next();
        }
        None
    }

    /// Type 1: one glyph for another, by delta or by table.
    fn single(&self, sub: Bytes<'a>, buf: &mut Buffer<'_>, pos: Index) -> Option<Applied> {
        let format: u16 = sub.read(Offset::ZERO)?;
        let coverage = coverage_at(sub, Offset::of(2u32))?;
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
        let coverage = coverage_at(sub, Offset::of(2u32))?;
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
            let Some(m) = match_input(filter, buf, pos, Count::of(count), |k, g| {
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
}

impl<'a> Nested<'a> for Gsub<'a> {
    fn nested(
        &self,
        lookup: Index,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
    ) -> Option<Applied> {
        let lookup = self.layout.lookup(lookup)?;
        self.apply_at(&lookup, buf, pos, depth)
    }
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
