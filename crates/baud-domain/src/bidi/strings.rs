//! **ECMA-48's directed and reversed strings**: SDS, `CSI Ps ]`, and SRS,
//! `CSI Ps [` (`[[source: ecma48]]` 8.3.114 and 8.3.137). Each "is used to
//! establish in the data component the beginning and the end of a string
//! of characters as well as the direction of the string"; SDS gives the
//! direction outright, SRS the one "opposite to that currently
//! established", and "Every beginning of such a string invokes the next
//! deeper level of nesting".
//!
//! **Each character carries the string it was written in**, as an
//! identifier in its rendition ([`StyleWord::string`]), and the core keeps
//! each string's enclosing string and kind in a table the identifier
//! indexes ([`StringEntry`]): a string moves with its cells, and the chain
//! of enclosing strings is read back from the table.
//!
//! **With BDSM reset a line holding strings is laid out by TR/53's rule**
//! ([`order_strings`]): each string presented in its own direction and
//! nothing implicit inside it, a string kept apart from its neighbour, as
//! TR/53's worked example 1 presents `k l _ SRS(1) m n o p _ SRS(1) q r s t
//! SRS(0) _ u v w SRS(0) _ x y z` as "kl_wvu_qrst_ponm_xyz"
//! (`[[source: ecma-tr53]]`). With BDSM set the renderer gives the
//! algorithm each string as an isolate of its direction (UAX #9 HL3).

use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// **Which string a character was written in**: an index into the core's
/// table of strings, `0` for none. One byte, beside the font's in the
/// rendition word, so the word stays twenty bytes.
///
/// An SDS/SRS string identifier is NONE or a table slot 1..=255 named through
/// from_index; the byte is private so a slot number cannot be written directly.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::StringId(7);
/// ```
///
/// The legal path:
///
/// ```
/// let _ = baud_domain::StringId::from_index(7);
/// ```
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct StringId(u8);

impl StringId {
    /// No string: the top level of the line.
    pub const NONE: StringId = StringId(0);
    /// How many strings the table holds at once: every identifier but
    /// [`StringId::NONE`].
    pub const CAPACITY: usize = 255;

    /// The identifier at table slot `i`, `1..=255`.
    pub fn from_index(i: usize) -> Option<StringId> {
        match u8::try_from(i) {
            Ok(0) | Err(_) => None,
            Ok(n) => Some(StringId(n)),
        }
    }
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
    pub fn is_none(self) -> bool {
        self == StringId::NONE
    }
}

/// **What began a string**: SDS 1, SDS 2 or SRS 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StringKind {
    /// SDS 1, "establish the direction left-to-right".
    LeftToRight,
    /// SDS 2, "establish the direction right-to-left".
    RightToLeft,
    /// SRS 1, "reverse the direction": the one opposite to the string
    /// around it, or at a line's top level to the line's character
    /// path.
    Reversed,
}

impl StringKind {
    /// The direction a string of this kind runs, inside one running
    /// `enclosing`.
    pub fn runs(self, enclosing: BidiDirection) -> BidiDirection {
        match self {
            StringKind::LeftToRight => BidiDirection::Ltr,
            StringKind::RightToLeft => BidiDirection::Rtl,
            StringKind::Reversed => match enclosing {
                BidiDirection::Ltr => BidiDirection::Rtl,
                BidiDirection::Rtl => BidiDirection::Ltr,
            },
        }
    }

    /// Whether `edge` ends a string of this kind: SDS 0 a directed string
    /// and SRS 0 a reversed one, and no other.
    pub fn ended_by(self, edge: StringEdge) -> bool {
        matches!(
            (self, edge),
            (
                StringKind::LeftToRight | StringKind::RightToLeft,
                StringEdge::EndDirected
            ) | (StringKind::Reversed, StringEdge::EndReversed)
        )
    }
}

/// **One SDS or SRS**: a string's beginning, or the end of a directed or a
/// reversed one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StringEdge {
    Begin(StringKind),
    /// SDS 0, "end of a directed string; re-establish the previous
    /// direction".
    EndDirected,
    /// SRS 0, "end of a reversed string; re-establish the previous
    /// direction".
    EndReversed,
}

impl StringEdge {
    /// SDS's `Ps` (default 0): 0, 1 or 2, any other `None`.
    pub fn directed(p: Option<Param>) -> Option<StringEdge> {
        Some(match p.map_or(0, |x| x.0) {
            0 => StringEdge::EndDirected,
            1 => StringEdge::Begin(StringKind::LeftToRight),
            2 => StringEdge::Begin(StringKind::RightToLeft),
            _ => return None,
        })
    }
    /// SRS's `Ps` (default 0): 0 or 1, any other `None`.
    pub fn reversed(p: Option<Param>) -> Option<StringEdge> {
        Some(match p.map_or(0, |x| x.0) {
            0 => StringEdge::EndReversed,
            1 => StringEdge::Begin(StringKind::Reversed),
            _ => return None,
        })
    }
}

/// **A string in the core's table**: the string it is nested in and what
/// began it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StringEntry {
    pub parent: StringId,
    pub kind: StringKind,
}

/// The deepest a string nests: UAX #9's `max_depth`, past which the
/// algorithm a string is given to under BDSM set reads no deeper isolate.
pub const STRING_DEPTH: usize = 125;

/// **`id`'s chain**, outermost first, into `out`; its length. A string
/// whose entry is gone ends the chain there.
pub fn string_chain(
    id: StringId,
    entry: &impl Fn(StringId) -> Option<StringEntry>,
    out: &mut [StringId; STRING_DEPTH],
) -> usize {
    let mut n = 0;
    let mut s = id;
    while !s.is_none() && n < STRING_DEPTH {
        let Some(e) = entry(s) else { break };
        out[n] = s;
        n += 1;
        s = e.parent;
    }
    out[..n].reverse();
    n
}

/// **The direction the innermost string of `chain` runs** on a line whose
/// top level runs `base`, each reversed string reversing the one around it.
pub fn chain_runs(
    chain: &[StringId],
    entry: &impl Fn(StringId) -> Option<StringEntry>,
    base: BidiDirection,
) -> BidiDirection {
    chain.iter().fold(base, |d, &s| match entry(s) {
        Some(e) => e.kind.runs(d),
        None => d,
    })
}

/// **One row laid out by TR/53's rule**, BDSM reset: `from[v]` the logical
/// column drawn at visual column `v`, and `runs[c]` the direction logical
/// column `c` is drawn in, for the first `width` columns of a row whose
/// cells carry `ids` and whose top level runs `base`; the columns past
/// `width` follow in order. `tail(c)` says whether column `c` is a wide
/// character's right half, which stays to its lead's right.
///
/// **Each string is reversed when it runs the other way from the string
/// around it**, deepest first, and the row when it runs right to left: a
/// string's columns stay together through the reversals inside it, so
/// each is presented in its own direction and kept apart from the strings
/// beside it, the order UAX #9's levels cannot give two adjacent strings
/// of one level.
pub fn order_strings(
    ids: &[StringId],
    tail: impl Fn(usize) -> bool,
    width: usize,
    base: BidiDirection,
    entry: &impl Fn(StringId) -> Option<StringEntry>,
    from: &mut [u16],
    runs: &mut [BidiDirection],
) {
    let cols = from.len().min(runs.len());
    let width = width.min(cols).min(ids.len());
    for (v, slot) in from.iter_mut().enumerate().take(cols) {
        *slot = u16::try_from(v).unwrap_or(u16::MAX);
    }
    let mut chain = [StringId::NONE; STRING_DEPTH];
    let mut deepest = 0;
    for (c, r) in runs.iter_mut().enumerate().take(cols) {
        *r = base;
        if c < width {
            let n = string_chain(ids[c], entry, &mut chain);
            *r = chain_runs(&chain[..n], entry, base);
            deepest = deepest.max(n);
        }
    }
    // The string at depth `d` (1 the outermost) column `c` is in, if any.
    let at = |c: usize, d: usize| {
        let mut ch = [StringId::NONE; STRING_DEPTH];
        let n = string_chain(ids[c], entry, &mut ch);
        match d <= n {
            true => Some((ch[d - 1], ch, n)),
            false => None,
        }
    };
    for d in (1..=deepest).rev() {
        let mut i = 0;
        while i < width {
            let Some((s, ch, _)) = at(i, d) else {
                i += 1;
                continue;
            };
            let mut j = i + 1;
            while j < width && at(j, d).map(|x| x.0) == Some(s) {
                j += 1;
            }
            let inner = chain_runs(&ch[..d], entry, base);
            let outer = chain_runs(&ch[..d - 1], entry, base);
            if inner != outer {
                from[i..j].reverse();
            }
            i = j;
        }
    }
    if base == BidiDirection::Rtl {
        from[..width].reverse();
    }
    // A wide character's halves, reversed an odd number of times, back in
    // order: the lead, then its tail.
    let mut v = 0;
    while v + 1 < width {
        let l = usize::from(from[v]);
        if tail(l) && usize::from(from[v + 1]) + 1 == l {
            from.swap(v, v + 1);
            v += 2;
        } else {
            v += 1;
        }
    }
}
