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
| **partial** | implemented but known-incomplete; the gap is named in the row — **no row carries this, and `no_row_is_marked_partial` fails if one does** |
| **parsed** | recognised by the parser and then **dropped** — worse than missing, because it looks supported |
| **todo** | in scope, not implemented |
| **out** | deliberately not in scope; the reason is in the row |

A `parsed` row is a bug. It is the state where a terminal accepts a sequence,
does nothing, and gives the application no way to find out.

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
| `dcs_entry` | done | parameters and intermediates collected |
| `dcs_param` | done | |
| `dcs_intermediate` | done | |
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
