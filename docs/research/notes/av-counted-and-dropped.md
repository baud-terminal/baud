# AV. Everything baud reads and then drops: the walk

**Status: 2026-10-19.** Every control the core recognises and does not
act on is counted, so a program gets silence, never a wrong answer: the
five parser counters -- `unsupported`, `unsupported_param`,
`vt52_unsupported`, `charset_unsupported`, `strings_ignored` -- for a
control with no handler, and a refusal counter of its own wherever a
handler declines part of what it was asked. This note lists every such
family, and the `out` rows no counter reaches, with what the governing
text says, and recommends for each: build it, keep the refusal, or ask. Per the owner's rule of 2026-09-25 ("full
VT420-class terminal. we need to support such protocols fully, always!")
a refusal stands only where answering is unsafe or would claim what baud
is not.

## Summary for the dashboard

- **Built 2026-10-19:** `?1047` (the alternate screen, wiped on the way
  out) and `?1048` (save and restore the cursor), `?1049`'s two effects
  as programs written for older terminfo entries send them apart.
- **Built 2026-10-20:** the title-stack position report (`CSI # S`) and
  the title modes (`CSI > Pm t`, `CSI > Pm T`): a title sent in
  hexadecimal, and a UTF-8 title under `ESC % @`; the SGR stack
  (`CSI # {`, `CSI # }`) and the rendition report (`CSI # |`); the
  palette stack (`CSI # P`, `CSI # Q`, `CSI # R`).
- **Built 2026-10-20:** ECMA-48's backward moves HPB and VPB.
- **Small, to build next** (the text is complete and nothing is the
  owner's): XTCHECKSUM's checksum flags, the pointer-hiding mode, the
  `?30` scrollbar mode, `?41`'s tab after a pending wrap, and OSC 60-62's
  feature lists.
- **Larger, to build** (whole protocols a VT420-class terminal has): the
  DEC locator, the host-writable status line, and xterm's special colours.
- **Decisions needed:** the tertiary DA, the bell volumes, the keyboard
  LEDs, the alternate function-key keyboards, printing, pointer shapes and
  colours, font changes by a program, X resources, rxvt's font-shifting
  keys, and ECMA-48's C1 functions one at a time.
- **Refusals that stand:** the ones whose reply would be unsafe, would
  give an identity baud does not have, or answer a function no text
  assigns.

## How the walk was taken

A scratch harness, not committed, fed every form `[[source: ctlseqs]]`
lists (`CSI`, `ESC`, `OSC`, `DCS`, each with every `Ps = N` its section
lists, bare and with `1`) to a fresh `Term` at 80x24 and read the five
counters; then every `ESC` final `0x30-0x7E` in ANSI and in VT52 mode, and
every `CSI` final bare and behind `?`, `>` and `=`. Hits from the probe's
own shape (an invalid colour `x`, a parameter where a list needs two)
were checked against `docs/vt-coverage.md` and the code and dropped. What
remains is below. `docs/vt-coverage.md` has rows for some; where it has
none the table says so.

## The table

| family | what baud does | the governing text | row | recommendation |
|---|---|---|---|---|
| `?1047` | counted | ctlseqs DECSET/DECRST `1 0 4 7`; xterm `srm_OPT_ALTBUF` | was in the `?47` row | **built**: the `?1047` row of `docs/vt-coverage.md`, `mode_1047_clears_the_alternate_screen_on_the_way_out` |
| `?1048` | counted | ctlseqs `1 0 4 8`, "as in DECSC" / "as in DECRC"; xterm `srm_SAVE_CURSOR` | was in the `?47` row | **built**: the `?1048` row of `docs/vt-coverage.md`, `mode_1048_saves_and_restores_the_cursor_as_decsc_and_decrc`; XTSAVE saves the cursor (Jev, 0.91/0.94) |
| `CSI # S` XTTITLEPOS | answered since 2026-10-20 | ctlseqs line 664: reply `CSI Pn ; Pm # S`, the stack's index and its maximum | `CSI # S` | **built**: `xttitlepos_reports_the_pushes_and_the_depth` |
| `CSI Pn j` HPB, `CSI Pn k` VPB | counted | `[[source: ecma48]]` HPB and VPB, the backward twins of HPR and VPR | the `CSI j` and `CSI k` rows | **built** 2026-10-20: `backward_moves.rs` |
| `CSI Pm # {` / `# }` / `# \|` (and `# p`, `# q`) XTPUSHSGR, XTPOPSGR, XTREPORTSGR | built 2026-10-20 | ctlseqs lines 1871-1953: a ten-deep stack of the rendition, selectable by attribute | `CSI # {`, `CSI # \|` | **built**: `a_list_push_restores_only_its_groups`, `xtreportsgr_answers_the_attributes_every_cell_shares` |
| `CSI # P` / `# Q` / `# R` XTPUSHCOLORS, XTPOPCOLORS, XTREPORTCOLORS | built 2026-10-20 | ctlseqs lines 644-660: a ten-deep stack of the palette and dynamic colours | `CSI # P`, `CSI # R` | **built**: `a_pop_restores_the_palette_the_specials_and_the_dynamic_colours` |
| `CSI Ps # y` XTCHECKSUM | counted | ctlseqs line 1823: five bits that change DECRQCRA's checksum | DECRQCRA's row: "not built" | **build**: DECRQCRA exists |
| `CSI > Pm t` / `CSI > Pm T` XTSMTITLE, XTRMTITLE | the set modes built 2026-10-20 | ctlseqs lines 1707 and 733: set titles in hex or UTF-8, and query them so | `CSI > Pm t` | **built** the two set modes; the query modes act on title reports, which stay refused as unsafe, and are counted with them (`the_query_modes_are_refused_with_the_title_reports`) |
| `CSI > Ps p` XTSMPOINTER | counted | ctlseqs line 1464: whether the pointer hides while the user types -- never, unless mouse tracking is on, always but on leaving the window, always | none | **build** in the app (AppKit's hide-until-moved) |
| `?30` | counted | ctlseqs DECSET `3 0`, "Show scrollbar (rxvt)" | none | **build**: baud has a scrollbar |
| `OSC 60` / `61` / `62` XTQALLOWED, XTQDISALLOWED, XTQALLOWABLE | counted | ctlseqs line 2182: reply `OSC 6 0 ; Pt ST`, the allowed categories among seven xterm resource names | none | **build**, each category named for what baud does; Jev where one is partly allowed |
| `?35` | counted | ctlseqs DECSET `3 5`, "Enable font-shifting functions (rxvt)" | none | **ask Jev**: the text names rxvt's functions and not their keys; baud's zoom is the nearest |
| DEC locator: `CSI Ps ; Pu ' z` DECELR, `' {` DECSLE, `' \|` DECRQLP, `' w` DECEFR | counted | ctlseqs lines 1776-1907, the VT330/340 locator | none | **build**: a whole input protocol of the DEC terminals, beside the mouse modes baud has |
| `CSI Ps $ }` DECSASD, `CSI Ps $ ~` DECSSDT | counted | ctlseqs lines 1993 and 2001, the host-writable status line | DECRQSS's row answers them invalid, "no status line" | **build**: a VT420 feature |
| `CSI Ps ; Pf ; Pb , \|` DECAC, `, }` DECATC, `CSI Ps ) {` DECSTGLT | counted | ctlseqs lines 1954-1965: the VT525's colour assignments | DECRQSS's row answers them invalid | **keep refused** while baud is VT420-class; note AP's level question decides it |
| `OSC 5` / `6` / `105` / `106` special colours | counted | ctlseqs lines 2232-2241: the colours bold, underline, blink, reverse and italic draw in | none | **build** with the palette |
| `CSI = Ps c` tertiary DA | counted | ctlseqs line 810: DECRPTUI, `DCS ! \| 00000000 ST` as xterm sends it | none | **ask Jev**, quoting the owner's DA1 pick: it is a VT400 report from a terminal that answers as a VT102 |
| `CSI Ps SP t` DECSWBV, `CSI Ps SP u` DECSMBV | counted | ctlseqs lines 1720 and 1739: off, low, high | none | **ask Jev**: macOS's alert sound has no volume baud sets, so "off" is buildable and "low" is not |
| `CSI Ps q` DECLL | counted | ctlseqs line 1536: light or put out Num, Caps and Scroll Lock | none | **the owner's**, beside note AH's title-bar lights |
| `?1050`-`?1053`, `?1060`, `?1061` | counted | ctlseqs DECSET `1 0 5 0` to `1 0 6 1`: the termcap, Sun, HP, SCO, legacy and VT220 keyboards | the VT220 table is `out` in `docs/input-coverage.md` | **ask Jev**: each is a key encoding, and the owner's rule asks for protocols whole |
| `CSI Ps i`, `CSI ? Ps i` MC, `?18`, `?19`, `?43`-`?46` | counted | ctlseqs lines 1047-1054 and DECSET `1 8` to `4 6`: printing and logging | none | **keep refused** where it writes a file (`?46`, `CSI 10 i`, `CSI 11 i`, which a remote program could aim at the user's disk); **the owner's** for a printer |
| `OSC 22`, `OSC 13`/`14`/`113`/`114` | counted | ctlseqs lines 2118 and 2128: the pointer's shape and colours | none | **ask Jev**: AppKit has named pointer shapes and no recoloured pointer |
| `OSC 50` | counted | ctlseqs line 2137: set or query the font | none | **ask Jev**: a program changing the user's font |
| `DCS + Q` XTGETXRES | counted | ctlseqs line 510: the value of an X resource | none | **ask Jev**: baud has no X resources, and its configuration is not theirs |
| `DCS + p` XTSETTCAP | counted | ctlseqs line 535: set a termcap entry | none | **keep refused**: it redefines what a key sends, DECUDK's reason |
| XTWINOPS 1-10, 24 and up; `CSI Ps * \|` DECSNLS | counted | ctlseqs XTWINOPS; DECSNLS line 1947 | XTWINOPS `out`, DECSNLS none | **the owner's**, already open for XTWINOPS; DECSNLS resizes the window too and goes with it |
| XTWINOPS 20, 21 | counted | ctlseqs XTWINOPS | `out` | **keep refused**: the reply types a program's text back |
| `CSI > c` DA2 | counted | ctlseqs line 817 | `out` | **keep refused**: the owner's DA1 pick |
| `CSI > 2 s`, `CSI > 3 s`; `?13`, `?1020`-`?1023` set or reset; DECSCL above Level 4 | counted | ctlseqs: "disallowed in the control sequence"; read-only; beyond a VT420 | their rows | **keep refused**: conforming |
| `?14`, `?1014`, `?1044` set or reset | counted | ctlseqs DECSET | their rows | **keep**: permanently in the state baud always has, decided |
| `?80`, `?1070`, `?8452` without the Sixel claim | counted | the DA gate (V18) | their rows | **keep**: decided |
| `CSI ? Pp g` | counted | ctlseqs line 883 documents it; xterm reads `CSI > Pp g` | the XTQFMTKEYS row | **keep**: decided |
| `CSI Ps ^` SIMD | counted | ECMA-48 SIMD against ctlseqs' SD | `out` | **keep**: decided |
| `?2002`, `?2003`, `?1040`, `?42`, `?38` | counted | ctlseqs DECSET | `out` | **keep**: their rows' reasons |
| `?10` | counted | ctlseqs: rxvt's toolbar | none | **keep refused**: baud has no toolbar |
| `?41` | counted | ctlseqs: "more(1) fix (see curses resource)"; xterm's `TabNext` wraps a pending wrap before a tab under it (`[[source: xterm-tabs-c]]` 113-118) | none | **build**: one behaviour of HT, so a timed A/B on the write path |
| `OSC 3`, `OSC 15`/`16`/`18`/`115`/`116`/`118`, `OSC 51` | counted | ctlseqs: an X property, the Tektronix window's colours, Emacs's reserve | none | **keep refused**: no X window, no Tektronix window, nothing assigned |
| `OSC 46` | counted | ctlseqs line 2133: the log file's name | none | **keep refused**: a remote program naming a file to write |
| `ESC` with an ECMA-48 C1 function: `B` BPH, `C` NBH, `F` SSA, `G` ESA, `I` HTJ, `K` PLD, `L` PLU, `Q` PU1, `R` PU2, `S` STS, `T` CCH, `U` MW, `` ` `` DMI, `a` INT, `b` EMI, `d` CMD | counted | `[[source: ecma48]]` | none | **ask Jev, one at a time**: CCH (the preceding graphic character and CCH itself "are to be ignored") and DMI/EMI (manual input off and on, the keyboard lock baud has as KAM) are small; PLD/PLU need half-line rendering; the rest act on transmission baud does not do |
| other `ESC` finals in ANSI mode (`0`-`5`, `:`, `;`, `<`, `?`, `@`, `A`, `Y`, `e`-`m`, `p`-`z`, `{`) and in VT52 mode | counted | private or unassigned; every VT52 form ctlseqs lists is acted on | none | **keep**: no function to act on |
| `CSI` finals no text assigns, and `?`, `>`, `=` forms of the rest | counted (`unsupported_by_final`) | ECMA-48's private finals and unassigned forms | none | **keep** |
| `PM`, `SOS`, an `APC` that is not kitty's | counted, and `strings_ignored` | ECMA-48: their content is for the application or undefined | none | **keep**: no function to act on |
| `ESC ( F` and the other designations with a final no list names | `charset_unsupported` | ctlseqs' set lists | the charset rows | **keep**: no set to designate |
| SGR 56, 57 | ignored | ECMA-48 § 8.3.117 reserves them | `out` | **keep** |

## The handlers' own refusal counters

Each counts a request its handler understood and declined in part, and
each has a decision behind it.

| counter | what it counts | the decision | recommendation |
|---|---|---|---|
| `clipboard_reads_refused` | `OSC 52` reads | answering would hand a remote program the user's clipboard | **keep** |
| `clipboard_writes_refused` | `OSC 52` writes with no clipboard provisioned, no selection named, or a body past half the area (`Term::clipboard_refusals` splits them) | the area is the embedder's | **keep** |
| `checksums_refused` | DECRQCRA while the embedder turned the checksum off | the embedder's switch | **keep** |
| `links_refused`, `links_clipped` | `OSC 8` past 127 targets, and a URI that may have been cut | a refused link is plain text; a reclaimed slot could send a cell to another address | **for Jev**, already on the list: reclaim past 127, or keep refusing |
| `title_reports_refused` | XTWINOPS 20 and 21, and since 2026-10-20 XTSMTITLE's query modes 1 and 3 | the reply types a program's text back | **keep** |
| `mouse_unencodable` | X10 reports past column or row 223 | X10 would name a different cell | **keep**; `?1006` has no limit |
| `marks_dropped` | combining marks past the cluster store | note AB's Q7 | **keep** open there |
| `sixel_p2_ignored` | a Sixel asking for an opaque background, drawn transparent | note S records why the fill is not built and what the texts leave open | **for Jev**, with note S's open points: the owner's rule asks for it built |
| `reply_overflows`, `string_truncations` | replies past the reply buffer, control strings past theirs | a cut reply is a wrong answer | **keep** |
| the kitty stream's `refused`, `ignored`, `virtuals_refused` | refused commands; animation and frame deletes; virtual placements past the table | kitty graphics' animation is in scope and unbuilt | **build** animation (the out-of-scope table's row) |
| the image store's `images_refused`, `placements_refused` | images and placements past the store's limits | bounded by design | **keep** |

## `out` rows no counter reaches

| row | what it says | recommendation |
|---|---|---|
| SUB displaying the DEC error character | baud cancels and draws nothing; its earlier reason was wrong | **the owner's**: note AF's Q-UNDEF |
| 8-bit C1 introducers and GR-as-GL bytes, under UTF-8 | they are UTF-8 continuation and lead bytes there | **keep**: conforming; both are done under ISO 8859-1 |
| DECUDK | redefines what a key sends | **keep** |
| ReGIS, Tektronix | vector graphics | **keep**; the owner's rule names VT420-class, and neither is a VT420 protocol |
| DECNRCM `?42` | DEC's texts disagree on what it does | **keep** open on note AF |

## What the tables do not cover

The rows marked **partial** in `docs/vt-coverage.md` act on part of what
they name and say which part; they are not counted and are not repeated
here. Kitty graphics' animation commands are in that file's out-of-scope
table as "in scope and unbuilt".
