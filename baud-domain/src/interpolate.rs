//! **iTerm2's interpolated strings**, the form a badge's format takes
//! (`[[source: iterm2-badges]]`: "This value is an interpolated string").
//!
//! The syntax is the one iTerm2's documentation gives (`[[source:
//! iterm2-doc-scripting-fundamentals]]`): "To embed an expression in a
//! string, place it in `\(…)`"; an expression holds parentheses that group
//! and that call, and double-quoted strings; "Interpolated strings may be
//! nested", a `\(` inside an expression's string opening another; and a
//! literal part takes the page's eleven escapes, `\n` to `\uxxxx`.
//!
//! This module was written from that page and from the cases in
//! `baud-domain/tests/interpolate.rs`, without iTerm2's source code open.
//! Its author had read the earlier version of this file, a translation of
//! iTerm2's parser, before writing, and used that version only as a
//! black-box oracle; none of its code was kept. Its split is a pair of
//! functions, one for an expression and one for a string inside it, each
//! finding where its own text ends. Its call check reads each expression
//! the split gives as the split reads it, as far as the first `\(` the
//! split reads as text, and reads the whole format again with a count of
//! open parentheses for each expression it is inside.
//! Where the page says nothing, the tests decide: an expression never
//! closed is literal text, and a format with no expression is expanded
//! whole; a backslash that begins no escape is kept, a last one too; `\u`
//! also reads its four characters with white space before them or a `0x`
//! in them, and a surrogate is U+FFFD; and a user variable may be written
//! with a `?` after its name. That a format a program sends may not call
//! a function is iTerm2's rule, not the page's ([`calls_a_function`]).
//! Which variables and functions an expression names, and what it
//! evaluates to, are the caller's.
//!
//! Open: the page sets no limit on nesting ("Interpolated strings may be
//! nested"), and this parser follows sixteen `\(` deep inside strings,
//! reading one deeper as its string's text. That is a departure. The call
//! check reads every level as well, so a call is refused however deep it
//! is written, and a format of up to [`CALL_CHECK_MAX`] bytes that calls
//! nothing in either reading is accepted however deep it nests. A call
//! the split reads is refused too, but for one sixteen levels deep after
//! a `\(` the split reads as text: past that `\(` the two readings part,
//! and the reading of every depth decides.

/// One part of an interpolated string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interpolated<'a> {
    /// Text as written, escapes not yet expanded ([`expand_escapes`]).
    Literal(&'a str),
    /// The expression inside a `\(…)`, without its delimiters.
    Expression(&'a str),
}

/// How many `\(` deep, inside the strings of expressions, an expression is
/// still read as one; one deeper is read as that string's text.
const NESTING: usize = 16;

/// The index of the `)` that closes an expression whose text starts at
/// `from`, `depth` strings deep, or `None` when nothing closes it. Every
/// delimiter is ASCII, which no byte of a longer UTF-8 character equals,
/// so the bytes are read one at a time.
fn expression_end(b: &[u8], from: usize, depth: usize) -> Option<usize> {
    let mut open = 0usize;
    let mut i = from;
    while let Some(&byte) = b.get(i) {
        match byte {
            b'(' => open += 1,
            b')' if open == 0 => return Some(i),
            b')' => open -= 1,
            b'"' => i = string_end(b, i + 1, depth)?,
            _ => {}
        }
        i += 1;
    }
    None
}

/// The index of the `"` that closes a string whose text starts at `from`,
/// inside an expression `depth` strings deep, or `None`: a backslash takes
/// the byte after it, and `\(` opens an expression nested in the string.
fn string_end(b: &[u8], from: usize, depth: usize) -> Option<usize> {
    let mut i = from;
    while let Some(&byte) = b.get(i) {
        match byte {
            b'"' => return Some(i),
            b'\\' if b.get(i + 1) == Some(&b'(') && depth < NESTING => {
                i = expression_end(b, i + 2, depth + 1)?;
            }
            b'\\' => i += 1,
            _ => {}
        }
        i += 1;
    }
    None
}

/// **Each part of `s`, in order**: the text between expressions, and each
/// expression a `\(` opens and its own `)` closes, with the parentheses
/// and strings inside it read as `expression_end` reads them. A
/// backslash before anything but `(` is an escape and opens nothing, `\\(`
/// included. An expression nothing closes ends the split, and what follows
/// its `\(` is the last literal part. An empty literal part is not given.
pub fn interpolated_parts<'a>(s: &'a str, mut f: impl FnMut(Interpolated<'a>)) {
    let b = s.as_bytes();
    let mut text = 0usize;
    let mut i = 0usize;
    while let Some(&byte) = b.get(i) {
        if byte != b'\\' {
            i += 1;
            continue;
        }
        if b.get(i + 1) != Some(&b'(') {
            i += 2;
            continue;
        }
        if i > text {
            f(Interpolated::Literal(&s[text..i]));
        }
        let Some(end) = expression_end(b, i + 2, 0) else {
            text = i + 2;
            break;
        };
        f(Interpolated::Expression(&s[i + 2..end]));
        text = end + 1;
        i = end + 1;
    }
    if b.len() > text {
        f(Interpolated::Literal(&s[text..]));
    }
}

/// **A literal part's escapes expanded**, the documentation's eleven:
/// `\n`, `\a`, `\t`, `\r`, `\b`, `\f`, `\e`, `\\`, `\"`, `\/`, and `\u`
/// with four characters naming a code point in hexadecimal (`hex4`). A
/// backslash that begins none of them is kept, and the character after it
/// is read as any other.
pub fn expand_escapes(s: &str, mut f: impl FnMut(char)) {
    let mut rest = s;
    while let Some(at) = rest.find('\\') {
        rest[..at].chars().for_each(&mut f);
        let after = &rest[at + 1..];
        match escape(after) {
            Some((c, len)) => {
                f(c);
                rest = &after[len..];
            }
            None => {
                f('\\');
                rest = after;
            }
        }
    }
    rest.chars().for_each(f);
}

/// The escape that `after`, the text after a backslash, begins: the
/// character it stands for and how many bytes of `after` it spans.
fn escape(after: &str) -> Option<(char, usize)> {
    let c = match *after.as_bytes().first()? {
        b'n' => '\n',
        b'a' => '\u{7}',
        b't' => '\t',
        b'r' => '\r',
        b'b' => '\u{8}',
        b'f' => '\u{c}',
        b'e' => '\u{1b}',
        b'\\' => '\\',
        b'"' => '"',
        b'/' => '/',
        b'u' => return hex4(&after[1..]).map(|(c, len)| (c, 1 + len)),
        _ => return None,
    };
    Some((c, 1))
}

/// `\u`'s four characters, the first four of `s`, as a code point and
/// their length in bytes: hexadecimal digits that run to the fourth
/// character's end, which white space may precede and `0x` or `0X` may
/// begin. A surrogate is U+FFFD.
fn hex4(s: &str) -> Option<(char, usize)> {
    let (at, last) = s.char_indices().nth(3)?;
    let len = at + last.len_utf8();
    let four = s[..len].trim_start();
    let digits = match four.strip_prefix("0x").or_else(|| four.strip_prefix("0X")) {
        Some(d) if !d.is_empty() => d,
        _ => four,
    };
    if digits.is_empty() || !digits.bytes().all(|d| d.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(digits, 16).ok()?;
    Some((char::from_u32(v).unwrap_or('\u{FFFD}'), len))
}

/// How many expressions deep, the outermost counted, [`calls_a_function`]
/// reads a string: as deep as one of [`CALL_CHECK_MAX`] bytes can nest.
const LEVELS: usize = 128;

/// **The longest string [`calls_a_function`] reads exactly at every
/// depth**, 385 bytes. Each level past the outermost takes three bytes to
/// open, a `"` and a `\(`, so a string one byte longer can open one more
/// expression than [`LEVELS`], and that one is read as a call.
pub const CALL_CHECK_MAX: usize = 2 + 3 * LEVELS - 1;

/// **Whether any expression in `s` calls a function**, which iTerm2
/// refuses in a format a program sends (`[[source:
/// iterm2-964a75d-ptysession]]`: "For security reasons, this is not
/// allowed"). A call is the documentation's `name(argument: value)`: a `(`
/// after a name, `foo(` or `foo (`, outside the expression's strings, or
/// one in an expression nested in them at any depth. A parenthesis that
/// groups, `(a + b)`, follows no name. Two readings are taken and a
/// call either finds is refused: the split's, which follows sixteen
/// levels ([`interpolated_parts`]), in each expression up to a `\(` it
/// reads as text, and one that follows every level. An expression that
/// nothing closes is text to both and calls nothing. A
/// string longer than [`CALL_CHECK_MAX`] that opens more than [`LEVELS`]
/// expressions inside one another is read as one that calls.
pub fn calls_a_function(s: &str) -> bool {
    let mut found = false;
    interpolated_parts(s, |p| {
        if let Interpolated::Expression(e) = p {
            found |= calls_as_split(e, 0);
        }
    });
    found || calls_at_every_depth(s)
}

/// Whether the expression `e`, `depth` strings deep, calls as the split
/// reads it: a `\(` in its strings opens an expression [`NESTING`] deep
/// at most. One deeper is that string's text to the split, but opens an
/// expression to the reading of every depth ([`calls_at_every_depth`]);
/// from that `\(` the two read the rest of `e` differently, and the rest
/// is left to the reading of every depth, since a `(` the split reads
/// there as code can be in a string, as in `\("a(b)")` that deep.
fn calls_as_split(e: &str, depth: usize) -> bool {
    let b = e.as_bytes();
    let mut i = 0usize;
    let mut in_string = false;
    while let Some(&byte) = b.get(i) {
        match byte {
            b'(' if !in_string && follows_a_name(&e[..i]) => return true,
            b'"' => in_string = !in_string,
            b'\\' if in_string && b.get(i + 1) == Some(&b'(') && depth < NESTING => {
                let Some(end) = expression_end(b, i + 2, depth + 1) else {
                    return true;
                };
                if calls_as_split(&e[i + 2..end], depth + 1) {
                    return true;
                }
                i = end;
            }
            b'\\' if in_string && b.get(i + 1) == Some(&b'(') => return false,
            b'\\' if in_string => i += 1,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Whether `s` calls, its expressions read to every depth.
fn calls_at_every_depth(s: &str) -> bool {
    let b = s.as_bytes();
    // The parentheses open in each expression `s` is inside at `i`,
    // the outermost first; `depth` of them are in use, none in the text.
    let mut open = [0usize; LEVELS];
    let mut depth = 0usize;
    let mut in_string = false;
    let mut called = false;
    let mut i = 0usize;
    while let Some(&byte) = b.get(i) {
        if depth == 0 {
            if byte == b'\\' {
                if b.get(i + 1) == Some(&b'(') {
                    depth = 1;
                    open[0] = 0;
                }
                i += 1;
            }
        } else if in_string {
            match byte {
                b'"' => in_string = false,
                b'\\' if b.get(i + 1) == Some(&b'(') => {
                    let Some(level) = open.get_mut(depth) else {
                        return true;
                    };
                    *level = 0;
                    depth += 1;
                    in_string = false;
                    i += 1;
                }
                b'\\' => i += 1,
                _ => {}
            }
        } else {
            let level = &mut open[depth - 1];
            match byte {
                b'(' => {
                    called |= follows_a_name(&s[..i]);
                    *level += 1;
                }
                b')' if *level == 0 => {
                    depth -= 1;
                    if depth == 0 && called {
                        return true;
                    }
                    in_string = depth > 0;
                }
                b')' => *level -= 1,
                b'"' => in_string = true,
                _ => {}
            }
        }
        i += 1;
    }
    false
}

/// Whether the text before a `(` ends in a name, white space after it
/// allowed: a letter, a digit, `_`, or the `.` of a dotted path.
fn follows_a_name(before: &str) -> bool {
    before
        .trim_end()
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// **An interpolated string evaluated**: each literal part's escapes
/// expanded and each expression replaced by what `eval` writes for it, as
/// the documentation replaces "the expression `hostname`" with "its
/// value". With no expression in it the whole string's escapes are
/// expanded, a `\(` nothing closes kept as written.
pub fn interpolate(
    s: &str,
    mut eval: impl FnMut(&str, &mut dyn FnMut(char)),
    mut out: impl FnMut(char),
) {
    let mut any = false;
    interpolated_parts(s, |p| any |= matches!(p, Interpolated::Expression(_)));
    if !any {
        return expand_escapes(s, out);
    }
    interpolated_parts(s, |p| match p {
        Interpolated::Literal(l) => expand_escapes(l, &mut out),
        Interpolated::Expression(e) => eval(e, &mut out),
    });
}

/// **The user variable an expression names**, `user.NAME` or `user.NAME?`
/// with white space around it, where iTerm2's user-defined variables
/// "always begin with `user.`" (`[[source:
/// iterm2-doc-scripting-fundamentals]]`): the name after `user.`, or
/// `None` for any other expression.
pub fn user_variable(e: &str) -> Option<&str> {
    let e = e.trim();
    let e = e.strip_suffix('?').unwrap_or(e);
    let name = e.strip_prefix("user.")?;
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-');
    ok.then_some(name)
}
