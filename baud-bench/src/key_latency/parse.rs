//! What the app and the helper print, read strictly: the `keylat:` records,
//! the `post` lines and the `front` line.

use super::py;

/// A stamp, or a span between two, in nanoseconds. `i128` so the reader
/// has no overflow the app's stamps could reach.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Ns(pub i128);

/// A key's sequence number, the helper's or the app's.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Seq(pub i128);

/// A number of things counted.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Count(pub usize);

impl std::fmt::Display for Count {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::ops::AddAssign for Count {
    fn add_assign(&mut self, rhs: Count) {
        self.0 += rhs.0;
    }
}

/// The offset tokens a record carries, in the order it prints them.
pub const OFFSETS: [&str; 10] = [
    "handled", "written", "pred", "echo", "fed", "commit", "landed", "shown", "vsync", "vsync_hi",
];

/// The tokens a record may omit: `pred` is printed by every app from the
/// predictor on and by no app before it, and a baseline arm is the app
/// before it. A missing optional token reads as `-`, a leg that never
/// happened.
pub const OPTIONAL: [&str; 1] = ["pred"];

/// The token saying how the echo judged a predicted key. Not an offset, and
/// optional: a line without it reads as never judged.
pub const PREDOK: &str = "predok";

/// A key the predictor declined: `keylat: refused <gate>`, one line a key,
/// no record.
pub const PREDICT_REFUSED: &str = "keylat: refused ";

/// A prediction made under a condition worth counting.
pub const PREDICT_NOTED: &str = "keylat: noted ";

/// The line a landing whose GPU end stamp is off the clock prints.
pub const REFUSAL_PREFIX: &str = "keylat: landing refused";

/// How the echo judged a predicted key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PredOk {
    /// The echo confirmed it.
    Yes,
    /// The echo contradicted it.
    No,
    /// Never judged before the record closed.
    Unjudged,
}

impl PredOk {
    /// What the record prints.
    pub fn token(self) -> &'static str {
        match self {
            PredOk::Yes => "yes",
            PredOk::No => "no",
            PredOk::Unjudged => "-",
        }
    }
}

/// The absolute stamp of every stage of one key, `None` for a leg that did
/// not happen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stamps {
    /// The responder ran.
    pub handled: Option<Ns>,
    /// The key's bytes were written to the pty.
    pub written: Option<Ns>,
    /// The key's echo was drawn before the pty answered.
    pub pred: Option<Ns>,
    /// The echo came back.
    pub echo: Option<Ns>,
    /// The reader thread fed the core.
    pub fed: Option<Ns>,
    /// The frame was committed.
    pub commit: Option<Ns>,
    /// The GPU finished.
    pub landed: Option<Ns>,
    /// The surface was set on the layer.
    pub shown: Option<Ns>,
    /// The display-link target before the surface was set.
    pub vsync: Option<Ns>,
    /// The display-link target after it.
    pub vsync_hi: Option<Ns>,
}

impl Stamps {
    /// Every stage with its token, in `OFFSETS`'s order.
    pub fn named(&self) -> [(&'static str, Option<Ns>); 10] {
        [
            ("handled", self.handled),
            ("written", self.written),
            ("pred", self.pred),
            ("echo", self.echo),
            ("fed", self.fed),
            ("commit", self.commit),
            ("landed", self.landed),
            ("shown", self.shown),
            ("vsync", self.vsync),
            ("vsync_hi", self.vsync_hi),
        ]
    }

    fn named_mut(&mut self) -> [(&'static str, &mut Option<Ns>); 10] {
        [
            ("handled", &mut self.handled),
            ("written", &mut self.written),
            ("pred", &mut self.pred),
            ("echo", &mut self.echo),
            ("fed", &mut self.fed),
            ("commit", &mut self.commit),
            ("landed", &mut self.landed),
            ("shown", &mut self.shown),
            ("vsync", &mut self.vsync),
            ("vsync_hi", &mut self.vsync_hi),
        ]
    }
}

/// One `keylat: seq=...` record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Record {
    /// The app's sequence number.
    pub seq: Seq,
    /// The event's own stamp.
    pub event: Ns,
    /// Every later stage, absolute.
    pub stamps: Stamps,
    /// How the echo judged it.
    pub predok: PredOk,
}

/// A record that is not what the app promises to emit: a known token
/// missing, or a value that does not parse.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KeylatParseError(pub String);

impl std::fmt::Display for KeylatParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The value of `+<us>`, `\+\d+(\.\d+)?`, in microseconds.
fn offset_value(v: &str) -> Option<f64> {
    let body = v.strip_prefix('+')?;
    let (whole, tenth) = match body.split_once('.') {
        Some((w, t)) => (w, Some(t)),
        None => (body, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || !tenth.is_none_or(digits) {
        return None;
    }
    body.parse().ok()
}

/// A `keylat: seq=... event=... <offset fields>` line, each stage made
/// absolute.
///
/// `Ok(None)` for any line that is not a per-key record: the `N keys open`
/// report, the `landing refused` line and all non-keylat output. `Err` for a
/// record that is one (it carries `seq=`) but lacks a token that is not
/// optional, or carries a value that is neither `+<us>` nor `-`. Tokens it
/// does not know are ignored; an optional token that is absent reads as `-`.
pub fn parse_keylat_line(line: &str) -> Result<Option<Record>, KeylatParseError> {
    let line = py::strip(line);
    let Some(rest) = line.strip_prefix("keylat:") else {
        return Ok(None);
    };
    let mut toks: Vec<(&str, &str)> = Vec::new();
    for tok in py::split(rest) {
        let Some((k, v)) = tok.split_once('=') else {
            return Ok(None); // "keylat: 3 keys open", "keylat: landing refused ..."
        };
        toks.push((k, v));
    }
    // A token given twice is the last one's, as a dict keeps it.
    let token = |name: &str| toks.iter().rev().find(|(k, _)| *k == name).map(|(_, v)| *v);
    let Some(seq) = token("seq") else {
        return Ok(None);
    };
    let missing: Vec<&str> = ["seq", "event"]
        .into_iter()
        .chain(OFFSETS)
        .filter(|k| token(k).is_none() && !OPTIONAL.contains(k))
        .collect();
    if !missing.is_empty() {
        return Err(KeylatParseError(format!(
            "missing {}: {line}",
            missing.join(",")
        )));
    }
    let (Some(seq), Some(event)) = (py::int(seq), token("event").and_then(py::int)) else {
        return Err(KeylatParseError(format!("seq/event not integers: {line}")));
    };
    let mut stamps = Stamps::default();
    for (name, slot) in stamps.named_mut() {
        let v = token(name).unwrap_or("-");
        if v == "-" {
            continue; // a leg that never happened
        }
        let Some(us) = offset_value(v) else {
            return Err(KeylatParseError(format!(
                "{name}={v} is neither +<us> nor -: {line}"
            )));
        };
        // `+<us>`: microseconds from event, one decimal; ties go to even.
        let ns = (us * 1000.0).round_ties_even();
        if ns.abs() >= 1e36 {
            return Err(KeylatParseError(format!(
                "{name}={v} is out of range: {line}"
            )));
        }
        *slot = Some(Ns(event + ns as i128));
    }
    let predok = match token(PREDOK).unwrap_or("-") {
        "yes" => PredOk::Yes,
        "no" => PredOk::No,
        "-" => PredOk::Unjudged,
        ok => {
            return Err(KeylatParseError(format!(
                "{PREDOK}={ok} is not yes, no or -: {line}"
            )));
        }
    };
    Ok(Some(Record {
        seq: Seq(seq),
        event: Ns(event),
        stamps,
        predok,
    }))
}

/// What every line of one run comes to.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Parsed {
    /// The records, in order.
    pub records: Vec<Record>,
    /// The parse failures' messages.
    pub failures: Vec<String>,
    /// How many `landing refused` lines.
    pub refusals: Count,
    /// The `N keys open` lines and the predictor's refused and noted lines.
    pub reports: Vec<String>,
}

/// Every line of one run, sorted into a [`Parsed`].
pub fn parse_keylat_lines<S: AsRef<str>>(lines: &[S]) -> Parsed {
    let mut out = Parsed::default();
    for line in lines {
        let s = py::strip(line.as_ref());
        if s.starts_with(REFUSAL_PREFIX) {
            out.refusals += Count(1);
            continue;
        }
        match parse_keylat_line(s) {
            Err(e) => out.failures.push(e.0),
            Ok(Some(rec)) => out.records.push(rec),
            Ok(None) => {
                let open = s.starts_with("keylat:") && s.ends_with("keys open");
                // A key the predictor declined, with the gate that did, or
                // one it predicted under a condition worth counting.
                if open || s.starts_with(PREDICT_REFUSED) || s.starts_with(PREDICT_NOTED) {
                    out.reports.push(s.to_string());
                }
            }
        }
    }
    out
}

/// One key the helper posted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Post {
    /// The helper's sequence number.
    pub seq: Seq,
    /// When it posted the key.
    pub ns: Ns,
    /// The letter.
    pub ch: String,
}

/// `post <seq> <ns> <char>`, or `None`.
pub fn parse_post_line(line: &str) -> Option<Post> {
    let mut toks = py::split(line);
    if toks.next()? != "post" {
        return None;
    }
    let digits = |s: &str| {
        if s.bytes().all(|b| b.is_ascii_digit()) {
            py::int(s)
        } else {
            None
        }
    };
    let seq = digits(toks.next()?)?;
    let ns = digits(toks.next()?)?;
    let ch = toks.next()?;
    if toks.next().is_some() {
        return None;
    }
    Some(Post {
        seq: Seq(seq),
        ns: Ns(ns),
        ch: ch.to_string(),
    })
}

/// The helper's `--front` line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Front {
    /// Who owns the frontmost layer-0 window.
    pub window_pid: i128,
    /// Its name.
    pub window_name: String,
    /// The frontmost application.
    pub app_pid: i128,
    /// That application's name.
    pub app_name: String,
}

/// `-?\d+` at the head of `s`: the number and what follows it.
fn leading_int(s: &str) -> Option<(i128, &str)> {
    let digits_at = usize::from(s.starts_with('-'));
    let len = s[digits_at..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if len == 0 {
        return None;
    }
    let end = digits_at + len;
    Some((py::int(&s[..end])?, &s[end..]))
}

/// `s` without its leading whitespace, if it has any.
fn after_space(s: &str) -> Option<&str> {
    let rest = s.trim_start_matches(py::is_space);
    (rest.len() < s.len()).then_some(rest)
}

/// `\s+app\s+(-?\d+)\s+(.*)$` at the head of `s`.
fn app_part(s: &str) -> Option<(i128, &str)> {
    let s = after_space(s)?.strip_prefix("app")?;
    let (pid, s) = leading_int(after_space(s)?)?;
    Some((pid, after_space(s)?))
}

/// `front <pid> <name> app <pid> <name>`, or `None`. A name may hold spaces:
/// the window's ends at the first `app <pid> ` after it.
pub fn parse_front_line(line: &str) -> Option<Front> {
    let s = py::strip(line).strip_prefix("front")?;
    let (window_pid, tail) = leading_int(after_space(s)?)?;
    let indent = tail.chars().take_while(|c| py::is_space(*c)).count();
    // The first `\s+` gives back what it took only if nothing matches with
    // all of it, and the name then begins with the rest.
    for taken in (1..=indent).rev() {
        let start: usize = tail.chars().take(taken).map(char::len_utf8).sum();
        let name = &tail[start..];
        for (end, _) in name.char_indices() {
            if let Some((app_pid, app_name)) = app_part(&name[end..]) {
                return Some(Front {
                    window_pid,
                    window_name: name[..end].to_string(),
                    app_pid,
                    app_name: app_name.to_string(),
                });
            }
        }
    }
    None
}
