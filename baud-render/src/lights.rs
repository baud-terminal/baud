//! **Activity lights**: two small lights at the right of the title strip,
//! RD for bytes baud read from the child and SD for bytes the kernel took
//! from baud, as a modem's front panel shows them. "SD (Send Data)
//! indicator flashes when data is sent from the RS-232C port to the
//! Smartmodem"; "RD (Receive Data) light flashes when data received from a
//! distant computer or terminal is sent from the Smartmodem to the RS-232C
//! port" (`[[source: hayes-smartmodem-owners-1981]]`, PDF p.18-19). The
//! viewpoint is the computer's, and baud is the computer, so RD is a read
//! of the master and SD a write to it (note AH, 2.3).
//!
//! **The look**: Hayes's letters to the left of each light, modem letters
//! rather than icons, RD green and SD amber, AppKit's own
//! `systemGreenColor` and `systemOrangeColor` resolved under the window's
//! appearance (the HIG's "Prefer system-defined colors", and colour
//! repeats what the letters and the place already say -- "Avoid relying
//! solely on color", `[[source: apple-hig-color]]`). A dark light is its
//! own colour mixed most of the way into the strip's grey, so the pair
//! reads as two lights when nothing moves. A change is one step, as an
//! LED's is.
//!
//! **The clock** ([`Activity`]): a light is lit for [`HOLD`] after the last
//! byte, and once it changes it does not change again for [`DWELL`]; a byte
//! that lands while a dark light dwells lights it when the dwell ends, so
//! no byte goes unshown. A light's changes are at least 200 ms apart, so
//! any closed second holds at most six of them, three flashes: WCAG 2.2's
//! "no more than three general flashes and / or no more than three red
//! flashes within any one-second period" (`[[source: wcag22]]`). The count
//! holds per light: two out of phase can make six flashes between them, so
//! the pair meets 2.3.1 by the area clause the lights' size meets. iTerm2's
//! activity indicator holds two seconds after the last read
//! (`[[source: iterm2-964a75d-ptysession-isprocessing]]`), the Linux tty
//! trigger flashes 50 ms per 100 ms poll (`[[source: linux-ledtrig-tty]]`);
//! 200 ms keeps a modem's flicker under the threshold.
//!
//! **Dim Flashing Lights** draws a lit light at the most of its colour over
//! the dark light that makes neither a general nor a red flash over any
//! grey of the backdrop the strip shows -- the standard material's range,
//! or black to white over the glass ([`dim_cap`], [`Glow::Dimmed`]), so no
//! change of a light is a flash by WCAG's definition. Reduce Motion changes
//! nothing: a light moves only for a byte and steps with no animation.

use crate::atlas::outline;
use crate::color::Argb;
use crate::color::Backdrop;
use crate::flash::{flashes, under_threshold};
use crate::units::slots;
use baud_domain::{Chan, Codepoint, HostTime};
use baud_font::{Canvas, Extent, Face, Point, Px, Rasterizer, Scale, VarPoint};
use core::time::Duration;

/// **Whether the activity lights are drawn**: `[window] activity_lights`,
/// on unless the config says off.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ActivityLights {
    #[default]
    On,
    Off,
}

/// One light's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Light {
    Dark,
    Lit,
}

/// What the two lights show: RD, bytes in, and SD, bytes out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shown {
    pub rd: Light,
    pub sd: Light,
}

impl Shown {
    pub const DARK: Shown = Shown {
        rd: Light::Dark,
        sd: Light::Dark,
    };

    /// Every state, for a cache of one picture each.
    pub const ALL: [Shown; 4] = [
        Shown::DARK,
        Shown {
            rd: Light::Lit,
            sd: Light::Dark,
        },
        Shown {
            rd: Light::Dark,
            sd: Light::Lit,
        },
        Shown {
            rd: Light::Lit,
            sd: Light::Lit,
        },
    ];
}

/// One of something for each state the lights can show: a picture each,
/// drawn once and swapped in when the lights change.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ByShown<T> {
    pub dark: T,
    pub rd: T,
    pub sd: T,
    pub both: T,
}

impl<T> ByShown<T> {
    /// One for each state, from `f`.
    pub fn of(mut f: impl FnMut(Shown) -> T) -> ByShown<T> {
        let [dark, rd, sd, both] = Shown::ALL;
        ByShown {
            dark: f(dark),
            rd: f(rd),
            sd: f(sd),
            both: f(both),
        }
    }

    /// The one for `shown`.
    pub fn get(&self, shown: Shown) -> &T {
        match (shown.rd, shown.sd) {
            (Light::Dark, Light::Dark) => &self.dark,
            (Light::Lit, Light::Dark) => &self.rd,
            (Light::Dark, Light::Lit) => &self.sd,
            (Light::Lit, Light::Lit) => &self.both,
        }
    }
}

/// How a lit light is drawn: at its colour, or under Dim Flashing Lights
/// at the most of it over the dark light that makes no flash.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glow {
    Full,
    Dimmed,
}

/// The two lights' colours and their letters' colour, straight alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LightsLook {
    /// RD's lit colour, then SD's.
    pub lit: [Argb; 2],
    /// Each light's colour while dark.
    pub dark: [Argb; 2],
    /// The letters', with the alpha they are drawn at.
    pub label: Argb,
    /// How a lit light is drawn, and under Dim Flashing Lights each lit
    /// colour's alpha over its dark one ([`dim_cap`]).
    pub glow: Glow,
    pub dim: [Chan; 2],
}

/// How much of a light's colour its dark state keeps over the strip's
/// grey: 80 of 255, enough to read as an unlit lamp and far under a lit
/// one.
fn unlit() -> Chan {
    Chan::from_u8(80)
}

impl LightsLook {
    /// The look for lit colours `lit`, letters in `label`, a dark light
    /// mixed into `ground`, drawn at `glow` over a strip anywhere in
    /// `backdrop`. Only a dimmed look scans the backdrop for its caps.
    pub fn new(
        lit: [Argb; 2],
        label: Argb,
        ground: Argb,
        glow: Glow,
        backdrop: Backdrop,
    ) -> LightsLook {
        let dark = lit.map(|c| c.mix(ground, unlit()));
        let dim = match glow {
            Glow::Full => [Chan::from_u8(0xFF); 2],
            Glow::Dimmed => [
                dim_cap(lit[0], dark[0], backdrop),
                dim_cap(lit[1], dark[1], backdrop),
            ],
        };
        LightsLook {
            lit,
            dark,
            label,
            glow,
            dim,
        }
    }
}

/// **The most alpha `lit` may carry over `dark` under Dim Flashing
/// Lights**: the largest, from the interior's own cap
/// ([`under_threshold`]) down, at which every pixel a light can be drawn
/// with -- `dark` at any coverage, `lit` over it at that alpha and the
/// same coverage, exactly as [`draw`] blends them -- shown over any grey
/// of `backdrop` makes neither a general nor a red transition from the
/// same pixel dark. So turning a light on or off is no flash over any grey
/// the strip can be. Computed when the look changes, never a frame.
pub fn dim_cap(lit: Argb, dark: Argb, backdrop: Backdrop) -> Chan {
    let full = Chan::from_u8(0xFF);
    // Each coverage's dark pixel, and each grey's view of it, once.
    let pairs: Vec<(Chan, Argb, Vec<Argb>)> = (0..=0xFF)
        .map(|c| {
            let c = Chan::from_u8(c);
            let d = over(Argb(0), dark, c);
            (c, d, backdrop.greys().map(|g| over(g, d, full)).collect())
        })
        .collect();
    let top = under_threshold(lit, dark).get();
    (0..=top)
        .rev()
        .map(Chan::from_u8)
        .find(|&k| {
            let ink = lit.with_alpha(k);
            pairs.iter().all(|(c, d, seen)| {
                let l = over(*d, ink, *c);
                backdrop
                    .greys()
                    .zip(seen)
                    .all(|(g, &dg)| !flashes(over(g, l, full), dg))
            })
        })
        .unwrap_or(Chan::from_u8(0))
}

/// The strip's grey a dark light is mixed into, by appearance: the middle
/// of the standard material's measured range, grey 33 to 92 under the
/// dark appearance and 196 to 235 under the light one
/// ([`crate::Backdrop::under_window_dark`] and `under_window_light`).
pub const GROUND_DARK: Argb = Argb(0xFF_3A_3A_3A);
pub const GROUND_LIGHT: Argb = Argb(0xFF_DC_DC_DC);

/// A drawn strip of lights, straight-alpha pixels row by row.
#[derive(Clone, PartialEq, Debug)]
pub struct LightsImage {
    pub width: Px,
    pub height: Px,
    pub pixels: Vec<Argb>,
}

impl LightsImage {
    pub const fn new() -> LightsImage {
        LightsImage {
            width: Px::of(0),
            height: Px::of(0),
            pixels: Vec::new(),
        }
    }
}

impl Default for LightsImage {
    fn default() -> LightsImage {
        LightsImage::new()
    }
}

/// The letters, RD's then SD's.
const NAMES: [&str; 2] = ["RD", "SD"];
/// A light's diameter, the letters' size, the gap between a light and its
/// letters and the gap between the two pairs, in points: a light half a
/// traffic light across, letters the size of a small label.
pub const DOT: f32 = 7.0;
pub const TEXT: f32 = 8.0;
const GAP: f32 = 3.0;
const PAIR: f32 = 9.0;
/// JetBrains Mono's cap height, 730 of 1000 units, to centre the letters
/// on the lights.
const CAP: f32 = 0.73;

/// The advance of `text` in `face` at `px` pixels per em.
fn advance(face: &Face<'_>, text: &str, px: f32) -> f32 {
    let scale = Scale::of(px, face.units_per_em());
    text.chars()
        .filter_map(|c| face.glyph(Codepoint::from_char(c)))
        .filter_map(|g| face.advance(g))
        .map(|a| scale.px(f32::from(a.value())))
        .sum()
}

/// **The width the strip takes, in points**, in `face`.
pub fn width(face: &Face<'_>) -> f32 {
    let label: f32 = NAMES.iter().map(|t| advance(face, t, TEXT)).sum();
    label + 2.0 * (GAP + DOT) + PAIR
}

/// A disc of radius `r` about `c`.
fn disc(canvas: &mut Canvas<'_>, c: Point, r: f32) {
    const KAPPA: f32 = 0.552_284_8;
    let k = KAPPA * r;
    let at = |dx: f32, dy: f32| Point::new(c.x + dx, c.y + dy);
    canvas.cubic(at(r, 0.0), at(r, k), at(k, r), at(0.0, r));
    canvas.cubic(at(0.0, r), at(-k, r), at(-r, k), at(-r, 0.0));
    canvas.cubic(at(-r, 0.0), at(-r, -k), at(-k, -r), at(0.0, -r));
    canvas.cubic(at(0.0, -r), at(k, -r), at(r, -k), at(r, 0.0));
}

/// The coverage `draw` leaves over `extent`, a byte a pixel.
fn coverage(extent: Extent, draw: impl FnOnce(Canvas<'_>) -> Canvas<'_>) -> Option<Vec<Chan>> {
    let mut scratch = vec![0.0; extent.scratch()?];
    let canvas = draw(Canvas::new(extent, &mut scratch)?);
    let mut bytes = vec![0; extent.cells()?];
    canvas.coverage(&mut bytes)?;
    Some(bytes.into_iter().map(Chan::from_u8).collect())
}

/// `text`'s outlines from `face` at `px`, its baseline's left end at `pen`.
fn letters<'c>(
    face: &Face<'_>,
    mut canvas: Canvas<'c>,
    text: &str,
    px: f32,
    pen: Point,
) -> Canvas<'c> {
    let scale = Scale::of(px, face.units_per_em());
    let mut x = pen.x;
    let mut var: Vec<VarPoint> = Vec::new();
    for c in text.chars() {
        let Some(g) = face.glyph(Codepoint::from_char(c)) else {
            continue;
        };
        let mut r = Rasterizer::new(canvas, scale.placing(x, pen.y));
        let _ = outline(&mut var, face, g, &mut r);
        canvas = r.finish();
        x += face
            .advance(g)
            .map_or(0.0, |a| scale.px(f32::from(a.value())));
    }
    canvas
}

/// `src` at `cover` over `dst`, straight alpha, rounded to a byte.
fn over(dst: Argb, src: Argb, cover: Chan) -> Argb {
    let sa = f32::from(src.a().get()) / 255.0 * f32::from(cover.get()) / 255.0;
    if sa <= 0.0 {
        return dst;
    }
    let da = f32::from(dst.a().get()) / 255.0;
    let oa = sa + da * (1.0 - sa);
    let ch = |s: Chan, d: Chan| {
        Chan::from_unit(
            (f32::from(s.get()) * sa + f32::from(d.get()) * da * (1.0 - sa)) / oa / 255.0,
        )
    };
    Argb::rgb(
        ch(src.r(), dst.r()),
        ch(src.g(), dst.g()),
        ch(src.b(), dst.b()),
    )
    .with_alpha(Chan::from_unit(oa))
}

/// **The strip, drawn** in `face` at `scale` device pixels a point,
/// `height` points tall, into `out`: RD's letters and light, then SD's,
/// each light centred on the strip and each pair's letters on its light.
/// `None` for a size no canvas holds.
pub fn draw(
    face: &Face<'_>,
    look: &LightsLook,
    shown: Shown,
    scale: f32,
    height: f32,
    out: &mut LightsImage,
) -> Option<()> {
    let w = Px::of(u32::try_from((width(face) * scale).ceil() as i64).ok()?);
    let h = Px::of(u32::try_from((height * scale).round() as i64).ok()?);
    let extent = Extent::new(w, h);
    let r = DOT * scale / 2.0;
    let mid = height * scale / 2.0;
    let px = TEXT * scale;
    let mut pens = [Point::new(0.0, 0.0); 2];
    let mut centres = [Point::new(0.0, 0.0); 2];
    let mut x = 0.0;
    for i in 0..2 {
        pens[i] = Point::new(x, mid + CAP * px / 2.0);
        x += advance(face, NAMES[i], TEXT) * scale + GAP * scale;
        centres[i] = Point::new(x + r, mid);
        x += 2.0 * r + PAIR * scale;
    }
    let text = coverage(extent, |c| {
        let c = letters(face, c, NAMES[0], px, pens[0]);
        letters(face, c, NAMES[1], px, pens[1])
    })?;
    out.width = w;
    out.height = h;
    out.pixels.clear();
    out.pixels.resize(slots(w) * slots(h), Argb(0));
    for (p, &c) in out.pixels.iter_mut().zip(&text) {
        *p = over(*p, look.label, c);
    }
    for (i, light) in [shown.rd, shown.sd].into_iter().enumerate() {
        let dot = coverage(extent, |mut c| {
            disc(&mut c, centres[i], r);
            c
        })?;
        // At full glow one disc in one colour, so a lit light's inside is
        // its colour exactly; dimmed, the dark disc and the lit colour at
        // its cap over it, the shape `dim_cap` holds under the threshold.
        let (under, top) = match (light, look.glow) {
            (Light::Dark, _) => (look.dark[i], None),
            (Light::Lit, Glow::Full) => (look.lit[i], None),
            (Light::Lit, Glow::Dimmed) => (look.dark[i], Some(look.lit[i].with_alpha(look.dim[i]))),
        };
        for (p, &c) in out.pixels.iter_mut().zip(&dot) {
            *p = over(*p, under, c);
            if let Some(top) = top {
                *p = over(*p, top, c);
            }
        }
    }
    Some(())
}

/// **How long a light stays lit after the last byte, and the least time
/// between two of its changes** (HOLD_DWELL_200).
pub const HOLD: Duration = Duration::from_millis(200);
pub const DWELL: Duration = Duration::from_millis(200);

/// Whether a dark light owes a byte it has not shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owed {
    Nothing,
    Byte,
}

/// One light's clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Lamp {
    light: Light,
    /// When it last changed.
    since: HostTime,
    /// The last byte.
    last: HostTime,
    owed: Owed,
}

impl Lamp {
    const fn new() -> Lamp {
        Lamp {
            light: Light::Dark,
            since: HostTime::ZERO,
            last: HostTime::ZERO,
            owed: Owed::Nothing,
        }
    }

    fn moved(&mut self, at: HostTime) {
        self.last = self.last.max(at);
        if self.light == Light::Dark {
            self.owed = Owed::Byte;
        }
    }

    fn step(&mut self, now: HostTime) {
        let settled = now >= self.since.plus(DWELL);
        match self.light {
            Light::Dark if self.owed == Owed::Byte && settled => {
                self.light = Light::Lit;
                self.since = now;
                self.owed = Owed::Nothing;
            }
            Light::Lit if settled && now >= self.last.plus(HOLD) => {
                self.light = Light::Dark;
                self.since = now;
            }
            _ => {}
        }
    }
}

/// **The two lights' clocks**: told of each read and each write that moved
/// bytes, stepped on the display link's clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Activity {
    rd: Lamp,
    sd: Lamp,
}

impl Default for Activity {
    fn default() -> Activity {
        Activity::new()
    }
}

impl Activity {
    pub const fn new() -> Activity {
        Activity {
            rd: Lamp::new(),
            sd: Lamp::new(),
        }
    }

    /// A read brought bytes at `at`.
    pub fn received(&mut self, at: HostTime) {
        self.rd.moved(at);
    }

    /// The kernel took bytes at `at`.
    pub fn sent(&mut self, at: HostTime) {
        self.sd.moved(at);
    }

    /// What the lights show at `now`, each light changed at most once.
    pub fn step(&mut self, now: HostTime) -> Shown {
        self.rd.step(now);
        self.sd.step(now);
        self.shown()
    }

    pub fn shown(&self) -> Shown {
        Shown {
            rd: self.rd.light,
            sd: self.sd.light,
        }
    }
}
