# Performance

baud is built to be fast at the four things a terminal does all day: taking
in a program's output, answering a key, drawing frames while the screen
churns, and doing it in little memory. This page gives the headline numbers
in plain terms. [MEASUREMENTS.md](../../MEASUREMENTS.md) is the full record,
with every table's method and, under Reproducing, how to re-run them.

Every figure here was taken on one Mac, an Apple M5 Max running macOS 27.0,
unless it is marked Linux: those were taken on one PC, an AMD Ryzen
Threadripper 9980X running Linux 7.2.6 under NixOS. A figure from one machine
is not comparable with a figure from the other. Every figure carries the
commit and date it was taken at. A figure is not current just because it is
printed: code has changed since most of them.

## In short

- **Taking in output: faster than ghostty and alacritty on every recording
  measured.** baud's core was 1.19 to 5.80 times as fast as ghostty's on
  six recordings of real programs, in memory (`e5bc3ef`, 2026-09-20), and
  1.65 to 1.70 times on three captured later (`e9af53d9`, 2026-10-28); the
  two sweeps are not comparable with each other. On Linux it was 2.14 to
  8.26 times as fast as ghostty's on four (`f231b87e`, 2026-11-15).
- **Through a real terminal connection: the fastest of five terminals on
  the Mac, and not the fastest on Linux.** On the Mac,
  replaying recordings into baud, alacritty, kitty, ghostty and wezterm,
  baud had the highest throughput on every recording, tied
  with alacritty on one, and used the least CPU, or tied for least, on all
  but one (`46f5e1c`, 2026-09-12; before baud kept scrollback). On Linux,
  drawing on the CPU, baud took in 77.9-98.1 MB/s as each terminal ships,
  behind the terminal fastest on cat-cjk and less-pager on all four
  recordings, which took in 1.2 to 2.0 times as much, and ahead of two
  terminals on less-pager and of one of them on all four; drawing
  on the GPU, its first choice on Linux, it took in 40.2-63.9 MB/s, an
  indicative figure (`7c5f994d`, 2026-11-17,
  [MEASUREMENTS.md](../../MEASUREMENTS.md#six-terminals-through-a-pty-both-drawing-arms-linux-x86_64-2026-11-17)).
- **Answering a key: about 3 ms to a new frame on the Mac.** From a key
  arriving to the new frame being handed to the display took a median of
  3.0 ms when the program echoing it was `cat` (`28f2382`, 2026-09-18). On
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

Three recordings were captured again on 2026-10-28, and pipes took the
place of asciiquarium; measured the same way that day, at `e9af53d9`, with
less paging a file beside them to show how far baud's own figure has moved
since the table above:

| recording | baud | ghostty | alacritty | baud vs ghostty |
|---|---|---|---|---|
| less, paging a file | **1450.0** | 292.6 | 247.7 | **4.96x** |
| [a system monitor](../../MEASUREMENTS.md#against-other-implementations-on-real-capture) | **879.1** | 516.4 | 355.5 | **1.70x** |
| vim, Unicode text | **600.7** | 357.4 | 300.2 | **1.68x** |
| pipes, an animation in eight colours | **503.0** | 304.6 | 316.5 | **1.65x** |

What this does and does not show:

- It compares the part of each terminal that interprets output, not the
  whole application drawing to a screen.
- baud always works out which characters belong together, such as an
  emoji sequence or a letter and its accent, and ghostty's core does only
  when a program asks, so the Chinese, Japanese and Korean rows do not
  compare equal work.
- The first table was taken before the benchmark pinned where the compiler
  places each function, which had moved one of baud's rows by as much as
  10%, and the second after it, so the two tables are not comparable: baud's
  less row reads 1684.5 in one and 1450.0 in the other.

On Linux, measured at `f231b87e` on 2026-11-15 the same way, in memory with
10,000 lines of scrollback each and the cores taken in turn, baud was ahead
of every other core measured on all four recordings, its range clear of
every other's each time, and 1.77 to 5.71 times as fast as alacritty's.
Megabytes per second:

| recording | baud | ghostty | alacritty | baud vs ghostty |
|---|---|---|---|---|
| cat, scrolling text | **919.5** | 111.3 | 210.3 | **8.26x** |
| cat, Chinese, Japanese and Korean text | **346.2** | 161.8 | 195.3 | **2.14x** |
| less, paging a file | **1084.1** | 273.3 | 208.9 | **3.97x** |
| git log | **1052.8** | 152.8 | 184.4 | **6.89x** |

alacritty and ghostty ended on baud's screen on three of the four; on the
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

The system monitor, asciiquarium and vim-unicode rows are of the recordings
as they were then; the files were captured again, or replaced, on 2026-10-28,
and this comparison has not been run on the new ones.
baud leads by 16-22% on the system monitor, cat-scroll and cat-cjk; on asciiquarium its
lead over alacritty is inside the noise. macOS hands a terminal at most
1024 bytes at a time, so these rows are bounded by the operating system as
much as by any terminal: baud reached 89% of what the connection delivers to a
program that reads and discards. baud did not keep scrollback when this was
taken. [MEASUREMENTS.md](../../MEASUREMENTS.md#five-terminals-through-a-pty)
has the CPU time of each run too.

On Linux, baud draws on the GPU, through Vulkan, first, and on the CPU
when the GPU cannot be used or fails; it was measured both ways at
`7c5f994d` on 2026-11-17. Each terminal opened at 200x50 under a Wayland
compositor, each recording was replayed into it through a pty in six rounds
with the first dropped, and the frames each terminal submitted were
counted. As each terminal ships, each at its own font and size, in
megabytes per second, not the MiB above, with the lowest and highest frames
submitted a megabyte across the four recordings:

| terminal | cat-scroll | cat-cjk | git-log | less-pager | frames a MB |
|---|---|---|---|---|---|
| baud, on the CPU | 79.2 | 77.9 | 96.1 | 98.1 | 0.50-0.87 |
| baud, on the GPU (indicative) | 56.2 | 40.2 | 58.5 | 63.9 | 1.05-1.55 |
| ghostty 1.4.0 | 103.4 | 157.5 | 133.1 | 118.5 | 0.39-0.66 |
| alacritty 0.17.0 | 100.1 | 131.3 | 120.8 | 109.5 | |
| foot 1.28.0 | 110.8 | 132.6 | 134.8 | 53.5 | 0.50-1.23 |
| kitty 0.50.0 | 95.2 | 108.7 | 127.9 | 108.2 | |
| wezterm | 29.2 | 39.1 | 30.9 | 24.0 | |

Drawing on the CPU, baud was behind the terminal fastest on cat-cjk and
less-pager on all four recordings, their ranges apart: it took in 1.2
times as much on less-pager, 1.3 on cat-scroll, 1.4 on git-log and 2.0 on
cat-cjk. baud was ahead of two terminals on less-pager and of one of them
on all four. With every terminal set to DejaVu Sans Mono 13 and a cell
within a pixel of baud's, baud on the CPU was again behind that terminal on
all four, ranges apart, at 1.2 to 1.8 times its rate.

The figures drawing on the GPU are indicative, not a result: the build
measured does not count, at exit, the frames drawn on the GPU, so which way
each frame was drawn is not checked. In this run baud on the GPU was behind
baud on the CPU on all four recordings and submitted about twice the frames
a megabyte, both indicative.
[MEASUREMENTS.md](../../MEASUREMENTS.md#six-terminals-through-a-pty-both-drawing-arms-linux-x86_64-2026-11-17)
has the ranges, the run with equal fonts and how both were checked.

## Typing

Measured at `28f2382` on 2026-09-18, on the built-in 120 Hz display, medians
of 120 keys:

| from a key arriving to | with `cat` | with the shell |
|---|---|---|
| the frame handed to the display | **3.0 ms** | **5.4 ms** |
| the display's next refresh, at the earliest | 7.1 ms | 11.3 ms |

The shell's extra time is the shell itself: zsh took 2.5 ms to echo a key
where the operating system's own echo for `cat` took 0.015 ms.
No other terminal's typing latency has been measured on the Mac, so there
is no comparison there.

On Linux, measured on 2026-11-16 with baud built from `2789e0d0` and app
changes not yet published, on a 3840x2160 240 Hz display
under Hyprland 0.56.2, each terminal running `cat`, medians of 90 keys from
a key being sent to the compositor committing the frame that shows it.
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
kitty's, foot's and alacritty's: its 95th percentile was 2.30 ms, where a
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
