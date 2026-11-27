//! **A reversed default page reads on the glass**: the bar `SGR 7` draws
//! in the default colours -- less's prompt, vim's status line -- DEC's
//! indicator status line, and the whole screen under `?5`, each drawn at the
//! lowest alpha, never below the window's dim, at which its text reads at
//! WCAG 2.2 SC 1.4.3's 4.5:1 (`[[source: wcag22]]`) over every grey the
//! window's material can show (`Theme::reversed_alpha`).
//!
//! Each reading is taken from the drawn pixels: the canvas pixel of a space
//! on the page, straight colour and alpha, laid over each grey by
//! source-over in encoded sRGB (`[[source: w3c-compositing-1]]`), rounded
//! as `Argb::mix` rounds, against the glyph colour drawn on it -- the text
//! colour with anti-aliasing off (`[[source: wcag22]]`, contrast ratio,
//! note 2), and against the part of the backdrop most like it
//! (`[[source: wcag22-technique-f83]]`). The luminance is computed here,
//! not borrowed from the renderer.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Backdrop, Blend, Damage, Instance, Kind, Metrics, PxSize, Renderer, Stack, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 16,
};

/// `[glass] dim_light`'s default, 0.60, and `dim_dark`'s, 0.70.
const DIM_LIGHT: u8 = 153;
const DIM_DARK: u8 = 179;

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn light(backdrop: Backdrop) -> Theme {
    Theme {
        bg_alpha: Chan::from_u8(DIM_LIGHT),
        backdrop,
        ..Theme::light()
    }
}

fn dark(backdrop: Backdrop) -> Theme {
    Theme {
        bg_alpha: Chan::from_u8(DIM_DARK),
        backdrop,
        ..Theme::dark()
    }
}

/// What a frame drew: the canvas's straight pixels and the glyphs.
struct Drawn {
    canvas: support::Canvas,
    metrics: Metrics,
    glyphs: Vec<Instance>,
    clear: Argb,
    clear_of: Argb,
    background: Vec<Argb>,
}

fn side(p: baud_font::Px) -> usize {
    usize::try_from(p.value()).unwrap()
}

impl Drawn {
    /// The straight pixel at the middle of cell `(row, col)`.
    fn page(&self, row: usize, col: usize) -> Argb {
        let (cw, ch) = (side(self.metrics.cell_w), side(self.metrics.cell_h));
        Argb(self.canvas.pixels[(row * ch + ch / 2) * self.canvas.width + col * cw + cw / 2])
    }

    /// The colours of the glyphs drawn on frame row `row`.
    fn text_on(&self, row: usize) -> Vec<Argb> {
        let ch = side(self.metrics.cell_h);
        self.glyphs
            .iter()
            .filter(|i| {
                let y = usize::try_from(i.y.0).unwrap();
                y >= row * ch && y < (row + 1) * ch
            })
            .map(|i| i.color)
            .collect()
    }
}

fn frame(theme: Theme, bytes: &[u8]) -> Drawn {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    r.fit(&t);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    Drawn {
        canvas,
        metrics: r.metrics(),
        glyphs: r
            .instances()
            .filter(|i| i.kind == Kind::Mask)
            .copied()
            .collect(),
        clear: r.clear_color(),
        clear_of: r.clear_color_of(&t),
        background: r.background().to_vec(),
    }
}

fn alpha(c: Argb) -> u8 {
    c.0.to_be_bytes()[0]
}

/// WCAG 2.2's relative luminance of an opaque colour.
fn lum(c: Argb) -> f64 {
    let [_, r, g, b] = c.0.to_be_bytes();
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn ratio(a: f64, b: f64) -> f64 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The straight pixel `page` laid over the opaque grey `v`: source-over in
/// encoded sRGB, each channel rounded half up.
fn over(page: Argb, v: u8) -> Argb {
    let [a, r, g, b] = page.0.to_be_bytes();
    let (a, v) = (u32::from(a), u32::from(v));
    let ch = |c: u8| u8::try_from((u32::from(c) * a + v * (255 - a) + 127) / 255).unwrap();
    Argb(u32::from_be_bytes([0xFF, ch(r), ch(g), ch(b)]))
}

/// The worst contrast of `text` against `page` over every grey from
/// `greys.0` to `greys.1`.
fn worst(text: Argb, page: Argb, greys: (u8, u8)) -> f64 {
    let t = lum(text);
    (greys.0..=greys.1)
        .map(|v| ratio(t, lum(over(page, v))))
        .fold(f64::INFINITY, f64::min)
}

/// Black to white: Liquid Glass's and Linux's, and the bound the raise
/// reads against on every material.
const ANY: (u8, u8) = (0, 255);

/// **The bar a program draws in `SGR 7` reads at 4.5:1 over every grey**
/// from black to white, on the light theme whatever its material, and on
/// the dark theme, whose bar read already and whose alpha is its dim. Its
/// page is found from the theme: 173 on the light theme, over the light
/// standard material too, and the dark dim, 179. Before the raise the
/// light bar drew at the dim, 153, and read 3.58:1 over white.
#[test]
fn a_reversed_default_bar_reads_at_4_5_over_every_grey() {
    for (name, theme, greys, raised) in [
        ("light, any", light(Backdrop::any()), ANY, 173),
        (
            "light, standard material",
            light(Backdrop::under_window_light()),
            ANY,
            173,
        ),
        ("dark, any", dark(Backdrop::any()), ANY, DIM_DARK),
    ] {
        // A space, then text, all reversed; the cursor parked off the bar.
        let d = frame(theme, b"\x1b[7m status \x1b[0m\x1b[3;16H");
        let page = d.page(0, 0);
        assert_eq!(page, d.background[0], "{name}: the canvas is the cell");
        let text = d.text_on(0);
        assert_eq!(text.len(), 6, "{name}: the bar's six letters");
        for c in text {
            let r = worst(c, page, greys);
            assert!(r >= 4.5, "{name}: the bar reads {r:.3}:1");
        }
        assert_eq!(alpha(page), raised, "{name}: the bar's alpha");
    }
}

/// **DEC's indicator status line reads the same**: its background is "the
/// reverse of the Main Display background" (`[[source: dec-std-070]]`
/// 070-14 p. 14-5), which baud draws as `SGR 7` in the default colours, so
/// it is raised as the bar is, and its text reads at 4.5:1.
#[test]
fn the_indicator_line_reads_at_4_5_over_every_grey() {
    for (name, theme, greys, raised) in [
        ("light, any", light(Backdrop::any()), ANY, 173),
        (
            "light, standard material",
            light(Backdrop::under_window_light()),
            ANY,
            173,
        ),
        ("dark, any", dark(Backdrop::any()), ANY, DIM_DARK),
    ] {
        let d = frame(theme, b"\x1b[1$~\x1b[?25l");
        // The indicator is the row under the three of the main display.
        let row = usize::from(WS.ws_row);
        let text = d.text_on(row);
        assert!(!text.is_empty(), "{name}: the indicator's text is drawn");
        // Its last cell, which no field reaches.
        let page = d.page(row, usize::from(WS.ws_col) - 1);
        for c in text {
            let r = worst(c, page, greys);
            assert!(r >= 4.5, "{name}: the indicator reads {r:.3}:1");
        }
        assert_eq!(alpha(page), raised, "{name}: the indicator's alpha");
    }
}

/// **The `?5` screen reads at 4.5:1 and stays glass**: every default
/// page under it, the margin a presenter clears to included, is raised as
/// the bar is -- and no further, so the screen is not a solid slab. A cell
/// reversed again inside it draws the normal page at the same alpha, so
/// the screen carries no hole.
#[test]
fn the_reverse_video_page_reads_at_4_5_and_stays_glass() {
    for (name, theme, greys, raised) in [
        ("light, any", light(Backdrop::any()), ANY, 173),
        (
            "light, standard material",
            light(Backdrop::under_window_light()),
            ANY,
            173,
        ),
        ("dark, any", dark(Backdrop::any()), ANY, DIM_DARK),
    ] {
        let bg = theme.bg;
        let d = frame(theme, b"\x1b[?5h page \x1b[7mx\x1b[0m\x1b[3;16H");
        let page = d.page(0, 0);
        let text = d.text_on(0);
        assert_eq!(text.len(), 5, "{name}: four letters and the x");
        for c in &text[..4] {
            let r = worst(*c, page, greys);
            assert!(r >= 4.5, "{name}: the page reads {r:.3}:1");
        }
        assert_eq!(alpha(page), raised, "{name}: the page's alpha");
        assert!(raised < 0xFF, "{name}: the page is glass, not a slab");
        assert_eq!(alpha(d.clear), raised, "{name}: the margin is the page");
        assert_eq!(d.clear_of, d.clear, "{name}: the driver's margin too");
        let normal = d.background[6];
        assert_eq!(alpha(normal), raised, "{name}: SGR 7 inside ?5");
        assert_eq!(
            normal.0 & 0x00FF_FFFF,
            bg.0 & 0x00FF_FFFF,
            "{name}: reversed twice, the normal page"
        );
    }
}

/// **What is not a reversed default page keeps the window's dim**: the
/// default page itself, a set background, and a cell reversed with a
/// colour of its own, on either side -- each drawn as before the raise.
/// And an opaque window draws every page opaque, as it did.
#[test]
fn every_other_background_keeps_the_dim() {
    for (name, theme, dim) in [
        ("light", light(Backdrop::any()), DIM_LIGHT),
        ("dark", dark(Backdrop::any()), DIM_DARK),
    ] {
        // Default, on red, reversed red text, reversed on blue, and under
        // `?5` a cell whose own text colour becomes its page.
        let d = frame(
            theme.clone(),
            b"a\x1b[41mb\x1b[0;7;31mc\x1b[0;7;44md\x1b[0m\x1b[3;16H",
        );
        for (col, what) in [(0, "default"), (1, "SGR 41"), (2, "SGR 7 on red text")] {
            assert_eq!(alpha(d.background[col]), dim, "{name}: {what}");
        }
        // Reversed with a set background: its page is the default
        // foreground, which is raised.
        assert!(
            alpha(d.background[3]) >= dim,
            "{name}: SGR 7 on blue draws the default foreground"
        );
        let r = frame(theme.clone(), b"\x1b[?5h\x1b[32mg\x1b[0m\x1b[3;16H");
        assert_eq!(alpha(r.background[0]), dim, "{name}: ?5 over green text");
        let opaque = frame(
            Theme {
                bg_alpha: Chan::from_u8(0xFF),
                ..theme
            },
            b"\x1b[7m bar \x1b[0m\x1b[?5h\x1b[3;16H",
        );
        assert_eq!(alpha(opaque.background[0]), 0xFF, "{name}: opaque");
        assert_eq!(alpha(opaque.clear), 0xFF, "{name}: opaque margin");
    }
}

/// **The dark theme draws as it did**: its bar reads from alpha 146, below
/// its dim of 179, so the raise leaves its pixels as the dim draws them.
/// The frame with the bar and the `?5` page is the frame drawn with every
/// background at the dim -- the same frame an opaque reversed page is not.
#[test]
fn the_dark_theme_is_unchanged() {
    let theme = dark(Backdrop::any());
    let with = |bytes: &[u8]| frame(theme.clone(), bytes);
    let bar = with(b"\x1b[7m status \x1b[0m\x1b[3;16H");
    let page = with(b"\x1b[?5h page \x1b[3;16H");
    for (what, d) in [("bar", &bar), ("?5 page", &page)] {
        assert_eq!(alpha(d.page(0, 0)), DIM_DARK, "dark {what}");
        assert_eq!(
            d.background[0].0 & 0x00FF_FFFF,
            Theme::dark().fg.0 & 0x00FF_FFFF,
            "dark {what}: the default foreground"
        );
    }
    assert_eq!(alpha(page.clear), DIM_DARK, "dark ?5 margin");
}

/// **The raised alpha is the lowest byte that reads**: one byte lower
/// reads below 4.5:1 somewhere from black to white, so the raise is no
/// further than it must be.
#[test]
fn the_raise_is_the_lowest_byte_that_reads() {
    let theme = light(Backdrop::any());
    let a = theme.reversed_alpha(theme.fg, theme.bg);
    let at = |a: u8| worst(theme.bg, theme.fg.with_alpha(Chan::from_u8(a)), ANY);
    assert!(at(a.get()) >= 4.5, "{:?} reads at {}", a, at(a.get()));
    assert!(at(a.get() - 1) < 4.5, "one lower reads {}", at(a.get() - 1));
    let d = dark(Backdrop::any());
    assert_eq!(d.reversed_alpha(d.fg, d.bg).get(), DIM_DARK);
    let low = Theme {
        bg_alpha: Chan::from_u8(56),
        ..d
    };
    let a = low.reversed_alpha(low.fg, low.bg).get();
    assert_eq!(a, 146, "the dark bar's own byte");
}

/// **One theme raises alike on every system**: the raise reads black to
/// white whatever material the theme's backdrop names, so the macOS
/// standard material, Liquid Glass and Linux draw one reversed page -- and
/// the `?5` margin and the lights' line on it, which are held to the same
/// pixels on every system (`lights_line.rs`). The standard material's
/// narrower greys would read at 165; it draws 173 with the rest.
#[test]
fn the_raise_is_the_same_on_every_material() {
    for (name, of) in [
        ("light", light as fn(Backdrop) -> Theme),
        ("dark", dark as fn(Backdrop) -> Theme),
    ] {
        let raise = |b: Backdrop| {
            let t = of(b);
            t.reversed_alpha(t.fg, t.bg)
        };
        let any = raise(Backdrop::any());
        assert_eq!(any, raise(Backdrop::under_window_light()), "{name}");
        assert_eq!(any, raise(Backdrop::under_window_dark()), "{name}");
    }
    let t = light(Backdrop::under_window_light());
    let standard = (196..=235)
        .map(|v| u8::try_from(v).unwrap())
        .collect::<Vec<_>>();
    let reads = |a: u8| {
        standard
            .iter()
            .map(|&v| ratio(lum(t.bg), lum(over(t.fg.with_alpha(Chan::from_u8(a)), v))))
            .fold(f64::INFINITY, f64::min)
    };
    assert!(
        reads(165) >= 4.5,
        "the premise: 165 reads over the material"
    );
    assert!(reads(164) < 4.5, "and 164 does not");
}
