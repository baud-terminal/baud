//! Where the linker finds HarfBuzz and FreeType: the `-L` directories
//! `pkg-config` names for them, which `tools/shell.nix` points at the
//! pinned builds. No `-sys` crate, so nothing here can fall back to a
//! vendored copy of either engine.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    for lib in ["harfbuzz", "freetype2"] {
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
}
