//! **The headless run opens no window, starts no application and puts
//! nothing in the Dock**, proven from the source, as the launch test is
//! (`src/app/launch_tests.rs`, `the_launch_path_names_no_appkit`).
//!
//! `--headless DIR` is three processes' work: this example's, which
//! prepares and builds; two of baud's tests, run by cargo
//! (`film::session::the_readme_session_lights_every_light` and
//! `programs::chawan_draws_a_pages_picture_by_kittys_protocol`); and
//! baud-bench's flash censuses of the film and of the README's GIF cut from
//! it, which ffmpeg and gifski make. Each is held here or beside its test:
//!
//! - this example: [`the_headless_run_reaches_no_window`] walks what the
//!   headless branch reaches and finds no AppKit, no event posted, no
//!   pasteboard, and no program started but the build and fetch tools;
//!   [`nothing_in_the_demo_starts_an_application`] finds no application,
//!   window or marker named anywhere in the example at all;
//! - the censuses: [`the_flash_census_names_no_appkit`];
//! - baud's two tests: `film/session.rs`, which holds each off the main
//!   thread, where alone AppKit's marker is made, and holds every command
//!   line the session submits to programs that open no window.

use std::path::{Path, PathBuf};

use crate::no_appkit::{
    FORBIDDEN, Follow, Marked, Part, Reached, Walk, bare_code, hits, reach, without_tests,
};

/// The example's module files.
fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/demo")
}

/// Every module of the example, by its file: none listed by hand.
fn modules() -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(demo())
        .expect("the example's folder")
        .filter_map(|e| {
            let p = e.expect("an entry").path();
            let rs = p.extension().is_some_and(|x| x == "rs");
            rs.then(|| {
                p.file_stem()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .collect();
    out.sort();
    out
}

/// `main`'s call of `run`, taken out of `main`: `run` is walked as its own
/// headless branch.
const RUN_CALL: &str = "let ran = run(&args);";

/// Where `run`'s headless branch has returned: the window's session, its
/// questions to macOS first, and everything after it, taken out of `run`.
const WINDOW_PATH: &str = "let host = std::env::var(";

/// **What the headless run reaches**: `main` without its call of `run`,
/// and `run` up to [`WINDOW_PATH`], every fn of every module of the example
/// they call followed, fn by fn.
fn headless() -> Reached {
    let names = modules();
    let one: Vec<[&str; 1]> = names.iter().map(|n| [n.as_str()]).collect();
    let by_fn: Vec<(&[&str], Follow)> = one.iter().map(|m| (&m[..], Follow::AllFns)).collect();
    let s = |x: &str| x.to_string();
    reach(&Walk {
        src: &demo(),
        start: vec![
            Part::Cut(vec![s("run")], s("main"), s(RUN_CALL), s(RUN_CALL)),
            Part::Cut(vec![s("run")], s("run"), s(WINDOW_PATH), s("\n}")),
        ],
        by_fn: &by_fn,
        sinks: &[],
        marked: Marked::Cut,
    })
}

/// What the window's session does, which the headless run must reach
/// none of: AppKit's workspace and pasteboard, a posted event, raising an
/// application, starting baud, and both trust checks, the one that prompts,
/// whose dialog is a window on the screen, and `mac::trusted`, which asks
/// without one: the headless run posts no event, so it asks neither.
const CALLS: [&str; 11] = [
    "NSWorkspace",
    "NSPasteboard",
    "CGEvent",
    "osascript",
    "Poster",
    "Command::new(baud)",
    "objc2_app_kit",
    "AXIsProcessTrustedWithOptions",
    "kAXTrustedCheckOptionPrompt",
    "AXIsProcessTrusted",
    "mac::trusted",
];

/// **The programs the headless branch can start**, as each reached
/// `Command::new`, or `say`, names its program, and what each is. The walk
/// follows calls by name and both arms of every `match`, so colima's is
/// here though a run given `DOCKER_HOST` starts no VM. None is a GUI
/// application, and none is baud.
const STARTED: [(&str, &str); 19] = [
    (
        "\"/bin/hostname\"",
        "this machine's names, kept out of the film",
    ),
    (
        "\"/usr/sbin/scutil\"",
        "this machine's names, kept out of the film",
    ),
    (
        "\"/usr/sbin/sysctl\"",
        "this machine's names, kept out of the film",
    ),
    ("\"/usr/bin/curl\"", "a pinned download"),
    ("\"/usr/bin/shasum\"", "a pinned file's hash"),
    ("\"/usr/bin/tar\"", "packs the source tree"),
    ("\"colima\"", "the VM, only when no DOCKER_HOST is given"),
    ("\"docker\"", "the session's container"),
    ("\"git\"", "the source tree and its commit"),
    ("\"nix\"", "builds the pinned programs"),
    (
        "&t.ffmpeg",
        "the README GIF's frames from the session's film, a CLI",
    ),
    ("&t.gifski", "the README GIF's encoder, a CLI"),
    ("cargo", "builds baud and runs the three tests"),
    (
        "cmd",
        "machine_names' say, container's say and sha256: the literals listed",
    ),
    ("keygen", "the session's ssh-keygen, beside ssh"),
    ("starship.join(\"starship\")", "starship asked its version"),
    ("stream", "nix's image stream script, written to a file"),
    ("tar", "the pinned GNU tar, packing the image"),
    ("\"shasum\"", "a file's hash: shasum, else sha256sum"),
];

/// The text from just past `at`, an opening bracket's end, to its
/// matching close.
fn argument(text: &str) -> &str {
    let mut depth = 1;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &text[..i];
                }
            }
            _ => {}
        }
    }
    text
}

/// Each program a reached `Command::new` names, as written, and the
/// first word of each `say` and `sha256` table entry that is a literal.
fn started(reached: &Reached) -> Vec<String> {
    let mut out = Vec::new();
    for body in reached.parts.values() {
        for (at, _) in body.match_indices("Command::new(") {
            out.push(
                argument(&body[at + "Command::new(".len()..])
                    .trim()
                    .to_string(),
            );
        }
        for pat in ["say(\"", "(cmd, args) in [(\""] {
            for (at, _) in body.match_indices(pat) {
                let rest = &body[at + pat.len() - 1..];
                let word = rest.split(',').next().unwrap_or("").trim();
                out.push(word.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// **The headless branch reaches no window**: walked from `main` and from
/// `run` up to [`WINDOW_PATH`], the example's code names none of
/// [`FORBIDDEN`] and makes none of [`CALLS`], and every program it starts
/// is one of [`STARTED`]. The controls: the walk reaches the headless
/// branch, the container and the build, and does not reach the window's
/// session; and the same walk past [`WINDOW_PATH`] is caught.
#[test]
fn the_headless_run_reaches_no_window() {
    // The cut is sound only while the headless branch returns before the
    // window's session: its block ends in its return.
    let run = include_str!("run.rs");
    let branch = run
        .split_once("    if let Some(stills) = &args.headless {\n")
        .and_then(|(_, b)| b.split_once(WINDOW_PATH))
        .map(|(b, _)| b)
        .expect("the headless branch, then the window's session");
    assert!(
        branch.trim_end().ends_with("return Ok(());\n    }"),
        "the headless branch returns before the window's session: {branch}"
    );
    let reached = headless();
    let fns = |m: &str, f: &str| Part::Fn(vec![m.to_string()], f.to_string());
    for (m, f) in [
        ("run", "headless"),
        ("run", "build_baud"),
        ("run", "prepare"),
        ("container", "up"),
        ("mac", "account"),
    ] {
        assert!(
            reached.parts.contains_key(&fns(m, f)),
            "the control: {m}::{f} is reached"
        );
    }
    for (m, f) in [
        ("run", "live"),
        ("run", "play"),
        ("run", "run"),
        ("mac", "trusted"),
        ("record", "kit"),
    ] {
        assert!(
            !reached.parts.contains_key(&fns(m, f)),
            "{m}::{f} is reached"
        );
    }
    // `mac::new` is reached by its name alone, as the mac fns call other
    // types' `new`; it is the poster's, and nothing reached makes one.
    let poster = fns("mac", "new");
    let made = concat!("Poster", "::new(");
    assert!(
        include_str!("run.rs").contains(made),
        "the control: the window's session makes one"
    );
    for (p, body) in &reached.parts {
        assert!(*p == poster || !body.contains(made), "{p:?} makes a poster");
    }
    let found: Vec<String> = hits(&reached, &CALLS, &[])
        .into_iter()
        .filter(|h| !h.starts_with(&format!("{poster:?}: ")))
        .collect();
    assert_eq!(found, Vec::<String>::new());
    let mut listed: Vec<String> = STARTED.iter().map(|(p, _)| p.to_string()).collect();
    listed.sort();
    assert_eq!(started(&reached), listed);
}

/// **Nothing in the example starts an application**: comments and
/// literals aside, no file of it, on any path, names an application, a
/// window, AppKit's main-thread marker or baud's presenter, nor forces a
/// marker, turns the process into an application from C, looks a class up
/// by name or sends a raw message. Without these a process has no Dock
/// icon and no window. The control: the same words
/// in code are caught, and in a string they are not.
#[test]
fn nothing_in_the_demo_starts_an_application() {
    let bypasses = [
        concat!("new", "_unchecked"),
        concat!("TransformProcess", "Type"),
        concat!("setActivation", "Policy"),
        concat!("NSApplication", "Load"),
        concat!("objc_get", "Class"),
        concat!("objc_lookUp", "Class"),
        concat!("NSClassFrom", "String"),
        concat!("AnyClass::", "get"),
        concat!("objc_msg", "Send"),
    ];
    let caught = |text: &str| -> Vec<String> {
        without_tests(&bare_code(text))
            .lines()
            .filter(|l| FORBIDDEN.iter().chain(&bypasses).any(|b| l.contains(b)))
            .map(|l| l.trim().to_string())
            .collect()
    };
    let named = format!("let a = {}::shared();\n", FORBIDDEN[1]);
    assert_eq!(caught(&named).len(), 1, "the control: code is caught");
    assert!(
        caught(&format!("let s = {named:?};\n")).is_empty(),
        "a string is not"
    );
    let mut files: Vec<PathBuf> = modules()
        .iter()
        .map(|m| demo().join(m).with_extension("rs"))
        .collect();
    files.push(demo().with_extension("rs"));
    assert!(files.len() > 10, "the control: the files are read");
    let mut found = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).expect("a module");
        for l in caught(&text) {
            found.push(format!("{}: {l}", f.display()));
        }
    }
    assert_eq!(found, Vec::<String>::new());
}

/// The source folders a crate's build compiles: its own and, through each
/// `path` dependency that is not a dev-dependency, every first-party
/// crate under it.
fn compiled(root: &Path, krate: &str, out: &mut Vec<PathBuf>) {
    let dir = root.join(krate);
    if out.contains(&dir.join("src")) {
        return;
    }
    out.push(dir.join("src"));
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).expect("a manifest");
    let mut dev = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            dev = t.contains("dev-dependencies");
            continue;
        }
        if dev || t.starts_with('#') {
            continue;
        }
        if let Some((_, rest)) = t.split_once("path = \"../") {
            let name = rest.split('"').next().unwrap_or("");
            compiled(root, name, out);
        }
    }
}

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).expect("a source folder") {
        let p = e.expect("an entry").path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// **The flash census names no AppKit**: baud-bench's
/// `tests/session_flashes.rs` and `tests/trailer_flashes.rs`, which the
/// headless run gives the README's GIF, its library and every first-party
/// crate the library is built from, comments and literals aside, name none of
/// [`FORBIDDEN`], no objc2 crate and no window crate, so the census binary
/// calls nothing that could make an application. baud-bench's benches and
/// dev-dependencies, which draw through Metal, are not in it. The control:
/// the crates read include the renderer and the core, and a test file
/// naming the presenter is caught.
#[test]
fn the_flash_census_names_no_appkit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut dirs = Vec::new();
    compiled(&root, "baud-bench", &mut dirs);
    for c in ["baud-core", "baud-render", "baud-font", "baud-domain"] {
        assert!(
            dirs.contains(&root.join(c).join("src")),
            "the control: {c} is read"
        );
    }
    let mut files = vec![
        root.join("baud-bench/tests/session_flashes.rs"),
        root.join("baud-bench/tests/trailer_flashes.rs"),
    ];
    for d in &dirs {
        sources(d, &mut files);
    }
    let names: Vec<&str> = FORBIDDEN
        .iter()
        .copied()
        .chain([concat!("obj", "c2"), concat!("macos_liquid", "_glass")])
        .collect();
    let caught = |text: &str| -> Vec<String> {
        bare_code(text)
            .lines()
            .filter(|l| names.iter().any(|n| l.contains(n)))
            .map(|l| l.trim().to_string())
            .collect()
    };
    let presenter = format!("use {}::Window;\n", FORBIDDEN[3]);
    assert_eq!(
        caught(&presenter).len(),
        1,
        "the control: a presenter is caught"
    );
    let mut found = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).expect("a source");
        for l in caught(&text) {
            found.push(format!("{}: {l}", f.display()));
        }
    }
    assert_eq!(found, Vec::<String>::new());
}
