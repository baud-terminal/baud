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
/// (`waitUntilCompleted` takes none, nor does a bare `recv()` on the
/// handler's answer, nor the unbounded idle wait this replaced); and no
/// loop spins on a frame in flight, whose end only the commit module may
/// wait for, within a bound. Every Rust file of this crate, the app and
/// the benches is read. Breaks: a bare `recv()` back in the waiter; a
/// bench spinning on `in_flight() == InFlight::Busy`.
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
    let nowhere = [concat!("wait", "UntilCompleted"), concat!("wait", "_idle(")];
    // The handler's answer, waited for with no bound: other modules' own
    // channels are not the GPU's.
    let unbounded = concat!(".re", "cv()");
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
            for t in nowhere {
                if line.contains(t) {
                    found.push(format!("{}: {t}", at()));
                }
            }
            if line.trim_start().starts_with("while ") && line.contains(spin) {
                found.push(format!("{}: a spin on a frame in flight", at()));
            }
            if ours {
                if line.contains(unbounded) {
                    found.push(format!("{}: {unbounded}", at()));
                }
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
        concat!(".recv_", "timeout("),
        concat!("fn idle_", "within("),
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

/// The source of `rel`, under this crate.
fn source(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).expect("a source")
}

/// The text of the item that starts at `head`, through its closing brace at
/// the indent `head` is written at.
fn item<'a>(text: &'a str, head: &str) -> &'a str {
    let at = text
        .find(head)
        .unwrap_or_else(|| panic!("{head} is in the source"));
    let indent = text[..at].rsplit('\n').next().unwrap_or("").len();
    let close = format!("\n{}}}", " ".repeat(indent));
    let rest = &text[at..];
    let end = rest.find(&close).unwrap_or_else(|| panic!("{head} ends"));
    &rest[..end + close.len()]
}

/// The identifiers in `text`.
fn idents(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
}

/// **Only the latch sets the GPU given up, and nothing reaches a given-up
/// `Gpu`** (S1): in `src/metal/commit.rs` the latch word `gave_up` is
/// written in `Counters::give_up` alone, and only by a compare from live,
/// so the first reason wins and nothing stores live back; in `src/lib.rs`
/// no method of `Parked` returns a type that names `Gpu` or `Target`, and
/// a `Draw::Gpu` is built only in `Presenter::over` and `Presenter::build`,
/// so nothing makes the GPU path again from a parked one. Breaks: a store
/// to the latch in the bound's wake; a `fn gpu(&self) -> &Gpu` on
/// `Parked`.
#[test]
fn only_the_latch_sets_the_gpu_use() {
    let commit = source("src/metal/commit.rs");
    let give_up = item(&commit, "    fn give_up(&self, why: GaveUp) -> bool {");
    assert!(
        give_up.contains(".compare_exchange(") && give_up.contains("UseWord::LIVE.0,"),
        "the latch moves by a compare from live: {give_up}"
    );
    // A write is the latch's when its receiver is: on the line, or, for a
    // call rustfmt put on a line of its own, ending the line before.
    let mut writes = Vec::new();
    let mut before = "";
    for (n, line) in code(&commit) {
        let written = ["store(", "swap(", "fetch_", "compare_exchange"]
            .iter()
            .any(|w| line.contains(w));
        let latch = line.contains("gave_up.")
            || (line.trim_start().starts_with('.') && before.trim_end().ends_with("gave_up"));
        if written && latch {
            writes.push((n, line.trim()));
        }
        before = line;
    }
    // The latch's one write: its receiver on one line and the compare on
    // the next, inside `give_up`.
    let at = commit.find(give_up).expect("give_up in the file");
    let first = commit[..at].lines().count() + 1;
    let last = first + give_up.lines().count();
    let outside: Vec<_> = writes
        .iter()
        .filter(|(n, _)| *n < first || *n > last)
        .collect();
    assert!(
        outside.is_empty(),
        "the latch is written outside Counters::give_up: {outside:?}"
    );
    assert!(
        !writes.is_empty(),
        "the control: the write in give_up is seen"
    );

    let lib = source("src/lib.rs");
    let parked = item(&lib, "impl Parked {");
    let mut handed = Vec::new();
    for line in parked.lines().filter(|l| l.contains("fn ")) {
        if let Some((_, ret)) = line.split_once("->") {
            if idents(ret).any(|w| w == "Gpu" || w == "Target") {
                handed.push(line.trim());
            }
        }
    }
    assert!(
        parked.lines().filter(|l| l.contains("fn ")).count() >= 3,
        "the control: Parked's methods are read: {parked}"
    );
    assert!(handed.is_empty(), "Parked hands out its Gpu: {handed:?}");

    // `Draw::Gpu { .. }` written as a value, not a pattern: a pattern is
    // followed by `=>` or `|`, or is a `let` pattern.
    let mut built = Vec::new();
    let mut at = 0;
    let head = concat!("Draw::", "Gpu {");
    while let Some(i) = lib[at..].find(head) {
        let start = at + i;
        let close = start + lib[start..].find('}').expect("closed");
        let after = lib[close + 1..].trim_start();
        let line_start = lib[..start].rfind('\n').map_or(0, |n| n + 1);
        let before = lib[line_start..start].trim_start();
        let pattern = after.starts_with("=>")
            || after.starts_with('|')
            || after.starts_with('=')
            || before.starts_with("let ")
            || before.starts_with("if let ");
        if !pattern {
            // The function it is in: the nearest line above that opens one.
            let name = lib[..start]
                .lines()
                .rev()
                .find_map(|l| {
                    let l = l.trim_start();
                    let l = l.strip_prefix("pub ").unwrap_or(l);
                    l.strip_prefix("fn ")
                })
                .and_then(|rest| {
                    rest.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .next()
                })
                .expect("in a function");
            built.push(name.to_owned());
        }
        at = close;
    }
    assert!(
        built.iter().any(|f| f == "over") && built.iter().any(|f| f == "build"),
        "the control: over and build make one: {built:?}"
    );
    let stray: Vec<_> = built
        .iter()
        .filter(|f| *f != "over" && *f != "build")
        .collect();
    assert!(stray.is_empty(), "a Draw::Gpu built in {stray:?}");
}

/// **The give-up is latched before the frame is counted out** (S3): in
/// `Encoded::commit`'s deliver closure the call to `give_up` comes before
/// the count-out's `fetch_sub`, so a draw that sees no frame in flight has
/// acquired the latch and refuses to commit after the frame that gave the
/// GPU up. A row could show the wrong order only on some runs; the source
/// shows it on every one. Break: the two swapped.
#[test]
fn the_give_up_is_latched_before_the_frame_is_counted_out() {
    let commit = source("src/metal/commit.rs");
    let deliver = commit
        .split_once("let deliver = {")
        .and_then(|(_, rest)| rest.split_once("seam.delivered();"))
        .map(|(body, _)| body)
        .expect("the deliver closure");
    let latch = deliver
        .find(concat!("counters.give_", "up("))
        .expect("the closure gives the GPU up");
    let out = deliver
        .find(concat!(".fetch_", "sub("))
        .expect("the closure counts the frame out");
    assert!(latch < out, "the count-out comes before the latch");
}
