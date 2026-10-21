//! **The status line reaches the pixels**: a row below the main display,
//! the main display's rows kept (`Term::status_rows`). The indicator is
//! drawn in reverse video, "It appears in reverse video (negative image) on
//! the line below the last line of the user window" (`[[source:
//! vt420-rm]]` p. 219), and its background is "the reverse of the Main
//! Display background" (`[[source: dec-std-070]]` 070-14 p. 14-5); the
//! host-writable line "has the same background as the main display" (the
//! same page), and the cursor is drawn there while DECSASD selects it.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Argb, Damage, Instance, Kind, Renderer, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 12,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn rgb(c: Argb) -> u32 {
    c.0 & 0x00FF_FFFF
}

/// One full CPU frame of `bytes`, handed over with the canvas.
fn frame(bytes: &[u8], f: impl FnOnce(&Renderer<'_, '_>, &support::Canvas, &Term<'_>)) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = support::renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    r.fit(&t);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    f(&r, &canvas, &t);
}

/// The pixels of cell `(row, col)`'s band, in the canvas.
fn cell_pixels(r: &Renderer<'_, '_>, c: &support::Canvas, row: Row, col: Col) -> Vec<u32> {
    let m = r.metrics();
    let side = |p: baud_font::Px| usize::try_from(p.value()).unwrap();
    let (cw, ch) = (side(m.cell_w), side(m.cell_h));
    let (row, col) = (row.index(), col.index());
    let mut out = Vec::new();
    for y in row * ch..(row + 1) * ch {
        for x in col * cw..(col + 1) * cw {
            out.push(c.pixels[y * c.width + x] & 0x00FF_FFFF);
        }
    }
    out
}

/// The glyphs drawn on frame row `row`.
fn glyphs_on(r: &Renderer<'_, '_>, row: Row) -> Vec<Instance> {
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let row = i32::try_from(row.index()).unwrap();
    r.instances()
        .filter(|i| i.kind == Kind::Mask)
        .filter(|i| {
            let y = i32::from(i.y.0);
            y >= row * ch && y < (row + 1) * ch
        })
        .copied()
        .collect()
}

/// **The frame grows a row and the main display keeps its rows**: no line
/// at power-on, one row more with either type, and back with none.
#[test]
fn the_frame_grows_a_row_for_the_line_and_gives_it_back() {
    let ch = |r: &Renderer<'_, '_>| r.metrics().cell_h.value();
    frame(b"", |r, _, t| {
        assert_eq!(r.pixel_size().1.value(), 2 * ch(r));
        assert_eq!(t.geometry().1, Rows::of_count(2));
    });
    for open in [b"\x1b[1$~".as_slice(), b"\x1b[2$~"] {
        frame(open, |r, _, t| {
            assert_eq!(r.pixel_size().1.value(), 3 * ch(r));
            assert_eq!(r.grid(), (Cols::of_index(12), Rows::of_count(3)));
            assert_eq!(r.main_rows(), Rows::of_count(2));
            assert_eq!(t.geometry().1, Rows::of_count(2));
        });
    }
    frame(b"\x1b[1$~\x1b[0$~", |r, _, _| {
        assert_eq!(r.pixel_size().1.value(), 2 * ch(r));
    });
}

/// **The indicator in reverse video**: every cell of its row has the
/// theme's foreground for its background, where the main display's blank
/// cells have the background, and its text is drawn on its row.
#[test]
fn the_indicator_is_drawn_in_reverse_video_below_the_main_display() {
    let theme = Theme::dark();
    frame(b"\x1b[1$~\x1b[?25l", |r, c, _| {
        let cols = 12;
        for x in 0..cols {
            assert_eq!(rgb(r.background()[cols + x]), rgb(theme.bg), "main {x}");
            assert_eq!(rgb(r.background()[2 * cols + x]), rgb(theme.fg), "line {x}");
        }
        // "(01,001)" from column 4: eight glyphs, all on the line's row.
        assert_eq!(glyphs_on(r, Row::from_index(2)).len(), 8);
        assert_eq!(
            glyphs_on(r, Row::ZERO).len() + glyphs_on(r, Row::from_index(1)).len(),
            0
        );
        // The pixels: a cell left of the text is the foreground whole, and
        // one under a digit holds ink.
        assert!(
            cell_pixels(r, c, Row::from_index(2), Col::at_index(0))
                .iter()
                .all(|&p| p == rgb(theme.fg))
        );
        assert!(
            cell_pixels(r, c, Row::from_index(2), Col::at_index(5))
                .iter()
                .any(|&p| p != rgb(theme.fg))
        );
        assert!(
            cell_pixels(r, c, Row::from_index(1), Col::at_index(5))
                .iter()
                .all(|&p| p == rgb(theme.bg))
        );
    });
}

/// **The host-writable line has the main display's background**, its
/// text on its own row, and the cursor drawn there while DECSASD selects
/// it: a block cursor swaps the cell after the text, and the main
/// display's cursor cell is not swapped.
#[test]
fn the_host_writable_line_draws_its_text_and_its_cursor() {
    let theme = Theme::dark();
    frame(b"\x1b[2$~\x1b[1$}AB", |r, c, _| {
        let cols = 12;
        assert_eq!(glyphs_on(r, Row::from_index(2)).len(), 2);
        assert_eq!(glyphs_on(r, Row::ZERO).len(), 0);
        assert_eq!(rgb(r.background()[2 * cols + 5]), rgb(theme.bg));
        assert_eq!(
            rgb(r.background()[2 * cols + 2]),
            rgb(theme.fg),
            "the cursor"
        );
        assert_eq!(
            rgb(r.background()[0]),
            rgb(theme.bg),
            "not on the main display"
        );
        assert!(
            cell_pixels(r, c, Row::from_index(2), Col::at_index(0))
                .iter()
                .any(|&p| p != rgb(theme.bg))
        );
    });
    // Back on the main display, the cursor is drawn there.
    frame(b"\x1b[2$~\x1b[1$}AB\x1b[0$}", |r, _, _| {
        let cols = 12;
        assert_eq!(rgb(r.background()[2 * cols + 2]), rgb(theme.bg));
        assert_eq!(rgb(r.background()[0]), rgb(theme.fg), "the main cursor");
    });
}

/// **A frame a row taller than its target paints what fits**: a window
/// that has not grown yet for the line, the frame the renderer draws for
/// the main display and the line, and the target the old size. The main
/// display's rows paint, the line's row falls past the target's bottom,
/// and nothing panics.
#[test]
fn a_target_a_row_short_takes_the_main_display_and_no_more() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = support::renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"main");
    r.fit(&t);
    let mut short = support::Canvas::for_renderer(&r);
    feed(&mut t, b"\x1b[1$~");
    r.fit(&t);
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    assert_eq!(short.height, 2 * ch, "the target is the main display's");
    support::paint(&mut r, &mut t, &mut short, Damage::Full);
    assert!(
        short.pixels.iter().any(|&p| p & 0x00FF_FFFF != 0),
        "the main display's rows are painted"
    );
}
