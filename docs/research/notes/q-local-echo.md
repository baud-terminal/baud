# Q. Local echo prediction: what row 9 draws before the pty answers, and what judges it

Row 9 of note N: "draw a key's echo before the pty round trip (local echo
prediction, Mosh-style)". Written before the code, as the rule here is.

## The number it is after

Row 0's instrument (`docs/research/notes/o-keystroke-latency.md`) splits a
keystroke through the login shell into legs, and the only leg that
separates the shell from the tty's own echo is `child`: **2.55 ms** for
zsh's turn against 15 us for the line discipline, on a `to_shown` of 5.4 ms.
Everything the app itself does is ~0.5 ms of that; the rest is delivery
(1.7 ms) and the GPU's round trip (0.6). So the ceiling on this row is the
shell's turn, ~2.5 ms of the 5.4, against an input floor of 1 ms. On the
panel the gain is quantised by the compositor's pass (note O): a frame
committed 2.5 ms earlier makes an earlier refresh on the keys whose echo
would have missed it, roughly the 2.5 of every 8.3 ms of phase, and gains
nothing on the others.

## What Mosh does, read rather than remembered

`[[source: mosh-terminaloverlay-cc]]`, `[[source: mosh-terminaloverlay-h]]`,
`[[source: mosh-stmclient-cc]]` (1.4.0) and `[[source: mosh-paper]]` section
3.2:

* **Only a width-one printable is predicted as a glyph** (cc:746: a byte
  under 0x20 or of another width only bumps the epoch). Backspace (0x7f)
  and the left/right arrows move the predicted cursor with no new epoch
  (cc:696-740, cc:843-855); **CR moves it and bumps the epoch as well**
  (cc:832-834, `become_tentative(); newline_carriage_return(fb)`); every
  other control or escape bumps the epoch alone. Insert semantics, the row shifted right.
* **A prediction is hidden until its epoch is confirmed** (h:76,
  cc:51-53): the first key after any control is drawn only once its real
  echo lands, and later keys of the same epoch at once. Paste (over 100
  bytes a read) and a resize reset the engine.
* **Judged against the real screen, never by a client timeout**: a
  prediction expires at a frame number the server's echo ack passes
  (cc:89-91); the paper says a client-side timeout was tried and flickered.
  A wrong displayed prediction clears the whole overlay (cc:530-533); a
  wrong tentative one kills its epoch.
* **The overlay is a copy applied at display time** (stmclient.cc:273-287);
  the authoritative framebuffer is never written.
* **Adaptive mode shows nothing under a 30 ms round trip** (h:203-204,
  cc:358-363, cc:455-474): the send interval floors at 20 ms, the trigger
  needs more than 30, and only a glitch -- an echo outstanding 250 ms --
  turns predictions on. Under 2.5 ms Mosh would draw nothing; this row runs
  what Mosh calls `Always`, and says so.
* Mosh's own error rate: **0.9 % of keystrokes** drawn wrong and repaired
  within a round trip, mostly word wrap.

## The design

**Predict one thing.** An **ASCII** printable (`0x20..=0x7e`) typed with
no modifier -- the app's `text` path -- when every one of these holds.
ASCII rather than any width-one printable, which is narrower than Mosh:
a narrow non-ASCII letter (`é`, Greek, Cyrillic) is refused, because the
cell comparison the judge makes is a codepoint equality the shell's own
composition could defeat and nothing here has tested one. Untested
rather than unsupported.

1. the core is in `Zone::Prompt`: between the shell's `OSC 133 ; A` (or
   `P`) and its `C` or `D`. This replaces Mosh's epochs: the shell says the
   user is at a prompt, and a program that stops echoing -- `passwd`, an
   editor -- runs after `C`, in `Zone::Output`, where nothing is predicted.
   A shell without the marks never predicts;
2. the primary screen, the live view (`back == 0`), no synchronised update
   open;
3. the cursor is visible. The cell under it may hold something: at a
   prompt that is the shell's own dimmed guess at what comes next
   (`zsh-autosuggestions` paints it after the first character and the
   first sweep refused eleven of thirty keys a round on it), and Mosh
   predicts over an occupied cell too -- as an insert, shifting the rest
   of the row in its overlay (`[[source: mosh-terminaloverlay-cc]]`,
   `new_user_byte`). baud draws the glyph over the cell and shifts
   nothing: where the shell inserts, the cell still holds the typed
   character on the echo and the prediction is confirmed, so the cost
   of not shifting is one frame in which the character to the right is
   the old one rather than the shifted one;
4. the cursor is not in the last column. The test is `col + 1 < cols`,
   the screen edge, and under DECSLRM that is **permissive rather than
   conservative**: `Term::cursor` clamps to the right margin, so with a
   margin inside the screen the gate passes at the margin and the
   prediction lands in the cell past it. No shell sets DECSLRM, and the
   zone gate refuses anything that is not a shell's prompt, so the case
   is unreachable through the predictor's own gates rather than handled;
5. no earlier prediction was contradicted since the zone opened;
6. the renderer is on the cell path, which is the only one that draws a
   prediction. On the instance and CPU paths a prediction would stamp a
   record and commit it on a frame that drew no glyph, so the instrument
   would report a gain nobody could see.

A second key before the first echo is predicted at the predicted cursor,
so typeahead chains.

**Its rendition is the cell to the left's**, Mosh's heuristic ("match
renditions of character to the left"): what was typed before is what
this will look like, and the cell under the cursor may carry the shell's
dimmed guess. A prediction in column zero takes its own cell's.

**Draw it as an overlay, never into the grid.** On the cell path the
frame already carries an overlay mask and a record list for the cells the
shader must not draw itself; a prediction is one more masked cell and one
more record -- the glyph `Renderer::cluster` gives the codepoint alone, in
the cursor cell's rendition -- and the cursor handed to the shader and to
the overlay's paint is one column right of the core's. The instance and
CPU paths draw the grid as the core holds it; predictions are not drawn
there, and the record says so.

**Judge it on the next read.** When bytes are fed, the pending
predictions are compared with the grid, oldest first: the cell at its row
and column holds its codepoint, confirmed and dropped, because the grid
is now the truth and the next frame draws it; the cell is still empty,
the echo has not reached it yet -- a read that carried only the first key
of two typed inside one round trip -- and the prediction stays pending
for the next read or the expiry; anything else, contradicted, with every
prediction after it, because the chain stood on it. And a read that
**scrolled** drops every prediction as contradicted before any of that:
a prediction names a screen row, so a scroll moves its cell out from
under it, and only a read can rotate the ring.

A contradiction holds predictions until the next prompt: until a read
after which the core's count of prompts opened on the primary
(`Term::prompts_opened`) has moved. The core bumps it as the bytes come,
wherever the primary's zone turns from output to `Zone::Prompt`, so a
single read carrying the whole of `C`, the program's output and the next
prompt's `A` releases the hold at its end, at any split of the read
(`a_command_in_one_read_releases_the_hold`). Until 2026-10-27 the reopen
was the zone sampled once a read, which that read left at `Prompt` on
both sides, and the hold stayed on until a later prompt. A redraw's mark,
which arrives in the prompt zone, and a mark on the alternate screen
open nothing (`a_prompt_opening_is_counted_whatever_the_reads`).
A prediction outstanding past `PREDICT_MAX` (150 ms, checked on the tick)
is dropped as expired and counted as contradicted: a shell that did not
echo a printable at its prompt is a shell whose widgets are eating keys.
Mosh rejected client timeouts because they flicker on a slow link; here
the link is 2.5 ms and the timeout is only reached when no echo comes.

**The frame is drawn at once.** The row-6 gate draws a fed echo whatever
the clock says; a prediction is another reason the frame is due now, and
the echo's own state survives the predicted frame so the real echo still
draws at once when it lands (a tick frame in between no longer loses it,
which was a gap row 6 left).

## The instrument

`baud-app/src/latency.rs` gains a `pred` stamp: a record whose key was
predicted is committed on the prediction's frame, before `fed`, and its
`commit`, `landed`, `shown` and bounds describe that frame -- the one the
user sees. `echo` and `fed` are still taken when the read comes, so
`child` and `wake` keep their meaning. The line prints `pred=+us` (or `-`)
after `written` and `predok=yes|no|-` at the end: whether the prediction
was confirmed, contradicted or never judged before the record closed.

A predicted record is shown before its echo, so its two bounds close
before the verdict; it is held past them until judged (or until the
app's own expiry calls it contradicted), so a slow echo lands in the
record rather than after it. A key the predictor declines prints
`keylat: refused <gate>`, and one predicted over an occupied cell
`keylat: noted over-occupied` -- lines beside the records, counted per
arm, so a sweep says how many keys the predictor never touched and why.

`scripts/key-latency.py` gains `pred` as an optional token (a baseline
from before the row prints none), a `predict` leg (`written` to `pred`),
a `build` measured from `pred` when there is one and from `fed`
otherwise -- so a predicted row is not excluded as a negative leg -- an
`app` leg (`handled` to `shown`: `to_shown` less the HID tap's delivery,
which is the system's and the noisiest leg under load), and per-arm
counts of predicted, confirmed, contradicted, refused and noted rows. The `--child none` control is unchanged: a child with no
prompt marks predicts nothing, so it still completes zero keys.

## What decides the row

* `to_shown` through the login shell, base against the change with an A/A
  copy: expected about -2.5 ms, floor 1 ms. `--child cat` and `--child
  stream` must be inside their floors -- no marks, no predictions.
* **Added during the row, not before it (2026-09-18):** an `app` leg,
  `handled` to `shown`, which is `to_shown` less the HID tap's delivery.
  It was added when the first session's sweeps put that delivery at
  4.1 ms against row 0's 1.65, which puts a four-millisecond IQR under
  `to_shown` and can hide the row's own. It turned out not to be needed:
  on a quieter box the delivery leg reads 2.0 ms and `to_shown` itself is
  a result three sweeps running. The leg stays because it is the one the
  app owns, and it is not what the row is kept on.
* The confirmation rate over the sweep's keys, printed per arm; any
  contradicted key is a case to read.
* The panel: by note O's pass rule, quantised; the probe under the shell
  is the adjudicator and is not run here.

## What is deliberately not built

Backspace, CR and arrow predictions (Mosh moves the cursor for them; here
a wrong cursor is worse than a late one); insert into existing text; wide
and combining characters; prediction outside a marked prompt; a flagging
underline; the instance and CPU paths.
