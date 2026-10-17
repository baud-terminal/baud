# Status

Where baud stands today: what you can use, and what is not built yet.

## Ready to use: macOS 26 and later

baud runs as an everyday terminal on a Mac with macOS 26 or later. Everything
on [Features](features.md) works there: the glass window, light and dark
colours, fonts and emoji, the cursor and its trails, scrollback with a
scrollbar, selection, copy and paste, mouse and keyboard support for
programs, and pictures.

## Not built yet

- **Right-to-left text.** Hebrew letters are drawn with their points, but a
  right-to-left line is laid out left to right. Bidirectional layout is
  not built ([note AB](../research/notes/ab-right-to-left.md)).
- **Search.** There is no way to search the scrollback yet.
- **Key releases and modifier keys on macOS.** Programs that ask to be told
  when a key is let go, or when Shift or Control alone is pressed, are not
  told yet. Key presses themselves work.
- **Linux.** The Linux version builds and has run in a headless session, but
  it takes no mouse input, nobody has looked at what it draws, and its GPU
  drawing is not started. It is not ready to use.
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
