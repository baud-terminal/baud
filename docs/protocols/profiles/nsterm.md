# `TERM=nsterm`

Apple Terminal.app's own entry — what macOS sets by default.

VT100 plus xterm's alternate screen (`?47`, the pre-1049 form) and mouse. Its `rmcup`/`smcup` use `?47`, which is why that mode is not an alias for `?1049`.

## Status

| | |
|---|---|
| output capabilities | **70** — all handled |
| input capabilities | 39 — **none**; baud has no input path |
| shared with `vt100` | 41 |
| conformance test | `baud-core/tests/conformance/` → `nsterm` |
| source | `../../research/sources/terminfo/nsterm.ti`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*;
that half is not implemented and is counted rather than skipped.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
