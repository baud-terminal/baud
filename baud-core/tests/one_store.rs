//! **One store owns every surface's cells and marks, and every store to them
//! stamps its row.**
//!
//! A renderer keeps the rows of its last copy that no write has touched since
//! (`Term::row_gens`, `Term::wrote_since`). That is sound only if *every*
//! store to a surface's cells or marks moves its row's generation, so this
//! check reads `baud-domain`'s source and `baud-core`'s and holds them to the
//! one shape that guarantees it: a surface's cells, marks and generations are
//! private to `baud-domain/src/grid/store.rs`, and the only mutable way in is a
//! token that has already stamped (`Stamped`, `RowMut`, `MarkMut`).
//!
//! Keyed on **types and access paths**, not on verbs: a verb list misses the
//! next spelling (`get_mut` then `*c =`, `first_mut`, `iter_mut`) and fires on
//! buffers that are not surfaces. Two rules, a third that keeps the stamp of a
//! surface's last write from being skipped, and a fourth on the stamp's own
//! writers.
//!
//! * **Rule A, types.** The text `mut [PackedCell]`, `mut [RowExtent]` or
//!   `mut [RowGen]` -- with any lifetime before `mut`, and with or without a
//!   path before the name -- appears in code only at the entries of
//!   [`ALLOWED`], each with its reason. In `grid/store.rs`, the home, it may
//!   appear anywhere but in a `fn`'s return type: a method there that hands
//!   out a bare slice would be a way in that skipped the stamp.
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
//! * **Rule D, the stamp's own writers.** A one-character writer stores
//!   through `RowWrite`, which skips the stamp when its row is the one the
//!   borrow stamped last (`Stamped::stamped`), and checks a column against
//!   the surface's cells, not the pitch. That is sound while the row stamped
//!   last holds `now` and a view is never wider than the pitch, so in
//!   `grid/store.rs` a row's generation, the row stamped last and a
//!   surface's last write are written only by the fns that own them, the
//!   token is built only where it is minted, and in the view's module its
//!   width is written only in `set_geometry` and `View::new`, each after
//!   its assert.
//!
//! Comments are not code: a doc that names a slice type is not a store, so
//! each line is read up to its `//`. Every failure is printed with its file,
//! line and text, so a red is the list itself and not a count.

use std::fs;
use std::path::{Path, PathBuf};

/// The slice element types a surface is made of.
const SURFACE_TYPES: [&str; 3] = ["PackedCell", "RowExtent", "RowGen"];

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
        "used: &'b mut [RowExtent],",
        "Storage::new's parameter",
    ),
    (
        "baud-core/src/storage.rs",
        "gens: &'b mut [RowGen],",
        "Storage::new's parameter",
    ),
    (
        "baud-core/src/storage.rs",
        "alt: &'b mut [PackedCell],",
        "Storage::new's parameter",
    ),
    (
        "baud-core/src/storage.rs",
        "alt_used: &'b mut [RowExtent],",
        "Storage::new's parameter",
    ),
    (
        "baud-core/src/storage.rs",
        "alt_gens: &'b mut [RowGen],",
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
        "let used: &'b mut [RowExtent] = row.lent_used;",
        "Storage::with_status_line: the lent row's extent, its one move",
    ),
    (
        "baud-core/src/status.rs",
        "pub(crate) lent_cells: &'b mut [PackedCell],",
        "StatusBuf: the status line's row as the embedder lends it, until with_status_line moves it",
    ),
    (
        "baud-core/src/status.rs",
        "pub(crate) lent_used: &'b mut [RowExtent],",
        "StatusBuf: the lent row's extent",
    ),
    (
        "baud-core/src/status.rs",
        "pub fn new(cells: &'b mut [PackedCell], used: &'b mut [RowExtent]) -> StatusBuf<'b> {",
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
                    // stamped: `RowMut` and `MarksMut` deref to their slice.
                    let token = head.trim_start().starts_with("fn deref_mut(&mut self)")
                        && [" for RowMut<", " for MarksMut<"]
                            .iter()
                            .any(|t| last_impl.contains(t));
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
/// row's generation is stored by the stamp alone (`stamp_new`, and
/// `stamp_all` for every row); the store's own slices of them only by a seal,
/// and lent out by the two methods that mint a `Stamped`, each needing
/// `&mut SurfaceStore`, which no live `Stamped` leaves to take. The row a
/// borrow stamped last is set only by `stamp_new`, after the store, and at
/// the mint ([`STAMPED_INIT_IN`]): a hit on it is sound only while that row
/// holds `now`. A surface's last write has one writer, the `Stamped`'s
/// `Drop`, beside the seal's wrap and the mint's borrow of it.
const D_WRITERS: &[(&str, &[&str])] = &[
    ("gens", &["stamp_new", "stamp_all"]),
    ("prim_gens", &["seal", "split", "stamped"]),
    ("alt_gens", &["seal", "split", "stamped"]),
    ("status_gens", &["seal", "split", "stamped"]),
    ("stamped", &["stamp_new"]),
    ("last", &["drop", "seal", "of"]),
];

/// The one fn whose struct literal may set the row stamped last.
const STAMPED_INIT_IN: &str = "of";

/// The token a one-character writer stores through, and the one fn that
/// builds it.
const TOKEN: (&str, &str) = ("RowWrite", "row_at");

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

/// Whether `code` sets the field `id` in a struct literal: `id: ..`, the
/// name standing alone, not a path (`id::`) or a field read (`.id`).
fn inits(code: &str, id: &str) -> bool {
    let b = code.as_bytes();
    let mut from = 0;
    while let Some(k) = code[from..].find(id) {
        let at = from + k;
        from = at + 1;
        let end = at + id.len();
        if (at > 0 && (is_ident(b[at - 1]) || b[at - 1] == b'.'))
            || (end < b.len() && is_ident(b[end]))
        {
            continue;
        }
        let rest = code[end..].trim_start();
        if rest.starts_with(':') && !rest.starts_with("::") {
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

/// **Rule D over `baud-domain`'s lines**: who may write a row's
/// generation, the row a borrow stamped last, and a view's width.
///
/// * In [`HOME`], outside its tests, each field of [`D_WRITERS`] is written
///   only inside its listed fns; `stamped: ..` in a struct literal only
///   inside [`STAMPED_INIT_IN`]; in `stamp_new`, `stamped` only after the
///   generation's store; the [`TOKEN`] built only in its minting fn.
/// * In the `View`'s module, a `cols` field is written only in
///   `set_geometry`, after an `assert!` in it, and a `View {` literal is
///   built only in a fn `new`, after an `assert!` in that fn: a column at
///   or past the pitch would store into the next row, which the token does
///   not check.
/// * The control: the field `stamped`, the fns `stamp_new`, `row_at` and
///   `set_geometry` and the token's struct must each be found, so a rename
///   that took the rule's subject away fails it rather than passing it.
fn rule_d(lines: &[Line]) -> Vec<String> {
    let mut bad = Vec::new();
    let (mut stack, mut file): (Vec<String>, &str) = (Vec::new(), "");
    let mut found = [false; 5];
    let mut stored_in: Option<String> = None;
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
            found[0] |= f.is_none() && t.starts_with("stamped: usize");
            found[1] |= spells(t, "fn stamp_new");
            found[2] |= spells(t, &format!("fn {}", TOKEN.1));
            found[4] |= spells(t, &format!("pub struct {}", TOKEN.0));
            for (id, fns) in D_WRITERS {
                if writes(t, id) && !fns.iter().any(|n| in_fn(n)) {
                    bad.push(format!("Rule D ({id} written in {f:?}): {}", at()));
                }
            }
            if f.is_some() && inits(t, "stamped") && !in_fn(STAMPED_INIT_IN) {
                bad.push(format!("Rule D (stamped built in {f:?}): {}", at()));
            }
            if in_fn("stamp_new") {
                if writes(t, "gens") {
                    stored_in = header.clone();
                }
                if writes(t, "stamped") && (stored_in.is_none() || stored_in != header) {
                    bad.push(format!("Rule D (stamped before the store): {}", at()));
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
                if !in_fn("new") {
                    bad.push(format!("Rule D (View built in {f:?}): {}", at()));
                } else if !checked {
                    bad.push(format!("Rule D (View built before the assert): {}", at()));
                }
            }
        }
    }
    let names = [
        "the field `stamped`",
        "fn stamp_new",
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
/// does not own it, is reported, on the line it is on; the listed fns' own
/// writes are not, nor the stamp's test, which reads `stamped` and calls
/// `stamp_new` behind its cold hint; and a file that lost the rule's
/// subjects fails its control.
#[test]
fn rule_d_fires_on_what_it_names() {
    assert!(writes("self.gens[p] = RowGen(self.now.0);", "gens"));
    assert!(writes("self.gens.fill(now);", "gens"));
    assert!(writes("let g = &mut self.gens;", "gens"));
    assert!(writes("let g = &mut *self.gens;", "gens"));
    assert!(writes("self.stamped = p;", "stamped"));
    assert!(writes("self.stamped += 1;", "stamped"));
    assert!(writes("*self.last = self.now;", "last"));
    assert!(writes("self.cols = cols;", "cols"));
    assert!(!writes("if p != self.stamped {", "stamped"));
    assert!(!writes("self.gens.get(p) == x", "gens"));
    assert!(!writes("let cols = self.cols.index();", "cols"));
    assert!(!writes("let gens = x;", "gens"));
    assert!(!writes("self.prim_gens.fill(z);", "gens"));
    assert!(builds("RowWrite { cells, mark, base, pitch }", "RowWrite"));
    assert!(!builds("pub struct RowWrite<'s> {", "RowWrite"));
    assert!(!builds("impl RowWrite<'_> {", "RowWrite"));
    assert!(builds("        View {", "View"));
    assert!(!builds("ViewRef {", "View"));
    assert!(inits("Stamped { stamped: 0 }", "stamped"));
    assert!(inits("    stamped: NONE_STAMPED,", "stamped"));
    assert!(!inits("if p != self.stamped {", "stamped"));
    assert!(!inits("stamped::X", "stamped"));
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
            "pub struct Stamped<'a> {",
            "    stamped: usize,",
            "}",
            "pub struct RowWrite<'s> {",
            "}",
            "impl<'a> Stamped<'a> {",
            "    fn of() -> Stamped<'a> {",
            "        Stamped { stamped: NONE_STAMPED }",
            "    }",
            "    fn stamp_new(&mut self, p: Row) {",
            "        self.gens[p] = RowGen(self.now.0);",
            "        self.stamped = p;",
            "    }",
            "    pub fn row_at(&mut self, p: Row) -> RowWrite<'_> {",
            "        RowWrite { cells, mark, base, pitch }",
            "    }",
            "    pub fn row_and_mark(&mut self, p: Row) {",
            "        self.stamped = p;",
            "        self.stamp(p);",
            "    }",
            "    pub fn mark_with(&mut self, p: Row) {",
            "        self.gens[p] = RowGen(self.now.0);",
            "        *self.last = self.now;",
            "    }",
            "    pub fn row_mut(&mut self, p: Row) {",
            "        let w = RowWrite { cells, mark, base: 0, pitch };",
            "        let s = Stamped { stamped: 0 };",
            "    }",
            "    #[inline(always)]",
            "    fn stamp(&mut self, p: Row) {",
            "        if p != self.stamped {",
            "            core::hint::cold_path();",
            "            self.stamp_new(p);",
            "        }",
            "    }",
            "}",
            "mod tests {",
            "    fn t() { st.gens[3] = RowGen::ZERO; }",
            "}",
        ],
    );
    home.extend(lines(
        "baud-domain/src/grid.rs",
        &[
            "impl<'a> View<'a> {",
            "    pub fn new(store: Stamped<'a>) -> View<'a> {",
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
            "    pub fn new(cells: &'a [T]) -> Grid<'a> {",
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
    assert!(
        bad.iter().any(
            |b| b.contains("(stamped written in Some(\"row_and_mark\"))") && b.contains(":18:")
        ),
        "{bad:#?}"
    );
    assert!(
        bad.iter()
            .any(|b| b.contains("(gens written in Some(\"mark_with\"))") && b.contains(":22:")),
        "{bad:#?}"
    );
    assert!(
        bad.iter()
            .any(|b| b.contains("(last written in Some(\"mark_with\"))") && b.contains(":23:")),
        "{bad:#?}"
    );
    assert!(
        bad.iter()
            .any(|b| b.contains("(RowWrite built in Some(\"row_mut\"))") && b.contains(":26:")),
        "{bad:#?}"
    );
    assert!(
        bad.iter()
            .any(|b| b.contains("(stamped built in Some(\"row_mut\"))") && b.contains(":27:")),
        "{bad:#?}"
    );
    assert!(
        bad.iter()
            .any(|b| b.contains("(cols before the assert)") && b.contains("grid.rs:10:")),
        "{bad:#?}"
    );
    assert!(
        bad.iter()
            .any(|b| b.contains("(cols written in Some(\"widen\"))") && b.contains("grid.rs:14:")),
        "{bad:#?}"
    );
    assert!(
        bad.iter()
            .any(|b| b.contains("(View built before the assert)") && b.contains("grid.rs:19:")),
        "{bad:#?}"
    );
    assert_eq!(
        bad.len(),
        8,
        "the listed writers' own writes are not reported: {at:#?}"
    );
    // In `stamp_new`, the row stamped last after the store.
    let swapped = lines(
        HOME,
        &[
            "    fn stamp_new(&mut self, p: Row) {",
            "        self.stamped = p;",
            "        self.gens[p] = RowGen(self.now.0);",
            "    }",
        ],
    );
    let bad = rule_d(&swapped);
    assert!(
        bad.iter()
            .any(|b| b.contains("(stamped before the store)") && b.contains(":2:")),
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
