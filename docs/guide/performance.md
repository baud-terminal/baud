# Performance

baud is built to be fast at the four things a terminal does all day: taking
in a program's output, answering a key, drawing frames while the screen
churns, and doing it in little memory. This page gives the headline numbers
in plain terms. [MEASUREMENTS.md](../../MEASUREMENTS.md) is the full record,
with every table's method, and [measuring](../contributing/measuring.md) says
how to re-run them.

Every figure here was taken on one Mac, an Apple M5 Max running macOS 27.0,
and carries the commit and date it was taken at. A figure is not current
just because it is printed: code has changed since most of them.

## In short

- **Taking in output: faster than ghostty and alacritty on every recording
  measured.** baud's core is 1.19 to 5.80 times as fast as ghostty's on
  nine recordings of real programs, in memory (`935e4f1`, 2026-09-20).
- **Through a real terminal connection: the fastest of five terminals.**
  Replaying recordings into baud, alacritty, kitty, ghostty and wezterm,
  baud had the highest throughput on every recording, tied
  with alacritty on one, and used the least CPU, or tied for least, on all
  but one (`a9bb28c`, 2026-09-12; before baud kept scrollback).
- **Answering a key: about 3 ms to a new frame.** From a key arriving to
  the new frame being handed to the display took a median of 3.0 ms when the
  program echoing it was `cat` (`e8bb82d`, 2026-09-18).
- **Starting up: about 0.1 s to the first frame.** 99 ms with the system's
  shader caches warm and 134 ms with them cleared (`4d583bb`, 2026-09-19).
- **Sitting idle: under half a percent of one CPU core.** 0.46% with a
  program that prints nothing (`59e6476`, 2026-09-18).

## Taking in output, against other terminals

Measured at `935e4f1` on 2026-09-20 with `bench-others/multibench`: each
terminal's core fed the same recorded bytes, in memory, each keeping 10,000
lines of scrollback; medians of 9 runs, taken in turn. Megabytes per second, higher is better:

| recording | baud | ghostty | alacritty | baud vs ghostty |
|---|---|---|---|---|
| less, paging a file | **1684.5** | 290.2 | 241.2 | **5.80x** |
| cat, scrolling text | **1425.1** | 424.3 | 203.3 | **3.36x** |
| git log | **1637.9** | 516.9 | 211.5 | **3.17x** |
| asciiquarium | **524.2** | 197.2 | 243.6 | **2.66x** |
| less, Chinese, Japanese and Korean text | **498.0** | 247.2 | 271.3 | **2.01x** |
| vim, Unicode text | **636.4** | 346.8 | 293.0 | **1.84x** |
| cava | **554.1** | 318.9 | 362.2 | **1.74x** |
| btop | **871.3** | 511.2 | 330.9 | **1.70x** |
| cat, Chinese, Japanese and Korean text | **459.4** | 385.1 | 221.7 | **1.19x** |

What this does and does not show:

- It compares the part of each terminal that interprets output, not the
  whole application drawing to a screen.
- baud always works out which characters belong together, such as an
  emoji sequence or a letter and its accent, and ghostty's core does only
  when a program asks, so the Chinese, Japanese and Korean rows do not
  compare equal work.
- It was taken before the benchmark pinned where the compiler places each
  function, which had moved one of baud's rows by as much as 10%. No sweep
  has been taken since.

## Through a real terminal connection, against other terminals

Measured at `a9bb28c` on 2026-09-12: each terminal opened fresh at 200x50 in
Menlo 13, with a recording replayed twenty times into it through a pty, the
same connection a shell uses.
Throughput in MiB per second:

| recording | baud | alacritty 0.17.0 | kitty 0.48.2 | ghostty 1.3.1 | wezterm |
|---|---|---|---|---|---|
| btop | **88** | 72 | 63 | 71 | 30 |
| cat-scroll | **81** | 67 | 57 | 66 | 39 |
| cat-cjk | **72** | 59 | 30 | 62 | 10 |
| asciiquarium | **68** | 67 | 43 | 53 | 46 |
| vim-unicode | **42** | 38 | 30 | 35 | 21 |

baud leads by 16-22% on btop, cat-scroll and cat-cjk; on asciiquarium its
lead over alacritty is inside the noise. macOS hands a terminal at most
1024 bytes at a time, so these rows are bounded by the operating system as
much as by any terminal: baud reached 89% of what the connection delivers to a
program that reads and discards. baud did not keep scrollback when this was
taken. [MEASUREMENTS.md](../../MEASUREMENTS.md#five-terminals-through-a-pty)
has the CPU time of each run too.

## Typing

Measured at `e8bb82d` on 2026-09-18, on the built-in 120 Hz display, medians
of 120 keys:

| from a key arriving to | with `cat` | with the shell |
|---|---|---|
| the frame handed to the display | **3.0 ms** | **5.4 ms** |
| the display's next refresh, at the earliest | 7.1 ms | 11.3 ms |

The shell's extra time is the shell itself: zsh took 2.5 ms to echo a key
where the operating system's own echo for `cat` took 0.015 ms.
No other terminal's typing latency has been measured on this machine, so
there is no comparison.

## Starting up and idling

- **Launch** (`4d583bb`, 2026-09-19): 99 ms from starting to the first frame
  with the system's shader caches warm, 134 ms with them cleared. baud ships
  its shaders compiled, which saved 256 ms on that cold launch.
- **Idle** (`59e6476`, 2026-09-18): 0.46% of one core with nothing being
  printed.

## Memory

baud's screen storage is about 24 MB reserved but only +0.3 MB actually
used for a 50x200 screen fed data, because pages never touched are never
allocated. That figure was taken on baud's previous core, at `8c2d182`
(2026-08-29), and is argued, not re-measured, for the current one.
No memory figure has been compared with another terminal.

## Not measured

- Memory, frame rate and typing latency against other terminals.
- Any discrete graphics card: every drawing figure is this Mac's built-in
  GPU.
- Any machine other than this one.
