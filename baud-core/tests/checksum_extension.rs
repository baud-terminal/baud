//! XTCHECKSUM, `CSI Ps # y`: xterm's bits that change how DECRQCRA sums.
//!
//! `[[source: ctlseqs]]`: "The bits of Ps modify the calculation of the
//! checksum returned by DECRQCRA: 0 -> do not negate the result. 1 -> do
//! not report the VT100 video attributes. 2 -> do not omit checksum for
//! blanks. 3 -> omit checksum for cells not explicitly initialized. 4 -> do
//! not mask cell value to 8 bits or ignore combining characters." xterm's
//! `xtermCheckRect` (`[[source: xterm-screen]]`) is the code; where the two
//! disagree each test says which it follows. Every test here but the
//! controls fails on an XTCHECKSUM that does nothing, which is what it was:
//! counted and dropped.
mod support;
use baud_core::{Owned, Term, WinSize};
use baud_domain::ChecksumReport;

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// DECRQCRA of row 1, columns 1 to `right`, after `bytes`: the sum's four
/// hex digits, or `None` for no reply; and the parser's dropped count.
fn sum_with(bytes: &str, right: u16, set: impl Fn(&mut Term<'_>)) -> (Option<u16>, usize) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    set(&mut t);
    support::feed(&mut t, bytes.as_bytes());
    t.clear_reply();
    support::feed(&mut t, format!("\x1b[7;1;1;1;1;{right}*y").as_bytes());
    let r = String::from_utf8(t.reply().to_vec()).expect("ASCII");
    let hex = r
        .strip_prefix("\x1bP7!~")
        .and_then(|r| r.strip_suffix("\x1b\\"))
        .map(|h| u16::from_str_radix(h, 16).expect("four hex digits"));
    (hex, t.unsupported)
}

fn sum(bytes: &str, right: u16) -> u16 {
    let (s, dropped) = sum_with(bytes, right, |_| {});
    assert_eq!(dropped, 0, "nothing dropped in {bytes:?}");
    s.expect("a reply")
}

fn neg(v: u16) -> u16 {
    v.wrapping_neg()
}

/// The control: **the default is `csDEC`**, the codes and weights negated.
#[test]
fn the_default_sum_is_decs() {
    assert_eq!(sum("AB", 2), neg(0x41 + 0x42));
    assert_eq!(sum("\x1b[1mA", 1), neg(0x41 + 0x80));
}

/// **Bit 0, "do not negate the result".**
#[test]
fn bit_0_does_not_negate() {
    assert_eq!(sum("\x1b[1#yAB", 2), 0x41 + 0x42);
}

/// **Bit 1, "do not report the VT100 video attributes"**: bold and
/// protection weigh nothing.
#[test]
fn bit_1_leaves_the_attributes_out() {
    assert_eq!(sum("\x1b[2#y\x1b[1m\x1b[1\"qA", 1), neg(0x41));
    assert_eq!(sum("\x1b[3#y\x1b[1mA", 1), 0x41);
}

/// **Bit 2, "do not omit checksum for blanks"**: an unwritten cell counts
/// as a space, weights included; the control is the default, which leaves
/// it out.
#[test]
fn bit_2_counts_an_unwritten_cell_as_a_space() {
    assert_eq!(sum("A", 3), neg(0x41));
    assert_eq!(sum("\x1b[4#yA", 3), neg(0x41 + 0x20 + 0x20));
}

/// **Bit 3, "omit checksum for cells not explicitly initialized"**, as
/// ctlseqs says: with bit 2 the unwritten cells are left out again and a
/// written space still counts. xterm's code reads bit 3 the other way,
/// "do not skip uninitialized cells" (Jev's lean, TEXT). Alone it changes
/// nothing, since the default already omits them.
#[test]
fn bit_3_omits_unwritten_cells_even_under_bit_2() {
    assert_eq!(sum("\x1b[12#yA ", 3), neg(0x41 + 0x20));
    assert_eq!(sum("\x1b[8#yA", 3), neg(0x41));
}

/// **Bit 4, "do not mask cell value to 8 bits"**: a wide character counts
/// its code point, where the default counts 0x1B, and a line-drawing cell
/// its Unicode value, where the default counts the byte received. U+FFFD
/// counts as itself, where xterm's `is_UCS_SPECIAL` skips it (Jev's lean,
/// COUNT).
#[test]
fn bit_4_counts_whole_code_points() {
    assert_eq!(sum("中", 2), neg(0x1b));
    assert_eq!(sum("\x1b[16#y中", 2), neg(0x4e2d));
    assert_eq!(sum("\x1b(0q", 1), neg(0x71));
    assert_eq!(sum("\x1b[16#y\x1b(0q", 1), neg(0x2500));
    assert_eq!(sum("\x1b[16#y\u{fffd}", 1), neg(0xfffd));
}

/// **Under bit 4 a weighted code wraps at sixteen bits**, as xterm adds
/// into an `int` and reports `checksum &= 0xffff`: a reverse U+FFE0 is
/// 0x10000, and a bold U+FFFD, what malformed UTF-8 renders, 0x1007D.
#[test]
fn bit_4_weights_wrap_at_sixteen_bits() {
    assert_eq!(sum("\x1b[16#y\x1b[7m\u{ffe0}", 1), neg(0x0000));
    assert_eq!(sum("\x1b[16#y\x1b[1m\u{fffd}", 1), neg(0x007d));
    assert_eq!(
        sum("\x1b[16#y\x1b[1m\u{ff00}", 1),
        neg(0xff80),
        "the control"
    );
}

/// **Combining marks count under bit 2 and not under bit 4**, as xterm's
/// code adds them (Jev, CODE, 0.94/0.98); the default leaves them out.
#[test]
fn marks_count_under_bit_2_and_not_bit_4() {
    let e = "e\u{301}";
    assert_eq!(sum(e, 1), neg(0x65));
    assert_eq!(sum(&format!("\x1b[4#y{e}"), 1), neg(0x65 + 0x301));
    assert_eq!(sum(&format!("\x1b[20#y{e}"), 1), neg(0x65));
}

/// **A bare `Ps` is 0, and bits above 4 are ignored**, as xterm's code
/// reads only bits 0 to 4 (Jev's lean, IGNORE_HIGH).
#[test]
fn a_bare_ps_is_0_and_high_bits_are_ignored() {
    assert_eq!(sum("\x1b[1#y\x1b[#yA", 1), neg(0x41));
    assert_eq!(sum("\x1b[32#yA", 1), neg(0x41));
    assert_eq!(sum("\x1b[33#yA", 1), 0x41);
    assert_eq!(sum("\x1b[257#yA", 1), 0x41, "bit 8 and bit 0");
}

/// **`RIS` and `DECSTR` put it back to the default**, "Reset checksum
/// extension to the checksumExtension resource" (`[[source: xterm-man]]`).
#[test]
fn both_resets_restore_the_default() {
    assert_eq!(sum("\x1b[1#y\x1bcA", 1), neg(0x41));
    assert_eq!(sum("\x1b[1#y\x1b[!pA", 1), neg(0x41));
}

/// **Below Level 4 it is ignored and counted**, xterm's `vtXX_level >= 4`
/// (Jev's lean, GATE), and so is the request: DECRQCRA is "Level: 4"
/// (`[[source: dec-std-070]]` p. 5-179), so at Level 3 it gets no answer.
#[test]
fn below_level_4_it_is_ignored_and_counted() {
    let (s, dropped) = sum_with("\x1b[63;1\"p\x1b[1#yA", 1, |_| {});
    assert_eq!(s, None, "no DECRQCRA at Level 3");
    assert_eq!(dropped, 2, "XTCHECKSUM and DECRQCRA");
    assert_eq!(
        sum("\x1b[64;1\"p\x1b[1#yA", 1),
        0x41,
        "the control: Level 4"
    );
}

/// **With the checksum off it is still honoured**, as xterm gates
/// `SetChecksum` apart from `GetChecksum` (Jev's lean, ALWAYS): no reply
/// then, and the sum it chose once the checksum is on.
#[test]
fn with_the_checksum_off_it_is_honoured_and_nothing_is_answered() {
    let (s, dropped) = sum_with("\x1b[1#yA", 1, |t| {
        t.set_checksum_report(ChecksumReport::Off)
    });
    assert_eq!((s, dropped), (None, 0));
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.set_checksum_report(ChecksumReport::Off);
    support::feed(&mut t, b"\x1b[1#yA");
    t.set_checksum_report(ChecksumReport::On);
    support::feed(&mut t, b"\x1b[7;1;1;1;1;1*y");
    assert_eq!(t.reply(), b"\x1bP7!~0041\x1b\\");
}
