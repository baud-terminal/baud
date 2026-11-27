//! **Readings a terminal gives a reader to compare**: each counts one kind
//! of event, and two equal readings saw none of it across the read between
//! them.

/// Which surface is up, and which time it went up: it moves on every switch
/// between the primary and the alternate screen, in either direction. Two
/// equal readings saw one surface throughout -- where `Term::screen` read
/// at both ends of a read cannot tell one that left the alternate screen and
/// entered it again, which clears it and resets its ring, from one that
/// never left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenEpoch(u32);

impl ScreenEpoch {
    pub const FIRST: ScreenEpoch = ScreenEpoch(0);

    pub fn next(self) -> ScreenEpoch {
        ScreenEpoch(self.0.wrapping_add(1))
    }
}

/// Which erasure of the primary's history the terminal is at: it moves on
/// every `CSI 3 J` and every `RIS`, the two operations that empty the saved
/// lines, and on nothing else -- not `CSI ? 3 J`, whose lines stay
/// ([`HistoryWipes`] counts it). A window
/// over the history read at both ends of a read cannot see one: the erase
/// scrolls nothing, so `Term::history_moved_since` reads no movement, and
/// output after it in the same read refills the history to a count that
/// says nothing of what went. An embedder holding a window into the history
/// asks this, and finds the lines it showed are gone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HistoryErasures(u32);

impl HistoryErasures {
    pub const FIRST: HistoryErasures = HistoryErasures(0);

    pub fn next(self) -> HistoryErasures {
        HistoryErasures(self.0.wrapping_add(1))
    }
}

/// Which selective erase of the primary's saved lines the terminal is at:
/// it moves on every `CSI ? 3 J`, from either screen, and on nothing else.
/// The lines stay lines of the history, so [`HistoryErasures`] does not
/// move and a window over them stays; but the characters DECSCA left
/// erasable on them are spaces now, and an embedder that keeps something
/// read from the history -- a find's matches -- asks this, and reads the
/// history again where it moved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HistoryWipes(u32);

impl HistoryWipes {
    pub const FIRST: HistoryWipes = HistoryWipes(0);

    pub fn next(self) -> HistoryWipes {
        HistoryWipes(self.0.wrapping_add(1))
    }
}

/// Which full reset the terminal is at: it moves on every `RIS` (`ESC c`),
/// from either screen and with or without a history, and on nothing else --
/// not `DECSTR`, `ED 2` or `CSI 3 J`. ECMA-48 § 8.3.105 has `RIS` put the
/// device "to its initial state, i.e. the state it has after it is made
/// operational" (`[[source: ecma48]]`), and an embedder's own state over the
/// screen -- a selection, a guess drawn ahead of an echo -- names cells that
/// state does not have. Nothing else across a read says one happened: a
/// reset scrolls nothing and keeps the rotation, so a terminal with no
/// history reads the same at both ends, and [`HistoryErasures`] moves on
/// `CSI 3 J` too, which leaves the screen as it was. Two equal readings saw
/// no reset; a count rather than a flag, because one read can hold two and a
/// flag would need clearing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resets(u32);

impl Resets {
    pub const FIRST: Resets = Resets(0);

    pub fn next(self) -> Resets {
        Resets(self.0.wrapping_add(1))
    }
}
