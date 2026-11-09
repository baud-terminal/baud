//! **A held key repeats.** Two checks of it: `run()` switches the system's
//! press-and-hold accent picker off before its first window can take a key
//! (the registration itself is read back in
//! `baud-present-macos/tests/press_and_hold.rs`), and a repeat AppKit hands
//! the app reaches the program encoded as kitty's keyboard protocol says,
//! through the app's own event building and `Term::key`, with DECARM reset
//! dropping it.

use super::{
    Keyed, chord, held_with_locks, kind_of, pc101, pressed, shift_unshifted, special, typed_text,
};
use crate::layouts::KeyLayout;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, KeyEvent, KeyEventKind, Modifiers, ReadLen, WinSize};
use macos_liquid_glass::drawable::{KeyPress, Modifiers as M};
use macos_liquid_glass::objc2_app_kit::NSEventModifierFlags;

const SRC: &str = include_str!("../app.rs");

/// The registration, as `run()` spells it.
const REGISTERS: &str = "baud_present_macos::launch::register_defaults();";

/// `run()`'s body up to the window's launch hook, the code before any key
/// can arrive, with comment lines left out.
fn run_before_window(src: &str) -> String {
    let body = src.split("\npub fn run() {\n").nth(1).expect("run()");
    let end = body
        .find("baud_present_macos::launch::when_launched(")
        .expect("its window");
    body[..end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// **`run()` registers press-and-hold off before its first window**: a
/// window that takes a key with the picker on hands a held letter's repeat
/// to `interpretKeyEvents:`, and the picker opens.
///
/// Breaks: the call removed, or moved after `when_launched` -- the
/// controls below splice each into the source and see it fail.
#[test]
fn run_registers_press_and_hold_off_before_the_first_window() {
    assert!(
        run_before_window(SRC).contains(REGISTERS),
        "run() does not call register_defaults before when_launched"
    );
    let removed = SRC.replacen(REGISTERS, "", 1);
    assert!(
        !run_before_window(&removed).contains(REGISTERS),
        "control: removed"
    );
    let moved = removed.replacen(
        "baud_present_macos::launch::when_launched(",
        &format!("baud_present_macos::launch::when_launched(\n    {REGISTERS}"),
        1,
    );
    assert!(
        moved.contains(REGISTERS),
        "control: the moved call is there"
    );
    assert!(
        !run_before_window(&moved).contains(REGISTERS),
        "control: moved after"
    );
}

/// A held key: its key code, characters, characters without modifiers,
/// modifiers and lock flags as AppKit hands them, and kitty's number and
/// final byte for it.
type Row = (
    u16,
    &'static str,
    &'static str,
    M,
    NSEventModifierFlags,
    u32,
    char,
);

/// What kitty's flags make of a repeat: without event types (flag 2) it
/// is sent as its press; with them but without all keys as escape codes
/// (flag 8), text and the legacy keys are still sent as their press; with
/// both, every repeat is reported.
#[derive(Clone, Copy)]
enum Kitty {
    NoEventTypes,
    EventTypes,
    EventTypesAllEscaped,
}

fn key_press(row: &Row, kind: KeyEventKind) -> KeyPress {
    let &(code, chars, unmod, mods, ..) = row;
    KeyPress {
        key_code: code,
        characters: chars.to_string(),
        unmodified: unmod.to_string(),
        modifiers: mods,
        repeat: kind == KeyEventKind::Repeat,
    }
}

/// The bytes `e` sends once `modes` is fed, through `reads::input` as
/// `App::send` sends a key.
fn core_sends(modes: &[u8], e: KeyEvent) -> Vec<u8> {
    let ws = WinSize {
        ws_col: 20,
        ws_row: 4,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    t.write_at(BytePos::ZERO, modes);
    t.feed(BytePos::ZERO, ReadLen::from_index(modes.len()));
    t.clear_reply();
    let mut out = Vec::new();
    crate::reads::input(&mut t, |t| t.key(e), |b| out.extend_from_slice(b));
    out
}

/// The event `Responder::key` and then `text`/`typed` build for `key`,
/// taking the input manager to insert the key's own characters, on the
/// US layout rather than the one this Mac has selected, which the rows'
/// characters are written for.
fn app_event(key: &KeyPress, flags: NSEventModifierFlags) -> Option<KeyEvent> {
    let m = held_with_locks(key, flags);
    let e = if let Some(code) = special(key) {
        pressed(code, key, m)
    } else if key.modifiers.control || key.modifiers.option {
        chord(key, m, shift_unshifted(KeyLayout::US, key))?
    } else {
        let keyed = Keyed {
            typed: key.characters.clone(),
            base: pc101(key),
            mods: m,
            unshifted: shift_unshifted(KeyLayout::US, key),
        };
        *typed_text(&key.characters, Some(&keyed)).first()?
    };
    // As `Driver::typed` stamps it from `key_kind`.
    Some(KeyEvent {
        kind: kind_of(key),
        ..e
    })
}

/// kitty's repeat for an event whose press is `press`, under `flags`: with
/// no event types (flag 2), the press; text sent as text (no flag 8), the
/// press; Enter, Tab and Backspace without flag 8, the press, as they
/// "still generate the same bytes as in legacy mode" under every modifier
/// (keyboard-protocol.rst); a bare Escape sent as itself, itself; else
/// `CSI num ; mods:2 final`.
fn kitty_repeat(press: &[u8], e: &KeyEvent, kitty: Kitty, row: &Row) -> Vec<u8> {
    let &(.., num, fin) = row;
    if let Kitty::NoEventTypes = kitty {
        return press.to_vec();
    }
    if matches!(kitty, Kitty::EventTypes)
        && (e.text.is_some() || matches!(num, 13 | 127 | 9) || press == b"\x1b")
    {
        return press.to_vec();
    }
    let mut v = 1u32;
    for (m, bit) in [
        (Modifiers::SHIFT, 1),
        (Modifiers::ALT, 2),
        (Modifiers::CTRL, 4),
        (Modifiers::CAPS_LOCK, 64),
    ] {
        if e.mods.has(m) {
            v += bit;
        }
    }
    format!("\x1b[{num};{v}:2{fin}").into_bytes()
}

/// **A held key's repeat encodes as kitty says, and DECARM reset drops
/// it**: 18 keys -- letters, shifted, under caps lock, with control; the
/// arrows under each modifier; Enter, F5, Escape, Backspace, Tab -- built
/// from the `KeyPress` AppKit hands the app, under legacy encoding and
/// kitty flags 1, 2, 3, 10 and 11, 108 rows; and each with `CSI ? 8 l`
/// fed, the repeat sends nothing while its press still sends.
#[test]
fn a_repeat_encodes_as_kitty_says_and_decarm_reset_drops_it() {
    let none = M::default();
    let shift = M {
        shift: true,
        ..none
    };
    let ctrl = M {
        control: true,
        ..none
    };
    let opt = M {
        option: true,
        ..none
    };
    let (lock, nolock) = (NSEventModifierFlags::CapsLock, NSEventModifierFlags(0));
    let keys: &[Row] = &[
        (0x00, "a", "a", none, nolock, 97, 'u'),
        (0x00, "A", "A", shift, nolock, 97, 'u'),
        (0x00, "A", "a", none, lock, 97, 'u'),
        (0x00, "\u{1}", "a", ctrl, nolock, 97, 'u'),
        (0x7E, "\u{f700}", "\u{f700}", none, nolock, 1, 'A'),
        (0x7E, "\u{f700}", "\u{f700}", shift, nolock, 1, 'A'),
        (0x7E, "\u{f700}", "\u{f700}", ctrl, nolock, 1, 'A'),
        (0x7E, "\u{f700}", "\u{f700}", opt, nolock, 1, 'A'),
        (0x7E, "\u{f700}", "\u{f700}", none, lock, 1, 'A'),
        (0x24, "\r", "\r", none, nolock, 13, 'u'),
        (0x24, "\r", "\r", shift, nolock, 13, 'u'),
        (0x24, "\r", "\r", none, lock, 13, 'u'),
        (0x60, "\u{f708}", "\u{f708}", none, nolock, 15, '~'),
        (0x60, "\u{f708}", "\u{f708}", ctrl, nolock, 15, '~'),
        (0x35, "\u{1b}", "\u{1b}", none, nolock, 27, 'u'),
        (0x33, "\u{7f}", "\u{7f}", none, nolock, 127, 'u'),
        (0x33, "\u{7f}", "\u{7f}", ctrl, nolock, 127, 'u'),
        (0x30, "\t", "\t", none, nolock, 9, 'u'),
    ];
    let mut bad = Vec::new();
    let mut rows = 0;
    for &flags in &[0u8, 1, 2, 3, 10, 11] {
        let set = match flags {
            0 => Vec::new(),
            f => format!("\x1b[={f}u").into_bytes(),
        };
        let kitty = match (flags & 2 != 0, flags & 8 != 0) {
            (false, _) => Kitty::NoEventTypes,
            (true, false) => Kitty::EventTypes,
            (true, true) => Kitty::EventTypesAllEscaped,
        };
        for row in keys {
            let &(_, chars, _, mods, lockf, ..) = row;
            let down = key_press(row, KeyEventKind::Press);
            let held = key_press(row, KeyEventKind::Repeat);
            let (Some(pe), Some(re)) = (app_event(&down, lockf), app_event(&held, lockf)) else {
                bad.push(format!("{chars:?} {mods:?}: no event"));
                continue;
            };
            assert_eq!(pe.kind, KeyEventKind::Press, "{chars:?}: a first press");
            assert_eq!(
                re.kind,
                KeyEventKind::Repeat,
                "{chars:?}: AppKit's repeat kept"
            );
            let p = core_sends(&set, pe);
            let r = core_sends(&set, re);
            let want = kitty_repeat(&p, &re, kitty, row);
            rows += 1;
            if r != want || p.is_empty() {
                bad.push(format!(
                    "flags={flags} {chars:?} {mods:?} {lockf:?}: press {p:?} repeat {r:?} want {want:?}"
                ));
            }
            let mut arm = set.clone();
            arm.extend_from_slice(b"\x1b[?8l");
            let (pa, ra) = (core_sends(&arm, pe), core_sends(&arm, re));
            if !ra.is_empty() || pa != p {
                bad.push(format!(
                    "flags={flags} {chars:?} {mods:?}: DECARM reset press {pa:?} repeat {ra:?}"
                ));
            }
        }
    }
    assert_eq!(rows, 108, "every row was built");
    assert!(bad.is_empty(), "{bad:#?}");
}
