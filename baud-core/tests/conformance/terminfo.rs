//! A terminfo entry, parsed far enough to replay its output capabilities.
//!
//! The fixtures in `docs/research/sources/terminfo/` are `infocmp -x -1`
//! output, with the user-defined capabilities programs read, captured
//! verbatim and hash-pinned in `MANIFEST.toml`. Parsing them here
//! rather than checking in a pre-expanded list keeps the source of truth the
//! thing ncurses actually ships.

/// One capability: its terminfo name and the bytes it expands to.
pub struct Cap {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Capabilities whose name starts with `k` are *keys the terminal sends*, not
/// sequences it receives.
///
/// **They are the input path, which baud now has.** This doc said "which baud
/// does not have" and was one of fourteen copies of that sentence, all true when
/// written and all false since `encode_key` landed. `input_caps.rs` measures how
/// many of each profile's it can send rather than restating a claim -- 85 of
/// `xterm`'s 88.
pub fn is_input(name: &str) -> bool {
    name.starts_with('k')
}

/// Expand a terminfo string capability into concrete bytes.
///
/// Parameter arithmetic (`%p1%d`, `%{8}%<%t...`) is substituted with a fixed
/// digit rather than evaluated: the point is to exercise the *shape* a terminal
/// must parse, and a real parameter value does not change which sequence it is.
/// A `%s` takes `string`, which [`string_argument`] picks by capability.
/// Padding (`$<100/>`) is dropped, as a terminal would.
pub fn expand(v: &str, string: &[u8]) -> Vec<u8> {
    let b: Vec<char> = v.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            '\\' => {
                i += 1;
                if i >= b.len() {
                    break;
                }
                let c = b[i];
                i += 1;
                match c {
                    'E' | 'e' => out.push(0x1b),
                    'n' => out.push(b'\n'),
                    'r' => out.push(b'\r'),
                    't' => out.push(b'\t'),
                    'b' => out.push(0x08),
                    'f' => out.push(0x0c),
                    's' => out.push(b' '),
                    '\\' => out.push(b'\\'),
                    ',' => out.push(b','),
                    ':' => out.push(b':'),
                    '^' => out.push(b'^'),
                    d if d.is_ascii_digit() => {
                        // Octal, up to three digits, with the first consumed.
                        let mut n = d.to_digit(8).unwrap_or(0);
                        let mut k = 1;
                        while k < 3 && i < b.len() && b[i].is_digit(8) {
                            n = n * 8 + b[i].to_digit(8).unwrap_or(0);
                            i += 1;
                            k += 1;
                        }
                        out.push((n & 0xff) as u8);
                    }
                    other => out.push(other as u8),
                }
            }
            '^' => {
                i += 1;
                if i < b.len() {
                    out.push((b[i].to_ascii_uppercase() as u8) ^ 0x40);
                    i += 1;
                }
            }
            '$' if i + 1 < b.len() && b[i + 1] == '<' => {
                // Padding delay. A terminal ignores it.
                while i < b.len() && b[i] != '>' {
                    i += 1;
                }
                i += 1;
            }
            '%' => {
                i += 1;
                if i >= b.len() {
                    break;
                }
                let op = b[i];
                match op {
                    '%' => {
                        out.push(b'%');
                        i += 1;
                    }
                    'd' | 'c' => {
                        out.push(b'1');
                        i += 1;
                    }
                    's' => {
                        out.extend_from_slice(string);
                        i += 1;
                    }
                    '\'' => {
                        i += 1;
                        while i < b.len() && b[i] != '\'' {
                            i += 1;
                        }
                        i += 1;
                    }
                    '{' => {
                        while i < b.len() && b[i] != '}' {
                            i += 1;
                        }
                        i += 1;
                    }
                    'p' | 'P' | 'g' => i += 2,
                    c if c.is_ascii_digit() || c == '.' || c == ':' => {
                        // A printf-style width/precision spec: skip to its verb.
                        while i < b.len() && !matches!(b[i], 'd' | 'c' | 's' | 'x' | 'X') {
                            i += 1;
                        }
                        out.push(b'1');
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                i += 1;
            }
        }
    }
    out
}

/// What a `%s` stands for, by capability.
///
/// **A digit is a number, and `Cs` takes a colour.** ncurses' `terminfo.src`:
/// "Cs is a string capability which sets the cursor color to a given value.
/// The single string parameter is the color name/number, according to the
/// implementation." xterm's `OSC 12` takes a colour specification, so the
/// digit replayed `ESC ] 12 ; 1 BEL`, which names no colour and was counted
/// as dropped. `red` is a name, as a program passes a user's colour, and
/// one in xterm's FAQ (`[[source: xterm-faq]]` 7645-7646). `Ms` keeps the digit
/// for both its strings, "the storage unit" and "the base64-encoded
/// clipboard content", and baud does not count what that makes as dropped.
fn string_argument(name: &str) -> &'static [u8] {
    match name {
        "Cs" => b"red",
        _ => b"1",
    }
}

/// Every capability in an `infocmp` fixture that carries an escape sequence.
pub fn parse(src: &str) -> Vec<Cap> {
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim().trim_end_matches(',');
        if line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (name, val) = line.split_once('=').expect("checked");
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        // Only sequences; plain literals like `cr=^M` still count, but a
        // capability with no control content tells us nothing about the parser.
        if !val.contains("\\E") && !val.contains('^') {
            continue;
        }
        out.push(Cap {
            name: name.to_string(),
            bytes: expand(val, string_argument(name)),
        });
    }
    out
}
