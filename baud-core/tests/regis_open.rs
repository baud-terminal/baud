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
