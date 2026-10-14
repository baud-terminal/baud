<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/baud-lockup-dark.svg">
  <img alt="baud" src="assets/logo/baud-lockup-light.svg" width="300">
</picture>

[![Nix](https://img.shields.io/badge/Nix-2b2b2b?logo=nixos&logoColor=white)](https://nixos.org)
[![devenv](https://img.shields.io/badge/devenv-2b2b2b?logo=nixos&logoColor=white)](https://devenv.sh)
[![License: CC BY-NC-SA 4.0](https://img.shields.io/badge/CC%20BY--NC--SA%204.0-2b2b2b?logo=creativecommons&logoColor=white)](LICENSE)

</div>

> A terminal emulator written in Rust. It runs on macOS 26 and later; the Linux
> port has run only headless.

Named for Émile Baudot, whose 1874 code gave the unit its name: a baud measures
how fast symbols cross a line, which is what this is about.

## What it is

baud implements the standards, specifications and protocols a terminal answers
to — ECMA-48, the DEC manuals, xterm's ctlseqs, the Unicode Standard and its
annexes, the font formats, kitty's keyboard protocol — with the goal of running
them faster than any shipping terminal on ingest throughput, input latency,
frame rate under full-grid churn and memory **at once**. The standard is the
requirement and speed is how it is met: a result that is faster because it
departs from a standard is a defect, and the departures known today are recorded
as such, per operation at **partial** in
[docs/vt-coverage.md](docs/vt-coverage.md) and as a list in
[docs/protocol-decisions.md](docs/protocol-decisions.md#open-departures).

Only ingest is measured against other terminals. The current sweep was taken
at `935e4f1` on 2026-09-20: Apple M5 Max, `bench-others/multibench` in memory,
medians of 9, interleaved and rotated, every core handed the same bytes in
4096-byte reads and 10,000 lines of scrollback. It predates `multibench`'s
128-byte function alignment, and MEASUREMENTS.md holds no cross-terminal sweep
taken after it. MB/s:

| recording | baud-core | ghostty | alacritty | **vs ghostty** |
|---|---|---|---|---|
| `ascii/less-pager.raw` | **1684.5** | 290.2 | 241.2 | **5.80x** |
| `ascii/cat-scroll.raw` | **1425.1** | 424.3 | 203.3 | **3.36x** |
| `ascii/git-log.raw` | **1637.9** | 516.9 | 211.5 | **3.17x** |
| `ascii/asciiquarium.raw` | **524.2** | 197.2 | 243.6 | **2.66x** |
| `utf8/less-cjk.raw` | **498.0** | 247.2 | 271.3 | **2.01x** |
| `utf8/vim-unicode.raw` | **636.4** | 346.8 | 293.0 | **1.84x** |
| `utf8/cava.raw` | **554.1** | 318.9 | 362.2 | **1.74x** |
| `utf8/btop.raw` | **871.3** | 511.2 | 330.9 | **1.70x** |
| `utf8/cat-cjk.raw` | **459.4** | 385.1 | 221.7 | **1.19x** |

baud-core is ahead of ghostty on all nine committed recordings, by 1.19x to
5.80x, and ahead of alacritty on all nine. baud always segments grapheme
clusters and libghostty-vt does only when an application asks, so the CJK rows
do not compare equal work. Input latency and frame time are baud's own numbers,
on one Mac, and no memory figure is compared with another terminal.
[MEASUREMENTS.md](MEASUREMENTS.md) has every table with its commit and caveats.

## What is built

On macOS: the VT core (parser, replies, and a grid that is a ring the screen is
a window into, so a scroll copies nothing and a resize reflows the primary);
UTF-8 decoding, widths generated from the Unicode data, and grapheme clustering;
key encoding with `modifyOtherKeys` and kitty's protocol, mouse reports,
selection and paste ([docs/input-coverage.md](docs/input-coverage.md)); a
renderer on the CPU and on Metal; shaping and rasterising in `baud-font` with no
engine dependency; and the wheel, a scrollbar and selection across the history.
What of the protocol is done, at **partial** or out, and why, is a row per
operation in [docs/vt-coverage.md](docs/vt-coverage.md). Some operations have no
row yet: the single shifts SS2/SS3 and the locking shifts LS2, LS3 and
LS1R-LS3R, which count as unsupported, and ECMA-48's bidirectional controls.

**Not built:** Linux past headless (no pointer input, nobody has looked at its
pixels, and its GPU half is unstarted); macOS key releases and modifier keys;
bidirectional text: UAX #9 and ECMA-48's bidirectional controls
([note AB](docs/research/notes/ab-right-to-left.md)); and search. No discrete
GPU has been measured: every GPU figure is this Mac's unified memory.

## Building and running

Requires [Nix](https://nixos.org) and [devenv](https://devenv.sh), which pin
the nightly toolchain.

```sh
devenv shell -- scripts/bundle-macos.sh          # target/baud.app
devenv shell -- cargo run --release              # the terminal, from the shell you are in
devenv test                                      # the whole gate, as CI runs it
```

[BUILD.md](BUILD.md) has the suites, the benchmarks and the gates, and
[CONTRIBUTING.md](CONTRIBUTING.md) what a change needs.

**Configuration** is optional: copy [config.toml.example](config.toml.example)
to `~/.config/baud/config.toml` and keep only the keys you change.
[BUILD.md](BUILD.md) lists every place baud looks.

## Embedding the core

`baud-core` is a `no_std` library, usable by any program that needs to
interpret a VT stream:

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

## Documentation

| | |
|---|---|
| [docs/README.md](docs/README.md) | the index of every document, and who each is for |
| [docs/vt-coverage.md](docs/vt-coverage.md) | what of the protocol is implemented, per operation, against primary sources |
| [docs/architecture.md](docs/architecture.md) | the data path, the crates and the constraints that shaped them |
| [MEASUREMENTS.md](MEASUREMENTS.md) | the current numbers, each with the host and commit that produced it |
| [docs/record.md](docs/record.md) | the dated record of every measured change and every decision |
| [CLAUDE.md](CLAUDE.md) | working notes: the rules, and what was tried and lost |

## Licence

baud is open source under Creative Commons Attribution-NonCommercial-ShareAlike
4.0 International (CC BY-NC-SA 4.0); see [LICENSE](LICENSE). The fonts and the
third-party files the research and tests hold are under their own licences:
LICENSE names the files it does not cover, and
[THIRD_PARTY.md](THIRD_PARTY.md) lists each with its licence and source.
Nothing is published to a registry, so build from source.
[FUNDING.md](FUNDING.md) covers sponsorship and how to arrange a commercial
licence.
