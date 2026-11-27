//! HTML tokenised as Python 3.13's `html.parser.HTMLParser` tokenises it,
//! with `convert_charrefs=True`, and `html.unescape`.
//!
//! The article-text pins in `MANIFEST.toml` were taken through that parser,
//! so a text read by any other one -- html5ever's tree builder among them,
//! which moves and closes elements -- would hash differently on the same
//! bytes. This is the tokeniser alone: start tags with their attributes,
//! end tags and data, in document order, with no tree. Its rules are
//! `Lib/html/parser.py`'s, each regular expression there written out as
//! the scan it performs, backtracking included where the pattern can
//! backtrack: an attribute value that opens a quote it never closes gives
//! the whitespace before the quote back and takes an empty bare value.
//!
//! The named references are the HTML Standard's list
//! ([[source: whatwg-html-entities]]), pinned as
//! `docs/research/sources/whatwg-html-entities.json`; it holds the 2,231
//! names Python's `html.entities.html5` holds, with the same characters.
//!
//! Positions are code points, as Python's string indices are: the parser's
//! only arithmetic on them, the 34-character window it searches for a
//! reference cut at the end of a feed, counts characters.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde_json::Value;

/// The named character references, without their leading `&`.
static ENTITIES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let json: Value = serde_json::from_str(include_str!(
        "../../../docs/research/sources/whatwg-html-entities.json"
    ))
    .expect("the pinned entity list is JSON");
    json.as_object()
        .expect("the entity list is an object")
        .iter()
        .map(|(name, e)| {
            let chars = e["characters"]
                .as_str()
                .expect("each entity has characters");
            (name.trim_start_matches('&').to_string(), chars.to_string())
        })
        .collect()
});

/// How many named references the pinned list holds.
pub fn entity_count() -> Count {
    Count(ENTITIES.len())
}

/// A number of things counted.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Count(pub usize);

/// A position in the text, in code points.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct At(pub usize);

impl At {
    fn next(self) -> Self {
        Self(self.0 + 1)
    }
    fn plus(self, n: Count) -> Self {
        Self(self.0 + n.0)
    }
}

/// HTML's ASCII whitespace as the parser's classes spell it, `[\t\n\r\f ]`.
fn ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{0c}' | ' ')
}

/// One attribute: its name, lowercased, and its value, `None` when it has
/// no `=`.
pub type Attr = (String, Option<String>);

/// What the tokeniser reports, as `HTMLParser`'s handlers receive it.
pub trait Handler {
    /// `handle_starttag`.
    fn start(&mut self, tag: &str, attrs: &[Attr]);
    /// `handle_endtag`.
    fn end(&mut self, tag: &str);
    /// `handle_data`, references already decoded where the parser decodes
    /// them.
    fn data(&mut self, data: &str);
}

/// Whether a feed may be followed by more text, as `goahead`'s `end`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Feed {
    More,
    Last,
}

/// Whether text in the current element has its references decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Escapable {
    Yes,
    No,
}

/// The raw-text and escapable raw-text elements, `CDATA_CONTENT_ELEMENTS`
/// and `RCDATA_CONTENT_ELEMENTS`.
const CDATA: [&str; 6] = ["script", "style", "xmp", "iframe", "noembed", "noframes"];
const RCDATA: [&str; 2] = ["textarea", "title"];

/// A whole document, fed at once and closed, as `feed(text); close()`.
pub fn tokenise(text: &str, h: &mut impl Handler) {
    let mut p = Parser {
        raw: text.chars().collect(),
        cdata: None,
        escapable: Escapable::Yes,
    };
    p.goahead(Feed::More, h);
    p.goahead(Feed::Last, h);
}

struct Parser {
    raw: Vec<char>,
    cdata: Option<String>,
    escapable: Escapable,
}

impl Parser {
    fn len(&self) -> At {
        At(self.raw.len())
    }

    fn at(&self, i: At) -> Option<char> {
        self.raw.get(i.0).copied()
    }

    fn text(&self, from: At, to: At) -> String {
        self.raw[from.0..to.0].iter().collect()
    }

    fn starts(&self, i: At, s: &str) -> bool {
        let mut k = i;
        for c in s.chars() {
            if self.at(k) != Some(c) {
                return false;
            }
            k = k.next();
        }
        true
    }

    fn find(&self, c: char, from: At) -> Option<At> {
        self.raw
            .get(from.0..)?
            .iter()
            .position(|&x| x == c)
            .map(|p| At(from.0 + p))
    }

    fn find_str(&self, s: &str, from: At) -> Option<At> {
        (from.0..self.raw.len())
            .map(At)
            .find(|&k| self.starts(k, s))
    }

    fn letter(&self, i: At) -> bool {
        self.at(i).is_some_and(|c| c.is_ascii_alphabetic())
    }

    fn emit(&self, from: At, to: At, h: &mut impl Handler) {
        let s = self.text(from, to);
        match self.escapable {
            Escapable::Yes => h.data(&unescape(&s)),
            Escapable::No => h.data(&s),
        }
    }

    /// `</elem(?=[\t\n\r\f />])`, ASCII case folded, from `i`; for
    /// `plaintext`, `\Z`.
    fn cdata_end(&self, elem: &str, i: At) -> Option<At> {
        if elem == "plaintext" {
            return Some(self.len());
        }
        let name: Vec<char> = elem.chars().collect();
        (i.0..self.raw.len()).map(At).find(|&k| {
            if !self.starts(k, "</") {
                return false;
            }
            let body = k.plus(Count(2));
            let same = name.iter().enumerate().all(|(n, c)| {
                self.at(body.plus(Count(n)))
                    .is_some_and(|x| x.eq_ignore_ascii_case(c))
            });
            same && self
                .at(body.plus(Count(name.len())))
                .is_some_and(|c| ws(c) || c == '/' || c == '>')
        })
    }

    fn goahead(&mut self, feed: Feed, h: &mut impl Handler) {
        let n = self.len();
        let mut i = At(0);
        while i < n {
            let j = match self.cdata.clone() {
                None => match self.find('<', i) {
                    Some(j) => j,
                    None => {
                        let from = At(i.0.max(n.0.saturating_sub(34)));
                        let amp = (from.0..n.0)
                            .rev()
                            .map(At)
                            .find(|&k| self.at(k) == Some('&'));
                        if let Some(a) = amp {
                            let ended = (a.0..n.0).any(|k| {
                                matches!(self.raw[k], '\t' | '\n' | '\r' | '\u{0c}' | ' ' | ';')
                            });
                            if !ended {
                                break;
                            }
                        }
                        n
                    }
                },
                Some(elem) => match self.cdata_end(&elem, i) {
                    Some(j) => j,
                    None => break,
                },
            };
            if i < j {
                self.emit(i, j, h);
            }
            i = j;
            if i == n {
                break;
            }
            // With references converted, text stops only at a `<`.
            let k = if self.at(i.next()).is_some_and(|c| c.is_ascii_alphabetic()) {
                self.starttag(i, h)
            } else if self.starts(i, "</") {
                self.endtag(i, h)
            } else if self.starts(i, "<!--") {
                self.comment(i)
            } else if self.starts(i, "<?") {
                self.find('>', i.plus(Count(2))).map(At::next)
            } else if self.starts(i, "<!") {
                self.declaration(i)
            } else if i.next() < n || feed == Feed::Last {
                h.data("<");
                Some(i.next())
            } else {
                break;
            };
            i = match k {
                Some(k) => k,
                None if feed == Feed::More => break,
                None => {
                    // An unterminated construct at the end: only `</`
                    // alone is data; the rest is a tag or a comment.
                    if self.starts(i, "</") && i.plus(Count(2)) == n {
                        h.data("</");
                    }
                    n
                }
            };
        }
        if feed == Feed::Last && i < n {
            self.emit(i, n, h);
            i = n;
        }
        self.raw.drain(..i.0);
    }

    /// `parse_comment`: `--!?>` searched, else `-?>` at the start.
    fn comment(&self, i: At) -> Option<At> {
        let body = i.plus(Count(4));
        let closed = (body.0..self.raw.len()).map(At).find_map(|k| {
            if self.starts(k, "--!>") {
                Some(k.plus(Count(4)))
            } else if self.starts(k, "-->") {
                Some(k.plus(Count(3)))
            } else {
                None
            }
        });
        closed.or_else(|| {
            if self.starts(body, "->") {
                Some(body.plus(Count(2)))
            } else if self.starts(body, ">") {
                Some(body.next())
            } else {
                None
            }
        })
    }

    /// `parse_html_declaration`, `<!--` already taken.
    fn declaration(&self, i: At) -> Option<At> {
        if self.starts(i, "<![CDATA[") {
            return self
                .find_str("]]>", i.plus(Count(9)))
                .map(|j| j.plus(Count(3)));
        }
        let head: String = self.raw[i.0..(i.0 + 9).min(self.raw.len())]
            .iter()
            .collect();
        if head.to_lowercase() == "<!doctype" {
            return self.find('>', i.plus(Count(9))).map(At::next);
        }
        self.find('>', i.plus(Count(2))).map(At::next)
    }

    /// `tagfind_tolerant` from `p`: the name and where the match ends.
    fn tagfind(&self, p: At) -> (String, At) {
        let mut k = p.next();
        while self.at(k).is_some_and(|c| !(ws(c) || c == '/' || c == '>')) {
            k = k.next();
        }
        let name = self.text(p, k).to_lowercase();
        (name, self.skip_space_or_lone_slash(k))
    }

    /// `(?:[\t\n\r\f ]|/(?!>))*`.
    fn skip_space_or_lone_slash(&self, mut k: At) -> At {
        loop {
            match self.at(k) {
                Some(c) if ws(c) => k = k.next(),
                Some('/') if self.at(k.next()) != Some('>') => k = k.next(),
                _ => return k,
            }
        }
    }

    /// `[\t\n\r\f /]*`.
    fn skip_space_or_slash(&self, mut k: At) -> At {
        while self.at(k).is_some_and(|c| ws(c) || c == '/') {
            k = k.next();
        }
        k
    }

    fn skip_ws(&self, mut k: At) -> At {
        while self.at(k).is_some_and(ws) {
            k = k.next();
        }
        k
    }

    /// An attribute name at `k`, behind its lookbehind
    /// `(?<=['"\t\n\r\f /])`: where it ends.
    fn attr_name(&self, k: At) -> Option<At> {
        let before = k.0.checked_sub(1).and_then(|b| self.raw.get(b).copied())?;
        if !(ws(before) || matches!(before, '\'' | '"' | '/')) {
            return None;
        }
        let first = self.at(k)?;
        if ws(first) || first == '/' || first == '>' {
            return None;
        }
        let mut e = k.next();
        while self
            .at(e)
            .is_some_and(|c| !(ws(c) || matches!(c, '/' | '=' | '>')))
        {
            e = e.next();
        }
        Some(e)
    }

    /// The value indicator `[ws]*=[ws]*('[^']*'|"[^"]*"|(?!['"])[^>ws]*)`
    /// at `k`: where the value starts and where it ends. The whitespace
    /// after `=` is given back one character at a time when a quote opens
    /// and never closes, as the backtracking engine gives it back.
    fn value(&self, k: At) -> Option<(At, At)> {
        let eq = self.skip_ws(k);
        if self.at(eq) != Some('=') {
            return None;
        }
        let lo = eq.next();
        let hi = self.skip_ws(lo);
        (lo.0..=hi.0).rev().map(At).find_map(|b| match self.at(b) {
            Some(q @ ('\'' | '"')) => self.find(q, b.next()).map(|e| (b, e.next())),
            _ => {
                let mut e = b;
                while self.at(e).is_some_and(|c| !(ws(c) || c == '>')) {
                    e = e.next();
                }
                Some((b, e))
            }
        })
    }

    /// `locatetagend` from `p`: where it ends.
    fn tag_end(&self, p: At) -> At {
        let mut k = p.next();
        while self.at(k).is_some_and(|c| !(ws(c) || c == '/' || c == '>')) {
            k = k.next();
        }
        k = self.skip_space_or_slash(k);
        while let Some(e) = self.attr_name(k) {
            k = self.value(e).map_or(e, |(_, end)| end);
            k = self.skip_space_or_slash(k);
        }
        if self.at(k) == Some('>') {
            k = k.next();
        }
        k
    }

    /// `parse_starttag`: where the tag ends, or `None` when it is not yet
    /// closed.
    fn starttag(&mut self, i: At, h: &mut impl Handler) -> Option<At> {
        let endpos = self.tag_end(i.next());
        if endpos.0 == 0 || self.raw[endpos.0 - 1] != '>' {
            return None;
        }
        let (tag, mut k) = self.tagfind(i.next());
        let mut attrs: Vec<Attr> = Vec::new();
        while k < endpos {
            let Some(e) = self.attr_name(k) else { break };
            let name = self.text(k, e).to_lowercase();
            let (value, after) = match self.value(e) {
                Some((b, end)) => {
                    let raw = self.text(b, end);
                    let quoted = raw.len() >= 2
                        && ((raw.starts_with('\'') && raw.ends_with('\''))
                            || (raw.starts_with('"') && raw.ends_with('"')));
                    let v = if quoted {
                        raw[1..raw.len() - 1].to_string()
                    } else {
                        raw
                    };
                    let v = if v.is_empty() { v } else { unescape_attr(&v) };
                    (Some(v), end)
                }
                None => (None, e),
            };
            attrs.push((name, value));
            k = self.skip_space_or_lone_slash(after);
        }
        let rest = if k < endpos {
            self.text(k, endpos)
        } else {
            String::new()
        };
        let end = crate::text::strip(&rest);
        if end != ">" && end != "/>" {
            h.data(&self.text(i, endpos));
            return Some(endpos);
        }
        if end.ends_with("/>") {
            h.start(&tag, &attrs);
            h.end(&tag);
        } else {
            h.start(&tag, &attrs);
            if CDATA.contains(&tag.as_str()) || tag == "plaintext" {
                self.cdata = Some(tag);
                self.escapable = Escapable::No;
            } else if RCDATA.contains(&tag.as_str()) {
                self.cdata = Some(tag);
                self.escapable = Escapable::Yes;
            }
        }
        Some(endpos)
    }

    /// `parse_endtag`.
    fn endtag(&mut self, i: At, h: &mut impl Handler) -> Option<At> {
        let body = i.plus(Count(2));
        self.find('>', body)?;
        if !self.letter(body) {
            if self.at(body) == Some('>') {
                return Some(body.next());
            }
            return self.find('>', body).map(At::next);
        }
        let j = self.tag_end(body);
        if self.raw[j.0 - 1] != '>' {
            return None;
        }
        let (tag, _) = self.tagfind(body);
        h.end(&tag);
        self.cdata = None;
        self.escapable = Escapable::Yes;
        Some(j)
    }
}

/// A code point a numeric reference names, saturated past the code space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Code(u32);

/// The base a numeric reference is written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Radix {
    Dec,
    Hex,
}

/// `_invalid_charrefs`: what a numeric reference to these decodes to.
fn invalid_charref(Code(n): Code) -> Option<char> {
    Some(match n {
        0x00 => '\u{fffd}',
        0x0d => '\r',
        0x80 => '\u{20ac}',
        0x81 => '\u{81}',
        0x82 => '\u{201a}',
        0x83 => '\u{0192}',
        0x84 => '\u{201e}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02c6}',
        0x89 => '\u{2030}',
        0x8a => '\u{0160}',
        0x8b => '\u{2039}',
        0x8c => '\u{0152}',
        0x8d => '\u{8d}',
        0x8e => '\u{017d}',
        0x8f => '\u{8f}',
        0x90 => '\u{90}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201c}',
        0x94 => '\u{201d}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02dc}',
        0x99 => '\u{2122}',
        0x9a => '\u{0161}',
        0x9b => '\u{203a}',
        0x9c => '\u{0153}',
        0x9d => '\u{9d}',
        0x9e => '\u{017e}',
        0x9f => '\u{0178}',
        _ => return None,
    })
}

/// `_invalid_codepoints`: a numeric reference to these decodes to nothing.
fn dropped(Code(n): Code) -> bool {
    matches!(n, 0x1..=0x8 | 0xb | 0xe..=0x1f | 0x7f..=0x9f | 0xfdd0..=0xfdef)
        || (n <= 0x10ffff && (n & 0xfffe) == 0xfffe)
}

/// A numeric reference's digits, saturating past the code space so a
/// number of any length reads as out of range, as Python's integers do.
fn number(digits: &str, radix: Radix) -> Code {
    let base = match radix {
        Radix::Dec => 10,
        Radix::Hex => 16,
    };
    Code(digits.chars().fold(0, |acc: u32, c| {
        let d = c.to_digit(base).unwrap_or(0);
        acc.saturating_mul(base).saturating_add(d).min(0x0011_0000)
    }))
}

/// `_replace_charref`, on the reference without its `&`.
fn replace(s: &str) -> String {
    if let Some(num) = s.strip_prefix('#') {
        let n = match num.strip_prefix(['x', 'X']) {
            Some(hex) => number(hex.trim_end_matches(';'), Radix::Hex),
            None => number(num.trim_end_matches(';'), Radix::Dec),
        };
        if let Some(c) = invalid_charref(n) {
            return c.to_string();
        }
        let Code(cp) = n;
        if matches!(cp, 0xd800..=0xdfff) || cp > 0x10ffff {
            return "\u{fffd}".to_string();
        }
        if dropped(n) {
            return String::new();
        }
        return char::from_u32(cp).map(String::from).unwrap_or_default();
    }
    if let Some(v) = ENTITIES.get(s) {
        return v.clone();
    }
    let chars: Vec<char> = s.chars().collect();
    for x in (2..chars.len()).rev() {
        let head: String = chars[..x].iter().collect();
        if let Some(v) = ENTITIES.get(&head) {
            let tail: String = chars[x..].iter().collect();
            return format!("{v}{tail}");
        }
    }
    format!("&{s}")
}

/// `html.unescape`: every reference the pattern
/// `&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)` matches,
/// leftmost first and never overlapping, replaced.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        match reference(&chars[i + 1..]) {
            Some(len) => {
                let r: String = chars[i + 1..i + 1 + len.0].iter().collect();
                out.push_str(&replace(&r));
                i += 1 + len.0;
            }
            None => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

/// The length of the reference after an `&`, by the unescape pattern.
fn reference(rest: &[char]) -> Option<Count> {
    let semi = |n: usize| Count(if rest.get(n) == Some(&';') { n + 1 } else { n });
    if rest.first() == Some(&'#') {
        let dec = rest[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if dec > 0 {
            return Some(semi(1 + dec));
        }
        if matches!(rest.get(1), Some('x' | 'X')) {
            let hex = rest[2..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            if hex > 0 {
                return Some(semi(2 + hex));
            }
        }
        return None;
    }
    let named = rest
        .iter()
        .take(32)
        .take_while(|&&c| !matches!(c, '\t' | '\n' | '\u{0c}' | ' ' | '<' | '&' | '#' | ';'))
        .count();
    (named > 0).then(|| semi(named))
}

/// `_unescape_attrvalue`: in an attribute, a named reference is decoded
/// only when it is a whole name the list holds and no `=` follows it.
fn unescape_attr(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let len = (chars[i] == '&')
            .then(|| attr_reference(&chars[i + 1..]))
            .flatten();
        let Some(len) = len else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let r: String = chars[i..i + 1 + len.0].iter().collect();
        if r.starts_with("&#") || (!r.ends_with('=') && ENTITIES.contains_key(&r[1..])) {
            out.push_str(&unescape(&r));
        } else {
            out.push_str(&r);
        }
        i += 1 + len.0;
    }
    out
}

/// `&(#[0-9]+|#[xX][0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*)[;=]?` after the `&`.
fn attr_reference(rest: &[char]) -> Option<Count> {
    let tail = |n: usize| {
        Count(if matches!(rest.get(n), Some(';' | '=')) {
            n + 1
        } else {
            n
        })
    };
    if rest.first() == Some(&'#') {
        let dec = rest[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if dec > 0 {
            return Some(tail(1 + dec));
        }
        if matches!(rest.get(1), Some('x' | 'X')) {
            let hex = rest[2..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            if hex > 0 {
                return Some(tail(2 + hex));
            }
        }
        return None;
    }
    if !rest.first().is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let n = 1 + rest[1..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    Some(tail(n))
}
