//! The differential against alacritty is a test target,
//! `tests/alacritty.rs`; this library is empty.
//!
//! Cargo refuses a package with no library and no binary, and baud has one
//! binary, `baud`; the rest is part of the tests. So the crate carries this
//! empty library, and `alacritty_terminal` and `baud-core` are
//! dev-dependencies: a `cargo build --workspace` compiles neither for it.
