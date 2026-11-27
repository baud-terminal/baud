//! The embedding guide's quickstart, compiled.
//!
//! `crates/baud-core/tests/embedding.rs` asserts this file's body and the Rust block
//! in `docs/embedding.md` are the same text, so the example cannot rot: it is
//! built by `cargo check --all-targets`, which the gate runs.
//!
//! **The README used to show `baud-core2`'s API under `use baud_core`** --
//! `Storage::boxed`, `OpMode`, `read_slot`/`commit_read` -- none of which this
//! crate has. It named the right crate and showed the wrong one, and nothing
//! caught it because the gate lived beside the other core and compared against
//! *its* example. Moving the gate is what found it.

use baud_core::{BytePos, Owned, ReadLen, Term, WinSize};

fn main() {
    // Storage is the caller's, and its size is the geometry: no allocator on
    // the ingest path. `Owned` is the convenience behind the `alloc` feature;
    // without it, `Storage::new` takes the same buffers as borrows.
    let ws = WinSize {
        ws_row: 50,
        ws_col: 200,
    };
    let mut store = Owned::new(&ws);
    let mut term = Term::new(store.storage(), &ws);

    // Feed bytes exactly as they arrive from read(2) -- split anywhere. The
    // core never copies on the parse path: a read writes into the arena and
    // `feed` parses it in place.
    let bytes = b"\x1b[1;32mhello\x1b[m";
    term.write_at(BytePos::ZERO, bytes);
    term.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

    // A terminal is not write-only: drain anything it owes the host.
    let owed = term.reply().len(); // e.g. the answer to a DSR
    let cells = term.cells().len(); // the grid, ring and all
    println!("{owed} bytes owed, {cells} cells");
}
