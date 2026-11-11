//! **Every test binary that makes a Metal device runs in nextest's `metal`
//! group**, two tests at a time (`.config/nextest.toml`).
//!
//! Each test of such a binary is a process of its own under nextest, with a
//! device and a command queue of its own, so a suite unbounded here starts as
//! many GPU clients at once as the machine has test threads. This reads every
//! workspace member's test sources for the calls that make a device, and the
//! nextest configuration for the group each of those binaries lands in: the
//! first override that names the binary and sets a group must set `metal`,
//! since nextest gives a test the group of the first override that matches.
//!
//! A window makes one too: a test binary run with an interposer on
//! `MTLCopyAllDevices` and `IOServiceOpen` showed AppKit opening the GPU
//! (`AGXAccelerator`) for `on_screen`'s, `about_window`'s and
//! `titlebar_mark`'s windows, never shown, and for none of the binaries that
//! make only views, layers or IOSurfaces (`crossings`, `deferred_link`,
//! `present`, `settle`, `cpu_copy`).
//!
//! What it reads: the integration tests (`tests/`) and the modules they
//! declare, `#[path]` ones included, and every `src/` file, where a call that
//! makes a device must sit in one of [`MAKERS_IN_SRC`]'s functions, so a unit
//! test or a new wrapper that makes one is red here until it is named. What
//! it does not: a unit test that reaches a device through a chain of calls
//! (the app's devices and windows are made by `Boot::start_probe`,
//! `App::present` and `Opening::new`, which run in a launched app), a
//! `Default::default()` that names no type, and benches, which nextest does
//! not run.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The group, as `.config/nextest.toml` names it.
const GROUP: &str = "metal";
/// Its `max-threads`.
const MAX_THREADS: i64 = 2;

/// **The calls that make a device**, or are handed one only such a call
/// made: Metal's two, `baud-present-macos`'s wrappers over them, and the
/// window and its two wrappers.
const MAKERS: &[&str] = &[
    "MTLCreateSystemDefaultDevice(",
    "MTLCopyAllDevices(",
    "Gpu::new(",
    "SuiteBuilds::new(",
    "SuiteBuilds::default(",
    "suite_misses(",
    "Presenter::with_gpu(",
    "Presenter::over(",
    "GlassWindow::new(",
    "About::new(",
    "about::build(",
];

/// **The app, started by a test.** A launched app makes its device on its
/// probe thread (`Boot::start_probe`) unless `BAUD_GPU` asks for the CPU.
const APP: &str = "env!(\"CARGO_BIN_EXE_baud\")";

/// Binaries that start the app and are not held to the group, each with why
/// the app they start makes no device.
const STARTS_NO_DEVICE: &[(&str, &str, &str)] = &[(
    "baud",
    "credits",
    "it runs `baud --credits` and `baud --version`, which `main` answers and \
     returns from before the app is started (baud-app/src/main.rs)",
)];

/// This file, which names every maker it looks for.
const THIS: (&str, &str) = ("xtask", "metal_group");

/// **The functions outside tests that make a device**, by file: the wrappers
/// themselves, and the app's places. Any other function in a `src/` file
/// that makes one is red here, a unit test's included.
const MAKERS_IN_SRC: &[(&str, &[&str])] = &[
    // `Gpu::new`, `SuiteBuilds::new`, its `Default` and `suite_misses`.
    (
        "baud-present-macos/src/metal.rs",
        &["new", "default", "suite_misses"],
    ),
    ("baud-present-macos/src/lib.rs", &["with_gpu"]),
    // `About::new`, its window.
    ("baud-present-macos/src/about.rs", &["new"]),
    ("baud-app/src/about.rs", &["build"]),
    // The probe, the presenter, and `Opening::new`, the launch's window.
    ("baud-app/src/app.rs", &["start_probe", "present", "new"]),
];

/// The binaries this check must find, its positive control: a scan that
/// matched nothing would otherwise pass.
const KNOWN: &[(&str, &str)] = &[
    ("baud", "about_window"),
    ("baud", "event_rig"),
    ("baud", "titlebar_mark"),
    ("baud-present-macos", "cells"),
    ("baud-present-macos", "metallib"),
    ("baud-present-macos", "on_screen"),
    ("baud-present-macos", "shader_cache_fault"),
    ("baud-present-macos", "suite_archive"),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("the repository")
}

fn toml_of(path: &Path) -> toml::Table {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.parse()
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A source with every `//` comment taken out, so a call named in a doc
/// comment is not a call.
fn code_of(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .map(|l| l.find("//").map_or(l, |at| &l[..at]))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rust_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files_under(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// A workspace member: its folder and its package's name.
struct Member {
    dir: PathBuf,
    package: String,
}

fn members(root: &Path) -> Vec<Member> {
    let manifest = toml_of(&root.join("Cargo.toml"));
    let list = manifest["workspace"]["members"]
        .as_array()
        .expect("the workspace's members");
    list.iter()
        .map(|m| {
            let dir = root.join(m.as_str().expect("a member's path"));
            let package = toml_of(&dir.join("Cargo.toml"))["package"]["name"]
                .as_str()
                .expect("a package name")
                .to_owned();
            Member { dir, package }
        })
        .collect()
}

/// A member's integration-test binaries: each name, and the files it is
/// built from, its own and those of the folders it declares as modules.
fn test_binaries(member: &Member) -> Vec<(String, Vec<PathBuf>)> {
    let tests = member.dir.join("tests");
    let manifest = toml_of(&member.dir.join("Cargo.toml"));
    // `[[test]]` entries that name a binary other than its file's stem.
    let named: Vec<(PathBuf, String)> = manifest
        .get("test")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_owned();
            let path = t.get("path")?.as_str()?;
            Some((member.dir.join(path), name))
        })
        .collect();
    let Ok(entries) = fs::read_dir(&tests) else {
        return Vec::new();
    };
    let mut mains = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "rs") {
            let stem = p.file_stem().and_then(|s| s.to_str()).expect("a name");
            mains.push((stem.to_owned(), p));
        } else if p.join("main.rs").is_file() {
            let stem = p.file_name().and_then(|s| s.to_str()).expect("a name");
            mains.push((stem.to_owned(), p.join("main.rs")));
        }
    }
    mains.sort();
    mains
        .into_iter()
        .map(|(stem, main)| {
            let name = named
                .iter()
                .find(|(path, _)| *path == main)
                .map_or(stem, |(_, n)| n.clone());
            let mut files = vec![main.clone()];
            let here = main.parent().expect("a folder").to_path_buf();
            let code = code_of(&main);
            // A `#[path = "..."]` on the line before a `mod` names its file.
            let mut path_attr: Option<String> = None;
            for line in code.lines() {
                let line = line.trim();
                if let Some(p) = line
                    .strip_prefix("#[path = \"")
                    .and_then(|r| r.strip_suffix("\"]"))
                {
                    path_attr = Some(p.to_owned());
                    continue;
                }
                let attr = path_attr.take();
                let Some(rest) = line
                    .strip_prefix("mod ")
                    .or_else(|| line.strip_prefix("pub mod "))
                else {
                    continue;
                };
                let Some(module) = rest.strip_suffix(';') else {
                    continue;
                };
                if let Some(p) = attr {
                    files.push(here.join(p));
                    continue;
                }
                let file = here.join(format!("{module}.rs"));
                if file.is_file() && file != main {
                    files.push(file);
                }
                rust_files_under(&here.join(module), &mut files);
            }
            (name, files)
        })
        .collect()
}

/// What in `code` makes a device: the makers it calls, and the app if it
/// starts it.
fn sites_in(code: &str) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = MAKERS
        .iter()
        .copied()
        .filter(|m| code.contains(m))
        .collect();
    if code.contains(APP) {
        found.push(APP);
    }
    found
}

/// **Every binary whose tests make a device**, by package and name, with
/// the sites found in it.
fn device_binaries(root: &Path) -> Vec<(String, String, Vec<&'static str>)> {
    let mut out = Vec::new();
    for member in members(root) {
        for (name, files) in test_binaries(&member) {
            if (member.package.as_str(), name.as_str()) == THIS {
                continue;
            }
            let mut sites = BTreeSet::new();
            for f in &files {
                sites.extend(sites_in(&code_of(f)));
            }
            let exempt = STARTS_NO_DEVICE
                .iter()
                .any(|(p, b, _)| *p == member.package && *b == name);
            if exempt {
                sites.remove(APP);
            }
            if !sites.is_empty() {
                out.push((member.package.clone(), name, sites.into_iter().collect()));
            }
        }
    }
    out
}

/// One `[[profile.default.overrides]]` entry.
struct Override {
    filter: String,
    group: Option<String>,
}

fn overrides(root: &Path) -> (toml::Table, Vec<Override>) {
    let config = toml_of(&root.join(".config/nextest.toml"));
    let list = config["profile"]["default"]
        .get("overrides")
        .and_then(|o| o.as_array())
        .cloned()
        .unwrap_or_default();
    let list = list
        .iter()
        .map(|o| Override {
            filter: o["filter"].as_str().expect("a filter").to_owned(),
            group: o
                .get("test-group")
                .and_then(|g| g.as_str())
                .map(str::to_owned),
        })
        .collect();
    (config, list)
}

/// Whether `filter` names `package` and `binary`, as the file spells them.
fn names(filter: &str, package: &str, binary: &str) -> bool {
    filter.contains(&format!("package({package})")) && filter.contains(&format!("binary({binary})"))
}

/// **The scan finds the binaries known to make a device**, so a scan that
/// matches nothing cannot pass the check below.
#[test]
fn the_scan_finds_the_known_device_binaries() {
    let found: BTreeSet<(String, String)> = device_binaries(&root())
        .into_iter()
        .map(|(p, b, _)| (p, b))
        .collect();
    for (package, binary) in KNOWN {
        assert!(
            found.contains(&((*package).to_owned(), (*binary).to_owned())),
            "the scan did not find {package}'s {binary}: {found:?}"
        );
    }
}

/// **Each binary that makes a device lands in the `metal` group**: the
/// first override that names it and sets a group sets `metal`, and the
/// group is two tests wide.
#[test]
fn every_test_binary_that_makes_a_device_is_in_the_metal_group() {
    let root = root();
    let (config, overrides) = overrides(&root);
    let mut outside = Vec::new();
    for (package, binary, sites) in device_binaries(&root) {
        let first = overrides
            .iter()
            .find(|o| o.group.is_some() && names(&o.filter, &package, &binary));
        let group = first.and_then(|o| o.group.as_deref());
        if group != Some(GROUP) {
            outside.push(format!(
                "{package}'s {binary} ({sites:?}) lands in {group:?}, not `{GROUP}`"
            ));
        }
    }
    assert!(
        outside.is_empty(),
        "test binaries that make a Metal device outside the `{GROUP}` group of \
         .config/nextest.toml:\n{}",
        outside.join("\n")
    );
    let width = config
        .get("test-groups")
        .and_then(|g| g.get(GROUP))
        .and_then(|g| g.get("max-threads"))
        .and_then(toml::Value::as_integer);
    assert_eq!(
        width,
        Some(MAX_THREADS),
        "`[test-groups]` has no `{GROUP} = {{ max-threads = {MAX_THREADS} }}`"
    );
}

/// **A call that makes a device, outside a test, is in a function named
/// here**, so a new wrapper or a unit test that makes one is seen.
#[test]
fn every_device_made_in_src_is_in_a_named_function() {
    let root = root();
    let mut stray = Vec::new();
    for member in members(&root) {
        let mut files = Vec::new();
        rust_files_under(&member.dir.join("src"), &mut files);
        for file in files {
            let rel = file
                .strip_prefix(&root)
                .expect("under the root")
                .to_string_lossy()
                .into_owned();
            let allowed = MAKERS_IN_SRC
                .iter()
                .find(|(f, _)| *f == rel)
                .map_or(&[][..], |(_, fns)| *fns);
            let mut current = String::new();
            for (n, line) in code_of(&file).lines().enumerate() {
                if let Some(name) = fn_name(line) {
                    current = name;
                }
                if sites_in(line).iter().any(|s| *s != APP) && !allowed.contains(&current.as_str())
                {
                    stray.push(format!("{rel}:{}: in fn {current}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        stray.is_empty(),
        "devices made outside the functions this check names; name the function in \
         MAKERS_IN_SRC and put every test binary that reaches it in the `{GROUP}` group:\n{}",
        stray.join("\n")
    );
}

/// The name a line declares a function of, if it does.
fn fn_name(line: &str) -> Option<String> {
    let at = line.find("fn ")?;
    let before = line[..at].trim();
    let plain = before.is_empty()
        || before.split_whitespace().all(|w| {
            matches!(w, "pub" | "const" | "unsafe" | "async" | "extern") || w.starts_with("pub(")
        });
    if !plain {
        return None;
    }
    let rest = &line[at + 3..];
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    (end > 0).then(|| rest[..end].to_owned())
}
