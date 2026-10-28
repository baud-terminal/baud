//! XTGETXRES, `DCS + Q Pt ST` (`[[source: ctlseqs]]` 510-533): the names
//! hex-encoded and `;` between them, answered `DCS 1 + R` with each known
//! name's `=` and value in hex, or `DCS 0 + R` when the first name is not a
//! resource, in xterm's `do_dcs` shape (`[[source: xterm-misc]]`'s file,
//! misc.c 5251-5306). A value is the one baud starts from.

use crate::support;
use baud_core::*;
use baud_domain::{ImageBudget, SixelClaim};

/// Hex, upper case, as xterm writes a value.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// `DCS + Q` with this list, sent as it is.
fn ask_raw(list: &str) -> Vec<u8> {
    format!("\x1bP+Q{list}\x1b\\").into_bytes()
}

/// `DCS + Q` asking for `names`, hex-encoded and joined with `;`.
fn ask(names: &[&str]) -> Vec<u8> {
    let list: Vec<String> = names.iter().map(|n| hex(n.as_bytes())).collect();
    ask_raw(&list.join(";"))
}

/// One known name's part of an answer.
fn pair(name: &str, value: &str) -> String {
    format!("{}={}", hex(name.as_bytes()), hex(value.as_bytes()))
}

fn reply(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    support::feed(t, bytes);
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

fn show(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

/// What `name` answers on `t`, decoded: `None` when it is unknown.
fn value_of(t: &mut Term<'_>, name: &str) -> Option<String> {
    let got = reply(t, &ask(&[name]));
    let head = format!("\x1bP1+R{}=", hex(name.as_bytes()));
    let body = got.strip_prefix(head.as_bytes())?.strip_suffix(b"\x1b\\")?;
    let text: Vec<u8> = body
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    Some(String::from_utf8(text).unwrap())
}

/// **A known name answers its value; an unknown one is echoed under `0`.**
/// `termName` is an application resource of xterm's, not the VT100
/// widget's, so xterm answers it invalid too.
#[test]
fn xtgetxres_answers_a_resource_and_echoes_an_unknown_one() {
    support::with_corpus(|t| {
        assert_eq!(
            show(&reply(t, &ask(&["autoWrap"]))),
            show(format!("\x1bP1+R{}\x1b\\", pair("autoWrap", "true")).as_bytes())
        );
        assert_eq!(
            show(&reply(t, &ask(&["modifyCursorKeys"]))),
            show(format!("\x1bP1+R{}\x1b\\", pair("modifyCursorKeys", "2")).as_bytes())
        );
        assert_eq!(
            show(&reply(t, &ask(&["termName"]))),
            show(format!("\x1bP0+R{}\x1b\\", hex(b"termName")).as_bytes())
        );
        // Resource names are matched exactly, as `strcmp` does.
        assert_eq!(
            show(&reply(t, &ask(&["autowrap"]))),
            show(format!("\x1bP0+R{}\x1b\\", hex(b"autowrap")).as_bytes())
        );
    });
}

/// **The first name decides the flag, and the list ends after the first
/// unknown one**, echoed with the `;` after it, as xterm's loop writes it;
/// a name is echoed in the case it was sent.
#[test]
fn a_list_ends_after_its_first_unknown_name() {
    support::with_corpus(|t| {
        let want = format!(
            "\x1bP1+R{};{};\x1b\\",
            pair("c132", "false"),
            hex(b"nothing")
        );
        assert_eq!(
            show(&reply(t, &ask(&["c132", "nothing", "autoWrap"]))),
            show(want.as_bytes())
        );
        let want = format!("\x1bP0+R{};\x1b\\", hex(b"nothing"));
        assert_eq!(
            show(&reply(t, &ask(&["nothing", "autoWrap"]))),
            show(want.as_bytes())
        );
        let lower = hex(b"c132").to_lowercase();
        let want = format!("\x1bP1+R{lower}={}\x1b\\", hex(b"false"));
        assert_eq!(show(&reply(t, &ask_raw(&lower))), show(want.as_bytes()));
        let want = format!(
            "\x1bP1+R{};{}\x1b\\",
            pair("c132", "false"),
            pair("autoWrap", "true")
        );
        assert_eq!(
            show(&reply(t, &ask(&["c132", "autoWrap"]))),
            show(want.as_bytes())
        );
    });
}

/// **No first name, no reply**: an empty list, an odd count of hex
/// digits, or a byte that is not hex where a name starts, as
/// `x_decode_hex` refuses them, counted as a parameter not acted on; after a
/// first name the reply ends there.
#[test]
fn a_list_with_no_first_name_gets_no_reply() {
    support::with_corpus(|t| {
        for list in ["", "616", "zz"] {
            let before = t.unsupported_param;
            assert_eq!(show(&reply(t, &ask_raw(list))), "", "{list}");
            assert_eq!(t.unsupported_param, before + 1, "{list} is counted");
        }
        let list = format!("{};616", hex(b"c132"));
        let want = format!("\x1bP1+R{};\x1b\\", pair("c132", "false"));
        assert_eq!(show(&reply(t, &ask_raw(&list))), show(want.as_bytes()));
        let list = format!("{}zz", hex(b"c132"));
        let want = format!("\x1bP1+R{}\x1b\\", pair("c132", "false"));
        assert_eq!(show(&reply(t, &ask_raw(&list))), show(want.as_bytes()));
    });
}

/// Whether a resource is true while its mode is set, or while it is reset.
#[derive(Clone, Copy)]
enum Sense {
    Same,
    Inverse,
}

/// **xterm's VT100-widget resources that start a private mode**, each with
/// that mode, read off xterm and not off baud's table, so a name baud
/// leaves out is a failure here rather than a row nobody wrote: the
/// variable each `dpmodes` case sets and the resource that variable
/// starts from (`[[source: xterm-charproc-c-bccd466]]`, `dpmodes` and
/// `xterm_resources`; `?4` is `!jumpscroll` at `VTInitialize`), and the
/// DECSET list's own "enables the ... resource" (`[[source: ctlseqs]]`
/// 925-1020), which alone ties `eightBitInput` to `?1034`; `?1020` to
/// `?1023` are the read-only modes DECRQM reads from their resources
/// (`[[source: xterm-misc]]` 5842-5853).
const XTERM_MODE_RESOURCES: [(&str, u16, Sense); 39] = [
    ("appcursorDefault", 1, Sense::Same),
    ("jumpScroll", 4, Sense::Inverse),
    ("reverseVideo", 5, Sense::Same),
    ("autoWrap", 7, Sense::Same),
    ("cursorBlink", 13, Sense::Same),
    ("cursorBlinkXOR", 14, Sense::Same),
    ("printerFormFeed", 18, Sense::Same),
    ("printerExtent", 19, Sense::Same),
    ("scrollBar", 30, Sense::Same),
    ("shiftFonts", 35, Sense::Same),
    ("c132", 40, Sense::Same),
    ("curses", 41, Sense::Same),
    ("marginBell", 44, Sense::Same),
    ("reverseWrap", 45, Sense::Same),
    ("logging", 46, Sense::Same),
    ("appkeypadDefault", 66, Sense::Same),
    ("backarrowKey", 67, Sense::Same),
    ("sixelScrolling", 80, Sense::Inverse),
    ("alternateScroll", 1007, Sense::Same),
    ("scrollTtyOutput", 1010, Sense::Same),
    ("scrollKey", 1011, Sense::Same),
    ("fastScroll", 1014, Sense::Same),
    ("utf8", 1020, Sense::Same),
    ("cjkWidth", 1021, Sense::Same),
    ("emojiWidth", 1022, Sense::Same),
    ("privateWidth", 1023, Sense::Same),
    ("eightBitInput", 1034, Sense::Same),
    ("eightBitMeta", 1034, Sense::Same),
    ("numLock", 1035, Sense::Same),
    ("metaSendsEscape", 1036, Sense::Same),
    ("deleteIsDEL", 1037, Sense::Same),
    ("altSendsEscape", 1039, Sense::Same),
    ("keepSelection", 1040, Sense::Same),
    ("selectToClipboard", 1041, Sense::Same),
    ("bellIsUrgent", 1042, Sense::Same),
    ("popOnBell", 1043, Sense::Same),
    ("keepClipboard", 1044, Sense::Same),
    ("titeInhibit", 1046, Sense::Inverse),
    ("privateColorRegisters", 1070, Sense::Same),
];

/// **Each mode's resource is the mode a fresh terminal starts in, and a
/// mode baud has answers its resource**: for every pair of xterm's, the
/// `DECRQM` answer of a terminal that has read nothing, read the other way
/// round where xterm's resource is the mode's inverse, and an unknown
/// resource where `DECRQM` does not recognise the mode. The Sixel modes
/// are recognised only while DA lists Sixel, so the terminal claims it.
#[test]
fn every_mode_resource_is_the_fresh_terminals_mode() {
    let mut wrong = Vec::new();
    let pairs =
        XTERM_MODE_RESOURCES
            .iter()
            .copied()
            .chain([("sixelScrollsRight", 8452, Sense::Same)]);
    for (name, mode, sense) in pairs {
        let ws = support::CORPUS;
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(1 << 16));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_sixel_claim(SixelClaim::Yes);
        let got = reply(&mut t, format!("\x1b[?{mode}$p").as_bytes());
        let state = got
            .strip_prefix(format!("\x1b[?{mode};").as_bytes())
            .and_then(|r| r.strip_suffix(b"$y"))
            .unwrap_or_else(|| panic!("{name}: {}", show(&got)));
        let set = match (state, sense) {
            // **`?35` is DEC's DECHEBM here**, so xterm's `shiftFonts`,
            // rxvt's font shifting that its `?35` starts, is a mode baud
            // refuses and its resource is unknown.
            _ if name == "shiftFonts" => {
                assert_eq!(state, b"2", "DECHEBM, North American");
                None
            }
            (b"0", _) => None,
            (b"1" | b"3", Sense::Same) | (b"2" | b"4", Sense::Inverse) => Some("true"),
            (b"2" | b"4", Sense::Same) | (b"1" | b"3", Sense::Inverse) => Some("false"),
            _ => panic!("{name}: ?{mode} answers {}", show(state)),
        };
        let got = value_of(&mut t, name);
        if got.as_deref() != set {
            wrong.push(format!("{name}: {got:?}, ?{mode} says {set:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **The key-modifier resources are the power-on values XTQMODKEYS
/// (`CSI ? Pp m`) and XTQFMTKEYS (`CSI > Pp g`) report**, and `titleModes` and `eightBitControl` the power-on
/// title modes and C1 transmission DECRQSS reports.
#[test]
fn the_setting_resources_are_the_fresh_terminals_settings() {
    let keys = [
        ("Keyboard", 0),
        ("CursorKeys", 1),
        ("FunctionKeys", 2),
        ("KeypadKeys", 3),
        ("OtherKeys", 4),
        ("ModifierKeys", 6),
        ("SpecialKeys", 7),
    ];
    support::with_corpus(|t| {
        for (name, pp) in keys {
            let got = reply(t, format!("\x1b[?{pp}m").as_bytes());
            let v = got
                .strip_prefix(format!("\x1b[>{pp};").as_bytes())
                .and_then(|r| r.strip_suffix(b"m"))
                .unwrap_or_else(|| panic!("{name}: {}", show(&got)));
            let v = std::str::from_utf8(v).unwrap();
            assert_eq!(value_of(t, &format!("modify{name}")).as_deref(), Some(v));
            if pp == 0 {
                continue;
            }
            let got = reply(t, format!("\x1b[>{pp}g").as_bytes());
            let v = got
                .strip_prefix(format!("\x1b[>{pp};").as_bytes())
                .and_then(|r| r.strip_suffix(b"f"))
                .unwrap_or_else(|| panic!("{name}: {}", show(&got)));
            let v = std::str::from_utf8(v).unwrap();
            assert_eq!(value_of(t, &format!("format{name}")).as_deref(), Some(v));
        }
        assert_eq!(
            show(&reply(t, b"\x1bP$q>t\x1b\\")),
            show(b"\x1bP1$r>t\x1b\\")
        );
        assert_eq!(value_of(t, "titleModes").as_deref(), Some("0"));
        assert_eq!(
            show(&reply(t, b"\x1bP$q\"p\x1b\\")),
            show(b"\x1bP1$r64;1\"p\x1b\\")
        );
        assert_eq!(value_of(t, "eightBitControl").as_deref(), Some("false"));
    });
}

/// **A resource is the value baud starts from, not the state now**: a
/// program that sets the mode does not move it, as ctlseqs says of
/// xterm's ("the resource variable may not reflect the current state").
#[test]
fn a_resource_does_not_follow_the_mode() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?7l\x1b[>1;3m");
        assert_eq!(value_of(t, "autoWrap").as_deref(), Some("true"));
        assert_eq!(value_of(t, "modifyCursorKeys").as_deref(), Some("2"));
    });
}

/// **The five the embedder gives at start answer what it gave**: the
/// cursor's blink and shape (`cursorUnderLine`, `cursorBar`), the pointer
/// mode, the ambiguous width.
#[test]
fn the_embedders_resources_answer_what_it_set() {
    support::with_corpus(|t| {
        assert_eq!(value_of(t, "cursorBlink").as_deref(), Some("true"));
        assert_eq!(value_of(t, "pointerMode").as_deref(), Some("1"));
        assert_eq!(value_of(t, "cjkWidth").as_deref(), Some("false"));
        assert_eq!(value_of(t, "cursorUnderLine").as_deref(), Some("false"));
        assert_eq!(value_of(t, "cursorBar").as_deref(), Some("false"));
        t.set_initial_cursor_style(CursorStyle::of(CursorShape::Bar, CursorBlink::Steady));
        t.set_initial_pointer_mode(PointerMode::Always);
        t.set_ambiguous_width(AmbiguousWidth::Wide);
        assert_eq!(value_of(t, "cursorBlink").as_deref(), Some("false"));
        assert_eq!(value_of(t, "pointerMode").as_deref(), Some("2"));
        assert_eq!(value_of(t, "cjkWidth").as_deref(), Some("true"));
        assert_eq!(value_of(t, "cursorUnderLine").as_deref(), Some("false"));
        assert_eq!(value_of(t, "cursorBar").as_deref(), Some("true"));
        t.set_initial_cursor_style(CursorStyle::of(
            CursorShape::Underline,
            CursorBlink::Blinking,
        ));
        assert_eq!(value_of(t, "cursorBlink").as_deref(), Some("true"));
        assert_eq!(value_of(t, "cursorUnderLine").as_deref(), Some("true"));
        assert_eq!(value_of(t, "cursorBar").as_deref(), Some("false"));
    });
}

/// **`allow*` is what `OSC 60` lists**, under each window-operations
/// setting, so the two queries cannot disagree.
#[test]
fn the_allow_resources_agree_with_osc_60() {
    let names = [
        "allowColorOps",
        "allowFontOps",
        "allowMouseOps",
        "allowPasteControls",
        "allowTcapOps",
        "allowTitleOps",
        "allowWindowOps",
    ];
    for ops in [WindowOps::On, WindowOps::Off] {
        support::with_corpus(|t| {
            t.set_window_ops(ops);
            let listed = reply(t, b"\x1b]60\x1b\\");
            let listed = String::from_utf8(listed).unwrap();
            for name in names {
                let want = match listed.contains(name) {
                    true => "true",
                    false => "false",
                };
                assert_eq!(value_of(t, name).as_deref(), Some(want), "{name} {ops:?}");
            }
        });
    }
}

/// Under `S8C1T` the answer opens with `90` and ends with `9C`.
#[test]
fn xtgetxres_follows_s8c1t() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b G");
        let got = reply(t, &ask(&["c132"]));
        let mut expect = b"\x901+R".to_vec();
        expect.extend_from_slice(pair("c132", "false").as_bytes());
        expect.push(0x9c);
        assert_eq!(show(&got), show(&expect));
    });
}
