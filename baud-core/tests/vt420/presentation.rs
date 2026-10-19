//! DECRQPSR, DECCIR, DECTABSR and DECRSPS: the presentation state reports
//! and their restore.
//!
//! `[[source: dec-std-070]]` pp. 5-198 to 5-205 and the VT510 pages
//! (`[[source: vt510-decrqpsr]]`, `[[source: vt510-deccir]]`,
//! `[[source: vt510-dectabsr]]`, `[[source: vt510-decrsps]]`). DECCIR
//! reports the cursor state DECSC saves: `Pr;Pc;Pp;Srend;Satt;Sflag;Pgl;Pgr;
//! Scss;Sdesig`, each flag byte `0x40` plus its bits. The default report is
//! STD 070's own example, `1;1;1;@;@;@;0;2;@;BB<<` (p. 5-203).

use crate::support;
use baud_core::{GSlot, Term};
use baud_domain::{Charset, Col, Row};

fn after<R>(bytes: &[u8], f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    support::render(&support::CORPUS, bytes, f)
}

fn cir(data: &str) -> Vec<u8> {
    format!("\x1bP1$u{data}\x1b\\").into_bytes()
}

/// The report a program asks for, after `prefix`.
fn report(prefix: &[u8]) -> Vec<u8> {
    let mut b = prefix.to_vec();
    b.extend_from_slice(b"\x1b[1$w");
    after(&b, |t| t.reply().to_vec())
}

fn ch(t: &Term<'_>, y: usize, x: usize) -> char {
    t.row(Row::from_index(y)).expect("a row")[x]
        .content
        .as_codepoint()
        .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
}

// ---- DECCIR, a field at a time -------------------------------------------------

/// STD 070's example of the default state, p. 5-203 read off the page:
/// home on page 1, no rendition, attribute or flag, G0 in GL and G2 in GR,
/// every set a 94-set, ASCII in G0 and G1 and the user-preference
/// supplemental set (`<`) in G2 and G3.
#[test]
fn the_default_report_is_std_070s_example() {
    assert_eq!(report(b""), cir("1;1;1;@;@;@;0;2;@;BB<<"));
}

#[test]
fn deccir_reports_the_cursor_position() {
    assert_eq!(report(b"\x1b[5;7H"), cir("5;7;1;@;@;@;0;2;@;BB<<"));
}

/// Srend: bit 1 bold, 2 underline, 3 blink, 4 reverse.
#[test]
fn deccir_reports_the_rendition() {
    assert_eq!(report(b"\x1b[1;4m"), cir("1;1;1;C;@;@;0;2;@;BB<<"));
    assert_eq!(report(b"\x1b[5;7m"), cir("1;1;1;L;@;@;0;2;@;BB<<"));
    assert_eq!(report(b"\x1b[4:3m"), cir("1;1;1;B;@;@;0;2;@;BB<<"));
}

/// Satt: bit 1, DECSCA's selectively erasable attribute.
#[test]
fn deccir_reports_the_selective_erase_attribute() {
    assert_eq!(report(b"\x1b[1\"q"), cir("1;1;1;@;A;@;0;2;@;BB<<"));
}

/// Sflag: bit 1 origin mode, bit 4 a pending wrap. The single-shift bits
/// are 0: the `CSI` that asks has cancelled any shift (070-3 p. 3-81).
#[test]
fn deccir_reports_origin_mode_and_a_pending_wrap() {
    assert_eq!(report(b"\x1b[?6h"), cir("1;1;1;@;@;A;0;2;@;BB<<"));
    let fill = vec![b'x'; 200];
    assert_eq!(report(&fill), cir("1;200;1;@;@;H;0;2;@;BB<<"));
    assert_eq!(report(b"\x1bN"), cir("1;1;1;@;@;@;0;2;@;BB<<"));
}

/// A pending wrap on a line that ends before the screen does reports the
/// line's last column, as CPR does: the right margin under DECSLRM.
#[test]
fn deccir_reports_a_pending_wrap_at_the_right_margin() {
    let mut b = b"\x1b[?69h\x1b[1;20s\x1b[1;1H".to_vec();
    b.extend_from_slice(&[b'x'; 20]);
    assert_eq!(report(&b), cir("1;20;1;@;@;H;0;2;@;BB<<"));
}

/// And the last column of a double-width line, half the screen's 200.
#[test]
fn deccir_reports_a_pending_wrap_at_a_double_width_lines_end() {
    let mut b = b"\x1b#6".to_vec();
    b.extend_from_slice(&[b'x'; 100]);
    assert_eq!(report(&b), cir("1;100;1;@;@;H;0;2;@;BB<<"));
}

/// Pgl and Pgr: the slots GL and GR invoke.
#[test]
fn deccir_reports_gl_and_gr() {
    assert_eq!(report(b"\x0e"), cir("1;1;1;@;@;@;1;2;@;BB<<"));
    assert_eq!(report(b"\x1bn\x1b|"), cir("1;1;1;@;@;@;2;3;@;BB<<"));
}

/// Scss: bit n+1 set when Gn holds a 96-character set.
#[test]
fn deccir_reports_the_set_sizes() {
    assert_eq!(report(b"\x1b.A"), cir("1;1;1;@;@;@;0;2;D;BBA<"));
    assert_eq!(report(b"\x1b-A\x1b/F"), cir("1;1;1;@;@;@;0;2;J;BA<F"));
}

/// Sdesig: each slot's designating tail, its second intermediate and final,
/// as it was designated (p. 5-202, "the User Preference Supplemental Set
/// (UPSS) designator if that is how a character set was designated").
#[test]
fn deccir_reports_each_designation_as_sent() {
    assert_eq!(report(b"\x1b(0\x1b)%5"), cir("1;1;1;@;@;@;0;2;@;0%5<<"));
    assert_eq!(report(b"\x1b(5\x1b*A"), cir("1;1;1;@;@;@;0;2;@;5BA<"));
}

/// Ps 0 is "ignored, no report sent" (p. 5-198), as is a value neither
/// text defines; eight-bit controls after S8C1T.
#[test]
fn decrqpsr_answers_only_one_and_two() {
    assert_eq!(
        after(b"\x1b[0$w\x1b[3$w\x1b[$w", |t| t.reply().to_vec()),
        b""
    );
    let want = [b"\x90".as_slice(), b"1$u1;1;1;@;@;@;0;2;@;BB<<", b"\x9c"].concat();
    assert_eq!(report(b"\x1b G"), want);
}

// ---- DECTABSR ----------------------------------------------------------------

/// The column of each stop, `/` between them (p. 5-204).
#[test]
fn dectabsr_lists_the_tab_stops() {
    let stops: Vec<String> = (9..=193).step_by(8).map(|c| c.to_string()).collect();
    let want = format!("\x1bP2$u{}\x1b\\", stops.join("/"));
    assert_eq!(after(b"\x1b[2$w", |t| t.reply().to_vec()), want.as_bytes());
    assert_eq!(
        after(b"\x1b[3g\x1b[5G\x1bH\x1b[2$w", |t| t.reply().to_vec()),
        b"\x1bP2$u5\x1b\\"
    );
    assert_eq!(
        after(b"\x1b[3g\x1b[2$w", |t| t.reply().to_vec()),
        b"\x1bP2$u\x1b\\"
    );
}

/// Every column a stop, the longest report this screen can give, is sent
/// whole.
#[test]
fn the_longest_dectabsr_is_sent_whole() {
    let mut prog = b"\x1b[3g".to_vec();
    for c in 1..=200 {
        prog.extend_from_slice(format!("\x1b[{c}G\x1bH").as_bytes());
    }
    prog.extend_from_slice(b"\x1b[2$w");
    let stops: Vec<String> = (1..=200).map(|c| c.to_string()).collect();
    let want = format!("\x1bP2$u{}\x1b\\", stops.join("/"));
    assert_eq!(after(&prog, |t| t.reply().to_vec()), want.as_bytes());
}

// ---- DECRSPS -----------------------------------------------------------------

fn rsps(ps: u8, data: &str) -> Vec<u8> {
    format!("\x1bP{ps}$t{data}\x1b\\").into_bytes()
}

#[test]
fn decrsps_restores_the_cursor_position() {
    let mut b = rsps(1, "5;7;1;@;@;@;0;2;@;BB<<");
    b.push(b'x');
    after(&b, |t| assert_eq!(ch(t, 4, 6), 'x'));
}

/// The four renditions Srend carries are set or cleared and the rest kept
/// (Jev's lean FOUR_ONLY, 0.80/0.70 then 0.84/0.86).
#[test]
fn decrsps_restores_the_four_renditions_and_keeps_the_rest() {
    let mut b = b"\x1b[31;5m".to_vec();
    b.extend(rsps(1, "1;1;1;C;@;@;0;2;@;BB<<"));
    b.extend_from_slice(b"\x1b[1$w");
    after(&b, |t| assert_eq!(t.reply(), cir("1;1;1;C;@;@;0;2;@;BB<<")));
    b.truncate(b.len() - 5);
    b.push(b'x');
    after(&b, |t| {
        let s = t.style_of(t.row(Row::ZERO).expect("row 0")[0]);
        assert!(s.attrs().has(baud_domain::Attrs::BOLD));
        assert_ne!(
            s.fg(),
            baud_domain::StyleWord::DEFAULT.fg(),
            "the colour stays"
        );
    });
}

#[test]
fn decrsps_restores_the_attribute_and_the_flags() {
    let mut b = rsps(1, "1;1;1;@;A;A;0;2;@;BB<<");
    b.extend_from_slice(b"\x1b[1$w");
    after(&b, |t| assert_eq!(t.reply(), cir("1;1;1;@;A;A;0;2;@;BB<<")));
    // A pending wrap at the last column: the next character wraps.
    let mut b = rsps(1, "1;200;1;@;@;H;0;2;@;BB<<");
    b.push(b'x');
    after(&b, |t| assert_eq!(ch(t, 1, 0), 'x'));
}

/// A pending single shift is restored (Jev's lean RESTORE_PENDING): the
/// next graphic character is drawn from G2, the one after from GL.
#[test]
fn decrsps_restores_a_pending_single_shift() {
    let mut b = rsps(1, "1;1;1;@;@;B;0;2;@;BB0<");
    b.extend_from_slice(b"qq");
    after(&b, |t| {
        assert_eq!(ch(t, 0, 0), '\u{2500}');
        assert_eq!(ch(t, 0, 1), 'q');
    });
}

/// GL, GR and the designations, where Scss says which series each final is
/// from: `A` with its bit set is ISO Latin-1 Supplemental, without it UK.
#[test]
fn decrsps_restores_the_slots_by_their_size() {
    let mut b = rsps(1, "1;1;1;@;@;@;1;3;D;B0AA");
    b.extend_from_slice(b"\x1b[1$w");
    after(&b, |t| assert_eq!(t.reply(), cir("1;1;1;@;@;@;1;3;D;B0AA")));
    let mut b = rsps(1, "1;1;1;@;@;@;1;3;D;B0AA");
    b.extend_from_slice(b"q\x1bNA\x1b%@\xa3");
    after(&b, |t| {
        assert_eq!(ch(t, 0, 0), '\u{2500}', "G1, line drawing, in GL");
        assert_eq!(ch(t, 0, 1), '\u{00c1}', "G2, Latin-1 Supplemental");
        assert_eq!(ch(t, 0, 2), '\u{00a3}', "G3, UK, in GR");
        assert_eq!(t.charset_in(GSlot::G3), Charset::Uk);
    });
}

/// An invalid value ends the restore there: the fields before it hold.
#[test]
fn decrsps_stops_at_an_invalid_value() {
    let mut b = rsps(1, "5;7;9;C;@;@;1;2;@;BB<<");
    b.push(b'x');
    after(&b, |t| {
        assert_eq!(ch(t, 4, 6), 'x', "row and column restored");
        assert!(
            !t.style_of(t.row(Row::from_index(4)).expect("row")[6])
                .attrs()
                .has(baud_domain::Attrs::BOLD)
        );
    });
}

/// DECTABSR's format: the stops listed, and only those.
#[test]
fn decrsps_restores_the_tab_stops() {
    let mut b = rsps(2, "5/10/40");
    b.extend_from_slice(b"\t\tx\tx\ty");
    after(&b, |t| {
        assert_eq!(ch(t, 0, 9), 'x');
        assert_eq!(ch(t, 0, 39), 'x');
        assert_eq!(ch(t, 0, 199), 'y', "no stop past 40");
        assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(199)));
    });
}

/// Every column a stop, a body longer than any string the core keeps.
#[test]
fn decrsps_restores_a_stop_at_every_column() {
    let stops: Vec<String> = (1..=200).map(|c| c.to_string()).collect();
    let mut b = rsps(2, &stops.join("/"));
    b.extend_from_slice(b"\x1b[2$w");
    let want = format!("\x1bP2$u{}\x1b\\", stops.join("/"));
    after(&b, |t| assert_eq!(t.reply(), want.as_bytes()));
}

/// **A report fed back restores the state it reported**, the programming
/// tip both VT510 pages give.
#[test]
fn a_report_fed_back_restores_what_it_reported() {
    let state = b"\x1b[3;4H\x1b[1;7m\x1b[1\"q\x1b)0\x1b.A\x0e\x1b}\x1b[3g\x1b[12G\x1bH\x1b[3;4H";
    let mut b = state.to_vec();
    b.extend_from_slice(b"\x1b[1$w\x1b[2$w");
    let (cir_r, tab_r) = after(&b, |t| {
        let r = t.reply().to_vec();
        let split = r.windows(2).position(|w| w == b"\x1b\\").expect("an ST") + 2;
        (r[..split].to_vec(), r[split..].to_vec())
    });
    let back = |r: &[u8]| {
        let mut v = r.to_vec();
        let at = v.iter().position(|&c| c == b'u').expect("u");
        v[at] = b't';
        v
    };
    let mut b = back(&cir_r);
    b.extend(back(&tab_r));
    b.extend_from_slice(b"\x1b[1$w\x1b[2$w");
    let again = after(&b, |t| t.reply().to_vec());
    assert_eq!(again, [cir_r, tab_r].concat());
}

/// **A restore takes effect at its terminator**: a `CAN`, an `ESC` or,
/// under ISO 8859-1, a C1 inside the body cancels the string, and the stops
/// stay what they were, as a cancelled Sixel leaves nothing.
#[test]
fn a_cancelled_tab_restore_leaves_the_stops() {
    let stops: Vec<String> = (9..=193).step_by(8).map(|c| c.to_string()).collect();
    let want = format!("\x1bP2$u{}\x1b\\", stops.join("/"));
    for cancel in [&b"\x18"[..], b"\x1b[m", b"\x1b%@\x1bP2$t5/1\x85"] {
        let mut b = b"\x1bP2$t5/1".to_vec();
        b.extend_from_slice(cancel);
        b.extend_from_slice(b"\x1b[2$w");
        after(&b, |t| assert_eq!(t.reply(), want.as_bytes(), "{cancel:?}"));
    }
}
