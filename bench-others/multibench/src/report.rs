/// A median and its range, `(median, min, max)`.
pub(crate) type Summary = (f64, f64, f64);

/// Median and range over collected samples.
///
/// Was `best()`, which returned the maximum and discarded everything else -- an
/// upward-biased estimator that also hides variance, and variance here is the
/// whole problem.
pub(crate) fn summarize(v: &mut [f64]) -> Summary {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (v[v.len() / 2], v[0], v[v.len() - 1])
}

/// One recording's rate on one core, as the table prints it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Rate {
    /// Not timed: the screen check did not pass on the recording.
    Untimed,
    /// Timed, the two arms shown to have done the same work.
    Quoted(Summary),
    /// Timed, equal work not shown: a compared count was "n/a" on an arm
    /// (UNEQUAL-UNKNOWN). The median is printed and is not a result.
    Unquotable(Summary),
    /// Timed, its screen shown to differ from baud-core's by its own gate
    /// (`screens`): a rate for other work, so none is printed.
    Unequal,
}

impl Rate {
    /// The table's 21-column cell.
    pub(crate) fn cell(self) -> String {
        match self {
            Rate::Quoted((median, min, max)) => format!("{median:>7.1} [{min:>5.1}-{max:>5.1}]"),
            Rate::Unquotable((median, _, _)) => {
                format!("{:>21}", format!("unquotable {median:.1}"))
            }
            Rate::Untimed | Rate::Unequal => format!("{:>21}", "no rate"),
        }
    }
}

/// One table row. Ranges are printed, not summarised away: if two cores' ranges
/// overlap there is no result, and the reader has to be able to see that.
pub(crate) fn row(label: &str, s: Rate, t: Rate) {
    let column = Rate::cell;
    println!("  {:<28} {}  {}", label, column(s), column(t));
}
