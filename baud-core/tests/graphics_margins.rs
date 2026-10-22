//! **What a scroll under side margins does to the pictures**: only the
//! character positions between the margins move (DECSLRM,
//! `[[source: dec-std-070]]` p. 5-24), and in the one bitmap of the VT330
//! and VT340 the pixels over them would move with them. The ReGIS page's
//! pixels in the rectangle move and the rest stay; a Sixel across a side
//! margin is split, the part inside moving and the rest staying (Jev's
//! lean, SPLIT, 0.71 and 0.71).
//!
//! Cells are 10 by 20 pixels throughout. The margins are columns 5 to 15
//! (cells 4 to 14) and lines 1 to 10.

use baud_core::*;
use baud_domain::{ImageBudget, ImagePx, Pixel};

mod support;

/// A 40 by 36 Sixel at 1:1, every pixel register 1's red: four columns
/// and two rows of cells. `P2` 1, so no background fill reaches past its
/// data.
const RED: &[u8] = b"\x1bP9;1q#1;2;100;0;0#1!40~-!40~-!40~-!40~-!40~-!40~\x1b\\";

/// A ReGIS fill over the page's upper-left corner: cells 0 to 19 of rows
/// 0 to 4.
const FILL: &[u8] = b"\x1bPpF(V[0,0][96,0][96,48][0,48])\x1b\\";

/// Side margins 5 to 15, lines 1 to 10.
const MARGINS: &[u8] = b"\x1b[?69h\x1b[5;15s\x1b[1;10r";

const CLEAR: u32 = 0;

fn term_run(budget: usize, bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(budget));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    support::feed(&mut t, bytes);
    f(&mut t);
}

/// Each placement: its line, column, pixels as bits and width, in line
/// then column order.
fn placed(t: &Term<'_>) -> Vec<(u64, usize, Vec<u32>, usize)> {
    let imgs = t.images();
    let mut v: Vec<_> = imgs
        .placements()
        .filter_map(|(_, p)| {
            let i = imgs.image(p.image)?;
            Some((
                p.line.value(),
                p.col.index(),
                i.pixels.iter().map(|px| px.bits()).collect::<Vec<_>>(),
                i.width.index(),
            ))
        })
        .collect();
    v.sort_by_key(|p| (p.0, p.1));
    v
}

/// **The page's pixels in the rectangle move and the rest stay**: SU 1
/// takes cells 4 to 14 of lines 2 to 10 up a line and clears line 10
/// there; cells 0 to 3 and 15 on, and every line below the region, are as
/// they were.
#[test]
fn a_rectangle_scroll_moves_the_page_only_inside_the_margins() {
    term_run(8 << 20, MARGINS, |t| {
        support::feed(t, FILL);
        let old = placed(t);
        assert_eq!(old.len(), 1, "one page");
        let (_, _, old, w) = old.into_iter().next().unwrap();
        support::feed(t, b"\x1b[S");
        let new = placed(t);
        assert_eq!(new.len(), 1, "still one page");
        let (_, _, new, _) = new.into_iter().next().unwrap();
        let at = |px: &[u32], x: usize, y: usize| px[y * w + x];
        for y in (0..240).step_by(5) {
            for x in (0..250).step_by(5) {
                let inside = (40..150).contains(&x) && y < 200;
                let want = match (inside, y < 180) {
                    (true, true) => at(&old, x, y + 20),
                    (true, false) => CLEAR,
                    (false, _) => at(&old, x, y),
                };
                assert_eq!(at(&new, x, y), want, "pixel ({x},{y})");
            }
        }
        assert!(
            (0..20).any(|y| at(&old, 45, y + 20) != CLEAR),
            "the control: the fill reaches the rows that move"
        );
    });
}

/// **A Sixel across the right margin is split**: drawn at cell 13 of line
/// 3, it covers cells 13 to 16 and the margin ends at cell 14. SU 1 moves
/// the part over cells 13 and 14 up a line as an image of its own; the
/// part over cells 15 and 16 stays, its first 20 columns of pixels
/// cleared.
#[test]
fn a_sixel_across_a_side_margin_is_split() {
    term_run(8 << 20, MARGINS, |t| {
        support::feed(t, b"\x1b[3;14H");
        support::feed(t, RED);
        let before = placed(t);
        assert_eq!(before.len(), 1, "the control, one image");
        let line = before[0].0;
        support::feed(t, b"\x1b[S");
        let after = placed(t);
        assert_eq!(after.len(), 2, "split in two");
        let (l1, c1, px1, w1) = &after[0];
        let (l2, c2, px2, w2) = &after[1];
        assert_eq!(
            (*l1, *c1, *w1),
            (line - 1, 13, 20),
            "the part inside, up a line"
        );
        assert!(px1.iter().all(|&p| p != CLEAR), "every pixel of it");
        assert_eq!((*l2, *c2, *w2), (line, 13, 40), "the rest, where it was");
        for y in 0..36 {
            for x in 0..40 {
                assert_eq!(px2[y * 40 + x] == CLEAR, x < 20, "the rest at ({x},{y})");
            }
        }
    });
}

/// **A Sixel wholly inside moves whole and one wholly outside stays**, as
/// before the split was built.
#[test]
fn a_sixel_inside_moves_and_one_outside_stays() {
    for (at, moves, what) in [
        (&b"\x1b[3;6H"[..], true, "inside"),
        (b"\x1b[3;17H", false, "outside"),
    ] {
        term_run(8 << 20, MARGINS, |t| {
            support::feed(t, at);
            support::feed(t, RED);
            let line = placed(t)[0].0;
            support::feed(t, b"\x1b[S");
            let after = placed(t);
            assert_eq!(after.len(), 1, "{what}: one image");
            let want = match moves {
                true => line - 1,
                false => line,
            };
            assert_eq!(after[0].0, want, "{what}");
            assert!(after[0].2.iter().all(|&p| p != CLEAR), "{what}: whole");
        });
    }
}

/// **With no room for the part inside, the image's own pixels there move
/// within it**: the arena holds the Sixel and not a second image half its
/// size, so the split is refused and counted, and the part over cells 13
/// and 14 moves up 20 pixel rows inside the image, the rows brought in
/// cleared; the part outside is untouched.
#[test]
fn with_no_room_to_split_the_pixels_move_within_the_image() {
    term_run(2000, MARGINS, |t| {
        support::feed(t, b"\x1b[3;14H");
        support::feed(t, RED);
        assert_eq!(placed(t).len(), 1, "the control, drawn");
        let refused = t.images().images_refused;
        support::feed(t, b"\x1b[S");
        assert_eq!(t.images().images_refused, refused + 1, "the part refused");
        let after = placed(t);
        assert_eq!(after.len(), 1, "one image");
        let px = &after[0].2;
        for y in 0..36 {
            for x in 0..40 {
                let want_clear = x < 20 && y >= 16;
                assert_eq!(px[y * 40 + x] == CLEAR, want_clear, "at ({x},{y})");
            }
        }
    });
}

/// The bits of a clear pixel are zero, which `CLEAR` stands for.
#[test]
fn clear_is_zero() {
    assert_eq!(Pixel::CLEAR.bits(), CLEAR);
}

/// **A Sixel across the left margin, and one across both, split the same
/// way**: drawn at cell 3 across the left margin at cell 4, the part over
/// cells 4 to 6 moves; drawn at cell 4 under margins of cells 5 and 6
/// only, the part over those two moves and the columns either side stay.
#[test]
fn a_sixel_across_the_left_margin_or_both_is_split() {
    let cases: [(&[u8], usize, usize, std::ops::Range<usize>); 2] = [
        (b"\x1b[?69h\x1b[5;15s\x1b[1;10r\x1b[3;4H", 4, 30, 10..40),
        (b"\x1b[?69h\x1b[6;7s\x1b[1;10r\x1b[3;5H", 5, 20, 10..30),
    ];
    for (setup, col, width, inside) in cases {
        term_run(8 << 20, setup, |t| {
            support::feed(t, RED);
            let line = placed(t)[0].0;
            let c0 = placed(t)[0].1;
            support::feed(t, b"\x1b[S");
            let after = placed(t);
            assert_eq!(after.len(), 2, "split in two at {col}");
            let (l1, c1, px1, w1) = &after[0];
            assert_eq!((*l1, *c1, *w1), (line - 1, col, width), "the part inside");
            assert!(px1.iter().all(|&p| p != CLEAR), "every pixel of it");
            let (l2, c2, px2, w2) = &after[1];
            assert_eq!((*l2, *c2, *w2), (line, c0, 40), "the rest");
            for y in 0..36 {
                for x in 0..40 {
                    assert_eq!(
                        px2[y * 40 + x] == CLEAR,
                        inside.contains(&x),
                        "at ({x},{y})"
                    );
                }
            }
        });
    }
}
