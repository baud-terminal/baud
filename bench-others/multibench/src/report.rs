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

/// One table row. Ranges are printed, not summarised away: if two cores' ranges
/// overlap there is no result, and the reader has to be able to see that. A
/// recording that was not timed, because the screen check did not pass on it,
/// is `None` and prints no rate.
pub(crate) fn row(label: &str, s: Option<Summary>, t: Option<Summary>) {
    let column = |c: Option<Summary>| match c {
        Some((median, min, max)) => format!("{median:>7.1} [{min:>5.1}-{max:>5.1}]"),
        None => format!("{:>21}", "no rate"),
    };
    println!("  {:<28} {}  {}", label, column(s), column(t));
}
