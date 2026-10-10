# Architecture

The data path, the two core crates, and the constraints that shaped both, as
of 2026-10-10. Read this before changing anything in `baud-core/`.
[CLAUDE.md](../CLAUDE.md) holds the rules and [the record](record.md) every
measured change behind them.

## The data path

```
pty ──▶ embedder's ──▶ arena ──▶ graph ──▶ action ──▶ grid ring ──▶ frame copy
read(2)  buffer      write_at   one lookup   Paint     screen is     renderer
                                a character            a window
```

1. **The embedder reads; the core never does.** `baud-core` is `no_std` with no
   I/O. `baud-app` reads the pty into its own buffer, and `Term::write_at`
   copies each read into the arena at `BytePos::ZERO` (`baud-app/src/reads.rs`).
   `write_at` is the seam that would let a caller hand the kernel the arena
   itself; no caller does. The arena is 1 MiB (`ARENA`), a ceiling on one read.
2. **`Term::feed` parses the read in place, one table lookup a character.**
   The protocol is a graph compiled at build time (`baud-core/src/graph.rs`):
   an edge names the action and the next node, and the action says how many
   bytes it eats, so a three-byte UTF-8 character is one lookup. The nodes
   follow Williams' DEC ANSI parser ([why](research/notes/c-vt-parser.md)),
   plus the UTF-8 continuations a character split across two reads leaves
   owed and a disjoint VT52 sub-graph from `Node::Vt52Ground`. The node, the UTF-8
   accumulator and the CSI parameters live on `Term` between reads, which is
   why the screen does not depend on where the reads fell.
3. **A printable run is found by a vector scan.** `run_of_text`
   (`baud-core/src/write.rs`) takes four bytes scalar, then sixteen a step
   through `core::simd` -- or NEON from `baud-isa-aarch64` under `neon-scan`,
   the default, or AVX2 from `baud-isa-x86-64` under `avx2-scan` on a build
   that targets AVX2.
4. **Actions write through `Paint`**, the one borrow of the grid a read takes
   (`baud-core/src/paint.rs`); `write.rs` performs what the table named and
   `ops.rs` what a control sequence does. Every character becomes an
   eight-byte `PackedCell`, and its rendition is interned in the style table.
   There is one representation: the core always materialises cells.
5. **The grid is a ring and the screen is a window into it.** A full-screen
   scroll moves the window (`retire` in `baud-core/src/ops.rs`) and copies
   nothing, `Term::scrollback` reads the lines behind it, and `Term::resize`
   reflows the primary in place. The alternate screen is a ring of its own
   (`AltBuf`) and keeps no history. Rows sit at a fixed pitch from
   `grid_stride`, chosen from the cache geometry.
6. **What the core owes the host goes in `Storage.reply`**, 256 bytes
   (`REPLY`), drained with `Term::reply` and `Term::clear_reply`; an overflow
   is counted in `reply_overflows`.
7. **The frame is the embedder's.** On macOS `baud-app` copies the screen
   into a slot per frame in flight (research decision D4), and `baud-render`
   builds what `baud-present-macos` draws on Metal; the Linux driver has
   `baud-render` draw on the CPU.

## The two crates

**`baud-domain`** is the type system, the protocol vocabulary and the pure
functions over them -- the key encoder (`encode_key`), the Sixel decoder,
terminfo -- with no I/O and no core state. `#![no_std]`,
`#![forbid(unsafe_code)]`, zero dependencies beyond `zerocopy`. Every quantity
is a distinct type: a `Col` is not a `Row` is not a `ByteLen`, the arithmetic
between them is only what is declared (a `Col` plus a `Room`), and each gives
up its integer only through a named accessor -- `index()`, `get()`, `len()`
and their kin -- for interop.

**`baud-core`** is the machine. `#![no_std]` and `#![forbid(unsafe_code)]`,
unconditionally. The intrinsics live in `baud-isa-aarch64` and
`baud-isa-x86-64`, and `baud-core` reaches them only through their safe
wrappers, under the `neon-scan` (default) and `avx2-scan` features.

They are separate crates so the boundary is enforced by the compiler rather
than by discipline: `baud-domain` cannot reach into the core's state because it
does not know it exists.

Everything else -- the font stack, the renderer, the two presenters, the
Wayland and XKB crates and the app -- sits on these two and is listed in
[README's layout](../README.md#layout).

## Why each layer is filed differently

Three taxonomies, deliberately, because the layers answer different questions.
This looks inconsistent until you try to make them agree, and then it does not
survive contact with `Csi`.

| layer | filed by | example |
|---|---|---|
| `baud-domain/src/vt/` | **sequence family** | `csi.rs`, `osc.rs`, `dcs.rs`, `sgr.rs` |
| `baud-core/src/` | **responsibility** | `graph.rs` (the automaton), `paint.rs` (the grid), `osc.rs`, `sixel.rs` |
| `baud-core/tests/` | **protocol level** | `vt100/`, `vt220/`, `vt420/`, `xterm/` |
| `docs/protocols/` | **protocol level**, plus one page per `$TERM` profile | `vt100.md`, `vt220.md`, `profiles/rxvt.md` |

**The domain cannot be filed by protocol level.** `Csi` is a single enum
whose variants span every level at once: `CursorUp` is VT100,
`SelectProtection` is VT220, `SetSideMargins` is VT420, `RequestVersion` is
xterm, and `PrivateMode` carries modes from all four. Rust does not split an
enum across files, and splitting the *parser* by which DEC model introduced a
sequence would fragment one match arm set into four.

**The core is filed by responsibility** because that is where the cohesion is:
`graph.rs` is the automaton, `paint.rs` the one borrow of the grid a read
writes through, `ops.rs` what a control sequence does to the grid. EL (VT100)
and DECSEL (VT220) are one operation family, and separating them by vintage
would put the two halves of one bug in different files. They are not yet one
implementation: DECSEL takes its span from `line_span` in `ops.rs`, and EL's
`erase_line` in `paint.rs` spells the same arithmetic again.

**Tests and docs are filed by protocol level** because that is what a
specification *is*. "Does baud do VT220?" is the question a reader asks, and
`tests/vt220/` is the answer. Protocol level is a historical fact about an
operation — the right axis for a claim, the wrong one for an implementation.

**VT52 is the exception in every layer**, and it earns it: it is not an earlier
level of the same protocol but a different one whose bytes *conflict* — `ESC D`
is cursor-left there and IND here. That is why it has its own module in both
crates, its own ground state, and its own test directory.

## Constraints that look arbitrary and are not

Each cost something to learn. Changing one needs a measurement, not an
argument — and `CLAUDE.md` carries the fuller list plus what was tried and lost.

| thing | why |
|---|---|
| `READ_CHUNK = 4096` | `N_TTY_BUF_SIZE` caps `read(2)` at 4095 bytes regardless of buffer size (`[[source: linux-n-tty]]`); Darwin's clist caps it at 1024 (`[[source: xnu-tty-h]]`). Offering 1 MiB gets you 4095. |
| `PackedCell` is 8 bytes | 4 could not hold a codepoint: `U+1F680 & 0xffff` is `U+F680`. |
| All state behind `&mut self` | A C prototype from before the import is said to have run slower with file-scope globals; no harness, data or record survives. LLVM's `noalias` is the presumed mechanism, not a shown one. |
| `Content::EMPTY` is a reserved value, not `U+0020` | "Nothing written" and "a space was written" differ for selection, reflow and equality. Storing 32 for blank breaks copy/paste of trailing whitespace. |
| `StyleId::bucket` takes the **high** bits | SGR colours are multiples of `0x101010`; masking the low bits collided them. 112,144 interns at 51.6% hit → 0 interns at 100%. |
| `Col`/`Row` are `usize`-backed | A `u16` cursor adds zero-extension and masking to every op on x86-64, at a cost unmeasured here. Narrow types earn their place only where thousands are stored. |
| Bounded tables (`tmpl`, `links`) | Both key on what an application sends, so both are bounded. `tmpl` is sized to the screen and never frees a slot, so it evicts only once every slot is filled, and a cell still naming the evicted slot is recoloured -- an open bug. The link table refuses a collision and counts it in `links_refused`, because an evicted link is a wrong destination. |

## The two invariants

A core change answers both. Before changing anything on the ingest path:

1. **Does this write more bytes per input byte?** (write amplification)
2. **Does this enlarge what is touched per byte?** (working set ≤ cache)

If either is yes, it needs a measurement. **Allocation is a proxy for these,
not the thing**: "it doesn't allocate" is not a defence.

A cell core cannot hold write amplification to 1 and is not meant to: one
printable byte becomes an eight-byte `PackedCell`, so 8x is a floor by
construction ([the record](record.md), "The pure-ASCII rung is at 74% of the
machine's store bandwidth").
