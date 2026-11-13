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
/// that make no effect: a window's wake at the settle's end
/// (`wake_holding`, `wake_at`'s, which the next stage moves onto a turn
/// with the other bare blocks),
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
            ("../baud-app/src/app.rs", "wake_holding", 1),
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
            // A dispatch source's, not the app's: `source.activate()`, the
            // master's room to write heard on the main queue.
            ("../baud-app/src/pty/outbox_watch.rs", "listen", 1),
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
            // The frame rig's quiet reads the state the probe recorded,
            // and its print shows it.
            ("../baud-app/src/app/rig.rs", "quiet", 1),
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
            ("../baud-app/src/app/rig.rs", "split", 1),
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

/// **The driver's sources a turn or a frame runs**, read for sends to
/// AppKit and Core Animation, from this crate's root.
const SENDERS: &[&str] = &[
    "../baud-app/src/app.rs",
    "../baud-app/src/lights.rs",
    "../baud-app/src/strip.rs",
    "../baud-app/src/mark.rs",
    "src/lib.rs",
];

/// Value types whose constructors and constants are not sends.
const VALUES: &[&str] = &[
    "NSPoint",
    "NSRect",
    "NSSize",
    "NSEdgeInsets",
    "CGPoint",
    "CGRect",
    "CGSize",
    "CGFloat",
];

/// **The window crate's methods that send**, snake_case and so unseen by
/// the camelCase rule, by names it alone uses: on any receiver.
const WRAPPERS: &[&str] = &[
    "set_title",
    "set_surface",
    "set_content_view",
    "set_origin",
    "set_size",
    "set_min_size",
    "start_display_link",
    "stop_display_link",
    "set_appearance",
    "set_material",
    "set_tint_color",
    "set_corner_radius",
    "set_has_shadow",
    "set_accepts_mouse_moved",
    "make_first_responder",
    "add_titlebar_accessory",
    "effective_appearance",
    "is_visible",
    "content_view",
];

/// **The window crate's sending methods whose names baud's own types use
/// too**: a send on any receiver but those, which are baud's.
const GENERIC: &[&str] = &[
    "show",
    "hide",
    "close",
    "center",
    "size",
    "origin",
    "set_style",
    "set_state",
];

/// The receivers of [`GENERIC`]'s names that are baud's own types: its
/// lights, its mark's place, a find bar, and `self`.
const NOT_WINDOW: &[&str] = &["self", "self.lights", "self.place", "f"];

/// **One-word selectors**, which a Rust method can share (`term.screen()`):
/// a send only on a receiver baud binds an AppKit view or window to.
const ONE_WORD: &[&str] = &[
    "window",
    "frame",
    "bounds",
    "layer",
    "superview",
    "subviews",
];

/// The identifier that starts at `at`.
fn ident_at(code: &str, at: usize) -> &str {
    let end = code[at..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(code.len(), |e| at + e);
    &code[at..end]
}

/// The receiver a `.` at `dot` is called on: the path of identifiers and
/// dots before it, `self.view` or `f`.
fn receiver(code: &str, dot: usize) -> &str {
    let start = code[..dot]
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .map_or(0, |s| s + 1);
    &code[start..dot]
}

/// **Each send in `code`**, by offset and what it is. A send is a
/// camelCase method call (an AppKit binding: Rust's own are snake_case),
/// `_` included so a selector of several parts is one; a class method on
/// an `NS`, `CA` or `CG` type other than a value type's; one of the
/// window crate's sending methods ([`WRAPPERS`], [`GENERIC`]); and a
/// one-word selector ([`ONE_WORD`]) on a view, `view` or a field named
/// so (`self.view`, `holder.view`), or on the window `ns_window()` hands
/// out.
fn sends_in(code: &str) -> Vec<(usize, String)> {
    let b = code.as_bytes();
    let mut out = Vec::new();
    for (dot, _) in code.match_indices('.') {
        let name = ident_at(code, dot + 1);
        if name.is_empty() || b.get(dot + 1 + name.len()) != Some(&b'(') {
            continue;
        }
        let lower = name.trim_start_matches(|c: char| c.is_ascii_lowercase());
        let camel = lower.len() < name.len() && lower.starts_with(|c: char| c.is_ascii_uppercase());
        let recv = receiver(code, dot);
        let bound =
            recv == "view" || recv.ends_with(".view") || code[..dot].ends_with(".ns_window()");
        let send = camel
            || WRAPPERS.contains(&name)
            || (GENERIC.contains(&name) && !NOT_WINDOW.contains(&recv))
            || (ONE_WORD.contains(&name) && bound);
        if send {
            out.push((dot, name.to_owned()));
        }
    }
    for prefix in ["NS", "CA", "CG"] {
        for (at, _) in code.match_indices(prefix) {
            if at > 0 && (b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_') {
                continue;
            }
            let ty = ident_at(code, at);
            if !ty[2..].starts_with(|c: char| c.is_ascii_uppercase()) || VALUES.contains(&ty) {
                continue;
            }
            let Some(rest) = code[at + ty.len()..].strip_prefix("::") else {
                continue;
            };
            let method = ident_at(rest, 0);
            if method.starts_with(|c: char| c.is_ascii_lowercase())
                && rest[method.len()..].starts_with('(')
            {
                out.push((at, format!("{ty}::{method}")));
            }
        }
    }
    out
}

/// **Every function in [`SENDERS`]' production code that sends**, by
/// `(file, function)`.
fn senders() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found: Vec<(String, String)> = Vec::new();
    for file in SENDERS {
        let code = production(&read_to_string(root.join(file)).expect("a source"));
        let spans = functions(&code);
        for (at, _) in sends_in(&code) {
            let f = function_at(&spans, at).unwrap_or("").to_owned();
            let row = ((*file).to_owned(), f);
            if !found.contains(&row) {
                found.push(row);
            }
        }
    }
    found.sort_unstable();
    found
}

/// **The functions that send and run only on an event of their own**:
/// the launch or a window's opening, a click or a key, a program's
/// request, a notification AppKit posts, a config reload. Each with the
/// event, never a frequency. What a turn or a frame sends on any other
/// path goes through `src/sends.rs`, where it is counted.
const COLD: &[(&str, &str, &str)] = &[
    ("../baud-app/src/app.rs", "apply_config", "a config reload"),
    (
        "../baud-app/src/app.rs",
        "apply_glass",
        "opening, and an appearance change",
    ),
    (
        "../baud-app/src/app.rs",
        "close_later",
        "the window's close: the shell's exit or the close button",
    ),
    ("../baud-app/src/app.rs", "closed", "the window closed"),
    (
        "../baud-app/src/app.rs",
        "current_event_flags",
        "a key event",
    ),
    (
        "../baud-app/src/app.rs",
        "current_event_time",
        "a key event",
    ),
    (
        "../baud-app/src/app.rs",
        "hsb",
        "opening, and an appearance change: the glass's colours",
    ),
    (
        "../baud-app/src/app.rs",
        "inactive_selection",
        "a focus change: the window becoming or resigning key",
    ),
    (
        "../baud-app/src/app.rs",
        "mac_window",
        "a window notification (`WindowHeard`), or a program's window report",
    ),
    (
        "../baud-app/src/app.rs",
        "material_appearance",
        "opening, and an appearance change",
    ),
    (
        "../baud-app/src/app.rs",
        "new",
        "opening: the window and its views made",
    ),
    (
        "../baud-app/src/app.rs",
        "new_window_from_menu",
        "a click or a key: File > New Window, or its key equivalent",
    ),
    (
        "../baud-app/src/app.rs",
        "open_on_screen",
        "opening: where the window opens",
    ),
    (
        "../baud-app/src/app.rs",
        "open_window",
        "the New Window key: a window opened after the launch",
    ),
    (
        "../baud-app/src/app.rs",
        "os_at_least_15",
        "opening, and a config reload: the caret's rate",
    ),
    (
        "../baud-app/src/app.rs",
        "os_cycle",
        "opening, and a config reload: the caret's rate",
    ),
    (
        "../baud-app/src/app.rs",
        "over",
        "a click on a link: the window its window opens over",
    ),
    (
        "../baud-app/src/app.rs",
        "placed",
        "a window notification (`WindowHeard`, `poll_window`)",
    ),
    (
        "../baud-app/src/app.rs",
        "predicted_inset",
        "opening: the title bar's height before the window exists",
    ),
    (
        "../baud-app/src/app.rs",
        "refont",
        "a font change: the font size keys, or a config reload",
    ),
    ("../baud-app/src/app.rs", "run", "the launch"),
    (
        "../baud-app/src/app.rs",
        "scroll",
        "a wheel or trackpad scroll",
    ),
    (
        "../baud-app/src/app.rs",
        "scroller_style",
        "opening, and the scroller style's notification",
    ),
    (
        "../baud-app/src/app.rs",
        "set_content_view",
        "opening: the container put on the material",
    ),
    (
        "../baud-app/src/app.rs",
        "set_focus",
        "a focus change: the window becoming or resigning key",
    ),
    (
        "../baud-app/src/app.rs",
        "show",
        "opening: the window shown",
    ),
    (
        "../baud-app/src/app.rs",
        "show_notice",
        "the config's notice put up or taken down",
    ),
    (
        "../baud-app/src/app.rs",
        "splash_check",
        "the launch splash waiting for its window (`Splash::waiting`)",
    ),
    (
        "../baud-app/src/app.rs",
        "system_cursor",
        "the pointer's shape changed: a move over a link, a program's shape",
    ),
    (
        "../baud-app/src/app.rs",
        "tip_at_pointer",
        "a pointer crossing, or a slide of the lights' window",
    ),
    (
        "../baud-app/src/app.rs",
        "widen_later",
        "a program's scrollbar change that resizes the window",
    ),
    (
        "../baud-app/src/lights.rs",
        "screen_frames",
        "the pointer over a light: its hover text",
    ),
    (
        "../baud-app/src/lights.rs",
        "show",
        "a light lit or gone dark, or the strip resized",
    ),
    (
        "../baud-app/src/strip.rs",
        "fit_lights",
        "a resize: the lights' accessory fitted",
    ),
    (
        "../baud-app/src/strip.rs",
        "follow",
        "a full-screen change's notification",
    ),
    (
        "../baud-app/src/strip.rs",
        "in_window",
        "a resize or a full-screen change: the title bar laid out",
    ),
    (
        "../baud-app/src/strip.rs",
        "inset_of",
        "opening, and a full-screen change: the title bar's height",
    ),
    (
        "../baud-app/src/strip.rs",
        "laid",
        "a resize or a full-screen change: the title bar laid out",
    ),
    (
        "../baud-app/src/strip.rs",
        "of",
        "a resize or a full-screen change: the safe area's inset",
    ),
    (
        "../baud-app/src/strip.rs",
        "put",
        "opening: the strip put in the title bar",
    ),
    (
        "../baud-app/src/mark.rs",
        "appearance_of",
        "opening, and an appearance change",
    ),
    (
        "../baud-app/src/mark.rs",
        "fit",
        "a resize, after the lights are placed",
    ),
    (
        "../baud-app/src/mark.rs",
        "lockup_end",
        "a resize: the lockup's right edge",
    ),
    ("../baud-app/src/mark.rs", "place", "opening, and a resize"),
    (
        "../baud-app/src/mark.rs",
        "put",
        "opening: the lockup put in the title bar",
    ),
];

/// **Every send in the driver's per-turn and per-frame sources goes
/// through a counted seam** (`src/sends.rs`), or is in a
/// function on [`COLD`], which runs only on an event of its own. Each
/// entry must still send, so a stale one fails, and its reason names an
/// event. The text cannot see a cold function called from a turn's path,
/// nor a one-word selector on a receiver it does not know: the event
/// rig's count on a still screen covers the path a still screen runs.
/// Named break: a raw `.setMiniwindowTitle(` put back in `send_title`.
#[test]
fn every_send_on_a_turn_goes_through_a_counted_seam() {
    let found = senders();
    let cold: Vec<(String, String)> = COLD
        .iter()
        .map(|(f, g, _)| ((*f).to_owned(), (*g).to_owned()))
        .collect();
    let unlisted: Vec<&(String, String)> = found.iter().filter(|r| !cold.contains(r)).collect();
    let stale: Vec<&(String, String)> = cold.iter().filter(|r| !found.contains(r)).collect();
    let vague: Vec<&str> = COLD
        .iter()
        .filter(|(_, _, why)| why.is_empty() || why.contains("rare") || why.contains("seldom"))
        .map(|(_, g, _)| *g)
        .collect();
    assert!(
        unlisted.is_empty() && stale.is_empty() && vague.is_empty(),
        "sends outside a seam and off the cold table: {unlisted:?}\n\
         cold entries that no longer send: {stale:?}\n\
         cold reasons that name no event: {vague:?}"
    );
}

/// **The send grammar reads each form it names**, and passes over a value
/// type, baud's own `show` and a Rust method's one word: the control for
/// [`every_send_on_a_turn_goes_through_a_counted_seam`].
#[test]
fn the_send_grammar_reads_each_form() {
    let src = "fn a(w: &NSWindow) { let r = w.convertRect_toView(r, None); }\n\
               fn b() { let s = NSSize::new(1.0, 2.0); let m = NSWindowStyleMask::FullScreen; }\n\
               fn c(v: &NSView) { v.setHidden(true); }\n\
               fn d() { NSCursor::arrowCursor().set(); }\n\
               fn e(&self) { let f = self.view.frame(); }\n\
               fn g(&self) { self.lights.show(); }\n\
               fn h(window: &GlassWindow) { window.show(); }\n\
               fn i(t: &Term) { t.screen(); t.frame(); }\n\
               fn j(win: &GlassWindow) { win.set_title(\"x\"); }\n\
               fn k(g: &GlassWindow) { let w = g.ns_window().window(); }\n";
    let code = production(src);
    let spans = functions(&code);
    let mut named: Vec<&str> = sends_in(&code)
        .iter()
        .filter_map(|(at, _)| function_at(&spans, *at))
        .collect();
    named.sort_unstable();
    named.dedup();
    assert_eq!(named, ["a", "c", "d", "e", "h", "j", "k"]);
}

/// **Every seam counts its send**: each free function in `sends.rs` names
/// its `Kind` to `count` before the
/// call. Named break: `count` taken out of `set_title`.
#[test]
fn every_seam_counts_its_send() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let code = production(&read_to_string(root.join("src/sends.rs")).expect("sends.rs"));
    let mut seams = 0;
    for (name, from, to) in functions(&code) {
        let head = &code[code[..from].rfind('\n').map_or(0, |n| n + 1)..from];
        if head != "pub " {
            continue;
        }
        seams += 1;
        assert!(
            code[from..to].contains("count(Kind::"),
            "the seam {name} counts its send"
        );
    }
    assert!(seams >= 10, "the seams were found: {seams}");
}

/// **The window crate's names in [`WRAPPERS`] and [`GENERIC`] are its own
/// methods** at the version the lockfile pins, read from cargo's copy of
/// its source: a name it does not have is a send the grammar would never
/// see.
#[test]
fn the_window_crate_s_senders_are_its_own() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lock = read_to_string(root.join("../Cargo.lock")).expect("the lockfile");
    let version = lock
        .split("[[package]]")
        .find(|p| p.contains("name = \"macos-liquid-glass\""))
        .and_then(|p| p.lines().find_map(|l| l.strip_prefix("version = \"")))
        .map(|v| v.trim_end_matches('"').to_owned())
        .expect("the window crate's version");
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .expect("cargo's home");
    let crate_dir = read_dir(home.join("registry/src"))
        .expect("cargo's registry")
        .filter_map(Result::ok)
        .map(|index| {
            index
                .path()
                .join(format!("macos-liquid-glass-{version}/src"))
        })
        .find(|p| p.is_dir())
        .expect("the window crate's source");
    let mut source = String::new();
    for entry in read_dir(&crate_dir).expect("its sources") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            source.push_str(&read_to_string(&path).expect("a source"));
        }
    }
    for name in WRAPPERS.iter().chain(GENERIC) {
        assert!(
            source.contains(&format!("pub fn {name}(")),
            "macos-liquid-glass {version} has `{name}`"
        );
    }
}
