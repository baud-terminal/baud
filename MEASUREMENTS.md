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

# Host A — Linux x86-64

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

# Host B — macOS arm64 (Apple Silicon)

*Harness `bench-others/multibench` · median of 9 over 32 MB, **interleaved and
rotated** · payloads generated inline in `multibench/src/main.rs`.*

Not comparable to Host A: Darwin has no `n_tty`, so no `N_TTY_BUF_SIZE` cap on
`read(2)` and no tty tax. **baud runs its scalar path here** -- there is no NEON
scanner -- while ghostty vectorises regardless of build flags (below).

| core | streaming median [min-max] | TUI median [min-max] |
|---|---|---|
| ghostty (libghostty-vt) | 214.2 [204.8-224.6] | 267.0 [242.9-284.0] |
| **baud** (spans, scalar) | **192.8** [152.6-206.4] | **280.7** [178.6-306.6] |
| ceiling (discard, no parse) | 180.8 [170.5-194.5] | 181.5 [172.1-208.2] |
| termwiz (wezterm) parse only | 157.9 [153.6-169.3] | 157.8 [141.6-165.2] |
| alacritty_terminal | 140.5 [132.7-149.7] | 153.0 [151.1-165.1] |
| vt100 crate | 118.2 [99.9-122.1] | 148.8 [109.1-159.6] |
| wezterm (full terminal) | 16.1 [15.8-16.6] | 27.5 [27.3-28.0] |

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
obvious suspect is `Storage::boxed()`, a ~1.7 MB zeroed allocation built fresh
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
`baud-core` implements ~22 VT operations against alacritty's 66; the
differential test matches on 13 of 20 cases, so part of every margin is work not
being done.

- **foot.** Linux/Wayland only; cannot be built on Host B at all.
- **Host A has not been re-run** since `multibench` switched to median-of-9 and
  interleaved ordering, and has never included ghostty or kitty. It remains the
  only host where baud has SIMD and where the tty tax exists.
- **baud's variance on this host** is 3x every other core's and is unexplained.
