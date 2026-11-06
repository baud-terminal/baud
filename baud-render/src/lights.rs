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
//! **The look**: modem letters rather than icons, and a dot with a soft
//! glow while it is lit, on a rounded plate filled with the theme's own
//! background ([`LightsLook`]). **In the title bar they are a modem's panel**
//! ([`draw_panel_led`], [`draw_panel_plate`]): each lamp stands over its
//! letters in a socket, on one bevelled plate with baud's logo, the plate's
//! numbers the `PANEL_*` constants. **On their own line** each light's
//! letters are to its left, on a small plate ([`draw_led`], [`draw_plate`]).
//! **Every lamp is one of the theme's sixteen colours, exactly** -- the lit
//! dot, the unlit one and an unread light's ring each an entry of
//! `Theme::palette[0..16]` as baud holds it, never lifted or mixed -- so
//! the lights are drawn in the colours the terminal draws its text in. Which
//! entry each light takes is a table compiled in, by theme half
//! ([`LightsColours`], `[window] activity_lights_colours`): `"meaning"`,
//! baud's own, one hue a kind; `"iec"`, IEC 60073's
//! meanings for an indicator's colour; or `"modem"`, every lamp red, as the
//! front panels' LEDs were. On the plate, which is the theme's background
//! and opaque, a lamp reads against one colour whatever the window's
//! material shows. Colour repeats what the letters and the place already
//! say ("Avoid relying solely on color", `[[source: apple-hig-color]]`),
//! and lit and unlit differ by size as well: an unlit lamp is a dot half
//! the lit one's diameter, so the state reads without its colour (WCAG
//! 2.2's SC 1.4.1, `[[source: wcag22]]`). A light baud may not read is
//! drawn as a ring ([`Light::Unread`]), neither lit nor dark (note AH,
//! Q8). CS is the modem's sense, lit while the line is clear: lit from the
//! first frame while the program takes what baud sends, and dark while
//! bytes are owed it that the kernel has not taken. A change is one step,
//! as an LED's is.
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
//! below, and is no row. Its plate and its lights are the line's records,
//! so every path draws them, and nothing in them reads the system: the
//! line on macOS is byte for byte the line on Linux. The pictures go to
//! the colour sheet once a look, and a light that changes names another.
//! The clock and the caps below are the title strip's.
//!
//! **The glow** is a halo of the lamp's own colour round a lit dot,
//! starting a point off its edge so the pixel touching the dot is the plate,
//! or on the panel the socket, and the dot keeps its contrast against it,
//! its strength `[window] activity_lights_glow` ([`Halo`]). **Dim Flashing
//! Lights** draws no glow and the dot as it is ([`Glow::Dimmed`]): a lamp
//! stays its palette entry, and the strip meets WCAG 2.3.1 by the area
//! clause, its changed pixels far under a quarter of the field (note AH, the
//! exact-palette section).

use crate::atlas::outline;
use crate::color::Argb;
use crate::color::Backdrop;
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
/// * **CS**, Clear to Send: the Courier's is "ON until the modem lowers
///   CTS"; baud's is lit while the line is clear and dark while bytes are
///   owed the program that the kernel has not taken -- it is not reading
///   its input fast enough, or has gone ([`Led::at_rest`]).
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
    /// stand for, and what lights it in baud, in plain words -- over the
    /// strip's light and over its word on DEC's indicator line alike
    /// (`tip::tip_look`), so CD's and OH's name both forms of the state
    /// baud cannot read.
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
                 connection up. A ring, or CD? on DEC's status line, when the system would not \
                 let baud look at that program. On Linux baud has no way to look yet: the light \
                 stays dark and DEC's status line shows CD?."
            }
            Led::Oh => {
                "OH -- Off Hook: lit while a program other than the shell is in front. A ring, \
                 or OH? on DEC's status line, when baud cannot look at the shell: the system \
                 would not let it."
            }
            Led::Rd => "RD -- Receive Data: flashes when baud receives data from the program.",
            Led::Sd => {
                "SD -- Send Data: flashes when baud sends data to the program: your keys, a \
                 paste, the terminal's answers."
            }
            Led::Cs => {
                "CS -- Clear to Send: lit while the program takes what baud sends it, your keys \
                 and the terminal's answers. It goes out while baud is holding bytes the \
                 program has not taken, because it is not reading its input or has gone."
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

    /// **What the light says of the line**: RD and HS bring data in, SD
    /// and AA send it out and CS says the way out is clear, and the rest
    /// show a state.
    pub const fn direction(self) -> Direction {
        match self {
            Led::Rd | Led::Hs => Direction::In,
            Led::Sd | Led::Aa | Led::Cs => Direction::Out,
            Led::Cd | Led::Oh | Led::Fax | Led::Syn => Direction::State,
        }
    }

    /// **What the light shows while nothing is seen**: dark, but CS lit.
    /// A modem's CS is "ON until the modem lowers CTS"
    /// (`[[source: usr-courier-hs-1988]]`, PDF p.97), so what CS's clock is
    /// told of is the line closing -- bytes owed the program that the
    /// kernel has not taken -- and it shows dark for that.
    pub const fn at_rest(self) -> Light {
        match self {
            Led::Cs => Light::Lit,
            _ => Light::Dark,
        }
    }

    /// **What the light shows while its thing is seen**: the other of
    /// [`Led::at_rest`]'s two.
    pub const fn when_seen(self) -> Light {
        match self.at_rest() {
            Light::Lit => Light::Dark,
            Light::Dark | Light::Unread => Light::Lit,
        }
    }
}

/// **The read that is full at the platform's ceiling**: Darwin's tty hands
/// back at most `TTYCLSIZE`, "1024", a read (`[[source: xnu-tty-h]]`
/// 104-105), 49,152 reads for 48 MiB, and Linux's at most
/// `N_TTY_BUF_SIZE` less one, 4095 (`[[source: linux-n-tty]]`). A read
/// that long is HS's
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

    /// **Every light at rest**, as nothing seen leaves it: CS lit, the rest
    /// dark ([`Led::at_rest`]).
    pub fn rest() -> Shown {
        ByLed::of(Led::at_rest)
    }

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

/// How a lit light is drawn: with its glow, or under Dim Flashing Lights
/// without it. The dot is its palette entry either way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glow {
    Full,
    Dimmed,
}

/// **One of the theme's sixteen ANSI colours**, by its place in the
/// palette: ECMA-48's eight for SGR 30-37, "black, red, green, yellow,
/// blue, magenta, cyan, white" (`[[source: ecma48]]` 8.3.117), and the
/// bright eight xterm puts after them for SGR 90-97 (`[[source:
/// ctlseqs]]`). What every lamp is drawn in, exactly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ansi {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl Ansi {
    /// The sixteen, in the palette's order.
    pub const ALL: [Ansi; 16] = [
        Ansi::Black,
        Ansi::Red,
        Ansi::Green,
        Ansi::Yellow,
        Ansi::Blue,
        Ansi::Magenta,
        Ansi::Cyan,
        Ansi::White,
        Ansi::BrightBlack,
        Ansi::BrightRed,
        Ansi::BrightGreen,
        Ansi::BrightYellow,
        Ansi::BrightBlue,
        Ansi::BrightMagenta,
        Ansi::BrightCyan,
        Ansi::BrightWhite,
    ];

    /// **The entry's colour in `palette`**, as the theme holds it, opaque.
    pub fn of(self, palette: &[Argb; 256]) -> Argb {
        let at = match self {
            Ansi::Black => 0,
            Ansi::Red => 1,
            Ansi::Green => 2,
            Ansi::Yellow => 3,
            Ansi::Blue => 4,
            Ansi::Magenta => 5,
            Ansi::Cyan => 6,
            Ansi::White => 7,
            Ansi::BrightBlack => 8,
            Ansi::BrightRed => 9,
            Ansi::BrightGreen => 10,
            Ansi::BrightYellow => 11,
            Ansi::BrightBlue => 12,
            Ansi::BrightMagenta => 13,
            Ansi::BrightCyan => 14,
            Ansi::BrightWhite => 15,
        };
        palette[at].with_alpha(Chan::from_u8(0xFF))
    }
}

/// **Which half of a theme a palette is**: a dark page or a light one,
/// from its default background against its default foreground. The
/// meaning set gives each half its own entries, a theme's normal colours
/// being built for its own page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Half {
    Dark,
    Light,
}

impl Half {
    /// The half whose page `bg` is, against text `fg`: dark where the page
    /// is the darker of the two.
    pub fn of(fg: Argb, bg: Argb) -> Half {
        match crate::color::darker(bg, fg) {
            true => Half::Dark,
            false => Half::Light,
        }
    }
}

/// **Which palette entry each light is lit in**: `[window]
/// activity_lights_colours`, a table of the sixteen a light, compiled in
/// ([`LightsColours::entries`]), so the runtime only indexes the theme.
///
/// * **`"meaning"`**, the default, baud's own: one hue a kind of thing,
///   and CS green, the colour of a clear line, which it is nearly all of a
///   session. On a dark page HS 10, AA 11, CD 4, OH 6, RD 2, SD 3, CS 2,
///   FAX 5, SYN 13; on a light one no yellow, whose normal entry a light
///   theme darkens toward brown -- HS 10, AA 6, CD 4, OH 12, RD 2, SD 14,
///   CS 2, FAX 5, SYN 13.
/// * **`"iec"`**: the colours IEC 60073:2002 gives an indicator's meaning
///   (Tables 7 and 8: green normal, yellow abnormal, white general
///   information and status), taken to the palette -- HS, CD and CS 2, CS
///   lit being the line's normal condition, FAX and SYN 6, the rest 7. The
///   standard is not among the sources the repository holds; note AH names
///   the clauses.
/// * **`"modem"`**: every lamp entry 1, red, as the Courier's and the
///   Hayes's front-panel LEDs were.
///
/// On the plate, the theme's own background, every set's normal entries
/// read at 3:1 on the built-in themes, so no set takes a bright entry for
/// that (note AH).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LightsColours {
    #[default]
    Meaning,
    Iec,
    Modem,
}

/// The meaning set on a dark page.
const MEANING_DARK: ByLed<Ansi> = ByLed {
    hs: Ansi::BrightGreen,
    aa: Ansi::BrightYellow,
    cd: Ansi::Blue,
    oh: Ansi::Cyan,
    rd: Ansi::Green,
    sd: Ansi::Yellow,
    cs: Ansi::Green,
    fax: Ansi::Magenta,
    syn: Ansi::BrightMagenta,
};

/// The meaning set on a light page: no yellow.
const MEANING_LIGHT: ByLed<Ansi> = ByLed {
    hs: Ansi::BrightGreen,
    aa: Ansi::Cyan,
    cd: Ansi::Blue,
    oh: Ansi::BrightBlue,
    rd: Ansi::Green,
    sd: Ansi::BrightCyan,
    cs: Ansi::Green,
    fax: Ansi::Magenta,
    syn: Ansi::BrightMagenta,
};

/// IEC 60073's set, the same on either page.
const IEC: ByLed<Ansi> = ByLed {
    hs: Ansi::Green,
    aa: Ansi::White,
    cd: Ansi::Green,
    oh: Ansi::White,
    rd: Ansi::White,
    sd: Ansi::White,
    cs: Ansi::Green,
    fax: Ansi::Cyan,
    syn: Ansi::Cyan,
};

/// The front panels' set: every lamp red.
const MODEM: ByLed<Ansi> = ByLed {
    hs: Ansi::Red,
    aa: Ansi::Red,
    cd: Ansi::Red,
    oh: Ansi::Red,
    rd: Ansi::Red,
    sd: Ansi::Red,
    cs: Ansi::Red,
    fax: Ansi::Red,
    syn: Ansi::Red,
};

impl LightsColours {
    /// The three, in the order the config names them.
    pub const ALL: [LightsColours; 3] = [
        LightsColours::Meaning,
        LightsColours::Iec,
        LightsColours::Modem,
    ];

    /// The name `[window] activity_lights_colours` takes.
    pub const fn name(self) -> &'static str {
        match self {
            LightsColours::Meaning => "meaning",
            LightsColours::Iec => "iec",
            LightsColours::Modem => "modem",
        }
    }

    /// The set named `name`, or none.
    pub fn named(name: &str) -> Option<LightsColours> {
        LightsColours::ALL.into_iter().find(|s| s.name() == name)
    }

    /// **Each light's entry on a page of `half`**: the set's table.
    pub const fn entries(self, half: Half) -> ByLed<Ansi> {
        match (self, half) {
            (LightsColours::Meaning, Half::Dark) => MEANING_DARK,
            (LightsColours::Meaning, Half::Light) => MEANING_LIGHT,
            (LightsColours::Iec, _) => IEC,
            (LightsColours::Modem, _) => MODEM,
        }
    }
}

/// **How strongly a lit light glows**: `[window] activity_lights_glow`, 0
/// to 1, held as the halo's alpha where it starts. 1 is a halo of 0.8 at
/// its start; 0 draws none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Halo(Chan);

/// The glow's strength by default: under half the strongest.
pub const DEFAULT_GLOW: f32 = 0.4;

/// The halo's alpha where it starts, at strength 1.
const HALO_PEAK: f32 = 0.8;

/// **The halo's reach past the dot, and its gap off the dot**, in points:
/// it starts a point out, so the pixel touching the dot is the plate and
/// the dot reads against the plate as it does with no glow, and fades as
/// `(1 - t)^1.6` to nothing three points out, or at the picture's edge.
pub const HALO: f32 = 3.0;
pub const HALO_GAP: f32 = 1.0;

impl Halo {
    /// The halo of `strength`, held to 0..1.
    pub fn of(strength: f32) -> Halo {
        Halo(Chan::from_unit(HALO_PEAK * strength.clamp(0.0, 1.0)))
    }

    /// Its alpha where it starts.
    pub fn alpha(self) -> Chan {
        self.0
    }
}

impl Default for Halo {
    fn default() -> Halo {
        Halo::of(DEFAULT_GLOW)
    }
}

/// **The lights' colours**, straight alpha: every lamp exactly a palette
/// entry, the plate the theme's background and the letters its foreground.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LightsLook {
    /// Each light's lit colour: its set's entry.
    pub lit: ByLed<Argb>,
    /// Every light's colour while dark, drawn as a dot half the lit one's
    /// diameter: entry 8, in every set and on every theme.
    pub unlit: Argb,
    /// An unread light's ring: the entry the foreground is, else 7.
    pub ring: Argb,
    /// The letters: the theme's foreground, lifted only where it would read
    /// under [`TEXT_CONTRAST`] against the plate.
    pub label: Argb,
    /// The plate under the lights: the theme's background, opaque.
    pub plate: Argb,
    /// The socket behind each lamp on the title bar's panel, from the plate.
    pub socket: Argb,
    /// The panel plate's bevel, from the plate.
    pub bevel: Bevel,
    /// Whether a lit light glows, and how strongly.
    pub glow: Glow,
    pub halo: Halo,
}

impl LightsLook {
    /// **The look of a theme**: its sixteen entries `palette`, its
    /// foreground `fg` and background `bg`, lit in `set` by the half `bg`
    /// is, glowing `halo` at `glow`. The theme's own colours, not the
    /// screen's: reverse video turns the program's page, not baud's plate.
    pub fn of(
        palette: &[Argb; 256],
        fg: Argb,
        bg: Argb,
        set: LightsColours,
        glow: Glow,
        halo: Halo,
    ) -> LightsLook {
        let opaque = Chan::from_u8(0xFF);
        let (fg, plate) = (fg.with_alpha(opaque), bg.with_alpha(opaque));
        let entries = set.entries(Half::of(fg, plate));
        let lit = ByLed::of(|l| entries.get(l).of(palette));
        let unlit = Ansi::BrightBlack.of(palette);
        let ring = ring_entry(palette, fg).of(palette);
        let label = crate::color::reads_at(fg, plate, opaque, Backdrop::any(), TEXT_CONTRAST);
        LightsLook {
            lit,
            unlit,
            ring,
            label,
            plate,
            socket: socket_of(plate),
            bevel: Bevel::of(plate),
            glow,
            halo,
        }
    }
}

/// **The entry an unread light's ring is drawn in**: the one the theme's
/// foreground `fg` is, where one is, else 7, white.
pub fn ring_entry(palette: &[Argb; 256], fg: Argb) -> Ansi {
    Ansi::ALL
        .into_iter()
        .find(|a| a.of(palette) == fg.with_alpha(Chan::from_u8(0xFF)))
        .unwrap_or(Ansi::White)
}

/// **The panel plate's bevel**, three colours with alpha over the plate
/// and what it stands on ([`draw_panel_plate`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bevel {
    /// The edge round the plate, just outside it.
    pub edge: Argb,
    /// The highlight along the plate's top.
    pub top: Argb,
    /// The line along the plate's bottom.
    pub bottom: Argb,
}

/// The built-in dark theme's page and the built-in light one's, in luma:
/// the two ends of what a bevel and a socket were drawn for
/// ([`page_lightness`]).
const DARK_PAGE: f32 = 16.0;
const LIGHT_PAGE: f32 = 239.0;

/// **Where `plate` stands between the dark page and the light one**, 0 to
/// 1, by its luma rounded to a whole level, so a grey page's luma is its
/// grey: a page past either end is held to it, so #000000 draws as #101010
/// does and #FFFFFF as #EFEFEF.
fn page_lightness(plate: Argb) -> f32 {
    let of = |c: Chan| f32::from(c.get());
    let luma = (0.299 * of(plate.r()) + 0.587 * of(plate.g()) + 0.114 * of(plate.b())).round();
    (luma.clamp(DARK_PAGE, LIGHT_PAGE) - DARK_PAGE) / (LIGHT_PAGE - DARK_PAGE)
}

/// A percentage at each end of the pages, as an alpha at `t` between them.
fn page_alpha(t: f32, dark: f32, light: f32) -> Chan {
    Chan::from_unit((dark + (light - dark) * t) / 100.0)
}

impl Bevel {
    /// **The bevel on `plate`**: black at 55% for the edge, white at 13%
    /// for the highlight and black at 80% for the bottom line on the dark
    /// page; 28%, 95% and 22% on the light one; between them by
    /// [`page_lightness`], so on any page the highlight is lighter than
    /// the plate and the two lines darker, as far as the page has room.
    /// White on #FFFFFF and black on #000000 cannot show, and the other
    /// two lines there still do.
    pub fn of(plate: Argb) -> Bevel {
        let t = page_lightness(plate);
        let black = Argb(0xFF_00_00_00);
        let white = Argb(0xFF_FF_FF_FF);
        Bevel {
            edge: black.with_alpha(page_alpha(t, 55.0, 28.0)),
            top: white.with_alpha(page_alpha(t, 13.0, 95.0)),
            bottom: black.with_alpha(page_alpha(t, 80.0, 22.0)),
        }
    }
}

/// **The socket on `plate`**: the plate, every channel lowered by a step,
/// so it reads as a recess in the page and keeps the page's hue. The step
/// is 11 levels on the dark page and 21 on the light, which are #050505
/// and #DADADA, and between them by [`page_lightness`]. A plate too dark
/// to lower by a step, #000000 and its near neighbours, is raised by it
/// instead, or the socket would not show at all.
fn socket_of(plate: Argb) -> Argb {
    let step = (11.0 + 10.0 * page_lightness(plate)).round();
    let of = |c: Chan| f32::from(c.get());
    let most = of(plate.r()).max(of(plate.g())).max(of(plate.b()));
    let shift = |c: Chan| {
        let level = match most < step {
            true => of(c) + step,
            false => of(c) - step,
        };
        Chan::from_unit(level.clamp(0.0, 255.0) / 255.0)
    };
    Argb::rgb(shift(plate.r()), shift(plate.g()), shift(plate.b())).with_alpha(plate.a())
}

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

/// **A light's picture's width, in points**: the light and the halo's
/// reach past its right edge, [`HALO`]; the gap to the next light,
/// [`PAIR`], is wider, so no two pictures overlap.
pub fn picture_width(face: &Face<'_>, led: Led) -> f32 {
    cell_width(face, led) + HALO
}

/// **Where a light sits in a strip drawn at a scale**, in device pixels
/// from the strip's left: its start, rounded to a whole pixel so a light
/// drawn on its own lands on the same pixels as in the strip, and its
/// picture's width ([`picture_width`]), rounded up.
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
        let start = u32::try_from((x * scale).round() as i64).ok()?;
        let width = u32::try_from((picture_width(face, led) * scale).ceil() as i64).ok()?;
        *out.get_mut(led) = Slot {
            start: Px::of(start),
            width: Px::of(width),
        };
        end = Px::of(start.checked_add(width)?);
        x += cell_width(face, led) + PAIR;
    }
    Some((out, end))
}

/// **The width the strip takes, in points**, in `face`: every light, the
/// gaps between them and the last one's halo.
pub fn width(face: &Face<'_>) -> f32 {
    let lights: f32 = Led::ALL.iter().map(|&l| cell_width(face, l)).sum();
    lights + PAIR * (Led::ALL.len() - 1) as f32 + HALO
}

/// **The space above and below the lights on their own line**, in
/// points: the line is the picture and this, and no taller.
pub const LINE_MARGIN: f32 = 2.0;
/// **The space the line keeps at its right end**, in points, as the title
/// strip keeps a gap at its own; a light that would start nearer the left
/// end than this is left out.
pub const LINE_END: f32 = 9.0;

/// **How far the halo reaches past the dot on the lights' own line**, in
/// points: its gap and the first point of its fade, two of the three the
/// title strip's taller pictures draw, so a lit lamp glows on the line and
/// the line stays as low as an indicator line is.
pub const LINE_HALO: f32 = HALO_GAP + 1.0;

/// **How tall the lights' picture is on their own line, in points**: the
/// tallest thing drawn about a light's centre -- the dot and the halo the
/// line draws past it ([`LINE_HALO`]), or the letters' caps, which have no
/// descender -- twice over, since [`draw_led`] centres both on the
/// picture, rounded up to a whole point. In points and never from the
/// cell, so a large font does not enlarge the lights and the area clause
/// WCAG 2.3.1's threshold rests on holds at every size.
pub fn line_picture() -> f32 {
    let above = (DOT / 2.0 + LINE_HALO).max(CAP * TEXT / 2.0);
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

/// The contrast a lit light reads at against its plate, at least: SC
/// 1.4.11's 3:1 for "Visual information required to identify user
/// interface components and states" (`[[source: wcag22]]`). Every set's
/// lamps on the built-in themes' plates reach it; a theme of the user's is
/// measured, not lifted (note AH).
pub const LIT_CONTRAST: f64 = 3.0;

/// The contrast the letters read at against the plate, at least: SC
/// 1.4.3's 4.5:1, "The visual presentation of text ... has a contrast
/// ratio of at least 4.5:1", which drops to 3:1 only for large text, 18
/// point or 14 point bold (`[[source: wcag22]]`). The letters are 8 point
/// bold ([`TEXT`]). Apple's table, which it says is "from WCAG Level AA",
/// gives 3:1 for "bold at any size" (`[[source: apple-hig-accessibility]]`);
/// the WCAG text it cites governs.
pub const TEXT_CONTRAST: f64 = 4.5;

/// **The plate**, in points: how far it reaches past the first and last
/// light, its height in the title strip, how far it is inset from the top
/// and bottom of the lights' own line, and its corners' radii there and on
/// the line.
pub const PLATE_PAD: f32 = 4.0;
pub const PLATE_TITLE: f32 = 20.0;
pub const PLATE_INSET: f32 = 1.0;
pub const PLATE_RADIUS_TITLE: f32 = 5.0;
pub const PLATE_RADIUS_LINE: f32 = 4.0;

/// **Where the plate is under the lights placed at `xs`**, in the same
/// pixels as `xs`, each light `at`'s width: from [`PLATE_PAD`] before the
/// first placed light to [`PLATE_PAD`] past the last, its left and width;
/// `None` with no light placed.
pub fn plate_span(at: &ByLed<Slot>, xs: &ByLed<Option<Px>>, scale: f32) -> Option<(Px, Px)> {
    let pad = u32::try_from((PLATE_PAD * scale).round() as i64).ok()?;
    let shown = || {
        Led::ALL
            .into_iter()
            .filter_map(|l| xs.get(l).map(|x| (l, x)))
    };
    let (_, first) = shown().next()?;
    let (last, x) = shown().next_back()?;
    let left = first.value().saturating_sub(pad);
    let right = x
        .value()
        .checked_add(at.get(last).width.value())?
        .checked_add(pad)?;
    Some((Px::of(left), Px::of(right.checked_sub(left)?)))
}

/// **The plate, drawn**: a rectangle `width` by `height` pixels with
/// corners of `radius` pixels, filled with the look's plate colour, into
/// `out`. `None` for a size no canvas holds.
pub fn draw_plate(
    look: &LightsLook,
    width: Px,
    height: Px,
    radius: f32,
    out: &mut LightsImage,
) -> Option<()> {
    let extent = Extent::new(width, height);
    let (x1, y1) = (slots(width) as f32, slots(height) as f32);
    let cover = coverage(extent, |c| rounded(c, (0.0, 0.0, x1, y1), radius))?;
    out.width = width;
    out.height = height;
    out.pixels.clear();
    out.pixels
        .extend(cover.iter().map(|&c| over(Argb(0), look.plate, c)));
    Some(())
}

/// A rectangle from `(x0, y0)` to `(x1, y1)` with corners of `radius`,
/// held to half its smaller side.
fn rounded<'c>(
    mut c: Canvas<'c>,
    (x0, y0, x1, y1): (f32, f32, f32, f32),
    radius: f32,
) -> Canvas<'c> {
    const KAPPA: f32 = 0.552_284_8;
    let rr = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    let k = KAPPA * rr;
    let p = Point::new;
    c.line(p(x0 + rr, y0), p(x1 - rr, y0));
    c.cubic(
        p(x1 - rr, y0),
        p(x1 - rr + k, y0),
        p(x1, y0 + rr - k),
        p(x1, y0 + rr),
    );
    c.line(p(x1, y0 + rr), p(x1, y1 - rr));
    c.cubic(
        p(x1, y1 - rr),
        p(x1, y1 - rr + k),
        p(x1 - rr + k, y1),
        p(x1 - rr, y1),
    );
    c.line(p(x1 - rr, y1), p(x0 + rr, y1));
    c.cubic(
        p(x0 + rr, y1),
        p(x0 + rr - k, y1),
        p(x0, y1 - rr + k),
        p(x0, y1 - rr),
    );
    c.line(p(x0, y1 - rr), p(x0, y0 + rr));
    c.cubic(
        p(x0, y0 + rr),
        p(x0, y0 + rr - k),
        p(x0 + rr - k, y0),
        p(x0 + rr, y0),
    );
    c
}

/// **The title bar's panel**, in points, of the 28-point strip it was
/// drawn for ([`PANEL_STRIP`]): the plate 24 tall and a whole device pixel
/// at each end, [`PANEL_RIGHT`] in from the window's right edge, its
/// corners [`PANEL_RADIUS`], [`PANEL_PAD`] from either end to the first
/// thing on it; [`PANEL_LOGO_GAP`] from the logo to the first lamp's
/// letters; [`PANEL_LETTER_GAP`] from one lamp's letters to the next's;
/// each lamp's centre [`PANEL_LAMP_Y`] down the strip and its letters'
/// baseline [`PANEL_BASELINE`]; [`PANEL_SOCKET`] the socket's radius, and
/// [`PANEL_BEVEL`] the bevel's three lines across.
pub const PANEL_STRIP: f32 = 28.0;
pub const PANEL_HEIGHT: f32 = 24.0;
pub const PANEL_RIGHT: f32 = 5.0;
pub const PANEL_RADIUS: f32 = 5.0;
pub const PANEL_PAD: f32 = 7.0;
pub const PANEL_LOGO_GAP: f32 = 10.0;
pub const PANEL_LETTER_GAP: f32 = 6.2;
pub const PANEL_LAMP_Y: f32 = 8.5;
pub const PANEL_BASELINE: f32 = 22.0;
pub const PANEL_SOCKET: f32 = 3.9;
pub const PANEL_BEVEL: f32 = 0.5;

/// **A lamp's letters on the panel**, in points from the left of the first
/// lamp's column, as they are before any rounding: their left and their
/// width. A column is the letters and half the letter gap at each side, so
/// the columns lie side by side with nothing between them.
pub fn panel_letters(face: &Face<'_>) -> ByLed<(f32, f32)> {
    let mut x = PANEL_LETTER_GAP / 2.0;
    let mut out = ByLed::of(|_| (0.0, 0.0));
    for led in Led::ALL {
        let w = advance(face, led.letters(), TEXT);
        *out.get_mut(led) = (x, w);
        x += w + PANEL_LETTER_GAP;
    }
    out
}

/// **Each lamp's column on the panel at `scale`, in device pixels**, and
/// their whole width: each starts on the whole pixel nearest its place and
/// ends where the next starts, so a point in the strip is over one lamp and
/// no more, and its picture holds its halo, which reaches 6.5 points from
/// the lamp's centre against the 7.9 a two-letter column gives it. `None`
/// for a size no pixel count holds.
pub fn panel_layout(face: &Face<'_>, scale: f32) -> Option<(ByLed<Slot>, Px)> {
    let letters = panel_letters(face);
    let px = |pt: f32| u32::try_from((pt * scale).round() as i64).ok();
    let mut out = ByLed::of(|_| Slot {
        start: Px::of(0),
        width: Px::of(0),
    });
    let mut end = 0;
    for led in Led::ALL {
        let (x, w) = *letters.get(led);
        let start = px(x - PANEL_LETTER_GAP / 2.0)?;
        end = px(x + w + PANEL_LETTER_GAP / 2.0)?;
        *out.get_mut(led) = Slot {
            start: Px::of(start),
            width: Px::of(end.checked_sub(start)?),
        };
    }
    Some((out, Px::of(end)))
}

/// **One light on the panel, drawn** in `face` at `scale` device pixels a
/// point, `height` points tall, into `out`, a picture its column of
/// [`panel_layout`] across: the socket, then, while lit and not under Dim
/// Flashing Lights, the halo, then the lamp, its left edge on a whole
/// device pixel and centred over its letters to within half of one, and
/// its letters under it. The lamps are the same as [`draw_led`]'s: a lit
/// light the full dot in its lit colour, a dark one a dot half that
/// diameter in the unlit entry, an unread one a ring in the ring's entry,
/// each of its palette entry exactly where it covers a pixel fully. The
/// numbers are the 28-point strip's, centred on a strip of any other
/// height. `None` for a size no canvas holds.
pub fn draw_panel_led(
    face: &Face<'_>,
    look: &LightsLook,
    led: Led,
    light: Light,
    scale: f32,
    height: f32,
    out: &mut LightsImage,
) -> Option<()> {
    let (slots_at, _) = panel_layout(face, scale)?;
    let slot = *slots_at.get(led);
    let (x, across) = *panel_letters(face).get(led);
    let w = slot.width;
    let h = Px::of(u32::try_from((height * scale).round() as i64).ok()?);
    let extent = Extent::new(w, h);
    let off = (height - PANEL_STRIP) / 2.0;
    let r = DOT * scale / 2.0;
    let mid = (x + across / 2.0) * scale - slots(slot.start) as f32;
    let cx = (mid - r).round() + r;
    let cy = (PANEL_LAMP_Y + off) * scale;
    let centre = Point::new(cx, cy);
    let px = TEXT * scale;
    let run = advance(face, led.letters(), TEXT) * scale;
    let pen = Point::new(cx - run / 2.0, (PANEL_BASELINE + off) * scale);
    let text = coverage(extent, |c| letters(face, c, led.letters(), px, pen))?;
    out.width = w;
    out.height = h;
    out.pixels.clear();
    out.pixels.resize(slots(w) * slots(h), Argb(0));
    for (p, &c) in out.pixels.iter_mut().zip(&text) {
        *p = over(*p, look.label, c);
    }
    let socket = disc_coverage(extent, centre, PANEL_SOCKET * scale, None)?;
    for (p, &c) in out.pixels.iter_mut().zip(&socket) {
        *p = over(*p, look.socket, c);
    }
    lamp(
        out,
        extent,
        look,
        led,
        light,
        centre,
        HALO.min(PANEL_LAMP_Y + off - DOT / 2.0),
        scale,
    )
}

/// **How far the panel's picture reaches past its plate at `scale`**, in
/// device pixels at each side: the bevel's edge is outside the plate.
pub fn panel_margin(scale: f32) -> Option<Px> {
    u32::try_from((PANEL_BEVEL * scale).ceil() as i64)
        .ok()
        .map(Px::of)
}

/// **The panel's plate, drawn** for a plate `width` by `height` device
/// pixels, into `out`, a picture [`panel_margin`] larger at every side:
/// the edge just outside the plate following its corners, the plate in the
/// look's colour with [`PANEL_RADIUS`]'s corners, the highlight inside its
/// top and the line inside its bottom, each [`PANEL_BEVEL`] across and
/// stopping where the corners start. `None` for a size no canvas holds.
pub fn draw_panel_plate(
    look: &LightsLook,
    width: Px,
    height: Px,
    scale: f32,
    out: &mut LightsImage,
) -> Option<()> {
    let m = panel_margin(scale)?;
    let two = m.value().checked_mul(2)?;
    let w = Px::of(width.value().checked_add(two)?);
    let h = Px::of(height.value().checked_add(two)?);
    let extent = Extent::new(w, h);
    let inset = slots(m) as f32;
    let (x0, y0) = (inset, inset);
    let (x1, y1) = (x0 + slots(width) as f32, y0 + slots(height) as f32);
    let (radius, bevel) = (PANEL_RADIUS * scale, PANEL_BEVEL * scale);
    let edge = coverage(extent, |c| {
        rounded(
            c,
            (x0 - bevel, y0 - bevel, x1 + bevel, y1 + bevel),
            radius + bevel,
        )
    })?;
    let plate = coverage(extent, |c| rounded(c, (x0, y0, x1, y1), radius))?;
    let top = coverage(extent, |c| {
        rounded(c, (x0 + radius, y0, x1 - radius, y0 + bevel), 0.0)
    })?;
    let bottom = coverage(extent, |c| {
        rounded(c, (x0 + radius, y1 - bevel, x1 - radius, y1), 0.0)
    })?;
    out.width = w;
    out.height = h;
    out.pixels.clear();
    out.pixels.extend((0..slots(w) * slots(h)).map(|i| {
        let p = over(Argb(0), look.bevel.edge, edge[i]);
        let p = over(p, look.plate, plate[i]);
        let p = over(p, look.bevel.top, top[i]);
        over(p, look.bevel.bottom, bottom[i])
    }));
    Some(())
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
/// `height` points tall, into `out`: its letters, then, while lit and not
/// under Dim Flashing Lights, the halo, then the light centred on the
/// strip and the letters' caps centred on it, in a picture its [`Slot`]'s
/// width. Every lamp pixel the dot or ring covers fully is its palette
/// entry exactly: a lit light the full dot in its lit colour, a dark one a
/// dot half that diameter in the unlit entry, an unread one a ring in the
/// ring's entry. `None` for a size no canvas holds.
pub fn draw_led(
    face: &Face<'_>,
    look: &LightsLook,
    led: Led,
    light: Light,
    scale: f32,
    height: f32,
    out: &mut LightsImage,
) -> Option<()> {
    let w = Px::of(u32::try_from((picture_width(face, led) * scale).ceil() as i64).ok()?);
    let h = Px::of(u32::try_from((height * scale).round() as i64).ok()?);
    let extent = Extent::new(w, h);
    let r = DOT * scale / 2.0;
    let mid = height * scale / 2.0;
    let px = TEXT * scale;
    let run = advance(face, led.letters(), TEXT) * scale;
    let pen = Point::new(0.0, mid + CAP * px / 2.0);
    let centre = Point::new(run + GAP * scale + r, mid);
    let text = coverage(extent, |c| letters(face, c, led.letters(), px, pen))?;
    out.width = w;
    out.height = h;
    out.pixels.clear();
    out.pixels.resize(slots(w) * slots(h), Argb(0));
    for (p, &c) in out.pixels.iter_mut().zip(&text) {
        *p = over(*p, look.label, c);
    }
    lamp(
        out,
        extent,
        look,
        led,
        light,
        centre,
        HALO.min(height / 2.0 - DOT / 2.0),
        scale,
    )
}

/// **A lamp over what `out` already holds**, about `centre`: while lit and
/// not under Dim Flashing Lights the halo, reaching `reach` points past
/// the dot, then the dot, the smaller dot or the ring its state is.
#[allow(clippy::too_many_arguments)]
fn lamp(
    out: &mut LightsImage,
    extent: Extent,
    look: &LightsLook,
    led: Led,
    light: Light,
    centre: Point,
    reach: f32,
    scale: f32,
) -> Option<()> {
    let r = DOT * scale / 2.0;
    let lit = *look.lit.get(led);
    if light == Light::Lit && look.glow == Glow::Full && look.halo.alpha().get() > 0 {
        halo(out, lit, look.halo, centre, r, reach, scale);
    }
    let (ink, cover) = match light {
        Light::Lit => (lit, disc_coverage(extent, centre, r, None)?),
        Light::Dark => (look.unlit, disc_coverage(extent, centre, r / 2.0, None)?),
        Light::Unread => (
            look.ring,
            disc_coverage(extent, centre, r, Some(r - RING * scale))?,
        ),
    };
    for (p, &c) in out.pixels.iter_mut().zip(&cover) {
        *p = over(*p, ink, c);
    }
    Some(())
}

/// **A line of small text's width, in points**, in `face` at `size`.
pub fn text_width(face: &Face<'_>, text: &str, size: f32) -> f32 {
    advance(face, text, size)
}

/// **A line of small text as a picture**, `height` points tall, in `ink`,
/// its capitals centred as a light's letters are: what the composed title
/// strip (`baud-app/src/film/chrome.rs`) shows the build's name with, where
/// there is no AppKit to ask for a label.
pub fn draw_text(
    face: &Face<'_>,
    text: &str,
    size: f32,
    ink: Argb,
    scale: f32,
    height: f32,
    out: &mut LightsImage,
) -> Option<()> {
    let w = Px::of(u32::try_from((advance(face, text, size) * scale).ceil() as i64).ok()?);
    let h = Px::of(u32::try_from((height * scale).round() as i64).ok()?);
    let extent = Extent::new(w, h);
    let px = size * scale;
    let pen = Point::new(0.0, height * scale / 2.0 + CAP * px / 2.0);
    let cover = coverage(extent, |c| letters(face, c, text, px, pen))?;
    out.width = w;
    out.height = h;
    out.pixels.clear();
    out.pixels.resize(slots(w) * slots(h), Argb(0));
    for (p, &c) in out.pixels.iter_mut().zip(&cover) {
        *p = over(*p, ink, c);
    }
    Some(())
}

/// A disc of radius `r` about `c` over `extent`, less the disc of `hole`
/// inside it where one is given.
fn disc_coverage(extent: Extent, c: Point, r: f32, hole: Option<f32>) -> Option<Vec<Chan>> {
    coverage(extent, |mut canvas| {
        disc(&mut canvas, c, r, Wind::Forward);
        if let Some(h) = hole {
            disc(&mut canvas, c, h, Wind::Back);
        }
        canvas
    })
}

/// **The halo round a lit dot**, into `out`: `colour` at the halo's alpha
/// from [`HALO_GAP`] off the dot's edge, fading as `(1 - t)^1.6` to
/// nothing `reach` points past it.
/// Never at full coverage, so no halo pixel is a lamp pixel; computed with
/// [`crate::det`]'s power, so the pixels are the same on every system.
fn halo(
    out: &mut LightsImage,
    colour: Argb,
    halo: Halo,
    centre: Point,
    r: f32,
    reach: f32,
    scale: f32,
) {
    use crate::det::Det;
    let reach = reach * scale;
    let gap = HALO_GAP * scale;
    if reach <= gap {
        return;
    }
    let a0 = f32::from(halo.alpha().get()) / 255.0;
    let w = slots(out.width).max(1);
    for (i, p) in out.pixels.iter_mut().enumerate() {
        let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
        let (dx, dy) = (x - centre.x, y - centre.y);
        let d = (dx * dx + dy * dy).sqrt() - r;
        if d < gap || d > reach {
            continue;
        }
        let t = ((d - gap) / (reach - gap)).clamp(0.0, 1.0);
        if t >= 1.0 {
            continue;
        }
        let a = a0 * (1.0 - t).det_powf(1.6);
        let cover = Chan::from_u8(Chan::from_unit(a).get().min(0xFE));
        *p = over(*p, colour, cover);
    }
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
    /// What it shows with nothing seen, and what while its thing is: dark
    /// and lit, but CS lit and dark ([`Led::at_rest`]).
    rest: Light,
    shows: Light,
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
    const fn new(led: Led) -> Lamp {
        Lamp {
            light: led.at_rest(),
            rest: led.at_rest(),
            shows: led.when_seen(),
            since: None,
            last: HostTime::ZERO,
            owed: Owed::Nothing,
            refused: None,
        }
    }

    fn seen(&mut self, at: HostTime) {
        self.last = self.last.max(at);
        if self.light != self.shows {
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
        let showing = self.light == self.shows;
        let want = match (self.owed, showing, held, refused) {
            (Owed::Seen, _, _, _) | (_, true, true, _) => self.shows,
            (_, _, _, true) => Light::Unread,
            _ => self.rest,
        };
        if want != self.light {
            self.light = want;
            self.since = Some(now);
        }
        if want == self.shows {
            self.owed = Owed::Nothing;
        }
    }
}

/// **Every light's clock**: told each time its thing is seen -- a read, a
/// write, a reply, a state found set at a tick, and for CS bytes owed --
/// and of each read that was refused, stepped on the display link's
/// clock. Each starts at rest, CS lit and the rest dark.
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
        Activity {
            lamps: ByLed {
                hs: Lamp::new(Led::Hs),
                aa: Lamp::new(Led::Aa),
                cd: Lamp::new(Led::Cd),
                oh: Lamp::new(Led::Oh),
                rd: Lamp::new(Led::Rd),
                sd: Lamp::new(Led::Sd),
                cs: Lamp::new(Led::Cs),
                fax: Lamp::new(Led::Fax),
                syn: Lamp::new(Led::Syn),
            },
        }
    }

    /// `led`'s thing was seen at `at`: bytes read or written, a reply
    /// queued, a state found set; for CS, bytes owed the program that the
    /// kernel has not taken, a write it refused any of or an outbox not
    /// yet empty.
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
                    (lamp.light == lamp.shows).then(|| lamp.last.plus(t.hold)),
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
/// set they are lit in and how strongly they glow -- the title strip's
/// too -- the looks made so far, and this frame's records -- the plate,
/// then one a placed light, each naming its picture on the colour sheet,
/// in the line's own pixels -- with what the CPU path last painted there.
pub(crate) struct LineState {
    pub(crate) line: Option<Line>,
    pub(crate) shown: Shown,
    pub(crate) glow: Glow,
    pub(crate) colours: LightsColours,
    pub(crate) halo: Halo,
    looks: Vec<(LookKey, Epoch)>,
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

/// What a look on the line is drawn from: the look, every colour of it
/// the theme's, and the scale the pictures are drawn at. Nothing of the
/// window's material: the plate is opaque, so no pixel of the line depends
/// on what is behind it, on any system.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct LookKey {
    pub(crate) look: LightsLook,
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
            colours: LightsColours::default(),
            halo: Halo::default(),
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
    /// kept now. Kept to the last few, which a theme and its reverse,
    /// both glows and two scales fill.
    fn look(&mut self, key: LookKey) -> (LightsLook, Epoch) {
        if let Some((k, e)) = self.looks.iter().find(|(k, _)| *k == key) {
            return (k.look, *e);
        }
        let epoch = Epoch(self.looks.last().map_or(0, |(_, e)| e.0 + 1));
        if self.looks.len() >= LOOKS_KEPT {
            self.looks.remove(0);
        }
        self.looks.push((key, epoch));
        (key.look, epoch)
    }

    /// **This frame's records**: the plate under the lights placed by
    /// [`line_places`] on a line `width` pixels wide, then each light at
    /// its state, its picture the one the colour sheet holds for this
    /// look. Every picture of a placed light -- dark, lit and unread -- is
    /// drawn there the first frame of a look
    /// ([`crate::atlas::Atlas::picture`]), so a light that changes names
    /// another picture and draws and uploads nothing; the plate once a
    /// look and width, which only a light dropped from a narrow line moves.
    pub(crate) fn place(
        &mut self,
        face: &Face<'_>,
        atlas: &mut crate::atlas::Atlas,
        key: LookKey,
        width: Px,
    ) {
        self.inst.clear();
        self.places = ByLed::default();
        let Some(line) = self.line else {
            return;
        };
        let (look, epoch) = self.look(key);
        let Some((at, strip)) = layout(face, line.scale) else {
            return;
        };
        let end = Px::of(u32::try_from((LINE_END * line.scale).round() as i64).unwrap_or(0));
        let xs = line_places(width, &at, strip, end);
        let height = line_picture();
        let screen = |v: u32| crate::instance::Screen(i16::try_from(v).unwrap_or(i16::MAX));
        let inset = u32::try_from((PLATE_INSET * line.scale).round() as i64).unwrap_or(0);
        let plate_h = Px::of(
            line.rows
                .height()
                .value()
                .saturating_sub(inset.saturating_mul(2)),
        );
        if let Some((x, w)) = plate_span(&at, &xs, line.scale) {
            let radius = PLATE_RADIUS_LINE * line.scale;
            let slot = atlas.picture(plate_key(epoch, w), || {
                let mut img = LightsImage::new();
                draw_plate(&look, w, plate_h, radius, &mut img)?;
                let px = img.pixels.iter().map(|p| p.0).collect();
                Some((px, img.width, img.height))
            });
            if let Some(slot) = slot {
                self.inst.push(crate::instance::Instance {
                    x: screen(x.value()),
                    y: screen(inset),
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

/// How many looks the line keeps: two themes, under either glow, at two
/// scales.
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

/// The key the line's plate `width` pixels wide is kept under for a look:
/// a tag of its own, the look's epoch and the width.
fn plate_key(epoch: Epoch, width: Px) -> crate::atlas::PictureKey {
    let w = u64::from(width.value()) & 0xFF_FFFF;
    crate::atlas::PictureKey((0x50 << 56) | ((epoch.0 & 0xFF_FFFF_FFFF) << 24) | w)
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
