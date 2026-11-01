//! The termcap keyboard's table, `?1050`: each key of xterm's XTGETTCAP
//! table that the pinned xterm-256color entry gives a string, with its
//! modifier parameter, and the string of the key's first row that the entry
//! has.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-termcap-keys.py`;
//! `the_termcap_table_is_the_pinned_entry` fails if this file and the
//! generator disagree. The strings are ncurses' xterm-256color entry's
//! (`LICENSES/ncurses-COPYING.txt`), the rows and their order xterm's
//! xtermcap.c (`LICENSES/xterm-COPYING.txt`).

use crate::{KeyCode, KeypadKey, PuaKey};

/// The string the entry gives `code` pressed with modifier parameter
/// `param` (0 for none, else xterm's `1 + mask`), or `None` for a key
/// and parameter it gives none.
pub(super) fn string(code: KeyCode, param: u8) -> Option<&'static [u8]> {
    Some(match (code, param) {
        // khome
        (KeyCode::Home, 0) => b"\x1bOH",
        // kHOM
        (KeyCode::Home, 2) => b"\x1b[1;2H",
        // kend
        (KeyCode::End, 0) => b"\x1bOF",
        // kEND
        (KeyCode::End, 2) => b"\x1b[1;2F",
        // kcub1
        (KeyCode::Left, 0) => b"\x1bOD",
        // kcuf1
        (KeyCode::Right, 0) => b"\x1bOC",
        // kcuu1
        (KeyCode::Up, 0) => b"\x1bOA",
        // kcud1
        (KeyCode::Down, 0) => b"\x1bOB",
        // kLFT
        (KeyCode::Left, 2) => b"\x1b[1;2D",
        // kRIT
        (KeyCode::Right, 2) => b"\x1b[1;2C",
        // kind
        (KeyCode::Down, 2) => b"\x1b[1;2B",
        // kri
        (KeyCode::Up, 2) => b"\x1b[1;2A",
        // kf21
        (KeyCode::Pua(PuaKey::F21), 0) => b"\x1b[20;2~",
        // kf22
        (KeyCode::Pua(PuaKey::F22), 0) => b"\x1b[21;2~",
        // kf23
        (KeyCode::Pua(PuaKey::F23), 0) => b"\x1b[23;2~",
        // kf24
        (KeyCode::Pua(PuaKey::F24), 0) => b"\x1b[24;2~",
        // kf25
        (KeyCode::Pua(PuaKey::F25), 0) => b"\x1b[1;5P",
        // kf26
        (KeyCode::Pua(PuaKey::F26), 0) => b"\x1b[1;5Q",
        // kf27
        (KeyCode::Pua(PuaKey::F27), 0) => b"\x1b[1;5R",
        // kf28
        (KeyCode::Pua(PuaKey::F28), 0) => b"\x1b[1;5S",
        // kf29
        (KeyCode::Pua(PuaKey::F29), 0) => b"\x1b[15;5~",
        // kf30
        (KeyCode::Pua(PuaKey::F30), 0) => b"\x1b[17;5~",
        // kf31
        (KeyCode::Pua(PuaKey::F31), 0) => b"\x1b[18;5~",
        // kf32
        (KeyCode::Pua(PuaKey::F32), 0) => b"\x1b[19;5~",
        // kf33
        (KeyCode::Pua(PuaKey::F33), 0) => b"\x1b[20;5~",
        // kf34
        (KeyCode::Pua(PuaKey::F34), 0) => b"\x1b[21;5~",
        // kf35
        (KeyCode::Pua(PuaKey::F35), 0) => b"\x1b[23;5~",
        // kent
        (KeyCode::Keypad(KeypadKey::Enter), 0) => b"\x1bOM",
        // kdch1
        (KeyCode::Delete, 0) => b"\x1b[3~",
        // kich1
        (KeyCode::Insert, 0) => b"\x1b[2~",
        // knp
        (KeyCode::PageDown, 0) => b"\x1b[6~",
        // kpp
        (KeyCode::PageUp, 0) => b"\x1b[5~",
        // kNXT
        (KeyCode::PageDown, 2) => b"\x1b[6;2~",
        // kPRV
        (KeyCode::PageUp, 2) => b"\x1b[5;2~",
        // kbs
        (KeyCode::Backspace, 0) => b"\x08",
        // kDN3
        (KeyCode::Down, 3) => b"\x1b[1;3B",
        // kDN4
        (KeyCode::Down, 4) => b"\x1b[1;4B",
        // kDN5
        (KeyCode::Down, 5) => b"\x1b[1;5B",
        // kDN6
        (KeyCode::Down, 6) => b"\x1b[1;6B",
        // kDN7
        (KeyCode::Down, 7) => b"\x1b[1;7B",
        // kLFT3
        (KeyCode::Left, 3) => b"\x1b[1;3D",
        // kLFT4
        (KeyCode::Left, 4) => b"\x1b[1;4D",
        // kLFT5
        (KeyCode::Left, 5) => b"\x1b[1;5D",
        // kLFT6
        (KeyCode::Left, 6) => b"\x1b[1;6D",
        // kLFT7
        (KeyCode::Left, 7) => b"\x1b[1;7D",
        // kRIT3
        (KeyCode::Right, 3) => b"\x1b[1;3C",
        // kRIT4
        (KeyCode::Right, 4) => b"\x1b[1;4C",
        // kRIT5
        (KeyCode::Right, 5) => b"\x1b[1;5C",
        // kRIT6
        (KeyCode::Right, 6) => b"\x1b[1;6C",
        // kRIT7
        (KeyCode::Right, 7) => b"\x1b[1;7C",
        // kUP3
        (KeyCode::Up, 3) => b"\x1b[1;3A",
        // kUP4
        (KeyCode::Up, 4) => b"\x1b[1;4A",
        // kUP5
        (KeyCode::Up, 5) => b"\x1b[1;5A",
        // kUP6
        (KeyCode::Up, 6) => b"\x1b[1;6A",
        // kUP7
        (KeyCode::Up, 7) => b"\x1b[1;7A",
        // kDC
        (KeyCode::Delete, 2) => b"\x1b[3;2~",
        // kDC3
        (KeyCode::Delete, 3) => b"\x1b[3;3~",
        // kDC4
        (KeyCode::Delete, 4) => b"\x1b[3;4~",
        // kDC5
        (KeyCode::Delete, 5) => b"\x1b[3;5~",
        // kDC6
        (KeyCode::Delete, 6) => b"\x1b[3;6~",
        // kDC7
        (KeyCode::Delete, 7) => b"\x1b[3;7~",
        // kEND3
        (KeyCode::End, 3) => b"\x1b[1;3F",
        // kEND4
        (KeyCode::End, 4) => b"\x1b[1;4F",
        // kEND5
        (KeyCode::End, 5) => b"\x1b[1;5F",
        // kEND6
        (KeyCode::End, 6) => b"\x1b[1;6F",
        // kEND7
        (KeyCode::End, 7) => b"\x1b[1;7F",
        // kHOM3
        (KeyCode::Home, 3) => b"\x1b[1;3H",
        // kHOM4
        (KeyCode::Home, 4) => b"\x1b[1;4H",
        // kHOM5
        (KeyCode::Home, 5) => b"\x1b[1;5H",
        // kHOM6
        (KeyCode::Home, 6) => b"\x1b[1;6H",
        // kHOM7
        (KeyCode::Home, 7) => b"\x1b[1;7H",
        // kIC
        (KeyCode::Insert, 2) => b"\x1b[2;2~",
        // kIC3
        (KeyCode::Insert, 3) => b"\x1b[2;3~",
        // kIC4
        (KeyCode::Insert, 4) => b"\x1b[2;4~",
        // kIC5
        (KeyCode::Insert, 5) => b"\x1b[2;5~",
        // kIC6
        (KeyCode::Insert, 6) => b"\x1b[2;6~",
        // kIC7
        (KeyCode::Insert, 7) => b"\x1b[2;7~",
        // kNXT3
        (KeyCode::PageDown, 3) => b"\x1b[6;3~",
        // kNXT4
        (KeyCode::PageDown, 4) => b"\x1b[6;4~",
        // kNXT5
        (KeyCode::PageDown, 5) => b"\x1b[6;5~",
        // kNXT6
        (KeyCode::PageDown, 6) => b"\x1b[6;6~",
        // kNXT7
        (KeyCode::PageDown, 7) => b"\x1b[6;7~",
        // kPRV3
        (KeyCode::PageUp, 3) => b"\x1b[5;3~",
        // kPRV4
        (KeyCode::PageUp, 4) => b"\x1b[5;4~",
        // kPRV5
        (KeyCode::PageUp, 5) => b"\x1b[5;5~",
        // kPRV6
        (KeyCode::PageUp, 6) => b"\x1b[5;6~",
        // kPRV7
        (KeyCode::PageUp, 7) => b"\x1b[5;7~",
        (KeyCode::Function(f), p) => return function(f.number(), p),
        _ => return None,
    })
}

/// The string for function key `n`, F1 to F20, with parameter `param`.
fn function(n: u8, param: u8) -> Option<&'static [u8]> {
    Some(match (n, param) {
        // kf1
        (1, 0) => b"\x1bOP",
        // kf2
        (2, 0) => b"\x1bOQ",
        // kf3
        (3, 0) => b"\x1bOR",
        // kf4
        (4, 0) => b"\x1bOS",
        // kf5
        (5, 0) => b"\x1b[15~",
        // kf6
        (6, 0) => b"\x1b[17~",
        // kf7
        (7, 0) => b"\x1b[18~",
        // kf8
        (8, 0) => b"\x1b[19~",
        // kf9
        (9, 0) => b"\x1b[20~",
        // kf10
        (10, 0) => b"\x1b[21~",
        // kf11
        (11, 0) => b"\x1b[23~",
        // kf12
        (12, 0) => b"\x1b[24~",
        // kf13
        (13, 0) => b"\x1b[1;2P",
        // kf14
        (14, 0) => b"\x1b[1;2Q",
        // kf15
        (15, 0) => b"\x1b[1;2R",
        // kf16
        (16, 0) => b"\x1b[1;2S",
        // kf17
        (17, 0) => b"\x1b[15;2~",
        // kf18
        (18, 0) => b"\x1b[17;2~",
        // kf19
        (19, 0) => b"\x1b[18;2~",
        // kf20
        (20, 0) => b"\x1b[19;2~",
        _ => return None,
    })
}
