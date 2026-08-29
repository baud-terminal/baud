# Measurements

Every number here is current. Superseded results are not kept — `git log
MEASUREMENTS.md` has them, and `CLAUDE.md` carries the reasoning behind changes
that did and did not work.

## How to read this file

A number without provenance is not a measurement. Every table below states:

* the **host**, because cross-machine comparison is meaningless;
* the **transport**, because a PTY on Darwin returns 1024 bytes per `read(2)`
  and a pipe returns 4096, which bounds throughput independently of the parser;
* the **statistic** and **rep count**;
* the **payload**, and whether it is a committed recording or generated.

Ranges are quoted as `[min-max]`. **If two ranges overlap there is no result.**

## Host

Apple M5 Max, macOS 27.0.0, on AC power. Commit `3797649`.

## Against other implementations, on real capture

`bench-others/multibench`, `TRANSPORT=memory`, medians of 9, interleaved and
rotated. Every core is driven with the same bytes through the same 4096-byte
reads, and every core is configured with **10,000 lines of scrollback**.

| recording | baud | ghostty | alacritty | **vs ghostty** |
|---|---|---|---|---|
| `ascii/less-pager.raw` | **1209.9** | 295.1 | 242.9 | **4.10x** |
| `ascii/cat-scroll.raw` | **1040.9** | 430.9 | 203.9 | **2.42x** |
| `ascii/git-log.raw` | **1223.4** | 526.0 | 213.0 | **2.33x** |
| `ascii/asciiquarium.raw` | **380.5** | 202.5 | 244.9 | **1.88x** |
| `utf8/less-cjk.raw` | **417.7** | 253.8 | 274.8 | **1.65x** |
| `utf8/btop.raw` | **760.9** | 507.8 | 324.3 | **1.50x** |
| `utf8/vim-unicode.raw` | **451.2** | 354.0 | 290.3 | **1.27x** |
| `utf8/cat-cjk.raw` | **403.7** | 391.1 | 229.8 | **1.03x** |

MB/s. baud is `OpMode::Computed`; see [baud's representations](#bauds-representations)
for why that is the row quoted.

## Against other implementations, on generated payloads

Same harness and statistic. `payload::streaming` models `cat`, logs and build
output, with an SGR every 37 bytes; `payload::tui` models a full-screen redraw.

| core | streaming | TUI redraw |
|---|---|---|
| **baud** | **2097.4** [2020.3-2139.8] | **1786.1** [1751.3-1812.5] |
| ghostty (libghostty-vt) | 678.9 [619.0-701.7] | 1095.4 [1064.8-1108.9] |
| termwiz (wezterm) parse only | 391.4 [381.5-395.6] | 379.1 [371.6-385.9] |
| alacritty_terminal | 220.0 [208.5-228.8] | 272.7 [268.0-279.9] |
| vt100 crate | 172.1 [168.7-173.9] | 222.8 [219.2-227.7] |
| wezterm (full terminal) | 17.9 [17.8-18.2] | 33.1 [32.8-33.6] |

**Generated payloads read higher than any real capture**, and their escape
density matches no real program. The recordings above are the guide.

## Three things the comparison is sensitive to

These decide whether a ratio means anything, and each has produced a wrong
answer in this repo before.

**Scrollback must be set on every core.** `ghostty_terminal_new` takes no
scrollback argument and defaults to unlimited lines and 10,000 *bytes*, pruned
at a page granularity ghostty documents as ~400 KB — which retains no history.
alacritty and vt100 take 10,000 lines explicitly and baud carries `SB =
10_000`. Configured alike, the scroll-heavy recordings move and the others do
not: `git-log` 853.9 → 526.0, `cat-scroll` 765.7 → 430.9, `cat-cjk` 516.7 →
391.1.

**Grapheme segmentation is not on in every core.** libghostty-vt's mode 2027
carries no default, so it segments only when an application asks. baud always
segments and reports `?2027` as permanently set. Segmentation is 25-45% of
baud's UTF-8 throughput, and switching it off writes 5.9% *more* cells, because
an unclustered combining mark becomes its own cell. The CJK rows therefore
compare unequal work.

**Throughput tracks escape density, not content.** btop and vim draw completely
different things — truecolour box art against scroll regions over CJK and emoji
— and have almost the same density (16.5 and 17.2 bytes per escape) and almost
the same throughput. asciiquarium is three times denser and slower to match.

## baud's representations

`OpMode` is fixed when a `Term` is constructed. `baud-bench/src/bin/ingest.rs`,
one payload in memory with no fork and no PTY, medians over 200 reps, MB/s.

| payload | `Runs` | `Computed` | `Templated` |
|---|---|---|---|
| `payload::cjk` | 2398.6 | **2571.8** | 2566.3 |
| `payload::streaming` | **1999.0** | 1613.1 | 1600.1 |
| `payload::tui` | 1431.5 | 1467.2 | **1480.8** |
| `ascii/git-log.raw` | **2026.0** | 1291.4 | 1293.4 |
| `ascii/cat-scroll.raw` | **1674.7** | 1110.1 | 1129.7 |
| `ascii/less-pager.raw` | 1225.6 | **1294.5** | 1275.4 |
| `utf8/btop.raw` | **828.7** | 828.4 | 821.7 |
| `utf8/vim-unicode.raw` | 470.4 | **477.7** | 476.6 |
| `utf8/less-cjk.raw` | 452.6 | **453.1** | 447.1 |
| `utf8/cat-cjk.raw` | 382.8 | 430.6 | **431.0** |
| `ascii/asciiquarium.raw` | 408.9 | 405.7 | **412.9** |

**`Computed` is the row quoted against other implementations.** `Runs` stores
lines as spans and does not materialise the grid; `Term::viewport` does that
work when a renderer asks. Every core it is compared against materialises, so
quoting `Runs` would compare a deferred cost against a paid one — and `Runs`
leads on exactly the payloads where deferral is worth most.

## Storage

`Storage::boxed()` is ~24 MB *virtual*. Measured RSS after allocating it,
initialising a 50x200 grid and feeding data is **+0.3 MB**: untouched pages are
never faulted in. `baud-core/tests/perf_floor.rs` asserts the bound.

## Write amplification

Cells written per byte ingested, from `Term::cell_amplification`.

| payload | `Runs` | `Computed` |
|---|---|---|
| `ascii/cat-scroll.raw` | 1.9x | 6.1x |
| `utf8/cat-cjk.raw` | — | 1.2x |

A cell is eight bytes, so a cells mode cannot go below 8x per *byte of text*
and the figures above are per byte of *input*, which includes escapes. The
spans path is the one the write-amplification invariant is about.

## Reproducing

```sh
# cross-implementation, real capture
cd bench-others/multibench
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory \
  RECORDINGS="utf8/btop.raw,utf8/cat-cjk.raw" cargo run --release

# cross-implementation, generated payloads
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory cargo run --release

# baud alone, one payload, no fork and no PTY
cargo run --release -p baud-bench --bin ingest -- cat-cjk 200 cells

# per-operation cost
cargo run --release -p baud-bench --bin opcost
```

`multibench` takes two recordings at a time. Without `GHOSTTY_LIB_DIR` the
ghostty row reports `n/a` rather than being silently dropped.

**Discard the first round of any `ingest` A/B.** A cold arm has read 47% below
its settled rate, which is larger than most effects being measured. Interleave
arms, run at least four rounds, and quote the settled ones.
