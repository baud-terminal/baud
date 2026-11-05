//! `textwrap.fill` as Python 3.13's `textwrap.TextWrapper` computes it,
//! for the arguments `cargo xtask publish` writes each NOTICE with:
//! `break_on_hyphens=False` and `break_long_words=False`, every other
//! argument at its default (`expand_tabs` with a tab size of 8,
//! `replace_whitespace`, `drop_whitespace`, no `max_lines`, no
//! `fix_sentence_endings`).
//!
//! What the port keeps, each a place a reading of the documentation alone
//! would differ:
//!
//! - widths and lengths count code points, as `len()` does;
//! - tabs are expanded first, the column starting again after `\n` and
//!   `\r`, and only then do the six ASCII whitespace characters
//!   `\t\n\x0b\x0c\r ` become spaces; text splits on runs of those six
//!   alone, so a no-break space stays inside its word;
//! - a chunk is dropped as whitespace when `str.strip()` empties it, which
//!   is Unicode whitespace: a word of U+2003 alone is dropped too;
//! - leading whitespace is dropped from every line but the first;
//! - a word longer than the line goes whole onto a line of its own.

use crate::text;

/// What `fill` is given beside the text.
pub struct Wrap<'a> {
    pub width: crate::html::Count,
    pub initial_indent: &'a str,
    pub subsequent_indent: &'a str,
}

impl Wrap<'_> {
    /// `TextWrapper(width=76, break_on_hyphens=False,
    /// break_long_words=False)`, with no indent.
    pub const PLAIN: Wrap<'static> = Wrap {
        width: crate::html::Count(76),
        initial_indent: "",
        subsequent_indent: "",
    };

    /// `fill(text)`: `"\n".join(wrap(text))`.
    pub fn fill(&self, text: &str) -> String {
        self.wrap(text).join("\n")
    }

    /// `wrap(text)`: the lines.
    pub fn wrap(&self, text: &str) -> Vec<String> {
        let mut chunks = split(&munge(text));
        chunks.reverse();
        let mut lines: Vec<String> = Vec::new();
        let full = self.width.0;
        while !chunks.is_empty() {
            let mut cur: Vec<String> = Vec::new();
            let mut cur_len = 0;
            let indent = if lines.is_empty() {
                self.initial_indent
            } else {
                self.subsequent_indent
            };
            // A width under the indent is a negative int in Python; every
            // chunk is longer than it, so nothing is taken but a long word.
            let width = full.checked_sub(len(indent).0);
            if blank(chunks.last()) && !lines.is_empty() {
                chunks.pop();
            }
            while let Some(c) = chunks.last() {
                let l = len(c).0;
                if width.is_some_and(|w| cur_len + l <= w) {
                    cur_len += l;
                    cur.extend(chunks.pop());
                } else {
                    break;
                }
            }
            if let Some(c) = chunks.last() {
                if width.is_none_or(|w| len(c).0 > w) && cur.is_empty() {
                    cur.extend(chunks.pop());
                }
            }
            if blank(cur.last()) {
                cur.pop();
            }
            if !cur.is_empty() {
                lines.push(format!("{indent}{}", cur.concat()));
            }
        }
        lines
    }
}

/// `len()`: code points.
fn len(s: &str) -> crate::html::Count {
    crate::html::Count(s.chars().count())
}

/// `chunk.strip() == ''`.
fn blank(c: Option<&String>) -> bool {
    c.is_some_and(|c| text::strip(c).is_empty())
}

/// The six characters `textwrap` treats as whitespace.
fn ascii_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ')
}

/// `_munge_whitespace`: `expandtabs(8)`, then each of the six to a space.
fn munge(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut col = 0;
    for c in text.chars() {
        match c {
            '\t' => {
                let n = 8 - col % 8;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\n' | '\r' => {
                out.push(' ');
                col = 0;
            }
            c if ascii_space(c) => {
                out.push(' ');
                col += 1;
            }
            c => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

/// `wordsep_simple_re.split(text)` with the empty chunks left out: runs of
/// spaces and runs of everything else, alternating.
fn split(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_space = false;
    for c in text.chars() {
        let s = ascii_space(c);
        if s != in_space && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
        }
        in_space = s;
        cur.push(c);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}
