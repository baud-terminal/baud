# Protocol decisions

Why the hard calls went the way they did. `docs/vt-coverage.md` says *what* is
implemented, one row per operation; this says *why*, and it is the half a
reader scanning a status table would never scroll to.

Several of these are **refusals**. They look like gaps and are not, so each one
says what it would cost to "fix" it.

---

## Cross-cutting gaps

Two things block whole tiers and are not any single sequence.

**~~No reply channel.~~** Resolved. `Storage.reply` is a buffer the
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

* **DA under-claims.** baud answers `CSI ? 6 c` (VT102), with attribute `4`
  added, `CSI ? 6 ; 4 c`, only when the embedder has provisioned image memory
  and claims Sixel (`Term::set_sixel_claim`; `baud-app`'s `[images]
  advertise_sixel`, off by default). It now implements
  selective erase, which is a VT220 feature, and the answer still does not move
  -- a DA identity is a promise about *everything* in that level. This said
  the bar was UDKs; it is not: DECUDK is DEC STD 070's independent extension
  8 (`Levels: 2X`), not a Level 2 requirement, so a Level 2 claim without `;8`
  is the honest shape for a terminal without them. What keeps baud off Level
  1 and 2 is the list in note AP (`docs/research/notes/ap-da-level.md`):
  no Level 1 operation (DECSCL selects Level 1 since 2026-10-19 without
  its restrictions), no DA2, and for Level 2 LS1R-LS3R and the Level 2
  keyboard. Claiming more would make applications
  send sequences baud drops, which is the `parsed` failure by another
  route. This also said baud had no NRCS and no 132-column mode;
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

**Mouse reports go in the encoding the application chose, and a position the
encoding cannot name is refused.** `?9`, `?1000`, `?1002` and `?1003` select
which events an application hears about. `?1005` (UTF-8 coordinates), `?1006`
(SGR), `?1015` (urxvt's decimal) and `?1016` (SGR in pixels) select how a
report is written, one at a time -- a set replaces the one in force and a reset
undoes only its own -- and without any of them baud sends X10
(`ESC [ M Cb Cx Cy`). Only X10 and `?1005` cap the ordinate below the grid's.

X10 puts each ordinate in one byte at `value + 32`, capping it at 223. baud's
grid goes to 2048 columns. A report from column 400 in that scheme is not a
truncated answer — it names a different cell, and the application acts on it.
So that position is refused and counted in `mouse_unencodable`, and every
position that fits is sent. `?1005` writes the same values as UTF-8, which
reaches 2015, and refuses past it the same way. That is the wrong-shaped-answer
rule applied to a report the terminal *sends*, and it is why the counter
exists: an application that enables tracking and hears silence should be
diagnosable from outside.

This said reporting was SGR-only and that without `?1006` baud reported
nothing. The reason covered column 400, and the refusal covered every report:
an application speaking X10 on an 80x24 screen got silence where every byte was
encodable. It then named `?1000`-`?1003` and `?1006` alone: `?9`, `?1005`,
`?1015` and `?1016` were counted and dropped until 2026-10-09.

The core has no input device, so `Term::mouse_report` takes an event in
`Col`/`Row` and returns whether it was queued. Pixel-to-cell division stays with
the embedder, which is where the off-by-one would otherwise hide. **Under
`?1016` the embedder gives the pixel too**, on `MouseEvent::px`, and an event
without one is refused and counted rather than made up from the cell, whose
corner is a pixel the pointer was probably not at. Both apps derive the cell
and the pixel in one function (`gesture::pointer_under`).

**Character sets are designated, invoked and translated.** `ESC ( ) * + C`
designate G0-G3, SO and SI invoke G1 and G0 into GL, and a byte written under a
set that replaces it becomes the character that set puts there.

**In a UTF-8 stream none of this is conforming, and no standard chooses
between the ways of doing it.** ISO/IEC 10646 says code extension control
functions "(such as designation escape sequences, single shift, and locking
shift) **shall not** be used with this coded character set"
(`[[source: iso10646-fcd]]` §11), and UTF-8 is reached by leaving ISO 2022
through `ESC % G` (`[[source: ecma35]]` §15.4, 10646 §12.2), not as a mode
inside it. So SCS, SO/SI and SS2/SS3 under UTF-8 are extensions: xterm's
translation, baud's, and the `CellFlags::GRAPHICS` provenance baud keeps
beside the translated codepoint. They are argued against xterm's practice
and ncurses', not against ECMA-35, which does not reach them
([note G](research/notes/g-text-encoding.md)).

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
(`string_truncations`), because dropping the sequence instead would leave the
previous title showing with nothing to say why.

Four OSC decisions are refusals or limits, and each is deliberate rather than a gap:

* **`OSC 52` clipboard reads get no answer.** A hostile program on the far end
  of an ssh session could otherwise read back whatever the user last copied,
  which may be a password. Answering with anything -- including an empty
  payload -- tells it the feature works, so the refusal is silence.
  **Writes are honoured**, the owner's V11 (2026-09-25), with `[clipboard]
  osc52 = "off"` to refuse them, and they are not held to `OSC_MAX`: the body
  after `52;` goes to its own two areas, the copy being collected and the last
  one finished, and a copy past them is refused whole rather than cut. A `Pd`
  that is not base64 clears the selection, as `[[source: ctlseqs]]` says.
* **A colour *name* is the X server's.** `[[source: ctlseqs]]` allows a name
  "as per XParseColor", which sends it to the X server; X.Org's server finds
  it in its rgb.txt table ignoring case and nothing else
  (`[[source: xserver-oscolor-98aed9d]]` 852-884), so baud resolves the 782
  names of `[[source: xorg-rgb-0d2caec]]` the same way, from a table built
  at build time. A name the server lacks is left alone and counted as
  unsupported. Until 2026-10-18 every name was.
* **An Xcms spec is converted for the screen baud draws on.** `rgbi:` and
  the CIE and TekHVC forms go through sRGB, the surface's colour space,
  where libX11 with no profile from the server uses a 1990 Tektronix CRT;
  CIELAB and CIELUV are CIE 015:2018's, where libX11 reads them at a
  hundredth of CIE's scale. TekHVC and the gamut clip are Xcms's own.
* **A colour query answers a colour something set, and nothing else.**
  `OSC 4 ; c ; ?` answers a program's override, else the entry the embedder
  handed over (`Term::set_theme`, or `Term::set_default_palette` alone), else
  nothing: the core holds no palette of its own, because index-to-pixels
  belongs to the renderer. `baud-app` hands it the theme's 256 entries. This
  said a query for a colour nothing overrode got no answer, which was true
  before the embedder could hand the table over.
* **A hyperlink is never re-pointed, and a click opens four schemes.** The
  `OSC 8` table refuses a colliding link rather than evicting the occupant,
  because a later run that writes no cell could otherwise retarget a link
  already on screen while nothing on screen changed; a clipped address is
  refused for the same reason. And `baud-app` opens `http`, `https` and
  `mailto` only, reveals a local `file` link in Finder, and refuses the rest
  -- every other scheme is a registered application the opener would launch,
  on output an attacker controls.

**Two modes are the embedder's to act on, and `baud-app` acts on both.**
Bracketed paste (`?2004`) and synchronised output (`?2026`) are state
`baud-core` cannot use itself -- it draws nothing, and a paste does not pass
through it -- so it records them and `Term::bracketed_paste()` and
`Term::synchronized_output()` are the channel. Both `baud-app` drivers bracket a
paste while `?2004` is set (`baud-app/src/paste.rs`). Both drivers hold
frames while an update is open, for 33 ms at most, so a program that dies
inside one does not freeze the window. Bracketed
paste is a security mode, not a cosmetic one: without it a pasted newline is
delivered as typed input and runs.

**~~No tab stop table.~~** Resolved. `Storage.tabs` is one flag per column,
defaulted every eight and restored by RIS; `HT`, `HTS`, `TBC`, `CHT` and `CBT`
all go through it. `baud-difftest` needs no checkout, and its tab case, plain
`HT`, agrees with alacritty.

---

## Recommended against, with reasons

Declining is a decision and belongs in the record.

| protocol | why not |
|---|---|
| **ReGIS** | Vector graphics. No modern application emits it. |
| **DECUDK**, user-defined keys | It redefines what a key sends, which needs a table of user-programmable strings and a policy for who may set them. Out on scope, not for want of an input path: `Term::key` is one. |
| **Tektronix mode** (`ESC [ ? 3 8 h`) | A different terminal emulated inside this one. Historical. |

Four rows that stood here are no longer declined. **Highlight tracking**
(`CSI ? 1001`) is built to the spec, the region drawn and the release
reported (V12): the "xterm will hang" warning describes an event loop, and
`baud-core` has none. **Sixel** is
decoded into caller memory with no external decoder and drawn by all three
render paths. **Kitty graphics** (`APC G`) is out today, consumed without
being executed and counted; the owner's call is to build it (below). **DECNRCM**, `?42`, was declined as "superseded by UTF-8": since
2026-10-24 it is recognised and changes nothing, as DEC STD 070 ignores it
for a North American keyboard and baud has no other (the `?42` row in
`docs/vt-coverage.md`).

---

## Still open

**Decided by the owner on 2026-09-25 and not built**
(`docs/research/notes/v-open-decisions.md`, under its standing rule: "A
half-built protocol feature is finished to the spec, not switched off"):

* **V10, side margins finished.** DECSLRM binds the wrap, ICH, DCH and
  DECIC/DECDC (above), and since 2026-10-12 every vertical scroll: `SU`,
  `SD`, `IND`, `RI`, `LF`, `IL` and `DL` move only the rectangle, which
  `[[source: vt510-decslrm]]` makes the scrolling region (`docs/record.md`,
  "Rectangle scrolling"). Steps (c) and (d) remain: line starts and cursor
  moves that honour the fence, and C2-min.
* **V14, an ambiguous-width switch with `?1021`**: built 2026-10-15. The
  app's `[text] ambiguous_width` sets the core's policy at launch, Private
  Use Area characters stay one column under either (the owner), and DECRQM
  answers `?1021` `1` or `2`, `?1022` `0` and `?1023` `1` (Jev). Under
  "wide" a box-drawing or block character two cells wide is drawn across
  both, since 2026-10-25 (design step 4): its lines run through the second
  cell, its verticals and corners stay at the first cell's centre at the
  one-cell weight, and a block is its fraction of both cells. Both shapes
  are Jev's leans, below the bar, and on the owner's list (`docs/record.md`,
  "A box line two cells wide is drawn across both").
* **V18, the Sixel modes `?80` and `?1070`**, built to the spec, with
  `?8452` and XTSMGRAPHICS, since 2026-10-15 (`docs/vt-coverage.md`). Each
  acts and answers only while DA lists Sixel; the choices Jev left below the
  bar are on the owner's list there and in `docs/record.md`, "The Sixel
  modes `?80`, `?1070` and `?8452` are built, and XTSMGRAPHICS answers".
* **V19, kitty graphics.** The reason this file gave against it was that
  `f=100` hands the terminal a PNG, which needs zlib, and the core decodes
  with no allocator and no dependency. The workspace already has such a
  decoder: written for `baud-font`'s colour emoji, with its own inflate and
  no allocation, and moved to `baud-domain/src/png.rs` (V19 step 2) beside
  a resumable inflate, `baud-domain/src/zlib.rs`.

**DEC Special Graphics `0x5F`** — Blank, U+0020 or U+00A0, the owner's call
(above).

**The other DEC sets** — designated and not translated, though
`[[source: xterm-charsets]]` has xterm's maps for eight of them (above).


## Open departures

Each is a departure from a standard, recorded as a defect. The list moved here
from the README on 2026-10-12. On the protocol side the full list is the rows
at **partial** in [vt-coverage.md](vt-coverage.md), which its legend names;
of those rows the list below repeats DECSC and DECRC and the DEC Special
Graphics table.

- **Which wrap state DECSC saves and DECRC restores is open** (note AE's
  Q12). They save G0-G3 and which of them GL and GR invoke; a single shift
  is never pending when DECSC runs, since its `ESC` cancels one
  (`[[source: dec-std-070]]` p. 3-81), so vt510's "Any single shift 2 (SS2)
  or single shift 3 (SS3) functions sent" is always empty.
- **DEC Special Graphics leaves `0x5F` as `_`**, where every DEC text read
  replaces it with Blank; which code point Blank stores is the owner's call
  ([the record](record.md), "DEC Special Graphics begins at 0x5F, and what
  it stores there is the owner's call").
- **A `SUB` draws no error character by default**, where DEC's VT220 to
  VT500 display one (`[[source: dec-std-070]]` p. 5-132): the owner's call of
  2026-10-23, with `[screen] substitute_mark` to draw it, and in VT52 mode
  it draws none either way, a lean Jev split on
  ([vt-coverage.md](vt-coverage.md), the SUB rows).
- **Every script is shaped as `latn` or `DFLT`.** OpenType directs an
  application to the script table for the text's own script and to `DFLT`
  where there is none; `baud-font` takes no script argument
  ([note AB](research/notes/ab-right-to-left.md), Q6).
- **Marks past what the cluster store holds are dropped**, and counted
  ([note AB](research/notes/ab-right-to-left.md), §7.2 and Q7).
- **When no face draws a cluster whole, each attached code point its base's
  face lacks is dropped**, and counted, where the Unicode Standard asks for a
  placeholder ([note AD](research/notes/ad-unrenderable-in-a-cell.md)).
- **The rendition table never frees a slot**, so a session that mints more
  distinct renditions than it has slots recolours cells that still name an
  evicted one.

---

## Where this came from

A file named docs/protocol-roadmap.md, since deleted, used to hold the tables
above. It was written as a roadmap and outlived that: Tiers A, B and C all landed, so every heading
read "**done**" and the file described the past while being shaped like a
plan. The reasoning was worth keeping and the framing was not, so it is
here — filed under decisions, which is what it always was.

