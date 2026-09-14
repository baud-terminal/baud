//! **A keysym and a modifier mask become the key event the core encodes.**
//! The compositor's `wl_keyboard` reports keycodes and the masks; the XKB
//! compiler answers the keysym and the text; this names the key the way
//! `Term::key` wants it: a character with its text, or a functional key.

use baud_domain::{Codepoint, FnKey, KeyCode, KeyEvent, KeypadKey, Modifiers};
use baud_xkb::{Keycode, Keysym, State};

/// The modifiers `Term::key` cares about, from the real bits: Shift, Lock,
/// Control, Mod1 (Alt), Mod2 (NumLock), Mod4 (Super).
pub fn modifiers(mask: baud_xkb::ModMask) -> Modifiers {
    let mut m = Modifiers::NONE;
    if mask & (1 << 0) != 0 {
        m = m.with(Modifiers::SHIFT);
    }
    if mask & (1 << 1) != 0 {
        m = m.with(Modifiers::CAPS_LOCK);
    }
    if mask & (1 << 2) != 0 {
        m = m.with(Modifiers::CTRL);
    }
    if mask & (1 << 3) != 0 {
        m = m.with(Modifiers::ALT);
    }
    if mask & (1 << 4) != 0 {
        m = m.with(Modifiers::NUM_LOCK);
    }
    if mask & (1 << 6) != 0 {
        m = m.with(Modifiers::SUPER);
    }
    m
}

/// The key event for a press of `keycode` under `state`, or `None` for a
/// key that is only a modifier or that the keymap does not bind.
pub fn press(state: &State<'_>, keycode: Keycode) -> Option<KeyEvent> {
    let lookup = state.lookup(keycode)?;
    let sym = state.sym(keycode);
    let eff = state.mods().effective();
    // The modifiers the lookup consumed are not reported again: Shift that
    // made a capital is in the character, not beside it.
    let mods = modifiers(eff & !lookup.consumed);
    let code = code_of(sym).or_else(|| {
        let c = state.text(keycode)?;
        // Control's transformation is `Term::key`'s to make from the
        // modifier; give it the printable and the modifier.
        let plain = match eff & (1 << 2) != 0 {
            true => lookup.sym.to_char()?,
            false => c,
        };
        Some(KeyCode::Char(Codepoint::from_char(plain)))
    })?;
    Some(KeyEvent::press(code, mods))
}

/// The functional keys, from the keysym values the pinned header names.
fn code_of(sym: Keysym) -> Option<KeyCode> {
    Some(match sym.0 {
        0xff0d => KeyCode::Enter,
        0xff09 | 0xfe20 => KeyCode::Tab,
        0xff08 => KeyCode::Backspace,
        0xff1b => KeyCode::Escape,
        0xffff => KeyCode::Delete,
        0xff52 => KeyCode::Up,
        0xff54 => KeyCode::Down,
        0xff51 => KeyCode::Left,
        0xff53 => KeyCode::Right,
        0xff50 => KeyCode::Home,
        0xff57 => KeyCode::End,
        0xff63 => KeyCode::Insert,
        0xff55 => KeyCode::PageUp,
        0xff56 => KeyCode::PageDown,
        0xffbe..=0xffd1 => KeyCode::Function(FnKey::new(u8::try_from(sym.0 - 0xffbd).ok()?)?),
        0xff80 => KeyCode::Keypad(KeypadKey::Space),
        0xff89 => KeyCode::Keypad(KeypadKey::Tab),
        0xff8d => KeyCode::Keypad(KeypadKey::Enter),
        0xff91 => KeyCode::Keypad(KeypadKey::Pf1),
        0xff92 => KeyCode::Keypad(KeypadKey::Pf2),
        0xff93 => KeyCode::Keypad(KeypadKey::Pf3),
        0xff94 => KeyCode::Keypad(KeypadKey::Pf4),
        0xffaa => KeyCode::Keypad(KeypadKey::Multiply),
        0xffab => KeyCode::Keypad(KeypadKey::Add),
        0xffac => KeyCode::Keypad(KeypadKey::Comma),
        0xffad => KeyCode::Keypad(KeypadKey::Minus),
        0xffae => KeyCode::Keypad(KeypadKey::Period),
        0xffaf => KeyCode::Keypad(KeypadKey::Divide),
        0xffb0 => KeyCode::Keypad(KeypadKey::Digit0),
        0xffb1 => KeyCode::Keypad(KeypadKey::Digit1),
        0xffb2 => KeyCode::Keypad(KeypadKey::Digit2),
        0xffb3 => KeyCode::Keypad(KeypadKey::Digit3),
        0xffb4 => KeyCode::Keypad(KeypadKey::Digit4),
        0xffb5 => KeyCode::Keypad(KeypadKey::Digit5),
        0xffb6 => KeyCode::Keypad(KeypadKey::Digit6),
        0xffb7 => KeyCode::Keypad(KeypadKey::Digit7),
        0xffb8 => KeyCode::Keypad(KeypadKey::Digit8),
        0xffb9 => KeyCode::Keypad(KeypadKey::Digit9),
        _ => return None,
    })
}
