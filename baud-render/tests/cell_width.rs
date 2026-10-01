//! Whether a grid cell's character spans two cells is read from the grid:
//! the next cell holds `Content::WIDE_TAIL`. `CellFlags::is_wide` is the
//! cell table's advance, not a grid fact -- on a grid cell those two bits
//! are `PROTECTED` and `GRAPHICS` -- and reading it there made every DEC
//! line-drawing cell "wide", so the CPU and instance paths stepped two
//! cells for one and drew every other one.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, Kind, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 16,
};

/// The columns of row 0 that draw ink, after `bytes`, on the instance path
/// the CPU compositor paints from.
fn inked_columns(bytes: &[u8]) -> Vec<usize> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    let cw = i32::try_from(r.metrics().cell_w.value()).unwrap();
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let mut cols: Vec<usize> = r
        .instances()
        .filter(|i| i.kind == Kind::Mask && i32::from(i.y.0) < ch)
        .map(|i| usize::try_from((i32::from(i.x.0) + i32::from(i.w.0) / 2) / cw).unwrap())
        .collect();
    cols.sort_unstable();
    cols.dedup();
    cols
}

/// **Every DEC line-drawing cell is drawn.** Six `q`s under `ESC ( 0` are
/// six horizontal lines, each its own cell; the control is the same six
/// box characters sent as themselves, with no `GRAPHICS` flag.
#[test]
fn every_dec_line_drawing_cell_is_drawn() {
    let sent =
        inked_columns("\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\x1b[2;16H".as_bytes());
    assert_eq!(sent, vec![0, 1, 2, 3, 4, 5], "the control");
    let dec = inked_columns(b"\x1b(0qqqqqq\x1b(B\x1b[2;16H");
    assert_eq!(dec, vec![0, 1, 2, 3, 4, 5]);
}

/// **And the ones that are text.** `f`, `g` and `~` under `ESC ( 0` are °, ±
/// and ·, shaped in a run with the letters around them, not drawn by baud;
/// the run steps through them a cell at a time too.
#[test]
fn every_dec_graphics_text_cell_is_drawn() {
    let dec = inked_columns(b"a\x1b(0fgf\x1b(Bb\x1b[2;16H");
    assert_eq!(dec, vec![0, 1, 2, 3, 4]);
}

/// **A protected cell is not narrowed, and a wide one is still two.** `SGR`
/// does not touch the flags, `DECSCA` does: a protected `ab` is two cells,
/// and a protected `水` is one character in two.
#[test]
fn protection_does_not_change_a_width() {
    assert_eq!(inked_columns(b"\x1b[1\"qab\x1b[2;16H"), vec![0, 1]);
    let wide = inked_columns("\u{6c34}x\x1b[2;16H".as_bytes());
    assert_eq!(
        wide.last(),
        Some(&2),
        "x after a wide character is in column 2"
    );
}
