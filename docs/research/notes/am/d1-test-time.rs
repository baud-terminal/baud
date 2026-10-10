//! XTGETTCAP research (note AM, section 8.1, N1's cost): how long the
//! exhaustive D1 test the note proposes takes, built the way a test is built.
//! It is D1 exactly as `keymax-main.rs` enumerates it -- each of xterm's table
//! rows a baud `KeyCode` can serve, with the row's own modifiers, as a press
//! with no text and no alternates, under every one of the 98,304 KeyStates --
//! run on one thread in a `#[test]` under the workspace's default (unoptimised)
//! test profile, asserting what the proposed test asserts: that the longest
//! value *equals* the constant, 10 B. `MODE_BITS`, `all_states`, `F21_35`,
//! `table_rows` and `mods_of` are copied verbatim from `keymax-main.rs`.
//!
//! Placed at baud-domain/tests/xtgettcap_d1.rs in a git-archive copy of baud.
//! Run: cargo test -p baud-domain --test xtgettcap_d1 -- --nocapture
use baud_domain::*;

const MODE_BITS: [ModeBit; 5] = [
    ModeBit::CursorKeys,
    ModeBit::LineFeedNewLine,
    ModeBit::AltSendsEscape,
    ModeBit::MetaSendsEscape,
    ModeBit::EightBitInput,
];

fn all_states(kitty: &[u8]) -> Vec<KeyState> {
    let mut v = Vec::new();
    for &k in kitty {
        for m in 0..32u32 {
            let mut modes = Modes::default();
            for (i, b) in MODE_BITS.iter().enumerate() {
                if m & (1 << i) != 0 {
                    modes.set(*b);
                }
            }
            for keypad in [KeypadMode::Numeric, KeypadMode::Application] {
                for other_keys in [OtherKeys::Off, OtherKeys::Level1, OtherKeys::Level2] {
                    for other_keys_format in [OtherKeysFormat::Xterm, OtherKeysFormat::CsiU] {
                        for c1 in [C1Transmit::SevenBit, C1Transmit::EightBit] {
                            for terminal in [TerminalMode::Ansi, TerminalMode::Vt52] {
                                for encoding in [InputEncoding::Utf8, InputEncoding::Latin1] {
                                    v.push(KeyState {
                                        modes,
                                        keypad,
                                        other_keys,
                                        other_keys_format,
                                        c1,
                                        terminal,
                                        encoding,
                                        kitty: KittyFlags::from_param(Some(Param::from_raw(
                                            u16::from(k),
                                        ))),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    v
}

const F21_35: [PuaKey; 15] = [
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

fn table_rows() -> Vec<(String, KeyCode, u8)> {
    let mut r: Vec<(String, KeyCode, u8)> = vec![
        ("khome".into(), KeyCode::Home, 1),
        ("kHOM".into(), KeyCode::Home, 2),
        ("kend".into(), KeyCode::End, 1),
        ("kEND".into(), KeyCode::End, 2),
        ("kcub1".into(), KeyCode::Left, 1),
        ("kcuf1".into(), KeyCode::Right, 1),
        ("kcuu1".into(), KeyCode::Up, 1),
        ("kcud1".into(), KeyCode::Down, 1),
        ("kLFT".into(), KeyCode::Left, 2),
        ("kRIT".into(), KeyCode::Right, 2),
        ("kind".into(), KeyCode::Down, 2),
        ("kri".into(), KeyCode::Up, 2),
        ("kent".into(), KeyCode::Keypad(KeypadKey::Enter), 1),
        // XK_ISO_Left_Tab, state 0: the keysym shift+Tab produces.
        ("kcbt".into(), KeyCode::Tab, 2),
        ("kdch1".into(), KeyCode::Delete, 1),
        ("kich1".into(), KeyCode::Insert, 1),
        ("knp".into(), KeyCode::PageDown, 1),
        ("kpp".into(), KeyCode::PageUp, 1),
        ("kNXT".into(), KeyCode::PageDown, 2),
        ("kPRV".into(), KeyCode::PageUp, 2),
        ("kbs".into(), KeyCode::Backspace, 1),
        ("kUP".into(), KeyCode::Up, 2),
        ("kDN".into(), KeyCode::Down, 2),
        // xtermcap.c 161-164 name the keypad's navigation keysyms, but input.c
        // 1186-1188 maps XK_KP_Home..XK_KP_Begin onto XK_Home..XK_Begin before
        // encoding (every keyboard but VT220), so xterm answers these four as
        // Home, End, Prior and Next.
        ("ka1".into(), KeyCode::Home, 1),
        ("kc1".into(), KeyCode::End, 1),
        ("ka3".into(), KeyCode::PageUp, 1),
        ("kc3".into(), KeyCode::PageDown, 1),
    ];
    for f in 1..=20u8 {
        r.push((
            format!("kf{f}"),
            KeyCode::Function(FnKey::new(f).unwrap()),
            1,
        ));
    }
    for (i, p) in F21_35.iter().enumerate() {
        r.push((format!("kf{}", 21 + i), KeyCode::Pua(*p), 1));
    }
    let groups = [
        ("DN", KeyCode::Down),
        ("LFT", KeyCode::Left),
        ("RIT", KeyCode::Right),
        ("UP", KeyCode::Up),
        ("DC", KeyCode::Delete),
        ("END", KeyCode::End),
        ("HOM", KeyCode::Home),
        ("IC", KeyCode::Insert),
        ("NXT", KeyCode::PageDown),
        ("PRV", KeyCode::PageUp),
    ];
    for (g, k) in groups {
        r.push((format!("k{g}"), k, 2));
        for p in 3..=8u8 {
            r.push((format!("k{g}{p}"), k, p));
        }
    }
    let digits = [
        KeypadKey::Digit1,
        KeypadKey::Digit2,
        KeypadKey::Digit3,
        KeypadKey::Digit4,
        KeypadKey::Digit5,
        KeypadKey::Digit6,
        KeypadKey::Digit7,
        KeypadKey::Digit8,
        KeypadKey::Digit9,
    ];
    for (i, d) in digits.iter().enumerate() {
        r.push((format!("kp{}", i + 1), KeyCode::Keypad(*d), 1));
    }
    let kgroups = [
        ("ADD", KeypadKey::Add),
        ("CMA", KeypadKey::Comma),
        ("DIV", KeypadKey::Divide),
        ("DOT", KeypadKey::Period),
        ("MUL", KeypadKey::Multiply),
        ("SUB", KeypadKey::Minus),
        ("ZRO", KeypadKey::Digit0),
    ];
    for (g, k) in kgroups {
        r.push((format!("kp{g}"), KeyCode::Keypad(k), 2));
        for p in 3..=8u8 {
            r.push((format!("kp{g}{p}"), KeyCode::Keypad(k), p));
        }
    }
    r
}

fn mods_of(p: u8) -> Modifiers {
    let m = p - 1;
    let mut r = Modifiers::NONE;
    if m & 1 != 0 {
        r = r.with(Modifiers::SHIFT);
    }
    if m & 2 != 0 {
        r = r.with(Modifiers::ALT);
    }
    if m & 4 != 0 {
        r = r.with(Modifiers::CTRL);
    }
    r
}

/// The simulated-press constant the proposed test holds the encoder to.
const SIMULATED_PRESS_MAX: usize = 10;

#[test]
fn d1_longest_simulated_press_equals_the_constant() {
    let t0 = std::time::Instant::now();
    let rows = table_rows();
    let states = all_states(&(0..32).collect::<Vec<u8>>());
    // Positive controls: the domain is the one the note counts.
    assert_eq!(rows.len(), 190, "D1 serves 190 rows");
    assert_eq!(states.len(), 98_304, "every KeyState");
    let mut n = 0u64;
    let mut longest = 0usize;
    for s in &states {
        for (_, code, p) in &rows {
            n += 1;
            if let Some(b) = encode_key(KeyEvent::press(*code, mods_of(*p)), *s) {
                longest = longest.max(b.as_bytes().len());
            }
        }
    }
    let el = t0.elapsed();
    println!(
        "D1: {} rows x {} states = {n} encodes, longest {longest} B, {:.2} s on one thread",
        rows.len(),
        states.len(),
        el.as_secs_f64()
    );
    assert_eq!(n, 18_677_760);
    assert_eq!(longest, SIMULATED_PRESS_MAX);
}
