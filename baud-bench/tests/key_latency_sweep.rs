//! A whole sweep, the driver's every process and table, against stand-ins
//! for the app, the helper, `pmset` and `osascript` in `tests/keylat/stub`:
//! no window opens, no key is posted and nothing is raised. Each scenario's
//! stdout, raw rows and exit code are the Python driver's, written to
//! `tests/keylat/golden/` over the same stand-ins before it was removed. What
//! the app and the helper really do is the part a headless box cannot check.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const STUB: &str = "tests/keylat/stub";

/// The arguments for [`as_the_driver`], a unit separator between each.
const AS_DRIVER: &str = "KEYLAT_TEST_AS_DRIVER";

/// What libtest prints before a test that ends the process itself.
const LIBTEST_HEADER: &str = "\nrunning 1 test\n";

/// Not a check: this test binary run as the driver, because the driver is an
/// example and a test can name no example's path. With no arguments asked
/// for it does nothing.
#[test]
fn as_the_driver() {
    if let Ok(args) = std::env::var(AS_DRIVER) {
        let args: Vec<String> = args
            .split('\u{1f}')
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .collect();
        std::process::exit(baud_bench::key_latency::run(&args));
    }
}

/// The driver as a process.
fn driver<S: AsRef<str>>(args: &[S]) -> Command {
    let joined: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let mut cmd = Command::new(std::env::current_exe().expect("this test binary"));
    cmd.args([
        "--exact",
        "as_the_driver",
        "--nocapture",
        "--format",
        "terse",
    ])
    .env(AS_DRIVER, joined.join("\u{1f}"));
    cmd
}

/// What the driver wrote, without libtest's header.
fn stdout_of(out: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&out.stdout);
    text.strip_prefix(LIBTEST_HEADER)
        .unwrap_or_else(|| panic!("libtest's header: {text:?}"))
        .to_string()
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("the repository's root")
}

/// `<needle><digits>` becomes `<needle><to>`: the digits are a pid.
fn scrub(text: &str, needle: &str, to: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(needle) {
        let after = at + needle.len();
        out.push_str(&rest[..after]);
        out.push_str(to);
        rest = rest[after..].trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

/// Run the scenario and compare it with the golden. `saw_osascript` says
/// whether the driver got far enough to raise the app.
fn scenario(name: &str, mode: &str, power: &str, args: &[String], saw_osascript: &str) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("keylat-sweep")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    let state = dir.join("state");
    fs::create_dir_all(&state).expect("a scratch directory");
    let path = format!(
        "{}:{}",
        root().join(STUB).display(),
        std::env::var("PATH").expect("a PATH")
    );
    let scratch = dir.join("scratch");
    let scratch_text = scratch.display().to_string();
    let every: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .chain(["--scratch", &scratch_text])
        .collect();
    let out = driver(&every)
        .current_dir(root())
        .env("PATH", path)
        .env("BAUD_SWIFT", format!("bash {STUB}/helper"))
        .env("KEYLAT_STUB_STATE", &state)
        .env("KEYLAT_STUB_MODE", mode)
        .env("KEYLAT_STUB_POWER", power)
        .output()
        .expect("the bin runs");
    let mut text = stdout_of(&out).replace(&dir.display().to_string(), "<dir>");
    text = scrub(&text, "pid=", "PID");
    text = scrub(&text, "seed ", "PID");
    text = scrub(&text, "key-post.swift ", "PID");
    let mut tsvs: Vec<PathBuf> = fs::read_dir(&scratch)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    tsvs.sort();
    for tsv in tsvs {
        let file = tsv.file_name().expect("a file name").to_string_lossy();
        let body = fs::read_to_string(&tsv).expect("the rows");
        text += &format!("--- {file}\n{body}");
    }
    text += &format!("--- exit {}\n", out.status.code().expect("an exit code"));
    let golden = root().join(format!("tests/keylat/golden/{name}.txt"));
    let want = fs::read_to_string(&golden).expect("the golden");
    assert_eq!(text, want, "{name}");
    let log = fs::read_to_string(state.join("osascript.log")).unwrap_or_default();
    assert!(log.contains(saw_osascript), "{name}: {log:?}");
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

fn arm(name: &str, file: &str) -> [String; 2] {
    ["--arm".to_string(), format!("{name}={STUB}/{file}")]
}

const RAISED: &str = "set frontmost of (first process whose unix id is";

#[test]
fn two_arms_and_an_aa_over_two_rounds() {
    let mut a = arm("base", "app").to_vec();
    a.extend(arm("new", "app-new"));
    a.extend(args(&["--aa", "--rounds", "2", "--keys", "12"]));
    scenario("ab", "sweep", "ac", &a, RAISED);
}

#[test]
fn the_helper_refusing_exits_2_and_keeps_what_it_posted() {
    let mut a = arm("base", "app").to_vec();
    a.extend(args(&["--rounds", "1", "--keys", "3"]));
    scenario("refused", "refused", "ac", &a, RAISED);
}

#[test]
fn the_helper_failing_exits_4_and_stops_the_sweep() {
    let mut a = arm("base", "app").to_vec();
    a.extend(args(&["--rounds", "1", "--keys", "3"]));
    scenario("failed", "failed", "ac", &a, RAISED);
}

#[test]
fn keys_that_reach_nothing_exit_1_on_battery_with_a_warm_up_used() {
    let mut a = arm("base", "app").to_vec();
    a.extend(args(&[
        "--rounds", "1", "--keys", "3", "--child", "none", "--gap", "60",
    ]));
    scenario("silent", "silent", "battery", &a, RAISED);
}

#[test]
fn a_refused_landing_makes_the_run_not_quotable() {
    let mut a = arm("base", "app").to_vec();
    a.extend(args(&["--rounds", "2", "--keys", "8", "--child", "mute"]));
    scenario("landings", "landings", "ac", &a, RAISED);
}

#[test]
fn an_app_that_never_presents_exits_1_and_raises_nothing() {
    let mut a = arm("base", "app").to_vec();
    a.extend(args(&["--rounds", "2", "--keys", "3", "--child", "shell"]));
    scenario("never", "never", "ac", &a, "");
}

#[test]
fn two_arms_of_one_binary_are_warned_about() {
    let mut a = arm("a", "app").to_vec();
    a.extend(arm("b", "app"));
    a.extend(args(&["--rounds", "1", "--keys", "8", "--gap", "0"]));
    scenario("same", "cpu", "ac", &a, RAISED);
}

#[test]
fn the_stream_child_writes_each_line_s_number_and_end() {
    use std::io::Read;
    let mut child = driver(&["--stream-child"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the driver runs");
    let mut out = child.stdout.take().expect("a pipe");
    let mut got = Vec::new();
    let mut buf = [0u8; 64];
    while got.iter().filter(|b| **b == b'\n').count() < 14 {
        let n = out.read(&mut buf).expect("the child writes");
        assert!(n > 0, "the child ended");
        got.extend_from_slice(&buf[..n]);
    }
    drop(out);
    // With its reader gone the next write fails, and that ends it.
    let status = child.wait().expect("it ends");
    assert_eq!(status.code(), Some(1));
    let text = String::from_utf8_lossy(&got);
    let want = format!("{LIBTEST_HEADER}0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n");
    assert!(text.starts_with(&want), "{text}");
}

/// A datagram socket keeps each write(2) apart, where a pipe may join them.
#[test]
fn the_stream_child_writes_each_number_and_each_end_alone() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixDatagram;
    let (ours, theirs) = UnixDatagram::pair().expect("a socket pair");
    let mut child = driver(&["--stream-child"])
        .stdout(OwnedFd::from(theirs))
        .spawn()
        .expect("the driver runs");
    // Should the child end, a read fails rather than waits.
    ours.set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .expect("a timeout");
    let mut buf = [0u8; 64];
    let writes: Vec<String> = (0..16)
        .map(|_| {
            let n = ours.recv(&mut buf).expect("a write");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        })
        .collect();
    child.kill().expect("it stops");
    child.wait().expect("it ends");
    // libtest's header comes first, in writes of its own.
    let at = writes.iter().position(|w| w == "0").unwrap_or(writes.len());
    assert_eq!(
        writes[at..].get(..6),
        Some(&["0", "\n", "1", "\n", "2", "\n"].map(String::from)[..]),
        "{writes:?}"
    );
}

#[test]
fn the_stream_child_s_command_quotes_a_path_with_a_space() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("keylat a'b");
    fs::create_dir_all(&dir).expect("a scratch directory");
    let exe = dir.join("says-its-flag");
    fs::write(&exe, "#!/bin/sh\necho \"$1\"\n").expect("a script");
    let chmod = Command::new("chmod").arg("+x").arg(&exe).status();
    assert!(chmod.is_ok_and(|s| s.success()));
    let cmd = baud_bench::key_latency::sweep::child_env(
        baud_bench::key_latency::cli::Child::Stream,
        &exe.display().to_string(),
    )
    .expect("the stream child sets a command");
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .expect("sh runs");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "--stream-child\n");
}

#[test]
fn value_checks_exit_1_before_anything_runs() {
    for (a, line) in [
        (
            &["--rounds", "0"][..],
            "--rounds and --keys must be >= 1, --gap >= 0\n",
        ),
        (
            &["--keys", "0"],
            "--rounds and --keys must be >= 1, --gap >= 0\n",
        ),
        (
            &["--gap", "-1"],
            "--rounds and --keys must be >= 1, --gap >= 0\n",
        ),
        (
            &["--arm", "noequals"],
            "--arm wants name=path, got 'noequals'\n",
        ),
        (
            &["--arm", "a=b", "--arm", "it's"],
            "--arm wants name=path, got \"it's\"\n",
        ),
    ] {
        let out = driver(a).output().expect("the driver runs");
        assert_eq!(out.status.code(), Some(1), "{a:?}");
        assert_eq!(stdout_of(&out), line, "{a:?}");
    }
}

#[test]
fn usage_errors_exit_2_and_print_nothing_on_stdout() {
    let out = driver(&["--bogus"]).output().expect("the driver runs");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout_of(&out), "");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unrecognized arguments: --bogus"), "{err}");
}

#[test]
fn help_exits_0_on_stdout() {
    let out = driver(&["--help"]).output().expect("the driver runs");
    assert_eq!(out.status.code(), Some(0));
    let text = stdout_of(&out);
    assert!(
        text.starts_with("usage: key-latency [-h] [--selftest]"),
        "{text}"
    );
}

#[test]
fn the_tree_s_self_test_prints_the_python_s_bytes() {
    let out = driver(&["--selftest"])
        .env("BAUD_KEYLAT_FIXTURES", "tests/keylat")
        .current_dir(root())
        .output()
        .expect("the driver runs");
    assert_eq!(out.status.code(), Some(0));
    let want =
        fs::read_to_string(root().join("tests/keylat/golden/selftest.txt")).expect("the golden");
    assert_eq!(stdout_of(&out), want);
}

#[test]
fn the_default_fixtures_are_the_committed_ones() {
    let out = driver(&["--selftest"])
        .env_remove("BAUD_KEYLAT_FIXTURES")
        .current_dir(std::env::temp_dir())
        .output()
        .expect("the driver runs");
    assert_eq!(out.status.code(), Some(0));
    let text = stdout_of(&out);
    assert!(
        text.contains("baud-bench/../tests/keylat/keylat-fixture.txt"),
        "{text}"
    );
}

#[test]
fn a_missing_fixture_exits_1_with_nothing_on_stdout() {
    let out = driver(&["--selftest"])
        .env("BAUD_KEYLAT_FIXTURES", "/nonexistent/keylat")
        .output()
        .expect("the driver runs");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_of(&out), "");
}
