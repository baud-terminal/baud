# `TERM=dtterm`

CDE's terminal, from the commercial Unix era.

VT100 plus a subset of xterm. Notable for `?45` reverse wraparound and `ESC SP F` (S7C1T), which is why those are implemented.

## Status

| | |
|---|---|
| output capabilities | **58** — all handled |
| input capabilities | 32 — **none**; baud has no input path |
| shared with `vt100` | 38 |
| conformance test | `baud-core/tests/conformance/` → `dtterm` |
| source | `../../research/sources/terminfo/dtterm.ti`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*;
that half is not implemented and is counted rather than skipped.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
