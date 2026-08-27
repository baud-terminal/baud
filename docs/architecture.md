# Architecture

The data path, the two crates, and the constraints that shaped both. Read this
before changing anything in `baud-core/`.

Much of what follows lived only in `CLAUDE.md`, a file addressed to agents. A
human contributor could not learn why `PackedCell` is eight bytes without
opening it.

## The data path

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

**`baud-domain`** is the type system and nothing else. No I/O, no state
machine, `#![no_std]`, `#![forbid(unsafe_code)]`, zero dependencies beyond
`zerocopy`. Every quantity is a distinct type: a `Col` is not a `Row` is not a
`ByteLen`, and none of them exposes its integer.

**`baud-core`** is the machine. `#![no_std]`, and `unsafe` appears in exactly
one place — `scan_avx2`, which takes a shared slice and returns before any
`&mut self` call.

They are separate crates so the boundary is enforced by the compiler rather
than by discipline: `baud-domain` cannot reach into the core's state because it
does not know it exists.

## Constraints that look arbitrary and are not

Each cost something to learn. Changing one needs a measurement, not an
argument — and `CLAUDE.md` carries the fuller list plus what was tried and lost.

| thing | why |
|---|---|
| `ARENA = 1 MiB` | A sweep peaked at 1 MiB against a 2 MiB L2 and went **negative** by 64 MiB. Bigger is worse. |
| `READ_CHUNK = 4096` | `N_TTY_BUF_SIZE` caps `read(2)` at 4095 bytes regardless of buffer size. Offering 1 MiB gets you 4095. |
| `PackedCell` is 8 bytes | 4 could not hold a codepoint: `U+1F680 & 0xffff` is `U+F680`. The extra 4 B cost 2 points of ceiling — 48% against 50%. |
| All state behind `&mut self` | A C prototype with file-scope globals ran **16–34% slower**; `__restrict` recovered only 2.5%. Presumed LLVM `noalias`. |
| `Content::EMPTY` is a reserved value, not `U+0020` | "Nothing written" and "a space was written" differ for selection, reflow and equality. Storing 32 for blank breaks copy/paste of trailing whitespace. |
| `StyleId::bucket` takes the **high** bits | SGR colours are multiples of `0x101010`; masking the low bits collided them. 112,144 interns at 51.6% hit → 0 interns at 100%. |
| `Col`/`Row` are `usize`-backed | A `u16` cursor costs 11–15% on x86-64 in zero-extension and masking. Narrow types earn their place only where thousands are stored. |
| Bounded, evicting tables (`tmpl`, `links`) | Both key on attacker-supplied content. A table that grows without limit walks out of cache and then out of memory. |

## The two invariants

Every result traces to these. Before changing anything on the ingest path,
answer both:

1. **Does this write more bytes per input byte?** (write amplification ≤ 1)
2. **Does this enlarge what is touched per byte?** (working set ≤ cache)

If either is yes, it needs a measurement. **Allocation is a proxy for these,
not the thing** — an immutable flat log that allocated nothing still cost 22%,
because the array grew past cache. "It doesn't allocate" is not a defence.

## One trap an embedder hits immediately

In `OpMode::Runs`, `viewport()` reports **representation, not rendered
content**. It reflects only promoted lines, and `MAXRUN` is 8 — so 112
single-byte reads promote a line that one read does not, and the viewport
differs between the two while the *rendered text* is identical.

Assertions about rendered content belong in `Computed`. This is not a bug; it
is what span storage means. It is written here because it has produced false
test failures more than once.
