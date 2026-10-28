# BE. Linux draws what macOS draws, pixel for pixel, given the same faces and floor

The Linux app's frames, taken from a real compositor, against the same
frames drawn by the CPU renderer on this Mac; and the Linux clipboard
against a compositor's. Taken 2026-10-26 at `ef519585` plus this note's
commits, by `scripts/linux-pixels.sh`, which any later run repeats. The
owner's rule is that text looks the same on both systems (2026-10-01:
"yes the rule is they must look the same").

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
font choice, not the renderer's; the owner's order (configured, then the
embedded font, then the system's) puts the fix in the embedded font. No
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

The floor reading black to white on Linux is b83's decision (Jev
0.98/0.97, decisions 796-797; the record, "The window body is the
standard material"). Jev re-weighed it with these renders, PER_BACKDROP
at 0.80/0.80 and then 0.74/0.82: below 0.9, so the lean is taken and
b83's decision stands unchanged. On Linux btop's dim greys are brighter
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

## 6. Open

- The pointer: neither headless seat has one, so wl_pointer, the
  pointer shapes and a selection made by dragging have not run on a
  compositor.
- Everything here is over a black background; a desktop with a
  wallpaper, and a screen, are not.
- The GPU half of Linux.
- One run of each fixture at one size and one font size.
