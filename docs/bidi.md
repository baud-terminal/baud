# Right-to-left text: what is built, and what is next

**Hebrew and Arabic lines are drawn right to left from the moment baud
starts** (ECMA-48's bidirectional support mode, BDSM, is on: its IMPLICIT
state). A line that starts in Hebrew or Arabic runs right to left against
the window's right edge, its closing period at its left end, after its last
word. A table drawn with box-drawing lines keeps its columns, and a Hebrew
name in one of its cells reads right to left against the cell's right side.
A Hebrew word in an English line reads right to left in its place, brackets
around Hebrew face the right way, vowel points stay on their letters,
Arabic letters join into words, and the cursor, the selection, the find
marks, the link underline and the mouse all follow what is drawn. A copy
gives the text in the order the program wrote it. A program that lays out
its own right-to-left text turns the mode off with `CSI 8 l`, and every
line is then drawn in the order its cells hold; `CSI 8 h` turns it back on,
and a reset (`RIS`) returns it to on. On a right-to-left line the Left and
Right arrow keys swap, so they move the cursor the way the line reads. This
is step 3 of right-to-left text.
Step 1 built the Unicode Bidirectional Algorithm; step 2 put its order on
the screen; step 3 joins Arabic letters.

## What you see

- **A line is drawn in the order UAX #9 gives it**, revision 51 for
  Unicode 17.0.0 (`[[source: uax9-bidi]]`), on all three draw paths: the
  Metal cell shader the app draws with, the Metal instances and the CPU
  compositor. `right_to_left_rows_draw_in_their_visual_order_on_every_arm`
  holds the three to the same pixels, and to the pixels of the same text
  written already in visual order with the mode off.
- **A paragraph is the rows a soft wrap joins.** Its levels are resolved
  over all of them, rows above the window and below it included, so an
  override or an embedding on one row reaches the next. A paragraph longer
  than 18,432 codepoints (`baud_render::CONTEXT`, a row at the widest grid
  with full clusters) is resolved over the rows drawn and as many whole rows
  around them as fit.
- **A paragraph takes its direction from its first strong letter** (UAX
  #9 P2-P3, isolates skipped): one that starts in Hebrew or Arabic is at
  level 1 and runs right to left, its erased cells laid out before its
  text, so it sits against the right edge; one that starts in a Latin
  letter, or holds no strong letter, is at level 0 and stays at the left
  edge. The letter is looked for in the text the layout reads, the
  paragraph's drawn rows and the context around them; a paragraph resolved
  in more than one piece keeps the first piece's level. This is every
  paragraph's until a program sets its lines' direction.
- **A program can set which way its lines run.** SCP 2 (`CSI 2 SP k`)
  sets the line the cursor is on, and the new lines after it, right to
  left; SCP 1 left to right; SPD 3 and SPD 0 (`CSI 3 SP S`, `CSI SP S`)
  turn every line of the screen at once (`[[source: ecma48]]` 8.3.111,
  8.3.130). **While direction detection is on, as it is at power-on, the
  set direction is only the fallback** of a line with no strong letter: an
  English line stays left to right under SCP 2, a line of digits or an
  empty one runs right to left (`?2501`, below). With detection off
  (`CSI ? 2501 l`) the set direction is the paragraph's level outright (UAX
  #9 HL1): SCP 2 draws an English line against the right edge and SCP 1
  keeps a Hebrew line left to right. It turns the paragraph
  holding the cursor's line at once and becomes the direction new lines
  take: a line feed into a new line, a line a scroll brings in, the lines
  an erase of the screen clears, the lines SU, SD, IL and DL bring in,
  each a line of its own, and the screen a clear leaves; a wrapped row keeps its paragraph's, and an
  erase inside a line keeps the line's. A cursor move takes nothing, so a
  line a program only moves to keeps its own. Each row keeps its
  paragraph's direction beside its mark, so scrolling, scrollback and a
  change of width carry it. On a table row each cell takes it. `DECSTR`
  returns new lines to first strong and leaves the lines on screen as they
  are; `RIS` returns every line to first strong. With BDSM reset a line
  set right to left is drawn in reverse, its first cell at the right edge,
  its brackets mirrored and a bar cursor on its cell's right side,
  terminal-wg's explicit RTL (`[[source: terminal-wg-bidi-basic-modes]]`).
  `CSI 0 SP k` and `CSI SP k` are refused and counted: ECMA-48 defines
  only 1 and 2 and gives SCP no default (`docs/vt-coverage.md`).
- **A program can turn direction detection off and on**, terminal-wg's
  `?2501` (`[[source: terminal-wg-bidi-basic-modes]]`). On, as at
  power-on, a paragraph runs the way its first strong letter gives it and
  falls back to the direction SCP or SPD set; off, it runs the set
  direction, left to right where none was set. Like the direction, it is
  kept on every row of a paragraph and carried by the same rules, but it
  turns a paragraph at once only when the cursor stands at that
  paragraph's start ("if the cursor stands at the first logical position
  of a paragraph then the received BiDi property value is immediately
  applied on that paragraph", `[[source:
  terminal-wg-bidi-escape-sequences]]`); otherwise the next new line takes
  it. SPD spreads only the direction. With BDSM reset it is ignored.
- **A box drawn right to left closes**, terminal-wg's `?2500` (`[[source:
  terminal-wg-bidi-misc]]`): while it is set on a paragraph, a Box Drawing
  character (U+2500-U+257F) drawn right to left is drawn as its left-right
  counterpart, `┌` as `┐`, so an ncurses box on a line BDSM reset draws in
  reverse keeps its corners in. The counterpart is the character whose
  Unicode name says the same with left and right exchanged. A table row's
  rules, which stay where they were written, are drawn as written. Off at
  power-on; carried and applied as `?2501` is.
- **Left and Right swap in a right-to-left paragraph**, terminal-wg's
  `?1243` (`[[source: terminal-wg-bidi-arrow-key-swapping]]`), on at
  power-on: a Left or Right key pressed with the cursor in a paragraph
  drawn right to left sends the other key's sequence, the keypad's too, so
  a shell's or editor's cursor moves the way the line reads. The direction
  is the one drawn: detected, the fallback, or set; on a table row, the
  cell the cursor is in, at the moment the key is pressed: a held key's
  repeats and its release name the key its press named. Home and End
  never swap, nor do the keys the wheel or a click on the prompt line
  send. `CSI ? 1243 l` turns it off.
- **Each of the three answers `DECRQM`, is kept by XTSAVE and carried by
  DECTSR**; `DECSTR` returns the values in force to their power-on states
  and leaves the lines', and `RIS` returns every line.
- **A table row keeps its columns.** A paragraph holding a box-drawing
  character (U+2500-U+257F: `│`, `┃`, `║`, the dashed verticals, the
  junctions, and DEC Special Graphics line drawing, which is stored as
  them) is a table row: it runs left to right, every rule stays where it
  was written, and each cell between two rules is a paragraph of its own
  at the level its own first strong letter gives it (UAX #9 section 3:
  "the text in two different cells of a table will be in different
  paragraphs"; HL4), ordered inside its own columns. A Hebrew cell reads
  right to left against its cell's right side, its padding at its left
  (L1), as a right-to-left line sits against the window's right edge; a
  cell of digits or Latin text stays as it is. A tree's branch (`└──`)
  divides its row the same way. ASCII `|` is text, so a Hebrew sentence
  with a `|` in it reads as before, and a psql-style table drawn with `|`
  is laid out as a line. A tab stores no character in the grid, only the
  erased cells it skips, so it is no rule.
- **The bidi controls work.** LRM, RLM, ALM, the embeddings, overrides and
  isolates that baud stores on the cell before them are given to the
  algorithm with that cell's character. One at the start of a line, with no
  cell before it to be stored on, is still dropped and counted.
- **Brackets, sums, roots and every other mirrored character face the
  right way** in right-to-left text (L4). A bracket draws its
  Bidi_Mirroring_Glyph, a bracket carrying only bidi controls (LRM, RLM,
  ALM, the embeddings, overrides and isolates) included. A mirrored
  character carrying a mark keeps it, its mirror under it: `<` followed
  by U+0338 draws as `>` followed by it, as the composed U+226E draws
  U+226F. The 126 mirrored characters with no such glyph (N-ary sums,
  roots, integrals and the like), which BidiMirroring.txt leaves "up to
  the rendering system to provide mirrored glyphs", draw their own glyph
  flipped left to right in their cell; a mark one carries is drawn as it
  is, where the flip puts it (Jev BASE_FLIPPED, 0.91 / 0.94). The three
  draw paths match pixel for pixel, a flipped glyph the left-to-right
  one mirrored (`mirrored_characters_draw_mirrored_on_every_arm`).
- **A cell moves whole**: a letter with its points, a wide character with
  its right half, kept on its left.
- **An erased cell between characters** moves with the character before it;
  erased cells after a line's text stay at the line's end.
- **No ligature or contextual form crosses from one direction run to
  another**, and the cells of a right-to-left run are shaped one at a time
  rather than in their drawn order, a lam-alef pair together (UAX #9 3.5).
- **The cursor** is drawn over the cell it is on, wherever that cell is
  drawn, in its own shape: a block over the cell, an underline under it,
  and a bar on the right side of a right-to-left letter. On a full line,
  where the next letter would wrap, it stays drawn over the last letter
  written, which on a reordered line can be mid-line.
- **The selection** is a range of the text as written: its highlight falls
  on each selected cell wherever it is drawn, so a selection across a
  Hebrew word can show as two runs, and a copy gives the written order. A
  click, a drag and a mouse report to a program name the cell the program
  wrote under the pointer (the inverse of the drawn order).
- **The alternate screen and the scrollback** are drawn the same way.
- **A line slides out the way it was drawn.** During a smooth scroll the
  line leaving the top of the window is drawn in the order it had as the
  window's first line, its paragraph read across the lines below it and
  the history above, and shaped as it was drawn still: its Arabic letters
  joined, a lam-alef pair as its ligature, a mirrored character with no
  mirroring glyph flipped, its marks on their letters and its ligatures,
  with BDSM set or reset. On the cell shader the row is walked for what
  its glyph map does not draw, as a row of the window is, through the
  order it was drawn in; a double-width line is drawn by the records the
  instance path converts it to
  (`a_right_to_left_line_slides_out_in_the_order_it_was_drawn` and
  `a_line_sliding_out_is_shaped_as_it_is_drawn_still`, the leaving row
  against the same line drawn still, every arm agreeing on that frame).
- **A double-width line runs right to left within its half.** A line
  DECDWL or DECDHL makes double-width shows half the screen's columns
  (DEC STD 070 5.14.1), and UAX #9 orders each display line (3.4), so a
  Hebrew or Arabic paragraph there sits against the half's right edge,
  its erased cells laid out before its text within the half; with BDSM
  reset, a line SCP sets right to left is reversed within the half. A
  click, a drag and a mouse report there name the character drawn under
  the pointer, and a composition is drawn as the same text written
  there, at the cursor's drawn column
  (`a_double_width_right_to_left_line_is_drawn_within_its_half`,
  `a_double_width_right_to_left_line_is_laid_out_over_its_drawn_half`,
  `a_pointer_on_a_double_width_right_to_left_line_names_the_letter_drawn`,
  `a_composition_on_a_right_to_left_row_draws_as_the_text_written`).
- **What you are typing is drawn where it will land.** An input method's
  composition and a predicted echo are laid out as if they were written
  in their cells, so their line is drawn in the order a write of the same
  text gives it: `cd` composed after `ab` in a Hebrew line is drawn
  `abcd`, the `ab` moved left to make room, and the cursor on the cell
  after it in logical order, which the paragraph draws left of `abcd`, as
  the text will be once it is sent and echoed (Jev AS_WRITTEN, 0.98 in
  both orders for the composition, 1.00 in both for the echo, asked
  again once the composition was decided; the options rendered in
  `docs/research/notes/ab/b205/`;
  `a_composition_on_a_right_to_left_row_draws_as_the_text_written`, on
  every arm, and
  `a_predicted_echo_on_a_right_to_left_row_draws_as_its_echo`). A
  bracket typed after a Hebrew letter is drawn mirrored, as its echo is
  (UAX #9 L4,
  `a_predicted_bracket_on_a_right_to_left_row_draws_mirrored_as_its_echo`).
- **Arabic letters join.** Each letter is drawn in the form its neighbours
  give it, initial, medial, final or alone, by the Unicode Standard's rules
  (`[[source: unicode-ch9]]` 9.2.2, from ArabicShaping.txt, `[[source:
  unicode-arabicshaping]]`), one letter a cell: a mark between two letters
  does not part them, ZWJ joins and ZWNJ parts. A lam joined to an alef
  forms the lam-alef ligature the standard makes obligatory (9.2.3), drawn
  centred over the two cells the pair holds. A word split by a soft wrap
  stays joined across it, as CSS Text 3 asks of a word an unforced break
  splits (`[[source: w3c-css-text-3]]`), and an erased cell between two
  letters does not part them either. A lam and an alef still form the
  ligature with erased cells between them, drawn centred over the cells
  from one to the other, and with a soft wrap between them, drawn in the
  lam's cell at the end of its row, the alef's cell on the next row left
  blank. The forms come from the face's own
  `isol`, `fina`, `medi` and `init` features, the ligature from its `rlig`,
  and from the presentation forms its `cmap` maps on a face with no such
  features. `arabic_letters_draw_joined_on_every_arm` holds the three draw
  paths to the same pixels, and to the presentation forms written out.
- **With the mode off, Arabic still joins, in the order it is displayed**,
  the order 9.2.2 states the rules in: a program that writes Arabic already
  reversed sees it joined, and one that sends presentation forms (vim's
  `arabicshape`) sees them as sent. Each row is joined alone, so with the
  mode off a word split by a soft wrap is not joined across it.
- **With the mode off, a Mongolian or Phags-pa word joins left to right**,
  its first letter, on the left, initial and its last final: the two
  scripts run left to right on a horizontal line (`[[source:
  unicode-ch13]]` 13.5, "the text should be laid out in horizontal lines
  running left to right"; `[[source: unicode-ch14]]` 14.4), so a program
  writes them in the order they read
  (`with_bdsm_reset_a_mongolian_word_is_joined_left_to_right`). A
  Mongolian letter and an Arabic one side by side do not join
  (`with_bdsm_reset_two_directions_of_joining_do_not_join`). With the mode
  on both scripts already joined in their stored order. The forms the
  face is given, in logical order, are exact against hb-shape on the
  macOS faces Noto Sans Mongolian and Noto Sans PhagsPa
  (`every_mongolian_and_phags_pa_fixture_joins_as_harfbuzz_does`).

## What decided each choice

Each choice was read against its standard first: ECMA-48 (`[[source:
ecma48]]` 7.1, 7.2.1, 8.3.111), ECMA TR/53 (`[[source: ecma-tr53]]`), UAX
#9, and terminal-wg's draft recommendation (`[[source:
terminal-wg-bidi-basic-modes]]`, `[[source: terminal-wg-bidi-display-only]]`,
`[[source: terminal-wg-bidi-erased-cells]]`, `[[source:
terminal-wg-bidi-cursor]]`, `[[source: terminal-wg-bidi-escape-sequences]]`,
`[[source: terminal-wg-bidi-ecosystem]]`); VTE, mintty, iTerm2, Konsole,
mlterm and WezTerm at source (note AB section 3). Jev was asked twice in
each order, the second time after more research, and L4 twice more; the
requests and the renders it was shown are in
`docs/research/notes/ab/step2/`.

**Decided by Jev, at 0.9 or more in both orders:**

| choice | decided | Jev |
|---|---|---|
| the bidi controls stored on a cell | given to the algorithm (UAX #9 4.2's lowest reordering class needs LRM, RLM and ALM) | 1.00 / 1.00 |
| selection and copy | logical: kept and copied as written, highlighted where drawn | 0.98 / 0.98 |
| Arabic in this step | reordered, drawn isolated until joining | 1.00 / 1.00 |
| a paragraph past the capacity | the drawn rows with whole rows of context (UAX #9 HL5) | 0.98 / 0.98 |
| the lam-alef ligature in a grid | centred over the two cells the pair holds | 0.96 / 0.97 (re-ask) |
| a soft wrap inside a word | joined across it (CSS Text 3) | 0.96 / 0.90 (re-ask) |
| an erased cell between letters | transparent, as the boundary neutral it is given | 0.96 / 0.96 |
| a lam and an alef erased cells part | the ligature, centred over the cells from one to the other | 0.91 / 0.93 |

**Decided by the owner** (2026-10-24):

| choice | decided | the owner's words |
|---|---|---|
| the state at power-on | BDSM set, so right-to-left lines are drawn right to left from the start; `RIS` returns it to set, `DECSTR` keeps it | "Should right-to-left lines be on from the start? C" |
| the cursor on a full reordered line | drawn over its cell, in its shape | "On a full right-to-left line, is the cursor drawn or hidden? drawn and based on how the cursor is , line, block or udnerline" |
| which way a paragraph runs when nothing says | its first strong letter's way (UAX #9 P2-P3), a right-to-left line against the right edge, for every example shown but the wrapped paragraph and the table | "all match C besides these 2 , the wrap line looks bad and the table should be like B but דנהshould b be rtl" |

ECMA-48 7.1 recommends the reset state as every mode's initial one, and
its Annex D leaves "the initial state of a device upon power up including
the settings of the modes" to the implementation; terminal-wg makes the
set state the default. `RIS` follows ECMA-48 8.3.105's "reset to its
initial state, i.e. the state it has after it is made operational", as
VTE's does (`[[source: vte-modes-56bad78]]` 186 declares BDSM set by
default, `[[source: vte-vteseq-cc-56bad78]]` 8315's RIS calls `reset()`,
which `[[source: vte-modes-hh-56bad78]]` makes every mode's default) and
as baud's own SRM already does, where the clause "set the modes into the
reset state" in its "may imply, if applicable" list would turn the mode
off. Jev's lean, which the owner can veto: INITIAL_STATE 0.78 / 0.82, then
0.64 / 0.86 with VTE's source; both asks said wrongly that every other
ANSI mode baud has starts reset, and asked again with SRM, 0.68 / 0.84.

**terminal-wg's three modes (2026-10-25)**, read against the draft's
pages (`[[source: terminal-wg-bidi-escape-sequences]]`, `[[source:
terminal-wg-bidi-basic-modes]]`, `[[source: terminal-wg-bidi-misc]]`,
`[[source: terminal-wg-bidi-arrow-key-swapping]]`) and VTE's and mintty's
code. The draft has no box-mirroring or autodetection page of its own.
Jev decided two at 0.9 in both orders: a mode applies at once only at a
paragraph's start (FIRST_POSITION, 0.90/0.94) and `?2500` mirrors the Box
Drawing block alone (BOX_DRAWING, 0.94/1.00); and, after a re-ask,
the cell's direction on a table row for the keys (CELL, 0.98/0.98).
**Taken from Jev's lean, which held in every answer; the owner can veto
each:**

| choice | taken | Jev, first ask then re-ask |
|---|---|---|
| `?2501` at power-on, and SCP under it | set, so a line nobody set a direction for takes its first letter's (the owner's C), and SCP is the fallback while it is set, as mintty does; so SCP 2 alone no longer turns an English line | 0.82 / 0.86, 0.82 / 0.89 |
| `?1243` at power-on | set, "needs to be the default", though baud detects direction from the start, which the draft's compatibility argument did not assume | 0.00 / 0.02, 0.86 / 0.86 |
| which box characters `?2500` mirrors | each drawn right to left (L4's resolved R, HL6 widening which characters), so a table row's rules are not | 0.60 / 0.74, 0.88 / 0.92 |
| `DECSTR` | returns all three in force to power-on, as mintty and VTE do | 0.30 / 0.51, 0.77 / 0.77 |

The requests, the renders shown and the answers are in
`docs/research/notes/ab/b211/`.

**Taken from Jev's lean, which held in all four answers but for the
erased cells, where the first ask's SKIPPED (0.10 / 0.18 against a space)
was re-offered as AS_BN, the same behaviour named after its class; the
owner can veto each:**

| choice | taken | Jev, first ask then re-ask |
|---|---|---|
| where BDSM lives | terminal-wide, as an ECMA-48 mode; per-paragraph state is the next step | 0.86 / 0.82, 0.72 / 0.76 |
| an erased cell between characters | a boundary neutral, moved with the character before it | (SKIPPED) 0.10 / 0.18, (AS_BN) 0.85 / 0.89 |
| the alternate screen | drawn like the primary ("The two screens should behave identically") | 0.58 / 0.34, 0.79 / 0.76 |
| Arabic with the mode off | joined in display order (terminal-wg's text says no shaping; its author's issue #7 reverses it) | 0.24 / 0.16, 0.66 / 0.58 |
| a lam and an alef a soft wrap parts | the ligature in the lam's cell, the alef's cell left blank | 0.19 / 0.15, 0.84 / 0.80 |

The joining questions, their renders and Jev's answers are in
`docs/research/notes/ab/joining/`.

**The table row, taken from Jev's lean, which held in all four answers;
the owner can veto each.** Jev was shown the owner's table, a psql-style
table drawn with `|`, a Hebrew sentence with a `|` in it beside a shell
pipeline, and a tree, each drawn by baud's renderer under five options
(`docs/research/notes/ab/b192/`), and asked again with HTML's `dir=auto`
(`[[source: whatwg-html-dom-dir]]`: a direction from "the first character
with a strong directionality") and CSS's `text-align: start`
(`[[source: css-text-3]]`), by which a web table's Hebrew cell sits
against its right edge. Prior art read at source: mlterm splits a line at
U+2500-U+259F and DEC graphics and resolves each segment with its own
direction, keeping a right-to-left segment's text at its left, ASCII
separators only by the user's choice (`[[source:
mlterm-vt-bidi-c-46cbbc9]]`); Konsole makes U+2500-U+25FF strong L, so a
table row stays left to right with no direction per cell (`[[source:
konsole-terminaldisplay-c8bc143]]`), a change of Bidi_Class no UAX #9
higher-level protocol allows.

| choice | taken | Jev, first ask then re-ask |
|---|---|---|
| which characters divide a row's cells | the Box Drawing block (every ASCII vertical bar as well would split a Hebrew sentence; a bar under a bar or a plus sign was the runner-up) | 0.52 / 0.92, 0.71 / 0.91 |
| how a row is known to be a table row | any rule in the paragraph (not a rule lined up with one in the row above or below) | 0.52 / 0.44, 0.42 / 0.30 |
| where a right-to-left cell's text sits | against its cell's right side, the cell its own paragraph (not mlterm's, at the cell's left) | 0.50 / 0.42, 0.52 / 0.22 |

**L4 for every mirrored character, built to the text.** The 126 mirrored
characters with no mirroring glyph draw their glyph flipped, and a
mirrored character carrying a mark draws its mirror under the mark. The
first two asks put only the first case, as 128 characters, and on the
premise that every character with a mirroring glyph draws it, which the
build did not then meet for a cell carrying an RLM; their answers
(counted, 0.88 / 0.92 and 0.86 / 0.92) are superseded. Asked with the true
counts, and again with the prior art (VTE swaps a cell's base and keeps
its marks; HarfBuzz substitutes or applies the font's `rtlm`, which baud's
faces do not carry; no terminal surveyed flips a glyph), Jev leaned to
build both: a flipped glyph 0.58 / 0.50 then 0.70 / 0.66, a substitute
base under the marks 0.62 / 0.54 then 0.72 / 0.66. A departure Jev did not
decide at 0.9 is a defect (the owner, 2026-10-10), so both are built as
L4 reads (`docs/research/notes/ab/step2/l4.png` showed both). One case
neither settled, one of the 126 carrying a mark the face draws apart, Jev
decided at 0.91 / 0.94: the base flipped and the mark not, which is
Bidi_Mirrored=N and so not "depicted by a mirrored glyph", over the whole
cell flipped (0.06 / 0.04) and the cell left unmirrored (0.00 / 0.00);
the renders and answers are in `docs/research/notes/ab/b197/`.

terminal-wg prefers hiding the cursor on a full reordered line ("It's
more consistent to make it disappear"); the owner chose it drawn.

The owner chose the direction from renders of three options on eight
examples: B, every line left to right (terminal-wg's default, built
before); C, the first strong letter with a right-to-left line against the
right edge; and C2, the first strong letter kept at the left edge. Jev had
leaned first strong 0.56 / 0.48, then left to right 0.22 / 0.30. The two
examples the answer excepts, a table row and a paragraph a soft wrap
splits, are under "What is not built yet".

## What it costs

Measured with the `frame` bench's convert column (then with the mode set
before a recording; since BDSM is set at power-on the bench draws with it
on and `FRAME_BIDI=explicit` turns it off), base 9dd37f0f against step 2
built the same way from `git archive`, an A/A copy, four arms rotated over
eight rounds with the first dropped, on mains (the docs/record.md entry,
"Right-to-left lines are drawn in their visual order", has the table):

- **With the mode off (`CSI 8 l`): no difference.** The layout returns
  at once; every recording's convert range overlaps base's.
- **With it on, as baud starts, recordings with almost no right-to-left
  character** (btop has none; cat-scroll one Arabic letter): ranges
  overlap. A drawn
  row is tested only when it changes, by one load a cell from a table
  compiled from the class data; an unchanged row costs a byte compare. Rows
  of a paragraph that runs past the window's edge, above it or below it,
  are tested on every frame, with no cache and no measurement: whenever the
  window's first or last row continues a soft-wrapped paragraph, up to
  `baud_render::CONTEXT` / columns rows (92 at 200 columns) are read cell
  by cell each frame.
- **With it on, recordings that hold Hebrew and Arabic** pay, from the
  start now, for laying
  those rows out: cat-cjk's convert +9.8%. vim-unicode reads 2.1% lower,
  its range clear of base's (268.7-273.7 against 275.7-280.1), which is
  unexplained.
- **Unmeasured: the app's cell path** copies the whole window
  (`Orders::window`) and clones the maps for the overlay walk on each frame
  in which any row is reordered; the bench's convert column does not hold
  that.
- Reading a program's output is unchanged: the core's only change is BDSM's
  state, and the instructions `ingest2` retires overlap base's on five
  recordings.
- **A session that has drawn no joining letter pays nothing measurable
  for joining.** With the mode on, a row is compared with its last copy each frame and
  tested only when it changed, as step 2 already did for reordering; the
  one pass that lets a row of Latin, Han or box drawing go loads a table
  that holds the joining blocks with the right-to-left ones, so it asks
  both questions for the price of one. With the mode off nothing is read
  for joins until the first joining letter is drawn. After that each path
  keeps the last frame's joins and reads again only the rows its own
  compare finds changed: the conversion's, and on the GPU's cell path,
  the default, the compare of the screen with the copy the last frame
  made, which then reads every row of a frame that changed rather than
  stopping at the first row that differs. That cost is unmeasured; a
  frame whose screen did not change reads no row for joins.
  A row that holds no joined cell runs the loop that gathers its runs as
  before, and a run with no joining letter is shaped with the grid's
  features. On btop and cava, which hold no joining letter, and git-log,
  which holds one, the `frame` bench retires fewer instructions than
  step 2 on all three in both modes, each outside step 2's own spread:
  btop 0.24% fewer with the mode on and 0.25% with it off, cava 0.06%
  and git-log 0.23% in both. On an earlier build of the change, against
  step 2 with only the loop that gathers a row's runs moved out of line,
  as it now is, btop and cava overlapped it in both modes, and a screen
  that holds Arabic paid for joining it: vim-unicode +0.6% with the mode
  on and +1.6% with it off, where the first joining letter a renderer
  draws has its rows converted twice, once.
- **The two rules that scan more than once are linear, measured.** N0
  scans each bracket pair's contents, and for a pair holding only the
  opposite direction the text before it; BD16's stack of 63 bounds how
  many pairs hold any one character, and a resolved bracket is strong,
  so each scan back for context stops at the nearest bracket already
  resolved (the previous pair's closer when pairs sit side by side, its
  opener when they nest). X5c scans from each FSI to its matching PDI,
  stepping over each nested isolate by its match, so a character is read
  by its innermost isolate alone. At 256, 512, 1024 and 2048 characters,
  `baud-bench/benches/bidi_rows.rs` reads 7.8-8.5 ns a character on rows
  of recorded text (the medians of four rounds), and no crafted row's
  doubles with the row: from the shortest to the longest, bracket pairs
  side by side read 9.4 and 9.1, neutral pairs before one that reads
  back over them all 9.6 and 8.8, FSIs nested 1023 deep 10.9 and 8.7,
  unmatched FSIs 10.1 and 8.5, 64 nested brackets, which overflow BD16's
  stack and stop it, 8.5 and 6.7, and 63 nested pairs around a run of
  Hebrew, the deepest BD16 holds, 35 and 39, rising as the 126 brackets'
  share of the row falls. A build that reads the whole text before every
  pair, with a first-strong scan that walks into nested isolates, reads
  more a character at each doubling (pairs side by side 32, 54, 99 and
  189, nested FSIs 22, 31, 50 and 88), so the bench sees a quadratic
  when there is one.

## What is not built yet

- **A paragraph a soft wrap splits mid-word** ("the wrap line looks bad"):
  an English word the wrap split lands at opposite ends of its two rows,
  drawn as before until the owner chooses. UAX #9 breaks a paragraph into
  lines first and reorders each line (L1-L2), and a terminal keeps each
  cell in its row, so in a right-to-left paragraph a run the wrap splits
  ends its first row at the left and begins the next at the right under
  every option that keeps the paragraph's direction on every row: the
  paragraph resolved whole (built; VTE's way, `[[source:
  vte-bidi-cc-56bad78]]`, and terminal-wg's paragraph, the rows between
  hard newlines, `[[source: terminal-wg-bidi-paragraphs]]`), or each row
  resolved alone in the paragraph's direction, which draws the same unless
  an embedding or a bracket pair crosses the wrap. Each row its own
  paragraph, as mlterm resolves each screen line and Konsole does with its
  BidiLineLTR setting off (`[[source:
  mlterm-vt-line-bidi-c-46cbbc9]]`, `[[source:
  konsole-terminaldisplay-c8bc143]]`), and every row left to right put the
  halves at matching ends (Konsole draws every row left to right while its
  BidiLineLTR setting is on, its default), at the cost of a row that runs left to right
  with the Hebrew period at its right end. The four options on three
  wrapped paragraphs: `docs/research/notes/ab/b192/wrap.png`.
  The owner rejected all four (2026-10-24) and asked for a mixed
  paragraph broken into lines of one direction each; the texts, the prior
  art, three prototypes and the recommendation are note BD,
  `docs/research/notes/bd-bidi-wrap.md`.
- **BDSM per paragraph**: terminal-wg keeps implicit or explicit per
  paragraph beside the three properties above; baud's BDSM is
  terminal-wide (the table under "What decided each choice").
- **The arrow keys on a paragraph longer than the layout reads**: the
  direction the keys swap by is read from a paragraph's first row, where
  the layout of one longer than `baud_render::CONTEXT` codepoints reads
  the rows around the window and keeps that piece's level.
- **SCP's edges**: `CSI 0 SP k`, which terminal-wg reads as the terminal's
  default, is refused, so a program that restores the default that way
  leaves its lines set until `DECSTR` or `RIS`; `DECSC` and `DECRC` do not
  save the direction (terminal-wg's open issue 3).
- SRS, SDS, SAPV and DCSM; DEC's DECRLM, DECRLCM and DECHEM.
- **Joining's edges**: Syriac's alaph forms (`fin2`, `fin3`, `med2`,
  Unicode 9.3) are not drawn, so Syriac joins by the Arabic rules alone; a
  selection that covers only one cell of a lam-alef pair colours the whole
  ligature in the lam cell's colours; an alef whose lam has scrolled above
  the window, or is above it while looking back, keeps its final form, the
  ligature being on a row not drawn; no discretionary ligature is drawn (the grid's `liga` is off); a
  face's contextual lookups across letters (`calt`, `rclt`) are not run,
  each letter being shaped alone; a face that joins through `morx` (Geeza
  Pro) is drawn through its presentation forms, which differ from its own
  chains where those pick an alternate (a final farsi yeh) or form the
  Allah ligature without the shadda 9.2.11 asks for.
- **A mark on a flipped glyph**: on a flipped base a mark's image is put
  where the flip puts its box, not where the flip puts its attachment
  point, so it is exact only for a mark centred on its attachment point.
- **Drawn in their stored order still**: the status line and images placed
  on a reordered row, a decision each: the status line is no line of the
  data a program writes the screen with, and an image is placed over cells
  rather than in one.
- **A predicted echo on a double-width line** is not laid out as
  written: its line is laid out without it.
- **A composed Arabic letter is drawn alone**, while the letters beside it
  join toward it as they will once it is written.
- **Telling programs it is supported**: DECRQM 8 is the only probe; no
  terminfo entry says so (note AB Q18).

## Decided in step 1

**Unicode version (Q20): 17.0.0**, the version of every other UCD table
baud compiles. Jev chose it over pinning only the bidi tables at 18.0.0, or
moving every table at once, with confidence 0.95 and 0.98 in the two orders
it was asked. Unicode 18.0.0 adds two brackets and five mirroring pairs.
The data in its two test files is the same, so a later move of every table
to 18.0.0 carries the bidi tables with it.
