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
  runs right to left against the window's right edge, a Hebrew word in an
  English line reads right to left in its place, Arabic letters join into
  words, and a program can set which way its lines run. Still to come: a
  line that mixes directions and is split by a soft wrap, terminal-wg's
  per-paragraph modes, and the smaller gaps [bidi](../bidi.md) lists.
- **Key releases and modifier keys on macOS.** Programs that ask to be told
  when a key is let go, or when Shift or Control alone is pressed, are not
  told yet. Key presses themselves work.
- **Linux.** The Linux version builds and has run in a headless session, but
  nobody has looked at what it draws, or used its mouse, copied or pasted on
  a real desktop, and its GPU drawing is not started. It is not ready to
  use.
- **A few control sequences.** The single shifts SS2 and SS3, and the
  locking shifts LS2, LS3 and LS1R to LS3R, are not supported. The other
  gaps are each listed with their reason in [vt-coverage](../vt-coverage.md),
  as a row at **partial** or **out**.

## Known departures from the standards

Where baud does something other than what a standard says, that is a defect
to fix, and each one known today is listed in
[protocol decisions](../protocol-decisions.md#open-departures).

## Measured on one machine

Every speed and latency figure was taken on one Mac, an Apple M5 Max, and no
discrete graphics card has been measured. [Performance](performance.md) says
what was measured and how.

## Getting it

Nothing is published to a package manager or as a download yet, so baud is
built from source: see [Installing](install.md).
