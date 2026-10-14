//! What identifies the cell a mark belongs to.
//!
//! Three bugs lived here and all three were the same mistake: a mark was keyed
//! on `(Row, Col)`, which does not identify a cell. The grid rotates under a
//! screen row, the scrollback ring recycles the physical row beneath that, and
//! the alternate screen is a different buffer -- so the same `(Row, Col)` names
//! different cells over time and across screens.
//!
//! A fourth was a capacity chosen for orthography rather than for clusters.

#[path = "support/mod.rs"]
mod support;

use baud_core::{Col, Owned, Row, Term};

fn feed(t: &mut Term, s: &str) {
    support::feed(t, s.as_bytes());
}

fn marks_at(t: &Term, r: usize, c: usize) -> Vec<u32> {
    t.marks(Row::from_index(r), Col::at_index(c)).collect()
}

/// A ZWJ family is one cluster of seven codepoints: a base plus six
/// attachments. The old capacity of three dropped half of them, and the cell
/// kept a glyph nobody sent.
#[test]
fn a_zwj_family_keeps_all_of_its_codepoints() {
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    feed(
        &mut t,
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}",
    );
    assert_eq!(
        t.marks_dropped, 0,
        "a ZWJ family is seven codepoints; dropping any renders another glyph"
    );
}

/// The boundary case that hid the one above: this fits the old capacity
/// exactly, which is why a corpus of common sequences never caught it.
#[test]
fn a_flag_sequence_keeps_all_of_its_codepoints() {
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    feed(&mut t, "\u{1f3f3}\u{fe0f}\u{200d}\u{1f308}");
    assert_eq!(t.marks_dropped, 0, "flag sequence lost codepoints");
}

/// A screen row names a different cell after the screen scrolls, so a mark
/// keyed on one outlives the text it belonged to.
#[test]
fn a_mark_does_not_outlive_the_cell_it_was_attached_to() {
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
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
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
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
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
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
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
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
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
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
/// guard in `Paint::attach` is about start-of-line, and the cursor parked at
/// the right margin must not read as one.
#[test]
fn a_mark_attaches_to_a_base_in_the_last_column() {
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    let cols = 200;
    feed(&mut t, &"a".repeat(cols - 1));
    feed(&mut t, "e\u{0301}");
    let d = t.marks_dropped;
    assert_eq!(marks_at(&t, 0, cols - 1), vec![0x301], "dropped={d}");
}

/// **Why the owning cell is resolved from the grid and not remembered.**
/// Nothing here is adjacent in the stream: the cursor writes a wide
/// character, leaves, comes back with `CUP`, and only then does a mark
/// arrive. A base position cached at write time would have been overwritten
/// by everything in between; the grid still knows which column holds a tail.
#[test]
fn a_mark_finds_its_base_after_the_cursor_has_been_away() {
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
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
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
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
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    feed(&mut t, &"a".repeat(198));
    feed(&mut t, "\u{1f468}\u{200d}\u{1f469}");
    let d = t.marks_dropped;
    assert_eq!(
        marks_at(&t, 0, 198),
        vec![0x200d, 0x1f469],
        "wide base at columns 198-199, marks_dropped={d}"
    );
}

/// The table a renderer resolves once a frame answers, slot for slot, what
/// `style_of` answers a cell at a time.
#[test]
fn the_style_table_is_what_style_of_reads() {
    let ws = baud_domain::WinSize {
        ws_col: 20,
        ws_row: 4,
    };
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    feed(&mut t, "\x1b[31;1mred\x1b[0m plain \x1b[44mblue");
    let cells: Vec<_> = t.viewport()[..14].to_vec();
    let table = t.styles();
    for cell in cells {
        let slot = table[cell.style.index()].unwrap_or(baud_domain::StyleWord::DEFAULT);
        assert_eq!(slot, t.style_of(cell));
    }
    // The three renditions the bytes minted are distinct slots.
    let ids: std::collections::BTreeSet<_> =
        t.viewport()[..14].iter().map(|c| c.style.index()).collect();
    assert_eq!(ids.len(), 3);
}

/// The rotated grid under its rotation is the viewport: what a shader
/// reading the cells in place has to compute, checked against the turn the
/// core makes for a reader that cannot.
#[test]
fn the_rotated_grid_under_its_rotation_is_the_viewport() {
    let ws = baud_domain::WinSize {
        ws_col: 20,
        ws_row: 5,
    };
    // The lent viewport is laid out at the storage's pitch, which is what a
    // GPU buffer would be sized by too -- not five rows of the maximum, the
    // hard-coded `MAX_COLS` pitch this repo records five porting bugs against.
    let mut st = Owned::without_surfaces(
        &ws,
        &baud_domain::CacheGeometry::UNKNOWN,
        baud_domain::Depth::NONE,
    );
    let ring = baud_domain::RowBlock::leading(st.ring(), st.stride());
    let screen = baud_domain::RowBlock::leading(st.rows(), st.stride());
    let mut vp = vec![baud_domain::PackedCell::EMPTY; ring.len()];
    let mut alt = vec![baud_domain::PackedCell::EMPTY; screen.len()];
    let mut t = Term::new(st.storage_over(&mut vp, &mut alt), &ws);
    // Eight lines on a five-row screen: three scrolls, a rotation of three.
    feed(&mut t, "a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh");
    let rot = t.rotation().index();
    assert_eq!(rot, 3);
    let stride = t.stride().index();
    let rows = 5usize;
    let ring = t.ring().index();
    assert_eq!(ring, rows, "no depth: the ring is the screen");
    let turned: Vec<_> = (0..rows)
        .flat_map(|y| {
            let p = (y + rot) % ring;
            t.cells()[p * stride..p * stride + stride].to_vec()
        })
        .collect();
    assert_eq!(turned, support::screen(&t));
    assert_eq!(
        t.rotation().index(),
        rot,
        "nothing is turned: the window is read where it is"
    );
}

/// A live rendition is not evicted from the style table by a colliding one.
///
/// The bug: `intern_style` overwrote a bucket on any hash collision, so a
/// foreground cell whose `StyleId` collided with a later background style
/// resolved to *that* style's word -- coloured box behind the text. 1024
/// buckets collide at even odds by ~38 distinct renditions, and a colour
/// chart followed by the 256-colour cube reproduced it every run. Open
/// addressing over a bounded window fixes it: a distinct style takes a free
/// slot rather than evicting a different live one. This paints the two and
/// asserts the foreground cells resolve to the same word before and after
/// the cube exists -- which fails on eviction and holds on the probe.
#[test]
fn a_colliding_rendition_does_not_evict_a_live_one() {
    let ws = baud_domain::WinSize {
        ws_col: 200,
        ws_row: 50,
    };
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    // Row 0: sixteen distinct foreground renditions, one glyph each.
    feed(&mut t, "\x1b[H");
    for c in 0..8 {
        feed(&mut t, &format!("\x1b[3{c}mX\x1b[0m"));
        feed(&mut t, &format!("\x1b[9{c}mY\x1b[0m"));
    }
    // What each of those cells resolves to, before any colliding style exists.
    let cells0: Vec<_> = t.viewport()[..16].to_vec();
    let before: Vec<_> = cells0.iter().map(|c| t.style_of(*c)).collect();
    // The 256-colour cube: 216 distinct background renditions, which under
    // eviction reuse some of row 0's buckets.
    feed(&mut t, "\r\n");
    for i in 16..=231 {
        feed(&mut t, &format!("\x1b[48;5;{i}m  \x1b[0m"));
    }
    // Row 0's cells must resolve to exactly what they did before.
    let cells1: Vec<_> = t.viewport()[..16].to_vec();
    let after: Vec<_> = cells1.iter().map(|c| t.style_of(*c)).collect();
    let wrong: Vec<_> = (0..16).filter(|&i| before[i] != after[i]).collect();
    assert!(
        wrong.is_empty(),
        "renditions evicted at columns {wrong:?}: a foreground cell now \
         resolves to a colliding style's background"
    );
    // And the sixteen are still sixteen distinct slots, none collapsed.
    let ids: std::collections::BTreeSet<_> =
        t.viewport()[..16].iter().map(|c| c.style.index()).collect();
    assert_eq!(ids.len(), 16, "two distinct renditions share a slot");
}

/// The style table is exact up to its capacity: distinct renditions each get
/// their own slot, and only the one past capacity evicts. Capacity is the
/// screen-sized [`Term::style_capacity`], at least `NSTYLE`.
///
/// This is the guarantee full-table open addressing buys over the bounded
/// window -- a distinct colour never displaces another while a slot is free.
/// It paints many distinct truecolour foregrounds, well past the ~38 where the
/// old direct-mapped table first collided and past the 16-slot window, and
/// asserts every cell still reports the colour it was written with.
#[test]
fn the_style_table_is_exact_below_capacity() {
    let ws = baud_domain::WinSize {
        ws_col: 200,
        ws_row: 50,
    };
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    // 900 distinct foregrounds, under capacity, over 5 rows of 200.
    // `u16` throughout so the channel values need no cast; `rgb` derives the
    // three from `i` the same way both when writing and when checking.
    const N: u16 = 900;
    let rgb = |i: u16| {
        (
            i & 0xff,
            i.wrapping_mul(5) & 0xff,
            i.wrapping_mul(11) & 0xff,
        )
    };
    feed(&mut t, "\x1b[H");
    for i in 0..N {
        let (r, g, b) = rgb(i);
        feed(&mut t, &format!("\x1b[38;2;{r};{g};{b}mX"));
        if (i + 1) % 200 == 0 {
            feed(&mut t, "\r\n");
        }
    }
    // Row by row, through the row accessor: the pitch is wider than the
    // 200-column screen, and the addressing is the core's to do.
    let cells: Vec<_> = (0..usize::from(N))
        .map(|i| {
            t.row(baud_domain::Row::from_index(i / 200))
                .expect("a row on the grid")[i % 200]
        })
        .collect();
    let mut wrong = 0usize;
    for (i, c) in cells.iter().enumerate() {
        let (r, g, b) = rgb(u16::try_from(i).unwrap());
        let want = baud_domain::ColorWord::rgb(
            baud_domain::Chan::from_offset(r, 0),
            baud_domain::Chan::from_offset(g, 0),
            baud_domain::Chan::from_offset(b, 0),
        );
        if t.style_of(*c).fg() != want {
            wrong += 1;
        }
    }
    assert_eq!(
        wrong, 0,
        "{wrong} of {N} distinct renditions report the wrong colour with \
         fewer than NSTYLE slots in use"
    );
}

/// `Term::marks` answers a screen position on the live screen. A reader
/// walking a view scrolled back -- the renderer's convert, the cell path's
/// overlay -- holds the cell itself, and the cell's cluster id and content
/// are the store's key, so `cluster_marks` answers for that cell wherever
/// on the ring it was read. Here the marked line is eleven rows above the
/// screen: the live position holds a blank row and reports nothing, and
/// the cell from the view scrolled back still carries its accent.
#[test]
fn a_mark_is_read_from_the_cell_of_a_view_scrolled_back() {
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    feed(&mut t, "e\u{0301}");
    for _ in 0..60 {
        feed(&mut t, "\n");
    }
    assert_eq!(
        marks_at(&t, 0, 0),
        Vec::<u32>::new(),
        "the live row 0 is blank"
    );
    let back = t.view(baud_domain::Rows::of_count(11));
    let cell = back.row(Row::from_index(0))[0];
    assert_eq!(
        t.cluster_marks(cell).collect::<Vec<u32>>(),
        vec![0x301],
        "the cell read from the view scrolled back carries its mark"
    );
}
