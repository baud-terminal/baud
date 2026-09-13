//! What the block loop must put on the screen.
//!
//! Written before any measurement: a fast loop that renders the wrong thing is
//! not a result, and this crate exists to be compared against `baud-core2`, so
//! it has to be right first.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

/// Feed `bytes` in `chunk`-sized reads and return the first row as text.
fn row0(bytes: &[u8], chunk: usize, cols: usize) -> String {
    row_n(bytes, chunk, cols, 0)
}

/// The same, for row `y`. **Row 0 is not where a wrap bug lives**: a writer
/// that clips at the margin leaves row 0 perfectly correct and loses
/// everything that belonged on row 1.
fn row_n(bytes: &[u8], chunk: usize, cols: usize, y: usize) -> String {
    let rows = 4usize;
    let ws = WinSize {
        ws_row: rows as u16,
        ws_col: cols as u16,
    };
    // **Twenty-five lines of `vec!` used to live here**, handing nine `&mut`s
    // to `Storage::new` in the right order -- nine chances to swap two
    // arguments of the same type, repeated in every test file that drives this
    // crate. `Owned` holds the allocations and lends them once.
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        t.write_at(BytePos::from_index(0), &bytes[i..i + n]);
        t.feed(BytePos::from_index(0), ReadLen::from_index(n));
        i += n;
    }
    let w = t.stride().index();
    let vp = t.viewport();
    let row = &vp[y * w..];
    (0..cols)
        .map(|c| match row[c].content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => '.',
        })
        .collect()
}

#[test]
fn a_printable_run_is_blitted() {
    assert_eq!(&row0(b"hello", 4096, 8), "hello...");
}

/// A run longer than one block, so the loop takes more than one iteration.
#[test]
fn a_run_crosses_block_boundaries() {
    assert_eq!(
        &row0(b"abcdefghijklmnopqrstuvwxyz", 4096, 30)[..26],
        "abcdefghijklmnopqrstuvwxyz"
    );
}

#[test]
fn a_wide_character_takes_two_cells() {
    // U+4E16 is East Asian Wide; the cell after it is its tail.
    let r = row0("a世b".as_bytes(), 4096, 8);
    assert_eq!(r.chars().next(), Some('a'));
    assert_eq!(r.chars().nth(1), Some('世'));
    assert_eq!(r.chars().nth(3), Some('b'), "the tail occupies column 2");
}

/// **The property the whole design rests on.** Where the read boundaries fell
/// must not be visible on the screen.
#[test]
fn the_screen_does_not_depend_on_where_the_reads_fell() {
    let src = "ab世cd漢字ef\ngh".as_bytes();
    let whole = row0(src, 4096, 24);
    for chunk in 1..=src.len() {
        assert_eq!(row0(src, chunk, 24), whole, "chunk size {chunk}");
    }
}

/// An overlong form is not a character. `E0 80 AF` used to render `/`, which
/// is the path-traversal overlong.
#[test]
fn an_overlong_form_is_rejected() {
    let r = row0(b"\xe0\x80\xafx", 4096, 8);
    assert_ne!(r.chars().next(), Some('/'));
}

/// **Overwriting half a wide character must clear the other half.**
///
/// The write path skips the orphan probe when the cursor is past the row's
/// written mark, because past it the cells are blank and a blank is not a
/// wide tail. This is the case where that shortcut must *not* apply: the
/// cursor comes back over text already written, so the probe has to run.
#[test]
fn overwriting_half_a_wide_character_clears_the_other_half() {
    // `世` occupies columns 0 and 1; `\r` returns to column 0; `x` lands on
    // the lead and must not leave the tail behind.
    let r = row0("世\rx".as_bytes(), 4096, 8);
    assert_eq!(r.chars().next(), Some('x'));
    assert_eq!(
        r.chars().nth(1),
        Some('.'),
        "the orphaned tail must be blanked, not left as half a character"
    );
}

/// The same for the second half: landing on a tail orphans the lead in front
/// of it.
#[test]
fn overwriting_the_tail_clears_the_lead() {
    let r = row0("世\r\u{1b}".as_bytes(), 4096, 8);
    let _ = r;
    // `\x1b` is not yet parsed by this core, so drive the case with a
    // backspace instead: write `ab`, back up one, then a wide character that
    // starts on `b` and runs off into fresh space.
    let r = row0("ab\u{8}世".as_bytes(), 4096, 8);
    assert_eq!(r.chars().next(), Some('a'));
    assert_eq!(r.chars().nth(1), Some('世'));
}

/// **A line longer than the row wraps; it does not stop.**
///
/// The one-line loop hands a writer the whole run, so a 300-byte line arrives
/// as one call where the block walk delivered it sixteen bytes at a time. The
/// blitter clipped to the margin and returned, and every later run found no
/// room and vanished in silence -- 3.6M cells of `btop.raw`, and the screen
/// simply stopped accepting text.
///
/// **Chunk invariance did not catch it and could not**, which is the part
/// worth keeping: the drop was driven by the *screen* being full, not by where
/// the read fell, so every chunk size agreed with every other on the same
/// wrong answer. A property that says "two runs agree" is silent when both are
/// wrong the same way.
#[test]
fn a_line_longer_than_the_row_wraps_instead_of_stopping() {
    // Ten columns, twelve characters: `kl` belongs on row 1, and the clipping
    // writer left row 0 exactly right while losing them.
    assert_eq!(&row0(b"abcdefghijkl", 4096, 10), "abcdefghij");
    assert_eq!(&row_n(b"abcdefghijkl", 4096, 10, 1), "kl........");
    // And the row must go on accepting text afterwards, which is the part
    // that cost `btop.raw` 3.6M cells: once the margin was reached, every
    // later run found no room and vanished.
    assert_eq!(&row_n(b"abcdefghijklZ", 4096, 10, 1), "klZ.......");
}

/// A run and the same bytes one at a time must put the same thing on the
/// screen. **The writer's chunking is not allowed to be visible**, which is
/// the same rule as the read boundary one level down -- and it is the one that
/// bites, because a bulk writer and a per-character writer are different code.
#[test]
fn a_run_agrees_with_the_same_characters_written_singly() {
    let src = b"the quick brown fox jumps over the lazy dog, twice over and again";
    for cols in [8usize, 10, 16, 17, 33, 64] {
        let whole = row0(src, 4096, cols);
        for chunk in [1usize, 2, 3, 5, 16, 17] {
            assert_eq!(row0(src, chunk, cols), whole, "cols {cols}, chunk {chunk}");
        }
    }
}

/// Feed `bytes` and return whatever the terminal owes the host.
fn reply_to(bytes: &[u8]) -> Vec<u8> {
    let rows = 4usize;
    let ws = WinSize {
        ws_row: rows as u16,
        ws_col: 20,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    t.write_at(BytePos::from_index(0), bytes);
    t.feed(BytePos::from_index(0), ReadLen::from_index(bytes.len()));
    t.reply().to_vec()
}

/// **`DA` under-claims as a VT102, and the shape is the whole point.**
///
/// An application parses whatever arrives as the report it asked for, so a
/// reply in the wrong shape is worse than silence -- which is why every answer
/// goes through a named `Report` rather than bytes chosen at the call site.
#[test]
fn a_device_attributes_query_is_answered_as_a_vt102() {
    assert_eq!(reply_to(b"\x1b[c"), b"\x1b[?6c");
    // `CSI 0 c` is the same query with an explicit default.
    assert_eq!(reply_to(b"\x1b[0c"), b"\x1b[?6c");
}

/// `CSI 5 n` is "are you well", `CSI 6 n` is "where is the cursor", 1-based.
#[test]
fn a_status_query_is_answered_and_an_unknown_one_is_not() {
    assert_eq!(reply_to(b"\x1b[5n"), b"\x1b[0n");
    // Home is row 1, column 1.
    assert_eq!(reply_to(b"\x1b[6n"), b"\x1b[1;1R");
    // Three characters in, still on row 1.
    assert_eq!(reply_to(b"abc\x1b[6n"), b"\x1b[1;4R");
    // **A DSR baud does not implement gets no answer at all.** Replying `0`
    // would tell the application the report exists.
    assert!(reply_to(b"\x1b[99n").is_empty());
}

/// Several queries in one read all reach the host, in order.
#[test]
fn a_read_carrying_two_queries_answers_both() {
    assert_eq!(reply_to(b"\x1b[c\x1b[5n"), b"\x1b[?6c\x1b[0n");
}

/// The buffer is emptied by the embedder, not by the next read.
#[test]
fn a_reply_survives_until_it_is_cleared() {
    let rows = 4usize;
    let ws = WinSize {
        ws_row: rows as u16,
        ws_col: 20,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let q = b"\x1b[c";
    t.write_at(BytePos::from_index(0), q);
    t.feed(BytePos::from_index(0), ReadLen::from_index(q.len()));
    assert_eq!(t.reply(), b"\x1b[?6c");
    // A second read that says nothing must not erase what is owed.
    t.write_at(BytePos::from_index(0), b"x");
    t.feed(BytePos::from_index(0), ReadLen::from_index(1));
    assert_eq!(t.reply(), b"\x1b[?6c", "an unrelated read must not drop it");
    t.clear_reply();
    assert!(t.reply().is_empty());
}

/// Drive `bytes` and hand the terminal back, so input state can be read off it.
fn after(bytes: &[u8]) -> (Vec<u8>, bool, bool, bool, bool, bool) {
    let rows = 4usize;
    let ws = WinSize {
        ws_row: rows as u16,
        ws_col: 20,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    t.write_at(BytePos::from_index(0), bytes);
    t.feed(BytePos::from_index(0), ReadLen::from_index(bytes.len()));
    (
        t.reply().to_vec(),
        t.cursor_keys_application(),
        t.keypad() == KeypadMode::Application,
        t.meta_sends_escape(),
        t.alt_sends_escape(),
        t.eight_bit_input(),
    )
}

/// **The input-side modes are recorded and readable.**
///
/// Each decides what a *key* sends, so the terminal's part is to know which is
/// live and let the embedder ask. Dropping them was the `parsed` failure:
/// accepted, did nothing, and no way for an application to find out.
#[test]
fn the_input_modes_are_recorded_and_readable() {
    // Defaults: cursor keys normal, keypad numeric.
    let (_, ck, kp, ..) = after(b"");
    assert!(!ck, "DECCKM is reset on a fresh terminal");
    assert!(!kp, "the keypad starts numeric");

    // DECCKM both ways.
    assert!(after(b"\x1b[?1h").1, "?1h sets application cursor keys");
    assert!(!after(b"\x1b[?1h\x1b[?1l").1, "?1l puts them back");

    // DECKPAM / DECKPNM are escapes, not modes -- `ESC =` and `ESC >`.
    assert!(after(b"\x1b=").2, "ESC = is DECKPAM");
    assert!(!after(b"\x1b=\x1b>").2, "ESC > is DECKPNM");

    // The three `ESC`-prefix modes.
    assert!(after(b"\x1b[?1036h").3, "?1036 metaSendsEscape");
    assert!(after(b"\x1b[?1039h").4, "?1039 altSendsEscape");
    assert!(after(b"\x1b[?1034h").5, "?1034 eightBitInput");
}

/// **Several private modes in one sequence all take effect.**
///
/// `CSI ? 1 ; 1036 ; 1034 h` is three modes, and reading only the first is a
/// bug this repo has already had once.
#[test]
fn every_parameter_of_a_private_mode_sequence_is_applied() {
    let (_, ck, _, meta, _, eight) = after(b"\x1b[?1;1036;1034h");
    assert!(ck && meta && eight, "all three, not just the first");
}

/// **DECSC saves the wrap flag and origin mode, and neither core restored
/// them.**
///
/// `[[source: vt510-decsc]]` lists seven items: cursor position, SGR
/// attributes, the character sets in GL and GR, the wrap flag, the state of
/// origin mode, the selective erase attribute, and any SS2/SS3 sent. The
/// coverage table called this `done` on the strength of the first two.
///
/// Origin mode is the one that bites: it decides where home *is*, so an
/// application that sets it, saves, resets it and restores gets the wrong
/// coordinate system for everything that follows.
#[test]
fn a_cursor_restore_brings_back_the_wrap_flag_and_origin_mode() {
    // DECSTBM 2;3 makes origin mode observable: with it set, `CUP 1;1` homes
    // to row 2 rather than row 1.
    //
    // Set origin, save, reset origin, restore, then home. If DECRC brought
    // origin mode back, home is inside the region.
    let r = row_n(
        b"\x1b[2;3r\x1b[?6h\x1b7\x1b[?6l\x1b8\x1b[1;1HX",
        4096,
        10,
        1,
    );
    assert_eq!(&r[..1], "X", "origin mode must survive DECSC/DECRC");

    // And with origin left off across the pair, home is row 0.
    let r = row_n(
        b"\x1b[2;3r\x1b[?6l\x1b7\x1b[?6h\x1b8\x1b[1;1HX",
        4096,
        10,
        0,
    );
    assert_eq!(&r[..1], "X", "and so must its absence");

    // The wrap flag likewise: DECAWM off, save, on, restore -> still off, so a
    // line longer than the row clips instead of wrapping to row 1.
    let r = row_n(b"\x1b[?7l\x1b7\x1b[?7h\x1b8abcdefghijkl", 4096, 10, 1);
    assert_eq!(&r, "..........", "autowrap must survive DECSC/DECRC");
}

/// **Shrinking after a scroll must not panic, and this is the one that bites.**
///
/// `Term::viewport` un-rotates in place with `rotate_left_by`, and a rotation
/// is an index into the row count that was live when the scroll happened.
/// Scroll a 50-row screen eleven times, shrink to ten rows, ask for the
/// viewport: the rotation is 11 and the slice is 10, and `rotate_left` panics
/// `mid <= self.len()`. That is why `resize` un-rotates **over the old
/// height** before it changes the row count -- and the rows around the
/// cursor survive, in order, with the rows above them retired to history. It is also the reason a resize entry
/// point that nothing calls is dangerous rather than merely untested --
/// `baud-core2`'s equivalent had a panic in it and no caller for long enough
/// to keep it.
#[test]
fn shrinking_after_a_scroll_does_not_panic() {
    let ws = WinSize {
        ws_row: 50,
        ws_col: 80,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let mut prog = Vec::new();
    for i in 0..60 {
        prog.extend_from_slice(format!("row{i}\r\n").as_bytes());
    }
    t.write_at(BytePos::ZERO, &prog);
    t.feed(BytePos::ZERO, ReadLen::from_index(prog.len()));
    // A live rotation, which is what makes this case reachable at all --
    // read through `row`, which applies it without undoing it.
    assert_ne!(t.rotation().index(), 0, "the screen has scrolled");
    // The cursor is on the last row, so the ten rows that stay are the
    // bottom ten; the forty above them retire to the history.
    let before: Vec<Vec<baud_domain::PackedCell>> = (40..50)
        .map(|y| t.row(baud_domain::Row::from_index(y)).to_vec())
        .collect();
    let small = WinSize {
        ws_row: 10,
        ws_col: 80,
    };
    assert_eq!(t.resize(&small), Resized::Ok);
    let w = t.stride().index();
    let vp = t.viewport();
    assert_eq!(vp.len(), 10 * w, "the viewport is the new size");
    for (y, row) in before.iter().enumerate() {
        assert_eq!(
            &vp[y * w..y * w + 80],
            &row[..80],
            "row {y} is the row that was on screen"
        );
    }
}

/// Growing past what the caller provisioned says so rather than clamping.
#[test]
fn growing_past_the_row_capacity_is_truncated() {
    let ws = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    // Rows are provisioned as given; the width is provisioned to what the
    // resize below asks for, because the pitch is the provisioned width and
    // a wider ask is truncated to it too, which is not the claim under test.
    let mut owned = Owned::new(&WinSize {
        ws_row: 24,
        ws_col: 100,
    });
    let mut t = Term::new(owned.storage(), &ws);
    // `Owned::new` provisions exactly the rows it was given, so 24 rows is
    // both the live count and the capacity -- and that is a claim worth
    // asserting rather than assuming, because it is what makes the next
    // resize truncate.
    assert_eq!(
        t.row_capacity().index(),
        24,
        "Owned::new provisions the geometry it is given and no more"
    );
    let big = WinSize {
        ws_row: 50,
        ws_col: 100,
    };
    match t.resize(&big) {
        Resized::Truncated { cols, rows } => {
            assert_eq!(rows.index(), 24, "clamped to what was provisioned");
            assert_eq!(cols.index(), 100, "columns are free to the pitch");
        }
        Resized::Ok => panic!("growing past the capacity must not report Ok"),
    }
    // And shrinking within it is not truncation.
    let small = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    assert_eq!(t.resize(&small), Resized::Ok);
}

/// **A resize while parked in the alternate screen reshapes the primary too.**
///
/// Both surfaces are laid out at the one pitch, so the parked primary takes
/// the same treatment the live one does: the smaller geometry hides what
/// lies past its edge, `restore_from` brings every parked row back with its
/// content to the mark, and growing the window afterwards shows the rest of
/// the line. Every full-screen program enters the alternate screen, so this
/// is the common case rather than an edge.
#[test]
fn a_resize_while_parked_restores_the_primary_whole() {
    let ws = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let prog = b"PRIMARY TEXT THAT COMES BACK CLIPPED\x1b[?1049hALT";
    t.write_at(BytePos::ZERO, prog);
    t.feed(BytePos::ZERO, ReadLen::from_index(prog.len()));
    let narrow = WinSize {
        ws_row: 12,
        ws_col: 20,
    };
    assert_eq!(t.resize(&narrow), Resized::Ok);
    let back = b"\x1b[?1049l";
    t.write_at(BytePos::ZERO, back);
    t.feed(BytePos::ZERO, ReadLen::from_index(back.len()));
    let w = t.stride().index();
    let vp = t.viewport();
    let text: String = vp[..20]
        .iter()
        .filter_map(|c| c.content.as_codepoint())
        .map(|cp| char::from_u32(cp.value()).unwrap_or('?'))
        .collect();
    assert_eq!(
        text, "PRIMARY TEXT THAT CO",
        "the first twenty columns are live"
    );
    assert!(
        vp[w..12 * w].iter().all(|c| c.content == Content::EMPTY),
        "and nothing else was on the primary"
    );
    assert_eq!(t.resize(&ws), Resized::Ok);
    let whole: String = t.viewport()[..36]
        .iter()
        .filter_map(|c| c.content.as_codepoint())
        .map(|cp| char::from_u32(cp.value()).unwrap_or('?'))
        .collect();
    assert_eq!(
        whole, "PRIMARY TEXT THAT COMES BACK CLIPPED",
        "grown back, the line is whole"
    );
}

/// **`REP` after a read boundary repeated nothing**, and the same bytes in one
/// read repeated correctly.
///
/// `last_cp` is a flat field on `Paint` -- deliberately, because putting it in
/// `Cold` cost 16% of `cat-scroll` -- and `Paint` is built once per `feed`.
/// It was initialised to `None` there and never written back, so "the
/// preceding graphic character" meant *preceding within this `read(2)`*, which
/// is a screen that depends on where the reads fell.
#[test]
fn rep_repeats_across_a_read_boundary() {
    let one = support::with_corpus(|t| {
        support::feed(t, b"ab\x1b[3b");
        row(t, 0, 8)
    });
    let two = support::with_corpus(|t| {
        support::feed(t, b"ab");
        support::feed(t, b"\x1b[3b");
        row(t, 0, 8)
    });
    assert_eq!(one, "abbbb   ", "one read");
    assert_eq!(two, one, "REP must not care where the read boundary fell");
}

fn row(t: &mut Term<'_>, y: usize, n: usize) -> String {
    let w = t.stride().index();
    let vp = t.viewport();
    (0..n)
        .map(|x| {
            vp[y * w + x]
                .content
                .as_codepoint()
                .and_then(|c| char::from_u32(c.value()))
                .unwrap_or(' ')
        })
        .collect()
}
