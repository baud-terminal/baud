//! Which script a run of text is in, and the OpenType script tags that
//! selects.
//!
//! **The property is Unicode's, resolved the way UAX #24 says.** Each code
//! point has one Script value and a set of Script_Extensions
//! (`[[source: unicode-scripts]]`, `[[source: unicode-scriptextensions]]`),
//! generated into `script_data.rs`. Two values are not scripts at all:
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
//! **What is not built: paired punctuation.** 5.1 goes on: "paired
//! punctuation, such as brackets or quotation marks, belongs to the
//! enclosing or outer level of the text and should therefore match the
//! script of the enclosing text". `resolve` takes the preceding script
//! for a bracket as for anything else, so in "... gamma (γ) is ..." the
//! closing parenthesis is Greek where UAX #24 would make it Latin. It
//! changes which `LangSys` shapes one punctuation glyph. The text leans
//! one way, so it is not a question to decide but conformance work still
//! to build, and it needs `BidiBrackets.txt` pinned for the pairs.
//!
//! **The OpenType side** is `[[source: ms-opentype-scripttags]]`: each
//! script's registered tags, the newer layout model's first where the
//! registry gives two (`dev2` before `deva`, `mym2` before `mymr`). Only
//! Myanmar's order is the registry's ("should use 'mym2' instead"); it
//! lists the Indic pairs in no order, so theirs is an open question
//! (docs/record.md, "OpenType script selection, phase 0").
//! [`crate::layout`] then looks those up and falls back to `DFLT`.

use crate::layout::ScriptTag;
use crate::script_data::{
    CODES, DIRECTIONS, EXTENSION_BLOCKS, EXTENSION_INDEX, EXTENSION_SETS, EXTENSION_SLOT,
    RUN_BLOCKS, RUN_INDEX, SCRIPT_BLOCKS, SCRIPT_COUNT, SCRIPT_INDEX, TAGS,
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

/// The order a run's glyphs are laid out in. baud does not reorder text
/// (UAX #9 is unbuilt; note AB), but a `morx` subtable names the order it
/// walks a run in by the run's layout, so the shaper still needs it.
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

/// One cell before its context is known: the script its cluster names, or
/// the scripts a Common cluster may join.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unresolved {
    /// The cluster names this script.
    Named(Script),
    /// A cluster with no script of its own. `joins` is its first code
    /// point's Script_Extensions; empty joins anything.
    Neutral { joins: &'static [u8] },
}

impl Unresolved {
    /// What a cluster contributes before its context is known.
    pub fn of(cps: &[Codepoint]) -> Unresolved {
        match Script::of_cluster(cps) {
            s if s.is_specific() => Unresolved::Named(s),
            _ => Unresolved::Neutral {
                joins: cps.first().map_or(&[], |&c| extensions(c)),
            },
        }
    }

    fn admits(self, s: Script) -> bool {
        match self {
            Unresolved::Named(n) => n == s,
            Unresolved::Neutral { joins } => joins.is_empty() || joins.contains(&s.0),
        }
    }
}

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
pub fn resolve(cells: &[Unresolved], out: &mut [Script]) {
    const PENDING: Script = Script(u8::MAX);
    let mut before: Option<Script> = None;
    for (cell, slot) in cells.iter().zip(out.iter_mut()) {
        *slot = match *cell {
            Unresolved::Named(s) => {
                before = Some(s);
                s
            }
            Unresolved::Neutral { .. } => match before {
                Some(b) if cell.admits(b) => b,
                _ => PENDING,
            },
        };
    }
    let mut after: Option<Script> = None;
    for (cell, slot) in cells.iter().zip(out.iter_mut()).rev() {
        if *slot == PENDING {
            *slot = match (after, *cell) {
                (Some(a), _) if cell.admits(a) => a,
                (_, Unresolved::Neutral { joins }) => {
                    joins.first().map_or(Script::COMMON, |&i| Script(i))
                }
                (_, Unresolved::Named(s)) => s,
            };
        }
        after = Some(*slot);
    }
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
