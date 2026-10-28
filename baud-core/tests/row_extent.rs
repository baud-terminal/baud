#![deny(dead_code)]
//! **The used-extent mark is never smaller than the truth.**
//!
//! `RowExtent` is a per-physical-row high-water mark: how far glyphs have
//! reached since the row was last blanked. It exists so that blanking a
//! scrolled row can stop at the mark instead of writing the full width, which
//! `MEASUREMENTS.md` measures as **72-82%** of all blanking on every real
//! scrolling recording.
//!
//! The direction of error is the entire safety argument. A mark that is too
//! **large** costs a few stores. A mark that is too **small** leaves stale text
//! on screen, silently, in a way no other test here would notice -- so that is
//! what these check, and they check it as an inequality rather than an
//! equality, because over-reporting is allowed by design.
//!
//! This file exists as its own gate because the mark is maintained in eleven
//! places -- every mutator on `View`, both line moves, and `Term::viewport`'s
//! un-rotation -- and `CLAUDE.md` records the failure "one representation" precisely: "an
//! invariant to maintain at every write site, where a site that forgets leaves
//! stale text on screen rather than merely being slow".

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

fn feed(t: &mut Term, bytes: &[u8], chunk: usize) {
    // **The chunk is the point of this file**, so it drives the reads itself
    // rather than taking `support::feed`'s fixed one: the extent has to come
    // out the same however the reads fell.
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(READ_CHUNK).min(bytes.len() - i);
        t.write_at(BytePos::ZERO, &bytes[i..i + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        i += n;
    }
}

/// The rightmost column of screen row `y` holding a glyph, as a count.
fn true_extent(t: &Term, y: usize) -> usize {
    t.row(Row::from_index(y))
        .expect("a row on the grid")
        .iter()
        .rposition(|c| c.content != Content::EMPTY)
        .map_or(0, |p| p + 1)
}

/// Every mark is an upper bound on the glyphs in its row, on every recording.
#[test]
fn the_mark_is_never_smaller_than_the_text_it_covers() {
    for name in [
        "ascii/pipes.raw",
        "ascii/cat-scroll.raw",
        "ascii/git-log.raw",
        "ascii/less-pager.raw",
        "utf8/btop.raw",
        "utf8/vim-unicode.raw",
        "utf8/cat-cjk.raw",
        "utf8/less-cjk.raw",
    ] {
        let d = std::fs::read(format!(
            "{}/../tests/recordings/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect(name);
        {
            let ws = support::CORPUS;
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            feed(&mut t, &d, READ_CHUNK);
            let (_, rows) = t.geometry();
            for y in 0..rows.index() {
                let want = true_extent(&t, y);
                let got = t.row_extent(Row::from_index(y)).index();
                assert!(
                    got >= want,
                    "{name} in: row {y} has glyphs to column {want} \
                     but its mark says {got}. A mark below the truth is stale \
                     text on screen: blanking would stop short and leave the \
                     tail of the previous line visible."
                );
            }
        }
    }
}

/// The same, driven by the protocol rather than by capture, over the operations
/// that move or reset rows.
///
/// The recordings above are real but they are not adversarial: none of them
/// sets a scrolling region and then runs `IL`/`DL` inside it, which is where
/// the marks have to be permuted by hand. This script does.
#[test]
fn the_mark_survives_regions_line_moves_and_the_alternate_screen() {
    let mut b: Vec<u8> = Vec::new();
    for y in 0..50 {
        b.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
        b.extend_from_slice(format!("row{y:02} and some more text here").as_bytes());
    }
    for seq in [
        "\x1b[5;40r",             // a region
        "\x1b[10;1H\x1b[3L",      // IL inside it
        "\x1b[12;1H\x1b[2M",      // DL inside it
        "\x1b[4S",                // SU
        "\x1b[2T",                // SD
        "\x1b[1;50r",             // back to full screen
        "\x1b[50;1H\n\n\n",       // full-screen scroll (the rotation)
        "\x1b[1;1H\x1bM\x1bM",    // reverse index
        "\x1b[?1049h",            // into the alternate screen
        "\x1b[1;1HALT\x1b[30;1H", // write there
        "\n\n\n",                 // scroll there
        "\x1b[?1049l",            // and back
        "\x1b[20;1H\x1b[2P",      // DCH
        "\x1b[21;1H\x1b[4@",      // ICH
        "\x1b[22;1H\x1b[K",       // EL
    ] {
        b.extend_from_slice(seq.as_bytes());
    }

    // Chunked several ways: the mark is state, and state that depends on where
    // the read boundaries fell is the chunk-invariance bug this repo has
    // already had once.
    for chunk in [1, 3, 17, READ_CHUNK] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        feed(&mut t, &b, chunk);
        let (_, rows) = t.geometry();
        for y in 0..rows.index() {
            let want = true_extent(&t, y);
            let got = t.row_extent(Row::from_index(y)).index();
            assert!(
                got >= want,
                "chunk {chunk}: row {y} has glyphs to column {want}, mark says \
                 {got}"
            );
        }
    }
}

/// **The probe can fail.** Without this the two tests above would pass just as
/// well against a mark hard-wired to the full width, which proves nothing about
/// the maintenance and everything about `>=`.
#[test]
fn the_mark_is_actually_tighter_than_the_full_width() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed(&mut t, b"\x1b[1;1Hshort\x1b[2;1Halso short", READ_CHUNK);
    let (cols, _) = t.geometry();
    let e = t.row_extent(Row::ZERO).index();
    assert_eq!(e, 5, "five glyphs written, so the mark is five");
    assert!(
        e < cols.index(),
        "a mark equal to the width would make the other tests vacuous"
    );
    // And a blanked row goes back to nothing.
    feed(&mut t, b"\x1b[1;1H\x1b[2K", READ_CHUNK);
    assert_eq!(
        t.row_extent(Row::ZERO).index(),
        0,
        "erasing the whole line resets the mark, or blanking never gets cheaper"
    );
}

/// DECALN fills every row with `E`, through the same `fill_row` that blanking
/// uses. The mark follows the *content*, not the call: that row is full.
#[test]
fn decaln_fills_the_row_so_the_mark_is_full() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed(&mut t, b"\x1b#8", READ_CHUNK);
    let (cols, rows) = t.geometry();
    for y in 0..rows.index() {
        assert_eq!(
            t.row_extent(Row::from_index(y)).index(),
            cols.index(),
            "DECALN wrote `E` across row {y}, so its mark is the full width"
        );
    }
}

/// **A renderer reading the screen between scrolls does not shift the marks.**
///
/// `Term::viewport` is the one place that changes which physical row holds
/// which content without going through `Grid`: it un-rotates the buffer in
/// place with `rotate_left` and sets `rot` back to `NONE`. `RowExtent` is
/// indexed physically, so the marks have to rotate by the same amount in the
/// same call.
///
/// The other tests here never call `viewport`, so they pass unchanged with the
/// mark rotation deleted -- verified by doing exactly that. This one fails,
/// which is the only reason it exists: a renderer calls `viewport` once a
/// frame, between scrolls, and that is the ordinary case rather than an exotic
/// one.
#[test]
fn reading_the_viewport_between_scrolls_keeps_the_marks_aligned() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    // Scroll a few screenfuls so the rotation is non-zero, reading the screen
    // in between the way a renderer would.
    for round in 0..6 {
        for line in 0..17 {
            let text = format!("r{round}l{line} text on this line\r\n");
            feed(&mut t, text.as_bytes(), READ_CHUNK);
        }
        let vp = t.viewport();
        assert!(!vp.is_empty(), "the renderer read a frame");
    }
    let (_, rows) = t.geometry();
    for y in 0..rows.index() {
        let want = true_extent(&t, y);
        let got = t.row_extent(Row::from_index(y)).index();
        assert!(
            got >= want,
            "after a viewport read: row {y} has glyphs to column {want}, mark \
             says {got}. The marks did not rotate with the rows."
        );
    }
}

/// Where screen row `y` stops being uniform, measured: the first column from
/// which every cell to the live edge equals the row's last cell -- content,
/// rendition and flags, the whole eight bytes.
fn uniform_from(t: &Term, y: usize) -> usize {
    let row = t.row(Row::from_index(y)).expect("a row on the grid");
    let Some(last) = row.last() else { return 0 };
    row.iter().rposition(|c| c != last).map_or(0, |p| p + 1)
}

/// Feed `bytes` in reads of `chunk` and, after every read, require each screen
/// row's mark to reach at least where the row stops being uniform.
fn feed_checking(name: &str, t: &mut Term, bytes: &[u8], chunk: usize) {
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(READ_CHUNK).min(bytes.len() - i);
        t.write_at(BytePos::ZERO, &bytes[i..i + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        i += n;
        let (_, rows) = t.geometry();
        for y in 0..rows.index() {
            let want = uniform_from(t, y);
            let got = t.row_extent(Row::from_index(y)).index();
            assert!(
                got >= want,
                "{name}, reads of {chunk}, after byte {i}: row {y} is not \
                 uniform to the edge until column {want}, but its mark says \
                 {got}"
            );
        }
    }
}

fn recording(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect(name)
}

/// **Past its mark a row is uniform: every cell to the live edge is the row's
/// last cell.** That is what the mark means -- "where the row stops being
/// uniform", not where the glyphs are -- and the test above checks only its
/// weaker half, the glyphs. Two readers depend on the stronger one. `fill_row`
/// stops at the mark when the row already ends in the blank it would write,
/// which leans on it only for rows ending in that blank; the region repair's
/// `copy_row` copies only to the larger of two rows' marks and fills past it
/// from the source's last cell, which leans on it for every row, a coloured
/// or styled tail included. So this checks every cell's eight bytes, after
/// every read, on every recording, the mixed stream, and a script of the
/// operations that write a styled tail.
#[test]
fn every_row_is_uniform_past_its_mark() {
    let names = [
        "ascii/cat-scroll.raw",
        "utf8/btop.raw",
        "ascii/git-log.raw",
        "utf8/cat-cjk.raw",
        "ascii/pipes.raw",
        "utf8/vim-unicode.raw",
        "ascii/less-pager.raw",
        "utf8/less-cjk.raw",
        "utf8/cava.raw",
    ];
    for name in names {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        feed_checking(name, &mut t, &recording(name), READ_CHUNK);
    }
    // The mixed stream, in `ingest2`'s order: the one corpus payload that
    // scrolls a region anchored at the top with rows under it.
    let mix: Vec<u8> = names[..8].iter().flat_map(|n| recording(n)).collect();
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed_checking("mix", &mut t, &mix, READ_CHUNK);
}

/// The same over the operations that leave a styled or protected tail, and a
/// log pane scrolling over a dashboard of coloured rows, at four read sizes.
#[test]
fn a_styled_tail_leaves_every_row_uniform_past_its_mark() {
    let mut b: Vec<u8> = Vec::new();
    for y in 1..=50 {
        b.extend_from_slice(format!("\x1b[{y};1H\x1b[3{}mrow{y:02} text", y % 8).as_bytes());
    }
    for seq in [
        "\x1b[0m\x1b[3;1H\x1b[44m\x1b[K",         // EL in a colour, mid-row
        "\x1b[4;5H\x1b[41m\x1b[3X",               // ECH in a colour, inside the text
        "\x1b[5;200H\x1b[42m\x1b[1X",             // ECH at the last column
        "\x1b[6;3H\x1b[43m\x1b[2@",               // ICH in a colour
        "\x1b[7;3H\x1b[45m\x1b[2P",               // DCH in a colour
        "\x1b[0m\x1b[8;1H\x1b[1\"qprot\x1b[0\"q", // DECSCA-protected text
        "\x1b[46m\x1b[8;1H\x1b[?K",               // DECSEL spares it
        "\x1b[9;1H\x1b[?2K",                      // DECSEL the whole line
        "\x1b[10;198H\x1b[0m\u{65e5}",            // a wide character at the edge
        "\x1b[11;199H\u{65e5}",                   // one that cannot fit
        "\x1b[12;1He\u{301}\u{301}",              // a mark on a cell
        "\x1b[47m\x1b[13;1H\x1b[?1J",             // DECSED above
        "\x1b[0m\x1b[30;1Hshort\x1b[30;30H\x1b[41m\x1b[3X", // ECH past the text
        "\x1b[0m\x1b[1;20r\x1b[20;1H",            // a top region over the rest
    ] {
        b.extend_from_slice(seq.as_bytes());
    }
    for i in 0..60 {
        b.extend_from_slice(format!("\x1b[3{}mlog {i:02} line\r\n", i % 8).as_bytes());
    }
    for chunk in [1, 3, 17, READ_CHUNK] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        feed_checking("the styled-tail script", &mut t, &b, chunk);
        // **The probe sees a styled tail.** Row 30 holds `short text` and
        // three red blanks at columns 29-31, under the region and so repaired
        // on every scroll; a check that read glyphs alone would put it at 10.
        assert_eq!(uniform_from(&t, 29), 32, "reads of {chunk}");
    }
}

/// Screen row `y` as text, `.` for a cell nothing was written to.
fn shown(t: &Term, y: usize) -> String {
    let (cols, _) = t.geometry();
    t.row(Row::from_index(y)).expect("a row on the grid")[..cols.index()]
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => '.',
        })
        .collect()
}

/// A terminal 13 by 4 with room for 20 columns and no history, a prompt
/// marked (`OSC 133 ; A`) so a growth re-cuts nothing unless `output`, a
/// line of twenty that wraps at 13, then grown to 17: the wrapped row's mark
/// is the 13 it wrapped at. `then` gets it with the bytes after the growth.
fn grown_over_a_wrapped_row(output: bool, alt: bool, then: impl FnOnce(&mut Term)) {
    let room = WinSize {
        ws_col: 20,
        ws_row: 4,
    };
    let ws = WinSize {
        ws_col: 13,
        ws_row: 4,
    };
    let mut s = Owned::with_history(&room, &CacheGeometry::UNKNOWN, Depth::of(0));
    let mut t = Term::new(s.storage(), &ws);
    let mut b: Vec<u8> = Vec::new();
    if alt {
        b.extend_from_slice(b"\x1b[?1049h");
    }
    if !output {
        b.extend_from_slice(b"\x1b]133;A\x07");
    }
    b.extend_from_slice(b"0123456789abcdefghij");
    feed(&mut t, &b, READ_CHUNK);
    assert_eq!(
        t.resize(&WinSize {
            ws_col: 17,
            ws_row: 4,
        }),
        Resized::Ok
    );
    then(&mut t);
}

/// **A wrapped row's mark is true after a growth at a prompt.** `XY`
/// written at columns 15 and 16 of the row that wrapped at 13 leaves it
/// uniform from 16, and its mark must say so after every read, at every
/// read size and every two-read split. A wrapped row's mark rose only by
/// `max` against a flagged word that no column can exceed, so it stayed at
/// 13 under the letters.
#[test]
fn a_wrapped_row_keeps_its_mark_true_after_a_growth_at_a_prompt() {
    let bytes = b"\x1b[1;15HXY";
    for read in 1..=bytes.len() {
        grown_over_a_wrapped_row(false, false, |t| {
            feed_checking("XY past a wrap at 13", t, bytes, read);
            assert_eq!(uniform_from(t, 0), 16, "the probe, reads of {read}");
        });
    }
    for cut in 1..bytes.len() {
        grown_over_a_wrapped_row(false, false, |t| {
            feed_checking("XY past a wrap at 13", t, &bytes[..cut], cut);
            feed_checking("XY past a wrap at 13", t, &bytes[cut..], bytes.len());
        });
    }
}

/// **And an erase reaches what was written past the old wrap**: `CSI 2 K`
/// and `CSI 2 J` stop at the mark when the row ends in the blank they write,
/// and left `XY` standing; `ICH` shifted `EF` past the mark, and a prompt's
/// redraw (a second `OSC 133 ; A`, which erases the block) left them under
/// a mark of zero. The controls: the same steps in the shell's output,
/// which re-cuts the line at 17 and marks its rows at the new width, and on
/// the alternate screen, whose growth blanks the columns it reveals and
/// marks every row at the old edge; both blank on the base too.
#[test]
fn an_erase_after_a_growth_at_a_prompt_reaches_a_wrapped_row_written_past_its_old_width() {
    let blank = ".".repeat(17);
    let mut wrong = Vec::new();
    for (output, alt) in [(false, false), (true, false), (false, true)] {
        for erase in [&b"\x1b[1;1H\x1b[2K"[..], b"\x1b[2J"] {
            grown_over_a_wrapped_row(output, alt, |t| {
                feed(t, b"\x1b[1;15HXY", READ_CHUNK);
                assert!(shown(t, 0).contains("XY"), "the control: {}", shown(t, 0));
                feed(t, erase, READ_CHUNK);
                if shown(t, 0) != blank {
                    wrong.push(format!(
                        "output {output}, alt {alt}, {erase:?}: {}",
                        shown(t, 0)
                    ));
                }
            });
        }
    }
    // The prompt's redraw over cells `ICH` moved past the mark.
    grown_over_a_wrapped_row(false, false, |t| {
        feed(t, b"\x1b[1;12H\x1b[3@", READ_CHUNK);
        assert_eq!(shown(t, 0), "0123456789a...bc.", "the control");
        feed(t, b"\x1b[1;1H\x1b]133;A\x07", READ_CHUNK);
        if shown(t, 0) != blank {
            wrong.push(format!("ICH, then a prompt redraw: {}", shown(t, 0)));
        }
    });
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **A `CR LF` in one read clears the wrap on the row it leaves**, as a
/// newline does (`RowExtent::WRAP`'s contract). Row 0 wraps at 10, then
/// `x CR LF` is written over its first cell: it ends there now, at every
/// read size and every two-read split. The fold that performs a `CR LF`
/// found in one read skipped the clear, so the mark said the row still
/// continued unless the two bytes fell in different reads.
#[test]
fn a_cr_lf_in_one_read_clears_the_wrap_on_the_row_it_leaves() {
    let bytes = b"abcdefghijklmno\x1b[Hx\r\n";
    let ws = WinSize {
        ws_col: 10,
        ws_row: 5,
    };
    let ended = |reads: &[&[u8]]| {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for r in reads {
            feed(&mut t, r, r.len());
        }
        t.view(Rows::ZERO).wrapped(Row::ZERO)
    };
    let mut wrong = Vec::new();
    for read in 1..=bytes.len() {
        let reads: Vec<&[u8]> = bytes.chunks(read).collect();
        if ended(&reads) != Wrap::Ended {
            wrong.push(format!("reads of {read}"));
        }
    }
    for cut in 1..bytes.len() {
        let (a, b) = bytes.split_at(cut);
        if ended(&[a, b]) != Wrap::Ended {
            wrong.push(format!("cut at {cut}"));
        }
    }
    assert!(wrong.is_empty(), "row 0 still continues: {wrong:?}");
}

/// One step of [`window_program`]: bytes, or a resize.
enum Step {
    Bytes(Vec<u8>),
    Resize(u16, u16),
}

/// `n` lines of the form `line i CR LF`.
fn lines(n: usize) -> Vec<u8> {
    (0..n)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect()
}

/// The program `history_window.rs`'s read-size property shrank to, less its
/// last wheel step: `lines(16)` at 12 columns leaves two rows that wrapped
/// and were then ended by a `CR LF`, and the re-cut at 20x24 joins any row
/// whose wrap survived to the next.
fn window_program() -> Vec<Step> {
    vec![
        Step::Bytes([&b"\x1b[10;1H"[..], &b"\n".repeat(10)].concat()),
        Step::Bytes(b"\x1b[3;7r\x1b[4H\x1b[2L\x1b[r".to_vec()),
        Step::Bytes(b"\x1b[?1049h".to_vec()),
        Step::Bytes(b"\x1b[?3J".to_vec()),
        Step::Resize(40, 8),
        Step::Resize(12, 10),
        Step::Bytes(b"\x1b[?47l".to_vec()),
        Step::Bytes(lines(16)),
        Step::Resize(20, 24),
    ]
}

/// What the program leaves: the history's depth, the cursor, and every
/// line from the oldest retained to the screen's last, as text.
fn run_window_program(read: impl Fn(usize, &[u8]) -> Vec<Vec<u8>>) -> (usize, Row, Vec<String>) {
    let room = WinSize {
        ws_col: 40,
        ws_row: 24,
    };
    let ws = WinSize {
        ws_col: 20,
        ws_row: 10,
    };
    let mut s = Owned::with_history(&room, &CacheGeometry::UNKNOWN, Depth::of(30));
    let mut t = Term::new(s.storage(), &ws);
    for (i, step) in window_program().iter().enumerate() {
        match step {
            Step::Bytes(b) => {
                for r in read(i, b) {
                    feed(&mut t, &r, r.len());
                }
            }
            Step::Resize(c, r) => {
                let _ = t.resize(&WinSize {
                    ws_col: *c,
                    ws_row: *r,
                });
            }
        }
    }
    let depth = t.scrollback().len();
    let (_, rows) = t.geometry();
    let text = (0..depth)
        .rev()
        .map(|b| t.view(Rows::of_count(b + 1)).row(Row::ZERO).to_vec())
        .chain((0..rows.index()).map(|y| t.row(Row::from_index(y)).expect("a row").to_vec()))
        .map(|cells| {
            cells
                .iter()
                .filter_map(|c| c.content.as_codepoint())
                .filter_map(|cp| char::from_u32(cp.value()))
                .collect::<String>()
        })
        .collect();
    (depth, t.cursor().0, text)
}

/// **The shrunk program comes out the same at every read size and every
/// two-read split**, and as a byte a read does: one line of history and
/// the cursor on row 23. Read with the `CR LF`s in one read, the two rows
/// kept their wrap, the re-cut joined them to the next, and the program
/// ended with no history and the cursor on row 22.
#[test]
fn the_shrunk_window_program_reads_the_same_at_every_read_size() {
    let one = run_window_program(|_, b| b.chunks(1).map(<[u8]>::to_vec).collect());
    assert_eq!((one.0, one.1), (1, Row::from_index(23)), "a byte a read");
    let longest = window_program()
        .iter()
        .map(|s| match s {
            Step::Bytes(b) => b.len(),
            Step::Resize(..) => 0,
        })
        .max()
        .unwrap_or(0);
    let mut wrong = Vec::new();
    for read in 2..=longest {
        let got = run_window_program(|_, b| b.chunks(read).map(<[u8]>::to_vec).collect());
        if got != one {
            wrong.push(format!("reads of {read}: {} {:?}", got.0, got.1));
        }
    }
    for (step, s) in window_program().iter().enumerate() {
        let Step::Bytes(bytes) = s else { continue };
        for cut in 1..bytes.len() {
            let got = run_window_program(|i, b| match i == step {
                true => vec![b[..cut].to_vec(), b[cut..].to_vec()],
                false => vec![b.to_vec()],
            });
            if got != one {
                wrong.push(format!("step {step} cut at {cut}: {} {:?}", got.0, got.1));
            }
        }
    }
    assert!(wrong.is_empty(), "{} differ: {wrong:#?}", wrong.len());
}

/// **A bare `LF` clears the wrap on the row it leaves**, with no `CR`
/// before it. Row 0 wraps at 10, then `x LF` is written over its first
/// cell: it ends there, at every read size and every two-read split.
/// `a_cr_lf_in_one_read_clears_the_wrap_on_the_row_it_leaves` reaches
/// `Paint::line_feed` only where a read splits the pair; this reaches
/// it at every read.
#[test]
fn a_bare_lf_clears_the_wrap_on_the_row_it_leaves() {
    let bytes = b"abcdefghijklmno\x1b[Hx\n";
    let ws = WinSize {
        ws_col: 10,
        ws_row: 5,
    };
    let ended = |reads: &[&[u8]]| {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for r in reads {
            feed(&mut t, r, r.len());
        }
        t.view(Rows::ZERO).wrapped(Row::ZERO)
    };
    let mut wrong = Vec::new();
    for read in 1..=bytes.len() {
        let reads: Vec<&[u8]> = bytes.chunks(read).collect();
        if ended(&reads) != Wrap::Ended {
            wrong.push(format!("reads of {read}"));
        }
    }
    for cut in 1..bytes.len() {
        let (a, b) = bytes.split_at(cut);
        if ended(&[a, b]) != Wrap::Ended {
            wrong.push(format!("cut at {cut}"));
        }
    }
    assert!(wrong.is_empty(), "row 0 still continues: {wrong:?}");
}
