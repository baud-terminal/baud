//! **What the source must say about committing GPU work**, read from the
//! source: every command buffer is made, committed and heard from in one
//! module (`src/metal/commit.rs`), which reads its status on every
//! completion, and the seam that fakes a failing or a held completion is
//! turned on only by dev-dependencies.
//!
//! No GPU is touched: these read files.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

/// The workspace root, above this crate.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace")
        .to_path_buf()
}

/// Every `.rs` file under `dirs`, with its text.
fn sources(dirs: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("a source folder").flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).expect("a source file");
                out.push((p, text));
            }
        }
    }
    out
}

/// The lines of `text` that are code: a line whose first characters are a
/// comment says what the code does and calls nothing.
fn code(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
        .map(|(i, l)| (i + 1, l))
}

/// Whether `line` calls `.commit()` on a receiver: an identifier or a
/// closing bracket before the dot, so a string that spells the call is
/// not one.
fn calls_commit(line: &str) -> bool {
    let call = concat!(".com", "mit()");
    line.match_indices(call).any(|(at, _)| {
        line[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ')' || c == ']')
    })
}

/// **Only the commit module commits, waits on or hears from the GPU**:
/// a command buffer is made, committed, given a handler and has its status
/// and stamps read only in `src/metal/commit.rs`, which reads the status
/// on every completion; nothing anywhere waits on one without a bound
/// (`waitUntilCompleted` takes none); and no loop spins on a frame in
/// flight, whose end only the commit module may wait for. Every Rust file
/// of this crate, the app and the benches is read.
#[test]
fn only_the_commit_module_commits_or_waits_on_the_gpu() {
    let r = root();
    let dirs = [
        r.join("baud-present-macos/src"),
        r.join("baud-app/src"),
        r.join("baud-bench/src"),
        r.join("baud-bench/benches"),
    ];
    // Spelled in pieces, so this file does not match itself.
    let confined = [
        concat!("add", "CompletedHandler"),
        concat!("add", "ScheduledHandler"),
        concat!("wait", "UntilScheduled"),
        concat!(".GPU", "StartTime("),
        concat!(".GPU", "EndTime("),
        concat!("MTLCommand", "BufferStatus"),
        concat!("command", "Buffer()"),
        concat!("command", "BufferWithDescriptor"),
    ];
    let nowhere = concat!("wait", "UntilCompleted");
    let spin = concat!("in_flight() == ", "InFlight::Busy");
    let files = sources(&dirs);
    let mut found = Vec::new();
    let mut module = None;
    for (p, text) in &files {
        let ours = p.ends_with("src/metal/commit.rs");
        if ours {
            module = Some(text.as_str());
        }
        for (n, line) in code(text) {
            let at = || format!("{}:{n}", p.display());
            if line.contains(nowhere) {
                found.push(format!("{}: {nowhere}", at()));
            }
            if line.trim_start().starts_with("while ") && line.contains(spin) {
                found.push(format!("{}: a spin on a frame in flight", at()));
            }
            if ours {
                continue;
            }
            if calls_commit(line) {
                found.push(format!("{}: .commit()", at()));
            }
            for t in confined {
                if line.contains(t) {
                    found.push(format!("{}: {t}", at()));
                }
            }
        }
    }
    assert!(files.len() > 100, "only {} source files read", files.len());
    assert!(
        found.is_empty(),
        "GPU commits, waits or spins outside src/metal/commit.rs:\n{}",
        found.join("\n")
    );
    // A positive control: the module was read, and the patterns find what
    // it does.
    let module = module.expect("src/metal/commit.rs was read");
    let lines: Vec<&str> = code(module).map(|(_, l)| l).collect();
    assert!(
        lines.iter().any(|l| calls_commit(l)),
        "the commit module commits nothing the pattern sees"
    );
    for t in [
        concat!("add", "CompletedHandler"),
        concat!(".GPU", "StartTime("),
        concat!("command", "Buffer"),
        concat!(".sta", "tus()"),
        concat!(".err", "or()"),
    ] {
        assert!(
            lines.iter().any(|l| l.contains(t)),
            "the commit module never spells {t}"
        );
    }
}

/// **No build that ships has the fault seam**: `gpu-fault` is turned on
/// only in a `dev-dependencies` section and declared only in this crate's
/// `[features]`, as `suite-archive` is. Every member's manifest is read.
#[test]
fn only_dev_dependencies_turn_gpu_fault_on() {
    let root = root();
    let mut read = 0;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&root).expect("the workspace").flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("Cargo.toml")) else {
            continue;
        };
        read += 1;
        let member = entry.file_name().to_string_lossy().into_owned();
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                section = line.to_string();
                continue;
            }
            if line.starts_with('#') || !line.contains("gpu-fault") {
                continue;
            }
            if member == "baud-present-macos"
                && section == "[features]"
                && line.starts_with("gpu-fault =")
            {
                continue;
            }
            found.push((member.clone(), section.clone(), line.to_string()));
        }
    }
    assert!(
        read > 10,
        "only {read} manifests read under {}",
        root.display()
    );
    // A positive control: the crate whose tests inject faults was read.
    assert!(
        found.iter().any(|(m, _, _)| m == "baud-present-macos"),
        "baud-present-macos turns the fault seam on for its tests: {found:?}"
    );
    let shipped: Vec<_> = found
        .iter()
        .filter(|(_, section, _)| !section.contains("dev-dependencies"))
        .collect();
    assert!(
        shipped.is_empty(),
        "the fault seam turned on outside dev-dependencies, so a shipped build has it: \
         {shipped:?}"
    );
}
