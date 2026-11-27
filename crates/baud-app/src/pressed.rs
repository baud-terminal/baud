//! **The keys the program was told are down, until it is told each one is
//! up.** The Linux driver's: the find field takes every key while it is
//! open, so a key pressed before it opens can be let go inside it, and a
//! key pressed inside it -- Escape, which closes it -- let go after. A
//! release goes to the program exactly when its press did: never one for a
//! press it was not sent, and never none for a press it was. kitty's event
//! types report "key repeat and key release events" (`[[source:
//! kitty-keyboard]]`), and a program counting keys down -- a game, an
//! editor's chords -- would otherwise hold a key for good or let go of one
//! it never saw.
//!
//! foot drops every release while its search is open and sends the one after
//! it closes whatever its press was (`[[source: foot-152487e-input]]`,
//! `key_press_release()`), so both happen there; this is the rule, not that
//! practice. Pairing is by the key's identity: the window names a release
//! by its press (`baud_present_wayland::Event::Key`), and a program sees
//! no more than the identity either, so two keys of one identity down at
//! once are released one for one.

use baud_domain::{KeyEvent, KeyEventKind, Modifiers};

/// Whose the keys are: the program's, or the find field's while it is
/// open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeysTo {
    Program,
    FindField,
}

/// Where one key event goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// To the program, through `Term::key`.
    Program(KeyEvent),
    /// To the find field.
    Field(KeyEvent),
    /// Nowhere: a release whose press the program was not sent.
    Nobody,
}

impl Route {
    /// **Whether this is a key typed**, a press or a repeat: a release is the
    /// end of one, which letting go of Ctrl after a copy's chord would be.
    pub fn typed(self) -> bool {
        match self {
            Route::Program(k) | Route::Field(k) => k.kind != KeyEventKind::Release,
            Route::Nobody => false,
        }
    }
}

/// The presses the program was sent and not yet told the end of.
#[derive(Default)]
pub struct Pressed(Vec<KeyEvent>);

impl Pressed {
    /// Where the window's key event `e` goes while the keys are `to`'s, and
    /// a press the program is sent is remembered for its release. A repeat
    /// goes where the keys go, as before.
    pub fn key(&mut self, e: KeyEvent, to: KeysTo) -> Route {
        match (e.kind, to) {
            (KeyEventKind::Release, _) => match self.0.iter().position(|p| p.code == e.code) {
                Some(i) => {
                    self.0.swap_remove(i);
                    Route::Program(e)
                }
                None => Route::Nobody,
            },
            (_, KeysTo::FindField) => Route::Field(e),
            (KeyEventKind::Press, KeysTo::Program) => {
                self.0.push(e);
                Route::Program(e)
            }
            (KeyEventKind::Repeat, KeysTo::Program) => Route::Program(e),
        }
    }

    /// Where a typed character goes -- a Compose sequence's or the input
    /// method's, which no release follows, so nothing is remembered.
    pub fn typed(e: KeyEvent, to: KeysTo) -> Route {
        match to {
            KeysTo::Program => Route::Program(e),
            KeysTo::FindField => Route::Field(e),
        }
    }

    /// **Every key still down, let go**, with no modifier held, and none
    /// remembered after: the window losing the keyboard, after which the
    /// compositor sends it no release -- `wl_keyboard.leave` "resets all
    /// values to their defaults", the keys logically down among them,
    /// empty by default (`[[source: wayland]]`). The core sends a release
    /// only where a program asked for release events through the progressive
    /// keyboard enhancement protocol (CSI > flags u).
    pub fn let_go(&mut self) -> impl Iterator<Item = KeyEvent> + '_ {
        self.0.drain(..).map(|press| KeyEvent {
            kind: KeyEventKind::Release,
            mods: Modifiers::NONE,
            ..press
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::{Codepoint, KeyCode};

    fn key(c: char, kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            kind,
            ..KeyEvent::press(KeyCode::Char(Codepoint::from_char(c)), Modifiers::NONE)
        }
    }

    use KeyEventKind::{Press, Release, Repeat};
    use KeysTo::{FindField, Program};

    /// **A release goes to the program exactly when its press did**: a
    /// key pressed before the field opened and let go inside it is let go
    /// to the program; a key pressed inside the field and let go after it
    /// closed goes nowhere; a release twice, or with no press at all,
    /// nowhere. Repeats and presses go where the keys go. Perturbation:
    /// send a release while the field is open to the field (as the driver
    /// did, dropping it) and the first assertion fails; send every release
    /// to the program and the second does.
    #[test]
    fn a_release_goes_to_the_program_exactly_when_its_press_did() {
        let mut p = Pressed::default();
        let (a, esc) = (key('a', Press), key('\u{1b}', Press));
        assert_eq!(p.key(a, Program), Route::Program(a));
        // The field opens with `a` still down, and `a` comes up inside it.
        assert_eq!(
            p.key(key('a', Repeat), FindField),
            Route::Field(key('a', Repeat))
        );
        assert_eq!(
            p.key(key('a', Release), FindField),
            Route::Program(key('a', Release))
        );
        // A key pressed into the field and let go after it closed.
        assert_eq!(p.key(esc, FindField), Route::Field(esc));
        assert_eq!(p.key(key('\u{1b}', Release), Program), Route::Nobody);
        // Released twice, or never pressed.
        assert_eq!(p.key(key('a', Release), Program), Route::Nobody);
        assert_eq!(p.key(key('b', Release), Program), Route::Nobody);
        // Two keys of one identity are let go one for one.
        p.key(a, Program);
        p.key(a, Program);
        assert_eq!(
            p.key(key('a', Release), FindField),
            Route::Program(key('a', Release))
        );
        assert_eq!(
            p.key(key('a', Release), Program),
            Route::Program(key('a', Release))
        );
        assert_eq!(p.key(key('a', Release), Program), Route::Nobody);
        // Typed text is never remembered: no release is owed for it.
        assert_eq!(Pressed::typed(a, Program), Route::Program(a));
        assert_eq!(Pressed::typed(a, FindField), Route::Field(a));
        assert_eq!(p.let_go().count(), 0);
    }

    /// **The keys down when the keyboard leaves are let go once**, each
    /// its press with no modifier held, and only those the program was
    /// sent: a key pressed into the field is not among them. Perturbation:
    /// read the presses without emptying them and the second leave lets
    /// them go again.
    #[test]
    fn the_keys_down_when_the_keyboard_leaves_are_let_go_once() {
        let mut p = Pressed::default();
        let shifted = KeyEvent {
            base_layout: Some(Codepoint::from_char('q')),
            ..KeyEvent::press(KeyCode::Char(Codepoint::from_char('w')), Modifiers::SHIFT)
        };
        p.key(shifted, Program);
        p.key(KeyEvent::press(KeyCode::Up, Modifiers::NONE), Program);
        p.key(key('x', Press), FindField);
        let gone: Vec<KeyEvent> = p.let_go().collect();
        assert_eq!(gone.len(), 2);
        assert!(
            gone.iter()
                .all(|e| e.kind == Release && e.mods == Modifiers::NONE)
        );
        assert_eq!(gone[0].code, shifted.code);
        assert_eq!(
            gone[0].base_layout, shifted.base_layout,
            "named as its press"
        );
        assert_eq!(gone[1].code, KeyCode::Up);
        assert_eq!(p.let_go().count(), 0, "once");
        assert_eq!(p.key(key('w', Release), Program), Route::Nobody);
    }
}
