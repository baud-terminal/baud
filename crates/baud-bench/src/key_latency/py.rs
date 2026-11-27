//! Values read and written as the Python driver this module replaced read
//! and wrote them, so a report's bytes stay the same: `str.strip`,
//! `str.split`, `str.splitlines`, `int`, `repr` and the `g` format.

/// `str.isspace`: Unicode's White_Space and the four information
/// separators.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// `s.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `s.split()`.
pub fn split(s: &str) -> impl Iterator<Item = &str> {
    s.split(is_space).filter(|t| !t.is_empty())
}

/// Lines cut wherever `boundary` says one ends, `\r\n` being one ending.
fn lines_by(s: &str, boundary: impl Fn(char) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if boundary(c) {
            out.push(&s[start..i]);
            if c == '\r' && chars.peek().is_some_and(|(_, n)| *n == '\n') {
                chars.next();
            }
            start = chars.peek().map_or(s.len(), |(n, _)| *n);
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// `s.splitlines()`.
pub fn splitlines(s: &str) -> Vec<&str> {
    lines_by(s, |c| {
        matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        )
    })
}

/// A text file's lines as universal newlines give them, without the ending.
pub fn universal_lines(s: &str) -> Vec<&str> {
    lines_by(s, |c| c == '\n' || c == '\r')
}

/// `int(s)`, as far as `i128` holds it and in ASCII digits: a sign, then
/// digits that single underscores may separate.
pub fn int(s: &str) -> Option<i128> {
    let s = strip(s);
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
    if !well_formed {
        return None;
    }
    let n: i128 = digits.replace('_', "").parse().ok()?;
    Some(if s.starts_with('-') { -n } else { n })
}

/// Whether `str.isprintable` holds for a character `repr` would keep: the
/// C0 and C1 controls, the separators, the format controls and the private
/// use planes do not, and an unassigned code point reads as printable.
fn printable(c: char) -> bool {
    !matches!(
        c,
        '\u{0}'..='\u{1f}'
            | '\u{7f}'..='\u{a0}'
            | '\u{ad}'
            | '\u{600}'..='\u{605}'
            | '\u{61c}'
            | '\u{6dd}'
            | '\u{70f}'
            | '\u{1680}'
            | '\u{180e}'
            | '\u{2000}'..='\u{200f}'
            | '\u{2028}'..='\u{202f}'
            | '\u{205f}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{3000}'
            | '\u{e000}'..='\u{f8ff}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{f0000}'..='\u{10ffff}'
    )
}

/// `repr(s)`.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if printable(c) => out.push(c),
            c => {
                let n = u32::from(c);
                if n <= 0xff {
                    out.push_str(&format!("\\x{n:02x}"));
                } else if n <= 0xffff {
                    out.push_str(&format!("\\u{n:04x}"));
                } else {
                    out.push_str(&format!("\\U{n:08x}"));
                }
            }
        }
    }
    out.push(quote);
    out
}

/// `repr` of a list of strings.
pub fn repr_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| repr_str(s)).collect();
    format!("[{}]", inner.join(", "))
}

/// `repr(f)`: the shortest digits that read back, a whole number with `.0`
/// and an exponent with a sign and two digits.
pub fn repr_float(f: f64) -> String {
    let s = format!("{f:?}");
    match s.split_once('e') {
        Some((m, e)) => match e.strip_prefix('-') {
            Some(e) => format!("{m}e-{e:0>2}"),
            None => format!("{m}e+{e:0>2}"),
        },
        None => s,
    }
}

/// `f"{x:g}"`: six significant digits, an exponent below 1e-4 and from 1e6,
/// and no trailing zeros.
pub fn format_g(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let sci = format!("{x:.5e}");
    let (mantissa, exp) = sci.split_once('e').expect("LowerExp writes an exponent");
    let exp: i32 = exp.parse().expect("LowerExp's exponent is an integer");
    let trim = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if !(-4..6).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", trim(mantissa), exp.abs());
    }
    let places = usize::try_from(5 - exp).expect("the exponent is under six");
    trim(&format!("{x:.places$}"))
}

/// What a text-mode pipe reads: `\r\n` and `\r` become `\n`.
pub fn text_mode(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// `os.path.abspath`: joined to the working directory, then normalised
/// without touching the disk.
pub fn abspath(path: &str) -> String {
    let joined = if path.starts_with('/') {
        path.to_string()
    } else {
        let cwd = std::env::current_dir().map_or(String::new(), |d| d.display().to_string());
        format!("{cwd}/{path}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}
