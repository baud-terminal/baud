//! **The Linux driver's timed effects**: `SGR 5` and `SGR 6` text blink,
//! the cursor trail, the alternate screen's slide and the scrollbar's thumb,
//! each the macOS app's and each drawn by the CPU path the driver paints
//! with. The clock is seconds since the driver started, where the macOS
//! app reads the display link's timestamp; the loop has no tick, so an
//! effect says what it owes instead:
//!
//! * **an instant**, for a change that comes on the clock with no event to
//!   bring it -- the next blink flip of a rate on screen, the end of a
//!   trail's dwell, the end of the thumb's hold -- which bounds the loop's
//!   one wait (`deadline`);
//! * **every frame**, for motion in flight -- a trail sweeping, the slide,
//!   the thumb fading -- which the compositor's frame events pace: the loop
//!   owes the next frame on each turn and presents it once the last one is
//!   done, so a frame in flight never spins the wait.
//!
//! Every trail style runs as it does on macOS: an effect -- the fire and
//! its family -- is drawn by the CPU path from its light
//! (`baud_render::effect`), the pixels the macOS app's CPU path draws. The
//! motion is the desktop's preference, as the driver hears it
//! from the Settings portal (`bus.rs`) or `BAUD_MOTION` fixes it, and
//! full until then: reduced, text blink is held shown and the default
//! trail is none, as under macOS's Reduce Motion ([`Timed::set_motion`]).
//! Pure, over a real `Term`, so the tests run on every platform.

use crate::config::{CursorTrail, ScreenTransition, SystemMotion, TrailScroll};
use crate::trail::Chase;
use baud_core::{CursorStyle, Term};
use baud_domain::{Col, HostTime, RegisBlink, RegisRepainted, Row, Rows};
use baud_render::{
    BlinkPhases, Blinking, Points, Rect, Scroll, Strength, Trail, TrailLook, Transition,
};
use std::time::{Duration, Instant};

/// Seconds since the driver started: the clock every effect reads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Clock {
    epoch: Instant,
}

impl Clock {
    pub(crate) fn new(epoch: Instant) -> Clock {
        Clock { epoch }
    }

    pub(crate) fn secs(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.epoch).as_secs_f64()
    }

    /// The clock's reading as the reveal and the trails take it.
    pub(crate) fn host(&self, now: Instant) -> HostTime {
        let ns = now.saturating_duration_since(self.epoch).as_nanos();
        HostTime::of_nanos(u64::try_from(ns).unwrap_or(u64::MAX))
    }

    fn instant_of_secs(&self, secs: f64) -> Instant {
        self.epoch + Duration::try_from_secs_f64(secs.max(0.0)).unwrap_or(Duration::MAX)
    }

    pub(crate) fn instant_of(&self, t: HostTime) -> Instant {
        self.epoch + Duration::from_nanos(t.nanos())
    }
}

/// **Whether a screen switch's slide runs on the clock or is held at its
/// start**, test-only (`slide-hold`). Held, the slide stays at progress 0
/// under the faint veil, where the frame drawn is the one kept at the
/// switch, so a capture can read that frame however long it takes.
#[cfg(feature = "slide-hold")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SlideClock {
    Runs,
    Held,
}

#[cfg(feature = "slide-hold")]
impl SlideClock {
    /// The clock `BAUD_SLIDE_HOLD` asks for: held at `1`, running
    /// otherwise.
    fn told(value: Option<&std::ffi::OsStr>) -> SlideClock {
        match value.and_then(|v| v.to_str()) {
            Some("1") => SlideClock::Held,
            _ => SlideClock::Runs,
        }
    }

    /// The instant a switch that began at `start` is drawn at, at `secs`.
    fn at(self, start: f64, secs: f64) -> f64 {
        match self {
            SlideClock::Runs => secs,
            SlideClock::Held => start,
        }
    }
}

/// A ghost's sweep in flight: the cells it left and reached, when it began
/// on the clock, how far the cell it left has ridden the screen's scroll
/// and whether it still rides (`trail::ghost_ride`), and how it is drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Ghost {
    from: (Row, Col),
    to: (Row, Col),
    start: f64,
    origin_dy: f32,
    ride: crate::trail::GhostRide,
    strength: Strength,
}

/// What a turn of the loop owes the frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owes {
    Nothing,
    /// A frame, its rows as damage says.
    Frame,
    /// A frame repainted whole: a blink flip moves no cell word, so row
    /// damage cannot see it, as on macOS.
    Full,
}

/// What the next frame draws, for the driver to hand the renderer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Plan {
    pub(crate) scroll: Option<Scroll>,
    pub(crate) trail: Option<TrailLook>,
    pub(crate) fade: Option<Transition>,
    /// The alternate-screen slide, which the driver composes from the live
    /// frame and the frame it kept at the switch.
    pub(crate) slide: Option<baud_render::ScreenSlide>,
    pub(crate) blink: BlinkPhases,
    pub(crate) splash: Option<baud_render::SplashLook>,
    /// iTerm2's fireworks (`fireworks.rs`), the older show first.
    pub(crate) fireworks: baud_render::fireworks::Shows,
}

/// The cell the frame is drawn on: its size in pixels and its height in
/// points, the flash bound's size gate (`trail::GATE`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Cell {
    pub(crate) px: (f32, f32),
    pub(crate) height: Points,
}

pub(crate) struct Timed {
    pub(crate) clock: Clock,
    motion: SystemMotion,
    /// `[images] animation`: whether a kitty animation plays, with the
    /// motion preference (`access::image_motion`).
    animation: crate::config::ImageAnimation,
    /// The phases the last frame drew with, and the rates it found on
    /// screen (`Renderer::blink_seen`).
    blink: BlinkPhases,
    seen: Blinking,
    /// Whether the ReGIS page shown keeps blinking pixels, which flip with
    /// SGR 5 whether or not any text blinks (`Term::regis_blink`).
    regis: RegisBlink,
    /// The config's trail, and the one drawn under the motion preference.
    pick: crate::config::TrailPick,
    trail: CursorTrail,
    chase: Chase<HostTime>,
    ghost: Option<Ghost>,
    last_cell: Option<(Row, Col)>,
    /// `[cursor] trail_scroll`: whether a trail in flight rides the
    /// screen's scroll.
    scroll: TrailScroll,
    /// The lines the whole screen retired since the last frame, which ride
    /// the trail at the next ([`Timed::scrolled`]).
    riding: Rows,
    /// The screen the last frame saw, from which a program's scroll that
    /// carried the cursor is taken away (`carried.rs`).
    carried: crate::carried::Carried,
    transition: ScreenTransition,
    /// The switch being animated: when it began, and how it is drawn.
    switch: Option<(f64, crate::effects::SwitchForm)>,
    /// The scrollbar's thumb (`bar.rs`).
    pub(crate) bar: crate::bar::Bar,
    /// The launch splash (`splash.rs`): gone unless [`Timed::with_splash`]
    /// shows it.
    splash: crate::splash::Splash,
    /// iTerm2's fireworks (`fireworks.rs`), built with the timing, so a
    /// request read before the first plan starts a show on it.
    fireworks: crate::fireworks::Fireworks,
    /// **Effect pacing** (`pacer.rs`): the decision, the refresh period the
    /// window last said, which of two trail-only turns this one is at half
    /// rate, and the refresh a skipped turn wakes for.
    pacer: crate::pacer::EffectPacer,
    period: Option<Duration>,
    parity: Parity,
    paced_wake: Option<Instant>,
    /// Whether a switch's slide runs or is held at its start.
    #[cfg(feature = "slide-hold")]
    slide: SlideClock,
}

/// Which of two trail-only turns at half rate a turn is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parity {
    Drawn,
    Skipped,
}

impl Timed {
    pub(crate) fn new(
        epoch: Instant,
        trail: crate::config::TrailPick,
        transition: ScreenTransition,
    ) -> Timed {
        let motion = SystemMotion::Full;
        let effective = trail.effective(motion);
        Timed {
            clock: Clock::new(epoch),
            motion,
            animation: crate::config::ImageAnimation::Os,
            blink: BlinkPhases::SHOWN,
            seen: Blinking::UNSEEN,
            regis: RegisBlink::Off,
            pick: trail,
            trail: effective,
            chase: Chase::new(effective.style()),
            ghost: None,
            last_cell: None,
            scroll: TrailScroll::Ride,
            riding: Rows::ZERO,
            carried: crate::carried::Carried::new(),
            transition,
            switch: None,
            bar: crate::bar::Bar::new(),
            splash: crate::splash::Splash::new(crate::config::LaunchSplash::Off),
            fireworks: crate::fireworks::Fireworks::new(),
            pacer: crate::pacer::EffectPacer::new(),
            period: None,
            parity: Parity::Skipped,
            paced_wake: None,
            #[cfg(feature = "slide-hold")]
            slide: SlideClock::told(std::env::var_os("BAUD_SLIDE_HOLD").as_deref()),
        }
    }

    /// **`[cursor] trail_scroll`, from the config read at launch.**
    pub(crate) fn with_trail_scroll(mut self, scroll: TrailScroll) -> Timed {
        self.scroll = scroll;
        self
    }

    /// **`[cursor] trail_scroll`, read again**: the next read's scroll rides
    /// a trail in flight, or leaves it, as it says.
    pub(crate) fn set_trail_scroll(&mut self, scroll: TrailScroll) {
        self.scroll = scroll;
    }

    /// **A read scrolled the screen**: the lines the whole screen retired
    /// at the live window ride a trail in flight up with the text at the
    /// next frame, as on macOS (`effects::trail_rides`). They are taken at
    /// that frame's instant, so the trail it draws is rigid with the text,
    /// and summed until then, as rides at one instant are. The count needs
    /// no guard for a resize inside the read, as the macOS pump's does: this
    /// driver refuses a program's page width, and its own resize snaps.
    pub(crate) fn scrolled(&mut self, term: &Term<'_>, before: crate::keep::Before) {
        let moved = crate::keep::live_moved(term, before);
        if let Some(n) = crate::effects::trail_rides(term, before, moved, self.scroll) {
            self.riding = Rows::of_count(self.riding.index() + n.index());
        }
    }

    /// **The window's splash**, from `[launch] splash` (and `BAUD_SPLASH`):
    /// waiting for the window to be shown, as on macOS.
    pub(crate) fn with_splash(mut self, setting: crate::config::LaunchSplash) -> Timed {
        self.splash = crate::splash::Splash::new(setting);
        self
    }

    /// **The window is shown**: the compositor's first frame callback after
    /// the first frame was committed -- "a good time to start drawing a new
    /// frame", and one a server "should avoid signaling ... if the surface
    /// is not visible in any way" (`[[source: wayland]]`, `wl_surface.frame`).
    /// The splash's clock starts on the frame after it, which this owes.
    pub(crate) fn shown(&mut self) -> Owes {
        match self.splash.shown() {
            crate::splash::Owes::Frame => Owes::Frame,
            crate::splash::Owes::Nothing => Owes::Nothing,
        }
    }

    /// **`[launch] splash` read again**: off takes a splash still on
    /// screen down, which owes the frame that clears it.
    pub(crate) fn set_splash(&mut self, setting: crate::config::LaunchSplash) -> Owes {
        match self.splash.set(setting) {
            crate::splash::Owes::Frame => Owes::Full,
            crate::splash::Owes::Nothing => Owes::Nothing,
        }
    }

    /// **The desktop's motion preference changed**: blinking text held
    /// shown or blinking again from the next turn, and the trail the
    /// config's pick gives under it -- only the default follows, as on
    /// macOS (`TrailPick::effective`). A switched trail cuts any trail in
    /// flight, a fire not left to burn out, and owes a frame to clear what
    /// was drawn; at rest, or with the trail unchanged, nothing.
    pub(crate) fn set_motion(&mut self, motion: SystemMotion, now: Instant) -> Owes {
        self.motion = motion;
        let next = self.pick.effective(motion);
        if next == self.trail {
            return Owes::Nothing;
        }
        let host = self.clock.host(now);
        let drawn = self.chase.owes(host) == crate::caret::Owes::Frame || self.ghost.is_some();
        self.trail = next;
        self.chase.restyle(next.style());
        self.ghost = None;
        self.last_cell = None;
        match drawn {
            true => Owes::Frame,
            false => Owes::Nothing,
        }
    }

    /// **The config's trail and screen transition, read again**
    /// (`reload.rs`): the pick's trail under the motion preference in
    /// force, switched as a change of that preference switches it.
    pub(crate) fn set_config(
        &mut self,
        pick: crate::config::TrailPick,
        transition: ScreenTransition,
        now: Instant,
    ) -> Owes {
        self.pick = pick;
        self.transition = transition;
        self.set_motion(self.motion, now)
    }

    /// **`[images] animation`, from the config read at launch or again**:
    /// the next turn steps or holds a kitty animation as it says.
    pub(crate) fn set_image_animation(&mut self, a: crate::config::ImageAnimation) {
        self.animation = a;
    }

    /// The trail drawn now.
    pub(crate) fn trail(&self) -> CursorTrail {
        self.trail
    }

    /// The seed the trail's next ignition takes, which moves on each one
    /// (`Chase::next_seed`).
    #[cfg(test)]
    pub(crate) fn next_seed(&self) -> baud_render::effect::Seed {
        self.chase.next_seed()
    }

    /// **The instant the loop must wake by**, if an effect changes on the
    /// clock before any event could arrive: the next flip of a rate on
    /// screen, a ReGIS page's blinking pixels flipping with the slow rate,
    /// a trail's dwell, the thumb's hold. Always ahead of `now`, so a past
    /// deadline never spins the wait.
    pub(crate) fn deadline(&self, now: Instant) -> Option<Instant> {
        let seen = match self.regis {
            RegisBlink::On => self.seen.with(Blinking::SLOW),
            RegisBlink::Off => self.seen,
        };
        let flip = crate::effects::next_flip(self.clock.secs(now), seen, self.motion)
            .map(|s| self.clock.instant_of_secs(s));
        let dwell = self.chase.deadline().map(|t| self.clock.instant_of(t));
        let hold = self.bar.deadline().map(|t| self.clock.instant_of(t));
        let cap = self
            .splash
            .deadline()
            .map(|s| self.clock.instant_of_secs(s));
        [flip, dwell, hold, cap, self.paced_wake]
            .into_iter()
            .flatten()
            .filter(|&t| t > now)
            .min()
    }

    /// **The refresh period the window says**, which effect pacing weighs a
    /// frame's cost against: `None` until presentation feedback names one.
    pub(crate) fn set_period(&mut self, period: Option<Duration>) {
        self.period = period;
    }

    /// **An effect frame drawn**: its key -- the arm, what it drew, its
    /// frame's size and scale --, what it cost, whether the frame before
    /// it was shown late and whether its build drew into the atlas, fed to
    /// the pacer; the divisor when it changed.
    pub(crate) fn effect_frame(
        &mut self,
        key: crate::pacer::Key,
        cost: Duration,
        missed: crate::pacer::Missed,
        cold: crate::pacer::Cold,
    ) -> Option<crate::pacer::Divisor> {
        self.pacer.frame(key, cost, missed, cold, self.period)
    }

    /// **What a turn of the loop at `now` owes**: a whole frame when a
    /// blink rate on screen flipped or a ReGIS page's blinking pixels were
    /// repainted, a frame while a trail, the fade or the thumb moves or a
    /// dwell ran out, else nothing.
    pub(crate) fn turn(&mut self, now: Instant, term: &mut Term<'_>) -> Owes {
        let phase = crate::effects::text_blink(self.clock.secs(now), self.motion);
        let mut owes = Owes::Nothing;
        if phase != self.blink {
            if self.seen.flips(self.blink, phase) {
                owes = Owes::Full;
            }
            self.blink = phase;
        }
        // A ReGIS drawing's blinking pixels flip with SGR 5's phase, as on
        // macOS; the core repaints them and says so, which owes a frame.
        if term.set_regis_blink(crate::effects::regis_phase(phase.slow)) == RegisRepainted::Yes {
            owes = Owes::Full;
        }
        let host = self.clock.host(now);
        // A kitty animation's next frame, on this clock and held as
        // `[images] animation` and reduced motion say, as on macOS
        // (`access::image_motion`); the desktop has no Animated Images
        // setting.
        let motion =
            crate::access::image_motion(self.animation, self.motion, crate::access::AutoPlay::On);
        if term.animate_images(host, motion) == baud_domain::Animated::Changed {
            owes = Owes::Full;
        }
        // The thumb steps every turn, whatever else is owed.
        let thumb = self.bar.turn(host, term) == crate::bar::Owes::Frame;
        let splash = self.splash.owes(self.clock.secs(now)) == crate::splash::Owes::Frame;
        // The fireworks owe every frame while a show is up, and the one a
        // request starts.
        let fireworks = self.fireworks.owes(term) == crate::fireworks::Owes::Frame;
        let chasing = self.chase.owes(host) == crate::caret::Owes::Frame;
        let others = thumb || splash || self.ghost.is_some() || self.switch.is_some();
        let moving = others || fireworks || chasing;
        // **Effect pacing** (`pacer.rs`): at half rate, of the turns owed by
        // the trail or the fireworks alone, every second owes nothing and
        // wakes the loop a period on; a turn owed for anything else is drawn.
        self.paced_wake = None;
        let paced_only = owes == Owes::Nothing && !others && (fireworks || chasing);
        if let (true, Some(period), crate::pacer::Divisor::Half) =
            (paced_only, self.period, self.pacer.divisor())
        {
            self.parity = match self.parity {
                Parity::Drawn => Parity::Skipped,
                Parity::Skipped => Parity::Drawn,
            };
            if self.parity == Parity::Skipped {
                self.paced_wake = Some(now + period);
                return Owes::Nothing;
            }
        }
        match (owes, moving) {
            (Owes::Full, _) => Owes::Full,
            (_, true) => Owes::Frame,
            _ => owes,
        }
    }

    /// **The instant a kitty animation's next frame is due**, after the
    /// turn at `now` took what was due: what bounds the loop's wait while a
    /// picture animates (`Term::next_image_frame`).
    pub(crate) fn frame_deadline(&self, now: Instant, term: &Term<'_>) -> Option<Instant> {
        term.next_image_frame(self.clock.host(now))
            .map(|t| self.clock.instant_of(t))
    }

    /// The rates the frame just drawn found on screen, and whether the
    /// ReGIS page it showed keeps blinking pixels.
    pub(crate) fn drawn(&mut self, seen: Blinking, regis: RegisBlink) {
        self.seen = seen;
        self.regis = regis;
    }

    /// **The last key, `key`** -- the caret's (`Caret::last_key`) -- handed
    /// before every plan, which arms the trail for the move after it
    /// (`Chase::keyed`).
    pub(crate) fn keyed(&mut self, key: Option<Instant>) {
        self.chase.keyed(key.map(|k| self.clock.host(k)));
    }

    /// **The cells moved under the trail** -- a resize, a font change: the
    /// next sighting starts afresh, no ghost sweeps across, and no line
    /// scrolled before it rides what comes after.
    pub(crate) fn snap(&mut self) {
        self.chase.snap();
        self.last_cell = None;
        self.ghost = None;
        self.riding = Rows::ZERO;
        self.carried.forget();
    }

    /// **Another surface came up**, `screen`: no trail crosses the switch,
    /// and when `[screen] transition` is on the new screen slides in over
    /// the frame the driver keeps now, or is veiled under reduced motion
    /// ([`crate::effects::switch_form`]).
    ///
    /// Returns the form it takes, so the driver keeps its frame for a slide;
    /// `None` with the transition off.
    pub(crate) fn switched(
        &mut self,
        now: Instant,
        screen: baud_core::Screen,
    ) -> Option<crate::effects::SwitchForm> {
        self.snap();
        if self.transition == ScreenTransition::Off {
            return None;
        }
        let form = crate::effects::switch_form(screen, self.motion);
        self.switch = Some((self.clock.secs(now), form));
        Some(form)
    }

    /// **The next frame's effects at `now`**, the macOS frame's order: the
    /// thumb as the reveal shows it, the trail spawned where the cursor now
    /// is and advanced, the fade, the blink phases. `rect` is the
    /// renderer's cursor rectangle for a cell and a style; `scroll` the
    /// thumb's frame.
    pub(crate) fn plan(
        &mut self,
        now: Instant,
        term: &Term<'_>,
        rect: impl Fn(Row, Col, CursorStyle) -> Rect,
        cell: Cell,
        widths: baud_render::Widths,
    ) -> Plan {
        let host = self.clock.host(now);
        let secs = self.clock.secs(now);
        let look = self.bar.look(host);
        self.bar.drawn = look;
        let scroll = match term.scrollbar_shown() {
            baud_domain::ScrollbarShown::Shown => Some(Scroll {
                window: crate::effects::window_of(term),
                look,
                widths,
                scroller: self.bar.scroller(),
            }),
            baud_domain::ScrollbarShown::Hidden => None,
        };
        // The lines the screen scrolled since the last frame ride the trail
        // first, at this frame's instant, as the macOS pump's ride comes
        // before the frame on the display link's: a ghost this frame
        // spawns starts from where the cursor now is.
        let riding = core::mem::replace(&mut self.riding, Rows::ZERO);
        if let Some((dy, floor)) = crate::trail::ride_px(riding, term.geometry().1, cell.px.1) {
            self.chase.ride(dy, host, floor, cell.px.1);
            self.ghost = self.ghost.and_then(|g| {
                let p = crate::trail::ghost_progress(g.start, secs);
                let rows = (g.from.0, g.to.0);
                crate::trail::ghost_ride(rows, (g.origin_dy, g.ride), dy, p, cell.px.1).map(
                    |(origin_dy, ride)| Ghost {
                        origin_dy,
                        ride,
                        ..g
                    },
                )
            });
        }
        let followed = crate::effects::trail_cursor(term);
        let cursor_cell = followed.map(crate::effects::TrailCursor::cell);
        // A cursor the program's scroll took with its line: what the trail
        // holds of the text moves with it, and only the rest of the move
        // trails, as on macOS (`carried.rs`). A frame the whole screen's
        // scroll rode is the ride's.
        let carried = crate::carried::carry(
            &mut self.carried,
            term,
            cursor_cell,
            (self.trail, self.scroll),
            riding,
            (&mut self.chase, &mut self.last_cell),
            cell.px.1,
        );
        if let Some(c) = carried {
            self.ghost = self.ghost.map(|g| Ghost {
                to: (c.row(g.to.0), g.to.1),
                origin_dy: g.origin_dy + c.px(cell.px.1),
                ..g
            });
        }
        if self.trail == CursorTrail::Ghost {
            let block = |(r, c): (Row, Col)| rect(r, c, CursorStyle::SteadyBlock);
            let sweep = crate::trail::ghost_sweep(
                self.chase.ledger(),
                self.last_cell,
                cursor_cell,
                host,
                block,
                (cell.px, cell.height),
            );
            if let Some(sw) = sweep {
                self.ghost = Some(Ghost {
                    from: sw.from,
                    to: sw.to,
                    start: secs,
                    origin_dy: 0.0,
                    ride: crate::trail::GhostRide::Riding(0.0),
                    strength: sw.strength,
                });
            }
        }
        self.last_cell = cursor_cell;
        // Looking back drops a sweep in flight, as the chase arms snap: its
        // live rows are not what the window shows there.
        if term.window_back().index() != 0 {
            self.ghost = None;
        }
        let ghost = self.ghost.and_then(|g| {
            let p = crate::trail::ghost_progress(g.start, secs);
            match p >= 1.0 {
                // Landed: this frame draws no trail, which clears the
                // ghost's last rows, and nothing more is owed.
                true => None,
                false => Some(Trail {
                    from: g.from,
                    to: g.to,
                    progress: p.max(0.0),
                    origin_dy: g.origin_dy,
                    lift: baud_font::Px::of(0),
                    strength: g.strength,
                }),
            }
        });
        if ghost.is_none() {
            self.ghost = None;
        }
        let trail = match self.trail {
            CursorTrail::Ghost => ghost.map(TrailLook::Ghost),
            CursorTrail::Off => None,
            CursorTrail::Quad
            | CursorTrail::Sweep
            | CursorTrail::Fire
            | CursorTrail::Plasma
            | CursorTrail::Lightning
            | CursorTrail::Ice
            | CursorTrail::Ufo
            | CursorTrail::Orb
            | CursorTrail::Liquid => {
                // Looking back forgets where the cursor was, as on macOS.
                if term.window_back().index() != 0 {
                    self.chase.snap();
                }
                let style = term.cursor_style();
                let r = followed.map(|f| {
                    let (row, col) = f.cell();
                    rect(row, col, f.style(style))
                });
                self.chase.frame(r, host, cell.px, cell.height)
            }
        };
        // The switch: settled, this frame draws no veil, which repaints the
        // screen once to clear it, and no slide, which goes whole.
        let switch = self.switch.and_then(|(start, form)| {
            #[cfg(feature = "slide-hold")]
            let secs = self.slide.at(start, secs);
            crate::effects::switch_at(start, form, secs)
        });
        if switch.is_none() {
            self.switch = None;
        }
        let slide = switch.and_then(|f| f.slide);
        let splash = self.splash.look(Some(secs), term, self.motion);
        // The fireworks, at the cell size the frame is drawn at, a point a
        // pixel as the cell's height in points says.
        let cells = crate::fireworks::Cells {
            w: cell.px.0,
            h: cell.px.1,
            back: term.window_back().index(),
        };
        let shows = self.fireworks.frame(
            secs,
            term,
            cells,
            cell.px.1 / cell.height.0.max(1.0),
            self.motion,
        );
        let fireworks = match shows.as_slice() {
            [] => [None, None],
            [one] => [Some(*one), None],
            [.., a, b] => [Some(*a), Some(*b)],
        };
        Plan {
            scroll,
            // No trail while the screen slides, as sfte draws none.
            trail: trail.filter(|_| slide.is_none()),
            fade: switch.and_then(|f| f.veil),
            slide,
            blink: self.blink,
            splash,
            fireworks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TrailPick;
    use baud_core::Owned;
    use baud_domain::{BytePos, FrameNo, ImageBudget, KittyId, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    const CELL: Cell = Cell {
        px: (10.0, 20.0),
        height: Points(20.0),
    };

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    fn rect(r: Row, c: Col, _: CursorStyle) -> Rect {
        let (x, y) = (c.index() as f32 * 10.0, r.index() as f32 * 20.0);
        Rect {
            left: x,
            top: y,
            right: x + 10.0,
            bottom: y + 20.0,
        }
    }

    fn widths() -> baud_render::Widths {
        crate::reveal::widths(crate::reveal::Scale(1.0))
    }

    /// **An idle screen bounds no wait and owes no frame**: nothing blinks,
    /// no trail is in flight, the thumb is hidden. This is what keeps an
    /// idle Linux window from waking. Perturbation: give an unseen screen a
    /// blink flip -- `deadline` names one and the first assertion fails.
    #[test]
    fn an_idle_screen_waits_for_nothing() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut fx = Timed::new(t0, TrailPick::Default, ScreenTransition::Off);
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        for ms in [0u64, 7, 401, 5000] {
            let now = t0 + Duration::from_millis(ms);
            assert_eq!(fx.deadline(now), None, "{ms}");
            assert_eq!(fx.turn(now, &mut t), Owes::Nothing, "{ms}");
        }
    }

    /// **The launch splash on Linux, as on macOS**: nothing of it until the
    /// window is shown, owing nothing and setting no deadline; the first
    /// frame callback owes a frame, once, and the plasma runs from the plan
    /// after it, owing every turn whatever the shell draws, and is gone
    /// half a second in, owing nothing after; and a reload to off takes it
    /// down with one whole frame. Without `with_splash` there is none.
    #[test]
    fn the_splash_waits_for_the_first_frame_then_plays_and_goes() {
        use crate::config::LaunchSplash;
        use baud_render::SplashPhase;
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let off = TrailPick::Named(CursorTrail::Off);
        let mut none = Timed::new(t0, off, ScreenTransition::Off);
        assert_eq!(none.plan(t0, &t, rect, CELL, widths()).splash, None);
        assert_eq!(none.shown(), Owes::Nothing);
        let mut fx = Timed::new(t0, off, ScreenTransition::Off).with_splash(LaunchSplash::On);
        assert_eq!(fx.plan(t0, &t, rect, CELL, widths()).splash, None);
        let later = t0 + Duration::from_millis(300);
        assert_eq!(fx.turn(later, &mut t), Owes::Nothing, "waiting");
        assert_eq!(fx.deadline(later), None);
        assert_eq!(fx.plan(later, &t, rect, CELL, widths()).splash, None);
        assert_eq!(fx.shown(), Owes::Frame, "the first frame callback");
        assert_eq!(fx.shown(), Owes::Nothing, "once");
        let first = fx.plan(later, &t, rect, CELL, widths()).splash;
        assert_eq!(first.map(|l| l.phase), Some(SplashPhase::Plasma(0.0)));
        let soon = later + Duration::from_millis(100);
        assert_eq!(fx.turn(soon, &mut t), Owes::Frame, "every turn");
        feed(&mut t, b"$ ");
        let shown = fx.plan(soon, &t, rect, CELL, widths()).splash;
        assert!(
            matches!(shown.map(|l| l.phase), Some(SplashPhase::Plasma(a)) if a > 0.09),
            "over the prompt, still playing: {shown:?}"
        );
        let gone = Duration::from_secs_f64(crate::splash::gone());
        let before = later + gone - Duration::from_millis(5);
        assert_eq!(fx.turn(before, &mut t), Owes::Frame);
        let last = fx.plan(before, &t, rect, CELL, widths()).splash;
        assert!(
            matches!(last.map(|l| l.phase), Some(SplashPhase::Plasma(_))),
            "{last:?}"
        );
        let at = later + gone + Duration::from_millis(1);
        assert_eq!(fx.plan(at, &t, rect, CELL, widths()).splash, None);
        assert_eq!(
            fx.turn(at + Duration::from_millis(10), &mut t),
            Owes::Nothing
        );
        let mut blank = Owned::new(&WS);
        let b = Term::new(blank.storage(), &WS);
        let mut held = Timed::new(t0, off, ScreenTransition::Off).with_splash(LaunchSplash::On);
        let _ = held.shown();
        let first = held.plan(t0, &b, rect, CELL, widths()).splash;
        assert_eq!(first.map(|l| l.phase), Some(SplashPhase::Plasma(0.0)));
        assert_eq!(held.set_splash(LaunchSplash::On), Owes::Nothing);
        assert_eq!(held.set_splash(LaunchSplash::Off), Owes::Full);
        assert_eq!(held.plan(t0, &b, rect, CELL, widths()).splash, None);
        assert_eq!(held.deadline(t0), None);
    }

    /// **A window's splash waits on nothing and changes nothing but the
    /// logo**, the first window's and every one opened after it: the same
    /// keys and the same output, a prompt, more lines than the screen
    /// holds and a cursor position report, go to a window with the splash
    /// on and one with it off, frame by frame at 120 Hz. Each key's bytes
    /// and the report reach the shell on the frame they were sent, every
    /// frame plans the same but for the logo, the window with the splash
    /// owes a frame whenever the other does, and the grid and the history
    /// end byte for byte the same. The control: the keys and the output
    /// land while the plasma plays.
    #[test]
    fn a_windows_splash_waits_on_nothing_and_changes_only_the_logo() {
        use crate::config::LaunchSplash;
        use baud_domain::{Back, Codepoint, Content, KeyCode, KeyEvent, Modifiers};
        /// What a window did: the bytes sent to its shell and the frame
        /// each was sent on, each frame's plan without the logo, its grid
        /// and its history.
        #[derive(Debug, PartialEq)]
        struct Did {
            sent: Vec<(u32, Vec<u8>)>,
            plans: Vec<Plan>,
            grid: Vec<Vec<Content>>,
            history: Vec<Vec<Content>>,
        }
        let rank = |o: Owes| match o {
            Owes::Nothing => 0,
            Owes::Frame => 1,
            Owes::Full => 2,
        };
        let lines: Vec<u8> = (0..15)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        let window = |setting: LaunchSplash| {
            let t0 = Instant::now();
            let mut s = Owned::new(&WS);
            let mut t = Term::new(s.storage(), &WS);
            let off = TrailPick::Named(CursorTrail::Off);
            let mut fx = Timed::new(t0, off, ScreenTransition::Off).with_splash(setting);
            let mut sent = Vec::new();
            let mut plans = Vec::new();
            let mut owed = Vec::new();
            let mut logo = Vec::new();
            for f in 0..120_u32 {
                let now = t0 + Duration::from_secs_f64(f64::from(f) / 120.0);
                if f == 1 {
                    let _ = fx.shown();
                }
                let mut sink = |b: &[u8]| sent.push((f, b.to_vec()));
                let landed = match f {
                    10 | 30 | 50 => {
                        let c = Codepoint::from_char('k');
                        let key = KeyEvent {
                            text: Some(c),
                            ..KeyEvent::press(KeyCode::Char(c), Modifiers::NONE)
                        };
                        let _ = crate::reads::input(&mut t, |t| t.key(key), &mut sink);
                        true
                    }
                    12 => {
                        feed(&mut t, b"$ ");
                        true
                    }
                    32 => {
                        feed(&mut t, &lines);
                        true
                    }
                    52 => {
                        feed(&mut t, b"\x1b[6n");
                        crate::reads::owed(&mut t, &mut sink);
                        true
                    }
                    _ => false,
                };
                owed.push(rank(fx.turn(now, &mut t)));
                let plan = fx.plan(now, &t, rect, CELL, widths());
                if landed {
                    logo.push(plan.splash.is_some());
                }
                plans.push(Plan {
                    splash: None,
                    ..plan
                });
            }
            let view = t.view(t.window_back());
            let grid = (0..view.rows().index())
                .map(|r| {
                    view.row(Row::from_index(r))
                        .iter()
                        .map(|c| c.content)
                        .collect()
                })
                .collect();
            let back = t.scrollback();
            let history = (0..back.len())
                .map(|i| {
                    back.line(Back::of(i))
                        .map(|(cells, _)| cells.iter().map(|c| c.content).collect())
                        .unwrap_or_default()
                })
                .collect();
            let did = Did {
                sent,
                plans,
                grid,
                history,
            };
            (did, owed, logo)
        };
        let (on, owed_on, logo) = window(LaunchSplash::On);
        let (off, owed_off, none) = window(LaunchSplash::Off);
        assert_eq!(logo, [true; 6], "the control: all land under the logo");
        assert_eq!(none, [false; 6], "and none in the window without it");
        assert!(
            on.sent.iter().any(|(f, b)| *f == 52 && b.ends_with(b"R")),
            "the control: the report is sent: {:?}",
            on.sent
        );
        assert!(!on.history.is_empty(), "the control: lines retired");
        assert_eq!(on, off);
        for (f, (a, b)) in owed_on.iter().zip(&owed_off).enumerate() {
            assert!(a >= b, "frame {f}: owed {a} with the splash, {b} without");
        }
    }

    /// **iTerm2's fireworks on Linux, as on macOS**: a request read before
    /// the first frame is not lost -- the turn owes a frame for it, and the
    /// plan carries a show whose first burst is at the request's cell,
    /// though the cursor has moved on; every turn owes a frame through the
    /// show's hold and fade, each plan carrying it; after its end the plan
    /// carries none and nothing more is owed. The control: the same screen
    /// with no request owes nothing. A window looking back three rows
    /// shows the request's cell three rows down, and the burst is there.
    #[test]
    fn the_fireworks_play_from_a_request_before_the_first_frame_and_go() {
        use baud_render::Pt;
        use baud_render::fireworks::{FADE, HOLD};
        let t0 = Instant::now();
        let off = TrailPick::Named(CursorTrail::Off);
        let req = b"\x1b[3;5H\x1b]1337;RequestAttention=fireworks\x07\x1b[7;20H";
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut quiet = Owned::new(&WS);
        let mut q = Term::new(quiet.storage(), &WS);
        let mut fx = Timed::new(t0, off, ScreenTransition::Off);
        let mut none = Timed::new(t0, off, ScreenTransition::Off);
        feed(&mut t, req);
        feed(&mut q, b"\x1b[3;5H\x1b[7;20H");
        assert_eq!(none.turn(t0, &mut q), Owes::Nothing, "the control");
        assert_eq!(
            fx.turn(t0, &mut t),
            Owes::Frame,
            "the request owes its first frame"
        );
        let first = fx.plan(t0, &t, rect, CELL, widths()).fireworks;
        let show = first[0].expect("a show on the first frame");
        assert_eq!(first[1], None);
        assert_eq!(show.age, 0.0);
        // Row 3, column 5: the centre of the cell at (2, 4), 10 by 20.
        assert_eq!(show.origins[0], Pt { x: 45.0, y: 50.0 }, "the request's");
        assert_eq!(show.hole, Pt { x: 195.0, y: 130.0 }, "the cursor's now");
        assert_eq!(show.scale, 1.0);
        let quiet_plan = none.plan(t0, &q, rect, CELL, widths());
        assert_eq!(quiet_plan.fireworks, [None, None], "the control");
        let end = Duration::from_secs_f64(f64::from(HOLD + FADE));
        let step = Duration::from_millis(16);
        let mut at = step;
        while at + Duration::from_millis(1) < end {
            assert_eq!(fx.turn(t0 + at, &mut t), Owes::Frame, "{at:?}");
            let p = fx.plan(t0 + at, &t, rect, CELL, widths());
            assert!(p.fireworks[0].is_some(), "{at:?}");
            at += step;
        }
        let after = t0 + end + Duration::from_millis(1);
        let gone = fx.plan(after, &t, rect, CELL, widths());
        assert_eq!(gone.fireworks, [None, None], "gone at the end");
        assert_eq!(fx.turn(after, &mut t), Owes::Nothing, "and owes nothing");
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let lines: Vec<u8> = (0..30)
            .flat_map(|i| format!("{i}\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        feed(&mut t, req);
        t.set_window_back(baud_domain::Rows::of_count(3));
        let mut fx = Timed::new(t0, off, ScreenTransition::Off);
        let back = fx.plan(t0, &t, rect, CELL, widths()).fireworks[0].expect("a show");
        assert_eq!(back.origins[0], Pt { x: 45.0, y: 110.0 }, "three rows down");
    }

    /// **Blinking text on screen wakes the wait at each flip and repaints
    /// whole there, and only there.** Slow-only text wakes at the slow
    /// flips, 400 ms apart, not at the rapid ones. Perturbation: answer
    /// `Owes::Frame` on a flip -- row damage would leave the cells as they
    /// were, and the flip's assertion fails.
    #[test]
    fn blinking_text_wakes_at_its_own_flips_and_repaints_whole() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b[5mslow\x1b[m");
        let mut fx = Timed::new(
            t0,
            TrailPick::Named(CursorTrail::Off),
            ScreenTransition::Off,
        );
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        fx.drawn(Blinking::SLOW, RegisBlink::Off);
        let wake = fx.deadline(t0).expect("a slow flip ahead");
        let after = wake.saturating_duration_since(t0);
        assert!(
            after >= Duration::from_millis(400) && after <= Duration::from_millis(402),
            "{after:?}"
        );
        assert_eq!(
            fx.turn(t0 + Duration::from_millis(200), &mut t),
            Owes::Nothing
        );
        assert_eq!(fx.turn(wake, &mut t), Owes::Full, "the slow flip");
        assert_eq!(fx.turn(wake, &mut t), Owes::Nothing, "once");
        // Every later wake, through the clock's round trip to an instant
        // and back, finds its flip done: none is a wake for nothing.
        fx.drawn(Blinking::BOTH, RegisBlink::Off);
        let mut now = wake;
        for n in 0..3000 {
            let next = fx.deadline(now).expect("the next");
            assert!(next > now, "{n}");
            assert_eq!(fx.turn(next, &mut t), Owes::Full, "flip {n} at {next:?}");
            now = next;
        }
    }

    /// **A kitty animation wakes the wait at each frame's gap and repaints
    /// whole there, and holds under reduced motion**, as the macOS tick
    /// steps it and Reduce Motion holds it (`access::image_motion`): a 1x1
    /// picture of two frames 100 ms apart, run. Perturbations: leave
    /// `animate_images` out of `turn` -- the frame owes nothing; leave the
    /// frames out of the wait -- no wake is named.
    #[test]
    fn a_kitty_animation_wakes_at_each_gap_and_holds_under_reduced_motion() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS).with_images(ImageBudget::of(4 << 20));
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b_Ga=T,f=24,s=1,v=1,i=1,q=2;AAAA\x1b\\");
        feed(&mut t, b"\x1b_Ga=f,f=24,s=1,v=1,i=1,z=100,q=2;////\x1b\\");
        feed(
            &mut t,
            b"\x1b_Ga=a,i=1,r=1,z=100\x1b\\\x1b_Ga=a,i=1,s=3\x1b\\",
        );
        let off = TrailPick::Named(CursorTrail::Off);
        let mut fx = Timed::new(t0, off, ScreenTransition::Off);
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        let shown = |t: &Term<'_>| {
            let img = t.images().kitty_image(KittyId::of(1)).expect("stored");
            t.images().kitty_frames(img).expect("frames").1
        };
        assert_eq!(fx.turn(t0, &mut t), Owes::Nothing, "stamped, not yet due");
        let wake = fx.frame_deadline(t0, &t).expect("the second frame ahead");
        let after = wake.saturating_duration_since(t0);
        assert!(
            after >= Duration::from_millis(99) && after <= Duration::from_millis(101),
            "{after:?}"
        );
        assert_eq!(fx.turn(wake, &mut t), Owes::Full, "the second frame");
        assert_eq!(shown(&t), FrameNo::of(2));
        let back = fx.frame_deadline(wake, &t).expect("the first again");
        assert_eq!(fx.turn(back, &mut t), Owes::Full, "looped");
        assert_eq!(shown(&t), FrameNo::ROOT);
        let _ = fx.set_motion(SystemMotion::Reduced, back);
        let later = back + Duration::from_millis(500);
        assert_eq!(fx.turn(later, &mut t), Owes::Nothing, "held");
        assert_eq!(fx.frame_deadline(later, &t), None, "nothing to wake for");
        assert_eq!(shown(&t), FrameNo::ROOT, "held where it was");
    }

    /// **`[images] animation` decides over the motion preference**:
    /// `"on"` plays a kitty animation under reduced motion and `"off"`
    /// holds it under full motion, at the frame it shows, and `"os"` gives
    /// the motion preference back. Perturbations: leave `animation` out of
    /// `turn`'s call -- `"on"` holds under reduced motion; leave
    /// `set_image_animation` unstored -- the same.
    #[test]
    fn the_animation_setting_plays_or_holds_whatever_the_motion() {
        use crate::config::ImageAnimation;
        let t0 = Instant::now();
        let mut s = Owned::new(&WS).with_images(ImageBudget::of(4 << 20));
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b_Ga=T,f=24,s=1,v=1,i=1,q=2;AAAA\x1b\\");
        feed(&mut t, b"\x1b_Ga=f,f=24,s=1,v=1,i=1,z=100,q=2;////\x1b\\");
        feed(
            &mut t,
            b"\x1b_Ga=a,i=1,r=1,z=100\x1b\\\x1b_Ga=a,i=1,s=3\x1b\\",
        );
        let off = TrailPick::Named(CursorTrail::Off);
        let mut fx = Timed::new(t0, off, ScreenTransition::Off);
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        let shown = |t: &Term<'_>| {
            let img = t.images().kitty_image(KittyId::of(1)).expect("stored");
            t.images().kitty_frames(img).expect("frames").1
        };
        let _ = fx.set_motion(SystemMotion::Reduced, t0);
        fx.set_image_animation(ImageAnimation::On);
        assert_eq!(fx.turn(t0, &mut t), Owes::Nothing, "stamped, not yet due");
        let wake = fx
            .frame_deadline(t0, &t)
            .expect("plays under reduced motion");
        assert_eq!(fx.turn(wake, &mut t), Owes::Full, "the second frame");
        assert_eq!(shown(&t), FrameNo::of(2));
        let _ = fx.set_motion(SystemMotion::Full, wake);
        fx.set_image_animation(ImageAnimation::Off);
        let later = wake + Duration::from_millis(500);
        assert_eq!(
            fx.turn(later, &mut t),
            Owes::Nothing,
            "held under full motion"
        );
        assert_eq!(fx.frame_deadline(later, &t), None, "nothing to wake for");
        assert_eq!(shown(&t), FrameNo::of(2), "held at the frame it showed");
        fx.set_image_animation(ImageAnimation::Os);
        let next = later + Duration::from_millis(1);
        let _ = fx.turn(next, &mut t);
        let wake = fx
            .frame_deadline(next, &t)
            .expect("full motion plays again");
        assert_eq!(fx.turn(wake, &mut t), Owes::Full, "the first again");
        assert_eq!(shown(&t), FrameNo::ROOT);
    }

    /// **A ReGIS page's blinking pixels wake the wait at the slow flips and
    /// repaint whole there, with no text blinking**, as the macOS tick
    /// flips them. The control is a steady drawing, which wakes for
    /// nothing. Perturbations: leave the page out of `deadline` -- no wake
    /// is named and the first `expect` fails; leave `set_regis_blink` out
    /// of `turn` -- the flip owes nothing.
    #[test]
    fn a_blinking_regis_page_wakes_at_the_slow_flips() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS).with_images(ImageBudget::of(4 << 20));
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1bPpP[0,0]V[9,0]\x1b\\");
        assert_eq!(t.regis_blink(), RegisBlink::Off, "a steady drawing");
        let off = TrailPick::Named(CursorTrail::Off);
        let mut fx = Timed::new(t0, off, ScreenTransition::Off);
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        fx.drawn(Blinking::UNSEEN, t.regis_blink());
        assert_eq!(fx.deadline(t0), None, "nothing blinks");
        feed(&mut t, b"\x1bPpW(I2,A1)P[0,2]V[9,2]\x1b\\");
        assert_eq!(t.regis_blink(), RegisBlink::On);
        fx.drawn(Blinking::UNSEEN, t.regis_blink());
        let wake = fx.deadline(t0).expect("a slow flip ahead");
        let after = wake.saturating_duration_since(t0);
        assert!(
            after >= Duration::from_millis(400) && after <= Duration::from_millis(402),
            "{after:?}"
        );
        assert_eq!(
            fx.turn(t0 + Duration::from_millis(200), &mut t),
            Owes::Nothing
        );
        assert_eq!(fx.turn(wake, &mut t), Owes::Full, "hidden");
        assert_eq!(fx.turn(wake, &mut t), Owes::Nothing, "once");
        let next = fx.deadline(wake).expect("the next flip");
        assert_eq!(fx.turn(next, &mut t), Owes::Full, "shown again");
    }

    /// **Under Reduce Motion a trail named in the config fades on a hidden
    /// cursor as it does under full motion**, and the default, which is
    /// off there, draws nothing to fade: a long move taken, then the cursor
    /// hidden for 100 ms, a quarter of the fade. Perturbation: the chase
    /// snapped on a hidden frame under Reduce Motion -- the named arm's
    /// hidden frame is none.
    #[test]
    fn a_named_trail_fades_on_a_hide_under_reduce_motion() {
        let opacity = |l: Option<TrailLook>| match l {
            Some(TrailLook::Vfx(v)) => Some(v.emitter.opacity.get()),
            Some(TrailLook::Smear(s)) => Some(s.opacity.get()),
            _ => None,
        };
        for pick in [TrailPick::Named(CursorTrail::Plasma), TrailPick::Default] {
            for motion in [SystemMotion::Full, SystemMotion::Reduced] {
                let case = format!("{pick:?} under {motion:?}");
                let t0 = Instant::now();
                let mut s = Owned::new(&WS);
                let mut t = Term::new(s.storage(), &WS);
                let mut fx = Timed::new(t0, pick, ScreenTransition::Off);
                let _ = fx.set_motion(motion, t0);
                let _ = fx.plan(t0, &t, rect, CELL, widths());
                fx.keyed(Some(t0));
                feed(&mut t, b"\x1b[8;30H");
                let at = t0 + Duration::from_millis(2);
                let _ = fx.plan(at, &t, rect, CELL, widths());
                let drawn = pick != TrailPick::Default || motion == SystemMotion::Full;
                let Some(dwell) = fx.deadline(at) else {
                    assert!(!drawn, "{case}: no dwell");
                    continue;
                };
                let lit = fx.plan(dwell, &t, rect, CELL, widths()).trail;
                // Not straight after the move, where the hide would place
                // the cursor (`Term::placed_cursor`), as helix's does.
                feed(&mut t, b"\x1b[m\x1b[?25l");
                let later = dwell + Duration::from_millis(100);
                let hidden = fx.plan(later, &t, rect, CELL, widths()).trail;
                match drawn {
                    true => {
                        assert_eq!(opacity(lit), Some(255), "{case}: lit, whole");
                        assert_eq!(opacity(hidden), Some(191), "{case}: three quarters");
                    }
                    false => assert_eq!((lit, hidden), (None, None), "{case}"),
                }
            }
        }
    }

    /// **A long cursor move trails: the dwell is a deadline, the flight
    /// owes every frame, and the landing frees the wait.** The default
    /// trail is an effect, the plasma, which the CPU path draws as itself;
    /// it owes frames until it is out, after its quad lands. The control
    /// is a one-cell move, which snaps and owes nothing. Perturbation:
    /// build the chase from the quad the effect rides -- the plasma
    /// assertion fails.
    #[test]
    fn a_long_move_trails_as_the_effect_and_lands() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut fx = Timed::new(t0, TrailPick::Default, ScreenTransition::Off);
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        feed(&mut t, b"\x1b[2C");
        let _ = fx.plan(t0 + Duration::from_millis(1), &t, rect, CELL, widths());
        assert_eq!(fx.deadline(t0), None, "a short move snaps");
        fx.keyed(Some(t0));
        feed(&mut t, b"\x1b[8;30H");
        let at = t0 + Duration::from_millis(2);
        let _ = fx.plan(at, &t, rect, CELL, widths());
        let dwell = fx.deadline(at).expect("the dwell");
        assert_eq!(fx.turn(dwell, &mut t), Owes::Frame, "the dwell ran out");
        let p = fx.plan(dwell, &t, rect, CELL, widths());
        assert!(
            matches!(
                p.trail,
                Some(TrailLook::Vfx(v)) if v.effect == baud_render::effect::Effect::Plasma
            ),
            "{:?}",
            p.trail
        );
        let mut now = dwell;
        let mut frames = 0;
        while fx.turn(now, &mut t) != Owes::Nothing {
            let _ = fx.plan(now, &t, rect, CELL, widths());
            now += Duration::from_millis(16);
            frames += 1;
            assert!(frames < 200, "the trail never landed");
        }
        assert!(frames > 1, "{frames}");
        assert_eq!(fx.deadline(now), None);
    }

    /// **A program's long move lights no effect**: the default plasma, the
    /// same move as [`a_long_move_trails_as_the_effect_and_lands`] with no
    /// key behind it -- the caret's last key at start-up, none -- draws
    /// nothing and owes nothing once the dwell runs out. A key 40 ms before
    /// the next move lights it. Perturbation: drop `Timed::keyed`'s
    /// forwarding -- the last assertion fails.
    #[test]
    fn a_programs_long_move_lights_no_effect() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut fx = Timed::new(t0, TrailPick::Default, ScreenTransition::Off);
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        fx.keyed(None);
        feed(&mut t, b"\x1b[8;30H");
        let at = t0 + Duration::from_millis(400);
        let _ = fx.plan(at, &t, rect, CELL, widths());
        let dwell = fx.deadline(at).expect("the dwell");
        assert_eq!(fx.turn(dwell, &mut t), Owes::Frame, "the dwell ran out");
        assert_eq!(
            fx.plan(dwell, &t, rect, CELL, widths()).trail,
            None,
            "unlit"
        );
        assert_eq!(
            fx.turn(dwell + Duration::from_millis(16), &mut t),
            Owes::Nothing
        );
        let at = dwell + Duration::from_millis(100);
        fx.keyed(Some(at - Duration::from_millis(40)));
        feed(&mut t, b"\x1b[1;1H");
        let _ = fx.plan(at, &t, rect, CELL, widths());
        let dwell = fx.deadline(at).expect("the dwell");
        let _ = fx.turn(dwell, &mut t);
        assert!(
            fx.plan(dwell, &t, rect, CELL, widths()).trail.is_some(),
            "a move 40 ms after input lights"
        );
    }

    /// **Reduced motion holds blinking text shown and turns the default
    /// trail off, as Reduce Motion does on macOS**: no blink flip bounds
    /// the wait or owes a frame, a trail in flight is cut with a frame
    /// owed to clear it, and a trail the config names is kept; full motion
    /// again blinks and trails. The controls are the same screen at full
    /// motion. Perturbations: leave `motion` unset in `set_motion` -- the
    /// held-shown assertion fails; take the trail full motion gives --
    /// the default's assertion fails.
    #[test]
    fn reduced_motion_holds_text_shown_and_turns_the_default_trail_off() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b[5mslow\x1b[m");
        let mut fx = Timed::new(t0, TrailPick::Default, ScreenTransition::Off);
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        fx.drawn(Blinking::SLOW, RegisBlink::Off);
        assert!(fx.deadline(t0).is_some(), "the control: it blinks");
        assert_eq!(fx.trail(), CursorTrail::default(), "the control: trailed");
        // A long move, in flight when the preference changes.
        fx.keyed(Some(t0));
        feed(&mut t, b"\x1b[8;30H");
        let at = t0 + Duration::from_millis(2);
        let _ = fx.plan(at, &t, rect, CELL, widths());
        let dwell = fx.deadline(at).expect("the dwell");
        assert_eq!(fx.turn(dwell, &mut t), Owes::Frame);
        let _ = fx.plan(dwell, &t, rect, CELL, widths());
        assert_eq!(
            fx.set_motion(SystemMotion::Reduced, dwell),
            Owes::Frame,
            "the trail in flight is cut and cleared"
        );
        assert_eq!(fx.trail(), CursorTrail::Off);
        assert_eq!(fx.deadline(dwell), None, "no flip, no dwell");
        for ms in [0u64, 250, 401, 801, 1201] {
            let now = dwell + Duration::from_millis(ms);
            let p = fx.plan(now, &t, rect, CELL, widths());
            assert_eq!(p.blink, BlinkPhases::SHOWN, "held shown at {ms}");
            assert_eq!(p.trail, None, "{ms}");
            assert_eq!(fx.turn(now, &mut t), Owes::Nothing, "{ms}");
        }
        assert_eq!(
            fx.set_motion(SystemMotion::Reduced, dwell),
            Owes::Nothing,
            "no change"
        );
        let named = TrailPick::Named(CursorTrail::Ghost);
        let mut kept = Timed::new(t0, named, ScreenTransition::Off);
        assert_eq!(kept.set_motion(SystemMotion::Reduced, t0), Owes::Nothing);
        assert_eq!(kept.trail(), CursorTrail::Ghost, "a named trail is kept");
        let back = dwell + Duration::from_secs(2);
        assert_eq!(fx.set_motion(SystemMotion::Full, back), Owes::Nothing);
        assert_eq!(fx.trail(), CursorTrail::default());
        assert!(fx.deadline(back).is_some(), "it blinks again");
    }

    /// **A switch slides the new screen in when `[screen] transition` is
    /// on, and only then**, toward the screen it brought up under the faint
    /// veil, which fades off once it has landed, a quarter of a second on;
    /// it owes frames until that settles 0.15 s later.
    #[test]
    fn a_switch_slides_in_only_when_the_transition_is_on() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        for (on, want) in [(ScreenTransition::On, true), (ScreenTransition::Off, false)] {
            let mut fx = Timed::new(t0, TrailPick::Named(CursorTrail::Off), on);
            let _ = fx.switched(t0, baud_core::Screen::Alternate);
            let p = fx.plan(t0 + Duration::from_millis(50), &t, rect, CELL, widths());
            assert_eq!(
                p.slide.map(|s| s.toward),
                want.then_some(baud_render::Toward::Alternate),
                "{on:?}"
            );
            assert_eq!(
                p.fade,
                want.then_some(Transition::faint(0.0)),
                "{on:?}: the faint veil while it slides"
            );
            assert_eq!(
                fx.turn(t0 + Duration::from_millis(60), &mut t) == Owes::Frame,
                want
            );
            let p = fx.plan(t0 + Duration::from_millis(200), &t, rect, CELL, widths());
            assert_eq!(p.slide.is_some(), want, "{on:?}: still sliding at 0.2 s");
            let p = fx.plan(t0 + Duration::from_millis(325), &t, rect, CELL, widths());
            assert_eq!(p.slide, None, "{on:?}: landed");
            assert_eq!(
                p.fade.map(|f| f.from),
                want.then_some(baud_render::VeilFrom::Faint),
                "{on:?}: the faint veil fading off"
            );
            let p = fx.plan(t0 + Duration::from_millis(410), &t, rect, CELL, widths());
            assert_eq!((p.slide, p.fade), (None, None), "settled");
            assert_eq!(
                fx.turn(t0 + Duration::from_millis(420), &mut t),
                Owes::Nothing
            );
        }
    }

    /// **A held slide stays at its start**: with the clock held, a switch
    /// slides at progress 0 under the faint veil however long after it
    /// began, and owes a frame each turn; the clock that runs, the control,
    /// has landed and settled by then. `BAUD_SLIDE_HOLD=1` holds it, and
    /// any other value, or none, runs it.
    #[cfg(feature = "slide-hold")]
    #[test]
    fn a_held_slide_stays_at_progress_zero() {
        use std::ffi::OsStr;
        assert_eq!(SlideClock::told(Some(OsStr::new("1"))), SlideClock::Held);
        for v in [None, Some("0"), Some(""), Some("yes"), Some("11")] {
            assert_eq!(
                SlideClock::told(v.map(OsStr::new)),
                SlideClock::Runs,
                "{v:?}"
            );
        }
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let start = Some(baud_render::ScreenSlide {
            progress: 0.0,
            toward: baud_render::Toward::Alternate,
        });
        for (clock, held) in [(SlideClock::Held, true), (SlideClock::Runs, false)] {
            let mut fx = Timed::new(t0, TrailPick::Named(CursorTrail::Off), ScreenTransition::On);
            fx.slide = clock;
            let _ = fx.switched(t0, baud_core::Screen::Alternate);
            let p = fx.plan(t0, &t, rect, CELL, widths());
            assert_eq!(p.slide, start, "{clock:?}: at the switch");
            for ms in [500u64, 10_000] {
                let now = t0 + Duration::from_millis(ms);
                let p = fx.plan(now, &t, rect, CELL, widths());
                assert_eq!(
                    (p.slide, p.fade),
                    if held {
                        (start, Some(Transition::faint(0.0)))
                    } else {
                        (None, None)
                    },
                    "{clock:?} at {ms} ms"
                );
                let owes = fx.turn(now + Duration::from_millis(1), &mut t);
                assert_eq!(owes == Owes::Frame, held, "{clock:?} at {ms} ms: {owes:?}");
            }
        }
    }

    /// **Back to the primary the slide comes from above, and under reduced
    /// motion a switch is veiled instead**, the veil settling in 0.15 s.
    #[test]
    fn a_switch_back_slides_down_and_reduced_motion_veils() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let t = Term::new(s.storage(), &WS);
        let mut fx = Timed::new(t0, TrailPick::Named(CursorTrail::Off), ScreenTransition::On);
        let _ = fx.switched(t0, baud_core::Screen::Normal);
        let p = fx.plan(t0 + Duration::from_millis(50), &t, rect, CELL, widths());
        assert_eq!(
            p.slide.map(|s| s.toward),
            Some(baud_render::Toward::Primary)
        );
        let _ = fx.set_motion(SystemMotion::Reduced, t0 + Duration::from_millis(500));
        let t1 = t0 + Duration::from_millis(600);
        let _ = fx.switched(t1, baud_core::Screen::Alternate);
        let p = fx.plan(t1 + Duration::from_millis(50), &t, rect, CELL, widths());
        assert_eq!(p.slide, None, "no slide under reduced motion");
        assert_eq!(
            p.fade.map(|f| f.from),
            Some(baud_render::VeilFrom::Fade),
            "the fade instead"
        );
        let p = fx.plan(t1 + Duration::from_millis(160), &t, rect, CELL, widths());
        assert_eq!(p.fade, None, "the veil settled");
    }

    /// **A config read again switches the trail and the transition**, as
    /// a change of the motion preference switches the trail: the new pick
    /// under the motion in force, and a switch that fades from then on.
    /// The control is the same switch before the reload.
    #[test]
    fn a_reload_switches_the_trail_and_the_transition() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let t = Term::new(s.storage(), &WS);
        let mut fx = Timed::new(t0, TrailPick::Default, ScreenTransition::Off);
        assert_eq!(fx.trail(), CursorTrail::default(), "the control: trailed");
        let _ = fx.switched(t0, baud_core::Screen::Alternate);
        let p = fx.plan(t0 + Duration::from_millis(50), &t, rect, CELL, widths());
        assert_eq!(p.slide, None, "the control: no slide");
        let _ = fx.set_config(
            TrailPick::Named(CursorTrail::Off),
            ScreenTransition::On,
            t0 + Duration::from_millis(300),
        );
        assert_eq!(fx.trail(), CursorTrail::Off);
        let t1 = t0 + Duration::from_millis(400);
        let _ = fx.switched(t1, baud_core::Screen::Alternate);
        let p = fx.plan(t1 + Duration::from_millis(50), &t, rect, CELL, widths());
        assert!(p.slide.is_some(), "the switch slides now");
    }

    /// **The ghost sweeps on any move and lands.** The control is no move.
    #[test]
    fn the_ghost_sweeps_on_a_move_and_lands() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut fx = Timed::new(
            t0,
            TrailPick::Named(CursorTrail::Ghost),
            ScreenTransition::Off,
        );
        let p = fx.plan(t0, &t, rect, CELL, widths());
        assert_eq!(p.trail, None, "no move");
        feed(&mut t, b"x");
        let p = fx.plan(t0 + Duration::from_millis(5), &t, rect, CELL, widths());
        assert!(
            matches!(p.trail, Some(TrailLook::Ghost(_))),
            "{:?}",
            p.trail
        );
        assert_eq!(fx.turn(t0 + Duration::from_millis(6), &mut t), Owes::Frame);
        let p = fx.plan(t0 + Duration::from_millis(400), &t, rect, CELL, widths());
        assert_eq!(p.trail, None, "landed");
        assert_eq!(
            fx.turn(t0 + Duration::from_millis(401), &mut t),
            Owes::Nothing
        );
    }

    /// **Looking back drops a ghost in flight**, as the chase arms snap:
    /// its sweep names live rows, and the window shows the history there.
    /// The control is the same sweep at the same moment on the live
    /// screen. Perturbation: keep the ghost while the window looks back --
    /// the trail is drawn over the history and the second assertion fails.
    #[test]
    fn looking_back_drops_the_ghost_in_flight() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let lines: Vec<u8> = (0..30)
            .flat_map(|i| format!("{i}\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        let mut fx = Timed::new(
            t0,
            TrailPick::Named(CursorTrail::Ghost),
            ScreenTransition::Off,
        );
        let _ = fx.plan(t0, &t, rect, CELL, widths());
        feed(&mut t, b"x");
        let p = fx.plan(t0 + Duration::from_millis(5), &t, rect, CELL, widths());
        assert!(matches!(p.trail, Some(TrailLook::Ghost(_))), "the control");
        t.set_window_back(baud_domain::Rows::of_count(3));
        let p = fx.plan(t0 + Duration::from_millis(10), &t, rect, CELL, widths());
        assert_eq!(p.trail, None, "nothing of it over the history");
        assert_eq!(
            fx.turn(t0 + Duration::from_millis(11), &mut t),
            Owes::Nothing,
            "and no frame owed for it"
        );
        t.window_live();
        let p = fx.plan(t0 + Duration::from_millis(15), &t, rect, CELL, widths());
        assert_eq!(p.trail, None, "nor back on the live screen");
    }

    /// **Looking back drops every trail in flight, and the way back draws
    /// none stale**: the ghost, the two smears and the seven effects, each
    /// caught drawing a long move when the window scrolls back three lines.
    /// While it looks back no frame draws a trail, and no turn owes more
    /// than the same turn with the trail off; output then moves the cursor,
    /// and on the live screen again neither the first sighting nor the
    /// frames after the dwell trail from where the cursor was before the
    /// reader looked away. The controls: each trail draws on the live
    /// screen, and the window is still back after the output.
    /// Perturbations: the chase not snapped and the cursor followed while
    /// looking back -- every chase trail is drawn over the history; the
    /// snap alone removed -- the chase keeps the cell it last saw and
    /// trails from it once the dwell has passed on the way back; the ghost
    /// kept while looking back -- it is drawn over the history.
    #[test]
    fn looking_back_drops_every_trail_in_flight() {
        let ms = |n: u64| Duration::from_millis(n);
        let trails = [
            CursorTrail::Ghost,
            CursorTrail::Sweep,
            CursorTrail::Quad,
            CursorTrail::Fire,
            CursorTrail::Plasma,
            CursorTrail::Lightning,
            CursorTrail::Ice,
            CursorTrail::Ufo,
            CursorTrail::Orb,
            CursorTrail::Liquid,
        ];
        let run = |trail: CursorTrail, check: &mut dyn FnMut(&str, u64, &Plan, Owes)| {
            let t0 = Instant::now();
            let mut s = Owned::new(&WS);
            let mut t = Term::new(s.storage(), &WS);
            let lines: Vec<u8> = (0..30)
                .flat_map(|i| format!("{i}\r\n").into_bytes())
                .collect();
            feed(&mut t, &lines);
            let mut fx = Timed::new(t0, TrailPick::Named(trail), ScreenTransition::Off);
            let _ = fx.plan(t0, &t, rect, CELL, widths());
            fx.keyed(Some(t0));
            feed(&mut t, b"\x1b[1;30H");
            let _ = fx.plan(t0 + ms(5), &t, rect, CELL, widths());
            let p = fx.plan(t0 + ms(20), &t, rect, CELL, widths());
            let o = fx.turn(t0 + ms(21), &mut t);
            check("live", 20, &p, o);
            t.set_window_back(baud_domain::Rows::of_count(3));
            feed(&mut t, b"\x1b[10;1H");
            assert_eq!(t.window_back().index(), 3, "{trail:?}: still back");
            for n in [30, 45, 60] {
                let p = fx.plan(t0 + ms(n), &t, rect, CELL, widths());
                let o = fx.turn(t0 + ms(n + 1), &mut t);
                check("back", n, &p, o);
            }
            t.window_live();
            for n in [70, 85, 100, 130] {
                let p = fx.plan(t0 + ms(n), &t, rect, CELL, widths());
                let o = fx.turn(t0 + ms(n + 1), &mut t);
                check("returned", n, &p, o);
            }
        };
        // What each turn owes with the trail off, whatever else moves.
        let mut off = Vec::new();
        run(CursorTrail::Off, &mut |_, _, p, o| {
            assert_eq!(p.trail, None, "off draws none");
            off.push(o);
        });
        for trail in trails {
            let mut i = 0;
            run(trail, &mut |at, n, p, o| {
                match at {
                    "live" => assert!(p.trail.is_some(), "{trail:?}: the control, drawn live"),
                    _ => {
                        assert_eq!(p.trail, None, "{trail:?}: none {at} at {n} ms");
                        assert_eq!(o, off[i], "{trail:?}: owes {at} at {n} ms as off does");
                    }
                }
                i += 1;
            });
        }
    }

    /// A long move from (2, 3) to column 30 of row `to` after `setup`,
    /// drawn by a `ride` arm and a `stay` arm off one terminal -- the second
    /// read `ride` at launch and `stay` from a reload -- the frame 40 ms in,
    /// then a read of `scroll` and the frame 10 ms after: each arm's trail
    /// at both, and the terminal's status rows.
    fn ride_and_stay(
        setup: &[u8],
        to: Row,
        scroll: &[u8],
        trail: CursorTrail,
    ) -> ([[Option<TrailLook>; 2]; 2], baud_domain::Rows) {
        let ms = |n: u64| Duration::from_millis(n);
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, setup);
        feed(&mut t, b"\x1b[3;4H");
        let mut arms = [TrailScroll::Ride, TrailScroll::Stay].map(|key| {
            let mut fx = Timed::new(t0, TrailPick::Named(trail), ScreenTransition::Off)
                .with_trail_scroll(TrailScroll::Ride);
            fx.set_trail_scroll(key);
            fx
        });
        for fx in &mut arms {
            let _ = fx.plan(t0, &t, rect, CELL, widths());
        }
        feed(&mut t, format!("\x1b[{};31H", to.index() + 1).as_bytes());
        for fx in &mut arms {
            let _ = fx.plan(t0 + ms(2), &t, rect, CELL, widths());
        }
        let in_flight = arms
            .each_mut()
            .map(|fx| fx.plan(t0 + ms(40), &t, rect, CELL, widths()).trail);
        let before = crate::keep::Before::of(&t);
        feed(&mut t, scroll);
        for fx in &mut arms {
            fx.scrolled(&t, before);
        }
        let after = arms
            .each_mut()
            .map(|fx| fx.plan(t0 + ms(50), &t, rect, CELL, widths()).trail);
        ([in_flight, after], t.status_rows())
    }

    /// How far, pixels, y down, `ride`'s smear lies from `stay`'s, when every
    /// corner is moved by the same amount and none across.
    fn smear_dy(ride: Option<TrailLook>, stay: Option<TrailLook>) -> Option<f32> {
        let (Some(TrailLook::Smear(a)), Some(TrailLook::Smear(b))) = (ride, stay) else {
            return None;
        };
        let dy = a.corners.0[0].y - b.corners.0[0].y;
        a.corners
            .0
            .iter()
            .zip(b.corners.0)
            .all(|(p, q)| (p.x - q.x).abs() < 1e-3 && (p.y - q.y - dy).abs() < 1e-3)
            .then_some(dy)
    }

    /// **On Linux a trail in flight rides every whole-screen scroll**, as on
    /// macOS: a line the primary, the alternate screen or a screen with a
    /// status line below retires moves the quad drawn at the next frame up
    /// by exactly one row, 20 pixels, against the same move under `stay`.
    /// Linux paints on the CPU path and scrolls in one jump, two of the
    /// cases the ride was built for on 2026-11-01; before, it rode nothing.
    /// The controls: the two arms draw the same quad before the scroll, and
    /// the status line is shown. Perturbations: the ride taken 5 ms before
    /// the frame's instant leaves the quad off the text in every case; the
    /// alternate screen refused, as the smooth slide's gate refuses it,
    /// fails that case.
    #[test]
    fn a_trail_rides_every_whole_screen_scroll_on_linux() {
        let mut wrong = Vec::new();
        for (setup, what) in [
            (&b""[..], "the primary"),
            (&b"\x1b[?1049h"[..], "the alternate screen"),
            (&b"\x1b[2$~"[..], "a status line below"),
        ] {
            let ([before, after], status) =
                ride_and_stay(setup, Row::from_index(9), b"\n", CursorTrail::Quad);
            assert_eq!(
                smear_dy(before[0], before[1]),
                Some(0.0),
                "{what}: the control, one quad in flight"
            );
            if what == "a status line below" {
                assert_eq!(status, baud_domain::Rows::of_count(1), "the control: shown");
            }
            let rode = smear_dy(after[0], after[1]);
            if rode != Some(-CELL.px.1) {
                wrong.push(format!("{what}: rode {rode:?}"));
            }
        }
        assert!(wrong.is_empty(), "the quad rode no row: {wrong:?}");
    }

    /// **On Linux a region's scroll rides nothing**, nor does anything under
    /// `stay`: a region anchored at the top retires its line as the whole
    /// screen does, and the rows under it stayed. The control is a region
    /// the whole screen tall, which is the whole screen and rides.
    /// Perturbation: a region's scroll let through `trail_rides` -- the
    /// region rides.
    #[test]
    fn a_region_scroll_rides_nothing_on_linux() {
        let last = Row::from_index(9);
        let ([_, whole], _) = ride_and_stay(b"\x1b[1;10r", last, b"\n", CursorTrail::Quad);
        assert_eq!(
            smear_dy(whole[0], whole[1]),
            Some(-CELL.px.1),
            "the control: a region the whole screen tall is the whole screen"
        );
        let ([_, region], _) =
            ride_and_stay(b"\x1b[1;5r", Row::from_index(4), b"\n", CursorTrail::Quad);
        assert_eq!(
            smear_dy(region[0], region[1]),
            Some(0.0),
            "a region's scroll"
        );
        let ([_, scrolled], _) = ride_and_stay(b"", last, b"\n", CursorTrail::Quad);
        let ([_, unscrolled], _) = ride_and_stay(b"", last, b"", CursorTrail::Quad);
        assert_eq!(
            smear_dy(scrolled[1], unscrolled[0]),
            Some(0.0),
            "stay draws the quad where no scroll would have"
        );
    }

    /// **On Linux the ghost rides too**: the box drawn at the next frame
    /// moves up by exactly one row against `stay`'s, its sweep's end on the
    /// cursor. The control: the two arms draw the same ghost before.
    /// Perturbation: the ride's origin left off the ghost drawn fails it.
    #[test]
    fn a_ghost_rides_a_whole_screen_scroll_on_linux() {
        let top = |look: Option<TrailLook>| match look {
            Some(TrailLook::Ghost(g)) => {
                let row = |r: Row| r.index() as f32 * CELL.px.1;
                Some((row(g.from.0) + g.origin_dy) * (1.0 - g.progress) + row(g.to.0) * g.progress)
            }
            _ => None,
        };
        let ([before, after], _) =
            ride_and_stay(b"", Row::from_index(9), b"\n", CursorTrail::Ghost);
        let (b0, b1) = (top(before[0]), top(before[1]));
        assert!(b0.is_some() && b0 == b1, "the control: {b0:?} and {b1:?}");
        let (a0, a1) = (top(after[0]).expect("ride"), top(after[1]).expect("stay"));
        assert!(
            (a0 - (a1 - CELL.px.1)).abs() < 1e-3,
            "the ghost rode {} px",
            a0 - a1
        );
    }

    /// **A ridden trail repaints on the CPU path, as a whole frame paints
    /// it**: the quad's frames across a whole-screen scroll, drawn by the
    /// renderer the Linux driver paints with, from the rows damage names,
    /// are the frames it draws whole, every one -- so the rows the trail
    /// left above the text are cleared and the rows it reached drawn. The
    /// control: the ridden frame is not the frame `stay` draws.
    #[test]
    fn a_ridden_trail_repaints_on_the_cpu_path_as_a_whole_frame_does() {
        use baud_domain::Cols;
        use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).unwrap()];
        let renderer = || {
            Renderer::new(
                Stack::new(&faces).unwrap(),
                PxSize::of(14.0).unwrap(),
                Theme::dark(),
                Blend::Native,
                Cols::of_index(usize::from(WS.ws_col)),
                baud_domain::Rows::of_index(usize::from(WS.ws_row)),
            )
            .unwrap()
        };
        let (mut inc, mut full, mut other) = (renderer(), renderer(), renderer());
        let (w, h) = inc.pixel_size();
        let len = usize::try_from(w.value()).unwrap() * usize::try_from(h.value()).unwrap();
        let paint = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut Vec<u32>, d: Damage| {
            let mut target = Target::new(px, w, w, h).unwrap();
            let seal = t.seal_writes();
            r.render(t, seal, &mut target, d);
        };
        let m = inc.metrics();
        let cell = Cell {
            px: (m.cell_w.value() as f32, m.cell_h.value() as f32),
            height: Points(m.cell_h.value() as f32),
        };
        let ms = |n: u64| Duration::from_millis(n);
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let lines: Vec<u8> = (0..9)
            .flat_map(|i| format!("row {i} abcdefghijklmnopqrstuvwxyz\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        feed(&mut t, b"\x1b[3;4H");
        let mut arms = [TrailScroll::Ride, TrailScroll::Stay].map(|key| {
            Timed::new(
                t0,
                TrailPick::Named(CursorTrail::Quad),
                ScreenTransition::Off,
            )
            .with_trail_scroll(key)
        });
        let (mut a, mut b, mut c) = (vec![0u32; len], vec![0u32; len], vec![0u32; len]);
        paint(&mut inc, &mut t, &mut a, Damage::Full);
        for n in [0u64, 2, 40, 50, 60, 75, 100, 150, 250, 400, 600] {
            if n == 50 {
                let before = crate::keep::Before::of(&t);
                feed(&mut t, b"\n");
                for fx in &mut arms {
                    fx.scrolled(&t, before);
                }
            }
            if n == 2 {
                feed(&mut t, b"\x1b[10;31H");
            }
            let rect = |r: Row, col: Col, style: CursorStyle| inc.cursor_rect(r, col, style);
            let [ride, stay] = arms
                .each_mut()
                .map(|fx| fx.plan(t0 + ms(n), &t, rect, cell, widths()).trail);
            inc.set_trail(ride);
            full.set_trail(ride);
            paint(&mut inc, &mut t, &mut a, Damage::Incremental);
            paint(&mut full, &mut t, &mut b, Damage::Full);
            assert!(
                a == b,
                "at {n} ms the incremental frame is not the whole frame"
            );
            if n == 50 {
                assert!(
                    ride.is_some(),
                    "the control: a quad in flight at the scroll"
                );
                other.set_trail(stay);
                paint(&mut other, &mut t, &mut c, Damage::Full);
                assert!(a != c, "the control: the ridden frame is stay's");
            }
        }
    }

    /// **On Linux every capture's jumps trail and its scrolls snap**, for
    /// every trail that rides the chase: the thirteen sessions of
    /// `tests/trail/scroll/` through `plan` on a 120 Hz clock, each move as
    /// `carried::captures::PINNED` spells it -- a wheel step, `zz`, `zt`,
    /// `Ctrl-E` and `Ctrl-Y` that carried the cursor with its line draw no
    /// trail, every jump draws one, and the views moved too far to read
    /// still do. Over all thirteen, the six `zb` with the cursor on the top
    /// line are every long scroll that trails. Perturbations: the carry dropped from `plan` (every
    /// scroll trails, as before); the pane read across the whole row
    /// (tmux's split session trails its scrolls); a quorum of two.
    #[test]
    fn every_captures_scrolls_snap_and_jumps_trail_on_linux() {
        use crate::carried::captures::{ALL, Long, Taken, check, replay};
        let mut streaks = 0;
        for trail in [
            CursorTrail::Quad,
            CursorTrail::Sweep,
            CursorTrail::Fire,
            CursorTrail::Plasma,
            CursorTrail::Lightning,
            CursorTrail::Ice,
            CursorTrail::Ufo,
            CursorTrail::Orb,
            CursorTrail::Liquid,
        ] {
            for (name, raw, steps) in ALL {
                let t0 = Instant::now();
                let mut fx = Timed::new(t0, TrailPick::Named(trail), ScreenTransition::Off);
                let moves = replay((raw, steps), |t, secs| {
                    let now = t0 + Duration::from_secs_f64(secs);
                    let _ = fx.plan(now, t, rect, CELL, widths());
                    fx.chase.longs
                });
                check(name, &moves, &format!("{trail:?}"));
                if trail == CursorTrail::Plasma {
                    streaks += moves
                        .iter()
                        .filter(|m| m.label == 'S' && m.long == Long::Long)
                        .filter(|m| m.trailed == Taken::Trailed)
                        .count();
                }
            }
        }
        assert_eq!(streaks, 6, "the long scrolls that still trail");
    }

    /// **The ghost sweeps only what the cursor moved beyond its text**: on
    /// the same captures, a sweep across more than two cells comes on
    /// exactly the moves the chase trails -- a carried cursor's last cell
    /// moves with its line, so the wheel's steps sweep at most the row or
    /// two helix's margin holds the cursor back. The control: with the
    /// carry taken away under `stay`, helix's wheel steps sweep three rows.
    #[test]
    fn the_ghost_sweeps_only_what_is_left_of_a_carried_move_on_linux() {
        use crate::carried::captures::{ALL, PINNED, check, replay, spelled};
        use crate::trail::Count;
        for (name, raw, steps) in ALL {
            for key in [TrailScroll::Ride, TrailScroll::Stay] {
                let t0 = Instant::now();
                let mut fx = Timed::new(
                    t0,
                    TrailPick::Named(CursorTrail::Ghost),
                    ScreenTransition::Off,
                )
                .with_trail_scroll(key);
                let (mut sweeps, mut last) = (Count(0), None);
                let moves = replay((raw, steps), |t, secs| {
                    let now = t0 + Duration::from_secs_f64(secs);
                    let _ = fx.plan(now, t, rect, CELL, widths());
                    if let Some(g) = fx.ghost.filter(|g| Some(g.start) != last) {
                        last = Some(g.start);
                        let far = |a: usize, b: usize| a.abs_diff(b) > 2;
                        if far(g.from.0.index(), g.to.0.index())
                            || far(g.from.1.index(), g.to.1.index())
                        {
                            sweeps.0 += 1;
                        }
                    }
                    sweeps
                });
                match key {
                    TrailScroll::Ride => check(name, &moves, "the ghost"),
                    TrailScroll::Stay => {
                        let pinned = PINNED.iter().find(|p| p.0 == name).unwrap();
                        if name.starts_with("helix") {
                            assert_ne!(
                                spelled(&moves).1,
                                pinned.2,
                                "the control: {name} under stay"
                            );
                        }
                    }
                }
            }
        }
    }

    /// **The corpus trails as it did, the ride included**: fourteen
    /// recordings the trail's earlier gates replay -- pagers, `cat`, `git
    /// log`, btop, cava, pipes, vim and helix -- fed through
    /// `plan` as the driver feeds it, 4096 bytes every 20 ms with the
    /// whole screen's scroll riding the trail (`scrolled`), takes exactly
    /// the long moves the chase took before the carry, driven beside it
    /// the way `plan` drove it then: the ride, then the frame. Streaming
    /// output that scrolls the whole screen while the cursor changes cell,
    /// as `cat` does at every read, is the ride's frame, and the carry
    /// takes nothing from it. The control: the corpus has long moves to
    /// keep, and the helix wheel capture, which the carry changes on
    /// purpose, differs.
    #[test]
    fn the_corpus_takes_the_moves_it_took_before_the_carry_on_linux() {
        use crate::carried::captures::{drive, spelled};
        use crate::trail::ride_px;
        let (wide, narrow, wheel) = ((200, 50), (160, 45), (80, 24));
        let corpus = [
            ("recordings/ascii/cat-scroll.raw", wide),
            ("recordings/ascii/git-log.raw", wide),
            ("recordings/ascii/less-pager.raw", wide),
            ("recordings/ascii/pipes.raw", narrow),
            ("recordings/utf8/btop.raw", narrow),
            ("recordings/utf8/vim-unicode.raw", narrow),
            ("recordings/utf8/cat-cjk.raw", wide),
            ("recordings/utf8/less-cjk.raw", wide),
            ("recordings/utf8/cava.raw", wide),
            ("cursor/helix-exit.raw", wide),
            ("cursor/vim-exit.raw", wide),
            ("panels/helix.raw", wide),
            ("trail/helix-ge-gg.raw", wide),
            ("trail/helix-wheel.raw", wheel),
        ];
        let mut long = 0;
        for (name, (ws_col, ws_row)) in corpus {
            let ws = WinSize { ws_col, ws_row };
            let raw = std::fs::read(format!("{}/../tests/{name}", env!("CARGO_MANIFEST_DIR")))
                .expect(name);
            let reads: Vec<(f64, usize)> = (0..raw.len().div_ceil(4096))
                .map(|k| (0.02 * k as f64, 4096))
                .collect();
            let t0 = Instant::now();
            let feed = |t: &mut Term<'_>, b: &[u8]| {
                t.write_at(BytePos::ZERO, b);
                t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            };
            // The driver: its read, its ride, its frame.
            let fx = core::cell::RefCell::new(Timed::new(
                t0,
                TrailPick::Named(CursorTrail::Plasma),
                ScreenTransition::Off,
            ));
            let now = drive(
                &raw,
                &ws,
                (&reads, &[]),
                |t, b| {
                    let before = crate::keep::Before::of(t);
                    feed(t, b);
                    fx.borrow_mut().scrolled(t, before);
                },
                |t, secs| {
                    let mut fx = fx.borrow_mut();
                    let _ = fx.plan(t0 + Duration::from_secs_f64(secs), t, rect, CELL, widths());
                    fx.chase.longs
                },
            );
            // The chase as `plan` drove it before the carry.
            let chase = core::cell::RefCell::new(Chase::new(CursorTrail::Plasma.style()));
            let riding = core::cell::Cell::new(Rows::ZERO);
            let clock = Clock::new(t0);
            let was = drive(
                &raw,
                &ws,
                (&reads, &[]),
                |t, b| {
                    let before = crate::keep::Before::of(t);
                    feed(t, b);
                    let moved = crate::keep::live_moved(t, before);
                    if let Some(n) =
                        crate::effects::trail_rides(t, before, moved, TrailScroll::Ride)
                    {
                        riding.set(Rows::of_count(riding.get().index() + n.index()));
                    }
                },
                |t, secs| {
                    let host = clock.host(t0 + Duration::from_secs_f64(secs));
                    let mut chase = chase.borrow_mut();
                    let n = riding.replace(Rows::ZERO);
                    if let Some((dy, floor)) = ride_px(n, t.geometry().1, CELL.px.1) {
                        chase.ride(dy, host, floor, CELL.px.1);
                    }
                    let style = t.cursor_style();
                    let r = crate::effects::trail_cursor(t).map(|f| {
                        let (row, col) = f.cell();
                        rect(row, col, f.style(style))
                    });
                    let _ = chase.frame(r, host, CELL.px, CELL.height);
                    chase.longs
                },
            );
            long += was
                .iter()
                .filter(|m| m.trailed == crate::carried::captures::Taken::Trailed)
                .count();
            match name {
                "trail/helix-wheel.raw" => {
                    assert_ne!(spelled(&now).1, spelled(&was).1, "the control: {name}")
                }
                _ => assert_eq!(spelled(&now), spelled(&was), "{name}"),
            }
        }
        assert!(
            long > 100,
            "the control: the corpus has {long} long moves to keep"
        );
    }

    /// **A ghost in flight moves with a carried cursor's text on Linux**:
    /// a jump from row 4 to row 8 starts a sweep, and inside its 100 ms the
    /// view is redrawn three lines up the file with the cursor kept on its
    /// line. The sweep's end moves the three rows with it and its start as
    /// far, so the ghost drawn is the last one moved by the carry, and no
    /// sweep starts for the carried cursor. Perturbation: the ghost in
    /// flight left where it was -- its end stays on row 8.
    #[test]
    fn a_ghost_in_flight_moves_with_a_carried_cursor_on_linux() {
        use crate::carried::tests::{Line, view};
        let ws = WinSize {
            ws_col: 40,
            ws_row: 12,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut fx = Timed::new(
            t0,
            TrailPick::Named(CursorTrail::Ghost),
            ScreenTransition::Off,
        );
        let ghost = |p: Plan| match p.trail {
            Some(TrailLook::Ghost(g)) => g,
            other => panic!("no ghost: {other:?}"),
        };
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, &view(Line(10), Line(13)));
        let _ = fx.plan(ms(0), &t, rect, CELL, widths());
        feed(&mut t, &view(Line(10), Line(17)));
        let _ = fx.plan(ms(8), &t, rect, CELL, widths());
        let started = fx.ghost.map(|g| g.start);
        let was = ghost(fx.plan(ms(16), &t, rect, CELL, widths()));
        assert_eq!(
            was.to,
            (Row::from_index(7), Col::at_index(0)),
            "the control: a sweep to row 8"
        );
        feed(&mut t, &view(Line(7), Line(17)));
        let now = ghost(fx.plan(ms(24), &t, rect, CELL, widths()));
        assert_eq!(
            fx.ghost.map(|g| g.start),
            started,
            "no sweep starts for the carry"
        );
        assert_eq!(
            now.to,
            (Row::from_index(10), Col::at_index(0)),
            "its end moved with the line"
        );
        assert_eq!(now.from, was.from);
        assert!(
            (now.origin_dy - was.origin_dy - 3.0 * CELL.px.1).abs() < 1e-3,
            "its start moved as far: {} against {}",
            now.origin_dy,
            was.origin_dy
        );
    }

    /// A Timed with the default plasma in flight after a long move, at
    /// the dwell's end, its frame drawn: the turn after owes a frame.
    fn in_flight(t0: Instant, t: &mut Term<'_>) -> (Timed, Instant) {
        let mut fx = Timed::new(t0, TrailPick::Default, ScreenTransition::Off);
        let _ = fx.plan(t0, t, rect, CELL, widths());
        fx.keyed(Some(t0));
        feed(t, b"\x1b[8;30H");
        let at = t0 + Duration::from_millis(2);
        let _ = fx.plan(at, t, rect, CELL, widths());
        let dwell = fx.deadline(at).expect("the dwell");
        assert_eq!(fx.turn(dwell, t), Owes::Frame, "the dwell ran out");
        let _ = fx.plan(dwell, t, rect, CELL, widths());
        (fx, dwell)
    }

    const PERIOD: Duration = Duration::from_millis(4);

    fn paced_key() -> crate::pacer::Key {
        crate::pacer::Key {
            arm: crate::pacer::PacedArm::Cpu,
            drawn: crate::pacer::Paced::Trail(CursorTrail::Plasma),
            size: (crate::pacer::Pixels(800), crate::pacer::Pixels(600)),
            scale: 1.0,
        }
    }

    /// **At half rate a turn owed only by the trail is drawn every second
    /// refresh**, and a skipped one wakes the loop a period on: while the
    /// plasma flies, the turns owe a frame and nothing in turn, and each
    /// owing nothing names the next refresh. The control: at every refresh
    /// each turn owes a frame. Named break: a pacer whose divisor no turn
    /// reads.
    #[test]
    fn a_trail_only_turn_at_half_rate_is_drawn_every_second_refresh() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let (mut fx, dwell) = in_flight(t0, &mut t);
        fx.set_period(Some(PERIOD));
        let owed = |fx: &mut Timed, t: &mut Term<'_>| -> Vec<(Owes, Option<Instant>)> {
            (1..=6u32)
                .map(|k| {
                    let now = dwell + PERIOD * k;
                    (fx.turn(now, t), fx.deadline(now))
                })
                .collect()
        };
        let full = owed(&mut fx, &mut t);
        assert!(
            full.iter().all(|(o, _)| *o == Owes::Frame),
            "the control: {full:?}"
        );
        let slow = PERIOD.mul_f64(1.5);
        assert_eq!(
            fx.effect_frame(
                paced_key(),
                slow,
                crate::pacer::Missed::No,
                crate::pacer::Cold::No
            ),
            Some(crate::pacer::Divisor::Half)
        );
        let (mut fx, dwell) = (fx, dwell);
        let mut drawn = 0;
        for k in 1..=6u32 {
            let now = dwell + PERIOD * (6 + k);
            match fx.turn(now, &mut t) {
                Owes::Frame => drawn += 1,
                Owes::Nothing => {
                    assert_eq!(
                        fx.deadline(now),
                        Some(now + PERIOD),
                        "turn {k} wakes a period on"
                    )
                }
                Owes::Full => panic!("turn {k}"),
            }
        }
        assert_eq!(drawn, 3, "every second refresh");
    }

    /// **At half rate a turn owed for anything else is drawn at once**: a
    /// blink flip owes the whole frame on the turn the trail would skip.
    #[test]
    fn a_turn_owed_for_more_than_the_trail_is_never_skipped() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b[5mslow\x1b[m");
        let (mut fx, dwell) = in_flight(t0, &mut t);
        fx.drawn(Blinking::SLOW, RegisBlink::Off);
        fx.set_period(Some(PERIOD));
        fx.effect_frame(
            paced_key(),
            PERIOD * 2,
            crate::pacer::Missed::No,
            crate::pacer::Cold::No,
        );
        let flip = fx.deadline(dwell).expect("the blink flips");
        let turns: Vec<Owes> = (0..3).map(|k| fx.turn(flip + PERIOD * k, &mut t)).collect();
        assert_eq!(turns[0], Owes::Full, "the flip, drawn at once: {turns:?}");
    }

    /// **With no period known no turn is skipped**, however slow a frame.
    #[test]
    fn with_no_period_no_turn_is_skipped() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let (mut fx, dwell) = in_flight(t0, &mut t);
        fx.effect_frame(
            paced_key(),
            Duration::from_millis(50),
            crate::pacer::Missed::Yes,
            crate::pacer::Cold::No,
        );
        for k in 1..=6u32 {
            assert_eq!(fx.turn(dwell + PERIOD * k, &mut t), Owes::Frame, "turn {k}");
        }
    }
}
