# AE. Code extension: the four G slots, GL and GR, and the shifts

**Status: built 2026-10-19, but for Q12, Q13's SCORC half,
Q15, Q17 and Q18; researched 2026-10-08.** Every question was weighed again
with baud's DA answer (it keeps `?6c`) and the rule that a protocol is
supported fully: GR on G2 decided and any number of SO/SI; leans taken,
each consistent since 2026-10-08: Q2 DEC Supplemental, Q4.1 the same in both
encodings, Q6 070's cancel rule, Q6.2 SO acts, Q7 strip bit 8, Q9 ungated,
Q10 the per-area reading, Q5's LS1R-LS3R state only, Q14 the UPSS. Q3 is
settled by the same rule: all six listed 96-sets are built. Sections 4-9
below are the research as written.

- **Open** (the routing of 2026-10-08): Q1, Q2
  (with Q4.1), Q8, Q9, Q11, Q12 and Q17. In each, every option departs from
  some text (for Q1 under the 8bit-level1 reading of `ESC % @`, and for Q9
  under the VT102 reading of DA, each reading itself a choice to make), or
  the option that conforms collides with a practice baud already follows.
  Since then the 2026-10-19 build decided Q1 and took leans on Q2 with Q4.1 and on Q9
  (above), Q8 is decided at AE8 (§10), and Q11 leans to slots (2026-10-09), still to be
  confirmed. Q11, Q12 and Q17 stay open.
- **Recommended:** the premise P and Q3-Q7, Q10, Q13-Q16 and Q18. A
  recommendation that departs from a text is still decided as a departure,
  so Q3, Q6 (its two sub-points), Q10, Q13, Q14 and Q18 go on for decision
  (§9).
- **Buildable before any answer:** section 9 says what.

Evidence is tagged as in the other notes:

- **source:** a manifest key with a clause, page or line, or file:line;
- **inferred;**
- **unknown.**

**What the code citations rest on.**

- Every behaviour of baud is read from source at `d894ebb`.
- Between `d894ebb` and `8cf753d` (HEAD when this was written), the only
  change under any `src/` is `baud-domain/src/store.rs`. `8cf753d`'s only code
  change is the coverage-table reader in `enumerated.rs`; it also touched
  the record.
- Nothing was built or run. Every behaviour of baud, xterm and foot here is
  traced, not observed.
- **What moved while this was written.** D1 and D2 (§8) were fixed in
  `bdd59c6`, with the two tests this note names (test 17); the description of
  each as found stays, because it is what the reading found at `d894ebb`. The
  change to `baud-core/tests/coverage.rs` is still uncommitted:
  it turns the DEC Special Graphics row red until it is decided which
  codepoint `0x5F` stores. The new manifest keys below are committed with this
  note.

**How the sources were read.**

- **[[source: ecma35]].** Read with pypdf 6.19.0, cross-checked against
  pdfminer.six run page by page and against a scratch LZW decoder. The streams
  are LZW; `scripts/extract-ecma48.py` decodes Flate only, and its positive
  control missed on this file. Printed page = PDF page - 10.
- **[[source: ecma48]].** Read with pdfminer.six; pypdf letter-spaces it.
  Printed page = PDF page - 14.
- **Controls for both.** Every quote was found again after stripping
  non-alphanumerics from both sides.
  - Positive controls hit: "LOCKING-SHIFT TWO", "SINGLE-SHIFT TWO", "SELECT
    GRAPHIC RENDITION", "graphic character".
  - The negative control "nonexistentxyzzy" missed.
- **[[source: iso10646-fcd]].** Read with pypdf. FCD clause 11 is clause 12 in
  the published edition.
- **[[source: dec-std-070]]: the extraction.**
  - The PDF matches the manifest sha256 `d30c2ace…`. Text was extracted with
    poppler `pdftotext -layout`.
  - The text layer is OCR and misreads: "a-bit" for "8-bit", `parv[O] & Ox7F`.
    So **[img]** marks a quote read off the rendered page, and **[text]** a
    quote from the text layer only.
  - The page renders were made in an untracked scratch directory. The pinned
    hash reproduces every page.
- **[[source: dec-std-070]]: chapter status.**

  | Chapter | Revision | Status |
  |---|---|---|
  | 070-3 | Rev A, 14-Apr-1989 | APPROVED |
  | 070-4 | Rev A, 04-Oct-1990 | APPROVED |
  | 070-5 | Rev AX12, 29-Jun-1990 | "FOR REVIEW ONLY" |
  | 070-D | Rev AX11, 18-Mar-1985 | "FOR REVIEW ONLY" |

  070-D is "Documented Exceptions", whose features "are not required for
  conformance" (D.1, quoted in section 1).
- **DEC HTML pages** ([[source: dec-special-graphics]], [[source: vt220-rm-ch4]],
  [[source: vt510-rm-ch4]], `vt510-*`).
  - Tag-stripped and normalised.
  - The positive control "Table 4-10" hit; the negative control missed.
  - The `dec-special-graphics` manifest title names Table 2-4, but the file is
    the whole of chapter 2, so a citation of its Table 2-3 resolves.
- **ctlseqs.** Line numbers are those of `xterm-ctlseqs.txt`.
- **Practice evidence only:**
  - xterm patch 411 at `9489b20`: [[source: xterm-reset]] (charproc.c),
    [[source: xterm-charsets-c]], [[source: xterm-cursor-c]],
    [[source: xterm-ptydata-c]], [[source: xterm-ptyx-h]],
    [[source: xterm-vtprstbl-c]];
  - foot 1.21.0: [[source: foot-vt-ris]] (vt.c), [[source: foot-csi]] (csi.c),
    [[source: foot-term-reset]] (terminal.c),
    [[source: foot-terminal-h-1-21]] (terminal.h).
- **Keys pinned with this note.** The following keys enter `MANIFEST.toml`
  in the same commit, and `scripts/check-sources.sh` passes with them.
  - `vt510-decrc`, `vt510-ris`, `vt510-ss`, `vt510-ls`, `vt510-scs`,
    `vt510-deccir`, `vt510-decaupss`, `vt510-decrqupss`, `vt510-decscl`,
    `vt510-s7c1t`, `vt510-s8c1t`, `vt510-ch7`;
  - `xterm-cursor-c`, `xterm-ptydata-c`, `xterm-ptyx-h`, `xterm-vtprstbl-c`,
    `xterm-xterm-h`.

---

## 1. The governing texts

### ECMA-35 [[source: ecma35]]

> **4.18 to invoke** (p. 5): "To cause a designated set of characters to be represented by the prescribed bit combinations whenever those bit combinations occur."

> **NOTE 1** (p. 1): "The coded character set specified in ISO/IEC 10646-1 has a different structure not in accordance with this Standard."

> **6.3.1** (p. 7): "In a 94-character set no character shall be allocated to positions 02/00 and 07/15."

> **7.3** (p. 14): "The designation of a graphic character set as a G0, G1, G2, or G3 code element shall invoke that set if the code element already has a shift status (8.3.3 and 9.3.3); otherwise the use of a corresponding shift function shall invoke that set."

Table 2 (7.3, p. 14):

| Name | Acronym | Usage | Coded representation |
|---|---|---|---|
| SHIFT-IN | SI | 7 | 00/15 |
| SHIFT-OUT | SO | 7 | 00/14 |
| LOCKING-SHIFT ZERO | LS0 | 8 | 00/15 |
| LOCKING-SHIFT ONE | LS1 | 8 | 00/14 |
| LOCKING-SHIFT TWO | LS2 | 7/8 | ESC 06/14 (`ESC n`) |
| LOCKING-SHIFT THREE | LS3 | 7/8 | ESC 06/15 (`ESC o`) |
| SINGLE-SHIFT TWO | SS2 | 7/8 | "ESC 04/14 or 08/14" |
| SINGLE-SHIFT THREE | SS3 | 7/8 | "ESC 04/15 or 08/15" |
| LOCKING-SHIFT ONE RIGHT | LS1R | 8 | ESC 07/14 (`ESC ~`) |
| LOCKING-SHIFT TWO RIGHT | LS2R | 8 | ESC 07/13 (`ESC }`) |
| LOCKING-SHIFT THREE RIGHT | LS3R | 8 | ESC 07/12 (`ESC \|`) |

> **8.1** (pp. 15-16): "− columns 00 and 01 shall be the CL area, − columns 02 to 07 shall be the GL area, − columns 08 and 09 shall be the CR area, − columns 10 to 15 shall be the GR area." … "− GR, either a set of 94 or 94n graphic characters (leaving positions 10/00 and 15/15 unused), or a set of 96 or 96 n graphic characters, or unused."

> **8.3.1** (p. 17): "When a locking-shift function LS0, LS1, LS2, or LS3 invokes a 94-character or 94 n-character set, that set shall be invoked into positions 02/01 to 07/14, SPACE shall be invoked into position 02/00, and DELETE shall be invoked into position 07/15. When a locking-shift function LS1, LS2, or LS3 invokes a 96-character or 96n-character set, that set shall be invoked into positions 02/00 to 07/15. The characters SPACE and DELETE cannot be represented while such a set is in an invoked state."

> **8.3.2** (p. 17): "Each of the three locking-shift functions LS1R, LS2R, and LS3R (7.3) shall invoke in the GR area the specific graphic character set that has been identified as the respective code element G1, G2, or G3. … When a locking-shift function LS1R, LS2R, or LS3R invokes a 94-character or a 94 n-character set, that set shall be invoked into positions 10/01 to 15/14, and positions 10/00 and 15/15 shall not be used. When a locking-shift function LS1R, LS2R, or LS3R invokes a 96-character or a 96 n-character set, that set shall be invoked into positions 10/00 to 15/15."

> **8.3.3** (p. 17): "In an 8-bit code only one of G0 to G3 can have a GL shift status, and only one of G1 to G3 can have a GR shift status, at each specific point within a CC-data-element." **NOTE 25**: "The same code element G1, G2, or G3 may be invoked in GL and GR simultaneously, but this is not recommended." Also: "At the beginning of information interchange either locking-shift functions shall be used to establish the initial GL and GR shift status, or the shift status at that point may be undefined (see 10.1)."

> **8.3.4** (p. 19): "When one of the code elements G0 to G3 has GL or GR shift status, the occurrence of the corresponding locking-shift function shall have no effect. The occurrence of a locking-shift function shall have no effect on the meaning of the following bit combinations: − those representing control characters in columns 00 and 01, or columns 08 and 09; − those included in any escape sequence; − those representing the character invoked by an SS2 or SS3 control function (8.4)."

> **8.4** (p. 19): "If either of the single-shift functions SS2 or SS3 are included in a code (10.1), then either the GL area or the GR area shall be identified as the single-shift area. This identification shall be stated in the specification of the version of the code (10.1).
> Each of the single-shift functions SS2 and SS3 shall invoke a single character from the specific character set that has been identified as the respective code element G2 or G3. …
> When such a shift function occurs, the immediately following bit combination (or sequence of n bit combinations for a multiple-byte set) in the CC-data-element shall represent the corresponding character from the set that is designated as the code element G2 or G3 respectively. The shift status that was in effect immediately before the occurrence of the single-shift function shall continue in effect immediately after the bit combination (or sequence) that represents the invoked single graphic character.
> When a single-shift function SS2 or SS3 invokes a character from a 94-character or a 94 n-character set the immediately following one or n bit combinations respectively shall be in the range from 02/01 to 07/14 if GL is the single-shift area, and shall be in the range from 10/01 to 15/14 if GR is the single-shift area. When a single-shift function SS2 or SS3 invokes a character from a 96-character or a 96 n-character set the immediately following one or n bit combinations respectively shall be in the range from 02/00 to 07/15 if GL is the single-shift area, and shall be in the range from 10/00 to 15/15 if GR is the single-shift area."

> **9.3.2** (p. 23): "When used in a 7-bit code LS1R, LS2R, and LS3R shall have the same effects as SO, LS2, and LS3 respectively."

> **10.1** (p. 24): "A version may optionally include identification of the initial conditions applicable at the beginning of information interchange, in particular: − the identification of all graphic character sets and control function sets that are to be used, − the initial designation status of such sets, and − the shift status."

> **10.3.1** Level 1, 8-bit (p. 25): "− a C0 code element, − a G0 code element having GL shift status, − the characters SPACE and DELETE, − optionally a C1 code element in the CR area, − a G1 code element having GR shift status. … The use of the locking-shift functions LS0 and LS1R is not required." Level 2 (p. 25) adds "one or both of the shift functions SS2 and SS3, with GL identified as the single-shift area."

> **10.3.2** (p. 26): "In a code that is according to Levels 2, 3, or 4, except that GR is identified as the single-shift area, then the corresponding levels are identified as Level 2A, 3A, and 4A."

> **14.3.2** (p. 35): "A 94-character graphic set or a 94 n-character graphic set may be designated as a G0, G1, G2, or G3 code element of a specific code. Any type of graphic character set may be designated as a G1, G2, or G3 code element." "When an identified set is designated as a particular code element then: − if that code element already has a shift status, the set shall be immediately invoked;"

Table 6 (p. 35) gives the designators:

- 94-sets: GZD4 `(` 02/08, G1D4 `)` 02/09, G2D4 `*` 02/10, G3D4 `+` 02/11.
- 96-sets: G1D6 `-` 02/13, G2D6 `.` 02/14, G3D6 `/` 02/15.
- 02/12 is "(reserved for future standardisation)" (Table 3.b, p. 30).

> **NOTE 44** (14.1, p. 34): "Such allocations of F (and associated I, if any) are taken from a separate series of Final bytes for each of the six types of sets indicated above. In consequence a designation function can only designate a set of suitable type as a particular code element."

Table 7 (pp. 38-41), ANNOUNCE CODE STRUCTURE `ESC 02/00 F`, where facility n has
F = 04/00 + n:

- Facilities 12-14 are `ESC SP L/M/N`. Row 14 reads "14 04/14 Level 3 of
  ECMA-43 shall be used.", and rows 12 and 13 read the same with Levels 1 and
  2.
- NOTE 52 reserves facility 15 (`ESC SP O`).
- Facility 28: "In an 8-bit code any single character invoked by SS2 or SS3
  shall be represented by bit combination(s) from columns 10 to 15."

> **15.4.1** (p. 41): "A code-identification function is provided to designate and invoke an identified coding system different from that of this Standard, not necessarily a character code."

> **15.4.2** (p. 42): "DOCS with Final Byte 04/00, without I byte, shall designate the coding system of this Standard. It is intended for use by other coding systems for returning to this coding system. It shall restore the state of the coding system to that at the time of invocation of the other coding system, that is the state established by announcer functions, and the designation and invocation state of graphic character and control character sets."

> **NOTE A.3** (p. 45): "− ISO 8859-1 (includes G0 and G1 code elements only) {iso standard 2022 transfer-syntaxes (0) 8bit-level1 (11) 6 100}"

> **Annex D.2** (p. 51; Annex D is informative, "Main differences between the 5th edition (1993) and the present edition"): "The coded representation of graphic characters invoked by SS2 and SS3 shall be taken from the same range of column numbers within any version of an 8-bit code, i.e. either from columns 02 to 07, or from columns 10 to 15, but not from both (8.4)."

### ECMA-48 [[source: ecma48]]

> **8.3.141 / 8.3.142** (p. 70): "Representation: 08/14 or ESC 04/14", "Representation: 08/15 or ESC 04/15"

> **8.3.105 RIS** (p. 55): "RIS causes a device to be reset to its initial state, i.e. the state it has after it is made operational."

> **9** (pp. 74-75): "However, when data containing these control functions are transformed from a 7-bit to an 8-bit coded representation or vice versa, the transformation algorithm specified in Standard ECMA-35 may produce results which are formally in disagreement with this Standard. In order to make allowance for such unintended but unavoidable deviations, the formal rules are extended in the manner described below.
> In an 8-bit code, the bit combinations of columns 10 to 15 are permitted to represent − Parameter Bytes, Intermediate Bytes, and Final Bytes of a control sequence − the contents of the command string or the character string as part of a control string (see 5.6) − the operand of a single-shift control function. In these situations, the bit combinations in the range 10/00 to 15/14 shall have the same meanings as the corresponding bit combinations in the range 02/00 to 07/14.
> In a 7-bit code, the control functions SHIFT-OUT (SO) and SHIFT-IN (SI) are permitted to occur − between the CONTROL SEQUENCE INTRODUCER (CSI) and the Final Byte of a control sequence, − between the opening delimiter of a control string and the STRING TERMINATOR (ST), − between a single-shift control function and its operand. SO and SI have no effect on the interpretation of a control sequence, a control string or the operand of a single-shift control function, but they may, indeed, affect the meanings of bit combinations following in the data stream."

Clause 9 is a tolerance for transformation artifacts. It covers only "these
situations" and stops at 15/14. It gives no meaning to a GR byte displayed
outside them.

### ISO/IEC 10646 FCD [[source: iso10646-fcd]]

> **FCD 11** (p. 25): "Code extension control functions for the ISO/IEC 2022 code extension techniques (such as designation escape sequences, single shift, and locking shift) shall not be used with this coded character set."

### ctlseqs [[source: ctlseqs]]

> 56-65: "a C1 control can be mistaken for badly-formed UTF-8 when the terminal runs in UTF-8 mode because C1 controls are valid continuation bytes … It is not possible to use a C1 control obtained from decoding the UTF-8 text, because that would require reprocessing the data."

> 157-181: IND, NEL, HTS, RI, SS2 and SS3 are among "pairs of 7-bit and 8-bit control characters" that "are equivalent". 175-177: "Single Shift Select of G2 Character Set (SS2 is 0x8e), VT220. This affects next character only." 179-181: the same for G3, with SS3 as 0x8f.

> 266-268: "This invokes the G0 character set (the default) as GL."

> 296-300: "ESC SP L  Set ANSI conformance level 1, ECMA-43." … "ESC SP M  Set ANSI conformance level 2, ECMA-43." … "ESC SP N  Set ANSI conformance level 3, ECMA-43."

> 312: "ESC % @   Select default character set.  That is ISO 8859-1 (ISO 2022)."

> 328-330: "The ASCII and DEC Supplemental character sets make up the two halves of the Multinational Character set, initially mapped to GL and GR."

> 380-382: "These controls apply only to 96-character sets. Unlike the 94-character sets, these can have different values than ASCII space and DEL for the mapping of 0x20 and 0x7f." 384-389 list the 96-set finals A, B, F, H, L and M.

> 419-427: "ESC n Invoke the G2 Character Set as GL (LS2)." / "ESC o Invoke the G3 Character Set as GL (LS3)." / "ESC | Invoke the G3 Character Set as GR (LS3R)." / "ESC } Invoke the G2 Character Set as GR (LS2R)." / "ESC ~ Invoke the G1 Character Set as GR (LS1R), VT100."

### williams [[source: williams]]

> ground: "If a 94-character set is mapped into GL, 20 will cause a space to be displayed, and 7F will be ignored. When a 96-character set is mapped into GL, both 20 and 7F may cause a character to be displayed."

### DEC STD 070 [[source: dec-std-070]]

**070-3, Code Extension Layer (APPROVED).**

> §3.1.3, p. 3-7 [img], headed "8-BIT INTERFACE ARCHITECTURE EXTENSION": "Support for 96-character graphic character sets, ISO Latin-1 Supplemental, and the User Preference Supplemental Set (UPSS) was introduced on the VT300 family of Digital terminals. The 8-bit Interface Architecture Extension is required for Level 3 conformance to the Character Cell Display service class, and is strongly recommended (but not mandatory) for Level 2. … The rest of this section for DEC STD 070 assumes the 8-bit Interface Architecture Extension is present."

> §3.4, p. 3-17 [img]: "When 94-character graphic character sets are used, the following characters are not part of either the control or graphic sets in a 7-bit or 8-bit code, and therefore have special processing as described below."
> §3.4.2: "When received by a terminal, the Delete character is treated as an unimplemented control character and shall be ignored."
> §3.4.3: "The character code 10/0 in 8-bit environments is treated as an unassigned graphic character code, and will cause the error character (usually imaged as a REVERSE QUESTION MARK) to be displayed on conforming devices. Within escape sequences and control sequences, the high order bit of this character will be ignored, and it will be treated as syntactically equivalent to Space (2/0)."
> §3.4.4: "The character code 15/15 in 8-bit environments is treated as an unimplemented control code and shall be ignored."

> §3.6.1, p. 3-59 [img]: "Single shifts cause a G-set to be invoked only for the occurrence of the single character code following the single shift control in the code stream, after which the table reverts to its previous state before the shift occurred."

> §3.6.1.1, p. 3-61 [img]: "Conforming terminal products will provide the following locking shift functions in Level 1 operation." The table lists LS0, LS1, LS2 and LS3, invoking G0-G3 into GL. "Conforming terminals will provide the following additional locking shift functions in Level 2 operation." That table lists LS1R, LS2R and LS3R, invoking G1-G3 into GR.
> The same page: "The following bit combinations are not affected by the use of the locking shifts. … c. Those corresponding to positions 10/0 and 15/15 when a 94-character set is invoked into GR … e. The bit combination following a single shift function (SS2 or SS3)"

> §3.6.1.2, p. 3-62 [img]: "Conforming terminal products will provide the following single shift functions in both 7-bit and 8-bit environments. SS2 Single Shift Two G2 GL / SS3 Single Shift Three G3 GL"

> §3.6.3, p. 3-62 [img]: "the character set used can be selected by the user as a SETUP option or by host control function in response to a user request. The UPSS was introduced to facilitate migration from DEC-MCS to ISO Latin-1."

> §3.6.4.1, p. 3-63 [img], headed "Level 1": "In Level 1 operation, the default character set designations will be ASCII in G0, G1, G2, and G3. Furthermore, the G0 set (ASCII) will be invoked into the GL position of the In Use Table. Level 1 operation is 7-bits only."

> §3.6.4.2, p. 3-63 [img], headed "Level 2 (without 8-bit Interface Architecture Extension)": "In Level 2 operation, the default character set designations will be ASCII in G0 and G1, and the DEC Supplemental graphics set in G2 and G3. Furthermore, the G0 set (ASCII) will be invoked into the GL position of the In Use Table, and the G2 set (DEC Supplemental graphics) will be invoked into the GR position of the In Use Table."

> §3.6.4.3, p. 3-64 [img], headed "Level 3 (or Level 2 with 8-bit Interface Architecture Extension)": "In Level 3 operation, the default character set designations will be ASCII in G0 and G1, and the User Preference Supplemental Set (UPSS) in G2 and G3. Furthermore, the G0 set (ASCII) will be invoked into the GL position of the In Use Table, and the G2 set (UPSS, typically DEC Supplemental or ISO Latin-1 supplemental) will be invoked into the GR position of the In Use Table." Its second paragraph: "Level 3 (or Level 2 with the 8-bit Interface Architecture Extension) also recognizes the "Announce Subset Of Code Extension Facilities" defined in ISO 4873 which re-initialize the designated and invoked character sets as described later in this chapter."

> p. 3-63 [img]: "The designating sequence previously used for DEC Supplemental was changed to designate the UPSS" … "Support for the UPSS is part of the 8-bit Interface Architecture Extension to Level 2, and required for conformance to Level 3. As a minimum, the UPSS must be selectable between DEC Supplemental and ISO Latin-1 supplemental."

> §3.8.1, pp. 3-68 to 3-70 [img], "ANNOUNCE SUBSET OF CODE EXTENSION FACILITIES", "Levels: 2X, 3 (8-bit Interface Architecture Extension)", Format `ESC SP <final>`: "L 4/12 | Level 1 of ISO 4873 / M 4/13 | Level 2 of ISO 4873 / N 4/14 | Level 3 of ISO 4873". "Upon receipt of a Level 1, 2, or 3 announcer sequence, the terminal will make the following designations and invocations: Level 1 ASCII is designated into G0 ISO Latin Alphabet Nr 1 supplemental is designated into G1 G0 is invoked into GL G1 is invoked into GR Level 2 [the same four lines] Level 3 ASCII is designated into G0 G0 is invoked into GL".
> Its NOTES: "Conformance to a subset of ISO 4873 code extension and interchange is NOT equivalent to the Digital conformance levels for Character Cell Display terminals." "Any hard or soft terminal reset (receipt of RIS, DECSTR, or DECSCL, or issuing the RECALL function from Set-Up), will cause the Terminal to reset the "Default Designations And Invocations" as described previously to conform with DEC STD 169-0."
> Its algorithm (p. 3-69) wraps each of `announce_level_1`, `announce_level_2` and `announce_level_3` in "if ( (conformance_level > 2) || (level_2_extensions[8bit_ia]) )", so below Level 3, without the extension, an announcer does nothing.

> pp. 3-80/3-81 [img] (SS3; SS2 on p. 3-80 is the same with G2; both "Levels: 1, 2, 3"): "The SS3 control causes the next character in the received data stream, which must be in the range 2/0 through 7/15 or 10/0 through 15/15, to be rendered according to the current contents of the G3 set. If the next character in the received data stream is not a graphic character in this range, the single shift function is ignored and the received character is processed as if the single shift had not been received.
> The only exception to this rule is that a single Shift Out (0/14) or Shift In (0/15) control character may occur between the Single Shift and the subsequent graphic character. The Shift Out or Shift In character will be interpreted normally, but does not cancel the Single Shift."

**070-4, Terminal Management (APPROVED).**

> p. 14 [text]: "upss = DEC_SUPPLEMENTAL_G; /* NVM */"

> p. 15 [img]: "single_shift = NO_SINGLE_SHIFT;" and "case LEVEL_1: in_use_table.gr = ASCII_G;"

> p. 31 [img] (DECSTR): "In Use Table (gl, gr) | set to current level defaults / Designated Sets | set to current level defaults / Cursor Save Buffer | set to current level defaults / … UPSS | DEC Supplemental (NVM if present)"

> p. 39 [text]: "Removed Reset To Initial State as a required function, and placed it in Appendix D Documented Exceptions. It is not intended for use by conforming software. The RIS algorithm is now included in this section under the heading "Device Initialization"."

**070-5, Character Cell Display ("FOR REVIEW ONLY").**

> pp. 5-140/5-141 [img]: "display[line][active_position.column].code = parv[0] & 0x7F; … else if (single_shift == SS2) … = designated_graphic_sets[2];"

> §5.13.1, p. 5-186 [img]: "The state which must be saved in Level 1 operation includes the Current Rendition value …, the current value of Origin Mode, the character sets designated to G0, G1, G2, and G3, and the character set most recently invoked into the left hand side (GL) of the In Use Table. In Level 2 operation, all of this state must also be saved, as well as the character set most recently invoked into the right hand side (GR) of the In Use Table."

> p. 5-187 [img]: "3. On initialization or reset the values of the save buffer are cleared to the following state: position.line = 1; position.column = 1; … origin_mode = ABSOLUTE; left = in_use_table.gl; right = in_use_table.gr … 4. Execution of Soft Terminal Reset (DECSTR) also causes the Save Buffer to be set to the default values as given above."

> pp. 5-200 to 5-202 [img] (DECCIR): "The data of a device control string for a Cursor Information Report consists of a string of parameters containing the information saved through a Save Cursor (DECSC) command" … "bit 4: auto-wrap pending (1 if pending) bit 3: SS3 pending (1 if SS3 received) bit 2: SS2 pending (1 if SS2 received)"

**070-D, Documented Exceptions ("FOR REVIEW ONLY"; guidelines, not requirements).**

> Abstract, p. D-1 [text]: "As exceptions, they are not required of conforming hardware implementations, and will not be used by conforming software."
> D.1, p. D-4 [text]: "The specifications contained in this appendix comprise a registry of exceptions to the architectural structure, which are intended to serve as guidelines to hardware implementors. These features are not required for conformance, and they will not be tested as a part of the certification process."

> p. D-7 [img] (RIS): "SINGLE_SHIFT:= NONE;" and, at Level 2, "IN_USE_TABLE.GR:= SUPPLEMENTAL_G;"
> p. D-14 [img]: "Furthermore, the state of the Last Column Flag should be saved when a Save Cursor operation is performed, and restored when a Restore Cursor operation is performed."

### VT220

> [[source: dec-special-graphics]] §2.3.2: "By ANSI standards, positions 10/0 and 15/15 are not used."
> §2.4.1: "By factory default, when you power up or reset the terminal, the DEC multinational character set ( Table 2-3 ) is mapped into the 8-bit code matrix (columns 0 through 15)."

> [[source: vt220-rm-ch4]] §4.4: "Single shifts SS2 and SS3 are active for only the next single graphic character." "The default mapping in VT200 mode is ASCII graphics in GL and DEC supplemental graphics in GR (DEC multinational)."
> Table 4-5: "LS2R … ESC } Invoke G2 into GR (VT200 mode only). (default)". The same table gives LS1R, LS2, LS3 and LS3R as "(VT200 mode only)", for example "LS2 (lock shift G2) 1/11 6/14 ESC n Invoke G2 into GL (VT200 mode only)."
> Figure 4-2, "Locking and Single-Shift Commands (VT100 Mode)" [img], read off the figure the page links (`ma-0280a-82.png`, a copy beside the research files; the figure is not pinned, only the HTML is). It shows SI, SO, SS2 and SS3, and its footnote on G2 and G3 reads: "Always ASCII or National Replacement Character sets in VT100 mode as selected in Set-Up."
> §4.4.4: "All single shifts are active for only the next single graphic character. The terminal returns the previous character set after displaying a single graphic character."
> §4.4.4.1: "SS2 is an 8-bit control character (8/14) that invokes G2 into GL for the next graphic character."

> [[source: vt220-rm-ch4]] §4.7: "Saves the following in terminal memory. cursor position / graphic rendition / character set shift state / state of wrap flag / state of origin mode / state of selective erase" … "If none of these characteristics were saved, the cursor moves to home position; origin mode is reset; no character attributes are assigned; and the default character set mapping is established."
> Table 4-10 (DECSTR): "Save Cursor State* … Cursor position Home / Character sets VT100 or VT200 defaults (as appropriate) / … Character shift (G0 to GL, G2 to GR, no shifts) | Power-up defaults" "* Applies only to later restore cursor commands (DECRC)."
> §4.18.2 (RIS): "Sets all character sets to the default."

### VT510

> [[source: vt510-rm-ch4]] §4.4.1: "When you turn on or reset the terminal, you automatically select the ASCII character set in GL and the current user-preferred character set in GR."

> [[source: vt510-decsc]]: "Cursor position / Character attributes set by the SGR command / Character sets (G0, G1, G2, or G3) currently in GL and GR / Wrap flag (autowrap or no autowrap) / State of origin mode (DECOM) / Selective erase attribute / Any single shift 2 (SS2) or single shift 3 (SS3) functions sent"

> [[source: vt510-decrc]]: "If nothing was saved by DECSC, then DECRC performs the following actions: Moves the cursor to the home position (upper left of screen). Resets origin mode (DECOM). Turns all character attributes off (normal setting). Maps the ASCII character set into GL, and the DEC Supplemental Graphic set into GR."

> [[source: vt510-decstr]] Table 5-9: "All character sets | G0, G1, G2, G3, GL, GR | Default settings." "Save cursor state | DECSC | Home position." "Assign user preference supplemental set | DECAUPSS | Set selected in Set-Up."

> [[source: vt510-ris]]: "Selects the default character sets (ASCII in GL, and DEC Supplemental Graphic in GR)."

> [[source: vt510-decaupss]]: "Default: DEC Supplemental Graphic"

> [[source: vt510-ss]]: "A single shift maps the G2 or G3 set into GL. The character set is active for only one character, then the terminal returns to the previous character set in GL." Its table: "Maps G2 into GL for the next character."

> [[source: vt510-ls]] Table 5-15: "The following locking shift functions are available only in VT Level 4 mode." It covers LS1R, LS2, LS2R, LS3 and LS3R.

> [[source: vt510-deccir]]: "The cursor information in a DECCIR sequence is the same information saved through a save cursor (DECSC) command." Sflag: "4 Autowrap 1 = autowrap pending … 3 Single shift 3 (SS3) setting 1 = G3 is mapped into GL for the next typed character only …"

> [[source: vt510-ch7]] §7.2.5, "Characters Not in a Character Set", whose table's last column is "Action on Receipt": "SP SPACE 32 … Treated as space / DEL DELETE 127 … Ignored (unless in CRM) / Reserved 160 None Error character generated / Reserved 255 None Ignored (unless in CRM)". Its notes: "If a 96-character character set is invoked into GL, then the receipt of codes 32 and 127 are part of that character set … If a 96-character character set is invoked into GR, the receipt of codes 160 and 255 are part of that character set"
> §7.3: "In VT100 mode, VT52 mode, or when 7-bit NRCS characters is selected (through Set-Up or DECNRCM), only ASCII, NRC sets, and DEC Special Graphic characters are available."

> [[source: vt510-scs]]: both its 94-character and its 96-character tables end "< User-preferred Supplemental".

> [[source: vt510-decscl]]: "Level 1 … VT100 mode"; "Level 4 includes levels 2 and 3."

---

## 2. Required behaviour, one row per input

"Counted" means `Esc::parse` (esc.rs:47-60) returns `Unsupported` and
`unsupported_total` goes up by 1 (write.rs:1793), with nothing else changed.
Both fast paths reach that count:

- `fast_esc`'s bare arm (write.rs:2220-2224);
- the graph's `Act::EscFinal` (write.rs:664-668).

| Input | Text that requires it | baud today | Change |
|---|---|---|---|
| `ESC N` (SS2) | ECMA-35 Table 2, 8.4; ECMA-48 8.3.141; ctlseqs 175-177; 070-3 p. 3-80; vt220-rm-ch4 §4.4.4.1; vt510-ss | Counted | The next graphic character is drawn from G2, after which GL (and GR) apply as before. What cancels the shift is Q6; UTF-8 is P and Q4. Entered only from a bare `ESC` and from Latin-1 `8E` (§5) |
| `ESC O` (SS3) | The same, for G3; ctlseqs 179-181 | Counted | The same, from G3 |
| `ESC <I…> N`, `ESC <I…> O`, e.g. `ESC SP N` | ctlseqs 300; ECMA-35 Table 7 facilities 14 (`N`) and 15 (`O`, reserved by NOTE 52) | Counted. Unsplit, `fast_esc`'s intermediate arm (write.rs:2195-2213) calls `esc_final`; split, the graph's `EscFinal` does. Both reach `esc_final`'s intermediate branch, where `ESC SP` counts every final but `F` and `G` (write.rs:1717-1747) and `ESC ( N` designates as any `ESC ( F` does | Unchanged at every read split. **Not a single shift**: the final byte follows an intermediate (§5; test 6). What `ESC SP N` itself should do is the next row's |
| `ESC SP L`, `ESC SP M`, `ESC SP N` (Announce Subset of Code Extension Facilities) | 070-3 §3.8.1 (pp. 3-68 to 3-70), the only receiver rule read: L and M designate ASCII into G0 and ISO Latin-1 Supplemental into G1 and invoke G0 into GL and G1 into GR; N designates ASCII into G0 and invokes G0 into GL; all only at "Levels: 2X, 3". ECMA-35 Table 7 facilities 12-14 and ctlseqs 296-300 name them and give no receiver rule | Since 2026-10-19 (lane b104) built at Levels 3 and 4, which DECSCL now selects, Level 4 at power-up: the designations and invocations §3.8.1 gives; counted at Levels 1 and 2. It counted at every level before, departing from §3.8.1 at Level 3 | Q18, answered (a). Whatever it does runs in `esc_final`'s intermediate branch, which both paths reach, so it is the same at every split |
| `ESC n` (LS2) | ECMA-35 8.3.1, Table 2 (7/8); ctlseqs 419 | Counted | `gl = G2`, then `invoke()` |
| `ESC o` (LS3) | ECMA-35 8.3.1; ctlseqs 421 | Counted | `gl = G3`, then `invoke()` |
| `ESC \|` (LS3R) | ECMA-35 8.3.2; ctlseqs 423 | Counted | `gr = G3`: state in both encodings, read only by GR bytes (Q5) |
| `ESC }` (LS2R) | ECMA-35 8.3.2; ctlseqs 425; vt220-rm-ch4 Table 4-5 | Counted | `gr = G2` |
| `ESC ~` (LS1R) | ECMA-35 8.3.2; ctlseqs 427 | Counted | `gr = G1` |
| SO `0x0E` (LS1) | ECMA-35 8.3.1, 9.3.1 | `gl = G1`, then `invoke()` (write.rs:388-392). CONFORMING | Unchanged. Between a single shift and its character it acts and leaves the shift pending (070-3 p. 3-81; ECMA-48 9) |
| SI `0x0F` (LS0) | The same, for G0 | `gl = G0`, then `invoke()` (write.rs:393-397). CONFORMING | As SO |
| `0x8E` under Latin-1 | ECMA-35 Table 2 ("08/14"); ECMA-48 8.3.141; ctlseqs 175 | graph.rs:871-873 routes the byte to `EscFinal`, but write.rs:664-667 re-reads the raw `0x8E`, so `Esc::parse` counts it (**D1**). A stale intermediate sends it to the designation arm instead (**D2**: `ESC * 0 8E` makes G2 `Other`). Nothing resets `csi` on the C1-from-Ground path; the resets are at write.rs:473, 499 and 593. DEPARTS | The C1 edge reads the bare-`ESC` row (`escape(b - 0x40)`), so once that row's `N` targets `SingleShift` (§5), `8E` is SS2 and no byte is re-read. D1 and D2 are still fixed, for the C1 finals that reach `esc_final` (84/85/88/8D): map the byte with `InputEncoding::c1_as_escape` (decode.rs:775, which has no caller) and reset `csi` on this path. Their tests are in the working tree (§6, test 17) |
| `0x8F` under Latin-1 | The same, "08/15" | The same | The same, as `ESC O` |
| `0x8E`/`0x8F` under UTF-8 | ctlseqs 56-65 | Stray continuation byte: U+FFFD, `replacements` +1. CONFORMING | None |
| `7F` DEL in Ground | 070-3 §3.4.2 ("shall be ignored"); vt510-ch7 §7.2.5 (DEL 127, "Ignored (unless in CRM)"); williams ground ("7F will be ignored" with a 94-set in GL) | `Route::of(0x7F)` is `Del` (scan.rs:520), which is neither `Control` nor `Text`. Under Latin-1 it is not a Latin-1 graphic, so `Act::Bad` (graph.rs:876). Under UTF-8 it is no UTF-8 lead, so `Act::Bad` (graph.rs:889). `Act::Bad` writes U+FFFD (write.rs:670-673). By reading; utf8.rs:96's comment names the same arm. DEPARTS (**D4**) | Ignored while GL holds a 94-set. With a 96-set in GL it is that set's character (ECMA-35 8.3.1; Q3) |
| GR byte `A0-FF` under Latin-1 | ECMA-35 8.1, 8.3.2, 10.3.1 | `Act::Latin1` (write.rs:437-443) → `single_byte` (decode.rs:787-793) → U+00A0-U+00FF. No slot is consulted | Translate `b & 0x7F` through `g[gr]`. With a 94-set in GR, A0 and FF follow Q8. What the default draws is Q2 |
| The same byte under UTF-8 | FCD 11; ECMA-35 NOTE 1, 15.4.1/15.4.2; ctlseqs 56-65 | A UTF-8 lead or continuation byte | None (P, Q5) |
| `ESC  * +` | ECMA-35 Table 6, 14.3.2, 7.3 | Slot loaded, then `invoke()` (write.rs:1687-1701). CONFORMING | Mechanism unchanged. A slot that holds GR takes effect at once (14.3.2), with no code needed, because GR is read at each GR byte (§5) |
| `ESC - . /` | ECMA-35 Table 6 (G1D6-G3D6), NOTE 44; ctlseqs 379-394; 070-5 p. 5-117 | `_ => unsupported_total += 1` (write.rs:1747) | Designate a 96-set into G1, G2 or G3 from the 96-series finals (NOTE 44). `A`, ISO Latin-1 Supplemental, is the one Q3 needs. Other finals are recorded and counted in `charset_unsupported`, as `Other` is today. `ESC , F` (02/12, reserved) stays counted |

---

## 3. Initial state, DECSC/DECRC, DECSTR, RIS

| Event | Sets | Text | baud today |
|---|---|---|---|
| Power-up (`Cold::new`, term.rs:507-508) | G0 = G1 = ASCII; G2 = G3 = UPSS (its value is Q2); GL = G0; GR = G2; no single shift; save buffer per the DECRC row | 070-3 §3.6.4.2/§3.6.4.3; vt220-rm-ch4 §4.4; vt510-rm-ch4 §4.4.1; 070-4 p. 15 `single_shift = NO_SINGLE_SHIFT` | All four slots ASCII, GL = G0, no GR |
| DECSC `ESC 7` | Position, rendition, `flags`, G0-G3, GL, **GR**, protection, the wrap item(s) Q12 picks, DECOM, and a pending single shift only under Q6(b) | vt510-decsc; 070-5 §5.13.1; vt220-rm-ch4 §4.7; 070-5 DECCIR pp. 5-200 to 5-202; vt510-deccir | `Saved` (term.rs:55-89) lacks GR, pending wrap and the single shift. `save()` is at ops.rs:69-81. 2026-10-13: `Saved::pending` exists, records only whether the saved `cx` is the pending-wrap sentinel (`cx == right + 1`), and decides nothing of Q12 |
| DECRC `ESC 8` | Restores the above and calls `invoke()` | The same | `restore()` (ops.rs:97-123) clamps `cx` to `cols-1` (ops.rs:99-100). That loses pending wrap, which is `cx == cols` (term.rs:2089-2090) |
| DECRC, nothing saved | Home; DECOM reset; attributes off; the default character-set mapping (GL = G0, GR = G2, default slots) | vt220-rm-ch4 §4.7; vt510-decrc; 070-5 p. 5-187 item 3 | Built 2026-10-11 as `Paint::restore_unsaved` from `ESC 8`, with the selective erase attribute off as 070's buffer holds it; no GR. Until then `restore(None)` returned (ops.rs:98), which DEPARTED. SCORC: Q13 |
| DECSTR `CSI ! p` | G0-G3, GL and GR to defaults; no single shift ("no shifts"); save buffer to defaults, cursor home | 070-4 p. 31; vt220-rm-ch4 Table 4-10; vt510-decstr Table 5-9 | `soft_reset` resets `g` and `gl` (ops.rs:1511-1512) and sets `saved = None` (ops.rs:1509). That DEPARTS from "Save cursor state … Home position" |
| RIS `ESC c` | As DECSTR, plus single shift NONE | ECMA-48 8.3.105; vt220-rm-ch4 §4.18.2; vt510-ris; 070-4 p. 15 (its "Device Initialization", which 070-4 p. 39 names as where RIS's algorithm now lives). 070-D p. D-7 is a guideline only (D.1) | `hard_reset` → `soft_reset` (ops.rs:1360). The same departure |

Under Q6(a) a pending single shift never survives into DECSTR or RIS, because
their `ESC` (or `CSI`) has already cancelled it. Both resets clear it anyway:
the texts say so, and the clear costs nothing.

070-3 §3.8.1's NOTES (pp. 3-68/3-69) say the same of the designations: RIS,
DECSTR and DECSCL "reset the "Default Designations And Invocations"". So
whatever an announcer designated (Q18) does not survive either reset, and
the resets need no extra state for it.

**Departure found: RIS keeps the input encoding.**

- `cold.encoding` is written only by `Cold::new` (`encoding:
  InputEncoding::Utf8`, term.rs:529) and by `esc_final`'s `%` arm
  (write.rs:1735-1746).
- `ops.rs` has no reset that writes it (found by grep). So after `ESC % @ …
  ESC c` the stream stays Latin-1.
- ECMA-48 8.3.105 resets "to its initial state, i.e. the state it has after it
  is made operational", and baud is made operational in UTF-8. So keeping
  Latin-1 across RIS DEPARTS.
- That is Q17, open. Test 27 is written so that it pins neither answer.
- For DECSTR, no text read says the input encoding changes. 070-4 p. 31's
  "Character Set Mode" row names DEC's multinational-versus-NRCS mode, not an
  input encoding (inferred).

---

## 4. Conflicts and silences: the open questions

Each option is marked **CONFORMING** or **DEPARTS** against a named text, and
practice is marked the same way. A recommendation states the departures
first. Where every option departs from some text, the question says so and
carries no recommendation.

### Q1. Which slot GR holds initially

**The reading that decides it.** GR exists in baud only under `ESC % @`, which
ctlseqs 312 calls "ISO 8859-1 (ISO 2022)". NOTE A.3 identifies ISO 8859-1 as
"8bit-level1" with "G0 and G1 code elements only", and 10.3.1 Level 1 gives
"a G1 code element having GR shift status". If `ESC % @` selects that code,
GR holds G1.

- (a) **G2.**
  - CONFORMING to vt220-rm-ch4 Table 4-5 (LS2R "(default)") and to 070-3
    §3.6.4.2 and §3.6.4.3.
  - Consistent with vt510-deccir's example report, "0; 2; indicates that G0
    is mapped into GL, and G2 is in GR" (an example, not a stated default).
  - vt510-ris and vt510-decrc name the set in GR, not the slot.
  - ECMA-35 8.3.3 and 10.1 leave the initial state to the version.
  - DEPARTS from ECMA-35 10.3.1 Level 1 and NOTE A.3 under the reading above,
    in which `ESC % @` selects the 8bit-level1 ISO 8859-1 code.
- (b) **G1.**
  - CONFORMING to ECMA-35 10.3.1 Level 1 and NOTE A.3 (ISO 8859-1 = G0 + G1).
  - It is also the GR state 070-3 §3.8.1's Level 1 and Level 2 announcers
    establish ("G1 is invoked into GR"), but only after an announcer, and so
    not as an initial state (Q18).
  - A.3.1's `gright` "(1)" default belongs to the identifier notation, not to
    a device's initial state.
  - DEPARTS from every DEC text read, all of which put ASCII in G1.
  - To keep today's GR cells, G1 would have to hold the Latin-1 right half.
    Then a bare SO would draw from it: today `SO a` draws `a`, under (b)
    `á`. The golden `charset` fixture (`golden/fixtures.txt:37-38`) pins
    SO/SI.
- Practice: xterm `curgr = 2` ([[source: xterm-reset]] charproc.c:1278),
  which is CONFORMING to DEC. foot has no GR.
- **No recommendation.** Under the 8bit-level1 reading every option departs
  from some text: (a) from ECMA-35 10.3.1 and NOTE A.3, (b) from every DEC
  text read. Under the reading that GR under `ESC % @` is DEC's Level 2/3 GR,
  (a) departs from none. Which reading holds, and so which departure baud
  takes, is a choice the project makes; §10 records it.

### Q2. What G2 and G3 hold initially, and so what a default GR byte draws

**The 20 positions.** Evidence from [[source: dec-special-graphics]] Table 2-3,
right half:

- **Extraction.** Each SVG `<text class="chname">` glyph is paired with the
  `<text class="chnums">` octal/decimal/hex triple that follows it, from the
  pinned `vt220-rm-chapter2.html`. The code numbers are 7-bit, so 0x80 was
  added.
- **Positive control.** 76 of the 96 GR positions carry the identity
  character, for example `C3` Ã, `A9` ©, `E9` é.
- **DEC Supplemental differs from today's Latin-1 cells at 20 positions.**
  - 5 substitutions, each shown as DEC Supplemental (today's cell):
    - `A8` ¤ (U+00A8 ¨)
    - `D7` Œ (U+00D7 ×)
    - `DD` Ÿ (U+00DD Ý)
    - `F7` œ (U+00F7 ÷)
    - `FD` ÿ (U+00FD ý)
  - 15 empty positions: `A0 A4 A6 AC AD AE AF B4 B8 BE D0 DE F0 FE FF`.
  - `A0` and `FF` are 10/00 and 15/15, which no 94-set can hold (ECMA-35
    8.3.2). So **no 94-set in GR reproduces today's `A0` or `FF`**.
    `latin1_has_no_ill_formed_bytes` (encoding/latin1.rs:59-63) pins `FF` →
    U+00FF.
- Each glyph's codepoint is the transcriber's choice. The names were checked
  against [[source: unicode-data]], for example `00D7;MULTIPLICATION SIGN`.

**Evidence that names DEC Supplemental as the factory or default value.**

- dec-special-graphics §2.4.1 (factory default: DEC multinational, Table 2-3)
  and vt220-rm-ch4 §4.4 (DEC supplemental graphics in GR).
- 070-3 §3.6.4.2. Its heading scopes it to "Level 2 (without 8-bit Interface
  Architecture Extension)". A terminal with 96-sets (Q3(a)) or a UPSS has that
  extension, so §3.6.4.3 governs it:
  - p. 3-63 says it directly of the UPSS: "Support for the UPSS is part of
    the 8-bit Interface Architecture Extension to Level 2".
  - For 96-sets it is inferred from §3.1.3 (p. 3-7). That section, headed
    "8-BIT INTERFACE ARCHITECTURE EXTENSION", describes the extension by
    "96-character graphic character sets, ISO Latin-1 Supplemental, and the
    User Preference Supplemental Set (UPSS)", and says "The rest of this
    section … assumes the 8-bit Interface Architecture Extension is present".
- The UPSS's factory value:
  - 070-4 p. 14, `upss = DEC_SUPPLEMENTAL_G; /* NVM */`;
  - 070-4 p. 31, "UPSS | DEC Supplemental (NVM if present)";
  - vt510-decaupss, "Default: DEC Supplemental Graphic".
- vt510-ris and vt510-decrc ("DEC Supplemental Graphic" into GR), and
  vt510-deccir's example report: "BB%5%5 indicates that ASCII is in G0 and
  G1, and DEC Supplemental Graphic is in G2 and G3" (an example).
- 070-D p. D-7, `IN_USE_TABLE.GR:= SUPPLEMENTAL_G`. That is a guideline only,
  and it predates the UPSS.
- ctlseqs 328-330.
- Practice: xterm's `DFT_UPSS` is `nrc_DEC_Supp_Graphic`
  ([[source: xterm-ptyx-h]] ptyx.h:1138). Its comment at charproc.c:1250-1251
  calls DEC Supplemental "the default".

**Evidence that the UPSS is selectable, and its value a set-up choice.**

- 070-3 §3.6.4.3 ("UPSS, typically DEC Supplemental or ISO Latin-1
  supplemental").
- 070-3 p. 3-63 ("must be selectable").
- 070-3 §3.6.3 ("selected by the user as a SETUP option"; "introduced to
  facilitate migration from DEC-MCS to ISO Latin-1").
- The "NVM" markers on 070-4 pp. 14 and 31, which mark the value as held in
  non-volatile memory (inferred: neither page defines the term).
- vt510-rm-ch4 §4.4.1 ("the current user-preferred character set in GR").
- vt510-decstr Table 5-9 ("DECAUPSS | Set selected in Set-Up").
- ctlseqs 312 ("ESC % @ … That is ISO 8859-1").
- Practice: xterm's `preferLatin1` defaults to True (charproc.c:460), which
  makes its UPSS ISO Latin-1 Supplemental (ptyx.h:1138-1140).

**Reading.** Under §3.6.4.3, G2 and G3 hold the UPSS. The UPSS's factory value
is DEC Supplemental, and the UPSS must be selectable. So every option below
also needs the selection (configuration plus DECAUPSS, the DECAUPSS
item) to conform to p. 3-63.

**Options.**

- (a) **DEC Supplemental (a 94-set), selectable to ISO Latin-1.**
  - CONFORMING to every text in the first list.
  - DEPARTS from ctlseqs 312: under `ESC % @`, GR bytes stop being ISO 8859-1
    at the 20 positions.
  - Breaks the control (test 22). Q8 then decides `A0` and `FF` on a default
    terminal.
  - Needs a DEC Supplemental table: `Dec(Supplemental)` falls to `with_ascii`
    today (cell.rs:389-396).
- (b) **ISO Latin-1 Supplemental (a 96-set) as baud's factory UPSS value.**
  - CONFORMING to 070-3 §3.6.4.3, p. 3-63 and §3.6.3 as a selectable value,
    to vt510-rm-ch4 §4.4.1 and to ctlseqs 312.
  - DEPARTS from the UPSS's factory value (070-4 pp. 14 and 31,
    vt510-decaupss), from vt510-ris and vt510-decrc, from dec-special-graphics
    §2.4.1 and vt220-rm-ch4 §4.4, and from ctlseqs 328-330.
  - Keeps the control at all 96 positions, **provided ISO Latin-1 Supplemental
    is ISO 8859-1's right half**. That rests on NOTE A.3 (register entries 6
    and 100) and on [[source: unicode18-ch2]]: "The first 256 codes follow
    precisely the arrangement of ISO/IEC 8859-1 (Latin 1)". Register entry 100
    itself is **unread** (§8).
  - Practice: xterm.
- (c) **ASCII.**
  - CONFORMING to 070-3 §3.6.4.1 (Level 1, "7-bits only"). At Level 1,
    070-4 p. 15 sets `in_use_table.gr = ASCII_G`, so GR exists and holds
    ASCII, but no 8-bit byte reaches it.
  - CONFORMING to the VT100-mode texts: vt220-rm-ch4 Figure 4-2's footnote,
    G2 and G3 are "Always ASCII or National Replacement Character sets in
    VT100 mode", and vt510-ch7 §7.3 ("In VT100 mode … only ASCII, NRC sets,
    and DEC Special Graphic characters are available").
  - DEPARTS from every Level 2/3 text.
  - Changes all 96 cells: A1-FE become ASCII, and A0 and FF follow Q8.
  - Practice: foot (no GR); xterm under UTF-8 (Q4.1).

No option conforms to every text, so no default is recommended.

- Only (b) keeps today's Latin-1 cells.
- Only (a) has DEC's factory value.
- **Q2 turns on Q9's level reading.** baud's DA answer claims a VT102, and
  vt510-decscl makes Level 1 "VT100 mode". Under that reading only (c)
  conforms to the DEC texts, through Figure 4-2's footnote and vt510-ch7
  §7.3. Under a Level 2/3 reading, (a) or (b), as above. Decide the two
  together.

### Q3. Whether 96-character sets are in scope now

- (a) **Now, in part.** `ESC - . /` with final `A`, and 96-set semantics in GL
  and GR.
  - In GL, 0x20 and 0x7F are characters of the set (ECMA-35 8.3.1; ctlseqs
    380-382; vt510-ch7 §7.2.5).
  - In GR, A0 and FF belong to the set (8.3.2).
  - CONFORMING to ECMA-35, and to 070-3 at Level 3 and 2X (§3.1.3; p. 3-63).
    Both Q2(b) and the selectable UPSS of Q2(a) need it.
  - DEPARTS under the VT102 (Level 1) reading of Q9 from vt510-ch7 §7.3 ("In
    VT100 mode … only ASCII, NRC sets, and DEC Special Graphic characters are
    available").
- (b) **Defer.** `ESC - . /` stay counted, and Q2(b) and a selectable UPSS
  become impossible.
  - DEPARTS at Level 3 from 070-3 p. 3-63 ("the UPSS must be selectable
    between DEC Supplemental and ISO Latin-1 supplemental"), because ISO
    Latin-1 Supplemental is a 96-set. Also DEPARTS from §3.1.3, which makes
    the extension that carries 96-sets "required for Level 3 conformance".
  - CONFORMING under the VT102 (Level 1) reading of Q9, through vt510-ch7
    §7.3.
- **Recommend (a), departure first.** It departs from vt510-ch7 §7.3 under
  the VT102 reading of DA, and conforms at Level 3. baud's Latin-1 mode
  already takes 8-bit bytes, which Level 1 does not ("Level 1 operation is
  7-bits only", §3.6.4.1), so baud already acts beyond Level 1 there
  (inferred). The reading is the decision's (Q9), so this goes for decision
  with Q9.
- The other 96 finals (B F H L M, ctlseqs 385-389) are recorded as a named 96
  `Other` and counted. Their tables belong to the open xterm-charsets.h item.

### P. The premise Q4, Q5 and Q15 rest on: is UTF-8 mode an ISO 2022 code?

Q4 and Q5 must rest on one premise.

- **P-A: UTF-8 is not an ISO 2022 code.** This is what the texts say:
  - FCD 11 ("shall not be used with this coded character set");
  - ECMA-35 NOTE 1 ("a different structure not in accordance with this
    Standard");
  - 15.4.1 (DOCS invokes "an identified coding system different from that of
    this Standard").

  Under P-A, **no ISO 2022 clause governs any code-extension function under
  UTF-8**. That includes the designations and SO/SI that baud already performs
  there: `ESC ( 0` box drawing depends on them, as it does in xterm and foot.
  So every such behaviour under UTF-8 rests on practice and on DEC receiver
  rules taken by analogy, and none conforms to or departs from an ISO text.
  FCD 11 binds the data's originator, and says nothing of receivers
  (inferred: "shall not be used" is worded as a rule for the coded data).
- **P-B: baud's UTF-8 mode runs a 7-bit ISO 2022 code in 00-7F and UTF-8 in
  80-FF.**
  - Under P-B, ECMA-35 clause 9 governs 00-7F: 9.1 (no GR), 9.3.1, 9.3.2 and
    9.4.
  - No text read defines such a hybrid.

How each premise answers Q4 and Q5:

| | under P-A | under P-B |
|---|---|---|
| Q4, `ESC N/O` | (a) act or (b) count: no text decides; practice acts (xterm, foot) | (a) act: CONFORMING to 9.4; (b) count: DEPARTS from 9.4 |
| Q5, GR bytes | (a) none: UTF-8 decides 80-FF | (a) none: 9.1 |
| Q5, LS1R-LS3R | (i) state, (ii) GL or (iii) count: no text decides; xterm does (i). 9.3.2 does not apply, because it governs an ISO 2022 7-bit code | (ii) **required** by 9.3.2 ("same effects as SO, LS2, and LS3"); (i) and (iii) DEPART |

**Recommend P-A**, because it is what FCD 11, NOTE 1 and 15.4.1 state. Under
P-A, Q4(a) and Q5(a)+(i) are the choices consistent with the designations
baud already honours under UTF-8, and they depart from no text. If P-B is
taken instead, Q5 becomes (ii).

### Q4. Single shifts under UTF-8 (on P)

- `0x8E`/`0x8F`: no change (ctlseqs 56-65). CONFORMING.
- `ESC N` / `ESC O`, options:
  - (a) **Act.** A following byte 0x20-0x7E is drawn from G2/G3. Any other
    character, including every decoded non-ASCII character, cancels the shift
    and is processed normally.
    - That is 070-3 p. 3-81's rule ("not a graphic character in this range").
      Under P-A it is applied by analogy; under P-B it is consistent with 9.4.
  - (b) **Count and ignore under UTF-8.** Under P-A no text forbids it either.
    It would make SS2 alone inert among the code-extension functions baud acts
    on under UTF-8.
    - Under P-B it DEPARTS from ECMA-35 9.4 (p. 23): "Each of the single-shift
      functions SS2 and SS3 shall invoke a single character from the specific
      character set that has been identified as the respective code element
      G2 or G3."
- Practice:
  - xterm: a decoded code point above 255 consumes the shift and is left
    unmapped (charproc.c:1706-1715; [[source: xterm-charsets-c]] 390-393).
    That gives the same cell as (a).
  - foot: non-ASCII does not consume the shift (terminal.c:4073-4081, 4145,
    4363). That DEPARTS from vt220-rm-ch4 §4.4.4.
- **Recommend (a)** under either premise.
- **Decided: (a)**, 2026-10-08 (§10).

**Q4.1. What G2 and G3 hold under UTF-8 (open, with Q2).** Q4(a) has a visible
consequence once G2/G3 hold anything but ASCII. baud's own key encoder sends
`SS3` sequences:

- `SS3 A` for an application-mode cursor key (baud-domain/src/vt/key.rs:1090);
- `SS3 P` for unmodified F1 (baud-domain/src/input.rs:154, 399).

When such a sequence comes back as output (an application in raw mode echoing
its input), Q4(a) draws the final byte from G3:

| Echoed | today, and xterm under UTF-8 | Q2(a), DEC Supplemental | Q2(b), ISO Latin-1 Supplemental |
|---|---|---|---|
| `ESC O A` | `A` | `Á` | `Á` |
| `ESC O P` | `P` | position D0, one of DEC Supplemental's 15 empty positions | `Ð` |

- (α) **Q2's value in both encodings.**
  - One state, no split by encoding. The echo draws `Á`.
  - CONFORMING to the DEC power-up texts (070-3 §3.6.4.2/§3.6.4.3,
    vt220-rm-ch4 §4.4, vt510-rm-ch4 §4.4.1), applied under UTF-8 by analogy:
    they describe 8-bit DEC terminals. Under P-A no ISO text governs G2/G3
    under UTF-8.
- (β) **ASCII under UTF-8 and Q2's value under Latin-1.**
  - This is xterm's practice (charproc.c:1259-1261). xterm's own comment says
    it "differ[s] from the documented DEC terminal" (charproc.c:1266-1269).
  - DEPARTS from the same DEC power-up texts under UTF-8, by that analogy;
    under P-A, from no ISO text.
  - The echo shows `A`, as today.
  - The state then differs by encoding, which brings in Q15: xterm resets the
    charsets on entering UTF-8 ([[source: xterm-ptydata-c]] ptydata.c:351-353).
  - **(β) fixes Q15's first `ESC % @`.** baud powers up in UTF-8
    (term.rs:529), so under (β) G2/G3 start ASCII, and Q15's first-return rule
    ("keeping the current state") would carry ASCII into Latin-1, which is not
    (β). So (β) needs the first `ESC % @` to install Q2's value. Choosing (β)
    chooses that for Q15's first return, and test 22 holds only after that
    install.
- (γ) **Q2's value, with SS2/SS3 counted under UTF-8 (Q4(b)).** The echo shows
  `A`, and SS2 does nothing under UTF-8. Under P-A it departs from no text;
  under P-B it DEPARTS from ECMA-35 9.4, as Q4(b) does.

The consequence arrives only at build stage (c) (§9). Until then G2 and G3
stay ASCII, so the echo shows `A`.

### Q5. GR under UTF-8; LS1R-LS3R under UTF-8 (on P)

- GR bytes under UTF-8:
  - (a) **No GR.** A0-FF stay UTF-8. Follows from P-A and from P-B (9.1).
  - (b) **xterm's practice.** Decoded U+0080-U+00FF are mapped through GR,
    and raw bytes bypass UTF-8 decoding whenever GR is not ASCII
    (ptydata.c:91-97).
    - After `ESC * 0`, `C3 A9` shows as `Ã©` (derived by reading).
    - No text read requires this. Under P-A it departs from no text read
      either, since no ISO 2022 clause governs 80-FF under UTF-8. Under P-B it
      DEPARTS from ECMA-35 9.1 (p. 20), which gives a 7-bit code only "the CL
      area" and "the GL area".
- LS1R-LS3R under UTF-8:
  - (i) Set the GR slot (state only).
  - (ii) Take 9.3.2's 7-bit meaning (into GL).
  - (iii) Count.
- **Recommend (a) and (i), under P-A.** Under P-B, (ii) is required.
- **Decided, GR bytes: (a), no GR** (AE5 part 1). Weighed again on its own with
  `[[source: unicode18-ch3]]` C4 and D92 added to the governing texts, (a)
  holds (§10, "Decided since"). It is baud's behaviour already, now held by
  `gr_bytes_under_utf8_are_utf8_whatever_is_designated`. LS1R-LS3R under
  UTF-8 stay open.

### Q6. A control function between SS2/SS3 and its character

The texts:

- ECMA-35 8.4 says "immediately following" and gives no receiver rule.
- ECMA-48 9 permits only SO and SI in that position, and only in 7-bit.
- 070-3 pp. 3-80/3-81 is the only text with a receiver rule: anything that is
  not a graphic character cancels the shift, except SO and SI.
- vt220-rm-ch4 §4.4 and §4.4.4 say "next single graphic character". vt510-ss
  says "active for only one character" and "for the next character". Both
  wordings also admit survival.
- vt510-decsc and both DECCIRs save or report a pending shift. Under 070's
  rule that cannot happen, because the `ESC` of `ESC 7` cancels it (conflict
  C5).

Options:

- (a) **070.** Everything other than a graphic character, SO or SI cancels the
  shift. SO and SI act and keep it pending. A later SS2/SS3 replaces it, per
  070's algorithm.
  - CONFORMING to 070-3; consistent with ECMA-35 8.4 and ECMA-48 9.
  - vt510-decsc's SS item and both DECCIRs' SS bits are then always empty
    (C5). Saving them is vacuous, so `Saved` gains no field.
- (b) **Survive every control and escape** until the next graphic character;
  DECSC saves the shift and DECRC restores it.
  - CONFORMING to vt220-rm-ch4 §4.4 under that reading, and to vt510-decsc.
  - DEPARTS from 070-3 p. 3-81.
- (c) **Survive, but not saved.**
  - Practice: xterm ([[source: xterm-reset]] charproc.c:1714;
    [[source: xterm-cursor-c]] cursor.c:398-416).
  - DEPARTS from vt510-decsc and from 070-3 p. 3-81.
- (d) **Practice: foot.**
  - These turn the pending shift into a locking shift (the mechanism is
    [[source: foot-term-reset]] terminal.c:4084-4102, with the single-shift
    printer at 4073-4081 and 4105-4110):

    | Trigger | Where |
    |---|---|
    | SGR 0 | [[source: foot-csi]] csi.c:31-40 |
    | SGR 4:x, 21, 24, 58, 59 | csi.c:121, 135, 144, 247, 284 |
    | IRM | csi.c:1186-1191 |
    | modes 47/1047/1049 | csi.c:495-542; 542 is that case's own call |
    | OSC 8 | terminal.c:4639-4650 |
    | SCS | [[source: foot-vt-ris]] vt.c:557, 567 |
    | DECRC, for a shift pending at DECSC | terminal.c:3225-3230 saves `selected`; 3241-3246 restores it and resets the printer |

    All of these DEPART from vt220-rm-ch4 §4.4.4.
  - SO, SI, LS2 and LS3 (vt.c:240-254, 484-498) set `selected` to their own
    slot and drop the pending shift. That DEPARTS from 070-3 p. 3-81 ("does
    not cancel the Single Shift").
  - The resets set `selected = G0` (terminal.c:2085-2090) before resetting
    the printer (2145), so they drop the shift rather than lock it.

**Recommend (a).** Its one conflict is C5, in which vt510-decsc's and the
DECCIRs' single-shift item is always empty. It is the only stated receiver
rule, and the cheapest to build (§5).

Sub-points:

- **How many SO/SI may come between (C4).** 070 conflicts with itself here:
  its text (p. 3-81 [img]) says "a single Shift Out (0/14) or Shift In (0/15)
  control character may occur", and its algorithm (p. 3-82 [img]) clears
  `cancel_single_shift` after each SO/SI, so any number. ECMA-48 9 also allows
  any number. **Recommend any number, departure first:** it DEPARTS from 070-3
  p. 3-81's text and is CONFORMING to its algorithm and to ECMA-48 9. A
  departure, so it is decided as one.
- **The shift status after `SS2 SO x`.**
  - ECMA-48 9 says SO "may, indeed, affect the meanings of bit combinations
    following".
  - ECMA-35 8.4/9.4's "shall continue in effect" sentence, read literally,
    conflicts with that.
  - 070 says SO "will be interpreted normally".
  - **Recommend that SO acts, departure first:** it DEPARTS from ECMA-35
    8.4/9.4 read literally, and is CONFORMING to 070-3 p. 3-81 and ECMA-48 9.
    A departure, so it is decided as one.

### Q7. A single-shift operand from GR, under Latin-1

The texts that name the single-shift area:

- ECMA-35 8.4 says a version shall identify "either the GL area or the GR area
  … as the single-shift area".
- 10.3.1 Level 2 (p. 25) has "GL identified as the single-shift area"; 10.3.2
  makes GR the area for Levels 2A, 3A and 4A; Table 7 facility 28 announces
  GR.
- Annex D.2 (informative) says "but not from both".
- The DEC texts all name GL: 070-3 §3.6.1.2's table ("SS2 Single Shift Two G2
  GL"), vt220-rm-ch4 §4.4.4.1 ("invokes G2 into GL"), and vt510-ss ("Maps G2
  into GL").

Against those:

- 070-3 p. 3-81 accepts operands "2/0 through 7/15 or 10/0 through 15/15".
- 070-5 pp. 5-140/5-141 strips bit 8 and draws from G2/G3.
- ECMA-48 9 gives an operand in 10/00-15/14 "the same meanings" as
  02/00-07/14, as a transformation tolerance.

Options:

- (a) **Strip bit 8 and draw from G2/G3.**
  - CONFORMING to 070-3 p. 3-81, to 070-5, and to ECMA-48 9 for 10/00-15/14.
  - With GL the single-shift area, a GR operand is non-conforming data, which
    ECMA-48 9 tolerates.
  - At 15/15, ECMA-48 9 is silent and 070 accepts it.
- (b) **The GR byte cancels the shift and draws from GR.**
  - Reads GL as the single-shift area (8.4, 10.3.1), which gives no receiver
    rule.
  - DEPARTS from 070-3 p. 3-81, 070-5 and ECMA-48 9.
- Practice: xterm draws from GR and consumes the shift
  ([[source: xterm-charsets-c]] 369). That DEPARTS from 070-5 and ECMA-48 9.
- **Recommend (a).** The DEC tables' "GL" names where the character is
  invoked, and 070's own range and algorithm take GR operands.

### Q8. 10/00 and 15/15 with a 94-set in GR; SP and DEL after a single shift

**Where Q8 arises depends on Q2.**

- Under Q2(b), GR holds a 96-set, so A0 and FF matter only after a 94-set is
  explicitly designated into GR's slot or locked into GR.
- Under Q2(a), Q8 decides A0 and FF on a default terminal, and turns
  `latin1_has_no_ill_formed_bytes` red.

**The texts.**

- 070-3 §3.4.3 and §3.4.4 govern receiver behaviour: 10/0 shows "the error
  character", and 15/15 "shall be ignored".
- vt510-ch7 §7.2.5, a published DEC manual, agrees: under "Action on
  Receipt", 160 is "Error character generated" and 255 "Ignored (unless in
  CRM)". Its notes take both out of that rule when a 96-set is in GR.
- 070-3 §3.6.1.1 c says locking shifts do not affect those positions.
- ECMA-35 8.1 and 8.3.2 and dec-special-graphics §2.3.2 say the positions are
  unused. That is a rule for the data.
- ECMA-48 9 does not reach them: it covers sequence bytes, control strings and
  single-shift operands, and stops at 15/14.

| Option | A0 | FF |
|---|---|---|
| (a) strip bit 8 (A0 is SPACE, FF is DEL) | DEPARTS from §3.4.3 (a space, not the error character) | Conforms in effect to §3.4.4 only once DEL is ignored. Ground's DEL writes U+FFFD today (D4) |
| (b) ignore both | DEPARTS from §3.4.3 | CONFORMING to §3.4.4 |
| (c) U+FFFD for both | CONFORMING if U+FFFD is accepted as the error character's image, which 070 leaves open ("usually imaged as") | DEPARTS from §3.4.4 |
| (d) **070 and vt510-ch7: A0 shows the error character, FF is ignored** | CONFORMING | CONFORMING |

The A0 and FF marks against 070-3 §3.4.3 and §3.4.4 hold the same against
vt510-ch7 §7.2.5.

- (d) is the only option that conforms to every text read. It is still a
  choice to make, for two reasons:
  1. **Consistency with SUB.** `docs/vt-coverage.md:189` keeps SUB's display
     of the same DEC error character `out`, following xterm, and gives it as
     `␦` (U+2426). Adopting 070 for A0 while following xterm for SUB is a
     consistency question, not one a text settles.
  2. **The codepoint.** Which codepoint images the error character: U+2426,
     as that row names it, or U+FFFD.
- **After a single shift with a 94-set** (the operand rules, not §3.4):
  - SP is inside 070's range, so it writes a space and consumes the shift.
  - A0, stripped by 070-5 to 2/0, does the same. ECMA-48 9 also gives an A0
    operand 2/0's meaning. This conflicts with §3.4.3, which this note reads
    as scoped to GR display.
  - DEL, or FF stripped, is not a graphic character of a 94-set. It cancels
    the shift and is re-dispatched to Ground, where DEL is ignored once D4 is
    fixed, and FF follows this question.

### Q9. Level gating of LS2/LS3, LS1R-LS3R and SS2/SS3 (C1, C2)

baud claims VT102 in DA (`baud-domain/src/vt/report.rs:201`) and has no
conformance-level model. vt510-decscl makes Level 1 "VT100 mode", so the
VT102 reading of DA is Level 1.

**DEC's own texts conflict on LS2 and LS3.**

- 070-3 §3.6.1.1 (p. 3-61): "Conforming terminal products will provide the
  following locking shift functions in Level 1 operation". Its table lists
  LS0, LS1, LS2 and LS3. LS1R-LS3R are "additional locking shift functions
  in Level 2 operation".
- vt220-rm-ch4 Table 4-5: LS2 and LS3 are "(VT200 mode only)", as are
  LS1R-LS3R.
- vt510-ls Table 5-15: "The following locking shift functions are available
  only in VT Level 4 mode", listing LS1R, LS2, LS2R, LS3 and LS3R.
- So at Level 1, 070-3 requires LS2 and LS3, and the two published manuals
  withhold them. On LS1R-LS3R they do not conflict at Level 1: 070-3 requires
  them from Level 2 and is silent on Level 1, and the manuals withhold them.

Options:

- (a) **Ungated.**
  - SS2/SS3: CONFORMING under every reading. 070-3 gives them "Levels: 1, 2,
    3" (pp. 3-80/3-81), and vt220-rm-ch4 Figure 4-2 shows them in VT100 mode.
  - LS2 and LS3: CONFORMING to ECMA-35 Table 2 and 8.3.1, and to 070-3
    §3.6.1.1 at every level. Under the VT102 reading they DEPART from
    vt220-rm-ch4 Table 4-5 and vt510-ls.
  - LS1R-LS3R: CONFORMING to ECMA-35 8.3.2, and to 070-3 from Level 2. Under
    the VT102 reading they DEPART from vt220-rm-ch4 Table 4-5 and vt510-ls;
    070-3 neither requires nor forbids them at Level 1.
  - baud already executes DECSTR, S7C1T/S8C1T and G2/G3 designation with the
    same departure from the two manuals: vt220-rm-ch4 §4.18.1 ("(When the
    terminal is in VT100 or VT52 mode, the escape sequence is ignored.)"),
    §4.4.1 ("NOTE: You cannot designate a G2 and G3 set in VT100 mode."), and
    vt510-s7c1t and vt510-s8c1t ("VT Level 1 … Terminal ignores
    sequence").
- (b) **Gate LS2, LS3 and LS1R-LS3R off** under the VT102 reading. They stay
  counted.
  - LS2 and LS3: CONFORMING to vt220-rm-ch4 Table 4-5 and vt510-ls. DEPARTS
    from 070-3 §3.6.1.1, which requires them "in Level 1 operation".
  - LS1R-LS3R: CONFORMING to all three under that reading.
- (c) **Gate LS1R-LS3R only** under the VT102 reading, which is 070-3's
  split.
  - CONFORMING to 070-3 §3.6.1.1 at every level.
  - LS2 and LS3 DEPART from vt220-rm-ch4 Table 4-5 and vt510-ls under the
    VT102 reading. That is the conflict above, and no option avoids it.
- Under the VT102 reading every option departs from some DEC text for LS2
  and LS3. Under a Level 2-4 reading, (a) conforms to every text read, and
  (b) and (c) gate nothing.
- Practice: xterm gates SS2/SS3 at `vtXX_level > 1` (charproc.c:5481-5493),
  and LS2/LS3/LS1R-LS3R at `ansi_level > 2` (6174-6207).
  - The default terminal ID is `DFT_DECID "420"` (ptyx.h:399), so
    `vtXX_level` is 4 (charproc.c:11051) and `set_vtXX_level` makes
    `ansi_level` 3 (1323-1328). By default, all act.
  - Gating SS2/SS3 below VT200 DEPARTS from 070-3 pp. 3-80/3-81 and from
    Figure 4-2.
  - `ESC SP L` and `ESC SP M` also lower `ansi_level`, to 1 and 2
    (charproc.c:5070-5086; `set_ansi_conformance`, 1311-1321, which acts only
    when `vtXX_level >= 3`). After them xterm is still at DEC Level 3 or
    above and stops acting on LS2, LS3 and LS1R-LS3R. That DEPARTS from 070-3
    §3.6.1.1, and 070-3 §3.8.1's NOTES say ISO 4873's levels are "NOT
    equivalent to the Digital conformance levels". See Q18.
- **No recommendation.** Under the VT102 reading every option departs from
  some text, so by this note's own rule it carries none. Which level baud's
  DA answer claims (the level-coherence question) is a choice too, and it
  also decides Q2, Q3 and Q18.

### Q10. ECMA-35 8.3.4 against NOTE 25

- Read 8.3.4's "corresponding locking-shift function" as specific to an area:
  LS1 corresponds to (G1, GL), and LS1R to (G1, GR).
- Then LS1 while G1 holds GR still invokes G1 into GL. That is CONFORMING to
  NOTE 25 and consistent with 070's algorithms and xterm.
- The literal reading, in which LS1 has no effect while G1 holds GR,
  contradicts NOTE 25.
- Marks: the per-area reading DEPARTS from 8.3.4's words read literally and is
  CONFORMING to NOTE 25; the literal reading is CONFORMING to 8.3.4's words
  and DEPARTS from NOTE 25. The two clauses of one standard conflict.
- **Recommend the per-area reading, departure first:** it departs from 8.3.4
  read literally. A departure, so it is decided as one.

### Q11. Whether DECSC saves a set or a slot (C6)

- **Slots.** This is what baud does today (term.rs:63-70).
  - CONFORMING to vt510-decsc ("(G0, G1, G2, or G3)") and to both DECCIRs'
    Pgl/Pgr.
  - DEPARTS from 070-5's `save_cursor`, which stores `in_use_table.gl` and
    never restores `invoked_gl`.
- **Sets.** CONFORMING to 070-5's algorithm; DEPARTS from vt510-decsc and the
  DECCIRs.
- **No recommendation.** Every option departs from some text, so by this
  note's rule it carries none. What weighs: the published VT510 manual and
  both DECCIRs agree with each other, and 070-5 is "FOR REVIEW ONLY"; slots
  are today's behaviour (§10).

### Q12. Which wrap state DECSC saves (C8)

Three texts say what DECSC saves, and each option is marked against all three:

- **vt510-decsc**: "Wrap flag (autowrap or no autowrap)", read here as DECAWM.
- **Both DECCIRs**, each of which says it reports what DECSC saved.
  - 070-5 pp. 5-200 to 5-202: "the information saved through a Save Cursor
    (DECSC) command", with "bit 4: auto-wrap pending (1 if pending)".
  - vt510-deccir: "The cursor information in a DECCIR sequence is the same
    information saved through a save cursor (DECSC) command", with Sflag "4
    Autowrap 1 = autowrap pending".
  - A DECSC that does not save pending wrap cannot supply that bit.
- **070-5's `save_buffer_t`**, which holds neither. §5.13.1 (p. 5-186) and
  DECSC's description, reset values and `save_cursor` (pp. 5-187/5-188
  [text]) name no wrap item. It sits in a "FOR REVIEW ONLY" chapter whose own
  DECCIR reports pending wrap.

Options:

- (i) **DECAWM only.** This is today's behaviour.
  - DEPARTS from both DECCIRs (070-5 pp. 5-200 to 5-202 and vt510-deccir),
    which report pending wrap as saved.
  - DEPARTS from 070-5's `save_buffer_t`.
  - CONFORMING to vt510-decsc.
  - Pinned by `a_cursor_restore_brings_back_the_wrap_flag_and_origin_mode`
    (text.rs:301).
- (ii) **Pending wrap only.**
  - CONFORMING to both DECCIRs.
  - DEPARTS from vt510-decsc ("Wrap flag (autowrap or no autowrap)") and from
    070-5's `save_buffer_t`.
  - 070-D p. D-14 says the same as the DECCIRs, but as a guideline only (D.1).
  - vt220-rm-ch4 §4.7's "state of wrap flag" fits either.
  - Practice: xterm.
    - It saves pending wrap, `sc->wrap_flag = screen->do_wrap`
      ([[source: xterm-cursor-c]] cursor.c:408), and restores it (cursor.c:494).
    - It does not restore DECAWM: `DECSC_FLAGS` is
      `(ATTRIBUTES|ORIGIN|PROTECTED)` (cursor.c:461).
  - Practice: foot, the same. `term_save_cursor` copies the whole cursor,
    its last-column flag included ([[source: foot-term-reset]]
    terminal.c:3227). `term_restore_cursor` restores `lcf` (3239) and not
    DECAWM.
- (iii) **Both.**
  - CONFORMING to vt510-decsc and to both DECCIRs.
  - DEPARTS only from 070-5's `save_buffer_t`.
- **Only (iii) conforms to vt510-decsc and both DECCIRs.** Every option still
  departs from 070-5's `save_buffer_t`, so by this note's rule the question
  stays open and carries no recommendation.
- For whichever option saves pending wrap: restore `cx = cols` when the width
  is unchanged; after a resize, clamp as today (§8).

### Q13. DECRC with nothing saved, and SCORC sharing the slot

`CSI s`/`CSI u` and `ESC 7`/`ESC 8` share `cold.saved` (write.rs:1509-1513 and
1752-1756; `scosc_and_decsc_share_a_slot`, vt100/cursor.rs:140).

- (i) **070's model.** Snapshot the post-reset state, with the cursor homed,
  into `saved` at power-up, DECSTR and RIS.
  - CONFORMING to 070-5 p. 5-187 items 3-4 and to 070-4.
  - Turns `scorc_without_a_save_is_inert` (vt100/cursor.rs:132) and the
    `CSI 1 u` UNOBSERVABLE entry (spec_inventory.rs:415) red.
  - Practice: xterm runs `CursorSave` at realize, DECSTR and RIS
    (charproc.c:13066-13069, 14546-14547, 14559-14561).
    - xterm's SCOSC is `CursorSave` when left-right mode is off and the
      parameters are default (charproc.c:4899-4901).
    - Its SCORC falls through to DECRC's `CursorRestore`
      (charproc.c:4912-4919).
    - So in xterm, SCORC with no earlier save restores the realize-time
      snapshot.
- (ii) **Apply the four nothing-saved items in DECRC's `None` arm only**, and
  let DECSTR/RIS leave `saved = None`.
  - DECRC's observable behaviour matches every DEC text read. It differs from
    (i) only in DECAWM, which the nothing-saved texts do not name.
  - SCORC stays as pinned. That DEPARTS from xterm's practice; SCO's own text
    is unread.
  - DEPARTS from 070-5 p. 5-187 items 3-4, whose reset save buffer also holds
    `attribute.selective erase = FALSE`, which the four-item arm omits.
- **Recommend (ii)**, departure stated. It changes only what a text read
  requires. Revisit once SCORC's text is read; until then the open SCORC
  half stays open.
- **2026-10-11: DECRC's half is built, the part both options share; SCORC's
  half and the model stay open.**
  - 070 read first-hand: the pinned PDF (sha256 `d30c2ace…`, from the
    bitsavers mirror at trailing-edge.com, the www host answering 403), text
    layer through PDFKit, positive control p. 5-115 = PDF 370 matching AF's
    page map. § 5.13.1 (p. 5-186) lists what the buffer holds. DECSC note 3
    (p. 5-187): "On initialization or reset the values of the save buffer are
    cleared to the following state: position. line = 1; position. column = 1;
    ... rendition.bold = FALSE; ... attribute. selective erase = FALSE; origin
    mode = ABSOLUTE; left = in use table.gl; right = in use table.gr; gO =
    designated graphic sets[O]; ..." [text]; note 4: "Execution of Soft
    Terminal Reset (DECSTR) also causes the Save Buffer to be set to the
    default values as given above." DECRC's algorithm (pp. 5-190-5-191)
    restores exactly those fields; neither holds a wrap flag.
  - So every DEC text agrees on what a DECRC with nothing saved since a reset
    does: home, DECOM reset, SGR off, the default sets in G0-G3 and GL (and GR),
    and, in 070 and vt220-rm-ch4 Table 4-10 though not vt510-decrc's list,
    the selective erase attribute off. Option (ii)'s arm with that fifth item
    is option (i)'s DECRC observably, but for DECAWM: (i)'s snapshot would
    hold baud's `Saved::autowrap`, which no text puts in a reset buffer. Built
    as `Paint::restore_unsaved`, `ESC 8` only (`tests/decrc_unsaved.rs`).
  - SCORC's own text is now pinned, `[[source: msdos2-ansi-doc]]`: "RCP -
    Restore cursor position ... Restores the cursor position to the value it
    had when the console driver received the SCP sequence." It is silent on a
    restore before any SCP, and it restores the position alone. xterm's
    `CASE_ANSI_RC` falls through to `CursorRestore` on a slot `CursorSave`
    filled at start-up. **Open**, the choice narrowed to SCORC:
    - (a) inert, as today: conforms to ANSI.SYS, which is silent;
      departs from xterm's practice.
    - (b) DECRC's nothing-saved arm: conforms to ANSI.SYS (silent) and
      matches xterm, whose start-up snapshot holds the same values; restores
      more than ANSI.SYS's RCP restores, as `CSI u` already does with a save.
    - (c) the position alone, home: conforms to ANSI.SYS's scope (position
      only), matches neither xterm nor today.
    No text decides between them. (b), like `CSI u` with a save today,
    restores more than RCP's "cursor position"; whether that is a departure
    from ANSI.SYS's scope is part of the question.

### Q14. What `<` designates (C13)

- vt220-rm-ch4 Table 4-4: DEC Supplemental. baud today: `Dec(Supplemental)`
  (charset.rs:278).
- 070-3 p. 3-63 ("The designating sequence previously used for DEC
  Supplemental was changed to designate the UPSS") and [[source: vt510-scs]]
  ("< User-preferred Supplemental"): the UPSS.
- A 96-set UPSS is one that "Conforming software shall not designate … as G0"
  (070-3 p. 3-73).
- **Unchanged** (`<` is always DEC Supplemental):
  - CONFORMING to vt220-rm-ch4 Table 4-4, which predates the UPSS.
  - DEPARTS from 070-3 p. 3-63 and vt510-scs wherever the UPSS is not DEC
    Supplemental. Under Q2(b) that is at once; under Q2(a), once DECAUPSS or
    set-up selects ISO Latin-1.
  - While the UPSS is DEC Supplemental, the two readings designate the same
    set.
- **`<` designates the UPSS:** CONFORMING to 070-3 p. 3-63 and vt510-scs.
  DEPARTS from vt220-rm-ch4 Table 4-4 only when the UPSS is not DEC
  Supplemental.
- **Recommend: unchanged in this change, departure stated.** It does not touch
  the default state, because initial G2/G3 are set directly. Decide it with
  DECAUPSS (the item), and at stage (c) at the latest if Q2 is (b),
  since that makes the factory UPSS ISO Latin-1.

### Q15. DOCS return (`ESC % @`) restores the ISO 2022 state

- (a) **Snapshot and restore.** Snapshot `g`, `gl` and `gr` on `ESC % G`;
  restore them on `ESC % @`. CONFORMING to ECMA-35 15.4.2.
- (b) **Today.** The state is shared and nothing is restored. DEPARTS from
  15.4.2.
- **The first `ESC % @`, with no snapshot.** baud starts in UTF-8
  (term.rs:529), so the first `ESC % @` returns to a coding system that was
  never left in-stream. 15.4.2's "the time of invocation of the other coding
  system" does not exist then. No text read says what to restore, and keeping
  the current state is the reading that invents nothing.
- **Entering UTF-8.** xterm also *resets* the charsets on `ESC % G`:
  `saveCharsets` then `resetCharsets`, with UTF-8's ASCII G2/G3
  (ptydata.c:351-353).
  On leaving, it restores the designations but not `curgl`/`curgr`
  (ptydata.c:355). That DEPARTS on invocation state.
  - Under P-A, what holds inside UTF-8 is not ISO 2022's business.
  - A reset on entry is needed only if Q4.1 picks (β).
- **Recommend (a)**, with no reset on entry unless Q4.1 is (β). It gets its
  own red test and may land in a separate commit. The `vt-coverage.md` row
  records the departure until it lands.

### Q16. `CellFlags::GRAPHICS` for GR and single-shifted writes

- **Where the flag lives.**
  - Defined at cell.rs:166-185 as "written while … invoked into GL".
  - *Set* by `refresh_flags` (ops.rs:771).
  - Read only by `is_graphics` (cell.rs:186) and by tests. No renderer reads
    it; the note at baud-render/src/cellmap.rs:90 records a past misread of
    its bit as width.
- **The definitional point, found by reading.** `Paint::glyph` stamps
  `self.flags` on every cell (paint.rs:682).
  - So a Latin-1 GR byte (`Act::Latin1`), or a decoded UTF-8 character, that
    is written while GL holds DEC graphics carries `GRAPHICS`, although no DEC
    graphics translation happened.
  - That is exactly what the doc's first sentence defines: "The character in
    this cell was written while the DEC Special Character and Line Drawing
    Set was invoked into GL" (cell.rs:166-167).
  - It is not what the second sentence implies: "so it keeps where the
    character came from" (cell.rs:169-171).
  - So the code is not wrong against its doc. The doc holds two definitions,
    and choosing one is this question. An earlier draft listed this as a
    defect (D5); it is not one.
- **Options.** The flag is baud's own record, and no standard read governs
  it, so neither option conforms to or departs from a text.
  - (a) **The first sentence: the set invoked into GL when the cell was
    written.** This is today's behaviour. A single-shifted or GR-written DEC
    graphics glyph then carries no flag, because GL does not hold DEC
    graphics, and a Latin-1 character written under GL's DEC graphics does.
  - (b) **The set the character was translated through.**
- ~~**Recommend (b).** A shifted or GR-written DEC graphics glyph carries the
  flag on that cell only, and an untranslated character never carries it.
  The doc changes to match.~~
- **Decided: (a), today's behaviour**, 2026-10-08 (§10), over this
  note's recommendation of (b), which is superseded. The flag keeps
  its first sentence's meaning: the set invoked into GL when the cell was
  written. A single-shifted or GR-written DEC graphics glyph carries no flag,
  and the doc's second sentence ("so it keeps where the character came from")
  is the one to change.

### Q17. Whether RIS restores the input encoding

- (a) **RIS sets UTF-8**, the state baud is made operational in (term.rs:529).
  CONFORMING to ECMA-48 8.3.105.
- (b) **Today.** RIS keeps Latin-1 (§3). DEPARTS from 8.3.105.
- Practice: xterm keeps the encoding across RIS. That is (b), and DEPARTS
  from ECMA-48 8.3.105. Found by reading, not run.
  - `CASE_RIS` calls `VTReset(xw, True, True)` (charproc.c:6149-6151).
    `VTReset` (14566) calls `ReallyReset` (14358), and neither writes
    `utf8_mode`.
  - In the pinned files, `utf8_mode` is written by `switchPtyData`
    (ptydata.c:343-344); by `modified_DECNRCM`, through
    `EXCHANGE(screen->utf8_nrc_mode, screen->utf8_mode, modefix)`
    (charproc.c:1295; the macro is ptyx.h:498); and at initialisation, in
    `VTInitialize_locale` (charproc.c:10052-10163), an assignment at 11618,
    `init_Ires` at 11649, and `VTInitialize` (11693).
  - `switchPtyData`'s callers are `CASE_UTF8`, the `ESC %` handler
    (charproc.c:6245), and `modified_DECNRCM` (1296), which `dpmodes` and
    `restoremodes` call for DECNRCM (7683, 8547). `ReallyReset` calls none
    of them.
  - Its callees defined outside the pinned files, such as the `update_*`
    menu functions, were not read.
  - foot has no Latin-1 mode.
- (a) conforms. Keeping (b) is a departure, and stays open. Test 27 is
  encoding-neutral until it is decided.

### Q18. `ESC SP L/M/N`, Announce Subset of Code Extension Facilities

**Built 2026-10-19**: option (a) at Levels 3 and 4, with the
conformance level DECSCL selects and Level 4 at power-up (ACT_AT_3_PLUS). DA1 still answers `?6c`.

**The texts.**

- 070-3 §3.8.1 (pp. 3-68 to 3-70) is the only receiver rule read. Its levels
  are "2X, 3", and the algorithm acts only when "(conformance_level > 2) ||
  (level_2_extensions[8bit_ia])".
  - L and M: ASCII into G0, ISO Latin-1 Supplemental into G1, G0 into GL, G1
    into GR.
  - N: ASCII into G0, G0 into GL.
- 070-3 §3.6.4.3 (p. 3-64): Level 3 "also recognizes" these announcers,
  "which re-initialize the designated and invoked character sets".
- ECMA-35 Table 7 (facilities 12-14) and ctlseqs 296-300 name the sequences
  as levels of ECMA-43 and give no receiver rule. 070 names the same finals
  as levels of ISO 4873. That the two standards define the same levels is
  inferred; neither standard was read (§8).
- After a reset, the announced state is gone: RIS, DECSTR and DECSCL restore
  the default designations and invocations (§3.8.1's NOTES; §3).

**Options.**

- (a) **Act as §3.8.1 says, at Level 3 or 2X.**
  - CONFORMING to 070-3 §3.8.1 at those levels. It adds no state, because
    the announcers only designate and invoke.
  - N needs neither GR nor a 96-set, and could land with stage (b). L and M
    need ISO Latin-1 Supplemental in G1 and GR, so stage (c) and Q3(a).
  - Under UTF-8 and P-A, only the GL half of L and M is visible, because GR
    bytes are UTF-8 there (Q5(a)).
  - A DOCS return restores "the state established by announcer functions"
    (ECMA-35 15.4.2). Q15(a)'s snapshot of `g`, `gl` and `gr` already holds
    it.
- (b) **Count, as today.**
  - DEPARTS from 070-3 §3.8.1 at Level 3 and 2X, the levels Q2's UPSS and
    Q3's 96-sets presuppose.
  - Matches §3.8.1 under the VT102 (Level 1) reading, where its algorithm
    does nothing.
- Practice: xterm maps `ESC SP L/M/N` to `CASE_ANSI_LEVEL_1/2/3`
  ([[source: xterm-vtprstbl-c]] VTPrsTbl.c:6416-6419).
  - Those cases call `set_ansi_conformance` (charproc.c:5070-5086), which
    sets `ansi_level` and nothing else, and only when `vtXX_level >= 3`
    (1311-1321).
  - That makes none of §3.8.1's designations or invocations, so it DEPARTS
    from §3.8.1.
  - Through Q9's gate it also turns LS2, LS3 and LS1R-LS3R off after
    `ESC SP L` or `M`, at DEC Level 3. That DEPARTS from 070-3 §3.6.1.1.
  - Its comment cites "VSRM - Documented Exceptions EL-00070-D"
    (charproc.c:1304-1309). 070-D's entry on this was not looked for (§8).
- **Recommend: follow the level Q9's reading settles.** Under Level 3 or 2X
  that is (a); under the VT102 reading, (b). Each conforms at its own level.
  Today's count departs under the first reading. Whether baud claims a
  level at which §3.8.1 applies is the decision's (Q9), so this is decided
  with Q9.

---

## 5. Hot-path design

**Rule #1 and rule #2, answered before any code.**

- No path adds cell writes per input byte.
- The working set gains:
  - two graph rows per encoding table, `EscInter` and `SingleShift`, plus
    whatever they add to the stride-composed tables. That is unmeasured;
    graph.rs:1179 pins 34 / 964 / 24,373 / 572,642.
  - two fields each in `Cold` (`gr`, `single`) and `Saved` (`gr` and the Q12
    field).
- Both need the "How to verify" A/B: `git archive` builds, an A/A copy,
  `cells_written` equal on every arm, and ranges.
- Whether any recording sends these sequences has not been checked.
  `Term::unsupported` per recording would say.

### State

- `Cold.gr: GrSlot`. `GrSlot` is a new type `{G1, G2, G3}` beside `GSlot`
  (charset.rs:289-294), so G0 in GR cannot be represented (ECMA-35 8.3.3).
- `Cold.single: SsSlot {G2, G3}`. It is meaningful only while the node is
  `SingleShift`.
- `Charset` gains `Latin1Supplemental` and a 96-series `Other`.
  `Charset::from_final96` covers the 96 series, which NOTE 44 makes separate.
- `Cold` grows, so term.rs:478 (`size_of::<Cold>() == 416`) is re-derived in
  the same commit.

### Entry: only a bare `ESC`, split at compile time

`escape()` (graph.rs:893-922) sends every intermediate back to `Node::Escape`
(`0x20..=0x2f => edge(Act::Inter, Node::Escape)`, graph.rs:919). An `N`/`O`
edge on that row would therefore also fire after `ESC SP`, `ESC (` or `ESC #`.
Then `ESC SP N` would be counted when unsplit (by `fast_esc`) and shifted when
split as `ESC SP | N` (by the graph). The fix is a second row:

- **`escape()` keeps the row for the byte straight after `ESC`.**
  - Its `0x20..=0x2f` arm targets a new `Node::EscInter`.
  - Its `0x4e`/`0x4f` arms target `(Act::Ss2 / Act::Ss3, Node::SingleShift)`.
- **`escape_inter()` is the row after one or more intermediates.**
  - C0 → `edge(c0(b), Node::EscInter)`. Not `Escape`, which would re-arm
    `N`/`O`.
  - `0x20..=0x2f` → `edge(Act::Inter, Node::EscInter)`, for a second
    intermediate (`ESC ( % 6`).
  - `0x30..=0x7e` → `edge(Act::EscFinal, Node::Ground)`.
  - Anything else → `Consume`, Ground.

  These are the edges `escape()` gives those bytes today, so every
  post-intermediate sequence behaves as it does now.
- **`in_sequence`** (graph.rs:679-682) gains `EscInter`, so an `ESC` abandons
  it. It is not `ground_like`, so CAN and SUB cancel it with no further edge.
  `ground_of`'s wildcard (graph.rs:998) already maps both new nodes to Ground.
  The node dispatch at graph.rs:735 gains their two arms.
- **The Latin-1 C1 edge** (graph.rs:871-873) reads the bare row through
  `escape(b - 0x40)`, so `8E`/`8F` enter `SingleShift`. D2 concerns the runtime
  `csi`, not the node.
- **`fast_esc`.**
  - The intermediate arm (write.rs:2195-2213) is unchanged: `ESC SP N` whole
    in one read still reaches `esc_final` and is counted.
  - The bare arm (write.rs:2220-2224) excludes `0x4e`/`0x4f`, as it already
    excludes the string introducers, so the graph walks `ESC N` and the
    protocol keeps one definition. `ESC N` is rare enough that the fast fold
    buys nothing.
- **Result.** `ESC SP N` takes the same path at every split: unsplit through
  `fast_esc`'s intermediate arm, split through `EscInter`'s `EscFinal`, both
  into `esc_final`'s intermediate branch (test 6). That branch counts it
  today, and acts on it under Q18(a).
- **The edge is compile-time, not a runtime override.** A runtime `*node = …`
  inside `esc_final` would be lost by any step that composes edges across
  `EscFinal` from that edge's own target, Ground. Whether the stride tables
  compose that way was not checked, and the compile-time edge is correct
  either way. `esc_final` gains no parameter.

### The pending single shift (Q6(a)): a node, with no check on the Ground row

This follows the VT52 sub-graph idiom (graph.rs:65-70). `Node::SingleShift` is
a row in both encoding tables. Its edges:

- `20-7F` → `Act::Shifted`, then Ground.
  - The table is built at compile time and cannot know whether `g[single]`
    has 94 or 96 characters, so `7F` always takes this edge.
  - For a 94-set, the act returns `ReadLen::ZERO` with Ground next, so DEL is
    re-dispatched (Q8; D4).
- Latin-1 table only: `A0-FF` → `Act::Shifted` with bit 8 stripped (Q7), then
  Ground. A stripped `FF` with a 94-set goes as `7F` does, and is re-dispatched
  as `FF` (Q8).
- `0E` / `0F` → `Act::ShiftOut` / `Act::ShiftIn`, staying in `SingleShift`.
- Every other byte → `Act::Again` (`ReadLen::ZERO`, write.rs:669), then
  Ground, which re-dispatches the byte.
  - That covers C0, `ESC`, Latin-1 C1 (so `8F` replaces `8E`) and UTF-8 lead
    bytes (Q4(a)).
  - CAN and SUB are taken first by `one()`'s non-ground rule.

Further properties:

- **Chunk boundaries cost nothing.** The node already survives across `feed`
  (feed.rs:298), so `ESC | N` and `8E | q` work.
- **`Act::Shifted`** is `#[cold] #[inline(never)]`.
  - It does `b & 0x7F` → `PackedCell::translate(b, g[single])` (cell.rs:363),
    with the 94/96 check for 0x20 and 0x7F.
  - It writes through a `glyph` that takes the cell's flags, so `last_cp`
    takes the translated glyph and REP repeats it
    (`rep_repeats_the_designated_glyph_not_the_raw_byte`, vt100/charsets.rs:638).
    Under Q16(a), decided, the cell takes `p.flags` as every write does; GL
    does not hold DEC graphics, so a shifted glyph carries no `GRAPHICS`.
- **The loop-advance gate must see the zero.**
  - `consumed()` (graph.rs:1618-1622) hard-codes `Act::Resync | Act::Again =>
    ZERO` and treats every other act as consuming its width.
  - `Act::Shifted` can return ZERO at run time, so it joins that arm. Then
    `every_step_of_the_loop_advances` (graph.rs:1706) checks that Ground
    consumes every byte `Shifted` can hand back: `7F` (`Act::Bad` today, the
    new DEL act after D4) and Latin-1 `FF` (`Act::Latin1`).
  - Without this, the gate passes against a hang it cannot see: the
    probe-that-cannot-fire shape that file records.
- **Graph costs:**
  - two new nodes;
  - three new acts (`Ss2`, `Ss3`, `Shifted`), and a fourth (`Del`) for D4;
  - `NNODE` (graph.rs:91), `FROM_INDEX`, `count_node`, `node_name`;
  - `in_sequence`; `consumed()`;
  - the ladder pins (graph.rs:1368-1371) and `CLASSES` (1596);
  - the graph tests at 1179, 1443, 1473, 1545, 1652, 1689 and 1706, whose
    pinned counts are re-derived and recorded.
- **Untouched:**
  - the Ground row, except `7F` (D4);
  - `Act::Text` (write.rs:324-330);
  - `piece`'s `k ≤ 2` loop (paint.rs:533-538);
  - `blit` (paint.rs:572);
  - `put_ascii_run` (grid.rs:839, branching per run at 901).

**Under Q6(b) instead.**

- A node cannot hold the shift, because `ESC 7` leaves it.
- `cold.single: Option<SsSlot>` is checked once per dispatch:
  - in the `Act::Text` arm (write.rs:324-330), before `p.text`, and never in
    `piece`, `wrapped` or `put_ascii_run`;
  - in the per-character arms that consume or cancel it: `Char2/3/4`
    (write.rs:310-312), `ContEnd` (428), `Latin1` (437), `Resync` (449) and
    `Bad` (670).
- `Saved` gains `single`.
- This costs a branch on the hottest arm, so it must be measured.

### DEL (D4)

Ground's `7F` edge becomes its own act, split from `Act::Bad` at compile time.
At run time it checks `g[gl]`:

- with a 94-set, the byte is ignored (070-3 §3.4.2; vt510-ch7 §7.2.5;
  williams);
- with a 96-set, it writes the set's character at 0x7F (ECMA-35 8.3.1;
  vt510-ch7 §7.2.5).

The check is on DEL, which is rare, and not on the text path. It edits a
Ground edge, so it takes the full A/B.

### GL and GR resolution

- `invoke()` (ops.rs:754-757) is unchanged: `charset = g[gl]`, which is what
  the bulk path reads.
  - LS2 and LS3 call it, as SO and SI do (write.rs:388-397).
  - Designation already calls it (write.rs:1701), and so does `restore`
    (ops.rs:112).
- **GR is not resolved into `Paint`.**
  - Only two cold per-character arms read it: `Act::Latin1` (write.rs:437-443),
    through `p.cold.g[p.cold.gr.index()]`, and `Act::Shifted`.
  - A cached copy would be one more value for `invoke()` to keep fresh, which
    its doc (ops.rs:747-753) argues against.
  - Because GR is read at use, a designation into the slot GR holds takes
    effect at once (ECMA-35 14.3.2).
- **A 96-set in GL** (`ESC - A`, then SO):
  - `with_charset` (cell.rs:389) gains a `Latin1Supplemental` arm mapping
    0x20-0x7F to U+00A0-U+00FF.
  - `put_ascii_run` already branches once per run on `set != Ascii`
    (grid.rs:901), so no branch is added.
  - 0x7F goes through the DEL act above.
- **LS1R-LS3R** write `cold.gr` only.

### What `Saved` gains (term.rs:55-89)

- `gr: GrSlot`, saved by `save()` (ops.rs:69-81) and restored before
  `invoke()` in `restore()` (ops.rs:109-112).
- The Q12 field. If pending wrap is saved, `restore` keeps `cx = cols` instead
  of clamping (ops.rs:99-100) when the width is unchanged.
  2026-10-13: `Saved::pending` exists, records only whether the saved `cx` is
  the pending-wrap sentinel (`cx == right + 1`), and decides nothing of Q12.
- `single`, only under Q6(b).
- `AltSaved`/`?1049` (ops.rs:85-93, 1203-1204) carry `Saved` and inherit all of
  this.

### DECRC with nothing saved (Q13(ii))

DECRC's `None` arm does four things:

1. homes the cursor;
2. resets DECOM;
3. sets `style_raw` to `StyleWord::DEFAULT` and re-interns it;
4. resets `g`, `gl` and (from stage (c)) `gr` to the defaults, and calls
   `invoke()`.

SCORC keeps calling the current `restore`, unchanged.

**As built (2026-10-11), five**: these four, with GR still unmodelled, and
`protect` cleared, the selective erase attribute 070's reset buffer and
vt220-rm-ch4 Table 4-10 hold off (Q13's 2026-10-11 entry). The `style` is
set to `StyleId::DEFAULT`, slot 0, rather than re-interned.

---

## 6. Red-first tests

Unless marked, each test is red at `d894ebb` for the reason given.

- **Guard** tests are green today and must stay green. Each one names the
  perturbation that turns it red.
- Every case runs at every read size, in the shape of `esc_abandons.rs` and the
  `each_read_size` helpers.
- Each test carries its build stage from §9: (a), (b), (b') or (c).

**Single shifts and locking shifts: `tests/vt220/shifts.rs`** (new; add
`mod shifts;` in `vt220/main.rs`)

Tests 1 and 2 expect LS2 and LS3 to act. They presuppose Q9(a) or (c), or a
Level 2-4 reading of DA. Under Q9(b) with the VT102 reading, `ESC n` and
`ESC o` stay counted, and the two tests are written the other way round.

1. (b) Feed `ESC * 0 ESC n q SI q`. Assert `─`, `q`, and `Term::charset()` is
   `DecGraphics` after `ESC n`. Red: `ESC n` is counted, so the first cell is
   `q`.
2. (b) Feed `ESC + 0 ESC o q`. Assert `─`. Red: counted.
3. (b) Feed `ESC * 0 ESC N q q`. Assert `─`, `q`. Red: `q`, `q`.
4. (b) Feed `ESC + 0 ESC O q q`. Assert `─`, `q`. Red.
5. (b) **Chunk boundaries.** Feed (3)'s bytes split between `ESC` and `N`,
   between `N` and `q`, and at every read size from 1 to the length. Assert
   (3)'s cells. Red: as (3).
6. (b) **A final after an intermediate is not a single shift (HIGH 1).**
   - Prefix: `ESC * 0 ESC + 0`. For each of `ESC SP N`, `ESC SP O`, `ESC ( N`,
     `ESC * O` and `ESC # N`, feed prefix + sequence + `q`.
   - Run at every read size and at each split inside the sequence. Assert that
     screen, cursor and `unsupported` equal the unsplit run.
   - For `ESC SP N` (ctlseqs 300; Table 7 facility 14; 070-3 §3.8.1) and
     `ESC SP O` (facility 15, reserved by NOTE 52), also assert that cell 0
     is `q`. That holds under either answer to Q18, because G0 already holds
     ASCII and GL is already G0.
   - The test does **not** assert that `unsupported` rose for `ESC SP N`.
     Counting it departs from 070-3 §3.8.1 at Level 3 (§2; Q18), so the test
     pins only split = unsplit. `ESC SP O` has no function in any text read,
     and its count may be asserted.
   - **Guard** today. Its red-first rung is a perturbation: build the naive
     single-row version (the Escape row's `N`/`O` → `SingleShift`, with
     intermediates still returning to `Escape`). The `ESC SP | N` split then
     draws `─`. Record that run, then build `EscInter`.
7. (b) **SO between the shift and its character (Q6(a)).** Feed `ESC ) A ESC *
   0 ESC N SO q # SI #`. Assert `─`, `£`, `#`. Red: the first cell is `q`.
8. (b) **A control cancels the shift (Q6(a)).** Feed `ESC * 0 ESC N CR q`.
   Assert `q` at column 0, not `─`. **Guard** (`ESC N` is ignored today). It
   goes red under Q6(b), or if the cancel edge is dropped.
9. (b) **foot's bug.** Feed `ESC * 0 ESC N CSI m q q`, and `ESC * 0 ESC N ESC ]
   8 ; ; x ESC \ q q`. Assert `q`, `q`: the shift was neither applied nor
   locked. **Guard**. It goes red if SGR or OSC 8 resets GL to `g[single]`.
10. (b) **UTF-8 after SS2 (Q4(a)).** Feed `ESC * 0 ESC N é q` and `ESC * 0 ESC
    N 中 q`. Assert `é`, `q` and `中`, `q`. **Guard**. It goes red if a
    non-ASCII character fails to consume the shift.
11. (b) **REP after a shifted character.** Feed `ESC * 0 ESC N q CSI 2 b`.
    Assert `─ ─ ─`. Red: `q q q`.
12. (b) **A shifted glyph takes GL's flags (Q16(a), decided).** Feed
    `ESC * 0 ESC N q q`. Assert `─`, `q`, and `GRAPHICS` on neither cell: GL
    holds ASCII throughout. **Guard** for the flag (no cell carries it today);
    the glyph half is red with test 3.

**C1 and GR operands: `tests/encoding/latin1.rs`**

13. (b) **`8E` under Latin-1.** Feed `ESC % @ ESC * 0 8E q q`, unsplit and split
    between `8E` and `q`. Assert `─`, `q`, and `charset_in(G2) ==
    DecGraphics`. Red: counted (and before D1's fix, D2 makes G2 `Other`).
14. (b) **`8E` under UTF-8.** Feed `ESC * 0 8E q`. Assert U+FFFD, `q`.
    **Guard** (ctlseqs 56-65).
15. (b) **SS3 operand from GR (Q7).** Feed `ESC % @ ESC + 0 ESC O F1 F1`.
    Assert `─`, `ñ`. Red: `ñ`, `ñ`.

**DECSC, DECRC and the save buffer**

16. (b) **DECSC/DECRC with a pending shift.** Feed `ESC * 0 ESC N ESC 7 ESC 8
    q`.
    - Under Q6(a), assert `q`: the `ESC` cancelled the shift, so nothing was
      saved. **Guard**.
    - Under Q6(b), also feed `ESC * 0 ESC N ESC 7 q ESC 8 q` and assert `─`,
      `─`. Red.
17. (a) **D1 and D2.** Landed in `bdd59c6`:
    `eight_bit_ind_nel_hts_and_ri_match_their_seven_bit_forms` and
    `a_c1_control_does_not_inherit_the_last_escapes_intermediate`, in
    `encoding/latin1.rs`. Each holds an 8-bit form against its 7-bit twin at
    every read size, and each was red, one mechanism at a time, before the
    fix.
18. (a) **G-slot save, which no test pins today.** Feed `ESC ) 0 SO ESC 7 SI
    ESC ) B ESC 8 q`. Assert `─`.
    - Green today. Red-first by perturbation: drop `self.cold.g = s.g`
      (ops.rs:109) and see it fail.
    - Restore the line before committing.
19. (b') **DECRC with nothing saved.** Feed `CSI ? 6 h CSI 1 m ESC ( 0 CSI 5;5H
    ESC 8 q`. Assert `q` at (0,0), not bold, `GRAPHICS` clear, DECOM reset.
    Red: `restore(None)` is a no-op. GR's default is added at (c).
20. (b') **After DECSTR.** Feed `CSI 5;5H ESC 7 CSI ! p CSI 9;9H ESC 8`. Assert
    the cursor at (0,0) (vt510-decstr "Home position"). Red: `saved = None`,
    so the cursor stays at (8,8).
21. (b') **After RIS.** Feed `ESC c CSI 5;5H ESC 8`. Assert (0,0). Red.

**GR, 96-sets and resets (stage (c))**

Tests 23, 26 and 27 expect LS1R-LS3R to act. They presuppose Q9(a), or a
Level 2-4 reading of DA. Under Q9(b) or (c) with the VT102 reading, those
sequences stay counted, and the tests change with that answer. Tests 23, 26,
27 and 29 also presuppose Q1(a), GR starting at G2: under Q1(b) GR starts at
G1, so `ESC ~` becomes the no-op where `ESC }` is one under (a), and the
default-GR glyphs change.

22. (c) **The Latin-1 control.** In `encoding/latin1.rs`, for every `b` in
    `A0..=FF`, feed `ESC % @ b`. Assert U+00`b`.
    - **Guard**, and the control for Q2.
    - It goes red under Q2(a) at the 20 positions of §4, and under Q2(c) at
      all 96. Under Q4.1(β) it holds only once the first `ESC % @` installs
      Q2's value (Q4.1); without that install it goes red at all 96.
23. (c) **LS1R-LS3R under Latin-1.** Feed `ESC % @ ESC ) 0 ESC ~ F1`, then `ESC
    } F1`. Assert `─`, then the Q2 default's F1 glyph (`ñ` under Q2(a) and
    (b)). Also feed `ESC % @ ESC + 0 ESC | F1` and assert `─`. Red: `ñ` every
    time.
24. (c) **Designation into the slot GR holds (14.3.2).** Feed `ESC % @ ESC * 0
    F1`. Assert `─`. Red: `ñ`.
25. (c) **96-set designation.** In `vt100/charsets.rs`:
    - Feed `ESC - A SO a`, `ESC - A SO SP` and `ESC - A SO 7F`. Assert U+00E1,
      U+00A0 and U+00FF. Red: `ESC -` is counted, and the cells are `a`, a
      space, and for `7F` U+FFFD (D4), or no cell once D4 lands.
    - Feed `ESC ) A SO #` and assert `£` (the 94 series, NOTE 44). **Guard**.
26. (c) **DECSC/DECRC round trip of GR.** In `vt100/cursor.rs`, feed `ESC % @
    ESC + 0 ESC | ESC 7 ESC } ESC 8 F1`. Assert `─`. Red: `ESC |` is counted.
27. (c) **Resets restore GL, GR and the slots, encoding-neutral.**
    - (27a) DECSTR: feed `ESC % @ ESC + 0 ESC | F1 CSI ! p F1`. No text read
      says DECSTR changes the encoding.
    - (27b) RIS: feed `ESC % @ ESC + 0 ESC | F1 ESC c ESC % @ F1`. The second
      `ESC % @` keeps the test from pinning Q17, **unless Q4.1 is (β)**.
    - Assert `─`, then the Q2 default's F1 glyph, and that `charset_in(G2)`
      and `charset_in(G3)` equal the Q2 default after each reset. Red: `ESC |`
      is counted.
    - **Under Q4.1(β), 27b's glyph could pin Q17**, and so it asserts none.
      - Under Q17(a), RIS enters UTF-8, where (β) puts ASCII in G2 and G3,
        and the next `ESC % @` installs Q2's value only if it counts as a
        first return (Q4.1's install rule) after a reset.
      - Under Q17(b), RIS stays in Latin-1, and F1 draws the Q2 default.
      - So under (β), 27b checks the state between `ESC c` and the second
        `ESC % @`: `Term::gr()` is G2 (under Q1(a)), and G2 and G3 equal the
        default for `input_encoding()` as it stands then. It asserts no F1
        glyph after the reset, and leaves that to the Q17 test.
    - A Q17 test is written only once Q17 is decided.
28. (c) **Pending wrap (Q12, if pending wrap is saved).** At `support::CORPUS`
    (50×200), feed `CSI 1;200H x ESC 7 CSI 1;1H ESC 8 y`. Assert `y` at (1,0)
    and `x` still at (0,199). Red: the clamp puts `y` over `x`.

**Conformance and properties**

29. (b) then (c): **`spec_inventory`** (`conformance/spec_inventory.rs:474`).
    - `PREAMBLE` (line 323) designates nothing today, and `PROBE` is `b"X"`
      (line 336). Change both.
      - The preamble gains `ESC ) A ESC * K ESC + 0`: UK in G1, German in G2,
        DEC Special Graphics in G3. G0 stays ASCII and GL stays G0.
      - All three are sets baud translates. `with_charset` (cell.rs:389-398)
        translates `DecGraphics`, `Uk` and `National(_)`; every other set,
        DEC Technical among them, falls to `with_ascii` and is drawn
        untranslated.
      - The probe becomes `b"{X"`. A single shift reaches only the probe's
        first byte, so that byte must be one that both G2's set and G3's set
        change. German maps `{` to `ä` (nrcs_data.rs:73), and DEC Special
        Graphics maps it to `π` (dec_graphics.rs:43).
      - No byte would serve with UK in G2: UK changes only `#`
        (nrcs_data.rs:234), and DEC Special Graphics only 0x60-0x7E.
      - `X` stays as the second byte, so the cell the old probe wrote is still
        written.
      - `snapshot` (338-391) feeds the probe as its own chunk, so the probe
        also crosses a `feed` boundary with a shift pending.
    - What each form changes from that state once it acts:
      - `ESC N`: the probe's `{` draws `ä` from G2. `ESC O`: it draws `π` from
        G3. Neither changes `Term::charset()`, which is the GL set
        (term.rs:1711-1713).
      - `ESC n` / `ESC o`: `Term::charset()` becomes German / DEC Special
        Graphics, and the probe translates through it.
      - `ESC ~` / `ESC |`: `Term::gr()` moves from G2 to G1 / G3.
      - `ESC }`, under Q5(i): GR already holds G2 in the prepared state
        (Q1(a)), so LS2R rewrites G2 into GR and changes nothing. Under
        Latin-1 that is ECMA-35 8.3.4's rule ("the occurrence of the
        corresponding locking-shift function shall have no effect"); under
        UTF-8 and P-A no ISO clause governs, and the no-op holds anyway.
        At (c) it joins the ESC test's UNOBSERVABLE list with that reason,
        and test 23 holds it. Moving GR off G2 in the shared preamble would
        only make `~` or `|` the silent one instead. Under Q5(ii), `ESC }`
        is LS2 and moves GL, and under Q5(iii) it is counted; either way it
        is visible, and the entry is not added.
    - Add `MUST_ACT`, asserting acted-on with no rise in `unsupported`. It is
      `[N, O, n, o]` at (b), and `[|, ~]` join it at (c). Red: all are counted
      today.
      - `N` and `O` presuppose Q4(a), and `|` and `~` presuppose Q5(i) or (ii).
      - `n` and `o` presuppose Q9(a) or (c), and `|` and `~` presuppose Q9(a);
        either way, a Level 2-4 reading of DA satisfies them.
      - Under Q4(b), Q5(iii), or a Q9 answer that gates them, those forms
        stay counted and stay out of `MUST_ACT`.
    - **The new preamble makes three ESC-intermediate forms no-ops.**
      - `every_spec_esc_intermediate_form_is_handled_or_counted` (769) feeds
        every charset designation from the prepared state. Against it,
        `ESC ) A`, `ESC * K` and `ESC + 0` designate a set into the slot that
        already holds it, and would be reported silent.
      - Its `already_true` list (801-811) gains those three, under the reason
        it already gives for `B`: designating a set into a slot that already
        holds it.
      - Its `B` entry narrows to `(`, the one slot that still holds ASCII.
      - Under Q18(a), `ESC SP N` joins the list too. From the prepared state
        it designates ASCII into G0 and invokes G0 into GL, and both are
        already so.
    - The CSI, OSC and DCS inventories share `PREAMBLE` and `PROBE`.
      - G0 stays ASCII in GL, so a form that touches neither GL nor the slots
        writes the probe as `{X` in both arms.
      - DECSTR and RIS reset the slots, so they become more visible, not
        less.
      - This was checked by reading and has not been run.
    - `Term::gr()` is added at (c) so that `snapshot` sees the GR slot.
      `api_coverage.rs:70` requires a test that calls it.
    - The `'8'` UNOBSERVABLE entry (line 502) is removed once (19) holds.
30. (b) then (c): **`enumerated.rs` reaches the new rows.**
    - Write the new `vt-coverage.md` rows as **done** first.
      `every_done_sequence_reaches_a_handler` then goes red for each until its
      sequence acts.
    - `ESC |` is reachable, because `8cf753d` made `cells()` keep an escaped
      `\|` inside its cell (`an_escaped_pipe_does_not_split_a_row`; row 725
      surfaced there).
31. (b) **`properties.rs` atoms.** In `ALPHABET` (properties.rs:71-185), add:
    - `ESC * 0` (stride 0, count 0);
    - `ESC N` (stride 0, count 0);
    - `ESC SP N` (stride 0, count 0; it reaches `EscInter`, and what it does
      there is Q18);
    - `ESC n`, `SI`;
    - `ESC % @` / `ESC % G`.

    `drive_atomwise` (properties.rs:229) then feeds a shift and its character
    in separate calls.
    - The red-first rung is the positive control in
      `the_alphabet_emits_what_it_claims` (248): the pair `ESC * 0`, `ESC N`
      followed by `ascii` emits `─` first. Red today.
    - The chunk-invariance and atomwise properties are guards. Run them at
      `PROPTEST_CASES=20000`.
32. (a) **DEL in Ground (D4).** Feed `a DEL b` under UTF-8, and `ESC % @ a DEL
    b`. Assert `a` and `b` adjacent and `replacements` unchanged. Red by
    reading: a U+FFFD between them. If it is green, D4 is wrong and is struck.
33. **Updated, not new.** These pin today's behaviour and change with it:
    - `vt100/charsets.rs`: `all_four_slots_are_independent` (77; its comment at
      79-80), `ris_resets_the_charsets` (261),
      `charset_handling_is_chunk_invariant` (268);
    - `xterm/soft_reset_scope.rs` (208, 241, 543; the charset arm at 577-583);
    - `esc_abandons.rs:117` (its comment);
    - every assertion that G2 or G3 default to ASCII (stage (c)).

---

## 7. Docs that change in the same commits

**`docs/vt-coverage.md`.**

- **Stage (a) corrections:**
  - lines 213-216 and 423-426 ("8-bit C1 controls are therefore out"; "`80..9F`
    stay ignored in Latin-1") are stale;
  - row 234's `84 85 88 8D`: done in `bdd59c6`, with the heading and the `80..9F` paragraph;
  - DECSC's "the one slot this core has" (540) is stale;
  - a DEL row (D4);
  - an `ESC SP L/M/N` row. There is none today. It records the count as an
    open departure from 070-3 §3.8.1 at Level 3 until Q18 is answered.
- **Stage (b):**
  - new rows for `ESC N`, `ESC O`, `ESC n` and `ESC o`;
  - row 234 gains `8E 8F`;
  - SO/SI (533-534) are named LS1/LS0, and the rows say they act inside a
    single shift.
- **Stage (c):**
  - new rows for `ESC }`, `ESC ~`, `ESC \|`, `ESC - ` / `ESC . ` / `ESC / `,
    `ESC - A`, and GR bytes under Latin-1;
  - designation (550): G0-G3; GL by SI, SO, LS2 and LS3; GR by LS1R-LS3R; GR
    defaults to G2;
  - row 246 splits by encoding; row 247's
    "one byte, one character" becomes "one byte through `g[gr]`";
  - the encoding heading (213) changes;
  - DECSC (540) and DECRC (541) go to `done`, listing every item including GR
    and the Q12 outcome, and stating Q6 and Q13;
  - legend line 36 ("Three rows carry it") and paragraph 56-63 drop to one row,
    DEC Special Graphics;
  - a DOCS-return row (Q15), marked as an open departure until it lands.

**The rules file.** These edits must not grow the file, which is held to its
budget.

- Line 534: "Closed but for DECSC, DECRC and DEC Special Graphics" becomes
  "but for DEC Special Graphics" (stage (c)).
- Lines 717-718: "done but for DECSC/DECRC and DEC graphics (partial)" becomes
  "but for DEC graphics (partial)" (stage (c)).
- Line 751: "8-bit C1 controls work under Latin-1" became true of every C1
  `esc_final` handles when D1 landed (`bdd59c6`). Keep the sentence.

**Other docs and code comments.**

- `docs/protocols/vt100.md:60` ("SO and SI invoke G1 and G0"): add LS2/LS3,
  SS2/SS3 and GR.
- Code comments:
  - term.rs:63-68 ("`GR` is not saved because this core does not model it");
  - esc.rs:19-21 (the DECSC list lacks SS2/SS3 and GR);
  - cell.rs:166-171 (`GRAPHICS`: its two sentences, which Q16 reconciles);
  - write.rs:1719-1723 ("`ESC SP L/M/N` … counted rather than dropped"), if
    Q18 makes them act;
  - charset.rs:164: "User Preferred Selection Set" becomes "User Preference
    Supplemental Set", per 070;
  - `dcs.rs:8`: the DCS string is DECAUPSS, and DECRQUPSS is `CSI & u`
    ([[source: vt510-decrqupss]]);
  - vt100/charsets.rs:79-80, spec_inventory.rs:497-503 and esc_abandons.rs:117.

**The record.** A dated record per stage, holding:

- the §4 Q2 table check;
- the decisions, each with its reason;
- the A/B of §5, with `cells_written` per arm;
- the red-first evidence for each test in §6, including test 6's naive-edge
  run.

**`docs/research/sources/MANIFEST.toml`.** The keys listed in the header,
pinned by the parallel change.


- **Do not close the SS2/SS3/LS/96-set item (1953).** Its acceptance text is
  "then DECSC/DECRC save a single shift in flight and move to `done`". Under
  Q6(a) no shift is ever in flight at DECSC: the `ESC` of `ESC 7` cancels it.
  Flag the mismatch, so the acceptance text can be amended.
- **The DECRC/SCORC item (323):** the DECRC half closes at stage (b'); the
  SCORC half stays open until SCO's text is read (Q13).
- **New entries:** D3 (read the VT52 text), D4, Q17 and Q18. (An earlier
  draft listed D5 as well; it is Q16's definitional point, not a defect.)

---

## 8. Unread or unverified

**Defects found by reading, none executed.**

- **D1.** A Latin-1 C1 final reached `esc_final` as the raw byte. Section 2
  has the detail. Fixed in `bdd59c6` (test 17).
- **D2.** A stale intermediate sent a C1 byte to the designation arm. Section
  2 has the detail. Fixed in `bdd59c6` (test 17).
- **D3. A Latin-1 C1 byte leaves VT52 mode.**
  - Under Latin-1, the ground row's C1 branch (graph.rs:871-873) returns
    `escape(b - 0x40)`, whose edges all name ANSI nodes (`Escape`, `CsiEntry`,
    `Str`, `Dcs`, `Ground`; graph.rs:893-922). `ground_of` is never applied
    there.
  - So in VT52 mode one C1 byte switches the parser to ANSI. For example, `84`
    ends at `Node::Ground` and `9B` enters an ANSI `CSI`.
  - That contradicts the file's own rule for CAN, "Back to the ground it was
    in, not to ANSI ground" (graph.rs:660-662).
  - **What the byte should do instead is unread.** vt220-rm-ch4 Table 4-3
    ("The 8th bit of all received characters is set to zero") covers Level 1,
    VT100 mode. dec-special-graphics §2.7 ("C1 and GR are meaningless in VT52
    or VT100 modes") covers display-controls mode. Neither text is about VT52
    mode's received 8-bit codes.
  - Settling D3 needs the DEC text for VT52 mode's handling of received 8-bit
    bytes. Until it is read, D3 is flagged and not built.
- **D4.** DEL in Ground wrote U+FFFD (§2). 070-3 §3.4.2 says DEL "shall be
  ignored", and vt510-ch7 §7.2.5 "Ignored (unless in CRM)". Confirmed red by
  test 32 (`del_in_ground_is_ignored`) and fixed in both grounds, VT52's
  included.
- **D5 is withdrawn.** It said `GRAPHICS` lands on cells that were not
  translated. The flag's doc defines it as the set invoked into GL when the
  cell was written (cell.rs:166-167), and that is what the code does. Which
  definition to keep is Q16.

**Other open items.**

- **ISO-IR register entries**, including 6 and 100 (NOTE A.3), and what each
  final names. No register is in MANIFEST. Q2(b)'s identity claim rests on
  NOTE A.3 plus [[source: unicode18-ch2]].
- **Other unread texts:**
  - ISO/IEC 10538 annex B (NOTE 23's single-byte SS2/SS3);
  - ISO 2022 and ISO 4873 as such, which 070 cites. 070-3 §3.8.1 gives the
    receiver's designations and invocations for ISO 4873's Levels 1-3, and
    that was read (Q18). ISO 4873 itself, and ECMA-43, which ECMA-35 Table 7
    and ctlseqs name for the same finals, were not;
  - 070-D's entry on ANSI conformance levels, which xterm's comment cites
    (charproc.c:1304-1309): the pinned Rev AX11's contents page (PDF p. 1190)
    lists D.1-D.10 -- RIS, Answerback, DECTST, SRM, Auto Wrap, CRM, DECALN,
    Local Function Keys and the change history -- and has none [text]. Which
    revision xterm's comment read is unknown;
  - 070-1, 070-6 and 070 Appendix A;
  - SCO's text for SCOSC/SCORC (Q13);
  - the VT52-mode text (D3).
- **Not shown in 070:** whether `parse_ansi` turns `ESC N` into `0x8E`.
- **Not found:**
  - VT220's G1/G3 defaults in VT200 mode (positive control: "G0, G1, G2, G3"
    hits in Table 4-10);
  - VT510's per-slot defaults, beyond vt510-deccir's example.
- **Not traced in practice:**
  - how xterm draws a 96-set in GL;
  - [[source: xterm-charsets-c]] 402-410, read only in part;
  - the callees of xterm's `ReallyReset` that are defined outside the pinned
    files (Q17).
- **CRM is out of this note's scope.** vt510-ch7 §7.2.5 makes DEL and 255
  "Ignored (unless in CRM)". baud has no control-representation mode: `CRM`
  appears nowhere in `baud-core/src` or `docs/vt-coverage.md` (grep), so D4
  and Q8 hold for baud as it is.
- **Not pinned:** vt220-rm-ch4's Figure 4-2 is an image the pinned HTML
  links. It was read from a copy beside the research files and is not in
  `MANIFEST.toml` (Q2, Q9).
- **In baud, by reading only:**
  - VT52's `designate` writes `g[gl]` (vt52.rs:78-81). After LS2/LS3, `gl` can
    be G2 or G3, and what DEC wants then is unread.
  - The `Cold` and `Saved` sizes after the change.
  - Pending wrap across a resize (Q12).
- **Not checked:** whether the stride-composed tables compose edges across
  `EscFinal`. §5's compile-time edges do not depend on the answer.
- **Unmeasured:**
  - the two nodes' cost in the stride-composed tables;
  - Q6(b)'s branch;
  - whether any recording sends these sequences.
- **Conflicts recorded and not needed here:**
  - C10 (whether DECSCL resets soft or hard);
  - C11 (the default C1 transmission);
  - C17 (the S7C1T/S8C1T codes in the vt100.net transcriptions);
  - C18 (070's swapped `(`/`)` labels).

---

## 9. Build order

The work splits by what each piece waits on.

**What is open, and why.**

| Question | Status | Why |
|---|---|---|
| Q1, Q2 (with Q4.1), Q8, Q9, Q11, Q12, Q17 | **Open** | No option is free of departure (Q1 under the 8bit-level1 reading of `ESC % @`; Q9 under the VT102 reading of DA; both readings a choice), or the conforming one collides with a practice baud follows (Q8 against the SUB precedent) |
| P, Q4, Q5, Q7, Q15, Q16 | **Recommended, free of departure** | Q4 and Q16 decided (§10), and Q5's GR-bytes half later; P, Q5's LS1R-LS3R half, Q7 and Q15 stayed open |
| Q3 (vt510-ch7 §7.3 under the VT102 reading), Q6 (its two sub-points), Q10 (8.3.4 read literally), Q13 (xterm's SCORC; 070-5's reset buffer), Q14 (070-3 p. 3-63 and vt510-scs once the UPSS is not DEC Supplemental), Q18 (070-3 §3.8.1, by level) | **Recommended, departing** | The recommended option carries a stated departure, or which option conforms turns on Q9's level reading |

This is the state of 2026-10-08. What the 2026-10-19 build and AE8 changed
since is in the first bullet at the head of this note.

**(a) Buildable now, with no decision.**

- **D1 and D2: landed in `bdd59c6`.** `Act::EscFinal` maps with
  `c1_as_escape` and resets `csi` on the C1 path (test 17). `8E`/`8F` stay
  counted, as `ESC N`/`ESC O` are, until (b).
- **D3: read first, do not build.** §8 says what must be read.
- **The `rows()` fix for an escaped `\|`: landed in `8cf753d`**, with row 725
  surfaced. Nothing is left to do.
- **Test 18**, the G-slot save pin, red by perturbation.
- **D4 (test 32)**, once the red test confirms the reading. It edits Ground's
  `7F` edge, so it takes the full `ingest2` A/B (rule #2). The 96-set branch of
  the DEL act waits for (c).
- **The §7 stage (a) doc corrections.** Also `dcs.rs:8` and charset.rs:164.

**(b) Once P and Q6 with its sub-points are decided (Q4 and Q16 are
decided; Q7 for the part it governs). LS2/LS3 also wait for Q9.**

- What is built:
  - `EscInter` and `SingleShift`;
  - `Act::Ss2`/`Ss3`/`Shifted`;
  - the `consumed()` change;
  - `fast_esc`'s bare-arm exclusion;
  - LS2/LS3.
- **G2 and G3 stay ASCII until Q2**, so only explicitly designated sets show a
  change.
- Tests: 1-16, 29 and 30 (the `N O n o` half), and 31. Tests 1 and 2, and
  the `n o` half of 29 and 30, also wait for Q9.
- This edits the graph and the stride tables, so the full `ingest2` A/B is
  required (rule #2), with `cells_written` equal on every arm.
- **(b') Once Q13 is decided** (no answer leaned, §10): DECRC's `None`
  arm (tests 19-21), without GR.
  It is independent of the graph.
  **Built 2026-10-11 without waiting**, because both Q13 options give DECRC
  the same result but for DECAWM, which it leaves alone, and every DEC text
  read agrees on the rest (Q13's 2026-10-11 entry); what the answer
  still decides is SCORC.
- **`ESC SP N`, if Q18 is (a):** ASCII into G0 and G0 into GL, in `esc_final`'s
  intermediate branch, with no GR and no 96-set needed. It may land with (b)
  or on its own, once Q18 is decided. `ESC SP L` and `M` wait for
  (c).

**(c) Once Q1 (GR's initial slot), Q2 (with Q4.1 and Q8, which follow from
it), Q9 (for LS1R-LS3R), Q12, Q3 and Q5 (LS1R-LS3R under UTF-8) are
decided.**

- The GR half:
  - `Cold.gr`;
  - LS1R-LS3R;
  - GR translation in `Act::Latin1`;
  - 96-sets (`ESC - . /`, `Latin1Supplemental`, the DEL act's 96-set branch);
  - DECSC/DECRC saving GR;
  - `Term::gr()`;
  - the Q12 wrap field;
  - G2/G3 taking Q2's value;
  - `ESC SP L` and `M`, if Q18 is (a).
- Tests: 22-28, the rest of 29 and 30, and 33.
- **Only then do the DECSC and DECRC rows reach `done`.**
- Q15 (DOCS restore) may land as its own commit once it is decided.
- Q17 is built once it is decided.

---

## 10. The answers, 2026-10-08

Every question in section 4 was weighed at once, with the governing texts
quoted and every option marked conforming or departing as this note marks it.

| Question | Leaning answer | Status |
|---|---|---|
| P | P-A, UTF-8 is not an ISO 2022 code | open |
| Q1 | (a) G2 | open |
| Q2 | (a) DEC Supplemental | open |
| Q3 | (a) now, in part | open |
| Q4 | (a) act | **decided** |
| Q4.1 | (α) Q2's value in both encodings | open |
| Q5, GR bytes | (a) no GR | **decided** later (below), AE5 part 1 |
| Q5, LS1R-LS3R | (ii) 9.3.2's 7-bit meaning | open; conflicts with P's answer (below) |
| Q6 | (a) 070's cancel rule | open |
| Q6, how many SO/SI | none | open |
| Q6, status after `SS2 SO x` | SO acts | open |
| Q7 | (a) strip bit 8 | open |
| Q8 | (d) 070: A0 the error character, FF ignored | open |
| Q8, the error character's codepoint | U+2426 | **decided** later (below), AE8.1 |
| Q9 | (a) ungated | open |
| Q10 | the per-area reading | open |
| Q11 | slots | open |
| Q12 | (iii) both | open |
| Q13 | none | open |
| Q14 | `<` designates the UPSS | open |
| Q15 | (a) snapshot and restore | open |
| Q16 | (a) as today | **decided** |
| Q17 | none | open |
| Q18 | (a) act as 070-3 §3.8.1 says | open |

This is the state of 2026-10-08. What the 2026-10-19 build and AE8 changed
since is in the first bullet at the head of this note.

**The answers on P and Q5 conflict with each other.** P-A and Q5's LS1R-LS3R
option (ii) cannot both hold: §4's table makes (ii) required only under P-B;
under P-A no text decides, and (i) is the note's recommendation. They go for
decision flagged, as note AD's AD1 conflict did.

**Where no answer leaned:** Q6's how-many sub-point, Q13, Q17 (where the
nearest answer was the departing option) and Q8's codepoint (since decided,
below).

**What the two decisions change.** Q4(a) is what §5 and tests 3-10 already
assume. Q16(a) reverses this note's recommendation: §4's Q16, §5's flags
sentence and test 12 are corrected above.

### Decided since

- **AE8.1, the error character's codepoint: U+2426, decided.** Weighed again
  with both code charts read and every option marked (note AF's Q-UNDEF). It
  conforms: 070-3 §3.4.3 leaves the image open ("usually imaged as"). Built as
  `baud_domain::DEC_ERROR_CHARACTER`, emitted beside the national tables by
  `scripts/gen-nrcs-tables.py` and held to its Unicode name by
  `the_dec_error_character_is_symbol_for_substitute_form_two`
  (`baud-domain/tests/nrcs_dec.rs`). Only the codepoint is decided: Q8 itself
  (what A0 and FF do with a 94-set in GR, AE8), SUB's display and note AF's
  Q-UNDEF still decide where it is drawn. Since 2026-10-12 it is drawn at
  DEC Supplemental's and JIS-Katakana's reserved positions (note AF, Q-UNDEF).
  A0 and FF with a 94-set in GR are decided too, AE8 option (d), 070's, the
  only option that conforms to every text read (Q8), and built in
  `Paint::gr_glyph`: A0 draws the error character and
  FF is ignored. SUB and the other sets' reserved positions still wait.
- **AE5 part 1, GR bytes under UTF-8: (a) no GR, decided.** Weighed again on
  its own with `[[source: unicode18-ch3]]` C4 and D92 added to the governing
  texts, each option marked as §4 marks it. It conforms: under P-A UTF-8 decides 80-FF, and under P-B ECMA-35 9.1
  gives a 7-bit code no GR. Nothing changes in baud, which already reads
  `A0-FF` as UTF-8 there; `gr_bytes_under_utf8_are_utf8_whatever_is_designated`
  (`baud-core/tests/encoding/utf8.rs`) now holds it after `ESC ) * +` with
  `0` or `A` and after `ESC ( 0` and `ESC ) 0 SO`, at every read size. Q5's
  other half, LS1R-LS3R under UTF-8, and its conflict with P's answer, are
  still open.
