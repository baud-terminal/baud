# AF. Character sets against DEC's own tables: the national replacement sets and the DEC sets baud designates but does not translate

**Status: research; section 5(a)'s items 1-4 and 6 built on 2026-10-09
(docs/record.md, "Swedish É and Turkish ı, and the national tables checked
against DEC's own"); researched 2026-10-08, corrected against its first check
and again against its second (round 2) on 2026-10-09.** Every question in
section 4 is open; the error character's codepoint now carries a Jev
recommendation of U+2426 at 1.00, repeated (Q-UNDEF).

- **Owner's call, whatever Jev says:** Q-UNDEF, Q-Greek, Q-ND-backtick,
  Q-DECNRCM, Q-SCS-Russian, Q-JIS-Roman, and Q-Technical's layout half. In
  each, every option departs from some DEC text, rests on a text nobody has
  read, or (Q-UNDEF) is an identification DEC does not make.
- **Jev first, at confidence 0.9 or better:** Q-Unlisted, Q-French-f,
  Q-Technical's hook half, and Q-Order. Each has an option that departs from
  no DEC text, and the note recommends it: for Q-French-f that is (b2),
  ignore `f` and leave the slot, never (b1), designate `Charset::Other`. If
  Jev picks an option that departs, it goes on to the owner. Q-Order goes on
  to the owner either way, since the owner sets the work order.
- **Buildable before any answer:** section 5(a). It covers the generator, the
  Swedish and Turkish fixes, an independent conformance test at the data
  level, the dead `Charset::to_u8`/`from_u8` pair and its stale docs, the
  corrected counts and the doc corrections. The conformance test compares
  DEC's tables with baud's tables and never renders through `Term`. Its
  finals half asserts only which set `Charset::from_final` names, and every
  gate (national mode, level) is built after `from_final`, in `Term`. So it
  answers no open question (§5(a)2).

**What decides a mark.** For DEC's character sets, the governing texts are
DEC's own: the VT220, VT420 and VT510 reference manuals, DEC STD 070,
EK-PPLV2-PM and the VT220 Programmer Pocket Guide.

- xterm (`charsets.h`, `charsets.c`, `charproc.c`) and foot are **practice**.
  Each is marked CONFORMING or DEPARTS against DEC. Neither is a reason in
  itself.
- **[[source: ctlseqs]] is practice here too.** It is xterm's documentation of
  xterm. Where it adds to DEC's texts (French `f`, line 338), the addition is
  marked as practice, not as a text. Note AE marked some options against
  ctlseqs 312 as a text. That was about the meaning of `ESC % @`, which no DEC
  text defines, and it does not carry over to the DEC sets.
- **The vt100.net pages are transcriptions.** Where one disagrees with DEC's
  printed page, the printed page wins and the disagreement is recorded (§2.5).

Evidence is tagged as in the other notes:

- **source:** a manifest key with a table, page or line, or file:line;
- **inferred;**
- **unknown.**

Quotes are tagged **[img]** when read off a rendered page, and **[text]** when
taken from a text layer only.

**What the code citations rest on.**

- Every behaviour of baud is read from source at `c793618`, the HEAD when this
  note was written.
- Nothing was built, run or benchmarked. Every behaviour of baud, xterm and foot
  here is traced from source, not observed.
- Bare file names stand for these paths: `charset.rs` is
  `baud-domain/src/vt/charset.rs` (not a `charset.rs` at the crate's `src/` root, which has none);
  `cell.rs` is `baud-domain/src/cell.rs`; `charsets.rs` is
  `baud-core/tests/vt100/charsets.rs`; `charsets.h`, `charsets.c` and
  `charproc.c` are xterm's pinned files ([[source: xterm-charsets]],
  [[source: xterm-charsets-c]], [[source: xterm-reset]]).

**Keys that are new with this note.** Fourteen keys enter `MANIFEST.toml`
with this note. They are in the working tree's manifest beside it, and none is
in the manifest at the committed `c793618`. So `scripts/check-sources.sh`
passes on this file only once they are committed with it. Thirteen are cited
below. `vt3xx-hr` is pinned, but only its cover and edition page were read,
for its manifest entry. No page of it was read for a claim, so it is not
cited (§6).

**Seven of the fourteen are `external`: pinned by hash, not committed.** They
are the DEC manuals in PDF.

- `vt220-pocket-guide`, `dec-pplv2-pm` and `vt420-rm` are `external` because
  the Pocket Guide's edition page (PDF p. 3) reads "The reproduction of this
  material, in part or whole, is strictly prohibited." **[img]** The manifest
  keeps those three DEC PDFs out of the tree for it. Only the Pocket Guide's
  edition page was read for that sentence; this note does not say the other
  two carry it.
- `vt510-rm-b`, `vt520-rm` and `vt382-rm` are `external` for size, over the
  3 MB this note commits. `vt3xx-hr` is `external` for size and for the same
  sentence on its own edition page (PDF p. 4) **[img]**. That page and the
  cover were read for its manifest entry, and no page of it for a claim
  (§6).

Each is re-fetched at its URL and checked against its sha256. So **no
generator or test can read any of them**. Where this note
says a value comes from one of them, a build can carry that value only as a
constant entered by hand with its citation, never as an extraction at build or
test time. The VT220 chapter 2 transcription, [[source: dec-special-graphics]],
is committed, and it is the one DEC table a test can read.

The reports behind this note proposed other names for four of them:
`vt220-hr`, `vt320-uu-appendix-e`, `vt510-rm-pdf` and `vt100net-dec-technical`.
This note uses the names below. The manifest entry governs each key, and each
hash below matches it.

| Key | Document | Form | sha256 |
|---|---|---|---|
| `vt220-pocket-guide` | VT220 Programmer Pocket Guide, EK-VT220-HR-002 | scan; `external` | `3f848566…f4910041` |
| `dec-pplv2-pm` | Digital ANSI-Compliant Printing Protocol Level 2 Programming Reference Manual, EK-PPLV2-PM.B01 | born-digital, vector figures; `external` | `0d47bb9b…5a4cf13f` (803,693 bytes) |
| `vt420-rm` | VT420 Programmer Reference Manual, EK-VT420-RM.002 | text layer, vector figures; `external` | `6edca845…3511ecd2` |
| `vt510-rm-b` | VT510 Video Terminal Programmer Information, EK-VT510-RM.B01 (DEC's PDF) | text layer; `external` | `440bbee1…81c7d514` |
| `vt520-rm` | VT520/VT525 Video Terminal Programmer Information, EK-VT520-RM.A01 | text layer; `external` | `7f46fe77…909017f2` |
| `vt382-rm` | VT382 Kanji Display Terminal Programmer Reference Manual, EK-VT382-RM-001 | scan; `external` | `5cab5255…d802f9ae` |
| `vt3xx-hr` | VT330/VT340 Programmer Pocket Guide, EK-VT3XX-HR-002 | scan; `external`; cover and edition page read for its manifest entry, no page for a claim, so not cited (§6) | `2d414c6e…af713415` |
| `vt320-uu-appe` | Installing and Using the VT320 Video Terminal, Appendix E (vt100.net) | HTML with SVG | `5a5e40b0…7f448e2b` |
| `vt420-uu-ch9` | Installing and Using the VT420 Video Terminal, chapter 9 (vt100.net) | HTML | `e4dbce34…8664b1af` |
| `vt100net-technical` | vt100.net, "DEC Technical Character Set" | HTML; **practice** (Paul Williams' Unicode guesses) | `6c087a09…4dc3e5aa` |
| `unicode-jis0201` | Unicode `MAPPINGS/OBSOLETE/EASTASIA/JIS/JIS0201.TXT` | text; a mapping table, not DEC | `3f26a4c8…fbd037de` |
| `iso-ir-013` | ISO-IR 013, JIS C 6220-1969 Katakana | scan; registration page read (per its manifest entry), chart pages not read | `b2cc1da4…b2c4d638` |
| `iso-ir-014` | ISO-IR 014, JIS C 6220-1969 Roman | scan; registration page read **[img]** (§3.3), chart pages not read | `eeefd699…851db325` |
| `xorg-keysymdef` | `keysymdef.h`, xorgproto 2026.1 | C header | `292e94a8…ebf0cc47` |

The same working-tree manifest widens three existing titles to what this note
cites: `dec-special-graphics` (the whole of chapter 2), `xterm-reset` (the whole
of `charproc.c`, `xtermDecodeSCS` among it) and `foot-vt-ris` (the whole of
`vt.c`, its SCS arm among it).

**Manifest follow-ups this note needs and does not make** (it edits only
itself):

- Eight more titles are narrower than what is cited here:
  - `vt420-rm` names Table 2-1 and Figures 2-7 and 2-10, but this note also
    cites Table 5-3, p. 27's national-mode text and the "ignores" sentence
    (pp. 41-42);
  - `xterm-vtprstbl-c` does not name SUB (`VTPrsTbl.c:188`);
  - `unicode-data` names decompositions and combining classes, but this note
    cites it for character names;
  - `xterm-charsets` names "the NRCS mapping macros", but this note also cites
    it for the DEC Supplemental, Technical, JIS and Cyrillic macros, the three
    DEC Supplementals (Hebrew, Greek, Turkish), `PUA` and `UNDEF` at
    `charsets.h:58` and `:60`, and `begin_CODEPAGE`/`end_CODEPAGE`
    (`:67-75`);
  - `dec-pplv2-pm` names Appendix A and Tables 7-7 and 7-8, but this note also
    cites chapter 6 (p. 6-4, SUB and the error character);
  - `unicode18-ch22` is titled "Chapter 22, Symbols (22.10 Regional Indicator
    Symbols)", but this note cites its §22.7.1, Control Pictures (Q-UNDEF,
    §6);
  - `xterm-ptyx-h` names `gsets`, `curgl`, `curgr`, `curss`, `DFT_DECID` and
    `EXP_C2_CONTROLS`, but this note also cites it for `Char` and `CharOf`
    (`ptyx.h:305`, `:331`) and the UPSS defaults (`:1138-1140`), in D5;
  - `xterm-xterm-h` names the prototypes of the charset paths, but this note
    cites it for the `xtermcfg.h` include and `OPT_WIDE_CHARS`'s fallback
    (`xterm.h:43-45`, `:298-300`), in D5.

  `xterm-charsets-c` is not on this list: its title names `xtermCharSetOut`
  (`charsets.c:326-595`), which contains both the British gate (`:404-406`)
  and `:443`.
- The comment inside the `[sources.dec-special-graphics]` entry calls the
  transcription's Dutch 7/12 `f` an error. It is not: the Pocket Guide draws
  the same `f`, and STD 070 p. 7-20 gives `f` as the florin's fallback
  presentation (§2.5). The comment should say the transcription is wrong at
  7/13 and 7/14 only, and that its 7/12 `f` is read as the VT220's
  presentation of the florin, which STD 070 and Figure A-17 name
  (**inferred**, §2.5). In the working tree's manifest the entry starts at
  line 170, and its 7/12 wording is at lines 191-193.
- `vt3xx-hr` is `external`, which the manifest's header defines as "read",
  but no page of it has been read for a claim: only its cover and edition
  page, for its manifest entry. This note does not cite it.

---

## 1. How the DEC tables were read

Two research passes compared baud with DEC's tables, and a third pass checked
both. Where the third pass corrected the first two, the correction is what
appears here. The working files are in `.claude/research/nrcs/`, which is
untracked. Every pinned source reproduces from its hash.

### The VT220 tables: SVG extraction, with two controls

[[source: dec-special-graphics]] is the vt100.net transcription of the VT220
Programmer Reference Manual's chapter 2. Tables 2-1 to 2-15 in it are inline
SVG.

- **What was extracted.** Each glyph is a `<text class="chname">` placed by x
  (the column) and y (the row). The `<text class="chnums">` that follows it
  carries the octal/decimal/hex code. Each substituted cell also carries a
  shaded `<rect fill="#ddd">`. The extractor collects both and checks one
  against the other.
- **Positive control.** Table 2-1, ASCII, gives 0 differences over 96 cells.
- **The shading control.** In every NRC table, the number of cells that differ
  from ASCII equals the number of shaded cells, and every differing cell is
  shaded. The counts, Tables 2-5 to 2-15 in order (UK, Dutch, Finnish, French,
  French Canadian, German, Italian, Norwegian/Danish, Spanish, Swedish, Swiss),
  are 1, 9, 9, 9, 10, 8, 10, 10, 8, 10 and 12.
- **A drawn glyph is detected.** On Table 2-4, the extractor reports columns 6
  and 7 as drawn paths, not text. That control is from the first pass, and the
  third pass did not re-run it (§6).
- **The manifest title was narrow.** It named only Table 2-4, but the file is
  the whole of chapter 2. It is widened beside this note (header), so a
  citation of Tables 2-3 or 2-5 to 2-15 matches its title.

### The VT220 Pocket Guide (EK-VT220-HR-002): a scan

[[source: vt220-pocket-guide]], pp. 7-17 (the NRC tables) and pp. 28-29 (the
SCS finals), was read from 200 dpi page renders **[img]**.

- Its artwork carries the same drawing numbers as the transcription:
  MA-0893-83F for the Dutch table (transcription line 3578) and MA-0893-83L
  for the Swedish one (line 7516).
- So it is the same DEC table, printed. It is the check on the transcription.

### The VT420 Programmer Reference: text layer and renders

[[source: vt420-rm]]:

- Table 2-1, "National Replacement Character Sets", p. 28, was compared with
  baud from the text layer by script. It has 11 sets by 12 positions, 132
  cells. French 7/14 is missing from the text layer, so 131 can be compared,
  and 126 of those agree.
- The 5 cells that disagree, and the one that cannot be compared, are:
  - Swedish 4/0 (§2.3);
  - Norwegian/Danish 4/0, 5/14, 6/0 and 7/14 (§2.6);
  - French 7/14, whose diaeresis is drawn, so the text layer cannot be
    compared.
- Table 5-3, p. 94 (PDF 116), was read from a render: the row labels, and the
  footnote mark on "NRC Sets", which the render shows as `*` and the text
  layer reads as `3` **[img]**.
- Figure 2-10, p. 30 (PDF 52), DEC Technical, was read at 400 dpi by the
  first pass and on 200 and 600 dpi renders for this revision **[img]**, with
  its text layer's fonts (§3.2).
- "Using Control Characters in Sequences", pp. 41-42, was read from the text
  layer **[text]**.

### EK-PPLV2-PM Appendix A: born-digital figures, read by geometry

[[source: dec-pplv2-pm]] Appendix A has a DEC chart for every NRC set in
this note but Greek, which has none (§2.7): Figures A-6 to A-19, with Hebrew
at A-21 and Turkish at A-26. It also has charts for most of the DEC sets
(Figures A-2, A-4, A-12, A-22, A-24, A-27 and A-31).

- **The text layer is lossy.**
  - Hebrew letters are not in it at all.
  - Greek tonos and Latin acute and grave accents are lost.
  - Greek arrives only through poppler's Symbol-font mapping, for example
    Delta as U+2206 and mu as U+00B5. That mapping is not DEC's choice.
  - Of JIS-Roman's 94 cells, 86 come through, and the two that distinguish the
    set (5/12 and 7/14) are among the missing.
- **Reserved positions are read from the vector strokes.** DEC marks a
  reserved cell with diagonal hatching:
  - 0.3 pt strokes on PDF pp. 182 and 185;
  - 0.25 pt on pp. 211, 215 and 221.

  A stroke detector, run independently by two passes, recovers the same masks
  (§3).
- **A white shape is detected too.** Figure A-26's 2/1 is a Helvetica-Bold `i`
  whose dot is covered by a white rectangle (§2.4). It is the only white shape
  on that page.
- **What the third pass checked independently of the transcription.**
  Figure A-15 for Swedish 4/0, and Figure A-17 for Dutch.
- **What it did not check.** Figures A-6 to A-19 as a whole (§6).
- **Printed against PDF pages.** Printed page A-n is PDF page 178 + n for
  every page cited here: p. A-2 is PDF 180, and p. A-11 is PDF 189. Figure
  numbers are not page numbers; Figure A-2, for example, is on p. A-4 (PDF
  182). Each citation below gives the figure and, where a pass recorded it,
  the PDF page.

### DEC STD 070: page images, OCR as the fallback

[[source: dec-std-070]] matches the manifest sha256 `d30c2ace…`. Its text layer
is OCR: it reads "U.s." for "U.S." and `~` where the page has `¼`.

- **Read on rendered pages [img]:**
  - §5.8.1, p. 5-115 (PDF 370), the character sets required at each
    conformance level, with notes 1-4 (§2.8, §2.10);
  - p. 5-116 (PDF 371), the end of note 4, notes 5 and 6, and the DEC STD 169
    sentence;
  - p. 5-117, the SCS designator list (PDF 372);
  - p. 6-58, the keyboard dialect and the VT320's NRC set per keyboard;
  - p. 6-109, the Dutch keyboard;
  - p. 6-129, Portuguese;
  - p. 7-18 (PDF 697), the NRC sets by keyboard dialect, with the
    availability sentence, the deviation note and the LA120 guideline;
  - p. 7-20 (PDF 699), §7.5.1, the NRC fallbacks: "There are no NRCS
    characters which are not in DEC Multinational or ISO Latin Alphabet Nr 1",
    and the deviation note under it that names two Dutch exceptions (§2.5);
  - §7.5.5, p. 7-26, for the parenthesis names at 2/11-2/14 and "not equal"
    at 3/13;
  - §7.5.5, p. 7-27 (PDF 706), for its last rows, "small tau" through "down
    arrow" (§3.2).
- **Read in the OCR only [text]:**
  - 070-2 p. 2-17, the escape-sequence pseudocode (§2.8);
  - 070-3 §3.6.2, p. 3-62, designating character sets (§2.8);
  - the rest of §7.5.5, pp. 7-25 to 7-27, the DEC Technical names and their
    order (PDF 704-706);
  - p. 8-85, the ReGIS designator list: JIS-Roman and Norwegian/Danish
    (PDF 832).
- **The chapters are not all of one revision.** The page headers read:
  - 070-2, EL-00070-02, 18-Mar-1985, "COMPANY CONF";
  - 070-3, EL-00070-03, 14-Apr-1989, "Digital Internal Use Only";
  - 070-5, EL-00070-05, 29-Jun-1990, "For Review Only";
  - 070-6, EL-00070-06, 19-Apr-1988, "Digital Internal Use Only";
  - 070-7, EL-00070-07, 28-Apr-1987, "COMPANY CONF";
  - 070-8, EL-00070-08, 31-Mar-1988 (from the OCR only).

### The VT382 Programmer Reference: a scan, read on page images

[[source: vt382-rm]] §2.4.4-2.4.6, pp. 2-16 to 2-18 (PDF 36-38), was read off
the scan by the second pass. It was re-read on the page images when its
manifest entry was written, and again by the check of this note. Every VT382
quote here is **[img]**.

- Figure 2-8, DEC Technical (PDF 36), is **partly read**: the cells §3.2
  names were read on page renders **[img]**, and its other cells have not
  been read one by one (§6).
- Figure 2-10, JIS-Katakana (PDF 38), was read on a 300 dpi render for its
  columns 14 and 15 only **[img]** (Q-UNDEF).

### The VT510 and VT520 texts

- [[source: vt510-scs]], [[source: vt510-ch7]] and [[source: vt510-rm-ch4]] are
  vt100.net transcriptions, tag-stripped and normalised.
  - Dutch is absent from vt510-scs. The positive controls "Portuguese NRCS"
    and "Swiss NRCS" hit in the same extraction.
  - vt510-rm-ch4 is `external`. The copy read matches the manifest's
    `90697715…`.
- [[source: vt510-rm-b]] and [[source: vt520-rm]] were read from their text
  layers **[text]**. Neither has a glyph chart for any set in this note.
- VT510 chapter 8 (vt100.net) was looked at in an unpinned copy. It has no
  key, so **nothing in this note rests on it** (§6).

### Unicode names

Every codepoint name comes from [[source: unicode-data]], for example
`00C9;LATIN CAPITAL LETTER E WITH ACUTE` and
`2426;SYMBOL FOR SUBSTITUTE FORM TWO`.

### What no DEC text gives

- **No DEC text read gives a Unicode codepoint for any set in this note.**
  - DEC gives a position, whether it is defined or reserved, a drawing, and
    sometimes a name.
  - Every codepoint here is therefore an identification: of a drawing (É), of
    a name ("florin"), or of both.
  - It is never a reading of DEC's codepoint, because DEC gives none. This is
    the status the DEC Special Graphics codepoints already have (TODO.md;
    docs/record.md, "DEC Special Graphics begins at 0x5F, and what it stores
    there is the owner's call").
- **The governing DEC standard is not online.** [[source: dec-std-070]] p.
  5-116: "The character sets themselves are described in DEC STD 169 Digital
  Standard Coded graphic Character Sets for Hardware and Software." **[img]**
  - Manx catalogues EL-00169-00 and EL-00169-01 Rev. A as "No copies known to
    be online", and bitsavers' standards directory has no 169.
  - Positive control: the same manx query form finds STD 070 (EL-SM070-00),
    with copies.
- EK-PPLV3-PM, the Level 3 printing manual, is also "No copies known to be
  online".

---

## 2. The national replacement sets against DEC's tables

baud's fifteen tables are fourteen `Nrcs` sets plus UK
(`baud-domain/src/nrcs_data.rs`). All are generated from
[[source: xterm-charsets]] by `scripts/gen-nrcs-tables.py`, except UK, which
the generator writes inline. Every position was compared with every DEC table
that covers it.

### 2.1 Agreement, per set

Notes on the table:

- "DEC subs." is the number of positions DEC substitutes.
- A dash means no DEC table covers the set.
- PG is the Pocket Guide; T is a VT220 table; A- is an EK-PPLV2-PM figure.

| Set | baud's finals | DEC tables read | DEC subs. | baud entries | Agree | Mismatch |
|---|---|---|---|---|---|---|
| UK | `A` | T2-5; PG p. 7; VT420 T2-1 | 1 | 1 | 1 | -- |
| Dutch | `4` | T2-6; PG p. 8; STD 070 pp. 6-109, 7-20; A-17 | 9 | 9 | 9 | transcription wrong at 2; 7/12 resolved as the florin (**inferred**, §2.5) |
| Finnish | `C` `5` | T2-7; PG; VT420 T2-1 | 9 | 9 | 9 | -- |
| French | `R` `f` | T2-8; PG; VT420 T2-1 | 9 | 9 | 9 | `f` (§2.8) |
| French Canadian | `Q` `9` | T2-9; PG; VT420 T2-1 | 10 | 10 | 10 | -- |
| German | `K` | T2-10; PG; VT420 T2-1 | 8 | 8 | 8 | -- |
| Italian | `Y` | T2-11; PG; VT420 T2-1 | 10 | 10 | 10 | -- |
| Norwegian/Danish | `` ` `` `E` `6` | T2-12 and PG (DEC set, 10); VT420 T2-1 and A-16 (ISO set, 6) | 10 or 6 | 10 | 10 against the DEC set | 4 against the ISO set (§2.6) |
| Spanish | `Z` | T2-13; PG; VT420 T2-1 | 8 | 8 | 8 | -- |
| Swedish | `H` `7` | T2-14; PG p. 16; VT420 T2-1; A-15 | 10 | 9 | 9 | **4/0 missing** (§2.3) |
| Swiss | `=` | T2-15; PG; VT420 T2-1 | 12 | 12 | 12 | -- |
| Portuguese | `% 6` | VT420 T2-1; STD 070 p. 6-129 (5/12, 7/12) | 6 | 6 | 6 | -- |
| Hebrew | `% =` | A-21 | 27 | 27 | 27 | -- |
| Turkish | `% 2` | A-26 | 12 | 11 | 11 | **2/1 missing** (§2.4) |
| Greek | `" >` | **none exists** (§2.7) | -- | 24 | unverified | 7/9, 7/10 dropped (§2.3) |

- **The totals.** baud carries 162 substitutions across the fourteen `Nrcs`
  sets, and 163 with UK. Both figures reflect the drop: neither counts the
  dropped Swedish line (§2.3).
- **Spot-checks independent of the transcription.** These all agree:
  - from VT420 Table 2-1: UK 2/3 £; Finnish 5/11 Ä and 6/0 é; French 4/0 à
    and 5/13 §; French Canadian 5/14 î and 6/0 ô; German 4/0 § and 7/14 ß;
    Italian 6/0 ù and 7/14 ì; Portuguese 5/11 Ã and 7/13 õ; Spanish 5/11 ¡
    and 7/11 °; Swiss 2/3 ù and 5/15 è; Swedish 7/14 ü;
  - from the Pocket Guide: Dutch 4/0 ¾, 5/11 ij and 5/12 ½, and Swedish 5/13
    Å;
  - from EK-PPLV2-PM: Hebrew 6/0 א through 7/10 ת, and Turkish 2/6 ğ and 4/0
    İ.
- **foot**, for every row, is practice and DEPARTS on every set.
  - `foot-vt.c:545-571` ([[source: foot-vt-ris]]) acts on the SCS finals `0`
    and `B` only.
  - Any other final leaves the slot as it was, so a slot that held DEC Special
    Graphics keeps it.

### 2.2 Every mismatch

"DEC" is DEC's reading, with each source; xterm and foot are practice.

| Set | Pos | DEC | baud | xterm | foot |
|---|---|---|---|---|---|
| Swedish | 4/0 | É U+00C9: T2-14, shaded, `chname` at line 7307; PG p. 16 **[img]**; VT420 T2-1 "`# É Ä Ö Å Ü`" **[text]**; A-15 **[img]** | `@`, **DEPARTS** | U+00C9, CONFORMING | `@`, DEPARTS |
| Turkish | 2/1 | a dotless small i, drawn (A-26, §2.4), which is U+0131 | `!`, **DEPARTS** | `!`, DEPARTS, although its comment cites this same figure (`charsets.h:2490`) | `!`, DEPARTS |
| Dutch | 7/12 | **The florin; resolved (inferred).** STD 070 p. 7-20 "florin (Dutch NRCs 7/12)", under a deviation note on Dutch characters of "older devices", with fallback presentation `f` **[img]**; A-17 draws a hooked ƒ, and its NOTE reads "7/12 Florin Sign (ƒ)", approximated as "Lowercase f (f)" **[img]**. The VT220 artwork draws a plain `f`, in the transcription and on PG p. 8 alike **[img]**: read as the florin's fallback in a VT200 (**inferred**, §2.5) | U+0192: CONFORMING to STD 070 and A-17, and to the VT220 on the inferred reading | U+0192 | DEPARTS |
| Dutch | 7/13 | ¼: PG p. 8 **[img]**; STD 070 p. 6-109 "one-qrtr (7/13)" **[img]**. The transcription has `£` | U+00BC, CONFORMING | U+00BC | DEPARTS |
| Dutch | 7/14 | acute accent: PG p. 8 draws a slanted mark shaped like the table's 2/7 apostrophe **[img]**; EK-PPLV2-PM Table A-2 names "Acute accent" (§2.5). The transcription has `¼` | U+00B4, CONFORMING | U+00B4 | DEPARTS |
| Norwegian/Danish | 4/0, 5/14, 6/0, 7/14 | DEC set (T2-12): Ä Ü ä ü. ISO set (VT420 T2-1, A-16): `@ ^ ` ~`. STD 070 names one set, NS 4551-1, for all its finals (**inferred**, §2.6) | DEC set under all three finals | DEC set under all three | DEPARTS |
| Greek | 7/9, 7/10 | no DEC table | `y`, `z` | U+2426 (`UNDEF`) | -- |

### 2.3 Swedish 4/0, and the generator that dropped it

**DEC.** Four DEC sources agree that Swedish 4/0 is É.

- [[source: dec-special-graphics]] Table 2-14, lines 7306-7308: a shaded
  `<rect fill="#ddd">` (7306), then `chname` "É" (7307), then the code triple
  100/64/40 (7308).
- [[source: vt220-pocket-guide]] p. 16 (PDF p. 21): É **[img]**.
- [[source: vt420-rm]] Table 2-1, p. 28: the Swedish row reads
  `# É Ä Ö Å Ü` **[text]**.
- [[source: dec-pplv2-pm]] Figure A-15, "DEC Swedish Character Set", PDF p.
  199: É **[img]**.

xterm has it, as `MAP(0x40, XK_Eacute)` ([[source: xterm-charsets]]
`charsets.h:393`), which is CONFORMING. baud stores U+0040, which DEPARTS.

**The generator bug (D1).**

- `scripts/gen-nrcs-tables.py:45` accepts a `MAP` line only when it carries a
  `/* U+XXXX */` comment:

  ```python
  mm = re.search(r"MAP\(0x([0-9A-Fa-f]+),\s*\w+\)\s*/\*\s*U\+([0-9A-Fa-f]+)", line)
  ```

- `charsets.h:393` has no comment, so the line is skipped silently.
- Across the fourteen macros, exactly three lines are dropped:
  - `:393`, Swedish's `MAP(0x40, XK_Eacute)`;
  - `:2380` and `:2381`, Greek's `XXX(0x79, UNDEF)` and `XXX(0x7a, UNDEF)`.
- The script's only check (`:51`) is that every wanted set yields at least one
  pair.

**Both tests that cite the tables compare baud with itself (D2).**

- `nrcs_tables_match_the_pinned_source`
  (`baud-core/tests/vt100/charsets.rs:615`) runs the generator with `--check`.
  That compares the generator with its own output, and a skipped line never
  enters either.
- `every_national_replacement_renders_as_its_table_says` (`charsets.rs:687`)
  takes its expectations from `nrcs_table`. Its own doc says so
  (`charsets.rs:683-685`): "The expectations come from `nrcs_table` rather than
  from a list here". So it is blind to the drop too.

**The published counts reflect the drop:**

- `docs/vt-coverage.md:251`, "14 sets, 162 substitutions";
- `docs/research/sources/MANIFEST.toml`, the comment above
  `[sources.xterm-charsets]`, "162 substitutions" (line 934 at `c793618`);
- `baud-core/tests/vt100/charsets.rs:533`, "14 sets, 162 substitutions";
- `baud-domain/src/cell.rs:457`, which says
  `every_national_replacement_renders_as_its_table_says` "drives all 163
  replacements through `Term`". That is already wrong. The test keeps only
  designators that resolve to `Charset::National` (`charsets.rs:693`, `:700`),
  and `ESC ( A` resolves to `Charset::Uk`, so UK is never driven. It drives
  the fourteen sets' 162 pairs: 163 once Swedish 4/0 lands, 164 with Turkish
  2/1.

**Why the fix needs more than a stricter parser.**

- `XK_Eacute` is referenced in the pinned `xterm-charsets.h` but never defined
  there. The file has 99 `XK_` references and no definitions.
- So a stricter parser can detect the drop, but it cannot emit U+00C9 from
  `xterm-charsets.h` alone. It needs one of two things:
  - [[source: xorg-keysymdef]], pinned beside this note. Its line 769 reads
    `#define XK_Eacute 0x00c9 /* U+00C9 LATIN CAPITAL LETTER E WITH ACUTE */`.
    Resolve by the `#define`'s value, not its comment: `XK_Ooblique`
    (Norwegian 0x5C, `charsets.h:353`) is defined at line 786 as `0x00d8 /*
    deprecated alias for Oslash */`, with no `U+` comment. All 99 `MAP`
    keysyms resolve to Latin-1 values. Each of the 98 lines that carry a
    `U+` comment resolves to that codepoint, and the one that does not,
    `:393`, resolves to U+00C9.
  - An override entry that takes the value from DEC's Table 2-14, which is
    pinned.

  Section 5(a) has the shape.
- The two Greek `UNDEF` lines force a choice once the parser is strict. Either
  emit what `UNDEF` means, which is Q-UNDEF, or skip them by name.

### 2.4 Turkish 2/1: a dotless i, by drawing

[[source: dec-pplv2-pm]] Figure A-26, "DEC 7-Bit Turkish Character Set" (PDF
p. 214), puts a lowercase letter at 2/1, where ASCII has `!`.

- **What the page draws.**
  - It is a Helvetica-Bold `i`, the same glyph id as the `i` at 6/9.
  - A white filled rectangle covers its dot: x 101.67-103.49, y 320.28-323.00,
    stroked 0.45 pt in white. That rectangle is the only white shape on the
    page.
  - The control: the 6/9 `i` has no attached shape.
- **So DEC draws a dotless small i at 2/1.** That letter is U+0131 LATIN
  SMALL LETTER DOTLESS I, the Turkish dotless i.
  - The identification is by drawing, not by elimination as the first pass had
    it.
  - DEC does not name it.
- **Why practice missed it.**
  - The text layer reads plain `i`, and poppler renders still show a stub of
    the dot, because Helvetica-Bold is not embedded.
  - That is the likely reason xterm, which cites this figure, leaves 2/1 as
    `!` (`charsets.h:2490-2505`) (**inferred**).
- **The second control.** The Hebrew figure in the same manual (A-21) keeps
  `!` at 2/1.
- **Marks.** baud DEPARTS, xterm DEPARTS, foot DEPARTS.
- [[source: dec-pplv2-pm]] is pinned by hash and `external` (header). Its
  manifest entry is in the working tree's manifest, to be committed with
  this note. The PDF is not in the tree and is never committed: the manifest
  keeps DEC's PDFs out for the Pocket Guide's reproduction notice, and has
  them re-fetched and checked against their sha256. Code can cite the key
  once the entry is committed, but no generator or test can read the figure,
  so U+0131 enters baud as a value typed with its citation (§5(a)1).

### 2.5 Dutch 7/12-7/14: the transcription is wrong at two, and the third is the florin drawn by its fallback (inferred)

The transcription's Table 2-6 (MA-0893-83F) has:

- 7/12 `f` (line 3496);
- 7/13 `£` (line 3499);
- 7/14 `¼` (line 3502).

The Pocket Guide p. 8, with the same MA-0893-83F artwork, has ¼ at 7/13, and at
7/14 a slanted mark shaped like the table's 2/7 apostrophe **[img]**. The
transcription's 7/14 is the 7/13 value shifted down a row. At 7/12 the Pocket
Guide agrees with the transcription: it draws a plain `f`, the same shape as
its 6/6 `f` at 600 dpi **[img]**.

- **7/13.** [[source: dec-std-070]] p. 6-109, Dutch keyboard, 7-bit
  Characters: "one-qrtr (7/13)" **[img]**. baud's U+00BC is CONFORMING. The
  transcription is wrong here.
- **7/12: the florin, drawn on the VT220 by its fallback. Resolved
  (inferred), not a transcription error.**
  - [[source: dec-std-070]] p. 7-20 (PDF 699), §7.5.1 "NRC Fallbacks", puts
    the florin under a deviation note **[img]**:

    > In older devices, two National Replacement Characters in the Dutch NRC
    > set are not available in DEC-MCS, or ISO Latin-1. If the Printer Style
    > is set to National Only or National + Line Drawing, and one of these
    > characters occur in the data stream and is not available in the current
    > national set, fallback is used as described below.

    Its table gives two rows, "ij vowel (Dutch NRCs 5/11)" with fallback
    presentation "ij", and "florin (Dutch NRCs 7/12)" with fallback
    presentation `f` **[img]**.
  - p. 7-18's deviation note says "VT200 family devices support the Dutch NRC
    set (designating character "4" (3/4))" **[img]**. Read with p. 7-20, a
    VT200 is among the "older devices" whose Dutch set has the florin at 7/12
    (**inferred**: p. 7-20 names no device).
  - [[source: dec-pplv2-pm]] Figure A-17 (p. A-23, PDF 201) draws a hooked ƒ.
    Its NOTE gives the approximations used for "the Dutch characters that are
    not available in the DEC MCS or ISO Latin-1 character sets", and lists
    "7/12 Florin Sign (ƒ)" with the approximation "Lowercase f (f)" **[img]**.
  - The VT220 artwork, printed (Pocket Guide p. 8) and transcribed (Table
    2-6), draws a plain `f` **[img]**.
  - **The inference.** p. 7-20 names the Dutch 7/12 of older devices the
    florin and gives `f` as its fallback presentation, p. 7-18 puts the VT200
    family among the devices with a Dutch set, and Figure A-17 gives the same
    approximation, "Lowercase f (f)". So the VT220's drawn `f` is read as the
    florin presented by that fallback (**inferred**). The deviation note is
    about printer-port fallback, not about the screen. Carrying it to the
    glyph the VT220 manuals draw is the inference, and no DEC text says it in
    words.
  - baud's U+0192 LATIN SMALL LETTER F WITH HOOK is CONFORMING to STD 070 p.
    7-20 and to Figure A-17 with its NOTE, and to the VT220 on the inferred
    reading. Its Unicode 1.0 name was LATIN SMALL LETTER SCRIPT F. U+0066
    would depart from STD 070 p. 7-20 and Figure A-17.
- **7/14.** [[source: dec-pplv2-pm]] Table A-2, p. A-11 (PDF p. 189),
  "National Replacement Character Set Pairing Fallbacks", has this row:
  "DEC Dutch | Acute accent ( ´ ) | 02/07 ( ’ ) | 11/04 ( ´ )" **[text]**.
  - 11/04 is the ISO Latin-1 position, U+00B4 ACUTE ACCENT.
  - The DEC Supplemental fallback, 02/07, is the likely reason the VT220
    artwork draws 7/14 like an apostrophe (**inferred**).
  - Table A-2 gives fallback positions, not the NRC position. Its placement at
    7/14 comes from Figure A-17, which draws ´ there. The figure has two
    accent cells, the diaeresis ¨ at 7/11 and this one, and Table A-2's
    "Acute accent" names only this one.
  - STD 070 p. 7-20's general sentence is "There are no NRCS characters which
    are not in DEC Multinational or ISO Latin Alphabet Nr 1" **[img]**. Its
    deviation note qualifies it for older devices, and names the only two
    Dutch characters it excepts: ij at 5/11 and the florin at 7/12 (7/12
    above) **[img]**. The acute accent at 7/14 is not among them, which
    agrees with Table A-2's Latin-1 11/04.
  - baud's U+00B4 is CONFORMING. The transcription is wrong here.

Figure A-17 also confirms ¾, ij, ½, |, ¨, ƒ, ¼ and ´ independently.

Any test built on the transcription must carry these three positions as
exceptions: 7/13 and 7/14 as transcription errors, each citing the DEC primary
that overrides it, and 7/12 as the florin, resolved (**inferred**) from STD
070 pp. 7-18 and 7-20 and Figure A-17 against the VT220's drawn `f` (§5(a)).

### 2.6 Norwegian/Danish: two DEC sets, one in baud

**Two texts define two sets: the DEC one under `E` or `6`, and an ISO one
under `` ` ``.**

- [[source: vt420-rm]] Table 5-3, p. 94: "ISO Norwegian/Danish† ‘ 6/0" and
  "DEC Norwegian/Danish 6 or E 3/6 4/5" (row labels, read from a render). Its
  Table 2-1's Norwegian/Danish row is the ISO repertoire:
  - `# @ Æ Ø Å ^` at 2/3, 4/0, 5/11-5/14;
  - `` _ ` æ ø å ~ `` at 5/15, 6/0, 7/11-7/14.
  - [[source: vt420-uu-ch9]], the transcription of the VT420 user guide's
    programming summary, has the same two rows: "DEC Norwegian/Danish 6 3/6"
    and "ISO Norwegian/Danish ` 6/0" **[text]**.
- [[source: dec-pplv2-pm]] Table 7-7, p. 7-74: "DEC Norwegian/Danish 6 3/6"
  and "Norwegian/Danish ` 6/0" **[text]**.
  - Figure A-13 is the DEC set: Ä Ü ä ü at 4/0, 5/14, 6/0 and 7/14.
  - Figure A-16 keeps `@ ^ ` ~` there.
  - Table 7-8 adds: "Fall back to DEC Norwegian/Danish E 4/5" **[text]**.

**The VT220 has the DEC set only.** [[source: dec-special-graphics]] Table
2-12 and [[source: vt220-rm-ch4]] Table 4-4 give the VT220's set, which is the
DEC repertoire, under "E or 6". The VT220 has no backtick final.

**These texts name a single set.**

- [[source: dec-std-070]] names one Norwegian/Danish set, and gives no
  repertoire for it:
  - p. 5-117 has a single row, "Norwegian/Danish E or 6", and no backtick
    **[img]**.
  - p. 7-18 gives one set, "Norwegian/Danish NRC Set" at "(6/0)", for both the
    Danish and Norwegian dialects **[img]**. Its guideline lists "Norway/Denmark
    "6" (3/6) or "E" (4/5)" among "LA120 non-conforming sequences", in one
    sentence with Finland "C", Sweden "H" and French Canadian "Q" **[img]**.
    Those three are alternate finals for the same sets as `5`, `7` and `9`,
    so the grouping reads `6` and `E` as alternates for the one set at 6/0
    too (**inferred**).
  - p. 6-58, in its "Guideline: On the VT320" list of the NRC set for each
    keyboard, identifies that set: "Danish - Norwegian/Danish NRCS 3/NS
    4551-1, DS 2089" **[img]**.
  - p. 8-85's ReGIS designator list has "Norwegian/Danish NRCS/NS 4551-1, DS
    2089" under a tail the OCR reads as `0'`, which in that list of `0`
    followed by an SCS final is presumably `` 0` `` (**inferred**). It has no
    DEC Norwegian/Danish row, and no `06` or `0E` tail, where it does have
    "DEC Finnish NRCS" (`05`) and "DEC Swedish NRCS" (`07`) **[text]**.
  - No STD 070 page read gives the Ä Ü ä ü repertoire.
  - **Inferred:** NS 4551-1 is the ISO repertoire of VT420 Table 2-1 and
    EK-PPLV2-PM Figure A-16. NS 4551-1 itself is unread (§6), so its contents
    are not given here.
- [[source: vt510-scs]]: "`, E, or 6 | Norwegian/Danish NRCS", with no chart.
- [[source: vt520-rm]] Table 5-14 has the same row, with "†We recommend using
  the first code shown" **[text]**.
- [[source: vt510-ch7]] §7.2.1 names one set, "Norwegian/Danish NRC (ISO)".
  - That label cannot settle the repertoire on its own. The same list labels
    Swedish "Swedish NRC (ISO)", where VT420 Table 5-3 and EK-PPLV2-PM Table
    7-7 both say "DEC Swedish".

**baud and practice.**

- baud maps all three finals to the DEC repertoire
  (`baud-domain/src/vt/charset.rs:278`).
- For `E` and `6` that is CONFORMING to the VT220 (Table 2-12) and to
  EK-PPLV2-PM (Figure A-13), which chart the DEC repertoire. It is CONFORMING
  to the VT420 only on a name (**inferred**): its Table 5-3 says "DEC
  Norwegian/Danish 6 or E", and its Table 2-1 charts only the ISO set, so the
  VT420 never charts Ä Ü ä ü. It DEPARTS from STD 070, whose one set for
  those finals is NS 4551-1 (**inferred**, as above).
- For `` ` `` it DEPARTS from VT420 Table 5-3 and Table 2-1 and from
  EK-PPLV2-PM Table 7-7 and Figure A-16, at four positions. It DEPARTS from
  STD 070 pp. 6-58 and 7-18 too, on the same inference.
- Practice: xterm keeps three enum values, `nrc_Norwegian_Danish`, `…2` and
  `…3` ([[source: xterm-reset]] `charproc.c:1841-1842, 1853`), and maps all
  three to one table. That DEPARTS in the same way.

Q-ND-backtick.

### 2.7 Greek: no DEC table

**DEC names the set.**

- [[source: vt510-scs]] gives `" >` as "Greek NRCS".
- [[source: vt510-ch7]] §7.2.1 lists "DEC 7-bit Greek".
- [[source: vt520-rm]] Table 5-14 gives "Greek NRCS ">" **[text]**.

**No DEC glyph table for it has been found.** There is a positive control.

- [[source: dec-pplv2-pm]] §A.9 lists two Greek sets only, "DEC Greek
  Supplemental Character Set, Figure A-24" and "ISO Latin-Greek Supplemental
  Character Set, Figure A-25" **[text]**.
- In that manual "7-Bit Hebrew" hits 11 times and "7-Bit Greek" none.
- [[source: vt510-rm-b]] and [[source: vt520-rm]] have only Greek keyboard
  figures, and [[source: vt510-ch7]] has no chart.
- DEC STD 169, which would hold it, is not online (§1).

**baud's 24 letters are xterm's.**

- `charsets.h:2350-2353`: "mentioned, but not documented in VT510 manual,
  etc., this uses the ELOT927 table from Kermit 95".
- The macro maps 6/1-7/8 to the 24 capitals and marks 7/9 and 7/10
  `XXX(…, UNDEF)` "unused". baud's generator drops both of those lines (§2.3),
  so baud draws `y` and `z` there.

**The Chi/Xi swap.**

- xterm encodes 6/14 as U+03A7 CHI, commented "CAPITAL GREEK LETTER KSI
  (CHI)", and 7/6 as U+039E XI, commented "CAPITAL GREEK LETTER XI"
  (`charsets.h:2369, 2377`).
- In Greek alphabetical order Ξ follows Ν (6/13) and Χ follows Φ (7/5). So Ξ
  at 6/14 and Χ at 7/6 would be alphabetical.
- **Inferred:** "KSI" romanizes Ξ, and "XI" was the spelling of Χ in the source
  xterm copied. The comments carry the alphabetical reading, and the codepoints
  swap it. baud copies the swap.
- ELOT 927 itself is unread, and no register entry for it is pinned (§6).

**Practice at a non-default ID.**

- xterm designates Greek NRC only at `min_level` 5 (`charproc.c:1859`).
- Its default terminal ID is `"420"` ([[source: xterm-ptyx-h]] `ptyx.h:399`),
  and
  `set_vtXX_level(screen, terminal_id / 100)` (`charproc.c:11051`) makes the
  level 4.
- So by default xterm does not designate the set at all. An unmatched
  designation falls back to ASCII (`charproc.c:1938-1940`).

Q-Greek and Q-UNDEF.

### 2.8 Finals

**French `f`.** No DEC text gives `f`:

- [[source: vt220-rm-ch4]] Table 4-4: "French 5/2 R" **[text]**;
- the Pocket Guide p. 28 **[img]**;
- [[source: vt510-scs]];
- [[source: dec-std-070]] p. 5-117 and p. 7-18 **[img]**;
- [[source: dec-pplv2-pm]] Table 7-7;
- [[source: vt420-rm]] Table 5-3, and [[source: vt420-uu-ch9]]'s table, "ISO
  French R 5/2" **[text]**.

Only [[source: ctlseqs]] line 338 has it: "C = R or f -> French, VT200." xterm
has `nrc_French2` at `f` (`charproc.c:1837`). baud accepts it
(`charset.rs:273`), and that DEPARTS from every DEC text read. Q-French-f.

**Dutch `4`.**

- CONFORMING to the VT220: Table 4-4 and the Pocket Guide.
- Absent from vt510-scs and from VT420 Table 5-3. [[source: vt420-uu-ch9]]'s
  table, the VT420 user guide's, does list "DEC Dutch 4 3/4" **[text]**, and
  so does [[source: dec-pplv2-pm]] Table 7-7, p. 7-74: "DEC Dutch 4 3/4"
  **[text]**.
- STD 070 p. 5-117 marks it "(discontinued in L3)" **[img]**.
- p. 7-18 maps the Dutch dialect to ASCII `B`, with the deviation note "VT200
  family devices support the Dutch NRC set (designating character "4" (3/4))"
  **[img]**.

**Levels: STD 070 §5.8.1, p. 5-115 (PDF 370).** It is the only DEC text read
that gives a level for each set. Its table, "Level at which required, or
availabe [sic] as Extension", reads **[img]**:

| Character set (DEC's spelling) | Level |
|---|---|
| U.S. ASCII | 1-4 |
| DEC Line Drawing | 1-4 |
| DEC Supplemental Graphic | 2-4 |
| ISO Latin-1 Supplemental | 2x, 3-4 |
| UPSS | 2x, 3-4 |
| UK set (United Kingdom) | 1x |
| NRCS | 1x, 2x, 3x, 4x |
| JIS_ROMAN, JIS_KATAKANA | 2x, 3x, 4x |
| DRCS | 2x, 3x, 4x |
| DEC Technical | 3x, 4x |

An `x` marks an extension. Notes 3 and 4 are quoted in §2.10 (Q-DECNRCM
(b.3), (b.4)); note 5 (p. 5-116) reads "JIS_ROMAN and JIS_KATAKANA are part
of the Katakana Extension." **[img]**

- DEC Supplemental is required from Level 2, and not listed for Level 1.
- DEC Technical is an extension at Levels 3 and 4 only. JIS-Roman and
  JIS-Katakana are an extension from Level 2.
- UK is listed at Level 1 only, as an extension. Note 3 says it "has since
  been replaced by the NRCS Extension" **[img]**, and p. 5-117 lists British
  among the NRCS finals.
- Dutch `4` is "discontinued in L3" on p. 5-117.

baud answers DA as a VT102 (`baud-domain/src/vt/report.rs:201-204`, D6), and
designates every set above, and translates the NRC sets, whatever level that
answer claims. Reading a VT102 as a Level 1 device is the level reading note
AE's Q9 leaves open (**inferred** here, not decided).

**Whether baud gates SCS finals by the level it claims is not yet a question
anywhere.** Note AE's Q9 is about level gating of the shifts (LS2/LS3,
LS1R-LS3R, SS2/SS3), not of designations. Gating an SCS final by level rests
on the same level reading, so it belongs with Q9 only if Q9 is extended to
designations, or needs a question of its own. It reaches Dutch `4`, the JIS
sets, DEC Technical, DEC Supplemental, and the VT500 sets STD 070 does not
list. This note does not choose.

**`C`, `H`, `Q`, `E` and `6`.**

- STD 070 p. 7-18 groups them **[img]**: "Guideline: LA120 non-conforming
  sequences: Finland "C" (4/3); Sweden "H" (4/8); French Canadian "Q" (5/1);
  Norway/Denmark "6" (3/6) or "E" (4/5)."
- EK-PPLV2-PM Table 7-8 calls `C`, `Q`, `E` and `H` "Fall back to DEC …"
  sequences, and recommends "that conforming software not use these sequences"
  **[text]**.
- Both are guidance on what software should send.
- Accepting all of them is CONFORMING: they are in VT220 Table 4-4 or VT510
  SCS.

**French Canadian `9`.**

- VT220 Table 4-4 gives `Q` only.
- `9` first appears in STD 070 p. 7-18, VT420 Table 5-3 and EK-PPLV2-PM Table
  7-7, and vt510-scs has "9 or Q".
- baud is CONFORMING.

**What an unlisted final does.** DEC's texts leave the slot as it was.

- [[source: dec-std-070]] 070-3 §3.6.2, p. 3-62, on designation itself: "Once
  a character set is designated to a G-set, it remains in that set until
  explicitly replaced by another character set designator." **[text]**
- 070-2 p. 2-17, the escape-sequence algorithm, designates only when the final
  matches a table entry **[text]**:

  ```
  40,41,42,43:
     FOR N:= 1 TO 4 DO
       IF EVENT.CODE.VALUE =
       CHARACTER.SET.TABLE[N].FINAL THEN
         DESIGNATE. CHARACTER. SET;
  ```

  A final that matches no entry reaches no `DESIGNATE`. The `OTHERWISE
  IGNORE` that follows the arm belongs to the `CASE` over intermediates, not
  to an unmatched final.
- The general rule, [[source: vt420-rm]] "Using Control Characters in
  Sequences" (pp. 41-42) and [[source: vt510-rm-b]] §4.3.5 of the same title:
  "The terminal ignores unrecognized sequences and strings, unless they end a
  current escape sequence" **[text]**.

Marks:

- **baud DEPARTS, for every unlisted final.** `Charset::from_final` and
  `from_multi` answer `Charset::Other`
  (`baud-domain/src/vt/charset.rs:258`, `:283`), and
  `esc_final` writes it into the slot and counts it
  (`baud-core/src/write.rs:1714-1718`). `Other` draws ASCII
  (`charset.rs:55-65`), so the set the slot held is replaced.
- xterm DEPARTS: an unmatched designation falls back to ASCII
  (`charproc.c:1938-1940`).
- foot CONFORMS on this point, since it leaves the slot (§3.7). It still
  DEPARTS by not drawing the sets (§2.1, §3.7).

Q-Unlisted, and with it Q-French-f.

### 2.9 SCS NRCS (`% 3`) and Russian NRCS (`& 5`)

**DEC names both sets and gives no glyph table for either.**

- [[source: vt510-scs]]: "% 3 | SCS NRCS" and "& 5 | Russian NRCS".
- [[source: vt510-ch7]] §7.2.1: "SCS NRC" and "Russian 7-bit (KOI-7)".
- [[source: vt520-rm]]:
  - p. 4-35, the Primary DA extensions: "12 Serbo-Croatian (SCS)" **[text]**;
  - p. 7-2: "Russian 7-bit (KOI-7)" **[text]**;
  - Table 5-14 (Cont.), p. 5-199: "Russian NRCS &5 3/6, 3/5" **[text]**. `&`
    is 2/6, so "3/6" is a DEC misprint.

**baud.**

- baud designates both as `DecSet::Undocumented` and translates nothing
  (`baud-domain/src/vt/charset.rs:257`), so both draw ASCII.
- Its doc calls `& 5` "DEC Russian" (`charset.rs:184`). That is
  [[source: ctlseqs]] line 368's name, "C = & 5 -> DEC Russian, VT500."
  The pinned DEC texts call it "Russian NRCS" (vt510-scs; VT520 Table 5-14)
  and "Russian 7-bit (KOI-7)" (vt510-ch7 §7.2.1; VT520 p. 7-2).
- The doc's reason quotes [[source: ctlseqs]] 365-368: "no documentation has
  been found for the mappings".

**Practice.** xterm, at the same commit, translates both by its own choices.

- `map_NRCS_Serbo_Croatian` (`charsets.h:2427-2444`): "VT520/VT525 manual p
  4-35 explains "SCS" as Serbo-Croatian. … choose ISO-IR-141".
- `map_NRCS_Russian` (`charsets.h:2448-2485`): "VT520/VT525 manual p 7-2
  explains "Russian" as KOI-7, though the dialect is unknown. Choose the one
  Kermit used."
- Both designate only at level 5 (`charproc.c:1861-1862`), so xterm's default
  ID does not reach them.

Q-SCS-Russian.

### 2.10 National mode: DEC makes the NRC sets conditional, and baud does not

**The texts.**

- [[source: dec-special-graphics]] §2.4.3: "These sets are available when you
  select national mode. Only one national character set is available for use
  at any one time. The NRC set used depends on the keyboard selection in
  set-up as outlined below." **[text]**
- [[source: vt220-rm-ch4]] Table 4-4: "NOTE: Only one national character set is
  available at any one time (national mode)." **[text]**
- [[source: vt220-rm-ch4]] §4.6.19: "Character set mode determines whether the
  terminal uses national replacement character sets or the DEC multinational
  character set." `CSI ? 4 2 h`: "Selects national mode. The terminal now
  generates 7-bit characters from the NRC sets." **[text]**
- [[source: vt420-rm]] p. 27 (PDF 49): "To use an NRC set, you must select
  national replacement character set mode. … You can only use one NRC set at
  a time. The NRC set used depends on the keyboard selected in set-up"
  **[text]**. Table 5-3's footnote on "NRC Sets", marked `*` on the page (the
  text layer reads `3`): "Only one national character set is available at a
  time. You must select national mode to use national character sets."
  **[text]**, mark **[img]**
- [[source: vt510-ch7]] §7.3: "In VT100 mode, VT52 mode, or when 7-bit NRCS
  characters is selected (through Set-Up or DECNRCM), only ASCII, NRC sets, and
  DEC Special Graphic characters are available." **[text]**
- [[source: dec-std-070]] §5.8.1, note 4, which begins on p. 5-115 and ends
  on p. 5-116 (PDF 370-371) **[img]**: "NRCS (National Replacement Character
  Set) is an extension which provides twelve 7-bit NRC sets, one of which may
  be selected under local user control (Set-Up) to replace U.S. ASCII as the
  default set. See DECNRCM. Depending on the implementation, only one NRC set
  corresponding to the keyboard dialect may be selectable at a time."
  - Note 3, p. 5-115 **[img]**: "The UK set is an optional extension to Level
    1 which if present, may be selected in Set-Up to replace ASCII (ASCII and
    UK cannot be designated simultaneously). This extension was implemented on
    the VT100, but has since been replaced by the NRCS Extension."
  - p. 7-18: "The National Replacement Character Sets are available only when
    the appropriate keyboard is selected in the terminal." **[img]**
  - p. 6-58: the keyboard dialect "selects the corresponding National
    Replacement Character Set (new for Level 3)" **[img]**.

**baud.**

- baud translates whatever set is designated, in every mode.
- `?42` is not recognised: `PrivateMode::from_wire` sends it to
  `PrivateMode::Other` (`baud-domain/src/vt/mode.rs:369-400`). Nothing under
  `baud-core/src` or `baud-domain/src` names DECNRCM.
- baud has no keyboard dialect.

**Practice: xterm gates.**

- `xtermDecodeSCS` ([[source: xterm-reset]] `charproc.c:1914-1941`) accepts a
  `scs_table` row only if `(scs_table[n].need_nrc == 0 || (xw->flags &
  NATIONAL) != 0)` (`:1929`).
- An unmatched designation falls back to `initCharset(screen, which,
  nrc_ASCII)` (`:1940`).
- Every NRC row has `need_nrc = 1` **except British**: `nrc_British` is
  `need_nrc = 0` (`:1827`). British is gated instead in `xtermCharSetOut`
  ([[source: xterm-charsets-c]] `charsets.c:404-406`, within the function's
  `:326-595`), as the first pass found. That gate passes when NATIONAL is
  set **or** when `vtXX_level <= 1`, so at level 1 xterm translates British
  without national mode.
  - That corrects the third pass's "every NRC set has `need_nrc=1`".
- xterm checks the level and NATIONAL, not a keyboard dialect.
- Setting DECNRCM under UTF-8 swaps xterm's UTF-8 mode off while national mode
  is on (`modified_DECNRCM`, `charproc.c:1287-1299`; `srm_DECNRCM`,
  `:7680-7686`).
- foot has no NRC sets.

**The docs contradict each other.** `docs/vt-coverage.md:760` lists "National
replacement charsets | DECNRCM | Superseded by UTF-8" as out of scope.
`:251` and `:557` say the sets are translated.

Q-DECNRCM.

---

## 3. The DEC sets baud designates but does not translate

`Charset::Dec(_)` falls to `with_ascii` in `with_charset`
(`baud-domain/src/cell.rs:389-398`, the arm at `:396`), so every set below
draws its ASCII byte today.

| Set | baud's final | DEC final | DEC source | Form | Pinned | Positions (defined / reserved) | Build-time table from a DEC text? |
|---|---|---|---|---|---|---|---|
| DEC Supplemental | `<`, `% 5` | `<` (VT220), `% 5` (STD 070, VT510, PPLV2) | VT220 Table 2-3; PPLV2 Figure A-2 | SVG with text glyphs; vector | Table 2-3 committed; PPLV2 with this note, `external` | 81 / 13 | **Yes**: glyphs and mask from Table 2-3 at build time; A-2's mask confirms it as a cited constant |
| DEC Technical | `>` | `>` | PPLV2 Figure A-4 and Table A-1; VT420 Figure 2-10; VT382 Figure 2-8; STD 070 §7.5.5 | vector; scan; OCR names | with this note, `external` | 85 / 9 | Mask and names as cited constants; codepoints are judgement; VT420 differs from three other DEC texts at five cells |
| JIS-Roman | `J` | `J` | VT382 §2.4.5; PPLV2 Figure A-12, Table A-2 | scan; vector | with this note, `external` | 94 / 0 | Yes, from DEC's prose rule (ASCII with two changes); 7/14's codepoint is judgement |
| JIS-Katakana | `I` | `I` | PPLV2 §A.12, Figure A-31; VT382 §2.4.6, Figure 2-10 | vector; scan | with this note, `external` | 63 / 31 | Mask as a cited constant; glyphs from JIS X 0201, which DEC names |
| DEC Hebrew Supplemental | `" 4` | `" 4` | PPLV2 Figure A-22 | vector | with this note, `external` | 49 / 45 | Mask as a cited constant; glyphs by eye |
| DEC Greek Supplemental | `" ?` | `" ?` | PPLV2 Figure A-24 | vector | with this note, `external` | 81 / 13 | Mask as a cited constant; glyphs by eye |
| DEC Turkish Supplemental | `% 0` | `% 0` | PPLV2 Figure A-27 | vector | with this note, `external` | 87 / 7 | Mask as a cited constant; glyphs by eye |
| DEC Cyrillic | `& 4` | `& 4` | **none found** | -- | -- | -- | No |

**What "build-time" can mean here.** Of the DEC sources in this table only
VT220 Table 2-3 ([[source: dec-special-graphics]]) is committed, so only it
can be read by a generator at build time. EK-PPLV2-PM, the VT420 RM, the
VT382 RM and DEC STD 070 are `external`: the first three as the header
says, and STD 070 for its 74 MB and its "COMPANY CONF - DEC Internal Use Only"
stamp, per its manifest entry. A mask or name read from one of them was read
by script from a hash-checked research copy, and a build can carry it only as
a constant entered with its citation, never as an extraction at build or test
time.

**Where each DEC source is.**

- VT220 Table 2-3, [[source: dec-special-graphics]]:
  https://vt100.net/docs/vt220-rm/chapter2.html
- EK-PPLV2-PM, [[source: dec-pplv2-pm]]:
  https://vt100.net/mirror/mds-199909/cd3/printer/pplv2pmb.pdf
- VT420 RM, [[source: vt420-rm]]:
  https://vt100.net/mirror/mds-199909/cd3/term/vt420rm2.pdf
- VT382 RM, [[source: vt382-rm]]: https://vt100.net/dec/ek-vt382-rm-001.pdf
- DEC STD 070, [[source: dec-std-070]]:
  https://www.bitsavers.org/pdf/dec/standards/EL-SM070-00_DEC_STD_070_Video_Systems_Reference_Manual_Dec91.pdf
- Searched for DEC Cyrillic, without a chart: [[source: vt510-rm-b]],
  https://vt100.net/mirror/mds-199909/cd3/term/vt510rmb.pdf, and
  [[source: vt520-rm]],
  https://bitsavers.org/pdf/dec/terminal/vt5xx/EK-VT520-RM_VT520_VT525_Programmer_Information_Jul94.pdf
- Not DEC: [[source: unicode-jis0201]],
  https://www.unicode.org/Public/MAPPINGS/OBSOLETE/EASTASIA/JIS/JIS0201.TXT,
  and the practice page [[source: vt100net-technical]],
  https://vt100.net/charsets/technical.html

**Every final agrees with DEC.**

- The finals are in [[source: vt510-scs]], EK-PPLV2-PM Table 7-7, VT520 Table
  5-14, and STD 070 p. 5-117 (`DEC_SUPPLEMENTAL_G %5`, `UPSS_G <`,
  `TCS_G >`, `J`, `I`) **[img]**. The page prints the underscores.
- baud is CONFORMING on each
  (`baud-domain/src/vt/charset.rs:252-256, 279-282`).
- Whether `<` designates DEC Supplemental or the UPSS is note AE's Q14.

**Reserved positions draw DEC's error character.** For each set whose section
says it, the wording is the same: "The error character is printed when one of
these reserved characters is received by the device." This is EK-PPLV2-PM
§A.2, §A.4, §A.8, §A.9 and §A.12 **[text]**. What that character is, as a
codepoint, is Q-UNDEF.

- **§A.10, Turkish, has no such sentence.** It lists the three Turkish
  sets and says they are 94- and 96-character sets, and says nothing of
  reserved positions **[text]**. Figure A-27 hatches seven cells like the
  other figures, so DEC Turkish Supplemental's reserved cells are marked, but
  no text says what they print.

### 3.1 DEC Supplemental

**DEC.**

- [[source: dec-special-graphics]] §2.4.1: "The GR codes are the DEC
  supplemental graphics set." **[text]** Table 2-3 (right half) carries it.
- [[source: dec-pplv2-pm]] §A.2: "There are 81 characters defined in this set.
  The remaining character positions are considered ''reserved for future use.''
  These positions are indicated by shaded boxes in the figure. The error
  character is printed when one of these reserved characters is received by the
  device." **[text]** Figure A-2 is on PDF p. 182.

**Recovered by script.**

- All 94 cells of Table 2-3 parse, pairing each `chname` with the `chnums`
  triple after it, as note AE's Q2 did.
- 81 carry a glyph. 13 are empty self-closing `chname` cells: `A4 A6 AC AD AE
  AF B4 B8 BE D0 DE F0 FE`.
- The SVG has no fills, so on its own "empty" means blank. The reserved meaning
  is EK-PPLV2-PM's.
- Figure A-2's hatched boxes, read by stroke detection, are exactly those 13
  cells. That is an independent DEC check.
- 0xDF is ß in Table 2-3, in VT320 Appendix E and in Figure A-2.
- [[source: vt320-uu-appe]]'s SVG agrees at 94/94, but it has the same
  transcriber, so it is not independent. The third pass did not re-run that
  comparison either (§6).

**Can it be generated at build time?** Yes, from the pinned Table 2-3, which
is committed: 81 glyphs, plus the 13-cell mask from its empty cells. Figure
A-2 cannot be read at generation, since EK-PPLV2-PM is `external`; its mask,
read by script from the research copy, is checked at generation as a constant
carrying its citation. The codepoints are the transcriber's. Note AE's Q2
checked names against [[source: unicode-data]] for the positions it listed.

**Practice: xterm's `map_DEC_Supp_Graphic`** ([[source: xterm-charsets]]
`charsets.h:438-462`).

- Its 13 `XXX(…, UNDEF)` positions equal DEC's mask: CONFORMING on the mask.
- It **DEPARTS at 5/15**. `UNI(0x5F, 0x005F)` (`:455`), reached through
  `map_DEC_Supp_Graphic(chr = seven, chr = eight)` ([[source: xterm-charsets-c]]
  `charsets.c:443`), draws GR 0xDF as `_` where DEC has ß.
- Read from source, not run.

### 3.2 DEC Technical

**DEC.**

- [[source: dec-pplv2-pm]] §A.4: "The DEC Technical character set, shown in
  Figure A-4, contains 85 characters including Greek letters, mathematical
  symbols, and logical symbols. The remaining character positions are
  considered ''reserved for future use.''" **[text]**
  - Figure A-4 is on PDF p. 185.
  - Table A-1 (PDF p. 186) says which large symbol each piece from 2/1 to 3/7
    belongs to.
- [[source: vt420-rm]] Figure 2-10, p. 30.
- [[source: vt382-rm]] Figure 2-8, p. 2-16 (PDF 36), a scan, under the note
  "You can only use the DEC Technical set in VT300 mode." **[img]**
- [[source: dec-std-070]] §7.5.5, pp. 7-25 to 7-27: 85 names in code order, for
  example "<Top(> top left parenthesis" **[text]**. The parenthesis names at
  2/11-2/14 and "not equal" at 3/13 were checked on the image of p. 7-26, and
  the last rows, from "small tau" and "function" to "left arrow", "up arrow",
  "right arrow" and "down arrow", on the image of p. 7-27 **[img]**. The rest
  of the list is OCR.

**Recovered by script.**

- The 9 hatched reserved cells: `38 39 3A 3B 52 54 55 6D 75`.
- The count, 85, agrees three ways: §A.4, STD 070's 85 entries, and
  [[source: vt100net-technical]].
- In EK-PPLV2-PM's text layer only the 31 Greek letters come through.

**VT420 Figure 2-10 departs from three other DEC texts, at five cells.**

- [[source: vt420-rm]] p. 30: "This 7-bit character set has 94 graphic
  characters." **[text]** VT382 §2.4.4 says the same **[img]**.
- The four DEC texts, cell by cell where they differ (VT420 Figure 2-10 at
  400 dpi by the first pass, and on a 200 dpi render for this revision at
  3/8, 3/13, 7/6 and 7/11-7/14; EK-PPLV2-PM Figure A-4 on a 200 dpi render;
  VT382 Figure 2-8 on page renders of PDF p. 36; all **[img]**; STD 070 by
  list order on the images of pp. 7-26 and 7-27 **[img]**):

  | Cell | VT420 Figure 2-10 | EK-PPLV2-PM Figure A-4 | VT382 Figure 2-8 | STD 070 §7.5.5 order |
  |---|---|---|---|---|
  | 3/13 | `=` | ≠ | ≠ | "not equal" |
  | 7/5 | an italic `f`, read as ƒ (see below) | hatched (reserved) | blank | one name, "function", after "small tau" for 7/5-7/6 |
  | 7/6 | reversed `?` | ƒ | ƒ | (the same) |
  | 7/12 | → | ↑ | ↑ | "up arrow" |
  | 7/13 | ↑ | → | → | "right arrow" |

- At 7/11 and 7/14 all four agree: ←, and ↓ ("left arrow", "down arrow").
- **VT420 7/5.** The page's text layer has an `f` there, set in
  Souvenir-DemiItalic, a font the PDF does not embed **[text]**. The first
  pass read it at 400 dpi as ƒ; this revision's renders substitute a font and
  show a plain `f` at 600 dpi, so the hook is not visible in them **[img]**.
  Reading DEC's italic `f` as the function sign ƒ is an identification
  (**inferred**).
- **VT420's reversed `?`.** The text layer's `?` at 3/8 and at 7/6 comes
  out of `pdftohtml` with a negative width, which is how a mirrored glyph
  shows there **[text]**. 3/8 is a cell the other three texts reserve, so
  the mark there reads as DEC's error character at a reserved cell, and the
  one at 7/6 as the VT420 reserving 7/6 where the others reserve 7/5 (both
  **inferred**: the VT420 says "94 graphic characters" and calls no cell
  reserved) (Q-UNDEF).
- VT382 Figure 2-8 leaves blank exactly EK-PPLV2-PM's nine hatched cells,
  3/8-3/11, 5/2, 5/4, 5/5, 6/13 and 7/5 **[img]**. Those and the cells in the
  table above are the cells of Figure 2-8 that were read; the rest of it has
  not been read one by one (§6).
- So the VT420 stands alone at all five cells. STD 070's order cannot settle
  7/5 against 7/6 by itself, but EK-PPLV2-PM and VT382 agree on both.

**Can it be generated at build time?** Partly, as cited constants (§3's
"What build-time can mean here").

- The mask and the piece membership were read by script from the research
  copy of EK-PPLV2-PM.
- The names come from STD 070 §7.5.5, with positions taken from list order.
- DEC gives no codepoints.

**Practice.**

- [[source: vt100net-technical]] is Paul Williams' Unicode guesses plus a PNG of
  the LN03 glyphs.
  - It gives 2/11-2/14 as the parenthesis hooks U+239B, U+239D, U+239E and
    U+23A0.
  - It gives no codepoint for the seven summation pieces 3/1-3/7.
  - It gives ƒ U+0192 at 7/6, "Probably chosen for its meaning of
    "function"".
- xterm's `map_DEC_Technical` (`charsets.h:490-591`) is "derived from" that
  page.
  - Its `UNDEF` set, `38 39 3A 3B 52 54 55 6D 75`, equals EK-PPLV2-PM's mask.
  - 3/1-3/7 are **not** `UNDEF`. They are `XXX(0x31, PUA(0))` through
    `XXX(0x37, PUA(6))` (`:512-518`), each commented as a sigma piece. Through
    `PUA(n) (0xEEEE + (n))` (`:58`) they become private-use U+EEEE-U+EEF4.
    - The second pass said these positions were `UNDEF`. **That is refuted.**
    - PUA codepoints are still a departure of their own kind: DEC defines a
      glyph there.
  - 2/11-2/14 use the curly-bracket hooks U+23A7, U+23A9, U+23AB and U+23AD.
    That DEPARTS from STD 070's names ("top left parenthesis" and so on) and
    from Figure A-4's drawing, although Table A-1 lists 2/11-2/14 under both
    Curly Braces and Parentheses.
- Both put ↑ U+2191 at 7/12 and → U+2192 at 7/13, the order of EK-PPLV2-PM,
  VT382 and STD 070: vt100.net's rows "07/12 U+2191 UPWARDS ARROW" and "07/13
  U+2192 RIGHTWARDS ARROW", and `charsets.h:587-588`. Both put ≠ at 3/13 and ƒ
  at 7/6, and leave 7/5 undefined.

Q-Technical.

### 3.3 JIS-Roman

**DEC.**

- [[source: vt382-rm]] §2.4.5, p. 2-17 **[img]**: "This set
  differs from the ASCII Character Set only in that the Yen sign(¥)replaces the
  backslash(\) in column 5, row 12 (5/12) and the over line (¯) replaces the
  tilde (˜) in column 7, row 14 (7/14)." The glyphs in parentheses were read
  off the scan.
- [[source: dec-pplv2-pm]] Figure A-12 (PDF p. 196).
- EK-PPLV2-PM Table A-2 row: "JIS Roman | Macron | 7/14 ( ~ ) | 10/15"
  **[text]**.
  - So DEC names 7/14 "Macron" there.
  - Paired with ISO Latin-1, DEC builds the glyph from Latin-1 10/15 (U+00AF
    MACRON). That is how DEC constructs the glyph on a printer, not a ruling
    between U+00AF and U+203E, so it adds nothing to the name.
  - Paired with DEC Supplemental, it falls back to the ASCII tilde.
- STD 070 p. 8-85 names the source standard "JIS Roman/JIS X 0201" **[text]**.
- Not DEC: [[source: iso-ir-014]], the ISO-IR register entry for JIS C 6220
  Roman at "ESC 2/8 4/10", the same final `J`. Its registration page reads
  "This graphic character set is a Japanese version of ISO 646." **[img]** Its
  chart pages are not read, so it says nothing yet about 7/14 (Q-JIS-Roman).

**Can it be generated at build time?** Yes, from DEC's prose rule: ASCII with
two substitutions.

- 5/12 is the Yen sign, U+00A5.
- 7/14 is U+203E OVERLINE ("over line", VT382) or U+00AF MACRON ("Macron",
  Table A-2). DEC uses both names.
- [[source: unicode-jis0201]], which is not DEC and sits in `OBSOLETE/`, maps
  0x5C to U+00A5 and 0x7E to U+203E.

**Practice.**

- xterm's `map_JIS_Roman` (`charsets.h:692-698`) has U+00A5 and U+203E.
- It designates `J` only at level 3 (`charproc.c:1857`, `max_level` 3), so
  xterm's default ID does not reach it.

Q-JIS-Roman.

### 3.4 JIS-Katakana

**DEC.**

- [[source: dec-pplv2-pm]] §A.12: "This character set is specified by JIS X
  0201-1976. There are 63 characters defined in this character set. The
  remaining character positions are considered ''reserved for future use.''"
  **[text]** Figure A-31 is on PDF p. 221.
- [[source: vt382-rm]] §2.4.6 **[img]**: "When the VT382 receives
  14/0 through 15/14, the VT382 displays a reverse question mark on the
  screen."
- [[source: vt382-rm]] Figure 2-10, p. 2-18 (PDF 38), draws that reversed `?`
  in every one of those 31 cells, 14/0-14/15 and 15/0-15/14, and hatches
  15/15 **[img]**, read on a 300 dpi render.

**Recovered by script.**

- 2/1-5/15 are defined and 6/0-7/14 reserved (31 cells), which matches the 63.
- The figure has 0 glyphs in its text layer.

**Can it be generated at build time?** Not from a DEC text. The mask is DEC's
(a cited constant, since EK-PPLV2-PM is `external`), and the glyph identities
come from JIS X 0201, which DEC names. [[source: unicode-jis0201]] maps
0xA1-0xDF to U+FF61-U+FF9F (63 characters). [[source: iso-ir-013]], the
register's JIS C 6220 Katakana entry, has had its registration page read
(per its manifest entry); its chart pages are not read.

**Practice.** xterm's `map_JIS_Katakana` (`charsets.h:705-802`):

- 2/1-5/15 are U+FF61-U+FF9F;
- 6/0-7/13 are `UNDEF`;
- 7/14 is unlisted, and the macro has no default arm, so it draws `~`. That
  DEPARTS from DEC's "reserved".
- Designated only at level 3 (`charproc.c:1856`).

### 3.5 DEC Hebrew, Greek and Turkish Supplemental

**DEC.** [[source: dec-pplv2-pm]], all **[text]**:

- §A.8: "DEC 7-Bit Hebrew and DEC Hebrew Supplemental are 94-character graphic
  character sets".
- §A.9: "DEC Greek Supplemental is a 94-character graphic character set."
- §A.10: "DEC 8-Bit Turkish Supplemental and DEC 7-Bit Turkish are
  94-character graphic character sets."

| Set | Figure | Defined / reserved | Text cells recovered | Layout |
|---|---|---|---|---|
| DEC Hebrew Supplemental | A-22 (PDF 208) | 49 / 45 | 10 | Columns 2-3 as DEC Supplemental; columns 4-5 reserved; 6/0-7/10 the 27 letters in ISO 8859-8 order, read by eye |
| DEC Greek Supplemental | A-24 (PDF 211) | 81 / 13 | 68 | The same 13 reserved cells as DEC Supplemental |
| DEC Turkish Supplemental | A-27 (PDF 215) | 87 / 7 | 74 | Reserved `24 26 2C 2D 2F 34 38` |

- Two independent stroke detectors give the same masks. The "text cells
  recovered" counts are the second pass's and were not re-checked (§6).
- **Can they be generated at build time?** The masks can, as cited constants:
  they were read by script from the research copy of EK-PPLV2-PM, which is
  `external`. The glyph identities cannot be generated from a DEC text: they
  must be read off the figures by eye.
- **Practice.**
  - xterm's `map_DEC_Hebrew_Supp`, `map_DEC_Greek_Supp` and
    `map_DEC_Turkish_Supp` cite these figures (`charsets.h:2044, 1863,
    2225`).
  - Their `UNDEF` sets equal DEC's masks: CONFORMING on the masks.
  - Nobody has checked the glyphs cell by cell (§6).
  - All three designate only at level 5 (`charproc.c:1865-1867`).

### 3.6 DEC Cyrillic

**No DEC source was found.** It is not in:

- [[source: vt510-rm-b]], or the pinned VT510 transcriptions.
  [[source: vt510-scs]] and [[source: vt510-rm-ch4]] name it in their
  designator tables ("& 4 DEC Cyrillic") and chart nothing.
  [[source: vt510-ch7]] does not name it at all: the Cyrillic sets it names
  are KOI-8 Cyrillic, ISO Latin-Cyrillic and PC Cyrillic;
- [[source: vt520-rm]];
- [[source: vt420-rm]];
- EK-PPLV2-PM's 31 figures;
- the VT510, VT520 or VT525 installation guides.

The positive controls hit: "DEC Cyrillic" in the VT510 and VT520 designator
tables, and ISO Latin-Cyrillic as Figure A-30 in EK-PPLV2-PM.

- [[source: vt510-ch7]] §7.2.1 lists "KOI-8 Cyrillic" as a multinational set.
  Reading DEC Cyrillic as KOI-8's right half is **inference only**.
- The final, `& 4`, is in vt510-scs. baud is CONFORMING.
- Practice: xterm's `map_DEC_Cyrillic` is commented "DEC Cyrillic from
  screenshot" (`charsets.h:1661`). 2/1-3/15 are `UNDEF` and 4/0-7/14 follow
  KOI-8 order.

**Cannot be generated from a DEC text.**

### 3.7 foot

foot implements ASCII and DEC Special Graphics only (`foot-vt.c:545-571`).

- Every other final leaves the slot as it was.
- That CONFORMS, for a set it does not know, to STD 070-3 §3.6.2, to 070-2
  p. 2-17's algorithm and to VT420/VT510's "ignores unrecognized sequences"
  (§2.8).
- It DEPARTS on every set in this section by not drawing it.

### 3.8 `Charset::to_u8` and `from_u8`: dead code, with stale docs (D3)

- `Charset::to_u8` maps every `Dec(_)` to 4
  (`baud-domain/src/vt/charset.rs:222`), and `Charset::from_u8(4)` returns
  `Dec(DecSet::Supplemental)` (`:234`). The pair is lossy for every `DecSet`
  but `Supplemental`.
- **Nothing calls either.** The only `.to_u8()` call in the workspace outside
  `to_u8` itself is `baud-core/tests/vt100/charsets.rs:709`, and it calls
  `Nrcs::to_u8`. No code calls `Charset::from_u8`.
- **`Nrcs::from_u8` (`charset.rs:135`) goes dead with it.** Its only caller is
  `Charset::from_u8` (`:235`). `Nrcs::to_u8` (`:116`) keeps a caller, the
  test at `charsets.rs:709`.
- **The mechanism their docs describe does not exist in this core.**
  - Nothing in `baud-core` builds a `Run`, and no `promote` exists there.
    `baud-domain/src/cell.rs:336-338` says `promote` was `baud-core2`'s and
    that "Nothing in this core calls" `from_wire`.
  - DECSC saves the slots as `[Charset; 4]` directly (`baud-core/src/ops.rs:75`,
    restored at `:109`), not through `to_u8`.
- So the loss reaches no cell, now or when a `DecSet` first translates. It is
  dead code whose docs state a mechanism as fact.
- **The seven stale sites:**
  - `charset.rs:209-215`, `to_u8`'s doc: "A run has to carry it: `promote`
    replays stored bytes into cells long after the designation";
  - `charset.rs:198-200`, `from_flags`'s doc: "The national sets reach the cell
    through `Run::charset` instead";
  - `charset.rs:115`, `Nrcs::to_u8`'s doc: "Dense index, for storing a
    [`Charset`] in one byte". Once `Charset::to_u8` goes, nothing stores a
    `Charset` in a byte, and the only caller left is the test;
  - `baud-domain/src/store.rs:89-95`, `Run::charset`'s doc, the same claim;
  - `docs/vt-coverage.md:251`: "the run records which set it was written
    under so promotion cannot change it";
  - `baud-core/tests/vt100/charsets.rs:572-577`, the doc of
    `a_promoted_run_keeps_the_set_it_was_written_under`. The test still checks
    something real, that cells already written keep their glyph when the set
    is designated again. It does not exercise `Run::charset` or any promotion,
    so its name and doc describe a mechanism it never reaches. It designates
    German with no `?42`, so it also pins Q-DECNRCM (a);
  - `baud-core/tests/vt100/charsets.rs:587-593`, the comments inside that
    test, which explain its writes by `MAXRUN` ("a ninth run on the line
    promotes it") and `OpMode::Runs`, both `baud-core2`'s.

---

## 4. Open questions

Each option is marked **CONFORMING** or **DEPARTS** against named DEC texts,
and practice is marked the same way. A recommendation is given only where one
option departs from no text, and the departures are stated first. Where every
option departs from some text, or rests on a text nobody has read, there is no
recommendation.

### Q-UNDEF. What an undefined position in a designated set draws (owner; with note AE Q8's codepoint row)

**Where it arises.**

- The DEC reserved positions in §3, each set with a text that says its
  reserved positions print the error character:
  - DEC Supplemental 13 (§A.2);
  - Technical 9 (§A.4);
  - Hebrew Supplemental 45 (§A.8);
  - Greek Supplemental 13 (§A.9);
  - JIS-Katakana 31 (§A.12; VT382 §2.4.6 for GR).
- DEC Turkish Supplemental's 7 reserved cells. Figure A-27 hatches them, but
  §A.10 has no error-character sentence (§3), so what they print rests on
  the other sections by analogy (**inferred**).
- Greek NRC 7/9 and 7/10, which xterm marks `UNDEF` "unused". There is no
  DEC table, so whether DEC reserves them is **unknown**.

**What DEC calls the character.**

- [[source: dec-pplv2-pm]] §A.2, §A.4, §A.8, §A.9, §A.12: "The error character
  is printed when one of these reserved characters is received by the device."
  **[text]**
- EK-PPLV2-PM chapter 6, "Control Characters", p. 6-4, under SUB: "SUB in
  printable text causes the error character (a reverse question mark) to be
  printed." **[text]**
- [[source: vt382-rm]] §2.4.6: "displays a reverse question mark" **[img]**.
- [[source: vt382-rm]] Figure 2-10 (PDF 38) draws it: a reversed `?` in all 31
  reserved JIS-Katakana cells, 14/0-15/14 **[img]** (§3.4). That is DEC's own
  drawing of the error character. VT382 Figure 2-8 (DEC Technical) leaves its
  nine reserved cells blank instead **[img]** (§3.2).
- VT420 Figure 2-10 draws a reversed `?` at 3/8 and 7/6 only **[img]**. The
  page's text layer has exactly two `?` in the figure, at those two cells
  **[text]**. Its other cells that EK-PPLV2-PM reserves (3/9-3/11, 5/2, 5/4,
  5/5, 6/13) carry no glyph, and 7/5 carries an italic `f`, read as ƒ
  (**inferred**, §3.2). A blank cell in a poppler
  render is not evidence on its own: Ω at 5/7 renders blank too, though it is
  in the text layer.

Today baud draws the ASCII byte (`cell.rs:396`).

**Options.**

- **(a) U+2426 SYMBOL FOR SUBSTITUTE FORM TWO.**
  - CONFORMING to the texts above if U+2426 is accepted as the image of DEC's
    "reverse question mark".
  - Its Unicode name does not say so, and [[source: unicode18-ch22]] §22.7.1,
    "Control Pictures", describes U+2400..U+2425 and U+2427..U+2429 and never
    mentions U+2426 **[text]**. The code chart does: its names list reads
    "2426 ␦ SYMBOL FOR SUBSTITUTE FORM TWO • from ISO 2047 → 061F arabic
    question mark" **[text]**, and its reference glyph is a reversed question
    mark **[img]** ([[source: unicode18-chart-2400]], pinned 2026-10-09, after
    the rest of this note was written). So U+2426 is ISO 2047's picture of SUB,
    the control whose display DEC defines as this character. xterm's comment,
    `#define UNDEF 0x2426 /* rendered as a backwards "?" */` (`charsets.h:60`),
    agrees, as practice.
  - Practice: xterm, for `UNDEF` and for SUB, each under conditions.
    - Its tables put `UNDEF` at reserved positions through `XXX` entries.
      `xtermCharSetOut` applies an `XXX` entry only in a build with
      `OPT_WIDE_CHARS`, and there only while `utf8_nrc_mode` is nonzero
      ([[source: xterm-charsets-c]] `charsets.c:340-346`). The maps of the
      DEC sets in §3 raise that counter for themselves outside national mode
      (`begin_CODEPAGE`, `charsets.h:67-75`, which opens
      `map_DEC_Supp_Graphic` at `:438-439`, for one), and
      `modified_DECNRCM` swaps it in when national mode changes under UTF-8
      ([[source: xterm-reset]] `charproc.c:1292-1295`). So a DEC set's
      `UNDEF` entries apply while national mode is off, and an NRC map's,
      such as Greek 7/9 and 7/10, only in national mode under UTF-8.
    - Its CASE_SUB writes nothing at ID 52, in any build, and code 2 at the
      other IDs in a build without `OPT_WIDE_CHARS` (`charproc.c:3665-3672`).
      In a build with it, it picks `0x2426` when `terminal_id > 200`:
      `dotext` goes on with U+2426 at decTerminalID 420, the default, in a
      UTF-8 locale, where GL and GR hold ASCII at reset, and with `&`
      outside UTF-8 mode with ASCII in GL. The VT100-level IDs write U+2592.
      The conditions and their lines are in §5(a)6, D5.
    - Read from source, not run.
- **(b) U+FFFD REPLACEMENT CHARACTER.**
  - DEPARTS from DEC's description and drawing. DEC calls the character "a
    reverse question mark" (EK-PPLV2-PM p. 6-4; VT382 §2.4.6) and draws one
    (VT382 Figure 2-10, 31 cells). U+FFFD's name, `FFFD;REPLACEMENT
    CHARACTER` ([[source: unicode-data]] line 16976), does not say so, and the
    glyph the Unicode Standard prints for it is not one:
    [[source: unicode18-ch23]] §23.8.4, "Replacement Characters", sets U+FFFD
    beside its name as an inline SVG glyph, and rendered, that glyph is a
    white question mark, not reversed, in a black diamond **[img]**.
  - The same section gives U+FFFD its role: "The U+FFFD � REPLACEMENT
    CHARACTER is the general substitute character in the Unicode Standard. It
    can be substituted for any “unknown” character in another encoding that
    cannot be mapped in terms of known Unicode characters." **[text]** A byte
    at a DEC reserved position fits that role (**inferred**), but the role is
    Unicode's, and the drawing DEC asks for is a reversed question mark.
  - baud already writes U+FFFD for a decoding error, so a reserved position
    would look like one.
- **(c) The ASCII byte (today).**
  - DEPARTS from EK-PPLV2-PM §A.2, §A.4, §A.8, §A.9 and §A.12, and from
    VT382 §2.4.6 and Figure 2-10.
  - Practice: foot, and xterm's JIS-Katakana 7/14 (§3.4).
- **(d) U+2E2E REVERSED QUESTION MARK.**
  - [[source: unicode-data]] line 10886: `2E2E;REVERSED QUESTION MARK`.
  - CONFORMING to the texts above by name: it is the only option whose
    Unicode name is DEC's description, "a reverse question mark"
    (EK-PPLV2-PM p. 6-4; VT382 §2.4.6). It is still an identification, since
    DEC names no codepoint.
  - Practice: neither xterm nor foot uses it.
  - Whether baud's embedded font has a glyph for it is unchecked.

**On the codepoint.** (b) and (c) depart. (a) and (d) depart from no text
read, and DEC gives no codepoint for either. (d) matches DEC's words by name
([[source: unicode18-chart-2e00]]: "2E2E ⸮ REVERSED QUESTION MARK = punctus
percontativus" **[text]**, a reversed question mark **[img]**), but it is
punctuation. (a)'s reference glyph is the same shape ([[source:
unicode18-chart-2400]]), and it is ISO 2047's symbol for SUB, the control DEC
ties the error character to. Asked once more with both charts read and every
option marked, Jev picked (a) at 1.00 and again at 1.00 on a repeat
(2026-10-09; `.claude/research/errchar/`). The codepoint stays the owner's to
confirm, with that recommendation.

**One codepoint serves three places.** This is the same codepoint as:

- note AE §10's row "Q8, the error character's codepoint". Jev answered U+2426
  at 0.26 and did not lean. That row is the AE "Q8.1" this note was asked to
  tie to; AE numbers no Q8.1. AE's Q8 offers U+2426 or U+FFFD, and does not
  list U+2E2E. Option (d), and this note's finding that U+FFFD departs from
  DEC's description and VT382 Figure 2-10's drawing by the glyph TUS
  §23.8.4 prints, bear on that row too;
- the SUB row at `docs/vt-coverage.md:190` (text at `:192-198`), which keeps
  the display `out` on a stated basis that this note finds misstated (D5,
  §5).

Decide all three together.

### Q-Greek. The Greek NRC set has no DEC table (owner)

The evidence is in §2.7. No DEC chart exists among the texts read, and the
standard that would hold one is not online.

- **(a) Keep xterm's ELOT927-derived table as it is.**
  - There is no DEC table for it to depart from.
  - DEPARTS (**inferred**) from Greek alphabetical order at 6/14 and 7/6, and
    from xterm's own comments.
  - ELOT 927 is unread.
  - Practice: xterm, but only at a non-default terminal ID (§2.7).
- **(b) Keep it, and swap 6/14 and 7/6 to Ξ (U+039E) and Χ (U+03A7).**
  - **Inferred** CONFORMING to alphabetical order and to xterm's comments.
  - DEPARTS from xterm's codepoints.
  - It is source-backed only once ELOT 927, or Kermit 95's ELOT927 table, is
    read.
- **(c) Designate it and draw ASCII**, as baud does for `% 3` and `& 5`.
  - No DEC chart is read, but [[source: vt510-ch7]] §7.2.1 lists "DEC 7-bit
    Greek" as a set the terminal draws, and drawing ASCII is not drawing it.
  - So (c) departs in effect.
- 7/9 and 7/10 follow Q-UNDEF under (a) and (b).
- **No recommendation.** Every option rests on a text nobody has read, or
  departs from one.

### Q-Unlisted. What an unlisted final does to its slot (Jev)

The evidence is in §2.8. It covers every SCS final DEC does not list, one byte
or two, and Q-French-f's option (b2) depends on it.

- **(a) Designate `Charset::Other` (today).**
  - DEPARTS from STD 070-3 §3.6.2, p. 3-62 ("it remains in that set until
    explicitly replaced by another character set designator"), from 070-2 p.
    2-17's algorithm (it designates only on a table match), and from the
    VT420/VT510 general rule ("ignores unrecognized sequences").
  - Practice: neither xterm nor foot. xterm DEPARTS another way, designating
    ASCII (`charproc.c:1938-1940`).
- **(b) Ignore: leave the slot as it was, and keep counting the final in
  `charset_unsupported`.**
  - CONFORMING to all three texts.
  - Practice: foot leaves the slot, CONFORMING on this point (§3.7). It counts
    nothing.
  - Counting keeps an ignored final observable, which is what the counter is
    for.
  - What it changes:
    - the counter at `baud-core/src/paint.rs:117-118` ("`Charset::Other` is
      what the slot holds, and this is how often") comes to mean "how many
      designations were ignored";
    - `an_unlisted_final_is_still_other` (`baud-core/tests/vt100/charsets.rs:429`)
      and `an_unlisted_multi_byte_final_is_counted` (`:466`) are rewritten;
    - `Charset::Other`'s doc (`charset.rs:55-59`), "Designated, so the slot is
      not mistaken for ASCII", is reversed;
    - `every_spec_esc_intermediate_form_is_handled_or_counted`
      (`baud-core/tests/conformance/spec_inventory.rs:769`) turns red unless
      it changes too. Its `Snapshot` (`:274-312`) holds `unsupported`,
      `unsupported_param` and the four slots, but not `charset_unsupported`.
      `charset_finals` (`:741`) takes every single byte from ctlseqs' `C = `
      lines, and six of the 29 it collects are finals `from_final` answers
      `Other`: `F`, `L` and `M` from the 96-set lines 386, 388 and 389, and
      `2`, `3` and `?`, the last bytes of `% 2`, `% 3` and `" ?` (lines 343,
      367 and 359). Under `ESC (`, `ESC )`, `ESC *` and `ESC +` (lines 316,
      370, 373 and 376) that is 24 forms. Today each shows as a change in a
      slot. Under (b) the slot is unchanged, so the test reports all 24 as
      silently consumed. The fix is to add `charset_unsupported` to
      `Snapshot`: each is counted, so it is handled, not silent.
  - Whether `Charset::Other` survives as a variant is a code question, not a
    conformance one.
- If Q-DECNRCM is answered (b) and its (b.1) as "ignore", the same path
  serves an NRC designation outside national mode.
- **Recommend (b), departure first:** it departs from xterm, which is
  practice, and from no DEC text. Jev decides at 0.9 or better. It changes
  behaviour that tests pin, so it is not in §5(a).

### Q-French-f. Whether `ESC ( f` designates French (Jev)

The evidence is in §2.8.

- **(a) Keep `f` as French (today).**
  - DEPARTS from VT220 Table 4-4, the Pocket Guide p. 28, VT420 Table 5-3,
    vt510-scs, STD 070 pp. 5-117 and 7-18, and EK-PPLV2-PM Table 7-7. Each lists
    French at `R` alone.
  - Practice: xterm (`nrc_French2`, gated on national mode) and ctlseqs 338.
- **(b1) `f` leaves `from_final` and becomes `Charset::Other`.** That is
  where an unlisted final goes today
  (`baud-domain/src/vt/charset.rs:283`), and `esc_final`
  writes it into the slot (`baud-core/src/write.rs:1714-1718`), replacing the
  set there with one that draws ASCII.
  - DEPARTS from STD 070-3 §3.6.2, from 070-2 p. 2-17's algorithm, and from
    the VT420/VT510 rule that "the terminal ignores unrecognized sequences"
    (§2.8).
  - DEPARTS from xterm practice and ctlseqs 338.
- **(b2) `f` is ignored, and the slot is unchanged.**
  - CONFORMING to every DEC text read. No DEC text lists `f`, and the texts
    on designation leave the slot as it was.
  - DEPARTS from xterm practice and ctlseqs 338.
  - It needs the ignore path. Either it lands with Q-Unlisted (b), and then
    `f` only leaves `from_final`, or (b2) special-cases `f` while every other
    unlisted final stays `Other` until Q-Unlisted is answered. The first is
    one rule. The second leaves the departure Q-Unlisted (a) records in place
    for every other final.
  - What it changes, either way:
    - `every_spec_esc_intermediate_form_is_handled_or_counted`
      (`baud-core/tests/conformance/spec_inventory.rs:769`): `charset_finals`
      (`:741`) collects `f` from ctlseqs line 338, so `ESC ( f` and its `)`,
      `*` and `+` forms are fed. Under (b2) they leave the slot unchanged, and
      the test reports them as silent unless `f` is counted in
      `charset_unsupported` and that counter is added to `Snapshot`
      (`:274-312`), as under Q-Unlisted (b);
    - `every_listed_single_byte_final_is_named` (`charsets.rs:376`), whose
      row at `:386` pins `ESC ( f` as French, is rewritten;
    - the stale `f` goes from `charset.rs:92`, `Nrcs::French`'s doc ("`R` or
      `f`"), and from `docs/vt-coverage.md:557`, whose row lists `f` among the
      NRCS finals.
- **Unknown.** ECMA-35 registers 94-set finals from 4/0, and which set is
  registered at 6/6 has not been read.
  - If xterm's `f` came from a register entry for NF Z 62-010:1982, its
    repertoire is not DEC's French.
  - STD 070 p. 6-58 names DEC's French "AFNOR NF Z 62-010 [1973]" **[img]**.
- **Recommend (b2), departure first:** it departs from xterm and ctlseqs,
  which are practice, and from no DEC text. (b1) departs from DEC and is not
  recommended. Jev decides at 0.9 or better, preferably with Q-Unlisted.

### Q-ND-backtick. Which repertoire `` ` `` designates, and with it `E` and `6` (owner)

The evidence is in §2.6.

STD 070's marks below rest on the inference in §2.6: STD 070 names one
Norwegian/Danish set, NS 4551-1, for `` ` ``, `E` and `6`, and NS 4551-1 is
read as the ISO repertoire. NS 4551-1 itself is unread (§6).

VT420's marks for `E` and `6` rest on a name: its Table 5-3 says "DEC
Norwegian/Danish 6 or E", and its Table 2-1 charts only the ISO set, so the
VT420 never charts the DEC repertoire (§2.6). Every VT420 mark for `E` and `6`
below is **inferred** on that name.

- **(a) All three finals designate the DEC repertoire (today).**
  - For `E` and `6`: CONFORMING to VT220 Table 4-4 and Table 2-12, to
    EK-PPLV2-PM Tables 7-7 and 7-8 with Figure A-13, and to VT420 Table 5-3 by
    name (**inferred**). STD 070 p. 5-117 lists the finals and gives no
    repertoire. Against STD 070's one set, DEPARTS (**inferred**).
  - For `` ` ``: DEPARTS from VT420 Table 5-3 and Table 2-1 and from
    EK-PPLV2-PM Table 7-7 and Figure A-16, at four positions. DEPARTS from STD
    070 pp. 6-58 and 7-18 (**inferred**).
  - Practice: xterm.
- **(b) `` ` `` designates the ISO repertoire; `E` and `6` the DEC one.**
  - For `` ` ``: CONFORMING to VT420 and EK-PPLV2-PM, and to STD 070
    (**inferred**).
  - For `E` and `6`: CONFORMING to VT220 and EK-PPLV2-PM, and to VT420 by
    name (**inferred**). DEPARTS from STD 070's one set (**inferred**).
  - DEPARTS from vt510-scs only if its single row is read as one repertoire
    for all three finals.
  - DEPARTS from xterm practice.
  - The ISO set, per VT420 Table 2-1, substitutes six positions: Æ Ø Å at
    5/11-5/13 and æ ø å at 7/11-7/13. It keeps `@ ^ ` ~`.
- **(c) All three designate the ISO repertoire.**
  - CONFORMING to STD 070 (**inferred**): one set, NS 4551-1, for all its
    finals (pp. 5-117, 6-58, 7-18 and 8-85).
  - CONFORMING to vt510-scs under the one-repertoire reading, with vt510-ch7's
    "(ISO)" label in support.
  - DEPARTS from VT220 Table 2-12 for `E` and `6`, from EK-PPLV2-PM Tables 7-7
    and 7-8 with Figure A-13, and from VT420 Table 5-3 by name
    (**inferred**).
  - The same vt510-ch7 list calls Swedish "(ISO)" against two DEC texts
    (§2.6), so the label is weak evidence.
- **No recommendation.**
  - VT220 and EK-PPLV2-PM chart the DEC repertoire under `E` and `6`, and
    VT420 names it there (**inferred**). STD 070, on the inferred reading,
    puts the ISO one there. No option conforms to all four.
  - (b) is the only option that conforms to VT220, EK-PPLV2-PM and VT420 (the
    last by name, **inferred**) together. It departs from STD 070 at `E` and
    `6` (**inferred**), and from vt510-scs under the one-repertoire reading.
  - (c) conforms to STD 070 and to vt510-scs under that reading, and departs
    from the other three.
  - Which texts govern, and whether the NS 4551-1 inference holds, is the
    owner's.

### Q-DECNRCM. Whether the NRC sets act only in national mode (owner)

The evidence is in §2.10.

- **(a) Ungated (today).**
  - DEPARTS from VT220 §2.4.3, Table 4-4's note and §4.6.19, from VT420 p. 27
    and Table 5-3's `*` footnote, and from STD 070 §5.8.1 note 4 (pp.
    5-115-5-116) and p. 7-18.
  - DEPARTS from xterm practice (`charproc.c:1929`).
  - Keeps `docs/vt-coverage.md:251` and `:557` true and `:760` false.
- **(b) Implement DECNRCM (`CSI ? 42 h/l`, reset by default), and translate an
  NRC set only while it is set.**
  - CONFORMING on the gate to every DEC text above.
  - Practice: xterm.
  - It changes what an application sees today: `ESC ( K` alone stops drawing
    German until `?42` is set.

**Sub-points under (b), each the owner's.**

- **(b.1) What an NRC designation does outside national mode.**
  - xterm designates ASCII.
  - VT420/VT510's "ignores unrecognized sequences" would leave the slot as it
    was, if an unavailable set counts as unrecognized (**inferred**).
  - STD 070-3 §3.6.2 has each terminal "maintain a repertory of available
    character sets, which may be designated" **[text]**. If an NRC set is not
    in that repertory outside national mode, its final matches nothing, and
    by 070-2 p. 2-17 the slot stays as it was (**inferred**). That is the
    path Q-Unlisted (b) builds.
  - No DEC text read says which directly.
- **(b.2) Whether national mode withdraws the other sets.** vt510-ch7 §7.3
  says that in that mode "only ASCII, NRC sets, and DEC Special Graphic
  characters are available". Under that sentence, DEC Supplemental and the
  rest are unavailable while `?42` is set.
- **(b.3) One NRC set at a time, chosen by a keyboard dialect.**
  - VT220 §2.4.3 and VT420 p. 27 state it for those devices.
  - STD 070 §5.8.1 note 4 (pp. 5-115-5-116) has the one set "selected under
    local user control (Set-Up) to replace U.S. ASCII as the default set",
    and makes the keyboard binding implementation-dependent ("Depending on
    the implementation, only one NRC set corresponding to the keyboard
    dialect may be selectable at a time") **[img]**.
  - baud has no keyboard dialect and no Set-Up choice of NRC set. xterm does
    not check either.
- **(b.4) UK.** DEC lists British among the NRC sets (VT220 Table 4-4; STD
  070 p. 5-117). STD 070 §5.8.1 note 3 (p. 5-115) gives the older UK set as a
  Level 1 extension "selected in Set-Up to replace ASCII (ASCII and UK cannot
  be designated simultaneously)", since "replaced by the NRCS Extension"
  **[img]**. baud designates UK by `ESC ( A` into any slot, beside ASCII in
  the others. Whether note 3's exclusion binds baud's UK depends on the level
  reading (§2.8) and on whether UK is treated as note 3's set or as one of
  note 4's NRC sets. xterm gates British in `xtermCharSetOut`, not in
  `xtermDecodeSCS`, and that gate also passes at `vtXX_level <= 1` without
  national mode (`charsets.c:404-406`, §2.10).
- **(b.5) UTF-8.** xterm turns UTF-8 off while national mode is set
  (`charproc.c:1287-1299`). Whether baud's NRC sets act under UTF-8 at all
  belongs here too.

**No recommendation.**

- (a) departs from every DEC text read.
- (b) conforms on the gate, but its sub-points include (b.3), where VT220 and
  VT420 bind one set to the keyboard and baud has no keyboard dialect to bind
  to.
- This is decided with the level reading behind note AE's Q9: which level
  baud claims. DA answers VT102, and `report.rs:201-204` says baud has no
  NRCS (D6). AE's Q9 itself asks about gating the shifts. Gating
  designations by level is an extension of it, and has to be stated as one
  (§2.8, STD 070 §5.8.1).
- **What stays open in §5(a).** §5(a)'s new tests check tables, not
  rendering through `Term`, so they build in neither option. Three existing
  tests in `charsets.rs` designate an NRC set through `Term` with no `?42`.
  Each pins (a) today and changes under (b):
  - `the_national_sets_substitute` (`:537`) sends `ESC ( A #`, `ESC ( K [`
    and `ESC ( R @`, and expects £, Ä and à. Its German and French cases
    fail under (b), and its UK case turns on (b.4);
  - `a_promoted_run_keeps_the_set_it_was_written_under` (`:579`) sends
    `ESC ( K [` and expects U+00C4;
  - `every_national_replacement_renders_as_its_table_says` (`:687`) drives
    every pair.

  (b.3) decides whether a terminal can carry more than one set, and (b.5)
  whether the sets act under UTF-8. §5(a) edits the first two: the first's
  doc ((a)1, (a)4), and the second's name and doc ((a)3). Neither edit may
  hide that the test pins (a).

### Q-SCS-Russian. What `% 3` and `& 5` draw (owner)

The evidence is in §2.9. DEC names both sets, VT520 names SCS as
Serbo-Croatian and Russian as KOI-7, and no DEC chart exists among the texts
read.

- **(a) Designate and draw ASCII (today).**
  - Departs in effect from vt510-ch7 §7.2.1, which lists both as sets the
    terminal draws.
- **(b) xterm's tables.**
  - SCS from ISO-IR 141, chosen by xterm from VT520's "Serbo-Croatian".
  - Russian from Kermit's KOI-7, "though the dialect is unknown".
  - Practice at level 5 only.
  - Neither ISO-IR 141 nor a KOI-7 standard has been read.
- **(c) A KOI-7 table from the KOI-7 standard itself, and SCS left as (a).**
  - This rests on a standard nobody has read. VT520 p. 7-2 names KOI-7 and not
    the dialect.
- **No recommendation.** Every option rests on an unread text.
- Whichever is chosen, `baud-domain/src/vt/charset.rs:184-190` should name
  both sets as DEC does (§5(a)).

### Q-JIS-Roman. What JIS-Roman 7/14 is (owner)

The evidence is in §3.3.

- ISO-IR 014, the register entry at the same final, calls the set "a
  Japanese version of ISO 646" **[img]**; its chart is unread, so it bears on
  this question only once read (§3.3).
- **(a) U+203E OVERLINE.**
  - Matches VT382 §2.4.5's "over line" **[img]** and
    [[source: unicode-jis0201]].
  - DEPARTS from EK-PPLV2-PM Table A-2's name, "Macron".
  - Table A-2's construction from Latin-1 10/15 is not a ground either way:
    it is how DEC builds the glyph on a printer, not a ruling between U+00AF
    and U+203E (§3.3).
  - Practice: xterm, at level 3 only.
- **(b) U+00AF MACRON.**
  - Matches EK-PPLV2-PM Table A-2's name.
  - DEPARTS from VT382's "over line".
- **No recommendation.** Each option departs from one DEC name, and DEC gives
  no codepoint. 5/12 is U+00A5 under both (VT382, Unicode JIS0201.TXT,
  xterm).

### Q-Technical. Which DEC layout, and which hooks (owner for the layout; Jev for the hooks)

The evidence is in §3.2.

**The layout (owner).**

- **(i) EK-PPLV2-PM's layout:** ≠ at 3/13, 7/5 reserved, ƒ at 7/6, ↑ at
  7/12 and → at 7/13, 85 defined.
  - CONFORMING to EK-PPLV2-PM §A.4 and Figure A-4; to VT382 Figure 2-8 at the
    cells read (3/13, 7/5, 7/6, 7/11-7/14 and the nine reserved cells)
    **[img]**; and to STD 070 §7.5.5's count and its order at 3/13 and
    7/12-7/13 **[img]**.
  - DEPARTS from VT420 Figure 2-10 at five cells: 3/13, 7/5, 7/6, 7/12 and
    7/13.
  - Practice: vt100.net and xterm.
- **(ii) VT420 Figure 2-10's layout:** `=` at 3/13, an italic `f` at 7/5
  (read as ƒ, **inferred**), a reversed `?` at 7/6 (read as reserved,
  **inferred**), → at 7/12 and ↑ at 7/13.
  - DEPARTS at those five cells from EK-PPLV2-PM Figure A-4 and VT382 Figure
    2-8, and from STD 070's order at 3/13 and 7/12-7/13.
- **No recommendation.** Each departs from a DEC manual. That is a count, not
  a ranking: the VT420 stands alone against three DEC texts at all five
  cells (§3.2), and (i) still departs from it, so by this note's rule the
  layout is the owner's.

**2/11-2/14 (Jev).**

- **(α) The parenthesis hooks U+239B, U+239D, U+239E and U+23A0.**
  - CONFORMING to STD 070 §7.5.5's names, checked on the image of p. 7-26
    **[img]**, and to Figure A-4's drawing.
  - Practice: vt100.net.
- **(β) The curly-bracket hooks.**
  - DEPARTS from both.
  - Practice: xterm.
- **Recommend (α):** it departs from no DEC text.

**3/1-3/7.** DEC defines seven summation pieces. vt100.net gives no codepoint,
and xterm uses U+EEEE-U+EEF4. Any choice is an identification DEC does not
make, so it waits on this question with (i).

### Q-Order. Which untranslated DEC set to build first (Jev, then the owner)

Translating any of these sets moves toward DEC, and not translating departs.
So every order departs from no text, and the question is one of evidence and
cost.

**Recommended order, by strength of evidence:**

1. **DEC Supplemental.**
   - It is the only set with a committed DEC table that a script reads
     whole.
   - Its mask is confirmed by a second DEC figure.
   - Note AE's Q2(a) needs it for GR.
   - Its 13 reserved cells wait on Q-UNDEF.
   - STD 070 §5.8.1 requires it at Levels 2-4 and does not list it for
     Level 1 (§2.8). Translating it while DA answers VT102 rests on the level
     reading behind note AE's Q9, which this note does not choose.
2. **JIS-Roman**, after Q-JIS-Roman: DEC's prose rule, two positions. STD
   070 §5.8.1 makes it part of the Katakana Extension, from Level 2 (§2.8).
3. **DEC Technical**, after Q-Technical. STD 070 §5.8.1 makes it an
   extension at Levels 3 and 4 only (§2.8).
4. **DEC Hebrew, Greek and Turkish Supplemental.** The masks are by script,
   carried as cited constants. The glyphs are by eye from EK-PPLV2-PM, and
   need a cell-by-cell second reading first (§6).
5. **JIS-Katakana.** DEC's mask, with glyphs from JIS X 0201, which DEC names.
6. **DEC Cyrillic: not built.** There is no DEC text, and that reason is
   recorded on the row.

Every one of these waits on Q-UNDEF for its reserved cells. DEC Supplemental's
defined positions could land with the reserved cells still drawing their byte,
departure stated. Whether to do that is part of this question.

### Routing

| Question | Who decides | Why |
|---|---|---|
| Q-Greek, Q-ND-backtick, Q-DECNRCM, Q-SCS-Russian, Q-JIS-Roman, Q-Technical (layout) | **Owner, whatever Jev says** | No option is free of a departure, or every option rests on an unread text. For Q-ND-backtick, STD 070's one set (inferred to be the ISO repertoire) conflicts with VT220, EK-PPLV2-PM and VT420 (the last by name, inferred) at `E` and `6` |
| Q-UNDEF | **Owner, whatever Jev says** | DEC gives no codepoint, and each option that departs from no text is an identification DEC does not make. The choice also settles note AE's Q8 codepoint row and the SUB row |
| Q-Unlisted, Q-French-f, Q-Technical (hooks) | **Jev, at 0.9 or better** | The recommended option departs from no DEC text, only from practice. For Q-French-f that is (b2), not (b1), which departs from DEC |
| Q-Order | **Jev, then the owner** | A priority, not a conformance mark; the owner sets the work order |

---

## 5. Build order

**Defects found by reading, none executed.**

| Id | Defect | Where described | Fixed by |
|---|---|---|---|
| D1 | The generator drops every `MAP` line without a `/* U+ */` comment, and Greek's two `XXX(…, UNDEF)` lines: Swedish 4/0 is lost | §2.3 | (a)1 |
| D2 | Both NRC tests compare baud with itself, so neither can see D1 | §2.3 | (a)1, (a)2 |
| D3 | `Charset::to_u8`/`from_u8` are dead code, and `Nrcs::from_u8` with them: lossy for every `DecSet` but `Supplemental`, called by nothing, and documented, at seven sites, as serving a `Run`/`promote` mechanism this core does not have | §3.8 | (a)3 |
| D4 | `docs/vt-coverage.md:760` contradicts `:251` and `:557` | §2.10 | (a)6 |
| D5 | The SUB row, `docs/vt-coverage.md:190` (text at `:192-198`), says xterm writes no glyph for SUB. xterm 411 writes nothing at decTerminalID 52, in any build; code 2 at the other IDs in a build without `OPT_WIDE_CHARS`; and in a build with it, U+2426 at decTerminalID 420, its default, in a UTF-8 locale, with ASCII in GL and GR, `&` outside UTF-8 mode with ASCII in GL, and U+2592 at the VT100-level IDs | below, (a)6 | (a)6 (the fact); Q-UNDEF (the decision) |
| D6 | `report.rs:201-204` says baud has no NRCS | below, (a)6 | (a)6 |
| D7 | `baud-domain/src/vt/charset.rs:184-190`: the name of `& 5`, and a reason that has gone stale. `docs/vt-coverage.md:560` has neither (see (a)6) | §2.9 | (a)6 |

### (a) Buildable now, with no decision

1. **The generator stops dropping lines.**
   `scripts/gen-nrcs-tables.py`:
   - fails on any `MAP`, `UNI` or `XXX` line inside a wanted macro that it
     cannot parse;
   - resolves each `MAP` line's X keysym from [[source: xorg-keysymdef]]
     rather than from the line's comment, by the `#define`'s value and not by
     its `U+` comment. `XK_Ooblique` (`charsets.h:353`) has no `U+` comment
     there (line 786, "deprecated alias for Oslash"), so a resolver that reads
     comments and fails on a line it cannot parse stops at it; following the
     alias works too (§2.3). Swedish `0x40` then comes out as U+00C9
     (`XK_Eacute`, line 769) with no override, and no other set changes. Its
     DEC primaries are [[source: dec-special-graphics]] Table 2-14 (line 7307),
     [[source: vt220-pocket-guide]] p. 16, [[source: vt420-rm]] Table 2-1 and
     [[source: dec-pplv2-pm]] Figure A-15, and the conformance test in item 2
     holds the value to Table 2-14;
   - checks each set's emitted count against that macro's line count in the
     source, adjusted for two explicit lists:
     - **A named skip list.** It holds Greek's two `XXX(…, UNDEF)` lines
       (`charsets.h:2380-2381`), each marked "awaits Q-UNDEF". The count check
       passes without anyone choosing what `UNDEF` draws.
     - **A DEC-override table.** It holds entries xterm's table lacks, each
       citing its DEC primary by key. Today that is one entry: Turkish `0x21`
       → U+0131, from [[source: dec-pplv2-pm]] Figure A-26 (§2.4). The value
       is typed into the table by hand with its citation: EK-PPLV2-PM is
       `external`, so nothing can extract it at generation.
   - The generator emits the override's DEC citation into the Turkish
     table's doc, so the output says which entry is DEC's and which xterm's.
   - `nrcs_data.rs` stays generated. Nothing is hand-edited.
   - **Docs that go stale with the override**, because they call every table
     xterm's judgement: the generator's docstring
     (`scripts/gen-nrcs-tables.py:5-8`); the table docs in `nrcs_data.rs`,
     which name only xterm's macro (the Turkish one at `:217`);
     `charset.rs:77-79`; `docs/vt-coverage.md:251`; and
     `charsets.rs:531-535`, the doc of `the_national_sets_substitute`. That
     test pins Q-DECNRCM (a) (Q-DECNRCM, "What stays open"). Only its doc
     changes here, and the new doc says it pins (a).
   - **Red first, all at the data level, none through `Term`:**
     - Swedish emits 10 entries.
     - Every set's count matches its source.
     - Deleting the skip entry fails the generator: the perturbation.
     - `nrcs_table(Nrcs::Swedish)` contains `(0x40, 0x00C9)`, and
       `nrcs_table(Nrcs::Turkish)` contains `(0x21, 0x0131)`.

     None of these renders through `Term`, so none builds in an answer to
     Q-DECNRCM (item 2, "What stays open").
2. **An independent conformance test against DEC's VT220 tables, at the
   data level.**
   - It extracts Tables 2-5 to 2-15 from `vt220-rm-chapter2.html`
     ([[source: dec-special-graphics]], committed) at test time: each `chname`
     placed by x/y, and the shaded `<rect fill="#ddd">` cells.
   - It shares no code with the generator.
   - It keeps both of §1's controls in the test:
     - Table 2-1 gives 0 differences;
     - in each table, the shaded count equals the differing count.

     So the extraction is itself checked.
   - **It compares tables. It never renders through `Term`.** Each comparison
     runs over 2/1-7/14, and a byte a table does not list is itself, as
     `with_nrcs` reads it (`cell.rs:466-476`). It has two halves:
     - **Content, by set.** DEC's extracted table for each VT220 set against
       baud's table for that set: `nrcs_table(n)` for a `Charset::National(n)`,
       and `uk_table()` for `Charset::Uk`. `nrcs_table` takes an `Nrcs`, not a
       final, so this half names the set and sends no byte. UK is included,
       which `every_national_replacement_renders_as_its_table_says` never
       drives (§2.3).
     - **Finals, by byte.** `Charset::from_final(f)`, for each final VT220
       Table 4-4 ([[source: vt220-rm-ch4]], committed) lists under "National
       replacement character sets", names the set whose table the content
       half checked: `Charset::Uk`, checked through `uk_table()`, for `A`, and
       a `Charset::National(n)`, checked through `nrcs_table(n)`, for `4`,
       `C`, `5`, `R`, `Q`, `K`, `Y`, `Z`, `H`, `7` and `=`
       (`baud-domain/src/vt/charset.rs:264-285`).
     - **The rule for both halves:** a table or a final is asserted only where
       no open question touches which set it is. `Charset::from_final` only
       names the set a final designates: it takes the byte and nothing else,
       no mode and no level (`charset.rs:264-285`), and `esc_final` writes
       what it returns into the slot (`baud-core/src/write.rs:1710-1718`).
       Every gate an open question could add, national mode (Q-DECNRCM) or
       level (§2.8), is built after `from_final`, in `Term`, and never into
       `from_final`. So the finals half asserts names only, and a question
       about whether a designation takes effect does not touch it.
   - **Left unasserted until their questions are answered:**
     - Norwegian/Danish, in both halves: Table 2-12's content and the finals
       `E` and `6` wait on Q-ND-backtick, which asks which set they name.
       Asserting that the table `E` and `6` reach is Table 2-12's Ä Ü ä ü
       would build in options (a) and (b) before the owner answers, since
       under (c) they designate the ISO repertoire. `` ` `` is not a VT220
       final, so the test never sends it;
     - `f`, which no DEC table lists, so the test never sends it. It waits on
       Q-French-f and Q-Unlisted.
   - **`4` and `A` are asserted, names and content alike.** Whether baud gates
     SCS finals by level (§2.8) bears on whether they take effect: `4` is
     "discontinued in L3" (STD 070 p. 5-117), and UK is "1x" on p. 5-115,
     with note 3's exclusion of ASCII and UK open under Q-DECNRCM (b.4). Each
     of those would be a gate after `from_final`, so neither changes the set
     `4` or `A` names, and no open question changes what Dutch or UK is.
   - **Recorded exceptions for Dutch's content:**
     - Dutch 7/13: the transcription has `£`; DEC has ¼ (Pocket Guide p. 8
       **[img]**; STD 070 p. 6-109 **[img]**). A transcription error.
     - Dutch 7/14: the transcription has `¼`; DEC has ´ (Pocket Guide p. 8
       **[img]**; EK-PPLV2-PM Table A-2 and Figure A-17). A transcription
       error.
     - Dutch 7/12: the VT220 table, printed and transcribed alike, draws `f`;
       the test expects ƒ, from STD 070 p. 7-20, where the florin is a Dutch
       character of older devices with fallback presentation `f` **[img]**,
       and EK-PPLV2-PM Figure A-17 with its NOTE ("7/12 Florin Sign (ƒ)",
       approximated as "Lowercase f (f)") **[img]**. Not a transcription
       error: the VT220's `f` is read as the florin's fallback, resolved
       (**inferred**, §2.5).
   - **Red today** at Swedish 4/0.
   - **What it builds in: nothing about national mode or level.** It holds
     whether Q-DECNRCM is answered (a) or (b), and whether or not SCS finals
     are gated by level. It never designates anything, and its finals half
     asserts only the set `from_final` names, with every gate after it. It
     needs no keyboard dialect, (b.3)'s binding.
   - **What stays open.** Translation through `Term`.
     - Three existing tests drive NRC designations through `Term` with no
       `?42`, so they already pin Q-DECNRCM (a) and change under (b):
       `the_national_sets_substitute` (`charsets.rs:537`),
       `a_promoted_run_keeps_the_set_it_was_written_under` (`:579`) and
       `every_national_replacement_renders_as_its_table_says` (`:687`)
       (Q-DECNRCM, "What stays open").
     - No new test renders an NRC set through `Term` until Q-DECNRCM is
       answered. (b.3), one set at a time and the keyboard dialect, and
       (b.5), whether the sets act under UTF-8, decide how such a test is
       written (§5(b)).
   - **What it cannot reach**, and what would cover each:
     - Portuguese: VT420 Table 2-1's text layer, or EK-PPLV2-PM Figure A-19;
     - Hebrew: Figure A-21;
     - Turkish: Figure A-26;
     - Greek: nothing.

     The VT420 RM and EK-PPLV2-PM are `external`, so no test can read them.
     A test can reach those three sets only through cells typed into it with
     their citation, read from a hash-checked copy.
3. **`Charset::to_u8`/`from_u8`: dead code, and its docs corrected** (§3.8).
   - Delete `Charset::to_u8` and `Charset::from_u8`, and `Nrcs::from_u8`
     (`charset.rs:135`), whose only caller is `Charset::from_u8` (`:235`).
     Nothing else calls any of the three. They are functions, so deleting
     them changes no behaviour and no layout.
   - Keep `Nrcs::to_u8` (`:116`): the test at `charsets.rs:709` calls it.
   - `Run::charset` (`store.rs:89-95`) is the one thing whose removal changes
     a layout: it is a field of a zerocopy struct that crosses the mmap
     boundary, carved from padding the struct already had. Leave the field
     and mark *it* dead, in its doc. That is the decision-free minimum, and
     "mark dead" here means the field, not the functions. Removing the field
     is a separate change.
   - Correct the seven stale sites listed in §3.8: `charset.rs:209-215`,
     `:198-200` and `:115`; `store.rs:89-95`; `docs/vt-coverage.md:251`'s
     "the run records which set it was written under so promotion cannot
     change it"; and `baud-core/tests/vt100/charsets.rs:572-577` and
     `:587-593`.
   - Re-document and rename `a_promoted_run_keeps_the_set_it_was_written_under`
     for what it checks, that written cells keep their glyph when the set is
     designated again, and rewrite its `MAXRUN`/`OpMode::Runs` comments to
     say why it writes what it writes. Do not delete it.
     - The new doc says the test pins Q-DECNRCM (a): it designates German
       with no `?42`, so it changes under (b) (Q-DECNRCM, "What stays open").
       The new name must not read as independent of that answer.
     - Do not make it independent by sending `?42` first. `?42` is not
       recognised today (§2.10), and under (b), (b.3) can still change what
       `ESC ( K` does.
   - No round-trip test: it would test code that nothing runs.
4. **The counts, corrected as each fix lands.** Today the figures are 162 (the
   fourteen sets) and 163 (with UK).
   - After Swedish 4/0: 163 and 164.
   - After Turkish 2/1: 164 and 165.
   - Greek's +2 waits on Q-UNDEF.
   - All four places take the fourteen-set figure (162, then 163, then 164):
     `docs/vt-coverage.md:251`, the comment above `[sources.xterm-charsets]`
     in `MANIFEST.toml`, `baud-core/tests/vt100/charsets.rs:533` (in the doc
     of `the_national_sets_substitute`, a test that pins Q-DECNRCM (a); see
     (a)1), and `cell.rs:457`. The last says the test drives "all 163
     replacements", but the test never drives UK (§2.3), so its figure is 162
     today, not 163, and it must not become 164 after Swedish lands.
5. **Manifest titles widened** to what is cited. Done beside this note
   (header), and committed with it:
   - `dec-special-graphics`: chapter 2 entire, not only Table 2-4;
   - `foot-vt-ris`: `vt.c` entire, its SCS arm (lines 545-571) among it;
   - `xterm-reset`: `charproc.c` entire, `xtermDecodeSCS` and its DECNRCM
     gate among it. CASE_SUB and `modified_DECNRCM`, also cited here, are in
     the same file.

   **Not yet done**, and listed in the header: eight more titles (`vt420-rm`,
   `xterm-vtprstbl-c`, `unicode-data`, `xterm-charsets`, `dec-pplv2-pm`,
   `unicode18-ch22`, `xterm-ptyx-h`, `xterm-xterm-h`), and the Dutch 7/12
   wording in the comment inside the `[sources.dec-special-graphics]` entry.
6. **Doc corrections that decide nothing.**
   - **D4.** `docs/vt-coverage.md:760` contradicts `:251` and `:557`. Rewrite
     it to state the departure from DEC's national-mode gate and point to
     Q-DECNRCM, without choosing.
   - **D5.** The SUB row (`docs/vt-coverage.md:190`, its text at `:192-198`)
     says xterm "does not write one" on SUB. A ground SUB reaches xterm 411's
     CASE_SUB through the ground table ([[source: xterm-vtprstbl-c]]
     `VTPrsTbl.c:188`; [[source: xterm-reset]] `charproc.c:3646-3678`). What
     it writes depends on three things:
     - **The terminal ID.** CASE_SUB writes only when `terminal_id >= 100`
       (`:3665`), so decTerminalID 52 writes nothing. It picks 0x2426 when
       `terminal_id > 200` and 0x2592 otherwise (`:3668`), so the VT100-level
       IDs xterm keeps (100, 101, 102, 131, 132; `:11020-11033`) write
       U+2592. The default ID is 420 ([[source: xterm-ptyx-h]]
       `ptyx.h:398-400`, the resource default at `charproc.c:516`).
     - **The build.** That choice is compiled only under `#if OPT_WIDE_CHARS`
       (`:3667`). Without it the effect is code 2 (`:3670`). `xterm.h`
       includes `xtermcfg.h`, which is not pinned, when `HAVE_CONFIG_H` is
       defined, and then defines `OPT_WIDE_CHARS` as 0 if nothing has set it
       ([[source: xterm-xterm-h]] `xterm.h:43-45`, `:298-300`). So the
       pinned files do not say whether a given build has it.
     - **The locale.** `dotext` sends the character through `xtermCharSetOut`
       unless GL and GR both hold ASCII, while `vt100Graphics` is set, its
       default (`charproc.c:7015-7025`, `:792`).
       - A UTF-8 locale selects UTF-8 mode at xterm's defaults (`utf8`
         "default" and `locale` "medium", `:795`, `:814`; `:10129-10133` in
         a build with luit support, `:10162-10164` in one without), and
         UTF-8 mode sets `wide_chars` (`:11688-11690`). `resetCharsets` then
         puts ASCII in G2 (`:1259-1261`, `:1273`), which is GR (`:1278`),
         and G0, ASCII, is in GL (`:1271`, `:1277`). No translation runs, and
         `dotext` goes on with U+2426. With another set in GL or GR the
         translation runs and still leaves it: in UTF-8 mode
         `xtermCharSetOut` skips any character above 255
         ([[source: xterm-charsets-c]] `charsets.c:384-393`).
       - Outside UTF-8 mode, at ID 420 (`ansi_level` 3;
         `charproc.c:1324-1328`, `:11051`), `resetCharsets` puts the
         preferred supplemental set in GR (`:1243-1245`, `:1273`, `:1278`):
         ISO Latin-1 Supplemental, by `preferLatin1`'s default of True
         (`:460`; `ptyx.h:1138-1140`). So the character goes through
         `xtermCharSetOut`, which takes its low eight bits
         (`charsets.c:366-370`; `CharOf` casts to `Char`, an
         `unsigned char`, `ptyx.h:305`, `:331`): 0x2426 becomes 0x26. With
         ASCII in GL the switch leaves that byte (`charsets.c:423-424`) and
         stores it (`:583-585`), so `dotext` goes on with `&`, not U+2426.
     - The two other `case CASE_SUB:` labels in `charproc.c` do not divert a
       ground SUB. One (`:3504-3505`) acts only when a CSI parameter list has
       subparameters. The other (`:3571-3572`) acts only while xterm's status
       line is shown and active.
     - **What the correction says.** The conditions, not "by default". xterm
       411 writes:
       - nothing at decTerminalID 52, in any build;
       - code 2 at the other IDs, in a build without `OPT_WIDE_CHARS` (below
         32, so `xtermCharSetOut` passes it unchanged, `charsets.c:395-396`);
       - in a build with `OPT_WIDE_CHARS`: U+2426 at decTerminalID 420 in a
         UTF-8 locale, with ASCII in GL and GR; `&` outside UTF-8 mode with
         ASCII in GL; and U+2592 at the VT100-level IDs.
     - The row's decision stays the owner's (Q-UNDEF). Its stated basis,
       "baud follows xterm", is wrong as written: under the conditions above
       xterm writes a glyph.
     - Read from source, not run. Each character above is what `dotext`
       goes on to write after the translation. The write path past that
       point, and xterm's drawing, were not read.
   - **D6.** `baud-domain/src/vt/report.rs:201-204` says baud "has neither
     UDKs nor NRCS nor 132-column mode". It translates fourteen NRC sets.
   - **D7.**
     - `baud-domain/src/vt/charset.rs:184-190`: name `& 5` as the pinned DEC
       texts do, "Russian NRCS" (vt510-scs; VT520 Table 5-14), and say that
       "DEC Russian" is ctlseqs' name (line 368).
     - Keep the ctlseqs quote, and add that xterm 411 translates both sets by
       its own choices (§2.9). The reason that stands is that no DEC chart has
       been read.
     - Its "Recognised as designations, which is all baud does with any set"
       (`:187-188`) has been stale since the NRC tables landed.
     - `docs/vt-coverage.md:560` has neither defect. It does not name `& 5`,
       and it gives no stale sentence: it calls `% 3` and `& 5` "the two whose
       mappings ctlseqs says are undocumented", which is the ctlseqs reason
       kept above. It needs no correction.

**A core-path note for whatever lands from (b).**

- `with_charset` (`cell.rs:389-398`) is on the per-character path while a set
  is designated.
- A new arm, or a direct-indexed table in place of `with_nrcs`'s linear search
  (`cell.rs:442-476`), is a core change. CLAUDE.md's rules require the
  `ingest2` A/B with `cells_written` equal on every arm.
- `cell.rs:463` says "no recording designates a national set". Whether any
  recording designates one of the DEC sets in §3 is unchecked. Either way a
  payload has to be written first, and the number quoted against that payload
  only.
- Generated, direct-indexed rows are the shape the table rule asks for.

### (b) After the answers

- **Q-UNDEF:** Greek 7/9 and 7/10 leave the skip list, and every DEC set's
  reserved cells get their codepoint. This also settles note AE Q8's codepoint
  row and the SUB row.
- **Q-Greek:** keep, swap, or leave untranslated.
- **Q-ND-backtick:** the ISO Norwegian/Danish table, if (b) or (c), from
  VT420 Table 2-1 with Figure A-16. Both are `external`, so it is a table
  typed with its citation, not generated from them. The conformance test then
  asserts Norwegian/Danish's content and the finals `E` and `6` (and, for a
  test of finals beyond the VT220's, `` ` ``), each against the repertoire the
  answer gives it.
- **Q-DECNRCM:** `?42`, DECRQM's answer for it, the gate and its sub-points.
  Then the first test that renders an NRC set through `Term`, written to the
  answer: under (b) it sets `?42`, and (b.3) and (b.5) decide whether it
  carries one set per terminal or a keyboard dialect and whether it runs
  under UTF-8. The three tests that pin (a) change with it:
  `the_national_sets_substitute`,
  `a_promoted_run_keeps_the_set_it_was_written_under` (under the name (a)3
  gives it) and `every_national_replacement_renders_as_its_table_says`.
  `docs/vt-coverage.md:760` moves to a done or partial row.
- **Q-Unlisted:** under (b), an unlisted final leaves its slot as it was and
  is still counted in `charset_unsupported`; the two tests that pin `Other`
  are rewritten, and `Snapshot` in `spec_inventory.rs` gains
  `charset_unsupported`, or
  `every_spec_esc_intermediate_form_is_handled_or_counted` reports 24 forms
  as silent (Q-Unlisted).
- **Q-French-f:** under (b2), `f` is ignored and counted, through Q-Unlisted
  (b)'s path or a case of its own, with the same `Snapshot` change;
  `every_listed_single_byte_final_is_named` loses its `f` row, and the stale
  `f` goes from `charset.rs:92` and `docs/vt-coverage.md:557`. Or `f` stays
  French with its departure recorded.
- **Q-SCS-Russian:** a table, or the departure recorded on the row.
- **Q-Order, Q-Technical, Q-JIS-Roman:** the DEC sets, in the order answered.
  - DEC Supplemental's generator reads Table 2-3, which is committed, and
    checks its mask against Figure A-2's, carried as a constant with its
    citation (EK-PPLV2-PM is `external`).
- **Level gating of SCS finals:** whether the sets are gated by the level
  baud claims, by STD 070 §5.8.1's levels (§2.8): Dutch `4` ("discontinued in
  L3"), JIS-Roman and JIS-Katakana ("2x, 3x, 4x"), DEC Technical ("3x, 4x"),
  DEC Supplemental (required at "2-4", not listed for Level 1), UK ("1x"),
  and the VT500 sets STD 070 does not list. Note AE's Q9 asks only about the
  shifts. This rests on the same level reading, but it has to be added to Q9
  or asked on its own. Any such gate is built after `from_final`, in `Term`
  (§5(a)2's rule), so the conformance test's finals half, which already
  asserts `4` and `A`, does not change. A test of the gate goes through
  `Term` and is written to the answer.

---

## 6. Unread or unverified

**Unverified: read by one pass and not re-read by the check.**

- VT382 Figure 2-8, **partly read**. The cells §3.2 names (3/8-3/11, 3/13,
  5/2, 5/4, 5/5, 6/13, 7/5, 7/6 and 7/11-7/14) were read on page renders of
  PDF p. 36 **[img]**; the rest have not been read one by one. The PDF is a
  scan. (Its §2.4.4-2.4.6 quotes are checked on the page images, §1.)
- VT382 Figure 2-10 outside columns 14 and 15.
- [[source: vt320-uu-appe]] agreeing with Table 2-3 at 94/94.
- Table 2-4's "drawn paths" control.
- EK-PPLV2-PM text-layer cell counts: 10, 68 and 74 for the three supplemental
  sets, and the 31 Greek letters of DEC Technical.
- STD 070 p. 8-85, 070-2 p. 2-17, 070-3 §3.6.2, and §7.5.5's list order
  outside 2/11-2/14, 3/13 and p. 7-27's last rows ("small tau" to "down
  arrow"): OCR only, not checked on the page image.
- The glyph identities of DEC Hebrew, Greek and Turkish Supplemental, cell by
  cell.
- EK-PPLV2-PM Figures A-6 to A-19 as an independent check of every NRC set.
  Only A-15 (Swedish) and A-17 (Dutch) were used.
- A second EK-PPLV2-PM variant, 733,833 bytes, on computer-refuge and vaxhaven.
  It has no hash.

**Unread.**

- DEC STD 169 (not online) and EK-PPLV3-PM (not online).
- ELOT 927, and Kermit 95's ELOT927 table (Q-Greek).
- The KOI-7 standard and ISO-IR 141 (Q-SCS-Russian).
- The ISO-IR register entry for 94-set final 6/6 (Q-French-f).
- NS 4551-1 and DS 2089, the standards STD 070 names for its one
  Norwegian/Danish set (Q-ND-backtick). That NS 4551-1 is the ISO repertoire
  of VT420 Table 2-1 is an inference (§2.6).
- The chart pages of [[source: iso-ir-013]] and [[source: iso-ir-014]]. Their
  registration pages are read: ISO-IR 014's for this note **[img]** (§3.3),
  ISO-IR 013's per its manifest entry.
- The VT330/VT340 Programmer Pocket Guide, EK-VT3XX-HR-002 (manifest key
  `vt3xx-hr`). Only its cover and edition page (PDF pp. 1 and 4) were read,
  for its manifest entry. No page of it was read for a claim, so it is not
  cited.
- The LN03 Reference Manual's Appendix A: not located in
  `ek-0ln03-rm-002.pdf`. bitsavers' copy (65 MB) and EK-VT3XX-TP (35 and 52
  MB) were over the 20 MB fetch limit.
- ~~Any Unicode text showing or describing U+2426's reference glyph.~~ Read
  after this note was written: [[source: unicode18-chart-2400]]'s names list
  and reference glyph (Q-UNDEF). U+FFFD's glyph is read, from the inline SVG in
  [[source: unicode18-ch23]] §23.8.4.

**Not examined.**

- What a DEC terminal does with an NRC designation outside national mode
  (Q-DECNRCM (b.1)). Whether `Charset::Other` conforms for an unlisted final
  is now examined (§2.8, Q-Unlisted): it DEPARTS.

**Not pinned.**

These were looked at and have no key. **Nothing in this note rests on them,
and they are not evidence for any claim here.**

- VT510 chapter 8 (vt100.net), in an untracked copy beside the research
  files. Earlier revisions of this note cited names from it; those claims are
  removed (§2.7, §2.9, D7).
- ISO-IR 88, the register entry an earlier revision used as a hint about
  Greek order; that use is removed (§2.7).

**For the owner, not a defect in this note.** The VT220 Pocket Guide's edition
page (PDF p. 3) reads "The reproduction of this material, in part or whole, is
strictly prohibited." **[img]** For that sentence, the manifest now records the
Pocket Guide, EK-PPLV2-PM and the VT420 RM as `external`: pinned by hash,
fetched to read, and not redistributed (header). None of the three is in the
tree, and none was ever committed. `vt3xx-hr` was already `external`, for its
size and for the same sentence on its own edition page (PDF p. 4) **[img]**,
which was read for its manifest entry and for no claim. Only those two
edition pages were read for the sentence. This note does not say that
EK-PPLV2-PM or the VT420 RM carries it. That choice is the owner's to
revisit.

**Practice traced, never run.** Every xterm and foot behaviour here is read
from source at the pinned commits: xterm patch 411 at `9489b20`, and foot
1.21.0.
