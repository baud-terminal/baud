//! A control sequence the fast path declines is walked from clean parameters.
//!
//! `fast_csi` takes a whole `CSI` in one step or returns `None`, and the
//! graph then walks the same bytes from the `[`. It may only decline, never
//! disagree, so a decline must leave `csi` as `Act::CsiStart` found it. The
//! `two-pass-csi` arm (off by default, kept for A/B) declined at the byte
//! that ended its parameter walk without `csi.reset()`, so the graph pushed
//! the parameters a second time onto the ones already there: `CSI 2 ; 5 CR H`
//! became `CSI 2 ; 2 ; 5 H`, a CUP to row 2 column 2. The fused default
//! path resets on every decline.
//!
//! **What the texts say happens to the byte that made it decline.**
//! - A C0 control: `[[source: dec-std-070]]` 3.5.3 e (PDF p. 129): "The
//!   occurrence of bit combinations 0/0 to 1/15, inclusive, as well as 7/15
//!   and 15/15 in control sequences, are special conditions that are handled
//!   by immediately executing the control function indicated, and continuing
//!   processing of the control sequence as if the intervening control code
//!   had not been received." `[[source: williams]]` 534: "C0 controls are
//!   executed immediately during the recognition of a control sequence", and
//!   `csi param` executes `00-17,19,1C-1F` and ignores `7F` (315).
//! - A GR byte, under ISO 8859-1: STD 070 3.5.3 g, "Characters from columns
//!   10 to 15 (10/0 to 15/14 inclusive) may occur in control sequences and
//!   have the same interpretation as the character from the corresponding row
//!   in columns 02 to 07"; `[[source: ecma48]]` clause 9 the same of
//!   "Parameter Bytes, Intermediate Bytes, and Final Bytes of a control
//!   sequence" in an 8-bit code.
//! - `FF` under ISO 8859-1: STD 070 3.5.3 e lists "7/15 and 15/15" together,
//!   and baud ignores it as it does `DEL`.
//! - `80-FF` under UTF-8 is no control: a continuation byte or a lead,
//!   which cancels the sequence and is the ground's after a control, as
//!   after `ESC`, the owner's decision of 2026-10-13
//!   (`under_utf8_a_c1_byte_in_a_sequence_is_what_it_was`; the row "a byte
//!   `80-FF` inside a control sequence"). It was dropped and the sequence
//!   went on.
//!
//! Each case is held against the same control written outside the sequence,
//! or the sequence without the byte, at every read size and every split into
//! two reads. The arm stores parameters only when the whole sequence is in
//! one read: a shorter read finds no final byte and declines before it has
//! touched `csi`. So the reads that were red are the ones that hold the
//! sequence whole, and the rest are the chunk-invariance half of the claim.
//!
//! **These cases test the `two-pass-csi` arm only when it is built**:
//! `cargo nextest run -p baud-core --features two-pass-csi --test
//! csi_decline`. Under default features they hold the fused path, which was
//! already right.
mod support;
use baud_core::*;

fn fed<R>(bytes: &[u8], cuts: &[usize], f: impl FnOnce(&Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 40,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let mut from = 0;
    for &to in cuts.iter().chain([bytes.len()].iter()) {
        support::feed(&mut t, &bytes[from..to]);
        from = to;
    }
    f(&t)
}

/// `got` against `want` fed whole, at every read size and every two-read
/// split: the cursor, the screen, the bells and the unsupported count.
fn same_at_every_split(what: &str, got: &[u8], want: &[u8]) {
    fed(want, &[], |w| {
        let check = |at: &str, t: &Term<'_>| {
            assert_eq!(t.cursor(), w.cursor(), "{what}, {at}: cursor");
            assert_eq!(t.unsupported, w.unsupported, "{what}, {at}: unsupported");
            assert_eq!(t.bells, w.bells, "{what}, {at}: bells");
            assert!(
                support::screen(t) == support::screen(w),
                "{what}, {at}: screen"
            );
        };
        for size in 1..=got.len() {
            let cuts: Vec<usize> = (size..got.len()).step_by(size).collect();
            fed(got, &cuts, |t| check(&format!("read size {size}"), t));
        }
        for at in 1..got.len() {
            fed(got, &[at], |t| check(&format!("split at byte {at}"), t));
        }
    });
}

/// **A C0 control inside the parameters runs, and the sequence goes on.**
/// In a CUP the parameters are what a leak changes, and in an SGR, whose
/// repeated parameters would set the same rendition, the C0's own effect
/// shows: after the CR, `Y` overwrites `X`. `BEL` is counted.
#[test]
fn a_c0_inside_the_parameters_runs_and_the_sequence_goes_on() {
    same_at_every_split("CR in a CUP", b"X\x1b[2;5\rHY", b"X\r\x1b[2;5HY");
    same_at_every_split("BS in a CUP", b"X\x1b[2;5\x08HY", b"X\x08\x1b[2;5HY");
    same_at_every_split("BEL in a CUP", b"X\x1b[2;5\x07HY", b"X\x07\x1b[2;5HY");
    same_at_every_split("CR in an SGR", b"X\x1b[1;31\rmY", b"X\r\x1b[1;31mY");
    // Before the first parameter nothing has been stored, so a decline there
    // leaves nothing behind: a control, green on both trees.
    same_at_every_split("CR before a parameter", b"X\x1b[\r2;5HY", b"X\r\x1b[2;5HY");
}

/// **`DEL` inside the parameters is ignored**, williams' `csi param` row.
#[test]
fn del_inside_the_parameters_is_ignored() {
    same_at_every_split("DEL", b"X\x1b[2;5\x7fHY", b"X\x1b[2;5HY");
}

/// **Under ISO 8859-1 a GR parameter byte is its GL byte**: `B5` is `5` and
/// `BB` is `;`, each after a parameter has been stored.
#[test]
fn under_latin1_a_gr_parameter_byte_is_its_gl_byte() {
    same_at_every_split("GR digit", b"\x1b%@X\x1b[2;\xb5HY", b"\x1b%@X\x1b[2;5HY");
    same_at_every_split(
        "GR separator",
        b"\x1b%@X\x1b[2;3\xbb5HY",
        b"\x1b%@X\x1b[2;3;5HY",
    );
    same_at_every_split("FF", b"\x1b%@X\x1b[2;5\xffHY", b"\x1b%@X\x1b[2;5HY");
}

/// **Under UTF-8 a byte in `80-FF` inside the parameters cancels the
/// sequence**, a continuation byte or a lead, and is the ground's after a
/// control, so no parameter is dispatched and the `H` is text: held against
/// the same bytes after a `NUL`, which leads to that ground and does nothing
/// else.
#[test]
fn under_utf8_a_byte_80_ff_inside_the_parameters_cancels_the_sequence() {
    same_at_every_split("85 under UTF-8", b"X\x1b[2;5\x85HY", b"X\x00\x85HY");
    same_at_every_split(
        "C3 A9 under UTF-8",
        b"X\x1b[2;5\xc3\xa9HY",
        b"X\x00\xc3\xa9HY",
    );
    same_at_every_split("E0 under UTF-8", b"X\x1b[2;5\xe0HY", b"X\x00\xe0HY");
}

/// **A digit after an intermediate sends the sequence to `csi ignore`**, so
/// nothing is dispatched and no parameter can leak into one: a decline here
/// cannot be seen, and the case pins that it stays so.
#[test]
fn a_digit_after_an_intermediate_dispatches_nothing() {
    same_at_every_split("digit after SP", b"X\x1b[2;5 1HY", b"XY");
}

/// **A `:` in a sequence that defines no sub-parameters ignores the whole
/// sequence**, counted: `[[source: dec-std-070]]` 3.5.3.1, "If 3/10 is
/// received within a parameter string, the entire sequence up to and
/// including the final character shall be ignored", and `[[source:
/// williams]]`'s `csi ignore`. Each is held against a sequence baud counts
/// and does nothing with, `CSI < Ps H`. Until 2026-10-19 a parameter after
/// `:` was read as if `;` had come, so `CSI 2:5 H` went to row 2, column 5.
#[test]
fn a_colon_outside_sgr_ignores_the_sequence() {
    same_at_every_split("CUP", b"X\x1b[2:5HY", b"X\x1b[<HY");
    same_at_every_split("CUF", b"X\x1b[1:3CY", b"X\x1b[<HY");
    same_at_every_split("a leading colon", b"X\x1b[:3CY", b"X\x1b[<HY");
    same_at_every_split("DECSTBM", b"X\x1b[1:2;3rY", b"X\x1b[<HY");
    same_at_every_split("ED", b"X\x1b[2:1JY", b"X\x1b[<HY");
    same_at_every_split(
        "DECSET",
        b"X\x1b[?7:1l\x1b[1;40HYZ",
        b"X\x1b[<H\x1b[1;40HYZ",
    );
}

/// **SGR keeps its groups and XTMODKEYS its one sub-parameter**, the two
/// `[[source: ctlseqs]]` gives them: neither is refused by the rule above.
/// XTMODKEYS reads it as modifyOtherKeys' mask, which
/// `the_ignore_mask_factors_a_modifier_out` pins. The controls.
#[test]
fn sgr_and_xtmodkeys_keep_their_sub_parameters() {
    fed(b"\x1b[38:2::255:0:0mX\x1b[4:3mY", &[], |t| {
        assert_eq!(t.unsupported, 0);
        let row = t.row(Row::from_index(0)).expect("a row");
        assert_ne!(t.style_of(row[0]), StyleWord::DEFAULT, "38:2 applied");
        assert_ne!(t.style_of(row[1]), t.style_of(row[0]), "4:3 applied");
    });
    let plain = fed(b"\x1b[>4;2m", &[], |t| t.unsupported);
    let masked = fed(b"\x1b[>4;2:1m", &[], |t| t.unsupported);
    same_at_every_split(
        "XTMODKEYS",
        b"\x1b[>4;2:1mX",
        &[&b"\x1b[>4;2m"[..], b"X"].concat(),
    );
    assert_eq!(plain, 0, "the control: XTMODKEYS is acted on");
    assert_eq!(masked, 0, "its sub-parameter is no reason to refuse it");
}
