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

## Both halves are built

* **`OSC 133`** is implemented and is what the design above needs. `A`
  and `P` open the prompt zone, `B` keeps it, `C` closes it, and
  `Term::resize` re-cuts a width change only outside it. `P` is taken as
  a prompt start and not only `A`, because the shell this was measured
  on sends `P` and never `A` -- reading only `A` is how a terminal
  decides a shell sends no marks at all. Measured after: the gentle
  capture goes four copies to one and the drag two to one, with every
  reflow, resize and scrollback test passing untouched, since a stream
  with no marks is in the output zone and behaves exactly as before.
* **DEC private mode 2048** is implemented, on its own merits and not as
  a fix for this. `CSI ? 2048 h` enables it and the terminal emits
  `CSI 48 ; rows ; cols ; height_px ; width_px t` on enablement, on
  re-enablement, and after each resize completes. It carries what the
  terminal *has* rather than what was asked for, because the clamps in
  `resize` are real. The pixel fields are zero, which the specification
  requires of a terminal that cannot report them: this core is handed a
  grid and never a window, and `CSI 14 t` is not implemented either.

What is still open from this note is the refinement rather than the
fix: re-cutting the output *above* a prompt while leaving the prompt's
own rows alone, rather than leaving the whole screen alone whenever a
prompt is on it. That needs the re-cut to anchor at the prompt and grow
upward into the history, and the in-place copy ordering argument for it
is not the one `View::reflow` currently carries -- so it is a change to
make deliberately, with that argument written out, rather than a flag.
What it would buy is a long line in finished output re-wrapping while
the user sits at a prompt; today that waits until the next command's
output, which is in the output zone and re-cuts.

## The contract, from the shell's own author

Everything above was reasoned from what the screen did. The shell says
what it needs, and it is one sentence. Bart Schaefer, on zsh-workers,
on prompt redraw after `SIGWINCH`: the prompt "is drawn based on a
calculation of the width of each line relative to the width of the
window, and the number of lines derived from that ... the rest of the
screen contents are unknown, and the vertical position (how far above
the last line the prompt is starting) is based on what ZLE knows." zsh
keeps **one number** -- rows from the prompt's first line to the cursor,
computed from its own text and `COLUMNS` -- moves up that many, erases
to the end of the screen, and redraws.

So the contract is: **across a resize, the distance from the prompt's
first row to the cursor must not change, and the prompt's rows must not
re-wrap.** The second half is the zone rule above. The first half is
what `Term::prompt_row` exists to measure: the block's first row is
recorded at the mark as a ring row, so it follows its cells through a
scroll and through a resize's rotation arithmetic, and
`the_prompt_to_cursor_distance_holds_across_every_resize` checks the
distance around every `resize` call alone, with nothing fed, on every
recorded gesture. It holds on all three today -- the shrink retires
rows from the top and the growth pulls them back, and both move the
cursor and the block together -- which says the orphan on a fresh
terminal is not the resize arithmetic on those fixtures. `BAUD_TRACE`
records the app's own session, resizes and chunks in the order the core
saw them, so the fixture that reproduces the recording *is* the
session rather than a reconstruction of it.

## What each side knows, re-read from the sources

* **kitty** states its feature in one sentence: "Glitch free window
  resizing even with complex prompts. Achieved by erasing the prompt on
  resize and allowing the shell to redraw it cleanly." Its own zsh
  integration marks the PS2 line with `OSC 133 ; A ; k=s`, and it
  defines `redraw=0` as "the shell will not redraw the prompt on resize
  so it should not erase it". The erase is at the *resize*, the shell's
  redraw follows.
* **Ghostty** rebuilt the same thing (PR 10455): a two-bit flag per
  row, `prompt` for the line an `A` opened and `prompt_continuation`
  for one a `k=s` or `k=c` opened; a `promptIterator` that walks up to
  the block's first row; and "clearing the prompt lines is now built-in
  to resize", taken when the cursor is not on command output, from the
  first prompt row down through its continuations. Its parser accepts
  `k=i|s|r|c` and `redraw=0|1|last`, and `last` exists for one shell.
* **bash**: readline handles `SIGWINCH` only when `read(2)` returns --
  Chet Ramey, bug-readline: "Since SIGWINCH doesn't interrupt a read(2)
  call, readline doesn't get to handle the signal and resize the window,
  performing redisplay along the way, until the read returns." A bash
  prompt is not redrawn during a drag at all, only at the next
  keystroke, and only its last line ("redraw=last": "the shell supports
  redrawing only the LAST line of the prompt"). A terminal that erased
  a bash prompt's earlier lines would erase them for good.
* **fish** carries the shell-side mitigation: `fish_handle_reflow`
  decides "whether fish should try to repaint the commandline when the
  terminal resizes. In terminals that reflow text this should be
  disabled." Its own duplicate-prompt issue (2320) reached the same
  diagnosis this note did from the terminal side -- "we're
  soft-wrapping from the previous line, and need to have a hard return
  there" -- and the repaint is turned *off* under reflowing terminals
  because there it "just gets in the way". Ghostty's integration sets
  it to 1, having taken on the erase.
* **The line editors** that send a prompt mark and do not redraw --
  reedline is the documented case -- lose their prompt under Ghostty's
  erase, which is `#209` in general form: an erase at the resize is a
  promise the terminal makes on the shell's behalf.
* **Academic treatment**: none found. Three searches over reflow,
  resize and prompt redraw return bug trackers and mailing lists only;
  the closest formal analogue is CSS scroll anchoring, which holds an
  anchor node across a layout change -- the terminal's anchor being the
  prompt, which is the contract in other words.

What that settles for baud: the `k=` kind rides on the mark
(`Semantic::PromptStart(PromptKind)`), the block start is recorded on
the *initial* line only
(`a_continuation_mark_keeps_the_block_start_where_the_initial_one_put_it`),
and any erase baud ever does must be of exactly the recorded block, on
the shell's own redraw mark rather than at the resize, and never under
`redraw=0` -- which is the form that cannot orphan bash's prompt or
reedline's, because it acts only after the shell has said it is
redrawing. That erase is built, and the recorded gestures named two mechanisms
for it before the live trace arrived:

* **A burst.** `zoom`, step 3: two resizes with nothing fed between
  them, then two of the shell's redraws landing together. The first
  was computed for the width the shell had at the first `SIGWINCH` --
  its prompt lines padded to 70 columns -- so on the 55-column screen
  they wrap, the block grows by two rows, and the second redraw's
  `CUU 4` lands below where the first began; its `ED` misses the top.
  Each redraw honours the contract for the width it was computed at;
  the *stream* is stale relative to the size. At the second `P` the
  terminal erases the recorded block, `[start .. cursor]`, and the
  orphan is inside it. `zoom` goes two to one.
* **A window too short for the block.** `drag`, step 146: at four
  rows the shell's `CUU n` stops at row 0, each redraw scrolls the last
  one into the history, and a later bare growth pulls them back --
  `0 bare resizes broke the contract`, and the copies appear on a
  growth with nothing fed. The recorded start is a ring row and is
  still the block's first row behind the window, so at the redraw mark
  **the window moves back up over the block's rows**: the block's
  first row becomes row 0, where the shell's clamped `CUU n` left the
  cursor, every row of the block is erased by ring row wherever it now
  sits, and the blank rows the window gave up at the bottom -- the
  shell's own `ED` just made them -- become hidden. Nothing the
  terminal manufactured is left in the history. A first version
  erased the rows behind the window where they were and called the
  blank row a growth then pulled back "residue"; the owner's recording
  of 2026-09-18 -- a fresh terminal's prompt at the bottom of a
  same-sized window under a screen of blank rows -- is that residue,
  and it was not acceptable. `drag` goes five to one, and the matrix's
  hand model is asserted at one rather than pinned at five.

Two more things the fresh-terminal fixtures (`fresh-drag`,
`fresh-zoom`: the same size sequences with the shell started where
nothing prints before the prompt) then required, each with its test
red first:

* **`D` closes the prompt zone.** Enter on an empty line is `B`, the
  newline, `D` and a new `P` -- no `C`, since nothing ran -- and a `P`
  in the prompt zone read as a redraw and erased the block above it:
  "when pressed enter prompt disappear for a few lines". After `D` the
  screen holds finished output, and so does anything a `precmd` hook
  prints before the next mark, which is the Ghostty failure not made.
* **A growth pulls content before it reveals blank cut rows.** xterm
  reveals the rows a shrink cut below the cursor before it pulls
  anything from the history, whatever they hold; a fresh terminal's
  banner then stayed retired while blank rows filled in under the
  prompt, and a burst's silent growth never reclaimed a block's own
  scrolled-off rows. The cut rows come back as far as the last that
  holds anything, the history is pulled next, blanks last -- the
  owner's rule from the design discussion, "pull down to the bottom,
  and blank-fill only when nothing is left to pull".

And a third, from the recording of 2026-09-18 09:09, which the burst
fixture could not show until the burst was captured as the app makes
it -- the sizes a display-link tick apart (`capture-resize.py --gap`),
so the shell redraws once per size rather than once per burst:

* **The mark puts the cursor on the block's first row.** The shell's
  `CUU n` counts the rows it believes its prompt has; a redraw computed
  for a wider screen counts one fewer than the wrapped block on this
  one, lands below the block's first row, and the new prompt was drawn
  there -- one row lower per stale redraw, blank rows above it, the
  block four rows under the one before it after a drag. The erase
  removed the copy and not the drift, and nothing asserted position.
  The terminal knows where the block starts, and its row is the one
  that survived the width change (`fresh-burst`; the whole starting
  screen, then Enter twice, a drag in and out, Enter again).

Then the live trace itself -- `BAUD_TRACE`, the owner's session as the
app fed the core, 767 steps, `tests/resize/live-drag` -- reproduced
the screenshot of 09:31 headlessly and showed the fourth thing, at one
mark in 767: at 21x3 the retreat was refused because the rows the
shell's `ED` had just cleared carried *marks* from a width of a
hundred columns, and the guard asked the mark rather than the cells.
`winch` prints each redraw mark's effect on the cursor, the history
and the hidden rows by feeding the step in two halves; a counter
encoding the guard's inputs, run once and removed, said which input.
`View::row_is_blank` asks the cells within the live width, which is
the question a retreat asks -- may this row leave the screen.
`no_more_than_two_blank_rows_come_between_consecutive_prompts` is the
symptom's own test, width-independent so a drag that returns to none
of its sizes is still checked.

`a_block_never_moves_once_it_is_drawn`,
`no_more_than_two_blank_rows_come_between_consecutive_prompts`,
`a_redraw_mark_puts_the_cursor_on_the_block_it_replaces`,
`a_redraw_computed_for_a_wider_screen_does_not_move_the_block_down`,
`the_prompt_row_is_where_the_mark_was_and_follows_the_screen`,
`a_redraw_of_a_block_taller_than_the_screen_takes_its_rows_back_from_the_history`,
`an_enter_at_the_prompt_leaves_the_old_prompt_where_it_was`, the three
`a_fresh_terminal_*` recorded tests with exact step-count controls, and
the matrix's fresh-terminal drags are the gates.

`winch <prefix>` is the instrument: per step, the prompt row, the
cursor, their distance across the bare resize, the count by the
recording's own signature, and the screen at the step the count rises.

## Unmeasured, and stated rather than assumed

* Whether the prompt-scoped re-cut behaves on **`bash`**, which redraws
  nothing until a keystroke and then only its last line, and on a shell
  with no marks at all. The contract test cannot see either: no fixture
  here was recorded from them.
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
