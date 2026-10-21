//! **What a search matches**: a query against the text of the grid, by
//! canonical caseless match over whole grapheme clusters.
//!
//! The comparison is `[[source: unicode17-ch3]]` D145, "A string X is a
//! canonical caseless match for a string Y if and only if:
//! NFD(toCasefold(NFD(X))) = NFD(toCasefold(NFD(Y)))", where R4's
//! toCasefold maps each character to its full case folding, status C and F
//! of `[[source: unicode-casefolding]]` -- not the simple folding, which the
//! text keeps for "compatibility with legacy definitions of
//! case-insensitivity". So `STRASSE` finds `Straße`, a typed `e` + U+0301
//! finds a precomposed `é`, and the reverse: the platform's own caseless
//! find answers the same on this Mac (the record, "Search in the
//! scrollback").
//!
//! **A match is whole clusters.** `[[source: uts10]]` § 11 calls it "Whole
//! Characters Only": "Users may not want a search for \"abc\" to match in
//! \"...abç...\" (with a cedilla on the c)", and `[[source: uax29]]` names
//! the grapheme cluster "the fundamental unit of matching". The grid already
//! holds one cluster a cell, so each cell's cluster is keyed on its own and
//! a match must begin at the start of a cluster's key and end at the end of
//! one: `e` does not match the `e` of `e` + U+0301, and `ss` matches the
//! whole of `ß` but `s` matches none of it.
//!
//! Everything a key needs is one index-only table, `search_data.rs`,
//! generated from the pinned data; nothing here searches a table. The
//! matcher is Knuth-Morris-Pratt over the keys, linear in the text, with a
//! ring of the last [`NEEDLE_MAX`] keyed code points so a match's start can
//! be checked against a cluster's start without keeping the line.

use crate::cell::ClusterSlot;
use crate::decode::Codepoint;
use crate::search_data::{
    DECOMP_MAX, FOLD_MAX, SEARCH_BLOCKS, SEARCH_INDEX, SEARCH_POOL, SEARCH_PROPS, SEARCH_SHIFT,
};

/// The longest query, in code points after folding and decomposing.
/// Longer than any line a person types into a find field; one past it is
/// refused rather than cut, since a cut query would find text the query
/// does not.
pub const NEEDLE_MAX: usize = 256;

/// The longest key one cell's cluster can have: its base and every
/// continuation a cluster slot holds, each decomposed and then folded and
/// decomposed again, at the longest the tables hold.
pub const CLUSTER_KEY_MAX: usize = (1 + ClusterSlot::CAPACITY) * DECOMP_MAX * FOLD_MAX;

const S_BASE: u32 = 0xAC00;
const S_COUNT: u32 = 11172;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = 21 * T_COUNT;

/// A code point's row of the table: its decomposition, its folding and
/// its canonical combining class.
fn props(cp: u32) -> (u16, u16, u8) {
    let block = usize::from(SEARCH_INDEX[(cp >> SEARCH_SHIFT) as usize]);
    let at = (block << SEARCH_SHIFT) | (cp & ((1 << SEARCH_SHIFT) - 1)) as usize;
    SEARCH_PROPS[usize::from(SEARCH_BLOCKS[at])]
}

fn ccc(cp: u32) -> u8 {
    match cp < 0x80 {
        true => 0,
        false => props(cp).2,
    }
}

/// A sequence of the pool, or `None` for the code point itself.
fn pooled(ix: u16) -> Option<&'static [u32]> {
    let i = usize::from(ix);
    (i != 0).then(|| {
        let n = SEARCH_POOL[i] as usize;
        &SEARCH_POOL[i + 1..i + 1 + n]
    })
}

/// Keyed code points, up to `N`.
#[derive(Clone, Copy, Debug)]
pub struct SearchKey<const N: usize> {
    cps: [u32; N],
    len: usize,
}

/// A key would not fit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Full;

impl<const N: usize> SearchKey<N> {
    pub const EMPTY: SearchKey<N> = SearchKey {
        cps: [0; N],
        len: 0,
    };

    pub fn as_slice(&self) -> &[u32] {
        &self.cps[..self.len]
    }

    fn push(&mut self, cp: u32) -> Result<(), Full> {
        let slot = self.cps.get_mut(self.len).ok_or(Full)?;
        *slot = cp;
        self.len += 1;
        Ok(())
    }

    fn extend(&mut self, cps: &[u32]) -> Result<(), Full> {
        cps.iter().try_for_each(|&c| self.push(c))
    }

    /// The canonical ordering algorithm (`[[source: unicode17-ch3]]` § 3.11):
    /// each run of characters whose class is not zero, stably sorted by
    /// class. A run is a handful of marks, so an insertion sort.
    fn reorder(&mut self) {
        let s = &mut self.cps[..self.len];
        for i in 1..s.len() {
            let c = ccc(s[i]);
            if c == 0 {
                continue;
            }
            let mut j = i;
            while j > 0 && ccc(s[j - 1]) > c {
                s.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    /// The key of `cps`: NFD(toCasefold(NFD(cps))), D145's.
    fn of(cps: impl Iterator<Item = u32>) -> Result<SearchKey<N>, Full> {
        let mut nfd = SearchKey::<N>::EMPTY;
        for cp in cps {
            if cp < 0x80 {
                nfd.push(cp)?;
            } else if (S_BASE..S_BASE + S_COUNT).contains(&cp) {
                // `[[source: unicode17-ch3]]` § 3.12's arithmetic.
                let s = cp - S_BASE;
                nfd.push(L_BASE + s / N_COUNT)?;
                nfd.push(V_BASE + (s % N_COUNT) / T_COUNT)?;
                if !s.is_multiple_of(T_COUNT) {
                    nfd.push(T_BASE + s % T_COUNT)?;
                }
            } else {
                match pooled(props(cp).0) {
                    Some(d) => nfd.extend(d)?,
                    None => nfd.push(cp)?,
                }
            }
        }
        nfd.reorder();
        let mut out = SearchKey::<N>::EMPTY;
        for &cp in nfd.as_slice() {
            if cp < 0x80 {
                out.push(ascii_fold(cp))?;
                continue;
            }
            match pooled(props(cp).1) {
                Some(g) => out.extend(g)?,
                None => out.push(cp)?,
            }
        }
        // D145's outer NFD. Over every code point with every mark, before
        // or after it, the folding left the marks in order already (the
        // record, "Search in the scrollback"); it is the definition, and a
        // later Unicode is free to need it.
        out.reorder();
        Ok(out)
    }
}

/// An ASCII code point's folding, which is its decomposition's too: `A`
/// to `Z` fold to `a` to `z` and nothing else in the block moves.
fn ascii_fold(cp: u32) -> u32 {
    match cp {
        0x41..=0x5A => cp + 0x20,
        _ => cp,
    }
}

/// One cell's key: its base and its continuations, as many as a cluster
/// slot holds.
pub type ClusterKey = SearchKey<CLUSTER_KEY_MAX>;

impl ClusterKey {
    /// The key of the cluster `base` heads, with `marks` after it.
    pub fn cluster(base: Codepoint, marks: impl Iterator<Item = u32>) -> ClusterKey {
        let cps = core::iter::once(base.value()).chain(marks.take(ClusterSlot::CAPACITY));
        // Nine code points at the tables' longest cannot overfill it: the
        // bound is `CLUSTER_KEY_MAX`'s product.
        SearchKey::of(cps).unwrap_or(SearchKey::EMPTY)
    }
}

/// Why a query is not a needle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NeedleRefused {
    /// Nothing to look for.
    Empty,
    /// Its key is longer than [`NEEDLE_MAX`].
    TooLong,
}

/// **A query, keyed and ready to match**: its key and the
/// Knuth-Morris-Pratt failure function over it.
#[derive(Clone, Debug)]
pub struct Needle {
    key: SearchKey<NEEDLE_MAX>,
    fail: [u16; NEEDLE_MAX],
}

impl Needle {
    /// The needle for `query`, keyed as the text is.
    pub fn new(query: &[Codepoint]) -> Result<Needle, NeedleRefused> {
        let key = SearchKey::of(query.iter().map(|c| c.value()))
            .map_err(|Full| NeedleRefused::TooLong)?;
        if key.len == 0 {
            return Err(NeedleRefused::Empty);
        }
        let k = key.as_slice();
        let mut fail = [0u16; NEEDLE_MAX];
        let mut q = 0usize;
        for i in 1..k.len() {
            while q > 0 && k[i] != k[q] {
                q = usize::from(fail[q - 1]);
            }
            if k[i] == k[q] {
                q += 1;
            }
            fail[i] = q as u16;
        }
        Ok(Needle { key, fail })
    }

    /// Its key: what the text's keys are compared with.
    pub fn key(&self) -> &[u32] {
        self.key.as_slice()
    }
}

/// Where a keyed code point began: the first of its cluster's key, or
/// after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Edge {
    Opens,
    Within,
}

#[derive(Clone, Copy, Debug)]
struct Fed<P> {
    at: P,
    edge: Edge,
}

/// **The matcher over a stream of clusters**, each at a caller's position
/// `P`: it reports every match whose first cluster and last cluster it
/// was fed at, leftmost first and none overlapping another, and none
/// across a [`Matcher::line_end`].
pub struct Matcher<'n, P> {
    needle: &'n Needle,
    state: usize,
    fed: u64,
    ring: [Fed<P>; NEEDLE_MAX],
    free_from: u64,
}

impl<'n, P: Copy> Matcher<'n, P> {
    /// A matcher for `needle`; `fill` is any position, held only until the
    /// ring is fed over it.
    pub fn new(needle: &'n Needle, fill: P) -> Matcher<'n, P> {
        Matcher {
            needle,
            state: 0,
            fed: 0,
            ring: [Fed {
                at: fill,
                edge: Edge::Within,
            }; NEEDLE_MAX],
            free_from: 0,
        }
    }

    /// Feed one cluster's key, at `at`; `found` hears each match it ends,
    /// as the positions of its first and last clusters.
    pub fn cluster(&mut self, key: &[u32], at: P, found: &mut impl FnMut(P, P)) {
        let k = self.needle.key();
        for (i, &c) in key.iter().enumerate() {
            self.ring[(self.fed % NEEDLE_MAX as u64) as usize] = Fed {
                at,
                edge: match i {
                    0 => Edge::Opens,
                    _ => Edge::Within,
                },
            };
            self.fed += 1;
            while self.state > 0 && k[self.state] != c {
                self.state = usize::from(self.needle.fail[self.state - 1]);
            }
            if k[self.state] == c {
                self.state += 1;
            }
            if self.state < k.len() {
                continue;
            }
            let start = self.fed - k.len() as u64;
            let first = self.ring[(start % NEEDLE_MAX as u64) as usize];
            if i + 1 == key.len() && first.edge == Edge::Opens && start >= self.free_from {
                found(first.at, at);
                self.free_from = self.fed;
            }
            self.state = usize::from(self.needle.fail[self.state - 1]);
        }
    }

    /// A line ended: no match runs across it.
    pub fn line_end(&mut self) {
        self.state = 0;
    }
}
