//! The crate's build: the cell shader's copy of the stress mark's skip
//! trie, `OUT_DIR/stress_skips.metal`, from the bytes `baud-render`'s
//! build compiled (`DEP_BAUD_RENDER_STRESS_OUT_DIR`, its `links` key), and
//! `OUT_DIR/shaders.metal`, `src/shaders.metal` with that file spliced
//! over its `#include`: the one text the runtime compiles and the
//! library's digest is of. `build/stress.rs` computes both; this `main` is
//! the one place that writes `OUT_DIR`. A failure is printed as
//! `cargo::error=` and fails the build.

#[path = "build/stress.rs"]
mod stress;

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build");
    println!("cargo::rerun-if-changed=src/shaders.metal");
    // A new SDK relinks the crate's tests, which make windows: Cargo does
    // not fingerprint SDKROOT, which the linker reads (`crates/baud-app/build.rs`).
    println!("cargo::rerun-if-env-changed=SDKROOT");
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
    let shaders = "src/shaders.metal";
    let dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let spliced = include.and_then(|text| {
        let src =
            std::fs::read_to_string(dir.join(shaders)).map_err(|e| format!("{shaders}: {e}"))?;
        stress::splice(shaders, &src, &text).map(|full| (text, full))
    });
    match spliced {
        Ok((text, full)) => {
            for (file, body) in [(stress::INCLUDE, text), ("shaders.metal", full)] {
                std::fs::write(out.join(file), body)
                    .unwrap_or_else(|e| panic!("write {file}: {e}"));
            }
        }
        Err(e) => println!("cargo::error={e}"),
    }
}
