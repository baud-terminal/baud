//! xterm's special colours reach the rendition a cell draws with: `OSC 5`
//! sets colorBD, colorUL, colorBL, colorRV and colorIT, `OSC 6` turns each
//! mode on, and a cell with the attribute is drawn in the colour instead of
//! the attribute -- "displayed in color or as bold characters" (xterm(1),
//! colorBDMode). Resolved where every path resolves a rendition, so the
//! CPU, instance and cell paths draw from the same slot.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Blend, FaceRole, FaceStyle, Floor, PxSize, Renderer, Resolved, Shapes, Stack, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 20,
};

fn rgb(c: Argb) -> [u8; 3] {
    [c.r().get(), c.g().get(), c.b().get()]
}

/// The slot the first cell of the screen draws from after `bytes`, with a
/// bold face in the stack so a bold cell has a face to lose, and no
/// contrast floor, so a colour comes out as it was set.
fn first_cell(bytes: &[u8]) -> Resolved {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&data).unwrap()];
    let mut theme = Theme::dark();
    theme.floor = Floor::Off;
    let mut r = Renderer::new(
        Stack::with_roles(
            &faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        baud_domain::Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut table = vec![Resolved::default(); t.styles().len()];
    r.resolve_styles_into(&t, &mut table);
    let cell = t.row(Row::from_index(0)).expect("a row")[0];
    table[cell.style.index()]
}

const RED: [u8; 3] = [0xff, 0, 0];
const BLUE: [u8; 3] = [0, 0, 0xff];

/// **A bold cell takes colorBD and is drawn regular**, while colorBDMode is
/// on; off, it is drawn bold in the text colour.
#[test]
fn a_bold_cell_takes_colorbd_and_loses_its_face() {
    let off = first_cell(b"\x1b]5;0;red\x07\x1b[1mX");
    assert_eq!(off.shapes.face(), FaceStyle::Bold);
    assert_ne!(rgb(off.fg), RED);
    let on = first_cell(b"\x1b]5;0;red\x07\x1b]6;0;1\x07\x1b[1mX");
    assert_eq!(rgb(on.fg), RED);
    assert_eq!(on.shapes.face(), FaceStyle::Regular);
}

/// **The mode applies to what is already on the screen**: a bold cell
/// written before `OSC 6` is drawn in colorBD after it, as the manual's
/// "should be displayed in color" reads and xterm's `extract_fg` maps a
/// cell in the default colours at draw time.
#[test]
fn turning_a_mode_on_recolours_what_is_on_the_screen() {
    let on = first_cell(b"\x1b[1mX\x1b[m\x1b]5;0;red\x07\x1b]6;0;1\x07");
    assert_eq!(rgb(on.fg), RED);
}

/// **An underlined cell takes colorUL and loses its underline**, a
/// blinking one colorBL and its blink, an italic one colorIT and its face;
/// blink is taken before bold, as xterm's `MapToColorMode` orders them.
#[test]
fn each_attribute_gives_way_to_its_colour() {
    let u = first_cell(b"\x1b]5;1;red\x07\x1b]6;1;1\x07\x1b[4mX");
    assert_eq!(rgb(u.fg), RED);
    assert!(!u.shapes.has(Shapes::UNDERLINE));
    let b = first_cell(b"\x1b]5;2;blue;0;red\x07\x1b]6;2;1;0;1\x07\x1b[1;5mX");
    assert_eq!(rgb(b.fg), BLUE, "blink before bold");
    assert!(!b.shapes.blinks());
    assert_eq!(
        b.shapes.face(),
        FaceStyle::Bold,
        "bold was not the one taken"
    );
}

/// **An explicit colour keeps its own unless colorAttrMode is on**:
/// "If not, these are displayed only when no ANSI colors have been set for
/// the corresponding position" (xterm(1)); `OSC 6 ; 5` is that mode.
#[test]
fn an_explicit_colour_wins_unless_attr_mode() {
    let own = first_cell(b"\x1b]5;0;red\x07\x1b]6;0;1\x07\x1b[1;38;2;0;255;0mX");
    assert_eq!(rgb(own.fg), [0, 0xff, 0]);
    let over = first_cell(b"\x1b]5;0;red\x07\x1b]6;0;1;5;1\x07\x1b[1;38;2;0;255;0mX");
    assert_eq!(rgb(over.fg), RED);
}

/// **A reversed cell takes colorRV as its page and is not exchanged**:
/// its text stays in the text colour, as xterm draws it. `setExtendedBG`
/// gives the cell colorRV as its background (`[[source: xterm-reset]]`'s
/// file 1140-1150), and `ReverseOrHilite` is false for a reversed cell
/// outside the selection while colorRVMode is on (`[[source:
/// xterm-ptyx-h]]` 1603-1607), so `updatedXtermGC` makes no exchange
/// (`[[source: xterm-scroll]]` util.c 5546). A cell with a page of its
/// own keeps the exchange, "displayed only when no ANSI colors have been
/// set" (`[[source: xterm-man]]`, colorAttrMode), where xterm's
/// `ReverseOrHilite` drops it there too (`[[source: xterm-ptyx-h]]`
/// 1603-1607).
#[test]
fn a_reversed_cell_shows_colorrv() {
    let text = first_cell(b"X");
    let r = first_cell(b"\x1b]5;3;blue\x07\x1b]6;3;1\x07\x1b[7mX");
    assert_eq!(rgb(r.bg), BLUE, "colorRV is the page");
    assert_eq!(rgb(r.fg), rgb(text.fg), "the text keeps the text colour");
    let own = first_cell(b"\x1b]5;3;blue\x07\x1b]6;3;1\x07\x1b[7;48;2;0;255;0mX");
    assert_eq!(
        rgb(own.fg),
        [0, 0xff, 0],
        "a page of its own keeps the exchange"
    );
}

/// **An unset colorRV is the text foreground, drawn as the core answers
/// it**, colorRV's default being "XtDefaultForeground" (`[[source:
/// xterm-man]]`): a reversed cell under colorRVMode with no `OSC 5 ; 3`
/// is the text colour on a page of the text colour, as xterm draws it --
/// not ordinary reverse video's exchange.
#[test]
fn an_unset_colorrv_is_the_text_foreground() {
    let text = first_cell(b"X");
    let on = first_cell(b"\x1b]6;3;1\x07\x1b[7mX");
    assert_eq!(rgb(on.bg), rgb(text.fg), "the page is the text foreground");
    assert_eq!(rgb(on.fg), rgb(text.fg), "the text is not exchanged");
    assert_ne!(rgb(on.bg), rgb(text.bg));
}
