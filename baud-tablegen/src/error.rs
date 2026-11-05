//! What a build reports: every failure at once, each at the file, line and
//! column of the row it is about.

use std::fmt;

/// What went wrong, so a test names the failure rather than matching its
/// words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Line 1 is not the header the build expects: another file, or another
    /// version of it, wired in.
    Header,
    /// The file could not be read.
    Read,
    /// A code point that is not four to six hexadecimal digits (UAX #44
    /// section 4.2.2, `[[source: uax44]]`), or a range that runs backwards.
    Malformed,
    /// A code point past U+10FFFF.
    PastLast,
    /// A row with the wrong number of fields.
    FieldCount,
    /// A UnicodeData.txt `<..., First>` line whose next line is not its
    /// `<..., Last>`, or a `Last` with no `First` (UAX #44 section 4.2.3).
    FirstWithoutLast,
    /// A value outside the property's closed set.
    UnknownValue,
    /// One key given twice.
    Duplicate,
    /// Two data rows of one file covering one code point.
    Overlap,
    /// A count the table was compiled with, missed: a row dropped or added.
    Count,
    /// A known code point that does not have the value the build checks it
    /// for.
    Control,
    /// A rule of the table's encoding broken: a value that does not fit its
    /// bits, a pair that is not symmetric.
    Invariant,
}

/// A place in a file, both counted from 1; the column counts characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// One failure.
#[derive(Clone, Debug)]
pub struct Error {
    pub kind: Kind,
    /// The file, as the crate holds it (`data/unicode-unicodedata.txt`), or
    /// the table, for a failure no one row caused.
    pub path: String,
    pub pos: Option<Pos>,
    pub message: String,
    /// The row the failure is about, as written.
    pub row: Option<String>,
}

impl Error {
    pub fn new(kind: Kind, path: &str, message: impl Into<String>) -> Error {
        Error {
            kind,
            path: path.to_string(),
            pos: None,
            message: message.into(),
            row: None,
        }
    }

    /// The same failure, placed at `pos` on the row `row`.
    pub fn at(mut self, pos: Pos, row: &str) -> Error {
        self.pos = Some(pos);
        self.row = Some(row.to_string());
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path)?;
        if let Some(p) = self.pos {
            write!(f, ":{}:{}", p.line, p.col)?;
        }
        write!(f, ": {}", self.message)?;
        if let Some(row) = &self.row {
            write!(f, ": {row:?}")?;
        }
        Ok(())
    }
}

/// Every failure of a build, collected so that one run reports them all.
#[derive(Debug, Default)]
pub struct Errors(Vec<Error>);

impl Errors {
    pub fn push(&mut self, e: Error) {
        self.0.push(e);
    }

    /// `r`'s value, or `None` with its failure recorded.
    pub fn take<T>(&mut self, r: Result<T, Error>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.0.push(e);
                None
            }
        }
    }

    pub fn extend(&mut self, other: Errors) {
        self.0.extend(other.0);
    }

    /// A positive control on a count: `got` of `what` in `path`, where the
    /// table was compiled with `want`. After an upgrade of the data this
    /// fails on purpose, and says which count to re-read.
    pub fn count(&mut self, path: &str, what: &str, got: usize, want: usize) {
        if got != want {
            self.0.push(Error::new(
                Kind::Count,
                path,
                format!("{got} {what}, {want} expected"),
            ));
        }
    }

    /// Records `e` unless `holds`.
    pub fn require(&mut self, holds: Check, e: impl FnOnce() -> Error) {
        if holds == Check::Fails {
            self.0.push(e());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn list(&self) -> &[Error] {
        &self.0
    }

    pub fn kinds(&self) -> Vec<Kind> {
        self.0.iter().map(|e| e.kind).collect()
    }

    /// `v` when nothing failed.
    pub fn or<T>(self, v: T) -> Result<T, Errors> {
        if self.0.is_empty() { Ok(v) } else { Err(self) }
    }

    /// One `cargo::error=` line for each failure, which a build script
    /// prints before it exits non-zero.
    pub fn directives(&self) -> String {
        self.0
            .iter()
            .map(|e| format!("cargo::error={e}\n"))
            .collect()
    }
}

impl From<Error> for Errors {
    fn from(e: Error) -> Errors {
        Errors(vec![e])
    }
}

impl fmt::Display for Errors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.0 {
            writeln!(f, "{e}")?;
        }
        Ok(())
    }
}

/// Whether a condition a table rests on holds, for [`Errors::require`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Holds,
    Fails,
}

impl From<bool> for Check {
    fn from(b: bool) -> Check {
        if b { Check::Holds } else { Check::Fails }
    }
}
