//! **The cell path's shaders read `CU` where Rust writes it**: the block in
//! every cell stage's SPIR-V has `cell_frame::CU_NAMES`' words, in that
//! order, each four bytes after the last, 152 bytes in all, at set 0 binding
//! 4 -- the layout `baud-render` holds the Metal declaration to as well, so
//! the three cannot fall out of step.

mod spirv;

use baud_render::cell_frame::{CU_NAMES, CU_WORDS};
use spirv::{Binding, DescriptorSet, block};
use std::path::PathBuf;

/// The cell path's stages, by the SPIR-V the crate embeds.
const STAGES: [&str; 4] = [
    "cell_bg.vert",
    "cell_glyph.vert",
    "cell_shape.vert",
    "cell_ring.vert",
];

fn spv(stage: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders/spv")
        .join(format!("{stage}.spv"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `CU_NAMES` as SPIR-V names a block's members: a `uint2`'s halves are one
/// `uvec2` member at the first half's offset.
fn members() -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for (i, name) in CU_NAMES.iter().enumerate() {
        let at = u32::try_from(4 * i).expect("an offset");
        match name.split_once('.') {
            Some((pair, "x")) => out.push((pair.to_string(), at)),
            Some(_) => {}
            None => out.push((name.to_string(), at)),
        }
    }
    out
}

/// **Every cell stage's `CU` is `CU_NAMES` at its offsets**, at set 0 binding
/// 4. Named break: a field moved or added on one side.
#[test]
fn every_cell_stage_reads_cu_where_rust_writes_it() {
    assert_eq!(CU_WORDS * 4, 152);
    for stage in STAGES {
        let b = block(&spv(stage), "CU").unwrap_or_else(|| panic!("{stage}: no CU block"));
        assert_eq!(b.members, members(), "{stage}");
        assert_eq!(
            (b.set, b.binding),
            (DescriptorSet(0), Binding(4)),
            "{stage}"
        );
    }
}

/// The bytes of `const uint <name>[n] = uint[](...)` in `src`, each word's
/// low byte first.
fn table(src: &str, name: &str) -> Vec<u8> {
    let open = format!("const uint {name}[");
    let at = src
        .find(&open)
        .unwrap_or_else(|| panic!("{name} is declared"));
    let rest = &src[at..];
    let body = &rest[rest.find("uint[](").expect("an initialiser") + 7..];
    let body = &body[..body.find(");").expect("the initialiser closes")];
    body.split(',')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .flat_map(|w| {
            let hex = w.trim_start_matches("0x").trim_end_matches('u');
            u32::from_str_radix(hex, 16)
                .unwrap_or_else(|e| panic!("{w}: {e}"))
                .to_le_bytes()
        })
        .collect()
}

/// `bytes` as the GLSL table `name`, four to a word, the first in the low
/// eight bits, the last word padded with zeros.
fn glsl_table(name: &str, bytes: &[u8]) -> String {
    let words: Vec<String> = bytes
        .chunks(4)
        .map(|c| {
            let mut w = [0u8; 4];
            w[..c.len()].copy_from_slice(c);
            format!("0x{:08x}u", u32::from_le_bytes(w))
        })
        .collect();
    let lines: Vec<String> = words
        .chunks(8)
        .map(|l| format!("    {}", l.join(", ")))
        .collect();
    format!(
        "const uint {name}[{}] = uint[](\n{}\n);\n",
        words.len(),
        lines.join(",\n")
    )
}

fn stress_skips() -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders/stress_skips.glsl"),
    )
    .expect("shaders/stress_skips.glsl")
}

/// **The shape stage's stress-mark trie is `baud-render`'s**, byte for
/// byte, the padding of the last word zero. Named break: the Unicode tables
/// regenerated in `baud-render` and not here. Regenerate with
/// `cargo test -p baud-present-vulkan --test cell_shaders -- --ignored
/// write_the_stress_tables`.
#[test]
fn the_stress_tables_are_baud_renders() {
    let src = stress_skips();
    for (name, want) in [
        ("STRESS_SKIP_INDEX", &baud_render::STRESS_SKIP_INDEX[..]),
        ("STRESS_SKIP_BLOCKS", &baud_render::STRESS_SKIP_BLOCKS[..]),
    ] {
        let got = table(&src, name);
        assert!(
            got.len() >= want.len(),
            "{name}: {} bytes, want {}",
            got.len(),
            want.len()
        );
        assert_eq!(&got[..want.len()], want, "{name}");
        assert!(got[want.len()..].iter().all(|&b| b == 0), "{name}: padding");
        assert!(
            got.len() - want.len() < 4,
            "{name}: more than a word of padding"
        );
    }
}

/// Writes `shaders/stress_skips.glsl` from `baud-render`'s tables.
#[test]
#[ignore = "a generator: run by name when baud-render's tables change"]
fn write_the_stress_tables() {
    let src = stress_skips();
    let head = &src[..src
        .find("const uint STRESS_SKIP_INDEX[")
        .expect("the first table")];
    let readers = &src[src.rfind(");").expect("the last table") + 3..];
    let out = format!(
        "{head}{}\n{}{readers}",
        glsl_table("STRESS_SKIP_INDEX", baud_render::STRESS_SKIP_INDEX),
        glsl_table("STRESS_SKIP_BLOCKS", baud_render::STRESS_SKIP_BLOCKS),
    );
    std::fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders/stress_skips.glsl"),
        out,
    )
    .expect("written");
}
