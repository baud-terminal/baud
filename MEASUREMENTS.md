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

**No baud-vs-ghostty result. The numbers are not stable enough to support one.**

Three runs of *identical* baud code on this host gave 195.9, 194.3 and 163.1
MB/s on streaming — a 17% spread, wider than almost every difference these
tables get used to argue about. The baud/ghostty verdict flipped with it:

| run | baud streaming | ghostty streaming | verdict |
|---|---|---|---|
| 1 | 195.9 [158.6-207.9] | 200.1 [189.6-205.1] | ranges overlap, no result |
| 3 | 163.1 [155.1-172.7] | 211.8 [194.6-223.4] | no overlap, ghostty ahead |

Run 3 differs from run 1 only by a row added *above* baud, which changed run
order. That is enough to flip the conclusion, so neither conclusion is one.
Recording it here rather than deleting it, because this project's notes already
list "orderings that flipped between runs and were reported as findings" as a
failure that has happened before.

What would make this host usable: many more reps, interleaved rather than
blocked ordering so thermal drift hits every core equally, and a quiet machine.
None of that has been done.

## The one thing measured here that is not in doubt

ghostty's terminal core can now be measured against baud at all, in the same
harness, through the same PTY, on the same payload bytes. That is new -- see
`bench-others/README.md`. The blocker this repo recorded (ghostty's build-time
`unicode_tables`) is obsolete: libghostty-vt is a shipped artifact with a C ABI.

The gross separations survive any amount of noise:

| core | streaming | TUI redraw |
|---|---|---|
| wezterm (full terminal) | 16.1 [15.8-16.3] | 28.4 [28.0-28.7] |
| vt100 crate | 118.5 [114.1-122.1] | 148.1 [142.6-149.0] |

wezterm's full terminal is an order of magnitude below every other core on both
payloads, in every run. Everything between alacritty and ghostty is inside the
noise floor of this host.

## Do not read the discard row as a ceiling

`multibench` prints a `ceiling (discard, no parse)` row. It is **not** an upper
bound, and the data says so: baud TUI (249.0) and ghostty TUI (272.0) both
exceed it.

`through_pty` times a producer-consumer pair. With a sink that discards, the
reader spins and blocks in `read(2)` continuously, so the pair pays more
scheduler round trips per byte; a sink that does work lets the writer refill
while the reader parses. The row is a different operating point, not a bound.

This matters because it invalidates the obvious way to compare across harnesses:
subtracting harness cost as if it were additive. It is not.

## kitty

*Harness `bench-others/kitty-bench` -- Python, same PTY shape, same payload
bytes dumped by multibench. Median of 9 over 32 MB.*

| core | streaming | TUI redraw |
|---|---|---|
| discard, no parse | 189.1 [178.8-201.7] | 190.8 [183.2-199.5] |
| kitty (`Screen.test_*`) | 101.7 [88.7-119.4] | 121.4 [116.1-135.4] |

kitty's parser is a CPython object and cannot be linked from Rust, so it is
driven through the same buffer-protocol cycle its own test suite uses. Three
Python calls per 4096-byte chunk are included in these numbers.

**These are not comparable to the Rust-harness tables above**, and the discard
row does not make them comparable, for the reason in the previous section. What
they establish is that kitty's parser runs, through a real PTY, on the same
bytes -- not where it ranks.

## The tty tax is not measurable here

Darwin has no `n_tty`, so there is no `N_TTY_BUF_SIZE` cap on `read(2)` and no
line-discipline cost. Every core is faster on this host than on Host A, and
alacritty gains most because the tax it pays on Linux is absent. That is a
hypothesis; confirming it needs `alacritty-bench`'s with/without-PTY split run
here, which has not been done.

baud also runs its **scalar** path on this host -- there is no NEON scanner --
while ghostty vectorises regardless of build flags, since
`src/terminal/stream.zig` calls `std.simd.suggestVectorLength` in
`consumeApcString` (:1046) and `consumeOscString` (:1089) ungated by
`build_options.simd`. So even with stable numbers, no build configuration on
this host would be like-for-like.

## What is not measured

Latency, frame rate, memory as a process. No renderer, no font shaping.
`baud-core` implements ~22 VT operations against alacritty's 66; the
differential test matches on 13 of 20 cases, so part of every margin is work not
being done.

- **foot.** Linux/Wayland only; cannot be built on Host B at all.
- **Host A has not been re-run** since `multibench` switched to median-of-9, and
  has never included ghostty. It remains the only host where baud has SIMD and
  where the tty tax exists.
