//! **The reports a VT420 answers**: the private device status reports
//! (`CSI ? Ps n`), DECREQTPARM, DECRQUPSS, DECRQDE, xterm's XTQMODKEYS and
//! XTQFMTKEYS, and the key-modifier strings of DECRQSS.
//!
//! Each reply is pinned byte for byte, in the shape its text gives:
//! `[[source: dec-std-070]]` (DSR, 070-4; DECXCPR p. 5-55; DECRQDE p. 5-88;
//! DECRQUPSS p. 5-125; DECMSR p. 5-185; DECCKSR p. 5-181; the printer port,
//! 070-7 p. 7-33; the UDK status, 070-11 p. 11-7; the locator, 070-13),
//! `[[source: vt510-dsr-kbd]]` for the keyboard, `[[source: vt100-ug-ch3]]`
//! for DECREQTPARM, and `[[source: xterm-reset]]`'s `report_mod_fkeys` and
//! `report_fmt_fkeys` for the xterm queries. Each answer says only what is
//! true of baud: no printer, no user-defined keys, a DEC locator that is a
//! mouse (DECELR), no macro memory, one page, one session and a pty with no
//! line errors.

use crate::support;
use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// What `bytes` makes a fresh 20 by 8 terminal send back.
fn answer(bytes: &[u8]) -> Vec<u8> {
    support::render(&WS, bytes, |t| t.reply().to_vec())
}

/// The reply to `query` alone, after `setup`, with what `setup` said cleared.
fn after(setup: &[u8], query: &[u8]) -> Vec<u8> {
    support::with_term(&WS, |t| {
        support::feed(t, setup);
        t.clear_reply();
        support::feed(t, query);
        t.reply().to_vec()
    })
}

fn show(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

// ---- DECXCPR -------------------------------------------------------------

/// "Report Format: CSI ? Pl ; Pc ; Pp R" (`[[source: dec-std-070]]` p. 5-55),
/// the page always sent, and baud has one page, the first. The `?` is the
/// standard's and ctlseqs'; `[[source: vt510-dsr-xcpr]]` prints the report
/// without it, the conflict decided for STD 070 (Jev 0.92/0.92).
#[test]
fn decxcpr_reports_the_cursor_with_its_page() {
    assert_eq!(show(&answer(b"\x1b[?6n")), show(b"\x1b[?1;1;1R"));
    assert_eq!(show(&answer(b"\x1b[3;5H\x1b[?6n")), show(b"\x1b[?3;5;1R"));
}

/// "If Origin Mode is set, addressing is performed relative to the origin of
/// the current scrolling region (the Top and Left Margin)" (p. 5-55, note 1),
/// and the column a pending wrap waits on is the last one: the same cell CPR
/// names, so the two reports cannot disagree.
#[test]
fn decxcpr_counts_from_the_margins_under_decom_as_cpr_does() {
    let margins = b"\x1b[2;6r\x1b[?69h\x1b[3;10s\x1b[?6h\x1b[2;4H";
    assert_eq!(show(&after(margins, b"\x1b[?6n")), show(b"\x1b[?2;4;1R"));
    assert_eq!(show(&after(margins, b"\x1b[6n")), show(b"\x1b[2;4R"));
    let wrap = b"\x1b[1;1Habcdefghijklmnopqrst";
    assert_eq!(show(&after(wrap, b"\x1b[?6n")), show(b"\x1b[?1;20;1R"));
    assert_eq!(show(&after(wrap, b"\x1b[6n")), show(b"\x1b[1;20R"));
}

// ---- the other private DSRs -----------------------------------------------

/// Each private DSR, answered with the one report its text gives for what
/// baud has.
#[test]
fn each_private_status_answers_what_baud_has() {
    let cases: &[(&[u8], &[u8], &str)] = &[
        // 070-7 p. 7-33: "13 The printer is not connected to the terminal".
        (b"\x1b[?15n", b"\x1b[?13n", "printer"),
        // 070-11 p. 11-7: "23 No User Defined Keys are present. The device
        // does not support this extension", and "A report will be given even
        // if the UDK extension is not supported".
        (b"\x1b[?25n", b"\x1b[?23n", "user-defined keys"),
        // `[[source: vt510-dsr-kbd]]`: language 0, "Keyboard language cannot
        // be determined", status 0, ready; no keyboard type, since no value
        // names an unidentified keyboard (Jev's lean, 0.81/0.93).
        (b"\x1b[?26n", b"\x1b[?27;0;0n", "keyboard"),
        // 070-13 p. 15: "?53 No locator", "?57;0 'Unknown Locator'
        // response": a core no embedder has told of a pointer
        // (`tests/vt420/locator.rs` has the other half).
        (b"\x1b[?55n", b"\x1b[?53n", "locator status"),
        (b"\x1b[?56n", b"\x1b[?57;0n", "locator type"),
        // p. 5-185: "the number of bytes available for macro definitions
        // divided by 16", and baud has no macro memory.
        (b"\x1b[?62n", b"\x1b[0*{", "macro space"),
        // p. 5-181: "DCS Pn ! ~ D...D ST", the checksum of the macro
        // definitions, of which there are none; the label is the request's.
        (b"\x1b[?63n", b"\x1bP0!~0000\x1b\\", "memory checksum"),
        (
            b"\x1b[?63;7n",
            b"\x1bP7!~0000\x1b\\",
            "memory checksum, labelled",
        ),
        // 070-4: "?83 not configured for virtual terminals".
        (b"\x1b[?85n", b"\x1b[?83n", "sessions"),
    ];
    for (ask, want, what) in cases {
        assert_eq!(show(&answer(ask)), show(want), "{what}");
    }
}

/// 070-4's algorithm: `?73` while "data integrity flag has not been reported
/// since last power-up or RIS", which clears the flag, then `?70`, "no comm
/// errors or power-up since last report". `?71` is for a communication error,
/// and a pty has no parity or framing to fail. DECSTR is no power-up.
#[test]
fn data_integrity_reports_the_power_up_once_and_then_ok() {
    support::with_term(&WS, |t| {
        let ask = |t: &mut Term<'_>, bytes: &[u8]| {
            t.clear_reply();
            support::feed(t, bytes);
            show(t.reply())
        };
        assert_eq!(ask(t, b"\x1b[?75n"), show(b"\x1b[?73n"), "power-up");
        assert_eq!(ask(t, b"\x1b[?75n"), show(b"\x1b[?70n"), "reported");
        assert_eq!(ask(t, b"\x1b[!p\x1b[?75n"), show(b"\x1b[?70n"), "DECSTR");
        assert_eq!(ask(t, b"\x1bc\x1b[?75n"), show(b"\x1b[?73n"), "RIS");
        assert_eq!(ask(t, b"\x1b[?75n"), show(b"\x1b[?70n"), "after RIS");
    });
}

/// A private DSR no text gives is not answered, and is counted.
#[test]
fn an_unknown_private_status_is_silent_and_counted() {
    support::with_term(&WS, |t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b[?99n\x1b[?16n\x1b[?0n");
        assert_eq!(show(t.reply()), "");
        assert_eq!(t.unsupported, before + 3);
    });
}

// ---- DECREQTPARM ----------------------------------------------------------

/// `[[source: vt100-ug-ch3]]`: `<sol>` 0 or none and 1 are requests, and the
/// report is the request's value plus two, then no parity (1), eight bits
/// (1), the transmit and receive speeds, the clock multiplier (1) and the STP
/// flags (0). The speed is 120, 19200, the highest the table defines: a pty
/// has no line speed (Jev's lean, 0.86/0.80; xterm sends 128, a code the
/// table does not have).
#[test]
fn decreqtparm_reports_the_line_the_vt100_table_can_name() {
    assert_eq!(show(&answer(b"\x1b[x")), show(b"\x1b[2;1;1;120;120;1;0x"));
    assert_eq!(show(&answer(b"\x1b[0x")), show(b"\x1b[2;1;1;120;120;1;0x"));
    assert_eq!(show(&answer(b"\x1b[1x")), show(b"\x1b[3;1;1;120;120;1;0x"));
}

/// 2 and 3 are reports, which a terminal sends rather than answers.
#[test]
fn a_decreptparm_sent_to_baud_is_not_answered_and_is_counted() {
    support::with_term(&WS, |t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b[2;1;1;120;120;1;0x\x1b[3x");
        assert_eq!(show(t.reply()), "");
        assert_eq!(t.unsupported, before + 2);
    });
}

// ---- DECRQUPSS, DECRQDE -----------------------------------------------------

/// p. 5-125: "send_int(0); send_string("!u%5")" while the UPSS is DEC
/// Supplemental, which is what `ESC ( <` draws and the only set baud's
/// repertory can make it.
#[test]
fn decrqupss_answers_dec_supplemental() {
    assert_eq!(show(&answer(b"\x1b[&u")), show(b"\x1bP0!u%5\x1b\\"));
}

/// p. 5-88: `CSI Ph ; Pw ; Pml ; Pmt ; Pmp " w`, the lines and columns
/// shown, the column and line of page memory at the window's corner, and the
/// page: all of the one page, from its first cell.
#[test]
fn decrqde_answers_the_whole_page() {
    assert_eq!(show(&answer(b"\x1b[\"v")), show(b"\x1b[8;20;1;1;1\"w"));
}

/// Both take no parameters (pp. 5-88 and 5-125), so one with a parameter is
/// no request either names, as SCOSC's bare form is the only SCOSC.
#[test]
fn a_parameter_on_decrqupss_or_decrqde_is_no_request() {
    support::with_term(&WS, |t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b[1&u\x1b[2\"v");
        assert_eq!(show(t.reply()), "");
        assert_eq!(t.unsupported, before + 2);
    });
}

// ---- XTQMODKEYS, XTQFMTKEYS -------------------------------------------------

/// `report_mod_fkeys` (`[[source: xterm-reset]]`): `CSI > Pp ; Pv m`, "replies
/// look like a set-mode", so the answer restores the state it reports.
#[test]
fn xtqmodkeys_answers_modify_other_keys_with_its_value() {
    assert_eq!(show(&answer(b"\x1b[?4m")), show(b"\x1b[>4;0m"));
    assert_eq!(
        show(&after(b"\x1b[>4;1m", b"\x1b[?4m")),
        show(b"\x1b[>4;1m")
    );
    assert_eq!(
        show(&after(b"\x1b[>4;2m", b"\x1b[?4m")),
        show(b"\x1b[>4;2m")
    );
    // The answer, fed back, is the setting.
    let back = after(b"\x1b[>4;2m", b"\x1b[?4m");
    assert_eq!(
        show(&after(
            &[b"\x1b[!p".as_slice(), &back].concat(),
            b"\x1b[?4m"
        )),
        show(b"\x1b[>4;2m")
    );
}

/// `report_fmt_fkeys`: `CSI > Pp ; Pv f`. xterm 411 parses the query as
/// `CSI > Pp g` (`[[source: xterm-vtprstbl-c]]`, `dec2_table`), where
/// `[[source: ctlseqs]]` documents `CSI ? Pp g`, which xterm ignores; baud
/// answers xterm's (Jev's lean, 0.48/0.34) and counts the other.
#[test]
fn xtqfmtkeys_answers_format_other_keys_with_its_value() {
    assert_eq!(show(&answer(b"\x1b[>4g")), show(b"\x1b[>4;0f"));
    assert_eq!(
        show(&after(b"\x1b[>4;1f", b"\x1b[>4g")),
        show(b"\x1b[>4;1f")
    );
    support::with_term(&WS, |t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b[?4g");
        assert_eq!(show(t.reply()), "");
        assert_eq!(t.unsupported, before + 1);
    });
}

/// xterm answers once a parameter (`for (value = 0; value < nparam; ...)`),
/// every resource with its value, and one naming none with its `DEFAULT`
/// written unsigned, 65535, which is counted here.
#[test]
fn each_queried_resource_is_answered_or_counted() {
    support::with_term(&WS, |t| {
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[?1;4m");
        assert_eq!(show(t.reply()), show(b"\x1b[>1;2m\x1b[>4;0m"));
        assert_eq!(t.unsupported_param, before);
        t.clear_reply();
        support::feed(t, b"\x1b[?5;4m\x1b[>2;4g");
        assert_eq!(
            show(t.reply()),
            show(b"\x1b[>5;65535m\x1b[>4;0m\x1b[>2;0f\x1b[>4;0f")
        );
        assert_eq!(t.unsupported_param, before + 1);
    });
}

// ---- DECRQSS ----------------------------------------------------------------

/// ctlseqs' DECRQSS strings `> Pm m` and `> Pm f`, answered with "the
/// corresponding CSI string". xterm's `do_dcs` spells the `f` answer with an
/// `m`, an XTMODKEYS, which is not the string that sets it.
#[test]
fn decrqss_answers_the_key_modifier_strings() {
    assert_eq!(
        show(&answer(b"\x1bP$q>4m\x1b\\")),
        show(b"\x1bP1$r>4;0m\x1b\\")
    );
    assert_eq!(
        show(&after(b"\x1b[>4;2m", b"\x1bP$q>4m\x1b\\")),
        show(b"\x1bP1$r>4;2m\x1b\\")
    );
    assert_eq!(
        show(&answer(b"\x1bP$q>4f\x1b\\")),
        show(b"\x1bP1$r>4;0f\x1b\\")
    );
    assert_eq!(
        show(&after(b"\x1b[>4;1f", b"\x1bP$q>4f\x1b\\")),
        show(b"\x1bP1$r>4;1f\x1b\\")
    );
}

/// The DECRQSS strings baud does not act on answer invalid: a key-modifier
/// string naming no resource, and of ctlseqs' eighteen the settings it has
/// none of.
#[test]
fn decrqss_answers_invalid_for_what_baud_does_not_act_on() {
    for pt in [">5m", ">m", ">8f", ">4", "){", "*|", ",|", ",}", ">4t"] {
        let ask = [b"\x1bP$q".as_slice(), pt.as_bytes(), b"\x1b\\"].concat();
        assert_eq!(show(&answer(&ask)), show(b"\x1bP0$r\x1b\\"), "{pt}");
    }
}

/// DECSLPP, `t`, answers the lines per page as the control that sets it
/// (`[[source: dec-std-070]]` p. 5-197): the screen's rows, and 24 on a
/// shorter screen, since "if (parv[0] <= 24) lines_per_page = 24" (p. 5-76)
/// and a `CSI Ps t` below 24 is an XTWINOPS (`[[source: ctlseqs]]`), not a
/// DECSLPP (Jev, FLOOR, 0.96/0.94).
#[test]
fn decrqss_answers_decslpp_as_the_lines_per_page() {
    assert_eq!(show(&answer(b"\x1bP$qt\x1b\\")), show(b"\x1bP1$r24t\x1b\\"));
    let tall = WinSize {
        ws_row: 30,
        ws_col: 20,
    };
    let got = support::render(&tall, b"\x1bP$qt\x1b\\", |t| t.reply().to_vec());
    assert_eq!(show(&got), show(b"\x1bP1$r30t\x1b\\"));
    let widest = WinSize {
        ws_row: 640,
        ws_col: 20,
    };
    let got = support::render(&widest, b"\x1bP$qt\x1b\\", |t| t.reply().to_vec());
    assert_eq!(show(&got), show(b"\x1bP1$r640t\x1b\\"));
}

/// **Every setting DECRQSS answers invalid is one baud does not act on**:
/// the control that would set it is counted as unsupported. Each DEC list
/// is swept -- `[[source: ctlseqs]]`'s, STD 070's (p. 5-196) and the
/// VT520's DECRPSS list (`[[source: vt520-rm]]` pp. 5-102 to 5-103) -- so
/// a setting that lands and leaves its DECRQSS answer behind fails here,
/// as DECSLPP's did.
#[test]
fn every_setting_decrqss_refuses_is_one_baud_counts() {
    let pairs: [(&str, &[u8]); 24] = [
        ("){", b"\x1b[1){"),
        ("*|", b"\x1b[24*|"),
        (",|", b"\x1b[1;1;2,|"),
        (",}", b"\x1b[1;1;2,}"),
        ("-q", b"\x1b[1-q"),
        (",z", b"\x1b[1,z"),
        ("-r", b"\x1b[1-r"),
        ("-p", b"\x1b[1-p"),
        ("*u", b"\x1b[1*u"),
        ("*r", b"\x1b[1*r"),
        ("(p", b"\x1b[1(p"),
        ("$q", b"\x1b[1$q"),
        ("*s", b"\x1b[1*s"),
        ("$s", b"\x1b[1$s"),
        ("*p", b"\x1b[1*p"),
        ("p", b"\x1b[1p"),
        (",{", b"\x1b[1,{"),
        (",x", b"\x1b[1,x"),
        (" r", b"\x1b[1 r"),
        (" v", b"\x1b[1 v"),
        ("+w", b"\x1b[1+w"),
        (" p", b"\x1b[1 p"),
        ("\"u", b"\x1b[1\"u"),
        (" ~", b"\x1b[1 ~"),
    ];
    for (pt, set) in pairs {
        let ask = [b"\x1bP$q".as_slice(), pt.as_bytes(), b"\x1b\\"].concat();
        assert_eq!(show(&answer(&ask)), show(b"\x1bP0$r\x1b\\"), "{pt}");
        let counted = support::with_term(&WS, |t| {
            let before = t.unsupported;
            support::feed(t, set);
            t.unsupported - before
        });
        assert_eq!(counted, 1, "{pt}: {}", show(set));
    }
}

// ---- the eight-bit form -------------------------------------------------------

/// Under `S8C1T` each report opens with the eight-bit `CSI` or `DCS` and a
/// DCS ends with `ST`, `9C`, as every other answer does.
#[test]
fn the_reports_follow_s8c1t() {
    let c1 = b"\x1b G";
    assert_eq!(show(&after(c1, b"\x1b[?6n")), show(b"\x9b?1;1;1R"));
    assert_eq!(show(&after(c1, b"\x1b[?15n")), show(b"\x9b?13n"));
    assert_eq!(show(&after(c1, b"\x1b[?62n")), show(b"\x9b0*{"));
    assert_eq!(show(&after(c1, b"\x1b[?63;2n")), show(b"\x902!~0000\x9c"));
    assert_eq!(show(&after(c1, b"\x1b[&u")), show(b"\x900!u%5\x9c"));
    assert_eq!(show(&after(c1, b"\x1b[x")), show(b"\x9b2;1;1;120;120;1;0x"));
    assert_eq!(show(&after(c1, b"\x1b[\"v")), show(b"\x9b8;20;1;1;1\"w"));
    assert_eq!(show(&after(c1, b"\x1b[?4m")), show(b"\x9b>4;0m"));
}

/// The integrity flag and the C1 width share a byte of state; reporting the
/// one leaves the other, and `S7C1T` after leaves the flag reported.
#[test]
fn reporting_the_integrity_flag_keeps_s8c1t() {
    let got = after(b"\x1b G", b"\x1b[?75n\x1b[?75n\x1b[?6n\x1b F\x1b[?75n");
    assert_eq!(show(&got), show(b"\x9b?73n\x9b?70n\x9b?1;1;1R\x1b[?70n"));
}

/// Every reply at every read size and split: a query cut anywhere is the
/// same query.
#[test]
fn the_reports_are_the_same_at_every_split() {
    let prog: &[u8] = b"\x1b[3;5H\x1b[?6n\x1b[?15n\x1b[?26n\x1b[?63;9n\x1b[?75n\x1b[x\x1b[&u\x1b[\"v\x1b[?4m\x1b[>4g\x1bP$q>4f\x1b\\";
    let whole = answer(prog);
    assert!(!whole.is_empty());
    for cut in 1..prog.len() {
        let got = support::with_term(&WS, |t| {
            support::feed(t, &prog[..cut]);
            support::feed(t, &prog[cut..]);
            t.reply().to_vec()
        });
        assert_eq!(show(&got), show(&whole), "split at {cut}");
    }
}
