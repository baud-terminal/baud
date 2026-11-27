//! How the app's frames reached the screen, from a compositor's
//! `WAYLAND_DEBUG=server` log of one session (`tools/scripts/linux-frames.sh` drives
//! it): what each commit on the window's surface carried and what the
//! compositor's presentation feedback said of it.
//!
//! ```text
//! linux-frames <traffic.log> <compositor.txt> <on|off>
//! ```
//!
//! `compositor.txt` is wayland-info's lines for the four globals, which say
//! what the compositor offered; on or off is the session's `[window]
//! tearing`. libwayland prints a message as `[ms] -> iface#id.name(args)`
//! for an event and without the arrow for a request (`@` in place of `#`
//! before 1.22), the time in milliseconds or, from weston, as a time of
//! day. The app is the one client in the log.
//!
//! The checks, each "ok" or "FAILED":
//!
//! - presentation feedback arrives, and where the compositor predicts a
//!   refresh, every `presented` carries it;
//! - where fifo-v1 is offered and tearing is off, every commit that
//!   attaches a buffer sets and waits on the barrier and no other commit
//!   does, and no frame is discarded -- a picture the barrier holds is
//!   never replaced unseen; where it is not, or tearing is on, none does;
//! - where commit-timing-v1 is offered and tearing is off, at least one
//!   frame is stamped, and every stamp is half a refresh before a whole
//!   number of refreshes after a `presented` time the app had been sent
//!   (exactly, in nanoseconds): the app's frame clock is the compositor's
//!   feedback; where it is not, or tearing is on, nothing is stamped;
//! - every stamped frame is presented no earlier than its stamp;
//! - where tearing-control-v1 is offered, tearing on makes one control,
//!   hinted async before the window's first frame, and tearing off none.
//!
//! It prints, besides, how many refreshes after its refresh each stamped
//! frame was shown and how many refreshes apart the frames presented were.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use super::Done;
use super::text::{Count, Scan, is_digit, is_word, list, message_timed, round};
use crate::key_latency::py;

/// The tearing control's `async` hint.
const ASYNC: i128 = 1;

/// `(?:new id )?(\w+)[@#](\d+)`, everywhere in `args`: (interface, number).
fn ids(args: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut s = Scan(args);
    while !s.0.is_empty() {
        s.any(|c| !is_word(c));
        let Some(iface) = s.some(is_word) else {
            break;
        };
        let mut after = s;
        if after.lit("@").is_none() && after.lit("#").is_none() {
            continue;
        }
        if let Some(id) = after.some(is_digit) {
            found.push((iface, id));
            s = after;
        }
    }
    found
}

/// The second object's number in `args`, or the failure its absence is.
fn second_id(args: &str) -> Result<&str, String> {
    ids(args)
        .get(1)
        .map(|i| i.1)
        .ok_or_else(|| format!("no second object in {args}"))
}

/// The numbers among `args`, split on `, `: those that are digits alone.
fn nums(args: &str) -> Result<Vec<i128>, String> {
    let mut out = Vec::new();
    for a in args.split(", ") {
        let bare = py::strip(a);
        if !bare.is_empty() && bare.chars().all(is_digit) {
            out.push(py::int(bare).ok_or("a number past what this tool holds")?);
        }
    }
    Ok(out)
}

/// What the compositor said of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// No word yet.
    Unknown,
    /// Presented at this time, this refresh, in nanoseconds.
    At(i128, i128),
    /// Discarded.
    Discarded,
}

/// Whether a commit carried something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Has {
    No,
    Yes,
}

impl From<bool> for Has {
    fn from(b: bool) -> Has {
        if b { Has::Yes } else { Has::No }
    }
}

/// What a commit on the window's surface carried.
#[derive(Clone, Debug)]
struct Commit<'a> {
    attach: Has,
    feedback: Option<&'a str>,
    set_barrier: Has,
    wait_barrier: Has,
    stamp: Option<i128>,
    known: Vec<(i128, i128)>,
    shown: Shown,
}

impl Commit<'_> {
    fn new() -> Self {
        Commit {
            attach: Has::No,
            feedback: None,
            set_barrier: Has::No,
            wait_barrier: Has::No,
            stamp: None,
            known: Vec::new(),
            shown: Shown::Unknown,
        }
    }
}

/// A presentation time from `tv_sec_hi`, `tv_sec_lo`, `tv_nsec`.
fn time_of(hi: i128, lo: i128, ns: i128) -> Result<i128, String> {
    let secs = hi
        .checked_mul(1 << 32)
        .map(|h| h | lo)
        .and_then(|s| s.checked_mul(1_000_000_000))
        .and_then(|t| t.checked_add(ns));
    secs.ok_or_else(|| "a presentation time past what this tool holds".to_string())
}

/// One log and what the checks need of it.
#[derive(Debug)]
struct Read<'a> {
    commits: Vec<Commit<'a>>,
    presented: Vec<(i128, i128)>,
    controls: Count,
    hints: Vec<(i128, usize)>,
}

fn read_log(text: &str) -> Result<Read<'_>, String> {
    let mut window: Option<&str> = None;
    let mut pending = Commit::new();
    let mut r = Read {
        commits: Vec::new(),
        presented: Vec::new(),
        controls: Count(0),
        hints: Vec::new(),
    };
    let mut asked: HashMap<&str, usize> = HashMap::new();
    for line in py::universal_lines(text) {
        let Some(m) = message_timed(line) else {
            continue;
        };
        if m.event {
            let owned = (m.iface == "wp_presentation_feedback" && m.name != "sync_output")
                .then(|| asked.remove(m.id))
                .flatten();
            if let Some(at) = owned {
                if m.name == "presented" {
                    let n = nums(m.args)?;
                    let [hi, lo, ns, refresh, ..] = n[..] else {
                        return Err("presented with fewer than four numbers".to_string());
                    };
                    let t = time_of(hi, lo, ns)?;
                    r.commits[at].shown = Shown::At(t, refresh);
                    r.presented.push((t, refresh));
                } else if m.name == "discarded" {
                    r.commits[at].shown = Shown::Discarded;
                }
            }
            continue;
        }
        match (m.iface, m.name) {
            ("xdg_wm_base", "get_xdg_surface") if window.is_none() => {
                window = Some(second_id(m.args)?);
            }
            ("wl_surface", "attach") if Some(m.id) == window => {
                pending.attach = (!m.args.starts_with("nil")).into();
            }
            ("wp_presentation", "feedback") => {
                pending.feedback = Some(second_id(m.args)?);
            }
            ("wp_fifo_v1", "set_barrier") => pending.set_barrier = Has::Yes,
            ("wp_fifo_v1", "wait_barrier") => pending.wait_barrier = Has::Yes,
            ("wp_commit_timer_v1", "set_timestamp") => {
                let n = nums(m.args)?;
                let [hi, lo, ns] = n[..] else {
                    return Err("set_timestamp without three numbers".to_string());
                };
                pending.stamp = Some(time_of(hi, lo, ns)?);
                let from = r.presented.len().saturating_sub(3);
                pending.known = r.presented[from..].to_vec();
            }
            ("wp_tearing_control_manager_v1", "get_tearing_control") => r.controls.0 += 1,
            ("wp_tearing_control_v1", "set_presentation_hint") => {
                let n = nums(m.args)?;
                let hint = *n.first().ok_or("a hint with no number")?;
                r.hints.push((hint, r.commits.len()));
            }
            ("wl_surface", "commit") if Some(m.id) == window => {
                if let Some(feedback) = pending.feedback {
                    asked.insert(feedback, r.commits.len());
                }
                r.commits
                    .push(std::mem::replace(&mut pending, Commit::new()));
            }
            _ => {}
        }
    }
    Ok(r)
}

/// `{'ok' if ok else 'FAILED':8}{name}: {said}`, counting a failure.
fn check(out: &mut String, failed: &mut usize, name: &str, ok: bool, said: &str) {
    let word = if ok { "ok" } else { "FAILED" };
    *out += &format!("{word:8}{name}: {said}\n");
    if !ok {
        *failed += 1;
    }
}

/// `round(a / b)`, `b` not zero.
fn refreshes_in(a: i128, b: i128) -> Result<i128, String> {
    // Both are differences of nanosecond times, well inside a double's whole
    // numbers, so the quotient rounds as Python's integer division does.
    round(a as f64 / b as f64).ok_or_else(|| "a quotient with no integer".to_string())
}

/// `key: count, ...`, the keys in order.
fn counts(c: &BTreeMap<i128, usize>) -> String {
    let parts: Vec<String> = c.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    parts.join(", ")
}

fn report(text: &str, offered: &str, tearing: &str, out: &mut String) -> Result<usize, String> {
    let fifo = offered.contains("wp_fifo_manager_v1");
    let timing = offered.contains("wp_commit_timing_manager_v1");
    let tear = offered.contains("wp_tearing_control_manager_v1");
    let torn = tear && tearing == "on";
    let Read {
        commits,
        presented,
        controls: Count(controls),
        hints,
    } = read_log(text)?;
    let mut failed = 0;

    let frames: Vec<&Commit> = commits.iter().filter(|c| c.feedback.is_some()).collect();
    let shown = frames
        .iter()
        .filter(|c| matches!(c.shown, Shown::At(..)))
        .count();
    let discarded = frames
        .iter()
        .filter(|c| c.shown == Shown::Discarded)
        .count();
    let stamped: Vec<&Commit> = commits.iter().filter(|c| c.stamp.is_some()).collect();
    let buffers = frames.iter().filter(|c| c.attach == Has::Yes).count();
    *out += &format!(
        "        {} commits on the window's surface, {} asking feedback, {buffers} with a buffer; \
         {shown} presented, {discarded} discarded, {} stamped\n",
        commits.len(),
        frames.len(),
        stamped.len()
    );

    let refreshes: BTreeSet<i128> = presented.iter().map(|p| p.1).collect();
    let refreshes = list(&refreshes.into_iter().collect::<Vec<_>>());
    check(
        out,
        &mut failed,
        "presentation feedback arrives",
        presented.len() >= 10,
        &format!("{} presented, refresh {refreshes} ns", presented.len()),
    );
    if timing {
        check(
            out,
            &mut failed,
            "every presented frame says the refresh",
            !presented.is_empty() && presented.iter().all(|p| p.1 > 0),
            &format!("refresh {refreshes} ns"),
        );
    }

    let barred = commits
        .iter()
        .filter(|c| c.set_barrier == Has::Yes || c.wait_barrier == Has::Yes)
        .count();
    if fifo && !torn {
        let wrong = commits
            .iter()
            .filter(|c| c.set_barrier != c.attach || c.wait_barrier != c.attach)
            .count();
        check(
            out,
            &mut failed,
            "every commit with a buffer sets and waits on the barrier, and only those",
            wrong == 0 && barred > 0,
            &format!("{barred} with the barrier, {wrong} wrong"),
        );
        check(
            out,
            &mut failed,
            "no frame is discarded",
            discarded == 0,
            &format!("{discarded} discarded"),
        );
    } else {
        check(
            out,
            &mut failed,
            "no commit carries the barrier",
            barred == 0,
            &format!("{barred} with it"),
        );
    }

    let stamp_of = |c: &Commit| c.stamp.unwrap_or_default();
    if timing && !torn {
        let off = stamped
            .iter()
            .filter(|c| {
                !c.known.iter().any(|&(p, r)| {
                    let at = stamp_of(c) + r.div_euclid(2) - p;
                    r != 0 && at > 0 && at.rem_euclid(r) == 0
                })
            })
            .count();
        check(
            out,
            &mut failed,
            "every stamp is half a refresh before a refresh counted from the feedback",
            !stamped.is_empty() && off == 0,
            &format!("{} stamped, {off} off the feedback's grid", stamped.len()),
        );
    } else {
        check(
            out,
            &mut failed,
            "no frame is stamped",
            stamped.is_empty(),
            &format!("{} stamped", stamped.len()),
        );
    }

    let early = stamped
        .iter()
        .filter(|c| matches!(c.shown, Shown::At(t, _) if t < stamp_of(c)))
        .count();
    check(
        out,
        &mut failed,
        "no stamped frame is shown before its stamp",
        early == 0,
        &format!("{early} early"),
    );

    if tear {
        if torn {
            let first = commits.iter().position(|c| c.feedback.is_some());
            let ok = controls == 1
                && hints.first().is_some_and(|h| h.0 == ASYNC)
                && first.is_some_and(|f| hints.first().is_some_and(|h| h.1 <= f));
            let told: Vec<i128> = hints.iter().map(|h| h.0).collect();
            check(
                out,
                &mut failed,
                "tearing on: one control, hinted async before the first frame",
                ok,
                &format!("{controls} controls, hints {}", list(&told)),
            );
        } else {
            check(
                out,
                &mut failed,
                "tearing off: no control",
                controls == 0 && hints.is_empty(),
                &format!("{controls} controls"),
            );
        }
    }

    if (!fifo || torn) && discarded > 0 {
        *out += &format!("        {discarded} discarded, with no barrier to hold them\n");
    }
    let mut late: BTreeMap<i128, usize> = BTreeMap::new();
    for c in &stamped {
        if let Shown::At(t, r) = c.shown {
            if r == 0 {
                return Err("a stamped frame presented with no refresh".to_string());
            }
            let n = refreshes_in(t - (stamp_of(c) + r.div_euclid(2)), r)?;
            *late.entry(n).or_default() += 1;
        }
    }
    if !late.is_empty() {
        *out += &format!(
            "        stamped frames shown n refreshes after their refresh: {}\n",
            counts(&late)
        );
    }
    let mut gaps: BTreeMap<i128, usize> = BTreeMap::new();
    for pair in presented.windows(2) {
        let [(a, r), (b, _)] = pair else {
            continue;
        };
        if *r != 0 {
            *gaps.entry(refreshes_in(b - a, *r)?).or_default() += 1;
        }
    }
    if !gaps.is_empty() {
        *out += &format!(
            "        frames presented n refreshes apart: {}\n",
            counts(&gaps)
        );
    }
    Ok(failed)
}

/// The whole tool: 1 when a check fails, or when the log will not read.
pub fn run(args: &[OsString]) -> Done {
    let mut out = String::new();
    let [log, compositor, tearing, ..] = args else {
        return Done::failed(
            out,
            "usage: linux-frames <traffic.log> <compositor.txt> <on|off>",
        );
    };
    // `compositor.txt` must be UTF-8; the log takes a replacement character
    // for whatever is not, a string a client sent being any bytes.
    let offered = match fs::read(Path::new(compositor)).map(String::from_utf8) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Done::failed(out, format!("{}: {e}", compositor.display())),
        Err(e) => return Done::failed(out, format!("{}: {e}", compositor.display())),
    };
    let bytes = match fs::read(Path::new(log)) {
        Ok(b) => b,
        Err(e) => return Done::failed(out, format!("{}: {e}", log.display())),
    };
    let text = String::from_utf8_lossy(&bytes);
    match report(&text, &offered, &tearing.to_string_lossy(), &mut out) {
        Ok(failed) => Done::with(out, i32::from(failed > 0)),
        Err(why) => Done::failed(out, why),
    }
}
