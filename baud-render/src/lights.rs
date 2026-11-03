//! **Activity lights**: small lights at the right of the title strip, each
//! named with a modem's letters and lit only for something baud measured,
//! in the order the 1988 Courier manual's table lists them -- "HS AA CD OH
//! RD SD TR MR RS CS ARQ SYN" (`[[source: usr-courier-hs-1988]]`, PDF
//! p.96-97; the V.34 and V.Everything manuals list SYN before ARQ/FAX) --
//! with the three baud has nothing honest for (TR, MR, RS: always on or
//! never lit, note AH 2.3) left out and FAX in ARQ's place, the light the
//! V.34 Courier shares as ARQ/FAX (`[[source: usr-courier-v34-1994]]`). The
//! Hayes Smartmodem's seven read the same way, right to left "MR, TR, SD,
//! RD, OH, CD and AA" (`[[source: hayes-smartmodem-owners-1981]]`, PDF
//! p.18-19). What lights each is [`Led`]'s; the viewpoint is the
//! computer's, and baud is the computer, so RD is a read of the master and
//! SD a write to it (note AH, 2.3). ARQ is not built: the record of another
//! process's connection that a retransmit is read from could not be shown
//! visible (note AH, 8.1 and 11).
//!
//! **The look**: each light's letters to its left, modem letters rather
//! than icons, in AppKit's own colours resolved under the window's
//! appearance (the HIG's "Prefer system-defined colors", and colour repeats
//! what the letters and the place already say -- "Avoid relying solely on
//! color", `[[source: apple-hig-color]]`). A dark light is its own colour
//! mixed most of the way into the strip's grey, so the row reads as lights
//! when nothing moves; a light baud may not read is drawn as a ring in the
//! letters' grey ([`Light::Unread`]), neither lit nor dark (note AH, Q8). CS is inverted
//! from the modem's -- lit when the program stopped taking what baud sends
//! -- and its letters carry a bar, the mark of an active-low signal. A
//! change is one step, as an LED's is.
//!
//! **The clock** ([`Activity`]): every light, the ones that show a state as
//! well as the ones that show an event, is lit for the hold after the last
//! time its thing was seen, and once it changes it does not change again
//! for the dwell; a thing seen while a dark light dwells lights it when the
//! dwell ends, so nothing seen goes unshown. At [`Pace::Full`] both are 200
//! ms, so a light's changes are at least 200 ms apart and any closed second
//! holds at most six of them, three flashes: WCAG 2.2's "no more than three
//! general flashes and / or no more than three red flashes within any
//! one-second period" (`[[source: wcag22]]`). The count holds per light:
//! lights out of phase can make more between them, so the strip meets 2.3.1
//! by the area clause the lights' size meets. A state goes through the
//! clock too, since one can toggle fast: a program that holds synchronized
//! output open across a tick and closes it between frames toggles SYN at
//! its frame rate. iTerm2's activity indicator holds two seconds after the
//! last read (`[[source: iterm2-964a75d-ptysession-isprocessing]]`), the
//! Linux tty trigger flashes 50 ms per 100 ms poll
//! (`[[source: linux-ledtrig-tty]]`); 200 ms keeps a modem's flicker under
//! the threshold.
//!
//! **Reduce Motion** ([`Pace::Steady`]) makes both times two seconds, as
//! blinking text is held steady under it rather than hidden: a stream of
//! activity reads as one steady light, and a light changes at most once
//! every two seconds. Apple's HIG asks an app to respond to the setting "by
//! reducing automatic and repetitive animations"
//! (`[[source: apple-hig-accessibility]]`).
//!
//! **Dim Flashing Lights** draws a lit light at the most of its colour over
//! the dark light that makes neither a general nor a red flash over any
//! grey of the backdrop the strip shows -- the standard material's range,
//! or black to white over the glass ([`dim_cap`], [`Glow::Dimmed`]), and an
//! unread light's ring at the most of the letters' grey over the dark lamp
//! that makes no flash from it dark or lit ([`ring_cap`]), so no change of
//! a light between any two of its states is a flash by WCAG's definition.

use crate::atlas::outline;
use crate::color::Argb;
use crate::color::Backdrop;
use crate::flash::{flashes, under_threshold};
use crate::units::slots;
use baud_domain::{Chan, Codepoint, HostTime};
use baud_font::{Canvas, Extent, Face, Point, Px, Rasterizer, Scale, VarPoint};
use core::time::Duration;

/// **Whether the activity lights are drawn**: `[window] activity_lights`,
/// on unless the config says off. One switch for the strip.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ActivityLights {
    #[default]
    On,
    Off,
}

/// **One light of the strip**, in its place left to right. The meaning of
/// each is the modem's, quoted, and then what lights it in baud (note AH,
/// 6.1):
///
/// * **HS**, High Speed. The Courier's "Remains ON after disconnect until
///   next call" is the rate it last ran at; baud's is a looser analogue,
///   lit while a read comes back at the platform's ceiling, a sign that
///   more was waiting ([`READ_CEILING`]).
/// * **AA**, Auto Answer: "Flashes ON for incoming ring detect" on the
///   V.34 Courier; baud's flashes when baud queued a reply nobody typed --
///   a device report, a colour's answer, a focus report.
/// * **CD**, Carrier Detect: "When the Smartmodem recognizes a carrier
///   signal, the CD indicator will light"; baud's while a member of the
///   job in front is an ssh holding an established TCP connection.
/// * **OH**, Off Hook: "illuminated when the Smartmodem is using the
///   telephone line"; baud's while a job other than the shell is in front.
/// * **RD**, Receive Data, and **SD**, Send Data: a read of the master,
///   and a write the kernel took any of.
/// * **CS**, Clear to Send, inverted: the Courier's is "ON until the modem
///   lowers CTS", always on in baud's sense; baud's is lit when the kernel
///   refused a write -- the program is not reading its input, or is gone.
/// * **FAX**: "Flashes steadily to indicate fax mode"; baud's while a
///   picture is placed on the screen.
/// * **SYN**: "ON when the modem ... enters synchronous mode"; baud's
///   while the program holds synchronized output (`?2026`) open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Led {
    Hs,
    Aa,
    Cd,
    Oh,
    Rd,
    Sd,
    Cs,
    Fax,
    Syn,
}

impl Led {
    /// Every light, left to right.
    pub const ALL: [Led; 9] = [
        Led::Hs,
        Led::Aa,
        Led::Cd,
        Led::Oh,
        Led::Rd,
        Led::Sd,
        Led::Cs,
        Led::Fax,
        Led::Syn,
    ];

    /// The modem's letters.
    pub const fn letters(self) -> &'static str {
        match self {
            Led::Hs => "HS",
            Led::Aa => "AA",
            Led::Cd => "CD",
            Led::Oh => "OH",
            Led::Rd => "RD",
            Led::Sd => "SD",
            Led::Cs => "CS",
            Led::Fax => "FAX",
            Led::Syn => "SYN",
        }
    }

    /// **What the pointer shows over the light**: its letters, what they
    /// stand for, and what lights it in baud, in plain words.
    pub const fn tip(self) -> &'static str {
        match self {
            Led::Hs => {
                "HS -- High Speed: lit while the program's output floods in, each read of it \
                 coming back as full as the line allows. A looser sense than a modem's, which \
                 showed the speed of the call."
            }
            Led::Aa => {
                "AA -- Auto Answer: flashes when baud answers the program on its own, with a \
                 reply nobody typed: the cursor's position, the colours, what the terminal is, \
                 a focus or a size report."
            }
            Led::Cd => {
                "CD -- Carrier Detect: lit while the program in front is ssh with its \
                 connection up. A ring when baud may not look at that program."
            }
            Led::Oh => {
                "OH -- Off Hook: lit while a program other than the shell is in front. A ring \
                 when baud may not look at the shell."
            }
            Led::Rd => "RD -- Receive Data: flashes when baud receives data from the program.",
            Led::Sd => {
                "SD -- Send Data: flashes when baud sends data to the program: your keys, a \
                 paste, the terminal's answers."
            }
            Led::Cs => {
                "CS -- Clear to Send, inverted: lit when the program stopped taking what baud \
                 sends it, because it is not reading its input or has gone. A modem's CS is \
                 lit while the line is clear; this one lights when it is not."
            }
            Led::Fax => "FAX -- Fax mode: lit while a picture is shown in the terminal.",
            Led::Syn => {
                "SYN -- Synchronous mode: lit while the program holds the screen to draw it \
                 all at once (synchronized output)."
            }
        }
    }

    /// **Whether the light rests on a read of another process**, which the
    /// kernel can refuse: OH's and CD's, the only two that can be
    /// [`Light::Unread`].
    pub const fn reads_a_process(self) -> bool {
        matches!(self, Led::Cd | Led::Oh)
    }

    /// Whether the letters carry a bar: CS's, which is inverted from the
    /// modem's.
    pub const fn bar(self) -> Bar {
        match self {
            Led::Cs => Bar::Over,
            _ => Bar::None,
        }
    }
}

/// A bar over a light's letters, the mark of a signal that is active low.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bar {
    None,
    Over,
}

/// **The read that is full at the platform's ceiling**: Darwin's tty hands
/// back at most `TTYCLSIZE`, "1024", a read (`[[source: xnu-tty-h]]`
/// 104-105), measured as 49,152 reads for 48 MB (CONTRIBUTING.md's
/// `READ_CHUNK` row). A read that long is HS's sign that more was waiting,
/// though not proof: exactly that much may have been all there was.
pub const READ_CEILING: usize = 1024;

/// **Whether `read` is HS's**: as long as [`READ_CEILING`].
pub fn full(read: &[u8]) -> bool {
    read.len() >= READ_CEILING
}

/// One thing of each light: a picture each, a clock each, a colour each.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ByLed<T> {
    pub hs: T,
    pub aa: T,
    pub cd: T,
    pub oh: T,
    pub rd: T,
    pub sd: T,
    pub cs: T,
    pub fax: T,
    pub syn: T,
}

impl<T> ByLed<T> {
    /// One for each light, from `f`.
    pub fn of(mut f: impl FnMut(Led) -> T) -> ByLed<T> {
        ByLed {
            hs: f(Led::Hs),
            aa: f(Led::Aa),
            cd: f(Led::Cd),
            oh: f(Led::Oh),
            rd: f(Led::Rd),
            sd: f(Led::Sd),
            cs: f(Led::Cs),
            fax: f(Led::Fax),
            syn: f(Led::Syn),
        }
    }

    /// The one for `led`.
    pub fn get(&self, led: Led) -> &T {
        match led {
            Led::Hs => &self.hs,
            Led::Aa => &self.aa,
            Led::Cd => &self.cd,
            Led::Oh => &self.oh,
            Led::Rd => &self.rd,
            Led::Sd => &self.sd,
            Led::Cs => &self.cs,
            Led::Fax => &self.fax,
            Led::Syn => &self.syn,
        }
    }

    pub fn get_mut(&mut self, led: Led) -> &mut T {
        match led {
            Led::Hs => &mut self.hs,
            Led::Aa => &mut self.aa,
            Led::Cd => &mut self.cd,
            Led::Oh => &mut self.oh,
            Led::Rd => &mut self.rd,
            Led::Sd => &mut self.sd,
            Led::Cs => &mut self.cs,
            Led::Fax => &mut self.fax,
            Led::Syn => &mut self.syn,
        }
    }

    /// Each light with its own, left to right.
    pub fn iter(&self) -> impl Iterator<Item = (Led, &T)> {
        Led::ALL.into_iter().map(move |l| (l, self.get(l)))
    }
}

/// One light's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Light {
    Dark,
    Lit,
    /// The read the light rests on was refused: a program baud may not
    /// read, which is neither quiet nor busy (note AH, 3.8 and Q8).
    Unread,
}

impl Light {
    /// Every state, for a cache of one picture each.
    pub const ALL: [Light; 3] = [Light::Dark, Light::Lit, Light::Unread];
}

/// What the lights show.
pub type Shown = ByLed<Light>;

impl ByLed<Light> {
    pub const DARK: Shown = ByLed {
        hs: Light::Dark,
        aa: Light::Dark,
        cd: Light::Dark,
        oh: Light::Dark,
        rd: Light::Dark,
        sd: Light::Dark,
        cs: Light::Dark,
        fax: Light::Dark,
        syn: Light::Dark,
    };

    /// Every light at `light`.
    pub fn all(light: Light) -> Shown {
        ByLed::of(|_| light)
    }
}

/// One of something for each state a light can show.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ByLight<T> {
    pub dark: T,
    pub lit: T,
    pub unread: T,
}

impl<T> ByLight<T> {
    /// One for each state, from `f`.
    pub fn of(mut f: impl FnMut(Light) -> T) -> ByLight<T> {
        ByLight {
            dark: f(Light::Dark),
            lit: f(Light::Lit),
            unread: f(Light::Unread),
        }
    }

    /// The one for `light`.
    pub fn get(&self, light: Light) -> &T {
        match light {
            Light::Dark => &self.dark,
            Light::Lit => &self.lit,
            Light::Unread => &self.unread,
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

/// The lights' colours and their letters' colour, straight alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LightsLook {
    /// Each light's lit colour.
    pub lit: ByLed<Argb>,
    /// Each light's colour while dark.
    pub dark: ByLed<Argb>,
    /// The letters', with the alpha they are drawn at; an unread light's
    /// ring is drawn in it too.
    pub label: Argb,
    /// How a lit light is drawn, and under Dim Flashing Lights each lit
    /// colour's alpha over its dark one ([`dim_cap`]) and, for a light
    /// that can be unread ([`Led::reads_a_process`]), the ring's over the
    /// dark lamp ([`ring_cap`]); none for the others, which never show it.
    pub glow: Glow,
    pub dim: ByLed<Chan>,
    pub ring: ByLed<Chan>,
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
    /// `backdrop`. Only a dimmed look scans the backdrop for its caps, once
    /// a colour: lights that share one share its caps.
    pub fn new(
        lit: ByLed<Argb>,
        label: Argb,
        ground: Argb,
        glow: Glow,
        backdrop: Backdrop,
    ) -> LightsLook {
        let dark = ByLed::of(|l| lit.get(l).mix(ground, unlit()));
        let full = Chan::from_u8(0xFF);
        let (dim, ring) = match glow {
            Glow::Full => (ByLed::of(|_| full), ByLed::of(|_| full)),
            Glow::Dimmed => {
                let mut caps: Vec<(Argb, Chan)> = Vec::new();
                let dim = ByLed::of(|l| {
                    let c = *lit.get(l);
                    if let Some(&(_, cap)) = caps.iter().find(|(k, _)| *k == c) {
                        return cap;
                    }
                    let cap = dim_cap(c, *dark.get(l), backdrop);
                    caps.push((c, cap));
                    cap
                });
                let mut rings: Vec<(Argb, Chan)> = Vec::new();
                let ring = ByLed::of(|l| {
                    let c = *lit.get(l);
                    if !l.reads_a_process() {
                        return Chan::from_u8(0);
                    }
                    if let Some(&(_, cap)) = rings.iter().find(|(k, _)| *k == c) {
                        return cap;
                    }
                    let ink = c.with_alpha(*dim.get(l));
                    let cap = ring_cap(label, ink, *dark.get(l), backdrop);
                    rings.push((c, cap));
                    cap
                });
                (dim, ring)
            }
        };
        LightsLook {
            lit,
            dark,
            label,
            glow,
            dim,
            ring,
        }
    }
}

/// **The most alpha `lit` may carry over `dark` under Dim Flashing
/// Lights**: the largest, from the interior's own cap
/// ([`under_threshold`]) down, at which every pixel a light can be drawn
/// with -- `dark` at any coverage, `lit` over it at that alpha and the
/// same coverage, exactly as [`draw_led`] blends them -- shown over any
/// grey of `backdrop` makes neither a general nor a red transition from
/// the same pixel dark. So turning a light on or off is no flash over any
/// grey the strip can be. Computed when the look changes, never a frame.
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

/// A coverage, the dark lamp's pixel at it, and each grey's view of that
/// pixel dark and lit.
type Views = (Chan, Argb, Vec<(Argb, Argb)>);

/// **The most alpha the unread ring's `label` may carry over `dark` under
/// Dim Flashing Lights**: the largest, up to the letters' own alpha, at
/// which no pixel of the ring -- `dark` at any coverage, `label` over it at
/// that alpha and the same coverage -- shown over any grey of `backdrop`
/// makes a flash from the same pixel dark, or from it lit, `ink` over the
/// same dark lamp. With `ink` at its own cap ([`dim_cap`]), a light moving
/// between any two of its three states makes no flash.
///
/// Searched from the top down, as [`dim_cap`] is, from the interior's own
/// cap ([`under_threshold`]), each alpha's pixels from the fullest coverage
/// down, the pixel the last alpha flashed at tried before any. Computed
/// when the look changes, never a frame.
pub fn ring_cap(label: Argb, ink: Argb, dark: Argb, backdrop: Backdrop) -> Chan {
    let full = Chan::from_u8(0xFF);
    let greys: Vec<Argb> = backdrop.greys().collect();
    // Each coverage's dark pixel, and each grey's view of it dark and lit,
    // once.
    let pairs: Vec<Views> = (0..=0xFF)
        .map(|c| {
            let c = Chan::from_u8(c);
            let d = over(Argb(0), dark, c);
            let l = over(d, ink, c);
            let seen = greys.iter().map(|&g| (over(g, d, full), over(g, l, full)));
            (c, d, seen.collect())
        })
        .collect();
    // The pixel the last alpha flashed at, tried first: the next alpha
    // down most often flashes there too.
    let mut witness: Option<(usize, usize)> = None;
    let mut calm = |k: Chan| {
        let ring = label.with_alpha(k);
        let flashes_at = |ci: usize, gi: usize| {
            let (c, d, seen) = &pairs[ci];
            let v = over(greys[gi], over(*d, ring, *c), full);
            let (dg, lg) = seen[gi];
            flashes(v, dg) || flashes(v, lg)
        };
        if witness.is_some_and(|(ci, gi)| flashes_at(ci, gi)) {
            return false;
        }
        for ci in (0..pairs.len()).rev() {
            if let Some(gi) = (0..greys.len()).find(|&gi| flashes_at(ci, gi)) {
                witness = Some((ci, gi));
                return false;
            }
        }
        true
    };
    let top = under_threshold(label, dark).get().min(label.a().get());
    (0..=top)
        .rev()
        .map(Chan::from_u8)
        .find(|&k| calm(k))
        .unwrap_or(Chan::from_u8(0))
}

/// The strip's grey a dark light is mixed into, by appearance: the middle
/// of the standard material's measured range, grey 33 to 92 under the
/// dark appearance and 196 to 235 under the light one
/// ([`crate::Backdrop::under_window_dark`] and `under_window_light`).
pub const GROUND_DARK: Argb = Argb(0xFF_3A_3A_3A);
pub const GROUND_LIGHT: Argb = Argb(0xFF_DC_DC_DC);

/// A drawn picture of lights, straight-alpha pixels row by row.
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

/// A light's diameter, the letters' size, the gap between a light and its
/// letters and the gap between two lights, in points: a light half a
/// traffic light across, letters the size of a small label.
pub const DOT: f32 = 7.0;
pub const TEXT: f32 = 8.0;
pub const GAP: f32 = 3.0;
pub const PAIR: f32 = 9.0;
/// JetBrains Mono's cap height, 730 of 1000 units, to centre the letters
/// on the lights.
const CAP: f32 = 0.73;
/// An unread light's ring, in points across its stroke.
const RING: f32 = 1.25;
/// CS's bar: its stroke and its gap above the letters' caps, in points.
const BAR: f32 = 0.9;
const BAR_GAP: f32 = 1.2;

/// The advance of `text` in `face` at `px` pixels per em.
fn advance(face: &Face<'_>, text: &str, px: f32) -> f32 {
    let scale = Scale::of(px, face.units_per_em());
    text.chars()
        .filter_map(|c| face.glyph(Codepoint::from_char(c)))
        .filter_map(|g| face.advance(g))
        .map(|a| scale.px(f32::from(a.value())))
        .sum()
}

/// **A light's own width, in points**: its letters, the gap, the light.
pub fn cell_width(face: &Face<'_>, led: Led) -> f32 {
    advance(face, led.letters(), TEXT) + GAP + DOT
}

/// **Where a light sits in a strip drawn at a scale**, in device pixels
/// from the strip's left: its start, rounded to a whole pixel so a light
/// drawn on its own lands on the same pixels as in the strip, and its
/// width, rounded up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub start: Px,
    pub width: Px,
}

/// The slots of every light at `scale`, and the strip's whole width in
/// device pixels; `None` for a size no pixel count holds.
pub fn layout(face: &Face<'_>, scale: f32) -> Option<(ByLed<Slot>, Px)> {
    let mut x = 0.0_f32;
    let mut end = Px::of(0);
    let mut out = ByLed::of(|_| Slot {
        start: Px::of(0),
        width: Px::of(0),
    });
    for led in Led::ALL {
        let w = cell_width(face, led);
        let start = u32::try_from((x * scale).round() as i64).ok()?;
        let width = u32::try_from((w * scale).ceil() as i64).ok()?;
        *out.get_mut(led) = Slot {
            start: Px::of(start),
            width: Px::of(width),
        };
        end = Px::of(start.checked_add(width)?);
        x += w + PAIR;
    }
    Some((out, end))
}

/// **The width the strip takes, in points**, in `face`: every light and
/// the gaps between them.
pub fn width(face: &Face<'_>) -> f32 {
    let lights: f32 = Led::ALL.iter().map(|&l| cell_width(face, l)).sum();
    lights + PAIR * (Led::ALL.len() - 1) as f32
}

/// Which way an outline winds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wind {
    Forward,
    Back,
}

/// A disc of radius `r` about `c`, wound `wind`, so a smaller one wound
/// back inside a larger cuts a ring.
fn disc(canvas: &mut Canvas<'_>, c: Point, r: f32, wind: Wind) {
    const KAPPA: f32 = 0.552_284_8;
    let k = KAPPA * r;
    let s = match wind {
        Wind::Forward => 1.0,
        Wind::Back => -1.0,
    };
    let at = |dx: f32, dy: f32| Point::new(c.x + dx, c.y + s * dy);
    canvas.cubic(at(r, 0.0), at(r, k), at(k, r), at(0.0, r));
    canvas.cubic(at(0.0, r), at(-k, r), at(-r, k), at(-r, 0.0));
    canvas.cubic(at(-r, 0.0), at(-r, -k), at(-k, -r), at(0.0, -r));
    canvas.cubic(at(0.0, -r), at(k, -r), at(r, -k), at(r, 0.0));
}

/// A rectangle from corner `a` to corner `b`.
fn rect(canvas: &mut Canvas<'_>, a: Point, b: Point) {
    let (p, q) = (Point::new(b.x, a.y), Point::new(a.x, b.y));
    canvas.line(a, p);
    canvas.line(p, b);
    canvas.line(b, q);
    canvas.line(q, a);
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

/// **One light, drawn** in `face` at `scale` device pixels a point,
/// `height` points tall, into `out`: its letters, then the light centred
/// on the strip and the letters' caps centred on it, in a picture its
/// [`Slot`]'s width. `None` for a size no canvas holds.
pub fn draw_led(
    face: &Face<'_>,
    look: &LightsLook,
    led: Led,
    light: Light,
    scale: f32,
    height: f32,
    out: &mut LightsImage,
) -> Option<()> {
    let w = Px::of(u32::try_from((cell_width(face, led) * scale).ceil() as i64).ok()?);
    let h = Px::of(u32::try_from((height * scale).round() as i64).ok()?);
    let extent = Extent::new(w, h);
    let r = DOT * scale / 2.0;
    let mid = height * scale / 2.0;
    let px = TEXT * scale;
    let run = advance(face, led.letters(), TEXT) * scale;
    let pen = Point::new(0.0, mid + CAP * px / 2.0);
    let centre = Point::new(run + GAP * scale + r, mid);
    let text = coverage(extent, |mut c| {
        if led.bar() == Bar::Over {
            let top = mid - CAP * px / 2.0 - BAR_GAP * scale;
            rect(
                &mut c,
                Point::new(0.0, top - BAR * scale),
                Point::new(run, top),
            );
        }
        letters(face, c, led.letters(), px, pen)
    })?;
    out.width = w;
    out.height = h;
    out.pixels.clear();
    out.pixels.resize(slots(w) * slots(h), Argb(0));
    for (p, &c) in out.pixels.iter_mut().zip(&text) {
        *p = over(*p, look.label, c);
    }
    let dot = coverage(extent, |mut c| {
        disc(&mut c, centre, r, Wind::Forward);
        c
    })?;
    let ring = match light {
        Light::Unread => Some(coverage(extent, |mut c| {
            disc(&mut c, centre, r, Wind::Forward);
            disc(&mut c, centre, r - RING * scale, Wind::Back);
            c
        })?),
        Light::Dark | Light::Lit => None,
    };
    // At full glow one disc in one colour, so a lit light's inside is its
    // colour exactly; dimmed, the dark disc and the lit colour at its cap
    // over it, the shape `dim_cap` holds under the threshold. An unread
    // light is a ring in the letters' colour, neither lit nor dark: hollow
    // at full glow, and dimmed a ring at its cap on the dark lamp, the
    // shape `ring_cap` holds under the threshold.
    let (lit, dark) = (*look.lit.get(led), *look.dark.get(led));
    let (under, top) = match (light, look.glow) {
        (Light::Dark, _) | (Light::Unread, Glow::Dimmed) => (Some(dark), None),
        (Light::Unread, Glow::Full) => (None, Some(look.label)),
        (Light::Lit, Glow::Full) => (Some(lit), None),
        (Light::Lit, Glow::Dimmed) => (Some(dark), Some(lit.with_alpha(*look.dim.get(led)))),
    };
    let top = match (light, look.glow) {
        (Light::Unread, Glow::Dimmed) => Some(look.label.with_alpha(*look.ring.get(led))),
        _ => top,
    };
    for (i, (p, &c)) in out.pixels.iter_mut().zip(&dot).enumerate() {
        if let Some(under) = under {
            *p = over(*p, under, c);
        }
        let cover = ring.as_ref().map_or(c, |ring| ring[i]);
        if let Some(top) = top {
            *p = over(*p, top, cover);
        }
    }
    Some(())
}

/// **The strip, drawn**: every light at `shown`, each in its [`Slot`] of
/// [`layout`], `height` points tall, into `out` -- the picture the views of
/// the lights make side by side, each pixel the one its light's own
/// picture holds. `None` for a size no canvas holds.
pub fn draw(
    face: &Face<'_>,
    look: &LightsLook,
    shown: Shown,
    scale: f32,
    height: f32,
    out: &mut LightsImage,
) -> Option<()> {
    let (at, w) = layout(face, scale)?;
    let h = Px::of(u32::try_from((height * scale).round() as i64).ok()?);
    out.width = w;
    out.height = h;
    out.pixels.clear();
    out.pixels.resize(slots(w) * slots(h), Argb(0));
    let mut one = LightsImage::new();
    for (led, &light) in shown.iter() {
        draw_led(face, look, led, light, scale, height, &mut one)?;
        let from = slots(at.get(led).start);
        let (cw, sw) = (slots(one.width).max(1), slots(w));
        for (y, row) in one.pixels.chunks(cw).enumerate() {
            let to = y * sw + from;
            out.pixels.get_mut(to..to + row.len())?.copy_from_slice(row);
        }
    }
    Some(())
}

/// **The two times a light runs on**: how long it stays lit after the
/// last time its thing was seen, and the least time between two of its
/// changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timing {
    pub hold: Duration,
    pub dwell: Duration,
}

/// **How fast the lights may change**, from the system's motion
/// preference: [`Pace::Full`], 200 ms each (HOLD_DWELL_200), or under
/// Reduce Motion [`Pace::Steady`], two seconds each, so a stream of
/// activity reads as one steady light.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pace {
    Full,
    Steady,
}

impl Pace {
    pub const fn timing(self) -> Timing {
        match self {
            Pace::Full => Timing {
                hold: Duration::from_millis(200),
                dwell: Duration::from_millis(200),
            },
            Pace::Steady => Timing {
                hold: Duration::from_secs(2),
                dwell: Duration::from_secs(2),
            },
        }
    }
}

/// Whether a light owes a sighting it has not shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owed {
    Nothing,
    Seen,
}

/// One light's clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Lamp {
    light: Light,
    /// When it last changed, if ever: a light that never changed has no
    /// dwell to wait out.
    since: Option<HostTime>,
    /// The last time its thing was seen.
    last: HostTime,
    owed: Owed,
    /// The last time its read was refused, if ever.
    refused: Option<HostTime>,
}

impl Lamp {
    const fn new() -> Lamp {
        Lamp {
            light: Light::Dark,
            since: None,
            last: HostTime::ZERO,
            owed: Owed::Nothing,
            refused: None,
        }
    }

    fn seen(&mut self, at: HostTime) {
        self.last = self.last.max(at);
        if self.light != Light::Lit {
            self.owed = Owed::Seen;
        }
    }

    fn refused(&mut self, at: HostTime) {
        self.refused = Some(self.refused.map_or(at, |r| r.max(at)));
    }

    fn step(&mut self, now: HostTime, t: Timing) {
        if self.since.is_some_and(|s| now < s.plus(t.dwell)) {
            return;
        }
        let held = now < self.last.plus(t.hold);
        let refused = self.refused.is_some_and(|r| now < r.plus(t.hold));
        let want = match (self.owed, self.light, held, refused) {
            (Owed::Seen, _, _, _) | (_, Light::Lit, true, _) => Light::Lit,
            (_, _, _, true) => Light::Unread,
            _ => Light::Dark,
        };
        if want != self.light {
            self.light = want;
            self.since = Some(now);
        }
        if want == Light::Lit {
            self.owed = Owed::Nothing;
        }
    }
}

/// **Every light's clock**: told each time its thing is seen -- a read, a
/// write, a reply, a state found set at a tick -- and of each read that was
/// refused, stepped on the display link's clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Activity {
    lamps: ByLed<Lamp>,
}

impl Default for Activity {
    fn default() -> Activity {
        Activity::new()
    }
}

impl Activity {
    pub const fn new() -> Activity {
        const L: Lamp = Lamp::new();
        Activity {
            lamps: ByLed {
                hs: L,
                aa: L,
                cd: L,
                oh: L,
                rd: L,
                sd: L,
                cs: L,
                fax: L,
                syn: L,
            },
        }
    }

    /// `led`'s thing was seen at `at`: bytes read or written, a reply
    /// queued, a state found set.
    pub fn seen(&mut self, led: Led, at: HostTime) {
        self.lamps.get_mut(led).seen(at);
    }

    /// The read `led` rests on was refused at `at`.
    pub fn refused(&mut self, led: Led, at: HostTime) {
        self.lamps.get_mut(led).refused(at);
    }

    /// What the lights show at `now`, each changed at most once, at `pace`.
    pub fn step(&mut self, now: HostTime, pace: Pace) -> Shown {
        let t = pace.timing();
        for led in Led::ALL {
            self.lamps.get_mut(led).step(now, t);
        }
        self.shown()
    }

    pub fn shown(&self) -> Shown {
        ByLed::of(|l| self.lamps.get(l).light)
    }
}
