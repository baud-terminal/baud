//! The programs that measure the app from outside it: they launch a binary
//! they are handed, read what it prints, and report. Each is an example, so
//! a bare `cargo bench` or `cargo test` starts none of them:
//!
//! | module | example | what it reports |
//! |---|---|---|
//! | [`startup`] | `startup-bench` | the wall from spawn to a stage, per arm |
//! | [`idle_cpu`] | `idle-cpu` | the CPU a binary spends with nothing to draw |
//! | [`shader_cache`] | `shader-cache-bench` | a launch with the Metal caches and without |
//! | [`dock_check`] | `dock-launch-check` | the locale a launchd launch gives the shell |
//! | [`vt_reply`] | `vt-reply` | a terminal's reply to a cursor report, in hex |
//!
//! Everything that would make a report differ between two runs comes in
//! through [`Host`], so `baud-bench/tests/host.rs` holds each report to its
//! exact bytes over stand-in programs and a clock that ticks.

pub mod dock_check;
pub mod idle_cpu;
pub mod py;
pub mod shader_cache;
pub mod startup;
pub mod vt_reply;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// This process's arguments, as text.
pub fn py_args() -> Vec<String> {
    std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// A process's environment, in order.
pub type Env = [(OsString, OsString)];

/// `env[key]`, as text.
pub fn var(env: &Env, key: &str) -> Option<String> {
    env.iter()
        .find(|(k, _)| k == OsStr::new(key))
        .map(|(_, v)| v.to_string_lossy().into_owned())
}

/// The world a report depends on: the clocks, the sleep, a process's CPU,
/// the cache directory and an interrupt.
pub trait Host {
    /// A monotonic clock, in seconds.
    fn now(&mut self) -> f64;
    /// The clock `idle-cpu` and `vt-reply` read, in seconds.
    fn wall(&mut self) -> f64;
    /// Sleeps, and stops early for an interrupt.
    fn pause(&mut self, seconds: f64);
    /// The CPU the live process `pid` has used, in seconds, or `None` when
    /// it is gone.
    fn cpu_seconds(&mut self, pid: u32) -> Option<f64>;
    /// `$(getconf DARWIN_USER_CACHE_DIR)`, which holds the Metal caches.
    fn cache_root(&mut self) -> Result<String, String>;
    /// Whether an interrupt has arrived since [`catch_interrupt`].
    fn interrupted(&mut self) -> bool;
}

/// This machine's clocks and kernel.
pub struct Live {
    start: Instant,
}

impl Live {
    /// The clock starts here.
    pub fn new() -> Self {
        Live {
            start: Instant::now(),
        }
    }
}

impl Default for Live {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for Live {
    fn now(&mut self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn wall(&mut self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    // `thread::sleep` goes back to sleep after `EINTR`, so this wakes to look.
    fn pause(&mut self, seconds: f64) {
        let end = Instant::now() + Duration::from_secs_f64(seconds);
        while !INTERRUPTED.load(Ordering::Relaxed) {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            std::thread::sleep(left.min(Duration::from_millis(50)));
        }
    }

    fn cpu_seconds(&mut self, pid: u32) -> Option<f64> {
        idle_cpu::cpu_seconds(pid)
    }

    fn cache_root(&mut self) -> Result<String, String> {
        let out = Command::new("getconf")
            .arg("DARWIN_USER_CACHE_DIR")
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| format!("getconf: {e}"))?;
        if !out.status.success() {
            return Err(format!("getconf: {}", out.status));
        }
        let text = String::from_utf8(out.stdout).map_err(|e| format!("getconf: {e}"))?;
        Ok(py::strip(&text).to_string())
    }

    fn interrupted(&mut self) -> bool {
        INTERRUPTED.load(Ordering::Relaxed)
    }
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn note_interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

/// **An interrupt ends a read or a [`Live`] sleep rather than the process**,
/// for a program with something to stop or put back first: without
/// `SA_RESTART` the blocked `read` returns `EINTR` and [`Host::interrupted`]
/// says why. `SIGTERM` and `SIGHUP`,
/// which `timeout`, `kill` and a closed terminal send, count as one.
///
/// **And none of the three blocked** on the calling thread, whose mask the
/// threads and programs it starts after take: a handler never runs for a
/// signal held blocked, and cargo-watch hands its command `SIGHUP` blocked.
pub fn catch_interrupt() {
    // SAFETY: an all-zero `sigaction` and `sigset_t` are valid values of
    // their types, the handler only stores to an atomic, and every pointer
    // is live for its call.
    unsafe {
        let mut action: libc::sigaction = core::mem::zeroed();
        let handler: extern "C" fn(libc::c_int) = note_interrupt;
        action.sa_sigaction = handler as libc::sighandler_t;
        libc::sigemptyset(&raw mut action.sa_mask);
        let mut caught: libc::sigset_t = core::mem::zeroed();
        libc::sigemptyset(&raw mut caught);
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::sigaction(signal, &raw const action, core::ptr::null_mut());
            libc::sigaddset(&raw mut caught, signal);
        }
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &raw const caught, core::ptr::null_mut());
    }
}

/// What a program does on an interrupt: the shell's code for one.
pub const EXIT_INTERRUPTED: i32 = 130;

/// A pipe read as text, a line at a time: `\r\n` and `\r` end a line as
/// `\n` does and come back as `\n`, and text that is not UTF-8 is an error.
pub struct Lines<R> {
    from: R,
    held: Vec<u8>,
    stream: Stream,
}

/// Whether a read has found the end of what [`Lines`] reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stream {
    Open,
    Ended,
}

impl<R: Read> Lines<R> {
    /// Lines from `from`.
    pub fn new(from: R) -> Self {
        Lines {
            from,
            held: Vec::new(),
            stream: Stream::Open,
        }
    }

    /// The next line with its `\n`, the last one without if the stream
    /// stopped short of one, and `None` at the end. An interrupted read is
    /// returned as an error and loses nothing: call again to go on.
    pub fn next_line(&mut self) -> io::Result<Option<String>> {
        loop {
            // A `\r` the buffer ends on may be half of a `\r\n`, so it waits
            // for the next byte unless there is none.
            let ended = self.stream == Stream::Ended;
            let cut = self.held.iter().position(|b| matches!(b, b'\n' | b'\r'));
            let usable =
                cut.filter(|i| ended || self.held[*i] == b'\n' || *i + 1 < self.held.len());
            if let Some(i) = usable {
                let ending = match (self.held[i], self.held.get(i + 1)) {
                    (b'\r', Some(b'\n')) => 2,
                    _ => 1,
                };
                let mut line: Vec<u8> = self.held.drain(..i + ending).collect();
                line.truncate(i);
                line.push(b'\n');
                return decode(line).map(Some);
            }
            if ended {
                if self.held.is_empty() {
                    return Ok(None);
                }
                return decode(std::mem::take(&mut self.held)).map(Some);
            }
            let mut chunk = [0u8; 8192];
            let n = self.from.read(&mut chunk)?;
            if n == 0 {
                self.stream = Stream::Ended;
            }
            self.held.extend_from_slice(&chunk[..n]);
        }
    }
}

fn decode(line: Vec<u8>) -> io::Result<String> {
    String::from_utf8(line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// A binary running with its stderr on a pipe and its stdout on nothing.
pub struct Launch {
    child: Child,
    lines: Lines<ChildStderr>,
}

impl Launch {
    /// Starts `binary` with exactly `env`, named a config ([`guarded`]).
    pub fn start(binary: &str, env: &Env) -> io::Result<Launch> {
        let mut child = Command::new(binary)
            .env_clear()
            .envs(guarded(env)?)
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()?;
        let stderr = child.stderr.take().expect("stderr was piped");
        Ok(Launch {
            child,
            lines: Lines::new(stderr),
        })
    }

    /// The next line of its stderr, as [`Lines::next_line`] gives it.
    pub fn next_line(&mut self) -> io::Result<Option<String>> {
        self.lines.next_line()
    }

    /// `SIGTERM`, five seconds to go, then `SIGKILL`. The pipe stays open
    /// until the process has gone, so a last write of its own cannot fail.
    pub fn stop(mut self) {
        stop(&mut self.child, Duration::from_secs(5));
    }
}

/// **`env` as a launch of the app is given it, named a config so the
/// launch never writes one.** baud writes its commented default config when
/// it finds none and `BAUD_CONFIG` is unset (`baud-app/src/default_config.rs`),
/// which under the caller's `HOME` would leave a file in their home. So with
/// `BAUD_CONFIG` unset or empty in `env`, it is set to the file baud's lookup
/// would read -- `$XDG_CONFIG_HOME/baud/config.toml` for an absolute
/// `XDG_CONFIG_HOME`, then `~/.config/baud/config.toml` -- and, with neither
/// there, to an empty file of this process's own under `TMPDIR`, which is the
/// defaults. What the launch reads is unchanged either way. Every launch of
/// the app binary in this crate goes through it.
pub fn guarded(env: &Env) -> io::Result<Vec<(OsString, OsString)>> {
    let mut out = env.to_vec();
    if var(env, "BAUD_CONFIG").is_some_and(|v| !v.is_empty()) {
        return Ok(out);
    }
    out.retain(|(k, _)| k != OsStr::new("BAUD_CONFIG"));
    let dir = |k: &str| var(env, k).map(PathBuf::from).filter(|p| p.is_absolute());
    let found = [
        dir("XDG_CONFIG_HOME").map(|x| x.join("baud/config.toml")),
        dir("HOME").map(|h| h.join(".config/baud/config.toml")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.exists());
    let config = match found {
        Some(p) => p,
        None => {
            let base = var(env, "TMPDIR").unwrap_or_else(|| "/tmp".to_string());
            let p = Path::new(&base).join(format!("baud-bench-config-{}.toml", std::process::id()));
            fs::write(&p, "")?;
            p
        }
    };
    out.push(("BAUD_CONFIG".into(), config.into_os_string()));
    Ok(out)
}

/// A new directory for one run, under `TMPDIR` as `env` has it.
pub fn scratch_dir(env: &Env, prefix: &str) -> io::Result<PathBuf> {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let base = var(env, "TMPDIR").unwrap_or_else(|| "/tmp".to_string());
    let run = RUNS.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(&base).join(format!("{prefix}{}-{run}", std::process::id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `SIGTERM`, `grace` to go, then `SIGKILL`.
pub fn stop(child: &mut Child, grace: Duration) {
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: `kill` reads no memory. `pid` is this process's child; if a
        // caller's `try_wait` has already reaped it, as dock_check's wait can,
        // the signal finds no such process.
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
    let end = Instant::now() + grace;
    while Instant::now() < end {
        match child.try_wait() {
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            _ => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The middle of `xs`, sorted; the upper of the two middles when the count
/// is even. `None` when there is nothing to take one of.
pub fn upper_median(xs: &[f64]) -> Option<f64> {
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    s.get(s.len() / 2).copied()
}

/// The smallest of `xs`.
pub fn lowest(xs: &[f64]) -> f64 {
    xs.iter().copied().fold(f64::INFINITY, f64::min)
}

/// The largest of `xs`.
pub fn highest(xs: &[f64]) -> f64 {
    xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

/// `items` turned left by `round`, so each arm takes each place in turn.
pub fn rotated<T: Clone>(items: &[T], round: usize) -> Vec<T> {
    let k = round.checked_rem(items.len()).unwrap_or(0);
    items[k..].iter().chain(&items[..k]).cloned().collect()
}

/// `(name, value)` pairs in the order each name first came, the way a
/// Python `dict` keeps them: two arms may share a name and then a list.
pub struct Keyed<T>(pub Vec<(String, T)>);

impl<T: Default> Keyed<T> {
    /// A key for each of `names`, empty.
    pub fn of<'a>(names: impl Iterator<Item = &'a str>) -> Self {
        let mut keyed = Keyed(Vec::new());
        for n in names {
            if keyed.get(n).is_none() {
                keyed.0.push((n.to_string(), T::default()));
            }
        }
        keyed
    }
}

impl<T> Keyed<T> {
    /// The value at `name`.
    pub fn get(&self, name: &str) -> Option<&T> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// The value at `name`, to change it.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut T> {
        self.0.iter_mut().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}
