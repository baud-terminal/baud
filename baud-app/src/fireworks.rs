//! **iTerm2's fireworks, as both drivers time them**
//! (`baud_render::fireworks`): a show starts on the first frame after a
//! program's `OSC 1337 ; RequestAttention=fireworks` (`Term::fireworks`),
//! its first burst at the cursor's cell as the request arrived and each
//! later one at the cursor's cell as it is born, the hole round the cursor
//! where the cursor is now; it holds a second after the last request and
//! is gone a quarter second later, on the clock alone -- the display
//! link's timestamp on macOS, seconds since start on Linux. A request
//! while a show holds restarts its hold, as iTerm2's restarts its teardown
//! timer; one while it fades starts another beside it, as iTerm2's opens a
//! new window while the old one's alpha falls.
//!
//! **Under Reduce Motion a show is still** (`Form::Still`): its frame half
//! a second in, at the cell the request arrived at, faded out over the
//! show's length, and a request while it shows starts nothing.
//!
//! **It waits on nothing**: a show owes a frame every tick while it is up
//! and nothing once it is gone, and the renderer draws what it is handed,
//! so the frame is a pure function of the clock, the screen and the
//! requests, and the tests run on every platform.

use crate::config::SystemMotion;
use baud_core::{ReportCount, Term};
use baud_domain::{Col, Row};
use baud_render::Pt;
use baud_render::effect::Seed;
use baud_render::fireworks::{BAUD, Burst, FADE, Form, HOLD, ORIGINS, Show};

/// What the shows owe the frame loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owes {
    Nothing,
    Frame,
}

/// One show in flight: when it began, when its hold ends, how it moves,
/// the cursor's centre as each burst was born, how many bursts that is,
/// and its seed.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Running {
    start: f64,
    hold: f64,
    form: Form,
    origins: [Pt; ORIGINS],
    born: Burst,
    seed: Seed,
}

impl Running {
    fn end(&self) -> f64 {
        self.start + self.hold + f64::from(FADE)
    }
}

/// **The fireworks' timing.**
#[derive(Clone, Debug)]
pub(crate) struct Fireworks {
    /// The request count the last frame saw, or none before a frame has
    /// seen a request: then any the terminal has had is new.
    seen: Option<ReportCount>,
    /// The shows up, the older first: at most two.
    shows: Vec<Running>,
    /// The last show's seed: each one's its own.
    seed: Seed,
}

/// Where a cell's centre is in frame pixels: the renderer's cell size, the
/// rows the window looks back added.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Cells {
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) back: usize,
}

impl Cells {
    /// The centre of `(row, col)` of the live screen, as the window shows
    /// it: iTerm2's cursor centre, the cell's origin plus half its width
    /// and half its height (`cursorCenter`).
    pub(crate) fn centre(&self, (row, col): (Row, Col)) -> Pt {
        let f = |v: usize| f32::from(u16::try_from(v).unwrap_or(u16::MAX));
        Pt {
            x: (f(col.index()) + 0.5) * self.w,
            y: (f(row.index() + self.back) + 0.5) * self.h,
        }
    }
}

impl Fireworks {
    /// No show and no request seen: a request the terminal read before
    /// the first frame starts a show on it, as one read after does.
    pub(crate) fn new() -> Fireworks {
        Fireworks {
            seen: None,
            shows: Vec::new(),
            seed: Seed(0xF1EE_0000),
        }
    }

    /// Whether `term` has had a request no frame has seen.
    fn unseen(&self, term: &Term<'_>) -> bool {
        term.fireworks().is_some() && Some(term.fireworks_requests()) != self.seen
    }

    /// **The shows for a frame at `now`**, which advances them: a request
    /// since the last frame starts a show, or restarts the hold of one
    /// that holds; a show past its end is gone; each moving show's bursts
    /// born since the last frame take the cursor's centre now. `scale` is
    /// device pixels a point.
    pub(crate) fn frame(
        &mut self,
        now: f64,
        term: &Term<'_>,
        cells: Cells,
        scale: f32,
        motion: SystemMotion,
    ) -> Vec<Show> {
        self.shows.retain(|s| now < s.end());
        let cursor = cells.centre(term.cursor());
        if self.unseen(term) {
            self.seen = Some(term.fireworks_requests());
            let at = term.fireworks().map_or(cursor, |c| cells.centre(c));
            self.request(now, at, motion);
        }
        let mut out = Vec::with_capacity(self.shows.len());
        for s in &mut self.shows {
            let age = (now - s.start).max(0.0) as f32;
            if s.form == Form::Moving {
                let by = Burst::by(age);
                while s.born < by {
                    s.born.set(&mut s.origins, cursor);
                    s.born = s.born.next();
                }
            }
            out.push(Show {
                age,
                hold: s.hold as f32,
                origins: s.origins,
                hole: cursor,
                scale,
                seed: s.seed,
                look: BAUD,
                form: s.form,
            });
        }
        out
    }

    /// A request at `now`, the cursor's centre `at` as it arrived.
    fn request(&mut self, now: f64, at: Pt, motion: SystemMotion) {
        let form = match motion {
            SystemMotion::Full => Form::Moving,
            SystemMotion::Reduced => Form::Still,
        };
        if let Some(last) = self.shows.last_mut() {
            let holding = now < last.start + last.hold;
            match (last.form, holding) {
                // The teardown timer restarted: a second from now.
                (Form::Moving, true) => {
                    last.hold = now - last.start + f64::from(HOLD);
                    return;
                }
                (Form::Still, _) if now < last.end() => return,
                _ => {}
            }
        }
        // Every burst's origin is the request's cell until it is born: the
        // first is born on the show's first frame, there, and a still show
        // keeps them all there.
        let origins = [at; ORIGINS];
        self.seed = Seed(self.seed.0.wrapping_add(0x9E37_79B9));
        self.shows.push(Running {
            start: now,
            hold: f64::from(HOLD),
            form,
            origins,
            born: Burst(1),
            seed: self.seed,
        });
        if self.shows.len() > 2 {
            self.shows.remove(0);
        }
    }

    /// **What a tick owes**: a frame while a show is up, and the one a
    /// request that has not been drawn yet starts.
    pub(crate) fn owes(&self, term: &Term<'_>) -> Owes {
        match !self.shows.is_empty() || self.unseen(term) {
            true => Owes::Frame,
            false => Owes::Nothing,
        }
    }

    /// Whether a show is up: what the shows hold the renderer's effect
    /// slot for.
    #[cfg(test)]
    pub(crate) fn up(&self) -> bool {
        !self.shows.is_empty()
    }
}

/// The render model's end, for the tests: seconds from a show's start to
/// its end with no second request.
#[cfg(test)]
fn end() -> f64 {
    f64::from(HOLD + FADE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 12,
    };

    const CELLS: Cells = Cells {
        w: 8.0,
        h: 16.0,
        back: 0,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// **A request starts a show at the cursor's cell, and it ends by
    /// itself**: on the frame after the request, a show whose first burst is
    /// the cell the request arrived at, though the cursor has moved on; it
    /// owes a frame every tick while it is up; its later bursts take the
    /// cursor's cell as each is born; at its end it is gone and owes
    /// nothing, with no request since.
    #[test]
    fn a_request_starts_a_show_that_ends_by_itself() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut fw = Fireworks::new();
        assert_eq!(fw.owes(&t), Owes::Nothing);
        feed(
            &mut t,
            b"\x1b[3;5H\x1b]1337;RequestAttention=fireworks\x07\x1b[7;20H",
        );
        assert_eq!(fw.owes(&t), Owes::Frame, "the request owes its first frame");
        let start = 10.0;
        let shows = fw.frame(start, &t, CELLS, 2.0, SystemMotion::Full);
        assert_eq!(shows.len(), 1);
        let s = shows[0];
        assert_eq!(s.age, 0.0);
        assert_eq!(s.origins[0], Pt { x: 36.0, y: 40.0 }, "the request's cell");
        assert_eq!(s.hole, Pt { x: 156.0, y: 104.0 }, "the cursor's now");
        assert_eq!(s.form, Form::Moving);
        assert_eq!(s.scale, 2.0);
        // A burst born a quarter second in takes the cursor's cell then.
        feed(&mut t, b"\x1b[9;2H");
        let shows = fw.frame(start + 0.26, &t, CELLS, 2.0, SystemMotion::Full);
        assert_eq!(shows[0].origins[1], Pt { x: 12.0, y: 136.0 });
        assert_eq!(shows[0].origins[0], Pt { x: 36.0, y: 40.0 }, "kept");
        let mut now = start + 0.26;
        while now < start + end() - 1e-6 {
            assert_eq!(fw.owes(&t), Owes::Frame, "{now}");
            assert_eq!(fw.frame(now, &t, CELLS, 2.0, SystemMotion::Full).len(), 1);
            now += 1.0 / 60.0;
        }
        assert!(
            fw.frame(start + end(), &t, CELLS, 2.0, SystemMotion::Full)
                .is_empty()
        );
        assert!(!fw.up());
        assert_eq!(fw.owes(&t), Owes::Nothing, "gone, and owes nothing");
    }

    /// **A request while a show holds restarts its hold; one while it fades
    /// starts a second beside it**, as iTerm2's teardown timer and its
    /// window do; under Reduce Motion the show is still, and a request
    /// while it shows starts nothing.
    #[test]
    fn requests_hold_a_show_or_start_another() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut fw = Fireworks::new();
        let req = b"\x1b]1337;RequestAttention=fireworks\x07";
        feed(&mut t, req);
        fw.frame(0.0, &t, CELLS, 1.0, SystemMotion::Full);
        feed(&mut t, req);
        let s = fw.frame(0.5, &t, CELLS, 1.0, SystemMotion::Full);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].hold, 1.5, "a second after the second request");
        feed(&mut t, req);
        let s = fw.frame(1.6, &t, CELLS, 1.0, SystemMotion::Full);
        assert_eq!(s.len(), 2, "a request in the fade starts another");
        assert!(s[0].window() < 1.0 && s[1].age == 0.0);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut fw = Fireworks::new();
        feed(&mut t, req);
        let s = fw.frame(0.0, &t, CELLS, 1.0, SystemMotion::Reduced);
        assert_eq!(s[0].form, Form::Still);
        feed(&mut t, req);
        let s = fw.frame(0.5, &t, CELLS, 1.0, SystemMotion::Reduced);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].hold, 1.0, "nothing restarted");
    }

    /// The model's end is the render crate's: a second's hold and its fade.
    #[test]
    fn the_end_is_the_models() {
        let s = Show {
            age: 0.0,
            hold: HOLD,
            origins: [Pt { x: 0.0, y: 0.0 }; ORIGINS],
            hole: Pt { x: 0.0, y: 0.0 },
            scale: 1.0,
            seed: Seed(0),
            look: BAUD,
            form: Form::Moving,
        };
        assert_eq!(f64::from(s.end()), end());
    }

    /// **Both drivers hand the shows to the renderer and owe them
    /// frames**: the macOS app builds its fireworks with the terminal, its
    /// frame takes the shows at the display link's target and hands them
    /// on, and its tick marks the frame dirty while they owe one; the Linux
    /// frame hands on its plan's (`timed.rs`, which owes the frames).
    /// AppKit- and Wayland-bound, so the source is what can fail: each
    /// compared whole with the spaces taken out. The controls: the splash
    /// and the trail handed on beside them.
    #[test]
    fn the_drivers_hand_the_fireworks_to_the_renderer() {
        let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let body = |src: &str, head: &str| {
            src.split_once(head)
                .map(|(_, rest)| rest.split_once("\n    }\n").map_or(rest, |(b, _)| b))
                .map(bare)
                .unwrap_or_else(|| panic!("{head}"))
        };
        let app = include_str!("app.rs");
        assert_eq!(
            app.matches("let fireworks = crate::fireworks::Fireworks::new();")
                .count(),
            1,
            "the app builds them with the terminal"
        );
        let present = body(
            app,
            "fn present(&mut self, screen: &DrawableView, show: Show) {",
        );
        assert!(
            present.contains("self.renderer.set_splash(splash);"),
            "the control"
        );
        const FRAME: &str = r#"
            let shows = match self.last_tick {
                Some(_) => self.fireworks.frame(
                    self.tick_target,
                    &self.term,
                    cells,
                    self.scale as f32,
                    self.motion,
                ),
                None => Vec::new(),
            };
            self.renderer.set_fireworks(&shows);
        "#;
        assert!(present.contains(&bare(FRAME)), "the frame hands them on");
        let tick = body(app, "fn frame(&mut self, target: f64) {");
        const SPLASH: &str = r#"
            if self.app.splash.owes(target) == crate::splash::Owes::Frame {
                self.app.pending = Pending::Dirty;
            }
        "#;
        assert!(tick.contains(&bare(SPLASH)), "the control");
        const TICK: &str = r#"
            if self.app.fireworks.owes(&self.app.term) == crate::fireworks::Owes::Frame {
                self.app.pending = Pending::Dirty;
            }
        "#;
        assert!(tick.contains(&bare(TICK)), "the tick owes them a frame");
        let linux = include_str!("linux.rs");
        let plan = linux
            .split_once("let plan = self.timed.plan(")
            .and_then(|(_, rest)| {
                rest.split_once("let margin = self.renderer.clear_color_of(&self.term);")
            })
            .map(|(b, _)| bare(b))
            .expect("the Linux frame's plan");
        assert!(
            plan.contains("self.renderer.set_trail(plan.trail);"),
            "the control"
        );
        const LINUX: &str = r#"
            let shows: Vec<_> = plan.fireworks.iter().flatten().copied().collect();
            self.renderer.set_fireworks(&shows);
        "#;
        assert!(plan.contains(&bare(LINUX)), "the Linux frame hands them on");
    }
}
