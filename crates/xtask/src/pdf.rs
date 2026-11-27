//! Text out of the pinned PDFs, three ways, because the documents differ.
//!
//! **Each extractor is the one its documents were first read through, not
//! one parser wearing three names.** They frame streams differently and
//! unescape strings differently, so unifying them would move the counts a
//! record quotes:
//!
//! - [`flate`]: streams inflated in the clear, every parenthesised string
//!   in document order, joined with a space (ECMA-48, the papers).
//! - [`lzw`]: `LZWDecode` streams (`[[source: pdf-reference-1-4]]`, 9-12-bit
//!   codes, clear 256, EOD 257, `EarlyChange` 1, its default), the strings
//!   of each text object joined (ECMA-35, TR/53). zlib cannot read these, so
//!   a Flate extractor finds nothing in them and every search misses.
//! - [`encrypted`]: the standard security handler with an empty user
//!   password (`[[source: pdf-reference-1-4]]`, Algorithms 3.1 and 3.2, RC4
//!   keyed through MD5), then inflated, the strings of each `TJ` and `Tj`
//!   (Adobe's technical notes). A plain inflater sees zero streams in them.
//!
//! **A search on what this returns is meaningless without a positive
//! control.** Each text layer is letter-spaced (`Operat i ng sy st em`), so
//! an exact search for `framed` cannot match `f r amed`: [`fold`] strips
//! both sides to `[a-z0-9]`. Which control each document carries is
//! `pdf_search`'s table.

use std::sync::LazyLock;

use baud_domain::zlib::{Inflate, Inflated, ZlibError};
use regex::bytes::{Captures, Regex};

use crate::html::Count;
use crate::{md5, py, text};

/// What stopped an extraction, printed alone as the script's `SystemExit`
/// printed it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stop(pub String);

fn stop<T>(message: &str) -> Result<T, Stop> {
    Err(Stop(message.to_string()))
}

/// A document's text and how many streams it came from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Extract {
    pub text: String,
    pub streams: Count,
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// `zlib.decompress`: a whole stream, or nothing. A stream the inflater
/// refuses or that ends early is `None`; bytes after the trailer are
/// ignored, as zlib ignores them.
///
/// The inflater writes into a buffer of the stream's exact length, and a
/// PDF stream declares none, so a first pass finds it: too small a buffer
/// is [`ZlibError::Overrun`], too large is [`ZlibError::Short`] once the
/// final block is in. The pass at the exact length is the one that reads
/// the trailer, so every stream returned has had its Adler-32 checked.
pub fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut len = data.len().saturating_mul(4).max(1 << 16);
    loop {
        let mut out = vec![0u8; len];
        let mut z = Inflate::NEW;
        match z.feed(data, &mut out) {
            Ok(Inflated::Done) => return Some(out),
            Ok(Inflated::More) => return None,
            Err(ZlibError::Overrun) => len = len.checked_mul(2)?,
            Err(ZlibError::Short) if z.written() < len => len = z.written(),
            Err(_) => return None,
        }
    }
}

static STREAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)stream\r?\n").expect("the stream pattern"));
static ENDSTREAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)endstream").expect("the endstream pattern"));
static STRING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)\((?:\\.|[^\\()])*\)").expect("the string pattern"));
static ESCAPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)\\([()\\])").expect("the escape pattern"));

/// Every stream that inflates, in file order. `stream\r?\n` also matches
/// the end of `endstream`, and what follows that is not zlib, which is why
/// a refusal is a skip: images and fonts are skipped the same way.
fn flate_streams(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for m in STREAM.find_iter(data) {
        let Some(end) = ENDSTREAM.find_at(data, m.end()) else {
            continue;
        };
        if let Some(inflated) = inflate(&data[m.end()..end.start()]) {
            out.push(inflated);
        }
    }
    out
}

/// Text out of Flate streams in the clear.
pub fn flate(data: &[u8]) -> Extract {
    let streams = flate_streams(data);
    let mut parts = Vec::new();
    for chunk in &streams {
        for m in STRING.find_iter(chunk) {
            let inner = &m.as_bytes()[1..m.len() - 1];
            parts.push(latin1(&ESCAPE.replace_all(inner, &b"$1"[..])));
        }
    }
    Extract {
        text: parts.join(" "),
        streams: Count(streams.len()),
    }
}

/// `LZWDecode`. `None` where the first code after a clear names no entry;
/// a later code past the table takes the `KwKwK` branch, as the original
/// decoder did, and the table is never capped, so `nbits` rests at 12.
pub fn lzw_decode(data: &[u8]) -> Option<Vec<u8>> {
    const CLEAR: usize = 256;
    const END: usize = 257;
    let mut table: Vec<Vec<u8>> = (0..=255u8).map(|b| vec![b]).collect();
    table.push(Vec::new());
    table.push(Vec::new());
    let mut out = Vec::new();
    let mut prev: Option<Vec<u8>> = None;
    let (mut at, mut nbits) = (0, 9);
    let total = data.len() * 8;
    while at + nbits <= total {
        let mut code = 0;
        for _ in 0..nbits {
            let bit = (data[at >> 3] >> (7 - (at & 7))) & 1;
            code = (code << 1) | usize::from(bit);
            at += 1;
        }
        if code == CLEAR {
            table.truncate(END + 1);
            nbits = 9;
            prev = None;
            continue;
        }
        if code == END {
            break;
        }
        let entry = match &prev {
            None => table.get(code)?.clone(),
            Some(before) => match table.get(code) {
                Some(found) => {
                    let found = found.clone();
                    let mut next = before.clone();
                    next.extend(found.first());
                    table.push(next);
                    found
                }
                None => {
                    let mut grown = before.clone();
                    grown.extend(before.first());
                    table.push(grown.clone());
                    grown
                }
            },
        };
        out.extend_from_slice(&entry);
        prev = Some(entry);
        let n = table.len() + 1;
        nbits = match n {
            2048.. => 12,
            1024.. => 11,
            512.. => 10,
            _ => 9,
        };
    }
    Some(out)
}

static TEXT_OBJECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)\((?:\\.|[^\\)])*\)|BT|ET").expect("the text object"));
static LZW_ESCAPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)\\([nrtbf()\\])").expect("the named escapes"));
static LZW_OCTAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)\\([0-7]{1,3})").expect("the octal escapes"));

/// The strings of a content stream, a line to a text object. Two passes
/// over the escapes, named then octal, as the original ran them: `\\101`
/// comes out `A`, where a one-pass decoder gives `\101`.
pub fn strings_of(content: &[u8]) -> String {
    let mut out = String::new();
    for m in TEXT_OBJECT.find_iter(content) {
        let t = m.as_bytes();
        if t == b"ET" {
            out.push('\n');
        } else if t.first() == Some(&b'(') {
            let named =
                LZW_ESCAPE.replace_all(&t[1..t.len() - 1], |c: &Captures<'_>| match c[1][0] {
                    b'n' => vec![b'\n'],
                    b't' => vec![b' '],
                    b'r' | b'b' | b'f' => Vec::new(),
                    other => vec![other],
                });
            let octal = LZW_OCTAL.replace_all(&named, |c: &Captures<'_>| octal(&c[1]));
            out.push_str(&latin1(&octal));
        }
    }
    out
}

/// One to three octal digits, the top bit of a third digit dropped.
fn octal(digits: &[u8]) -> [u8; 1] {
    let n = digits
        .iter()
        .fold(0u32, |n, &d| n * 8 + u32::from(d - b'0'))
        & 0xff;
    [u8::try_from(n).expect("masked to a byte")]
}

static LZW_STREAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s-u)<<(.*?)>>\s*stream\r?\n").expect("the LZW stream"));
static LENGTH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)/Length\s+(\d+)").expect("the length"));

/// An integer the patterns above found, past `usize` taken as "as far as
/// there is": every use is a slice end, and Python's slice clamps too.
fn digits(bytes: &[u8]) -> Count {
    Count(
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(usize::MAX),
    )
}

/// `data[start:start + len]`.
fn clamped(data: &[u8], start: Count, len: Count) -> &[u8] {
    let end = start.0.saturating_add(len.0).min(data.len());
    &data[start.0.min(end)..end]
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Text out of `LZWDecode` streams, headed stream by stream. Only a stream
/// with text counts, so the numbers in the headers are not file order's.
pub fn lzw(data: &[u8]) -> Extract {
    let mut text = String::new();
    let mut n = 0;
    for c in LZW_STREAM.captures_iter(data) {
        let dict = &c[1];
        let Some(length) = LENGTH.captures(dict) else {
            continue;
        };
        if !contains(dict, b"/LZWDecode") {
            continue;
        }
        let start = c.get(0).expect("the whole match").end();
        let Some(content) = lzw_decode(clamped(data, Count(start), digits(&length[1]))) else {
            continue;
        };
        let t = strings_of(&content);
        if !text::strip(&t).is_empty() {
            n += 1;
            text.push_str(&format!("\n=== stream {n}\n{t}"));
        }
    }
    Extract {
        text,
        streams: Count(n),
    }
}

/// RC4. No key here is empty: revision 2's is five bytes, and an object's
/// is at least five.
pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: Vec<u8> = (0..=255).collect();
    let mut j = 0;
    for i in 0..256 {
        j = (j + usize::from(s[i]) + usize::from(key[i % key.len()])) & 255;
        s.swap(i, j);
    }
    let (mut i, mut j) = (0, 0);
    data.iter()
        .map(|&byte| {
            i = (i + 1) & 255;
            j = (j + usize::from(s[i])) & 255;
            s.swap(i, j);
            byte ^ s[(usize::from(s[i]) + usize::from(s[j])) & 255]
        })
        .collect()
}

fn from_hex(hex: &[u8]) -> Result<Vec<u8>, Stop> {
    let nibble = |b: &u8| char::from(*b).to_digit(16);
    let nibbles: Vec<u32> = hex.iter().filter_map(nibble).collect();
    if !nibbles.len().is_multiple_of(2) {
        return stop("an odd number of hex digits");
    }
    Ok(nibbles
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::try_from(p[0] * 16 + p[1]).expect("two hex digits"))
        .collect())
}

/// A PDF literal string `(...)` with its escapes, or a hex string.
pub fn pdf_string(s: &[u8]) -> Result<Vec<u8>, Stop> {
    let inner = &s[1..s.len() - 1];
    if s.first() == Some(&b'<') {
        return from_hex(inner);
    }
    let mut body = Vec::with_capacity(inner.len());
    let mut bytes = inner.iter().copied().peekable();
    while let Some(b) = bytes.next() {
        if b == b'\r' {
            bytes.next_if_eq(&b'\n');
            body.push(b'\n');
        } else {
            body.push(b);
        }
    }
    let mut out = Vec::with_capacity(body.len());
    let mut rest = &body[..];
    while let Some((&c, tail)) = rest.split_first() {
        rest = tail;
        let Some((&n, tail)) = tail.split_first().filter(|_| c == b'\\') else {
            out.push(c);
            continue;
        };
        rest = tail;
        match n {
            b'n' => out.push(10),
            b'r' => out.push(13),
            b't' => out.push(9),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'0'..=b'7' => {
                let more = tail
                    .iter()
                    .take(2)
                    .take_while(|d| (b'0'..=b'7').contains(*d))
                    .count();
                let mut digits = vec![n];
                digits.extend_from_slice(&tail[..more]);
                rest = &tail[more..];
                out.extend_from_slice(&octal(&digits));
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

fn find_string(dict: &[u8], key: &str) -> Result<Vec<u8>, Stop> {
    let pattern = format!(
        r"(?s-u){}\s*(\((?:\\.|\((?:\\.|[^\\()])*\)|[^\\()])*\)|<[0-9A-Fa-f\s]*>)",
        regex::escape(key)
    );
    let re = Regex::new(&pattern).expect("the string entry's pattern");
    match re.captures(dict) {
        Some(c) => pdf_string(&c[1]),
        None => Err(Stop(format!("{key} not found in Encrypt dict"))),
    }
}

/// `int(group) & 0xFFFFFFFF` of a `-?\d+`, little-endian: Algorithm 3.2's
/// step 4 reads `P` as an unsigned four-byte integer, low-order byte first.
fn permissions(group: &[u8]) -> [u8; 4] {
    let (negative, magnitude) = match group.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, group),
    };
    let n = magnitude.iter().fold(0u32, |n, &d| {
        n.wrapping_mul(10).wrapping_add(u32::from(d - b'0'))
    });
    if negative { n.wrapping_neg() } else { n }.to_le_bytes()
}

/// Algorithm 3.2's padding string, which stands in for an empty password.
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

static ENCRYPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)/Encrypt\s+(\d+)\s+(\d+)\s+R").expect("/Encrypt"));
static PERMISSIONS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)/P\s+(-?\d+)").expect("/P"));
static REVISION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u)/R\s+(\d+)").expect("/R"));
static ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)/ID\s*\[\s*(<[0-9A-Fa-f]+>)").expect("/ID"));

/// `digest[:n]`, at most the digest's sixteen bytes.
fn key_truncated(digest: &[u8; 16], n: Count) -> Vec<u8> {
    digest[..n.0.min(16)].to_vec()
}

/// Algorithm 3.2, the empty user password, held to the document's own `/U`
/// where revision 2 lets it be: Algorithm 3.4's `U` is `PAD` under the key,
/// so a wrong derivation is caught here rather than read as a document with
/// no text.
pub fn file_key(data: &[u8]) -> Result<Vec<u8>, Stop> {
    let Some(enc) = ENCRYPT.captures(data) else {
        return stop("no /Encrypt: this PDF is not encrypted; it is a Flate document");
    };
    let pattern = format!(
        r"(?s-u)(?:^|[^0-9]){}\s+{}\s+obj\s*(<<.*?>>)\s*endobj",
        String::from_utf8_lossy(&enc[1]),
        String::from_utf8_lossy(&enc[2])
    );
    let re = Regex::new(&pattern).expect("the Encrypt object's pattern");
    let Some(object) = re.captures(data) else {
        return stop("the /Encrypt object is not in the file");
    };
    let d = &object[1];
    let o = find_string(d, "/O")?;
    let Some(p) = PERMISSIONS.captures(d) else {
        return stop("no /P in the Encrypt dict");
    };
    let Some(r) = REVISION.captures(d) else {
        return stop("no /R in the Encrypt dict");
    };
    let revision = digits(&r[1]).0;
    let n = match LENGTH.captures(d) {
        Some(l) if revision >= 3 => Count(digits(&l[1]).0 / 8),
        _ => Count(5),
    };
    let id = match ID.captures(data) {
        Some(c) => from_hex(&c[1][1..c[1].len() - 1])?,
        None => Vec::new(),
    };
    let mut input = PAD.to_vec();
    input.extend_from_slice(&o[..o.len().min(32)]);
    input.extend_from_slice(&permissions(&p[1]));
    input.extend_from_slice(&id);
    let mut key = key_truncated(&md5::digest(&input), n);
    if revision >= 3 {
        for _ in 0..50 {
            key = key_truncated(&md5::digest(&key), n);
        }
    }
    let u = find_string(d, "/U")?;
    if revision == 2 && rc4(&key, &PAD)[..] != u[..u.len().min(32)] {
        return stop(
            "file key does not reproduce /U -- the key derivation is wrong, not the document",
        );
    }
    Ok(key)
}

fn space(b: &u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn after_digits(rest: &[u8]) -> Option<(&[u8], &[u8])> {
    let n = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    (n > 0).then(|| rest.split_at(n))
}

fn after_space(rest: &[u8]) -> &[u8] {
    &rest[rest.iter().take_while(|b| space(b)).count()..]
}

/// One `N G obj << ... >> stream` the original's pattern finds, which the
/// `regex` crate cannot say: `(?:(?!endobj).)*?` stops the dictionary at an
/// `endobj`, and it is the shortest ending in `>>` that a `stream` follows.
struct Object<'a> {
    number: &'a [u8],
    generation: &'a [u8],
    dict: &'a [u8],
    /// The bytes after `stream` and its line end.
    content: &'a [u8],
}

fn object_at(at: &[u8]) -> Option<Object<'_>> {
    let (number, rest) = after_digits(at)?;
    let gap = after_space(rest);
    if gap.len() == rest.len() {
        return None;
    }
    let (generation, rest) = after_digits(gap)?;
    let gap = after_space(rest);
    if gap.len() == rest.len() {
        return None;
    }
    let rest = after_space(gap.strip_prefix(b"obj")?);
    let body = rest.strip_prefix(b"<<")?;
    let mut scan = body;
    loop {
        if let Some(close) = scan.strip_prefix(b">>") {
            let tail = after_space(close);
            if let Some(tail) = tail.strip_prefix(b"stream") {
                let line = tail.strip_prefix(b"\r").unwrap_or(tail);
                if let Some(content) = line.strip_prefix(b"\n") {
                    let dict_len = rest.len() - close.len();
                    return Some(Object {
                        number,
                        generation,
                        dict: &rest[..dict_len],
                        content,
                    });
                }
            }
        }
        if scan.starts_with(b"endobj") {
            return None;
        }
        scan = scan.split_first()?.1;
    }
}

static LENGTH_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)/Length\s+").expect("the length key"));
static INDIRECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)^\s+\d+\s+R").expect("an indirect reference"));

/// `/Length\s+(\d+)(?!\s+\d+\s+R)`, with the original's backtrack. The
/// lookahead refuses `/Length 123 0 R`, but `\d+` then gives back a digit
/// and `12` passes it, since the `3` after it is no space. So an indirect
/// length is read short, and the stream's Flate fails and is skipped, or a
/// clear stream is cut. One digit has nothing to give back and the search
/// moves on. Adobe TN #5177 has nine such lengths, and its stream count is
/// what this reads.
fn direct_length(dict: &[u8]) -> Option<Count> {
    for key in LENGTH_KEY.find_iter(dict) {
        let rest = &dict[key.end()..];
        let Some((number, tail)) = after_digits(rest) else {
            continue;
        };
        if !INDIRECT.is_match(tail) {
            return Some(digits(number));
        }
        if number.len() > 1 {
            return Some(digits(&number[..number.len() - 1]));
        }
    }
    None
}

/// Algorithm 3.1: each object's stream under a key from the file key, its
/// number and its generation, and inflated where it says it is Flate. A
/// number past the three bytes the algorithm gives it stops the run, as
/// `int.to_bytes` raised; the algorithm's own "low-order" bytes would read
/// on. No pinned document has one, so which is right has never mattered.
fn encrypted_streams(data: &[u8], key: &[u8]) -> Result<Vec<Vec<u8>>, Stop> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let starts = data[at].is_ascii_digit() && (at == 0 || !data[at - 1].is_ascii_digit());
        let object = if starts { object_at(&data[at..]) } else { None };
        let Some(object) = object else {
            at += 1;
            continue;
        };
        at = data.len() - object.content.len();
        let Some(length) = direct_length(object.dict) else {
            continue;
        };
        let number = u32::try_from(digits(object.number).0)
            .ok()
            .filter(|n| *n < 1 << 24);
        let generation = u16::try_from(digits(object.generation).0).ok();
        let (Some(number), Some(generation)) = (number, generation) else {
            return stop(
                "an object number past Algorithm 3.1's three bytes, or a generation past its two",
            );
        };
        let mut input = key.to_vec();
        input.extend_from_slice(&number.to_le_bytes()[..3]);
        input.extend_from_slice(&generation.to_le_bytes());
        let object_key = key_truncated(&md5::digest(&input), Count(key.len() + 5));
        let clear = rc4(&object_key, clamped(object.content, Count(0), length));
        if contains(object.dict, b"/FlateDecode") {
            if let Some(inflated) = inflate(&clear) {
                out.push(inflated);
            }
        } else {
            out.push(clear);
        }
    }
    Ok(out)
}

static SHOWN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s-u)\[((?:\((?:\\.|[^\\)])*\)|[^\]])*)\]\s*TJ|\(((?:\\.|[^\\)])*)\)\s*Tj")
        .expect("the text-showing operators")
});
static LITERAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s-u)\((?:\\.|[^\\)])*\)").expect("a literal string"));

/// Text out of the encrypted streams: each `TJ` or `Tj`'s strings joined,
/// a space after each operator. A key that does not reach the text is
/// caught by [`file_key`] or, past it, by the positive control.
pub fn encrypted(data: &[u8]) -> Result<Extract, Stop> {
    let key = file_key(data)?;
    let streams = encrypted_streams(data, &key)?;
    let mut text = String::new();
    for s in &streams {
        for tj in SHOWN.captures_iter(s) {
            let body = match (tj.get(1), tj.get(2)) {
                (Some(array), _) => array.as_bytes().to_vec(),
                (None, Some(one)) => [b"(", one.as_bytes(), b")"].concat(),
                (None, None) => unreachable!("one branch of the alternation matched"),
            };
            for lit in LITERAL.find_iter(&body) {
                text.push_str(&latin1(&pdf_string(lit.as_bytes())?));
            }
            text.push(' ');
        }
    }
    Ok(Extract {
        text,
        streams: Count(streams.len()),
    })
}

/// Lowercase and drop everything that is not `[a-z0-9]`: both sides of
/// every comparison. [`flatten`] keeps exactly these characters. When its
/// predicate once differed, the offsets it returned ran out of step and
/// every window landed in the wrong section, a search reporting the wrong
/// place, which looks like an answer.
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

/// The folded text, and where each of its characters came from.
pub struct Flat {
    pub flat: String,
    pub index: Vec<usize>,
}

/// [`fold`] of a text with an offset per kept character. It is Latin-1, so
/// lowercasing one character at a time can reach `[a-z]` only from
/// `[A-Z]`, where [`fold`]'s whole-string form is the same.
pub fn flatten(text: &str) -> Flat {
    let mut flat = String::new();
    let mut index = Vec::new();
    for (at, c) in text.char_indices() {
        let lo = c.to_ascii_lowercase();
        if lo.is_ascii_lowercase() || lo.is_ascii_digit() {
            flat.push(lo);
            index.push(at);
        }
    }
    Flat { flat, index }
}

/// Best-effort re-joining of letter-spaced words, for reading: whitespace
/// runs become one space, and then `(?<=\b\w) (?=\w\b)` goes, a space
/// between two one-letter words. The second is decided on the collapsed
/// text, never on what has already been joined.
pub fn unspace(s: &str) -> String {
    let mut collapsed: Vec<char> = Vec::with_capacity(s.len());
    let mut run = false;
    for c in s.chars() {
        if text::space(c) {
            if !run {
                collapsed.push(' ');
            }
            run = true;
        } else {
            collapsed.push(c);
            run = false;
        }
    }
    let word = |i: usize| collapsed.get(i).is_some_and(|c| py::word(*c));
    let mut out = String::with_capacity(collapsed.len());
    for (i, &c) in collapsed.iter().enumerate() {
        let lone_before = i >= 1 && word(i - 1) && (i == 1 || !word(i - 2));
        let lone_after = word(i + 1) && !word(i + 2);
        if !(c == ' ' && lone_before && lone_after) {
            out.push(c);
        }
    }
    out
}
