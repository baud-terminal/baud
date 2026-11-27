//! Count the loops on the ingest path that *search* rather than look up.
//!
//! The architecture is a compile-time database and a runtime that indexes
//! it. A scan is the shape that says a question was never put to the table.
//!
//! Not counted, and why:
//!   * `const fn` builders -- they run at build time and *are* the database.
//!   * `feed`'s `while done < n` -- that is the iteration, not a search.
//!   * bounded work loops -- `Paint::wrapped` walks the rows a run crosses
//!     and `put_run` decodes four characters a step; neither is looking for
//!     anything.
//!
//! Counted: a loop inside a run scanner, i.e. one that advances a byte
//! cursor to find how far a run goes. Those exist only because the graph is
//! one byte wide, so an edge can describe a single byte and nothing more.
//!
//! **The body's extent is read by counting braces on each line, and that
//! count includes braces inside strings and comments**; a line is matched
//! after stripping it. Both are the rule as it was measured, kept as it is:
//! changing what counts as the body is a change to the gate, made on its
//! own and measured, not in a port.

use std::sync::LazyLock;

use regex::Regex;

use crate::text;

/// The run scanners whose loops are counted.
pub const SCANNERS: [&str; 2] = ["run_of_text", "run_of"];

/// The file they live in.
pub const PATH: &str = "crates/baud-core/src/write.rs";

/// One counted loop.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scan {
    /// The scanner it is in.
    pub scanner: String,
    /// Its line, from 1.
    pub line: Line,
    /// The stripped line, cut to 60 characters.
    pub text: String,
}

/// A line number, from 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Line(pub usize);

/// The scan limit: the count may go down and must never go up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limit(pub usize);

static HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(pub\(crate\) )?fn (\w+)").expect("the head pattern"));

/// Every `while` inside the scanners' bodies in `src`, in order.
pub fn scans(src: &str) -> Vec<Scan> {
    let lines = text::splitlines(src);
    let mut found = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let name = HEAD
            .captures(text::strip(lines[i]))
            .and_then(|m| m.get(2))
            .map(|m| m.as_str())
            .filter(|n| SCANNERS.contains(n));
        if let Some(name) = name {
            let mut depth: isize = 0;
            let mut j = i;
            while j < lines.len() {
                depth += brace_delta(lines[j]);
                let body = text::strip(lines[j]);
                if body.starts_with("while ") && j > i {
                    found.push(Scan {
                        scanner: name.to_string(),
                        line: Line(j + 1),
                        text: body.chars().take(60).collect(),
                    });
                }
                if depth <= 0 && j > i {
                    break;
                }
                j += 1;
            }
            i = j;
        }
        i += 1;
    }
    found
}

/// Opening braces less closing ones on a line, wherever they stand.
fn brace_delta(line: &str) -> isize {
    line.chars().fold(0, |d, c| match c {
        '{' => d + 1,
        '}' => d - 1,
        _ => d,
    })
}

/// The report and whether it passes: one line per loop, then the verdict.
pub fn report(found: &[Scan], limit: Limit) -> (String, bool) {
    let mut out = String::new();
    for s in found {
        out += &format!("  ..   {PATH}:{}  {}: {}\n", s.line.0, s.scanner, s.text);
    }
    let ok = found.len() <= limit.0;
    out += &format!(
        "  {} ingest scans: {} (limit {}, target 0)\n",
        if ok { "ok  " } else { "FAIL" },
        found.len(),
        limit.0
    );
    (out, ok)
}
