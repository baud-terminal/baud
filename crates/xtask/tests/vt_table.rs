//! `cargo xtask vt-table`: a whole table as the script printed it, and a
//! perturbation per branch. Every expected output here is the script's,
//! under Python 3.13.12.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::features_since::Exit;
use xtask::html::Count;
use xtask::vt_table::{self, fixed, float, int, seconds, sorted};

struct Files(PathBuf);

static NEXT: AtomicUsize = AtomicUsize::new(0);

impl Files {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vt-table-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("the directory");
        Self(dir)
    }

    fn file(&self, name: &str, text: &[u8]) -> String {
        let path = self.0.join(name);
        fs::write(&path, text).expect("the file");
        path.to_str().expect("utf-8").to_string()
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> (Exit, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = vt_table::run(args, &mut out, &mut err).expect("writers");
    (
        exit,
        String::from_utf8(out).expect("utf-8"),
        String::from_utf8(err).expect("utf-8"),
    )
}

/// Three rounds, whose rates are 1, 1 and 3 MiB/s at settled 2 s and 2 s.
const ROUNDS: &str = "\
baud cat 200x50 9999999 0 1 0:09.00 baud R52
baud cat 200x50 2097152 0 2 0:00.125 baud R52
baud cat 200x50 6291456 0 2 0:00.375 baud R52
";

/// `Rate: .0f` lands exactly on ties (0.5 and 2.5 MiB/s); `cpu: .2f` on 0.125
/// and 0.375, which are exact in binary too.
#[test]
fn a_whole_table_is_the_scripts_ties_and_all() {
    let files = Files::new();
    let path = files.file("r", ROUNDS.as_bytes());
    let (exit, out, err) = run(&[&path]);
    assert_eq!((exit, err.as_str()), (Exit(0), ""));
    assert_eq!(
        out,
        "Throughput, MiB/s, median of the settled rounds [range]:

| recording | baud |
|---|---|
| cat | **3** [1-3] |

CPU seconds at the end of the run, startup included, median of the settled rounds [range]:

| recording | baud |
|---|---|
| cat | **0.38** [0.12-0.38] |

| terminal | geometry the child saw | process measured | startup CPU, s |
|---|---|---|---|
| baud | 200x50 | baud | nan |
"
    );
}

#[test]
fn a_baseline_subtracts_startup_and_changes_the_label() {
    let files = Files::new();
    let path = files.file("r", ROUNDS.as_bytes());
    let base = files.file(
        "b",
        b"baud cat 200x50 1 0 1 0:09.00 baud R52\nbaud cat 200x50 1 0 1 0:00.125 baud R52\n",
    );
    let (exit, out, _) = run(&[&path, &base]);
    assert_eq!(exit, Exit(0));
    assert!(
        out.contains("CPU seconds for the run, startup subtracted, median"),
        "{out}"
    );
    assert!(out.contains("| cat | **0.25** [0.00-0.25] |"), "{out}");
    assert!(out.contains("| baud | 200x50 | baud | 0.12 |"), "{out}");
    // An empty baseline is no baseline: the label says so too.
    let empty = files.file("e", b"");
    let (_, out, _) = run(&[&path, &empty]);
    assert!(out.contains("startup included"), "{out}");
    assert!(out.contains("| baud | 200x50 | baud | nan |"), "{out}");
}

#[test]
fn a_term_with_no_rows_for_a_recording_prints_dashes_and_a_lone_round_is_kept() {
    let files = Files::new();
    let path = files.file(
        "r",
        b"a x s 1048576 0 1 0:01 c 52\nb y s 1048576 0 1 0:02 d 52\n",
    );
    let (exit, out, _) = run(&[&path]);
    assert_eq!(exit, Exit(0));
    assert!(out.contains("| x | **1** [1-1] | -- |"), "{out}");
    assert!(out.contains("| y | -- | **1** [1-1] |"), "{out}");
    assert!(out.contains("| recording | a | b |"), "{out}");
    assert!(out.contains("|---|---|---|"), "{out}");
}

#[test]
fn a_run_with_no_cursor_report_is_named_and_at_most_eight_are() {
    let files = Files::new();
    let mut text = String::new();
    for i in 0..10 {
        text.push_str(&format!("t{i} r s 1 0 1 0:01 c x\n"));
    }
    text.push_str("t9 r s 1 0 1 0:01 c\n");
    let path = files.file("r", text.as_bytes());
    let (exit, out, _) = run(&[&path]);
    assert_eq!(exit, Exit(0));
    assert!(
        out.ends_with(
            "\n11 runs did not end in a cursor report and are not results: \
             t0/r, t1/r, t2/r, t3/r, t4/r, t5/r, t6/r, t7/r\n"
        ),
        "{out}"
    );
}

#[test]
fn a_line_short_of_eight_fields_is_passed_over_and_a_ninth_is_the_reply() {
    let files = Files::new();
    let path = files.file("r", b"too few\na r s 1 0 1 0:01 c 52 and more\n");
    let (exit, out, _) = run(&[&path]);
    assert_eq!(exit, Exit(0));
    assert!(!out.contains("too"), "{out}");
    assert!(!out.contains("did not end"), "{out}");
}

#[test]
fn the_line_ends_and_the_separators_are_pythons() {
    let files = Files::new();
    for text in [
        &b"a\x1cr s 1 0 1 0:01 c 52\r\n"[..],
        b"a\x0br s 1 0 1 0:01 c 52\r",
        b"a\xc2\xa0r s 1 0 1 0:01 c 52\n",
    ] {
        let path = files.file("r", text);
        let (exit, out, _) = run(&[&path]);
        assert_eq!(exit, Exit(0));
        assert!(out.contains("| r | **0** [0-0] |"), "{text:?}: {out}");
    }
}

#[test]
fn what_the_script_raised_on_exits_1_after_what_it_printed() {
    let files = Files::new();
    // Nothing printed: `load` stops first.
    for (name, line) in [
        ("int", "a r s x 0 1 0:01 c 52\n"),
        ("t0", "a r s 1 x 1 0:01 c 52\n"),
        ("t1", "a r s 1 0 x 0:01 c 52\n"),
        ("minutes", "a r s 1 0 1 x:01 c 52\n"),
        ("seconds", "a r s 1 0 1 0:x c 52\n"),
        ("hours", "a r s 1 0 1 x:0:1 c 52\n"),
        ("empty", "a r s 1 0 1 :01 c 52\n"),
    ] {
        let path = files.file("r", line.as_bytes());
        let (exit, out, err) = run(&[&path]);
        assert_eq!((exit, out.as_str()), (Exit(1), ""), "{name}");
        assert!(err.starts_with("vt-table: "), "{name}: {err}");
    }
    // A zero wall in the settled rounds: the header is out by then.
    let path = files.file("r", b"a r s 1 5 5 0:01 c 52\n");
    let (exit, out, err) = run(&[&path]);
    assert_eq!(exit, Exit(1));
    assert!(out.starts_with("Throughput"), "{out}");
    assert!(!out.contains("**"), "{out}");
    assert!(err.contains("division by zero"), "{err}");
    // In a discarded round, it is never divided by.
    let path = files.file("r", b"a r s 1 5 5 0:01 c 52\na r s 1 0 1 0:01 c 52\n");
    assert_eq!(run(&[&path]).0, Exit(0));
    // No argument, no file, and bytes that are not UTF-8.
    assert_eq!(run(&[]).0, Exit(1));
    assert_eq!(run(&["/nonexistent/r"]).0, Exit(1));
    let path = files.file("r", b"a r s 1 2 3 0:01 c \xff\n");
    assert_eq!(run(&[&path]).0, Exit(1));
    // A word past the two it reads is ignored.
    let path = files.file("r", ROUNDS.as_bytes());
    assert_eq!(run(&[&path, &path, "x", "y"]).0, Exit(0));
}

/// A total past a float's range raised only where it was divided.
#[test]
fn a_total_too_large_for_a_float_stops_only_where_it_is_divided() {
    let files = Files::new();
    let huge = "9".repeat(400);
    let line = |total: &str| format!("a r s {total} 0 1 0:01 c 52\n");
    // In a discarded round, and in a baseline, it is never divided.
    let good = line("1048576");
    let discarded = format!("{}{good}", line(&huge));
    let path = files.file("r", discarded.as_bytes());
    assert_eq!(run(&[&path]).0, Exit(0));
    let base = files.file("b", line(&huge).as_bytes());
    let plain = files.file("p", good.as_bytes());
    assert_eq!(run(&[&plain, &base]).0, Exit(0));
    // In a settled one, the header is out and it stops.
    let settled = format!("{good}{}", line(&huge));
    let path = files.file("s", settled.as_bytes());
    let (exit, out, err) = run(&[&path]);
    assert_eq!(exit, Exit(1));
    assert!(out.starts_with("Throughput"), "{out}");
    assert!(!out.contains("**"), "{out}");
    assert!(err.contains("too large"), "{err}");
    // Past `int`'s digit limit it stops at reading, as it raised.
    let path = files.file("l", line(&"1".repeat(4301)).as_bytes());
    let (exit, out, _) = run(&[&path]);
    assert_eq!((exit, out.as_str()), (Exit(1), ""));
    let path = files.file("k", line(&"1".repeat(300)).as_bytes());
    assert_eq!(run(&[&path]).0, Exit(0));
}

/// A minutes field reads as a float and overflows once multiplied, at
/// reading, where the script's `int * 60 + float` raised.
#[test]
fn a_cpu_time_too_large_for_a_float_stops_at_reading() {
    let nines = "9".repeat(308);
    assert!(int(&nines).expect("in range").is_finite());
    assert!(seconds(&format!("{nines}:01")).is_err());
    assert!(seconds(&format!("1:{nines}:01")).is_err());
    assert!(seconds(&format!("{nines}:0:01")).is_err());
    assert!(seconds(&format!("{}:01", "9".repeat(400))).is_err());
    assert!(seconds("0:inf").expect("a float may be").is_infinite());
}

#[test]
fn numbers_read_as_python_reads_them() {
    assert_eq!(int("1_000"), Ok(1000.0));
    assert_eq!(int("+7"), Ok(7.0));
    assert_eq!(int("-7"), Ok(-7.0));
    assert_eq!(int("007"), Ok(7.0));
    for bad in ["", "_1", "1_", "1__0", "1.0", "x", "1e3", "+"] {
        assert!(int(bad).is_err(), "{bad:?}");
    }
    assert_eq!(float("1_0.5"), Ok(10.5));
    assert_eq!(float("+1.5e0"), Ok(1.5));
    assert_eq!(float(".5"), Ok(0.5));
    assert_eq!(float("5."), Ok(5.0));
    assert_eq!(float("1E3"), Ok(1000.0));
    assert_eq!(float("inf"), Ok(f64::INFINITY));
    assert_eq!(float("-Infinity"), Ok(f64::NEG_INFINITY));
    assert!(float("NaN").expect("nan").is_nan());
    for bad in ["", "_1.0", "1._0", "1.0_", "x", "."] {
        assert!(float(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn seconds_reads_two_or_three_fields_and_anything_else_is_nan() {
    assert_eq!(seconds("1:30.5"), Ok(90.5));
    assert_eq!(seconds("1:02:03.5"), Ok(3723.5));
    assert!(seconds("5.0").expect("no colon").is_nan());
    assert!(seconds("N/A").expect("no colon").is_nan());
    assert!(seconds("1:2:3:4").expect("three colons").is_nan());
    assert!(seconds("x:1").is_err());
}

#[test]
fn fixed_spells_what_python_spells() {
    let w = Count(0);
    assert_eq!(fixed(f64::NAN, w), "nan");
    assert_eq!(fixed(f64::INFINITY, w), "inf");
    assert_eq!(fixed(f64::NEG_INFINITY, w), "-inf");
    assert_eq!(fixed(-0.0, Count(2)), "-0.00");
    // Half to even, on the exact binary value.
    assert_eq!(fixed(0.5, w), "0");
    assert_eq!(fixed(1.5, w), "2");
    assert_eq!(fixed(2.5, w), "2");
    assert_eq!(fixed(0.125, Count(2)), "0.12");
    assert_eq!(fixed(0.375, Count(2)), "0.38");
    assert_eq!(fixed(1.005, Count(2)), "1.00");
}

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x == y || (x.is_nan() && y.is_nan()))
}

/// With a `nan`, the answer is whatever the algorithm's `<` did: each `want`
/// is `sorted()` under CPython 3.13.12.
#[test]
fn sorted_with_a_nan_leaves_it_where_python_does() {
    let n = f64::NAN;
    let table: [(&[f64], &[f64]); 8] = [
        (&[1.0, n, 0.5], &[1.0, n, 0.5]),
        (&[n, 1.0, 0.5], &[0.5, n, 1.0]),
        (&[3.0, 2.0, n, 1.0], &[2.0, n, 1.0, 3.0]),
        (&[2.0, 2.0, 1.0, n], &[1.0, n, 2.0, 2.0]),
        (&[1.0, 2.0, n, 0.0, 3.0], &[1.0, 2.0, n, 0.0, 3.0]),
        (&[n, n, 1.0], &[n, n, 1.0]),
        (&[5.0, 4.0, 4.0, 3.0, n, 9.0], &[3.0, n, 9.0, 4.0, 4.0, 5.0]),
        (&[1.0, 1.0, 0.0, 0.0, n], &[0.0, 0.0, n, 1.0, 1.0]),
    ];
    for (input, want) in table {
        let got = sorted(input.to_vec());
        assert!(same(&got, want), "{input:?}: {got:?}, want {want:?}");
    }
}

#[test]
fn sorted_without_one_is_a_stable_sort() {
    assert_eq!(sorted(vec![3.0, 1.0, 2.0]), [1.0, 2.0, 3.0]);
    assert_eq!(sorted(Vec::new()), [0.0; 0]);
    // `-0.0` and `0.0` are equal, so they stay in the order they came.
    let got = sorted(vec![0.0, -0.0]);
    assert!(got[0].is_sign_positive() && got[1].is_sign_negative());
}
