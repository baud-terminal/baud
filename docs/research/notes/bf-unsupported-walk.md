# BF. What baud still counts as unsupported, walked again

**Status: 2026-10-28.** Note AV took this walk on 2026-10-19, and most of
what it listed to build has been built since. This note takes it again
at `411bdc8c` and records only the change: what is still counted, what
the governing text says of each, and which of three it is --

- **(a)** buildable to the text with a true answer or action;
- **(b)** refused for a reason that still holds: unsafe, the wrong shape,
  or no counterpart in a terminal (each row says which);
- **(c)** a decided lean, or a question TODO.md marks as the owner's.

The walk classifies the families `docs/vt-coverage.md` already has, and
the forms section 2 marks **not classified** are a list of what it
found, not a reading: the 25 `ECMA-48` `SP` presentation functions,
DECDLD with DECDMAC and DECINVM, and `OSC 633`, `5522` and `5113` were
found counted and their governing texts were not read here. DECSTUI,
DECPFK and DECPAK carry (c) from TODO.md's owner markers alone, not from
a reading of their text. **Since 2026-10-28 every one of them is read
at source and classified (section 8)**, and the walk is complete.

The rule it is read against, the owner's of 2026-09-25: "full
VT420-class terminal. we need to support such protocols fully, always!"
A refusal stands only where answering is unsafe or would be wrong-shaped.

## 1. How the walk was taken

The harness AV describes, rebuilt and not committed: every form
`[[source: ctlseqs]]` lists (`CSI`, `ESC`, `OSC`, `DCS`), each with every
`Ps = N` its section lists, bare and with `1`, fed to a fresh 80x24
`Term`; then every `ESC` final `0x30-0x7E` alone and behind each
intermediate, in ANSI mode and in VT52 mode; then every `CSI` final bare,
with `1`, and behind `SP`. The five counters were read after each.

Hits that come from the probe and not from baud were checked and dropped:

- XTWINOPS 11, 13, 14, 15, 16 and 19 are answered from the window facts
  the embedder hands over (`Term::set_window`), and a fresh `Term` has
  none; `?80`, `?1070` and `?8452` wait on the Sixel claim, as decided
  for V18; `?1020`-`?1023` and `?13` are read-only.
- An `OSC 4`, `10`-`19` or `106` body of `1` is no colour or pair; an
  `OSC 3`, `52` or `22` body of `1` is no property, selection or name;
  `CSI ? 1 S`, `CSI 1 $ x` and the two-parameter colour forms with one
  parameter are malformed. A `CSI Ps c` value of 15 to 29 is a value of
  DA1's reply, not of its request.
- The VT52 probe leaves with `ESC <`, which ANSI mode counts.

What remains is in sections 2 to 4.

## 2. What is still counted, and what has changed since AV

| family | the governing text | class | since AV |
|---|---|---|---|
| `ESC l` / `ESC m`, memory lock and unlock | `[[source: ctlseqs]]` 414-417: "Memory Lock (per HP terminals). Locks memory above the cursor." and "Memory Unlock (per HP terminals)"; xterm acts on both unconditionally (`[[source: xterm-vtprstbl-c]]` `esc_table` 6133-6134; `[[source: xterm-reset]]` 4759-4770: the top margin to the cursor's line, or to the first, the bottom kept, the cursor left where it is). `[[source: ecma48]]` assigns 06/12 and 06/13 nothing, and Table 5 (5.5) leaves them "reserved for future standardization and shall not be used"; no HP manual is pinned. The `xterm-256color` entry baud's `TERM` names carries `meml=\El` and `memu=\Em` (`[[source: terminfo-xterm-256color]]`) | (c) | **missed by AV**, which read `e`-`m` as unassigned. Counted; Jev chose KEEP_COUNTED at 0.90/0.90 in both orders on the re-ask and at 0.76/0.50 on a third ask with SGR 58/59 and DECID beside it (section 6); a row since this note (section 5). TODO.md marks `ESC l/m` as the owner's call because it is "xterm-resource-gated". For `l` and `m` that premise does not hold: xterm's `esc_table` maps them at `[[source: xterm-vtprstbl-c]]` 6133-6134 with no `#if`, and the handler at `[[source: xterm-reset]]` 4759-4770 tests no resource. Only `ESC F` has a gate, `hpLowerleftBugCompat`. The owner's call is still open, and KEEP_COUNTED is Jev's lean for it |
| `` ESC ` `` DMI / `ESC b` EMI | `[[source: ecma48]]` 8.3.34, "DMI causes the manual input facilities of a device to be disabled", and 8.3.43, "EMI is used to enable the manual input facilities of a device"; both Fs, `ESC 06/00` and `ESC 06/02`, with no C1 form, both allocated in Table 5. KAM, 7.2.11: "DISABLED: All or part of the manual input facilities are disabled. NOTE No control functions are affected" | (a) | AV: "ask Jev, one at a time". Jev leans to one lock with KAM's, SHARE_KAM, 0.73/0.82 and 0.56/0.59 asked again, under the bar. The lane rule takes a lean after one re-ask, but this lane's brief builds only what Jev decides at 0.9 or more in both orders, so the lean is recorded and not built. Built on KAM's shared lock, DMI and EMI would leave nothing counted; unbuilt, both stay counted, and that is the gap the owner would be taking. Next work |
| `ESC T` CCH | 8.3.8: "both the preceding graphic character in the data stream ... including SPACE, and the control function CCH itself are to be ignored"; after a control, "not defined by this Standard" | (a), not small | AV: ask Jev. Undoing the last graphic character means keeping it and the cell it overwrote on the per-byte path, a change to what is written per byte that rules 1 and 2 need measured. Next work |
| `ESC K` PLD / `ESC L` PLU | 8.3.92 and 8.3.93: a partial line down or up, sub- and superscript | (a), not small | needs a half-line position in the cell and the renderer. Next work |
| `ESC U` MW | 8.3.83: "set a message waiting indicator in the receiving device", acknowledged by DSR | (a), not small | an indicator the embedder must draw, beside DECLL's lights; "never record a mode you do not act on". Next work, with DECLL |
| `ESC B` BPH, `ESC C` NBH | 8.3.4 and 8.3.85: where a line break may or may not occur "when text is formatted" | (b) no counterpart | baud wraps at the line's end, and does not format text |
| `ESC Q` PU1, `ESC R` PU2, `ESC S` STS, `ESC a` INT, `ESC d` CMD | private use with no meaning; the transmit state; "some agreed procedure"; the coding method's delimiter | (b) no counterpart | unchanged |
| `ESC F` SSA, `ESC G` ESA | `[[source: ecma48]]`: the start and end of a string "eligible to be transmitted ... or transferred to an auxiliary input/output device" | (c) | REFUSE_COUNT, 1.00/1.00, 2026-10-26 (`ESC F`'s xterm meaning is off by default, `hpLowerleftBugCompat`) |
| `ECMA-48`'s `SP`-intermediate presentation functions: GSM `SP B`, GSS `SP C`, FNT `SP D`, TSS `SP E`, JFY `SP F`, SPI `SP G`, QUAD `SP H`, PFS `SP J`, SHS `SP K`, SVS `SP L`, IGS `SP M`, IDCS `SP O`, DTA `SP T`, FNK `SP W`, SPQR `SP X`, SEF `SP Y`, PEC `SP Z`, SSW `SP [`, SACS `SP \`, STAB `SP ^`, GCC `SP _`, SCO `SP e`, SRCS `SP f`, SCS `SP g`, SLS `SP h` | `[[source: ecma48]]` 8.3 | (a) 17, (b) 7, (c) 1 | AV probed `CSI` finals bare and behind `?`, `>` and `=`, never behind `SP`. Read 2026-10-28, each classified in section 8; the 8 not to build have a row each |
| `DCS Pfn ; Pcn ; ... { ... ST` DECDLD soft fonts; `DCS Pid ; Pdt ; Pen ! z ... ST` DECDMAC and `CSI Pid * z` DECINVM, macros | `[[source: vt420-rm]]` pp. 44-48 and 114 on; `[[source: dec-std-070]]` pp. 5-182 to 5-185 | (a), not small | read 2026-10-28 (section 8). VT420 functions; `CSI ? 62 n` answers that there is no macro space, which is true and is the gap. Next work, with the forks section 8 lists |
| `DCS ! { ... ST` DECSTUI; `DCS " x` DECPFK, `DCS " y` DECPAK | `[[source: vt420-rm]]` Table 4-1; `[[source: dec-std-070]]` 070-4 p. 4-31; `[[source: vt510-rm-b]]` pp. 5-77, 5-83 | DECSTUI (c); DECPFK, DECPAK (b) and (c) | read 2026-10-28 (section 8), a row each. DECSTUI's one reader is DA3, the owner's; DECPFK and DECPAK are the VT510's, above the VT420 class, and carry DECUDK's owner question |
| `CSI Ps q` DECLL | ctlseqs 1536 | (c) | the owner's, beside note AH's title-bar lights (Jev, DRAWN 0.31 / DRAWN_GATED 0.27, 2026-10-20, under the bar) |
| `CSI = Ps c` DA3, DECRPTUI | ctlseqs 810 | (c) | TODO.md: "Owner: answer DA2 and DA3?" |
| `?1050`-`?1053`, `?1060`, `?1061` | ctlseqs DECSET | (c) | TODO.md: "Owner: `?1050`-`?1053`, `?1060`, `?1061`" |
| `CSI Ps * \|` DECSNLS | ctlseqs | (a) | resizes the window as DECSLPP does; XTWINOPS 1-10 and DECSLPP are the owner's call of 2026-10-19 ("4 it is"), built and on by default, so DECSNLS rides the same window request |
| `CSI Ps ; Pf ; Pb , \|` DECAC, `, }` DECATC, `CSI Ps ) {` DECSTGLT | ctlseqs 1954-1987, "VT525 only"; xterm gates both on `terminal_id >= 525` and its level 5 table (`[[source: xterm-reset]]` 5930-5990) | (b) above the VT420 class | a VT525 function, above DECSCL's Level 4, as AV kept it; unchanged |
| `OSC 66` kitty text sizing | kitty's protocol | (c) | TODO.md: no row, the owner's call |
| `OSC 633` VS Code's shell integration, `OSC 5522` kitty's clipboard, `OSC 5113` kitty's file transfer | `[[source: vscode-shell-integration]]`, `[[source: kitty-clipboard]]`, `[[source: kitty-file-transfer]]` | `633` (c) a lean; `5522` reads (b), writes (a) not small; `5113` (b) unsafe | pinned and read 2026-10-28 (section 8), a row each |
| VT52 `ESC E`, `L`, `M` and every final VT52 does not list | ctlseqs 3391-3431 lists every VT52 form | (b) no function | unchanged |
| the `ESC` finals no text assigns, `CSI` finals likewise, `PM`, `SOS`, other `APC`s, charsets no list names, SGR 56 and 57 | as AV | (b) | unchanged |

**Built since AV**, so no longer counted: `OSC 5`, `6`, `105`, `106`
(the special colours), the DEC locator, the host-writable status line,
`OSC 22` (partial), `OSC 13`/`14` (decided out), `DCS + Q` XTGETXRES,
MC and printing, `?35` DECHEBM, `OSC 60`-`62`, and the ECMA-48 page,
line-limit, tabulation and area functions of 2026-10-26 and 27.

## 3. The 26 `out` rows

Each row's reason was read against its scope. None says less than the
governing text does, so none is reopened. A 27th, `ESC l` / `ESC m`,
is added by this note's lane (section 5).

| row | class | the reason, and why it holds |
|---|---|---|
| 8-bit C1 introducers under UTF-8 | (b) wrong shape | continuation bytes there; done under ISO 8859-1 |
| GR as GL inside a sequence, under UTF-8 | (b) wrong shape | lead and continuation bytes there; done under ISO 8859-1 |
| `CSI Ps ^` SIMD | (c) | SIMD_COUNTED, a lean; not reopened (the brief) |
| `CSI > c` DA2 | (c) | the owner's DA1 pick and TODO.md's owner question; not reopened |
| `?2002`, `?2003` | (c) | REASON, 0.94/0.94; not reopened |
| SGR 56, 57 | (b) no function | ECMA-48 § 8.3.117 reserves them, ctlseqs assigns nothing |
| `DCS + p` XTSETTCAP | (c) | COUNT_REFUSE, a lean: no host terminal database, so every retrieval fails as the text says |
| `OSC 9 ; 1`-`12` but `9 ; 4` | (b) unsafe, or carried elsewhere | five run processes, macros or block; the rest are `OSC 0`-`2`, `7` and `133` |
| `OSC 777` but `notify` | (b) no counterpart | a call into urxvt's perl |
| `OSC 1337 ; StealFocus`, `SetProfile`, `SetProfileProperty` | (b) unsafe | focus stealing; rewriting the user's settings |
| `OSC 1337 ; RequestUpload` | (b) unsafe | files leave the machine; the reply's newline types a command |
| `OSC 1337 ; Custom`, `ShellIntegrationVersion`, `ClearCapturedOutput`, `Disinter`, and the keys its pages do not name | (b) no counterpart | iTerm2's Python API, its own scripts, its toolbelt, its buried sessions, its ssh integration |
| `OSC 13`, `14`, `113`, `114` | (c) | 0.98/0.97: the system draws the pointer |
| `OSC 15`, `16`, `18`, `115`, `116`, `118` | (c) | the Tektronix window's |
| `OSC 21` as DECSWT | (c) | a VT520 function; the number is kitty's colour control since 2026-10-22 |
| `OSC 51` | (b) no function | "reserved for Emacs shell" |
| Tektronix 4014 | (c) | taken lean, 2026-10-20; not reopened |
| `?64` DECPCCM | (c) | NOT_RECOGNIZED, a lean; it comes with several pages |
| `CSI SP i` / `SP j` SPH, SPL | (c) | REFUSE_COUNT, a lean; not reopened |
| `CSI 6 i` / `7 i` locator controller mode | (b) no counterpart | no locator port; the pointer is the window system's |

## 4. The handlers' own refusals

AV's table of refusal counters stands; each has its decision. Those
added since are the same kind: `Term::fonts_refused` (`OSC 50` changes
while changes are off, or naming a face not loaded),
`Term::window_ops_refused` (the window operations while the setting has
them off), `Term::directories_refused` (an `OSC 7` off RFC 8089's
grammar), `Term::logs_refused` (logging off, or a start naming a
command), `Term::variants_refused` (SAPV 12, which a cell grid has no
column for), `Term::strings_refused` (an SDS or SRS end with nothing
open, or past UAX #9's depth), `Term::locator_keys_refused` (a DECLKD
key past 4 or a string past six bytes) and `Term::tcap_sets_refused`.
Each declines a request it understood, for a reason its row gives; none
is a protocol left unbuilt.

One exemption had expired. The terminfo conformance test, which feeds
every output capability of each entry baud claims and fails on one that
is counted, exempted `mc0`, `mc4`, `mc5` and `mc5p` as "printer support,
out of scope", and media copy is built since 2026-10-20: with the
exemption a regression in MC passed the `ansi`, `vt100`, `vt102`,
`xterm`, `xterm-16color` and `xterm-256color` profiles. It is removed
(section 5). `meml` and `memu` stay exempt, for the reason of Jev's
lean here.

## 5. Built here

Nothing a user sees changes; no file under `baud-core/src` changed.

1. **The printer capabilities lose their exemption.** `mc0`, `mc4` and
   `mc5` are acted on, and the profiles pass without it. Red first: with
   MC perturbed to count, the `ansi`, `vt100`, `vt102`, `xterm`,
   `xterm-16color` and `xterm-256color` profiles pass at the parent and
   fail without the exemption, naming `mc0` (all but `ansi`), `mc4` and
   `mc5`. At the parent, `every_spec_csi_form_is_handled_or_counted`,
   `every_spec_csi_final_is_handled_or_counted` and
   `every_done_sequence_reaches_a_handler` already failed under that
   perturbation, so only the profile tests were blind.
2. **`ESC l` and `ESC m` have an `out` row** in `docs/vt-coverage.md`
   and `hp_memory_lock_and_unlock_are_counted_and_move_no_margin` holds
   them counted and moving no margin, red under three perturbations (xterm's
   lock built and not counted; built and counted; only `ESC m` built).

DMI and EMI, the one small (a) left, are not built: the lane rule
takes a lean after one re-ask, but this lane's brief builds only what
Jev decides at 0.9 or more in both orders. Unbuilt, both stay counted;
built on KAM's shared lock, they would leave nothing counted.

## 6. Jev

Asked uncached in two orders, the owner's rules of 2026-09-25, 10-08 and
10-10 and the DA1 pick quoted, with ctlseqs, xterm's `esc_table` and
`CASE_HP_MEM_LOCK`, and ECMA-48's DMI, EMI and KAM:

| question | first ask | asked again, with Table 5 and the terminfo entry | outcome |
|---|---|---|---|
| `ESC l` / `ESC m` | KEEP_COUNTED 0.82 / 0.82 | KEEP_COUNTED 0.90 / 0.90 | see below |
| DMI / EMI | SHARE_KAM 0.73 / 0.82 | SHARE_KAM 0.56 / 0.59 | under the bar; not built |

The second ask of `ESC l` reached the bar, KEEP_COUNTED 0.90 / 0.90 in
both orders. It was asked a third time, after a premise change: SGR
58/59 and DECID already act on codes ECMA-48 reserves because a pinned
text assigns them (the SGR 56/57 row; the `Esc::Identify` doc), and
Table 5's "shall not be used" is the one difference. That ask went
beyond the one re-ask the rule provides, and read KEEP_COUNTED 0.76 /
0.50. Both asks chose the same option, so nothing built changes. Since
TODO.md marks `ESC l/m` as the owner's, the result is Jev's lean for the
owner's call, which is still open. The log: decisions.jsonl 1745-1746
for the first ask, 1747-1748 for the second, 1749-1750 for the third.

## 7. Next work

1. DMI and EMI on KAM's lock, Jev's lean SHARE_KAM, if the owner takes
   it: `DECRQM 2`, DECTSR, `RIS` and `DECSTR` then follow KAM's, and
   neither is counted any more. Not built here because the brief builds
   only at 0.9 or more in both orders.
2. ~~The `SP` presentation functions~~, ~~DECDLD, DECDMAC, DECINVM,
   DECSTUI, DECPFK and DECPAK~~, ~~`OSC 633`, `5522`, `5113`~~: read
   and classified 2026-10-28 (section 8); what is left to build is
   there.
3. CCH, measured on the per-byte path; PLD and PLU with the renderer; MW
   with DECLL's lights.
4. DECSNLS on the window request DECSLPP makes (section 2).

## 8. The families read on 2026-10-28

Each read at source: ECMA-48 through `scripts/extract-ecma48.py`, whose
positive control passed first; the VT420 and VT510 manuals and DEC STD
070 re-fetched with their digests matching and read on the text layer,
DECSTUI and DECDLD found in STD 070 by the same search that finds no
DECPFK or DECPAK; VS Code's and kitty's pages pinned at a commit.

| function | the governing text | class | why |
|---|---|---|---|
| JFY `SP F`, QUAD `SP H` | 8.3.73, 8.3.102: a string justified, or positioned "flush to line home position margin", centred or "flush to line limit position margin" | (a), not small | SLH and SLL give the line home and limit, and TAC, TALE and TATE already lay a string out at a stop; JFY's word fill and hyphenation are formatting beside `[screen] wrap_words` |
| IDCS `SP O` | 8.3.65: 1 "reserved for use with the DIAGNOSTIC state" of SRTM, 2 "reserved for Dynamically Redefinable Character Sets (DRCS) according to Standard ECMA-35" | (a), not small | with DECDLD |
| GCC `SP _` | 8.3.54: "two or more graphic characters are to be imaged as one single graphic symbol" | (a), not small | the renderer's; "does not explicitly specify the relative sizes or placements" |
| SCO `SP e` | 8.3.110: rotation in steps of 45 degrees | (a), not small | the renderer's; a quarter turn of a narrow glyph does not fit its cell |
| FNT `SP D` | 8.3.53: "Ps2 identifies the character font according to a register which is to be established" | (b) wrong shape | no register exists, so no `Ps2` names a font |
| IGS `SP M` | 8.3.66: identifies a repertoire "registered in accordance with ISO/IEC 7350" | (b) no function | it identifies and selects nothing; what a byte draws is the encoding's and SCS's |
| FNK `SP W` | 8.3.52: "identifies the function key which has been operated" | (b) wrong direction | a device's report to its host |
| SPQR `SP X`, SEF `SP Y` | 8.3.134, 8.3.116: print quality and speed; a sheet ejected "into a specified output stacker" and loaded "from a specified paper bin" | (b) no counterpart | baud prints to the system's print panel, which chooses both |
| STAB `SP ^` | 8.3.144: the list of stops "specified in other standards, for example ISO 8613-6" | (b) no counterpart | no text baud follows makes the list, so no `Ps` names a stop |
| GSM `SP B`, GSS `SP C`, TSS `SP E`, SPI `SP G`, SHS `SP K`, SVS `SP L`, PEC `SP Z`, SSW `SP [`, SACS `SP \`, SRCS `SP f`, SCS `SP g`, SLS `SP h` | 8.3.55, 56, 157, 132, 118, 149, 90, 140, 107, 136, 112, 124: a character's size, its escapement, or the line and character spacing | (a), not small, a lean | on baud's grid each is the cell's; to build on the cell machinery `OSC 66` would bring (Jev's lean, BUILD_LATER, below). DEC STD 070's character cells have none of them, and its ReGIS text has them, on the page (070-8 § 8.6.3) |
| PFS `SP J` | 8.3.91: "the available area for the imaging of pages of text based on paper size", the pages "introduced by the subsequent occurrence of FORM FEED (FF)"; Annex E, Table E.1, each format's lines and characters by line and character spacing | (b) no counterpart | baud has one page and its FF is `LF`, so no later page is introduced for the area to apply to, as SPH's page home is read by nothing; DECSLPP, done, "sets the number of display lines per page" now (`[[source: dec-std-070]]` p. 5-75). Table E.1's column is SVS's and SHS's, neither built. Jev, REFUSE_COUNT, 0.95 / 0.95 (below) |
| DTA `SP T` | 8.3.36: "the dimensions of the text area for subsequent pages", in SSU's unit | (c) a lean | as PFS, no subsequent page begins; built, it would be `CSI 8 ; Pn1 ; Pn2 t`'s window request, in CHARACTER, SSU's default, since baud does not act on SSU. Jev's lean REFUSE_COUNT, 0.69 / 0.73 (below) |
| DECDMAC `DCS ! z`, DECINVM `CSI * z` | `[[source: dec-std-070]]` pp. 5-182 to 5-185; `[[source: vt420-rm]]` pp. 44-48 | (a), not small | see below |
| DECDLD `DCS {` | `[[source: vt420-rm]]` p. 114 on | (a), not small | soft fonts: a glyph store and the renderer's; IDCS 2 with it |
| DECSTUI `DCS ! {` | `[[source: vt420-rm]]` Table 4-1; STD 070 070-4 p. 4-31 | (c) | its one reader is DA3, "factory set", whose answer is an open question |
| DECPFK `DCS " x`, DECPAK `DCS " y` | `[[source: vt510-rm-b]]` pp. 5-77, 5-83 | (b) above the VT420 class, and (c) | the VT510's, in neither the VT420 manual nor STD 070, as DECAC is; and DECUDK's owner question |
| `OSC 633` | `[[source: vscode-shell-integration]]`: "These sequences should be ignored by other terminals" | (c) a lean | kitty ignores it by name (`[[source: kitty-6d5d0c4-vt-parser]]` 650), Windows Terminal routes it to one action, `Completions` (`[[source: wt-c91793f-output-state-machine-engine]]` 902-904, `[[source: wt-c91793f-adaptdispatch]]`); Jev split (below), KEEP_COUNTED kept |
| `OSC 5522` read | `[[source: kitty-clipboard]]`: "a terminal that rejects a read request must simply ignore it" | (b) unsafe | as an `OSC 52` read; `DECRQM ? 5522` answers not recognized |
| `OSC 5522` write | the same: "they must accept at least 64MB of data" | (a), not small | several MIME types, chunked, with error replies |
| `OSC 5113` | `[[source: kitty-file-transfer]]`: "a session must be approved by the user in the terminal emulator" | (b) unsafe | files leave or reach your disk, as `RequestUpload`; baud has no prompt |

**Macros, the forks before they are built.** DECDMAC stores up to 64
macros in "6k bytes", text or hex pairs with a repeat introducer; DECINVM
substitutes one "as if received from the host", "up to 16 levels of
nesting, but cannot invoke themselves recursively"; `RIS` clears them and
`DECSTR` does not; under printer controller mode DECINVM goes to the
printer unexpanded; `?62` DECMSR and `?63` DECCKSR, which answer "none"
now, then answer the space and a checksum. Three questions are open: a
bound on what one invocation expands to (sixteen levels, each invoking
the next many times, expand a short definition without limit inside one
read); DECCKSR's checksum, "four hexadecimal digits", for which no
pinned text gives an algorithm; and expansion re-entering the parser
from inside a dispatch.

**Built here**: nothing a user sees changes, and no file under
`baud-core/src` changed. Eight rows for the `SP` functions and five for
DECSTUI, DECPFK and DECPAK, `OSC 633`, the `OSC 5522` read and `OSC
5113`, held by `the_typesetting_functions_are_counted_and_write_nothing`,
`the_typesetting_functions_each_have_an_out_row` and
`the_refused_strings_are_counted_and_answered_with_silence`.

Jev, `OSC 633`, uncached, the standing rule quoted:

| ask, confidences | order A | order B |
|---|---|---|
| first | KEEP_COUNTED 0.16 | BUILD_ON_133 0.42 |
| with kitty's and Windows Terminal's practice | KEEP_COUNTED 0.60 | BUILD_ON_133 0.14 |

Under the bar both times, and in all four asks Jev chose the option
listed second, so the split carries no signal either way. KEEP_COUNTED
stands as what baud already does and what the page asks, a lean for the
owner's veto.

Jev, the twelve size and spacing functions, uncached in two orders, the
standing rule and the open `OSC 66` question quoted, the same pick each
time:

| ask, confidences | order A | order B |
|---|---|---|
| first: BUILD_LATER, REFUSE or WITH_OSC_66 | BUILD_LATER 0.40 | BUILD_LATER 0.37 |
| with DEC STD 070 and baud's ReGIS text | BUILD_LATER 0.27 | BUILD_LATER 0.31 |

Under the bar; BUILD_LATER is taken as the lean, so the twelve are
listed to build and have no row.

Jev, PFS and DTA on their own text, uncached in two orders, the owner's
"4 it is" (2026-10-19), the standing rule and the SPH and SPL precedent
quoted:

| ask, confidences | order A | order B |
|---|---|---|
| first: DTA | RESIZE_NOW 0.31 | RESIZE_NOW 0.52 |
| first: PFS | REFUSE_COUNT 0.72 | REFUSE_COUNT 0.72 |
| with DEC STD 070's DECSLPP and the search of ctlseqs and the DEC texts: DTA | REFUSE_COUNT 0.69 | REFUSE_COUNT 0.73 |
| the same: PFS | REFUSE_COUNT 0.95 | REFUSE_COUNT 0.95 |

The research added was the timing: DECSLPP's text acts on the page now,
where DTA's names "subsequent pages" and PFS's the pages a later FF
introduces; and practice, where ctlseqs, DEC STD 070 and the VT420 and
VT510 manuals have neither, the same searches finding DECSLPP in each.
PFS is decided at the bar. DTA's REFUSE_COUNT is the lean after the one
re-ask, for the owner's veto.
