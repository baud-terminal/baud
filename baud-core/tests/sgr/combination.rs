//! GRCM, ANSI mode 21 (`[[source: ecma48]]` 7.2.8): baud's SGR is
//! CUMULATIVE, "Each occurrence of the control function SELECT GRAPHIC
//! RENDITION (SGR) causes only those graphic rendition aspects to be
//! changed that are specified by that SGR", and has that one state, which
//! 7.1 allows ("some or all of the modes may have one state only").
//! `DECRQM` answers it permanently set; SM and RM change nothing and are
//! counted.

use crate::support;
use baud_core::*;
use baud_domain::Attrs;

/// The rendition of the cell at column `col` after `bytes`, and how many
/// parameters were counted as unsupported.
fn style_at(bytes: &[u8], col: usize) -> (StyleWord, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let cell = t.viewport()[col];
    (t.styles().one().word_of(cell), t.unsupported_param)
}

#[test]
fn a_second_sgr_changes_only_the_aspects_it_names() {
    let (w, _) = style_at(b"\x1b[1m\x1b[4mX", 0);
    assert!(w.attrs().has(Attrs::BOLD), "SGR 4 keeps the bold SGR 1 set");
    assert!(w.attrs().has(Attrs::UNDERLINE));
    let (w, _) = style_at(b"\x1b[1;4m\x1b[24mX", 0);
    assert!(
        w.attrs().has(Attrs::BOLD),
        "SGR 24 clears the underline alone"
    );
    assert!(!w.attrs().has(Attrs::UNDERLINE));
}

#[test]
fn rm_21_leaves_sgr_cumulative() {
    let (base, none) = style_at(b"\x1b[1m\x1b[4mX", 0);
    let (after, counted) = style_at(b"\x1b[21l\x1b[1m\x1b[4mX", 0);
    assert_eq!(after, base, "RM 21 does not select REPLACING");
    assert_eq!(counted, none + 1, "RM 21 counted");
}

#[test]
fn sm_21_leaves_sgr_cumulative() {
    let (base, none) = style_at(b"\x1b[1m\x1b[4mX", 0);
    let (after, counted) = style_at(b"\x1b[21h\x1b[1m\x1b[4mX", 0);
    assert_eq!(after, base);
    assert_eq!(counted, none + 1, "SM 21 counted");
}

#[test]
fn grcm_is_reported_permanently_set() {
    support::render(
        &support::CORPUS,
        b"\x1b[21$p\x1b[21l\x1b[21$p\x1b[21h\x1b[21$p",
        |t| {
            assert_eq!(t.reply(), b"\x1b[21;3$y\x1b[21;3$y\x1b[21;3$y");
            assert_eq!(t.unsupported_param, 2, "SM and RM 21 counted");
        },
    );
}
