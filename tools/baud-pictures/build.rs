//! Where the linker finds zlib and libwebp: the `-L` directories
//! `pkg-config` names for them, which `tools/shell.nix` points at the
//! pinned zlib 1.3.2 and libwebp 1.6.0. No `-sys` crate, so nothing here
//! can fall back to a bundled or another deflater or WebP encoder.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    let out = Command::new("pkg-config")
        .args([
            "--libs-only-L",
            "zlib",
            "libwebp",
            "libwebpmux",
            "libwebpdemux",
        ])
        .output()
        .unwrap_or_else(|e| panic!("pkg-config did not run ({e}); build in tools/shell.nix"));
    assert!(
        out.status.success(),
        "pkg-config has no zlib or libwebp; build in tools/shell.nix:\n{}",
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
