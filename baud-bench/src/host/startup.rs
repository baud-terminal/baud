//! Launches a binary repeatedly and reports the wall from the spawn to its
//! first presented frame, with the in-app stages: one arm, or several arms
//! interleaved with an A/A copy, which is how a launch change is kept or
//! reverted.
//!
//! The app prints `startup: <stage> <ms>` lines under `BAUD_STARTUP=1`,
//! timed from its own `main`; this times from the spawn, so the gap between
//! the two is the loader's share. Each launch runs `BAUD_CMD` (a sleep, so
//! the window is not closed by the child exiting before it has presented)
//! at a fixed grid, and is stopped once the stage the wall is taken to has
//! arrived.
//!
//! With `--arm`, every round launches each arm once in an order that
//! rotates round by round, and `--aa` adds a second launch of the first
//! arm's binary under the name `aa`: the same program measured twice in the
//! same sweep, whose spread is the noise floor a difference between arms
//! has to clear. The first round is discarded, the way the first round of
//! any sweep here is. A binary may be the executable inside an `.app`
//! bundle (`<bundle>.app/Contents/MacOS/baud`): `NSBundle` resolves the
//! bundle from the executable's path, so that is how a bundle is measured.
//!
//! Per arm the report gives the median and [min-max] of the wall and of
//! every in-app stage that appeared in all of the arm's settled launches,
//! so a stage one arm does not print (a Dock icon it no longer sets) is
//! absent from that arm rather than a zero. With two or more arms it then
//! prints, for every stage both arms carry, arm minus base beside the A/A
//! spread of the same stage: a difference inside that spread is not a
//! result. Two stages at one median are listed by name.
//!
//! Take it on mains, on an idle box, and read the range as well as the
//! median: the first launch after a build pays the loader's cold cache and
//! is reported but excluded from the median.
//!
//! Exits 0 when measured, and 1 when a launch never reached the stage or
//! the usage is wrong.

use super::{Env, Host, Keyed, Launch, highest, lowest, py, rotated, upper_median, var};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::{self, Write};

/// The arguments, for the usage line.
pub const USAGE: &str = "\
usage: startup-bench [runs=7] <binary>
       startup-bench --rounds 7 --arm base=<binary> --arm new=<binary> [--aa]

env:   BAUD_BENCH_STAGE   the line the wall is taken to: `first completion|first
                          present` by default, or `first content` (the first
                          frame after the child wrote)
       BAUD_CMD           the child: `shell` runs the login shell the app would
                          run; unset is `sleep 5`
       BAUD_GRID          the grid, `200x50` by default";

/// The line the wall is taken to, by default.
const DEFAULT_STAGE: &str = "first completion|first present";

/// `(name, binary)`.
type Arm = (String, String);
/// One launch: the wall and each stage's own clock.
type Launched = (f64, BTreeMap<String, f64>);

/// What one launch read: the wall if the stage came, each stage's own clock,
/// and every `startup:` line.
type Reading = (Option<f64>, BTreeMap<String, f64>, Vec<String>);

/// Whether `line` is the stage the wall is taken to: `startup: `, one of
/// `stages`, and white space.
pub fn reaches(line: &str, stages: &[&str]) -> bool {
    let Some(rest) = line.strip_prefix("startup: ") else {
        return false;
    };
    stages.iter().any(|s| {
        rest.strip_prefix(s)
            .and_then(|after| after.chars().next())
            .is_some_and(py::is_space)
    })
}

/// `startup: <stage> <value> ms`: the stage and the value's text. The value
/// is the digits and dots the line ends on, and white space divides it from
/// a stage of at least one character.
pub fn mark(line: &str) -> Option<(&str, &str)> {
    let body = line.strip_prefix("startup: ")?.strip_suffix(" ms")?;
    let head = body.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    let value = &body[head.len()..];
    if value.is_empty() {
        return None;
    }
    let name = head.trim_end_matches(py::is_space);
    if name.len() == head.len() {
        return None;
    }
    if !name.is_empty() {
        return Some((name, value));
    }
    // All white space: the stage is its first character, if one is left over.
    let first = head.chars().next()?.len_utf8();
    (head.len() > first).then(|| (&head[..first], value))
}

fn parse_args(args: &[String]) -> Result<(i128, Vec<Arm>), String> {
    let (mut rounds, mut arms, mut aa) = (7, Vec::new(), false);
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--rounds" => {
                let v = it.next().ok_or("--rounds takes a count")?;
                rounds = py::int(v).ok_or_else(|| format!("--rounds: {v}"))?;
            }
            "--arm" => {
                let v = it.next().ok_or("--arm takes name=path")?;
                match v.split_once('=') {
                    Some((name, path)) if !path.is_empty() => {
                        arms.push((name.to_string(), path.to_string()));
                    }
                    _ => return Err("--arm takes name=path".into()),
                }
            }
            "--aa" => aa = true,
            a if a.starts_with('-') => return Err(USAGE.into()),
            a => positional.push(a),
        }
    }
    if let Some(first) = positional.first() {
        rounds = py::int(first).ok_or_else(|| format!("runs: {first}"))?;
    }
    if arms.is_empty() {
        let binary = positional.get(1).ok_or(USAGE)?;
        arms.push(("base".to_string(), (*binary).to_string()));
    }
    if aa {
        let first = arms[0].1.clone();
        arms.push(("aa".to_string(), first));
    }
    Ok((rounds, arms))
}

/// `env` as a launch has it: `BAUD_STARTUP`, a grid and a child.
fn child_env(env: &Env) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = env
        .iter()
        .filter(|(k, _)| k != "BAUD_STARTUP")
        .cloned()
        .collect();
    out.push(("BAUD_STARTUP".into(), "1".into()));
    if var(env, "BAUD_GRID").is_none() {
        out.push(("BAUD_GRID".into(), "200x50".into()));
    }
    // `BAUD_CMD=shell` runs the login shell the app would run, for the wall
    // to `first content`; anything else is the child, and unset is `sleep 5`.
    match var(env, "BAUD_CMD").as_deref() {
        Some("shell") => out.retain(|(k, _)| k != "BAUD_CMD"),
        Some(_) => {}
        None => out.push(("BAUD_CMD".into(), "sleep 5".into())),
    }
    out
}

/// One launch: the wall to the stage if it came, every stage's own clock,
/// and every `startup:` line.
fn launch(
    binary: &str,
    env: &Env,
    stages_wanted: &[&str],
    host: &mut dyn Host,
) -> io::Result<Reading> {
    let t0 = host.now();
    let mut running = Launch::start(binary, env)?;
    let mut stages = BTreeMap::new();
    let mut lines = Vec::new();
    let mut wall = None;
    loop {
        let line = match running.next_line() {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                running.stop();
                return Err(e);
            }
        };
        let line = line.strip_suffix('\n').unwrap_or(&line);
        if line.starts_with("startup:") {
            lines.push(line.to_string());
            // A note (`startup: walk: 0 map misses, ... 0.0 ms`) is not a
            // stage: it carries a colon, and a stage name never does.
            if let Some((name, value)) = mark(line) {
                if !name.contains(':') && !stages.contains_key(name) {
                    let Some(ms) = py::float(value) else {
                        running.stop();
                        return Err(io::Error::other(format!("not a number: {value}")));
                    };
                    stages.insert(name.to_string(), ms);
                }
            }
        }
        if reaches(line, stages_wanted) {
            wall = Some((host.now() - t0) * 1e3);
            break;
        }
    }
    running.stop();
    Ok((wall, stages, lines))
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
    let (rounds, arms) = parse_args(args)?;
    let stage = var(env, "BAUD_BENCH_STAGE").unwrap_or_else(|| DEFAULT_STAGE.to_string());
    let wanted: Vec<&str> = stage.split('|').collect();
    let launch_env = child_env(env);
    let mut results = Keyed::<Vec<Launched>>::of(arms.iter().map(|(n, _)| n.as_str()));
    let mut last_lines = Keyed::<Vec<String>>::of(arms.iter().map(|(n, _)| n.as_str()));
    for rnd in 0..usize::try_from(rounds).unwrap_or(0) {
        for (name, binary) in rotated(&arms, rnd) {
            let (wall, stages, lines) = launch(&binary, &launch_env, &wanted, host)?;
            let Some(wall) = wall else {
                writeln!(
                    out,
                    "round {rnd} arm {name}: never reached the stage; stages seen: {}",
                    py::repr_list(&lines)
                )?;
                return Ok(1);
            };
            if let Some(r) = results.get_mut(&name) {
                r.push((wall, stages));
            }
            if let Some(l) = last_lines.get_mut(&name) {
                *l = lines;
            }
            let tag = if rnd == 0 { "  (discarded)" } else { "" };
            writeln!(
                out,
                "round {rnd} arm {name:5}: {wall:8.1} ms to the stage{tag}"
            )?;
            host.pause(0.5);
        }
    }
    report(rounds, &arms, &results, &last_lines, out)
}

/// A settled arm: its walls, and each stage all its launches printed.
type Summary = (Vec<f64>, BTreeMap<String, Vec<f64>>);

fn report(
    rounds: i128,
    arms: &[Arm],
    results: &Keyed<Vec<Launched>>,
    last_lines: &Keyed<Vec<String>>,
    out: &mut dyn Write,
) -> Result<i32, Box<dyn std::error::Error>> {
    writeln!(out)?;
    if let [(name, _)] = arms {
        if rounds <= 0 {
            return Err("no launch to report".into());
        }
        for line in last_lines.get(name).into_iter().flatten() {
            writeln!(out, "{line}")?;
        }
        writeln!(out)?;
    }
    let mut summary = Keyed::<Summary>::of(arms.iter().map(|(n, _)| n.as_str()));
    for (name, _) in arms {
        let rs = results.get(name).map_or(&[][..], Vec::as_slice);
        let rs = if rs.len() > 1 { &rs[1..] } else { rs };
        let walls: Vec<f64> = rs.iter().map(|(w, _)| *w).collect();
        let per_stage: BTreeMap<String, Vec<f64>> = rs
            .first()
            .map(|(_, s)| s.keys())
            .into_iter()
            .flatten()
            .filter(|st| rs.iter().all(|(_, s)| s.contains_key(*st)))
            .map(|st| (st.clone(), rs.iter().map(|(_, s)| s[st]).collect()))
            .collect();
        writeln!(out, "== arm {name} ({} settled launches) ==", rs.len())?;
        let middle = upper_median(&walls).ok_or("no settled launch")?;
        writeln!(
            out,
            "  wall to the stage: median {middle:.1} ms, range [{:.1}-{:.1}]",
            lowest(&walls),
            highest(&walls)
        )?;
        let mut order: Vec<(&String, &Vec<f64>)> = per_stage.iter().collect();
        order.sort_by(|a, b| median(a.1).total_cmp(&median(b.1)));
        for (st, v) in order {
            writeln!(
                out,
                "  {st:20} median {:8.1} ms  [{:7.1}-{:7.1}]",
                median(v),
                lowest(v),
                highest(v)
            )?;
        }
        writeln!(out)?;
        if let Some(s) = summary.get_mut(name) {
            *s = (walls, per_stage);
        }
    }
    if arms.len() > 1 {
        against(arms, &summary, out)?;
    }
    Ok(0)
}

/// The upper median of a list known not to be empty.
fn median(v: &[f64]) -> f64 {
    upper_median(v).unwrap_or(f64::NAN)
}

/// Each arm against the first, beside the A/A spread.
fn against(arms: &[Arm], summary: &Keyed<Summary>, out: &mut dyn Write) -> io::Result<()> {
    let base_name = &arms[0].0;
    let Some((base_walls, base_stages)) = summary.get(base_name) else {
        return Ok(());
    };
    let aa = summary.get("aa");
    writeln!(
        out,
        "== against {base_name}, per stage: arm - base, beside the A/A spread =="
    )?;
    for (name, _) in &arms[1..] {
        if name == "aa" {
            continue;
        }
        let Some((walls, stages)) = summary.get(name) else {
            continue;
        };
        let d = median(walls) - median(base_walls);
        let mut floor = String::new();
        if let Some((aw, _)) = aa {
            let spread = (median(aw) - median(base_walls))
                .abs()
                .max(highest(aw) - lowest(aw))
                .max(highest(base_walls) - lowest(base_walls));
            let apart = highest(walls) < lowest(base_walls) || lowest(walls) > highest(base_walls);
            let verdict = if d.abs() > spread && apart {
                "result"
            } else {
                "inside the A/A spread -- not a result"
            };
            floor = format!("  (A/A spread {spread:.1} ms: {verdict})");
        }
        writeln!(out, "  {name}: wall {d:+.1} ms{floor}")?;
        let theirs: BTreeSet<&String> = stages.keys().collect();
        let ours: BTreeSet<&String> = base_stages.keys().collect();
        for st in theirs.intersection(&ours) {
            let ds = median(&stages[*st]) - median(&base_stages[*st]);
            let fl = aa
                .and_then(|(_, a)| a.get(*st))
                .map(|av| {
                    format!(
                        "  (A/A {:.1})",
                        (median(av) - median(&base_stages[*st])).abs()
                    )
                })
                .unwrap_or_default();
            writeln!(out, "    {st:20} {ds:+8.1} ms{fl}")?;
        }
        let gone: Vec<&str> = ours.difference(&theirs).map(|s| s.as_str()).collect();
        let new: Vec<&str> = theirs.difference(&ours).map(|s| s.as_str()).collect();
        if !gone.is_empty() {
            writeln!(
                out,
                "    stages the arm no longer prints: {}",
                gone.join(", ")
            )?;
        }
        if !new.is_empty() {
            writeln!(out, "    stages new to the arm: {}", new.join(", "))?;
        }
    }
    Ok(())
}
