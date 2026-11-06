//! **The compiled tables are the committed ones**: `build/tcap.rs` and
//! `build/iterm_keys.rs`' output in `OUT_DIR` against the files the Python
//! generators wrote, before those files are deleted, element for element
//! and as text, the header comments aside.

extern crate std;
use std::string::String;

/// A generated file's Rust: every line after its leading `//` comments.
pub(crate) fn body(text: &str) -> String {
    text.lines()
        .skip_while(|l| l.starts_with("//"))
        .flat_map(|l| [l, "\n"])
        .collect()
}

#[test]
fn the_compiled_tcap_and_iterm2_tables_are_the_committed_ones() {
    use super::{iterm_keys as i0, iterm_keys_built as i1, tcap_built as t1, tcap_data as t0};
    assert_eq!(t1::MUL, t0::MUL, "MUL");
    assert_eq!(t1::ROWS.len(), t0::ROWS.len(), "ROWS");
    for (i, (a, b)) in t1::ROWS.iter().zip(t0::ROWS.iter()).enumerate() {
        assert_eq!(a, b, "ROWS[{i}]");
    }
    assert_eq!(i1::DOC_KEYS, i0::DOC_KEYS, "DOC_KEYS");
    assert_eq!(i1::SOURCE_KEYS, i0::SOURCE_KEYS, "SOURCE_KEYS");
    for (name, built, committed) in [
        (
            "tcap_data.rs",
            include_str!(concat!(env!("OUT_DIR"), "/tcap_data.rs")),
            include_str!("tcap_data.rs"),
        ),
        (
            "iterm_keys.rs",
            include_str!(concat!(env!("OUT_DIR"), "/iterm_keys.rs")),
            include_str!("iterm_keys.rs"),
        ),
    ] {
        assert_eq!(body(built), body(committed), "{name}");
    }
    std::println!(
        "identical: MUL {:#018x}, ROWS {}, DOC_KEYS {}, SOURCE_KEYS {}; both files' Rust as text",
        t1::MUL,
        t1::ROWS.len(),
        i1::DOC_KEYS.len(),
        i1::SOURCE_KEYS.len()
    );
}
