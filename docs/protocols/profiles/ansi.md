# `TERM=ansi`

The generic ANSI/ECMA-48 entry — no vendor extensions.

VT100's CSI operations without the DEC private modes. If baud handles VT100 it handles this; the profile exists for terminals that are ANSI but not DEC.

## Status

| | |
|---|---|
| output capabilities | **63** — all handled |
| input capabilities | 8 — **none**; baud has no input path |
| shared with `vt100` | 33 |
| conformance test | `baud-core/tests/conformance/` → `ansi` |
| source | `../../research/sources/terminfo/ansi.ti`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*;
that half is not implemented and is counted rather than skipped.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
