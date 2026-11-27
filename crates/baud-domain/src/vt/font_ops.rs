//! `OSC 50 ; Pt`, "Set Font to Pt", and its `?` query (`[[source: ctlseqs]]`):
//! what a program asks of the font, parsed. The core answers the query and
//! queues the change; the embedder resolves the name and changes the size.
//!
//! `[[source: ctlseqs]]`: "If Pt begins with a "#", index in the font menu,
//! relative (if the next character is a plus or minus sign) or absolute. A
//! number is expected but not required after the sign (the default is the
//! current entry for relative, zero for absolute indexing). The same rule
//! (plus or minus sign, optional number) is used when querying the font.
//! The remainder of Pt is ignored. A font can be specified after a "#"
//! index expression, by adding a space and then the font specifier."
//!
//! **The text's defaults, where xterm's code differs** (`ParseShiftedFont`,
//! `[[source: xterm-misc]]` 3759-3796): a sign with no number is the current
//! entry, where xterm steps one; a bare `#` is entry 0, where xterm keeps
//! the current one.

/// An index expression, after the `#` or the `?`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontIndex {
    pub entry: FontEntry,
    /// Whether anything followed the sign, or the `#` or `?` with no sign:
    /// xterm's query names the entry, `#n`, only then (`QueryFontRequest`,
    /// "identify the font-entry, unless it is the current one").
    pub written: FontWritten,
}

/// The menu entry an index expression names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontEntry {
    /// An absolute index, `#n`: "zero for absolute indexing" with no number.
    Absolute(u32),
    /// A relative one, `#+n` or `#-n`: 0, "the current entry", with no number.
    Relative(i32),
}

/// Whether an index expression wrote more than its `#` or `?` and sign.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontWritten {
    Yes,
    No,
}

/// `OSC 50 ; Pt`, parsed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontRequest<'a> {
    /// `?` and an index expression: the font an entry holds.
    Query(FontIndex),
    /// A change: the index expression after a `#`, if `Pt` began with one,
    /// and the name -- the rest after its digits, or the whole of `Pt` --
    /// with the spaces either side trimmed, as xterm's `x_strtrim`. Either
    /// may be empty.
    Set {
        index: Option<FontIndex>,
        name: &'a [u8],
    },
}

impl<'a> FontRequest<'a> {
    /// `Pt`, the bytes after `OSC 50 ;`.
    pub fn parse(pt: &'a [u8]) -> FontRequest<'a> {
        match pt.first() {
            Some(b'?') => FontRequest::Query(index(&pt[1..]).0),
            Some(b'#') => {
                let (index, rest) = index(&pt[1..]);
                FontRequest::Set {
                    index: Some(index),
                    name: trim(rest),
                }
            }
            _ => FontRequest::Set {
                index: None,
                name: trim(pt),
            },
        }
    }
}

/// The index expression at the start of `s` and what follows its digits.
fn index(s: &[u8]) -> (FontIndex, &[u8]) {
    let (sign, rest) = match s.first() {
        Some(b'+') => (Some(1i64), &s[1..]),
        Some(b'-') => (Some(-1i64), &s[1..]),
        _ => (None, s),
    };
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    let value = rest[..digits].iter().fold(0i64, |v, &b| {
        (v * 10 + i64::from(b - b'0')).min(i64::from(i32::MAX))
    });
    let entry = match sign {
        Some(s) => FontEntry::Relative(i32::try_from(s * value).unwrap_or(i32::MAX)),
        None => FontEntry::Absolute(u32::try_from(value).unwrap_or(u32::MAX)),
    };
    let written = match rest.is_empty() {
        true => FontWritten::No,
        false => FontWritten::Yes,
    };
    (FontIndex { entry, written }, &rest[digits..])
}

/// `s` without the spaces either side, as C's `isspace` names them.
fn trim(s: &[u8]) -> &[u8] {
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let start = s.iter().position(|b| !space(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !space(b)).map_or(start, |e| e + 1);
    &s[start..end]
}

/// Whether a program's `OSC 50` may change the font: `On` by default, and
/// `Off` refuses every change, counted; the query is answered either way.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum FontChanges {
    #[default]
    On,
    Off,
}

/// Whether the font the embedder draws in is at the size its configuration
/// gives, menu entry 0, or at a size a step took it to, which has no
/// entry: what a query naming the current entry can say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontSize {
    Configured,
    Stepped,
}

/// The size a change asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeStep {
    /// The size kept: no index, or the current entry.
    Keep,
    /// Entry 0, the configured size, as Actual Size returns to it.
    Configured,
    /// This many points larger, or smaller, than the size in force.
    By(i32),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(index: Option<FontIndex>, name: &[u8]) -> FontRequest<'_> {
        FontRequest::Set { index, name }
    }

    fn at(entry: FontEntry, written: FontWritten) -> FontIndex {
        FontIndex { entry, written }
    }

    /// **Every shape the text gives, parsed as it gives it.**
    #[test]
    fn each_form_parses_as_the_text_gives_it() {
        use FontEntry::*;
        use FontWritten::{No, Yes};
        let cases: &[(&[u8], FontRequest<'_>)] = &[
            (b"?", FontRequest::Query(at(Absolute(0), No))),
            (b"?0", FontRequest::Query(at(Absolute(0), Yes))),
            (b"?7", FontRequest::Query(at(Absolute(7), Yes))),
            (b"?+", FontRequest::Query(at(Relative(0), No))),
            (b"?+1", FontRequest::Query(at(Relative(1), Yes))),
            (b"?-2", FontRequest::Query(at(Relative(-2), Yes))),
            (b"?x", FontRequest::Query(at(Absolute(0), Yes))),
            (b"", set(None, b"")),
            (b"Menlo", set(None, b"Menlo")),
            (b"  JetBrains Mono \t", set(None, b"JetBrains Mono")),
            (b"#", set(Some(at(Absolute(0), No)), b"")),
            (b"#+1", set(Some(at(Relative(1), Yes)), b"")),
            (b"#-", set(Some(at(Relative(0), No)), b"")),
            (b"#3 Menlo", set(Some(at(Absolute(3), Yes)), b"Menlo")),
            (b"#+2Menlo", set(Some(at(Relative(2), Yes)), b"Menlo")),
            (
                b"#99999999999 x",
                set(Some(at(Absolute(i32::MAX as u32), Yes)), b"x"),
            ),
        ];
        for (pt, want) in cases {
            assert_eq!(
                FontRequest::parse(pt),
                *want,
                "{:?}",
                core::str::from_utf8(pt)
            );
        }
    }
}
