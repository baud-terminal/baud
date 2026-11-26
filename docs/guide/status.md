# Status

baud is a terminal emulator for macOS and Linux. This page says where it
stands today: what you can use, and what is not built.

## Ready to use: macOS 26 and later

baud runs as an everyday terminal on a Mac with macOS 26 or later. Everything on
[Features](features.md) that is not marked Linux works there: the glass window,
light and dark colours, fonts and emoji, the cursor and its trails, scrollback with a
scrollbar and find, selection, copy and paste, mouse and keyboard support for
programs, and pictures.

## Ready to use: Linux under Wayland

baud runs as an everyday terminal on Linux under a Wayland compositor, with
the same terminal core as on the Mac. It draws on the GPU, through Vulkan,
on the device the compositor names, and draws on the CPU any frame the GPU
does not. With no Vulkan, or when the compositor refuses the GPU's buffers,
every frame is drawn on the CPU. A lost device is replaced once, a second
later, and a second loss keeps the window on the CPU; a window that
outgrows the device's memory draws on the CPU until it is made smaller; any
other failure keeps that window on the CPU. `BAUD_GPU=off` keeps every frame
on the CPU. On a 4K monitor under a tiling compositor it draws sharp at
scales 1, 1.5 and 2 and follows a change of scale live;
draws Hebrew, Arabic, Devanagari, Chinese, Japanese, Korean, Thai, emoji
and ligatures, the cursor trails, the activity lights on the top line, the
launch logo and the padding around the text; follows the desktop's light
and dark setting both ways; copies and pastes with the desktop's
clipboard; and runs full-screen programs such as system monitors, with
the keys you type and the mouse reaching them. A `font_size` in
points draws as large as the desktop's other windows draw it: 96 dots to
the inch times the desktop's text scale. What the window leaves beyond the
whole cells is split between its sides. On the headless Wayland
compositors baud is tested under, given the same fonts and colours at the same
size in pixels, it draws the same pixels as the Mac version, stays sharp
when its window moves to an output of another scale, copies and pastes
with the compositor's clipboard, and the mouse selects text, scrolls back
through the history, pastes with the middle button, passes links on to be
opened with Ctrl-click and works in programs that use the mouse. With
animations turned off on the desktop, baud reduces its motion: no default
cursor trail, a plain blink and still blinking text.

Open on Linux:

- **A font DPI a desktop keeps outside the settings portal is not read.**
  The text scale the portal relays is, and `[font] dpi` sets a size of
  your own.
- **Dim text a program draws is drawn brighter than on
  the Mac,** because baud on Linux cannot know what is behind its window.
- **A program cannot move, raise, lower or un-minimise the window,** or
  maximise it one way only: Wayland has no request for them.

## Not built

- **Right-to-left text, in part.** A line that starts in Hebrew or Arabic
  runs right to left against the window's right edge, and a Hebrew word in
  an English line reads right to left in its place; Arabic letters join, a
  program can set which way its lines run, and Left and Right follow a
  right-to-left line. A paragraph that wraps with both directions in it,
  and ECMA-48's reversed and directed strings, are not drawn
  ([bidi](../bidi.md)).
- **A few control sequences.** The gaps are each listed with their reason
  in [vt-coverage](../vt-coverage.md), as a row at **partial** or **out**.

## A program's own colours and light or dark

When the desktop or macOS switches between light and dark under
`theme = "auto"`, baud drops every colour a program set -- the palette
(`OSC 4`, `OSC 5`), the text and page (`OSC 10`, `OSC 11`), the cursor
(`OSC 12`) and the selection (`OSC 17`, `OSC 19`) -- and draws the theme's
colours for the new appearance, because colours chosen for one background
rarely read well on the other. A program that wants to choose again turns
on colour-change notices with `CSI ? 2031 h`. On each switch, baud then
sends it `CSI ? 997 ; 1 n` for dark or `CSI ? 997 ; 2 n` for light
([vt-coverage](../vt-coverage.md), the `?2031` row).

## Known departures from the standards

Where baud does something other than what a standard says, that is a defect,
and each one known is listed under
[open departures](../vt-coverage.md#open-departures) in the coverage table.

## Measured on two machines

Every speed and latency figure was taken on one Mac, an Apple M5 Max, or on
one PC, an AMD Ryzen Threadripper 9980X running Linux. On Linux, the
throughput through a pty is quotable with baud drawing on the CPU and
indicative with it drawing on the GPU, because the build measured does not
count, at exit, the frames drawn on the GPU
([MEASUREMENTS.md](../../MEASUREMENTS.md#six-terminals-through-a-pty-both-drawing-arms-linux-x86_64-2026-11-18)).
[Performance](performance.md) says what was measured and how.

## Getting it

Nix builds and runs baud from the repository in one command. There is no
release, and nothing to download. See [Installing](install.md).
