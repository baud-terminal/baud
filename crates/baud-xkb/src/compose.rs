//! **Compose sequences and dead keys: the table a Compose file defines and
//! the state machine a keysym at a time walks it.** A dead key types
//! nothing, and the key after it types what the two compose to: `´` then
//! `a` is `á`, and `Multi_key` `o` `c` is `©`. The file is the X11 locale's
//! Compose table (`[[source: libx11-compose-5]]`), and the reader and the
//! machine are libxkbcommon's (`[[source: xkbcommon-compose-h]]`), which
//! is the oracle: `tests/compose.rs` holds every entry and thousands of
//! fed sequences of the library's own answers over two locales' system
//! tables, and the grammar, the conflict rules, the escapes and the error
//! budget below are ported from its parser line for line
//! (`[[source: libxkbcommon-compose-parser-c]]`) so that a file reads here
//! as it reads there.
//!
//! The table is a trie whose children are kept sorted by keysym, so a fed
//! keysym is a binary search among the possible next keysyms, never a walk
//! of the table. Files come through [`Files`], so the crate reads nothing
//! itself; where a locale's table is found is the presenter's.
//!
//! Ported from libxkbcommon 1.13.2's `src/compose/parser.c` and
//! `src/compose/state.c`: Copyright 1992 by Oki Technosystems Laboratory,
//! Inc.; Copyright 1992 by Fuji Xerox Co., Ltd.; Copyright © 2013 Ran
//! Benita. MIT and HPND-sell-variant, whose texts are in
//! `LICENSES/libxkbcommon-LICENSE.txt`.

use crate::keysym::Keysym;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// `COMPOSE_MAX_LHS_LEN`: a longer sequence is skipped with a warning.
const MAX_LHS: usize = 10;
/// `COMPOSE_MAX_INCLUDE_DEPTH`: deeper, and the whole file fails.
const MAX_INCLUDE_DEPTH: usize = 5;
/// `XKB_COMPOSE_MAX_STRING_SIZE`, its terminating NUL counted.
const MAX_STRING: usize = 256;
/// More errors than this in one file and the file fails.
const MAX_ERRORS: usize = 10;
/// The scanner's buffer, its terminating NUL counted.
const BUF: usize = 1024;
/// `MAX_COMPOSE_NODES`: past it a production is dropped.
const MAX_NODES: usize = 1 << 23;

/// Why a Compose file made no table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The first bytes are not ASCII or UTF-8: a NUL in the first two, or
    /// a first byte above `0x7f` without a byte-order mark.
    Encoding,
    /// More than ten errors in one file.
    TooManyErrors,
    /// Includes nested more than five deep, a loop most likely.
    IncludeDepth,
    /// An included file could not be read.
    Include(String),
}

/// What an `include` reads and what its `%` expansions name.
pub trait Files {
    /// The bytes of the file at `path`.
    fn read(&mut self, path: &str) -> Option<Vec<u8>>;
    /// `%H`: `$HOME`.
    fn home(&self) -> Option<String>;
    /// `%L`: the locale's own Compose file.
    fn locale_file(&mut self) -> Option<String>;
    /// `%S`: the system's X locale directory.
    fn system_dir(&self) -> String;
}

/// No files: an `include` fails, as an unreadable one does.
pub struct NoFiles;

impl Files for NoFiles {
    fn read(&mut self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn home(&self) -> Option<String> {
        None
    }
    fn locale_file(&mut self) -> Option<String> {
        None
    }
    fn system_dir(&self) -> String {
        String::new()
    }
}

/// A node's index in the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(u32);

/// The table's nodes, indexed by [`NodeId`].
#[derive(Clone, Debug, Default)]
struct Nodes(Vec<Node>);

impl core::ops::Index<NodeId> for Nodes {
    type Output = Node;
    fn index(&self, id: NodeId) -> &Node {
        &self.0[usize::try_from(id.0).unwrap_or(usize::MAX)]
    }
}

impl core::ops::IndexMut<NodeId> for Nodes {
    fn index_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.0[usize::try_from(id.0).unwrap_or(usize::MAX)]
    }
}

/// What a sequence composes to: a keysym, a string, or both. A string
/// written as `""` is kept as an empty one, which the library answers
/// rather than the keysym's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composed {
    pub keysym: Keysym,
    pub text: Option<Box<str>>,
}

impl Composed {
    /// `xkb_compose_state_get_utf8()`: the string, or "if there's no string
    /// specified, but only a keysym", the keysym's text.
    pub fn utf8(&self) -> String {
        match (&self.text, self.keysym.to_char()) {
            (Some(t), _) => String::from(&**t),
            (None, Some(c)) if self.keysym != Keysym::NO_SYMBOL => String::from(c),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug)]
enum Kind {
    /// Sorted by keysym.
    Inner(Vec<NodeId>),
    Leaf(Composed),
}

#[derive(Clone, Debug)]
struct Node {
    keysym: Keysym,
    kind: Kind,
}

/// A compiled Compose table.
#[derive(Clone, Debug, Default)]
pub struct Table {
    nodes: Nodes,
    /// The first keysyms, sorted.
    root: Vec<NodeId>,
}

/// Where a production attaches: the top level, or under a node.
#[derive(Clone, Copy)]
enum Under {
    Root,
    Node(NodeId),
}

impl Table {
    /// The table a Compose file's text defines, `include`s read through
    /// `files`.
    pub fn parse(text: &[u8], files: &mut dyn Files) -> Result<Table, Error> {
        let mut t = Table::default();
        parse(&mut t, text, files, Depth(0))?;
        Ok(t)
    }

    fn children(&self, under: Under) -> &[NodeId] {
        match under {
            Under::Root => &self.root,
            Under::Node(id) => match &self.nodes[id].kind {
                Kind::Inner(c) => c,
                Kind::Leaf(_) => &[],
            },
        }
    }

    fn find(&self, list: &[NodeId], sym: Keysym) -> Result<usize, usize> {
        list.binary_search_by(|&id| self.nodes[id].keysym.cmp(&sym))
    }

    /// `add_production`: the sequence into the trie, the last definition
    /// winning. A sequence that extends one already there turns that one's
    /// end into a step ("a sequence already exists which is a prefix of
    /// this sequence; overriding"), and one that ends where others go on
    /// drops them ("this compose sequence is a prefix of another").
    fn add(&mut self, lhs: &[Keysym], result: Composed) {
        if self.nodes.0.len() + 1 + lhs.len() >= MAX_NODES {
            return;
        }
        let mut under = Under::Root;
        for (i, &sym) in lhs.iter().enumerate() {
            let id = match self.find(self.children(under), sym) {
                Ok(j) => self.children(under)[j],
                Err(j) => {
                    let id = NodeId(u32::try_from(self.nodes.0.len()).unwrap_or(u32::MAX));
                    self.nodes.0.push(Node {
                        keysym: sym,
                        kind: Kind::Inner(Vec::new()),
                    });
                    match under {
                        Under::Root => self.root.insert(j, id),
                        Under::Node(p) => {
                            if let Kind::Inner(c) = &mut self.nodes[p].kind {
                                c.insert(j, id);
                            }
                        }
                    }
                    id
                }
            };
            let node = &mut self.nodes[id];
            if i + 1 < lhs.len() {
                if let Kind::Leaf(_) = node.kind {
                    node.kind = Kind::Inner(Vec::new());
                }
                under = Under::Node(id);
            } else {
                node.kind = Kind::Leaf(result);
                return;
            }
        }
    }

    /// Every sequence and what it composes to, in the library iterator's
    /// order: keysym by keysym, ascending.
    pub fn entries(&self) -> Vec<(Vec<Keysym>, Composed)> {
        let mut out = Vec::new();
        let mut path = Vec::new();
        self.walk(&self.root, &mut path, &mut out);
        out
    }

    fn walk(
        &self,
        list: &[NodeId],
        path: &mut Vec<Keysym>,
        out: &mut Vec<(Vec<Keysym>, Composed)>,
    ) {
        for &id in list {
            let n = &self.nodes[id];
            path.push(n.keysym);
            match &n.kind {
                Kind::Inner(c) => self.walk(c, path, out),
                Kind::Leaf(r) => out.push((path.clone(), r.clone())),
            }
            path.pop();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_empty()
    }
}

/// `enum xkb_compose_feed_result`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fed {
    /// A modifier's keysym: no effect.
    Ignored,
    /// It started, advanced, ended or cancelled a sequence.
    Accepted,
}

/// `enum xkb_compose_status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Nothing,
    Composing,
    Composed,
    /// "The last sequence was cancelled due to an unmatched keysym."
    Cancelled,
}

/// Whether the position before the last feed was inside a sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Before {
    #[default]
    Outside,
    Inside,
}

/// **The state machine over a table**, `xkb_compose_state`: the node the
/// keysyms fed so far reach, and whether the one before was a step. It
/// holds no reference, so it lives beside the table it walks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Compose {
    at: Option<NodeId>,
    before: Before,
}

impl Compose {
    /// `xkb_compose_state_feed()`: from the top after a sequence ended,
    /// failed or never began, else among the steps after the last; a
    /// modifier is not fed at all, "otherwise it'd be impossible to have a
    /// sequence like `<dead_acute><A>` (needs Shift in the middle)".
    pub fn feed(&mut self, table: &Table, sym: Keysym) -> Fed {
        if sym.is_modifier() {
            return Fed::Ignored;
        }
        let list = match self.at {
            Some(id) => match &table.nodes[id].kind {
                Kind::Inner(c) => c.as_slice(),
                Kind::Leaf(_) => table.root.as_slice(),
            },
            None => table.root.as_slice(),
        };
        let next = table.find(list, sym).ok().map(|j| list[j]);
        self.before = match self.at.map(|id| &table.nodes[id].kind) {
            Some(Kind::Inner(_)) => Before::Inside,
            _ => Before::Outside,
        };
        self.at = next;
        Fed::Accepted
    }

    pub fn reset(&mut self) {
        *self = Compose::default();
    }

    pub fn status(&self, table: &Table) -> Status {
        match (self.at.map(|id| &table.nodes[id].kind), self.before) {
            (None, Before::Inside) => Status::Cancelled,
            (None, Before::Outside) => Status::Nothing,
            (Some(Kind::Inner(_)), _) => Status::Composing,
            (Some(Kind::Leaf(_)), _) => Status::Composed,
        }
    }

    /// What the sequence composed to, once it has.
    pub fn composed<'t>(&self, table: &'t Table) -> Option<&'t Composed> {
        match &table.nodes[self.at?].kind {
            Kind::Leaf(r) => Some(r),
            Kind::Inner(_) => None,
        }
    }
}

/// The scanner's tokens, `enum rules_token`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Eof,
    Eol,
    Include,
    IncludeString,
    LhsKeysym,
    Colon,
    Bang,
    Tilde,
    Str,
    Ident,
    Error,
}

/// **A byte of the file**: the scanner reads bytes, as the library's does,
/// since a comment may hold anything and a string's UTF-8 is checked only
/// once it is whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Byte(u8);

impl Byte {
    /// What `scanner_peek` answers past the end.
    const NUL: Byte = Byte(0);

    /// `is_space`: the space and `\t` through `\r`.
    fn is_space(self) -> bool {
        self.0 == b' ' || (b'\t'..=b'\r').contains(&self.0)
    }

    fn is(self, c: char) -> bool {
        u32::from(self.0) == u32::from(c)
    }

    /// Its value as a hex digit.
    fn hex(self) -> Option<Byte> {
        char::from(self.0)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
            .map(Byte)
    }

    /// Its value as an octal digit.
    fn octal(self) -> Option<Byte> {
        (b'0'..=b'7').contains(&self.0).then(|| Byte(self.0 - b'0'))
    }
}

/// A cursor over the file's bytes; a byte past the end reads as NUL, as
/// `scanner_peek` answers.
struct Scanner<'a> {
    rest: &'a [u8],
    buf: Vec<u8>,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Byte {
        self.rest.first().map_or(Byte::NUL, |&b| Byte(b))
    }

    fn eof(&self) -> bool {
        self.rest.is_empty()
    }

    fn eol(&self) -> bool {
        self.peek().is('\n')
    }

    fn next(&mut self) -> Byte {
        match self.rest.split_first() {
            Some((&b, r)) => {
                self.rest = r;
                Byte(b)
            }
            None => Byte::NUL,
        }
    }

    fn chr(&mut self, c: char) -> bool {
        match self.rest.split_first() {
            Some((&b, r)) if Byte(b).is(c) => {
                self.rest = r;
                true
            }
            _ => false,
        }
    }

    fn skip_to_eol(&mut self) {
        let n = self
            .rest
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(self.rest.len());
        self.rest = &self.rest[n..];
    }

    /// `scanner_buf_append`, whose failure the loops ignore.
    fn push(&mut self, b: Byte) {
        if self.buf.len() + 1 < BUF {
            self.buf.push(b.0);
        }
    }

    /// `scanner_buf_appends`.
    fn push_str(&mut self, s: &str) -> Result<(), ()> {
        match self.buf.len() + s.len() < BUF {
            true => {
                self.buf.extend_from_slice(s.as_bytes());
                Ok(())
            }
            false => Err(()),
        }
    }

    /// Room for the terminating NUL, which the token needs.
    fn ends(&self) -> bool {
        self.buf.len() + 1 < BUF
    }

    /// `scanner_hex`: up to two hex digits.
    fn hex(&mut self) -> Option<Byte> {
        let mut v = 0u8;
        let mut n = 0;
        while n < 2 {
            let Some(d) = self.peek().hex() else { break };
            self.next();
            v = v.wrapping_mul(16).wrapping_add(d.0);
            n += 1;
        }
        (n > 0).then_some(Byte(v))
    }

    /// `scanner_oct`: up to four octal digits, a fourth taken whole and
    /// marked invalid once the value passes `040`.
    fn oct(&mut self) -> Option<Byte> {
        let mut v = 0u8;
        let mut n = 0;
        while n < 4 {
            let Some(d) = self.peek().octal() else { break };
            self.next();
            if v >= 0o40 {
                return None;
            }
            v = v * 8 + d.0;
            n += 1;
        }
        (n > 0).then_some(Byte(v))
    }

    /// `scanner_check_supported_char_encoding()`.
    fn supported(&mut self) -> bool {
        if self.rest.starts_with(b"\xef\xbb\xbf") {
            self.rest = &self.rest[3..];
            return true;
        }
        match self.rest {
            [] | [_] => true,
            [0, ..] | [_, 0, ..] => false,
            [b, ..] => b.is_ascii(),
        }
    }

    /// `lex`.
    fn lex(&mut self) -> Tok {
        loop {
            while self.peek().is_space() {
                if self.next().is('\n') {
                    return Tok::Eol;
                }
            }
            if self.chr('#') {
                self.skip_to_eol();
                continue;
            }
            break;
        }
        if self.eof() {
            return Tok::Eof;
        }
        self.buf.clear();
        if self.chr('<') {
            while !self.peek().is('>') && !self.eol() && !self.eof() {
                let b = self.next();
                self.push(b);
            }
            if !self.chr('>') || !self.ends() {
                return Tok::Error;
            }
            return Tok::LhsKeysym;
        }
        if self.chr(':') {
            return Tok::Colon;
        }
        if self.chr('!') {
            return Tok::Bang;
        }
        if self.chr('~') {
            return Tok::Tilde;
        }
        if self.chr('"') {
            while !self.eof() && !self.eol() && !self.peek().is('"') {
                if self.chr('\\') {
                    if self.chr('\\') {
                        self.push(Byte(b'\\'));
                    } else if self.chr('"') {
                        self.push(Byte(b'"'));
                    } else if self.chr('x') || self.chr('X') {
                        // "illegal hexadecimal escape sequence": dropped.
                        if let Some(o) = self.hex().filter(|&o| o != Byte::NUL) {
                            self.push(o);
                        }
                    } else if let Some(o) = self.oct().filter(|&o| o != Byte::NUL) {
                        self.push(o);
                    }
                    // An illegal octal escape is dropped, and an unknown
                    // one leaves its character to be read as itself.
                } else {
                    let b = self.next();
                    self.push(b);
                }
            }
            if !self.chr('"') || !self.ends() || core::str::from_utf8(&self.buf).is_err() {
                return Tok::Error;
            }
            return Tok::Str;
        }
        if self.peek().0.is_ascii_alphabetic() || self.peek().is('_') {
            while self.peek().0.is_ascii_alphanumeric() || self.peek().is('_') {
                let b = self.next();
                self.push(b);
            }
            if !self.ends() {
                return Tok::Error;
            }
            return match self.buf.as_slice() {
                b"include" => Tok::Include,
                _ => Tok::Ident,
            };
        }
        // "unrecognized token": the rest of the line is discarded.
        self.skip_to_eol();
        Tok::Error
    }

    /// `lex_include_string`, with `%%`, `%H`, `%L` and `%S` expanded.
    fn lex_include(&mut self, files: &mut dyn Files) -> Tok {
        while self.peek().is_space() {
            if self.next().is('\n') {
                return Tok::Eol;
            }
        }
        self.buf.clear();
        if !self.chr('"') {
            return Tok::Error;
        }
        while !self.eof() && !self.eol() && !self.peek().is('"') {
            if self.chr('%') {
                let expanded = if self.chr('%') {
                    self.push(Byte(b'%'));
                    Ok(())
                } else if self.chr('H') {
                    match files.home() {
                        Some(h) => self.push_str(&h),
                        None => Err(()),
                    }
                } else if self.chr('L') {
                    match files.locale_file() {
                        Some(p) => self.push_str(&p),
                        None => Err(()),
                    }
                } else if self.chr('S') {
                    let d = files.system_dir();
                    self.push_str(&d)
                } else {
                    Err(())
                };
                if expanded.is_err() {
                    return Tok::Error;
                }
            } else {
                let b = self.next();
                self.push(b);
            }
        }
        if !self.chr('"') || !self.ends() {
            return Tok::Error;
        }
        Tok::IncludeString
    }
}

/// `resolve_modifier`: the names a modifier list may hold. They are read
/// and then not used, as the library does not use them.
fn is_modifier_name(name: &[u8]) -> bool {
    matches!(
        name,
        b"Shift" | b"Ctrl" | b"Alt" | b"Meta" | b"Lock" | b"Caps"
    )
}

/// The parser's states, the labels of the library's `parse`.
#[derive(Clone, Copy)]
enum St {
    Initial,
    InitialEol,
    Include,
    IncludeEol,
    Lhs,
    LhsTok,
    LhsKeysym,
    LhsKeysymTok,
    LhsModList,
    LhsModListTok,
    Rhs,
    Unexpected,
    Fault,
    Skip,
}

/// How deep in includes a file is read, the first file at none.
#[derive(Clone, Copy)]
struct Depth(usize);

/// One file's productions into the table: the library's `parse`, its
/// gotos as states.
fn parse(t: &mut Table, text: &[u8], files: &mut dyn Files, depth: Depth) -> Result<(), Error> {
    let mut s = Scanner {
        rest: text,
        buf: Vec::new(),
    };
    if !s.supported() {
        return Err(Error::Encoding);
    }
    let mut errors = 0;
    let mut st = St::Initial;
    let mut tok = Tok::Eof;
    let mut lhs: Vec<Keysym> = Vec::new();
    let mut text_rhs: Option<Box<str>> = None;
    loop {
        st = match st {
            St::Initial => {
                lhs.clear();
                text_rhs = None;
                St::InitialEol
            }
            St::InitialEol => {
                tok = s.lex();
                match tok {
                    Tok::Eol => St::InitialEol,
                    Tok::Eof => return Ok(()),
                    Tok::Include => St::Include,
                    _ => St::LhsTok,
                }
            }
            St::Include => {
                tok = s.lex_include(files);
                match tok {
                    Tok::IncludeString => St::IncludeEol,
                    _ => St::Unexpected,
                }
            }
            St::IncludeEol => {
                let path = String::from_utf8_lossy(&s.buf).into_owned();
                tok = s.lex();
                match tok {
                    Tok::Eol => {
                        if depth.0 >= MAX_INCLUDE_DEPTH {
                            return Err(Error::IncludeDepth);
                        }
                        let Some(bytes) = files.read(&path) else {
                            return Err(Error::Include(path));
                        };
                        parse(t, &bytes, files, Depth(depth.0 + 1))?;
                        St::Initial
                    }
                    _ => St::Unexpected,
                }
            }
            St::Lhs => {
                tok = s.lex();
                St::LhsTok
            }
            St::LhsTok => match tok {
                // "expected at least one keysym on left-hand side".
                Tok::Colon if lhs.is_empty() => St::Skip,
                Tok::Colon => St::Rhs,
                Tok::Ident if s.buf == b"None" => St::LhsKeysym,
                Tok::Ident | Tok::Tilde => St::LhsModListTok,
                Tok::Bang => St::LhsModList,
                _ => St::LhsKeysymTok,
            },
            St::LhsKeysym => {
                tok = s.lex();
                St::LhsKeysymTok
            }
            St::LhsKeysymTok => match tok {
                Tok::LhsKeysym => {
                    let name = core::str::from_utf8(&s.buf).ok();
                    match name.and_then(Keysym::from_xkb_name) {
                        None => St::Fault,
                        // "too many keysyms on left-hand side".
                        Some(_) if lhs.len() + 1 > MAX_LHS => St::Skip,
                        Some(k) => {
                            lhs.push(k);
                            St::Lhs
                        }
                    }
                }
                _ => St::Unexpected,
            },
            St::LhsModList => {
                tok = s.lex();
                St::LhsModListTok
            }
            St::LhsModListTok => {
                if tok != Tok::Tilde && tok != Tok::Ident {
                    St::LhsKeysymTok
                } else {
                    if tok == Tok::Tilde {
                        tok = s.lex();
                    }
                    match tok {
                        Tok::Ident if is_modifier_name(&s.buf) => St::LhsModList,
                        Tok::Ident => St::Fault,
                        _ => St::Unexpected,
                    }
                }
            }
            St::Rhs => {
                tok = s.lex();
                match tok {
                    // "at most one string", and one that fits.
                    Tok::Str if text_rhs.is_some() || s.buf.len() + 1 > MAX_STRING => St::Skip,
                    Tok::Str => {
                        text_rhs = Some(String::from_utf8_lossy(&s.buf).into());
                        St::Rhs
                    }
                    Tok::Ident => {
                        let name = core::str::from_utf8(&s.buf).ok();
                        match name.and_then(Keysym::from_xkb_name) {
                            None => St::Fault,
                            Some(k) => {
                                t.add(
                                    &lhs,
                                    Composed {
                                        keysym: k,
                                        text: text_rhs.take(),
                                    },
                                );
                                St::Initial
                            }
                        }
                    }
                    // "at least one of string or keysym".
                    Tok::Eol if text_rhs.is_none() => St::Skip,
                    Tok::Eol => {
                        t.add(
                            &lhs,
                            Composed {
                                keysym: Keysym::NO_SYMBOL,
                                text: text_rhs.take(),
                            },
                        );
                        St::Initial
                    }
                    _ => St::Unexpected,
                }
            }
            St::Unexpected => St::Fault,
            St::Fault => {
                errors += 1;
                if errors > MAX_ERRORS {
                    return Err(Error::TooManyErrors);
                }
                St::Skip
            }
            St::Skip => {
                while tok != Tok::Eol && tok != Tok::Eof {
                    tok = s.lex();
                }
                St::Initial
            }
        };
    }
}

/// **The two tables a locale's Compose file is found through**,
/// `locale.alias` and `compose.dir` in the X locale directory, read as
/// `resolve_name` reads them (`[[source: libxkbcommon-compose-paths-c]]`):
/// a line is a name, an optional colon and a value, `#` a comment, and the
/// first matching line wins.
pub mod paths {
    use alloc::string::String;

    /// Which side of a line is matched.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Match {
        /// `locale.alias`: an alias on the left names a locale.
        Left,
        /// `compose.dir`: a locale on the right names a file.
        Right,
    }

    /// The other side of the first line whose `side` is `name`.
    pub fn resolve(text: &[u8], side: Match, name: &str) -> Option<String> {
        let is_space = |b: &u8| super::Byte(*b).is_space();
        let mut s = text;
        while !s.is_empty() {
            while s.first().is_some_and(is_space) {
                s = &s[1..];
            }
            if s.first() == Some(&b'#') {
                let n = s.iter().position(|&b| b == b'\n').unwrap_or(s.len());
                s = &s[n..];
                continue;
            }
            let n = s
                .iter()
                .position(|b| is_space(b) || *b == b':')
                .unwrap_or(s.len());
            let (left, mut r) = s.split_at(n);
            if r.first() == Some(&b':') {
                r = &r[1..];
            }
            while r.first().is_some_and(is_space) {
                r = &r[1..];
            }
            let n = r.iter().position(is_space).unwrap_or(r.len());
            let (right, mut rest) = r.split_at(n);
            let n = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
            rest = &rest[n..];
            let (key, value) = match side {
                Match::Left => (left, right),
                Match::Right => (right, left),
            };
            if key == name.as_bytes() {
                return Some(String::from_utf8_lossy(value).into_owned());
            }
            s = rest;
        }
        None
    }

    /// The locale's Compose file named in `compose.dir`, relative to the
    /// X locale directory or absolute: `C` is read as `en_US.UTF-8`, "99%
    /// of the time a C locale is really just a misconfiguration for
    /// UTF-8", and a locale the file does not register falls back to
    /// `en_US.UTF-8`, "what most locales use anyway" -- but only one the
    /// system has, which `known` answers as `newlocale` does for the
    /// library, "so that we can catch missing system locales and typos".
    pub fn locale_file(
        compose_dir: &[u8],
        locale: &str,
        known: &dyn Fn(&str) -> bool,
    ) -> Option<String> {
        const FALLBACK: &str = "en_US.UTF-8";
        let locale = match locale {
            "C" => FALLBACK,
            l => l,
        };
        resolve(compose_dir, Match::Right, locale)
            .or_else(|| known(locale).then(|| resolve(compose_dir, Match::Right, FALLBACK))?)
    }
}
