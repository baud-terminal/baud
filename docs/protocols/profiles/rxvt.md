# `TERM=rxvt`

rxvt and its descendants (urxvt, mrxvt).

VT100 plus much of xterm. Its reset string is `CSI ? 1 ; 3 ; 4 ; 6 l` — a multi-parameter DECRST, which baud handled only the first parameter of until this profile was tested.

## Status

| | |
|---|---|
| output capabilities | **66** — all handled |
| input capabilities | 99 — **27 sent**, measured against the pinned terminfo by `the_profiles_input_counts_are_what_baud_can_send` |
| shared with `vt100` | 39 |
| conformance test | `baud-core/tests/conformance/` → `rxvt` |
| source | `../../research/sources/terminfo/rxvt.ti`, `infocmp -x -1`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*, and that half is
implemented: `Term::key` encodes them. The count is **measured** rather than
asserted -- every capability in the pinned terminfo is expanded to bytes and
intersected with everything the encoder can produce, so no name is mapped to a
key by hand.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
