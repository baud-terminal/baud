//! **When the pointer hides as the user types**: XTSMPOINTER's policy,
//! `CSI > Ps p` (`[[source: ctlseqs]]`), which the core keeps
//! (`Term::pointer_mode`) and each driver acts on -- macOS with
//! `NSCursor.setHiddenUntilMouseMoves(_:)` (`[[source:
//! apple-nscursor-sethiddenuntilmousemoves]]`), Linux with
//! `wl_pointer.set_cursor` and no surface, "If surface is NULL, the pointer
//! image is hidden" (`[[source: wayland]]`).
//!
//! When it hides and when it comes back are xterm's (`[[source: xterm-misc]]`
//! and `[[source: xterm-reset]]`'s file): a key hides it as
//! `xtermShowPointer(False)` does at the end of `Input`, unless the mode says
//! not to, and a key under a mode that no longer hides shows it; motion shows
//! it at every mode; crossing into or out of the window, or losing the
//! keyboard, shows it below mode 3 (`unselectwindow`, and `xevents`, which at
//! mode 3 shows it only on `MotionNotify`); a program turning mouse tracking
//! on or changing it shows it (`really_set_mousemode`, `CASE_DECELR`), as do
//! `RIS` (`ReallyReset`'s hard branch) and a new pointer shape (`OSC 22`,
//! xterm's "briefly override pointerMode after changing the pointer").
//!
//! **A button press shows it, at every mode**, as the resource XTSMPOINTER
//! sets is documented: "It will be redisplayed if the user moves the
//! mouse, or clicks one of its buttons" (`[[source: xterm-man]]`,
//! `pointerMode`). xterm's own `xevents` departs from that text: it passes
//! `ButtonPress` and `ButtonRelease` by, so a click without motion leaves
//! xterm's pointer hidden. The text is followed.
//!
//! **A key that sends bytes**, not every key: "hide the pointer cursor as
//! the user types" (`[[source: ctlseqs]]`). xterm hides at the end of every
//! `Input`, a modifier's press and a shortcut's included.

// Each driver reads its own half.
#![cfg_attr(not(test), allow(dead_code))]

use baud_core::{MouseTracking, PointerHides, PointerShape, Resets, Term};

/// What a driver does to the pointer for an event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Hide,
    Show,
    Nothing,
}

/// The pointer as the driver last left it, and what the core said when it
/// last looked: the tracking, the shape and the resets, so a change in any
/// is seen once.
#[derive(Clone, Copy, Debug)]
pub struct Hiding {
    hidden: PointerHides,
    tracking: MouseTracking,
    shape: Option<PointerShape>,
    resets: Resets,
}

impl Hiding {
    /// Shown, and the core as it is now.
    pub fn new(term: &Term<'_>) -> Hiding {
        Hiding {
            hidden: PointerHides::No,
            tracking: term.mouse_tracking(),
            shape: term.pointer_shape(),
            resets: term.resets(),
        }
    }

    /// Whether the pointer is hidden.
    pub fn hidden(&self) -> PointerHides {
        self.hidden
    }

    /// **A key sent bytes**: hidden if the mode hides it against the
    /// tracking in force, shown again if it was hidden and no longer hides.
    pub fn key(&mut self, term: &Term<'_>) -> Act {
        self.to(term.hides_pointer_on_key())
    }

    /// **After a read**: shown if the program turned tracking on or changed
    /// it, set a new pointer shape, or reset the terminal.
    pub fn read(&mut self, term: &Term<'_>) -> Act {
        let tracking = term.mouse_tracking();
        let shape = term.pointer_shape();
        let resets = term.resets();
        let tracks = tracking != self.tracking && tracking != MouseTracking::Off;
        let changed = tracks || shape != self.shape || resets != self.resets;
        self.tracking = tracking;
        self.shape = shape;
        self.resets = resets;
        match changed {
            true => self.to(PointerHides::No),
            false => Act::Nothing,
        }
    }

    /// **The pointer moved**, over the window: shown, at every mode.
    pub fn moved(&mut self) -> Act {
        self.to(PointerHides::No)
    }

    /// **A button was pressed** over the window: shown, at every mode --
    /// "or clicks one of its buttons" (`[[source: xterm-man]]`).
    pub fn pressed(&mut self) -> Act {
        self.to(PointerHides::No)
    }

    /// **The pointer crossed into or out of the window, or the window lost
    /// the keyboard**: shown, but at mode 3.
    pub fn crossed(&mut self, term: &Term<'_>) -> Act {
        match self.hidden {
            PointerHides::Yes => self.to(term.pointer_mode().hidden_on_leaving()),
            PointerHides::No => Act::Nothing,
        }
    }

    fn to(&mut self, want: PointerHides) -> Act {
        let act = match (self.hidden, want) {
            (PointerHides::No, PointerHides::Yes) => Act::Hide,
            (PointerHides::Yes, PointerHides::No) => Act::Show,
            _ => Act::Nothing,
        };
        self.hidden = want;
        act
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{Owned, PointerMode, WinSize};

    fn with_term(f: impl FnOnce(&mut Term<'_>)) {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        f(&mut t);
    }

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(baud_core::BytePos::ZERO, b);
        t.feed(
            baud_core::BytePos::ZERO,
            baud_core::ReadLen::from_index(b.len()),
        );
    }

    /// **A key hides the pointer at xterm's default, and motion shows it.**
    /// A second key while hidden does nothing; a key under tracking at mode
    /// 1 leaves it shown.
    #[test]
    fn a_key_hides_it_and_motion_shows_it() {
        with_term(|t| {
            let mut h = Hiding::new(t);
            assert_eq!(h.hidden(), PointerHides::No, "shown at first");
            assert_eq!(h.key(t), Act::Hide);
            assert_eq!(h.hidden(), PointerHides::Yes);
            assert_eq!(h.key(t), Act::Nothing, "already hidden");
            assert_eq!(h.moved(), Act::Show);
            assert_eq!(h.moved(), Act::Nothing, "already shown");
            feed(t, b"\x1b[?1000h");
            assert_eq!(h.read(t), Act::Nothing, "shown, nothing to do");
            assert_eq!(h.key(t), Act::Nothing, "tracking at mode 1: shown");
        });
    }

    /// **Mode 0 never hides, and a key under it shows a pointer hidden
    /// before**, as xterm's `xtermShowPointer` enables it at `pNever`.
    #[test]
    fn mode_0_never_hides_and_a_key_shows_it() {
        with_term(|t| {
            let mut h = Hiding::new(t);
            assert_eq!(h.key(t), Act::Hide);
            feed(t, b"\x1b[>0p");
            assert_eq!(h.read(t), Act::Nothing, "setting the mode shows nothing");
            assert_eq!(h.key(t), Act::Show, "the next key does");
            assert_eq!(h.key(t), Act::Nothing);
        });
    }

    /// **Tracking turned on or changed shows it, at any mode**, as xterm's
    /// `really_set_mousemode` and DECELR do; turning it off does not.
    #[test]
    fn tracking_turned_on_shows_it() {
        with_term(|t| {
            let mut h = Hiding::new(t);
            feed(t, b"\x1b[>2p");
            assert_eq!(h.key(t), Act::Hide);
            feed(t, b"\x1b[?1002h");
            assert_eq!(h.read(t), Act::Show, "on");
            assert_eq!(h.key(t), Act::Hide, "mode 2 hides under tracking");
            feed(t, b"\x1b[1'z");
            assert_eq!(h.read(t), Act::Show, "the locator");
            assert_eq!(h.key(t), Act::Hide);
            feed(t, b"\x1b[0'z");
            assert_eq!(h.read(t), Act::Nothing, "off shows nothing");
        });
    }

    /// **A button press shows it at every mode that hid it**, as xterm(1)
    /// documents `pointerMode`: "redisplayed if the user moves the mouse,
    /// or clicks one of its buttons"; at mode 0 nothing hid it.
    #[test]
    fn a_press_shows_it_at_every_mode() {
        with_term(|t| {
            let mut h = Hiding::new(t);
            feed(t, b"\x1b[>0p");
            assert_eq!(h.key(t), Act::Nothing, "mode 0 never hides");
            assert_eq!(h.pressed(), Act::Nothing, "so a press shows nothing");
            for mode in [&b"\x1b[>1p"[..], b"\x1b[>2p", b"\x1b[>3p"] {
                feed(t, mode);
                assert_eq!(h.key(t), Act::Hide, "{mode:?}");
                assert_eq!(h.pressed(), Act::Show, "{mode:?}");
                assert_eq!(h.hidden(), PointerHides::No);
                assert_eq!(h.pressed(), Act::Nothing, "already shown");
            }
        });
    }

    /// **`RIS` and a new pointer shape show it.**
    #[test]
    fn a_reset_or_a_new_shape_shows_it() {
        with_term(|t| {
            let mut h = Hiding::new(t);
            assert_eq!(h.key(t), Act::Hide);
            feed(t, b"\x1bc");
            assert_eq!(h.read(t), Act::Show, "RIS");
            assert_eq!(h.key(t), Act::Hide);
            feed(t, b"\x1b]22;hand2\x07");
            assert_eq!(h.read(t), Act::Show, "OSC 22");
            assert_eq!(h.key(t), Act::Hide);
            feed(t, b"x");
            assert_eq!(h.read(t), Act::Nothing, "text alone");
        });
    }

    /// **Crossing the window's edge shows it below mode 3, and at 3 only
    /// motion does**: "always hide the pointer, except when leaving the
    /// window" against "even if leaving/entering the window".
    #[test]
    fn crossing_shows_it_but_at_mode_3() {
        with_term(|t| {
            let mut h = Hiding::new(t);
            for (mode, want) in [
                (&b"\x1b[>1p"[..], Act::Show),
                (b"\x1b[>2p", Act::Show),
                (b"\x1b[>3p", Act::Nothing),
            ] {
                feed(t, mode);
                assert_eq!(h.key(t), Act::Hide, "{mode:?}");
                assert_eq!(h.crossed(t), want, "{mode:?}");
                h.moved();
            }
            assert_eq!(h.crossed(t), Act::Nothing, "shown already");
        });
    }

    /// **The configured start value is what `RIS` returns to**, and `off`
    /// never hides from the first key.
    #[test]
    fn the_start_value_is_the_embedders() {
        with_term(|t| {
            t.set_initial_pointer_mode(PointerMode::Never);
            let mut h = Hiding::new(t);
            assert_eq!(h.key(t), Act::Nothing);
            feed(t, b"\x1b[>1p");
            assert_eq!(h.key(t), Act::Hide);
            feed(t, b"\x1bc");
            assert_eq!(h.read(t), Act::Show);
            assert_eq!(h.key(t), Act::Nothing, "RIS: never again");
        });
    }
}
