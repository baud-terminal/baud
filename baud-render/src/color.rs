//! Renditions to pixels: the theme, the palette, and the attribute rules.

use baud_domain::{Attrs, Chan, ColorWord, DynamicColor, StyleWord};

/// A pixel: `0xAARRGGBB`, straight alpha, which is what a `CALayer` and a
/// `wl_shm` buffer both take once premultiplied at the edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(transparent)]
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
    /// The alpha **every** background is painted with -- the default one
    /// and any an application sets -- so a window's translucency is not a
    /// property only the unstyled cells have. Opaque unless a window wants
    /// the glass through; `baud-render/tests/translucent.rs` is the gate,
    /// and it is the only one in the workspace that renders below `0xFF`.
    pub bg_alpha: Chan,
    /// Whether a background the application set is drawn under the dim
    /// or above it. The default is [`AppBg::Dimmed`]: every background is
    /// translucent, which is the owner's 2026-09-20 call. [`AppBg::Opaque`]
    /// keeps every background an application set at full alpha and leaves
    /// only the default one glass; see [`Theme::opaque_bg`].
    pub app_bg: AppBg,
    /// Whether a picture drawn in text keeps its backgrounds solid under
    /// [`AppBg::Dimmed`]. See [`PictureBg`].
    pub picture_bg: PictureBg,
    pub palette: [Argb; 256],
}

/// What the window's dim does to a background the application set.
///
/// **Why the rule is every set background and not something narrower**, and
/// the one exception [`PictureBg`] carves out of it by looking at a block of
/// cells rather than at one,
/// measured 2026-09-22 on the owner's own yazi preview: a file manager
/// with no image protocol draws a photo with chafa, whose output is a
/// grid of cells each carrying its own foreground *and* background --
/// block elements, box-drawing strokes, and plain spaces for the flat
/// areas. On that .heic 68% of the preview's area was cell background, and
/// on a photo with flat regions most cells are spaces. A space on a set
/// background is also every TUI panel there is, so no rule that looks at
/// one cell can keep the image's spaces opaque and btop's glass; the
/// block-elements-only rule this replaced covered 66% of that photo's
/// cells and 19% of another's. So the choice is the owner's, per window,
/// and it is this key -- and under `Dimmed`, [`PictureBg`] finds the
/// pictures from the blocks they make and draws those solid.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AppBg {
    /// The same alpha as the default background.
    #[default]
    Dimmed,
    /// Opaque: a colour an application chose is drawn as it chose it.
    Opaque,
}

/// What the window's dim does to a picture drawn in text, under
/// [`AppBg::Dimmed`] -- a block of set backgrounds holding at least
/// [`crate::PICTURE_COLOURS`] distinct colours, found by
/// [`crate::Pictures`]. The owner's call, 2026-09-22: panels stay glass and
/// pictures are drawn solid, because under the dim a picture splits into
/// solid glyphs beside see-through backgrounds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PictureBg {
    /// A picture's backgrounds are drawn solid.
    #[default]
    Opaque,
    /// A picture is dimmed like every other background.
    Dimmed,
}

/// Whether the screen is drawn in reverse video: `?5`, DECSCNM.
///
/// **The rule is xterm's `ReverseVideo`** (util.c 3269-3345, the file
/// `[[source: xterm-scroll]]` pins): the default foreground and
/// background swap, and so do palette entries 0 and 7 and entries 8 and 15 --
/// "by convention, these are the colors assigned to black and white" -- while
/// every other entry, and every truecolour, is left alone. xterm swaps the
/// colours the table *holds*, so an `OSC 4` override set before the flip
/// travels with its entry; here the index is mapped before the table is read,
/// which is the same answer. A cell's own `SGR 7` still swaps after that, so
/// reverse inside reverse video draws a normal cell.
///
/// **Every background keeps the window's alpha under it**, the reversed
/// default one included: the owner's call, 2026-09-25, "same alpha for all".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Video {
    #[default]
    Normal,
    Reverse,
}

impl Video {
    /// The palette entry a colour index reads under this video: 0 and 7, and
    /// 8 and 15, trade places.
    pub fn entry(self, i: Chan) -> Chan {
        let swapped = match (self, i.get()) {
            (Video::Normal, n) => n,
            (Video::Reverse, 0) => 7,
            (Video::Reverse, 7) => 0,
            (Video::Reverse, 8) => 15,
            (Video::Reverse, 15) => 8,
            (Video::Reverse, n) => n,
        };
        Chan::from_u8(swapped)
    }
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
            app_bg: AppBg::Dimmed,
            picture_bg: PictureBg::Opaque,
            palette: xterm_palette(),
        }
    }

    /// Dark text on near-white, **the same xterm palette**.
    ///
    /// The two greys are [`Theme::dark`]'s mirrored channel for channel --
    /// `0xFF - 0xD8` is `0x27` and `0xFF - 0x10` is `0xEF` -- so the pair
    /// carries the same contrast rather than a second opinion about what
    /// "near-black" should be. Neither end is pure: `dark` does not use
    /// `0xFF` because white on black glares, and the mirror inherits that.
    ///
    /// **The sixteen ANSI entries are unchanged, and that is a known
    /// legibility gap rather than an oversight.** xterm keeps one palette
    /// under either background, so `SGR 33` is the same yellow on white as
    /// on black and reads poorly there; every light theme that fixes it --
    /// Solarized, Apple's Basic -- does so by redefining those sixteen,
    /// which is a palette *opinion* this crate does not hold. An
    /// application can already repaint them with `OSC 4`, and the core
    /// records that, so the mechanism exists. Choosing the numbers is the
    /// owner's call and is not made here.
    pub fn light() -> Theme {
        Theme {
            fg: grey(0x27),
            bg: grey(0xEF),
            bg_alpha: Chan::from_u8(0xFF),
            app_bg: AppBg::Dimmed,
            picture_bg: PictureBg::Opaque,
            palette: xterm_palette(),
        }
    }

    /// Whether a cell drawn under `style` keeps its background opaque: the
    /// exemption is on and the background it resolves to is one the
    /// application set. A pure function of the rendition, so the cell path
    /// bakes it into the style table and the shader reads one bit.
    pub fn opaque_bg(&self, style: StyleWord) -> bool {
        self.app_bg == AppBg::Opaque && Self::set_bg(style)
    }

    /// Whether pictures drawn in text are looked for at all: only under the
    /// dim, only while the pictures are asked for solid, and only when the
    /// dim is below opaque -- otherwise every background is already what a
    /// picture would be drawn as, and finding them would change nothing.
    pub fn finds_pictures(&self) -> bool {
        self.app_bg == AppBg::Dimmed
            && self.picture_bg == PictureBg::Opaque
            && self.bg_alpha.get() < 0xFF
    }

    /// Whether the background `style` resolves to is one the application
    /// set rather than the default -- under `SGR 7` that is the
    /// foreground, because reverse swaps them.
    pub fn set_bg(style: StyleWord) -> bool {
        if style.attrs().has(Attrs::REVERSE) {
            !style.fg().is_default()
        } else {
            !style.bg().is_default()
        }
    }

    /// The foreground and background a rendition draws with, after the
    /// attribute rules: reverse swaps, bold brightens the first eight
    /// palette entries, dim halves -- all under `video`, the screen's `?5`.
    /// Hiding is not a colour and is not here: `Resolved::text`.
    pub fn resolve(
        &self,
        style: StyleWord,
        video: Video,
        palette: impl Fn(Chan) -> Option<(u8, u8, u8)>,
        dynamic: impl Fn(DynamicColor) -> Option<(u8, u8, u8)>,
    ) -> (Argb, Argb) {
        let attrs = style.attrs();
        let (fg_default, bg_default) = self.defaults(video, &dynamic);
        let pick = |w: ColorWord, default: Argb, bright: bool| -> Argb {
            if let Some((r, g, b)) = w.as_rgb() {
                return triple(r, g, b);
            }
            if let Some(i) = w.as_index() {
                let i = if bright && i < 8 { i + 8 } else { i };
                return self.entry(video.entry(Chan::from_u8(i)), &palette);
            }
            default
        };
        let bold = attrs.has(Attrs::BOLD);
        let mut fg = pick(style.fg(), fg_default, bold);
        let mut bg = pick(style.bg(), bg_default, false);
        if attrs.has(Attrs::DIM) {
            fg = fg.dimmed();
        }
        if attrs.has(Attrs::REVERSE) {
            core::mem::swap(&mut fg, &mut bg);
        }
        // `SGR 8` is not a colour: concealed text keeps its foreground, so a
        // block cursor over it is drawn in it, and draws nothing of its own
        // (`Resolved::text`). Drawing it in the background's colour, as this
        // did, left an opaque silhouette on every translucent background.
        (fg, bg)
    }

    /// The default foreground and background the screen draws under `video`:
    /// the theme's unless the application moved them with `OSC 10`/`11` --
    /// consulted first, the way `palette` is consulted before the theme's own
    /// table -- and swapped under reverse video, so the default a program
    /// moved is the one `?5` puts on the other side. What a margin, a
    /// scrollbar or a fade drawn "in the foreground" or "in the background"
    /// takes.
    pub fn defaults(
        &self,
        video: Video,
        dynamic: impl Fn(DynamicColor) -> Option<(u8, u8, u8)>,
    ) -> (Argb, Argb) {
        let fg = dynamic(DynamicColor::Foreground).map_or(self.fg, |(r, g, b)| triple(r, g, b));
        let bg = dynamic(DynamicColor::Background).map_or(self.bg, |(r, g, b)| triple(r, g, b));
        match video {
            Video::Normal => (fg, bg),
            Video::Reverse => (bg, fg),
        }
    }

    /// Palette entry `i`: the application's `OSC 4` override, else the
    /// theme's table.
    fn entry(&self, i: Chan, palette: impl Fn(Chan) -> Option<(u8, u8, u8)>) -> Argb {
        match palette(i) {
            Some((r, g, b)) => triple(r, g, b),
            None => self.palette[usize::from(i.get())],
        }
    }
}

impl Theme {
    /// The colour an underline draws in: `SGR 58`'s when set, else the
    /// resolved foreground.
    pub fn underline(
        &self,
        style: StyleWord,
        video: Video,
        fg: Argb,
        palette: impl Fn(Chan) -> Option<(u8, u8, u8)>,
    ) -> Argb {
        let w = style.underline();
        if let Some((r, g, b)) = w.as_rgb() {
            return triple(r, g, b);
        }
        if let Some(i) = w.as_index() {
            return self.entry(video.entry(Chan::from_u8(i)), palette);
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
