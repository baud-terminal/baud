# Architecture

The data path, the two crates, and the constraints that shaped both. Read this
before changing anything in `baud-core/`.

**The data path below is `baud-core2`'s, deleted in bcefaae (2026-09-10).**
`baud-core` has one representation -- it always materialises cells, and the
grid is a ring the screen is a window into -- forbids `unsafe`, and takes its
run scan by build feature rather than by a choice at startup. The data path is
kept as that core's until someone writes this one's. The two crates, the filing
table, the constraints and the invariants below were re-read against the code
on 2026-10-08, and [CLAUDE.md](../CLAUDE.md) and [the record](record.md) hold
the current design and its history.

Much of what follows lived only in `CLAUDE.md`, a file addressed to agents. A
human contributor could not learn why `PackedCell` is eight bytes without
opening it.

## The data path, as `baud-core2` had it

```
read(2) ──▶ arena ──▶ scan ──▶ parse ──▶ runs or cells ──▶ viewport
            1 MiB     SIMD     state       storage          projection
                               machine
```

1. **`read(2)` writes directly into the arena.** Not into a scratch buffer that
   is then copied — the arena *is* the read destination, which is what keeps
   write amplification at 1. `Term::read_slot()` hands out the destination;
   `commit_read` says how much arrived.
2. **The scan finds the next interesting byte** — end of a printable run, a CSI
   final byte, a string terminator — vectorised. Three backends behind one
   trait (`scan/`), chosen once at startup because `no_std` has no runtime
   feature detection.
3. **The parser** is Williams' 13-state DEC ANSI machine plus a `Utf8` state
   ([why](research/notes/c-vt-parser.md)). A fast path handles whole sequences
   that lie inside one read; anything crossing a read boundary falls back to
   the byte-at-a-time machine, which is what makes chunk-invariance hold.
4. **Storage is one of two shapes**, chosen by `OpMode` at startup:
   - `Runs` keeps byte *spans* into the arena — the text is never copied.
   - `Computed`/`Templated` materialise `PackedCell`s.
5. **The viewport is a projection, not storage.** It is derived from the lines;
   it is never the source of truth.

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
