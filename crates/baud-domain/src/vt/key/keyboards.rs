//! **xterm's keyboards other than its default one**, the ones `?1050` to
//! `?1053`, `?1060` and `?1061` select (`[[source: ctlseqs]]` 1032-1039,
//! [`Keyboard`]): each a table, compiled in, that a key it names is sent
//! from; **a key the table does not name keeps the default keyboard's
//! encoding** ([`super::ansi_key`]).
//!
//! **The order is xterm's `Input()`** (`[[source: xterm-input-c]]`): a
//! resource at 4 takes its keys first ([`super::extended`], in the caller),
//! then the VT220 and legacy keyboards' changes to the key and its
//! modifiers (1092-1271), then the keyboard's table (the `switch
//! (keyboard->type)` at 1279), then the default keyboard. At Level 1 no
//! keyboard is read: the keyboard is the VT100's ([`super::level_one_key`]),
//! "Shall neither implement additional keys" (`[[source: dec-std-070]]`
//! 070-6 §6.2.1 b). The VT52 keyboard never reaches here ([`super::vt52`]):
//! "A VT52 does not have function keys" (`[[source: ctlseqs]]`).
//!
//! **A modified Sun or HP key is the table's sequence shaped by xterm's
//! `modifyCursorKey`**, as every other key baud sends is: the function keys
//! and Insert by modifyFunctionKeys, the cursor and editing keys by
//! modifyCursorKeys, -1 dropping the modifier and 3 marking the sequence `>`
//! (input.c 1302-1313). **A modified Delete carries the parameter too**,
//! Sun `CSI 3 ; 2 z` and HP `CSI 1 ; 2 P` with Shift, where xterm sends
//! `CSI 3 z` and `ESC P`: its `allowModifierParm` gives Delete no
//! parameter while `xtermDeleteIsDEL` is true, which it is on both
//! keyboards, and modifyOtherKeys is off (input.c 212-234, 401-430,
//! 878-905), a difference recorded and open.
//! The SCO keyboard's modifiers are its own table's columns.

use super::termcap_data;
use super::*;

/// What a selected keyboard makes of a key.
pub(super) enum Table {
    /// Its table sent it: whether anything was sent.
    Sent(bool),
    /// Its table does not name it: the default keyboard sends this key with
    /// these modifiers, which the VT220 and legacy keyboards may have
    /// changed.
    Default(KeyCode, Modifiers),
}

/// The key a selected keyboard sends for `code`.
pub(super) fn key(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> Table {
    let sent = match (s.level, s.keyboard) {
        (ConformanceLevel::One, _) | (_, Keyboard::Default) => None,
        (_, Keyboard::Termcap) => termcap(o, code, mods, s),
        (_, Keyboard::Sun) => sun(o, code, mods, s),
        (_, Keyboard::Hp) => hp(o, code, mods, s),
        (_, Keyboard::Sco) => sco(o, code, mods, s),
        (_, Keyboard::Legacy | Keyboard::Vt220) => return dec(o, code, mods, s),
    };
    match sent {
        Some(sent) => Table::Sent(sent),
        None => Table::Default(code, mods),
    }
}

/// What a key is to a table.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Named {
    /// The key itself, or for a navigation key on the numeric keypad the
    /// key it duplicates.
    Key(KeyCode),
    /// The keypad's middle key with NumLock off, `5 (Begin)` (ctlseqs line
    /// 2518), which duplicates no other key.
    Begin,
}

/// **A navigation key on the numeric keypad is the key it duplicates**, the
/// default keyboard's rule (ctlseqs line 2462, "duplicate the smaller
/// cursor and scrolling keypads"), so a table that names Home names the
/// keypad's Home too. Under DECKPAM it is the keypad's own row, which the
/// Sun, HP and SCO tables leave as it is ("keypad and cursor keys are the
/// same"), and so is every other keypad key: `None`.
fn named(code: KeyCode, mods: Modifiers, s: KeyState) -> Option<Named> {
    let KeyCode::Keypad(k) = code else {
        return Some(Named::Key(code));
    };
    k.shares_row_with()?;
    if s.keypad_for(k, mods) != KeypadMode::Numeric {
        return None;
    }
    Some(k.duplicates().map_or(Named::Begin, Named::Key))
}

/// The number of F21 to F35, which baud names with the keys
/// `[[source: kitty-keyboard]]` gives no legacy form ([`PuaKey`]): their
/// keysyms, `XK_F21` 0xffd2 to `XK_F35` 0xffe0, less 0xffbd.
fn pua_function(p: PuaKey) -> Option<u8> {
    match p.keysym()? {
        k @ 0xffd2..=0xffe0 => u8::try_from(k - 0xffbd).ok(),
        _ => None,
    }
}

/// The function key a key is, by its number: F1 to F20, and F21 to F35.
fn function_number(code: KeyCode) -> Option<u8> {
    match code {
        KeyCode::Function(f) => Some(f.number()),
        KeyCode::Pua(p) => pua_function(p),
        _ => None,
    }
}

/// Whether modifyFunctionKeys -1 made a function key another.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Remap {
    No,
    Yes,
}

/// Function key `n` with `mods` as modifyFunctionKeys -1 makes it: Shift
/// adding 10 and Control 20, both then gone, as on the default keyboard
/// ([`super::function_key`]), xterm's `mfkNone` before the keyboard's
/// table reads the key (input.c 1252-1263); and whether it was remapped.
fn remapped(n: u8, mods: Modifiers, s: KeyState) -> (u8, Modifiers, Remap) {
    if !remaps(mods, s) {
        return (n, mods, Remap::No);
    }
    let up = if mods.has(Modifiers::SHIFT) { 10 } else { 0 }
        + if mods.has(Modifiers::CTRL) { 20 } else { 0 };
    let mods = mods.without(Modifiers::SHIFT.with(Modifiers::CTRL));
    (n + up, mods, Remap::Yes)
}

/// `ESC <final>`, or as `modifyCursorKey` shapes a modified one:
/// [`super::cursor_shaped`] with `ESC` for the introducer, which from 1 is
/// `CSI`, "SS3 should not have params" (input.c 785-801), so HP's Shift-Up
/// is `CSI 1 ; 2 A`.
fn escaped_key(o: &mut KeyBytes, final_byte: u8, mods: Modifiers, s: KeyState, level: ModifyValue) {
    if !carries(mods) || level == ModifyValue::DISABLED {
        o.push(ESC);
        o.push(final_byte);
        return;
    }
    let v = level.get();
    match v >= 1 {
        true => o.introducer(Introducer::Csi, s.c1),
        false => o.push(ESC),
    }
    if v >= 3 {
        o.push(b'>');
    }
    if v >= 2 {
        o.all(b"1;");
    }
    o.num(u16::from(mods.wire()));
    o.push(final_byte);
}

// ── ?1050, termcap ──────────────────────────────────────────────────────────

/// **The termcap keyboard, `?1050`**: each key of xterm's xtermcap.c
/// table that the pinned xterm-256color entry gives a string
/// (`[[source: terminfo-xterm-256color]]`, the `TERM` baud gives the
/// child) sends it, through the table `build/termcap_keys.rs` compiles from the
/// two, **whatever DECCKM, DECKPAM and DECBKM say**: the arrows
/// `\EOA`-`\EOD`, Home `\EOH`, End `\EOF`, keypad Enter `\EOM`, Backspace
/// `^H`, F13 `\E[1;2P`. xterm's `xtermcapString` sends the string the
/// host's terminfo gives its `TERM` for the key and its modifier
/// parameter, the first of its table's rows that the entry has
/// (`[[source: xterm-xtermcap-c]]` 461-511); baud's entry is compiled in,
/// so the two differ only where a host's entry differs from the pinned
/// one. A key or a modifier no row reaches keeps the default keyboard's
/// encoding, **two keys the entry names among them**: keypad Begin, its
/// `kb2`, since no row names Begin, and Shift-Tab, its `kcbt`, whose row
/// carries no modifier while a back tab carries Shift.
///
/// **A navigation key is the key it duplicates in either keypad mode**,
/// as xterm moves `KP_Home` to `KP_Begin` onto Home to Begin before it
/// reads the entry (input.c 1185-1191); the keypad's Insert and Delete are
/// not moved, and no row names them or Begin. modifyFunctionKeys -1
/// remaps a function key before it is read, as there (1252-1263).
fn termcap(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> Option<bool> {
    let code = match code {
        KeyCode::Keypad(KeypadKey::Insert | KeypadKey::Delete) => code,
        KeyCode::Keypad(k) => k.duplicates().unwrap_or(code),
        _ => code,
    };
    let (code, mods) = match function_number(code) {
        Some(n) => {
            let (n, mods, _) = remapped(n, mods, s);
            (function_code(n)?, mods)
        }
        None => (code, mods),
    };
    let param = if carries(mods) { mods.wire() } else { 0 };
    let bytes = termcap_data::string(code, param)?;
    o.all(bytes);
    Some(true)
}

/// Function key `n` as a key: F1 to F20 and the keys named F21 to F35.
fn function_code(n: u8) -> Option<KeyCode> {
    if let Some(f) = FnKey::new(n) {
        return Some(KeyCode::Function(f));
    }
    const PUA: [PuaKey; 15] = [
        PuaKey::F21,
        PuaKey::F22,
        PuaKey::F23,
        PuaKey::F24,
        PuaKey::F25,
        PuaKey::F26,
        PuaKey::F27,
        PuaKey::F28,
        PuaKey::F29,
        PuaKey::F30,
        PuaKey::F31,
        PuaKey::F32,
        PuaKey::F33,
        PuaKey::F34,
        PuaKey::F35,
    ];
    PUA.get(usize::from(n).checked_sub(21)?)
        .map(|p| KeyCode::Pua(*p))
}

// ── ?1051, Sun ──────────────────────────────────────────────────────────────

/// **The Sun keyboard, `?1051`**: "Only function keys are altered; keypad
/// and cursor keys are the same" (`[[source: ctlseqs]]` 2741-2751), and
/// the editing keys of its Non-Function Keys table's SUN column (2759-2793):
/// Insert `CSI 2 z`, Delete `CSI 3 z`, Prior `CSI 216 z`, Next `CSI 222 z`,
/// Home `CSI 214 z`, End `CSI 220 z`, Begin `CSI 218 z`, Menu `CSI 197 z`.
/// Find, Select and Help have no key here. The function keys are the
/// xterm-sun entry's (`[[source: xterm-terminfo]]` 1510-1555, `kf1=\E[224z`
/// ...) and xterm's `sunfuncvalue` (`[[source: xterm-input-c]]` 1811-1905).
///
/// **The arrows follow DECCKM**, ctlseqs' "cursor keys are the same", which
/// the entry's application arrows under its `smkx` agree with; xterm's
/// `sunfuncvalue` sends `SS3` whatever DECCKM says, a departure of xterm's
/// not followed. **Delete is `CSI 3 z` whatever `?1037` says**, the table
/// taking the key first, as in xterm.
fn sun(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> Option<bool> {
    let (number, class) = match named(code, mods, s)? {
        Named::Key(KeyCode::Pua(PuaKey::Menu)) => (197, KeyResource::Function),
        Named::Key(k @ (KeyCode::Function(_) | KeyCode::Pua(_))) => {
            return Some(sun_function(o, function_number(k)?, mods, s));
        }
        Named::Key(KeyCode::Insert) => (2, KeyResource::Function),
        Named::Key(KeyCode::Delete) => (3, KeyResource::Cursor),
        Named::Key(KeyCode::PageUp) => (216, KeyResource::Cursor),
        Named::Key(KeyCode::PageDown) => (222, KeyResource::Cursor),
        Named::Key(KeyCode::Home) => (214, KeyResource::Cursor),
        Named::Key(KeyCode::End) => (220, KeyResource::Cursor),
        Named::Begin => (218, KeyResource::Cursor),
        Named::Key(_) => return None,
    };
    numbered_key(o, Param(number), b'z', mods, s, s.keys.modify(class));
    Some(true)
}

/// Function key `n` on the Sun keyboard, after modifyFunctionKeys -1's
/// remap; one it makes past F37, which the Sun table does not number, is
/// the default keyboard's ([`remapped_function_key`]).
fn sun_function(o: &mut KeyBytes, n: u8, mods: Modifiers, s: KeyState) -> bool {
    let (n, mods, _) = remapped(n, mods, s);
    match sun_number(n) {
        Some(number) => {
            let level = s.keys.modify(KeyResource::Function);
            numbered_key(o, Param(number), b'z', mods, s, level);
            true
        }
        None => remapped_function_key(o, n, s),
    }
}

/// The Sun keyboard's number for function key `n`: F1-F10 224-233, F11-F20
/// 192-201, F21-F35 208-222 and F36-F37 234-235, xterm's `sunfuncvalue`
/// and the xterm-sun entry's `kf1`-`kf20` and `kf31`-`kf47`.
fn sun_number(n: u8) -> Option<u16> {
    let n = u16::from(n);
    Some(match n {
        1..=10 => 223 + n,
        11..=20 => 181 + n,
        21..=35 => 187 + n,
        36 => 234,
        37 => 235,
        _ => return None,
    })
}

// ── ?1052, HP ───────────────────────────────────────────────────────────────

/// **The HP keyboard, `?1052`**: ctlseqs' HP column (2759-2793), the
/// xterm-hp entry (`[[source: xterm-terminfo]]` 1577-1598) and xterm's
/// `hpfuncvalue` (input.c 1664-1719), which agree: F1-F8 `ESC p` to
/// `ESC w`, the arrows `ESC A` to `ESC D` whatever DECCKM says, Home
/// `ESC h`, End `ESC F`, Delete `ESC P`, Insert `ESC Q`, Page Down `ESC S`
/// and Page Up `ESC T`. The HP 2392A's own table (`[[source: hp-2392a-rm]]`
/// Appendix A) gives the same codes for f1-f8, the arrows, home up (`Ec h`,
/// A-3) and Delete character, and for Insert character on its first press,
/// `Ec Q`, Start insert character mode (A-2). The 2392A's [Insert char] is
/// also listed as `Ec R`, End insert character (A-2), so the key toggles,
/// where baud sends `ESC Q` every time. **It gives seven keys other
/// codes**: [Next] `Ec U` and [Prev] `Ec V` (A-2), as ncurses' hpterm entry
/// does (`[[source: terminfo-hpterm]]`), where baud sends `ESC S` and
/// `ESC T`; Shift-Up `Ec S` and Shift-Down `Ec T`, Roll up and Roll down
/// (A-2), where baud sends `CSI 1 ; 2 A` and `CSI 1 ; 2 B`; Shift-Home
/// `Ec F`, home down (A-1), where baud sends `CSI 1 ; 2 h` and gives
/// `ESC F` to End; Shift-Tab `Ec i`, Backtab (A-3), where baud sends
/// `CSI Z`; and Tab `Ec I`, Horizontal tab (A-1), where baud sends HT. A
/// conflict between the texts, recorded, open, and left as ctlseqs has it.
/// A modified key is shaped as on the Sun keyboard, from `ESC`. Clear,
/// `ESC J`, has no key here; F9 up are the default keyboard's.
fn hp(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> Option<bool> {
    let (letter, class) = match named(code, mods, s)? {
        Named::Key(KeyCode::Function(f)) => return hp_function(o, f.number(), mods, s),
        Named::Key(KeyCode::Up) => (b'A', KeyResource::Cursor),
        Named::Key(KeyCode::Down) => (b'B', KeyResource::Cursor),
        Named::Key(KeyCode::Right) => (b'C', KeyResource::Cursor),
        Named::Key(KeyCode::Left) => (b'D', KeyResource::Cursor),
        Named::Key(KeyCode::End) => (b'F', KeyResource::Cursor),
        Named::Key(KeyCode::Delete) => (b'P', KeyResource::Cursor),
        Named::Key(KeyCode::Insert) => (b'Q', KeyResource::Function),
        Named::Key(KeyCode::PageDown) => (b'S', KeyResource::Cursor),
        Named::Key(KeyCode::PageUp) => (b'T', KeyResource::Cursor),
        Named::Key(KeyCode::Home) => (b'h', KeyResource::Cursor),
        _ => return None,
    };
    escaped_key(o, letter, mods, s, s.keys.modify(class));
    Some(true)
}

/// F1-F8 on the HP keyboard, after modifyFunctionKeys -1's remap; any
/// other function key the default keyboard's.
fn hp_function(o: &mut KeyBytes, n: u8, mods: Modifiers, s: KeyState) -> Option<bool> {
    let (n, mods, remap) = remapped(n, mods, s);
    match (n, remap) {
        (1..=8, _) => {
            let level = s.keys.modify(KeyResource::Function);
            escaped_key(o, b'o' + n, mods, s, level);
            Some(true)
        }
        (_, Remap::Yes) => Some(remapped_function_key(o, n, s)),
        (_, Remap::No) => None,
    }
}

// ── ?1053, SCO ──────────────────────────────────────────────────────────────

/// The SCO Console's function keys F1 to F12, each the final of its `CSI`
/// in the four columns Unmodified, Shift, Ctrl and Ctrl/Shift: the VT520's
/// Table 6-3 (`[[source: vt520-rm]]` pp. 6-2 and 6-3), as printed, which
/// the VT510's Table 6-3 is too (`[[source: vt510-rm-b]]`). **Two cells
/// differ from xterm's `scofuncvalue` and the xterm-sco entry**: Ctrl/Shift
/// F7 is `<` where xterm has `\`, and F11 `'` (printed `’`, the typeset
/// apostrophe) where xterm has `` ` ``. Ctrl/Shift F8 is `]`, printed
/// `ESC[ ]` with a space no other cell has.
const SCO_FUNCTION: [[u8; 4]; 12] = [
    *b"MYkw", *b"NZlx", *b"Oamy", *b"Pbnz", *b"Qco@", *b"Rdp[", *b"Seq<", *b"Tfr]", *b"Ugs^",
    *b"Vht_", *b"Wiu'", *b"Xjv{",
];

/// **The SCO keyboard, `?1053`**: the VT520's SCO Console keys on a PC
/// keyboard (`[[source: vt520-rm]]` pp. 6-2 to 6-6), which the VT510 has
/// too and which ctlseqs points to, "the VT510 and VT520 reference manuals
/// do document a related feature ... its emulation of the SCO console"
/// (2602-2609). The keys, not the emulation: its 25 lines, WYSE wrap,
/// `CSI Pn z` and `ESC Q` are not part of the mode, as xterm's `?1053`
/// changes keys alone.
///
/// F1-F12 by Table 6-3's four columns ([`SCO_FUNCTION`]): Shift-F1 is
/// `CSI Y`, Ctrl-F1 `CSI k`; F13-F20 "the same codes as F13 through F20 in
/// VT mode, except Do sends an ESC code" (p. 6-5), so F16, Do, is `ESC`;
/// the editing keypad's Table 6-7, Insert `CSI L`, Home `CSI H`, Page Up
/// `CSI I`, Delete DEL, End `CSI F`, Page Down `CSI G`, and the cursor
/// keypad's Table 6-8, `CSI A` to `CSI D`, the same in every column; and
/// the numeric keypad with Num Lock off, Table 6-5's 7 `CSI H` to 0 `CSI L`,
/// 5 `CSI E` and `.` DEL, which are the keys each duplicates. "Alt has no
/// effect on the key pressed unless otherwise indicated", so Alt and Meta
/// add nothing, and neither do modifyFunctionKeys and modifyCursorKeys,
/// which no column reads. F21 and up, which no table names, send nothing,
/// as on the default keyboard. **Cells of three of the VT520's tables are
/// not followed, open departures**: Table 6-4's Backspace BS and
/// Ctrl+Return LF, the main keypad being the default keyboard's, as
/// xterm's is; Table 6-5's Enter, LF with Ctrl and with Ctrl/Shift, where
/// the rest of that table is built; and Table 6-6, the keypad with Num
/// Lock on, which is not built.
fn sco(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> Option<bool> {
    let letter = match named(code, mods, s)? {
        Named::Key(KeyCode::Function(f)) => match f.number() {
            n @ 1..=12 => {
                let column = usize::from(mods.has(Modifiers::SHIFT))
                    + 2 * usize::from(mods.has(Modifiers::CTRL));
                SCO_FUNCTION[usize::from(n - 1)][column]
            }
            16 => {
                o.push(ESC);
                return Some(true);
            }
            _ => {
                tilde_key(o, f.tilde_param()?, Modifiers::NONE, s, ModifyValue::PARAM);
                return Some(true);
            }
        },
        Named::Key(KeyCode::Delete) => {
            o.push(0x7f);
            return Some(true);
        }
        Named::Key(KeyCode::Insert) => b'L',
        Named::Key(KeyCode::Home) => b'H',
        Named::Key(KeyCode::PageUp) => b'I',
        Named::Key(KeyCode::End) => b'F',
        Named::Key(KeyCode::PageDown) => b'G',
        Named::Key(KeyCode::Up) => b'A',
        Named::Key(KeyCode::Down) => b'B',
        Named::Key(KeyCode::Right) => b'C',
        Named::Key(KeyCode::Left) => b'D',
        Named::Begin => b'E',
        Named::Key(_) => return None,
    };
    o.introducer(Introducer::Csi, s.c1);
    o.push(letter);
    Some(true)
}

// ── ?1060 and ?1061, legacy and VT220 ───────────────────────────────────────

/// **The legacy and VT220 keyboards, `?1060` and `?1061`**: the default
/// keyboard's tables with xterm's changes to the key and its modifiers
/// (input.c 1092-1271, 1325-1350), which both make:
///
/// * **a modifier parameter only where modifyKeyboard allows it**
///   ([`permitted`]);
/// * **Control adds 10 to a function key**, then is gone: Ctrl-F1 is F11,
///   `CSI 23 ~`, and Ctrl-F3 F13, `CSI 25 ~`, xterm's `ctrlFKeys` at its
///   default (`[[source: xterm-man]]`: "The default is "10", which means
///   that CTRL F1 generates the key symbol for F11"); ctlseqs' "access
///   these keys by Control/F1 for F13" disagrees, and is not followed.
///   modifyFunctionKeys -1 makes no other remap here, as xterm's `mfkNone`
///   branch is skipped on these two keyboards.
///
/// The legacy keyboard, X11R6's: **F1-F4 are `CSI 11 ~` to `CSI 14 ~`**,
/// the forms ctlseqs calls deprecated, "since they do not correspond to any
/// hardware terminal", sent only once a program selects them; and
/// **Delete is DEL** until `?1037` is reset, xterm's `xtermDeleteIsDEL`
/// (input.c 878-905, [`Keyboard::delete_is_del`]).
///
/// The VT220 keyboard, xterm's `sunKeyboard`, "to force a Sun/PC keyboard
/// to act like a VT220 keyboard" (ctlseqs 2618-2696): **Home is Find,
/// `CSI 1 ~`, End Select, `CSI 4 ~`**, and Delete Remove, `CSI 3 ~`, until
/// `?1037` is set, the 6-key editing keypad, "not affected by DECCKM";
/// **the keypad's `+` is its `,`, and Control with `,` is `-`**, neither
/// while Shift is held, as xterm maps them (1092-1112) for EDT's
/// delete-character on a keypad with no comma; **a navigation key is its digit**, a VT220 keypad having no
/// other, xterm's `TranslateFromSUNPC` (813-852); and Shift with a
/// function key is the key's own sequence, the user-defined key xterm
/// sends in its place (`udk_lookup`, 1325-1333) not being built.
fn dec(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> Table {
    let vt220 = s.keyboard == Keyboard::Vt220;
    let (code, mods) = match (vt220, code) {
        (true, KeyCode::Keypad(k)) => vt220_keypad(k, mods),
        _ => (code, mods),
    };
    // Control makes a function key another before modifyKeyboard is read,
    // xterm's remap reading the event's state and not the parameter.
    if let KeyCode::Function(f) = code {
        let (n, mods) = match mods.has(Modifiers::CTRL) {
            true => (f.number() + 10, mods.without(Modifiers::CTRL)),
            false => (f.number(), mods),
        };
        return Table::Sent(dec_function(o, n, permitted(code, mods, s), s));
    }
    let mods = permitted(code, mods, s);
    match (code, vt220) {
        (KeyCode::Home, true) => {
            tilde_key(o, Param(1), mods, s, ModifyValue::PARAM);
            Table::Sent(true)
        }
        (KeyCode::End, true) => {
            tilde_key(o, Param(4), mods, s, ModifyValue::PARAM);
            Table::Sent(true)
        }
        (KeyCode::Delete, _) if s.keyboard.delete_is_del(s.delete) && !carries(mods) => {
            o.push(0x7f);
            Table::Sent(true)
        }
        (KeyCode::Delete, _) => {
            tilde_key(o, Param(3), mods, s, ModifyValue::PARAM);
            Table::Sent(true)
        }
        _ => Table::Default(code, mods),
    }
}

/// The VT220 keyboard's keypad: `+` is `,` (`KP_Separator`), and Control
/// with `,` -- the comma so made among them -- is `-`, Control then gone,
/// both only while Shift is not held, as xterm guards them (input.c
/// 1097-1112), so Shift keeps `+` and Shift with Control keeps `,`; a
/// navigation key is the digit on its key, `.` for Delete.
fn vt220_keypad(k: KeypadKey, mods: Modifiers) -> (KeyCode, Modifiers) {
    let shift = mods.has(Modifiers::SHIFT);
    let k = match (k, shift) {
        (KeypadKey::Add, false) => KeypadKey::Comma,
        _ => k.shares_row_with().unwrap_or(k),
    };
    match (k, mods.has(Modifiers::CTRL) && !shift) {
        (KeypadKey::Comma, true) => (
            KeyCode::Keypad(KeypadKey::Minus),
            mods.without(Modifiers::CTRL),
        ),
        _ => (KeyCode::Keypad(k), mods),
    }
}

/// **modifyKeyboard's categories** (`[[source: xterm-man]]`): on the legacy
/// and VT220 keyboards a modifier is sent as a parameter only for a key in
/// a category whose bit it sets -- 1 the numeric keypad, 2 the editing
/// keypad and the cursor keys, 4 the function keys, 8 the other special
/// keys -- and at 0, the first value, for none, "The legacy/vt220
/// keyboards interpret only the Control-modifier when constructing
/// numbered function-keys". xterm's `allowModifierParm` with `LegacyAllows`
/// (input.c 384-428), which allows every key while modifyOtherKeys is on.
/// Delete sending DEL is in no category, as xterm's `IsEditKeypad` leaves it
/// out. Every other key keeps its modifiers.
fn permitted(code: KeyCode, mods: Modifiers, s: KeyState) -> Modifiers {
    let bit = match code {
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Insert => 2,
        KeyCode::Delete if !s.keyboard.delete_is_del(s.delete) => 2,
        KeyCode::Delete => 0,
        KeyCode::Keypad(_) => 1,
        KeyCode::Function(_) => 4,
        KeyCode::Pua(p) if pua_function(p).is_some() => 4,
        KeyCode::Pua(PuaKey::Menu) => 8,
        _ => return mods,
    };
    let allow = s.keys.modify(KeyResource::Keyboard).get();
    match s.keys.other_keys() != OtherKeys::Off || allow & bit != 0 {
        true => mods,
        false => mods.without(Modifiers::ALL),
    }
}

/// Function key `n` on the legacy or VT220 keyboard, Control's 10 already
/// added: the default keyboard's form for it -- on the legacy keyboard
/// F1-F4 the deprecated `CSI 11 ~` to `CSI 14 ~` -- and past F20 the number
/// xterm's `decfuncvalue` gives, `42 + n - 21`.
fn dec_function(o: &mut KeyBytes, n: u8, mods: Modifiers, s: KeyState) -> bool {
    let level = s.keys.modify(KeyResource::Function);
    let Some(f) = FnKey::new(n) else {
        tilde_key(o, Param(42 + u16::from(n) - 21), mods, s, level);
        return true;
    };
    match (f.ss3_final(), s.keyboard) {
        (Some(_), Keyboard::Legacy) => tilde_key(o, Param(10 + u16::from(n)), mods, s, level),
        (Some(final_byte), _) => cursor_shaped(o, Introducer::Ss3, final_byte, mods, s, level),
        (None, _) => match f.tilde_param() {
            Some(p) => tilde_key(o, p, mods, s, level),
            None => return false,
        },
    }
    true
}
