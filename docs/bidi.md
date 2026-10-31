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
Right arrow keys swap, so they move the cursor the way the line reads.
**Finished text wraps at its words**: a word that does not fit at the end
of a line moves whole to the next, and in a Hebrew paragraph an English run
the wrap would cut gets a line of its own, left to right against the left
edge, the Hebrew resuming on the next line (`[screen] wrap_words`, on by
default). This is step 3 of right-to-left text.
Step 1 built the Unicode Bidirectional Algorithm; step 2 put its order on
the screen; step 3 joins Arabic letters.

## What you see

- **A line is drawn in the order UAX #9 gives it**, revision 51 for
  Unicode 17.0.0 (`[[source: uax9-bidi]]`), on all three draw paths: the
  Metal cell shader the app draws with, the Metal instances and the CPU
  compositor. `right_to_left_rows_draw_in_their_visual_order_on_every_arm`
  holds the three to the same pixels, and to the pixels of the same text
  written already in visual order with the mode off.
- **A finished paragraph is drawn at its words** (`[screen] wrap_words`,
  `"on"` by default; `"off"` draws every row as the terminal cut it at the
  last column, as before). No word is split across two lines: a word that
  does not fit drops whole to the next line, a line's trailing spaces
  hanging past the edge, and only a word wider than the whole line breaks
  at its edge, between cells (UAX #14's emergency break, `[[source: uax14]]`
  section 3). Every break is a UAX #14 opportunity, chosen greedily, which
  section 1 leaves to "higher level software"; an erased cell between
  characters is given to the breaker as a space. In a paragraph that mixes
  directions, **a run of the other direction that the wrap would split --
  its words falling on two lines, or one word wider than the whole line --
  gets lines of its own**, as many as it needs at its words, laid out in
  its own direction against its own language's edge -- an English line
  flush left with its comma at its end, a Hebrew line in an English
  paragraph flush right -- and the paragraph's language resumes on a fresh
  line. A run that fits stays inline, and so does one the wrap breaks
  before rather than inside: a single foreign word that does not fit drops
  to the next line inline, as any word does, and so does a run of several
  that fits whole there (the B and own-direction panels of `docs/research/notes/bd/owner-runs/`, chosen 2026-10-27). A run is a stretch above the paragraph's level
  that holds a letter of the other direction, so a number in a Hebrew
  sentence stays on its line; its ends move to the opportunities around
  it. **Only finished text**: on the main screen, a paragraph wholly in the
  history, or wholly above the latest prompt while the shell waits at it
  (`OSC 133`); never the line being typed, a running command's rows, the
  alternate screen, or the live screen of a shell that sends no marks. A
  paragraph with a row holding text past the window's edge keeps the
  terminal's cut: a narrowing while the shell waits at a marked prompt
  re-cuts no row and keeps each row's cells past the new edge, and the
  cells the window shows, read as the paragraph's text, would join into
  words the program never wrote.
  With BDSM reset nothing is re-broken (ECMA-48 7.2.1's EXPLICIT: the
  program addresses the presentation lines). **The lines a paragraph gains
  push the text above it up**: the window's last row stays, so the cursor,
  the prompt and every row a program can address keep their rows, and the
  lines pushed above the window are read by scrolling back; at the top of
  what the history keeps, the oldest re-broken paragraphs in the window
  keep the terminal's cut until the rest fits. A paragraph whose words
  need fewer lines than it was stored on -- trailing spaces the terminal
  wrapped onto a row of their own, which hang -- keeps the terminal's cut,
  so the window's last row stays where it is and no line made only of
  rows above the window is drawn in the gap, where a mouse report could
  not name it. The cursor, a selection's
  ends, a mouse report and a find match name stored cells, each drawn
  where its cell is drawn. **Every cell drawn can be selected and found**:
  where a paragraph runs on from above the window, the window's first
  line holds cells of the rows above it, and while the window looks back
  its last line holds cells of the rows below it; a selection, a find
  mark and a pointer that selects name each by its stored line
  (`a_selection_reaches_the_cells_a_window_line_draws_from_above_it`,
  `a_find_mark_reaches_the_cells_a_window_line_draws_from_above_it`,
  `a_drag_over_cells_drawn_from_above_the_window_selects_them`), and a
  mouse report over one names the nearest window cell, since a program
  addresses no line past the window. A copy gives the stored text in logical
  order, soft wraps joined; under `?96` it reads each display line from
  its right edge, a block each line it selects a cell of, and a paragraph
  the window keeps at the terminal's cut by that cut. A picture moves with
  its text as on a scroll, anchored on a row the display pushed out of the
  window or above it as well, and is mirrored by the direction of the line
  its row is drawn on, so on an English run given lines of its own it
  stays left to right; and a line a smooth scroll slides out, the cells a
  trail's flash bound reads and a placeholder's run, on the window and on
  that sliding line, are the display's. All three draw paths read the one display:
  `a_re_broken_paragraph_draws_the_same_on_every_arm`. **A departure, a choice the project made**: UAX #9 resolves a paragraph at one level and orders each line
  by it (section 3, L1-L2); a moved run's line is laid out at its own
  direction's level, and each piece of the paragraph between moved runs is
  resolved as a paragraph of its own at the paragraph's level, which draws
  otherwise only where an embedding or a bracket pair crosses a moved
  run's edge. terminal-wg keeps each cell in its row
  (`[[source: terminal-wg-bidi-display-only]]`); a re-broken paragraph's
  cells move between rows.
- **A paragraph is the rows a soft wrap joins.** Its levels are resolved
  over all of them, rows above the window and below it included, so an
  override or an embedding on one row reaches the next. **Text written
  inside DECSLRM side margins is laid out inside them**: a row that
  wrapped at side margins narrower than the page keeps the margins it
  wrapped at, and its paragraph is its cells inside them and the next
  row's inside the same, one paragraph across its rows, while each run of
  cells outside them -- another pane's text -- is a paragraph of its own,
  ordered in its own columns, as a table row's cells are (DECSLRM "sets
  the left and right margins to define the scrolling region",
  `[[source: vt510-decslrm]]`; that the cells outside are no part of the
  paragraph is baud's reading;
  `a_paragraph_inside_side_margins_is_laid_out_inside_them`, and on every
  arm `a_paragraph_inside_side_margins_draws_alike_on_every_arm`). The
  margins move with the paragraph's cells when a wrap at the bottom
  margin, `IL` or `DL` scrolls the rectangle under the margins it
  wrapped at (`a_paragraph_the_rectangle_scrolls_keeps_its_margins`), and stop
  applying once the row wraps again at the page's edge or wraps no
  longer (`a_row_wrapped_again_at_the_page_edge_is_read_whole`,
  `a_row_that_no_longer_wraps_is_read_whole`). A paragraph past
  `baud_render::CONTEXT` codepoints, resolved in pieces, gives the runs
  outside the margins of its later pieces its first piece's level, where
  UAX #9 P2-P3 gives each its own: an open departure. A paragraph longer
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
  is, where the flip puts it. The three
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
  Its joined letters read their own words, not the first row's below
  them, also a word a soft wrap carries onto that row
  (`a_sliding_lines_letter_reads_its_word_as_drawn_still`).
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
  after it in logical order, which the paragraph draws left of `abcd`, as the text will be once it is sent and echoed (the options rendered in
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
- **Syriac's alaph takes its own forms, and each letter is drawn with the
  letters of its word around it.** An alaph that ends its word after a
  letter that does not join it is drawn in its `fin2` form, and after
  dalath or rish in its `fin3` form (`[[source: unicode-ch9]]` 9.3, S2
  and S3); after a letter that joins it, it stays `fina` (S1); and one
  joined inside a word is drawn in its `med2` form (`[[source:
  ms-syriac-script-development]]`, the OpenType Syriac model). An alaph
  alone after a space or ZWNJ is drawn as it is at the start of a line,
  as HarfBuzz and CoreText draw it. Every joined letter is shaped
  with as many letters of its word before and after it, in logical order,
  as its face's rules read, up to 16 a side, across a soft wrap and past
  erased cells as joining reaches, each rule meeting the letters it reads:
  a face's required ligatures and its
  contextual alternates across letters (`rlig`, `rclt`, `calt`) are drawn,
  Noto Sans Syriac's wider alaph after a beth and SF Arabic's dotted
  letters taken apart into skeletons and dots among them. `rclt` and
  `calt` run as one stage, the union of their lookups in lookup order
  (`[[source: ms-opentype-chapter2]]`), after `rlig`, as the Arabic model
  orders them (`[[source: ms-arabic-script-development]]`). The glyphs are
  exact against hb-shape on Noto Sans Syriac and SF Arabic
  (`every_contextual_fixture_joins_as_harfbuzz_does`), and the draw paths
  agree (`a_joined_letter_reads_its_word_on_every_arm`); a letter whose
  word changes on another row is drawn again
  (`a_changed_word_repaints_the_row_of_the_letter_that_reads_it`). A face
  that would take letters into a glyph across cells, as SF Arabic's `rlig`
  forms the word Allah with no shadda, which 9.2.11 says it should not,
  draws each of those letters alone in its cell, as the grid draws no
  ligature but lam-alef
  (`a_word_takes_no_letter_into_a_ligature_across_cells`).
- **Arabic and Syriac draw with no font of your own.** No face of baud's
  macOS stack maps either script, and a character no listed face maps is
  asked of the system the first frame that draws it and drawn from the face
  answered from then on: on macOS CoreText's answer from baud's own font,
  Courier New for Arabic and Urdu and Noto Sans Syriac for Syriac, each
  joined as above; on Linux the installed face fontconfig's charset match
  picks. The box is left only where no installed font has the
  character (`[[source: unicode18-ch5]]` 5.3); each character is asked for
  once a process, and the faces answered are kept across a config reload
  (`the_system_draws_arabic_and_syriac_no_listed_face_has`).
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
- **A status line in Hebrew reads right to left.** The line a program
  opens under the screen (`DECSSDT 2`) is laid out as a line of its own,
  as every line above it is: from the right edge when its first strong
  letter is Hebrew or Arabic, joined and mirrored, and reversed with the
  mode off when its path is right to left. SCP and `?2501` sent while
  `DECSASD` selects it set its own direction and leave the screen's, the
  line keeping "its own ANSI text state and settings" (`[[source:
  dec-std-070]]` 070-14 § 14.2;
  `a_hebrew_status_line_runs_from_the_right_edge`,
  `the_status_line_takes_the_direction_set_on_it`); no line of the
  screen is its context
  (`the_status_line_reads_no_line_of_the_main_display_as_context`). The
  cell path draws it as the instance path does
  (`the_cell_path_draws_the_status_line_as_the_instance_path_does`).
  Its Arabic and Syriac letters are shaped with their words, as a
  screen line's are (`arabic_on_a_hebrew_status_line_joins_with_its_word`,
  `a_status_lines_letter_reads_its_word_on_every_arm`).
- **A picture after a Hebrew caption sits after it.** A Sixel, a kitty
  picture or an iTerm2 image anchored on a row drawn right to left is
  placed mirrored, its box `cols - (col + w)` from the left, by the
  direction of the row it is anchored on -- where the display draws that
  row's paragraph at its words, the line the row is drawn on
  (`a_picture_is_mirrored_by_the_direction_of_its_display_line`) -- and
  its pixels are not flipped:
  terminal-wg's "images must not be flipped by default in RTL mode ... In
  paragraphs of RTL direction, the overall layout should be the mirrored
  version of the LTR one by default" (`[[source:
  terminal-wg-bidi-misc]]`), the anchor being where the picture is in the
  text, UAX #9 3.2's object
  (`a_picture_after_a_hebrew_caption_is_placed_mirrored`,
  `a_mirrored_picture_is_not_flipped`). A tall picture keeps its place
  once its top row scrolls off
  (`the_anchor_row_places_the_whole_box_after_it_scrolls_off`); one on a
  table row, which keeps its columns, keeps its own
  (`a_picture_on_a_table_row_keeps_its_place`). kitty's placeholder cells
  are characters, ordered with the text, and each strip is drawn over its
  cells where they are drawn
  (`a_placeholder_strip_is_drawn_over_its_cells_where_they_are_drawn`), on
  the line a smooth scroll slides out above the window as well
  (`a_placeholder_strip_on_the_leaving_row_is_drawn_where_its_cells_are`,
  `the_leaving_rows_placeholders_are_the_line_the_display_draws`).
- **A program can choose how the text it writes after is drawn** (SAPV,
  `CSI Ps... SP ]`, `[[source: ecma48]]` 8.3.108): Arabic-Indic digits
  (2), a decimal number's point drawn as a comma (10), a letter drawn in
  the joining form asked (5 to 8, held for every letter after it by 21),
  Arabic joined without the lam-alef ligature (14) or not joined at all
  (18, 19), vowels hidden (16), italics on a right-to-left run slanted
  to the left (17), and, with the mode off, a right-to-left line's
  brackets left as stored (15). Each value is carried by the characters
  written after it, so what was written before keeps its own. With the
  mode on, brackets on a right-to-left run are always mirrored: UAX #9
  L4 governs there. Vowels after their letter (12) are refused and
  counted, a vowel having no column of its own in the grid.
  Under 17 an italic is drawn as its upright glyph sheared left by the
  italic face's own angle, `post.italicAngle`, or 14 degrees where the
  face gives none, on every draw path
  (`value_17_slants_right_to_left_italics_to_the_left`); left-to-right
  italics lean right as the italic face draws them, and an italic the
  stack has no italic face for is drawn upright either way.
  A letter drawn in a form a program asked is shaped with the letters of
  its word around it, joined or not, so a face's contextual shapes still
  fit its neighbours, and they read it in the form asked
  (`a_form_asked_wins_over_the_word_and_the_word_reads_it`); a word
  slanted under 17 reads its own letters as it does upright
  (`sapv_meets_a_letters_word_on_every_arm`), a lam-alef pair among them
  (`a_slanted_lam_alef_is_read_in_its_word_on_every_arm`). A letter's
  word reads the digits and the decimal mark beside it as 2 and 10 draw
  them, Arabic-Indic and a comma, so a face's contextual lookups meet
  what is shown (`a_word_reads_its_digits_as_drawn`,
  `a_letters_word_reads_its_digits_as_drawn_on_every_arm`).
  Text an input method is composing is drawn as it will be written:
  over digits drawn as Arabic-Indic it shows its own characters, and
  where 17 slants the cell it starts on it leans left
  (`a_composition_over_varied_text_draws_as_its_text_written`,
  `a_composition_over_varied_text_draws_as_written_on_every_arm`).
  Arabic an input method is composing is drawn joined, as it will be
  once written (`[[source: unicode-ch9]]` 9.2.2): its letters take their
  forms with one another and with the letters written either side of it,
  which take theirs from it, a lam and an alef make their ligature, also
  when one of the two is written already (9.2.3), and on cells SAPV 8
  holds under 21, the letters composed there take its final form; with
  BDSM reset too, and on a double-width line
  (`a_composition_of_arabic_draws_joined_as_written_on_every_arm`, on
  every arm). Where a composed letter's ink meets a written one's, the
  two are drawn in the order the same text written is, left to right. On
  a double-width line a composition holding a character the line
  mirrors, such as `(`, draws that mirror once
  (`a_composition_on_a_right_to_left_row_draws_as_the_text_written`).
- **A program can mark a run of text to run left to right, right to left
  or reversed** (SDS, `CSI 1 ]` and `CSI 2 ]`, and SRS, `CSI 1 [`, each
  ended by its own `0`; `[[source: ecma48]]` 8.3.114, 8.3.137). With the
  mode off each string is drawn in its own direction inside its line,
  strings nested inside strings and two side by side kept apart, as
  TR/53's worked example 1 draws them (`[[source: ecma-tr53]]`;
  `tr53_example_1_left_to_right`, `tr53_example_1_right_to_left`). With
  the mode on, as baud starts, each string is laid out by UAX #9 as an
  isolate of its direction: a Hebrew string next to another stays apart
  from it, and a number inside one keeps its order
  (`with_bdsm_set_each_string_is_an_isolate`). The string travels with
  its characters, through edits, scrolling, the history and a change of
  width; erasing a character leaves its place in the string, and a copy
  gives the text as written.
- **With the mode off, a program can edit a line as it is drawn** (DCSM,
  `CSI 9 l`, `[[source: ecma48]]` 7.2.3, the state baud starts in): on a
  line holding a string, CR goes to the character drawn at the line's
  start, DCH deletes and ECH, EL, EA and EF erase the characters drawn
  where the cursor's drawn place reaches, ICH opens room where the cursor
  is drawn, a character typed in insert mode goes in where the cursor is
  drawn, a line feed keeps the cursor's drawn column, and a cursor report
  gives it (`dch_deletes_the_cells_drawn_at_its_positions`,
  `ef_erases_the_field_drawn_at_its_position`,
  `irm_under_presentation_inserts_at_the_presentation_position`,
  `cpr_reports_the_presentation_column`). `CSI 9 h` edits by the cells as
  stored. **The cursor moves as drawn in both**, as ECMA-48 defines every
  cursor function: CUP and CHA go to the character drawn at the column
  they name, CUF, CUB, CUU, CUD, CNL, CPL and the tabs step from the
  cursor's drawn place, and on a right-to-left line CUF moves right on the
  screen and CUB left (`cup_goes_to_the_cell_drawn_at_its_position`,
  `on_a_right_to_left_line_cuf_moves_right_on_the_screen`); HVP and HPA
  name the character stored at the column, which is how TR/53's worked
  example 2 reaches its data columns here
  (`tr53_example_2_data_lines_by_the_data_movers`). With the mode on it
  does nothing, as ECMA-48 says.
- **A program can make an insertion push the text before the cursor**
  (HEM, `CSI 10 h`, `[[source: ecma48]]` 7.2.9): DCH closes its gap from
  the line's start, ICH pushes toward it, and a character typed in insert
  mode goes in at the cursor, which stays where it is
  (`irm_under_preceding_inserts_at_the_cursor_which_stays`); on a line
  drawn as edited, the characters drawn before the cursor's place move.
  `CSI 10 l`, the state baud starts in, pushes the text after it.
- **A program can write right to left, as on a VT510 with a Hebrew
  keyboard** (DECRLM, `CSI ? 34 h`, `[[source: vt510-decrlm]]`): each
  character is written at the cursor and the cursor moves left, BS moves
  right, CR goes to the right margin, a character at the left margin
  wraps to the right margin of the next line, and Tab goes back a stop. A
  line written so is drawn as written, whatever the mode, so a Hebrew word
  reads as the VT510 shows it, until the line is erased whole, when text
  written on it in order is drawn by UAX #9 again; a DECERA or DECSERA
  over the whole line leaves it drawn as written, its occupied cells
  becoming spaces, where DECSED's spaces end that
  (`a_character_is_written_and_the_cursor_moves_left`,
  `a_row_decrlm_wrote_is_drawn_as_its_cells_hold`,
  `a_row_decrlm_wrote_erased_whole_is_drawn_by_uax9_again`). Arabic
  written so joins in the order it was written, read from the right, and
  a SAPV form asked goes to the next character alone, as on a line
  written left to right
  (`under_decrlm_a_form_is_the_following_characters_alone`,
  `under_decrlm_letters_join_as_written_and_take_the_forms_asked`).
- **A copy can read each line from its right edge** (DEC's DECRLCM,
  `CSI ? 96 h`, `[[source: vt510-decrlcm]]`): each line of the copy
  gives its characters in the order they are drawn, from right to left,
  the lines kept in order. A line nothing reorders comes out reversed,
  "ABCD" as "DCBA" as on the VT510, and a Hebrew line drawn right to left
  comes out in the order it reads (`decrlcm_copies_each_line_from_its_right_edge`,
  `decrlcm_reads_a_reordered_line_as_it_is_drawn`).
- **A program can switch to the VT510's Hebrew national mode** (DECHEM,
  `CSI ? 36 l`, `[[source: vt510-dechem]]`): lowercase letters are drawn
  as Hebrew, the DEC 7-bit Hebrew set in G0 to G3, until `CSI ? 36 h`
  puts the power-up sets back
  (`dechem_reset_designates_the_hebrew_nrcs_and_set_ascii`).
- **A program can switch the keyboard to Hebrew** (DECHEBM, `CSI ? 35 h`,
  `[[source: vt510-dechebm]]`), while the system has a Hebrew layout
  enabled: each key types the letter the standard Israeli layout puts on
  it, whatever layout the system is typing in, and in the national mode
  a Hebrew letter is sent as its 7-bit code and lowercase letters
  received are drawn as Hebrew
  (`group_two_types_the_israeli_layouts_letter_on_the_keys_place`,
  `under_seven_bit_characters_ascii_in_gl_is_drawn_as_hebrew`).

## What decided each choice

Each choice was read against its standard first: ECMA-48 (`[[source:
ecma48]]` 7.1, 7.2.1, 8.3.111), ECMA TR/53 (`[[source: ecma-tr53]]`), UAX
#9, and terminal-wg's draft recommendation (`[[source:
terminal-wg-bidi-basic-modes]]`, `[[source: terminal-wg-bidi-display-only]]`,
`[[source: terminal-wg-bidi-erased-cells]]`, `[[source:
terminal-wg-bidi-cursor]]`, `[[source: terminal-wg-bidi-escape-sequences]]`,
`[[source: terminal-wg-bidi-ecosystem]]`); VTE, mintty, iTerm2, Konsole,
mlterm and WezTerm at source (note AB section 3); the renders each choice
was weighed on are in `docs/research/notes/ab/step2/`.

**Built so:**

| choice | behaviour |
|---|---|
| the bidi controls stored on a cell | given to the algorithm (UAX #9 4.2's lowest reordering class needs LRM, RLM and ALM) |
| selection and copy | logical: kept and copied as written, highlighted where drawn |
| Arabic in this step | reordered, drawn isolated until joining |
| a paragraph past the capacity | the drawn rows with whole rows of context (UAX #9 HL5) |
| the lam-alef ligature in a grid | centred over the two cells the pair holds |
| a soft wrap inside a word | joined across it (CSS Text 3) |
| an erased cell between letters | transparent, as the boundary neutral it is given |
| a lam and an alef erased cells part | the ligature, centred over the cells from one to the other |
| the host-writable status line | laid out as a line of its own under the terminal's BDSM and the direction set on it (ECMA-48 7.2.1's IMPLICIT; DEC STD 070 ch. 14 says nothing on direction) |
| a picture on a row drawn right to left | its whole box mirrored by its anchor row's direction, or the direction of the display line that row is drawn on, unflipped (terminal-wg's images section; UAX #9 3.2); not stored, not row by row |
| SAPV 9 and 10's decimal mark | the FULL STOP between two digits (UAX #9 W4 reads a separator between numbers) |
| an alaph after a space or ZWNJ | isolated, as at a line's start: S2's character to its right is a letter of the word, as HarfBuzz and CoreText read it |

**The display's word wrap** (2026-10-27):

| choice | behaviour |
|---|---|
| where a re-broken paragraph's extra lines go | the window's last row stays and the text above is pushed up into the history; at the top of what the history keeps the oldest re-broken paragraphs keep the cut until the rest fits; the cursor, a selection, find, copy and the mouse name stored cells |
| which wrap decides "the run the wrap would split" | the display's own word wrap |
| which rows are finished | the history; the live rows above the latest prompt while the shell waits at it; none while a command runs, none without `OSC 133` marks |
| BDSM reset | nothing re-broken (ECMA-48 7.2.1's EXPLICIT) |
| an erased cell between characters | a space to the breaker |

**Chosen on 2026-10-27**, on the renders of note BD: a run the wrap would
split gets lines of its own and other runs stay inline, each moved line in
its own direction at its own edge; no word is split, in every finished
paragraph the display wraps, single-direction ones too; on by default.

**Chosen on 2026-10-24:**

| choice | behaviour |
|---|---|
| the state at power-on | BDSM set, so right-to-left lines are drawn right to left from the start; `RIS` returns it to set, `DECSTR` keeps it |
| the cursor on a full reordered line | drawn over its cell, in its shape |
| which way a paragraph runs when nothing says | its first strong letter's way (UAX #9 P2-P3), a right-to-left line against the right edge, for every example shown but the wrapped paragraph and the table |

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
off.

**terminal-wg's three modes (2026-10-25)**, read against the draft's
pages (`[[source: terminal-wg-bidi-escape-sequences]]`, `[[source:
terminal-wg-bidi-basic-modes]]`, `[[source: terminal-wg-bidi-misc]]`,
`[[source: terminal-wg-bidi-arrow-key-swapping]]`) and VTE's and mintty's
code. The draft has no box-mirroring or autodetection page of its own.
A mode applies at once only at a paragraph's start, `?2500` mirrors the Box
Drawing block alone, and the keys take the cell's direction on a table
row. **Built so:**

| choice | behaviour |
|---|---|
| `?2501` at power-on, and SCP under it | set, so a line nobody set a direction for takes its first strong letter's way (UAX #9 P2-P3, the 2026-10-24 table above), and SCP is the fallback while it is set, as mintty does; so SCP 2 alone no longer turns an English line |
| `?1243` at power-on | set, "needs to be the default", though baud detects direction from the start, which the draft's compatibility argument did not assume |
| which box characters `?2500` mirrors | each drawn right to left (L4's resolved R, HL6 widening which characters), so a table row's rules are not |
| `DECSTR` | returns all three in force to power-on, as mintty and VTE do |

**SAPV (2026-10-26)**, read against ECMA-48 8.3.108, TR/53's worked
example 1 and section 7, UAX #9's L4 and section 7, the Unicode
Standard's chapters 9 and 23 and ECMA-114
(`docs/research/notes/ab/b215/`). The decimal mark is settled (above).
**Built so:**

| choice | behaviour |
|---|---|
| what a value is attached to | each character, in its rendition, as `SGR` is |
| 12, vowels after their letter | refused and counted |
| which characters are vowels | Arabic tanwin and harakat, U+064B-U+0650, and the Hebrew vowel points, U+05B0-U+05BB and U+05C7 |
| 17, italics leaning left | the upright glyph sheared left by the italic face's angle |
| a form a letter has not got | the nearest it has, as a joiner beside it asks (Unicode 23.2) |
| 20, "device dependent" digits | as stored |
| `DECSTR` | the default presentation |
| a letter 5 to 8 asks a form of, inside a word | shaped in that form with its word around it, as a joiner beside it is ("change the context of a particular character occurrence", Unicode 23.2), so `rlig`, `rclt` and `calt` pick that form's variant (`docs/research/notes/ab/b216r/`) |

**15, where mirroring is the default**, unmirrors a right-to-left path
with BDSM reset, and with it set L4 governs. UAX #9 section 7: "mirroring
must be done according to rule L4".

The renders are in `docs/research/notes/ab/b211/`.

**DEC's Hebrew modes and ECMA-48's strings (2026-10-26)**, read against
the VT510's DECRLCM, DECHEM, DECSTR and chapter 4 pages (`[[source:
vt510-decrlcm]]`, `[[source: vt510-dechem]]`, `[[source: vt510-decstr]]`,
`[[source: vt510-chapter4]]`), ECMA-48 7.1, 7.2.3, 7.3.4, 8.3.114 and
8.3.137, and TR/53's section 7 and worked examples
(`docs/research/notes/ab/b220/`). Two are settled, both DECRLM's: a row it
wrote is drawn as its cells hold, and `HT` goes to the previous stop.
**Built so:**

| choice | behaviour |
|---|---|
| what DECRLCM's copy reads | each line's cells in the order they are drawn, from the right edge |
| a DECRLCM copy over several lines | each line reversed, the lines in order |
| SDS and SRS with the mode on | each string an isolate of its direction (UAX #9 HL3) |
| a reversed string at a line's top | reverses the line's character path, left to right where none is set |
| an end of the other kind | SDS 0 ends only a directed string and SRS 0 only a reversed one; any other end is counted |
| `DECSTR` and open strings | every string ends, the characters written keeping theirs |
| which DCSM state `CSI 9 h` selects | DATA, as 7.1, 7.2 and Table 6 have it |
| DCSM's PRESENTATION | built: the editing functions 7.2.3 lists take the presentation position on a line holding a string |
| DCSM at power-on | PRESENTATION, the reset state |
| the cursor functions and DCSM | by the positions as drawn under PRESENTATION, by cell under DATA, as TR/53's worked example 2 has `CUP` and `CHA`, where their definitions (8.3.7-8.3.23, 8.3.60, 8.3.161) move the active presentation position in every state, 7.2.3's NOTE leaves them out and Annex F calls them independent of DCSM: an open departure |
| the cursor functions under DATA, re-asked with TR/53's standing | as drawn in both states, the definitions' reading; TR/53's DATA lines by `HVP` and `HPA` (ALWAYS_EXPLICIT) |
| a character inserted under PRESENTATION | drawn at the presentation position, what was drawn there moving on, as 7.2.9 says, where TR/53's line 7 inserts before the cell in the data |
| HEM's PRECEDING under PRESENTATION | read as its DATA paragraph, the PRESENTATION paragraph's two "following" an erratum |
| HEM and `DECSTR` | kept, as its table names no HEM |
| where PRECEDING's shifted part starts | the left margin under DECLRMM, as DECRLM's insertion stops there |
| a wide character at the shifted part's start | the cursor's column and the one after it |
| IRM under DECRLM with HEM | DECRLM's own shift in both states; `DCH` and `ICH` follow HEM |
| `CUF` and `CUB` on a right-to-left line | right and left on the screen, "rightwards in the presentation component" |
| which cursor functions | every one that moves the active presentation position: the tabs, `CVT` and `VT` with `CUP` to `CUD` |
| `DECRQM 9` with the mode on | DATA's state, "considered to be set to DATA" |
| DECRLM under side margins | `CR` and the wrap's landing at the right margin, the wrap at the left one |
| DECHEM | acted on as a Hebrew VT510: reset puts the DEC 7-bit Hebrew set in G0 to G3, set the power-up sets back, as STD 070's DECNRCM algorithms do; DECNRCM itself, the opposite, stays ignored (note AF's Q-DECNRCM), so `?36 l` acts where `?42 h`, which Table 4-8 note 1 gives the same result, does nothing: an open departure |

**Built so:**

| choice | behaviour |
|---|---|
| where BDSM lives | terminal-wide, as an ECMA-48 mode; per-paragraph state is the next step |
| an erased cell between characters | a boundary neutral, moved with the character before it |
| the alternate screen | drawn like the primary ("The two screens should behave identically") |
| Arabic with the mode off | joined in display order (terminal-wg's text says no shaping; its author's issue #7 reverses it) |
| a lam and an alef a soft wrap parts | the ligature in the lam's cell, the alef's cell left blank |

The joining questions and their renders are in
`docs/research/notes/ab/joining/`.

**The table row, where the texts leave the choice open.** The options
were drawn on a table of box-drawing rules, a psql-style table drawn with `|`, a Hebrew sentence with a `|` in it beside a shell
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

| choice | behaviour |
|---|---|
| which characters divide a row's cells | the Box Drawing block (every ASCII vertical bar as well would split a Hebrew sentence; a bar under a bar or a plus sign was the runner-up) |
| how a row is known to be a table row | any rule in the paragraph (not a rule lined up with one in the row above or below) |
| where a right-to-left cell's text sits | against its cell's right side, the cell its own paragraph (not mlterm's, at the cell's left) |

**L4 for every mirrored character, built to the text.** The 126 mirrored
characters with no mirroring glyph draw their glyph flipped, and a
mirrored character carrying a mark draws its mirror under the mark. The
first two asks put only the first case, as 128 characters, and on the
premise that every character with a mirroring glyph draws it, which the
build did not then meet for a cell carrying an RLM; that reading, counting
them, is superseded. The prior art (VTE swaps a cell's base and keeps its
marks; HarfBuzz substitutes or applies the font's `rtlm`, which baud's
faces do not carry; no terminal surveyed flips a glyph) does not settle
it, and leaving them unmirrored would depart from L4, so both are built as
L4 reads (`docs/research/notes/ab/step2/l4.png` shows both). One of the
126 carries a mark the face draws apart: the base is flipped and the mark
not, which is Bidi_Mirrored=N and so not "depicted by a mirrored glyph",
over the whole cell flipped and the cell left unmirrored; the renders are
in `docs/research/notes/ab/b197/`.

terminal-wg prefers hiding the cursor on a full reordered line ("It's
more consistent to make it disappear"); baud draws it.

The direction was chosen from renders of three options on eight examples:
B, every line left to right (terminal-wg's default, built before); C, the
first strong letter with a right-to-left line against the right edge; and
C2, the first strong letter kept at the left edge. C was taken. The two
examples it excepts, a table row and a paragraph a soft wrap
splits, are under "What is not built yet".

## What it costs

Measured with the `frame` bench's convert column (then with the mode set
before a recording; since BDSM is set at power-on the bench draws with it
on and `FRAME_BIDI=explicit` turns it off), base 1b335a67 against step 2
built the same way from `git archive`, an A/A copy, four arms rotated over
eight rounds with the first dropped, on mains:

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
- **The display's word wrap** (`[screen] wrap_words`) is read off the
  window, never on the path that writes the grid: `baud-core` is
  unchanged. A frame whose window reads as the last frame's keeps the last
  layout. On the cases of note BD at 44x12 the convert column's ranges
  overlap base's on a replay, and the instructions a run retires differ
  from base's by less than base's own spread from run to run. A window that
  moves every frame lays its paragraphs out again each time, unmeasured
  at 200x50.
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
- **The letters around a joined letter cost a row without one nothing
  measured.** A joined cell's run is cached with its word, and a row with
  no joined cell gathers and shapes its runs as before. The `frame` bench,
  against the base with the walk over a run's lookups that this step makes
  read only the lookups a feature names: btop's instructions overlap the
  base's in both modes, and less-pager and asciiquarium, which hold no
  letter that joins and miss the run cache more, retire 0.5% to 0.7%
  fewer; cat-cjk, whose recording holds 537 letters that join, retires
  0.05% more with the mode on and 0.14% with it off, its ranges apart.
  Against the base as it was, btop retires 0.8% fewer and cat-cjk 10%
  fewer (10.2% with the mode on, 9.9% with it off). Unmeasured: the cell
  path gathers each joined cell's word, its faces and codepoints, every
  frame before its cache lookup.
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
- **SDS, SRS, DCSM, HEM and DECRLM cost a program that never sends them
  nothing on the text path**: a string rides in the rendition, DCSM's and
  DECRLM's tests are one bit of the mode word `CR` already reads, and
  DECRLM's writer, and a write under insert mode that HEM or a drawn line
  shifts, are reached only by the one-by-one graph. **Once a string has been begun** a
  frame reads a row's cells for strings one bit a cell, and only while a
  rendition naming a string is in the table; with BDSM set a row keeps
  its answer while its cells are the same. The
  core pays more: from then on each erase keeps strings cell by cell
  (`Paint::erase_keeping_strings`), and with BDSM reset every `CR` and
  line feed maps its line (`Paint::line_map`), several times the
  instructions on cat-cjk, less-cjk and vim, a gap.
  **Unmeasured: a copy under DECRLCM**, which lays out a whole window
  for each line of the history it copies (`Selection::text` reading each
  line through the window that shows it).
- **SAPV costs a program that never sends it nothing**: no row is read
  for a variant until one is selected (`Term::variants_seen`), and after
  that a frame reads each row's cells once for the substitutions and the
  slants. Text is read one character at a time only while a form waits
  for the next letter or the letter that took it was the last one
  written; the rendition table's inline probe keys a rendition without
  variants and with the primary font as it did before they existed; an
  alternate font's key moved with the font, from bit 48 to bit 27. `ingest2` retires no more
  instructions than before SAPV on any recording.

## What is not built yet

- **The display's word wrap, at its edges**: the oldest re-broken
  paragraphs at the top of what the history keeps, and every paragraph on
  a shell that sends no `OSC 133` marks until it scrolls into the history,
  keep the terminal's cut; a paragraph longer than `baud_render::CONTEXT`
  codepoints, a table row, a line not single-width, a row DECRLM wrote, a
  paragraph holding an SDS or SRS string and one that wrapped at DECSLRM
  side margins keep it too, rather than be re-broken inside its margins; the scroll
  position, the scrollbar and the wheel count stored lines; a link's
  preview and its Cmd-click over a cell the display draws from a line
  past the window read the nearest window cell. A picture anchored on a
  row the display draws at its words keeps its stored column, where the
  display draws the anchor's cell at another column of its line. A row a
  write from right of the right margin wrapped keeps the margins, so the
  cells it wrote between the margin and the edge are read as a paragraph
  of their own; a wide character a side margin cuts is ordered with the
  run that holds its lead; a rectangle scrolled under margins other than
  those a paragraph wrapped at moves its cells and leaves its margins on
  the rows it held.
- **A paragraph a soft wrap splits mid-word, with `[screen] wrap_words` off**
  ("the wrap line looks bad"): an English word the wrap split lands at
  opposite ends of its two rows. UAX #9 breaks a paragraph into
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
  All four were rejected (2026-10-24) for a mixed paragraph broken into lines of one direction each; the texts, the prior
  art, three prototypes and the recommendation are note BD,
  `docs/research/notes/bd-bidi-wrap.md`, and with `[screen] wrap_words` on, the
  default, a finished paragraph is drawn at its words (above, "What you
  see").
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
- **TR/53's worked example 2 under DATA**: its lines reach their data
  columns with `CUP` and `CHA`, which move the presentation position here
  in both DCSM states, as ECMA-48 defines them; `HVP` and `HPA` give its
  published result (`tr53_example_2_data_lines_by_the_data_movers`).
- **Insert mode on a line drawn as edited**: a character typed goes in
  at the cursor's drawn place and the cursor then moves on by the cells,
  so two typed into a reversed string are drawn apart, and at the line's
  edge the second goes to the next line, or with autowrap reset in front
  of the first; TR/53's examples give each insertion a string of its
  own, which a program sends.
- **Joining's edges**: with BDSM reset an erased cell between two
  letters parts them on a right-to-left path, where on a left-to-right
  one and with the mode set joining passes through it; a
  selection that covers only one cell of a lam-alef pair colours the whole
  ligature in the lam cell's colours; an alef whose lam has scrolled above
  the window, or is above it while looking back, keeps its final form, the
  ligature being on a row not drawn; no discretionary ligature is drawn (the grid's `liga` is off); a
  face whose rules read more than 16 letters either side is shaped with
  16, and a rule that reads what another made from a letter past the
  reach can draw otherwise than the word shaped whole; `mset` and `curs`,
  which the Arabic model always applies, are not
  run, and none of the five faces the joining fixtures shape carries
  them; GPOS contextual positioning (type 7) is not built; a face whose
  grid features hold a type 5 or 8 lookup admits every pair to the
  ligature prefilter rather than walking its rules; `rclt` is run for a
  run that holds a letter that joins and not for one that holds none; a
  face that joins through `morx` (Geeza
  Pro) is drawn through its presentation forms, which differ from its own
  chains where those pick an alternate (a final farsi yeh) or form the
  Allah ligature without the shadda 9.2.11 asks for.
- **A system face's letters at baud's size**: a face the system answers is
  drawn at the embedded font's em and not narrowed to the cell (rule D,
  decided and unbuilt), so Noto Sans Syriac, which is proportional, runs
  past its cells into its neighbours; Courier New's Arabic is 0.6 em, the
  cell's width, and fits.
- **A mark on a flipped glyph**: on a flipped base a mark's image is put
  where the flip puts its box, not where the flip puts its attachment
  point, so it is exact only for a mark centred on its attachment point.
- **A picture placed under a kitty placeholder picture** (a relative
  placement whose parent is virtual) is placed from the placeholders'
  stored column on a row drawn in another order.
- **A predicted echo on a double-width line** is not laid out as
  written: its line is laid out without it.
- **The row sliding out of a smooth scroll, at its edges**: a space
  carrying a combining mark there gets no stress mark (`SGR 64`), judged
  from the space alone, where on a still row the mark it carries gives it
  one; and a glyph on the window's first row whose ink reaches into the
  sliding row is not drawn in order with that row's text.
- **A composition at its edges**: on the cell path a letter after it
  that the glyph map draws lies under it where its ink meets the
  composition's, where written it lies over it; a selected
  composition's background lies under the glyphs of the row above that
  reach into its row on the cell path and over them on the others; a
  lam-alef pair across the composition's edge is drawn in the colours of
  the cell under the composed letter, not those its hints give it; a
  composition is drawn in the rendition of the cell it starts on, where
  its commit is written in the current one, so a form SAPV 5-8 gives is
  read from that cell; and with BDSM reset, once a joining letter has
  been drawn, the layout the cell path keeps reads no predicted echo
  (`Renderer::keep_written`), where with it set an echo is laid out as
  written.
- **Telling programs it is supported**: DECRQM 8 is the only probe; no
  terminfo entry says so (note AB Q18).

## Decided in step 1

**Unicode version (Q20): 17.0.0**, the version of every other UCD table
baud compiles, over pinning only the bidi tables at 18.0.0, or moving every table at once. Unicode 18.0.0 adds two brackets and five mirroring pairs.
The data in its two test files is the same, so a later move of every table
to 18.0.0 carries the bidi tables with it.
