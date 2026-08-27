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
| **partial** | implemented but known-incomplete; the gap is named in the row — **no row carries this** |
| **parsed** | recognised by the parser and then **dropped** — worse than missing, because it looks supported |
| **todo** | in scope, not implemented |
| **out** | deliberately not in scope; the reason is in the row |

A `parsed` row is a bug. It is the state where a terminal accepts a sequence,
does nothing, and gives the application no way to find out.

**No row carries `partial`, `parsed` or `todo`, and `no_row_is_unfinished` in
`baud-core/tests/coverage.rs` fails if one ever does.** `out` is the only status
that means "not implemented", and it requires a reason in the row. Do not
relabel an unfinished operation as `out` to satisfy the check: reaching for
`partial` is the signal that the operation is not done, and the fix is the
operation.

---

## Tier 0 — the parser state machine

Everything below depends on this being right, so it is not a tier of features
but the substrate. `[[source: williams]]` specifies the DEC ANSI parser as a
13-state machine. baud now has all of them; the `Utf8` state below is an
addition, not a substitution.

| state | in baud | note |
|---|---|---|
| `ground` | done | |
| `escape` | done | |
| `escape_intermediate` | done | final byte consumed; DECALN dispatched |
| `csi_entry` | done | its own state — see below |
| `csi_param` | done | `CsiParams` |
| `csi_intermediate` | done | intermediate recorded, in both feed paths |
| `csi_ignore` | done | |
| `dcs_entry` | done | its own state — the marker rule, as `csi_entry` |
| `dcs_param` | done | C0 **ignored**, not executed |
| `dcs_intermediate` | done | `30-3F` ends the sequence — the parameter string is over |
| `dcs_passthrough` | done | payload collected for DECRQSS |
| `dcs_ignore` | done | unhandled DCS consumed, never executed |
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

### `csi_entry` was folded into `csi_param`, and the fold was the bug

The two states differ in one transition, and it is the one that matters. In
`csi_entry` the bytes `3C-3F` (`<`, `=`, `>`, `?`) are **collected** as a private
marker; in `csi_param` those same bytes go to `csi_ignore`. A marker is legal
only in the position immediately after `CSI`.

Folded into a single state, baud accepted a marker anywhere in the parameter
string — and then *executed* the result, which is the dangerous direction:

* `CSI 2 ? l` is malformed and must be discarded. baud read it as DECRST 2,
  reset DECANM and **switched the terminal into VT52 mode** — a protocol change
  produced by a sequence the spec says to throw away.
* `CSI 1 ? J` is malformed and must be discarded. baud read it as DECSED and
  erased the screen.

`a_private_marker_after_a_parameter_is_ignored` and
`a_marker_after_a_parameter_does_not_erase` pin both. Splitting the state also
closed two smaller holes in the same arm:

* **C0 inside a control sequence.** `[[source: williams]]` gives `csi_param` an
  `execute` action on `00-17,19,1C-1F` that returns to the same state. baud had
  no C0 branch, so a stray NUL fell through to `csi_ignore` and silently
  cancelled an unrelated SGR. `a_c0_inside_a_csi_does_not_cancel_it` and
  `the_c0_inside_a_csi_actually_executes` pin the rule and the fact that
  *execute* means executed, not skipped.
* **`<` and `=` were dropped silently.** baud recognised only `?` and `>`, so
  the other two private namespaces fell to `csi_ignore` and vanished without
  reaching dispatch — a `parsed` failure, invisible to `Term::unsupported`.
  `Marker` now carries all four. `Csi::parse` rejects `Lt` and `Equals` before
  the standard table, because falling through would read `CSI < c` as a primary
  DA and **answer a query in the wrong shape**.

Both feed paths share one definition of the rule (`Marker::from_private`), so
the fast path and the state machine cannot drift on it — the divergence class
this parser has had before.

### The "anywhere" block

`[[source: williams]]` has a set of transitions that fire *regardless of the
current state*, and baud had none of them:

> On the VT220, VT420 and VT500, the C0 controls CAN and SUB cancel any escape
> sequence, control sequence or control string in progress and return to ground
> state.

`1B` ESC does the same, starting a fresh sequence rather than ending in ground.

Four arms each wrote `if b.is_c0() { self.execute(b) }` — and `is_c0()` is
`b < 0x20`, which **includes all three**. So an ESC arriving mid-sequence was
executed and the parser stayed in the parameter string: `CSI 1 ESC [ 2 m` lost
its escape, dispatched a nonsense `CSI 1 [`, and printed `2m` as text.

It is handled once now, ahead of the state match, so no arm can forget it. The
states that decline are the ones that must: inside a control string ESC is the
first half of ST, so the payload has to survive until the next byte decides
between *terminator* and *cancel*. Those arms already implement that themselves.

| status | |
|---|---|
| `18` CAN, `1A` SUB — cancel from any state, control strings included | done |
| `1B` ESC — abandon and begin a new sequence | done |
| C0 in `csi_entry` — execute and **stay**, without consuming the marker position | done — `CSI <BS> ? J` is still DECSED |
| C0 in `csi_intermediate` | done — this arm had no C0 branch, so `CSI ! <NUL> p` lost its DECSTR |
| `7F` DEL in `csi_entry` / `csi_param` | done — ignored, and does not consume the marker position |
| SUB additionally displaying the DEC error character `␦` | **out** — see below |

`[[source: williams]]` records that "SUB will also display the error character, a
reversed question mark". baud cancels but writes nothing. That is a *rendering*
decision made where no renderer exists, and the same reasoning as the DEC
line-drawing table applies: a glyph written into the grid on a control byte
would be indistinguishable from text the application sent, and unrecoverable if
wrong. Cancelling is the part an application can observe.

### Encoding: UTF-8 only, and 8-bit C1 controls are therefore out

`[[source: williams]]`'s parser has 8-bit C1 controls throughout — `9B` enters
`csi_entry`, `9D` `osc_string`, `90` `dcs_entry`, `9C` is ST. baud has none of
them, and a reader comparing against the state diagram will notice. The absence
is forced rather than chosen:

> a C1 control can be mistaken for badly-formed UTF-8 when the terminal runs in
> UTF-8 mode because C1 controls are valid continuation bytes of a UTF-8 encoded
> (multibyte) value […] it is not possible to use a C1 control obtained from
> decoding the UTF-8 text, because that would require reprocessing the data.
> — `[[source: ctlseqs]]`

`80-9F` cannot be both a control and a continuation byte. A terminal picks one,
and picking UTF-8 makes the choice for every byte at once. So:

| | status |
|---|---|
| UTF-8 decoding, including the `Utf8` parser state williams does not have | done |
| 8-bit C1 introducers `90`, `9B`, `9C`, `9D`, `98`, `9E`, `9F` | **out** — they are continuation bytes; a stray one is a decode error and renders U+FFFD |
| 7-bit equivalents `ESC P`, `ESC [`, `ESC \`, `ESC ]`, `ESC X`, `ESC ^`, `ESC _` | done — this is the whole control interface |
| `ESC SP F` — S7C1T, send 7-bit controls | done — baud always does, so the request is *satisfied* |
| `ESC SP G` — S8C1T, send 8-bit controls | **out** — counted in `Term::unsupported`, never answered |
| GR bytes `A0-FF` treated as GL `20-7F` inside a sequence | **out** — same reason; they are UTF-8 lead and continuation bytes |
| Legacy single-byte charsets (Latin-1, national replacement sets) | **out** — see below |

`S7C1T`/`S8C1T` are about what the terminal **sends**, not what it accepts:
"this tells the terminal to send C1 control characters as 7-bit sequences, e.g.,
its responses to queries". baud always replies in the 7-bit form, which is
`S7C1T` and is xterm's default. Emitting `9B` into a UTF-8 stream would make
baud's own replies invalid UTF-8, and every application parses the 7-bit form.
`ESC SP G` is therefore refused — but *countably*, which is the part that was
missing: it used to be swallowed by the escape-intermediate arm in silence.

`ESC SP F` is the opposite case and the distinction is not pedantry. It asks for
7-bit replies, which is what baud sends, so the request is **satisfied** — there
is nothing to change and nothing withheld. Counting it as unsupported would
claim baud had refused something it already does. dtterm's `is2` opens with it,
and the conformance fixture is what drew the line.

**Legacy charsets are the shell's problem, not the terminal's.** baud already
has the model for the one charset mechanism it does implement: `SCS`
(`ESC ( 0`) sets `CellFlags::GRAPHICS` and the cell keeps the byte, so a
renderer with the DEC table loses nothing and baud never guesses a glyph. It
annotates rather than translates. Byte-level transcoding would put a table
lookup on the per-byte ingest path, against the rule that the working set stays
in cache — and `iconv` already does it, once, outside the hot loop.

### The DCS prefix had the same merge, and the same three bugs

`dcs_entry`, `dcs_param` and `dcs_intermediate` were one arm, exactly as the CSI
states were, and lost exactly the same three rules:

| rule | `[[source: williams]]` | was |
|---|---|---|
| `3C-3F` is a marker in `dcs_entry` and an error in `dcs_param` | "Occurrences of 3C to 3F in positions other than the first character of the parameter string cause the entire control sequence to be ignored" | accepted anywhere |
| `30-3F` after an intermediate is malformed | `dcs_intermediate` sends them to `dcs_ignore` | walked back into `dcs_param` and accepted the digit |
| C0 in the prefix is **ignored** | "C0 controls other than CAN, SUB and ESC are **not executed** while recognising the first part of a device control string" | sent to `dcs_ignore`, costing the sequence |

Note the third against the CSI rule directly above it: williams says "C0 controls
are executed immediately during the recognition of a control sequence", and *not*
executed during a device control string. Same byte, different state, opposite
action — which is the whole reason these are separate states and not one.

Unlike CSI there is no `[[source: ctlseqs]]` sub-parameter form to accommodate in
a DCS prefix, so `3A` follows williams unmodified and ends the sequence.

### C0 inside a control string payload is ignored, not stored

`osc_string` takes the *ignore* action on `01-17,19,1C-1F`; only `20-7F` is
`osc_put`, and the same holds for `dcs_passthrough`. baud appended every byte
that was not a terminator, so a raw control byte landed in the title — and OSC
payloads are attacker-supplied, which is why this one is in the threat model
rather than only in the conformance table.

**This block broke chunk-invariance the moment it landed, and the property
caught it.** `fast_esc` scans an OSC straight to its terminator, so it could not
see a cancel in between while the state machine could — the same bytes set a
title or did not, depending on whether they fit inside one `read(2)`. The fast
path now declines any OSC payload containing a C0 byte at all — it cannot
reproduce either rule, since it scans to the terminator (so it cannot see a
cancel) and copies the payload wholesale (so it would keep bytes that should be
dropped).
`a_cancel_inside_an_osc_is_honoured_in_both_feeds` pins it deterministically
rather than leaving it to a saved seed.

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
| `CSI ? 2 l` | DECANM — enter VT52 | done — see [protocols/vt52.md](protocols/vt52.md) |
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
| `CSI h` | SM set mode | done — LNM and IRM |
| `CSI l` | RM reset mode | done — as SM |
| `CSI m` | SGR character attributes | done — see SGR below |
| `CSI n` | DSR device status report | done — 5 and 6; others unanswered, not guessed |
| `CSI r` | DECSTBM scroll region | done |
| `CSI s` / `CSI u` | SCOSC / SCORC | done |
| `CSI ? h` / `CSI ? l` | DECSET / DECRST | done — every parameter of a multi-parameter form is applied |

Private modes, one row each. A mode is **done** only when something acts on it;
for the input-side and renderer-side ones "acting" means an accessor an embedder
can read, which is the only channel a `no_std` core with no keyboard and no
renderer has.

| mode | name | status |
|---|---|---|
| `?1` | DECCKM cursor keys | done — `Term::cursor_keys_application()` |
| `?2` | DECANM — **reset enters VT52** | done — see [protocols/vt52.md](protocols/vt52.md) |
| `?3` | DECCOLM 132 columns | done — clears, resets margins, homes; the *resize* is the caller's, via `Term::resize` |
| `?4` | DECSCLM smooth scroll | done — `Term::smooth_scroll()` |
| `?5` | DECSCNM reverse video | done — `Term::reverse_video()` |
| `?6` | DECOM origin | done |
| `?7` | DECAWM autowrap | done |
| `?8` | DECARM auto-repeat | done — `Term::auto_repeat()` |
| `?12` | cursor blink | done — the blink axis of DECSCUSR, reached by another route |
| `?25` | DECTCEM cursor visible | done |
| `?45` | reverse wraparound | done |
| `?47` | alternate screen, pre-1049 | done — switches buffers, does **not** save the cursor |
| `?69` | DECLRMM left/right margins | done |
| `?1034` | meta sends escape | done — `Term::meta_sends_escape()` |
| `?1001` | highlight tracking | **out** — "requires a cooperating program, else xterm will hang" |

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

Every SGR parameter above is **done** or **out**; none is partial. `CSI m` used
to be listed as `partial` on the strength of that last row, which conflated two
different things. `out` is a scope decision with a reason — storing a font
selection no renderer can read would be a bit set and never used, which is the
failure this table exists to catch. `partial` means the row claims something it
does not do. Nothing here does.

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
| `CSI s` (DECSLRM) | left/right margins | done — with DECLRMM (`?69`) |
| `CSI ' }` / `CSI ' ~` | DECIC / DECDC | done |

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
| `OSC 8` | hyperlinks | done — bounded, evicting link table |
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

## Why these decisions went the way they did

The reasoning — the refusals, the source conflicts, the things that look like
gaps and are not — is in [protocol-decisions.md](protocol-decisions.md). It was
in this file, 180 lines below the last table, where nobody scanning for an
operation's status was ever going to find it.

---

## How this file stays true

`baud-core/tests/coverage.rs` reads this file and fails if a row marked **done**
names an operation no test mentions. Run by `devenv test`, so it gates CI.

Be precise about what that proves, because this paragraph was itself the
failure it warns about for most of the project's life: it named
`tests/coverage.rs` while **no such file existed**, so the guarantee was
enforced by nothing.

* The test **does** prove every `done` row is mentioned by some test. Marking a
  row done with no test at all fails.
* It **cannot** prove the test would fail if the operation regressed. Nothing
  mechanical can. That stays a review obligation, and it is stated here as one
  rather than dressed up as a guarantee.

A weaker check that is real beats a stronger claim that is not. Documentation
claiming coverage it does not have is the failure mode this file exists to
prevent — the README claimed "~22 operations" while three of them were parsed
and dropped.
