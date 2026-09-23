# L. The scrollbar

The spec for the app's scrollbar, written before the code. No protocol
claim is in it; every line is a decision with its reason.

## What it is

An overlay indicator of where the window sits in the ring: a **thumb** on
the right edge, and nothing else -- no track, no arrows. It is visible
while the window is off the live screen -- the window by the core's clamp,
below, so never on the alternate screen -- and absent otherwise,
because the wheel is how a reader leaves the live screen and the thumb's
job is to say how far up they are and to let them move fast. That is the
convention of overlay scrollbars on this platform, without the hover and
the fade, which are a follow-on.

## Geometry

Over a screen `h` pixels high with `rows` rows and `retained` lines of
history behind it:

* the thumb's **length** is the screen's share of the ring, `h * rows /
  (retained + rows)`, and never less than two cells, so a deep history
  still leaves something to grab;
* its **top** is the window's position over its travel: `(h - length) *
  (retained - back) / retained`, so at `back == retained` it is at the top
  and one line above the live screen it is nearly at the bottom;
* its **width** is half a cell, at least four pixels, inset two pixels
  from the right edge.

It is drawn in the theme's foreground at half alpha, as a solid instance
-- the kind underlines and cursors already are -- so all three paths draw
it from one function: the CPU compositor and the instance path through
the renderer's instance list, the cell path through the record list drawn
after the cell passes.

## Damage

The thumb moves when the window moves or history arrives under it. A row
the thumb covered last frame or covers now is dirty, the cursor's and the
selection's rule, so the CPU path repaints those rows and the instance
path re-emits them; the cell path redraws everything every frame anyway.

## Interaction

The thumb's strip -- its width, the screen's full height -- takes a press
before the selection and before the child's tracking, while the thumb is
visible:

* a press **on the thumb** starts a drag: the thumb's top follows the
  pointer by the distance moved, and the window follows the thumb through
  the inverse of the geometry above;
* a press **off the thumb** in the strip jumps: the thumb centres on the
  pointer and the window follows;
* release ends the drag. The wheel is unchanged.

A pointer at the strip while the thumb is absent is a pointer over cells.

## The window belongs to the primary screen

The alternate screen keeps no history, so it has no window over one: while
it is up, what the user looks through is the live alternate screen, and
**every reader asks the core's one clamp** -- `Term::looking_back`, the
function `Term::view` already applies -- rather than reading the window's
count as it stands. That is the thumb, the cursor, a selection's press, its
highlight and the oldest line it may name, the link under a Cmd-click, a
copy, and the CPU path's frame. Read raw, a window left scrolled back when a
program enters the alternate screen draws a thumb over a screen with
nothing behind it, hides the cursor of the program that now owns the
screen, and copies the live screen's top row once for every line the
window claims to be up; and the history count, which is still the
primary's, kept a selection scrolled off the alternate screen's top alive
to be copied as bare newlines.

**The primary keeps its window across the excursion.** foot holds a view
and an offset per grid (`[[source: foot-vt100-0]]`, terminal.c 2179-2180);
`?1049` swaps the live grid and resets neither (`[[source: foot-csi]]`,
497-538); a handled key calls `term_reset_view` (`[[source: foot-input]]`,
1732-1735), which resets the *live* grid's view (terminal.c 3215-3222); and
foot's own scrollback bindings act only on the normal grid (input.c
99-111). So baud does the same three things: entering the alternate screen
leaves the window's count alone, a key typed there does not snap it --
the live screen is already what is drawn -- and leaving puts the reader
back on the lines they were reading. A key on the primary snaps it to the
live screen as before.

**It is anchored to the primary's window line, not the live rotation.** A
scrolled-back window follows its lines up the history as output retires
rows beneath it, and the rows it follows are the primary's. The live
surface's rotation says nothing about them: while the alternate screen
scrolls, the primary has not moved, and across a read that switches
screens the two rotations belong to different rings, so their difference
is not a movement of anything. `Term::history_mark` notes the absolute
line at the primary window's top whatever screen is up, and
`Term::history_moved_since` measures from it -- a count rather than a
rotation, so a flood that retires more than half the ring in one read is
still that many rows; and a separate type from the live `RowRot`, so the
one cannot be passed where the other is meant. A reflow renumbers that
line with the rows it re-cuts, so across a change of width the measure is
the renumbering of the window's top, not the rows between it and the live
screen -- a gap that predates this and is recorded rather than closed.

**A switch is the core's epoch, not the screen at a read's two ends.** A
read that leaves the alternate screen and enters it again ends where it
began, on a surface the entry cleared and whose ring it reset -- the
hand-off from a file manager to an editor sends both at once. So
`Term::screen_epoch` moves on every switch, and a read whose two ends
carry different epochs switched: the selection is dropped, a slide is
settled, and the selection's line count takes no movement from it. A read
on one surface keeps the live movement, which is what numbers the lines
of the surface the user is selecting on.

## Tests

The geometry is a pure function with tests: no thumb on the live screen or
with no history; at the top when fully scrolled back; the minimum length
on a deep history; the inverse landing where the forward mapping put it.
The pixel oracle draws the thumb through both GPU arms and requires them
equal and different from the frame without it.

The window's half: the core's clamp is the history on the primary and
nothing on the alternate screen, beside a history count that still reports
the primary's; the history mark measures only the primary's movement --
nothing while the alternate screen scrolls, the rows retired before a
switch while it is up, both sides of an excursion, and a flood past the
whole ring; the screen epoch moves on every switch and nothing else; the
app's window is the live screen there, with no thumb and a cursor, anchors
by the primary alone through a read and a resize, keeps its count through
a key typed there, clips a selection at the alternate screen's top, and
sees a leave-and-re-enter as a switch; the renderer draws the cursor there
whatever window it is handed, and the CPU path paints the window it is
handed on the primary; a selection over lines the alternate screen cannot
reach copies none of them; and the driver's calls, which are AppKit-bound,
are held by a gate over the source that names every raw read of the
window.
