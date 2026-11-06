//! What the Metal shader cache is worth at launch: the same binary launched
//! with the system's shader caches in place (warm) and moved aside (cold),
//! interleaved, reading the app's own `gpu build:` line.
//!
//! The app, under `BAUD_STARTUP=1`, prints one line from its GPU probe:
//!
//!     startup: gpu build: device D ms, library L (S), pipelines P, warm pass W, probe T
//!
//! `S` is where the library came from: `Library` for the committed
//! `baud.metallib`, `Compiled` when it would not load and the source was
//! compiled instead. A cold arm that reads `Library` and a cold library time
//! is the whole of what the `.metallib` shipped, and an arm that reads
//! `Compiled` is measuring the old program.
//!
//! `library` is the shader source to a library and `pipelines` the five
//! `newRenderPipelineState` calls; both are served from the system's cache
//! on a warm launch and compiled on a cold one. `device` and the warm pass
//! do not depend on the cache and are the control: if they move between
//! arms, the arms are not what they claim.
//!
//! **What "cold" is here.** macOS keeps the compiled shaders in two per-user
//! directories under `$(getconf DARWIN_USER_CACHE_DIR)`: `com.apple.metalfe`
//! (the front end: source to AIR) and `com.apple.metal` (per-GPU function
//! and library binaries). A cold arm moves both aside before the launch and
//! restores them after it, so every cold launch compiles from nothing, which
//! is what the first launch after an install or an OS update does. The
//! directories belong to every app on the box, so restoring them is not
//! optional: this restores them on every way out but a `SIGKILL`, and an
//! entry another app wrote while they were aside is lost to it (that app
//! recompiles once). A run killed outright leaves them aside, and the next
//! run puts them back first. Do not run this beside anything that matters.
//!
//! **How to read it.** Per arm, the median and [min-max] of each field over
//! the settled launches (the first launch of each arm is discarded), plus
//! the wall from spawn to the first present. The keep rule is >= 20 ms on
//! launch: if cold - warm is under that, the cache is not where the launch
//! time is.
//!
//! `BAUD_SHADER_CACHE_ASIDE` picks what the cold arm moves aside: `both`
//! (the default, a first launch after an install or an OS update), `metalfe`
//! (the front end only: what a precompiled `.metallib` would spare) or
//! `metal` (the per-GPU binaries only: what a binary archive would spare).
//!
//! Exits 0 when measured, and 1 when a launch printed no `gpu build:` line
//! or the caches could not be moved and restored. `SIGINT`, `SIGTERM` or
//! `SIGHUP` puts the caches back and exits 130.

use super::{EXIT_INTERRUPTED, Env, Host, Launch, highest, lowest, py, upper_median, var};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// The arguments, for the usage line.
pub const USAGE: &str = "usage: shader-cache-bench [launches=7] <binary>";

/// The fields of the `gpu pipelines:` line.
const PIPE_FIELDS: [&str; 5] = ["bg", "glyph", "cell_bg", "cell_glyph", "cell_shape"];
/// Every column of the report, in its order.
const FIELDS: [&str; 11] = [
    "device",
    "library",
    "pipelines",
    "warm",
    "probe",
    "wall",
    "bg",
    "glyph",
    "cell_bg",
    "cell_glyph",
    "cell_shape",
];
/// A cache's name, with the moved-aside name that follows it.
const ASIDE_SUFFIX: &str = ".shader-cache-bench-aside";

/// A line read with a cursor, the digits-and-dots fields as text.
struct Reader<'a>(&'a str);

impl<'a> Reader<'a> {
    fn lit(&mut self, s: &str) -> Option<()> {
        self.0 = self.0.strip_prefix(s)?;
        Some(())
    }

    fn take(&mut self, keep: impl Fn(char) -> bool) -> Option<&'a str> {
        let rest = self.0.trim_start_matches(keep);
        let got = &self.0[..self.0.len() - rest.len()];
        self.0 = rest;
        (!got.is_empty()).then_some(got)
    }

    fn number(&mut self) -> Option<&'a str> {
        self.take(|c| c.is_ascii_digit() || c == '.')
    }
}

/// The `gpu build:` line, as text: device, library, its source, pipelines,
/// warm pass, probe. Anything after the probe's value is ignored.
pub fn build_line(line: &str) -> Option<([&str; 5], &str)> {
    let mut r = Reader(line);
    r.lit("startup: gpu build: device ")?;
    let device = r.number()?;
    r.lit(" ms, library ")?;
    let library = r.number()?;
    r.lit(" (")?;
    let source = r.take(|c| c.is_alphanumeric() || c == '_')?;
    r.lit("), pipelines ")?;
    let pipelines = r.number()?;
    r.lit(", warm pass ")?;
    let warm = r.number()?;
    r.lit(", probe ")?;
    let probe = r.number()?;
    Some(([device, library, pipelines, warm, probe], source))
}

/// The `gpu pipelines:` line's five, as text.
pub fn pipes_line(line: &str) -> Option<[&str; 5]> {
    let mut r = Reader(line);
    r.lit("startup: gpu pipelines: bg ")?;
    let bg = r.number()?;
    r.lit(" ms, glyph ")?;
    let glyph = r.number()?;
    r.lit(", cell_bg ")?;
    let cell_bg = r.number()?;
    r.lit(", cell_glyph ")?;
    let cell_glyph = r.number()?;
    r.lit(", cell_shape ")?;
    let cell_shape = r.number()?;
    Some([bg, glyph, cell_bg, cell_glyph, cell_shape])
}

/// Whether a line, with its ending, is the first present.
pub fn first_present(line: &str) -> bool {
    super::startup::reaches(line, &["first completion", "first present"])
}

/// The caches and where each goes aside.
pub struct Caches {
    pairs: Vec<(String, String)>,
}

impl Caches {
    /// The caches `names` under `root`.
    pub fn of(root: &str, names: &[&str]) -> Self {
        Caches {
            pairs: names
                .iter()
                .map(|n| (format!("{root}/{n}"), format!("{root}/{n}{ASIDE_SUFFIX}")))
                .collect(),
        }
    }

    /// The caches out of the way; a leftover from an interrupted run is
    /// restored first so nothing is buried twice.
    pub fn move_aside(&self) -> io::Result<()> {
        self.restore()?;
        for (c, a) in &self.pairs {
            if Path::new(c).is_dir() {
                fs::rename(c, a)?;
            }
        }
        Ok(())
    }

    /// The originals back, over whatever the cold launch wrote.
    pub fn restore(&self) -> io::Result<()> {
        for (c, a) in &self.pairs {
            if Path::new(a).is_dir() {
                if Path::new(c).is_dir() {
                    fs::remove_dir_all(c)?;
                }
                fs::rename(a, c)?;
            }
        }
        Ok(())
    }
}

/// One launch's fields and the library's source.
#[derive(Clone, Default)]
struct Got {
    fields: BTreeMap<&'static str, f64>,
    source: Option<String>,
}

/// How a launch ended.
enum Ended {
    Measured(Got),
    /// It printed too little, and this says what.
    Short(&'static str),
    /// An interrupt arrived.
    Interrupted,
}

fn number(text: &str) -> io::Result<f64> {
    py::float(text).ok_or_else(|| io::Error::other(format!("not a number: {text}")))
}

fn launch(binary: &str, env: &Env, host: &mut dyn Host) -> io::Result<Ended> {
    let mut child_env: Vec<(OsString, OsString)> = env
        .iter()
        .filter(|(k, _)| !matches!(k.to_str(), Some("BAUD_STARTUP" | "BAUD_GRID" | "BAUD_CMD")))
        .cloned()
        .collect();
    child_env.push(("BAUD_STARTUP".into(), "1".into()));
    child_env.push(("BAUD_GRID".into(), "120x40".into()));
    child_env.push(("BAUD_CMD".into(), "sleep 5".into()));
    let t0 = host.now();
    let mut running = Launch::start(binary, &child_env)?;
    let mut got = Got::default();
    let mut wall = None;
    loop {
        if host.interrupted() {
            running.stop();
            return Ok(Ended::Interrupted);
        }
        let line = match running.next_line() {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                running.stop();
                return Err(e);
            }
        };
        let read = read_fields(&line, &mut got);
        if let Err(e) = read {
            running.stop();
            return Err(e);
        }
        if first_present(&line) {
            wall = Some((host.now() - t0) * 1e3);
            break;
        }
    }
    running.stop();
    let Some(wall) = wall.filter(|_| got.fields.contains_key("probe")) else {
        return Ok(Ended::Short("no gpu build line, or no first present"));
    };
    if PIPE_FIELDS.iter().any(|f| !got.fields.contains_key(f)) {
        // The instrument outliving what feeds it is the failure this repo
        // records four times; a missing line stops the run rather than
        // reporting a sum it cannot break down. It names *which* line is
        // missing, because a guard that misattributes its own cause sends
        // the next reader to the wrong half of the instrument.
        return Ok(Ended::Short(
            "no gpu pipelines line: the five cannot be attributed",
        ));
    }
    got.fields.insert("wall", wall);
    Ok(Ended::Measured(got))
}

fn read_fields(line: &str, got: &mut Got) -> io::Result<()> {
    if let Some((values, source)) = build_line(line) {
        got.source = Some(source.to_string());
        for (name, v) in FIELDS[..5].iter().zip(values) {
            got.fields.insert(name, number(v)?);
        }
    }
    if let Some(values) = pipes_line(line) {
        for (name, v) in PIPE_FIELDS.iter().zip(values) {
            got.fields.insert(name, number(v)?);
        }
    }
    Ok(())
}

/// Runs the program; the return value is its exit code.
pub fn run(args: &[String], env: &Env, host: &mut dyn Host, out: &mut dyn Write) -> i32 {
    match measure(args, env, host, out) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn measure(
    args: &[String],
    env: &Env,
    host: &mut dyn Host,
    out: &mut dyn Write,
) -> Result<i32, Box<dyn std::error::Error>> {
    let launches = match args.first() {
        Some(n) => py::int(n).ok_or_else(|| format!("launches: {n}"))?,
        None => 7,
    };
    let binary = args.get(1).ok_or(USAGE)?;
    let which = var(env, "BAUD_SHADER_CACHE_ASIDE").unwrap_or_else(|| "both".to_string());
    let names: &[&str] = match which.as_str() {
        "both" => &["com.apple.metal", "com.apple.metalfe"],
        "metal" => &["com.apple.metal"],
        "metalfe" => &["com.apple.metalfe"],
        other => return Err(format!("BAUD_SHADER_CACHE_ASIDE: {other}").into()),
    };
    let root = host.cache_root()?;
    let caches = Caches::of(&root, names);

    let mut warm = Vec::new();
    let mut cold = Vec::new();
    // Whatever way the sweep ends, the caches go back first.
    let swept = sweep(
        launches, binary, env, &caches, host, out, &mut warm, &mut cold,
    );
    let restored = caches.restore();
    let stopped = swept?;
    restored?;
    if let Some(code) = stopped {
        return Ok(code);
    }
    report(&warm, &cold, names, out)
}

/// Interleaved, the cold arm first so a warm launch never follows a cold
/// one that left the caches aside. `Some` is the exit code of a run that
/// stopped early.
#[allow(clippy::too_many_arguments)]
fn sweep(
    launches: i128,
    binary: &str,
    env: &Env,
    caches: &Caches,
    host: &mut dyn Host,
    out: &mut dyn Write,
    warm: &mut Vec<Got>,
    cold: &mut Vec<Got>,
) -> Result<Option<i32>, Box<dyn std::error::Error>> {
    for i in 0..usize::try_from(launches).unwrap_or(0) {
        let nth = Nth(i);
        for arm in [Arm::Cold, Arm::Warm] {
            if arm == Arm::Cold {
                caches.move_aside()?;
            }
            let ended = launch(binary, env, host);
            if arm == Arm::Cold {
                let restored = caches.restore();
                let ended = ended?;
                restored?;
                if !step(ended, nth, arm, cold, host, out)? {
                    return Ok(Some(last_code(host)));
                }
            } else if !step(ended?, nth, arm, warm, host, out)? {
                return Ok(Some(last_code(host)));
            }
        }
    }
    Ok(None)
}

/// The exit code of a run that stopped early.
fn last_code(host: &mut dyn Host) -> i32 {
    if host.interrupted() {
        EXIT_INTERRUPTED
    } else {
        1
    }
}

/// Which arm a launch belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arm {
    Cold,
    Warm,
}

impl Arm {
    fn name(self) -> &'static str {
        match self {
            Arm::Cold => "cold",
            Arm::Warm => "warm",
        }
    }
}

/// A launch's place in the sweep, from zero.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Nth(usize);

/// One launch's line; `false` stops the sweep.
fn step(
    ended: Ended,
    nth: Nth,
    arm: Arm,
    into: &mut Vec<Got>,
    host: &mut dyn Host,
    out: &mut dyn Write,
) -> io::Result<bool> {
    let got = match ended {
        Ended::Measured(got) => got,
        Ended::Short(why) => {
            writeln!(out, "launch {} {}: {why}; stopping", nth.0, arm.name())?;
            return Ok(false);
        }
        Ended::Interrupted => return Ok(false),
    };
    let f = |name: &str| got.fields.get(name).copied().unwrap_or(f64::NAN);
    writeln!(
        out,
        "launch {} {:4}: library {:6.1} ms ({}), pipelines {:6.1}, device {:5.1}, \
         warm pass {:5.1}, probe {:6.1}, wall to first present {:6.1}{}",
        nth.0,
        arm.name(),
        f("library"),
        got.source.as_deref().unwrap_or("?"),
        f("pipelines"),
        f("device"),
        f("warm"),
        f("probe"),
        f("wall"),
        if nth == Nth(0) { "  (discarded)" } else { "" }
    )?;
    into.push(got.clone());
    host.pause(0.5);
    Ok(!host.interrupted())
}

/// The launches a median is taken over: all but the first, or the one.
fn settled(all: &[Got]) -> &[Got] {
    if all.len() > 1 { &all[1..] } else { all }
}

fn report(
    warm: &[Got],
    cold: &[Got],
    names: &[&str],
    out: &mut dyn Write,
) -> Result<i32, Box<dyn std::error::Error>> {
    writeln!(out)?;
    writeln!(
        out,
        "{:10} {:28} {:28} cold - warm",
        "field", "warm median [min-max]", "cold median [min-max]"
    )?;
    for f in FIELDS {
        let column = |all: &[Got]| -> Vec<f64> {
            settled(all)
                .iter()
                .filter_map(|g| g.fields.get(f).copied())
                .collect()
        };
        let (w, c) = (column(warm), column(cold));
        let wm = upper_median(&w).ok_or("no launch to report")?;
        let cm = upper_median(&c).ok_or("no launch to report")?;
        writeln!(
            out,
            "{f:10} {wm:7.1} [{:6.1}-{:6.1}]       {cm:7.1} [{:6.1}-{:6.1}]       {:+7.1} ms",
            lowest(&w),
            highest(&w),
            lowest(&c),
            highest(&c),
            cm - wm
        )?;
    }
    writeln!(
        out,
        "\n{} settled launches an arm; the first of each arm is discarded; \
         the cold arm moved aside: {}.",
        warm.len() - 1,
        names.join(", ")
    )?;

    // **Where the library came from, asserted and not merely printed.** A
    // field that is shown per launch and never read is the counter this
    // repo keeps recording: an arm whose launches disagree with each other
    // -- a fallback firing on some and not others -- would average two
    // programs and read as one result. Comparing a source-only binary
    // against a `.metallib` one is the whole point, so the arms are allowed
    // to differ from each other; what is refused is an arm that differs
    // from itself.
    let mut mixed = false;
    for (arm, all) in [("warm", warm), ("cold", cold)] {
        let mut seen: Vec<&str> = settled(all)
            .iter()
            .map(|g| g.source.as_deref().unwrap_or("?"))
            .collect();
        seen.sort_unstable();
        seen.dedup();
        writeln!(out, "{arm:5} library from: {}", seen.join(", "))?;
        mixed |= seen.len() > 1;
    }
    if mixed {
        eprintln!(
            "an arm's launches did not all take the same library, so its median \
             is two programs averaged; discard this run"
        );
        return Ok(1);
    }
    Ok(0)
}
