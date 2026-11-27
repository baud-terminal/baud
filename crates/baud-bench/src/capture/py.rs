//! Values read and written as Python read and wrote them, so a `.steps` line
//! and a command line mean what they did: `bytes` and `str` `repr`,
//! `float`, `int`, `str.strip`, and the `unicode_escape` codec `--keys` and
//! `--answer` are written in.

pub use crate::key_latency::py::{is_space, repr_float, repr_list, repr_str as str_repr, strip};

/// Whether `_` sits only between two ASCII digits, as Python's numbers ask.
fn underscores_fit(s: &str) -> bool {
    let b = s.as_bytes();
    b.iter().enumerate().all(|(i, c)| {
        *c != b'_'
            || (i > 0 && b[i - 1].is_ascii_digit() && b.get(i + 1).is_some_and(u8::is_ascii_digit))
    })
}

/// `float(s)`, for ASCII digits.
pub fn float(s: &str) -> Option<f64> {
    let t = strip(s);
    if !underscores_fit(t) {
        return None;
    }
    t.replace('_', "").parse().ok()
}

/// `int(s)`, for ASCII digits, a value past `i64` held at its end: every use
/// here refuses those as it refuses any number out of its field.
pub fn int(s: &str) -> Option<i64> {
    let t = strip(s);
    if !underscores_fit(t) {
        return None;
    }
    let (neg, digits) = match t.as_bytes().first()? {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = digits.replace('_', "");
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let held = if neg { i64::MIN } else { i64::MAX };
    let text = if neg { format!("-{digits}") } else { digits };
    Some(text.parse().unwrap_or(held))
}

/// `repr(b)` of a `bytes`.
pub fn bytes_repr(b: &[u8]) -> String {
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(b.len() + 3);
    out.push('b');
    out.push(quote);
    for c in b {
        match *c {
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => {
                if char::from(*c) == quote {
                    out.push('\\');
                }
                out.push(char::from(*c));
            }
            other => out.push_str(&format!("\\x{other:02x}")),
        }
    }
    out.push(quote);
    out
}

/// `len` hex digits at `at`, or nothing when one is missing.
fn hex(b: &[u8], at: usize, len: usize) -> Option<u32> {
    let end = at.checked_add(len)?;
    let mut v = 0u32;
    for d in b.get(at..end)? {
        v = v.checked_mul(16)? + char::from(*d).to_digit(16)?;
    }
    Some(v)
}

/// `s.encode().decode("unicode_escape").encode("latin-1")`, how `--keys` and
/// `--answer` are written: `\x1b`, `\e` left as it is, a bare byte as itself.
/// `Err` is the exception the script died of, exit 1.
///
/// `\N{name}` is refused whatever the name. A name above U+00FF dies in
/// `latin-1` anyway and an unknown one in the decode; `\x` says the rest.
pub fn unescape(s: &str) -> Result<Vec<u8>, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let wide = |v: u32| -> Result<u8, String> {
        u8::try_from(v).map_err(|_| {
            format!("'latin-1' codec can't encode character U+{v:04X}: ordinal not in range(256)")
        })
    };
    while i < b.len() {
        let c = b[i];
        i += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&e) = b.get(i) else {
            return Err("'unicodeescape' codec: \\ at end of string".to_string());
        };
        i += 1;
        match e {
            b'\n' => {}
            b'\\' | b'\'' | b'"' => out.push(e),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b't' => out.push(9),
            b'n' => out.push(10),
            b'r' => out.push(13),
            b'v' => out.push(11),
            b'a' => out.push(7),
            b'0'..=b'7' => {
                let mut v = u32::from(e - b'0');
                for _ in 0..2 {
                    match b.get(i) {
                        Some(d @ b'0'..=b'7') => {
                            v = v * 8 + u32::from(*d - b'0');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push(wide(v)?);
            }
            b'x' | b'u' | b'U' => {
                let len = match e {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let v = hex(b, i, len).ok_or_else(|| {
                    format!(
                        "'unicodeescape' codec: truncated \\{} escape",
                        char::from(e)
                    )
                })?;
                i += len;
                out.push(wide(v)?);
            }
            b'N' => {
                return Err("\\N{name} is not read here: write the byte as \\xNN".to_string());
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    Ok(out)
}

/// `haystack.count(needle)`: non-overlapping, and one more than the length
/// for an empty needle.
pub fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return haystack.len() + 1;
    }
    let mut n = 0;
    let mut at = 0;
    while let Some(i) = find(&haystack[at..], needle) {
        n += 1;
        at += i + needle.len();
    }
    n
}

/// `haystack.find(needle)`, for a needle that is not empty.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// `time.sleep(seconds)`: its refusals are exit 1.
pub fn sleep(seconds: f64) -> Result<(), String> {
    if seconds.is_nan() {
        return Err("Invalid value NaN (not a number)".to_string());
    }
    if seconds < 0.0 {
        return Err("sleep length must be non-negative".to_string());
    }
    let d = std::time::Duration::try_from_secs_f64(seconds)
        .map_err(|_| "timestamp too large to convert to C PyTime_t".to_string())?;
    std::thread::sleep(d);
    Ok(())
}
