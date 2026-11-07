# Status

Where baud stands today: what you can use, and what is not built yet.

## Ready to use: macOS 26 and later

baud runs as an everyday terminal on a Mac with macOS 26 or later. Everything on
[Features](features.md) that is not marked Linux works there: the glass window,
light and dark colours, fonts and emoji, the cursor and its trails, scrollback with a
scrollbar and find, selection, copy and paste, mouse and keyboard support for
programs, and pictures.

## Not built yet

- **Right-to-left text, in part.** A line that starts in Hebrew or Arabic
  runs right to left against the window's right edge, and a Hebrew word in
  an English line reads right to left in its place; Arabic letters join, a
  program can set which way its lines run, and Left and Right follow a
  right-to-left line. A paragraph that wraps with both directions in it,
  and ECMA-48's reversed and directed strings, are not drawn yet
  ([bidi](../bidi.md)).
- **Linux.** The Linux version builds and has run only on headless Wayland
  compositors, weston and sway, which show nothing on a screen. There,
  given the same fonts and colours, it drew the same pixels as the Mac
  version at scales 1 and 2 and in-between scales such as 1.5, stayed
  sharp when its window moved to an output of another scale, copied and
  pasted with the compositor's clipboard, and a virtual mouse selected
  text, scrolled back through the history, pasted with the middle button,
  passed links on to be opened with Ctrl-click and worked in programs
  that use the mouse. Characters drawn from each system's own fonts, and
  some programs' dim colours, can still look different. Input methods,
  the bell, zoom and the desktop's light, dark and accessibility settings
  have not run even headless. Nobody has used it on a real screen, and
  its GPU drawing is not started. It is not ready to use.
- **A few control sequences.** The gaps are each listed with their reason
  in [vt-coverage](../vt-coverage.md), as a row at **partial** or **out**.

## Known departures from the standards

Where baud does something other than what a standard says, that is a defect
to fix, and each one known today is listed in
[protocol decisions](../protocol-decisions.md#open-departures).

## Measured on one machine

Every speed and latency figure was taken on one Mac, an Apple M5 Max, and no
discrete graphics card has been measured. [Performance](performance.md) says
what was measured and how.

## Getting it

Nix builds and runs baud from the repository in one command. There is no
release yet, and nothing to download. See [Installing](install.md).
