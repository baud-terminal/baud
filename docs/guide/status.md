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
- **Key releases and modifier keys on macOS.** Programs that ask to be told
  when a key is let go, or when Shift or Control alone is pressed, are not
  told yet. Key presses themselves work.
- **Linux.** The Linux version builds and has run in headless sessions.
  There, given the same fonts and colours, it draws the same pixels as the
  Mac version at scales 1 and 2 and in-between scales such as 1.5, it
  stays sharp when its window moves to a screen of another scale, it
  copies and pastes with the desktop's clipboard, and a mouse selects
  text, scrolls back through the history, pastes with the middle button,
  opens links with Ctrl-click and works in programs that use the mouse;
  characters drawn from each system's own fonts, and some programs' dim
  colours, can still look different. Nobody has used it on a real screen,
  and its GPU drawing is not started. It is not ready to use.
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

Nix builds and runs baud from the repository in one command, and Cargo
will once baud's first release is on crates.io; there is no download yet.
See [Installing](install.md).
