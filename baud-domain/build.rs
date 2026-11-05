//! The crate's build: one module a table under `build/`, each a function of
//! its inputs, and this `main` the one place that reads `data/` and writes
//! `OUT_DIR`. A failure in a table's data is printed as `cargo::error=` at
//! its file, line and column, every one of them, and fails the build.

#[path = "build/bidi.rs"]
mod bidi;
#[path = "build/contrast.rs"]
mod contrast;
#[path = "build/grapheme.rs"]
mod grapheme;
#[path = "build/linebreak.rs"]
mod linebreak;
#[path = "build/ucd.rs"]
mod ucd;
#[path = "build/width.rs"]
mod width;

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
        &bidi::INPUTS[..],
        &grapheme::INPUTS,
        &width::INPUTS,
        &linebreak::INPUTS,
    ]
    .iter()
    .flat_map(|inputs| inputs.iter().map(|i| i.file))
    .collect();
    files.sort_unstable();
    files.dedup();
    for file in files {
        println!("cargo::rerun-if-changed=data/{file}");
    }
    std::fs::write(out.join("contrast.bin"), contrast::build()).expect("write contrast.bin");
    let read = |file: &str| std::fs::read_to_string(data.join(file));
    let tables: [(&str, Table); 4] = [
        ("bidi", bidi::build),
        ("grapheme", grapheme::build),
        ("width", width::build),
        ("linebreak", linebreak::build),
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
