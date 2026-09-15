# M. Resize, reflow, and the prompt the shell redraws

**The question.** A window resize leaves another copy of the shell's
prompt on screen, and every resize adds one. The owner reports it on
baud, and reports the same defect on wezterm, alacritty, Ghostty and
Terminal.app. This note is what the mechanism is, what everyone else
does about it, and which design baud should take.

**Everything below that is a URL is a search result rather than a
source.** `MANIFEST.toml` carries a SHA-256 for every `[[source: ...]]`
and `scripts/check-sources.sh` gates it, so nothing here may be cited
that way until it is fetched and pinned. They are leads, and the ones
worth pinning are named at the end.

## The mechanism, measured

A shell redraws its prompt on a resize by **counting rows**. Captured
from the owner's own `zsh`, one resize is:

```
\r  ESC[4A  ESC[0m ESC[27m ESC[24m  ESC[J  OSC 133;P;k=i  \r\n  zsh ...
```

Carriage return, cursor **up 4**, erase from the cursor to the end of
the screen, then the prompt again. The `4` is the shell's own count of
how many rows its prompt occupies. The erase only removes the old prompt
if that count reaches the top of it.

`baud-bench`'s `winch` replays such a capture through `Term` with the
same resizes between the chunks and prints the screen at each step. At
the step that adds a copy, the old prompt occupies **seven rows** while
the shell moves up **four**: the erase begins three rows too low and the
rows above it survive. Seven, because the prompt was drawn at 111
columns and re-cut to 92 -- a line padded to the old width becomes two
rows at the new one.

**So the shell computes for the new width and the terminal re-laid the
old prompt.** The two cannot agree, and each resize orphans another
line.

| capture, replayed through the core | shipped | with the width re-cut removed |
|---|---|---|
| six resizes, every size holds the prompt | **4** | **1** |
| ten steps at 16 ms, a drag | **2** | **1** |
| a cycle reaching 12x4 | 2 | 2 |

The last row stays at two either way and is not a defect: a five-line
prompt does not fit a four-row screen, so the shell's own redraw
scrolls and a prompt line goes into the history. That copy is content.

**One correction, because it was nearly a wrong conclusion.** An earlier
reading of the first row said "one copy, so an ordinary zoom is clean".
It was measured on a binary built before the source was restored -- the
no-reflow arm wearing the shipped arm's name. Rebuild between arms and
check the binary is the one you think it is.

## What the mechanism is not

Three candidates were flipped off one at a time against the captures and
none is the cause:

* **Deferred wrap.** A line that exactly fills the row must not leave it
  until something follows, or a prompt padded to the width occupies one
  row too many. baud is correct here and `baud-core/tests/wraptest.rs`
  pins it.
* **Pulling rows back from history on a growth.** Disabling it leaves
  the copies.
* **Reflowing the history.** Reflowing only the visible rows leaves the
  copies too, so the history's re-cut is not what moves the prompt.

What remains is the re-cut of the rows the shell is about to redraw.

## What everyone else does

* **VS Code** debounces. `microsoft/vscode#330040` skips the resize when
  the grid is unchanged, applies **row** changes immediately and
  **debounces column changes to 300 ms**, on the stated grounds that
  "zsh needs longer than 100 ms to complete prompt redraw". The split is
  the same observation this note makes from the other side: height does
  not re-wrap and width does.
* **Ghostty** clears. On seeing a prompt mark it assumes the shell can
  redraw, and on a later resize it **clears the active prompt** if the
  cursor is at one, searching upward and stopping at command output or
  another marker. It re-wraps and then removes the evidence. Its own
  tracker carries the failure mode that produces:
  `ghostty-org/ghostty#209`, "clears prompt on resize in some cases and
  never redraws", and discussion 8817, "shell-integration causes prompt
  to disappear when resizing".
* **xterm and Terminal.app** do not reflow at all. The owner reports
  Terminal.app still duplicating, so not reflowing is not by itself
  sufficient *there* -- though on baud it is, measured above, and the
  difference is worth understanding before leaning on it.
* **The shells** know. `romkatv/powerlevel10k#1200` proposes that "the
  shell/p10k tells the terminal which lines must not be reflown (via
  sending out a VT sequence ...)" and a `prerender` hook to send it. The
  author notes no such sequence exists. `fish-shell#7491` is the same
  class for the right prompt.

**Nobody uses the marks a shell already sends to decide what may be
re-cut.** Ghostty uses them to clear after the fact; the p10k proposal
wants a new sequence for it. That gap is the opportunity.

## The design

**Do not re-cut what the application is about to redraw; re-cut
everything above it.**

The boundary is `OSC 133`, which the owner's shell already emits -- the
capture above carries `133;P` and `133;B`. Prompt start marks where the
region the shell owns begins; everything above it is finished command
output that nobody will redraw, and re-wrapping it is a pure win.

Three properties fall out, and the third is why this beats the prior
art:

1. **The shell's count stays valid**, because the rows it drew are the
   rows it finds. No orphan, with no timer and no debounce.
2. **Output above the prompt still re-wraps**, so a long line stays
   readable at a narrower width -- which is what not reflowing at all
   gives up.
3. **Nothing is cleared speculatively.** Ghostty's failure mode is a
   prompt it erased that the shell never redrew. A rule that only
   *declines to disturb* rows cannot produce it: the worst case is a
   prompt laid out for the old width, which the shell's own redraw then
   replaces.

**Without `OSC 133` it degrades to the same answer by a coarser
boundary**: do not re-cut the rows from the cursor's line up to the last
boundary the terminal can see, or in the simplest form do not re-cut the
live screen at all. The measured 4 to 1 above is that simplest form,
which is what makes it a safe default rather than a guess.

**And baud's fixed pitch makes not-re-cutting cheaper than it is
elsewhere.** A cell keeps its address whatever the geometry, so a
narrower screen stops addressing what lies past its edge and a wider one
addresses it again -- xterm truncates where baud hides. What is given up
by declining to re-cut a region is that a long line in it is not visible
past the edge until the window widens again; nothing is destroyed.

## What baud does not have yet

* **`OSC 133`** is not implemented -- no prompt marks, no semantic
  zones. It is the boundary this design needs, and it is separately
  worth having for prompt navigation and output selection.
* **DEC private mode 2048, in-band resize notifications**, is not
  implemented. `CSI ? 2048 h` enables it and the terminal then emits
  `CSI 48 ; rows ; cols ; height_px ; width_px t` after each resize
  completes, **in the byte stream**. That removes the race for
  applications that adopt it -- the resize has a defined position
  relative to the application's own output, where `SIGWINCH` arrives
  from the kernel on its own schedule and intermediate sizes coalesce.
  Neovim 0.10+ consumes it and stops handling `SIGWINCH` once it is on.
  It does **not** help the prompt case, because no shell consumes it
  yet; it is the right answer for full-screen applications and should
  be implemented on its own merits rather than as a fix for this.

## Unmeasured, and stated rather than assumed

* Whether the prompt-scoped re-cut behaves on **`bash`**, whose prompt
  marks are emitted differently, and on a shell with no marks at all.
* Whether Terminal.app's duplication has the same cause, given it does
  not reflow. Until that is understood, "not reflowing fixes it" is a
  claim about baud on these captures and not a general one.
* The **cost** of the re-cut being skipped: nothing here measures what a
  user loses by having a long line clipped rather than wrapped until the
  window widens, and that is a judgement rather than a number.
* Whether a resize should be **coalesced** at all. VS Code needed 300 ms
  because it re-wraps; a terminal that does not re-cut the prompt may
  not need any window, and this repo's own rule is that nothing
  time-based should behave like a debounce. Skipping a resize whose
  computed grid is unchanged is free and correct either way, and baud
  already does it.

## Worth pinning

`ghostty-org/ghostty#209` and discussion 8817 for the clear-on-resize
failure mode; `microsoft/vscode#330040` for the debounce and the
row/column split; `romkatv/powerlevel10k#1200` for the shell's own
statement of the problem; the in-band resize specification for mode
2048. None is cited as a source above and none may be until it is in
`MANIFEST.toml`.
