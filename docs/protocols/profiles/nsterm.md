# `TERM=nsterm`

Apple Terminal.app's own entry — what macOS sets by default.

VT100 plus xterm's alternate screen (`?47`, the pre-1049 form) and mouse. Its `rmcup`/`smcup` use `?47`, which is why that mode is not an alias for `?1049`.

## Status

| | |
|---|---|
| output capabilities | **71** — all handled |
| input capabilities | 45 — **38 sent**, measured against the pinned terminfo by `the_profiles_input_counts_are_what_baud_can_send` |
| shared with `vt100` | 41 |
| conformance test | `baud-core/tests/conformance/` → `nsterm` |
| source | `../../research/sources/terminfo/nsterm.ti`, `infocmp -x -1`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*, and that half is
implemented: `Term::key` encodes them. The count is **measured** rather than
asserted -- every capability in the pinned terminfo is expanded to bytes and
intersected with everything the encoder can produce, so no name is mapped to a
key by hand.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
