# VT protocol coverage

What baud implements, what it does not, and what it has decided not to. Every
row traces to a primary source in `docs/research/sources/`, cited as
`[[source: <key>]]` and checked by `scripts/check-sources.sh`.

**This file is the scope definition.** "Implement the VT protocol" is not a
statement with a truth value until the surface is enumerated, because the
surface is not finite in any useful sense: `[[source: ctlseqs]]` documents 117
CSI sequences alone, many of them xterm extensions with no other implementation.
Tiers below say what baud is committing to and what it is not.

Status values are deliberately blunt:

| | meaning |
|---|---|
| **done** | implemented, dispatched, and covered by a test that would fail if it regressed |
| **partial** | implemented but known-incomplete; the gap is named in the row |
| **parsed** | recognised by the parser and then **dropped** — worse than missing, because it looks supported |
| **todo** | in scope, not implemented |
| **out** | deliberately not in scope; the reason is in the row |

A `parsed` row is a bug. It is the state where a terminal accepts a sequence,
does nothing, and gives the application no way to find out.

---

## Tier 0 — the parser state machine

Everything below depends on this being right, so it is not a tier of features
but the substrate. `[[source: williams]]` specifies the DEC ANSI parser as a
13-state machine; baud currently has 5.

| state | in baud | note |
|---|---|---|
| `ground` | done | |
| `escape` | done | |
| `escape_intermediate` | done | final byte consumed; DECALN dispatched |
| `csi_entry` | partial | folded into `CsiParams` |
| `csi_param` | done | `CsiParams` |
| `csi_intermediate` | done | intermediate recorded, in both feed paths |
| `csi_ignore` | done | |
| `dcs_entry` | partial | payload consumed via `StringIgnore`; no DCS dispatch |
| `dcs_param` | **todo** | |
| `dcs_intermediate` | **todo** | |
| `dcs_passthrough` | **todo** | |
| `dcs_ignore` | **todo** | |
| `osc_string` | done | payload captured, bounded, and dispatched |
| `sos_pm_apc_string` | done | payload consumed, never executed |

~~`sos_pm_apc_string` is a correctness and security gap.~~ Resolved: DCS, SOS,
PM and APC payloads are consumed by `StringIgnore` and never executed. Note the
specified limit, which is not a hole: `[[source: williams]]` says a bare `ESC`
*cancels* a control string, so the guarantee is precisely that a payload
containing no ESC is inert.

baud adds a `Utf8` state, which `[[source: williams]]` does not have — that
parser predates UTF-8 and is defined over bytes. Keeping it is deliberate; the
note in `docs/research/notes/c-vt-parser.md` records why.

---

## Tier 1 — VT100/ANSI core

The set every terminal must implement. No application can be assumed to work
without these.

### C0 controls

| byte | name | status |
|---|---|---|
| `0x00` NUL | ignored | done |
| `0x07` BEL | bell | done — counted; no UI to ring |
| `0x08` BS | backspace | done |
| `0x09` HT | tab | done — real stop table |
| `0x0A` LF | line feed | done — index only unless LNM |
| `0x0B` VT | vertical tab | done — index |
| `0x0C` FF | form feed | done — index |
| `0x0D` CR | carriage return | done |
| `0x0E` SO | shift out (G1) | done |
| `0x0F` SI | shift in (G0) | done |

### ESC sequences

| seq | name | status |
|---|---|---|
| `ESC 7` | DECSC save cursor | done |
| `ESC 8` | DECRC restore cursor | done |
| `ESC D` | IND index | done — column preserved (was resetting it) |
| `ESC E` | NEL next line | done |
| `ESC M` | RI reverse index | done |
| `ESC c` | RIS full reset | done |
| `ESC H` | HTS set tab stop | done |
| `ESC =` / `ESC >` | DECKPAM/DECKPNM | done |
| `ESC # 8` | DECALN screen alignment | done |
| `ESC ( ` / `ESC ) ` / `ESC * ` / `ESC + ` | charset designate G0-G3 | done — mechanism; glyph table blocked, see below |

### CSI operations

Final bytes from `[[source: ctlseqs]]` § "Functions using CSI".

| seq | name | status |
|---|---|---|
| `CSI @` | ICH insert blanks | done |
| `CSI A` | CUU cursor up | done |
| `CSI B` | CUD cursor down | done |
| `CSI C` | CUF cursor forward | done |
| `CSI D` | CUB cursor back | done |
| `CSI E` | CNL cursor next line | done |
| `CSI F` | CPL cursor preceding line | done |
| `CSI G` | CHA cursor char absolute | done |
| `CSI H` / `CSI f` | CUP / HVP | done |
| `CSI I` | CHT forward tabulation | done |
| `CSI J` | ED erase in display | done |
| `CSI K` | EL erase in line | done |
| `CSI L` | IL insert lines | done |
| `CSI M` | DL delete lines | done |
| `CSI P` | DCH delete chars | done |
| `CSI S` | SU scroll up | done |
| `CSI T` | SD scroll down | done |
| `CSI X` | ECH erase chars | done |
| `CSI Z` | CBT backward tabulation | done |
| `CSI \`` | HPA char position absolute | done |
| `CSI a` | HPR char position relative | done |
| `CSI b` | REP repeat preceding char | done |
| `CSI c` | DA device attributes | done — identifies as VT102 |
| `CSI d` | VPA line position absolute | done |
| `CSI e` | VPR line position relative | done |
| `CSI g` | TBC tab clear | done — 0 and 3; others inert, not aliased |
| `CSI h` | SM set mode | partial — LNM and IRM parsed; IRM not yet acted on |
| `CSI l` | RM reset mode | partial — as SM |
| `CSI m` | SGR character attributes | partial — see SGR below |
| `CSI n` | DSR device status report | done — 5 and 6; others unanswered, not guessed |
| `CSI r` | DECSTBM scroll region | done |
| `CSI s` / `CSI u` | SCOSC / SCORC | done |
| `CSI ? h` / `CSI ? l` | DECSET / DECRST | partial — DECOM, DECAWM, DECTCEM act; the rest inert by decision |

### SGR

Enumerated against `[[source: ecma48]]` § 8.3.117 and, for the extensions,
`[[source: ctlseqs]]` § "Character Attributes".

| parameters | status |
|---|---|
| 0 reset | done |
| 1, 2 bold/dim · 22 neither | done |
| 3 italic · 23 not | done |
| 4 underline, 21 double · 24 not | done — one underline bit, so double is an underline |
| 5, 6 blink · 25 steady | done — no timer, so rapid and slow are the same bit |
| 7 reverse · 27 positive | done |
| 8 conceal · 28 reveal | done |
| 9 strike · 29 not | done |
| 30-37, 39 foreground · 40-47, 49 background | done |
| 90-97, 100-107 aixterm bright | done — palette indices 8..15 |
| 38/48 `;5;n` 256-colour, `;2;r;g;b` truecolour | done |
| 38/48 `:5:n` / `:2:Pi:r:g:b` sub-parameter form | done — both the six-part form with a colour-space id and the five-part form without |
| 10-19 fonts, 20 Fraktur, 26, 50-65 | **out** — no renderer consumes them, and a bit set and never read is the failure this table exists to catch |

**This was the largest `parsed` gap in the codebase and the table did not show
it.** `SgrAttr::fold` handled `Reset` and foreground only, falling through to "no
change" for `Set`, `Unset` and every `Bg` — because `StyleWord` was a bare `u32`
holding a 24-bit foreground and nothing else. Bold, italic, underline, reverse,
blink, conceal, strike and **all background colours** were parsed and discarded:
support in the parser, nothing on the screen. `StyleWord` now carries
foreground, background and attributes.

---

## Tier 2 — VT220 / VT420

In scope. Needed by real applications (vim, tmux, htop) but not by every one.

| seq | name | status |
|---|---|---|
| `CSI ! p` | DECSTR soft reset | done |
| `CSI SP q` | DECSCUSR cursor style | done |
| `CSI " q` | DECSCA character protection | done |
| `CSI ? J` | DECSED selective erase display | done |
| `CSI ? K` | DECSEL selective erase line | done |
| `CSI $ p` | DECRQM request mode | done |
| `CSI s` (DECSLRM) | left/right margins | **todo** |
| `CSI ' }` / `CSI ' ~` | DECIC / DECDC | **todo** |

DECSCA, DECSED and DECSEL land together on purpose: the two erases without the
protection are silent aliases for ED and EL, and an application that protected a
region would watch it vanish.

Protection is **not** an `Attrs` bit. `[[source: vt510-decsca]]`: "DECSCA does
not effect visual character attributes set by the select graphic rendition (SGR)
function" -- so it sits in its own field of `StyleWord`, where a renderer
reading attributes cannot mistake it for one. It *is* part of the interning key,
so protected and unprotected text are distinct renditions and the flag survives
in the template table without widening the 8-byte `PackedCell`.

Two rules follow, and both are pinned by tests. A plain ED/EL erases protected
characters -- only the private forms are selective. And the blank an erase
leaves behind does not inherit the protection, or the first DECSEL under
`DECSCA 1` would protect everything it just cleared.

---

## Tier 3 — xterm extensions in universal use

In scope because applications assume them, not because DEC specified them.

| seq | name | status |
|---|---|---|
| `OSC 0/1/2` | window/icon title | done |
| `OSC 4` | set colour palette | done — including the `?` query |
| `OSC 8` | hyperlinks | **todo** |
| `OSC 52` | clipboard | consumed; **read deliberately refused** — see security note |
| `OSC 104` | reset palette | done |
| `CSI ? 1049 h/l` | alternate screen buffer | done |
| `CSI ? 2004 h/l` | bracketed paste | done — recorded and exposed as `Term::bracketed_paste()` |
| `CSI ? 1000/1002/1003 h/l` | mouse tracking | done |
| `CSI ? 1006 h/l` | SGR mouse encoding | done — the only encoding, see below |
| `CSI ? 1001 h/l` | highlight tracking | **out of scope** — "requires a cooperating program, else xterm will hang" |
| `CSI ? 2026 h/l` | synchronised output | done — exposed as `Term::synchronized_output()` |
| `CSI ? 25 h/l` | DECTCEM cursor visibility | done — state exposed via `cursor_visible()` |

---

## Out of scope, with reasons

Recorded so that "not implemented" is a decision rather than an omission.

| seq | name | why not |
|---|---|---|
| `DCS ... q` | Sixel graphics | An image protocol, not a terminal protocol. Large surface, and it does not interact with the ingest architecture baud exists to test. |
| Kitty graphics `APC G` | image protocol | As above. The `APC` **string must still be consumed** — that is Tier 0, and it is a bug today. |
| `DCS ... | DECUDK` | user-defined keys | Input-side; baud has no input path. |
| `CSI > ... m` | XTMODKEYS | Input-side. |
| `CSI ... T` XTHIMOUSE | mouse highlight tracking | Requires a reply channel and a UI. |
| ReGIS, Tektronix | vector graphics | Historical; no modern application emits them. |
| National replacement charsets | DECNRCM | Superseded by UTF-8. Recorded because `[[source: ctlseqs]]` documents them and a reader will ask. |

---

## A source conflict, recorded

`[[source: vt510-decawm]]` gives DECAWM's default as **"No autowrap"**.
`[[source: ctlseqs]]` documents xterm, where autowrap is on, and every modern
terminal wraps. baud follows xterm: an application emitting a long line expects
it to wrap, and the DEC default would break essentially all of them. The older
and stricter reading is the one baud does **not** implement, and that is a
choice rather than an oversight.

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

## How this file stays true

`docs/vt-coverage.md` is checked by `tests/coverage.rs`: every row marked
**done** must have a named test, and the test asserts the operation is reachable
from `Term::feed`. A row cannot be marked done without one. Documentation that
claims coverage it does not have is the failure mode this file exists to
prevent — the README claimed "~22 operations" while three of them were parsed
and dropped.
