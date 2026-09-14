# K. Selection, copy and paste

The spec for the app's selection model, written before the code. Nothing
here is a protocol claim except the two the sources settle: what bracketed
paste sends, and that Shift overrides an application's mouse tracking.
Everything else is a decision, stated with its reason so it can be
re-decided rather than rediscovered.

## What it is

A selection is a contiguous range of cells over the **live surface's
lines**, in reading order, made with the mouse and read back as text.
Character, word and line granularity; Cmd-C copies it to the pasteboard,
Cmd-V pastes the pasteboard into the child. The three render paths draw it
as reverse video.

Not in this slice, each for a stated reason:

* **Rectangular selection** (Option-drag). A second membership rule and a
  second extraction order; wanted, and separable.
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
  is a different ring and keeps no history) and on a resize (a reflow
  re-cuts every line, so the columns it named no longer name cells).

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

## Mouse tracking, and Shift

When the child has any mouse tracking on, a mouse event is the child's
and not a selection's -- except with **Shift held**, which is xterm's
default (`[[source: ctlseqs]]`, XTSHIFTESCAPE: "Ps = 0 -> allow shift-key
to override mouse protocol", "the default, which is 0"). With Shift the
event goes to selection and the child sees nothing; the report's Shift
bit is therefore never sent while tracking is on, which is what xterm does
under the same default. The `CSI > Ps s` control itself is not
implemented in this slice; the default is the behaviour.

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
  line expansion, clipping when the ring recycles a line;
* extraction against a `Term`: a wide character, a wrapped line, a gap, a
  trailing blank, a cell with marks;
* the paste transform: bracketed with a forged end inside, and the newline
  rule without;
* the pixel oracle, extended.
