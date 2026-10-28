# L. The scrollbar

The spec for the app's scrollbar, written before the code. No protocol
claim is in it; every line is a decision with its reason.

## What it is

An overlay indicator of where the window sits in the ring: a **thumb** on
the right edge, and nothing else -- no track, no arrows. It is visible
while the window is off the live screen -- the window by the core's clamp,
below, so never on the alternate screen -- and absent otherwise,
because the wheel is how a reader leaves the live screen and the thumb's
job is to say how far up they are and to let them move fast. That was the
first version; the platform's fade and hover, and the system's "Show scroll
bars" setting, are the sections at the end -- measured from AppKit rather
than recalled, because its headers document none of the numbers.

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
screen. ~~-- a gap that predates this and is recorded rather than closed.~~
The measure is still a renumbering across a reflow, and nothing reads it
there: a resize anchors the window with `Term::resize_holding` (`b0f4b89`,
note U), which is the "anchors by the primary alone through a read and a
resize" the tests below hold (corrected 2026-10-10).

**A switch is the core's epoch, not the screen at a read's two ends.** A
read that leaves the alternate screen and enters it again ends where it
began, on a surface the entry cleared and whose ring it reset -- the
hand-off from a file manager to an editor sends both at once. So
`Term::screen_epoch` moves on every switch, and a read whose two ends
carry different epochs switched: the selection is dropped, a slide is
settled, and the selection's line count takes no movement from it. A read
on one surface keeps the live movement, which is what numbers the lines
of the surface the user is selecting on.

## What AppKit's overlay scroller does, measured

`[[source: macos-sdk-nsscroller-h]]` says `preferredScrollerStyle` follows
the user's "Show scroll bars" setting and changes over time, posting
`NSPreferredScrollerStyleDidChangeNotification`; `[[source:
macos-sdk-nsscrollview-h]]` says `flashScrollers` shows the knobs
"momentarily". Neither gives a duration, a width or a colour.
`scripts/scroller-probe.swift` measures them on a scroll view of its own,
captured with ScreenCaptureKit at 1/120 s against each frame's
`displayTime`, triggering only in-process: nothing is posted to the
desktop. Six trials a case, the first discarded, median [range] of five, on
this Mac (Show scroll bars left at its default, Reduce Motion off, 2x); a
second run agreed with every row within its ranges:

| trigger | appears | last frame at 97% of the peak | first frame below it | gone |
|---|---|---|---|---|
| `flashScrollers()` | 28 ms | 1029 [1023-1035] | 1038 [1037-1044] | 1248 [1245-1251] |
| one wheel click | 28 ms | 524 [517-535] | 538 [533-546] | 748 [744-754] |
| a programmatic scroll | 27 ms | 532 [526-535] | 541 [535-544] | 751 [744-754] |
| the document grows, the view stays | -- | -- | -- | never revealed |

* **A scroll holds the knob about 0.50 s and fades it over about 0.25 s**;
  `flashScrollers` holds about a second. A programmatic scroll is a scroll.
  The hold ends where the fade's fit starts, 23 ms before the last frame at
  97% of the peak -- the frames between read 0.99 and are below the
  threshold rather than invisible -- and the fade then ends at 748 ms,
  which is the wheel's measured "gone" to the millisecond.
* **The document growing under a view that stays on its lines reveals
  nothing.** Its peak is 1.5% of a knob's ink, a transient that returns to
  the baseline after about a second in most trials. What draws it is not
  known -- the knob is at its 26 pt minimum length here, so growing the
  document cannot change it -- but it is not the knob shown. That is a
  terminal scrolled back while output arrives.
* **The fade is Core Animation's ease-in-ease-out.** As a fraction of the
  peak's ink, which is linear in the knob's alpha over a black document, a
  flash's 26 frames from the last near full read 0.95 at 10 ms past it,
  0.79 at 55, 0.47 at 102, 0.14 at 163 and 0.02 at 215. They are from the
  run whose frames all arrived; the second run's capture dropped some
  twenty frames mid-fade, and its shape is not used. Fitted with a free
  start and length, the `easeInEaseOut` Bezier (0.42, 0) (0.58, 1) starting
  23 ms before that frame and lasting 247 ms is within 0.020 of every one
  (RMS 0.014); a smoothstep is within 0.027 at best (12 ms, 226 ms, RMS
  0.018), and a linear fade 0.05. The better fit is the curve one would
  expect an AppKit animation to use, and baud takes it.
* **The knob at rest is 6 pt wide, 3 pt in from the edge**: white at 0.55
  alpha in the dark appearance, black at 0.50 in the light.
* **A legacy scroller is an 11 pt band, 3 pt in**: a track of the knob's
  colour at 0.047 (12 of 255), and the knob over it reading 0.56 and 0.51
  as a composite -- 0.53 and 0.49 of its own. It is tiled beside the
  content: a 240 pt frame's content is 223 pt under it, and the full 240
  under an overlay scroller, whose 17 pt frame lies over the content's
  edge.
* **AppKit fades the knob and the track separately.**
  `[[source: macos-sdk-nsscroller-h]]` line 55 requires a custom scroller
  to draw through `drawKnob` and `drawKnobSlotInRect:highlight:` "to allow
  for separate knob and track fade in/out", AppKit applying the fade alpha
  to each. The track's own fade is not measured: the probe cannot hover.

**What the probe could not drive, stated rather than guessed.** A phased
trackpad gesture built in-process revealed the knob once and never again,
so it is no measurement. Hover cannot be sent in-process -- neither view
exposes a tracking area -- and a real move would take the user's cursor,
so the hovered knob's geometry is **the legacy scroller's, assumed**: the
headers document no hovered geometry, the 17 pt frame is the only width
they support, and that an overlay knob widens to the legacy band is an
assumption from how the two look, not a measurement. The hover-exit delay
and the track's fade are unmeasured; the exit is taken as a scroll's hold,
and the track appears and goes with the hover at once.

## Fade and hover (decided 2026-09-23)

Follow the platform: the thumb fades to nothing, the timings are the
measured ones, and the pointer widens it and shows a track. Two calls
narrow the platform for a terminal: under **Always** the scroller takes its
own column, and hovering reveals the thumb **only while scrolled back**, so
at the live screen the rightmost columns stay the terminal's -- its clicks,
its selection, and a tracking program's reports.

**The thumb exists wherever there is history; whether it is seen is an
alpha.** Its geometry is the geometry above at any `back`, the live screen
included, and the app hands the renderer an alpha and a hover state; the
renderer interpolates nothing. The app's state is one of four, driven off
the display link's `tick_target` as blink and the trail are:

* **hidden** -- alpha 0;
* **held** until a time -- alpha 1;
* **fading** from a time -- alpha falls along the ease-in-ease-out curve
  over 0.25 s;
* **hovered** -- alpha 1 for as long as the pointer is over the strip while
  scrolled back, or a drag is in progress.

A **reveal** -- a wheel that moved the window, a jump, the end of a drag,
the pointer leaving the strip -- holds for 0.50 s and then fades. The
wheel that brings the window to the live screen reveals the thumb there, at
the bottom of its travel, and it fades like any other; so does a key or a
paste snapping the window to the live screen, which is a programmatic
scroll, and AppKit reveals those. **Output arriving
while scrolled back reveals nothing**, which is what AppKit measured, and
neither does any output at the live screen.

**Hovered, the thumb widens and a track appears**: the knob 11 pt wide,
the track the same band down the whole height at 12 of 255 of the theme's
foreground, both 3 pt in; at rest the knob is 6 pt, 3 pt in. The knob stays
the theme's foreground at half alpha, which the measured 0.50-0.56 agrees
with. Widths are points times the backing scale.

**The strip is the overlay scroller's frame, 17 pt from the grid's right
edge.** The pointer over it while scrolled back hovers, and the hover is
re-evaluated on every tick from where the pointer last was, so the window
reaching the live screen under a still pointer -- a wheel, a key, a paste,
a jump -- ends the hover there, and leaving it takes the hover up. A press
in it is the scrollbar's when the last frame drew the thumb -- alpha above
zero, in the geometry that frame drew -- and on the knob it drags, off it
it jumps. With the thumb gone it is cells again, so at the live screen a
press there reaches the terminal once the fade that followed a scroll is
over. A drag in progress when the window can no longer scroll -- the
alternate screen coming up mid-drag -- ends, rather than write the
primary's window from a geometry with nothing behind it.

**Damage covers the alpha, a row at a time.** A row is dirty when what
the bar draws inside its band changed -- the knob and the track clipped to
it, and the alpha. So a fade dirties the rows the knob covers each frame, a
hover's start and end dirty every row once, because the track is the whole
height, and a knob that output moves under a hover dirties only the rows it
entered or left: the rows the track alone covers are the same before and
after.

**Idle builds nothing.** Held, hovered and hidden are constant: the tick
forces a frame only while fading, about 30 frames at 120 Hz, and once
hidden it forces none. A test counts the forced frames over a timeline.

## Always: a column of its own

Under the legacy style -- "Show scroll bars: Always", or Automatically with
a mouse -- the scroller is tiled beside the grid, as AppKit tiles it: the
grid is laid out in the view less a 17 pt gutter on the right, and the
gutter holds an 11 pt track, 3 pt in, always drawn, with the knob in it
whenever there is history. No fade and no hover. The style is read from
`NSScroller::preferredScrollerStyle` when the window is laid out and when
a wheel would reveal the thumb; a change of the setting applies at the
next of those, re-laying the grid, which the shell sees as a resize.

**The gutter is inside the renderer's frame, not in the margin.** The
Metal paths set the rasteriser viewport to the frame they are handed, so a
knob drawn in the letterbox margin would be clipped. So the renderer holds
a gutter width beside the grid, `pixel_size` stays the grid's and a second
accessor gives the frame, grid and gutter. Everything the bar is drawn and
hit-tested against reads the frame: the bar's geometry, the press's strip,
the letterbox, the viewport uniforms, the CPU frame buffer. Everything
about cells reads the grid: the cells, the screen fade, the images. Draw
and hit-test read the same size, or the knob draws in the gutter while a
press lands on the last column.

**The gutter's background is the margin's**, the default background at the
window's dim, so the gutter reads as the edge of the window and not as a
column of cells. The CPU compositor fills a dirty row's pixels past its last
cell with that colour; the instance path's background pass is scissored to
the grid, so the gutter keeps the pass's clear colour; the cell path draws
no cell there.

**The gutter is never cells.** A press in it is the scrollbar's, whatever
the button: on the knob a drag, elsewhere with history a jump, and with
nothing to scroll -- no history, or the alternate screen -- it does nothing.
A press the scrollbar takes keeps its drags and its release, so neither the
selection nor a tracking program sees a release without its press. Motion
over the gutter with nothing pressed is taken too, so a program tracking
every motion is not told the pointer is on the last column; a drag that
began in the grid is the grid's, and reaches it clamped. That
rule is the overlay strip's too: a jump there used to swallow its press and
let the release through to the cells.

**The look is constant under legacy**, so the display link forces no frame
for it and a pointer over the gutter hovers nothing: the knob is 11 pt at
the knob's alpha, the track 11 pt at the track's, and a wheel reveals
nothing because nothing is hidden.

**The style is read at a layout and a wheel, and not at launch.** The
shell is spawned before AppKit starts, at a grid computed without a gutter
-- the order is a measured cold-boot win and asking AppKit for the style
would move its initialisation in front of it. So under legacy the first
layout takes the gutter's columns away and the shell gets one resize while
it is still reading its rc files. A wheel that finds the style changed
re-lays the grid first and then scrolls, so the step lands in the new
geometry. A key's snap to the live screen is a reveal too and does not
read it: re-laying the grid there would clear a keystroke's own local
echo prediction. `NSPreferredScrollerStyleDidChangeNotification` is not
observed: a changed setting shows at the next resize or wheel.

Under a fixed grid only the launch sizes the window for the gutter, so a
style that changes mid-session clips the gutter off the right edge; normal
sizing re-lays the grid instead.

`BAUD_SCROLLER=overlay|legacy` overrides the system's answer, for a check
that must not change the user's settings; it is an instrument, like
`BAUD_GPU`, and not a configuration.

## Tests

The geometry is a pure function with tests: no thumb with no history, and
at the live screen at the bottom of its travel; at the top when fully
scrolled back; the minimum length on a deep history; the knob widening and
the track spanning the height under the pointer; a bar seen not at all
drawing nothing and a fade scaling both alphas; the strip as the overlay
scroller's frame; the inverse landing where the forward mapping put it.

The legacy style has tests of its own: the bar is a track always and a wide
knob with history, whatever the look; the look never changes through a
reveal, a fade or a hover; the gutter is never cells, to a press or to
motion, and a press the scrollbar takes keeps its release; the grid gives up the gutter's columns
and the frame fits the view; the CPU gutter is the margin's colour, not the
last cell's, with the track on every row and no knob without history; and
a gutter that appears repaints every row.

The reveal is a pure state machine with tests: a hold then the measured
fade to nothing; a reveal during a fade bringing it back; the pointer
over the strip widening and holding until it leaves; the look changing
only while it fades, which is the idle rule; the strip hovering only while
scrolled back and taking a press only while seen; the widths as AppKit's
points at the backing scale.

The pixel oracle draws the thumb through both GPU arms held, half faded
and hovered with its track, and requires them equal, each different from
the frame without it and from each other. It draws the legacy scroller in
its gutter too, and there the arms agreeing is not enough -- both could
lose the gutter together -- so it reads the pixels: on every row the
gutter's margin columns are what they are with no bar, its track columns
are not, and the grid's pixels are untouched. A thumb faded to nothing draws
nothing by construction -- its solids have no alpha -- so no pixel can
gate it; the geometry test holds that no bar is built at all. The CPU
path, which the oracle cannot see, has three tests of its own: an
incremental frame after a fade or a hover equals a fresh full frame, a
hover changes pixels only in the widened band, down every row, and a knob
that moves under a hover repaints only the rows it touched.

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
