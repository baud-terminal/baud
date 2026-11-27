//! **What a launcher shows for baud**: the progress a program reports and
//! whether it asks for attention, as libunity's `LauncherEntry` carries
//! them (`[[source: libunity-launcher-entry]]`).

use crate::{Attention, Percent, Progress, ProgressState};

/// **A fraction of one, from a percentage**: libunity's `progress`, "from
/// 0 to 1" as baud sends it, made only from a [`Percent`], which is
/// never past 100.
///
/// ```
/// let q = baud_domain::UnitInterval::of(baud_domain::Percent::clamped(25));
/// assert_eq!(q.get(), 0.25);
/// let full = baud_domain::UnitInterval::of(baud_domain::Percent::clamped(250));
/// assert_eq!(full.get(), 1.0);
/// ```
///
/// A fraction is made only from a percentage:
///
/// ```compile_fail,E0423
/// let _ = baud_domain::UnitInterval(1.5);
/// ```
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub struct UnitInterval(f64);

impl UnitInterval {
    /// The fraction `p` is.
    pub fn of(p: Percent) -> UnitInterval {
        UnitInterval(p.fraction())
    }

    /// The fraction, 0 to 1.
    pub fn get(self) -> f64 {
        self.0
    }
}

/// **A launcher's progress bar**: libunity's `progress` with its
/// `progress-visible`. The launcher has no states, so a program's error
/// and pause show as their fraction, and a state with no percentage as an
/// empty bar that runs.
///
/// ```
/// use baud_domain::{LauncherBar, Percent, Progress, ProgressState};
/// let bar = LauncherBar::of(Progress {
///     state: ProgressState::Normal,
///     percent: Some(Percent::clamped(25)),
/// });
/// assert_eq!(bar.fraction(), Some(0.25));
/// assert_eq!(LauncherBar::of(Progress::HIDDEN).fraction(), None);
/// ```
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LauncherBar {
    /// No bar.
    Hidden,
    /// A bar at this fraction.
    Fraction(UnitInterval),
    /// A bar with no fraction to show: an empty one.
    Running,
}

impl LauncherBar {
    /// The bar for a program's progress.
    pub fn of(p: Progress) -> LauncherBar {
        match (p.state, p.percent) {
            (ProgressState::Hidden, _) => LauncherBar::Hidden,
            (_, Some(pct)) => LauncherBar::Fraction(UnitInterval::of(pct)),
            (_, None) => LauncherBar::Running,
        }
    }

    /// The `progress` a launcher is sent while the bar shows, `None`
    /// while it does not: an empty bar is 0.
    pub fn fraction(self) -> Option<f64> {
        match self {
            LauncherBar::Hidden => None,
            LauncherBar::Fraction(f) => Some(f.get()),
            LauncherBar::Running => Some(0.0),
        }
    }
}

/// **Whether a launcher asks for attention**: libunity's `urgent`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LauncherUrgent {
    No,
    Yes,
}

impl LauncherUrgent {
    /// A program's attention as urgency: asked, once or for good, is
    /// urgent until the program says no.
    pub fn of(a: Attention) -> LauncherUrgent {
        match a {
            Attention::Yes | Attention::Once => LauncherUrgent::Yes,
            Attention::No => LauncherUrgent::No,
        }
    }
}

/// **What a launcher was last told**: the bar and the urgency, sent
/// together.
///
/// ```
/// use baud_domain::{Attention, LauncherUrgent};
/// let mut e = baud_domain::LauncherEntry::IDLE;
/// let told = e.heard(None, Some(Attention::Once)).expect("a change");
/// assert_eq!(told.urgent, LauncherUrgent::Yes);
/// assert_eq!(e.heard(None, Some(Attention::Yes)), None);
/// ```
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LauncherEntry {
    pub bar: LauncherBar,
    pub urgent: LauncherUrgent,
}

impl LauncherEntry {
    /// No bar and no request: what a launcher shows before a program says.
    pub const IDLE: LauncherEntry = LauncherEntry {
        bar: LauncherBar::Hidden,
        urgent: LauncherUrgent::No,
    };

    /// **A program's news, taken in**: the progress and the attention
    /// that differ from the last read's, either or both. The whole entry
    /// when that changes what the launcher shows, so it is sent; `None`
    /// when it does not.
    pub fn heard(
        &mut self,
        progress: Option<Progress>,
        attention: Option<Attention>,
    ) -> Option<LauncherEntry> {
        let mut next = *self;
        if let Some(p) = progress {
            next.bar = LauncherBar::of(p);
        }
        if let Some(a) = attention {
            next.urgent = LauncherUrgent::of(a);
        }
        if next == *self {
            return None;
        }
        *self = next;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Attention, LauncherBar, LauncherEntry, LauncherUrgent, Percent, Progress, ProgressState,
        UnitInterval,
    };

    fn p(state: ProgressState, pct: Option<u32>) -> Progress {
        Progress {
            state,
            percent: pct.map(Percent::clamped),
        }
    }

    fn at(n: u32) -> LauncherBar {
        LauncherBar::Fraction(UnitInterval::of(Percent::clamped(n)))
    }

    /// **A launcher shows a program's progress as its fraction**: hidden
    /// shows nothing, a state with a percentage its fraction, a state
    /// with none an empty bar that runs. The launcher has no states of its
    /// own, so an error and a pause show as their fraction.
    #[test]
    fn a_launcher_shows_the_programs_progress() {
        use ProgressState as S;
        assert_eq!(LauncherBar::of(p(S::Hidden, Some(5))), LauncherBar::Hidden);
        assert_eq!(LauncherBar::of(p(S::Normal, Some(25))), at(25));
        assert_eq!(LauncherBar::of(p(S::Error, Some(40))), at(40));
        assert_eq!(LauncherBar::of(p(S::Paused, Some(100))), at(100));
        assert_eq!(
            LauncherBar::of(p(S::Indeterminate, None)),
            LauncherBar::Running
        );
        assert_eq!(LauncherBar::of(p(S::Error, None)), LauncherBar::Running);
        assert_eq!(LauncherBar::Hidden.fraction(), None);
        assert_eq!(at(25).fraction(), Some(0.25));
        assert_eq!(at(0).fraction(), Some(0.0));
        assert_eq!(at(100).fraction(), Some(1.0));
        assert_eq!(LauncherBar::Running.fraction(), Some(0.0));
        for n in 0..=100 {
            let f = UnitInterval::of(Percent::clamped(n)).get();
            assert!((0.0..=1.0).contains(&f), "{n}: {f}");
        }
    }

    /// **A program's attention is the launcher's urgency**: asked, once
    /// or for good, it is urgent until the program says no.
    #[test]
    fn attention_is_urgency() {
        assert_eq!(LauncherUrgent::of(Attention::Yes), LauncherUrgent::Yes);
        assert_eq!(LauncherUrgent::of(Attention::Once), LauncherUrgent::Yes);
        assert_eq!(LauncherUrgent::of(Attention::No), LauncherUrgent::No);
    }

    /// **The launcher is told only of a change**: no news is no entry,
    /// news that changes nothing is none either, and each change is the
    /// whole entry, progress and urgency together.
    #[test]
    fn the_launcher_is_told_only_of_a_change() {
        use ProgressState as S;
        let mut e = LauncherEntry::IDLE;
        assert_eq!(e.bar, LauncherBar::Hidden);
        assert_eq!(e.urgent, LauncherUrgent::No);
        assert_eq!(e.heard(None, None), None, "no news");
        let quarter = LauncherEntry {
            bar: at(25),
            urgent: LauncherUrgent::No,
        };
        assert_eq!(e.heard(Some(p(S::Normal, Some(25))), None), Some(quarter));
        assert_eq!(
            e.heard(Some(p(S::Normal, Some(25))), None),
            None,
            "the same"
        );
        assert_eq!(
            e.heard(None, Some(Attention::Once)),
            Some(LauncherEntry {
                bar: at(25),
                urgent: LauncherUrgent::Yes,
            }),
            "the bar kept, urgency added"
        );
        assert_eq!(e.heard(None, Some(Attention::Yes)), None, "still urgent");
        assert_eq!(
            e.heard(Some(p(S::Hidden, None)), Some(Attention::No)),
            Some(LauncherEntry::IDLE),
            "both at once"
        );
        assert_eq!(e, LauncherEntry::IDLE);
    }
}
