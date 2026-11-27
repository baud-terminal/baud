//! **`config.toml.example`, read as the file it is**: every key commented
//! out at its default, under a comment that explains it. The build reads it
//! with [`entries`] and stops on the first line that breaks that shape
//! (`build.rs`), so a malformed example is a build error rather than a file
//! written to every new user; the app's tests read it with the same code
//! (`config.rs`, `mod example`). First-party and dependency-free, because
//! a build script shares it through `#[path]`, as `build_id.rs` is shared.
//!
//! Each line of the example is one of:
//!
//! * **blank**, which ends a paragraph;
//! * **a key**: `# ` and then, from the third column, a name of `a-z`,
//!   `0-9` and `_`, ` = ` and a whole value -- a quoted string, a bare word
//!   or number, or a list, which may run over the lines after it, each
//!   starting with `#`, to the one holding its `]`; a comment may follow;
//! * **a section**: `# [name]`, a dotted name such as `message.copy` for a
//!   section within the one before it, a comment allowed after it;
//! * **prose**: any other line starting with `#`. An example written
//!   inside the prose is indented past the third column, so it is never
//!   read as a key.
//!
//! A line that does not start with `#` is a stop: nothing in the file may
//! be in force, or the written file would freeze its users on the value of
//! the baud that wrote it.

/// **A line of the example**, numbered from one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineNo(pub usize);

impl core::fmt::Display for LineNo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// One key of the example.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// The section it sits in; empty for the top level.
    pub section: String,
    pub key: String,
    /// Its first line.
    pub line: LineNo,
    /// Its lines with the `#` taken off, as uncommenting them writes them.
    pub text: String,
}

/// Why the example is not the file baud may write.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Stop {
    /// A line that is neither blank nor a comment: it would be in force.
    Active { line: LineNo },
    /// A key with no prose above it in its paragraph, neither on the line
    /// before it (a section line between them aside) nor naming it.
    Unexplained { line: LineNo, key: String },
    /// A key given twice in one section.
    Twice {
        line: LineNo,
        section: String,
        key: String,
        first: LineNo,
    },
    /// A section given a second time.
    Again {
        line: LineNo,
        section: String,
        first: LineNo,
    },
    /// A dotted section that does not follow its own parent or a sibling.
    Orphan {
        line: LineNo,
        section: String,
        parent: String,
    },
    /// A list opened on this line and never closed.
    Unclosed { line: LineNo, key: String },
}

impl Stop {
    /// The sentence the build stops with.
    pub fn said(&self) -> String {
        match self {
            Stop::Active { line } => format!(
                "line {line} is in force: every line is a comment, so the written file changes nothing"
            ),
            Stop::Unexplained { line, key } => {
                format!("line {line}: `{key}` has no comment above it that explains it")
            }
            Stop::Twice {
                line,
                section,
                key,
                first,
            } => format!("line {line}: [{section}] {key} is given again, first on line {first}"),
            Stop::Again {
                line,
                section,
                first,
            } => format!("line {line}: [{section}] is given again, first on line {first}"),
            Stop::Orphan {
                line,
                section,
                parent,
            } => format!(
                "line {line}: [{section}] is out of place: it follows neither [{parent}] nor a section within it"
            ),
            Stop::Unclosed { line, key } => {
                format!("line {line}: the list `{key}` opens is never closed")
            }
        }
    }
}

/// What one line of the example is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind<'a> {
    Blank,
    /// A bare `#`: part of its paragraph, and no explanation.
    Rule,
    Prose(&'a str),
    Section(&'a str),
    /// A key, and whether its value is a list that runs past this line.
    Key(&'a str, Runs),
    Active,
}

/// Whether a key's list closes on its own line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Runs {
    Closed,
    Open,
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Nothing after a value but space and, perhaps, a comment.
fn ends(rest: &str) -> bool {
    let rest = rest.trim();
    rest.is_empty() || rest.starts_with('#')
}

/// Whether `v` is a whole value, and whether a list it opens runs on.
fn value(v: &str) -> Option<Runs> {
    if let Some(inner) = v.strip_prefix('"') {
        let close = inner.find('"')?;
        return ends(&inner[close + 1..]).then_some(Runs::Closed);
    }
    if v.starts_with('[') {
        return Some(match v.rfind(']') {
            Some(close) if ends(&v[close + 1..]) => Runs::Closed,
            Some(_) => return None,
            None => Runs::Open,
        });
    }
    let word = v
        .find(|c: char| !(c.is_ascii_alphanumeric() || "_.+-".contains(c)))
        .unwrap_or(v.len());
    (word > 0 && ends(&v[word..])).then_some(Runs::Closed)
}

fn kind(line: &str) -> Kind<'_> {
    if line.trim().is_empty() {
        return Kind::Blank;
    }
    let Some(rest) = line.strip_prefix('#') else {
        return Kind::Active;
    };
    if rest.trim().is_empty() {
        return Kind::Rule;
    }
    let Some(body) = rest.strip_prefix(' ').filter(|b| !b.starts_with(' ')) else {
        return Kind::Prose(rest);
    };
    let section = body
        .strip_prefix('[')
        .and_then(|inner| inner.split_once(']'))
        .filter(|(name, after)| name.split('.').all(is_name) && ends(after));
    if let Some((name, _)) = section {
        return Kind::Section(name);
    }
    let key = body
        .split_once(" = ")
        .filter(|(key, _)| is_name(key))
        .and_then(|(key, v)| Some((key, value(v)?)));
    match key {
        Some((key, runs)) => Kind::Key(key, runs),
        None => Kind::Prose(rest),
    }
}

/// Whether `prose` holds `key` as a word of its own.
fn names(prose: &str, key: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    prose.split(|c: char| !word(c)).any(|w| w == key)
}

/// **Every key of the example, in order**, or the first line that is not
/// the shape the file is written in.
pub fn entries(text: &str) -> Result<Vec<Entry>, Stop> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<Entry> = Vec::new();
    let mut sections: Vec<(String, LineNo)> = Vec::new();
    let mut section = String::new();
    // The prose of the paragraph so far, and whether the line before was
    // prose (a section line between them aside).
    let mut paragraph: Vec<&str> = Vec::new();
    let mut after_prose = false;
    let mut i = 0;
    while i < lines.len() {
        let n = LineNo(i + 1);
        match kind(lines[i]) {
            Kind::Active => return Err(Stop::Active { line: n }),
            Kind::Blank => {
                paragraph.clear();
                after_prose = false;
            }
            Kind::Rule => after_prose = false,
            Kind::Prose(p) => {
                paragraph.push(p);
                after_prose = true;
            }
            Kind::Section(name) => {
                if let Some((_, first)) = sections.iter().find(|(s, _)| s == name) {
                    return Err(Stop::Again {
                        line: n,
                        section: name.to_string(),
                        first: *first,
                    });
                }
                if let Some((parent, _)) = name.rsplit_once('.') {
                    let within = section == parent
                        || section
                            .strip_prefix(parent)
                            .is_some_and(|r| r.starts_with('.'));
                    if !within {
                        return Err(Stop::Orphan {
                            line: n,
                            section: name.to_string(),
                            parent: parent.to_string(),
                        });
                    }
                }
                sections.push((name.to_string(), n));
                section = name.to_string();
            }
            Kind::Key(key, runs) => {
                if !after_prose && !paragraph.iter().any(|p| names(p, key)) {
                    return Err(Stop::Unexplained {
                        line: n,
                        key: key.to_string(),
                    });
                }
                if let Some(first) = out.iter().find(|e| e.section == section && e.key == key) {
                    return Err(Stop::Twice {
                        line: n,
                        section: section.clone(),
                        key: key.to_string(),
                        first: first.line,
                    });
                }
                let mut text = lines[i][2..].to_string();
                if runs == Runs::Open {
                    loop {
                        i += 1;
                        let Some(next) = lines.get(i).and_then(|l| l.strip_prefix('#')) else {
                            return Err(Stop::Unclosed {
                                line: n,
                                key: key.to_string(),
                            });
                        };
                        text.push('\n');
                        text.push_str(next);
                        if next.contains(']') {
                            break;
                        }
                    }
                }
                out.push(Entry {
                    section: section.clone(),
                    key: key.to_string(),
                    line: n,
                    text,
                });
                after_prose = false;
            }
        }
        i += 1;
    }
    Ok(out)
}
