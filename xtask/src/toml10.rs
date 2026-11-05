//! What TOML 1.1 added, refused, so a file the `toml` crate reads is one
//! TOML 1.0 reads too.
//!
//! The manifest and the licence sidecar were read with Python's `tomllib`,
//! which reads TOML 1.0, and are now read with `toml` 1.1.8, which reads
//! 1.1. Every 1.0 document is a 1.1 document, but not the other way round,
//! so the "is not valid TOML" check would have widened. TOML 1.1 added,
//! and `toml` accepts where `tomllib` refused (each probed on both):
//!
//! - an inline table spanning lines, a comment inside one, and a comma
//!   after its last pair;
//! - the escapes `\e` and `\xHH` in a basic string, single-line or
//!   multi-line;
//! - a time, or a date-time, with no seconds.
//!
//! It also refuses a byte order mark before the first line, which TOML
//! 1.0's grammar has no place for: `toml` skips it, and `tomllib`
//! refused it.
//!
//! This scan refuses those and nothing else: it reads the strings,
//! comments and brackets of the document, and leaves every other rule to
//! the parser. An array inside an inline table may still span lines, as
//! TOML 1.0 allows newlines between the braces "where they are valid
//! within a value".

/// Where a refusal is, from 1, as the parsers count lines and columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Place {
    pub line: crate::html::Count,
    pub column: crate::html::Count,
}

/// What TOML 1.0 does not allow was found, and where.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Refusal {
    pub what: &'static str,
    pub at: Place,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}, which TOML 1.0 does not allow (at line {}, column {})",
            self.what, self.at.line.0, self.at.column.0
        )
    }
}

/// The bracket a value is inside.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Open {
    /// `[` or `[[` of a table header.
    Header,
    /// `[` of an array.
    Array,
    /// `{` of an inline table.
    Inline,
}

/// The first form in `text` that TOML 1.0 does not allow, if any.
pub fn first_refusal(text: &str) -> Option<Refusal> {
    if text.starts_with('\u{feff}') {
        return Some(Refusal {
            what: "a byte order mark",
            at: Place {
                line: crate::html::Count(1),
                column: crate::html::Count(1),
            },
        });
    }
    let chars: Vec<char> = text.chars().collect();
    let mut stack: Vec<Open> = Vec::new();
    let mut line = 1;
    let mut col = 0;
    let mut line_start = true;
    let mut i = 0;
    let place = |line: usize, col: usize| Place {
        line: crate::html::Count(line),
        column: crate::html::Count(col),
    };
    let at = |k: usize| chars.get(k).copied();
    let starts = |k: usize, s: &str| s.chars().enumerate().all(|(n, c)| at(k + n) == Some(c));
    // Advances over chars[i] and keeps the line and column.
    macro_rules! step {
        () => {{
            if chars[i] == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
            i += 1;
        }};
    }
    while i < chars.len() {
        let c = chars[i];
        let inline = stack.last() == Some(&Open::Inline);
        match c {
            '\n' => {
                if inline {
                    return Some(Refusal {
                        what: "an inline table spans lines",
                        at: place(line, col + 1),
                    });
                }
                step!();
                line_start = true;
                continue;
            }
            '#' => {
                if inline {
                    return Some(Refusal {
                        what: "a comment inside an inline table",
                        at: place(line, col + 1),
                    });
                }
                while i < chars.len() && chars[i] != '\n' {
                    step!();
                }
                continue;
            }
            ' ' | '\t' | '\r' => {
                step!();
                continue;
            }
            _ => {}
        }
        let first = line_start;
        line_start = false;
        match c {
            '"' | '\'' => {
                let triple = starts(i, if c == '"' { "\"\"\"" } else { "'''" });
                let basic = c == '"';
                let quotes = if triple { 3 } else { 1 };
                for _ in 0..quotes {
                    step!();
                }
                loop {
                    if i >= chars.len() {
                        return None;
                    }
                    let d = chars[i];
                    if basic && d == '\\' {
                        if matches!(at(i + 1), Some('e' | 'x')) {
                            return Some(Refusal {
                                what: if at(i + 1) == Some('e') {
                                    "the escape \\e"
                                } else {
                                    "the escape \\x"
                                },
                                at: place(line, col + 1),
                            });
                        }
                        step!();
                        if i < chars.len() {
                            step!();
                        }
                        continue;
                    }
                    if d == c && (!triple || starts(i, if basic { "\"\"\"" } else { "'''" })) {
                        // A closing triple may carry up to two more quotes
                        // of the string's own; they are content.
                        let mut run = 0;
                        while at(i + run) == Some(c) {
                            run += 1;
                        }
                        let take = if triple { run.min(5) } else { 1 };
                        for _ in 0..take {
                            step!();
                        }
                        break;
                    }
                    if !triple && d == '\n' {
                        break;
                    }
                    step!();
                }
                continue;
            }
            '[' => {
                stack.push(
                    if (first && stack.is_empty()) || stack.last() == Some(&Open::Header) {
                        Open::Header
                    } else {
                        Open::Array
                    },
                );
            }
            '{' => stack.push(Open::Inline),
            ']' | '}' => {
                stack.pop();
            }
            ',' if inline => {
                let mut k = i + 1;
                while matches!(at(k), Some(' ' | '\t')) {
                    k += 1;
                }
                if at(k) == Some('}') {
                    return Some(Refusal {
                        what: "a comma after an inline table's last pair",
                        at: place(line, col + 1),
                    });
                }
            }
            d if d.is_ascii_digit() => {
                // `HH:MM` that is not the end of an offset or of `HH:MM:SS`,
                // and has no seconds after it.
                let before = i.checked_sub(1).and_then(at);
                let hhmm = at(i + 1).is_some_and(|x| x.is_ascii_digit())
                    && at(i + 2) == Some(':')
                    && at(i + 3).is_some_and(|x| x.is_ascii_digit())
                    && at(i + 4).is_some_and(|x| x.is_ascii_digit());
                let lead =
                    !before.is_some_and(|b| b.is_ascii_digit() || matches!(b, ':' | '+' | '-'));
                if hhmm && lead && at(i + 5) != Some(':') {
                    return Some(Refusal {
                        what: "a time with no seconds",
                        at: place(line, col + 1),
                    });
                }
            }
            _ => {}
        }
        step!();
    }
    None
}

/// A parse error as `tomllib` shaped its line: what is wrong, then where,
/// `(at line L, column C)`, both from 1 and the column in characters. The
/// words are the `toml` crate's, not `tomllib`'s.
pub fn described(text: &str, e: &toml::de::Error) -> String {
    let what = e.message().trim().to_string();
    let Some(span) = e.span() else { return what };
    let before = text.get(..span.start).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("{what} (at line {line}, column {column})")
}
