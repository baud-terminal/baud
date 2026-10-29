//! The one thing the app's link asks for: drop the dylibs nothing uses.
//!
//! `otool -L` on the release binary listed CoreData, CoreImage and
//! CloudKit -- pulled in by the AppKit bindings' link directives and never
//! called -- and `/nix/store/...-libiconv-109.100.2/lib/libiconv.2.dylib`:
//! the `libc` crate's Apple module carries `#[link(name = "iconv")]` for
//! bindings this binary never calls (`nm -u` finds no iconv symbol), and the
//! nix toolchain's linker resolves that name to the store's copy rather
//! than `/usr/lib`'s. That last one is an absolute path
//! into this box's nix store, so the binary, and the `.app` bundle made
//! from it, could not launch on a machine without it: a portability defect
//! wearing the shape of a launch-time row. `-dead_strip_dylibs` makes the
//! linker omit a load command whose library exports nothing the binary
//! references, which removes all four; row 5 of note N measured what that
//! is worth at launch, and the answer is in `docs/research/notes/n-make-faster.md`'s
//! record.
//!
//! A build script rather than `.cargo/config.toml`: devenv exports
//! `RUSTFLAGS`, and an environment `RUSTFLAGS` replaces every `rustflags`
//! a config file sets, so a flag there would be silently gone under the
//! shell that runs every gate. `cargo:rustc-link-arg-bins` survives it.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // **AppKit has to be named as needed, or the strip takes it too.**
        // The Objective-C classes the app uses -- NSApplication, NSWindow,
        // the glass views -- are resolved *by name* through libobjc at run
        // time, so the linker sees no symbol referenced from AppKit and
        // `-dead_strip_dylibs` drops the framework that provides them; the
        // first stripped binary launched into "class NSWindow could not be
        // found". `-needed_framework` keeps the load command whatever the
        // linker can see. AppKit loads Foundation, CoreText and CoreVideo
        // as its own dependencies, so naming it is enough; Metal, QuartzCore,
        // IOSurface and CoreGraphics are referenced by symbol and stay.
        println!("cargo:rustc-link-arg-bins=-Wl,-needed_framework,AppKit");
        println!("cargo:rustc-link-arg-bins=-Wl,-dead_strip_dylibs");
        // **The floor travels with the crate.** `.cargo/config.toml` sets
        // the deployment target for a build in the repository, and a
        // published crate carries no `.cargo/config.toml`: `cargo install
        // baud` would link for rustc's default, macOS 11. Cargo hands a
        // build script's `rustc-env` to every rustc it runs for this
        // package, the link included, and rustc reads the variable from
        // there; `baud-app/tests/macos_floor.rs` holds it to the others.
        println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=26.0");
    }
}
