//! Renditions to pixels: the theme, the palette, and the attribute rules.

use baud_domain::themefile::Scheme;
use baud_domain::{Attrs, Chan, Color, ColorWord, Count, DynamicColor, LINEAR, StyleWord};

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

    /// Half way to `other`, channel by channel in encoded sRGB, rounded half
    /// up; this colour's alpha kept. Faint text drawn on `other`
    /// (note Y, decision 10): ghostty's `faint-opacity` default of 0.5, which
    /// ghostty applies as opacity and blends in the native, encoded space.
    /// Mixed in linear light instead, faint default text on the light page
    /// reads at 1.86:1 where the dark page gets 7.19:1; encoded, 2.96:1 and
    /// 4.07:1.
    fn faint_on(self, other: Argb) -> Argb {
        let [a, r, g, b] = self.channels();
        let [_, or, og, ob] = other.channels();
        let half = |x: u8, y: u8| {
            u8::try_from((u16::from(x) + u16::from(y)).div_ceil(2)).unwrap_or(u8::MAX)
        };
        Argb(u32::from_be_bytes([
            a,
            half(r, or),
            half(g, og),
            half(b, ob),
        ]))
    }
}

/// **The contrast floor** (note Y, decision 6): text whose contrast with the
/// colour it is drawn on is below `trigger` is lifted to `target`, by a
/// linear-light mix toward black or white that keeps its hue. Only a pair the
/// application did not choose exactly: see [`Floor::covers`]. On by default at
/// 1.2 and 4.5.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Floor {
    Off,
    Lift { trigger: f64, target: f64 },
}

impl Floor {
    /// The owner's defaults: a trigger of about 1.2 -- "fixing the invisible
    /// pairs of any convention, the built-in light theme's included" -- and
    /// WCAG 2.2's 4.5:1 for text (SC 1.4.3).
    pub const DEFAULT: Floor = Floor::Lift {
        trigger: 1.2,
        target: 4.5,
    };

    /// Whether the floor may touch a pair drawn from these two colour words:
    /// they differ -- unless both are the default, since text and page are
    /// two defaults and not one choice -- and **the page is ours** (the
    /// default or an index 0-15) **or the text is the default**. An
    /// application that set the same colour on both sides, or set the page
    /// itself as a cube or truecolour colour and chose the text on it, chose
    /// the pair and keeps it.
    ///
    /// **Not "either side ours"**, which note Y proposed: a real chafa
    /// `--colors 240` preview (`tests/previews/chafa-240.ans`) draws palette
    /// red, `38;5;1`, on ramp grey `48;5;239` in nine cells, 1.04:1 under the
    /// light theme, and that rule lifted them into pale speckles inside the
    /// picture. Over every capture in `tests/`, those are the only cells the
    /// two rules treat differently (measured, 2026-09-29).
    pub fn covers(text: ColorWord, page: ColorWord) -> bool {
        let same = text == page && !text.is_default();
        let ours = |w: ColorWord| w.is_default() || w.as_index().is_some_and(|i| i < 16);
        !same && (ours(page) || text.is_default())
    }

    /// `text` as drawn on `page`, lifted when this floor covers the pair and
    /// its contrast is below the trigger.
    ///
    /// **Toward black or white, in linear light, just far enough**: the text's
    /// linear channels are scaled toward 0 or toward 1, which keeps their
    /// order and so the hue, until the pair reaches `target`. The direction
    /// keeps the text on its own side of the page -- lighter text gets
    /// lighter -- unless that side cannot reach the target, and then the
    /// other; on a mid-grey page where neither can, the extreme with the
    /// higher ratio. `#00ff00` on the light page becomes `#007f00` at 4.53,
    /// `#ffffff` becomes `#6c6c6c`.
    pub fn lift(self, text: Argb, page: Argb, (tw, pw): (ColorWord, ColorWord)) -> Argb {
        let Floor::Lift { trigger, target } = self else {
            return text;
        };
        if !Floor::covers(tw, pw) {
            return text;
        }
        let (lt, lp) = (Lum::of(text), Lum::of(page));
        if lt.ratio(lp) >= trigger {
            return text;
        }
        // The text's luminance at most (darker) or at least (lighter) for
        // the pair to read at `target`.
        let dark = (lp.0 + 0.05) / target - 0.05;
        let light = target * (lp.0 + 0.05) - 0.05;
        let (dark_ok, light_ok) = (dark >= 0.0, light <= 1.0);
        let toward_white = match (lt.0 >= lp.0, dark_ok, light_ok) {
            (true, _, true) | (false, false, true) => true,
            (false, true, _) | (true, true, false) => false,
            _ => Lum(1.0).ratio(lp) >= Lum(0.0).ratio(lp),
        };
        let [a, r, g, b] = text.channels();
        let out = |f: &dyn Fn(Linear) -> Chan| {
            let l = |v: u8| Linear::of(Chan::from_u8(v));
            [a, f(l(r)).get(), f(l(g)).get(), f(l(b)).get()]
        };
        let channels = if toward_white {
            let want = if light_ok { light } else { 1.0 };
            let t = if lt.0 >= 1.0 {
                0.0
            } else {
                ((want - lt.0) / (1.0 - lt.0)).clamp(0.0, 1.0)
            };
            out(&|c| Linear(c.0 + t * (1.0 - c.0)).encode_up())
        } else {
            let want = if dark_ok { dark } else { 0.0 };
            let s = if lt.0 <= 0.0 {
                0.0
            } else {
                (want / lt.0).clamp(0.0, 1.0)
            };
            out(&|c| Linear(c.0 * s).encode_down())
        };
        Argb(u32::from_be_bytes(channels))
    }
}

/// WCAG 2.2 relative luminance (`[[source: wcag22]]`), 0 to 1, through
/// `baud_domain::LINEAR`, the one sRGB decode the 996 classifier uses.
#[derive(Clone, Copy)]
struct Lum(f64);

impl Lum {
    fn of(c: Argb) -> Lum {
        let [_, r, g, b] = c.channels();
        let l = |v: u8| Linear::of(Chan::from_u8(v)).0;
        Lum(0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b))
    }

    /// The contrast ratio of the two, the lighter over the darker.
    fn ratio(self, other: Lum) -> f64 {
        (self.0.max(other.0) + 0.05) / (self.0.min(other.0) + 0.05)
    }
}

/// One channel in linear light, 0 to 1.
#[derive(Clone, Copy)]
struct Linear(f64);

impl Linear {
    fn of(c: Chan) -> Linear {
        Linear(f64::from(LINEAR[usize::from(c.get())]) / 65535.0)
    }

    /// The largest channel value whose linear light is at most this: rounding
    /// toward black never undershoots the contrast the lift reached.
    fn encode_down(self) -> Chan {
        let n = LINEAR.partition_point(|&v| f64::from(v) / 65535.0 <= self.0);
        Chan::from_u8(u8::try_from(n.saturating_sub(1)).unwrap_or(u8::MAX))
    }

    /// The smallest channel value whose linear light is at least this.
    fn encode_up(self) -> Chan {
        let n = LINEAR.partition_point(|&v| f64::from(v) / 65535.0 < self.0);
        Chan::from_u8(u8::try_from(n).unwrap_or(u8::MAX))
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
    /// The contrast floor, [`Floor::DEFAULT`] on both built-in themes.
    pub floor: Floor,
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
            floor: Floor::DEFAULT,
            palette: xterm_palette(),
        }
    }

    /// Dark text on near-white, with [`MODUS_OPERANDI`] as the sixteen.
    ///
    /// The two greys are [`Theme::dark`]'s mirrored channel for channel --
    /// `0xFF - 0xD8` is `0x27` and `0xFF - 0x10` is `0xEF` -- so the pair
    /// carries the same contrast rather than a second opinion about what
    /// "near-black" should be. Neither end is pure: `dark` does not use
    /// `0xFF` because white on black glares, and the mirror inherits that.
    ///
    /// **The sixteen are not xterm's**, which it had been: xterm keeps one
    /// palette under either background, so on this page its bright green
    /// read at 1.19:1 and its bright cyan at 1.09:1. The owner chose Modus
    /// Operandi's (note Y, decision 3), whose colours are made for a light
    /// page; the cube and the grey ramp stay xterm's.
    pub fn light() -> Theme {
        Theme {
            fg: grey(0x27),
            bg: grey(0xEF),
            bg_alpha: Chan::from_u8(0xFF),
            app_bg: AppBg::Dimmed,
            picture_bg: PictureBg::Opaque,
            floor: Floor::DEFAULT,
            palette: light_palette(),
        }
    }

    /// Lays a theme file's scheme over this half: each of the sixteen, the
    /// page and the text that the file sets replaces the half's, and the
    /// count of those it leaves unset -- which stay the half's -- comes back
    /// for the caller to report. Entries 16-255 are not written whatever the
    /// file says (note Y, decision 2): a program addresses them by number and
    /// expects xterm's cube. The dim, the floor and the exemptions stay the
    /// window's.
    pub fn take_scheme(&mut self, scheme: &Scheme) -> Count {
        let mut unset = 0;
        let pairs = self.palette[..16].iter_mut().zip(&scheme.ansi);
        let named = [(&mut self.bg, &scheme.page), (&mut self.fg, &scheme.text)];
        for (slot, c) in pairs.chain(named) {
            match (*c).and_then(argb_of) {
                Some(a) => *slot = a,
                None => unset += 1,
            }
        }
        Count::of(unset)
    }

    /// Dark or light by the rule `CSI ? 996 n` answers with: the page's
    /// relative luminance against the text's.
    pub fn polarity(&self) -> Option<baud_domain::ColorScheme> {
        let word = |a: Argb| {
            let [_, r, g, b] = a.channels();
            ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
        };
        baud_domain::ColorScheme::of(word(self.fg), word(self.bg))
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
    /// palette entries, faint mixes the drawn text half way to the colour it
    /// is drawn on -- all under `video`, the screen's `?5`.
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
        if attrs.has(Attrs::REVERSE) {
            core::mem::swap(&mut fg, &mut bg);
        }
        // **After the swap**: faint dims the text that is drawn, which under
        // `SGR 7` is the background's colour, toward the band it is drawn
        // on. Before the swap it would dim the band instead.
        if attrs.has(Attrs::DIM) {
            fg = fg.faint_on(bg);
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

/// A scheme colour as a palette entry; a scheme holds only `Rgb`.
fn argb_of(c: Color) -> Option<Argb> {
    match c {
        Color::Rgb(r, g, b) => Some(Argb::rgb(r, g, b)),
        Color::Default | Color::Indexed(_) => None,
    }
}

fn grey(v: u8) -> Argb {
    triple(v, v, v)
}

/// The sixteen of the light theme: **Modus Operandi by Protesilaos Stavrou,
/// (C) Free Software Foundation, GPL-3.0-or-later** -- its Emacs `ansi-color`
/// mapping, the sixteen hex values and no file text (note Y, decision 3;
/// `[[source: modus-operandi-theme-el]]`, pinned external). Designed as text
/// on a near-white page: every colour entry from 1 to 6 and 9 to 14 is at
/// least 6.09:1 on this theme's `0xEF`. Entries 7 and 15 are the light greys
/// that sit near the page, which the contrast floor exists for.
pub const MODUS_OPERANDI: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xA6, 0x00, 0x00),
    (0x00, 0x68, 0x00),
    (0x6F, 0x55, 0x00),
    (0x00, 0x31, 0xA9),
    (0x72, 0x10, 0x45),
    (0x00, 0x5E, 0x8B),
    (0xA6, 0xA6, 0xA6),
    (0x59, 0x59, 0x59),
    (0x97, 0x25, 0x00),
    (0x00, 0x66, 0x3F),
    (0x88, 0x49, 0x00),
    (0x35, 0x48, 0xCF),
    (0x53, 0x1A, 0xB6),
    (0x00, 0x5F, 0x5F),
    (0xFF, 0xFF, 0xFF),
];

/// xterm's 256 with [`MODUS_OPERANDI`] as the sixteen: the cube and the grey
/// ramp stay xterm's, because an application addresses them by number and
/// expects their fixed values.
pub fn light_palette() -> [Argb; 256] {
    let mut p = xterm_palette();
    for (slot, &(r, g, b)) in p.iter_mut().zip(&MODUS_OPERANDI) {
        *slot = triple(r, g, b);
    }
    p
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
