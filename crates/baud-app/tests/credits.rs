//! **`baud --credits` and `baud --version`, run as a user runs them**: with
//! an empty home and no config named, each prints exactly its text -- the
//! notices file; the version line, the copyright and licence lines and the
//! pointer to `--credits` -- exits 0, and leaves the home as empty as it
//! found it: no config written, nothing read into being, no window. The
//! same on macOS and Linux.
//!
//! A probe build (the test-only `effect-probe` feature) answers neither:
//! it runs only as a rig, and `main`'s door refuses every other call
//! before it reads the arguments for anything else (`rig_door.rs`). Under
//! the probe these tests hold that refusal instead -- exit 2, the door's
//! reason on stderr, nothing on stdout, the home left empty.
//!
//! The binary runs only after its source says `main` answers both before
//! it starts anything, so a red run of this test is a wrong answer and
//! never an app opening.

#[allow(dead_code)]
#[path = "../src/version.rs"]
mod version;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether `main` does nothing that starts the app before it answers
/// `asks` (`credits::asks(` or `version::asks(`), read from the source as
/// the modules' own tests read it.
fn answers_first(asks: &str) -> bool {
    let src = std::fs::read_to_string(crate_dir().join("src/main.rs")).expect("main.rs");
    let Some(body) = src.find(concat!("fn ", "main() {")).map(|at| &src[at..]) else {
        return false;
    };
    let Some(asked) = body.find(asks) else {
        return false;
    };
    [
        concat!("host::", "startup_begin()"),
        concat!("log::", "init()"),
        concat!("discover::", "Catalogue"),
        concat!("app::", "run()"),
        concat!("linux::", "run("),
    ]
    .iter()
    .all(|later| body.find(later).is_some_and(|at| asked < at))
}

/// How one run exited, what it printed on stdout and stderr, and what it
/// left in the home.
struct Ran {
    status: ExitStatus,
    out: Vec<u8>,
    err: String,
    left: Vec<OsString>,
}

/// **`baud <arg>` with an empty home** and no config named, once it has
/// exited.
fn in_empty_home(arg: &str) -> Ran {
    let home = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("credits-home{arg}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("an empty home");
    let mut child = Command::new(env!("CARGO_BIN_EXE_baud"))
        .arg(arg)
        .env("HOME", &home)
        .env_remove("BAUD_CONFIG")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_STATE_HOME")
        .env_remove("XDG_CACHE_HOME")
        .current_dir(&home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("baud starts");
    // The notices are 40 KB, more than a pipe holds: read while it writes.
    let mut stdout = child.stdout.take().expect("its stdout");
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut stdout, &mut out).map(|_| out)
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(s) = child.try_wait().expect("its status") {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("baud {arg} did not exit within 30 s");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let out = reader.join().expect("the reader").expect("its stdout");
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        std::io::Read::read_to_string(&mut e, &mut err).expect("its stderr");
    }
    let left = std::fs::read_dir(&home)
        .expect("the home")
        .map(|e| e.expect("an entry").file_name())
        .collect();
    let _ = std::fs::remove_dir_all(&home);
    Ran {
        status,
        out,
        err,
        left,
    }
}

/// **A plain build answered `arg`**: it exited 0 and wrote nothing into
/// the home.
#[cfg(not(feature = "effect-probe"))]
fn answered(ran: &Ran, arg: &str) {
    assert!(
        ran.status.success(),
        "baud {arg}: {}: {}",
        ran.status,
        ran.err
    );
    assert!(
        ran.left.is_empty(),
        "it wrote into the home: {:?}",
        ran.left
    );
}

/// **A probe build's door refused `arg`**: exit 2, the door's reason and
/// nothing else on stderr, nothing on stdout, nothing in the home. On
/// macOS the reason is the one `RigCall::parse` gives for `arg`, read from
/// it rather than copied; off macOS a probe build has no rig at all.
#[cfg(feature = "effect-probe")]
fn refused(ran: &Ran, arg: &str) {
    #[cfg(target_os = "macos")]
    let why = baud_present_macos::rig_call::RigCall::parse(&[arg.to_owned()])
        .expect_err("a lone flag is no rig's call")
        .0;
    #[cfg(not(target_os = "macos"))]
    let why = "a probe build runs only as macOS's rig";
    assert_eq!(
        ran.status.code(),
        Some(2),
        "baud {arg}: {}: {}",
        ran.status,
        ran.err
    );
    assert_eq!(ran.err, format!("baud: {why}\n"), "the door's reason");
    assert!(
        ran.out.is_empty(),
        "and prints nothing: {} bytes",
        ran.out.len()
    );
    assert!(
        ran.left.is_empty(),
        "it wrote into the home: {:?}",
        ran.left
    );
}

/// **It prints the file, exits 0 and writes nothing.** Red under: a newline
/// printed after the file (`println!`), which the byte comparison fails,
/// and a probe written into `HOME`, which the emptiness check fails. Under
/// the probe, the door refuses it; red under a door that admits it.
#[test]
fn credits_prints_the_file_and_touches_nothing() {
    assert!(
        answers_first(concat!("credits::", "asks(")),
        "main must answer --credits before it starts anything; not run, as running it would \
         start the app"
    );
    let ran = in_empty_home("--credits");
    #[cfg(feature = "effect-probe")]
    refused(&ran, "--credits");
    #[cfg(not(feature = "effect-probe"))]
    printed_the_file(&ran);
}

#[cfg(not(feature = "effect-probe"))]
fn printed_the_file(ran: &Ran) {
    answered(ran, "--credits");
    let file = std::fs::read(crate_dir().join("assets/CREDITS.txt")).expect("the notices");
    assert!(
        ran.out == file,
        "it prints the file exactly: {} bytes against {}",
        ran.out.len(),
        file.len()
    );
    assert_eq!(ran.err, "", "and says nothing else");
}

/// **`baud --version` prints its first line as before, then the About
/// window's copyright notice and licence, then where the third-party
/// notices are, and writes nothing.** The first line is `baud <build>`, as
/// scripts read it; the rest is `version::text`, the same constants, and
/// none of the notices themselves. Red under: `main` printing the first
/// line alone, as it did. Under the probe, the door refuses it.
#[test]
fn version_prints_its_line_then_the_notices_and_touches_nothing() {
    assert!(
        answers_first(concat!("version::", "asks(")),
        "main must answer --version before it starts anything; not run, as running it would \
         start the app"
    );
    let ran = in_empty_home("--version");
    #[cfg(feature = "effect-probe")]
    refused(&ran, "--version");
    #[cfg(not(feature = "effect-probe"))]
    printed_the_version(ran);
}

#[cfg(not(feature = "effect-probe"))]
fn printed_the_version(ran: Ran) {
    answered(&ran, "--version");
    let out = String::from_utf8(ran.out).expect("UTF-8");
    let lines: Vec<&str> = out.lines().collect();
    let first = version::line(version::BUILD);
    let (copyright, licence) = version::notice();
    assert_eq!(
        lines,
        [first.as_str(), copyright, licence, version::CREDITS_POINTER]
    );
    assert_eq!(out, version::text(version::BUILD));
    assert!(!out.contains("SIL OPEN FONT"), "no notices themselves");
    assert_eq!(ran.err, "", "and says nothing else");
}
