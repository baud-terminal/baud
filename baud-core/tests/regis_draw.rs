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

/// A stand-in for the embedder's font: every character a box round its
/// cell's edge, so where and how large it is drawn can be read back.
fn boxes(_: char, w: ImagePx, h: ImagePx, set: &mut dyn FnMut(ImagePx, ImagePx)) {
    let (w, h) = (w.raw(), h.raw());
    for y in 0..h {
        for x in 0..w {
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                set(ImagePx::of(x), ImagePx::of(y));
            }
        }
    }
}

fn run_font(bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_regis_font(boxes);
    support::feed(&mut t, bytes);
    f(&mut t);
}

#[test]
fn a_built_in_character_is_drawn_by_the_font_at_its_unit_cell() {
    // Standard size 1: an 8 by 20 unit cell (`[[source: vt3xx-text]]`
    // Table 7-3), its upper-left at the drawing position.
    run_font(b"\x1bPpP[100,100]T'A'\x1b\\", |t| {
        let (px, w, _) = page(t);
        let p = painted(&px, w);
        let xs: Vec<_> = p.iter().map(|&(x, _, _)| x).collect();
        let ys: Vec<_> = p.iter().map(|&(_, y, _)| y).collect();
        assert_eq!((xs.iter().min(), xs.iter().max()), (Some(&100), Some(&107)));
        assert_eq!((ys.iter().min(), ys.iter().max()), (Some(&100), Some(&119)));
        // The box's edge, 2 * 8 + 2 * 18 pixels.
        assert_eq!(p.len(), 2 * 8 + 2 * 18);
    });
}

#[test]
fn a_standard_size_draws_larger_and_a_turned_baseline_turns_it() {
    run_font(b"\x1bPpP[100,100]T(S2)'A'\x1b\\", |t| {
        let (px, w, _) = page(t);
        let p = painted(&px, w);
        let xmax = p.iter().map(|&(x, _, _)| x).max().unwrap();
        let ymax = p.iter().map(|&(_, y, _)| y).max().unwrap();
        // Size 2's unit cell is 16 by 30.
        assert_eq!((xmax, ymax), (115, 129));
    });
    // `T(D90)`: the character turned a quarter turn counterclockwise about
    // its upper-left, so it lies above the drawing position.
    run_font(b"\x1bPpP[100,100]T(D90)'A'\x1b\\", |t| {
        let (px, w, _) = page(t);
        let p = painted(&px, w);
        assert!(
            p.iter()
                .all(|&(x, y, _)| (100..=120).contains(&x) && (92..=100).contains(&y)),
            "{p:?}"
        );
    });
}

#[test]
fn without_a_font_a_built_in_character_is_counted() {
    run(b"\x1bPpP[100,100]T'AB'\x1b\\", |t| {
        assert_eq!(t.regis_counts().unfonted, 2);
    });
}

#[test]
fn command_display_shows_the_commands_on_the_bottom_line() {
    // `DCS 2p`: the characters received drawn on the bottom line, ten device
    // pixels a column, from row 460 (`[[source: vt3xx-regis-intro]]`).
    run_font(b"\x1bP2pP[1,1]\x1b\\", |t| {
        let (px, w, _) = page(t);
        let p = painted(&px, w);
        // Six characters, `P[1,1]`, each a box of 10 by 20.
        assert_eq!(p.len(), 6 * (2 * 10 + 2 * 18));
        assert!(p.iter().all(|&(x, y, _)| x < 60 && (460..480).contains(&y)));
    });
    // A line feed erases the line to the background, the drawing under it
    // with it.
    run_font(b"\x1bP2pP[0,470]V[799,470]\n\x1b\\", |t| {
        let (px, w, _) = page(t);
        assert!((460..480).all(|y| (0..800).all(|x| px[y * w + x] == BLACK)));
    });
}

// ---------------------------------------------------------------------------
// `S(T)`, 070-8 § 8.7.1.4.3: "the number of device dependent timer "ticks"
// for which ReGIS interpretation should be suspended".
// ---------------------------------------------------------------------------

/// One read under `RegisHold::On`: the ticks it waits and the bytes it held.
fn read_held(t: &mut Term<'_>, bytes: &[u8]) -> (Option<u32>, Option<Vec<u8>>) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let wait = t.take_regis_wait().map(RegisTicks::get);
    let rest = t
        .take_held()
        .map(|r| bytes[r.at.index()..r.at.index() + r.len.index()].to_vec());
    (wait, rest)
}

/// What the pages show, none placed or not.
fn drawn(t: &Term<'_>) -> Vec<(usize, usize, u32)> {
    pages(t)
        .iter()
        .flat_map(|(px, w, _, _)| painted(px, *w))
        .collect()
}

fn held_term(f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_regis_hold(RegisHold::On);
    f(&mut t);
}

/// **A delay holds the rest of the read, and nothing after it is drawn
/// until the rest is fed.** Fed, the vector is drawn as it is without the
/// hold.
#[test]
fn a_delay_holds_the_rest_of_the_read_until_it_is_fed() {
    held_term(|t| {
        let (wait, rest) = read_held(t, b"\x1bPpP[100,100]S(T60)V[110,100]\x1b\\");
        assert_eq!(wait, Some(60));
        assert_eq!(rest.as_deref(), Some(&b"V[110,100]\x1b\\"[..]));
        assert!(drawn(t).is_empty(), "nothing drawn before the delay ends");
        let (wait, rest) = read_held(t, &rest.unwrap());
        assert_eq!((wait, rest), (None, None));
        let (px, w, _) = page(t);
        let want: Vec<_> = (100..=110).map(|x| (x, 100, GRAY50)).collect();
        assert_eq!(painted(&px, w), want);
        assert_eq!(t.regis_counts().delays_unheld, 0);
    });
    // The control: without the hold the delay is passed over and counted.
    run(b"\x1bPpP[100,100]S(T60)V[110,100]\x1b\\", |t| {
        let (px, w, _) = page(t);
        assert_eq!(painted(&px, w).len(), 11);
        assert_eq!(t.regis_counts().delays_unheld, 1);
        assert_eq!(t.take_held(), None);
    });
}

/// **"S(T60,T60,T60)" stops at each**, the ticks waited one delay at a
/// time.
#[test]
fn a_cascade_of_delays_stops_at_each() {
    held_term(|t| {
        let mut bytes = b"\x1bPpS(T60,T30,T10)\x1b\\".to_vec();
        let mut waits = Vec::new();
        while let (Some(w), Some(rest)) = read_held(t, &bytes) {
            waits.push(w);
            bytes = rest;
        }
        assert_eq!(waits, vec![60, 30, 10]);
    });
}

/// **A delay inside a macrograph stops it where it is**, and the next byte
/// plays the rest of it before itself -- the envelope's terminator too,
/// held again at each delay the macrograph still has.
#[test]
fn a_delay_in_a_macrograph_stops_it_and_the_next_byte_plays_on() {
    held_term(|t| {
        let (wait, rest) = read_held(t, b"\x1bPp@:AS(T20)V[110,100]@;P[100,100]@AP[0,0]\x1b\\");
        assert_eq!(wait, Some(20));
        assert_eq!(rest.as_deref(), Some(&b"P[0,0]\x1b\\"[..]));
        assert!(drawn(t).is_empty());
        // The rest plays the macrograph's vector, then moves.
        let (wait, _) = read_held(t, &rest.unwrap());
        assert_eq!(wait, None);
        let (px, w, _) = page(t);
        let want: Vec<_> = (100..=110).map(|x| (x, 100, GRAY50)).collect();
        assert_eq!(
            painted(&px, w),
            want,
            "the vector from the macrograph's place"
        );
    });
    // Invoked last: the terminator plays it on, and is held at its second
    // delay, the parser back where the `\` is read again.
    held_term(|t| {
        let (wait, rest) = read_held(
            t,
            b"\x1bPp@:AS(T20)V[110,100]S(T10)V[110,110]@;P[100,100]@A\x1b\\x",
        );
        assert_eq!((wait, rest.as_deref()), (Some(20), Some(&b"\x1b\\x"[..])));
        let (wait, rest) = read_held(t, &rest.unwrap());
        assert_eq!((wait, rest.as_deref()), (Some(10), Some(&b"\\x"[..])));
        assert_eq!(drawn(t).len(), 11, "the first vector, not the second");
        let (wait, rest) = read_held(t, &rest.unwrap());
        assert_eq!((wait, rest), (None, None));
        assert_eq!(drawn(t).len(), 11 + 10, "the second vector");
        // The envelope ended, and the `x` after it is text.
        assert_eq!(t.cursor().1.index(), 1);
    });
}

/// **Held at every delay and resumed, a body draws what it draws whole**,
/// the page pixel for pixel.
#[test]
fn a_body_held_at_its_delays_draws_what_it_draws_whole() {
    let body: &[u8] =
        b"\x1bPpW(I2)P[10,10]S(T5)V[50,60]S(T1)C[+20]@:BS(T2)V[90,10]@;@BF(C[200,200][+30])\x1b\\";
    let mut whole = Vec::new();
    run(body, |t| whole = page(t).0);
    held_term(|t| {
        let (mut bytes, mut held) = (body.to_vec(), 0);
        while let (Some(_), Some(rest)) = read_held(t, &bytes) {
            held += 1;
            bytes = rest;
        }
        assert_eq!(held, 3);
        assert!(
            page(t).0 == whole,
            "the held body's page is the whole one's"
        );
    });
}

/// **The hold and the font are the embedder's, and `RIS` keeps them.**
#[test]
fn ris_keeps_the_hold_and_the_font() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_regis_hold(RegisHold::On);
    t.set_regis_font(boxes);
    support::feed(&mut t, b"\x1bc");
    let (wait, _) = read_held(&mut t, b"\x1bPpP[100,100]T'A'S(T6)\x1b\\");
    assert_eq!(wait, Some(6), "the hold survives RIS");
    assert_eq!(t.regis_counts().unfonted, 0, "and so does the font");
}

/// A terminal of 2000 by 1000 pixels of text area, cells 10 by 20, with
/// room for two pages and a plane.
fn run_sized(bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    support::feed(&mut t, bytes);
    f(&mut t);
}

/// **Complement writing reads what is shown where it draws, after a
/// scroll**: "complement the contents of the pixels present in the image"
/// (070-8 § 8.7.2.4.1.1), and under the scrolling model the screen's
/// contents move away beneath its coordinates (§ 8.8.1). A fill over cells
/// 0 to 19 of lines 1 to 5, the screen scrolled two lines, then a
/// complement fill over cells 0 to 9 of lines 1 to 10: where the first fill now shows, on
/// lines 1 to 3, the complement differs from where nothing was drawn. The
/// page that scrolled keeps only its two lines in the history; the rest
/// went to the page at the window's top.
#[test]
fn complement_reads_the_page_that_scrolled_under_it() {
    const FILL: &[u8] = b"\x1bPpF(V[0,0][96,0][96,48][0,48])\x1b\\";
    run_sized(FILL, |t| {
        support::feed(t, b"\x1b[50;1H\n\n");
        support::feed(t, b"\x1bPpW(C)F(V[0,0][48,0][48,96][0,96])\x1b\\");
        let mut p = pages(t);
        p.sort_by_key(|q| q.3);
        assert_eq!(p.len(), 2, "the old page and the new");
        let (old, w, h, old_line) = &p[0];
        let (new, _, _, new_line) = &p[1];
        assert_eq!(new_line - old_line, 2, "two lines apart");
        let at = |px: &[u32], col: usize, row: usize| px[(row * 20 + 10) * w + col * 10 + 5];
        assert_ne!(
            at(new, 5, 1),
            at(new, 5, 7),
            "over the first fill, the complement of what it drew"
        );
        assert_ne!(at(new, 5, 1), 0, "drawn");
        assert_eq!(at(new, 5, 7), at(new, 5, 8), "where nothing was, alike");
        assert_ne!(
            at(new, 15, 1),
            0,
            "the first fill carried, past the complement"
        );
        assert_eq!(
            at(new, 5, 4),
            at(new, 5, 7),
            "line 5, which the first fill left as it scrolled up, alike"
        );
        assert!(
            (0..40).any(|y| (0..*w).any(|x| old[y * w + x] != 0)),
            "the old page keeps its lines in the history"
        );
        assert!(
            (40..*h).all(|y| (0..*w).all(|x| old[y * w + x] == 0)),
            "and nothing on the screen"
        );
        let imgs = t.images();
        for (_, q) in imgs.placements() {
            let v = imgs.image(q.image).expect("live");
            let n = v.pixels.iter().filter(|px| px.bits() != 0).count();
            assert_eq!(
                imgs.lit(q.image),
                Some(u32::try_from(n).unwrap()),
                "each page's count"
            );
        }
    });
}

/// **A page scrolled wholly into the history carries nothing**: the next
/// drawing begins a page of its own, and the complement there reads only
/// the background; the old page keeps all it drew.
#[test]
fn a_page_scrolled_wholly_away_carries_nothing() {
    const FILL: &[u8] = b"\x1bPpF(V[0,0][96,0][96,48][0,48])\x1b\\";
    run_sized(FILL, |t| {
        let drawn = painted(&page(t).0, 2000).len();
        support::feed(t, b"\x1b[50;1H");
        support::feed(t, &[b'\n'; 60]);
        support::feed(t, b"\x1bPpW(C)F(V[0,0][96,0][96,96][0,96])\x1b\\");
        let mut p = pages(t);
        p.sort_by_key(|q| q.3);
        assert_eq!(p.len(), 2);
        assert_eq!(painted(&p[0].0, 2000).len(), drawn, "the old page whole");
        let new = &p[1].0;
        let at = |col: usize, row: usize| new[(row * 20 + 10) * 2000 + col * 10 + 5];
        assert_eq!(at(5, 1), at(5, 7), "the complement of the background alone");
    });
}

/// **A carry with no room for the new page is refused once**: the arena
/// holds one page and its plane and not a second page, so the drawing
/// after the scroll has no page, one refusal and one drawing counted.
#[test]
fn a_carry_with_no_room_is_refused_once() {
    const FILL: &[u8] = b"\x1bPpF(V[0,0][96,0][96,48][0,48])\x1b\\";
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(3 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    support::feed(&mut t, FILL);
    assert_eq!(pages(&t).len(), 1, "the control, one page");
    let (refused, unpaged) = (t.images().images_refused, t.regis_counts().unpaged);
    support::feed(&mut t, b"\x1b[50;1H\n\n");
    support::feed(&mut t, b"\x1bPpP[0,0]V[9,0]\x1b\\");
    assert_eq!(t.images().images_refused, refused + 1, "one refusal");
    assert_eq!(t.regis_counts().unpaged, unpaged + 1, "one drawing");
    assert_eq!(pages(&t).len(), 1, "the old page stays");
}
