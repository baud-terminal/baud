//! **`CU`'s words are the shaders' `CU`**: `cell_frame::CU_NAMES` against the
//! Metal cell path's declaration, read from `shaders.metal` as written, so a
//! field added, removed or moved on one side fails here on any platform.

use baud_render::cell_frame::{CU_NAMES, CU_WORDS};

/// The words a C-like struct declaration's body names, a `uint2` as `.x` then
/// `.y`, comments dropped.
fn words(body: &str) -> Vec<String> {
    let text: String = body
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = Vec::new();
    for decl in text.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let mut parts = decl.split_whitespace();
        let (Some(ty), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        match ty {
            "uint2" | "uvec2" => {
                out.push(format!("{name}.x"));
                out.push(format!("{name}.y"));
            }
            _ => out.push(name.to_string()),
        }
    }
    out
}

/// The body of `struct <name> { ... };` in `src`.
fn body<'s>(src: &'s str, open: &str) -> &'s str {
    let at = src
        .find(open)
        .unwrap_or_else(|| panic!("{open} is declared"));
    let rest = &src[at + open.len()..];
    &rest[..rest.find('}').expect("the declaration closes")]
}

#[test]
fn the_cell_uniform_words_are_the_metal_cu() {
    let metal = include_str!("../../baud-present-macos/src/shaders.metal");
    let declared = words(body(metal, "struct CU {"));
    assert_eq!(declared.len(), CU_WORDS, "{declared:?}");
    assert_eq!(declared, CU_NAMES.map(str::to_string).to_vec());
}
