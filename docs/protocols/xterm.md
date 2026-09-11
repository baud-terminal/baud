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

## Mouse: SGR encoding only

`?1000/1002/1003` select which events; `?1006` selects the encoding. Without
`?1006` baud reports **nothing**, and counts the drop in `mouse_unencodable`.

The X10 scheme it declines puts each ordinate in one byte at `value + 32`,
capping it at 223. baud's grid goes to 2048 columns. A report from column 400 in
that scheme is not truncated — it names a different cell, and the application
acts on it.

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

## Declined

`?1001` highlight tracking — `[[source: ctlseqs]]`: "this mode requires a
cooperating program, else xterm will hang". A terminal a sequence can hang is
worse than one that ignores the sequence.

`OSC 52` clipboard **reads** get no reply; see
[../protocol-decisions.md](../protocol-decisions.md).
