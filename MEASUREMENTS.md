# Measurements

Every number here names the commit that first carried it, and a number taken on
code that has changed since says so rather than passing for current. Superseded
results are not kept — `git log MEASUREMENTS.md` has them.

## How to read this file

A number without provenance is not a measurement. Every table below states:

* the **host**, because cross-machine comparison is meaningless;
* the **transport**, because a PTY on Darwin returns 1024 bytes per `read(2)`
  -- the capacity of the tty's output clist, `[[source: xnu-tty-h]]` -- and a
  pipe returns 4096, which bounds throughput independently of the parser;
* the **statistic** and **rep count**;
* the **payload**, and whether it is a committed recording or generated;
* the **commit** -- the first to carry the section's numbers -- and the
  **power state**, where its source states one.

Ranges are quoted as `[min-max]`. **If two ranges overlap there is no result.**

## Host

Apple M5 Max, macOS 27.0. Each section names its own commit and date, and its
power state where its source states one.

The compiler moved to rustc 1.96.0-nightly 02c7f9bec, the nightly of
2026-04-11, when devenv.lock moved to it, and no figure here was
changed: across the two compilers, ingest2 on nine recordings and
opcost found no rate that moved, and multibench found none where its
ghostty control held (less-pager, asciiquarium); its other rows were
void.

**Recordings captured again on 2026-10-28.** `utf8/btop.raw`,
`utf8/vim-unicode.raw`, `sixel/yazi-walk.raw`, the seven `tests/resize/`
gestures and `tests/scheme/tmux-start.raw` were captured again, and
`ascii/asciiquarium.raw` was replaced by `ascii/pipes.raw`
(`tests/recordings/README.md`, `tests/resize/README.md`). A figure below
dated before 2026-10-28 that names one of them was measured on the capture
that name held then, and is kept as its sweep measured it; it is not a
figure for the file in the tree now. The sections re-taken on the new files
say so.

## Against other implementations, on real capture

**Taken before `multibench` aligned every function to 128 bytes**, as is
every `multibench` table in this file taken before 2026-10-11, when the
alignment landed: this one, sfte's, cava's and the generated payloads'.
Before it, baud-core's `cat-scroll` row moved by as much as 10% with where
the linker put three functions, for no reason in the source; `multibench`
now aligns to 128 bytes. A table taken after the alignment, as the
2026-10-28 captures' table below was, is not comparable to these.

**Taken at `e5bc3ef`, 2026-09-20.** Its commit message states no power
state. The file's Host line at that commit read "on AC power", dated to an earlier
table's commit, so whether it covered this sweep is not stated.

`bench-others/multibench`, `TRANSPORT=memory`, medians of 9, interleaved and
rotated. Every core is driven with the same bytes through the same 4096-byte
reads, and every core, baud included, is configured with **10,000 lines of
scrollback**.

| recording | baud-core | ghostty | alacritty | **vs ghostty** |
|---|---|---|---|---|
| `ascii/less-pager.raw` | **1684.5** | 290.2 | 241.2 | **5.80x** |
| `ascii/cat-scroll.raw` | **1425.1** [1295-1626] | 424.3 | 203.3 | **3.36x** |
| `ascii/git-log.raw` | **1637.9** [1106-1670] | 516.9 | 211.5 | **3.17x** |
| `utf8/less-cjk.raw` | **498.0** | 247.2 | 271.3 | **2.01x** |
| `utf8/cava.raw` | **554.1** | 318.9 | 362.2 | **1.74x** |
| `utf8/cat-cjk.raw` | **459.4** | 385.1 | 221.7 | **1.19x** |

Ahead on all six, **1.19x to 5.80x**. baud-core is ahead of alacritty on all
six too. alacritty is ahead of ghostty on two rows -- cava and less-cjk -- and
behind it on the other four. The sweep's asciiquarium, vim-unicode and btop
rows were taken on captures since replaced, and are re-taken below.

**The 2026-10-28 captures, taken at `e9af53d9`, 2026-10-28, on AC**, with
`less-pager` beside them as the control: the same harness, `TRANSPORT=memory`,
4096-byte reads, medians of 9, interleaved and rotated, two recordings a run.
Not comparable with the table above: baud-core's `less-pager` reads 1450.0
here against 1684.5 there, and ghostty's 292.6 [290.6-294.2] against 290.2,
so the core has moved between the sweeps and the control has not.

| recording | baud-core | ghostty | alacritty | sfte | **vs ghostty** |
|---|---|---|---|---|---|
| `ascii/less-pager.raw` | **1450.0** [1436-1465] | 292.6 | 247.7 | 14.3 | **4.96x** |
| `utf8/btop.raw` | **879.1** [873-891] | 516.4 | 355.5 | 354.1 | **1.70x** |
| `utf8/vim-unicode.raw` | **600.7** [590-607] | 357.4 | 300.2 | 166.1 | **1.68x** |
| `ascii/pipes.raw` | **503.0** [498-509] | 304.6 | 316.5 | 447.6 | **1.65x** |

Ahead of every other core on all four. alacritty is ahead of ghostty on
pipes and behind it on the other three. sfte drew baud-core's screen on
less-pager, btop and pipes and a different one on vim-unicode, which it
draws without clustering; its pipes figure is the fastest of the other cores
on that row, a stream that never scrolls.

`git-log`'s and `cat-scroll`'s wide baud ranges are the kind of spread measured for `cat-scroll`, ~25% within an arm (for `git-log` a 9% spread was measured once, in an `ingest2` sweep); `git-log` was taken twice in this
sweep, 1637.9 and 1600.7.

## sfte, and the row a screen gate is for

**Taken before `multibench` aligned every function to 128 bytes**, like the
table above.

**Taken at `e5bc3ef`, 2026-09-20**, in the sweep of the table above; its power
state is not stated.

`nihiL7331/sfte` -- a 10.2k-line single-file C11 terminal, pinned at
`5d7ac25`, driven through `baud-sfte-difftest`'s shim -- is `multibench`'s
eighth core since 2026-09-20. Same sweep as the table above: `TRANSPORT=
memory`, 4096-byte reads, medians of 9, interleaved and rotated, every core
at 10,000 lines **except sfte, which is compiled at 2,000** and says so in
the header (its depth is a macro in the oracle's shim, where 2,000 is what
makes the differential like-for-like with `Depth::DEFAULT`; the unfairness
runs toward sfte on the scrolling rows). Locale `C.UTF-8`, asserted.

**The row carries its own admissibility verdict, and three of the six rows
kept here fail it.** The sweep's btop, asciiquarium and vim-unicode rows were
taken on captures since replaced; their sfte figures on the new files are in
the 2026-10-28 table above. sfte has no `cells_written`, so after one full rep of the same replay
the harness digests both screens and prints whether they match under the
row. A row whose two arms drew different screens is a figure for a different
program and is not quoted as a ratio.

| recording | baud-core | sfte | sfte, damage marking removed | same screen? |
|---|---|---|---|---|
| `utf8/cava.raw` | 553.7 | **347.6** [344-355] | 364.0 | yes |
| `ascii/git-log.raw` | 1645.4 | **17.2** [17-17] | **110.1** [104-111] | yes |
| `ascii/cat-scroll.raw` | 1423.6 | **11.2** [11-12] | **82.9** [78-84] | yes |
| `ascii/less-pager.raw` | 1706.3 | 14.3 | 115.9 | **no** -- alt-screen re-entry; yes since `3024b2b`, see below |
| `utf8/less-cjk.raw` | 505.1 | 22.5 | -- | **no** -- clustering |
| `utf8/cat-cjk.raw` | 474.1 | 20.9 | -- | **no** -- clustering |

**Two mechanisms, and the split between them is the finding.** On the
like-for-like rows that never scroll, sfte sat at 341-359 in this sweep --
cava's 347.6, and 341.4 and 359.1 on the asciiquarium and btop captures
since replaced -- in ghostty's band, above alacritty, and that is its
per-character path: one 36-byte
cell a character, a libc `wcwidth` call, no run path. On the two
like-for-like rows that scroll every line it reads 11 and 17, **30x
slower than on that btop capture**, and `sample` puts 95% of that in one function:
sfte's grid-scroll routine ends by marking every cell of the whole scrolling
region dirty -- 10,000 byte stores at stride 36, 360 KB, on every newline
that reaches the bottom row. That is a render-side damage model paid on
the ingest path; baud computes row damage at frame time and its ingest
pays nothing for it.

**The fourth column is the ceiling arm that separates the two.**
`SFTE_CEILING=no-dirty-rows` builds sfte with that one call removed --
every cell byte-identical, which the oracle's five tests prove by passing
under the arm, and only the render damage sfte would report is wrong. The
scrolling rows go **6.4x and 7.4x** (8.1x on less-pager); the three that
never scroll are unmoved, which is the control. So of sfte's scroll-heavy
throughput, **85-88% is damage marking** and 80-116 MB/s is its parser and
cell write, against baud's 1400-1700.

**Which corrects the prediction the plan was written with.** It said baud
would lead on ASCII by per-character against per-run write bookkeeping,
amplified by a cell 4.5x as wide, and its second refutation condition was
that the scroll-heavy rows must show a *larger* gap than a non-scrolling
one. They do -- by a factor that no bookkeeping story reaches, because the
dominant term on those rows is not the write at all. The per-character
mechanism is real and is the 341-359 against 528-865 of that sweep; it is second-order
on the rows where the plan expected it to be the story.

**The gate's fourth `no` is a defect found by replay, not by the oracle.**
`less-pager.raw` agrees with baud at every one of its read boundaries and
after any number of *whole* passes; what differs is the harness's 32 MiB
total cutting the 1,114th pass 1,934 bytes in. The recording never leaves
the alternate screen, so every pass after the first re-enters it, and sfte
clears the screen on re-entry where baud and alacritty both treat it as a
no-op. By the end of a whole pass the pager has redrawn what the clear
removed; at the cut it has not, and 155 cells differ. That is pinned twice
in `baud-sfte-difftest` -- as an isolated five-cell control and as the cut
-- and it is the case a single pass structurally cannot see.

**Since `3024b2b` baud clears on re-entry too, and the gate moved on two
rows** of the captures of the time. xterm's `?1049 h` set arm clears whatever buffer is up, and baud
follows it now. The harness's own replay -- `baud-sfte-difftest`'s `cut`
example, 32 MiB in 4096-byte reads, which is multibench's -- was re-run on
2026-09-25 over all nine recordings. less-pager agrees at 0 cells.
**The btop capture of the time differed in 2,800 cells, every one in its
background and none in its character**: its first pass ends inside a coloured rendition, so each
later pass re-enters under a set background, and xterm and baud clear in
the background in force (`ClearCurBackground`) where sfte clears to the
default. cava, asciiquarium, git-log and cat-scroll still agreed, and the
three clustering rows still differed (vim 135 cells, less-cjk 147, cat-cjk
178). The btop capture of 2026-10-28 ends its first pass under the default
rendition and agrees after a second pass at 0 cells
(`a_second_pass_over_every_recording_is_pinned`), as pipes does; multibench
reports the same screen on both.

## cava, and every implementation on it

**Taken before `multibench` aligned every function to 128 bytes**, like the
first table in this file.

**Taken at `1774072`, 2026-08-30**, the commit that first carries these rows.
Its baud-core, ghostty and alacritty figures, cava's among them, are superseded
by the first table in this file, taken at `e5bc3ef`; the vt100, termwiz and
wezterm rows have not been taken since. The file's Host line at that commit read
"on AC power", dated to an earlier table's commit, so whether it covered this
sweep is not stated.

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
| vt100 crate | 402.0 [391.1-415.0] | **1.41x** |
| alacritty_terminal | 366.0 [348.6-381.8] | **1.55x** |
| ghostty (libghostty-vt) | 319.5 [315.5-322.8] | **1.78x** |
| termwiz (wezterm) parse only | 281.5 [278.8-284.1] | **2.02x** |
| wezterm (full terminal) | 135.3 [133.7-136.4] | **4.19x** |

**cava is stable to about 1.5% across four independent pairings** -- 567.4,
568.6, 567.4, 559.7 for the new core -- which is tighter than most rows here
and makes it a good instrument as well as a good payload.

## Against other implementations, on generated payloads

**Taken before `multibench` aligned every function to 128 bytes**, like the
first table in this file.

**First carried at `9c8ee95`, 2026-09-09**, in the README, from which it moved
here on 2026-10-12. `949a87f`'s message says the tables published before it
were measured at `f1a67c6`, a commit that does not resolve, and whether that
covers this table is not stated; no power state is stated. **It predates
grapheme clustering (`657fc91`) and the scrollback ring (`2d14bc3`) in the
core it names, and has not been re-taken.**

Same harness and statistic. The streaming payload models `cat`, logs and build
output, with an SGR every 37 bytes; the TUI payload models a full-screen redraw.
Both are generated by `multibench` itself, so every core gets the same bytes.

| core | streaming | TUI redraw |
|---|---|---|
| **baud-core** | **3206.7** [3172.1-3234.1] | **2340.4** [2269.8-2377.7] |
| ghostty (libghostty-vt) | 640.5 [582.1-681.8] | 1061.8 [1043.0-1078.9] |
| termwiz (wezterm) parse only | 380.5 [371.5-383.4] | 366.7 [360.8-370.2] |
| alacritty_terminal | 218.3 [201.3-227.4] | 273.7 [268.0-276.5] |
| vt100 crate | 168.4 [166.6-170.3] | 218.6 [214.0-219.2] |
| wezterm (full terminal) | 17.5 [17.3-17.7] | 32.5 [32.1-32.7] |

MB/s. **Generated payloads read higher than any real capture**, and their
escape density matches no real program. The recordings above are the guide.

## Five terminals through a pty

**Taken at `46f5e1c`, 2026-09-12, and not re-taken.** No power state was
recorded for this sweep, and this box's power log (`pmset -g log`) begins
on 2026-10-05, so it cannot be recovered. It was not re-run for this file: it
launches five GUI terminals, and on 2026-10-12 the box carried a 15-minute
load of 5.1-6.0 with other work running. Its btop, asciiquarium and vim-unicode rows, and the fixed cost
taken from btop, are figures for the captures those names held on 2026-09-12;
re-taking them on the new files means launching the five terminals' windows
again, which was not done.

`scripts/vt-compare.sh`: every terminal launched fresh at **200x50 in Menlo
13**, running `scripts/vt-child.sh`, which replays the recording through one
`cat`, twenty times over, into the pty, then homes the cursor to 13;37 and
ends the run when `ESC [ 13 ; 37 R` comes back -- the reply proves the bytes
were consumed, not only accepted. Three interleaved rounds, the first
discarded; `cargo xtask vt-table` reduces them and flags a row whose reply is
not the child's. baud ran the cell path (`BAUD_GPU=cells`). Median of the
settled rounds [range].

**Throughput, MiB/s:**

| recording | baud | alacritty 0.17.0 | kitty 0.48.2 | ghostty 1.3.1 | wezterm |
|---|---|---|---|---|---|
| btop | **88** [86-88] | 72 [71-72] | 63 [62-63] | 71 [70-71] | 30 [29-30] |
| cat-scroll | **81** [78-81] | 67 [63-67] | 57 [55-57] | 66 [66-66] | 39 [37-39] |
| cat-cjk | **72** [71-72] | 59 [59-59] | 30 [27-30] | 62 [62-62] | 10 [8-10] |
| asciiquarium | **68** [67-68] | 67 [66-67] | 43 [43-43] | 53 [52-53] | 46 [46-46] |
| vim-unicode | **42** [42-42] | 38 [38-38] | 30 [30-30] | 35 [35-35] | 21 [19-21] |

**CPU seconds of the terminal process for the run**, its own startup
subtracted (a zero-replay run of the same shape put each terminal's startup
at 0.16-0.28 s):

| recording | baud | alacritty | kitty | ghostty | wezterm |
|---|---|---|---|---|---|
| btop | **0.09** [0.09-0.09] | **0.09** [0.09-0.09] | 0.16 [0.16-0.16] | 0.13 [0.13-0.13] | 0.38 [0.36-0.38] |
| cat-scroll | **0.09** [0.09-0.09] | 0.11 [0.09-0.11] | 0.14 [0.14-0.14] | 0.16 [0.15-0.16] | 0.26 [0.25-0.26] |
| cat-cjk | **0.05** [0.03-0.05] | 0.17 [0.15-0.17] | 0.23 [0.21-0.23] | 0.14 [0.13-0.14] | 1.89 [1.56-1.89] |
| asciiquarium | 0.07 [0.07-0.07] | **0.06** [0.05-0.06] | 0.15 [0.15-0.15] | 0.18 [0.13-0.18] | 0.15 [0.14-0.15] |
| vim-unicode | **0.03** [0.02-0.03] | 0.04 [0.04-0.04] | 0.06 [0.06-0.06] | 0.06 [0.05-0.06] | 0.20 [0.18-0.20] |

**The fixed cost and the marginal rate**, from btop replayed 1, 5, 20 and 40
times through three terminals, two rounds, a straight line through the walls,
with `transport` (read and discard, no parser) re-run the same minute. MB/s:

| | fixed cost a run | marginal rate | of the pty's 177 MB/s |
|---|---|---|---|
| **baud** | 46 ms | **157 MB/s** | **89%** |
| alacritty | 52 ms | 123 MB/s | 69% |
| ghostty | 52 ms | 117 MB/s | 66% |
| the pty, discarding | -- | 177 MB/s | -- |

* **baud leads btop by 22%, cat-scroll by 20% and cat-cjk by 16%** over the
  next terminal, every pair disjoint, and ties alacritty on asciiquarium,
  whose ranges overlap. With the fixed cost out its margin over the next
  terminal is 28%, and it is at 89% of what the pty delivers to a reader
  that discards.
* **The CPU column is where the terminals differ.** baud is lowest or tied
  lowest on every row but asciiquarium, where alacritty is 0.01 s ahead. `top`
  resolves 10 ms and the startup subtraction carries ±0.02 s, so a
  difference under 0.03 s is not a result.
* **The vim row is mostly the fixed cost**: about 45 ms a run against 2.8 MB.

**Two caveats of this sweep were true only at `46f5e1c`.** One said "baud has
no scrollback and the others keep one": the ring landed the next day, `2d14bc3`
(2026-09-13), so baud's rows predate scrollback. The other said the cell path drew
no cross-cell ligature: the cell path's ligature windows are measured since, row
7 below. The same caveat says the cell path skipped attached marks, 55 cells a
frame on cat-cjk; that is not re-checked here. wezterm was a nix build of an
unstable branch (a kitty-image PR), ghostty the 1.3.1 app bundle, alacritty and
kitty nix packages, each configured only for geometry and font.

## Three things the comparison is sensitive to

These decide whether a ratio means anything, and each has produced a wrong
answer in this repo before.

**Scrollback must be set on every core.** `ghostty_terminal_new` takes no
scrollback argument and defaults to unlimited lines and 10,000 *bytes*, pruned
at a page granularity ghostty documents as ~400 KB — which retains no history.
alacritty and vt100 take 10,000 lines explicitly, and `baud-core` is handed
10,000 lines by `multibench` since 2026-09-20 (the first table above).
Configured alike, ghostty's scroll-heavy recordings move and the others do not:
`git-log` 853.9 → 526.0, `cat-scroll` 765.7 → 430.9, `cat-cjk` 516.7 → 391.1
(`af45a56`, 2026-08-29).

**Grapheme segmentation is not on in every core.** libghostty-vt's mode 2027
carries no default, so it segments only when an application asks. baud always
segments -- into legacy grapheme clusters since 2026-11-02, while `?2027` is
reset, and into extended ones once a program sets it -- and these rows were
taken when it reported `?2027` as permanently set. The CJK rows therefore
compare unequal work. What segmentation costs `baud-core` was measured when its cluster
machine landed: **-33%** on cat-cjk, -31% on less-cjk, -8.8% on vim and -7.5% on
btop against the same core storing without it.

**Legacy cells on the per-byte path (2026-11-02).** Whether a join takes a
cell of its own is decided where the cluster machine joined, out of line, so a
run of ASCII or CJK never asks. `ingest2`, the change against its parent, one
binary each built from `git archive` at one path: the change, an A/A copy that
`cmp` finds byte-identical to it, and the change with `?2027` set at power-on,
which writes exactly the parent's cells. Cycles elapsed and instructions
retired over 300 reps (`/usr/bin/time -l`), seven rotated rounds with the first
dropped, at load 2.9-3.3; medians and ranges against the parent's median:

| recording | instructions | cycles, change | cycles, A/A copy | instructions, `?2027` set | cycles, `?2027` set | cycles, parent |
|---|---|---|---|---|---|---|
| btop | -0.11% | +1.73% [+0.18, +2.24] | +0.47% [-0.14, +2.39] | -0.10% | +0.44% [-0.56, +2.31] | [-0.54, +0.43] |
| cat | +0.19% | +1.82% [-1.32, +4.45] | -0.79% [-2.06, +3.10] | +0.18% | -0.36% [-1.25, +4.61] | [-2.08, +3.98] |
| vim | +0.24% | +0.13% [+0.01, +1.33] | -0.04% [-1.30, +0.76] | +0.16% | +0.23% [-0.40, +1.21] | [-0.56, +0.90] |
| cat-cjk | +1.69% | +1.31% [+1.12, +1.92] | +1.21% [+1.00, +1.57] | +0.50% | +0.50% [+0.12, +1.14] | [-0.46, +0.11] |
| less-cjk | +1.43% | +1.37% [+0.16, +2.18] | +0.76% [+0.38, +2.31] | +0.39% | -0.06% [-0.59, +0.31] | [-1.08, +0.64] |

btop and cat write the parent's cells (38,949,100 and 254,874,681), and vim
0.30% more (34,240,291 against 34,138,302), the त of each नमस्ते in a cell of
its own; their cycles ranges overlap the parent's. cat-cjk and less-cjk write
0.53% more cells, their Devanagari vowel signs and conjunct consonants taking
cells of their own, and retire 1.69% and 1.43% more instructions; cat-cjk's
cycles range, +1.31% [+1.12, +1.92], lies above the parent's, and less-cjk's
overlaps it.

The arm with `?2027` set writes the parent's cells on every recording, so it
separates the join path from the cells: it still retires 0.50% more
instructions on cat-cjk, 0.39% on less-cjk and 0.18% on cat, the part of the
cost that is not the cells. A ceiling arm, the `?2027`-set change with the
join's call written as before it -- the machine before the codepoint not
kept, no answer returned and compared -- retires the parent's instructions
(+0.01% on cat-cjk and on less-cjk, -0.03% on cat, four rounds after the
first): that part is the call's shape, not the cluster table. A first sweep before the change was rebased,
at load 5.7-6.0, read the same cells on every arm and instructions within
0.07 points of these.

**No single property of a recording predicts its cost.** On the 2026-10-28
captures btop sends an escape every 18.9 bytes and vim one every 17.2, and
baud-core takes in btop 1.46x as fast (879.1 against 600.7 MB/s, the table
above); the difference between them is that vim carries wide characters and
btop none. pipes carries none and is the slowest of the three per byte, on
escape density alone: one every 6.0 bytes. So the cost is wide content *or*
escape density, and neither predicts it by itself.

## Cycles a byte across payloads

**Taken on 2026-08-29 on `baud-core2`, the core this one replaced and since
deleted, so it cannot be re-taken.** One binary across every payload, so every row implies the same
clock, which `the_cycles_per_byte_table_agrees_with_itself` checks. The core
has changed since, so the rates are historical; the table is quoted for its
ordering. Throughput is code times content, and cycles a byte is the unit that
separates the two:

| payload | MB/s | cycles/byte |
|---|---|---|
| `payload::cjk`, synthetic | 2452 | **1.63** |
| `payload::streaming`, synthetic | 1613 | 2.48 |
| btop | 819 | 4.89 |
| vim | 473 | 8.46 |
| less-cjk | 440 | 9.11 |
| **cat-cjk** | **425** | **9.42** |
| **asciiquarium** | 419 | **9.57** |

asciiquarium is pure ASCII -- no emoji, no CJK, no wide characters -- and costs
more a byte than cat-cjk, so cat-cjk is not an emoji outlier: it sits in the
same band as an escape-dense ASCII capture. Scrolling does not predict the
cost either: `payload::streaming` scrolls most often per kilobyte and is the
second fastest, and asciiquarium never scrolls and is the slowest.

## Storage

Written at `7b00e4c`, 2026-08-29; the +0.3 MB figure first appears at
`d8262f5`, 2026-08-26.

~24 MB *virtual*. Measured RSS after allocating it, initialising a 50x200 grid
and feeding data is **+0.3 MB**: untouched pages are never faulted in.
`baud-core/tests/fixed.rs` asserts the sizes it is made of; the RSS figure is
not re-measured.

The figure was taken against `baud-core2::Storage::boxed()`, which owned its
arrays. `baud-core`'s storage is **caller-provided** -- it borrows every buffer
and allocates nothing -- so the equivalent is `Owned::new(&ws)`, which provisions
exactly the geometry it is given and declines the 2 MiB cell table by default.
The untouched-pages result is a property of the allocator and the grid, not of
which crate holds them, so it should carry over, but on `baud-core` it is
argued, not re-measured; the sizes `fixed.rs` asserts are `baud-core`'s own.

## Write amplification

Cells written per byte ingested, from `Term::amplification`, which returns the
bytes in and the cells written; `ingest2` prints their ratio. First carried at
`e9edd30` (2026-09-10), and **re-read at `ac96f39` on 2026-10-12** -- a count,
not a rate, so the machine's load does not enter it: `ascii/cat-scroll.raw`
wrote 16,932,361 cells for 8,969,200 bytes at 20 reps and 169,895,281 for
89,692,000 at 200, 1.9x both times; `utf8/cat-cjk.raw` 7,549,906 for 6,547,020
at 20 reps and 76,336,186 for 65,470,200 at 200, 1.2x both times. At one rep the
two read 1.8x and 0.9x, so the figure is a replayed one, and the rep count is
part of it.

| payload | `baud-core` |
|---|---|
| `ascii/cat-scroll.raw` | 1.9x |
| `utf8/cat-cjk.raw` | 1.2x |

A cell is eight bytes, so a materialising path cannot go below 8x per *byte of
text*, and the figures above are per byte of *input*, which includes escapes.

**`RowExtent` bounds blanking at the row's used extent**, so cat-scroll reads 1.9x without
deferring a write. `ingest2` prints the figure per run.

## The ASCII ladder and the store ceiling

**Taken at `1774072e`, 2026-08-30, on the core of that day; the store-traffic
column was corrected at `c05133fb`, 2026-09-11.** A terminal is not a copier: it turns
one byte into eight, and every rate here is per *input* byte. Four synthetic
payloads, 16 MB each, each rung adding exactly one thing to the one below, so
the step that costs is named rather than inferred. They are not recordings and
are never quoted against another implementation.

| rung | what it adds | MB/s | cells a byte | store traffic |
|---|---|---|---|---|
| `wrap` | printable ASCII, `CR` at the margin, never scrolls | **5,850** | 1.0 | 49.1 GB/s |
| `flat` | the same, wrapping off the bottom, so it scrolls | 4,400 | 2.0 | 73.8 GB/s |
| `lines` | `CRLF` every 80 bytes | 3,260 | 1.9 | 52.0 GB/s |
| `sgr` | one `ESC [ 3 2 m` a line | 2,200 | 1.8 | 33.2 GB/s |

**The units are mixed, and the traffic column is arithmetic.** `ingest2`
divides by `1 << 20`, so its MB/s are mebibytes, while the traffic is decimal:
`traffic = rate x 2^20 x 8 x cells a byte / 1e9`, which
`the_ladder_arithmetic_holds` checks. The column first read 46 / 69 / 48 / 31,
4.86% low in every row, because it multiplied mebibytes as decimal.

**The ceiling is the store port.** `baud-bench/benches/ceiling.rs` stores eight
cells at a time into an 80 KB grid at 143.9 GB/s, which at one cell a byte is
17,156 MB/s of input; everything a materialising terminal can do sits under it.
Pure text is store-bound and near it: `wrap`'s stores are 47% of its clock and
run at about 104 GB/s while they run, 73% of the peak. Scrolling costs cells,
not time: 1.0 to 1.9 a byte at the same traffic. One escape sequence per eighty
bytes costs 29% and takes the loop off the store ceiling, about 44 cycles for a
five-byte SGR, thirteen times a text byte.

## Reproducing

The cross-implementation and in-repository benchmarks, with what each is
for:

```sh
# cross-implementation, real capture
cd bench-others/multibench
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory \
  RECORDINGS="utf8/btop.raw,utf8/cat-cjk.raw" cargo run --release

# cross-implementation, generated payloads
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory cargo run --release

# the sfte row with its damage marking removed -- the ceiling column above.
# The arm is applied by baud-sfte-difftest's build.rs only when named here,
# and the oracle's own tests pass under it (cells byte-identical):
SFTE_CEILING=no-dirty-rows GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory \
  RECORDINGS="ascii/git-log.raw,ascii/cat-scroll.raw" cargo run --release

# baud alone, one payload, no fork and no PTY -- the instrument the memory
# columns here are measured with. A recording, a ladder rung
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

Per-rep figures and rates taken before `481613f` are not comparable with those
after it: ingest2's setup moved its timed loop (btop +5.95%
in instructions, +3.0% in cycles).

## Keystroke to pixel

Taken at `28f2382`, 2026-09-18. Apple M5 Max, macOS 27.0, on AC, the built-in 120 Hz panel, the app's default
cell path at 120x40 under a user config. `scripts/key-latency.py --rounds 5
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
8.33 ms period.

## The shader cache at launch

Taken at `a90b11d`, 2026-09-18, on the source-compiled library and the Xcode
before the one the `.metallib` section below was measured on; that section
re-took the source arm the same afternoon as its change and says this
section's figure came from an older Xcode. Since `bdc99eb` the app ships the
compiled library, so the rows below describe a launch path that no longer
ships. Apple M5 Max, macOS 27.0, on AC. `scripts/shader-cache-bench.py 7`: the same
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
cache keys on the source and survives rebuilds.

## The Dock icon from the bundle

Taken at `6bb94e9`, 2026-09-18. Apple M5 Max, macOS 27.0, on AC. `BAUD_CMD=shell BAUD_BENCH_STAGE="first
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

Taken at `1eb2e5d`, 2026-09-18. Apple M5 Max, macOS 27.0, on AC. `BAUD_CMD=shell BAUD_BENCH_STAGE="first
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

## The shell's fork behind the machine's name

Taken 2026-10-30. Apple M5 Max, macOS 27.0, on AC. Probes in fresh
processes, no window: CoreGraphics' display-scale call, then `forkpty`
about 1.8 ms after it, with a thread reading `NSProcessInfo.hostName` and
one making the Metal device started before or after the fork. Eight
launches an arm, arms interleaved, milliseconds.

| arm | `forkpty` | the device's own time |
|---|---|---|
| both threads before the fork (the base) | 1.22-24.52 | 10.6-48.7 |
| the base, an earlier batch of eight | 2.09-71.19 | 10.2-88.5 |
| the names after, the device before | 1.60-5.44 | 8.6-41.5 |
| both after (this change) | 1.31-4.54 | 7.1-30.9 |

In the both-after arm the probe waits for its child before starting the
device, so the device starts about 7 ms after the scale; baud does not
wait.

The wait is `_mdns_atfork_prepare()`, 2.66-32.92 ms timed alone; the
kernel's `__fork()` beside the same thread never took over 0.37 ms. The
`BAUD_STARTUP=1` launch log before the change read `renderer` at 14.9 ms
and `pty` at 77.0. `docs/research/sources/aj-observed/fork-wait.txt` has
the rest.

## Overlapping sharedApplication with the CoreGraphics connection (row 4, reverted)

Taken at `a6b01a2`, 2026-09-18. Apple M5 Max, macOS 27.0, on AC. `scripts/startup-bench.py --rounds 7 --arm
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

Taken at `f06d047`, 2026-09-18. Apple M5 Max, macOS 27.0, on AC. `scripts/startup-bench.py --rounds 9 --arm
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

Taken at `c5078da`, 2026-09-18. Apple M5 Max, macOS 27.0, on AC, the built-in 120 Hz panel, the default
cell path at 120x40. `scripts/key-latency.py --rounds 5 --keys 30 --gap 150
--child stream --arm base=<HEAD> --arm new=<row 6> --aa`: the child prints a
line every 2 ms under the tty's echo, so a key's echo always lands inside
the frame loop's frame's-time gate. Five rounds, the first discarded, the
arm order rotated, an A/A copy in the sweep; medians of ~120 keys an arm,
microseconds; the floor is the A/A copy's per leg. This is the second of
four sweeps, the quiet one whose posts were phase-locked: a keystroke under
output waited for the tick, and drawn at once it is 2.8 ms sooner on the
layer, the panel unproven; the uniform-phase sweep read the same `build` and lost `to_shown` to a
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

**Re-taken on the 2026-10-28 captures, at `e9af53d9`, 2026-10-28.** Apple M5
Max, macOS 27.0, on AC. `FRAME_ARMS=async cargo bench -p baud-bench
--bench frame_gpu -- <recording> 2 incremental`, one binary per arm from one
source -- the base, `render-no-window-cache` (every window shaped) and
`render-no-windows` (no window walk) -- plus a byte-identical copy of the
base as the A/A control, four launches a binary in separate processes with
the order rotated, each launch's in-process warm-up round discarded (one
short of the rule's five); 200x50 at 14 px.
Convert in microseconds a frame, medians of four:

| recording, face | base | A/A | every window a miss | no windows | windows a frame | misses a frame | a miss, us |
|---|---|---|---|---|---|---|---|
| btop, JetBrains Mono | 40.8 [38.8-41.5] | 40.2 | 53.0 | 38.3 | 9.2 | 0.5 | 1.4 |
| cat-scroll, JetBrains Mono | 91.0 [90.5-92.2] | 90.2 | 170.2 | 26.1 | 60.6 | 17.8 | 1.8 |
| git-log, JetBrains Mono | 162.6 [161.7-167.0] | 164.0 | 191.9 | 31.0 | 26.7 | 14.9 | 2.5 |
| less-pager, JetBrains Mono | 134.2 [130.2-138.0] | 132.7 | 201.0 | 55.3 | 55.8 | 22.9 | 2.0 |
| pipes, JetBrains Mono | 2119.6 [2110.2-2146.8] | 2139.6 | 6016.8 | 35.7 | 336.0 | 42.9 | 13.3 |
| cat-scroll, FiraCode | 219.6 [218.6-220.2] | 219.6 | 274.0 | 27.7 | 54.3 | 37.4 | 3.2 |
| git-log, FiraCode | 287.5 [287.1-287.9] | 287.6 | 350.4 | 32.2 | 48.6 | 36.7 | 5.3 |

A miss is the extra convert of the every-miss arm over the base, divided by
the windows it added to the misses; that arm still writes the cache, so a
miss here is a shape plus a replace less the get the base pays. The
asciiquarium row of the 2026-09-18 table went with its recording and cannot
be re-taken; pipes stands in its place, an SGR every 9 bytes, and its
convert is almost all the window walk: 336 windows a frame,
2.12 ms against 0.04 with no windows. No change: the frame's keep floor is
one 120 Hz frame, 8.33 ms, the largest base convert in the table is 2.12 ms
and the largest of any arm 6.02.

## The glyph map's first-frame misses (row 8, measured, no change)

**Re-taken on the 2026-10-28 captures, at `e9af53d9`, 2026-10-28.** Apple M5
Max, macOS 27.0, on AC. `FRAME_FACES=<the app's default CJK stack: Menlo,
Apple Color Emoji, Apple Symbols, Hiragino Sans GB, Apple SD Gothic Neo>
FRAME_FIRST=<n> FRAME_ARMS=async cargo bench -p baud-bench --bench frame_gpu
-- <recording> 3 incremental`, 200x50 at 14 px; the first n frames of a
replay reported apart from the rest, three rounds a launch with the first
discarded, three launches, the spans across the six settled rounds. Convert
in microseconds a frame, and map misses:

| recording | n | first n: convert, misses a frame | the rest: convert, misses a frame |
|---|---|---|---|
| cat-cjk | 1 | 2874-3063, 663 | 125-129, 32 |
| cat-cjk | 3 | 1621-1714, 454 | 103-111, 24 |
| cat-cjk | 10 | 841-889, 255 | 61.5-62.7, 9.2 |
| cat-cjk | 30 | 371-393, 106 | 32.3-34.3, 0.18 |
| less-cjk | 10 | 687-717, 201 | 45.3-48.1, 6.0 |
| vim-unicode | 10 | 112-114, 9.2 | 23.0-23.6, 0 |
| btop | 10 | 46-50, 8.0 | 27.5-29.7, 0.11 |

A miss is 4.1-4.4 us (the first frame's convert less the walk, over its
misses). No change shipped: the first frame is 3 ms against a frame floor of
8, and warming the map at load moves the same shaping and rasterising to
launch rather than removing it.

## A key's echo drawn before the pty answers (row 9)

Taken at `75a969f`, 2026-09-18. Apple M5 Max, macOS 27.0, on AC, the built-in 120 Hz panel, the default
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

**Re-taken on the 2026-10-28 gestures, at `e9af53d9`, on AC.** `cargo bench -p
baud-bench --bench winch -- ../tests/resize/<fixture> cost 20`: each fixture's
recorded steps -- a size and the bytes that arrived at it -- replayed through
the core twenty times, the first round discarded, the resize and the feed timed
apart and the median taken. A **fresh storage per round**, because `Term::new`
resets the cursor and not the grid, so a round on a reused one starts on the
last round's screen. The parser-cell column is one value across all nineteen
settled rounds on every fixture, which is the check. Three runs of each
fixture, the seven in turn on an otherwise idle box, after one pass over all
seven discarded, the spans across those runs. No window and no GPU.

| gesture | steps | bytes | width changes | a step: resize + feed | gesture | parser cells | lines held |
|---|---|---|---|---|---|---|---|
| live-drag | 519 | 231,204 | 438 | 3.99-4.09 + 0.63-0.64 us | 2.40-2.46 ms | 284,744 | 0 |
| drag | 331 | 159,824 | 156 | 2.05-2.07 + 0.51-0.51 us | 0.85-0.85 ms | 206,026 | 0 |
| fresh-drag | 331 | 156,248 | 156 | 1.56-1.61 + 0.56-0.57 us | 0.70-0.72 ms | 196,867 | 0 |
| zoom | 26 | 13,280 | 23 | 9.45-9.66 + 0.53-0.54 us | 0.26-0.27 ms | 14,828 | 24 |
| fresh-zoom | 26 | 9,704 | 23 | 9.24-9.53 + 0.51-0.52 us | 0.25-0.26 ms | 10,499 | 0 |
| fresh-burst | 43 | 15,463 | 30 | 4.69-4.78 + 0.52-0.53 us | 0.22-0.23 ms | 16,023 | 0 |
| short | 26 | 12,730 | 23 | 8.13-9.22 + 0.54-0.54 us | 0.23-0.25 ms | 13,698 | 29 |

**The resize column is the core's work now, not the clock.** A null arm -- an
empty timed region per step -- reads 0.00-0.01 us beside it. The table this
replaces, taken at `2e3da21` on 2026-09-18 on the gestures since re-recorded,
read 0.01-0.02 us a resize; the passes past a narrower edge (8323e8a) added a
measure at every narrowing, and those two tables differ in both the code and
the recordings, so no ratio between them is a result.

The control, in the same run, looped the way the gesture is and on the
screen the gesture left: one width change costs **2.25-2.75 us** in
`Zone::Prompt`, where the core leaves the shell's rows for the shell to
redraw, and **4.6-13.9 us** in `Zone::Output`, where it re-cuts -- the
span of the seven fixtures' medians over the three runs, each a median of 19
settled rounds. **0 of every fixture's width changes are in
`Zone::Output`**, counted at the call.

No change follows. A step is 2.1-10.2 us against a frame's 8.33 ms, three
orders under it, and the whole of live-drag is under a third of one frame.
Frames are per resize step and per display period, not per redraw.

## Idle CPU (row 11, measured, no change)

Taken at `e8fe895`, 2026-09-18. Apple M5 Max, macOS 27.0, on AC, nothing else
running.
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

No change shipped. `present` already returns on `Pending::Clean` before the
glyph-map walk, the style resolve, the overlay walk and the draw. The style
resolve that sentence names was the whole rendition table each frame then, a
fixed 1,024 slots before `d0fe8e0`; it is a cache since `60cce43` (below, "The
rendition cache's frame cost"), so the frame a non-idle tick builds is not the
one this row skipped.

## Smooth scroll idle (measured, no change)

Taken at `fdda380`, 2026-09-21. Apple M5 Max, macOS 27.0, on AC. scripts/idle-cpu.py on the release
`baud`, interleaving `[scroll] mode = smooth`, `= normal` and an A/A copy of
smooth in one sweep through pid-preserving wrappers, first round discarded.
The child writes nothing.

| arm | idle CPU | range | a tick at 120 Hz |
|---|---|---|---|
| `[scroll] mode = smooth` | 0.878% of one core | 0.845-0.894 | 73.2 us |
| `[scroll] mode = normal` | 0.865% | 0.831-0.896 | 72.1 us |
| a byte-identical copy of smooth | 0.877% | 0.842-0.900 | 73.0 us |

**Smooth adds nothing to idle.** normal against smooth is -0.013% (-1.5%),
inside the A/A copy's own 0.058% spread, and every range overlaps -- not a
result, which is the by-construction claim: an idle screen retires no line,
so `Slide.retire` is never called, so the tick's `slide.active()` gate is
false and no frame is forced. The one smooth-vs-normal difference at idle is
a `remaining > 0.0` bool per tick.

**The absolute is the branch's, not row 11's.** ~0.87% against row 11's
0.464% master baseline is per-tick work accumulated on the sfte-oracle
branch (the focus `isKeyWindow` poll and the like), unrelated to this
feature -- the `normal` arm carries the smooth code fully inert and reads
the same. Not chased here; the branch is not merged.

**The per-frame cost of a slide is not in `frame_gpu`, which drives
`draw_cells` and never runs `present`.** A slide turns one discrete jump
frame into at most seven cell-path frames over 60 ms (the cap holds it to
<= one row), each the cell-path convert `frame_gpu`'s async arm measures
(14-19 us on ASCII, more on CJK) plus one full-view map walk for the
leaving-row `ensure` and one row's copy, with `(rows+1)/rows` more instances
drawn. There is no accumulating backlog to price -- that is what the cap
removes.

## Smooth scroll's per-frame cost (measured, under the floor)

**Re-taken, the 2026-10-28 btop capture among them, at `e9af53d9`,
2026-10-28.** The table this replaces was taken at `fdda380` on 2026-09-21,
on the btop of the time and before the rendition table was sized to the
screen (`d0fe8e0`) and its resolve became a cache (`60cce43`), so both its
columns carried a resolve of the whole table that this one's do not; no
ratio between the two is a result.
Apple M5 Max, macOS 27.0, on AC. `frame_gpu`'s `slide` arm is the `async`
cell frame plus the extra `map.ensure` the app's `present` slide branch does --
it walks the map over the view one line up (the leaving row's glyphs) as well as
over the screen, where a steady frame walks only the screen. So `slide - async`
is the per-frame CPU cost a slide adds; `FRAME_ARMS=async,slide`, four rounds
interleaved, first discarded, three launches, JetBrains Mono at 14 px, 200x50,
convert in us a frame, the spans across the nine settled rounds; the delta is
each round's `slide` less the same round's `async`:

| recording | `async` (steady) | `slide` | delta | map misses |
|---|---|---|---|---|
| btop | 38.5-40.6 | 40.6-42.3 | **+1.3-2.5** | 91 both |
| cat-scroll | 88.7-90.6 | 92.4-94.7 | **+2.7-5.0** | 101 both |
| cat-cjk | 65.7-68.4 | 67.8-69.8 | **+1.4-3.1** | 3197 both |

On btop the two spans touch at 40.6; the delta is paired, and in each of the
nine settled rounds `slide` reads above that round's `async`.

**The delta is a redundant walk, and the misses say so: identical on both
arms.** The extra ensure rasterises nothing new -- the leaving row's glyphs
are already in the map from when the row was on screen -- so it is pure
walk over the screen's cells, and a single-row ensure of just the leaving
row would recover nearly all of it.

**And it is far under the frame floor, so it is not a cost problem.** A
slide is at most seven cell-path frames over `SLIDE_SECS_PER_ROW = 60 ms`
(the cap holds it to one row), so a discrete scroll costs at most
`7 x 5 us = 35 us` of extra convert spread over 60 ms -- at most 5 us in any
one frame, **0.06% of a 120 Hz frame**, against a keep rule of the larger of
5% (417 us) and one
frame (8333 us). This is note N's rows 7 and 8 again: a real redundancy
with no floor to spend the fix against. The leaving-row copy (one row) and
the `(rows+1)/rows` extra instances the GPU draws are bounded-negligible by
construction and are not modelled by the arm.

**So the levers for improving smooth scroll are fidelity, coverage and
smoothness, not cost** -- marks and ligatures do not slide (the overlay
records hold their un-slid screen position for the ~60 ms), the instance
and CPU paths jump (the slide is cell-path only), and the pan advances
linearly in whole pixels. Those are what a change would move; the
per-frame CPU cost is already invisible to the budget.

## The shipped `.metallib` (row 1, kept)

Taken at `bdc99eb`, 2026-09-19. Apple M5 Max, macOS 27.0, Xcode 27.0, on AC.
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

**A cold launch is 256 ms faster.** The warm arms overlap, so nothing is claimed
there; every other pair is disjoint. The cold pipelines are **28.5 ms dearer**
from a shipped library than from a source-built one, reproduced in every settled
launch. Timed one pipeline at a time since (`Build::each`), all of it is the
*first* pipeline build of the process, its function lookups and its pipeline
state together. It follows the position rather than the shader: built last, `bg`
costs what the others do, and the one built first pays instead. Why
`newLibraryWithData` defers that work to the first pipeline is still not known.
With the shaders compiled at build time, a cold launch costs 34 ms instead of
287.

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

The library was 48,900 bytes at `bdc99eb`, generated by
`scripts/build-metallib.sh` from `baud-present-macos/src/shaders.metal` with
`Apple metal version 32023.921`, and committed. **At `ac96f39` it is 133,780
bytes**, last rebuilt at `6a2eff5` (2026-10-01), carrying every shader added
since -- the trails among them -- and the cold-launch rows above have not
been re-taken on it. Every launch reports
which library it used -- `library 0.1 (Library)` -- because a cold arm
that quietly compiled the source would measure the old program.

## The rendition cache's frame cost

**Taken at `60cce43`, 2026-09-25, on mains**, HEAD and the change built at
one path, an A/A copy of the change in every sweep. Not re-taken since: the
cell path has changed under it (it was measured later at
other configurations, not this one), and this file's load reading on 2026-10-12
was not a quiet box.

Before it, every cell-path frame resolved the whole rendition table, 65,536
slots at the app's provisioning (`MAX_GRID`, 400x150). `frame_gpu`, the cell
path as the app runs it, `FRAME_GRID=400x150`, frames with nothing read
(`FRAME_IDLE`) and frames after a read, four launches rotated, each launch's
first round dropped; us a frame of baud's CPU:

| recording | frame | HEAD: resolve | HEAD: frame | cache: styles | cache: frame | A/A |
|---|---|---|---|---|---|---|
| btop | nothing read | 191.6 | 222.9 [221.4-225.9] | 0.0 | **28.8** [28.3-29.9] | 29.1 |
| btop | after a read | 190.9 | 226.5 [225.1-229.5] | 0.0 | **32.4** [31.4-33.6] | 32.9 |
| vim | nothing read | 189.5 | 212.8 [209.8-216.8] | 0.0 | **20.5** [20.1-21.3] | 20.6 |
| vim | after a read | 189.1 | 221.9 [216.1-231.0] | 0.0 | **29.5** [27.6-31.4] | 28.8 |
| cat-scroll | nothing read | 189.3 | 216.5 [215.5-220.6] | 0.0 | **24.2** [23.9-24.9] | 24.4 |
| cat-scroll | after a read | 189.1 | 296.2 [294.3-300.7] | 0.0 | **103.2** [102.0-112.1] | 103.7 |

What is left of the idle frame is the glyph-map walk (11-17 us), the overlay
walk (8-11) and the commit (2); after a read on `cat-scroll` it is the
overlay's ligature windows, 84 us. The instrument's first reading, a
different sweep, put the resolve at 159 us of a 190 us idle frame, 84%.

**The CPU path**, `frame`, six rounds rotated, first dropped, convert in us a
frame, every pair disjoint with the copy on the change: btop 155.5
[155.3-158.3] to **141.6** [140.3-142.3], vim 250.8 to **229.8**, cat-scroll
766.9 to **737.7**, cat-cjk 1095.5 to **1066.0**, git-log 1002.3 to
**980.1**.

**What the app spends**, scripts/idle-cpu.py over a user config with a
child moving the cursor every 200 ms, the window unfocused, five rounds of
ten seconds, the first dropped; percent of one core, frames a second and the
app's own `convert` from its report. The trail on by default at `60cce43`
was the one-cell ghost (the quad, at `c2e20be` the same evening, measured
against it as "ghost, HEAD"):

| arm | CPU | frames a second | the app's `convert` |
|---|---|---|---|
| trail, HEAD | 7.08% [6.28-7.22] | 59.6 | 476 us |
| trail, the cache | **3.12%** [2.55-3.22] | 60.5 | **29 us** |
| trail, A/A copy | 3.15% [2.99-3.27] | 60.6 | 30 us |
| no trail, HEAD | 1.33% [1.30-1.34] | 4.8 | 575 us |
| no trail, the cache | 0.97% [0.67-0.99] | 4.8 | 38 us |

**A trail frame cost about 1.05 ms of CPU on HEAD and 0.39 ms with the
cache** -- (7.08 - 1.33)% over 54.8 more frames a second, against
(3.12 - 0.97)% over 55.7. The app's `convert` read 476 us where the bench's
same frame reads 223; why the app's frame thread is slower is not measured.
The focused-caret arm could not be taken: a window raised by script is
frontmost and not focused, so nothing blinked.

**Where the rest of a frame goes**, at `fed5f8d`, 2026-09-25, on mains: Time
Profiler attached to the app on the cache's trail arm above, ten seconds,
270 ms on a CPU over about 596 frames.

| where | on a CPU, 10 s | a frame |
|---|---|---|
| **Metal's submission**: `IOGPUCommandQueueSubmitCommandBuffers`, on Metal's own dispatch thread | **131 ms** | **0.22 ms** |
| `Gpu::draw_cells`: baud's cell-path build and the driver's encoding | 65 ms | 0.11 ms |
| Core Animation's commit | 26 ms | 0.04 ms |
| the run loop, thread starts and the rest | 48 ms | 0.08 ms |

`scripts/metal-submit-cost.swift`, one command buffer at a time as baud
submits, six rounds rotated, the first dropped; us of CPU a frame:

| arm | back to back | one frame every 16 ms | every 8 ms |
|---|---|---|---|
| no Metal, the pacing alone | -- | 13.1 [12.7-14.5] | 11.4 [11.2-12.3] |
| a cleared pass, nothing bound | 60.6 [60.0-62.3] | **243.3** [235.2-277.3] | 237.8 [215.5-242.0] |
| the cell path's resources bound | 62.0 [61.3-63.4] | 248.2 [245.5-291.7] | 241.6 [212.6-254.9] |
| the same, hazard tracking off | 61.5 [61.1-63.7] | 261.7 [253.0-267.2] | 223.6 [216.9-246.1] |
| the same, in a residency set | 63.6 [61.3-64.6] | 255.0 [249.5-272.8] | 234.9 [225.6-269.4] |
| a command buffer holding no references | 62.6 [60.1-63.6] | 246.7 [240.4-263.8] | 235.1 [221.2-245.9] |

Every arm with Metal overlaps the empty pass. **An animation's CPU is its
frame count times about a quarter of a millisecond of submission**, ~1.3% of
a core at 60 frames a second, and no change to baud's ~30 us build can take
more than a fifteenth of a frame. The 0.45 ms a whole frame costs here (270
ms over 596) and the 0.39 ms the trail arm above reads are two sweeps' and
are not reconciled.

## A Devanagari word drawn whole (2026-11-02)

**The run cache's value.** A word is shaped as one run at its face's natural
advances, its clusters laid end to end, and cached as a grid run is, under a
key no grid run's fold starts with. A cached run now says where its offsets
are from -- each cell's origin, or the run's start -- and, laid as a run,
its advance, which the word's next piece goes on from: a byte and two more.
`size_of::<Shaped>()` goes from 770 to 774 bytes; an entry, the 8-byte key
beside it at 8-byte alignment, stays 784, so the cache's 2,048 entries stay
1.57 MB and its working set does not change. A word's syllables were shaped
on every frame before, outside the cache; they are cache hits now.

**Frame cost.** `frame`, whole-process instructions retired (`/usr/bin/time
-l`), JetBrains Mono with Kohinoor Devanagari behind it, five reps a run,
five rotated rounds with the first dropped; the change against its parent,
with an A/A copy `cmp` finds identical to the change:

| recording | change | A/A copy | parent |
|---|---|---|---|
| btop | +2.39% [+2.37, +2.42] | +2.38% [+2.37, +2.38] | [-0.02, +0.02] |
| cat-cjk | +1.99% [+1.98, +2.01] | +2.00% [+1.98, +2.02] | [-0.00, +0.01] |
| vim-unicode | +1.72% [+1.68, +1.74] | +1.73% [+1.68, +1.81] | [-0.00, +0.01] |

The four rounds kept agree within 0.1 points of each arm's median. btop
draws what its parent draws -- the same frames, rows, instances, cache
hits and glyphs rasterised -- and holds no Devanagari: its 2.39% is paid
by rows that hold none: the syllable walk over each cell, and the word
and syllable tests in the run loop (below, "Rows with nothing to
re-form"). cat-cjk and vim-unicode draw other work, their
Devanagari in more columns: cat-cjk rasterises 39 glyphs where it
rasterised 33, and the cells of runs a prefiltered path would shape a
frame are 396.3 where they were 426.2, vim-unicode's 950.0 where they
were 962.3.

**A glyph's pen to a quarter pixel.** Two hundred words of `devanagari.txt`
at 80 columns in Noto Sans Devanagari, headline columns left open inside
each word's ink, and the atlas's entries for them:

| pen | open, 24 px | open, 28 px | entries |
|---|---|---|---|
| whole pixels | 156 of 6,951 | 194 of 8,236 | 148 / 144 |
| a quarter pixel | 135 | 182 | 279 / 279 |
| 1/256 px | 127 | 183 | 664 / 667 |

**Narrowed, and left over.** Every word of the corpus at 80 columns, 24 and
28 px together. With Noto Sans Devanagari's variable file and its 15
narrower instances, 24 of 830 words are narrowed under the default count
and 2 overlap a neighbour; with spacing marks joined 248 of 818 narrowed
and 2 over; under `?2027` 389 of 814 narrowed and 17 over. No glyph is
scaled on any of them. With a face that has no `wdth` axis the over words
are 5 and 18 of 830 under the default count (Noto's static file, Kohinoor
Devanagari), 95 and 131 of 818 joined, and 228 and 273 of 814 under `?2027`.
The built-in font, which carries Noto's `wdth` axis as a hidden axis, on
the stack a launch makes with no config, gives the variable file's counts,
each placement at each size the same, every word drawn from its own
Devanagari glyphs; the same stack without its narrower instances gives the
static file's, 5, 95 and 228 over.

## The narrower instances at launch (2026-11-03)

**Read from their base's tables, the 60 narrower instances cost a launch
0.25 million instructions and 0.17 MB, no stage's time apart from the
parent's but `Fonts::new` itself, about 0.05 ms against 0.02, which the
table's first row counts with the faces' mapping and parsing; built per
face, they cost it 4 ms and 7 MB.** Taken 2026-11-03, Apple M5 Max,
macOS 27.0, on AC. A release build of the app's unit tests times a
launch's own calls in a fresh process: the default config with no file,
mapping and parsing the faces and `Fonts::new` over them, `Renderer::new`
at 28 px and 80x24, a shell prompt's first frame on the CPU path, a zoom
to 30 px, a font reload, the restack that a character no face draws
brings (Arabic, a system face appended), each with its frame, and the
prompt drawn 51 times more. A frame drawn again is the render alone;
every other frame is the probe's whole frame: the terminal it builds and
feeds, the render, and an FNV digest of every pixel. Four arms at one
path -- the parent, the change with the instances' tables built per
face, the change with them shared, and the shared build making no
instance -- and a copy of the shared arm's binary, `cmp`-identical, as
its A/A; six rounds of five processes, rotated, the first dropped.
Milliseconds, median [range], and millions of instructions the process
retired:

| stage | parent, 12 faces | per face, 72 | shared, 72 | no instances |
|---|---|---|---|---|
| mapping, parsing and `Fonts::new` | 0.149 [0.134-0.169], 1.62 | 0.210 [0.193-0.233], 2.76 | 0.177 [0.162-0.193], 1.83 | 0.147, 1.62 |
| `Renderer::new` | 0.632 [0.599-0.692], 11.17 | 3.306 [3.221-3.474], 77.36 | 0.649 [0.614-0.682], 11.49 | 0.639, 11.48 |
| a probe's first frame, its terminal and a pixel digest included | 2.176 [2.118-2.288], 24.81 | 3.536 [3.418-3.641], 51.17 | 2.176 [2.114-2.330], 24.63 | 2.145, 24.59 |
| zoom | 0.001 [0.001-0.002] | 0.001 [0.001-0.002] | 0.001 [0.001-0.003] | 0.001 |
| its frame | 1.882 [1.834-1.986], 19.74 | 1.960 [1.874-2.024], 19.86 | 1.915 [1.846-2.042], 19.83 | 1.896, 19.84 |
| font reload | 0.489 [0.465-0.536], 10.24 | 3.032 [2.945-3.153], 76.10 | 0.534 [0.500-0.620], 10.72 | 0.503, 10.53 |
| its frame | 1.909 [1.859-2.019], 23.04 | 3.249 [3.184-3.354], 49.35 | 1.927 [1.880-2.089], 22.88 | 1.909, 22.89 |
| fallback restack | 8.429 [8.066-8.848], 87.40 | 10.922 [10.737-11.382], 150.77 | 8.588 [8.316-11.123], 87.95 | 8.449, 87.71 |
| its frame | 2.056 [2.038-2.199], 24.86 | 4.458 [4.385-4.547], 74.96 | 2.031 [2.010-2.231], 24.11 | 2.026, 24.06 |
| a frame again | 0.202 [0.197-0.207], 6.64 | 0.207 [0.199-0.210], 6.74 | 0.206 [0.199-0.212], 6.73 | 0.204, 6.73 |
| footprint after launch, MB | 9.26 [9.16-9.32] | 16.35 [16.30-16.40] | 9.41 [9.22-9.50] | 9.24 |

The A/A copy is within 0.03 ms of the shared arm's median on every stage
but the restack, 0.07, and the corpus frame, 0.05.

A second run, the shared arm and the parent built again at one path with
a `cmp`-identical A/A copy, five rounds of five processes, rotated, the
first dropped, times `Fonts::new` alone and each of the first frame's
three parts: `Fonts::new` takes 0.047 ms [0.041-0.061] and 0.57 million
instructions against the parent's 0.017 [0.015-0.025] and 0.31, the A/A
copy 0.049; the frame's terminal, render and digest take 0.137
[0.123-0.155], 0.948 [0.899-0.975] and 1.061 [1.050-1.131] ms against
the parent's 0.132 [0.122-0.152], 0.935 [0.887-0.971] and 1.052
[1.050-1.090], the digest 6.05 million instructions in both.

Per face, each instance's lookup digests, pair tables, extents, reaches
and per-script selections were built again, its `cmap` proposed every
block the face table decides, and the colour search of each character a
block decides walked the 60 instances. Shared, an instance reads its
base's layout tables, built from `GSUB`, `GPOS`, `GDEF` and `morx`
alone; the face table and the fallback chain leave the instances out;
and each instance is a copy of its face's parse given its width. What
the 60 cost now is the shared arm less the no-instances arm:
0.21 million instructions of mapping, parsing and `Fonts::new`, 0.19 of
a font reload, 0.24 of the fallback restack, 0.05 or less of
`Renderer::new` and of any frame but the narrowed corpus's (below), and
0.17 MB. A zoom rebuilds only the per-face scales, shared or not. The
rest of the distance from the parent, a frame drawn again among it, is
the no-instances arm's too.

**The first word narrowed.** A probe frame of the prompt and the 399
Noto lines of `baud-font/tests/shape/devanagari.txt` after it, at 30 px,
its terminal and a pixel digest included, the digest 1.210 ms
[1.202-1.299] in the second run, 10 words narrowed: 7.423 ms
[7.227-7.722] and 131.18 million instructions, where the no-instances
arm, narrowing none, takes 6.952 and 122.09, and per face 9.517.
Nothing is built on first need: the difference is the narrowed words'
glyphs rasterised at their widths and each word measured at the
instances it tried. Drawn again, 1.606 ms and 44.02 million against
1.578 and 43.42.

## Rows with nothing to re-form (2026-11-03)

**A row with no character a syllable or a word is made of is let go by
the layout's own pass over it, and its runs are drawn by the loop that
reads neither: the five recordings with no Devanagari retire 0.06-0.22%
fewer instructions than before words were drawn whole, where they
retired 1.01-2.45% more.** What paid it on btop, by ceiling arms of the
word layout's commit: the syllable walk, 77.6 million of its 89.3
million instructions, and the rest the word and syllable tests of the
run loop and the light kept a cell, whose arm without them as well came
within 0.12% of the earlier build. Taken 2026-11-03, Apple M5
Max, macOS 27.0, on AC, load 3-8. `frame`, JetBrains Mono with Kohinoor
Devanagari behind it, five reps a process, whole-process instructions
(`/usr/bin/time -l`), five rotated rounds with the first dropped,
against the median of the build before words were drawn whole, whose
own rounds lie within 0.11 points of it: the word layout's commit; the
gate as a pass of the renderer's own, one load a cell from the block
table; and the gate as built, the layout's pass answering, with a
`cmp`-identical A/A copy. Median [range]:

| recording | word layout | own pass | built | A/A copy |
|---|---|---|---|---|
| btop | +2.45% | +0.11% | -0.22% [-0.23, -0.12] | -0.21% |
| cat-scroll | +1.75% | +0.21% | -0.09% [-0.10, -0.01] | -0.08% |
| pipes | +1.01% | +0.09% | -0.07% [-0.08, -0.07] | -0.06% |
| git-log | +1.49% | +0.17% | -0.06% [-0.07, +0.14] | -0.06% |
| less-pager | +1.71% | +0.21% | -0.10% [-0.12, -0.05] | -0.09% |
| vim-unicode | +1.78% | +0.90% | +0.54% [+0.53, +0.62] | +0.53% |
| cat-cjk | +1.99% | +1.17% | +0.87% [+0.84, +0.91] | +0.89% |
| less-cjk | +2.24% | +1.22% | +0.82% [+0.80, +1.04] | +0.81% |

Every arm draws the word layout's rows, instances, cache hits and
glyphs, and `FRAME_HASH=on` gives its pixels on every recording, the
earlier build's on the five without Devanagari; cycles overlap the
earlier build's range on those five. With BDSM reset the layout keeps
no answer for a row, and the renderer's own pass answers: btop +0.06%, cat-scroll
+0.21%, vim-unicode +0.90%, cat-cjk +1.15%, where the word layout's
commit retired +2.45, +1.72, +1.82 and +2.04%.

The cell path's frame loop, `frame_gpu`'s app arm fed at 2 MB/s so
every read is a frame of its own, CPU microseconds a frame, five rotated
rounds of three a process, the first round and each process's first
dropped: btop 55.9 [54.3-62.8] against the earlier
build's 56.5 [53.4-62.2] and the word layout's 81.8, cat-scroll 110.9
against 111.0 and 126.3. With BDSM reset it folds every row of every
frame: btop 53.1-55.8 against 48.8 and 75.2. vim-unicode, 88.5 against
62.1 and 98.9, and cat-cjk, 97.0 against 78.8 and 111.9, lay and draw
their words by records on every frame, their rows changed or not.
`ingest2`'s machine code is the word layout commit's, its `__TEXT,__text`
hashing the same.

## A row's words laid once (2026-11-03)

**A frame over rows whose cells did not change lays none of their words
and, on the cell path, draws the records it drew: the app's frame loop
over an unchanged screen costs vim-unicode 59.7 us of CPU a frame where
it cost 93.0, and cat-cjk 37.2 where it cost 51.8, against 56.7 and 38.0
before words were drawn whole.** Taken 2026-11-03, Apple M5 Max, macOS
27.0, on AC. Four arms at one path -- the build before words were drawn
whole and the parent, each given this change's `FRAME_IDLE` on the app
arm, this change and a `cmp`-identical copy of it -- five rotated rounds
of three a process, the first round and each process's first dropped;
JetBrains Mono with Kohinoor Devanagari behind it.
`frame_gpu`'s app arm at 2 MB/s, then `FRAME_IDLE=200` frames over the
last screen with `FRAME_CARET=switch`, median CPU us a frame [range]:

| recording | before words | parent | change | A/A copy |
|---|---|---|---|---|
| vim-unicode | 56.7 [55.9-60.9] | 93.0 [92.2-93.5] | 59.7 [58.8-60.9] | 59.5 |
| cat-cjk | 38.0 [38.0-40.6] | 51.8 [51.5-53.4] | 37.2 [37.0-38.1] | 37.2 |
| less-cjk | 21.5 [21.3-22.1] | 21.8 [21.5-22.2] | 21.4 [21.3-22.0] | 21.6 |
| btop | 21.6 [21.3-23.5] | 23.7 [21.7-24.2] | 22.6 [21.6-24.5] | 22.4 |
| cat-scroll | 29.6 [28.2-31.7] | 29.7 [27.4-31.0] | 29.0 [28.0-29.5] | 29.8 |

With BDSM reset: btop 21.2 against 24.1 and 20.1, vim-unicode 48.9
against 78.2 and 47.5, cat-cjk 25.6 against 44.3 and 26.6. On the app
arm's own frames, every read a frame, the rows do change: about half of
vim-unicode's frames bring 11 or 12 rows not kept before and every
cat-cjk frame 11 to 15, and the medians stay the parent's -- vim-unicode
79.9 [77.2-85.2] against 78.9 [77.2-81.7], cat-cjk 90.5 [86.8-104.0]
against 86.8 [82.8-87.6], ranges overlapping, as in two earlier sweeps
-- while BDSM reset no longer folds a row: btop 47.0 [46.1-47.8] against
50.4 [49.5-51.7] and 47.4. A kept answer for that fold, each row compared
whole with a copy, saved 0.6 of its 3.5 us; the glyph map's walk notes
each row's blocks on the entries it probes instead.

A row is kept while it holds a syllable or a word, measured on rows of
distinct Hindi words: 18.5 KB a row at 200 columns and 39.7 KB at 400 on
average, a glyph's place 36 bytes, and the cell path's records kept for
it 6.3 KB and 12.6 KB. At most twice the window's rows are kept, a bound
of 2.0 MB at 200x50 and 12.5 MB at 400x150 from the most a row took, for
rows as wide as the window. A window made smaller drops the rows used
longest ago down to twice its new rows, and rows of its old width stay
until rows of its new width take their places: from 400x150 to 200x50,
up to 4.2 MB, and 3.1 MB on a screen that then does not change, from the
same most a row took; a frame reads only the rows on screen. The CPU
frame bench
draws the same frames, rows, instances and glyphs, and retires
+0.13% on vim-unicode, +0.39% cat-cjk, -0.26% less-cjk, +0.20% btop and
+0.24% cat-scroll against the parent, the A/A copy within 0.03 points,
cycles overlapping. `FRAME_HASH=on` gives the parent's pixels on all five
with BDSM set and reset, and `frame_gpu`'s `FRAME_HASH=check` after 20
idle frames the parent's digest on each of the async, copied and app
arms; the app arm, which alone reorders, differs from the copied arm on
vim-unicode with BDSM set in both builds, the check reporting it. The
`ingest2` binary is `cmp`-identical to the parent's.

## The cursor blink's cost: a battery reading, unmeasured on mains

**Taken at `7f8413d`, 2026-09-25, on battery at 30%** -- not quotable by this file's own rule, a retake on mains owed. It is
published here labelled, and **nothing on this page gives the blink's cost
at HEAD**.

scripts/idle-cpu.py, each arm a wrapper that raises its window and then
`exec`s baud, four rounds of ten seconds with the first discarded, a focused
window, an A/A copy of HEAD in the sweep; percent of one core:

| arm | idle CPU | range |
|---|---|---|
| HEAD, whose blink is never drawn | 0.636% | [0.592-0.649] |
| blinking | **0.887%** | [0.880-0.938] |
| `blink = false` | 0.600% | [0.585-0.612] |
| HEAD, A/A copy | 0.613% | [0.597-0.638] |

Blinking was disjoint from HEAD against a floor of 0.041%: 2.2 flips a second at
about 1.1 ms of CPU a flip, at the default window size; a background window pays
none of it. **That reading is stale twice over.** The frame it priced resolved
the whole rendition table, which the cache removed the same day (`60cce43`,
above). And on macOS the rate is the system caret's since 2026-09-27, AppKit's
650/350 ms where it is unset -- two flips a second against 600/300's 2.22. The mains retake was attempted at `60cce43` and could not be
taken, because a window raised by script is frontmost and not focused, so
nothing blinked.

**Stale a third time**: the rate became 333/333 ms where nothing sets it, and
the default blink animated -- the plasma glow drew a frame every display tick
of the hidden phase, 613 frames in ten seconds at 120 Hz, about 41 a 666 ms
cycle, where the hard switch drew two. The idle CPU of that default was not
measured, and the default below replaced it.

## The default blink is a plain switch at 250 ms

**What changed.** Where nothing sets them, each phase is 250 ms (`caret.rs`,
`EVEN`) and `blink_animation` is `"none"`, the hard switch. The order of
precedence is unchanged: `[cursor] blink_interval_ms`, then macOS's
`NSTextInsertionPointBlinkPeriodOn` and `...Off` where the user set them, each
on its own, then 250; on Linux the config's value, else 250. The plasma, fade,
fire and ufo stay as settings, and Reduce Motion and `blink = false` behave as
before. The reason: the plasma glow's pulse in the middle of the hidden phase
read as a second blink, a plain edge reads as one even beat, and two blinks a
second are livelier than 1.5.

**Frames an idle blinking cursor owes**, 120 Hz ticks over ten seconds, counted
by `caret.rs`'s `frames_a_cycle_for_each_animation` and
`the_default_blink_draws_only_its_flips` (no clock, the caret's own plan):

| arm | rate | frames in 10 s | a second |
|---|---|---|---|
| the default before: plasma | 333/333 | 613 | 61.3 |
| the default now: none | 250/250 | 39 | 3.9 |
| plasma, by name | 250/250 | 619 | 61.9 |
| fade, by name | 250/250 | 359 | 35.9 |
| fire, ufo, by name | 250/250 | 599 | 59.9 |
| none, as the base pinned it | 333/333 | 30 | 3.0 |

The default draws a sixteenth of the frames it drew: its flips, 20 hides and
19 shows, the twentieth show a tick past the ten seconds because the cycle
restarts at tick 1. A frame is the cursor's row on the CPU path (`frame`'s
`FRAME_BLINK`). This is off the per-byte path; no ingest arm was timed.

**WCAG 2.3.1** (`[[source: wcag22]]`): `baud-bench/tests/blink_flashes.rs`
counts the hard blink at 250/250 on the CPU path at 60 frames a second, at
13, 32, 46 and 95 pt, 1x and 2x, on the dark and the light theme: in every
case two general flashes in the worst second disjoint, three chained, no red
flash, and no area flashing more than three times, so it passes on the count
before the area is read. The cursor's pixels are 129-7141 pt^2 of the 21,824
limit. The control, the same cursor at 125/125, counts four flashes. **WCAG
2.2.2**: blinking past five seconds can be stopped by `blink = false` and
`stop_blinking_after`, both unchanged.

## baud's amber cursor: contrast, flashes and the trail hashes

**What changed.** The built-in themes' cursor is baud's amber, chosen against
the page drawn (`baud_render::BaudCursor::on`): `[palette] cursor = "baud"`,
the default, over a theme file's cursor too, and every trail and blink takes
it. Draw-time only: no per-byte path changed and no ingest arm was timed.

**Contrast**, WCAG 2.2's ratio (`[[source: wcag22]]`), against the page drawn
opaque, computed by `baud-render/tests/cursor_amber.rs` from the standard's
definitions, which derives every value again with the platform's `powf`:

| page | cursor | on the page | text | text on it |
|---|---|---|---|---|
| `#101010`, dark | `#F4A32C`, the logo's | 9.17 | `#15171B` | 8.65 |
| `#EFEFEF`, light | `#986100` | 4.52 | `#EFEFEF` | 4.52 |
| `#D8D8D8`, dark reversed | `#855400` | 4.51 | `#D8D8D8` | 4.51 |
| `#272727`, light reversed | `#F4A32C` | 7.20 | `#15171B` | 8.65 |

The rule: `#F4A32C`'s oklch(0.7775 0.155 70.7), lightness stepped away from the
page by 0.0001, hue held, chroma the most sRGB holds up to the amber's own
(bisected), rounded to bytes, to the first step that reads 4.5:1 (1.4.3, so
the prompt's `❯` in it reads as text); the text is `#15171B` where it reads
4.5:1, else the page. `#986100` is step 2,374, L 0.540, chroma 0.116, hue
71.1; the step before, `#996200`, reads 4.49. On it `#15171B` reads 3.46 and
the light text `#272727` 2.88. The previous light amber, `#C07B00` (3:1), read
4.32 on the reversed light page; the dark amber read 1.46 on the reversed
dark page, which the choice against the page drawn replaces with 4.51.

**WCAG 2.3.1** (`baud-bench/tests/trail_flashes.rs`): each trail across a 64 by
20 cell window at 1x and 60 frames a second, one full pass and two repeats at
once, on both themes, counted by `Census`. Worst verdict area 24 square points
(light liquid), worst chained 346 (light liquid), of 21,824; at most 5 general
flashes a pixel in a second (light liquid), each over an area far under the
threshold. Red: 0 transitions on all twenty runs, linear and encoded; the
amber's linear R/(R+G+B) is 0.70 dark and 0.72 light, under 0.8. The control:
a plasma under `OSC 12 ; #ff0000` makes red transitions. Fireworks keep their
own colours; `fireworks_flashes` passes unchanged.

**Frames moved, and why.** `trail 1` at 64x20 (`TRAIL_SIZE=64x20`): all 20 rows'
hashes moved against the same tree with the old cursor, the trail and the block
now amber. The pixel pins: six of `golden.rs`'s nine and its light git-log,
`system.rs`'s cat-cjk, 92 of `every_trail_draws_the_same_pixels_on_every_os`'s
96 and all 74 of `every_blink_draws_the_same_pixels_on_every_os`'s; with the
cursor of no colour of its own every golden and system pin is its old value.
The light amber's move to `#986100`, with the page under it, moved the light
git-log pin alone; every dark pin is the same, its amber and text unchanged.

## The README logo's plasma, ported from the frames it was chosen in

**What changed.** The README's header logo is the lockup with baud's plasma
moving round and through it as if the glyphs were walls
(`assets/readme/README.md`). The renderer gained splash layers drawn behind or
in front of the lockup, through its glyphs as slabs (`splash::Slabs`: the
keyline, the knot cut, the rim), and a plasma detail clock (`effect::Live`);
the generator is now `tools/baud-pictures geometry` and `plasma`. Draw-time
only, in a tool the script runs: no per-byte path changed and no ingest arm
was timed. The trail bench's frame hashes (`trail 1`) equal master's on all 60
rows.

**How close it is to the frames it was chosen from**, the window's interior
(frames 18 to 194, clear of the fades), pixels of 487,200 a frame more than 8
levels apart in a channel, and the mean level apart, medians over the frames:

| this | against | px > 8 levels, median (max) | mean level, median (max) |
|---|---|---|---|
| the chosen frames' own strands, drawn by this renderer | the chosen GIF | 1,231 (3,380) | 0.124 (0.232) |
| the generator's strands, drawn the same | the chosen frames' own strands, drawn the same | 0 (92) | 0.000 (0.006) |
| this film | the chosen GIF | 1,231 (3,380) | 0.124 (0.232) |

The light half reads 1,139 (3,175) and 0.119 (0.225). The floor is the chosen
GIF's encoder (quality 90, its palette snapped) and what the renderer changed
since: a leftward strand's braid mirrored (e7253f69) and glyph coverage
rounding (e30fd9c4). The generator's strands differ from the chosen ones only
in the current's tail: libm's `sin` and `exp`, which the chosen frames used,
round differently from the generator's own (`det`) in the last bit, and the
advected parcels are chaotic, so 87 of the window's 11,048 tail links move
more than half a unit; the head, the residual and the tongues are the same to
the printed digit. Over the whole ten-second chain 2,138 of 28,550 tail links
move.

**Why the walls are frozen.** Recomputed from today's drawing of the lockup --
3,930 pixels apart by at most 2 levels, 3 pixels of the half-coverage mask --
the chain's seed search lands on another seed, 2309, and every strand of every
frame moves; seed 2241 itself draws a chain 31% behind the letters with two
counter passes, under its targets of 40% and three. So the drawing the path was
chosen against is kept (`assets/readme/logo-walls.png`), and every run holds
today's drawing to it at those counts.

**The files**: GIFs of 2,671,015 and 2,646,691 bytes; SVGs of 1,178,009 and
1,102,717. WCAG 2.3.1: 9 general and 0 red flashes at a pixel in a second,
areas 630.5 and 761.25, strict 1,495 and 2,304.5, of 21,824: both pass by the
area. The word's worst contrast 6.79 and 12.61 to 1. The haze: 0.0004% of the
dark GIF's page pixels are off the page, by at most 13 levels.

## The cursor trails' costs

scripts/idle-cpu.py, the window unfocused, five rounds of ten seconds with the
first dropped, an A/A copy of the new arm in each sweep; percent of one core,
frames from the app's own report over each run's first ten seconds where they were kept. Two movers from the quad on: a 30-column jump every 200 ms,
and typing -- a keystroke every 100 ms with a newline every 5 s (the quad's
table writes "a newline every 50"; at a keystroke every 100 ms that is five
seconds, as its text says). **Each table is one sweep, and the absolutes move
between sweeps** -- under the same jumps the fire read 6.06% in the lightning's
sweep and 2.14% in the ufo's -- so only arms of one sweep are compared. The
frame is the cost: about a quarter of a millisecond of Metal's submission a
paced frame (above). **The default trail at HEAD is the plasma** since `eb2f75e`
(2026-10-10, "The plasma is the default trail, and the fire stays by name"), and
its only cost reading is `bc40570`'s, below. No trail cost has been re-taken at
HEAD.

**The first trail, at `7f8413d`, 2026-09-25, on battery**, a child moving the
cursor between two cells every 200 ms: trail on 6.384% [6.164-6.676], off 1.238%
[1.222-1.249], the A/A copy 6.497% [6.101-6.523] -- about 55 more frames a
second at about 0.9 ms each. On mains the same pre-cache frame read 7.08%
against 1.33% (`60cce43`'s HEAD arms, above), and 3.12% against 0.97% with the
cache.

**The quad, at `c2e20be`, 2026-09-25, on mains**, over a user config with
the blink off, HEAD's binary (the ghost) and the change's built at one path. The typing rows
are the committed build's; the jump rows are the build before the held-key rule
and the looking-back snap, neither of which a 30-column move reaches.

| mover | off | ghost, HEAD | ghost | quad | quad, A/A |
|---|---|---|---|---|---|
| a 30-column jump every 200 ms | 0.77% | 2.93% [2.58-3.09] | 2.80% | **4.83%** [2.30-5.60] | 4.96% [4.77-5.24] |
| its frames a second | 5 | 61 | 60 | **116** | -- |
| a keystroke every 100 ms, a newline every 50 | 1.29% [1.25-1.48] | 5.41% [4.97-5.58] | -- | **1.50%** [1.39-1.55] | 1.49% |
| its frames a second | 9 | 112 | -- | 9, and 14 in the window a newline falls in | -- |

**The sweep, at `8da4787`, 2026-09-25, on mains at a load of about 2.5.** Its CPU columns are no result -- the A/A copy reads 3.49% against
the sweep's 5.80% with the same frames:

| mover | off | quad | sweep | sweep, A/A |
|---|---|---|---|---|
| a 30-column jump every 200 ms | 0.98% [0.46-1.10] | 5.50% [4.89-6.02] | 5.80% [2.32-5.98] | 3.49% [2.62-4.45] |
| its frames, ten seconds | 48-50 | 1183-1184 | 1181-1184 | 1182-1185 |
| a keystroke every 100 ms, a newline every 5 s | 0.98% [0.55-1.34] | 1.29% [1.24-1.69] | 1.50% [1.23-1.61] | 1.42% [1.15-1.64] |
| its frames, ten seconds | 88-94 | 112-114 | 113-115 | 114-118 |

**The fire, at `8211be4`, 2026-09-25, on mains at a load of about 2.5.** Under typing a fire costs +0.68% of a core over the quad,
69 frames more in ten seconds; under the jumps the two are no result:

| mover | off | quad | fire | fire, A/A |
|---|---|---|---|---|
| a 30-column jump every 200 ms | 0.95% [0.75-0.98] | 5.78% [2.84-6.38] | 6.18% [5.11-6.35] | 5.76% [3.44-6.45] |
| its frames, ten seconds | 48-49 | 1183 | 1181-1184 | 1180-1184 |
| a keystroke every 100 ms, a newline every 5 s | 1.31% [1.31-1.35] | 1.55% [1.54-1.69] | **2.23%** [2.06-2.25] | 2.21% [2.15-2.26] |
| its frames, ten seconds | 89-90 | 113-114 | **182-183** | 182-183 |

**Four sweeps of 2026-09-26 whose power state was not recorded**,
each on the fire's protocol (the plasma's over a config with the blink off):

* **The plasma, `bc40570`** ("The plasma trail is a braid that starves, and the critique
  shaped it"):
  under the jumps the quad, the fire and the plasma read +5.9%, +5.2% and
  +5.8% of a core over no trail, all inside the copy's 3.0% spread; under
  typing the copy spread 0.63% and nothing separated. No result.
* **The lightning, `0cef30d`** ("The lightning trail is one flash a pixel,
  over a region of its own"): under the jumps the fire 6.06% [5.96-6.27],
  the lightning 3.92% [2.23-4.93] and its copy 4.52% [4.31-4.87], the copy
  disjoint from the fire; under typing the copy spread 0.86% and nothing
  separated from no trail.
* **The ice, `26b26b0`** ("The ice trail is frost laid on the path, and the
  bands learned to cover it"): under the jumps the fire 2.74% [2.65-6.50],
  the ice 3.64% [2.72-5.93], its copy 2.98% [2.51-5.04]; under typing the
  fire 0.80% [0.66-0.91], the ice 0.68% [0.62-0.79], its copy 0.85%
  [0.63-0.91]. Every range overlaps.
* **The ufo, `90bb30d`** ("The ufo trail is a saucer on a spring, and its
  beam waits for it"): under the jumps the fire 2.14% [1.83-2.21], the ufo
  2.26% [1.77-2.62], its copy 1.90% [1.72-2.47]; under typing the fire 1.62%
  [0.93-2.04], the ufo 1.74% [0.96-2.08], its copy spreading 1.5%. Every
  range overlaps.

**The orb on a spring, at `1488b3c`, 2026-09-26**, power not stated; its
frame counts were not kept. Under typing it cost +0.50% of a
core over no trail against the fire's +0.85%, disjoint from both:

| mover | off | fire | orb | orb, A/A |
|---|---|---|---|---|
| a 30-column jump every 200 ms | 0.93% [0.90-0.94] | 6.07% [5.98-6.20] | 5.97% [5.80-6.04] | 5.99% [5.88-6.22] |
| a keystroke every 100 ms, a newline every 5 s | 1.21% [1.20-1.23] | 2.06% [2.02-2.13] | **1.72%** [1.69-1.72] | 1.73% [1.72-1.78] |

**The liquid, at `818daaa`, taken on mains on 2026-09-27** at a one-minute load
of 1.2-1.8, on the branch's tip built that day, which carries
three changes to what runs since the liquid: a refused ignition owing no frames,
the orb's landing and the core's `ESC` rule. Under the jumps it costs 0.57%
of a core less than the fire with 129 fewer frames in ten seconds; under typing
+0.34% over no trail against the fire's +0.71%, disjoint from both:

| mover | off | fire | liquid | liquid, A/A |
|---|---|---|---|---|
| a 30-column jump every 200 ms | 0.95% [0.94-0.96] | 5.26% [5.19-5.48] | **4.69%** [4.65-4.73] | 4.73% [4.66-4.78] |
| its frames, ten seconds | 48-49 | 1184 | **1055-1056** | -- |
| a keystroke every 100 ms, a newline every 5 s | 1.22% [1.21-1.25] | 1.93% [1.88-1.96] | **1.57%** [1.56-1.58] | 1.60% [1.53-1.63] |
| its frames, ten seconds | 89 | 182 | **135** | -- |

**The orb that lands, at `9e06507`, in the liquid's sweep on mains on
2026-09-27.** The binary is that sweep's branch tip, not `9e06507`'s tree alone. Under the jumps it costs what a fire costs; under typing +0.45% over no
trail against the fire's +0.74%, disjoint from both:

| mover | off | fire | orb | orb, A/A |
|---|---|---|---|---|
| a 30-column jump every 200 ms | 0.93% [0.92-0.94] | 5.24% [5.21-5.33] | 5.16% [5.12-5.19] | 5.11% [5.08-5.13] |
| its frames, ten seconds | 48-49 | 1184 | 1184 | -- |
| a keystroke every 100 ms, a newline every 5 s | 1.22% [1.18-1.28] | 1.96% [1.92-2.03] | **1.66%** [1.63-1.76] | 1.68% [1.63-1.70] |
| its frames, ten seconds | 89 | 182 | **143** | -- |

**A trail riding a smooth scroll, at `0415566`, 2026-09-27, on mains** at a
one-minute load of 2-5, under the fire, ride against `be25132` and `stay`:
every pair overlaps on every mover, the A/A copy spreading 1.3-2.0% of a core
under the scrolling movers against 0.07% idle. No result.

**A program's scroll taken away from the cursor's move** (note W 5.15),
in retired instructions and cycles (`proc_pid_rusage`), not time: the
change is off the per-byte path. A release build at `0141197f`, the base
before the rebase onto `abb4da08`, which changes nothing on the trail path,
plus the change, Apple M5 Max, macOS 27.0, on AC, at a one-minute load of 8
to 22 from other work, which moves the cycles and not the instructions. The
research's helix captures at 200x50 and 360x90, replayed at their read
times on a 120 Hz clock, five rounds, the medians of each round, a
counter pair's own 7,167 instructions taken off:

| a frame of `Carried::frame` | 200x50 instructions | cycles | 360x90 instructions | cycles |
|---|---|---|---|---|
| nothing read since the last (823, 824) | 45-70 | 120-1,095 | 45-70 | 105-471 |
| a read, the cursor in its cell (8, 7) | 27,917-27,942 | 4,642-7,473 | 83,432-83,482 | 13,266-18,174 |
| a move: the read and the vote (8) | 112,516-113,479 | 23,801-34,366 | 314,734-319,183 | 66,883-103,922 |

The vote at first hashed a word at a time, one chain of multiplies: 177,227
to 178,029 instructions and 75,564 to 85,935 cycles a move at 200x50,
528,474 to 529,001 and 263,409 to 299,157 at 360x90, at a load of 20 to
32; four lanes of two
words cut it to the above. Through `frame_trail`, the trail `present`
draws, the plasma over the 839 frames of the 200x50 replay retired 0.77 to
0.85 million instructions under `stay` and 1.80 to 1.83 million under
`ride` in rounds 2 to 5 (the first, cold, 1.22 and 2.01), and was handed a
trail on 270 frames against 389.

**On the GPU path, offscreen: the same binary, so not timed.**
`frame_gpu`, which draws through Metal into an offscreen texture, built
release from `abb4da08` and from the change at one path (`git archive`,
every file touched, every workspace crate it links recompiled), is the
same file byte for byte: `cmp` finds no difference, and its `__text` is
2,473,792 bytes in both. The change is in `baud-app` alone, which
`frame_gpu` does not link, so it adds nothing to the GPU path; what it
changes there is the trail the renderer is handed: over the helix replay
above, a trail on 270 frames against 389.

## The plasma at full strength (2026-11-04)

**Lit on every long move, clamped at the flash cap on a repeat, and drawn
in front of the text, the plasma fails none of the flash census's cases
on any of its four readings, and its pixel check finds no transition on
any repeat frame.** Taken 2026-11-04, Apple M5 Max, macOS 27.0, on AC.
`baud-bench/benches/flash.rs`, drawn by the Metal pipelines offscreen:
the plasma at 13, 24, 26, 32, 34.5 and 72 pt (cells of 17, 32, 34, 42, 46
and 95 pt), 1x and 2x, 60 and 120 Hz, the four inks, the seven motions,
bare and over `X`s (`FLASH_TEXT=1`), on the dark and light pages opaque
and on the glass over a white and a black desktop (`FLASH_GLASS=1`),
8,064 cases, and on the red and dark red grounds a program sets, opaque,
2,688 more. The block control failed at 4 Hz and passed at 2 Hz on every
run. Worst areas in pt² over 1x and 2x, against 21,824; the base is the
committed O2 census's plasma, the dark and light pages bare
(`b68-o2-census.tsv.gz`); "lit and clamped" is the tree before the blend:

| font | cell | base | lit and clamped | in front | glass | strict | red grounds | busiest pixel, a second |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 13 | 17 | 8,274 | 10,208 | 10,208 | 10,127 | 10,208 | 7,835 | 18 |
| 24 | 32 | 15,257 | 18,707 | 18,707 | 18,559 | 18,707 | 14,338 | 19 |
| 26 | 34 | 1,091 | 1,066 | 1,066 | 1,080 | 1,771 | 719 | 7 |
| 32 | 42 | 1,608 | 1,590 | 1,590 | 1,610 | 2,571 | 846 | 8 |
| 34.5 | 46 | -- | 1,923 | 1,923 | 1,937 | 3,161 | 1,196 | 7 |
| 72 | 95 | 8,170 | 8,599 | 8,599 | 8,219 | 12,125 | 4,085 | 7 |

Every move lit an ignition, 2,032 of 2,032 a size and scale on the bare
pages, where the base lit 672. Under the gate that raised the worst area
by 23%, to 86% of the threshold at 24 pt; the busiest pixel flashed 19
times a second, passing by the area clause as the base did. Over it the
pixel check (`under_over`, `under_red`, `over_cap`) is 0 on every case.

**The blend's first run flashed: the cap the bound read was
source-over's.** On the dark and light pages, opaque and glass, it
failed none of 8,064 cases on the four readings, but its pixel check
counted 11,245,366 general transitions on repeat frames, every one over
the dark theme's default text, the luminance moving by up to 0.118
against source-over's 0.098: source-over leaves a letter in the
cursor's own colour as it was, and `screen` lights it. The cap is now
scanned over the composite the plasma is drawn by and never above
source-over's (`flash::under_threshold_by`). Of the census's pairs three
fall, the dark page's default ink bare (95 to 94) and over its text (66
to 55) and the red ground's over text (78 to 60). Under the gate nothing
is a repeat and the frames are unchanged (28 of 28 frame hashes at 13
and 24 pt); above it the census ran again, 5,376 cases, with the table's
results, the busiest pixel back from 12 flashes a second to 7 or 8.

**A letter under it keeps more of its contrast than under source-over**
(`the_plasma_lights_a_letter_it_crosses_and_shades_it_on_a_light_theme`,
`X`s at 2x, 0.03 to 0.1 s into a 30-column move): every solid ink pixel
the light reaches moves, brighter on the dark theme and darker on the
light. WCAG's contrast of a cell's mean ink to its mean ground, the
least over the cells lit, is 5.68-6.88:1 on the dark theme against
source-over's 5.21-6.39, and 7.93-9.14:1 on the light against 7.05-8.15
(bare, 13.35 and 12.99). Its worst pair, a filament on the ground beside
a letter's dimmest ink, is 1.14-1.69:1 against 1.05-1.65, and 1.15-2.11
against 1.07-2.04: the filament is the text's colour at up to 0.9 of its
opacity, and neither blend keeps 4.5:1 there.

**What it costs**, in instructions retired (`/usr/bin/time -l`); wall
time is unmeasured, under a load of 10 to 26. The base `192dcc9d`, the
lane's change on it before the cap's fix, which runs only for a repeat,
and a `cmp`-identical A/A copy: release builds at one path, six rotated
rounds, the first dropped. The `frame` bench's plasma trail over
`git-log`'s last screen, 600 full frames at 200x50 and 14 px, less its
`none` run, over 600, M a frame [range]; rows and records equal:

| theme | base | change | A/A copy |
|---|---|---|---|
| dark (the change by `screen`) | 3.260 [3.255-3.265] | 3.295 [3.291-3.302] | 3.296 |
| light (the change by `multiply`) | 3.260 [3.251-3.275] | 3.309 [3.304-3.313] | 3.310 |

+1.1% and +1.5%, ranges separate. `frame_gpu` has no trail mode: its
arms and the `frame` replay of `vim-unicode`, with no trail, overlap
their base (2.0724 G [2.0643-2.0829] against 2.0896 [2.0753-2.0984], A/A
2.0902; 1.7738 G against 1.7758), no result; what the two blend states
cost the GPU is unmeasured. Over `vim-unicode` the `frame` bench's
default move lights an ignition pointing left (`dir.x` -0.998), so the
hand mirrors it: 4.92 rows a frame become 4.95.

## The glass's backdrop premultiplied once a run (measured, no change)

**A memo of the premultiplied backdrop paints the see-through page no
faster on the CPU path and retires more instructions, so it was not
taken.** At `4aca0ef2`, 2026-11-01:
Apple M5 Max, macOS 27.0, on AC, at a one-minute load of 5.0-7.9 from other
work (3.4-3.8 for the inlined arms below). Over the glass,
`Blend::over_glass` premultiplies the pixel under each partly covered pixel
as the presenter will, `baud_domain::premultiply_chan` on three channels.
The arm tried, in `Target::over`'s two glass loops (out of line, and
inlined into the record's loop below), keeps the last backdrop word with
its premultiplied word and forms it again only when the word changes. A build of it with counters
added shows it removes 92-99% of the premultiplies: btop 8,765 a frame to
667, cat-scroll 58,186 to 1,398, git-log at 6K 401,533 to 3,866.

The premultiply was already cheap. LLVM forms the alpha's multiplier once a
pixel and each channel as one multiply-add and a shift: removing it outright
saves 11.0-11.3 instructions a partly covered pixel on btop and cat-scroll
at 1x, and the memo, net, retires 4.4-5.9 instructions a partly covered pixel
more than the base (btop 5.9, cat-scroll 4.4).

`frame`, each arm a `git archive` at one path, the memo built twice and
`cmp`-identical, five rounds rotated, the first dropped; 200x50 cells at 14
px (1x) and 28 px (2x), 360x90 at 28 px (6K); 3 reps, 2 at 6K; a dim of 179
(the dark default, 0.70) or 153 (the light, 0.60). A patch to every arm's
bench, not in the tree, added a light-theme switch, the cell size in px, and
an FNV-1a over every frame's pixels on every rep, printed once a run. The
last column drops the premultiply, wrong by construction. Paint in us a
frame, median [range], and instructions a process against the base:

| recording, page | base | memo | memo, A/A | no premultiply |
|---|---|---|---|---|
| btop, dark 1x | 97.5 [96.9-98.6] | 99.5 [97.5-99.9] +0.7% | 99.4 [98.7-100.1] +0.7% | 94.8 [94.5-95.5] -1.4% |
| cat-scroll, dark 1x | 455.1 [451.9-457.9] | 463.4 [457.9-466.5] +0.8% | 461.5 [453.5-464.3] +0.9% | 437.6 [436.4-438.0] -2.1% |
| cat-scroll, light 1x | 453.9 [452.9-458.2] | 462.7 [460.8-464.7] +0.9% | 459.2 [455.0-466.2] +0.8% | 434.7 [433.8-435.1] -2.1% |
| cat-scroll, dark 2x | 1190 [1173-1209] | 1204 [1197-1217] +1.2% | 1215 [1194-1216] +1.2% | 1141 [1127-1156] -3.1% |
| cat-scroll, dark 6K | 2859 [2856-2888] | 2923 [2882-2957] +1.0% | 2894 [2854-2914] +1.0% | 2783 [2745-2800] -2.5% |
| cat-scroll, opaque 1x | 388.9 [387.6-390.7] | 384.4 [382.2-389.7] 0.0% | 384.9 [382.9-391.5] 0.0% | 388.4 [384.6-392.1] 0.0% |

Instructions rise 0.6-1.2% on all ten glass rows taken (btop and cat-scroll,
each page above, a dim of 56, 2x and 6K), every range apart from the base's.
Paint overlaps the base on nine and is slower on cat-scroll light. The hash
covered 120 configurations: btop, cat-scroll, vim-unicode, less-pager,
git-log and cat-cjk; the dark page at 179, 56 and 255 and the light at 153
and 255; 1x incremental and full, 2x and 6K. The memo equals the base on
all 120. The no-premultiply arm differs on 66 of the 72 glass
configurations and on none of the opaque; the six it matches, btop at 2x
and 6K, do not test the premultiply.

**With the blend inlined into the record's loop**, so the memo can sit in
registers, the loop got worse: instructions +7.3-11.9% and paint +11-19%
against the base, its A/A `cmp`-identical. Counted over `over_as_is`, its
`ldur` loads went from the inlined base's 52 to 82 and its `str` stores from
40 to 62 (83 to 112 counting `stp` and `stur` too); over `over_translucent`,
62 to 70 and 38 to 51. The inlining alone, one build with no A/A and no
hash, read paint 2.0-3.5% below the base, ranges apart, on seven of the ten
glass rows (btop light, dim 56 and 2x; cat-scroll light, dim 56, 2x and 6K)
at +0.2-0.8% instructions: a lead, not a result.

**The blend inlined into the glass loops, without the memo, is not taken
either: pixels equal, paint below the base on nine of ten glass rows, and
no result on the tenth.** At `90e22100` with `frame`'s light theme, font
size and hash, which it now carries (`FRAME_THEME`, `FRAME_PX`,
`FRAME_HASH=on`), so every arm ran the tree's own bench; 2026-11-01, the
same machine on AC, at a one-minute load at the start of rounds 0-4 of
4.74, 4.32, 3.88, 3.47 and 3.25 (3.06 at the end) in the first sweep and
2.53, 2.41, 2.51, 2.66 and 2.71 (2.60) in the second. The narrow shape
inlines `Blend::over_glass`'s body into `Target::over`'s two glass loops
alone. Inlined into each loop's closure, the body left the closure out of
line, still a call for each pixel blended, so the shape also marks the two closures
`#[inline(always)]`, which takes the nightly stmt_expr_attributes feature.
The wide shape is the lead's own build, `Blend::over_glass` itself
inlined at every caller. Built, `over_as_is` and `over_translucent` are
instruction for instruction the same in the two shapes, addresses
masked, and the wide differs from the narrow only in `blit_blended`, the
trail's quad and `cover_set`, and in keeping no out-of-line
`Blend::over_glass`, none of which these recordings draws: the second
sweep repeats the first's loops against a fresh base. Each arm a
`git archive` at one path, every file touched, each shape built twice
and `cmp`-identical, five rounds rotated, the first dropped; the rows and
reps above. Paint in us a frame, median [range], and instructions a
process against each sweep's own base:

| recording, page | base | narrow | narrow, A/A | base | wide | wide, A/A |
|---|---|---|---|---|---|---|
| btop, dark 1x | 99.8 [99.0-100.5] | 96.6 [96.3-97.3] +0.6% | 96.2 [95.5-97.6] +0.6% | 102.0 [99.7-105.3] | 97.6 [96.9-99.3] +0.6% | 97.4 [96.9-97.7] +0.6% |
| cat-scroll, light 1x | 468.9 [464.6-470.5] | 454.1 [449.1-455.9] +0.8% | 451.4 [448.2-455.2] +0.8% | 476.1 [470.9-476.6] | 459.5 [457.2-462.1] +0.8% | 458.5 [455.9-464.0] +0.8% |
| btop, dark 2x | 284.8 [281.2-289.3] | 263.7 [263.0-277.8] +0.3% | 268.7 [262.1-275.8] +0.3% | 286.9 [282.7-292.6] | 271.9 [267.7-275.7] +0.4% | 269.4 [266.8-278.1] +0.3% |
| cat-scroll, dark 2x | 1230 [1212-1236] | 1169 [1164-1191] +0.8% | 1186 [1170-1258] +0.8% | 1253 [1224-1266] | 1204 [1178-1208] +0.8% | 1193 [1192-1194] +0.8% |
| btop, dark 6K | 357.0 [351.0-363.0] | 337.8 [333.3-354.0] +0.2% | 336.6 [331.3-344.8] +0.2% | 361.1 [355.6-378.6] | 340.1 [338.6-375.3] +0.3% | 350.2 [337.1-351.3] +0.3% |
| cat-scroll, dark 6K | 2928 [2911-2958] | 2829 [2805-2884] +0.6% | 2854 [2820-2920] +0.6% | 2968 [2941-3022] | 2871 [2866-2902] +0.6% | 2868 [2846-2904] +0.6% |
| btop, opaque 1x | 92.5 [90.8-94.6] | 91.9 [91.4-94.0] 0.0% | 92.7 [92.4-93.2] 0.0% | 94.1 [92.2-94.2] | 93.6 [92.7-94.9] 0.0% | 93.7 [92.1-94.5] 0.0% |
| cat-scroll, opaque 1x | 399.1 [398.8-401.0] | 399.9 [397.0-401.6] 0.0% | 398.9 [393.3-400.3] 0.0% | 401.4 [398.3-404.6] | 405.5 [403.5-411.0] 0.0% | 402.6 [401.2-404.5] 0.0% |

On the ten glass rows, paint lies below the base, ranges apart, on nine
in each sweep, and the A/A copies' on seven and ten; the medians are
2.2-7.4% below their base's on every glass row of all four copies, at
0.2-0.8% more instructions. On btop at 6K, two reps a run, the change's
range overlaps the base's in both sweeps, so that row has no result, and
neither shape meets the bar set before the sweeps ran: every glass row
apart, the A/A's too, the opaque rows not slower. The opaque rows overlap
the base in paint and are within 0.05% in instructions. The hash covered
the 120 configurations above. Both shapes equal the base on all 120; the
no-premultiply arm, the positive control, differs on 66 of the 72 glass
configurations and on none of the 48 opaque; and at 1x, incremental
damage hashes as full damage does on every recording and page.

## What `cargo install` and `nix build` make

Taken on `78d68d0` with this section's change applied, 2026-10-29. Apple
M5 Max, macOS 27.0, rustc 1.96.0-nightly 02c7f9bec. A crate's arm is
`cargo package --no-verify` of `baud` and the crates it depends on, each
`.crate` unpacked and patched in for its registry name, and `cargo install
--locked --path` of baud's, with no `RUSTFLAGS`: what a user's `cargo
install baud` builds. Cargo reads a profile from the root manifest alone
(`[[source: cargo-eb94155-profiles]]`), and the published crate is its own
root, so before this change none of the workspace's `[profile.release]`
reached it. The workspace arm is `cargo build --release -p baud` in the dev
shell. Symbols are `nm`'s text symbols; the floor is `LC_BUILD_VERSION minos`.

| arm | `__text` bytes | text symbols | 64-byte aligned | minos |
|---|---|---|---|---|
| workspace build | 3,071,552 | 3,018 | 3,015 | 26.0 |
| crate, before | 3,075,300 | 5,263 | 366 | 11.0 |
| **crate, after** | **3,071,552** | **3,018** | **3,015** | **26.0** |
| crate, after, `MACOSX_DEPLOYMENT_TARGET=14.0` | 3,071,552 | 3,018 | 3,015 | 26.0 |
| crate, after, `build.rs` floor line removed | 3,071,552 | 3,018 | 3,015 | 11.0 |
| `nix build`, nixpkgs' rustflags | 3,077,632 | 3,040 | 3,037 | 26.0 |
| **`nix build`, `RUSTFLAGS` empty** | **3,071,552** | **3,018** | **3,015** | **26.0** |

Before, the crate built without LTO (5,263 symbols), in cargo's default
16 units, with the alignment on 7% of its functions. After, its functions
have the workspace build's sizes, the same multiset of 3,017; the
`__text` bytes differ, and so do the symbol names' crate hashes. A
deployment target in the environment leaves the floor at 26.0, because `build.rs` sets it for baud's own rustc
(`[[source: cargo-eb94155-build-scripts-rustc-env]]`,
`[[source: rustc-02c7f9b-apple-darwin]]`). The one span between symbols
that differs, in every arm, runs from the Mach-O header to the first
function: it is padding that moves with `__cstring`'s size, which holds
the source paths the build ran from, and it is not code. Without the
line, the floor is
rustc's default, 11.0.

nixpkgs' cargo hook writes `-Cforce-frame-pointers=yes` into the target's
`rustflags` (`[[source: nixpkgs-9cf7092-rust-hooks]]`): 6,080 more bytes
of `__text` and 20,328 of `__eh_frame`, a build never measured on ingest.
Neither turning cargo-auditable off nor nix's strip setting changed the
`__text` size, the symbol count or a function size. An empty
`RUSTFLAGS` displaces the target's (`[[source: cargo-eb94155-config-rustflags]]`)
while the profile's alignment stays, so the flake sets it.

A stable cargo 1.99.1 stops at the after crate's manifest, with nothing
built: "the cargo feature `profile-rustflags` requires a nightly version of
Cargo, but this is the `stable` channel", the same from `--path` and, for a
one-line crate with the same `cargo-features`, from a directory registry
(`[[source: cargo-eb94155-cargo-features-tests]]`).

## A narrowing at a prompt re-cuts the output above it

**What changed.** At a marked prompt a change of width re-cut nothing, so a
listing printed in a new window and narrowed kept every row at the old width:
long lines clipped at the edge, a line wrapped at the old width with its
middle past it. Now `Term::resize` re-cuts every line above the shell's block
and keeps the block's rows whole (`View::reflow`, `Recut::Above`; note M), and
the display draws the re-cut output at its words. The shell's count holds:
`the_prompt_to_cursor_distance_holds_across_every_resize`,
`a_block_never_moves_once_it_is_drawn` and
`a_gesture_leaves_one_prompt_on_screen` pass on every recorded gesture.

**What a drag step costs the core**, `winch <gesture> cost 5`, the mean
resize per step over the gesture, base (master) against the change, rotated,
four rounds, the first dropped, on an Apple M5 Max on mains:

| gesture | steps | base, us | change, us |
|---|---|---|---|
| `drag` (at a prompt) | 331 | 2.67-3.76 | 7.04-7.73 |
| `live-drag` (at a prompt) | 519 | 5.11-5.59 | 9.99-10.21 |
| `full-ring` (no marks: re-cut on both) | 83 | 335.79-352.82 | 337.73-355.41 |

The ranges at a prompt do not overlap: a step that re-cut nothing re-cuts
the ring now, 4-5 us more on these gestures. `full-ring` re-cuts on both arms
and its ranges overlap, so the unchanged path is unchanged; it is also the
bound on a step at a prompt over a full ring, which re-cuts the same rows.
A resize is not on the per-byte path: no write or parse function changed.
`ingest2 <payload> 300` under `/usr/bin/time -l`, instructions retired,
the same four rotated rounds, the first dropped: `cat` 2,135.71-2,143.02 M
base, 2,135.62-2,137.14 M change; `btop` 5,150.39-5,150.63 M base,
5,150.02-5,151.11 M change. The ranges overlap: no result, as expected.

**Write amplification and working set.** Per ingested byte, nothing
changed. Per resize at a prompt, the re-cut writes every retained cell once,
as a re-cut in the output zone always did.

## The title strip in the title bar, and the grid's top in full screen

**What changed.** baud's lockup and activity lights were views in its own
content, placed by hand, so in full screen they stayed in the window
while the title bar left it: the lockup sat indented for traffic lights
that were gone, the terminal kept a title bar's height clear above the
grid, and on reveal the system's bar slid down over that strip with only
the buttons in it. Now the lockup is a leading titlebar accessory and the
lights a trailing one (`macos-liquid-glass` 1.0.0-beta.8's
`add_titlebar_accessory`; `baud-app/src/strip.rs`), in the traffic
lights' own view, which AppKit detaches from the window in full screen;
and the terminal's view takes the window's whole height once `NSWindowDidEnterFullScreenNotification` is posted and the
title bar's height off it again at `NSWindowDidExitFullScreenNotification`
(`baud-present-macos/src/full_screen.rs`).

**Where things are, windowed**, in `baud-app/tests/titlebar_mark.rs`'s
never-shown windows, base (master) against the change, at 786x572,
960x600, 1200x900 and 200x100 points, both bodies:

| | base | change |
|---|---|---|
| title bar | 28 pt | 28 pt |
| traffic lights | 14x16 at x 9, 29, 49 | the same |
| lockup | x 69, 51.37x13.78, centred 14 pt below the top | the same, x now AppKit's |
| lights may start at | x 129.37 | x 129.37 |
| lights end | 9 pt in from the right | 9 pt in from the right |
| lights' accessory at 960 / 200 pt | (none) | x 766.5, 193.5 wide / x 125, 75 wide |

The lights' positions equal the hand-placed ones at 960, 786, 800.25 and
200 points and scales 1 and 2 (`the_lights_accessory_keeps_the_lights_where_the_strip_had_them`).
Full screen itself is not measured: AppKit refuses the full-screen state
outside a real transition, which takes the display. The harness posts the
two notifications by hand to baud's observer instead.

**The per-byte path.** No write or parse function changed; `ingest2`'s
`__TEXT,__text` is the same size on both arms, but differs in branch
offsets because the window crate's version changed. `ingest2 <payload>
300` under `/usr/bin/time -l`, instructions retired, four rotated rounds,
the first dropped: `cat` 2,136.01-2,136.17 M base, 2,136.06-2,136.13 M
change; `btop` 5,149.36-5,149.60 M base, 5,149.42-5,149.61 M change. The
ranges overlap: no result, as expected.

**Write amplification and working set.** Per ingested byte, nothing
changed. A full-screen transition lays the grid out twice, once at the
transition's resize and once at the notification.

## The launch logo and the prompt's frame (modelled)

Not timed: a replay through `frame_gate`, the pump's frame gate, on a
virtual clock (`app::gate_tests`). A 469-byte prompt in one read lands on
ticks 3, 10, 40 and 70 and half a tick after each, at 120 Hz with the
ticks 0, +0.6, -0.4 and +0.2 ms off their vsyncs, each frame's completion
2 ms after it is drawn and a frame on screen at the first vsync after its
completion. Arms: the logo off, waiting for a window it never sees, and
playing from tick 0, 5, 30 and 60.

| read lands | logo off or waiting | logo playing |
|---|---|---|
| on a tick | drawn on its wake | drawn on its wake, the same vsync |
| between ticks | drawn on its wake | drawn on the next tick, one vsync later |

Between ticks the logo costs one vsync, 8.3 ms at 120 Hz. Every frame
sets the last commit, and while the logo draws one a tick a wake finds
it younger than the interval and leaves the read to the tick: the flood
rule every timed
effect shares, a trail, a transition or the scrollbar's fade among them.
Timing the rule from the last frame that drew a read instead would add
frames to output under any animation; unmeasured, and not changed here.
With the tick's own rule dropped from the gate, a tick the display link
fires early leaves the read past the next tick in the playing arms,
which the check reports.

## What the Nix package ships

`nix build .#default` on aarch64-darwin, macOS 27.0: `Applications/baud.app`
with `bin/baud` a link to its executable. Its Info.plist is the one
`scripts/bundle-macos.sh --info-plist` prints, its icon is the committed
`packaging/macos/baud.icns` byte for byte, the fonts' licences are 31
files, as many as `share/doc/baud/fonts` holds, and the binary holds the committed
`baud.metallib`'s 220,196 bytes. The fixup strips `bin` before the bundle
is assembled, and rcodesign, built by nixpkgs, signs the bundle ad hoc
last: `codesign -v` reads the bundle and its executable valid and
satisfying their designated requirement. The Linux arm, a desktop entry
and the icon under `share`, evaluates (`nix flake check --no-build
--all-systems`) and its entry passes `desktop-file-validate` 0.28; no
Linux build was run.

## The light logo in the light cursor's amber

Not timed: no byte path changed. The light lockups and marks, the title
bar's light lockup, the launch logo on a light window and the README's
light header take `#986100`, `BaudCursor::light`'s fill, in place of
`#C8801F`; every dark file is byte for byte as before.

| | `#C8801F` | `#986100` |
|---|---|---|
| against the light page `#EFEFEF` | 2.78:1 | 4.52:1 |
| `logo-light.gif`, bytes | 2,646,691 | 2,646,127 |
| `logo-light.svg`, bytes | 1,102,717 | 1,100,085 |
| GIF census: flashes general / red, area, strict | 9 / 0, 761.25, 2,304.5 | 9 / 0, 761.25, 2,304.5 |
| SVG census: area, strict | 763.2, 2,336.8 | 762.0, 2,337.8 |
| the word's worst contrast, light | 12.61:1 | 12.61:1 |

Both films pass WCAG 2.3.1 by the area, of 21,824, as before
(the film's driver, now `baud-pictures logo`, its census and
`readme_logo_flashes`); the
word's ink is unchanged, so its contrast is. The light half's haze is
0.0% of the page's pixels, at most 10 levels.

## The bidirectional tables compiled at build time

First carried by "build(domain): drop the python bidi generator and its
table". `build/bidi.rs` compiles BIDI, MIRROR and BOX_COUNTERPART into
`OUT_DIR` through `baud-tablegen` from the five pinned UCD 17.0.0 files
in `baud-domain/data/`; nothing generated is committed. Before the
delete, a unit test held every compiled static to the committed table and
printed "identical: BIDI_INDEX 4352, BIDI_BLOCKS 31232, MIRROR_INDEX 4352,
MIRROR_BLOCKS 3840, MIRROR_GLYPHS 429, BOX_COUNTERPART 128 elements"; the
Python's own check passed on that table, so the build computes what the
generator wrote. `contrast.bin`, moved to `build/contrast.rs`, is the
base's byte for byte.

**Release binaries, at one path, are not byte-identical, and the code is.**
A crate's first build-dependency changes its crate hash, which renames
every symbol downstream, and LTO then orders `__text` differently: the
same section sizes, and 627,908 of `ingest2`'s 1,225,216 text bytes differ.
A control arm -- base's `baud-domain/src` and `build.rs` under the
change's manifests -- gives an `ingest2` byte-identical to the change's.
For `baud`, the control differs from the change in 24 bytes of
`baud_render::rebreak::rebreak`, a register swap at the same size that
`#[repr(u8)]` on `BidiClass` causes -- the control with it added matches
-- and is not timed, and in 5 bytes of panic line numbers from
`bidi.rs`. A/A identical. The next crate to gain its first build script
shows the same rename; the control arm reads through it.

`ingest2`, the three arms rotated over six rounds, the first dropped,
`cells_written` equal on every arm, MB/s median [range]:

| payload, reps | base | change | change A/A |
|---|---|---|---|
| cat, 400 | 1521.8 [1429.2-1587.0] | 1523.6 [1423.2-1584.0] | 1512.1 [1405.5-1584.0] |
| cat-cjk, 400 | 507.3 [505.3-510.9] | 508.4 [503.7-511.4] | 507.5 [502.1-512.3] |
| btop, 400 | 861.9 [860.4-865.2] | 864.5 [853.3-868.2] | 858.1 [855.5-864.2] |
| vim, 1000 | 588.4 [578.1-590.7] | 583.9 [579.3-585.7] | 587.0 [580.2-593.2] |
| less, 2000 | 1374.5 [1347.7-1385.5] | 1377.2 [1366.3-1385.5] | 1371.8 [1345.0-1385.5] |

Every range overlaps: no result, which is what `-align-all-functions=6` is
for. Load average 5.8-6.8 from other work throughout.

**The build.** `cargo build -p baud-domain --timings`, rounds 2-5 of five
rotated: the build script compiles in [0.55-1.10] s against base's
[0.25-0.34] and runs in [0.17-0.26] s against [0.04-0.09];
`baud-tablegen` compiles in [0.14-0.29] s; the lib's ranges overlap.
Touching `src/lib.rs` ran no build script in any round on either arm,
and touching a `data/` file ran it once in each. At opt-level 3,
`baud-tablegen` (the package override reaches the build-dependency, `-v`
shows `-C opt-level=3`) runs the script in [0.08-0.10] s dev and
[0.19-0.22] s release but compiles in [0.27-0.41] s; compile and run
together overlap 0's in both profiles, so the default stays. The `.crate`s:
`baud-domain` 1,127,175 to 1,502,269 bytes with `data/`, 14.3% of
crates.io's 10 MiB; `baud-tablegen` 15,864.

## The audit and three gates in `cargo xtask`

Not timed: no byte path changed. `xtask` is a new unpublished member
reached through `.cargo/config.toml`'s alias. `baud`'s resolved graph
(`cargo tree -p baud --target all -e normal,build`) is the same 58
lines before and after, and `Cargo.lock` only gains packages.
`audit_all.sh` is `exec cargo xtask audit`, `check-changelog.sh` pipes
into `cargo xtask changelog`, and `build-metallib.sh` calls `cargo xtask
fnv1a`; the two Python audits and the two inline scripts are gone. Each Rust arm was run beside the Python it replaces,
stdout and exit code compared:

| gate | inputs | equal |
|---|---|---|
| `impl-audit <file>` | 978 tracked `.rs` files and xtask's 11 | all |
| `audit` | the tree, and 62 perturbations: one per budget row and sum, per empty root, per control | all |
| `scan-audit` | the tree, limits 5, 4 and none, and 7 perturbations | all |
| `changelog` | the history, 1,713 commits; 30 commits; bad subjects, no catch-all, a commit after the context, none | all |
| `fnv1a` | `shaders.metal`, `baud.metallib` | byte-equal, and equal to the committed `.fnv` |

The Python arm of `audit` was the shell with the two xtask rows added,
so the new rows were compared too. `impl_audit.py` had nine categories,
not ten: its `prim` pattern was never used. The `\w`, `\s` and `\b`
classes differ between the two engines (marks, connectors and the
other numerics), and no file of the tree reaches the difference.

The budgets are `xtask/src/audit.rs`'s const tables, with the reasons
that were the shell's comments; `xtask` itself is held at 5 and 0, all
five the cast and index category names matching themselves, and
`xtask/tests` at 0. Its tests drive one perturbation per row from the
tables; each of eight mutations of a check (the signature sum, the
bool pattern, leg 2, the `u8` cast, the scan matcher, `mindepth`, the
address count, the cross control) turned its test red.

Two behaviours changed, neither in a count. The file lists sort in byte
order, which `sort` gave here and glibc's locale collation does not, so
on Linux the per-file lines may come in another order than before. The
changelog's parsers are compiled with `regex`, the engine release-plz
and git-cliff run them with, where Python's `re` read them before.

## The bitmap and picture fixtures written by their tests

Not timed: no source under any `src/` changed, and the one manifest
change is a dev-dependency, `miniz_oxide` 0.9.1 for baud-font's tests,
already in the lockfile. `ingest2` built at one path from base, change
and an A/A copy of the change: all three binaries are byte for byte
the same, 2,716,880 bytes.

Each Python generator was run on the tree before it was deleted, and
each Rust generator that replaces it writes the committed bytes:

| fixture | Rust | streams spliced / literal |
|---|---|---|
| `sbix-strikes.ttf` | `bitmap.rs`, `sbix_font` | 33 / 7 |
| `cblc-cases.ttf` | `cblc.rs`, `cblc_font` | 66 / 0 |
| `tests/png/`, 120 files | `bitmap.rs`, `adam7_files` | 120 / 0 |
| `NotoColorEmoji-subset.ttf` | `noto_subset.rs`, ignored | none |
| `scene.ppm`, `scene16.ppm`, `edge16.pgm` | `jpeg.rs` | none |

The zlib streams are the one thing not regenerated. All 219 valid ones
are taken from the committed files by what they inflate to. The
generator asks for the stream of the rows it computes, so a changed row
finds none. The 7 corrupt streams are written as the literal
`78 9c ff ff ff ff`. The Adam7 pass starts that test the prior reset
number 221, as the Python printed. The subsetter, given the pinned
Noto (sha256 `72a635cb...`, fetched from its URL), writes 51,080 bytes
equal to the committed subset.

`key_tables` in `coverage.rs` replaces `extract-keys.py`. It still
counts 10 tables and 112 key rows, its listing equals the Python's
line for line (133 lines), and both agree on three perturbed copies of
`ctlseqs`: a header deleted (9 tables, 108 rows), a row's first bar
deleted, and U+001C/U+001F as whitespace.

Each check fired on these perturbations, run by hand and then
reverted: sbix's white, a `post` field, CBLC's purple, an Adam7 pass
start's filter, the scene's blue, edge16's seed, the 16-bit dither,
and the subsetter's format-3 padding. The committed controls: sbix,
CBLC and Adam7 each have one generated row changed in its last byte and
one header byte moved; the JPEG test flips a byte of each picture; the
key extraction has the lost header and the U+001C separator. The
subsetter has none, since its source is not committed.


## The grapheme, width and line-break tables compiled at build time

First carried by "build(domain): drop the python unicode generators and
tables". `build/grapheme.rs`, `build/width.rs` and `build/linebreak.rs`
compile BREAK and SPACING_EXTEND, WIDTH and WIDTH8, and LINE into
`OUT_DIR` through `baud-tablegen` from eight more pinned files in
`baud-domain/data/`; `build/ucd.rs` holds what they read alike. The three
Python generators printed OK on the base. Before the delete,
"build(domain): compile grapheme, width and line break tables" held every
compiled static to the committed table and printed "identical:
BREAK_INDEX 4352, BREAK_BLOCKS 28416, SPACING_EXTEND 46, WIDTH_INDEX
4352, WIDTH_BLOCKS 7360, WIDTH8_INDEX 4352, WIDTH8_BLOCKS 6592,
LINE_INDEX 4352, LINE_BLOCKS 36864 elements". Applying each EAW header's
default-W ranges to 17.0.0 changes nothing there: that file lists them.

**Release binaries, at one path, are byte-identical.** `baud` and
`ingest2`, base and change and a clean A/A rebuild of the change, have one
sha256 each (00003eea... and d102d223...), the change arm built with no
`*_data.rs` of these three in its source and all three in `OUT_DIR`.
`baud-domain` already had a build-dependency, so no crate hash moved, and
`Width::of`, `of_with_lookback` and `LineProps::of` read the same bytes;
no timing run, as the verification rule allows.

**The build.** `cargo build -p baud-domain --timings`, `baud-domain` and
`baud-tablegen` cleaned each round, five rounds rotated, the first
dropped: the build script runs in [0.32-0.38] s against base's
[0.18-0.23] and compiles in [0.59-0.72] s against [0.55-0.57]; the lib's
ranges overlap ([9.36-14.02] s against [9.50-10.13]); `baud-tablegen`
[0.14-0.15] s in both. Touching `src/lib.rs` ran no build script in any
round on either arm, and touching a `data/` file ran it once in each.
Load average 11-18 from other work throughout. The `baud-domain` .crate
is 1,978,292 bytes, up from 1,502,269, 18.9% of crates.io's 10 MiB.

**Red first.** Built and run: width's order turned to wide over zero is a
build error at U+302A; Prepended_Concatenation_Mark not left out is one
at U+0600; Halfwidth dropped from line breaking's East Asian set passes
every control, and the identity test (LINE_BLOCKS[7593]) and
`every_code_point_has_the_line_class_the_pinned_files_give` go red;
InCB Linker and Consonant swapped passes the build's controls, which
read the same constants, and turns the identity test, `build_tables.rs`
and the grapheme oracle red. Each oracle was seen red on a
perturbation of its own reader.

## The source gate and THIRD_PARTY.md in `cargo xtask`

Not timed: no byte path changed. `cargo xtask sources` replaces
`check-sources.sh`'s awk and two inline scripts, with `check-page-text.py`
as `xtask::page_text` and `gen-third-party.py` as `cargo xtask
third-party [--check]`; the shell now execs it. xtask builds at opt-level
1 in `dev`: at 0 its SHA-256 held the gate at 2.93-2.94 s, at 1 it took
0.25-0.26 s, against the base shell's 0.50-0.51 s (four rounds, the
first dropped).

| arm | inputs | result |
|---|---|---|
| article text | the 57 pages saved on 2026-11-01 | 57 of 57 hash as pinned; `--from`, `--print` and `--self-test` output equal to the Python's |
| tokeniser | 80,000 generated documents, seeds 1-3, events against `html.parser` | all equal; 16 fixed cases committed |
| `sources` | the tree, and 27 perturbations plus 29 licence ones through it | 71 of 85 equal in output and exit; 14 below |
| `third-party` | the tree, and 29 perturbations | THIRD_PARTY.md byte-equal; every NOTICE unchanged |

The named references are the HTML Standard's list, now pinned as
`whatwg-html-entities.json`: its 2,231 names and characters are exactly
Python 3.13's `html.entities.html5`. The manifest gains `derived_from`,
refused when it names an absent, gated or own key; a TOML 1.0 scan
refuses what `toml` 1.1.8 reads and `tomllib` refused (an inline table
across lines, a comment or trailing comma in one, `\e`, `\x`, a time
with no seconds, a leading byte order mark), each probed on both
parsers.

The 14 departures, in two classes, are accepted: every exit code is
equal, and the seven Python crashes become failure lines.

- **Invalid TOML (7).** The line gives the `toml` crate's reason, or the
  scan's refusal, where tomllib's stood, at the line and column tomllib
  named where the `toml` crate refuses (772, 16 for the fixture's
  unescaped quote, 42631, 19 for one in the manifest); the 1.0 scan
  names the column where the TOML 1.1 form starts, up to two columns
  from tomllib's.
- **Render's KeyError (7).** For a group naming a file the manifest does
  not hold, or a status the sidecar does not define, the Python stopped
  rendering on a KeyError and printed nothing; the Rust prints the line.

Three cases came after the 85 were compared: a byte order mark on the
manifest, and on the sidecar through both gates. tomllib refuses a
leading mark with "Invalid statement (at line 1, column 1)"; the scan
refuses at the same place and names the mark.

Keys and citations sort in byte order, where `sort -u` used the locale,
which is accepted: only failure lines can come in another order.
`page-text` fetches through `curl`, whose error text replaces urllib's.

Each perturbation is a `#[test]` asserting its line; eight mutations
(the length and empty-hash checks, a duplicate group, `\x`, the citation
floor, self-derivation, the in-file URI, the quote backtrack) each
turned a test red. A 32-to-31 reference window did not: no name on the
pinned list is longer than 31 characters without its semicolon.
`check-publish.py` keeps its own copy of the status meanings and licence
URIs, which a test holds equal to xtask's until it is ported.


## baud-font's oracles in `tools/baud-oracle`

Not timed: no byte path changed, and nothing in the workspace links the
new crate. `tools/` is its own workspace with its own `Cargo.lock`,
excluded from the root's, so `cargo test --workspace` never builds a C
engine. `baud-oracle` calls HarfBuzz and FreeType through first-party
`extern "C"` (`tools/baud-oracle/src/hb.rs`, `tools/baud-oracle/src/ft.rs`).
A C probe compiled against the headers holds each struct's size, the
offset of every field the bindings read, the constants, the version
macros and FreeType's 95 error texts to them.

The engines: none of nixos-25.11 (HarfBuzz 12.1.0, FreeType 2.13.3),
nixos-26.05 (13.2.1, 2.14.3) or nixpkgs-unstable (14.5.1, 2.14.3) has the
pair; nixpkgs `21622bac70c2`, the commit that took FreeType to 2.14.3,
has both, and `tools/shell.nix` pins it. HarfBuzz's
check phase fails there on one CoreText shaping test against this host's
CoreText, so it is skipped. The Python arm was nixos-26.05's uharfbuzz
0.53.2, which builds its own vendored 12.3.0, and nixpkgs' 12.3.0 carries
a cmap patch for CVE-2026-22693: two builds of one version, so their
agreement below is measured, not assumed. Every bin refuses to write a
line unless the linked versions are 12.3.0 and 2.14.3.

| oracle | committed rows | on its perturbation | rows |
|---|---|---|---|
| `advance` | 13,252 equal | first hmtx advance of `gvar-cases.ttf` +37 | 13,252 equal |
| `bearing` | 180 equal | `bearing-cases-bit1.ttf` swapped for JetBrains Mono | 15,768 equal |
| `cff2` | 1,360 equal | `cff2-cases.otf` swapped for JetBrains Mono (quadratics) | 42,952 equal |
| `component` | 264 equal | `component-cases.ttf` swapped for JetBrains Mono | 13,936 equal |
| `gvar` | 1,098 equal | `gvar-cases.ttf` swapped for Source Code VF (cubics) | 71,208 equal |
| `marks` | 11,722 equal | `ink-cases.ttf` swapped for Fira Code | 11,722 equal |
| `var` | 12,003 equal | `FiraCode-VF.ttf` swapped for Source Code VF | 12,003 equal |
| `vertical` | 126 equal | `post` version 3 in `vertical-cases.ttf` (no names) | 126 equal |

"Equal" is every data line byte for byte, Python's against Rust's, both
exiting 0; the perturbations reach branches the committed fonts do not
(a bare `C`, `Q`, a `None` name). `tools/baud-oracle/tests/regenerate.rs`
keeps the identity and one perturbation per oracle that must fail
`--check`.

Red first: the first `cff2` run differed at data line 225, `...07813`
for Python's `...07812`. Where two shortest strings tie, Rust's `{:e}`
rounds up and CPython rounds to even, so `repr` now rounds to the shortest
length with `{:.*e}` (`tools/baud-oracle/src/pyfmt.rs`). Three mutations
each turned a test red: back to `{:e}`'s digits (cff2, and the repr rows),
`x_offset` and `y_offset` swapped (the probe, and marks at line 17), and
the perturbations not written (the positive control).

The six ported scripts are deleted and their files regenerated: only the
headers changed, once, `uharfbuzz 0.53.2` to `tools/baud-oracle` and the
generator line to the bin, because a Rust regenerator cannot print the
Python binding's name; `bearings.txt`'s pp1 reader is `read-fonts 0.46.0`,
not fontTools. `scripts/gvar-oracle.py` and `scripts/cff2-oracle.py` stay
beside their ports with their headers unchanged: LICENSE names both as
the writers of `outlines.txt` and `cff2-outlines.txt`, and LICENSE is not
edited here, so their deletion waits on that edit. FreeType errors print
FreeType's own texts. freetype-py's table misquotes eight of them
(0x8F-0x9B). All eight are table-missing or table-format errors that
FT_New_Face raises, and there both arms stop before writing a row, so no
row of either arm can hold one.


## The font comparator, `tools/baud-fontbuild compare`

Not timed: no byte path changed; a self-compare of the built Regular takes
0.10 s, release. `compare A.ttf B.ttf` reads both through read-fonts
0.46.0 and expands each table to lines (`tools/baud-fontbuild/src/`):
glyph order and post names; every cmap record, format and mapping; each
glyph's points, flags, instructions, components and bbox; each gvar
tuple's region and every point's delta, unreferenced ones inferred as the
OpenType gvar chapter, revision `810414e1`, infers them
(`[[source: ms-opentype-gvar]]`); hmtx and HVAR per glyph; head, hhea,
maxp, OS/2, post, name, gasp, cvt, fpgm, prep, fvar, avar and STAT field
by field (`checkSumAdjustment` excepted); GDEF, GSUB and GPOS as a
feature graph, feature parameters included, and per-lookup first-match
mappings, so a subtable split is not a difference. Devices resolve at -1,
0, 1 and every region coordinate of either font: on the built fonts' one
axis, wdth, that is -1, 0 and 1, between which a delta is linear. It
prints the first difference per table and exits 0 equal, 1 different, 2
unreadable.

All 57 committed fonts equal themselves; the four built ones expand every
table with none left as bytes. The seven perturbations of the built
Regular, byte edits with checksums recomputed (`tests/compare.rs`), each
name one table:

| edit | report |
|---|---|
| last x of `y` +1 | `glyf: glyph 'y' (gid 368) points: p13=272,-180,on vs p13=273,...` |
| first mark anchor x +1 | `GPOS: glyph 'uni030C.alt' (gid 1645) lookup 0 attachments: 96=(-175,550);...` |
| GSUB lookup 0 IgnoreMarks | `GSUB: lookup 0: flag=0x0000 vs flag=0x0008` |
| format 4 endCode -1 | `cmap: record 0 map: U+0039>733 vs U+003A>811` |
| a named point's x delta +1 | `gvar: glyph 'uni0918' (gid 1932) tuple 0 deltas: p8=-63.0973... vs p8=-62.6194...` |
| fontRevision low bit | `head: fontRevision: 2.3040008544921875 vs 2.303985595703125` |
| family name's first char +1 | `name: platform=3 encoding=1 language=0x0409 nameID=1: Proto vs Qroto` |

p8 is a point no tuple names: the edit reached it only through inference.
An eighth, ss01's UI name ID +1, reads `GSUB: script DFLT default:
ss01[436]+ss(version=0,uiName=256) vs ...257)`. A copy with only
`checkSumAdjustment` changed is equal, and recomputing the checksums of
each built font leaves its bytes as they are.

Red first: `compare` stubbed to "equal" turned 9 of 15 tests red, the
eight edits and the exit codes. Three mechanism perturbations: inference
off (unreferenced points 0) turned three red, the spec's own example (P2's
x delta 10.5), the case test and the inferred-point test, which then names
p9, the edited point; gvar deltas blanked turned the gvar edit red;
feature parameters compared by presence turned the ss01 edit red.

Compared as bytes, and said so: CFF, CFF2, sbix, CBDT and the other
tables not listed, avar 2's data, and the 24-bit-glyph-id formats. Also
not modelled: a cmap mapping to glyph 0 counts as unmapped, a cursive
lookup is compared glyph by glyph, and with more than one axis only the
samples are compared. Shaping and raster differentials are not built here.

## The publish gate in `cargo xtask`

Not timed: no byte path changed. `cargo xtask publish [--size|--verify|
--release|--write-notices|--write-licences]` replaces `check-publish.py`
with its thirteen checks, and `test-publish-fresh.py` becomes
`each_verify_reads_its_own_unpack_and_leaves_none`; both scripts are
deleted. The `.crate` is read by a ustar reader over `baud-domain`'s
inflater, fed a zlib header in place of the gzip one and the Adler-32 of
what it wrote as the trailer, with the gzip CRC-32 and length checked
here. The status meanings and licence URIs are `third_party`'s own, so
the copy and the test that held it equal are gone.

| arm | inputs | result |
|---|---|---|
| the tree | plain, `--release`, `--size`, `--verify` | stdout and exit equal to the Python's; every unpack removed |
| NOTICE | all 13 emptied, then `--write-notices` | each byte-equal to the committed file |
| `license` | all 13 set to `MIT`, then `--write-licences` | each manifest byte-equal |
| the tree, perturbed | 7: a keyword, a licence, a NOTICE date, an unversioned path dependency, a link to /etc/hosts, a NOTICE edit, an unpublished member published | stdout and exit equal |
| a ten-crate fixture | 44 trees, each under no flag, `--release`, `--write-notices` and `--write-licences` | 176 of 176 equal in stdout, exit and the files left |
| `textwrap.fill` | 86 cases against Python 3.13 | all equal |

Two departures are accepted, both in text and neither in an exit code:
the three hints read `run cargo xtask publish --write-...` where they
named the script, and a TOML error is the `toml` crate's line on stderr,
where tomllib's traceback stood, exit 1 on both. The published set is 13
crates, `baud-tablegen` among them, reached as a build-dependency.

What the port keeps, each pinned by a test: Python's `$` matches before
a final newline, so a keyword `"terminal\n"` passes; `\s` holds for
`\x1c`-`\x1f`; a link is resolved as `realpath` does, without failing
on a dangling one; sizes round half to even (131,072 bytes is "0.12");
`[profile.release]` compares without order, with `True == 1` and an
int equal to a float only when exactly so; the
wrap drops leading space on every line but the first, strips U+2003 as
space, keeps a no-break space inside its word and expands tabs to
eight columns. `\b` is the `regex` crate's Unicode boundary, whose `\w`
takes combining marks and leaves out No-category numerals, where
Python's does the reverse: `decided` followed by U+0301 is found by
Python and missed here, and `²2026-01-01` is missed by Python and found
here. This departure is open, and no NOTICE reaches it. The root is the
directory the command runs in, canonicalised, as for every xtask
command. The script took its own folder's parent unresolved, so through
a symlinked checkout its relpath of a linked file could start with `..`.

Each check is a `#[test]` on a perturbation, and 27 mutations, one per
check and per kept rule, each turned a named test red. The stale unpack
runs `package` twice in one build folder with the unpack folder made
between; the oversize crate holds 10.5 MiB of xorshift bytes.


## The caseless, search, sequence, joining and ignorable tables compiled at build time

Caseless, search, sequence and joining compile into `baud-domain`'s
`OUT_DIR` from pinned `data/`, and `baud-font` gains its first build
script (presentation forms and default-ignorable); the sequence edges are
placed by `baud_tablegen::place`, seeds from 0 up as the generator tried
them. Before the delete, the identity commit printed "identical:
CASELESS_INDEX 4352, CASELESS_BLOCKS 6912, CASELESS_POOL 3136,
SEARCH_INDEX 8704, SEARCH_BLOCKS 18560, SEARCH_PROPS 3136, SEARCH_POOL
7687, NODE_SPAN 4688, SPANS 10495, SEEDS 2048, EDGES 8192, CONTINUES 544,
JOINING_INDEX 4352, JOINING_BLOCKS 20224, DEFAULT_IGNORABLE 17 elements"
and, in `baud-font`, "identical: DEFAULT_IGNORABLE 17, FORMS 242,
LAM_ALEF 4 elements".

**Release binaries differ from base by the crate-hash rename only.**
`baud-font`'s first build-dependency renames its symbols, so `baud` and
`ingest2` differ from base; `__text` is 4,033,984 and 1,225,216 bytes in
both. A control arm (base's sources under the change's `baud-font`
manifest, `build.rs`, `build/`, `data/` and `baud-tablegen`) gives
binaries byte-identical to the change's, and so does a clean A/A rebuild
of the change. No timing run.

**The build.** `cargo build -p baud-font --timings`, both arms extracted
at one path, five rounds rotated, the first dropped: `baud-domain`'s
build script runs in [0.68-0.86] s against base's [0.37-0.39] and
compiles in [0.74-0.99] s against [0.57-0.90]; `baud-font`'s, new, runs
in [0.14-0.19] s and compiles in [0.34-0.46] s; the libs' ranges
overlap. Touching `src/lib.rs` ran no build script in any round on
either arm, and touching a `data/` file ran its crate's script once in
each. Load average 7-23 from other work throughout. The `.crate`s:
`baud-domain` 1,978,083 to 1,968,107 bytes; `baud-font` 282,227 to
812,097 with `data/`, 7.7% of crates.io's 10 MiB; `baud-tablegen` 15,864
to 17,087.

**Red first.** Built and run: the sequence seeds tried from 1, as
`perfect_hash` tries them, place every edge and turn the identity test
(SEEDS[0]) and `the_sequence_seeds_are_tried_from_zero` red while
`named_sequences.rs`'s oracles stay green; one
Default_Ignorable_Code_Point row left unread passes the count and the
controls and turns both crates' identity tests and new default-ignorable
oracles red (3600 code points from U+E01F0); merged ranges ending a
point short is a `cargo::error` on `baud-font` (22 ranges, 17 expected);
FORMS sorted by form first turns `presentation.rs`'s oracle and the
identity test red. Every check `build_tables.rs` names in both crates is
seen to fire.


## The xkb, raster and shape-fixture oracles in `tools/baud-oracle`

Not timed: no byte path changed, and nothing in the workspace links the
new code. `baud-oracle xkb`, `raster` and `shape-helpers` replace the
deleted `xkb-oracle.py` and `raster-oracle.py` and the 25 inline Python
blocks of `scripts/gen-shape-fixtures.sh`; libxkbcommon is
declared as `extern "C"` (`tools/baud-oracle/src/xkb.rs`), its enums and
the new FreeType fields held to the headers by the C probe (80 values).

The engines, each the version the fixtures name: libxkbcommon 1.13.2
from nixpkgs `13e53762` (flake.lock's revision; L6's `21622bac` has
1.13.1) in `tools/shell.nix`, and hb-shape from `tools/hb-shape.nix`:
14.5.1 at `13e53762`, the very derivation the script names
(`vxihlv2p...-harfbuzz-14.5.1.drv`), and 13.2.1 at nixos-26.05 `7dcdd873`,
whose tarball, patch and meson flags equal the build that made the
13.2.1 files. libxkbcommon has no version call, so `build.rs` passes
`pkg-config`'s and `xkb` refuses any other.

| oracle | identity | side by side, perturbed |
|---|---|---|
| `xkb` | 16 fixtures byte-equal (4 keymaps, 12 Compose tables) | `a` made `b` in the US keymap; `g1` made `g2` in edge.Compose; a missing file: equal output and exit |
| `raster` | 21 faces, 47,228,225 bytes equal to freetype-py on FreeType 2.14.3 | sizes 13.5, 7.25, 100, 0.5 and `1e1`; a bitmap-only face; a bad path, index and integer: equal, exit 0 or 1 |
| `shape-helpers` | all 17 hb-shape fixtures byte-equal, fnv64 column included, each under its header's version, a `python3` that exits 99 first on PATH and never called | morx defaults on every face a recursive walk of /System/Library/Fonts finds on this Mac, 786 (17 non-trivial, 547 with no `morx`) and two synthetic fonts, one refused: equal text and exit |

Before the port, the Python script reproduced every fixture too under
the same two pins, so the base arm is measured, not assumed. The
Compose environment was never recorded: XLOCALEDIR=`compose/xlocale`,
HOME=`compose/home` (unset for `home-unset`), locale `xx` for the small
files, found by regenerating all twelve byte-equal; `xkb --check` keeps
that table. The Wikisource pages are fetched in bash with curl under
the old User-Agent and hash pins; neither holds `&`, so the helper
refuses a character reference rather than port `html.unescape`. Python's
`\s` is held to CPython's 29 code points, U+001C-U+001F among them.

`raster.rs` reads `BAUD_FREETYPE_ORACLE=<bin>`: unset it says so and
passes; set, 164,184 glyphs pass; a bad path fails. Eight mutations each
turned a test red: layout and level swapped, HOME set for `home-unset`,
left and top swapped, the size rounded down, the FNV basis off by one,
Rust's whitespace for Python's, the off selectors not requested, and
`bitmap_left` and `bitmap_top` swapped in the struct. `bake-oracle.py`
is retired: its TTFs stay as fontTools 4.63.0 observations, the recipe
in `baud-font/tests/bake.rs`.

## The colour-name and placeholder tables compiled at build time

`build/colour_names.rs` compiles kitty's and the X server's colour names
into `baud-domain`'s `OUT_DIR` from `data/` links to the two pinned
rgb.txt files, through `baud_tablegen::perfect_hash`, and
`build/placeholder.rs` the placeholder diacritics, pinned as
`kitty-placeholder-diacritics` (`derived_from` kitty's external file).
kitty's two key tables are pinned the same way and only tests read them.
A slot is `ColourSlot`, `repr(C)`, held to six bytes, an alignment of two
and the tuple's offsets. Before the delete, the identity commit printed
"identical: kitty SEEDS 256, kitty SLOTS 1024, kitty POOL 6601, x SEEDS
256, x SLOTS 1024, x POOL 6832, DIACRITICS 297, DIACRITIC_INDEX 1866,
DIACRITIC_BLOCKS 2240 elements". Two spellings differing only in case
with one colour are one name; one spelling twice, or two colours, is a
build error. Neither pin has such a pair.

**Release binaries.** Built at one path from fresh extractions, every
file touched: `ingest2` is byte-identical across base, change and A/A,
and is not vacuous, since `baud` from the same builds shows baud-domain
rebuilt. `baud`'s `__TEXT,__text`
(4,033,984 bytes) and `__TEXT,__const`, which holds the tables, are
base's byte for byte; the binary differs in 9 bytes of
`__DATA_CONST,__const`, each a panic location's line in `themefile.rs`
moved down 7 by its `mod colour_names` block, and in the UUID and code
signature over them. A/A matches the change. No timing run.

**Red first.** Seeds tried from 0, the pool in file order and the trie's
blocks renumbered by content each turn the identity test red (kitty
SEEDS[0], kitty SLOTS[0], DIACRITIC_INDEX[12]) while the oracles stay
green; the duplicate rule, the pool's bound and the 753 and 782 counts,
each turned off, turn `build_tables.rs` red; `debianred` at
215, 7, 80 turns `kitty_names_are_rgb_txt_and_debianred` and the
ignored `kitty_names_are_its_own_file` red; ESCAPE 28 and another
sha256 on line 1 of the key tables turn five key tests red. On kitty's
file at its pin the ignored test passes where the Python's `--kitty`
printed OK, and a copy with `debianred` changed fails both.

**The build.** `cargo build -p baud-domain --timings`, both arms at one
path, five rounds rotated, the first dropped: the build script compiles
in [0.78-3.69] s against base's [0.74-1.80], runs in [0.71-2.78] s
against [0.69-0.87], and the lib in [9.59-18.99] s against
[9.56-20.09]; every range overlaps, round 3 high on both arms, load
average 8.8-22 from other work. Touching `src/lib.rs` ran no build
script on either arm; touching a `data/` file, a new one included, ran
it once. The `.crate`: 1,968,038 bytes on base, 1,952,398 on the change.

## The compose, Indic, script and CFF Standard Encoding tables compiled at build time

`baud-font`'s build compiles four more tables from pinned `data/`:
canonical composition, the Indic classes, the script tables and the CFF
Standard Encoding. The last is read from
`adobe-5176-cff-standard-encoding.txt`, Appendix B's code and SID
columns pinned as text derived from `adobe-5176-cff`, which was read
once with `extract-adobe-cff.py`; MANIFEST.toml records how. Before the
delete, the identity commit printed "identical: COMPOSE 961,
DECOMPOSE 2081, CCC 403, DENSE 2304, LATIN1 512, SPARSE_BLOCKS 5, SPARSE
455, CODES 700, TAGS 1400, EXTENSION_SETS 2856, SCRIPT_INDEX 4352,
SCRIPT_BLOCKS 42240, EXTENSION_INDEX 4352, EXTENSION_BLOCKS 9984,
DIRECTIONS 175, RUN_INDEX 4352, RUN_BLOCKS 9216, BRACKET_INDEX 4352,
BRACKET_BLOCKS 3072, STANDARD_ENCODING 256 elements". Compose and the
Standard Encoding had no test before it.

**Release binaries are byte-identical.** `baud` and `ingest2` were built
from base and change at one path, with a clean A/A rebuild of the change;
all three `cmp` equal. `__text` is 4,033,984 and 1,225,216 bytes. No
crate-hash rename: `baud-font` already had its build-dependency. No
timing run.

**The build.** `cargo build -p baud-font --timings`, both arms extracted
at one path, five rounds rotated, the first dropped. `baud-font`'s build
script runs in [0.40-0.58] s against base's [0.14-0.22] and compiles in
[0.91-1.46] s against [0.35-0.47]. The lib compiles in [1.87-3.95] s
against [1.80-3.73], which overlap. Touching `src/lib.rs` ran no build
script on either arm, and touching `data/unicode-unicodedata.txt` ran it
once on each. Load average was 18-23 from other work. The `baud-font`
`.crate` went from 811,968 to 1,166,650 bytes, 11.1% of crates.io's
10 MiB. The largest file it gained is DerivedNormalizationProps.txt,
1,377,582 bytes before compression.

**Red first.** Each was built and run:
- HarfBuzz's override of U+0953 to A instead of SM passes every build
  check and turns the identity test red (DENSE[166]).
- Composites with their exclusion test removed are a `cargo::error`
  (1046 primary composites, 961 expected) and fail the U+0958 control.
- With the Standard Encoding's code count dropped, the test's
  dropped-row case goes red.

`build_tables.rs` holds 44 perturbed inputs across the four modules,
and each fires. They cover a header per module, every row-level assert,
KeyError and SystemExit of the generators, and at least one control and
one count in each module. Three checks have no case, because no one-row
edit reaches them: 256 or more scripts, more than 256 extension sets,
and the 37 right-to-left scripts.

## The stress mark's tables compiled at build time

`baud-render` gains its first build script: `build/stress.rs` compiles
the stress mark's skip trie, the separators and the combining trie into
`OUT_DIR` from pinned `data/` through `baud-tablegen`, and a `links` key
hands that `OUT_DIR` to `baud-present-macos`'s new build, which writes
the skip trie as `stress_skips.metal` and `shaders.metal` with it spliced
over its `#include`. The identity commit printed "identical:
STRESS_SKIP_INDEX 4352, STRESS_SKIP_BLOCKS 4704, COMBINING_INDEX 4352,
COMBINING_BLOCKS 2400, SEPARATORS 19 elements" and "identical: 56693
bytes of arrays".

**The `.metallib` is base's byte for byte.** `build-metallib.sh` finds
`OUT_DIR` in `cargo check --message-format=json` and compiles with `-I`
there; at one path both arms wrote 220,196 `cmp`-identical bytes, equal
to the committed library. The spliced text differs from base's in
comments, so only `shaders.metal.fnv` moves.

**Release binaries: the crate-hash rename and the shader's comments.**
`baud`'s `__text` is 4,033,984 bytes in every arm; `__TEXT,__const` is
232 bytes larger, the 226 the comments add to the runtime fallback's
source. Arm A, the identity commit (base's code, both build scripts),
against arm B, the change with base's comment lines put back (its
spliced text `cmp`-equal to base's `shaders.metal`): 147 bytes differ,
the UUID, the signature and 35 panic line numbers of `metal.rs`, +3. A
clean A/A rebuild is identical. No timing: the per-byte path is
untouched.

**The build.** `--timings`, five rounds rotated at one path, the first
dropped: `baud-render`'s script compiles in [0.31-0.34] s and runs in
[0.18-0.19] s, `baud-present-macos`'s in [0.23-0.25] and [0.04-0.06];
the libs overlap base's. Touching `src/lib.rs` ran no build script;
touching UnicodeData.txt ran each that reads it once. `baud-render`'s
`.crate` goes 0.63 to 0.99 MiB with `data/`.

**Red first.** U+2030 dropped from CSS's exceptions passes the build and
turns `the_stress_mark_skips_what_css_leaves_bare` red there; all
exceptions dropped, or bits packed high first, is a `cargo::error`; a
short shader block turns the table pin and the digest gate red; the
unspliced source fails at run time, "'stress_skips.metal' file not
found". Every case in `build_tables.rs` and `build_stress.rs` fires.
With the source digest now of the spliced text, `xtask/tests/fnv.rs`
holds only the library's digest, and `metallib.rs` holds the source's.

## The cases fonts in `tools/baud-oracle cases`

Not timed: no byte path changed. `baud-oracle cases <name>` builds the
bake, bearing, budget, cff2, colour-refused and component fonts from
their recipes on write-fonts 0.55.0 (`tools/baud-oracle/src/cases/`),
and the six Python generators are deleted. Before the port, the Python
under fontTools 4.63.0 rebuilt all eleven files it wrote byte for byte.
Four places are not write-fonts' own: OS/2 `version` is set to 3 over
the 4 write-fonts names for the same layout; budget's `full`, 65,536
points, is written by hand, since write-fonts numbers its last point as
`cur as u16 - 1`; the CFF2 table is laid out here, write-fonts having
only an INDEX helper; and bake's `intc`, `fracc` and `nestc` set
`WE_HAVE_INSTRUCTIONS` on their last component with a program of no
bytes, a uint16 0 after it, as fontTools wrote them. write-fonts writes
the bit and the length only for a program with bytes, which left no
committed font reaching the bake's read of an empty program: refusing a
flagged program of no bytes turns 10 of the 12 bake tests red on the
old font and on this one, and turned none on the port before it.

| font | old sha256 | new sha256 | tables that differ |
|---|---|---|---|
| `bearing-cases.ttf` | `60644011...` | `88de9199...` | glyf, loca, gvar |
| `bearing-cases-bit1.ttf` | `645cd787...` | `85063e7e...` | glyf, loca, gvar |
| `budget-cases.ttf` | `1aca1ae5...` | `e4919d09...` | glyf, loca |
| `colour-refused-cases.ttf` | `dee5a634...` | `8c7a416d...` | glyf, loca |
| `component-cases.ttf` | `700386d7...` | `946a3bb2...` | glyf, loca, gvar |
| `bake/cases.ttf` | `39a50667...` | `7145dafc...` | gvar, GDEF |
| `cff2-cases.otf` | `cfc8a58f...` | the same | none |

In the six fonts that changed, head differs only in
`checkSumAdjustment`; `cff2-cases.otf`'s is equal. In the five besides
bake, the glyf difference is that same flag: fontTools set it, with an
empty program, on all 47 of their composites, and write-fonts writes
neither; of baud's readers only the bake reads it, and none of the five
is baked. bake's GDEF holds the same glyph classes in both, {1: base,
3: base, 8: mark}, as a glyph ClassDef format 1 in the old font and a
format 2 of three ranges, glyphs 1, 3 and 8, in the new.

`baud-fontbuild compare` finds each old font equal to its new one, and
every gvar tuple references the same points in both, bake's two sparse
tuples included; COLR, CPAL, CFF2 and `full`'s glyph bytes are equal
as bytes. The first colour-refused draft read `OS/2: ulUnicodeRange2:
0x00002000 vs 0x00000000`, Geometric Shapes' bit, now set. All eight
oracles regenerated on the new fonts equal their files line for line,
but for the two headers that still name Python scripts. fontTools
4.63.0's instances of the new `bake/cases.ttf` equal the committed
`cases-<wght>.ttf` byte for byte, at all four weights, so the
observations stand; without the empty programs all four differed.

Three perturbations: OS/2 version left at 4 names `OS/2: version` in
all seven fonts; tri's apex moved one unit names `glyph 'tri' (gid 1)
points: p2=200,300,on vs p2=201,300,on` and turns `component --check`
red at data line 1; unreferenced points written as zeros names bake's
sq tuple 0 deltas. Each fails `cases --check`. read-fonts 0.46.0 reads
`full` as no points (`end + 1` in a u16), so the comparator sees only
its box and end point; its test decodes the glyf text by hand.

## The cluster and merged tables compiled at build time

`build/cluster.rs` compiles the cluster machine from a second
implementation of UAX #29's rules, GB1 to GB999 in the annex's order and
each legacy variant the rules less what section 3.1.1 excludes;
`build/merged.rs` compiles the merged table from the BREAK, WIDTH and
WIDTH8 tries as `of_with_lookback` and `Width::of` read them. Before the
delete, "build(domain): compile the cluster and merged tables at build
time" printed "identical: CLUSTER_READY 1, CLUSTER_STATES 94,
CLUSTER_NEXT 24064, CLUSTER_PARTS_LEGACY 3008, CLUSTER_PARTS_MARKS_JOINED
3008, MERGED_PALETTE 99, MERGED_INDEX 4352, MERGED_BLOCKS 65536,
BMP_DIRECT 131072, cluster.bin 30178, merged.bin 201071 elements", the
two `.bin` reassembled from the compiled statics in their layout.

**Release binaries.** `baud` and `ingest2`, built at one path from fresh
extractions with every file touched: change and a clean A/A rebuild are
byte-identical. Against base, `__TEXT,__text` (4,033,984 and 1,225,216
bytes), `__TEXT,__const` and `__cstring` are byte-identical and every
section keeps its size; each binary differs in two bytes of
`__DATA_CONST,__const`, two panic locations in `grapheme.rs` moved down
three lines (599 to 602, 621 to 624) by the `ClusterRules` doc, and in
the UUID and the code signature. No timing run.

**The build.** `cargo build -p baud-domain --timings`, both arms at one
path, five rounds rotated, the first dropped: the build script runs in
[0.79-1.67] s against base's [0.72-1.10], compiles in [0.96-2.20] s
against [0.80-1.45], and the lib in [10.78-22.11] s against
[9.28-18.51]; every range overlaps, load average 10-58 from other work.
Touching `src/lib.rs` ran no build script in any round on either arm,
and touching a `data/` file ran it once. The `baud-domain` .crate is
1.85 MiB compressed.

**Red first.** GB8 widened to join T V passes the build's controls and
turns `the_cluster_table_agrees_with_the_rules`, the identity test,
`the_parts_tables_agree_with_the_rules` and baud-core's
GraphemeBreakTest.txt runners red at lines 505 and 581; a parts bit
dropped after an odd regional indicator leaves the first green and turns
the parts test, `the_legacy_variants_are_the_extended_machine_parted`
and the legacy-parts suite red; private use ending at U+10FFFC turns
`the_merged_table_agrees_with_both_tries` red at U+10FFFD. The ready
predicate without its InCB term ("ready states [1, 29, 63]") and U+0000
narrow are `cargo::error`s. In `build_tables.rs`, GB1, GB3 to GB9c, GB11
and GB12/13 dropped one at a time each fire their own control -- GB1 and
GB5 did not until an Extend at the start of text and Prepend LF were
added -- and the state count, the seven-bit id and the palette and block
bounds are red; GB999 dropped is the same table by construction.

## The brand pictures in `tools/baud-pictures brand`

Not timed: no byte path changed, and nothing in the workspace links the
new crate. `baud-pictures brand [--check]` replaces the deleted
`make-brand-pictures.py`: resvg 0.48.1 as a library with its command
line's options, Pillow 12.3.0's PNG writer (`ZipEncode.c`'s filter
choice and row-at-a-time deflate, one IDAT per `max(65536, 4w)` buffer)
on zlib 1.3.2 through `extern "C"`, Pillow's LANCZOS on mode `F`, and
fontTools 4.63.0's glyf drawing, `SVGPathPen` and `BoundsPen` in `f64`.
Pillow's `_imaging` carries zlib 1.3.2 itself; `tools/shell.nix` adds
that nixpkgs revision's (`7dd199b0`) first on pkg-config's path, and
the run refuses another.

Identity: on the base `f4bbbaed` each arm wrote all 33 files (23 logo
SVGs, 8 logo PNGs, the card's two), every file byte-equal, and
`--check` printed the same 35 lines and exit 1 in both. The SVGs' note
now names the tool, the one line they differ in; no PNG moves. Five
perturbations through both arms gave equal stdout and exit, and stderr
equal but for the refusal's prefix, now `baud-pictures brand:`.
`baud-github-org.png` stays as committed: both arms draw
`e5a514ef...`, 48 pixels from it in the mark's light, by at most 15
levels.

Lanczos: in the pinned build the horizontal pass adds groups of four
unfused products and fuses the rest (`fmadd`); the vertical pass fuses
every tap. On six noise resizes no variant moved one of 71,307 pixels;
three inputs whose taps cancel do: the same 20 pixels give -2008.89 down
a column and -1336.97 along a row, and only the compiled split matches.

| check | perturbation, each a test |
|---|---|
| README line 8, the font pins, a missing glyph | a word changed, a byte flipped, U+10FFFD |
| clear space, card crowding, the crop | each box moved past its bound |
| tape, opacity, light control | a hole taped over, alpha 254, a moved pixel, nothing lit |
| GitHub limits | a tEXt chunk, the wrong size, RGBA, 1.2 MB, 3000 wide |
| outlines, fonts named | `<text>`, no path, `font-family=`, `FONT :`, `font\x1c=` |
| plasma report | the pen, the block, the cell width, either amber |

Nine code mutations each turned a test red; one did not: a component's
offset added before the pen's scale writes the same two decimals for
every point the card draws, so a unit test holds the order instead.

## The gvar, hvar, ink, joining and vertical cases, in Rust

Not timed: no byte path changed. `baud-oracle cases` builds the other
five cases fonts, 21 files, on write-fonts 0.55.0, and their Python
generators are deleted. Before the port, the Python under fontTools
4.63.0 with uharfbuzz 0.53.2 rebuilt all 21 byte for byte. 20 of them
come out byte for byte again; `joining-cases.ttf` is re-pinned,
`d6162796...` to `306fd95d...`, and differs only in where GSUB's and
GPOS's subtables lie and in the checksums that follow. Its new bytes
are what the deleted generator writes under the same fontTools without
uharfbuzz: fontTools' `otBase` packs GSUB and GPOS with `hb.repack`
when it can import uharfbuzz, else with its own serializer. Both
tables keep their sizes, 222 and 160 bytes, and every format word:
init, medi and the inner lookup SingleSubst 1, fina 2, calt
ChainContext 3, mark MarkBasePos 1, GDEF's ClassDef 2.

Three places are not write-fonts' own. gvar is packed here as fontTools
packed it (`cases/tuples.rs`). write-fonts writes a sparse tuple dense
when dense is no smaller, filling the points left to inference with the
values given for them, and shares no point set that only one tuple uses.
On its gvar, `compare` named sq's one-point tuple, `p0=0,-30` vs
`p0=0,0`, the first of the differences. Two's tuples over points 0, 2
and 5 and over 5 and 6, whose x is shared, came out dense too. So did
hvar's phantom-only tuples in all 17 fonts, where the values given are
the ones inference gives, so `compare` found them equal. No glyph of one
tuple shared its points. The packer's word-run yield (a word run ending
before two byte-sized values) is reached by no committed cases font and
is held only by `points_and_deltas_pack_as_fonttools_packs_them`, whose
bytes are fontTools 4.63.0's `compilePoints` and `compileDeltaValues_`
output for the same input. The bake's empty-program patch moves to
`cases/mod.rs` and covers every composite here, as fontTools wrote
them. hvar's HVARs, the malformed ones included, and phantom's 12-byte
`broken` are written whole as bytes, not as patches over a write-fonts
table, and all 17 hvar fonts come out byte for byte.

All eight oracles regenerated on the new fonts equal their files.
joining has no oracle file; beside `compare` and the format words,
`an_over_wide_letter_draws_alike_on_every_arm` (baud-present-macos)
draws the new font on every arm and passes. Perturbations, each red:

- every tuple written dense: `compare` and `advances.txt` stay equal,
  but `outlines.txt` moves at data line 660 and
  `the_gvar_cases_leave_their_points_to_inference` fails;
- vertical without the empty programs: `cases --check`, 1472 bytes
  built against 1492;
- mark-to-base refused in baud-font: `a_mark_moves_with_its_fitted_base`,
  on the old joining and the new;
- one-tuple point sets left unshared, init as a SingleSubst 2, and
  `broken` a byte longer: each new test's own.

No `overwide_letters` test observes joining's substitutions, on the old
font or the new; refusing SingleSubst turns 15 baud-font shaping tests
red.


## The feature list's gate in `cargo xtask`

Not timed: no byte path changed. `cargo xtask features [--check]`
replaces `gen-features.py` with its nine checks, and
`xtask/tests/features.rs` replaces baud-core's `feature_list` test,
which ran the script; both are deleted, so baud-core runs no Python for
the feature list.

| arm | inputs | result |
|---|---|---|
| the pages | the tree's data | `features.md` and `roadmap.md` written byte-equal to the script's |
| a 10-file tree | its pages written by each | byte-equal; done items newest first, a tie in title order |
| the checks | 38 perturbations of it, one or more per check | 34 of 38 equal in stderr and exit; 4 below |
| the stops | 5 more, a CRLF page among them | exit equal; text below |

Accepted, every exit code equal: the stale line names `cargo xtask
features` where it named the script (4); where the script stopped on an
exception, the run prints a line in place of the traceback, which for a
feature with no `id` is the traceback's last line, `KeyError: 'id'`;
invalid TOML gives the `toml` crate's reason at tomllib's line and
column; and a bad argument prints `xtask: not an option` where the
script printed its docstring. A page checked out with `\r\n` endings
passes on both, as Python's universal newlines read it.

The heading anchors keep Python's `\w`: the class `[\p{L}\p{N}_]` is a
word character on exactly the 138,558 code points Python 3.13's `\w`
matches, and on 4,382 that Unicode 15.1, Python's table, leaves
unassigned; `lower()` and `to_lowercase` differ on 55 code points, all
unassigned there, and `isspace` is equal on every one. No heading uses
them. `\bfn` is a scan with Python's boundary, pinned by a test.

Each perturbation is a `#[test]` holding the script's output line for
line. Red first: one proof test renamed in the real data turned both
`the_repository_passes` and the old `feature_list` red. Five mutations
each turned a named test red: a built feature's partial row let pass,
done items oldest first, sorted and then reversed, the `fn` boundary
dropped, the README count off by one; a first form of the boundary
mutation did not compile and is not counted.

`readme-forbidden.py` stayed at this commit: eight of the external list's
patterns use look-around, which `regex` lacks ("The forbidden search in
`xtask::forbidden`" below). The verify `cargo xtask publish`
runs builds the default targets, the library and the binaries, never an
example ([[source: cargo-eb94155-package-verify-rs]] lines 95-110), and
baud-app's `include` leaves `examples/` out of its .crate.

## The Hebrew stage built from its sources, `tools/baud-fontbuild stage`

Not timed: no byte path changed. `baud-fontbuild stage` merges
`hebrew/stage/<Style>.txt` into stock JetBrains Mono on write-fonts
0.55.0; `scripts/build-fonts.sh` runs it in place of the parametric
Python build, whose modules stay in the tree, unrun but for
`instance.py` and `pin.py`, which the Devanagari step still uses. The
four sources were read out of the Python Hebrew stage (`cb6087a6...`,
`2b46941c...`, `f38d9c23...`, `67947476...`, the hashes MANIFEST had
pinned) by `baud-fontbuild extract`, which writes a source only when
the stage built from it compares equal to the file it read. They hold
135 appended glyphs per style, 30 appended GPOS lookups, four mark sets,
two feature lists, the two hebr records and the OS/2 and name edits.

Spike first: every table of the four stages and of the four built
fonts (gvar through its tuples), read by read-fonts and written by
write-fonts, compares equal,
but for STAT's version (write-fonts writes 1.2; the stage has no STAT).
Against the Python stage, the built stage is comparator-equal in every
table, none left as bytes; as bytes, GSUB, GPOS, cmap (format 4: 3978
bytes for 3986, another segmentation) and head's checksum differ, and
every other table is equal. Two encodings
were patched back: write-fonts pads each glyph to an even length, and
the Devanagari merge digests every glyph's stored bytes, so the padded
stage failed it with "686 objects of BUILT changed" (the 686 odd-length
glyphs); and read-fonts' format 4 `glyph_id_array` runs to the end of
the cmap table, so a converted subtable carried format 12's 13,456
bytes inside it. The build trims the pad and builds cmap from mappings.

Through the unchanged merge, each style's font compares equal to the
committed one, none left as bytes, and differs in bytes in GSUB (772
bytes shorter: the stranded list tables the merge left among the old
lookups are not there) and head's checksum alone. REACH: GSUB is the
one encoding that reaches the shipped tables, read by baud-font's
layout. With the four rebuilt fonts in place of the committed ones and
the fixtures' fnv column moved to theirs, baud-font's and baud-render's
suites pass, 1236 tests; with Regular put back, shape.rs fails 7 of 15
on its fnv pin, the control that the rebuilt files were the ones read.
The committed fonts stay: MANIFEST pins the stage's and the merge's
new hashes and the committed outputs, which the build holds equal.

Perturbations: the pad trim off passes the comparator and fails
`every_glyph_is_stored_without_padding_as_the_stage_stores_it`; the
long-metric run not collapsed reads `hhea: numberOfHMetrics: 1848 vs
1878` against the Python stage. Ten source edits (a point, an anchor,
a lookup flag, a code point, a name, an OS/2 range, a class, an
advance, a feature list, a mark set) each name their table, and five
malformed sources are refused (`tests/stage.rs`).

## The README logo's geometry and plasma in `tools/baud-pictures`

Not timed: no byte path changed, and nothing in the workspace links the
crate. `baud-pictures geometry` and `plasma` replace the deleted NumPy
generator and its no-libm launcher; the film's driver, then Python and
now `baud-pictures logo`, runs them in `tools/shell.nix`. Every
expression keeps its grouping, and the Python and NumPy the bytes rest
on are ported: CPython 3.14.7's compensated builtin `sum`, `math.fsum`,
float `%` and `//`, and both `round`s; NumPy 2.5.2's pairwise sum,
`arange`'s fill, `gradient`'s one-sided edges, and `round` on a
`float64`, which rounds `x * 10^n` and divides: 146.95 to one place is
147.0 there and 146.9 in Python, and the plasma takes NumPy's where a
waypoint came off a contour walk. Their licences and fdlibm's are beside
the files and credited.

Identity, on master `28878986` and again rebased on `18a091cf`: from
`logo-walls.png` the four geometry files and, for each half, the
strands (22,471 and 22,358 lines) and the report are byte-equal. So
are both arms on two perturbed walls: a 5 by 5 dot of ink at (300,
100), which moves the geometry and not the strands, and the walls a
pixel right, where the seed search falls through to 2493. The
driver's own `walls_hold` and `strands`, repointed, give the same
digests. Each perturbation is a test pinned by the Python's SHA-256;
the frame grid's and a short field's refusals are its words and exit,
run against it, and the usage line names the tool.
Three error paths depart: a failed assertion prints `AssertionError:`
and a missing field file a message, each exit 1, where Python printed a
traceback; and the walls must be an 8-bit RGB PNG without interlace,
where Pillow opens more, which the driver never passes.

The spike, the parcels' 9-value hug `mean`: its values are 0 and 1, so
every order sums exactly. The pairwise port is held instead on 1 to 300
values over 61 binades, beside the other operations, against the pinned
Python's digests.

| mutation | red |
|---|---|
| naive mean of `FLOW_K`, half away in `nrm_field` or the parcel index, `det`'s kernel regrouped, the fields not narrowed to `f32`, `lexsort`'s second key descending | strands, both walls |
| `gradient`'s edge halved | the same, and the dot |
| naive or 64-block pairwise, `fsum`'s half-even fix-up or `sum`'s compensation dropped | the vectors |
| libm's `exp` in the parcels' settling | no-platform-math only |

Six move no strand: a plain sum in `Rng.pick` or the gate's low-pass,
Rust's `%` in the gate's distance, Python's `round` for NumPy's on a
move's end, `floor` for `//`, and `<=` for the coverage tie. The vectors
hold each function but not the call sites; the tie has no test. The
deleted generator called its second part of pi/2 33 bits: it is
fdlibm's `pio2_1t`, all 53. The constant is kept and its comment says so.

## The README logo's film and encoders in `tools/baud-pictures logo`

Not timed: no byte path changed, and the workspace links nothing new.
`baud-pictures logo [--check]`, run in `tools/shell.nix`, replaces the
deleted Python driver; `--keep DIR` leaves its frames and encodes. The
palette snap and delay rewrite are byte edits, gifski 1.34.0 stays a
program, and WebP goes through libwebp as Pillow 12.3.0 called it
(`webp.rs`'s module doc). Base `4a1daafc`.

Identity, both arms with `--check`: all four files `same`, the
2,671,015, 1,178,009, 2,646,127 and 1,100,085 bytes committed, and
stdout equal but for cargo's own test times. `cmp` finds each half's
`bare.rgb`, `bare.json`, `strands.txt`, `film.json`, `frames.rgb` and
`svg.repeats`, the eight geometry files and gifski's raw output equal;
all 434 snapped frames equal pixel for pixel (`6ee2ade8...`,
`f971aef3...`).

The GIF checks read baud's own decoder, which on the committed GIFs
composes all 434 frames and delays as Pillow did (`3937b1f2...`,
`2f71a24f...`). `tools/` links no crate of the workspace, so `logo` runs
baud-bench's `gif_frames` example with cargo, as it runs `flash_census`,
and reads back its frames, delays and size; whether a GIF loops stays
`gif::loops`, Pillow's rule. On a copy of `logo-dark.gif`, a signature
byte changed is refused (exit 1) and a byte changed in the first image's
data changes all 217 frames; that byte changed in what `logo` hands the
example stops the run, `dark: the word's contrast falls to 1.04 to 1`.

A byte of `logo-walls.png` flipped exits 1 with the same words but for
the prefix, a byte past `logo-dark.gif`'s trailer prints `DIFFERS` and a
`FAIL` line, and `--frob` a usage, each exiting 1. Each is a test too
(`walls_pinned`, `compared`, `args`). The determinism checks on gifski
and the SVG's encoder, the version refusals, `logo::decoded` and the
census run have no test of their own; the real runs exercise them.
`logo` without `--check`, on a `logo-light.svg` with a byte appended,
wrote all four back to the committed bytes.

Departures, all in the driver's own errors: a failed subprocess prints a
line where Python printed a traceback, the prefix is now `baud-pictures
logo:`, and the `tools:` line names zlib and libwebp. Gifski's frame
PNGs are filter 0 at zlib level 1, and it reads the same pixels;
`luminance` calls the platform's `pow`, so the printed contrast is not
held to `det`.

Twenty mutations against `tests/logo.rs`: 17 red. Half-even for
half-away and a fused composite cannot show, `(v k + g (255 - k)) / 255`
never being an integer plus a half, and skipping the mux writes the same
bytes on these files. `--check` runs by hand, not in `devenv test`, as
before.

## The DEC, national and ISO 96 sets compiled at build time

`build/dec_graphics.rs`, `build/dec_sets.rs`, `build/nrcs.rs` and
`build/iso96.rs` compile DEC Special Graphics, DEC Supplemental, the
JIS sets, the 14 national sets and the six 96-character sets from
`data/` links to xterm's charsets.h, keysymdef.h, JIS0201.TXT and the
six ISO 8859 mapping files. DEC Special Graphics now comes from xterm's
`map_DEC_Spec_Graphic`, whose 31 codepoints are foot's `vt100_0[]`
(`dec_graphics_is_xterms_and_foots`). No DEC page is read by the
build: `vt220_tables.tsv` and `std070_levels.tsv` stay in
`baud-domain/tests/`, out of the .crate (`cargo package --list`: 180
files, none under `tests/`), and `named_exactly` holds the cells where
xterm and Table 2-3 differ to `CONFORM` and `XTERM_DEPARTS` there.
Before the delete, the identity commit printed "identical:
DEC_GRAPHICS 31, DEC_SUPPLEMENTAL 94, JIS_KATAKANA 94, JIS_ROMAN 94,
LATIN_1 96, LATIN_2 96, GREEK 96, HEBREW 96, LATIN_CYRILLIC 96,
LATIN_5 96, UK 1" and the 14 national tables, 164 pairs in all.

**Release binaries.** `baud` and `ingest2`, built at one path from
fresh extractions with every file touched, baud-domain recompiled in
each arm: base, change and A/A are byte-identical. The comparison is
not vacuous: with the hand-written half in a module renamed `nrcs`, it
caught the 14 national statics' new symbols in `__text` and
`__TEXT,__const`, so the module keeps the name `nrcs_data`. No timing
run.

**Red first.** The national pairs left unsorted turn the identity test
red at Turkish[0] with the DEC-table oracles green; `CONFORM` not
applied, its build check dropped, turns it red at DEC_SUPPLEMENTAL[62]
and `dec_supplemental_is_table_2_3` red at 5/15. A changed xterm
codepoint at 7/14 of DEC Special Graphics turns the foot oracle red,
and a changed sha256 in either transcription's header its header test.
In `build_tables.rs` 41 perturbed inputs each fire their own
`cargo::error`: every refusal of the two xterm readers, the
headers, the 81, 63 and per-set counts, Figure A-2's mask and
JIS0201.TXT's lines; `the_cells_where_xterm_and_dec_differ_are_named`
is red with 5/15 unnamed, a named cell that agrees, and either side moved.

**The build.** `cargo build -p baud-domain --timings`, both arms at one
path, five rounds rotated, the first dropped: the build script compiles
in [0.99-1.04] s against base's [0.84-0.91], runs in [0.75-0.76] s
against [0.73-0.76], and the lib in [9.26-12.36] s against
[9.51-11.33]; only the compile's ranges part, by the four modules
added. Load average 4.8-11.6. Touching `src/lib.rs` ran no build
script in any round; touching a `data/` file ran it once. The
`baud-domain` .crate is 1.92 MiB compressed against base's 1.85.

## The gvar and cff2 oracles write their own files

Not timed: no byte path changed. `tools/baud-oracle`'s `gvar` and `cff2`
bins now write `outlines.txt` and `cff2-outlines.txt`, which their
Python scripts wrote until now; the bins were held to the data lines
alone. Only the two header lines changed, as the other six oracles'
did: `uharfbuzz 0.53.2` to `tools/baud-oracle`, and the generator line
to the bin. Both arms ran on a copy of the test fonts: the scripts on
nixos-26.05's Python 3.13.17 with uharfbuzz 0.53.2, its vendored
HarfBuzz 12.3.0, under `-I`, each edit made once at an anchor the
driver asserts; the bins on `tools/shell.nix`'s HarfBuzz 12.3.0.

| oracle | arm | rows | data lines, Python against Rust |
|---|---|---|---|
| `gvar` | the tree | 1,098 | byte-equal, fnv64 `75ea3687ca6110ae` |
| `cff2` | the tree | 1,360 | byte-equal, fnv64 `a4441be357bc8cbd` |
| `gvar` | JetBrains Mono at wght 100, 550, 700, 1000 | 1,098 | equal, `c40591db997cb7bc` |
| `gvar` | Fira Code's every 29th glyph | 1,158 | equal, `1d5a9ce409fe9cff` |
| `cff2` | Source Code VF at wght 150, 400, 555, 900 | 1,360 | equal, `ebdace6b81bdb714` |
| `cff2` | Source Code VF's every 5th glyph | 1,810 | equal, `6ce06aa1e74f0687` |

The digest is FNV-1a over the data lines joined by newlines. 550 and
555 sit between masters and are sampled nowhere in the committed rows;
1000 is past JetBrains Mono's 800 and 150 below Source Code VF's 200,
and HarfBuzz clamps both, so the 1000 rows are the committed 800 rows
and the 150 rows the default's, each asserted. The steps draw glyphs
no committed row draws. `tools/baud-oracle/tests/outlines.rs` holds the
four perturbations and the tree's two digests; `regenerate.rs` now
holds all eight oracles byte for byte.

Red first: with the header exemption gone from `regenerate.rs` and the
files not yet rewritten, `every_oracle_regenerates_its_committed_file`
failed, `cff2: not byte for byte`. Three mutations each turned tests in
`outlines.rs` red: `gvar`'s four places to three (3 of 5), `cff2`'s
`repr` to `{}` (3 of 5), and the variations not set (5 of 5); a first
form of the `{}` mutation did not compile and is not counted.
baud-font's `var` and `cff2` tests pass unchanged: they skip `#` lines.

`scripts/gvar-oracle.py` and `scripts/cff2-oracle.py` go in the next
commit. LICENSE names both as the writers of the two files, so
`cargo xtask third-party --check` fails on that commit until LICENSE
names `tools/baud-oracle` instead.

## The Noto instances and the Devanagari merge in Rust

Not timed: no byte path changed. The font build runs no Python now.
`baud_font::bake_partial` pins `wght` and keeps `wdth`, with fontTools
4.63.0's rules read at source; `baud-fontbuild merge` is the Devanagari
merge; `scripts/build-fonts.sh` runs both, and its pinned Python env, the
Python instancer, its repacker pin and the Python merge are gone. The full
bake is byte-identical to base's on all 8 instances `tests/bake.rs` reads.

The bake against fontTools' instances (MANIFEST's oracle lines, sha256
`92cde87d...` and `f3e9be2b...`): `compare` says equal at 400 and 700,
every table expanded, none left as bytes. They are not committed: a
third-party font under `baud-font/tests/` must be named in LICENSE, so
`tests/bake.rs` reads them from `BAUD_FONTTOOLS_NOTO` and says so when it
is unset; the build gates on the instances' hashes.

The IUP thinning is fontTools' `iup_contour_optimize`, held to its own
output on 1,000,000 random contours (seeds 7 and 8). That found what the
fonts did not: a search range one short (1 wrong in the first 200,000) and
a tolerance test written as correct rounding (1 wrong in the next
800,000). fontTools' test is `abs(complex(a, b)) <= 0.5`, through the C
library's `hypot`, and this Mac's is not correctly rounded: 3,000,000
pairs near the circle differ from correct rounding on 185,112 and from
`sqrt(a*a + b*b)` on 22,316. No threshold on `a*a + b*b` reproduces it, so
fontTools' own output depends on the platform within an ulp of 0.5. The
bake takes the `sqrt` form: 0 wrong of the million, and the instances'
hashes did not move.

The four faces (Regular `69b15411...`, Bold `ce3630a8...`, Italic
`2c528a8d...`, Bold Italic `8a5b9de6...`) compare equal to the files before
in every table, none left as bytes, STAT included: it is written by hand
at version 1.1, as before. As bytes, cmap, GDEF, GPOS, GSUB, gvar, HVAR and
head's checksum differ; glyf, loca, hmtx, post, name and the rest do not.
The files grow by 19,412 to 19,784 bytes: the item stores keep every data set
of the input where fontTools optimised them (GDEF 1,540 to 3,402 in
Regular), and gvar has no shared tuples.

REACH. `the_built_font_keeps_jetbrains_mono` failed on the first rebuild:
L30's stage writes GSUB through write-fonts, so stock's lookups were no
longer its bytes, and only baud-app's suite reads that. Patched back: the
stage splices stock's lookup block, 772 bytes of old lists inside it, and
`every_stage_keeps_the_stock_gsub_lookups_byte_for_byte` holds it without
the cache. The encodings the shipped fonts did not carry: data sets with no
regions, every delta 16-bit, gvar with long offsets, private points and
embedded peaks. Read back on all four, old against new: hb-shape 14.5.1 on
all 6,574 built-face fixture lines at the default and at wdth 62.5 and
81.25, 0 differ; baud's shaper on the 39,252 lines those make at three
widths, as run and cells, 0 differ (4,302 of 13,084 move with the width);
FreeType at 12, 16 and 24 px on every glyph, identical; baud's rasteriser
at those sizes and widths, identical, advances included. OTS 9.3.0 passes.
The fixtures moved in the fnv column alone, and the 2,394 Devanagari lines
still equal their recorded output.

Perturbations: thinning off names gvar at both weights; a value record's
default delta off names GPOS in Bold alone; the dotted-circle swap off
names GSUB's nukt; the stage's splice off fails the new test. A face
swapped in fires the shaping probes (399 of 798, 7,830 of 39,252). A fifth,
a floor on the axis name's ID, did not fire: no such name exists to reuse.

## The tcap, termcap and iTerm2 key tables compiled at build time

`build/tcap.rs`, `build/termcap_keys.rs` and `build/iterm_keys.rs`
compile xterm's 319 XTGETTCAP names, the termcap keyboard's 107 strings
and iTerm2's OSC 1337 keys from `data/` links to xtermcap.c,
xterm-256color.ti and two files of names. Those hold 39 and 54
identifiers and nothing else of the GPL-2.0 pages and source, which stay
`external`; line 1 of each names its parents' sha256s, which a test holds
to the manifest. Before the delete, `gen-iterm2-keys.py --check`, which
nothing had called, passed for the first time, and the identity commit
printed "identical: MUL 0xf2242639261b5841, ROWS 319, DOC_KEYS 39,
SOURCE_KEYS 54; both files' Rust as text" and "identical: string over 84
keys and every parameter, 21504 asked, 107 given; the file's Rust as text".

**The searches.** `MUL` is the first of 3,000 odd draws of CPython's
MT19937 from seed 1 that keeps every name within one slot, the 1009th,
found again by `build/py_random.rs`, its draws held to Python 3.13.12's
for seeds 0, 1 and 7. `--search` read 47 of `iterm.rs`'s 54 rows, its
pattern missing those rustfmt wraps; a multiplier refused for some names
is refused for all, so the search over all 54 finds the committed one,
and `each_index_multiplier_is_the_seeded_search` holds both indexes to it.

**Release binaries.** `baud` and `ingest2`, built at one path from fresh
extractions with every file touched, baud-domain recompiled in each arm:
base, change and A/A are byte-identical. The comparison is not vacuous:
with the modules written inline in `key.rs` and `py_random` declared at
the top of `iterm.rs`, `__TEXT,__text` matched and `cmp` differed, on 9
words of `__DATA_CONST,__const` in `ingest2`, line numbers moved by those
files' 9 and 6 lines. So each table keeps its file, now an `include!`, and
`py_random` is declared after `iterm.rs`' last line. No timing run.

**Red first.** The identity test is red with termcap names taken first
(ROWS[0]) and with the termcap table's last row winning. After the
delete, the tcap build's Left mapped to Right turns
`xtgettcap_answers_a_key_as_it_would_be_sent_now` red, the termcap build's
Up and Down swapped `the_termcap_keyboard_sends_the_entrys_strings`, Copy
misspelt in the names `every_iterm2_key_has_its_row`, and seed 8 the search
test. In `build_tables.rs` 38 perturbed inputs each fire their own
`cargo::error`, across the table reader, the entry and the names.

**The build.** `cargo build -p baud-domain --timings`, both arms at one
path, five rounds rotated, the first dropped: the build script compiles
in [1.05-1.35] s against base's [0.99-1.02], runs in [0.76-0.82] s
against [0.75-0.80], and the lib in [9.08-10.76] s against [9.31-9.37];
only the compile's ranges part. Load average 3.7-7.1. Touching
`src/lib.rs` ran no build script in any round; touching a `data/` file
ran it once. The `baud-domain` .crate is 1.93 MiB compressed.

## The feature arms' scope in `cargo xtask features-since`

Not timed: no byte path changed. `cargo xtask features-since <rev>|--explain`
replaces the Python script `check-features.sh --since` ran, whose command
line is unchanged, and the script is deleted. The lexer is scanned by hand
over code points, since `regex` has no look-behind and `'é'` is a literal
because of the quote two code points on.

| arm | inputs | result |
|---|---|---|
| `--explain` | 10 checkouts from the tip back 1,200 commits, 15 dirty trees | byte-equal; 1 checkout cargo refuses, both exit 1 |
| `<rev>` | 95 runs: 43 scoped, 42 `all`, 10 FAIL, edges among them (non-ancestor tips and merge bases, root, tag, reflog, short SHA, tree, blob, untracked, ignored, a subdirectory, no cargo), and the tests' trees, 130 runs | 95 equal in stdout and exit, and the 130; master has no merge commit, and a scoped merge base is a test |
| random workspaces | 300 trees with 774 revs, seed 21 | 258 explains and 774 revs equal; 42 trees the script never returned on |
| lexer, `os.path` | 120,000 random inputs and every `.rs` of the tree, seed 31 | every line equal |

A wrong port is caught by the three arms that ran it: one built with `'é'` read in bytes and
reads matched as paths differed on 5,761 lexer lines, on 32 of the 121 rev
and explain lines, and on the fourth random tree.

Accepted, every exit code equal. The usage line names the command. Where
the script stopped on an exception on `--explain`, the run prints one line
in place of the traceback, stdout empty. `cargo metadata` output that is
not JSON gives serde_json's words inside the `all` line; the control is a
`cargo` that prints `nonsense`.

**One departure in what it does, a hang, accepted as a recorded lean under the departure rule.** The script's loop re-queued the
file of every `include!` it resolved on every pass and ran while the queue
held anything, so a tree with one `include!` naming a file never returned:
under `ulimit -t 15` it exits 152, and the same tree with `include_str!`
exits 0. Master holds no such `include!`; the next one would have hung
`--since`. Here a file already scanned does not count, so a tree the script
returned on takes one pass, and the others end with the file followed.

The list that reaches every crate names `features_since.rs`, `py.rs`,
`py_path.rs` and `text.rs` in place of the script; the side-by-side arm was
built with the script's list, and the two differ by those lines. An xtask
that does not build checks every crate.

Python's text layer is kept: `\r` and `\r\n` read as `\n`, invalid UTF-8
as U+FFFD, and `\s`, `\w` and `\d` as Python reads them.

Each case is a `#[test]`, 106 of them. Red: 114 perturbations turned their
named test red and two turned another test red, by the kept logs; most ran
before the raw-byte paths and the digits, which they do not touch, 14 did
not build under `-D warnings` and ran without it, and two were green until
a test reached them: a reason repeated on a second pass, and `anything`
reaching on a change in a member.

## The keystroke driver is an example

Not timed: no byte path changed. The Python keystroke driver is now
`baud-bench/examples/key-latency.rs` over
`baud-bench/src/key_latency/mod.rs`, with the flags it had; the sweeps
above name the script as it was then. An example, because
`baud_is_the_only_binary_and_the_default` allows no other binary and a bare
`cargo bench` would launch a bench that posts keys. It still runs
`scripts/key-post.swift`; `scripts/key-pixel.swift` was never launched by
the driver and is still run by hand. No real sweep ran: one posts keys to
a window. The self-test and the stand-ins below open none.

Python 3.13.12 under `-I` against the debug build, each input to both:

| input | cases | stdout and exit code |
|---|---|---|
| the tree's self-test, `tests/keylat/` | 1 | byte-equal, sha256 `9b817f3c588d2695` |
| one perturbation of its fixture a branch | 41, exit 3 on 35, 0 on 6 | byte-equal |
| whole sweeps over `tests/keylat/stub` | 7, exits 0, 1, 2, 4 | byte-equal, raw rows too, pids scrubbed |
| command lines that stop before a launch | 53 | byte-equal, help's name scrubbed |
| mutated `keylat:` lines | 20,000 | same record or same failure text |
| `front` and `post` lines | 3,021 and 13 | same fields |
| random three-arm runs, all tables | 1,500 | byte-equal |

The first three rows' answers are committed in `tests/keylat/golden/`, so
`baud-bench/tests/` holds the Rust to them with the Python gone.
`the_fixture_s_first_record_is_what_the_line_emits` still ties the fixture
to `baud-app/src/latency.rs`.

**What differs.** With `BAUD_KEYLAT_FIXTURES` unset the `fixtures:` lines
read `baud-bench/..` for `scripts/..`; set, they are equal. A Python
traceback (a fixture unreadable, a binary that will not start) is one
stderr line at the same exit code and stdout, and a helper that will not
start now also stops the app. An offset too large for a stamp is a counted
parse failure where Python crashed. Non-ASCII digits are refused where
`int` took them; the app prints ASCII, and every ASCII case above agrees.

**The stream child.** `--child stream` ran `python3 -u`; it now runs this
driver with a flag, so the header's `child:` line names that command. Over
a datagram socket, which keeps each write(2) apart, both children write the
number and the line's end separately, and
`the_stream_child_writes_each_number_and_each_end_alone` pins it. Through a
pipe, five rounds rotated, the first dropped, a line takes [2.512-2.573] ms
under Python and [2.531-2.598] ms here: ranges overlap, no result. What the
swap does to a sweep's `build` is unmeasured.

**Red first.** 35 mutations of the Rust each turn a test red, the kept logs'
34 and `-h=1` reaching help. A first run left three alive: two have tests
now, and the third weights the quartile by 3/4 and 1/4, which are exact, so
it is the same function, and 200,000 random lists found no difference.

## The build's name: `<version>+<commit>`, and what it costs the core

The app calls itself `<version>+<commit>`: the manifests' version and SemVer
build metadata (`[[source: semver]]`, rule 10), which orders nothing and is
"generally ignored by Cargo" (`[[source: cargo-manifest-version]]`). A registry
index keeps one entry for `1.0.7` and `1.0.7+extra`
(`[[source: cargo-registry-index]]`), so a hash in a manifest would have nowhere
to go; none goes there. `baud-app/build.rs` writes `BAUD_VERSION`, and no file.

**Real runs of the identifier**, `baud --version` after each build, when the
version was `1.0.0-beta.1`:

| tree | prints |
|---|---|
| a clean commit | `1.0.0-beta.1+0a8418b` |
| a tracked doc edited | `1.0.0-beta.1+0a8418b.dirty` |
| the edit reverted | `1.0.0-beta.1+0a8418b` |
| `BAUD_GIT_REV=abc1234-dirty`, nix's form | `1.0.0-beta.1+abc1234.dirty` |
| `git archive` unpacked, no `.git` | `1.0.0-beta.1+unknown` |
| `BAUD_GIT_REV=a_b` | the build stops, naming the rule |

No build leaves a file in `git status`. `self.shortRev` evaluated to `11dd43f` for a
commit and the flake's `BAUD_GIT_REV` to `11dd43f-dirty` on a dirty tree, so a
clone takes the same seven digits. An edit reruns the build script and
recompiles the `baud` crate, 3.1-5.4 s here, since a binary that kept saying
"clean" would be the wrong answer; a second build with nothing changed is fresh,
a packed ref and a missing `packed-refs` included.

**XTVERSION's text is told at run time and lives in `StrBuf`**, an
`Option<&'static VersionText>`, 8 bytes that `StrBuf`'s 11 of end padding hold.
`-Zprint-type-sizes` gives `Term`, 17,264 bytes, and `Storage`, 15,600, the same
fields at the same places as base; inside `StrBuf`, `end padding` goes 11 to 3.

**`ingest2` is not byte-identical**: `__text` 1,225,216 against 1,225,472 bytes,
`cmp` differs, an A/A rebuild of the change is identical. `want_reply` grows from
48 to 112 bytes of code. The `StrBuf` fields laid out after the new one sit 8
bytes later, and `feed_read_outlined` reads some of them, so that is an immediate
in a path that runs per read, and it was timed. Base e34337a3, change and A/A at
one path, six rounds rotated, the first dropped, MB/s median [range], load average
9.3-9.6:

| payload, reps | base | change | A/A |
|---|---|---|---|
| cat, 400 | 1519 [1397-1579] | 1539 [1518-1566] | 1533 [1382-1565] |
| btop, 200 | 857 [854-858] | 856 [854-861] | 854 [840-856] |
| cat-cjk, 100 | 506 [500-514] | 504 [491-508] | 503 [502-510] |
| less, 400 | 1361 [1350-1377] | 1358 [1342-1364] | 1369 [1364-1380] |

Every range overlaps base's: no result. `cells_written` is the same on all three
arms of each payload: 339,854,081, 25,964,600, 38,322,698, 20,748,228. What would
have proven it wrong is a range parting below base's; none did.

**The film.** `NSImageCurrentFrameDuration` sums to the file's own delays, by
baud's GIF decoder, within a millisecond and under five seconds, so a tick steps
by AppKit's reading. The two GIFs, 5.3 MB, go in the bundle's Resources: in the
crate they would take `baud` from 4.39 MiB to about 9.7 of crates.io's 10.

## The capture tools are examples

Not timed: no byte path changed. The five capture and record scripts and the
three input generators are examples over `baud-bench/src/capture/mod.rs`,
flags kept. Python 3.13.12 under `-I` against the debug build, each run to a
scratch path; no committed recording was written:

| tool | run | Python against Rust |
|---|---|---|
| `gen-vim-unicode` | the file | byte-equal, sha256 `8ed81bcbf06b3258` |
| `gen-marks-payload` | `marks`, `covered` | byte-equal, 1,048,665 and 1,048,653 bytes |
| `gen-yazi-files` | seven files, the same `sips` | byte-equal, `photo.jpg` too |
| `capture-tui` | vim 9.1 on that file, 9 s, 16 keys | equal, and equal to `tests/recordings/utf8/vim-unicode.raw`, 138,583 bytes |
| `capture-tui` | btop 1.4.7, 3 s, a scratch home | `.steps` header equal; 85,648 and 85,644 bytes, 18,084 in common, then live figures |
| `capture-resize` | bash over `tests/resize/short.sizes` | `.raw` and `.steps` equal; header and size column equal the committed zsh `.steps` |
| `capture-yazi-walk` | yazi 26.5.6 on each's own files, 120x40 | equal, 2,804,522 bytes |
| `record-kitten-icat` | all 16 cases | 12 stream cases equal, with the README's sha256s; 4 differ |
| `record-shell-start` | a stand-in execing `bash --noprofile --norc` | one read, the same 18 bytes; only its time differs |
| the command lines | 1,034 random and listed lines | exit code, stdout and every file equal |

The walk's 13 Sixel bodies equal the committed file's, generator and `sips`
included; of the 14 stretches of text around them one differs from the
committed bytes, the first screen, 2,318 bytes to 2,396, in both tools' runs.
The 4 kitten cases differ between two Python runs as well: a random image id
in `placeholder`, a random temp-file name in the `detect-*` queries.

`baud-bench/src/capture/deflate.rs` is zlib 1.3.1's level 6 in Rust: which
bytes a deflate writes is the deflater's choice, and no C engine links into the
workspace. Against `zlib.compress`, 600 inputs differ in none; against Python's
`repr`, `unicode_escape`, `float` and `int`, 40,000, 20,000 and 20,000 inputs
differ in none.

**Two exit codes differ, on values no caller uses.** `\N{NAME}` in `--keys` or
`--answer` is refused, exit 1, where Python took a name at or below U+00FF: no
README or `.steps` names one, `\xNN` writes the same byte, and the names and
aliases of 256 code points would be carried for it. Non-ASCII digits in a
number are refused, as the keystroke driver's are.

**Red first.** 40 mutations of the Rust each turn a committed test red, one run
on this tree. `rfc1321-md5` and `pdf-reference-1-4` are pinned `external`.

## A child's last bytes on the pty

Not timed against the core: no byte path changed. `capture-tui` over a program that
will not start failed once with `0 bytes`: a child that wrote and exited at once had
lost all it wrote.

**The cause.** `forkpty` closes the slave in the parent after the fork. Where the
child has already written and exited, that close is the last, so `d_close` runs in the
parent (`[[source: xnu-spec-vnops]]`). `ptsclose` sets `t_timeout = 60`
(`[[source: xnu-tty-dev]]`) and `ttylclose` waits that long for the queue to be read,
then flushes it (`[[source: xnu-tty]]`). The one waiting is the reader.

| control | result |
|---|---|
| `forkpty`'s steps, the parent's close put off 0.05 or 0.2 s, no load | close 0.601-0.602 s, 0 bytes read, 5 of 5 |
| `pty.fork` under the load below, time inside it | lost 38 of 2,700, all 0.603-0.631 s inside; the others at most 0.438 s |
| `pty.fork`, the reader waits 0.55 s, then 0.65 s, no load | lost 0 of 20, then 20 of 20; slave kept, 0 at every wait to 3 s |

**The arms.** 90 at once beside 18 spinners at nice 0, 18 logical CPUs, the first two
rows rotated, as were the last three:

| arm | rounds | lost |
|---|---|---|
| the tool on `forkpty` | 6 | 304 of 21,600 |
| the tool keeping the slave | 6 | 0 of 21,600 |
| Python's `capture-tui`, deleted, under `-I` | 4 | 25 of 3,600 |
| `pty.fork` in Python | 4 | 85 of 3,600 |
| `openpty`, the parent keeping the slave | 4 | 0 of 3,600 |
| `pty.fork`, the child's `tcdrain` before its exit | 4 | 0 of 3,600 |

The first three run `-- ./nope/x`, a lost run being exit 1. At nice 10 without spinners
the first two gave no power, 2 of 21,600 and 0. Only keeping the slave asks nothing of
the child. No test can show a race gone: these counts are that claim.

**The fix.** `Child` is `openpty`, `fork` and, in the child, the master's close and
`login_tty`; the parent keeps the slave. `readable` selects in ticks of 5 ms, asks
`waitid` with `WNOWAIT` between them, and closes the slave once the child is gone and
the master empty. A byte wakes `select` whatever the tick. On Darwin a session leader's
exit revokes the terminal (`[[source: xnu-kern-exit]]`), so the master has its EOF
first and the tick decides nothing; on Linux, with that close taken out, the capture
tests hang. `-- /usr/bin/true`, 40 runs twice, ended at medians of 315.8 and 312.9 ms
before and 314.6 and 316.2 after; 5 s idle read 0.00 s user and sys on each. A child
left with bytes unread at the tool's end (`yes`, 1 s) ends as it did before only
because the master closes first; with the slave closed first it ended 0.6 s later,
the same wait.

**Unchanged.** A C program printing its session, group, foreground, fds, signals, mask,
umask and termios is byte-equal to the build before, through both pty tools and with
the tool's fds 0, 1 or all three closed; `/usr/bin/env` too; on Linux, through
`capture-tui`, the same. vim equals the committed 138,583 bytes. `capture-resize`
over bash equals the build before, 171 bytes over 26 steps. A `.steps` file's read
splits differ between runs of one build.

**Departures from Python.** It lost these bytes and the tool does not, cause and
controls above. A plain program that closes fds 0, 1 and 2 and lives on ended Python's
read at once; here the read ends at its exit, the price of the fd. A grandchild
ignoring SIGHUP ends as it did: `early` on Darwin, `earlylate` on Linux.

Nine perturbations each turned a named test red, seven on Darwin and two on Linux. A
close with bytes unread turned none on either, so that guard has no test.
`baud-app/src/pty.rs` is still on `forkpty`; whether it loses bytes is unmeasured.

## The launch, idle and reply programs are examples

Not timed against the core: no byte path changed. The programs that measure the
app from outside are examples over `baud-bench/src/host/mod.rs`: `startup-bench`,
`idle-cpu`, `shader-cache-bench`, `vt-reply` and `dock-launch-check`. The dated
sections above name each script as it was run then. No app ran here: each launches
what it is handed, and these runs handed `baud-bench/tests/host/app.sh`.

Python 3.13.12 against the Rust, stdout and exit code:

| input | cases | result |
|---|---|---|
| `baud-bench/tests/host/cases.tsv`, the stand-in moving the clock | 195: exit 0 on 95, 1 on 67, 2 on 33 | byte-equal, help's name set |
| the same rows, the Python alone | hash seeds 0-5 | byte-equal to the goldens |
| real clocks over the stand-ins | 8, and 3 of `vt-reply` | equal, clock figures scrubbed |
| option forms, nothing patched: `--opt=a b`, `--op=v`, `-1e5`, `-1.`, `-1_0`, `-1`, an option after an option, a repeat | 21,169: `idle-cpu` 19,335, `startup-bench` 1,355, `shader-cache-bench` 142, `vt-reply` 251, `dock-launch-check` 86 | exit equal on 20,949; 7 ran past the sweep's 25 s in both arms; the other 213 hold a non-ASCII digit |
| mutated lines through each parser against Python's `re` | 40,000 | same fields |
| `int`, `float` and `splitlines` on mutated text | 20,000, 20,000 | same values |

`baud-bench/tests/host/golden/` keeps the first row's answers. The stand-in prints
other numbers while its cache directory is away, so a cold arm is seen, and no run
touched the real caches.

**What differs.** A traceback is one stderr line at the same exit code and stdout.
Stages at one median came out in Python's `set` order, five orders over six seeds on
one fixture; here by name. `startup-bench` and `shader-cache-bench` require the
binary where Python defaulted to `target/release/baud`, so `startup-bench 0` stops at
the usage where it printed a blank line, exit 1 in both. `BAUD_BENCH_STAGE` is
`|`-separated names, not the pattern `re` took. `SIGTERM` and `SIGHUP` left the Metal
caches aside under Python, until its next run; here they restore them and exit 130,
as `SIGINT` does.

`idle-cpu` catches the same three. Sent to it alone, `SIGINT` stopped the launched
program under Python, and `SIGTERM` and `SIGHUP`, which it did not catch, left it
running. Here all three stop it, remove the `--aa` copy and exit 130, where Python
died of the signal: -2, -15 and -1 to its parent.

**Two kinds of command line keep a different exit code.** Non-ASCII digits are refused
where `\d`, `int` and `float` took them: `--rounds ٣` is exit 2 where it ran three
rounds, `startup-bench ٣ <binary>` and `vt-reply ٣` exit 1 where they ran, and
`--arm -٣` or `--cmd -٣` is an option here, exit 2, where `\d` made it a value. A count
is typed in ASCII, and a path or a command opening on `-` and such a digit is the one
value this loses; reading them would take a table of Unicode's decimal digits here. A
count past `usize` is no rounds and past `i128` is refused, in the three programs that
take one, where Python went on until stopped: a run that never ends has no report to keep.

**Beside them.** `iutf8_probe` is `the_kernels_erase_follows_iutf8_alone`. The embedded
half of `verify_fonts` is `baud-font/tests/embedded_hebrew.rs`, fontTools 4.63.0's
counts held against baud-font's reader. The live kitten test forks its own pty in
place of its Python bridge, because only it runs kitten; on kitten 0.49.2 base and
change printed the same control data and counts in all three modes.

**The wall.** One stand-in through each `startup-bench`, six rotated sweeps of seven
launches, the first dropped: medians [18.3-50.5] ms under Python, [20.4-70.8] here, [17.6-73.5] an
A/A copy. Load average 44, and the ranges overlap: no result. `scripts/vt-child.sh` runs the reader between its two clocks,
and a waiting reply took [11.9-20.8] ms through Python and [3.5-6.6] here, so a `vt-compare` wall
from now on holds that much less harness, on every terminal alike.

**Red first.** Deleting the scripts turned
`every_repository_path_named_in_the_docs_exists` red on the idle script's path. Of 77
mutations of the Rust, 76 are red; a first pass left 9 green and 6 unbuilt, and each
got a case or a rewrite. The last changes only a message: `dock-launch-check` without
its `access` check still fails the spawn. Undoing each of `idle-cpu`'s option and
interrupt lines alone, 19 undoings, turns a named test red. The sweep's first 15,807
`idle-cpu` lines differed in exit on 2,031 on the build before them; all 19,335 differ on
207 now. `catch_interrupt` in the example has no test, because no test runs an example;
real signals to that binary, before and after, gave the figures above.

## The forbidden search in `xtask::forbidden`

Not timed: no byte path changed, and the app's release binary, built base and
change at one path, is byte-identical (`cmp`). `readme-forbidden.py` is now a
module and `cargo xtask forbidden`, on `regex::bytes` with `unicode(false)`;
`demo` and its test call the module through a path-only dev-dependency, which
`cargo xtask publish --verify` never builds. No second engine: 8 of the external
list's 48 patterns used look-around, and a copy of the list holds them rewritten
at the same line numbers, with a capture group named `hit` as the reported span.

**Every pattern against Python 3.13.12's `re.finditer` on the original**, on both
views of each input: 17,223 blobs reachable from master at `eccfbdd9` and 2,968
tracked files (1.77 GB, 0 spans either way), 187,456 hand-built edge cases and
1,600,000 seeded token strings. 3,643,323 spans, 0 differences on all 48 lines:

| line | spans | line | spans |
|---|---|---|---|
| 48 | 133,799 | 60 | 52,890 |
| 55 | 161,512 | 62 | 130,259 |
| 58 | 206,119 | 63 | 136,593 |
| 59 | 225,957 | 64 | 159,084 |

The other forty total 2,437,110. 400 real blobs with every control planted:
37,910 spans, 0 differences. The escape-stripped view hashed equal on all
1,804,690 inputs. The proof can fire: 12 perturbations of the eight lines, a guard
or a refusal taken out, each moved that line's spans.

**The script against the module.** 2,968 tracked files: 0 hits, stdout, stderr and
exit equal; each control planted in its own file: 53 hits, equal. Of 117
side-by-side cases, 87 are equal, 13 are tracebacks with stdout and exit equal,
and 6 are the usage line, which names the command. The other 11: a pattern the
engine refuses (look-around, a backreference, `\Z`, an unclosed group) exits 2,
naming its label and reason and not its text, where an uncaught `re.error` exited
1; a raw byte above 0x7f is refused rather than read as a character; `(?i)` and a
group name hold to their own pattern, where the script's text join stopped; two
labels that are not UTF-8 give one line; `$` is the end of the text alone. The
list holds no raw high byte, bare flag, unescaped `$`, `\<` or class operation.

**Red:** 36 mutations of the module, 35 turning a named test red (an empty hit
never ending as a hang at 300 s); the other takes out `\x1b[()*+][0-~]`, which
`\x1b[ -/]*[0-~]` already covers. 83 of the tests' 84 span rows were checked
against `re`, 0 differences; the 84th pins the departure below.

Open: `^` holds at offset 0 alone, so after a one-byte hit there, or a refusal
with a look-behind, a hit at offset 1 is not reported
(`offset_one_is_not_looked_at_twice`). No line of the list reaches it. The command
still exits 1 for a line with no label, as the script did; `demo` reads the verdict.

## The Linux tools are examples

Not timed: no byte path changed. The five tools of `scripts/linux-damage.sh`,
`linux-frames.sh`, `linux-pixels.sh` and `linux-pointer.sh` are examples over
`baud-bench/src/linux/mod.rs`; the three Python blocks in `linux-pixels.sh` are
`linux-pixels window-rect`, `control` and `png-size`; `python3` is off the four
`apt-get` lines. What runs in a container is built by the driver's build step into
its target volume, `linux-damage` and the Mac half of `linux-pixels.sh` by the host's.

| driver, every arm | Python's run against Rust's |
|---|---|
| `linux-pointer.sh`, two runs each, six arms | stdout byte-equal, 86 ok and 16 FAILED, pixel counts too |
| its client's requests, 34 sessions | 1,590 lines of sway's log byte-equal, the time taken off |
| its 34 traces | 6 differ, and 3 between the two Python runs: the app's requests move against its events |
| `linux-pixels.sh`, 18 fixtures and the moves | the comparison stage's 237 lines byte-equal |
| `linux-frames.sh`, weston 16.0.0 and sway 1.10.1 | the same 23 checks ok; commits and gaps are the run's |
| `linux-damage.sh`, 3 rounds, 100 frames, 10 shots | exit 0 on both; times are the run's |

**Each tool on the same inputs.** Python and Rust over every log, screen and tree those
runs kept, and 300 random frames through `png`: 1,189 comparisons of stdout, exit code
or bytes, none differing, on the Mac (Python 3.13.12, zlib 1.3.1) and with the Linux
build in trixie (3.13.5, 1.3.1) and bookworm (3.11.2, 1.2.13). The Rust comparison stage
over Python's frames gave its 24 diff PNGs and 42 PAMs byte-equal. `linux-pointer`
against a fake compositor, on Linux: every request byte, keymap, stdout and exit code
equal over 103 scripts, and the exit code and requests over 9 starts. These and 456
reader runs, each one input perturbed, are the Python's goldens in
`baud-bench/tests/linux_tools/`, run on the Mac and on Linux. A test reads the opcodes
from the two pinned XMLs.

**zlib.** `write_png` deflates at level 9. `baud-bench/src/capture/deflate.rs` now takes
a level: 6 and 9 run one path, 9's row being `{32, 258, 258, 4096}` of 1.3.1's
`configuration_table`, read there. `tools/baud-pictures` links real zlib, but the drivers
build no part of that workspace. A PNG's chunks and filters are read here and its stream
by `baud_domain::zlib`, because `baud_domain::png` refuses a wrong CRC and a filter above
4, which the Python read.

**What differs.** Text at equal codes, each with a golden or a run showing it: a
traceback is one line; the usage says `linux-pixels`, loses "Standard library only" and
gains three commands; an `sh` line not UTF-8 reaches bash with U+FFFD; a compositor time
of `nan` prints the other numbers' range. Exit codes, on values no driver writes:
non-ASCII digits and integers past `i128` are refused, as are, in sway's tree, a list or
object as a rect's field, `NaN` and nesting past 128. A trace's stdin not UTF-8 reads with
U+FFFD, as in the containers' locale; under a UTF-8 one the Python exited 1.

**A half close the test could not rely on.** The fake compositor's `shutdown(SHUT_WR)`
left the client blocked in `recv` in 130 of 120,000 runs on this Mac and 0 of 20,000 on
Linux; a plain socketpair in Python lost none of 40,000, and the cause was not found. The
fake closes at the client's next `sync` instead, 0 of 120,000.

**Red first.** 93 mutations of the Rust over four passes, tests added as survivors showed
what they missed: 92 turned a test red, level 9's row and level 6 among them. The other
changes a global's name padding, which no request shows: each name bound, with its NUL, is
a multiple of four bytes.

**On this tree, in both arms.** `linux-pointer.sh` exits 1, its 16 FAILED being the wheel,
the thumb and the edges, and `linux-pixels.sh` reports screens that differ from the
bench, which it exits 0 on. `snapshot.debian.org`, which `linux-frames.sh` pins, failed
image builds with a 504 where the error was read, and later let them through.
