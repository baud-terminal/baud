# Protocol decisions

Why the hard calls went the way they did. `docs/vt-coverage.md` says *what* is
implemented, one row per operation; this says *why*, and it is the half a
reader scanning a status table would never scroll to.

Several of these are **refusals**. They look like gaps and are not, so each one
says what it would cost to "fix" it.

---

## Cross-cutting gaps

Two things block whole tiers and are not any single sequence.

**~~No reply channel.~~** Resolved. `Storage.reply` is a 256-byte buffer the
core fills and the caller drains with `Term::reply()` / `Term::clear_reply()`,
mirroring the input side -- the core is `no_std` with no I/O, so it cannot own a
PTY. Overflow is counted in `reply_overflows`, never silent.

DA, DSR, DECRQM and XTVERSION all go through it. XTVERSION is the one report
that is **not** a CSI — `[[source: ctlseqs]]` gives the response as
`DCS > | text ST` — which is exactly why the reply side is a typed `Report`
rather than byte literals: the shape comes from the variant, so the one
odd-shaped answer cannot be written in the shape of the others by mistake.

DECRQM is worth its own note, because its interesting answer is the negative
one. `[[source: ctlseqs]]` gives `0` as "not recognized", and baud returns it
for every mode it does not implement. That is the *correct* answer rather than
a fallback: an application uses DECRQM precisely to discover whether a mode
exists, so replying `2` ("reset") would tell it the mode is available and off,
and it would then set the mode and believe it took effect. The rule against
answering with the wrong shape extends to answering with the wrong *value*.

Replies are typed. `Report` in `baud-domain` is the mirror of `Csi`: the parse
side has had a type since the beginning, while the reply side was three byte
literals and a hand-rolled decimal formatter at the call site. The shape --
introducer, parameters, final byte -- is now chosen by the variant, so a DSR
cannot be answered in a DA's shape by editing a string. `ESC Fe` got the same
treatment as `Esc`: `b'M'` and `b'D'` are not the names of anything, and a
reader should not have to remember which of them scrolls backwards (RI does).

**CPR reports a column that exists.** With autowrap on, the internal cursor
sits one *past* the last column after a character lands there -- the
deferred-wrap sentinel the next write consumes. That is an implementation
device, not a position: `[[source: vt510-decawm]]` describes the wrap as
applying to "graphic characters received when the cursor is at the right border
of the page", so the cursor is *at* the border. baud reported `CSI 1 ; 201 R`
on a 200-column page until a property test walked into it -- an application
that saves and restores the cursor would drift one column, silently, only on
full lines.

Two rules the tests pin, because both failure modes are worse than silence:

* **DA under-claims.** baud answers `CSI ? 6 c` (VT102). It now implements
  selective erase, which is a VT220 feature, and the answer still does not move
  -- a DA identity is a promise about *everything* in that level, and baud has
  neither UDKs nor NRCS nor 132-column mode. Claiming VT220 would make
  applications send sequences baud drops, which is the `parsed` failure by
  another route.
* **An unsupported DSR gets no answer at all.** Replying with the wrong report
  shape is worse than not replying: the application parses whatever arrives as
  the report it asked for.

**The alternate screen is a second buffer, not a mode bit.** `?1049` switches
`Term` between two full grids, saving the cursor on the way in and restoring it
on the way out, per `[[source: ctlseqs]]`. Two things make it cheaper than it
looks. It has no scrollback -- no ring, no run table, no promotion -- so
`OpMode::Runs` materialises directly there, and the O(1) ring-advance fast path
in `scroll_region_up` is deliberately not taken on it: that path retires a line
by moving the scrollback window, and using it on the alt screen would scroll the
primary screen's history from under it. And the second grid is 10 MiB of
*virtual* space that a terminal never running a full-screen program never faults
in.

Entering twice must not re-save the cursor, or an application that sets the mode
twice is restored to a position from inside the alt screen. The switch also
keeps its own saved cursor rather than sharing DECSC's, so a program that does
both gets each back.

**DCS is dispatched, and DECRQSS answers.** `[[source: ctlseqs]]` gives
`DCS $ q Pt ST` as Request Status String, answered `DCS 1 $ r Pt ST` when valid
and `DCS 0 $ r ST` when not. baud reports SGR, DECSTBM, DECSLRM, DECSCUSR and
DECSCA, and answers *invalid* for everything else — which is the honest answer,
since a body it made up would be sent straight back to it.

**1 is valid and 0 is invalid, and the DEC manuals say the opposite.** ctlseqs
records the errata: "testing a DEC VT420 in 1996 showed that the documented code
for a valid or invalid response to DECRQSS was reversed", with vttest and DEC
STD 070 agreeing against the VT420 and VT525 manuals. baud follows xterm because
that is what applications are written against, and there is a test named for it
so a reader consulting the DEC manual does not "fix" it backwards.

The `dcs_ignore` state is the security-relevant one: an unhandled DCS consumes
its payload instead of falling through to Ground, where the bytes inside would
run as commands. The guarantee is precise, and narrower than it first looks — a
payload containing no `ESC` is inert, and one containing an `ESC` is *cancelled*
there per `[[source: williams]]`, so what follows is a fresh sequence. A test
asserted the stronger, wrong version first and was corrected against the spec.

**Left and right margins are real, and `CSI s` is ambiguous.** DECSLRM shares
its bytes with SCOSC and is told apart only by DECLRMM (`?69`), so the parser is
given the mode rather than guessing. `[[source: vt510-decslrm]]` requires
`Pl < Pr`, so an inverted or empty range is *rejected* rather than clamped into
something the application did not ask for. The margins bound the wrap (text
resumes at the **left margin**, not column 0), ICH, DCH, and DECIC/DECDC, and
the write path folds the right margin into the loop bound it already computed —
so with no margins set there is no extra branch.

**Hyperlinks ride in spare cell bits.** `OSC 8`'s link index lives in the
fourteen unused bits of `CellFlags`, because `PackedCell` is eight bytes *by
measurement* and a ninth would round it to twelve. The URI table is bounded at
`NLINK` and evicts on collision, like the style table and for the reason
`CLAUDE.md` names: a URI is attacker-supplied content used as a key, and a
structure keyed on that which grows without limit is the content-addressing
failure mode. An evicted link renders as an ordinary cell.

**Mouse reporting is SGR-only, and that is a correctness decision.** `?1000`,
`?1002` and `?1003` select which events an application hears about; `?1006`
selects the encoding. Without `?1006` baud reports **nothing**, and counts the
drop in `mouse_unencodable`.

The X10 encoding it declines to use puts each ordinate in one byte at
`value + 32`, capping it at 223. baud's grid goes to 2048 columns. A report from
column 400 in that scheme is not a truncated answer — it names a different cell,
and the application acts on it. That is the wrong-shaped-answer rule applied to
a report the terminal *sends*, and it is why the counter exists: an application
that enables tracking and hears silence should be diagnosable from outside.

The core has no input device, so `Term::mouse_report` takes an event in
`Col`/`Row` and returns whether it was queued. Pixel-to-cell division stays with
the embedder, which is where the off-by-one would otherwise hide.

**Character sets are designated, invoked and recorded — but not translated.**
`ESC ( ) * + C` designate G0-G3, SO and SI invoke G1 and G0 into GL, and a byte
written while the DEC Special Character and Line Drawing Set is active is
carried to the cell **as the byte the application sent, flagged**
(`CellFlags::GRAPHICS`) rather than as a translated codepoint.

That is a sourcing decision, not a design preference. `[[source: ctlseqs]]`
names the set — "C = 0 -> DEC Special Character and Line Drawing Set, VT100" —
but gives no byte-to-glyph mapping, and `MANIFEST.toml` records
`dec-special-graphics` as **gated**: vt100.net publishes the table as a figure,
so no claim may rest on it. Writing the table from memory is exactly what
"Do not implement a control function from memory" forbids, and this project has
been overruled by the spec on smaller things.

Carrying the flag loses nothing recoverable: a renderer holding the table has
the byte and the fact that it is a graphics byte, which is everything it needs.
Translating to a guessed codepoint would be unrecoverable, and it would be the
`parsed`-status failure in its worst form — the terminal would look like it
worked. When a citable table is obtained, the mapping is a renderer change and
these tests do not move.

Sets baud does not distinguish are recorded as `Charset::Other` rather than
folded into ASCII, so designating DEC Supplemental is not mistaken for a no-op.

**OSC payloads are bounded, and truncated rather than dropped.** Every byte of
an OSC is attacker-controlled -- a window title is whatever the far end of an
ssh session sent -- and a `no_std` core cannot grow a buffer to meet it. A
payload over `OSC_MAX` is truncated and the truncation is *counted*
(`osc_truncations`), because dropping the sequence instead would leave the
previous title showing with nothing to say why.

Three OSC decisions are refusals, and each is deliberate rather than a gap:

* **`OSC 52` clipboard reads get no answer.** A hostile program on the far end
  of an ssh session could otherwise read back whatever the user last copied,
  which may be a password. Answering with anything -- including an empty
  payload -- tells it the feature works, so the refusal is silence.
* **Colour *names* are not resolved.** `[[source: ctlseqs]]` allows a name "as
  per XParseColor", which needs X11's `rgb.txt`; baud has no citable copy, and
  approximating it would be inventing a protocol. The entry is left alone.
* **A query for a colour nothing overrode says nothing.** The core holds no
  built-in palette, because index-to-pixels belongs to the renderer, and a
  second copy here would be one to keep in step.

**Two modes are recorded for an embedder rather than acted on internally.**
Bracketed paste (`?2004`) and synchronised output (`?2026`) are real state that
this core cannot itself use -- it has no input path and no renderer. They are
recorded because `Term::bracketed_paste()` and `Term::synchronized_output()`
give an embedder the only channel through which either mode can be honoured,
which is what separates them from the "set and never read" failure. Bracketed
paste is a security mode, not a cosmetic one: without it a pasted newline is
delivered as typed input and runs.

**~~No tab stop table.~~** Resolved. `Storage.tabs` is one flag per column,
defaulted every eight and restored by RIS; `HT`, `HTS`, `TBC`, `CHT` and `CBT`
all go through it. Whether this closes the differential failures the README
names is **unverified** -- that needs a differential run, which needs an
alacritty checkout.

---

---

## Recommended against, with reasons

Declining is a decision and belongs in the record.

| protocol | why not |
|---|---|
| **Highlight tracking** (`CSI ? 1001`) | `[[source: ctlseqs]]`: "this mode requires a cooperating program, else xterm will hang". A terminal that a sequence can hang is worse than one that ignores the sequence. |
| **Sixel** (`DCS ... q`) | An image format, not a terminal protocol. Large surface, needs a palette and a raster buffer, and it does not interact with the ingest architecture baud exists to test. The `DCS` payload is already consumed safely, which is the part that matters for correctness. |
| **ReGIS** | Vector graphics. No modern application emits it. |
| **Kitty graphics** (`APC G`) | Same reasoning as sixel. Note baud already consumes `APC` payloads without executing them, which was a real bug before Tier 0 landed — the protocol is *safe to ignore*, which is the bar. |
| **DECUDK**, user-defined keys | Input-side, and superseded by the kitty keyboard protocol for anything new. |
| **National Replacement Character Sets** (DECNRCM) | Superseded by UTF-8. Documented here because `[[source: ctlseqs]]` carries them and a reader will ask. |
| **Tektronix mode** (`ESC [ ? 3 8 h`) | A different terminal emulated inside this one. Historical. |

---

---

## Still open

**Kitty keyboard protocol** — the one recommendation not implemented. It
fixes what terminals have never been able to express: Ctrl+Shift
combinations, key release, and disambiguation of Escape. Input-side, so it
needs an input path baud does not have — which is the real blocker, not the
protocol.

[spec](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)

---

## Where this came from

`docs/protocol-roadmap.md` used to hold the tables above. It was written as
a roadmap and outlived that: Tiers A, B and C all landed, so every heading
read "**done**" and the file described the past while being shaped like a
plan. The reasoning was worth keeping and the framing was not, so it is
here — filed under decisions, which is what it always was.

