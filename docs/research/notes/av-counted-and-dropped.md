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
- **Built 2026-10-20:** ECMA-48's backward moves HPB and VPB,
  XTCHECKSUM's checksum flags, `?41`'s tab after a pending wrap, and the
  `?30` scrollbar mode.
- **Built 2026-10-20:** the bell volumes (DECSWBV, DECSMBV) and the
  margin bell (`?44`), after the owner's scope call took them in.
- **Small, to build next** (the text is complete and nothing is the
  owner's): none is left. OSC 60-62's feature lists were built on
  2026-10-20 (the `OSC 60` row of `docs/vt-coverage.md`).
- **Larger, to build** (whole protocols a VT420-class terminal has): the
  DEC locator, the host-writable status line, and xterm's special colours.
- **Built 2026-10-20, the owner's calls of that day:** typing hides the
  pointer (XTSMPOINTER), and a program can ask for and change the font
  (`OSC 50`).
- **Decisions needed:** the tertiary DA, the keyboard
  LEDs, the alternate function-key keyboards, printing, pointer shapes
  and colours, X resources, rxvt's font-shifting keys, and ECMA-48's C1
  functions one at a time.
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
| `CSI Ps # y` XTCHECKSUM | counted | ctlseqs line 1823: five bits that change DECRQCRA's checksum | the `CSI Ps # y` row | **built** 2026-10-20: `checksum_extension.rs` |
| `CSI > Pm t` / `CSI > Pm T` XTSMTITLE, XTRMTITLE | the set modes built 2026-10-20 | ctlseqs lines 1707 and 733: set titles in hex or UTF-8, and query them so | `CSI > Pm t` | **built** the two set modes; the query modes act on title reports, which stay refused as unsafe, and are counted with them (`the_query_modes_are_refused_with_the_title_reports`) |
| `CSI > Ps p` XTSMPOINTER | built 2026-10-20 | ctlseqs line 1464: whether the pointer hides while the user types -- never, unless mouse tracking is on, always but on leaving the window, always | `CSI > 2 p` | **built** 2026-10-20 on both systems, the owner's call of that day, verbatim: "3 46 and 50 all yes, pointer hiding - yes , ReGIS - yes", taken as in with xterm's default, hide while typing unless tracking; `[pointer] hide_while_typing` on or off (Jev, ON_OFF, 0.96/0.91), a value past 3 refused (Jev, IGNORE, 0.96/0.92) |
| `?30` | counted | ctlseqs DECSET `3 0`, "Show scrollbar (rxvt)" | the `?30` row | **built** 2026-10-20: `scrollbar_mode.rs`; partial on Linux, whose driver draws no scrollbar yet |
| `OSC 60` / `61` / `62` XTQALLOWED, XTQDISALLOWED, XTQALLOWABLE | answered since 2026-10-20 | ctlseqs line 2182: reply `OSC 6 0 ; Pt ST`, the allowed categories among seven xterm resource names | the `OSC 60` row | **built** 2026-10-20, each category named for what baud does |
| `?35` | counted | ctlseqs DECSET `3 5`, "Enable font-shifting functions (rxvt)" | none | **ask Jev**: the text names rxvt's functions and not their keys; baud's zoom is the nearest |
| DEC locator: `CSI Ps ; Pu ' z` DECELR, `' {` DECSLE, `' \|` DECRQLP, `' w` DECEFR | counted | ctlseqs lines 1776-1907, the VT330/340 locator | none | **build**: a whole input protocol of the DEC terminals, beside the mouse modes baud has |
| `CSI Ps $ }` DECSASD, `CSI Ps $ ~` DECSSDT | counted | ctlseqs lines 1993 and 2001, the host-writable status line | DECRQSS's row answers them invalid, "no status line" | **build**: a VT420 feature |
| `CSI Ps ; Pf ; Pb , \|` DECAC, `, }` DECATC, `CSI Ps ) {` DECSTGLT | counted | ctlseqs lines 1954-1965: the VT525's colour assignments | DECRQSS's row answers them invalid | **keep refused** while baud is VT420-class; note AP's level question decides it |
| `OSC 5` / `6` / `105` / `106` special colours | counted | ctlseqs lines 2232-2241: the colours bold, underline, blink, reverse and italic draw in | none | **build** with the palette |
| `CSI = Ps c` tertiary DA | counted | ctlseqs line 810: DECRPTUI, `DCS ! \| 00000000 ST` as xterm sends it | none | **ask Jev**, quoting the owner's DA1 pick: it is a VT400 report from a terminal that answers as a VT102 |
| `CSI Ps SP t` DECSWBV, `CSI Ps SP u` DECSMBV | built 2026-10-20 | the VT520 manual pp. 5-154 and 5-138: off, low, high, 0 read as DEC reads it | their rows | **built**: `bell_volume.rs`; on macOS low sounds as high, their rows' gap |
| `CSI Ps q` DECLL | counted | ctlseqs line 1536: light or put out Num, Caps and Scroll Lock | none | **the owner's**, beside note AH's title-bar lights |
| `?1050`-`?1053`, `?1060`, `?1061` | counted | ctlseqs DECSET `1 0 5 0` to `1 0 6 1`: the termcap, Sun, HP, SCO, legacy and VT220 keyboards | the VT220 table is `out` in `docs/input-coverage.md` | **ask Jev**: each is a key encoding, and the owner's rule asks for protocols whole |
| `CSI Ps i`, `CSI ? Ps i` MC, `?18`, `?19`, `?43`-`?46` | counted | ctlseqs lines 1047-1054 and DECSET `1 8` to `4 6`: printing and logging | none | `?46` **built** 2026-10-20, the owner's call (below), with `[screen] logging` to turn it off; **keep refused** `CSI 10 i` and `CSI 11 i`, which write a file; **the owner's** for a printer |
| `OSC 22`, `OSC 13`/`14`/`113`/`114` | counted | ctlseqs lines 2118 and 2128: the pointer's shape and colours | none | **ask Jev**: AppKit has named pointer shapes and no recoloured pointer |
| `OSC 50` | built 2026-10-20 | ctlseqs line 2137: set or query the font | the `OSC 50` row | **built** 2026-10-20 on both systems, the owner's call of that day, verbatim: "3 46 and 50 all yes, pointer hiding - yes , ReGIS - yes", taken as option D, set and query, on by default, `[font] changes = "off"` turning changes off; a name resolves to a face baud has loaded (Jev's lean, OWN_FACES, 0.79/0.85), `#+n` and `#-n` step the size (Jev's lean, SIZE_STEPS, 0.22/0.18), a relative query is answered empty (Jev, EMPTY, 0.91/0.91) |
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
| `?41` | counted | ctlseqs: "more(1) fix (see curses resource)"; xterm's `TabNext` wraps a pending wrap before a tab under it (`[[source: xterm-tabs-c]]` 113-118) | the `?41` row | **built** 2026-10-20: `more_fix.rs` |
| `OSC 3` | built 2026-10-20 | ctlseqs: "Set X property on top-level window" | the `OSC 3` row | **built**, the owner's call of 2026-10-20 (below): the window's and the icon's names mapped, every other property counted (`x_property.rs`) |
| `OSC 15`/`16`/`18`/`115`/`116`/`118`, `OSC 51` | counted | ctlseqs: the Tektronix window's colours, Emacs's reserve | none | **keep refused**: no Tektronix window, nothing assigned |
| `OSC 46` | built 2026-10-20 | ctlseqs line 2133: the log file's name | the `OSC 46` row | **built**, the owner's call of 2026-10-20 (below), with `?46` and a setting |
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

## The owner's calls of 2026-10-20

The owner, 2026-10-20, verbatim: "3 46 and 50 all yes, pointer hiding -
yes , ReGIS - yes". Read against the decisions page's options (TODO.md,
"DECIDED 2026-10-20"): `OSC 3` is option B, "map the properties with a
counterpart (window name, icon name), count the rest". The settings take
the owner's words of 2026-10-19: "i would say on or off, not answer,
keep it simple" (TODO.md dates this quote OWNER 2026-10-19). `OSC 46`
is option B, "log to the file the program names", "built with the
on/off setting pattern of the window operations, on by default". This
overturns this note's "keep refused" for both; the one for `OSC 46`
rested on a remote program naming a file to write, a risk the owner's
call keeps, behind a setting.
`OSC 50`, the pointer-hiding mode and ReGIS are recorded by their own
lanes.

- **`OSC 3`, built 2026-10-20.** `WM_NAME` and `_NET_WM_NAME` set the
  title, `WM_ICON_NAME` and `_NET_WM_ICON_NAME` the icon name, ICCCM's in
  ISO 8859-1 and EWMH's in UTF-8 (Jev, ICCCM_AND_EWMH 0.96/0.98 and
  BY_PROPERTY_TYPE 0.97/1.00, each on a second ask); a bare name deletes;
  any other name is counted in `Term::properties_dropped`. Refused with
  the window operations, as xterm's `allowWindowOps` gates it (Jev's
  lean, FOLLOWS_WINDOW_OPS, 0.38/0.52).
- **`?46` and `OSC 46`, built 2026-10-20.** The host's bytes from after
  `?46 h` to the end of `?46 l`, appended to the named file on a writer
  thread with xterm's ownership and mode checks (Jev, XTERM_CHECKS
  1.00/1.00); `-` standard output, `|` refused (XTERM_DEFAULT_BUILD
  1.00/1.00); no name, a generated one where the shell started, and a
  relative name read from there (Jev's leans, 0.85/0.72 and 0.86/0.58);
  `DECRQM ?46` `4` with `[screen] logging = "off"` (PERMANENTLY_RESET
  0.96/1.00); `RIS` keeps it (RIS_KEEPS 0.98/0.96).
