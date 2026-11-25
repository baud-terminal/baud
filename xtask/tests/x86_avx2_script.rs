//! **`scripts/x86-avx2.sh` decides as it says**, its functions sourced and
//! run on any host: the arm is live only with more 32-byte movemasks than
//! the base binary holds, and every check runs, a red one failing the run
//! without stopping the checks after it.

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace")
        .to_path_buf()
}

/// `script` run in bash after the file is sourced: whether it exited 0, and
/// what it printed on either stream.
fn sourced(script: &str) -> (bool, String) {
    let out = Command::new("bash")
        .current_dir(root())
        .arg("-c")
        .arg(format!("source scripts/x86-avx2.sh; {script}"))
        .output()
        .expect("bash");
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

/// **The arm is live only above the base binary's count**: the base holds
/// 32-byte movemasks of its own, so as many as the base, or none, is not
/// live. Sourced, the file runs nothing.
#[test]
fn the_arm_is_live_only_with_more_wide_movemasks_than_the_base() {
    let (ok, text) = sourced("true");
    assert!(ok && text.is_empty(), "sourcing runs nothing: {text}");
    assert!(sourced("live 20 16").0, "more than the base");
    assert!(!sourced("live 16 16").0, "as many as the base");
    assert!(!sourced("live 0 16").0, "none");
}

/// **Every check runs, and a red one fails the run**: with the second of
/// three checks red, the third still runs and the run fails naming the
/// second; a check stops at its own first failure, even with the caller's
/// `set -e` on; all green passes.
#[test]
fn every_check_runs_and_a_red_one_fails_the_run() {
    let checks = "a() { echo a-ran; }; \
                  b() { false; echo b-went-on; }; \
                  c() { echo c-ran; }";
    let (ok, text) = sourced(&format!("{checks}; set -e; run_checks a b c"));
    assert!(!ok, "a red check fails the run: {text}");
    assert!(
        text.contains("c-ran"),
        "the check after the red one ran: {text}"
    );
    assert!(!text.contains("b-went-on"), "the red check stopped: {text}");
    assert!(text.contains("FAILED: b"), "named: {text}");
    let (ok, text) = sourced(&format!("{checks}; run_checks a c"));
    assert!(ok, "all green: {text}");
}
