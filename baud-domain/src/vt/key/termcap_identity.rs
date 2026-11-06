//! **The compiled termcap table is the committed one**:
//! `build/termcap_keys.rs`' output in `OUT_DIR` against the file
//! `scripts/gen-termcap-keys.py` wrote, before it is deleted: `string`
//! over every key it can be asked for but `Char`, F1-F20 reaching
//! `function` through it, and as text, the header comments aside.

extern crate std;
use super::{termcap_built as built, termcap_data as committed};
use crate::{FnKey, KeyCode, KeypadKey, PuaKey};
use std::vec::Vec;

#[test]
fn the_compiled_termcap_table_is_the_committed_one() {
    use KeypadKey::*;
    let pad = [
        Space, Tab, Enter, Pf1, Pf2, Pf3, Pf4, Multiply, Add, Comma, Minus, Period, Divide, Digit0,
        Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Equal, Left, Right,
        Up, Down, PageUp, PageDown, Home, End, Insert, Delete, Begin,
    ];
    let pua = [
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
    let mut codes: Vec<KeyCode> = Vec::from([
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Right,
        KeyCode::Left,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Insert,
        KeyCode::Delete,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Backspace,
        KeyCode::Escape,
    ]);
    codes.extend(pad.iter().map(|&k| KeyCode::Keypad(k)));
    codes.extend(pua.iter().map(|&k| KeyCode::Pua(k)));
    codes.extend((1..=20).filter_map(FnKey::new).map(KeyCode::Function));
    let (mut asked, mut given) = (0, 0);
    for &code in &codes {
        for param in 0..=255u8 {
            let (a, b) = (built::string(code, param), committed::string(code, param));
            assert_eq!(a, b, "{code:?} {param}");
            asked += 1;
            given += usize::from(a.is_some());
        }
    }
    assert_eq!(
        super::super::table_identity::body(include_str!(concat!(
            env!("OUT_DIR"),
            "/termcap_data.rs"
        ))),
        super::super::table_identity::body(include_str!("termcap_data.rs")),
        "termcap_data.rs"
    );
    std::println!(
        "identical: string over {} keys and every parameter, {asked} asked, {given} given; the file's Rust as text",
        codes.len()
    );
}
