//! xterm's title modes and the title stack's position report
//! (`[[source: ctlseqs]]`): XTSMTITLE `CSI > Pm t` and XTRMTITLE
//! `CSI > Pm T`, which choose whether a title arrives in hexadecimal and
//! whether it is read as UTF-8; DECRQSS's `> t`, which reads them back; and
//! XTTITLEPOS `CSI # S`, as xterm's `xtermReportTitleStack` answers it
//! (`[[source: xterm-misc]]`).

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn decrqss(t: &mut Term<'_>, pt: &[u8]) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, &[b"\x1bP$q", pt, b"\x1b\\"].concat());
    t.reply().to_vec()
}

/// **XTTITLEPOS answers the pushes not yet popped and the stack's depth**,
/// `CSI Pn ; Pm # S`, and the count goes past ten once a push has
/// overwritten the oldest slot, as xterm's `used` does.
#[test]
fn xttitlepos_reports_the_pushes_and_the_depth() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[#S");
        assert_eq!(t.reply(), b"\x1b[0;10#S");
        t.clear_reply();
        support::feed(t, b"\x1b[22t\x1b[22;2t\x1b[#S");
        assert_eq!(t.reply(), b"\x1b[2;10#S");
        t.clear_reply();
        for _ in 0..10 {
            support::feed(t, b"\x1b[22t");
        }
        support::feed(t, b"\x1b[23t\x1b[#S");
        assert_eq!(t.reply(), b"\x1b[11;10#S");
    });
}

/// A direct slot neither pushes nor pops, so XTTITLEPOS does not move.
#[test]
fn a_direct_slot_leaves_the_title_position() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[22;0;3t\x1b[23;0;3t\x1b[#S");
        assert_eq!(t.reply(), b"\x1b[0;10#S");
    });
}

/// **In hexadecimal mode a title is decoded**, either case, and `OSC 0`
/// sets both names; XTRMTITLE 0 turns it back off.
#[test]
fn a_hex_title_is_decoded_while_the_mode_is_set() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[>0t\x1b]2;48656c6C6f\x07");
        assert_eq!(t.title(), b"Hello");
        support::feed(t, b"\x1b]0;4869\x07");
        assert_eq!((t.title(), t.icon_name()), (&b"Hi"[..], &b"Hi"[..]));
        support::feed(t, b"\x1b]1;6963\x07");
        assert_eq!((t.title(), t.icon_name()), (&b"Hi"[..], &b"ic"[..]));
        support::feed(t, b"\x1b[>0T\x1b]2;4869\x07");
        assert_eq!(t.title(), b"4869");
    });
}

/// **A body that is not an even count of hex digits changes nothing**, as
/// xterm's `ChangeGroup` returns, and is counted; a decoded name ends at
/// its first byte below 32.
#[test]
fn a_title_that_is_not_hex_changes_nothing_and_is_counted() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]2;kept\x07\x1b[>0t");
        let before = t.unsupported_param;
        support::feed(t, b"\x1b]2;486\x07\x1b]2;48 69\x07\x1b]2;zz\x07");
        assert_eq!(t.title(), b"kept");
        assert_eq!(t.unsupported_param, before + 3);
        support::feed(t, b"\x1b]2;41074243\x07");
        assert_eq!(t.title(), b"A");
    });
}

/// **The two query modes are refused with the title reports they shape**:
/// counted there and never recorded, so DECRQSS reports them off. A value
/// past 3 is counted; an omitted one is skipped, as xterm skips it.
#[test]
fn the_query_modes_are_refused_with_the_title_reports() {
    support::with_term(&WS, |t| {
        let refused = t.title_reports_refused();
        support::feed(t, b"\x1b[>1;3t");
        assert_ne!(t.title_reports_refused(), refused, "not counted");
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>t\x1b\\");
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[>;2;7t");
        assert_eq!(t.unsupported_param, before + 1);
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>2t\x1b\\");
    });
}

/// **DECRQSS answers the control that sets the modes in force**, `> 0 ; 2
/// t`, and `> t` with none set, which sent back is XTSMTITLE's default; a
/// `> Ps t` naming one of the four gets the same, and one naming none is
/// invalid.
#[test]
fn decrqss_answers_the_title_modes_as_the_control_that_sets_them() {
    support::with_term(&WS, |t| {
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>t\x1b\\");
        support::feed(t, b"\x1b[>2;0t");
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>0;2t\x1b\\");
        assert_eq!(decrqss(t, b">2t"), b"\x1bP1$r>0;2t\x1b\\");
        assert_eq!(decrqss(t, b">4t"), b"\x1bP0$r\x1b\\");
        support::feed(t, b"\x1b[>0T");
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>2t\x1b\\");
        support::feed(t, b"\x1b[>t");
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>t\x1b\\");
        support::feed(t, b"\x1b[>0;2t\x1b[>T");
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>t\x1b\\");
    });
}

/// **`RIS` returns the modes to the default and `DECSTR` keeps them**, as
/// xterm's full and soft resets do.
#[test]
fn ris_resets_the_title_modes_and_decstr_keeps_them() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[>0;2t\x1b[!p");
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>0;2t\x1b\\");
        support::feed(t, b"\x1bc");
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>t\x1b\\");
    });
}

/// **Under ISO 8859-1 the UTF-8 mode keeps a title's GR bytes**, which
/// arrive as UTF-8 and are read so; without it each is its 7-bit byte, as
/// DEC STD 070 reads GR in an `OSC`. The icon name keeps them too, the
/// text's "window/icon labels", and an `OSC` that names neither does not.
#[test]
fn under_latin1_the_utf8_mode_keeps_a_titles_gr_bytes() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b%@\x1b]2;caf\xc3\xa9\x07");
        assert_eq!(t.title(), b"cafC)");
        assert_eq!(t.title_encoding(), InputEncoding::Latin1);
        support::feed(t, b"\x1b[>2t\x1b]2;caf\xc3\xa9\x07\x1b]1;\xc3\xa9\x07");
        assert_eq!(t.title(), "café".as_bytes());
        assert_eq!(t.icon_name(), "é".as_bytes());
        assert_eq!(t.title_encoding(), InputEncoding::Utf8);
        assert_eq!(t.icon_name_encoding(), InputEncoding::Utf8);
        support::feed(t, b"\x1b]2;caf\xe9\x07");
        assert_eq!(t.title(), b"caf\xe9");
        assert_eq!(t.title_encoding(), InputEncoding::Latin1);
    });
}

/// The same title fed a byte at a time: whether a GR byte is kept is read
/// from the body kept so far, so a read boundary cannot move it.
#[test]
fn a_kept_gr_title_is_chunk_invariant() {
    support::with_term(&WS, |t| {
        for b in b"\x1b%@\x1b[>2t\x1b]2;caf\xc3\xa9\x07" {
            support::feed(t, &[*b]);
        }
        assert_eq!(t.title(), "café".as_bytes());
    });
}

/// **A GR-spelled `0;` does not name a title**: the code is read as xterm
/// reads it, 7-bit, so `B0 BB` is `0;` and the body after it keeps its GR
/// bytes, where an `OSC` naming a palette entry folds every one.
#[test]
fn the_code_before_a_kept_title_is_read_seven_bit() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b%@\x1b[>2t\x1b]\xb2\xbbx\xc3\xa9\x07");
        assert_eq!(t.title(), "xé".as_bytes());
    });
}

/// **Under UTF-8 a title is UTF-8 whatever mode 2 says**, as xterm's
/// `utf8Title` resource follows the encoding; the mode's bit is still
/// recorded and reported.
#[test]
fn under_utf8_mode_2_changes_no_title() {
    support::with_term(&WS, |t| {
        support::feed(t, "\x1b[>2T\x1b]2;café\x07".as_bytes());
        assert_eq!(t.title(), "café".as_bytes());
        assert_eq!(t.title_encoding(), InputEncoding::Utf8);
        support::feed(t, "\x1b[>2t\x1b]2;naïve\x07".as_bytes());
        assert_eq!(t.title(), "naïve".as_bytes());
        assert_eq!(decrqss(t, b">t"), b"\x1bP1$r>2t\x1b\\");
    });
}

/// **A hex title that is not UTF-8 is ISO 8859-1**, as `ChangeGroup`
/// assumes one, and one that is reads as UTF-8 while the UTF-8 title mode
/// is in force -- always, under UTF-8.
#[test]
fn a_hex_title_is_utf8_only_where_it_is_valid() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[>0t\x1b]2;636166e9\x07");
        assert_eq!(t.title(), b"caf\xe9");
        assert_eq!(t.title_encoding(), InputEncoding::Latin1);
        support::feed(t, b"\x1b]2;636166c3a9\x07");
        assert_eq!(t.title(), "café".as_bytes());
        assert_eq!(t.title_encoding(), InputEncoding::Utf8);
        support::feed(t, b"\x1b%@\x1b]2;636166c3a9\x07");
        assert_eq!(t.title_encoding(), InputEncoding::Latin1);
    });
}

/// **The title stack keeps each title's encoding**, so a title pushed as
/// UTF-8 under ISO 8859-1 comes back as UTF-8.
#[test]
fn the_title_stack_keeps_each_titles_encoding() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b%@\x1b[>2t\x1b]2;\xc3\xa9\x07\x1b[22;2t");
        support::feed(t, b"\x1b]2;caf\xe9\x07");
        assert_eq!(t.title_encoding(), InputEncoding::Latin1);
        support::feed(t, b"\x1b[23;2t");
        assert_eq!(t.title(), "é".as_bytes());
        assert_eq!(t.title_encoding(), InputEncoding::Utf8);
    });
}
