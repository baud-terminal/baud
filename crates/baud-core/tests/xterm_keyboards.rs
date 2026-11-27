//! xterm's six keyboards, `?1050` to `?1053`, `?1060` and `?1061`
//! (`[[source: ctlseqs]]` 1032-1039): one in force at a time, each a table
//! a key it names is sent from, a key it does not name the default
//! keyboard's.
//!
//! The tables are held to the texts each comes from, read here rather than
//! copied: the termcap keyboard to the pinned xterm-256color entry
//! (`[[source: terminfo-xterm-256color]]`), the Sun, HP and SCO keyboards
//! to xterm's own entries for them (`[[source: xterm-terminfo]]`) where the
//! DEC and HP texts agree with those, and to the VT520's and the HP
//! 2392A's tables where they do not.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use std::collections::{BTreeMap, BTreeSet};

fn keys(t: &mut Term<'_>, code: KeyCode, mods: Modifiers) -> Vec<u8> {
    t.clear_reply();
    t.key(KeyEvent::press(code, mods));
    t.reply().to_vec()
}

fn reply(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, bytes);
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

fn show(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

fn f(n: u8) -> KeyCode {
    KeyCode::Function(FnKey::new(n).expect("F1-F20"))
}

/// F21 to F35, the keys baud names with no legacy form.
fn pua_f(n: u8) -> KeyCode {
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
    KeyCode::Pua(PUA[usize::from(n - 21)])
}

fn pad(k: KeypadKey) -> KeyCode {
    KeyCode::Keypad(k)
}

const SHIFT: Modifiers = Modifiers::SHIFT;
const CTRL: Modifiers = Modifiers::CTRL;
const ALT: Modifiers = Modifiers::ALT;
const NONE: Modifiers = Modifiers::NONE;

/// DECRQM's answer for private mode `m`.
fn decrqm(t: &mut Term<'_>, m: u16) -> String {
    show(&reply(t, format!("\x1b[?{m}$p").as_bytes()))
}

/// The six modes' answers, in wire order.
fn family(t: &mut Term<'_>) -> Vec<String> {
    [1050, 1051, 1052, 1053, 1060, 1061]
        .into_iter()
        .map(|m| decrqm(t, m))
        .collect()
}

/// The answers with `on` set and the other five reset.
fn only(on: Option<u16>) -> Vec<String> {
    [1050, 1051, 1052, 1053, 1060, 1061]
        .into_iter()
        .map(|m| {
            let v = if Some(m) == on { 1 } else { 2 };
            format!("\\x1b[?{m};{v}$y")
        })
        .collect()
}

// ── the terminal descriptions ──────────────────────────────────────────────

/// terminfo(5)'s escapes in a key's string.
fn unescape(v: &str) -> Vec<u8> {
    let b = v.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                out.push(match b[i + 1] {
                    b'E' | b'e' => 0x1b,
                    b'\\' => b'\\',
                    b'^' => b'^',
                    b',' => b',',
                    b':' => b':',
                    b's' => b' ',
                    // `\ddd`, three octal digits: two more than the
                    // other escapes take.
                    b'0'..=b'7' => {
                        let oct = std::str::from_utf8(&b[i + 1..i + 4]).expect("octal");
                        i += 2;
                        u8::from_str_radix(oct, 8).expect("three octal digits")
                    }
                    c => panic!("an escape the keys do not use: \\{}", c as char),
                });
                i += 2;
            }
            b'^' => {
                out.push(if b[i + 1] == b'?' {
                    0x7f
                } else {
                    b[i + 1] & 0x1f
                });
                i += 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// A terminal description's key capabilities, by name.
type Caps = BTreeMap<String, Vec<u8>>;

/// Every entry of a terminfo source: its own key capabilities and the
/// fragments it `use=`s, by its first name.
type Entries = BTreeMap<String, (Caps, Vec<String>)>;

/// Every entry of a terminfo source.
fn entries(text: &str) -> Entries {
    let mut out = BTreeMap::new();
    let mut name = None;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            let n = line.split('|').next().unwrap_or("").trim_end_matches(',');
            name = Some(n.to_string());
            out.entry(n.to_string())
                .or_insert_with(|| (BTreeMap::new(), Vec::new()));
            continue;
        }
        let Some(n) = &name else { continue };
        let cap = line.trim().trim_end_matches(',');
        if let Some(u) = cap.strip_prefix("use=") {
            out.get_mut(n).expect("entry").1.push(u.to_string());
        } else if let Some((k, v)) = cap.split_once('=') {
            // The keys' capabilities alone: theirs are the escapes
            // `unescape` reads.
            if k.starts_with('k') && k.chars().all(|c| c.is_ascii_alphanumeric()) {
                out.get_mut(n)
                    .expect("entry")
                    .0
                    .entry(k.to_string())
                    .or_insert_with(|| unescape(v));
            }
        }
    }
    out
}

/// An entry's string capabilities with its `use=` fragments resolved, the
/// entry's own first, as terminfo(5) has a capability's first definition
/// win.
fn resolved(all: &Entries, name: &str) -> Caps {
    let (own, uses) = all.get(name).unwrap_or_else(|| panic!("no entry {name}"));
    let mut caps = own.clone();
    for u in uses {
        for (k, v) in resolved(all, u) {
            caps.entry(k).or_insert(v);
        }
    }
    caps
}

fn source(path: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../../docs/research/sources/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("a pinned source")
}

/// xterm's entry `name`, from its own terminfo source.
fn xterm_entry(name: &str) -> BTreeMap<String, Vec<u8>> {
    resolved(&entries(&source("xterm-terminfo")), name)
}

/// The capabilities xterm's entry `name` writes itself, without the
/// fragments it `use=`s.
fn xterm_own(name: &str) -> BTreeMap<String, Vec<u8>> {
    entries(&source("xterm-terminfo"))
        .remove(name)
        .unwrap_or_else(|| panic!("no entry {name}"))
        .0
}

/// Each capability of `entry` the list maps to a key, against what the key
/// sends: the mismatches, and how many were compared.
fn against(
    t: &mut Term<'_>,
    entry: &BTreeMap<String, Vec<u8>>,
    list: &[(&str, KeyCode, Modifiers)],
) -> (Vec<String>, usize) {
    let mut wrong = Vec::new();
    let mut n = 0;
    for (cap, code, mods) in list {
        let Some(want) = entry.get(*cap) else {
            continue;
        };
        n += 1;
        let got = keys(t, *code, *mods);
        if &got != want {
            wrong.push(format!("{cap}: sent {}, entry {}", show(&got), show(want)));
        }
    }
    (wrong, n)
}

// ── the family ─────────────────────────────────────────────────────────────

/// **One keyboard at a time.** DECSET of a member selects it and DECRQM
/// answers set for it alone; DECRST of any member, the one in force or
/// not, returns to the default keyboard, as xterm's `set_keyboard_type`
/// stores `keyboardIsDefault` for any reset. None is counted as
/// unsupported.
#[test]
fn the_keyboards_are_one_family_and_any_reset_returns_to_the_default() {
    support::with_corpus(|t| {
        assert_eq!(family(t), only(None), "the default at power-on");
        for m in [1050u16, 1051, 1052, 1053, 1060, 1061] {
            support::feed(t, format!("\x1b[?{m}h").as_bytes());
            assert_eq!(family(t), only(Some(m)), "?{m} set");
            let other = if m == 1050 { 1061 } else { 1050 };
            support::feed(t, format!("\x1b[?{other}l").as_bytes());
            assert_eq!(family(t), only(None), "?{other} reset under ?{m}");
        }
        support::feed(t, b"\x1b[?1051h\x1b[?1052h");
        assert_eq!(family(t), only(Some(1052)), "the last set wins");
        assert_eq!(t.unsupported_param, 0);
    });
}

/// **RIS returns to the default keyboard and DECSTR keeps it**: DECSTR's
/// table names no keyboard, and xterm resets it in its full reset alone.
#[test]
fn ris_returns_to_the_default_keyboard_and_decstr_keeps_it() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1053h\x1b[!p");
        assert_eq!(family(t), only(Some(1053)), "DECSTR keeps it");
        assert_eq!(keys(t, f(1), NONE), b"\x1b[M");
        support::feed(t, b"\x1bc");
        assert_eq!(family(t), only(None), "RIS");
        assert_eq!(keys(t, f(1), NONE), b"\x1bOP");
    });
}

/// **XTSAVE keeps the family as one value**, whichever member is listed,
/// and XTRESTORE of any member brings back the keyboard saved, the default
/// one included; a family never saved restores the power-on keyboard, the
/// default one, as `RIS` does.
#[test]
fn xtsave_keeps_the_keyboard_family_as_one_value() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1051h\x1b[?1052r");
        assert_eq!(family(t), only(None), "never saved: the default keyboard");
        support::feed(t, b"\x1b[?1051h");
        support::feed(t, b"\x1b[?1060s\x1b[?1060h\x1b[?1050r");
        assert_eq!(
            family(t),
            only(Some(1051)),
            "saved under ?1060, restored under ?1050"
        );
        support::feed(t, b"\x1b[?1061l\x1b[?1052s\x1b[?1061h\x1b[?1061r");
        assert_eq!(family(t), only(None), "the default keyboard comes back");
        support::feed(t, b"\x1b[?1053h\x1b[?1053s\x1b[?1053l\x1b[?1051r");
        assert_eq!(family(t), only(Some(1053)));
        assert_eq!(t.unsupported_param, 0);
    });
}

/// **DECRSTS brings the keyboard back** from the terminal state report.
#[test]
fn decrsts_restores_the_keyboard() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1052h");
        let report = reply(t, b"\x1b[1$u");
        let data = report
            .strip_prefix(b"\x1bP1$s")
            .and_then(|r| r.strip_suffix(b"\x1b\\"))
            .expect("a DECTSR");
        support::feed(t, b"\x1b[?1060h");
        support::feed(t, &[b"\x1bP1$p".as_slice(), data, b"\x1b\\"].concat());
        assert_eq!(family(t), only(Some(1052)), "from ?1060");
        support::feed(t, b"\x1bc");
        support::feed(t, &[b"\x1bP1$p".as_slice(), data, b"\x1b\\"].concat());
        assert_eq!(family(t), only(Some(1052)), "from the default");
    });
}

/// **XTSAVE and DECTSR keep `?1037` as DECRQM answers it**, so a restore
/// brings back set or reset and never the unset state: a set `?1037`
/// under the Sun keyboard, whose Delete is its own, is kept as reset, and
/// a reset one under the SCO keyboard, whose Delete is DEL, as set; xterm
/// keeps its three states.
#[test]
fn xtsave_and_dectsr_keep_1037_as_decrqm_answers_it() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1037s\x1b[?1037h\x1b[?1037r\x1b[?1060h");
        assert_eq!(
            keys(t, KeyCode::Delete, NONE),
            b"\x1b[3~",
            "the unset state was saved as reset"
        );
        support::feed(t, b"\x1bc\x1b[?1037h\x1b[?1051h");
        assert_eq!(decrqm(t, 1037), "\\x1b[?1037;2$y");
        let report = reply(t, b"\x1b[1$u");
        let data = report
            .strip_prefix(b"\x1bP1$s")
            .and_then(|r| r.strip_suffix(b"\x1b\\"))
            .expect("a DECTSR");
        support::feed(t, b"\x1b[?1051l");
        assert_eq!(keys(t, KeyCode::Delete, NONE), b"\x7f");
        support::feed(t, &[b"\x1bP1$p".as_slice(), data, b"\x1b\\"].concat());
        assert_eq!(family(t), only(Some(1051)));
        support::feed(t, b"\x1b[?1051l");
        assert_eq!(
            keys(t, KeyCode::Delete, NONE),
            b"\x1b[3~",
            "the set ?1037 was saved as reset"
        );
        support::feed(t, b"\x1bc\x1b[?1037l\x1b[?1053h\x1b[?1037s");
        support::feed(t, b"\x1b[?1053l\x1b[?1037r");
        assert_eq!(
            keys(t, KeyCode::Delete, NONE),
            b"\x7f",
            "the reset ?1037 was saved as set under SCO"
        );
    });
}

/// **Level 1 keeps the VT100 keyboard and VT52 mode the VT52's**, the
/// mode still recorded: a selected keyboard acts in ANSI mode at Levels 2
/// to 4.
#[test]
fn level_one_and_vt52_keep_their_own_keyboards() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1051h\x1b[61\"p");
        assert_eq!(keys(t, f(1), NONE), b"\x1bOP", "PF1 at Level 1");
        assert_eq!(keys(t, f(5), NONE), b"", "F5 inoperative at Level 1");
        support::feed(t, b"\x1b[64;1\"p");
        assert_eq!(decrqm(t, 1051), "\\x1b[?1051;1$y", "still recorded");
        support::feed(t, b"\x1b[?2l");
        assert_eq!(keys(t, f(1), NONE), b"", "no function keys on a VT52");
        assert_eq!(keys(t, KeyCode::Up, NONE), b"\x1bA");
        support::feed(t, b"\x1b<\x1b[64;1\"p");
        assert_eq!(keys(t, f(1), NONE), b"\x1b[224z", "back in ANSI at Level 4");
    });
}

// ── ?1051, Sun ─────────────────────────────────────────────────────────────

/// **The Sun keyboard sends the xterm-sun entry's keys**, ctlseqs' "See
/// the xterm-sun terminfo entry for details": F1-F20, the R keys the entry
/// numbers `kf31` to `kf45` (X's F21 to F35, its comment says), and Home,
/// End, Insert, Delete, Page Up, Page Down and the keypad's Begin -- the
/// keys the entry writes itself. Where it leaves one out to a fragment, the
/// key is its own: F16 is the entry's `kcpy`, `CSI 197 z`, as xterm's
/// `sunfuncvalue` numbers it.
#[test]
fn the_sun_keyboard_sends_the_xterm_sun_entrys_keys() {
    let sun = xterm_own("xterm-sun");
    assert_eq!(
        sun.get("kf1").map(Vec::as_slice),
        Some(b"\x1b[224z".as_slice())
    );
    let mut list = vec![
        ("khome", KeyCode::Home, NONE),
        ("kend", KeyCode::End, NONE),
        ("kich1", KeyCode::Insert, NONE),
        ("kdch1", KeyCode::Delete, NONE),
        ("kpp", KeyCode::PageUp, NONE),
        ("knp", KeyCode::PageDown, NONE),
        ("kb2", pad(KeypadKey::Begin), NONE),
    ];
    let names: Vec<String> = (1..=45).map(|n| format!("kf{n}")).collect();
    for n in 1..=20u8 {
        list.push((names[usize::from(n - 1)].as_str(), f(n), NONE));
    }
    for n in 31..=45u8 {
        list.push((names[usize::from(n - 1)].as_str(), pua_f(n - 10), NONE));
    }
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1051h");
        let (wrong, n) = against(t, &sun, &list);
        assert!(wrong.is_empty(), "{wrong:#?}");
        assert_eq!(n, 37, "every key of the entry's that baud has");
        assert_eq!(
            keys(t, KeyCode::Pua(PuaKey::Menu), NONE),
            b"\x1b[197z",
            "ctlseqs' Menu"
        );
        assert_eq!(Some(&keys(t, f(16), NONE)), sun.get("kcpy"), "F16");
    });
}

/// **"Keypad and cursor keys are the same"**: the arrows follow DECCKM, as
/// the entry's application arrows under its `smkx` do, and the keypad is
/// the default keyboard's, a navigation key under DECKPAM its own row and
/// under DECKPNM the key it duplicates, which the table names.
#[test]
fn the_sun_keyboard_leaves_the_arrows_and_the_keypad() {
    let sun = xterm_entry("xterm-sun");
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1051h");
        assert_eq!(keys(t, KeyCode::Up, NONE), b"\x1b[A");
        support::feed(t, b"\x1b[?1h\x1b=");
        let (wrong, n) = against(
            t,
            &sun,
            &[
                ("kcuu1", KeyCode::Up, NONE),
                ("kcud1", KeyCode::Down, NONE),
                ("kcuf1", KeyCode::Right, NONE),
                ("kcub1", KeyCode::Left, NONE),
            ],
        );
        assert!(wrong.is_empty() && n == 4, "{wrong:#?}");
        assert_eq!(
            keys(t, pad(KeypadKey::Home), NONE),
            b"\x1bOH",
            "DECKPAM's row"
        );
        assert_eq!(keys(t, pad(KeypadKey::Digit1), NONE), b"\x1bOq");
        support::feed(t, b"\x1b>");
        assert_eq!(keys(t, pad(KeypadKey::Home), NONE), b"\x1b[214z");
        assert_eq!(keys(t, pad(KeypadKey::Digit1), NONE), b"1");
    });
}

/// **A modified Sun key is shaped as every other key is**: the modifier
/// second, modifyFunctionKeys -1 remapping a function key first (Shift
/// adds 10, Control 20) and 3 marking it `>`, modifyCursorKeys -1 dropping
/// it from an editing key.
#[test]
fn a_modified_sun_key_is_shaped_by_the_modify_resources() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1051h");
        assert_eq!(keys(t, f(1), SHIFT), b"\x1b[224;2z");
        assert_eq!(keys(t, KeyCode::Home, CTRL), b"\x1b[214;5z");
        assert_eq!(
            keys(t, KeyCode::Delete, SHIFT),
            b"\x1b[3;2z",
            "where xterm sends no parameter"
        );
        support::feed(t, b"\x1b[>2n");
        assert_eq!(keys(t, f(1), SHIFT), b"\x1b[192z", "F11");
        assert_eq!(keys(t, f(1), CTRL), b"\x1b[208z", "F21");
        assert_eq!(
            keys(t, f(8), SHIFT.with(CTRL)),
            b"\x1b[59~",
            "F38, past the table"
        );
        support::feed(t, b"\x1b[>2;3m");
        assert_eq!(keys(t, f(1), SHIFT), b"\x1b[>224;2z");
        support::feed(t, b"\x1b[>1n");
        assert_eq!(keys(t, KeyCode::Home, SHIFT), b"\x1b[214z");
    });
}

/// **The Sun keyboard's Delete is its own whatever `?1037` says**, and
/// DECRQM `?1037` answers what the key sends: reset, until the default
/// keyboard is back.
#[test]
fn the_sun_delete_is_its_own_and_1037_answers_what_is_sent() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1037h\x1b[?1051h");
        assert_eq!(keys(t, KeyCode::Delete, NONE), b"\x1b[3z");
        assert_eq!(decrqm(t, 1037), "\\x1b[?1037;2$y");
        support::feed(t, b"\x1b[?1051l");
        assert_eq!(keys(t, KeyCode::Delete, NONE), b"\x7f");
        assert_eq!(decrqm(t, 1037), "\\x1b[?1037;1$y");
    });
}

// ── ?1052, HP ──────────────────────────────────────────────────────────────

/// **The HP keyboard sends the xterm-hp entry's keys**, ctlseqs' HP
/// column: F1-F8, the arrows whatever DECCKM says, Home, End, Delete,
/// Insert, Page Down and Page Up.
#[test]
fn the_hp_keyboard_sends_the_xterm_hp_entrys_keys() {
    let hp = xterm_entry("xterm-hp");
    assert_eq!(hp.get("kf1").map(Vec::as_slice), Some(b"\x1bp".as_slice()));
    let names: Vec<String> = (1..=8).map(|n| format!("kf{n}")).collect();
    let mut list = vec![
        ("kcuu1", KeyCode::Up, NONE),
        ("kcud1", KeyCode::Down, NONE),
        ("kcuf1", KeyCode::Right, NONE),
        ("kcub1", KeyCode::Left, NONE),
        ("khome", KeyCode::Home, NONE),
        ("kend", KeyCode::End, NONE),
        ("kdch1", KeyCode::Delete, NONE),
        ("kich1", KeyCode::Insert, NONE),
        ("knp", KeyCode::PageDown, NONE),
        ("kpp", KeyCode::PageUp, NONE),
    ];
    for n in 1..=8u8 {
        list.push((names[usize::from(n - 1)].as_str(), f(n), NONE));
    }
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1052h");
        for ckm in [b"\x1b[?1l".as_slice(), b"\x1b[?1h"] {
            support::feed(t, ckm);
            let (wrong, n) = against(t, &hp, &list);
            assert!(wrong.is_empty(), "{wrong:#?}");
            assert_eq!(n, 18);
        }
        assert_eq!(keys(t, f(9), NONE), b"\x1b[20~", "F9, the default's");
        support::feed(t, b"\x1b=");
        assert_eq!(
            keys(t, pad(KeypadKey::Home), NONE),
            b"\x1bOH",
            "DECKPAM's row"
        );
        support::feed(t, b"\x1b>");
        assert_eq!(keys(t, pad(KeypadKey::Home), NONE), b"\x1bh");
    });
}

/// **A modified HP key is shaped as every other key is**, from `ESC`:
/// `CSI` with the modifier second from modifyCursorKeys and
/// modifyFunctionKeys 1, the introducer kept at 0, the modifier dropped at
/// -1.
#[test]
fn a_modified_hp_key_is_shaped_by_the_modify_resources() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1052h");
        assert_eq!(keys(t, KeyCode::Up, SHIFT), b"\x1b[1;2A");
        assert_eq!(keys(t, KeyCode::Down, SHIFT), b"\x1b[1;2B");
        assert_eq!(keys(t, KeyCode::Home, SHIFT), b"\x1b[1;2h");
        assert_eq!(keys(t, KeyCode::Up, CTRL), b"\x1b[1;5A");
        assert_eq!(keys(t, KeyCode::Delete, SHIFT), b"\x1b[1;2P");
        assert_eq!(keys(t, f(1), SHIFT), b"\x1b[1;2p");
        assert_eq!(
            keys(t, KeyCode::Tab, SHIFT),
            b"\x1b[Z",
            "the default's back tab"
        );
        support::feed(t, b"\x1b[>1;0m");
        assert_eq!(
            keys(t, KeyCode::Left, CTRL),
            b"\x1b5D",
            "modifyCursorKeys 0"
        );
        support::feed(t, b"\x1b[>1n");
        assert_eq!(
            keys(t, KeyCode::Left, CTRL),
            b"\x1bD",
            "modifyCursorKeys -1"
        );
    });
}

// ── ?1053, SCO ─────────────────────────────────────────────────────────────

/// The VT520's Table 6-3, SCO Console Mode Function Key Default Codes
/// (`[[source: vt520-rm]]` pp. 6-2 and 6-3): each key's final in the
/// columns Unmodified, Shift, Ctrl and Ctrl/Shift, as printed.
const VT520_TABLE_6_3: [&[u8; 4]; 12] = [
    b"MYkw", b"NZlx", b"Oamy", b"Pbnz", b"Qco@", b"Rdp[", b"Seq<", b"Tfr]", b"Ugs^", b"Vht_",
    b"Wiu'", b"Xjv{",
];

/// **F1-F12 in Table 6-3's four columns**, Alt adding nothing ("Alt has no
/// effect on the key pressed unless otherwise indicated"), and the
/// modify resources none either.
#[test]
fn the_sco_keyboard_sends_the_vt520s_function_keys() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1053h");
        let columns = [NONE, SHIFT, CTRL, SHIFT.with(CTRL)];
        for (i, row) in VT520_TABLE_6_3.iter().enumerate() {
            let n = u8::try_from(i + 1).expect("F1-F12");
            for (c, mods) in columns.iter().enumerate() {
                let want = [0x1b, b'[', row[c]];
                assert_eq!(keys(t, f(n), *mods), want, "F{n} column {c}");
                assert_eq!(
                    keys(t, f(n), mods.with(ALT)),
                    want,
                    "F{n} column {c} with Alt"
                );
            }
        }
        support::feed(t, b"\x1b[>2n");
        assert_eq!(
            keys(t, f(1), SHIFT),
            b"\x1b[Y",
            "modifyFunctionKeys -1 adds nothing"
        );
    });
}

/// **xterm-sco agrees with Table 6-3 in all but two cells**, its `kf13` to
/// `kf48` being the Shift, Ctrl and Ctrl/Shift columns: Ctrl/Shift F7 and
/// F11, which the VT520 prints `ESC[<` and `ESC[’` and xterm sends as
/// `\E[\\` and `` \E[` ``. Table 6-3 is followed.
#[test]
fn xterm_sco_differs_from_table_6_3_in_two_cells() {
    let sco = xterm_entry("xterm-sco");
    assert_eq!(
        sco.get("kf1").map(Vec::as_slice),
        Some(b"\x1b[M".as_slice())
    );
    let mut differ = Vec::new();
    for k in 1..=48u8 {
        let want = sco.get(&format!("kf{k}")).expect("kf1-kf48");
        let (row, column) = (usize::from((k - 1) % 12), usize::from((k - 1) / 12));
        let ours = [0x1b, b'[', VT520_TABLE_6_3[row][column]];
        if want.as_slice() != ours {
            differ.push(format!("kf{k}"));
        }
    }
    assert_eq!(differ, ["kf43", "kf47"]);
}

/// **The rest of the VT520's SCO keys**: F13-F20 VT mode's codes, F16 (Do)
/// `ESC` (p. 6-5); the editing keypad's Table 6-7 and the cursor keypad's
/// Table 6-8 in every column, whatever DECCKM says; and the numeric keypad
/// with Num Lock off, Table 6-5, under DECKPNM, which is xterm-sco's
/// `kbeg`, `kich1`, `knp`, `kpp`, `khome`, `kend` and `kdch1` too.
#[test]
fn the_sco_keyboard_sends_the_vt520s_other_keys() {
    let sco = xterm_entry("xterm-sco");
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1053h\x1b[?1h");
        for (n, want) in [
            (13, b"\x1b[25~".as_slice()),
            (14, b"\x1b[26~"),
            (15, b"\x1b[28~"),
            (16, b"\x1b"),
            (17, b"\x1b[31~"),
            (20, b"\x1b[34~"),
        ] {
            assert_eq!(keys(t, f(n), NONE), want, "F{n}");
            assert_eq!(keys(t, f(n), SHIFT), want, "F{n} with Shift");
        }
        for mods in [NONE, SHIFT, CTRL, SHIFT.with(CTRL)] {
            for (code, want) in [
                (KeyCode::Insert, b"\x1b[L".as_slice()),
                (KeyCode::Home, b"\x1b[H"),
                (KeyCode::PageUp, b"\x1b[I"),
                (KeyCode::Delete, b"\x7f"),
                (KeyCode::End, b"\x1b[F"),
                (KeyCode::PageDown, b"\x1b[G"),
                (KeyCode::Up, b"\x1b[A"),
                (KeyCode::Down, b"\x1b[B"),
                (KeyCode::Right, b"\x1b[C"),
                (KeyCode::Left, b"\x1b[D"),
            ] {
                assert_eq!(keys(t, code, mods), want, "{code:?} {mods:?}");
            }
        }
        let (wrong, n) = against(
            t,
            &sco,
            &[
                ("kbeg", pad(KeypadKey::Begin), NONE),
                ("kich1", pad(KeypadKey::Insert), NONE),
                ("knp", pad(KeypadKey::PageDown), NONE),
                ("kpp", pad(KeypadKey::PageUp), NONE),
                ("khome", pad(KeypadKey::Home), NONE),
                ("kend", pad(KeypadKey::End), NONE),
                ("kdch1", pad(KeypadKey::Delete), NONE),
            ],
        );
        assert!(wrong.is_empty() && n == 7, "{wrong:#?}");
        assert_eq!(decrqm(t, 1037), "\\x1b[?1037;1$y", "Delete sends DEL");
        assert_eq!(keys(t, pua_f(21), NONE), b"", "F21, which no table names");
    });
}

// ── ?1050, termcap ─────────────────────────────────────────────────────────

/// The keys of xterm's table's rows (`[[source: xterm-xtermcap-c]]`
/// 70-244) that the pinned xterm-256color entry gives a string, with their
/// modifiers.
fn termcap_keys() -> Vec<(String, KeyCode, Modifiers)> {
    let mut out: Vec<(String, KeyCode, Modifiers)> = vec![
        ("kcuu1".into(), KeyCode::Up, NONE),
        ("kcud1".into(), KeyCode::Down, NONE),
        ("kcuf1".into(), KeyCode::Right, NONE),
        ("kcub1".into(), KeyCode::Left, NONE),
        ("khome".into(), KeyCode::Home, NONE),
        ("kend".into(), KeyCode::End, NONE),
        ("kent".into(), pad(KeypadKey::Enter), NONE),
        ("kbs".into(), KeyCode::Backspace, NONE),
        ("kdch1".into(), KeyCode::Delete, NONE),
        ("kich1".into(), KeyCode::Insert, NONE),
        ("knp".into(), KeyCode::PageDown, NONE),
        ("kpp".into(), KeyCode::PageUp, NONE),
        ("kri".into(), KeyCode::Up, SHIFT),
        ("kind".into(), KeyCode::Down, SHIFT),
    ];
    for n in 1..=20u8 {
        out.push((format!("kf{n}"), f(n), NONE));
    }
    for n in 21..=35u8 {
        out.push((format!("kf{n}"), pua_f(n), NONE));
    }
    let groups = [
        ("UP", KeyCode::Up),
        ("DN", KeyCode::Down),
        ("LFT", KeyCode::Left),
        ("RIT", KeyCode::Right),
        ("HOM", KeyCode::Home),
        ("END", KeyCode::End),
        ("IC", KeyCode::Insert),
        ("DC", KeyCode::Delete),
        ("NXT", KeyCode::PageDown),
        ("PRV", KeyCode::PageUp),
    ];
    for (g, code) in groups {
        out.push((format!("k{g}"), code, SHIFT));
        for p in 3..=8u8 {
            let m = Modifiers::from_wire(p).expect("a parameter");
            out.push((format!("k{g}{p}"), code, m));
        }
    }
    out
}

/// **The termcap keyboard sends the pinned entry's strings, whatever
/// DECCKM, DECKPAM and DECBKM say**: every key and modifier of xterm's
/// table that the entry gives a string, in all eight of their states.
#[test]
fn the_termcap_keyboard_sends_the_entrys_strings() {
    let all = entries(&source("terminfo/xterm-256color.ti"));
    let entry = all.values().next().expect("the entry").0.clone();
    assert_eq!(
        entry.get("kcuu1").map(Vec::as_slice),
        Some(b"\x1bOA".as_slice())
    );
    let list = termcap_keys();
    let list: Vec<(&str, KeyCode, Modifiers)> =
        list.iter().map(|(c, k, m)| (c.as_str(), *k, *m)).collect();
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1050h");
        for state in 0..8u8 {
            let ckm = if state & 1 != 0 { "h" } else { "l" };
            let kpam = if state & 2 != 0 { "\x1b=" } else { "\x1b>" };
            let bkm = if state & 4 != 0 { "h" } else { "l" };
            support::feed(t, format!("\x1b[?1{ckm}{kpam}\x1b[?67{bkm}").as_bytes());
            let (wrong, n) = against(t, &entry, &list);
            assert!(wrong.is_empty(), "state {state}: {wrong:#?}");
            assert_eq!(
                n, 109,
                "every row of xterm's that the entry gives a string and baud has a key for"
            );
        }
    });
}

/// **Every key the pinned entry names is in the list
/// `the_termcap_keyboard_sends_the_entrys_strings` sends, or in the list of
/// keys xterm's table does not reach, never both**: no row names Begin
/// (`kb2`); `kcbt`'s row carries no modifier while a back tab carries
/// Shift; xterm's `kf36`-`kf63` rows carry no keysym; `kmous` is not a
/// key. The default encodings of `kb2` and `kcbt` are held by
/// `the_termcap_keyboard_leaves_what_the_entry_does_not_name`.
#[test]
fn every_key_the_termcap_entry_names_is_sent_or_kept_for_a_reason() {
    let all = entries(&source("terminfo/xterm-256color.ti"));
    let entry = &all.values().next().expect("the entry").0;
    assert_eq!(
        entry.get("kb2").map(Vec::as_slice),
        Some(b"\x1bOE".as_slice())
    );
    assert_eq!(
        entry.get("kcbt").map(Vec::as_slice),
        Some(b"\x1b[Z".as_slice())
    );
    let kept = |c: &str| {
        matches!(c, "kb2" | "kcbt" | "kmous")
            || c.strip_prefix("kf")
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (36..=63).contains(&n))
    };
    let sent: BTreeSet<String> = termcap_keys().into_iter().map(|(c, _, _)| c).collect();
    let both: Vec<&String> = sent.iter().filter(|c| kept(c)).collect();
    assert!(both.is_empty(), "sent and kept: {both:?}");
    let neither: Vec<&String> = entry
        .keys()
        .filter(|c| !sent.contains(*c) && !kept(c))
        .collect();
    assert!(neither.is_empty(), "named and unaccounted for: {neither:?}");
}

/// **A key or modifier no row of xterm's table reaches keeps the default
/// keyboard's encoding**, Begin among them; a navigation key is the key it
/// duplicates in either keypad mode, as xterm moves it before it reads the
/// entry; modifyFunctionKeys -1 remaps a function key first.
#[test]
fn the_termcap_keyboard_leaves_what_the_entry_does_not_name() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1050h");
        assert_eq!(keys(t, f(1), CTRL), b"\x1b[1;5P", "no kf1 with Control");
        assert_eq!(
            keys(t, KeyCode::Delete, SHIFT.with(CTRL).with(ALT)),
            b"\x1b[3;8~"
        );
        assert_eq!(keys(t, pad(KeypadKey::Digit1), NONE), b"1");
        assert_eq!(keys(t, KeyCode::Tab, SHIFT), b"\x1b[Z");
        support::feed(t, b"\x1b=");
        assert_eq!(
            keys(t, pad(KeypadKey::Up), NONE),
            b"\x1bOA",
            "kcuu1, under DECKPAM"
        );
        assert_eq!(
            keys(t, pad(KeypadKey::Begin), NONE),
            b"\x1b[E",
            "the entry's kb2, which no row names"
        );
        support::feed(t, b"\x1b[>2n");
        assert_eq!(keys(t, f(1), SHIFT), b"\x1b[23~", "F11's kf11");
    });
}

// ── ?1060, legacy ──────────────────────────────────────────────────────────

/// **The legacy keyboard**: F1-F4 `CSI 11 ~` to `CSI 14 ~`; Control adds
/// 10 to a function key; a modifier parameter only where modifyKeyboard
/// allows it, none at its first value 0, every one while modifyOtherKeys
/// is on; Delete DEL until `?1037` is reset, which DECRQM answers.
#[test]
fn the_legacy_keyboard_is_x11r6s() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1060h");
        for (n, want) in [
            (1, b"\x1b[11~".as_slice()),
            (2, b"\x1b[12~"),
            (3, b"\x1b[13~"),
            (4, b"\x1b[14~"),
            (5, b"\x1b[15~"),
        ] {
            assert_eq!(keys(t, f(n), NONE), want, "F{n}");
        }
        assert_eq!(keys(t, f(1), CTRL), b"\x1b[23~", "Ctrl-F1 is F11");
        assert_eq!(keys(t, f(11), CTRL), b"\x1b[42~", "Ctrl-F11 is F21");
        assert_eq!(keys(t, f(1), SHIFT), b"\x1b[11~", "Shift is dropped");
        assert_eq!(keys(t, KeyCode::Up, SHIFT), b"\x1b[A");
        assert_eq!(keys(t, pad(KeypadKey::Digit1), CTRL), b"1");
        assert_eq!(keys(t, KeyCode::Delete, NONE), b"\x7f");
        assert_eq!(keys(t, KeyCode::Delete, SHIFT), b"\x7f");
        assert_eq!(decrqm(t, 1037), "\\x1b[?1037;1$y", "DEL, ?1037 untouched");
        support::feed(t, b"\x1b[>0;4m");
        assert_eq!(keys(t, f(1), SHIFT), b"\x1b[11;2~", "modifyKeyboard 4");
        assert_eq!(keys(t, KeyCode::Up, SHIFT), b"\x1b[A");
        support::feed(t, b"\x1b[>0;2m");
        assert_eq!(
            keys(t, KeyCode::Up, SHIFT),
            b"\x1b[1;2A",
            "modifyKeyboard 2"
        );
        support::feed(t, b"\x1b[>0m\x1b[>4;1m");
        assert_eq!(keys(t, KeyCode::Up, SHIFT), b"\x1b[1;2A", "modifyOtherKeys");
        support::feed(t, b"\x1b[>4m\x1b[?1037l");
        assert_eq!(keys(t, KeyCode::Delete, NONE), b"\x1b[3~", "?1037 reset");
        assert_eq!(decrqm(t, 1037), "\\x1b[?1037;2$y");
        support::feed(t, b"\x1b[?1060l");
        assert_eq!(keys(t, f(1), NONE), b"\x1bOP", "the default keyboard again");
    });
}

// ── ?1061, VT220 ───────────────────────────────────────────────────────────

/// **The VT220 keyboard**: Home Find `CSI 1 ~`, End Select `CSI 4 ~`,
/// Delete Remove until `?1037` is set; F1-F4 PF1-PF4; Control adds 10 and
/// Shift sends the key's own sequence; the modifiers as on the legacy
/// keyboard.
#[test]
fn the_vt220_keyboard_has_the_vt220s_editing_keypad() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1061h\x1b[?1h");
        assert_eq!(keys(t, KeyCode::Home, NONE), b"\x1b[1~");
        assert_eq!(keys(t, KeyCode::End, NONE), b"\x1b[4~");
        assert_eq!(keys(t, KeyCode::Delete, NONE), b"\x1b[3~");
        assert_eq!(keys(t, KeyCode::Insert, NONE), b"\x1b[2~");
        assert_eq!(
            keys(t, KeyCode::Home, SHIFT),
            b"\x1b[1~",
            "modifyKeyboard 0"
        );
        assert_eq!(keys(t, f(1), NONE), b"\x1bOP");
        assert_eq!(keys(t, f(3), CTRL), b"\x1b[25~", "Ctrl-F3 is F13");
        assert_eq!(keys(t, f(5), SHIFT), b"\x1b[15~", "Shift-F5 is F5's own");
        assert_eq!(keys(t, KeyCode::Up, SHIFT), b"\x1bOA");
        support::feed(t, b"\x1b[?1037h");
        assert_eq!(keys(t, KeyCode::Delete, NONE), b"\x7f");
        assert_eq!(decrqm(t, 1037), "\\x1b[?1037;1$y");
        support::feed(t, b"\x1b[>0;6m");
        assert_eq!(keys(t, KeyCode::End, SHIFT), b"\x1b[4;2~");
        assert_eq!(keys(t, f(5), SHIFT), b"\x1b[15;2~");
    });
}

/// **The VT220 keyboard's keypad**: `+` is the keypad's `,`, and Control
/// with it `-`, for EDT's delete-character on a PC keypad with no comma;
/// Shift keeps `+`; a navigation key is its digit, a VT220 keypad having
/// no other.
#[test]
fn the_vt220_keypad_has_a_comma_and_no_navigation_keys() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1061h");
        assert_eq!(keys(t, pad(KeypadKey::Add), NONE), b",");
        assert_eq!(keys(t, pad(KeypadKey::Add), CTRL), b"-");
        assert_eq!(keys(t, pad(KeypadKey::Comma), CTRL), b"-");
        assert_eq!(keys(t, pad(KeypadKey::Add), SHIFT), b"+");
        assert_eq!(
            keys(t, pad(KeypadKey::Comma), SHIFT.with(CTRL)),
            b",",
            "Shift with Control keeps the comma"
        );
        assert_eq!(keys(t, pad(KeypadKey::Add), SHIFT.with(CTRL)), b"+");
        assert_eq!(keys(t, pad(KeypadKey::Home), NONE), b"7");
        assert_eq!(keys(t, pad(KeypadKey::Delete), NONE), b".");
        support::feed(t, b"\x1b=");
        assert_eq!(keys(t, pad(KeypadKey::Add), NONE), b"\x1bOl");
        assert_eq!(keys(t, pad(KeypadKey::Add), CTRL), b"\x1bOm");
        assert_eq!(keys(t, pad(KeypadKey::Home), NONE), b"\x1bOw");
        assert_eq!(keys(t, pad(KeypadKey::Begin), NONE), b"\x1bOu");
    });
}
