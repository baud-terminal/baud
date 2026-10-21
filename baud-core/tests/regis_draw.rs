//! **What ReGIS draws, pixel for pixel, against what `[[source:
//! dec-std-070]]` 070-8 and the VT340's manual say it draws.**
//!
//! The page is an image placed at the window's top line; its pixels are read
//! back from the store and compared to pixels worked out from the text --
//! never from another terminal's output. The VT340's default map gives the
//! colours (`[[source: vt3xx-screen-control]]` Table 2-3): the foreground
//! `W(I7)` is gray 50%, RGB 53% each, 135 of 255; the background `S(I0)`
//! black.

use baud_core::*;
use baud_domain::{ImageBudget, ImagePx, Pixel};

mod support;

const GRAY50: u32 = 0xff87_8787;
const BLACK: u32 = 0xff00_0000;

fn run(bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    support::feed(&mut t, bytes);
    f(&mut t);
}

/// The pages placed, in placement order: each image's pixels, width and
/// height, and the line it is anchored at.
fn pages(t: &Term<'_>) -> Vec<(Vec<u32>, usize, usize, u64)> {
    let imgs = t.images();
    imgs.placements()
        .filter_map(|(_, p)| {
            let v = imgs.image(p.image)?;
            Some((
                v.pixels.iter().map(|px| px.bits()).collect(),
                v.width.index(),
                v.height.index(),
                p.line.value(),
            ))
        })
        .collect()
}

fn page(t: &Term<'_>) -> (Vec<u32>, usize, usize) {
    let p = pages(t);
    assert_eq!(p.len(), 1, "one page placed");
    let (px, w, h, _) = p.into_iter().next().unwrap();
    (px, w, h)
}

/// The pixels that are not clear, as `(x, y, colour)`.
fn painted(px: &[u32], w: usize) -> Vec<(usize, usize, u32)> {
    px.iter()
        .enumerate()
        .filter(|(_, &c)| c != Pixel::CLEAR.bits())
        .map(|(i, &c)| (i % w, i / w, c))
        .collect()
}

#[test]
fn a_vector_draws_its_pixels_in_the_foreground_and_nothing_else() {
    run(b"\x1bPpP[100,100]V[110,100]\x1b\\", |t| {
        let (px, w, h) = page(t);
        assert_eq!((w, h), (800, 480), "no cell size: the VT340's 800 by 480");
        let want: Vec<_> = (100..=110).map(|x| (x, 100, GRAY50)).collect();
        assert_eq!(painted(&px, w), want);
    });
}

#[test]
fn a_diagonal_vector_steps_a_pixel_a_column() {
    run(b"\x1bPpP[0,0]V[4,2]\x1b\\", |t| {
        let (px, w, _) = page(t);
        let pts: Vec<_> = painted(&px, w).iter().map(|&(x, y, _)| (x, y)).collect();
        // Five pixels, one a column, `y` rounding half away from zero.
        assert_eq!(pts, vec![(0, 0), (1, 1), (2, 1), (3, 2), (4, 2)]);
    });
}

#[test]
fn replace_writes_the_pattern_s_zeros_as_background() {
    // P2 is 11110000, its elements two pixels long by default (`W(P(M2))`):
    // eight on, eight off.
    run(b"\x1bPpW(R,P2)P[0,10]V[31,10]\x1b\\", |t| {
        let (px, w, _) = page(t);
        let row: Vec<u32> = (0..32).map(|x| px[10 * w + x]).collect();
        let want: Vec<u32> = (0..32)
            .map(|x| if (x / 8) % 2 == 0 { GRAY50 } else { BLACK })
            .collect();
        assert_eq!(row, want);
    });
    // Overlay leaves the zeros alone.
    run(b"\x1bPpW(V,P2)P[0,10]V[31,10]\x1b\\", |t| {
        let (px, w, _) = page(t);
        let clear = (0..32).filter(|&x| px[10 * w + x] == 0).count();
        assert_eq!(clear, 16);
    });
}

#[test]
fn complement_twice_is_nothing_and_a_joint_is_complemented_once() {
    // A line and the same line complemented: index 7 becomes 8 (0111 ->
    // 1000), the VT340's gray 25%.
    run(b"\x1bPpP[0,0]V[9,0]W(C)P[0,0]V[9,0]\x1b\\", |t| {
        let (px, w, _) = page(t);
        assert!(painted(&px, w).iter().all(|&(_, _, c)| c == 0xff42_4242));
    });
    run(b"\x1bPpW(C)P[0,0]V[9,0]P[0,0]V[9,0]\x1b\\", |t| {
        let (px, w, _) = page(t);
        // Complement over clear (index 0) gives 15, then back to 0: written,
        // black.
        assert!(painted(&px, w).iter().all(|&(_, _, c)| c == BLACK));
    });
    // A polyline's joint, drawn once: complemented to 15, not back to 0.
    run(b"\x1bPpW(C)P[0,0]V[5,0][5,5]\x1b\\", |t| {
        let (px, _, _) = page(t);
        assert_eq!(px[5], 0xffcc_cccc, "gray 75%, index 15");
    });
}

#[test]
fn the_plane_mask_limits_a_write_to_its_planes() {
    // W(F1): only plane 0. Index 7 written over index 8 (1000) through
    // plane 0 gives 1001, 9: the less saturated blue.
    run(b"\x1bPpW(I8)P[0,0]V[3,0]W(F1,I7)P[0,0]V[3,0]\x1b\\", |t| {
        let (px, w, _) = page(t);
        assert!(painted(&px, w).iter().all(|&(_, _, c)| c == 0xff54_5499));
    });
}

#[test]
fn erase_makes_the_page_the_background() {
    run(b"\x1bPpS(I2)S(E)\x1b\\", |t| {
        let (px, _, _) = page(t);
        // The VT340's red: 80%, 13%, 13%.
        assert!(px.iter().all(|&c| c == 0xffcc_2121), "{:#x}", px[0]);
    });
}

#[test]
fn a_new_map_entry_recolours_what_is_drawn() {
    run(b"\x1bPpP[0,0]V[3,0]S(M7(R))\x1b\\", |t| {
        let (px, w, _) = page(t);
        assert!(painted(&px, w).iter().all(|&(_, _, c)| c == 0xffff_0000));
    });
}

#[test]
fn a_fill_covers_its_inside() {
    run(b"\x1bPpF(V[10,10][20,10][20,20][10,20])\x1b\\", |t| {
        let (px, w, _) = page(t);
        let p = painted(&px, w);
        // The square's pixels by centres, its left and top edges in and its
        // right and bottom out: 10 by 10.
        assert_eq!(p.len(), 100);
        assert!(
            p.iter()
                .all(|&(x, y, _)| (10..20).contains(&x) && (10..20).contains(&y))
        );
    });
    // A hole by the even-odd rule.
    run(
        b"\x1bPpF(V[0,0][40,0][40,40][0,40]F(V[10,10][30,10][30,30][10,30]))\x1b\\",
        |t| {
            let (px, w, _) = page(t);
            assert_eq!(painted(&px, w).len(), 40 * 40 - 20 * 20);
            assert_eq!(px[20 * w + 20], 0);
        },
    );
}

#[test]
fn a_circle_s_pixels_are_on_its_radius() {
    run(b"\x1bPpP[400,240]C[+100]\x1b\\", |t| {
        let (px, w, _) = page(t);
        let p = painted(&px, w);
        assert!(
            p.len() > 500,
            "a circle of radius 100 is some 600 pixels, {}",
            p.len()
        );
        for (x, y, _) in p {
            let d = (((x as f64) - 400.0).powi(2) + ((y as f64) - 240.0).powi(2)).sqrt();
            assert!((d - 100.0).abs() <= 1.0, "({x},{y}) at {d}");
        }
    });
}

#[test]
fn the_page_covers_the_text_area_at_its_pixels() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    // 200 by 50 cells of 10 by 20: a 2000 by 1000 page, the 800 by 480
    // device fitted into it, 999/479 a pixel.
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    support::feed(&mut t, b"\x1bPpP[0,0]V[799,0]\x1b\\");
    let (px, w, h) = page(&t);
    assert_eq!((w, h), (2000, 1000));
    let p = painted(&px, w);
    let last = p.iter().map(|&(x, _, _)| x).max().unwrap();
    // 799 * 999 / 479 = 1666.4.
    assert_eq!(last, 1666);
}

#[test]
fn a_scrolled_page_stays_and_the_next_drawing_begins_another() {
    run(b"\x1bPpP[0,0]V[9,0]\x1b\\", |t| {
        support::feed(t, &[b'\n'; 60]);
        support::feed(t, b"\x1bPpP[0,0]V[9,0]\x1b\\");
        let p = pages(t);
        assert_eq!(p.len(), 2);
        assert_ne!(p[0].3, p[1].3, "anchored at two lines");
    });
}

#[test]
fn without_image_memory_drawing_is_counted() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1bPpP[0,0]V[9,0]V[0,0]R(P)\x1b\\");
        assert_eq!(t.reply(), b"[0,0]\r");
        assert_eq!(t.regis_counts().unpaged, 2);
    });
}

#[test]
fn text_written_over_the_page_clears_its_cells_and_no_more() {
    // The VT330 and VT340 draw text and graphics into one bitmap, so a
    // character replaces what was drawn in its cell (Jev's lean,
    // 2026-10-20, CELLS).
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    // A page of 2000 by 1000; fill its upper-left 200 by 100 pixels.
    support::feed(&mut t, b"\x1bPpF(V[0,0][96,0][96,48][0,48])\x1b\\");
    let (px, w, _) = page(&t);
    let before = painted(&px, w).len();
    assert!(before > 0);
    // `ab` at row 0, columns 0 and 1: pixels 0..20 of rows 0..20 clear.
    support::feed(&mut t, b"\x1b[Hab");
    let (px, w, _) = page(&t);
    for y in 0..20 {
        for x in 0..20 {
            assert_eq!(px[y * w + x], 0, "({x},{y}) cleared");
        }
    }
    assert_ne!(px[20 * w], 0, "the row below kept");
    assert_ne!(px[20], 0, "the next column kept");
    assert_eq!(painted(&px, w).len(), before - 400);
    // The indices went too: a new map entry shows nothing there.
    support::feed(&mut t, b"\x1bPpS(M7(G))\x1b\\");
    let (px, w, _) = page(&t);
    assert_eq!(px[0], 0);
    assert_eq!(painted(&px, w).len(), before - 400);
}

#[test]
fn the_alternate_screen_draws_on_its_own_page() {
    run(
        b"\x1bPpP[0,0]V[9,0]\x1b\\\x1b[?1049h\x1bPpP[0,0]V[9,0]\x1b\\",
        |t| {
            let imgs = t.images();
            let screens: Vec<_> = imgs.placements().map(|(_, p)| p.screen).collect();
            assert_eq!(screens.len(), 2);
            assert_ne!(screens[0], screens[1]);
        },
    );
}

#[test]
fn ris_takes_the_page_away() {
    run(b"\x1bPpP[0,0]V[9,0]\x1b\\\x1bc", |t| {
        assert_eq!(pages(t).len(), 0);
        support::feed(t, b"\x1bPpP[0,0]V[9,0]\x1b\\");
        assert_eq!(pages(t).len(), 1);
    });
}

#[test]
fn a_loaded_character_draws_its_cell_at_the_unit_size() {
    // A box: the top and bottom rows full, the sides one pixel. Standard
    // size 1's unit cell is 8 by 20 for the VT300's 8 by 10 storage cell, so
    // each stored row is two pixel rows (`[[source: vt3xx-text]]` Table
    // 7-3).
    run(
        b"\x1bPpL(A1)'A'FF,81,81,81,81,81,81,81,81,FFP[100,100]T(A1)'A'\x1b\\",
        |t| {
            let (px, w, _) = page(t);
            for v in 0..20usize {
                for u in 0..9usize {
                    let row: u16 = if v / 2 == 0 || v / 2 == 9 { 0xff } else { 0x81 };
                    let on = u < 8 && row & (0x80 >> u) != 0;
                    let got = px[(100 + v) * w + 100 + u];
                    assert_eq!(got != 0, on, "({u},{v})");
                }
            }
        },
    );
}

#[test]
fn replace_writes_the_display_cell_s_background() {
    // In replace writing the cell's zeros and the display cell past the
    // unit cell are the background (§ 8.7.3.5).
    run(b"\x1bPpW(R)L(A1)'A'FFP[100,100]T(A1)'A'\x1b\\", |t| {
        let (px, w, _) = page(t);
        // The 9th column is past the 8-pixel unit cell: background.
        assert_eq!(px[100 * w + 108], BLACK);
        assert_eq!(px[100 * w + 100], GRAY50);
        assert_eq!(px[110 * w + 100], BLACK);
    });
}
