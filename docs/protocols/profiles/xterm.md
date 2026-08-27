# `TERM=xterm`

The de-facto standard, and the source most other entries copy.

The largest entry: alternate screen, mouse, bracketed paste, OSC, DECRQM. See [../xterm.md](../xterm.md).

## Status

| | |
|---|---|
| output capabilities | **83** — all handled |
| input capabilities | 88 — **none**; baud has no input path |
| shared with `vt100` | 43 |
| conformance test | `baud-core/tests/conformance/` → `xterm` |
| source | `../../research/sources/terminfo/xterm.ti`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*;
that half is not implemented and is counted rather than skipped.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
