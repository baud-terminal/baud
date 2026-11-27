//! **What the GPU arm does after a failure** (row C5): every failure hands
//! the frame to the CPU arm in the same turn, and [`Fallback`] decides what
//! the GPU arm does next. It is a pure function of the failures it is told
//! and the clock it is given:
//!
//! - a lost device is replaced by a new one once a process, [`RESTART_AFTER`]
//!   later -- the lost state belongs to the logical device and the physical
//!   one "may be otherwise unaffected" (`[[source: vulkan-devsandqueues]]`,
//!   Lost Device) -- and a second loss anywhere in the process, or a failed
//!   restart, turns the arm off for the window's life: a loss baud's own
//!   work causes would otherwise reset the GPU once for every window;
//! - memory running out while the targets are made waits for a smaller
//!   size, which may fit, so a window growing past what the device holds
//!   does not allocate and fail at every step;
//! - anything else -- no loader, memory running out elsewhere, the
//!   compositor refusing the buffers, any other failure -- turns it off.
//!
//! The wait before a restart is a deadline the driver's loop polls to,
//! never a sleep: the CPU arm draws every frame meanwhile.

use baud_present_vulkan::frame::Failed;
use baud_present_wayland::Px;
use baud_render::Tally;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long after a lost device the GPU arm makes a new one.
pub const RESTART_AFTER: Duration = Duration::from_secs(1);

/// A frame's size in pixels.
pub type Size = (Px, Px);

/// What failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// No Vulkan loader, or no instance from it.
    NoLoader,
    /// The device was lost.
    Lost,
    /// Memory ran out making the targets for this size.
    OutOfMemoryFitting(Size),
    /// Memory ran out anywhere else.
    OutOfMemory,
    /// The compositor refused the buffers, made at this size when it did.
    Refused(Option<Size>),
    /// Any other failure, as the device said it.
    Other(Failed),
}

/// What the GPU arm does after a failure; the CPU arm draws until then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Off for the window's life.
    Off,
    /// Tried again at a size smaller than this one.
    AtSmaller(Size),
    /// A new device made at this moment.
    RestartAt(Instant),
}

/// What [`Fallback::due`] says a turn should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// Make a new device now.
    Restart,
    /// Nothing yet.
    No,
}

/// Each failure counted, each restart made, and the size the compositor
/// last refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub no_loader: Tally,
    pub lost: Tally,
    pub restarted: Tally,
    pub oom: Tally,
    pub refused: Tally,
    pub refused_at: Option<Size>,
    pub other: Tally,
}

impl Counts {
    /// **The five-second report's GPU line**, once any count has moved:
    /// each failure by kind, the restarts, and the size the compositor last
    /// refused.
    pub fn line(&self) -> Option<String> {
        (*self != Counts::default()).then(|| {
            let at = match self.refused_at {
                Some((w, h)) => format!(" at {}x{}", w.0, h.0),
                None => String::new(),
            };
            format!(
                "gpu: {} lost, {} restarted, {} out of memory, {} refused{at}, {} no loader, {} other",
                self.lost.value(),
                self.restarted.value(),
                self.oom.value(),
                self.refused.value(),
                self.no_loader.value(),
                self.other.value(),
            )
        })
    }
}

/// Whether the one restart a process gets has been taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Restart {
    Unused,
    Used,
}

/// **The process's one restart**, shared by every window's [`Fallback`].
#[derive(Clone)]
pub struct Restarts(Arc<Mutex<Restart>>);

impl Restarts {
    /// A budget of its own, for a test's windows.
    pub fn new() -> Restarts {
        Restarts(Arc::new(Mutex::new(Restart::Unused)))
    }

    /// The process's budget.
    pub fn process() -> Restarts {
        static PROCESS: OnceLock<Restarts> = OnceLock::new();
        PROCESS.get_or_init(Restarts::new).clone()
    }

    /// **Take the restart**: whether it was still there.
    fn take(&self) -> Restart {
        let mut r = self.0.lock().unwrap_or_else(|e| e.into_inner());
        core::mem::replace(&mut *r, Restart::Used)
    }
}

/// A window's decision after each failure, and the restart it waits for.
pub struct Fallback {
    restarts: Restarts,
    pending: Option<Instant>,
    counts: Counts,
}

impl Fallback {
    pub fn new(restarts: Restarts) -> Fallback {
        Fallback {
            restarts,
            pending: None,
            counts: Counts::default(),
        }
    }

    /// **`failure` at `now`**: counted, and what the arm does next.
    pub fn failed(&mut self, failure: Failure, now: Instant) -> Next {
        let next = match failure {
            Failure::NoLoader => {
                self.counts.no_loader.bump();
                Next::Off
            }
            Failure::Lost => {
                self.counts.lost.bump();
                match self.restarts.take() {
                    Restart::Unused => Next::RestartAt(now + RESTART_AFTER),
                    Restart::Used => Next::Off,
                }
            }
            Failure::OutOfMemoryFitting(size) => {
                self.counts.oom.bump();
                Next::AtSmaller(size)
            }
            Failure::OutOfMemory => {
                self.counts.oom.bump();
                Next::Off
            }
            Failure::Refused(size) => {
                self.counts.refused.bump();
                self.counts.refused_at = size;
                Next::Off
            }
            Failure::Other(_) => {
                self.counts.other.bump();
                Next::Off
            }
        };
        self.pending = match next {
            Next::RestartAt(at) => Some(at),
            Next::Off | Next::AtSmaller(_) => None,
        };
        next
    }

    /// **When the loop must wake for a restart**, while one waits.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending
    }

    /// **Whether a restart is due at `now`**: once, at its deadline, and
    /// counted then.
    pub fn due(&mut self, now: Instant) -> Due {
        match self.pending {
            Some(at) if now >= at => {
                self.pending = None;
                self.counts.restarted.bump();
                Due::Restart
            }
            _ => Due::No,
        }
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }
}

/// **Whether `size` is smaller than `failed`**: no side longer and not the
/// same size.
pub fn smaller(size: Size, failed: Size) -> bool {
    size.0.0 <= failed.0.0 && size.1.0 <= failed.1.0 && size != failed
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// **A lost device is replaced once, a second later, and the second
    /// loss is final**: the restart waits for its deadline, is due once,
    /// and a loss after it turns the arm off.
    #[test]
    fn a_lost_device_is_replaced_once_after_its_deadline() {
        let t = Instant::now();
        let mut f = Fallback::new(Restarts::new());
        assert_eq!(
            f.failed(Failure::Lost, t),
            Next::RestartAt(t + RESTART_AFTER)
        );
        assert_eq!(f.deadline(), Some(t + RESTART_AFTER));
        assert_eq!(
            f.due(t + RESTART_AFTER - MS),
            Due::No,
            "before its deadline"
        );
        assert_eq!(f.due(t + RESTART_AFTER), Due::Restart);
        assert_eq!(f.due(t + RESTART_AFTER + MS), Due::No, "once");
        assert_eq!(f.deadline(), None);
        let later = t + RESTART_AFTER * 5;
        assert_eq!(f.failed(Failure::Lost, later), Next::Off, "the second loss");
        assert_eq!(f.deadline(), None);
        let c = f.counts();
        assert_eq!((c.lost.value(), c.restarted.value()), (2, 1));
    }

    /// **The restart is the process's, not each window's**: two windows
    /// each losing the device make one restart between them, and the
    /// other is off.
    #[test]
    fn two_windows_losing_the_device_share_one_restart() {
        let t = Instant::now();
        let shared = Restarts::new();
        let (mut a, mut b) = (Fallback::new(shared.clone()), Fallback::new(shared));
        assert_eq!(
            a.failed(Failure::Lost, t),
            Next::RestartAt(t + RESTART_AFTER)
        );
        assert_eq!(b.failed(Failure::Lost, t), Next::Off, "the second window");
        assert_eq!(a.due(t + RESTART_AFTER), Due::Restart);
        assert_eq!(a.failed(Failure::Lost, t + RESTART_AFTER * 2), Next::Off);
        let restarted = a.counts().restarted.value() + b.counts().restarted.value();
        assert_eq!(restarted, 1);
    }

    /// **Memory running out for the targets waits for a smaller size;
    /// anywhere else it is final**, and both are counted as one kind.
    #[test]
    fn memory_running_out_waits_for_a_smaller_size_only_while_fitting() {
        let t = Instant::now();
        let big = (Px(7680), Px(4320));
        let mut f = Fallback::new(Restarts::new());
        assert_eq!(
            f.failed(Failure::OutOfMemoryFitting(big), t),
            Next::AtSmaller(big)
        );
        assert_eq!(f.failed(Failure::OutOfMemory, t), Next::Off);
        assert_eq!(f.counts().oom.value(), 2);
        assert_eq!(f.deadline(), None, "no restart waits");
        assert!(smaller((Px(3840), Px(2160)), big));
        assert!(smaller((Px(7680), Px(4319)), big), "one side shorter");
        assert!(!smaller(big, big), "the same size");
        assert!(!smaller((Px(7681), Px(100)), big), "one side longer");
    }

    /// **Everything else is final and counted under its own name**, and
    /// nothing else moves; a refusal keeps its size.
    #[test]
    fn every_other_failure_is_final_and_counted_by_name() {
        let t = Instant::now();
        let at = Some((Px(800), Px(600)));
        let code = Failed::NoGraphicsQueue;
        for failure in [
            Failure::NoLoader,
            Failure::Refused(at),
            Failure::Other(code),
        ] {
            let mut f = Fallback::new(Restarts::new());
            assert_eq!(f.failed(failure, t), Next::Off, "{failure:?}");
            let mut only = Counts::default();
            match failure {
                Failure::NoLoader => only.no_loader.bump(),
                Failure::Refused(size) => {
                    only.refused.bump();
                    only.refused_at = size;
                }
                _ => only.other.bump(),
            }
            assert_eq!(f.counts(), only, "{failure:?} alone");
        }
    }

    /// **The report's line waits for a count to move**, then names each
    /// kind and the size last refused.
    #[test]
    fn the_report_line_names_each_count_once_one_moves() {
        let t = Instant::now();
        let mut f = Fallback::new(Restarts::new());
        assert_eq!(f.counts().line(), None, "nothing moved");
        let _ = f.failed(Failure::Lost, t);
        let _ = f.due(t + RESTART_AFTER);
        let _ = f.failed(Failure::Refused(Some((Px(800), Px(600)))), t);
        assert_eq!(
            f.counts().line().as_deref(),
            Some(
                "gpu: 1 lost, 1 restarted, 0 out of memory, 1 refused at 800x600, 0 no loader, 0 other"
            )
        );
    }

    /// **A failure while a restart waits cancels it**: memory running out
    /// then leaves nothing due.
    #[test]
    fn a_later_failure_replaces_a_waiting_restart() {
        let t = Instant::now();
        let mut f = Fallback::new(Restarts::new());
        let _ = f.failed(Failure::Lost, t);
        let _ = f.failed(Failure::OutOfMemory, t + MS);
        assert_eq!(f.deadline(), None);
        assert_eq!(f.due(t + RESTART_AFTER * 2), Due::No);
        assert_eq!(f.counts().restarted.value(), 0);
    }
}
