//! The crate's build: the cell shader's copy of the stress mark's skip
//! trie, from the bytes `baud-render`'s build compiled
//! (`DEP_BAUD_RENDER_STRESS_OUT_DIR`, its `links` key), computed by
//! `build/stress.rs` and written here, the one place that writes
//! `OUT_DIR`. A failure is printed as `cargo::error=` and fails the build.

#[path = "build/stress.rs"]
mod stress;

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let Some(render) = std::env::var_os("DEP_BAUD_RENDER_STRESS_OUT_DIR") else {
        println!("cargo::error=baud-render's build handed over no OUT_DIR");
        return;
    };
    let render = PathBuf::from(render);
    let read = |file: &str| {
        let path = render.join(file);
        println!("cargo::rerun-if-changed={}", path.display());
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let include = read("stress_skip_index.bin").and_then(|index| {
        read("stress_skip_blocks.bin").and_then(|blocks| stress::metal(&index, &blocks))
    });
    match include {
        Ok(text) => std::fs::write(out.join(stress::INCLUDE), text)
            .unwrap_or_else(|e| panic!("write {}: {e}", stress::INCLUDE)),
        Err(e) => println!("cargo::error={e}"),
    }
}
