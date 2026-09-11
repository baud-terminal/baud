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
| Scrollback | ⬜ not implemented. `baud-core2` stored `SB = 10_000` lines and **no accessor read above the viewport**, so it was never a feature; `baud-core` does not store them at all — retired rows are overwritten by `RowRot`. Building it needs a ring the viewport is a window into, so a retired line stays put and the window moves |
| Input path | ◐ sources pinned, [key rows enumerated](docs/input-coverage.md) (15 done). The base layer is `baud_domain::encode_key` — cursor keys, keypad in both modes, 12 function keys, the VT220 editing keys, the VT52 keyboard, the meta/alt modes — and `baud-core::Term::key` reads the five modes off the terminal and calls it. It lives in the domain rather than in a core because nothing in it touches the grid. The two *negotiated* enhancements, fixterms `CSI u` and kitty progressive enhancement, are unbuilt |
| Reply channel | ◐ caller-provided. `baud-core` answers `DA` (under-claiming as a VT102), `DSR` 5/6 and `DECRQSS`; `DECRQM`, `XTVERSION` and highlight tracking are recognised and counted but unanswered, each for a named reason — a wrong-shaped answer is worse than silence |
| Renderer | ⬜ not started |
| Font shaping | ⬜ not started — and the largest unknown in the project |

So: you cannot run this as your terminal today. You can run its core, feed it a
real escape stream, and measure it — which is what the rest of this file is
about.

## The road to a terminal

The next work is the **input path**, then the renderer — the owner's order. The
base key encoder is already shared in `baud_domain`; what is unbuilt is the two
negotiated layers, fixterms `CSI u` and kitty progressive enhancement.

Then two questions decide whether the measurements above survive contact with a
real terminal:

1. **Shaping.** Wire HarfBuzz and a glyph atlas into viewport materialisation,
   then re-measure on mixed CJK/emoji/ligature content. If throughput holds
   above ~50% of the kernel ceiling and full-damage frames stay under 2 ms, the
   rest is engineering. If shaping drops it to 15%, the architecture needs a
   shaping cache keyed on line content before anything else matters.
2. **Real streams.** Replay captured sessions (`script -c "vim ..."`) instead of
   synthetic payloads, and find out whether the margin survives real escape
   traffic.

After the renderer and shaping, it is a terminal.

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

| recording | baud-core | ghostty | alacritty | **vs ghostty** |
|---|---|---|---|---|
| `ascii/git-log.raw` | **1837.1** | 497.0 | 192.0 | **3.70x** |
| `ascii/cat-scroll.raw` | **1766.9** | 394.7 | 173.2 | **4.48x** |
| `ascii/less-pager.raw` | **1734.1** | 291.3 | 239.0 | **5.95x** |
| `utf8/btop.raw` | **893.1** | 502.8 | 328.8 | **1.78x** |
| `utf8/vim-unicode.raw` | **655.6** | 353.3 | 279.1 | **1.86x** |
| `utf8/cava.raw` | **573.0** | 319.4 | 355.3 | **1.79x** |
| `ascii/asciiquarium.raw` | **555.7** | 199.0 | 240.1 | **2.79x** |
| `utf8/less-cjk.raw` | **500.8** | 250.5 | 275.6 | **2.00x** |
| `utf8/cat-cjk.raw` | **493.4** | 383.7 | 197.3 | **1.29x** |

Ahead on all nine, **1.29x to 5.95x**.

**The two CJK rows moved +4% and +3% since the last table, and that is the NEON
run scan now on by default.** `baud-isa-aarch64::Neon` extracts a lane mask with
`shrn` where `core::simd`'s `to_bitmask` lowers to a horizontal reduction; on
aarch64 that is a real instruction difference rather than a wider one, since
both process sixteen bytes. Measured in isolation, six rounds with an A/A
control interleaved: **cat-cjk +4.3%, less-cjk +3.7%**, disjoint from base and
from the control, stable under reversing the arm order, every other recording
overlapping -- which is the prediction holding, because the stride-three scan
only fires on runs of three-byte characters.

It costs no `unsafe` in `baud-core`, which still carries
`#![forbid(unsafe_code)]` unconditionally: `Scan`'s methods are `unsafe fn` for
`Avx2`'s sake, where AVX2 is not baseline, and `Neon` wraps them in a safe
inherent surface because on aarch64 NEON *is* baseline and the obligation is
satisfied by the target. `cargo build -p baud-bench --no-default-features` is
the arm that turns it off.

**Three rows moved since the previous table and the reason is a feature, not a
regression.** That table was measured before grapheme clustering landed. Holding
the ghostty ratio, which is the only thing comparable across two sessions:

| | ratio then | now | |
|---|---|---|---|
| six rows -- cat-scroll, git-log, less-pager, btop, cava, asciiquarium | | | within **±3%** |
| `vim-unicode` | 2.11x | 1.86x | **-12%** |
| `cat-cjk` | 1.73x | 1.25x | **-27%** |
| `less-cjk` | 2.84x | 1.92x | **-32%** |

The three that fell are exactly the recordings carrying wide characters and
emoji, and what changed is that baud now **segments them**. `U+1F468 ZWJ U+1F469
ZWJ U+1F467` was three separate people on the screen and is one glyph now; the
old numbers were for a core that did not do that work. Measured at the time:
**-33% on cat-cjk**, of which +18.6% came back when the run gate was fixed.

This is the second time a headline here has been withdrawn for the same reason.
The first was the escape-dense rows, measured while the core parsed no escapes
and blitted the parameter bytes to the screen as text -- *"that is cheap per byte
and it is not parsing."* Same shape, different feature.

**Absolutes are not comparable across the two tables and ratios are.** ghostty
is down 4-15% and alacritty 23% in this run, on code that has not changed, so the
box is not where it was. That is why the table above is one interleaved sweep and
why the comparison is stated as a ratio.

**There is one core.** `baud-core2` was the previous implementation and the
reference this one was checked against; it has been deleted, and every row above
is `baud-core`. For the record of what it read on the same bytes and the same
box — cat-scroll 1107.2, git-log 1275.9, less-pager 1219.5, btop 792.6, vim
508.8, less-cjk 478.8, cat-cjk 467.5, cava 457.7, asciiquarium 372.9 — the newer
core is 1.16x to 1.71x ahead of it while rendering all nine cell for cell.

What it carried that went with it was an oracle, not a feature: the only thing
lost is the ability to drive two implementations over the same bytes and compare
10,000 cells. Its rendering is frozen as data in `baud-core/tests/golden.rs`
instead, which is strictly weaker — a golden capture finds regressions only on
bytes somebody wrote down — and `baud-difftest` is now the one instrument that
can disagree from outside.

**`alacritty` beats `ghostty` on `cava` and on nothing else here.** cava is the
one recording that scrolls inside a scrolling region it sets, and that is where
ghostty's row falls furthest — an outside signal for something baud's own
counters say from within.

These are committed recordings of real programs, in `tests/recordings/`.

And on generated payloads, where every core is driven through the same
harness. The streaming payload models `cat`, logs and build output;
the TUI payload models a full-screen redraw. `multibench` generates both, so
every core is handed the same bytes.

| core | streaming | TUI redraw |
|---|---|---|
| **baud-core** | **3206.7** [3172.1-3234.1] | **2340.4** [2269.8-2377.7] |
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
baud-core/     the ingest core: parser and grid. no_std, no unsafe, one dependency.
baud-bench/    ingest2, the single-payload instrument, and transport, the PTY ceiling.
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
- **Scrollback.** Not stored. `baud-core2` stored it and nothing read it —
  there was no accessor above the viewport, so it was never scrollback — and
  `baud-core` does not store it at all. Either way there is nothing to scroll
  back to until a renderer exists.
- **A second implementation to check against.** `baud-core2` was that, and it
  is gone; what it rendered is frozen as golden data, which catches regressions
  on recorded bytes and cannot find an operation nobody wrote a test for.
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
