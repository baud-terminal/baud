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
//! references, which removes all four.
//!
//! A build script rather than `.cargo/config.toml`: devenv exports
//! `RUSTFLAGS`, and an environment `RUSTFLAGS` replaces every `rustflags`
//! a config file sets, so a flag there would be silently gone under the
//! shell that runs every gate. `cargo:rustc-link-arg-bins` survives it.

//!
//! **And the build's name**: `BAUD_VERSION`, `<package version>+<identifier>`,
//! the logic in `src/build_id.rs`, and `BAUD_RELEASED`, its commit's time in
//! seconds since 1970 or empty, which the About window's age line counts
//! from. It writes no file; a `rustc-env` is all the tree sees of either.

#[path = "src/build_id.rs"]
mod build_id;

use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

/// The build's identifier and its commit's time, and what Cargo should
/// rerun this for. A packager that names the commit names its time too, or
/// none: git is not asked for half of the answer.
fn identifier() -> (String, Option<SystemTime>) {
    println!("cargo:rerun-if-changed=src/build_id.rs");
    println!("cargo:rerun-if-env-changed=BAUD_GIT_REV");
    println!("cargo:rerun-if-env-changed=BAUD_GIT_TIME");
    if let Some(rev) = std::env::var("BAUD_GIT_REV").ok().filter(|r| !r.is_empty()) {
        let id = build_id::told(&rev).unwrap_or_else(|e| panic!("{e}"));
        let time = std::env::var("BAUD_GIT_TIME").map_or(Ok(None), |t| build_id::told_time(&t));
        return (id, time.unwrap_or_else(|e| panic!("{e}")));
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let crate_dir = std::fs::canonicalize(&manifest).expect("the package's folder");
    let git = |args: &[&str]| {
        // Asked of the crate's folder, whatever a hook this build runs
        // under has exported.
        let out = Command::new("git")
            .arg("-C")
            .arg(&crate_dir)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    match build_id::read(Path::new(&crate_dir), &git) {
        Some(read) => {
            // Canonical, as git's paths are: `CARGO_TARGET_DIR` is taken as
            // written, `..` and symbolic links included.
            let out = std::env::var("OUT_DIR").expect("cargo sets it");
            let out = std::fs::canonicalize(&out).expect("cargo made it");
            let entries = |dir: &Path| {
                std::fs::read_dir(dir).map_or_else(
                    |_| Vec::new(),
                    |entries| entries.flatten().map(|e| e.path()).collect(),
                )
            };
            for path in build_id::rerun_on(&read, &out, &|p| p.exists(), &entries) {
                println!("cargo:rerun-if-changed={}", path.display());
            }
            (read.id, read.time)
        }
        None => (build_id::UNKNOWN.to_owned(), None),
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let package = std::env::var("CARGO_PKG_VERSION").expect("cargo sets it");
    let (id, time) = identifier();
    println!(
        "cargo:rustc-env=BAUD_VERSION={}",
        build_id::full(&package, &id)
    );
    println!(
        "cargo:rustc-env=BAUD_RELEASED={}",
        build_id::stamp(&id, time)
    );
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
