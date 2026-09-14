# L. The scrollbar

The spec for the app's scrollbar, written before the code. No protocol
claim is in it; every line is a decision with its reason.

## What it is

An overlay indicator of where the window sits in the ring: a **thumb** on
the right edge, and nothing else -- no track, no arrows. It is visible
while the window is off the live screen (`back > 0`) and absent otherwise,
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

## Tests

The geometry is a pure function with tests: no thumb on the live screen or
with no history; at the top when fully scrolled back; the minimum length
on a deep history; the inverse landing where the forward mapping put it.
The pixel oracle draws the thumb through both GPU arms and requires them
equal and different from the frame without it.
