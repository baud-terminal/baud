//! **What a test's code reaches, read from the source, and whether any of
//! it names AppKit**: the proof, by test, that a path opens no window.
//!
//! Every NSApplication call baud makes goes through a main-thread marker,
//! so a path whose code names no marker, NSApplication, NSWindow and
//! `baud_present_macos` cannot start an application, a window or a Dock
//! icon. What a path reaches is computed, never listed by hand: from its
//! start, every `crate::` and `super::` path, `use` trees spread, and every
//! `m::item` written through a module a `use` named, to the module file it
//! names and on through that file's code. A module given [`Follow::FreeFns`]
//! or [`Follow::AllFns`] is followed fn by fn instead: only the fns reached
//! and the fns they call by name. A *sink* module is never read: the items
//! reached in it are recorded, for the caller to hold to a list. Another
//! module's unit tests (a `mod ...tests {` block), and every item
//! `#[cfg(test)]` marks, are not on any path and are not followed. A small
//! lexer blanks comments and counts only braces that are code, so a brace
//! in a string ends no block early.
//!
//! Used by baud-app's tests and, through `#[path]`, by the demo's: each
//! uses its part.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The four names, in halves so this file does not match itself.
pub const FORBIDDEN: [&str; 4] = [
    concat!("MainThread", "Marker"),
    concat!("NSApp", "lication"),
    concat!("NSWin", "dow"),
    concat!("baud_present", "_macos"),
];

/// **The link worker's question whether an app opens a scheme**, allowed
/// wherever a walk reaches the worker: NSWorkspace's
/// `URLForApplicationToOpenURL`, a LaunchServices lookup made off the main
/// thread in its own autorelease pool, which starts no AppKit application,
/// window or Dock icon and opens nothing.
pub const LINK_WORKER: (&[&str], &str, &str) = (
    &["probe"],
    concat!("baud_present", "_macos::link::has_handler(address)"),
    "the link worker's question whether an app opens a scheme: NSWorkspace's \
     URLForApplicationToOpenURL, a LaunchServices lookup made off the main thread in \
     its own autorelease pool, which starts no AppKit application, window or Dock icon",
);

/// What one reached part is: a module's file, or one fn of a module
/// followed fn by fn.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Part {
    File(Vec<String>),
    Fn(Vec<String>, String),
    /// One fn of a module followed fn by fn, with the span from the first
    /// `from` through the first `to` after it taken out: a start that is
    /// one branch of a fn, its other branch walked by nothing.
    Cut(Vec<String>, String, String, String),
}

/// How a module's code is followed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Follow {
    /// Its free fns, those at the start of a line, called by bare name:
    /// app.rs, whose methods are the window's.
    FreeFns,
    /// Every fn it defines, methods too, called by name in any form: a
    /// file whose methods are on the path.
    AllFns,
}

/// **A walk to make**: the source root its module paths resolve under,
/// where it starts, the modules followed fn by fn, and the sinks.
pub struct Walk<'a> {
    pub src: &'a Path,
    pub start: Vec<Part>,
    pub by_fn: &'a [(&'a [&'a str], Follow)],
    pub sinks: &'a [&'a [&'a str]],
}

/// **What a walk reached**: each part and its code, and each sink item,
/// with the part that reached it.
pub struct Reached {
    pub parts: BTreeMap<Part, String>,
    pub sink_items: BTreeSet<(Vec<String>, String, Part)>,
}

/// What a byte of Rust source is: code, inside a string or character
/// literal, or inside a comment.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Byte {
    Code,
    Literal,
    Comment,
}

/// Each byte of `t` classed, a lexer's worth: line and nested block
/// comments; strings, byte strings and raw strings with their hashes;
/// character literals told from lifetimes.
fn classes(t: &[u8]) -> Vec<Byte> {
    let mut k = vec![Byte::Code; t.len()];
    let word = |i: usize| i > 0 && (t[i - 1].is_ascii_alphanumeric() || t[i - 1] == b'_');
    let mut i = 0;
    while i < t.len() {
        let start = i;
        let kind = if t[i..].starts_with(b"//") {
            while i < t.len() && t[i] != b'\n' {
                i += 1;
            }
            Byte::Comment
        } else if t[i..].starts_with(b"/*") {
            let mut depth = 0usize;
            while i < t.len() {
                if t[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if t[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            Byte::Comment
        } else if (t[i] == b'r' || t[i..].starts_with(b"br")) && !word(i) && {
            let r = i + usize::from(t[i] == b'b') + 1;
            let h = t[r..].iter().take_while(|&&b| b == b'#').count();
            t.get(r + h) == Some(&b'"')
        } {
            let r = i + usize::from(t[i] == b'b') + 1;
            let h = t[r..].iter().take_while(|&&b| b == b'#').count();
            let mut close = vec![b'"'];
            close.extend(std::iter::repeat_n(b'#', h));
            i = r + h + 1;
            while i < t.len() && !t[i..].starts_with(&close) {
                i += 1;
            }
            i = (i + close.len()).min(t.len());
            Byte::Literal
        } else if t[i] == b'"' {
            i += 1;
            while i < t.len() && t[i] != b'"' {
                i += if t[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(t.len());
            Byte::Literal
        } else if t[i] == b'\'' {
            let len = |b: u8| match b {
                0xf0.. => 4,
                0xe0.. => 3,
                0xc0.. => 2,
                _ => 1,
            };
            if t.get(i + 1) == Some(&b'\\') {
                i += 3;
                while i < t.len() && t[i] != b'\'' {
                    i += 1;
                }
                i = (i + 1).min(t.len());
                Byte::Literal
            } else if t
                .get(i + 1)
                .is_some_and(|&c| t.get(i + 1 + len(c)) == Some(&b'\''))
            {
                let c = t[i + 1];
                i += 2 + len(c);
                Byte::Literal
            } else {
                i += 1;
                Byte::Code
            }
        } else {
            i += 1;
            Byte::Code
        };
        for b in &mut k[start..i.min(t.len())] {
            *b = kind;
        }
    }
    k
}

/// A file's code: every comment blanked to spaces, newlines kept, and
/// literals as written, since a string naming a class names it.
pub fn code(text: &str) -> String {
    let k = classes(text.as_bytes());
    let bytes: Vec<u8> = text
        .bytes()
        .zip(k)
        .map(|(b, c)| match (c, b) {
            (Byte::Comment, b'\n') | (Byte::Code | Byte::Literal, _) => b,
            (Byte::Comment, _) => b' ',
        })
        .collect();
    // A whole comment's bytes are blanked, so the text stays UTF-8.
    String::from_utf8(bytes).expect("UTF-8")
}

/// A byte offset into a file's text, or a count of bytes read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Offset(usize);

/// A file's code with every comment *and* every literal blanked: what
/// a call is, never what a string says.
pub fn bare_code(text: &str) -> String {
    let k = classes(text.as_bytes());
    let bytes: Vec<u8> = text
        .bytes()
        .zip(k)
        .map(|(b, c)| match (c, b) {
            (Byte::Code, _) | (_, b'\n') => b,
            (Byte::Comment | Byte::Literal, _) => b' ',
        })
        .collect();
    // Whole comments and literals are blanked, so the text stays UTF-8.
    String::from_utf8(bytes).expect("UTF-8")
}

/// The index just past the `}` matching the `{` at `open`, counting
/// only braces that are code.
fn block_end(text: &str, Offset(open): Offset) -> Offset {
    let k = classes(text.as_bytes());
    let mut depth = 0usize;
    for (i, b) in text.bytes().enumerate().skip(open) {
        if k[i] != Byte::Code {
            continue;
        }
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Offset(i + 1);
                }
            }
            _ => {}
        }
    }
    Offset(text.len())
}

/// The leading identifier of `s`.
fn ident(s: &str) -> &str {
    let n = s
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map_or(s.len(), |(i, _)| i);
    &s[..n]
}

/// `code` with every `mod NAME {` block whose name ends in `tests` cut, and
/// every item with a body that `#[cfg(test)]` marks: a test's helper is on
/// no path the app takes.
pub fn without_tests(code: &str) -> String {
    let mut out = String::new();
    let mut lines = code.lines();
    let mut rest_from = 0;
    let mut at = 0;
    let mut marked = false;
    for line in lines.by_ref() {
        let t = line.trim_start();
        let attribute = t.starts_with("#[") || t.starts_with("///") || t.is_empty();
        let t = t.strip_prefix("pub(crate) ").unwrap_or(t);
        let head = t.strip_prefix("mod ").map(ident);
        let is_tests = head.is_some_and(|n| n.ends_with("tests")) && t.trim_end().ends_with('{');
        // The item a `#[cfg(test)]` marks, when it has a body: its first
        // `{` comes before any `;`.
        let body = match (
            marked && !attribute,
            code[at..].find('{'),
            code[at..].find(';'),
        ) {
            (true, Some(b), Some(s)) if b < s => Some(at + b),
            (true, Some(b), None) => Some(at + b),
            _ => None,
        };
        if t.trim_end() == "#[cfg(test)]" {
            marked = true;
        } else if !attribute {
            marked = false;
        }
        if at >= rest_from {
            let open = match (is_tests, body) {
                (true, _) => Some(at + line.find('{').expect("a block")),
                (false, Some(b)) => Some(b),
                (false, None) => None,
            };
            if let Some(open) = open {
                out.push_str(&code[rest_from..at]);
                rest_from = block_end(code, Offset(open)).0;
            }
        }
        at += line.len() + 1;
    }
    if rest_from < code.len() {
        out.push_str(&code[rest_from..]);
    }
    out
}

/// Each path a `use` tree names, `{a, b::{c}}` spread; the bytes read.
fn tree(s: &str, base: &mut Vec<String>, out: &mut Vec<Vec<String>>) -> Offset {
    let t = s.trim_start();
    let skipped = s.len() - t.len();
    if let Some(mut rest) = t.strip_prefix('{') {
        let mut used = skipped + 1;
        loop {
            let r = rest.trim_start();
            used += rest.len() - r.len();
            rest = r;
            if rest.starts_with('}') || rest.is_empty() {
                return Offset(used + usize::from(!rest.is_empty()));
            }
            let n = tree(rest, base, out).0;
            used += n.max(1);
            rest = &rest[n.max(1).min(rest.len())..];
            let r = rest.trim_start();
            used += rest.len() - r.len();
            rest = r;
            if let Some(r) = rest.strip_prefix(',') {
                used += 1;
                rest = r;
            }
        }
    }
    let id = ident(t);
    if id.is_empty() {
        return Offset(skipped);
    }
    base.push(id.to_string());
    let used = match t[id.len()..].strip_prefix("::") {
        Some(r) => skipped + id.len() + 2 + tree(r, base, out).0,
        None => {
            out.push(base.clone());
            skipped + id.len()
        }
    };
    base.pop();
    Offset(used)
}

/// Every `crate::...` and `super::...` path in `code`: whether it is
/// `super`'s, and its segments.
fn paths(code: &str) -> Vec<(bool, Vec<String>)> {
    let mut out = Vec::new();
    for (prefix, is_super) in [("crate::", false), ("super::", true)] {
        let mut from = 0;
        while let Some(i) = code[from..].find(prefix) {
            let at = from + i;
            from = at + prefix.len();
            if code[..at]
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let mut found = Vec::new();
            let mut base = Vec::new();
            if code[from..].trim_start().starts_with('{') {
                tree(&code[from..], &mut base, &mut found);
            } else {
                // A path, not a tree: its segments up to the first that
                // is not a module's name.
                let mut seg = Vec::new();
                let mut rest = &code[from..];
                loop {
                    let id = ident(rest);
                    if id.is_empty() {
                        break;
                    }
                    seg.push(id.to_string());
                    match rest[id.len()..].strip_prefix("::") {
                        Some(r) if !r.starts_with('{') => rest = r,
                        Some(r) => {
                            let mut b = seg.clone();
                            tree(r, &mut b, &mut found);
                            break;
                        }
                        None => break,
                    }
                }
                found.push(seg);
            }
            out.extend(found.into_iter().map(|p| (is_super, p)));
        }
    }
    out
}

/// The file a module path names: segments in lower case only, so a
/// case-blind disk does not take `Config` for `config`.
fn file_of(src: &Path, path: &[String]) -> Option<PathBuf> {
    let lower = |s: &String| {
        s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    if path.is_empty() || !path.iter().all(lower) {
        return None;
    }
    let rel: PathBuf = path.iter().collect();
    let flat = src.join(&rel).with_extension("rs");
    if flat.is_file() {
        return Some(flat);
    }
    let dir = src.join(&rel).join("mod.rs");
    dir.is_file().then_some(dir)
}

/// A module's fns by name, each name's code.
type Fns = BTreeMap<String, String>;

/// The modules a file names in a `use`, by the name it uses them under.
type Aliases = BTreeMap<String, Vec<String>>;

/// Each fn `code` defines under [`Follow`]: its name and its code. Every
/// fn of that name is kept, joined, since a call by name may mean any.
fn fns(code: &str, follow: Follow) -> Fns {
    let mut out: Fns = BTreeMap::new();
    let mut at = 0;
    for line in code.lines() {
        let start = at;
        at += line.len() + 1;
        let t = match follow {
            Follow::FreeFns => line,
            Follow::AllFns => line.trim_start(),
        };
        let t = ["pub(crate) ", "pub(super) ", "pub "]
            .iter()
            .find_map(|p| t.strip_prefix(p))
            .unwrap_or(t);
        let t = t
            .strip_prefix("const ")
            .or_else(|| t.strip_prefix("unsafe "))
            .unwrap_or(t);
        let Some(rest) = t.strip_prefix("fn ") else {
            continue;
        };
        let name = ident(rest);
        if name.is_empty() || !matches!(rest[name.len()..].chars().next(), Some('(' | '<')) {
            continue;
        }
        let Some(open) = code[start..].find('{').map(|o| start + o) else {
            continue;
        };
        // A declaration with no body (a trait's, or `extern`) ends at `;`.
        if code[start..open].contains(';') {
            continue;
        }
        let body = &code[start..block_end(code, Offset(open)).0];
        out.entry(name.to_string()).or_default().push_str(body);
    }
    out
}

/// The modules `code` names in a `use` of its own crate, by the name it
/// uses them under: `use crate::m;` and `use crate::{m, n::o}`.
fn aliases(src: &Path, code: &str, sinks: &[&[&str]]) -> Aliases {
    let mut out = BTreeMap::new();
    for (is_super, p) in paths(code) {
        if is_super || p.is_empty() {
            continue;
        }
        let is_sink = sinks
            .iter()
            .any(|s| s.iter().copied().eq(p.iter().map(String::as_str)));
        if is_sink || file_of(src, &p).is_some() {
            out.insert(p.last().cloned().unwrap_or_default(), p);
        }
    }
    out
}

/// Every `m::item` in `code` for an alias `m`: the module and the item.
fn through_aliases(code: &str, aliases: &Aliases) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for (alias, module) in aliases {
        let pat = format!("{alias}::");
        for (at, _) in code.match_indices(&pat) {
            let before = code[..at].chars().last();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':') {
                continue;
            }
            let item = ident(&code[at + pat.len()..]);
            let mut p = module.clone();
            if !item.is_empty() {
                p.push(item.to_string());
            }
            out.push(p);
        }
    }
    out
}

/// **The closure of a walk**: every part reached and its code.
pub fn reach(walk: &Walk<'_>) -> Reached {
    let follow_of = |p: &[String]| {
        walk.by_fn
            .iter()
            .find(|(m, _)| m.iter().copied().eq(p.iter().map(String::as_str)))
            .map(|(_, f)| *f)
    };
    let is_sink = |p: &[String]| {
        walk.sinks
            .iter()
            .any(|s| s.iter().copied().eq(p.iter().map(String::as_str)))
    };
    // The fn-by-fn modules, read once: their fns and their aliases.
    let mut by_fn: BTreeMap<Vec<String>, (Fns, Aliases)> = BTreeMap::new();
    for (m, follow) in walk.by_fn {
        let p: Vec<String> = m.iter().map(|s| s.to_string()).collect();
        let Some(f) = file_of(walk.src, &p) else {
            continue;
        };
        let text = without_tests(&code(&std::fs::read_to_string(&f).expect("a module")));
        let a = aliases(walk.src, &text, walk.sinks);
        by_fn.insert(p, (fns(&text, *follow), a));
    }
    let mut parts: BTreeMap<Part, String> = BTreeMap::new();
    let mut sink_items = BTreeSet::new();
    let mut todo = walk.start.clone();
    while let Some(part) = todo.pop() {
        if parts.contains_key(&part) {
            continue;
        }
        let (body, here, names) = match &part {
            Part::File(p) => {
                let Some(f) = file_of(walk.src, p) else {
                    continue;
                };
                let text = code(&std::fs::read_to_string(&f).expect("a module"));
                let body = match walk.start.contains(&part) {
                    true => text,
                    false => without_tests(&text),
                };
                let names = aliases(walk.src, &body, walk.sinks);
                (body, p.clone(), names)
            }
            Part::Fn(p, name) => {
                let Some((fs, names)) = by_fn.get(p) else {
                    continue;
                };
                let Some(b) = fs.get(name) else { continue };
                (b.clone(), p.clone(), names.clone())
            }
            Part::Cut(p, name, from, to) => {
                let (fs, names) = by_fn
                    .get(p)
                    .expect("a cut is of a module followed fn by fn");
                let b = fs.get(name).expect("a cut is of a fn its module defines");
                let a = b.find(from.as_str()).expect("a cut's start is in its fn");
                let z = a + b[a..]
                    .find(to.as_str())
                    .expect("a cut's end follows its start");
                let kept = format!("{}{}", &b[..a], &b[z + to.len()..]);
                (kept, p.clone(), names.clone())
            }
        };
        let mut found: Vec<Vec<String>> = paths(&body)
            .into_iter()
            .map(|(is_super, p)| {
                let mut full = match is_super {
                    true => here[..here.len().saturating_sub(1)].to_vec(),
                    false => Vec::new(),
                };
                full.extend(p);
                full
            })
            .collect();
        found.extend(through_aliases(&body, &names));
        for full in found {
            // A sink's item is recorded, never read.
            if let Some(k) = (1..=full.len()).find(|&k| is_sink(&full[..k])) {
                let item = full.get(k).cloned().unwrap_or_default();
                sink_items.insert((full[..k].to_vec(), item, part.clone()));
                continue;
            }
            // The longest prefix that is a module's file; past it, an item.
            let k = (1..=full.len())
                .rev()
                .find(|&k| file_of(walk.src, &full[..k]).is_some());
            let Some(k) = k else { continue };
            let module = full[..k].to_vec();
            match follow_of(&module) {
                Some(_) => {
                    if let Some(item) = full.get(k) {
                        todo.push(Part::Fn(module, item.clone()));
                    }
                }
                None => todo.push(Part::File(module)),
            }
        }
        if let Part::Fn(p, _) | Part::Cut(p, _, _, _) = &part {
            // And the fns of its own module it calls by name.
            let follow = follow_of(p).expect("followed fn by fn");
            let (fs, _) = &by_fn[p];
            for f in fs.keys() {
                let called = body.match_indices(f.as_str()).any(|(at, _)| {
                    let before = body[..at].chars().last();
                    let bare = !before.is_some_and(|c| c.is_alphanumeric() || c == '_');
                    let qualified = before.is_some_and(|c| c == '.' || c == ':');
                    // A fn's own signature calls nothing.
                    let defined = body[..at].ends_with("fn ");
                    body[at + f.len()..].starts_with('(')
                        && bare
                        && !defined
                        && (follow == Follow::AllFns || !qualified)
                });
                if called {
                    todo.push(Part::Fn(p.clone(), f.clone()));
                }
            }
        }
        parts.insert(part, body);
    }
    Reached { parts, sink_items }
}

/// **The lines of what was reached that name a forbidden name or make a
/// forbidden call**, but those an allowance names: its module, and the
/// code on the line. Each allowance must be used, so none outlives its
/// reason; an unused one is reported too.
pub fn hits(reached: &Reached, calls: &[&str], allowed: &[(&[&str], &str, &str)]) -> Vec<String> {
    let module = |part: &Part| match part {
        Part::File(p) | Part::Fn(p, _) | Part::Cut(p, _, _, _) => p.clone(),
    };
    let mut found = Vec::new();
    let mut used = vec![false; allowed.len()];
    for (part, body) in &reached.parts {
        for line in body.lines() {
            let names = FORBIDDEN.iter().any(|f| line.contains(f));
            let call = calls
                .iter()
                .any(|c| line.contains(c) && !line.contains(&format!("fn {c}")));
            if !(names || call) {
                continue;
            }
            let m = module(part);
            match allowed.iter().position(|(at, code, _)| {
                at.iter().copied().eq(m.iter().map(String::as_str)) && line.contains(code)
            }) {
                Some(i) => used[i] = true,
                None => found.push(format!("{part:?}: {}", line.trim())),
            }
        }
    }
    for ((at, _, why), u) in allowed.iter().zip(used) {
        if !u {
            found.push(format!("{at:?}'s allowance is unused: {why}"));
        }
    }
    found
}
