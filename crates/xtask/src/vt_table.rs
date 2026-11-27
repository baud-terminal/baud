//! `cargo xtask vt-table <results> [<baseline>]`: the comparison table from
//! `tools/scripts/vt-compare.sh`'s results file.
//!
//! Per terminal and recording: the median throughput of the settled rounds
//! (the first round is discarded) in MiB/s -- mebibytes, the unit `ingest2`
//! prints -- with the range, and the terminal process's CPU seconds for the
//! run, with its startup subtracted when a baseline file -- the same runs at
//! zero replays -- is given. Every run is a fresh process, so a run's CPU
//! time is startup plus the work, and the baseline is startup plus nothing.
//!
//! **What a malformed row does is what the script did**: a number that
//! cannot be read, or a run of zero seconds where a rate is needed, stops it
//! with exit 1 and what it had already printed; a `cpu` field with no colon
//! is `nan`, and sorts where Python's `sorted` leaves it ([`sorted`]).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::Path;

use crate::features_since::Exit;
use crate::html::Count;
use crate::{py, text};

/// `float.__format__` with a fixed precision: Python spells what Rust's
/// `NaN` spells `nan`.
pub fn fixed(v: f64, places: Count) -> String {
    if v.is_nan() {
        "nan".to_string()
    } else if v.is_infinite() {
        if v > 0.0 { "inf" } else { "-inf" }.to_string()
    } else {
        format!("{:.*}", places.0, v)
    }
}

fn is_digit_at(s: &[u8], at: Option<usize>) -> bool {
    at.and_then(|i| s.get(i)).is_some_and(u8::is_ascii_digit)
}

/// Python takes `_` between two digits in a number and nothing else.
fn without_underscores(s: &str) -> Option<String> {
    let b = s.as_bytes();
    for (i, c) in b.iter().enumerate() {
        if *c == b'_' && !(is_digit_at(b, i.checked_sub(1)) && is_digit_at(b, i.checked_add(1))) {
            return None;
        }
    }
    Some(s.replace('_', ""))
}

/// `float(s)`.
pub fn float(s: &str) -> Result<f64, String> {
    without_underscores(s)
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| format!("could not convert string to float: {}", py::repr_str(s)))
}

/// `int(s)` where only its value through a float is read. Past a float's
/// range it is infinite here, and raises where the script first made it a
/// float ([`whole`]); reading it never raised, so a discarded round's total
/// does not stop a run. More digits than `sys.int_info`'s default limit did.
pub fn int(s: &str) -> Result<f64, String> {
    let bad = || {
        format!(
            "invalid literal for int() with base 10: {}",
            py::repr_str(s)
        )
    };
    let t = without_underscores(s).ok_or_else(bad)?;
    let digits = t.strip_prefix(['+', '-']).unwrap_or(&t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    if digits.len() > 4300 {
        return Err(bad());
    }
    t.parse().map_err(|_| bad())
}

/// An exact integer of the script's on its way to a float: the conversion
/// raises when it has no float, where an infinity here would print `inf`.
fn whole(v: f64) -> Result<f64, String> {
    if v.is_infinite() {
        Err("int too large to convert to float".to_string())
    } else {
        Ok(v)
    }
}

/// `top`'s `MM:SS.CC`, or `HH:MM:SS`, as seconds; `nan` from anything else.
pub fn seconds(t: &str) -> Result<f64, String> {
    let parts: Vec<&str> = t.split(':').collect();
    match parts[..] {
        [m, s] => {
            let (m, s) = (int(m)?, float(s)?);
            Ok(whole(m * 60.0)? + s)
        }
        [h, m, s] => {
            let (h, m, s) = (int(h)?, int(m)?, float(s)?);
            Ok(whole(h * 3600.0 + m * 60.0)? + s)
        }
        _ => Ok(f64::NAN),
    }
}

/// The one comparison Python's sort makes: false both ways when a `nan` is
/// in it, which is what makes the order the algorithm's.
fn less(a: &f64, b: &f64) -> bool {
    a < b
}

/// `count_run` of CPython 3.13's `listobject.c`, over the `<` Python
/// compares floats with: the run the list starts with, made ascending.
fn count_run(v: &mut [f64]) -> Count {
    let mut n = 1;
    while n < v.len() && !less(&v[n], &v[n - 1]) {
        n += 1;
    }
    if n == v.len() {
        return Count(n);
    }
    if n > 1 {
        if less(&v[0], &v[n - 1]) {
            return Count(n);
        }
        v[..n].reverse();
    }
    n += 1;
    let mut neq = 0;
    while n < v.len() {
        if less(&v[n], &v[n - 1]) {
            reverse_last(v, Count(n), &mut neq);
        } else if less(&v[n - 1], &v[n]) {
            break;
        } else {
            neq += 1;
        }
        n += 1;
    }
    reverse_last(v, Count(n), &mut neq);
    v[..n].reverse();
    while n < v.len() && !less(&v[n], &v[n - 1]) {
        n += 1;
    }
    Count(n)
}

fn reverse_last(v: &mut [f64], n: Count, neq: &mut usize) {
    if *neq != 0 {
        let len = *neq + 1;
        v[n.0 - len..n.0].reverse();
        *neq = 0;
    }
}

/// `sorted(v)`. A list with no `nan` is a total preorder, where any stable
/// sort gives Python's answer. With one, what `<` answers is no order, and
/// the answer is what the algorithm does: under 64 elements that is
/// `count_run` and then a binary insertion, ported here. Past 64 runs in one
/// cell, Python would merge runs and this keeps inserting; a results file
/// that long with a `nan` in it would differ.
pub fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    if !v.iter().any(|x| x.is_nan()) {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        return v;
    }
    let start = count_run(&mut v).0;
    for ok in start..v.len() {
        let pivot = v[ok];
        let (mut l, mut r) = (0, ok);
        while l < r {
            let m = (l + r) / 2;
            if less(&pivot, &v[m]) {
                r = m;
            } else {
                l = m + 1;
            }
        }
        v[l..=ok].rotate_right(1);
    }
    v
}

/// One line of the results file.
#[derive(Clone, Debug)]
struct Run {
    total: f64,
    wall: f64,
    cpu: f64,
    reply: String,
}

/// `defaultdict(list)`, in the order the keys first appear.
#[derive(Default)]
struct Rows(Vec<((String, String), Vec<Run>)>);

impl Rows {
    fn get(&self, term: &str, rec: &str) -> &[Run] {
        self.0
            .iter()
            .find(|((t, r), _)| t == term && r == rec)
            .map_or(&[], |(_, runs)| runs.as_slice())
    }

    fn push(&mut self, term: &str, rec: &str, run: Run) {
        if let Some((_, runs)) = self.0.iter_mut().find(|((t, r), _)| t == term && r == rec) {
            runs.push(run);
        } else {
            self.0
                .push(((term.to_string(), rec.to_string()), vec![run]));
        }
    }
}

type Meta = BTreeMap<String, (String, String)>;

fn load(path: &str) -> Result<(Rows, Meta), String> {
    let source = text::read_universal(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    let mut rows = Rows::default();
    let mut meta = Meta::new();
    for line in source.split_inclusive('\n') {
        let f = py::split(line);
        if f.len() < 8 {
            continue;
        }
        let [term, rec, size, total, t0, t1, cpu, comm] = f[..8] else {
            continue;
        };
        let reply = f.get(8).copied().unwrap_or("?").to_string();
        let total = int(total)?;
        let t1 = float(t1)?;
        let wall = t1 - float(t0)?;
        let cpu = seconds(cpu)?;
        rows.push(
            term,
            rec,
            Run {
                total,
                wall,
                cpu,
                reply,
            },
        );
        meta.insert(term.to_string(), (size.to_string(), comm.to_string()));
    }
    Ok((rows, meta))
}

/// `runs[1:] or runs`: the first round discarded, unless it is the only one.
fn settled(runs: &[Run]) -> &[Run] {
    if runs.len() > 1 { &runs[1..] } else { runs }
}

fn median(v: &[f64]) -> f64 {
    v[v.len() / 2]
}

fn header(out: &mut impl Write, terms: &[&str]) -> io::Result<()> {
    writeln!(out, "| recording | {} |", terms.join(" | "))?;
    writeln!(out, "|---|{}", "---|".repeat(terms.len()))
}

/// What stopped a run after some of it was printed.
pub enum Failure {
    Io(io::Error),
    Crash(String),
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Failure::Io(e)
    }
}

impl From<String> for Failure {
    fn from(s: String) -> Self {
        Failure::Crash(s)
    }
}

fn table(args: &[&str], out: &mut impl Write) -> Result<(), Failure> {
    let Some(results) = args.first() else {
        return Err(Failure::Crash(
            "usage: vt-table <results> [<baseline>]".to_string(),
        ));
    };
    let (rows, meta) = load(results)?;
    let mut base: BTreeMap<String, f64> = BTreeMap::new();
    if let Some(baseline) = args.get(1) {
        let (brows, _) = load(baseline)?;
        for ((term, _), runs) in &brows.0 {
            let cpus = sorted(settled(runs).iter().map(|r| r.cpu).collect());
            base.insert(term.clone(), median(&cpus));
        }
    }
    let term_set: BTreeSet<&str> = rows.0.iter().map(|((t, _), _)| t.as_str()).collect();
    let rec_set: BTreeSet<&str> = rows.0.iter().map(|((_, r), _)| r.as_str()).collect();
    let terms: Vec<&str> = term_set.into_iter().collect();
    let recs: Vec<&str> = rec_set.into_iter().collect();
    writeln!(
        out,
        "Throughput, MiB/s, median of the settled rounds [range]:"
    )?;
    writeln!(out)?;
    header(out, &terms)?;
    for rec in &recs {
        let mut cells = Vec::new();
        for term in &terms {
            let runs = settled(rows.get(term, rec));
            if runs.is_empty() {
                cells.push("--".to_string());
                continue;
            }
            let mut rates = Vec::with_capacity(runs.len());
            for r in runs {
                let total = whole(r.total)?;
                if r.wall == 0.0 {
                    return Err(Failure::Crash("float division by zero".to_string()));
                }
                rates.push(total / r.wall / f64::from(1 << 20));
            }
            let rates = sorted(rates);
            let w = Count(0);
            cells.push(format!(
                "**{}** [{}-{}]",
                fixed(median(&rates), w),
                fixed(rates[0], w),
                fixed(rates[rates.len() - 1], w)
            ));
        }
        writeln!(out, "| {rec} | {} |", cells.join(" | "))?;
    }
    writeln!(out)?;
    let label = if base.is_empty() {
        "CPU seconds at the end of the run, startup included"
    } else {
        "CPU seconds for the run, startup subtracted"
    };
    writeln!(out, "{label}, median of the settled rounds [range]:")?;
    writeln!(out)?;
    header(out, &terms)?;
    for rec in &recs {
        let mut cells = Vec::new();
        for term in &terms {
            let runs = settled(rows.get(term, rec));
            if runs.is_empty() {
                cells.push("--".to_string());
                continue;
            }
            let startup = base.get(*term).copied().unwrap_or(0.0);
            let cpus = sorted(runs.iter().map(|r| r.cpu - startup).collect());
            let w = Count(2);
            cells.push(format!(
                "**{}** [{}-{}]",
                fixed(median(&cpus), w),
                fixed(cpus[0], w),
                fixed(cpus[cpus.len() - 1], w)
            ));
        }
        writeln!(out, "| {rec} | {} |", cells.join(" | "))?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "| terminal | geometry the child saw | process measured | startup CPU, s |"
    )?;
    writeln!(out, "|---|---|---|---|")?;
    for term in &terms {
        let question = ("?".to_string(), "?".to_string());
        let (size, comm) = meta.get(*term).unwrap_or(&question);
        let startup = base.get(*term).copied().unwrap_or(f64::NAN);
        writeln!(
            out,
            "| {term} | {size} | {comm} | {} |",
            fixed(startup, Count(2))
        )?;
    }
    let bad: Vec<String> = rows
        .0
        .iter()
        .flat_map(|((t, r), runs)| {
            runs.iter()
                .filter(|run| !run.reply.ends_with("52"))
                .map(move |_| format!("{t}/{r}"))
        })
        .collect();
    if !bad.is_empty() {
        writeln!(out)?;
        let shown: Vec<&str> = bad.iter().take(8).map(String::as_str).collect();
        writeln!(
            out,
            "{} runs did not end in a cursor report and are not results: {}",
            bad.len(),
            shown.join(", ")
        )?;
    }
    Ok(())
}

/// The command: exit 0, or exit 1 and what stopped it, as an uncaught
/// exception did, after whatever was already printed.
pub fn run(args: &[&str], out: &mut impl Write, err: &mut impl Write) -> io::Result<Exit> {
    match table(args, out) {
        Ok(()) => Ok(Exit(0)),
        Err(Failure::Io(e)) => Err(e),
        Err(Failure::Crash(m)) => {
            writeln!(err, "vt-table: {m}")?;
            Ok(Exit(1))
        }
    }
}
