//! The selection in a window that is not key, on the CPU path: the pixel
//! oracle holds the instance arm and the cell shader to each other, so a
//! rule both lost would pass it.
//!
//! AppKit draws the selection of a window that is not key in its
//! unemphasized pair, `unemphasizedSelectedTextBackgroundColor` and
//! `unemphasizedSelectedTextColor`, "e.g. when the associated
//! control/view/window is not key" (`[[source: sdk27-nscolor-h]]`
//! 232-235). A reverse-video selection, the default, takes that pair while
//! the window is not key and is reverse video again once it is; a
//! selection colour of its own (`[palette] selection`, a theme file,
//! `OSC 17`/`19`) is drawn as it is (Jev's lean, note AZ).

mod support;

use baud_core::{Owned, Term};
use baud_domain::themefile::CellPaint;
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Blend, Damage, Highlight, Hilite, PxSize, Region, Renderer, Stack, Target, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 6,
    ws_row: 2,
};
const RED: Argb = Argb(0xFF_CD_00_00);
/// AppKit's darkAqua pair, resolved in sRGB (note AZ).
const DARK_PAGE: Argb = Argb(0xFF_46_46_46);
const DARK_TEXT: Argb = Argb(0xFF_FF_FF_FF);
const SET: Argb = Argb(0xFF_2F_5F_9F);

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn dark_pair() -> Hilite {
    Hilite::of(
        Some(CellPaint::Colour(DARK_PAGE)),
        Some(CellPaint::Colour(DARK_TEXT)),
    )
}

/// A red full block, so its middle pixel is the text colour, and a blank,
/// selected together under `theme` after `bytes`, drawn once for each of
/// `pairs` on one renderer: the page of each cell and the block's middle
/// pixel, per draw.
fn drawn(theme: Theme, bytes: &[u8], pairs: &[Option<Hilite>]) -> Vec<(Argb, Argb, u32)> {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(6),
        Rows::of_index(2),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[38;2;205;0;0m\xe2\x96\x88\x1b[0m ");
    feed(&mut t, bytes);
    r.set_highlight(Some(Highlight::new(
        (Row::ZERO, Col::at_index(0)),
        (Row::ZERO, Col::at_index(1)),
        Region::Stream,
    )));
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let mut out = Vec::new();
    for pair in pairs {
        r.set_inactive_selection(*pair);
        {
            let mut target = Target::new(&mut px, w, w, h).unwrap();
            // Incremental, as the app draws: the change of colours alone
            // has to repaint the selected cells.
            r.render(&mut t, &mut target, Damage::Incremental);
        }
        let middle = px[usize::try_from((ch / 2) * w.value() + cw / 2).unwrap()];
        out.push((r.background()[0], r.background()[1], middle));
    }
    out
}

/// **Reverse video takes the unemphasized pair while the window is not
/// key**, and is reverse video again once it is key: the block's page and
/// the blank's are the pair's page and the glyph its text.
#[test]
fn a_reverse_video_selection_takes_the_unemphasized_pair_while_not_key() {
    let dark = Theme::dark();
    let opaque = |c: Argb| Argb(c.0 | 0xFF00_0000);
    let got = drawn(dark.clone(), b"", &[None, Some(dark_pair()), None]);
    assert_eq!(
        got[0],
        (RED, opaque(dark.fg), opaque(dark.bg).0),
        "key: reverse video"
    );
    assert_eq!(
        got[1],
        (DARK_PAGE, DARK_PAGE, DARK_TEXT.0),
        "not key: the unemphasized pair"
    );
    assert_eq!(got[2], got[0], "key again: reverse video");
}

/// **A selection colour of its own is drawn as it is**: a theme's page
/// alone, a theme's page and text, and a program's `OSC 17` over reverse
/// video each draw the same while the window is not key.
#[test]
fn a_selection_colour_of_its_own_is_unchanged_while_not_key() {
    let page_only = Theme {
        selection: Some(CellPaint::Colour(SET)),
        ..Theme::dark()
    };
    let both = Theme {
        selection: Some(CellPaint::Colour(SET)),
        selection_text: Some(CellPaint::Colour(Argb(0xFF_FF_D7_5F))),
        ..Theme::dark()
    };
    let text_only = Theme {
        selection_text: Some(CellPaint::Colour(Argb(0xFF_FF_D7_5F))),
        ..Theme::dark()
    };
    for (what, theme, bytes) in [
        ("a theme's page alone", page_only, &b""[..]),
        ("a theme's page and text", both, b""),
        ("a theme's text alone", text_only, b""),
        ("OSC 17", Theme::dark(), b"\x1b]17;#2f5f9f\x07"),
        ("OSC 19", Theme::dark(), b"\x1b]19;#ffd75f\x07"),
    ] {
        let got = drawn(theme, bytes, &[None, Some(dark_pair())]);
        assert_eq!(got[1], got[0], "{what}: unchanged while not key");
        assert_ne!(got[1].0, DARK_PAGE, "{what}: not the pair");
    }
}

/// **`OSC 117` puts reverse video back, and with it the pair**: the
/// substitution reads the colours as resolved this frame, not as they
/// were when the window lost the keyboard.
#[test]
fn a_reset_to_reverse_video_takes_the_pair_while_not_key() {
    let got = drawn(
        Theme::dark(),
        b"\x1b]17;#2f5f9f\x07\x1b]117\x07",
        &[Some(dark_pair())],
    );
    assert_eq!(got[0], (DARK_PAGE, DARK_PAGE, DARK_TEXT.0));
}
