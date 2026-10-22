//! A theme file's cursor and selection colours, on the CPU path, read from
//! the files themselves: the pixel oracle holds the instance arm and the
//! cell shader to each other, so a rule both lost would pass it.
//!
//! **The cursor's colours are drawn**: the block fills with a theme's
//! cursor colour and its glyph takes the theme's cursor text colour; a
//! program's `OSC 12` wins over the fill while set and leaves the text
//! colour as it is, as kitty, alacritty, ghostty and foot leave theirs
//! (Jev, 0.94 and 0.96). **A cell's own colour is a colour too**:
//! alacritty's `CellForeground`/`CellBackground` "references the affected
//! cell" (`[[source: alacritty-0c405d5-man-5]]` 327-334, 398-405), and
//! ghostty's `cell-foreground`/`cell-background` match "the cell foreground
//! color" and "the cell background color" (`[[source:
//! ghostty-config-8f0dd37]]` 707-716, 845-859, 905-911).

mod support;

use baud_core::{Owned, Term};
use baud_domain::themefile::{Scheme, alacritty, ghostty, kitty};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Argb, Blend, Damage, Highlight, PxSize, Region, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 6,
    ws_row: 2,
};
const RED: Argb = Argb(0xFF_CD_00_00);

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The dark half with a theme file's scheme laid over it.
fn themed(scheme: Result<Scheme, impl core::fmt::Debug>) -> Theme {
    let mut t = Theme::dark();
    t.take_scheme(&scheme.expect("the file reads"));
    t
}

/// A red full block, so its middle pixel is the colour its glyph is drawn
/// in, at column 0, painted under `theme` after `bytes`: the page of the
/// block's cell and of the blank beside it, and the block's middle pixel.
fn painted(theme: Theme, bytes: &[u8], select: bool) -> (Argb, Argb, u32) {
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
    feed(&mut t, b"\x1b[38;2;205;0;0m\xe2\x96\x88\x1b[0m ");
    feed(&mut t, bytes);
    if select {
        r.set_highlight(Some(Highlight {
            from: (Row::ZERO, Col::at_index(0)),
            to: (Row::ZERO, Col::at_index(1)),
            region: Region::Stream,
        }));
    }
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let middle = px[usize::try_from((ch / 2) * w.value() + cw / 2).unwrap()];
    (r.background()[0], r.background()[1], middle)
}

fn opaque(c: Argb) -> Argb {
    Argb(c.0 | 0xFF00_0000)
}

/// **A theme file's cursor colours are drawn**: the block fills with the
/// cursor colour and its glyph is the cursor text colour; a program's
/// `OSC 12` takes the fill and leaves the text, and `OSC 112` gives the
/// theme's fill back. Without a theme colour, the cell's colours swap.
#[test]
fn a_theme_files_cursor_colours_are_drawn_and_osc_12_takes_the_fill() {
    let theme = || {
        themed(kitty(
            b"cursor #ff8800\ncursor_text_color #102030\n",
            |_, _, _| {},
        ))
    };
    let on_block = b"\x1b[2 q\x1b[1;1H";
    let (block, _, glyph) = painted(theme(), on_block, false);
    assert_eq!((block, glyph), (Argb(0xFF_FF_88_00), 0xFF_10_20_30));
    let mut program = on_block.to_vec();
    program.extend_from_slice(b"\x1b]12;#00ff00\x07");
    let (block, _, glyph) = painted(theme(), &program, false);
    assert_eq!(
        (block, glyph),
        (Argb(0xFF_00_FF_00), 0xFF_10_20_30),
        "a program's fill, the theme's text"
    );
    program.extend_from_slice(b"\x1b]112\x07");
    let (block, _, _) = painted(theme(), &program, false);
    assert_eq!(block, Argb(0xFF_FF_88_00), "the reset is the theme's");
    let dark = Theme::dark();
    let (block, _, glyph) = painted(dark.clone(), on_block, false);
    assert_eq!((block, glyph), (RED, opaque(dark.bg).0), "the swap");
}

/// **A cell's own colour names the cursor's**: alacritty's `cursor =
/// "CellBackground"` fills the block with the cell's page, and `text =
/// "CellForeground"` draws the glyph in the cell's own text.
#[test]
fn a_cell_relative_cursor_takes_the_cells_own_colours() {
    let dark = Theme::dark();
    let theme = themed(alacritty(
        b"[colors.cursor]\ncursor = \"CellBackground\"\ntext = \"CellForeground\"\n",
        |_, _, _| {},
    ));
    let (block, _, glyph) = painted(theme, b"\x1b[2 q\x1b[1;1H", false);
    assert_eq!(opaque(block), opaque(dark.bg), "the cell's page fills it");
    assert_eq!(glyph, RED.0, "the cell's own text");
}

/// **alacritty's `text = "CellBackground"` over a page draws each selected
/// cell's text in its own page**; left out, the cell keeps its own text.
#[test]
fn alacrittys_cell_background_selection_text_is_the_cells_page() {
    let dark = Theme::dark();
    let theme = themed(alacritty(
        b"[colors.selection]\nbackground = \"#2f5f9f\"\ntext = \"CellBackground\"\n",
        |_, _, _| {},
    ));
    let (block, blank, glyph) = painted(theme, b"\x1b[?25l", true);
    assert_eq!((block, blank), (Argb(0xFF_2F_5F_9F), Argb(0xFF_2F_5F_9F)));
    assert_eq!(glyph, opaque(dark.bg).0, "the cell's page");
}

/// **ghostty's `selection-invert-fg-bg`, as its own two keys**:
/// `selection-foreground = cell-background` and `selection-background =
/// cell-foreground` are each cell's colours exchanged, reverse video,
/// where the file was refused whole.
#[test]
fn ghosttys_cell_colours_select_in_each_cells_colours_exchanged() {
    let dark = Theme::dark();
    let theme = themed(ghostty(
        b"selection-foreground = cell-background\nselection-background = cell-foreground\n",
        |_, _| {},
    ));
    let (block, blank, glyph) = painted(theme, b"\x1b[?25l", true);
    assert_eq!(block, RED, "the cell's text is its page");
    assert_eq!(blank, opaque(dark.fg));
    assert_eq!(glyph, opaque(dark.bg).0, "the cell's page is its text");
}
