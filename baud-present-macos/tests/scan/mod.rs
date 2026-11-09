//! **Rust source read as code**: comments, strings and character
//! literals blanked, byte for byte, for the source checks that read
//! where a call is made. Shared by `probe_gates` and `turn_doors`.

/// `src` with every comment, string and character literal blanked to
/// spaces, byte for byte, so offsets agree with `src` and no brace, `;` or
/// `,` inside one is read as code. A lifetime (`'a`) is left as it is.
pub fn blanked(src: &str) -> Vec<u8> {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for c in &mut out[from..to.min(b.len())] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
    };
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i < b.len() {
        let rest = &b[i..];
        let after_ident = i > 0 && ident(b[i - 1]);
        if rest.starts_with(b"//") {
            let end = rest
                .iter()
                .position(|&c| c == b'\n')
                .map_or(b.len(), |e| i + e);
            blank(&mut out, i, end);
            i = end;
        } else if rest.starts_with(b"/*") {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                if b[j..].starts_with(b"/*") {
                    depth += 1;
                    j += 2;
                } else if b[j..].starts_with(b"*/") {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if !after_ident && (rest.starts_with(b"r#") || rest.starts_with(b"r\"")) {
            let hashes = rest[1..].iter().take_while(|&&c| c == b'#').count();
            if rest.get(1 + hashes) != Some(&b'"') {
                i += 1;
                continue;
            }
            let close: Vec<u8> = core::iter::once(b'"')
                .chain(core::iter::repeat_n(b'#', hashes))
                .collect();
            let body = i + 2 + hashes;
            let end = b[body..]
                .windows(close.len())
                .position(|w| w == close.as_slice())
                .map_or(b.len(), |e| body + e + close.len());
            blank(&mut out, i, end);
            i = end;
        } else if b[i] == b'"' {
            let mut j = i + 1;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            blank(&mut out, i, j + 1);
            i = j + 1;
        } else if b[i] == b'\'' {
            // A character literal, `'x'` or `'\n'`; else a lifetime.
            let end = match rest.get(1) {
                Some(b'\\') => rest[2..]
                    .iter()
                    .position(|&c| c == b'\'')
                    .map(|e| i + 2 + e + 1),
                Some(_) => {
                    let width = src[i + 1..].chars().next().map_or(1, char::len_utf8);
                    (b.get(i + 1 + width) == Some(&b'\'')).then_some(i + 1 + width + 1)
                }
                None => None,
            };
            match end {
                Some(end) => {
                    blank(&mut out, i, end);
                    i = end;
                }
                None => i += 1,
            }
        } else {
            i += 1;
        }
    }
    out
}
