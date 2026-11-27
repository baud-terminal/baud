//! Link libghostty-vt, or fail the build saying why.
//!
//! **A missing library is an error here, where `multibench` warns and skips.**
//! multibench measures five other cores without ghostty and prints `n/a` in
//! its row; this harness has one oracle, and a run with none would pass every
//! fixture by comparing nothing. Note V's V10 design asks for exactly this:
//! "A missing library exits nonzero with a message."
//!
//! The library comes from `GHOSTTY_LIB_DIR` and nowhere else, built from the
//! ghostty checkout at d9840f3 with
//!
//!     zig build -Demit-lib-vt=true -Doptimize=ReleaseFast
//!
//! The library carries no commit -- its version string reads `0.1.0-dev`,
//! which `main` checks at start-up -- so the tie to d9840f3 is the
//! checkout it was built in, not the binary.

fn main() {
    println!("cargo:rerun-if-env-changed=GHOSTTY_LIB_DIR");
    let dir = match std::env::var("GHOSTTY_LIB_DIR") {
        Ok(d) => d,
        Err(_) => panic!(
            "GHOSTTY_LIB_DIR is not set: point it at the zig-out/lib of a ghostty \
             checkout at d9840f3 built with `zig build -Demit-lib-vt=true`"
        ),
    };
    let archive = std::path::Path::new(&dir).join("libghostty-vt.a");
    if !archive.exists() {
        panic!("libghostty-vt.a not found in {dir}: the oracle cannot run without it");
    }
    // **Copy the archive somewhere the dylib is not**, multibench's reason:
    // zig-out/lib holds both, and macOS `ld` prefers the dylib for
    // -lghostty-vt even under rustc's `static=`, which links and then fails at
    // run time with "Library not loaded: @rpath/libghostty-vt.dylib".
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::copy(&archive, out.join("libghostty-vt.a")).expect("copy libghostty-vt.a");
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");
}
