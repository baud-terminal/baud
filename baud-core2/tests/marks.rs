//! What identifies the cell a mark belongs to.
//!
//! Three bugs lived here and all three were the same mistake: a mark was keyed
//! on `(Row, Col)`, which does not identify a cell. The grid rotates under a
//! screen row, the scrollback ring recycles the physical row beneath that, and
//! the alternate screen is a different buffer -- so the same `(Row, Col)` names
//! different cells over time and across screens.
//!
//! A fourth was a capacity chosen for orthography rather than for clusters.

use baud_core2::{Col, OpMode, Row, Storage, StreamByte, Term};

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

/// **The bug this file's fourth entry is about.** A double-width base puts
/// its glyph in the lead cell and `WIDE_TAIL` in the next one. `cx.before()`
/// names the tail, so every ZWJ continuation was stored against a cell no
/// renderer asks about: nothing was dropped, nothing was counted wrong, and
/// the family emoji rendered as one person.
#[test]
fn a_wide_base_carries_its_marks_on_the_lead_cell() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, "\u{1f468}\u{200d}\u{1f469}");
    let lead = marks_at(&t, 0, 0);
    let tail = marks_at(&t, 0, 1);
    assert_eq!(
        lead,
        vec![0x200d, 0x1f469],
        "lead has {lead:x?}, tail has {tail:x?}"
    );
}

/// A cell rewritten in place must not keep the previous character's marks.
#[test]
fn overwriting_a_cell_does_not_inherit_its_old_marks() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, "e\u{0301}");
    assert_eq!(marks_at(&t, 0, 0), vec![0x301], "setup");
    // The space lands on column 0, replacing the `e` the accent belonged to.
    // Column 1 is where `x` goes, and never had marks -- asserting there is
    // the vacuous version of this test, which is what it said at first.
    feed(&mut t, "\r x");
    assert_eq!(
        marks_at(&t, 0, 0),
        Vec::<u32>::new(),
        "the space inherited the accent from the character it replaced"
    );
}

/// A base in the last column still owns what follows it. The `Col::ZERO`
/// guard in `attach_mark` is about start-of-line, and the cursor parked at
/// the right margin must not read as one.
#[test]
fn a_mark_attaches_to_a_base_in_the_last_column() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    let cols = 200;
    feed(&mut t, &"a".repeat(cols - 1));
    feed(&mut t, "e\u{0301}");
    let d = t.marks_dropped.count();
    assert_eq!(marks_at(&t, 0, cols - 1), vec![0x301], "dropped={d}");
}

/// **Why the owning cell is resolved from the grid and not remembered.**
/// Nothing here is adjacent in the stream: the cursor writes a wide
/// character, leaves, comes back with `CUP`, and only then does a mark
/// arrive. A base position cached at write time would have been overwritten
/// by everything in between; the grid still knows which column holds a tail.
#[test]
fn a_mark_finds_its_base_after_the_cursor_has_been_away() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, "\u{4e00}");
    feed(&mut t, "\r\nelsewhere\x1b[1;3H");
    feed(&mut t, "\u{0301}");
    assert_eq!(
        marks_at(&t, 0, 0),
        vec![0x301],
        "the mark belongs to the wide character at column 0"
    );
}

/// Asking about either half of a double-width character gives the same marks:
/// both columns are one character, and the marks belong to the character. A
/// renderer iterating cells never has to know which half it is looking at.
#[test]
fn either_half_of_a_wide_character_reports_its_marks() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, "\u{1f468}\u{200d}\u{1f469}");
    assert_eq!(marks_at(&t, 0, 1), marks_at(&t, 0, 0), "tail against lead");
    assert_eq!(marks_at(&t, 0, 0), vec![0x200d, 0x1f469]);
}

/// The two guards meet here: a wide base in the last two columns leaves the
/// cursor at the right margin, and the continuation has to resolve *back*
/// past a `WIDE_TAIL` from there. Covered by neither the lead test nor the
/// last-column test on its own.
#[test]
fn a_wide_base_at_the_right_margin_still_owns_what_follows_it() {
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    feed(&mut t, &"a".repeat(198));
    feed(&mut t, "\u{1f468}\u{200d}\u{1f469}");
    let d = t.marks_dropped.count();
    assert_eq!(
        marks_at(&t, 0, 198),
        vec![0x200d, 0x1f469],
        "wide base at columns 198-199, marks_dropped={d}"
    );
}
