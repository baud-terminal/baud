//! The ISO 8613-6 sub-parameter (colon) form of SGR colour.
//!
//! `[[source: ctlseqs]]` § "Character Attributes" gives two spellings of direct
//! colour and they are **not** the same shape:
//!
//! * `38 : 2 : Pi : Pr : Pg : Pb` — the ISO 8613-6 form, with a colour-space
//!   identifier `Pi` that "is ignored".
//! * `38 ; 2 ; Pr ; Pg ; Pb` — the konsole-compatible legacy form, no `Pi`.
//!
//! A parser that flattens colons into semicolons reads `Pi` as red and shifts
//! every channel by one. That is the bug this file exists to prevent, and it is
//! why `docs/protocol-decisions.md` calls the colon form "the one
//! implementations get wrong".
//!
//! Colour is compared by observing cells, so these cannot go vacuous if the
//! packing changes.

use crate::support;
use baud_core::*;

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    support::screen(&t).to_vec()
}

/// The six-part colon form skips `Pi` and lands the channels correctly.
///
/// If `Pi` were read as red, `38:2:0:10:20:30` would come out as rgb(0,10,20)
/// and differ from the legacy spelling of the same colour.
#[test]
fn colon_direct_colour_ignores_the_colour_space_id() {
    assert_eq!(
        grid(b"\x1b[38:2:0:10:20:30mx"),
        grid(b"\x1b[38;2;10;20;30mx"),
        "the colon form must mean the same colour as the legacy form"
    );
    // And a different Pi must not change the colour, since it is ignored.
    assert_eq!(
        grid(b"\x1b[38:2:1:10:20:30mx"),
        grid(b"\x1b[38:2:0:10:20:30mx")
    );
}

/// The five-part colon form has no `Pi`, and emitters do write it.
#[test]
fn colon_direct_colour_without_a_colour_space_id() {
    assert_eq!(grid(b"\x1b[38:2:10:20:30mx"), grid(b"\x1b[38;2;10;20;30mx"));
}

/// Indexed colour, both spellings.
#[test]
fn colon_indexed_colour_matches_the_legacy_form() {
    assert_eq!(grid(b"\x1b[38:5:9mx"), grid(b"\x1b[38;5;9mx"));
    assert_eq!(grid(b"\x1b[48:5:9mx"), grid(b"\x1b[48;5;9mx"));
}

/// Background works the same way.
#[test]
fn colon_form_applies_to_background() {
    assert_eq!(
        grid(b"\x1b[48:2:0:10:20:30mx"),
        grid(b"\x1b[48;2;10;20;30mx")
    );
    // fg and bg remain distinct
    assert_ne!(
        grid(b"\x1b[48:2:0:10:20:30mx"),
        grid(b"\x1b[38:2:0:10:20:30mx")
    );
}

/// The whole colon group is **one** attribute: a following semicolon-separated
/// parameter is still seen.
///
/// This is the consumed-count check. If the group were consumed one parameter
/// at a time, the trailing `1` would be swallowed and the bold would be lost.
#[test]
fn a_colon_group_is_one_attribute() {
    assert_eq!(
        grid(b"\x1b[38:2:0:10:20:30;1mx"),
        grid(b"\x1b[1;38;2;10;20;30mx"),
        "the bold after the group must still apply"
    );
    assert_eq!(
        grid(b"\x1b[38:5:9;1mx"),
        grid(b"\x1b[1;38;5;9mx"),
        "and after an indexed group"
    );
}

/// An attribute *before* a colon group is not disturbed by it.
#[test]
fn attributes_before_a_colon_group_survive() {
    assert_eq!(
        grid(b"\x1b[1;38:2:0:10:20:30mx"),
        grid(b"\x1b[1;38;2;10;20;30mx")
    );
}

/// Two colon groups in one SGR.
#[test]
fn two_colon_groups_in_one_sequence() {
    assert_eq!(
        grid(b"\x1b[38:2:0:10:20:30;48:5:9mx"),
        grid(b"\x1b[38;2;10;20;30;48;5;9mx")
    );
}

/// The colon form must parse identically however the read boundaries fall.
///
/// The fast CSI path and the byte-at-a-time machine accumulate parameters in
/// separate code, and a separator handled in only one of them is exactly the
/// silent-divergence bug this codebase has had before.
#[test]
fn colon_form_is_chunk_invariant() {
    let seq = b"\x1b[38:2:0:10:20:30;1mabc";
    let whole = grid(seq);
    for chunk in 1..seq.len() {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            t.write_at(BytePos::ZERO, &seq[i..i + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            i += n;
        }
        assert_eq!(
            support::screen(&t).to_vec(),
            whole,
            "differed at chunk {chunk}"
        );
    }
}
