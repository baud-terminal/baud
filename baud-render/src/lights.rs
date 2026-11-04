//! **Activity lights**: small lights at the right of the title strip, or on
//! a line of their own in the frame ([`ActivityLights`]), each
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
//! **On their own line** (`LineState`, `Renderer::set_lights_line`) the
//! pictures are the same [`draw_led`] draws for the title strip's views, at
//! [`line_picture`]'s height in points, never the cell's, so the area clause
//! holds at every font size; the line is that and [`LINE_MARGIN`] above and
//! below, and is no row. AppKit's colours exist only on macOS and the two
//! systems draw the same pixels, so there the lights take the theme's
//! ([`line_colours`]), each lit one at 3:1 against the line. The pictures go
//! to the colour sheet once a look, and a light that changes names another.
//! The clock and the caps below are the title strip's.
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

/// **Where the activity lights are drawn**: `[window] activity_lights` as
/// the config resolves it for the system the window is on -- the title
/// strip on macOS, a line of their own on Linux, whose window has no title
/// bar -- or nowhere. One setting for all nine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActivityLights {
    /// In the window's title strip, beside AppKit's own buttons: macOS's.
    TitleBar,
    /// **A line of their own under the program's last line**, and under
    /// DEC's status line when a program shows one, in the window's own
    /// background: the VT420's keyboard indicator line, which "appears at
    /// the bottom of the screen, below the status line" and "in the same
    /// video background as the main display" (`[[source: vt420-uu-ch4]]`).
    /// It is no row of the program's: neither the rows nor the text area
    /// a program is told hold it, and only a report of the whole window
    /// does. Only as tall as the lights ([`LineRows`]), not a text row.
    BottomLine,
    /// The same line above the grid, which no DEC text governs.
    TopLine,
    /// Nowhere, and nothing is read for them.
    Off,
}

impl ActivityLights {
    /// Whether the lights are drawn anywhere, and so read for.
    pub const fn shown(self) -> bool {
        !matches!(self, ActivityLights::Off)
    }

    /// The edge of the window the lights' own line takes, or none: in the
    /// title strip or off, the frame keeps no line for them.
    pub const fn line(self) -> Option<LineEdge> {
        match self {
            ActivityLights::BottomLine => Some(LineEdge::Bottom),
            ActivityLights::TopLine => Some(LineEdge::Top),
            ActivityLights::TitleBar | ActivityLights::Off => None,
        }
    }
}

/// Which edge of the window the lights' own line is at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineEdge {
    /// Below the grid, the status line and the config's notice.
    Bottom,
    /// Above the grid.
    Top,
}

/// What a light says of the line, which its colour repeats: data in, data
/// out, or a state of the session or the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    In,
    Out,
    State,
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

    /// **What the light says of the line**: RD and HS bring data in, SD,
    /// AA and CS send it out or fail to, and the rest show a state.
    pub const fn direction(self) -> Direction {
        match self {
            Led::Rd | Led::Hs => Direction::In,
            Led::Sd | Led::Aa | Led::Cs => Direction::Out,
            Led::Cd | Led::Oh | Led::Fax | Led::Syn => Direction::State,
        }
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
/// `READ_CHUNK` row), and Linux's at most `N_TTY_BUF_SIZE` less one, 4095
/// (`[[source: linux-n-tty]]`, the same row). A read that long is HS's
/// sign that more was waiting, though not proof: exactly that much may
/// have been all there was.
pub const READ_CEILING: usize = match cfg!(target_os = "linux") {
    true => LINUX_CEILING,
    false => DARWIN_CEILING,
};
/// Darwin's ceiling, `TTYCLSIZE`.
pub const DARWIN_CEILING: usize = 1024;
/// Linux's ceiling, `N_TTY_BUF_SIZE` less the byte the line discipline keeps.
pub const LINUX_CEILING: usize = 4095;

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

/// **The space above and below the lights on their own line**, in
/// points: the line is the picture and this, and no taller.
pub const LINE_MARGIN: f32 = 2.0;
/// **The space the line keeps at its right end**, in points, as the title
/// strip keeps a gap at its own; a light that would start nearer the left
/// end than this is left out.
pub const LINE_END: f32 = 9.0;

/// **How tall the lights' picture is on their own line, in points**: the
/// tallest thing drawn above a light's centre is CS's bar -- the caps'
/// half height, the bar's gap and its stroke -- and below it the light's
/// own half, since the letters have no descender; [`draw_led`] centres
/// both on the picture, so the picture is twice the taller half, rounded
/// up to a whole point. In points and never from the cell, so a large
/// font does not enlarge the lights and the area clause WCAG 2.3.1's
/// threshold rests on holds at every size.
pub fn line_picture() -> f32 {
    let above = (CAP * TEXT / 2.0 + BAR_GAP + BAR).max(DOT / 2.0);
    (2.0 * above).ceil()
}

/// **The lights' line at a scale**, in device pixels: the picture's rows
/// and the margin above and below it, each rounded as [`draw_led`] rounds
/// the picture, so the line is exactly the picture and its margins.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineRows {
    pub picture: Px,
    pub margin: Px,
}

impl LineRows {
    /// The rows at `scale` device pixels a point; `None` for a scale no
    /// pixel count holds.
    pub fn at(scale: f32) -> Option<LineRows> {
        let px = |pt: f32| u32::try_from((pt * scale).round() as i64).ok().map(Px::of);
        Some(LineRows {
            picture: px(line_picture())?,
            margin: px(LINE_MARGIN)?,
        })
    }

    /// The line's whole height: the picture and a margin above and below.
    pub fn height(self) -> Px {
        Px::of(
            self.picture
                .value()
                .saturating_add(self.margin.value().saturating_mul(2)),
        )
    }
}

/// **Where each light sits on a line `width` pixels wide**, from the line's
/// left, as the title strip places its views: the strip of `strip` pixels,
/// laid out by [`layout`], ends `end` pixels in from the right, and a light
/// that would start less than `end` from the left is left out -- in a
/// narrow window the leftmost go first and the rest keep their places.
pub fn line_places(width: Px, at: &ByLed<Slot>, strip: Px, end: Px) -> ByLed<Option<Px>> {
    let x0 = i64::from(width.value()) - i64::from(end.value()) - i64::from(strip.value());
    ByLed::of(|led| {
        let x = x0 + i64::from(at.get(led).start.value());
        (x >= i64::from(end.value()))
            .then(|| u32::try_from(x).ok().map(Px::of))
            .flatten()
    })
}

/// **The lights' colours on their own line, from the theme**, where
/// AppKit's system colours do not exist and the two systems must draw the
/// same pixels: what comes in (RD, HS) in the palette's green, entry 2,
/// what goes out (SD, AA, CS) in its yellow, entry 3, and a state (CD,
/// OH, FAX, SYN) in its blue, entry 4 -- the hues the title strip's
/// system colours give them. Each lit colour is lifted, if it must be,
/// until it reads at 3:1 against the line, `page` drawn at `alpha` over
/// whatever `backdrop` spans: WCAG 2.2's SC 1.4.11, "a contrast ratio of
/// at least 3:1 against adjacent color(s)" for "Visual information
/// required to identify user interface components and states"
/// (`[[source: wcag22]]`). The letters are the foreground at two thirds
/// of its strength over the page, as the title strip's are AppKit's
/// secondary label colour, lifted to `text`, the floor the window's text
/// reads at. Where no colour reaches a target against all of the
/// backdrop, the lift goes as far as it can ([`crate::Floor`]).
pub fn line_colours(
    palette: &[Argb; 256],
    fg: Argb,
    page: Argb,
    alpha: Chan,
    backdrop: Backdrop,
    text: f64,
) -> (ByLed<Argb>, Argb) {
    let reads = |c: Argb, target: f64| {
        crate::color::reads_at(
            c.with_alpha(Chan::from_u8(0xFF)),
            page,
            alpha,
            backdrop,
            target,
        )
    };
    let [inward, outward, state] = [2, 3, 4].map(|i| reads(palette[i], LIT_CONTRAST));
    let lit = ByLed::of(|l| match l.direction() {
        Direction::In => inward,
        Direction::Out => outward,
        Direction::State => state,
    });
    let soft = fg
        .with_alpha(Chan::from_u8(0xFF))
        .mix(page, Chan::from_u8(170));
    (lit, reads(soft, text))
}

/// The contrast a lit light reads at against its line, at least: SC
/// 1.4.11's 3:1 for a component's state.
pub const LIT_CONTRAST: f64 = 3.0;

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

    /// **The next moment after `now` a step at `pace` could change a
    /// light**, or none while every light is as it will stay until
    /// something is seen: the end of a dwell, of a hold or of a refusal's
    /// hold, the soonest of any light's still ahead. A driver with no
    /// display link wakes for it, so a light goes dark when its hold ends
    /// and not at the next event; a wake at a moment that changes nothing
    /// costs one step. Stepped at `now` first, so nothing owed is behind.
    pub fn next_change(&self, now: HostTime, pace: Pace) -> Option<HostTime> {
        let t = pace.timing();
        Led::ALL
            .iter()
            .flat_map(|&l| {
                let lamp = self.lamps.get(l);
                [
                    lamp.since.map(|s| s.plus(t.dwell)),
                    (lamp.light == Light::Lit).then(|| lamp.last.plus(t.hold)),
                    lamp.refused.map(|r| r.plus(t.hold)),
                ]
            })
            .flatten()
            .filter(|&at| at > now)
            .min()
    }
}

/// **The lights' own line in the frame**, kept by the renderer: where it
/// is and how tall, what the lights show and how a lit one is drawn, the
/// looks made so far, and this frame's records -- one a placed light, each
/// naming that light's picture in its state on the colour sheet, in the
/// line's own pixels -- with what the CPU path last painted there.
pub(crate) struct LineState {
    pub(crate) line: Option<Line>,
    pub(crate) shown: Shown,
    pub(crate) glow: Glow,
    looks: Vec<(LookKey, LightsLook, Epoch)>,
    pub(crate) inst: Vec<crate::instance::Instance>,
    /// Each placed light's left and width, in the line's pixels: where its
    /// hover text is.
    pub(crate) places: ByLed<Option<(Px, Px)>>,
    pub(crate) painted: Option<(Argb, Vec<crate::instance::Instance>)>,
    pub(crate) fresh: Fresh,
}

/// Whether the last CPU frame repainted the line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Fresh {
    No,
    Yes,
}

/// Where the line is, its rows, and the scale its points are drawn at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Line {
    pub(crate) edge: LineEdge,
    pub(crate) rows: LineRows,
    pub(crate) scale: f32,
}

/// What a look on the line is made from: the three lit colours asked, the
/// foreground, the page, its alpha and backdrop, the letters' target, the
/// glow, and the scale the pictures are drawn at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct LookKey {
    pub(crate) asked: [Argb; 3],
    pub(crate) fg: Argb,
    pub(crate) page: Argb,
    pub(crate) alpha: Chan,
    pub(crate) backdrop: Backdrop,
    pub(crate) text: f64,
    pub(crate) glow: Glow,
    pub(crate) scale: f32,
}

/// Which look a picture on the colour sheet was drawn in: one a look made,
/// counted up from the first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Epoch(u64);

impl LineState {
    pub(crate) fn new() -> LineState {
        LineState {
            line: None,
            shown: Shown::DARK,
            glow: Glow::Full,
            looks: Vec::new(),
            inst: Vec::new(),
            places: ByLed::default(),
            painted: None,
            fresh: Fresh::No,
        }
    }

    /// The line's height in pixels: none without a line.
    pub(crate) fn height(&self) -> Px {
        self.line.map_or(Px::of(0), |l| l.rows.height())
    }

    /// **The look for `key` and its epoch**, the number its pictures are
    /// kept under on the colour sheet: from the looks made so far, else
    /// made now. A dimmed look scans its caps over every grey
    /// ([`Backdrop::any`]): what is under the line is the page, at its
    /// alpha over the window's material, and black to white bounds that
    /// whatever it is. Kept to the last few, which a theme and its reverse
    /// video, both glows and two scales fill.
    fn look(&mut self, key: LookKey, palette: &[Argb; 256]) -> (LightsLook, Epoch) {
        if let Some((_, l, e)) = self.looks.iter().find(|(k, _, _)| *k == key) {
            return (*l, *e);
        }
        let (lit, label) =
            line_colours(palette, key.fg, key.page, key.alpha, key.backdrop, key.text);
        let look = LightsLook::new(lit, label, key.page, key.glow, Backdrop::any());
        let epoch = Epoch(self.looks.last().map_or(0, |(_, _, e)| e.0 + 1));
        if self.looks.len() >= LOOKS_KEPT {
            self.looks.remove(0);
        }
        self.looks.push((key, look, epoch));
        (look, epoch)
    }

    /// **This frame's records**: each light at its state, placed by
    /// [`line_places`] on a line `width` pixels wide, its picture the one
    /// the colour sheet holds for this look. Every picture of a placed
    /// light -- dark, lit and unread -- is drawn there the first frame of a
    /// look ([`crate::atlas::Atlas::picture`]), so a light that changes
    /// names another picture and draws and uploads nothing.
    pub(crate) fn place(
        &mut self,
        face: &Face<'_>,
        atlas: &mut crate::atlas::Atlas,
        key: LookKey,
        palette: &[Argb; 256],
        width: Px,
    ) {
        self.inst.clear();
        self.places = ByLed::default();
        let Some(line) = self.line else {
            return;
        };
        let (look, epoch) = self.look(key, palette);
        let Some((at, strip)) = layout(face, line.scale) else {
            return;
        };
        let end = Px::of(u32::try_from((LINE_END * line.scale).round() as i64).unwrap_or(0));
        let xs = line_places(width, &at, strip, end);
        let height = line_picture();
        for (led, &light) in self.shown.iter() {
            let Some(x) = *xs.get(led) else {
                continue;
            };
            let slots = ByLight::of(|state| {
                atlas.picture(picture_key(epoch, led, state), || {
                    let mut img = LightsImage::new();
                    draw_led(face, &look, led, state, line.scale, height, &mut img)?;
                    let px = img.pixels.iter().map(|p| p.0).collect();
                    Some((px, img.width, img.height))
                })
            });
            let Some(slot) = *slots.get(light) else {
                continue;
            };
            *self.places.get_mut(led) = Some((x, at.get(led).width));
            let screen = |v: u32| crate::instance::Screen(i16::try_from(v).unwrap_or(i16::MAX));
            self.inst.push(crate::instance::Instance {
                x: screen(x.value()),
                y: screen(line.rows.margin.value()),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color: Argb(0xFFFF_FFFF),
                kind: slot.kind,
                curve: baud_domain::contrast::CurveRow::default(),
            });
        }
    }
}

/// How many looks the line keeps: a theme and its reverse video, under
/// either glow, at two scales.
const LOOKS_KEPT: usize = 8;

/// The key a light's picture in one state of one look is kept under on
/// the colour sheet: the look's epoch, the light and the state.
fn picture_key(epoch: Epoch, led: Led, light: Light) -> crate::atlas::PictureKey {
    let at = |i: Option<usize>| u64::try_from(i.unwrap_or(0)).unwrap_or(0);
    let l = at(Led::ALL.iter().position(|&x| x == led));
    let s = at(Light::ALL.iter().position(|&x| x == light));
    // A tag in the top byte keeps the lights' keys apart from any other
    // picture's.
    crate::atlas::PictureKey((0x4C << 56) | (epoch.0 << 8) | (l << 2) | s)
}

/// **Whether DEC's indicator line also carries the state lights**:
/// `[window] status_line_lights`, `"off"` by default, the lights then
/// only where `activity_lights` puts them; `"states"` writes the five that show a lasting
/// condition, CD, OH, CS, FAX and SYN, as words on the line while a
/// program shows it (`Term::set_indicator_lamps`). The traffic lights,
/// HS, AA, RD and SD, never go there: DEC's line holds states, and an
/// event has no indicator on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StatusLineLights {
    #[default]
    Off,
    States,
}

/// **Which clause of WCAG 2.3.1 the words on DEC's indicator line meet at
/// a cell size**: the words follow DEC's timing, not the lights' pace.
/// "The interval between the change in the state of a feature and the
/// update of the appropriate indicator shall not exceed 80 milliseconds.
/// Indicator changes are not queued: when an indicator is updated on the
/// Status Display, it shall reflect the state of that feature at the time
/// of the update" (`[[source: dec-std-070]]` 070-14 p. 14-6). A word that
/// follows a condition that fast is a flash whenever the condition comes
/// and goes: the line is reverse video, and a word's letters cross from
/// its ground to its ink. WCAG 2.2 passes content when "there are no more
/// than three general flashes and / or no more than three red flashes
/// within any one-second period; or the combined area of flashes occurring
/// concurrently occupies no more than a total of .006 steradians within
/// any 10 degree visual field on the screen (25% of any 10 degree visual
/// field on the screen)", the field estimated as "a 341 x 256 pixel
/// rectangle anywhere on the displayed screen area when the content is
/// viewed at 1024 x 768 pixels" on a "15-17 inch screen"
/// (`[[source: wcag22]]`).
///
/// **The area is bounded by whole cells**: every cell a word can take, the
/// letters and both unknown marks, fourteen, whatever the font draws in
/// them, all changing at once. A point is taken at 1024 pixels across the
/// 12 inches a 15-inch 4:3 screen is wide, 85.3 pixels an inch, at 72
/// points an inch -- the narrower of WCAG's two screens, so the most
/// pixels a point -- which holds on any display down to about 61 points an
/// inch, where a point is larger than that. Within [`AREA_LIMIT`] the
/// words pass by the area clause however often they change, and follow at
/// DEC's timing. **Past it they are left out** ([`line_lamps`]): they are
/// additions to DEC's floor, so leaving them out departs from neither
/// text, where holding them to three flashes a second would make a change
/// up to 200 ms late, past DEC's 80. Jev decided it, 0.98 and 0.94.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlashClause {
    /// The words' cells are under a quarter of the field: any rate.
    Area,
    /// They are not.
    Count,
}

/// The cells the five words can take, letters and marks: `CD?`, `OH?`,
/// `CS`, `FAX` and `SYN`.
pub const WORD_CELLS: f32 = 14.0;

/// A quarter of WCAG's 341 x 256 field, in its pixels.
pub const AREA_LIMIT: f32 = 341.0 * 256.0 / 4.0;

/// WCAG's pixels a point: 1024 pixels over a 15-inch 4:3 screen's 12
/// inches, at 72 points an inch.
pub const PIXELS_A_POINT: f32 = 1024.0 / 12.0 / 72.0;

/// **The clause the words meet in cells `w` by `h` points**, the cell
/// at the size and zoom the window draws: [`FlashClause::Area`] while
/// every cell they can take is within a quarter of the field.
pub fn flash_clause(w: f32, h: f32) -> FlashClause {
    let area = WORD_CELLS * w * h * PIXELS_A_POINT * PIXELS_A_POINT;
    match area <= AREA_LIMIT {
        true => FlashClause::Area,
        false => FlashClause::Count,
    }
}

/// **What DEC's line is given of the lamps read now**: `lamps` as they
/// are, with no hold and nothing shown later that has gone, while the
/// setting asks for them and the words meet the area clause; none
/// otherwise. A driver reads the lamps at each step it draws and hands
/// this to `Term::set_indicator_lamps`, which draws a change at once.
pub fn line_lamps(
    setting: StatusLineLights,
    clause: FlashClause,
    lamps: baud_core::IndicatorLamps,
) -> Option<baud_core::IndicatorLamps> {
    match (setting, clause) {
        (StatusLineLights::States, FlashClause::Area) => Some(lamps),
        (StatusLineLights::Off, _) | (_, FlashClause::Count) => None,
    }
}
