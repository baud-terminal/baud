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
| **Throughput** | ✅ [ahead of ghostty on all eight recordings](#the-number) — 1.12–4.31× on committed capture |
| Scrollback | ◐ **`baud-core2` only** — stored and measured, `SB = 10_000` lines, but no accessor reads above the viewport, so it is storage rather than a feature. The newer `baud-core` has none: retired rows are overwritten by `RowRot` |
| Input path | ◐ **`baud-core2` only** — sources pinned, [key rows enumerated](docs/input-coverage.md) (15 done), and `Term::key` encodes the base layer: cursor keys, keypad in both modes, 12 function keys, the VT220 editing keys, the VT52 keyboard, and the meta/alt modes. The two *negotiated* enhancements — fixterms `CSI u` and kitty progressive enhancement — are unbuilt |
| Reply channel | ◐ **`baud-core2` only** — the newer `baud-core` has no reply buffer, so `DA`, `DSR`, `DECRQM`, `XTVERSION`, cursor style and mouse highlighting are recognised and counted there but deliberately unanswered |
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

*Apple M5 Max, in-memory transport, medians of 9, interleaved and rotated.
Every core is driven with the same bytes and given 10,000 lines of scrollback.
Full tables, other hosts, and every caveat in
[MEASUREMENTS.md](MEASUREMENTS.md).*

| recording | baud (cells) | ghostty | alacritty | **vs ghostty** |
|---|---|---|---|---|
| `ascii/less-pager.raw` | **1251.7** | 290.5 | 236.9 | **4.31x** |
| `ascii/git-log.raw` | **1236.5** | 526.5 | 214.7 | **2.35x** |
| `ascii/cat-scroll.raw` | **1077.5** | 429.9 | 205.8 | **2.51x** |
| `utf8/btop.raw` | **790.4** | 513.1 | 330.7 | **1.54x** |
| `utf8/vim-unicode.raw` | **491.9** | 361.4 | 285.9 | **1.36x** |
| `utf8/less-cjk.raw` | **476.0** | 260.7 | 273.6 | **1.83x** |
| `utf8/cat-cjk.raw` | **463.7** | 415.5 | 234.0 | **1.12x** |
| `ascii/asciiquarium.raw` | **385.8** | 202.4 | 247.8 | **1.91x** |

Ahead on all eight, and the closest row is no longer close: `cat-cjk` was
1.03x and is 1.12x, from baud moving rather than ghostty. These are committed
recordings of real programs, in `tests/recordings/`.

And on generated payloads, where every core is driven through the same
harness. `payload::streaming` models `cat`, logs and build output;
`payload::tui` models a full-screen redraw.

| core | streaming | TUI redraw |
|---|---|---|
| **baud** | **2078.2** [2040.9-2096.5] | **1735.1** [1673.5-1777.4] |
| ghostty (libghostty-vt) | 659.5 [616.8-686.9] | 1065.7 [1039.5-1072.6] |
| termwiz (wezterm) parse only | 381.4 [380.3-383.6] | 367.1 [363.3-370.8] |
| alacritty_terminal | 224.4 [215.4-229.7] | 273.9 [273.0-276.7] |
| vt100 crate | 171.2 [167.5-172.8] | 211.3 [209.6-214.0] |
| wezterm (full terminal) | 17.9 [17.8-18.2] | 32.7 [32.7-32.8] |

MB/s. **Generated payloads read higher than any real capture** and their escape
density matches no real program, so the recordings above are the guide and
this table is the like-for-like comparison across cores.

Two things belong with the number rather than under it:

* **baud clusters, ghostty does not.** libghostty-vt's mode 2027 carries no
  default, so it does no grapheme segmentation unless an application asks.
  baud always does, and it is 25-45% of baud's UTF-8 throughput — so the CJK
  rows above are not comparing equal work.
* **Throughput tracks escape density, not content.** A payload's ratio of
  escape sequences to text is what these numbers measure, which is why
  recordings are the guide and synthetic payloads are not.

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
- **Reflow, selection, search.** All need random access by `(line, column)`
  into variable-length spans.
- **Scrollback as a feature.** In `baud-core2` lines above the viewport are
  stored, but nothing reads them — there is no accessor, so it is storage
  rather than scrollback. In `baud-core` they are not stored at all.
- **The two cores are not the same program.** Every throughput row below is
  `baud-core`, which is the faster one and has neither scrollback, a reply
  channel, nor an input path; `baud-core2` has all three. The ingest comparison
  is like for like — both parse the same bytes into the same grid — but the
  engineering contract is not, and this repo already applies that rule to
  ghostty, so it applies it to itself.
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
