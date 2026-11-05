//! Values written as the Python gates this crate replaced wrote them, so a
//! failure line reads the same as it did: `repr()` of a string or of a
//! TOML value as `tomllib` returned it, `str()` of one, and `sorted()`
//! of a list of strings.

use toml::Value;

/// Whether `str.isprintable()` holds for a character `repr` would keep.
/// Python leaves out the categories Cc, Cf, Cs, Co, Cn, Zl, Zp and every
/// Zs but the space; this holds the assigned code points of those that a
/// manifest field could plausibly carry -- the C0 and C1 controls, the
/// separators, the format controls and the private use planes -- and
/// reads an unassigned code point as printable.
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

/// `repr(float)` for the values a manifest holds: Python writes a whole
/// number with `.0` and an exponent with a sign.
fn repr_float(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let s = format!("{f:?}");
    match s.split_once('e') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e:0>2}"),
        Some((m, e)) => format!("{m}e-{:0>2}", &e[1..]),
        None => s,
    }
}

/// `repr()` of what `entry.get(key)` returned: `None` when absent.
pub fn repr(v: Option<&Value>) -> String {
    match v {
        None => "None".to_string(),
        Some(Value::String(s)) => repr_str(s),
        Some(Value::Integer(n)) => n.to_string(),
        Some(Value::Float(f)) => repr_float(*f),
        Some(Value::Boolean(b)) => if *b { "True" } else { "False" }.to_string(),
        Some(Value::Datetime(d)) => d.to_string(),
        Some(Value::Array(a)) => format!(
            "[{}]",
            a.iter()
                .map(|x| repr(Some(x)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(Value::Table(t)) => format!(
            "{{{}}}",
            t.iter()
                .map(|(k, x)| format!("{}: {}", repr_str(k), repr(Some(x))))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// `str()` of what `entry.get(key)` returned.
pub fn str_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        other => repr(other),
    }
}

/// `repr(sorted(names))`.
pub fn repr_sorted(names: &[&str]) -> String {
    let mut v: Vec<&str> = names.to_vec();
    v.sort_unstable();
    format!(
        "[{}]",
        v.iter().map(|s| repr_str(s)).collect::<Vec<_>>().join(", ")
    )
}

/// `str.split()` with no argument.
pub fn split(s: &str) -> Vec<&str> {
    s.split(|c: char| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// The first `n` characters, as `s[:n]`.
pub fn head(s: &str, n: crate::html::Count) -> String {
    s.chars().take(n.0).collect()
}
