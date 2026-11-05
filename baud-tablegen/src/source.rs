//! A pinned data file and its rows, read as UAX #44 section 4.2 lays a UCD
//! file out (`[[source: uax44]]`): fields separated by semicolons, numbered
//! from zero, the first a code point or a range; leading and trailing spaces
//! in a field not significant, except in UnicodeData.txt, which allows none;
//! code points four to six hexadecimal digits; a range `X..Y`; and
//! everything from `#` to the end of the line a comment, but for the
//! `# @missing:` lines of section 4.2.10.

use crate::{Error, Kind, LAST, Pos};
use std::ops::RangeInclusive;

/// A data file a table is compiled from: its name under the crate's `data/`,
/// its key in the repository's source manifest, and the text its line 1
/// starts with -- the positive control that the pinned file, at its pinned
/// version, is the one wired in.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub file: &'static str,
    pub key: &'static str,
    pub header: &'static str,
}

/// An [`Input`]'s text, its header checked.
#[derive(Debug)]
pub struct Source {
    path: String,
    key: &'static str,
    text: String,
}

/// Whether `#` starts a comment in the rows read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Comments {
    Strip,
    Keep,
}

impl Source {
    /// `text` as the file `input` names, refused unless its line 1 starts
    /// with `input.header`.
    pub fn new(input: &Input, text: String) -> Result<Source, Error> {
        let path = format!("data/{}", input.file);
        let first = text.lines().next().unwrap_or("");
        if !first.starts_with(input.header) {
            return Err(Error::new(
                Kind::Header,
                &path,
                format!(
                    "line 1 does not start with {:?}: is this the pinned file [{}]?",
                    input.header, input.key
                ),
            )
            .at(Pos { line: 1, col: 1 }, first));
        }
        Ok(Source {
            path,
            key: input.key,
            text,
        })
    }

    /// The file `input` names, read through `read`, which takes its name
    /// under `data/`.
    pub fn load(
        input: &Input,
        read: &dyn Fn(&str) -> std::io::Result<String>,
    ) -> Result<Source, Error> {
        let text = read(input.file)
            .map_err(|e| Error::new(Kind::Read, &format!("data/{}", input.file), e.to_string()))?;
        Source::new(input, text)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn key(&self) -> &'static str {
        self.key
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Line `n`, counted from 1; empty past the end.
    pub fn line(&self, n: usize) -> &str {
        n.checked_sub(1)
            .and_then(|i| self.text.lines().nth(i))
            .unwrap_or("")
    }

    /// A failure of the file as a whole.
    pub fn error(&self, kind: Kind, message: impl Into<String>) -> Error {
        Error::new(kind, &self.path, message)
    }

    /// The data rows, comments and blank lines left out.
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.text
            .lines()
            .enumerate()
            .filter_map(move |(i, l)| row(&self.path, i + 1, l, 0, Comments::Strip))
    }

    /// The `# @missing: <range>; <value>` lines, in the file's order, each a
    /// row whose field 0 is the range. Each one overrides the ones before it
    /// for its range (UAX #44 section 4.2.10).
    pub fn missing(&self) -> impl Iterator<Item = Row<'_>> {
        const AT: &str = "# @missing: ";
        self.text.lines().enumerate().filter_map(move |(i, l)| {
            l.strip_prefix(AT)
                .and_then(|_| row(&self.path, i + 1, l, AT.len(), Comments::Strip))
        })
    }

    /// UnicodeData.txt's entries: fifteen fields each, none with spaces
    /// around it, and a `<..., First>` line with the `<..., Last>` line
    /// after it read as one entry for the range between them (UAX #44
    /// section 4.2.3), the `Last` line's fields standing for it.
    pub fn unicode_data(&self) -> Vec<Result<Entry<'_>, Error>> {
        let mut out = Vec::new();
        let mut first: Option<(Row<'_>, u32)> = None;
        let rows = self
            .text
            .lines()
            .enumerate()
            .filter_map(|(i, l)| row(&self.path, i + 1, l, 0, Comments::Keep));
        for r in rows {
            if let Err(e) = r.spaces().and_then(|()| r.expect(15)) {
                out.push(Err(e));
                continue;
            }
            let cp = match r.code_point(0) {
                Ok(cp) => cp,
                Err(e) => {
                    out.push(Err(e));
                    continue;
                }
            };
            let name = r.field(1);
            let open = first.take();
            if let Some(label) = name.strip_suffix(", Last>") {
                match open {
                    Some((f, lo)) if f.field(1).strip_suffix(", First>") == Some(label) => {
                        out.push(
                            CpRange::new(lo, cp)
                                .map_err(|(k, m)| r.error(k, 0, m))
                                .map(|range| Entry { range, row: r }),
                        );
                    }
                    _ => out.push(Err(r.error(
                        Kind::FirstWithoutLast,
                        1,
                        format!("{name} does not follow its First"),
                    ))),
                }
                continue;
            }
            if let Some((f, _)) = open {
                out.push(Err(f.error(
                    Kind::FirstWithoutLast,
                    1,
                    format!("{} is not followed by its Last", f.field(1)),
                )));
            }
            if name.ends_with(", First>") {
                first = Some((r, cp));
            } else {
                out.push(Ok(Entry {
                    range: CpRange { lo: cp, hi: cp },
                    row: r,
                }));
            }
        }
        if let Some((f, _)) = first {
            out.push(Err(f.error(
                Kind::FirstWithoutLast,
                1,
                format!("{} is not followed by its Last", f.field(1)),
            )));
        }
        out
    }
}

/// One UnicodeData.txt entry: a code point, or a First/Last range.
#[derive(Debug)]
pub struct Entry<'a> {
    pub range: CpRange,
    pub row: Row<'a>,
}

/// The row of line `n`, the fields starting at byte `from`; `None` for a
/// line with no data.
fn row<'a>(
    path: &'a str,
    n: usize,
    line: &'a str,
    from: usize,
    comments: Comments,
) -> Option<Row<'a>> {
    let body = &line[from..];
    let body = match comments {
        Comments::Strip => body.split('#').next().unwrap_or(""),
        Comments::Keep => body,
    };
    if body.trim().is_empty() {
        return None;
    }
    let mut fields = Vec::new();
    let mut at = from;
    for raw in body.split(';') {
        let lead = raw.len() - raw.trim_start().len();
        let start = at + lead;
        fields.push(Field {
            text: raw.trim(),
            raw,
            col: line[..start].chars().count() + 1,
        });
        at += raw.len() + 1;
    }
    Some(Row {
        path,
        line: n,
        text: line,
        fields,
    })
}

/// One data line, split into its fields.
#[derive(Debug)]
pub struct Row<'a> {
    path: &'a str,
    line: usize,
    text: &'a str,
    fields: Vec<Field<'a>>,
}

/// A field, trimmed, and the column its value starts at.
#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    pub text: &'a str,
    raw: &'a str,
    pub col: usize,
}

impl<'a> Row<'a> {
    pub fn line(&self) -> usize {
        self.line
    }

    /// The line as written.
    pub fn text(&self) -> &'a str {
        self.text
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Field `i`, trimmed; empty past the last.
    pub fn field(&self, i: usize) -> &'a str {
        self.fields.get(i).map_or("", |f| f.text)
    }

    /// Field `i`'s value and column; `None` past the last.
    pub fn get(&self, i: usize) -> Option<Field<'a>> {
        self.fields.get(i).copied()
    }

    /// A failure at field `i` of this row, or at its end past the last.
    pub fn error(&self, kind: Kind, i: usize, message: impl Into<String>) -> Error {
        let col = self
            .fields
            .get(i)
            .map_or(self.text.chars().count() + 1, |f| f.col);
        Error::new(kind, self.path, message).at(
            Pos {
                line: self.line,
                col,
            },
            self.text,
        )
    }

    /// Refused unless the row has exactly `n` fields.
    pub fn expect(&self, n: usize) -> Result<(), Error> {
        if self.fields.len() == n {
            Ok(())
        } else {
            Err(self.error(
                Kind::FieldCount,
                self.fields.len().min(n),
                format!("{} fields, {n} expected", self.fields.len()),
            ))
        }
    }

    /// Refused if any field has spaces around its value.
    fn spaces(&self) -> Result<(), Error> {
        match self.fields.iter().position(|f| f.raw != f.text) {
            None => Ok(()),
            Some(i) => Err(self.error(
                Kind::Malformed,
                i,
                "a UnicodeData.txt field has spaces around it",
            )),
        }
    }

    /// Field `i` as a code point or a range.
    pub fn range(&self, i: usize) -> Result<CpRange, Error> {
        CpRange::parse(self.field(i)).map_err(|(k, m)| self.error(k, i, m))
    }

    /// Field `i` as one code point, not a range.
    pub fn code_point(&self, i: usize) -> Result<u32, Error> {
        hex(self.field(i)).map_err(|(k, m)| self.error(k, i, m))
    }
}

/// A range of code points, `lo <= hi <= U+10FFFF`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpRange {
    pub lo: u32,
    pub hi: u32,
}

impl CpRange {
    /// `X..Y` or `X`, each four to six hexadecimal digits.
    pub fn parse(s: &str) -> Result<CpRange, (Kind, String)> {
        match s.split_once("..") {
            Some((lo, hi)) => CpRange::new(hex(lo)?, hex(hi)?),
            None => hex(s).map(|cp| CpRange { lo: cp, hi: cp }),
        }
    }

    fn new(lo: u32, hi: u32) -> Result<CpRange, (Kind, String)> {
        if lo <= hi {
            Ok(CpRange { lo, hi })
        } else {
            Err((
                Kind::Malformed,
                format!("U+{lo:04X}..U+{hi:04X} runs backwards"),
            ))
        }
    }

    /// Whether the range is one code point.
    pub fn is_one(&self) -> bool {
        self.lo == self.hi
    }

    pub fn iter(&self) -> RangeInclusive<u32> {
        self.lo..=self.hi
    }
}

impl std::fmt::Display for CpRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_one() {
            write!(f, "U+{:04X}", self.lo)
        } else {
            write!(f, "U+{:04X}..U+{:04X}", self.lo, self.hi)
        }
    }
}

/// A code point: four to six hexadecimal digits, at most U+10FFFF.
fn hex(s: &str) -> Result<u32, (Kind, String)> {
    let digits = (4..=6).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit());
    let cp = u32::from_str_radix(s, 16)
        .ok()
        .filter(|_| digits)
        .ok_or_else(|| {
            (
                Kind::Malformed,
                format!("{s:?} is not a code point: four to six hexadecimal digits"),
            )
        })?;
    if cp > LAST {
        return Err((Kind::PastLast, format!("U+{cp:X} is past U+10FFFF")));
    }
    Ok(cp)
}
