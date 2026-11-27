//! Reading text the way the gates this crate replaced read it.
//!
//! The audit's counts and the scan audit's line numbers were taken through
//! Python's text layer, and a port that read a file differently would move
//! them without any rule changing. Two of its behaviours matter: `read()`
//! translates every `\r\n` and lone `\r` to `\n` (universal newlines), and
//! `splitlines()` breaks on eleven separators rather than one.

use std::fs;
use std::io;
use std::path::Path;

/// A file as `open(path).read()` returns it: strict UTF-8, with `\r\n` and
/// a lone `\r` each read as `\n`.
pub fn read_universal(path: &Path) -> io::Result<String> {
    let raw = fs::read(path)?;
    let text = String::from_utf8(raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(universal(&text))
}

/// Universal newlines over text already decoded.
pub fn universal(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Whether `str.splitlines()` ends a line at this character.
fn breaks(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r'
            | '\u{0b}'
            | '\u{0c}'
            | '\u{1c}'
            | '\u{1d}'
            | '\u{1e}'
            | '\u{85}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// `str.splitlines()`: `\r\n` is one break, and a break at the end of the
/// text opens no empty last line.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if !breaks(c) {
            continue;
        }
        lines.push(&text[start..at]);
        let mut end = at + c.len_utf8();
        if c == '\r' {
            if let Some(&(next, '\n')) = chars.peek() {
                end = next + 1;
                chars.next();
            }
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Whether `str.isspace()` holds: Unicode `White_Space`, and also the four
/// information separators `\x1c`-`\x1f`, whose bidirectional class is a
/// separator.
pub fn space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// `str.strip()` with no argument.
pub fn strip(line: &str) -> &str {
    line.trim_matches(space)
}
