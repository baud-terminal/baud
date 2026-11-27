//! **What erasing does to the pixels a Sixel or ReGIS drew**, read back
//! from the image store. DEC STD 070's B.8.11 leaves the interaction of
//! text erasing and graphics to the device; the VT330 and VT340 keep both
//! in one bitmap, so a character position the terminal erases, or redraws
//! with a shifted character, shows no graphic pixel afterwards, and lines
//! a band moves take their pixels with them (vt340test,
//! `[[source: vt340test-faketextcolor]]`).
//!
//! Cells are 10 by 20 pixels throughout.

use baud_core::*;
use baud_domain::{ImageBudget, ImagePx, Pixel};

mod support;

/// A 40 by 36 Sixel at 1:1 (`P1` 9), every pixel register 1's red: four
/// columns and two rows of cells from where it is drawn. `P2` 1, so no
/// background fill reaches past its data.
const RED: &[u8] = b"\x1bP9;1q#1;2;100;0;0#1!40~-!40~-!40~-!40~-!40~-!40~\x1b\\";

fn term_run(bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    support::feed(&mut t, bytes);
    f(&mut t);
}

/// The one image placed: its pixels as bits, and its width.
fn image(t: &Term<'_>) -> Option<(Vec<u32>, usize)> {
    let imgs = t.images();
    let mut placed = imgs.placements();
    let (_, p) = placed.next()?;
    assert!(placed.next().is_none(), "one image placed");
    let v = imgs.image(p.image)?;
    Some((
        v.pixels.iter().map(|px| px.bits()).collect(),
        v.width.index(),
    ))
}

/// Which pixels of a `w`-wide image are drawn, `#`, or clear, `.`, a
/// string a row, so a failure shows the shape.
fn shape(px: &[u32], w: usize, step: usize) -> Vec<String> {
    px.chunks(w)
        .step_by(step)
        .map(|row| {
            row.iter()
                .step_by(step)
                .map(|&c| match c == Pixel::CLEAR.bits() {
                    true => '.',
                    false => '#',
                })
                .collect()
        })
        .collect()
}

/// The 40 by 36 Sixel after `then`, drawn at the home cell, as one
/// character a cell: rows 0..20 and 20..36, columns of 10.
fn sixel_after(then: &[u8]) -> Option<Vec<String>> {
    let mut out = None;
    term_run(b"\x1b[H", |t| {
        support::feed(t, RED);
        support::feed(t, then);
        out = image(t).map(|(px, w)| shape(&px, w, 10).into_iter().step_by(2).collect());
    });
    out
}

/// **Each erase clears the Sixel's pixels in exactly the positions it
/// erases**, and an image left with no pixel is freed.
#[test]
fn each_erase_clears_the_sixel_in_its_positions() {
    let drawn = Some(vec!["####".to_string(), "####".to_string()]);
    assert_eq!(sixel_after(b""), drawn, "the control");
    let cases: &[(&[u8], [&str; 2], &str)] = &[
        (b"\x1b[2;3H\x1b[K", ["####", "##.."], "EL 0"),
        (b"\x1b[2;3H\x1b[1K", ["####", "...#"], "EL 1"),
        (b"\x1b[1;3H\x1b[2K", ["....", "####"], "EL 2"),
        (b"\x1b[1;2H\x1b[J", ["#...", "...."], "ED 0"),
        (b"\x1b[2;2H\x1b[1J", ["....", "..##"], "ED 1"),
        (b"\x1b[1;2H\x1b[2X", ["#..#", "####"], "ECH"),
        (b"\x1b[1;2;2;3$z", ["#..#", "#..#"], "DECERA"),
    ];
    for (bytes, want, what) in cases {
        let want = want.iter().map(|s| s.to_string()).collect();
        assert_eq!(sixel_after(bytes), Some(want), "{what}");
    }
    assert_eq!(sixel_after(b"\x1b[2J"), None, "ED 2 freed it");
    assert_eq!(
        sixel_after(b"\x1b[1;1H\x1b[K\x1b[2;1H\x1b[K"),
        None,
        "two ELs that clear every pixel free it"
    );
}

/// **A character shifted into a position clears it**: ICH, DCH and IRM's
/// gap clear the line from the cursor to the right margin, as the VT340
/// was seen to ("Colors are removed from the line when text is shifted
/// left by Esc [ P"). DECIC and DECDC leave the image where it is
/// (`decic_and_decdc_leave_an_image_on_a_sized_line_where_it_is`).
#[test]
fn a_shift_of_characters_clears_the_sixel_from_the_cursor() {
    let cases: &[(&[u8], [&str; 2], &str)] = &[
        (b"\x1b[1;2H\x1b[P", ["#...", "####"], "DCH"),
        (b"\x1b[2;3H\x1b[@", ["####", "##.."], "ICH"),
        (b"\x1b[1;2H\x1b['~", ["####", "####"], "DECDC leaves it"),
    ];
    for (bytes, want, what) in cases {
        let want = want.iter().map(|s| s.to_string()).collect();
        assert_eq!(sixel_after(bytes), Some(want), "{what}");
    }
}

/// **A selective erase clears the positions it erases and no others**: an
/// occupied unprotected position, which becomes a space. DECSCA protects a
/// character, and the position it keeps keeps what is drawn over it; "Unoccupied
/// character positions are not affected" (DECSERA, `[[source: dec-std-070]]` p.
/// 5-172), and keep it too. The text is written before the Sixel, since text
/// written over a Sixel frees it: a protected `A`, then `BC`, on the first row,
/// `xy` on the second.
#[test]
fn a_selective_erase_clears_only_the_positions_it_erases() {
    let text: &[u8] = b"\x1b[1\"qA\x1b[0\"qBC\x1b[2;1Hxy\x1b[H";
    let after = |then: &[u8]| {
        let mut out = None;
        term_run(text, |t| {
            support::feed(t, RED);
            support::feed(t, then);
            out = image(t)
                .map(|(px, w)| shape(&px, w, 10).into_iter().step_by(2).collect::<Vec<_>>());
        });
        out
    };
    let rows = |a: &str, b: &str| Some(vec![a.to_string(), b.to_string()]);
    assert_eq!(after(b""), rows("####", "####"), "the control");
    assert_eq!(after(b"\x1b[?2J"), rows("#..#", "..##"), "DECSED");
    assert_eq!(after(b"\x1b[1;1H\x1b[?2K"), rows("#..#", "####"), "DECSEL");
    assert_eq!(after(b"\x1b[2;1;2;4${"), rows("####", "..##"), "DECSERA");
}

// ---- the ReGIS page ---------------------------------------------------------

/// The page's pixels, its width and height.
fn page(t: &Term<'_>) -> (Vec<u32>, usize, usize) {
    let imgs = t.images();
    let mut placed = imgs.placements();
    let (_, p) = placed.next().expect("a page");
    assert!(placed.next().is_none(), "one page placed");
    let v = imgs.image(p.image).expect("its image");
    (
        v.pixels.iter().map(|px| px.bits()).collect(),
        v.width.index(),
        v.height.index(),
    )
}

/// A ReGIS fill over the page's upper-left corner, about 200 by 100 of its
/// 2000 by 1000 pixels: cells 0 to 19 of rows 0 to 4.
const FILL: &[u8] = b"\x1bPpF(V[0,0][96,0][96,48][0,48])\x1b\\";

/// Whether the page's cell `(col, row)` holds a drawn pixel at its centre.
fn drawn_at(px: &[u32], w: usize, col: usize, row: usize) -> bool {
    px[(row * 20 + 10) * w + col * 10 + 5] != Pixel::CLEAR.bits()
}

/// A cell's column and row, and whether it keeps its pixels.
type Cell = (usize, usize, bool);

/// **The erases clear the ReGIS page in the positions they erase**, and
/// leave the rest of it.
#[test]
fn each_erase_clears_the_page_in_its_positions() {
    let cases: &[(&[u8], &[Cell], &str)] = &[
        (
            b"\x1b[2;6H\x1b[K",
            &[
                (4, 1, true),
                (5, 1, false),
                (19, 1, false),
                (5, 0, true),
                (5, 2, true),
            ],
            "EL 0",
        ),
        (
            b"\x1b[2;6H\x1b[J",
            &[(4, 1, true), (5, 1, false), (0, 2, false), (5, 0, true)],
            "ED 0",
        ),
        (
            b"\x1b[3;3H\x1b[4X",
            &[(1, 2, true), (2, 2, false), (5, 2, false), (6, 2, true)],
            "ECH",
        ),
        (
            b"\x1b[2;2;3;4$z",
            &[(0, 1, true), (1, 1, false), (3, 2, false), (4, 2, true)],
            "DECERA",
        ),
        (
            b"\x1b[4;5H\x1b[3P",
            &[(3, 3, true), (4, 3, false), (19, 3, false), (4, 2, true)],
            "DCH",
        ),
    ];
    for (bytes, cells, what) in cases {
        term_run(FILL, |t| {
            let (px, w, _) = page(t);
            for &(c, r, _) in cells.iter() {
                assert!(drawn_at(&px, w, c, r), "{what}: the control at ({c},{r})");
            }
            support::feed(t, bytes);
            let (px, w, _) = page(t);
            for &(c, r, kept) in cells.iter() {
                assert_eq!(drawn_at(&px, w, c, r), kept, "{what} at ({c},{r})");
            }
        });
    }
}

/// **A selective erase spares the page over a protected position and an
/// unoccupied one**, the text written before the drawing, since text
/// written over the page clears its cells.
#[test]
fn a_selective_erase_spares_the_page_over_a_protected_position() {
    term_run(b"\x1b[1\"qA\x1b[0\"qBC", |t| {
        support::feed(t, FILL);
        support::feed(t, b"\x1b[?2J");
        let (px, w, _) = page(t);
        assert!(drawn_at(&px, w, 0, 0), "protected, kept");
        assert!(!drawn_at(&px, w, 1, 0), "beside it, erased");
        assert!(!drawn_at(&px, w, 2, 0), "and the next");
        assert!(drawn_at(&px, w, 3, 0), "unoccupied, kept");
        assert!(drawn_at(&px, w, 0, 1), "below it, unoccupied, kept");
    });
}

/// **Lines a band moves take the page's pixels with them**: DL and IL
/// shift the rows from the cursor's line to the region's bottom, and a
/// scroll inside a region its rows, the rows brought in blank cleared and
/// the rest of the page where it was.
#[test]
fn the_lines_a_band_moves_take_the_page_with_them() {
    let row = |px: &[u32], w: usize, y: usize| px[y * w..(y + 1) * w].to_vec();
    let clear =
        |px: &[u32], w: usize, y: usize| row(px, w, y).iter().all(|&c| c == Pixel::CLEAR.bits());
    // DL at line 2: rows 40.. come up to 20.., the last line blank.
    term_run(FILL, |t| {
        let (old, w, h) = page(t);
        support::feed(t, b"\x1b[2;1H\x1b[M");
        let (new, _, _) = page(t);
        for y in 0..20 {
            assert_eq!(row(&new, w, y), row(&old, w, y), "DL: line 1 stays, y {y}");
        }
        for y in 20..h - 20 {
            assert_eq!(
                row(&new, w, y),
                row(&old, w, y + 20),
                "DL: up a line, y {y}"
            );
        }
        assert!(
            (h - 20..h).all(|y| clear(&new, w, y)),
            "DL: the line brought in"
        );
    });
    // IL at line 2: rows 20.. go down to 40.., line 2 blank.
    term_run(FILL, |t| {
        let (old, w, h) = page(t);
        support::feed(t, b"\x1b[2;1H\x1b[L");
        let (new, _, _) = page(t);
        for y in 0..20 {
            assert_eq!(row(&new, w, y), row(&old, w, y), "IL: line 1 stays, y {y}");
        }
        assert!((20..40).all(|y| clear(&new, w, y)), "IL: the line inserted");
        for y in 40..h {
            assert_eq!(
                row(&new, w, y),
                row(&old, w, y - 20),
                "IL: down a line, y {y}"
            );
        }
    });
    // A scroll inside lines 2 to 4: rows 40..80 come up to 20..60, 60..80
    // blank, and line 5 on, below the region, where it was.
    term_run(FILL, |t| {
        let (old, w, h) = page(t);
        support::feed(t, b"\x1b[2;4r\x1b[4;1H\n");
        let (new, _, _) = page(t);
        for y in 0..20 {
            assert_eq!(row(&new, w, y), row(&old, w, y), "region: above, y {y}");
        }
        for y in 20..60 {
            assert_eq!(row(&new, w, y), row(&old, w, y + 20), "region: up, y {y}");
        }
        assert!(
            (60..80).all(|y| clear(&new, w, y)),
            "region: the line brought in"
        );
        for y in 80..h {
            assert_eq!(row(&new, w, y), row(&old, w, y), "region: below, y {y}");
        }
    });
    // A region from the top, lines 1 to 4: the window moves and takes the
    // page with it, so the lines under the region are put back -- the page's
    // rows with them. Line 1 scrolled off the top; seen from the window,
    // lines 1 to 3 hold the old 2 to 4 and line 4 is blank; in the page,
    // anchored a line above the window now, rows 20..80 stay, 80..100 are
    // the line brought in, and rows 100.. are the old 80.., down a line.
    term_run(FILL, |t| {
        let (old, w, h) = page(t);
        support::feed(t, b"\x1b[1;4r\x1b[4;1H\n");
        let (new, _, _) = page(t);
        for y in 20..80 {
            assert_eq!(row(&new, w, y), row(&old, w, y), "top region: kept, y {y}");
        }
        assert!(
            (80..100).all(|y| clear(&new, w, y)),
            "top region: the line brought in"
        );
        for y in 100..h {
            assert_eq!(
                row(&new, w, y),
                row(&old, w, y - 20),
                "top region: put back, y {y}"
            );
        }
    });
}

/// **IRM's gap clears the page from the cursor to the margin**: a
/// character written under insert mode shifts the line's tail right, so
/// every position from the cursor on is a shifted character's or blank,
/// and loses what was drawn over it. Under replace mode, the control,
/// only the position written loses it.
#[test]
fn irm_s_gap_clears_the_page_from_the_cursor() {
    term_run(FILL, |t| {
        support::feed(t, b"\x1b[2;6Hx");
        let (px, w, _) = page(t);
        assert!(!drawn_at(&px, w, 5, 1), "replace: the position written");
        assert!(drawn_at(&px, w, 6, 1), "replace: the next, the control");
    });
    term_run(FILL, |t| {
        support::feed(t, b"\x1b[4h\x1b[2;6Hx\x1b[4l");
        let (px, w, _) = page(t);
        assert!(drawn_at(&px, w, 4, 1), "insert: before the cursor, kept");
        assert!(!drawn_at(&px, w, 5, 1), "insert: the position written");
        assert!(!drawn_at(&px, w, 6, 1), "insert: the next, shifted");
        assert!(!drawn_at(&px, w, 19, 1), "insert: and on to the margin");
        assert!(drawn_at(&px, w, 6, 0), "insert: the line above, kept");
        assert!(drawn_at(&px, w, 6, 2), "insert: the line below, kept");
    });
}

/// **Under ERM reset an erase spares a guarded position and what is drawn
/// over it**: SPA and EPA guard columns 3 to 5 of the first line, and EL
/// clears the page in the rest of it (`[[source: ecma48]]` 7.2.4,
/// PROTECT, "Only the contents of unprotected areas are affected by an
/// erasure control function"). Under ERM set, ALL, it clears them too.
#[test]
fn erm_spares_the_page_over_a_guarded_position() {
    let guard: &[u8] = b"\x1b[1;3H\x1bV\x1b[1;5H\x1bW";
    term_run(guard, |t| {
        support::feed(t, FILL);
        support::feed(t, b"\x1b[2K");
        let (px, w, _) = page(t);
        assert!(!drawn_at(&px, w, 1, 0), "unguarded, erased");
        for c in 2..5 {
            assert!(drawn_at(&px, w, c, 0), "guarded, kept at {c}");
        }
        assert!(!drawn_at(&px, w, 5, 0), "past the area, erased");
        assert!(drawn_at(&px, w, 2, 1), "the next line, kept");
    });
    term_run(guard, |t| {
        support::feed(t, FILL);
        support::feed(t, b"\x1b[6h\x1b[2K");
        let (px, w, _) = page(t);
        for c in 0..6 {
            assert!(!drawn_at(&px, w, c, 0), "ALL: erased at {c}");
        }
    });
}

/// The page placed, if one is: its image's pixels lit by a recount, and
/// the count the store keeps.
fn page_lit(t: &Term<'_>) -> Option<(usize, Option<u32>)> {
    let imgs = t.images();
    let (_, p) = imgs.placements().next()?;
    let v = imgs.image(p.image)?;
    let n = v
        .pixels
        .iter()
        .filter(|px| px.bits() != Pixel::CLEAR.bits())
        .count();
    Some((n, imgs.lit(p.image)))
}

/// **A page with nothing left lit is freed**, with the plane it is drawn
/// from, as a Sixel with no pixel left is: ED 2, enough ELs to clear every
/// drawn row, and text written over every drawn cell each free it, and
/// one row short of that keeps it. A drawing after makes a new page.
#[test]
fn a_page_with_nothing_lit_is_freed() {
    let els = |rows: usize| -> Vec<u8> {
        (1..=rows)
            .flat_map(|r| format!("\x1b[{r};1H\x1b[2K").into_bytes())
            .collect()
    };
    let text = |rows: usize| -> Vec<u8> {
        (1..=rows)
            .flat_map(|r| format!("\x1b[{r};1H{}", "x".repeat(21)).into_bytes())
            .collect()
    };
    let cases: [(Vec<u8>, bool, &str); 5] = [
        (b"\x1b[2J".to_vec(), false, "ED 2"),
        (els(4), true, "EL over rows 1 to 4, row 5 still lit"),
        (els(6), false, "EL over every drawn row"),
        (text(4), true, "text over rows 1 to 4"),
        (text(6), false, "text over every drawn cell"),
    ];
    for (bytes, kept, what) in cases {
        term_run(FILL, |t| {
            let dropped = t.images().images_dropped;
            support::feed(t, &bytes);
            assert_eq!(page_lit(t).is_some(), kept, "{what}");
            if !kept {
                assert_eq!(
                    t.images().images_dropped,
                    dropped + 2,
                    "{what}: the page and its plane freed"
                );
                support::feed(t, FILL);
                let (n, kept) = page_lit(t).expect("a new page");
                assert!(n > 0, "{what}: the next drawing shows");
                assert_eq!(kept, Some(u32::try_from(n).unwrap()), "{what}: its count");
            }
        });
    }
}

/// **The store's count of a page's lit pixels is a recount's**, after
/// every kind of write to the page: drawing, complement drawing over what
/// is there, text over it, the erases, lines moving in a band, and a
/// rectangle moving under side margins.
#[test]
fn the_page_s_lit_count_is_kept_exactly() {
    let steps: &[&[u8]] = &[
        FILL,
        b"\x1bPpW(C)F(V[40,20][140,20][140,90][40,90])\x1b\\",
        b"\x1b[2;3HHello",
        b"\x1b[3;1H\x1b[K",
        b"\x1b[1;5H\x1b[3X",
        b"\x1b[2;1H\x1b[M",
        b"\x1b[1;1H\x1b[2L",
        b"\x1b[2;6r\x1b[6;1H\n\n\x1b[r",
        b"\x1b[4;4H\x1b[2P",
        b"\x1b[?69h\x1b[3;12s\x1b[2S\x1b[?69l",
        b"\x1bPpS(E)\x1b\\",
        b"\x1b[10;10H\x1b[1J",
    ];
    term_run(b"", |t| {
        for (i, step) in steps.iter().enumerate() {
            support::feed(t, step);
            if let Some((n, kept)) = page_lit(t) {
                assert_eq!(kept, Some(u32::try_from(n).unwrap()), "after step {i}");
            }
        }
        assert!(page_lit(t).is_some(), "S(E) lit every pixel, still placed");
    });
}
