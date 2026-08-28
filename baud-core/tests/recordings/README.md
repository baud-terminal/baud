# recordings

Real escape streams, captured once from real programs through a real PTY, and
committed as fixed input.

Every payload baud had been tested and measured against before these was
written by the same people who wrote baud. `baud-bench/src/payload.rs` says so
in its own header: the synthetic payloads want validating against real capture
"before it is designed around". These are that capture.

| file | program | bytes | what it exercises that the others do not |
|---|---|---|---|
| `btop.raw` | btop, 45x160 | 524288 | truecolour `38;2`, box drawing, dense full-screen repaint, SGR mouse modes |
| `asciiquarium.raw` | asciiquarium, 45x160 | 335479 | curses animation: heavy cursor addressing with 16-colour SGR |
| `vim-unicode.raw` | vim on a generated UTF-8 file, paged with `^F`/`^B` | 138571 | scroll regions (DECSTBM), and CJK, emoji, ZWJ, RTL and combining marks |
| `cat-scroll.raw` | `cat` of 437 KB of Rust source, 50x200 | 448460 | **scrolling**, and text with no escapes at all: 232,951 scrolls, zero dispatches |
| `git-log.raw` | `git --no-pager log --stat --color`, 50x200 | 501185 | colour over scrolling text: 171,091 scrolls with an SGR every 141 bytes |
| `less-pager.raw` | `less -R` paged forwards and back with space/`b`/`G`/`g` | 30146 | full-screen repaint by erase: **213 cells written per input byte** |

**The last three were added because `Term::scrolls` said the first three do not
scroll.** btop scrolls 0 times, vim 0, asciiquarium 20 -- while blanking is 73%
of `payload::streaming`'s cell writes and `scroll_region_up` is 11% of its
profile. Every decision this project has made about scrolling and blanking
rested on a payload written by the same people who wrote the code, which is the
exact thing this directory exists to stop.

They also cover the case baud is *fastest* at and had no recording of: `cat`,
logs and build output are what `payload::streaming` models and what the spans
representation wins on, and until `cat-scroll.raw` the headline rested on a
synthetic file.

`less-pager.raw` is the outlier worth keeping for its own sake: 28.8 MB/s
against `cat`'s 824.8, because a pager repaints the screen by erasing it and
each erase writes 10,000 cells for about four bytes of input. It is the slowest
workload baud has, by thirty times.

`scripts/capture-tui.py` is how they were made and is kept for provenance, not
because CI runs it. **A capture is not reproducible**: btop draws live CPU
figures, so a second run of the same command produces different bytes and a
different hash. That is why these are committed rather than regenerated, and
why `replay.rs` pins each file's length and checksum -- a recording that
changes is a recording that is no longer the thing the assertions were written
against.

`btop.raw` is the first 512 KiB of a 2.48 MB capture. The whole thing was 10
seconds and 147,492 escape sequences; the tail is more of the same, and a
megabyte of it in git buys nothing a test can use.

They are fixtures, deliberately not entries in
`docs/research/sources/MANIFEST.toml`. That manifest is for *specifications*
that a claim can cite, with a `url` and a retrieval status. A recording is
neither retrieved nor something to cite -- it is input.
