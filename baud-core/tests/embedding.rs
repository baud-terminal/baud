#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// the blanking test in `amplification.rs` -- then named
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, and since
// renamed because the decision it pinned was reversed by measurement -- whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! The embedding guide's Rust example is the example file, not a copy of it.
//!
//! A usage snippet that does not compile is the same class of false claim as a
//! coverage table nothing checks — and examples in prose rot silently, because
//! nothing builds them. `baud-core/examples/quickstart.rs` *is* built, by
//! `cargo check --all-targets`. This test asserts `docs/embedding.md` shows
//! that file and not a drifted paraphrase of it. The block lived in the README
//! until 2026-10-16, when embedding got its own page and the README a link.

use std::fs;

fn read(rel: &str) -> String {
    fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The body of the example, minus its module docs, `use`, and `fn main`
/// scaffolding — the part a reader is meant to copy.
fn example_body() -> Vec<String> {
    let src = read("examples/quickstart.rs");
    let start = src.find("fn main() {").expect("example must have a main");
    let inner = &src[start + "fn main() {".len()..];
    let end = inner.rfind('}').expect("unterminated main");
    inner[..end]
        .lines()
        .map(|l| l.strip_prefix("    ").unwrap_or(l).to_string())
        // The example prints so it can run; the guide does not need that.
        .filter(|l| !l.starts_with("println!"))
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn guide_body() -> Vec<String> {
    let src = read("../docs/embedding.md");
    let start = src
        .find("```rust")
        .expect("docs/embedding.md must show a Rust example");
    let rest = &src[start + "```rust".len()..];
    let end = rest.find("```").expect("unterminated code fence");
    rest[..end]
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with("use "))
        .collect()
}

#[test]
fn the_embedding_guide_example_is_the_compiled_example() {
    let ex = example_body();
    let rd = guide_body();
    assert!(
        ex.len() > 5,
        "extracted {} lines from the example -- the parser stopped matching it",
        ex.len()
    );
    assert_eq!(
        rd, ex,
        "docs/embedding.md's Rust block has drifted from \
         baud-core/examples/quickstart.rs.\nThe example is the one that compiles; \
         update the guide to match it."
    );
}
