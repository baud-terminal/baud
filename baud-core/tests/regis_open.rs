//! **ReGIS's Open Extensions, pixel for pixel**, against `[[source:
//! dec-std-070]]` 070-8 § 8.8 and the VT330/VT340 manual where it has them.
//!
//! The VT340's default map gives the colours (`[[source:
//! vt3xx-screen-control]]` Table 2-3), a percentage `p` being `p * 255 / 100`
//! rounded: index 0 black, 1 blue (20, 20, 80), 2 red (80, 13, 13), 3 green
//! (20, 80, 20), 4 magenta (80, 20, 80), 5 cyan (20, 80, 80), 6 yellow
//! (80, 80, 20), 7 gray 53%.

use baud_core::*;
use baud_domain::{ImageBudget, Pixel};

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
