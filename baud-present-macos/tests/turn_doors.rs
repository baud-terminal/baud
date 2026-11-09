//! **No door to the main queue but the turn's, and each desktop call in
//! its own function.**
//!
//! An effect made from a frame, a pump or a responder method re-enters
//! the view; `turn::Turn` is the proof that one runs on a main-queue turn
//! of its own, and `turn.rs` makes every turn. A bare block put on the
//! main queue with a delay, `DispatchQueue::main().after(..)`, is a door
//! the type does not see, so it is made only in `turn::after`, and in the
//! two places that make no effect. And each AppKit call that asks the
//! desktop for the user's attention, activates the app or makes a window
//! key is made in the one function that takes a `Turn` for it, so a
//! second call site cannot skip it.
//!
//! Each source is read as code: comments, strings and every `#[cfg(test)]`
//! item blanked, wherever it sits in the file -- `app.rs` holds production
//! code after its first test module. Each call is counted in the function
//! it must be in; the tables are the positive controls.
//!
//! Perturbations: a notification's expiry put back on
//! `dispatch2::DispatchQueue::main().after(` in `baud-app/src/app.rs`; a
//! second `.makeKeyAndOrderFront(` in `baud-app/src/app.rs`.

use std::fs::{read_dir, read_to_string};
use std::path::{Path, PathBuf};

mod scan;
use scan::blanked;

/// Every Rust source under the two crates whose code asks the desktop,
/// as `(path from this crate, text)`.
fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for dir in ["src", "../baud-app/src"] {
        let mut stack = vec![root.join(dir)];
        while let Some(d) = stack.pop() {
            for entry in read_dir(&d).expect("a source directory") {
                let path: PathBuf = entry.expect("an entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let name = path
                        .strip_prefix(root)
                        .expect("under the crate")
                        .to_string_lossy()
                        .into_owned();
                    out.push((name, read_to_string(&path).expect("a source")));
                }
            }
        }
    }
    assert!(out.len() > 50, "the sources were found: {}", out.len());
    out
}

/// Whether the attribute `attr`, its spaces taken out, builds its item
/// only for tests.
fn test_only(attr: &str) -> bool {
    attr.starts_with("#[cfg(test)") || attr.starts_with("#[cfg(all(test,")
}

/// **`src` as production code**: comments, strings and characters
/// blanked ([`blanked`]), and each item an attribute builds only for
/// tests blanked whole -- from its attributes to the brace that closes
/// it, or its `;` -- byte for byte, so offsets agree with `src`.
fn production(src: &str) -> String {
    let mut code = blanked(src);
    let mut i = 0;
    while let Some(at) = find(&code, i, b"#[") {
        let Some(close) = attr_end(&code, at) else {
            break;
        };
        let attr: String = String::from_utf8_lossy(&code[at..close])
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if !test_only(&attr) {
            i = close;
            continue;
        }
        // The item: past its other attributes, to its closing brace or `;`.
        let mut j = close;
        let mut depth = 0usize;
        let mut end = code.len();
        while j < code.len() {
            match code[j] {
                b'[' | b'(' => depth += 1,
                b']' | b')' => depth = depth.saturating_sub(1),
                b'{' => {
                    end = matching(&code, j).map_or(code.len(), |e| e + 1);
                    break;
                }
                b';' if depth == 0 => {
                    end = j + 1;
                    break;
                }
                _ => {}
            }
            j += 1;
        }
        for c in &mut code[at..end] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
        i = end;
    }
    String::from_utf8_lossy(&code).into_owned()
}

/// The first `pat` in `code` from `from`.
fn find(code: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    code.get(from..)?
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| from + p)
}

/// The end of the attribute opening at `at`, past its `]`.
fn attr_end(code: &[u8], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (j, &c) in code[at..].iter().enumerate() {
        match c {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + j + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The brace that closes the one at `open`.
fn matching(code: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (j, &c) in code[open..].iter().enumerate() {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + j);
                }
            }
            _ => {}
        }
    }
    None
}

/// **The function each offset of `code` is in**: every `fn NAME` with its
/// span, from the keyword to the brace closing its body; the innermost
/// span holding the offset names it.
fn functions(code: &str) -> Vec<(String, usize, usize)> {
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = find(b, i, b"fn ") {
        i = at + 3;
        if at > 0 && (b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_') {
            continue;
        }
        let name: String = code[at + 3..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let Some(open) = find(b, at, b"{") else {
            break;
        };
        // A declaration with no body ends at its `;` first.
        if find(b, at, b";").is_some_and(|semi| semi < open) {
            continue;
        }
        if let Some(close) = matching(b, open) {
            out.push((name, at, close));
        }
    }
    out
}

/// The innermost function around `at`, by [`functions`].
fn function_at(spans: &[(String, usize, usize)], at: usize) -> Option<&str> {
    spans
        .iter()
        .filter(|(_, from, to)| *from <= at && at <= *to)
        .min_by_key(|(_, from, to)| to - from)
        .map(|(name, ..)| name.as_str())
}

/// Each place in every source's production code the spelling `call`
/// begins, by `(file, function)`, counted; `matches` decides whether the
/// code at an offset is the call.
fn census(
    sources: &[(String, String)],
    matches: impl Fn(&str, usize) -> bool,
    call: &str,
) -> Vec<(String, String, usize)> {
    let mut found: Vec<(String, String, usize)> = Vec::new();
    for (name, text) in sources {
        let code = production(text);
        let spans = functions(&code);
        for (at, _) in code.match_indices(call) {
            if !matches(&code, at) {
                continue;
            }
            let f = function_at(&spans, at).unwrap_or("").to_owned();
            match found.iter_mut().find(|(n, g, _)| *n == *name && *g == f) {
                Some((.., n)) => *n += 1,
                None => found.push((name.clone(), f, 1)),
            }
        }
    }
    found.sort_unstable();
    found
}

/// A table's rows as the census spells them.
fn want(rows: &[(&str, &str, usize)]) -> Vec<(String, String, usize)> {
    let mut w: Vec<_> = rows
        .iter()
        .map(|(f, g, n)| ((*f).to_owned(), (*g).to_owned(), *n))
        .collect();
    w.sort_unstable();
    w
}

/// `app.rs`, read whole.
fn app_rs() -> String {
    read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-app/src/app.rs"
    ))
    .expect("app.rs")
}

/// **The production code is read past `app.rs`'s first test module**: a
/// function past it is there, and a test past it is not. The control for
/// every census below. Named break: cut each file at its first test
/// module, and `print_later` is gone.
#[test]
fn production_code_is_read_past_the_first_test_module() {
    let app = app_rs();
    let first = app
        .find("\n#[cfg(test)]\nmod tests {")
        .expect("app.rs's first test module");
    let print = app.find("\nfn print_later(").expect("print_later");
    assert!(first < print, "print_later past the first test module");
    let code = production(&app);
    assert!(code.contains("\nfn print_later("), "read past it");
    assert!(!code.contains("mod tests {"), "its test modules left out");
    let spans = functions(&code);
    let at = code.find("\nfn print_later(").expect("print_later") + 1;
    assert_eq!(function_at(&spans, at), Some("print_later"));
}

/// **A delayed block goes on the main queue only through `turn::after`**:
/// `DispatchQueue::main()` followed by `.after(` -- across a line break
/// too -- is found in `turn::after` and in no other function but the two
/// that make no effect: a window's wake at the settle's end (`wake_at`,
/// which the next stage moves onto a turn with the other bare blocks),
/// and the README filming run's reading of its window (`run`). Named
/// break: the expiry back on a bare main-queue block.
#[test]
fn a_delayed_block_reaches_the_main_queue_only_through_the_turn() {
    let main = "DispatchQueue::main()";
    let delayed =
        |code: &str, at: usize| code[at + main.len()..].trim_start().starts_with(".after(");
    let found = census(&sources(), delayed, main);
    assert_eq!(
        found,
        want(&[
            ("src/turn.rs", "after", 1),
            ("../baud-app/src/app.rs", "wake_at", 1),
            ("../baud-app/src/full_screen_check.rs", "run", 1),
        ]),
        "{found:?}"
    );
}

/// A call's sites: each file, the function in it, and how many times.
type Sites = &'static [(&'static str, &'static str, usize)];

/// Each call, and the functions it is made in, with counts. `.activate()`
/// is also a dispatch source's activation, named with its functions, so a
/// third anywhere is found.
const CALLS: &[(&str, Sites)] = &[
    (
        ".requestUserAttention(",
        &[("src/notify.rs", "attention", 1)],
    ),
    (
        ".cancelUserAttentionRequest(",
        &[("src/notify.rs", "cancel_attention", 1)],
    ),
    (
        ".makeKeyAndOrderFront(",
        &[("src/window.rs", "make_key_and_order_front", 1)],
    ),
    (
        ".activate()",
        &[
            ("src/window.rs", "activated", 1),
            // A dispatch source's, not the app's: `s.activate()`.
            ("../baud-app/src/full_screen_check.rs", "arm", 1),
            // A dispatch source's, not the app's: `source.activate()`.
            ("../baud-app/src/reload.rs", "listen", 1),
        ],
    ),
    // The launch's activation, before the run loop and any view: defined
    // in the window module, called once, from the launch.
    (
        "activate_at_launch(",
        &[
            ("src/window.rs", "activate_at_launch", 1),
            ("../baud-app/src/app.rs", "run", 1),
        ],
    ),
    // A turn is made in one place, which checks the main thread.
    ("Turn(", &[("src/turn.rs", "on_main", 1)]),
    // The display link is started and stopped by its door alone, and a
    // closing window's by the close's own stop beside it.
    (
        ".start_display_link(",
        &[("src/display_link.rs", "send", 1)],
    ),
    (".stop_display_link(", &[("src/display_link.rs", "send", 1)]),
    // The door is told to run only where a frame or a tick is owed, and
    // to pause only where the driver settles (`pause`).
    (
        "LinkState::Run",
        &[
            ("../baud-app/src/app.rs", "owe_tick", 1),
            // The frame rig prints the state the probe recorded.
            ("../baud-app/src/app/rig.rs", "run_frames", 1),
            // The call table both builds read.
            ("src/display_link.rs", "call", 1),
            ("src/probe.rs", "linked", 1),
            ("src/probe.rs", "tick", 1),
        ],
    ),
    (
        "LinkState::Pause",
        &[
            ("../baud-app/src/app.rs", "pause", 1),
            ("../baud-app/src/app/rig.rs", "run_frames", 1),
            ("src/display_link.rs", "closed", 1),
            ("src/display_link.rs", "call", 1),
            ("src/display_link.rs", "new", 1),
            ("src/probe.rs", "", 1),
        ],
    ),
    // The holders are lifted only by the frame rig, in a probe build: the
    // probe module exists only under the feature, so a build without it
    // cannot name the lift at all.
    (
        "Lift::Lifted",
        &[
            ("../baud-app/src/app.rs", "holding", 1),
            ("../baud-app/src/app/rig.rs", "run_frames", 1),
        ],
    ),
    // A tick is owed through one door: a frame owed, the window's first
    // frame at its opening (the rig's as well), a deadline's restart, and a
    // settle that finds a frame owed.
    (
        "owe_tick(",
        &[
            ("../baud-app/src/app.rs", "owe_frame", 1),
            ("../baud-app/src/app.rs", "owe_tick", 1),
            ("../baud-app/src/app.rs", "restarted", 1),
            ("../baud-app/src/app.rs", "settle", 1),
            ("../baud-app/src/app.rs", "show", 1),
            ("../baud-app/src/app/rig.rs", "run", 1),
            ("../baud-app/src/app/rig.rs", "run_frames", 1),
        ],
    ),
];

/// **Each desktop call is made in its own function, as many times as
/// listed**, and in no other: the attention request and its cancel in
/// `notify.rs`, a window made key and the app activated in `window.rs`,
/// each behind a function that takes a `Turn`; a turn made only in
/// `on_main`. Named break: a second `.makeKeyAndOrderFront(` in `app.rs`.
#[test]
fn each_desktop_call_is_made_in_its_own_function() {
    let sources = sources();
    for (call, allowed) in CALLS {
        let whole = |code: &str, at: usize| {
            let before = code[..at].chars().next_back();
            !call.starts_with(|c: char| c.is_alphanumeric())
                || !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
        };
        let found = census(&sources, whole, call);
        assert_eq!(found, want(allowed), "{call}");
    }
}

/// **A macOS window's frame is owed through `owe_frame` alone**, which
/// runs the display link as it owes the frame, so a turn that never
/// settles still draws: in the macOS driver's sources (`app.rs` and
/// `app/`) the assignment is made in that one function. The Linux loop
/// keeps its own, and waits on its own clock. Named break: a
/// `self.app.pending = Pending::Dirty;` in `frame`.
#[test]
fn a_macos_frame_is_owed_only_through_its_door() {
    let sources: Vec<_> = sources()
        .into_iter()
        .filter(|(name, _)| {
            name == "../baud-app/src/app.rs" || name.starts_with("../baud-app/src/app/")
        })
        .collect();
    assert!(sources.len() > 2, "the driver's sources: {}", sources.len());
    let found = census(&sources, |_, _| true, "pending = Pending::Dirty");
    assert_eq!(found, want(&[("../baud-app/src/app.rs", "owe_frame", 1)]));
}

/// **Every responder call settles once it returns, but `cursor_rect`**:
/// each method of `Guarded`'s `Responder` ends by settling the inner
/// responder (`unwind::Settles`), after its guarded call, so an early
/// return inside the driver's method never skips the display link's
/// settle. Named break: `frame` without `self.settled();`.
#[test]
fn every_responder_call_settles_but_the_cursor_rect() {
    let src =
        read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/unwind.rs")).expect("unwind.rs");
    let code = production(&src);
    let start = code
        .find("impl<R: Responder + Settles> Responder for Guarded<R> {")
        .expect("Guarded's responder");
    let end = matching(
        code.as_bytes(),
        start + code[start..].find('{').expect("its body"),
    )
    .expect("its end");
    let body = &code[start..end];
    let spans = functions(body);
    let names: Vec<&str> = spans.iter().map(|(n, ..)| n.as_str()).collect();
    assert!(names.len() >= 15, "every method read: {names:?}");
    for (name, from, to) in &spans {
        let f = &body[*from..*to];
        let settles = f.matches("self.settled();").count();
        match name.as_str() {
            "cursor_rect" => assert_eq!(settles, 0, "{name} changes nothing"),
            _ => {
                assert_eq!(settles, 1, "{name} settles once");
                let guarded = f.find("guard(").expect("guarded");
                assert!(
                    guarded < f.find("self.settled();").expect("settled"),
                    "{name}: after"
                );
            }
        }
    }
}

/// **The launch activates the app before its run loop runs**: in `run`,
/// `activate_at_launch` comes before `ns_app.run()`, where no view can
/// yet hold a responder borrow. Named break: the activation after the
/// run loop starts.
#[test]
fn the_launch_activates_before_the_run_loop() {
    let code = production(&app_rs());
    let spans = functions(&code);
    let run = spans
        .iter()
        .filter(|(name, ..)| name == "run")
        .find(|(_, from, to)| code[*from..*to].contains("activate_at_launch("))
        .expect("the launch's run");
    let body = &code[run.1..run.2];
    let activated = body
        .find("window::activate_at_launch(mtm);")
        .expect("activated");
    let runs = body.find("ns_app.run();").expect("the run loop");
    assert!(activated < runs, "activated before the run loop");
}
