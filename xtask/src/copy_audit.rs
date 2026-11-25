//! **The copy functions of the pipeline, counted against a budget.** A
//! function that copies on a hop between the pty and the screen lives in a
//! module named `copy` and takes that hop's `CopyCounts` by `&mut`, so
//! every copy is named and counted where it is made. This finds every `fn`
//! in a `copy` module in the sources of the crates below the root, and
//! [`BUDGET`] is how many there are: a copy function added without the
//! budget raised in the same commit fails `xtask/tests/copy_audit.rs`.
//!
//! A `copy` module is a file named `copy.rs`, any file in a directory named
//! `copy`, or an inline `mod copy { .. }`, whose extent is found by
//! counting braces. Like the raw-integer audit, it matches text, not
//! syntax: a brace in a string inside an inline `copy` module moves its
//! end.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::feature_arms::members;
use crate::impl_audit::Sites;
use crate::walk::{Depth, find_rs};

/// The copy functions the sources hold: none yet.
pub const BUDGET: Sites = Sites(0);

/// One function found in a `copy` module.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CopyFn {
    /// The file and line, `path:line`, the path from the root.
    pub at: String,
    /// The function's name.
    pub name: String,
}

/// What a walk read and what it found.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Found {
    /// Every source file read, from the root.
    pub files: Vec<String>,
    /// Every function in a `copy` module, in file order.
    pub fns: Vec<CopyFn>,
}

impl Found {
    /// How many copy functions there are, to hold against [`BUDGET`].
    pub fn count(&self) -> Sites {
        Sites(self.fns.len())
    }
}

static FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+(?:"[^"]*"\s+)?)?fn\s+(\w+)"#,
    )
    .expect("the fn pattern")
});

static MOD_COPY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+copy\s*\{").expect("the mod pattern")
});

/// Whether the file at `path` is itself a `copy` module or inside one.
fn is_copy_file(path: &str) -> bool {
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        let last = parts.peek().is_none();
        if (part == "copy" && !last) || (part == "copy.rs" && last) {
            return true;
        }
    }
    false
}

/// The functions of `text` that are in a `copy` module, the whole file
/// being one when `path` says so.
pub fn copy_fns(path: &str, text: &str) -> Vec<CopyFn> {
    let whole = is_copy_file(path);
    let mut out = Vec::new();
    // Brace depth inside the innermost inline `mod copy`, while in one.
    let mut inside: Option<Sites> = None;
    for (n, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        let opening = inside.is_none() && MOD_COPY.is_match(code);
        if whole || inside.is_some() {
            if let Some(c) = FN.captures(code) {
                out.push(CopyFn {
                    at: format!("{path}:{}", n + 1),
                    name: c[1].to_string(),
                });
            }
        }
        if opening {
            inside = Some(Sites(0));
        }
        if let Some(depth) = inside {
            let opens = code.matches('{').count();
            let closes = code.matches('}').count();
            let level = depth.0 + opens;
            inside = level.checked_sub(closes).filter(|l| *l > 0).map(Sites);
        }
    }
    out
}

/// Every function in a `copy` module in `src/` of each crate directly
/// below `root`.
pub fn find(root: &Path) -> Found {
    let mut found = Found::default();
    for krate in members(root) {
        for file in find_rs(root, &format!("{krate}/src"), Depth(1)) {
            let Ok(bytes) = fs::read(root.join(&file)) else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            found.fns.extend(copy_fns(&file, &text));
            found.files.push(file);
        }
    }
    found
}
