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
  no UDKs: DECUDK is under "Out of scope, with reasons" in
  `docs/vt-coverage.md`. Claiming VT220 would
  make applications send sequences baud drops, which is the `parsed` failure
  by another route. This also said baud had no NRCS and no 132-column mode;
  the national sets are designated and translated, and DECCOLM is done.
* **An unsupported DSR gets no answer at all.** Replying with the wrong report
  shape is worse than not replying: the application parses whatever arrives as
  the report it asked for.

**The alternate screen is a second buffer, not a mode bit.** `?1049` switches
`Term` between two full grids, saving the cursor on the way in and restoring it
on the way out, per `[[source: ctlseqs]]`. It keeps no history. A full-screen
scroll rotates it as it rotates the primary, inside a ring of its own rows with
its own rotation (`AltBuf`), and `retire` keeps no line on it, so rotating the
alternate screen moves nothing of the primary's history. This said the
alternate screen took no rotation and that `OpMode::Runs` materialised there,
which was `baud-core2`'s; it also called the second grid 10 MiB of virtual
space never faulted in, a figure not re-taken on this core.

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

**Hyperlinks ride in spare cell bits.** `OSC 8`'s link index lives in seven
bits of `CellFlags`, above its three flags and below the six bits of the cluster
index, because `PackedCell` is eight bytes *by measurement* and a ninth would
round it to twelve. The URI table is bounded at `NLINK`, 128, because a URI is
attacker-supplied content used as a key, and a structure keyed on that which
grows without limit is the content-addressing failure mode. **It refuses a
collision rather than evicting**, unlike the style table: an evicted link would
be a wrong destination under a cell whose text did not change. A refused link
is counted in `links_refused` and its cells are plain text. This said the link
took fourteen bits, and that the table evicts on collision and an evicted link
renders as an ordinary cell.

**Mouse reports go in the encoding the application chose, and a position X10
cannot name is refused.** `?1000`, `?1002` and `?1003` select which events an
application hears about; `?1006` selects SGR, which has no ordinate limit, and
without it baud sends X10 (`ESC [ M Cb Cx Cy`).

X10 puts each ordinate in one byte at `value + 32`, capping it at 223. baud's
grid goes to 2048 columns. A report from column 400 in that scheme is not a
truncated answer — it names a different cell, and the application acts on it.
So that position is refused and counted in `mouse_unencodable`, and every
position that fits is sent. That is the wrong-shaped-answer rule applied to a
report the terminal *sends*, and it is why the counter exists: an application
that enables tracking and hears silence should be diagnosable from outside.

This said reporting was SGR-only and that without `?1006` baud reported
nothing. The reason covered column 400, and the refusal covered every report:
an application speaking X10 on an 80x24 screen got silence where every byte was
encodable.

The core has no input device, so `Term::mouse_report` takes an event in
`Col`/`Row` and returns whether it was queued. Pixel-to-cell division stays with
the embedder, which is where the off-by-one would otherwise hide.

**Character sets are designated, invoked and translated.** `ESC ( ) * + C`
designate G0-G3, SO and SI invoke G1 and G0 into GL, and a byte written under a
set that replaces it becomes the character that set puts there.

**DEC Special Graphics: the positions are DEC's and the codepoints are foot's.**
`[[source: vt100-ug-ch3]]` names the set's glyphs in text, Table 3-9, for "ASCII
codes 137 through 176" octal: 32 positions, `0x5F..0x7E`.
`[[source: vt102-ug-ch5]]`, `[[source: dec-special-graphics]]` and
`[[source: dec-std-070]]` agree. None of the four gives a Unicode codepoint, so
the table takes `[[source: foot-vt100-0]]`'s, cited as foot's judgement. The
cell holds the translated codepoint and keeps `CellFlags::GRAPHICS`, so a copy
of `─` is `─` and the cell still says where it came from.

**baud has 31 of the 32, and `0x5F` is an open departure.** foot's table starts
at `0x60`, so `ESC ( 0 _` stores `_` where DEC draws "Blank": on the bulk run,
on the per-byte path and under VT52 `ESC F`. U+0020 and U+00A0 both conform in
what they draw, and the texts do not choose between them, so the codepoint is
the owner's call. It is open in docs/record.md, "DEC Special Graphics begins at
0x5F, and what it stores there is the owner's call", and the coverage row is
**partial** until it is answered.

This section said the opposite: the cell kept "the byte the application sent,
flagged", because `[[source: ctlseqs]]` names the set without a mapping and
`MANIFEST.toml` recorded `dec-special-graphics` as gated. That was a reason not
to guess, and not a reason never to have a table. f297b21 translated through
foot's, and the DEC pages pinned since then name every position.

**The UK and national sets translate too**, through `[[source: xterm-charsets]]`,
cited as xterm's judgement (`every_national_replacement_renders_as_its_table_says`).

**The other DEC sets are designated and not translated.** DEC Supplemental,
Technical, Supplemental Graphics, Cyrillic, Greek, Hebrew, Turkish and the JIS
pair are named (`Charset::Dec`), and a final `[[source: ctlseqs]]` does not list
is `Charset::Other`. Neither is folded into ASCII, so designating DEC
Supplemental is not mistaken for a no-op. `[[source: xterm-charsets]]` has
xterm's maps for the Technical, Supplemental Graphics, Cyrillic, Greek, Hebrew,
Turkish and JIS sets, matched to them by name, so what is missing is the
translation, not a source. That is open.

**OSC payloads are bounded, and truncated rather than dropped.** Every byte of
an OSC is attacker-controlled -- a window title is whatever the far end of an
ssh session sent -- and a `no_std` core cannot grow a buffer to meet it. A
payload over `OSC_MAX` is truncated and the truncation is *counted*
(`osc_truncations`), because dropping the sequence instead would leave the
previous title showing with nothing to say why.

Four OSC decisions are refusals, and each is deliberate rather than a gap:

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
* **A hyperlink is never re-pointed, and a click opens four schemes.** The
  `OSC 8` table refuses a colliding link rather than evicting the occupant,
  because a later run that writes no cell could otherwise retarget a link
  already on screen while nothing on screen changed; a clipped address is
  refused for the same reason. And `baud-app` opens `http`, `https` and
  `mailto` only, reveals a local `file` link in Finder, and refuses the rest
  -- every other scheme is a registered application the opener would launch,
  on output an attacker controls.

**Two modes are recorded for an embedder rather than acted on internally.**
Bracketed paste (`?2004`) and synchronised output (`?2026`) are real state that
this core cannot itself use -- it has no renderer, and its input path encodes
keys rather than consuming these. They are
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
| **ReGIS** | Vector graphics. No modern application emits it. |
| **Kitty graphics** (`APC G`) | `f=100` hands the terminal a PNG, which needs zlib, and `baud-core` decodes into caller memory with no allocator and no dependency. Note baud already consumes `APC` payloads without executing them, which was a real bug before Tier 0 landed — the protocol is *safe to ignore*, which is the bar. |
| **DECUDK**, user-defined keys | It redefines what a key sends, which needs a table of user-programmable strings and a policy for who may set them. Out on scope; baud has an input path. |
| **DECNRCM**, the national replacement mode | Superseded by UTF-8. Documented here because `[[source: ctlseqs]]` carries it and a reader will ask. The national *sets* are not declined: each is designated and translated (above). |
| **Tektronix mode** (`ESC [ ? 3 8 h`) | A different terminal emulated inside this one. Historical. |

Two rows that stood here were built anyway, and their rows in
`docs/vt-coverage.md` are done. **Highlight tracking** (`CSI ? 1001`) is
recorded, with the region, and not drawn: the "xterm will hang" warning
describes an event loop, and `baud-core` has none. **Sixel** is decoded into
caller memory with no external decoder and drawn by all three render paths.

---

---

## Still open

**~~Kitty keyboard protocol~~** — built: `encode_key` in `baud-domain`, with
all five progressive-enhancement flags and the query answered (its row in
`docs/vt-coverage.md` is done). The blocker this named was an input path, and
`Term::key` is one. On macOS, key releases still wait on the window crate.
[spec](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)

**DEC Special Graphics `0x5F`** — Blank, U+0020 or U+00A0, the owner's call
(above).

**The other DEC sets** — designated and not translated, though
`[[source: xterm-charsets]]` has xterm's maps for eight of them (above).

---

## Where this came from

A file named docs/protocol-roadmap.md, since deleted, used to hold the tables
above. It was written as a roadmap and outlived that: Tiers A, B and C all landed, so every heading
read "**done**" and the file described the past while being shaped like a
plan. The reasoning was worth keeping and the framing was not, so it is
here — filed under decisions, which is what it always was.

