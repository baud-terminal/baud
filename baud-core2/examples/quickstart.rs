//! The README quickstart, compiled.
//!
//! `baud-core2/tests/readme.rs` asserts this file's body and the README's Rust
//! block are the same text, so the example in the README cannot rot: it is
//! built by `cargo check --all-targets`, which the gate runs.

use baud_core2::{OpMode, Storage, StreamByte, Term};

fn main() {
    // Storage is caller-owned and fixed-size: no allocator on the ingest path.
    let mut store = Storage::boxed();
    let mut term = Term::new(&mut store, OpMode::Computed);

    // Feed bytes exactly as they arrive from read(2) — split anywhere.
    let mut slot = term.read_slot();
    let base = slot.base();
    let filled = slot.write(StreamByte::slice_from(b"\x1b[1;32mhello\x1b[m"));
    term.feed(base, filled.len());
    term.commit_read(filled.len());

    // A terminal is not write-only: drain anything it owes the host.
    let owed = term.reply().len(); // e.g. the answer to a DSR
    let cells = term.viewport().len(); // the materialised grid
    println!("{owed} bytes owed, {cells} cells");
}
