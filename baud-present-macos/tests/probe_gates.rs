//! **Every desktop effect's body is the effect probe's under its feature.**
//!
//! The event rig (`baud-app/tests/event_rig.rs`) runs baud in a child with
//! the test-only `effect-probe` feature and must change nothing on the
//! desktop: no beep, no notification, no window raised, no pasteboard
//! written. So each AppKit call that makes such an effect sits in a
//! function of this crate whose body, under the feature, is
//! `probe::record` of its kind -- or in a helper built only without the
//! feature. This reads the sources and checks it, call by call. The table
//! is the positive control: each call must be found where it says.
//!
//! Perturbation: delete the `#[cfg(feature = "effect-probe")]` arm of
//! `bell::beep` and `NSBeep` is named here as ungated.

use std::fs::read_to_string;

/// Each call, and the file it is made in.
const GATED: &[(&str, &str)] = &[
    ("src/bell.rs", "NSBeep()"),
    ("src/window.rs", "orderFrontRegardless()"),
    ("src/window.rs", "makeKeyAndOrderFront("),
    ("src/window.rs", ".activate()"),
    ("src/window.rs", "set_application_icon("),
    ("src/clipboard.rs", "clearContents()"),
    ("src/clipboard.rs", "setString_forType("),
    (
        "src/announce.rs",
        "NSAccessibilityPostNotificationWithUserInfo(",
    ),
    ("src/notify.rs", "requestAuthorizationWithOptions:"),
    ("src/notify.rs", "addNotificationRequest:"),
    (
        "src/notify.rs",
        "removeDeliveredNotificationsWithIdentifiers:",
    ),
    ("src/notify.rs", "dockTile()"),
    ("src/notify.rs", "requestUserAttention("),
    ("src/notify.rs", "cancelUserAttentionRequest("),
];

/// The function a byte offset is in: its signature's offset, and whether
/// the line above it builds it only without the probe.
fn enclosing(src: &str, at: usize) -> (usize, bool) {
    let start = src[..at]
        .rfind("\nfn ")
        .into_iter()
        .chain(src[..at].rfind("\npub fn "))
        .chain(src[..at].rfind("\n    fn "))
        .chain(src[..at].rfind("\n    pub fn "))
        .max()
        .expect("a function");
    let above = src[..start].lines().last().unwrap_or("");
    (
        start,
        above.trim() == "#[cfg(not(feature = \"effect-probe\"))]",
    )
}

#[test]
fn each_desktop_effect_is_the_probes_under_its_feature() {
    let root = env!("CARGO_MANIFEST_DIR");
    for (file, call) in GATED {
        let src = read_to_string(format!("{root}/{file}")).expect(file);
        let code = src
            .split_once("\n#[cfg(test)]\nmod tests")
            .map_or(src.as_str(), |(c, _)| c);
        let found: Vec<usize> = code.match_indices(call).map(|(i, _)| i).collect();
        assert!(!found.is_empty(), "{file} makes {call}");
        for at in found {
            let line = code[..at].lines().last().unwrap_or("");
            if line.trim_start().starts_with("//") {
                continue;
            }
            let (start, helper) = enclosing(code, at);
            if helper {
                continue;
            }
            let body = &code[start..at];
            let gate = body.find("#[cfg(feature = \"effect-probe\")]");
            let record = body.find("EffectKind::");
            assert!(
                matches!((gate, record), (Some(g), Some(r)) if g < r),
                "{file}: {call} is made without the probe's gate"
            );
        }
    }
}
