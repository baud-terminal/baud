//! **The material probe opens no window** (`tests/material_probe.rs`): it
//! makes views and never a window or the application, so nothing can be
//! put on the screen. This reads the probe's source, with its comments
//! blanked, and fails on any name that makes, shows or activates one.
//!
//! The positive control: the names the probe must use to do its work are
//! found in the same blanked source, so a read that finds nothing at all
//! cannot pass.
//!
//! Perturbation: add `NSWindow` or `NSApp(` to the probe and this goes red.

use std::fs::read_to_string;

mod scan;
use scan::blanked;

/// Names that make, show or activate a window or the application.
const BARRED: &[&str] = &[
    "NSWindow",
    "NSApplication",
    "NSApp(",
    "NSPanel",
    "orderFront",
    "makeKeyAndOrderFront",
    "activate",
    "macos_liquid_glass::Window",
    "Window::",
];

/// Names the probe needs, which a working read must find.
const NEEDED: &[&str] = &["NSVisualEffectView", "CARenderer", "setMaterial("];

/// Whether `code` holds `name`'s bytes anywhere.
fn has(code: &[u8], name: &str) -> bool {
    code.windows(name.len()).any(|w| w == name.as_bytes())
}

#[test]
fn the_material_probe_names_no_window_and_no_application() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/material_probe.rs");
    let src = read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let code = blanked(&src);
    for name in NEEDED {
        assert!(
            has(&code, name),
            "the control: {name} is not found in the probe's code"
        );
    }
    for name in BARRED {
        assert!(
            !has(&code, name),
            "the probe names {name}, which can put a window on the screen"
        );
    }
}
