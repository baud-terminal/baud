//! **ReGIS hard copy prints the visible image as a sixel dump**:
//! `[[source: dec-std-070]]` 070-8 § 8.8.1.4.4's `S(H)` and 070-7 § 7.8's
//! graphics printing, the dump read back pixel for pixel with the decoder
//! baud holds to 070-9. Cells are 10 by 20 pixels on an 80 by 24 screen, so
//! the page is the VT300's 800 by 480 and a ReGIS coordinate is a pixel.

use baud_core::*;
use baud_domain::{
    Chan, GraphicsPrint, ImageBudget, ImagePx, Pixel, PrintBackground, PrintColour, PrintRotation,
    PrintSyntax, Sixel, SixelAspect, SixelCanvas, SixelPx,
};

mod support;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

/// A 40 by 36 Sixel at 1:1, every pixel register 1's red; `P2` 1, so no
/// background fill reaches past its data.
const RED: &[u8] = b"\x1bP9;1q#1;2;100;0;0#1!40~-!40~-!40~-!40~-!40~-!40~\x1b\\";

/// A face whose every character fills its cell.
fn solid(_: char, w: ImagePx, h: ImagePx, set: &mut dyn FnMut(ImagePx, ImagePx)) {
    for y in 0..h.raw() {
        for x in 0..w.raw() {
            set(ImagePx::of(x), ImagePx::of(y));
        }
    }
}

fn run(budget: usize, bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let mut owned = Owned::new(&WS)
        .with_images(ImageBudget::of(8 << 20))
        .with_printer(PrintBudget::of_bytes(budget));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    t.set_regis_font(solid);
    t.set_printer(PrinterPort::Ready);
    support::feed(&mut t, bytes);
    f(&mut t);
}

fn jobs(t: &mut Term<'_>) -> Vec<Vec<u8>> {
    let out = t.print_jobs().map(|j| j.bytes.to_vec()).collect();
    t.clear_print_jobs();
    out
}

/// A dump decoded as baud's printer reads it, an undefined register black:
/// its raster attributes' size and its pixels.
fn decode(job: &[u8]) -> (usize, usize, Vec<Pixel>) {
    let q = job.iter().position(|&b| b == b'q').expect("an introducer");
    let st = job
        .windows(2)
        .rposition(|w| w == b"\x1b\\")
        .expect("a terminator");
    let data = &job[q + 1..st];
    assert_eq!(data[0], b'"', "raster attributes first");
    let rest = &data[1..];
    let end = rest
        .iter()
        .position(|b| !(b.is_ascii_digit() || *b == b';'))
        .unwrap_or(rest.len());
    let raster = String::from_utf8_lossy(&rest[..end]);
    let n: Vec<usize> = raster.split(';').filter_map(|v| v.parse().ok()).collect();
    let (w, h) = (n[2], n[3]);
    let mut px = vec![Pixel::CLEAR; w * h];
    let mut c = SixelCanvas::new(&mut px, SixelPx::of(u16::try_from(w).unwrap()));
    let ink = Pixel::opaque(Chan::from_u8(0), Chan::from_u8(0), Chan::from_u8(0));
    Sixel::decode_inked(data, SixelAspect::ONE_TO_ONE, ink, &mut c);
    (w, h, px)
}

/// The lit pixels' bounding box, `(x0, y0, x1, y1)`, exclusive at the end.
fn ink_box(w: usize, px: &[Pixel]) -> Option<(usize, usize, usize, usize)> {
    let lit: Vec<(usize, usize)> = px
        .iter()
        .enumerate()
        .filter(|(_, p)| **p != Pixel::CLEAR)
        .map(|(i, _)| (i % w, i / w))
        .collect();
    let x0 = lit.iter().map(|p| p.0).min()?;
    let y0 = lit.iter().map(|p| p.1).min()?;
    let x1 = lit.iter().map(|p| p.0).max()? + 1;
    let y1 = lit.iter().map(|p| p.1).max()? + 1;
    Some((x0, y0, x1, y1))
}

/// **`S(H)` prints the whole page as a Level 2 dump**: SSU, `DCS 0;1;6 q`,
/// raster attributes at 1:1 with the page's size, the sixels and `ST`, the
/// character in its cell inked, nothing sent to the host. Red before hard
/// copy was built: `S(H)` was counted unbuilt and printed nothing.
#[test]
fn s_h_prints_the_screen_as_a_level_two_dump() {
    run(1 << 20, b"A\x1bPpS(H)\x1b\\", |t| {
        let j = jobs(t);
        assert_eq!(j.len(), 1, "one job");
        assert!(
            j[0].starts_with(b"\x1b[2 I\x1bP0;1;6q\"1;1;800;480"),
            "{:?}",
            &j[0][..30]
        );
        assert!(j[0].ends_with(b"\x1b\\"));
        let (w, h, px) = decode(&j[0]);
        assert_eq!((w, h), (800, 480));
        assert_eq!(ink_box(w, &px), Some((0, 0, 10, 20)), "the A's cell");
        assert_eq!(t.reply(), b"");
        assert_eq!(t.regis_counts().unbuilt, 0);
    });
}

/// **Two corners print the rectangle between them**, both inside it; one
/// corner and the drawing position likewise.
#[test]
fn a_region_prints_between_its_corners() {
    run(1 << 20, b"AB\r\nCD\x1bPpS(H[5,10][24,29])\x1b\\", |t| {
        let j = jobs(t);
        let (w, h, px) = decode(&j[0]);
        assert_eq!((w, h), (20, 20));
        // x 5-24 and y 10-29 take a part of each of A (x 0-9, y 0-19),
        // B (x 10-19), C (y 20-39) and D, and of the blank third column.
        let lit = |x: usize, y: usize| px[y * w + x] != Pixel::CLEAR;
        assert!(
            lit(0, 0) && lit(14, 9) && lit(4, 10) && lit(5, 10),
            "A, B, C, D"
        );
        assert!(
            !lit(15, 0) && !lit(15, 10) && !lit(19, 19),
            "the third column"
        );
    });
    run(1 << 20, b"\x1bPpP[30,40]S(H[+10,+20])\x1b\\", |t| {
        let j = jobs(t);
        assert!(j[0].starts_with(b"\x1b[2 I\x1bP0;1;6q\"1;1;11;21"));
    });
}

/// **A Sixel prints in its colour and the ReGIS page with it**: in colour
/// the red image's pixels decode red, register 1 named in HLS with red at
/// 120 degrees; the ReGIS fill's pixels in its map entry.
#[test]
fn pictures_print_over_the_text() {
    run(1 << 20, b"\x1b[H", |t| {
        support::feed(t, RED);
        support::feed(
            t,
            b"\x1bPpF(V[100,100][150,100][150,120][100,120])S(H)\x1b\\",
        );
        t.set_graphics_print(GraphicsPrint {
            colour: PrintColour::Colour,
            ..GraphicsPrint::DEFAULT
        });
        support::feed(t, b"\x1bPpS(H)\x1b\\");
        let j = jobs(t);
        assert_eq!(j.len(), 2);
        let (w, _, px) = decode(&j[0]);
        let lit = |x: usize, y: usize| px[y * w + x] != Pixel::CLEAR;
        assert!(
            lit(0, 0) && lit(39, 35) && !lit(40, 0) && !lit(0, 36),
            "the Sixel, monochrome"
        );
        assert!(
            lit(110, 110) && !lit(90, 110) && !lit(160, 110) && !lit(110, 130),
            "the fill"
        );
        let text = String::from_utf8_lossy(&j[1]);
        assert!(
            text.contains(";1;120;50;100"),
            "red in HLS: {}",
            &text[..80]
        );
        let (w, _, px) = decode(&j[1]);
        let red = Pixel::opaque(Chan::from_u8(255), Chan::from_u8(0), Chan::from_u8(0));
        assert_eq!(px[0], red);
        assert_eq!(px[35 * w + 39], red);
    });
}

/// **A Sixel recoloured earlier in the read prints in its new colour**:
/// under shared registers (`?1070` reset) the red picture, a body that
/// only redefines its register blue, and `S(H)` in colour, all in one
/// read, print blue, the colour the screen shows after the read ("you do
/// not have to redraw an image to change its color",
/// `[[source: vt3xx-screen-control]]`). The control, the same read with
/// no palette, prints red.
#[test]
fn a_sixel_recoloured_in_the_same_read_prints_in_its_new_colour() {
    let blue = Pixel::opaque(Chan::from_u8(0), Chan::from_u8(0), Chan::from_u8(255));
    let red = Pixel::opaque(Chan::from_u8(255), Chan::from_u8(0), Chan::from_u8(0));
    let palette: &[u8] = b"\x1bP9;1q#1;2;0;0;100\x1b\\";
    for (between, want) in [(palette, blue), (b"".as_slice(), red)] {
        run(1 << 20, b"", |t| {
            t.set_sixel_claim(baud_domain::SixelClaim::Yes);
            t.set_graphics_print(GraphicsPrint {
                colour: PrintColour::Colour,
                ..GraphicsPrint::DEFAULT
            });
            let read = [
                b"\x1b[H\x1b[?1070l".as_slice(),
                RED,
                between,
                b"\x1bPpS(H)\x1b\\",
            ]
            .concat();
            assert!(read.len() <= READ_CHUNK, "one read");
            support::feed(t, &read);
            let j = jobs(t);
            assert_eq!(j.len(), 1);
            let (w, _, px) = decode(&j[0]);
            assert_eq!((px[0], px[35 * w + 39]), (want, want), "{between:?}");
        });
    }
}

/// **A Sixel's background fill prints as the background**: its positions
/// are "set to the current background color" (`[[source: vt3xx-sixel]]`),
/// so the printed page shows no ink there and no text under them, where a
/// fill word read as a colour printed black. The data's own pixels print;
/// the text beside the extent prints, the control.
#[test]
fn a_sixel_fill_prints_as_the_background() {
    run(1 << 20, b"\x1b[H", |t| {
        support::feed(t, b"\x1b[1;1HEEEEEEEEEE\x1b[2;1HEEEEEEEEEE\x1b[1;1H");
        support::feed(
            t,
            b"\x1bP9;0;0q\"1;1;60;40#1;2;100;0;0#1!20i-!20i-!20i-!20A\x1b\\",
        );
        support::feed(t, b"\x1bPpS(H)\x1b\\");
        let j = jobs(t);
        let (w, _, px) = decode(&j[0]);
        let lit = |x: usize, y: usize| px[y * w + x] != Pixel::CLEAR;
        assert!(lit(0, 1) && lit(19, 19), "the data prints");
        assert!(!lit(0, 0), "a zero position of the data is the background");
        let inked = (0..40)
            .flat_map(|y| (20..60).map(move |x| (x, y)))
            .filter(|&(x, y)| lit(x, y))
            .count();
        assert_eq!(
            inked, 0,
            "the fill hides the text under it and prints no ink"
        );
        let beside = (0..40)
            .flat_map(|y| (60..100).map(move |x| (x, y)))
            .filter(|&(x, y)| lit(x, y))
            .count();
        assert!(beside > 0, "the control: the text beside the extent prints");
    });
}

/// **A shared Sixel drawn with no foreground said prints as the screen
/// shows it**: register 1, never defined, over text, with `P2` 0 over 60 by
/// 40. The terminal is told no foreground, so the register paints a clear
/// ink, drawn as the fill there: the page prints no ink and none of the
/// text where the picture is. Once a palette defines register 1 red, the
/// data prints red and the fill past it still prints nothing. The
/// control: the text beside the picture prints both times.
#[test]
fn a_shared_sixel_in_a_clear_ink_prints_as_the_screen_shows_it() {
    let red = Pixel::opaque(Chan::from_u8(255), Chan::from_u8(0), Chan::from_u8(0));
    run(1 << 20, b"\x1b[H", |t| {
        t.set_sixel_claim(baud_domain::SixelClaim::Yes);
        t.set_graphics_print(GraphicsPrint {
            colour: PrintColour::Colour,
            ..GraphicsPrint::DEFAULT
        });
        support::feed(
            t,
            b"\x1b[?1070l\x1b[1;1HEEEEEEEEEE\x1b[2;1HEEEEEEEEEE\x1b[1;1H",
        );
        support::feed(t, b"\x1bP9;0;0q\"1;1;60;40#1!20~-!20~-!20~-!20~\x1b\\");
        for palette in [&b""[..], b"\x1bP9;1q#1;2;100;0;0\x1b\\"] {
            support::feed(t, palette);
            support::feed(t, b"\x1bPpS(H)\x1b\\");
            let j = jobs(t);
            let (w, _, px) = decode(&j[0]);
            let lit = |x: usize, y: usize| px[y * w + x] != Pixel::CLEAR;
            let inked = |xs: core::ops::Range<usize>, ys: core::ops::Range<usize>| {
                ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
                    .filter(|&(x, y)| lit(x, y))
                    .count()
            };
            let what = palette.escape_ascii().to_string();
            assert_eq!(inked(20..60, 0..40), 0, "{what}: the fill prints nothing");
            assert!(
                inked(60..100, 0..40) > 0,
                "{what}: the control, the text beside"
            );
            match palette.is_empty() {
                true => assert_eq!(inked(0..20, 0..24), 0, "the clear ink prints nothing"),
                false => assert_eq!(
                    (px[0], px[23 * w + 19]),
                    (red, red),
                    "the data prints in the colour defined"
                ),
            }
        }
    });
}

/// **The offset, the expanded grid and the rotated dump**: `P[30,12]` puts
/// blank pixels before the image; DECGEPM sets grid 9 and DECRQM answers
/// it; rotation turns the raster; RGB names colours in percent; the
/// background prints as register 0.
#[test]
fn offset_size_rotation_syntax_and_background() {
    run(
        1 << 20,
        b"\x1bPpS(H(P[30,12]))\x1b\\\x1b[?43h\x1b[?43$p",
        |t| {
            let j = jobs(t);
            assert!(j[0].starts_with(b"\x1b[2 I\x1bP0;1;6q\"1;1;830;492"));
            assert_eq!(t.reply(), b"\x1b[?43;1$y");
            support::feed(t, b"\x1bPpS(H)\x1b\\");
            let j = jobs(t);
            assert!(
                j[0].starts_with(b"\x1b[2 I\x1bP0;1;9q\"1;1;830;492"),
                "the offset is kept"
            );
            t.set_graphics_print(GraphicsPrint {
                colour: PrintColour::Colour,
                syntax: PrintSyntax::Rgb,
                background: PrintBackground::On,
                rotation: PrintRotation::Rotated,
            });
            support::feed(t, b"\x1bPpS(A[0,0][799,479])S(H)\x1b\\");
            let j = jobs(t);
            let text = String::from_utf8_lossy(&j[0]);
            assert!(
                text.starts_with("\x1b[2 I\x1bP0;2;9q\"1;1;480;800#0;2;"),
                "{}",
                &text[..40]
            );
        },
    );
}

/// **DECGEPM is compressed at power-on and after `RIS`, and `DECSTR` keeps
/// it**, as the VT330/VT340's reset table names no graphics print mode;
/// XTSAVE and XTRESTORE carry it; the user's four choices start at the
/// manual's defaults.
#[test]
fn decgepm_resets_on_ris_and_survives_decstr() {
    run(
        4096,
        b"\x1b[?43$p\x1b[?43h\x1b[!p\x1b[?43$p\x1bc\x1b[?43$p",
        |t| {
            assert_eq!(t.reply(), b"\x1b[?43;2$y\x1b[?43;1$y\x1b[?43;2$y");
            assert_eq!(t.graphics_print(), GraphicsPrint::DEFAULT);
        },
    );
    run(
        4096,
        b"\x1b[?43h\x1b[?43s\x1b[?43l\x1b[?43r\x1b[?43$p",
        |t| {
            assert_eq!(t.reply(), b"\x1b[?43;1$y");
        },
    );
}

/// **"Graphics to Printer" is accepted and "Graphics to Host" refused**:
/// `CSI ? i` and `CSI ? 0 i` change nothing, a hard copy always going to
/// the printer; `CSI ? 2 i` would send the screen's image to the host and is
/// counted with the refusals that are unsafe.
#[test]
fn graphics_to_printer_is_accepted_and_to_host_refused() {
    run(4096, b"\x1b[?i\x1b[?0i\x1b[?2i", |t| {
        assert_eq!(t.unsupported, 0);
        assert_eq!(t.print_refusals().unsafe_kinds, 1);
        assert_eq!(t.reply(), b"");
    });
}

/// **With no printer a hard copy is ignored and counted**; under DECPFF
/// one ends in a form feed; with no room in the area for its working space
/// it prints nothing and is counted.
#[test]
fn no_printer_form_feed_and_no_room() {
    let mut owned = Owned::new(&WS).with_printer(PrintBudget::of_bytes(4096));
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, b"\x1bPpS(H)\x1b\\");
    assert_eq!(t.print_refusals().no_printer, 1);
    assert_eq!(t.print_jobs().count(), 0);
    run(1 << 20, b"\x1b[?18h\x1bPpS(H[0,0][9,9])\x1b\\", |t| {
        let j = jobs(t);
        assert!(j[0].ends_with(b"\x1b\\\x0c"));
    });
    run(4096, b"\x1bPpS(H)\x1b\\", |t| {
        assert_eq!(t.print_refusals().hard_copies_cut, 1);
        assert_eq!(t.print_jobs().count(), 0, "an empty job opens no panel");
    });
}

/// **Reverse video swaps a cell's colours, and an invisible character is
/// not drawn**: in monochrome a reversed character on the default
/// background is index 0 in an inked cell, so its solid glyph leaves the
/// cell blank; an invisible one inks nothing.
#[test]
fn reverse_and_invisible() {
    run(
        1 << 20,
        b"\x1b[7m \x1b[0mA\x1b[8mB\x1bPpS(H[0,0][29,19])\x1b\\",
        |t| {
            let j = jobs(t);
            let (w, _, px) = decode(&j[0]);
            let lit = |x: usize, y: usize| px[y * w + x] != Pixel::CLEAR;
            assert!(
                lit(0, 0) && lit(9, 19),
                "a reversed space is its background"
            );
            assert!(lit(10, 0), "A");
            assert!(!lit(20, 0) && !lit(29, 19), "an invisible B");
        },
    );
    run(1 << 20, b"\x1b[7mA\x1bPpS(H[0,0][9,19])\x1b\\", |t| {
        let j = jobs(t);
        let (w, _, px) = decode(&j[0]);
        assert_eq!(ink_box(w, &px), None, "a reversed solid glyph is index 0");
    });
}

/// The pixels a dump inks.
fn lit_count(px: &[Pixel]) -> usize {
    px.iter().filter(|p| **p != Pixel::CLEAR).count()
}

/// **A pixel written with index 0 has no bit set in any plane**, so it
/// prints nothing in black and white, "a logical OR of all pixels which
/// are selected for any color", and in colour only when the background is
/// printed (`[[source: dec-std-070]]` 070-7 § 7.8.2.1). It covers the text
/// beneath it, as it does on the screen: after `S(E)` the characters under
/// the page print nothing and only the vector inks; a line erase-written
/// over text clears it.
#[test]
fn index_zero_prints_no_ink_and_covers_the_text() {
    let text = b"A\x1b[6;11HXXXXXXXXXXX\x1b[H";
    let mut job = text.to_vec();
    job.extend_from_slice(b"\x1bPpS(E)P[100,100]V[200,100]S(H)\x1b\\");
    run(1 << 20, &job, |t| {
        let j = jobs(t);
        let (w, _, px) = decode(&j[0]);
        assert_eq!(ink_box(w, &px), Some((100, 100, 201, 101)), "the vector");
        assert_eq!(lit_count(&px), 101);
        t.set_graphics_print(GraphicsPrint {
            colour: PrintColour::Colour,
            ..GraphicsPrint::DEFAULT
        });
        support::feed(t, b"\x1bPpS(H)\x1b\\");
        let j = jobs(t);
        let (w, _, px) = decode(&j[0]);
        assert_eq!(
            ink_box(w, &px),
            Some((100, 100, 201, 101)),
            "in colour the erased page is clear"
        );
    });
    let mut job = text.to_vec();
    job.extend_from_slice(b"\x1bPpW(E)P[100,100]V[200,100]S(H[0,0][799,479])\x1b\\");
    run(1 << 20, &job, |t| {
        let j = jobs(t);
        let (w, _, px) = decode(&j[0]);
        let lit = |x: usize, y: usize| px[y * w + x] != Pixel::CLEAR;
        assert!(lit(0, 0), "the A");
        assert!(lit(150, 101) && lit(150, 119), "the Xs below the line");
        assert!(
            (100..=200).all(|x| !lit(x, 100)),
            "the erase-written line covers the Xs"
        );
    });
}

/// **A magnified page prints as the screen shows it**: under `S(S2)` the
/// plane's vector at row 100, x 100-200, shows over page rows 200-201,
/// x 200-401, and that is what the visible image holds; a region's
/// positions name plane pixels, so its dump is the block they show as.
#[test]
fn a_magnified_page_prints_as_it_is_shown() {
    run(
        1 << 20,
        b"\x1bPpS(E)S(S2)P[100,100]V[200,100]S(H)\x1b\\",
        |t| {
            let j = jobs(t);
            let (w, _, px) = decode(&j[0]);
            assert_eq!(ink_box(w, &px), Some((200, 200, 402, 202)));
            assert_eq!(lit_count(&px), 202 * 2);
        },
    );
    run(
        1 << 20,
        b"\x1bPpS(E)S(S2)P[100,100]V[200,100]S(H[100,100][200,100])\x1b\\",
        |t| {
            let j = jobs(t);
            let (w, h, px) = decode(&j[0]);
            assert_eq!((w, h), (202, 2));
            assert_eq!(lit_count(&px), 202 * 2);
        },
    );
}

/// **A page whose width is no multiple of four reads its plane by its
/// rows' stride**: 81 columns of 10 pixels make an 810 pixel page, whose
/// plane rows are padded to whole words; after `S(E)` only the vector's
/// row inks.
#[test]
fn a_page_of_any_width_reads_its_plane_by_rows() {
    let ws = WinSize {
        ws_col: 81,
        ws_row: 24,
    };
    let mut owned = Owned::new(&ws)
        .with_images(ImageBudget::of(8 << 20))
        .with_printer(PrintBudget::of_bytes(1 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    t.set_regis_font(solid);
    t.set_printer(PrinterPort::Ready);
    support::feed(&mut t, b"\x1bPpS(E)P[100,100]V[200,100]S(H)\x1b\\");
    let j = jobs(&mut t);
    let (w, h, px) = decode(&j[0]);
    assert_eq!((w, h), (810, 480));
    let (x0, y0, x1, y1) = ink_box(w, &px).expect("the vector");
    assert_eq!(y1, y0 + 1, "one row");
    assert!((95..=110).contains(&x0) && (195..=210).contains(&x1));
    assert_eq!(lit_count(&px), x1 - x0, "every pixel of it, nothing else");
}

/// **A page left behind keeps no plane**, so a pixel of it showing index
/// 0's colour prints as index 0: after the cell grows, the next drawing
/// is on a page of its own, and the erased page beneath it inks only its
/// vector.
#[test]
fn a_page_left_behind_prints_index_zero_by_its_colour() {
    run(1 << 20, b"\x1bPpS(E)P[100,100]V[200,100]\x1b\\", |t| {
        t.set_cell_size(ImagePx::of(10), ImagePx::of(21));
        support::feed(t, b"\x1bPpP[300,300]V[310,300]S(H)\x1b\\");
        let j = jobs(t);
        let (w, h, px) = decode(&j[0]);
        assert_eq!((w, h), (800, 504), "the grown page");
        let lit = |x: usize, y: usize| px[y * w + x] != Pixel::CLEAR;
        assert!(lit(150, 100), "the page left behind's vector");
        assert!(!lit(150, 200) && !lit(700, 400), "its erased pixels");
        assert!(lit_count(&px) < 200, "{} lit", lit_count(&px));
    });
}

/// **A large offset makes a page's dump, not a sheet's worth of pages**:
/// `S(H(P[60000,60000]))` is held to 17 inches at grid 6, 2040 pixels
/// each way.
#[test]
fn a_large_offset_is_held_to_a_sheet() {
    run(1 << 20, b"\x1bPpS(H(P[60000,60000]))\x1b\\", |t| {
        let j = jobs(t);
        assert!(
            j[0].starts_with(b"\x1b[2 I\x1bP0;1;6q\"1;1;2840;2520"),
            "{}",
            String::from_utf8_lossy(&j[0][..30])
        );
    });
}

/// **A blinking ReGIS pixel prints its own colour in every phase**:
/// the hidden phase shows the background in its place (070-8 § 8.8.3.1),
/// the faint one half way to it, and a hard copy of it in either phase
/// still prints the line.
#[test]
fn a_blinking_pixel_prints_its_own_colour_when_hidden() {
    for phase in [
        baud_domain::RegisPhase::Hidden,
        baud_domain::RegisPhase::Faint,
    ] {
        run(1 << 20, b"\x1bPpW(I2,A1)P[100,100]V[200,100]\x1b\\", |t| {
            t.set_graphics_print(GraphicsPrint {
                colour: PrintColour::Colour,
                ..GraphicsPrint::DEFAULT
            });
            t.set_regis_blink(phase);
            support::feed(t, b"\x1bPpS(H)\x1b\\");
            let j = jobs(t);
            let (w, _, px) = decode(&j[0]);
            let (r, g, b) = px[100 * w + 150].rgb();
            assert!(
                r.get() > 150 && g.get() < 80 && b.get() < 80,
                "{phase:?}: the line's red, ({}, {}, {})",
                r.get(),
                g.get(),
                b.get()
            );
        });
    }
}

/// **A hard copy prints the surface displayed**: under `S(P1)` the
/// second's line, and the first's not.
#[test]
fn a_hard_copy_prints_the_surface_displayed() {
    let body = b"\x1bPpP[100,50]V[200,50]P(P1)P[100,100]V[200,100]S(P1)S(H)S(P0)S(H)\x1b\\";
    run(1 << 20, body, |t| {
        let j = jobs(t);
        assert_eq!(j.len(), 2);
        let (w, _, px) = decode(&j[0]);
        assert_eq!(ink_box(w, &px), Some((100, 100, 201, 101)), "the second");
        let (w, _, px) = decode(&j[1]);
        assert_eq!(ink_box(w, &px), Some((100, 50, 201, 51)), "the first");
    });
}
