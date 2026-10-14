//! An `ESC` abandons the escape or control sequence in progress.
//!
//! `[[source: williams]]`: `ESC` "will immediately cancel any escape
//! sequence, control sequence or control string in progress", its edge to
//! `escape` is on the *anywhere* list, and entering `escape` runs *clear*,
//! "so that erroneous sequences like CSI 3 ; 1 CSI 2 J are handled
//! correctly". baud's `CSI` nodes and its `Escape` node sent `ESC` to `c0()`,
//! which consumed it and stayed in the sequence: `CSI 1 ESC [ 2 m` dispatched
//! `CSI 1 [` and printed `2m`, and `ESC ( ESC 0` designated DEC graphics. The
//! `CAN`/`SUB` fix took the anywhere edge for `18` and `1A` and not for `1B`.
//!
//! **One test a node**, each red before the fix at its own assertion, fed at
//! read sizes 1, 3, 7 and whole: a read boundary between the `ESC` and what
//! follows is exactly where a half-applied rule would show, and both
//! differentials feed 4096-byte reads, which never cut these. No recording in
//! the corpus carries an `ESC` inside a sequence, which is why the fixtures
//! are written here (the HT and `CAN`/`SUB` precedent).
//!
//! The string nodes and VT52 keep their own `ESC`: in a control string it is
//! the first half of `ST`, and a VT52 escape restarts on a second `ESC` or
//! takes it as a coordinate. The controls at the end hold both.
mod support;
use baud_core::{Owned, Term};
use baud_domain::{Attrs, Col, Row};

/// The character at screen row `y`, column `x`: `' '` for a blank.
fn ch(t: &Term<'_>, y: usize, x: usize) -> char {
    t.row(Row::from_index(y)).expect("a row on the grid")[x]
        .content
        .as_codepoint()
        .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
}

/// Feed `bytes` at every read size from one byte up, and hand each terminal
/// to `f`.
fn each_read_size(bytes: &[u8], f: impl Fn(&Term<'_>)) {
    for chunk in [1, 3, 7, bytes.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&t);
    }
}

fn at(y: usize, x: usize) -> (Row, Col) {
    (Row::from_index(y), Col::at_index(x))
}

/// `CsiParam`: the example `docs/vt-coverage.md`'s anywhere block gives for
/// the `1B` row, `CSI 1 ESC [ 2 m`. The `SGR 1` that was abandoned applies nothing; the `SGR 2`
/// that follows is a sequence of its own.
#[test]
fn an_esc_in_the_parameters_starts_a_new_sequence() {
    each_read_size(b"\x1b[1\x1b[2mX", |t| {
        assert_eq!(ch(t, 0, 0), 'X', "the text after the new sequence");
        assert_eq!(ch(t, 0, 1), ' ', "`2m` is a sequence, not text");
        let x = t
            .style_of(t.row(Row::from_index(0)).expect("a row on the grid")[0])
            .attrs();
        assert!(x.has(Attrs::DIM), "`ESC [ 2 m` ran");
        assert!(!x.has(Attrs::BOLD), "the abandoned `CSI 1` applied nothing");
        assert_eq!(t.cursor(), at(0, 1));
    });
}

/// `CsiParam`, williams' own example: `CSI 3 ; 1 CSI H` is a `CUP` home, and
/// the half-parsed `3 ; 1` does not survive into it.
#[test]
fn williams_example_moves_home() {
    each_read_size(b"\x1b[5;5H\x1b[3;1\x1b[HX", |t| {
        assert_eq!(ch(t, 0, 0), 'X', "the `CUP` home ran");
        assert_eq!(ch(t, 4, 4), ' ', "nothing printed where the cursor was");
    });
}

/// `CsiEntry`: an `ESC` straight after `CSI`.
#[test]
fn an_esc_straight_after_csi_starts_a_new_sequence() {
    each_read_size(b"\x1b[\x1b[5;5HX", |t| {
        assert_eq!(ch(t, 4, 4), 'X', "the `CUP 5;5` ran");
        assert_eq!(ch(t, 0, 0), ' ', "`5;5H` is a sequence, not text");
    });
}

/// `CsiIgnore`: a sequence already broken by a marker out of place is still
/// abandoned by an `ESC`, not left to eat bytes up to the next final.
#[test]
fn an_esc_in_an_ignored_sequence_starts_a_new_one() {
    each_read_size(b"\x1b[5;5H\x1b[1?\x1b[HX", |t| {
        assert_eq!(ch(t, 0, 0), 'X', "the `CUP` home ran");
        assert_eq!(ch(t, 4, 4), ' ', "nothing printed where the cursor was");
    });
}

/// `CsiInter`, and the one that tells a reset from a mere move to `Escape`:
/// the `CSI`'s intermediate (`SP`) must not survive to be read as the escape
/// sequence's, or `ESC ( 0` would not designate.
#[test]
fn an_esc_after_an_intermediate_starts_a_new_sequence() {
    each_read_size(b"\x1b[ \x1b(0q", |t| {
        assert_eq!(ch(t, 0, 0), '\u{2500}', "`ESC ( 0` designated DEC graphics");
    });
}

/// `Escape`: an `ESC` inside an escape sequence restarts it, and the
/// intermediate collected before it is forgotten, so `ESC 0` alone is not a
/// designation.
#[test]
fn an_esc_in_an_escape_sequence_restarts_it() {
    each_read_size(b"\x1b(\x1b0q", |t| {
        assert_eq!(ch(t, 0, 0), 'q', "G0 is still ASCII");
    });
}

/// `Escape` again, with a final that means something alone: `ESC # ESC 8` is
/// `DECRC`, not `DECALN`. The cursor is saved at home first, because what
/// `DECRC` does with nothing saved is its own question (note AE, Q13).
#[test]
fn an_esc_after_hash_is_not_decaln() {
    each_read_size(b"\x1b7\x1b[5;5H\x1b#\x1b8X", |t| {
        assert_eq!(
            ch(t, 0, 0),
            'X',
            "`DECRC` restored the cursor saved at home"
        );
        assert_eq!(ch(t, 4, 4), ' ', "the screen was not filled with `E`");
        assert_eq!(ch(t, 10, 10), ' ', "the screen was not filled with `E`");
    });
}

/// **The controls.** In a control string an `ESC` is the first half of `ST`,
/// and in VT52 a second `ESC` restarts the escape: neither may change.
#[test]
fn strings_and_vt52_keep_their_own_esc() {
    each_read_size(b"\x1b]2;title\x1b\\", |t| {
        assert_eq!(t.title(), b"title", "`ESC \\` still ends an OSC");
    });
    each_read_size(b"\x1bP$qm\x1b\\", |t| {
        assert!(
            t.reply().starts_with(b"\x1bP1$r"),
            "`ESC \\` still ends a DECRQSS: {:?}",
            t.reply()
        );
    });
    each_read_size(b"\x1b[5;5H\x1b[?2l\x1b\x1bA", |t| {
        assert_eq!(t.cursor(), at(3, 4), "VT52 `ESC ESC A` moves up");
    });
}
