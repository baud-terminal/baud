//! **The background a Sixel's `P2` of 0 or 2 asks for.** `[[source:
//! vt3xx-sixel]]`: 0 or 2, "Pixel positions specified as 0 are set to the
//! current background color", and "The VT300 uses Ph and Pv to erase the
//! background when P2 is set to 0 or 2"; with no raster extent, `[[source:
//! dec-std-070]]` 070-9 §10 starts a Sixel with the extent from the text
//! position to the right and bottom margins, and a VT340 clears "from the
//! cursor to the bottom right corner" (`[[source: vt340test-p2effect-sh]]`).
//! Decided by Jev 2026-10-12, EXTENT_ELSE_MARGINS, 0.92 and 0.94.
//!
//! The core sets each such position to `Pixel::FILL` and marks the image
//! `ImageGround::Filled` with how far its data reached; what colour a
//! filled position draws in is the renderer's, at each frame. Held here:
//! the region with a raster extent, to the margins without one, at a cursor
//! past a margin, in display mode, and with no cell size; the cursor's
//! advance, which the fill does not move; text and erases over the fill;
//! and the same image at every read size. The programs are vt340test's
//! p2effect stripes: a 20 by 24 striped square at 1:1, register 1 red.

use baud_core::*;
use baud_domain::{ImageBudget, ImageGround, ImagePx, Pixel, SixelClaim};

const WS: WinSize = WinSize {
    ws_row: 12,
    ws_col: 40,
};

/// The cell every test with a cell size hands the core, in pixels.
const CELL: (u16, u16) = (10, 20);

/// `P2` 0 with a 60 by 60 raster extent, the data a 20 by 24 striped
/// square: `i` sets rows 1, 3 and 5 of a band, `A` row 1.
const OPAQUE_RA: &[u8] = b"\x1bP9;0;0q\"1;1;60;60#1;2;100;0;0#1!20i-!20i-!20i-!20A\x1b\\";
/// The same with `P2` 1.
const CLEAR_RA: &[u8] = b"\x1bP9;1;0q\"1;1;60;60#1;2;100;0;0#1!20i-!20i-!20i-!20A\x1b\\";
/// `P2` 0 and no raster command.
const OPAQUE: &[u8] = b"\x1bP9;0;0q#1;2;100;0;0#1!20i-!20i-!20i-!20A\x1b\\";

/// The one placed image: its width, height, ground and pixels.
type Placed = (u16, u16, ImageGround, Vec<Pixel>);

/// Feeds `bytes` in reads of `chunk` into a fresh terminal, with the cell
/// size set when `cell` says so, and hands it over.
fn with_fed<R>(bytes: &[u8], chunk: usize, cell: bool, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 20));
    let mut t = Term::new(owned.storage(), &WS);
    if cell {
        t.set_cell_size(ImagePx::of(CELL.0), ImagePx::of(CELL.1));
    }
    // DA lists Sixel, so `?80` is recorded.
    t.set_sixel_claim(SixelClaim::Yes);
    for part in bytes.chunks(chunk.max(1)) {
        t.write_at(BytePos::ZERO, part);
        t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
    }
    f(&mut t)
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn placed(t: &Term<'_>) -> Option<Placed> {
    let imgs = t.images();
    let p = imgs.placements().next().map(|(_, p)| *p)?;
    let v = imgs.image(p.image)?;
    Some((v.width.raw(), v.height.raw(), v.ground, v.pixels.to_vec()))
}

fn red() -> Pixel {
    use baud_domain::Chan;
    Pixel::opaque(Chan::from_u8(255), Chan::from_u8(0), Chan::from_u8(0))
}

/// The pixel at `(x, y)` of an image `w` wide.
fn at(px: &[Pixel], w: u16, x: usize, y: usize) -> Pixel {
    px[y * usize::from(w) + x]
}

fn data(w: u16, h: u16) -> ImageGround {
    ImageGround::Filled {
        data_w: ImagePx::of(w),
        data_h: ImagePx::of(h),
    }
}

/// **With a raster extent the fill is `Ph` by `Pv`**, larger than the data
/// here, and every position the data left at zero in it is the background;
/// the data's own red pixels are kept.
#[test]
fn a_raster_extent_is_filled_where_the_data_left_zero() {
    let (w, h, ground, px) = with_fed(OPAQUE_RA, OPAQUE_RA.len(), true, |t| placed(t)).unwrap();
    assert_eq!((w, h), (60, 60), "the image is the raster extent");
    assert_eq!(ground, data(20, 24), "the data reached 20 by 24");
    assert_eq!(at(&px, w, 0, 0), Pixel::FILL, "row 0 of `i` is unset");
    assert_eq!(at(&px, w, 0, 1), red(), "row 1 of `i` is set");
    assert_eq!(at(&px, w, 19, 19), red(), "row 1 of the fourth band, `A`");
    assert_eq!(
        at(&px, w, 30, 30),
        Pixel::FILL,
        "past the data, inside Ph x Pv"
    );
    assert_eq!(at(&px, w, 59, 59), Pixel::FILL, "the extent's last pixel");
    assert!(
        !px.contains(&Pixel::CLEAR),
        "nothing in the extent stays clear"
    );
    with_fed(OPAQUE_RA, OPAQUE_RA.len(), true, |t| {
        assert_eq!(t.images().sixels_filled, 1, "counted");
    });
}

/// **`P2` 1 keeps every zero position clear**: the image is the data's
/// reach, unfilled and unmarked. The control for the test above.
#[test]
fn p2_1_keeps_the_zero_positions_clear() {
    let (w, h, ground, px) = with_fed(CLEAR_RA, CLEAR_RA.len(), true, |t| placed(t)).unwrap();
    assert_eq!((w, h), (20, 24));
    assert_eq!(ground, ImageGround::Clear);
    assert_eq!(at(&px, w, 0, 0), Pixel::CLEAR);
    assert!(!px.contains(&Pixel::FILL));
    with_fed(CLEAR_RA, CLEAR_RA.len(), true, |t| {
        assert_eq!(t.images().sixels_filled, 0, "not counted");
    });
}

/// **Without a raster command the fill reaches the margins in force**: from
/// the image's corner, the cursor at row 3 column 5, to the screen's right
/// edge and bottom, then to `DECSTBM`'s bottom margin, then to `DECSLRM`'s
/// right margin.
#[test]
fn without_a_raster_the_fill_reaches_the_margins() {
    let dims = |pre: &[u8]| {
        let prog = [pre, b"\x1b[3;5H", OPAQUE].concat();
        let (w, h, ground, _) = with_fed(&prog, prog.len(), true, |t| placed(t)).unwrap();
        assert_eq!(ground, data(20, 24));
        (w, h)
    };
    assert_eq!(
        dims(b""),
        ((40 - 4) * 10, (12 - 2) * 20),
        "the screen's edges"
    );
    assert_eq!(dims(b"\x1b[1;8r"), (360, (8 - 2) * 20), "DECSTBM's bottom");
    assert_eq!(
        dims(b"\x1b[?69h\x1b[1;20s"),
        ((20 - 4) * 10, 200),
        "DECSLRM's right margin"
    );
}

/// **A cursor below the bottom margin fills to the screen's bottom**, and
/// one right of the right margin to the screen's right edge: the margin is
/// behind it, and no text names a negative extent.
#[test]
fn a_cursor_past_a_margin_fills_to_the_screen_edge() {
    let prog = [b"\x1b[1;4r\x1b[6;5H".as_slice(), OPAQUE].concat();
    let (w, h, _, _) = with_fed(&prog, prog.len(), true, |t| placed(t)).unwrap();
    assert_eq!((w, h), (360, (12 - 5) * 20));
    let prog = [b"\x1b[?69h\x1b[1;20s\x1b[?6l\x1b[3;30H".as_slice(), OPAQUE].concat();
    let (w, _, _, _) = with_fed(&prog, prog.len(), true, |t| placed(t)).unwrap();
    assert_eq!(w, (40 - 29) * 10);
}

/// **Display mode (`?80` set) stops the fill at the screen's edges**, as it
/// stops the data: the image starts at the upper-left cell, and a raster
/// extent past the screen is cut to it.
#[test]
fn display_mode_stops_the_fill_at_the_screen_edges() {
    let body = b"\x1b[?80h\x1bP9;0;0q\"1;1;1000;1000#1;2;100;0;0#1!20~\x1b\\";
    let (w, h, _, _) = with_fed(body, body.len(), true, |t| placed(t)).unwrap();
    assert_eq!((w, h), (400, 240));
}

/// **With no cell size the fill is the data's reach**: no margin can be
/// said in pixels, so the image's own rectangle is filled and no more.
#[test]
fn with_no_cell_size_the_fill_is_the_datas_reach() {
    let (w, h, ground, px) = with_fed(OPAQUE, OPAQUE.len(), false, |t| placed(t)).unwrap();
    assert_eq!((w, h), (20, 24));
    assert_eq!(ground, data(20, 24));
    assert_eq!(at(&px, w, 0, 0), Pixel::FILL);
}

/// **The cursor goes below the data, not below the fill**: 070-9 §11.3
/// moves the text by "the vertical motion commands executed in the Sixel
/// printing" (PDF p. 928). 24 rows of data at 20 a cell are two rows; the
/// 100-row raster extent would be five.
#[test]
fn the_fill_does_not_move_the_cursor_further() {
    let prog = b"\x1b[2;1H\x1bP9;0;0q\"1;1;60;100#1;2;100;0;0#1!20i-!20i-!20i-!20A\x1b\\";
    let row = with_fed(prog, prog.len(), true, |t| t.cursor().0.index());
    assert_eq!(row, 1 + 2);
}

/// **Text on the fill alone clears the fill in its cells and keeps the
/// image**, as a VT340 writes text into its one bitmap and as text clears a
/// ReGIS page; the cell beside it keeps the fill. The image's data is rows
/// 2 and 3 (24 pixels from row 2 at 20 a cell), so the prompt the cursor
/// writes on row 4, inside the fill, keeps it.
#[test]
fn text_on_the_fill_clears_its_cells_and_keeps_the_image() {
    let prog = [b"\x1b[3;5H".as_slice(), OPAQUE].concat();
    with_fed(&prog, prog.len(), true, |t| {
        assert_eq!(t.cursor(), (Row::from_index(4), Col::at_index(4)));
        feed(t, b"$ ");
        let (w, _, _, px) = placed(t).expect("the image is kept");
        let (cw, ch) = (usize::from(CELL.0), usize::from(CELL.1));
        for y in 2 * ch..3 * ch {
            for x in 0..2 * cw {
                assert_eq!(at(&px, w, x, y), Pixel::CLEAR, "under the text ({x}, {y})");
            }
            assert_eq!(at(&px, w, 2 * cw, y), Pixel::FILL, "the next cell ({y})");
        }
        assert_eq!(at(&px, w, 0, 1), red(), "the data is untouched");
    });
}

/// **Text on the data frees the image**, as text over any Sixel does: a
/// previewer replaces one preview with the next by writing over it. The
/// control for the test above.
#[test]
fn text_on_the_data_frees_the_image() {
    let prog = [b"\x1b[3;5H".as_slice(), OPAQUE, b"\x1b[3;5HX"].concat();
    with_fed(&prog, prog.len(), true, |t| {
        assert_eq!(t.images().placements().count(), 0);
    });
}

/// **An erase clears the fill in the cells it erases**, as it clears a
/// Sixel's own pixels there (the record, "Erasing clears what a Sixel or
/// ReGIS drew in the erased cells"), and keeps it elsewhere.
#[test]
fn an_erase_clears_the_fill_in_the_cells_it_erases() {
    let prog = [b"\x1b[3;5H".as_slice(), OPAQUE, b"\x1b[6;9H\x1b[K"].concat();
    with_fed(&prog, prog.len(), true, |t| {
        let (w, _, _, px) = placed(t).expect("kept");
        let (cw, ch) = (usize::from(CELL.0), usize::from(CELL.1));
        // Screen row 5 is image row 3; column 8 is image column 4.
        assert_eq!(at(&px, w, 4 * cw, 3 * ch), Pixel::CLEAR, "erased");
        assert_eq!(at(&px, w, 4 * cw - 1, 3 * ch), Pixel::FILL, "left of it");
        assert_eq!(at(&px, w, 0, 4 * ch), Pixel::FILL, "the row below");
    });
}

/// **The same image at every read size**: the raster extent and the fill
/// are settled at the terminator from what the walk carried across reads.
#[test]
fn the_fill_is_the_same_at_every_read_size() {
    for body in [OPAQUE_RA, OPAQUE, CLEAR_RA] {
        let prog = [b"\x1b[3;5H".as_slice(), body].concat();
        let whole = with_fed(&prog, prog.len(), true, |t| placed(t));
        assert!(whole.is_some(), "the control places an image");
        for chunk in 1..prog.len() {
            let got = with_fed(&prog, chunk, true, |t| placed(t));
            assert!(got == whole, "read size {chunk}");
        }
    }
}

/// Eleven bands of the 20-wide stripe at 1:1, 66 rows, at the screen's row
/// 10 of 12: two rows of 20 pixels are left, so the image scrolls the
/// screen. `ra` is the raster command, or none.
fn tall(ra: &str) -> Vec<u8> {
    let body = format!("{ra}#1;2;100;0;0#1{}!20A", "!20i-".repeat(10));
    [
        b"\x1b[11;1H\x1bP9;0;0q".as_slice(),
        body.as_bytes(),
        b"\x1b\\",
    ]
    .concat()
}

/// **The fill goes on as the image scrolls**: a VT340 fills the lines a
/// Sixel that runs past the bottom margin scrolls in, from the band that
/// overruns the rows left to the data's last row rounded up to a whole
/// cell, across the fill's width, and counts the rows left as filled
/// "even if it wasn't all filled to start with", so with a raster extent
/// shorter than the rows left the rows between stay as they were
/// (Windows Terminal's `_fillImageBackgroundWhenScrolled()`, written
/// against the hardware, `[[source: wt-sixel-parser]]`). Here the rows
/// left are 40 pixels, the band 6: the band at 36 overruns, and the data's
/// 66 rows round up to 80.
#[test]
fn the_fill_goes_on_as_the_image_scrolls() {
    let ra = tall("\"1;1;30;12");
    let (w, h, ground, px) = with_fed(&ra, ra.len(), true, |t| placed(t)).unwrap();
    assert_eq!((w, h), (30, 80), "Ph wide, to the cell the data ends in");
    assert_eq!(ground, data(20, 66), "the data reached 20 by 66");
    assert_eq!(at(&px, w, 25, 11), Pixel::FILL, "inside Ph x Pv");
    assert_eq!(at(&px, w, 25, 12), Pixel::CLEAR, "below Pv, above the band");
    assert_eq!(at(&px, w, 25, 35), Pixel::CLEAR, "the row above the band");
    assert_eq!(at(&px, w, 25, 36), Pixel::FILL, "the band that overran");
    assert_eq!(
        at(&px, w, 0, 36),
        Pixel::FILL,
        "a zero bit of the data there"
    );
    assert_eq!(at(&px, w, 0, 37), red(), "its set bit");
    assert_eq!(at(&px, w, 29, 79), Pixel::FILL, "the last row, rounded up");
    // Without a raster the fill is the margins', and the scroll goes on
    // from the same band across them.
    let bare = tall("");
    let (w, h, _, px) = with_fed(&bare, bare.len(), true, |t| placed(t)).unwrap();
    assert_eq!((w, h), (400, 80), "the screen's width, to the cell");
    assert!(!px.contains(&Pixel::CLEAR), "nothing stays clear");
    // The controls: with no cell size, nothing measures the rows left and
    // the fill is the data's reach; and the same image with room for it
    // fills only Pv.
    let (w, h, _, _) = with_fed(&ra, ra.len(), false, |t| placed(t)).unwrap();
    assert_eq!((w, h), (30, 66), "no cell size, no scroll fill");
    let roomy = [b"\x1b[1;1H".as_slice(), &ra[b"\x1b[11;1H".len()..]].concat();
    let (w, h, _, px) = with_fed(&roomy, roomy.len(), true, |t| placed(t)).unwrap();
    assert_eq!((w, h), (30, 66), "no scroll, no scroll fill");
    assert_eq!(at(&px, w, 25, 40), Pixel::CLEAR, "past Pv, unfilled");
    // Display mode never scrolls: data past a bottom margin stays on the
    // screen, and no scroll fill follows it.
    let shown = [b"\x1b[?80h\x1b[1;3r".as_slice(), &ra[b"\x1b[11;1H".len()..]].concat();
    let (w, h, _, px) = with_fed(&shown, shown.len(), true, |t| placed(t)).unwrap();
    assert_eq!((w, h), (30, 66), "display mode, no scroll fill");
    assert_eq!(
        at(&px, w, 25, 60),
        Pixel::CLEAR,
        "below the margin, unfilled"
    );
}
