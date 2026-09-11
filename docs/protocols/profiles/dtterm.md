# `TERM=dtterm`

CDE's terminal, from the commercial Unix era.

VT100 plus a subset of xterm. Notable for `?45` reverse wraparound and `ESC SP F` (S7C1T), which is why those are implemented.

## Status

| | |
|---|---|
| output capabilities | **58** — all handled |
| input capabilities | 32 — **25 sent**, measured against the pinned terminfo by `the_profiles_input_counts_are_what_baud_can_send` |
| shared with `vt100` | 38 |
| conformance test | `baud-core/tests/conformance/` → `dtterm` |
| source | `../../research/sources/terminfo/dtterm.ti`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*, and that half is
implemented: `Term::key` encodes them. The count is **measured** rather than
asserted -- every capability in the pinned terminfo is expanded to bytes and
intersected with everything the encoder can produce, so no name is mapped to a
key by hand.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
