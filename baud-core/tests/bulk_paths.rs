//! **Every stride that has a bulk path must actually enter it.**
//!
//! This repo's own rule, written down long before this file existed: *"do not
//! read a bulk path as covering a character class -- measure how often it is
//! **entered**."* It was stated for the wide collector and then not applied to
//! the run scan, and the gap it left is the reason this file exists.
//!
//! **What it catches is invisible to every other kind of test.** `run_of` finds
//! a run of four-byte characters either way: through the vector scan if one
//! exists, and otherwise through the scalar loop underneath, which asks
//! `utf8_width` once per byte. The cells are byte-identical, chunk invariance
//! holds, every differential agrees -- and one of the two is four table lookups
//! a character where the other is one vector step. **No assertion about output
//! can separate them, so this asserts about entry.**
//!
//! `CLAUDE.md` records the defect being half-fixed: *"`Act::Char2` and
//! `Act::Char4` had no run scan at all ... Fixed by generalising `run_of_three`
//! to `run_of` over any stride."* The **scalar** loop was generalised. The
//! vector scan was not, and stayed stride-three-only while `Act::Char4` grew to
//! **37.8% of `cat-cjk.raw`'s iterations** -- the largest single action.

#![cfg(feature = "diag")]

mod support;

use baud_core::*;

/// `n` copies of a character of `len` encoded bytes, as one run.
fn run_of_width(len: usize, n: usize) -> Vec<u8> {
    let ch: &[u8] = match len {
        2 => "é".as_bytes(),  // U+00E9, two bytes, narrow
        3 => "終".as_bytes(), // U+7D42, three bytes, wide
        _ => "🚀".as_bytes(), // U+1F680, four bytes, wide
    };
    assert_eq!(ch.len(), len, "fixture character is not {len} bytes");
    ch.repeat(n)
}

/// **The scan must be entered for every stride a block can hold more than one
/// of**, and it was entered for exactly one of them.
///
/// Twenty characters is comfortably past the sixteen-byte block for all three
/// lengths, so a vector scan that exists cannot fail to be reached.
///
/// **And the corpus cannot reach stride four at all**, which is why this is a
/// written fixture rather than a recording. Counted over every `.raw` in
/// `tests/recordings/`: 13,722 four-byte runs in `cat-cjk.raw`, 3,267 in
/// `less-cjk.raw`, 1,122 in `vim-unicode.raw` -- and **zero** of length four or
/// more, which is what the vector step needs. Four-byte characters in real text
/// are emoji, and emoji arrive as ZWJ families, so a pictograph is followed by
/// a three-byte joiner rather than by another pictograph. Text that *would*
/// reach it exists -- CJK Extension B is four bytes a character -- and no
/// recording here contains any. Same shape as HT, SO/SI and `ESC ( 0`: a real
/// recording only covers what it happens to contain.
#[test]
fn stride_three_enters_its_bulk_path_and_stride_four_has_none() {
    // Stride three has a vector scan and a run past one block must reach it.
    let three = run_of_width(3, 20);
    let entered = support::with_term(&support::CORPUS, |t| {
        support::feed(t, &three);
        t.stride_scan[3]
    });
    assert!(
        entered > 0,
        "stride 3: the vector scan was never entered, so a run of twenty \
         three-byte characters went through the scalar loop. The run is still \
         found -- that is why no output test can see this."
    );

    // **Stride four has none, on purpose, and this pins the reason.** It was
    // built and reverted: entered zero times on all nine recordings, flat on
    // every one, and refused by `audit_all.sh` as a sixth scan loop against a
    // floor of five. Anyone adding one has to move that floor, and the floor
    // does not move to fit a change.
    let four = run_of_width(4, 20);
    let entered = support::with_term(&support::CORPUS, |t| {
        support::feed(t, &four);
        t.stride_scan[4]
    });
    assert_eq!(
        entered, 0,
        "stride 4 has a bulk path again. If that is deliberate, the corpus \
         still cannot reach it -- zero runs of four consecutive four-byte \
         characters in any recording -- so measure it on a payload that can, \
         and re-derive the scan-loop budget in the same commit."
    );
}

/// The counter is not measuring itself: a payload with no run of that stride
/// must leave it at zero, or `entered > 0` above would pass on anything.
#[test]
fn a_stride_with_no_run_does_not_enter_its_scan() {
    let ascii = b"plain ascii, no multi-byte characters at all".repeat(8);
    let entered = support::with_term(&support::CORPUS, |t| {
        support::feed(t, &ascii);
        (t.stride_scan[3], t.stride_scan[4])
    });
    assert_eq!(
        entered,
        (0, 0),
        "pure ASCII entered a multi-byte run scan, so the counter counts \
         something other than what it claims"
    );
}

/// **The bulk path and the scalar loop must agree**, which is what makes the
/// entry counter safe to act on: if they disagreed, entering the bulk path more
/// often would be a correctness change rather than a speed one.
#[test]
fn the_bulk_scan_finds_the_same_run_the_scalar_loop_would() {
    for len in [2usize, 3, 4] {
        for n in [1usize, 2, 3, 4, 5, 7, 12, 20] {
            let mut bytes = run_of_width(len, n);
            bytes.extend_from_slice(b"|");
            let cells = support::with_term(&support::CORPUS, |t| {
                support::feed(t, &bytes);
                let row = t.row(Row::from_index(0)).expect("a row on the grid");
                row.iter().filter(|c| c.content != Content::EMPTY).count()
            });
            // Wide characters take two cells and a tail; narrow take one.
            let per = if len == 2 { 1 } else { 2 };
            assert_eq!(
                cells,
                n * per + 1,
                "stride {len}, {n} characters: the run was not written whole"
            );
        }
    }
}
