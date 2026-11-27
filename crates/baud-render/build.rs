//! The crate's build: one module a table under `build/`, each a function of
//! its inputs, and this `main` the one place that reads `data/` and writes
//! `OUT_DIR`. A failure in a table's data is printed as `cargo::error=` at
//! its file, line and column, every one of them, and fails the build.
//!
//! `OUT_DIR` is also handed to the crates that depend on this one, as
//! `DEP_BAUD_RENDER_STRESS_OUT_DIR` (the `links` key): `baud-present-macos`'s
//! build writes the stress skip trie's bytes from it into the cell shader.

#[path = "build/stress.rs"]
mod stress;

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build");
    let crate_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let data = crate_dir.join("data");
    for input in stress::INPUTS {
        println!("cargo::rerun-if-changed=data/{}", input.file);
    }
    let read = |file: &str| std::fs::read_to_string(data.join(file));
    match stress::build(&read) {
        Ok(emit) => emit
            .write(&out)
            .unwrap_or_else(|e| panic!("write the stress tables: {e}")),
        // Cargo fails the build on any `cargo::error=` line, after the
        // script has printed them all.
        Err(errs) => print!("{}", errs.directives()),
    }
    println!("cargo::metadata=out_dir={}", out.display());
}
