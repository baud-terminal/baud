//! VT52 — a different protocol, reached through DECANM.
//!
//! `[[source: vt510-decanm]]`: "DECANM changes the terminal to the VT52 mode of
//! operation. In VT52 mode, the terminal acts like a VT52 terminal."
//!
//! **VT52 is not a subset of ANSI, and that is the whole reason it is a mode.**
//! The same bytes mean different things in each: `ESC D` is IND in ANSI and
//! cursor-*left* in VT52; `ESC H` is HTS in ANSI and cursor-*home* in VT52. A
//! parser serving both from one table is necessarily wrong for one of them, so
//! these tests check both readings of the same byte.

#[path = "../support/mod.rs"]
mod support;

mod sequences;
