# `TERM=xterm-16color`

`xterm`, with aixterm's bright colours as a colour formula.

Differs from `xterm` in **five** capabilities, all colour: `setaf`, `setab`, `setf`, `setb`, `initc`. Same protocol.

## Status

| | |
|---|---|
| output capabilities | **90** — all handled |
| input capabilities | 140 — **139 sent**, measured against the pinned terminfo by `the_profiles_input_counts_are_what_baud_can_send` |
| shared with `vt100` | 43 |
| conformance test | `crates/baud-core/tests/conformance/` → `xterm_16color` |
| source | `../../research/sources/terminfo/xterm-16color.ti`, `infocmp -x -1`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*, and that half is
implemented: `Term::key` encodes them. The count is **measured** rather than
asserted -- every capability in the pinned terminfo is expanded to bytes and
intersected with everything the encoder can produce, so no name is mapped to a
key by hand.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
