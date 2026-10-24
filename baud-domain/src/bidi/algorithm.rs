//! **The Unicode Bidirectional Algorithm**, UAX #9 revision 51 for Unicode
//! 17.0.0 (`[[source: uax9-bidi]]`), from the paragraph level to the order
//! of a line: P1-P3, X1-X10, W1-W7, N0-N2, I1-I2, then L1-L2 for each line.
//! Every rule is the annex's text read in its order, and
//! `every_bidi_test_case_resolves_and_reorders_as_it_says` and
//! `every_bidi_character_test_case_resolves_and_reorders_as_it_says` hold it
//! to every case of `BidiTest.txt` and `BidiCharacterTest.txt`.
//!
//! **No allocation and no unbounded stack.** [`Bidi`] is the caller's,
//! sized by a capacity `N` the caller picks (a row's width, for a terminal),
//! and holds every per-character array the rules need; the two stacks the
//! annex sizes are fixed, the directional status stack at max_depth + 2 =
//! 127 entries (X1) and BD16's bracket stack at 63. A text longer than `N`
//! is refused with [`BidiTooLong`], not cut. A call touches only the first
//! `len` entries of each array, so a large capacity costs nothing per call.
//!
//! The characters X9 removes (the embeddings, overrides, PDF and BN) are
//! skipped by position rather than deleted, and are given the level of the
//! character before them, or the paragraph's. X9 allows any placement:
//! "Conformance does not require any particular placement of these
//! characters", and the conformance files skip them. Section 5.2's
//! retaining guideline carries them through the rules at the stack's
//! level, ignoring BN in I1-I2, and only at L1 resolves each "to the level
//! of the preceding character if there is one, and otherwise to the base
//! level", the level they are given here. L1 also resets the ones that
//! trail a line or precede a separator, as 5.2 asks.

use super::{BidiClass, BidiClass::*, BidiProps, BracketKind, paired_bracket};
use core::ops::Range;

/// An embedding level: 0 to max_depth + 1, odd for right to left.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct BidiLevel(u8);

/// A direction: the parity of a level, or a paragraph's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BidiDirection {
    Ltr,
    Rtl,
}

impl BidiLevel {
    /// Level 0, a left-to-right paragraph's.
    pub const LTR: BidiLevel = BidiLevel(0);
    /// Level 1, a right-to-left paragraph's.
    pub const RTL: BidiLevel = BidiLevel(1);
    /// max_depth: the deepest level an explicit embedding reaches (BD2).
    pub const MAX_DEPTH: u8 = 125;

    /// The level as a number.
    pub fn number(self) -> u8 {
        self.0
    }

    /// Left to right for an even level, right to left for an odd one (BD3).
    pub fn direction(self) -> BidiDirection {
        match self.0 & 1 {
            0 => BidiDirection::Ltr,
            _ => BidiDirection::Rtl,
        }
    }

    /// The least level above this one in `dir`, if it is a valid explicit
    /// level (X2-X5c).
    fn next(self, dir: BidiDirection) -> Option<BidiLevel> {
        let up = match (self.direction(), dir) {
            (BidiDirection::Ltr, BidiDirection::Rtl) | (BidiDirection::Rtl, BidiDirection::Ltr) => {
                1
            }
            _ => 2,
        };
        let l = self.0 + up;
        (l <= Self::MAX_DEPTH).then_some(BidiLevel(l))
    }

    /// The level `by` above this one (I1-I2).
    fn raised(self, by: u8) -> BidiLevel {
        BidiLevel(self.0 + by)
    }
}

impl BidiDirection {
    fn class(self) -> BidiClass {
        match self {
            BidiDirection::Ltr => L,
            BidiDirection::Rtl => R,
        }
    }
}

/// The paragraph level: set by a higher-level protocol (HL1), or found from
/// the text by P2 and P3.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BidiParagraph {
    /// Level 0, whatever the text.
    Ltr,
    /// Level 1, whatever the text.
    Rtl,
    /// P2-P3: level 1 if the first strong character, isolates skipped, is
    /// R or AL, else 0.
    FirstStrong,
}

/// A text longer than the [`Bidi`] holding it, or a line longer than the
/// order it is to fill, which is refused whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BidiTooLong {
    pub len: usize,
    pub capacity: usize,
}

/// A directional status stack entry's override status (X1).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Override {
    Neutral,
    Ltr,
    Rtl,
}

impl Override {
    /// X5a, X5b, X6, X6a: a character's type under the status.
    fn apply(self, c: BidiClass) -> BidiClass {
        match self {
            Override::Neutral => c,
            Override::Ltr => L,
            Override::Rtl => R,
        }
    }
}

/// A directional status stack entry's isolate status (X1).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Isolate {
    Yes,
    No,
}

#[derive(Clone, Copy)]
struct Status {
    level: BidiLevel,
    over: Override,
    isolate: Isolate,
}

/// No position: an isolate initiator with no matching PDI, or a PDI that
/// matches none.
const NONE: u16 = u16::MAX;
/// max_depth + 2 (X1).
const STATUS_DEPTH: usize = 127;
/// BD16's "fixed-size stack for exactly 63 elements".
const BRACKET_DEPTH: usize = 63;

/// Removed by X9, and kept in place by section 5.2.
fn removed(c: BidiClass) -> bool {
    matches!(c, RLE | LRE | RLO | LRO | PDF | BN)
}

/// An isolate initiator (BD8).
fn initiator(c: BidiClass) -> bool {
    matches!(c, LRI | RLI | FSI)
}

/// A neutral or isolate formatting character (NI) once W1-W7 are done.
fn neutral(c: BidiClass) -> bool {
    matches!(c, B | S | WS | ON | LRI | RLI | FSI | PDI)
}

/// The direction N0 and N1 read from a resolved type: EN and AN count as
/// R; `None` for a neutral.
fn strong(c: BidiClass) -> Option<BidiDirection> {
    match c {
        L => Some(BidiDirection::Ltr),
        R | AL | EN | AN => Some(BidiDirection::Rtl),
        _ => None,
    }
}

/// Reset by L1 when they precede a separator or end the line: whitespace
/// and the isolate formatting characters, with the characters X9 removed,
/// which 5.2 puts in the same sequences.
fn trailing(c: BidiClass) -> bool {
    matches!(c, WS | FSI | LRI | RLI | PDI) || removed(c)
}

fn pos(i: usize) -> u16 {
    u16::try_from(i).unwrap_or(NONE)
}

/// The algorithm's working state for one text of at most `N` characters.
/// It is the caller's and is reused: [`Bidi::resolve`] resolves a text's
/// levels, then [`Bidi::reorder`] orders each line of it.
pub struct Bidi<const N: usize> {
    len: usize,
    /// Each character's Bidi_Class, as the text gives it.
    original: [BidiClass; N],
    /// Each character's type as the rules change it.
    class: [BidiClass; N],
    /// 0, or a paired bracket's pair number times two, plus one for a
    /// closer.
    bracket: [u8; N],
    level: [BidiLevel; N],
    /// The level of the paragraph each character is in.
    paragraph: [BidiLevel; N],
    /// The levels L1 gives the line last reordered.
    line: [BidiLevel; N],
    /// An isolate initiator's matching PDI, a matched PDI's initiator.
    link: [u16; N],
    /// The positions X9 keeps, in order, and each kept position's index
    /// among them.
    kept: [u16; N],
    at: [u16; N],
    /// The isolating run sequence being resolved, as positions.
    run: [u16; N],
    /// BD16's bracket pairs, each the opener's index in `run` above the
    /// closer's.
    pairs: [u32; N],
}

impl<const N: usize> Default for Bidi<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Bidi<N> {
    /// Every position fits a `u16` with [`NONE`] to spare.
    const FITS: () = assert!(N < 0xFFFF, "a Bidi holds fewer than 65535 characters");

    /// The capacity: the longest text it resolves.
    pub const CAPACITY: usize = N;

    pub const fn new() -> Self {
        let () = Self::FITS;
        Bidi {
            len: 0,
            original: [L; N],
            class: [L; N],
            bracket: [0; N],
            level: [BidiLevel::LTR; N],
            paragraph: [BidiLevel::LTR; N],
            line: [BidiLevel::LTR; N],
            link: [NONE; N],
            kept: [0; N],
            at: [0; N],
            run: [0; N],
            pairs: [0; N],
        }
    }

    /// **Resolves the levels of `text`** (P1-I2), its paragraphs split at
    /// each paragraph separator (P1), and answers the first paragraph's
    /// level.
    pub fn resolve(&mut self, text: &[char], dir: BidiParagraph) -> Result<BidiLevel, BidiTooLong> {
        self.fit(text.len())?;
        for (i, &c) in text.iter().enumerate() {
            let props = BidiProps::of(c);
            self.original[i] = props.class;
            self.bracket[i] = match props.bracket.and_then(|_| paired_bracket(c)) {
                Some(b) => {
                    b.pair.number() * 2
                        + match b.kind {
                            BracketKind::Open => 0,
                            BracketKind::Close => 1,
                        }
                }
                None => 0,
            };
        }
        Ok(self.paragraphs(dir))
    }

    /// [`Bidi::resolve`] over a text given as Bidi_Class values, with no
    /// paired brackets: `BidiTest.txt`'s form, and a caller's that has the
    /// classes already.
    pub fn resolve_classes(
        &mut self,
        classes: &[BidiClass],
        dir: BidiParagraph,
    ) -> Result<BidiLevel, BidiTooLong> {
        self.fit(classes.len())?;
        self.original[..classes.len()].copy_from_slice(classes);
        self.bracket[..classes.len()].fill(0);
        Ok(self.paragraphs(dir))
    }

    /// Each character's resolved level, after I1-I2 and before L1.
    pub fn levels(&self) -> &[BidiLevel] {
        &self.level[..self.len]
    }

    /// The level of the paragraph holding character `i`.
    pub fn paragraph_level(&self, i: usize) -> Option<BidiLevel> {
        self.paragraph[..self.len].get(i).copied()
    }

    /// **Orders one line** of the text last resolved (L1-L2): `order[v]` is
    /// the position, in the text, of the character displayed `v`-th from
    /// the left. The line is `line`, clipped to the text, and `order` takes
    /// one entry a character of it. An `order` too short for the line is
    /// refused rather than the line cut, since L1 would read the cut as the
    /// line's end. Answers the line's levels after L1.
    pub fn reorder(
        &mut self,
        line: Range<usize>,
        order: &mut [u16],
    ) -> Result<&[BidiLevel], BidiTooLong> {
        let end = line.end.min(self.len);
        let start = line.start.min(end);
        let n = end - start;
        if order.len() < n {
            return Err(BidiTooLong {
                len: n,
                capacity: order.len(),
            });
        }
        // L1, on the original types.
        self.line[start..end].copy_from_slice(&self.level[start..end]);
        self.reset_before(start, end);
        for i in start..end {
            if matches!(self.original[i], S | B) {
                self.line[i] = self.paragraph[i];
                self.reset_before(start, i);
            }
        }
        // L2: from the highest level to the lowest odd one, reverse every
        // run at that level or above.
        for (k, o) in order[..n].iter_mut().enumerate() {
            *o = pos(start + k);
        }
        let levels = &self.line[start..end];
        let high = levels.iter().max().map_or(0, |l| l.0);
        let low = levels.iter().min().map_or(0, |l| l.0) | 1;
        let mut at = high;
        while at >= low {
            let mut k = 0;
            while k < n {
                if self.line[usize::from(order[k])].0 < at {
                    k += 1;
                    continue;
                }
                let from = k;
                while k < n && self.line[usize::from(order[k])].0 >= at {
                    k += 1;
                }
                order[from..k].reverse();
            }
            at -= 1;
        }
        Ok(&self.line[start..end])
    }

    /// L1: the whitespace and isolate formatting characters ending at `to`,
    /// back to `from`, take the paragraph level.
    fn reset_before(&mut self, from: usize, to: usize) {
        let mut j = to;
        while j > from && trailing(self.original[j - 1]) {
            j -= 1;
            self.line[j] = self.paragraph[j];
        }
    }

    fn fit(&mut self, len: usize) -> Result<(), BidiTooLong> {
        if len > N {
            return Err(BidiTooLong { len, capacity: N });
        }
        self.len = len;
        Ok(())
    }

    /// P1: each paragraph ends after its paragraph separator.
    fn paragraphs(&mut self, dir: BidiParagraph) -> BidiLevel {
        let mut first = match dir {
            BidiParagraph::Rtl => BidiLevel::RTL,
            _ => BidiLevel::LTR,
        };
        let mut start = 0;
        while start < self.len {
            let mut end = start;
            while end < self.len && self.original[end] != B {
                end += 1;
            }
            let end = (end + 1).min(self.len);
            let level = self.paragraph(start, end, dir);
            if start == 0 {
                first = level;
            }
            start = end;
        }
        first
    }

    /// P2-P3 over `from..to`: the first L, AL or R, skipping each isolate
    /// initiator's text to its matching PDI, or to the end where it has
    /// none.
    fn first_strong(&self, from: usize, to: usize) -> BidiLevel {
        let mut i = from;
        while i < to {
            match self.original[i] {
                L => return BidiLevel::LTR,
                R | AL => return BidiLevel::RTL,
                LRI | RLI | FSI => match self.link[i] {
                    NONE => return BidiLevel::LTR,
                    j => i = usize::from(j),
                },
                _ => {}
            }
            i += 1;
        }
        BidiLevel::LTR
    }

    /// Resolves one paragraph, `start..end`, and answers its level.
    fn paragraph(&mut self, start: usize, end: usize, dir: BidiParagraph) -> BidiLevel {
        // BD9: each isolate initiator's matching PDI, with `run` as the
        // stack of initiators still open.
        let mut open = 0;
        for i in start..end {
            self.link[i] = NONE;
            match self.original[i] {
                LRI | RLI | FSI => {
                    self.run[open] = pos(i);
                    open += 1;
                }
                PDI if open > 0 => {
                    open -= 1;
                    let j = usize::from(self.run[open]);
                    self.link[j] = pos(i);
                    self.link[i] = pos(j);
                }
                _ => {}
            }
        }
        let base = match dir {
            BidiParagraph::Ltr => BidiLevel::LTR,
            BidiParagraph::Rtl => BidiLevel::RTL,
            BidiParagraph::FirstStrong => self.first_strong(start, end),
        };
        self.explicit(start, end, base);
        // X9: the positions kept, and their index among them.
        let mut kept = 0;
        for i in start..end {
            if !removed(self.original[i]) {
                self.kept[kept] = pos(i);
                self.at[i] = pos(kept);
                kept += 1;
            }
        }
        // X10 and BD13: an isolating run sequence starts at each level run
        // whose first character is not a matched PDI, and goes on at the
        // matching PDI's run while the last run ends with an initiator that
        // has one.
        let mut k = 0;
        while k < kept {
            let from = k;
            let level = self.level[usize::from(self.kept[k])];
            k = self.run_end(k, kept, level);
            let first = usize::from(self.kept[from]);
            if self.original[first] == PDI && self.link[first] != NONE {
                continue;
            }
            let (mut a, mut b, mut m) = (from, k, 0);
            loop {
                for j in a..b {
                    self.run[m] = self.kept[j];
                    m += 1;
                }
                let last = usize::from(self.kept[b - 1]);
                if !initiator(self.original[last]) || self.link[last] == NONE {
                    break;
                }
                a = usize::from(self.at[usize::from(self.link[last])]);
                b = self.run_end(a, kept, level);
            }
            self.sequence(m, kept, base);
        }
        // I1 and I2, once every sequence is resolved: X10's sos and eos and
        // BD13's level runs read the explicit levels, "before any changes
        // are made to them in the steps below".
        for k in 0..kept {
            let i = usize::from(self.kept[k]);
            let l = self.level[i];
            self.level[i] = match (l.direction(), self.class[i]) {
                (BidiDirection::Ltr, R) => l.raised(1),
                (BidiDirection::Ltr, AN | EN) => l.raised(2),
                (BidiDirection::Rtl, L | EN | AN) => l.raised(1),
                _ => l,
            };
        }
        // 5.2: a removed character takes the level before it.
        for i in start..end {
            if removed(self.original[i]) {
                self.level[i] = if i > start { self.level[i - 1] } else { base };
            }
            self.paragraph[i] = base;
        }
        base
    }

    /// The end of the level run of kept characters at `level` from `k`.
    fn run_end(&self, mut k: usize, kept: usize, level: BidiLevel) -> usize {
        k += 1;
        while k < kept && self.level[usize::from(self.kept[k])] == level {
            k += 1;
        }
        k
    }

    /// X1-X8: the explicit levels and directions.
    fn explicit(&mut self, start: usize, end: usize, base: BidiLevel) {
        let mut stack = [Status {
            level: base,
            over: Override::Neutral,
            isolate: Isolate::No,
        }; STATUS_DEPTH];
        let mut depth = 1;
        let (mut overflow_isolates, mut overflow_embeddings, mut valid_isolates) = (0, 0, 0);
        for i in start..end {
            let c = self.original[i];
            self.class[i] = c;
            let top = stack[depth - 1];
            match c {
                // X2-X5.
                RLE | LRE | RLO | LRO => {
                    self.level[i] = top.level;
                    let dir = match c {
                        RLE | RLO => BidiDirection::Rtl,
                        _ => BidiDirection::Ltr,
                    };
                    match top.level.next(dir) {
                        Some(level) if overflow_isolates == 0 && overflow_embeddings == 0 => {
                            stack[depth] = Status {
                                level,
                                over: match c {
                                    RLO => Override::Rtl,
                                    LRO => Override::Ltr,
                                    _ => Override::Neutral,
                                },
                                isolate: Isolate::No,
                            };
                            depth += 1;
                        }
                        _ if overflow_isolates == 0 => overflow_embeddings += 1,
                        _ => {}
                    }
                }
                // X5a-X5c.
                RLI | LRI | FSI => {
                    self.level[i] = top.level;
                    self.class[i] = top.over.apply(c);
                    let dir = match c {
                        RLI => BidiDirection::Rtl,
                        LRI => BidiDirection::Ltr,
                        _ => {
                            let to = match self.link[i] {
                                NONE => end,
                                j => usize::from(j),
                            };
                            self.first_strong(i + 1, to).direction()
                        }
                    };
                    match top.level.next(dir) {
                        Some(level) if overflow_isolates == 0 && overflow_embeddings == 0 => {
                            valid_isolates += 1;
                            stack[depth] = Status {
                                level,
                                over: Override::Neutral,
                                isolate: Isolate::Yes,
                            };
                            depth += 1;
                        }
                        _ => overflow_isolates += 1,
                    }
                }
                // X6a.
                PDI => {
                    if overflow_isolates > 0 {
                        overflow_isolates -= 1;
                    } else if valid_isolates > 0 {
                        overflow_embeddings = 0;
                        while stack[depth - 1].isolate == Isolate::No {
                            depth -= 1;
                        }
                        depth -= 1;
                        valid_isolates -= 1;
                    }
                    let top = stack[depth - 1];
                    self.level[i] = top.level;
                    self.class[i] = top.over.apply(c);
                }
                // X7.
                PDF => {
                    if overflow_isolates > 0 {
                    } else if overflow_embeddings > 0 {
                        overflow_embeddings -= 1;
                    } else if top.isolate == Isolate::No && depth >= 2 {
                        depth -= 1;
                    }
                    self.level[i] = stack[depth - 1].level;
                }
                // X8.
                B => self.level[i] = base,
                // X6, whose exclusion of BN 5.2 lifts for its level.
                BN => self.level[i] = top.level,
                _ => {
                    self.level[i] = top.level;
                    self.class[i] = top.over.apply(c);
                }
            }
        }
    }

    /// W1-W7 and N0-N2 over the isolating run sequence in `run[..m]`, in
    /// a paragraph whose kept characters number `kept`.
    fn sequence(&mut self, m: usize, kept: usize, base: BidiLevel) {
        let first = usize::from(self.run[0]);
        let last = usize::from(self.run[m - 1]);
        let level = self.level[first];
        let embedding = level.direction().class();
        // X10: sos and eos, from the higher level on either side.
        let before = match usize::from(self.at[first]) {
            0 => base,
            k => self.level[usize::from(self.kept[k - 1])],
        };
        let after = match usize::from(self.at[last]) + 1 {
            _ if initiator(self.original[last]) => base,
            k if k < kept => self.level[usize::from(self.kept[k])],
            _ => base,
        };
        let sos = level.max(before).direction().class();
        let eos = level.max(after).direction().class();
        let run = &self.run[..m];
        let class = &mut self.class;
        let at = |k: usize| usize::from(run[k]);

        // W1.
        let mut prev = sos;
        for k in 0..m {
            if class[at(k)] == NSM {
                class[at(k)] = if matches!(prev, LRI | RLI | FSI | PDI) {
                    ON
                } else {
                    prev
                };
            }
            prev = class[at(k)];
        }
        // W2.
        let mut last_strong = sos;
        for k in 0..m {
            match class[at(k)] {
                c @ (L | R | AL) => last_strong = c,
                EN if last_strong == AL => class[at(k)] = AN,
                _ => {}
            }
        }
        // W3.
        for k in 0..m {
            if class[at(k)] == AL {
                class[at(k)] = R;
            }
        }
        // W4.
        for k in 1..m.saturating_sub(1) {
            let (p, n) = (class[at(k - 1)], class[at(k + 1)]);
            match class[at(k)] {
                ES if p == EN && n == EN => class[at(k)] = EN,
                CS if p == n && matches!(p, EN | AN) => class[at(k)] = p,
                _ => {}
            }
        }
        // W5.
        let mut k = 0;
        while k < m {
            if class[at(k)] != ET {
                k += 1;
                continue;
            }
            let from = k;
            while k < m && class[at(k)] == ET {
                k += 1;
            }
            if (from > 0 && class[at(from - 1)] == EN) || (k < m && class[at(k)] == EN) {
                for j in from..k {
                    class[at(j)] = EN;
                }
            }
        }
        // W6.
        for k in 0..m {
            if matches!(class[at(k)], ES | ET | CS) {
                class[at(k)] = ON;
            }
        }
        // W7.
        let mut last_strong = sos;
        for k in 0..m {
            match class[at(k)] {
                c @ (L | R) => last_strong = c,
                EN if last_strong == L => class[at(k)] = L,
                _ => {}
            }
        }

        // N0, with BD16's pairs.
        let (mut ids, mut opens) = ([0u8; BRACKET_DEPTH], [0u16; BRACKET_DEPTH]);
        let (mut depth, mut pairs) = (0, 0);
        for k in 0..m {
            let b = self.bracket[at(k)];
            if b == 0 || class[at(k)] != ON {
                continue;
            }
            let (id, closer) = (b >> 1, b & 1);
            if closer == 0 {
                if depth == BRACKET_DEPTH {
                    pairs = 0;
                    break;
                }
                ids[depth] = id;
                opens[depth] = pos(k);
                depth += 1;
            } else if let Some(d) = (0..depth).rev().find(|&d| ids[d] == id) {
                self.pairs[pairs] = (u32::from(opens[d]) << 16) | u32::from(pos(k));
                pairs += 1;
                depth = d;
            }
        }
        self.pairs[..pairs].sort_unstable();
        let e = strong(embedding);
        for p in 0..pairs {
            let open = usize::try_from(self.pairs[p] >> 16).unwrap_or(0);
            let close = usize::try_from(self.pairs[p] & 0xFFFF).unwrap_or(0);
            let (mut same, mut opposite) = (false, false);
            for k in open + 1..close {
                match strong(class[at(k)]) {
                    None => {}
                    s if s == e => same = true,
                    _ => opposite = true,
                }
            }
            let dir = if same {
                embedding
            } else if opposite {
                let context = (0..open)
                    .rev()
                    .find_map(|k| strong(class[at(k)]))
                    .unwrap_or(strong(sos).unwrap_or(BidiDirection::Ltr));
                if Some(context) != e {
                    context.class()
                } else {
                    embedding
                }
            } else {
                continue;
            };
            for b in [open, close] {
                class[at(b)] = dir;
                let mut k = b + 1;
                while k < m && self.original[at(k)] == NSM {
                    class[at(k)] = dir;
                    k += 1;
                }
            }
        }

        // N1 and N2.
        let mut k = 0;
        while k < m {
            if !neutral(class[at(k)]) {
                k += 1;
                continue;
            }
            let from = k;
            while k < m && neutral(class[at(k)]) {
                k += 1;
            }
            let before = if from == 0 {
                strong(sos)
            } else {
                strong(class[at(from - 1)])
            };
            let after = if k == m {
                strong(eos)
            } else {
                strong(class[at(k)])
            };
            let dir = match (before, after) {
                (Some(b), Some(a)) if a == b => b.class(),
                _ => embedding,
            };
            for j in from..k {
                class[at(j)] = dir;
            }
        }
    }
}
