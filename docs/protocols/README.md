# Protocols

One document per protocol level, mirroring the code and the tests:

| protocol | implementation | tests | doc |
|---|---|---|---|
| VT52 | `baud-core/src/vt52.rs`, `baud-domain/src/vt/vt52.rs` | `baud-core/tests/vt52/` | [vt52.md](vt52.md) |
| VT100 / ANSI | `baud-core/src/{write,ops,paint}.rs` | `baud-core/tests/vt100/` | [vt100.md](vt100.md) |
| VT220 | `baud-core/src/dcs.rs`, `baud-core/src/write.rs` | `baud-core/tests/vt220/` | [vt220.md](vt220.md) |
| VT420 | `baud-core/src/ops.rs` | `baud-core/tests/vt420/` | [vt420.md](vt420.md) |
| xterm | `baud-core/src/osc.rs`, `baud-core/src/ops.rs` | `baud-core/tests/xterm/` | [xterm.md](xterm.md) |

Each says what the level is, what baud implements of it, what it declines and
why, and which primary sources the claims rest on.

## `$TERM` is a profile, not a protocol

The names in a terminal's `$TERM` list — `xterm-256color`, `rxvt`, `nsterm` —
are **terminfo capability profiles**. `xterm-256color` is not a different
protocol from `xterm`; it is the same one with a different colour capability
declared. There is therefore no implementation per `$TERM` value, and there
should not be: there is one parser, and a profile is a question asked of it.

`baud-core/tests/conformance/` asks that question mechanically. For each
profile it replays every *output* capability from the ncurses entry and
requires that nothing was recognised and dropped:

| profile | output caps | status |
|---|---|---|
| `ansi` | 63 | ✅ |
| `vt52` | 17 | ✅ (after `CSI ? 2 l`) |
| `vt100` | 44 | ✅ |
| `vt102` | 49 | ✅ |
| `xterm` | 89 | ✅ |
| `xterm-16color` | 90 | ✅ |
| `xterm-256color` | 88 | ✅ |
| `rxvt` | 66 | ✅ |
| `dtterm` | 58 | ✅ |
| `nsterm` | 71 | ✅ |

One page each in [profiles/](profiles/), with what the name means and what it
draws on.

Input capabilities — those whose terminfo name begins with `k`, the keys a
terminal *sends* — are counted separately and not required. xterm declares 140
of them. **That sentence used to end "and baud has no input path", which has
been false since `encode_key` landed** -- see each profile's measured count. It is stated rather
than skipped.

The entries themselves are in `../research/sources/terminfo/`, captured with
`infocmp -x -1` from ncurses 6.0.20150808 and hash-pinned in `MANIFEST.toml`, so
the conformance target cannot drift without the source gate noticing.

## What baud declines, across all levels

Each is a decision with a cost, not an oversight:

| | why |
|---|---|
| printer control (`CSI i`, VT52 `ESC ^ _ W X ] V`) | no printer; each is recognised and counted, because accepting silently is the `parsed` failure |
| ReGIS | historical; no modern application emits it |
| `OSC 52` clipboard **reads** | a remote program could read back what the user last copied |

**Not declined, and not built.** Kitty graphics (`APC G`) is consumed without
being executed and counted. The owner decided on 2026-09-25 to build it to
the spec (V19), with the side margins' rectangle
scrolling (V10), the ambiguous-width switch and `?1021` (V14), `OSC 4`, `10`
and `11` acting on the colour shown under `?5` (V17) and the Sixel modes
`?80` and `?1070` (V18, built 2026-10-15), and to turn `OSC 52` writes on by
default (V11), in `docs/research/notes/v-open-decisions.md`.

Three things this table listed are built; their rows are in
[../vt-coverage.md](../vt-coverage.md). `?1001` highlight tracking is built
to the spec, drawn and reported (V12), and sixel is decoded and drawn; both
rows are done. The DEC
line-drawing glyph table translates `0x60..0x7E` and keeps `CellFlags::GRAPHICS`
on the cell. Its row is **partial**: DEC's set also replaces `0x5F`, and baud
leaves it as `_`, an open departure whose codepoint is the owner's call.

Fuller reasoning in [../protocol-decisions.md](../protocol-decisions.md).
