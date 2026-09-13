# F — Where a retired line lives: the grid as a ring, checked against the field

**The question.** A terminal's screen scrolls once every few hundred bytes of
output, and each scroll retires one row. Where that row goes, what it costs
to put it there, and what it costs to have it there are the whole of a
scrollback design. baud built one (`baud-core`, 2026-09-13), measured it, and
this note checks the result against what the shipping terminals do, so the
choice rests on the field and the counters rather than on what anyone in the
conversation wanted.

## What baud measured, in one paragraph

The screen is a window of `rows` into a ring of `ring` physical rows; a scroll
advances the rotation by one and blanks the row that comes into view, and the
row that left the top stays where it was written. Nothing is copied. The
hardware counters on a pure scrolling payload say the same thirteen L1 misses
happen on every scroll at any depth of history -- the row the window advances
into was last touched `ring` scrolls ago -- and that what varies is where they
are served from: L2 at fifty lines, past L2 at two thousand, where the same
misses cost ten times the stall. At the searched pitch of 13.4 KB a row that
put 2,000 lines at 27 MB and cost 0.56x on `flat`; at the width's own pitch,
3.3 MB, it costs 0.92x. A dense ring beside the grid that a retiring row was
*copied* into cost 0.55x at every ring size, because the copy lands on the
same cold lines. `CLAUDE.md`'s scrollback section carries the tables.

## What the field does

| terminal | structure for retired lines | a scroll does | memory per line | reclaimed |
|---|---|---|---|---|
| **alacritty** | `Storage<T>`: one `Vec<Row<T>>` holding scrollback and screen, a `zero` offset, "the `rotate` and `rotate_down` functions are fast modular additions on the internal `zero` field" (`[[source: alacritty-storage]]`) | moves `zero`; "rows never move" | a `Vec` of cells per row, at the column count (`[[source: alacritty-row]]`) | `len` shrinks without truncating; the raw buffer is truncated only past a 1,000-line cache |
| **foot** | `rows[]`, a power-of-two ring: `grid_row_absolute` is `(offset + row_no) & (num_rows - 1)`; scrollback and screen are the same array, with a separate `view` offset for the scrolled-back window (`[[source: foot-grid]]`) | advances `offset` | one `struct row` per slot, **allocated when first written** | never freed while the ring lives; a slot is reused when the ring wraps |
| **ghostty** | `PageList`: pages from a pool, each a contiguous buffer of rows and cells; "the screen starts with a single page that is the entire viewport, and we'll split it thereafter if it gets too large" (`[[source: ghostty-pagelist]]`) | appends a row to the current page, a new page from the pool when full; "normal scrolling never copies rows" | dense at the page's columns (`[[source: ghostty-page]]`) | the oldest *page* is evicted against `max_size` and `max_lines`; cold pages can be compressed and restored on demand |
| **kitty** | a `LineBuf` for the screen and a `HistoryBuf` behind it: segments of 2,048 lines at full width, a ring over them (`[[source: kitty-history]]`) | **copies** the retiring line into history (`historybuf_add_line` -> `copy_line`); the oldest line is pushed to `pagerhist` as ANSI text when the ring is full | full `xnum` columns a line, plus a GPU cell array | the ring overwrites; `pagerhist` keeps an escaped-text tail |
| **tmux** | one `linedata` array, history at `0..hsize` and the screen after it; "lines are not allocated until cells in that line are written to" (`[[source: tmux-grid]]`) | `realloc`s the array one line longer and marks the old top line history | dense `grid_cell_entry`, extended cells out of line | `grid_collect_history` frees the oldest tenth and `memmove`s the rest up |

Three of the five -- alacritty, foot, ghostty -- do exactly what baud does:
the retired row stays where it was written and an index moves. kitty copies,
which is the design baud built first and withdrew on measurement. tmux
reallocates, which is a copy of the *pointer* array rather than of cells.

## What the field cannot tell baud, because none of them measured it

**Nobody avoids the miss.** Every structure above writes the retiring row --
or the row that replaces it -- into memory the screen was not writing a
moment ago: the next slot of a ring, the next row of a page, the next segment
of a history buffer. The counters say that write is what a scroll costs with
history and without it the row is the one that just left, still in L1. So the
question the literature answers is not "how to make retention free" but "how
to keep the misses cheap", and the answer in every design is **footprint**: a
ring past L2 is served from memory, a ring inside it from L2, and the
difference on this box is 0.56x against 0.92x for the same lines.

Where the field is ahead of the first version of baud's ring is exactly
there, and baud has taken it:

* **Dense per-line memory.** ghostty's pages, alacritty's per-row `Vec` and
  tmux's `grid_cell_entry` all store a line at or near its width. baud's ring
  at the searched pitch stored every line at 8x the corpus width, for a
  set-spreading property the ring made irrelevant; the pitch is the width's
  lines made odd now, and that alone was the 0.56x to 0.92x.
* **A bound that keeps the ring where the misses are cheap.** ghostty bounds
  pages by bytes as well as lines. baud's `Depth::DEFAULT` of 2,000 lines is
  3.3 MB at the corpus width and 6.4 MB at the app's, both inside this box's
  L2, and a caller who wants more pays what the depth sweep prices.

And where it is not, stated so it is not re-proposed as an improvement:

* **Lazily allocated rows** (foot, tmux) save memory for rows never written.
  baud's storage is caller-provided and fixed by design -- `no_std`, no
  allocator -- so every row is provisioned up front, and the pitch rule is
  what keeps that cheap.
* **Copying on retire** (kitty) buys a screen buffer that stays hot at the
  cost of a copy that measured 0.55x here and lands on the same cold lines a
  ring would. It is not a trade; it is the same cost plus a copy.
* **Compression of cold history** (ghostty pages, kitty's `pagerhist`)
  matters when history is meant to exceed cache by orders of magnitude. At
  2,000 lines in a few megabytes there is nothing to compress yet; it is the
  lever to reach for if the depth is ever raised past L2, and the depth
  sweep is the instrument that would say so.
* **Reflow on resize** copies in every design that has it -- ghostty's
  `ReflowCursor` is "the only bulk row-copying operation". baud has no
  reflow yet; when it comes, the wrap flag beside the row marks is the
  input, and a copy on resize is what the field pays and what a resize can
  afford, once, where a scroll cannot, every two hundred bytes.

## What the research says, and it had the answer before the terminals did

The terminals are the practice; four papers are the reasons, and each one
names a thing the measurements found without having read it.

**The miss on the incoming row is a write miss, and the fetch is the cost.**
`[[source: jouppi-write-policies]]` § 4 separates the policies a cache can
take on a write miss -- fetch-on-write, write-validate, write-around,
write-invalidate -- and measures that "write misses account for about
one-third of all cache misses" across its benchmarks, so "stores are about
as likely to cause a miss as loads". Its block-copy example is baud's scroll
exactly: under fetch-on-write "the original contents of the target of the
copy will be fetched even though they are never used and are only
overwritten with write data", which "would have only two-thirds of the
performance on large block copies as a no-fetch-on-write policy since half
of the items fetched would be discarded". The blank of the incoming row
overwrites a whole line and reads none of it; every one of the thirteen
misses a scroll is a fetch of data about to be discarded. Jouppi's answer
is write-validate in the cache or a cache-line-allocate instruction in the
ISA, "limited to situations where new data spaces are being allocated" --
which a scroll's incoming row is.

**That instruction exists on aarch64, and baud cannot use it as the cell is
encoded.** `DC ZVA` zeroes a cache line by virtual address without fetching
it, and `STNP` stores a pair with a non-temporal hint. Both are named here
without a pinned source: the Arm developer pages are rendered by script and
the reference manual was not fetched, so the claim is the instruction's
existence and not its measured behaviour on this core. What stops the
experiment is not the pin. `Content::EMPTY` is a reserved value above
`U+10FFFF` chosen so that a blank cell is not a space -- a measured design
this repo keeps -- and a zeroed line is a row of `U+0000`, which is a
character. A blank the hardware can produce without a fetch would have to
be all-zero, which means the empty encoding would have to move; that is a
`PackedCell` change with a cost on every path that tests for the blank, and
it is the one lever the write-miss literature points at that has not been
priced. It is recorded as the next arm, not built.

**The software prefetch that lost is the paper's negative case.**
`[[source: lee-prefetching]]` § 3.2 lists the negative impacts of software
prefetching: "software prefetch instructions consume fetch and execution
bandwidth and machine resources", the distance and the decision are static
and "cannot adapt to runtime behavioral changes", and "when the number of
instructions in a loop is extremely small, it can be challenging to insert
software prefetching instructions to provide timely prefetching requests
because there are not enough computations between prefetching instructions
and demand requests". A scroll on `cat` is a few dozen instructions apart
from the next; thirteen prefetches between them is the density the paper
says cannot be placed, and the measurement -- the loss scaling with lines
hinted, a single line neutral, thirteen a third of the clock -- is § 3.2 (1)
and (3) together. § 3.4's antagonistic training, "software prefetch requests
can slow down the hardware prefetcher training", is the mechanism left
unmeasured here; it would take the counters, and the arm is kept for it.

**The pitch that stacked the sets is Lam's self-interference.**
`[[source: lam-blocked]]` § 1 attributes the swings in blocked matrix
performance to "interference misses in the cache": "the address mapping may
map multiple rows of a matrix to the same cache lines, making it infeasible
to try to fully use the cache", and § 3 defines self interference, the
reuse of one array defeated by its own rows landing on one set. A grid
strided by `MAX_COLS` is a matrix whose row length is one way of the L1, the
pathology in its purest form, and the one-line pad past it is Lam's rows
"periodically" sharing a location. The odd-lines rule is the paper's array
padding, and `CLEAR` is the distance the measurement found the padding
needs.

**A ring of rows is a log, and the field's structures are its segments.**
`[[source: lfs]]` § 1 writes "all modifications to disk sequentially in a
log-like structure", divides "the log into segments", and reclaims with "a
segment cleaner" -- because a write that goes forward never reads what it
replaces. The grid as a ring is a log of fixed-size records with no cleaner,
since every record is the same size and the oldest is simply overwritten;
ghostty's pages are the segments, evicted whole; kitty's `pagerhist` is a
cleaner that rewrites a segment smaller. What LFS says about the write is
what Jouppi says: the log wins because it writes forward and never fetches
what it overwrites -- on a disk. On a cache that fetches on write, the
forward write still fetches, and that is the thirteen misses.

## The decision

The grid as a ring stands: it is the structure alacritty and foot ship, it is
what ghostty does inside a page, and it is the one the counters picked over a
copy. The two things the survey changed are already in: the ring's rows are
dense, and its default depth sits inside L2. The viewport onto it is a view
over the same memory -- rows through the rotation, an index at the pitch --
which is foot's `view` offset and alacritty's `compute_index`, not a copy.
