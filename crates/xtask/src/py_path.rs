//! `os.path` as the gates this crate replaced called it: strings in,
//! strings out, each function the same steps as `posixpath`'s, so a path
//! printed by a gate reads as it did.

use std::collections::HashMap;
use std::env;
use std::fs;

/// `os.path.join(a, b)`.
pub fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.normpath(p)`: no filesystem is read, and exactly two leading
/// slashes stay two.
pub fn normpath(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let lead = if p.starts_with("//") && !p.starts_with("///") {
        "//"
    } else if p.starts_with('/') {
        "/"
    } else {
        ""
    };
    let mut kept: Vec<&str> = Vec::new();
    for comp in p.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp != ".." || (lead.is_empty() && kept.is_empty()) || kept.last() == Some(&"..") {
            kept.push(comp);
        } else {
            kept.pop();
        }
    }
    let out = format!("{lead}{}", kept.join("/"));
    if out.is_empty() { ".".to_string() } else { out }
}

/// `os.path.dirname(p)`.
pub fn dirname(p: &str) -> String {
    let head = p.rfind('/').map_or("", |i| &p[..=i]);
    if head.chars().all(|c| c == '/') {
        head.to_string()
    } else {
        head.trim_end_matches('/').to_string()
    }
}

/// `os.path.basename(p)`.
pub fn basename(p: &str) -> &str {
    p.rfind('/').map_or(p, |i| &p[i + 1..])
}

/// `os.path.splitext(p)[0]`: leading dots of the last component are not
/// an extension's.
pub fn stem(p: &str) -> &str {
    let name_at = p.rfind('/').map_or(0, |i| i + 1);
    match p.rfind('.') {
        Some(dot) if dot > name_at && !p[name_at..dot].chars().all(|c| c == '.') => &p[..dot],
        _ => p,
    }
}

fn cwd() -> String {
    env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// One entry of what `realpath` has still to walk.
enum Part {
    Name(String),
    /// The link whose target has just been walked.
    Resolved(String),
}

/// `os.path.realpath(p)`, not strict: what does not exist is kept as
/// written, and a link cycle stops at the link.
pub fn realpath(p: &str) -> String {
    let mut rest: Vec<Part> = p
        .split('/')
        .rev()
        .map(|s| Part::Name(s.to_string()))
        .collect();
    let mut path = if p.starts_with('/') {
        "/".to_string()
    } else {
        cwd()
    };
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    while let Some(part) = rest.pop() {
        let name = match part {
            Part::Resolved(link) => {
                seen.insert(link, Some(path.clone()));
                continue;
            }
            Part::Name(name) => name,
        };
        if name.is_empty() || name == "." {
            continue;
        }
        if name == ".." {
            let cut = path.rfind('/').unwrap_or(0);
            path = if cut == 0 {
                "/".to_string()
            } else {
                path[..cut].to_string()
            };
            continue;
        }
        let next = if path == "/" {
            format!("/{name}")
        } else {
            format!("{path}/{name}")
        };
        let link = fs::symlink_metadata(&next).is_ok_and(|m| m.file_type().is_symlink());
        if !link {
            path = next;
            continue;
        }
        if let Some(known) = seen.get(&next) {
            path = known.clone().unwrap_or(next);
            continue;
        }
        let Ok(target) = fs::read_link(&next) else {
            path = next;
            continue;
        };
        let target = target.to_string_lossy().into_owned();
        seen.insert(next.clone(), None);
        rest.push(Part::Resolved(next));
        if target.starts_with('/') {
            path = "/".to_string();
        }
        rest.extend(target.split('/').rev().map(|s| Part::Name(s.to_string())));
    }
    path
}

/// `os.path.relpath(path, start)`, for two absolute paths.
pub fn relpath(path: &str, start: &str) -> String {
    let parts = |p: &'_ str| -> Vec<String> {
        normpath(p)
            .split('/')
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (to, from) = (parts(path), parts(start));
    let common = to.iter().zip(&from).take_while(|(a, b)| a == b).count();
    let mut out = vec!["..".to_string(); from.len() - common];
    out.extend_from_slice(&to[common..]);
    if out.is_empty() {
        ".".to_string()
    } else {
        out.join("/")
    }
}
