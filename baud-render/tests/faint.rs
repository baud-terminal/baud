//! Faint (`SGR 2`) mixes the text toward the colour it is drawn on.
//!
//! It halved the channels, which on the light page made faint text *darker*
//! than normal text -- `0x13` at 16.16:1 against 12.99:1 (note Y, defect 1;
//! decision 10). The mix is in encoded sRGB, keeping half the text colour:
//! ghostty's `faint-opacity` default of 0.5 applied as opacity, which ghostty
//! blends in the native, encoded space. A linear-light mix was note Y's plan
//! and was measured before it was built: it leaves faint default text on the
//! light page at 1.86:1 where the dark page gets 7.19:1; the encoded mix gives
//! 2.96:1 and 4.07:1.

use baud_domain::{Attrs, Chan, ColorWord, StyleWord};
use baud_render::{Argb, Theme, Video};

fn lin(c: Chan) -> f64 {
    let v = f64::from(c.get()) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn contrast(a: Argb, b: Argb) -> f64 {
    let l = |c: Argb| 0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b());
    let (x, y) = (l(a), l(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn hex(c: Argb) -> u32 {
    (u32::from(c.r().get()) << 16) | (u32::from(c.g().get()) << 8) | u32::from(c.b().get())
}

fn resolve(theme: &Theme, style: StyleWord) -> (Argb, Argb) {
    theme.resolve(style, Video::Normal, |_| None, |_| None)
}

fn faint() -> StyleWord {
    StyleWord::DEFAULT.with_attrs(Attrs::DIM)
}

#[test]
fn faint_default_text_is_half_way_to_the_page_on_both_themes() {
    for (theme, want, name) in [
        (Theme::light(), 0x8b8b8b, "light"),
        (Theme::dark(), 0x747474, "dark"),
    ] {
        let (nfg, nbg) = resolve(&theme, StyleWord::DEFAULT);
        let (fg, bg) = resolve(&theme, faint());
        assert_eq!(hex(bg), hex(nbg), "{name}: the page does not move");
        assert_eq!(hex(fg), want, "{name}");
        assert!(
            contrast(fg, bg) < contrast(nfg, nbg),
            "{name}: faint reads dimmer than normal"
        );
    }
}

/// A colour is mixed the same way: Modus red on the light page.
#[test]
fn faint_colour_text_is_half_way_to_the_page() {
    let red = StyleWord::DEFAULT
        .with_fg(ColorWord::indexed(Chan::from_u8(1)))
        .with_attrs(Attrs::DIM);
    let (fg, _) = resolve(&Theme::light(), red);
    // (0xa6 + 0xef) / 2, (0x00 + 0xef) / 2, rounded half up.
    assert_eq!(hex(fg), 0xcb7878);
}

/// **After the reverse swap**: `SGR 2;7` dims the text that is drawn -- the
/// swapped-in background colour -- and leaves the band, which is the text's
/// own colour, whole.
#[test]
fn faint_reverse_dims_the_text_drawn_not_the_band() {
    let theme = Theme::light();
    let (nfg, nbg) = resolve(&theme, StyleWord::DEFAULT.with_attrs(Attrs::REVERSE));
    let (fg, bg) = resolve(
        &theme,
        StyleWord::DEFAULT.with_attrs(Attrs::REVERSE.with(Attrs::DIM)),
    );
    assert_eq!(hex(bg), hex(nbg), "the band is the text colour, whole");
    assert_eq!(
        hex(nfg),
        0xefefef,
        "the control: reverse draws the page colour"
    );
    assert_eq!(hex(fg), 0x8b8b8b, "the drawn text, half way to the band");
}
