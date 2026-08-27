# `TERM=xterm-16color`

`xterm`, with aixterm's bright colours as a colour formula.

Differs from `xterm` in **five** capabilities, all colour: `setaf`, `setab`, `setf`, `setb`, `initc`. Same protocol.

## Status

| | |
|---|---|
| output capabilities | **84** — all handled |
| input capabilities | 88 — **none**; baud has no input path |
| shared with `vt100` | 43 |
| conformance test | `baud-core/tests/conformance/` → `xterm_16color` |
| source | `../../research/sources/terminfo/xterm-16color.ti`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*;
that half is not implemented and is counted rather than skipped.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
