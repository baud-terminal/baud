//! **DEC's text locator, from the byte stream alone**: DECELR, DECSLE,
//! DECEFR and DECRQLP as `[[source: dec-std-070]]` 070-13 gives them
//! (pp. 5-11, PDF 1020-1026), and the DECLRP each answers with before any
//! pointer is given. What the pointer reports is `tests/locator.rs`.
//!
//! Before 2026-10-20 all four were counted and dropped, and `CSI ? 55 n`
//! said there was no locator.

use crate::support;
use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// **A core embedded with no pointer says it has no locator**, and once
/// the embedder says it has one, that it is ready: 070-13 p. 15, "?53 No
/// locator", "?50 Locator ready", "?57;0 'Unknown Locator' response",
/// "?57;1 Device is Digital Corporate Mouse", the pairs ctlseqs gives ("CSI
/// ? 5 0 n Locator available ... or CSI ? 5 3 n No Locator"). With none,
/// a DECRQLP and a DECEFR answer DECLRP's "the locator is unavailable (not
/// enabled, or physically not present)" though DECELR enabled it.
#[test]
fn a_core_with_no_pointer_has_no_locator() {
    let ask = |device: Option<LocatorDevice>, bytes: &[u8]| {
        support::with_term(&WS, |t| {
            if let Some(d) = device {
                t.set_locator_device(d);
            }
            support::feed(t, bytes);
            show(t.reply())
        })
    };
    let none = None;
    let told = Some(LocatorDevice::Present);
    assert_eq!(ask(none, b"\x1b[?55n"), show(b"\x1b[?53n"));
    assert_eq!(ask(none, b"\x1b[?56n"), show(b"\x1b[?57;0n"));
    assert_eq!(ask(told, b"\x1b[?55n"), show(b"\x1b[?50n"));
    assert_eq!(ask(told, b"\x1b[?56n"), show(b"\x1b[?57;1n"));
    let absent = Some(LocatorDevice::Absent);
    assert_eq!(ask(absent, b"\x1b[?55n"), show(b"\x1b[?53n"), "untold");
    for q in [&b"\x1b[1'z\x1b['|"[..], b"\x1b[1'z\x1b[1;1;5;5'w"] {
        assert_eq!(ask(none, q), show(b"\x1b[0&w"), "{}", show(q));
    }
    assert_eq!(ask(told, b"\x1b[1'z\x1b['|"), show(b"\x1b[1;0&w"));
    // RIS keeps what the embedder said.
    assert_eq!(ask(told, b"\x1bc\x1b[?55n"), show(b"\x1b[?50n"), "RIS");
}

/// What `bytes` make a fresh terminal send back, and what they counted.
fn reply(bytes: &[u8]) -> (String, usize) {
    support::with_term(&WS, |t| {
        t.set_locator_device(LocatorDevice::Present);
        support::feed(t, bytes);
        (show(t.reply()), t.unsupported_param + t.unsupported)
    })
}

fn show(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

/// **DECRQLP with the locator disabled answers event 0**, "indicating
/// locator not available to avoid timing out the application"; once
/// DECELR enables it, event 1, "Request", with no buttons and the
/// position omitted, since no pointer was given: "If Pr is omitted, the
/// locator position is undefined".
#[test]
fn decrqlp_answers_unavailable_until_decelr_enables_the_locator() {
    assert_eq!(reply(b"\x1b['|").0, show(b"\x1b[0&w"));
    assert_eq!(reply(b"\x1b[0'|").0, show(b"\x1b[0&w"));
    assert_eq!(reply(b"\x1b[1'z\x1b['|").0, show(b"\x1b[1;0&w"));
    assert_eq!(reply(b"\x1b[1;2'z\x1b[1'|").0, show(b"\x1b[1;0&w"));
    assert_eq!(reply(b"\x1b[1;1'z\x1b['|").0, show(b"\x1b[1;0&w"), "pixels");
}

/// "If the terminal receives a filter rectangle when locator reporting is
/// not enabled, the rectangle shall be triggered to send a report
/// indicating that the locator is not available" (DECEFR note 4); xterm
/// sends nothing.
#[test]
fn decefr_with_the_locator_disabled_reports_it_unavailable() {
    assert_eq!(reply(b"\x1b[1;1;5;5'w").0, show(b"\x1b[0&w"));
    assert_eq!(reply(b"\x1b['w").0, show(b"\x1b[0&w"));
}

/// **With the position undefined, DECEFR with a corner given reports at
/// once with the coordinates omitted, and one with every corner omitted
/// waits** (note 2).
#[test]
fn decefr_with_no_pointer_reports_at_once_or_waits_by_its_corners() {
    assert_eq!(reply(b"\x1b[1'z\x1b[1;1;5;5'w").0, show(b"\x1b[10;0&w"));
    assert_eq!(reply(b"\x1b[1'z\x1b[;;;5'w").0, show(b"\x1b[10;0&w"));
    assert_eq!(reply(b"\x1b[1'z\x1b['w").0, "", "waits for the pointer");
}

/// "Undefined values for Ps or Pu causes the entire sequence to be
/// ignored" (DECELR, p. 5): nothing enabled, and counted. xterm reads any
/// `Ps` but 1 and 2 as off and any `Pu` but 1 as cells.
#[test]
fn decelr_with_an_undefined_value_is_ignored_and_counted() {
    for bad in [&b"\x1b[3'z"[..], b"\x1b[1;3'z", b"\x1b[0;9'z"] {
        let mut v = bad.to_vec();
        v.extend_from_slice(b"\x1b['|");
        let (r, n) = reply(&v);
        assert_eq!(r, show(b"\x1b[0&w"), "{}", show(bad));
        assert_eq!(n, 1, "{}", show(bad));
    }
    // And an ignored DECELR 0 leaves an enabled locator enabled.
    assert_eq!(reply(b"\x1b[1'z\x1b[0;7'z\x1b['|").0, show(b"\x1b[1;0&w"));
}

/// **The locator is one of ctlseqs' mutually exclusive mouse protocols**:
/// a DECSET of an xterm one replaces it and a DECRST of any turns it off,
/// and DECELR 0 turns off whichever is in force, as xterm's single
/// `send_mouse_pos` does (Jev, ALL_OFF, 0.92 and 0.92).
#[test]
fn decelr_and_the_xterm_mouse_protocols_replace_each_other() {
    let rqm = b"\x1b[?1000$p";
    let on = show(b"\x1b[?1000;1$y");
    let off = show(b"\x1b[?1000;2$y");
    assert_eq!(reply(b"\x1b[?1000h\x1b[?1000$p").0, on);
    let v = [&b"\x1b[?1000h\x1b[1'z"[..], rqm].concat();
    assert_eq!(reply(&v).0, off, "DECELR 1 replaces ?1000");
    let v = [&b"\x1b[?1000h\x1b[0'z"[..], rqm].concat();
    assert_eq!(reply(&v).0, off, "DECELR 0 turns ?1000 off");
    assert_eq!(
        reply(b"\x1b[1'z\x1b[?1000h\x1b['|").0,
        show(b"\x1b[0&w"),
        "?1000 replaces the locator"
    );
    assert_eq!(
        reply(b"\x1b[1'z\x1b[?1002l\x1b['|").0,
        show(b"\x1b[0&w"),
        "a DECRST of any turns it off"
    );
}

/// "a single locator report (DECLRP) shall reset locator reporting mode to
/// disabled" (DECLRP note 1) after a one-shot DECELR -- a request's answer
/// and a filter's report as well as a button's.
#[test]
fn a_one_shot_decelr_is_disabled_by_its_first_report() {
    assert_eq!(
        reply(b"\x1b[2'z\x1b['|\x1b['|").0,
        show(b"\x1b[1;0&w\x1b[0&w")
    );
    assert_eq!(
        reply(b"\x1b[2'z\x1b[1;1;5;5'w\x1b['|").0,
        show(b"\x1b[10;0&w\x1b[0&w")
    );
}

/// **A DECLRP the reply buffer cannot take is dropped and counted, and a
/// one-shot DECELR waits for the next**: the program was told nothing.
#[test]
fn a_dropped_decrqlp_answer_leaves_a_one_shot_decelr_on() {
    support::with_term(&WS, |t| {
        t.set_locator_device(LocatorDevice::Present);
        support::feed(t, b"\x1b[2'z");
        let before = t.reply_overflows;
        while t.reply_overflows == before {
            support::feed(t, b"\x1b[5n");
        }
        let full = t.reply_overflows;
        support::feed(t, b"\x1b['|");
        assert_eq!(t.reply_overflows, full + 1, "dropped and counted");
        t.clear_reply();
        support::feed(t, b"\x1b['|\x1b['|");
        assert_eq!(show(t.reply()), show(b"\x1b[1;0&w\x1b[0&w"));
    });
}

/// DECRQLP's `Ps` is "0-(or omitted) Default to 1. 1-Transmit a single
/// locator report"; any other names no report, and is counted.
#[test]
fn decrqlp_with_an_undefined_ps_is_ignored_and_counted() {
    assert_eq!(reply(b"\x1b[1'z\x1b[2'|"), (String::new(), 1));
}

/// A DECSLE parameter the text does not give is counted, and the ones
/// beside it still apply (`tests/locator.rs` reads what they select).
#[test]
fn decsle_with_an_undefined_parameter_counts_it() {
    assert_eq!(reply(b"\x1b[5'{"), (String::new(), 1));
    assert_eq!(reply(b"\x1b[1;7;3'{"), (String::new(), 1));
}

/// `RIS` is power-up, "When disabled (the power-up default)"; DECSTR's
/// table (070-4 p. 31) does not name the locator, so it stays, as xterm's
/// soft reset keeps it.
#[test]
fn ris_disables_the_locator_and_decstr_keeps_it() {
    assert_eq!(reply(b"\x1b[1'z\x1b[!p\x1b['|").0, show(b"\x1b[1;0&w"));
    assert_eq!(reply(b"\x1b[1'z\x1bc\x1b['|").0, show(b"\x1b[0&w"));
}

/// **The locator's controls are not settings a query reads back**:
/// neither 070-5's DECRQSS list nor ctlseqs' names `' z`, `' {` or
/// `' w`, so DECRQSS answers them invalid, and DECELR is no mode, so
/// DECRQM has nothing to report.
#[test]
fn decrqss_does_not_read_the_locator_controls() {
    for q in [&b"'z"[..], b"'{", b"'w"] {
        let v = [&b"\x1b[1'z\x1bP$q"[..], q, b"\x1b\\"].concat();
        assert_eq!(reply(&v).0, show(b"\x1bP0$r\x1b\\"), "{}", show(q));
    }
}

/// A DECLRP is a control sequence the terminal sends, so S8C1T writes its
/// CSI as one byte.
#[test]
fn declrp_follows_s8c1t() {
    assert_eq!(reply(b"\x1b G\x1b['|").0, show(b"\x9b0&w"));
}

/// **The Locator Port's controls are out, and counted**: DECLKD, the
/// locator keys' strings for ReGIS and Tektronix input, and locator
/// controller mode, `CSI 6 i` and `CSI 7 i` (070-13 § 5.4, § 7), whose
/// row in `docs/vt-coverage.md` says why.
#[test]
fn declkd_and_locator_controller_mode_are_counted() {
    for q in [&b"\x1bP0$w1/1B5B/\x1b\\"[..], b"\x1b[6i", b"\x1b[7i"] {
        let (r, n) = reply(q);
        assert_eq!(r, "", "{}", show(q));
        assert!(n >= 1, "{} counted", show(q));
    }
}

/// **DECLRP is a report the terminal sends**, "To transmit locator
/// information to the host"; one the host sends baud is no control baud
/// acts on, and it is counted.
#[test]
fn a_received_declrp_is_counted() {
    assert_eq!(reply(b"\x1b[1;0;1;1&w"), (String::new(), 1));
}
