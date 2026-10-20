# K. Selection, copy and paste

The spec for the app's selection model, written before the code. Nothing
here is a protocol claim except the two the sources settle: what bracketed
paste sends, and that Shift overrides an application's mouse tracking.
Everything else is a decision, stated with its reason so it can be
re-decided rather than rediscovered.

## What it is

A selection is a range of cells over the **live surface's lines**, made
with the mouse and read back as text: a **stream** -- contiguous, in
reading order -- or, with Option held at the press, a **block** of whole
columns (see "Rectangular selection" below). Character, word and line
granularity; Cmd-C copies it to the pasteboard, Cmd-V pastes the
pasteboard into the child. The three render paths draw it as reverse
video.

Not in this slice, each for a stated reason:

* **Copy on select, middle-click paste.** X conventions; macOS has neither.
* **A selection colour.** Reverse video needs no theme entry and is what
  the block cursor already does; a colour under glass wants the luma
  measurement the register still owes (D14).
* **The Linux driver.** The model is platform-free and the Wayland window
  delivers no mouse events yet; it is type-checked and not run, like the
  rest of that driver.

## Where the endpoints live

**An endpoint is an absolute line of the live surface plus a column.** The
app keeps a count of the rotation's advances -- the same count the
scrollback window already reads in `pump` to stay on its content while
output scrolls -- and a cell at screen row `y` in a view `back` lines up
is at absolute line `scrolled - back + y`. So a selection:

* survives output scrolling underneath it, because the line it names moves
  up the ring and the window follows;
* is drawn in whatever view is up, by converting its lines back to screen
  rows of that view and clipping to the screen;
* is **clipped** when the ring recycles a line it names -- the oldest
  retained line is `scrolled - history`, and an endpoint older than that
  moves to it; a selection both of whose endpoints are gone is cleared;
* is **cleared** on a surface switch (`?1049` in or out: the other surface
  is a different ring and keeps no history) and on a change of columns (a
  reflow re-cuts every line, so the columns it named no longer name cells);
* is **kept** across a change of height alone on the primary, the count
  moved by the rows the resize retired or took back, and cleared on the
  alternate screen and where it reaches a row the shrink cut (note V, V3,
  since 2026-10-20; every resize cleared it before).
* **moves with its text** when a program scrolls rows under it without
  moving the ring -- a region's scroll, `IL`, `DL` -- and stays on its text
  under a top region's scroll, losing only the lines a scroll takes out of
  its region, pushes under the screen or sends off the alternate screen's
  top, as xterm does with `keepSelection` at its default (the core's watch,
  `Term::watched_lines`; Jev's decisions of 2026-10-20, where each of these
  dropped it before). A write or an erase on it, `IL` at a row in it, `DL`
  of a row in it, and a scroll under side margins still clear it.

The alternative -- keying on physical rows and a generation -- is what the
position-keyed marks table was, and CLAUDE.md records what that cost. An
absolute line is a number the app already maintains.

## Membership and expansion

Two endpoints `anchor` and `head`; the range is from the earlier to the
later in row-major order, both ends **inclusive** of the cell they name.
A `WIDE_TAIL` selects with its lead: a range that starts on a tail starts
on the lead, one that ends on a lead ends on its tail, so a wide character
is never half-selected.

* **Press** sets both endpoints to the cell under the pointer; **drag**
  moves the head; **release** ends the gesture and leaves the selection.
* **Double click** selects the **word** under the pointer: the maximal run
  of cells on that row that are not `EMPTY`, not whitespace, and not in
  the delimiter set `` ,│`|:"' ()[]{}<>`` plus a tab. A drag after a double
  click extends by whole words at the head.
* **Triple click** selects the **line**: the row and every row the wrap
  flag joins to it, before and after. A drag after a triple click extends
  by whole lines.
* **Shift-click** moves the head and keeps the anchor.
* **A press with no drag** on a single click clears the selection.

The delimiter set is alacritty's default for the same purpose; it is a
decision, not a source, and it is one constant.

## Rectangular selection

**Option at the press makes the selection a block**: every row from the
earlier endpoint's line to the later's, and on each of them the same range
of columns. It is Terminal.app's gesture; alacritty takes Control and
iTerm2 Cmd-Option, and none of them is a source -- this is a decision.

* **An Option press always starts a new block**, even with Shift held.
  Under mouse tracking Shift is the only way into a selection at all, so
  Shift+Option is how a block is started there; were Shift to win, an
  existing selection would be extended instead and a block could not be
  started while one was on screen.
* **Shift without Option extends whatever is there and keeps its shape**:
  the shape belongs to the gesture that made the selection, not to the
  modifiers of the click that moves its head.
* **Grains apply as they do to a stream.** A double click with Option is a
  block over the two words' columns, a triple click one over whole rows.

**The column range is taken apart from the line range.** A stream orders
its two endpoints row-major, so the earlier point's column is where it
starts; a block's left edge is the smaller of the two spans' start
columns and its right edge the larger of their end columns, whatever the
lines. A drag up and to the right, or down and to the left, is the case
the row-major order gets backwards.

**The corners are snapped, both of them.** A press or a drag's head on a
wide character takes both its columns, as in a stream, so the rectangle
drawn encloses the characters at its corners -- and on every *other* row
the block is that wide too, one column past where the pointer is on the
narrow text beside it. The alternative, snapping neither end and letting
the rule below keep characters whole, would draw a block one column wide
on ASCII rows and two on the CJK row the user pressed, which reads as a
ragged edge rather than a rectangle.

**A wide tail is selected exactly when its lead is.** On the corner rows
this is already so; on any other row a block's edge can cut a wide
character in two, and a character half-selected is drawn with half its
glyph over the swapped background and half not. So on every path -- the
CPU compositor, the instance path, the cell shader and the overlay's
records -- a `WIDE_TAIL` cell takes the selection of the cell before it.
A character straddling the left edge is out, and one straddling the right
edge is in whole. The consequence worth having is that two blocks side by
side partition the characters: each belongs to the block its lead is in.

**The rule applies to a stream too, and there it is a behaviour change.**
A stream's endpoints are snapped when they are made, so a tail and its
lead always agreed -- until output overwrote a row under a live
selection. Then a start that now lands on a tail stopped drawing that
tail's lead and still drew the tail, and an end that now lands on a lead
drew the lead and not its tail. Under the rule the drawing matches the
text, which already read a tail as nothing and a lead as its character.

**Clipping keeps the columns.** When the ring recycles a line a block
names, the endpoint moves to the oldest retained line *in the same
column*, where a stream's moves to that line's first column.

## The text

Rows are walked in order; within a row, cells from the range's first
column to its last:

* a `WIDE_TAIL` contributes nothing, its lead already did;
* an `EMPTY` cell *inside* the row's selected content -- before the last
  non-empty selected cell on that row -- is a space, because a gap an
  application left by moving the cursor reads as a space on the screen;
* `EMPTY` cells after the last non-empty selected cell are dropped, which
  is the rule `Content::EMPTY` exists for: a blank never written is not a
  trailing space the user typed;
* a cell that carries marks contributes its base and then its marks, in
  order, through `Term::cluster_marks`;
* a row whose wrap flag says `Wrap::Continues` joins the next with **no
  newline**, because the writer wrapped it and the text is one line; any
  other row boundary is `\n`; there is no newline after the last row.

A **block** reads each of its rows over its column range alone, by the same
cell rules -- a tail is nothing, a gap is a space, the blanks never written
after a row's last selected content are dropped -- and **every** row
boundary is `\n`, the wrap flag's included: a block is a shape on the
screen, and the rows a writer wrapped are cut by it like any other.

## Mouse tracking, and Shift

When the child has any mouse tracking on, a mouse event is the child's
and not a selection's -- except with **Shift held**, which is xterm's
default (`[[source: ctlseqs]]`, XTSHIFTESCAPE: "Ps = 0 -> allow shift-key
to override mouse protocol", "the default, which is 0"). With Shift the
event goes to selection and the child sees nothing; the report's Shift
bit is therefore never sent while tracking is on, which is what xterm does
under the same default. Since 2026-10-19 the control is built:
`CSI > 1 s` hands Shift only the left button, the one the selection binds,
and sends a Shift middle or right press, and the Shift wheel, to the child
with its Shift bit; at 0 the Shift wheel is the screen's too (the history,
or the cursor keys under `?1007`), as xterm's `ShiftOverride` takes it.

## Copy and paste

**Copy** writes the text as `NSPasteboardTypeString` on the general
pasteboard, replacing what was there. Cmd-C with no selection does
nothing, and is not sent to the child: Cmd is not a terminal modifier.

**Paste** reads the general pasteboard's string, if any, and writes it to
the child through the same channel keys take:

* with `?2004` set, bracketed as `[[source: ctlseqs]]` specifies -- `ESC [
  2 0 0 ~`, the text, `ESC [ 2 0 1 ~` -- with any `ESC [ 2 0 1 ~` inside the
  text removed first, because the child would take it as the end;
* without it, `\r\n` and `\n` become `\r`, which is what a line ending is
  to a line discipline reading typed input.

## Drawing

One rule on every path: a selected cell's resolved foreground and
background swap, before the block cursor's own swap, so the cursor inside
a selection is still the cursor. The renderer holds the current highlight
as state the driver sets before a frame (the shape `set_blink` has), in
the view's screen coordinates:

* the CPU and instance paths apply it in `convert_row` beside the cursor
  swap, and a row is dirty when the highlight it carried last frame differs
  from the one it carries now -- the cursor's own damage rule;
* the cell shader takes it as two `uint2` and a flag in the uniform and
  applies it in `colors()`.

The gate is the pixel oracle: the scene the two GPU arms are compared on
gains a selection, the arms must agree pixel for pixel, and moving the
selection must change the pixels.

## Tests

* the model, pure: normalisation, the wide-tail rule at both ends, word and
  line expansion, clipping when the ring recycles a line; for a block, all
  four drag directions, the corner snap widening a row that is not a
  corner's, and a clip that keeps the column;
* a block's text against a `Term`: a wrapped row cut by a newline, a wide
  character straddling each edge, the per-row trailing blank;
* extraction against a `Term`: a wide character, a wrapped line, a gap, a
  trailing blank, a cell with marks;
* the paste transform: bracketed with a forged end inside, and the newline
  rule without;
* the pixel oracle, extended, with a block whose edges cut a wide
  character on each side;
* the tail rule on the CPU path alone, since the oracle compares two arms
  and cannot see both lose it.
