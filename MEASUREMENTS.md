# Measurements

## How to read this file

Every table is stamped with **host, harness, statistic, and payload generator**.
Those four things are what make two numbers comparable, and a table missing any
of them cannot be cross-compared with another — a rule this file previously
stated only for rep counts, which is how two tables measuring the same core at
318 and 204 MB/s sat three lines apart without either being wrong.

Ranges are printed everywhere. **If two ranges overlap there is no result**, and
saying so is the point of printing them. That is a floor, not a guarantee: runs
of identical code have differed here by 17% *between* invocations, wider than
any single run's range, so two non-overlapping ranges from one run are still not
a result unless the ordering has been varied. See Host B.

Ratios within one table are the finding. Absolute numbers are not portable
across hosts and the two hosts below are not comparable to each other at all.

---

# Host D — macOS arm64 (Apple M5 Max) — **current**

*Apple M5 Max, 18 cores, 48 GiB. Darwin 27.0.0 arm64. Commit `2d23dc8`,
2026-08-27. `bench-others/multibench`, `TRANSPORT=memory`, medians of 9,
interleaved and rotated ordering. Payloads from `baud-bench/src/payload.rs`,
generated fresh per write.*

**This is the only section measured against the current implementation.**
Everything below it predates some or all of the VT protocol work, and each
older host now says so in its own heading. Host A in particular has never been
re-run under interleaved ordering, which the ordering section below shows was
biasing baud by 16–20%.

| core | streaming | TUI |
|---|---|---|
| baud (spans) | **1615.6** [1477.2–1639.3] | **1956.6** [1637.5–1980.4] |
| baud (cells, computed) | 1056.5 [1022.8–1068.1] | 963.8 [928.1–991.0] |
| baud (cells, templated) | 1053.6 [1011.3–1085.0] | 973.6 [937.0–979.3] |
| ghostty (libghostty-vt) | 1101.8 [1049.5–1116.9] | 1024.4 [994.0–1027.9] |
| termwiz (wezterm) parse only | 367.2 [352.5–373.9] | 356.0 [338.6–361.4] |
| alacritty_terminal | 174.9 [167.7–179.9] | 264.7 [258.7–267.0] |
| vt100 crate | 161.2 [159.5–165.4] | 213.9 [208.6–218.2] |
| wezterm (full terminal) | 17.1 [17.0–17.4] | 31.6 [31.0–32.2] |

A second run the same session agreed: baud spans 1616.2 / 1953.9, ghostty
1086.8 / 1011.2, baud cells 1058.2 / 977.3.

**What this does and does not say.**

In **span** storage baud is 1.47× ghostty streaming and 1.91× on TUI. No range
overlaps, in either run. That is a result.

In **cell** storage there is **no result within a run**, and an earlier edition
of this file wrongly stated flatly that ghostty was faster. Four runs, same
session:

| run | streaming ranges overlap | TUI ranges overlap |
|---|---|---|
| 1 | yes | yes |
| 2 | yes | no |
| 3 | yes | yes |
| 4 | yes | no |

Streaming overlaps in 4/4. TUI overlaps in 2/4 — an ordering that flips between
runs, which this project has mistaken for a finding before.

**Across** runs there is a directional signal. Ghostty's median exceeded baud's
in 8 of 8 comparisons (4 runs × 2 payloads); under a sign test that is
p = 0.004. The medians:

| | baud (cells) | ghostty |
|---|---|---|
| streaming | 1058.2 / 1056.5 / 1062.7 / 1049.5 | 1086.8 / 1101.8 / 1104.6 / 1085.8 |
| TUI | 977.3 / 963.8 / 948.7 / 964.5 | 1011.2 / 1024.4 / 1022.7 / 1015.9 |

So the honest statement is the two-part one, and neither half stands alone:
**no measured margin — the ranges overlap — but the direction is consistent and
favours ghostty on the cells path by roughly 3–6%.** That is a reason to look at
cell materialisation, not a number to quote.

The two modes are a configuration choice, not a benchmark selection: `OpMode`
is fixed at startup, the same status ghostty's `-Dsimd` has.

`ceiling (discard, no parse)` measured ~1.68 GB/s here and is *not* an upper
bound on a parser — see "Do not read the discard row as a ceiling".

---

# Host A — Linux x86-64 — **superseded**

*Predates the VT protocol work and the interleaved run ordering. Never re-run;
kept for the architectural findings (arena sweep, representation costs), not for
the cross-core comparison.*

Single-core Xeon 2.1 GHz, 3 GB RAM, kernel 6.18.44. baud runs its **AVX2**
scanner here. This is the only host where `n_tty` applies, so it is the only
host where the tty-tax numbers mean anything.

## Cores compared

*Harness `bench-others/multibench` · statistic **best of 5** over 32 MB ·
payloads generated inline in `multibench/src/main.rs`.*

| core                          | streaming | TUI redraw |
|-------------------------------|-----------|------------|
| **baud**                      | 318-347   | 275-286    |
| alacritty_terminal            | 60-75     | 95-99      |
| termwiz (wezterm) parse only  | 60-61     | 63-64      |
| vt100 crate                   | 46-50     | 73-77      |
| wezterm (full terminal)       | 7.1       | 14.9       |

> **Superseded statistic.** `best of 5` returns the maximum of five runs and
> discards the rest — an upward-biased estimator that also hides variance.
> `multibench` now reports median-of-9 with ranges, like `baud-bench` always
> has. These figures have not been re-run on this host; treat them as
> provisional and expect the medians to land lower.

termwiz discards actions and keeps no screen, so it does strictly less work than
the others; it is wezterm's parse floor, not a terminal. Wezterm's own parser
runs at 61 MB/s while its full terminal runs at 7.1, so roughly 90% of wezterm's
cost is applying actions to the model rather than parsing.

## baud alone

*Harness `baud-bench` · statistic **median of 9** over 48 MB ·
payloads from `baud-bench/src/payload.rs`.*

| mode                      | streaming | TUI redraw |
|---------------------------|-----------|------------|
| spans, zero-copy          | 204-211   | -          |
| viewport cells, computed  | 190-193   | 185-189    |
| viewport cells, templated | 189-191   | 186-191    |
| spans + promote hybrid    | -         | 185-187    |

**Do not cross-compare this with the table above.** Different harness,
different statistic, and — the part that was never written down — *different
payloads*. `payload.rs` addresses an arbitrary column and emits periodic
ECH/IL/ED; `multibench`'s TUI payload does neither. `payload.rs`'s own comment
records that the arbitrary column took promotions from 6,540 to 19,709, which is
exactly the case span storage is worst at. The two tables measure different
work, not the same work twice.

## The tty tax, measured on alacritty

*Harness `bench-others/alacritty-bench` · Linux only.*

| payload    | from memory | through a PTY | cost |
|------------|-------------|---------------|------|
| streaming  | 58-75       | 43-51         | -30% |
| TUI redraw | 111-115     | 91            | -19% |

This is the line discipline, not the PTY as such, and it applies to every
terminal on Linux. It is the argument for a custom `tty_ldisc` independent of
anything in this repo.

---

# Host C — x86-64 Linux (GitHub-hosted runner) — **superseded**

*Predates the VT protocol work. The PTY-transport findings here still hold —
this box has a real `n_tty` and the current host does not — but the absolute
throughput figures do not describe the current parser.*

*Harness `bench-others/multibench` · median of 9 over 32 MB, interleaved and
rotated · `.github/workflows/bench.yml`, run 33016512979.*

The only host where baud's **AVX2** path and a real `n_tty` both exist. AVX2 is
not inferred: `multibench` calls `feed_avx2` unconditionally on x86-64 with no
runtime detection, so a runner without it would fault rather than measure.

A shared runner is noisy and this is not a merge gate. Ratios within one run are
the finding; absolutes are not portable off it.

## From memory (no PTY) — the parser comparison

| core | streaming median [min-max] | TUI median [min-max] |
|---|---|---|
| **baud** (spans, **AVX2**) | **2023.0** [1980.1-2096.3] | **2684.0** [2616.2-2697.4] |
| baud (cells, computed) | 1202.0 [1171.9-1217.2] | 1153.3 [1144.7-1170.5] |
| baud (cells, templated) | 1198.8 [1176.5-1227.3] | 1151.2 [1141.9-1172.2] |
| ghostty (libghostty-vt) | 969.5 [880.6-994.3] | 998.1 [750.3-1002.5] |
| alacritty_terminal | 142.7 [115.7-157.6] | 225.9 [172.6-228.8] |
| vt100 crate | 127.1 [123.1-128.8] | 158.2 [147.3-159.6] |
| termwiz (wezterm) parse only | 107.5 [106.4-107.6] | 112.5 [111.8-112.6] |
| wezterm (full terminal) | 13.3 [13.2-13.3] | 26.7 [24.4-26.8] |

No range overlaps against any core, on either payload.

| baud (spans) vs | streaming | TUI redraw |
|---|---|---|
| **ghostty** | **2.09x** | **2.69x** |
| alacritty_terminal | 14.2x | 11.9x |
| vt100 | 15.9x | 17.0x |
| termwiz (parse only, keeps no screen) | 18.8x | 23.9x |
| wezterm (full terminal) | 152x | 100x |

Wider than Host B's 1.55x/2.14x, which is what having a SIMD scanner should
buy: baud runs AVX2 here and scalar there.

## Through a real PTY — where the advantage disappears

| core | streaming median [min-max] | TUI median [min-max] |
|---|---|---|
| ghostty (libghostty-vt) | 232.1 [178.5-239.6] | 221.6 [194.3-245.2] |
| baud (cells, computed) | 232.0 [222.3-236.5] | 239.5 [219.7-250.2] |
| baud (cells, templated) | 216.9 [172.1-240.8] | 232.3 [213.2-251.8] |
| **baud** (spans) | 202.1 [183.2-209.6] | 204.5 [188.7-219.1] |
| alacritty_terminal | 82.1 [77.4-88.5] | 107.1 [103.7-108.8] |
| termwiz (wezterm) parse only | 80.1 [79.5-80.9] | 81.2 [78.6-83.3] |
| vt100 crate | 75.5 [75.2-76.8] | 89.1 [85.8-89.5] |
| wezterm (full terminal) | 12.3 [12.2-12.5] | 23.4 [22.6-23.6] |

**baud does not beat ghostty through a PTY here**, and this is the honest half of
the result. Spans drop from 2023 to 202 -- **-90%** -- and every fast core lands
in a 200-240 MB/s band that is the line discipline, not the parser. baud's cell
modes reach 232.0/239.5 and tie or edge ghostty; spans, which win by a factor of
two from memory, come last among the fast cores.

That is the same shape Host B shows without an `n_tty` at all, and it is the
argument this repo has always made: `read(2)` is capped at `N_TTY_BUF_SIZE - 1`,
so a core fast enough to saturate the tty is measured as the tty. A parser
cannot be compared to another parser through a channel neither can saturate.

The corollary is uncomfortable and worth stating: **on today's Linux tty, baud's
ingest advantage is not reachable by a real terminal.** Realising it needs the
transport changed -- the custom `tty_ldisc` this file already argues for, or the
shared-memory path `Storage` being a caller-owned `&mut [u8]` exists to allow.

---

# Host B — macOS arm64 (Apple Silicon) — **superseded**

*Predates DCS dispatch, side margins, OSC 8 and IRM. Three sections below reach
different verdicts on ghostty; they were taken at different times against
different code and different transports, and none of them is current. Host D at
the top of this file is.*

## The PTY was the constraint, and it hid every difference

*Harness `bench-others/multibench` · median of 9 over 32 MB, interleaved and
rotated · three runs · `TRANSPORT=memory`, no PTY and no syscalls.*

`MEASUREMENTS.md` has always recorded the tty tax for *alacritty* and never for
baud. Measuring it finally explains why every through-a-PTY table in this file
compresses the field into a narrow band: on Darwin the transport caps the
pipeline near 200 MB/s, and a core fast enough to reach that cap is measured as
"200 MB/s" no matter how much faster it is.

| core | streaming median [min-max] | TUI median [min-max] |
|---|---|---|
| **baud** (spans, **scalar** on this host) | **1741.7** [1667.2-1757.6] | **2276.0** [2212.2-2313.3] |
| ghostty (libghostty-vt, vectorised) | 1123.9 [1115.0-1135.7] | 1062.6 [1034.9-1071.3] |
| baud (cells, templated) | 1113.3 [1100.0-1118.6] | 1104.1 [1100.4-1117.7] |
| baud (cells, computed) | 1102.6 [1096.8-1117.2] | 1105.8 [1090.6-1113.8] |
| termwiz (wezterm) parse only | 352.8 [351.4-353.8] | 343.2 [338.5-344.5] |
| alacritty_terminal | 225.2 [200.7-234.4] | 275.6 [273.5-277.3] |
| vt100 crate | 169.6 [167.1-171.1] | 220.1 [219.0-221.6] |
| wezterm (full terminal) | 17.8 [17.6-18.0] | 32.4 [32.3-32.5] |

baud's three representations are all listed because each is a startup
configuration a user could run -- the same status ghostty's `-Dsimd` flag has.
Spans lead on both payloads here; the cell modes are within noise of ghostty.

### Both payloads: a result

Two runs, from memory, after the VT protocol work.

| baud (spans) vs | streaming | TUI redraw |
|---|---|---|
| **ghostty** | **1.55x** | **1.97x** |
| termwiz (parse only, keeps no screen) | 4.4x | 5.4x |
| alacritty_terminal | 7.8x | 7.6x |
| vt100 | 10.4x | 9.5x |
| wezterm (full terminal) | 104x | 65x |

No range overlaps, on either payload, in either run: baud
1756.0 [1642.1-1765.5] and 2076.7 [2033.7-2113.5] against ghostty's
1135.8 [1106.4-1147.0] and 1055.6 [1035.2-1074.2].

### What the protocol work cost, and how it was recovered

Correctness is not free, and the bill went on the record before it was paid off:

| baud, from memory | before VT work | after protocol work | after the StyleWord redesign |
|---|---|---|---|
| streaming | 1741.7 | ~1600 (-8%) | **1756.0** |
| TUI | 2276.0 | ~2120 (-7%) | 2076.7 |

ghostty was untouched throughout and moved 1123.9 -> 1135.8, so these are the
changes and not drift.

Two causes, one avoidable and one not:

* Two `modes.is_set(AutoWrap)` reads sat **inside** `put_run`'s inner loop,
  which runs per span. Hoisting them recovered 1540 -> 1600.
* The rest came back with the `StyleWord` redesign, which was made for
  correctness rather than speed. `StyleWord::DEFAULT` is now all-zero, so a
  `new_box_zeroed` `Storage` starts every template slot at the default rendition
  instead of at an arbitrary grey that had to be interned on first use.

Streaming ended slightly **above** where it started while the terminal now
applies attributes and background colours it previously discarded. TUI is ~9%
below its pre-protocol figure and that is not recovered.

That is with baud on its **scalar** path -- there is no NEON scanner -- against a
ghostty that vectorises. The advantage is not parser speed.

The TUI figure was 1048.6 until `erase_line` stopped filling a row it had
already truncated: see "Erase to end of line" below.

### The tty tax, measured on baud for the first time

| core | from memory | through a PTY | transport cost |
|---|---|---|---|
| **baud** | 1699 | ~165-193 | **-89%** |
| ghostty | 1121 | ~210-226 | -81% |
| alacritty | 216 | ~140-148 | -33% |
| wezterm | 17.8 | ~16 | -10% |

The gradient is the finding. wezterm is compute-bound and barely notices the
PTY; alacritty loses a third, close to the -30% this file already recorded for
it on Linux; baud loses **89%**, because it is fast enough that the transport is
essentially all of its measured time. A parser cannot be compared to another
parser through a channel that neither of them saturates.

### Erase to end of line

`CSI K` on the TUI payload fires once per ~77 input bytes, against a 200-column
grid. `erase_line` set `nrun` to ZERO for an unpromoted line in `Runs` mode --
the O(1) truncation that *is* the erase, since the viewport is a projection of
the spans -- and then promoted the line and filled `[cx, cols)` anyway.

That fill was ~134 cell writes per 77 input bytes: **write amplification of
roughly 20:1**, against rule #1's <= 1, to blank a row that had already been
truncated.

Removing it took the TUI figure from 1048.6 to 2276.0 MB/s, 2.17x, and turned a
tie with ghostty into 2.14x. Pinned by
`known_bugs.rs::erase_to_end_preserves_text_left_of_cursor`, which asserts in
all three modes that `CSI K` keeps `[0, cx)` -- chunk-invariance compares baud
against baud and would not have caught the difference.

### Streaming: a result

Three runs, ratios 1.49x, 1.51x, 1.52x. No range overlaps against any core.

| baud vs | ratio | overlap |
|---|---|---|
| ghostty | **1.52x** | none |
| termwiz (parse only, keeps no screen) | 4.5x | none |
| alacritty_terminal | 7.9x | none |
| vt100 | 10.1x | none |
| wezterm (full terminal) | 95x | none |

That is with baud running its **scalar** path — there is no NEON scanner — against
a ghostty that vectorises. See `docs/research/notes/b-data-parallel-fsm.md` for why that is
consistent with parser speed not being where baud's advantage comes from.

### TUI: not a result

baud 1076.1 [929.4-1084.8] against ghostty 1047.7 [1033.2-1052.7]. Ranges
overlap in all three runs; the medians differ by 2.7%, and baud's lower bound is
below ghostty's. **baud does not beat ghostty on the TUI payload.** That is the
open half of the comparison.

Note also that the TUI payload here omits the arbitrary-column addressing and
the ECH/IL/ED operations that `baud-bench`'s payload carries, and `payload.rs`
records that arbitrary columns tripled promotions. The case span storage is
worst at is under-represented in this table.

## Run order was biasing the results, and now does not

Cores used to be measured in blocks -- nine reps of one, then nine of the next.
That let run position bias a core: inserting a row *above* baud cost it 16%
while every other core moved under 6%.

`multibench` now interleaves rep-major and rotates the starting core by the rep
number, so no core keeps a fixed position. Between-run drift in the streaming
median, across two runs of each scheme:

| core | blocked | interleaved |
|---|---|---|
| **baud** | **20.1%** | **8.9%** |
| ghostty | 5.8% | 0.2% |
| alacritty | 6.3% | 6.0% |
| termwiz | 4.6% | 2.0% |
| wezterm | 5.6% | 1.3% |
| vt100 | 1.9% | 0.6% |

baud remains the noisiest core by a wide margin, within runs as well as between
them: its streaming range spans ~35% against ghostty's ~10%. Unexplained. The
obvious suspect is `Storage::boxed()`, a ~24 MB zeroed allocation built fresh
for each of the nine reps, but that is a hypothesis and has not been tested.

## baud vs alacritty: a result

No range overlaps, in either interleaved run.

| | streaming | TUI redraw |
|---|---|---|
| run 1 | 1.34x | 1.68x |
| run 2 | 1.37x | 1.83x |

Much smaller than Host A's ~4x. The likely reason is that alacritty's Linux
deficit is partly line discipline, which Darwin does not charge -- untested
here, and it would need `alacritty-bench`'s with/without-PTY split re-run on
this host.

## baud vs ghostty: no result within a run, a directional signal across them

Ranges overlap on both payloads in both runs, so no single run supports a
ranking. But the sign is consistent: **ghostty's streaming median exceeded
baud's in all five runs measured** -- 200.1, 208.7, 211.8, 213.7, 214.2 against
195.9, 194.3, 163.1, 177.1, 192.8 -- including the two blocked runs where baud
had the positional advantage of going first. A sign test over five independent
runs gives p = 0.031.

Suggestive, not settled. On TUI the direction flips between runs (0.96x then
1.05x), which is what no result actually looks like.

The honest reading of both together: **baud, running no SIMD at all, is within
about 10% of a vectorised ghostty on streaming and indistinguishable from it on
TUI.** That is consistent with this project's own position that parser speed is
not the binding constraint. It is not a claim that baud is faster.

### And it cannot be made like-for-like here

`ghostty/src/terminal/stream.zig` calls `std.simd.suggestVectorLength` in
`consumeApcString` (:1046) and `consumeOscString` (:1089), **ungated by
`build_options.simd`**, so `-Dsimd=false` reduces vectorisation without removing
it. Measured both ways, the flag produced no defensible difference. A
like-for-like comparison needs x86-64, where baud has AVX2.

## Do not read the discard row as a ceiling

It is not an upper bound and the table above shows why: baud (280.7) and ghostty
(267.0) both exceed it on TUI.

`through_pty` times a producer-consumer pair. With a sink that discards, the
reader blocks in `read(2)` continuously and the pair pays more scheduler round
trips per byte; a sink that does work lets the writer refill while the reader
parses. It is a different operating point, not a bound -- so harness cost cannot
be subtracted as if it were additive, which is the only way the kitty numbers
below could have been made comparable to these.

## kitty

*Harness `bench-others/kitty-bench` -- Python, same PTY shape, same payload
bytes dumped by multibench. Median of 9 over 32 MB, blocked.*

| core | streaming | TUI redraw |
|---|---|---|
| discard, no parse | 189.1 [178.8-201.7] | 190.8 [183.2-199.5] |
| kitty (`Screen.test_*`) | 101.7 [88.7-119.4] | 121.4 [116.1-135.4] |

kitty's parser is a CPython object and cannot be linked from Rust, so it is
driven through the same buffer-protocol cycle its own test suite uses. Three
Python calls per 4096-byte chunk are included.

**Not comparable to the tables above**, and the discard row does not make them
comparable. What these establish is that kitty's parser runs, through a real
PTY, on the same bytes -- not where it ranks.

## What is not measured

Latency, frame rate, memory as a process. No renderer, no font shaping.
`docs/vt-coverage.md` carries 126 rows at **done** and none at `todo`. The
operation gap that earlier editions of this file described -- "~22 VT
operations against alacritty's 66" -- is closed, and every table below
predating that work is marked with the host and date it was taken on.

The differential test's "13 of 20 cases" figure is **stale and has not been
re-run**: `baud-difftest` needs an alacritty checkout this box does not have.
It predates every VT commit from the selective-erase work onward.

- **foot.** Linux/Wayland only; cannot be built on Host B at all.
- **Host A has not been re-run** since `multibench` switched to median-of-9 and
  interleaved ordering, and has never included ghostty or kitty. It remains the
  only host where baud has SIMD and where the tty tax exists.
- **baud's variance on this host** is 3x every other core's and is unexplained.
