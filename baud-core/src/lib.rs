//! The terminal core, as a block-driven graph.
//!
//! **Why this crate exists beside `baud-core2`.** The previous core reached
//! 456 MB/s on `cat-cjk.raw` and could not be made materially faster by
//! optimising anything it *did*: ceiling arms priced every decode, every table
//! lookup, every cluster decision, every cell write and every scroll at **45%
//! between them**, so removing all of it left the recording at ~660. What
//! remained was the ingest loop, and the corpus said its cost is per
//! *iteration*: `cat`, which iterates once per printable run, floors at 2830,
//! and `cat-cjk`, which iterates once per 2.23-byte character, floors at 670.
//!
//! Iterating sixteen bytes at a time instead measured **14,100** on the same
//! nothing -- 21x. That is the one mechanism in this project's history whose
//! arithmetic reaches 1000 MB/s on `cat-cjk`, and this crate is built around
//! it rather than around a faster version of the per-character path.
//!
//! `CLAUDE.md` carries the measurements; `docs/research/notes/b-parser-as-a-graph.md`
//! carries the literature on storing the automaton.
//!
//! **What is reused and what is not.** Nothing comes from `baud-core2`. The
//! foundation is `baud-domain` -- the grid, the cell, the generated Unicode
//! tables and the VT vocabulary, all of which are already compile-time
//! lookups and were worth +17% -- and `baud-isa` for the vector primitives.

#![no_std]
#![forbid(unsafe_code)]

mod feed;
mod graph;
mod paint;
mod storage;
mod term;
mod write;

pub use baud_domain::{
    ByteLen, BytePos, Charset, Codepoint, Col, Cols, Content, InputEncoding, PackedCell, Row,
    RowBlock, RowExtent, Rows, StreamByte, Width, WinSize,
};
pub use graph::Node;
pub use storage::{ARENA, GRID, MAX_COLS, MAX_ROWS, READ_CHUNK, Storage};
pub use term::Term;
