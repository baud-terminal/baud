# BE. Linux draws what macOS draws, pixel for pixel, given the same faces and floor

The Linux app's frames, taken from a real compositor, against the same
frames drawn by the CPU renderer on this Mac; the Linux clipboard
against a compositor's; in section 6, a pointer moved over the app
on two compositors (2026-10-29); and in section 7, what a frame copies
and damages there (2026-10-31). Sections 1-5 were taken 2026-10-26 at
`ef519585` plus this note's commits, by `scripts/linux-pixels.sh`, which
any later run repeats. The rule is that text looks the same on both
systems (2026-10-01).

## 1. What was compared

Six fixtures, each the same bytes on every arm: five recordings from
`tests/recordings/` -- less-pager (plain text), cat-cjk (CJK, emoji,
Hebrew and Arabic), vim-unicode (the same scripts in an editor), btop
(box drawing, braille, colour) and the sixel swirl (an image) -- and a
control, the page cleared to
rgb(185, 37, 105). Each ends with `CSI ? 25 l`, so no cursor is drawn:
the Linux app starts without the keyboard and would draw the hollow one.

Three arms, at 100x35 cells of 8x17 px, 13 px at a backing scale of one:

| arm | what draws | where |
|---|---|---|
| mac | the `pixels` bench: one frame on the CPU path, set up as the Linux driver sets it up with the default config | this Mac, aarch64 |
| linux | the same bench, the same source and the same nightly (commit `bcded3316`, checked) | Debian bookworm aarch64 in colima |
| shot | the Linux app, `baud`, on Wayland | weston 10.0.1, headless backend, pixman renderer, desktop-shell with no panel and a black background, taken by weston-screenshooter |

The default Linux stack finds one system face in the container, DejaVu
Sans Mono (fonts-dejavu-core 2.37), after the embedded Regular and icon
face; both benches read that file, the Mac arm from the copy the
container sends out. weston leaves a floating window's size to its
client, and the Linux window opens at 800x600, so the output is 800x600
and the window fills it at the origin; its grid is 100x35, 800x595, and
the five rows below it are the page. The app's child waits until `stty
size` reads `35 100`, then sets the tty raw and replays the stream: fed
at the 80x24 the pty is spawned at, a stream is reflowed by the resize
that follows, and a cooked tty turns LF into CR LF and echoes the
answers to a recording's queries.

## 2. Results

| fixture | mac against linux | shot against linux over black |
|---|---|---|
| less-pager | identical (`cmp`) | 0 of 476,000 pixels |
| cat-cjk | identical | 0 |
| vim-unicode | identical | 0 |
| btop | identical | 0 |
| swirl | identical | 0 |
| control | identical | 0 |

"Over black" is the bench's straight-alpha frame premultiplied and
rounded, (c a + 127) / 255, as both presenters round: the window's page
is dimmed to alpha 179 (`[glass] dim_dark = 0.70`), and weston draws it
over its black background. Every screenshot was taken twice a second
apart and was the same both times. The control holds (130, 26, 74) on
every one of its 476,000 pixels, computed in the script and read from
neither arm; each of its three channels rounds up there, so a truncating
premultiply would read one less on all three.

So on these fixtures nothing of baud's departs from the same pixels: the
renderer's arithmetic gives the same bytes on Darwin and on Linux, and
the Wayland driver hands the compositor exactly the renderer's frame.

*The image, The Linux app as weston shows it: vim-unicode, is not in the repository: it is taken again.*

## 3. What differs, and whose it is

Measured with the bench on this Mac, against the linux arm.

**The faces each system has.** The macOS default stack -- Menlo, Apple
Color Emoji, Apple Symbols, Hiragino Sans GB, Apple SD Gothic Neo and
Kohinoor, the first face of each collection, after the embedded Regular
and icon face -- against Linux's, DejaVu Sans Mono there:

| fixture | pixels that differ | largest channel difference |
|---|---|---|
| cat-cjk | 60,396 (12.7%) | 239 |
| vim-unicode | 18,768 (3.9%) | 239 |
| less-pager, btop, swirl, control | 0 | 0 |

The CJK, the emoji and the Arabic come from each system's own faces, the
Mac's drawn and Linux's a placeholder where it has none. The platform's
font choice, not the renderer's; the font order (configured, then the embedded font, then the system's) puts the fix in the embedded font. No
image of the macOS stack is committed: it draws Apple's glyphs.

**The backdrop the contrast floor reads.** The page is translucent, and
the floor lifts a text colour whose contrast against it, over the
backdrop behind the window, would fall below its trigger. The macOS app
reads the standard material's greys, 33 to 92 of 255; the Linux driver,
with no material behind it, reads black to white:

| fixture | pixels that differ | largest channel difference |
|---|---|---|
| btop | 26,029 (5.5%), in 580 of 3,500 cells | 146 |
| the other five | 0 | 0 |

btop's dim grey 64 is drawn 188 on Linux and 64 on macOS (186 cells);
against the page over black that is 10.36:1 and 1.90:1, over grey 33
9.62:1 and 1.76:1, over white 3.80:1 and 1.43:1 (WCAG 2.2 relative
luminance).

*The image, btop with the Linux driver's floor, over black, is not in the repository: it is taken again.*
*The image, btop with the macOS app's floor, over black, is not in the repository: it is taken again.*

The floor reading black to white on Linux was decided before these renders; weighed again with them, the decision stands unchanged. On Linux btop's dim greys are brighter
than on macOS, and legible over any background.

**The window's default size.** The Linux window opens at 800x600 pixels
when the compositor leaves the size to it, 100x35 cells; the macOS
window at 960x600 points. The grids differ, not their pixels.

## 4. The clipboard

weston's headless backend has no seat: `wl-copy` and `wl-paste` both
answer "Missing a seat", and the app has no data device there. sway 1.7's
headless backend has one, and wtype types through its virtual-keyboard
protocol; sway will not run as root, so that part runs as a user.

| step | result |
|---|---|
| `wl-copy` then `wl-paste`, no baud | the text |
| a key, then the program's `OSC 52` write; `wl-paste` | "from baud" |
| `wl-copy one`, Ctrl+Shift+V on the same keyboard | "one" reaches the program |
| wtype exits, `wl-copy two`, Ctrl+Shift+V on a second keyboard | before the fix nothing; after it "two" |

A copy needs the serial of an input event (wl_data_device.set_selection),
so it waits for the key; the key gives the window the focus that brings
the compositor's selection offer.

**The second keyboard was baud's.** Each wtype run is a keyboard of its
own, and when it exits the seat loses the keyboard capability. The
window kept its wl_keyboard and, on the next capability event, asked
for none, so no key reached it again. wl_seat.capabilities says "When
the pointer capability is removed, a client should destroy the
wl_pointer objects ... using the wl_pointer.release request. No further
pointer events will be received on these objects", and "The above
behavior also applies to wl_keyboard" (`[[source: wayland]]`, lines
2042-2056). The window now releases a lost keyboard, version 3 and
later, ends its focus if it had it, and asks for the next one. On a
desktop that is a keyboard unplugged and plugged in again, or a
Bluetooth keyboard reconnecting.

## 5. Checks of the harness itself

- A truncating premultiply put in the Wayland presenter in place of the
  rounding one: the control's 476,000 pixels and 3-6% of each text
  frame's pixels one level off (less-pager 20,437, cat-cjk 14,685,
  vim-unicode 29,155, btop 25,947); swirl's picture, opaque, none.
- The bench's page alpha 178 in place of 179 on the Mac arm: 93% to 100%
  of every frame's pixels (vim-unicode 441,007 to control 476,000), by 1
  on the text frames and swirl, by up to 4 on btop and by up to 15 on
  the control.
- weston 10's kiosk-shell shows its screenshooter nothing, not even
  weston-simple-shm, where desktop-shell shows both; that is why the
  run uses desktop-shell.

## 6. The pointer

Taken 2026-10-29 by `scripts/linux-pointer.sh`, which any later run
repeats. sway's headless backend, started with no input device, is
given one by `scripts/linux-pointer.py`: a client that writes the
Wayland wire format itself and holds one virtual pointer
(`[[source: wlr-virtual-pointer-unstable-v1]]`) and one virtual keyboard
(`[[source: virtual-keyboard-unstable-v1]]`) for a whole script of
absolute moves, buttons, wheel detents, keys and modifiers. Each event
carries a time from the script's own clock, so a double click is one
however the machine is scheduled. One binary, built in bookworm, runs
under two sways:

| arm | compositor | wl_seat | the pointer's image | a wheel detent |
|---|---|---|---|---|
| bookworm | sway 1.7 | 7 | no cursor-shape global: baud draws its I-beam, its pointing hand (since the run in "The hand over a link" below) or its arrow and sets it with wl_pointer.set_cursor | axis_discrete |
| trixie | sway 1.10.1 | 9 | wp_cursor_shape_device_v1.set_shape (`[[source: wp-cursor-shape-v1]]`) | axis_value120 |

The window floats at 640x408 at the origin of a 1024x768 output: 80x24
cells of 8x17 px. What is read: the bytes the program receives,
wl-paste's primary selection and clipboard, the arguments a stand-in
`xdg-open` writes down instead of opening anything, the app's pointer
requests in sway's `WAYLAND_DEBUG=server` log, and grim's screenshots.
Each arm's 32 checks and its traffic are in
`[[source: be-obs-pointer-bookworm]]` and `[[source: be-obs-pointer-trixie]]`.
Both arms read the same on every check but the image's:

| path | read |
|---|---|
| a drag from column 7 of row 1 to column 7 of row 2 | primary "beta gamma\none two"; Ctrl+Shift+C puts the same on the clipboard |
| the same drag with Alt held | primary "0123\nabcd\nABCD", a block (`[[source: be-obs-pointer-bookworm-block]]`) |
| a double click; a triple click | "gamma"; "one two three four" |
| two clicks 300 ms apart; the same 10 ms apart; then Shift and a click | nothing selected; "beta"; "beta gamma\none", extended by words |
| three wheel detents up over 100 lines, then three down | the top row reads "line 75", then "line 78"; the program reads nothing; the window's rows are the live rows three lower, 0 of 214,200 pixels different (`[[source: be-obs-pointer-trixie-wheel-back]]`) |
| a touchpad's 51 px up, no detents | "line 75": three lines of the 17 px cell |
| the thumb dragged to the top | "line 1"; below the first row the frame is the wheel's at the top, 0 of 234,600 pixels different, and not the live rows |
| the wheel on the alternate screen, three up and two down | `CSI A` three times and `CSI B` twice |
| `?1000` `?1006`: left, middle, right, the wheel both ways and sideways, Ctrl, Alt, Shift | `CSI < 0;11;3 M` and `m`, `1`, `2`, then `64`, `65`, `66`, `67`, then `16` and `8` pressed and released; Shift's click selects and sends nothing |
| `?1002` drag; `?1003` moves; `?1000` alone | a report a cell, `32` added while the button is held, `35` with none; X10's `CSI M` and three bytes, each value plus 32 |
| the middle button, then Shift+Insert, with `?2004` | "from the primary" twice, bracketed |
| Ctrl over an `OSC 8` link; a Ctrl click on it, on a URI in the text, and on the link under `?1000` | the address in the bar (`[[source: be-obs-pointer-trixie-link-preview]]`), gone when the pointer leaves; `xdg-open` given each address, one argument; under tracking the program hears only a Ctrl click off the link, `CSI < 16;38;4 M` |
| a click six columns left of the cursor under `?2001` | six `CSI D` |
| the shape, through no `OSC 22`, a key, `hand2`, `left_ptr`, `watch`, an empty `OSC 22`, a leave and an enter, and a new pointer | sway 1.10.1: text, hidden, pointer, hidden, default, hidden, wait, hidden, text, then text after each enter (`[[source: be-obs-pointer-trixie-hand2]]`); sway 1.7: the I-beam, and the arrow for the three named (`[[source: be-obs-pointer-bookworm-hand2]]`) |
| every set_cursor and set_shape | the serial of the last enter, as both requests require |
| the side, extra, forward and back buttons under `?1000` `?1006`; `BTN_TASK`; the side button dragged under `?1002`, then `?1000` alone | `CSI < 128;11;3 M` and `m` to `131`, nothing for the twelfth, `160` for the drag, X10's byte 160 and a release that names no button |
| DEC's locator: DSR `?55` `?56`, DECELR 1;2 and DECSLE 1;3, a left click, a side click, an extra click, Shift's click; DECEFR 2;10;4;14 and a move out of it; DECRQLP; DECELR 1;1 and DECRQLP, a click; DECELR 2 and two clicks; DECELR 1;2 with DECEFR's corners omitted and the pointer leaving the window; DECELR 0 and DECRQLP | `CSI ?50n` `CSI ?57;1n`, `CSI 2;4;3;11&w` `CSI 3;0;3;11&w`, `8;8` and `9;0`, nothing, nothing; `CSI 10;0;3;16&w` once; `CSI 1;0;3;18&w`; `CSI 1;0;43;141&w` and the click at pixel 141, row 43; one report; `CSI 10;0&w`, the position omitted; `CSI 0&w` |
| the `OSC 133` prompt: `cl=line` over `?2001`, the input "ab", a wide character and "cd"; clicks on "b", the wide character's right half, the left prompt, past the input and a row below; then prompts with `click_events=1` and `=2`, each after a `C` and a `D` | four, three, five and no `CSI D`, nothing for the row below; `CSI < 0;4;2 M` at the screen's row, `CSI < 0;4;1 M` from the prompt's top |
| the grid's corners under `?1000` `?1006` on the alternate screen, at scale 1, 2 and 1.5 | `1;1` at (0, 0); `80;24` at (639, 407), (632, 391) and (639.5, 407.5); `79;23` at (631, 390) and (631.5, 390.5); under `?1016` pixels `1;1` and `640;408`, at every scale |

Every check of the locator, the prompt and the side buttons reads bytes
the program asked for and the DEC, semantic-prompts and xterm texts
give: DECLRP Pe;Pb;Pr;Pc with no page (070-13, `[[source: dec-std-070]]`),
`cl=`'s "double-width characters only count one"
(`[[source: semantic-prompts]]`), kitty's absolute and relative rows
(`[[source: kitty-shell-integration]]`), "adding 128 (for buttons 8
through 11)" (`[[source: ctlseqs]]`).

**At another scale** each arm runs again with the output at 2048x1536
scale 2 and 1536x1152 scale 1.5, so the layout, the window and every
point above stay in 1024x768 logical pixels; the virtual pointer's
extent doubled reaches the half pixels between. The corners read the
same at every scale on both arms, the half pixels included
(`[[source: be-obs-pointer-scale]]`). The window's buffer stays
640x408 with no buffer scale at either: sway scales it up, and the app
binds no fractional-scale global, though sway 1.10.1 offers
wp_fractional_scale_manager_v1 and wp_viewporter. sway 1.10.1
sends `preferred_buffer_scale` 2 at both scales and names the pointer's
shape; sway 1.7, at `wl_compositor` 4, has no `preferred_buffer_scale`,
and the app draws its pointer at 48x48, buffer scale 2, at both.

The new pointer is the seat losing its pointer and keyboard and gaining
new ones: the app releases the old wl_pointer, destroys its shape device,
asks for the new one and sets the I-beam at its enter.

**What the second run found, and what changed.** On the binary built
from `487d78bc`, both arms (`[[source: be-obs-pointer-base2]]`):

- *The side buttons reached a program as a release.* `BTN_SIDE` to
  `BTN_BACK` and every button past them were reported as "3=release":
  `CSI < 3;11;3 M` for a press, a drag with one held not reported, and
  in X10 a press that read as a release. ctlseqs gives buttons 8 to 11
  "by adding 128", and xterm's `BtnCode` adds 128 for the bit 8
  (`[[source: xterm-button-c]]`). They are X's numbers -- the kernel's
  codes as "8 + b - BTN_SIDE" (`[[source: xf86-input-libinput-btn]]`),
  AppKit's `buttonNumber` 3 to 6 as XQuartz numbers them
  (`[[source: xquartz-x11application]]`) -- on both apps, and a button
  past the eleventh, where "the encoding is ambiguous", sends nothing and
  is counted in `mouse_unencodable`. `buttons_8_to_11_add_128`,
  `a_button_past_the_eleventh_is_refused_and_counted`,
  `the_side_button_is_button_8_to_a_tracking_child` and
  `appkits_buttons_past_the_three_are_xs_8_to_11` hold it.
- *On sway 1.7 at scale 2 and 1.5 the pointer was drawn at 24x24, buffer
  scale 1*, and sway scaled it up: the app took its scale only from
  `preferred_buffer_scale`, which a compositor below version 6 never
  sends. `set_buffer_scale` asks for "the same buffer scale as the scale
  of the output that the surface is displayed on" (`[[source: wayland]]`):
  the window binds each `wl_output` for its `scale` and draws the pointer
  at the largest scale of the outputs its surface entered, which
  `an_old_compositor_draws_the_pointer_at_its_outputs_scale` holds over a
  socket pair.

Checks of the harness: on the base binary the side check and the drawn
pointer's check failed on their arms and every other check passed; the
seat's test failed with the side button reported as button 9, and with
the twelfth reported as "3=release"; the socket-pair test failed with the
output's scale read as one, and with its leave ignored.

**What the first run found, and what changed.** Four checks failed on the
binary built from `12a190d1`, on both arms (`[[source: be-obs-pointer-base]]`),
one for each fix below and two more for the second:

- *A modifier pressed or let go in the same read as a click.* The window
  queued each pointer frame and the app read the modifiers when it took
  the event from the queue, so it read what the keyboard held by then.
  With Ctrl, Alt and Shift each held for one click and let go before the
  next, sent without a pause, all three clicks were reported as
  `CSI < 0;11;3 M`: no Ctrl, no Alt, and Shift's click reported where it
  selects. It is a race: the Alt drag, whose Alt is let go after the
  release, failed on the first run of the base and passed on later ones,
  as the modifier and the click fell in one read or two. On a desktop
  that is Ctrl let go just after a click, while the app is busy drawing. wl_keyboard.modifiers "notifies clients that the
  modifier and/or group state has changed" (`[[source: wayland]]`), at
  its place in the stream; each pointer event now carries the modifiers
  held when its frame came, and `a_pointer_event_carries_the_modifiers_of_its_moment`
  holds it over a socket pair.
- *The wheel and the thumb moved the window over the history and the
  app drew the live screen.* The selection, a link's lookup and the
  thumb read the history -- the triple click read "line 75" -- while the
  frame kept "line 78" on top
  (`[[source: be-obs-pointer-base-trixie-wheel-back]]`), and the thumb
  dragged to the top drew the live rows: the driver painted the live
  rows where the macOS CPU path paints the window's. It now paints
  `Term::window_back`'s rows through `render_at`.

**Checks of the harness.** `hand2` mapped to `default` failed the trixie
arm's shape check and passed bookworm's, which draws the arrow for both;
each pointer event given the modifiers held at `next_event` failed the
unit test. The history drawn under the wheel and the thumb is held only
by the harness's three pixel checks: the Linux driver has no source or
unit guard like the one `app.rs` has for its `.render_at(` call, so an
edit back to `render` would be caught only by a compositor run.

**Found and left.** After a key hides the pointer, the program's next
`OSC 22` is set twice, the same shape in two requests, harmless: the read
shows it and then the read's unhide shows it again (closed by the run
below, as is the I-beam kept over a link while Ctrl shows it). A second `OSC 133 ; A` on the row after
an input line, with no `C` or `D` between, was taken by the app on sway
as a redraw: after `$ `, `B`, "ab", a wide character and "cd" on row 0,
the bytes `\r\n`, `A;click_events=1`, `> `, `B` and "xy" drew the new
prompt over row 0, where the core fed the same bytes, at 80x24 and after
the app's 100x35 to 80x24, opens it on row 1; the child's `stty size`
read 24 80 throughout. Unexplained; the prompt session now sends `C` and
`D` between its prompts, as a shell does. The thumb dragged to the top
read "line 12" once, on the bookworm arm, while the host compiled; every
run without that load read "line 1". Not run on a compositor: the window
drawn at a scale other than one, which the app does not do on Linux
(its buffer is the window's size in logical pixels and the compositor
scales it), and so no pixels compared against the CPU renderer at a
scale; and the pointer over a link under the scrollbar's strip, which
keeps its shape while the thumb shows and is held by
`over_the_scrollbar_a_link_keeps_the_programs_pointer`.

**The hand over a link.** Run again on 2026-10-29 with three checks
added or changed (`[[source: be-obs-pointer-hand]]`): the link session gained a
second row, an `OSC 8` link to an `ftp:` address, which the app refuses,
and its pointer requests are compared, folded as the shape session's
are. All 28 checks pass on each arm:

| arm | Ctrl over the `https` link, over the refused one, off the window, then under `?1000` after a key |
|---|---|
| sway 1.10.1 | text, pointer, text, leave, enter, text, hidden, text, pointer, text, leave (`[[source: be-obs-pointer-trixie-link-hand]]`) |
| sway 1.7 | the I-beam, the hand, the I-beam, leave, enter, the I-beam, hidden, the I-beam, the hand, the I-beam, leave (`[[source: be-obs-pointer-bookworm-link-hand]]`) |

The shape session's `hand2` on sway 1.7 is now the hand the app draws,
where it was the arrow; sway 1.10 names `pointer` as before. The
harness tells the drawn hand by its hotspot, (9, 2). The same script on
the base binary, `487d78bc`, fails exactly the three expectations this
run added or changed and passes the other 53: neither compositor is sent the pointer shape
while Ctrl is held over the link, and sway 1.7 draws the arrow for
`hand2`. In the shape session's raw trace, which is not committed and
which the folded check cannot show, the double set above is gone: after
a key hid the pointer, the base sent each of `pointer`, `default` and
`wait` twice and the change sends each once, since a hidden pointer is
no longer given a shape until what shows it. An earlier run of the change, whose
two new expectations lacked the session's closing leave, sent the same
folded requests on both arms.

## 7. A frame's damage

**A one-line change on Linux copies 54 KB into the window's buffer, not
1.9 MB, and tells the compositor 17 rows changed, not 600; on two real
compositors the screen then shows the frame exactly.** Taken
2026-10-31 by `scripts/linux-damage.sh`, which any later run repeats
(`[[source: be-obs-damage]]`). The `present` bench
(`baud-bench/benches/present.rs`) draws through the Linux driver's
renderer and window, filling an 800x600 output on sway 1.7 and 1.10.1,
headless with the pixman renderer: 100x35 cells of 8x17 px. Its `rows`
arm gives `Window::present` the bands the renderer painted, as the
driver has since 2026-10-21; `whole` says every frame changed
everything, as the driver did before. A workload rewrites one row a
frame (line), one row a frame a row lower each time (walk), prints a
line at the bottom (scroll) or rewrites every row (full); 300 frames a
run, nine rounds an arm, the first dropped.

| workload | arm | copied a frame | damaged | `present`, slower speed | faster speed |
|---|---|---|---|---|---|
| line | whole | 1,920,000 B, 600 rows | 600 rows | 660 / 655 us | 154 / 152 us |
| line | rows | 54,400 B, 17 rows | 17 rows | 22.0 / 23.1 us, the second one round | 5.8 / 6.0 us |
| walk | whole | 1,920,000 B | 600 | 657 / 649 | 155 / 152 |
| walk | rows | 163,200 B, 51 rows | 34 | 59.8 / 53.2 | 14.1 / 14.8 |
| scroll | whole | 1,920,000 B | 600 | 648 / 647 | 153 / 150 |
| scroll | rows | 1,904,000 B, 595 rows | 595 | 643 / 644, the second one round | 150 / 151 |
| full | whole | 1,920,000 B | 600 | 644 / 572, the second one round | 149 / 151 |
| full | rows | 1,904,000 B | 595 | 640 / 640 | 151 / 151 |

Each time is sway 1.7's, then 1.10.1's: the median of the rounds' median
frames, over two to six rounds at that speed, or the one round where the
table says so, which is no result against another. The VM ran each run
at one of two speeds about four times apart, the render with them, so
`scripts/linux-damage.py` tells a round by its render time and reports
each speed apart, leaving out the few that changed speed partway; every
round's figures are in the observation. The damaged column is the
bench's own set of the rows it told the window, not a count of the
`wl_surface.damage_buffer` requests sent; sway's trace of the opaque
`rows` run corroborates it, its requests 17, 34, 595 and 600 rows high
on both sways, the 600 the whole frame each workload starts with. A
scroll and a full repaint change every row of the grid, so the two arms
copy the same but for the five margin rows below it: `wl_shm` has no
request that moves rows within a buffer. sway's own CPU time a frame on
the line and walk runs read 0.32-1.22 ms whole and 0.10-1.24 ms rows on
1.7, 0.05-0.20 and 0.06-0.17 on 1.10.1; the ranges overlap, so there is
no result. The bench's faster rounds on 1.7 alone did not overlap,
0.32-0.80 against 0.10-0.25, but sway runs at a speed of its own, which
the bench's render time does not tell.

**The screen.** grim took the screen after each of the first 30 frames
of every workload: with the page opaque, none of the 480,000 pixels of
any of the 120 shots an arm differed from the frame, on either sway; at
the default dim the two arms' shots hashed the same frame for frame. Two
patches the script applies, kept in the observation, show the check
fails when the code is wrong. Each strip damaged in its top half alone:
5,092,573 pixels unlike the frame on the whole arm and 2,387,376 on rows
on 1.7, 5,092,396 and 2,360,989 on 1.10.1, so both sways repaint what is
damaged and no more. A buffer given only its own frame's rows, not those
it missed: 13,992,000 and 14,030,172 on rows, and none on whole, which
copies every row. The 13,992,000 are the line workload's on both
sways, from the second buffer's first frame: a new buffer misses every
row, and the patch gave it 17. A buffer used again that misses a row the
frame before changed, the walk's, showed on 1.10.1 alone, 38,172
pixels, and on 1.7 not at all: the row lies outside the frame's damage,
which is all a compositor need repaint, so a screen cannot be relied on
to show it. The buffer's content is held by
`a_frame_copies_and_damages_only_the_rows_it_changed`, on the fake
compositor.

**A buffer used again.** In the `WAYLAND_DEBUG=server` trace both sways
hold a buffer until the next frame's commit and release it after: the
opaque `rows` run's 164 attaches went to the two buffers in turn, so each
frame is drawn into the buffer that missed the frame before -- what the
walk's 51 rows are, its own 34 and the 17 rows the buffer missed. The
protocol leaves the timing to the compositor: a release "before the
frame callback requested in the same wl_surface.commit" frees the
buffer at once (`[[source: wayland]]`, `wl_buffer.release`), so a
compositor that copies to a texture can be given the same buffer back.

## 8. Open

- The pointer on a screen, and on GNOME's and KDE's compositors.
- Everything here is over a black background; a desktop with a
  wallpaper, and a screen, are not.
- The GPU half of Linux.
- The window drawn at the output's scale: its buffer at the scale and
  the grid in device pixels, through `preferred_buffer_scale` or
  wp_fractional_scale_v1 with wp_viewporter, as the macOS app draws
  at the backing scale.
- One run of each fixture at one size and one font size.
- A frame's damage on a GPU compositor, which may release a buffer
  before the next commit, and a screen's.
- A reused buffer's missed rows on a screen: seen on sway 1.10.1, out
  of the screen's reach on 1.7, and held on the fake compositor.
