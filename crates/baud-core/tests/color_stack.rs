//! xterm's palette stack (`[[source: ctlseqs]]`): XTPUSHCOLORS `CSI Pm #
//! P`, XTPOPCOLORS `CSI Pm # Q` and XTREPORTCOLORS `CSI # R`, as xterm's
//! `xtermPushColors`, `xtermPopColors` and `xtermReportColors` keep and
//! answer them (`[[source: xterm-misc]]`): the `OSC 4` palette, the special
//! colours and the dynamic ones, ten palettes deep.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn report(t: &mut Term<'_>) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, b"\x1b[#R");
    t.reply().to_vec()
}

fn entry(t: &Term<'_>, i: u8) -> Option<(u8, u8, u8)> {
    t.palette(Chan::from_u8(i))
}

/// **A pop puts back every colour the push saved**: a palette entry, a
/// special colour and the dynamic foreground, background and cursor.
#[test]
fn a_pop_restores_the_palette_the_specials_and_the_dynamic_colours() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            b"\x1b]4;1;rgb:11/22/33\x07\x1b]5;0;rgb:44/55/66\x07\
              \x1b]10;rgb:77/88/99\x07\x1b]12;rgb:aa/bb/cc\x07",
        );
        support::feed(t, b"\x1b[#P");
        support::feed(
            t,
            b"\x1b]4;1;rgb:ff/00/00\x07\x1b]4;2;rgb:00/ff/00\x07\x1b]105\x07\
              \x1b]110\x07\x1b]12;rgb:01/02/03\x07",
        );
        assert_eq!(entry(t, 1), Some((0xff, 0, 0)));
        support::feed(t, b"\x1b[#Q");
        assert_eq!(entry(t, 1), Some((0x11, 0x22, 0x33)));
        assert_eq!(entry(t, 2), None, "an entry the push had not set");
        assert_eq!(
            t.special_color(SpecialColor::ALL[0]),
            Some((0x44, 0x55, 0x66))
        );
        assert_eq!(
            t.dynamic_color(DynamicColor::Foreground),
            Some((0x77, 0x88, 0x99))
        );
        assert_eq!(
            t.dynamic_color(DynamicColor::Cursor),
            Some((0xaa, 0xbb, 0xcc))
        );
    });
}

/// **A pop that recolours moves the colour epoch**, so the renderer
/// resolves its table again; one that changes nothing does not.
#[test]
fn a_pop_that_recolours_moves_the_colour_epoch() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]4;3;rgb:10/20/30\x07\x1b[#P\x1b[#P");
        let before = t.color_epoch();
        support::feed(t, b"\x1b[#Q");
        assert_eq!(t.color_epoch(), before, "nothing changed");
        support::feed(t, b"\x1b]4;3;rgb:ff/ff/ff\x07");
        let set = t.color_epoch();
        support::feed(t, b"\x1b[#Q");
        assert_ne!(t.color_epoch(), set);
        assert_eq!(entry(t, 3), Some((0x10, 0x20, 0x30)));
    });
}

/// **XTREPORTCOLORS answers `CSI ? Pu ; Ps # Q`**: the current entry and
/// the palettes stored, which a pop leaves stored.
#[test]
fn xtreportcolors_answers_the_entry_and_the_palettes_stored() {
    support::with_term(&WS, |t| {
        assert_eq!(report(t), b"\x1b[?0;0#Q");
        support::feed(t, b"\x1b[#P\x1b[#P");
        assert_eq!(report(t), b"\x1b[?2;2#Q");
        support::feed(t, b"\x1b[#Q");
        assert_eq!(report(t), b"\x1b[?1;2#Q");
    });
}

/// **A palette from 1 to 10 is stored or read directly, "without
/// pushing" and "without popping"**: the current entry stays where it is,
/// where xterm's code moves it.
#[test]
fn a_direct_palette_leaves_the_current_entry() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]4;5;rgb:01/01/01\x07\x1b[3#P");
        assert_eq!(report(t), b"\x1b[?0;1#Q");
        support::feed(t, b"\x1b]4;5;rgb:02/02/02\x07\x1b[#P");
        support::feed(t, b"\x1b]4;5;rgb:03/03/03\x07\x1b[3#Q");
        assert_eq!(entry(t, 5), Some((1, 1, 1)));
        assert_eq!(report(t), b"\x1b[?1;2#Q");
        support::feed(t, b"\x1b[#Q");
        assert_eq!(entry(t, 5), Some((2, 2, 2)));
        assert_eq!(report(t), b"\x1b[?0;2#Q");
    });
}

/// **Each parameter is one push**, as xterm's loop takes them.
#[test]
fn each_parameter_is_one_push() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[;#P");
        assert_eq!(report(t), b"\x1b[?2;2#Q");
    });
}

/// **Ten palettes**: an eleventh bare push, a pop of an empty stack, a
/// palette never stored and one past 10 change nothing and are counted.
#[test]
fn the_stack_is_ten_deep_and_refusals_are_counted() {
    support::with_term(&WS, |t| {
        for _ in 0..10 {
            support::feed(t, b"\x1b[#P");
        }
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[#P\x1b[11#P\x1b[11#Q");
        assert_eq!(t.unsupported_param, before + 3);
        assert_eq!(report(t), b"\x1b[?10;10#Q");
    });
    support::with_term(&WS, |t| {
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[#Q\x1b[4#Q");
        assert_eq!(t.unsupported_param, before + 2);
        assert_eq!(report(t), b"\x1b[?0;0#Q");
    });
}

/// **`RIS` empties the stack and `DECSTR` keeps it**, as `RIS` empties
/// XTSAVE's cache; xterm keeps it across both.
#[test]
fn ris_empties_the_palette_stack_and_decstr_keeps_it() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]4;6;rgb:12/34/56\x07\x1b[#P\x1b[!p");
        assert_eq!(report(t), b"\x1b[?1;1#Q");
        support::feed(t, b"\x1bc");
        assert_eq!(report(t), b"\x1b[?0;0#Q");
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[#Q\x1b[1#Q");
        assert_eq!(t.unsupported_param, before + 2);
        assert_eq!(entry(t, 6), None);
    });
}

/// The answer follows `S8C1T`.
#[test]
fn the_report_follows_s8c1t() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b G");
        assert_eq!(report(t), b"\x9b?0;0#Q");
    });
}
