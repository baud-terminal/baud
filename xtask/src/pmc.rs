//! `cargo xtask pmc <export.xml>`: the twelve unnamed counter columns of an
//! `xctrace export` of a `CPU Counters` trace (`scripts/counter-controls/`).
//!
//! It aggregates **consecutive same-core** deltas only. The counters are
//! per-core and cumulative, so a thread migrating produces a delta that is
//! not work: taking max-minus-min across all samples implies an 8.34 GHz
//! clock, and this rule gives the machine's actual 4.5.
//!
//! **The reader is the export's own shape and nothing more.** Each `<row>`
//! names its time and core once by value, `id="7"`, and after that by
//! `ref="7"`; a row whose time or core cannot be resolved, or with no
//! `<pmc-events>`, is not a sample. It does not parse XML in general: the
//! export carries no entity or comment an integer could hide in, and a
//! reader that is exact about one element's spelling cannot read a `core`
//! the export did not write as one. ASCII digits only, which is all the
//! export writes.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};

use crate::features_since::Exit;
use crate::html::Count;
use crate::text;

/// The bytes after `prefix`, at each place it occurs.
fn after<'a>(hay: &'a str, prefix: &str) -> Vec<&'a str> {
    hay.match_indices(prefix)
        .map(|(at, _)| &hay[at + prefix.len()..])
        .collect()
}

fn digits(s: &str) -> Option<(&str, &str)> {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    (n > 0).then(|| s.split_at(n))
}

/// `<name id="(\d+)"[^>]*>(\d+)</name>`, the first one: its id and value.
pub fn defined<'a>(row: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let open = format!("<{name} id=\"");
    let close = format!("</{name}>");
    after(row, &open).into_iter().find_map(|rest| {
        let (id, rest) = digits(rest)?;
        let rest = rest.strip_prefix('"')?;
        let (_, rest) = rest.split_once('>')?;
        let (value, rest) = digits(rest)?;
        rest.starts_with(&close).then_some((id, value))
    })
}

/// `<name ref="(\d+)"/>`, the first one: its id.
pub fn referred<'a>(row: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name} ref=\"");
    after(row, &open).into_iter().find_map(|rest| {
        let (id, rest) = digits(rest)?;
        rest.starts_with("\"/>").then_some(id)
    })
}

/// `<pmc-events[^>]*>([\d ]+)</pmc-events>`, the first one: the counters.
pub fn events(row: &str) -> Option<Vec<i128>> {
    after(row, "<pmc-events").into_iter().find_map(|rest| {
        let (_, rest) = rest.split_once('>')?;
        let n = rest
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b' ')
            .count();
        if n == 0 || !rest[n..].starts_with("</pmc-events>") {
            return None;
        }
        rest[..n]
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(|w| w.parse().ok())
            .collect()
    })
}

/// The rows of the export: `<row>(.*?)</row>`.
pub fn rows(xml: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<row>") {
        let body = &rest[at + "<row>".len()..];
        let Some(end) = body.find("</row>") else {
            break;
        };
        out.push(&body[..end]);
        rest = &body[end + "</row>".len()..];
    }
    out
}

/// One row with its time and core resolved.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sample {
    pub time: i128,
    pub core: i128,
    pub counters: Vec<i128>,
}

fn number(s: &str) -> Result<i128, String> {
    s.parse().map_err(|e| format!("{s}: {e}"))
}

/// Resolves a row's value of one element, remembering what `id` it defines.
fn resolve(
    row: &str,
    name: &str,
    known: &mut HashMap<String, i128>,
) -> Result<Option<i128>, String> {
    if let Some((id, value)) = defined(row, name) {
        let value = number(value)?;
        known.insert(id.to_string(), value);
        return Ok(Some(value));
    }
    Ok(referred(row, name).and_then(|id| known.get(id).copied()))
}

/// Every sample of an export, in file order.
pub fn samples(xml: &str) -> Result<Vec<Sample>, String> {
    let (mut times, mut cores) = (HashMap::new(), HashMap::new());
    let mut out = Vec::new();
    for row in rows(xml) {
        let time = resolve(row, "sample-time", &mut times)?;
        let core = resolve(row, "core", &mut cores)?;
        let counters = events(row);
        if let (Some(time), Some(core), Some(counters)) = (time, core, counters) {
            out.push(Sample {
                time,
                core,
                counters,
            });
        }
    }
    Ok(out)
}

/// `f"{n:,}"`.
pub fn grouped(n: i128) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i != 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The sum over consecutive same-core samples: a pair is skipped when it
/// crosses cores, when any counter ran backwards, or when the gap is not in
/// (0, 5 ms]. What it returns is `(wall, totals)`.
pub fn aggregate(samples: &[Sample]) -> Result<(i128, Vec<i128>), String> {
    let columns = samples.first().map_or(0, |s| s.counters.len());
    let mut totals = vec![0; columns];
    let mut wall = 0;
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.core != b.core {
            continue;
        }
        let deltas: Vec<i128> = a
            .counters
            .iter()
            .zip(&b.counters)
            .map(|(x, y)| y - x)
            .collect();
        if deltas.iter().any(|d| *d < 0) {
            continue;
        }
        let dt = b.time - a.time;
        if dt <= 0 || dt > 5_000_000 {
            continue;
        }
        wall += dt;
        for (i, total) in totals.iter_mut().enumerate() {
            let Some(d) = deltas.get(i) else {
                return Err("list index out of range: a row with fewer columns".to_string());
            };
            *total += d;
        }
    }
    Ok((wall, totals))
}

/// `itertools.combinations(items, 4)`, in its order.
fn fours(items: &[usize]) -> Vec<[usize; 4]> {
    let mut out = Vec::new();
    for (i, a) in items.iter().enumerate() {
        for (j, b) in items.iter().enumerate().skip(i + 1) {
            for (k, c) in items.iter().enumerate().skip(j + 1) {
                for d in items.iter().skip(k + 1) {
                    out.push([*a, *b, *c, *d]);
                }
            }
        }
    }
    out
}

fn report(xml: &str, out: &mut impl Write) -> Result<(), Failure> {
    let samples = samples(xml)?;
    let Some(first) = samples.first() else {
        return Err("no counter samples".to_string().into());
    };
    let n = first.counters.len();
    writeln!(out, "{} counter samples, {n} columns", samples.len())?;
    let (wall, totals) = aggregate(&samples)?;
    let seconds = wall as f64 / 1e9;
    writeln!(
        out,
        "consecutive same-core wall time: {} s",
        crate::vt_table::fixed(seconds, Count(4))
    )?;
    for (i, v) in totals.iter().enumerate() {
        let rate = if wall == 0 { 0.0 } else { *v as f64 / seconds };
        writeln!(
            out,
            "  idx{i:<2} {:>16}   {:>8} G/s",
            grouped(*v),
            crate::vt_table::fixed(rate / 1e9, Count(3))
        )?;
    }
    writeln!(
        out,
        "\nadditive identities (target = sum of 4 others, within 2%):"
    )?;
    let candidates: Vec<usize> = (0..n).filter(|i| totals[*i] > 0).collect();
    for &target in &candidates {
        let others: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|i| *i != target)
            .collect();
        for combo in fours(&others) {
            let sum: i128 = combo.iter().map(|i| totals[*i]).sum();
            let t = totals[target];
            if ((sum - t).abs() as f64) / (t as f64) < 0.02 {
                let names: Vec<String> = combo.iter().map(|i| format!("idx{i}")).collect();
                let percent = (100 * (sum - t)) as f64 / t as f64;
                writeln!(
                    out,
                    "  idx{target} ~= {}   ({} vs {}, {percent:+.2}%)",
                    names.join(" + "),
                    grouped(sum),
                    grouped(t)
                )?;
            }
        }
    }
    Ok(())
}

enum Failure {
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

/// The report of an export's text.
pub fn run_text(xml: &str, out: &mut impl Write, err: &mut impl Write) -> io::Result<Exit> {
    match report(xml, out) {
        Ok(()) => Ok(Exit(0)),
        Err(Failure::Io(e)) => Err(e),
        Err(Failure::Crash(m)) => {
            writeln!(err, "pmc: {m}")?;
            Ok(Exit(1))
        }
    }
}

/// The command: the first word is the export, as the script read
/// `sys.argv[1]`, and an unreadable file is exit 1.
pub fn run(args: &[&str], out: &mut impl Write, err: &mut impl Write) -> io::Result<Exit> {
    let Some(path) = args.first() else {
        writeln!(err, "usage: cargo xtask pmc <export.xml>")?;
        return Ok(Exit(1));
    };
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(e) => {
            writeln!(err, "{path}: {e}")?;
            return Ok(Exit(1));
        }
    };
    match String::from_utf8(raw) {
        Ok(xml) => run_text(&text::universal(&xml), out, err),
        Err(e) => {
            writeln!(err, "{path}: {e}")?;
            Ok(Exit(1))
        }
    }
}
