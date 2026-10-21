//! **ReGIS's Open Extensions, pixel for pixel**, against `[[source:
//! dec-std-070]]` 070-8 § 8.8 and the VT330/VT340 manual where it has them.
//!
//! The VT340's default map gives the colours (`[[source:
//! vt3xx-screen-control]]` Table 2-3), a percentage `p` being `p * 255 / 100`
//! rounded: index 0 black, 1 blue (20, 20, 80), 2 red (80, 13, 13), 3 green
//! (20, 80, 20), 4 magenta (80, 20, 80), 5 cyan (20, 80, 80), 6 yellow
//! (80, 80, 20), 7 gray 53%.

use baud_core::*;
use baud_domain::{ImageBudget, Pixel, RegisBlink, RegisPhase, RegisRepainted};

mod support;

const BLACK: u32 = 0xff00_0000;
const BLUE: u32 = 0xff33_33cc;
const GREEN: u32 = 0xff33_cc33;
const MAGENTA: u32 = 0xffcc_33cc;
const CYAN: u32 = 0xff33_cccc;
const GRAY50: u32 = 0xff87_8787;
const CLEAR: u32 = 0;

fn run(bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    support::feed(&mut t, bytes);
    f(&mut t);
}

/// The one page placed: its pixels and width.
fn page(t: &Term<'_>) -> (Vec<u32>, usize) {
    let imgs = t.images();
    let pages: Vec<_> = imgs
        .placements()
        .filter_map(|(_, p)| imgs.image(p.image))
        .map(|v| {
            let px: Vec<u32> = v.pixels.iter().map(|px| px.bits()).collect();
            (px, v.width.index())
        })
        .collect();
    assert_eq!(pages.len(), 1, "one page placed");
    pages.into_iter().next().unwrap()
}

/// Row `y`'s pixels from `x0` up to `x1`.
fn row(t: &Term<'_>, y: usize, x0: usize, x1: usize) -> Vec<u32> {
    let (px, w) = page(t);
    px[y * w + x0..y * w + x1].to_vec()
}

/// `n` copies of `c`.
fn n_of(n: usize, c: u32) -> Vec<u32> {
    vec![c; n]
}

/// A line of index 1 along row 0 from column 0 to 9, the foreground back
/// to 7 after it.
const BLUE_LINE: &[u8] = b"W(I1)P[0,0]V[9,0]W(I7)";

fn regis(body: &[&[u8]]) -> Vec<u8> {
    let mut v = b"\x1bPp".to_vec();
    for b in body {
        v.extend_from_slice(b);
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

#[test]
fn w_w_writes_its_loaded_entries_and_the_mode_the_rest() {
    // P2 is 11110000 at the default multiplier 2: eight on, eight off.
    // Over index 1 a one writes 4 and a zero 5; over a clear pixel no
    // entry is loaded and overlay writes ones in the foreground, 7, and
    // leaves zeros alone.
    let bytes = regis(&[BLUE_LINE, b"W(P2,W1(P4N5))P[0,0]V[19,0]"]);
    run(&bytes, |t| {
        let mut want = n_of(8, MAGENTA);
        want.extend(n_of(2, CYAN));
        want.extend(n_of(6, CLEAR));
        want.extend(n_of(4, GRAY50));
        assert_eq!(row(t, 0, 0, 20), want);
        assert_eq!(t.regis_counts().unbuilt, 0, "W(W) is built");
    });
}

#[test]
fn a_writing_mode_command_clears_the_entries() {
    for mode in [&b"W(V)"[..], b"W(R)", b"W(N0)", b"W(C)W(V)", b"W(E)W(V)"] {
        let bytes = regis(&[BLUE_LINE, b"W(W1(P4))", mode, b"P[0,0]V[9,0]"]);
        run(&bytes, |t| {
            assert_eq!(row(t, 0, 0, 10), n_of(10, GRAY50), "{mode:?}");
        });
    }
    // An option that sets no writing mode keeps them.
    let bytes = regis(&[BLUE_LINE, b"W(W1(P4))W(I7,F15,P1)P[0,0]V[9,0]"]);
    run(&bytes, |t| assert_eq!(row(t, 0, 0, 10), n_of(10, MAGENTA)));
}

#[test]
fn the_bit_is_the_pattern_s_own_before_negate() {
    // A solid pattern under W(N1): every bit a one, inverted to zero for
    // the writing modes. The loaded entries read the one.
    let bytes = regis(&[BLUE_LINE, b"W(N1)W(W1(P4N5))P[0,0]V[9,0]"]);
    run(&bytes, |t| assert_eq!(row(t, 0, 0, 10), n_of(10, MAGENTA)));
}

#[test]
fn the_plane_mask_limits_a_microcoded_write() {
    // Index 1 (0001) written to 4 (0100) through planes 1 and 2 (W(F6)):
    // 0001 keeps its plane 0 and takes planes 1 and 2 from 0100, 0101, 5.
    // Written whole it would be 4; the foreground 7 through the mask, 7.
    let bytes = regis(&[BLUE_LINE, b"W(F6,W1(P4))P[0,0]V[9,0]"]);
    run(&bytes, |t| assert_eq!(row(t, 0, 0, 10), n_of(10, CYAN)));
}

#[test]
fn a_valid_s_a_resets_the_entries_and_a_temporary_one_ends_with_its_command() {
    let bytes = regis(&[BLUE_LINE, b"W(W1(P4))S(A[0,0][799,479])P[0,0]V[9,0]"]);
    run(&bytes, |t| assert_eq!(row(t, 0, 0, 10), n_of(10, GRAY50)));
    // A temporary entry draws its own command, and the next draws as the
    // persistent controls say.
    let bytes = regis(&[BLUE_LINE, b"P[0,0]V(W(W1(P4)))[4,0]V[9,0]"]);
    run(&bytes, |t| {
        let mut want = n_of(5, MAGENTA);
        want.extend(n_of(5, GRAY50));
        assert_eq!(row(t, 0, 0, 10), want);
    });
}

#[test]
fn an_entry_over_index_zero_writes_a_clear_pixel_too() {
    // A pixel never written reads index 0, as complement reads it: an
    // entry for 0 writes it, a zero bit included.
    let bytes = regis(&[b"W(P2,W0(P1N3))P[0,0]V[15,0]"]);
    run(&bytes, |t| {
        let mut want = n_of(8, BLUE);
        want.extend(n_of(8, GREEN));
        assert_eq!(row(t, 0, 0, 16), want);
        assert_ne!(Pixel::CLEAR.bits(), BLACK);
    });
}

const RED: u32 = 0xffcc_2121;
const GRAY80: u32 = 0xffcc_cccc;

/// The pixel at `(x, y)`.
fn at(t: &Term<'_>, x: usize, y: usize) -> u32 {
    let (px, w) = page(t);
    px[y * w + x]
}

/// A square's outline in the foreground, `[10,10]` to `[30,30]`.
const BOX: &[u8] = b"P[10,10]V[30,10][30,30][10,30][10,10]";

#[test]
fn a_flood_fills_the_closed_area_and_not_its_boundary() {
    let bytes = regis(&[BOX, b"W(I2)F[20,20]"]);
    run(&bytes, |t| {
        for y in 9..32 {
            for x in 9..32 {
                let edge = (x == 10 || x == 30) && (10..=30).contains(&y)
                    || (y == 10 || y == 30) && (10..=30).contains(&x);
                let inside = (11..30).contains(&x) && (11..30).contains(&y);
                let want = match (edge, inside) {
                    (true, _) => GRAY50,
                    (_, true) => RED,
                    _ => CLEAR,
                };
                assert_eq!(at(t, x, y), want, "({x},{y})");
            }
        }
        assert_eq!(t.regis_counts().unbuilt, 0, "the flood is built");
    });
}

#[test]
fn a_diagonal_boundary_closes_the_flood() {
    // A diamond of diagonal lines, its pixels joined only corner to
    // corner: "A boundary need only be connected by diagonally adjacent
    // pixels to define closure".
    let bytes = regis(&[b"P[20,0]V[40,20][20,40][0,20][20,0]W(I2)F[20,20]"]);
    run(&bytes, |t| {
        assert_eq!(at(t, 20, 20), RED);
        assert_eq!(at(t, 20, 2), RED, "the top's inside");
        for (x, y) in [(0, 0), (40, 0), (0, 40), (40, 40), (5, 5), (35, 35)] {
            assert_eq!(at(t, x, y), CLEAR, "({x},{y}) outside");
        }
    });
}

#[test]
fn f1_floods_the_seed_s_index_and_f0_the_seed_s_state() {
    // A red line on row 5 and a blue one under it on row 6, both
    // foreground. F1, the default, floods the red alone; F0 every
    // foreground joined to the seed, and neither the background.
    let lines: &[u8] = b"W(I2)P[0,5]V[9,5]W(I1)P[0,6]V[9,6]W(I3)";
    let bytes = regis(&[lines, b"F[0,5]"]);
    run(&bytes, |t| {
        assert_eq!(row(t, 5, 0, 10), n_of(10, GREEN));
        assert_eq!(row(t, 6, 0, 10), n_of(10, BLUE));
        assert_eq!(at(t, 0, 4), CLEAR);
    });
    let bytes = regis(&[lines, b"F(B(F0))[0,5]"]);
    run(&bytes, |t| {
        assert_eq!(row(t, 5, 0, 10), n_of(10, GREEN));
        assert_eq!(row(t, 6, 0, 10), n_of(10, GREEN));
        assert_eq!(at(t, 0, 4), CLEAR);
        assert_eq!(at(t, 10, 5), CLEAR);
    });
    // From the background under F0: the background, up to any foreground.
    let bytes = regis(&[BOX, b"W(I1)P[20,20]V[25,20]W(I2)F(B(F0))[12,12]"]);
    run(&bytes, |t| {
        assert_eq!(at(t, 12, 12), RED);
        assert_eq!(at(t, 22, 20), BLUE, "a foreground line inside stays");
        assert_eq!(at(t, 10, 10), GRAY50);
        assert_eq!(at(t, 5, 5), CLEAR);
    });
}

#[test]
fn f_b_i_floods_up_to_the_named_index() {
    // A red outline, a blue line across its inside: bounded by index 2,
    // the flood takes the background and the blue alike.
    let body: &[u8] = b"W(I2)P[10,10]V[30,10][30,30][10,30][10,10]W(I1)P[11,20]V[29,20]W(I3)";
    for bound in [&b"F(B(I2))"[..], b"F(B(I(R)))"] {
        let bytes = regis(&[body, bound, b"[15,15]"]);
        run(&bytes, |t| {
            assert_eq!(at(t, 15, 15), GREEN, "{bound:?}");
            assert_eq!(at(t, 20, 20), GREEN, "{bound:?}: the blue line");
            assert_eq!(at(t, 15, 25), GREEN, "{bound:?}: below the line");
            assert_eq!(at(t, 10, 10), RED, "{bound:?}");
            assert_eq!(at(t, 5, 5), CLEAR, "{bound:?}");
        });
    }
    // The boundary is kept until the next and a valid S(A) restores F1.
    let bytes = regis(&[body, b"F(B(I2))S(A[0,0][799,479])W(I3)F[15,15]"]);
    run(&bytes, |t| {
        assert_eq!(at(t, 15, 15), GREEN);
        assert_eq!(at(t, 20, 20), BLUE, "F1: the seed's index only");
    });
}

#[test]
fn a_flood_writes_each_pixel_once() {
    // Complement inside an outline bounded by index 7: index 0 becomes 15,
    // itself not 7, so a flood that could reach a pixel twice would turn
    // it back.
    let bytes = regis(&[BOX, b"W(C)F(B(I7))[20,20]"]);
    run(&bytes, |t| {
        for (x, y) in [(11, 11), (20, 20), (29, 29)] {
            assert_eq!(at(t, x, y), GRAY80, "({x},{y})");
        }
        assert_eq!(at(t, 10, 10), GRAY50);
    });
}

#[test]
fn the_texture_is_the_last_shading_snapshot() {
    // W(P4,S1) snapshots 10101010 at multiplier 2 and W(S0) turns shading
    // off; the flood is textured with it, registered to the page's rows,
    // two on and two off. Overlay leaves the zeros.
    let bytes = regis(&[BOX, b"W(P4,S1)W(S0,P1)W(I2)F[20,20]"]);
    run(&bytes, |t| {
        for y in 11..30 {
            let want = match (y / 2) % 2 {
                0 => RED,
                _ => CLEAR,
            };
            assert_eq!(at(t, 20, y), want, "row {y}");
        }
    });
}

#[test]
fn the_clip_limits_what_a_flood_writes_not_where_it_spreads() {
    // An outline 0..40 split by a wall at x 20 open below row 30, and a
    // clip over rows 0 to 25: the flood goes round below the wall, outside
    // the clip, and writes the right half's rows inside it.
    let body: &[u8] =
        b"P[0,0]V[40,0][40,40][0,40][0,0]P[20,0]V[20,30]S(R1[0,0][40,25])W(I2)F[10,10]";
    let bytes = regis(&[body]);
    run(&bytes, |t| {
        assert_eq!(at(t, 10, 10), RED);
        assert_eq!(at(t, 30, 10), RED, "reached round the wall");
        assert_eq!(at(t, 10, 35), CLEAR, "outside the clip");
        assert_eq!(at(t, 30, 26), CLEAR, "outside the clip");
    });
}

/// Every placed page's pixels and width, oldest line first.
fn pages(t: &Term<'_>) -> Vec<(Vec<u32>, usize, u64)> {
    let imgs = t.images();
    let mut v: Vec<_> = imgs
        .placements()
        .filter_map(|(_, p)| {
            let v = imgs.image(p.image)?;
            let px: Vec<u32> = v.pixels.iter().map(|px| px.bits()).collect();
            Some((px, v.width.index(), p.line.value()))
        })
        .collect();
    v.sort_by_key(|p| p.2);
    v
}

#[test]
fn a_blinking_drawing_shows_the_background_in_the_hidden_phase() {
    // A blinking red line on row 0 and a steady one on row 2.
    let bytes = regis(&[b"W(I2,A1)P[0,0]V[9,0]W(A0)P[0,2]V[9,2]"]);
    run(&bytes, |t| {
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED), "shown at first");
        assert_eq!(t.set_regis_blink(RegisPhase::Hidden), RegisRepainted::Yes);
        assert_eq!(row(t, 0, 0, 10), n_of(10, BLACK), "the background, opaque");
        assert_eq!(row(t, 2, 0, 10), n_of(10, RED), "a steady line stays");
        assert_eq!(at(t, 0, 1), CLEAR, "nothing drawn stays clear");
        assert_eq!(
            t.set_regis_blink(RegisPhase::Hidden),
            RegisRepainted::No,
            "the same phase again repaints nothing"
        );
        assert_eq!(t.set_regis_blink(RegisPhase::Shown), RegisRepainted::Yes);
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED));
        assert_eq!(t.regis_counts().unbuilt, 0, "W(A) is built");
    });
    // The background is S(I)'s when the phase is shown: blue here.
    let bytes = regis(&[b"W(I2,A1)P[0,0]V[9,0]S(I1)"]);
    run(&bytes, |t| {
        t.set_regis_blink(RegisPhase::Hidden);
        assert_eq!(row(t, 0, 0, 10), n_of(10, BLUE));
    });
}

#[test]
fn only_foreground_data_blinks_and_any_other_write_clears_it() {
    // Replace with P2: eight ones in the foreground, eight zeros in the
    // background (blue, S(I1)), only the ones blinking. Then S(I2): the
    // hidden phase shows red where the ones were and the zeros keep blue.
    let bytes = regis(&[b"S(I1)W(R,P2,A1)P[0,0]V[15,0]S(I2)"]);
    run(&bytes, |t| {
        t.set_regis_blink(RegisPhase::Hidden);
        let mut want = n_of(8, RED);
        want.extend(n_of(8, BLUE));
        assert_eq!(row(t, 0, 0, 16), want);
    });
    // A steady line over part of a blinking one: that part stops.
    let bytes = regis(&[b"W(I2,A1)P[0,0]V[9,0]W(A0,I3)P[0,0]V[4,0]"]);
    run(&bytes, |t| {
        t.set_regis_blink(RegisPhase::Hidden);
        let mut want = n_of(5, GREEN);
        want.extend(n_of(5, BLACK));
        assert_eq!(row(t, 0, 0, 10), want);
    });
}

#[test]
fn w_a0_s_e_and_a_valid_s_a_end_the_attribute_and_drawn_pixels_keep_it() {
    for end in [&b"W(A0)"[..], b"S(E)W(I2)", b"S(A[0,0][799,479])W(I2)"] {
        let bytes = regis(&[b"W(I2,A1)", end, b"P[0,5]V[9,5]"]);
        run(&bytes, |t| {
            t.set_regis_blink(RegisPhase::Hidden);
            assert_eq!(row(t, 5, 0, 10), n_of(10, RED), "{end:?}");
        });
    }
    // "Clearing the subsequent application of the blink attribute by W(A0)
    // does not change the blinking status of any displayed elements".
    let bytes = regis(&[b"W(I2,A1)P[0,0]V[9,0]W(A0)"]);
    run(&bytes, |t| {
        t.set_regis_blink(RegisPhase::Hidden);
        assert_eq!(row(t, 0, 0, 10), n_of(10, BLACK));
    });
}

#[test]
fn a_page_with_no_blinking_pixel_left_stops_repainting() {
    // Every blinking pixel written over steadily: the first flip finds none
    // left and the next repaints nothing.
    let bytes = regis(&[b"W(I2,A1)P[0,0]V[9,0]W(A0)P[0,0]V[9,0]"]);
    run(&bytes, |t| {
        assert_eq!(t.regis_blink(), RegisBlink::On, "blinking pixels kept");
        assert_eq!(t.set_regis_blink(RegisPhase::Hidden), RegisRepainted::Yes);
        assert_eq!(t.regis_blink(), RegisBlink::Off, "the flip found none");
        assert_eq!(t.set_regis_blink(RegisPhase::Shown), RegisRepainted::No);
        assert_eq!(t.set_regis_blink(RegisPhase::Hidden), RegisRepainted::No);
    });
    // Nothing ever blinked: nothing to repaint.
    let bytes = regis(&[b"P[0,0]V[9,0]"]);
    run(&bytes, |t| {
        assert_eq!(t.set_regis_blink(RegisPhase::Hidden), RegisRepainted::No);
    });
}

#[test]
fn a_page_left_behind_keeps_its_own_colours() {
    // A blinking line, hidden; the text scrolls three lines and the next
    // drawing begins a page at the window's top. The page left behind
    // shows the line in its own colour, nothing repainting it again.
    let bytes = regis(&[b"W(I2,A1)P[0,0]V[9,0]"]);
    run(&bytes, |t| {
        t.set_regis_blink(RegisPhase::Hidden);
        support::feed(t, b"\x1b[50;1H\n\n\n");
        support::feed(t, &regis(&[b"W(A0)P[0,100]V[9,100]"]));
        let p = pages(t);
        assert_eq!(p.len(), 2, "a page each");
        let (old, w, _) = &p[0];
        assert_eq!(old[..10].to_vec(), n_of(10, RED));
        assert_eq!(old[100 * w], CLEAR);
        t.set_regis_blink(RegisPhase::Shown);
        t.set_regis_blink(RegisPhase::Hidden);
        assert_eq!(pages(t)[0].0[..10].to_vec(), n_of(10, RED));
    });
}

#[test]
fn a_blinking_flood_and_a_blinking_character_blink() {
    let bytes = regis(&[BOX, b"W(I2,A1)F[20,20]"]);
    run(&bytes, |t| {
        t.set_regis_blink(RegisPhase::Hidden);
        assert_eq!(at(t, 20, 20), BLACK);
        assert_eq!(at(t, 10, 10), GRAY50, "the steady boundary");
    });
}

// The output cursor shows while an envelope waits, at the drawing
// position: `S(C0)` turns it off where a test reads the page mid-envelope.

#[test]
fn the_second_surface_shows_only_while_it_is_displayed() {
    run(b"", |t| {
        support::feed(t, b"\x1bPpS(C0)W(I2)P(P1)P[0,0]V[9,0]");
        assert_eq!(pages(t).len(), 0, "drawn on a surface not displayed");
        support::feed(t, b"S(P1)");
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED), "displayed");
        support::feed(t, b"\x1b\\");
        assert_eq!(row(t, 0, 0, 10), n_of(10, CLEAR), "the first at the end");
        assert_eq!(t.regis_counts().unbuilt, 0, "P(P) and S(P) are built");
    });
}

/// `bytes` fed in one read to the corpus screen, 200 by 50 cells of 10
/// by 20 pixels.
fn one_read(bytes: &[u8]) -> Vec<(Vec<u32>, usize, u64)> {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(baud_domain::ImagePx::of(10), baud_domain::ImagePx::of(20));
    support::feed(&mut t, bytes);
    pages(&t)
}

/// An envelope that ends with the empty second displayed, `end` after it
/// in the same read: the page left showing the first's red line.
fn the_first_kept_after(end: &[u8]) {
    let mut bytes = b"\x1bPpS(C0)W(I2)P[0,0]V[9,0]S(P1)".to_vec();
    bytes.extend_from_slice(end);
    let p = one_read(&bytes);
    assert_eq!(p.len(), 1, "the page kept");
    assert_eq!(p[0].0[..10].to_vec(), n_of(10, RED), "the first shown");
}

#[test]
fn text_after_st_on_the_second_keeps_the_first() {
    the_first_kept_after(b"\x1b\\\x1b[10;10Hx");
    // The control: no `S(P1)`.
    let p = one_read(b"\x1bPpS(C0)W(I2)P[0,0]V[9,0]\x1b\\\x1b[10;10Hx");
    assert_eq!(p[0].0[..10].to_vec(), n_of(10, RED));
}

#[test]
fn text_after_a_cancel_on_the_second_keeps_the_first() {
    the_first_kept_after(b"\x18\x1b[10;10Hx");
}

#[test]
fn an_erase_after_st_on_the_second_keeps_the_first() {
    the_first_kept_after(b"\x1b\\\x1b[10;1H\x1b[2K");
}

#[test]
fn text_after_an_escape_that_leaves_the_second_keeps_the_first() {
    the_first_kept_after(b"\x1b[10;10Hx");
}

#[test]
fn an_erase_of_all_of_the_first_frees_the_page_once_the_first_shows() {
    // The first's line lies on the first text line, and an erase of that
    // line clears all of it: the page goes, as it goes without `S(P1)`,
    // once the first is displayed again, by `ST` at once and after a
    // cancel at the read's end.
    let erase = b"\x1b[1;1H\x1b[2K";
    let mut control = b"\x1bPpS(C0)W(I2)P[0,0]V[9,0]\x1b\\".to_vec();
    control.extend_from_slice(erase);
    assert_eq!(one_read(&control).len(), 0, "the control");
    for end in [&b"\x1b\\"[..], b"\x18"] {
        let mut bytes = b"\x1bPpS(C0)W(I2)P[0,0]V[9,0]S(P1)".to_vec();
        bytes.extend_from_slice(end);
        bytes.extend_from_slice(erase);
        assert_eq!(one_read(&bytes).len(), 0, "{end:?}");
    }
}

#[test]
fn each_surface_keeps_its_own_picture() {
    let bytes = regis(&[b"S(C0)W(I1)P[0,2]V[9,2]P(P1)W(I2)P[0,0]V[9,0]S(P1)"]);
    run(&bytes[..bytes.len() - 2], |t| {
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED));
        assert_eq!(
            row(t, 2, 0, 10),
            n_of(10, CLEAR),
            "the first's line not shown"
        );
        support::feed(t, b"S(P0)");
        assert_eq!(row(t, 0, 0, 10), n_of(10, CLEAR));
        assert_eq!(row(t, 2, 0, 10), n_of(10, BLUE));
        // Values other than 0 and 1 are ignored.
        support::feed(t, b"S(P2)P(P7)W(I3)P[0,4]V[9,4]");
        assert_eq!(
            row(t, 4, 0, 10),
            n_of(10, CLEAR),
            "still drawn on the second"
        );
        support::feed(t, b"S(P1)");
        assert_eq!(row(t, 4, 0, 10), n_of(10, GREEN));
    });
}

#[test]
fn the_drawing_surface_persists_and_each_entry_displays_the_first() {
    run(&regis(&[b"S(C0)P(P1)"]), |t| {
        support::feed(t, &regis(&[b"W(I2)P[0,0]V[9,0]"]));
        assert_eq!(
            pages(t).len(),
            0,
            "drawn on the second, the first displayed"
        );
        support::feed(t, b"\x1bPpS(P1)");
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED));
        // A cancel ends the envelope as ST does.
        support::feed(t, b"\x18");
        assert_eq!(row(t, 0, 0, 10), n_of(10, CLEAR));
    });
}

#[test]
fn s_e_erases_the_drawing_surface() {
    let bytes = regis(&[b"S(C0)W(I2)P[0,0]V[9,0]P(P1)S(E)S(P0)"]);
    run(&bytes[..bytes.len() - 2], |t| {
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED), "the first untouched");
        support::feed(t, b"S(P1)");
        assert_eq!(row(t, 0, 0, 10), n_of(10, BLACK), "the second erased");
        assert_eq!(at(t, 400, 300), BLACK);
    });
}

#[test]
fn the_second_surface_is_the_screen_s_and_the_text_scrolls_the_first() {
    run(
        &regis(&[b"S(C0)W(I2)P[0,0]V[9,0]P(P1)W(I1)P[0,0]V[9,0]"]),
        |t| {
            support::feed(t, b"\x1b[50;1H\n\n\n");
            support::feed(t, b"\x1bPpS(P1)");
            let p = pages(t);
            assert_eq!(p.len(), 2, "the first's page and one at the window's top");
            assert_eq!(
                p[0].0[..10].to_vec(),
                n_of(10, RED),
                "the first scrolled away"
            );
            assert_eq!(
                p[1].0[..10].to_vec(),
                n_of(10, BLUE),
                "the second where it was drawn"
            );
        },
    );
}

#[test]
fn the_cursors_show_on_the_displayed_surface_only() {
    // The output cursor shows while the envelope waits, but not while the
    // surface drawn on is not displayed.
    run(b"", |t| {
        support::feed(t, b"\x1bPpP[0,0]V[1,0]P(P1)P[300,200]");
        assert_eq!(at(t, 290, 200), CLEAR, "no cursor on the first");
        support::feed(t, b"S(P1)");
        assert_ne!(
            at(t, 290, 200),
            CLEAR,
            "the cursor on the second, displayed"
        );
    });
}

#[test]
fn a_reset_forgets_the_second_surface() {
    // RIS drops every image: the second surface made again after it is a
    // plane of its own, and drawing on it leaves the first's alone. A
    // Sixel first, so the second's slot is the one a new page's plane takes
    // after the reset.
    let mut bytes = b"\x1bPq#1!10~\x1b\\".to_vec();
    bytes.extend(regis(&[b"S(C0)P(P1)W(I1)P[0,2]V[9,2]"]));
    run(&bytes, |t| {
        support::feed(t, b"\x1bc");
        let again = regis(&[b"S(C0)W(I2)P[0,0]V[9,0]P(P1)W(I1)P[0,2]V[9,2]S(P0)"]);
        support::feed(t, &again[..again.len() - 2]);
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED));
        assert_eq!(
            row(t, 2, 0, 10),
            n_of(10, CLEAR),
            "the second's line not on the first"
        );
        support::feed(t, b"S(P1)");
        assert_eq!(row(t, 2, 0, 10), n_of(10, BLUE));
        assert_eq!(row(t, 0, 0, 10), n_of(10, CLEAR));
    });
}

#[test]
fn a_resize_while_the_second_is_displayed_keeps_the_first() {
    // The empty second displayed, the read ends inside the envelope, and
    // the embedder resizes: the first's line is there again at the end.
    run(b"", |t| {
        support::feed(t, b"\x1bPpS(C0)W(I2)P[0,0]V[9,0]S(P1)");
        assert_eq!(row(t, 0, 0, 10), n_of(10, CLEAR), "the empty second shown");
        let _ = t.resize(&WinSize {
            ws_col: 150,
            ws_row: 40,
        });
        support::feed(t, b"\x1b\\");
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED), "the first's line kept");
    });
    // With cells, a resize makes a page of another size at the next
    // drawing: the page it leaves shows the first's line, as the shell's
    // text page does, and the new page the second, made again empty at the
    // new size, until the envelope ends.
    let ws = WinSize {
        ws_col: 80,
        ws_row: 24,
    };
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(baud_domain::ImagePx::of(10), baud_domain::ImagePx::of(20));
    support::feed(&mut t, b"\x1bPpS(C0)W(I2)P[0,0]V[9,0]S(P1)");
    let _ = t.resize(&WinSize {
        ws_col: 60,
        ws_row: 20,
    });
    support::feed(&mut t, b"W(I1)P[0,100]V[9,100]");
    let p = pages(&t);
    assert_eq!(p.len(), 2);
    let old = p.iter().find(|q| q.1 == 800).expect("the page left behind");
    assert_eq!(old.0[..10].to_vec(), n_of(10, RED), "it shows the first");
    let new = p.iter().find(|q| q.1 == 600).expect("the new page");
    assert!(new.0.iter().all(|&c| c == CLEAR), "the second, empty");
    support::feed(&mut t, b"\x1b\\");
    let p = pages(&t);
    let new = p.iter().find(|q| q.1 == 600).expect("the new page");
    let w = new.1;
    // 600 by 400: the device's 800 by 480 fitted, 599/799 a pixel, so row
    // 100 is row 75.
    assert_eq!(
        new.0[75 * w..75 * w + 7].to_vec(),
        n_of(7, BLUE),
        "the first's new line"
    );
}

#[test]
fn s_d1_names_the_scrolling_model_and_s_d0_is_counted() {
    // The scrolling model, the VT340's, is the one baud has: `S(D1)` names
    // it and a scroll still moves the data; `S(D0)` asks for the windowing
    // model and is counted, not acted on.
    let bytes = regis(&[b"W(I2)P[100,0]V[109,0]S(D1)S[+100,+0]"]);
    run(&bytes, |t| {
        assert_eq!(t.regis_counts().unbuilt, 0);
        assert_eq!(row(t, 0, 0, 10), n_of(10, RED), "the data moved left");
    });
    for d in [&b"S(D0)"[..], b"S(D)", b"S(D2)"] {
        let bytes = regis(&[b"W(I2)P[100,0]V[109,0]", d, b"S[+100,+0]"]);
        run(&bytes, |t| {
            assert_eq!(t.regis_counts().unbuilt, 1, "{d:?}");
            assert_eq!(row(t, 0, 0, 10), n_of(10, RED), "{d:?}: still scrolling");
        });
    }
}
