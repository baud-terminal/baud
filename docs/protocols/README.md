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
| `CSI 2 i`, a printer-to-host session, xterm's file dumps | unsafe: the screen typed back to the program, a printer talking to the host, files a program names; each counted. Printing itself is built, through the macOS print panel (`../vt-coverage.md`, the MC row) |
| `OSC 52` clipboard **reads** | a remote program could read back what the user last copied |

Each row of [../vt-coverage.md](../vt-coverage.md) gives what baud does for
its operation and why, and the open departures from the standards are listed
at its end. The DEC line-drawing glyph table translates the set's 32
positions, `0x5F..0x7E`, `0x5F` "Blank" to U+0020 SPACE, and keeps
`CellFlags::GRAPHICS` on the cell.
