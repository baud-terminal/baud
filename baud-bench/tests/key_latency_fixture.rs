//! `--selftest` over the committed fixture and over one perturbation of it
//! per statistic and parser branch. Each case's whole stdout and exit code
//! is the Python driver's, written to `tests/keylat/golden/perturbed/` by
//! running it on the same perturbed files before it was removed, and each
//! asserts the line that says what fired.

use baud_bench::key_latency::selftest::selftest;
use std::fs;
use std::path::{Path, PathBuf};

const KEYLAT: &str = "keylat-fixture.txt";
const POSTS: &str = "keylat-posts-fixture.txt";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Write the fixtures with `edits` made -- each must find what it replaces,
/// or the perturbation would be the unperturbed run in disguise -- and
/// compare the self-test with the Python's answer on the same files.
fn perturbed(name: &str, file: &str, edits: &[(&str, &str)], exit: i32, fired: &str) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("keylat-perturbed")
        .join(name);
    fs::create_dir_all(&dir).expect("a scratch directory");
    for f in [KEYLAT, POSTS] {
        let mut text = fs::read_to_string(root().join("tests/keylat").join(f)).expect("a fixture");
        if f == file {
            for (from, to) in edits {
                assert!(text.contains(from), "{name}: {from:?} is not in {f}");
                text = text.replace(from, to);
            }
        }
        fs::write(dir.join(f), text).expect("the perturbed fixture");
    }
    let dir_text = dir.display().to_string();
    let got = selftest(&dir_text).expect("the fixtures read");
    assert_eq!(got.code, exit, "{name}\n{}", got.stdout);
    assert!(
        got.stdout.contains(fired),
        "{name}: {fired:?}\n{}",
        got.stdout
    );
    let golden = root().join(format!("tests/keylat/golden/perturbed/{name}.txt"));
    let want = fs::read_to_string(&golden).unwrap_or_else(|e| panic!("{}: {e}", golden.display()));
    let text = format!(
        "exit {}\n{}",
        got.code,
        got.stdout.replace(&dir_text, "<fixtures>")
    );
    assert_eq!(text, want, "{name}");
}

const REFUSAL: &str = "keylat: landing refused ended=1003500390000 now=1003500385000\n";

/// Replace `from` with `to` in the first three records only.
fn three_rows(name: &str, from: &str, to: &str, fired: &str) {
    let text = fs::read_to_string(root().join("tests/keylat").join(KEYLAT)).expect("a fixture");
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("keylat: seq="))
        .collect();
    let edits: Vec<(String, String)> = lines[..3]
        .iter()
        .map(|l| (l.to_string(), l.replace(from, to)))
        .collect();
    let edits: Vec<(&str, &str)> = edits
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    perturbed(name, KEYLAT, &edits, 3, fired);
}

#[test]
fn a_fixture_that_is_not_utf8_exits_1() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("keylat-not-utf8");
    fs::create_dir_all(&dir).expect("a scratch directory");
    fs::write(dir.join(KEYLAT), b"keylat: \xff\n").expect("a fixture");
    fs::copy(root().join("tests/keylat").join(POSTS), dir.join(POSTS)).expect("a fixture");
    assert!(selftest(&dir.display().to_string()).is_err());
}

#[test]
fn a_record_dropped() {
    let text = fs::read_to_string(root().join("tests/keylat").join(KEYLAT)).expect("a fixture");
    let line = format!("{}\n", text.lines().nth(5).expect("seq=3"));
    assert!(line.starts_with("keylat: seq=3 "));
    perturbed(
        "record-dropped",
        KEYLAT,
        &[(&line, "")],
        3,
        "expected 9 keylat records, got 8",
    );
}

#[test]
fn a_post_dropped() {
    perturbed(
        "post-dropped",
        POSTS,
        &[("post 3 1000500000 d\n", "")],
        3,
        "expected 10 posts, got 9",
    );
}

#[test]
fn a_post_malformed() {
    perturbed(
        "post-malformed",
        POSTS,
        &[("post 4 1001000000 f", "post 4 x f")],
        3,
        "expected 10 posts, got 9",
    );
}

#[test]
fn no_parse_failure() {
    perturbed(
        "no-parse-failure",
        KEYLAT,
        &[("vsync=+400.0\n", "vsync=+400.0 vsync_hi=+416.7\n")],
        3,
        "expected one parse failure naming vsync_hi on seq=10, got []",
    );
}

#[test]
fn a_second_parse_failure_on_a_bare_value() {
    perturbed(
        "bare-value",
        KEYLAT,
        &[(
            "seq=2 event=1000010000 handled=+300.0",
            "seq=2 event=1000010000 handled=300.0",
        )],
        3,
        "handled=300.0 is neither +<us> nor -",
    );
}

#[test]
fn a_seq_that_is_no_integer() {
    perturbed(
        "seq-not-int",
        KEYLAT,
        &[("seq=2 ", "seq=x ")],
        3,
        "seq/event not integers",
    );
}

#[test]
fn an_event_that_is_no_integer() {
    perturbed(
        "event-not-int",
        KEYLAT,
        &[("event=1000010000", "event=1e9")],
        3,
        "seq/event not integers",
    );
}

#[test]
fn the_refusal_gone() {
    perturbed(
        "refusal-gone",
        KEYLAT,
        &[(REFUSAL, "")],
        3,
        "expected 1 landing-refused line, got 0",
    );
}

#[test]
fn the_refusal_doubled() {
    let two = format!("{REFUSAL}{REFUSAL}");
    perturbed(
        "refusal-doubled",
        KEYLAT,
        &[(REFUSAL, &two)],
        3,
        "expected 1 landing-refused line, got 2",
    );
}

#[test]
fn the_open_line_gone() {
    perturbed(
        "open-gone",
        KEYLAT,
        &[("keylat: 3 keys open\n", "")],
        3,
        "expected the one open-keys line, got []",
    );
}

#[test]
fn the_open_line_doubled() {
    perturbed(
        "open-doubled",
        KEYLAT,
        &[(
            "keylat: 3 keys open\n",
            "keylat: 3 keys open\nkeylat: 4 keys open\n",
        )],
        3,
        "got ['keylat: 3 keys open', 'keylat: 4 keys open']",
    );
}

#[test]
fn a_gate_refusal_gone() {
    perturbed(
        "gate-gone",
        KEYLAT,
        &[("keylat: refused zone\n", "")],
        3,
        "Counter({'occupied': 1})",
    );
}

#[test]
fn a_gate_refusal_more() {
    perturbed(
        "gate-more",
        KEYLAT,
        &[(
            "keylat: refused occupied\n",
            "keylat: refused occupied\nkeylat: refused occupied\n",
        )],
        3,
        "Counter({'occupied': 2, 'zone': 1})",
    );
}

#[test]
fn the_seq_mismatch_gone() {
    perturbed(
        "seq-agrees",
        KEYLAT,
        &[("seq=88 ", "seq=8 ")],
        3,
        "diag counts wrong",
    );
}

#[test]
fn the_event_before_its_post_gone() {
    perturbed(
        "event-after-post",
        KEYLAT,
        &[("event=1001999000", "event=1002000000")],
        3,
        "expected exclusions",
    );
}

#[test]
fn the_window_exclusion_gone() {
    perturbed(
        "event-in-window",
        KEYLAT,
        &[("event=1202500000", "event=1002500000")],
        3,
        "expected exclusions",
    );
}

#[test]
fn a_negative_leg() {
    let from = "seq=1 event=1000000000 handled=+300.0 written=+350.0 pred=- echo=+360.0 fed=+365.0 commit=+370.0 landed=+380.0 shown=+385.0";
    let to = from.replace("shown=+385.0", "shown=+375.0");
    perturbed("negative-leg", KEYLAT, &[(from, &to)], 3, "'negative show'");
}

#[test]
fn the_stamp_leg() {
    perturbed(
        "leg-stamp",
        POSTS,
        &[
            ("post 1 1000000000", "post 1 999999000"),
            ("post 2 1000010000", "post 2 1000009000"),
            ("post 3 1000500000", "post 3 1000499000"),
        ],
        3,
        "leg stamp: {'median': 1.0,",
    );
}

#[test]
fn the_deliver_leg() {
    three_rows(
        "leg-deliver",
        "handled=+300.0",
        "handled=+301.0",
        "leg deliver: {'median': 301.0,",
    );
}

#[test]
fn the_write_leg() {
    three_rows(
        "leg-write",
        "written=+350.0",
        "written=+351.0",
        "leg write: {'median': 51.0,",
    );
}

#[test]
fn the_child_leg() {
    three_rows(
        "leg-child",
        "echo=+360.0",
        "echo=+361.0",
        "leg child: {'median': 11.0,",
    );
}

#[test]
fn the_wake_leg() {
    three_rows(
        "leg-wake",
        "fed=+365.0",
        "fed=+366.0",
        "leg wake: {'median': 6.0,",
    );
}

#[test]
fn the_build_leg() {
    three_rows(
        "leg-build",
        "commit=+370.0",
        "commit=+371.0",
        "leg build: {'median': 6.0,",
    );
}

#[test]
fn the_gpu_leg() {
    three_rows(
        "leg-gpu",
        "landed=+380.0",
        "landed=+381.0",
        "leg gpu: {'median': 11.0,",
    );
}

#[test]
fn the_show_and_app_and_to_shown_legs() {
    three_rows(
        "leg-show",
        "shown=+385.0",
        "shown=+386.0",
        "leg show: {'median': 6.0,",
    );
    let text = fs::read_to_string(root().join("tests/keylat/golden/perturbed/leg-show.txt"))
        .expect("the golden");
    assert!(text.contains("leg app: {'median': 86.0,"));
    assert!(text.contains("leg to_shown: {'median': 386.0,"));
}

#[test]
fn the_vsync_and_total_legs() {
    three_rows(
        "leg-vsync",
        "shown=+385.0 vsync=+",
        "shown=+385.0 vsync=+9",
        "leg vsync: {'median':",
    );
    let text = fs::read_to_string(root().join("tests/keylat/golden/perturbed/leg-vsync.txt"))
        .expect("the golden");
    assert!(text.contains("leg total: {'median':"));
}

#[test]
fn the_late_bound_legs() {
    three_rows(
        "leg-vsync-hi",
        " vsync_hi=+",
        " vsync_hi=+9",
        "leg vsync_hi: {'median':",
    );
    let text = fs::read_to_string(root().join("tests/keylat/golden/perturbed/leg-vsync-hi.txt"))
        .expect("the golden");
    assert!(text.contains("leg total_hi: {'median':"));
}

#[test]
fn a_leg_loses_one_value() {
    let from = "seq=1 event=1000000000 handled=+300.0 written=+350.0 pred=- echo=+360.0 fed=+365.0 commit=+370.0 landed=+380.0";
    let to = from.replace("commit=+370.0", "commit=-");
    perturbed(
        "leg-n",
        KEYLAT,
        &[(from, &to)],
        3,
        "'n': 4} != median 5.0 n 5",
    );
}

#[test]
fn a_leg_loses_every_value() {
    let text = fs::read_to_string(root().join("tests/keylat").join(KEYLAT)).expect("a fixture");
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("keylat: seq="))
        .collect();
    let edits: Vec<(String, String)> = lines[..5]
        .iter()
        .map(|l| (l.to_string(), l.replace("shown=+385.0", "shown=-")))
        .collect();
    let edits: Vec<(&str, &str)> = edits
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    perturbed(
        "leg-none",
        KEYLAT,
        &edits,
        3,
        "leg show: None != median 5.0 n 5",
    );
}

#[test]
fn a_key_that_never_reached_vsync() {
    let from = "seq=1 event=1000000000 handled=+300.0 written=+350.0 pred=- echo=+360.0 fed=+365.0 commit=+370.0 landed=+380.0 shown=+385.0 vsync=+400.0";
    let to = from.replace("vsync=+400.0", "vsync=-");
    perturbed(
        "not-completed",
        KEYLAT,
        &[(from, &to)],
        3,
        "expected 5 completed, got 4",
    );
}

#[test]
fn two_keys_on_one_frame() {
    perturbed(
        "shared-frame",
        KEYLAT,
        &[(
            "seq=2 event=1000010000 handled=+300.0 written=+350.0 pred=- echo=+360.0 fed=+365.0 commit=+370.0",
            "seq=2 event=1000010000 handled=+300.0 written=+350.0 pred=- echo=+352.0 fed=+355.0 commit=+360.0",
        )],
        3,
        "expected 0 shared-frame keys, got 2",
    );
}

#[test]
fn a_second_refused_landing() {
    perturbed(
        "second-refused-landing",
        KEYLAT,
        &[(
            "seq=5 event=1001500000 handled=+300.0 written=+350.0 pred=- echo=+360.0 fed=+365.0 commit=+370.0 landed=+380.0",
            "seq=5 event=1001500000 handled=+300.0 written=+350.0 pred=- echo=+360.0 fed=+365.0 commit=+370.0 landed=-",
        )],
        3,
        "expected 1 refused-landing row, got 2",
    );
}

#[test]
fn the_refused_row_landed() {
    perturbed(
        "refused-row-landed",
        KEYLAT,
        &[(
            "commit=+370.0 landed=- shown",
            "commit=+370.0 landed=+380.0 shown",
        )],
        3,
        "expected 1 refused-landing row, got 0",
    );
}

#[test]
fn an_unknown_token_changes_nothing() {
    perturbed(
        "unknown-token",
        KEYLAT,
        &[(
            "predok=- extra=tolerated",
            "predok=- extra=tolerated more=+1.0",
        )],
        0,
        "SELFTEST OK",
    );
}

#[test]
fn line_endings_change_nothing() {
    perturbed("crlf", KEYLAT, &[("\n", "\r\n")], 0, "SELFTEST OK");
    perturbed("cr", POSTS, &[("\n", "\r")], 0, "SELFTEST OK");
}

#[test]
fn separators_change_nothing() {
    perturbed(
        "tabs",
        KEYLAT,
        &[
            ("keylat: seq=", "keylat:\tseq="),
            (" handled=", "\t handled="),
        ],
        0,
        "SELFTEST OK",
    );
}
