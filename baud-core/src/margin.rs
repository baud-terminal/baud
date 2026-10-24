//! **The bells' volumes and the margin bell.** DECSWBV sets the warning
//! bell's volume and DECSMBV the margin bell's (`[[source: vt520-rm]]`
//! pp. 5-154 and 5-138); `?44` is xterm's switch for the margin bell
//! (`[[source: ctlseqs]]`). The core keeps the three and counts the margin
//! bell's rings in [`Term::margin_bells`](crate::Term#structfield.margin_bells);
//! the embedder makes the sound, as it does for `BEL`.
//!
//! **The margin bell is DEC STD 070's** (070-6 § 6.6.5.2, pp. 6-45 and
//! 6-46, `[[source: dec-std-070]]`; Jev, STD070, 0.94 and 0.95). "The
//! Margin Bell is set to RING ENABLED" when it is on and "A 'normal
//! keystroke' is typed" -- one whose first character is not `ESC`, `CSI` or
//! `DCS` ([`Term::key`](crate::Term::key)). It "is rung" when "Any character
//! is printed to the screen which causes the cursor to move from a column
//! <= (RIGHT MARGIN - 8) to any column on the same line > (RIGHT MARGIN -
//! 8)", eight columns as the VT100 and VT102 guides give them (Jev,
//! DEC_EIGHT, 0.97 and 0.97). It is set to RING DISABLED when it is turned
//! off, "The Margin Bell has been rung", or "The cursor moves to a new
//! line"; horizontal positioning neither rings it nor arms it.
//!
//! **"A new line" is tested at the write, against the line the keystroke
//! armed on** ([`Line`]): the screen's epoch, the ring's rotation (which
//! every whole-screen scroll moves, on either screen) and the cursor's row.
//! A different line disarms it unrung (Jev, AT_RING, 0.94 and 0.94). **A
//! scroll that moves the armed line's text disarms it, and one that does
//! not keeps it armed**: rows moved in place -- a region scrolled by copy,
//! `IL`, `DL` -- disarm it when the armed row is among them
//! ([`BellState::rows_moved`]), and a region at the top, which rotates the
//! ring and puts the rows under it back, re-reads the rotation of a line
//! armed under it (`Paint::margin_bell_under`). **A cursor that leaves the
//! line and comes back before anything is printed stays armed**, where STD
//! 070 disarms it: an open departure, since telling it needs a test on
//! every row change.
//!
//! The test runs behind the write path's one hook compare
//! (`Paint::overwrote`), on while a bell is armed, so a terminal with no
//! margin bell armed writes as it did. A write that prints no character --
//! a double line's cleared half, a rectangle -- takes the hook with the
//! bell set aside (`Paint::overwrote_unprinted`), so it neither rings nor
//! disarms it.

use baud_domain::{BellVolume, BellsRung, Col, MarginBell, Row, RowRot};

use crate::term::ScreenEpoch;

/// What the bells are set to, and where the margin bell is armed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct BellState {
    /// DECSWBV's volume: "High (default)".
    pub(crate) warning: BellVolume,
    /// DECSMBV's volume: "Off (default)".
    pub(crate) margin: BellVolume,
    /// `?44`, off at power-on.
    pub(crate) switch: MarginBell,
    /// The line a normal keystroke armed the margin bell on, or none.
    pub(crate) armed: Option<Line>,
    /// The `BEL`s rung, at the warning volume each rang at: credited by
    /// `Paint::fold_bells` before the volume changes and at a read's end.
    pub(crate) warning_rung: BellsRung,
    /// The margin bell's rings, at the volume each rang at.
    pub(crate) margin_rung: BellsRung,
    /// How many of the read's `BEL`s are credited already, 0 between reads.
    pub(crate) folded: usize,
    /// **What a `SUB` draws** (`Term::set_substitute_mark`): the other C0
    /// control whose effect the embedder chooses, beside the bells. Here
    /// for the struct's tail padding, so `Cold` keeps its size, which has
    /// none. The embedder's, so no reset touches it.
    pub(crate) sub_mark: baud_domain::SubstituteMark,
}

/// The cursor's line as the margin bell tells one line from another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Line {
    pub(crate) epoch: ScreenEpoch,
    pub(crate) rot: RowRot,
    pub(crate) row: Row,
}

/// Whether the margin bell can ring: `?44` on and its volume not off,
/// xterm's two switches (Jev's lean, TWO_SWITCHES, 0.37/0.27 then
/// 0.27/0.14).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Margin {
    Off,
    On,
}

/// Whether a write crossed the margin bell's column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Crossed {
    No,
    Rung,
}

impl BellState {
    /// Power-on: no bell rung yet.
    pub(crate) const POWER_ON: BellState = BellState {
        warning: BellVolume::WARNING_POWER_ON,
        margin: BellVolume::MARGIN_POWER_ON,
        switch: MarginBell::Off,
        armed: None,
        warning_rung: BellsRung::NONE,
        margin_rung: BellsRung::NONE,
        folded: 0,
        sub_mark: baud_domain::SubstituteMark::Off,
    };

    /// What `RIS` restores, the counts kept: the volumes are set-up
    /// features, and "RIS replaces all set-up features with their saved
    /// settings" (`[[source: vt510-rm-b]]` p. 5-184), the power-on ones
    /// here; `?44` is a mode, and RIS sets "the modes into the reset state"
    /// (ECMA-48 8.3.105, `[[source: ecma48]]`). `DECSTR`'s Table 5-9 names
    /// neither volume, so it keeps them.
    pub(crate) fn reset(&mut self) {
        self.warning = BellVolume::WARNING_POWER_ON;
        self.margin = BellVolume::MARGIN_POWER_ON;
        self.switch = MarginBell::Off;
        self.armed = None;
    }

    pub(crate) fn margin(self) -> Margin {
        match (self.switch, self.margin) {
            (MarginBell::On, BellVolume::Low | BellVolume::High) => Margin::On,
            _ => Margin::Off,
        }
    }

    /// `?44` set or reset. Off disarms: "RING DISABLED when ... Margin Bell
    /// is disabled".
    pub(crate) fn set_switch(&mut self, s: MarginBell) {
        self.switch = s;
        self.disarm_if_off();
    }

    /// DECSMBV. Off disarms, as `?44` reset does.
    pub(crate) fn set_margin(&mut self, v: BellVolume) {
        self.margin = v;
        self.disarm_if_off();
    }

    fn disarm_if_off(&mut self) {
        if self.margin() == Margin::Off {
            self.armed = None;
        }
    }

    /// A normal keystroke typed with the cursor on `at`.
    pub(crate) fn keystroke(&mut self, at: Line) {
        if self.margin() == Margin::On {
            self.armed = Some(at);
        }
    }

    /// Rows `from` to `to` moved in place: a region's scroll that copies
    /// its rows, `IL` or `DL`. The armed line among them is gone from its
    /// row, so the bell is disarmed; a line above or below them stays
    /// where it is, and stays armed.
    pub(crate) fn rows_moved(&mut self, from: Row, to: Row) {
        if self.armed.is_some_and(|a| from <= a.row && a.row <= to) {
            self.armed = None;
        }
    }

    /// A write on the cursor's line `at` from column `start` to `end`, the
    /// line's last column `fence`: rung if it crossed RIGHT MARGIN - 8 on
    /// the armed line, and disarmed if it rang or the line is another.
    pub(crate) fn wrote(&mut self, at: Line, start: Col, end: Col, fence: Col) -> Crossed {
        let Some(armed) = self.armed else {
            return Crossed::No;
        };
        if armed != at {
            self.armed = None;
            return Crossed::No;
        }
        // 1-based, `column <= RIGHT MARGIN - 8` is 0-based `start <= fence
        // - 8`, and `> RIGHT MARGIN - 8` is `end > fence - 8`: on 80
        // columns, the character written in column 72.
        let Some(edge) = fence.index().checked_sub(8) else {
            return Crossed::No;
        };
        match start.index() <= edge && end.index() > edge {
            true => {
                self.armed = None;
                Crossed::Rung
            }
            false => Crossed::No,
        }
    }
}

/// Whether a key's bytes are a "normal keystroke": "any keystroke which
/// does not generate one of the following characters as the first
/// character: ESC, CSI, DCS" (070-6 p. 6-45).
pub(crate) fn is_normal_keystroke(bytes: &[u8]) -> Normal {
    match bytes.first() {
        Some(0x1b | 0x9b | 0x90) | None => Normal::No,
        Some(_) => Normal::Yes,
    }
}

/// [`is_normal_keystroke`]'s answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Normal {
    No,
    Yes,
}
