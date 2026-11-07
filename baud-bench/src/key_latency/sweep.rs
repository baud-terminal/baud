//! The sweep: launch each arm, post keys with the Swift helper, collect the
//! `keylat:` lines, and report. It needs a built `baud`, a window and the
//! helper's Accessibility grant; the tests run it against stand-ins for all
//! three, which is the whole of what a headless box can check of it.

use super::cli::{AaArm, Args, Child};
use super::parse::{Count, PREDICT_NOTED, PREDICT_REFUSED, Post};
use super::parse::{parse_front_line, parse_keylat_lines, parse_post_line};
use super::report::{aa_text, deltas_text, summary_text};
use super::rows::{Diag, Row, Tally, exclude_refused_landings, pair_by_order};
use super::stats::{
    DEFAULT_WINDOW, Ms, Round, Summary, WINDOW_MARGIN, WarmUp, aa_floor, gap_bounds, merge_diags,
    refused_runs, rotate, settled_rounds, summarize, warmup_measured_nothing, window_for_gap,
    window_ns,
};
use super::{py, sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The app expires a record 2 s after its event, on a tick; SIGTERM must
/// come no sooner than this after the last post, so a no-echo run emits every
/// record it opened.
const DRAIN: Duration = Duration::from_millis(2600);

const XCODE_DEVELOPER_DIR: &str = "/Applications/Xcode.app/Contents/Developer";

/// What the helper is, relative to the repository's root, where a sweep runs.
const HELPER_SCRIPT: &str = "scripts/key-post.swift";

/// A prompt that never echoes: the marks zsh sends (`A`, then `B` where the
/// input starts), echo off, and nothing that reads. `\033` and `\007` are
/// what `sh`'s printf spells ESC and BEL as.
pub const MUTE_CMD: &str = r"printf '\033]133;A\007\033]133;B\007'; stty -echo; sleep 60";

/// The flag that runs this binary as `--child stream`'s child.
pub const STREAM_FLAG: &str = "--stream-child";

/// `--child stream` prints a line every this long: two to three lines a
/// display period at 120 Hz, so a key's echo always lands inside the frame's
/// time gate.
pub const STREAM_PERIOD: Duration = Duration::from_millis(2);

/// `--child stream`'s child: the number, the line's end, and a sleep, for
/// ever. Each write goes out alone, two to a line, as the interpreter the
/// first form of this child ran them. It ends when the pty goes away.
pub fn stream_child() -> i32 {
    let mut out = std::io::stdout().lock();
    let mut i: u128 = 0;
    loop {
        let wrote = write!(out, "{i}")
            .and_then(|()| out.flush())
            .and_then(|()| writeln!(out))
            .and_then(|()| out.flush());
        if wrote.is_err() {
            return 1;
        }
        i += 1;
        std::thread::sleep(STREAM_PERIOD);
    }
}

/// `s` as one word to `sh`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// What `BAUD_CMD` the child mode sets, or `None` to leave the login shell:
/// `cat` the tty's own echo, `none` a tty with echo off -- the control,
/// `stream` this binary printing a line every 2 ms under the tty's echo --
/// the row-6 workload, `mute` the prompt that never answers.
pub fn child_env(child: Child, exe: &str) -> Option<String> {
    match child {
        Child::Cat => Some("cat".to_string()),
        Child::None => Some("stty -echo; sleep 60".to_string()),
        Child::Stream => Some(format!("{} {STREAM_FLAG}", sh_quote(exe))),
        Child::Mute => Some(MUTE_CMD.to_string()),
        Child::Shell => None,
    }
}

fn own_path() -> String {
    std::env::current_exe().map_or_else(|_| "key-latency".to_string(), |p| p.display().to_string())
}

/// A process id, the app's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pid(pub u32);

/// What a program printed and how it ended.
struct Ran {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn exit_of(status: std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.code().or(status.signal().map(|s| -s))
}

/// Run a program to its end, or kill it at `limit`, with both its outputs
/// read as a text-mode pipe reads them. An `Err` says why it gave no answer.
fn run_for(argv: &[String], limit: Duration) -> Result<Ran, String> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let collect = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            py::text_mode(&String::from_utf8_lossy(&bytes))
        })
    };
    let out = collect(child.stdout.take().map(|p| Box::new(p) as _));
    let err = collect(child.stderr.take().map(|p| Box::new(p) as _));
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Command {} timed out after {} seconds",
                    argv.join(" "),
                    limit.as_secs()
                ));
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    Ok(Ran {
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
        code: exit_of(status),
    })
}

/// The Xcode developer directory with a compiler in it: Xcode.app when it
/// exists, else what `xcode-select -p` answers if that carries a toolchain.
/// A dev shell's DEVELOPER_DIR can point at an SDK with no compiler, and
/// `xcode-select -p` honours DEVELOPER_DIR, so Xcode.app is preferred
/// outright.
fn developer_dir() -> Option<String> {
    if Path::new(XCODE_DEVELOPER_DIR).is_dir() {
        return Some(XCODE_DEVELOPER_DIR.to_string());
    }
    let argv = ["xcode-select".to_string(), "-p".to_string()];
    let p = run_for(&argv, Duration::from_secs(5)).ok()?;
    let cand = py::strip(&p.stdout).to_string();
    let swift = format!("{cand}/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift");
    (p.code == Some(0) && Path::new(&swift).is_file()).then_some(cand)
}

/// The argv to run a Swift script. `$BAUD_SWIFT` overrides the whole command
/// (space-split); otherwise Xcode.app's toolchain with an explicit SDK, then
/// `xcode-select -p`, then PATH `swift` for a normal box.
pub fn swift_argv(script: &str, args: &[String]) -> Vec<String> {
    let mut argv: Vec<String> = Vec::new();
    let over = std::env::var("BAUD_SWIFT").ok().filter(|v| !v.is_empty());
    if let Some(over) = over {
        argv.extend(py::split(&over).map(str::to_string));
    } else if let Some((dd, sdk)) = developer_dir().and_then(|dd| {
        let sdk = format!("{dd}/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk");
        Path::new(&sdk).is_dir().then_some((dd, sdk))
    }) {
        argv.extend([
            "/usr/bin/env".to_string(),
            format!("DEVELOPER_DIR={dd}"),
            "swift".to_string(),
            "-sdk".to_string(),
            sdk,
        ]);
    } else {
        argv.push("swift".to_string());
    }
    argv.push(script.to_string());
    argv.extend_from_slice(args);
    argv
}

/// Block until every needle has appeared as a prefix in the shared lines, the
/// app's stderr closed, or the limit passed.
fn wait_for(
    lines: &Mutex<Vec<String>>,
    stop: &AtomicBool,
    needles: &[&str],
    limit: Duration,
) -> Result<(), &'static str> {
    let deadline = Instant::now() + limit;
    let mut pending: Vec<&str> = needles.to_vec();
    while !pending.is_empty() && Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
        {
            let seen = lines.lock().expect("the reader never panics");
            pending.retain(|n| !seen.iter().any(|l| l.starts_with(n)));
        }
        if !pending.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if pending.is_empty() {
        return Ok(());
    }
    if stop.load(Ordering::SeqCst) {
        return Err("app exited before presenting");
    }
    Err("app never presented (timeout)")
}

/// Raise baud to the front from the driver: the helper's own
/// NSRunningApplication.activate() is blocked by focus-stealing prevention
/// (it is a separate, unbundled process), so System Events sets the process
/// frontmost, and the helper's `--front` mode is polled up to 2 s for both
/// the front layer-0 window and the frontmost application to be baud's. `Ok`
/// and `Err` each carry the last front line. The helper's guard is still the
/// gate; this only makes it pass.
fn activate(pid: Pid) -> Result<String, String> {
    let script = format!(
        "tell application \"System Events\" to set frontmost of (first process whose unix id \
         is {}) to true",
        pid.0
    );
    let argv = ["osascript".to_string(), "-e".to_string(), script];
    run_for(&argv, Duration::from_secs(5)).map_err(|e| format!("osascript failed: {e}"))?;
    let want = i128::from(pid.0);
    let mut last = "?".to_string();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let argv = swift_argv(HELPER_SCRIPT, &["--front".to_string()]);
        let fp = run_for(&argv, Duration::from_secs(10))
            .map_err(|e| format!("helper --front failed: {e}"))?;
        for line in py::splitlines(&fp.stdout) {
            if let Some(f) = parse_front_line(line) {
                last = py::strip(line).to_string();
                if f.window_pid == want && f.app_pid == want {
                    return Ok(last);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(last)
}

/// How a run ended, for the sweep.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RunEnd {
    /// The helper posted all it was asked to.
    Clean,
    /// The helper's guard refused; what it posted first is paired.
    Refused,
    /// The helper failed some other way; the sweep stops.
    Fatal,
}

/// One arm's run.
struct RunOut {
    rows: Vec<Row>,
    diag: Option<Diag>,
    end: RunEnd,
}

fn stop_app(app: &mut std::process::Child, stop: &AtomicBool) {
    if matches!(app.try_wait(), Ok(None)) {
        // kill(1) rather than kill(2), so this crate's `unsafe` stays in its
        // two harnesses. A child not yet waited for keeps its pid.
        let term = Command::new("kill")
            .args(["-TERM", &app.id().to_string()])
            .status();
        if !term.is_ok_and(|s| s.success()) {
            let _ = app.kill();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match app.try_wait() {
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                let _ = app.kill();
                let _ = app.wait();
                break;
            }
            _ => break,
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Launch `binary`, wait for it to present, post the keys with the Swift
/// helper, collect the keylat lines and write the raw rows beside the report.
/// SIGTERMs the app no sooner than [`DRAIN`] after the last post.
fn run_arm(
    binary: &str,
    args: &Args,
    scratch: &str,
    rnd: Round,
    arm: &str,
    window: super::parse::Ns,
) -> Result<RunOut, String> {
    let round = rnd.0;
    let mut cmd = Command::new(binary);
    cmd.env("BAUD_KEYLAT", "1")
        .env("BAUD_STARTUP", "1")
        .env("BAUD_GRID", &args.grid)
        .env_remove("BAUD_CMD")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(c) = child_env(args.child, &own_path()) {
        cmd.env("BAUD_CMD", c);
    }
    // Named a config, so the arm never writes one (`host::guarded`).
    let env: Vec<_> = std::env::vars_os().collect();
    let guarded = crate::host::guarded(&env).map_err(|e| format!("a config to name: {e}"))?;
    if let Some((k, v)) = guarded.into_iter().find(|(k, _)| k == "BAUD_CONFIG") {
        cmd.env(k, v);
    }
    let mut app = cmd
        .spawn()
        .map_err(|e| format!("launching {binary}: {e}"))?;
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(pipe) = app.stderr.take() {
        let (lines, stop) = (lines.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut pipe = BufReader::new(pipe);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match pipe.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = String::from_utf8_lossy(&buf);
                        let mut seen = lines.lock().expect("nothing panics holding the lines");
                        seen.extend(py::universal_lines(&text).into_iter().map(str::to_string));
                    }
                }
            }
            stop.store(true, Ordering::SeqCst);
        });
    }
    let snapshot = |from: usize| -> Vec<String> {
        lines.lock().expect("the reader never panics")[from..].to_vec()
    };

    let mut needles = vec!["startup: first present"];
    if args.child == Child::Shell {
        needles.push("startup: first content");
    }
    if let Err(why) = wait_for(&lines, &stop, &needles, Duration::from_secs(30)) {
        stop_app(&mut app, &stop);
        let stages: Vec<String> = snapshot(0)
            .into_iter()
            .filter(|l| l.starts_with("startup:"))
            .collect();
        println!(
            "  round {round} arm {arm}: {why}; stages seen: {}",
            py::repr_list(&stages)
        );
        return Ok(RunOut {
            rows: Vec::new(),
            diag: None,
            end: RunEnd::Clean,
        });
    }

    std::thread::sleep(Duration::from_millis(400)); // let the shell settle before typing

    let pid = Pid(app.id());
    if let Err(front) = activate(pid) {
        println!(
            "  round {round} arm {arm}: could not bring baud to the front ({front}); the \
             helper's guard will decide"
        );
    }

    let keylat_start = snapshot(0).len(); // only lines from here belong to this run
    let helper = swift_argv(
        HELPER_SCRIPT,
        &[
            pid.0.to_string(),
            args.keys.to_string(),
            args.gap.0.to_string(),
            args.chars.clone(),
        ],
    );
    let hp = match Command::new(&helper[0])
        .args(&helper[1..])
        .stdin(Stdio::inherit())
        .output()
    {
        Ok(hp) => hp,
        Err(e) => {
            stop_app(&mut app, &stop);
            return Err(format!("running the helper {}: {e}", helper.join(" ")));
        }
    };
    let (hout, herr) = (
        py::text_mode(&String::from_utf8_lossy(&hp.stdout)),
        py::text_mode(&String::from_utf8_lossy(&hp.stderr)),
    );
    let posts: Vec<Post> = py::splitlines(&hout)
        .into_iter()
        .filter_map(parse_post_line)
        .collect();
    let code = exit_of(hp.status);
    let refused = code == Some(3) || hout.contains("REFUSED") || herr.contains("REFUSED");
    if refused {
        println!(
            "  round {round} arm {arm}: helper REFUSED: {} {}",
            py::strip(&hout),
            py::strip(&herr)
        );
    } else if code != Some(0) {
        println!(
            "  round {round} arm {arm}: helper FAILED, exit {}\n  helper argv: {}\n  helper \
             stdout:\n{hout}\n  helper stderr:\n{herr}",
            code.unwrap_or(0),
            helper.join(" ")
        );
        stop_app(&mut app, &stop);
        return Ok(RunOut {
            rows: Vec::new(),
            diag: None,
            end: RunEnd::Fatal,
        });
    }

    // Drain: the app expires a record 2 s after its event, on a tick, so a
    // key that never echoes is only emitted after that; wait it out.
    std::thread::sleep(DRAIN);
    stop_app(&mut app, &stop);

    let parsed = parse_keylat_lines(&snapshot(keylat_start));
    let tally = |prefix: &str| {
        let mut t = Tally::default();
        for r in parsed.reports.iter().filter(|r| r.starts_with(prefix)) {
            t.add(py::strip(&r[prefix.len()..]), Count(1));
        }
        t
    };
    let (predict_refused, predict_noted) = (tally(PREDICT_REFUSED), tally(PREDICT_NOTED));
    if !posts.is_empty() && parsed.records.is_empty() {
        println!(
            "  round {round} arm {arm}: keys posted, none reached the app (Accessibility? wrong \
             target?)"
        );
    }
    for fmsg in &parsed.failures {
        println!("  round {round} arm {arm}: PARSE FAILURE {fmsg}");
    }
    let (mut rows, mut diag) = pair_by_order(&posts, &parsed.records, window);
    diag.parse_failures = Some(Count(parsed.failures.len()));
    diag.landing_refused = Some(parsed.refusals);
    if !predict_refused.is_empty() {
        println!(
            "  round {round} arm {arm}: predict refused: {}",
            predict_refused.sorted_line()
        );
    }
    if !predict_noted.is_empty() {
        println!(
            "  round {round} arm {arm}: predict noted: {}",
            predict_noted.sorted_line()
        );
    }
    diag.predict_refused = predict_refused;
    diag.predict_noted = predict_noted;
    let excluded = exclude_refused_landings(&mut rows, parsed.refusals);
    diag.refused_landing_rows = Some(excluded);
    if parsed.refusals != Count(0) {
        println!(
            "  round {round} arm {arm}: NOT QUOTABLE -- {} landing(s) refused; {excluded} \
             GPU-path row(s) with landed=- excluded as refused-landing",
            parsed.refusals
        );
    }
    for r in &rows {
        if let Some(why) = &r.exclude_reason {
            println!(
                "  round {round} arm {arm}: excluded seq={} (app seq={}): {why}",
                r.seq.0, r.app_seq.0
            );
        }
    }

    let tsv = tsv_path(scratch, arm, rnd);
    std::fs::write(&tsv, tsv_text(&rows)).map_err(|e| format!("{tsv}: {e}"))?;
    println!(
        "  round {round} arm {arm}: pid={} posted={} keylats={} paired={} unpaired posts={} \
         unpaired keylats={} seq mismatches={} negative={} parse failures={} landings refused={} \
         -> {tsv}",
        pid.0,
        posts.len(),
        parsed.records.len(),
        diag.paired,
        diag.unpaired_posts,
        diag.unpaired_keylats,
        diag.seq_mismatch,
        diag.negative,
        parsed.failures.len(),
        parsed.refusals,
    );
    Ok(RunOut {
        rows,
        diag: Some(diag),
        end: if refused {
            RunEnd::Refused
        } else {
            RunEnd::Clean
        },
    })
}

/// Where a run's raw rows go.
fn tsv_path(scratch: &str, arm: &str, rnd: Round) -> String {
    let sep = if scratch.ends_with('/') { "" } else { "/" };
    format!("{scratch}{sep}keylat-{arm}-{}.tsv", rnd.0)
}

/// The raw rows, as a TSV for anyone who wants to look.
pub fn tsv_text(rows: &[Row]) -> String {
    let mut cols = vec![
        "seq",
        "app_seq",
        "char",
        "post",
        "event",
        "excluded",
        "exclude_reason",
    ];
    cols.extend(super::parse::OFFSETS);
    cols.push(super::parse::PREDOK);
    let mut o = cols.join("\t");
    o.push('\n');
    for r in rows {
        let mut cells = vec![
            r.seq.0.to_string(),
            r.app_seq.0.to_string(),
            r.ch.clone(),
            r.post.0.to_string(),
            r.event.0.to_string(),
            if r.excluded() { "True" } else { "False" }.to_string(),
            r.exclude_reason.clone().unwrap_or("None".to_string()),
        ];
        cells.extend(
            r.stamps
                .named()
                .map(|(_, v)| v.map_or("None".to_string(), |v| v.0.to_string())),
        );
        cells.push(r.predok.token().to_string());
        o += &cells.join("\t");
        o.push('\n');
    }
    o
}

/// The sweep's header; whether what it found is quotable.
fn print_header(args: &Args, arms: &[(String, String)], window: Ms) -> Quotable {
    println!("== keystroke-to-pixel sweep ==");
    let argv = ["pmset", "-g", "batt"].map(str::to_string);
    let pm: Vec<String> = match run_for(&argv, Duration::from_secs(5)) {
        Ok(p) => py::splitlines(&p.stdout)
            .into_iter()
            .take(2)
            .map(str::to_string)
            .collect(),
        Err(_) => vec!["pmset: unavailable".to_string()],
    };
    for line in &pm {
        println!("power: {}", py::strip(line));
    }
    let mut quotable = Quotable::Yes;
    if pm.iter().any(|l| l.contains("Battery Power")) || !pm.iter().any(|l| l.contains("AC Power"))
    {
        quotable = Quotable::No;
        println!(
            "power: NOT QUOTABLE -- on battery (or power source unknown); battery numbers are \
             not quotable"
        );
    }
    let helper = swift_argv(HELPER_SCRIPT, &["--info".to_string()]);
    match run_for(&helper, Duration::from_secs(60)) {
        Ok(ip) => {
            let refresh = py::splitlines(&ip.stdout)
                .into_iter()
                .find(|l| l.starts_with("refresh"))
                .unwrap_or("refresh unknown")
                .to_string();
            let failed = if ip.code == Some(0) {
                String::new()
            } else {
                format!(
                    " (helper --info exit {}: {})",
                    ip.code.unwrap_or(0),
                    py::strip(&ip.stderr)
                )
            };
            println!("display: {refresh}{failed}");
        }
        Err(e) => println!("display: refresh unknown ({e})"),
    }
    println!("swift: {}", swift_argv(HELPER_SCRIPT, &[]).join(" "));
    println!(
        "child: {} ({})",
        args.child.name(),
        child_env(args.child, &own_path()).unwrap_or("the login shell".to_string())
    );
    let (lo, hi) = gap_bounds(args.gap);
    let (lo_g, hi_g) = (py::format_g(lo), py::format_g(hi));
    println!(
        "gap: {} ms MEAN; each inter-key gap is uniform in [{lo_g}-{hi_g}] ms, seeded from \
         baud's pid (printed per round) -- a fixed gap phase-locks to the refresh and narrows \
         vsync/total falsely",
        args.gap.0
    );
    println!(
        "pairing window: {} ms (minimum jittered gap {lo_g} ms)",
        window.0
    );
    if lo < DEFAULT_WINDOW.0 as f64 {
        println!(
            "  minimum gap under {} ms: the window is that minimum - {} ms so a one-key shift \
             cannot fit inside it",
            DEFAULT_WINDOW.0, WINDOW_MARGIN.0
        );
    }
    println!(
        "A/B number: to_shown (post -> the surface on the layer; no display-period \
         quantisation). vsync/total are a bracket, not a measurement -- see the doc."
    );
    let mut shas: Vec<(String, String)> = Vec::new();
    for (name, path) in arms {
        let sha = sha256::of_file(Path::new(path)).unwrap_or_else(|e| format!("unreadable ({e})"));
        println!("arm {name}: {path} sha256={sha}");
        match shas.iter_mut().find(|(n, _)| n == name) {
            Some((_, have)) => *have = sha,
            None => shas.push((name.clone(), sha)),
        }
    }
    if args.aa == AaArm::Run {
        println!("arm aa is the first arm's binary again, byte-identical by design");
    }
    let sha_of = |name: &str| {
        shas.iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.as_str())
    };
    for (i, (a, _)) in arms.iter().enumerate() {
        for (b, _) in &arms[i + 1..] {
            if a == "aa" || b == "aa" {
                continue;
            }
            if sha_of(a) == sha_of(b) {
                println!(
                    "WARNING: arms {a} and {b} are byte-identical -- an A/B of one binary \
                     against itself measures the hour"
                );
            }
        }
    }
    println!("arm order rotates every round so no arm is always last");
    quotable
}

/// Whether a sweep's power source lets its numbers be quoted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Quotable {
    Yes,
    No,
}

/// `tempfile.gettempdir()`: the first of `$TMPDIR`, `$TEMP`, `$TMP`, the
/// usual directories and the working directory in which a file can be made.
pub fn gettempdir() -> String {
    let mut candidates: Vec<String> = ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .filter_map(|e| std::env::var(e).ok())
        .filter(|d| !d.is_empty())
        .collect();
    candidates.extend(["/tmp", "/var/tmp", "/usr/tmp"].map(str::to_string));
    candidates.push(".".to_string());
    let probe = format!(".key-latency-{}", std::process::id());
    for dir in candidates {
        let dir = py::abspath(&dir);
        let file = format!("{dir}/{probe}");
        if std::fs::File::create(&file).is_ok() {
            let _ = std::fs::remove_file(&file);
            return dir;
        }
    }
    py::abspath(".")
}

/// Each arm's value of something per round, in the order the arms first came.
struct PerArm<T>(Vec<(String, Vec<T>)>);

impl<T: Clone> PerArm<T> {
    fn new(arms: &[(String, String)]) -> Self {
        let mut out: Vec<(String, Vec<T>)> = Vec::new();
        for (name, _) in arms {
            if !out.iter().any(|(n, _)| n == name) {
                out.push((name.clone(), Vec::new()));
            }
        }
        PerArm(out)
    }

    fn of(&self, name: &str) -> &[T] {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map_or(&[], |(_, v)| v.as_slice())
    }

    fn push(&mut self, name: &str, value: T) {
        if let Some((_, v)) = self.0.iter_mut().find(|(n, _)| n == name) {
            v.push(value);
        }
    }
}

/// A sweep, to its exit code. An `Err` is a failure the driver cannot go on
/// from, for stderr and exit 1.
pub fn sweep(args: &Args) -> Result<i32, String> {
    let scratch = match &args.scratch {
        Some(s) => s.clone(),
        None => match std::env::var("BAUD_SCRATCH") {
            Ok(s) => s,
            Err(_) => gettempdir(),
        },
    };
    if scratch.is_empty() {
        return Err("the scratch directory is an empty path".to_string());
    }
    std::fs::create_dir_all(&scratch).map_err(|e| format!("{scratch}: {e}"))?;
    let mut arms: Vec<(String, String)> = args
        .arm
        .iter()
        .map(|a| {
            let (n, p) = a.split_once('=').expect("the arms were checked");
            (n.to_string(), p.to_string())
        })
        .collect();
    if args.aa == AaArm::Run {
        let first = arms[0].1.clone();
        arms.push(("aa".to_string(), first)); // the same binary again
    }
    let window = window_for_gap(args.gap);
    let quotable = print_header(args, &arms, window);

    let mut per_arm: PerArm<Vec<Row>> = PerArm::new(&arms);
    let mut per_arm_diag: PerArm<Option<Diag>> = PerArm::new(&arms);
    let mut refused_any = false;
    let mut rnd = 1;
    while rnd <= args.rounds {
        let round = Round(usize::try_from(rnd).map_err(|e| e.to_string())?);
        let order = rotate(&arms, round);
        let names: Vec<String> = order.iter().map(|(n, _)| n.clone()).collect();
        println!("\nround {rnd}: order {}", py::repr_list(&names));
        for (name, path) in &order {
            let res = run_arm(path, args, &scratch, round, name, window_ns(window))?;
            if res.end == RunEnd::Fatal {
                println!("\nthe helper failed; sweep aborted");
                return Ok(4);
            }
            refused_any = refused_any || res.end == RunEnd::Refused;
            per_arm.push(name, res.rows);
            per_arm_diag.push(name, res.diag);
        }
        rnd += 1;
    }

    // Round 1 is warm-up, discarded.
    let mut zero_runs: Vec<(String, usize)> = Vec::new();
    let mut zero_pooled = false;
    let mut summaries: Vec<(String, Summary)> = Vec::new();
    let mut warmup_used = false;
    let mut warmup_empty: Vec<String> = Vec::new();
    for (name, _) in &arms {
        let rounds = per_arm.of(name);
        let (settled, warm) = settled_rounds(rounds);
        warmup_used = warmup_used || warm == WarmUp::Used;
        if warmup_measured_nothing(rounds) {
            warmup_empty.push(name.clone());
        }
        let first = if warm == WarmUp::Used { 1 } else { 2 };
        for (i, rows) in settled.iter().enumerate() {
            if summarize(rows, None).completed == Count(0) {
                zero_runs.push((name.clone(), first + i));
            }
        }
        let pooled: Vec<Row> = settled.iter().flatten().cloned().collect();
        let sdiag = merge_diags(settled_rounds(per_arm_diag.of(name)).0);
        let summ = summarize(&pooled, sdiag.as_ref());
        print!("{}", summary_text(name, &summ, sdiag.as_ref()));
        if summ.completed == Count(0) {
            zero_pooled = true;
        }
        match summaries.iter_mut().find(|(n, _)| n == name) {
            Some((_, have)) => *have = summ,
            None => summaries.push((name.clone(), summ)),
        }
    }

    if warmup_used {
        println!("\nWARM-UP ROUND USED -- --rounds 1 has no settled round; not quotable");
    }
    if quotable == Quotable::No {
        println!("\nNOT QUOTABLE -- taken on battery (or an unknown power source)");
    }

    let summary_of = |name: &str| {
        summaries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s)
            .expect("every arm was summarised")
    };
    let base_name = arms[0].0.as_str();
    let floors = (args.aa == AaArm::Run).then(|| {
        let (b, a) = (summary_of(base_name), summary_of("aa"));
        print!("{}", aa_text(base_name, b, a));
        aa_floor(&b.legs, &a.legs)
    });
    for (other, _) in arms.iter().filter(|(n, _)| n != "aa" && n != base_name) {
        print!(
            "{}",
            deltas_text(
                base_name,
                other,
                summary_of(base_name),
                summary_of(other),
                floors.as_ref()
            )
        );
    }

    // The notes a reader must see last: a warm-up that measured nothing,
    // and any run whose landings were refused (NOT QUOTABLE, named).
    for name in &warmup_empty {
        println!("\narm {name}: round 1 measured nothing (warm-up, discarded)");
    }
    for (name, rnd, k) in refused_runs(&per_arm_diag.0) {
        if rnd == Round(1) && args.rounds > 1 {
            println!("\narm {name}: round 1 had {k} landing(s) refused (warm-up, discarded)");
        } else {
            println!(
                "\nNOT QUOTABLE -- landings refused: arm {name} round {} ({k}); its GPU-path \
                 rows with landed=- were excluded as refused-landing",
                rnd.0
            );
        }
    }

    if refused_any {
        return Ok(2);
    }
    for (name, rnd) in &zero_runs {
        println!("\nzero completed keys: arm {name} round {rnd}");
    }
    if !zero_runs.is_empty() || zero_pooled {
        println!("\nzero completed keys in a run -- the instrument reported nothing; not a pass.");
        return Ok(1);
    }
    Ok(0)
}
