# AN. DCS bodies and Sixel: what the texts say per byte

**Status: research, 2026-10-10. Item 8, the pixel aspect ratio where both
DEC texts make it whole, is built since 2026-10-11 (the record, "A Sixel pixel
is drawn at its aspect ratio where both DEC texts agree"), and so is part of
item 6, an HLS value past its range ignoring the whole command (the record, "A
Sixel HLS register is converted, with DEC's hue origin"); nothing else here is
built.** Part 1 is what every text says a terminal does with each byte of a
DCS command string, and what baud does since `e6a817f`. Part 2 reads DEC STD
070's Sixel chapter, 070-9, byte by byte against the other Sixel texts the
manifest pins, and against baud's Sixel handler. Where the texts agree and
baud departs, the item is listed as work (section 10.1). Where one option
conforms to every text, that is work too, and the other options are kept as
alternatives considered (sections 5 and 8). Where no option conforms to every
text, it is a question with options, each option marked against each text
(section 14). Every choice between options here goes to Jev first, by the
owner's rule of 2026-10-10: Jev at or above 0.9 confidence both times
decides, a departure from a standard included, and otherwise the owner does,
from the review page. So items 10, 11 and 12 are work, for Jev, and questions
1 and 2 go to Jev before the owner. None has been put to Jev yet.

**Base.** Line numbers in baud's code are at `e6a817f`, which built the two
classes the texts agree on in a DCS body: the format effectors `08-0D` are put
to the handler and `DEL` is ignored (the record, "A DCS body puts the format
effectors and ignores DEL, where every text agrees"). smooth-scroll's HEAD is
now `5ad5913`; no file this note cites a line of changed between the two
(`git diff --stat e6a817f 5ad5913` over those files), and the harness below
gives the same 55 lines at `e6a817f` and at `2543f8b`. Every line number and
every answer of baud's in Part 1 was derived at `e6a817f`.

**Evidence.** The harness is `docs/research/notes/an/an-controls.rs`, run in a
git-archive copy of baud at `e6a817f` as a `baud-core` test. Its output is
`docs/research/notes/an/an-controls-output.txt`, and its lines are cited D1-D4
(a DECRQSS) and S1-S20 (a Sixel). The two scripts that searched and printed
DEC STD 070's text are `docs/research/notes/an/std070-find.py` and
`docs/research/notes/an/std070-page.py`, and
`docs/research/notes/an/change-bars.py` finds the change bars on its page
images. `docs/research/notes/an/sixel-scan.py` counts the Sixel shapes in the
tree's recordings; run at `2543f8b` over its 27 `.raw` files and its one
`.six`, it finds two Sixels: `swirl.raw` and
`tests/recordings/sixel/img2sixel-96x72.six`, which
`baud-domain/tests/sixel_oracle.rs` decodes. Its output, with a positive
control that fires every count, is
`docs/research/notes/an/sixel-scan-output.txt`. Sections 5.3 and 5.4's test
costs come from the arms in `docs/research/notes/an/q1-arms/`, each applied to a
git-archive copy at `2543f8b` and run under `cargo nextest run --workspace`.

**Tags.** **M**: measured by that harness, through `Term` and its real graph,
`str_put` and decoder. **C**: read from the code. **R**: this note's reading of
a text, not the text. **U**: unknown, or resting on a source that is not
pinned. A text is marked *conforms* or *departs*; xterm is practice, and is
described, never used as a mark.

**How the sources were read.**

* Every repository source was read from the tree at `e6a817f`, and each hashes
  to its manifest entry: [[source: ecma48]], [[source: williams]],
  [[source: ctlseqs]], [[source: xterm-reset]] (charproc.c),
  [[source: xterm-vtprstbl-c]], [[source: xterm-misc]],
  [[source: xterm-ptyx-h]], [[source: xterm-graphics-sixel]],
  [[source: vt3xx-sixel]] and [[source: vt320-uu-appe]].
* The DEC PDFs are `external`, and the local copies hash to the manifest:
  [[source: dec-std-070]] `d30c2ace...`, [[source: vt510-rm-b]] and
  [[source: vt520-rm]]. They were extracted with `pdftotext -layout` from
  poppler-utils 26.09.
* **STD 070's text layer is OCR**: `pdfinfo` gives the PDF's producer as
  Adobe Acrobat 9.42 Paper Capture Plug-in. Its quotes are as extracted, with
  [sic] at each misread,
  and the page image's reading follows in square brackets where a misread
  changes a code or a word. Every 070-9 quote below was read against the page
  image, rendered with `pdftoppm` from the same poppler.
* **The positive control came first.** On the alphanumeric-stripped text,
  `DECRQSS` hits 5 pages (PDF 446, 447, 451, 482, 1045) and "Device Control
  String" 38. Then "Coding Issues" hits PDF 26, 910 and 925; "blank Sixel",
  "same action as in text mode" and "8th bit is set to zero" each hit PDF 926
  alone.
* **070-9 is PDF pp. 908-935**: the title page is 908, printed page 1 is 912,
  and printed page *n* is PDF 911 + *n*. It is "DEC STD 070-9 Video and
  PrInter[sic] Systems Reference Manual - Sixel Graphics extension[sic]"
  [image: "Printer", "Extension"], "Rev A1, 03-Aug-1990" (PDF 909). Every
  page of 070-9 was read, and every page this note quotes from it was
  rendered and read as an image too.
* **The two chapters' revisions, as their pages give them.** 070-9's
  revision history is "Rev A, 24-Nov-1987" and "Rev A1, 03-Aug-1990" (PDF
  909). 070-3's, the chapter Part 1 quotes, runs from "Original Draft"
  05-Jun-1982 [OCR "OS-Jun-1982"] through revisions 0.1 to 0.3, AX04, AX10,
  AX11 and AX12 to "Revision A" 14-Apr-1989 (PDF 107; the identifier, "Rev A,
  14-Apr-1989", is on PDF 106 and 107). 070-3's change history, under
  "REVISION 0.3 to AxlO[sic]" [image: "AX10"] (PDF 189), AX10 being
  15-May-1983, has "Changed the description of control characters occurring
  within control strings to permit, but deprecate the use of, any function
  other than String Terminator to terminate a string" (item 8, PDF 190),
  which names no control.
* **What the change bars date.** 070-3's AX11 "Removed Rev AX10 change bars"
  and its AX12 "Removed Rev AXll[sic] change bars", each adding bars "to any
  change that could affect conformance or interpretation" (item 1 of each, PDF
  190 and 191). On PDF 107 the "Rev A" identifier and the "Revision A" history
  line carry bars, so Rev A followed the same convention, and the bars on
  3.5.4.4's lines from "characters 0/8 through 0/13 is valid" to "be ignored
  by hardware" (PDF 135) date that wording, the sentence on `SUB` among it, to
  Rev A, 14-Apr-1989: after 070-9's Rev A (24-Nov-1987) and before its A1
  (03-Aug-1990). No page of 070-9, PDF 912 to 933, carries a change bar,
  though its change history (PDF 931-933, AX11 to AX12) lists changes, so its
  pages do not date its sentence on `SUB` to Rev A or to A1. Read from the
  page images, and by `docs/research/notes/an/change-bars.py`, whose control
  fires on 070-3's barred pages (PDF 134, 135, 191) and which finds none on
  PDF 908-933. So the pages show neither sentence to be the later one, and
  this note draws no order from the dates.
* ECMA-48 was read with `scripts/extract-ecma48.py`, whose controls hit. A
  "silent" below means a search with a positive control run beside it, given
  where it is used.

## 0. In plain words

- **A DCS body.** Since `e6a817f` baud does what every text allows for the six
  format effectors and for `DEL`. The other C0 controls in a body, `BEL`
  among them, looked like a conflict and are not one: williams' parser row
  passes every one to the handler; DEC STD 070 recommends ignoring the ones a
  string does not use, and the VT510 ignores `NUL`; ECMA-48 leaves error
  recovery to the implementation. A parser that passes them all and handlers
  that ignore the ones they give no meaning to satisfy every one of these,
  and in a Sixel the handler rings a `BEL`, as DEC's Sixel standard says. It
  is work, for Jev (section 5), and the options that keep today's split or
  ignore them all at the parser are kept as alternatives considered. A GR
  byte in a DECRQSS is work, for Jev, too: under ISO 8859-1 it should be read
  as its 7-bit equivalent, where baud stores it raw.
- **A Sixel body.** DEC's own Sixel standard says more about the data stream
  than the VT330/VT340 manual the decoder was built from. On most bytes the
  texts agree, and baud departs on eight things:
  - a `BEL` should ring;
  - a `SPACE` should be ignored, but inside a number it splits it (`!1 0~`
    draws one column where `!10~` draws ten, **M**);
  - a GR byte should be read as its 7-bit equivalent under ISO 8859-1, but it
    draws nothing;
  - an unrecognised command should cancel a pending repeat, and does not;
  - an invalid colour command should be ignored whole, but baud selects its
    register and clamps its values;
  - a colour the image never defined should draw in a default colour that
    contrasts with the background, but it draws transparent, so a Sixel with
    no colour command draws nothing (**M**);
  - the pixel aspect ratio is 2:1 unless the image says otherwise, and baud
    always draws 1:1;
  - a Sixel cut off by `ESC` or a C1 should keep what it drew, and baud draws
    none of it.
- **Two Sixel conflicts, questions for Jev and then the owner.** DEC STD 070's
  chapter 3 ends every control string on `SUB`, as williams and the VT510 do;
  its Sixel chapter says a `SUB` inside a Sixel is a blank sixel and the image
  goes on. baud ends the string, and the rest of the image is then drawn as
  text (**M**, S14). For a Sixel cut off by `CAN`, williams keeps what was
  drawn while ECMA-48 says the data preceding a `CAN` "shall be ignored" and
  fixes no scope for "preceding"; baud keeps nothing.
- **A pixel aspect ratio that is not a whole number is work, for Jev, not a
  question.** DEC's Sixel standard gives 4.5:1, 2.5:1, 1.5:1, 1.3:1 and
  1.12:1, says the ratio "should be preserved if possible", and measures a
  device's grid as an average over up to 30 pixels, which rows drawn 4 and 5
  device pixels tall in turn meet exactly for 4.5:1. The VT330/VT340 manual
  rounds the ratio to a whole one, and that is its own terminal's fallback,
  which the standard leaves each device to document; baud is not that
  terminal.

---

# Part 1. A DCS command string

## 1. What baud does at `e6a817f`, class by class

The path, read from the code (**C**) and run (**M**, D1-D4):

* **The prefix.** `Node::Dcs` (`baud-core/src/graph.rs` 865-903) and
  `Node::DcsDollar` (904-911). C0 other than `18`, `1A` and `1B`, and `7F`,
  are ignored there (900, 908). `30-39` and `3A-3B` collect as parameters
  (897-898). `$` leads to `DcsDollar` (867), and `$ q` to a DECRQSS (906); a
  bare `q` is a Sixel (871). Every other byte, `A0-FF` included under ISO
  8859-1, goes to the body (901, 909).
* **The body, `Node::DcsStr`,** takes `string()` (1169-1280):
  * `07` is `StrPut` (1195-1199).
  * `1B` goes to `StrEsc` (1200). There `\` ends the string and dispatches it;
    any other byte cancels the string and is dispatched again from `Escape`
    (917-922).
  * `08-0D` are `StrPut` (1225-1228), since `e6a817f`.
  * `00-06`, `0E-17`, `19` and `1C-1F` are `Consume`, dropped (1252).
  * `7F` is `Consume` in a DCS body (1274-1277), since `e6a817f`.
  * Everything else is `StrPut` (1278): `20-7E`, `A0-FF`, and `80-9F` under
    UTF-8.
* **From any node**, `CAN` and `SUB` go to ground (700-702). Under ISO 8859-1
  a C1 cancels the string and is executed, and `9C` ends it (750-761;
  `latin1_c1` at 1016-1024).
* **The handlers**, in `Paint::str_put` (`baud-core/src/osc.rs` 49-110):
  * a Sixel drops every C0 it is put (75-77) and stores the rest in its own
    buffer (78-83);
  * a DECRQSS drops a format effector (100-102,
    `StreamByte::is_format_effector`, exactly `08-0D`) and keeps every other
    byte, up to `OSC_MAX` (103-109);
  * every other DCS is stored, and discarded at the terminator as
    `StrKind::Other` (osc.rs 137-140).
* **At ST** a DECRQSS goes through `StatusTarget::parse` (osc.rs 128-132),
  an exact match on `m`, `r`, `s`, ` q` and `"q`
  (`baud-domain/src/vt/dcs.rs` 51-60). It answers `0$r` for anything else
  (`baud-core/src/dcs.rs` 150) and `1$r` and the setting for a match (151 on).
  `1` means valid.

| class | what baud does | measured |
|---|---|---|
| `08-0D` | put; DECRQSS and Sixel drop it | D2: `m BS`, `m CR` answer `1$r0m` |
| `07` BEL | put; DECRQSS keeps it in the `Pt`, Sixel drops it silently | D2: `m BEL` answers `0$r`; S12: no bell |
| other C0 | ignored by the graph | D2: `m NUL`, `m SOH`, `m ENQ`, `m SO`, `m US` answer `1$r0m` |
| `18` CAN, `1A` SUB | cancel the string; no handler is told; `SUB` draws no error character | S14, S15 |
| `1B` ESC | `ESC \` ends; any other byte cancels, then runs | S16 |
| `20-7E` | content | D1 |
| `7F` DEL | ignored | D2: `m DEL` answers `1$r0m` |
| `80-9F`, ISO 8859-1 | a C1 cancels and runs; `9C` ends | S17 |
| `A0-FF` in the body, ISO 8859-1 | stored raw | D3: `ED` (`m` with the eighth bit) answers `0$r` |
| `A0-FF` in the prefix, ISO 8859-1 | sent to the body | D4: `DCS A4 F1 m ST` gets no reply |

## 2. What each text says

Citation keys: W is [[source: williams]] by line; C is [[source: ctlseqs]] by
line; X is xterm 411 ([[source: xterm-reset]]'s charproc.c,
[[source: xterm-vtprstbl-c]], [[source: xterm-misc]],
[[source: xterm-ptyx-h]]); DEC sources by printed page, then PDF page.

### 2.1 ECMA-48, 5th edition ([[source: ecma48]])

- **E1**, §5.6, p. 13: "A command string is a sequence of bit combinations in
  the range 00/08 to 00/13 and 02/00 to 07/14. A character string is a
  sequence of any bit combination, except those representing START OF STRING
  (SOS) or STRING TERMINATOR (ST). The interpretation of the command string or
  the character string is not defined by this Standard, but instead requires
  prior agreement between the sender and the recipient of the data."
- **E2**, §8.3.27, p. 38: "The command string following may consist of bit
  combinations in the range 00/08 to 00/13 and 02/00 to 07/14. The control
  string is closed by the terminating delimiter STRING TERMINATOR (ST)." Its
  purpose and format "are specified by the most recent occurrence of IDENTIFY
  DEVICE CONTROL STRING (IDCS), if any, or depend on the sending and/or the
  receiving device."
- **E3**, §8.3.143, p. 70: ST's "Representation: 09/12 or ESC 05/12"; it "is
  used as the closing delimiter of a control string opened by ... DEVICE
  CONTROL STRING (DCS)".
- **E4**, §9, pp. 74-75, which extends the formal rules "in order to make
  allowance for such unintended but unavoidable deviations" as a 7-bit to
  8-bit transformation produces: "In an 8-bit code, the bit combinations of
  columns 10 to 15 are permitted to represent ... the contents of the command
  string ... In these situations, the bit combinations in the range 10/00 to
  15/14 shall have the same meanings as the corresponding bit combinations in
  the range 02/00 to 07/14." And in a 7-bit code SO and SI "are permitted to
  occur ... between the opening delimiter of a control string and the STRING
  TERMINATOR (ST) ... SO and SI have no effect on the interpretation of ... a
  control string ..., but they may, indeed, affect the meanings of bit
  combinations following in the data stream."
- **E5**, Annex D, p. 83, among the matters left to implementors: "10) What
  action will be taken in error recovery."
- **E6**, §8.3.6, p. 34: "CAN is used to indicate that the data preceding it
  in the data stream is in error. As a result, this data shall be ignored. The
  specific meaning of this control function shall be defined for each
  application and/or between sender and recipient." §8.3.148, p. 71: "SUB is
  used in the place of a character that has been found to be invalid or in
  error."
- **E7**, Annex F.8.1, p. 89: "DEL characters may be inserted into, or
  removed from, a CC-data-element without affecting its information content,
  but such action may affect the information layout and/or the control of
  equipment."
- **E8**, §5.3, pp. 8-9: C1 is represented "in an 8-bit code by bit
  combinations from 08/00 to 09/15"; DCS is 09/00 and ST 09/12.

So in ECMA-48, `08-0D` and `20-7E` are the command string, with their meaning
by agreement. ECMA-48 says nothing of any other byte arriving inside one: on
its de-spaced text "error" occurs three times, in §8.3.6 (CAN), §8.3.148
(SUB) and Annex D's item 10 (E5), with `extract-ecma48.py`'s controls hitting
and a nonsense needle missing. It is X3.64, as williams quotes it (W4), that
calls such a byte an error condition "whose recovery is not specified", and
ctlseqs reads ECMA-48 the same way (C4); reading ECMA-48's silence as making
the byte an error is this note's (**R**), and ECMA-48 leaves "what action will
be taken in error recovery" to the implementation (E5). ECMA-48 never says
that CAN, SUB, ESC or a C1 ends a string.

### 2.2 williams ([[source: williams]])

- **W1**, 388, `dcs passthrough`: "entry / hook | event
  00-17,19,1C-1F,20-7E / put | event 7F / ignore | exit / unhook". 391: `9C`
  goes to ground.
- **W2**, 726, put: "C0 controls are also passed to the handler." 728-729,
  unhook: "When a device control string is terminated by ST, CAN, SUB or ESC,
  this action calls the previously selected handler function with an 'end of
  data' parameter. This allows the handler to finish neatly." (Quotes inside
  a quotation are written single here; williams' are curly double.)
- **W3**, 818-822, DEC's practice: "In the first section, C0 controls are
  ignored. In the second section, C0 controls may or may not have a meaning to
  the device function. If they have no meaning for the device function (e.g.
  when defining soft characters with DECDLD) they will be silently ignored. If
  they have a meaning for the device function (e.g. when in ReGIS mode), they
  will be acted upon."
- **W4**, 803-811, quoting X3.64 §3.2.2: "These may be characters from
  columns 2 to 7, excluding 7/15 (2/0 to 7/14) [...] The occurrence of other
  control characters and/or characters from columns 10 to 15 within such a
  string are error conditions whose recovery is not specified by this
  standard."
- **W5**, 268, anywhere: "18,1A / execute | 80-8F,91-97,99,9A / execute | 9C
  / (no action)". 608-610: "On the VT220, VT420 and VT500, the C0 controls CAN
  and SUB cancel any escape sequence, control sequence or control string in
  progress and return to ground state. SUB will also display the error
  character". 612-613: "All C1 controls cancel any escape sequence, control
  sequence or control string in progress and are executed."
- **W6**, 497-498: ESC "will immediately cancel any escape sequence, control
  sequence or control string in progress". 501-503: "the ESC that cancels a
  control string may occur after the control function has been determined and
  the following string has had some effect on terminal state. For example,
  some soft characters may already have been defined. Cancelling a control
  string does not undo these effects." 506-507: the ESC "will 'cancel' the
  control string, so the ST control function that is invoked by the arrival
  of the following '\' is essentially a 'no-op' function."
- **W7**, 462: "There are no explicit actions shown for incoming codes in the
  GR area (A0-FF). In all states, these codes are treated identically to GL
  codes 20-7F."
- **W8**, the prefix: 354, 361 and 368 ignore `00-17,19,1C-1F` and `7F`; 565,
  "C0 controls other than CAN, SUB and ESC are not executed while recognising
  the first part of a device control string"; 593, "The data string that
  follows is not checked by this parser."

### 2.3 ctlseqs ([[source: ctlseqs]])

- **C1**, 45: "Pt A text parameter composed of printable characters." It
  is under "Definitions" (29), which opens: "Many controls use parameters,
  shown in italics" (31): the notation of the sequences the document lists.
- **C2**, 93-94: string mode "is ended on a ST".
- **C3**, 96-113 quote STD 070 §3.5.4.5 on GR; 128-130: "a standard-compliant
  implementation of ECMA-48 ignores the eighth bit of bytes in control strings
  other than the C1 controls. XTerm does this."
- **C4**, 132-137: "ECMA-48 describes only correct behavior, telling what
  types of characters are expected at each stage of the control sequences. It
  says that the action taken in error recovery is implementation-dependent.
  ... It handles errors in decoding i.e., unexpected characters, by resetting
  to the initial (ground) state."
- **C5**, 149-153, among the workarounds for an application that "does not
  send the string terminator" (140-141): "The terminal should accept
  single-byte controls within the string. But some applications omit a string
  terminator, like the Linux console. The brokenStringTerm resource setting
  tells xterm to exit string mode if it decodes a common control character
  such as carriage return before the string terminator." It is about a
  missing ST, and this note does not rest anything on it.
- **C6**, 2023-2024: "XTerm accepts either BEL or ST for terminating OSC
  sequences", for an OSC only.
- **C7**, 499-501 and 3459-3466: DECRQSS answers `DCS 1 $ r` for valid and
  `DCS 0 $ r` for invalid, and the VT420 and VT525 manuals have it reversed.
- **C8**, 478-480, DECRQSS: "DCS $ q Pt ST ... The string following the "q"
  is one of the following:", `m`, `SP q`, `" q`, `r`, `s` and the rest. It
  says nothing of a C0 or a GR byte inside the string.

### 2.4 DEC STD 070, chapters 3 and 5 ([[source: dec-std-070]])

- **D1**, §3.5.4, p. 3-28 (PDF 133): "the command string must be made up of
  characters in the range 0/8 to 0/13 inclusive'and[sic] 2/0 to 7/14
  inclusive. The inclusion of co[sic] Controls 0/8 to 0/13 in control string
  data is intended for convenience in formating and storing control strings.
  It is recommended that these characters not affect the interpretation of the
  control string." [image: "inclusive and", "C0"; the extraction's mark
  before "and" is a double quote, written here as a single one]
- **D2**, §3.5.4.1, pp. 3-28/29 (PDF 133-134): the form "DCS P .. P I .. I F
  0[sic] .. 0[sic] ST", where "0[sic] .. 0[sic] is the command string, whose
  format is determined by the combination of parameters, intermediate
  characters, and final character that precede it." [image: "D..D" both
  times]
- **D3**, §3.5.4.4, p. 3-30 (PDF 135): "Control strings will normally
  terminate upon receipt of a string Terminator (9/12) chatacter[sic]. The
  receipt of the control characters. 0/8 through 0/13 is valid within control
  strings. For backward compatibility, and to minimize the effect of a lost
  String Terminator, the control codes CAN (1/8), SUB (1/10), ESC (1/11), or
  any C1 control will also terminate control strings. Conforming software
  should not depend on this practice however, and shall only use the String
  Terminator control function to terminate a'[sic] control string. Control
  characters other than those recognized within the command string syntax may
  be defined to have other meanings .in[sic] the future. It is recommended
  that all other control codes not used within a command string be ignored by
  hardware." And §3.3.4.1, p. 3-16 (PDF 121): "DEL (delete, 7/15) is also a
  control character, although it is not a member of the CO or C1 sets."
- **D4**, §3.5.4.5, p. 3-30 (PDF 135): "GR (8-bit) graphic characters are
  permitted within Device Control Strings, and the graphic character's
  interpretation will be dependent on the internal control string format. When
  they occur in the introducer sequence to a Device Control String, the eighth
  bit will be ignored, and they will be treated as their 7-bit equivalent."
- **D5**, §3.5.4.6, p. 3-30: for an unimplemented string, "all data (control
  and,graphic[sic] characters) from the control string introducer to the
  string terminator (inclusive) will be discarded without being displayed."
- **D6**, §3.4.2, p. 3-17 (PDF 122): "When received by a terminal, the Delete
  character is treated as an unimplemented control character and shall be
  ignored", under §3.4's opening condition, "When 94-character graphic
  character sets are used".
- **D7**, §3.5.1.2.5(c), p. 3-20 (PDF 125): "If the String Terminator is
  received after the Final Character, it causes normal termination of the
  control string".
- **D8**, §3.5.5, pp. 3-39/40 (PDF 144-145), the reference parser: "All other
  CO[sic] controls are immediately recognized within escape, control, ·or[sic]
  control string introducer sequences. The escape, control, or control string
  introducer sequence continues where it left off". The same summary: "Within
  escape, control, and control string introducer sequences, eight-bit graphic
  codes (GR set) are truncated to seven~bit[sic] codes" [image:
  "seven-bit"].
- **D9**, DECRPSS, p. 5-197 (PDF 452): "o[sic] UNKNOWN selection or setting,
  request unsuccessful" [image: "0"] and "1 request successful, data contains
  selection information"; UNKNOWN "is always coded as follows: DCS 0 $ r ST".
  The page, as every page of chapter 5 quoted here, is headed "For Review
  Only 29-Jun-1990".
- **D10**, §3.5.5, p. 3-40 (PDF 145), the reference parser again: "The parser
  does not keep track of whether a control string is in progress. Therefore it
  is possible to receive control codes, escape sequences, and control
  sequences within a control string~[sic] Handling of control strings must be
  performed at a higher level than the parser." [image: "string."]
- **D11**, DECRQSS, p. 5-196 (PDF 451), "For Review Only 29-Jun-1990": format
  "DCS $ q D...D ST", and "The data of the device control string for DECRQSS
  contains the identifying intermediate and final characters of the control
  sequence used to make the selection." It says nothing of a C0 or a GR byte
  in the data.

### 2.5 VT510 and VT520 ([[source: vt510-rm-b]], [[source: vt520-rm]])

- **V1**, Table 4-1 (VT510 pp. 4-1/2, PDF 87-88). NUL: "NUL has no function
  (ignored by the terminal)." CAN: "Immediately cancels an escape sequence,
  control sequence, or device control string in progress. In this case, the
  VT510 does not display any error character." SUB: "Immediately cancels an
  escape sequence, control sequence, or device control string in progress, and
  displays a reverse question mark as an error character." ESC: "ESC also
  cancels any escape sequence, control sequence, or device control string in
  progress." DEL: "Ignored when received, unless a 96-character set is mapped
  into GL."
- **V5**, DECDMAC (VT510 pp. 5-46/47, PDF 170-171): "You can include
  characters from positions 0/8 through 0/13 to format the device control
  string, but these characters cannot be part of the macro definition."
- **V6**, DECRPSS (VT510 pp. 5-99/100, PDF 223-224): "0 Host's request is
  valid. 1 Host's request is invalid", the reversed coding C7 records.

### 2.6 xterm 411, practice

- **X1**: in a string every byte is appended as `AsciiOf(c)`, which is
  `0x7f & (n)` (X ptyx.h 330; charproc.c 3454-3456), before the case switch,
  so `CASE_IGNORE` means stored and nothing else.
- **X2**, `sos_table` (VTPrsTbl.c 7619 on): `00-06` and the rest of C0 but
  `07`, `18`, `1A` and `1B` are `CASE_IGNORE`, `ENQ` among them (7628); `07`
  is `CASE_BELL` (7630); `18` `CASE_CAN`, `1A` `CASE_SUB`, `1B` `CASE_ESC`
  (7652-7655); `7F` is `CASE_IGNORE` (7780); a C1 goes to its own case or to
  ground (7781-7820), `9C` to `CASE_ST` (7817); `A0-FF` are `CASE_IGNORE`.
- **X3**: `CASE_BELL` ends an OSC and otherwise calls `Bell` and leaves the
  string open (charproc.c 3691-3705); `CASE_CAN` and `CASE_SUB` reset the
  parser, `SUB` drawing an error character (3618-3678).
- **X4**: `do_dcs` returns without a reply if the string holds a NUL
  (misc.c 4891-4893), and DECRQSS matches with `strcmp` (4935), so any C0 in
  the `Pt` answers invalid.

## 3. Where the texts agree, and where they conflict

**They agree** (texts only; xterm is practice and listed after):

- `20-7E` is content (E1, E2, D1, W1, V5).
- `9C`, or `ESC \`, is the one conforming terminator (E3, D3, D7, V1, W1).
- A C1 other than `9C` ends the string and then acts (D3, W5).
- CAN and SUB end the string (D3, V1, W5); ECMA's CAN (E6) is
  consistent.
- **The format effectors `08-0D` are allowed in the body and need not change
  its meaning** (E1, E2, D1, D3, V5, W1, W3). Built at `e6a817f`.
- **`DEL` is dropped or may be** (W1, D3 with §3.3.4.1, D6, V1, E7). Built at
  `e6a817f`.
- **A GR byte in the introducer is its 7-bit equivalent** (D4, W7, E4, C3).
  Not built (section 5.5).
- **The other C0 in the body, `BEL` included, are put to the handler, and
  each handler ignores the ones it gives no meaning to and acts on the ones it
  does** (W1, W2, W3; D3's recommendation to ignore them, met by the handler;
  D10, which leaves a control string's handling "at a higher level than the
  parser"; V1 for NUL; 070-9 §9.1 for a Sixel's `BEL` and `ENQ`). ECMA-48 and
  ctlseqs leave the recovery to the implementation (E5, C4), and ctlseqs'
  `Pt` (C1) is read in section 5.2. Not built (section 5): the graph ignores
  22 of them and puts `BEL`, and DECRQSS keeps a `BEL` in its `Pt`.
- **A GR byte in a DECRQSS body is its 7-bit equivalent, under ISO 8859-1**
  (E4, W7, C3). D4 leaves a DCS body's GR to the string's format; DECRQSS's
  (D11) is "the identifying intermediate and final characters of the control
  sequence", and in a control sequence STD 070 truncates a GR code to seven
  bits (D8's summary, and D4's own "(Note that this is the same way 8-bit
  graphic characters are handled within control sequences.)", quoted at C3)
  (**R**). No DECRQSS text speaks of GR: not D9, D11, C7 or C8, nor the
  VT510's DECRPSS and DECRQSS pages (V6; pp. 5-109/110, PDF 233-234). Not
  built (section 5.5).

xterm, as practice: it ends the string on a C1, CAN and SUB (X2, X3), keeps
`AsciiOf` of every byte, so a GR byte is its 7-bit equivalent (X1), and stores
`DEL` and every C0 but the terminators (X2).

**They conflict:**

| question | one side | the other side |
|---|---|---|
| SO and SI in the body: whether they shift what follows the string | put: W1; ignored: D3 (a recommendation) | no effect on the string, "but they may ... affect the meanings of bit combinations following": E4 |
| a C0 in the prefix | ignored: W8 | executed, and the introducer "continues where it left off": D8 |
| `SUB` inside a Sixel | ends the string: D3, V1, W5 | a blank sixel: 070-9 §9.1 (section 7.3) |
| the sixels a Sixel that `CAN` cuts off has drawn | kept: williams 501-503 and 728-729, 070-9 (**R**) | ignored: ECMA-48 §8.3.6, under its wider reading (**R**, section 7.4) |

The one place two texts give opposite answers to the same request is the
DECRPSS digit: D9 codes UNKNOWN "as follows: DCS 0 $ r ST", on a page headed
"For Review Only 29-Jun-1990", and V6's table has `0` for valid, the reversal
C7 records as the manuals' error. baud follows D9 and C7.

## 4. DECRQSS: what a `Pt` holding the byte means

The request is `DCS $ q m <b> ST`. Each line gives the answer the text implies,
and which digit that text uses for "valid". baud's answers are **M** (D2, D3).

ctlseqs is read the same way for every byte (section 5.2): its `Pt` is
"composed of printable characters" (C1), a definition in its notation of what
a request holds, and it leaves what a terminal does with any other byte to
error recovery, "implementation-dependent" (C4), so a handler that ignores the
byte before it reads the `Pt` reads a `Pt` of printable characters (**R**).
The other reading, that C1 makes a `Pt` received with such a byte invalid, is
not C1's text, and it would make the `e6a817f` answers to `m CR` and `m DEL`
departures from ctlseqs, where the record of `e6a817f` lists ctlseqs among the
texts that let the byte be dropped.

**`b` = CR, LF or HT.**
- ECMA-48: in range (E1, E2), meaning by agreement; it has no DECRQSS.
- STD 070 (1 is valid): "not affect the interpretation" (D1), so valid.
- VT510/VT520 (0 is valid): silent for DECRQSS; DECDMAC treats them as
  formatting (V5), so valid by analogy (**R**).
- williams: put (W1); applying W3, valid. It gives no digit.
- ctlseqs (1 is valid): the byte dropped, `m` is the `Pt` and valid (**R**,
  above).
- xterm: stored, `strcmp` fails, `DCS 0 $ r ST` (X4).
- **baud: `DCS 1 $ r 0 m ST`.** The handler drops the byte, since `e6a817f`;
  the graph dropped it before, with the same answer.

**`b` = NUL.**
- ECMA-48: outside the command string, its recovery the implementation's
  (E1, E5). STD 070: ignored (D3), valid. VT510: ignored (V1), valid.
  williams: put, and by W3 valid. ctlseqs: valid once dropped (**R**).
- xterm: no reply at all (X4).
- **baud: valid**, the graph ignoring the byte.

**`b` = DEL.**
- ECMA-48: removable (E7), so `m` is a permitted reading. STD 070: ignored
  (D3, D6), valid. VT510: ignored (V1), valid. williams: ignored before the
  handler (W1), valid. ctlseqs: valid once dropped (**R**).
- xterm: stored, `DCS 0 $ r ST`.
- **baud: valid since `e6a817f`** (D2); it answered `0$r` at `83fee5d`.

**`b` = BEL.** STD 070's recommendation (D3) and W3 give valid; williams' row
puts it; ctlseqs gives valid once it is dropped (**R**). xterm rings, stores
it and answers invalid. **baud: `DCS 0 $ r ST`** (D2), without a bell: a
departure from D3 and W3, which section 5 lists as work.

**`b` = `ED`, an `m` with its eighth bit, under ISO 8859-1.** E4, W7 and C3
read the `Pt` as `m`, so valid. D4 leaves it to DECRQSS's format, and D11's
format is a control sequence's characters, whose GR codes STD 070 truncates to
seven bits (D8; **R**, section 3). No DECRQSS text speaks of GR. xterm, as
practice, reads it as `m` too. **baud: `DCS 0 $ r ST`** (D3), a departure
from the texts, which agree; section 5.5 lists it as work.

## 5. The other C0 in a DCS body: work, for Jev, not a question

"The other C0" is every C0 but the format effectors and `CAN`, `SUB` and
`ESC`: `00-07`, `0E-17`, `19` and `1C-1F`, 23 bytes, `BEL` included. What a
handler does with a byte it is put is part of each option, because the
texts' marks depend on it.

### 5.1 The options

- **A: keep today's split.** The graph puts `BEL` and ignores the other 22.
  DECRQSS keeps a `BEL` in its `Pt`, so `m BEL` answers invalid; Sixel drops
  it (today silently; build 1 rings it); every other DCS discards it.
- **B: put them all, and each handler decides**, williams' row (W1). The
  graph puts all 23. DECRQSS ignores every C0 before it reads its `Pt`, as
  STD 070 3.5.4.4 recommends (D3), so `m NUL` stays valid and `m BEL` becomes
  valid; the Sixel handler gives `BEL` and `ENQ` their text-mode action, as
  070-9 §9.1 says, and ignores the rest; every other DCS stores it with the
  rest of the string and discards the whole at its terminator (D5), as it
  does a `BEL` today.
- **B'**: B with DECRQSS keeping the 23 in its `Pt` and answering invalid for
  any of them, as xterm's `strcmp` does.
- **C: ignore them all at the parser.** `BEL` joins the 22 the graph ignores.
  `m BEL` becomes valid, and no handler is put any C0 but `08-0D`.

### 5.2 Marks, text by text

| text | A | B | B' | C |
|---|---|---|---|---|
| ECMA-48: outside the command string (E1); error recovery the implementation's (E5) | conforms | conforms | conforms | conforms |
| williams' row, put `00-17,19,1C-1F` (W1, W2) | departs: 22 ignored | conforms | conforms | departs: 23 ignored |
| williams on DEC's practice: ignored where meaningless, acted on where meaningful (W3) | departs: DECRQSS's `BEL` changes the answer | conforms | departs: the bytes change the answer | departs: a Sixel's `BEL`, which 070-9 gives a meaning, reaches no handler |
| STD 070 3.5.4.4, "recommended that all other control codes not used within a command string be ignored by hardware" (D3) | departs for DECRQSS's `BEL`; conforms elsewhere | conforms: the terminal ignores them, in the handler | departs | conforms |
| STD 070 3.5.5's reference parser: "Handling of control strings must be performed at a higher level than the parser" (D10) | conforms (**R**, below) | conforms | conforms | conforms (**R**, below) |
| STD 070-9 §9.1, in a Sixel: `BEL` and `ENQ` "same action as in text mode", "All other CO[sic] Codes are ignored" (section 7) | departs today for `BEL`, which is dropped silently, and conforms once build 1 rings it; `ENQ` conforms by equivalence (7.2) | conforms | conforms | departs: `BEL` cannot ring; `ENQ` conforms by equivalence |
| VT510/VT520, NUL "ignored by the terminal" (V1) | conforms | conforms | departs | conforms |
| ctlseqs, "Pt A text parameter composed of printable characters" (C1, line 45), read as notation (**R**, below) | conforms: a `Pt` holding `BEL` answers invalid, a recovery C4 allows; no other C0 enters it | conforms: the `Pt` read holds printable characters only | conforms: a `Pt` holding a C0 answers invalid | conforms: no C0 enters the `Pt` |
| ctlseqs, error recovery "implementation-dependent" (C4) | conforms | conforms | conforms | conforms |

xterm, as practice: it stores every C0 in the buffer, so its DECRQSS answers
invalid for any of them and gives no reply for a NUL (X4), and it rings a
`BEL` in every string but an OSC (X3).

**B conforms to every row, so by this note's rule it is work, for Jev**
(section 10.1, item 10), with A, B' and C kept as the alternatives considered
(section 5.4). Three readings carry that, and each is this note's (**R**):

- **ctlseqs' `Pt` is notation.** C1 sits under "Definitions", which introduces
  the parameters of the sequences ctlseqs lists (29-31): it says what a
  request holds. What a terminal does with a byte outside that, ctlseqs says
  apart from it: ECMA-48 "describes only correct behavior", and "the action
  taken in error recovery is implementation-dependent" (C4). A handler that
  ignores a C0 before it reads the `Pt` reads a `Pt` composed of printable
  characters. The other reading is that a C0 received inside the string is
  part of the `Pt` and makes the request invalid. Under it A, B and C depart
  from C1 and B' alone conforms; B' departs from D3, V1 and W3, so no option
  would conform to every text and this would be a question with C1 on one
  side. That reading is not in C1's text, and applied to every byte, as this
  note must apply it, it makes the `e6a817f` answers to `m CR`, `m LF`, `m HT`
  and `m DEL` departures from ctlseqs as well, where the record of `e6a817f`
  lists ctlseqs, C1 with C4, among the texts that let `DEL` be dropped. STD
  070's DECRQSS describes the request the same way, as the "intermediate and
  final characters" its data holds (D11), and says nothing of a C0 in it.
- **"Ignored by hardware" names no layer.** D3 recommends that the terminal
  ignore them; B ignores them in the handler, as W3 says DEC's devices did
  with a C0 that has no meaning for the device function.
- **baud's `DcsStr` row is already above STD 070's reference parser.** D10's
  parser "does not keep track of whether a control string is in progress";
  baud's graph does, so its body row is part of the "higher level" D10 hands
  string handling to, whichever bytes it ignores. D10 therefore marks no
  option down.

**SO and SI are two of the 23**, and B decides only that they enter neither
the `Pt` nor the image, which every text allows (E4: "no effect on the
interpretation of ... a control string"). Whether they shift what follows the
string is section 5.5's open item, under every option.

### 5.3 What building B means

- **The graph: 44 edges.** `Consume` to `StrPut` for the 22 bytes `00-06`,
  `0E-17`, `19` and `1C-1F` in `DcsStr`, under both encodings; `07` is put
  already. A dump of every edge, 11,264 lines an arm, differs from the base in
  exactly those 44 (**M**, arm E below). The tables keep their size (22 rows
  of 256 an encoding).
- **DECRQSS**: the drop in `str_put` widens from `is_format_effector` to
  `is_c0` (osc.rs 100), one comparison, so every C0 is ignored before the
  `Pt` is kept; `m BEL`, `m NUL`, `m ENQ` and the rest answer `DCS 1 $ r 0 m
  ST`.
- **Sixel**: the arm counts a bell on `07`, `bells += 1` as text mode's
  `Act::Bell` does (write.rs 605-606); that is build 1. `ENQ` takes text
  mode's action, which in baud is nothing: `c0()` has no `05` arm (graph.rs
  650-660), baud keeps no answerback message and has no setting for one, and
  `docs/vt-coverage.md` has no `ENQ` row (section 11). If an answerback is
  ever built, the Sixel arm sends it too. Every other C0 is dropped, as
  today.
- **Every other DCS** (`StrKind::Other`) stores the byte and discards the
  string whole at its terminator, as it does a `BEL` now; only
  `string_truncations` can tell, on a string past `OSC_MAX`.
- **On the path**: each such byte in a DCS body is a cold call to `str_put`
  where it cost nothing. No recording or fixture holds one in a DCS (the
  record's scan at `e6a817f`, and the two Sixel streams, section 13).
- **The stride ladder** moves from 36/1,075/27,634/663,397 classes at strides
  1 to 4 to 36/1,075/27,640/664,425: stride 3 by 6 and stride 4 by 1,028,
  +0.15% (**M**: the counts `the_alphabet_reduces_as_the_stride_doubles`
  prints and its failed assertion, arm B). The edges alone move it; the two
  handler changes do not (arms D and S, 5.4).
- **The tests that go red** under arm B, measured: 7 of 1,951, all in
  baud-core, and each one pins today's split, so each is rewritten with the
  build rather than broken by it. Nothing outside baud-core goes red: the two
  oracles, `baud-difftest` and `baud-sfte-difftest`, and the render and app
  tests stay green.
  - `graph::classes::a_dcs_body_puts_the_format_effectors_and_ignores_del`,
    the table gate, fails at `Utf8 [27, 80, 36, 113] 0x00`: `StrPut` where it
    pins `Consume`.
  - `graph::classes::the_alphabet_reduces_as_the_stride_doubles`, at stride
    3: 27,640 where it pins 27,634.
  - `a_format_effector_in_a_dcs_body_reaches_the_handler_and_a_del_does_not`
    (`tests/control_strings.rs`), at "SOH, ignored as before": a discarded
    DCS now holds the byte, truncations `(1, 1)` where it pins `(0, 1)`.
  - `a_bel_in_a_dcs_body_is_payload_and_only_st_ends_it`
    (`tests/control_strings.rs`) and `dcs::a_bel_does_not_terminate_a_dcs`
    (`tests/vt220/dcs.rs`): `m BEL` answers `DCS 1 $ r 0 m ST` where they pin
    `DCS 0 $ r ST`.
  - `a_bel_inside_a_sixel_is_payload_and_draws_nothing` and
    `a_c0_or_del_inside_a_sixel_number_splits_nothing` (`tests/sixel.rs`): a
    `BEL` in a Sixel counts one bell where they pin none.
- **Red-first tests for the build**: the table gate's `DcsStr` row takes
  `StrPut` for all 23 under both encodings; `DCS $ q m <c> ST` answers `DCS 1
  $ r 0 m ST` for each of the 23, at every read size and under both
  encodings, against the control `DCS $ q m ST`; a `BEL` between two data
  bytes, inside a number and inside a colour command of a Sixel counts one
  bell each and leaves the image the `BEL`-less body draws (build 1); `!1 ENQ
  0~` draws 10 wide and replies nothing, as `ENQ` in text does; a DCS that is
  neither DECRQSS nor Sixel and holds all 23 is discarded whole, counted once
  in `strings_ignored`.
- **The text that goes stale** when it lands is listed in section 11.

### 5.4 The alternatives considered, measured

Six arms, each applied to a git-archive copy at `2543f8b` and run under
`cargo nextest run --workspace`, 1,951 tests, none red on the copy unperturbed
(2 skipped): **E** the 44 edges alone, **D** the DECRQSS drop widened alone,
**S** the Sixel bell alone, **B** = E + D + S, **B'** = E + S, and **C** the 2
edges, `07` in `DcsStr` under each encoding from `StrPut` to `Consume` (a dump
differs from the base in exactly those 2, **M**). The patches and each arm's
red list are in `docs/research/notes/an/q1-arms/`.

| test | E | D | S | B | B' | C |
|---|---|---|---|---|---|---|
| `graph::classes::a_dcs_body_puts_the_format_effectors_and_ignores_del` | red | | | red | red | red |
| `graph::classes::the_alphabet_reduces_as_the_stride_doubles` | red | | | red | red | red |
| `a_format_effector_in_a_dcs_body_reaches_the_handler_and_a_del_does_not` | red | | | red | red | red |
| `a_bel_in_a_dcs_body_is_payload_and_only_st_ends_it` | | red | | red | | red |
| `dcs::a_bel_does_not_terminate_a_dcs` | | red | | red | | red |
| `a_bel_inside_a_sixel_is_payload_and_draws_nothing` | | | red | red | red | |
| `a_c0_or_del_inside_a_sixel_number_splits_nothing` | | | red | red | red | |
| `graph::classes::a_bel_ends_an_osc_and_no_other_control_string` | | | | | | red |
| **red, of 1,951** | **3** | **2** | **2** | **7** | **5** | **6** |
| stride ladder 1-4 (base 36/1,075/27,634/663,397) | 36/1,075/27,640/664,425 | unchanged | unchanged | 36/1,075/27,640/664,425 | 36/1,075/27,640/664,425 | 36/1,075/27,632/663,303 |

- **A**: nothing changes and nothing goes red. It departs as section 5.2
  marks it.
- **B'**: 44 edges and the Sixel bell; 5 red, E's three and S's two. The
  two DECRQSS tests stay green, since `m BEL` still answers invalid; `m NUL`
  and the other 21 turn invalid where they answer valid, which no test pins.
  The ladder moves as B's. It departs from D3, V1 and W3.
- **C**: 2 edges; 6 red: the table gate and the handler test at their `BEL`
  rows ("BEL, put as before", truncations `(0, 1)` where `(1, 1)` is
  pinned), the ladder, which moves to 36/1,075/27,632/663,303 (stride 3 down
  2, stride 4 down 94, -0.01%), the two DECRQSS tests, `m BEL` answering
  valid, and `a_bel_ends_an_osc_and_no_other_control_string`, which pins the
  `BEL` edge in `DcsStr`. The two Sixel tests stay green, since no `BEL`
  reaches the Sixel arm. It departs from williams' row for all 23, and from
  070-9 §9.1 and W3 for a Sixel `BEL`; build 1 would then need a Sixel body
  node of its own (question 1's option b gives its cost).

### 5.5 The other classes, still open, and two to build

These stay as the record of `e6a817f` names them and are not re-optioned here:

- SO and SI in a body: E4 lets them affect what follows; baud drops them
  without a shift.
- A C0 in the prefix: W8 against D8.
- A string that CAN, SUB, ESC or a C1 cancels is handed to no handler, where
  W2 unhooks it (the record, "A C1 control inside a control string"). For a
  Sixel cut off by `ESC` or a C1 the texts agree and it is work (7.5, 7.11);
  for one cut off by `CAN` they conflict, question 2 (7.4).
- `SUB` draws no error character (note AF, Q-UNDEF).

**To build, the texts agreeing:**

- **Item 9**: a GR byte in the DCS introducer under ISO 8859-1 is its 7-bit
  equivalent (D4, W7, E4, C3). baud sends it to the body (graph.rs 901, 909),
  so `ESC P A4 F1 m ST` is no DECRQSS (**M**, D4). The `dcs_entry` and
  `dcs_param` rows read done; the record of `e6a817f` found this and left it
  for its own change.
- **Item 11**, for Jev: a GR byte in a DECRQSS body under ISO 8859-1 is its
  7-bit equivalent (section 3: E4, W7, C3, and D4 through D11 and D8, **R**).
  The record of `e6a817f` left `A0-FF` in a body open as E4 against D4; for a
  DECRQSS, D4 hands the meaning to a format whose texts say nothing of GR and
  whose data are a control sequence's characters, so the record's conflict is
  none there. A Sixel's format sets it (build 3, section 7.10), and every
  other DCS is discarded whole. baud stores the byte raw, so `DCS $ q ED ST`
  answers invalid (**M**, D3). The build clears the eighth bit of `A0-FF` in
  DECRQSS's arm of `str_put`, as build 3 does in the Sixel's; `FF` becomes
  `7F` and is dropped as a `DEL` is. Under UTF-8 the bytes belong to a
  character, and no text speaks.

---

# Part 2. A Sixel body, per byte

## 6. The texts

- **070-9**, [[source: dec-std-070]] PDF 908-935, read whole (the provenance
  above). §6.1 places the Sixel in a DCS "as de1in.ed[sic] in the 'Code
  Extension' section of this standard, DEC STD 070-3" [image: "defined", and
  double quotes round Code Extension] (p. 5, PDF 916), so chapter 3's rules
  apply where 070-9 does not say otherwise (**R**). Its goals require a
  protocol "transmittable on 7-bit stream-oriented serial lines, including
  control code (X-ON, X-OFF) flow control" (p. 2, PDF 913).
- **The VT330/VT340 manual's Sixel chapter**, [[source: vt3xx-sixel]], by line
  of the HTML. It gives the envelope, the data range, the commands and the
  scrolling modes, and for the DCS defers: "NOTE: See Volume 1, Chapter 2 of
  this manual for general information about device control strings" (95-96).
  Volume 1 is not pinned (**U**). The chapter says nothing of any C0, of
  SPACE, DEL, GR or errors: on the tag-stripped text, `SUB`, `cancel`, `BEL`,
  `bell`, `space`, `blank`, `C0`, `eighth`, `8th`, `error`, `ignore` (as a
  word), `DEL`, `delete` and `ENQ` each match nothing, while the controls
  `repeat` (7) and `raster attributes` (5) match.
- **williams**, Part 1's W lines. It names Sixel nowhere; its DCS rules are
  general.
- **ctlseqs**' Sixel section, [[source: ctlseqs]] 3208-3243, gives the envelope
  `DCS Pa ; Pb ; Ph q Ps..Ps ST` and defers to the VT330/VT340 manual
  (3229-3235). It has no per-byte rule; its general string rules are Part 1's
  C lines. Its history section judges, as xterm's maintainer, that "DEC STD 070
  is more likely to describe the actual behavior of DEC's terminals than the
  more polished user's guides" (3500-3502).
- **xterm's Sixel parser**, [[source: xterm-graphics-sixel]], practice.
  charproc.c hands it each byte whose `sos_table` case is `CASE_IGNORE`, as
  `AsciiOf(c)` (3408-3412), and calls `parse_sixel_finished` only at `CASE_ST`
  (5311-5312).
- **The VT320 summary**, [[source: vt320-uu-appe]], is pinned for note AF and
  bears on one row: its CAN, SUB and ESC entries name escape and control
  sequences and no device control string (3751-3765). It is the VT300
  generation's text-only terminal, not a Sixel one.

## 7. Byte by byte

Each class gives every text that speaks, baud, the marks, and whether the
texts agree. 070-9 quotes are as extracted.

### 7.1 `07` BEL

- **070-9 §9.1** (p. 14, PDF 925): "Codes in the range 0/0 through on[sic]
  and 0114[sic] through 1115[sic] are considered errors" [image: "0/0 through
  0/7 and 0/14 through 1/15"]. "Cancel (CAN), Substitute (SUB), Escape (ESC),
  Bell (BEL), and Enquire (ENQ) are treated as follows:" and (p. 15, PDF 926)
  "4. BEL·[sic] same action as in text mode." [image: "BEL -"]
- **STD 070-3 §3.5.4.4** (D3): "Control characters other than those
  recognized within the command string syntax may be defined to have other
  meanings .in[sic] the future. It is recommended that all other control codes
  not used within a command string be ignored by hardware." In a Sixel, `BEL`
  is recognised within the syntax by 070-9 §9.1, so the recommendation does
  not reach it (**R**).
- **williams**: put to the handler (W1, W2); a C0 with a meaning for the device
  function is "acted upon" (W3).
- **vt3xx-sixel** and **ctlseqs**: silent (section 6).
- **xterm**: `CASE_BELL` rings and leaves the Sixel open, and the parser never
  sees the byte (charproc.c 3410, 3691-3705).
- **baud**: the graph puts it (graph.rs 1195-1199) and the Sixel handler drops
  it with every C0 (osc.rs 75-77). **M**: S12, a `BEL` between two data bytes
  leaves `bells` at 0 and the image 2 wide; a `BEL` in text counts 1. Text
  mode's action is `Act::Bell` (graph.rs 658), `bells += 1` (write.rs 605-606).
- **Marks**: departs from 070-9 §9.1; consistent with W1 and W3, which leave
  the act to the handler.
- **The texts agree. Build:** the Sixel arm of `str_put` counts a bell on
  `07` and stores nothing. It is the Sixel half of section 5's build, and
  buildable on its own today, since the graph already puts the `BEL`; under
  section 5.4's alternative C it would need a Sixel body node.
  2026-10-09's "neither rings" (the record, "A BEL ends an OSC and no other
  control string") was decided from williams' rows before 070-9 was read; it
  stands for every other string.

### 7.2 `05` ENQ

- **070-9 §9.1** (PDF 926): "5. ENQ.[sic] same action as in text mode."
  [image: "ENQ -"]
- **williams**: put (W1). **vt3xx-sixel**, **ctlseqs**: silent. The VT320's
  ENQ "Sends the answerback message" ([[source: vt320-uu-appe]] 3689-3691).
- **xterm**: `ENQ` is `CASE_IGNORE` in `sos_table` (VTPrsTbl.c 7628), so it is
  handed to the Sixel parser and answers nothing.
- **baud**: text mode's `ENQ` is `Act::Consume` (graph.rs 650-660): baud keeps
  no answerback and sends nothing. In a DCS body the graph ignores it (1252).
  **M**: S13, `ENQ` in text replies nothing, and `!1 ENQ 0~` is 10 wide.
- **Marks**: conforms to 070-9 by equivalence: the text-mode action is
  nothing, and nothing happens. If baud ever answers `ENQ` in text, a Sixel's
  `ENQ` must reach the handler and answer too; section 5's build puts it
  there.
- **No work.** `docs/vt-coverage.md` has no row for `ENQ` (section 11).

### 7.3 `1A` SUB

- **070-9 §9.1** (PDF 926): "2. SUB·[sic] process as if a 3115[sic] code
  (blank Sixel) was received·[sic] this is to limit the affect[sic] of some
  communication line errors." [image: "SUB -", "3/15", "received -"; "the
  affect of" is in the original]
- **STD 070-3 §3.5.4.4** (D3): "the control codes CAN (1/8), SUB (1/10), ESC
  (1/11), or any C1 control will also terminate control strings", "For
  backward compatibility, and to minimize the effect of a lost String
  Terminator". 070-3's change bars date this wording to its Rev A,
  14-Apr-1989; 070-9's pages carry no change bars, so its sentence may be
  from its Rev A (1987) or A1 (1990). The provenance above gives both, and
  this note draws no order from them.
- **williams**: SUB cancels "any ... control string in progress" and "will
  also display the error character" (608-610), and unhooks the handler (728).
  Its 610-611 adds: "The programmer's information for the VT320 says that CAN
  and SUB 'no longer' cancel these sequences, so there must have been a
  rethink when the VT420 was being designed."
- **VT510**: "Immediately cancels an escape sequence, control sequence, or
  device control string in progress, and displays a reverse question mark as
  an error character" (V1). The VT320's SUB "Immediately cancels an escape
  sequence or control sequence in progress", no DCS named
  ([[source: vt320-uu-appe]] 3757-3760).
- **ECMA-48**: `SUB` is outside a command string (E1); ECMA-48 says nothing
  of one arriving inside a string and leaves error recovery to the
  implementation (E5). §8.3.148 says "SUB is used in the place of a character
  that has been found to be invalid or in error" (E6), which is what 070-9's
  blank sixel does with it (**R**). It says nothing of ending a string on it.
- **vt3xx-sixel**: silent. **ctlseqs**: error recovery
  "implementation-dependent" (C4).
- **xterm**: `CASE_SUB` resets the parser and draws an error character
  (charproc.c 3646-3678), without `parse_sixel_finished`.
- **baud**: `SUB` goes to ground from any node (graph.rs 700-702), no handler
  is told, and no error character is drawn. **M**: S14, `~~ SUB ~~` places no
  image, and the two `~` after the `SUB` are drawn as text, `row0 "~~"`; the
  same body with `?` in place of the `SUB` is one image 5 wide.
- **Marks**: conforms to D3, williams and the VT510 (but for the glyph, note
  AF); departs from 070-9 §9.1.
- **The texts conflict: question 1.**

### 7.4 `18` CAN

- **070-9 §9.1** (PDF 926): "1. CAN·[sic] exit DCS mode processing, exit
  Sixel mode." [image: "CAN -"]
- **D3**, **V1** ("Immediately cancels ... device control string in
  progress"), **williams** (608-609, and the unhook, 728): the string ends.
- **What was drawn, kept.** 070-9 prints as it receives: "Sixel printing
  consists of setting context and attributes for the pixels and then printing
  each Sixel in received order" (§5, p. 4, PDF 915), and §11.3, "ExmNG[sic]
  SIXEL MODE" [image: "EXITING"] (p. 17, PDF 928), updates the text position
  "to reflect the vertical motion commands executed in the Sixel printing".
  So the sixels before the
  `CAN` stay (**R**). williams says so of any string: "Cancelling a control
  string does not undo these effects" (501-503), and unhook lets the handler
  "finish neatly" (728-729).
- **What was drawn, discarded.** ECMA-48 §8.3.6 (E6): "CAN is used to
  indicate that the data preceding it in the data stream is in error. As a
  result, this data shall be ignored." It fixes no scope for "preceding".
  xterm's comment on `CASE_CAN` quotes the same clause (charproc.c 3619-3626)
  and goes on: "The scope of 'preceding' is vague. DEC 070 clarifies it by
  saying that the current control sequence is cancelled, and also page 3-30,
  3.5.4.4 Termination Conditions" (3627-3629), practice: a reading that STD
  070 narrows "preceding" to the sequence or string in progress, which is
  still silent on what that string has already drawn. Read as the string so
  far, ECMA-48 discards the image; read as the sixel in progress, it does not
  (**R**).
- **STD 070-3** (D3) says CAN "will also terminate control strings", and the
  **VT510** (V1) that it "Immediately cancels ... device control string in
  progress"; neither says what becomes of what the string already did.
  **vt3xx-sixel**, **ctlseqs**: silent (ctlseqs leaves error recovery to the
  implementation, C4).
- **xterm**: `CASE_CAN` resets the parser (charproc.c 3618-3644) and never
  calls `parse_sixel_finished`; whether the partial graphic stays drawn is
  decided in graphics.c, not pinned (**U**).
- **baud**: the string goes to ground (graph.rs 700-702), and `sixel_end`
  runs only from `str_end` at a terminator (osc.rs 127), so nothing is
  decoded. **M**: S15, `~~ CAN Z` places no image and draws `Z`.
- **Marks**: the string ends, as every text says, and that conforms. What
  was received is discarded: that departs from williams (501-503, 728-729)
  and from 070-9's model (**R**), and conforms to ECMA-48 §8.3.6 under its
  wider reading.
- **The texts conflict on what was drawn: question 2.** For `ESC` and a C1
  no text discards it (7.5, 7.11), and that is work.

### 7.5 `1B` ESC

- **070-9 §9.1** (PDF 926): "3. ESC·[sic] exit DCS mode processing, exit
  Sixel mode, begin processing escape sequence." [image: "ESC -"] §6.1 (PDF
  916): ST "may also be coded as ESC \ (1111, 5112)[sic] in 7·bits[sic]"
  [image: "(1/11, 5/12)", "7-bits"].
- **D3**, **V1**, **williams** (497-507): ESC ends the string; `ESC \` is ST,
  or a cancel and a no-op ST, the handler unhooked either way (728).
- **baud**: `ESC \` ends and decodes; any other byte cancels and runs from
  `Escape` (graph.rs 917-922). **M**: S16, `~~ ESC [5C` places no image and
  moves the cursor to column 5.
- **What was drawn.** williams keeps it: "Cancelling a control string does not
  undo these effects" (501-503), and unhook lets the handler "finish neatly"
  (728-729). 070-9 exits Sixel mode, which §11.3 describes as after printing
  (**R**, 7.4). ECMA-48 puts `ESC` outside a command string (E1), leaves
  error recovery to the implementation (E5), and gives it no discarding rule;
  nor does any other text.
- **Marks**: the escape sequence runs, as 070-9 says, and conforms; the image
  is discarded, a departure from williams and 070-9 (**R**).
- **The texts agree. Build:** a Sixel that `ESC` cuts off (any byte but `\`
  after it) is decoded and placed from what it received, the cursor moving
  as at ST, and then the escape sequence runs. It is the record's open cancel
  question, narrowed to the string and the bytes whose texts settle it. Cost:
  the edges that cancel a Sixel body, `StrEsc`'s edge for a byte other than
  `\` (graph.rs 917-922) and, under ISO 8859-1, the C1 edges (750-761), take
  an act that ends a Sixel and then runs the byte. `StrEsc` and `DcsStr` are
  shared by every string, so the act tests the string's kind, on a path taken
  only at a cancel; unmeasured.

### 7.6 `08-0D`, the format effectors

- **070-9 §9.1** (PDF 925): "The inclusion of CO[sic] Controls 0/8 to 0113[sic]
  in control string data'is[sic] intended for convenience in formatting and
  storing control strings. These codes shall have no effect on the
  interpretation of Size![sic] Control strings." §8.1 (p. 8, PDF 919): "Note
  Format Effectors (0/8 to 0/13) and SPACE (210)[sic] are ignored within sixel
  control strings (see coding section)." Change history item 20 (p. 21, PDF
  932): "format effectors (018 - 0113)[sic] shall not effect the
  interpretation of sixel control strings."
- **D1**, **D3**: valid, and recommended not to affect the interpretation.
  **williams**: put (W1). **vt3xx-sixel**: silent.
- **xterm**: `HT LF VT FF CR` are skipped as white space without ending a
  number; `BS` is not white space and ends one
  ([[source: xterm-graphics-sixel]] 541-548).
- **baud**: put (graph.rs 1225-1228), dropped by the handler (osc.rs 75-77).
  **M**: S5, `!1 CR 0~` is 10 wide.
- **Conforms**, since `e6a817f`. The texts agree.

### 7.7 The other C0: NUL, XON, XOFF and the rest

- **070-9 §9.1** (PDF 925-926): "The communications control characters, XON
  (DCl)[sic] and XOFF (DC3) are handled at a different level" and "All other
  CO[sic] Codes are ignored."
- **D3** recommends ignoring them; **V1** ignores NUL; **williams** puts them
  (W1) and DEC's practice ignores a meaningless one (W3).
- **xterm**: a NUL is skipped, but a NUL inside a repeat ends the image
  ([[source: xterm-graphics-sixel]] 541-547); the rest reach its parser and end
  a number.
- **baud**: the graph ignores them (graph.rs 1252), and `str_put` would drop
  them too. Flow control is the tty's: baud's parser has none. **M**: S18,
  `!1 DC1 0~!1 DC3 0~` is 20 wide; S20, `!1 NUL 0~` is 10.
- **Conforms.** The texts agree.

### 7.8 `20` SPACE

- **070-9 §9.2** (PDF 926): "Codes in the range 210[sic] through 7/15 are
  treated as follows: 2/0 (SPACE) Ignored" [image: "2/0 through 7/15"]. §8.1
  (PDF 919): "Note Format Effectors (0/8 to 0/13) and SPACE (210)[sic] are
  ignored within sixel control strings", where a command "is terminated by any
  non parameter code, that is, any code other than 3/0 through 3/9 (parameter
  digits 0-9) or 3/11 (parameter separator ;)". Change history item 19 (PDF
  932): "Removed SPACE (210)[sic] from the set of sixel control codes. Added
  statement SPACE shall be ignored in sixel control strings."
- **vt3xx-sixel**: silent. **williams**: put (W1), the handler deciding.
  **ctlseqs**: silent.
- **xterm**: skipped as white space before the digit test, so it ends no
  number ([[source: xterm-graphics-sixel]] 541-548).
- **baud**: stored (osc.rs 78-83). The decoder skips it between commands
  (`baud-domain/src/sixel.rs` 271-273), but `number` stops at the first
  non-digit (302-314), so a SPACE inside a parameter ends it. **M**: S2,
  `!1 SPACE 0~` is 1 wide where `!10~` is 10; S4, `#1 SPACE ;2;100;0;0#1~`
  defines nothing and draws transparent; S3, `~ SPACE ~` is 2 wide, as it
  should be.
- **Marks**: departs from 070-9 §9.2 and §8.1 inside a parameter list;
  conforms between commands.
- **The texts agree** (070-9 alone speaks). **Build:** the Sixel arm drops a
  SPACE before the decoder, as it drops a C0. It is the shape of the `BEL`
  that split a number on 2026-10-09.

### 7.9 `7F` DEL

- **070-9 §9.2** (PDF 926): "7/15 (DELE1'E)[sic] Ignored" [image:
  "DELETE"]; change history item 21: "SPACE (210)[sic] and. DELETE (7115)[sic]
  shall be ignored."
- **williams** ignores it (W1); **D3**, **D6**, **V1** ignore it.
- **xterm** hands it to the parser, where it ends a number.
- **baud**: ignored by the graph (graph.rs 1274-1277). **M**: S5, `!1 DEL 0~`
  is 10 wide.
- **Conforms**, since `e6a817f`. The texts agree.

### 7.10 `A0-FF`, GR

- **070-9 §9.4** (PDF 926): "Codes in range 10/0 through 15115[sic] are
  errors. The 8th bit is set to zero and the code is then processed as a GL
  code. Conforming software shall not transmit GR codes." [image: "15/15"]
- **D4**: a DCS's GR characters are "dependent on the internal control string
  format", which for a Sixel is 070-9's. **E4**: the same meaning as `20-7E`.
  **williams** 462: "treated identically to GL codes 20-7F". **ctlseqs**
  128-130: xterm "ignores the eighth bit of bytes in control strings other
  than the C1 controls".
- **vt3xx-sixel**: "The terminal uses 6 bits of the 8-bit character code to
  encode bitmap data" (88-89); nothing on GR.
- **xterm**: `AsciiOf(c)` before the parser (charproc.c 3410-3411).
- **baud**: under ISO 8859-1, `A0-FF` is put (graph.rs 1278), stored raw
  (osc.rs 78-83) and skipped by the decoder (`baud-domain/src/sixel.rs`
  271-273). **M**: S11, `FE` (`~` with the eighth bit) draws nothing and the
  Sixel is refused as empty; `! B1 B0 ~` is 1 wide where 070-9 reads `!10~`.
- **Marks**: departs from 070-9 §9.4, and from E4, W7 and C3.
- **The texts agree. Build:** under ISO 8859-1 the Sixel arm clears the
  eighth bit of `A0-FF` before the decoder. `FF` becomes `7F`, which §9.2
  ignores.
- **Under UTF-8 no text speaks**: 070-9 describes 8-bit codes, and a byte in
  `80-FF` of a UTF-8 stream is part of a character. baud stores the raw byte
  and skips it (S11, `FE` under UTF-8). xterm decodes UTF-8 before its table,
  so a character in `A0-FF` reaches the Sixel parser with its eighth bit
  stripped as under 8859-1 (charproc.c 3390-3411), practice. Recorded, not a
  question.

### 7.11 `80-9F`, C1 under ISO 8859-1

- **070-9 §9.3** (PDF 926): "Codes in the range 8/0 through 9/15 exit DCS mode
  processing, exit Sixel mode, and the C1 control code is processed outside of
  the Sixel mode."
- **D3**, **williams** 612-613, xterm (VTPrsTbl.c 7781-7820): the same.
- **What was drawn**: williams' "Cancelling a control string does not undo
  these effects" (501-503) and 070-9's exit from Sixel mode (**R**) keep it;
  williams' unhook (728) names ST, CAN, SUB and ESC and not a C1, and no text
  discards it.
- **baud**: `latin1_c1` cancels and runs it, `9C` ending the string (graph.rs
  750-761). **M**: S17, `~~ 85 Z` places no image and the NEL moves the
  cursor to the next line.
- **Marks**: the C1 runs and conforms; the image is discarded, a departure.
  **The texts agree. Build:** as 7.5's, for a C1 other than `9C`.

### 7.12 The whole table

| class | 070-9 | STD 070-3 | williams | VT3xx | baud today | verdict |
|---|---|---|---|---|---|---|
| `07` BEL | text-mode action (ring) | ignore what is unused (rec.) | put; handler acts | silent | dropped silently (S12) | departs; agree: build |
| `05` ENQ | text-mode action | as BEL | put | silent | ignored; text mode does nothing (S13) | conforms |
| `1A` SUB | blank sixel | ends the string | ends, error glyph | silent | ends; tail drawn as text (S14) | conflict: question 1 |
| `18` CAN | exit | ends | ends, unhook | silent | ends; image discarded (S15) | ends: conforms; the discard: conflict with ECMA-48 §8.3.6, question 2 |
| `1B` ESC | exit, run the escape | ends | ends, unhook | silent | runs it; image discarded (S16) | runs: conforms; discard departs: build |
| `08-0D` | no effect | no effect (rec.) | put | silent | dropped (S5) | conforms |
| other C0 | ignored | ignore (rec.) | put | silent | ignored (S18, S20) | conforms |
| `20` SPACE | ignored | -- | put | silent | splits a number (S2, S4) | departs; build |
| `7F` DEL | ignored | ignored | ignored | silent | ignored (S5) | conforms |
| `A0-FF`, 8859-1 | as GL | by the format | as GL | silent | skipped (S11) | departs; build |
| `80-9F`, 8859-1 | exit, run the C1 | ends | ends, runs | silent | runs; image discarded (S17) | as ESC |

## 8. The commands in the data stream

070-9 says six things about the commands that baud's decoder does
differently or that settle a comment, all in `baud-domain/src/sixel.rs`
unless a line says otherwise.

**A zero repeat is one.** 070-9 §8.1.1 (p. 9, PDF 920): "A repeat count of
zero implies a repeat count of 1." The VT3xx chapter says only "The repeat
count can be any decimal value" (251). baud: `pn.max(1)` (209), whose comment
calls it "a guess the source does not settle" (205-206). **M**: S7, `!0~` is
1 wide. **Conforms**; the comment is stale (section 11).

**The maximum is 32767.** 070-9 §8.1.1: "The maximum. value for the repeat
count is 32767." The VT3xx chapter: "any decimal value". baud reads up to
65535 (`number`, 302-314, saturating). **M**: S7, `!40000~` is 40000 wide and
`!70000~` 65535. Neither text says what a device does with a larger count, so
no text is departed from; recorded.

**Another command cancels a pending repeat.** 070-9 §8.1.1: "If no sixel data
character follows the repeat count (before the STRING TERMINATOR or another
Sixel Command), the repeat count is ignored. The start of another Sixel
Command will cancel a pending repeat count." Change history item 11 (PDF 932)
says it again. A command is "a code in the range 211[sic] through 3/14
(except parameters and parameter separators)" [image: "2/1"], and "Sixel
Commands that are not recognized by the device must be parsed and ignored"
(§8.1, PDF 919). The VT3xx chapter is silent. xterm drops a pending repeat at
any non-data byte, and swallows that byte ([[source: xterm-graphics-sixel]]
585-591). baud resets `repeat` at `#`, `!`, `$`, `-` and `"` (202, 209, 216,
223, 235), but its catch-all (271-273) does not. **M**: S6, `!5%~` and `!5:~`
are 5 wide where `!5#1~` is 1. **Departs from 070-9; the texts agree (070-9
alone speaks). Build:** the catch-all resets `repeat` for `21-3E` other than
the digits and `;`.

**An invalid colour command is ignored whole.** 070-9 §8.1.3 (p. 10, PDF 921):
"Values for Pu, Px, Py, and pz[sic] other than those specified above cause the
entire color specifi.er[sic] commaDd[sic] to be ignored, including the color
select parameter, Pc", and "parameter value '0' (zero) for Pu will cause the
entire commaDd[sic] to be ignored." The values specified are `Pu` 1 or 2, HLS
hue 0-360 and lightness and saturation 0-100, RGB 0-100. The VT3xx chapter's
Table 14-1 gives the same ranges (369-416) with no rule for others. xterm
aborts the whole image on an out-of-range value
([[source: xterm-graphics-sixel]] 776-803), practice. baud selects `Pc` before
it reads `Pu` (186), clamps RGB above 100 (`pct`, 293-294), and selects the
register for an unknown or zero `Pu` while counting it unapplied (193-199).
**M**, S9, after a green register 1: `#5;2;150;0;0` draws red, and `#5;3;...`,
`#5;0;...` and `#5;1;400;50;50` draw transparent; 070-9 draws green for all
four. **Departs; the texts agree. Build:** read and check the whole command,
then define and select, or do neither.

**A register past the device is mapped back.** 070-9 §8.1.3 (PDF 922):
"Devices must implement color numbers 0 to 255. ... In the case of an index
which exceeds the bounds of the device, the unimplemented index must be mapped
back into the supported range." baud clamps to 255 (186). **M**: S10,
`#300;...#300~` draws the colour defined. **Conforms**; 070-9 fixes no
mapping, and xterm wraps, "The DEC terminals wrapped register indices"
([[source: xterm-graphics-sixel]] 696-697), practice.

**The pixel aspect ratio.**

- 070-9 §7.1 (p. 7, PDF 918) gives the macro parameter's table, read off the
  page image (the OCR has "0 (d.efault)[sic]" and "ll2[sic]"), vertical to
  horizontal pixel: macro value 0 (default) and 1, 200 : 100; 2, 450 : 100; 3,
  300 : 100; 4, 250 : 100; 5, 200 : 100; 6, 150 : 100; 7, 130 : 100; 8, 112 :
  100; 9, 100 : 100. "The intended vertical grid size implied by the macro
  value is always 1172[sic] of an inch" [image: "1/72"]. §10's note (PDF
  926): "the raster attributes (horizontal grid size, pixel aspect ratio) are
  initialized by the Macro Parameter, even if unspeci1ied[sic] and defaulted
  to zero." §8.1.2 (PDF 920): the raster attributes' first two parameters
  "specify the Pixel Aspect Ratio. This ratio defines the shape of the pixel
  needed to reproduce the picture without distortion." The command "must
  precede any Sixel, or any sixel positioning command within a single
  Bixel[sic] control string or the command will be ignored." And (PDF 921):
  "Conforming software shall specify all parameters explicitly. Guideline:
  omitted parameters for Pn1 and Pn2 may be interpreted as '0' (zero) and
  default to 'r[sic] (one). ";0 may be interpreted as "1;1 for example"
  [image: "'1' (one)"], a guideline being, in STD 070's own terms, "good
  practice, but should not be considered as requirements" (PDF 106). §12
  (p. 18, PDF 929): "The pixel aspect ratio should be preserved if
  possible."
- The VT3xx chapter: `P1` omitted is "2:1 (default)", `0, 1` 2:1, `2` 5:1,
  `3, 4` 3:1, `5, 6` 2:1 and `7, 8, 9` 1:1 (156-179); "If you use the set
  raster attributes command (`"`) in a sixel data string, you must specify a
  pixel aspect ratio. You can only use integer values for Pan and Pad. The
  VT300 rounds the pixel aspect ratio to the nearest integer" (293-295).
- **The two tables agree at `P1` omitted, 0, 1, 3, 5 and 9, and differ at 2,
  4, 6, 7 and 8**, where 070-9's ratio is not a whole number: 4.5, 2.5, 1.5,
  1.3 and 1.12. The VT3xx column there is 070-9's rounded half up, 5, 3, 2, 1
  and 1 (**R**).
- **The VT3xx column is the VT300's own fallback, and binds the VT300**
  (**R**). Its ratio "defines the shape of the pixels the terminal uses to
  draw the sixel image" (288-289), and "The VT300 rounds the pixel aspect
  ratio to the nearest integer" (295). 070-9 §12 leaves how a device falls
  back to that device's documentation: "Conforming devices must clearly state
  in their product documentation how grid sizes fall back" (PDF 928). The
  VT3xx chapter is that statement for the VT300, and baud is not a VT300: its
  DA identifies it as a VT102 (`docs/vt-coverage.md`, `CSI c`). This is the
  reading section 5.2 gives ctlseqs' `Pt`, a text that says what one thing is
  and not what every terminal must do. Read the same way, the VT510's Table
  4-1 (V1) binds the VT510, whose error recovery ECMA-48 leaves to it (E5); no
  mark in this note rests on V1 alone: D3, D6 or williams carries each one
  as well (sections 3, 4, 5.2, 7.3, 7.4, 7.5, 7.7, 7.9 and 14).
- xterm computes the ratio and then "FIXME: Aspect Ratio is buggy, so we'll
  just force it off", drawing 1:1 ([[source: xterm-graphics-sixel]] 268-272),
  practice.
- baud reads only `P2` at `SixelStart` (`baud-core/src/write.rs` 640-651),
  drops `Pa` and the raster attributes' ratio (`baud-core/src/sixel.rs`
  101-107; `baud-domain/src/sixel.rs` 229-236), and draws one image pixel per
  device pixel. **M**: S19, with no raster attributes, with `"2;1;1;6` and with
  `P1 = 9`, each image is 1x6.
- 070-9 lets a device fall back, within limits. §4.1 (p. 4, PDF 915): "Level 1
  devices will parse these commands, but only print in one grid size. The
  level 1 grid size is that defined for Macro Parameter 1", which the table
  gives as 200 : 100, so even a Level 1 device draws 2:1. §12 (p. 17, PDF
  928): "The commands allowing fallback are the interpretation of Pixel aspect
  ratio, Horizontal grid size, and color selection", "Conforming devices must
  clearly state in their product documentation how grid sizes fall back", and
  "The general rule for grid size fall back is that a device selects an actual
  grid size that will come closest to but does not exceed the specified grid
  size, but never less than the smallest imageable unit of the device." A
  device that can draw a sixel's pixel two device pixels tall has the
  specified vertical grid itself available, so the general rule picks it and
  not one device pixel (**R**); and baud documents no fallback. §12 measures
  the actual grid as an average (p. 18, PDF 929): "Since some devices may have
  high frequency grid size errors that do not accumulate, the average grid
  size over not more than 30 pixels is used to determine the actual grid
  size", and "The pixel aspect ratio should be preserved if possible." The
  same table's horizontal grid sizes are not taken up here: baud draws one
  sixel column per device pixel.
- **Departs from both DEC texts where they agree: a Sixel that sends no raster
  attributes, or `P1` 0, 1 or 5, has 2:1 pixels, `P1` 3 has 3:1 and `P1` 9
  1:1, and `Pan:Pad` sets the ratio; 1:1 throughout is neither 070-9's Level
  1 grid nor the grid its fallback rule would choose. Build 8**, with its
  costs stated:
  - the decoder writes each sixel row as many device rows tall as the ratio,
    and the extent, the arena reservation and the rows the cursor moves scale
    with it;
  - `Pan` or `Pad` omitted or 0 reads as 1, by 070-9's guideline (PDF 921),
    so `";` and `"0;0` draw as `"1;1`;
  - **a ratio that is not a whole number**, `P1` 2, 4, 6, 7 or 8 or a
    `Pan:Pad` such as `"3;2`, is item 12, below, not part of this build;
  - an encoder that sends `"1;1` draws as today. Both Sixel streams in the
    tree do: the ImageMagick capture `swirl.raw` opens `ESC P 0;1;0 q
    "1;1;64;48`, and `tests/recordings/sixel/img2sixel-96x72.six` opens `ESC P
    q "1;1;96;72` (**M**, `docs/research/notes/an/sixel-scan.py`), so
    `baud-domain/tests/sixel_oracle.rs`, which decodes the second, does not
    move;
  - a Sixel with no raster attributes, or `P1` 0, 1 or 5, draws twice as tall,
    and `P1` 3 three times. The hand-written Sixels in the tests are of that
    kind: none of the 48 lines that open one in `baud-core/tests/sixel.rs`
    (46), `baud-core/tests/image_anchor.rs` and `baud-render/tests/image.rs`
    carries a raster command (**C**), so every height they pin, and the frame
    digest the last one pins, moves. `baud-domain/tests/sixel_decode.rs`
    decodes eight bodies with no raster command (its lines 29, 53, 69, 92,
    111, 134, 152 and 178), each pinning a height or a pixel row: they move
    if the default ratio is applied inside `Sixel::decode`, as the first point
    has it, and not if it is applied at placement, where the baud-core tests
    above move instead. Its measure-pass cases (238-243) compare `decode` with
    `extent` and move together. The golden fixture `decrqss-sixel` sends no
    raster command either; it pins cells, the cursor and the reply, which no
    cell height moves there.
- Once the ratio is honoured, a raster command after data must be ignored, as
  both texts say; today it has no effect either way.

**Item 12: a pixel aspect ratio that is not a whole number**, `P1` 2, 4, 6, 7
and 8, which 070-9's table gives as 4.5, 2.5, 1.5, 1.3 and 1.12 to 1, and a
`Pan:Pad` such as `"3;2`. One option conforms to every text, for every `P1`
and every `Pan:Pad` whose `Pan` and `Pad` in lowest terms are both at most 30,
so it is work, for Jev (the status above), with the other two kept as
alternatives considered. ECMA-48, williams and ctlseqs say nothing of the
ratio and conform under every option; the VT3xx column binds the VT300 and not
baud (**R**, above).

- **a, preserve 070-9's ratio, the build**: the device rows a sixel row takes
  vary so that the image is the ratio times its rows tall, rounded once (4.5:1
  draws rows 4 and 5 device pixels tall in turn). Its reason is §12's "The
  pixel aspect ratio should be preserved if possible", its average "over not
  more than 30 pixels", and §7.1's table. The rows repeat every 2, 2, 2, 10
  and 25 sixel pixels (9/2, 5/2, 3/2, 13/10, 28/25), each within 30, so
  averaged over one repeat the grid is the specified one, which is as close as
  §12's general rule asks and does not exceed it (**R**). Conforms to 070-9
  and to §12's fallback rule. A repeat is `Pad` sixel pixels and `Pan` device
  pixels, and §12 does not say which pixels it counts; the `P1` ratios repeat
  within 30 of either (9, 5, 3, 13 and 28 device pixels). A `Pan:Pad` with
  `Pan` or `Pad` above 30 in lowest terms, such as `"31;30`, may repeat over
  more than 30, so its average over 30 comes near the ratio and not onto it;
  whether that meets "does not exceed" is this note's open reading (**R**).
  VT3xx's "You can only use integer values for Pan and Pad" binds the sender,
  so a `"3;2` is outside that text rather than against it (**R**).
- **b, considered: the largest whole ratio not above 070-9's**, 4, 2, 1, 1 and
  1, stated in baud's documentation as §12 requires. Its reason is §12's
  "closest to but does not exceed the specified grid size", read per pixel.
  Departs from §12's "should be preserved if possible", since a is possible,
  and its grid is further from the specified one than a's, which equals it
  (**R**).
- **c, considered: round half up, as the VT300 does**, 5, 3, 2, 1 and 1. Its
  reason is VT3xx's rounding (295) and its table. Departs from §12's general
  rule at `P1` 2, 4 and 6, where the rounded ratio exceeds the specified one,
  and from "should be preserved if possible"; conforms to the VT3xx column,
  which binds the VT300 (**R**).
- Cost, each: one function from the ratio to the device rows of each sixel
  row, on build 8's path; under a the height varies within an image, under b
  and c it is one height an image. Neither Sixel stream in the tree sends a
  ratio that is not whole (both send `"1;1`, **M**).

## 9. The state a Sixel starts in

070-9 §10 (p. 15, PDF 926): "No Sixel state is preserved from one entering of
Sixel mode to the next. All Sixel state is reset when the Sixel mode is
entered." And among the states initialized: "Sixel color: Contrastinq[sic] to
backqround[sic]" and "Color numbers: All unspecified color numbers imaqe[sic]
as the default Sixel color."

- The VT3xx chapter defers colour to "the 'Output Mapping' section in Chapter
  2" (419-420), not pinned (**U**). xterm starts on register 3 ("FIXME: using
  green, but not sure what it should be", [[source: xterm-graphics-sixel]]
  391-392); the registers' contents are set in graphics.c, not pinned (**U**).
  ctlseqs' `?1070` gives each graphic private registers (3224-3225).
- baud starts all 256 registers at `ColorWord::DEFAULT`
  (`baud-domain/src/sixel.rs` 167), the word the canvas, the CPU compositor and
  the GPU path all treat as an unpainted, transparent pixel (sixel.rs 60-63;
  `baud-render/src/composite.rs` 172-176; `baud-render/src/renderer.rs`
  1418-1421). **M**: S8, `DCS q ~ ST` and `#1~` each place a 1x6 image whose
  pixels are transparent: a Sixel with no colour command draws nothing.
- **Marks**: the reset per image conforms (the registers are a local of each
  walk, note V's V18). Unspecified registers drawing nothing departs from §10.
- **The texts agree (070-9 alone speaks). Build:** every register starts as
  one default Sixel colour that contrasts with the background. This note reads
  that as the default foreground (**R**), which is by definition the colour
  text contrasts with the default background in. A canvas pixel must then hold
  that colour resolved, since an indexed or default word draws nothing there.

## 10. Agree or conflict: the list

### 10.1 Build, the texts agreeing

| # | what | texts | baud today | evidence |
|---|---|---|---|---|
| 1 | a `BEL` in a Sixel rings, as in text | 070-9 §9.1; williams W1, W3 | dropped silently | S12 |
| 2 | a SPACE in a Sixel is ignored, inside a number too | 070-9 §8.1, §9.2 | ends a number | S2, S4 |
| 3 | a GR byte in a Sixel under ISO 8859-1 is its GL code | 070-9 §9.4; D4, E4, W7, C3 | skipped | S11 |
| 4 | a Sixel cut off by `ESC` or a C1 keeps what it drew | williams 501-503, 728; 070-9 §5, §11.3 (**R**) | discarded | S16, S17 |
| 5 | an unrecognised command cancels a pending repeat | 070-9 §8.1.1 | repeat kept | S6 |
| 6 | an invalid colour command is ignored whole | 070-9 §8.1.3 | `Pc` selected, RGB clamped | S9 |
| 7 | an unspecified register draws a default contrasting colour | 070-9 §10 | transparent | S8 |
| 8 | the pixel aspect ratio where the texts agree: 2:1 with no raster attributes or `P1` 0, 1, 5; 3:1 for `P1` 3; `Pan:Pad` honoured when whole; `Pan`/`Pad` omitted or 0 read as 1 | 070-9 §4.1, §7.1, §8.1.2, §12 (**R** on §12's rule), its guideline on PDF 921; vt3xx 156-179, 293-295 | always 1:1 | S19 |
| 9 | a GR byte in the DCS introducer under ISO 8859-1 is its 7-bit code | D4, W7, E4, C3 | sent to the body | D4 |
| 10 | the other 23 C0 in a DCS body (`BEL` among them) are put to its handler; DECRQSS ignores them before it reads its `Pt`, a Sixel rings a `BEL`, gives `ENQ` its text-mode action and ignores the rest | W1, W2, W3; D3, D10; V1; 070-9 §9.1; E5, C4; C1 read as notation (**R**) | 22 ignored by the graph; `m BEL` answers invalid | D2, S12, S13 |
| 11 | a GR byte in a DECRQSS body under ISO 8859-1 is its 7-bit code | E4, W7, C3; D4 with D11 and D8 (**R**) | stored raw, answers invalid | D3 |
| 12 | a pixel aspect ratio that is not a whole number is preserved, the rows a sixel row takes varying so the average over a repeat of at most 30 pixels is the ratio | 070-9 §7.1, §12 (**R**); the VT3xx column read as the VT300's own fallback (**R**) | always 1:1 | S19; the ratio is dropped (`baud-core/src/write.rs` 640-651, `baud-core/src/sixel.rs` 101-107, **C**) |

Item 1 is the Sixel half of item 10 and can be built first. Item 4 widens to
`CAN` under question 2's option b, and to `SUB` under question 1's option a,
for which williams unhooks (728) and no text discards (ECMA-48's SUB,
§8.3.148, stands in for a character and ignores nothing). Item 8 leaves the
ratios that are not whole numbers to item 12.

Items 10, 11 and 12 rest on this note's readings (**R**); 10 and 12 carry
alternatives considered (sections 5.4 and 8), and 11 has the alternative of
keeping the byte raw (section 5.5). Each is work, for Jev:
by the owner's rule of 2026-10-10, Jev at or above 0.9 confidence both times
decides between the options, and otherwise the owner does. None is built
before that.

### 10.2 Questions, the texts conflicting

- **Question 1**: `SUB` inside a Sixel (section 7.3).
- **Question 2**: what a Sixel that `CAN` cuts off keeps (section 7.4).

Each goes to Jev first, and to the owner if Jev does not reach 0.9 both
times.

### 10.3 Conforming, no work

`ENQ` by equivalence (7.2), the format effectors (7.6), the other C0 (7.7),
`DEL` (7.9), a C1 ending the string (7.11), a zero repeat (8), a register past
255 (8), and the reset of state per image (9).

## 11. Found on the way, decided by no question

- **Stale comments**, at `e6a817f`:
  - `baud-domain/src/sixel.rs` 205-206 says `[[source: vt3xx-sixel]]` "is
    silent on `Pn = 0`" and that one is "a guess the source does not settle".
    070-9 §8.1.1 settles it, as baud does it.
  - `baud-core/src/sixel.rs` 113-115 says "the DEC source for the exact column
    is unread". 070-9 §11.3 (PDF 928): "The Horizontal Active position of the
    ANSI text is not affected by S~el[sic] mode and set exactly to what is was
    when Sixel mode was entered." The column baud keeps is that one.
  - `baud-domain/src/sixel.rs` 269-270 says what the catch-all skips is
    "newlines an encoder wrapped the data with, stray whitespace". Since
    `e6a817f` no C0 reaches the decoder, and what does is SPACE, GR bytes and
    the unrecognised commands (7.8, 7.10, 8).
  - `docs/research/notes/s-sixel.md` calls `!0` a repeat "which the source does
    not define".
- **The row after an image.** 070-9 §11.3: "The ANSI text vertical position is
  updated to reflect the vertical motion commands executed in the Sixel
  printing", and §11.2.4 scrolls a video device past the bottom margin "if text
  drawn into this region would have scrolled". Note V's V18 lists baud's row
  as one lower than the VT3xx manual's; 070-9 is a second text on it.
- **070-9 places the HLS hues, which bears on the conversion
  `unapplied_registers` waits on.** Its colour introducer gives only "Hue
  angle, 0-360" (PDF 921), but §12's fallback table for eight-colour devices
  (p. 18, PDF 929, read off the page image, the OCR misreading it as "Rue
  An9le") maps "Hue Angle" ranges to an "Intended HLS Color": "31 - 90
  Magenta", "91 - 150 Red", "151 - 210 Yellow", "211 - 270 Green", "271 - 330
  Cyan", "331 - 30 Blue". Read by the centres of those ranges (**R**), blue
  is at 0, red at 120 and green at 240, as
  `baud-domain/src/sixel.rs` 127-129 expects when it says DEC's HLS "does not
  put red at 0". The table is a fallback for printers and gives no formula
  for the conversion (**R**); whether it is enough to build one is not
  decided here.
- **`docs/vt-coverage.md`'s C0 table has no `ENQ` row.** baud does nothing
  with `ENQ` in text: `c0()` has no `05` arm, so it is `Act::Consume`
  (graph.rs 650-660); baud keeps no answerback message and has no setting for
  one, and no row records it.
- **Text that items 10 and 11 make stale, left for the commit that builds
  them**, since this note changes no code:
  - `baud-core/src/graph.rs` 1243-1251 calls the 22 bytes "the owner's call"
    and says "`[[source: ecma48]]` puts them outside a command string, an
    error whose recovery the implementation picks", the attribution section
    2.1 corrects; its 1182-1194, "**Neither rings**", stops holding for a
    Sixel at item 1.
  - `baud-core/src/osc.rs` 98-99 calls the `BEL` in a DECRQSS `Pt` "the
    owner's open call".
  - `docs/vt-coverage.md`'s `dcs_passthrough` row lists the other C0, the
    Sixel `BEL` and `ENQ`, and `A0-FF` in a body as "Open, the owner's call,
    where the texts conflict", and its `0x07` BEL row says that inside a
    control string it "rings nothing".
  - The record of `e6a817f` lists the other C0, the `BEL` and `A0-FF` in a
    body as left "because the texts conflict", with ECMA-48's "error"
    attribution.
- **`P2` reads differently in the two DEC texts.** 070-9 §7.2 (PDF 918): 0
  "device default action", 1 "no action", 2 "set to background color"; the
  VT3xx chapter: "0 or 2 (default)" set the zero pixels to the background,
  1 leaves them (192-203). It is note S's counted, unbuilt fill, and not a
  data-stream byte.
- **Volume 1, chapter 2 of the VT330/VT340 manual**, which the Sixel chapter
  defers to for device control strings (95-96), is not pinned. It is the one
  text from DEC's own Sixel terminal that could speak to these bytes.

## 12. Red-first tests

For the builds (section 10.1), each red on `e6a817f` and green after, at every
read size and under both encodings where the byte is not 8859-1-only:

- A `BEL` between two data bytes, inside a number and inside a colour command
  of a Sixel counts one bell each and leaves the image the `BEL`-less program
  draws. It inverts the bell assertion in
  `a_bel_inside_a_sixel_is_payload_and_draws_nothing`.
- `!1 SPACE 0~` is 10 wide; `#1 SPACE ;2;100;0;0#1~` draws red; control:
  `~ SPACE ~` is 2 wide on both trees.
- Under ISO 8859-1, `FE` draws what `7E` draws, `! B1 B0 ~` what `!10~` does,
  and `FF` is ignored as `7F` is; under UTF-8 each is unchanged.
- `~~ ESC [5C` and, under ISO 8859-1, `~~ 85` each place a 2x6 image, and the
  escape sequence or the C1 still runs; control: the same body ended by ST.
- `!5%~` and each other unrecognised command between a repeat and its data
  draw one column; control: `!5~` draws five.
- After a green register 1, `#5;2;150;0;0~`, `#5;3;...~`, `#5;0;...~` and an
  HLS hue above 360 draw green; control: `#5;2;100;0;0~` draws red.
- `DCS q ~ ST` draws six pixels in the default colour; control: a defined
  register draws its own.
- With no raster attributes, and with `P1` 0, 1 and 5, a one-band image is
  twice as tall as with `"1;1`, and with `P1` 3 three times; `P1` 9 and
  `"1;1` are unchanged; `";` and `"0;0` draw as `"1;1`; a raster command
  after data changes nothing.
- Item 12: with `P1` 2, a one-band image is 27 device rows tall, its sixel
  rows 4 and 5 device pixels tall in turn (24 under alternative b, 30 under
  c); with `P1` 6, 9; control: `P1` 9 is 6, `P1` 0 is 12.
- `ESC P A4 F1 m ST` under ISO 8859-1 answers as `ESC P $ q m ST` does.
- Item 10: see section 5.3.
- Item 11: under ISO 8859-1, `DCS $ q ED ST` (`m` with its eighth bit) and
  `DCS $ q A0 F1 ST` (`SP q`, DECSCUSR, with it) answer as `m` and `SP q` do,
  and an `FF` in the `Pt` is dropped as a `DEL` is; under UTF-8 the bytes are
  part of a character and the answer is unchanged; control: `DCS $ q m ST`.

For question 1: `~~ SUB ~~` is one image 5 wide under option b, and under
option a the two `~` before the `SUB` are kept (with build 4) and the two
after are text. For question 2: `~~ CAN Z` places a 2x6 image under option b
and none under a, and draws `Z` under both.

## 13. Unmeasured

- The cost of every build here on the recordings' rates, and the stride
  ladder's movement under question 1's option b. Each change sits in
  `str_put`'s cold Sixel arm, the decoder, or one graph row; item 10's and the
  alternatives' ladders are measured (sections 5.3, 5.4). The tree's two Sixel
  streams end on ST and hold no SPACE, C0, `DEL` or byte in `80-FF`, no repeat
  followed by another command, no colour define that is not RGB or is above
  100 (`swirl.raw` has 233 defines, `img2sixel-96x72.six` 32), no selection of
  a register they never defined, and no data before their first colour
  command (**M**, `sixel-scan.py`), so builds 1 to 8 leave their images as
  they are; `sixel-cat` in `ingest2` replays `swirl.raw`.
- What xterm draws for a Sixel cut off mid-body (graphics.c, not pinned).
- What a VT330 or VT340 did with any of these bytes: no text from that
  terminal speaks to them, and none was run.

## 14. The questions

Each option carries a mark, conforms or departs, against each text, the
reason the texts give for it, and a cost. Each question goes to Jev first, and
to the owner if Jev does not reach 0.9 both times. The C0 in a DCS body and a
ratio that is not a whole number are not among them: one option conforms to
every text, and they are items 10 and 12 (sections 5 and 8).

1. **`SUB` inside a Sixel.** Section 7.3 holds the derivation. ECMA-48 puts
   `SUB` outside a command string (E1), says nothing of one arriving inside a
   string, and leaves error recovery to the implementation (E5); ctlseqs
   leaves error recovery to the implementation (C4); the VT3xx chapter is
   silent. So both options conform to ECMA-48 and ctlseqs. Neither STD 070
   chapter is shown to be the later one (the provenance above).
   - **a, end the string, as today.** Its reason is D3's: "For backward
     compatibility, and to minimize the effect of a lost String Terminator,
     the control codes CAN (1/8), SUB (1/10), ESC (1/11), or any C1 control
     will also terminate control strings." Conforms to STD 070 3.5.4.4,
     williams (608-610) and the VT510's Table 4-1, and to ECMA-48 and
     ctlseqs; departs from 070-9 §9.1. The rest of the body is drawn as text
     (S14). Cost: nothing; under build 4 the sixels before the `SUB` are
     kept, as williams' unhook (728) has them. SUB's error glyph stays note
     AF's Q-UNDEF.
   - **b, a blank sixel**, `SUB` read as `?` and the image going on. Its
     reason is 070-9 §9.1's: "this is to limit the affect[sic] of some
     communication line errors" ("affect" is the original's). ECMA-48
     §8.3.148's "SUB is used in the place of a character that has been found
     to be invalid or in error" (E6) describes what a blank sixel does with it
     (**R**). Conforms to 070-9 §9.1, and to ECMA-48 and ctlseqs; departs from
     STD 070 3.5.4.4, williams' anywhere row and the VT510. Cost: the
     anywhere rule sends `SUB` to ground before a node's own row is consulted
     (graph.rs 700-702), and `DcsStr` is shared by DECRQSS, Sixel and every
     other DCS, so a Sixel takes a body node of its own, excepted from that
     rule for `SUB`: a 23rd row of 256 edges under each encoding, 512 bytes
     more an encoding at the record's 2 bytes an edge, the `q` edge
     (graph.rs 871) leading to it, and the stride ladder moving by an amount
     not measured. `str_put` maps the byte to `3F`. A DECRQSS and every other
     DCS keep ending on `SUB`.
2. **What a Sixel that `CAN` cuts off keeps.** Section 7.4 holds the
   derivation. **It is a conflict only under the wider reading of ECMA-48
   §8.3.6's "preceding"** (**R**): under the narrower one, in which the data
   in error is the sixel the `CAN` arrived in, option b conforms to every text
   and would be work, as build 4 is for `ESC` and a C1. So what the owner
   decides is which reading baud takes. The string ends under both options,
   as every text says; the question is the sixels received before the `CAN`.
   STD 070-3 3.5.4.4 and the VT510's Table 4-1 say the string ends and not
   what becomes of it, ctlseqs leaves error recovery to the implementation
   (C4), and the VT3xx chapter is silent, so each conforms under both options
   (**R**). xterm's maintainer reads STD 070 as narrowing "preceding" to "the
   current control sequence" (charproc.c 3627-3629), practice, which does not
   say what that sequence already drew.
   - **a, discard them, as today.** Its reason is ECMA-48 §8.3.6, "the data
     preceding it in the data stream is in error. As a result, this data
     shall be ignored", read as reaching back over the string. Conforms to
     ECMA-48 under that reading; departs from williams (501-503, 728-729) and
     from 070-9's printing in received order and its exit from Sixel mode (§5,
     §9.1, §11.3, **R**). Cost: nothing. A Sixel that `ESC` or a C1 cuts off
     then keeps its sixels (build 4) where one that `CAN` cuts off does not.
   - **b, keep them**: decode and place what was received, and move the cursor
     as at ST. Its reason is williams' "Cancelling a control string does not
     undo these effects" (501-503) and its unhook, which lets the handler
     "finish neatly" (728-729). Conforms to williams and to 070-9 (**R**);
     departs from ECMA-48 §8.3.6 under the wider reading, and conforms under
     the narrower one (**R**). Cost: with build 4, the `CAN` edge out of the
     Sixel body joins the act that ends a Sixel before the byte runs: 2 edges,
     `CAN` in `DcsStr` under each encoding, and since `DcsStr` is shared, the
     act tests the string's kind on that cold path, or the Sixel body node of
     question 1's option b carries the edge. The general cancel question for
     an OSC or another DCS stays where the record left it.
