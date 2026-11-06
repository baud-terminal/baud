//! The crate's build: one module a table under `build/`, each a function of
//! its inputs, and this `main` the one place that reads `data/` and writes
//! `OUT_DIR`. A failure in a table's data is printed as `cargo::error=` at
//! its file, line and column, every one of them, and fails the build.

#[path = "build/bidi.rs"]
mod bidi;
#[path = "build/caseless.rs"]
mod caseless;
#[path = "build/cluster.rs"]
mod cluster;
#[path = "build/colour_names.rs"]
mod colour_names;
#[path = "build/contrast.rs"]
mod contrast;
#[path = "build/dec_graphics.rs"]
mod dec_graphics;
#[path = "build/dec_sets.rs"]
mod dec_sets;
#[path = "build/grapheme.rs"]
mod grapheme;
#[path = "build/ignorable.rs"]
mod ignorable;
#[path = "build/iso96.rs"]
mod iso96;
#[path = "build/joining.rs"]
mod joining;
#[path = "build/linebreak.rs"]
mod linebreak;
#[path = "build/merged.rs"]
mod merged;
#[path = "build/nrcs.rs"]
mod nrcs;
#[path = "build/placeholder.rs"]
mod placeholder;
#[path = "build/search.rs"]
mod search;
#[path = "build/sequence.rs"]
mod sequence;
#[path = "build/ucd.rs"]
mod ucd;
#[path = "build/width.rs"]
mod width;
#[path = "build/xterm.rs"]
mod xterm;

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
        &caseless::INPUTS,
        &search::INPUTS,
        &sequence::INPUTS,
        &joining::INPUTS,
        &ignorable::INPUTS,
        &colour_names::INPUTS,
        &placeholder::INPUTS,
        &dec_sets::INPUTS,
        &nrcs::INPUTS,
        &iso96::INPUTS,
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
    let tables: [(&str, Table); 16] = [
        ("bidi", bidi::build),
        ("grapheme", grapheme::build),
        ("width", width::build),
        ("linebreak", linebreak::build),
        ("caseless", caseless::build),
        ("search", search::build),
        ("sequence", sequence::build),
        ("joining", joining::build),
        ("ignorable", ignorable::build),
        ("kitty colour names", colour_names::kitty),
        ("x colour names", colour_names::x),
        ("placeholder", placeholder::build),
        ("dec graphics", dec_graphics::build),
        ("dec sets", dec_sets::build),
        ("nrcs", nrcs::build),
        ("iso96", iso96::build),
    ];
    let mut built: Vec<(&str, Result<Emit, Errors>)> = tables
        .into_iter()
        .map(|(name, build)| (name, build(&read)))
        .collect();
    // The merged table is compiled from the grapheme and width tables as
    // they were just built, and the cluster machine from the rules alone.
    let emit = |name: &str| {
        built
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, r)| r.as_ref().ok())
    };
    if let (Some(g), Some(w)) = (emit("grapheme"), emit("width")) {
        let m = merged::build(g, w);
        built.push(("merged", m));
    }
    built.push(("cluster", cluster::build()));
    for (name, built) in built {
        match built {
            Ok(emit) => emit
                .write(&out)
                .unwrap_or_else(|e| panic!("write the {name} tables: {e}")),
            // Cargo fails the build on any `cargo::error=` line, after the
            // script has printed them all.
            Err(errs) => print!("{}", errs.directives()),
        }
    }
}
