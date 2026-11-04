//! **The visual bell a `BEL` rings**: what a frame shows of it, in the
//! colour a program set with kitty's `OSC 21 ; visual_bell=` or, with none
//! set, as the other video; and how often it may flash.
//!
//! **A colour is drawn as kitty draws its visual bell** (`[[source:
//! kitty-6d5d0c4-shaders]]` 1111-1169): one solid over the whole frame, in
//! the bell's colour at an alpha of the flash's intensity times 0.4, or
//! times 0.6 when the colour's largest channel is above 0.45 in linear light
//! ("light color"). The intensity rises from 0 to 1 and falls back to 0,
//! each half under CSS's `ease-in-out`, cubic-bezier(0.42, 0, 0.58, 1)
//! (`[[source: kitty-6d5d0c4-animation-h]]` 20, the curve kitty's
//! `get_visual_bell_intensity()` builds), over the bell's length: "The flash
//! is animated, fading in and out over the specified duration"
//! (`[[source: kitty-6d5d0c4-options-definition]]` 1450-1464). kitty's code
//! runs the whole fade inside half of its configured duration (`duration =
//! OPT(visual_bell_duration) / 2`, and the bell over once the progress
//! passes it), a departure from its own text not followed. The length is
//! the one baud's other-video flash is shown for, the entry's `flash`
//! padding (`baud-app`'s `bell::LENGTH`), since kitty's own default length
//! is zero, its bell off.
//!
//! **The solid is mixed as the frame mixes every solid**, in the frame's
//! [`crate::Blend`]: in linear light under [`crate::Blend::Linear`], as
//! kitty's layered path blends its tint, and in encoded numbers under the
//! default, a departure in how the colour mixes, not in its alpha.
//!
//! **A bell rung faster than WCAG 2.3.1 allows flashes no faster**: "no
//! more than three general flashes and / or no more than three red flashes
//! within any one-second period" (`[[source: wcag22]]`), and a bell is one
//! flash, a pair of opposing changes, wherever its colour and the page
//! differ enough. kitty restarts the fade at each `BEL` (`screen_bell()`,
//! `[[source: kitty-6d5d0c4-screen]]` 3146-3158), so a bell rung every
//! frame dips to nothing and rises again each frame: a flash a frame
//! interval. Here **a ring while the flash rises or holds its peak holds it
//! there** from the ring for half the length, so bells closer than that are
//! one flash however long they last; **a ring after the peak and through
//! the [`quiet`] after the flash ends is absorbed**, as xterm suppresses a
//! bell inside `bellSuppressTime` (`[[source: xterm-man]]`). A flash and
//! the quiet after it last at least [`SPAN`], half a second, except where a
//! change of `?5` ends an other-video flash early: then the quiet runs from
//! that change ([`BellFlash::video_ended`]), and the change itself shows
//! the flash's video, so no extra transition is drawn. Either way no second
//! holds more than four of the bell's changes and no reading of the rule
//! counts more than three flashes in one -- the census's verdict and its
//! strict reading both (`baud-bench/tests/bell_flashes.rs`). The same rule
//! holds the other-video flash a `BEL` shows with no colour set.
//!
//! **Every change of the whole screen a frame draws shares one budget**
//! ([`ScreenFlash`]): the video -- `?5`'s, a `?5` pair's flash, a `BEL`'s
//! other-video flash -- and the bell's solid. No second holds more than
//! [`CHANGES`] of them, so a second holds at most three flashes on the
//! chained reading and two on the disjoint one (`baud_bench::flash`),
//! whatever mix of them a program drives. A change back toward the mode
//! past the budget is held, and drawn by the first frame the budget
//! allows, at most a second later, on a deadline the loop's clock reaches
//! ([`ScreenFlash::owed`]); a flash away from the mode starts only with
//! room for its return, and is not shown otherwise. The core's `?5`
//! changes at once and `DECRQM` answers it at once: only what is drawn is
//! paced. DEC STD 070 lets the drawing lag, "Conforming software should
//! not depend on the speed of execution when changing Screen Mode. It may
//! take as long as the time required to repaint the entire screen"
//! (070-5 p. 5-94, `[[source: dec-std-070]]`), and a held change lags
//! longer than a repaint, a departure; so does xterm's practice, which
//! repaints at every change (`ReverseVideo`, `[[source: xterm-scroll]]`'s
//! file 3269-3345). WCAG2ICT reads 2.3.1 onto software as written, and has
//! "Content from an external source that is presented through the ICT"
//! as "the responsibility of the source" (`[[source: wcag2ict-22]]`): the
//! hold of a program's own changes is baud's choice, not the text's.

use core::time::Duration;

use crate::color::{Argb, Video};
use crate::renderer::VideoFlash;
use baud_core::{PaintRole, ProgramPaint, Term};
use baud_domain::Chan;

/// A flash and the quiet after it, at least: two in a second at most.
pub const SPAN: Duration = Duration::from_millis(500);

/// How a bell that rang shows: the colour a program set, or the other
/// video, fixed when its flash starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BellKind {
    /// The other video, kitty's "Automatic color selection based on current
    /// screen colors" (`[[source: kitty-6d5d0c4-color-stack]]`): no colour
    /// set, or the automatic one.
    Video,
    /// A colour of the program's, opaque.
    Tint(Argb),
}

impl BellKind {
    /// How a bell rung now shows: in the colour a program set with
    /// `OSC 21 ; visual_bell=` ([`Term::program_paint`]), else -- none
    /// set, or the automatic one -- as the other video.
    pub fn of(term: &Term<'_>) -> BellKind {
        match term.program_paint(PaintRole::VisualBell) {
            ProgramPaint::Colour((r, g, b)) => BellKind::Tint(Argb::rgb(
                Chan::from_u8(r),
                Chan::from_u8(g),
                Chan::from_u8(b),
            )),
            ProgramPaint::Automatic | ProgramPaint::Unset => BellKind::Video,
        }
    }
}

/// What one frame draws of the bell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BellShown {
    Dark,
    /// The other video, [`crate::VideoFlash::On`].
    Video,
    /// The solid over the frame ([`crate::Renderer::set_bell_tint`]).
    Tint(BellTint),
}

impl BellShown {
    /// Its half of the frame's [`VideoFlash`].
    pub fn video(self) -> VideoFlash {
        match self {
            BellShown::Video => VideoFlash::On,
            BellShown::Dark | BellShown::Tint(_) => VideoFlash::Off,
        }
    }

    /// The solid it draws, if any ([`crate::Renderer::set_bell_tint`]).
    pub fn tint(self) -> Option<BellTint> {
        match self {
            BellShown::Tint(t) => Some(t),
            BellShown::Dark | BellShown::Video => None,
        }
    }
}

/// The bell's solid over the whole frame: its colour, and the alpha the
/// fade is at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BellTint {
    pub colour: Argb,
    pub alpha: Chan,
}

/// A flash on screen: how it shows, when it started, and the last ring it
/// holds its peak for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Lit {
    kind: BellKind,
    start: Duration,
    held: Duration,
}

/// Whether a frame comes with a ring the last drawn frame had not shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rang {
    No,
    Yes,
}

/// **The bell's flash on a clock**, as a value: [`BellFlash::at`] says what
/// a frame drawn at a moment shows and returns the state that frame
/// leaves, which a driver keeps once the frame is drawn. Moments are the
/// time since any fixed origin the caller keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BellFlash {
    lit: Option<Lit>,
    /// No flash starts before this: the [`quiet`] after the last one ended.
    quiet: Duration,
    length: Duration,
}

/// The quiet after a flash of `length`, so that the two last [`SPAN`].
pub fn quiet(length: Duration) -> Duration {
    SPAN.saturating_sub(length)
}

impl BellFlash {
    /// No flash, none ever shown, each to last `length`.
    pub fn new(length: Duration) -> BellFlash {
        BellFlash {
            lit: None,
            quiet: Duration::ZERO,
            length,
        }
    }

    /// The frame drawn at `now`: what it shows, and the state it leaves.
    /// `kind` is how a flash this frame starts shows. A frame earlier than
    /// one already drawn is read as at that frame.
    pub fn at(self, now: Duration, rang: Rang, kind: BellKind) -> (BellFlash, BellShown) {
        let half = self.length / 2;
        let mut next = self;
        if let Some(mut lit) = self.lit {
            let fall = match lit.kind {
                BellKind::Video => lit.held + self.length,
                BellKind::Tint(_) => lit.held + half,
            };
            if rang == Rang::Yes && now < fall {
                lit.held = now.max(lit.held);
            }
            let end = lit.held + self.length;
            if now < end {
                next.lit = Some(lit);
                return (next, self.show(lit, now));
            }
            next.lit = None;
            next.quiet = end + quiet(self.length);
        }
        if rang == Rang::Yes && now >= next.quiet {
            let lit = Lit {
                kind,
                start: now,
                held: now,
            };
            next.lit = Some(lit);
            return (next, self.show(lit, now));
        }
        (next, BellShown::Dark)
    }

    /// Whether a flash is on screen, and how.
    pub fn lit(&self) -> Option<BellKind> {
        self.lit.map(|l| l.kind)
    }

    /// When the solid's fade under way at `now` ends: its rise, from the
    /// frame after its start to the peak, or its fall, from the peak to
    /// its end. The other video does not fade.
    fn fading(&self, now: Duration) -> Option<Duration> {
        let half = self.length / 2;
        match self.lit {
            Some(Lit {
                kind: BellKind::Tint(_),
                start,
                held,
            }) => {
                if now > start && now < start + half {
                    Some(start + half)
                } else if now > held + half && now < held + self.length {
                    Some(held + self.length)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// When a flash on screen ends, if nothing holds it longer.
    pub fn end(&self) -> Option<Duration> {
        self.lit.map(|l| l.held + self.length)
    }

    /// The flash a frame's mode change ends: one of the other video, since
    /// the mode is what a frame draws, starting the quiet from `now`; a
    /// tint is not of the video and runs on.
    pub fn video_ended(self, now: Duration) -> BellFlash {
        match self.lit {
            Some(Lit {
                kind: BellKind::Video,
                ..
            }) => BellFlash {
                lit: None,
                quiet: now + quiet(self.length),
                ..self
            },
            _ => self,
        }
    }

    fn show(&self, lit: Lit, now: Duration) -> BellShown {
        match lit.kind {
            BellKind::Video => BellShown::Video,
            BellKind::Tint(colour) => BellShown::Tint(BellTint {
                colour,
                alpha: tint_alpha(colour, self.intensity(lit, now)),
            }),
        }
    }

    /// The fade at `now`: up over the first half from the start, held from
    /// there until half the length after the last ring, down over the
    /// second half.
    fn intensity(&self, lit: Lit, now: Duration) -> f64 {
        let half = (self.length / 2).as_secs_f64();
        if half <= 0.0 {
            return 0.0;
        }
        let up = (now.saturating_sub(lit.start)).as_secs_f64() / half;
        if up < 1.0 {
            return ease_in_out(up);
        }
        let down = (now.saturating_sub(lit.held + self.length / 2)).as_secs_f64() / half;
        if down <= 0.0 {
            return 1.0;
        }
        1.0 - ease_in_out(down.min(1.0))
    }
}

/// **The most changes of the whole screen one second holds**: four, so the
/// chained reading, every adjacent opposing pair a flash, counts three and
/// the disjoint one two -- "no more than three general flashes and / or no
/// more than three red flashes within any one-second period", "A flash
/// consists of two opposing transitions" (`[[source: wcag22]]`).
pub const CHANGES: usize = 4;

/// The period WCAG 2.3.1 counts flashes in.
pub const PERIOD: Duration = Duration::from_secs(1);

/// Whether the change back toward the mode has its slot counted
/// already: a flash's return, counted when the flash began, or a change a
/// dropped frame spent a slot on ([`ScreenFlash::refused`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Return {
    Free,
    Counted,
}

/// Whether the flash wanted now was refused for want of room: it stays
/// refused, never shown late, until the mode is wanted again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Refused {
    No,
    Yes,
}

/// How many changes a step makes room for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Need {
    One,
    Two,
}

/// Whether a step's changes fit the second that ends at its moment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fits {
    Yes,
    No,
}

/// Whether `?5` reads otherwise than the last drawn frame read it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoMoved {
    No,
    Yes,
}

/// Whether a `?5` pair's flash is on, as the driver times it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairFlash {
    Off,
    On,
}

/// What one frame draws of the whole screen: its video, and the bell's
/// solid over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenShows {
    pub video: Video,
    pub tint: Option<BellTint>,
}

fn other(v: Video) -> Video {
    match v {
        Video::Normal => Video::Reverse,
        Video::Reverse => Video::Normal,
    }
}

/// **The budget**: the video the last drawn frame drew, the moments of
/// the last [`CHANGES`] changes drawn, and the returns counted for the
/// flashes on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Pace {
    drawn: Video,
    /// Any order; a moment after the frame's is a change still to
    /// complete, counted from now.
    at: [Option<Duration>; CHANGES],
    video: Return,
    tint: Return,
    refused: Refused,
    /// When a change held back toward the mode may be drawn.
    owed: Option<Duration>,
}

impl Pace {
    fn new(video: Video) -> Pace {
        Pace {
            drawn: video,
            at: [None; CHANGES],
            video: Return::Free,
            tint: Return::Free,
            refused: Refused::No,
            owed: None,
        }
    }

    /// When `need` more changes fit: `now` if they fit already, else the
    /// moment enough of the changes the second holds have left it, or
    /// `None` while the returns counted ahead leave no room at all.
    fn fits_from(&self, now: Duration, need: Need) -> Option<Duration> {
        let mut live = self
            .at
            .map(|t| t.filter(|&t| now.saturating_sub(t) < PERIOD));
        live.sort_unstable();
        let held = live.iter().flatten().count();
        let returns = [self.video, self.tint]
            .iter()
            .filter(|&&r| r == Return::Counted)
            .count();
        let n = match need {
            Need::One => 1,
            Need::Two => 2,
        };
        let room = CHANGES.checked_sub(returns + n)?;
        match held.checked_sub(room) {
            None | Some(0) => Some(now),
            // The oldest held sort last of the empties, so the `over`-th
            // oldest is `over` past the first held.
            Some(over) => live
                .get(CHANGES - held + over - 1)
                .copied()
                .flatten()
                .map(|t| t + PERIOD),
        }
    }

    fn fits(&self, now: Duration, need: Need) -> Fits {
        match self.fits_from(now, need) {
            Some(t) if t <= now => Fits::Yes,
            _ => Fits::No,
        }
    }

    /// A change drawn at `t`, in the slot of the oldest change or an empty
    /// one. Every caller made room first, so that slot is one the second
    /// no longer holds, or, for a change a dropped frame spent a slot on,
    /// one no later than that spent slot, which counts in its place.
    fn record(&mut self, t: Duration) {
        if let Some(slot) = self.at.iter_mut().min() {
            *slot = Some(t);
        }
    }

    /// The video the frame at `now` draws, wanting `want` over a screen
    /// whose mode is `mode`, and held a while if the bell's solid is
    /// fading until `fade`: a change inside a fade would split the fade's
    /// transition in two.
    fn video(mut self, now: Duration, want: Video, mode: Video, fade: Option<Duration>) -> Pace {
        self.owed = None;
        if self.drawn == mode {
            self.video = Return::Free;
        }
        if want == mode {
            self.refused = Refused::No;
        }
        if want == self.drawn {
            return self;
        }
        if let Some(end) = fade {
            self.owed = Some(end);
            return self;
        }
        if want == mode {
            // Back to the mode: a flash's return, counted when it began, a
            // change a dropped frame counted, or a change of the mode itself.
            match (self.video, self.fits(now, Need::One)) {
                (Return::Counted, _) | (Return::Free, Fits::Yes) => {
                    self.record(now);
                    self.drawn = want;
                    self.video = Return::Free;
                }
                (Return::Free, Fits::No) => self.owed = self.fits_from(now, Need::One),
            }
        } else {
            // Away from the mode: a flash, with room for its return.
            match (self.refused, self.fits(now, Need::Two)) {
                (Refused::No, Fits::Yes) => {
                    self.record(now);
                    self.drawn = want;
                    self.video = Return::Counted;
                }
                _ => self.refused = Refused::Yes,
            }
        }
        self
    }
}

/// **The whole screen's flashes on a clock**: the bell's
/// ([`BellFlash`]) and the video's, held to one budget of [`CHANGES`] a
/// second. [`ScreenFlash::at`] says what a frame drawn at a moment shows
/// and returns the state that frame leaves, which a driver keeps once the
/// frame is drawn; moments are the time since any fixed origin the caller
/// keeps, as [`BellFlash`]'s are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenFlash {
    bell: BellFlash,
    pace: Pace,
}

impl ScreenFlash {
    /// No flash, none ever shown, a bell's to last `length`, over a screen
    /// drawn in `video`.
    pub fn new(length: Duration, video: Video) -> ScreenFlash {
        ScreenFlash {
            bell: BellFlash::new(length),
            pace: Pace::new(video),
        }
    }

    /// **The frame drawn at `now`**: `mode` the video `?5` names, `moved`
    /// whether it reads otherwise than at the last drawn frame, `pair`
    /// whether a `?5` pair's flash is on, and `rang` whether a `BEL` rang
    /// since, its flash showing as `kind`. A change of `?5` ends the
    /// bell's other-video flash and refuses one rung with it, since the
    /// mode is what a frame draws; a tint is not of the video and runs on.
    /// A tint starts only with room for its rise and its fall, which are
    /// counted where they complete or later.
    pub fn at(
        self,
        now: Duration,
        mode: Video,
        moved: VideoMoved,
        pair: PairFlash,
        rang: Rang,
        kind: BellKind,
    ) -> (ScreenFlash, ScreenShows) {
        let (mut bell, mut pace) = (self.bell, self.pace);
        let tint_start = |b: &BellFlash| match b.lit {
            Some(Lit {
                kind: BellKind::Tint(_),
                start,
                ..
            }) => Some(start),
            _ => None,
        };
        if tint_start(&bell).is_none() {
            pace.tint = Return::Free;
        }
        if moved == VideoMoved::Yes {
            bell = bell.video_ended(now);
        }
        let ring = match (rang, moved, kind) {
            (Rang::No, _, _) | (Rang::Yes, VideoMoved::Yes, BellKind::Video) => Rang::No,
            (Rang::Yes, _, _) => Rang::Yes,
        };
        let before = tint_start(&bell);
        let (mut next, mut shown) = bell.at(now, ring, kind);
        if before.is_some() && tint_start(&next) != before {
            pace.record(now);
            pace.tint = Return::Free;
        }
        if tint_start(&next).is_some_and(|s| Some(s) != before) {
            match pace.fits(now, Need::Two) {
                Fits::Yes => {
                    pace.record(now + bell.length);
                    pace.tint = Return::Counted;
                }
                Fits::No => (next, shown) = bell.at(now, Rang::No, kind),
            }
        }
        let want = match (pair, shown.video()) {
            (PairFlash::Off, VideoFlash::Off) => mode,
            _ => other(mode),
        };
        let pace = pace.video(now, want, mode, next.fading(now));
        let shows = ScreenShows {
            video: pace.drawn,
            tint: shown.tint(),
        };
        (ScreenFlash { bell: next, pace }, shows)
    }

    /// Whether the bell's flash is on screen, and how.
    pub fn lit(&self) -> Option<BellKind> {
        self.bell.lit()
    }

    /// When the bell's flash on screen ends, if nothing holds it longer.
    pub fn end(&self) -> Option<Duration> {
        self.bell.end()
    }

    /// When a change held back toward the mode may be drawn: the frame the
    /// budget owes.
    pub fn owed(&self) -> Option<Duration> {
        self.pace.owed
    }

    /// **The frame `plan` was for was dropped**, its present failed: the
    /// screen shows what the last drawn frame drew. A bell's flash on
    /// screen runs on as the frame left it, and one it would have started
    /// is lost with it; the changes it would have counted stay counted,
    /// which only spends the budget sooner. A change back toward the mode
    /// it would have drawn -- a flash's return, or a change of the mode
    /// itself, held or not -- keeps its reservation, the slot it spent, so
    /// the next frame drawn for any reason draws it, as it draws a change
    /// the budget has room for. Nothing is owed, so no frame is forced
    /// while presents keep failing.
    pub fn refused(self, plan: ScreenFlash) -> ScreenFlash {
        let bell = if self.bell.lit().is_some() || plan.bell.lit().is_none() {
            plan.bell
        } else {
            self.bell
        };
        // A change back toward the mode the frame drew -- a flash's return,
        // a change of the mode, held or not -- spent its slot there, and the
        // slot is its reservation: it needs no room again.
        let video = match (plan.pace.drawn == self.pace.drawn, plan.pace.video) {
            (false, Return::Free) => Return::Counted,
            _ => self.pace.video,
        };
        let pace = Pace {
            drawn: self.pace.drawn,
            video,
            owed: None,
            ..plan.pace
        };
        ScreenFlash { bell, pace }
    }
}

/// **The solid's alpha at `intensity`**: kitty's `draw_visual_bell_flash()`,
/// the intensity times its attenuation, 0.4, or 0.6 for a colour whose
/// largest channel in linear light is above 0.45, rounded to a byte.
pub fn tint_alpha(colour: Argb, intensity: f64) -> Chan {
    let lin = |c: Chan| {
        let c = f64::from(c.get()) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            crate::det::Det::det_powf((c + 0.055) / 1.055, 2.4)
        }
    };
    let most = lin(colour.r()).max(lin(colour.g())).max(lin(colour.b()));
    let attenuation = if most > 0.45 { 0.6 } else { 0.4 };
    let a = (intensity.clamp(0.0, 1.0) * attenuation * 255.0).round();
    Chan::from_u8(u8::try_from(a as i64).unwrap_or(u8::MAX))
}

/// CSS's `ease-in-out`, cubic-bezier(0.42, 0, 0.58, 1), at `x` in 0 to 1:
/// the curve's y where its x is `x`, the parameter found by bisection,
/// which the curve's x rising monotonically from 0 to 1 makes exact to the
/// 52 steps taken.
pub fn ease_in_out(x: f64) -> f64 {
    const P1: (f64, f64) = (0.42, 0.0);
    const P2: (f64, f64) = (0.58, 1.0);
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bez = |t: f64, a: f64, b: f64| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..52 {
        let mid = (lo + hi) * 0.5;
        if bez(mid, P1.0, P2.0) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) * 0.5, P1.1, P2.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);
    const RED: Argb = Argb(0xFFFF_0000);

    fn tint(s: BellShown) -> Option<Chan> {
        match s {
            BellShown::Tint(t) => Some(t.alpha),
            _ => None,
        }
    }

    /// The curve's ends and middle, and its symmetry about the middle.
    #[test]
    fn ease_in_out_is_css_curve() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert!((ease_in_out(1.0) - 1.0).abs() < 1e-12);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-9);
        for k in 1..10 {
            let x = f64::from(k) / 10.0;
            assert!((ease_in_out(x) + ease_in_out(1.0 - x) - 1.0).abs() < 1e-9);
        }
        assert!(ease_in_out(0.1) < 0.1, "slow at the start");
    }

    /// kitty's attenuation: 0.4 for a dark colour, 0.6 for a light one,
    /// the line at 0.45 in linear light.
    #[test]
    fn a_light_colour_is_attenuated_less() {
        let grey = |v: u8| Argb::rgb(Chan::from_u8(v), Chan::from_u8(v), Chan::from_u8(v));
        assert_eq!(tint_alpha(grey(0x40), 1.0).get(), 102);
        assert_eq!(tint_alpha(RED, 1.0).get(), 153);
        // 0.45 in linear light is about 178 encoded.
        assert_eq!(tint_alpha(grey(178), 1.0).get(), 102);
        assert_eq!(tint_alpha(grey(179), 1.0).get(), 153);
    }

    /// One ring: up over the first half, down over the second, gone at the
    /// length.
    #[test]
    fn one_ring_fades_in_and_out_over_the_length() {
        let b = BellFlash::new(MS * 100);
        let (b, s) = b.at(MS * 1000, Rang::Yes, BellKind::Tint(RED));
        assert_eq!(
            tint(s).map(Chan::get),
            Some(0),
            "the first frame is the fade's start"
        );
        let (b2, s) = b.at(MS * 1050, Rang::No, BellKind::Video);
        assert_eq!(tint(s).map(Chan::get), Some(153), "the peak");
        let (_, s) = b.at(MS * 1025, Rang::No, BellKind::Video);
        assert!(tint(s).map(Chan::get).unwrap() > 50 && tint(s).map(Chan::get).unwrap() < 103);
        let (b3, s) = b2.at(MS * 1100, Rang::No, BellKind::Video);
        assert_eq!(s, BellShown::Dark, "over at the length");
        assert_eq!(b3.lit(), None);
    }

    /// Rings closer than half the length hold the peak; the fall starts
    /// half the length after the last.
    #[test]
    fn rings_while_it_rises_or_holds_hold_the_peak() {
        let mut b = BellFlash::new(MS * 100);
        let mut shown = vec![];
        for f in 0..30u32 {
            let rang = if f < 12 { Rang::Yes } else { Rang::No };
            let (n, s) = b.at(MS * (1000 + f * 16), rang, BellKind::Tint(RED));
            b = n;
            shown.push(tint(s).map(Chan::get));
        }
        // Rising, then held at the peak while the rings come and for half
        // the length after the last, at 1176 ms.
        assert_eq!(shown[0], Some(0));
        assert!(shown[4..15].iter().all(|&a| a == Some(153)), "{shown:?}");
        assert!(shown[15].unwrap() < 153 && shown[15].unwrap() > 0);
        assert_eq!(shown[18], None, "over 100 ms after the last ring");
        // Monotone: up, then down, never up again.
        let a: Vec<u8> = shown.iter().map(|s| s.unwrap_or(0)).collect();
        let peak = a.iter().position(|&v| v == 153).unwrap();
        assert!(a[..=peak].windows(2).all(|w| w[0] <= w[1]));
        assert!(a[peak..].windows(2).all(|w| w[0] >= w[1]));
    }

    /// A ring after the peak, and through the quiet after the end, is
    /// absorbed; the first after the quiet starts a flash.
    #[test]
    fn a_ring_in_the_fall_or_the_quiet_is_absorbed() {
        let b = BellFlash::new(MS * 100);
        let (b, _) = b.at(Duration::ZERO, Rang::Yes, BellKind::Tint(RED));
        let (b, s) = b.at(MS * 60, Rang::Yes, BellKind::Tint(RED));
        assert!(tint(s).map(Chan::get).unwrap() < 153, "falling, not held");
        let (b, s) = b.at(MS * 100, Rang::No, BellKind::Video);
        assert_eq!(s, BellShown::Dark);
        let (b, s) = b.at(MS * 499, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Dark, "inside the quiet");
        let (_, s) = b.at(MS * 500, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Video, "half a second after the start");
    }

    /// The other video holds while rings come closer than the length, as
    /// a second `?5` pair restarts its flash.
    #[test]
    fn the_other_video_holds_for_the_length_after_each_ring() {
        let b = BellFlash::new(MS * 100);
        let (b, s) = b.at(Duration::ZERO, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Video);
        let (b, _) = b.at(MS * 90, Rang::Yes, BellKind::Video);
        let (b, s) = b.at(MS * 180, Rang::No, BellKind::Video);
        assert_eq!(s, BellShown::Video, "held from the second ring");
        assert_eq!(b.end(), Some(MS * 190));
        let (b, s) = b.at(MS * 190, Rang::No, BellKind::Video);
        assert_eq!(s, BellShown::Dark);
        let (_, s) = b.at(MS * 589, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Dark, "the quiet runs from the end");
    }

    /// A mode change ends a flash of the other video and starts the quiet;
    /// a tint runs on.
    #[test]
    fn a_mode_change_ends_the_other_video_and_not_a_tint() {
        let b = BellFlash::new(MS * 100);
        let (v, _) = b.at(Duration::ZERO, Rang::Yes, BellKind::Video);
        let v = v.video_ended(MS * 10);
        assert_eq!(v.lit(), None);
        let (_, s) = v.at(MS * 409, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Dark);
        let (t, _) = b.at(Duration::ZERO, Rang::Yes, BellKind::Tint(RED));
        assert_eq!(t.video_ended(MS * 10), t);
    }

    #[test]
    fn the_quiet_and_the_length_make_the_span() {
        assert_eq!(quiet(MS * 100), MS * 400);
        assert_eq!(quiet(MS * 100) + MS * 100, SPAN);
    }

    const N: Video = Video::Normal;
    const R: Video = Video::Reverse;

    /// A frame of `s` at `now` over a screen in `mode`, the mode moved
    /// since the last frame or not, with no pair and no ring.
    fn video_at(
        s: ScreenFlash,
        now: Duration,
        mode: Video,
        moved: VideoMoved,
    ) -> (ScreenFlash, Video) {
        let (s, shows) = s.at(now, mode, moved, PairFlash::Off, Rang::No, BellKind::Video);
        (s, shows.video)
    }

    /// Four changes in a row are drawn as they come; the fifth inside the
    /// second is held, owed when the first leaves it, and drawn then. A
    /// change after a quiet second is drawn at once.
    #[test]
    fn a_fifth_change_in_a_second_is_held_until_the_first_leaves_it() {
        let mut s = ScreenFlash::new(MS * 100, N);
        for (k, mode) in [R, N, R, N].into_iter().enumerate() {
            let ms = u32::try_from(k).unwrap() * 50;
            let (n, v) = video_at(s, MS * ms, mode, VideoMoved::Yes);
            assert_eq!(v, mode, "change {k}, drawn");
            s = n;
        }
        let (s, v) = video_at(s, MS * 200, R, VideoMoved::Yes);
        assert_eq!(v, N, "the fifth, held");
        assert_eq!(s.owed(), Some(MS * 1000), "a second after the first");
        let (s, v) = video_at(s, MS * 999, R, VideoMoved::No);
        assert_eq!(v, N, "still held");
        let (s, v) = video_at(s, MS * 1000, R, VideoMoved::No);
        assert_eq!((v, s.owed()), (R, None), "drawn when owed");
        let (s, _) = video_at(s, MS * 1050, N, VideoMoved::Yes);
        let (s, _) = video_at(s, MS * 1100, R, VideoMoved::Yes);
        let (s, v) = video_at(s, MS * 1150, N, VideoMoved::Yes);
        assert_eq!(v, N, "each of the first four left the second in turn");
        let (s, v) = video_at(s, MS * 1200, R, VideoMoved::Yes);
        assert_eq!((v, s.owed()), (N, Some(MS * 2000)), "this second's fifth");
        let (s, v) = video_at(s, MS * 1250, N, VideoMoved::Yes);
        assert_eq!(
            (v, s.owed()),
            (N, None),
            "a change back before it is drawn: nothing"
        );
        let (_, v) = video_at(s, MS * 2300, R, VideoMoved::Yes);
        assert_eq!(v, R, "after a quiet second, at once");
    }

    /// **A flash starts only with room for its return**: with three
    /// changes in the second, a pair's flash is not shown, and stays
    /// unshown -- never started late -- until the mode is wanted again.
    #[test]
    fn a_flash_starts_only_with_room_for_its_return() {
        let mut s = ScreenFlash::new(MS * 100, N);
        for (k, mode) in [R, N, R].into_iter().enumerate() {
            s = video_at(
                s,
                MS * (u32::try_from(k).unwrap() * 50),
                mode,
                VideoMoved::Yes,
            )
            .0;
        }
        let pair = |s: ScreenFlash, ms: u32, p: PairFlash| {
            let (s, shows) = s.at(MS * ms, R, VideoMoved::No, p, Rang::No, BellKind::Video);
            (s, shows.video)
        };
        let (s, v) = pair(s, 200, PairFlash::On);
        assert_eq!(v, R, "refused: one change left");
        let (s, v) = pair(s, 1050, PairFlash::On);
        assert_eq!(v, R, "the room came back, and the flash stays refused");
        let (s, v) = pair(s, 1100, PairFlash::Off);
        assert_eq!(v, R);
        let (s, v) = pair(s, 1116, PairFlash::On);
        assert_eq!(v, N, "a new flash, with room");
        let (_, v) = pair(s, 1216, PairFlash::Off);
        assert_eq!(v, R, "and its return, counted when it began");
    }

    /// **A change inside the solid's fade waits for the fade**, so the
    /// fade's transition is not split in two: a change while the solid
    /// rises is owed at the peak and drawn there.
    #[test]
    fn a_change_inside_the_solids_fade_waits_for_it() {
        let s = ScreenFlash::new(MS * 100, N);
        let tint = BellKind::Tint(RED);
        let (s, shows) = s.at(
            Duration::ZERO,
            N,
            VideoMoved::No,
            PairFlash::Off,
            Rang::Yes,
            tint,
        );
        assert!(shows.tint.is_some(), "the solid starts");
        let (s, shows) = s.at(MS * 16, R, VideoMoved::Yes, PairFlash::Off, Rang::No, tint);
        assert_eq!(shows.video, N, "held while it rises");
        assert_eq!(s.owed(), Some(MS * 50), "owed at the peak");
        let (s, shows) = s.at(MS * 50, R, VideoMoved::No, PairFlash::Off, Rang::No, tint);
        assert_eq!((shows.video, s.owed()), (R, None), "drawn at the peak");
    }

    /// **A solid starts only with room for its rise and its fall**: after
    /// three changes of the video in the second, a ring draws nothing.
    #[test]
    fn a_solid_starts_only_with_room_for_its_rise_and_fall() {
        let mut s = ScreenFlash::new(MS * 100, N);
        for (k, mode) in [R, N, R].into_iter().enumerate() {
            s = video_at(
                s,
                MS * (u32::try_from(k).unwrap() * 50),
                mode,
                VideoMoved::Yes,
            )
            .0;
        }
        let tint = BellKind::Tint(RED);
        let (s, shows) = s.at(MS * 200, R, VideoMoved::No, PairFlash::Off, Rang::Yes, tint);
        assert_eq!((shows.tint, s.lit()), (None, None), "absorbed");
        let (_, shows) = s.at(
            MS * 1100,
            R,
            VideoMoved::No,
            PairFlash::Off,
            Rang::Yes,
            tint,
        );
        assert!(shows.tint.is_some(), "with room again");
    }

    /// **A dropped frame keeps the video last shown and owes nothing**: a
    /// change it would have drawn is drawn by the next frame that can.
    #[test]
    fn a_dropped_frame_keeps_the_video_shown_and_owes_nothing() {
        let mut s = ScreenFlash::new(MS * 100, N);
        for (k, mode) in [R, N, R, N].into_iter().enumerate() {
            s = video_at(
                s,
                MS * (u32::try_from(k).unwrap() * 50),
                mode,
                VideoMoved::Yes,
            )
            .0;
        }
        let (held, v) = video_at(s, MS * 200, R, VideoMoved::Yes);
        assert_eq!((v, held.owed()), (N, Some(MS * 1000)));
        let (plan, v) = video_at(held, MS * 1000, R, VideoMoved::No);
        assert_eq!(v, R);
        let kept = held.refused(plan);
        assert_eq!(kept.owed(), None, "no frame forced while presents fail");
        let (_, v) = video_at(kept, MS * 1300, R, VideoMoved::No);
        assert_eq!(v, R, "the next frame draws it");
    }
}
