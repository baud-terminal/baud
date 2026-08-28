<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/baud-lockup-dark.svg">
  <img alt="baud" src="assets/logo/baud-lockup-light.svg" width="300">
</picture>

</div>

<div align="center">

[![crates.io](https://img.shields.io/crates/v/baud?logo=rust&logoColor=white)](https://crates.io/crates/baud)
[![Downloads](https://img.shields.io/crates/d/baud?logo=rust&logoColor=white)](https://crates.io/crates/baud)
[![MSRV](https://img.shields.io/crates/msrv/baud?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![CI](https://img.shields.io/github/actions/workflow/status/i-am-logger/baud/ci.yml?branch=master&label=CI&logo=githubactions&logoColor=white)](https://github.com/i-am-logger/baud/actions/workflows/ci.yml)
[![docs.rs](https://img.shields.io/docsrs/baud?logo=docsdotrs&logoColor=white)](https://docs.rs/baud)

[![Nix](https://img.shields.io/badge/Nix-2b2b2b?logo=nixos&logoColor=white)](https://nixos.org)
[![devenv](https://img.shields.io/badge/devenv-2b2b2b?logo=nixos&logoColor=white)](https://devenv.sh)
[![License: CC BY-NC-SA 4.0](https://img.shields.io/badge/CC%20BY--NC--SA%204.0-2b2b2b?logo=creativecommons&logoColor=white)](LICENSE)

</div>

> A terminal emulator written in Rust, under construction.

Named for Émile Baudot, whose 1874 code gave the unit its name. The lineage runs
teletype → glass tty → terminal emulator, and a baud measures how fast symbols
cross a line — which is what this is about.

## Status

**Under construction, and the hard half is done.** The VT protocol core — the
part that turns a byte stream from a PTY into a grid — is complete, measured,
and faster than every implementation it has been compared against. What is
missing is everything between that grid and a window.

| | |
|---|---|
| **VT protocol** | ✅ 149 operations, [enumerated against primary sources](docs/vt-coverage.md) |
| **Parser** | ✅ all 13 DEC ANSI states, chunk-invariant, property-tested |
| **Grid** | ✅ two storage representations, both measured |
| **Throughput** | ✅ [2.19× ghostty on spans, 1.50× on cells](#the-number) — ahead in both representations, on both payloads |
| Scrollback | ◐ stored and measured, `SB = 10_000` lines — but no accessor reads above the viewport, so it is storage rather than a feature |
| Input path | ◐ started — sources pinned, [112 key rows enumerated](docs/input-coverage.md), types in; no encoder yet |
| Renderer | ⬜ not started |
| Font shaping | ⬜ not started — and the largest unknown in the project |

So: you cannot run this as your terminal today. You can run its core, feed it a
real escape stream, and measure it — which is what the rest of this file is
about.

## The road to a terminal

Two questions decide whether the measurements above survive contact with a real
terminal, and they are the next work:

1. **Shaping.** Wire HarfBuzz and a glyph atlas into viewport materialisation,
   then re-measure on mixed CJK/emoji/ligature content. If throughput holds
   above ~50% of the kernel ceiling and full-damage frames stay under 2 ms, the
   rest is engineering. If shaping drops it to 15%, the architecture needs a
   shaping cache keyed on line content before anything else matters.
2. **Real streams.** Replay captured sessions (`script -c "vim ..."`) instead of
   synthetic payloads, and find out whether the margin survives real escape
   traffic.

After those: renderer, then an input path, then it is a terminal.

## Running it today

There is no terminal binary yet — see [Status](#status). What exists is the
core and the harness that measures it:

```sh
devenv shell -- dev-test                       # the whole suite
devenv shell -- cargo run --release -p baud-bench   # feed it through a real PTY
```

## Embedding the core

`baud-core` is a `no_std` library and is usable now — it is the engine the
terminal will be built on, and it is published because it is useful on its own
for anything that needs to interpret a VT stream.

```rust
use baud_core::{OpMode, Storage, StreamByte, Term};

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
```

`OpMode` picks the representation and is fixed at startup:
`Computed` and `Templated` materialise cells; `Runs` keeps byte spans and
materialises on demand. In `Runs`, `viewport()` reports *representation*, not
rendered content — see [architecture](docs/architecture.md).

## Why it exists

Every result here is predicted by two quantities:

1. **Write amplification ≤ 1** — bytes written to memory per byte ingested.
2. **Working set ≤ cache** — the structures touched per byte stay resident.

Allocation is a *proxy* for these, not the thing. An immutable flat log that
allocated nothing still cost 22%, because the array grew past cache.

## The number

*Apple M5 Max, 2026-08-28, commit `0bcd659`, **on mains**, in-memory transport,
medians of 9, interleaved, two runs quoted. ghostty is built from an unchanged
checkout and is the control. Full table and every caveat in
[MEASUREMENTS.md](MEASUREMENTS.md).*

| core | streaming | TUI redraw |
|---|---|---|
| **baud** (spans) | **2510.3 / 2416.0** MB/s | **2887.8 / 2761.3** MB/s |
| **baud** (cells, computed) | **1706.7 / 1661.8** | **1693.3 / 1547.1** |
| ghostty (libghostty-vt) | 1140.0 / 1101.6 | 1066.2 / 1021.1 |
| termwiz (wezterm) parse only | 379.8 / 375.8 | 369.5 / 360.8 |
| alacritty_terminal | 225.8 / 202.5 | 276.7 / 268.2 |
| vt100 crate | 166.9 / 160.5 | 217.0 / 204.9 |
| wezterm (full terminal) | 17.7 / 17.5 | 32.3 / 31.7 |

**Spans: 2.19x ghostty streaming, 2.70x TUI. Cells: 1.50x and 1.55x.**

## The synthetic payloads went **down** while every real recording went up

This is the most important caveat on the table above, and it is not a small
one. Over one session, measured with `ingest` against the same starting commit:

| real capture | before | after | |
|---|---|---|---|
| `less-pager.raw` | 26.6 | **931** | **35x** |
| `vim-unicode.raw` | 246 | **343** | **+40%** |
| `btop.raw` | 452 | **550** | **+22%** |
| `cat-scroll.raw` | 778 | **841** | +8% |
| `git-log.raw` | 990 | 990 | flat |
| `cat-cjk.raw`, `less-cjk.raw` | *did not exist* | 194, 205 | — |

| synthetic payload | before | after | |
|---|---|---|---|
| `payload::streaming`, cells | 1327 | 1241 | **-6.5%** |
| `payload::streaming`, runs | 1714 | 1632 | **-4.8%** |
| `payload::tui`, cells | 1414 | 1307 | **-7.6%** |
| `payload::tui`, runs | 1470 | 1226 | **-16.6%** |

Two causes, and neither is avoidable:

* **Correctness.** Every operation in baud could split a double-width character
  in half, and erases did nothing at all on the alternate screen in `Runs`
  mode. Fixing both costs work on every write, and the synthetic payloads are
  pure ASCII, so they pay it and collect none of it.
* **A used-extent mark** makes blanking stop where the text stops. It is worth
  72-82% of all blanking on real scrolling capture and **16%** on
  `payload::streaming`, which emits an SGR every 232 bytes and keeps defeating
  the check. `payload::tui` barely scrolls at all, so it pays the bookkeeping
  and collects nothing.

**So read the table above as a comparison, not as a trend.** It is the only
number here measured against other implementations, because multibench drives
every core with the same payload -- but `payload::tui`'s escape density matches
no real program, and `MEASUREMENTS.md` records real capture running at 131-159
MB/s where it reaches 229-240. The recordings are the better guide to what
changed, and they are all in `baud-core/tests/recordings/`.

**The largest single change was deleting a measurement, not writing code.**
`Scan::run` was a scalar byte loop carrying a comment that a `core::simd`
version had "measured a 12% regression". That measurement was taken on a
harness which reports the same ~230 MB/s for builds differing by 2.5x, and it
had kept the hottest loop in the project scalar. Re-measured properly it is
**+41% on spans**. The old claim was half right and generalised: runs really
are short on escape-dense capture, where the vector version is flat -- but not
on text, which is what `cat`, logs and build output are.

**Cells used to be the weak half and no longer is.** Earlier editions of this
file said "level on cells", then "behind on cells" at 908 against 1124 -- both
true when written. Three changes closed it: `put_ascii_run` was a scalar loop
writing one eight-byte cell per store while the blanking path beside it used
`slice::fill` and vectorised; a 1 MB template buffer was filled on every style
miss and read by nothing; and the grapheme lookback ran two binary searches per
codepoint that nothing guarded. On both payloads the ranges no longer overlap
ghostty's, in either run.

Both modes ship; `OpMode` is fixed at startup.

## Layout

```
baud-domain/   the type system: quantities, VT operations, reports. no_std, no unsafe.
baud-core/     the ingest core: parser, grid, scrollback. no_std, one unsafe block.
baud-bench/    PTY harness — the numbers above come from here.
baud-difftest/ differential test against alacritty_terminal.
bench-others/  the cross-implementation comparison.
```

`baud-core` depends on `baud-domain` and `zerocopy`. Both crates are `no_std`;
`baud-core` is built for `x86_64-unknown-none` in CI to prove it.

## Building

Requires [Nix](https://nixos.org) and [devenv](https://devenv.sh) — the
toolchain is nightly-pinned and reproduced from `devenv.nix`.

```sh
devenv shell -- dev-ci      # the full gate: fmt, sources, clippy, check, test, bare, audit
devenv shell -- dev-test    # tests only
```

`dev-ci` is not an approximation of CI: `.github/workflows/ci.yml` runs
`devenv test`, the same task set. **Add a check by adding a task, never by
adding a step to the workflow.** The same suite runs on `git push` via a
pre-push hook.

More in [BUILD.md](BUILD.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

## Where to read more

[docs/README.md](docs/README.md) indexes everything. The short version:

| | |
|---|---|
| [docs/architecture.md](docs/architecture.md) | the data path and why it is shaped this way |
| [docs/protocols/](docs/protocols/) | one document per protocol level, mirroring the code and tests |
| [docs/vt-coverage.md](docs/vt-coverage.md) | what of the protocol is implemented, per operation |
| [docs/protocol-decisions.md](docs/protocol-decisions.md) | why the hard calls went the way they did |
| [MEASUREMENTS.md](MEASUREMENTS.md) | every number, with the host that produced it |
| [CLAUDE.md](CLAUDE.md) | working notes: what was tried and lost |

Every protocol claim traces to a primary source in `docs/research/sources/`,
hash-pinned in `MANIFEST.toml` and gated by `scripts/check-sources.sh`. Control
functions are not implemented from memory here — the spec has overruled
confident guesses more than once, and the commit log says where.

## Not done

- **Rendering and font shaping.** No renderer exists; all render figures in
  MEASUREMENTS.md are isolated cost models. Shaping is the largest unknown: a
  warm glyph-cache hit is 1.1 ns and a fontconfig fallback is 245 µs, a
  220,000× cliff.
- **Grapheme clusters.** `Width` is per codepoint; it should be per cluster.
  The width table is hand-rolled and measured 5.6% wrong across 194,528
  codepoints.
- **Reflow, selection, search.** All need random access by `(line, column)`
  into variable-length spans.
- **Concurrency and GPU.** Unevaluable on the hardware to hand.

## Releases

Automated with [release-plz](https://release-plz.dev). Commits follow
[Conventional Commits](https://www.conventionalcommits.org/); release-plz opens
a `chore: release` PR that bumps the version and writes `CHANGELOG.md`, and
**merging that PR is what authorises the publish to crates.io.** Versions are
never edited by hand — release-plz reads its baseline from crates.io, so a
manual bump or publish desynchronises it.

## Funding

Solo-maintained, and sponsorship buys maintenance time rather than obligations.
See [FUNDING.md](FUNDING.md) — including how to arrange a commercial licence,
which the NonCommercial clause below makes a separate conversation.

## License

Creative Commons Attribution-NonCommercial-ShareAlike (CC BY-NC-SA) 4.0 International

See [LICENSE](LICENSE) for details.
