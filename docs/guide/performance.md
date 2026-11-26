# Performance

baud is built to be fast at the four things a terminal does all day: taking
in a program's output, answering a key, drawing frames while the screen
churns, and doing it in little memory. This page gives the headline numbers
in plain terms. [MEASUREMENTS.md](../../MEASUREMENTS.md) is the full record,
with every table's method and, under Reproducing, how to re-run them.

Every figure here was taken on one Mac, an Apple M5 Max running macOS 27.0,
unless it is marked Linux: those were taken on one PC, an AMD Ryzen
Threadripper 9980X running Linux 7.2.6. A figure from one machine
is not comparable with a figure from the other. Every figure carries the
commit and date it was taken at. A figure is not current just because it is
printed: code has changed since most of them.

## In short

- **Taking in output: faster than the other terminals' cores in
  [the tables](#taking-in-output-against-other-terminals), on every
  recording measured.** By each table's last column, baud's core was 1.19
  to 5.80 times as fast on six recordings of real programs, in memory
  (`e5bc3ef`, 2026-09-20), and 1.65 to 1.70 times on three captured later
  (`e9af53d9`, 2026-10-28); the two sweeps are not comparable with each
  other. On Linux it was 2.14 to 8.26 times as fast on four (`f231b87e`,
  2026-11-15).
- **Through a real terminal connection: the fastest of five terminals on
  the Mac, and level with five others on Linux on three of four recordings.**
  On the Mac, replaying recordings into baud and four other terminals, baud
  had the highest throughput on every recording, tied with one of them on
  one, and used the least CPU, or tied for least, on all but one (`46f5e1c`,
  2026-09-12; before baud kept scrollback). On Linux, drawing on the GPU, its
  first choice there, baud took in 93.7-116.8 MB/s as each terminal ships:
  level with or ahead of each of the five other terminals measured on the
  scrolling text, the commit log and the paging recording, and behind two of
  them on the Chinese, Japanese and Korean text, the faster of the two by 1.27
  times, with ranges apart. Drawing on the CPU it took in 76.9-91.9 MB/s
  (`8e404c38`, 2026-11-24, four recordings,
  [MEASUREMENTS.md](../../MEASUREMENTS.md#six-terminals-through-a-pty-both-drawing-arms-linux-x86_64-2026-11-24)).
- **Answering a key: about 3 ms to a new frame on the Mac.** From a key
  arriving to the new frame being handed to the display took a median of
  3.0 ms when the operating system echoed it (`28f2382`, 2026-09-18). On
  Linux, a key reached the compositor's commit of the frame showing it in a
  median of 0.92 ms, an indicative figure: baud was built with app changes
  not in the repository (`2789e0d0`, 2026-11-16).
- **Starting up: about 0.1 s to the first frame.** 99 ms with the system's
  shader caches warm and 134 ms with them cleared (`bdc99eb`, 2026-09-19).
- **Sitting idle: under half a percent of one CPU core.** 0.46% with a
  program that prints nothing (`e8fe895`, 2026-09-18).

## Taking in output, against other terminals

Measured at `e5bc3ef` on 2026-09-20 with `bench-others/multibench`: each
terminal's core fed the same recorded bytes, in memory, each keeping 10,000
lines of scrollback; medians of 9 runs, taken in turn. Megabytes per second, higher is better:

| recording | baud | ghostty | alacritty | baud vs ghostty |
|---|---|---|---|---|
| less, paging a file | **1684.5** | 290.2 | 241.2 | **5.80x** |
| cat, scrolling text | **1425.1** | 424.3 | 203.3 | **3.36x** |
| git log | **1637.9** | 516.9 | 211.5 | **3.17x** |
| less, Chinese, Japanese and Korean text | **498.0** | 247.2 | 271.3 | **2.01x** |
| cava | **554.1** | 318.9 | 362.2 | **1.74x** |
| cat, Chinese, Japanese and Korean text | **459.4** | 385.1 | 221.7 | **1.19x** |

[MEASUREMENTS.md](../../MEASUREMENTS.md#against-other-implementations-on-real-capture)
has this table's rows and their ranges.

Three recordings were captured again on 2026-10-28, and a new animation
took the place of an older one; measured the same way that day, at
`e9af53d9`, with the recording of paging a file beside them to show how
far baud's own figure has moved since the table above:

| recording | baud | ghostty | alacritty | baud vs ghostty |
|---|---|---|---|---|
| less, paging a file | **1450.0** | 292.6 | 247.7 | **4.96x** |
| [a system monitor](../../MEASUREMENTS.md#against-other-implementations-on-real-capture) | **879.1** | 516.4 | 355.5 | **1.70x** |
| vim, Unicode text | **600.7** | 357.4 | 300.2 | **1.68x** |
| pipes, an animation in eight colours | **503.0** | 304.6 | 316.5 | **1.65x** |

[MEASUREMENTS.md](../../MEASUREMENTS.md#against-other-implementations-on-real-capture)
has this table's rows and their ranges.

What this does and does not show:

- It compares the part of each terminal that interprets output, not the
  whole application drawing to a screen.
- baud always works out which characters belong together, such as an
  emoji sequence or a letter and its accent, and the core in the third
  column does only when a program asks, so the Chinese, Japanese and
  Korean rows do not compare equal work.
- The first table was taken before the benchmark pinned where the compiler
  places each function, which had moved one of baud's rows by as much as
  10%, and the second after it, so the two tables are not comparable: baud's
  row for paging a file reads 1684.5 in one and 1450.0 in the other.

On Linux, measured at `f231b87e` on 2026-11-15 the same way, in memory with
10,000 lines of scrollback each and the cores taken in turn, baud was ahead
of every other core measured on all four recordings, its range clear of
every other's each time, and 1.77 to 5.71 times as fast as the core in
the fourth column. Megabytes per second:

| recording | baud | ghostty | alacritty | baud vs ghostty |
|---|---|---|---|---|
| cat, scrolling text | **919.5** | 111.3 | 210.3 | **8.26x** |
| cat, Chinese, Japanese and Korean text | **346.2** | 161.8 | 195.3 | **2.14x** |
| less, paging a file | **1084.1** | 273.3 | 208.9 | **3.97x** |
| git log | **1052.8** | 152.8 | 184.4 | **6.89x** |

Both other cores ended on baud's screen on three of the four; on the
Chinese, Japanese and Korean recording they lay out an emoji sequence one
code point at a time where baud joins it into one character, so baud does
more work there, not less.
[MEASUREMENTS.md](../../MEASUREMENTS.md#against-other-implementations-on-real-capture-linux-x86_64-2026-11-15)
has the method and three more cores.

## Through a real terminal connection, against other terminals

Measured at `46f5e1c` on 2026-09-12: each terminal opened fresh at 200x50 in
Menlo 13, with a recording replayed twenty times into it through a pty, the
same connection a shell uses.
Throughput in MiB per second:

| recording | baud | alacritty 0.17.0 | kitty 0.48.2 | ghostty 1.3.1 | wezterm |
|---|---|---|---|---|---|
| [a system monitor](../../MEASUREMENTS.md#five-terminals-through-a-pty) | **88** | 72 | 63 | 71 | 30 |
| cat-scroll | **81** | 67 | 57 | 66 | 39 |
| cat-cjk | **72** | 59 | 30 | 62 | 10 |
| asciiquarium | **68** | 67 | 43 | 53 | 46 |
| vim-unicode | **42** | 38 | 30 | 35 | 21 |

The first, fourth and fifth rows are of the recordings as they were then;
the files were captured again, or replaced, on 2026-10-28, and this
comparison has not been run on the new ones.
baud leads by 16-22% on the first three rows; on the fourth its lead over
the terminal in the next column is inside the noise. macOS hands a terminal at most
1024 bytes at a time, so these rows are bounded by the operating system as
much as by any terminal: baud reached 89% of what the connection delivers to a
program that reads and discards. baud did not keep scrollback when this was
taken. [MEASUREMENTS.md](../../MEASUREMENTS.md#five-terminals-through-a-pty)
has the CPU time of each run too.

On Linux, baud draws on the GPU, through Vulkan, first, and on the CPU
when the GPU cannot be used or fails; it was measured both ways at
`8e404c38` on 2026-11-24, on an AMD Radeon RX 9070 XT. Each terminal opened
at 200x50 under a Wayland compositor, each recording was replayed into it
through a pty in six rounds with the first dropped, and the frames each
terminal submitted were counted. As each terminal ships, each at its own
font and size, in megabytes per second, not the MiB above, with the lowest
and highest frames submitted a megabyte across the four recordings:

| terminal | cat-scroll | cat-cjk | git-log | less-pager | frames a MB |
|---|---|---|---|---|---|
| baud, on the GPU | 93.7 | 111.3 | 116.8 | 98.7 | 0.61-0.74 |
| baud, on the CPU | 76.9 | 81.8 | 87.2 | 91.9 | 0.55-0.89 |
| ghostty 1.4.0 | 96.3 | 141.0 | 118.7 | 103.6 | 0.45-0.71 |
| alacritty 0.17.0 | 96.9 | 123.4 | 112.3 | 98.9 | 0.55-0.71 |
| foot 1.28.0 | 102.2 | 119.8 | 118.9 | 49.1 | 0.55-1.34 |
| kitty 0.50.0 | 94.1 | 101.6 | 116.4 | 106.0 | |
| wezterm | 28.8 | 36.7 | 30.0 | 22.0 | |

Drawing on the GPU, baud was ahead of baud drawing on the CPU on all four
recordings, their ranges apart. It was level with the terminal in the third
row on the scrolling text, the commit log and the paging recording, their
ranges overlapping, and behind it on the Chinese, Japanese and Korean text,
where that terminal took in 1.27 times as much and the fifth row's 1.08
times. baud was ahead of the fifth row's terminal on the paging recording,
by 2.0 times, and of the last row's on all four. With every terminal set to
DejaVu Sans Mono 13 and a cell within a pixel of baud's, baud on the GPU was
again level with the third row's terminal on three recordings and behind it
on the Chinese, Japanese and Korean text, by 1.41 times. Drawing on the CPU,
baud was level with the third row's terminal on the scrolling text and
behind it on the other three, by 1.13 to 1.72 times, and behind it on all
four at equal fonts.

Both ways are measured results: at exit each run counts where its frames
were drawn, and on the GPU every frame drawn after the GPU's pipelines were
built was drawn there.
[MEASUREMENTS.md](../../MEASUREMENTS.md#six-terminals-through-a-pty-both-drawing-arms-linux-x86_64-2026-11-24)
has the ranges, the run with equal fonts and how both were checked.

## Typing

Measured at `28f2382` on 2026-09-18, on the built-in 120 Hz display, medians
of 120 keys:

| from a key arriving to | with the system's echo | with the shell |
|---|---|---|
| the frame handed to the display | **3.0 ms** | **5.4 ms** |
| the display's next refresh, at the earliest | 7.1 ms | 11.3 ms |

The shell's extra time is the shell itself: the shell took 2.5 ms to echo
a key where the operating system's own echo took 0.015 ms.
No other terminal's typing latency has been measured on the Mac, so there
is no comparison there.

On Linux, measured on 2026-11-16 with baud built from `2789e0d0` and app
changes not yet published, on a 3840x2160 240 Hz display
under a tiling Wayland compositor, each terminal with the operating system
echoing the keys, medians of 90 keys from a key being sent to the
compositor committing the frame that shows it.
These figures are indicative, not a result, because baud was built with
app changes not in the repository:

| terminal | median |
|---|---|
| baud | **0.92 ms** |
| kitty 0.50.0 | 1.16 ms |
| foot 1.28.0 | 1.42 ms |
| alacritty 0.17.0 | 1.68 ms |
| ghostty 1.4.0 | 4.29 ms |
| wezterm | 8.37 ms |

Every terminal was read from outside, the same way. This ends at the
compositor's commit, not at the light leaving the panel, which comes up to a
refresh and a scan-out later for every terminal alike, so it is not
comparable with the Mac figures above. baud's slowest keys were slower than
those of the three terminals after it in the table: its 95th percentile
was 2.30 ms, where a
second copy of baud in the same run read 1.07 ms, so the run cannot tell
whether that tail is baud's.
[MEASUREMENTS.md](../../MEASUREMENTS.md#keystroke-to-the-compositors-commit-linux-x86_64-2026-11-16)
has the method and its checks.

## Starting up and idling

- **Launch** (`bdc99eb`, 2026-09-19): 99 ms from starting to the first frame
  with the system's shader caches warm, 134 ms with them cleared. baud ships
  its shaders compiled, which saved 256 ms on that cold launch.
- **Idle** (`e8fe895`, 2026-09-18): 0.46% of one core with nothing being
  printed.

## Memory

baud's screen storage is about 24 MB reserved but only +0.3 MB actually
used for a 50x200 screen fed data, because pages never touched are never
allocated. That figure was taken on baud's previous core, at `7b00e4c`
(2026-08-29), and is argued, not re-measured, for the current one.
No memory figure has been compared with another terminal.

## Not measured

- Memory and frame rate against other terminals, and typing latency
  against them on the Mac.
- Any discrete graphics card: every drawing figure on the Mac is its
  built-in GPU, and no Linux figure measures drawing on its own or names
  its graphics card.
- Any machine other than these two.
