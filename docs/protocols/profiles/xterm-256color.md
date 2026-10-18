# `TERM=xterm-256color`

`xterm`, with the 256-colour palette as a colour formula. The common default.

Differs from `xterm` in the **same five** capabilities. `setaf` becomes a conditional emitting `\E[3<n>m`, `\E[9<n>m` or `\E[38;5;<n>m` by range — three spellings baud already implements. Same protocol.

This is the entry baud gives its child, so it is pinned with the capabilities programs read beyond the standard set (`infocmp -x`): the cursor style (`Ss`, `Se`), the cursor colour (`Cs`, `Cr`), the clipboard (`Ms`), clearing the scrollback (`E3`) and the modified keys. The `xterm` entry beside it is pinned without them, so these six show as differences too; the system's `xterm` entry carries the same six.

## Status

| | |
|---|---|
| output capabilities | **88** — all handled |
| input capabilities | 140 — **137 sent**, measured against the pinned terminfo by `the_profiles_input_counts_are_what_baud_can_send` |
| shared with `vt100` | 43 |
| conformance test | `baud-core/tests/conformance/` → `xterm_256color` |
| source | `../../research/sources/terminfo/xterm-256color.ti`, `infocmp -x -1`, hash-pinned |

Conformance replays every output capability and requires that none is
recognised-and-dropped. Input capabilities are the keys a terminal *sends*, and that half is
implemented: `Term::key` encodes them. The count is **measured** rather than
asserted -- every capability in the pinned terminfo is expanded to bytes and
intersected with everything the encoder can produce, so no name is mapped to a
key by hand.

A `$TERM` is a capability **profile**, not a protocol — see
[../README.md](../README.md). The protocols this draws on are documented per
level in the parent directory.
