//! The differential against alacritty is a test target,
//! `tests/alacritty.rs`; this library is empty.
//!
//! Cargo refuses a package with no library and no binary, and baud has one
//! binary, `baud` (the owner, 2026-09-14: "the only bin should be and default
//! one is baud, rest is part of tests"). So the crate carries this empty
//! library, and `alacritty_terminal` and `baud-core` are dev-dependencies: a
//! `cargo build --workspace` compiles neither for it.
