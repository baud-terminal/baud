//! The cursor's blink: when it shows, when it hides, and which frames that
//! owes.
//!
//! **The timing is xterm's.** Shown for `cursorOnTime`, 600 ms, hidden for
//! `cursorOffTime`, 300 ms -- its resources' defaults (`[[source:
//! xterm-reset]]`'s file, 577-578). A pinned number rather than a round one,
//! and the only terminal whose default for it is written down here.
//!
//! **It blinks where foot blinks it, and nowhere else.** foot arms its blink
//! timer only while the window has focus (`term_cursor_blink_update`, whose
//! `activate` needs `term->visual_focus`, in `[[source: foot-term-reset]]`'s
//! file), so a window in the background holds a steady cursor. And it restarts
//! the cycle shown on every read of the child's output -- "Prevent blinking
//! while typing" (the same file, 254-258) -- so a cursor that is moving is not
//! hidden half the time. baud restarts it on a key sent as well: a key the
//! child does not echo, a password, reads nothing back to restart it.
//!
//! **A cursor that is not blinking draws [`Blink::Shown`], and the frame that
//! stops it blinking is owed.** Losing focus, a steady style, `DECTCEM`, a
//! window scrolled back: any of them in the off phase would otherwise leave
//! the cursor hidden until something else drew a frame.
//!
//! **There is no timeout.** alacritty and kitty stop blinking after a while;
//! neither pinned source does, and the owner asked for a blinking cursor. What
//! it costs an idle screen is two frames every 0.9 s, counted by a test below.
//!
//! The clock is the driver's -- the display link's timestamp on macOS, the
//! `poll` timeout on Linux ([`Caret::next_change`]) -- so nothing sleeps and
//! no frame is held.

use core::time::Duration;

use baud_core::{CursorStyle, Focus};
use baud_render::Blink;

use crate::bell::Moment;

/// How long a blinking cursor shows: xterm's `cursorOnTime`.
pub const ON: Duration = Duration::from_millis(600);
/// How long it hides: xterm's `cursorOffTime`.
pub const OFF: Duration = Duration::from_millis(300);

/// Whether the cursor blinks this frame at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blinks {
    Yes,
    No,
}

/// Whether a cursor of `style`, in a window with `focus`, blinks: it has to
/// ask to, the window has to have focus, and there has to be a cursor on the
/// screen being drawn -- `cursor` is the driver's, `None` when `DECTCEM` hid it
/// or the window is looking back.
pub fn blinks<T>(style: CursorStyle, focus: Focus, cursor: Option<T>) -> Blinks {
    if style.blinks() && focus == Focus::Gained && cursor.is_some() {
        Blinks::Yes
    } else {
        Blinks::No
    }
}

/// Whether the blink owes a frame: the phase a frame drawn now would show is
/// not the one last drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owes {
    Frame,
    Nothing,
}

/// The blink's clock and what the last drawn frame showed.
pub struct Caret<M> {
    /// When the current cycle began, shown.
    epoch: M,
    /// Whether the cursor blinked at the last reading, so a cursor that starts
    /// blinking again starts shown -- foot's `CURSOR_BLINK_ON` when it arms.
    was: Blinks,
    /// The phase the last drawn frame showed.
    drawn: Blink,
}

impl<M: Moment> Caret<M> {
    pub fn new(now: M) -> Caret<M> {
        Caret {
            epoch: now,
            was: Blinks::No,
            drawn: Blink::Shown,
        }
    }

    /// Input or output: the cursor shows, and the cycle restarts from `now`.
    pub fn restart(&mut self, now: M) {
        self.epoch = now;
    }

    /// The phase a frame drawn at `now` shows. A cursor that does not blink
    /// shows; one that starts blinking again restarts its cycle, shown.
    pub fn plan(&mut self, now: M, blinks: Blinks) -> Blink {
        if blinks == Blinks::Yes && self.was == Blinks::No {
            self.epoch = now;
        }
        self.was = blinks;
        if blinks == Blinks::No {
            return Blink::Shown;
        }
        phase_at(now.elapsed_since(self.epoch))
    }

    /// Whether a frame drawn at `now` would show another phase than the last
    /// drawn frame did: the tick owes a frame when it would.
    pub fn due(&mut self, now: M, blinks: Blinks) -> Owes {
        if self.plan(now, blinks) == self.drawn {
            Owes::Nothing
        } else {
            Owes::Frame
        }
    }

    /// A frame showing `phase` reached the screen. One put off or refused
    /// does not call this, so the next asks again.
    pub fn commit(&mut self, phase: Blink) {
        self.drawn = phase;
    }

    /// When the phase next changes, for a `poll` timeout: `None` while the
    /// cursor does not blink, so a steady cursor adds no wakeup. The macOS
    /// driver reads the display link instead and has no use for it.
    #[cfg(any(target_os = "linux", test))]
    pub fn next_change(&self, now: M, blinks: Blinks) -> Option<M> {
        if blinks == Blinks::No {
            return None;
        }
        let into = cycle_offset(now.elapsed_since(self.epoch));
        let left = if into < ON {
            ON - into
        } else {
            ON + OFF - into
        };
        Some(now.after(left))
    }
}

/// How far into its cycle a cursor is, `elapsed` after the cycle began.
fn cycle_offset(elapsed: Duration) -> Duration {
    let cycle = (ON + OFF).as_nanos();
    let into = elapsed.as_nanos() % cycle;
    // Below one cycle, which is 900 ms, so it fits.
    Duration::from_nanos(u64::try_from(into).unwrap_or(0))
}

/// The phase `elapsed` into a cycle: shown for `ON`, then hidden.
fn phase_at(elapsed: Duration) -> Blink {
    if cycle_offset(elapsed) < ON {
        Blink::Shown
    } else {
        Blink::Hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Ten seconds of 120 Hz ticks, each drawing a frame when one is owed:
    /// the phase every frame the blink cost drew.
    fn frames_over_ten_seconds(blinks: Blinks) -> Vec<Blink> {
        let t0 = Instant::now();
        let mut c = Caret::new(t0);
        let mut drawn = Vec::new();
        for tick in 1..=1200u64 {
            let now = t0 + Duration::from_nanos(tick * 1_000_000_000 / 120);
            if c.due(now, blinks) == Owes::Frame {
                let phase = c.plan(now, blinks);
                c.commit(phase);
                drawn.push(phase);
            }
        }
        drawn
    }

    /// Blinking and focused, a frame at every flip: two a 0.9 s cycle, 22 in
    /// ten seconds (flips at 0.6 + 0.9k and 0.9k, each eleven times). Not
    /// blinking, none -- an idle steady cursor builds no frame.
    #[test]
    fn a_blinking_cursor_costs_two_frames_a_cycle_and_a_steady_one_none() {
        let drawn = frames_over_ten_seconds(Blinks::Yes);
        assert_eq!(drawn.len(), 22);
        // Each frame shows the other phase from the one before: a frame per
        // flip and none between, the first of them a hide.
        assert!(drawn.windows(2).all(|w| w[0] != w[1]), "{drawn:?}");
        assert_eq!(drawn.first(), Some(&Blink::Hidden));
        assert!(frames_over_ten_seconds(Blinks::No).is_empty());
    }

    /// xterm's on and off times: shown for 600 ms, hidden for 300.
    #[test]
    fn the_cycle_is_six_hundred_on_and_three_hundred_off() {
        let t0 = Instant::now();
        let mut c = Caret::new(t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.plan(at(0), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(599), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(600), Blinks::Yes), Blink::Hidden);
        assert_eq!(c.plan(at(899), Blinks::Yes), Blink::Hidden);
        assert_eq!(c.plan(at(900), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(1500), Blinks::Yes), Blink::Hidden);
    }

    /// Input restarts the cycle shown, and a frame is owed if the drawn one
    /// had the cursor hidden.
    #[test]
    fn input_in_the_off_phase_shows_the_cursor_at_once() {
        let t0 = Instant::now();
        let mut c = Caret::new(t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        c.plan(t0, Blinks::Yes);
        let hidden = c.plan(at(700), Blinks::Yes);
        assert_eq!(hidden, Blink::Hidden);
        c.commit(hidden);
        c.restart(at(750));
        assert_eq!(
            c.due(at(751), Blinks::Yes),
            Owes::Frame,
            "the restart owes no frame"
        );
        assert_eq!(c.plan(at(751), Blinks::Yes), Blink::Shown);
        // And the next hide is a whole `ON` later, not where the old cycle
        // would have put it.
        assert_eq!(c.plan(at(1349), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(1350), Blinks::Yes), Blink::Hidden);
    }

    /// A cursor that stops blinking in the off phase -- focus lost, a steady
    /// style, hidden, looked away from -- is owed the frame that shows it.
    #[test]
    fn stopping_in_the_off_phase_owes_the_frame_that_shows_it() {
        let t0 = Instant::now();
        let mut c = Caret::new(t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        c.plan(t0, Blinks::Yes);
        let hidden = c.plan(at(700), Blinks::Yes);
        c.commit(hidden);
        assert_eq!(c.due(at(710), Blinks::No), Owes::Frame);
        assert_eq!(c.plan(at(710), Blinks::No), Blink::Shown);
    }

    /// A cursor that starts blinking again starts shown, whatever the old
    /// cycle says -- foot re-arms its timer with the state on.
    #[test]
    fn blinking_again_restarts_the_cycle_shown() {
        let t0 = Instant::now();
        let mut c = Caret::new(t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        c.plan(t0, Blinks::No);
        // 700 ms into a cycle that began at t0 would be hidden.
        assert_eq!(c.plan(at(700), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(1299), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(1300), Blinks::Yes), Blink::Hidden);
    }

    /// A frame that was put off draws nothing, so the tick after it is owed
    /// the frame again.
    #[test]
    fn a_frame_put_off_is_owed_again() {
        let t0 = Instant::now();
        let mut c = Caret::new(t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.due(at(600), Blinks::Yes), Owes::Frame);
        assert_eq!(
            c.due(at(608), Blinks::Yes),
            Owes::Frame,
            "no commit, and nothing owed"
        );
        let p = c.plan(at(616), Blinks::Yes);
        c.commit(p);
        assert_eq!(c.due(at(624), Blinks::Yes), Owes::Nothing);
    }

    /// The `poll` timeout's deadline: the next flip while blinking, and none
    /// for a steady cursor.
    #[test]
    fn the_next_change_is_the_next_flip() {
        let t0 = Instant::now();
        let mut c = Caret::new(t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.next_change(at(100), Blinks::Yes), Some(at(600)));
        assert_eq!(c.next_change(at(650), Blinks::Yes), Some(at(900)));
        assert_eq!(c.next_change(at(900), Blinks::Yes), Some(at(1500)));
        assert_eq!(c.next_change(at(100), Blinks::No), None);
    }

    /// Blinking needs all three: a style that blinks, focus, and a cursor
    /// on the screen being drawn.
    #[test]
    fn a_cursor_blinks_only_focused_visible_and_asked_to() {
        let on = Some(());
        let none: Option<()> = None;
        assert_eq!(
            blinks(CursorStyle::BlinkingBlock, Focus::Gained, on),
            Blinks::Yes
        );
        assert_eq!(
            blinks(CursorStyle::BlinkingBar, Focus::Gained, on),
            Blinks::Yes
        );
        assert_eq!(
            blinks(CursorStyle::SteadyBlock, Focus::Gained, on),
            Blinks::No
        );
        assert_eq!(
            blinks(CursorStyle::BlinkingBlock, Focus::Lost, on),
            Blinks::No
        );
        assert_eq!(
            blinks(CursorStyle::BlinkingBlock, Focus::Gained, none),
            Blinks::No
        );
    }
}
