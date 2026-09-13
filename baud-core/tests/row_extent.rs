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
        .iter()
        .rposition(|c| c.content != Content::EMPTY)
        .map_or(0, |p| p + 1)
}

/// Every mark is an upper bound on the glyphs in its row, on every recording.
#[test]
fn the_mark_is_never_smaller_than_the_text_it_covers() {
    for name in [
        "ascii/asciiquarium.raw",
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
