//! **One store owns every surface's cells and marks, and every store to them
//! stamps its row.**
//!
//! A renderer keeps the rows of its last copy that no write has touched since
//! (`Term::row_gens`, `Term::wrote_since`). That is sound only if *every*
//! store to a surface's cells or marks moves its row's generation, so this
//! check reads `baud-domain`'s source and `baud-core`'s and holds them to the
//! one shape that guarantees it: a surface's cells, and its rows' marks and
//! generations (one `RowMeta` a row), are private to
//! `baud-domain/src/grid/store.rs`, and the only mutable way in is a token
//! that has already stamped (`Stamped`, `RowMut`, `MarkMut`).
//!
//! Keyed on **types and access paths**, not on verbs: a verb list misses the
//! next spelling (`get_mut` then `*c =`, `first_mut`, `iter_mut`) and fires on
//! buffers that are not surfaces. Two rules, a third that keeps the stamp of a
//! surface's last write from being skipped, and a fourth on the stamp's own
//! writers.
//!
//! * **Rule A, types.** The text `mut [PackedCell]`, `mut [RowExtent]`,
//!   `mut [RowGen]` or `mut [RowMeta]` -- with any lifetime before `mut`,
//!   and with or without a path before the name -- appears in code only at
//!   the entries of [`ALLOWED`], each with its reason. In `grid/store.rs`,
//!   the home, it may appear anywhere but in a `fn`'s return type: a method
//!   there that hands out a bare slice would be a way in that skipped the
//!   stamp.
//! * **Rule B, access paths.** The spellings that reached a surface's raw
//!   fields are gone: `ring_cells_mut` anywhere; in `baud-core`, the
//!   `Storage`, `AltBuf`, `StatusBuf` and `Surface` fields by their old
//!   names; in `baud-domain`, `self.cells` and `self.used` inside an `impl
//!   View` or `impl Surface` block. `Viewport`, `ViewRef` and the generic
//!   `Grid` keep fields of those names and are read-only or not surfaces, so
//!   the rule is scoped to the two impls rather than to the file. The status
//!   line's lent row (`StatusBuf`) is named only where it is declared, built
//!   and moved into the store (`Storage::with_status_line`).
//! * **Rule C, the drop.** A surface's last write is stored when the
//!   `Stamped` that borrowed it is dropped, so a `Stamped` -- or a `View`,
//!   `Surface` or `Paint` holding one -- that is never dropped leaves every
//!   write it made unreported to `Term::wrote_since`. Neither crate spells
//!   `mem::forget` or `ManuallyDrop`, or imports `forget` from `mem`: a
//!   type-keyed rule would have to follow every holder, and neither crate
//!   has a use for either.
//! * **Rule D, the stamp's own writers.** The stamp is one store into the
//!   row's `RowMeta`, made by every mint before it hands out a token. So in
//!   `grid/store.rs` a row's generation, a surface's entries and its last
//!   write are written only by the fns that own them, and a token is built
//!   only on or after its fn's stamp. A one-character writer stores through
//!   `RowWrite`, which checks a column against the surface's cells, not the
//!   pitch: sound while a view is never wider than the pitch, so `RowWrite`
//!   is built only where it is minted, and in the view's module the width is
//!   written only in `set_geometry` and `View::lent`, each after its assert.
//!
//! Comments are not code: a doc that names a slice type is not a store, so
//! each line is read up to its `//`. Every failure is printed with its file,
//! line and text, so a red is the list itself and not a count.

use std::fs;
use std::path::{Path, PathBuf};

/// The slice element types a surface is made of.
const SURFACE_TYPES: [&str; 4] = ["PackedCell", "RowExtent", "RowGen", "RowMeta"];

/// **Rule A's allowlist**: `(file, the code's trimmed text, why)`. Each entry
/// must match exactly one line, so a second spelling of an allowed line is a
/// failure too, and an entry nothing matches any more fails as stale.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "baud-domain/src/grid.rs",
        "pub fn window_into(&self, dst: &mut [PackedCell]) {",
        "a reader's own buffer, written from the window",
    ),
    (
        "baud-domain/src/grid.rs",
        "fn put_designated_run(dst: &mut [PackedCell], src: &[u8], proto: PackedCell, set: Charset) {",
        "a sub-slice of a RowMut minted by the caller; the slice keeps the hot loop's shape",
    ),
    (
        "baud-domain/src/grid.rs",
        "fn put_narrow_vectored(dst: &mut [PackedCell], cps: &[Codepoint], proto: PackedCell) {",
        "a sub-slice of a RowMut minted by the caller; the slice keeps the hot loop's shape",
    ),
    (
        "baud-domain/src/grid.rs",
        "fn put_wide_vectored(dst: &mut [PackedCell], cps: &[Codepoint], proto: PackedCell) {",
        "a sub-slice of a RowMut minted by the caller; the slice keeps the hot loop's shape",
    ),
    (
        "baud-domain/src/grid.rs",
        "fn put_ascii_vectored(dst: &mut [PackedCell], src: &[u8], proto: PackedCell) {",
        "a sub-slice of a RowMut minted by the caller; the slice keeps the hot loop's shape",
    ),
    (
        "baud-core/src/storage.rs",
        "vp: &'b mut [PackedCell],",
        "Storage::new's parameter: the caller's memory, moved into the store",
    ),
    (
        "baud-core/src/storage.rs",
        "used: &'b mut [RowMeta],",
        "Storage::new's parameter",
    ),
    (
        "baud-core/src/storage.rs",
        "alt: &'b mut [PackedCell],",
        "Storage::new's parameter",
    ),
    (
        "baud-core/src/storage.rs",
        "alt_used: &'b mut [RowMeta],",
        "Storage::new's parameter",
    ),
    (
        "baud-core/src/storage.rs",
        "cells3: &'b mut [PackedCell],",
        "Storage::new's parameter: the three-byte cell table, not a surface",
    ),
    (
        "baud-core/src/storage.rs",
        "pub(crate) cells3: &'b mut [PackedCell],",
        "the three-byte cell table, not a surface",
    ),
    (
        "baud-core/src/storage.rs",
        "vp: &'v mut [PackedCell],",
        "Owned::storage_over's parameter: the caller's memory, GPU-shared on the app",
    ),
    (
        "baud-core/src/storage.rs",
        "alt: &'v mut [PackedCell],",
        "Owned::storage_over's parameter: the caller's memory, GPU-shared on the app",
    ),
    (
        "baud-core/src/storage.rs",
        "let cells: &'b mut [PackedCell] = row.lent_cells;",
        "Storage::with_status_line: the status line's lent row, moved into the store, its one move",
    ),
    (
        "baud-core/src/storage.rs",
        "let used: &'b mut [RowMeta] = row.lent_used;",
        "Storage::with_status_line: the lent row's extent, its one move",
    ),
    (
        "baud-core/src/status.rs",
        "pub(crate) lent_cells: &'b mut [PackedCell],",
        "StatusBuf: the status line's row as the embedder lends it, until with_status_line moves it",
    ),
    (
        "baud-core/src/status.rs",
        "pub(crate) lent_used: &'b mut [RowMeta],",
        "StatusBuf: the lent row's extent",
    ),
    (
        "baud-core/src/status.rs",
        "pub fn new(cells: &'b mut [PackedCell], used: &'b mut [RowMeta]) -> StatusBuf<'b> {",
        "StatusBuf::new: the carrier's constructor, its signature unchanged",
    ),
];

/// The home of every surface store.
const HOME: &str = "baud-domain/src/grid/store.rs";

/// Rule B's spellings in `baud-core`: the raw fields a surface was reached
/// through.
const CORE_PATHS: [&str; 10] = [
    "store.vp",
    "store.used",
    "alt.cells",
    "alt.used",
    "status.cells",
    "status.used",
    "other.cells",
    "other.used",
    "surface.cells",
    "surface.used",
];

/// `StatusBuf`'s two fields, and the only places they may be named: the
/// declaration, its constructor, and the one move into the store.
const LENT: [&str; 2] = ["lent_cells", "lent_used"];
/// Each place by the item enclosing it, so a reflow by `rustfmt` cannot move
/// a use out of the list: `(file, the enclosing item's header)`.
const LENT_IN: &[(&str, &str)] = &[
    ("baud-core/src/status.rs", "pub struct StatusBuf<'b>"),
    ("baud-core/src/status.rs", "impl<'b> StatusBuf<'b>"),
    ("baud-core/src/storage.rs", "pub fn with_status_line("),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut rd: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("a directory entry").path())
        .collect();
    rd.sort();
    for p in rd {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The line's code: up to a `//` outside a string literal, with string and
/// character literals blanked so a brace or a path inside one is not read.
fn code_of(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
                out.push('"');
            }
            i += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            break;
        }
        if c == b'"' {
            in_str = true;
            out.push('"');
            i += 1;
            continue;
        }
        // A character literal: `'x'` or `'\x'`, never a lifetime (`'a`
        // followed by anything but a closing quote).
        if c == b'\'' {
            let close = match b.get(i + 1) {
                Some(b'\\') => b[i + 2..]
                    .iter()
                    .position(|&x| x == b'\'')
                    .map(|k| i + 2 + k),
                Some(_) if b.get(i + 2) == Some(&b'\'') => Some(i + 2),
                _ => None,
            };
            if let Some(k) = close {
                out.push_str("' '");
                i = k + 1;
                continue;
            }
        }
        out.push(c as char);
        i += 1;
    }
    out
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Whether `code` spells `mut [T]` for a surface type `T`, any path before
/// the name.
fn names_surface_slice(code: &str) -> bool {
    let b = code.as_bytes();
    let mut from = 0;
    while let Some(k) = code[from..].find("mut") {
        let at = from + k;
        from = at + 3;
        if at > 0 && is_ident(b[at - 1]) {
            continue;
        }
        let mut i = at + 3;
        if i >= b.len() || !b[i].is_ascii_whitespace() {
            continue;
        }
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) != Some(&b'[') {
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while i < b.len() && (is_ident(b[i]) || b[i] == b':') {
            i += 1;
        }
        let path = &code[s..i];
        let name = path.rsplit("::").next().unwrap_or(path);
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) == Some(&b']') && SURFACE_TYPES.contains(&name) {
            return true;
        }
    }
    false
}

/// Whether `code` spells `pat` as a field path, bounded on both sides: not
/// `x_alt.cells`, not `alt.cells_x`, and not `alt.cells()`, a method --
/// `Stamped::cells` is a read.
fn spells_field(code: &str, pat: &str) -> bool {
    spells_at(code, pat, true)
}

/// Whether `code` spells `pat` bounded on both sides, a call included.
fn spells(code: &str, pat: &str) -> bool {
    spells_at(code, pat, false)
}

fn spells_at(code: &str, pat: &str, field: bool) -> bool {
    let b = code.as_bytes();
    let mut from = 0;
    while let Some(k) = code[from..].find(pat) {
        let at = from + k;
        from = at + 1;
        let before = at == 0 || !is_ident(b[at - 1]);
        let end = at + pat.len();
        let after = end >= b.len() || !is_ident(b[end]);
        let call = field && b.get(end) == Some(&b'(');
        if before && after && !call {
            return true;
        }
    }
    false
}

struct Line {
    file: String,
    no: LineNo,
    code: String,
}

/// A line's one-based number in its file.
#[derive(Clone, Copy)]
struct LineNo(usize);

impl std::fmt::Display for LineNo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn lines_of(crate_dir: &str) -> Vec<Line> {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join(crate_dir).join("src"), &mut files);
    let mut out = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(&root)
            .expect("under the root")
            .to_string_lossy()
            .into_owned();
        let text = fs::read_to_string(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for (i, l) in text.lines().enumerate() {
            out.push(Line {
                file: rel.clone(),
                no: LineNo(i + 1),
                code: code_of(l),
            });
        }
    }
    out
}

/// Rule A over one crate's lines. Returns the failures, and counts each
/// allowlist entry's matches into `seen`.
fn rule_a(lines: &[Line], seen: &mut [usize]) -> Vec<String> {
    let mut bad = Vec::new();
    let mut sig: Option<(usize, String)> = None;
    let mut last_impl = String::new();
    for l in lines {
        if l.file == HOME {
            // A `fn` there whose return type names a bare surface slice.
            let t = l.code.trim();
            if t.starts_with("impl") {
                last_impl = t.to_string();
            }
            if sig.is_none() && spells(t, "fn") {
                sig = Some((l.no.0, String::new()));
            }
            if let Some((no, s)) = sig.as_mut() {
                s.push_str(t);
                s.push(' ');
                if t.contains('{') || t.ends_with(';') {
                    let head = s.split('{').next().unwrap_or(s);
                    // The tokens' own `DerefMut` is the way in, already
                    // stamped: `RowMut` derefs to its slice.
                    let token = head.trim_start().starts_with("fn deref_mut(&mut self)")
                        && last_impl.contains(" for RowMut<");
                    if !token
                        && head
                            .split_once("->")
                            .is_some_and(|(_, ret)| names_surface_slice(ret))
                    {
                        bad.push(format!(
                            "Rule A: {HOME}:{no}: a fn returns a bare surface slice: {}",
                            head.trim()
                        ));
                    }
                    sig = None;
                }
            }
            continue;
        }
        if !names_surface_slice(&l.code) {
            continue;
        }
        let t = l.code.trim();
        match ALLOWED
            .iter()
            .position(|(f, text, _)| *f == l.file && *text == t)
        {
            Some(k) => seen[k] += 1,
            None => bad.push(format!("Rule A: {}:{}: {t}", l.file, l.no)),
        }
    }
    bad
}

/// The `impl View` and `impl Surface` blocks' lines, by brace depth.
fn in_view_impls(lines: &[Line]) -> Vec<bool> {
    let mut out = vec![false; lines.len()];
    let mut depth: i64 = 0;
    let mut open: Option<i64> = None;
    let mut file = "";
    for (i, l) in lines.iter().enumerate() {
        if l.file != file {
            file = &l.file;
            depth = 0;
            open = None;
        }
        let t = l.code.trim_start();
        if open.is_none() && t.starts_with("impl") {
            let head = t.split('{').next().unwrap_or(t);
            let target = head.rsplit(" for ").next().unwrap_or(head);
            let target = target
                .trim_start_matches("impl")
                .trim_start_matches(|c: char| c != ' ' && !c.is_ascii_uppercase());
            let name: String = target
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if (name == "View" || name == "Surface") && !head.contains(" for ") {
                open = Some(depth);
            }
        }
        if open.is_some() {
            out[i] = true;
        }
        for c in l.code.bytes() {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if open == Some(depth) {
                        open = None;
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn rule_b_core(lines: &[Line], lent_seen: &mut [usize]) -> Vec<String> {
    let mut bad = Vec::new();
    // The headers of the items enclosing each line, by brace depth.
    let (mut stack, mut file): (Vec<String>, &str) = (Vec::new(), "");
    for l in lines {
        if l.file != file {
            file = &l.file;
            stack.clear();
        }
        let t = l.code.trim();
        let enclosing = stack.clone();
        for c in l.code.bytes() {
            match c {
                b'{' => stack.push(t.to_string()),
                b'}' => {
                    stack.pop();
                }
                _ => {}
            }
        }
        if spells(t, "ring_cells_mut") {
            bad.push(format!("Rule B: {}:{}: {t}", l.file, l.no));
            continue;
        }
        if let Some(p) = CORE_PATHS.iter().find(|p| spells_field(t, p)) {
            bad.push(format!("Rule B ({p}): {}:{}: {t}", l.file, l.no));
            continue;
        }
        if LENT.iter().any(|n| spells(t, n)) {
            let at = LENT_IN
                .iter()
                .position(|(f, h)| *f == l.file && enclosing.iter().any(|e| e.starts_with(h)));
            match at {
                Some(k) => lent_seen[k] += 1,
                None => bad.push(format!(
                    "Rule B (StatusBuf's row): {}:{}: {t}",
                    l.file, l.no
                )),
            }
        }
    }
    bad
}

fn rule_b_domain(lines: &[Line]) -> Vec<String> {
    let scoped = in_view_impls(lines);
    let mut bad = Vec::new();
    for (l, &inside) in lines.iter().zip(&scoped) {
        if l.file == HOME {
            continue;
        }
        let t = l.code.trim();
        if spells(t, "ring_cells_mut") {
            bad.push(format!("Rule B: {}:{}: {t}", l.file, l.no));
            continue;
        }
        if inside && (spells_field(t, "self.cells") || spells_field(t, "self.used")) {
            bad.push(format!("Rule B (impl View): {}:{}: {t}", l.file, l.no));
        }
    }
    bad
}

/// The spellings that skip a `Drop`.
const FORGETS: [&str; 2] = ["mem::forget", "ManuallyDrop"];

/// Rule C over one crate's lines: no spelling in [`FORGETS`], and no `use`
/// statement -- across however many lines `rustfmt` laid it out on -- that
/// takes `forget` from `mem`.
fn rule_c(lines: &[Line]) -> Vec<String> {
    let mut bad = Vec::new();
    let mut stmt: Option<String> = None;
    for l in lines {
        let t = l.code.trim();
        if FORGETS.iter().any(|p| spells(t, p)) {
            bad.push(format!("Rule C: {}:{}: {t}", l.file, l.no));
            continue;
        }
        let opens = ["use ", "pub use ", "pub(crate) use "]
            .iter()
            .any(|u| t.starts_with(u));
        if opens && stmt.is_none() {
            stmt = Some(String::new());
        }
        if let Some(s) = stmt.as_mut() {
            s.push_str(t);
            s.push(' ');
            if t.contains(';') {
                if spells(s, "mem") && spells(s, "forget") {
                    bad.push(format!("Rule C (import): {}:{}: {s}", l.file, l.no));
                }
                stmt = None;
            }
        }
    }
    bad
}

/// **Rule D's writers**: `(field, the fns of [`HOME`] that may write it)`. A
/// row's generation is stored by the stamp alone (`stamp`, and `stamp_all`
/// for every row), beside the seal's wrap. A surface's entries -- each row's
/// mark beside its generation -- are written through a field path only where
/// they are lent (`of`), swept (`seal`, `stamp_all`) or handed out whole
/// after the stamp (`all_mut`), or exchanged and both stamped (`swap_rows`):
/// every other store reaches an entry through
/// the mark `stamp` hands back. A surface's last write has one writer, the
/// `Stamped`'s `Drop`, beside the seal's wrap and the mint's borrow of it.
const D_WRITERS: &[(&str, &[&str])] = &[
    ("row_gen", &["stamp", "stamp_all", "seal"]),
    ("meta", &["of", "seal", "stamp_all", "all_mut", "swap_rows"]),
    ("last", &["drop", "seal", "of"]),
];

/// The token a one-character writer stores through, and the one fn that
/// builds it.
const TOKEN: (&str, &str) = ("RowWrite", "row_at");

/// The tokens built by a call, `Name(..)`: each a way to store, minted only
/// after the stamp.
const CALL_TOKENS: [&str; 3] = ["RowMut", "MarkMut", "MarksMut"];

/// The `View`'s module: `grid.rs` and its children, which see its private
/// fields.
fn in_grid_module(file: &str) -> bool {
    file == "baud-domain/src/grid.rs" || file.starts_with("baud-domain/src/grid/")
}

/// The innermost `fn` among `headers`, the items enclosing a line: its name,
/// and its header, which tells two fns of one name apart.
fn enclosing_fn(headers: &[String]) -> Option<(String, String)> {
    headers.iter().rev().find_map(|h| {
        let at = h
            .find("fn ")
            .filter(|&k| k == 0 || !is_ident(h.as_bytes()[k - 1]))?;
        let name: String = h[at + 3..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        Some((name, h.clone()))
    })
}

/// The slice and field methods that write through what they are called on.
const MUT_METHODS: [&str; 12] = [
    "fill",
    "fill_with",
    "iter_mut",
    "get_mut",
    "first_mut",
    "last_mut",
    "split_at_mut",
    "copy_from_slice",
    "clone_from_slice",
    "copy_within",
    "swap",
    "as_mut_ptr",
];

/// Whether `code` writes the field `id` (`.id`, never a local of that
/// name): `.id = ` or `op=`, `.id[..] = `, a `&mut` borrow of it, or a call
/// through it to one of [`MUT_METHODS`]. A compare (`==`, `!=`, `<=`, `>=`)
/// is a read.
fn writes(code: &str, id: &str) -> bool {
    let b = code.as_bytes();
    let mut from = 0;
    while let Some(k) = code[from..].find(id) {
        let at = from + k;
        from = at + 1;
        let end = at + id.len();
        if at == 0 || b[at - 1] != b'.' || (end < b.len() && is_ident(b[end])) {
            continue;
        }
        // A `&mut` borrow of a path ending in `id`.
        let head = code[..at]
            .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            .trim_end_matches('*')
            .trim_end();
        if head.ends_with("&mut") {
            return true;
        }
        let mut i = end;
        if b.get(i) == Some(&b'[') {
            let mut depth = 0;
            while i < b.len() {
                match b[i] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    _ => {}
                }
                i += 1;
                if depth == 0 {
                    break;
                }
            }
        }
        let rest = code[i..].trim_start();
        let assigns = match rest.as_bytes() {
            [b'=', b'=', ..] => false,
            [b'=', ..] => true,
            [b'<', b'<', b'=', ..] | [b'>', b'>', b'=', ..] => true,
            [op, b'=', ..] => b"+-*/%|&^".contains(op),
            _ => false,
        };
        let calls = rest.strip_prefix('.').is_some_and(|m| {
            MUT_METHODS
                .iter()
                .any(|n| m.starts_with(n) && m[n.len()..].starts_with('('))
        });
        if assigns || calls {
            return true;
        }
    }
    false
}

/// Whether `code` builds `name` by a struct literal: `name {`, not a
/// declaration or an `impl` (`name<'a> {`).
fn builds(code: &str, name: &str) -> bool {
    let b = code.as_bytes();
    let mut from = 0;
    while let Some(k) = code[from..].find(name) {
        let at = from + k;
        from = at + 1;
        let end = at + name.len();
        if (at > 0 && is_ident(b[at - 1])) || (end < b.len() && is_ident(b[end])) {
            continue;
        }
        let before = code[..at].trim_end();
        if ["struct", "impl", "for", "enum"]
            .iter()
            .any(|w| before.ends_with(w))
        {
            continue;
        }
        if code[end..].trim_start().starts_with('{') {
            return true;
        }
    }
    false
}

/// Whether `code` calls `name`: `name(`, the name bounded before. A tuple
/// struct's constructor is such a call; its declaration (`Name<'a>(`) and
/// a type (`Name<'_>`) are not.
fn calls(code: &str, name: &str) -> bool {
    let b = code.as_bytes();
    let mut from = 0;
    while let Some(k) = code[from..].find(name) {
        let at = from + k;
        from = at + 1;
        let end = at + name.len();
        if (at == 0 || !is_ident(b[at - 1])) && b.get(end) == Some(&b'(') {
            return true;
        }
    }
    false
}

/// Whether `code` stamps: a call of `stamp` or `stamp_all`, not the
/// definition of `stamp`.
fn calls_stamp(code: &str) -> bool {
    (calls(code, "stamp") && !spells(code, "fn stamp")) || calls(code, "stamp_all")
}

/// **Rule D over `baud-domain`'s lines**: who may write a row's
/// generation and a surface's entries, where a token may be built, and a
/// view's width.
///
/// * In [`HOME`], outside its tests, each field of [`D_WRITERS`] is written
///   only inside its listed fns, and the [`TOKEN`] built only in its
///   minting fn.
/// * In [`HOME`], a fn that builds a token ([`CALL_TOKENS`], or the
///   [`TOKEN`] by its literal) calls `stamp` or `stamp_all` on that line or
///   an earlier one of the same fn: a way to store handed out before the
///   stamp is a store the stamp may not cover. A token built from a token
///   already stamped is the exception: `RowWrite::mark` builds its mark
///   from a `RowWrite`, and `RowMut::part` a narrower `RowMut` from a
///   `RowMut`.
/// * In the `View`'s module, a `cols` field is written only in
///   `set_geometry`, after an `assert!` in it, and a `View {` literal is
///   built only in a fn `lent`, after an `assert!` in that fn: a column at
///   or past the pitch would store into the next row, which the token does
///   not check.
/// * The control: the field `row_gen`, the fns `stamp`, `row_at` and
///   `set_geometry` and the token's struct must each be found, so a rename
///   that took the rule's subject away fails it rather than passing it.
fn rule_d(lines: &[Line]) -> Vec<String> {
    let mut bad = Vec::new();
    let (mut stack, mut file): (Vec<String>, &str) = (Vec::new(), "");
    let mut found = [false; 5];
    let mut stamped_in: Option<String> = None;
    let mut asserted_in: Option<String> = None;
    // A `fn` signature `rustfmt` laid out over several lines, until its `{`.
    let mut sig: Option<String> = None;
    for l in lines {
        if l.file != file {
            file = &l.file;
            stack.clear();
            sig = None;
        }
        let t = l.code.trim();
        let enclosing = stack.clone();
        if sig.is_none() && spells(t, "fn") && !t.contains('{') && !t.ends_with(';') {
            sig = Some(String::new());
        }
        if let Some(s) = sig.as_mut() {
            s.push_str(t);
            s.push(' ');
        }
        for c in l.code.bytes() {
            match c {
                b'{' => stack.push(sig.take().unwrap_or_else(|| t.to_string())),
                b'}' => {
                    stack.pop();
                }
                _ => {}
            }
        }
        if t.ends_with(';') {
            sig = None;
        }
        if enclosing.iter().any(|h| h.starts_with("mod tests")) {
            continue;
        }
        let (f, header) = enclosing_fn(&enclosing).unzip();
        let in_fn = |n: &str| f.as_deref() == Some(n);
        let at = || format!("{}:{}: {t}", l.file, l.no);
        if l.file == HOME {
            found[0] |= f.is_none() && t.starts_with("row_gen: RowGen");
            found[1] |= spells(t, "fn stamp");
            found[2] |= spells(t, &format!("fn {}", TOKEN.1));
            found[4] |= spells(t, &format!("pub struct {}", TOKEN.0));
            for (id, fns) in D_WRITERS {
                if writes(t, id) && !fns.iter().any(|n| in_fn(n)) {
                    bad.push(format!("Rule D ({id} written in {f:?}): {}", at()));
                }
            }
            if f.is_some() && calls_stamp(t) {
                stamped_in = header.clone();
            }
            let token = CALL_TOKENS
                .iter()
                .copied()
                .find(|k| calls(t, k))
                .or_else(|| builds(t, TOKEN.0).then_some(TOKEN.0));
            let from_token = enclosing.iter().any(|h| {
                h.starts_with(&format!("impl {}", TOKEN.0)) || h.starts_with("impl RowMut<")
            });
            if let Some(k) = token {
                if f.is_some() && !from_token && (stamped_in.is_none() || stamped_in != header) {
                    bad.push(format!(
                        "Rule D ({k} built before the stamp in {f:?}): {}",
                        at()
                    ));
                }
            }
            if builds(t, TOKEN.0) && !in_fn(TOKEN.1) {
                bad.push(format!("Rule D ({} built in {f:?}): {}", TOKEN.0, at()));
            }
        }
        if in_grid_module(&l.file) {
            found[3] |= spells(t, "fn set_geometry");
            if t.starts_with("assert!(") {
                asserted_in = header.clone();
            }
            let checked = asserted_in.is_some() && asserted_in == header;
            if writes(t, "cols") {
                if !in_fn("set_geometry") {
                    bad.push(format!("Rule D (cols written in {f:?}): {}", at()));
                } else if !checked {
                    bad.push(format!("Rule D (cols before the assert): {}", at()));
                }
            }
            if builds(t, "View") {
                if !in_fn("lent") {
                    bad.push(format!("Rule D (View built in {f:?}): {}", at()));
                } else if !checked {
                    bad.push(format!("Rule D (View built before the assert): {}", at()));
                }
            }
        }
    }
    let names = [
        "the field `row_gen`",
        "fn stamp",
        "fn row_at",
        "fn set_geometry",
        "struct RowWrite",
    ];
    for (n, ok) in names.iter().zip(found) {
        if !ok {
            bad.push(format!("Rule D (control): {n} not found"));
        }
    }
    bad
}

#[test]
fn every_surface_store_goes_through_the_one_store() {
    let domain = lines_of("baud-domain");
    let core = lines_of("baud-core");
    let mut seen = vec![0usize; ALLOWED.len()];
    let mut lent_seen = vec![0usize; LENT_IN.len()];
    let mut bad = Vec::new();
    bad.extend(rule_a(&domain, &mut seen));
    bad.extend(rule_a(&core, &mut seen));
    bad.extend(rule_b_core(&core, &mut lent_seen));
    bad.extend(rule_b_domain(&domain));
    bad.extend(rule_c(&core));
    bad.extend(rule_c(&domain));
    bad.extend(rule_d(&domain));
    for ((f, t, why), n) in ALLOWED.iter().zip(&seen) {
        if *n != 1 {
            bad.push(format!(
                "Rule A allowlist: {f}: `{t}` ({why}) matched {n} lines, not one"
            ));
        }
    }
    for ((f, h), n) in LENT_IN.iter().zip(&lent_seen) {
        if *n == 0 {
            bad.push(format!(
                "Rule B allowlist: {f}: no use of StatusBuf's row inside `{h}`, a stale entry"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} line(s) reach a surface's cells or marks outside the one store:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// **Positive controls**: each matcher fires on the spellings it exists for,
/// so a check that passes is a check that could have failed.
#[test]
fn the_matchers_fire_on_what_they_name() {
    assert!(names_surface_slice("fn f(a: &mut [PackedCell])"));
    assert!(names_surface_slice("fn f(a: &'a mut [ RowExtent ])"));
    assert!(names_surface_slice("x: &mut [baud_domain::RowGen],"));
    assert!(names_surface_slice("used: &'b mut [RowMeta],"));
    assert!(names_surface_slice(
        "type R<'a> = &'a mut [crate::PackedCell];"
    ));
    assert!(!names_surface_slice("fn f(a: &mut [u8])"));
    assert!(!names_surface_slice("fn f(a: &[PackedCell])"));
    assert!(!names_surface_slice("let immut [PackedCell]"));
    assert!(spells_field(
        "let a = &mut self.other.cells;",
        "other.cells"
    ));
    assert!(spells_field("alt.cells[0] = x;", "alt.cells"));
    assert!(!spells_field("alt.cells().len()", "alt.cells"));
    assert!(spells("v.ring_cells_mut()", "ring_cells_mut"));
    let forgets: Vec<Line> = [
        "core::mem::forget(p);",
        "let v = ManuallyDrop::new(view);",
        "use core::mem::{",
        "    forget,",
        "    swap,",
        "};",
        "use core::mem;",
        "self.cold.placement.forget();",
        "self.forget(i);",
        "use crate::placed::forget_all;",
    ]
    .iter()
    .enumerate()
    .map(|(i, s)| Line {
        file: "x.rs".into(),
        no: LineNo(i + 1),
        code: code_of(s),
    })
    .collect();
    let bad = rule_c(&forgets);
    assert_eq!(bad.len(), 3, "{bad:?}");
    assert!(
        bad[0].contains(":1:") && bad[1].contains(":2:") && bad[2].contains(":6:"),
        "{bad:?}"
    );
    assert!(spells("self.store.vp[0]", "store.vp"));
    assert!(!spells("my_alt.cells", "alt.cells"));
    assert!(!spells("alt.cells_of", "alt.cells"));
    assert_eq!(code_of("a // b"), "a ");
    assert_eq!(code_of("let s = \"//x{\"; y"), "let s = \"\"; y");
    assert_eq!(code_of("match c { '{' => 1 }"), "match c { ' ' => 1 }");
    assert_eq!(code_of("fn f<'a>(x: &'a T)"), "fn f<'a>(x: &'a T)");
    let lines: Vec<Line> = [
        "impl<'a> View<'a> {",
        "    fn f(&mut self) { self.cells[0] = x; }",
        "}",
        "impl Viewport<'_> {",
        "    fn g(&self) { self.cells[0] }",
        "}",
        "impl<'a> Surface<'a> {",
        "    fn h(&mut self) { self.used[0] = y; }",
        "}",
    ]
    .iter()
    .enumerate()
    .map(|(i, s)| Line {
        file: "x.rs".into(),
        no: LineNo(i + 1),
        code: code_of(s),
    })
    .collect();
    let bad = rule_b_domain(&lines);
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(bad[0].contains(":2:") && bad[1].contains(":8:"), "{bad:?}");
    // StatusBuf's row: inside `with_status_line` it may be named, inside
    // any other function it may not.
    let lines: Vec<Line> = [
        "    pub fn with_status_line(mut self, row: StatusBuf<'b>) -> Storage<'b> {",
        "        let used = row.lent_used;",
        "    }",
        "    fn other(&mut self, row: StatusBuf<'b>) {",
        "        let cells = row.lent_cells;",
        "    }",
    ]
    .iter()
    .enumerate()
    .map(|(i, s)| Line {
        file: "baud-core/src/storage.rs".into(),
        no: LineNo(i + 1),
        code: code_of(s),
    })
    .collect();
    let mut seen = vec![0usize; LENT_IN.len()];
    let bad = rule_b_core(&lines, &mut seen);
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(bad[0].contains(":5:"), "{bad:?}");
    assert_eq!(seen[2], 1);
}

/// **Rule D's positive controls**: each writer it names, put in a fn that
/// does not own it, is reported, on the line it is on, and so is a token
/// built before the stamp; the listed fns' own writes are not, nor a token
/// built on or after its fn's stamp, nor `RowWrite`'s mark; and a file that
/// lost the rule's subjects fails its control.
#[test]
fn rule_d_fires_on_what_it_names() {
    assert!(writes("m.row_gen = RowGen(now.0);", "row_gen"));
    assert!(writes(
        "self.meta[p].row_gen = RowGen(self.now.0);",
        "row_gen"
    ));
    assert!(writes("self.meta[p] = RowMeta::ZERO;", "meta"));
    assert!(writes(
        "(RowMut(&mut *self.cells), MarksMut(&mut *self.meta))",
        "meta"
    ));
    assert!(writes("for m in self.meta.iter_mut() {", "meta"));
    assert!(writes("*self.last = self.now;", "last"));
    assert!(writes("self.cols = cols;", "cols"));
    assert!(!writes(
        "self.meta.get(p).is_none_or(|m| m.row_gen.0 > c)",
        "row_gen"
    ));
    assert!(!writes("let mark = stamp(self.meta, p, self.now);", "meta"));
    assert!(!writes("let from = self.meta[src].used;", "meta"));
    assert!(!writes("let cols = self.cols.index();", "cols"));
    assert!(!writes("let row_gen = x;", "row_gen"));
    assert!(calls(
        "(RowMut(&mut self.cells[a..b]), MarkMut(mark))",
        "MarkMut"
    ));
    assert!(!calls(
        "pub struct RowMut<'a>(&'a mut [PackedCell]);",
        "RowMut"
    ));
    assert!(!calls("-> (RowMut<'_>, MarkMut<'_>)", "RowMut"));
    assert!(!calls(
        "pub struct MarksMut<'a>(&'a mut [RowMeta]);",
        "MarkMut"
    ));
    assert!(calls_stamp("        *stamp(self.meta, p, self.now) = e;"));
    assert!(calls_stamp("        self.stamp_all();"));
    assert!(!calls_stamp(
        "fn stamp(meta: &mut [RowMeta], p: usize, now: WriteGen) -> &mut RowExtent {"
    ));
    assert!(builds("RowWrite { cells, mark, base, pitch }", "RowWrite"));
    assert!(!builds("pub struct RowWrite<'s> {", "RowWrite"));
    assert!(!builds("impl RowWrite<'_> {", "RowWrite"));
    assert!(builds("        View {", "View"));
    assert!(!builds("ViewRef {", "View"));
    assert!(
        enclosing_fn(&["fn of( plane: P, ) -> Stamped<'a> {".into()])
            .is_some_and(|(n, _)| n == "of")
    );
    let lines = |file: &str, src: &[&str]| -> Vec<Line> {
        src.iter()
            .enumerate()
            .map(|(i, s)| Line {
                file: file.into(),
                no: LineNo(i + 1),
                code: code_of(s),
            })
            .collect()
    };
    let mut home = lines(
        HOME,
        &[
            "pub struct RowMeta {",
            "    row_gen: RowGen,",
            "}",
            "pub struct RowWrite<'s> {",
            "}",
            "fn stamp(meta: &mut [RowMeta], p: usize, now: WriteGen) -> &mut RowExtent {",
            "    let m = &mut meta[p];",
            "    m.row_gen = RowGen(now.0);",
            "    &mut m.used",
            "}",
            "impl<'a> Stamped<'a> {",
            "    pub fn row_at(&mut self, p: Row) -> RowWrite<'_> {",
            "        let mark = stamp(self.meta, p, self.now);",
            "        RowWrite { cells, mark, base, pitch }",
            "    }",
            "    pub fn row_and_mark(&mut self, p: Row) {",
            "        (RowMut(&mut self.cells[a..b]), MarkMut(&mut self.meta[p].used))",
            "    }",
            "    pub fn mark_with(&mut self, p: Row) {",
            "        self.meta[p].row_gen = RowGen(self.now.0);",
            "        *self.last = self.now;",
            "    }",
            "    pub fn row_mut(&mut self, p: Row) {",
            "        let w = RowWrite { cells, mark, base: 0, pitch };",
            "    }",
            "    pub fn mark_mut(&mut self, p: Row) -> MarkMut<'_> {",
            "        MarkMut(stamp(self.meta, p, self.now))",
            "    }",
            "    pub fn all_mut(&mut self) {",
            "        self.stamp_all();",
            "        (RowMut(&mut *self.cells), MarksMut(&mut *self.meta))",
            "    }",
            "    pub fn stamp_all(&mut self) {",
            "        for m in self.meta.iter_mut() {",
            "            m.row_gen = now;",
            "        }",
            "    }",
            "}",
            "impl RowWrite<'_> {",
            "    pub fn mark(&mut self) -> MarkMut<'_> {",
            "        MarkMut(&mut *self.mark)",
            "    }",
            "}",
            "mod tests {",
            "    fn t() { st.meta[3].row_gen = RowGen::ZERO; }",
            "}",
        ],
    );
    home.extend(lines(
        "baud-domain/src/grid.rs",
        &[
            "impl<'a> View<'a> {",
            "    pub fn lent(store: Stamped<'a>) -> View<'a> {",
            "        assert!(",
            "            cols.index() <= store.stride().index(),",
            "        );",
            "        View {",
            "        }",
            "    }",
            "    pub fn set_geometry(&mut self, cols: Cols, rows: Rows) {",
            "        self.cols = cols;",
            "        assert!(cols.index() <= self.stride().index());",
            "    }",
            "    pub fn widen(&mut self, cols: Cols) {",
            "        self.cols = cols;",
            "    }",
            "}",
            "impl<'a> Grid<'a> {",
            "    pub fn lent(cells: &'a [T]) -> Grid<'a> {",
            "        View {",
            "        }",
            "    }",
            "}",
        ],
    ));
    let bad = rule_d(&home);
    let at: Vec<String> = bad
        .iter()
        .map(|b| b.split(": ").next().unwrap_or("").to_string())
        .collect();
    let has = |what: &str, line: &str| bad.iter().any(|b| b.contains(what) && b.contains(line));
    assert!(
        has(
            "(RowMut built before the stamp in Some(\"row_and_mark\"))",
            "store.rs:17:"
        ),
        "{bad:#?}"
    );
    assert!(
        has("(meta written in Some(\"row_and_mark\"))", "store.rs:17:"),
        "{bad:#?}"
    );
    assert!(
        has("(row_gen written in Some(\"mark_with\"))", "store.rs:20:"),
        "{bad:#?}"
    );
    assert!(
        has("(last written in Some(\"mark_with\"))", "store.rs:21:"),
        "{bad:#?}"
    );
    assert!(
        has("(RowWrite built in Some(\"row_mut\"))", "store.rs:24:"),
        "{bad:#?}"
    );
    assert!(
        has(
            "(RowWrite built before the stamp in Some(\"row_mut\"))",
            "store.rs:24:"
        ),
        "{bad:#?}"
    );
    assert!(has("(cols before the assert)", "grid.rs:10:"), "{bad:#?}");
    assert!(
        has("(cols written in Some(\"widen\"))", "grid.rs:14:"),
        "{bad:#?}"
    );
    assert!(
        has("(View built before the assert)", "grid.rs:19:"),
        "{bad:#?}"
    );
    assert_eq!(
        bad.len(),
        9,
        "the listed writers' own writes, the tokens built after their stamp and RowWrite's mark are not reported: {at:#?}"
    );
    // A stamp in one fn does not cover a token built in the next.
    let next = lines(
        HOME,
        &[
            "impl<'a> Stamped<'a> {",
            "    pub fn mark_mut(&mut self, p: Row) {",
            "        let _ = stamp(self.meta, p, self.now);",
            "    }",
            "    pub fn row_mut(&mut self, p: Row) {",
            "        RowMut(&mut self.cells[a..b])",
            "    }",
            "}",
        ],
    );
    let bad = rule_d(&next);
    assert!(
        bad.iter().any(
            |b| b.contains("(RowMut built before the stamp in Some(\"row_mut\"))")
                && b.contains(":6:")
        ),
        "{bad:#?}"
    );
    // The control: a file with none of the subjects fails, naming each.
    let bad = rule_d(&lines(HOME, &["fn other() {}"]));
    assert_eq!(
        bad.iter().filter(|b| b.contains("(control)")).count(),
        5,
        "{bad:#?}"
    );
}
