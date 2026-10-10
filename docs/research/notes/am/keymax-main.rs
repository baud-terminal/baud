//! XTGETTCAP research (note AM, section 8.3): how long can one key value
//! be? Measures baud's real `encode_key` at the commit this tree was archived
//! from, over four domains, each enumerated exhaustively:
//!
//! * D0, the global maximum: every `KeyCode` (text keys sampled at every
//!   decimal length of a codepoint, 1 to 7 digits -- U+0000, U+0007 and U+0009
//!   are the one-digit samples -- plus the control-fold and past-u16
//!   classes), every modifier byte, every event kind, and text /
//!   shifted / base-layout each absent or present (U+10FFFF, or the key's own
//!   codepoint for text). Base layer (kitty flags 0) over every KeyState the
//!   base layer reads; kitty layer (flags 1-31) over both C1 forms with the
//!   other fields at their defaults. `encode_base` never reads `s.kitty`
//!   (key.rs 374 and 421 are the only reads), so a kitty-flagged key that
//!   falls through to the base layer is bounded by the base-layer search.
//! * D1, xterm's table rows exactly: each row of xtermcap.c 70-244 that a baud
//!   `KeyCode` can name, with the row's own modifiers, as a press with no text
//!   and no alternates, under every KeyState (all 98,304 combinations).
//! * D2, the table's keys under any of xterm's shift/alt/ctrl combinations
//!   (the eight states its rows use, modifier parameters 1-8), presses, no
//!   text, no alternates, every KeyState.
//! * D3, the table's keys under all 256 modifier bytes, otherwise as D2.
//!
//! Then F21-F35 (`KeyCode::Pua`) under every kitty flags value, `kcbt`
//! (Tab with shift) under each `modifyOtherKeys` level, the longest D1 value
//! on the base layer, the four keypad-corner rows on their own, and positive
//! controls.
//!
//! Run: cargo run --release -p baud-domain --example xtgettcap_keymax
use baud_domain::*;

fn show(b: &[u8]) -> String {
    b.iter()
        .map(|&c| match c {
            0x1b => "ESC ".into(),
            0x7f => "DEL".into(),
            c if c < 32 => format!("^{}", (c + 64) as char),
            c if c >= 0x80 => format!("<{c:02X}>"),
            c => (c as char).to_string(),
        })
        .collect()
}

fn cp(v: u32) -> Codepoint {
    Codepoint::from_char(char::from_u32(v).unwrap())
}

const MODE_BITS: [ModeBit; 5] = [
    ModeBit::CursorKeys,
    ModeBit::LineFeedNewLine,
    ModeBit::AltSendsEscape,
    ModeBit::MetaSendsEscape,
    ModeBit::EightBitInput,
];

/// Every KeyState with each of the given kitty flags. The base layer reads
/// modes (the five bits above: key.rs 351, 936-953, 1102), keypad,
/// other_keys, its format, c1, terminal and encoding.
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

fn base_state(kitty: u8, c1: C1Transmit) -> KeyState {
    KeyState {
        modes: Modes::default(),
        keypad: KeypadMode::default(),
        other_keys: OtherKeys::default(),
        other_keys_format: OtherKeysFormat::default(),
        c1,
        terminal: TerminalMode::default(),
        encoding: InputEncoding::default(),
        kitty: KittyFlags::from_param(Some(Param::from_raw(u16::from(kitty)))),
    }
}

fn describe(s: &KeyState) -> String {
    let m: Vec<&str> = MODE_BITS
        .iter()
        .filter(|b| s.modes.is_set(**b))
        .map(|b| match b {
            ModeBit::CursorKeys => "DECCKM",
            ModeBit::LineFeedNewLine => "LNM",
            ModeBit::AltSendsEscape => "?1039",
            ModeBit::MetaSendsEscape => "?1036",
            ModeBit::EightBitInput => "?1034",
            _ => "?",
        })
        .collect();
    format!(
        "kitty={} modes=[{}] keypad={:?} other_keys={:?}/{:?} c1={:?} terminal={:?} encoding={:?}",
        s.kitty.bits(),
        m.join(","),
        s.keypad,
        s.other_keys,
        s.other_keys_format,
        s.c1,
        s.terminal,
        s.encoding
    )
}

const PUA: [PuaKey; 48] = [
    PuaKey::CapsLock,
    PuaKey::ScrollLock,
    PuaKey::NumLock,
    PuaKey::PrintScreen,
    PuaKey::Pause,
    PuaKey::Menu,
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
    PuaKey::MediaPlay,
    PuaKey::MediaPause,
    PuaKey::MediaPlayPause,
    PuaKey::MediaReverse,
    PuaKey::MediaStop,
    PuaKey::MediaFastForward,
    PuaKey::MediaRewind,
    PuaKey::MediaTrackNext,
    PuaKey::MediaTrackPrevious,
    PuaKey::MediaRecord,
    PuaKey::LowerVolume,
    PuaKey::RaiseVolume,
    PuaKey::MuteVolume,
    PuaKey::LeftShift,
    PuaKey::LeftControl,
    PuaKey::LeftAlt,
    PuaKey::LeftSuper,
    PuaKey::LeftHyper,
    PuaKey::LeftMeta,
    PuaKey::RightShift,
    PuaKey::RightControl,
    PuaKey::RightAlt,
    PuaKey::RightSuper,
    PuaKey::RightHyper,
    PuaKey::RightMeta,
    PuaKey::IsoLevel3Shift,
    PuaKey::IsoLevel5Shift,
];
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
const KEYPAD: [KeypadKey; 24] = [
    KeypadKey::Space,
    KeypadKey::Tab,
    KeypadKey::Enter,
    KeypadKey::Pf1,
    KeypadKey::Pf2,
    KeypadKey::Pf3,
    KeypadKey::Pf4,
    KeypadKey::Multiply,
    KeypadKey::Add,
    KeypadKey::Comma,
    KeypadKey::Minus,
    KeypadKey::Period,
    KeypadKey::Divide,
    KeypadKey::Digit0,
    KeypadKey::Digit1,
    KeypadKey::Digit2,
    KeypadKey::Digit3,
    KeypadKey::Digit4,
    KeypadKey::Digit5,
    KeypadKey::Digit6,
    KeypadKey::Digit7,
    KeypadKey::Digit8,
    KeypadKey::Digit9,
    KeypadKey::Equal,
];

/// Every KeyCode variant; text keys sampled by class and by decimal length.
fn every_code() -> Vec<KeyCode> {
    let mut v = vec![
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
    ];
    for f in 1..=20u8 {
        v.push(KeyCode::Function(FnKey::new(f).unwrap()));
    }
    v.extend(KEYPAD.iter().map(|k| KeyCode::Keypad(*k)));
    v.extend(PUA.iter().map(|p| KeyCode::Pua(*p)));
    // 1 to 7 decimal digits (0x00, 0x07 and 0x09 are the one-digit codepoints;
    // the others start at two), the control-fold rows (@, a, space, ?), the u8
    // and u16 edges, and astral codepoints.
    for c in [
        0x00, 0x07, 0x09, 0x37, 0x61, 0x40, 0x20, 0x3f, 0x5b, 0x7e, 0xe9, 0xff, 0x100, 0x3b1,
        0x3e8, 0x4e00, 0xfffd, 0xffff, 0x10000, 0x1f600, 0x10fffd, 0x10ffff,
    ] {
        v.push(KeyCode::Char(cp(c)));
    }
    v
}

/// One row of xterm's table that baud can name: terminfo name, key, xterm's
/// modifier parameter (1 = none, 2 = shift, ... 8 = shift+alt+ctrl).
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

/// xterm's modifier parameter to baud's modifiers: 1 + mask, shift 1, alt 2,
/// ctrl 4 (ctlseqs' table; xterm's rows use 1-8).
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

/// Modifiers from a raw byte, bit by bit through the public constants.
fn mods_byte(b: u8) -> Modifiers {
    let all = [
        Modifiers::SHIFT,
        Modifiers::ALT,
        Modifiers::CTRL,
        Modifiers::META,
        Modifiers::SUPER,
        Modifiers::HYPER,
        Modifiers::CAPS_LOCK,
        Modifiers::NUM_LOCK,
    ];
    let mut r = Modifiers::NONE;
    for (i, m) in all.iter().enumerate() {
        if b & (1 << i) != 0 {
            r = r.with(*m);
        }
    }
    r
}

#[derive(Clone)]
struct Best {
    len: usize,
    bytes: Vec<u8>,
    what: String,
    n: u64,
    hist: [u64; 49],
}
impl Best {
    fn new() -> Best {
        Best {
            len: 0,
            bytes: vec![],
            what: String::new(),
            n: 0,
            hist: [0; 49],
        }
    }
    fn take(&mut self, out: Option<KeyBytes>, what: impl FnOnce() -> String) {
        self.n += 1;
        if let Some(k) = out {
            let b = k.as_bytes();
            self.hist[b.len()] += 1;
            if b.len() > self.len {
                self.len = b.len();
                self.bytes = b.to_vec();
                self.what = what();
            }
        }
    }
    fn merge(mut self, o: Best) -> Best {
        self.n += o.n;
        for i in 0..49 {
            self.hist[i] += o.hist[i];
        }
        if o.len > self.len {
            self.len = o.len;
            self.bytes = o.bytes;
            self.what = o.what;
        }
        self
    }
    fn print(&self, label: &str) {
        let h: Vec<String> = self
            .hist
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 0)
            .map(|(l, c)| format!("{l}:{c}"))
            .collect();
        println!("{label}");
        println!(
            "  encodes {}  max {} B: {}",
            self.n,
            self.len,
            show(&self.bytes)
        );
        println!("  at: {}", self.what);
        println!("  length histogram (B:count, Some only): {}", h.join(" "));
    }
}

fn parallel<F>(states: &[KeyState], f: F) -> Best
where
    F: Fn(&KeyState, &mut Best) + Sync,
{
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = states.len().div_ceil(threads);
    std::thread::scope(|sc| {
        let hs: Vec<_> = states
            .chunks(chunk)
            .map(|c| {
                let f = &f;
                sc.spawn(move || {
                    let mut b = Best::new();
                    for s in c {
                        f(s, &mut b);
                    }
                    b
                })
            })
            .collect();
        hs.into_iter()
            .map(|h| h.join().unwrap())
            .fold(Best::new(), Best::merge)
    })
}

fn main() {
    let codes = every_code();
    let rows = table_rows();
    let mut table_keys: Vec<KeyCode> = Vec::new();
    for (_, k, _) in &rows {
        if !table_keys.contains(k) {
            table_keys.push(*k);
        }
    }
    let base_states = all_states(&[0]);
    let every_state = all_states(&(0..32).collect::<Vec<u8>>());
    println!(
        "codes {} (D0), xterm table rows baud can name {} over {} distinct keys; base-layer states {}, all states {}",
        codes.len(),
        rows.len(),
        table_keys.len(),
        base_states.len(),
        every_state.len()
    );

    // ---- Positive controls.
    let k1 = base_state(1, C1Transmit::SevenBit);
    let f13 = KeyCode::Function(FnKey::new(13).unwrap());
    let ctl = |e: KeyEvent, s: KeyState| encode_key(e, s).map(|k| show(k.as_bytes()));
    println!("-- positive controls");
    println!(
        "  F13 shift, kitty 1: {:?} (expect ESC [57376;2u)",
        ctl(KeyEvent::press(f13, Modifiers::SHIFT), k1)
    );
    println!(
        "  kcuu1 reset: {:?} (expect ESC [A); Backspace: {:?} (expect DEL)",
        ctl(
            KeyEvent::press(KeyCode::Up, Modifiers::NONE),
            base_state(0, C1Transmit::SevenBit)
        ),
        ctl(
            KeyEvent::press(KeyCode::Backspace, Modifiers::NONE),
            base_state(0, C1Transmit::SevenBit)
        )
    );
    // key.rs 279's worked case, `CSI 57383 ; 128 : 3 u`: F20, kitty_wire 128
    // (shift alt ctrl super hyper meta caps = 127), release, event types.
    let mut rel = KeyEvent::press(
        KeyCode::Function(FnKey::new(20).unwrap()),
        mods_byte(0b0111_1111),
    );
    rel.kind = KeyEventKind::Release;
    let r = encode_key(rel, base_state(0b11, C1Transmit::SevenBit)).unwrap();
    println!(
        "  key.rs 279's case CSI 57383;128:3u: {} = {} B (the comment says 15)",
        show(r.as_bytes()),
        r.as_bytes().len()
    );

    // ---- D0, the global maximum.
    let big = cp(0x10ffff);
    let kinds = [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ];
    let d0_base = parallel(&base_states, |s, b| {
        for &code in &codes {
            let id = match code {
                KeyCode::Char(c) => Some(c),
                _ => None,
            };
            for m in 0..=255u8 {
                let mods = mods_byte(m);
                for kind in kinds {
                    for text in [None, id, Some(big)] {
                        let e = KeyEvent {
                            code,
                            mods,
                            kind,
                            text,
                            shifted: None,
                            base_layout: None,
                        };
                        b.take(encode_key(e, *s), || {
                            format!("{code:?} mods={m} {kind:?} text={text:?} {}", describe(s))
                        });
                    }
                }
            }
        }
    });
    d0_base.print("D0a: base layer (kitty 0), every KeyCode, every modifier byte, every kind, text absent/own/U+10FFFF, every base KeyState");
    let kitty_states: Vec<KeyState> = (1..32u8)
        .flat_map(|k| [C1Transmit::SevenBit, C1Transmit::EightBit].map(|c| base_state(k, c)))
        .collect();
    let d0_kitty = parallel(&kitty_states, |s, b| {
        for &code in &codes {
            let id = match code {
                KeyCode::Char(c) => Some(c),
                _ => None,
            };
            for m in 0..=255u8 {
                let mods = mods_byte(m);
                for kind in kinds {
                    for text in [None, id, Some(big)] {
                        for shifted in [None, Some(big)] {
                            for base_layout in [None, Some(big)] {
                                let e = KeyEvent {
                                    code,
                                    mods,
                                    kind,
                                    text,
                                    shifted,
                                    base_layout,
                                };
                                b.take(encode_key(e, *s), || {
                                    format!(
                                        "{code:?} mods={m} {kind:?} text={text:?} shifted={shifted:?} base={base_layout:?} {}",
                                        describe(s)
                                    )
                                });
                            }
                        }
                    }
                }
            }
        }
    });
    d0_kitty.print("D0b: kitty layer (flags 1-31, both C1 forms), every KeyCode, every modifier byte, every kind, text/shifted/base absent or present");
    println!(
        "D0: encode_key's global maximum over D0a and D0b: {} B (KeyBytes::MAX_LEN = 48)",
        d0_base.len.max(d0_kitty.len)
    );

    // ---- D1, the table rows exactly.
    let d1 = parallel(&every_state, |s, b| {
        for (name, code, p) in &rows {
            let e = KeyEvent::press(*code, mods_of(*p));
            b.take(encode_key(e, *s), || {
                format!("{name} = {code:?} param {p} {}", describe(s))
            });
        }
    });
    d1.print("D1: xterm's table rows baud can name, each with its own modifiers; presses, no text, no alternates; every KeyState (kitty 0-31)");

    // ---- D2, the table's keys under xterm's shift/alt/ctrl combinations.
    let d2 = parallel(&every_state, |s, b| {
        for code in &table_keys {
            for p in 1..=8u8 {
                let e = KeyEvent::press(*code, mods_of(p));
                b.take(encode_key(e, *s), || {
                    format!("{code:?} param {p} {}", describe(s))
                });
            }
        }
    });
    d2.print("D2: the table's keys, any shift/alt/ctrl combination (params 1-8); presses, no text, no alternates; every KeyState");
    // The same domain with kitty flags only (other fields default), the
    // verifier's reading: does anything outside kitty move the maximum?
    let d2k = parallel(&kitty_states, |s, b| {
        for code in &table_keys {
            for p in 1..=8u8 {
                let e = KeyEvent::press(*code, mods_of(p));
                b.take(encode_key(e, *s), || {
                    format!("{code:?} param {p} {}", describe(s))
                });
            }
        }
    });
    d2k.print("D2k: as D2, kitty flags 1-31 and both C1 forms only, every other KeyState field at its default");
    let d2b = parallel(&base_states, |s, b| {
        for code in &table_keys {
            for p in 1..=8u8 {
                let e = KeyEvent::press(*code, mods_of(p));
                b.take(encode_key(e, *s), || {
                    format!("{code:?} param {p} {}", describe(s))
                });
            }
        }
    });
    d2b.print("D2b: as D2, base layer only (kitty 0), every base KeyState");

    // ---- D3, all 256 modifier bytes.
    let d3 = parallel(&every_state, |s, b| {
        for code in &table_keys {
            for m in 0..=255u8 {
                let e = KeyEvent::press(*code, mods_byte(m));
                b.take(encode_key(e, *s), || {
                    format!("{code:?} mods={m} {}", describe(s))
                });
            }
        }
    });
    d3.print("D3: the table's keys, all 256 modifier bytes; presses, no text, no alternates; every KeyState");
    let d3b = parallel(&base_states, |s, b| {
        for code in &table_keys {
            for m in 0..=255u8 {
                let e = KeyEvent::press(*code, mods_byte(m));
                b.take(encode_key(e, *s), || {
                    format!("{code:?} mods={m} {}", describe(s))
                });
            }
        }
    });
    d3b.print("D3b: as D3, base layer only (kitty 0)");

    // ---- F21-F35 under every kitty flags value.
    println!("-- F21-F35 (KeyCode::Pua), unmodified press, 7-bit, every other field default");
    for k in 0..32u8 {
        let s = base_state(k, C1Transmit::SevenBit);
        let outs: Vec<String> = F21_35
            .iter()
            .map(|p| {
                encode_key(KeyEvent::press(KeyCode::Pua(*p), Modifiers::NONE), s)
                    .map_or("None".into(), |b| show(b.as_bytes()))
            })
            .collect();
        let uniq: std::collections::BTreeSet<&String> = outs.iter().collect();
        if k == 0 || k == 1 || k == 31 {
            println!("  kitty {k:2}: {}", outs.join(" | "));
        } else {
            println!(
                "  kitty {k:2}: F21 {}  F35 {}  ({} distinct)",
                outs[0],
                outs[14],
                uniq.len()
            );
        }
    }
    // Over every KeyState: is F21's answer decided by the kitty flags alone?
    let mut by_flags: std::collections::BTreeMap<u8, std::collections::BTreeSet<String>> =
        Default::default();
    for s in &every_state {
        let o = encode_key(
            KeyEvent::press(KeyCode::Pua(PuaKey::F21), Modifiers::NONE),
            *s,
        )
        .map_or("None".into(), |b| show(b.as_bytes()));
        by_flags.entry(s.kitty.bits()).or_default().insert(o);
    }
    let zero: Vec<&String> = by_flags[&0].iter().collect();
    let nonzero: std::collections::BTreeSet<&String> = by_flags
        .iter()
        .filter(|(k, _)| **k != 0)
        .flat_map(|(_, v)| v.iter())
        .collect();
    println!(
        "  F21 over every KeyState: kitty 0 gives {:?}; kitty 1-31 gives {:?}",
        zero, nonzero
    );

    // ---- kcbt: xterm's row is XK_ISO_Left_Tab with state 0; baud has no such
    // code and serves it as Tab with shift (table_rows). What baud's Shift-Tab
    // sends under each modifyOtherKeys level and format, every other field at
    // its default.
    println!("-- kcbt as baud's Shift-Tab, unmodified state otherwise, 7-bit, kitty 0");
    for other_keys in [OtherKeys::Off, OtherKeys::Level1, OtherKeys::Level2] {
        for other_keys_format in [OtherKeysFormat::Xterm, OtherKeysFormat::CsiU] {
            let mut s = base_state(0, C1Transmit::SevenBit);
            s.other_keys = other_keys;
            s.other_keys_format = other_keys_format;
            let o = encode_key(KeyEvent::press(KeyCode::Tab, Modifiers::SHIFT), s)
                .map_or("None".into(), |b| {
                    format!("{} ({} B)", show(b.as_bytes()), b.as_bytes().len())
                });
            println!("  modifyOtherKeys {other_keys:?}/{other_keys_format:?}: {o}");
        }
    }

    // ---- D1 on the base layer alone (kitty 0, every base KeyState).
    let d1b = parallel(&base_states, |s, b| {
        for (name, code, p) in &rows {
            let e = KeyEvent::press(*code, mods_of(*p));
            b.take(encode_key(e, *s), || {
                format!("{name} = {code:?} param {p} {}", describe(s))
            });
        }
    });
    d1b.print("D1b: as D1, base layer only (kitty 0), every base KeyState");

    // ---- The four keypad-corner rows on their own, every KeyState.
    let corners: Vec<(String, KeyCode, u8)> = rows
        .iter()
        .filter(|(n, _, _)| matches!(n.as_str(), "ka1" | "kc1" | "ka3" | "kc3"))
        .cloned()
        .collect();
    let dk = parallel(&every_state, |s, b| {
        for (name, code, p) in &corners {
            let e = KeyEvent::press(*code, mods_of(*p));
            b.take(encode_key(e, *s), || {
                format!("{name} = {code:?} param {p} {}", describe(s))
            });
        }
    });
    dk.print(&format!(
        "D1corners: ka1/kc1/ka3/kc3 ({} rows) as Home/End/PageUp/PageDown, presses, every KeyState",
        corners.len()
    ));
    for (name, code, p) in &corners {
        let reset = encode_key(
            KeyEvent::press(*code, mods_of(*p)),
            base_state(0, C1Transmit::SevenBit),
        )
        .map_or("None".into(), |b| show(b.as_bytes()));
        let mut ckm = base_state(0, C1Transmit::SevenBit);
        ckm.modes.set(ModeBit::CursorKeys);
        let decckm = encode_key(KeyEvent::press(*code, mods_of(*p)), ckm)
            .map_or("None".into(), |b| show(b.as_bytes()));
        println!("  {name}: reset {reset}, DECCKM {decckm}");
    }
}
