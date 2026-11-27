//! **Every desktop effect is built only without the effect probe.**
//!
//! The event rig (`crates/baud-app/tests/event_rig.rs`) runs baud in a child with
//! the test-only `effect-probe` feature and must change nothing on the
//! desktop: no beep, no notification, no window raised, minimised or put
//! in full screen, no pasteboard written. So each AppKit call that makes
//! such an effect is configured out of the probe build: it carries
//! `#[cfg(not(feature = "effect-probe"))]` itself, or a block or function
//! around it does, and the probe's arm beside it records the effect's
//! kind. This reads the sources and checks it, call by call, by the
//! attribute's own scope -- a probe arm earlier in the function proves
//! nothing, since a body that records and then calls would pass that. The
//! table is the positive control: each call must be found where it says.
//!
//! Perturbation: delete the `#[cfg(not(feature = "effect-probe"))]` line
//! above `NSBeep()` in `bell.rs`, and the probe build records the beep and
//! still beeps; `NSBeep()` is named here as built into the probe.

use std::fs::read_to_string;

mod scan;
use scan::blanked;

/// Each call, and the file it is made in. A method is spelled with its
/// receiver's `.`, so `.miniaturize(` is not found inside `deminiaturize(`.
const GATED: &[(&str, &str)] = &[
    ("src/bell.rs", "NSBeep()"),
    ("src/window.rs", ".orderFrontRegardless()"),
    ("src/window.rs", ".makeKeyAndOrderFront("),
    ("src/window.rs", ".activate()"),
    ("src/window.rs", "set_application_icon("),
    ("src/window.rs", ".deminiaturize(None)"),
    ("src/window.rs", ".miniaturize(None)"),
    ("src/window.rs", ".orderBack(None)"),
    ("src/window.rs", ".toggleFullScreen(None)"),
    ("src/window.rs", ".setFrame_display("),
    ("src/window.rs", "opens()"),
    ("src/clipboard.rs", ".clearContents()"),
    ("src/clipboard.rs", ".setString_forType("),
    (
        "src/announce.rs",
        "NSAccessibilityPostNotificationWithUserInfo(",
    ),
    // The center itself: a field the probe build does not have, and the
    // three messages that find it and give it the delegate, or ask it.
    ("src/notify.rs", "center: Retained<AnyObject>,"),
    ("src/notify.rs", "currentNotificationCenter]"),
    ("src/notify.rs", "setDelegate:"),
    (
        "src/notify.rs",
        "getDeliveredNotificationsWithCompletionHandler:",
    ),
    ("src/notify.rs", "requestAuthorizationWithOptions:"),
    ("src/notify.rs", "addNotificationRequest:"),
    ("src/notify.rs", "setNotificationCategories:"),
    (
        "src/notify.rs",
        "removeDeliveredNotificationsWithIdentifiers:",
    ),
    (
        "src/notify.rs",
        "removePendingNotificationRequestsWithIdentifiers:",
    ),
    ("src/notify.rs", ".dockTile()"),
    ("src/notify.rs", ".requestUserAttention("),
    ("src/notify.rs", ".cancelUserAttentionRequest("),
    // The README filming run's full-screen check, whose arming is the
    // only way to its `toggleFullScreen:`.
    ("../baud-app/src/app.rs", "full_screen_check::arm("),
];

/// The gate, its spaces taken out.
const GATE: &str = "#[cfg(not(feature=\"effect-probe\"))]";

/// Where a statement starts: a byte offset in the source.
#[derive(Clone, Copy)]
struct Start(usize);

/// An open delimiter.
enum Open {
    /// A brace: where the statement it opens starts -- its parent level's
    /// statement when it was opened -- and the brace itself; and where the
    /// statement now being read at its level starts, after the last `;`,
    /// `,`, `{` or `}` there.
    Brace { head: (Start, usize), now: Start },
    /// A parenthesis or a bracket, whose `,` separates no statement.
    Group,
}

/// Whether the statement from `from` carries [`GATE`] among its leading
/// attributes: past blanks (comments included), each `#[...]` read whole
/// from `src` and compared with its spaces taken out.
fn gated_from(src: &str, code: &[u8], from: Start, to: usize) -> bool {
    let mut at = from.0;
    loop {
        while at < to && code[at].is_ascii_whitespace() {
            at += 1;
        }
        if !code[at..to].starts_with(b"#[") {
            return false;
        }
        let mut depth = 0usize;
        let mut end = at;
        for (j, &c) in code[at..to].iter().enumerate() {
            match c {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = at + j + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end == at {
            return false;
        }
        let attr: String = src[at..end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if attr == GATE {
            return true;
        }
        at = end;
    }
}

/// Where the statement being read at the innermost brace level starts.
fn level(stack: &[Open]) -> Start {
    stack
        .iter()
        .rev()
        .find_map(|o| match o {
            Open::Brace { now, .. } => Some(*now),
            Open::Group => None,
        })
        .unwrap_or(Start(0))
}

/// Whether the code at `at` is configured out of the probe build: its own
/// statement, or the statement that opens any brace around it -- a block,
/// an `unsafe` block, a function or an `impl` -- carries [`GATE`].
fn gated_at(src: &str, code: &[u8], at: usize) -> bool {
    let mut stack = vec![Open::Brace {
        head: (Start(0), 0),
        now: Start(0),
    }];
    for (i, &c) in code[..at].iter().enumerate() {
        match c {
            b'{' => {
                let parent = level(&stack);
                stack.push(Open::Brace {
                    head: (parent, i),
                    now: Start(i + 1),
                });
            }
            b'(' | b'[' => stack.push(Open::Group),
            b'}' | b')' | b']' => {
                let popped = stack.pop().expect("balanced");
                if let (Open::Brace { .. }, Some(Open::Brace { now, .. })) =
                    (popped, stack.last_mut())
                {
                    *now = Start(i + 1);
                }
            }
            b';' | b',' => {
                if let Some(Open::Brace { now, .. }) = stack.last_mut() {
                    *now = Start(i + 1);
                }
            }
            _ => {}
        }
    }
    gated_from(src, code, level(&stack), at)
        || stack.iter().skip(1).any(|o| match o {
            Open::Brace { head, .. } => gated_from(src, code, head.0, head.1),
            Open::Group => false,
        })
}

/// Each place `call` is made in `src` -- comments and strings left out --
/// and whether it is configured out of the probe build there.
fn sites(src: &str, call: &str) -> Vec<(usize, bool)> {
    let code = blanked(src);
    src.match_indices(call)
        .map(|(at, _)| at)
        .filter(|&at| code[at] == src.as_bytes()[at])
        .map(|at| (at, gated_at(src, &code, at)))
        .collect()
}

#[test]
fn each_desktop_effect_is_built_only_without_the_probe() {
    let root = env!("CARGO_MANIFEST_DIR");
    for (file, call) in GATED {
        let src = read_to_string(format!("{root}/{file}")).expect(file);
        let code = src
            .split_once("\n#[cfg(test)]\nmod tests")
            .map_or(src.as_str(), |(c, _)| c);
        let found = sites(code, call);
        assert!(!found.is_empty(), "{file} makes {call}");
        for (at, gated) in found {
            let line = code[..at].lines().count();
            assert!(gated, "{file}:{line}: {call} is built into the probe");
        }
    }
}

/// **Every use of the notification center is built only without the
/// probe, and listed**: found by its spelling rather than from a table, so
/// a new one cannot slip past, and counted, so a new one is named here when
/// it comes. The probe build has no center field, so a site there would not
/// compile; this holds the default build to it too. Perturbation: take the
/// gate off `Center::remove`'s call.
#[test]
fn every_use_of_the_center_is_built_only_without_the_probe() {
    let root = env!("CARGO_MANIFEST_DIR");
    let src = read_to_string(format!("{root}/src/notify.rs")).expect("notify.rs");
    let code = src
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(src.as_str(), |(c, _)| c);
    let found = sites(code, "self.center");
    // `ask`, `ask_delivered`, `remove`, `send` and `category`.
    assert_eq!(found.len(), 5, "the five uses of the center");
    for (at, gated) in found {
        let line = code[..at].lines().count();
        assert!(
            gated,
            "notify.rs:{line}: the center is used in the probe build"
        );
    }
}

/// **The scanner, on shapes it must take and refuse.** Taken: a
/// statement, a block and a function each configured out, with braces in a
/// string and a character literal and a lifetime around them, and a gate
/// among other attributes. Refused: the probe's arm followed by the call
/// (the perturbation above), an early return with the call after it, the
/// gate on a sibling statement or on the function before, and a wider
/// gate; and a call in a comment or a string is no call.
#[test]
fn the_gate_is_read_by_its_scope() {
    let one = |src: &str| {
        let s = sites(src, "NSBeep()");
        assert_eq!(s.len(), 1, "{src}");
        s[0].1
    };
    // Taken.
    assert!(one(
        "fn f() {\n    #[cfg(feature = \"effect-probe\")]\n    record(K);\n    \
         #[cfg(not(feature = \"effect-probe\"))]\n    NSBeep();\n}\n"
    ));
    assert!(one(
        "fn f() {\n    let s = \"{\";\n    #[cfg(not(feature = \"effect-probe\"))]\n    \
         unsafe {\n        let t = '}';\n        g(s, t);\n        NSBeep();\n    }\n}\n"
    ));
    assert!(one(
        "/// A helper.\n#[cfg(not(feature = \"effect-probe\"))]\nfn said<'a>(x: &'a str) {\n    \
         let y = format!(\"{}{x}\", 1);\n    if y.is_empty() { return; }\n    NSBeep();\n}\n"
    ));
    assert!(one(
        "impl A {\n    #[cfg(not(feature = \"effect-probe\"))]\n    #[allow(dead_code)]\n    \
         fn f(&self) {\n        h(1, 2);\n        NSBeep();\n    }\n}\n"
    ));
    // Refused.
    assert!(!one(
        "fn f() {\n    #[cfg(feature = \"effect-probe\")]\n    record(K);\n    NSBeep();\n}\n"
    ));
    assert!(!one(
        "fn f() {\n    #[cfg(feature = \"effect-probe\")]\n    {\n        record(K);\n        \
         return;\n    }\n    #[allow(unreachable_code)]\n    NSBeep();\n}\n"
    ));
    assert!(!one(
        "fn f() {\n    #[cfg(not(feature = \"effect-probe\"))]\n    let a = 1;\n    NSBeep();\n}\n"
    ));
    assert!(!one(
        "#[cfg(not(feature = \"effect-probe\"))]\nfn g() {}\nfn f() {\n    NSBeep();\n}\n"
    ));
    assert!(!one(
        "fn f() {\n    #[cfg(any(not(feature = \"effect-probe\"), test))]\n    NSBeep();\n}\n"
    ));
    assert!(
        sites(
            "fn f() {\n    // NSBeep();\n    let s = \"NSBeep()\";\n}\n",
            "NSBeep()"
        )
        .is_empty()
    );
}
