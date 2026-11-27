//! A theme file's scheme laid over a built-in half (note Y, step 7): the
//! sixteen and the page and text it sets replace the half's; what it leaves
//! unset stays the half's and is counted; 16-255 are never written; its
//! selection colours become the half's.

use baud_domain::themefile::{CellPaint, CursorText, Format, Scheme, SelectionText};
use baud_domain::{Chan, Color};
use baud_render::{Argb, Theme};

fn rgb(v: u32) -> Color {
    let [_, r, g, b] = v.to_be_bytes();
    Color::Rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

fn argb(v: u32) -> Argb {
    let [_, r, g, b] = v.to_be_bytes();
    Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

fn empty() -> Scheme {
    Scheme {
        ansi: [None; 16],
        page: None,
        text: None,
        extended: [None; 240],
        cursor: None,
        cursor_text: None,
        selection: None,
        selection_text: None,
        cursor_text_under: CursorText::UnderEveryCursor,
        unset_selection_text: SelectionText::Reverse,
        format: Format::Ghostty,
        declared: None,
    }
}

#[test]
fn a_whole_scheme_replaces_the_sixteen_the_page_and_the_text() {
    let mut s = empty();
    for (i, e) in s.ansi.iter_mut().enumerate() {
        *e = Some(rgb(0x010203 * (1 + u32::try_from(i).unwrap())));
    }
    s.page = Some(rgb(0x102030));
    s.text = Some(rgb(0xe0d0c0));
    let mut t = Theme::dark();
    let alpha = t.bg_alpha;
    let unset = t.take_scheme(&s);
    assert_eq!(unset.get_raw(), 0);
    for (i, entry) in t.palette.iter().enumerate().take(16) {
        assert_eq!(
            *entry,
            argb(0x010203 * (1 + u32::try_from(i).unwrap())),
            "entry {i}"
        );
    }
    assert_eq!((t.bg, t.fg), (argb(0x102030), argb(0xe0d0c0)));
    assert_eq!(
        t.bg_alpha, alpha,
        "the dim is the window's, not the scheme's"
    );
}

/// A partial file keeps the half's own entries and says how many.
#[test]
fn what_a_scheme_leaves_unset_stays_the_halfs() {
    let mut s = empty();
    s.ansi[1] = Some(rgb(0xa00001));
    s.page = Some(rgb(0xffffff));
    let mut t = Theme::light();
    let built = Theme::light();
    let unset = t.take_scheme(&s);
    assert_eq!(unset.get_raw(), 16, "fifteen entries and the text");
    assert_eq!(t.palette[1], argb(0xa00001));
    assert_eq!(t.palette[2], built.palette[2]);
    assert_eq!((t.bg, t.fg), (argb(0xffffff), built.fg));
}

/// 16-255 stay xterm's whatever the file says: legacy programs address them
/// by number and expect the cube.
#[test]
fn entries_past_fifteen_are_never_written() {
    let mut s = empty();
    s.extended = [Some(rgb(0x123456)); 240];
    let mut t = Theme::dark();
    let built = Theme::dark();
    t.take_scheme(&s);
    assert_eq!(t.palette[16..], built.palette[16..]);
}

#[test]
fn the_built_in_halves_read_as_their_names() {
    use baud_domain::ColorScheme;
    assert_eq!(Theme::dark().polarity(), Some(ColorScheme::Dark));
    assert_eq!(Theme::light().polarity(), Some(ColorScheme::Light));
}

/// **A theme file's selection colours are the half's selection**, each
/// one the file names, uncounted when unset: a half with none is reverse
/// video, which is no colour of baud's to keep.
#[test]
fn a_schemes_selection_colours_are_taken_and_unset_ones_are_not_counted() {
    let mut s = empty();
    s.selection = Some(CellPaint::Colour(rgb(0x334e5e)));
    let mut t = Theme::dark();
    let unset = t.take_scheme(&s);
    assert_eq!(unset.get_raw(), 18, "the sixteen, the page and the text");
    assert_eq!(
        (t.selection, t.selection_text),
        (Some(CellPaint::Colour(argb(0x334e5e))), None)
    );
    s.selection_text = Some(CellPaint::Colour(rgb(0xffffff)));
    t.take_scheme(&s);
    assert_eq!(t.selection_text, Some(CellPaint::Colour(argb(0xffffff))));
    assert_eq!(
        (Theme::dark().selection, Theme::light().selection_text),
        (None, None),
        "the built-in halves are reverse video"
    );
}

/// **A theme file's cursor colours are the half's cursor**, a colour or
/// the cell's own as the file names it, uncounted when unset; the built-in
/// halves swap the cell's colours.
#[test]
fn a_schemes_cursor_colours_are_taken_and_unset_ones_are_not_counted() {
    let mut s = empty();
    s.cursor = Some(CellPaint::Colour(rgb(0xff8800)));
    s.cursor_text = Some(CellPaint::Text);
    let mut t = Theme::dark();
    let unset = t.take_scheme(&s);
    assert_eq!(unset.get_raw(), 18, "the sixteen, the page and the text");
    assert_eq!(
        (t.cursor, t.cursor_text),
        (
            Some(CellPaint::Colour(argb(0xff8800))),
            Some(CellPaint::Text)
        )
    );
    assert_eq!(
        (Theme::dark().cursor, Theme::light().cursor_text),
        (
            Some(CellPaint::Colour(baud_render::BaudCursor::dark().fill)),
            Some(CellPaint::Colour(baud_render::BaudCursor::light().text))
        ),
        "the built-in halves draw baud's amber"
    );
}
