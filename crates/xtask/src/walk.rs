//! The tree walks the audit's shell form ran through `find`, a glob and
//! `grep -r`, each with the rules it had.

use std::fs;
use std::path::Path;

use regex::Regex;

/// How far below a walk's directory an entry is: its children are at 1.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Depth(pub usize);

impl Depth {
    fn below(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Every entry named `*.rs` under `dir`, at `mindepth` or deeper, as
/// `find <dir> -mindepth <n> -name '*.rs' | sort` listed them: symbolic
/// links are listed and not followed, a missing `dir` lists nothing, and
/// the order is byte order.
pub fn find_rs(root: &Path, dir: &str, mindepth: Depth) -> Vec<String> {
    let mut out = Vec::new();
    descend(root, dir, Depth(1), mindepth, &mut out);
    out.sort();
    out
}

fn descend(root: &Path, dir: &str, depth: Depth, mindepth: Depth, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root.join(dir)) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = format!("{dir}/{name}");
        if name.ends_with(".rs") && depth >= mindepth {
            out.push(path.clone());
        }
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            descend(root, &path, depth.below(), mindepth, out);
        }
    }
}

/// `dir/*.rs` as the shell expands it: no dot files, in byte order, and
/// the pattern itself when nothing matches.
pub fn glob_rs(root: &Path, dir: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join(dir))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.') && n.ends_with(".rs"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    if names.is_empty() {
        return vec![format!("{dir}/*.rs")];
    }
    names.into_iter().map(|n| format!("{dir}/{n}")).collect()
}

/// Which files a `grep -r` reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Files {
    /// Every file, as a bare `-r`.
    All,
    /// Only `*.rs`, as `--include='*.rs'`.
    Rust,
}

/// One matching line, as `grep -rn` prints it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hit {
    /// The file, under the directory as it was named.
    pub path: String,
    /// The line number, from 1.
    pub line: usize,
    /// The line, without its newline.
    pub text: String,
}

/// Every line under `dir` that `re` matches, as GNU `grep -rE` reads them:
/// line by line, symbolic links met on the way skipped, a file with a NUL
/// byte or invalid UTF-8 treated as binary and printing nothing, and the
/// files in the order the directory lists them, unsorted.
pub fn grep(root: &Path, dir: &str, files: Files, re: &Regex) -> Vec<Hit> {
    let mut out = Vec::new();
    grep_dir(root, dir.trim_end_matches('/'), files, re, &mut out);
    out
}

fn grep_dir(root: &Path, dir: &str, files: Files, re: &Regex, out: &mut Vec<Hit>) {
    let Ok(entries) = fs::read_dir(root.join(dir)) else {
        return;
    };
    for e in entries.flatten() {
        let Ok(t) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        let path = format!("{dir}/{name}");
        if t.is_dir() {
            grep_dir(root, &path, files, re, out);
        } else if t.is_file() && (files == Files::All || name.ends_with(".rs")) {
            let Ok(bytes) = fs::read(root.join(&path)) else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            for (n, line) in text.split_terminator('\n').enumerate() {
                if re.is_match(line) {
                    out.push(Hit {
                        path: path.clone(),
                        line: n + 1,
                        text: line.to_string(),
                    });
                }
            }
        }
    }
}
