//! Renditions to pixels: the theme, the palette, and the attribute rules.

use baud_domain::{Attrs, Chan, ColorWord, StyleWord};

/// A pixel: `0xAARRGGBB`, straight alpha, which is what a `CALayer` and a
/// `wl_shm` buffer both take once premultiplied at the edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Argb(pub u32);

impl Argb {
    pub fn rgb(r: Chan, g: Chan, b: Chan) -> Argb {
        Argb(u32::from_be_bytes([0xFF, r.get(), g.get(), b.get()]))
    }

    fn channels(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    pub fn with_alpha(self, a: Chan) -> Argb {
        let [_, r, g, b] = self.channels();
        Argb(u32::from_be_bytes([a.get(), r, g, b]))
    }

    pub fn a(self) -> Chan {
        Chan::from_u8(self.channels()[0])
    }
    pub fn r(self) -> Chan {
        Chan::from_u8(self.channels()[1])
    }
    pub fn g(self) -> Chan {
        Chan::from_u8(self.channels()[2])
    }
    pub fn b(self) -> Chan {
        Chan::from_u8(self.channels()[3])
    }

    /// Each channel halved: what `SGR 2` does to a foreground.
    fn dimmed(self) -> Argb {
        let [a, r, g, b] = self.channels();
        Argb(u32::from_be_bytes([a, r / 2, g / 2, b / 2]))
    }
}

/// The colours a rendition resolves against. The 256-entry palette is
/// xterm's by default and an application may repaint it through `OSC 4`,
/// which the core records and [`Theme::resolve`] reads back.
#[derive(Clone)]
pub struct Theme {
    pub fg: Argb,
    pub bg: Argb,
    /// The alpha the default background is painted with: opaque unless a
    /// window wants the glass through (D14, default-background only).
    pub bg_alpha: Chan,
    pub palette: [Argb; 256],
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

impl Theme {
    /// Light text on near-black, xterm's palette.
    pub fn dark() -> Theme {
        Theme {
            fg: grey(0xD8),
            bg: grey(0x10),
            bg_alpha: Chan::from_u8(0xFF),
            palette: xterm_palette(),
        }
    }

    /// The foreground and background a rendition draws with, after the
    /// attribute rules: reverse swaps, bold brightens the first eight
    /// palette entries, dim halves, hidden hides.
    pub fn resolve(
        &self,
        style: StyleWord,
        palette: impl Fn(Chan) -> Option<(u8, u8, u8)>,
    ) -> (Argb, Argb) {
        let attrs = style.attrs();
        let pick = |w: ColorWord, default: Argb, bright: bool| -> Argb {
            if let Some((r, g, b)) = w.as_rgb() {
                return triple(r, g, b);
            }
            if let Some(i) = w.as_index() {
                let i = if bright && i < 8 { i + 8 } else { i };
                return match palette(Chan::from_u8(i)) {
                    Some((r, g, b)) => triple(r, g, b),
                    None => self.palette[usize::from(i)],
                };
            }
            default
        };
        let bold = attrs.has(Attrs::BOLD);
        let mut fg = pick(style.fg(), self.fg, bold);
        let mut bg = pick(style.bg(), self.bg, false);
        if attrs.has(Attrs::DIM) {
            fg = fg.dimmed();
        }
        if attrs.has(Attrs::REVERSE) {
            core::mem::swap(&mut fg, &mut bg);
        }
        if attrs.has(Attrs::HIDDEN) {
            fg = bg;
        }
        (fg, bg)
    }
}

impl Theme {
    /// The colour an underline draws in: `SGR 58`'s when set, else the
    /// resolved foreground.
    pub fn underline(
        &self,
        style: StyleWord,
        fg: Argb,
        palette: impl Fn(Chan) -> Option<(u8, u8, u8)>,
    ) -> Argb {
        let w = style.underline();
        if let Some((r, g, b)) = w.as_rgb() {
            return triple(r, g, b);
        }
        if let Some(i) = w.as_index() {
            return match palette(Chan::from_u8(i)) {
                Some((r, g, b)) => triple(r, g, b),
                None => self.palette[usize::from(i)],
            };
        }
        fg
    }
}

fn triple(r: u8, g: u8, b: u8) -> Argb {
    Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

fn grey(v: u8) -> Argb {
    triple(v, v, v)
}

/// xterm's 256-colour palette: the sixteen ANSI colours, a 6x6x6 cube and
/// a 24-step grey ramp.
pub fn xterm_palette() -> [Argb; 256] {
    let mut p = [Argb(0); 256];
    const ANSI: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xCD, 0x00, 0x00),
        (0x00, 0xCD, 0x00),
        (0xCD, 0xCD, 0x00),
        (0x00, 0x00, 0xEE),
        (0xCD, 0x00, 0xCD),
        (0x00, 0xCD, 0xCD),
        (0xE5, 0xE5, 0xE5),
        (0x7F, 0x7F, 0x7F),
        (0xFF, 0x00, 0x00),
        (0x00, 0xFF, 0x00),
        (0xFF, 0xFF, 0x00),
        (0x5C, 0x5C, 0xFF),
        (0xFF, 0x00, 0xFF),
        (0x00, 0xFF, 0xFF),
        (0xFF, 0xFF, 0xFF),
    ];
    for (i, &(r, g, b)) in ANSI.iter().enumerate() {
        p[i] = triple(r, g, b);
    }
    const STEPS: [u8; 6] = [0, 0x5F, 0x87, 0xAF, 0xD7, 0xFF];
    for (i, slot) in p.iter_mut().enumerate().skip(16).take(216) {
        let n = i - 16;
        *slot = triple(STEPS[n / 36], STEPS[(n / 6) % 6], STEPS[n % 6]);
    }
    for (i, slot) in p.iter_mut().enumerate().skip(232) {
        let v = u8::try_from(8 + (i - 232) * 10).unwrap_or(0xFF);
        *slot = grey(v);
    }
    p
}
