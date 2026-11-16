# Status

Where baud stands today: what you can use, and what is not built.

## Ready to use: macOS 26 and later

baud runs as an everyday terminal on a Mac with macOS 26 or later. Everything on
[Features](features.md) that is not marked Linux works there: the glass window,
light and dark colours, fonts and emoji, the cursor and its trails, scrollback with a
scrollbar and find, selection, copy and paste, mouse and keyboard support for
programs, and pictures.

## Ready to use: Linux under Wayland

baud runs as an everyday terminal on Linux under a Wayland compositor, with
the same terminal core as on the Mac. On a 4K monitor under Hyprland it
draws sharp at scales 1, 1.5 and 2 and follows a change of scale live;
draws Hebrew, Arabic, Devanagari, Chinese, Japanese, Korean, Thai, emoji
and ligatures, the cursor trails, the activity lights on the top line, the
launch logo and the padding around the text; follows the desktop's light
and dark setting both ways; copies and pastes with the desktop's
clipboard; and runs programs such as btop, with the keys you type and the
mouse reaching them. A `font_size` in
points draws as large as the desktop's other windows draw it: 96 dots to
the inch times the desktop's text scale. What the window leaves beyond the
whole cells is split between its sides. On the headless Wayland
compositors weston and sway, given the same fonts and colours at the same
size in pixels, it draws the same pixels as the Mac version, stays sharp
when its window moves to an output of another scale, copies and pastes
with the compositor's clipboard, and the mouse selects text, scrolls back
through the history, pastes with the middle button, passes links on to be
opened with Ctrl-click and works in programs that use the mouse. With
animations turned off on the desktop, baud reduces its motion: no default
cursor trail, a plain blink and still blinking text.

In progress on Linux:

- **Drawing on the GPU.** baud draws on the CPU; a renderer on the GPU,
  through Vulkan, is being built.

Open on Linux:

- **A font DPI a desktop keeps outside the settings portal is not read.**
  The text scale the portal relays is, and `[font] dpi` sets a size of
  your own.
- **Dim text a program draws, such as btop's, is drawn brighter than on
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

## Measured on one machine

Every speed and latency figure was taken on one Mac, an Apple M5 Max, and no
discrete graphics card has been measured. [Performance](performance.md) says
what was measured and how.

## Getting it

Nix builds and runs baud from the repository in one command. There is no
release, and nothing to download. See [Installing](install.md).
