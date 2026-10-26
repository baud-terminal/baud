//! **SDS and SRS, `CSI Ps ]` and `CSI Ps [`** (`[[source: ecma48]]`
//! 8.3.114, 8.3.137): a directed or reversed string's beginning and end.
//!
//! **The string being written is in the rendition being built**
//! ([`baud_domain::StyleWord::string`]), so every character written takes
//! it, and a string's entry in [`Strings`] names the string around it: the
//! rendition's identifier is the whole stack of open strings, an end
//! puts back its enclosing one, and the status line, whose rendition is
//! parked apart, keeps strings of its own. `SGR 0` leaves the string, as
//! it leaves SAPV's variants, being no `SGR`; `DECSC` and `DECRC`,
//! `XTPUSHSGR` and `XTPOPSGR` keep the string open now; `DECSTR` and `RIS`
//! end every string (Jev's lean, CLOSE, 0.26/0.46 then 0.82/0.86), the
//! characters written keeping theirs.
//!
//! **An end ends only a string of its kind**: SDS 0 a directed string and
//! SRS 0 a reversed one (Jev's lean, MATCH_KIND); a mismatched end, and an
//! end with no string open, end nothing and are counted
//! ([`Term::strings_refused`]). A string past [`STRING_DEPTH`] deep, or
//! one the table has no room for once its unnamed strings are freed, is
//! refused and counted, its characters written in the string around it,
//! and its end, matched to it, ends nothing more.
//!
//! **What it costs a program that never sends one: nothing on the text
//! path.** The string rides in the rendition, which the write path does
//! not read; erasing reads strings only once one has been begun since
//! power-on ([`Strings::begun`]).

use baud_domain::{
    Col, PackedCell, Row, STRING_DEPTH, StringEdge, StringEntry, StringId, StringKind, StyleId,
    StyleWord,
};

use crate::paint::Paint;
use crate::term::Term;

/// A count of SDS and SRS refused, as a reading to compare: an end with
/// nothing of its kind to end, a string too deep, or one with no room.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StringsRefused(u32);

impl StringsRefused {
    const FIRST: StringsRefused = StringsRefused(0);

    fn next(self) -> StringsRefused {
        StringsRefused(self.0.wrapping_add(1))
    }
}

/// Whether a string has been begun since power-on or `RIS`: until one has,
/// no cell names a string, and an erase reads none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Begun {
    No,
    Yes,
}

/// Whether a sweep found a string named.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Named {
    No,
    Yes,
}

/// **The strings refused while open**, innermost last, so each end is
/// matched to the refused string it ends: one bit a string, set for a
/// reversed one, and how many.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Refused {
    kinds: u128,
    open: OpenRefused,
}

/// How many refused strings are open, innermost last.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct OpenRefused(u16);

impl Refused {
    const NONE: Refused = Refused {
        kinds: 0,
        open: OpenRefused(0),
    };

    fn any(&self) -> bool {
        self.open.0 > 0
    }

    fn push(&mut self, k: StringKind) {
        if self.open.0 < 128 {
            let bit = 1u128 << self.open.0;
            match k {
                StringKind::Reversed => self.kinds |= bit,
                _ => self.kinds &= !bit,
            }
        }
        self.open.0 = self.open.0.saturating_add(1);
    }

    fn pop(&mut self) {
        self.open.0 = self.open.0.saturating_sub(1);
    }

    /// The innermost refused string's kind, as one that `edge` ends or not.
    fn ends(&self, edge: StringEdge) -> bool {
        let at = self.open.0.saturating_sub(1);
        let reversed = at < 128 && self.kinds & (1u128 << at) != 0;
        let k = match reversed {
            true => StringKind::Reversed,
            false => StringKind::LeftToRight,
        };
        k.ended_by(edge)
    }
}

/// Whether a string can nest one deeper.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Room {
    Deeper,
    Full,
}

/// **The strings begun and not yet freed**, by identifier.
#[derive(Clone, Copy)]
pub(crate) struct Strings {
    table: [Option<StringEntry>; StringId::CAPACITY + 1],
    refused_open: Refused,
    pub(crate) refused: StringsRefused,
    pub(crate) begun: Begun,
}

impl Strings {
    pub(crate) const POWER_ON: Strings = Strings {
        table: [None; StringId::CAPACITY + 1],
        refused_open: Refused::NONE,
        refused: StringsRefused::FIRST,
        begun: Begun::No,
    };

    pub(crate) fn entry(&self, id: StringId) -> Option<StringEntry> {
        self.table.get(id.index()).copied().flatten()
    }

    /// Whether a string inside `id` stays within [`STRING_DEPTH`].
    fn room(&self, id: StringId) -> Room {
        let mut n = 0;
        let mut s = id;
        while let Some(e) = self.entry(s) {
            n += 1;
            s = e.parent;
        }
        match n < STRING_DEPTH {
            true => Room::Deeper,
            false => Room::Full,
        }
    }

    fn free_slot(&self) -> Option<StringId> {
        (1..=StringId::CAPACITY)
            .find(|&i| self.table[i].is_none())
            .and_then(StringId::from_index)
    }
}

impl Paint<'_, '_> {
    /// **SDS or SRS** ([`StringEdge`]).
    #[cold]
    #[inline(never)]
    pub(crate) fn string_edge(&mut self, edge: StringEdge) {
        self.form_ends();
        let now = self.style_raw.string();
        match edge {
            StringEdge::Begin(kind) => {
                let s = &self.strs.strings;
                let id = match (s.refused_open.any(), s.room(now)) {
                    (false, Room::Deeper) => self.new_string(),
                    _ => None,
                };
                let Some(id) = id else {
                    self.strs.strings.refused_open.push(kind);
                    self.strs.strings.refused = self.strs.strings.refused.next();
                    return;
                };
                self.strs.strings.table[id.index()] = Some(StringEntry { parent: now, kind });
                self.strs.strings.begun = Begun::Yes;
                self.refresh_presentation_edits();
                self.write_string(id);
            }
            end => {
                let s = &mut self.strs.strings;
                if s.refused_open.any() {
                    match s.refused_open.ends(end) {
                        true => s.refused_open.pop(),
                        false => s.refused = s.refused.next(),
                    }
                    return;
                }
                match s.entry(now) {
                    Some(e) if e.kind.ended_by(end) => self.write_string(e.parent),
                    _ => s.refused = s.refused.next(),
                }
            }
        }
    }

    fn write_string(&mut self, id: StringId) {
        self.style_raw = self.style_raw.with_string(id);
        self.style = self.intern_style(self.style_raw);
    }

    /// **A free identifier**, freeing first, when none is, every string no
    /// cell names, on either screen, in the history or on the status line,
    /// and no rendition being built names, nor encloses one that is.
    fn new_string(&mut self) -> Option<StringId> {
        if let Some(id) = self.strs.strings.free_slot() {
            return Some(id);
        }
        let mut live = [Named::No; StringId::CAPACITY + 1];
        let strings = self.strs.strings;
        let mut mark = |mut s: StringId| {
            while let Some(e) = strings.entry(s) {
                if live[s.index()] == Named::Yes {
                    break;
                }
                live[s.index()] = Named::Yes;
                s = e.parent;
            }
        };
        let tmpl = &*self.tmpl;
        let of = |id: StyleId| {
            tmpl.get(id.index())
                .copied()
                .flatten()
                .map_or(StringId::NONE, |w| w.string())
        };
        let mut last = StyleId::DEFAULT;
        for c in self
            .view
            .ring_cells()
            .iter()
            .chain(self.other.cells.iter())
            .chain(self.status.cells.iter())
        {
            if c.style != last {
                last = c.style;
                mark(of(c.style));
            }
        }
        mark(self.style_raw.string());
        mark(self.strs.status.parked_string());
        for (i, l) in live.iter().enumerate().skip(1) {
            if *l == Named::No {
                self.strs.strings.table[i] = None;
            }
        }
        self.strs.strings.free_slot()
    }

    /// The string the rendition `id` names.
    pub(crate) fn string_of(&self, id: StyleId) -> StringId {
        self.tmpl
            .get(id.index())
            .copied()
            .flatten()
            .map_or(StringId::NONE, |w| w.string())
    }

    /// **Each blank of `[from, to)` on row `r` put back in its string**,
    /// `kept(c)` naming it: the blank's rendition with the string, interned.
    pub(crate) fn strings_kept(
        &mut self,
        r: Row,
        (from, to): (Col, Col),
        blank: PackedCell,
        kept: impl Fn(Col) -> StringId,
    ) {
        let word = self
            .tmpl
            .get(blank.style.index())
            .copied()
            .flatten()
            .unwrap_or(StyleWord::DEFAULT);
        for c in from.index()..to.index() {
            let at = Col::at_index(c);
            let s = kept(at);
            if s.is_none() || self.view.get(r, at) != blank {
                continue;
            }
            let style = self.intern_style(word.with_string(s));
            self.view.put(r, at, PackedCell::empty_with(style));
        }
    }

    /// `DECSTR` and `RIS`: every string ends, the characters written
    /// keeping theirs.
    pub(crate) fn end_strings(&mut self) {
        self.strs.strings.refused_open = Refused::NONE;
        if !self.style_raw.string().is_none() {
            self.write_string(StringId::NONE);
        }
    }

    /// `RIS`: the table empty, as no cell is left naming a string.
    pub(crate) fn reset_strings(&mut self) {
        self.end_strings();
        self.strs.strings = Strings::POWER_ON;
        self.refresh_presentation_edits();
    }

    /// **`raw` put back as the rendition being built, the string open now
    /// kept**: `DECRC`, `XTPOPSGR`.
    pub(crate) fn keeping_string(&self, raw: StyleWord) -> StyleWord {
        raw.with_string(self.style_raw.string())
    }
}

impl Term<'_> {
    /// **SDS and SRS refused** ([`StringsRefused`]): an end with no string
    /// of its kind open, a string past UAX #9's depth, or one the table
    /// had no room for.
    pub fn strings_refused(&self) -> StringsRefused {
        self.store.strs.strings.refused
    }

    /// **The string `id` names**: the string around it and what began it,
    /// for a layout to read the chain from; `None` for no string.
    pub fn string_entry(&self, id: StringId) -> Option<StringEntry> {
        self.store.strs.strings.entry(id)
    }

    /// **Whether any cell can name a string**: one has been begun since
    /// power-on or `RIS`. While none has, a layout reads no cell's string.
    pub fn strings_seen(&self) -> bool {
        self.store.strs.strings.begun == Begun::Yes
    }
}
