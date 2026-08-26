# Measurements

Single-core Xeon 2.1 GHz, 3 GB RAM, kernel 6.18.44. Ratios are meaningful;
absolute numbers are not portable.

## Cores compared, same real PTY, same payloads, best of 5

| core                          | streaming | TUI redraw |
|-------------------------------|-----------|------------|
| **baud**                      | 318-347   | 275-286    |
| alacritty_terminal            | 60-75     | 95-99      |
| termwiz (wezterm) parse only  | 60-61     | 63-64      |
| vt100 crate                   | 46-50     | 73-77      |
| wezterm (full terminal)       | 7.1       | 14.9       |

termwiz discards actions and keeps no screen, so it does strictly less work than
the others; it is wezterm's parse floor, not a terminal. Wezterm's own parser
runs at 61 MB/s while its full terminal runs at 7.1, so roughly 90% of wezterm's
cost is applying actions to the model rather than parsing.

## baud alone, real PTY, 48 MB/rep, median of 9

| mode                      | streaming | TUI redraw |
|---------------------------|-----------|------------|
| spans, zero-copy          | 204-211   | -          |
| viewport cells, computed  | 190-193   | 185-189    |
| viewport cells, templated | 189-191   | 186-191    |
| spans + promote hybrid    | -         | 185-187    |

Different harness from the table above (median-of-9 over 48 MB vs best-of-5 over
32 MB), so do not cross-compare the two.

## The tty tax, measured on alacritty

| payload    | from memory | through a PTY | cost |
|------------|-------------|---------------|------|
| streaming  | 58-75       | 43-51         | -30% |
| TUI redraw | 111-115     | 91            | -19% |

This is the line discipline, not the PTY as such, and it applies to every
terminal on Linux. It is the argument for a custom `tty_ldisc` independent of
anything in this repo.

## What is not measured

Latency, frame rate, memory as a process. ghostty and foot — the two
implementations most likely to be competitive. No renderer, no font shaping.
`baud-core` implements ~22 VT operations against alacritty's 66; the
differential test matches on 13 of 20 cases.
