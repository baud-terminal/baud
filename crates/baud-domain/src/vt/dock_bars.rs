//! **The macOS Dock tile's progress bar**: a program's progress as the
//! tile draws it ([`DockBar`]), and the one bar the tile shows over every
//! window's ([`DockBars`]).

#[cfg(feature = "alloc")]
use alloc::vec::Vec;
#[cfg(feature = "alloc")]
use core::cmp::Ordering;

use crate::{Progress, ProgressState, UnitInterval};

/// **The Dock tile's bar**: how full, `None` for a bar that runs with no
/// fraction, and its tone.
///
/// ```
/// use baud_domain::{DockBar, DockTone, Percent, Progress, ProgressState};
/// let b = DockBar::of(Progress {
///     state: ProgressState::Paused,
///     percent: Some(Percent::clamped(40)),
/// })
/// .expect("a bar");
/// assert_eq!(b.tone, DockTone::Paused);
/// assert_eq!(b.fraction.map(baud_domain::UnitInterval::get), Some(0.4));
/// assert_eq!(DockBar::of(Progress::HIDDEN), None);
/// ```
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DockBar {
    pub fraction: Option<UnitInterval>,
    pub tone: DockTone,
}

/// The tone the tile draws a bar in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockTone {
    Normal,
    Error,
    Paused,
}

impl DockBar {
    /// The tile's bar for a program's progress: none while it is hidden.
    /// Indeterminate draws as normal, with no fraction unless the program
    /// gave one.
    pub fn of(p: Progress) -> Option<DockBar> {
        let tone = match p.state {
            ProgressState::Hidden => return None,
            ProgressState::Normal | ProgressState::Indeterminate => DockTone::Normal,
            ProgressState::Error => DockTone::Error,
            ProgressState::Paused => DockTone::Paused,
        };
        Some(DockBar {
            fraction: p.percent.map(UnitInterval::of),
            tone,
        })
    }
}

/// How urgent a window's bar is on the one tile, most urgent first: the
/// Windows taskbar's order for a group, `TBPF_ERROR`, `TBPF_PAUSED`,
/// `TBPF_NORMAL`, `TBPF_INDETERMINATE` (see [`DockBars`]). A bar with no
/// fraction draws as running with none, which is indeterminate's.
#[cfg(feature = "alloc")]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum DockUrgency {
    Error,
    Paused,
    Determinate,
    Indeterminate,
}

#[cfg(feature = "alloc")]
impl DockUrgency {
    fn of(b: &DockBar) -> DockUrgency {
        match (b.tone, b.fraction) {
            (DockTone::Error, _) => DockUrgency::Error,
            (DockTone::Paused, _) => DockUrgency::Paused,
            (DockTone::Normal, Some(_)) => DockUrgency::Determinate,
            (DockTone::Normal, None) => DockUrgency::Indeterminate,
        }
    }
}

/// **The one Dock tile's bar, over every window's progress**, keyed by
/// window. The app has one tile and each window its own progress, so the
/// tile shows the rule the Windows taskbar applies when one button stands
/// for several windows, the system the progress report was made for (`[[source: conemu-ansi-escape-codes]]`):
/// "The taskbar button can show a progress indicator for only one window
/// at a time ... the taskbar button chooses its progress display based on
/// state priority", error, paused, normal, indeterminate, and "In the case
/// of a priority collision between two windows that are broadcasting
/// determinate progress, the window with the least progress is used" (`[[source: ms-itaskbarlist3-setprogressstate]]`).
/// Among bars of one priority and equal progress, the one set longest ago
/// is shown. A window's close takes its bar with it.
///
/// ```
/// use baud_domain::{DockBar, DockBars, DockTone, Percent, UnitInterval};
/// let at = |p| DockBar {
///     fraction: Some(UnitInterval::of(Percent::clamped(p))),
///     tone: DockTone::Normal,
/// };
/// let mut tile = DockBars::<u8>::new();
/// assert_eq!(tile.set(1, Some(at(70))), Some(Some(at(70))));
/// assert_eq!(tile.set(2, Some(at(20))), Some(Some(at(20))), "the least");
/// assert_eq!(tile.set(1, Some(at(80))), None, "nothing new to draw");
/// assert_eq!(tile.gone(2), Some(Some(at(80))));
/// ```
///
/// The bar the tile shows is only the one `set` and `gone` chose:
///
/// ```compile_fail,E0451
/// let _ = baud_domain::DockBars::<u8> {
///     each: Vec::new(),
///     shown: None,
/// };
/// ```
#[cfg(feature = "alloc")]
#[derive(Clone, Debug)]
pub struct DockBars<K> {
    each: Vec<(K, DockBar)>,
    shown: Option<DockBar>,
}

#[cfg(feature = "alloc")]
impl<K: PartialEq> DockBars<K> {
    /// No window's bar, and none shown.
    pub const fn new() -> DockBars<K> {
        DockBars {
            each: Vec::new(),
            shown: None,
        }
    }

    /// Window `k`'s bar is now `bar`, `None` when it shows no progress;
    /// the tile's bar when that changes it.
    pub fn set(&mut self, k: K, bar: Option<DockBar>) -> Option<Option<DockBar>> {
        self.each.retain(|(w, _)| *w != k);
        if let Some(b) = bar {
            self.each.push((k, b));
        }
        self.chosen()
    }

    /// Window `k` has closed; the tile's bar when that changes it.
    pub fn gone(&mut self, k: K) -> Option<Option<DockBar>> {
        self.set(k, None)
    }

    /// The tile's bar, and whether it differs from the one last shown.
    fn chosen(&mut self) -> Option<Option<DockBar>> {
        let top = self.each.iter().map(|(_, b)| DockUrgency::of(b)).min();
        let pick = top.and_then(|u| {
            self.each
                .iter()
                .map(|(_, b)| *b)
                .filter(|b| DockUrgency::of(b) == u)
                .min_by(|x, y| match (x.fraction, y.fraction) {
                    (Some(a), Some(b)) => a.get().total_cmp(&b.get()),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                })
        });
        (pick != self.shown).then(|| {
            self.shown = pick;
            pick
        })
    }
}

#[cfg(feature = "alloc")]
impl<K: PartialEq> Default for DockBars<K> {
    fn default() -> DockBars<K> {
        DockBars::new()
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use crate::{DockBar, DockBars, DockTone, Percent, Progress, ProgressState, UnitInterval};

    fn bar(tone: DockTone, percent: Option<u32>) -> Option<DockBar> {
        Some(DockBar {
            fraction: percent.map(|p| UnitInterval::of(Percent::clamped(p))),
            tone,
        })
    }

    /// **A program's progress as the tile's bar**: none while hidden;
    /// normal and indeterminate draw as normal, error and paused in their
    /// own tone; the fraction is the percentage's, and none without one.
    #[test]
    fn the_tile_shows_a_programs_progress() {
        use ProgressState::{Error, Hidden, Indeterminate, Normal, Paused};
        let of = |state, percent: Option<u32>| {
            DockBar::of(Progress {
                state,
                percent: percent.map(Percent::clamped),
            })
        };
        assert_eq!(of(Hidden, Some(40)), None, "hidden: no bar");
        assert_eq!(of(Normal, Some(40)), bar(DockTone::Normal, Some(40)));
        assert_eq!(of(Indeterminate, None), bar(DockTone::Normal, None));
        assert_eq!(of(Error, Some(10)), bar(DockTone::Error, Some(10)));
        assert_eq!(of(Paused, None), bar(DockTone::Paused, None));
        assert_eq!(
            of(Normal, Some(40))
                .and_then(|b| b.fraction)
                .map(UnitInterval::get),
            Some(0.4)
        );
    }

    /// **The one Dock tile shows the most urgent window's bar, the Windows
    /// taskbar's group rule**: error over paused over determinate over
    /// indeterminate; among determinate, the least progress; indeterminate
    /// only while every window with progress is; a window that clears its
    /// progress or closes takes its bar away and the next one shows. The
    /// control: one window's bar alone is the tile's.
    #[test]
    fn the_dock_shows_the_most_urgent_windows_progress() {
        use DockTone::{Error, Normal, Paused};
        let mut d = DockBars::new();
        assert_eq!(
            d.set(1, bar(Normal, Some(70))),
            Some(bar(Normal, Some(70))),
            "the control"
        );
        assert_eq!(
            d.set(2, bar(Normal, None)),
            None,
            "a determinate bar over an indeterminate one"
        );
        assert_eq!(
            d.set(3, bar(Normal, Some(20))),
            Some(bar(Normal, Some(20))),
            "the least"
        );
        assert_eq!(d.set(4, bar(Paused, Some(90))), Some(bar(Paused, Some(90))));
        assert_eq!(
            d.set(5, bar(Error, None)),
            Some(bar(Error, None)),
            "an error over all"
        );
        assert_eq!(
            d.set(3, bar(Normal, Some(25))),
            None,
            "a change under the error shows nothing new"
        );
        assert_eq!(
            d.gone(5),
            Some(bar(Paused, Some(90))),
            "the error's window closed"
        );
        assert_eq!(
            d.set(4, None),
            Some(bar(Normal, Some(25))),
            "paused cleared: the least again"
        );
        assert_eq!(d.gone(3), Some(bar(Normal, Some(70))));
        assert_eq!(
            d.gone(1),
            Some(bar(Normal, None)),
            "indeterminate once it is all there is"
        );
        assert_eq!(d.gone(2), Some(None), "the last window's bar gone: no bar");
        assert_eq!(d.gone(2), None, "a second close changes nothing");
    }
}
