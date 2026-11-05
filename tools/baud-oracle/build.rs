//! Where the linker finds HarfBuzz, FreeType and libxkbcommon: the `-L`
//! directories `pkg-config` names for them, which `tools/shell.nix` points
//! at the pinned builds. No `-sys` crate, so nothing here can fall back to
//! a vendored copy of any engine. libxkbcommon has no version call, so the
//! version `pkg-config` gives is passed to the crate, which refuses any but
//! the pinned one.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    for lib in ["harfbuzz", "freetype2", "xkbcommon"] {
        let out = Command::new("pkg-config")
            .args(["--libs-only-L", lib])
            .output()
            .unwrap_or_else(|e| panic!("pkg-config did not run ({e}); build in tools/shell.nix"));
        assert!(
            out.status.success(),
            "pkg-config has no {lib}; build in tools/shell.nix:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let flags = String::from_utf8(out.stdout).expect("utf-8 flags");
        for dir in flags
            .split_whitespace()
            .filter_map(|f| f.strip_prefix("-L"))
        {
            println!("cargo:rustc-link-search=native={dir}");
        }
    }
    let out = Command::new("pkg-config")
        .args(["--modversion", "xkbcommon"])
        .output()
        .expect("pkg-config ran above");
    let version = String::from_utf8(out.stdout).expect("a utf-8 version");
    println!("cargo:rustc-env=BAUD_ORACLE_XKBCOMMON={}", version.trim());
}
