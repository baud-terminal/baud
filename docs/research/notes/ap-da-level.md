# AP. The Primary DA level: can baud answer `?61;4c`?

**Status: research, 2026-10-15. Nothing is built and the DA reply is
unchanged.** baud answers `CSI ? 6 c`, and `CSI ? 6 ; 4 c` when the embedder
opts in to Sixel (`baud-domain/src/vt/report.rs`, `DeviceAttribute::Vt102`
and `SixelClaim`). A held commit, "app: the Primary DA claims Sixel by
default", not landed, turns the opt-in on by default.

**The answer, in short: no.** baud does not meet DEC STD 070's Level 1 today.

* Nine functions that STD 070 requires at Level 1 are counted and not acted
  on: SS2, SS3, DECID, DECSWL, DECDWL, DECDHLT, DECDHLB, KAM and SRM. LS2 and
  LS3 are two more if 070-3 §3.6.1.1 governs (section 4).
* Three more are partial: DEC Special Graphics at 5/15, SUB's error
  character, and DECSCLM, which no embedder acts on.
* baud has no Level 1 operation: no 7-bit mode that strips the eighth bit,
  and no Level 1 key set.
* The DA extension, which `?61` itself uses, also wants DA2 and DECSCL, and
  baud has neither.
* Read literally, 070-1 §1.5.3.1.1 also forbids a Level 1 device from
  implementing Level 2 functions it does not implement in full. baud
  implements many of them.

Section 7 lists what is missing, and section 8 the choices it leaves for Jev
or the owner.

## 1. The question

Jev picked "advertise Sixel by default" at 0.94 and 0.98 (decision log lines
559 and 560). The change built for it makes every session answer
`CSI ? 6 ; 4 c`, and no text defines that shape:

* `[[source: ctlseqs]]` 783-785: "The VT100-style response parameters do not
  mean anything by themselves. VT220 (and higher) parameters do".
* `[[source: xterm-reset]]`'s `charproc.c` 4112-4156 appends extensions only
  when `display_da1` is 200 or more. Below that, `case 102` sends a bare `6`
  (4135).

Asked for the shape, Jev picked SEND_61_4_AFTER_LEVEL1_CHECK, "Answer ?61;4c
(level 1 plus Sixel) as DEC STD 070 Table 1 allows, but only after checking
baud against STD 070's level 1 requirements and pinning the missing VT340 text
volume; until then do not flip the default" (log line 570, 0.91). On the
replicate the same pick came at **0.79** (line 571). That is under the 0.9 the
owner's 2026-10-10 rule asks on both asks, so the shape is not decided, and
the rule sends it to research. This note is that research.

baud's own rule is in `docs/protocol-decisions.md`, "DA under-claims": "a DA
identity is a promise about *everything* in that level".

## 2. The texts, and how they were read

* **`[[source: dec-std-070]]`**, EL-SM070-00, 1,252 PDF pages, re-fetched
  2026-10-15 from its manifest URL. Its sha256 matched the pin. Its chapters
  carry different dates:
  * 070-1 (Concepts and Conformance), 15-May-1983;
  * 070-3, 14-Apr-1989;
  * 070-4, 04-Oct-1990;
  * 070-5, 29-Jun-1990;
  * 070-6, 19-Apr-1988;
  * 070-9, 03-Aug-1990.

  The text layer is OCR. `pdftotext -layout` (poppler 26.09.0) gave 2,320,788
  bytes, and every `Levels:` field was collected by script (section 9).
  - Positive control: the scan found the fields of functions already known to
    be in the document, such as DSR (PDF p. 239), SGR (358) and DECSC (442),
    140 in all.
  - Every quotation below from 070-1 §1.5.3, §1.6.2, §1.6.3 and §1.7.2, and
    from 070-4 Table 1, was read on the page image (PDF pp. 55, 56, 58-61, 63
    and 223) [img]. The rest is [text].
* **`[[source: vt3xx-tp]]`**, the VT330/VT340 Programmer Reference Manual,
  Volume 1: Text Programming, EK-VT3XX-TP-002 (May 1988). It was found on
  bitsavers (`/pdf/dec/terminal/vt340/`) and pinned in this commit as
  `external`, beside its DEC siblings.
  - Its text layer is Acrobat Paper Capture OCR, 826,920 bytes. Positive
    control: "cursor" 371 times, and the index entry "Device Attributes (DA),
    196" leads to the section.
  - The DA pages (printed 196-199, PDF 205-208) and DECSCL (printed 68, PDF
    77) were read on the page image [img].
  - The first edition, EK-VT3XX-TP-001 (March 1987, same directory,
    54,100,022 bytes, sha256 `e60d032f...`), has no text layer: `pdftotext`
    returns 348 bytes. It was hashed and not read, and it is not pinned.
* Read for the code side: `docs/vt-coverage.md`, notes AE and AN, and baud at
  `bf3b6762`. The counted functions were confirmed by a probe run against a
  `git archive` of that commit (section 9).

## 3. What a `61` says

**The first parameter is the highest level the device supports.** 070-4 DA
(PDF pp. 221-222): "a response with a first parameter of 61 indicates that the
device conforms to Level 1 of the Character Cell Display architecture" and
"the device code in the response represents the highest level that the device
can support, and does not indicate the level at which the device is currently
operating". The rest of the parameters "indicate the architectural extensions
supported at the current operating level".

**Extension 4 is defined at Level 1.** Table 1 (PDF p. 223) [img]: "4 | 1 - 4
| Sixels Display". So `?61;4c` is a shape STD 070 defines. `?6;4c` is not one,
and no text read shows it.

**Neither the VT340 nor xterm sends `61`.**

* `[[source: vt3xx-tp]]` p. 197 (PDF 206) [img]: "Psc indicates the
  terminal's maximum operating level, regardless of the current operating
  level". Its table has one row: "63 Level 3 (VT200 or VT300 family)".
* The example on p. 198 (PDF 207) [img] answers `CSI ? 63; 1; 2; 3; 4; 6; 7;
  8; 9; 13; 15; 16; 18; 19c`.
* Table 12-1, "Alias Primary DA Responses From the VT300", p. 199 (PDF 208)
  [img], gives "VT102 DA ESC [ ? 6 c" with no extension. The aliases that
  carry extensions are Level 2: VT220 `CSI ? 62; 1; 2; 6; 7; 8; 9 c` and
  VT240 `CSI ? 62; 1; 2; 3; 4; 6; 7; 8; 9 c`.
* So DEC's Sixel terminals report Sixel under 62 or 63, and DEC's own VT102
  answer is the bare alias.
* The alias form is sanctioned in 070-4 DA's Known Deviations (PDF p. 225):
  "Some terminals provide an alias ID (identification) capability, selectable
  under local control, to emulate the DA response of older terminals".
* The VT340's DECSCL page (p. 68, PDF 77) [img] makes Level 1 "VT100 mode",
  `CSI 6 1 " p`.

**What a level claim promises.** 070-1 §1.5.3.1.1, p. 1-15 (PDF 55) [img]:
"Terminal hardware/firmware claiming to conform to Level n:

1. shall implement ALL functions of the lowest defined level to Level n,
2. may NOT implement any functions defined by a higher level unless it
   implements all of the functions of that level (in which case it would
   conform to the higher level),
3. may NOT implement any function not defined in any level (unless it is
   included in an appendix to both the architectural and product
   specifications and clearly identified as an exception to the
   architecture)."

§1.5.3.1.2: "Terminal hardware/firmware claiming to implement an extension
shall implement ALL functions of that extension and the associated
conformance level".

## 4. Level 1, function by function

**What Level 1 requires is the union of four lists:**

* 070-1 §1.6.2, "Required Functions", pp. 1-18 and 1-19 (PDF 58-59) [img];
* every `Levels:` field that names a bare 1 (a `1x` field is an extension);
* 070-3's Level 1 operation;
* 070-6 §6.2.1 and §6.2.4, the keyboard.

**Where the lists disagree, the row says so:**

* SRM is `Levels: 1-4` on its page and absent from §1.6.2.
* LS2 and LS3 are required "in Level 1 operation" by §3.6.1.1, while their
  own pages say "Levels: 2, 3".
* DECID is "Levels: 1, 2".

**Not counted:** Appendix D's documented exceptions. These are RIS, DECALN,
DECTST and DECAWM. 070-5's change history, item 19, p. 5-219 (PDF 474)
[text], reads "Made Auto Wrap Mode an exception to Level 1". By §1.5.3.1.1's rule 3 an exception may be
implemented and is not required.

**Counted as `implemented`** when `docs/vt-coverage.md` marks the row `done`,
whose test gate is that file's, or where the code is cited. **Counted as
`missing`** where the probe in section 9 saw the sequence counted as
unsupported (rows 5-7, 13-16, 21 and 49-51), or, for a mode of operation,
a key set or a Set-up feature, where no code or configuration provides it
(rows 54, 56 and 57). **`unchecked`** is a row nothing here tested; it is
left out of the tally.

### 4.1 070-1 §1.6.2, the required controls

| # | function | where | baud | status |
|---|---|---|---|---|
| 1 | NUL | §1.6.2 | ignored; the `0x00` row | implemented |
| 2 | SP | §1.6.2 | a graphic character, written to a cell | implemented |
| 3 | SI (LS0) | §1.6.2; LS0 p. 3-73 (PDF 178), Levels 1, 2, 3 | the `0x0F` row | implemented |
| 4 | SO (LS1) | §1.6.2; LS1 p. 3-74 (PDF 179) | the `0x0E` row | implemented |
| 5 | SS2 | §1.6.2; SS2 p. 3-80 (PDF 185), Levels 1, 2, 3 | `ESC N` counted in `unsupported`; unbuilt, note AE | missing |
| 6 | SS3 | §1.6.2; SS3 p. 3-81 (PDF 186) | `ESC O` counted | missing |
| 7 | DECID | §1.6.2; 070-4 DECID (PDF 232), Levels 1, 2: "The device responds by transmitting an appropriate primary Device Attributes (DA) response" | in ANSI mode `ESC Z` was counted: `Esc::parse` (`baud-domain/src/vt/esc.rs`) had no `Z`. Since 2026-10-16 it answers what `CSI c` answers (`Esc::Identify`, the `ESC Z` row of `docs/vt-coverage.md`). VT52 mode answers `ESC / Z`, `docs/protocols/vt52.md` | implemented 2026-10-16 |
| 8 | DSR | §1.6.2; 070-4 DSR (PDF 239), at Levels 1-4 only `5`, answered `0` or `3` | `CSI 5 n` answers `CSI 0 n`; the `CSI n` row | implemented |
| 9 | DECSTBM | §1.6.2; p. 5-25 (PDF 280) | the `CSI r` row | implemented |
| 10 | SCS, ASCII into G0, G1 | §1.6.2; SCS p. 5-117 (PDF 372), note 1: "In Level 1 operation character sets can only be designated into G0 and G1" | the `ESC (` rows | implemented |
| 11 | SCS, Line Drawing into G0, G1 | as row 10 | `0x60-0x7E` translated; **5/15 stays `_`**, the `ESC ( 0` row | partial |
| 12 | SGR: Normal, Bold, Blink, Underscore, Reverse Video | §1.6.2; SGR p. 5-103 (PDF 358) | the SGR rows | implemented |
| 13 | DECSWL | §1.6.2; p. 5-96 (PDF 351), Levels 1-4 | `ESC # 5` counted (`baud-core/src/write.rs`, the `ESC # 3/4/5` comment) | missing |
| 14 | DECDWL | §1.6.2; p. 5-97 (PDF 352) | `ESC # 6` counted | missing |
| 15 | DECDHLT | §1.6.2; p. 5-99 (PDF 354) | `ESC # 3` counted | missing |
| 16 | DECDHLB | §1.6.2; p. 5-99 (PDF 354) | `ESC # 4` counted | missing |
| 17 | DECSCLM | §1.6.2; p. 5-32 (PDF 287): "A conforming device shall provide the capability of selecting either jump scrolling ... and slow scrolling" | recorded and readable (`Term::smooth_scroll()`), but only tests read it, so no scroll slows. The page's Known Deviations: "Workstation terminal emulators which do not have scrolling hardware do not implement this control (DECterm, UIS)" | partial |
| 18 | DECSCNM | §1.6.2; p. 5-94 (PDF 349) | drawn; the `?5` row | implemented |
| 19 | DECOM | §1.6.2; p. 5-31 (PDF 286) | the `?6` row | implemented |
| 20 | LNM | §1.6.2; p. 5-57 (PDF 312); 070-6 §6.2.1 d | LF, and Return sends CR LF (`baud-domain/src/vt/key.rs`, `ret`) | implemented |
| 21 | KAM | §1.6.2; 070-6 §6.2.1 e; KAM (PDF 526), Levels 1, 2 | `CSI 2 h` counted in `unsupported_param` (`apply_ansi_mode`, `baud-core/src/ops.rs`) | missing |
| 22 | DECARM | §1.6.2; 070-6 (PDF 528), Levels 1, 2 | the `?8` row | implemented |
| 23 | DECCKM | §1.6.2; 070-6 (PDF 549) | the `?1` row | implemented |
| 24 | DECKPAM, DECKPNM | §1.6.2; 070-6 (PDF 553-554) | the `ESC =` / `ESC >` row | implemented |
| 25 | CUU | §1.6.2; p. 5-41 (PDF 296) | done | implemented |
| 26 | CUD | §1.6.2 | done | implemented |
| 27 | CUF | §1.6.2 | done | implemented |
| 28 | CUB | §1.6.2 | done | implemented |
| 29 | CUP | §1.6.2 | done, under DECOM from the margins | implemented |
| 30 | HVP | §1.6.2 | done, with CUP | implemented |
| 31 | CPR | §1.6.2 | `CSI 6 n`, from the origin under DECOM | implemented |
| 32 | DECSC | §1.6.2; §5.13.1 p. 5-186 (PDF 441) | see the note below this table | implemented |
| 33 | DECRC | §1.6.2; §5.13.1 | restores what row 32 saves | implemented |
| 34 | BEL | §1.6.2; p. 5-114 (PDF 369) | counted and rung | implemented |
| 35 | BS | §1.6.2 | done | implemented |
| 36 | HT | §1.6.2 | done | implemented |
| 37 | LF | §1.6.2 | done | implemented |
| 38 | VT | §1.6.2 | done, as an index | implemented |
| 39 | FF | §1.6.2 | done, as an index | implemented |
| 40 | CR | §1.6.2 | done | implemented |
| 41 | SUB | §1.6.2; SUB p. 5-132 (PDF 387), Levels 1-4: "This control is processed by placing a reversed question mark in the display at the Active Position" | cancels a sequence and writes nothing; the "SUB additionally displaying the DEC error character" row is `out` | partial |
| 42 | IND | §1.6.2 | done | implemented |
| 43 | RI | §1.6.2 | done | implemented |
| 44 | NEL | §1.6.2 | done | implemented |
| 45 | HTS | §1.6.2 | done | implemented |
| 46 | TBC | §1.6.2 | 0 and 3 | implemented |
| 47 | EL | §1.6.2 | 0, 1, 2 | implemented |
| 48 | ED | §1.6.2 | 0, 1, 2 | implemented |

**DECSC (row 32).** `docs/vt-coverage.md` marks it `partial` against
`[[source: vt510-decsc]]`'s seven items, GR and a pending single shift among
them. At Level 1 the list is shorter. §5.13.1: "The state which must be saved
in Level 1 operation includes the Current Rendition value ..., the current
value of Origin Mode, the character sets designated to G0, G1, G2, and G3,
and the character set most recently invoked into the left hand side (GL) of
the In Use Table". GR comes in only "In Level 2 operation". baud saves all of
that list, so the row is `implemented` at Level 1.

### 4.2 The other Level 1 fields, and Level 1 operation

| # | requirement | where | baud | status |
|---|---|---|---|---|
| 49 | SRM | p. 5-135 (PDF 390), Levels 1-4; **not in §1.6.2** | `CSI 12 h` counted in `unsupported_param` | missing |
| 50 | LS2 | 070-3 §3.6.1.1, p. 3-61 (PDF 166): "Conforming terminal products will provide the following locking shift functions in Level 1 operation", listing LS0-LS3. **Its own page, p. 3-75, says "Levels: 2, 3"** | `ESC n` counted; note AE Q9 | missing (disputed) |
| 51 | LS3 | as row 50; its page "Levels: 2, 3" | `ESC o` counted | missing (disputed) |
| 52 | Insert or Replace Graphic Character | p. 5-139 (PDF 394), Levels 1-4 | the writer | implemented |
| 53 | Level 1 defaults | 070-3 §3.6.4.1, p. 3-63 (PDF 168): "the default character set designations will be ASCII in G0, G1, G2, and G3", G0 in GL | `baud-core/src/term.rs`: `g: [Charset::Ascii; 4]`, `gl: GSlot::G0` | implemented |
| 54 | Level 1 is 7-bit | §3.6.4.1: "Level 1 operation is 7-bits only". §3.7.5.1, p. 3-67 (PDF 172): "The high order bit of all 8-bit characters received will be stripped by the terminal before any processing occurs. All C1 controls will be transmitted by the terminal as ESC Fe sequences" | baud reads UTF-8 by default and ISO 8859-1 under `ESC % @`, and S8C1T makes it send 8-bit C1. No operation strips the eighth bit. Whether UTF-8 is inside 070-3 at all is note AE's premise P | missing |
| 55 | Level 1 keys | 070-6 §6.2.1 a, p. 6-16 (PDF 503): "Shall implement all keys indicated in Level 1 Operation" | the main keys, cursor keys, numeric keypad and PF1-PF4 (`encode_key`) | implemented |
| 56 | No other keys | §6.2.1 b and c: "Shall neither implement additional keys nor make operative keys that are not part of Level 1 Operation"; "Shall make inoperative all keys that are part of Level 2 operation only". §6.2.4, p. 6-19 (PDF 506): E1-E6, F6-F10 and F14-F20 "inoperative"; F11 sends ESC, F12 BS and F13 LF | `encode_key` sends the editing keypad (`CSI 2 ~` for Insert, and so on) and F1-F20 in their tilde forms. No mode restricts them | missing |
| 57 | The four modes as Set-up features | §6.2.1 d: DECARM, DECCKM, DECKPAM/DECKPNM and LNM "for receipt from software application processes and as a local Set-up feature" | from the host, rows 20-24. `config.toml.example` has no key for any of the four | missing |
| 58 | Caps Lock | §6.2.1 f: "Caps/Shift Lock Mode as Caps Lock Operation; may optionally implement a local Set-up feature to change the mode" | whatever the platform's Caps Lock does; baud has no code for it. Not checked against "Caps Lock Operation" | unchecked |

**The tally**, by `python3 -I` over the status column of the two tables:

* 40 implemented: rows 1-4, 8-10, 12, 18-20, 22-40, 42-48, 52, 53 and 55;
* 3 partial: rows 11, 17 and 41;
* 14 missing: rows 5-7, 13-16, 21, 49-51, 54, 56 and 57, two of them
  disputed;
* 1 unchecked, row 58, not counted.

Of the 48 rows in §1.6.2 alone, 37 are implemented, 3 partial and 8 missing.

## 5. The extensions `?61;4c` would claim

`?61;4c` claims two extensions. By §1.5.3.1.2, each must be implemented in
full.

**Identification and Conformance, which DA itself belongs to.**

* 070-4 DA's field reads "Levels: 1x, 2, 3, 4" (PDF p. 221): at Level 1, DA
  is an extension, not a requirement.
* 070-1 §1.6.3.1, p. 1-20 (PDF 60) [img], names that extension's members:
  "DEVICE ATTRIBUTES (primary) DA(1) / DEVICE ATTRIBUTES (secondary) DA(2) /
  SELECT CONFORMANCE LEVEL DECSCL". It is "an extension to Level 1 that is
  required in all future products, and in Level 2".
* baud:
  - DA1 is implemented.
  - DA2 is `out`: `CSI > c` is answered with silence and counted (its row
    in `docs/vt-coverage.md`).
  - DECSCL has no arm: `baud-domain/src/vt/csi.rs` dispatches `! p` and
    `$ p` and no `" p`, and the probe counts `CSI 61 " p`.
  - **Partial.**
* **070-4 conflicts with this for DECSCL.**
  - Its DECSCL Note 1 (PDF p. 226): "The DECSCL control function is required
    only in Level 2. However, in Level 2 (and higher) conforming devices, the
    DECSCL control must also be executed when the device is in Level 1
    operation ... DECSCL is an Implicit Extension to Level 1."
  - Its implicit-extension table (PDF p. 224) lists DECSCL at operating level
    1 for a device that reports "2 or higher".
  - Read so, a device whose highest level is 1 needs no DECSCL. DA2's field
    is "Levels: 1X, 2, 3, 4" (PDF p. 228), and nothing in 070-4 removes it.
  - The 1983 text and the 1990 text disagree on DECSCL. Both are named here,
    and neither is preferred.

**Sixel, extension 4.**

* §1.6.3.6, p. 1-21: "As defined in the section 'Sixel Graphics Extension'".
  That is 070-9, whose §4 (PDF p. 914) has Sixel levels of its own: "Level 2
  devices support everything a Level 1 device supports, plus the Set Raster
  Attribute command, Background Select, Horizontal Grid Size and Macro
  Parameter commands", and "Color is an extension to either level 1 or level
  2 devices".
* baud draws Sixel with raster attributes and colour (the `DCS ... q` row).
* `docs/vt-coverage.md`'s `dcs_passthrough` row and note AN carry 070-9 §9.1
  departures that are open with the owner:
  - BEL and ENQ, which 070-9 gives the "same action as in text mode";
  - SUB, which 070-9 has "process as if a 3/15 code (blank Sixel) was
    received";
  - the other C0 controls.
* **Partial**, by those departures.

## 6. Beyond Level 1: rule 2 and rule 3

§1.5.3.1.1's rule 2 says a Level 1 device "may NOT implement any functions
defined by a higher level unless it implements all of the functions of that
level", and its example: "A conforming Level 1 implementation cannot implement
any Level 2 functions (or it must implement all of the Level 2 functions, in
which case it is a Level 2 conforming terminal)".

**baud implements, from `docs/vt-coverage.md`'s done rows:**

* Level 2 required functions, from §1.7.2, p. 1-23 (PDF 63) [img]:
  - S7C1T and S8C1T;
  - DECSTR ("Levels: 2, 3, 4", PDF p. 235);
  - DECTCEM ("Levels: 2-4");
  - ICH and ECH ("Levels: 2-4");
  - designation into G2 and G3, with DEC Supplemental.
* Level 2 extensions:
  - DECSCA, DECSEL and DECSED ("2x, 3x, 4x (Selective Erase)");
  - DECRQM and DECRQSS ("2x (TSI), 3-4");
  - colour through SGR (Table 1, 22, "Color Text", levels 2-4).
* Level 3 and Level 4 functions:
  - SU and SD ("3x, 4x (Windowing)");
  - DECSLRM, DECLRMM, DECIC and DECDC ("4x (Horizontal Scrolling)").

Rule 3 also forbids "any function not defined in any level" unless it is an
appendix exception, and baud implements xterm's extensions, OSC among them.

**A level device has to offer the lower levels.** 070-4 DA (PDF p. 222): "a
device that supports a higher conformance level must support the lower levels
as well (using DECSCL)". 070-6 §6.2.2 f (PDF p. 504) requires a Level 2
keyboard to implement "Select Conformance Level (DECSCL) to switch between
Level 1 Operation and Level 2 Operation".

**What this means for any level number.** The rules govern a DEC product's
claim to the architecture, so whether they bind a DA answer is a reading, and
it is not settled here. Read literally, baud conforms to no level:

* Level 1 forbids what baud has.
* Level 2 and above require a Level 1 operating mode that baud lacks.

The bare alias `?6c` makes no level claim. That is the "Known Deviations"
route in section 3.

## 7. The plain answer

**Does baud meet Level 1? No.** Exactly what is missing:

1. **Nine required functions, each counted and not acted on:**
   - SS2 and SS3 (§1.6.2; 070-3 pp. 3-80 and 3-81);
   - DECID in ANSI mode (§1.6.2; 070-4 PDF p. 232), built 2026-10-16;
   - DECSWL, DECDWL, DECDHLT and DECDHLB (§1.6.2; 070-5 pp. 5-96 to 5-99);
   - KAM (§1.6.2; 070-6 §6.2.1 e);
   - SRM (070-5 p. 5-135, `Levels: 1-4`, though not in §1.6.2).
2. **Two more if 070-3 §3.6.1.1 governs:** LS2 and LS3. Their own pages say
   Levels 2, 3. This is note AE's Q9, the owner's.
3. **Three partial functions:**
   - DEC Special Graphics at 5/15;
   - SUB's reversed question mark;
   - DECSCLM's slow scroll, which nothing acts on. STD 070 lists
     workstation emulators not implementing it as a known deviation.
4. **No Level 1 operation:**
   - no 7-bit mode that strips the eighth bit and sends C1 as `ESC Fe`
     (070-3 §3.6.4.1, §3.7.5.1);
   - no Level 1 key set, with E1-E6, F6-F10 and F14-F20 inoperative and
     F11-F13 sending ESC, BS and LF (070-6 §6.2.1 b and c, §6.2.4);
   - no Set-up for DECARM, DECCKM, DECKPAM and LNM (§6.2.1 d).
5. **For the `61` itself:** DA2 and DECSCL, the rest of the Identification
   and Conformance extension (§1.6.3.1). 070-4 makes DECSCL implicit only
   for devices reporting Level 2 or higher.
6. **For the `;4`:** the 070-9 §9.1 departures already open with the owner
   (section 5).
7. **And under §1.5.3.1.1's rule 2, read literally:** a Level 1 device must
   not implement Level 2, 3 or 4 functions, and baud implements many of them
   (section 6).

So `?61;4c` today would claim nine functions baud drops. That is the `parsed`
failure that "DA under-claims" exists to avoid.

## 8. What this leaves for Jev or the owner

None of these is decided here. Each goes to Jev at 0.9 on both asks, or to
the owner, with this note as its research.

* **(a) Keep `?6c` by default and leave the Sixel opt-in as it is.**
  - CONFORMING: the bare VT102 alias is in TP-002 Table 12-1 and is 070-4 DA's
    Known Deviations route.
  - Jev's 0.94/0.98 pick to advertise Sixel is not met.
  - The opt-in's `?6;4c` stays a shape no text defines.
* **(b) Build items 1-6 of section 7, then answer `?61;4c`.**
  - CONFORMING to 070-4 Table 1's shape, and to Level 1 except under the
    literal reading of rule 2 (item 7).
  - Item 4 needs a 7-bit Level 1 operation. Whether it applies to a UTF-8
    terminal is note AE's premise P.
* **(c) Claim `62` or `63` with `;4`, as DEC's own Sixel terminals did.**
  - Level 2 needs everything in (b) and also:
    - LS1R-LS3R (§1.7.2; note AE);
    - DECSCL with a Level 1 operation to switch to (070-4 DA PDF p. 222;
      070-6 §6.2.2 f);
    - the Level 2 keyboard's Set-up features (§6.2.2 g: keyclick, Caps/Shift
      Lock).
  - Level 3 is not enumerated here.
* **(d) Send `?6;4c`, the held commit's default.**
  - DEPARTS: no text read defines a VT100-family code with an extension, and
    ctlseqs says those parameters "do not mean anything by themselves".

**A premise in `docs/protocol-decisions.md` does not hold as stated, and this
commit does not change that file.** "DA under-claims" keeps baud off VT220
because "baud has no UDKs". UDKs are not a Level 2 requirement:

* §1.7.2's required list (PDF p. 63) [img] has no DECUDK.
* UDK is the independent extension §1.7.3.8 ("USER DEFINED KEYS DECUDK"), and
  Table 1's extension 8, levels 2-4.
* The DECUDK page reads "Levels: 2X".
* TP-002's VT220 alias reports it as extension `8`.

A Level 2 claim without `;8` is the honest shape for a terminal with no UDKs.
What keeps baud off Level 2 is the list in (c).

## 9. How it was checked

**The `Levels:` scan.**

* A regular expression, `^\s*[Ll1I][eo][vy][eo][l1I]s?\s*[:;.]`, tolerant of
  the OCR's `l`/`1`/`I` confusions, over the 2,320,788-byte text.
* It printed each field with the two nearest preceding lines as its title:
  140 fields, plus 4 prose lines that begin with "level.".
* A line's PDF page is the count of form feeds before it, plus one.
* Positive control: the fields of three functions already known to be in the
  document came out on their pages. SGR's is on the page headed 5-103 and
  DECSC's on 5-187; DSR's is on PDF p. 239, where 070-4 has no page header.

**The probe.** It was run with `cargo test -p baud-core` against a
`git archive` of `bf3b6762` in a scratch directory, and is not committed. Each
case feeds `A` then the bytes, and prints the change in `Term::unsupported`,
`unsupported_param` and `charset_unsupported`, and the reply. The controls
came first: CUP changed nothing, DA replied `ESC [ ? 6 c`, and DSR 5 replied
`ESC [ 0 n`.

```text
control: CUP (done)          unsupported +0 param +0 charset +0 reply ""
control: DA                  unsupported +0 param +0 charset +0 reply "\u{1b}[?6c"
control: DSR 5               unsupported +0 param +0 charset +0 reply "\u{1b}[0n"
SS2 ESC N                    unsupported +1 param +0 charset +0 reply ""
SS3 ESC O                    unsupported +1 param +0 charset +0 reply ""
DECID ESC Z                  unsupported +1 param +0 charset +0 reply ""
DECDHLT ESC # 3              unsupported +1 param +0 charset +0 reply ""
DECDHLB ESC # 4              unsupported +1 param +0 charset +0 reply ""
DECSWL ESC # 5               unsupported +1 param +0 charset +0 reply ""
DECDWL ESC # 6               unsupported +1 param +0 charset +0 reply ""
KAM CSI 2 h                  unsupported +0 param +1 charset +0 reply ""
SRM CSI 12 h                 unsupported +0 param +1 charset +0 reply ""
LS2 ESC n                    unsupported +1 param +0 charset +0 reply ""
LS3 ESC o                    unsupported +1 param +0 charset +0 reply ""
DA2 CSI > c                  unsupported +1 param +0 charset +0 reply ""
DECSCL CSI 61 " p            unsupported +1 param +0 charset +0 reply ""
SCS G0 line drawing          unsupported +0 param +0 charset +0 reply ""
SUB                          unsupported +0 param +0 charset +0 reply ""
```

**Not read:**

* TP-001, which has no text layer.
* The rest of TP-002 beyond its DA, DECSCL and report pages.
* 070-1's "Product Reference" appendix, which §1.6.1 says lists the
  conforming Level 1 products.
* Level 3's required list.
