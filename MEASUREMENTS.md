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

## Spawning the shell earlier (row 3, reverted)

Apple M5 Max, macOS 27.0, on AC. `BAUD_CMD=shell BAUD_BENCH_STAGE="first
content" scripts/startup-bench.py --rounds 7 --arm base=<HEAD bundle> --arm
early=<the arm's bundle> --aa`, two `.app` bundles, seven rounds with the first
discarded, the arm order rotated, an A/A copy of the base. Medians of six
launches, milliseconds; the A/A column is |aa - base| on that stage.

| stage | base | early | A/A |
|---|---|---|---|
| pty spawned | 24.5 | **9.8** | 0.2 |
| nsapp | 48.4 | 43.2 | 0.3 |
| glass window | 72.5 | 67.0 | 0.1 |
| shown | 84.9 | 79.1 | 0.9 |
| first present | 87.9 | 82.7 | 0.7 |
| first bytes (the shell's prompt) | 195.8 | **187.8** | 2.3 |
| wall to first content | 210.3 [201.8-214.6] | 198.4 [195.7-203.6] | 207.6 [202.0-213.8] |

Under the row's 20 ms floor on every metric; reverted. The `scale` mark shows
the display-scale query at 23.6 ms (the first CoreGraphics call), the inset at
1.2 -- what precedes the spawn.

## Overlapping sharedApplication with the CoreGraphics connection (row 4, reverted)

Apple M5 Max, macOS 27.0, on AC. `scripts/startup-bench.py --rounds 7 --arm
base=<HEAD bundle> --arm overlap=<the arm's bundle> --aa`, to the first present,
and again with `BAUD_CMD=shell BAUD_BENCH_STAGE="first content"`. Medians of six
settled launches, milliseconds; the A/A column is |aa - base| on that stage.

| stage | base | overlap | A/A |
|---|---|---|---|
| nsapp | 50.5 | 44.6 | 3.6 |
| scale | 17.7 | 45.4 | 1.3 |
| pty spawned | 25.0 | 48.6 | 1.2 |
| glass window | 72.2 | 73.6 | 1.6 |
| first present | 89.5 [88.1-101.3] | 90.7 [84.6-112.9] | 1.7 |
| first bytes, through the shell | 195.1 | 213.9 | 4.7 |
| wall to first content | 210.7 [202.1-214.0] | 227.4 [199.1-241.8] | 212.5 [201.9-230.7] |

`sharedApplication` alone went from ~25 to ~41 ms with the CoreGraphics query
running beside it: the two window-server connections serialise. No gain on the
first present; the prompt 19 ms later. Reverted.

## Dead-stripping the dylibs (row 5)

Apple M5 Max, macOS 27.0, on AC. `scripts/startup-bench.py --rounds 9 --arm
base=<HEAD bundle> --arm stripped=<bundle with build.rs> --aa`, to the first
present; medians of eight settled launches, milliseconds; the A/A column is
|aa - base| on that stage.

| stage | base | stripped | A/A |
|---|---|---|---|
| scale | 17.2 | 16.7 | 0.2 |
| nsapp | 50.1 | 48.6 | 1.4 |
| glass window | 75.9 | 75.4 | 1.8 |
| shown | 89.6 | 89.7 | 0.5 |
| first present | 93.2 [85.0-118.8] | 93.4 [84.4-121.3] | 0.6 |
| wall to first present | 106.0 [97.2-132.8] | 104.4 [92.4-131.5] | 107.0 [99.3-114.0] |

No launch result. What changed is the binary: 15 load commands to 8, the nix
store's `libiconv.2.dylib` among the seven gone.


## Pacing a keystroke's frame by completion (row 6)

Apple M5 Max, macOS 27.0, on AC, the built-in 120 Hz panel, the default
cell path at 120x40. `scripts/key-latency.py --rounds 5 --keys 30 --gap 150
--child stream --arm base=<HEAD> --arm new=<row 6> --aa`: the child prints a
line every 2 ms under the tty's echo, so a key's echo always lands inside
the frame loop's frame's-time gate. Five rounds, the first discarded, the
arm order rotated, an A/A copy in the sweep; medians of ~120 keys an arm,
microseconds; the floor is the A/A copy's per leg. This is the second of
four sweeps, the quiet one whose posts were phase-locked (see `CLAUDE.md`);
the uniform-phase sweep read the same `build` and lost `to_shown` to a
noisier box.

| leg | base | new | floor | verdict |
|---|---|---|---|---|
| deliver | 942 | 972 | 288 | within |
| **build** (fed to committed) | **3184** [2317-3469] | **41** [31-59] | 1629 | **result** |
| gpu | 661 | 675 | 396 | within |
| show | 151 | 149 | 25 | within |
| **to_shown** (the A/B number) | **4905** [4220-5327] | **2109** [1749-2600] | 1681 | **result, -2795** |
| vsync (surface set to the announced target) | 7435 | 1887 | 422 | result |
| total, early bound | 12426 | 4277 | 1683 | result |
| total, late bound | 20760 | 12610 | 1683 | result |

Sweep 4, posts at a uniform phase, box noisier (`deliver` 1781 [1471-3386]):
`build` 2938 [1826-3695] -> 47 [39-64], floor 2175, result; `to_shown`
5972 -> 3428, -2544, floor 4364, not a result. Isolated keys through `cat`
in the same session: every leg inside its floor (`build` 65 -> 63, floor
38; `to_shown` 2657 -> 2528, floor 998).

Kept on the layer. The panel is not measured for this row: by the row-0
probe's rule both arms' frames make the same compositor pass except in the
base's tail, where its tick-paced frame reached the layer more than
1.05 ms after its tick on 8-12 % of keys.

## The ligature window's miss (row 7, measured, no change)

Apple M5 Max, macOS 27.0, on AC. `FRAME_ARMS=async cargo bench -p baud-bench
--bench frame_gpu -- <recording> 2 incremental`, one binary per arm from one
source -- the base, `render-no-window-cache` (every window shaped) and
`render-no-windows` (no window walk) -- plus a byte-identical copy of the
base as the A/A control, four launches a binary in separate processes with
the order rotated, each launch's in-process warm-up round discarded (one
short of the rule's five); 200x50 at 14 px.
Convert in microseconds a frame, medians of four:

| recording, face | base | A/A | every window a miss | no windows | windows a frame | misses a frame | a miss, us |
|---|---|---|---|---|---|---|---|
| btop, JetBrains Mono | 37.9 [37.4-39.6] | 38.7 | 193.0 | 30.1 | 59.7 | 0.4 | 2.6 |
| cat-scroll, JetBrains Mono | 110.3 [109.3-113.3] | 110.8 | 233.1 | 23.2 | 60.6 | 17.8 | 2.9 |
| git-log, JetBrains Mono | 190.6 [187.8-193.7] | 192.0 | 233.2 | 26.2 | 26.7 | 14.9 | 3.6 |
| less-pager, JetBrains Mono | 171.3 [156.4-179.5] | 164.2 | 271.4 | 53.8 | 55.8 | 22.9 | 3.0 |
| asciiquarium, JetBrains Mono | 332.3 [318.0-376.5] | 329.0 | 773.7 | 20.8 | 73.7 | 16.0 | 7.6 |
| cat-scroll, FiraCode | 287.9 [278.5-293.2] | 284.5 | 350.1 | 24.0 | 54.3 | 37.4 | 3.7 |
| git-log, FiraCode | 350.6 [347.6-354.9] | 355.6 | 432.7 | 26.9 | 48.6 | 36.7 | 6.9 |

A miss is the extra convert of the every-miss arm over the base, divided by
the windows it added to the misses; that arm still writes the cache, so a
miss here is a shape plus a replace less the get the base pays. No change
shipped: the frame's keep floor is one 120 Hz frame, the largest base
convert in the table is 0.35 ms and the largest of any arm 0.77.

## The glyph map's first-frame misses (row 8, measured, no change)

Apple M5 Max, macOS 27.0, on AC. `FRAME_FACES=<the app's default CJK stack>
FRAME_FIRST=<n> FRAME_ARMS=async cargo bench -p baud-bench --bench frame_gpu
-- <recording> 3 incremental`, 200x50 at 14 px; the first n frames of a
replay reported apart from the rest, three rounds, the spread across rounds
under 5 % on every row. Convert in microseconds a frame, and map misses:

| recording | n | first n: convert, misses a frame | the rest: convert, misses a frame |
|---|---|---|---|
| cat-cjk | 1 | 2869-3087, 663 | 157-163, 32 |
| cat-cjk | 3 | 1924-2048, 454 | 126-127, 24 |
| cat-cjk | 10 | 1135-1188, 255 | 65-68, 9.2 |
| cat-cjk | 30 | 481-486, 106 | 25.0-25.5, 0.18 |
| less-cjk | 10 | 884-970, 201 | 44-48, 6.0 |
| vim-unicode | 10 | 50-56, 9.7 | 16.5-17.1, 0 |
| btop | 10 | 32-40, 10.4 | 20.5-20.8, 0.19 |

A miss is about 4.3 us (the first frame's convert less the walk, over its
misses). No change shipped: the first frame is 3 ms against a frame floor of
8, and warming the map at load moves the same shaping and rasterising to
launch rather than removing it.

## A key's echo drawn before the pty answers (row 9)

Apple M5 Max, macOS 27.0, on AC, the built-in 120 Hz panel, the default
cell path at 120x40. `scripts/key-latency.py --rounds 5 --keys 30 --gap
150 --child shell --arm base=<row 8> --arm new=<row 9> --aa`: the login
zsh, whose prompt carries the OSC 133 marks the predictor keys on. Five
rounds, the first discarded, the arm order rotated, an A/A copy in the
sweep; medians of 120 keys an arm, microseconds, quartiles in brackets;
the floor is the larger of the A/A pair's median gap and either arm's
IQR. Every key predicted and confirmed, none contradicted, 44 of 120
drawn over a cell the shell's autosuggestion had filled.

Six sweeps in the order taken. Sweeps 1-4 ran the branch before its last
change, a gate refusing a prediction for a key held with a modifier; 5 and
6 ran the shipped binary:

| sweep | `to_shown` base -> new | delta | floor | `app` delta | floor | verdict |
|---|---|---|---|---|---|---|
| 1 | 5633 -> 3636 | -1997 | 3618 | -2017 | 2629 | **no result** |
| 2 | 7094 -> 3963 | **-3131** | 1054 | -3127 | 776 | result |
| 3 | 7020 -> 4019 | **-3002** | 966 | -3019 | 808 | result |
| 4 | 7068 -> 3994 | **-3074** | 875 | -3060 | 723 | result |
| **5**, shipped | 7005 -> 3960 | **-3045** | 1339 | -3047 | 1076 | result |
| **6**, shipped | 7096 -> 3990 | **-3106** | 1018 | -3125 | 751 | result |

Sweep 1 could not resolve: its `gpu` leg read 986-1162 us against
1247-1307 in the three that follow, its `write` 27.4 us against 35.0-37.5
and its `build` 128.3 against 227.0-233.2, with an A/A spread on `app` of
2.6 ms against 0.7-0.8. Why the box was in that state is unexplained. It
is reported because it was taken.

Sweep 6, the last on the shipped binary:

| leg | base | new | aa | floor | verdict |
|---|---|---|---|---|---|
| deliver | 2021 [1794-2116] | 2078 [1912-2161] | 2017 [1873-2123] | 322 | within |
| write | 34.4 | 37.3 | 35.7 | 7.5 | within |
| predict | -- | 1.8 [1.5-3.0] | -- | -- | -- |
| child | 3154 [2744-3361] | 3289 [2890-3613] | 3083 [2632-3286] | 655 | within |
| wake | 35.3 | 32.3 | 35.3 | 9.9 | within |
| build (base: fed to commit; new: the prediction to commit) | 238 [211-258] | 295 [204-306] | 226 [201-248] | 47.3 | IQRs overlap |
| gpu | 1266 [1213-1320] | 1293 [1254-1339] | 1261 [1218-1305] | 107 | within |
| show | 307 [282-322] | 315 [258-359] | 310 [283-329] | 46.7 | within |
| **app** (handled to shown) | **5079** [4720-5352] | **1954** [1789-2027] | 4989 [4495-5246] | 751 | **result, -3125** |
| **to_shown** (the post to shown) | **7096** [6421-7439] | **3990** [3664-4159] | 7032 [6469-7281] | 1018 | **result, -3106** |

Controls on the shipped binary, same session. `--child cat`:
`refused: zone=120`, `child` 15.6 -> 15.3 (floor 3.1), `build` 110.9 ->
112.5 (29.5), `app` 1964 -> 1964 (212), `to_shown` 3899 -> 3954 (441),
every leg within. `--child stream`: `refused: zone=120`, `child` 8.4 ->
8.1 (4.3), `build` 54.5 -> 56.5 (37.9), `app` 1201 -> 1392 (644),
`to_shown` 3522 -> 3623 (1902), every leg within. `--child mute --rounds
3 --keys 10 --gap 300`, one arm: `predict refused: hold=9` in each of the
three rounds and `predicted=2 confirmed=0 contradicted=2` over the two
settled ones.

Kept on `to_shown`, the leg note Q named before the row began: -3.0 ms
with the arms' quartiles disjoint, five sweeps running, against note Q's
expected -2.5 ms and the input floor of 1 ms. `app` (`handled` to
`shown`) is `to_shown` less the HID tap's delivery, was added to the
driver during this row rather than before it, and moves the same -3.0 ms
because that leg is unchanged (2021/2078/2017 us across the three arms).

The three milliseconds are the child's turn leaving the path to the first
frame: `child` is 3154/3289/3083 across the arms and inside its floor.
That leg is not stable -- 2.55 ms in row 0's baseline, 1.6 ms in this
row's earlier session, 2.1-2.6 ms in sweep 1 and 3.1-3.4 ms in sweeps 2-6
-- so it moves within a session as well as between two. That is why the
row is a paired A/B inside one sweep, why no figure here is read across
from another session, and why the gain is bounded by the shell's turn on
the day: the same change would read about 1.7 ms on a 1.6 ms turn. The
observed -3.0 ms exceeds note Q's expected -2.5.

## A drag step's core work (row 10, measured, no change)

Apple M5 Max, macOS 27.0, on AC. `cargo bench -p baud-bench --bench winch
-- ../tests/resize/<fixture> cost 20`: each fixture's recorded steps -- a
size and the bytes that arrived at it -- replayed through the core twenty
times, the first round discarded, the resize and the feed timed apart and
the median taken. A **fresh storage per round**, because `Term::new`
resets the cursor and not the grid, so a round on a reused one starts on
the last round's screen and writes up to 21% more cells (`short` 15,219
against 12,612); the parser-cell column is one value across all nineteen
settled rounds on every fixture, which is the check. Three runs of each fixture, the spans across those runs. No window
and no GPU.

| gesture | steps | bytes | width changes | a step: resize + feed | gesture | parser cells | lines held |
|---|---|---|---|---|---|---|---|
| live-drag | 767 | 99,018 | 438 | 0.01-0.02 + 0.17-0.19 us | 0.14-0.16 ms | 115,010 | 0 |
| drag | 331 | 140,050 | 156 | 0.02 + 0.45-0.47 us | 0.16 ms | 164,605 | 0 |
| fresh-drag | 331 | 132,732 | 156 | 0.02 + 0.35-0.39 us | 0.12-0.13 ms | 196,746 | 0 |
| zoom | 26 | 13,603 | 23 | 0.01-0.02 + 0.53-0.54 us | 0.01 ms | 13,584 | 15 |
| fresh-zoom | 26 | 8,148 | 23 | 0.02 + 0.32-0.35 us | 0.01 ms | 10,399 | 0 |
| fresh-burst | 43 | 9,495 | 30 | 0.01-0.02 + 0.28-0.30 us | 0.01 ms | 10,682 | 0 |
| short | 26 | 13,116 | 23 | 0.01-0.02 + 0.53-0.55 us | 0.01 ms | 12,612 | 19 |

**The resize column is the clock, not the resize.** A null arm -- an
empty timed region per step -- reads the same 0.01-0.02 us a step beside
it, so what the column says is that a width change at a prompt costs less
than a clock pair, not that it costs 20 ns.

The control, in the same run, looped the way the gesture is and on the
screen the gesture left: one width change costs **0 [0-42] ns** in
`Zone::Prompt`, where the core leaves the shell's rows for the shell to
redraw, and **1.5-7.0 us** in `Zone::Output`, where it re-cuts -- the
span of the seven fixtures' medians, each a median of 19 settled rounds. **0 of
every fixture's width changes are in `Zone::Output`**, counted at the
call, which is why the resize column is at the floor.

No change shipped. A step averages 0.19-0.56 us against a frame's 8.33 ms,
four orders under it, and the whole of live-drag is under 2% of one frame. In
`Zone::Output`, which no fixture reaches and which no coalescing of
redraws would reach either -- a re-cut follows the resize, not the redraw
-- the control puts a width change at 1.5-7.0 us, three orders under a
frame. Frames are per resize step and per display period, not per redraw.

## Idle CPU (row 11, measured, no change)

Apple M5 Max, macOS 27.0, on AC, nothing else running.
`scripts/idle-cpu.py --arm base=<bin> --aa --rounds 5`: the binary
launched with a child that writes nothing, its CPU read twice from the
**live** process -- `proc_pid_rusage`, whose time fields are mach ticks
and not nanoseconds -- so the launch's ~0.13 s is outside both samples.
The first round is discarded and the arms rotate. `ps -o time` needs an
entitlement on this box and `top` refuses to sample, which is why the
source is neither.

| arm | idle CPU | range | a tick at 120 Hz |
|---|---|---|---|
| base | **0.464%** of one core | 0.428-0.477 | 38.7 us |
| `ceiling-idle-skip`, the whole `present` preamble skipped | 0.472% | 0.460-0.480 | 39.3 us |
| a byte-identical copy of base | 0.474% | 0.449-0.508 | 39.5 us |

**The floor is the A/A copy's own spread, 0.044-0.060% of a core** --
9-13% of the metric, where the keep rule asks for 5%, so this instrument
cannot resolve a keep-sized change here. Sixty-second windows did not
tighten it over twenty-second ones. What it does establish is a bound: an
arm that removes every candidate operation still lands inside the floor.

**And an idle screen builds nothing, which is the row's own claim.** The
app counts display-link ticks beside frames: five consecutive
five-second reports read **601 ticks and 0 frames** each -- 120 Hz
exactly -- against 77 frames in 21 s when a child prints every 250 ms.
The one frame an idle run ever builds is the launch frame.

No change shipped. `present` already returns on `Pending::Clean` before
the glyph-map walk, the style resolve, the overlay walk and the draw.

## The shipped `.metallib` (row 1, kept)

Apple M5 Max, macOS 27.0, Xcode 27.0, on AC.
`scripts/shader-cache-bench.py 7`: the same binary launched with the
system's Metal caches in place and moved aside, interleaved, the first
launch of each arm discarded. The `before` column is the source-compiled
library measured the same afternoon, not the figure this file carried
from an older Xcode.

| field | before: source | after: `baud.metallib` |
|---|---|---|
| cold library | 277.6 ms [276.0-284.8] | **0.1** [0.1-0.1] |
| cold pipelines | 45.1 [44.7-45.8] | 73.6 [65.0-74.3] |
| **cold wall to first present** | **389.9** [375.1-394.0] | **133.5** [109.2-135.3] |
| warm wall | 102.9 [99.7-119.1] | 99.1 [94.1-101.6] |
| **cold - warm** | **+287.0** | **+34.3** |
| device *(control)* | +0.0 | -0.3 |
| warm pass *(control)* | -0.1 | -0.3 |

Every cell is the instrument's own row; it differences unrounded medians,
so subtracting the printed walls gives 34.4 where the difference reads
34.3.

**A cold launch is 256 ms faster.** The warm arms overlap, so nothing is
claimed there; every other pair is disjoint. The cold pipelines are
**28.5 ms dearer** from a shipped library than from a source-built one,
reproduced in every settled launch and **unexplained**.

**The 34 ms of wall is not the 73.6 ms of pipelines**, and the gap is the
GPU probe running on its own thread under AppKit's initialisation, so
about half of that build never reaches the first present. Read the wall
row as what a cold cache costs a launch and the pipelines row as what it
costs the probe.

**The proxy, re-taken on the same Xcode so it can be compared at all**
(`BAUD_SHADER_CACHE_ASIDE=metal`, a binary whose embedded library is
corrupt so it compiles the source, four settled launches, both arms
reporting `Compiled`): with only the per-GPU cache aside the wall is
**173.0 ms [162.2-176.3]**. So a shipped library is 39.5 ms better than
the front end's cache alone, and note P's 193 ms -- taken on the previous
Xcode -- overstated what the front end's cache is worth.

The library is 48,900 bytes, generated by `scripts/build-metallib.sh`
from `baud-present-macos/src/shaders.metal` with
`Apple metal version 32023.921`, and committed. Every launch reports
which library it used -- `library 0.1 (Library)` -- because a cold arm
that quietly compiled the source would measure the old program.
