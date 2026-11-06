//! The table: the same header and rows `multibench` prints for a core.

/// Median, minimum and maximum.
pub type Stat = (f64, f64, f64);

/// Median and range of `v`. An even count takes the upper of the middle two.
pub fn stat(v: &mut [f64]) -> Stat {
    v.sort_by(f64::total_cmp);
    (v[v.len() / 2], v[0], v[v.len() - 1])
}

pub fn header(chunk: usize, reps: usize, stream: usize, tui: usize) -> String {
    format!(
        "  kitty via Screen.test_* -- same payloads, same real PTY, {chunk}-byte reads, median of {reps}\n\
         \x20 stream.bin {stream} B, tui.bin {tui} B\n\n\
         \x20 {:<28} {:>21}  {:>21}",
        "core", "streaming median [min-max]", "TUI median [min-max]"
    )
}

pub fn row(label: &str, s: Stat, t: Stat) -> String {
    format!(
        "  {:<28} {:>7.1} [{:>5.1}-{:>5.1}]  {:>7.1} [{:>5.1}-{:>5.1}]",
        label, s.0, s.1, s.2, t.0, t.1, t.2
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_and_the_range_is_the_ends() {
        assert_eq!(stat(&mut [3.0, 1.0, 2.0]), (2.0, 1.0, 3.0));
    }

    #[test]
    fn the_median_of_an_even_count_is_the_upper_middle() {
        assert_eq!(stat(&mut [4.0, 1.0, 2.0, 3.0]), (3.0, 1.0, 4.0));
    }

    /// The lines the Python harness this replaced printed, from a run of it.
    #[test]
    fn the_table_keeps_its_layout() {
        assert_eq!(
            header(4096, 9, 524192, 524124),
            "  kitty via Screen.test_* -- same payloads, same real PTY, 4096-byte reads, median of 9\n\
             \x20 stream.bin 524192 B, tui.bin 524124 B\n\n\
             \x20 core                         streaming median [min-max]   TUI median [min-max]"
        );
        assert_eq!(
            row(
                "kitty (Screen.test_*)",
                (107.6, 105.6, 108.4),
                (165.7, 143.9, 168.1)
            ),
            "  kitty (Screen.test_*)          107.6 [105.6-108.4]    165.7 [143.9-168.1]"
        );
    }

    #[test]
    fn one_sample_is_its_own_range() {
        assert_eq!(stat(&mut [5.0]), (5.0, 5.0, 5.0));
    }
}
