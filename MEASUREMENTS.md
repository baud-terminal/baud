# Measurements

## How to read this file

Every table is stamped with **host, harness, statistic, and payload generator**.
Those four things are what make two numbers comparable, and a table missing any
of them cannot be cross-compared with another — a rule this file previously
stated only for rep counts, which is how two tables measuring the same core at
318 and 204 MB/s sat three lines apart without either being wrong.

Ranges are printed everywhere. **If two ranges overlap there is no result**, and
saying so is the point of printing them. Two runs of identical code have
differed here by 9.3%, which is larger than most differences these tables get
used to argue about.

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

*Harness `bench-others/multibench` · statistic **median of 9** with ranges, over
32 MB · payloads generated inline in `multibench/src/main.rs`.*

**Not comparable to Host A in any respect.** Darwin has no `n_tty`, so there is
no `N_TTY_BUF_SIZE` cap on `read(2)` and no tty tax — every core is faster here
than on the Xeon, and alacritty benefits most because the tax it pays on Linux
is gone. baud also runs its **scalar** path: there is no NEON scanner.

### The ghostty comparison is not like-for-like, and cannot be made so here

ghostty is the first competing SIMD parser measured against baud. But
`ghostty/src/terminal/stream.zig` calls `std.simd.suggestVectorLength` in
`consumeApcString` (:1046) and `consumeOscString` (:1089) **ungated by
`build_options.simd`**, so `-Dsimd=false` reduces vectorisation without removing
it. Every ghostty row below is therefore *ghostty vectorised vs baud scalar*.
A like-for-like comparison needs x86-64, where baud has AVX2.

Both ghostty builds are shown because neither is a control.

**ghostty built with `-Dsimd=true` (its default)**

| core                          | streaming median [min-max] | TUI median [min-max] |
|-------------------------------|----------------------------|----------------------|
| **baud** (spans)              | **195.9** [158.6-207.9]    | **294.4** [233.8-305.7] |
| ghostty (libghostty-vt)       | 200.1 [189.6-205.1]        | 234.8 [210.9-260.6]  |
| termwiz (wezterm) parse only  | 157.1 [153.5-159.1]        | 156.0 [151.6-171.0]  |
| alacritty_terminal            | 147.5 [113.5-155.7]        | 150.9 [149.4-152.5]  |
| vt100 crate                   | 119.2 [118.6-128.8]        | 168.6 [151.8-170.8]  |
| wezterm (full terminal)       | 16.8 [15.6-17.3]           | 29.0 [28.5-29.6]     |

**ghostty built with `-Dsimd=false`**

| core                          | streaming median [min-max] | TUI median [min-max] |
|-------------------------------|----------------------------|----------------------|
| **baud** (spans)              | **194.3** [172.3-207.8]    | **265.6** [250.3-279.3] |
| ghostty (libghostty-vt)       | 208.7 [204.4-218.7]        | 274.2 [256.4-281.3]  |
| alacritty_terminal            | 142.9 [127.3-151.9]        | 157.1 [152.2-159.9]  |
| termwiz (wezterm) parse only  | 150.2 [146.9-162.6]        | 154.1 [148.4-160.5]  |
| vt100 crate                   | 117.0 [113.8-119.9]        | 147.0 [145.1-156.7]  |
| wezterm (full terminal)       | 17.0 [16.7-17.2]           | 30.0 [29.4-30.9]     |

### What these support, and what they do not

**baud vs ghostty: no result, on all four comparisons.** Every range overlaps,
in both builds and on both payloads. The medians move in opposite directions
between the two builds — ghostty 0.98x then 0.93x on streaming, 1.25x then 0.97x
on TUI — which is what noise looks like, not a trend. The two cores are
indistinguishable here.

That is worth stating precisely because it is easy to over-claim in either
direction. baud does not beat ghostty on this host. It also does not lose to it,
while running no SIMD at all against a vectorised parser. That is consistent
with the repo's own position that parser speed is not the binding constraint,
and it is the strongest thing these numbers support.

**ghostty `-Dsimd` on vs off: no result.** Ranges overlap on both payloads and
the flag trends the wrong way. Consistent with the flag not removing the
portable-vector paths.

**baud vs alacritty: a real result.** No range overlaps in any of the four
comparisons.

| | streaming | TUI redraw |
|---|---|---|
| ghostty simd=on run  | 1.33x | 1.95x |
| ghostty simd=off run | 1.36x | 1.69x |

Much smaller than Host A's ~4x, and the likely reason is the missing tty tax:
alacritty's Linux deficit is partly line discipline, which Darwin does not
charge. Unverified — it would need the `alacritty-bench` with/without-PTY split
re-run on this host to say so properly.

**baud vs termwiz: mixed.** Real on TUI (1.89x), no result on streaming (ranges
touch at 158.6/159.1). termwiz keeps no screen, so it does strictly less work
than any core it is listed beside.

## What is not measured

Latency, frame rate, memory as a process. No renderer, no font shaping.
`baud-core` implements ~22 VT operations against alacritty's 66; the
differential test matches on 13 of 20 cases, so part of every margin above is
work not being done.

Still absent as implementations:

- **kitty.** Its parser is a CPython object — `Parser` is declared
  `PyObject_HEAD` and `alloc_vt_parser(window_id)` resolves its dispatch target
  through kitty's global window registry — so it cannot be linked from Rust. It
  is reachable through `Screen.test_create_write_buffer` /
  `test_commit_write_buffer` / `test_parse_written_data`, which is the same
  buffer-protocol cycle baud uses, at a cost of three Python calls per 4096
  bytes that would have to be measured and reported alongside.
- **foot.** Linux/Wayland only; cannot be built on Host B at all.
- **Host A has not been re-run** since `multibench` switched to median-of-9, nor
  has it ever included ghostty.
