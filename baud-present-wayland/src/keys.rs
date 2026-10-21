//! **A keysym and a modifier mask become the key event the core encodes.**
//! The compositor's `wl_keyboard` reports keycodes and the masks; the XKB
//! compiler answers the keysym and the text; this names the key the way
//! `Term::key` wants it: a character with its text, or a functional key.

use baud_domain::{Codepoint, FnKey, KeyCode, KeyEvent, KeypadKey, Modifiers};
use baud_xkb::{Keycode, Keymap, Keysym, Mods, State};

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
    let mut mods = modifiers(eff & !lookup.consumed);
    // **Except NumLock on a keypad key**, `XK_KP_Space` to `XK_KP_Equal`:
    // the keymap's `KEYPAD` type consumes it to make a digit, and `?1035`
    // asks whether it is on. xterm tests it against the event's whole
    // state, `xw->work.num_lock & evt_state` for `IsKeypadKey`
    // (`[[source: xterm-input-c]]` lines 1131-1136), so a digit leaves
    // DECKPAM as `KP_Add` does. A lock adds no modifier parameter.
    if (0xff80..=0xffbd).contains(&sym.0) && eff & (1 << 4) != 0 {
        mods = mods.with(Modifiers::NUM_LOCK);
    }
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
    let mut e = KeyEvent::press(code, mods);
    if let KeyCode::Char(id) = code {
        // kitty's base layout key: the physical key's character "in the
        // standard PC-101 key layout" (`[[source: kitty-keyboard]]`), sent
        // where it differs from the key's own unshifted one in the active
        // layout, as kitty compares it with the key's clean keysym.
        let mut clean = state.clone();
        clean.update(Mods::default(), state.group());
        let own = clean.lookup(keycode).and_then(|l| l.sym.to_char());
        e.base_layout = pc101(keycode)
            .filter(|&b| Some(b) != own)
            .map(Codepoint::from_char)
            .filter(|&b| b != id);
        // Control on a key whose character is not ASCII: the key's first
        // other layout that gives one, libX11's `XkbLC_ControlFallback`.
        if eff & (1 << 2) != 0 && lookup.consumed & (1 << 2) == 0 && lookup.sym.0 > 0x7f {
            e.control = control_ascii(state, keycode).map(Codepoint::from_char);
        }
    }
    // **A keypad key carries the text it types**, which is how kitty's
    // disambiguate flag tells a digit from a navigation key (`[[source:
    // kitty-keyboard]]`, "all non text keypad keys"): the keymap's text,
    // none while Control, Alt or Super is held and none for a control code,
    // as kitty's keyboard layer gives it (`[[source: kitty-xkb-glfw-c]]`).
    // The numeric column sends that text (`keypad_numeric`); the application
    // column reads the key.
    if let KeyCode::Keypad(_) = code {
        let held = [Modifiers::CTRL, Modifiers::ALT, Modifiers::SUPER];
        e.text = state
            .text(keycode)
            .filter(|c| !c.is_control() && !held.iter().any(|m| mods.has(*m)))
            .map(Codepoint::from_char);
    }
    Some(e)
}

/// **The key's character under Control from another layout**, where the
/// active layout's is not ASCII, as libX11's `XLookupString` finds it with
/// `XkbLC_ControlFallback` -- which its `XKBUse.c` turns on by default --
/// for xterm (`[[source: libx11-xkbbind-c]]` 694-725, `[[source:
/// libx11-xkbuse-c]]` 659): the key looked up in each other group at the
/// same modifiers, in order, the first keysym below `0x80` taken. VTE does
/// the same over groups 0 to 3 (`[[source: vte-0-80-0-widget]]`
/// 1439-1490). XKB has at most four groups, and a group past a key's own
/// wraps, so four lookups cover every keymap. On a keymap with no Latin
/// layout there is none, and the key keeps its letter.
fn control_ascii(state: &State<'_>, keycode: Keycode) -> Option<char> {
    (0..4).filter(|&g| g != state.group()).find_map(|g| {
        let mut st = state.clone();
        st.update(state.mods(), g);
        let l = st.lookup(keycode)?;
        (l.sym != Keysym::NO_SYMBOL && l.sym.0 < 0x80)
            .then(|| l.sym.to_char())
            .flatten()
    })
}

/// **The standard PC-101 layout**, by XKB keycode (evdev plus 8): the
/// character each printable key types unshifted on it, the first level of
/// the pinned US keymap (`baud-xkb/tests/fixtures/keymap-us.xkb`, which
/// `pc101_is_the_us_keymaps_first_level` holds it to). The ISO key between
/// left Shift and Z is not on a 101-key board, and the keypad's keys are
/// kitty's functional keys, not text.
const PC101: [(u32, char); 48] = [
    (10, '1'),
    (11, '2'),
    (12, '3'),
    (13, '4'),
    (14, '5'),
    (15, '6'),
    (16, '7'),
    (17, '8'),
    (18, '9'),
    (19, '0'),
    (20, '-'),
    (21, '='),
    (24, 'q'),
    (25, 'w'),
    (26, 'e'),
    (27, 'r'),
    (28, 't'),
    (29, 'y'),
    (30, 'u'),
    (31, 'i'),
    (32, 'o'),
    (33, 'p'),
    (34, '['),
    (35, ']'),
    (38, 'a'),
    (39, 's'),
    (40, 'd'),
    (41, 'f'),
    (42, 'g'),
    (43, 'h'),
    (44, 'j'),
    (45, 'k'),
    (46, 'l'),
    (47, ';'),
    (48, '\''),
    (49, '`'),
    (51, '\\'),
    (52, 'z'),
    (53, 'x'),
    (54, 'c'),
    (55, 'v'),
    (56, 'b'),
    (57, 'n'),
    (58, 'm'),
    (59, ','),
    (60, '.'),
    (61, '/'),
    (65, ' '),
];

/// The key's character on the PC-101 layout, if it is a printable key.
pub fn pc101(keycode: Keycode) -> Option<char> {
    PC101
        .binary_search_by_key(&keycode.0, |&(k, _)| k)
        .ok()
        .map(|i| PC101[i].1)
}

/// **A character typed as text, not as a key**: what a Compose sequence or
/// an input method commits, as the macOS app's `text` types what AppKit's
/// input manager commits -- the character's lower case as the key, the
/// character as its text, no modifiers.
pub fn typed(c: char) -> KeyEvent {
    let identity = c.to_lowercase().next().unwrap_or(c);
    let mut e = KeyEvent::press(
        KeyCode::Char(Codepoint::from_char(identity)),
        Modifiers::NONE,
    );
    e.text = Some(Codepoint::from_char(c));
    e
}

/// **A key the terminal keeps for itself**: the clipboard's keys and the
/// zoom keys as the Linux desktop binds them, never sent to the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    /// Ctrl+Shift+C: the selection to the clipboard.
    Copy,
    /// Ctrl+Shift+V: the clipboard to the program.
    Paste,
    /// Shift+Insert: xterm's `insert-selection(SELECT, CUT_BUFFER0)`, the
    /// primary selection, or the clipboard under `?1041`.
    PasteSelection,
    /// Ctrl+plus, Ctrl+equal or Ctrl+KP_Add: the font a step larger.
    ZoomIn,
    /// Ctrl+minus or Ctrl+KP_Subtract: a step smaller.
    ZoomOut,
    /// Ctrl+0 or Ctrl+KP_0: back to the configured size.
    ZoomReset,
    /// Ctrl+Shift+F: the find field, Ghostty's and Alacritty's binding.
    Find,
}

/// **The shortcut a press of `keycode` makes under `state`, if any.**
/// Ctrl+Shift+C and Ctrl+Shift+V are foot's `clipboard-copy` and
/// `clipboard-paste` (`[[source: foot-2705e36-foot-ini-5]]`) and kitty's
/// `kitty_mod+c` and `kitty_mod+v` (`[[source:
/// kitty-options-definition]]`); Shift+Insert is xterm's default
/// translation (`[[source: xterm-man]]`), and foot's, kitty's and
/// wezterm's primary paste (`[[source: wezterm-3725482-commands]]`; Jev's
/// lean, BIND, 0.66/0.44).
///
/// **Matched as foot matches a binding** (`[[source: foot-2705e36-input]]`
/// 1709-1745): the key's first-level keysyms in the active layout against
/// the exact chord, then the keysym the state translates to against the
/// chord less what the lookup consumed, then the key's keysym in the first
/// layout, as foot lists a binding's key codes with a fresh state
/// (`[[source: foot-2705e36-key-binding]]` 269-290). The last is what
/// makes Ctrl+Shift+C on a Cyrillic or Hebrew layout listed after a Latin
/// one. The chord is Shift, Control, Alt and Super, exact, so Caps Lock,
/// which is not Shift, makes no shortcut and Alt or Super held makes none.
pub fn shortcut(km: &Keymap, state: &State<'_>, keycode: Keycode) -> Option<Shortcut> {
    let key = km.key(keycode)?;
    let eff = state.mods().effective();
    let lookup = state.lookup(keycode)?;
    let first = |layout: usize| -> &[Keysym] {
        key.groups
            .get(layout)
            .and_then(|g| g.levels.first())
            .map_or(&[], Vec::as_slice)
    };
    if let Some(s) = first(lookup.layout)
        .iter()
        .find_map(|&sym| bound(sym, chord(eff)))
    {
        return Some(s);
    }
    if let Some(s) = bound(lookup.sym, chord(eff & !lookup.consumed)) {
        return Some(s);
    }
    bound(State::new(km).sym(keycode), chord(eff))
}

/// The chord of a modifier mask: Shift, Control, Alt and Super.
fn chord(mask: baud_xkb::ModMask) -> Modifiers {
    let held = modifiers(mask);
    [
        Modifiers::SHIFT,
        Modifiers::CTRL,
        Modifiers::ALT,
        Modifiers::SUPER,
    ]
    .into_iter()
    .filter(|m| held.has(*m))
    .fold(Modifiers::NONE, Modifiers::with)
}

/// The shortcut bound to `sym` under `chord`: foot's keysyms, `c`, `v` and
/// `Insert`, lower case as its bindings name them, and foot's
/// `font-increase`, `font-decrease` and `font-reset` defaults, "Control+plus
/// Control+equal Control+KP_Add", "Control+minus Control+KP_Subtract" and
/// "Control+0 Control+KP_0" (`[[source: foot-2705e36-foot-ini-5]]`
/// 1368-1377; Jev's lean, CTRL, 0.50/0.40, over kitty's Ctrl+Shift).
fn bound(sym: Keysym, chord: Modifiers) -> Option<Shortcut> {
    let ctrl = Modifiers::NONE.with(Modifiers::CTRL);
    let ctrl_shift = ctrl.with(Modifiers::SHIFT);
    const INSERT: u32 = 0xff63;
    const PLUS: u32 = 0x2b;
    const EQUAL: u32 = 0x3d;
    const MINUS: u32 = 0x2d;
    const ZERO: u32 = 0x30;
    const KP_ADD: u32 = 0xffab;
    const KP_SUBTRACT: u32 = 0xffad;
    const KP_0: u32 = 0xffb0;
    match (sym.0, chord) {
        (0x63, c) if c == ctrl_shift => Some(Shortcut::Copy),
        (0x76, c) if c == ctrl_shift => Some(Shortcut::Paste),
        (0x66, c) if c == ctrl_shift => Some(Shortcut::Find),
        (INSERT, c) if c == Modifiers::SHIFT => Some(Shortcut::PasteSelection),
        (PLUS | EQUAL | KP_ADD, c) if c == ctrl => Some(Shortcut::ZoomIn),
        (MINUS | KP_SUBTRACT, c) if c == ctrl => Some(Shortcut::ZoomOut),
        (ZERO | KP_0, c) if c == ctrl => Some(Shortcut::ZoomReset),
        _ => None,
    }
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
        // The keypad with NumLock off, `XKB_KEY_KP_Home` to `XKB_KEY_KP_Delete`
        // (`[[source: xkbcommon-keysyms]]` lines 299-311); with NumLock on the
        // same keys produce `KP_0`-`KP_9` and `KP_Decimal` below.
        0xff95 => KeyCode::Keypad(KeypadKey::Home),
        0xff96 => KeyCode::Keypad(KeypadKey::Left),
        0xff97 => KeyCode::Keypad(KeypadKey::Up),
        0xff98 => KeyCode::Keypad(KeypadKey::Right),
        0xff99 => KeyCode::Keypad(KeypadKey::Down),
        0xff9a => KeyCode::Keypad(KeypadKey::PageUp),
        0xff9b => KeyCode::Keypad(KeypadKey::PageDown),
        0xff9c => KeyCode::Keypad(KeypadKey::End),
        0xff9d => KeyCode::Keypad(KeypadKey::Begin),
        0xff9e => KeyCode::Keypad(KeypadKey::Insert),
        0xff9f => KeyCode::Keypad(KeypadKey::Delete),
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
        0xffbd => KeyCode::Keypad(KeypadKey::Equal),
        _ => return None,
    })
}
