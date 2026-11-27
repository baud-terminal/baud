# `TERM=ansi`

The generic ANSI/ECMA-48 entry — no vendor extensions.

VT100's CSI operations without the DEC private modes. If baud handles VT100 it handles this; the profile exists for terminals that are ANSI but not DEC.

## Status

| | |
|---|---|
| output capabilities | **63** — all handled |
| input capabilities | 8 — **7 sent**, measured against the pinned terminfo by `the_profiles_input_counts_are_what_baud_can_send` |
| shared with `vt100` | 33 |
| conformance test | `crates/baud-core/tests/conformance/` → `ansi` |
| source | `../../research/sources/terminfo/ansi.ti`, `infocmp -x -1`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*, and that half is
implemented: `Term::key` encodes them. The count is **measured** rather than
asserted -- every capability in the pinned terminfo is expanded to bytes and
intersected with everything the encoder can produce, so no name is mapped to a
key by hand.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
