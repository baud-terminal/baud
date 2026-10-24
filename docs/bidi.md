# Right-to-left text: what is built, and what drawing it needs

Hebrew and Arabic are still drawn left to right, in the order they were
typed. Nothing on screen has changed. What is new is underneath: baud now
works out, for any line of text, the order a right-to-left reader expects,
exactly as the Unicode Standard says it should. This is the first of two
steps. The second puts that order on the screen, and this page lists what
it needs.

## What is built

**The Unicode Bidirectional Algorithm, UAX #9 revision 51, for Unicode
17.0.0** (`[[source: uax9-bidi]]`), in `baud-domain`
(`baud-domain/src/bidi.rs` and `baud-domain/src/bidi/algorithm.rs`). It
covers every rule from the paragraph level to the order of a line: P1-P3,
X1-X10, W1-W7, N0-N2, I1-I2 and L1-L2.

- **It passes both of Unicode's conformance files, every line.** Those are
  `BidiTest.txt`, 490,846 cases run 770,241 times across their paragraph
  directions, and `BidiCharacterTest.txt`, 91,707 cases with brackets
  nested past the 63-deep stack (`[[source: unicode-biditest]]`,
  `[[source: unicode-bidicharactertest]]`). The tests are
  `every_bidi_test_case_resolves_and_reorders_as_it_says` and
  `every_bidi_character_test_case_resolves_and_reorders_as_it_says`.
- **The character properties are compiled tables.**
  `scripts/gen-bidi-tables.py` builds Bidi_Class, Bidi_Mirrored,
  Bidi_Mirroring_Glyph and the paired brackets from the pinned UCD 17.0.0
  files into two tries, 49 KB in all. Each property is one indexed load, and
  `bidi_tables_match_the_pinned_data` keeps the committed tables equal to
  the files. baud-font's `script_data.rs` holds a second paired-bracket
  trie (`BRACKET_INDEX`), built from the same `BidiBrackets.txt` by
  `scripts/gen-script-tables.py`. Merging the two is a cleanup for step 2.
- **No allocation, and bounded.** A `Bidi<N>` holds a text of up to `N`
  characters and every array the rules need, about 18 bytes a character. The
  two stacks the annex sizes are fixed: 127 entries for the directional
  status and 63 for brackets. A longer text is refused whole
  (`BidiTooLong`), never cut. A `Bidi<2048>` holds a row at the grid's
  widest.
- **No cost to reading output.** Nothing on the path that reads a
  program's bytes calls it.

The API comes in two halves, because UAX #9 resolves levels over a whole
paragraph and orders each line separately. `Bidi::resolve` (or
`Bidi::resolve_classes`) resolves a paragraph's levels. `Bidi::reorder`
then gives one line's visual order and its levels after L1. Both files hold
each text to one paragraph on one line. `a_line_ends_its_trailing_whitespace_at_the_paragraph_level`
pins the case of one paragraph across lines, which the files leave out.

## What step 2 needs

Step 2 draws a line in that order, and moves the cursor and the selection
with it. Every choice below is still open. Each is to be decided with Jev,
reading its standard at source, under the owner's standing rule: "full
VT420-class terminal. we need to support such protocols fully, always!"
(2026-09-25). The questions are numbered as in note AB
(`docs/research/notes/ab-right-to-left.md`, section 8).

### What the algorithm is given

- **The paragraph.** A paragraph is the rows joined by soft wraps, up to a
  hard newline. `RowExtent`'s wrap bit already marks the difference.
  - Such a paragraph can be longer than a row, so the renderer needs a
    `Bidi` sized to the longest paragraph it resolves. Otherwise it needs a
    stated rule for longer ones. Size it from baud-core's `MAX_COLS` where
    that constant is visible, or from the longest paragraph the renderer
    resolves, and never from a bare 2048, so the two bounds cannot drift
    apart.
  - terminal-wg's cap of about 500 lines falls back to no reordering past
    it, and no higher-level protocol in UAX #9 permits that (note AB 2.4).
    Resolving a truncated segment under HL4 or HL5 is the conforming
    alternative.
  - The paragraph's start can lie in scrollback (Q9).
- **The unit.** A cell holds a base character and up to eight attached
  ones. Note AB 7.1 proposes giving the algorithm the base's class for
  each cell and moving whole cells. That keeps each mark with its base,
  which is what L3 asks of a renderer that draws marks after their base,
  and note AB reads it as the effect of FriBidi's REORDER_NSM flag. A wide
  character's lead cell and its tail must stay together.
- **Erased cells.** A cell nothing was written to has no character, and
  which class it takes is open (Q8).
- **The bidi controls.** LRM, RLM, ALM, the embeddings and the isolates
  are zero-width, so `attach` stores each with the character before it.
  Note AB 7.1 lists the places where one is dropped instead. terminal-wg's
  level 2 uses these controls and its level 1 discards them (Q4).

### Drawing

- **The order.** Each row needs a visual-to-logical map on all three
  render paths: the cell shader, the instances and the CPU. The pixel
  oracle must hold the three equal on a right-to-left scene (Q10).
  - `convert_row` lays cells out at their logical column today.
  - `baud-present-macos/src/shaders.metal` reads a cell at its own column.
- **Mirroring (L4).** A character at an odd level that is Bidi_Mirrored
  draws its mirrored glyph. That glyph is Bidi_Mirroring_Glyph where the UCD
  names one; where it names none, as for U+2211, the font supplies it. The
  glyph key needs a mirrored bit (Q11).
  - Box drawing is never Bidi_Mirrored. HL6 permits mirroring it, which is
    terminal-wg's `?2500` (Q16).
- **Shaping.** A level run is shaped right to left. Arabic joining is a
  separate decision, D15 in note J (Q12, Q13).

### Following the order

- **The cursor.** Its shape and side on a right-to-left character, and
  whether it moves logically with arrow swapping or visually (Q14).
- **Selection.** It stays a logical stream, and its highlight becomes
  visually discontiguous on a mixed row (Q15).
- **The mouse.** `cell_under` needs the inverse map from a visual cell to
  the logical one.

### The protocols

There is no coverage row for any of these yet. Each becomes a row when it
is built, and is never recorded unless baud acts on it.

- **ECMA-48 and terminal-wg.**
  - BDSM, mode 8: explicit or implicit. What RIS and DECSTR do to it, and
    what DECRQM answers, are Q5.
  - SCP sets a paragraph's direction.
  - SRS, SDS, SPD, SIMD and SAPV are Q19.
  - terminal-wg's `?2501` (autodetect) and `?1243` (arrow swapping) are
    Q3. XTSAVE and XTRESTORE are already built.
- **DEC and xterm.** DECRLM, DECRLCM and DECHEM (Q19). xterm's ctlseqs
  lists no bidirectional control, so `spec_inventory.rs` cannot require
  any of these (note AB 7.1).
- **The defaults.** Whether to reorder by default, and whether to on the
  alternate screen, are Q2 and Q17. Agent CLIs that order their own text
  were never tested.
- **Discovery.** How applications discover the support is Q18.

### Cost

Resolving a row is per frame, not per byte. Before step 2 lands, the
frame bench's CPU column is measured on both arms. UAX #9 section 5.1
names the obvious fast path: "first test for right-to-left characters and
invoke the Bidirectional Algorithm only if they are present". baud's own
reading adds one condition: the row must also have no RLO, since an
override turns Latin into right-to-left text.

- **Two rules scan more than once.** N0 scans every bracket pair's
  contents, and the text before it for context, so it is quadratic in the
  worst case. X5c scans ahead from every FSI to its matching PDI. Section
  5.1 names bracket matching as a place to optimise. Step 2 measures both
  on a row of nested brackets before it resolves rows per frame.

## Decided here

**Unicode version (Q20): 17.0.0**, the version of every other UCD table
baud compiles. Jev chose it over pinning only the bidi tables at 18.0.0, or
moving every table at once, with confidence 0.95 and 0.98 in the two orders
it was asked. Unicode 18.0.0 adds two brackets and five mirroring pairs.
The data in its two test files is the same, so a later move of every table
to 18.0.0 carries the bidi tables with it.
