fn main() {
    println!("cargo:rustc-link-lib=util");

    // ghostty's terminal core ships as libghostty-vt, built from a ghostty
    // checkout with:
    //
    //     zig build -Demit-lib-vt=true -Doptimize=ReleaseFast
    //
    // On macOS that build's final xcframework step needs xcodebuild and may
    // fail; libghostty-vt.a is produced before it and is what we link.
    let asked = std::env::var("GHOSTTY_LIB_DIR").ok();
    let dir = asked
        .clone()
        .unwrap_or_else(|| "../../../ghostty/zig-out/lib".to_string());
    println!("cargo:rerun-if-env-changed=GHOSTTY_LIB_DIR");
    let archive = std::path::Path::new(&dir).join("libghostty-vt.a");
    if archive.exists() {
        // Copy the archive somewhere the dylib is not. zig-out/lib holds both
        // libghostty-vt.a and libghostty-vt.dylib, and on macOS `ld` prefers the
        // dylib for -lghostty-vt even under rustc's `static=` — which links, then
        // fails at runtime with "Library not loaded: @rpath/libghostty-vt.dylib".
        // An OUT_DIR containing only the archive removes the ambiguity.
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        std::fs::copy(&archive, out.join("libghostty-vt.a")).expect("copy libghostty-vt.a");
        println!("cargo:rerun-if-changed={}", archive.display());
        println!("cargo:rustc-link-search=native={}", out.display());
        println!("cargo:rustc-link-lib=static=ghostty-vt");
        println!("cargo:rustc-cfg=have_ghostty");
    } else if asked.is_some() {
        // A build that names the library is one that compares against
        // ghostty; without it every ghostty figure would be empty, so the
        // build stops here rather than pass without the arm.
        panic!("GHOSTTY_LIB_DIR is {dir}, and libghostty-vt.a is not there");
    } else {
        // Not asked for: the other five cores still measure without it, and
        // the missing row is said in the output, never skipped in silence.
        println!("cargo:warning=libghostty-vt.a not found in {dir}; ghostty row will be skipped");
    }
    println!("cargo:rustc-check-cfg=cfg(have_ghostty)");
}
