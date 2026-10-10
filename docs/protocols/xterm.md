# xterm extensions

Not DEC protocol, and mostly not in any standard — but applications depend on
them, which is the only bar that matters here.

- **Implementation:** `baud-core/src/osc.rs`, `baud-core/src/ops.rs`
- **Tests:** `baud-core/tests/xterm/`
- **Conformance:** `xterm` (83 caps), `xterm-16color` (84),
  `xterm-256color` (82), `rxvt` (66), `nsterm` (70), `dtterm` (58)
- **Source:** `[[source: ctlseqs]]`

## Alternate screen

`?1049` switches to a second full grid, saving the cursor on the way in. It has
**no scrollback** — no ring, no run table, no promotion — which is what makes it
cheaper than the primary screen rather than merely as expensive. The O(1)
ring-advance fast path in `scroll_region_up` is deliberately *not* taken there:
that path retires a line by moving the scrollback window, and using it on the
alt screen would scroll the primary screen's history from under it.

`?47` is the pre-1049 form: it switches buffers and does **not** save the
cursor. `?1049` exists because applications kept getting that wrong, so the two
are not aliases.

## Mouse: four tracking levels, five encodings

`[[source: ctlseqs]]`, "Mouse Tracking": "There are two sets of mutually
exclusive modes", the mouse protocol and the protocol encoding. The first says
which pointer events become reports and the second how a report is written;
`Term::mouse_report` applies both to each event an embedder gives it.

**The tracking level** (`Term::mouse_tracking`) is one of four, or off:

| mode | what is reported |
|---|---|
| `?9` X10 compatibility | a press of button 1, 2 or 3 and nothing else: no release, no motion, no wheel and no modifiers ("only on button press", "Cb is button-1, where button is 1, 2 or 3") |
| `?1000` normal | presses and releases, with the modifiers; a wheel press, never its release ("Release events for the wheel buttons are not reported") |
| `?1002` button-event | `?1000`'s, and motion with a button held |
| `?1003` any-event | `?1000`'s, and all motion |

Motion is reported only into a cell other than the last report's, "only if the
mouse pointer has moved to a different character cell". A set selects its level
whatever was in force, and a reset of any of the four turns tracking off, as
xterm's `really_set_mousemode` does (`[[source: xterm-reset]]`'s file).
ctlseqs counts five mouse protocols, `?1001` among them; baud records `?1001`
beside the level rather than as a fifth one (below), an open departure that is
V12's to build (`docs/research/notes/v-open-decisions.md`).

**The encoding** is one of five. It has no accessor: `mouse_report` reads it.

| mode | a report | largest ordinate |
|---|---|---|
| none: X10, the default | `CSI M Cb Cx Cy`, each value plus 32 in one byte; a release is code 3 | 223 |
| `?1005` UTF-8 | X10's bytes, each UTF-8 encoded past 127 | 2015 |
| `?1006` SGR | `CSI < Cb ; Cx ; Cy M`, decimal with no 32 added, and `m` for a release | the grid's |
| `?1015` urxvt | `CSI Cb ; Cx ; Cy M`: X10's button value, its 32 and its release code, in decimal | the grid's |
| `?1016` SGR-Pixels | `?1006`'s, with the position in pixels, one-based, from the event's `MouseEvent::px` | a pixel below 65535; an event without one is refused |

ctlseqs makes the encodings "mutually exclusive" and says nothing of which wins.
baud takes xterm's rule: **a set replaces the encoding in force, and a reset
undoes only its own** -- "a reset is only effective against the matching mode"
(`[[source: xterm-reset]]`'s file). So `?1015 h ?1006 h` is SGR,
`?1006 h ?1015 h` is urxvt, `?1015 h ?1006 l` stays urxvt, and only `?1015 l`
returns to X10. The encoding in force applies at every level, `?9` included, as
xterm's `EmitButtonCode` writes it (`[[source: xterm-button]]`).

**A position the encoding cannot name is refused and counted, not truncated.**
X10 caps each ordinate at 223 and `?1005` at 2015. baud's grid goes to 2048
columns, and a report from column 400 written in X10 is not a truncated answer
-- it names a different cell, and the application acts on it. So past the limit
the report is dropped and counted in `mouse_unencodable`, where xterm clamps and
sends a `0` byte (`EmitMousePosition`, `[[source: xterm-button]]`), and under
`?1016` an event with no pixel is counted the same way rather than made up from
the cell. Every position that fits is sent.

**`?1007` alternate scroll** (`Term::alternate_scroll`): on the alternate
screen, with no tracking to take the wheel, set sends cursor up and down through
`Term::key`, so in DECCKM's form, and reset sends nothing -- "cursor up/down
controls are sent when the terminal is displaying the Alternate Screen Buffer".
It is set at power-on. ctlseqs leaves the initial state to the `alternateScroll`
resource, which xterm defaults to off, so baud's default differs from xterm's:
the owner's call.

`RIS` clears the level and the encoding and `DECSTR` keeps them; neither touches
`?1007`. DECRQM answers each level and each encoding `1` only while it is the
one in force, and `?1007` from its flag. The rows in `docs/vt-coverage.md` name
the tests.

**The core has no pointer.** `Term::mouse_report` takes an event in cells, and
under `?1016` the pixel too, from the embedder. The macOS app is the one
embedder with a pointer. The Linux app binds only the keyboard, so it sends no
report at any level or in any encoding, though DECRQM there answers from the
same core state.

This section was headed "SGR encoding only" from 2026-08-26 until 2026-10-10.
It said that without `?1006` baud reported nothing, and declined X10. `?9`,
`?1005`, `?1015`, `?1016` and `?1007` were built on 2026-10-09.

## OSC payloads are bounded and truncated, not dropped

Every byte of an OSC is attacker-controlled. A payload over `OSC_MAX` is
truncated and the truncation is *counted*; dropping the sequence instead would
leave the previous title showing with nothing to say why.

`OSC 8` hyperlink URIs live in a bounded, evicting table — a URI is
attacker-supplied content used as a key. The link index rides in spare
`CellFlags` bits, because `PackedCell` is eight bytes by measurement and a ninth
would round it to twelve.

## Modes recorded for an embedder

Bracketed paste, synchronised output, DECCKM, DECSCNM, DECSCLM, DECARM,
`?1034`, `?45`, `?3`. The core has no renderer, and its input path encodes keys
rather than reading them, so exposing
them by accessor *is* acting on them — an embedder has no other channel. That
is what separates them from the "set and never read" failure.

Bracketed paste is a **security** mode, not a cosmetic one: without it a pasted
newline is delivered to the shell as typed input and runs.

`?1001` highlight tracking is recorded the same way (`Term::highlight_tracking`),
with the region from the application's answer, `CSI Ps ; Ps ; Ps ; Ps ; Ps T`
(`Term::highlight_region`), which is told from SD by its five parameters and so
never scrolls; DECRQM answers from the flag. Nothing more is built: the region
is not drawn, no release report is sent, and `?1001` alone sends no press,
where ctlseqs has "the same information as for normal tracking" generated.
Building it to the spec is the owner's V12
(`docs/research/notes/v-open-decisions.md`). ctlseqs' warning that "this mode
requires a cooperating program, else xterm will hang" describes an event loop
-- "All X events are ignored until the proper escape sequence is received" --
and `baud-core` has none to block, so whether to wait is the embedder's
decision.

## Declined

`OSC 52` clipboard **reads** get no reply; see
[../protocol-decisions.md](../protocol-decisions.md).

This listed `?1001` as declined until 2026-10-10, for the hang warning above.
