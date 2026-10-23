//! **iTerm2's interpolated strings**, the form a badge's format takes
//! (`[[source: iterm2-badges]]`): text with expressions embedded in `\(…)`
//! and backslash escapes (`[[source: iterm2-doc-scripting-fundamentals]]`).
//! What is here is the parse, as iTerm2's code does it: the split into
//! literal text and expressions (`iTermSwiftyStringParser`), the escapes a
//! literal part expands (`it_stringByExpandingBackslashEscapedCharacters`),
//! and whether an expression calls a function, which iTerm2 refuses in a
//! format a program sends (`[[source: iterm2-964a75d-ptysession]]`). What
//! an expression evaluates to is the caller's.

/// One part of an interpolated string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interpolated<'a> {
    /// Text as written, escapes not yet expanded ([`expand_escapes`]).
    Literal(&'a str),
    /// The expression inside a `\(…)`, without its delimiters.
    Expression(&'a str),
}

/// The parser's states, `iTermSwiftyStringParser`'s five.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Literal,
    LiteralEscape,
    Expression,
    ExprString,
    ExprStringEscape,
}

/// The deepest `\(` nested in an expression's strings this follows; one
/// deeper is read as the string it sits in.
const NESTING: usize = 16;

/// **Each part of `s`, in order**, as `iTermSwiftyStringParser` splits it
/// (`[[source: iterm2-964a75d-swifty-string-parser]]`): a `\(` opens an
/// expression, which its matching `)` closes, counting the parentheses in
/// it and skipping those in its double-quoted strings, where a `\(` opens
/// a nested expression; whatever is left at the end, an expression not
/// closed included, is literal text. An empty literal part is not given.
pub fn interpolated_parts<'a>(s: &'a str, mut f: impl FnMut(Interpolated<'a>)) {
    let mut state = State::Literal;
    let (mut start, mut parens) = (0usize, 0usize);
    let mut stack = [0usize; NESTING];
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match state {
            State::Literal => {
                if c == '\\' {
                    state = State::LiteralEscape;
                }
            }
            State::LiteralEscape => {
                if c == '(' {
                    // The text up to the `\`, which is one byte.
                    if i - 1 > start {
                        f(Interpolated::Literal(&s[start..i - 1]));
                    }
                    start = i + 1;
                    parens = 1;
                    state = State::Expression;
                } else {
                    state = State::Literal;
                }
            }
            State::Expression => match c {
                '(' => parens += 1,
                ')' => {
                    parens -= 1;
                    if parens == 0 {
                        if depth == 0 {
                            f(Interpolated::Expression(&s[start..i]));
                            start = i + 1;
                            state = State::Literal;
                        } else {
                            depth -= 1;
                            parens = stack[depth];
                            state = State::ExprString;
                        }
                    }
                }
                '"' => state = State::ExprString,
                _ => {}
            },
            State::ExprString => match c {
                '\\' => state = State::ExprStringEscape,
                '"' => state = State::Expression,
                _ => {}
            },
            State::ExprStringEscape => {
                if c == '(' && depth < NESTING {
                    stack[depth] = parens;
                    depth += 1;
                    parens = 1;
                    state = State::Expression;
                } else {
                    state = State::ExprString;
                }
            }
        }
    }
    if s.len() > start {
        f(Interpolated::Literal(&s[start..]));
    }
}

/// **A literal part's escapes expanded**, as
/// `it_stringByExpandingBackslashEscapedCharacters` expands them
/// (`[[source: iterm2-964a75d-nsstringiterm]]`): `\n`, `\r`, `\t`, `\a`,
/// `\b`, `\f`, `\e`, `\\`, `\"` and `\/`, and `\u` and four characters
/// that scan as hexadecimal to the end, `NSScanner`'s `scanHexInt` (white
/// space skipped, an optional `0x`); any other backslash is kept with the
/// character after it. A code point that is a surrogate is U+FFFD.
pub fn expand_escapes(s: &str, mut f: impl FnMut(char)) {
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            f(c);
            continue;
        }
        let Some(&e) = it.peek() else {
            f('\\');
            break;
        };
        let simple = match e {
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'a' => Some('\u{7}'),
            'b' => Some('\u{8}'),
            'f' => Some('\u{c}'),
            'e' => Some('\u{1b}'),
            '\\' => Some('\\'),
            '"' => Some('"'),
            '/' => Some('/'),
            _ => None,
        };
        if let Some(r) = simple {
            it.next();
            f(r);
            continue;
        }
        if e == 'u' {
            let mut ahead = it.clone();
            ahead.next();
            let mut four = ['\0'; 4];
            let mut n = 0;
            for slot in &mut four {
                match ahead.next() {
                    Some(h) => {
                        *slot = h;
                        n += 1;
                    }
                    None => break,
                }
            }
            if n == 4 {
                if let Some(v) = scan_hex(&four) {
                    it.next();
                    for _ in 0..4 {
                        it.next();
                    }
                    f(char::from_u32(v).unwrap_or('\u{FFFD}'));
                    continue;
                }
            }
        }
        // Not an escape: the backslash stays, and the character after it
        // is read as any other.
        f('\\');
    }
}

/// `scanHexInt` over exactly these four characters: true only when it
/// reads to their end.
fn scan_hex(four: &[char; 4]) -> Option<u32> {
    let mut i = 0;
    while i < 4 && four[i].is_whitespace() {
        i += 1;
    }
    if i + 1 < 4
        && four[i] == '0'
        && matches!(four[i + 1], 'x' | 'X')
        && four[i + 2..].iter().any(|c| c.is_ascii_hexdigit())
    {
        i += 2;
    }
    let digits = &four[i..];
    if digits.is_empty() || !digits.iter().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    digits
        .iter()
        .try_fold(0u32, |v, c| Some(v * 16 + c.to_digit(16)?))
}

/// **Whether any expression in `s` calls a function**, which iTerm2
/// refuses in a format a program sends ("For security reasons, this is not
/// allowed"): a `(` that follows a name, `foo(` or `foo (`, outside the
/// expression's strings and inside the expressions nested in them. A
/// parenthesis that groups, `(a + b)`, follows no name.
pub fn calls_a_function(s: &str) -> bool {
    let mut found = false;
    interpolated_parts(s, |p| {
        if let Interpolated::Expression(e) = p {
            found |= expression_calls(e);
        }
    });
    found
}

fn expression_calls(e: &str) -> bool {
    let mut in_string = false;
    let mut escape = false;
    let mut last: Option<char> = None;
    let mut it = e.chars().peekable();
    while let Some(c) = it.next() {
        if in_string {
            if escape {
                escape = false;
                if c == '(' {
                    // A nested expression: read to its close as one.
                    let mut depth = 1usize;
                    let mut inner = alloc_free::Buf::new();
                    for d in it.by_ref() {
                        match d {
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                        if depth == 0 {
                            break;
                        }
                        inner.push(d);
                    }
                    if inner.calls() {
                        return true;
                    }
                }
                continue;
            }
            match c {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' if last.is_some_and(|l| l.is_alphanumeric() || l == '_' || l == '.') => {
                return true;
            }
            _ => {}
        }
        if !c.is_whitespace() {
            last = Some(c);
        }
    }
    false
}

/// A nested expression read without allocating: its first characters,
/// as many as fit, which is all a call needs to be seen in, and whether
/// it ran past them, which is then read as a call, the safe side.
mod alloc_free {
    const CAP: usize = 256;

    pub(super) struct Buf {
        bytes: [u8; CAP],
        len: usize,
        over: Over,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Over {
        Fits,
        Past,
    }

    impl Buf {
        pub(super) fn new() -> Buf {
            Buf {
                bytes: [0; CAP],
                len: 0,
                over: Over::Fits,
            }
        }

        pub(super) fn push(&mut self, c: char) {
            let mut tmp = [0u8; 4];
            let s = c.encode_utf8(&mut tmp);
            if self.len + s.len() > CAP {
                self.over = Over::Past;
                return;
            }
            self.bytes[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
            self.len += s.len();
        }

        pub(super) fn calls(&self) -> bool {
            self.over == Over::Past
                || core::str::from_utf8(&self.bytes[..self.len]).is_ok_and(super::expression_calls)
        }
    }
}

/// **An interpolated string evaluated**: each literal part's escapes
/// expanded, each expression replaced by what `eval` writes for it, as
/// iTerm2's evaluator joins the parts; with no expression in it the whole
/// string's escapes expanded, as iTerm2's parser does then.
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
/// "always begin with user." (`[[source:
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
