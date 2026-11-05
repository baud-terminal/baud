//! Which script a run of text is in, and the OpenType script tags that
//! selects.
//!
//! **The property is Unicode's, resolved the way UAX #24 says.** Each code
//! point has one Script value and a set of Script_Extensions
//! (`[[source: unicode-scripts]]`, `[[source: unicode-scriptextensions]]`),
//! compiled by `build/script.rs`. Two values are not scripts at all:
//! Common, "for other characters that may be used with multiple scripts",
//! and Inherited, a mark that takes its base's (`[[source: uax24]]` 2.1).
//! UAX #24 says how a renderer settles them, and both rules are here:
//!
//! * **A cluster is in the script of its first character that is neither
//!   Common nor Inherited** (5.2): "treating all the characters of a
//!   combining character sequence (or extended grapheme cluster) as having
//!   the Script property value of the first non-Inherited, non-Common
//!   character in the sequence if there is one, and otherwise treating all
//!   the characters as having the Common Script property value". A cell is
//!   a cluster, so a Hebrew point is in its letter's run, and a dotted
//!   circle carrying a Hebrew point is Hebrew. [`Script::of_cluster`].
//! * **A Common cluster takes its context's script** (5.1): "programs must
//!   resolve any of the special Script property values, such as Common,
//!   based on the context of the surrounding characters. A simple heuristic
//!   uses the script of the preceding character". Script_Extensions narrows
//!   it (5.3, Example 2): U+30FC "should not continue a Latin script run,
//!   but instead should only continue runs of certain scripts".
//!   [`resolve`].
//!
//! * **A bracket pair takes the script of the text enclosing it** (5.1):
//!   "paired punctuation, such as brackets or quotation marks, belongs to
//!   the enclosing or outer level of the text and should therefore match
//!   the script of the enclosing text. In addition, opening and closing
//!   elements of a pair resolve to the same Script property values, where
//!   possible". The preceding-script rule alone makes the closer in
//!   "... gamma (γ) is ..." Greek, the text's own example. Which characters
//!   pair is `[[source: unicode-bidibrackets]]`, and a pair is matched as
//!   UAX #9 matches one (`[[source: uax9-bidi]]`, BD16): a 63-deep stack,
//!   a closer popping through the opener it matches, and no pair at all on
//!   a line that overflows it. Quotation marks are not paired: "it is not
//!   possible to tell from the character code alone whether a particular
//!   quotation mark is used as an opening or closing punctuation", and the
//!   file lists none. [`resolve`].
//!
//! **The OpenType side** is `[[source: ms-opentype-scripttags]]`: each
//! script's registered tags, the newer layout model's first where the
//! registry gives two (`dev2` before `deva`, `mym2` before `mymr`). Only
//! Myanmar's order is the registry's ("should use 'mym2' instead"); it
//! lists the Indic pairs in no order, so theirs is an open question.
//! [`crate::layout`] then looks those up and falls back to `DFLT`.

use crate::layout::ScriptTag;
use crate::script_data::{
    BRACKET_BLOCKS, BRACKET_INDEX, CODES, DIRECTIONS, EXTENSION_BLOCKS, EXTENSION_INDEX,
    EXTENSION_SETS, EXTENSION_SLOT, RUN_BLOCKS, RUN_INDEX, SCRIPT_BLOCKS, SCRIPT_COUNT,
    SCRIPT_INDEX, TAGS,
};
use baud_domain::Codepoint;

/// A Unicode Script property value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Script(u8);

impl Script {
    /// `Zzzz`: an unassigned code point.
    pub const UNKNOWN: Script = Script(0);
    /// `Zyyy`: used with many scripts; settled from context.
    pub const COMMON: Script = Script(1);
    /// `Zinh`: a mark that takes its base's script.
    pub const INHERITED: Script = Script(2);
    /// How many values there are.
    pub const COUNT: usize = SCRIPT_COUNT;

    /// The Script property of `c`.
    pub fn of(c: Codepoint) -> Script {
        let v = usize::try_from(c.value()).unwrap_or(0);
        let block = usize::from(SCRIPT_INDEX.get(v >> 8).copied().unwrap_or(0));
        Script(
            SCRIPT_BLOCKS
                .get(block * 256 + (v & 0xFF))
                .copied()
                .unwrap_or(0),
        )
    }

    /// The script a whole cluster is in (UAX #24 5.2): its first code point
    /// that names a script, else Common.
    pub fn of_cluster(cps: &[Codepoint]) -> Script {
        cps.iter()
            .map(|&c| Script::of(c))
            .find(|s| s.is_specific())
            .unwrap_or(Script::COMMON)
    }

    /// Every value, in index order.
    pub fn all() -> impl Iterator<Item = Script> {
        (0..=u8::MAX).take(SCRIPT_COUNT).map(Script)
    }

    /// The ISO 15924 code: `Hebr`, `Latn`, `Zyyy`.
    pub fn code(self) -> [u8; 4] {
        let at = usize::from(self.0) * 4;
        match CODES.get(at..at + 4) {
            Some(&[a, b, c, d]) => [a, b, c, d],
            _ => *b"Zzzz",
        }
    }

    /// Whether this value names a script rather than Unknown, Common or
    /// Inherited, which take their script from context.
    pub fn is_specific(self) -> bool {
        self.0 > Script::INHERITED.0
    }

    /// The OpenType script tags this script selects, the one to try first
    /// first; none for Unknown, Common and Inherited, whose text selects
    /// `DFLT`.
    pub fn tags(self) -> impl Iterator<Item = ScriptTag> {
        let at = usize::from(self.0) * 8;
        let pair = match TAGS.get(at..at + 8) {
            Some(&[a, b, c, d, e, f, g, h]) => [[a, b, c, d], [e, f, g, h]],
            _ => [[0; 4]; 2],
        };
        pair.into_iter().filter(|t| *t != [0; 4]).map(ScriptTag)
    }

    /// The direction a run of this script is laid out in: right to left
    /// where every letter of the script is Bidi_Class R or AL, which UAX #9
    /// resolves to an odd level whatever the paragraph's direction
    /// (`[[source: uax9-bidi]]`, W3 and I1-I2), and left to right
    /// otherwise -- Common and Inherited among them.
    pub fn direction(self) -> Direction {
        match DIRECTIONS.get(usize::from(self.0)) {
            Some(1) => Direction::RightToLeft,
            _ => Direction::LeftToRight,
        }
    }
}

/// The order a run's glyphs are laid out in. The shaper is given a run's
/// cells in the order they are drawn: a line is reordered before it is
/// shaped (`docs/bidi.md`), and a right-to-left run's cells are shaped one
/// at a time. A `morx` subtable still names the order it walks a run in by
/// the run's layout, so the shaper needs it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

/// Whether a run has held a character of one kind yet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    No,
    Yes,
}

/// What a run's characters say about its direction beyond its script.
/// A run of a right-to-left script that holds a decimal digit or a
/// regional indicator and no letter is laid out left to right -- UAX #9
/// resolves EN and AN to an even level and a regional indicator is
/// Bidi_Class L. A decimal digit of Bidi_Class R -- NKo's and Adlam's, 20
/// in all -- is counted too, as HarfBuzz counts gc Nd; UAX #9 would level
/// it odd. CoreText puts a flag pair beside Hebrew in a
/// left-to-right run; HarfBuzz keeps such a run left to right only when
/// its caller asks for left to right (`hb_ensure_native_direction`,
/// `[[source: harfbuzz-hb-ot-shape]]`, hb-ot-shape.cc:613), and baud has
/// no caller direction to ask with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunText {
    letter: Held,
    number: Held,
}

impl RunText {
    /// A run that has held nothing yet.
    pub const EMPTY: RunText = RunText {
        letter: Held::No,
        number: Held::No,
    };

    /// Notes one character of the run.
    pub fn push(&mut self, c: Codepoint) {
        let v = usize::try_from(c.value()).unwrap_or(0);
        let block = usize::from(RUN_INDEX.get(v >> 8).copied().unwrap_or(0));
        match RUN_BLOCKS.get(block * 256 + (v & 0xFF)) {
            Some(1) => self.letter = Held::Yes,
            Some(2) => self.number = Held::Yes,
            _ => {}
        }
    }

    /// The direction a run of `script` holding these characters is laid
    /// out in.
    pub fn direction(self, script: Script) -> Direction {
        match script.direction() {
            Direction::RightToLeft if self.letter == Held::No && self.number == Held::Yes => {
                Direction::LeftToRight
            }
            d => d,
        }
    }
}

/// The scripts a code point is used with beyond its own Script value
/// (`[[source: unicode-scriptextensions]]`): empty where the file lists
/// nothing, which means the Script value alone.
fn extensions(c: Codepoint) -> &'static [u8] {
    let v = usize::try_from(c.value()).unwrap_or(0);
    let block = usize::from(EXTENSION_INDEX.get(v >> 8).copied().unwrap_or(0));
    let set = EXTENSION_BLOCKS
        .get(block * 256 + (v & 0xFF))
        .copied()
        .unwrap_or(0);
    let at = usize::from(set) * EXTENSION_SLOT;
    let slot = EXTENSION_SETS.get(at..at + EXTENSION_SLOT).unwrap_or(&[]);
    let n = slot.iter().position(|&m| m == 0xFF).unwrap_or(slot.len());
    slot.get(..n).unwrap_or(&[])
}

/// Which bracket pair a code point belongs to: a number shared by the two
/// characters BidiBrackets.txt pairs, and by their canonical equivalents.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Pair(u8);

/// What a code point is to bracket pairing (`[[source: uax9-bidi]]`,
/// BD14-BD15): an opening or a closing paired bracket, by its
/// Bidi_Paired_Bracket_Type (`[[source: unicode-bidibrackets]]`), or
/// neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bracket {
    None,
    Open(Pair),
    Close(Pair),
}

impl Bracket {
    /// The paired-bracket type of `c`, and its pair.
    pub fn of(c: Codepoint) -> Bracket {
        let v = usize::try_from(c.value()).unwrap_or(0);
        let block = usize::from(BRACKET_INDEX.get(v >> 8).copied().unwrap_or(0));
        match BRACKET_BLOCKS
            .get(block * 256 + (v & 0xFF))
            .copied()
            .unwrap_or(0)
        {
            0 => Bracket::None,
            b if b & 1 == 0 => Bracket::Open(Pair(b >> 1)),
            b => Bracket::Close(Pair(b >> 1)),
        }
    }
}

/// One cell before its context is known: the script its cluster names, or
/// the scripts a Common cluster may join.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unresolved {
    /// The cluster names this script.
    Named(Script),
    /// A cluster with no script of its own. `joins` is its first code
    /// point's Script_Extensions; empty joins anything. `bracket` is its
    /// first code point's paired-bracket type.
    Neutral {
        joins: &'static [u8],
        bracket: Bracket,
    },
}

impl Unresolved {
    /// A cell with nothing drawn, which joins whatever is around it.
    pub const BLANK: Unresolved = Unresolved::Neutral {
        joins: &[],
        bracket: Bracket::None,
    };

    /// What a cluster contributes before its context is known.
    pub fn of(cps: &[Codepoint]) -> Unresolved {
        match Script::of_cluster(cps) {
            s if s.is_specific() => Unresolved::Named(s),
            _ => Unresolved::Neutral {
                joins: cps.first().map_or(&[], |&c| extensions(c)),
                bracket: cps.first().map_or(Bracket::None, |&c| Bracket::of(c)),
            },
        }
    }

    fn admits(self, s: Script) -> bool {
        match self {
            Unresolved::Named(n) => n == s,
            Unresolved::Neutral { joins, .. } => joins.is_empty() || joins.contains(&s.0),
        }
    }
}

/// How deep BD16's stack is: "Create a fixed-size stack for exactly 63
/// elements" (`[[source: uax9-bidi]]`).
const BRACKET_DEPTH: usize = 63;

/// A cell's position in the line being resolved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct At(usize);

/// How many openers [`Brackets`] holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Depth(usize);

/// The line held more openers at once than BD16's stack.
struct Overflow;

/// BD16's stack: the openers not yet closed, innermost last.
struct Brackets {
    open: [(Pair, At); BRACKET_DEPTH],
    depth: Depth,
}

impl Brackets {
    fn new() -> Brackets {
        Brackets {
            open: [(Pair(0), At(0)); BRACKET_DEPTH],
            depth: Depth(0),
        }
    }

    /// "If an opening paired bracket is found and there is room in the
    /// stack, push" it; with no room, BD16 stops and pairs nothing.
    fn push(&mut self, pair: Pair, at: At) -> Result<(), Overflow> {
        let slot = self.open.get_mut(self.depth.0).ok_or(Overflow)?;
        *slot = (pair, at);
        self.depth = Depth(self.depth.0 + 1);
        Ok(())
    }

    /// The opener a closer of `pair` closes, the innermost of its pair,
    /// popped "through the current stack element inclusively"; none, and
    /// nothing popped, where no opener of its pair is open.
    fn close(&mut self, pair: Pair) -> Option<At> {
        let open = self.open.get(..self.depth.0)?;
        let k = open.iter().rposition(|&(p, _)| p == pair)?;
        let at = open.get(k)?.1;
        self.depth = Depth(k);
        Some(at)
    }
}

/// Whether bracket pairs are matched on a pass: BD16 matches none on a
/// line that overflows its stack.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pairing {
    On,
    Off,
}

/// Whether a forward pass left a closer whose opener was still unsettled
/// -- every cell from the opener to the closer unsettled, so the opener's
/// script comes from what follows the closer -- for the pass after the
/// backward one to give the closer, and the text after it, that script.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Deferred {
    No,
    Yes,
}

/// What `resolve`'s forward pass leaves for the backward one.
const PENDING: Script = Script(u8::MAX);

/// Settles every cell's script in context into `out`, which must be as
/// long as `cells` (a shorter `out` settles only its length).
///
/// A named cell keeps its script. A neutral one takes the script before it
/// where its extensions admit that, else the script after it where they
/// admit that, else its first extension, else Common -- UAX #24's
/// "script of the preceding character" with Script_Extensions as the
/// limit, and the following script for what leads a line, which the
/// OpenType text permits: "Applications may process script-neutral
/// characters together with immediately-preceding or following
/// script-specific characters" (`[[source: ms-opentype-chapter2]]`,
/// ScriptList table, note). A line of neutrals alone stays Common, and
/// selects `DFLT`: "or if the text does not have a specific script (for
/// example, it contains only symbols or punctuation)".
///
/// A closing bracket that BD16 pairs with an opener takes the opener's
/// script where its extensions admit that, and the text after it then
/// continues the opener's script: the pair "belongs to the enclosing or
/// outer level of the text" (`[[source: uax24]]` 5.1). An opener that
/// leads the line has no enclosing script before it and takes the one
/// after it, and its closer takes the same.
pub fn resolve(cells: &[Unresolved], out: &mut [Script]) {
    let deferred = match forward(cells, out, Pairing::On) {
        Ok(d) => d,
        Err(Overflow) => forward(cells, out, Pairing::Off).unwrap_or(Deferred::No),
    };
    let mut after: Option<Script> = None;
    for (cell, slot) in cells.iter().zip(out.iter_mut()).rev() {
        if *slot == PENDING {
            *slot = following(*cell, after);
        }
        after = Some(*slot);
    }
    if deferred == Deferred::Yes {
        // A closer whose opener was unsettled until the backward pass takes
        // the opener's script now, and the cells after it continue that
        // script as the forward pass continues a closer's. The pairs are
        // the forward pass's: the same cells under the same stack, which
        // did not overflow.
        let mut open = Brackets::new();
        let mut before: Option<Script> = None;
        for (i, cell) in cells.iter().enumerate() {
            let Unresolved::Neutral { bracket, .. } = *cell else {
                before = None;
                continue;
            };
            let paired = match bracket {
                Bracket::Open(p) => {
                    if open.push(p, At(i)).is_err() {
                        return;
                    }
                    None
                }
                Bracket::Close(p) => open.close(p).and_then(|At(k)| out.get(k).copied()),
                Bracket::None => None,
            };
            let Some(slot) = out.get_mut(i) else {
                return;
            };
            match (paired, before) {
                (Some(o), _) if cell.admits(o) => {
                    *slot = o;
                    before = Some(o);
                }
                (_, Some(b)) if cell.admits(b) => *slot = b,
                _ => {}
            }
        }
    }
}

/// The script a cell the forward pass left unsettled takes from the one
/// `after` it: that script where its extensions admit it, else its first
/// extension, else Common.
fn following(cell: Unresolved, after: Option<Script>) -> Script {
    match (after, cell) {
        (Some(a), _) if cell.admits(a) => a,
        (_, Unresolved::Neutral { joins, .. }) => {
            joins.first().map_or(Script::COMMON, |&i| Script(i))
        }
        (_, Unresolved::Named(s)) => s,
    }
}

/// `resolve`'s forward pass: every named cell its script, every neutral
/// one the script before it where it admits that, else [`PENDING`], and
/// with [`Pairing::On`] every paired closer its opener's script. A stretch
/// left pending is settled, as the backward pass settles it, by the first
/// cell after it that is settled, so an opener that leads the line holds
/// its script by the time its closer is reached. It fails where the line
/// overflows BD16's stack, and says whether a closer was left for an
/// opener still pending.
fn forward(
    cells: &[Unresolved],
    out: &mut [Script],
    pairing: Pairing,
) -> Result<Deferred, Overflow> {
    let mut before: Option<Script> = None;
    let mut open = Brackets::new();
    let mut deferred = Deferred::No;
    let mut pending: Option<At> = None;
    for (i, cell) in cells.iter().enumerate() {
        let pair = match (*cell, pairing) {
            (Unresolved::Neutral { bracket, .. }, Pairing::On) => match bracket {
                Bracket::Open(p) => {
                    open.push(p, At(i))?;
                    None
                }
                Bracket::Close(p) => open.close(p),
                Bracket::None => None,
            },
            _ => None,
        };
        let settled = match (*cell, pair) {
            (Unresolved::Named(s), _) => {
                before = Some(s);
                s
            }
            (_, Some(At(k))) => match out.get(k).copied() {
                Some(PENDING) => {
                    deferred = Deferred::Yes;
                    PENDING
                }
                Some(o) if cell.admits(o) => {
                    before = Some(o);
                    o
                }
                _ => match before {
                    Some(b) if cell.admits(b) => b,
                    _ => PENDING,
                },
            },
            (_, None) => match before {
                Some(b) if cell.admits(b) => b,
                _ => PENDING,
            },
        };
        match out.get_mut(i) {
            Some(slot) => *slot = settled,
            None => break,
        }
        if settled == PENDING {
            pending = pending.or(Some(At(i)));
        } else if let Some(At(p)) = pending.take() {
            let mut after = settled;
            let stretch = cells.get(p..i).zip(out.get_mut(p..i));
            for (cell, slot) in stretch.into_iter().flat_map(|(c, o)| c.iter().zip(o)).rev() {
                *slot = following(*cell, Some(after));
                after = *slot;
            }
        }
    }
    Ok(deferred)
}

/// One value per script, indexed by [`Script`]: what a renderer keeps per
/// face, such as which lookups each script selects there.
#[derive(Clone, Debug)]
pub struct PerScript<T>([T; SCRIPT_COUNT]);

impl<T> PerScript<T> {
    /// The table holding `f(s)` for every script `s`.
    pub fn from_fn(mut f: impl FnMut(Script) -> T) -> PerScript<T> {
        PerScript(core::array::from_fn(|i| {
            f(Script(u8::try_from(i).unwrap_or(0)))
        }))
    }

    /// Whether every script holds the same value.
    pub fn uniform(&self) -> bool
    where
        T: PartialEq,
    {
        self.0.iter().all(|v| Some(v) == self.0.first())
    }
}

impl<T> core::ops::Index<Script> for PerScript<T> {
    type Output = T;
    fn index(&self, s: Script) -> &T {
        // A `Script` is below `SCRIPT_COUNT` by construction.
        &self.0[usize::from(s.0)]
    }
}
