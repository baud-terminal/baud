//! What identifies the cell a mark belongs to.
//!
//! Three bugs lived here and all three were the same mistake: a mark was keyed
//! on `(Row, Col)`, which does not identify a cell. The grid rotates under a
//! screen row, the scrollback ring recycles the physical row beneath that, and
//! the alternate screen is a different buffer -- so the same `(Row, Col)` names
//! different cells over time and across screens.
//!
//! A fourth was a capacity chosen for orthography rather than for clusters.

use baud_core::{Col, OpMode, Row, Storage, StreamByte, Term};

fn feed(t: &mut Term, s: &str) {
    let b = s.as_bytes();
    let mut at = 0;
    while at < b.len() {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&b[at..]));
        let n = f.len();
        t.feed(base, n);
        t.commit_read(n);
        at += n.index();
    }
}

fn marks_at(t: &Term, r: usize, c: usize) -> Vec<u32> {
    t.marks(Row::from_index(r), Col::at_index(c))
        .map(|cp| cp.value())
        .collect()
}

/// A ZWJ family is one cluster of seven codepoints: a base plus six
/// attachments. The old capacity of three dropped half of them, and the cell
/// kept a glyph nobody sent.
#[test]
fn a_zwj_family_keeps_all_of_its_codepoints() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(
        &mut t,
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}",
    );
    assert_eq!(
        t.marks_dropped.count(),
        0,
        "a ZWJ family is seven codepoints; dropping any renders another glyph"
    );
}

/// The boundary case that hid the one above: this fits the old capacity
/// exactly, which is why a corpus of common sequences never caught it.
#[test]
fn a_flag_sequence_keeps_all_of_its_codepoints() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, "\u{1f3f3}\u{fe0f}\u{200d}\u{1f308}");
    assert_eq!(t.marks_dropped.count(), 0, "flag sequence lost codepoints");
}

/// A screen row names a different cell after the screen scrolls, so a mark
/// keyed on one outlives the text it belonged to.
#[test]
fn a_mark_does_not_outlive_the_cell_it_was_attached_to() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, "e\u{0301}");
    assert_eq!(
        marks_at(&t, 0, 0),
        vec![0x301],
        "the mark should start on row 0"
    );

    // Scroll the whole screen away: row 0 now holds a blank line, and the
    // original row 0 has been retired into scrollback.
    for _ in 0..50 {
        feed(&mut t, "\r\n");
    }
    let after = marks_at(&t, 0, 0);
    assert!(
        after.is_empty(),
        "row 0 is blank but its marks report {after:x?} -- the mark outlived \
         the cell, because the key named a screen position rather than a cell"
    );
}

/// The two screens are separate cell buffers and must not share marks.
#[test]
fn the_alternate_screen_does_not_see_the_primary_screen_marks() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, "e\u{0301}");
    assert_eq!(
        marks_at(&t, 0, 0),
        vec![0x301],
        "mark should be on the primary"
    );

    feed(&mut t, "\x1b[?1049h");
    let alt = marks_at(&t, 0, 0);
    assert!(
        alt.is_empty(),
        "the alternate screen is blank but its (0,0) reports {alt:x?} -- the \
         surface is part of a cell's identity"
    );
}

/// And the mark survives what should not disturb it.
#[test]
fn a_mark_survives_writes_elsewhere_on_the_screen() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, "e\u{0301}");
    feed(&mut t, " and more text on the same line");
    feed(&mut t, "\x1b[10;1Hother rows too");
    assert_eq!(
        marks_at(&t, 0, 0),
        vec![0x301],
        "writing elsewhere must not strand a mark"
    );
}
