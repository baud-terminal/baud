//! The patterns the Linux tools read a compositor's log with, written out:
//! each method below is one atom of a pattern, in order, and each pattern is
//! one no atom of which has to give back what it took, so none backtracks.
//!
//! `\w` is `char::is_alphanumeric` and `_`, `\s` is `py::is_space`, and `\d`
//! is an ASCII digit: libwayland, the benches and `int` below write nothing
//! else there.

use crate::key_latency::py;

/// `\w`.
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `\d`.
pub fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// `[\d.]`.
pub fn is_number(c: char) -> bool {
    c.is_ascii_digit() || c == '.'
}

/// How many of something a tool counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count(pub usize);

/// A place in a line.
#[derive(Clone, Copy)]
pub struct Scan<'a>(pub &'a str);

impl<'a> Scan<'a> {
    /// The text `t`.
    pub fn lit(&mut self, t: &str) -> Option<()> {
        self.0 = self.0.strip_prefix(t)?;
        Some(())
    }

    /// One or more of a class: `\w+`.
    pub fn some(&mut self, class: impl Fn(char) -> bool) -> Option<&'a str> {
        let rest = self.0.trim_start_matches(class);
        let took = self.0.get(..self.0.len() - rest.len())?;
        if took.is_empty() {
            return None;
        }
        self.0 = rest;
        Some(took)
    }

    /// Any number of a class: `\s*`.
    pub fn any(&mut self, class: impl Fn(char) -> bool) {
        self.0 = self.0.trim_start_matches(class);
    }
}

/// One message of libwayland's debug log, without its timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message<'a> {
    /// The arrow: the compositor sent it. Otherwise the client asked it.
    pub event: bool,
    /// The interface.
    pub iface: &'a str,
    /// The object's number, as the log wrote it.
    pub id: &'a str,
    /// The request or event.
    pub name: &'a str,
    /// What is between the outer parentheses.
    pub args: &'a str,
}

/// The rest of a line after its timestamp's `\s+`:
/// `(?:\{[^}]*\}\s+)?(->\s+)?(\w+)[@#](\d+)\.(\w+)\((.*)\)\s*$`.
fn message_at(mut s: Scan<'_>) -> Option<Message<'_>> {
    if s.0.starts_with('{') {
        let close = s.0.find('}')?;
        s.0 = s.0.get(close + 1..)?;
        s.some(py::is_space)?;
    }
    let event = s.0.starts_with("->");
    if event {
        s.lit("->")?;
        s.some(py::is_space)?;
    }
    let iface = s.some(is_word)?;
    if s.lit("@").is_none() {
        s.lit("#")?;
    }
    let id = s.some(is_digit)?;
    s.lit(".")?;
    let name = s.some(is_word)?;
    s.lit("(")?;
    // `(.*)\)\s*$`: the last `)`, with only space after it. The line holds
    // no `\n`, the one thing `.` will not take.
    let args = s.0.trim_end_matches(py::is_space).strip_suffix(')')?;
    Some(Message {
        event,
        iface,
        id,
        name,
        args,
    })
}

/// A line that starts `^\[[\s\d:.]+\]\s+`: milliseconds, or from weston a
/// time of day.
pub fn message_timed(line: &str) -> Option<Message<'_>> {
    let mut s = Scan(line);
    s.lit("[")?;
    s.some(|c| py::is_space(c) || is_number(c) || c == ':')?;
    s.lit("]")?;
    s.some(py::is_space)?;
    message_at(s)
}

/// A line that starts `^\[\s*[\d.]+\]\s+`: milliseconds alone.
pub fn message_ms(line: &str) -> Option<Message<'_>> {
    let mut s = Scan(line);
    s.lit("[")?;
    s.any(py::is_space);
    s.some(is_number)?;
    s.lit("]")?;
    s.some(py::is_space)?;
    message_at(s)
}

/// The number in the first `<iface>[@#](\d+)`, anywhere in `args`.
pub fn numbered<'a>(args: &'a str, iface: &str) -> Option<&'a str> {
    args.match_indices(iface).find_map(|(at, _)| {
        let mut s = Scan(args.get(at + iface.len()..)?);
        if s.lit("@").is_none() {
            s.lit("#")?;
        }
        s.some(is_digit)
    })
}

/// `float(s)`: what `f64`'s parser takes, around space and with underscores
/// between digits.
pub fn float(s: &str) -> Option<f64> {
    let s = py::strip(s);
    let bytes = s.as_bytes();
    let joined = bytes.iter().enumerate().all(|(i, b)| {
        *b != b'_'
            || (i > 0
                && bytes[i - 1].is_ascii_digit()
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
    });
    if !joined {
        return None;
    }
    s.replace('_', "").parse().ok()
}

/// `f"{x:.{places}f}"`.
pub fn fixed(x: f64, places: usize) -> String {
    if x.is_nan() {
        "nan".to_string()
    } else {
        format!("{x:.places$}")
    }
}

/// `round(x)` of a float: ties to even, and none of a value with no integer.
pub fn round(x: f64) -> Option<i128> {
    if !x.is_finite() {
        return None;
    }
    // A float past `i128` is out of every range the callers pack into.
    let r = x.round_ties_even();
    (r.abs() < 1e38).then_some(r as i128)
}

/// A list of integers as `print` writes one.
pub fn list(items: &[i128]) -> String {
    let inner: Vec<String> = items.iter().map(i128::to_string).collect();
    format!("[{}]", inner.join(", "))
}
