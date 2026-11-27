//! The keymap text's tokens: identifiers, `<KEY>` names, strings, numbers
//! and the punctuation the four sections use. Comments in both styles are
//! skipped. The grammar is `[[source: xkb-keymap-text-format]]`'s.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok<'a> {
    Ident(&'a str),
    /// `<NAME>`, the angle brackets stripped.
    KeyName(&'a str),
    Str(&'a str),
    Int(u32),
    /// A number with a fraction, which only the geometry section uses.
    Float,
    Punct(char),
    End,
}

pub struct Lexer<'a> {
    src: &'a str,
    at: usize,
    pub line: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Lexer<'a> {
        Lexer {
            src,
            at: 0,
            line: 1,
        }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.at..]
    }

    fn skip(&mut self) {
        loop {
            let r = self.rest();
            let Some(c) = r.chars().next() else { return };
            if c == '\n' {
                self.line += 1;
                self.at += 1;
            } else if c.is_whitespace() {
                self.at += c.len_utf8();
            } else if r.starts_with("//") || r.starts_with('#') {
                let n = r.find('\n').unwrap_or(r.len());
                self.at += n;
            } else if r.starts_with("/*") {
                let n = r.find("*/").map_or(r.len(), |i| i + 2);
                self.line += r[..n].matches('\n').count();
                self.at += n;
            } else {
                return;
            }
        }
    }

    /// Take the next token.
    pub fn take(&mut self) -> Tok<'a> {
        self.skip();
        let r = self.rest();
        let Some(c) = r.chars().next() else {
            return Tok::End;
        };
        if c == '<' {
            let end = r.find('>').unwrap_or(r.len());
            self.at += end + 1;
            return Tok::KeyName(&r[1..end]);
        }
        if c == '"' {
            let mut i = 1;
            let b = r.as_bytes();
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            self.at += (i + 1).min(r.len());
            return Tok::Str(&r[1..i.min(r.len())]);
        }
        if c.is_ascii_digit() {
            let mut end = 0;
            let b = r.as_bytes();
            if r.starts_with("0x") || r.starts_with("0X") {
                end = 2;
                while end < b.len() && b[end].is_ascii_hexdigit() {
                    end += 1;
                }
                self.at += end;
                return u32::from_str_radix(&r[2..end], 16).map_or(Tok::Float, Tok::Int);
            }
            while end < b.len() && b[end].is_ascii_digit() {
                end += 1;
            }
            if end < b.len() && b[end] == b'.' {
                while end < b.len() && (b[end].is_ascii_digit() || b[end] == b'.') {
                    end += 1;
                }
                self.at += end;
                return Tok::Float;
            }
            self.at += end;
            return r[..end].parse().map_or(Tok::Float, Tok::Int);
        }
        if c.is_alphanumeric() || c == '_' {
            let end = r
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .unwrap_or(r.len());
            self.at += end;
            return Tok::Ident(&r[..end]);
        }
        self.at += c.len_utf8();
        Tok::Punct(c)
    }

    /// The next token without taking it.
    pub fn peek(&mut self) -> Tok<'a> {
        let (at, line) = (self.at, self.line);
        let t = self.take();
        self.at = at;
        self.line = line;
        t
    }
}
