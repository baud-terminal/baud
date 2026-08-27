/// Median and range over collected samples.
///
/// Was `best()`, which returned the maximum and discarded everything else -- an
/// upward-biased estimator that also hides variance, and variance here is the
/// whole problem.
pub(crate) fn summarize(v: &mut [f64]) -> (f64, f64, f64) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (v[v.len() / 2], v[0], v[v.len() - 1])
}

/// One table row. Ranges are printed, not summarised away: if two cores' ranges
/// overlap there is no result, and the reader has to be able to see that.
pub(crate) fn row(label: &str, s: (f64, f64, f64), t: (f64, f64, f64)) {
    println!(
        "  {:<28} {:>7.1} [{:>5.1}-{:>5.1}]  {:>7.1} [{:>5.1}-{:>5.1}]",
        label, s.0, s.1, s.2, t.0, t.1, t.2
    );
}
