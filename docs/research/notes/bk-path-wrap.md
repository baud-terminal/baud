# BK. A path wraps before its slash, and a mark beside each line the wrap continued

**What changed.** With `[screen] wrap_words = "on"`, the default, a path breaks before its separators: `ls /home/jordan` at ten columns draws `ls /home` then `/jordan`, where UAX #14's default drew `ls /home/` then `jordan`. Each line the display re-wrapped but a paragraph's first now has a hook arrow, ↪, in the window's padding at its start side (`[screen] wrap_marker`, on by default), so a continuation that begins `/jordan` does not read as a path the program printed.

**The texts.**
- UAX #14 rev 55 (`[[source: uax14]]`) LB13: "Do not break before ']' or '!' or '/', even after spaces. × SY"; the SY class is "a break opportunity after", a "limited break opportunity to improve layout of Web addresses". Breaking before `/` therefore departs from the default rules.
- UAX14-C1 allows it as a tailoring: "If the implementation tailors the behavior of Section 6.2, Tailorable Line Breaking Rules, that fact must be disclosed." § 8.1 permits "alteration, removal, or addition of rules applied after rule LB12", and § 8 gives the URL case itself: a tailoring "that suppresses SY × NU from rule LB25 within the scope of a URL".
- WCAG 2.2 SC 1.4.11 (`[[source: wcag22]]`): "a contrast ratio of at least 3:1 against adjacent color(s)" for "Visual information required to identify user interface components and states". SC 1.4.1: colour is not the only means, and the shape carries the mark.

**The tailoring** (`baud_domain::line_breaks_in_paths`; `line_breaks` is unchanged and still passes all of `LineBreakTest.txt`). It applies only to path runs: runs that start `/`, `~/`, `./` or `../`, that hold `://`, that start with a drive `X:\` or a UNC `\\` (where `\` separates as `/` does), or that hold two or more `/` and are not a number form (LB25's classes alone). The rules are T1 (`SP ÷` before the path's first separator or character, except after an opening bracket or quote), T2 (`sep × sep`), T3 (`: × sep`), T4 (`~/`, `./` and `../` stay whole), B1 (`seg ÷ sep`) and B2 (`sep × seg`, except after `://`). They are read after LB12a, so no Section 6.1 decision can move; `the_tailoring_keeps_every_six_one_decision` holds that, and was run at 20,000 cases.

**The defects of the default this fixes**, red on the base:

| text, columns | before | now |
|---|---|---|
| `ls /home/jordan`, 10 | `ls /home/` `jordan` | `ls /home` `/jordan` |
| `cd /home`, 7 | `cd /` `home` | `cd` `/home` |
| `/var/log/2026/11/04.log`, 12 | `/var/log/` `2026/11/04.l` `og`, broken at the edge | `/var/log` `/2026/11` `/04.log` |
| `C:\Users\jordan\Code`, 10 | `C:` `\Users\log` `ger\Code`, broken at the edge | `C:\Users` `\jordan` `\Code` |
| `https://github.com/jordan`, 20 | `https://github.com/` `jordan` | `https://github.com` `/jordan` |

`1/2`, `06/07/99`, `w/o` and `and/or` wrap as before.

**Prior art, at source.** helix, vim's `breakat` and VS Code's `wordWrapBreakAfterCharacters` all break after `/`; the Chicago Manual of Style, quoted secondhand, breaks before a single slash and after `//`. ICU and CLDR carry no path or URL rule. helix's `wrap-indicator` (`↪ `) and vim's `showbreak` are drawn inside the text and take two columns, Emacs's fringe arrows sit outside it, and VS Code's indicator, off by default, sits at the right edge. No terminal read (kitty, ghostty, wezterm) draws one, since none re-wraps at words.

**The mark.** It is drawn in the padding, never in a cell, so the grid keeps every column, a program is told the same size, and a copy, a selection, a search and a mouse report never meet it. The driver lends each side a band of its padding (`Renderer::set_margins`), which the whole frame holds as a margin at each end, so the grid's origin is where it was (`the_wrap_marks_take_the_padding_and_no_column`). The mark is a drawn mask, the same pixels on every system, in palette entry 8 lifted to 3:1 over every grey the material shows by `color::reads_at`, the ungated form of `Floor::lift_on_glass`'s arithmetic: `lift_on_glass` does nothing while the floor is off, so it cannot hold 3:1 alone. It is on the right for a right-to-left line, drawn only beside lines the display re-wrapped (not a row the terminal cut, not the alternate screen), and not drawn at all where the band is narrower than 8 pixels.

**Tests.** `baud-domain/tests/linebreak.rs`: the table above as opportunities beside the default's, the default kept outside path runs, and two properties. `baud-render/tests/word_wrap.rs`: `a_path_wraps_before_its_separators`, red on every row before; `a_fraction_a_date_and_w_o_wrap_as_the_default`. `baud-render/tests/wrap_marker.rs`: the marked rows, the right margin, the alternate screen, no room and the settings off, the grid's pixels unmoved beside the margins, a copy, 3:1. `baud-present-macos/tests/cells.rs`, `wrap_marks_gate`: both Metal arms and the CPU arm agree on every pixel. Perturbations: the hook before LB11, T1 dropped, number forms read as paths and the first line marked each turn a test red; with no marks placed all seven marker tests go red. Removing the explicit alternate-screen guard leaves them green, because the display never re-wraps that screen.

**Draw time only.** Line breaking runs on finished paragraphs when the display lays its window out again, not per ingested byte; finding the path runs is one pass over the paragraph. `baud-core` is not in the diff and has no caller of `line_breaks`; line breaking runs only in `baud-render` when the display lays its window out again. Nothing was timed because no per-byte path changed.

**Open.**
- The Linux driver keeps no padding, so it draws no mark.
- The film renders (`film.rs`) lend no padding, so they draw no mark.
- The badge's layout (`badge.rs`) still uses the default rules, though a badge often holds a directory.
- A path with a space in it is two runs, so `My Documents/x` keeps the default.
- `23/Jan/2024` and `TCP/IP/UDP` hold two slashes and letters, so they are path runs and break before their slashes. CSS Text 3 § 5.2 asks for no break between `/` and a letter there, and the default already broke after the slash.
