# `TERM=rxvt`

rxvt and its descendants (urxvt, mrxvt).

VT100 plus much of xterm. Its reset string is `CSI ? 1 ; 3 ; 4 ; 6 l` — a multi-parameter DECRST, which baud handled only the first parameter of until this profile was tested.

## Status

| | |
|---|---|
| output capabilities | **66** — all handled |
| input capabilities | 77 — **none**; baud has no input path |
| shared with `vt100` | 39 |
| conformance test | `baud-core/tests/conformance/` → `rxvt` |
| source | `../../research/sources/terminfo/rxvt.ti`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*;
that half is not implemented and is counted rather than skipped.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
