# Embedding the core

The part of baud that understands what programs print is a library,
`baud-core`, and any program that needs to interpret a terminal's output can
use it: a terminal of your own, a test harness that checks what a program
draws, a recorder, or a tool that reads a screen. It is the same core the
baud app runs, held to the same standards
([vt-coverage](vt-coverage.md)) and measured against ghostty's and
alacritty's cores ([Performance](guide/performance.md)).

## What you get

- **A screen you can read.** Feed it bytes; read back the grid of cells, the
  cursor, the title, the scrollback behind the screen, and any reply the
  terminal owes the program, such as the answer to a cursor-position query.
- **No allocator needed.** `baud-core` is `no_std`: you give it the memory,
  sized to the screen, and it never allocates while it reads output. With
  the `alloc` feature, `Owned` provides that memory for you.
- **No `unsafe` in the crate.** The core forbids it; the SIMD scanners it
  calls live in separate crates behind safe wrappers.
- **Keys too.** `Term::key` encodes a key press the way the program asked
  for, including kitty's keyboard protocol and `modifyOtherKeys`
  ([input-coverage](input-coverage.md)).

## Getting it

`baud-core` is not published to crates.io. Depend on it from a checkout of
this repository, by path or by git revision.

## A first program

This block is `baud-core/examples/quickstart.rs`, which the build compiles,
and a test fails if the two drift apart:

```rust
use baud_core::{BytePos, Owned, ReadLen, Term, WinSize};

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
```

Run it with `cargo run -p baud-core --example quickstart`.

## Where to go next

- `Term::reply` and `Term::clear_reply`: the bytes to write back to the
  program after each feed.
- `Term::row`, `Term::cursor` and `Term::scrollback`: reading the screen and
  the history behind it.
- `Term::resize`: a new window size, with the primary screen's lines
  rewrapped.
- [The architecture](contributing/architecture.md): how the core is laid
  out and why.
