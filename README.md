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
| **VT protocol** | ✅ 147 operations at **done** and 2 at **partial**, [enumerated against primary sources](docs/vt-coverage.md) — DECSC/DECRC are the partial pair: they save five of the seven items `[[source: vt510-decsc]]` lists |
| **Parser** | ✅ all 13 DEC ANSI states, chunk-invariant, property-tested |
| **Grid** | ✅ two storage representations, both measured |
| **Throughput** | ✅ [ahead of ghostty on all nine recordings](#the-number) — 1.73–5.70× on committed capture |
| Scrollback | ◐ **`baud-core2` only** — stored and measured, `SB = 10_000` lines, but no accessor reads above the viewport, so it is storage rather than a feature. The newer `baud-core` has none: retired rows are overwritten by `RowRot` |
| Input path | ◐ sources pinned, [key rows enumerated](docs/input-coverage.md) (15 done). `baud-core2` encodes the base layer in `Term::key`: cursor keys, keypad in both modes, 12 function keys, the VT220 editing keys, the VT52 keyboard, and the meta/alt modes. `baud-core` now records and exposes the state that encoder needs — DECCKM, DECKPAM/DECKPNM, `?1036`, `?1039`, `?1034` — but the encoder itself is not yet ported. The two *negotiated* enhancements, fixterms `CSI u` and kitty progressive enhancement, are unbuilt in both |
| Reply channel | ◐ both cores have one, caller-provided. `baud-core` answers `DA` (under-claiming as a VT102) and `DSR` 5/6; `DECRQM`, `XTVERSION`, `DECSCUSR` and highlight tracking are recognised and counted but unanswered there, each for a named reason. `baud-core2` answers the full set |
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

| recording | baud-core | baud-core2 | ghostty | alacritty | **vs ghostty** |
|---|---|---|---|---|---|
| `ascii/cat-scroll.raw` | **1896.9** | 1107.2 | 441.9 | 207.7 | **4.29x** |
| `ascii/git-log.raw` | **1881.4** | 1275.9 | 544.9 | 218.2 | **3.45x** |
| `ascii/less-pager.raw` | **1734.0** | 1219.5 | 304.3 | 246.2 | **5.70x** |
| `utf8/btop.raw` | **925.6** | 792.6 | 518.1 | 341.7 | **1.79x** |
| `utf8/vim-unicode.raw` | **773.8** | 508.8 | 367.1 | 295.0 | **2.11x** |
| `utf8/less-cjk.raw` | **729.0** | 478.8 | 257.1 | 273.2 | **2.84x** |
| `utf8/cat-cjk.raw` | **710.3** | 467.5 | 411.0 | 228.2 | **1.73x** |
| `utf8/cava.raw` | **587.6** | 457.7 | 325.1 | 380.8 | **1.81x** |
| `ascii/asciiquarium.raw` | **579.9** | 372.9 | 203.8 | 252.4 | **2.85x** |

Ahead on all nine, 1.73x to 5.70x, and the closest row is `cat-cjk` — the
recording whose name oversells it, since a quarter of its bytes are emoji and
those are cluster-breaking rather than CJK.

**Two cores, and the columns are not the same program.** `baud-core` is the
newer one and every headline figure is its; `baud-core2` is beside it because
the two differ in what they *are*, not only in speed — core2 has scrollback, a
full reply channel and a key encoder, and `baud-core` has none of the first and
part of the second. The ingest comparison is like for like (both parse the same
bytes into the same grid); the engineering contract is not, which is the rule
this repo applies to ghostty and so applies to itself.

**`alacritty` beats `ghostty` on `cava` and on nothing else here.** cava is the
one recording that scrolls inside a scrolling region it sets, and that is where
ghostty's row falls furthest — an outside signal for something baud's own
counters say from within.

These are committed recordings of real programs, in `tests/recordings/`.

And on generated payloads, where every core is driven through the same
harness. `payload::streaming` models `cat`, logs and build output;
`payload::tui` models a full-screen redraw.

| core | streaming | TUI redraw |
|---|---|---|
| **baud-core** | **3206.7** [3172.1-3234.1] | **2340.4** [2269.8-2377.7] |
| baud-core2 | 2040.1 [2022.2-2058.7] | 1697.7 [1674.0-1752.7] |
| ghostty (libghostty-vt) | 640.5 [582.1-681.8] | 1061.8 [1043.0-1078.9] |
| termwiz (wezterm) parse only | 380.5 [371.5-383.4] | 366.7 [360.8-370.2] |
| alacritty_terminal | 218.3 [201.3-227.4] | 273.7 [268.0-276.5] |
| vt100 crate | 168.4 [166.6-170.3] | 218.6 [214.0-219.2] |
| wezterm (full terminal) | 17.5 [17.3-17.7] | 32.5 [32.1-32.7] |

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
