//! xterm's rendition stack and its rendition report (`[[source:
//! ctlseqs]]`): XTPUSHSGR `CSI Pm # {` and XTPOPSGR `CSI # }`, with their
//! aliases `# p` and `# q`, as xterm's `xtermPushSGR` and `xtermPopSGR` keep
//! them (`[[source: xterm-misc]]`); and XTREPORTSGR `CSI Pt ; Pl ; Pb ; Pr #
//! |`, the rendition a rectangle's cells share.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

/// The rendition in force, as DECRQSS answers it: `0;...m`.
fn sgr(t: &mut Term<'_>) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, b"\x1bP$qm\x1b\\");
    let r = t.reply().to_vec();
    let body = r
        .strip_prefix(b"\x1bP1$r")
        .and_then(|b| b.strip_suffix(b"\x1b\\"))
        .expect("a valid DECRQSS answer");
    t.clear_reply();
    body.to_vec()
}

fn ask(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, bytes);
    t.reply().to_vec()
}

/// **A bare push and its pop restore the whole rendition**, what xterm
/// keeps and what it does not: the underline style and colour, overline,
/// a font, the colours.
#[test]
fn a_bare_push_and_pop_restore_the_whole_rendition() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1;4:3;53;12;38;5;196;48;2;1;2;3;58;5;9m");
        let before = sgr(t);
        support::feed(t, b"\x1b[#{\x1b[0;7m");
        assert_eq!(sgr(t), b"0;7m");
        support::feed(t, b"\x1b[#}");
        assert_eq!(sgr(t), before);
    });
}

/// `# p` and `# q` are `# {` and `# }`, "an alias ... used to work around
/// language limitations of C#".
#[test]
fn the_aliases_push_and_pop() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[3m\x1b[#p\x1b[0m\x1b[#q");
        assert_eq!(sgr(t), b"0;3m");
    });
}

/// **A list restores only the groups it names**: bold comes back, italic
/// stays off and reverse stays on.
#[test]
fn a_list_push_restores_only_its_groups() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1;3m\x1b[1#{\x1b[0;7m\x1b[#}");
        assert_eq!(sgr(t), b"0;1;7m");
    });
}

/// 30 and 31 are the foreground and the background, "colors (which do not
/// have a unique SGR code)"; 10 and 11, SGR's fonts, name no group, where
/// xterm reads them as 30 and 31.
#[test]
fn the_colour_groups_are_thirty_and_thirty_one() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[31;42m\x1b[30#{\x1b[32;41m\x1b[#}");
        assert_eq!(sgr(t), b"0;38:5:1;48:5:1m");
        support::feed(t, b"\x1b[0;31;42m\x1b[10;11#{\x1b[32;41m\x1b[#}");
        assert_eq!(sgr(t), b"0;38:5:2;48:5:1m");
    });
}

/// **`CSI 0 # {` saves no group**: its pop changes nothing.
#[test]
fn a_push_of_zero_saves_nothing() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1m\x1b[0#{\x1b[0;7m\x1b[#}");
        assert_eq!(sgr(t), b"0;7m");
    });
}

/// **Ten levels, as xterm keeps them**: an eleventh push stores nothing,
/// so its pop restores nothing, and the ten below still pair with their
/// pushes. A pop of an empty stack changes nothing and is counted.
#[test]
fn the_stack_is_ten_deep() {
    support::with_term(&WS, |t| {
        for n in 1..=11u8 {
            support::feed(t, format!("\x1b[0;38;5;{n}m\x1b[#{{").as_bytes());
        }
        support::feed(t, b"\x1b[0;7m\x1b[#}");
        assert_eq!(sgr(t), b"0;7m", "the eleventh level kept a rendition");
        support::feed(t, b"\x1b[#}");
        assert_eq!(sgr(t), b"0;38:5:10m");
        for _ in 0..9 {
            support::feed(t, b"\x1b[#}");
        }
        assert_eq!(sgr(t), b"0;38:5:1m");
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[#}");
        assert_eq!(t.unsupported_param, before + 1);
        assert_eq!(sgr(t), b"0;38:5:1m");
    });
}

/// **4 and 21 are xterm's two underline bits on baud's one style**: 4
/// alone leaves a double underline in force, 21 alone brings one back.
#[test]
fn four_and_twenty_one_restore_xterms_underline_bits() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[4:3m\x1b[4#{\x1b[21m\x1b[#}");
        assert_eq!(sgr(t), b"0;21m");
        support::feed(t, b"\x1b[0;21m\x1b[21#{\x1b[4m\x1b[#}");
        assert_eq!(sgr(t), b"0;21m");
        support::feed(t, b"\x1b[0;4:3m\x1b[4;21#{\x1b[21m\x1b[#}");
        assert_eq!(sgr(t), b"0;4:3m");
    });
}

/// **5 restores both blink rates**, one aspect, as the owner chose.
#[test]
fn five_restores_the_blink_rate() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[6m\x1b[5#{\x1b[5m\x1b[#}");
        assert_eq!(sgr(t), b"0;6m");
    });
}

/// **The popped rendition is the one text is written in.**
#[test]
fn text_after_a_pop_takes_the_restored_rendition() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1m\x1b[#{\x1b[0mA\x1b[#}B");
        let cells = support::screen(t);
        assert!(!t.style_of(cells[0]).attrs().has(Attrs::BOLD));
        assert!(t.style_of(cells[1]).attrs().has(Attrs::BOLD));
    });
}

/// **`RIS` empties the stack and `DECSTR` keeps it**: RIS returns the
/// terminal "to its initial state", where nothing is saved, as it empties
/// XTSAVE's cache; xterm's resets keep it, a departure not followed.
#[test]
fn ris_empties_the_rendition_stack_and_decstr_keeps_it() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1m\x1b[#{\x1b[!p\x1b[#}");
        assert_eq!(sgr(t), b"0;1m");
        support::feed(t, b"\x1b[0;3m\x1b[#{\x1bc\x1b[0;7m");
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[#}");
        assert_eq!(sgr(t), b"0;7m");
        assert_eq!(t.unsupported_param, before + 1, "the stack was not empty");
    });
}

/// **XTREPORTSGR answers what every cell of the rectangle shares**, in
/// DECRQSS's spelling: bold and red where both cells are, and the colour
/// dropped once one cell is green.
#[test]
fn xtreportsgr_answers_the_attributes_every_cell_shares() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1;31mAB\x1b[32mC\x1b[0m");
        assert_eq!(ask(t, b"\x1b[1;1;1;2#|"), b"\x1b[0;1;38:5:1m");
        assert_eq!(ask(t, b"\x1b[1;1;1;3#|"), b"\x1b[0;1m");
        assert_eq!(ask(t, b"\x1b[1;1;1;4#|"), b"\x1b[0m");
    });
}

/// **A direct colour is answered in T.416's form**, `48:2::r:g:b`, as
/// DECRQSS answers it and as xterm's `xtermFormatSGR` writes both.
#[test]
fn xtreportsgr_answers_a_direct_colour_in_t416s_form() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[48;2;1;2;3;58;2;4;5;6mAB\x1b[0m");
        assert_eq!(
            ask(t, b"\x1b[1;1;1;2#|"),
            b"\x1b[0;58:2::4:5:6;48:2::1:2:3m"
        );
    });
}

/// **A rectangle that holds no cell answers `CSI 0 m`**, as xterm's does.
#[test]
fn an_empty_rectangle_answers_zero() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1mAB\x1b[0m");
        assert_eq!(ask(t, b"\x1b[2;1;1;1#|"), b"\x1b[0m");
    });
}

/// **The rectangle counts from the origin under DECOM**, as DEC's
/// rectangle operations do.
#[test]
fn xtreportsgr_counts_from_the_origin_under_decom() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[3;10r\x1b[?6h\x1b[1;1H\x1b[7mX\x1b[0m");
        assert_eq!(ask(t, b"\x1b[1;1;1;1#|"), b"\x1b[0;7m");
    });
}
