#![deny(dead_code)]
//! **The longest key `encode_key` produces, measured.**
//!
//! `KeyBytes::MAX_LEN` is a fixed array, and a key that would not fit is
//! dropped whole and comes back `None` -- the same answer as a key that sends
//! nothing. So a bound that fell below the longest key would fail silently, and
//! `measured <= MAX_LEN` could never fail at all: an overflowed key is not
//! measured. What can fail is the equality. The longest key is derived here from
//! `[[source: kitty-keyboard]]`'s fields, the sweep must find exactly that
//! length, and the key that has it must come back whole.
//!
//! **The sweep is exhaustive by argument over the codepoints and by enumeration
//! over everything else.** A codepoint reaches the output in only three ways,
//! all read off `key.rs`: as decimal digits (kitty's key number, alternate keys
//! and associated text, and `modifyOtherKeys`' `u16` number), as UTF-8 or one
//! ISO 8859-1 byte (the base layer's produced text), and through the base
//! layer's tests on it (the control fold's ranges, Latin-1's `<= 0xFF` and its
//! eightBitInput shift's `< 0x80` on the produced text, and `<= 0xFFFF`). The
//! codepoints below stand for every class of those, U+10FFFF the widest in
//! digits and in UTF-8. The shift sets the high bit of the one byte, so it
//! never changes a key's length. The modifiers are all 256 masks on the kitty
//! layer; the base layer reads Shift, Alt, Control and Meta one bit at a time
//! and the other four only through `is_none` (`Modifiers::wire` masks them
//! away), so there it is the sixteen low masks, with and without Super. The
//! kitty layer's three optional fields are swept together, for the reason
//! `no_kitty_key_is_longer_than_forty_bytes` gives. Every key is enumerated on
//! both layers; every event kind and every flag set on the kitty layer, where
//! the kind is read; and on the base layer, presses only, for the reason
//! `no_base_layer_key_is_longer_than_fourteen_bytes` gives, over the full
//! product of every state it reads (DECCKM, LNM, `?1036`, `?1039`, `?1034`,
//! DECKPAM, `modifyOtherKeys` and its format, `S8C1T`, VT52 and the input
//! encoding).

use baud_domain::*;

/// The eight modifiers, so a mask builds a `Modifiers` without its private bits.
const EIGHT: [Modifiers; 8] = [
    Modifiers::SHIFT,
    Modifiers::ALT,
    Modifiers::CTRL,
    Modifiers::META,
    Modifiers::SUPER,
    Modifiers::HYPER,
    Modifiers::CAPS_LOCK,
    Modifiers::NUM_LOCK,
];

fn mods(mask: u8) -> Modifiers {
    (0..8)
        .filter(|i| mask & (1 << i) != 0)
        .fold(Modifiers::NONE, |m, i| m.with(EIGHT[i]))
}

/// The five enhancements, so a bit set builds a `KittyFlags` the way a
/// `CSI = flags ; 2 u` would.
const FIVE: [KittyFlags; 5] = [
    KittyFlags::DISAMBIGUATE,
    KittyFlags::EVENT_TYPES,
    KittyFlags::ALTERNATE_KEYS,
    KittyFlags::ALL_AS_ESCAPES,
    KittyFlags::ASSOCIATED_TEXT,
];

fn flags(bits: u8) -> KittyFlags {
    (0..5)
        .filter(|i| bits & (1 << i) != 0)
        .fold(KittyFlags::NONE, |f, i| f.apply(FIVE[i], KittySetMode::Or))
}

fn cp(v: u32) -> Codepoint {
    Codepoint::new(v).expect("a scalar value")
}

/// **One list per enum feeds both the sweep and a match without a wildcard,
/// so a key left out of the sweep fails to compile.** `every!` expands its
/// list twice: into the array the sweep walks, and into the patterns of an
/// exhaustive match. A variant missing from the list -- dropped from it, or
/// added to the enum and not to it -- is a non-exhaustive match (E0004) in this
/// test rather than a key the bound was never measured on, and
/// `#![deny(dead_code)]` makes an array no sweep reads a build error too. A
/// variant named after `carrying` holds a value: it is matched with `(_)`, is
/// not in the array, and is swept by code the compiler does not check, which
/// [`NAMED`]'s doc lists.
macro_rules! every {
    ($ty:ident; $($v:ident),+ $(; carrying $($d:ident),+)?) => {{
        const fn listed(k: $ty) -> $ty {
            match k {
                $($ty::$v)|+ $($(| $ty::$d(_))+)? => k,
            }
        }
        [$(listed($ty::$v)),+]
    }};
}

/// The fourteen keys that carry nothing. The four that carry a value are
/// swept as: `Char` by the codepoint classes below, `Function` by F1-F20,
/// `Keypad` by [`KEYPAD`] and `Pua` by [`PUA`].
const NAMED: [KeyCode; 14] = every!(KeyCode;
    Up, Down, Right, Left, Home, End, Insert, Delete, PageUp, PageDown, Enter, Tab, Backspace,
    Escape;
    carrying Char, Function, Keypad, Pua);

const KEYPAD: [KeypadKey; 35] = every!(KeypadKey;
    Space, Tab, Enter, Pf1, Pf2, Pf3, Pf4, Multiply, Add, Comma, Minus, Period, Divide, Digit0,
    Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Equal,
    Left, Right, Up, Down, PageUp, PageDown, Home, End, Insert, Delete, Begin);

const PUA: [PuaKey; 48] = every!(PuaKey;
    CapsLock, ScrollLock, NumLock, PrintScreen, Pause, Menu,
    F21, F22, F23, F24, F25, F26, F27, F28, F29, F30, F31, F32, F33, F34, F35,
    MediaPlay, MediaPause, MediaPlayPause, MediaReverse, MediaStop, MediaFastForward,
    MediaRewind, MediaTrackNext, MediaTrackPrevious, MediaRecord,
    LowerVolume, RaiseVolume, MuteVolume,
    LeftShift, LeftControl, LeftAlt, LeftSuper, LeftHyper, LeftMeta,
    RightShift, RightControl, RightAlt, RightSuper, RightHyper, RightMeta,
    IsoLevel3Shift, IsoLevel5Shift);

/// Every key but the text keys: the fourteen named ones, F1-F20 (all
/// `FnKey::new` accepts), the keypad and the Private Use Area keys.
fn functional() -> Vec<KeyCode> {
    let mut v = NAMED.to_vec();
    v.extend((1..=20).map(|n| KeyCode::Function(FnKey::new(n).expect("F1-F20"))));
    assert!(FnKey::new(21).is_none(), "F21 is a PUA key, not an FnKey");
    v.extend(KEYPAD.map(KeyCode::Keypad));
    v.extend(PUA.map(KeyCode::Pua));
    v
}

/// The base layer's text-key identities: the control fold's four ranges
/// (`@`, `a`, space, `?`), a key with no control form, the top of ISO 8859-1,
/// the `u16` ceiling `modifyOtherKeys` can name, the first codepoint past it,
/// and U+10FFFF.
const BASE_IDS: [u32; 9] = [
    0x40, 0x61, 0x20, 0x3f, 0x31, 0xff, 0xffff, 0x1_0000, 0x10_ffff,
];

/// The kitty layer's text-key identities, which it writes as decimal digits:
/// the widest codepoint of each digit count, one to seven.
const KITTY_IDS: [u32; 7] = [9, 99, 999, 9_999, 99_999, 999_999, 0x10_ffff];

/// U+10FFFF, 1114111: seven decimal digits, the most a codepoint has.
const WIDEST: u32 = 0x10_ffff;

/// The longest kitty key, summed from `[[source: kitty-keyboard]]`'s format
/// rather than read off the encoder: "CSI unicode-key-code:alternate-key-codes ;
/// modifiers:event-type ; text-as-codepoints u", where "All parameters are
/// decimal numbers" and the modifier value is "1 + actual modifiers" over its
/// eight bits. Every field at its widest: a codepoint is at most seven digits
/// (U+10FFFF), the modifiers three (1 + 255), an event type one (1-3).
fn kitty_longest() -> usize {
    let (csi, codepoint, mods, event) = ("\x1b[".len(), "1114111".len(), "256".len(), 1);
    csi + codepoint // key number
        + 1 + codepoint // :shifted key
        + 1 + codepoint // :base layout key
        + 1 + mods // ;modifiers
        + 1 + event // :event type
        + 1 + codepoint // ;associated text
        + 1 // u
}

/// Every state of DECBKM, `?1037` and `?1035` together.
fn key_modes() -> Vec<(BackarrowKey, DeleteKey, NumLockKeypad)> {
    let mut out = Vec::new();
    for b in [BackarrowKey::Delete, BackarrowKey::Backspace] {
        for d in [DeleteKey::Remove, DeleteKey::Del] {
            for n in [NumLockKeypad::Overrides, NumLockKeypad::Ignored] {
                out.push((b, d, n));
            }
        }
    }
    out
}

/// The longest base-layer key: `modifyOtherKeys`' xterm format, `CSI 27 ;
/// modifier ; codepoint ~`, with xterm's widest modifier parameter (16, all
/// four modifiers) and the widest codepoint a `u16` parameter names.
fn base_longest() -> usize {
    "\x1b[".len() + "27".len() + 1 + "16".len() + 1 + "65535".len() + 1
}

/// A fresh terminal's state with every enhancement set.
fn all_flags() -> KeyState {
    KeyState {
        modes: Modes::default(),
        keypad: KeypadMode::default(),
        other_keys: OtherKeys::default(),
        other_keys_format: OtherKeysFormat::default(),
        c1: C1Transmit::SevenBit,
        terminal: TerminalMode::default(),
        encoding: InputEncoding::default(),
        kitty: KittyFlags::ALL,
        backarrow: BackarrowKey::default(),
        delete: DeleteKey::default(),
        num_lock: NumLockKeypad::default(),
    }
}

/// **The longest key is 40 bytes, and it comes back whole.** A `MAX_LEN` below
/// 40 drops it, and this is the assertion that sees the drop. The witness is a
/// repeat, `:2`: associated text is embedded for "key events that generate
/// text", which a held text key does. A release, `:3`, carries no text and is
/// shorter.
#[test]
fn the_longest_key_is_forty_bytes_and_comes_back_whole() {
    assert_eq!(kitty_longest(), 40);
    assert_eq!(base_longest(), 14);
    let witness = KeyEvent {
        code: KeyCode::Char(cp(WIDEST)),
        mods: mods(0xff),
        kind: KeyEventKind::Repeat,
        text: Some(cp(WIDEST)),
        shifted: Some(cp(WIDEST)),
        base_layout: Some(cp(WIDEST)),
    };
    let got = encode_key(witness, all_flags()).map(|k| k.as_bytes().to_vec());
    assert_eq!(
        got.as_deref(),
        Some(b"\x1b[1114111:1114111:1114111;256:2;1114111u".as_slice()),
        "the longest key came back {got:?}; dropped means MAX_LEN {} is below {}",
        KeyBytes::MAX_LEN,
        kitty_longest(),
    );
    // Under S8C1T the introducer is one byte and the rest is the same.
    let eight = KeyState {
        c1: C1Transmit::EightBit,
        ..all_flags()
    };
    let got = encode_key(witness, eight).map(|k| k.as_bytes().to_vec());
    assert_eq!(
        got.as_deref(),
        Some(b"\x9b1114111:1114111:1114111;256:2;1114111u".as_slice())
    );
}

/// **No kitty key is longer than 40 bytes, and one is exactly 40.** Every key,
/// every modifier mask, every event kind, all 31 flag sets and both C1
/// transmissions -- through the CSI-u form, and through the base layer for a
/// key the flags leave alone. The three optional fields go in together, all
/// absent or all at their widest: whether a key takes the CSI-u form depends on
/// its code, its modifiers and the flags alone, and there `kitty_csi_u` only
/// appends for each field present, so all three present bound every mix of
/// them; a key the flags leave alone reads only the text, swept both ways. All
/// eight mixes cost four times the run, eight seconds in a debug build.
#[test]
fn no_kitty_key_is_longer_than_forty_bytes() {
    let mut codes = functional();
    codes.extend(KITTY_IDS.map(|v| KeyCode::Char(cp(v))));
    let masks: Vec<Modifiers> = (0..=255u8).map(mods).collect();
    let states: Vec<KeyState> = (1..=31u8)
        .flat_map(|bits| {
            [C1Transmit::SevenBit, C1Transmit::EightBit].map(|c1| KeyState {
                c1,
                kitty: flags(bits),
                ..all_flags()
            })
        })
        .collect();
    let mut longest = 0;
    for &code in &codes {
        for &m in &masks {
            for kind in [
                KeyEventKind::Press,
                KeyEventKind::Repeat,
                KeyEventKind::Release,
            ] {
                for field in [None, Some(cp(WIDEST))] {
                    let e = KeyEvent {
                        code,
                        mods: m,
                        kind,
                        text: field,
                        shifted: field,
                        base_layout: field,
                    };
                    for &s in &states {
                        if let Some(k) = encode_key(e, s) {
                            longest = longest.max(k.as_bytes().len());
                        }
                    }
                }
            }
        }
    }
    assert_eq!(longest, kitty_longest(), "the kitty layer's longest key");
}

/// **No base-layer key is longer than 14 bytes, and one is exactly 14.** No
/// flag set, and every state the base layer reads, in full product. A repeat is
/// a press there and a release sends nothing
/// (`the_base_encoder_sends_a_repeat_as_a_press_and_no_release`), so presses
/// stand for all three.
#[test]
fn no_base_layer_key_is_longer_than_fourteen_bytes() {
    let mut keys: Vec<(KeyCode, Option<Codepoint>)> =
        functional().into_iter().map(|c| (c, None)).collect();
    for id in BASE_IDS {
        for text in [None, Some(id), Some(0xff), Some(WIDEST)] {
            keys.push((KeyCode::Char(cp(id)), text.map(cp)));
        }
    }
    let modes = |m: u8| {
        let mut out = Modes::default();
        for (i, bit) in [
            ModeBit::CursorKeys,
            ModeBit::LineFeedNewLine,
            ModeBit::MetaSendsEscape,
            ModeBit::AltSendsEscape,
            ModeBit::EightBitInput,
        ]
        .into_iter()
        .enumerate()
        {
            match m & (1 << i) != 0 {
                true => out.set(bit),
                false => out.reset(bit),
            }
        }
        out
    };
    let mut states = Vec::new();
    for m in 0..32u8 {
        for keypad in [KeypadMode::Numeric, KeypadMode::Application] {
            for other_keys in [OtherKeys::Off, OtherKeys::Level1, OtherKeys::Level2] {
                for other_keys_format in [OtherKeysFormat::Xterm, OtherKeysFormat::CsiU] {
                    for c1 in [C1Transmit::SevenBit, C1Transmit::EightBit] {
                        for terminal in [TerminalMode::Ansi, TerminalMode::Vt52] {
                            for encoding in [InputEncoding::Utf8, InputEncoding::Latin1] {
                                // DECBKM, `?1037` and `?1035`, every pair.
                                for (backarrow, delete, num_lock) in key_modes() {
                                    states.push(KeyState {
                                        modes: modes(m),
                                        keypad,
                                        other_keys,
                                        other_keys_format,
                                        c1,
                                        terminal,
                                        encoding,
                                        kitty: KittyFlags::NONE,
                                        backarrow,
                                        delete,
                                        num_lock,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let masks: Vec<Modifiers> = (0..16u8)
        .flat_map(|low| {
            [
                mods(low),
                mods(low).with(Modifiers::SUPER),
                // NumLock reaches `?1035`'s override.
                mods(low).with(Modifiers::NUM_LOCK),
            ]
        })
        .collect();
    let mut longest = 0;
    for &(code, text) in &keys {
        for &m in &masks {
            let e = KeyEvent {
                text,
                ..KeyEvent::press(code, m)
            };
            for &s in &states {
                if let Some(k) = encode_key(e, s) {
                    longest = longest.max(k.as_bytes().len());
                }
            }
        }
    }
    assert_eq!(longest, base_longest(), "the base layer's longest key");
}
