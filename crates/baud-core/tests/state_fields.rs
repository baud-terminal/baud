#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles and silently stops running;
// the compiler's own dead-code analysis is the check that catches it.
//! Every field of every struct in `baud-core` is *read* somewhere, not
//! merely written.
//!
//! A field that is written and never read is state the terminal pays for and
//! never acts on: a mode recorded and never consulted, a counter bumped and
//! never reported, a buffer filled and never drawn. It is the same failure as
//! a sequence parsed and dropped, and the compiler does not see it: on this
//! toolchain `rustc`'s `dead_code` lint names a field that nothing touches,
//! but counts an assignment to it -- `self.f = v`, `self.f += 1`, `p.f = v`
//! -- as a use, so a field that is written and never read compiles clean,
//! and a `pub` field of a `pub` struct is never named at all.
//!
//! `baud-core2`, the core before this one, had this gate for its `Storage`
//! alone, and its first run there found `tmpl`: a megabyte filled with 2048
//! cells on every rendition-table miss and read by nothing, whose removal
//! measured +27% on btop in that core.
//!
//! **What counts as a read.** Every use of a field other than as the target
//! of an assignment: passing it, borrowing it (`&mut` included), comparing
//! it, matching on it, calling a method on it, indexing to read from it, and
//! binding it in a destructuring pattern, and writing through it when it is
//! a reference (`*x.f = v`). `x.f = v`, every compound assignment
//! (`x.f += 1`), and an assignment into it (`x.f[i] = v`, `x.f.g = v`) are
//! writes, and so is the field's initialiser in a struct expression.
//!
//! **What it cannot see.** It reads source, not types. A read is matched to
//! its struct when the expression's type is spelled nearby: `self` inside
//! the struct's `impl`, a parameter declared with the struct's type, a field
//! declared with it (so `self.s.vp` is `Storage`'s `vp`), or a pattern that
//! names it; a `let` bound from one of those, spelled with a type, or bound
//! to a struct expression or `S::new(..)`; and a comparison of two values
//! whose struct derives `PartialEq`, `PartialOrd` or `Ord`, which reads
//! every field of it. Any other `x.f` -- a closure's parameter, an element,
//! a method's result -- reads every field called `f`, so two structs that
//! share a field name, or a core field and a field of another crate's
//! type, can clear each other through it. Two structs of one name in two
//! files are checked as one, so a read or a derived comparison of either
//! clears the other's field of the same name. When this was written, 57 of the
//! 699 fields in 82 structs had no read the parser could tie to their own
//! struct: 48 `pub` fields of `pub` structs, and nine others, each read in
//! the code.
//!
//! A read inside `#[cfg(test)]` code does not count, because the program
//! itself never makes it; a read behind a cargo feature does. A field
//! declared `pub` on a `pub` struct may be read by any crate in the
//! workspace, and its tests count, because an embedder reading it is what
//! it is for. Tuple and unit structs have no field names and are not
//! checked.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// Fields no code can read, each with the reason reading it is impossible.
///
/// Empty on purpose. A field belongs here only when reading it from code is
/// impossible rather than merely awkward; a field nothing reads is removed,
/// or the code that should act on it is written.
const NEVER_READABLE: [(&str, &str, &str); 0] = [];

// ---------------------------------------------------------------- lexing

/// A file's tokens, each with the line it starts on.
type Toks = [(Tok, usize)];

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Punct(&'static str),
    /// A number, a tuple index among them; its value never matters here.
    Num,
    /// A string, byte string or character literal: opaque, so a field named
    /// in a message or a doc example reads nothing.
    Lit,
    Lifetime,
}

const PUNCTS: [&str; 51] = [
    "<<=", ">>=", "...", "..=", "..", "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "+=",
    "-=", "*=", "/=", "%=", "^=", "&=", "|=", "<<", ">>", ";", ",", ".", "(", ")", "[", "]", "{",
    "}", "<", ">", "=", "!", "~", "?", ":", "#", "$", "@", "+", "-", "*", "/", "%", "^", "&", "|",
];

const ASSIGN: [&str; 11] = [
    "=", "+=", "-=", "*=", "/=", "%=", "^=", "&=", "|=", "<<=", ">>=",
];

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// Tokens with the line each starts on; comments are dropped and literals
/// made opaque.
fn lex(src: &str) -> Vec<(Tok, usize)> {
    let s: Vec<char> = src.chars().collect();
    let at = |i: usize| s.get(i).copied().unwrap_or('\0');
    let mut out: Vec<(Tok, usize)> = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    // Consume `s[from..to]`, counting the newlines it spans.
    let advance = |from: usize, to: usize, line: &mut usize| {
        *line += s[from..to.min(s.len())]
            .iter()
            .filter(|&&c| c == '\n')
            .count();
        to
    };
    while i < s.len() {
        let c = s[i];
        if c.is_whitespace() {
            i = advance(i, i + 1, &mut line);
            continue;
        }
        if c == '/' && at(i + 1) == '/' {
            while i < s.len() && s[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && at(i + 1) == '*' {
            let start = i;
            let mut depth = 0usize;
            loop {
                if i >= s.len() {
                    break;
                }
                if s[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if s[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            advance(start, i, &mut line);
            continue;
        }
        let tok_line = line;
        // Raw strings, byte and C strings, and byte characters, before
        // identifiers swallow their prefix.
        let mut p = i;
        if (c == 'b' || c == 'c') && at(i + 1) == 'r' && (at(i + 2) == '"' || at(i + 2) == '#') {
            p = i + 1;
        }
        if s[p] == 'r'
            && (at(p + 1) == '"' || (at(p + 1) == '#' && (at(p + 2) == '#' || at(p + 2) == '"')))
        {
            let mut j = p + 1;
            let mut hashes = 0usize;
            while at(j) == '#' {
                hashes += 1;
                j += 1;
            }
            j += 1; // the opening quote
            loop {
                if j >= s.len() {
                    break;
                }
                if s[j] == '"' && (0..hashes).all(|k| at(j + 1 + k) == '#') {
                    j += 1 + hashes;
                    break;
                }
                j += 1;
            }
            i = advance(i, j, &mut line);
            out.push((Tok::Lit, tok_line));
            continue;
        }
        if ((c == 'b' || c == 'c') && at(i + 1) == '"') || (c == 'b' && at(i + 1) == '\'') {
            p = i + 1;
        }
        if s[p] == '"' {
            let mut j = p + 1;
            while j < s.len() && s[j] != '"' {
                if s[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            i = advance(i, j + 1, &mut line);
            out.push((Tok::Lit, tok_line));
            continue;
        }
        if s[p] == '\'' {
            if at(p + 1) == '\\' {
                let mut j = p + 3;
                while j < s.len() && s[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
                out.push((Tok::Lit, tok_line));
                continue;
            }
            if at(p + 2) == '\'' {
                i = p + 3;
                out.push((Tok::Lit, tok_line));
                continue;
            }
            let mut j = p + 1;
            while j < s.len() && is_ident(s[j]) {
                j += 1;
            }
            i = j;
            out.push((Tok::Lifetime, tok_line));
            continue;
        }
        if c == 'r' && at(i + 1) == '#' && is_ident_start(at(i + 2)) {
            let mut j = i + 2;
            while j < s.len() && is_ident(s[j]) {
                j += 1;
            }
            out.push((Tok::Ident(s[i + 2..j].iter().collect()), tok_line));
            i = j;
            continue;
        }
        if is_ident_start(c) {
            let mut j = i;
            while j < s.len() && is_ident(s[j]) {
                j += 1;
            }
            out.push((Tok::Ident(s[i..j].iter().collect()), tok_line));
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            // After a `.` a number is a tuple index, so `x.0.1` is two of
            // them rather than a float.
            let index = matches!(out.last(), Some((Tok::Punct("."), _)));
            let mut j = i;
            loop {
                while j < s.len() && is_ident(s[j]) {
                    j += 1;
                }
                if !index && at(j) == '.' && at(j + 1).is_ascii_digit() {
                    j += 1;
                    continue;
                }
                break;
            }
            out.push((Tok::Num, tok_line));
            i = j;
            continue;
        }
        let rest: String = s[i..(i + 3).min(s.len())].iter().collect();
        let Some(p) = PUNCTS.iter().find(|p| rest.starts_with(**p)) else {
            // A character Rust gives no meaning outside a literal; skip it.
            i += 1;
            continue;
        };
        out.push((Tok::Punct(p), tok_line));
        i += p.chars().count();
    }
    out
}

fn is_p(t: Option<&(Tok, usize)>, p: &str) -> bool {
    matches!(t, Some((Tok::Punct(q), _)) if *q == p)
}

fn ident(t: Option<&(Tok, usize)>) -> Option<&str> {
    match t {
        Some((Tok::Ident(n), _)) => Some(n),
        _ => None,
    }
}

/// The index just past the group that opens at `i` (`(`, `[` or `{`).
fn close(t: &Toks, i: usize) -> usize {
    let mut depth = 0usize;
    let mut j = i;
    while j < t.len() {
        match &t[j].0 {
            Tok::Punct("(" | "[" | "{") => depth += 1,
            Tok::Punct(")" | "]" | "}") => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    t.len()
}

/// The index just past the `<...>` that opens at `i`.
fn close_angle(t: &Toks, i: usize) -> usize {
    let mut depth = 0isize;
    let mut j = i;
    while j < t.len() {
        match &t[j].0 {
            Tok::Punct("<") => depth += 1,
            Tok::Punct("<<") => depth += 2,
            Tok::Punct(">") => depth -= 1,
            Tok::Punct(">>") => depth -= 2,
            Tok::Punct("(" | "[" | "{") => {
                j = close(t, j);
                continue;
            }
            _ => {}
        }
        j += 1;
        if depth <= 0 {
            return j;
        }
    }
    t.len()
}

/// Is `#[cfg(test)]` at `i`?
fn cfg_test_at(t: &Toks, i: usize) -> bool {
    is_p(t.get(i), "#")
        && is_p(t.get(i + 1), "[")
        && ident(t.get(i + 2)) == Some("cfg")
        && is_p(t.get(i + 3), "(")
        && ident(t.get(i + 4)) == Some("test")
        && is_p(t.get(i + 5), ")")
        && is_p(t.get(i + 6), "]")
}

/// The tokens with every `#[cfg(test)]` item removed: an item ends at a `;`
/// outside any group or with the group its first `{` opens.
fn strip_cfg_test(t: Vec<(Tok, usize)>) -> Vec<(Tok, usize)> {
    let mut out = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if !cfg_test_at(&t, i) {
            out.push(t[i].clone());
            i += 1;
            continue;
        }
        let mut j = i + 7;
        while is_p(t.get(j), "#") && is_p(t.get(j + 1), "[") {
            j = close(&t, j + 1);
        }
        while j < t.len() {
            match &t[j].0 {
                Tok::Punct(";") => {
                    j += 1;
                    break;
                }
                Tok::Punct("{") => {
                    j = close(&t, j);
                    break;
                }
                Tok::Punct("(" | "[") => j = close(&t, j),
                _ => j += 1,
            }
        }
        i = j;
    }
    out
}

// ------------------------------------------------------------- the model

/// Whether a struct derives `PartialEq`, `PartialOrd` or `Ord`, whose code
/// reads every field when two values are compared.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Compared {
    #[default]
    No,
    Derived,
}

/// Who may read a field: the core alone, or any crate, as a `pub` field of
/// a `pub` struct may be.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reach {
    Crate,
    Anyone,
}

/// The line a declaration is on, for the failure message.
#[derive(Clone, Copy, Debug)]
struct SrcLine(usize);

#[derive(Debug)]
struct Decl {
    file: String,
    name: String,
    compared: Compared,
    fields: Vec<Field>,
}

#[derive(Debug)]
struct Field {
    name: String,
    line: SrcLine,
    reach: Reach,
    /// The type's name, past references and lifetimes: `Storage` for
    /// `Storage<'b>` and for `&'a mut Storage<'b>`.
    ty: Option<String>,
    /// Every name in the type: `Option` and `Line` for `Option<Line>`.
    tys: Vec<String>,
}

/// Every identifier in the type `t[k..end]`.
fn type_names(t: &Toks, k: usize, end: usize) -> Vec<String> {
    t[k..end]
        .iter()
        .filter_map(|(x, _)| match x {
            Tok::Ident(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

/// Does the struct declared at `k` (its `struct` keyword) derive a
/// comparison?
fn derives_comparison(t: &Toks, mut k: usize) -> Compared {
    if k >= 1 && ident(t.get(k - 1)) == Some("pub") {
        k -= 1;
    } else if k >= 1 && is_p(t.get(k - 1), ")") {
        k = open_of(t, k - 1);
        if k >= 1 && ident(t.get(k - 1)) == Some("pub") {
            k -= 1;
        }
    }
    let mut found = Compared::No;
    while k >= 2 && is_p(t.get(k - 1), "]") {
        let open = open_of(t, k - 1);
        if open == 0 || !is_p(t.get(open - 1), "#") {
            break;
        }
        let derives = |x: &Tok| matches!(x, Tok::Ident(n) if n == "PartialEq" || n == "PartialOrd" || n == "Ord");
        if ident(t.get(open + 1)) == Some("derive") && t[open..k].iter().any(|(x, _)| derives(x)) {
            found = Compared::Derived;
        }
        k = open - 1;
    }
    found
}

/// The name a type is spelled with, past `&`, `mut`, `dyn`, lifetimes and
/// its path: `crate::status::StatusBuf<'b>` is `StatusBuf`. A tuple, an
/// array or a slice has none.
fn type_head(t: &Toks, mut k: usize, end: usize) -> Option<String> {
    while k < end {
        match &t[k].0 {
            Tok::Punct("&" | "&&") | Tok::Lifetime => k += 1,
            Tok::Ident(n) if n == "mut" || n == "dyn" => k += 1,
            _ => break,
        }
    }
    let mut last = None;
    while let Some(n) = ident(t.get(k)).filter(|_| k < end) {
        last = Some(n.to_string());
        if !is_p(t.get(k + 1), "::") {
            break;
        }
        k += 2;
    }
    last
}

/// `pub struct` is anyone's; `pub(crate) struct` and a bare `struct` are
/// the core's.
fn visibility_before(t: &Toks, k: usize) -> Reach {
    match k > 0 && ident(t.get(k - 1)) == Some("pub") {
        true => Reach::Anyone,
        false => Reach::Crate,
    }
}

/// Every struct with named fields declared in `t`.
fn structs(file: &str, t: &Toks) -> Vec<Decl> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if ident(t.get(i)) != Some("struct") {
            i += 1;
            continue;
        }
        let Some(name) = ident(t.get(i + 1)) else {
            i += 1;
            continue;
        };
        let reach = visibility_before(t, i);
        let compared = derives_comparison(t, i);
        let mut j = i + 2;
        if is_p(t.get(j), "<") {
            j = close_angle(t, j);
        }
        // A `where` clause, up to the body.
        while j < t.len() && !matches!(t[j].0, Tok::Punct("{" | "(" | ";")) {
            if is_p(t.get(j), "<") {
                j = close_angle(t, j);
            } else {
                j += 1;
            }
        }
        if !is_p(t.get(j), "{") {
            i = j;
            continue;
        }
        let end = close(t, j) - 1;
        let mut fields = Vec::new();
        let mut k = j + 1;
        while k < end {
            while is_p(t.get(k), "#") && is_p(t.get(k + 1), "[") {
                k = close(t, k + 1);
            }
            let mut field_reach = Reach::Crate;
            if ident(t.get(k)) == Some("pub") {
                k += 1;
                if is_p(t.get(k), "(") {
                    k = close(t, k);
                } else {
                    field_reach = reach;
                }
            }
            let (Some(fname), true) = (ident(t.get(k)), is_p(t.get(k + 1), ":")) else {
                panic!(
                    "{file}:{}: a field of `{name}` the parser cannot read",
                    t[k].1
                );
            };
            let (fname, line) = (fname.to_string(), SrcLine(t[k].1));
            // The type, to the comma that ends it.
            k += 2;
            let ty_at = k;
            let mut angle = 0isize;
            while k < end {
                match &t[k].0 {
                    Tok::Punct(",") if angle <= 0 => break,
                    Tok::Punct("<") => angle += 1,
                    Tok::Punct("<<") => angle += 2,
                    Tok::Punct(">") => angle -= 1,
                    Tok::Punct(">>") => angle -= 2,
                    Tok::Punct("(" | "[" | "{") => {
                        k = close(t, k);
                        continue;
                    }
                    _ => {}
                }
                k += 1;
            }
            fields.push(Field {
                name: fname,
                line,
                reach: field_reach,
                ty: type_head(t, ty_at, k),
                tys: type_names(t, ty_at, k),
            });
            k += 1;
        }
        out.push(Decl {
            file: file.to_string(),
            name: name.to_string(),
            compared,
            fields,
        });
        i = end + 1;
    }
    out
}

/// For each token, the type of the `impl` block it sits in, if any.
fn impl_owners(t: &Toks) -> Vec<Option<String>> {
    let mut owner = vec![None; t.len()];
    let mut i = 0;
    while i < t.len() {
        // An `impl` item, not `impl Trait` in a signature.
        let item = i == 0
            || matches!(t[i - 1].0, Tok::Punct("}" | ";" | "]" | "{"))
            || ident(t.get(i - 1)) == Some("unsafe");
        if ident(t.get(i)) != Some("impl") || !item {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if is_p(t.get(j), "<") {
            j = close_angle(t, j);
        }
        let mut name: Option<String> = None;
        while j < t.len() && !is_p(t.get(j), "{") && !is_p(t.get(j), ";") {
            match &t[j].0 {
                Tok::Ident(n) if n == "where" => {
                    while j < t.len() && !is_p(t.get(j), "{") {
                        j += 1;
                    }
                    break;
                }
                Tok::Ident(n) if n == "for" => name = None,
                Tok::Ident(n) if n != "mut" && n != "dyn" => name = Some(n.clone()),
                Tok::Punct("<") => {
                    j = close_angle(t, j);
                    continue;
                }
                Tok::Punct("(" | "[") => {
                    j = close(t, j);
                    continue;
                }
                _ => {}
            }
            j += 1;
        }
        if !is_p(t.get(j), "{") {
            i = j;
            continue;
        }
        let end = close(t, j);
        // Inner impls are rare and visited later, so they overwrite.
        for o in &mut owner[j..end] {
            o.clone_from(&name);
        }
        i = j + 1;
    }
    owner
}

/// What a struct's name says about its values: whether comparing two reads
/// every field, and each field's type, as its name and every name in it.
#[derive(Default)]
struct Info {
    compared: Compared,
    fields: BTreeMap<String, (Option<String>, Vec<String>)>,
}

/// Each struct by name. Two structs of one name, in two files, are one
/// entry with the fields of both.
type Decls = BTreeMap<String, Info>;

/// A parameter's type: its name, and every name in it.
type Param = (Option<String>, Vec<String>);

/// Each `fn`'s parameters with the names of their types, and for each
/// token the innermost `fn` whose body holds it.
fn fn_scopes(t: &Toks) -> (Vec<Option<usize>>, Vec<BTreeMap<String, Param>>) {
    let mut at = vec![None; t.len()];
    let mut scopes = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if ident(t.get(i)) != Some("fn") || ident(t.get(i + 1)).is_none() {
            i += 1;
            continue;
        }
        let mut j = i + 2;
        if is_p(t.get(j), "<") {
            j = close_angle(t, j);
        }
        if !is_p(t.get(j), "(") {
            i = j;
            continue;
        }
        let params_end = close(t, j) - 1;
        let mut params = BTreeMap::new();
        let mut k = j + 1;
        while k < params_end {
            // One parameter: a pattern, a colon, a type, to its comma.
            let start = k;
            let mut colon = None;
            let mut angle = 0isize;
            while k < params_end {
                match &t[k].0 {
                    Tok::Punct(",") if angle <= 0 => break,
                    Tok::Punct(":") if colon.is_none() => colon = Some(k),
                    Tok::Punct("<") => angle += 1,
                    Tok::Punct("<<") => angle += 2,
                    Tok::Punct(">") => angle -= 1,
                    Tok::Punct(">>") => angle -= 2,
                    Tok::Punct("(" | "[" | "{") => {
                        k = close(t, k);
                        continue;
                    }
                    _ => {}
                }
                k += 1;
            }
            if let Some(c) = colon {
                let mut p = start;
                if ident(t.get(p)) == Some("mut") {
                    p += 1;
                }
                if let (Some(name), true) = (ident(t.get(p)), p + 1 == c) {
                    params.insert(
                        name.to_string(),
                        (type_head(t, c + 1, k), type_names(t, c + 1, k)),
                    );
                }
            }
            k += 1;
        }
        // The body, past the return type and any `where` clause; a
        // declaration without one ends at `;`.
        let mut b = params_end + 1;
        while b < t.len() && !matches!(t[b].0, Tok::Punct("{" | ";")) {
            match &t[b].0 {
                Tok::Punct("<") => b = close_angle(t, b),
                Tok::Punct("(" | "[") => b = close(t, b),
                _ => b += 1,
            }
        }
        if is_p(t.get(b), "{") {
            let end = close(t, b);
            for a in &mut at[b..end] {
                *a = Some(scopes.len());
            }
            scopes.push(params);
        }
        i = params_end + 1;
    }
    (at, scopes)
}

/// The index of the group's opening bracket, for the `)`, `]` or `}` at `k`.
fn open_of(t: &Toks, k: usize) -> usize {
    let mut depth = 0usize;
    let mut j = k;
    loop {
        match &t[j].0 {
            Tok::Punct(")" | "]" | "}") => depth += 1,
            Tok::Punct("(" | "[" | "{") => {
                depth -= 1;
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
        if j == 0 {
            return 0;
        }
        j -= 1;
    }
}

/// Is the place expression whose last field access is the `.` at `i`
/// dereferenced, as in `*x.f = v`? Then the assignment writes through the
/// field, which reads it.
fn dereferenced(t: &Toks, i: usize) -> bool {
    let mut k = i;
    while k >= 1 {
        match &t[k - 1].0 {
            Tok::Ident(_) | Tok::Num if k >= 2 && is_p(t.get(k - 2), ".") => k -= 2,
            Tok::Ident(_) | Tok::Num => {
                k -= 1;
                break;
            }
            Tok::Punct(")" | "]") => {
                k = open_of(t, k - 1);
                if !matches!(t.get(k.wrapping_sub(1)), Some((Tok::Ident(_), _))) {
                    break;
                }
            }
            _ => break,
        }
    }
    k >= 1 && is_p(t.get(k - 1), "*")
}

/// Field reads found in one body of source.
#[derive(Default)]
struct Reads {
    /// Read through `x.f` with `x`'s type unknown: credits every field
    /// called `f`.
    by_name: BTreeSet<String>,
    /// Read through an expression whose type is known to be `S`: `self` in
    /// `impl S`, a parameter `p: &S`, a field of type `S`, or a pattern
    /// naming `S`.
    by_owner: BTreeSet<(String, String)>,
}

impl Reads {
    fn credit(&mut self, owner: Option<&str>, field: &str, decls: &Decls) {
        match owner {
            // `self.f` where the impl's type declares `f`: that field only.
            Some(o) if decls.get(o).is_some_and(|d| d.fields.contains_key(field)) => {
                self.by_owner.insert((o.to_string(), field.to_string()));
            }
            _ => {
                self.by_name.insert(field.to_string());
            }
        }
    }

    fn has(&self, owner: &str, field: &str) -> bool {
        self.by_name.contains(field)
            || self
                .by_owner
                .contains(&(owner.to_string(), field.to_string()))
    }
}

/// Does the place expression that continues at `j` end in an assignment?
fn assigned(t: &Toks, mut j: usize) -> bool {
    loop {
        if is_p(t.get(j), "[") {
            j = close(t, j);
        } else if is_p(t.get(j), ".")
            && matches!(t.get(j + 1), Some((Tok::Ident(_) | Tok::Num, _)))
            && !is_p(t.get(j + 2), "(")
            && !is_p(t.get(j + 2), "::")
        {
            j += 2;
        } else {
            break;
        }
    }
    matches!(t.get(j), Some((Tok::Punct(p), _)) if ASSIGN.contains(p))
}

/// Is the struct expression or pattern named at `name_at`, whose braces end
/// just before `end`, a pattern?
fn is_pattern(t: &Toks, name_at: usize, end: usize) -> bool {
    // `S { a, .. }`: a rest with no base expression after it.
    if is_p(t.get(end - 2), "..") {
        return true;
    }
    if matches!(t.get(end), Some((Tok::Punct("=>" | "=" | "|" | ":"), _))) {
        return true;
    }
    if ident(t.get(end)) == Some("in") {
        return true;
    }
    // `let S { .. }`, `let path::S { .. }`, `let &S { .. }`.
    let mut k = name_at;
    while k >= 2 && is_p(t.get(k - 1), "::") && ident(t.get(k - 2)).is_some() {
        k -= 2;
    }
    while k >= 1 && matches!(t.get(k - 1), Some((Tok::Punct("&" | "("), _))) {
        k -= 1;
    }
    k >= 1 && ident(t.get(k - 1)) == Some("let")
}

const NOT_STRUCT_EXPR: [&str; 8] = [
    "struct", "impl", "for", "enum", "trait", "union", "mod", "fn",
];

/// What one file's source says about the type of each expression, as far
/// as the parser can tell.
struct Typed {
    /// For each token, the type of the `impl` it sits in.
    owners: Vec<Option<String>>,
    /// For each token, the innermost `fn` body it sits in.
    scope_at: Vec<Option<usize>>,
    /// Each `fn`'s parameters and the `let` bindings typed so far.
    scopes: Vec<BTreeMap<String, Param>>,
    /// A `let` whose value is a field access waits for that access's type:
    /// the binding's scope and name, and whether `Some(..)` unwraps it.
    pending: BTreeMap<usize, (usize, String, Unwrap)>,
    /// The struct each field access ending at a token evaluates to, when
    /// known, so `self.s.vp` is `Storage`'s `vp`.
    ty_end: Vec<Option<String>>,
    /// The struct and field each such access names.
    field_at: Vec<Option<(String, String)>>,
}

/// A token's position in its file's tokens.
#[derive(Clone, Copy)]
struct Pos(usize);

/// Whether a binding takes `Option<T>`'s `T` rather than the option.
#[derive(Clone, Copy, PartialEq)]
enum Unwrap {
    No,
    Yes,
}

impl Typed {
    fn new(t: &Toks) -> Typed {
        let (scope_at, scopes) = fn_scopes(t);
        Typed {
            owners: impl_owners(t),
            scope_at,
            scopes,
            pending: BTreeMap::new(),
            ty_end: vec![None; t.len()],
            field_at: vec![None; t.len()],
        }
    }

    /// The type a variable `v` at token `i` was declared with.
    fn var(&self, i: usize, v: &str) -> Option<&Param> {
        self.scope_at[i].and_then(|s| self.scopes[s].get(v))
    }

    /// The struct the expression before the `.` at `i` evaluates to.
    fn base(&self, t: &Toks, i: usize) -> Option<String> {
        match t.get(i.checked_sub(1)?) {
            Some((Tok::Ident(_), _)) if i >= 2 && is_p(t.get(i - 2), ".") => {
                self.ty_end[i - 1].clone()
            }
            Some((Tok::Ident(v), _)) if v == "self" => self.owners[i].clone(),
            Some((Tok::Ident(v), _)) if !is_p(t.get(i.wrapping_sub(2)), "::") => {
                self.var(i, v).and_then(|p| p.0.clone())
            }
            _ => None,
        }
    }

    /// Record the type of the `let` binding at `i` when it is spelled or
    /// follows from its value: `let p: &mut Paint = ..`,
    /// `let p = Paint { .. }`, `let p = Paint::new(..)`,
    /// `let p = &mut self.x`, `let Some(p) = self.x.take()`.
    fn bind(&mut self, t: &Toks, at: Pos, decls: &Decls) {
        let Pos(i) = at;
        let Some(s) = self.scope_at[i] else { return };
        let mut k = i + 1;
        let mut unwrap = Unwrap::No;
        if ident(t.get(k)) == Some("Some") && is_p(t.get(k + 1), "(") {
            unwrap = Unwrap::Yes;
            k += 2;
        }
        while matches!(ident(t.get(k)), Some("mut" | "ref")) {
            k += 1;
        }
        let Some(name) = ident(t.get(k)).map(str::to_string) else {
            return;
        };
        k += 1;
        if unwrap == Unwrap::Yes {
            if !is_p(t.get(k), ")") {
                return;
            }
            k += 1;
        }
        if is_p(t.get(k), ":") {
            let mut e = k + 1;
            while e < t.len() && !matches!(t[e].0, Tok::Punct("=" | ";")) {
                match &t[e].0 {
                    Tok::Punct("<") => e = close_angle(t, e),
                    Tok::Punct("(" | "[") => e = close(t, e),
                    _ => e += 1,
                }
            }
            self.scopes[s].insert(name, (type_head(t, k + 1, e), type_names(t, k + 1, e)));
            return;
        }
        if !is_p(t.get(k), "=") {
            return;
        }
        let mut v = k + 1;
        // `Paint { .. }` and `crate::paint::Paint::new(..)`.
        let mut path = v;
        while ident(t.get(path)).is_some() && is_p(t.get(path + 1), "::") {
            path += 2;
        }
        let ctor = |n: usize| {
            ident(t.get(n))
                .filter(|x| decls.contains_key(*x) && unwrap == Unwrap::No)
                .map(str::to_string)
        };
        let built = match ctor(path) {
            Some(ty) if is_p(t.get(path + 1), "{") => Some(ty),
            _ if path >= v + 2 && ident(t.get(path)).is_some() && is_p(t.get(path + 1), "(") => {
                ctor(path - 2)
            }
            _ => None,
        };
        if let Some(ty) = built {
            self.scopes[s].insert(name, (Some(ty.clone()), vec![ty]));
            return;
        }
        while matches!(t.get(v), Some((Tok::Punct("&" | "*"), _))) || ident(t.get(v)) == Some("mut")
        {
            v += 1;
        }
        // The path, then the `Option` methods that keep its value: `self.x`,
        // `self.x.as_mut()`, `self.x.take().filter(..)`, `self.x.unwrap()`.
        let mut l = v;
        while is_p(t.get(l + 1), ".")
            && ident(t.get(l + 2)).is_some()
            && !matches!(t.get(l + 3), Some((Tok::Punct("(" | "::"), _)))
        {
            l += 2;
        }
        if ident(t.get(l)).is_none() {
            return;
        }
        let mut tail = l + 1;
        while is_p(t.get(tail), ".") && is_p(t.get(tail + 2), "(") {
            match ident(t.get(tail + 1)) {
                Some("as_mut" | "as_ref" | "take" | "copied" | "cloned" | "filter") => {}
                Some("unwrap" | "expect") if unwrap == Unwrap::No => unwrap = Unwrap::Yes,
                _ => return,
            }
            tail = close(t, tail + 2);
        }
        // Only a value that is the whole path: `let x = a.b + 1` is not `b`.
        if !matches!(t.get(tail), Some((Tok::Punct(";" | "{"), _)))
            && ident(t.get(tail)) != Some("else")
        {
            return;
        }
        let ty = match t.get(l) {
            Some((Tok::Ident(_), _)) if l >= 1 && is_p(t.get(l - 1), ".") => {
                self.pending.insert(l, (s, name, unwrap));
                return;
            }
            Some((Tok::Ident(x), _)) if x == "self" => {
                self.owners[i].clone().map(|o| (Some(o.clone()), vec![o]))
            }
            Some((Tok::Ident(x), _)) => self.scopes[s].get(x).cloned(),
            _ => None,
        };
        if let Some(ty) = ty {
            self.scopes[s].insert(name, unwrap_option(ty, unwrap));
        }
    }

    /// The type names of the operand whose last token is `l`, in the code
    /// at token `at`, when it is a path `a`, `self` or `a.b.c` whose type is
    /// known.
    fn operand(&self, t: &Toks, l: usize, at: usize, decls: &Decls) -> Vec<String> {
        match t.get(l) {
            Some((Tok::Ident(_), _)) if l >= 1 && is_p(t.get(l - 1), ".") => {
                match &self.field_at[l] {
                    Some((o, f)) => decls[o].fields[f].1.clone(),
                    None => Vec::new(),
                }
            }
            Some((Tok::Ident(v), _)) if v == "self" => self.owners[at].iter().cloned().collect(),
            Some((Tok::Ident(v), _)) if !is_p(t.get(l.wrapping_sub(1)), "::") => {
                self.var(at, v).map(|p| p.1.clone()).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }
}

fn scan(t: &Toks, decls: &Decls, reads: &mut Reads) {
    let mut ty = Typed::new(t);
    for i in 0..t.len() {
        if ident(t.get(i)) == Some("let") {
            ty.bind(t, Pos(i), decls);
            continue;
        }
        // `x.f`, not a method call `x.f(` or `x.f::<T>(`.
        if is_p(t.get(i), ".") {
            let Some(f) = ident(t.get(i + 1)) else {
                continue;
            };
            if is_p(t.get(i + 2), "(") || is_p(t.get(i + 2), "::") || is_p(t.get(i + 2), "!") {
                continue;
            }
            let owner = ty
                .base(t, i)
                .filter(|b| decls.get(b).is_some_and(|d| d.fields.contains_key(f)));
            if let Some(o) = &owner {
                ty.ty_end[i + 1] = decls[o].fields[f].0.clone();
                ty.field_at[i + 1] = Some((o.clone(), f.to_string()));
                if let Some((s, name, unwrap)) = ty.pending.remove(&(i + 1)) {
                    ty.scopes[s].insert(name, unwrap_option(decls[o].fields[f].clone(), unwrap));
                }
            }
            if assigned(t, i + 2) && !dereferenced(t, i) {
                continue;
            }
            reads.credit(owner.as_deref(), f, decls);
            continue;
        }
        // `S { a, b: x, .. }` as a pattern binds what it names.
        let Some(name) = ident(t.get(i)) else {
            continue;
        };
        if !is_p(t.get(i + 1), "{") {
            continue;
        }
        let struct_name = if name == "Self" {
            match ty.owners[i].as_deref() {
                Some(o) => o.to_string(),
                None => continue,
            }
        } else if decls.contains_key(name) {
            name.to_string()
        } else {
            continue;
        };
        if i >= 1
            && matches!(t.get(i - 1), Some((Tok::Ident(k), _)) if NOT_STRUCT_EXPR.contains(&k.as_str()))
            || is_p(t.get(i.wrapping_sub(1)), "->")
        {
            continue;
        }
        let end = close(t, i + 1);
        if !is_pattern(t, i, end) {
            continue;
        }
        credit_pattern(t, i + 2..end - 1, &struct_name, decls, reads);
    }
    // A comparison whose operand's type is known reads what the comparison
    // reads.
    for i in 0..t.len() {
        let operands: Vec<Option<usize>> = match &t[i].0 {
            Tok::Punct(p) if COMPARE_OPS.contains(p) && i >= 1 => {
                vec![Some(i - 1), path_from(t, i + 1)]
            }
            Tok::Ident(m)
                if i >= 2
                    && is_p(t.get(i - 1), ".")
                    && is_p(t.get(i + 1), "(")
                    && COMPARE_METHODS.contains(&m.as_str()) =>
            {
                vec![Some(i - 2), path_from(t, i + 2)]
            }
            _ => continue,
        };
        for l in operands.into_iter().flatten() {
            let names = ty.operand(t, l, i, decls);
            credit_compared(&names, decls, reads, &mut BTreeSet::new());
        }
    }
}

/// `Option<T>`'s `T` when unwrapped; any other type as it is.
fn unwrap_option((head, tys): Param, unwrap: Unwrap) -> Param {
    match (unwrap, tys.first().map(String::as_str)) {
        (Unwrap::Yes, Some("Option")) => (tys.get(1).cloned(), tys[1..].to_vec()),
        _ => (head, tys),
    }
}

/// The fields a pattern `S { .. }` binds, from the tokens inside its braces.
fn credit_pattern(
    t: &Toks,
    inside: Range<usize>,
    struct_name: &str,
    decls: &Decls,
    reads: &mut Reads,
) {
    let mut k = inside.start;
    while k < inside.end {
        while matches!(ident(t.get(k)), Some("ref" | "mut")) {
            k += 1;
        }
        if let Some(f) = ident(t.get(k)) {
            let ignored = is_p(t.get(k + 1), ":")
                && ident(t.get(k + 2)) == Some("_")
                && matches!(t.get(k + 3), Some((Tok::Punct("," | "}"), _)));
            if !ignored {
                reads.credit(Some(struct_name), f, decls);
            }
        }
        // On to the next field: past this one's pattern, to its comma.
        while k < inside.end && !is_p(t.get(k), ",") {
            if matches!(t.get(k), Some((Tok::Punct("(" | "[" | "{"), _))) {
                k = close(t, k);
            } else {
                k += 1;
            }
        }
        k += 1;
    }
}

/// The last token of the path that starts at `k`, past `&`, `*` and a
/// `Some(..)` around it, if the operand is only that path.
fn path_from(t: &Toks, mut k: usize) -> Option<usize> {
    while matches!(t.get(k), Some((Tok::Punct("&" | "*"), _))) || ident(t.get(k)) == Some("mut") {
        k += 1;
    }
    if ident(t.get(k)) == Some("Some") && is_p(t.get(k + 1), "(") {
        let l = path_from(t, k + 2)?;
        return is_p(t.get(l + 1), ")").then_some(l);
    }
    ident(t.get(k))?;
    while is_p(t.get(k + 1), ".")
        && ident(t.get(k + 2)).is_some()
        && !matches!(t.get(k + 3), Some((Tok::Punct("(" | "::"), _)))
    {
        k += 2;
    }
    // A call, an index or a method on it is another value.
    if matches!(t.get(k + 1), Some((Tok::Punct("(" | "[" | "." | "::"), _))) {
        return None;
    }
    Some(k)
}

/// Comparing two values of a struct that derives the comparison reads every
/// field, and the fields of the structs inside it.
fn credit_compared(
    names: &[String],
    decls: &Decls,
    reads: &mut Reads,
    seen: &mut BTreeSet<String>,
) {
    for n in names {
        let Some(d) = decls.get(n) else { continue };
        if d.compared == Compared::No || !seen.insert(n.clone()) {
            continue;
        }
        for (f, (_, tys)) in &d.fields {
            reads.by_owner.insert((n.clone(), f.clone()));
            credit_compared(tys, decls, reads, seen);
        }
    }
}

const COMPARE_OPS: [&str; 6] = ["==", "!=", "<", ">", "<=", ">="];
const COMPARE_METHODS: [&str; 11] = [
    "eq",
    "ne",
    "cmp",
    "partial_cmp",
    "lt",
    "le",
    "gt",
    "ge",
    "max",
    "min",
    "clamp",
];

/// The fields of `core` that nothing reads, as `Struct.field` with where
/// each is declared. `outside` is every other source a `pub` field may be
/// read from.
fn unread(
    core: &[(String, String)],
    outside: &[(String, String)],
) -> (Vec<Decl>, BTreeMap<String, String>) {
    let lexed: Vec<(String, Vec<(Tok, usize)>)> = core
        .iter()
        .map(|(f, s)| (f.clone(), strip_cfg_test(lex(s))))
        .collect();
    let mut all = Vec::new();
    for (f, t) in &lexed {
        all.extend(structs(f, t));
    }
    let mut decls = Decls::new();
    for d in &all {
        let info = decls.entry(d.name.clone()).or_default();
        if d.compared == Compared::Derived {
            info.compared = Compared::Derived;
        }
        info.fields.extend(
            d.fields
                .iter()
                .map(|f| (f.name.clone(), (f.ty.clone(), f.tys.clone()))),
        );
    }
    let mut reads = Reads::default();
    for (_, t) in &lexed {
        scan(t, &decls, &mut reads);
    }
    let mut out = BTreeMap::new();
    let mut public_unread = Vec::new();
    for d in &all {
        for f in &d.fields {
            if reads.has(&d.name, &f.name) {
                continue;
            }
            if f.reach == Reach::Anyone {
                public_unread.push((d, f));
            } else {
                out.insert(
                    format!("{}.{}", d.name, f.name),
                    format!("{}:{}", d.file, f.line.0),
                );
            }
        }
    }
    // A `pub` field's readers may be anywhere, with its tests and their
    // `cfg(test)` code: the core's own, typed as before, and every other
    // crate's, where no type is resolved and every read is by name. Only
    // the files that spell a name are lexed.
    let mut outer = Reads::default();
    let names: BTreeSet<&str> = public_unread.iter().map(|(_, f)| f.name.as_str()).collect();
    let spells = |s: &str| names.iter().any(|n| s.contains(n));
    for (_, s) in core.iter().filter(|(_, s)| spells(s)) {
        scan(&lex(s), &decls, &mut outer);
    }
    let none = Decls::new();
    for (_, s) in outside.iter().filter(|(_, s)| spells(s)) {
        scan(&lex(s), &none, &mut outer);
    }
    for (d, f) in public_unread {
        if !outer.has(&d.name, &f.name) {
            out.insert(
                format!("{}.{}", d.name, f.name),
                format!("{}:{}", d.file, f.line.0),
            );
        }
    }
    (all, out)
}

// ------------------------------------------------------------ the sources

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_files(&p, out);
            }
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

fn read(dirs: &[PathBuf], root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for d in dirs {
        rust_files(d, &mut files);
    }
    files
        .iter()
        .map(|f| {
            let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
            (rel, fs::read_to_string(f).expect("a source file reads"))
        })
        .collect()
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// **A field is only state if something reads it.**
#[test]
fn every_state_field_is_read_somewhere() {
    let root = workspace();
    let core = read(&[root.join("crates/baud-core/src")], &root);
    let mut others: Vec<PathBuf> = fs::read_dir(root.join("crates"))
        .expect("the workspace lists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("baud-") && n != "baud-core")
        })
        .collect();
    others.push(root.join("crates/baud-core/tests"));
    others.push(root.join("crates/baud-core/benches"));
    others.push(root.join("crates/baud-core/examples"));
    others.push(root.join("tools/bench-others"));
    others.sort();
    let outside = read(&others, &root);

    let (decls, mut unread) = unread(&core, &outside);
    let fields: usize = decls.iter().map(|d| d.fields.len()).sum();
    // A parser that finds nothing passes vacuously.
    assert!(
        decls.len() >= 60 && fields >= 300,
        "parsed {} structs and {fields} fields: the parser is wrong",
        decls.len()
    );
    for name in ["Term", "Storage", "Cold", "Rest", "Owned"] {
        assert!(
            decls.iter().any(|d| d.name == name),
            "`{name}` was not parsed"
        );
    }
    for (s, f, why) in NEVER_READABLE {
        assert!(
            decls
                .iter()
                .any(|d| d.name == s && d.fields.iter().any(|x| x.name == f)),
            "`{s}.{f}` is exempt and no longer exists"
        );
        assert!(!why.is_empty(), "`{s}.{f}` is exempt without a reason");
        assert!(
            unread.remove(&format!("{s}.{f}")).is_some(),
            "`{s}.{f}` is exempt and is read"
        );
    }
    assert!(
        unread.is_empty(),
        "struct fields written but never read -- remove each, or write the code that acts on it:\n{}",
        unread
            .iter()
            .map(|(f, at)| format!("  {f}  ({at})"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// --------------------------------------------------------- the controls

/// The fields `src` leaves unread, as one file of a core with no other
/// crate beside it.
fn unread_in(src: &str) -> BTreeSet<String> {
    unread(&[("lib.rs".to_string(), src.to_string())], &[])
        .1
        .into_keys()
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

const DECL: &str = "pub(crate) struct S { pub(crate) f: u32, g: [u32; 4], h: Inner }\n\
                    pub(crate) struct Inner { pub(crate) x: u32 }\n";

/// Every shape the definition calls a read clears the field.
#[test]
fn each_kind_of_read_counts() {
    for body in [
        "fn r(s: &S) -> u32 { s.f }",
        "fn r(s: &S) { take(s.f); }",
        "fn r(s: &mut S) { bump(&mut s.f); }",
        "fn r(s: &S) -> bool { s.f == 3 }",
        "fn r(s: &S) { match s.f { 0 => {} _ => {} } }",
        "fn r(s: &S) { if let 0 = s.f {} }",
        "fn r(s: &S) -> u32 { s.f.count_ones() }",
        "fn r(s: &S) -> u32 { s.f.wrapping_add::<>(1) }",
        "fn r(s: &mut S) { s.g[0] = s.f; }",
        "fn r(s: &S) -> u32 { let x = [0u32; 2]; x[s.f] }",
        "fn r(s: &S) { let S { f, .. } = s; take(*f); }",
        "fn r(s: &S) { match s { S { f: 1, .. } => {} _ => {} } }",
        "impl S { fn r(&self) -> u32 { self.f } }",
        "impl S { fn r(&self) { let Self { f, .. } = self; take(*f); } }",
        "fn r(s: &S) { take(format_args!(\"{}\", s.f)); }",
    ] {
        let src = format!(
            "{DECL}fn w(s: &mut S) {{ s.f = 1; }}\n{body}\nfn other(s: &S) {{ take(s.g[0]); take(s.h.x); }}"
        );
        assert_eq!(unread_in(&src), set(&[]), "{body}");
    }
}

/// Every shape the definition calls a write leaves it unread.
#[test]
fn each_kind_of_write_does_not() {
    for body in [
        "fn w(s: &mut S) { s.f = 1; }",
        "fn w(s: &mut S) { s.f += 1; s.f -= 1; s.f *= 1; s.f /= 1; s.f %= 1; }",
        "fn w(s: &mut S) { s.f ^= 1; s.f &= 1; s.f |= 1; s.f <<= 1; s.f >>= 1; }",
        "impl S { fn w(&mut self) { self.f = 2; } }",
        "fn w() -> S { S { f: 1, g: [0; 4], h: Inner { x: 0 } } }",
        // A comment, a string and a doc example are not code.
        "// s.f\n/* take(s.f) */\n/// take(s.f)\nfn w() { take(\"s.f\"); take(r#\"s.f\"#); }",
        "#[cfg(test)]\nmod tests { fn t(s: &super::S) -> u32 { s.f } }",
        "#[cfg(test)]\nfn t(s: &S) -> u32 { s.f }",
        // A pattern that ignores it does not read it.
        "fn w(s: &S) { let S { f: _, .. } = s; }",
    ] {
        let src = format!("{DECL}{body}\nfn other(s: &S) {{ take(s.g[0]); take(s.h.x); }}");
        assert_eq!(unread_in(&src), set(&["S.f"]), "{body}");
    }
    // Writes into a field are writes to it: an element, a sub-field, a
    // dereference.
    let src = format!(
        "{DECL}fn w(s: &mut S) {{ s.g[1] = 0; s.h.x = 1; s.g[s.f] += 1; }}\nfn r(s: &S) {{ take(s.f); }}"
    );
    assert_eq!(unread_in(&src), set(&["S.g", "Inner.x", "S.h"]));
}

/// A read whose type is spelled -- `self` in an `impl`, a parameter, a
/// field -- reads that struct's field and not its namesake's.
#[test]
fn a_read_of_a_known_type_reads_that_struct() {
    let decl = "pub(crate) struct A { pub(crate) n: u32 }\n\
                pub(crate) struct B { pub(crate) n: u32 }\n\
                pub(crate) struct W<'a> { pub(crate) a: &'a mut A, pub(crate) b: crate::B }\n\
                fn uses(w: &W) { take(&w.a); take(&w.b); }\n";
    for body in [
        "impl A { fn r(&self) -> u32 { self.n } }\nimpl B { fn w(&mut self) { self.n = 1; } }",
        "fn r(a: &A) -> u32 { a.n }\nfn w(mut b: B) { b.n = 1; }",
        "fn r(w: &W) -> u32 { w.a.n }\nfn w(w: &mut W) { w.b.n = 1; }",
        "fn r(w: &W) -> u32 { let a = &w.a; a.n }\nfn w(b: &mut B) { b.n = 1; }",
        "fn r() -> u32 { let a = A { n: 0 }; a.n }\nfn w(b: &mut B) { b.n = 1; }",
        "fn r() -> u32 { let a = crate::A::new(); a.n }\nfn w(b: &mut B) { b.n = 1; }",
        "fn r(x: u32) -> u32 { let a: &A = get(x); a.n }\nfn w(b: &mut B) { b.n = 1; }",
        "fn r(w: &W) -> u32 { let a = &w.a; let c = a; c.n }\nfn w(b: &mut B) { b.n = 1; }",
    ] {
        assert_eq!(unread_in(&format!("{decl}{body}")), set(&["B.n"]), "{body}");
    }
    // An expression of unknown type reads both.
    assert_eq!(
        unread_in(&format!("{decl}fn r() -> u32 {{ make().n }}")),
        set(&[])
    );
}

/// Writing through a reference reads the reference.
#[test]
fn writing_through_a_field_reads_it() {
    let decl = "pub(crate) struct D<'a> { pub(crate) n: &'a mut u32, pub(crate) m: [u32; 2] }\n";
    for body in [
        "fn w(d: &mut D) { *d.n += 1; take(d.m); }",
        "impl D<'_> { fn w(&mut self) { *self.n = 1; take(self.m); } }",
    ] {
        assert_eq!(unread_in(&format!("{decl}{body}")), set(&[]), "{body}");
    }
    assert_eq!(
        unread_in(&format!(
            "{decl}fn w(d: &mut D) {{ take(&d.n); d.m[0] = 1; }}"
        )),
        set(&["D.m"])
    );
}

/// A `pub` field on a `pub` struct may be read by another crate; a
/// `pub(crate)` one may not.
#[test]
fn a_public_field_may_be_read_outside() {
    let core = "pub struct P { pub open: u32, pub(crate) shut: u32 }\n\
                fn w(p: &mut P) { p.open = 1; p.shut = 1; }";
    let outside = "fn r(p: &P) -> u32 { p.open + p.shut }";
    let (_, unread) = unread(
        &[("lib.rs".to_string(), core.to_string())],
        &[("other.rs".to_string(), outside.to_string())],
    );
    assert_eq!(
        unread.into_keys().collect::<BTreeSet<_>>(),
        set(&["P.shut"])
    );
}

/// Comparing two values whose struct derives the comparison reads every
/// field of it, and of the structs inside it.
#[test]
fn a_derived_comparison_reads_every_field() {
    let decl = "#[derive(Clone, Copy, PartialEq, Eq, Debug)]\n\
                pub(crate) struct L { pub(crate) e: u32 }\n\
                #[derive(PartialEq)]\n\
                pub(crate) struct H { pub(crate) at: Option<L> }\n\
                fn w(h: &mut H, l: L) { h.at = Some(l); }\n\
                fn s(h: &H) { take(&h.at); }\n";
    for body in [
        "fn r(h: &H, l: L) -> bool { h.at == Some(l) }",
        "fn r(a: L, b: L) -> bool { a != b }",
        "fn r(h: &H) -> bool { let Some(a) = h.at else { return false }; a.eq(&L { e: 0 }) }",
        "fn r(a: &H, b: &H) -> bool { *a == *b }",
    ] {
        assert_eq!(unread_in(&format!("{decl}{body}")), set(&[]), "{body}");
    }
    // Never compared, the derive reads nothing.
    assert_eq!(unread_in(decl), set(&["L.e"]));
    // Compared without a derived comparison, it reads nothing either.
    let hand = decl.replace("Copy, PartialEq, Eq,", "Copy,");
    assert_eq!(
        unread_in(&format!("{hand}fn r(a: L, b: L) -> bool {{ a != b }}")),
        set(&["L.e"])
    );
}
