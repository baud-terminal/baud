# Documentation

What each document is for, and who should read it. This index exists because
its absence is why the README grew to 354 lines: with nowhere to point, every
question got answered inline.

## Start here

| | for | read when |
|---|---|---|
| [architecture.md](architecture.md) | contributors | before touching code — the data path and the constraints that shaped it |
| [protocols/](protocols/) | anyone | **start here for a protocol** — one document per level (VT52, VT100, VT220, VT420, xterm), mirroring the code and tests |
| [vt-coverage.md](vt-coverage.md) | anyone | checking whether a specific operation works. One row per operation |
| [protocol-decisions.md](protocol-decisions.md) | contributors | something looks like a gap and you want to know if it is deliberate |
| [../MEASUREMENTS.md](../MEASUREMENTS.md) | sceptics | checking a number. Every table stamped with the host that produced it |
| [../CLAUDE.md](../CLAUDE.md) | contributors and agents | before proposing an optimisation — it lists what was tried and lost |
| [../BUILD.md](../BUILD.md) | contributors | setting up, running the suites |

## Research

`research/sources/` holds the primary sources every protocol claim cites:
xterm's `ctlseqs`, Paul Flo Williams' DEC ANSI parser, ECMA-48, the VT510
programmer reference, and the papers behind the SIMD and FSM work. Each is
recorded in [`MANIFEST.toml`](research/sources/MANIFEST.toml) with a SHA-256,
and `scripts/check-sources.sh` re-hashes them, so a citation to a key the
manifest does not carry — or one recorded as `gated`, meaning never obtained —
fails the build.

`research/notes/` is where a source gets turned into an argument about baud.

**It is 1/6 built, and that is a known gap rather than an oversight.**
`MANIFEST.toml` declares six groups and the notes directory holds one:

| group | topic | note |
|---|---|---|
| A | SIMD parsing and vectorised classification | missing |
| B | data-parallel finite-state machines | [b-data-parallel-fsm.md](research/notes/b-data-parallel-fsm.md) |
| C | the terminal parser as specified | [c-vt-parser.md](research/notes/c-vt-parser.md) |
| D | memory hierarchy and write amplification | missing |
| E | the tty layer and syscall batching | missing |
| F | text sequence data structures | missing |

A, D and E each underpin a claim made elsewhere without a note to cite: A is
the SIMD scanner argument, D is the write-amplification thesis the whole
project rests on, E is the tty-tax story. They are listed here so the gap is
visible rather than implied by an empty directory.

## The rule these documents follow

A claim is either mechanically checked or it is not made.

- `vt-coverage.md`'s **done** rows are checked by
  `baud-core/tests/coverage.rs`.
- The README's Rust example is `baud-core/examples/quickstart.rs`, compiled by
  the gate, with `baud-core/tests/readme.rs` asserting they have not drifted.
- Every source citation is checked by `scripts/check-sources.sh`, which
  re-hashes the file behind each key.
- Type-system claims are checked by `audit_all.sh`.

This rule is written down because it was broken: `vt-coverage.md` claimed for
most of the project's life to be verified by a `tests/coverage.rs` that did not
exist, in the same paragraph where it warned against documentation claiming
coverage it does not have.
