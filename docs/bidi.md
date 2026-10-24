# Right-to-left text: what is built, and what is next

**Hebrew and Arabic lines are drawn right to left once a program turns
bidirectional mode on** (`CSI 8 h`, ECMA-48's BDSM in its IMPLICIT state).
A Hebrew word in an English line reads right to left in its place, brackets
around Hebrew face the right way, vowel points stay on their letters, and
the cursor, the selection, the find marks, the link underline and the mouse
all follow what is drawn. A copy gives the text in the order the program
wrote it. baud still starts with the mode off, drawing every line left to
right as before: whether it should start on is one of three questions left
for the owner (below).

This is step 2 of right-to-left text. Step 1 built the Unicode
Bidirectional Algorithm; step 2 puts its order on the screen.

## What you see with the mode on

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
- **Every paragraph is at level 0** (left to right, left-aligned) until a
  program can set a direction (SCP, next step) or the owner chooses
  first-strong detection.
- **The bidi controls work.** LRM, RLM, ALM, the embeddings, overrides and
  isolates that baud stores on the cell before them are given to the
  algorithm with that cell's character. One at the start of a line, with no
  cell before it to be stored on, is still dropped and counted.
- **Brackets and other mirrored characters** resolved right to left draw
  their Bidi_Mirroring_Glyph (L4), an RLM or LRM typed after a bracket
  included. Two kinds still draw as they are, counted in
  `Renderer::bidi_unmirrored`: the 126 mirrored characters with no such
  glyph (N-ary sums, roots, integrals and the like), and a mirrored
  character carrying a mark that draws, such as `<` followed by U+0338
  (text in composed form writes U+226E, which is mirrored). Both are open
  departures from L4 (below).
- **A cell moves whole**: a letter with its points, a wide character with
  its right half, kept on its left.
- **An erased cell between characters** moves with the character before it;
  erased cells after a line's text stay at the line's end.
- **No ligature or contextual form crosses from one direction run to
  another**, and the cells of a right-to-left run are shaped one at a time
  rather than in their drawn order (UAX #9 3.5).
- **The cursor** is drawn over the cell it is on, wherever that cell is
  drawn; a bar cursor sits on the right side of a right-to-left letter.
- **The selection** is a range of the text as written: its highlight falls
  on each selected cell wherever it is drawn, so a selection across a
  Hebrew word can show as two runs, and a copy gives the written order. A
  click, a drag and a mouse report to a program name the cell the program
  wrote under the pointer (the inverse of the drawn order).
- **The alternate screen and the scrollback** are drawn the same way.
- **Arabic** is reordered like Hebrew and drawn in the isolated letter forms
  it draws in today: joining is a separate item, not built.

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

**Taken from Jev's lean, which held in all four answers but for the
erased cells, where the first ask's SKIPPED (0.10 / 0.18 against a space)
was re-offered as AS_BN, the same behaviour named after its class; the
owner can veto each:**

| choice | taken | Jev, first ask then re-ask |
|---|---|---|
| where BDSM lives | terminal-wide, as an ECMA-48 mode; per-paragraph state is the next step | 0.86 / 0.82, 0.72 / 0.76 |
| an erased cell between characters | a boundary neutral, moved with the character before it | (SKIPPED) 0.10 / 0.18, (AS_BN) 0.85 / 0.89 |
| the alternate screen | drawn like the primary ("The two screens should behave identically") | 0.58 / 0.34, 0.79 / 0.76 |

**Open departures from UAX #9 L4, which Jev leans to build.** The 126
mirrored characters with no mirroring glyph, and a mirrored character
carrying a mark that draws, are drawn unmirrored and counted. The first
two asks put only the first case, as 128 characters, and on the premise
that every character with a mirroring glyph draws it, which the build did
not then meet for a cell carrying an RLM; their answers (counted, 0.88 /
0.92 and 0.86 / 0.92) are superseded. Asked with the true counts, and
again with the prior art (VTE swaps a cell's base and keeps its marks;
HarfBuzz substitutes or applies the font's `rtlm`, which baud's faces do
not carry; no terminal surveyed flips a glyph), Jev leans to build both: a
flipped glyph 0.58 / 0.50 then 0.70 / 0.66, a substitute base under the
marks 0.62 / 0.54 then 0.72 / 0.66. That lean is to build both; this step
built only the bidi-control case, so both stay open departures until they
are built (`docs/research/notes/ab/step2/l4.png` shows both).

## Open: three questions for the owner

The renders are in `docs/research/notes/ab/step2/` (`mode.png`,
`cursor.png`). Each is built as today's behaviour until the owner answers.

1. **Should baud start with right-to-left lines drawn right to left?**
   Built as off (EXPLICIT), today's drawing and the state ECMA-48 7.1
   recommends as the initial one. Jev leaned to off in all four answers
   (0.82 / 0.82, then 0.51 / 0.37), so it is taken as the other leans are;
   it is asked all the same because the owner's own words point the other
   way ("you drew it ltr not rtl which is how hebtrew is written", "RTL
   support is a MUST"), and an owner's call outranks a lean. On (IMPLICIT)
   is terminal-wg's "must be the default", and what VTE, iTerm2, mintty,
   Konsole and mlterm do; no program or terminfo entry read sends
   `CSI 8 h`, so with it off nobody sees right-to-left lines unless they
   turn it on.
The other two are asked because Jev's lean flipped between the asks, or
split between the orders:

2. **Which way does a paragraph run when nothing says?** Built as left to
   right (level 0, terminal-wg's default). The other choice is UAX #9's own
   rule, the first strong letter, which right-aligns a Hebrew paragraph and
   flips a whole box-drawn row that starts with Hebrew. Jev: first strong
   0.56 / 0.48, then left to right 0.22 / 0.30.
3. **Where is the cursor when a reordered line is full?** Built as drawn
   over the last character written, which on a reordered line can be
   mid-line. terminal-wg prefers hiding it there. Jev: 0.28 hidden / 0.73
   drawn, then 0.52 hidden / 0.30 drawn.

## What it costs

Measured with the `frame` bench's convert column (`FRAME_BIDI=implicit`
sets the mode before a recording), base 9dd37f0f against this change
built the same way from `git archive`, an A/A copy, four arms rotated over
eight rounds with the first dropped, on mains (the docs/record.md entry,
"Right-to-left lines are drawn in their visual order", has the table):

- **With the mode off, as baud starts: no difference.** The layout returns
  at once; every recording's convert range overlaps base's.
- **With it on, recordings with almost no right-to-left character**
  (btop has none; cat-scroll one Arabic letter): ranges overlap. A drawn
  row is tested only when it changes, by one load a cell from a table
  compiled from the class data; an unchanged row costs a byte compare. Rows
  of a paragraph that runs past the window's edge, above it or below it,
  are tested on every frame, with no cache and no measurement: whenever the
  window's first or last row continues a soft-wrapped paragraph, up to
  `baud_render::CONTEXT` / columns rows (92 at 200 columns) are read cell
  by cell each frame.
- **With it on, recordings that hold Hebrew and Arabic** pay for laying
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
- **Two rules scan more than once.** N0 scans every bracket pair's
  contents, and the text before it for context, so it is quadratic in the
  worst case. X5c scans ahead from every FSI to its matching PDI. Section
  5.1 names bracket matching as a place to optimise. Neither is measured
  on a row of nested brackets yet.

## What is not built yet

- **A direction a program sets**: SCP (per line in ECMA-48), SPD, and
  terminal-wg's per-paragraph modes with `?2501` (autodetection), `?1243`
  (arrow-key swapping) and `?2500` (box mirroring). They need a direction
  each row keeps. Until then they are counted and `DECRQM` says they are
  not recognised (`docs/vt-coverage.md`).
- SRS, SDS, SAPV and DCSM; DEC's DECRLM, DECRLCM and DECHEM.
- **Arabic joining** (its own item); L4's two open departures, a flipped
  glyph for the 126 mirrored characters with no mirroring glyph and a
  mirrored base under a mark that draws.
- **Drawn in their stored order still**: the status line, the row leaving
  the window during a smooth scroll, images placed on a reordered row, and
  an input method's composition and a predicted echo on one.
- **Telling programs it is supported**: DECRQM 8 is the only probe; no
  terminfo entry says so (note AB Q18).

## Decided in step 1

**Unicode version (Q20): 17.0.0**, the version of every other UCD table
baud compiles. Jev chose it over pinning only the bidi tables at 18.0.0, or
moving every table at once, with confidence 0.95 and 0.98 in the two orders
it was asked. Unicode 18.0.0 adds two brackets and five mirroring pairs.
The data in its two test files is the same, so a later move of every table
to 18.0.0 carries the bidi tables with it.
