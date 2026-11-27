//! Idle CPU: what a binary costs with a screen that never changes.
//!
//! The app is launched with a child that writes nothing, so every
//! display-link tick finds no bytes, no dirty rows and no blink.
//!
//! **The launch is not idle and must not be in the number.** It is ~0.13 s
//! of CPU on this box against an idle rate near 0.004 s a second, so a
//! window total is three quarters launch and its variance swamps the
//! signal: differencing two window totals from two separate spawns measured
//! an A/A pair 37% apart. So the reading is taken from the *live* process,
//! twice, and only the delta is kept: one spawn, launch outside both
//! samples.
//!
//! `ps -o time` needs an entitlement on this macOS and `top` refuses to
//! sample, so the source is `proc_pid_rusage`. Its `ri_user_time` and
//! `ri_system_time` are **mach absolute time**, not nanoseconds: a fully
//! busy process moves them 23.8e6 in a second, which is the 41.67 ns tick
//! `mach_timebase_info` reports and the same clock the keystroke instrument
//! runs on. Reading them as nanoseconds understates CPU 24-fold.
//!
//! Arms are rotated round to round, the first round is discarded as
//! warm-up, and `--aa` adds a byte-identical copy of the first arm so the
//! spread of two identical binaries is the floor a difference must clear.
//! macOS only: elsewhere the first sample finds no process and it exits 1.

use super::{Env, Host, Keyed, highest, lowest, py};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

/// What `--help` prints.
pub const HELP: &str = "\
usage: idle-cpu [-h] --arm NAME=PATH [--rounds ROUNDS] [--settle SETTLE]
                [--window WINDOW] [--aa] [--hz HZ] [--cmd CMD]

options:
  -h, --help       show this help message and exit
  --arm NAME=PATH
  --rounds ROUNDS
  --settle SETTLE
  --window WINDOW
  --aa             add a copy of the first arm
  --hz HZ          the display's rate, for the per-tick column
  --cmd CMD        the child, which must write nothing
";

/// The usage line a bad command line gets on stderr.
const USAGE: &str = "usage: idle-cpu [-h] --arm NAME=PATH [--rounds ROUNDS] [--settle SETTLE]
                [--window WINDOW] [--aa] [--hz HZ] [--cmd CMD]";

/// The exit code of a bad command line.
const EXIT_USAGE: i32 = 2;

/// The CPU a live process has used, in seconds, or `None` if it is gone.
#[cfg(target_os = "macos")]
pub fn cpu_seconds(pid: u32) -> Option<f64> {
    // rusage_info_v2 opens with ri_uuid[16] -- two words -- then user and
    // system time. Read as words rather than mirroring the whole struct,
    // which has grown a flavor at a time and would rot.
    const UUID_WORDS: usize = 2;
    const FLAVOR_V2: libc::c_int = 2;
    let mut words = [0u64; 64];
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: `words` is larger than rusage_info_v2 and the kernel writes
    // only that flavor.
    let rc = unsafe { libc::proc_pid_rusage(pid, FLAVOR_V2, words.as_mut_ptr().cast()) };
    if rc != 0 {
        return None;
    }
    let ticks = words[UUID_WORDS] + words[UUID_WORDS + 1];
    Some(ticks as f64 * tick_ns() / 1e9)
}

/// There is no `proc_pid_rusage` here.
#[cfg(not(target_os = "macos"))]
pub fn cpu_seconds(_: u32) -> Option<f64> {
    None
}

/// A mach tick, in nanoseconds.
#[cfg(target_os = "macos")]
fn tick_ns() -> f64 {
    // SAFETY: `info` is a live value of the type the call writes. Deprecated
    // in the crate for the `mach2` crate's, which is the same call.
    #[allow(deprecated)]
    let (numer, denom) = unsafe {
        let mut info: libc::mach_timebase_info = core::mem::zeroed();
        libc::mach_timebase_info(&raw mut info);
        (info.numer, info.denom)
    };
    f64::from(numer) / f64::from(denom)
}

/// Whether `--aa` was given.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AaArm {
    Absent,
    Present,
}

/// The parsed command line.
struct Args {
    arms: Vec<(String, String)>,
    rounds: i128,
    settle: f64,
    window: f64,
    aa: AaArm,
    hz: f64,
    cmd: String,
}

/// The long options and whether each takes a value.
const OPTIONS: [(&str, bool); 7] = [
    ("--help", false),
    ("--arm", true),
    ("--rounds", true),
    ("--settle", true),
    ("--window", true),
    ("--aa", false),
    ("--hz", true),
];

/// What a typed `--name` names.
enum Named {
    One(&'static str),
    /// Argparse stops here, before anything later on the line is read.
    Several,
    /// Left over: it is an error only if the line runs to its end.
    Nothing,
}

/// Every long option's name.
fn names() -> impl Iterator<Item = &'static str> + Clone {
    OPTIONS.iter().map(|(n, _)| *n).chain(["--cmd"])
}

/// An option's whole name from what was typed: exact, or the one it begins.
fn resolve(typed: &str) -> Named {
    if let Some(exact) = names().find(|n| *n == typed) {
        return Named::One(exact);
    }
    let mut matching = names().filter(|n| n.starts_with(typed));
    match (matching.next(), matching.next()) {
        (Some(one), None) => Named::One(one),
        (Some(_), Some(_)) => Named::Several,
        _ => Named::Nothing,
    }
}

/// Argparse's `^-\d+$|^-\d*\.\d+$`, in ASCII digits: `$` also takes a
/// closing newline. `-1e5`, `-1.` and `-1_0` are numbers to `float` and options
/// to this.
fn is_negative_number(a: &str) -> bool {
    let Some(rest) = a.strip_prefix('-') else {
        return false;
    };
    let rest = rest.strip_suffix('\n').unwrap_or(rest);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(rest)
        || rest
            .split_once('.')
            .is_some_and(|(whole, frac)| (whole.is_empty() || digits(whole)) && digits(frac))
}

/// Whether `a` can be an option's value rather than another option, in
/// argparse's order: what names an option is one whatever else it holds, and
/// only then do a negative number and a space make a value of the rest.
fn is_value(a: &str) -> bool {
    if !a.starts_with('-') || a == "-" {
        return true;
    }
    if a == "--" || a.starts_with("-h") || a.starts_with("-=") {
        return false;
    }
    if a.starts_with("--") {
        let typed = a.split_once('=').map_or(a, |(t, _)| t);
        if names().any(|n| n.starts_with(typed)) {
            return false;
        }
    }
    is_negative_number(a) || a.contains(' ')
}

/// How a command line ended without arguments to run on.
enum Parsed {
    Run(Args),
    Help,
    Usage(String),
}

fn parse(args: &[String]) -> Parsed {
    let mut p = Args {
        arms: Vec::new(),
        rounds: 5,
        settle: 3.0,
        window: 20.0,
        aa: AaArm::Absent,
        hz: 120.0,
        cmd: "sleep 600".to_string(),
    };
    let mut unknown = Vec::new();
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        if a == "--" {
            unknown.push(a.clone());
            unknown.extend(it.by_ref().cloned());
            break;
        }
        // What follows `-h` is more single-letter options, which help comes
        // before, unless it is an explicit argument, which help refuses.
        if let Some(tail) = a.strip_prefix("-h") {
            if tail.starts_with(['=', '-']) {
                return Parsed::Usage("-h: ignored explicit argument".to_string());
            }
            return Parsed::Help;
        }
        // Before an `=`, a lone `-` begins every option's name.
        if a.starts_with("-=") {
            return Parsed::Usage(format!("ambiguous option: {a}"));
        }
        if !a.starts_with("--") {
            unknown.push(a.clone());
            continue;
        }
        let (typed, inline) = match a.split_once('=') {
            Some((t, v)) => (t, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        let name = match resolve(typed) {
            Named::One(name) => name,
            Named::Several => return Parsed::Usage(format!("ambiguous option: {a}")),
            Named::Nothing => {
                unknown.push(a.clone());
                continue;
            }
        };
        match name {
            "--help" | "--aa" => {
                if inline.is_some() {
                    return Parsed::Usage(format!("{name}: ignored explicit argument"));
                }
                if name == "--help" {
                    return Parsed::Help;
                }
                p.aa = AaArm::Present;
            }
            _ => {
                let value = match inline {
                    Some(v) => v,
                    None => match it.next_if(|v| is_value(v)) {
                        Some(v) => v.clone(),
                        None => return Parsed::Usage(format!("{name}: expected one argument")),
                    },
                };
                let bad = || Parsed::Usage(format!("{name}: invalid value: {value}"));
                match name {
                    "--arm" => {
                        let (n, path) = value.split_once('=').unwrap_or((value.as_str(), ""));
                        p.arms.push((n.to_string(), py::abspath(path)));
                    }
                    "--rounds" => match py::int(&value) {
                        Some(v) => p.rounds = v,
                        None => return bad(),
                    },
                    "--settle" | "--window" | "--hz" => {
                        let Some(v) = py::float(&value) else {
                            return bad();
                        };
                        match name {
                            "--settle" => p.settle = v,
                            "--window" => p.window = v,
                            _ => p.hz = v,
                        }
                    }
                    _ => p.cmd = value,
                }
            }
        }
    }
    if p.arms.is_empty() {
        return Parsed::Usage("the following arguments are required: --arm".to_string());
    }
    if !unknown.is_empty() {
        return Parsed::Usage(format!("unrecognized arguments: {}", unknown.join(" ")));
    }
    Parsed::Run(p)
}

/// A sleep Python would have taken: a negative or a NaN one is an error, and
/// so is one whose nanoseconds do not fit its clock's 64 bits.
fn pause(host: &mut dyn Host, seconds: f64) -> Result<(), String> {
    const LONGEST_NS: f64 = 9_223_372_036_854_775_808.0;
    if !(seconds >= 0.0 && seconds * 1e9 < LONGEST_NS) {
        return Err(format!("sleep length: {seconds}"));
    }
    host.pause(seconds);
    Ok(())
}

/// CPU-seconds a second the binary spends idling, launch excluded.
fn sample(
    binary: &str,
    args: &Args,
    env: &[(OsString, OsString)],
    host: &mut dyn Host,
) -> Result<f64, String> {
    let env = super::guarded(env).map_err(|e| format!("{binary}: a config to name: {e}"))?;
    let mut child = Command::new(binary)
        .env_clear()
        .envs(env)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{binary}: {e}"))?;
    let pid = child.id();
    let mut measure = || -> Result<f64, String> {
        pause(host, args.settle)?;
        if host.interrupted() {
            return Err("interrupted".to_string());
        }
        let t0 = host.wall();
        let c0 = host
            .cpu_seconds(pid)
            .ok_or_else(|| format!("{binary} exited during its settle"))?;
        pause(host, args.window)?;
        if host.interrupted() {
            return Err("interrupted".to_string());
        }
        let t1 = host.wall();
        let c1 = host
            .cpu_seconds(pid)
            .ok_or_else(|| format!("{binary} exited during its window"))?;
        if t1 == t0 {
            return Err("division by zero: an empty window".to_string());
        }
        Ok((c1 - c0) / (t1 - t0))
    };
    let got = measure();
    super::stop(&mut child, Duration::from_secs(15));
    got
}

/// The middle of a sorted list, the two middles' mean when the count is even.
pub fn true_median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// `env` with `cmd` as the child.
pub fn child_env(env: &Env, cmd: &str) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = env
        .iter()
        .filter(|(k, _)| k != "BAUD_CMD")
        .cloned()
        .collect();
    out.push(("BAUD_CMD".into(), cmd.into()));
    out
}

/// Runs the program; the return value is its exit code.
pub fn run(args: &[String], env: &Env, host: &mut dyn Host, out: &mut dyn Write) -> i32 {
    let p = match parse(args) {
        Parsed::Run(p) => p,
        Parsed::Help => {
            let _ = out.write_all(HELP.as_bytes());
            return 0;
        }
        Parsed::Usage(why) => {
            eprintln!("{USAGE}\nidle-cpu: error: {why}");
            return EXIT_USAGE;
        }
    };
    let mut arms = p.arms.clone();
    let mut aa_dir = None;
    if p.aa == AaArm::Present {
        let dir = match super::scratch_dir(env, "idle-aa-") {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!("--aa: {e}");
                return 1;
            }
        };
        let copy = dir.join("baud");
        let made = fs::copy(&arms[0].1, &copy);
        aa_dir = Some(dir);
        if let Err(e) = made {
            eprintln!("--aa: {e}");
            remove(aa_dir.as_ref());
            return 1;
        }
        arms.push(("aa".to_string(), copy.display().to_string()));
    }
    let code = measure(&p, &arms, env, host, out).unwrap_or_else(|e| {
        eprintln!("{e}");
        1
    });
    remove(aa_dir.as_ref());
    if host.interrupted() {
        return super::EXIT_INTERRUPTED;
    }
    code
}

fn remove(dir: Option<&std::path::PathBuf>) {
    if let Some(dir) = dir {
        let _ = fs::remove_dir_all(dir);
    }
}

fn measure(
    p: &Args,
    arms: &[(String, String)],
    env: &Env,
    host: &mut dyn Host,
    out: &mut dyn Write,
) -> Result<i32, Box<dyn std::error::Error>> {
    let launch_env = child_env(env, &p.cmd);
    let mut rates = Keyed::<Vec<f64>>::of(arms.iter().map(|(n, _)| n.as_str()));
    for r in 0..usize::try_from(p.rounds).unwrap_or(0) {
        for (name, path) in super::rotated(arms, r) {
            let rate = sample(&path, p, &launch_env, host)?;
            if p.hz == 0.0 {
                return Err("division by zero: --hz 0".into());
            }
            if let Some(v) = rates.get_mut(&name) {
                v.push(rate);
            }
            writeln!(
                out,
                "  round {} {name:6}: {:6.3}% of a core, {} us a tick",
                r + 1,
                rate * 100.0,
                py::fixed(rate / p.hz * 1e6, 6, 1)
            )?;
            out.flush()?;
        }
    }
    report(p, arms, &rates, out)
}

fn report(
    p: &Args,
    arms: &[(String, String)],
    rates: &Keyed<Vec<f64>>,
    out: &mut dyn Write,
) -> Result<i32, Box<dyn std::error::Error>> {
    writeln!(out)?;
    writeln!(
        out,
        "{:8} {:>12} {:>20} {:>12}",
        "arm", "idle CPU", "range", "a tick"
    )?;
    let mut settled = Keyed::<Vec<f64>>(Vec::new());
    for (name, _) in arms {
        let all = rates.get(name).map_or(&[][..], Vec::as_slice);
        let mut v: Vec<f64> = all.iter().skip(1).copied().collect(); // the first round is warm-up
        if v.is_empty() {
            continue;
        }
        v.sort_by(f64::total_cmp);
        let med = true_median(&v);
        writeln!(
            out,
            "{name:8} {:11.3}% [{:7.3}-{:7.3}] {} us",
            med * 100.0,
            v[0] * 100.0,
            v[v.len() - 1] * 100.0,
            py::fixed(med / p.hz * 1e6, 9, 1)
        )?;
        if settled.get(name).is_none() {
            settled.0.push((name.clone(), v));
        }
    }
    let [(first, base_rates), .., (last, last_rates)] = settled.0.as_slice() else {
        return Ok(0);
    };
    let base = true_median(base_rates);
    let floor = highest(last_rates) - lowest(last_rates);
    writeln!(out)?;
    writeln!(
        out,
        "floor (the {last} arm's own spread): {:.3}% of a core",
        floor * 100.0
    )?;
    for (name, v) in &settled.0[1..] {
        let med = true_median(v);
        let delta = med - base;
        let disjoint = lowest(v) > highest(base_rates) || highest(v) < lowest(base_rates);
        let big = delta.abs() > floor && delta.abs() >= 0.05 * base;
        if base == 0.0 {
            return Err(io_error("division by zero: the base arm read no CPU").into());
        }
        writeln!(
            out,
            "{name} against {first}: {:+.3}% ({:+.1}%), ranges {} -- {}",
            delta * 100.0,
            delta / base * 100.0,
            if disjoint { "disjoint" } else { "overlap" },
            if disjoint && big {
                "result"
            } else {
                "not a result"
            }
        )?;
    }
    Ok(0)
}

fn io_error(text: &str) -> io::Error {
    io::Error::other(text)
}
