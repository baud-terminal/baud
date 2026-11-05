//! The crate's build: one module a table under `build/`, each a function of
//! its inputs, and this `main` the one place that reads `data/` and writes
//! `OUT_DIR`. A failure in a table's data is printed as `cargo::error=` at
//! its file, line and column, every one of them, and fails the build.

#[path = "build/bidi.rs"]
mod bidi;
#[path = "build/contrast.rs"]
mod contrast;

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build");
    let crate_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let data = crate_dir.join("data");
    for input in &bidi::INPUTS {
        println!("cargo::rerun-if-changed=data/{}", input.file);
    }
    std::fs::write(out.join("contrast.bin"), contrast::build()).expect("write contrast.bin");
    let read = |file: &str| std::fs::read_to_string(data.join(file));
    match bidi::build(&read) {
        Ok(emit) => emit.write(&out).expect("write the bidi tables"),
        // Cargo fails the build on any `cargo::error=` line, after the
        // script has printed them all.
        Err(errs) => print!("{}", errs.directives()),
    }
}
