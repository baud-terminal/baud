//! `tools/scripts/bounded`, driven as a process over this test binary re-entered in a
//! role, `as_the_role`, at a bound of 1 GiB. Every hog stops itself at
//! [`HOG_CAP`] whatever the guard does, and is started in its own process group
//! with a kill of the group on a deadline.
//!
//! `BOUNDED_TEST_SCRIPT` names another copy of the script, for running this
//! binary against a perturbed one, which reads its bound from a `tools/budgets.toml`
//! one directory above it, as the script itself does.

#![cfg(unix)]
// Most of the driver is the watcher's, which is macOS's.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::fs;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

mod exec;

const ROLE: &str = "BOUNDED_TEST_ROLE";
const DIR: &str = "BOUNDED_TEST_DIR";
const CODE: &str = "BOUNDED_TEST_CODE";
const SIGNAL: &str = "BOUNDED_TEST_SIGNAL";
/// The bound `cargo_shaped` gives the script it starts.
const INNER_BOUND: &str = "BOUNDED_TEST_INNER_MIB";
const BOUND: &str = "BAUD_TEST_MEMORY_BOUND_MIB";

/// The exit codes, which `the_exit_codes_are_named_alike` holds to the script's
/// and the watcher's.
const EXIT_OVER: i32 = 113;
const EXIT_REFUSED: i32 = 114;
const EXIT_TIMED_OUT: i32 = 115;

const MIB: usize = 1 << 20;
/// Where a hog stops whatever the guard does.
const HOG_CAP: usize = 3 << 30;
const HOG_STEP: usize = 64 * MIB;
/// A step's pause: 320 MiB/s, so 1 GiB takes three seconds and one poll's
/// lateness is 80 MiB.
const HOG_PAUSE: Duration = Duration::from_millis(200);
/// A hog's exit at the cap, which no guarded test should see.
const REACHED_CAP: i32 = 42;

/// What `cargo_shaped` exits with when its test fails.
const PARENT_FAILED: i32 = 101;

/// What `handles_term` exits with.
const HANDLED: i32 = 57;

const SIGKILL: i32 = 9;
#[cfg(target_os = "macos")]
const SIGTERM: i32 = 15;

unsafe extern "C" {
    fn getpid() -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn raise(sig: i32) -> i32;
    fn setsid() -> i32;
    fn signal(sig: i32, handler: *const ()) -> *const ();
}

#[cfg(target_os = "macos")]
const SIGUSR1: i32 = 30;
#[cfg(not(target_os = "macos"))]
const SIGUSR1: i32 = 10;

/// **Not a check: this test binary run in a role.** Unset, it does nothing.
#[test]
fn as_the_role() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let code = match role.as_str() {
        "hog" => hog(false),
        "hog-repeated" => hog(true),
        "parent-of-hog" => parent_of(&["hog"], false, Linger::UntilItEnds),
        "session-hog" => parent_of(&["hog"], true, Linger::UntilItEnds),
        "leaves-a-hog" => parent_of(&["hog"], false, Linger::APollOrTwo),
        "parent-of-sleeper" => parent_of(&["sleeper"], true, Linger::UntilItEnds),
        "cargo-shaped" => cargo_shaped(),
        "leaves-a-sleeper" => parent_of(&["sleeper"], true, Linger::APollOrTwo),
        "sleeper" => sleeper(),
        "handles-term" => handles_term(),
        "traps-term" => traps_term(),
        "says-its-session" => says_its_session(),
        #[cfg(target_os = "macos")]
        "parent-of-setuid" => parent_of_setuid(),
        "printer" => printer(),
        "dies-by" => dies_by(),
        "mark" => mark(),
        "resident" => resident_not_footprint(),
        "small-and-held" => small(Duration::from_secs(1)),
        "small-and-quick" => small(Duration::ZERO),
        #[cfg(target_os = "linux")]
        "reserves" => reserves(),
        other => panic!("no role {other}"),
    };
    std::process::exit(code);
}

fn role_dir() -> PathBuf {
    PathBuf::from(std::env::var_os(DIR).expect("a role is given its directory"))
}

/// Write a file by rename, so a reader sees all of it, and a kill of this process
/// leaves the last whole value and not an empty file.
fn publish(name: &str, value: &str) {
    let dir = role_dir();
    let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
    fs::write(&tmp, value).expect("the value");
    fs::rename(&tmp, dir.join(name)).expect("the value");
}

fn publish_pid(name: &str) {
    publish(&format!("{name}.pid"), &std::process::id().to_string());
}

fn hog(repeated: bool) -> i32 {
    publish_pid("hog");
    let mut kept: Vec<Vec<u8>> = Vec::new();
    let mut touched = 0;
    while touched < HOG_CAP {
        let mut block = vec![0u8; HOG_STEP];
        if repeated {
            block.fill(0x5a);
        } else {
            for at in (0..HOG_STEP).step_by(4096) {
                block[at] = 1;
            }
        }
        kept.push(block);
        touched += HOG_STEP;
        publish("hog.touched", &touched.to_string());
        std::thread::sleep(HOG_PAUSE);
    }
    REACHED_CAP
}

enum Linger {
    UntilItEnds,
    /// Long enough for a poll to see the child, and then this ends and leaves it.
    APollOrTwo,
}

fn parent_of(role: &[&str], own_session: bool, linger: Linger) -> i32 {
    publish_pid("parent");
    let mut child = Command::new(std::env::current_exe().expect("this binary"));
    child
        .args(["--exact", "as_the_role", "--nocapture"])
        .env(ROLE, role[0])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if own_session {
        // SAFETY: `setsid` is async-signal-safe.
        unsafe {
            child.pre_exec(|| {
                setsid();
                Ok(())
            });
        }
    }
    let mut child = child.spawn().expect("the child");
    match linger {
        Linger::UntilItEnds => child
            .wait()
            .expect("the child ends")
            .code()
            .unwrap_or(REACHED_CAP + 1),
        Linger::APollOrTwo => {
            std::thread::sleep(Duration::from_millis(800));
            0
        }
    }
}

/// What cargo does with its runner: the script over a test binary, here a hog,
/// and a status of its own when that fails, for a driver to tell it from the
/// helper's. (`cargo test` passes the 113 on, and `cargo nextest run` says 100.)
fn cargo_shaped() -> i32 {
    publish_pid("parent");
    let mut inner = Command::new(the_script());
    inner
        .arg(std::env::current_exe().expect("this binary"))
        .args(["--exact", "as_the_role", "--nocapture"])
        .env(ROLE, "hog")
        .env(BOUND, std::env::var(INNER_BOUND).expect("the inner bound"))
        .stdout(Stdio::null());
    let mut inner = inner.spawn().expect("the inner script");
    publish("inner.pid", &inner.id().to_string());
    let status = inner.wait().expect("the inner script ends");
    if status.success() { 0 } else { PARENT_FAILED }
}

fn sleeper() -> i32 {
    publish_pid("sleeper");
    std::thread::sleep(Duration::from_secs(60));
    0
}

/// A program that acts on `SIGTERM`, as a graceful one does: it says so with a
/// code of its own, so its status shows it was left to.
fn handles_term() -> i32 {
    static TERMED: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn note(_: i32) {
        TERMED.store(1, Ordering::SeqCst);
    }
    const SIGTERM: i32 = 15;
    // SAFETY: the handler only stores to an atomic.
    unsafe { signal(SIGTERM, note as *const ()) };
    publish_pid("handler");
    let deadline = Instant::now() + Duration::from_secs(60);
    while TERMED.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    HANDLED
}

/// **A program that hears `SIGTERM` and goes on running**, as one still
/// finishing its work does: it notes the signal in `termed`, then counts in
/// `beats` every 10 ms until it is killed, so a reader sees whether it ran
/// on after the signal or was stopped. It ends itself after a minute.
fn traps_term() -> i32 {
    static TERMED: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn note(_: i32) {
        TERMED.store(1, Ordering::SeqCst);
    }
    const SIGTERM: i32 = 15;
    // SAFETY: the handler only stores to an atomic.
    unsafe { signal(SIGTERM, note as *const ()) };
    publish_pid("trapper");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut beats = 0usize;
    while Instant::now() < deadline {
        if TERMED.load(Ordering::SeqCst) != 0 {
            if beats == 0 {
                publish("termed", "TERM");
            }
            beats += 1;
            publish("beats", &beats.to_string());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    HANDLED
}
/// A child this binary cannot read and the guard cannot either, for longer than
/// the guard's limit on the program: `top` in logging mode, setuid root, drawing
/// nothing. 77 if it turned out readable, which would make the test vacuous.
#[cfg(target_os = "macos")]
fn parent_of_setuid() -> i32 {
    let mut top = Command::new("/usr/bin/top")
        .args(["-l", "2", "-s", "3", "-n", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("top");
    std::thread::sleep(Duration::from_millis(300));
    let mut info = libproc::RusageInfoV0::default();
    // SAFETY: a live struct of the flavour's shape.
    let rc = unsafe { libproc::proc_pid_rusage(top.id() as i32, 0, &raw mut info) };
    let code = top.wait().expect("top ends").code().unwrap_or(1);
    if rc == 0 { 77 } else { code }
}

fn says_its_session() -> i32 {
    unsafe extern "C" {
        fn getsid(pid: i32) -> i32;
    }
    // SAFETY: plain calls about this process.
    let (session, pid) = unsafe { (getsid(0), getpid()) };
    fs::write(role_dir().join("session"), format!("{session} {pid}")).expect("the session");
    0
}

const PRINTED_OUT: &[u8] = b"on stdout \0\xff\xfe\n no newline";
const PRINTED_ERR: &[u8] = b"on stderr \0\xff\xfe\n no newline";

fn printer() -> i32 {
    use std::io::Write;
    std::io::stdout().write_all(PRINTED_OUT).expect("stdout");
    std::io::stdout().flush().expect("stdout");
    std::io::stderr().write_all(PRINTED_ERR).expect("stderr");
    std::env::var(CODE)
        .expect("a code")
        .parse()
        .expect("a number")
}

fn dies_by() -> i32 {
    let signal: i32 = std::env::var(SIGNAL)
        .expect("a signal")
        .parse()
        .expect("a number");
    // SAFETY: a signal to this process; it is the point of the role.
    unsafe { raise(signal) };
    1
}

fn mark() -> i32 {
    fs::write(role_dir().join("ran"), b"ran").expect("the mark");
    0
}

/// `size` of memory touched, held for `hold`, for the peak log to see.
fn small(hold: Duration) -> i32 {
    let size = 128 * MIB;
    let mut block = vec![0u8; size];
    for at in (0..size).step_by(4096) {
        block[at] = 1;
    }
    std::thread::sleep(hold);
    std::hint::black_box(&block);
    0
}

#[cfg(target_os = "macos")]
mod libproc {
    #[repr(C)]
    #[derive(Default)]
    pub struct RusageInfoV0 {
        pub uuid: [u8; 16],
        pub times: [u64; 6],
        pub resident: u64,
        pub footprint: u64,
        pub start: u64,
        pub exit: u64,
    }
    unsafe extern "C" {
        pub fn proc_pid_rusage(pid: i32, flavor: i32, out: *mut RusageInfoV0) -> i32;
        pub fn proc_listchildpids(pid: i32, out: *mut i32, size: i32) -> i32;
    }
}

/// **A file mapped past the bound and read through**: clean file pages count in
/// resident size and not in footprint, so its resident size can pass the bound
/// while its footprint stays under it.
#[cfg(target_os = "macos")]
fn resident_not_footprint() -> i32 {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn mmap(at: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    }
    const PROT_READ: i32 = 1;
    const MAP_PRIVATE: i32 = 2;
    let dir = role_dir();
    let size = 1280 * MIB;
    let file = fs::File::create_new(dir.join("sparse")).expect("a file");
    file.set_len(size as u64).expect("a sparse file");
    let file = fs::File::open(dir.join("sparse")).expect("the file");
    // SAFETY: a new read-only private mapping of the whole file.
    let at = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ,
            MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };
    assert!(!at.is_null() && at as isize != -1, "the mapping");
    // SAFETY: every read is inside the mapping just made, and it is never unmapped.
    let pages = unsafe { std::slice::from_raw_parts(at, size) };
    let mut sum = 0u64;
    for at in (0..size).step_by(4096) {
        sum += u64::from(std::hint::black_box(pages[at]));
    }
    std::hint::black_box(sum);
    std::thread::sleep(Duration::from_millis(800));
    0
}

#[cfg(not(target_os = "macos"))]
fn resident_not_footprint() -> i32 {
    panic!("the footprint is macOS's")
}

/// A reservation of address space past the fallback's limit, with nothing
/// touched: exits 0 if the mapping was granted.
#[cfg(target_os = "linux")]
fn reserves() -> i32 {
    unsafe extern "C" {
        fn mmap(at: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    }
    const PROT_NONE: i32 = 0;
    const MAP_PRIVATE: i32 = 2;
    const MAP_ANONYMOUS: i32 = 0x20;
    // SAFETY: a new anonymous mapping, never touched.
    let at = unsafe {
        mmap(
            std::ptr::null_mut(),
            64usize << 30,
            PROT_NONE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    i32::from(at as isize == -1)
}

// ---------------------------------------------------------------------------
// The driver.

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("xtask is in the repository")
        .to_path_buf()
}

fn the_script() -> PathBuf {
    std::env::var_os("BOUNDED_TEST_SCRIPT")
        .map_or_else(|| repo().join("tools/scripts/bounded"), PathBuf::from)
}

/// One hog at a time in a process, as `.config/nextest.toml`'s test group has it
/// across processes, and a swap-heavy machine has no room for two.
fn one_hog_at_a_time() -> MutexGuard<'static, ()> {
    static HOGS: Mutex<()> = Mutex::new(());
    HOGS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "xtask-bounded-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("scratch");
        Self(dir.canonicalize().expect("scratch"))
    }

    fn pid(&self, name: &str, within: Duration) -> i32 {
        let path = self.0.join(format!("{name}.pid"));
        let deadline = Instant::now() + within;
        loop {
            if let Ok(text) = fs::read_to_string(&path) {
                return text.parse().expect("a pid");
            }
            assert!(Instant::now() < deadline, "{name} never started");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn touched(&self) -> usize {
        fs::read_to_string(self.0.join("hog.touched")).map_or(0, |t| t.parse().expect("a number"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A failed check may have left a role running; each ends itself anyway.
        if std::thread::panicking() {
            let entries = fs::read_dir(&self.0).into_iter().flatten().flatten();
            for entry in entries.filter(|e| e.file_name().to_string_lossy().ends_with(".pid")) {
                let text = fs::read_to_string(entry.path()).unwrap_or_default();
                if let Ok(pid) = text.parse::<i32>() {
                    // SAFETY: a signal to a role of this test.
                    unsafe { kill(pid, SIGKILL) };
                }
            }
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A started process in its own group. On its deadline, or if the test fails,
/// the group is killed.
struct Running {
    child: Child,
    dir: PathBuf,
}

struct Ended {
    status: ExitStatus,
    out: Vec<u8>,
    err: Vec<u8>,
}

impl Ended {
    fn err_text(&self) -> String {
        String::from_utf8_lossy(&self.err).into_owned()
    }
}

impl Running {
    fn start(mut command: Command, scratch: &Scratch) -> Self {
        let dir = scratch.0.clone();
        let out = fs::File::create(dir.join("stdout")).expect("stdout");
        let err = fs::File::create(dir.join("stderr")).expect("stderr");
        command
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .process_group(0);
        let child = command.spawn().expect("the script runs");
        Self { child, dir }
    }

    fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    fn end(mut self, within: Duration) -> Ended {
        let deadline = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().expect("the wait") {
                return Ended {
                    status,
                    out: fs::read(self.dir.join("stdout")).expect("stdout"),
                    err: fs::read(self.dir.join("stderr")).expect("stderr"),
                };
            }
            assert!(Instant::now() < deadline, "not ended in {within:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            // SAFETY: the group this test started, led by this child.
            unsafe { kill(-(self.child.id() as i32), SIGKILL) };
            let _ = self.child.wait();
        }
    }
}

const LIMIT: Duration = Duration::from_secs(60);

/// This binary, in `role`, as the arguments a harness gives its runner.
fn in_role(mut command: Command, role: &str, scratch: &Scratch) -> Command {
    command.arg(std::env::current_exe().expect("this binary"));
    with_role(command, role, scratch)
}

/// This binary, in `role`, with no runner.
fn bare_role(role: &str, scratch: &Scratch) -> Command {
    let this = std::env::current_exe().expect("this binary");
    with_role(Command::new(this), role, scratch)
}

fn with_role(mut command: Command, role: &str, scratch: &Scratch) -> Command {
    command
        .args(["--exact", "as_the_role", "--nocapture", "--format", "terse"])
        .env(ROLE, role)
        .env(DIR, &scratch.0);
    command
}

/// The script at a bound of 1 GiB.
fn runner() -> Command {
    let mut command = Command::new(the_script());
    command.env("BAUD_TEST_MEMORY_BOUND_MIB", "1024");
    command
}

fn helper(seconds: u32, mib: u32) -> Command {
    let mut command = Command::new(the_script());
    command
        .args(["--timeout", &seconds.to_string(), "--"])
        .env("BAUD_TEST_MEMORY_BOUND_MIB", mib.to_string());
    command
}

/// A copy of the script and its source in a tree of its own, under `repo/`, with
/// `budgets` for its `tools/budgets.toml`, or no file at all.
fn a_tree_with(scratch: &Scratch, budgets: Option<&str>) -> PathBuf {
    let dir = scratch.0.join("repo/tools/scripts");
    fs::create_dir_all(&dir).expect("the copy");
    let from = the_script();
    let script = dir.join("bounded");
    exec::copy(&from, &script);
    fs::copy(from.with_file_name("bounded.rs"), dir.join("bounded.rs")).expect("the source");
    if let Some(budgets) = budgets {
        write_budgets(&script, budgets);
    }
    script
}

fn write_budgets(script: &Path, budgets: &str) {
    let file = script
        .parent()
        .and_then(Path::parent)
        .expect("the tree")
        .join("budgets.toml");
    fs::write(file, budgets).expect("the file");
}

/// `tools/budgets.toml` holding one value, the only file in the way of it.
fn budget_of(value: &str) -> String {
    format!("[tests]\nmemory-max = \"{value}\"\n")
}

/// The refusal: the code, the file named, and the mark never written. With the
/// override set too, which replaces the value and not the file.
fn assert_refused_over_the_budget(script: &Path, scratch: &Scratch, case: &str) {
    for bound in [None, Some("1024")] {
        let mut command = Command::new(script);
        command.env_remove(BOUND);
        if let Some(bound) = bound {
            command.env(BOUND, bound);
        }
        let command = in_role(command, "mark", scratch);
        let ended = Running::start(command, scratch).end(LIMIT);
        let err = ended.err_text();
        assert_eq!(ended.status.code(), Some(EXIT_REFUSED), "{case}: {err}");
        assert!(err.contains("budgets.toml"), "{case}: {err}");
        assert!(err.contains("not run"), "{case}: {err}");
        assert!(!scratch.0.join("ran").exists(), "{case} ran the program");
    }
}

/// The control the refusals stand on: this tree, given a value it can read, runs
/// the program, so that the mark can appear.
fn assert_runs_given_a_good_file(script: &Path, scratch: &Scratch) {
    write_budgets(script, &budget_of("1G"));
    let mut command = Command::new(script);
    command.env_remove(BOUND);
    let command = in_role(command, "mark", scratch);
    let ended = Running::start(command, scratch).end(LIMIT);
    assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
    assert!(scratch.0.join("ran").exists(), "the control did not run");
}

fn is_gone(pid: i32) -> bool {
    // SAFETY: signal 0 only asks whether the process exists.
    unsafe { kill(pid, 0) != 0 }
}

/// Gone within a limit a loaded machine meets, the time in the message if not.
fn assert_gone(pid: i32, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !is_gone(pid) {
        assert!(
            Instant::now() < deadline,
            "{what}, pid {pid}, is still alive"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ---------------------------------------------------------------------------
// Everywhere the script runs.

#[test]
fn the_cargo_config_names_the_runner_and_the_script_is_executable() {
    use std::os::unix::fs::PermissionsExt;
    let config = fs::read_to_string(repo().join(".cargo/config.toml")).expect("the config");
    let table: toml::Table = config.parse().expect("toml");
    let runner = table["target"]["cfg(all())"]["runner"].as_str();
    assert_eq!(runner, Some("tools/scripts/bounded"));
    let mode = fs::metadata(repo().join("tools/scripts/bounded"))
        .expect("the script")
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "the script is executable");
    let nextest = fs::read_to_string(repo().join(".config/nextest.toml")).expect("nextest");
    let table: toml::Table = nextest.parse().expect("toml");
    let slow = &table["profile"]["default"]["slow-timeout"];
    assert!(
        slow["terminate-after"].as_integer().is_some(),
        "every test ends at a time bound"
    );
}

/// Exit codes and output through the runner are what they are with none.
#[test]
fn a_program_under_the_bound_runs_to_its_exit_code_byte_for_byte() {
    for code in [0, 3, 101] {
        let scratch = Scratch::new();
        let code_text = code.to_string();
        let mut bare = bare_role("printer", &scratch);
        bare.env(CODE, &code_text);
        let without = Running::start(bare, &scratch).end(LIMIT);
        let mut guarded = in_role(runner(), "printer", &scratch);
        guarded.env(CODE, &code_text);
        let with = Running::start(guarded, &scratch).end(LIMIT);
        assert_eq!(without.status.code(), Some(code), "the control's code");
        assert_eq!(with.status.code(), Some(code), "{}", with.err_text());
        assert_eq!(without.out, with.out, "stdout");
        assert_eq!(without.err, with.err, "stderr");
        assert!(
            with.out
                .windows(PRINTED_OUT.len())
                .any(|w| w == PRINTED_OUT),
            "the control: the bytes are there at all"
        );
    }
}

/// A status read by whoever started this says the signal the program died of.
#[test]
fn a_child_killed_by_a_signal_reads_as_that_signal() {
    for signal in [SIGUSR1, SIGKILL] {
        let scratch = Scratch::new();
        let mut guarded = in_role(runner(), "dies-by", &scratch);
        guarded.env(SIGNAL, signal.to_string());
        let ended = Running::start(guarded, &scratch).end(LIMIT);
        assert_eq!(ended.status.signal(), Some(signal), "{}", ended.err_text());
    }
}

#[test]
fn a_program_that_cannot_start_is_126_or_127() {
    let scratch = Scratch::new();
    let mut missing = runner();
    missing.arg(scratch.0.join("no-such-program"));
    let ended = Running::start(missing, &scratch).end(LIMIT);
    assert_eq!(ended.status.code(), Some(127), "{}", ended.err_text());

    let not_executable = scratch.0.join("not-executable");
    fs::write(&not_executable, b"data").expect("the file");
    let mut denied = runner();
    denied.arg(&not_executable);
    let ended = Running::start(denied, &scratch).end(LIMIT);
    assert_eq!(ended.status.code(), Some(126), "{}", ended.err_text());
}

#[test]
fn a_bad_bound_is_refused_and_the_program_is_not_run() {
    // The last two are numbers, past any machine: a way of switching this off.
    // Left unset, the default comes down to a small machine instead, which only
    // a machine that small can show.
    for bad in [
        "",
        "abc",
        "0",
        "-1",
        "1.5",
        "1 2",
        "0x10",
        "99999999",
        "99999999999",
    ] {
        let scratch = Scratch::new();
        let mut command = in_role(Command::new(the_script()), "mark", &scratch);
        command.env("BAUD_TEST_MEMORY_BOUND_MIB", bad);
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(
            ended.status.code(),
            Some(EXIT_REFUSED),
            "{bad:?}: {}",
            ended.err_text()
        );
        assert!(!scratch.0.join("ran").exists(), "{bad:?} ran the program");
        assert!(ended.err_text().contains("not run"), "{bad:?}");
    }
    // The control: a good bound does run it, so the mark can appear.
    let scratch = Scratch::new();
    let command = in_role(runner(), "mark", &scratch);
    let ended = Running::start(command, &scratch).end(LIMIT);
    assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
    assert!(scratch.0.join("ran").exists(), "the control did not run");
}

#[test]
fn a_missing_budget_file_is_refused_and_the_program_is_not_run() {
    let scratch = Scratch::new();
    let script = a_tree_with(&scratch, None);
    assert_refused_over_the_budget(&script, &scratch, "no file");
    assert_runs_given_a_good_file(&script, &scratch);
}

#[test]
fn a_missing_key_is_refused_and_the_program_is_not_run() {
    for body in [
        "",
        "[tests]\n",
        "# memory-max = \"16G\"\n[tests]\n# memory-max = \"16G\"\n",
        // Outside the section, which is as far as a key is read.
        "memory-max = \"16G\"\n[tests]\n",
        "[tests]\n[other]\nmemory-max = \"16G\"\n",
        "[tests.inner]\nmemory-max = \"16G\"\n",
        " [tests]\nmemory-max = \"16G\"\n",
        "[tests]\nmax = \"16G\"\n",
    ] {
        let scratch = Scratch::new();
        let script = a_tree_with(&scratch, Some(body));
        assert_refused_over_the_budget(&script, &scratch, body);
    }
    let scratch = Scratch::new();
    let script = a_tree_with(&scratch, Some(""));
    assert_runs_given_a_good_file(&script, &scratch);
}

#[test]
fn a_malformed_value_is_refused_and_the_program_is_not_run() {
    let forms = [
        // Not the line's form.
        "[tests]\nmemory-max = 16G\n",
        "[tests]\nmemory-max = \"16G\" # a comment\n",
        "[tests]\nmemory-max=\"16G\"\n",
        "[tests]\n memory-max = \"16G\"\n",
        "[tests]\nmemory-max = \"16G\"\r\n",
        // Not a size.
        "[tests]\nmemory-max = \"\"\n",
        "[tests]\nmemory-max = \"G\"\n",
        "[tests]\nmemory-max = \"16g\"\n",
        "[tests]\nmemory-max = \"16GB\"\n",
        "[tests]\nmemory-max = \"16 G\"\n",
        "[tests]\nmemory-max = \"1.5G\"\n",
        "[tests]\nmemory-max = \"-1G\"\n",
        "[tests]\nmemory-max = \"0x10\"\n",
        // Named twice, so which is meant is not known.
        "[tests]\nmemory-max = \"1G\"\nmemory-max = \"2G\"\n",
    ];
    // The shared line reads a size in these, and each is no bound: none, less
    // than a MiB or not a whole one, more than the override may say, and a
    // count that would wrap the shell's 64 bits to a small one.
    let unusable = [
        "0",
        "0G",
        "000T",
        "512K",
        "1000",
        "1048577",
        "1000000000M",
        "976563G",
        "954T",
        "17592186044416T",
        "9223372036854775808",
        "18446744073709551616",
        "18446744090889420800",
    ];
    for body in forms
        .iter()
        .map(|f| (*f).to_owned())
        .chain(unusable.iter().map(|v| budget_of(v)))
    {
        let scratch = Scratch::new();
        let script = a_tree_with(&scratch, Some(&body));
        assert_refused_over_the_budget(&script, &scratch, &body);
    }
    let scratch = Scratch::new();
    let script = a_tree_with(&scratch, Some(forms[0]));
    assert_runs_given_a_good_file(&script, &scratch);
}

#[test]
fn bad_helper_arguments_are_refused() {
    let shapes: [&[&str]; 5] = [
        &["--timeout"],
        &["--timeout", "x", "--", "true"],
        &["--timeout", "0", "--", "true"],
        &["--timeout", "1", "true"],
        &["--timeout", "1", "--"],
    ];
    for shape in shapes {
        let scratch = Scratch::new();
        let mut command = Command::new(the_script());
        command.args(shape);
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(EXIT_REFUSED), "{shape:?}");
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn the_memory_tests_are_macos_s() {
    eprintln!(
        "skipped off macOS, because each reads the watcher, and Linux has a kernel limit \
         instead: the hogs, the sentinel, the descriptor probe, the peak log, the helper's \
         tree kill, and a test that ends returning at once"
    );
}

/// The fallback: a reservation past the limit is refused, and one the limit's
/// headroom leaves room for is not what it counts. With a user `runs.slice` this
/// is the cgroup, which does not count reservations at all.
#[cfg(target_os = "linux")]
#[test]
fn the_fallback_limit_counts_reservations_and_the_cgroup_does_not() {
    let scratch = Scratch::new();
    let command = in_role(runner(), "reserves", &scratch);
    let ended = Running::start(command, &scratch).end(LIMIT);
    let has_slice = Command::new("systemctl")
        .args(["--user", "cat", "runs.slice"])
        .output()
        .is_ok_and(|o| o.status.success());
    let expected = i32::from(!has_slice);
    assert_eq!(ended.status.code(), Some(expected), "{}", ended.err_text());
}

/// The cgroup's kill: SIGKILL, as the script says, where there is a slice.
#[cfg(target_os = "linux")]
#[test]
fn over_the_bound_in_a_slice_is_a_kill() {
    let has_slice = Command::new("systemctl")
        .args(["--user", "cat", "runs.slice"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_slice {
        eprintln!("skipped: no user runs.slice here");
        return;
    }
    let _one = one_hog_at_a_time();
    let scratch = Scratch::new();
    let ended = Running::start(in_role(runner(), "hog", &scratch), &scratch).end(LIMIT);
    assert_eq!(ended.status.signal(), Some(SIGKILL), "{}", ended.err_text());
    assert!(scratch.touched() < HOG_CAP, "the hog reached its cap");
}

// ---------------------------------------------------------------------------
// The watcher, which is macOS's.

#[cfg(target_os = "macos")]
mod watcher {
    use super::*;

    /// Over the bound, the distinct code and the message, and a hog that never
    /// reached the cap. The footprint it names is near the bound: one poll's
    /// growth at this rate is 80 MiB, and this leaves room for a slow machine.
    fn assert_killed_near_the_bound(ended: &Ended, scratch: &Scratch) {
        let err = ended.err_text();
        assert_eq!(ended.status.code(), Some(EXIT_OVER), "{err}");
        assert!(err.contains("1024 MiB"), "{err}");
        assert!(err.contains("BAUD_TEST_MEMORY_BOUND_MIB"), "{err}");
        assert!(
            err.contains("as_the_role") || err.contains("bounded-"),
            "{err}"
        );
        let reached: f64 = err
            .split("reached ")
            .nth(1)
            .and_then(|r| r.split(" GiB").next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("no footprint in {err}"));
        assert!((1.0..2.0).contains(&reached), "reached {reached} GiB");
        let touched = scratch.touched();
        assert!(touched >= 512 * MIB, "only {touched} touched");
        assert!(touched < HOG_CAP, "the hog reached its cap");
    }

    #[test]
    fn a_hog_is_killed_near_the_bound() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        let ended = Running::start(in_role(runner(), "hog", &scratch), &scratch).end(LIMIT);
        assert_killed_near_the_bound(&ended, &scratch);
        assert_gone(scratch.pid("hog", LIMIT), "the hog");
    }

    #[test]
    fn a_hog_in_a_child_is_killed() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        let ended =
            Running::start(in_role(runner(), "parent-of-hog", &scratch), &scratch).end(LIMIT);
        assert_killed_near_the_bound(&ended, &scratch);
        assert_gone(scratch.pid("hog", LIMIT), "the child");
        assert_gone(scratch.pid("parent", LIMIT), "the parent");
    }

    /// A kill of the group would miss this one.
    #[test]
    fn a_hog_in_its_own_session_is_killed_by_pid() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        let ended = Running::start(in_role(runner(), "session-hog", &scratch), &scratch).end(LIMIT);
        assert_killed_near_the_bound(&ended, &scratch);
        assert_gone(scratch.pid("hog", LIMIT), "the child in its own session");
    }

    /// One value in every byte, the shape the compressor takes first. On an idle
    /// machine nothing compresses, so the difference between the two numbers is
    /// held by the next test.
    #[test]
    fn a_hog_of_one_repeated_value_is_seen() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        let ended =
            Running::start(in_role(runner(), "hog-repeated", &scratch), &scratch).end(LIMIT);
        assert_killed_near_the_bound(&ended, &scratch);
    }

    /// **A mapped file past the bound runs to its end.** Its resident size is
    /// the machine's to keep or evict, so this asserts only that the watcher
    /// let it finish; the rule is held on given sizes by the verdict tests.
    #[test]
    fn a_file_mapped_past_the_bound_is_not_killed() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        let ended = Running::start(in_role(runner(), "resident", &scratch), &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
    }

    /// The watcher's verdict on a tree of these sizes at this bound, in bytes.
    fn verdict(watcher: &Path, bound: u64, footprint: u64, resident: u64) -> String {
        let out = Command::new(watcher)
            .args([
                "--over".to_owned(),
                bound.to_string(),
                footprint.to_string(),
                resident.to_string(),
            ])
            .output()
            .expect("the watcher");
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    const GIB: u64 = 1 << 30;

    #[test]
    fn resident_size_past_the_bound_with_footprint_under_it_is_not_killed() {
        let watcher = the_watcher();
        for resident in [GIB + 1, 2 * GIB, 64 * GIB, u64::MAX] {
            assert_eq!(
                verdict(&watcher, GIB, GIB / 2, resident),
                "under",
                "resident {resident}"
            );
        }
        assert_eq!(verdict(&watcher, GIB, 0, 64 * GIB), "under");
    }

    #[test]
    fn footprint_past_the_bound_is_killed_whatever_resident_size_is() {
        let watcher = the_watcher();
        for footprint in [GIB + 1, 2 * GIB, u64::MAX] {
            for resident in [0, GIB / 2, GIB, footprint, 64 * GIB, u64::MAX] {
                assert_eq!(
                    verdict(&watcher, GIB, footprint, resident),
                    "over",
                    "footprint {footprint}, resident {resident}"
                );
            }
        }
    }

    /// Over is past the bound: a tree at it exactly runs on.
    #[test]
    fn footprint_at_the_bound_is_not_over_it() {
        let watcher = the_watcher();
        assert_eq!(verdict(&watcher, GIB, GIB, 64 * GIB), "under");
        assert_eq!(verdict(&watcher, GIB, GIB + 1, 0), "over");
    }

    /// The program's end returns the status, and what it left stays alive and
    /// bounded, which is the shape of a test binary that leaves a hog behind.
    #[test]
    fn a_hog_left_behind_is_bounded_after_the_program_ends() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        let ended =
            Running::start(in_role(runner(), "leaves-a-hog", &scratch), &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
        let hog = scratch.pid("hog", LIMIT);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !is_gone(hog) {
            assert!(Instant::now() < deadline, "the left hog is alive");
            std::thread::sleep(Duration::from_millis(20));
        }
        let touched = scratch.touched();
        assert!(touched < HOG_CAP, "the left hog reached its cap");
        assert!(touched >= 512 * MIB, "only {touched} touched");
    }

    /// `kill -9` of the watcher leaves nothing running, its descendants included.
    #[test]
    fn killing_the_watcher_kills_the_program_and_its_descendants() {
        let scratch = Scratch::new();
        let running = Running::start(in_role(runner(), "parent-of-sleeper", &scratch), &scratch);
        let parent = scratch.pid("parent", LIMIT);
        let sleeper = scratch.pid("sleeper", LIMIT);
        // Two polls, so the sentinel has been told of the descendant.
        std::thread::sleep(Duration::from_millis(600));
        // SAFETY: the watcher this test started.
        unsafe { kill(running.pid(), SIGKILL) };
        let ended = running.end(LIMIT);
        assert_eq!(ended.status.signal(), Some(SIGKILL));
        assert_gone(parent, "the program");
        assert_gone(sleeper, "its descendant in its own session");
    }

    fn children_of(pid: i32) -> Vec<i32> {
        let mut out = [0i32; 64];
        // SAFETY: a live buffer of the size given.
        let count =
            unsafe { libproc::proc_listchildpids(pid, out.as_mut_ptr(), size_of_val(&out) as i32) };
        out[..usize::try_from(count).unwrap_or(0)].to_vec()
    }

    /// `kill -9` of the sentinel: the program goes and the status says so.
    #[test]
    fn killing_the_sentinel_kills_the_program_and_fails() {
        let scratch = Scratch::new();
        let running = Running::start(in_role(runner(), "parent-of-sleeper", &scratch), &scratch);
        let parent = scratch.pid("parent", LIMIT);
        let sleeper = scratch.pid("sleeper", LIMIT);
        let others: Vec<i32> = children_of(running.pid())
            .into_iter()
            .filter(|p| *p != parent)
            .collect();
        assert_eq!(
            others.len(),
            1,
            "the watcher has one other child: {others:?}"
        );
        std::thread::sleep(Duration::from_millis(600));
        // SAFETY: the sentinel of the watcher this test started.
        unsafe { kill(others[0], SIGKILL) };
        let ended = running.end(LIMIT);
        assert_eq!(
            ended.status.code(),
            Some(EXIT_REFUSED),
            "{}",
            ended.err_text()
        );
        assert_gone(parent, "the program");
        assert_gone(sleeper, "its descendant");
    }

    /// A copy of the script and the source beside it, the source patched, built
    /// on first use in the copy's own `target/`.
    fn copy_of_the_script(scratch: &Scratch, anchor: &str, replacement: &str) -> PathBuf {
        let from = the_script();
        let budgets = fs::read_to_string(
            from.parent()
                .and_then(Path::parent)
                .expect("the tree")
                .join("budgets.toml"),
        )
        .expect("the file");
        let script = a_tree_with(scratch, Some(&budgets));
        let source = script.with_file_name("bounded.rs");
        let text = fs::read_to_string(&source).expect("the source");
        assert!(text.contains(anchor), "the anchor {anchor:?} is gone");
        fs::write(&source, text.replacen(anchor, replacement, 1)).expect("the source");
        script
    }

    /// What the script hands the watcher for `budgets`, seen by a stand-in for
    /// the watcher that prints it, where a real one would quietly bring 1T down
    /// to this machine: the MiB, or `None` for a refusal.
    fn handed_on(budgets: &str, bound: Option<&str>) -> Option<u64> {
        let scratch = Scratch::new();
        let script = a_tree_with(&scratch, Some(budgets));
        fs::remove_file(script.with_file_name("bounded.rs")).expect("the source");
        let stand_in = scratch.0.join("repo/target/bounded/bounded");
        fs::create_dir_all(stand_in.parent().expect("the directory")).expect("target");
        exec::script(&stand_in, "#!/bin/sh\nprintf %s \"$1\"\n");
        let mut command = Command::new(&script);
        command.env_remove(BOUND).arg("/usr/bin/true");
        if let Some(bound) = bound {
            command.env(BOUND, bound);
        }
        let ended = Running::start(command, &scratch).end(LIMIT);
        match ended.status.code() {
            Some(0) => Some(
                String::from_utf8(ended.out)
                    .expect("text")
                    .parse()
                    .expect("a number"),
            ),
            Some(EXIT_REFUSED) => None,
            other => panic!("{other:?}: {}", String::from_utf8_lossy(&ended.err)),
        }
    }

    #[test]
    fn each_size_means_what_it_says() {
        for (size, mib) in [
            ("16G", 16 * 1024),
            ("512M", 512),
            ("1T", 1024 * 1024),
            // No letter is a count of bytes, as MemoryMax= has it.
            ("17179869184", 16 * 1024),
            ("1048576", 1),
            ("2097152K", 2048),
            // Decimal with a leading zero, and not octal or an error.
            ("016G", 16 * 1024),
            ("08M", 8),
            // The largest the override may say, in each unit that can.
            ("999999999M", 999_999_999),
            ("976562G", 999_999_488),
            ("953T", 999_292_928),
        ] {
            assert_eq!(handed_on(&budget_of(size), None), Some(mib), "{size}");
        }
    }

    /// The other project's parser, over bodies that differ in which lines count:
    /// one line of it is the value this reads, and any other count is a refusal.
    #[test]
    fn this_reads_what_the_shared_line_reads() {
        let committed =
            fs::read_to_string(repo().join("tools/budgets.toml")).expect("the committed file");
        assert!(handed_on(&committed, None).is_some(), "the committed file");
        for body in [
            committed.as_str(),
            "[tests]\nmemory-max = \"2G\"",
            "# a comment\n\n[tests]\n# a comment\nother = 1\nmemory-max = \"2G\"\n\n[next]\n",
            "[before]\nmemory-max = \"1G\"\n[tests]\nmemory-max = \"2G\"\n",
            "[tests]\nmemory-max = \"2G\"\n[after]\nmemory-max = \"3G\"\n",
            "[tests]\n[tests]\nmemory-max = \"2G\"\n",
            "[tests]\n[tests]\n[tests]\nmemory-max = \"2G\"\n",
            "[tests]\nmemory-max = \"2G\"\n[tests]\nmemory-max = \"3G\"\n",
            "[tests]\nmemory-max = \"2G\"\nmemory-max = \"3G\"\n",
            "[tests]\nmemory-max = \"2G\" \n",
            "[other]\nmemory-max = \"2G\"\n",
        ] {
            let scratch = Scratch::new();
            let file = scratch.0.join("budgets.toml");
            fs::write(&file, body).expect("the file");
            let sed = Command::new("sed")
                .args([
                    "-n",
                    r#"/^\[tests\]$/,/^\[/s/^memory-max = "\([0-9][0-9]*[KMGT]\{0,1\}\)"$/\1/p"#,
                ])
                .arg(&file)
                .output()
                .expect("sed");
            let sed = String::from_utf8(sed.stdout).expect("text");
            let lines: Vec<&str> = sed.lines().collect();
            let expected = match lines[..] {
                [size] => {
                    let (digits, mib) = match size.split_at(size.len() - 1) {
                        (digits, "G") => (digits, 1024),
                        (digits, "T") => (digits, 1024 * 1024),
                        _ => panic!("only G and T here: {size}"),
                    };
                    Some(digits.parse::<u64>().expect("digits") * mib)
                }
                _ => None,
            };
            assert_eq!(
                handed_on(body, None),
                expected,
                "{body:?} against {lines:?}"
            );
        }
    }

    #[test]
    fn the_override_replaces_the_files_value() {
        assert_eq!(handed_on(&budget_of("1G"), Some("2048")), Some(2048));
        assert_eq!(handed_on(&budget_of("1G"), None), Some(1024));
        assert_eq!(handed_on(&budget_of("1G"), Some("abc")), None);
    }

    /// The whole way, the real watcher built in the copy: the file's 1G kills a
    /// hog at 1 GiB, with no variable set. The bound it replaced was 16 GiB, past
    /// the hog's cap.
    #[test]
    fn the_value_is_read_from_the_file() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        let script = a_tree_with(&scratch, Some(&budget_of("1G")));
        let mut command = Command::new(&script);
        command.env_remove(BOUND);
        let command = in_role(command, "hog", &scratch);
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_killed_near_the_bound(&ended, &scratch);
    }

    /// A file's value past the machine comes down to the machine, one file
    /// serving every machine, where the variable's is refused.
    #[test]
    fn a_files_value_past_the_machine_comes_down_to_it() {
        let scratch = Scratch::new();
        let script = a_tree_with(&scratch, Some(&budget_of("900T")));
        let mut command = Command::new(&script);
        command.env_remove(BOUND);
        let command = in_role(command, "mark", &scratch);
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
        assert!(scratch.0.join("ran").exists(), "the program did not run");
    }

    /// `tools/scripts/bounded --timeout` over a cargo-shaped parent that starts the
    /// script again, each at a bound of its own. The watcher, its sentinel and
    /// all below them are gone at the end, whichever fired.
    struct Nested {
        ended: Ended,
        scratch: Scratch,
    }

    fn nested(seconds: u32, outer_mib: u32, inner_mib: u32) -> Nested {
        let scratch = Scratch::new();
        let mut command = in_role(helper(seconds, outer_mib), "cargo-shaped", &scratch);
        command.env(INNER_BOUND, inner_mib.to_string());
        let running = Running::start(command, &scratch);
        let parent = scratch.pid("parent", LIMIT);
        let inner = scratch.pid("inner", LIMIT);
        let hog = scratch.pid("hog", LIMIT);
        let sentinels: Vec<i32> = children_of(inner)
            .into_iter()
            .filter(|p| *p != hog)
            .collect();
        assert_eq!(sentinels.len(), 1, "the inner watcher's other child");
        let ended = running.end(LIMIT);
        assert_gone(hog, "the hog");
        assert_gone(parent, "the cargo-shaped parent");
        assert_gone(inner, "the inner watcher");
        assert_gone(sentinels[0], "the inner sentinel");
        assert!(scratch.touched() < HOG_CAP, "the hog reached its cap");
        Nested { ended, scratch }
    }

    /// The outer fires, by a bound under the inner's: its own status, its own
    /// message, and the inner watcher no obstacle.
    #[test]
    fn nested_the_outer_bound_kills_the_inner_watcher_too() {
        let _one = one_hog_at_a_time();
        let Nested { ended, scratch } = nested(60, 1024, 4096);
        assert_killed_near_the_bound(&ended, &scratch);
        assert_eq!(ended.err_text().matches(" reached ").count(), 1);
    }

    /// The inner fires: the test fails, the parent says so, and the helper returns
    /// what the parent returned and not a code of its own.
    #[test]
    fn nested_the_inner_bound_is_the_parents_failure_and_not_the_helpers() {
        let _one = one_hog_at_a_time();
        let Nested { ended, .. } = nested(60, 4096, 1024);
        let err = ended.err_text();
        assert_eq!(ended.status.code(), Some(PARENT_FAILED), "{err}");
        assert!(err.contains("the bound of 1024 MiB"), "{err}");
        assert_eq!(err.matches(" reached ").count(), 1, "{err}");
    }

    /// One bound, as a harness that sets the variable has it on both: where the
    /// polls fall says which fires, and either is a kill near the bound.
    #[test]
    fn nested_at_one_bound_either_may_fire_and_all_ends() {
        let _one = one_hog_at_a_time();
        let Nested { ended, .. } = nested(60, 1024, 1024);
        let err = ended.err_text();
        assert!(
            matches!(ended.status.code(), Some(EXIT_OVER | PARENT_FAILED)),
            "{:?}: {err}",
            ended.status
        );
        assert!(err.contains("the bound of 1024 MiB"), "{err}");
    }

    /// The outer's clock over the inner's watcher.
    #[test]
    fn nested_the_timeout_kills_the_inner_watcher_too() {
        let _one = one_hog_at_a_time();
        let Nested { ended, .. } = nested(2, 4096, 4096);
        let err = ended.err_text();
        assert_eq!(ended.status.code(), Some(EXIT_TIMED_OUT), "{err}");
        assert!(err.contains("did not end in 2 s"), "{err}");
    }

    /// Both ways of the script over itself leave a program's status and bytes as
    /// one does.
    #[test]
    fn nested_a_program_keeps_its_status_and_bytes() {
        let scratch = Scratch::new();
        let mut command = helper(60, 1024);
        command.arg(the_script());
        let mut command = in_role(command, "printer", &scratch);
        command.env(CODE, "3");
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(3), "{}", ended.err_text());
        assert!(
            ended
                .out
                .windows(PRINTED_OUT.len())
                .any(|w| w == PRINTED_OUT)
        );
        assert_eq!(ended.err, PRINTED_ERR);
    }

    #[test]
    fn a_source_that_will_not_compile_is_refused() {
        let scratch = Scratch::new();
        let script = copy_of_the_script(&scratch, "#![no_main]", "#![no_main]\nnot rust at all");
        let command = in_role(Command::new(&script), "mark", &scratch);
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(
            ended.status.code(),
            Some(EXIT_REFUSED),
            "{}",
            ended.err_text()
        );
        assert!(!scratch.0.join("ran").exists(), "the program ran");
        assert!(ended.err_text().contains("could not be built"));
        assert!(ended.err_text().contains("not run"));
    }

    /// Two first starts at once, as nextest makes on a tree with no watcher
    /// yet: `rustc` writes its objects beside `-o`, so each build has a
    /// directory of its own or they delete each other's. A `rustc` first on
    /// `PATH` holds each until the other has started, so the overlap is not
    /// left to chance.
    #[test]
    fn two_first_starts_build_apart() {
        let scratch = Scratch::new();
        let script = copy_of_the_script(&scratch, "#![no_main]", "#![no_main]");
        let seen = scratch.0.join("seen");
        let shims = scratch.0.join("shims");
        fs::create_dir_all(&seen).expect("the record");
        fs::create_dir_all(&shims).expect("the shims");
        let path = std::env::var("PATH").expect("a PATH");
        let real = path
            .split(':')
            .map(|d| Path::new(d).join("rustc"))
            .find(|p| p.is_file())
            .expect("a rustc");
        let shim = shims.join("rustc");
        exec::script(
            &shim,
            &format!(
                "#!/bin/bash\n\
                 prev=; out=\n\
                 for a in \"$@\"; do [ \"$prev\" = -o ] && out=$a; prev=$a; done\n\
                 echo \"$out\" > {seen}/$$\n\
                 n=0\n\
                 while [ \"$(/bin/ls {seen} | /usr/bin/wc -l)\" -lt 2 ] && [ $n -lt 600 ]; do\n\
                 /bin/sleep 0.1; n=$((n+1))\n\
                 done\n\
                 exec {real} \"$@\"\n",
                seen = seen.display(),
                real = real.display(),
            ),
        );
        let path = format!("{}:{path}", shims.display());
        let start = |role: &str| {
            let mut command = Command::new(&script);
            command.env("PATH", &path);
            Running::start(in_role(command, role, &scratch), &scratch)
        };
        let (one, two) = (start("mark"), start("mark"));
        let (one, two) = (one.end(LIMIT), two.end(LIMIT));
        assert_eq!(one.status.code(), Some(0), "{}", one.err_text());
        assert_eq!(two.status.code(), Some(0), "{}", two.err_text());
        let parents: Vec<PathBuf> = fs::read_dir(&seen)
            .expect("the record")
            .map(|e| {
                let text = fs::read_to_string(e.expect("an entry").path()).expect("a path");
                Path::new(text.trim())
                    .parent()
                    .expect("a directory")
                    .to_path_buf()
            })
            .collect();
        assert_eq!(parents.len(), 2, "{parents:?}");
        assert_ne!(parents[0], parents[1], "one directory for both builds");
    }

    /// No `rustc`, and no `env` to find a `bash` with, so bash is named here.
    #[test]
    fn no_rustc_is_refused() {
        let scratch = Scratch::new();
        let script = copy_of_the_script(&scratch, "#![no_main]", "#![no_main]");
        let mut command = Command::new("/bin/bash");
        command.arg(&script).env("PATH", "/nonexistent");
        let command = in_role(command, "mark", &scratch);
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(
            ended.status.code(),
            Some(EXIT_REFUSED),
            "{}",
            ended.err_text()
        );
        assert!(!scratch.0.join("ran").exists(), "the program ran");
        assert!(
            ended.err_text().contains("could not be built"),
            "{}",
            ended.err_text()
        );
    }

    #[test]
    fn a_footprint_that_cannot_be_read_kills_and_fails() {
        let scratch = Scratch::new();
        // Three reads work, the program's first among them, and no read after.
        let script = copy_of_the_script(
            &scratch,
            "let mut info = RusageInfoV0::default();",
            "{
                static READS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                if READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 2 {
                    return Seen::Unreadable;
                }
            }
            let mut info = RusageInfoV0::default();",
        );
        let command = in_role(Command::new(&script), "sleeper", &scratch);
        let running = Running::start(command, &scratch);
        let sleeper = scratch.pid("sleeper", LIMIT);
        let ended = running.end(LIMIT);
        assert_eq!(
            ended.status.code(),
            Some(EXIT_REFUSED),
            "{}",
            ended.err_text()
        );
        assert!(ended.err_text().contains("could not be read"));
        assert_gone(sleeper, "the program");
    }

    /// A descendant that cannot be read is not the program: it is left, as a
    /// `sudo` under a shell under `cargo run` must be.
    #[test]
    fn a_setuid_descendant_is_left_alone() {
        let scratch = Scratch::new();
        let command = in_role(runner(), "parent-of-setuid", &scratch);
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
    }

    /// **A test that ends returns at once**, so a poll is never waited out: a
    /// comparison with a program that sleeps one poll with no runner at all, on
    /// the same machine at the same load, and never an absolute.
    #[test]
    fn a_test_that_ends_returns_well_inside_one_poll() {
        let scratch = Scratch::new();
        let timed = |make: &dyn Fn() -> Command| {
            (0..7)
                .map(|_| {
                    let begun = Instant::now();
                    let ended = Running::start(make(), &scratch).end(LIMIT);
                    assert_eq!(ended.status.code(), Some(0));
                    begun.elapsed()
                })
                .min()
                .expect("seven runs")
        };
        let quick = timed(&|| {
            let mut command = runner();
            command.arg("/usr/bin/true");
            command
        });
        let one_poll = timed(&|| {
            let mut command = Command::new("/bin/sleep");
            command.arg("0.25");
            command
        });
        assert!(
            quick * 2 < one_poll,
            "{quick:?} under the runner against {one_poll:?} for one poll's sleep"
        );
    }

    /// Everything a process inherits, one fact a line, argv, descriptors, signal
    /// dispositions and mask, group, session, umask, cwd, nice, limits and the
    /// environment sorted. C, because Rust's start-up would ignore `SIGPIPE`
    /// itself and hide a runner that had left it ignored.
    const INHERITED: &str = r#"
#include <dirent.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <unistd.h>
extern char **environ;
static int by_string(const void *a, const void *b) {
    return strcmp(*(char *const *)a, *(char *const *)b);
}
int main(int argc, char **argv) {
    for (int i = 0; i < argc; i++) printf("arg %s\n", argv[i]);
    for (int fd = 0; fd < 256; fd++) {
        int flags = fcntl(fd, F_GETFD);
        if (flags < 0) continue;
        struct stat st;
        fstat(fd, &st);
        printf("fd %d flags %d mode %o dev %d ino %llu\n", fd, flags, st.st_mode, st.st_dev,
               (unsigned long long)st.st_ino);
    }
    for (int sig = 1; sig < NSIG; sig++) {
        struct sigaction action;
        if (sigaction(sig, NULL, &action) == 0)
            printf("signal %d %s\n", sig, action.sa_handler == SIG_IGN ? "ignored"
                                        : action.sa_handler == SIG_DFL ? "default" : "caught");
    }
    sigset_t mask;
    sigprocmask(SIG_BLOCK, NULL, &mask);
    for (int sig = 1; sig < NSIG; sig++)
        if (sigismember(&mask, sig)) printf("blocked %d\n", sig);
    printf("group %d session %d\n", getpgrp(), getsid(0));
    mode_t mode = umask(0);
    printf("umask %o\n", mode);
    char cwd[4096];
    printf("cwd %s\n", getcwd(cwd, sizeof cwd));
    printf("nice %d\n", getpriority(PRIO_PROCESS, 0));
    for (int r = 0; r < RLIM_NLIMITS; r++) {
        struct rlimit limit;
        if (getrlimit(r, &limit) == 0)
            printf("limit %d %llu %llu\n", r, (unsigned long long)limit.rlim_cur,
                   (unsigned long long)limit.rlim_max);
    }
    int count = 0;
    while (environ[count]) count++;
    qsort(environ, count, sizeof(char *), by_string);
    for (int i = 0; i < count; i++) printf("env %s\n", environ[i]);
    return 0;
}
"#;

    fn compile_c(source: &str, scratch: &Scratch, name: &str) -> PathBuf {
        let src = scratch.0.join(format!("{name}.c"));
        let bin = scratch.0.join(name);
        fs::write(&src, source).expect("the source");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let out = Command::new(&cc)
            .arg("-o")
            .arg(&bin)
            .arg(&src)
            .output()
            .unwrap_or_else(|e| panic!("{cc} did not run: {e}"));
        assert!(
            out.status.success(),
            "{name}.c did not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        bin
    }

    unsafe extern "C" {
        fn dup2(from: i32, to: i32) -> i32;
        fn sigprocmask(how: i32, set: *const u32, old: *mut u32) -> i32;
        fn umask(mask: u16) -> u16;
    }

    /// What the probe printed, started by `command` with a harness's inheritance:
    /// a descriptor beyond the standard three, a signal ignored, a signal blocked
    /// and a umask, or none of those.
    fn inherited(mut command: Command, scratch: &Scratch, unusual: bool) -> Vec<u8> {
        use std::os::fd::IntoRawFd;
        let path = scratch.0.join("inherited");
        let out = fs::File::options()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .expect("the output");
        let extra = fs::File::open(&path).expect("a descriptor").into_raw_fd();
        command
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(Stdio::null())
            .current_dir(&scratch.0);
        if unusual {
            // SAFETY: each call is async-signal-safe.
            unsafe {
                command.pre_exec(move || {
                    const SIGHUP: i32 = 1;
                    const SIGPIPE: i32 = 13;
                    let sig_ign = std::ptr::without_provenance(1);
                    const SIG_BLOCK: i32 = 1;
                    let sigusr2: u32 = 1 << 30;
                    dup2(extra, 9);
                    signal(SIGHUP, sig_ign);
                    signal(SIGPIPE, sig_ign);
                    sigprocmask(SIG_BLOCK, &raw const sigusr2, std::ptr::null_mut());
                    umask(0o027);
                    Ok(())
                });
            }
        }
        let status = command.status().expect("the probe runs");
        assert_eq!(status.code(), Some(0), "the probe's run: {status:?}");
        // SAFETY: the descriptor made above, which nothing else holds.
        drop(unsafe { <fs::File as std::os::fd::FromRawFd>::from_raw_fd(extra) });
        fs::read(&path).expect("the output")
    }

    /// `PATH`, and the four variables bash rewrites, present and stale or absent.
    fn the_environment(command: &mut Command, present: bool) {
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").expect("PATH"));
        command.env("A_VARIABLE", "with spaces, and = in it");
        // Names a script might use for its own variables.
        for name in [
            "src", "dir", "bin", "tmp", "key", "value", "unit", "status", "here",
        ] {
            command.env(name, "the harness's");
        }
        command.env("BAUD_TEST_MEMORY_BOUND_MIB", "1024");
        if present {
            command
                .env("_", "/what/the/parent/ran")
                .env("PWD", "/a/stale/directory")
                .env("OLDPWD", "/an/old/one with a space")
                .env("SHLVL", "7");
        }
    }

    #[test]
    fn what_the_program_inherits_is_what_it_would_with_no_runner() {
        let scratch = Scratch::new();
        let probe = compile_c(INHERITED, &scratch, "inherited-probe");
        for unusual in [false, true] {
            for present in [true, false] {
                let mut bare = Command::new(&probe);
                the_environment(&mut bare, present);
                let without = inherited(bare, &scratch, unusual);

                let mut guarded = Command::new(the_script());
                guarded.arg(&probe);
                the_environment(&mut guarded, present);
                let with = inherited(guarded, &scratch, unusual);
                assert_eq!(
                    String::from_utf8_lossy(&without),
                    String::from_utf8_lossy(&with),
                    "unusual {unusual}, variables present {present}"
                );
            }
        }
        // The control: this probe tells the inheritances apart.
        let mut usual = Command::new(&probe);
        the_environment(&mut usual, true);
        let mut other = Command::new(&probe);
        the_environment(&mut other, true);
        assert_ne!(
            inherited(usual, &scratch, false),
            inherited(other, &scratch, true),
            "the probe cannot see an inherited descriptor, signal or umask"
        );
    }

    /// `--timeout` over a parent standing for cargo, and under it a test binary
    /// in its own session that a kill of the parent alone would orphan. Neither
    /// is left.
    #[test]
    fn the_timeout_kills_a_cargo_shaped_parent_and_its_orphanable_child() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        // Under the bound for all two seconds, so that the clock is what ends it.
        let running = Running::start(in_role(helper(2, 4096), "session-hog", &scratch), &scratch);
        let hog = scratch.pid("hog", LIMIT);
        let parent = scratch.pid("parent", LIMIT);
        let ended = running.end(LIMIT);
        assert_eq!(
            ended.status.code(),
            Some(EXIT_TIMED_OUT),
            "{}",
            ended.err_text()
        );
        assert!(ended.err_text().contains("did not end in 2 s"));
        assert_gone(parent, "the cargo-shaped parent");
        assert_gone(hog, "the orphanable child");
        assert!(scratch.touched() < HOG_CAP, "the hog reached its cap");
    }

    /// A bound over a tree under `--timeout`, as a harness would set one, and a
    /// parent that ended leaving its child: nothing is left either way.
    #[test]
    fn the_helper_bounds_the_tree_and_leaves_nothing_when_the_command_ends() {
        let _one = one_hog_at_a_time();
        let scratch = Scratch::new();
        let over = in_role(helper(60, 1024), "session-hog", &scratch);
        let ended = Running::start(over, &scratch).end(LIMIT);
        assert_killed_near_the_bound(&ended, &scratch);
        assert_gone(scratch.pid("hog", LIMIT), "the hog");

        let scratch = Scratch::new();
        let left = in_role(helper(60, 1024), "leaves-a-sleeper", &scratch);
        let running = Running::start(left, &scratch);
        let sleeper = scratch.pid("sleeper", LIMIT);
        let ended = running.end(LIMIT);
        assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
        assert_gone(sleeper, "what the command left");
    }

    /// **A remembered pid's start time is checked before a kill**, so a reused
    /// pid is not hit. A copy of the watcher remembers every descendant's start
    /// wrong, which is what a reused pid looks like from here; the descendant
    /// survives the kill, and without the check would not.
    #[test]
    fn a_pid_with_another_start_time_is_not_killed() {
        let scratch = Scratch::new();
        let script = copy_of_the_script(
            &scratch,
            "Seen::Alive { start, .. } => self.remember(child, start, false),",
            "Seen::Alive { start, .. } => self.remember(child, start + 1, false),",
        );
        let mut command = Command::new(&script);
        command
            .args(["--timeout", "2", "--"])
            .env("BAUD_TEST_MEMORY_BOUND_MIB", "1024");
        let running = Running::start(in_role(command, "parent-of-sleeper", &scratch), &scratch);
        let parent = scratch.pid("parent", LIMIT);
        let sleeper = scratch.pid("sleeper", LIMIT);
        let ended = running.end(LIMIT);
        assert_eq!(
            ended.status.code(),
            Some(EXIT_TIMED_OUT),
            "{}",
            ended.err_text()
        );
        assert_gone(parent, "the program, whose start was remembered right");
        let spared = !is_gone(sleeper);
        // SAFETY: this test's own descendant, which would otherwise sleep on.
        unsafe { kill(sleeper, SIGKILL) };
        assert!(spared, "a pid with another start time was killed");
    }

    /// A signal to the group reaches the program with nothing relayed; a signal
    /// to this process alone reaches it after [`RELAY_AFTER`] in the watcher. Each
    /// reads as the signal.
    #[test]
    fn signals_reach_the_program_from_the_group_and_from_the_watcher() {
        let scratch = Scratch::new();
        let running = Running::start(in_role(runner(), "sleeper", &scratch), &scratch);
        scratch.pid("sleeper", LIMIT);
        // SAFETY: the group this test started.
        unsafe { kill(-running.pid(), SIGTERM) };
        let ended = running.end(LIMIT);
        assert_eq!(ended.status.signal(), Some(SIGTERM), "{}", ended.err_text());

        let scratch = Scratch::new();
        let running = Running::start(in_role(runner(), "sleeper", &scratch), &scratch);
        scratch.pid("sleeper", LIMIT);
        // SAFETY: the watcher this test started.
        unsafe { kill(running.pid(), SIGTERM) };
        let ended = running.end(LIMIT);
        assert_eq!(ended.status.signal(), Some(SIGTERM), "{}", ended.err_text());
    }

    /// A program that acts on the signal gets to: the watcher does not die of
    /// the group's signal and have the sentinel kill it.
    #[test]
    fn a_program_that_handles_the_groups_signal_is_left_to() {
        let scratch = Scratch::new();
        let running = Running::start(in_role(runner(), "handles-term", &scratch), &scratch);
        scratch.pid("handler", LIMIT);
        // SAFETY: the group this test started.
        unsafe { kill(-running.pid(), SIGTERM) };
        let ended = running.end(LIMIT);
        assert_eq!(ended.status.code(), Some(HANDLED), "{}", ended.err_text());
    }

    /// **The guard's kill asks first and never stops**: on the helper's
    /// timeout, a program that traps `SIGTERM` hears it, runs on through the
    /// grace, counting, and is killed after it. Named breaks: the old kill,
    /// `SIGSTOP` then `SIGKILL`, and the program never hears `SIGTERM`;
    /// `SIGTERM` then `SIGSTOP`, and it counts no beat after the signal.
    #[test]
    fn a_timeout_asks_with_sigterm_and_kills_after_the_grace() {
        let scratch = Scratch::new();
        let running = Running::start(in_role(helper(1, 1024), "traps-term", &scratch), &scratch);
        let trapper = scratch.pid("trapper", LIMIT);
        let ended = running.end(LIMIT);
        assert_eq!(
            ended.status.code(),
            Some(EXIT_TIMED_OUT),
            "{}",
            ended.err_text()
        );
        let termed = fs::read_to_string(scratch.0.join("termed"));
        assert_eq!(
            termed.ok().as_deref(),
            Some("TERM"),
            "the program was never sent SIGTERM"
        );
        // 10 ms apart through a grace of seconds: three is a program that
        // ran on after the signal, and a stopped one counts at most one.
        let beats: usize =
            fs::read_to_string(scratch.0.join("beats")).map_or(0, |b| b.parse().expect("a count"));
        assert!(
            beats >= 3,
            "the program counted {beats} beats after SIGTERM: it was stopped or killed at once"
        );
        assert_gone(trapper, "the program that traps SIGTERM");
    }

    /// **No process the guard watches is ever sent `SIGSTOP`.** A stopped
    /// Metal client holds the GPU work it was handing over until the kill,
    /// so the guard's kill is `SIGTERM`, a grace, then `SIGKILL`, and a stop
    /// is nowhere in the source it builds from. Named break: the old
    /// `kill(pid, SIGSTOP)` in `kill_all`.
    #[test]
    fn the_guard_never_stops_a_process() {
        let source = fs::read_to_string(the_script().with_file_name("bounded.rs"))
            .expect("the watcher's source");
        let code: Vec<&str> = source
            .lines()
            .map(|l| l.find("//").map_or(l, |at| &l[..at]))
            .collect();
        let code = code.join("\n");
        assert!(
            code.contains("kill("),
            "the control: the source sends signals"
        );
        assert!(!code.contains("SIGSTOP"), "the watcher names SIGSTOP");
    }

    /// The helper's command is a session's leader.
    #[test]
    fn the_helper_starts_the_command_in_a_new_session() {
        let scratch = Scratch::new();
        let command = in_role(helper(60, 1024), "says-its-session", &scratch);
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
        let said = fs::read_to_string(scratch.0.join("session")).expect("the session");
        let (session, pid) = said.split_once(' ').expect("two numbers");
        assert_eq!(session, pid, "the command leads its session");
    }

    /// **The sentinel holds nothing a harness waits on.** It outlives this run,
    /// bounding what was left, and a harness reading the program's output must
    /// still see end of file when the program ends.
    #[test]
    fn output_ends_when_the_program_does_though_the_sentinel_lives_on() {
        use std::io::Read;
        let scratch = Scratch::new();
        let mut command = in_role(runner(), "leaves-a-sleeper", &scratch);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("the script runs");
        let mut out = child.stdout.take().expect("the pipe");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = out.read_to_end(&mut bytes);
            let _ = sender.send(());
        });
        let sleeper = scratch.pid("sleeper", LIMIT);
        let eof = receiver.recv_timeout(Duration::from_secs(30));
        let status = child.wait().expect("the wait");
        // SAFETY: this test's own descendant, which would otherwise sleep on.
        unsafe { kill(sleeper, SIGKILL) };
        assert!(eof.is_ok(), "output stayed open past the program's end");
        assert_eq!(status.code(), Some(0));
    }

    /// **The peak log.** A run longer than a poll has a polled peak; one shorter
    /// is seen by `ru_maxrss` at the reap. The log is one line each, and nothing
    /// when unnamed, and a log that cannot be written fails nothing.
    #[test]
    fn the_peak_log_has_each_run_and_cannot_fail_one() {
        let scratch = Scratch::new();
        let log = scratch.0.join("peaks");
        for (role, polled) in [("small-and-held", true), ("small-and-quick", false)] {
            let mut command = in_role(runner(), role, &scratch);
            command.env("BAUD_TEST_MEMORY_LOG", &log);
            let ended = Running::start(command, &scratch).end(LIMIT);
            assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
            let text = fs::read_to_string(&log).expect("the log");
            let line = text.lines().last().expect("a line");
            let field = |name: &str| -> u64 {
                line.split('\t')
                    .find_map(|f| f.strip_prefix(name))
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| panic!("no {name} in {line}"))
            };
            assert!(line.starts_with(std::env::current_exe().unwrap().to_str().unwrap()));
            let peak = field("peak_footprint=");
            let maxrss = field("maxrss=");
            assert!(maxrss >= 100 << 20, "{line}");
            assert_eq!(peak >= 100 << 20, polled, "{line}");
        }
        assert_eq!(
            fs::read_to_string(&log).expect("the log").lines().count(),
            2
        );

        let mut unwritable = in_role(runner(), "mark", &scratch);
        unwritable.env(
            "BAUD_TEST_MEMORY_LOG",
            scratch.0.join("no/such/directory/peaks"),
        );
        let ended = Running::start(unwritable, &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
        assert!(
            !ended.err_text().contains("bounded:"),
            "{}",
            ended.err_text()
        );

        let unnamed = scratch.0.join("unnamed");
        let mut none = in_role(runner(), "mark", &scratch);
        none.env_remove("BAUD_TEST_MEMORY_LOG")
            .current_dir(&scratch.0);
        let ended = Running::start(none, &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(0));
        assert!(!unnamed.exists(), "a log appeared with none named");
    }

    /// The watcher's built copy, built by running it once.
    fn the_watcher() -> PathBuf {
        let scratch = Scratch::new();
        let mut command = runner();
        command.arg("/usr/bin/true");
        let ended = Running::start(command, &scratch).end(LIMIT);
        assert_eq!(ended.status.code(), Some(0), "{}", ended.err_text());
        the_script()
            .parent()
            .expect("the script's directory")
            .join("../../target/bounded/bounded")
    }

    /// **The layouts and numbers `tools/scripts/bounded.rs` declares, held to this
    /// machine's headers.** It prints them from what it compiled.
    const LAYOUTS: &str = r#"
#include <errno.h>
#include <fcntl.h>
#include <libproc.h>
#include <signal.h>
#include <stddef.h>
#include <stdio.h>
#include <sys/event.h>
#include <sys/proc_info.h>
#include <sys/resource.h>
#include <sys/time.h>
#include <sys/wait.h>
#define AT(name, type, field) printf("%s=%zu\n", name, offsetof(type, field))
#define SIZE(name, type) printf("%s=%zu\n", name, sizeof(type))
#define NUMBER(name) printf("%s=%lld\n", #name, (long long)(name))
int main(void) {
    SIZE("rusage_info_v0.size", struct rusage_info_v0);
    AT("rusage_info_v0.resident", struct rusage_info_v0, ri_resident_size);
    AT("rusage_info_v0.footprint", struct rusage_info_v0, ri_phys_footprint);
    AT("rusage_info_v0.start", struct rusage_info_v0, ri_proc_start_abstime);
    AT("rusage_info_v0.exit", struct rusage_info_v0, ri_proc_exit_abstime);
    SIZE("kevent.size", struct kevent);
    AT("kevent.ident", struct kevent, ident);
    AT("kevent.filter", struct kevent, filter);
    AT("kevent.flags", struct kevent, flags);
    AT("kevent.fflags", struct kevent, fflags);
    AT("kevent.data", struct kevent, data);
    AT("kevent.udata", struct kevent, udata);
    SIZE("timespec.size", struct timespec);
    AT("timespec.nsec", struct timespec, tv_nsec);
    SIZE("rusage.size", struct rusage);
    AT("rusage.maxrss", struct rusage, ru_maxrss);
    SIZE("proc_fdinfo.size", struct proc_fdinfo);
    AT("proc_fdinfo.fd", struct proc_fdinfo, proc_fd);
    SIZE("rlimit.size", struct rlimit);
    SIZE("sigset.size", sigset_t);
    NUMBER(RUSAGE_INFO_V0);
    NUMBER(PROC_PIDLISTFDS);
    NUMBER(EVFILT_PROC);
    NUMBER(EVFILT_SIGNAL);
    NUMBER(EV_ADD);
    printf("NOTE_EXIT=%lld\n", (long long)(unsigned int)NOTE_EXIT);
    NUMBER(F_SETFL);
    NUMBER(F_DUPFD_CLOEXEC);
    NUMBER(O_NONBLOCK);
    NUMBER(O_WRONLY);
    NUMBER(O_APPEND);
    NUMBER(O_CREAT);
    NUMBER(O_CLOEXEC);
    NUMBER(ENOENT);
    NUMBER(ESRCH);
    NUMBER(EINTR);
    NUMBER(WNOHANG);
    NUMBER(SIG_UNBLOCK);
    NUMBER(RLIMIT_CORE);
    NUMBER(SIGHUP);
    NUMBER(SIGINT);
    NUMBER(SIGQUIT);
    NUMBER(SIGKILL);
    NUMBER(SIGPIPE);
    NUMBER(SIGALRM);
    NUMBER(SIGTERM);
    NUMBER(SIGCHLD);
    NUMBER(SIGXCPU);
    NUMBER(SIGXFSZ);
    NUMBER(SIGVTALRM);
    NUMBER(SIGPROF);
    NUMBER(SIGUSR1);
    NUMBER(SIGUSR2);
    return 0;
}
"#;

    #[test]
    fn the_layouts_are_the_platform_headers_own() {
        let scratch = Scratch::new();
        let probe = compile_c(LAYOUTS, &scratch, "layouts");
        let headers = Command::new(probe).output().expect("the probe");
        let watcher = Command::new(the_watcher())
            .arg("--layouts")
            .output()
            .expect("the watcher");
        let headers = String::from_utf8_lossy(&headers.stdout).into_owned();
        let watcher = String::from_utf8_lossy(&watcher.stdout).into_owned();
        let declared: Vec<&str> = watcher
            .lines()
            .filter(|l| !l.starts_with("exit."))
            .collect();
        let found: Vec<&str> = headers.lines().collect();
        assert_eq!(declared, found);
        assert!(found.len() > 50, "the control: {} lines", found.len());
    }

    #[test]
    fn the_exit_codes_are_named_alike() {
        let script = fs::read_to_string(the_script()).expect("the script");
        let watcher = Command::new(the_watcher())
            .arg("--layouts")
            .output()
            .expect("the watcher");
        let watcher = String::from_utf8_lossy(&watcher.stdout).into_owned();
        for (name, code) in [
            ("over", EXIT_OVER),
            ("refused", EXIT_REFUSED),
            ("timed_out", EXIT_TIMED_OUT),
        ] {
            assert!(
                script.contains(&format!("__bounded_exit_{name}={code}")),
                "the script's {name}"
            );
            assert!(
                watcher.lines().any(|l| l == format!("exit.{name}={code}")),
                "the watcher's {name}"
            );
        }
    }
}
