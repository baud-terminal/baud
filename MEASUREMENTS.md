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

> [!WARNING]
> **Superseded. The current headline is in "After the fix" below.** This table
> is commit `2d23dc8` and is kept because the regression section compares
> against it. `2d23dc8` still measures 1708.6 on spans today, so it is not
> merely an old host -- it is a state the code has not returned to.

**This section is measured against `2d23dc8`, not against HEAD.**
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

## Both layout anomalies are ~0.9% real and ~5x inflated by the harness

*`baud-bench/src/bin/ingest.rs`, 3000 reps per run, interleaved pairs, **on
mains with the machine thermally settled**. The first attempt at these was on
battery and both numbers were wrong -- one too large, one too small.*

| anomaly | in multibench | isolated, on mains | pairs |
|---|---|---|---|
| `036a432` reject overlong UTF-8 | −5% | **−0.8%** | slower in 8 of 10 |
| `a4cecba` width tables from Unicode | −5% | **−0.9%** | slower in 10 of 12 |

Medians: `036a432` 1.895 s against 1.91; `a4cecba` 2.89 s against 2.915.

**The two agree, and an earlier edition of this file said they did not.** On
battery `036a432` measured as *exactly* null over ten pairs and `a4cecba` as
1.7%, and the conclusion drawn was that the class is "inflated by the
comparison binary, not invented by it -- and not uniformly". The real picture
is simpler: both changes add code or rodata to baud's binary, both cost about
nine tenths of one percent there, and `multibench` -- which links six terminal
implementations together -- multiplies that by roughly five.

**What each measurement error was.** Battery noise hid the 0.8% at `036a432`
entirely, which is the ordinary failure: a small effect inside the spread reads
as zero. It also *inflated* `a4cecba` to 1.7%, which is the less obvious one --
the same noise that hides a real effect can manufacture one. Only the settled
mains runs separate them, and the first eight pairs of the `a4cecba` re-run had
to be discarded because the machine was charging from 31% and spread from 3.02
to 5.89 s.

So the residual is real, small, and still unexplained: the streaming payload is
pure ASCII and calls neither the new decoder path nor `Width::of`. 5.4 KB of
rodata and a slightly larger `feed_with` are what is left to explain 0.9%.

## Outlining the cold Ground arms: neutral, and how nearly it wasn't

*`a5a7c61` against its parent, multibench, `TRANSPORT=memory`, medians of 9,
eight interleaved pairs, both builds warm.*

| pair | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---|---|---|---|---|---|---|---|
| before | 1293.8 | 1321.6 | 1315.0 | 1323.4 | 1319.5 | 1210.3 | 1354.1 | 1301.6 |
| after | 1307.6 | 1327.7 | 1321.8 | 1313.7 | 1318.4 | 1339.4 | 1307.1 | 1301.4 |

**Four wins each.** The first three pairs all favoured the change, which reads
as a small consistent gain and is the point at which it was nearly written up
as one. A sign test on 3/3 is p = 0.125 -- never a result -- and five more
pairs went 1-4 the other way.

The change shrinks `feed_with` from 14,968 to 13,488 bytes, -9.9%, and is kept
for that and for the annotation being true. It is **not** kept for throughput,
and the eight pairs are recorded so nobody re-derives a win from the first
three.

## The ceiling on zero-copy line moves: 22x on a pager

*`ingest`, `OpMode::Computed`, medians of 60 reps. The "no moves" arm deletes
the row-copy loops from `insert_lines_in` and `delete_lines_in` and keeps the
blanking. Its output is **wrong**; it exists only to bound the win, the same
way `null_ceiling` bounds the PTY.*

| recording | real | line moves removed | |
|---|---|---|---|
| **less-pager** | 25.6 / 28.6 / 28.7 | **440.9 / 622.7 / 619.4** | **~22x** |
| vim | 252.0 / 262.0 / 259.5 | 259.0 / 259.6 / 261.2 | flat |

**A pager is almost entirely bound by moving lines**, and nothing else in the
corpus is: vim does not move them at all. `less-pager.raw` profiles at 41.8% in
`insert_lines_in`, `delete_lines_in` and `_platform_memmove` together, and this
is what that 41.8% is worth.

**The same shape as the win this project already took.** `RowRot` made
full-screen scroll zero-copy by advancing an index instead of copying cells:
41.3 MB/s to 907.2, also about 22x. It does not generalise to a *region* or to
`IL`/`DL`, which is exactly what a pager uses, so a pager pays the copy that a
scrolling shell does not.

The generalisation is a per-row indirection: a permutation table instead of a
single rotation, so moving lines permutes indices rather than cells. The
scaffolding is already there -- `View::phys` is the single point every
addressing path goes through, which is what made `RowRot` one line rather than
twenty-six chances to forget it.

Two costs to weigh before building it, neither measured yet:

* `phys` becomes a table load instead of an add and a conditional subtract, on
  **every** cell access. A regression there hits every workload, including the
  ones that never move a line.
* `Term::viewport` un-rotates in place with one `rotate_left`. An arbitrary
  permutation needs a gather instead, once a frame.

## The cells path, instrumented then fixed

*`baud-bench/src/bin/ingest.rs`, `OpMode::Computed`, medians of 60 internally
timed reps, on mains. The recordings, because the synthetic payloads report
`utf8_share` 0.0% and reach none of this.*

| payload | session start | now | |
|---|---|---|---|
| btop (truecolour, box drawing) | 306 | **~440** | **+44%** |
| cjk (pure multi-byte) | 123 | **~150** | **+22%** |
| vim (CJK, emoji) | 222 | **~255** | **+15%** |
| streaming (ASCII) | 840 | **~1075** | **+28%** |

Three changes, each found by instrumenting rather than by reading:

1. **`put_ascii_run` was scalar.** `Term::cell_attribution` put 27% of
   streaming's cells through it for 33% of the profile, against `slice::fill`
   doing the other 73% in 17% -- 5.3x per cell for the same eight bytes.
   `otool -tv` confirmed no vector register in the function. Vectorised: +30%
   on streaming.
2. **A 1 MB template buffer nothing read.** `intern_style` filled 2048 cells
   per style-table miss; btop misses 109,274 times, so ~223 million cell
   writes for a buffer `grep` finds no reader of. +27% on btop, and 1 MB off
   `Storage`.
3. **Two unguarded binary searches per codepoint.** `ClusterBreak::advance`
   searched `EXTENDED_PICTOGRAPHIC` and `INCB_CONSONANT` for every codepoint
   above `U+00A9`. A 32-codepoint-granularity membership bitmap skips both for
   98.9% of the space: +6% btop, +11% vim, +17% cjk.

**And one that failed**, which is why the third one is worth trusting: a bitmap
over `BREAK_PROPERTY` measured flat and was reverted. That search sits behind
`GraphemeClass::of`, which already had an ASCII fast path, and was never the
cost -- the cost was one level down. Same technique, wrong target.

## `fast_utf8`: decoding a character without a dispatch per byte

*Apple M5 Max, 2026-08-27, `5ae7df4` against its parent, `baud-bench` memory
transport, medians of 9, three runs interleaved with both builds warm before
any timing. Payloads are the real recordings, because the synthetic ones
contain no multi-byte text at all.*

| recording | before | after | |
|---|---|---|---|
| btop (box drawing, `U+250x`) | 284.9 / 289.1 / 291.6 | **309.2 / 310.8 / 315.7** | **+7.5%** |
| vim (CJK, emoji) | 212.7 / 209.9 / 217.1 | **227.9 / 220.3 / 230.0** | **+7.1%** |

Every median after exceeds every median before, on both recordings. The sets do
not overlap.

**Why the synthetic payloads cannot show this.** `payload::streaming` and
`payload::tui` are pure ASCII -- `Term::utf8_share` reports 0.0% for both -- so
they never execute the path at all. This is the "payloads that don't exercise
the case" entry in `CLAUDE.md`'s own list, and it is why the first estimate of
this opportunity was ~5% and wrong: it was reasoned from streaming.

**A first attempt at this measurement was discarded**, and the reason is worth
keeping: it interleaved `git stash` with the timing, so a rebuild ran
concurrently with a benchmark. It produced 131.7 and 172.8 for an arm whose
real value is ~289. Both builds are now warm before any run starts.

## Open regression: streaming lost two thirds, and TUI did not

*Apple M5 Max, 2026-08-27, same session, `bench-others/multibench`,
`TRANSPORT=memory`, medians of 9, interleaved and rotated. ghostty is the
control: it is built from an unchanged checkout and moved 1%.*

| streaming, in-memory | `2d23dc8` | `cb9078e` (HEAD) | |
|---|---|---|---|
| baud (spans) | **1708.6** [1339.4–1744.0] | **578.4** [410.0–585.6] | −66% |
| baud (cells, computed) | **1096.8** [1072.4–1103.8] | **584.4** [570.9–591.8] | −47% |
| ghostty — *control* | 1120.9 [1110.3–1127.4] | 1132.4 [1119.5–1147.3] | +1% |

| TUI, in-memory | `2d23dc8` | `cb9078e` (HEAD) | |
|---|---|---|---|
| baud (spans) | 1999.2 [1713.4–2056.9] | 1847.8 [1796.7–1911.3] | −8% |
| baud (cells, computed) | 945.7 [934.1–962.7] | 778.4 [761.9–786.8] | −18% |
| ghostty — *control* | 1050.9 [1032.7–1057.5] | 1049.4 [1030.1–1060.1] | 0% |

No range overlaps on streaming, in either representation. This is a result,
and it is a regression.

**How it went unnoticed is the more useful half.** The A/B that should have
caught it was run on `baud-bench`, which reports ~230 MB/s for *both* commits
and resolves nothing — its variance swamps a 3× effect. Every increment since
`2d23dc8` was checked against a harness that could not see this, while the
harness that could was only ever run when a number was being quoted. A gate
that runs on demand is not a gate.

### After the fix

*Same box, 2026-08-27, commit `b6429f4`, `TRANSPORT=memory`, medians of 9,
interleaved, two runs.*

| core | streaming | TUI |
|---|---|---|
| baud (spans) | **1310.8** [1015.7-1342.0] / **1349.0** [976.7-1385.5] | 1520.8 [1441.1-1543.9] / 1852.5 [1730.7-1887.5] |
| ghostty — *control* | 1127.3 [1114.3-1141.1] / 1119.6 [1096.2-1138.1] | 1047.1 / 1045.1 |
| baud (cells, computed) | 915.5 [879.9-923.6] / 924.8 [885.6-935.5] | 873.8 / 926.3 |
| termwiz parse only | 378.4 / 376.9 | 366.9 / 364.9 |
| alacritty_terminal | 219.3 / 222.0 | 266.8 / 272.3 |
| vt100 crate | 166.8 / 166.5 | 217.4 / 216.2 |

Spans are **1.20x** ghostty streaming and **1.77x** TUI. Cells are **behind**,
924.8 against 1119.6 -- the "level on cells" claim was true at `2d23dc8` and is
not true here.

### Profiled: the gradual erosion under the cliff

The cliff is fixed and the number has not returned to `2d23dc8`'s. Measured
today on one box: `2d23dc8` 1708.6, `9f6ddc3` 1455.8, `657e1cb` 1407.2,
`886ce9e` 1382.6, `ac0ed98` 1368.2, HEAD ~1330.

That is a **monotone decline of about 21%** across the commits before the one
that caused the cliff, and it was nearly missed: the bisect log reads as a flat
plateau followed by a cliff only if you do not compare the plateau with its own
starting point. No commit in it is individually large enough for a bisect
threshold to catch, which is the shape a per-commit gate would find and a
periodic one would not.

### Profiled, and it is mostly not a defect

Nine points across `2d23dc8..9f6ddc3` on multibench, one worktree, ghostty
within 1102-1122 throughout. **Ranges are wide on several rows because a
duplicate run briefly contended for the box; treat the steps, not the
individual figures, as the result.**

| commit | spans, streaming | step |
|---|---|---|
| `2d23dc8` | 1690.1 | — |
| `43f91d4` VT52 and nine private modes | 1647.5 | −2% |
| `8b6b3c3` williams' anywhere block | 1647.6 | flat |
| `036a432` reject overlong UTF-8 | 1570.7 | −5% |
| `a4cecba` width tables from Unicode | 1524.5 | −3% |
| `931906a` UAX #29 clustering | 1561.1 | flat |
| `7f17251` ClusterBreak in the write path | 1549.1 | flat |
| `7c5e3a7` `ESC % @` switchable encoding | 1579.1 | flat |
| **`9f6ddc3`** | **1374.6** | **−13%** |

**The largest step is the last, and this file already priced it.** That range
carries `0a12283` and `29bd2fb` -- full-screen scroll did not move the grid,
and the fix rotates it. `CLAUDE.md` records the trade in its own words:
"Against the version that did not scroll at all (~1035) this costs about 12%,
and that is the honest price of scrolling." Before it, baud was fast partly
because full-screen scrolling **did not work**.

So the erosion decomposes into paid-for correctness rather than a defect:

* ~12-13% for scrolling that actually scrolls;
* ~3-5% for width tables generated from Unicode data, kept deliberately because
  the hand-written set gave every Arabic, Hebrew and Devanagari combining mark
  a column of its own;
* ~5% at `036a432`, which is **not** decoder work -- `blit` hands raw arena
  bytes to `put_ascii_run` and 95% of streaming never reaches the decoder, so
  this is the same layout class as the width tables and is unexplained;
* the rest inside the noise.

**None of it is recoverable without giving up correctness**, and the one step
that is unexplained is 5% of a synthetic ASCII payload. `2d23dc8` was a
terminal that mis-rendered combining marks, executed `ESC` mid-sequence, had no
grapheme clustering, and did not scroll. Its 1690 was not a better terminal.

Where the throughput actually is, is not here. Note B measures the state
machine at **57-90% of real capture bytes** with both preconditions favourable and nothing implemented, and note A
prices the decoder against the literature. The erosion is closed as understood; the opportunity is those.

### The commit

`6f2499a`, "Bound scroll blanking to what the row held". The bisect is a clean
cliff rather than a staged decline, which is worth stating because a staged one
would mean naming a single commit was wrong:

| commit | spans, streaming |
|---|---|
| `9f6ddc3` | 1455.8 |
| `657e1cb` | 1407.2 |
| `886ce9e` | 1382.6 |
| `ac0ed98` — parent | 1368.2 |
| **`6f2499a`** | **558.4** |
| `56240a4` | 532.9 |

Confirmed against its parent, interleaved, three pairs in one session:

| pair | `ac0ed98` | `6f2499a` | ghostty — *control* |
|---|---|---|---|
| 1 | 1412.5 [875.8–1424.6] | 573.9 [420.0–575.4] | 1149.6 / 1129.2 |
| 2 | 1361.9 [1346.7–1386.3] | 562.4 [423.0–566.9] | 1125.5 / 1123.6 |
| 3 | 1361.0 [753.0–1390.6] | 559.7 [514.8–568.0] | 1126.1 / 1129.3 |

### The fix, and what the arms rule out

Three variants of `fill_row_counted`, interleaved, three rounds each, rebuilt
between every arm. ghostty stayed within 1105.5-1136.2 across all nine runs.

| arm | spans, streaming | cells, computed |
|---|---|---|
| **head** — scan, shortened write, two `View` borrows | 520.2 / 568.8 / 567.1 | 549.7 / 576.7 / 576.8 |
| **C2** — same, but scan and fill share one borrow | 559.5 / 530.4 / 537.3 | 576.6 / 551.7 / 564.9 |
| **C1** — no scan, unconditional full-row write | **1334.0 / 1387.9 / 1402.4** | **920.9 / 945.9 / 946.6** |

**C2 refutes the obvious explanation.** The tempting story was the `blit`
failure this repo already records — a `View` helper rebuilt inside a loop,
since `Term::row` constructs a four-field borrow and `view()` then constructs a
second. Hoisting them into one borrow moved nothing: C2 tracks head in every
round. It was also a weak story on its arithmetic, `blit`'s being per *cell*
and this per *row blank*, roughly 1/200th the frequency.

So the cost is in the scan-and-shorten itself, and C1 is the fix.

**Which half of it, though, is still open**, and the two readings sound absurd
in opposite directions:

* the **read** costs — a backwards `rposition` over 1600 bytes per scroll,
  against a prefetcher that streams forwards; or
* the **shortened write** costs — writing `n` cells instead of the full row.

The second is not the strawman it looks like. `CLAUDE.md` already records a
measurement pointing that way: a variant that skipped blanking *entirely*,
strictly less work, measured 142-150 against 213-229. Writing less was slower
there too, and that anomaly is still unexplained.

`C3` (forward scan, shortened write) and `C4` (backwards scan, full write)
separated them, and **both stayed slow** -- 582.7 / 572.6 / 561.1 and 510.8 /
516.0 / 496.5. So neither the direction nor the shortened write is the cost:
the variable that tracks the outcome is whether the row is **read**.

`[[source: apple-cpu-guide]]` § 4.6.10 supplies a mechanism that fits all four
arms -- a store closely followed by an overlapping load causes a memory
ordering violation and the load is re-executed, which is `put_ascii_run`
storing cells and the next scroll's scan loading them. It also **refuted** the
cache explanation offered here first: § 4.20 documents the L1D as write-back
*write-allocate*, so the store allocates too and the read adds no fill the
write avoided. `docs/research/notes/d-memory-hierarchy.md` carries both, and
records the mechanism as a candidate because no counter has been read.

**`0e7cb84` is reverted by association, not separately measured.** It added the
same `rposition` pattern to `fill_counted`, the `EL`/`ED` erase paths, after
the bisect boundary. C1 reverts both functions, so the erase-path scan's own
cost is untested.

**The mechanism is not isolated, and two candidates changed together.**
`fill_row_counted` and `fill_counted` both gained a backwards `rposition` scan
of the row before every blank, reached through `Term::row` — which builds a
four-field `View` borrow with bounds checks, after which `view()` builds a
second one to do the fill. So the diff added *both* a reversed 200-cell read
per scroll and a doubled view construction, and either could dominate.

The tempting story is that this is the `blit` failure this repo already
records — a view helper rebuilt inside a loop. It is a weak story on its own:
`blit`'s was per *cell* and this is per *row blank*, roughly 1/200th the
frequency. The reversed access direction over 200 cells per scroll is the more
plausible dominant term. Naming one without separating them would make this the
third open anomaly in this repo recorded as a confident guess.

**What the commit's own message says is the useful part.** It reported
"baseline medians 185/203/214 against 226/214/227, higher in all three rounds,
but the ranges overlap. By this repo's rule that is not a result" — correct
reasoning applied to the wrong instrument. Those are `baud-bench` numbers, and
`baud-bench` reports ~230 MB/s for two commits that differ by 2.5× here. The
change looked free because the only harness pointed at it could not see it.

Also ruled out: **it is not the input work.** `fd8ad9a` and `cb9078e` measure
the same (546.0 / 578.4 spans, ranges overlapping), so the regression predates
the encoder, and the mode-bitset widening from `u16` to `u32` cost nothing.

**What this does and does not say.**

In **span** storage baud was 1.47× ghostty streaming and 1.91× on TUI **at
`2d23dc8`**. No range overlapped, in either run. That was a result; on
streaming it is no longer the current one.

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

## Widening `Attrs` to 32 bits (SGR 60-65)

Host B (arm64 macOS, portable scanner, no `n_tty`), 2026-08-27, `62475f2`
against `4c0cc92`. Two binaries built from the two commits and run
**alternating** (old/new, new/old, new/old) so an ordering effect cannot be
read as a result. Median of 9 per row, 48 MB/rep, real PTY, `payload::text`.

| row | old median | new median | old range | new range |
|---|---|---|---|---|
| spans, zero-copy | 200.3 | 201.2 | 186.8-214.5 | 193.6-214.7 |
| viewport cells, computed | 229.5 | 231.9 | 222.6-247.9 | 226.5-246.4 |

The ranges overlap completely, so **there is no result**: the ideogram aspects
cost nothing this benchmark can see. What would have proven the change wrong is
a new range sitting clear of the old one on the cells row, since that is the
path `StyleWord` is read on; it does not.

**The limitation matters more than the number.** Both payloads report `styles 0
interned / 208954 hits (100.0%)` -- one rendition for the whole run. `StyleWord`
went from 16 to 20 bytes and the style table from 1024 to 1280, and a payload
that interns nothing cannot exercise either. This is the "payloads that don't
exercise the case" trap in `CLAUDE.md`, and it is *not* worked around here: the
table above measures code layout, not the style table. Sizing that needs a
payload that cycles renditions faster than `NSTYLE`, which does not exist yet.

## Widening the CSI parameter array from 16 to 35

Host B (arm64 macOS, portable scanner), 2026-08-27, medians of 9, alternating
order, 48 MB/rep, real PTY. `Term::params` went from 16 to 35 entries (+76
bytes) and `SubParams` from `u16` to `u64` (+6), so `Term` grew ~82 bytes.

| row | 16 params | 35 params | 16 range | 35 range |
|---|---|---|---|---|
| spans, zero-copy | 195.0 | 198.9 | 182.5-207.8 | 178.1-210.3 |
| spans + promote hybrid | 219.3 | 219.5 | 211.0-229.9 | 207.8-233.0 |
| TUI through a PTY | 215.9 | 220.5 | 210.6-232.2 | 211.1-232.7 |

Ranges overlap on every row, so **there is no result**. What would have proven
it wrong is the TUI rows separating, since that payload is the one carrying
escape sequences; they do not.

The same caveat as the `Attrs` widening applies and is the reason both entries
say so: `payload::text` and `payload::tui` send short parameter lists, so this
measures `Term` getting bigger, not the deeper array being walked. A payload
that sends 30-parameter SGR does not exist here.

## Real capture, replayed through a PTY

Host B (arm64 macOS, portable scanner), 2026-08-27, `790272a`. Median of 9,
48 MB per repetition, `OpMode::Runs`, PTY transport — the same conditions as
the spans rows above, so these are comparable to them. Three runs, all shown.

| payload | 1 ESC per | run 1 | run 2 | run 3 |
|---|---|---|---|---|
| asciiquarium | 5.4 B | 129.0 [127.0-136.5] | 131.7 [128.6-137.1] | 131.2 [128.4-135.6] |
| btop | 16.5 B | 158.8 [154.8-159.5] | 159.4 [158.3-160.0] | 158.0 [156.9-158.9] |
| vim | 17.2 B | 158.5 [154.8-167.2] | 161.5 [155.8-166.5] | 155.3 [153.7-166.0] |
| **`payload::tui`** (synthetic) | — | 235.4 [230.8-258.8] | 239.8 [215.2-253.2] | 228.9 [211.1-237.7] |

**The synthetic TUI payload is optimistic by about a third.** Real capture runs
at 131-159 MB/s where `payload::tui` reaches 229-240, and the ranges do not
overlap — btop tops out at 160.0 against the synthetic payload's floor of
211.1. This is the answer to the question `CLAUDE.md` had open: *replay real
capture instead of `payload::tui` and find out whether the margin survives*.
It does not survive intact.

**Throughput tracks escape density, not content.** btop and vim have almost the
same density (16.5 and 17.2 bytes per escape) and almost the same throughput
(158.7 and 158.0), despite one being truecolour box-drawing and the other
scroll regions over CJK and emoji. asciiquarium is three times denser at 5.4
bytes per escape and is 17% slower. Escapes leave the bulk ASCII path and go
through the state machine one byte at a time, so the ratio of the two is what
the number measures.

### Separating the transport from the parser

Same host and commit, median of 9, 48 MB a repetition, `OpMode::Runs`.
`Tr::Memory` copies the payload straight into the slot: no fork, no `read(2)`,
no line discipline, and the same `READ_CHUNK` slot size, so the parser sees the
chunking a PTY would give it and nothing else is left.

| recording | memory | pipe | PTY | PTY costs |
|---|---|---|---|---|
| btop | 304.0 [298.9-306.3] | 247.7 [235.4-249.0] | 157.2 [156.0-158.9] | **48%** |
| vim | 220.5 [192.8-221.3] | 174.1 [172.6-175.0] | 158.0 [152.5-168.2] | 28% |
| asciiquarium | 173.2 [171.9-174.2] | 138.9 [138.0-140.2] | 130.6 [126.9-137.7] | 25% |

**The tty tax is not a constant, it is a fraction of what is left.** btop loses
nearly half its throughput to the transport; asciiquarium loses a quarter. The
ordering is the same one escape density predicts: asciiquarium is parse-bound
at 5.4 bytes per escape, so the syscall is a smaller share of its total, while
btop at 16.5 spends more of its time waiting on the tty than working.

That also means **the memory column is the one to optimise against** for parser
work, and the PTY column is the one to quote for a terminal. Comparing a parser
change measured in memory against a throughput figure measured through a PTY
would flatter it by up to a factor of two.

The pipe column here is a **64 KiB** pipe, not the 1 MiB one `README.md` quotes:
`F_SETPIPE_SZ` is Linux-only and this is Darwin, which the harness comments
already say. It is a third transport, not a faster pipe.

**Two caveats, both making these an upper bound.** A recording is fixed, so
filling a 48 MB repetition replays a 138-524 KB capture 90-350 times: caches
and branch predictors see the same bytes repeatedly, which a live terminal
never does. And each is ten seconds of one program on one machine. The
direction of both errors is the same — reality is at or below these numbers,
not above.

## Write amplification, measured rather than asserted

Rule #1 -- "bytes written per byte ingested <= 1" -- is the first thing
`README.md` says every result here is predicted by, and it had **no
measurement**: the instrumentation that fed it printed `inf x amplification`
from counters nothing updated and was removed rather than repaired. It is back,
split into the quantities it was conflating.

| payload | mode | span | cell | of which blanking |
|---|---|---|---|---|
| `payload::streaming` | Runs | **0.283x** | 20.7x | 20.7x |
| `payload::streaming` | Computed | 0.000x | 28.3x | 20.7x |
| btop | Runs | **0.000x** | 1.9x | 0.0x |
| asciiquarium | Runs | **0.000x** | 4.5x | 1.4x |
| vim | Runs | **0.000x** | 12.3x | 9.1x |

**Span and cell are different quantities and only the first is rule #1.** A
`Run` costs `size_of::<Run>()` however many bytes it covers, so it amortises to
0.283x. A cell mode cannot compete and is not trying to: one ASCII byte becomes
an eight-byte `PackedCell`, so 8x is a floor, and reading the rule as covering
that makes it unsatisfiable by construction. `amplification.rs` asserts both --
span under 1, and text cells exactly equal to bytes in.

**On the synthetic payload, blanking is 73% of all cell traffic.** Scrolling
blanks the incoming row across the full width, so 60-column lines on a
200-column grid blank 200 cells to store 60 characters. That is the cost worth
attacking, and it is not the storing.

**And the span path is never used by a full-screen application.** All three
recordings enter the alternate screen and never leave, and `put_run`
materialises every write there -- "no scrollback" one layer down. So spans
measure `cat`, logs and shell output; btop, vim and asciiquarium never touch
them. The 1.47x on spans is a real number about a real workload, and it is not
the workload a TUI produces.

Real capture is also far cheaper in cells than the synthetic payload suggested
-- 1.9x for btop against 28.3x for `payload::streaming` -- because a TUI
repaints what changed rather than every cell, and btop never scrolls at all.

## The transport ceiling: what a PTY row can possibly reach

Host B, 2026-08-27. A consumer that only reads and discards -- same fork, same
payload, same transport, no parser behind it.

| transport | ceiling | reads for 48 MB | bytes per read |
|---|---|---|---|
| PTY | **198.6** | 49,152 | **1024** |
| pipe | **6144.3** | 12,288 | 4096 |

**The Darwin tty returns 1024 bytes per `read(2)`, not 4095.** That is a
quarter of what `CLAUDE.md`'s `READ_CHUNK` entry describes, which is a Linux
number, and it caps the PTY at 198.6 MB/s before any parsing happens at all.

Against that ceiling: baud reaches 117-160 through a PTY, 245 through a pipe
and 278 in memory on the same btop recording, with **identical write
amplification** in all three -- the same work and the same stores.

So a PTY row here is 60-80% of a ceiling it cannot exceed however fast the
parser gets, and the pipe ceiling is thirty-one times higher than anything baud
produces, which makes the pipe and memory columns the ones a parser change
shows up in. **The PTY column measures the operating system.**

This is why the measurement exists rather than the reasoning: "the PTY costs
45%" and "the process feeding it cannot keep up" produce the same number, and
nothing here could tell them apart until a consumer with no parser behind it
was timed.

## Where PTY-mode time actually goes, by sampling

Host B, 2026-08-27, macOS `sample` over 20 s of a running benchmark. No
dependency and no code change: a sampling profiler answers "where is the time"
where a hand-placed timer only answers "how long was the thing I already
suspected".

| symbol | samples | |
|---|---|---|
| `__wait4` | 12,422 | the harness reaping forked children |
| **`read`** | **3,296** | the PTY syscall |
| `feed_with` | 267 | baud |
| `scroll_region_up` | 248 | baud |
| `put_ascii_run` | 194 | baud |
| `dispatch`, `anywhere_c0` | 20 | baud |

`__wait4` runs *after* `t0.elapsed()`, so it is outside the timed region and
does not enter the reported MB/s -- but it is three quarters of the process's
life, which is worth knowing before reading any profile of this benchmark.

### Corrected by dtrace: that is waiting, not working

The reading above -- "`read` is 82%" -- is wall time, and `sample` counts a
thread blocked *in* a syscall as time spent there. Sampling only when the thread
is on-CPU in userland tells a different story:

| symbol | `sample` (wall) | `dtrace profile-4999` (on-CPU) |
|---|---|---|
| `read` | 82% | **3.7%** |
| `feed_with` | | **38%** |
| `scroll_region_up` | | **31%** |
| `put_ascii_run` | | **26%** |
| baud, total | 18% | **93%** |

Both are true and they answer different questions. The process spends most of
its *wall* time blocked waiting for the child to produce bytes; of the *CPU*
time it actually uses, 93% is baud's own code.

**The conclusion survives but the reason changes.** A PTY row still cannot show
a parser change -- but because the benchmark is **producer-bound**, not because
the syscall is expensive. The child cannot write fast enough, so a faster parser
would spend the difference waiting. That is the null-consumer ceiling seen from
the inside: 198.6 MB/s with no parser at all.

**And `scroll_region_up` is 31% of on-CPU time**, for an operation documented as
O(1). The ring advance is one addition; what is expensive is what got inlined
into it -- the row scan `fill_row_counted` does, which is `#[inline(always)]`.
So the blanking work moved rather than vanished, and the profile is where it
shows up rather than in the amplification counters.

## What is not measured

Latency, frame rate, memory as a process. No renderer, no font shaping.
`docs/vt-coverage.md` carries 149 rows at **done** and none at `todo`. The
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
