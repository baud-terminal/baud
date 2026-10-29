//! **The Linux driver's timed effects**: `SGR 5` and `SGR 6` text blink,
//! the cursor trail, the alternate screen's fade and the scrollbar's thumb,
//! each the macOS app's and each drawn by the CPU path the driver paints
//! with. The clock is seconds since the driver started, where the macOS
//! app reads the display link's timestamp; the loop has no tick, so an
//! effect says what it owes instead:
//!
//! * **an instant**, for a change that comes on the clock with no event to
//!   bring it -- the next blink flip of a rate on screen, the end of a
//!   trail's dwell, the end of the thumb's hold -- which bounds the loop's
//!   one wait (`deadline`);
//! * **every frame**, for motion in flight -- a trail sweeping, the fade,
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

use crate::config::{CursorTrail, ScreenTransition, SystemMotion};
use crate::trail::Chase;
use baud_core::{CursorStyle, Term};
use baud_domain::{Col, HostTime, RegisBlink, RegisRepainted, Row};
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

/// A ghost's sweep in flight: the cells it left and reached, when it began
/// on the clock, and how it is drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Ghost {
    from: (Row, Col),
    to: (Row, Col),
    start: f64,
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
    pub(crate) blink: BlinkPhases,
    pub(crate) splash: Option<baud_render::SplashLook>,
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
    transition: ScreenTransition,
    fade: Option<f64>,
    /// The scrollbar's thumb (`bar.rs`).
    pub(crate) bar: crate::bar::Bar,
    /// The launch splash (`splash.rs`): gone unless [`Timed::with_splash`]
    /// shows it.
    splash: crate::splash::Splash,
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
            transition,
            fade: None,
            bar: crate::bar::Bar::new(),
            splash: crate::splash::Splash::new(crate::config::LaunchSplash::Off),
        }
    }

    /// **The launch's splash**, from `[launch] splash` (and `BAUD_SPLASH`):
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
    #[cfg(test)]
    fn trail(&self) -> CursorTrail {
        self.trail
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
        [flip, dwell, hold, cap]
            .into_iter()
            .flatten()
            .filter(|&t| t > now)
            .min()
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
        let moving = thumb
            || splash
            || self.ghost.is_some()
            || self.fade.is_some()
            || self.chase.owes(host) == crate::caret::Owes::Frame;
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

    /// **The cells moved under the trail** -- a resize, a font change: the
    /// next sighting starts afresh, and no ghost sweeps across.
    pub(crate) fn snap(&mut self) {
        self.chase.snap();
        self.last_cell = None;
        self.ghost = None;
    }

    /// **Another surface came up**: no trail crosses the switch, and the
    /// new screen fades in from the background when `[screen] transition`
    /// is on.
    pub(crate) fn switched(&mut self, now: Instant) {
        self.snap();
        if self.transition == ScreenTransition::On {
            self.fade = Some(self.clock.secs(now));
        }
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
        let followed = crate::effects::trail_cursor(term);
        let cursor_cell = followed.map(crate::effects::TrailCursor::cell);
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
                    origin_dy: 0.0,
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
        let fade = self.fade.and_then(|start| {
            let p = ((secs - start) / crate::effects::TRANSITION_S) as f32;
            match p >= 1.0 {
                // Settled: this frame draws no veil, which repaints the
                // screen once to clear it.
                true => None,
                false => Some(Transition {
                    progress: p.max(0.0),
                }),
            }
        });
        if fade.is_none() {
            self.fade = None;
        }
        let splash = self.splash.look(Some(secs), term, self.motion);
        Plan {
            scroll,
            trail,
            fade,
            blink: self.blink,
            splash,
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

    /// **A switch fades the new screen in when `[screen] transition` is
    /// on, and only then**, and the fade owes frames until it settles.
    #[test]
    fn a_switch_fades_in_only_when_the_transition_is_on() {
        let t0 = Instant::now();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        for (on, want) in [(ScreenTransition::On, true), (ScreenTransition::Off, false)] {
            let mut fx = Timed::new(t0, TrailPick::Named(CursorTrail::Off), on);
            fx.switched(t0);
            let p = fx.plan(t0 + Duration::from_millis(50), &t, rect, CELL, widths());
            assert_eq!(p.fade.is_some(), want, "{on:?}");
            assert_eq!(
                fx.turn(t0 + Duration::from_millis(60), &mut t) == Owes::Frame,
                want
            );
            let p = fx.plan(t0 + Duration::from_millis(200), &t, rect, CELL, widths());
            assert_eq!(p.fade, None, "settled");
            assert_eq!(
                fx.turn(t0 + Duration::from_millis(210), &mut t),
                Owes::Nothing
            );
        }
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
        fx.switched(t0);
        let p = fx.plan(t0 + Duration::from_millis(50), &t, rect, CELL, widths());
        assert_eq!(p.fade, None, "the control: no fade");
        let _ = fx.set_config(
            TrailPick::Named(CursorTrail::Off),
            ScreenTransition::On,
            t0 + Duration::from_millis(300),
        );
        assert_eq!(fx.trail(), CursorTrail::Off);
        let t1 = t0 + Duration::from_millis(400);
        fx.switched(t1);
        let p = fx.plan(t1 + Duration::from_millis(50), &t, rect, CELL, widths());
        assert!(p.fade.is_some(), "the switch fades now");
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
}
