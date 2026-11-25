//! **baud's own build cannot reach the Vulkan crate's test hooks**: in the
//! graph of `baud`'s normal dependencies on every target, as `Cargo.lock`
//! resolves it, `baud-present-vulkan` is built without `test-hooks`, the
//! feature that turns on `Gpu::clear`, a wait on the CPU, the held
//! pipeline builds, and the device failures the crate's tests force. The
//! crate's own tests take it through a dev-dependency, which this graph
//! leaves out.

use std::path::Path;
use std::process::Command;

/// The crate whose hooks are checked, and the feature that turns them on.
const CRATE: &str = "baud-present-vulkan";
const HOOKS: &str = "test-hooks";

/// `baud-present-vulkan`'s features in `baud`'s normal graph on every
/// target, one line per place the crate is reached.
fn features_in_bauds_build() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "tree",
            "--locked",
            "--offline",
            "-p",
            "baud",
            "-e",
            "normal",
            "--target",
            "all",
            "--prefix",
            "none",
            "-f",
            "{p}|{f}",
        ])
        .current_dir(&root)
        .output()
        .expect("cargo tree");
    assert!(
        out.status.success(),
        "cargo tree: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("UTF-8")
        .lines()
        .filter(|l| l.starts_with(&format!("{CRATE} v")))
        .map(|l| l.split_once('|').map_or("", |(_, f)| f).to_string())
        .collect()
}

#[test]
fn bauds_build_cannot_reach_the_vulkan_test_hooks() {
    let reached = features_in_bauds_build();
    assert!(!reached.is_empty(), "{CRATE} is not in baud's graph");
    for features in &reached {
        assert!(
            !features.split(',').any(|f| f == HOOKS),
            "baud's build turns on {CRATE}/{HOOKS}: [{features}]"
        );
    }
}
