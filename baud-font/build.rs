//! The crate's build: one module a table under `build/`, each a function of
//! its inputs, and this `main` the one place that reads `data/` and writes
//! `OUT_DIR`. A failure in a table's data is printed as `cargo::error=` at
//! its file, line and column, every one of them, and fails the build.

#[path = "build/compose.rs"]
mod compose;
#[path = "build/ignorable.rs"]
mod ignorable;
#[path = "build/indic.rs"]
mod indic;
#[path = "build/presentation.rs"]
mod presentation;
#[path = "build/script.rs"]
mod script;
#[path = "build/standard_encoding.rs"]
mod standard_encoding;

use baud_tablegen::{Emit, Errors};
use std::path::PathBuf;

/// A table's build: its inputs read through the function it is given.
type Table = fn(&dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors>;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build");
    let crate_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let data = crate_dir.join("data");
    let mut files: Vec<&str> = [
        &compose::INPUTS[..],
        &ignorable::INPUTS,
        &indic::INPUTS,
        &presentation::INPUTS,
        &script::INPUTS,
        &standard_encoding::INPUTS,
    ]
    .iter()
    .flat_map(|inputs| inputs.iter().map(|i| i.file))
    .collect();
    files.sort_unstable();
    files.dedup();
    for file in files {
        println!("cargo::rerun-if-changed=data/{file}");
    }
    let read = |file: &str| std::fs::read_to_string(data.join(file));
    let tables: [(&str, Table); 6] = [
        ("compose", compose::build),
        ("ignorable", ignorable::build),
        ("indic", indic::build),
        ("presentation", presentation::build),
        ("script", script::build),
        ("standard encoding", standard_encoding::build),
    ];
    for (name, build) in tables {
        match build(&read) {
            Ok(emit) => emit
                .write(&out)
                .unwrap_or_else(|e| panic!("write the {name} tables: {e}")),
            // Cargo fails the build on any `cargo::error=` line, after the
            // script has printed them all.
            Err(errs) => print!("{}", errs.directives()),
        }
    }
}
