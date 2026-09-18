# Measurements

Every number here is current. Superseded results are not kept — `git log
MEASUREMENTS.md` has them, and `CLAUDE.md` carries the reasoning behind changes
that did and did not work.

## How to read this file

A number without provenance is not a measurement. Every table below states:

* the **host**, because cross-machine comparison is meaningless;
* the **transport**, because a PTY on Darwin returns 1024 bytes per `read(2)`
  -- the capacity of the tty's output clist, `[[source: xnu-tty-h]]` -- and a
  pipe returns 4096, which bounds throughput independently of the parser;
* the **statistic** and **rep count**;
* the **payload**, and whether it is a committed recording or generated.

Ranges are quoted as `[min-max]`. **If two ranges overlap there is no result.**

## Host

Apple M5 Max, macOS 27.0.0, on AC power. Commit `90076f9`.

The two CJK rows were re-measured after the flat BMP property table landed
(+2.3% cat-cjk, +2.4% less-cjk). The other six predate it and are unchanged by
it only to the extent that they carry little non-ASCII -- `vim-unicode` and
`btop` moved and should be re-run.

## Against other implementations, on real capture

`bench-others/multibench`, `TRANSPORT=memory`, medians of 9, interleaved and
rotated. Every core is driven with the same bytes through the same 4096-byte
reads, and every core is configured with **10,000 lines of scrollback**.

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

**Three rows fell against the previous table and it is a feature landing, not a
regression.** That table predates grapheme clustering. Ratios, which are the only
thing comparable across two sessions:

| | then | now | |
|---|---|---|---|
| cat-scroll, git-log, less-pager, btop, cava, asciiquarium | | | within **±3%** |
| `utf8/vim-unicode.raw` | 2.11x | 1.86x | **-12%** |
| `utf8/cat-cjk.raw` | 1.73x | 1.25x | **-27%** |
| `utf8/less-cjk.raw` | 2.84x | 1.92x | **-32%** |

The three are exactly the recordings carrying wide characters and emoji, and what
changed is that baud **segments** them: a ZWJ family was three separate people on
the screen and is one glyph now. `CLAUDE.md` measured the cost when it landed --
**-33% on cat-cjk**, of which +18.6% returned when the run gate was fixed -- and
the old figures were for a core that was not doing the work. libghostty-vt still
does no grapheme segmentation unless an application asks for it, so that column
is not paying what baud's is.

**Absolutes are not comparable across the two tables; ratios are.** ghostty moved
4-15% and alacritty 23% in this run on code that has not changed, so the box is
not where it was.

**And `baud-core2` is gone from the table because the crate is gone.** For the
record, on the same box in the same sweeps in its materialising mode it read
cat-scroll 1107.2, git-log 1275.9, less-pager 1219.5, btop 792.6, vim 508.8,
less-cjk 478.8, cat-cjk 467.5, cava 457.7, asciiquarium 372.9 -- a 1.16x-to-1.71x
margin against the implementation this one replaced, and nothing can re-measure
it.

## cava, and every implementation on it

`utf8/cava.raw` -- a spectrum analyser, 636 KB, one escape per 12.1 bytes,
19.5% non-ASCII block glyphs, and **the only recording in the corpus that
scrolls inside a scrolling region it set** (51 copied scrolls a rep against
zero for every other one).

Same harness, `TRANSPORT=memory`, 4096-byte reads, median of 9, interleaved and
rotated. **These are one fresh run and are not mixed with the table above**,
which was measured on a different day -- this file's own rule is that only
same-box, same-session numbers may be compared.

| core | cava.raw | baud-core is |
|---|---|---|
| **baud-core** | **567.4** [558.0-576.6] | -- |
| baud-core2, cells *(deleted; recorded)* | 447.4 [442.0-454.1] | **1.27x** |
| vt100 crate | 402.0 [391.1-415.0] | **1.41x** |
| alacritty_terminal | 366.0 [348.6-381.8] | **1.55x** |
| ghostty (libghostty-vt) | 319.5 [315.5-322.8] | **1.78x** |
| termwiz (wezterm) parse only | 281.5 [278.8-284.1] | **2.02x** |
| wezterm (full terminal) | 135.3 [133.7-136.4] | **4.19x** |

**cava is stable to about 1.5% across four independent pairings** -- 567.4,
568.6, 567.4, 559.7 for the new core -- which is tighter than most rows here
and makes it a good instrument as well as a good payload.

Alongside it, from the same four runs, on the recordings it was paired with:

| recording | baud-core | baud-core2 *(deleted)* | ghostty | alacritty |
|---|---|---|---|---|
| `ascii/cat-scroll.raw` | **1872.7** | 1079.1 | 383.9 | 199.7 |
| `utf8/btop.raw` | **904.9** | 780.0 | 508.3 | 330.1 |
| `utf8/vim-unicode.raw` | **747.3** | 488.4 | 349.4 | 284.5 |
| **`utf8/cava.raw`** | **567.4** | 447.4 | 319.5 | 366.0 |
| `ascii/asciiquarium.raw` | **553.7** | 371.1 | 200.8 | 244.2 |

**alacritty beats ghostty on cava and on nothing else here**, which is worth
noticing rather than smoothing over: cava is the region-scrolling payload, and
ghostty's row falls furthest exactly where the scrolling regions are.

## Against other implementations, on generated payloads

Same harness and statistic. The streaming payload models `cat`, logs and build
output, with an SGR every 37 bytes; the TUI payload models a full-screen redraw.
Both are generated by `multibench` itself, so every core gets the same bytes.

| core | streaming | TUI redraw |
|---|---|---|
| **baud** | **2078.2** [2040.9-2096.5] | **1735.1** [1673.5-1777.4] |
| ghostty (libghostty-vt) | 659.5 [616.8-686.9] | 1065.7 [1039.5-1072.6] |
| termwiz (wezterm) parse only | 381.4 [380.3-383.6] | 367.1 [363.3-370.8] |
| alacritty_terminal | 224.4 [215.4-229.7] | 273.9 [273.0-276.7] |
| vt100 crate | 171.2 [167.5-172.8] | 211.3 [209.6-214.0] |
| wezterm (full terminal) | 17.9 [17.8-18.2] | 32.7 [32.7-32.8] |

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

## baud's representations -- core2's, and there is one now

**This whole section measures `baud-core2` and is kept as a record.** `OpMode`
was its: `Runs` stored lines as spans and materialised in `Term::viewport`,
`Computed` and `Templated` materialised on ingest. `baud-core` has no
representations to choose between -- it materialises, always -- so there is
nothing here to re-measure and no flag to pass.

It also named the wrong harness. `OpMode` was `ingest`'s, never `ingest2`'s, and
both `ingest` and the `payload::tui` generator went with the crate. `ingest2`
drives `baud-core` and carries `cjk` and `streaming`, which are the two
synthetic payloads `CLAUDE.md` quotes by name.

Measured with baud-bench's `ingest` binary -- deleted, so it is named here in
prose rather than cited as a path -- one payload in memory with no fork and no
PTY, medians over 200 reps, MB/s.

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

**`Computed` was the row quoted against other implementations.** Every core it
was compared against materialises, so quoting `Runs` would have compared a
deferred cost against a paid one — and `Runs` led on exactly the payloads where
deferral is worth most. The span model was built in `baud-core` too, measured at
**+50% on two recordings and -5% to -10% on the other six purely to carry the
field**, and reverted; `CLAUDE.md` has the numbers.

## Storage

~24 MB *virtual*. Measured RSS after allocating it, initialising a 50x200 grid
and feeding data is **+0.3 MB**: untouched pages are never faulted in.
`baud-core/tests/fixed.rs` asserts the bound.

The figure was taken against `baud-core2::Storage::boxed()`, which owned its
arrays. `baud-core`'s storage is **caller-provided** -- it borrows every buffer
and allocates nothing -- so the equivalent is `Owned::new(&ws)`, which provisions
exactly the geometry it is given and declines the 2 MiB cell table by default.
The untouched-pages result is a property of the allocator and the grid, not of
which crate holds them, so it carries over; the bound `fixed.rs` asserts is
`baud-core`'s own.

## Write amplification

Cells written per byte ingested, from `Term::cell_amplification`.

| payload | `Runs` (core2, spans) | `Computed` (core2, cells) | `baud-core` |
|---|---|---|---|
| `ascii/cat-scroll.raw` | 1.9x | 6.1x | 1.9x |
| `utf8/cat-cjk.raw` | — | 1.2x | 1.2x |

A cell is eight bytes, so a materialising path cannot go below 8x per *byte of
text*, and the figures above are per byte of *input*, which includes escapes.

**`baud-core` matches the spans column while materialising**, which is the part
worth keeping: `RowExtent` bounds blanking at the used extent, so cat-scroll went
6.1x to 1.9x without deferring a single write. The span model was the other way
of getting there and it cost 5-10% on six recordings just to carry the field.
`ingest2` prints the figure per run.

## Reproducing

```sh
# cross-implementation, real capture
cd bench-others/multibench
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory \
  RECORDINGS="utf8/btop.raw,utf8/cat-cjk.raw" cargo run --release

# cross-implementation, generated payloads
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory cargo run --release

# baud alone, one payload, no fork and no PTY -- the instrument every number
# in CLAUDE.md is measured with. A recording, a ladder rung
# (flat/wrap/lines/sgr), a synthetic payload (cjk/streaming), or `mix`.
cargo bench -p baud-bench --bench ingest2 -- cat-cjk 200

# the transport with no parser behind it: the ceiling a PTY row is measured
# against, and why a PTY row is a claim about the tty rather than the parser
cargo bench -p baud-bench --bench transport -- utf8/btop.raw 48
```

`multibench` takes two recordings at a time. Without `GHOSTTY_LIB_DIR` the
ghostty row reports `n/a` rather than being silently dropped.

**Discard the first round of any `ingest2` A/B.** A cold arm has read 47% below
its settled rate, which is larger than most effects being measured. Interleave
arms, run at least four rounds, and quote the settled ones.

## Keystroke to pixel

Apple M5 Max, macOS 27.0, on AC, the built-in 120 Hz panel, the app's default
cell path at 120x40 under the owner's config. `scripts/key-latency.py --rounds 5
--keys 30 --gap 150 --child <cat|shell> --aa`: five rounds, the first discarded,
the arm order rotated, an A/A copy of the same binary in the sweep; **medians
of 120 keys an arm, microseconds**; the floor is the A/A copy's per leg (the
larger of the median delta and either arm's IQR). `cat` is the tty's own echo
with no program in the loop; the shell is the login zsh. Every key paired,
`stamp = 0` on all 480, no landing refused.

| leg | `cat` | shell | floor, cat / shell |
|---|---|---|---|
| deliver (post to the responder) | 1652 | 1664 | 613 / 584 |
| write | 38 | 37 | 13 / 12 |
| **child** (write to the echo read) | **14.5** | **2551** | 4.7 / 628 |
| wake (read to fed) | 46 | 29 | 22 / 14 |
| build (fed to committed) | 91 | 188 | 44 / 44 |
| gpu (committed to `GPUEndTime`) | 1004 | 615 | 451 / 399 |
| show (GPU end to the surface set) | 218 | 229 | 46 / 51 |
| **to_shown** (post to the surface set; the A/B number) | **3041** | **5422** | 731 / 1163 |
| vsync (surface set to the announced target) | 2827 | 4499 | 2896 / 5172 |
| total, early bound (post to the announced target) | 7097 | 11286 | 4182 / 4915 |
| total, late bound (post to the target after) | 15431 | 19620 | 4182 / 4915 |

Disjoint between the two children, above both floors: `child` +2536,
`build` +97, `to_shown` +2381. Everything else overlaps its floor.

`scripts/key-pixel.swift`, 20 keys through `cat`, the cell right of the cursor
watched through ScreenCaptureKit: the window server's display time of the
changed frame is **3.5-6.0 ms after the display-link target before it (median
4.1)** in every run, inside `[vsync, vsync_hi)` in 16 and one period earlier in
the 4 where the surface was set within 1.1 ms of its tick; never after the late
bound. The median key of the sweep above is 5.5 ms after its tick, so its panel
time is the late bound. **Under the probe's capture `to_shown` reads 13 ms, four
times the sweep's**, so the probe validates the bounds and does not replace the
sweep. Typeahead at a 2 ms cadence: 76 of 80 keys shared a frame, one commit per
8.33 ms period. `docs/research/notes/o-keystroke-latency.md` has the rest.

## The shader cache at launch

Apple M5 Max, macOS 27.0, on AC. `scripts/shader-cache-bench.py 7`: the same
binary launched with the system's Metal shader caches in place (warm) and moved
aside (cold), interleaved, the first launch of each arm discarded; medians of
six launches an arm, milliseconds, from the app's own `gpu build:` line under
`BAUD_STARTUP=1` and the wall from spawn to the first present.

| step | warm | cold (both caches aside) | cold - warm |
|---|---|---|---|
| device | 9.3 [8.7-11.2] | 11.1 [9.5-11.5] | +1.8 |
| library (`newLibraryWithSource`) | 0.6 [0.5-0.9] | **301.9 [285.3-305.2]** | **+301.3** |
| pipelines (five) | 1.1 [0.7-1.3] | **50.7 [48.0-56.5]** | **+49.6** |
| warm pass | 0.7 | 0.5 | -0.2 |
| **wall to first present** | **104.0 [97.0-108.3]** | **409.5 [393.1-420.1]** | **+305.5** |

With only `com.apple.metalfe` (the front end) aside: no change. With only
`com.apple.metal` (the per-GPU binaries) aside: library 81.8, pipelines 48.3,
wall 193.1 -- the proxy for a shipped `.metallib`. A one-line comment in the
shader source: the next launch's library 76.2, pipelines 0.8, then warm; the
cache keys on the source and survives rebuilds. `docs/research/notes/p-shader-cache.md`.

## The Dock icon from the bundle

Apple M5 Max, macOS 27.0, on AC. `BAUD_CMD=shell BAUD_BENCH_STAGE="first
content" scripts/startup-bench.py --rounds 7 --arm base=<HEAD bundle> --arm
bundled=<bundle with the change> --aa`: two `.app` bundles, the login shell as
the child, seven rounds with the first discarded, the arm order rotated, an A/A
copy of the base binary in the sweep. Medians of six launches, milliseconds.

| | base | bundled | A/A |
|---|---|---|---|
| first tick | 134.6 | 139.4 | 133.3 |
| the tick's cost (`dock icon` / `first tick done` minus `first tick`) | **9.0** | **0.0** | 9.0 |
| first content (in-app) | 191.8 | 193.3 | 199.7 |
| wall to first content | 203.7 [196.3-211.2] | 207.9 [205.2-216.3] | 211.2 [204.1-211.9] |

Wall: bundled - base +4.1 ms against an A/A spread of 14.9 -- not a result.
The first present is before the first tick and is unmoved. What moved is 9 ms
of main-thread work on the first tick, which nothing a user sees waits for.

