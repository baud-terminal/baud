//! Sums up a `tools/scripts/linux-damage.sh` run: `<dir>` holds one folder a
//! compositor, each with the `present` bench's output.
//!
//! ```text
//! linux-damage <dir>
//! ```
//!
//! For each compositor and workload, each arm's `present` time -- the median
//! of the per-round medians, the first round dropped, and the range of those
//! medians -- with the bytes copied and the rows damaged a frame, the waits
//! for a buffer and the compositor's CPU time a frame. **A VM's frames run
//! at two speeds**, some four times apart, a whole run at a time and the
//! render with them, so a round is told by its render time, which is the
//! same work on both arms: at 0.8 or more of the workload's slowest round
//! it is slow, at 0.35 or less fast, and between, a run that changed speed,
//! counted and left out. Each speed is reported apart. Then the screen
//! checks: the pixels of the opaque runs that are not the frame (0 is
//! right), and whether the two arms' screens hash the same frame by frame,
//! opaque and at the default dim. Then the frame traffic of the opaque `rows`
//! run: which buffer each frame was attached to, whether one was used for
//! two frames running, and whether a buffer's release came before or after
//! the next frame's commit.
//!
//! Exits 1 when a check fails.

use std::collections::{BTreeSet, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use super::Done;
use super::text::{Count, Scan, fixed, float, is_digit, is_number, is_word, list};
use crate::key_latency::py;

/// The workloads, in the order they print; any other is a failure.
const WORKS: [&str; 4] = ["line", "walk", "scroll", "full"];

/// One `present` line, its fields as written.
#[derive(Clone, Debug)]
struct Timed {
    work: String,
    present: String,
    render: String,
    copied: String,
    damaged: String,
    waits: String,
    compositor: String,
}

/// `^(\w+)\s+frames (\d+) present_us ([\d.]+) \[([\d.]+)-([\d.]+)\] render_us
/// ([\d.]+) \[[\d.]+-[\d.]+\] copied_bytes/frame (\d+) damaged_rows/frame
/// (\d+) waits (\d+) compositor_ms/frame (\S+)`, and anything after.
fn timed_line(line: &str) -> Option<Timed> {
    let mut s = Scan(line);
    let work = s.some(is_word)?;
    s.some(py::is_space)?;
    s.lit("frames ")?;
    s.some(is_digit)?;
    s.lit(" present_us ")?;
    let present = s.some(is_number)?;
    s.lit(" [")?;
    s.some(is_number)?;
    s.lit("-")?;
    s.some(is_number)?;
    s.lit("] render_us ")?;
    let render = s.some(is_number)?;
    s.lit(" [")?;
    s.some(is_number)?;
    s.lit("-")?;
    s.some(is_number)?;
    s.lit("] copied_bytes/frame ")?;
    let copied = s.some(is_digit)?;
    s.lit(" damaged_rows/frame ")?;
    let damaged = s.some(is_digit)?;
    s.lit(" waits ")?;
    let waits = s.some(is_digit)?;
    s.lit(" compositor_ms/frame ")?;
    let compositor = s.some(|c| !py::is_space(c))?;
    Some(Timed {
        work: work.to_string(),
        present: present.to_string(),
        render: render.to_string(),
        copied: copied.to_string(),
        damaged: damaged.to_string(),
        waits: waits.to_string(),
        compositor: compositor.to_string(),
    })
}

/// One line of a screen check.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Shot {
    work: String,
    frame: i128,
    hash: String,
    differ: i128,
}

/// `^shot (\w+) (\d+) (\S+) differ (\d+) copied (\d+) damaged (\d+)`.
fn shot_line(line: &str) -> Option<Shot> {
    let mut s = Scan(line);
    s.lit("shot ")?;
    let work = s.some(is_word)?;
    s.lit(" ")?;
    let frame = s.some(is_digit)?;
    s.lit(" ")?;
    let hash = s.some(|c| !py::is_space(c))?;
    s.lit(" differ ")?;
    let differ = s.some(is_digit)?;
    s.lit(" copied ")?;
    s.some(is_digit)?;
    s.lit(" damaged ")?;
    s.some(is_digit)?;
    Some(Shot {
        work: work.to_string(),
        frame: py::int(frame)?,
        hash: hash.to_string(),
        differ: py::int(differ)?,
    })
}

/// A file as text, or why not: it must be UTF-8.
fn text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every `time-*-*.txt` but round 1's, by arm, the names in byte order, so
/// round 10 reads before round 2.
fn timed(folder: &Path) -> Result<Vec<(String, Timed)>, String> {
    let mut names: Vec<String> = Vec::new();
    let dir = fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for entry in dir {
        let entry = entry.map_err(|e| format!("{}: {e}", folder.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let mid = name
            .strip_prefix("time-")
            .and_then(|n| n.strip_suffix(".txt"));
        if mid.is_some_and(|m| m.contains('-')) {
            names.push(name);
        }
    }
    names.sort();
    let mut runs = Vec::new();
    for name in names {
        let stem = name.strip_suffix(".txt").unwrap_or(&name);
        let parts: Vec<&str> = stem.split('-').collect();
        let [_, round, arm] = parts[..] else {
            return Err(format!("{name}: not time-<round>-<arm>.txt"));
        };
        if round == "1" {
            continue;
        }
        for line in py::splitlines(&text(&folder.join(&name))?) {
            if let Some(m) = timed_line(line) {
                runs.push((arm.to_string(), m));
            }
        }
    }
    Ok(runs)
}

/// The screen checks in `path`, none when it is not there.
fn shots(path: &Path) -> Result<Vec<Shot>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(py::splitlines(&text(path)?)
        .into_iter()
        .filter_map(shot_line)
        .collect())
}

/// The frame traffic of one run.
#[derive(Debug, Default)]
struct Traffic {
    attached: Vec<String>,
    damage: Vec<i128>,
    again: Count,
    before_next: Count,
    after_next: Count,
}

/// `\.damage_buffer\((\d+), (\d+), (\d+), (\d+)\)`, its fourth number.
fn damage_height(line: &str) -> Option<i128> {
    const CALL: &str = ".damage_buffer(";
    line.match_indices(CALL).find_map(|(at, _)| {
        let mut s = Scan(line.get(at + CALL.len()..)?);
        s.some(is_digit)?;
        s.lit(", ")?;
        s.some(is_digit)?;
        s.lit(", ")?;
        s.some(is_digit)?;
        s.lit(", ")?;
        let height = s.some(is_digit)?;
        s.lit(")")?;
        py::int(height)
    })
}

/// The first `<before>(\d+)<after>` in `line`.
fn between<'a>(line: &'a str, before: &str, after: &str) -> Option<&'a str> {
    line.match_indices(before).find_map(|(at, _)| {
        let mut s = Scan(line.get(at + before.len()..)?);
        let n = s.some(is_digit)?;
        s.lit(after)?;
        Some(n)
    })
}

/// The attaches, and how the releases fell against the commits.
fn traffic(path: &Path) -> Result<Traffic, String> {
    let mut t = Traffic::default();
    if !path.exists() {
        return Ok(t);
    }
    let body = text(path)?;
    let mut releases: Vec<&str> = Vec::new();
    let mut held: Option<String> = None;
    for line in py::splitlines(&body) {
        if let Some(buffer) = between(line, ".attach(wl_buffer#", "") {
            t.attached.push(buffer.to_string());
            continue;
        }
        if let Some(height) = damage_height(line) {
            t.damage.push(height);
            continue;
        }
        let last = t.attached.last().filter(|_| line.ends_with(".commit()"));
        if let Some(last) = last {
            // The buffer of the frame before is released after this commit,
            // or was before it.
            match &held {
                Some(h) if h != last && releases.contains(&h.as_str()) => t.before_next.0 += 1,
                Some(h) if h != last => t.after_next.0 += 1,
                _ => {}
            }
            held = Some(last.clone());
            releases.clear();
            continue;
        }
        if let Some(buffer) = between(line, "wl_buffer#", ".release") {
            releases.push(buffer);
        }
    }
    t.again = Count(t.attached.windows(2).filter(|w| w[0] == w[1]).count());
    Ok(t)
}

/// `statistics.median`.
fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn smallest(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::INFINITY, f64::min)
}

fn largest(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

/// A field `float` reads, or the failure it is when it will not.
fn number(s: &str) -> Result<f64, String> {
    float(s).ok_or_else(|| format!("could not convert string to float: {}", py::repr_str(s)))
}

/// One compositor's folder. `failed` is set when a check fails.
fn folder_report(folder: &Path, out: &mut String, failed: &mut bool) -> Result<(), String> {
    let compositor_file = folder.join("compositor.txt");
    let comp = if compositor_file.exists() {
        py::strip(&text(&compositor_file)?).to_string()
    } else {
        "?".to_string()
    };
    let name = folder
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned());
    *out += &format!("== {name}: {comp}\n");
    let runs = timed(folder)?;
    let mut works: Vec<usize> = Vec::new();
    for (_, m) in &runs {
        let at = WORKS
            .iter()
            .position(|w| *w == m.work)
            .ok_or_else(|| format!("{} is not in the workloads", py::repr_str(&m.work)))?;
        if !works.contains(&at) {
            works.push(at);
        }
    }
    works.sort_unstable();
    for at in works {
        let work = WORKS[at];
        let mine: Vec<&(String, Timed)> = runs.iter().filter(|(_, m)| m.work == work).collect();
        let mut renders = Vec::with_capacity(mine.len());
        for (_, m) in &mine {
            renders.push(number(&m.render)?);
        }
        let slowest = largest(&renders);
        type Keep = fn(f64, f64) -> bool;
        let speeds: [(&str, Keep); 2] = [
            ("slow", |r, top| r >= 0.8 * top),
            ("fast", |r, top| r <= 0.35 * top),
        ];
        for (speed, keep) in speeds {
            for arm in ["whole", "rows"] {
                let every: Vec<&Timed> = mine
                    .iter()
                    .filter(|(a, _)| a == arm)
                    .map(|(_, m)| m)
                    .collect();
                let mut ms = Vec::new();
                let mut mixed = 0usize;
                for m in &every {
                    let r = number(&m.render)?;
                    if keep(r, slowest) {
                        ms.push(*m);
                    }
                    if 0.35 * slowest < r && r < 0.8 * slowest {
                        mixed += 1;
                    }
                }
                let Some(first) = ms.first() else {
                    continue;
                };
                let mut med = Vec::with_capacity(ms.len());
                for m in &ms {
                    med.push(number(&m.present)?);
                }
                let mut comp_ms = Vec::new();
                for m in &ms {
                    if m.compositor != "-" {
                        comp_ms.push(number(&m.compositor)?);
                    }
                }
                let cpu = if comp_ms.is_empty() {
                    "-".to_string()
                } else {
                    format!(
                        "{} [{}-{}]",
                        fixed(median(&comp_ms), 3),
                        fixed(smallest(&comp_ms), 3),
                        fixed(largest(&comp_ms), 3)
                    )
                };
                let mut waits = 0i128;
                for m in &ms {
                    waits += py::int(&m.waits).ok_or("a count past what this tool adds")?;
                }
                *out += &format!(
                    "  {work:6} {speed} {arm:5} present {:>6} us [{}-{}], {} rounds ({mixed} mixed); \
                     copied {} B/frame, damaged {} rows/frame, waits {waits}; compositor {cpu} ms/frame\n",
                    fixed(median(&med), 1),
                    fixed(smallest(&med), 1),
                    fixed(largest(&med), 1),
                    ms.len(),
                    first.copied,
                    first.damaged,
                );
            }
        }
    }
    for alpha in ["255", "dim"] {
        let a = shots(&folder.join(format!("shot-{alpha}-whole.txt")))?;
        let b = shots(&folder.join(format!("shot-{alpha}-rows.txt")))?;
        let key = |x: &Shot| (x.work.clone(), x.frame, x.hash.clone());
        let same = a.iter().map(key).eq(b.iter().map(key));
        let held = same && !a.is_empty();
        let mut line = format!(
            "  screens, page {alpha}: {} and {} shots, the arms' hashes {}",
            a.len(),
            b.len(),
            if held { "the same" } else { "DIFFER" }
        );
        if alpha == "255" {
            let whole: i128 = a.iter().map(|x| x.differ).sum();
            let rows: i128 = b.iter().map(|x| x.differ).sum();
            line += &format!("; pixels not the frame: whole {whole}, rows {rows}");
            if (whole, rows) != (0, 0) {
                *failed = true;
            }
        }
        if !held {
            *failed = true;
        }
        *out += &line;
        out.push('\n');
    }
    let t = traffic(&folder.join("trace-255-rows.txt"))?;
    let buffers: HashSet<&String> = t.attached.iter().collect();
    let heights: BTreeSet<i128> = t.damage.iter().copied().collect();
    let heights: Vec<i128> = heights.into_iter().collect();
    *out += &format!(
        "  traffic, opaque rows: {} attaches over {} buffers, {} onto the buffer just shown; \
         the last buffer released before the next commit {} times, after it {}; damage heights {}\n",
        t.attached.len(),
        buffers.len(),
        t.again.0,
        t.before_next.0,
        t.after_next.0,
        list(&heights),
    );
    Ok(())
}

/// The whole tool. Exits 1 when a check fails or a file will not read.
pub fn run(args: &[OsString]) -> Done {
    let mut out = String::new();
    let Some(top) = args.first() else {
        return Done::failed(out, "usage: linux-damage <dir>");
    };
    let top = Path::new(top);
    let entries = match fs::read_dir(top) {
        Ok(e) => e,
        Err(e) => return Done::failed(out, format!("{}: {e}", top.display())),
    };
    let mut folders = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) if e.path().is_dir() => folders.push(e.path()),
            Ok(_) => {}
            Err(e) => return Done::failed(out, format!("{}: {e}", top.display())),
        }
    }
    folders.sort();
    let mut failed = false;
    for folder in folders {
        if let Err(why) = folder_report(&folder, &mut out, &mut failed) {
            return Done::failed(out, why);
        }
    }
    Done::with(out, i32::from(failed))
}
