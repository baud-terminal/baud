//! **Every event-rig row ends on its event, never on wall time**, or is
//! listed here as one that does not yet.
//!
//! A row that ends on a duration -- a count of run-loop passes, a fixed
//! window, a timeout reached on the success path -- passes or fails by how
//! fast the machine ran, not by what the app did: the printed-paths row
//! that ran two seconds of passes read red under load, fed and never asked
//! about. This reads the rig's sources and names every such row.
//!
//! **Rows** ([`rows`]): each `FrameSite` and `Site` literal in
//! `baud-app/tests/event_rig.rs`. How a row ends is read from the child,
//! `baud-app/src/app/rig.rs`, not from a list kept here:
//! - a frame row's end is its `RigEnd`, which the parent's one mapping
//!   (`fn rig_end`) gives what it expects, and the child ends it where
//!   `run_frames`' `match rig.end` sends that `RigEnd` ([`settles_ends`]
//!   holds the mapping to naming every `Settles` and none by a wildcard).
//!   A variant on the arm that runs `rig.attempts` passes of the run loop
//!   ends on a count of passes, each one up to the pass's timeout: a
//!   duration. An arm with nothing in it ends where the ticks' own run
//!   did, the arm of its `RigFrameTicks` in the `match rig.ticks` before
//!   it. Any other arm, the ticks' included, must loop on an event and
//!   print a label past its bound, or the row is unclassified and the
//!   check red;
//! - an event row's acts are read from `run_arm`'s `match site.cause`, each
//!   act's `Ends` from `run_call`'s `match act`, and each `Ends`' bound
//!   from `run`'s `match ends`. An `Ends` that waits for a sentinel ends
//!   on it, its backstop printing a label of its own. One that waits for
//!   the effects alone, under `BOUND`, ends on them while the row passes,
//!   if `run` says on a line of its own that the bound passed with an
//!   effect owed (`bound passed before its effects`); with no such line,
//!   reaching it reads as an iteration with nothing to say, which cannot
//!   tell a lost effect from a late program, and the row ends on wall
//!   time. A row whose recorded failure is `BoundPassed` passes by reaching
//!   that bound, a timeout on its success path: a duration, labelled or not.
//!
//! **A hang bound that only fails is allowed**, and told apart by its
//! label: [`Kind::Bound`] names the line printed past it, which the check
//! finds in the file. A wait's bound reached with no line of its own is
//! [`Kind::Unlabelled`]: not an end on the success path, and listed apart
//! from the offenders.
//!
//! **Sites** ([`SITES`]): every wall-clock wait in the rig's files -- an
//! `elapsed()`, a deadline made from now, a count of passes, a `sleep`, a
//! `recv_timeout` -- found by its text and classified here with why. A
//! site found that no entry names, or an entry that finds no site, is red,
//! so nothing is exempt by silence. The settles every row passes through
//! (the setup's thirty ticks, the drains of sixty-four passes) are listed
//! as sites, each once, with the rows they reach, not counted per row.
//! The files read: the parent (`event_rig.rs`), the child (`rig.rs`), the
//! call both speak (`rig_call.rs`), the effect probe the child records
//! through (`baud-present-macos/src/probe.rs`), and the paths worker's
//! tests (`baud-app/src/probe.rs`; its own deadlines are the app's pace).
//!
//! The offenders are allowlisted, exactly: a row or site that comes to end
//! on wall time is red, and one that no longer does is red until it is
//! taken off, so the lists only shrink as rows are converted.
//! [`no_rig_row_ends_on_wall_time`] is the target, ignored until then.
//!
//! The sources are read under `RIG_ENDS_ROOT` when it is set, these four
//! files of another commit extracted there; the workspace otherwise.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use regex::Regex;

const EVENT_RIG: &str = "baud-app/tests/event_rig.rs";
const RIG: &str = "baud-app/src/app/rig.rs";
const RIG_CALL: &str = "baud-present-macos/src/rig_call.rs";
const PATHS_PROBE: &str = "baud-app/src/probe.rs";
const EFFECT_PROBE: &str = "baud-present-macos/src/probe.rs";

/// **The rows that end on wall time today**, by name: the conversions'
/// work list. A row converted to end on its event comes off; none goes on.
const ENDS_ON_WALL_TIME: &[&str] = &[
    // A frame row on `RigEnd::Passes`: `rig.attempts` passes of the run
    // loop, not yet on its event and quiet, which counts no GPU frame in
    // flight. Among them the splash's first frame, whose cause is open,
    // and the one link's row, whose blinking caret leaves a restart always
    // to come, so quiet never does.
    "a static screen pauses the display link",
    "an echo runs the link, and it pauses again",
    "a flash runs the link to its end, then it pauses",
    "a key with no echo runs the link until its record expires",
    "a flip of light and dark while paused is drawn",
    "a splash waiting for its window holds no link",
    "the splash starts when its window is shown, with no tick",
    "the splash starts on the first CPU frame of a window shown before it",
    "a held p=alive is answered at its bound with no tick",
    "a held p=alive is answered on the list's wake with no tick",
    "a program's notice expires with no tick",
    "a window made key is heard with the link paused",
    "a window that resigns key is heard with the link paused",
    "a window opened not key starts with its focus lost",
    "a first-responder change in a window not key leaves its focus lost",
    "the display options changed are heard with the link paused",
    "a link printed under a resting pointer is previewed by the frame it owes",
    "output away from a resting pointer reads nothing under it",
    "scrolling output under a resting pointer is read once a frame",
    "a job started with no tick is in the title",
    "a command's child is reaped at its exit with no tick",
    "the followed settings are taken with no tick",
    "a report naming this machine titles the window, the lookup stalled",
    "the names read late are handed with no tick",
    "a title waiting on the names is set when they land, with no tick",
    "what a read lit goes dark with no tick polling for it",
    "a job come to the front lights OH with no tick polling for it",
    "a file link under a resting pointer previews with no tick",
    "a frame held past its bound is counted overdue with no tick",
    "a failed frame is counted and the window draws whole after it",
    "a launch frame past its bound opens drawn on the CPU",
    "a frame the GPU never answers gives the window to the CPU",
    "a launch the GPU never answers opens and goes on on the CPU",
    "every frame failing gives the GPU up at the third",
    "two failed frames in a row do not give the GPU up",
    "a revoked device gives the GPU up at once",
    "one display link, paused, not made again per restart",
    "a remote command's printed path stays dark under its shell's D",
    // An event row recorded as `BoundPassed`: each iteration waits out
    // `BOUND`.
    "that Cmd-click, the window not believed key",
];

/// A source with its comments blanked (`code`), and with its string and
/// character literals' insides blanked too (`skel`), byte for byte the
/// same length, so a brace or `=>` in either is found in the skeleton and
/// read in the code.
struct Source {
    code: String,
    skel: String,
}

fn ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Blank `range` of `v`, newlines kept, every byte of a character made a
/// space so the text stays UTF-8.
fn blank(v: &mut [u8], from: usize, to: usize) {
    for b in &mut v[from..to] {
        if *b != b'\n' {
            *b = b' ';
        }
    }
}

/// Where the string whose contents start at `i` ends: past its `"`.
fn string_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

fn lex(text: &str) -> Source {
    let b = text.as_bytes();
    let mut code = b.to_vec();
    let mut skel = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        let next = b.get(i + 1).copied();
        match b[i] {
            b'/' if next == Some(b'/') => {
                let end = b[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(b.len(), |n| i + n);
                blank(&mut code, i, end);
                blank(&mut skel, i, end);
                i = end;
            }
            b'/' if next == Some(b'*') => {
                let (mut depth, mut j) = (1, i + 2);
                while j < b.len() && depth > 0 {
                    match (b[j], b.get(j + 1).copied()) {
                        (b'/', Some(b'*')) => (depth, j) = (depth + 1, j + 2),
                        (b'*', Some(b'/')) => (depth, j) = (depth - 1, j + 2),
                        _ => j += 1,
                    }
                }
                blank(&mut code, i, j);
                blank(&mut skel, i, j);
                i = j;
            }
            b'"' => {
                let end = string_end(b, i + 1);
                blank(&mut skel, i + 1, end.saturating_sub(1));
                i = end;
            }
            b'r' if (i == 0
                || !ident(b[i - 1])
                || (b[i - 1] == b'b' && (i < 2 || !ident(b[i - 2]))))
                && matches!(next, Some(b'"' | b'#')) =>
            {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                let open = i + 1 + hashes;
                if b.get(open) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                let end = b[open + 1..]
                    .windows(close.len())
                    .position(|w| w == close.as_slice())
                    .map_or(b.len(), |n| open + 1 + n);
                blank(&mut skel, open + 1, end);
                i = (end + close.len()).min(b.len());
            }
            b'\'' => {
                // A character literal, or a lifetime, which has no closing
                // quote.
                let end = if next == Some(b'\\') {
                    b[i + 3..]
                        .iter()
                        .position(|&c| c == b'\'')
                        .map(|n| i + 3 + n + 1)
                } else {
                    let len = text[i + 1..].chars().next().map_or(1, char::len_utf8);
                    (b.get(i + 1 + len) == Some(&b'\'')).then_some(i + 2 + len)
                };
                match end {
                    Some(end) => {
                        blank(&mut skel, i + 1, end - 1);
                        i = end;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    let text = |v: Vec<u8>| String::from_utf8(v).expect("blanking keeps UTF-8");
    Source {
        code: text(code),
        skel: text(skel),
    }
}

/// The index just past the bracket that closes the one at `open`.
fn close(skel: &str, open: usize) -> usize {
    let b = skel.as_bytes();
    let (o, c) = match b[open] {
        b'{' => (b'{', b'}'),
        b'(' => (b'(', b')'),
        b'[' => (b'[', b']'),
        other => panic!("no bracket at {open}: {:?}", other as char),
    };
    let mut depth = 0_usize;
    for (k, &x) in b.iter().enumerate().skip(open) {
        if x == o {
            depth += 1;
        } else if x == c {
            depth -= 1;
            if depth == 0 {
                return k + 1;
            }
        }
    }
    panic!("unclosed bracket at {open}")
}

/// The block `{ ... }` whose first brace follows the first match of `head`
/// at or after `from`, as a byte range of the source.
fn block_after(src: &Source, head: &str, from: usize) -> Option<(usize, usize)> {
    let at = from + src.skel[from..].find(head)?;
    let open = at + src.skel[at..].find('{')?;
    Some((open, close(&src.skel, open)))
}

/// A `match`'s arms between its braces: each arm's pattern and its body,
/// as code.
fn arms(src: &Source, (open, end): (usize, usize)) -> Vec<(String, String)> {
    let b = src.skel.as_bytes();
    let stop = end - 1;
    let mut out = Vec::new();
    let mut i = open + 1;
    loop {
        while i < stop && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        if i >= stop {
            return out;
        }
        let mut depth = 0_i32;
        let mut j = i;
        while j + 1 < stop && !(depth == 0 && &b[j..j + 2] == b"=>") {
            match b[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let pattern = src.code[i..j].trim().to_owned();
        let mut k = j + 2;
        while k < stop && b[k].is_ascii_whitespace() {
            k += 1;
        }
        let body_end = if b[k] == b'{' {
            close(&src.skel, k)
        } else {
            let mut depth = 0_i32;
            let mut m = k;
            while m < stop && !(depth == 0 && b[m] == b',') {
                match b[m] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth -= 1,
                    _ => {}
                }
                m += 1;
            }
            m
        };
        out.push((pattern, src.code[k..body_end].trim().to_owned()));
        i = body_end;
    }
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].bytes().filter(|&b| b == b'\n').count() + 1
}

fn first(re: &str, text: &str) -> Option<String> {
    Regex::new(re)
        .expect("a pattern")
        .captures(text)
        .map(|c| c[1].to_owned())
}

fn all(re: &str, text: &str) -> Vec<String> {
    Regex::new(re)
        .expect("a pattern")
        .captures_iter(text)
        .map(|c| c[1].to_owned())
        .collect()
}

/// The rig's files, as read.
struct Tree {
    event_rig: Source,
    rig: Source,
    rig_call: Source,
    paths_probe: Source,
    effect_probe: Source,
}

fn root() -> PathBuf {
    match std::env::var_os("RIG_ENDS_ROOT") {
        Some(r) => PathBuf::from(r),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."),
    }
}

fn read(path: &str) -> Source {
    let at = root().join(path);
    lex(&std::fs::read_to_string(&at).unwrap_or_else(|e| panic!("{}: {e}", at.display())))
}

fn tree() -> Tree {
    Tree {
        event_rig: read(EVENT_RIG),
        rig: read(RIG),
        rig_call: read(RIG_CALL),
        paths_probe: read(PATHS_PROBE),
        effect_probe: read(EFFECT_PROBE),
    }
}

/// How a row is driven: a frame row's ticks, window and attempts, or an
/// event row's cause, its gesture's act, and the failure it is recorded
/// to come to, if it is a control or pending.
#[derive(Clone, Debug)]
enum Rig {
    Frame {
        ticks: String,
        attempts: String,
        /// The `Settles` variant it expects, which its end is mapped from.
        expect: String,
    },
    Event {
        cause: String,
        gesture_act: Option<String>,
        recorded: Option<String>,
    },
}

/// A line of a file, counted from one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Line(usize);

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug)]
struct Row {
    name: String,
    line: Line,
    /// The row's literal, as code: its shell text included.
    text: String,
    rig: Rig,
}

/// A row's name: `SiteName("...")`, its escapes as written.
fn name_of(text: &str) -> Option<String> {
    first(r#"name:\s*SiteName\(\s*"((?:[^"\\]|\\.)*)""#, text)
}

/// The act a gesture's literal names, or the one its constant or the
/// constant it spreads names.
fn gesture_act(src: &Source, text: &str) -> Option<String> {
    if let Some(act) = first(r"act:\s*RigAct::(\w+)", text) {
        return Some(act);
    }
    let named = first(r"Cause::Gesture\(\s*(\w+)\s*\)", text)
        .or_else(|| first(r"\.\.\s*([A-Z][A-Z_0-9]+)", text))?;
    let (open, end) = block_after(src, &format!("const {named}: Gesture = Gesture"), 0)?;
    first(r"act:\s*RigAct::(\w+)", &src.code[open..end])
}

/// Every `FrameSite` and `Site` literal in the event rig's file.
fn rows(src: &Source) -> Vec<Row> {
    let mut out = Vec::new();
    for (kind, re) in [("frame", r"\bFrameSite\s*\{"), ("event", r"\bSite\s*\{")] {
        for m in Regex::new(re).expect("a pattern").find_iter(&src.skel) {
            let before = src.skel[..m.start()].trim_end();
            if before.ends_with("struct") || before.ends_with("for") {
                continue;
            }
            let open = m.end() - 1;
            let text = src.code[open..close(&src.skel, open)].to_owned();
            let Some(name) = name_of(&text) else {
                continue;
            };
            let rig = match kind {
                "frame" => Rig::Frame {
                    ticks: first(r"ticks:\s*RigFrameTicks::(\w+)", &text).unwrap_or_default(),
                    attempts: first(r"attempts:\s*Iterations\(([^)]*)\)", &text)
                        .unwrap_or_default(),
                    expect: first(r"expect:\s*Settles::(\w+)", &text).unwrap_or_default(),
                },
                _ => Rig::Event {
                    cause: first(r"cause:\s*Cause::(\w+)", &text).unwrap_or_default(),
                    gesture_act: gesture_act(src, &text),
                    recorded: first(r"RowState::Control\(\s*Failure::(\w+)", &text)
                        .or_else(|| first(r"recorded:\s*Failure::(\w+)", &text)),
                },
            };
            out.push(Row {
                name,
                line: Line(line_of(&src.code, m.start())),
                text,
                rig,
            });
        }
    }
    out
}

/// A duration constant's value as written, from the first file of
/// `files` that defines it.
fn duration(files: &[&Source], name: &str) -> String {
    let re = format!(
        r"const {name}:\s*(?:std::time::)?Duration\s*=\s*(?:std::time::)?Duration::from_(secs|millis|micros)\((\d+)\)"
    );
    let re = Regex::new(&re).expect("a pattern");
    files
        .iter()
        .find_map(|f| {
            re.captures(&f.code).map(|c| {
                let unit = match &c[1] {
                    "secs" => "s",
                    "millis" => "ms",
                    _ => "us",
                };
                format!("{} {unit}", &c[2])
            })
        })
        .unwrap_or_else(|| format!("{name} (no value found)"))
}

/// **Whether `run` says that a bound with no sentinel passed**, on a line
/// of its own, when an iteration's effects owed are not all in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BoundLine {
    /// An arm of its `match` on how the iteration ended has no sentinel
    /// (`None`) and prints `bound passed before`.
    Said,
    /// No such arm: the bound reached reads as an iteration with nothing
    /// to say.
    Unsaid,
}

/// **What a `RigEnd` arm of the child's run does**, other than a count.
#[derive(Clone, Debug)]
enum EndArm {
    /// Nothing: the run ended where the ticks' own run did.
    Ticks,
    /// A loop of its own: on an event with a label past its bound, or why
    /// it is not one.
    Own(Result<(), String>),
}

/// **How the child ends each kind of row**, read from its source.
struct Ends {
    /// The frame rig's `RigEnd`s that end on `rig.attempts` passes.
    by_count: Vec<String>,
    /// The longest a pass of those runs waits, as written.
    pass: String,
    /// Each other `RigEnd`'s arm.
    end_arms: BTreeMap<String, EndArm>,
    /// The `RigEnd`s whose arm reads the act's clear itself and signs it
    /// (`act_signed = true`), so no wait after it is that row's.
    signs: Vec<String>,
    /// Each `Settles` variant's `RigEnd`, the parent's one mapping.
    settles_end: BTreeMap<String, String>,
    /// Each `RigFrameTicks` on an arm of the ticks' run: why it is not an
    /// end on an event, or why it cannot be told.
    by_event: BTreeMap<String, Result<(), String>>,
    /// Each event act's `Ends`.
    act_ends: BTreeMap<String, String>,
    /// Each `Ends`' bound, and whether it waits for a sentinel.
    ends_bound: BTreeMap<String, (String, bool)>,
    /// Whether `run` says that an iteration with no sentinel passed its
    /// bound with an effect owed.
    bound_line: BoundLine,
    /// Each cause's acts (`run_arm`); a cause naming none takes its
    /// gesture's.
    cause_acts: BTreeMap<String, Vec<String>>,
}

fn ends(t: &Tree) -> Result<Ends, String> {
    let rig = &t.rig;
    let frames = rig
        .skel
        .find("fn run_frames(")
        .ok_or("rig.rs: no `fn run_frames`")?;
    // The run's end: `run_frames`' `match rig.end`, one of whose arms runs
    // `rig.attempts` passes.
    let by_end = block_after(rig, "match rig.end {", frames)
        .ok_or("rig.rs: no `match rig.end` in `run_frames`")?;
    let count_arm = Regex::new(
        r"(?s)((?:RigEnd::\w+\s*\|\s*)*RigEnd::\w+)\s*=>\s*(run|offer)\(rig\.attempts\.0\)",
    )
    .expect("a pattern");
    let found = count_arm
        .captures(&rig.code[by_end.0..by_end.1])
        .ok_or("rig.rs: no arm of `match rig.end` runs `rig.attempts` passes")?;
    let by_count = all(r"RigEnd::(\w+)", &found[1]);
    // The ticks' own run: the `match rig.ticks` before it that closes
    // last, so neither one nested in another arm nor the run's closure.
    let end_match = rig.skel[frames..by_end.0]
        .match_indices("match rig.ticks {")
        .map(|(p, m)| {
            let open = frames + p + m.len() - 1;
            (open, close(&rig.skel, open))
        })
        .filter(|&(_, end)| end <= by_end.0)
        .max_by_key(|&(_, end)| end)
        .ok_or("rig.rs: no `match rig.ticks` before `match rig.end`")?;
    // A pass's longest wait: the `offer` and `run` closures' passes.
    let mut pass = String::new();
    for head in ["let offer = |", "let run = |"] {
        let (start, end) = block_after(rig, head, frames).ok_or(format!("rig.rs: no `{head}`"))?;
        for p in all(r"probe::pass\(([0-9.]+)\)", &rig.code[start..end]) {
            if pass.is_empty() || p.parse::<f64>().ok() > pass.parse::<f64>().ok() {
                pass = p;
            }
        }
    }
    if pass.is_empty() {
        return Err("rig.rs: no pass of a timeout in `offer` or `run`".to_owned());
    }
    let a_loop = Regex::new(r"\b(while|loop)\b").expect("a pattern");
    let a_label = Regex::new(r"println!\(|\bfail\(|break Some\(").expect("a pattern");
    let verdict = |v: &str, body: &str| match (a_loop.is_match(body), a_label.is_match(body)) {
        (true, true) => Ok(()),
        (false, _) => Err(format!(
            "{v}: its arm has no loop, and so waits on no event"
        )),
        (true, false) => Err(format!("{v}: its loop prints no label past its bound")),
    };
    let mut by_event = BTreeMap::new();
    for (pattern, body) in arms(rig, end_match) {
        for v in all(r"RigFrameTicks::(\w+)", &pattern) {
            let judged = verdict(&v, &body);
            by_event.insert(v, judged);
        }
    }
    let mut end_arms = BTreeMap::new();
    let mut signs = Vec::new();
    for (pattern, body) in arms(rig, by_end) {
        for v in all(r"RigEnd::(\w+)", &pattern) {
            if body.contains("act_signed = true") {
                signs.push(v.clone());
            }
            if by_count.contains(&v) {
                continue;
            }
            let arm = match body.trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace()) {
                "" => EndArm::Ticks,
                _ => EndArm::Own(verdict(&v, &body)),
            };
            end_arms.insert(v, arm);
        }
    }
    let settles_end = settles_ends(&t.event_rig).map_err(|red| red.join("; "))?;
    // The event rig: each act's `Ends`, the default the thread's own.
    let default = first(r"static ENDS:[^;]*Cell::new\(Ends::(\w+)\)", &rig.code)
        .ok_or("rig.rs: no default `Ends`")?;
    let calls = rig
        .skel
        .find("fn run_call(")
        .ok_or("rig.rs: no `fn run_call`")?;
    let acts = block_after(rig, "let act = match act", calls).ok_or("rig.rs: no `match act`")?;
    let mut act_ends = BTreeMap::new();
    for (pattern, body) in arms(rig, acts) {
        let set = first(r"Ends::(\w+)", &body).unwrap_or_else(|| default.clone());
        for a in all(r"RigAct::(\w+)", &pattern) {
            act_ends.insert(a, set.clone());
        }
    }
    let run = rig.skel.find("\nfn run(").ok_or("rig.rs: no `fn run`")?;
    let by_ends = block_after(rig, "= match ends", run).ok_or("rig.rs: no `match ends`")?;
    let mut ends_bound = BTreeMap::new();
    for (pattern, body) in arms(rig, by_ends) {
        let bound = first(r"\(\s*(?:probe_call::)?([A-Z_]+)", &body).unwrap_or_default();
        let sentinel = body.contains("Some(");
        for e in all(r"Ends::(\w+)", &pattern) {
            ends_bound.insert(e, (bound.clone(), sentinel));
        }
    }
    let said = block_after(rig, "match (ended, holds(&got, each), sentinel)", run)
        .ok_or("rig.rs: no `match` on how an iteration ended")?;
    let bound_line = match arms(rig, said).iter().any(|(pattern, body)| {
        pattern.starts_with("(None") && body.contains("\"bound passed before")
    }) {
        true => BoundLine::Said,
        false => BoundLine::Unsaid,
    };
    let src = &t.event_rig;
    let run_arm = src
        .skel
        .find("fn run_arm(")
        .ok_or("event_rig.rs: no `fn run_arm`")?;
    let causes = block_after(src, "match site.cause", run_arm)
        .ok_or("event_rig.rs: no `match site.cause`")?;
    let mut cause_acts = BTreeMap::new();
    for (pattern, body) in arms(src, causes) {
        let acts = all(r"RigAct::(\w+)", &body);
        for c in all(r"Cause::(\w+)", &pattern) {
            cause_acts.insert(c, acts.clone());
        }
    }
    Ok(Ends {
        by_count,
        pass,
        end_arms,
        signs,
        settles_end,
        by_event,
        act_ends,
        ends_bound,
        bound_line,
        cause_acts,
    })
}

/// **The parent's one mapping from what a frame row expects to its end**
/// (`event_rig.rs`, `fn rig_end`), read arm by arm: each `Settles`
/// variant and its `RigEnd`. Red, with each reason, on an arm that names
/// a variant by a wildcard or a binding -- one that would end a variant
/// added later without its end being chosen -- on a variant of
/// `enum Settles` named on no arm or on two, and on an arm with no one
/// `RigEnd`.
fn settles_ends(src: &Source) -> Result<BTreeMap<String, String>, Vec<String>> {
    let mut red = Vec::new();
    let Some(at) = src.skel.find("fn rig_end(") else {
        return Err(vec!["event_rig.rs: no `fn rig_end`".to_owned()]);
    };
    let Some(by) = block_after(src, "match expect {", at) else {
        return Err(vec![
            "event_rig.rs: `rig_end` has no `match expect`".to_owned(),
        ]);
    };
    let Some(settles) = block_after(src, "enum Settles {", 0) else {
        return Err(vec!["event_rig.rs: no `enum Settles`".to_owned()]);
    };
    let variant =
        Regex::new(r"^Settles::(\w+)\s*(?:\(\s*\.\.\s*\)|\{\s*\.\.\s*\})?$").expect("a pattern");
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for (pattern, body) in arms(src, by) {
        let ends = all(r"RigEnd::(\w+)", &body);
        let [end] = ends.as_slice() else {
            red.push(format!("the arm `{pattern}` names {} ends", ends.len()));
            continue;
        };
        // The alternatives, split at the top level's `|`.
        let mut alts = Vec::new();
        let (mut depth, mut from) = (0_i32, 0);
        for (k, c) in pattern.char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                '|' if depth == 0 => {
                    alts.push(pattern[from..k].trim());
                    from = k + 1;
                }
                _ => {}
            }
        }
        alts.push(pattern[from..].trim());
        for alt in alts {
            match variant.captures(alt) {
                Some(c) => {
                    if map.insert(c[1].to_owned(), end.clone()).is_some() {
                        red.push(format!("Settles::{} is on two arms", &c[1]));
                    }
                }
                None => red.push(format!(
                    "`{alt}` names no one Settles variant: a wildcard or a binding ends what is added later unchosen"
                )),
            }
        }
    }
    let body = &src.code[settles.0 + 1..settles.1 - 1];
    let declared = all(r"(?m)^\s{4}([A-Z]\w*)\b", body);
    if declared.is_empty() {
        red.push("enum Settles: no variant found".to_owned());
    }
    for v in &declared {
        if !map.contains_key(v) {
            red.push(format!("Settles::{v} has no end in `rig_end`"));
        }
    }
    match red.is_empty() {
        true => Ok(map),
        false => Err(red),
    }
}

/// What ends a row.
#[derive(Clone, PartialEq, Eq, Debug)]
enum End {
    /// Its event, any bound past it a labelled failure.
    Event,
    /// A duration -- a count of passes, a bound reached on the success
    /// path, or one reached with no line of its own: why.
    WallTime(String),
    /// Not told: why. Red.
    Unknown(String),
}

fn end_of(row: &Row, e: &Ends, t: &Tree) -> End {
    let files = [&t.rig, &t.rig_call];
    match &row.rig {
        Rig::Frame {
            ticks,
            attempts,
            expect,
        } => {
            let Some(end) = e.settles_end.get(expect) else {
                return End::Unknown(format!("Settles::{expect:?}: no end in `rig_end`"));
            };
            if e.by_count.contains(end) {
                let n: Option<f64> = attempts.parse().ok();
                let up_to = match (n, e.pass.parse::<f64>().ok()) {
                    (Some(n), Some(p)) => format!(", up to {:.1} s", n * p),
                    _ => String::new(),
                };
                return End::WallTime(format!(
                    "RigEnd::{end} (Settles::{expect}): ends after {attempts} passes of the run \
                     loop, each up to {} s (`run(rig.attempts.0)`){up_to}",
                    e.pass
                ));
            }
            let judged = match e.end_arms.get(end) {
                Some(EndArm::Own(judged)) => judged,
                Some(EndArm::Ticks) => match e.by_event.get(ticks) {
                    Some(judged) => judged,
                    None => {
                        return End::Unknown(format!(
                            "RigFrameTicks::{ticks}: on no arm of the ticks' run"
                        ));
                    }
                },
                None => {
                    return End::Unknown(format!("RigEnd::{end}: on no arm of the run's end"));
                }
            };
            match judged {
                Ok(()) => End::Event,
                Err(why) => End::Unknown(why.clone()),
            }
        }
        Rig::Event {
            cause,
            gesture_act,
            recorded,
        } => {
            let Some(acts) = e.cause_acts.get(cause) else {
                return End::Unknown(format!("Cause::{cause}: on no arm of `run_arm`"));
            };
            let acts = match (acts.is_empty(), gesture_act) {
                (false, _) => acts.clone(),
                (true, Some(a)) => vec![a.clone()],
                (true, None) => return End::Unknown(format!("Cause::{cause}: no act found")),
            };
            for a in &acts {
                let Some(ends) = e.act_ends.get(a) else {
                    return End::Unknown(format!("RigAct::{a}: on no arm of `run_call`"));
                };
                let Some((bound, sentinel)) = e.ends_bound.get(ends) else {
                    return End::Unknown(format!("Ends::{ends}: on no arm of `match ends`"));
                };
                if *sentinel {
                    continue;
                }
                let value = duration(&files, bound);
                if recorded.as_deref() == Some("BoundPassed") {
                    return End::WallTime(format!(
                        "RigAct::{a}, Ends::{ends}: each iteration waits out {bound} ({value}); \
                         its recorded failure, BoundPassed, is that bound reached"
                    ));
                }
                if e.bound_line == BoundLine::Unsaid {
                    return End::WallTime(format!(
                        "RigAct::{a}, Ends::{ends}: {bound} ({value}) reached reads as an \
                         iteration with nothing to say, with no line of its own"
                    ));
                }
            }
            End::Event
        }
    }
}

/// Every row and its end, red on one that cannot be told.
fn judged(t: &Tree) -> Vec<(Row, End)> {
    let e = ends(t).unwrap_or_else(|why| panic!("the child's ends could not be read: {why}"));
    let rows = rows(&t.event_rig);
    assert!(!rows.is_empty(), "no row found: the literals moved");
    rows.into_iter()
        .map(|r| {
            let end = end_of(&r, &e, t);
            (r, end)
        })
        .collect()
}

fn report(judged: &[(Row, End)]) -> String {
    let mut s = String::new();
    for (r, end) in judged {
        if let End::WallTime(why) | End::Unknown(why) = end {
            let _ = writeln!(s, "  {EVENT_RIG}:{} {:?}: {why}", r.line, r.name);
        }
    }
    s
}

/// The rows that end on wall time, by name, as their literals spell them.
fn offenders(judged: &[(Row, End)]) -> Vec<String> {
    judged
        .iter()
        .filter(|(_, e)| matches!(e, End::WallTime(_)))
        .map(|(r, _)| r.name.clone())
        .collect()
}

/// **The rows that end on wall time are exactly [`ENDS_ON_WALL_TIME`]**:
/// a row that comes to end on one, or a row moved to its event and left on
/// the list, is red. Every row's end is told, or it is red too.
#[test]
fn every_rig_row_that_ends_on_wall_time_is_listed() {
    let t = tree();
    let judged = judged(&t);
    let names: Vec<&str> = judged.iter().map(|(r, _)| r.name.as_str()).collect();
    let mut seen = std::collections::BTreeSet::new();
    let twice: Vec<&&str> = names.iter().filter(|n| !seen.insert(**n)).collect();
    assert!(
        twice.is_empty(),
        "rows named twice, which the list could not tell apart: {twice:?}"
    );
    let unknown: Vec<_> = judged
        .iter()
        .filter(|(_, e)| matches!(e, End::Unknown(_)))
        .collect();
    let found = offenders(&judged);
    println!(
        "{} rows; {} end on wall time:\n{}",
        judged.len(),
        found.len(),
        report(&judged)
    );
    assert!(
        unknown.is_empty(),
        "rows whose end cannot be told: {unknown:#?}"
    );
    let new: Vec<&String> = found
        .iter()
        .filter(|n| !ENDS_ON_WALL_TIME.contains(&n.as_str()))
        .collect();
    let gone: Vec<&&str> = ENDS_ON_WALL_TIME
        .iter()
        .filter(|n| !found.iter().any(|f| f == *n))
        .collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "rows that end on wall time, not listed: {new:#?}\n\
         listed rows that no longer do (take them off): {gone:#?}"
    );
}

/// **No row ends on wall time**: the conversions' end, red until every
/// row of [`ENDS_ON_WALL_TIME`] ends on its event.
#[test]
#[ignore = "red until the rows of ENDS_ON_WALL_TIME are converted"]
fn no_rig_row_ends_on_wall_time() {
    let t = tree();
    let judged = judged(&t);
    let found = offenders(&judged);
    assert!(
        found.is_empty(),
        "{} rows end on wall time:\n{}",
        found.len(),
        report(&judged)
    );
}

/// **The check's own controls**, on rows written here over the child's
/// real source and the parent's real mapping: a frame row on a count of
/// passes is named and one on its ticks' event or on quiet is not; an
/// event row recorded as `BoundPassed` is named and a
/// green one is not, its bound said on a line of its own -- and named
/// once the child's line is taken out; a frame row on an arm the run's end
/// does not have is unclassified, never passed.
#[test]
fn the_check_names_a_count_of_passes_and_not_an_event() {
    let mut t = tree();
    let frame = |name: &str, ticks: &str, expect: &str| {
        format!(
            "    FrameSite {{\n        name: SiteName(\"{name}\"),\n        setup: STILL,\n        \
             act: \"\",\n        ticks: RigFrameTicks::{ticks},\n        window: RigWindow::Released,\n        \
             attempts: Iterations(200),\n        expect: Settles::{expect},\n        \
             state: RowState::Green,\n    }},\n"
        )
    };
    let event = |name: &str, state: &str| {
        format!(
            "    Site {{\n        name: SiteName(\"{name}\"),\n        cause: Cause::Gesture(LINK_CLICK),\n        \
             expect: Expect::effect(&[EffectKind::Command]),\n        state: {state},\n    }},\n"
        )
    };
    let real = &t.event_rig.code;
    let keep = |head: &str| {
        let at = real.find(head).unwrap_or_else(|| panic!("no {head}"));
        let open = at + real[at..].find('{').expect("a brace");
        real[at..close(&lex(real).skel, open)].to_owned()
    };
    let text = format!(
        "const LINK_CLICK: Gesture = Gesture {{ act: RigAct::Link, }};\n{}\n{}\n{}\n{}\n\
         const ROWS: [FrameSite; 4] = [\n{}{}{}{}];\nconst EVENTS: [Site; 2] = [\n{}{}];\n",
        keep("fn run_arm("),
        keep("enum Settles {"),
        keep("fn rig_end("),
        "fn after() {}",
        frame("on a count", "Withheld", "Greeted"),
        frame("on its event", "UntilAsked", "Filed(\"/x\")"),
        frame("on no arm", "Sideways", "Marked(1)"),
        frame("on quiet", "Withheld", "StillOnCpu"),
        event(
            "waits out its bound",
            "RowState::Control(Failure::BoundPassed)"
        ),
        event("ends on its effect", "RowState::Green"),
    );
    t.event_rig = lex(&text);
    let judged = judged(&t);
    let end = |name: &str| {
        judged
            .iter()
            .find(|(r, _)| r.name == name)
            .map(|(_, e)| e.clone())
            .unwrap_or_else(|| panic!("no row {name:?}: {judged:#?}"))
    };
    assert!(matches!(end("on a count"), End::WallTime(_)), "{judged:#?}");
    assert_eq!(end("on its event"), End::Event);
    assert!(matches!(end("on no arm"), End::Unknown(_)), "{judged:#?}");
    assert_eq!(end("on quiet"), End::Event);
    assert!(
        matches!(end("waits out its bound"), End::WallTime(_)),
        "{judged:#?}"
    );
    assert_eq!(end("ends on its effect"), End::Event);
    // The child's line taken out, the same green row reaches its bound with
    // nothing said: wall time.
    let said = "\"bound passed before";
    assert!(t.rig.code.contains(said), "the child says no bound passed");
    t.rig = lex(&t.rig.code.replace(said, "\"passed before"));
    let unsaid = crate::judged(&t);
    assert!(
        unsaid
            .iter()
            .any(|(r, e)| r.name == "ends on its effect" && matches!(e, End::WallTime(_))),
        "{unsaid:#?}"
    );
}

/// **Every `Settles` has its end, chosen by name**: the parent's one
/// mapping (`rig_end`) names every variant of `Settles` on one arm and
/// none by a wildcard or a binding, each arm one `RigEnd`, and each
/// `RigEnd` it gives is one the child's run ends on. Its controls, on the
/// real mapping: a `_` arm added, a variant taken off its arm, and a
/// variant named twice are each red. Break: a `_` arm in `rig_end`.
#[test]
fn every_settles_has_its_end_chosen_by_name() {
    let t = tree();
    let map = settles_ends(&t.event_rig).unwrap_or_else(|red| panic!("{}", red.join("\n")));
    let e = ends(&t).unwrap_or_else(|why| panic!("the child's ends could not be read: {why}"));
    let unended: Vec<String> = map
        .iter()
        .filter(|(_, end)| !e.by_count.contains(end) && !e.end_arms.contains_key(*end))
        .map(|(s, end)| format!("Settles::{s} -> RigEnd::{end}"))
        .collect();
    assert!(
        unended.is_empty(),
        "ends the child's run has no arm for: {unended:?}"
    );
    let real = &t.event_rig.code;
    let head = "match expect {";
    let at = real.find("fn rig_end(").expect("rig_end");
    let open = at + real[at..].find(head).expect("its match") + head.len();
    let wild = format!(
        "{}\n        _ => RigEnd::Passes,{}",
        &real[..open],
        &real[open..]
    );
    let red = settles_ends(&lex(&wild)).expect_err("a `_` arm");
    assert!(red.iter().any(|r| r.contains("wildcard")), "{red:?}");
    let off = "        | Settles::OffHook => RigEnd::Passes,";
    assert_eq!(real.matches(off).count(), 1, "the last arm's last variant");
    let dropped = real.replace(off, "        => RigEnd::Passes,");
    let red = settles_ends(&lex(&dropped)).expect_err("a variant with no end");
    assert!(
        red.iter().any(|r| r.contains("OffHook has no end")),
        "{red:?}"
    );
    let twice = real.replace(
        off,
        "        | Settles::OffHook\n        | Settles::Still => RigEnd::Passes,",
    );
    let red = settles_ends(&lex(&twice)).expect_err("a variant on two arms");
    assert!(
        red.iter().any(|r| r.contains("Still is on two arms")),
        "{red:?}"
    );
}

/// **Every row that ends on quiet draws on the CPU**: quiet counts no
/// frame in flight, and a GPU frame's completion wakes the window from the
/// GPU's thread, which a barrier posted from the main thread is not
/// ordered after (rig.rs, `quiet`). So until a frame in flight is a part
/// of quiet, every frame row whose `Settles` the mapping (`rig_end`) ends
/// on `RigEnd::Quiet`, `RigEnd::QuietOffered` or `RigEnd::QuietCounted`
/// sets `BAUD_GPU` to `cpu` in its own environment, as the CPU twins do. Its controls, on the real
/// rows: at least one row ends on quiet, so the check reads something, and
/// with the CPU's setting spelled otherwise every such row is named.
/// Break: a twin's `BAUD_GPU` taken off.
#[test]
fn every_row_that_ends_on_quiet_draws_on_the_cpu() {
    const CPU: &str = r#"("BAUD_GPU", "cpu")"#;
    let t = tree();
    let map = settles_ends(&t.event_rig).unwrap_or_else(|red| panic!("{}", red.join("\n")));
    let on_quiet = |r: &Row| match &r.rig {
        Rig::Frame { expect, .. } => map
            .get(expect)
            .is_some_and(|end| end == "Quiet" || end == "QuietOffered" || end == "QuietCounted"),
        Rig::Event { .. } => false,
    };
    let on_gpu = |src: &Source| -> Vec<String> {
        rows(src)
            .iter()
            .filter(|r| on_quiet(r) && !r.text.contains(CPU))
            .map(|r| r.name.clone())
            .collect()
    };
    let quiet: Vec<String> = rows(&t.event_rig)
        .iter()
        .filter(|r| on_quiet(r))
        .map(|r| r.name.clone())
        .collect();
    assert!(!quiet.is_empty(), "no row ends on quiet: nothing is read");
    assert_eq!(
        on_gpu(&t.event_rig),
        Vec::<String>::new(),
        "rows that end on quiet and may draw on the GPU"
    );
    let spelled = lex(&t.event_rig.code.replace(CPU, r#"("BAUD_GPU", "auto")"#));
    assert_eq!(
        on_gpu(&spelled),
        quiet,
        "the control: with the CPU's setting spelled otherwise, every row on quiet is named"
    );
}

/// **The lexer blanks what is no code**: a brace in a string, a raw
/// string, a character or a comment is never counted, and a lifetime
/// leaves what follows it read.
#[test]
fn the_lexer_blanks_strings_characters_and_comments() {
    let s = lex("fn f<'a>(x: &'a str) { let _ = (\"{\", r#\"}\"#, '{', b'}'); // }\n /* { */ }");
    let open = s.skel.find('{').expect("a brace");
    assert_eq!(close(&s.skel, open), s.skel.len());
    assert!(s.code.contains("\"{\"") && !s.code.contains("// }"));
}

/// **What a wall-clock wait is for.**
#[derive(Clone, Copy, Debug)]
enum Kind {
    /// The end of a row's run or iteration, told row by row by
    /// [`every_rig_row_that_ends_on_wall_time_is_listed`].
    RowEnd,
    /// A hang bound that only fails, printing this line past it.
    Bound(&'static str),
    /// A bound that only fails, reaching it read as the row's own failure
    /// with no line of its own.
    Unlabelled,
    /// Wall time passed on purpose as an input -- a cause made late, a
    /// tick held to the display's period -- and the row still ends on its
    /// event.
    Hold,
    /// Time read and printed or compared, or handed on, never waited on.
    Measure,
    /// A pass inside the loop of the site whose needle this is, classified
    /// there.
    Within(&'static str),
    /// A test of the rig's own harness, or the parent's wait on a child it
    /// ends: no row's.
    Harness,
    /// **An offender**: a fixed window -- a count of passes or a span of
    /// time -- that ends or settles what these rows judge, whatever came in
    /// it.
    Window(Reach),
    /// **An offender**: a bound that, reached, goes on as if its event had
    /// come.
    FailOpen(Reach),
}

/// The rows an offending site reaches.
#[derive(Clone, Copy, Debug)]
enum Reach {
    EveryFrameRow,
    /// Frame rows whose act ends with the pasteboard's clear and whose
    /// clear no step before has waited for: not a `Released` window's,
    /// whose settle signs it, nor `UntilMarked`'s, whose run does, nor a
    /// row whose `RigEnd` (the parent's mapping of what it expects) has an
    /// arm that does, as quiet's (`act_signed`).
    FrameRowsWhoseActClears,
    /// Event rows of these causes.
    EventCauses(&'static [&'static str]),
    /// Frame rows on these `RigWindow`s.
    FrameWindows(&'static [&'static str]),
    /// These rows, by name.
    Named(&'static [&'static str]),
}

fn reaches(reach: Reach, r: &Row, e: &Ends) -> bool {
    match (reach, &r.rig) {
        (Reach::EveryFrameRow, Rig::Frame { .. }) => true,
        (Reach::FrameRowsWhoseActClears, Rig::Frame { ticks, expect, .. }) => {
            let window = first(r"window:\s*RigWindow::(\w+)", &r.text).unwrap_or_default();
            let signed_at_its_end = e
                .settles_end
                .get(expect)
                .is_some_and(|end| e.signs.contains(end));
            first(r"(?s)act:\s*(.*?)\n\s*keys:", &r.text).is_some_and(|a| a.contains("]52;c;!"))
                && !window.starts_with("Released")
                && ticks != "UntilMarked"
                && !signed_at_its_end
        }
        (Reach::EventCauses(c), Rig::Event { cause, .. }) => c.contains(&cause.as_str()),
        (Reach::FrameWindows(w), Rig::Frame { .. }) => {
            first(r"window:\s*RigWindow::(\w+)", &r.text).is_some_and(|x| w.contains(&x.as_str()))
        }
        (Reach::Named(n), _) => n.contains(&r.name.as_str()),
        _ => false,
    }
}

/// How many lines an entry's needle is found on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Times(usize);

/// One classified site: `needle` found in a wall-clock wait's line of
/// `file`, `times` times.
struct Site {
    file: &'static str,
    needle: &'static str,
    times: Times,
    kind: Kind,
    why: &'static str,
}

const fn site(
    file: &'static str,
    needle: &'static str,
    times: Times,
    kind: Kind,
    why: &'static str,
) -> Site {
    Site {
        file,
        needle,
        times,
        kind,
        why,
    }
}

/// **Every wall-clock wait in the rig's files, classified.**
const SITES: &[Site] = &[
    // The event rig's child (`run`).
    site(
        RIG,
        "let at = Instant::now() + probe_call::HELD_REPLY;",
        Times(1),
        Kind::Hold,
        "TypeHeld holds a decode's reply HELD_REPLY, a late cause on purpose; its rows end on their copy or sentinel",
    ),
    site(
        RIG,
        "*at = now + HALVES_APART;",
        Times(1),
        Kind::Hold,
        "the held reply's second half, written apart from its first, as a tty splits a write",
    ),
    site(
        RIG,
        "if now < *at {",
        Times(1),
        Kind::Hold,
        "the held reply waits for its own time",
    ),
    site(
        RIG,
        "while !ready && start.elapsed() < BOUND {",
        Times(2),
        Kind::Bound("the row's screen was never drawn"),
        "the setup's clear, waited for; reached, the row fails",
    ),
    site(
        RIG,
        "for _ in 0..64 {",
        Times(2),
        Kind::Window(Reach::EventCauses(&[
            "PrintedPath",
            "Program",
            "ProgramThen",
            "ProgramUntil",
            "Gesture",
        ])),
        "sixty-four passes of no timeout, after the setup and after each iteration's end: what is ready then is \
         taken, and what a later turn asks falls in the next iteration or is never seen",
    ),
    site(
        RIG,
        "while !holds(&got, &each[..1]) && start.elapsed() < BOUND {",
        Times(1),
        Kind::FailOpen(Reach::EventCauses(&["ProgramThen"])),
        "TypeTwo's first stage: BOUND reached, the second line is sent anyway, as if the first effect had run",
    ),
    site(
        RIG,
        "while !over(&got) && start.elapsed() < bound {",
        Times(1),
        Kind::RowEnd,
        "an iteration's end: its effects or sentinel, the bound BOUND or SENTINEL_BACKSTOP by its Ends",
    ),
    site(
        RIG,
        "(&act, SPACING.checked_sub(answered.elapsed()))",
        Times(1),
        Kind::Window(Reach::EventCauses(&[
            "Program",
            "ProgramThen",
            "ProgramUntil",
            "Gesture",
        ])),
        "SPACING, 50 ms from the records: a floor the bell's frame interval needs (`Ringer::hear`), and what comes \
         in it is this iteration's; a window all the same, and listed as one",
    ),
    site(
        RIG,
        "probe::pass(left.min(TICK).as_secs_f64());",
        Times(1),
        Kind::Within("(&act, SPACING.checked_sub(answered.elapsed()))"),
        "SPACING's passes, each up to TICK",
    ),
    site(
        RIG,
        "&& waiting.elapsed() < probe_call::SENTINEL_BACKSTOP",
        Times(1),
        Kind::Bound("a read left to a frame no tick fed"),
        "a split read still waiting is ticked for; reached, the row fails",
    ),
    // The frame rig's child (`run_frames`).
    site(
        RIG,
        "probe::set_target((now + FRAME_PERIOD).as_secs_f64());",
        Times(1),
        Kind::Measure,
        "each offered tick's target, a period on, as a real link's; the tick is offered at once",
    ),
    site(
        RIG,
        "while probe::frames().value() == 0 && start.elapsed() < BOUND {",
        Times(1),
        Kind::Bound("the first frame was never drawn"),
        "ShownFirst's first frame",
    ),
    site(
        RIG,
        "while probe::greetings().0 == 0 && start.elapsed() < BOUND {",
        Times(1),
        Kind::Unlabelled,
        "ShownFirst's splash start: reached, `first=0` is printed and the row's Settles fails it",
    ),
    site(
        RIG,
        "while probe::splash() == probe::Splash::Moving && start.elapsed() < BOUND {",
        Times(1),
        Kind::Bound("the splash never ended"),
        "ShownFirst's splash played to its end",
    ),
    site(
        RIG,
        "while !done() && start.elapsed() < bound {",
        Times(1),
        Kind::Bound("no whole frame was drawn after"),
        "the GPU rows' waits (`until`), each within BOUND or the give-up's bound and BOUND: the act's frame \
         held, counted overdue, a frame failed, a whole frame drawn after it, the GPU given up, a frame after \
         it; reached, each fails with a line of its own",
    ),
    site(
        RIG,
        "while owed() != (false, false) && start.elapsed() < PATHS_BOUND {",
        Times(1),
        Kind::Bound("the setup's paths were never answered"),
        "the setup's printed paths answered and its lights settled; reached, either fails with a line of its own",
    ),
    site(
        RIG,
        "while probe::expirations() == expired && start.elapsed() < BOUND {",
        Times(1),
        Kind::Bound("the notice never expired"),
        "`Expired`: the act's notice expired on a turn of the window's own",
    ),
    site(
        RIG,
        "offer(30);",
        Times(1),
        Kind::Window(Reach::EveryFrameRow),
        "the settle before the act: thirty ticks offered, each pass up to 10 ms, the counts then taken as the base",
    ),
    site(
        RIG,
        "Err(_) if now >= due + PATHS_BOUND => second.set(Second::Unread),",
        Times(1),
        Kind::Bound("the act never read its second stage"),
        "`go` never opened by the act",
    ),
    site(
        RIG,
        "if now < due {",
        Times(1),
        Kind::Hold,
        "the second stage written no sooner than it is due",
    ),
    site(
        RIG,
        "if (signed && answered && drawn) || start.elapsed() >= RELEASE_BOUND {",
        Times(1),
        Kind::FailOpen(Reach::FrameWindows(&["Released", "ReleasedLate"])),
        "the first stage's settle: the check past it reads `signed` and the paths, not `drawn`, so RELEASE_BOUND \
         reached with the act's frame still owed releases the second stage",
    ),
    site(
        RIG,
        "second.set(Second::Due(Instant::now() + hold));",
        Times(1),
        Kind::Hold,
        "ReleasedLate holds the newline LATE_RELEASE, a late cause on purpose; the row ends on its feed and batch",
    ),
    site(
        RIG,
        "while since() < wanted && start.elapsed() < probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::RowEnd,
        "UntilFlips: ends on the flips, \"timed out at the backstop\" past its bound",
    ),
    site(
        RIG,
        "Some(left) if !left.is_zero() => probe::pass(left.as_secs_f64()),",
        Times(2),
        Kind::Hold,
        "a tick offered no sooner than the last one's target: the display link's period",
    ),
    site(
        RIG,
        "due = now.plus(FRAME_PERIOD);",
        Times(2),
        Kind::Hold,
        "the next tick's target, a period on",
    ),
    site(
        RIG,
        "let periods = start.elapsed().as_micros() / FRAME_PERIOD.as_micros();",
        Times(1),
        Kind::Measure,
        "the rig's own clock checked against its period; fails, never waits",
    ),
    site(
        RIG,
        "&& start.elapsed() < probe_call::SENTINEL_BACKSTOP",
        Times(1),
        Kind::Bound("the frame owed before the act was never drawn"),
        "OneAfterFeeds and EventFrames: the window's frame drawn before the act",
    ),
    site(
        RIG,
        "while fed_since() == 0 && start.elapsed() < probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::Bound("the act never fed the core"),
        "the act's first feed",
    ),
    site(
        RIG,
        "while !done() && start.elapsed() < probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::RowEnd,
        "EventFrames: two frames, two feeds and two reads, a label past the bound",
    ),
    site(
        RIG,
        "while fed_since() < 2 && start.elapsed() < probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::RowEnd,
        "OneAfterFeeds: two feeds, a label past the bound",
    ),
    site(
        RIG,
        "while drawn_since() == 0 && start.elapsed() < probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::RowEnd,
        "OneAfterFeeds: the one tick that draws, a label past the bound",
    ),
    site(
        RIG,
        "while marks.is_none() && start.elapsed() < probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::RowEnd,
        "UntilMarked: ends on the marks' frame, a label past the bound",
    ),
    site(
        RIG,
        "if at.elapsed() >= probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::RowEnd,
        "UntilAsked: the feed after the newline, \"bound passed before the path was fed\" past its bound",
    ),
    site(
        RIG,
        "if fed_at.elapsed() >= probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::RowEnd,
        "UntilAsked: the path's batch, \"bound passed before the path's batch\" past its bound",
    ),
    site(
        RIG,
        "RigEnd::Passes => run(rig.attempts.0),",
        Times(1),
        Kind::RowEnd,
        "RigEnd::Passes: `rig.attempts` passes, the rows of ENDS_ON_WALL_TIME",
    ),
    site(
        RIG,
        "while unmet.is_some() && start.elapsed() < probe_call::SENTINEL_BACKSTOP {",
        Times(1),
        Kind::RowEnd,
        "RigEnd::Quiet, QuietOffered and QuietCounted: the row's event and quiet, \"bound passed before quiet\" past the backstop",
    ),
    site(
        RIG,
        "while !signed && start.elapsed() < BOUND {",
        Times(1),
        Kind::Bound("the act never finished"),
        "an act that ends with the pasteboard's clear, waited for to it",
    ),
    site(
        RIG,
        "run(30);",
        Times(1),
        Kind::Window(Reach::FrameRowsWhoseActClears),
        "after the act's clear, thirty passes more, each up to 10 ms, before the counts are printed",
    ),
    // The parent (`tests/event_rig.rs`).
    site(
        EVENT_RIG,
        "line: r\"sleep 2.5; printf",
        Times(1),
        Kind::Hold,
        "the late bell's program, late on purpose past BOUND; the row ends on its sentinel",
    ),
    site(
        EVENT_RIG,
        "else : > \"$HOME/late\"; sleep 0.08; fi; ",
        Times(1),
        Kind::Hold,
        "every other ring late on purpose past SPACING; the row ends on its sentinel",
    ),
    site(
        EVENT_RIG,
        "sleep 0.2; printf '\\033]52;c;!\\007'; sleep 0.1",
        Times(1),
        Kind::Hold,
        "the bell's sentinel written apart from its bell, and the next line held, on purpose; ends on its sentinel",
    ),
    site(
        EVENT_RIG,
        "if start.elapsed() > bound {",
        Times(1),
        Kind::Harness,
        "the parent's wait on a child it then ends",
    ),
    site(
        EVENT_RIG,
        "std::thread::sleep(Duration::from_millis(5));",
        Times(3),
        Kind::Harness,
        "the parent polls a child's exit or a stand-in's file",
    ),
    site(
        EVENT_RIG,
        "while asked.elapsed() < CHILD_GRACE {",
        Times(1),
        Kind::Harness,
        "SIGTERM's grace before SIGKILL",
    ),
    site(
        EVENT_RIG,
        "while :; do sleep 0.01; done",
        Times(1),
        Kind::Harness,
        "a stand-in that runs until it is ended",
    ),
    site(
        EVENT_RIG,
        "start.elapsed() < DOOR_BOUND,",
        Times(1),
        Kind::Harness,
        "the stand-in's trap set, under a bound that fails",
    ),
    site(
        EVENT_RIG,
        "act: \"sleep 30\",",
        Times(1),
        Kind::Hold,
        "a job that outlasts the run, the row about its title; the row's end is its ticks', listed",
    ),
    site(
        EVENT_RIG,
        "act: r\"(sleep 1; ls; printf",
        Times(1),
        Kind::Window(Reach::Named(&[
            "a listing's paths are not held by the batch that named its writer",
        ])),
        "a second's pause before `ls` so the batch that names the listing's writer comes first: a settle on time, \
         which the row's `batches >= 2` then reads",
    ),
    site(
        EVENT_RIG,
        "\"; sleep 0.1; env \",",
        Times(1),
        Kind::Hold,
        "a shell's D row's two commands a tenth of a second apart, so the second comes under the floor the \
         first's asking batch laid: a cause made early on purpose; the rows end on their marks' frame \
         (`UntilMarked`), and a pause past the floor passes them without the floor",
    ),
    // The paths worker's tests (`baud-app/src/probe.rs`): no rig row.
    site(
        PATHS_PROBE,
        "let took = t0.elapsed();",
        Times(1),
        Kind::Measure,
        "a lookup's time, printed",
    ),
    site(
        PATHS_PROBE,
        "let plain_took = t0.elapsed();",
        Times(1),
        Kind::Measure,
        "a resize's time, measured",
    ),
    site(
        PATHS_PROBE,
        "let told_took = t1.elapsed();",
        Times(1),
        Kind::Measure,
        "a resize's time, measured",
    ),
    site(
        PATHS_PROBE,
        "let followed_took = t2.elapsed();",
        Times(1),
        Kind::Measure,
        "a resize's time, measured",
    ),
    site(
        PATHS_PROBE,
        "let deadline = Instant::now() + Duration::from_secs(30);",
        Times(1),
        Kind::Bound("never named"),
        "a worker's naming waited for on the test's own clock; reached, the test fails",
    ),
    site(
        PATHS_PROBE,
        "assert!(Instant::now() < deadline, \"never named\");",
        Times(1),
        Kind::Bound("never named"),
        "that bound, checked",
    ),
    site(
        PATHS_PROBE,
        ".recv_timeout(Duration::from_secs(60))",
        Times(1),
        Kind::Bound("the worker answers every batch"),
        "a worker's answer waited for; reached, the test fails",
    ),
    // The effect probe's doors (`baud-present-macos/src/probe.rs`), on every
    // event row's path.
    site(
        EFFECT_PROBE,
        "if !first && start.elapsed() >= bound {",
        Times(1),
        Kind::Unlabelled,
        "an effect's nested wake, waited for under BOUND with the responder free: reached, the \
         record says Nested::Never and a green row reads Lost. With the responder held the bound \
         is zero, one nested call, the wake held by the window crate until the call returns, so a \
         control's Never is its construction, not the wait's length",
    ),
];

/// A wall-clock wait's line.
const WAITS: &[&str] = &[
    r"\.elapsed\(\)",
    r"Instant::now\(\)\s*[<>+]",
    r"recv_timeout\(",
    r"thread::sleep\(",
    r"\bsleep\s+[0-9.]+",
];

/// **The child's own waits**, read in `rig.rs` alone: a deadline made from
/// `now`, a count of passes of its `offer` and `run` (a pass of one is a
/// step of a loop listed by its condition), a loop of passes, a tick held
/// to its period. Elsewhere those names are a test's own clock or closure.
const CHILD_WAITS: &[&str] = &[
    r"\bnow\s*(?:<|>=|\+)",
    r"\b(?:offer|run)\((?:[02-9]|[1-9][0-9]+|rig\.attempts\.0)\)",
    r"\bfor _ in 0\.\.[0-9]+",
    r"\.plus\(",
    r"probe::pass\(left",
];

/// Every wall-clock wait's line in the rig's files: file, line, text.
fn waits(t: &Tree) -> Vec<(&'static str, usize, String)> {
    let every = Regex::new(&WAITS.join("|")).expect("a pattern");
    let child = Regex::new(&CHILD_WAITS.join("|")).expect("a pattern");
    let mut out = Vec::new();
    for (file, src) in [
        (RIG, &t.rig),
        (EVENT_RIG, &t.event_rig),
        (RIG_CALL, &t.rig_call),
        (PATHS_PROBE, &t.paths_probe),
        (EFFECT_PROBE, &t.effect_probe),
    ] {
        // The paths worker's own deadlines are the app's pace, not a wait:
        // its tests alone are read.
        let from = match file {
            PATHS_PROBE => src
                .skel
                .find("mod tests {")
                .map_or(0, |at| line_of(&src.code, at) - 1),
            _ => 0,
        };
        for (n, line) in src.code.lines().enumerate().skip(from) {
            if every.is_match(line) || (file == RIG && child.is_match(line)) {
                out.push((file, n + 1, line.trim().to_owned()));
            }
        }
    }
    out
}

/// **Every wall-clock wait in the rig's files is classified**, each by
/// one entry of [`SITES`] as often as the entry says; a bound's label is
/// printed in its file. The offenders, windows and bounds that go on, are
/// printed with the rows they reach.
#[test]
fn every_wall_clock_wait_in_the_rig_is_classified() {
    let t = tree();
    let found = waits(&t);
    let mut red = Vec::new();
    let mut times = vec![0_usize; SITES.len()];
    for (file, line, text) in &found {
        // A line an entry spells whole is that entry's; any other, the one
        // entry whose needle it holds.
        let hits: Vec<usize> = match (0..SITES.len())
            .find(|&i| SITES[i].file == *file && SITES[i].needle == text.as_str())
        {
            Some(i) => vec![i],
            None => (0..SITES.len())
                .filter(|&i| SITES[i].file == *file && text.contains(SITES[i].needle))
                .collect(),
        };
        match hits.as_slice() {
            [i] => times[*i] += 1,
            [] => red.push(format!("{file}:{line}: a wait no entry names: {text}")),
            many => red.push(format!(
                "{file}:{line}: named by {} entries: {text}",
                many.len()
            )),
        }
    }
    let src = |file: &str| match file {
        RIG => &t.rig,
        EVENT_RIG => &t.event_rig,
        RIG_CALL => &t.rig_call,
        EFFECT_PROBE => &t.effect_probe,
        _ => &t.paths_probe,
    };
    let rows = rows(&t.event_rig);
    let e = ends(&t).unwrap_or_else(|why| panic!("the child's ends could not be read: {why}"));
    // The arms that sign the act's clear, found: a reach that leaves them
    // out reads a list that is not empty by accident.
    for end in ["Quiet", "QuietOffered", "QuietCounted"] {
        assert!(
            e.signs.iter().any(|s| s == end),
            "RigEnd::{end}'s arm signs the act's clear: {:?}",
            e.signs
        );
    }
    let mut offending = String::new();
    for (i, s) in SITES.iter().enumerate() {
        if Times(times[i]) != s.times {
            red.push(format!(
                "{}: {:?} found {} times, listed {}",
                s.file, s.needle, times[i], s.times.0
            ));
        }
        if let Kind::Within(outer) = s.kind {
            if !SITES.iter().any(|o| o.file == s.file && o.needle == outer) {
                red.push(format!(
                    "{}: {:?} is within {outer:?}, which no entry is",
                    s.file, s.needle
                ));
            }
        }
        if let Kind::Bound(label) = s.kind {
            if !src(s.file).code.contains(&format!("\"{label}")) {
                red.push(format!(
                    "{}: {:?}'s label {label:?} is printed nowhere",
                    s.file, s.needle
                ));
            }
        }
        if let Kind::Window(reach) | Kind::FailOpen(reach) = s.kind {
            let reached: Vec<&str> = rows
                .iter()
                .filter(|r| reaches(reach, r, &e))
                .map(|r| r.name.as_str())
                .collect();
            if reached.is_empty() {
                red.push(format!("{}: {:?} reaches no row", s.file, s.needle));
            }
            let at: Vec<String> = found
                .iter()
                .filter(|(f, _, text)| *f == s.file && text.contains(s.needle))
                .map(|(_, l, _)| l.to_string())
                .collect();
            let _ = writeln!(
                offending,
                "  {}:{} {:?} ({}): {}; {} rows: {reached:?}",
                s.file,
                at.join(","),
                s.needle,
                match s.kind {
                    Kind::FailOpen(_) => "goes on at its bound",
                    _ => "a window",
                },
                s.why,
                reached.len(),
            );
        }
    }
    println!(
        "{} waits found; the sites that end or settle on wall time:\n{offending}",
        found.len()
    );
    assert!(red.is_empty(), "{}", red.join("\n"));
}
