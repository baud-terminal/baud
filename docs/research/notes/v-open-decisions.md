# V. The owner's open decisions, researched

Taken at `0987b98`, 2026-09-25. **Nothing in this note is built.** Each
decision waited on the owner's answer; the answers are the next section.

## Decisions taken, 2026-09-25

The owner's answers, in their words where they gave any. Each overrides the
section's recommendation where the two differ.

- **A standing rule.** "We need to support such protocols fully, always ...
  we want baud to be the best VT." A half-built protocol feature is finished
  to the spec, not switched off. A refusal stands only where answering is
  itself unsafe, as with clipboard reads.
- **V10: F.** Finish left and right margins: a full VT420-class terminal.
- **V13: copy-on-select on by default**, with a config key to turn it off.
  The selection colour is as recommended: a setting that defaults to reverse
  video, chosen after a side-by-side look.
- **V12, V14, V18, V19: built to the spec**, overriding their
  recommendations. `?1001` highlight tracking, the ambiguous-width switch
  with `?1021`, the Sixel modes `?80` and `?1070`, and the kitty graphics
  protocol.
- **V16: research first.** The owner's words: "the terminal has LS_COLORS I
  think or something like that, what other VTs are doing? as I know there
  are a bunch like base16/24/ansi/etc and I have also vogix16, so maybe
  research here is needed." The `[palette]` recommendation is withdrawn
  until that research is done.
- **V24: not yet.** Nothing is deleted.
- **V23**: the corpus question had no default and is still open.
- **Every other decision: as recommended.** V1-V9, V11, V15, V17, V20-V22
  and V25.

Two requests arrived with the answers, and are not decisions from this note:

- **The cursor trail on by default**, with `[cursor] trail = false` to turn
  it off.
- **The cursor's shape and blink set in the config**, blinking block by
  default. The core has defaulted to a blinking block all along; no renderer
  path drew the cursor's blink then (it has since 2026-09-25, `d6bab1b`).
  SGR 5 text had blinked since 2026-09-13 (`d28e6f9`); that is not the
  cursor.

Later the same day the owner answered the cursor's questions, and these steer
the builds that follow them:

- **Every trail style is config**: kitty's four-corner quad, sfte's fading
  swept box, baud's first ghost, and a fire trail drawn on the GPU, which is
  **the default** -- chosen over the recommended quad. On the CPU path,
  `fire` draws the quad, and Linux's driver draws no trail at all
  (`baud-app/src/linux.rs`). **Superseded on 2026-09-29**: the owner made
  the plasma the default, replacing the fire (built 2026-10-10; the record,
  "The plasma is the default trail, and the fire stays by name").
- **The flame is opaque over glass**, and the light theme gets a darker
  variant that is not additive. **Met by one rule since the fire was built**
  (`2dcb9b5`): the flame is drawn in the cursor's colour, premultiplied, with
  its heat as its opacity, which "is valid over a glass window by
  construction ... and is dark on a light theme, whose cursor is dark, so the
  two variants the owner asked for (opaque over glass, darker over light) are
  one rule" (the record, "The fire trail is a flame in the cursor's colour,
  held to a Rust reference"). Every effect trail draws by that rule
  (`baud-render/src/effect.rs`).
- **Reduce Motion means no trail**, unless the config names one.
- **The trail rides smooth scroll** by default, with an option to stay put.
- **The blink reads the OS**: macOS's per-phase insertion-point keys, and
  AppKit's 650 ms on, 350 off when they are unset; the system's
  non-blinking-caret switch is honoured unless the config says otherwise.
  It never stops by default, with an option to stop after a time.
- **The per-frame cost first**: the resolved rendition table kept as a cache
  in baud's own idiom, then the trail styles one commit each.

Built since: the rendition cache (`fe776c9`), the quad trail (`6872f61`),
sfte's swept box (`trail = "sweep"`, `6700c54`), and the fire trail
(`2dcb9b5`), the default until the plasma replaced it (`898f9e6`) -- drawn in
the cursor's colour, the owner's word mid-build: "the flame should not use a
special color, we use the cursor color, default color". Then plasma
(`e8798ae`), lightning (`dbea478`), ice (`301f589`), ufo (`32042ce`), orb
(`4478fb2`) and liquid (`6daf3d3`), ten trails with the ghost: the one-cell
box that was the trail before these decisions (`c77d2ef`, 2026-09-20), named
`ghost` by the quad's commit and drawn in the cursor's colour since `c632ae3`.
Then Reduce Motion (`336d269`): the default trail is off under it and a named
one is kept (CLAUDE.md, "Reduce Motion means no trail, unless the config names
one"). Then the blink from the OS, with the owner's answers of 2026-09-27
(`9c4769d`) -- hard edges at 650/350, every other sub-question as recommended:
the rate read from the system's keys, macOS 15's non-blinking switch holding
the drawing steady unless the config says `true`, and
`[cursor] stop_blinking_after`, never by default (CLAUDE.md, "The cursor blinks at the
system caret's rate"). Then rule F (`2b54cfd`), the owner's call the same day:
`?12` or-ed with the style's own blink, as foot does, and the style saved and
restored across `?1049`, so vim and helix leave the shell its configured
cursor (CLAUDE.md, "`?12` is a second input to the blink"). Then the ride
(`cd5bc29`), with the owner's bound of the same day -- two rows, then fold in:
a trail in flight moves up with the text under a smooth scroll, and `[cursor]
trail_scroll = "stay"` keeps it put (CLAUDE.md, "A trail rides a smooth
scroll, two rows at most").

**On the `TODO.md` citations below (2026-10-10).** `TODO.md` is the owner's
work list, gitignored and never committed, so no reader of this note can open
it, and its line numbers have moved since. The seven citations that gave it
by line now name the committed document that holds the finding where one
exists; where one named a step on the work list, the step is described and
the line numbers dropped. The plain mentions of "the TODO list" record what
the list said on 2026-09-25 and stay as written.

## What this is

The TODO list and the research notes leave these twenty-five decisions to
the owner. Each one here went through three steps:

1. One agent read the code and the sources, and proposed an answer.
2. A second agent tried to refute the proposal.
3. A third wrote the section below. Where the first two disagreed, the
   reviewer's correction is taken.

Every section has the same parts: the question, what you would notice, how
it is today, what other terminals do, the options, a recommendation in bold,
a design, the tests and measurements it owes, the effort and risk, and what
would change the answer.

**Two kinds of claim, kept apart.**

- **"By reading".** Most "today" claims were traced through the code at
  `0987b98` and not run. They are labelled "by reading" or "not run" where
  they appear, and each test in a design must go red before its effect is
  claimed.
- **UNVERIFIED and UNPINNED.** A claim marked UNVERIFIED was not checked. A
  claim marked UNPINNED rests on a source the manifest does not hold. Every
  cited source key is in `docs/research/sources/MANIFEST.toml`.

**Numbering.** V1 to V25 are this note's numbers, in reading order. Two other
numberings appear inside the sections:

- Bare D-numbers are the rendering register's, in
  `docs/research/notes/j-rendering.md`. D13 is its decision to speak
  Wayland's wire protocol with no libwayland. D14 is frame pacing.
- "Note U question n" refers to
  `docs/research/notes/u-resize-and-the-window.md`.

The window part and the reset part both first wrote up the reset counter.
The reset part's version is the reviewed one: the field goes in `Book`, and
the effort is M, not S. So V1 is that version, and the other was dropped.

## How to answer

The decisions fall into four groups. The lists below are ordered so that
what blocks other work comes first.

1. **Decide these.** Eight have options that differ in what you would get,
   not only in what they cost. A letter or a number per line is enough.
2. **Confirm my defaults.** Sixteen have a clear answer and little cost. One
   word, "accept the rest", takes every default in that table. A different
   answer on any one row is fine. One question in that table, V23's about
   the corpus, has no default and needs its own yes or no.
3. **Needs an explicit yes, because it deletes things.** One: the old backup
   branches and the stash. It is kept out of the batch on purpose.
4. **Waiting on a source I can pin without you.** Pinning a source means
   fetching it and recording its hash in the manifest. That is research, not
   building, and needs no decision. Seven decisions have a pin in front of
   them, listed below. One pin decides an answer (V4); the others only have
   to land before the building starts.

### 1. Decide these

| | The question, plainly | The choices | My lean |
|---|---|---|---|
| V2 | When you scroll up into the history, which part remembers the line you are on? Today the app keeps it, and asks the core after every read and resize how far to move it. Three recorded bugs came from the two numbering lines differently. | **A**: leave it in the app. **B**: the core keeps it, as a line number or "live". **B+**: B plus a public list of tracked points the core moves with their text. | **B.** It gives the view one owner, though not one numbering: the selection keeps the app's own count until B+. The Linux driver gets the anchoring for free and nothing more; its wheel, scrollbar and scrolled-back rendering still have to be built. B+ can come later. |
| V4 | A program saves the cursor with `ESC 7`, the window is resized, then it restores with `ESC 8`. Today the cursor can land on another line's text, and the program writes over output. Should the saved cursor follow its text? And how many saved slots are there? | Follow the text, with either **(i)** one slot per screen, with the `?1049` save merged into the main screen's slot, as ghostty and alacritty do, or **(ii)** three slots, which keeps today's restore behaviour. Or keep today's clamping, alacritty's: no code, and the restored cursor can still land on another line's text. | Follow the text. **(i)** if xterm's source agrees once it is pinned, otherwise **(ii)**. |
| V10 | Left and right margins (`?69` and DECSLRM, called the side fence below) are half built: a scroll moves whole rows (a recorded gap). By reading, not run, a new line also ignores the fence, and text written right of the fence is placed differently from xterm. Switch the feature off, or finish it? | **W**: switch it off. It is small and reversible, and programs are then told the feature is unsupported. **F**: finish it. That is large: an oracle harness, line starts that honour the fence, then the corner in its reduced form (C2-min, never C2 alone). Rectangle scrolling is its own decision and is not designed yet. | First run neovim in baud with `:vsplit`, scroll one window, and look. Then **W**, unless you want baud to be a VT420-class terminal. |
| V11 | Should a program inside baud -- remote neovim, tmux over ssh -- be able to put text on your Mac clipboard with OSC 52? Today the copy is dropped with no sign. Reads stay refused either way. | Off by default, as xterm's man page (unpinned) and iTerm2 (unverified) are. Or on by default with an off switch, as kitty and sfte are (pinned). Either way, an interim that honours copies up to about 380 bytes can ship first. | **On by default**, with `[clipboard] osc52 = "off"` to turn it off. No focus gate until one is tested over a real slow ssh link. |
| V13 | Two questions. Under glass, reverse video draws a selection as a bright opaque slab. And should finishing a drag also copy? | Colour: reverse video (today), the macOS selection pair (the system's text and background colours), or the system background under each character's own colour. Copy-on-select: off, or on. | Copy-on-select **off** by default, with an opt-in. Colour: build a selection setting with reverse video as its default, then choose after a side-by-side look at Terminal.app and iTerm2. The lean is the macOS pair. |
| V15 | baud tells programs it supports emoji-width mode 2027, and does not widen a heart followed by U+FE0F as that mode requires. | **Step 1**: answer "permanently off", a one-line honest fix. **Step 2**: build a real mode, either off at power-on (**D**) or on at power-on (**D'**). | **Both steps, with D**, including rule G: a ZWJ-joined or skin-toned emoji after a one-column base also widens. Your zsh counts a heart plus U+FE0F as one column, so off is the default that matches your shell. |
| V16 | In light mode, xterm's green and cyan are nearly invisible on near-white: GNU `ls` shows executables at 1.19:1 contrast and symlinks at 1.09:1. Give light mode its own sixteen colours? | Leave it to programs, through OSC 4. Add a `[palette]` section with a light and a dark list, defaults unchanged. Ship a new default light set. Or a contrast floor at draw time. | **The `[palette]` section, defaults unchanged.** Decide the light glass dim, `dim_light`, before or alongside this: it changes more of what you see than any palette. For the commented example set, lean "balanced", about 3.6:1 both ways. |
| V20 | Confirm the 12 September platform calls: call 2, Vulkan only for the GPU on Linux; call 3, X11; call 4, Windows. | Calls 2 and 4: confirm, scoped to a first release. Call 3: **(a)** Wayland only for v1, **(b)** a native X11 backend later, or **(c)** native X11 in v1. | Confirm 2 and 4. For 3, your 12 September answer was "XWayland fallback", whose one coherent reading is (b). My lean, **(a)**, changes it to Wayland only for v1, with a clear exit message on an X11-only session in place of today's raw error. Drop the words "XWayland fallback" whichever you pick. |

### 2. Confirm my defaults

| | The question | The default I would take | If you would rather |
|---|---|---|---|
| V1 | After `reset` (`ESC c`) while scrolled back, where the view lands depends on how the bytes were split into reads. A selection survives the reset, and a smooth-scroll slide can draw a line twice for up to 60 ms. All by reading, not run. | Add a reset counter to the core. When it moves, the app drops the selection, settles the slide, clears pending predictions and goes to the live screen. Build it now; it does not wait on V2. Inside it: reveal the scrollbar on that snap (yes), and fade as a screen switch does (no). | Keep the selection across a reset, which drops one line of the design. |
| V3 | Select some text, then make the window taller: today the selection is dropped. | Keep it across a height-only resize on the main screen. Clear it on a width change, on the alternate screen, and where it touches rows a shrink cut. | Clip the selection at cut rows instead of clearing it. |
| V5 | While a command is printing (not at a marked prompt), the re-wrap on a width change runs at once and history is 2,000 lines, so dragging very narrow can drop the oldest lines. At a prompt nothing is lost. How many a real drag drops is unmeasured. | Keep both. Count the rows a re-wrap drops, and commit a full-history drag fixture that measures a drag step. | |
| V6 | Which rule decides the top line when the window changes height ("rule 3", today's). | Confirm rule 3. | Rule 1 for growth, kitty's default: a taller window adds blank rows at the bottom instead of bringing history back. |
| V7 | Should old output re-wrap while you sit at a marked shell prompt? Today it is cut and comes back whole when you widen. | Not now. Revisit after V2, V9 and V5's fixture. | Build it: a large change. |
| V8 | A resize and a `SIGWINCH` on every drag step, with no debounce. | Confirm. | |
| V9 | What must a row hidden below the screen hold to come back when the window grows? Today's test misfires both ways. | A written character, where a space counts and colour alone does not. First compare the seven recorded gestures to confirm which resizes change. | Also count a background the program set, so rows erased in a background colour (the red band) come back. Rows erased in a foreground-only colour still do not. |
| V12 | baud tells programs `?1001` highlight tracking is on, and draws nothing. | Stop claiming it. Count `?1001 h` and answer DECRQM with 0, the house convention for a mode baud refuses. | Build it. No user is known. |
| V14 | Should config.toml offer a switch to draw East Asian ambiguous-width characters (box lines, Greek, Nerd Font icons) two columns wide? | Fix the stale docs now. Build the switch only when a user who needs it asks. Keep the default narrow: this Mac's C library and your zsh both count these characters one column. | Build the switch now: medium effort. It also means deciding, in the same change, whether "wide" covers Nerd Font icons (Private Use, 99% of the class). |
| V17 | While the screen is reversed (`?5`), OSC 11's reply names the colour drawn as text, so a program checking for a light background gets "dark" on a white screen. | After I pin xterm's source, make OSC 4, 10 and 11 set, query and reset the colour you see, as xterm does. | The majority's model (foot, wezterm, ghostty): undo the palette swap baud already draws. Under it, a program asking OSC 11 on a flipped screen still gets the text colour, so the light/dark answer stays wrong. |
| V18 | What should DECRQM answer for the Sixel modes `?80`, `?1070` and `?8452`? | Answer them as permanently fixed while Sixel is claimed. Refuse and count requests for the other state, in new Sixel counters. Build neither display mode nor shared registers. | Keep one counter (`unsupported_param`) for both directions: cheaper, but chafa's harmless requests still read as failures. |
| V19 | Kitty graphics: the documents say a PNG decoder cannot live under the core's rules, and baud-font already has one. | Kitty stays parked. Fix the six stale places now, and record the design for unparking as depending on measurements. | Unpark it: large. |
| V21 | When the unit tests fail before a push, the five checks after them never run, so one push shows one broken category. | Let the later checks run after a failure, with the first four kept hard. In the same change, guard the one trap this creates: if a gate is ever run without being named, a red unit test would exit 0. (Moving the path check after the format check is a separate yes; lean yes.) | |
| V22 | Offer a scale-the-font resize mode beside reflow? | No. Keep reflow, keep Cmd+ and Cmd- for text size, and close the item. | A config key, for demos and screen recordings. |
| V23 | A CLAUDE.md table rests on two long recordings that were never committed and are lost. | Do not re-capture. Mark the table "from captures no longer held; ordering only", point to cat-scroll's figures, and fix one sentence. | Not covered by "accept the rest": a separate yes or no, with no default, on whether you want recordings of paging through code, on the alternate screen, in the corpus. |
| V25 | Pursue a decode-then-write split of the ingest path? | Close it as a direction. `put_run` already decodes before it writes, and the single-character decode was priced at 0-0.9% on earlier builds of this core, before GB11 and the 64-byte alignment. | A 30-minute re-price when the machine is free. |

### 3. Needs an explicit yes, because it deletes things

**V24.** Bundle `backup-pre-squash-smooth-scroll` into one file outside the
repository and verify the bundle. Then:

- delete all five backup branches, including the bundled
  `backup-pre-squash-smooth-scroll`, plus `backup-pre-fixup-squash`,
  `backup-pre-split-ris-prompt`, `backup-pre-trailer-rewrite` and
  `sfte-oracle`;
- drop the August stash;
- delete the twelve branches already merged into `master`: `row0` to `row10`,
  and `note-n-and-gate-audit`.

Each was checked read-only; section V24 has the detail. The one branch with
content that exists nowhere else is 58 KB of pre-squash commit messages,
which is why it is bundled rather than deleted. Deleting
`note-n-and-gate-audit` on origin is a separate call and is not included.

### 4. Waiting on a source I can pin without you

| Decision | The pin it waits on | What the pin does |
|---|---|---|
| V4, the saved cursor | xterm's `cursor.c`: does xterm keep one saved cursor per screen, or a separate `?1049` save? | decides (i) against (ii) |
| V10, only on the finish branch | ghostty's `Terminal.zig` at d9840f3c, and xterm's `cursor.c` (the same pin as V4's) | both pinned: [[source: xterm-cursor-c]], and [[source: ghostty-d9840f3-terminal]] committed 2026-10-12, the commit the oracle's libghostty-vt was built from |
| V11, the clipboard | RFC 4648, base64's standard | lands before building |
| V13, the selection colour on macOS | the NSColor selection colours, from the local SDK header | lands before building |
| V17, colours under `?5` | xterm's `misc.c`, where OSC 4, 10 and 11 are handled | pinned ([[source: xterm-misc]]); OSC 10/11 built 2026-10-11; OSC 4 needs no mapping since 2026-10-14, when DEC's per-cell exchange for `?5`, the owner's choice of 2026-10-13, was built (the record, "`?5` exchanges every cell's colours") |
| V15 step 2, the real mode 2027 | the mode 2027 draft, and Unicode's emoji variation sequences file | lands before building |
| V14's `?1021` | xterm's man page and `misc.c`, only if the switch is built and `?1021` is answered, which the lean leaves out even then | pinned ([[source: xterm-man]], [[source: xterm-misc]], [[source: xterm-wcwidth-c]]); read 2026-10-15 under "V14's open questions, answered" |

## What blocks what

- **V1 first.** Nothing blocks it. By reading, it must land before V2's
  differential test uses the app's anchoring as its oracle; otherwise V2 is
  built to match today's reset behaviour. Under V2, the reset
  sets the view live explicitly.
- **V2 before V3, V7 and the Linux wheel.** V3 and V2 both change the same
  resize code, so V3 goes after V2 and the code changes once.
- **V6 before V9.** V9 does not need V2 and can land first.
- **V5's fixture feeds V7 and V8.**
- **V4 waits on its pin.** Within it, the per-screen slot comes before the
  remap.
- **V10 waits on the neovim look.** On the finish branch, the harness comes
  first. Three pending-wrap items are separate later decisions left out of
  the finish branch, and they do not wait on it: PendingAt, a pending wrap
  surviving save and restore, and the full-width follow-ons.
- **V16 goes with or after the light glass dim.** On Linux, "auto" needs an
  appearance signal the driver does not have.
- **V17 waits on its pin.** V15's step 2 waits on its pins, and on two
  existing problems, found by reading, being fixed first.
- **V11 and V13 on Linux** both wait on a Wayland data device. V13's
  copy-on-select also needs the pointer the Linux driver lacks.
- **V11 and V19** would share one incremental base64 decoder.
- **V20's Vulkan work** comes after Linux pixels and pointer input.

## Part 1. The scroll position, a reset and a resize

### V1. What a full reset owes the app

**The question**

A program can reset the terminal with `ESC c`. The spec calls it RIS, "reset
to initial state", and it is the two bytes the `reset` command sends. baud
then erases the screen and, since 2026-09-23, the whole scrollback (the lines
that scrolled off the top).

The app keeps three things of its own on top of the text, and none of them
learns that the reset happened:

- your **selection**;
- a running **slide**, the smooth-scroll animation, which is on by default;
- the **window**: how far up the history you have scrolled, stored as a row
  count `back`.

So there are two questions:

1. Should a reset drop the selection, stop the slide, clear any pending
   typed-ahead guesses, and put the view back on the live screen?
2. Should that be decided from the reset itself, rather than from how its
   bytes happened to arrive?

A **read** is one chunk of bytes the terminal takes from the program at once.
The same output can arrive in one read or several, depending on timing.

**What you would notice**

Today, there are four effects. Each is traced by reading the code at 0987b98;
none has been run. Each test below must go red before its effect is claimed.

1. **The highlight outlives the reset.** Select a word, then type `reset`. The
   screen goes blank, but the highlight stays over the blank cells, and Cmd-C
   copies whatever lands there next.
2. **The slide draws a line twice.** Take `seq 1 45; reset` arriving in one
   read:
   - that read scrolls 26 lines, so the app starts a slide;
   - but the history is now empty, so the row the slide draws above the screen
     is the screen's own first row;
   - so for up to 60 ms (`SLIDE_SECS_PER_ROW`) the cleared screen slides as if
     it had scrolled, and its first new line appears twice.

   `tests/reset/seq-reset.raw`, replayed as one read, is exactly this shape.
3. **A scrolled-back view lands in two places.** If the reset and the output
   after it arrive in one read, you land on the first line after the reset and
   stay pinned there. If the reset arrives alone, you land on the live screen.

   Same bytes, two outcomes. That breaks, at the app level, the rule the core
   is held to: *chunk invariance*, meaning the answer must not depend on how
   the bytes split into reads.
4. **A stale guess lingers (rare).** A *local-echo prediction* is a typed
   character baud draws before the shell echoes it. One still waiting when the
   reset lands is compared with a blank cell, which counts as "still pending".
   So it stays drawn over the reset screen until it expires after 150 ms
   (`PREDICT_MAX`).

*Updated 2026-10-09:* effects 2 and 3 are closed, by the `CSI 3 J` change
rather than by V1. The core counts every erasure of the history
(`Term::history_erasures`, moved by `CSI 3 J` and by `RIS`), and when a read
moves it the app puts the window on the live screen and settles the slide,
however the bytes fell into reads (`after_read`, `erased` in
`baud-app/src/app.rs`). Effects 1 and 4 -- the highlight over the erased
screen and the stale guess -- are V1's still: the screen's rows are not saved
lines, so the clip that drops a selection in the history keeps one on them.

*Built 2026-10-11:* effects 1 and 4, and the two forks as answered. The core
counts every `RIS` and nothing else (`Term::resets`, a `Resets` count in
`Book`, because `Cold`'s pinned 464 bytes had no padding left), and the app's
`restart` (`baud-app/src/app.rs`), called by the pump after the erase arm,
drops the selection, drops the guesses as contradicted -- the switch's bare
clear with them -- clears their frame and settles the wheel's part-line when
it or the screen epoch moves, and reveals the thumb when the reset took a
window over the history to the live screen; the fade stays the switch's. The
window and the slide stay with the erasure count, which `RIS` moves too, so no
line of `restart` repeats them. Not built: the recorded level's new capture
(the window half it was for is the erasure count's, held at every read size
already), and the candidate below, a slide settled on a `Restored` read. The
record has the tests and the measurement: "A full reset drops what the app
keeps over the screen".

*Corrected 2026-10-11:* the reveal was read at the pump's edges, so
`?1049h` then `RIS` revealed in one pump and not in two. It is read at the
reset's byte now: the first erasure in a pump, kept by the core
(`Term::first_erasure`), reveals when it is a reset made from the primary
with the window above 0. The window is still the one stored at the pump's
start, so a full-screen `SD`, `RI` at the top or a prompt redraw that
takes it live in the same pump as the reset still depends on the
batching (note V's V2); the record names it. Q2 is not decided by it.
Both options are now batching-invariant, so the request round 2026-10-11d
sent describes neither as built: its DRAWN_WINDOW Does slot and
`baud_probe` (d) give fbc69f0's batching dependence, and a re-ask must
carry this change's rule. The record: "A reset's
reveal is decided at the reset's byte, not at the pump's edges
(2026-10-11)", which also holds what an emptied history shows: Jev's
answer A, as built (decisions lines 453-454).

After the change: a reset leaves a clean slate, with no highlight, no slide,
the live screen and no stale guess. It does so however the bytes fall into
reads.

Why this is open now: all four used to be handled by accident. The old reset
rewound the ring's *rotation* (the offset that says which stored row is the
top of the screen). The app read that rewind as "rows restored" and dropped
the selection, and the same rewind tripped the prediction check. Commit
5e00c08 correctly kept the rotation, and the accidents went with it.

**Today**

Core: `hard_reset` in `baud-core/src/ops.rs` (lines 1277-1382):
- leaves the alternate screen first;
- zeroes the history counts (`book.hist`, `book.hidden`);
- erases the whole primary ring and the parked alternate surface;
- keeps the rotation and the primary's top line.

So on a reset issued from the primary, nothing the app compares across a read
moves. That covers `moved_since`, `history_mark` and `history_moved_since`,
`screen_epoch` (which moves only on a real screen switch) and
`region_scrolls`, all in `baud-core/src/term.rs`.

The only trace is `scrollback().len()` dropping to 0. That drop is invisible
if there was no history, and output after the reset refills it.

App: `baud-app/src/app.rs`.
- `pump` snapshots `Before` (line 1867), then feeds the read.
- `live_moved` returns `Retired(0)` across a reset. So nothing is renumbered
  or dropped, and the `Retired` arm may start a slide (line 3091).
- `after_read` calls `anchored` (lines 1850-1861 and 1990-1993):
  - a reset alone gives `back` = 0;
  - a reset plus k scrolled lines in the same read gives `back` = k.
- The selection clip drops only the lines above the new oldest line.
- `sliding` (line 1134) needs only a non-zero pan. Its leaving row is
  `view(back + 1).row(0)`, and with an empty history the clamp `looking_back`
  makes that the screen's own row 0.

**The alternate-screen round trip is already handled.** `switched` (line 1932)
reads `screen_epoch`. Across a switch it drops the selection, clears
predictions, settles the slide and fades the new screen in.

A reset issued *from* the alternate screen also moves the epoch, so those
three are covered there too. What is not covered is the primary's window.
`snapped` (line 2011), following foot, keeps that window while the alternate
screen is up, and after a reset it survives into a history that no longer
exists. It then lands by read boundaries, as in effect 3.

Two smaller gaps sit in the same code.
- **Predictions are cleared without a verdict.** The switch block empties them
  with a bare `clear()`. `judge` and `expire_predictions` instead record each
  one as contradicted (`Judged::Contradicted`, `baud-app/src/latency.rs`). So
  under the keystroke-latency instrument (`BAUD_KEYLAT`), that key's record
  waits for the instrument's own timeout.
- **The Linux driver has no window, selection or slide**
  (`baud-app/src/linux.rs`), so none of this applies there yet.

The TODO list records this item as waiting on note U's questions 1-3, which
ask who owns the scroll position. It need not wait; see the last section.

**What others do**

What happens to the selection on RIS:

- **foot: pinned end to end.**
  - `ESC c` calls *term_reset(term, true)* (vt.c 481,
    [[source: foot-vt-ris]]).
  - Its hard path calls *selection_cancel* and sets *normal.offset =
    normal.view = 0* (terminal.c 2178-2179, [[source: foot-term-reset]]). So
    the selection goes and the view is live.
  - DECSTR, the soft reset, is *term_reset(term, false)* (csi.c 1794,
    [[source: foot-csi]]) and keeps the selection on the primary.
- **xterm: strongly indicated.**
  - *CASE_RIS* calls *VTReset(xw, True, True)*, which clears the saved lines
    (charproc.c 6149-6152 and 14372-14375, [[source: xterm-reset]]).
  - The hard branch calls *ClearScreen*, which calls *ScrnDisownSelection*
    (util.c 2366, [[source: xterm-scroll]]).
  - *ScrnDisownSelection* is in screen.c 916-926 ([[source: xterm-screen]]).
    Under *keepSelection*, which defaults to True (charproc.c 453,
    [[source: xterm-reset]]), it calls *UnhiliteSelection*: the highlight
    goes and the text stays for pasting. Otherwise it calls
    *DisownSelection*, which gives the selection up (button.c 5196 and 5234,
    [[source: xterm-button]]).
  - *Corrected 2026-10-09:* this line said the function was in button.c,
    which was not pinned, and gave its behaviour from memory. It is in
    screen.c. Both files are pinned now and match the manifest's SHA-256,
    and the remembered behaviour was right.
  - Where a scrolled-back xterm view lands after RIS is UNVERIFIED.
- **alacritty: strongly indicated.**
  - *reset_state* swaps back from the alternate grid, resets both grids and
    clears the selection ([[source: alacritty-reset-state]], commit d692748d).
  - *Grid::reset* clears the history and sets *display_offset = 0*, the live
    screen ([[source: alacritty-grid-reset]], tag v0.15.1).
  - The two sources are **different versions**, paired here.
  - That `ESC c` reaches *reset_state* is decided in the vte crate, which is
    not pinned. UNVERIFIED.
- **VTE: strongly indicated.**
  - *Terminal::reset*, when asked to clear the history, puts the view on the
    live screen and deselects (vte.cc 10710-10789,
    [[source: vte-vte-cc-0-84]]).
  - Which arguments `ESC c` passes is in vteseq.cc, which is not pinned.
    UNVERIFIED.
- **kitty: strongly indicated, read and not run.**
  - On a hard reset, *do_screen_reset* returns to the main buffer, clears the
    history and calls *clear_all_selections* (screen.c 209-255,
    [[source: kitty-screen-0-49]]).
  - How `ESC c` and `CSI ! p` reach it is in vt-parser.c, which is not pinned.
    So "kitty also drops the selection on DECSTR" is UNVERIFIED.
  - Its scrolled-back offset is not reset; it is clamped at render time. By
    reading, that makes where the view lands depend on render boundaries, the
    way baud's depends on read boundaries.
  - Its Python reset callback was not read, so kitty's view after a reset is
    UNVERIFIED.
- **ghostty: not shown.**
  - *fullReset* switches to the primary and resets the active screen
    (Terminal.zig 4910-4919, [[source: ghostty-terminal-9bbb9b2]]).
  - *PageList.reset* marks every tracked position as garbage, because
    "semantically it really doesn't make sense", and sets *viewport = .active*
    ([[source: ghostty-pagelist-9bbb9b2]], lines 984-1058).
  - The path from `ESC c` through Screen.zig is not pinned, and the pinned
    code shows no selection drop. UNVERIFIED.
- **sfte.** RIS clears the visible cells and keeps the history
  ([[source: sfte-ris]]). Its smooth scroll animates the viewer's own offset,
  which RIS does not touch ([[source: sfte]]), so it offers no guidance on an
  output slide.
- **wezterm, iTerm2, Terminal.app:** no pinned evidence, and nothing is
  claimed about them.

**The direct precedent for an explicit signal is VTE.** Its reset forces its
scroll-position value to -1, with a comment calling this a hack so that its
change handler does not short-circuit to a no-op "even if the value remains"
(bug 730599; vte.cc around 10775). That is baud's exact problem: nothing a
watcher compares changes across a reset. VTE solved it with a signal rather
than an inference.

**The slide has no precedent.** No terminal surveyed animates output
scrolling. The nearest analogies:
- xterm flushes its pending jump scroll before the clear;
- kitty resets its pixel-scroll offset on a buffer switch and on a scrollback
  clear (screen.c 1935 and 2887).

The six external sources (alacritty, kitty, ghostty, VTE) have no committed
copy. The researcher fetched each from its manifest URL, the reviewer fetched
them again, and each SHA-256 matched the manifest both times.

**Options**

- **O1: leave it.**
  - Cost: none.
  - Result: four visible defects stay, and the answer keeps depending on read
    boundaries, which the core refuses.
- **O2: infer the reset from what the app already reads**, such as "the
  history shrank with no Restored move".
  - Cost: no core change.
  - It cannot be made correct:
    - a reset on a terminal with no history changes nothing the app reads;
    - a reset followed by scrolling in the same read regrows the history, so
      the shrink is never seen.

    A guessed signal is the "probe that cannot fire" this repo keeps
    recording, and VTE's hack is the same lesson.
- **O3: count a reset as a screen switch**, by bumping `screen_epoch` in
  `hard_reset`.
  - Cost: small.
  - Problems:
    - a primary reset would fire the alternate-screen fade;
    - it changes the epoch's documented meaning and breaks
      `the_screen_epoch_moves_on_every_switch_and_nothing_else`;
    - it does not fix the window: a real `?1049` round trip must keep the
      primary's window, so the window rule has to tell a reset from a switch
      anyway.
- **O4: a dedicated reset count in the core, acted on by the app (recommended,
  with the reviewer's corrections).**
  - Cost: one field, one accessor, an extraction in the app and an ingest
    sweep.
  - Forecloses nothing: under note U's models it moves into the core line by
    line.
- **O5: wait for note U's option B or B+ and do it all in the core.**
  - Cost: blocked on the owner while the defects stay live.
  - The slide and predictions are app state in every model, so O4's count is
    needed anyway.
  - By reading, B's own clamp-on-exit rule (note U, lines 312-317) reproduces
    effect 3. So B needs an explicit reset rule regardless.

**Recommendation**

**Yes or no: when the core's reset count moves across a read, the app drops
the selection, clears pending predictions as contradicted, settles the slide
and puts the view on the live screen. That holds whether the reset came from
the primary or the alternate screen, and because a core count decides it, the
answer is the same however the bytes fall into reads. Build it now; it does
not wait on note U.**

Two small forks, each with my lean:
- **Reveal the scrollbar thumb when a reset moves a scrolled-back view to the
  live screen?** Lean **yes**.
  - The scrollbar note (`docs/research/notes/l-scrollbar.md`) measured AppKit
    revealing the thumb on a programmatic scroll, and a key's snap to the live
    screen already reveals it.
  - It is new behaviour, though: today a reset alone already moves the view to
    0 without revealing anything.
- **Fade a reset in, the way a screen switch fades?** Lean **no**: a primary
  reset is not a switch. A reset issued from the alternate screen is also a
  switch, so it keeps today's fade.

Deliberately left out of the rule:
- **DECSTR** (`CSI ! p`, the soft reset). It keeps the history, as xterm's and
  foot's do on the primary.
- **ED 2** (erase the whole screen), which is what `clear` sends.
  - `clear` is far more common than `reset`, and it leaves the same highlight
    over blank cells.
  - xterm drops the selection there too, because the same *ClearScreen* runs.
    Following it would be a separate decision with its own count.
  - UNPINNED: ncurses' `clear` also sends E3, `CSI 3 J` (erase the
    scrollback), which baud counts as unsupported.

**Design**

Core. `baud-core` is `no_std`, has no allocator and no unsafe code, uses
newtypes, and has audit budgets.

1. **The type.** In `baud-core/src/term.rs`, beside `RegionScrolls`,
   `ReverseFlips` and `UpdatesOpened` (lines 150-200), add `Resets(u32)`, with
   a first value and a wrapping `next`.
   - Those three are the house shape: a counter the app compares across a
     read.
   - The doc says it moves on every RIS and on nothing else, and that two
     equal readings saw no reset.
   - It is a counter rather than a flag because one read can hold two resets,
     and a flag would need clearing: a mode recorded and never read back.
2. **The field goes in `Book`, not `Cold`.** `Book` is the scroll bookkeeping
   in `baud-core/src/storage.rs`; `Cold` is the struct of rarely written
   terminal state. Put the field beside `hist`, for three reasons:
   - `Book`'s own doc says grid bookkeeping lives there "rather than in
     `Cold`, whose size is pinned". `Cold` is asserted at 336 bytes (term.rs
     line 431).
   - `Paint`, the per-byte writer, carries `&mut Book` by reference (paint.rs
     line 156). So the hot loop carries no extra bytes.
   - Note U's option B puts the window in `Book`, so a later move stays in one
     struct.

   `Book` is built once, when the storage is built (storage.rs line 746).
3. **The bump.** In `hard_reset`, beside `book.hist = Rows::ZERO` (ops.rs line
   1336), with a comment citing [[source: foot-term-reset]] and
   [[source: xterm-reset]].
4. **The accessor.** `Term::resets()`, beside `region_scrolls`, exported from
   `baud-core/src/lib.rs`.

Nothing else in the core changes. The rotation and the history mark stay as
they are.

App: `baud-app/src/app.rs`.

0. **First, a refactor that changes no behaviour.** Move `pump`'s
   after-the-read bookkeeping into a plain function with no AppKit in it. It
   takes the `Term`, the `Before` snapshot, and the window, the scrolled-line
   count, the selection, the predictions, the guess and the slide. `pump`
   calls it.
   - Today those halves live inside `pump`, which tests cannot run.
   - Moving them out is what lets the selection, slide and prediction halves
     go red at an assertion.
1. `Before` gains the reset count, read in `Before::of`.
2. Two small predicates beside `switched`:
   - *reset_in*: the count moved;
   - *restarted*: switched or *reset_in*.
3. `after_read` returns the live screen (`Rows::ZERO`) when *reset_in*.
   - It must not use `snapped` here: `snapped` keeps the primary's window on
     the alternate screen, and the history under that window is gone.
4. In the extracted function, after the `Retired`/`Restored` match. The order
   matters: the `Retired` arm starts a slide, and a settle placed before it
   would be undone.
   - **Drops, keyed on *restarted*:** the selection goes, the slide settles,
     the scroll accumulator zeroes and the guess clears.
   - **Predictions** are drained each with `Judged::Contradicted`, as `judge`
     does. This also fixes the existing switch block's bare clear.
   - **The fade** stays keyed on `switched` alone.
   - **If the thumb fork is yes:** when the view went from above 0 to 0,
     reveal the thumb the way `snap_to_live` does (line 4019).

**Dropped from the researcher's design: a "leaving row" gate on `sliding`.**
- The app's scrollback depth is fixed at `Depth::DEFAULT`. Once the reset's
  read settles the slide, the next slide needs a scroll, and a scroll makes
  the history at least one line.
- So no perturbation can make the gate fire, and the repo's rule is that a
  line no perturbation can fire is a claim nothing checks.
- The one place it might fire is the candidate below.

**A separate candidate, UNVERIFIED by run:** settle a running slide on a
`Restored` read.
- The `Restored` arm (line 3094) drops the selection but never settles a slide
  already running.
- A full-screen scroll down on the primary (`unretire` in ops.rs) or a
  prompt-block redraw moves the window down while an earlier slide keeps
  panning up.
- Build it only after a red-first test shows it.

Under note U's models:
- **A (today):** exactly the above.
- **B (the core owns the window):** `hard_reset` sets the window to none
  itself, as alacritty's *display_offset = 0* and foot's *view = 0* do. The
  app's `after_read` line is deleted. The explicit reset is required, not an
  optimisation: B's clamp-on-exit alone would recreate effect 3.
- **B+ (the core tracks anchors):** `hard_reset` marks every anchor invalid,
  as ghostty's *PageList.reset* does, and the app's selection line is deleted.
  The slide and predictions stay app state, so the count stays.
- **Q3 (a selection across a width change):** no interaction.

The Linux driver takes up *reset_in* when it gains a window, a selection or a
slide.

**Tests and measurement**

Every test goes red first, each at its own assertion, and every mechanism gets
one perturbation. The order:
1. Extract; the existing tests stay green and the app behaves the same.
2. Write the tests below against today's code and watch each go red.
3. Make the fix.

Core, in `baud-core/tests/scrollback.rs`, beside
`the_screen_epoch_moves_on_every_switch_and_nothing_else`:
*the_reset_count_moves_on_a_full_reset_and_nothing_else*.
- It moves on `ESC c`:
  - from the primary;
  - from the alternate screen;
  - on a fresh terminal with no history, the case O2 cannot see;
  - twice for two resets in one read.
- It does not move on:
  - `CSI ! p`;
  - ED 2;
  - `CSI 3 J`, which is unsupported: `EraseExtent` in
    `baud-domain/src/vt/csi.rs` covers 0-2;
  - a `?1049` round trip;
  - a whole-screen scroll;
  - a region scroll.
- It cannot go red at an assertion before the accessor exists, so its red
  comes from the perturbations.

App tests run through the extracted function over a real `Term`, following the
pattern of
`a_read_that_switches_screens_anchors_by_what_the_primary_scrolled`.
1. *a_reset_brings_a_window_over_the_history_to_the_live_screen_however_the_reads_fall*.
   - Setup: 40 lines on 20 rows, `back` = 5, then `ESC c` plus 30 lines.
   - Feed it whole, split just after the `c`, and at read sizes 1, 3 and 7.
     The window must be 0 every time.
   - Red today, at an assertion, through the existing `after_read`: the whole
     read lands on the first line after the reset.
2. *a_reset_from_the_alternate_screen_ends_the_kept_window*.
   - Setup: the primary at `back` = 5, then `?1049h`, then a key (the window
     is kept), then `ESC c` plus scrolling output in one read.
   - The window must be 0. Red today.
3. *a_reset_drops_a_selection_even_with_no_history*.
   - A fresh terminal with a selection on row 2, then `ESC c` alone.
   - Controls: `CSI ! p` and ED 2 keep the selection.
   - Red once the extraction exists.
4. *a_reset_in_the_read_that_scrolled_starts_no_slide*.
   - Replay `tests/reset/seq-reset.raw` as one read; the slide must be
     settled.
   - Red after the extraction: today a 26-row slide starts.
5. *a_reset_drops_outstanding_predictions*.
   - A prediction on the prompt row, then `ESC c` alone. It must be cleared,
     with a contradicted verdict recorded.
6. **Recorded level.**
   - Take a new capture that scrolls after the reset, through
     `scripts/capture-tui.py` as the existing one was: for example
     `seq 1 45; reset; seq 101 140` at 20 rows by 60 columns.
   - Replay it at read sizes 1, 2, 3, 4, 5, 8, 17 and 4096. The selection must
     be dropped, the window at 0 and the slide settled at every size.
   - `tests/reset/seq-reset.raw` cannot test the window. Only five lines
     follow its reset and they fit on the screen (its README says so), so the
     window is 0 at every size today and that half would pass without the fix.
     It stays for the selection and slide halves.
7. **Source gate.** Extend `every_reader_of_the_window_asks_the_clamp`, or add
   a sibling, to hold that `pump` calls the extracted function. The logic
   itself is tested, not text-matched.

Fork tests: a primary reset spawns no fade; if the thumb fork is yes, a reset
from `back` above 0 reveals the thumb.

Perturbations, each red alone:

| broken | what goes red |
|---|---|
| no bump in `hard_reset` | the core test, and app tests 1-6 |
| a bump in `soft_reset` too | the DECSTR controls |
| `after_read` ignoring the count | the whole-read arm of test 1, test 2, and the window half of test 6 |
| `snapped` in place of zero | test 2 |
| the selection dropped on `switched` only | test 3 |
| the settle placed before the `Retired` arm | test 4 |
| predictions cleared without a verdict | test 5's verdict assertion |
| the fade keyed on *restarted* | the no-fade test |

Measurement:
- **Core ingest sweep.** The count is written only in `hard_reset`, off the
  ingest path, and `Book` is reached by reference, so the expected result is
  no result. The repo's rule still owes the sweep for any `baud-core` change:
  - `ingest2`, with base, change and an A/A copy of the change (a
    byte-identical second build, which shows the noise);
  - all three built at one path, not in a git worktree, which moved code 96
    bytes in an earlier sweep;
  - the order rotated, the first round discarded, rep counts sized to each
    recording, on mains;
  - `cells_written` identical on all nine recordings;
  - a disassembly compare of the ingest loop.

  HEAD 0987b98 aligns every function to 64 bytes. That weakens the layout
  argument but does not remove the sweep.
- **Oracles.** Run `cargo test --workspace` before committing: the sfte and
  alacritty cross-checks live outside `baud-core`.
- **App.** One integer compare per read. That cost is argued, not swept: it is
  far under the idle-CPU noise floor (note N, row 11). No shader change.

**Effort and risk**

- **Effort: M.**
  - Core: about 30 lines and one test.
  - App: about 40 lines, plus the extraction, six tests, a source-gate line
    and one new capture.
- **Risk: low.**
  - A reset now drops a selection the user might have wanted. No terminal read
    here keeps one.
  - A scrolled-back reader is sent to the live screen, but the history they
    were reading no longer exists.
  - The extraction is a refactor of `pump`, held by the existing tests.
  - The one ordering hazard is pinned by test 4.
  - DECSTR, ED 2, the alternate-screen round trip and the prediction hold are
    unaffected.

**What would change it / depends on**

What would change it:
- **The owner wants the selection kept across a reset:** remove that line; the
  window and slide halves stand.
- **A reset should fade like a switch:** key the fade on *restarted*, one
  word.
- **Any full-screen erase should drop the selection, as in xterm:** that is a
  separate erase-count decision.
- **ED 3 is implemented:** it needs its own count, because a selection on the
  live screen should survive it.
- **The sweep shows a cost:** unlikely with `Book`; re-sweep.

What it depends on: nothing blocks it. Two sequencing points, both by reading:
- It must land in `after_read` before note U's differential uses that function
  as its oracle. That differential includes RIS
  (`docs/research/notes/u-resize-and-the-window.md`, line 521); otherwise B is
  built to match effect 3.
- B must set the window to none explicitly in `hard_reset`.

Note U's Q2 only decides where the selection line lives, and Q3 does not
interact.

### V2. Who keeps "which history line am I looking at" (note U questions 1 and 2)

**The question** -- When you scroll up into the history, something has to
remember which line you are on, and keep you on it while output arrives or the
window is resized. Today the app (baud-app) keeps it as one number, "rows up
from the bottom", and after every read and every resize asks the core how far
to move it. Should the core (baud-core) keep it instead, as the absolute
number of the line at the top of the view? And if so, just that one position,
or also a public list of "tracked points" that an embedder registers and the
core moves with their text (called B+ in note U)?

A "re-cut" below means re-wrapping every retained line to a new width, also
called reflow.

**What you would notice**
- Today: nothing is wrong. The app's anchoring is exact after three fixes.
- After: nothing different on screen, by design; the move is proved to behave
  the same. What changes is the next bug. Three recorded bugs came from the
  app numbering lines by a different rule from the core's:
  - a zoom repeated the banner four times;
  - the alternate screen's output pushed the main screen's view up its
    history, and a read that switched screens panicked;
  - a re-cut moved the view from `s 05` to `s 00`.
- Each was fixed with a new core accessor or parameter. The next thing that
  must follow its text needs another.

**Today**
- The app holds `App::back: Rows` (line 346 of `baud-app/src/app.rs`) and
  moves it in three places:
  - `anchored` (1850), fed after every read by `after_read` (1990) through
    `Term::history_moved_since`;
  - `resized_window` (2002), which calls `Term::resize_holding(ws, back)`
    (line 942 of `baud-core/src/term.rs`);
  - `snapped` (2011): a key on the main screen sends the view live; on the
    alternate screen (the one full-screen programs use, with no history) it
    keeps the main screen's view.
- `back` is also written at app.rs lines 810, 3402 (the wheel), 4022 and 4348.
- A source gate, `every_reader_of_the_window_asks_the_clamp` (app.rs:2424),
  lists every raw read of it.
- The core already supplies the pieces:
  - line numbering, `Book::top` in `baud-core/src/storage.rs`;
  - the clamp, `Term::looking_back` (term.rs:1451), which is zero on the
    alternate screen;
  - the map through a re-cut, the `grid::Follow` hook inside `resize_holding`
    (term.rs:1165-1210).
- The Linux driver has no wheel yet (lines 17-18 of `baud-app/src/linux.rs`)
  and binds no pointer at all (CLAUDE.md). When it gets a wheel, it would have
  to repeat this anchoring.

**What others do**
- alacritty keeps the view offset in the core grid, which advances it as
  output scrolls ([[source: alacritty-grid-mod-0-26]] grid/mod.rs:134,
  266-268). Its anchoring is only approximate.
- foot carries its view through the reflow as a tracking point
  ([[source: foot-grid-c-1-21]] grid.c:832, 874-876, 1231-1234).
- ghostty keeps a tracked pin for the view in its page list, and snaps it live
  when it lands on the live area ([[source: ghostty-pagelist-9bbb9b2]]
  PageList.zig:474, 1318-1320, 2900-2903).
- kitty keeps `scrolled_by` in its screen, but adjusts it at render time by a
  count of added history lines ([[source: kitty-screen-0-49]] screen.c:4044,
  4087). That is the same "number plus a delta" shape baud-app uses today.
  kitty only clamps it across a re-cut.
- VTE and wezterm come from note U's survey and were not re-read (UNVERIFIED).
  wezterm holds the view in its GUI, and it goes stale across a rewrap.
- So the prior art does not settle it. A core-side view is not necessary
  (baud's app is exact today) and not sufficient (kitty). The case rests on
  baud's own three bugs and on the Linux driver.

**Options**
- A. Status quo. Cost: each new thing that must follow its text needs one more
  accessor -- a selection kept across a resize, a saved cursor, a search hit,
  a second view. The Linux driver repeats the anchoring. Forecloses nothing.
- B. The core owns the view: one field, the absolute line at the top, or
  `Live`. Cost: medium work, an API change, view state in a core otherwise
  described as "VT finished", a normalization step a differential test must
  prove, and an ingest sweep. Forecloses nothing: B+ can go on top later.
- B+. A public array of tracked points the embedder registers, mapped through
  every re-cut. Enables two views over one session and a selection remapped
  through a re-cut. Cost: a registration API and a bounded array before
  anything needs one, and the app's selection numbering would have to merge
  into the core's.

**Recommendation** -- **Yes to B: the core keeps the view as an absolute line
(or `Live`), and the app keeps only the policy of when a key snaps it live. No
public tracked-point array yet.**
- It gives the view one owner. It does not give one numbering: the selection
  keeps the app's own `Scrolled` count until B+.
- Linux gets the anchoring for free, and only that. Its wheel, its scrollbar
  and a render of a scrolled-back view all still have to be built.

**Design**
- State: `Book` gains `window: HistoryWindow`, with
  `enum HistoryWindow { Live, At(LineIdx) }` -- an enum rather than an
  `Option`, per house style. `LineIdx` is the type `Book::top` already uses.
  It is caller storage, so no allocation; the core stays `no_std`, alloc-free
  and free of `unsafe`.
- Invariant: `At(l)` means l is at or after the oldest retained line and
  before the line at the screen's top.
- Who moves it:
  - Output scrolling, region scrolls, reverse scrolls, the prompt block's
    retreat (lines 111-126 of `baud-core/src/prompt.rs`) and a height pull:
    nothing. An absolute line does not move when the screen does.
  - A width re-cut maps `At(l)` through the existing `Follow` code. Today's
    local `held` becomes the field.
  - `hard_reset` sets `Live`.
- Normalization, O(1) and never per byte: a line at or past the screen's top
  becomes `Live`; a line older than the oldest retained line becomes that
  line, or `Live` if there is no history. It runs at three points:
  - at the exit of `Term::feed`;
  - at the exit of `Term::resize`;
  - between the height step and the width step inside `resize` -- the review's
    correction. `resize_holding` already does this today. Without it, a view
    that a height pull brought onto the live screen becomes `At(l)`, a
    narrowing re-cut can land it behind the new top, and the exit then reads
    it as scrolled back.
- API (names indicative):
  - `Term::window_back() -> Rows`, through `looking_back`, zero on the
    alternate screen;
  - `Term::window() -> Viewport`;
  - `Term::scroll_window(WindowStep)`, with
    `WindowStep::{Older(Rows), Newer(Rows)}`;
  - `Term::set_window_back(Rows)`, for the scrollbar thumb and a jump;
  - `Term::window_live()`;
  - `resize_holding` goes, and `resize` holds the view. `view(back)`,
    `looking_back`, `history_mark` and `history_moved_since` stay, because
    core tests pin them.
- App: delete `App::back`, `anchored`, `after_read` and `resized_window`. The
  calls at app.rs lines 3053, 3402, 4022, 4348 and 4459 become core calls.
  `snapped` becomes "on the main screen, `window_live()`". Rewrite the source
  gate to demand `window_back` or `window` and to forbid a stored count.

**Tests and measurement**
- The migration differential (note U section 4, step 4):
  - Port today's `anchored`, `after_read` and `resized_window` into
    baud-core's tests as the oracle. They are pure over `Term`'s public API.
  - A proptest (a randomized property test) over random programs: LF floods,
    RI (reverse index, a scroll down) at the top, IL and DL (insert and delete
    line) inside scroll regions, OSC 133 prompt marks and redraw marks, ?1049
    and ?47 round trips, RIS. Mixed with wheel up and down, set, snap, and
    resizes of width and height in both zones: the output zone, while a
    command prints, and the prompt zone, while the shell waits between OSC 133
    prompt marks.
  - Read sizes 1 and 4096, `PROPTEST_CASES=20000`. `window_back()` must equal
    the oracle after every step. RIS is amended to `Live` in the oracle. Only
    then is the app code deleted.
- Re-point these to the core API:
  `a_width_reflow_keeps_the_window_on_its_line`,
  `the_alternate_screens_output_does_not_move_the_primarys_window`,
  `a_read_that_switches_screens_anchors_by_what_the_primary_scrolled`,
  `a_resize_on_the_alternate_screen_anchors_by_the_primary`,
  `a_window_over_the_history_holds_its_line_through_a_re_cut`. Add the three
  in `baud-core/tests/reflow.rs` the review named:
  `a_live_window_stays_live_through_a_re_cut` (line 415),
  `a_window_a_growth_brings_to_the_live_screen_stays_live_through_the_re_cut`
  (431) and
  `a_window_whose_line_the_re_cut_dropped_holds_the_oldest_line_kept` (449).
- Perturbations, each shown red: no normalization at the exit of `feed`; no
  floor clamp; no `Live` on RIS; no `Follow` mapping; no normalization between
  the height and width steps, which must redden reflow.rs:431.
- Measurement: a field in `Book` and a normalization at the exit of `feed` owe
  an `ingest2` sweep. Both arms built at one path (a worktree build once moved
  code 96 bytes and read -6.5% on `cat`), 7 rounds rotated, the first dropped,
  an A/A copy, the nine recordings plus `mix`, `cells_written` identical.
  Commit 0987b98, which starts every function on a 64-byte boundary, shrinks
  the layout risk; the sweep is still owed. The cost is argued to be nil and
  is unmeasured.

**Effort and risk** -- Effort M: about 150 core lines, about 200 app lines
deleted, the differential and the sweep. Risk medium: a path that moves the
screen's top or the history in a way the normalization does not reproduce (the
differential is the check); a layout-class ingest regression (the sweep is the
check); API churn in the app.

**What would change it / depends on**
- Build B+ instead if you want two views over one terminal, or a selection
  remapped through a re-cut.
- Stop if the sweep shows a cost outside the A/A spread, or the differential
  finds a movement normalization cannot reproduce.
- V1 lands first. Under B, `hard_reset` sets the view `Live` explicitly, as
  V1's design says: B's normalization alone would reproduce V1's effect 3.
- V3 should land after it, so `relayout` changes once. V7 and the Linux wheel
  depend on it.

### V3. Does a selection survive a resize (note U question 3)

**The question** -- If you select text and then resize the window, should the
selection stay? Today any change to the grid clears it, even making the window
only taller or shorter. Should it survive a height-only change?

**What you would notice**
- Today: select a few lines, drag the window taller, and the selection
  disappears.
- After: on the main screen, a height-only resize keeps it on the same lines.
  A width change still clears it, and so does any resize while a full-screen
  program is up.

**Today**
- The selection is app state, numbered by the app's own `Scrolled` count
  (lines 55-81 of `baud-render/src/select.rs`).
- `relayout` clears it on any grid change, height-only included (app.rs, near
  line 4499). `docs/research/notes/k-selection.md` (lines 43-46) records that.

**What others do**
- alacritty clears it on a column change and rotates it on a height-only
  change ([[source: alacritty-term-term-mod]] term/mod.rs:680-690).
- kitty clears it on every resize ([[source: kitty-screen-0-49]]
  screen.c:705).
- foot passes the selection's ends through the reflow as tracking points and
  remaps them ([[source: foot-render-1-21]] render.c:4732-4740).
- ghostty and VTE remap it (UNVERIFIED, from note U, not re-read).

**Options**
- Clear on every resize (today, and kitty). Free; drops a selection that did
  not need dropping.
- Keep it across a height-only resize on the main screen by renumbering
  (alacritty). A small app change.
- Remap it through a re-cut (foot). Needs V2's B+ registration API.
- A sub-choice the review raised: a selection that touches rows a shrink cut
  below the cursor. Those rows are hidden, not deleted; they may come back, or
  be blanked when history is pulled instead. Clip the selection to what is
  still shown, or clear it. My lean is clear: a kept selection could later
  name a row that was blanked or replaced.

**Recommendation** -- **Yes: on the main screen, keep the selection across a
height-only resize by renumbering it. Clear it on any column change, on any
resize while the alternate screen is up, and when it touches rows the shrink
cut (lean; clipping is the alternative). No remap through a re-cut for now.**

**Design** (baud-app and baud-render only)
- In `relayout`, take `Before::of(term)` (app.rs:1875) before `term.resize`.
- If the columns are unchanged and the main screen is up, renumber the
  selection by `history_moved_since(before.history)`:
  - n rows retired into the history: `Scrolled::advanced(n)`, which exists;
  - n rows restored from it: a new `Scrolled::retreated(n)` in select.rs.
- Then clip with `oldest`, as a read does.
- Clear when the columns changed; when the alternate screen is up (the program
  redraws on SIGWINCH, the "window size changed" signal, and that screen has
  no history to renumber by); or when the selection touches cut rows.
- This replaces the unconditional clear near app.rs:4499.

**Tests and measurement**
- Red first: *a_height_only_resize_keeps_the_selection_on_its_lines*
  (baud-app). It fails today because of the unconditional clear.
- Controls, all proposed: *a_width_change_clears_the_selection*; a resize on
  the alternate screen clears it; a selection touching cut rows is cleared.
- Perturbations: no renumbering, so the selection lands on the wrong lines; no
  alternate-screen clear.
- No ingest measurement: nothing on the ingest path changes.

**Effort and risk** -- Effort S. Risk low.

**What would change it / depends on** -- If you want the selection remapped
through a re-cut, that is B+ (V2). It does not need V2, but should land after
it so both change `relayout` once. A later option: keep it across a width
change at a prompt too, where nothing is re-cut.

### V4. Does a cursor saved with ESC 7 follow its text through a resize (note U question 4)

**The question** -- A program can save the cursor position with ESC 7 (DECSC,
"save cursor") and restore it with ESC 8 (DECRC). If the window is resized in
between -- lines re-wrapped, or rows pushed into the history -- should the
saved position move with the text it was on, or only be clamped to the screen?
Underneath sits a structural question: does baud keep one saved-cursor slot
per screen, or three?

**What you would notice**
- Today: after a resize, ESC 8 can land on a cell of another line, and the
  program then writes over output. That is the wezterm #6669 class: "losing
  shell text because the restored cursor is put in a place where the shell
  overwrites prior output".
- The commonest real case -- nvim open, resize, exit -- uses the ?1049 save,
  taken when a full-screen program starts, and baud already maps that one.
  What is left affects programs and scripts that use plain ESC 7 and ESC 8
  around a resize.
- After: ESC 8 returns to the same text.

**Today**
- The ESC 7 slot, `cold.saved` (term.rs:376), is written by ESC 7 and `CSI s`
  (lines 1509 and 1743 of `baud-core/src/write.rs`). Neither resize step moves
  it; it is only clamped when restored (ops.rs:89-90).
- It is one slot shared by both screens (CLAUDE.md, the ?47 section, "What is
  left").
- The ?1049 save, `alt_saved`, is already carried through both resize steps
  (`PrimaryCursor::Saved`, term.rs:919-927, 1011, 1211-1218).

**What others do**
- foot carries the saved cursor through the reflow as a tracking point
  ([[source: foot-grid-c-1-21]] grid.c:858-872), and keeps one per screen
  (grid.c:858; [[source: foot-render-1-21]] render.c:4737-4744).
- kitty passes the main screen's saved cursor into its buffer resize and
  applies the result ([[source: kitty-screen-0-49]] screen.c:312-315,
  711-712). The mapping itself is in rewrap code that is not pinned
  (UNVERIFIED).
- alacritty moves it down on a height pull
  ([[source: alacritty-grid-resize-0-26]] resize.rs:64), clamps it on a shrink
  (resize.rs:93) and on a width change (resize.rs:387), and never carries it
  through a re-cut. It keeps one per grid (resize.rs:64, 92;
  [[source: alacritty-term-term-mod]] term/mod.rs:677-678). Its ?1049 swap
  drops the main screen's saved cursor (term/mod.rs:719-720), so the ?1049
  save is the main screen's slot.
- ghostty keeps "one saved state ... per screen"
  ([[source: ghostty-terminal-9bbb9b2]] Terminal.zig:1999-2002), and ?1049
  saves into the main screen's slot (Terminal.zig:4820-4822). Its saved cursor
  is tracked as a pin through a resize (Screen.zig, UNPINNED).
- wezterm #6669 is open and calls this "wholly unspecified"
  ([[source: wezterm-issue-6669]]). It also asks that a pending wrap be reset.
  ghostty issue #5718, which it cites, was not fetched (UNPINNED).
- xterm is said to keep one saved cursor per buffer (cursor.c, UNPINNED). The
  TODO says to pin it first.

**Options**
- Keep clamping (alacritty's shrink and width behaviour). No code. The #6669
  class stays.
- (i) One slot per screen, with the ?1049 save merged into the main screen's
  slot -- the ghostty and alacritty shape.
  - Two slots, today's count, so `Cold` probably stays within its pinned 336
    bytes (not checked).
  - The remap is nearly free, because the ?1049 save is already mapped.
  - Cost: a behaviour change. After ESC 7, a ?1049 round trip, then ESC 8, the
    cursor goes where ?1049 saved it. `a_47_exit_leaves_what_1049_saved`
    probably flips.
- (ii) Three slots: the main screen's ESC 7, the alternate screen's ESC 7, and
  the separate ?1049 save.
  - Keeps today's restore behaviour.
  - Adds one saved state (about 50 bytes, not checked), which trips the
    336-byte size assert on `Cold` (term.rs:431). The remap needs a second
    mapped point.

**Recommendation** -- **Yes: the ESC 7 position should follow its text through
a resize. Then a fork, (i) or (ii). My lean is (i), provided xterm's cursor.c,
pinned first, agrees with ghostty and alacritty; if xterm keeps the ?1049 save
separate, take (ii).**

**Design**
- Step a, the prerequisite: one ESC 7 slot per screen, swapped by ?1049 and
  ?47 the way the prompt state (`prompt::Shell`) and kitty's keyboard stacks
  already are. ESC 7 and `CSI s` write the live screen's slot. This also
  closes the ?47 shared-slot gap for the rxvt and nsterm `smcup` and `rmcup`
  pairing. Under (i), ?1049's entry saves into the main screen's slot, and
  `alt_saved` goes.
- Step b, the remap:
  - `View::reflow` (line 278 of `baud-domain/src/grid/reflow.rs`) takes a
    small fixed slice of points instead of the one cursor.
  - The live cursor still decides where the screen lands. The saved cursor is
    mapping-only: it moves with its text and moves nothing else.
  - The height step applies the cursor's retire and pull arithmetic
    (term.rs:996-1011, 1051) to the main screen's saved row.
  - The restore clamp stays as the backstop.
- Rules the research did not specify (proposed here, not researched):
  - A saved row the height step retires into the history: the cursor's
    arithmetic would underflow. Proposal: clamp it to the top row.
  - A saved row below the cursor that a shrink hides: proposal: keep it on its
    line, and let the restore clamp it if the row is still hidden.
  - A saved column in the pending-wrap position (baud's marker column, one
    past the right margin): #6669 asks that pending wrap be reset. Proposal:
    map the column it would wrap from, and clear the pending state on restore.

**Tests and measurement**
- Step a: *a_47_program_does_not_move_the_primarys_decsc*.
- Step b, red today: *decsc_follows_its_cell_through_a_narrowing_re_cut* (in
  the output zone, at several read sizes) and
  *decsc_follows_its_row_through_a_shrink_that_retires_rows*; plus one test
  per rule above.
- A hand model of the ghostty #5718 script shape (UNPINNED): ESC 7 on the main
  screen, a long line, narrow while output runs, ESC 8, write; no output
  overwritten.
- Perturbations: no per-screen swap; `reflow` ignoring the second point; the
  second point allowed to move the screen; the height step ignoring the saved
  row.
- Measurement: the `Cold` change owes an `ingest2` sweep. Under (ii) the size
  assert is re-derived first.

**Effort and risk** -- Effort M: step a M, step b S to M. Risk medium: the
split touches ?1049, ?47, RIS, DECSTR and the scroll region, where every save
and restore path has had bugs this month.

**What would change it / depends on** -- Pinning xterm's cursor.c decides (i)
against (ii). If you prefer alacritty's clamp, nothing is built. Step b
depends on step a.

### V5. History depth, and old lines lost when you drag through a narrow width (note U question 5)

**The question** -- While a command prints output (not at a prompt), every
drag step that changes the width re-cuts all retained lines. Drag very narrow
and each line takes many rows; the 2,000-line ring (the history buffer, reused
in a circle) cannot hold them all, the oldest are dropped, and widening back
does not return them. Accept that, or defer re-wrapping to the end of the
drag, or store whole logical lines instead of rows?

**What you would notice**
- Today and after: dragging very narrow while output runs can lose the oldest
  history lines. At a marked shell prompt nothing is re-cut, so nothing is
  lost. How many lines a real narrow drag drops on a full ring is unmeasured.
- After: the loss is counted, so it can be seen, and the cost of a full-ring
  drag step is measured on a committed fixture.

**Today**
- The re-cut is eager: it runs on every width step, in the output zone only
  (term.rs:1124).
- The depth is 2,000 lines (`Depth::DEFAULT`, line 131 of
  `baud-domain/src/history.rs`). Its own doc gives the reason: 2,000 lines
  keep the ring inside L2, the CPU's second-level cache, because scrolling
  into rows past L2 measured 0.56x. So the ingest cost fixes the depth long
  before the re-cut cost does; depth is not a lever here.
- Costs on record:
  - a step of the owner's recorded drag (`live-drag`): 1.44 [1.42-1.48] us to
    resize;
  - one output-zone width change on a small history: 6.58-7.02 us;
  - a full 2,050-row ring re-cut, 200 to 100 columns and back: 317 and 412 us,
    from unkept scratch benches whose figures disagree with each other by
    about 2x.
- The re-cut already knows how many rows it dropped (`Reflowed::dropped`,
  counted in rows). Nothing exposes it.

**What others do**
- Every eager terminal whose history is capped in rows loses lines this way.
  VTE: "The scrollback buffer also remains defined in terms of lines", and
  re-wrapping a million lines takes about 0.2 s
  ([[source: vte-rewrap-txt-0-84]] rewrap.txt:289-291, 434-439).
- Nobody ships a deferred re-cut or logical-line storage.

**Options**
- Keep eager, keep 2,000, count the loss. Cheap; the loss stays.
- Defer (C4 in note U): address cells during the drag and re-cut once at the
  end. Needs a width per row, since rows written mid-drag are cut at mixed
  widths, and it re-wraps the shell's per-step redraws (alacritty #2408's
  mechanism).
- Logical lines (D): store paragraphs and wrap when read. Replaces the ring.
  Its nearest measured precedent, spans, cost 5-10% on six recordings just to
  carry the field.

**Recommendation** -- **Yes: keep the re-cut eager and the depth at 2,000
lines (fixed by the L2 scroll cost), count the rows a re-cut drops, and commit
a full-ring fixture that measures a drag step.**

**Design**
- Expose the drop as a running count in rows,
  `Term::reflow_dropped() -> Rows`, fed from `Reflowed::dropped` -- the house
  refuse-and-count style.
- Capture a real fixture with `scripts/capture-resize.py`: a command printing
  long lines past 2,000, then a drag from 200 to 40 to 200 columns while it
  still runs, so the zone is output.

**Tests and measurement**
- *a_narrow_drag_through_a_full_ring_counts_the_rows_it_drops*.
- `winch` cost mode (`baud-bench/benches/winch.rs`) on the new fixture,
  interleaved launches with an A/A copy (a single launch has read about 1.5x
  high). Report the resize cost per step and the rows dropped per gesture. The
  floor is one 120 Hz frame, 8.33 ms, per step.

**Effort and risk** -- Effort S to M: the counter and a captured fixture. Risk
low: the measurement only confirms or refutes.

**What would change it / depends on** -- A drag step over 8.33 ms on the
fixture, or reports of wanted history lost after a narrow drag, would revive
the deferred re-cut or a deeper, lazily wrapped store. V7 and V8 lean on this
fixture.

### V6. Which rule decides the top line when the window changes height (note U question 6)

**The question** -- When the window gets shorter or taller, which line ends up
at the top? The record carries three rules in sequence, and the code has
followed the third since 2026-09-18. This is a confirmation, not a new choice.
1. "We should not change the scroll ever": a resize never moves the top line.
2. "A resize never changes the top line. Only feeding bytes may."
3. Today's rule. Shorter: hide ("cut") rows below the cursor first, and push
   lines into the history only as far as the cursor needs. Taller: bring back
   the cut rows that hold something (V9), then pull lines back from the
   history, then fill with blanks -- "pull down to the bottom, and blank-fill
   only when nothing is left to pull".

**What you would notice** -- Nothing changes if you confirm. Under rule 1, a
taller window would add blank rows at the bottom instead of bringing history
back.

**Today** -- Rule 3, in `Term::resize` (term.rs:964-1110).
`a_taller_primary_screen_pulls_the_scrolled_lines_back_above_it` in
`baud-core/tests/altresize.rs` encodes it.

**What others do**
- alacritty pulls history first and blank-fills after; on a shrink it pushes
  lines out of the top only as far as the cursor needs
  ([[source: alacritty-grid-resize-0-26]] resize.rs:38-68, 70-97).
- ghostty pulls on a growth only when the cursor is on the last row, and on a
  shrink first trims trailing blank rows ([[source: ghostty-pagelist-9bbb9b2]]
  PageList.zig:2851-2905, 2828-2849).
- kitty does not pull by default: its option *scrollback_fill_enlarged_window*
  is off ([[source: kitty-conf]]; [[source: kitty-screen-0-49]]
  screen.c:721-732).
- foot puts the last re-wrapped row at the bottom
  ([[source: foot-grid-c-1-21]] grid.c:1203-1209).
- baud's own finding rules out rule 1 on a shrink:
  `a_shrink_does_not_let_the_prompt_overwrite_output`.

**Options** -- Rule 3 (today): no code. Rule 1 for growth (kitty's default):
the top line never moves on a growth, but it cannot govern a shrink, the
altresize test flips, and V9 becomes mostly moot.

**Recommendation** -- **Yes: confirm rule 3. Rule 1 survives as the rule for a
scrolled-back view: a view up in the history holds its line, and a live view
stays live.**

**Design** -- No code.

**Tests and measurement** -- None new; the altresize test already encodes it.

**Effort and risk** -- None.

**What would change it / depends on** -- If you mean rule 1 for growth, that
test flips and V9 only decides reveal against blank-fill. V9 depends on this
answer.

### V7. Re-wrap old output while you sit at a prompt (note U question 7)

**The question** -- When the shell sits at its prompt (between the OSC 133
prompt marks your zsh sends), baud re-wraps nothing on a width change:
narrower hides columns, wider shows them again. So long lines of old output
are cut at the right edge until you widen. Should baud re-wrap the output
above the prompt, and leave the prompt's own rows for the shell to redraw
(kitty's shape, called E2 in note U)?

**What you would notice**
- Today: at your prompt, narrow the window and long lines of old output are
  cut, not wrapped; widen and they come back whole. A drag step costs about
  1.4 us and loses nothing.
- With E2: long lines re-wrap at the prompt, as in every surveyed terminal.
  Each drag step becomes a full-history re-cut, and dragging very narrow can
  lose the oldest lines (V5) in your commonest case.

**Today** -- The main screen is re-cut only in the output zone (term.rs:1124).
In the prompt zone it is addressed by cell, not re-wrapped. `settle_growth`
(term.rs:1245) keeps the row marks honest in either zone. The recorded
gestures had no width change in the output zone.

**What others do**
- kitty keeps the current prompt out of the rewrap and pastes it back
  ([[source: kitty-screen-0-49]] screen.c:659-664, 733-746).
- ghostty re-cuts and then blanks the prompt (from note U, not re-read here).
- `docs/research/notes/m-resize-and-reflow.md` (lines 166-175) notes that the
  in-place ordering argument for a re-cut anchored at the prompt is not the
  one `View::reflow` carries today.

**Options**
- Keep "no re-cut at a marked prompt". Free and lossless; long lines stay cut
  until you widen.
- E2 now. Cost: a full-history re-cut per drag step, roughly 0.3-0.4 ms on a
  full ring (from unkept scratch benches, not properly measured); loss through
  narrow widths at the prompt; and a new in-place ordering proof, in the
  prompt-block code that has been the most bug-prone area of recent weeks.

**Recommendation** -- **Not now: keep "no re-cut at a marked prompt", and
revisit after V2, V9 and V5's full-ring bench.**

**Design** -- If ever taken: write the in-place ordering proof first. Re-cut
the rows older than the prompt block's start, anchored there; leave the
block's rows addressed by cell; map the block's start (`PromptBlock.start`)
through the same `Follow` hook.

**Tests and measurement** -- If taken: the proof as a gated property, the
recorded gestures replayed at several read sizes, and `winch` cost on the
full-ring fixture. The per-step cost at a prompt with a full ring is
unmeasured today.

**Effort and risk** -- Effort L, if ever. Risk high without the proof, because
it reopens the prompt-block seam.

**What would change it / depends on** -- If you value re-wrapping at the
prompt over a free, lossless drag, or V5's bench shows the full-ring re-cut
trivial and loss rare, build E2, proof first. Depends on V2, V9 and V5.

### V8. Resize on every drag step (note U question 8)

**The question** -- While you drag the window's edge, should baud resize the
grid and tell the program (SIGWINCH, the "window size changed" signal) on
every step, or wait until the drag settles? This confirms the rule baud
already follows.

**What you would notice** -- Nothing changes if you confirm: full-screen
programs redraw as the window moves, instead of showing stale or cut content
until you stop.

**Today** -- No debounce: a SIGWINCH and an eager resize on every drag step
that changes the grid. The rule stands on a measured freeze
(`docs/research/notes/n-make-faster.md`, lines 75 and 90; the comment in
`relayout`). A step of the owner's recorded drag costs 1.44 us to resize
(CLAUDE.md).

**What others do**
- foot debounces during a drag and pauses the pty (the channel between the
  terminal and the program), showing a snapshot meanwhile
  ([[source: foot-render-1-21]] render.c:4700-4745).
- ghostty throttles at 25 ms, kitty debounces, alacritty keeps the last size
  per batch of events, and wezterm and VTE do not coalesce (UNVERIFIED: note
  U's table, not re-read).

**Options** -- No debounce (today). Or coalesce, as foot, kitty and ghostty
do: fewer re-cuts and less loss, but forbidden by the recorded rule, and it
shows stale or truncated content during the delay.

**Recommendation** -- **Yes: confirm no debounce, and a SIGWINCH plus a resize
on every drag step that changes the grid.**

**Design** -- No code.

**Tests and measurement** -- V5's full-ring fixture is the check that a step
stays under one 120 Hz frame (8.33 ms) at the chosen depth.

**Effort and risk** -- None.

**What would change it / depends on** -- A full-ring drag step over 8.33 ms on
V5's fixture.

### V9. What a hidden row must hold to come back when the window grows (note U question 9)

**The question** -- When the window gets shorter, baud does not delete the
rows below the cursor; it hides them ("cut rows"). When the window gets taller
again, the rule (V6) is: bring back the cut rows that hold something, then
pull older lines back from the history, then fill with blanks. What does
"holds something" mean? Today the answer comes from the row's mark
(`RowExtent`): a bookkeeping note of where a row stops being uniform, kept to
make erasing fast. It is not a record of whether the row shows anything.

**What you would notice**
- Today the mark misfires both ways:
  - A row that looks blank counts. A row of blanks erased in a foreground-only
    colour comes back as a blank-looking row under the prompt, while a history
    line could have come back instead. Your own recorded drag shows this for
    14 steps (`live-drag` 641-654): the screen sits one row higher than it did
    before the marks were made honest.
  - A row that looks full does not count. A row erased whole in a red
    background is uniform, so its mark is 0. A history line is pulled over it,
    and once the history runs out it is blanked -- erased, not merely kept
    hidden.
- After the recommendation: the first shape goes away. The red-band row
  behaves as today.

**Today**
- The height growth tests a cut row by its mark (term.rs:1023-1027), reveals
  up to the furthest row that counts, pulls history (term.rs:1041), then
  blank-fills the rest (term.rs:1060-1080).
- The core already asks the same question elsewhere, a different way. The
  prompt block's retreat uses `View::row_is_blank` (line 1628 of
  `baud-domain/src/grid.rs`), which reads cell content, not the mark (called
  at prompt.rs:109). One core, two definitions of "this row holds nothing".
- An erase writes `Content::EMPTY` in the current colour. A written space
  holds U+0020, which is distinct by design (`baud-domain/src/cell.rs`).
- Four tests in `baud-core/tests/scrollback.rs` pin today's behaviour:
  `a_growth_reveals_a_cut_row_of_text_before_pulling_history` (the control),
  `pinned_a_cut_row_of_invisible_blanks_is_revealed_as_content`,
  `pinned_a_cut_row_uniform_in_a_visible_colour_is_left_cut` and
  the band test, `pinned_a_band_past_the_old_edge_goes_with_the_erase_of_its_row`
  since 2026-10-14, when the erase began to clear the band and both of its
  arms began to pull the line.

**What others do** -- They ask a related question, not this one: none of them
keeps hidden cut rows. They support a test for "content", not the reveal
policy.
- foot: a row's content runs to its last cell that is not empty (empty means a
  zero character or a spacer), so colour is ignored and a written space
  counts. A row holding a tracking point -- the cursor, the saved cursor, the
  view, a selection end -- also counts ([[source: foot-grid-c-1-21]]
  grid.c:926-933, 939-951, 1150-1159).
- ghostty: on a shrink it trims trailing blank rows. "A blank row is any row
  with no text (but may have styling)", and trimming stops at a row holding a
  tracked pin ([[source: ghostty-pagelist-9bbb9b2]] PageList.zig:2828-2849,
  3154-3156, 3175-3181). Its text test counts a written space (page.zig,
  UNPINNED).
- kitty: a line is empty when every cell's character is 0, so a space counts
  and colour does not ([[source: kitty-lineops-0-49]] lineops.h:75-82). That
  test is used when moving lines into the scrollback
  ([[source: kitty-screen-0-49]] screen.c:2906-2914), not on resize. What
  kitty's resize counts lies in unpinned code (UNVERIFIED).
- alacritty never reveals cut rows: a growth pulls history first and adds
  blanks after ([[source: alacritty-grid-resize-0-26]] resize.rs:38-68).
- Why "visible" cannot be exact in the core: the renderer decides how an empty
  cell looks. A truecolour background equal to the theme's, OSC 11 (a program
  setting the default background) and reverse video (?5) all decide visibility
  at draw time.

**Options**
- A. The mark (today). Free; wrong both ways; disagrees with the core's own
  retreat test.
- B. A written character, one test shared with the retreat. A row counts when
  a cell in the columns it will be shown at holds anything but
  `Content::EMPTY`, a space included. Matches foot, ghostty and kitty; exact
  in the core; one definition. A colour-only row still does not count. Costs a
  scan of the cut rows' cells, once per height growth.
- B2. A written character, or a background the application set -- the
  `Bg::Set` test `baud-difftest` already uses. Also exact in the core, and it
  keeps the red-band row. No surveyed terminal does it.
- C. Visible: text, a set background, underline, strike, or reverse on a
  blank. Only an approximation, since the core does not know the theme. It
  stops counting rows of written spaces, and no surveyed terminal does it.
- D. B, plus a row carrying an image placement. See Design for why it is left.
- E. No reveal step at all. Either always pull history first (alacritty), or
  reveal every cut row (attributed to xterm in a term.rs comment, UNPINNED).
  The first never brings back output a shrink cut. The second is what baud
  left: a fresh terminal's banner stayed retired while blank rows filled in
  under the prompt.

**Recommendation** -- **Yes to B: a cut row comes back when it holds a written
character (a space counts, colour alone does not), tested by the same function
the prompt retreat uses -- after the seven-gesture comparison below confirms
which of your recorded resizes change.**
- Two pinned tests flip: the foreground-only blanks and the band past the old
  edge stop counting, and a history line is pulled instead.
- The red-band row stays as today: pulled over, then blanked.
- `live-drag` 641-654 is predicted, not measured, to show the older screens
  again.
- If you want red bands kept, take B2.

**Design**
- baud-domain (`baud-domain/src/grid.rs`): lift `row_is_blank`'s test into one
  ring-row function, `View::ring_row_holds(at: RowRot, width: Cols) -> Holds`,
  with `enum Holds { Text, Nothing }` -- an enum, not a `bool`, per the audit.
  It answers `Text` if any cell in the first `width` columns of the physical
  row holds anything but `Content::EMPTY`. `row_is_blank` becomes a call to
  it, so the retreat keeps its behaviour through the same function.
- baud-core (term.rs:1023-1027): replace the mark read with `ring_row_holds`.
  Choose the width once, before the height step:
  - the old width when this resize's width step will re-cut the row (the
    columns changed and the main screen is in the output zone);
  - the new, clamped width otherwise.
  - The rest of the height step is unchanged.
- Cost: cut rows times width cell reads, once per height growth, on a cold
  path. The ingest path is untouched. A shortcut that bounds the scan by the
  row's mark is not used: a mark is only true up to the live edge, and the
  bound is unproven.
- Images: a Sixel (an image format a program draws into the terminal) writes
  no cells -- `sixel_end` only places the image and moves the cursor -- so an
  image-only cut row is never revealed under A or B. History is pulled over
  it; `expire_images` (term.rs:1814) keeps placements on hidden rows, but the
  row never comes back. The case needs the cursor moved above an image before
  a shrink, because a Sixel leaves the cursor below itself. That is rare, and
  how rare is UNMEASURED. D is the refinement left if it matters; the retreat
  should then take the same rule.

**Tests and measurement**
- Red first on today's code, in scrollback.rs:
  - *a_cut_row_of_foreground_only_blanks_is_not_content*, the renamed first
    pinned test: a history line is pulled, history 10.
  - *a_band_past_the_old_edge_is_not_content_after_a_width_growth*, the
    renamed band test: both arms pull.
  - *text_only_past_the_new_edge_at_a_prompt_is_not_content*: text at columns
    150-160 at 200 wide, narrowed to 100 at a prompt, then cut and grown.
    Predicted red today (reasoned, not run).
- Controls, green before and after:
  `a_growth_reveals_a_cut_row_of_text_before_pulling_history`;
  *a_cut_row_in_a_colour_alone_is_not_content*, the renamed red-band test;
  *a_cut_row_of_written_spaces_is_content*;
  *text_a_re_cut_will_wrap_into_view_is_content*, the control for the width
  choice.
- Hand model (`baud-core/tests/resize_matrix.rs`): your zsh with an erase to
  the line's end in a foreground-only colour below the prompt, then shrink and
  grow. Recorded level (`baud-core/tests/resize_recorded.rs`): `live-drag`
  641-654 show the base build's text. Both red first.
- Perturbations, each red at its own test: back to the mark; count any
  non-default style (the colour control reddens); treat U+0020 as empty (the
  spaces control); always the new width; always the old width; a second copy
  of the test for the retreat (a change to the shared function must redden
  both a scrollback.rs test and a prompt test).
- Before choosing: a text sweep over all seven recorded gestures with `winch`,
  under both rules, dumping the screen text at every step; count and inspect
  every step that differs. The prediction that only `live-drag` 641-654 differ
  is unmeasured.
- `winch` cost mode, interleaved launches with an A/A copy, both arms built at
  one path. The floor is one 120 Hz frame.

**Effort and risk** -- Effort S: one domain function, about 10 core lines, 6-8
tests and two sweeps. Risk low to medium: it changes which rows appear after a
growth in real sessions, and the seven-gesture sweep bounds how much.

**What would change it / depends on**
- The sweep shows more differing steps than `live-drag` 641-654, and they look
  worse under B; or you want colour-only rows kept (B2); or you want images to
  hold a row (D).
- Depends on V6 (rule 3 confirmed). Independent of V2; it can land first.

## Part 2. The side margins

### V10. The side-margin fence: finish it or switch it off

**Status, 2026-10-12.** The owner answered F. Steps (a), the oracle
harness (`bench-others/ghostty-oracle`), and (b), rectangle scrolling, are
built: the record, "V10's oracle" and "Rectangle scrolling". Steps (c) and
(d) remain, and what follows is the note as it was written before them.

**The question**

A program can fence its text into a band of columns. It first turns on mode
?69 (DECLRMM, "left/right margin mode"), then sends DECSLRM ("set left and
right margins", `CSI Pl ; Pr s`). baud accepts both today, but its fence is
only half built.

The research asked about the fence's rarest corner: a program moves the cursor
outside the fence and prints, and the text has to go somewhere. The review
showed this corner is the least reachable question inside a bigger one. So the
real decision is **whether to switch the fence feature off or finish it
properly.** Where text right of the fence goes (the question as first asked)
is the last step of "finish".

Two terms used below:
- **Pending wrap.** After a character lands in the last allowed column, the
  terminal does not move to the next line yet. It waits, and wraps only if
  another character arrives.
- **Sentinel.** baud stores a pending wrap as "the cursor is one column past
  the fence" (right margin + 1). It keeps no separate flag.

**What you would notice**

- **Today, from the corner itself: nothing.** No recording, no ncurses program
  and no tmux session found here writes text outside a fence.
- **Today, probably: neovim with side-by-side windows.**
  - This comes from reading neovim's source (UNPINNED, not run against baud).
  - Scrolling one window of a `:vsplit` fences that window's columns and sends
    DL/IL (delete/insert line).
  - baud moves whole rows for those, so the neighbouring window would be
    dragged along.
  - This is probable by reading and unverified by observation. One look
    settles it.
- **After switching the fence off:** neovim learns the fence is unsupported
  and redraws the window instead. The screen is correct and more bytes are
  sent. (UNPINNED, not run.)
- **After finishing it:**
  - Scrolling a split moves only the fenced columns.
  - A new line inside a fence starts at the fence's left edge.
  - The cursor is drawn and reported where it really is.

**Today**

All of this is by reading the code; nothing was run.

**What works.** Writing inside the fence wraps at the fence. ICH, DCH, DECIC
and DECDC (insert and delete characters and columns) respect it. The DECSLRM
row in `docs/vt-coverage.md` calls it done "for writing", and records one gap:
vertical scrolls move whole rows.

**What the row does not say, and it is more reachable than the corner:**
- **Line starts ignore the fence.** Carriage return, NEL (next line), CNL and
  CPL (cursor next/previous line) set column 0, not the left margin
  (`baud-core/src/write.rs` lines 342, 1754, 1484, 1488).
  - So `\r\n` inside a fence at columns 5-10 starts the next line at column 1,
    outside the fence.
  - The next write then fills columns 1-10 before it wraps.
  - No test covers this. The only CR LF in
    `baud-core/tests/vt420/side_margins.rs` is sent before the margins are
    set.
- **Cursor moves ignore it too.** CUF (cursor forward) and HPR (relative
  column move) clamp to the screen's last column through `Col::forward`
  (`baud-domain/src/quantity.rs` lines 627-629). CHA (absolute column) does
  the same.

**The corner itself:**
- **A pending wrap is the sentinel.** After a write fills to the right margin,
  the stored column is right+1 (`baud-core/src/paint.rs` lines 580, 600-605,
  831).
- **A cursor a program *moves* to right+1 looks identical.** This can happen
  only while the fence is on. At full width, right+1 is past the last column,
  and no move reaches it.
- **Any write at or past right+1 wraps into the fence first.**
  - Single characters go through `glyph` (paint.rs 668-670).
  - Runs go through `Paint::room`, which saturates to 0 past the margin
    (867-869), and then `wrapped` (551-562).
  - With autowrap (DECAWM) off, the character lands on the right margin
    instead (957-969, 604).
- **Reports clamp to the margin.** Both `Term::cursor`
  (`baud-core/src/term.rs` 1893-1901) and the cursor-position report CPR
  (write.rs 1566) clamp. So a cursor moved to column 30 under a 5-10 fence is
  drawn and reported at column 10.

**Tests:**
- `a_run_right_of_the_right_margin_wraps_to_the_left_margin` (side_margins.rs
  line 136) is the only test that pins today's corner rule.
- Its comment says "no pinned source answers". That was true when it was
  written, on the morning of 2026-09-23. The xterm source was pinned that
  evening and read for this rule on 2026-09-24, and it disagrees with baud.
- `no_wrap_pins_to_the_right_margin` (line 169) does *not* pin a disagreement.
  It places the cursor with `CUP 1;5`, inside the fence, where xterm also
  stops at the margin.

**What baud claims about itself:**
- Its device-attributes reply (DA1) says VT102 (`baud-domain/src/vt/report.rs`
  194-206), which is a terminal older than margins.
- Yet the DECRQM query ("is this mode supported?") for ?69 answers
  "recognised" (`baud-core/src/ops.rs` 1513).

**Reach.** By the researcher's grep, 0 of 23 captures contain ?69 or
`CSI Pl;Pr s`. The TERM entry baud sets has no margin capabilities
[[source: terminfo-xterm-256color]], so ncurses never fences.

**What others do**

**Specifications:**
- [[source: vt510-decslrm]]: DECSLRM "only works when vertical split screen
  mode (DECLRMM) is set", and only at VT Level 4. It says nothing about text
  written outside the margins.
- [[source: ctlseqs]]: VT420 and up.

**xterm, pinned** ([[source: xterm-reset]], charproc.c):
- In *dotext* (7056-7058, with the comment at 6991), a cursor right of the
  margin writes on to the screen's edge.
- It then wraps to the left margin (*WrapLine*, 6983), and always marks the
  row wrapped (6979).
- The pending wrap is a separate flag, *do_wrap*.
- *do_wrap* survives ?69l (7477-7488). It also survives DECSTR: the soft-reset
  path (14358-14563) was read in full, and it neither resets the flag nor
  moves the cursor.
- DECLRMM is honoured only at VT level 4 or higher (7481, 7783-7789).
- With DECLRMM off, `CSI s` saves the cursor only when it has no parameters
  (*only_default*, 4899). So `CSI 5;10 s` does nothing. I found this while
  writing, by reading; it bears on option W below.

**xterm cursor.c**, same commit (UNPINNED, fetched):
- Carriage return goes to the left margin when the cursor is at or right of
  it.
- CUF stops at the right margin while the cursor is inside the fence.
- CUB stops at the left margin, unless the cursor was already left of it.
- Save and restore keep the wrap flag.

**ghostty** (local checkout at d9840f3c, UNPINNED):
- The same right-limit rule, and a separate *pending_wrap* flag.
- Carriage return and cursor-right honour the margins.
- It marks a row wrapped only at the full screen edge. xterm and baud mark it
  at a mid-row margin too.

**The tattoy fork of wezterm** (UNPINNED): a third behaviour. The first
character goes where the cursor is, and the next goes to the left margin.

**Terminals with no DECSLRM at all, so they cannot judge this:** alacritty
(UNPINNED), sfte ([[source: sfte]]) and foot ([[source: foot-csi]]).
- baud's two running *oracles* (reference terminals it is compared against
  byte for byte) are alacritty and sfte, so neither can judge any of this.
- libghostty-vt can. It is already built, and linked by
  `bench-others/multibench/build.rs`, and it exposes the cursor's column, its
  row and its pending-wrap flag.

**How others store a pending wrap:**
- As a flag separate from the column: xterm, foot ([[source: foot-vt-ris]]),
  alacritty, ghostty and the tattoy fork.
  - alacritty is UNPINNED. Its doc says why: so that no stored position is
    ever past the last column.
- As a sentinel like baud's: only sfte.
- baud's own `Paint::room` underflow, fixed in 959cdbe, was a sentinel
  failure.

**Who switches the fence on:**
- **tmux** (UNPINNED): never for baud.
- **neovim** (tui.c read in source at 4b26a2a8; UNPINNED, not run):
  - It asks DECRQM ?69 at startup, and any "recognised" answer turns margins
    on.
  - To scroll a split narrower than the screen, it sends DECSTBM (the vertical
    region), then ?69h, DECSLRM, a cursor move and DL/IL, and then resets.
  - It writes no text while fenced. So it would hit the rectangle-scrolling
    gap, not the corner.
- **vim**: off unless configured (UNPINNED).
- **vttest**: no test of text past the margin was found (UNVERIFIED).

**Options**

- **W. Switch the fence off.**
  - ?69h is refused and counted, like any mode baud lacks. DECRQM ?69 answers
    0, "not recognised".
  - A bare `CSI s` stays save-cursor (SCOSC). A `CSI s` with parameters is
    ignored and counted, which is xterm's rule above.
  - Cost: S, and reversible.
  - It retires at once:
    - the corner;
    - rectangle scrolling;
    - the line-start and CUF gaps;
    - the cursor and CPR clamp under a fence.
  - It matches DA1's VT102 claim and xterm's level-4 gating.
  - It forecloses VT420 margins for any program. None was found that writes
    text inside them.
  - It changes:
    - the ?69 and DECSLRM coverage rows, which move to out;
    - the ICH/DCH/DECIC/DECDC margin behaviour;
    - the DECRQSS margin reply (`baud-core/src/dcs.rs`);
    - DECSTR's DECLRMM restore.
- **F. Finish it**, in this order:
  - (a) a libghostty-vt oracle harness;
  - (b) rectangle scrolling for IL/DL/SU/SD under the fence;
  - (c) line starts and cursor moves that honour the fence;
  - (d) the corner, as C2-min.
  - Cost: L overall.
- **For step (d), the corner itself:**
  - *A. Keep wrap-first; fix the record and the reports.* Effort S. baud stays
    different from pinned xterm.
  - *B. xterm's rule only past right+1.* Rejected. Column 11 would wrap into
    the fence while column 12 writes on, and no implementation makes two
    adjacent columns behave that differently.
  - *C1. A separate pending-wrap flag*, as everyone else does.
    - Exact and conventional. Effort L.
    - Every write that reaches the edge sets the flag, and ~40 move sites
      clear it.
    - Its cost is unmeasured. The research's "5-10%" was borrowed from other
      fields carried through the loop (spans, the row map). So ranking C1 as
      riskier than C2 is argued, not measured.
  - *C2. Keep the sentinel for the common case; give the two rare states typed
    values that only cold code creates* (Placed, PendingAt). Effort M-L.
  - *C2-min. C2 with Placed only.* No PendingAt, and no change to
    save/restore. Those two are separate behaviours, each needing its own
    oracle.
- **Do not take C2 on its own.** Take a CUF from inside the fence that
  overshoots the margin:
  - It lands past the margin, at right+1 or beyond, because `Col::forward`
    clamps only to the screen edge (by reading).
  - Today the next character wraps back into the fence. That is wrong, but the
    text stays inside the box.
  - Under C2 that landing is a placed cursor, and the character is written
    outside the fence.
  - xterm (UNPINNED cursor.c) and ghostty stop that CUF at the margin. That is
    step (c), which is why (c) comes before (d).

**Recommendation**

**First, run neovim in baud, open a `:vsplit`, scroll one window and look.
Then decide: switch the fence off (option W), yes or no? My lean is yes. Say
no only if you want baud to be a VT420-class terminal. In that case, finish it
in this order: the harness, then rectangle scrolling, then fence-honouring
line starts and moves, then C2-min. Either way, never take C2 alone.**

Why I lean that way:
- Nothing found writes text outside a fence.
- The one program found that fences at all, neovim, is served by either
  branch.
- W is small and reversible, and finishing is large.
- W makes baud's claims consistent, since DA1 already says VT102.

If W, fix the record in the same change: the coverage rows, and the stale
comment above the line-136 test.

**Design**

**Branch W.** Only `baud-core` changes, and the per-byte path is not edited.
- In `baud-core/src/ops.rs`, the `PrivateMode::LeftRightMargin` arm
  (1010-1014) stops setting `ModeBit::SideMargins` and goes to the counted
  path for modes baud does not implement. The DECRQM arm (1513) answers "not
  recognised".
- With that bit never set, the check at write.rs 1454 always gives
  `MarginMode::Off`. `Csi::parse_with` (`baud-domain/src/vt/csi.rs` 698-708)
  then never yields `SetSideMargins`.
- `CSI s` parses as `SaveCursor` (csi.rs 898) whatever its parameters, and
  today that includes `CSI 5;10 s` with ?69 off. W adds xterm's check: a
  parameterised `CSI s` is counted and does nothing.
- `set_side_margins` and the DECRQSS margin reply become unreachable. Either
  delete them by name (not by range), or keep them behind an off-by-default
  feature if finishing later is likely.
- A DECRQSS query for the margins answers invalid (`0$r`), the shape any
  unknown request gets.
- The left and right fields in `Cold` can stay, always at full width. ICH and
  DCH already read them.

**Branch F.**
- **(a) The harness.** A new crate beside multibench, outside the workspace.
  - It links libghostty-vt and feeds each fixture at every read size.
  - It compares the plain-text screen, the cursor's column and row, and the
    pending-wrap flag.
    - It compares text and cursor only, not wrap marks. ghostty marks wrapped
      rows only at the full edge, and that known difference is not this
      decision.
  - It takes the library path from an environment variable.
    - A missing library exits nonzero with a message.
    - multibench's own default warns and skips, which is not the model for a
      gate.
  - First, pin ghostty's Terminal.zig at d9840f3c and xterm's cursor.c in
    `docs/research/sources/MANIFEST.toml`, so their rules can be cited.
- **(b) Rectangle scrolling.** Its own decision (the TODO list item after this
  one), not designed here.
- **(c) Line starts and moves**, following xterm cursor.c (pin it first):
  - CR, NEL, CNL and CPL go to the left margin when DECLRMM is on and the
    cursor is at or right of it; otherwise to column 0.
  - CUF and HPR stop at the right margin while the cursor is inside the fence.
  - CUB stops at the left margin, unless the cursor was already left of it.
  - Sites: write.rs 342, 1480-1488, 1496 and 1754.
  - Add margin-aware moves in `baud-core` rather than changing `Col::forward`,
    whose other callers are full width.
  - CHA and tabs are left alone: xterm's rule for them was not read
    (UNVERIFIED).
- **(d) C2-min.**
  - **The type.** A `CursorCol` newtype in `baud-domain/src/quantity.rs`
    beside `Col`, `repr(transparent)` over usize.
    - It replaces the cursor column in `Paint`, `Term` and the saved cursor,
      so `Paint` does not grow.
    - It uses enums, never bools: baud-domain is gated at zero bool fields.
  - **The encoding.**
    - An untagged value below the column count is a real column.
    - An untagged right+1 is the pending wrap, and only writes create it.
    - `Placed(c)` carries a high-bit tag, so its value is at or above the
      column count. Only a move creates it, and only when the move lands
      exactly on right+1 while the fence is narrower than the screen.
    - With a high-bit tag and a character width of at most 2, `cx + need`
      cannot overflow.
  - **Constructors and accessors.**
    - "Written" is the constructor for writes; "placed" is the constructor for
      every move.
    - A hot accessor serves the write path after its fast test. A cold decode
      returns a `CursorAt` enum. "Position" keeps today's raw meaning.
      "Reported" serves `Term::cursor` and CPR.
    - Assigning a plain `Col` stops compiling, so a forgotten decode is a
      compile error, not a silent wrong column.
  - **The write path** (`baud-core/src/paint.rs`).
    - `glyph`'s existing branch, "cx + need > right + 1", calls an edge
      function instead of `wrap`.
    - Today's cases stay inline. The new ones go to a
      `#[cold] #[inline(never)]` function, which handles a cursor past the
      margin or Placed by xterm's rule, with the limit at the screen edge:
      - A wide character that does not fit before the edge wraps to the left
        margin (charproc 7160-7171).
      - With DECAWM off, the clamp is to the last screen column, not to the
        margin (7152).
    - `wrapped` makes the same decision, and sizes the piece to the screen
      edge so `put_ascii_run` never clips.
  - **BS and CUB.** BS (write.rs 365, inlined into the per-byte loop) and CUB
    must decode a tag. There are two ways to do it, and the disassembly picks
    between them:
    - an explicit test in the BS arm;
    - an encoding under which raw-1 decodes to c-1.
  - **Margin changes.** Re-encode a Placed value only when the margins
    actually change (?69l, DECSTR, resize), never on a no-op. Otherwise tagged
    values would appear on a full-width screen.
  - **Save and restore.**
    - The saved cursor stores the decoded value.
    - Restore re-encodes it against the current margins, and must not go
      through "placed". Going through it would turn a saved pending wrap into
      a placed cursor, and break the fenced DECRC case (restore cursor) that
      works today by accident.
  - **Reports.** A pending wrap reports its margin column, as today. A placed
    cursor, and anything past the margin, reports its real column.

**Not in this yes.** Each is its own later decision, with its own oracle:
- **PendingAt:** a pending wrap surviving ?69l and DECSTR. xterm keeps it,
  read in full.
- **A pending wrap surviving save and restore.**
  - xterm's cursor.c restores the wrap flag (UNPINNED), and so do alacritty
    and ghostty. Today `baud-core/src/ops.rs` line 90 drops it.
  - Its reach is the opposite of the corner's. Through ?1049l it touches the
    exit of every full-screen program whenever the shell sat at a pending
    wrap.
- **Full-width follow-ons:** CUB and BS after a full row, a resize dropping a
  pending wrap, and EL0 at a pending wrap ([[source: xterm-scroll]]).
- **Adjacent:** `wrap` marks a row wrapped at a mid-row margin. xterm always
  marks; ghostty marks only at the full edge. The sources disagree.

**Tests and measurement**

**W:**
- **Red first**, unit tests in `baud-core/tests/vt420/`:
  - DECRQM ?69 answers 0.
  - `?69h` is counted as unsupported.
  - `CSI 5;10 s` neither saves the cursor nor sets margins.
  - A bare `CSI s` / `CSI u` still saves and restores.
  - DECRQSS for the margins answers invalid.
- **Perturbation:** put the mode apply back, and each of those goes red.
- **Rewrites, in the same commit:**
  - The side_margins tests are rewritten to pin the withdrawal, with the
    reason in their comments.
  - `csi_s_is_scosc_without_declrmm` stays green: it sends a bare `CSI s`.
  - The coverage rows move to out.
- **Controls:**
  - Every golden digest is unchanged, since no recording contains ?69.
  - The alacritty and sfte oracles are unchanged.
  - Run `cargo test --workspace`, because those oracles live outside
    `baud-core`.
- **Measurement.** Build base and change at one path (git archive into one
  directory, then touch every file), and compare ingest2's disassembly with
  addresses masked.
  - If identical, no sweep is owed.
  - If not, run a rotated ingest2 sweep with an A/A copy.

**F, step (c):**
- **Red first:**
  - `\r\n` inside a 5-10 fence starts the next line at column 5.
  - A CUF that overshoots from column 8 stops at column 10.
  - A CUB from inside the fence stops at column 5.
- **Controls:**
  - A CR from left of the fence goes to column 1.
  - At full width nothing changes: `cells_written` is identical on the corpus,
    and every golden is unchanged.
- **Oracle:** the ghostty harness on the same fixtures.

**F, step (d), C2-min.**

**Red first.** Each runs at every read size in side_margins.rs.
1. **Replace the line-136 test**, traced to *dotext*. Set margins 5..10 and
   `CUP 1;30`.
   - Eight X land in columns 30-37 of row 1.
   - A 180-character run from column 30 puts 171 on row 1, and the rest inside
     the fence from the left margin.
   - The same holds with é, and with single characters.
2. **The pair that proves a placed cursor differs from a waiting one.**
   - `CUP 1;11` then "AB": A lands at column 11 and B at 12. Red today.
   - Control: fill columns 5-10, then "X": it lands at row 2, column 5. Green
     today, and must stay green.
3. **DECAWM off past the margin.** ?7l, `CUP 1;30`, "XYZ" land at columns 30,
   31 and 32. Today all three land at column 10.
4. **A wide character** at the last column, past the margin, wraps to row 2,
   column 5.
5. **Reports.** `CUP 1;30` then CPR answers `ESC[1;30R`; today it answers
   `1;10R`. The pending-wrap control still reports the margin.

**Controls that must stay green:**
- A cursor left of the left margin writes to the right margin, then wraps.
  xterm does the same.
- The vt100 pending-wrap tests, and
  `erase_to_start_with_a_pending_wrap_covers_the_whole_row`.
- The 24 alacritty difftest cases and the sfte pins.
- `every_recording_renders_to_its_pinned_digest`.
- `cells_written` identical on the corpus.

**Levels.** Unit tests, hand-written fixtures at every read size, and the
ghostty harness. There is no recorded level, because no capture contains ?69,
and the record must say so.

**Perturbations**, each red alone:

| broken | goes red |
|---|---|
| the slow path ignores Placed | test 2 |
| a move produces an untagged right+1 | test 2 |
| the limit past the margin is right+1 | tests 1, 3, 4 |
| the wrap goes to column 0 | tests 1, 4 |
| "reported" clamps everything | test 5 |
| the DECAWM-off clamp is the margin | test 3 |
| restore goes through "placed" | a narrowed-margin DECRC at a pending wrap, which must still wrap |
| the edge treats a cursor left of the margin as past it | the left-margin control |

**Measurement.**
- Make a same-path build, then disassemble ingest2's `main`, where feed and
  act inline.
- HEAD aligns every function to 64 bytes (0987b98). That fixes where functions
  start, but not the code inside an edited function, and this change edits
  `glyph`'s inlined body.
  - The record prices that class of change: `for w in [WIDE, NARROW]` cost
    -3.5% on a payload that could not even execute it.
- So an ingest2 sweep is owed:
  - arms base, change and an A/A copy (the same binary run twice, to show the
    noise);
  - order rotated, six or more rounds, the first dropped;
  - rep counts sized per recording, on mains;
  - over the nine recordings, mix, and the wrap/flat/lines ladder (flat and
    wrap cross the edge every 200 bytes).
- `cat` is readable now: its aligned arm read 1662.3-1680.8 over nine rounds.
- No capture reaches the new code, so the sweep prices carrying and layout
  only, and the record must say so.
- A disjoint regression the A/A copy does not cover: try the other BS
  encoding. If both regress, fall back to option A.

**Effort and risk**

- **Effort.**
  - W is S.
  - F is L: the harness is M (it needs bindings for the formatter and the
    cursor getters, which multibench does not declare yet); rectangle
    scrolling is M-L and is not designed here; line starts and moves are S-M;
    C2-min is M.
- **Risk.**
  - W is low and reversible. Nobody found uses what it gives up.
  - F is medium. Correctness risk sits in the decode rules and in restore;
    speed risk sits in the edited inlined body.

**What would change it / depends on**

- **Changes it:**
  - You want a VT420-class terminal: finish.
  - neovim looks fine in baud: the priority drops, and the lean stays.
  - A capture shows a real program writing right of a fence: the corner rises
    on the finish branch.
  - The ghostty harness disagrees with the pinned xterm reading on any
    fixture: stop, and name both sources before choosing.
- **Depends on:** the neovim observation. On the finish branch the harness
  comes first, and it serves rectangle scrolling too.

## Part 3. Clipboard, selection and highlight tracking

### V11. Programs copying to the Mac clipboard (OSC 52 writes)

**The question**

When a program inside baud asks to copy text, should that text land on the Mac
clipboard? Examples are neovim yanking with `"+y`, or tmux copy-mode on a
server you reached over ssh. Today baud throws the request away.

The request is **OSC 52**: an escape sequence, meaning bytes a program prints
that the terminal acts on instead of showing. It says "put this text on the
clipboard", with the text in **base64** (a way to write any bytes using 64
printable characters). The same sequence with `?` asks "tell me what is on the
clipboard". That read stays refused whatever is decided here.

Two things to decide: whether writes are on by default, and if they are,
whether only while baud's window is **key** (the window that receives your
keystrokes).

**What you would notice**

- Today: you yank in remote neovim or copy in tmux, switch to the browser,
  press Cmd-V, and get whatever you copied before. There is no error and no
  sign the copy was dropped.
- After: the copy lands, and Cmd-V pastes it anywhere.
- The price: any program you run, or a file you `cat`, can replace your
  clipboard without asking. You might then paste that text into a shell.

**Today**

- baud-domain splits `52;Pc;Pd` into `ClipboardOp::Read` or
  `ClipboardOp::Write(bytes)` (`baud-domain/src/vt/osc.rs`, lines 378-384; the
  read refusal is written down as a decision at 171-185).
- baud-core counts both halves and does nothing else (`baud-core/src/osc.rs`,
  207-208), in `clipboard_reads_refused` and `clipboard_writes_dropped`. A
  read gets silence, pinned by `baud-core/tests/xterm/osc.rs`. Nothing in
  baud-app reads either counter.
- An OSC body is capped at `OSC_MAX` = 512 bytes (`baud-core/src/storage.rs`,
  587). `str_put` stops storing past it and counts each byte it drops in
  `string_truncations` (`baud-core/src/osc.rs`, 57-63). 512 bytes of base64
  body is about 380 bytes of text.
- baud's own copy works: Cmd-C runs `copy` in `baud-app/src/app.rs`, which
  calls `write_text` in `baud-present-macos/src/clipboard.rs`. That writes the
  **general pasteboard** (macOS's one clipboard) as plain UTF-8.
- The Linux driver has no clipboard at all.
- No recording under tests/ contains `ESC ] 52`.
- `SECURITY.md` and `docs/protocol-decisions.md` record the read refusal.
  `docs/vt-coverage.md` marks OSC 52 done.

**What others do**

- **The spec**, [[source: ctlseqs]]: `Pc` names the target (c clipboard, p
  primary, q secondary, s select, 0-7 cut buffers; empty means "s 0"). `Pd` is
  base64, and `?` asks for a reply. Anything else clears the selection. No
  size limit is stated, and xterm can disable both halves.
- **xterm**: off by default, both halves. The pinned [[source: xterm-reset]]
  shows the resources exist (`allowWindowOps`, `disallowedWindowOps`, with
  `GetSelection` and `SetSelection` among the window operations). The defaults
  come from xterm.man at the same commit: allowWindowOps false, and
  disallowedWindowOps listing GetSelection and SetSelection among the
  "dangerous" operations. Both the researcher and the reviewer read that man
  page, and it is **UNPINNED**.
- **kitty**, [[source: kitty-conf]]: writes allowed, reads ask for permission;
  `clipboard_max_size 512` (MB).
- **sfte**, [[source: sfte]]: OSC 52 on by default. It skips reads and hands
  every write to a callback with **no focus check**. Its Wayland callback
  accepts only target `c`, and its payload cap is 16 MiB.
- **foot**, [[source: foot-readme]]: lists OSC 52 support. Its `osc52` option
  and default (enabled, reads included) are **UNPINNED**.
- **Others, all UNPINNED and UNVERIFIED in source**: alacritty copy-only;
  wezterm writes on and reads opt-in; ghostty write allow, read ask; iTerm2
  write-only and off by default; Terminal.app and GNOME Terminal have no
  OSC 52.
- **So the pinned evidence is two to one**: kitty and sfte honour writes by
  default, and xterm refuses them. "Nearly every modern terminal honours
  writes" rests mostly on UNPINNED docs.
- **The two workflows**:
  - tmux sends OSC 52 outward on a copy by default (`set-clipboard external`):
    **UNPINNED**.
  - Neovim's docs say it "automatically detects host terminal support for OSC
    52" without saying how (**UNPINNED**). Whether neovim in baud needs
    `vim.g.clipboard` set by hand is **UNVERIFIED**.
  - The pinned `docs/research/sources/terminfo/xterm-256color.ti`, the
    terminfo entry baud tells programs to use, has no `Ms` capability. `Ms` is
    the terminfo flag that advertises OSC 52.

**Options**

- **A. Keep dropping.** No code. Remote copy never works, and nothing says so.
- **B. Honour writes on today's 512-byte buffer, as it is.** Not viable. A
  cut-off body is not always caught. With `Pc` empty the cut leaves 508 base64
  characters, a whole multiple of four, which decodes cleanly into a shortened
  copy.
- **B-prime. The same buffer, but refuse any body that hit `OSC_MAX`.**
  Truncation is already detected, since `string_truncations` counts it. Copies
  up to about 380 bytes work (a URL, a command, a line or two), and longer
  ones are refused and counted. It is little code with no new memory, and it
  could ship first.
- **C. A dedicated, bounded buffer.** Writes honoured, reads refused as today.
  About 256 KiB more per terminal, and every failure is refused whole rather
  than truncated.
- **C with a focus gate**: write only while baud is key. It protects
  background windows only, and does not stop `cat hostile-file` in the window
  you are looking at. It could also refuse a real copy: a yank over a slow ssh
  link arrives one round trip later, and if you Cmd-Tab away at once, the copy
  is refused and you paste the old clipboard. That is reasoning, not a
  measurement.
- **C with a "recent keystroke" gate**: accept a write that arrives soon after
  a key sent to the child. It is a timestamp comparison rather than a wait,
  but it is a time constant, which baud's event-driven rules resist. Offered
  by the reviewer, and not leaned toward.
- **D. No gate.** kitty's documentation describes this, and sfte's code does
  it.
- **E. Also honour reads, behind a confirmation.** It needs a prompt baud does
  not have, and reverses a recorded security decision. Out of scope.
- **Alongside any of B-prime to D: a visible signal on an accepted write**,
  such as a log line and a count in the five-second report. As designed, only
  refusals warn, and an accepted write is silent, which is exactly the
  clipboard-replacement risk.

**Recommendation**

**A fork, with a lean: should programs be allowed to write your clipboard by
default? Lean yes, as kitty and sfte do, with `[clipboard] osc52 = "off"` to
turn it off. The alternative is off by default, as xterm and iTerm2 are. If
yes, no focus gate by default; test a gate live over a real slow ssh link
before adopting one.** Reads stay refused with silence either way. Optionally,
land B-prime first so short copies work at once.

**Design**

These are the researcher's design with the reviewer's four changes applied.

*Core (`baud-core`: no_std, no allocator, no unsafe):*

1. **Where the buffer hangs.** The clipboard body buffer is provisioned by the
   caller, like `sixel_body` (an `Owned` builder method, as `with_images` is).
   It hangs off the existing `StrBuf` reference that `Paint` already carries,
   not as a new slice on `Paint`. New counters go in `Cold`, the struct for
   rarely-changed state. `Paint` is what the per-byte loop (the core's inner
   parsing loop) carries, and extra words there have cost 5-10% before, so
   this is a risk to measure, not a known cost.
2. **Keep the finished copy apart from the one being collected.** Use two
   areas, or drop the published result when a new clipboard string starts. The
   reason is a bug in the first design: `pump` in `baud-app/src/app.rs` feeds
   every chunk it drained and only then polls. So a second OSC 52 that starts
   in the same turn and finishes in the next would overwrite the first one's
   decoded bytes after it was announced, and put garbage on the pasteboard.
3. **Routing.** In `str_put`, a cold path (code that runs rarely, kept out of
   the hot loop), an OSC body whose first three bytes are `52;` switches to a
   clipboard string kind, and the rest of it goes to the clipboard buffer. The
   length keeps counting past the end so an overflow is visible at the
   terminator, as `sixel_len` does. `str_start` resets it, so CAN, SUB and ESC
   cancel it with no new rule. **This is a new routing mechanism and should be
   written down as one**: every other string kind is routed by a graph action
   (`Act::DcsQuery`, `Act::SixelStart` in `baud-core/src/write.rs`).
4. **One grammar and one decoder, in `baud-domain`.**
   - Parse `Pc` into a set of targets (c, p, q, s, 0-7; empty means "s 0"),
     and `?` into a read.
   - For base64, build the one incremental decoder that
     `docs/research/notes/t-images.md` already specifies for Kitty graphics:
     four bytes to three, carrying state across reads, into a caller's buffer.
     Decoding as bytes arrive means the buffer holds text, so the same size
     holds a third more.
   - Pin RFC 4648 (base64's standard) in the research sources before writing
     code; it is **UNPINNED** today.
   - Refusals are an enum (no buffer, clipped, bad base64, empty), never a
     bool.
   - An empty or invalid `Pd` is refused and counted, rather than clearing the
     clipboard as ctlseqs says. That deviation is stated in
     `docs/vt-coverage.md`.
5. **What `Term` exposes.** A generation count that moves once per accepted
   write, and an accessor returning the targets and the bytes.
   `clipboard_writes_dropped` becomes a refused count, and a clipped count
   joins it. Reads are unchanged. The core stores bytes and the app interprets
   them, as with the title (`window_title` in `baud-app/src/title.rs`).

*App (`baud-app`, macOS):*

6. **Poll after each fed chunk**, inside `pump`'s feed loop. The title is
   polled once after all chunks, which is right for a title, because a title
   is state. A clipboard write is an event, and needs the per-chunk poll.
7. **A pure policy function** in a new clip.rs module in baud-app, so its
   tests also run on the Linux lint. It writes when:
   - the config says `write`;
   - the targets include c, p or s (macOS has one pasteboard);
   - the bytes are valid UTF-8.

   Anything else is refused with a reason, counted, and warned once per
   reason. An accepted write goes through `write_text` and is counted in the
   five-second report, which is the visible signal.
8. **Config.** `[clipboard] osc52 = "write" | "off"`, with the default per
   your fork, and `[clipboard] body_kb = 256`. The section goes after `fonts`
   in `config.toml.example`, because a section header placed earlier swallows
   the keys after it.
9. **Linux**: the same poll refuses and counts until the Wayland driver has a
   data device (the Wayland object that carries clipboard data).
10. **Docs, same change**: `SECURITY.md` gains the write policy and its
    threat. `docs/protocol-decisions.md` and `docs/vt-coverage.md` change with
    it. The domain comment that cites xterm only for reads says xterm refuses
    writes too.

*Follow-on constraint*: answering the `Ms` query so neovim detects OSC 52 is a
separate decision, and the pinned xterm-256color entry baud declares carries
no `Ms`.

**Tests and measurement**

Every test is red first. There are three levels, and there is no recorded
level: no recording contains OSC 52, the same situation the HT and SO/SI hand
fixtures cover.

- **Core** (a new OSC 52 test file):
  - A 4 KiB text arrives whole. This is red today: it is dropped, and would be
    truncated at 512 bytes anyway.
  - The same at read sizes 1, 3, 7, 4096 and whole.
  - **Two writes where the second spans into the next read, at several read
    sizes.** This is the reviewer's bug, and single-write tests cannot see it.
  - A body past the buffer is refused whole and counted, and nothing is
    published.
  - Bad base64 or padding is refused, and the generation does not move.
  - An empty `Pd` is refused, and a storage without the buffer refuses.
  - CAN, SUB or ESC mid-body publishes nothing.
  - Targets c, p, s and empty are accepted; q and 0-7 are recorded as such.
  - Controls: `52;c;?` still gets silence and a count (the existing test in
    `baud-core/tests/xterm/osc.rs` stays green), and a 600-byte `OSC 2` title
    is still truncated at `OSC_MAX`.
- **Domain**: RFC 4648's own test vectors, once it is pinned. An exhaustive
  sweep of short inputs against a simple reference in the test. The decoder
  fed at every split point must equal the decoder fed whole.
- **App**:
  - Policy unit tests, one per refusal reason.
  - `the_committed_example_parses_to_what_it_documents` covers the new
    section.
  - A source gate that `pump` calls the policy inside the feed loop, the shape
    of `every_reader_of_the_window_asks_the_clamp`.
- **Live, posting no input**: a `BAUD_CMD` child prints an OSC 52 with baud
  frontmost, then `pbpaste` is compared. Save and restore the clipboard around
  it.
- **Perturbations, each red alone**:
  - publish on overflow;
  - the generation not bumped;
  - the prefix checked at the wrong offset;
  - padding not validated;
  - the finished result overwritten by the next body;
  - the poll moved back after the loop, caught by the source gate;
  - `q` mapped to the pasteboard;
  - a read answered;
  - if a gate ships, the gate removed.
- **Measurement owed**: `ingest2` on cava (the only recording with OSC), btop,
  asciiquarium, cat and cat-cjk.
  - Arms: base, change, and an **A/A copy**, a byte-identical second copy of
    the change that shows how much the instrument varies on its own.
  - All built at one path, seven rounds rotated, the first discarded,
    `cells_written` identical.
  - Plus a comparison of the ingest loop's disassembly, since `Cold`'s layout
    moves.
  - Expected: no result. That is inferred from the cold placement, not
    measured.
  - Memory: +256 KiB per terminal, stated.

**Effort and risk**

- Effort: M, about a day, with the decoder shared with Kitty graphics. B-prime
  alone is S.
- Risk: medium, and it is a policy risk, not a correctness one. Anything
  printed to baud can replace your clipboard. Bracketed paste blunts the worst
  paste-jacking in modern shells, but baud's paste does not strip other
  control bytes.

**What would change it / depends on**

- You judging clipboard replacement unacceptable by default means shipping it
  off. A live slow-ssh test showing a gate costs nothing means adding the
  gate. A regression outside the noise on cava means rework.
- Nothing is needed to decide. Related decisions: answering the `Ms` query,
  paste sanitisation (kitty's `paste_actions` in [[source: kitty-conf]] is the
  precedent), and the Wayland data device, which the Linux half of this and of
  V13 both wait on.

### V12. ?1001 highlight tracking: stop claiming it

**Built 2026-10-11, to the spec, as the owner answered** ("built to the spec",
overriding the recommendation below). `?1001` is the fifth tracking level,
exclusive with the other four as ctlseqs' "five modes of the mouse protocols"
are; a press is normal tracking's; button 1's press is owed the program's
XTHIMOUSE answer, which starts a region from the program's start to an end
that tracks the mouse within the program's rows; the macOS app draws it
through the selection's highlight; and the release is reported as `CSI t` or
`CSI T` in the encoding in force. Nothing waits on the answer. The record has
it, "`?1001` highlight tracking is built to the text", with the questions the
text leaves for Jev. What follows is the question as it was asked.

**The question**

`?1001` is an early-xterm mouse handshake. You press the mouse, and the
terminal reports it. The program answers with the rows it allows to be
highlighted, and the terminal draws the highlight while you drag and reports
where you let go.

baud records the mode and tells programs it is on, but draws nothing. Should
baud build the handshake, or stop claiming it? If it stops, which "no" does it
give when a program asks? The question is **DECRQM**, "is mode N on?", and its
answers are 0 not recognised, 1 set, 2 reset, 3 permanently set and 4
permanently reset.

**What you would notice**

Nothing, either way. No recording, no pinned terminfo and no program found
sets it. What changes is only what a program is told if it asks.

**Today**

- `CSI ? 1001 h/l` sets a flag in `Cold` (`baud-core/src/ops.rs`, 1066-1069),
  and **RIS** (a full reset) clears it at 1310.
- DECRQM answers from that flag (1530-1533).
  The test then named highlight_tracking_is_recorded_and_reported in
  `baud-core/tests/xterm/mouse.rs` pins the answer as `?1001;1$y` (since
  2026-10-11, `highlight_tracking_is_a_level_and_decrqm_reports_it`).
- It does not change mouse reporting. In xterm `?1001` is a mouse mode. In
  baud, a program that sets only `?1001` gets no reports.
- **XTHIMOUSE**, the program's answer (`CSI Ps;Ps;Ps;Ps;Ps T`), is told apart
  from scroll-down by its parameter count, so it never scrolls
  (`baud-domain/src/vt/csi.rs`, 900-914).
- The region it names is stored (`baud-core/src/write.rs`, 1593). The
  accessors `highlight_tracking` and `highlight_region` in
  `baud-core/src/term.rs` are read by tests only, and baud-app never calls
  them.
- **The docs contradict each other.** `docs/vt-coverage.md` and
  `docs/input-coverage.md` say done/recorded. `SECURITY.md` says not
  implemented, for the hang risk. `docs/protocol-decisions.md` lists it as
  recommended against.
  - *Corrected 2026-10-10:* they agree now. `SECURITY.md`, and
    `docs/protocols/xterm.md`, which listed it as declined, say it is
    recorded and not drawn, and that nothing waits on the program's answer.
    `docs/protocol-decisions.md` had already moved it out of its declined
    table, among the rows "built anyway".
- **The house convention for DECRQM:** every mode baud counts and refuses
  (`?1047`, `?1048`, `?9`) answers 0 through the catch-all arm in
  `baud-core/src/ops.rs`. Nothing answers 4 today. The `ModeState` doc in
  `baud-domain/src/vt/mode.rs` and CLAUDE.md both say 0 is the right answer
  for a mode baud does not implement.
  - *Corrected 2026-10-10:* `?9` is no longer among them. It is X10
    compatibility tracking since 2026-10-09, and DECRQM answers it from the
    tracking level (`decrqm_answers_every_mouse_mode`); `?1047` and `?1048`
    still answer 0. And something answers 4: `?2027` with VS16 configured to
    one cell (`one_cell_is_a_configuration_and_decrqm_reports_it`).

**What others do**

- **The spec**, [[source: ctlseqs]]: the mode needs "a cooperating program,
  else xterm will hang". Release reports use single-byte coordinates, the
  **X10 encoding** (one byte per coordinate, so nothing past column or row
  223). That there is no SGR form of the release report is inferred from that
  text. xterm's button.c is not pinned, so it is **UNVERIFIED**.
  - *Corrected 2026-10-09:* button.c is pinned now,
    [[source: xterm-button]], and the file was checked against the
    manifest's SHA-256. It contradicts the inference. `EndExtend` writes the release
    report as `CSI < x ; y t`, and the long form as `CSI < ... T`, under
    `?1006` and `?1016`. The text contradicts it too: ctlseqs' SGR (1006)
    paragraph says "The highlight tracking responses are also modified to an
    SGR-like format, using the same SGR-style scheme and button-encodings."
- **xterm** implements it: [[source: xterm-reset]],
  *srm_VT200_HIGHLIGHT_MOUSE*.
- **foot** declines it and answers 4, permanently reset
  ([[source: foot-csi]]), the same as it does for ?9 and ?1005.
- **kitty, alacritty, wezterm, ghostty, iTerm2, Terminal.app, VTE**: not read,
  **UNVERIFIED**.
- **Usage**: in none of the 21 recordings and in no pinned terminfo entry. A
  web claim that Midnight Commander saves and restores 1001 is **UNVERIFIED**,
  and would not be a use of the handshake anyway.

**Options**

- **A. Build and draw it.** M-L effort for a feature with no known user.
  - It needs a new gesture state tangled with the Shift, Option, Cmd-click and
    scrollbar handling.
  - The X10 encoding cannot name positions past 223.
  - "The program did not answer" needs either a timer, which baud's rules
    reject, or "the next mouse event".
- **B. Keep as is.** No work, but baud keeps telling programs "set" while
  doing nothing. That is `SECURITY.md`'s own reportable class and CLAUDE.md's
  "never record a mode you do not act on", and the three docs keep
  disagreeing.
  - *Corrected 2026-10-10:* the docs agree now (the correction to **Today**'s
    contradiction bullet); the rest of B stands.
- **C. Withdraw, and answer 4** (foot's answer). S. A one-off: nothing else in
  baud answers 4.
  - *Corrected 2026-10-10:* not a one-off: `?2027` with VS16 configured to
    one cell answers 4 (the correction to **Today**'s DECRQM bullet).
- **D. Withdraw, and answer 0** (the house convention). S, and consistent with
  every other mode baud counts and refuses.
- **E. The embedder-claim pattern, as `set_sixel_claim` does.** Keep the flag
  and region, and answer set or reset only when the embedder says it honours
  `?1001`, else 0. It keeps the recorded channel for an embedder, at the price
  of machinery for a feature nobody uses.
- **F. Change the convention for every mode baud recognises and refuses**
  (`?1001`, `?1047`, `?1048`, `?9`, ...) to 4. A repo-wide decision, not this
  item's.
  - *Corrected 2026-10-10:* `?9` is not among them: it is a tracking level
    since 2026-10-09 (the correction to **Today**'s DECRQM bullet).

**Recommendation**

**Stop claiming `?1001`: count `?1001 h`, act on nothing, keep XTHIMOUSE's
parameter-count parse so it can never scroll, remove the flag and the region,
and answer DECRQM with 0 -- yes or no?** Answer 4 only if you separately
decide to move every counted mode to 4 (option F).

**Design**

- **`baud-domain`**:
  - Delete `HighlightTracking` in `baud-domain/src/vt/mouse.rs`.
  - Keep `HighlightRegion` only as the payload of
    `Csi::HighlightMouseTracking`, or collapse it to a variant carrying
    nothing. The parameter-count split is what matters.
  - `PrivateMode::HighlightTracking` stays, so DECRQM can name the mode.
  - Fix a **stranded doc comment** in `baud-domain/src/vt/mode.rs` (293-301):
    the `?1020` paragraph sits on `HighlightTracking`, and `Utf8Resource` has
    none. It is also stale: it says baud "decodes UTF-8 and nothing else", and
    Latin-1 switching exists.
- **`baud-core`**:
  - The set arm in `baud-core/src/ops.rs` counts in `unsupported_param` and
    sets nothing.
  - Remove the explicit DECRQM arm so the mode falls to the not-recognised
    arm.
  - Remove the two `Cold` fields in `baud-core/src/term.rs`, the RIS line, the
    two accessors, and their re-exports in `baud-core/src/lib.rs`.
  - XTHIMOUSE in `baud-core/src/write.rs` counts in `unsupported_total` and
    does nothing.
- **Docs, same commit**:
  - `docs/vt-coverage.md` and `docs/input-coverage.md` say "refused; DECRQM
    answers 0".
  - `SECURITY.md` and `docs/protocol-decisions.md` stay, with the DECRQM
    answer added.
  - CLAUDE.md's paragraph that argued for recording the mode is struck through
    with the reason. This **reverses a recorded decision** (the recorded
    region was kept as a channel for an embedder), and it should read as one.

**Tests and measurement**

- **Red first**: rewrite highlight_tracking_is_recorded_and_reported as a
  refusal test.
  - After `?1001h`, DECRQM replies `?1001;0$y` (red today: `;1$y`).
  - `unsupported_param` moves by one, and mouse tracking stays off.
  - The same answer before any set (red today: `;2$y`).
- **Also rewrite `five_parameter_t_is_not_scroll_down`**: it asserts the
  region through `highlight_region()` today. Its no-scroll assertion stays.
- **Fix the sibling test's comment** that says "there is no counter to read".
  Unknown private modes are counted in `unsupported_param`.
- **Controls that stay green**:
  - `a_final_with_two_arities_means_two_things` (a five-parameter `T` must not
    scroll);
  - `?1000;1001;1006h` still sets 1000 and 1006;
  - the `?9999` answers 0 test in `baud-core/tests/conformance/enumerated.rs`.
- **A correction to the research**: `baud-core/tests/api_coverage.rs` has no
  list to update. It checks that every remaining public function is called by
  a test, and names these two accessors only in its history comment. Removing
  them does not turn it red.
- **Perturbations, each red**: answer set; answer reset; answer 4; drop the
  count; route a five-parameter `T` to scroll-down.
- **No recorded level**: nothing in the corpus sends the mode.
- **Measurement**: removing fields from `Cold` changes the struct the per-byte
  loop carries by reference. So one `ingest2` sweep is owed:
  - payloads btop, asciiquarium, cat and cat-cjk;
  - base, change and an A/A copy, built at one path;
  - the first round dropped, `cells_written` identical;
  - plus a comparison of the ingest loop's disassembly.
  - Expected: no result, which is inferred.

**Effort and risk**

- Effort: S, an hour or two including docs and the sweep.
- Risk: low. The only change a program can see is the DECRQM value, and no
  known program sets the mode.

**What would change it / depends on**

- A real, current program that depends on the handshake would mean option A:
  X10's 223 limit refused and counted, and no waiting. A decision to move
  every counted mode to 4 would mean answering 4 here too.
- It depends on nothing. Option A, if ever built, would draw with V13's
  selection colour.

### V13. Selection colour, and copying when you finish a drag

**The question**

Two choices about dragging to select text:

1. **What colour a selection is drawn.** Today baud swaps each cell's text and
   background colours, which is **reverse video**. The alternatives:
   - macOS's own selection colours, which follow System Settings >
     Appearance > the highlight and accent colour;
   - macOS's selection background, with each character keeping its own text
     colour.
2. **Whether finishing a drag also copies to the clipboard**
   ("copy-on-select"), or only Cmd-C does.

**What you would notice**

1. Under baud's glass, reverse video on ordinary dark-theme text turns a
   selection into opaque light-grey blocks (the dark theme's text is grey
   0xD8), the brightest thing in the window.
2. With copy-on-select, every drag replaces what you copied with Cmd-C,
   because a Mac has one clipboard. Linux and X11 have a second one, the
   **primary selection**, pasted with a middle-click, which is where the habit
   comes from.

**Today**

- **Reverse video on every render path, by one rule.**
  - The CPU and instance paths swap and force both colours opaque in
    `convert_row` (`baud-render/src/renderer.rs`, around 1620-1633).
  - Overlay records (marks and ligatures) take the swapped colour
    (`baud-render/src/overlay.rs`, around 589-604).
  - The cell shader swaps in `colors()`
    (`baud-present-macos/src/shaders.metal`).
  - The block cursor then swaps again.
- **`Theme` has no selection colour** (`baud-render/src/color.rs`).
- **Copy is Cmd-C only**: `copy` in `baud-app/src/app.rs`. A mouse-up copies
  nothing, and a single unmoved click is not a selection (`is_click` in
  `baud-render/src/select.rs`).
- **The stated blocker is stale twice.** `docs/research/notes/k-selection.md`
  says a colour "wants the luma measurement the register still owes (D14)".
  But D14 in `docs/research/notes/j-rendering.md` is frame pacing, and the
  luma under glass was measured (CLAUDE.md, "Luma under glass, measured"; the
  surface is tagged sRGB). The caveat: only opaque cells under the Regular
  glass material were measured
  (`docs/research/notes/j-rendering-evidence.md`).

**What others do**

- **xterm**: reverse video by default ([[source: xterm-reset]], its highlight
  resources). `selectToClipboard` False is pinned, so it uses PRIMARY rather
  than the clipboard. That selecting copies to PRIMARY at all is X11 behaviour
  stated in xterm.man, **UNPINNED**.
- **Programs may set selection colours** with OSC 17 and 19, and reset them
  with 117 and 119 ([[source: ctlseqs]]).
- **kitty**, [[source: kitty-conf]]:
  - Fixed selection colours by default (black on light yellow), with "none"
    for both giving reverse video, and programs may override them.
  - `copy_on_select no` by default, noting macOS has no primary selection.
  - It warns that copying to the clipboard is a security risk, because any
    program, websites included, can read it.
- **foot**, [[source: foot-readme]]: copies a finished selection to primary
  (not under mouse tracking), and supports OSC 17, 19, 117 and 119.
- **macOS**: NSColor offers a selected-text colour and a
  selected-text-background colour, plus an "unemphasized" pair for a window
  that is not key. Read from the local SDK header, **UNPINNED**; pin it before
  building.
- **Others, UNPINNED and UNVERIFIED**: alacritty reverse and no
  copy-on-select; ghostty copy-on-select on since 1.1.0, into a "selection
  clipboard"; wezterm copies on mouse-up; iTerm2 has an option.
- **Terminal.app and iTerm2, the closest Mac peers, are UNVERIFIED.** Whether
  they replace each character's colour (option B) or keep it on a highlight
  background (option C) is unknown. "Looks like every Mac app" is not the same
  as "looks like every Mac terminal", and Apple choosing the pair is a strong
  prior for contrast, not a guarantee once you pick an arbitrary highlight
  colour.

**Options (colour)**

- **A. Keep reverse video.** No work, but bright opaque slabs under glass, and
  a selection over already-reversed text reads as unselected.
- **B. The system pair**: the text colour on the selection background, and the
  unemphasized pair when baud is not key.
  - It is Mac-like and follows your accent colour.
  - Syntax colours inside a selection become one colour, which reverse video
    also does.
  - It needs a colour resolver and a shader rebuild.
- **C. The system background, with each character's own text colour.** It
  keeps syntax colours. But blue text on a blue accent selection is
  unreadable, so it needs a minimum-contrast rule, which is new machinery.
- **D. Fixed configurable colours, kitty-style.** It ignores light and dark
  switching unless given two pairs. Better as an extension, and it is also the
  object OSC 17 and 19 would set.
- **B and C as two values of one key** (`[selection] colors`): the `Theme`
  object can carry both shapes.

**Options (copy-on-select)**

- **Off by default on macOS, with an opt-in.** The opt-in copies drags,
  double-clicks and triple-clicks, but not a single unmoved click.
- **On by default, wezterm-style.** Every drag clobbers Cmd-C's clipboard.
- **A private primary with middle-click paste.** An X11 convention macOS does
  not have, and a second paste path.

**Recommendation**

- **Copy-on-select: off by default on macOS, with an opt-in that copies drags,
  double- and triple-clicks but not single clicks -- yes or no?**
- **Selection colour -- a fork, decided by a look first. Build the `Theme`
  selection object with reverse video as its default either way. Before
  choosing what the app sets, compare Terminal.app and iTerm2 side by side (a
  few minutes). If they keep each character's colour, take C (with a contrast
  rule) or offer both; otherwise take B. Lean: B, if the look supports it.**

**Design**

*`baud-render` (no platform code):*

- **`Theme` gains a selection field**: an enum of reverse, or a fixed pair
  (text and background), plus a background-only form if C is chosen. The
  default is reverse, so every existing gate is unchanged until the app opts
  in.
- **`convert_row`**: a pair sets both colours, opaque, and reverse is today's
  swap. The block cursor still swaps afterwards, pinned by a test.
- **`Overlay::emit`**: a record over a selected cell takes the pair's text
  colour, or its background under the cursor.
- **Damage**: a setter that, when the colours change, marks exactly the rows
  the selection covers as needing a repaint. The existing "selection moved"
  damage rule is extended with "or its colours changed", so a focus change
  repaints the selected rows, not the frame.

*`baud-present-macos`:*

- **The cell shader's uniforms** (small values the CPU hands the GPU for a
  whole draw) gain a selection text word and background word. 0 means "swap",
  the convention `cursor_color` already uses in
  `baud-present-macos/src/metal.rs`. That is safe only because a selection
  pair is always opaque, so a test pins an opaque black pair (0xFF000000) to
  stop anyone loosening it.
- **`colors()` in the shader uses them**, and the committed `baud.metallib`
  (the precompiled shader library) is rebuilt with
  `scripts/build-metallib.sh`. The digest gate in
  `baud-present-macos/tests/metallib.rs` forces the rebuild.
- **A resolver** reads NSColor's selection pair, or the unemphasized one, and
  converts it to sRGB.
  - Whether it needs NSAppearance's per-drawing call is **UNVERIFIED**: baud
    does not force its window's appearance, so the app's current appearance
    may already be the right one.
  - If a block-taking API is needed, it goes in the owner's macos-liquid-glass
    crate. That crate is pinned at `1.0.0-beta.5`, so the change would then
    wait on a crate release, which the effort estimate below does not include.
  - A colour that does not convert falls back to reverse and is counted.

*`baud-app`:*

- **When the colours are resolved**: on the appearance callback (`theme_for`)
  and in `set_focus`, which already runs on every key-window change. A
  highlight colour changed in System Settings is picked up the moment you
  return to baud, with no observer and no polling.
- **Config**: `[selection] colors` and
  `[selection] copy_on_select = "off" | "clipboard"`, in a new section after
  `fonts` in `config.toml.example`.
- **Copy-on-select**: in `select()`'s mouse-up arm, a pure function decides, a
  single click never copies, and a copy calls the existing `copy`.
- **Docs**: rewrite `docs/research/notes/k-selection.md`'s "not in this slice"
  entry and its drawing section. OSC 17, 19, 117 and 119 become an honest
  follow-on once the object exists: `baud-domain/src/vt/osc.rs` says 13-19 are
  out because "this terminal has no object for" them.
- **Linux**: copy-on-select to primary waits on the Wayland driver gaining a
  pointer and a data device.

**Tests and measurement**

- **Red first, on the CPU path**:
  - A selected cell under a pair paints exactly the pair, opaque.
  - A cursor inside a selection pins the inverted pair.
  - An incremental frame equals a full repaint across a colour change.
- **The CPU test is required, not optional.** The **pixel oracle**
  `the_cell_arm_draws_what_the_instance_arm_draws` draws one scene through two
  GPU paths and requires identical pixels, so it cannot see the CPU path.
- **The oracle**: rerun its selection scene under a pair. The two arms must
  agree, and must differ from the reverse frame.
- **A test with an opaque black pair**, for the 0-means-swap sentinel.
- **Perturbations, each red**:
  - the shader ignores the selection uniforms (oracle red);
  - the overlay emits the swapped colour (oracle red);
  - `convert_row` ignores the pair (CPU test red);
  - damage ignores a colour change (repaint test red);
  - a single click copies (copy-decision test red).
- **Live, posting no new input**: select a band and capture it with
  `scripts/luma-swatch.sh` and `scripts/luma-swatch.swift`. Require the
  selected pixels to equal the resolved sRGB pair byte for byte, in dark and
  light, key and not key. That capture also settles the one unmeasured case,
  opaque cells under the Clear material.
- **Measurement**: the `frame_gpu` convert column with an A/A copy, which
  should land far under the keep floor (the larger of 5% and one 120 Hz
  frame); that is inferred. `baud-core` is untouched, so confirm the `ingest2`
  binary is byte-identical instead of sweeping it.

**Effort and risk**

- Effort: M for the colour, about a day, plus a window-crate release if the
  resolver lands there. S for copy-on-select.
- Risk: low to medium. The oracle and the CPU test bound the rendering risk.
  The real risks are a system colour that does not convert (fall back and
  count), and you disliking one text colour inside selections (then C).

**What would change it / depends on**

- The Terminal.app and iTerm2 look decides B against C. A live capture where
  the resolved pair does not reach the screen as resolved means investigating
  the material or colour space first. Your own copy-on-select habit could flip
  that one default, with the design unchanged.
- It depends on nothing to decide. It enables OSC 17, 19, 117 and 119. V12's
  option A would draw with this colour. The Linux half waits on the Wayland
  data device, the same missing piece as V11's Linux half.

## Part 4. Character width

### V14. Ambiguous-width characters: a config switch, or not yet

**The question**

Some characters have no fixed width in Unicode. Their East Asian Width
property (a per-character Unicode label, "EAW" below) is **A, "ambiguous"**:
one column in Western text, two columns in older Chinese, Japanese and Korean
text. baud's core can already count them either way, but nothing in the app
lets anyone choose, so they are always one column. Should `config.toml` get a
switch, and if it does, what else must be built so that "two columns" works
fully rather than half?

What is in the class (138,739 codepoints):
- 137,468 of them (99%) are the **Private Use Area**, the block where Nerd
  Fonts (fonts that add icons for prompts) put their icons;
- the rest include Greek and Cyrillic letters, ±, curly quotes, 112 of the 128
  box-drawing lines, and 20 of the 32 block elements (the shaded bars btop and
  cava draw with).
- **Braille is not in it** (it is EAW N, "neutral"), whatever earlier drafts
  said.

**What you would notice**

Today: nothing. Every such character is one column, which is what this Mac's C
library and your zsh count too (measured, below).

If the switch existed and someone set it to "wide":
- box lines, bars, Greek letters and every Nerd Font icon in the prompt would
  take two columns;
- any program still counting one column would put everything after such a
  character one column off: the shell redraws your command over the wrong
  cells, a TUI's right border sticks out, lines wrap early.

How many characters would change width in the committed recordings:
- cava 41,406 (all block elements); btop 16,907 (12,304 of them box or block);
  vim-unicode 5,798;
- cat-cjk 1,923; less-cjk 471; git-log 152; cat-scroll 133; less-pager 7.
- cat-cjk's raw count of ambiguous characters is 2,937, but 1,014 of them are
  U+FE0F. That is a combining mark (category Mn), and the width generator
  marks those zero-width after it applies "ambiguous", so it stays zero-width
  under "wide". The same holds for 267 of less-cjk's 738.

Also by reading the code, not seen on screen: under "wide" a box line would be
drawn half-length (its mask is one cell), and a narrow font's α or ± would sit
at the left of a two-column slot.

**Today**

The core has the policy and nobody sets it:
- `Term::set_ambiguous_width` in `baud-core/src/term.rs` (lines 1593-1600)
  writes `Cold::ambiguous`, which defaults to `AmbiguousWidth::Narrow`
  (term.rs:462). It takes effect on the next character; cells already written
  keep their width.
- The write path reads the policy on every non-ASCII character through
  `Classified::width` in `baud-domain/src/merged.rs` (line 109), at three
  places in `baud-core/src/write.rs`: 1106 (four characters decoded at once),
  1167 (the leftover characters) and 1314 (a single character).
- `ambiguous_width_follows_the_policy` in `baud-core/tests/encoding/width.rs`
  gates it.
- Nothing calls the setter. The config reader (`apply` in
  `baud-app/src/config.rs`, line 291) has no key for it. The macOS and Linux
  drivers each configure the terminal on their own (`set_sixel_claim` at
  app.rs:789 and linux.rs:145) and share only `host::tell_defaults` in
  `baud-app/src/host.rs`.

What would be wrong the moment "wide" became reachable:
1. `fill_cells3` hardcodes Narrow (term.rs:780). This fills the table behind
   the `cell-table` feature, which is off by default.
2. The renderer draws box and block characters itself, as masks exactly one
   cell wide (`baud-render/src/boxes.rs`, and `Atlas::synthetic` in
   `baud-render/src/atlas.rs`, whose memo at line 337 is keyed on the
   codepoint alone).
3. No program can ask what the policy is. xterm offers a read-only query, mode
   ?1021; baud maps 1021 to `PrivateMode::Other`
   (`baud-domain/src/vt/mode.rs`, line 392) and answers 0, "not recognised".

Documentation slips that exist whatever is decided, and cost nothing to fix:
- term.rs:312-330 stacks three doc comments on `left: Col`, so `ambiguous`
  (353), `cursor_style` (358) and `encoding` (352) have none; the "how
  incoming bytes are decoded" text sits on `c1` (333-344).
- mode.rs:293-300 puts the ?1020 doc on `HighlightTracking`, and it is stale
  ("baud decodes UTF-8 and nothing else, by construction").
- `GraphemeClustering`'s doc (mode.rs:302-303) calls ?1020 permanent. It is
  not: ops.rs:1508-1511 answers 1 or 2 from the live encoding.
- A comment at ops.rs:1555-1562 says "?1020 is not answered here", which is
  false.

Already right under "wide":
- DEC line drawing (the old `ESC ( 0` box set that ncurses uses) is written
  one column whatever the policy (`baud-core/src/paint.rs`, lines 530 and
  566), and those cells carry `CellFlags::GRAPHICS`.
- A full reset (RIS) leaves the policy alone. By reading `hard_reset`
  (ops.rs:1277-1394); no test covers it.

**What others do**

The specification:
- [[source: uax11]] §5: ambiguous characters "should be treated as narrow
  characters by default" when the context cannot be established reliably. §4.2
  lists font information as context, which a terminal has; what a terminal
  cannot know is the *program's* width function. So the narrow default still
  follows from §5.
- [[source: uax11]] §2 note: the property "is not intended for use by modern
  terminal emulators without appropriate tailoring on a case-by-case basis".
  §4.2: private-use codes "may stand in for characters of any width". §6.1
  still classifies Private Use as A by default.

Terminals, from pinned sources:
- **xterm**: *cjkWidth* defaults to narrow; separate *emojiWidth* (off) and
  *privateWidth* (on) settings sit beside it ([[source: xterm-reset]]
  charproc.c:803-810). It exposes them as read-only queries ?1020 to ?1023
  ([[source: ctlseqs]] lines 992-994 and 1519-1524). What each covers,
  and what DECRQM answers for it, is under "V14's open questions,
  answered" below, from the pinned man page and source.
- **VTE** (GNOME Terminal's engine): a setter that takes 1 or 2
  ([[source: vte-vte-cc-0-84]] vte.cc:2154-2163). Its default is a constant
  defined in a file that is not pinned ([[source: vte-internal-hh-0-84]] only
  names it).
- **ghostty**: no ambiguous-width key ([[source: ghostty-config]]; a search
  absence, with *grapheme-width-method* found by the same search as the
  positive control). Its doc warns that widths differing from wcswidth "may
  result in cursor-desync issues with some programs (such as shells)".
- **kitty**: no option ([[source: kitty-conf]], search absence); it has
  *narrow_symbols* for icon glyphs instead.
- **foot**: no ambiguous key ([[source: foot-config]]).

UNPINNED:
- wezterm: *treat_east_asian_ambiguous_width_as_wide*, default false, with a
  doc warning about text UI layout, and *cell_widths* per-range overrides.
- iTerm2 and Terminal.app each have a checkbox; their defaults are UNVERIFIED.
- Windows Terminal: *compatibility.ambiguousWidth* (note G, line 308).
- kitty's OSC 66 width-only subset: a way for a program to fix icon widths
  itself, without a global switch.
- A blog reports glibc 2.39 made ja_JP.UTF-8 count ambiguous characters
  narrow; Debian bug #471021 says the opposite for older glibc. Unresolved.

Measured on this Mac by the reviewer (not pinned; one OS build, macOS 27.0
26A5425a):
- The C library's `wcwidth` (the function programs call to ask how many
  columns a character takes) returns 1 for U+2500, U+2581, ±, α, U+2018,
  U+E0B0 and U+F0000 under ja_JP, zh_CN and ko_KR UTF-8 locales. The locale
  tables are themselves inconsistent: U+0301 and U+FE0F read 1 under
  en_US.UTF-8 and 0 under C.UTF-8.
- Your zsh 5.9.2 counts ─ and the Nerd Font icon U+E0B0 as one column under
  both ja_JP.UTF-8 and en_US.UTF-8. Whether its line editor lays out with the
  same function is UNVERIFIED.
- So on this machine "wide" would disagree with your own shell on box drawing
  and on prompt icons.

**Options**

1. **Leave it API-only (today).** No work. Invisible to the users it exists
   for.
2. **The key alone.** Small. Ships "wide" with half-length box and block
   masks, and the `cell-table` arm disagreeing with the policy.
3. **The key, and make "wide" whole.** Medium. Masks that span two cells, the
   table fill following the policy, one shared setup helper for both drivers.
   The Private Use choice must be made in the same change (see 5).
4. **"auto" from the locale** (LANG/LC_CTYPE). Refuted on macOS by the
   measurement above, and unresolved for glibc. A wrong guess desyncs
   silently. baud already refuses this trade for encoding detection.
5. **Private Use as its own knob.** The reviewer's point: Private Use is 99%
   of the class, UAX #11 asks terminals to tailor, and every program measured
   counts icons as one column. Either a second knob, or a documented rule that
   "wide" includes icons. Not a separate follow-up.
6. **Fix the docs now, build the key when someone asks.** The reviewer's
   revision. Costs almost nothing now, forecloses nothing.
- **3a (only with 3): answer ?1021.** Small, but it waits on pinning xterm's
  man page and misc.c. A mode fixed by config should answer DEC's 3 or 4
  ("permanently set/reset"), not the 1 or 2 baud gives ?1020, which a program
  can change itself.

**Recommendation**

**Fix the documentation slips now, and hold the config key until a user who
needs "wide" asks for it. The default stays narrow. Yes or no?**

My lean follows the reviewer: on this Mac nothing measured would agree with
"wide", so the medium-sized renderer work serves only programs configured by
hand (vim's *ambiwidth=double*, whose effect on box drawing is UNVERIFIED). If
you want the key built now anyway, the answer is option 3 with the Private Use
rule decided in the same change, braille left out of the mask work, and no
"auto" and no ?1021.

**Design**

Now (docs only):
- Move the stranded doc comments onto `ambiguous`, `cursor_style` and
  `encoding` in term.rs.
- Fix mode.rs:293-303 (the ?1020 doc on the wrong variant, and "permanent").
- Fix the false comment at ops.rs:1555-1562, and term.rs:316-323's "there is
  no escape sequence for it", which is true of setting the policy but not of
  querying it.

If the key is built (option 3):
1. **Config** (baud-app): `Config.ambiguous`, of the existing type in
   `baud-domain/src/width.rs`, default Narrow. An `apply` arm for
   `[text] ambiguous_width = "narrow" | "wide"`, quoted or bare; anything
   else, or the key outside `[text]`, complains and keeps Narrow.
   `config.toml.example` gains `[text]` after `fonts`, because a section
   header opens a namespace. The Private Use rule lands here too: a second
   key, or a doc line saying "wide" includes icons.
2. **One place for both drivers**: `Boot` carries the value. A new
   `host::configure` beside `host::tell_defaults` sets the width and takes
   over the duplicated `set_sixel_claim` and `set_cell_size` calls
   (app.rs:788-789, linux.rs:140-145). Both drivers call it right after
   `Term::new`, before the first feed. Read once at launch, so the grid never
   mixes widths.
3. **Core**: `fill_cells3` takes the policy, and `set_ambiguous_width` refills
   a non-empty table when the policy changes. The ingest path does not change.
4. **Renderer**: only box drawing and block elements need wide masks, not
   braille.
   - A mask's span comes from the cell: a lead followed by `WIDE_TAIL` is two
     cells. The atlas memo is keyed on codepoint and span, and `boxes::draw`
     fills a two-cell area.
   - The key is per cell, not per policy, because one frame mixes widths: DEC
     graphics stay narrow (`CellFlags::GRAPHICS` tells them apart without
     looking at a neighbour), and some box pieces are not ambiguous
     (U+254C..254F, U+2574..257F), so they stay narrow even in UTF-8.
   - The GPU cell path draws one glyph per cell from a codepoint map. Either
     the map entry learns the span, or wide box cells take the overlay-record
     route marks and ligatures already use. The pixel oracle (the test
     comparing both GPU paths pixel for pixel) decides; unsettled.
   - Open: under "wide", should a narrow font's α or icon be centred, or drawn
     from the CJK face already in the default stack (Hiragino)?
5. **Docs**: coverage rows for the policy and for ?1021 (answering 0 until
   3a); a note where the key is documented that "wide" needs programs to
   agree.

**Tests and measurement**

Docs now: no code moves. Build `ingest2` before and after at one path and
require identical disassembly.

If the key is built, each test red before its fix:
- **Reader**: "wide" gives Wide; absent gives Narrow; a typo complains and
  gives Narrow; the key at top level complains.
  `the_committed_example_parses_to_what_it_documents` stays silent and counts
  the new line.
- **Helper**: after `host::configure` with Wide, ± advances two columns.
  Perturbed to skip the setter, it goes red.
- **Core** (width.rs): RIS keeps the policy; DECSTR (soft reset) keeps it; DEC
  graphics stay one column while U+2500 takes two; a non-ambiguous box piece
  (U+2574) stays one. One test per write-path spot: four or more ─ in a row
  (the batch decode), two or three (the tail), one alone; each at read sizes
  1, 2, 3, 5 and whole.
- **Cell table**: its equivalence test in `baud-core/tests/cell_table.rs` runs
  under both policies. Red today.
- **Renderer**: a CPU test where "──" under Wide has no background pixel
  across all four columns of the stroke row. The pixel oracle's scene gains a
  wide box row, a narrow-piece row and a DEC-graphics row; both GPU paths
  agree, and differ from the narrow frame.
- **Property**: the policy as an axis of the chunk-invariance property (the
  check that splitting input at any byte gives the same screen).
- **Perturbations**: helper skips the setter; fill hardcodes Narrow; memo
  keyed on codepoint only; DEC graphics measured from the translated
  codepoint.

Measurement:
- Default path: compare `ingest2` disassembly at one path. If the loop
  differs, run an interleaved sweep of all nine recordings plus mix, first
  round discarded, with an A/A copy (a byte-identical second build, to show
  the noise), and require `cells_written` identical.
- Renderer: an interleaved `frame` sweep on btop and cava at the default, with
  an A/A copy; the convert column must not move.
- Under "wide", a cells count is a different program, never a throughput
  figure.

**Effort and risk**

- Effort: docs now, small. The key, medium, most of it the two-cell mask
  across the CPU path and both GPU paths; 3a small but waiting on two pins.
- Risk: low at the default. For "wide" users the desync with one-column
  programs is inherent, and on this Mac that includes libc and zsh. The mask
  change could regress box glyphs at the default; the pixel oracle and btop's
  and cava's golden digests catch it.

**What would change it / depends on**

- A user who needs "wide" asking; or a pinned source showing a platform libc
  answers 2 for EAW A under a CJK locale, which would also make "auto"
  defensible (macOS measured no; glibc unresolved). Pinning xterm's man page
  and misc.c settles 3a and what *privateWidth* means.
- The doc fixes depend on nothing. The key shares the width generator with V15
  (33 of V15's 219 emoji bases are EAW A, so "wide" already widens them), and
  the mask work belongs to the renderer track if it owns boxes.rs and
  atlas.rs.

#### V14's open questions, answered (2026-10-15)

Read for the work list's "V14's open questions, before building", after
the owner's 2026-09-25 call to build the switch with `?1021`. Nothing here
is built. Every xterm file is the patch 411 commit the others are pinned at
(9489b20). `wcwidth.c` and `wcwidth.h` were fetched for this section.

**Positive controls.** Each extraction found something already known
before its misses were trusted:
- ctlseqs found `?1020`'s "utf8 resource (read-only)".
- The man page found `allowTitleOps`, the subject its manifest title names.
- The UAX #11 text found ED6's definition of Ambiguous.
- The trie decode below found ±, 水, U+0301 and ç at their known codes.

**1. The data puts the whole Private Use Area in the class.**
- `EastAsianWidth.txt` gives `A` to `E000..F8FF`, `F0000..FFFFD` and
  `100000..10FFFD` (lines 1690, 2718 and 2719, [[source: eaw-data]]).
- That is 137,468 of the class's 138,739 codepoints, recounted.
- baud's committed trie agrees. A script decoded
  `baud-domain/src/width_data.rs` as `Width::of` does, and every codepoint
  of the three ranges is code 3, "ambiguous". The controls read ± as 3,
  水 as 2, U+0301 as 1 and ç as 0.
- The write path's own table, `Classified` in `baud-domain/src/merged.rs`,
  is held equal to that trie under both policies at every codepoint
  (`the_merged_table_agrees_with_both_tries`).
- Measured through `Term`, by a probe in a scratch tree, with nothing
  committed: under `Wide`, U+E0B0, U+F0186 and U+100000 each advance the
  cursor two columns, as ± does, and five U+E0B0 in a row advance it ten.
  Under `Narrow` each advances it one.
- So `Term::set_ambiguous_width(Wide)` already widens every Nerd Fonts icon
  today. Nothing calls it.

**2. The text does not settle whether "wide" must cover it, and does not
make excluding it a departure.** [[source: uax11]]:
- § 6.1: "All private-use characters are by default classified as
  Ambiguous, because their definition depends on context."
- § 4.2: "Private-use character codes and the replacement character have
  ambiguous width, because they may stand in for characters of any width."
  The same paragraph: "The term context as used here includes extra
  information such as explicit markup, knowledge of the source code page,
  font information, or language and script identification."
- § 5: "Ambiguous characters behave like wide or narrow characters
  depending on the context (language tag, script identification, associated
  font, source of data, or explicit markup; all can provide the context)."
- § 2: "the guidelines on use of this property should be considered
  recommendations based on a particular legacy practice that may be
  overridden by implementations as necessary". Its note: the property "is
  not intended for use by modern terminal emulators without appropriate
  tailoring on a case-by-case basis".

So `A` is the default classification of a code whose width the annex says
context decides. It names font information as context, and it invites
terminals to tailor. Two contexts are in play, and they point opposite
ways:
- **The font.** The embedded icon face draws every icon one cell wide.
- **The program's width function**, which "wide" exists to match. A
  function that widens PUA counts two columns; one that does not counts
  one.

Both readings conform. The work list's premise, that excluding PUA "would
be a departure", is not what the text says, and the choice is still not a
free one: it goes to Jev at 0.9 on both asks, or to the owner. xterm's
default excludes PUA (point 3). That is practice, not the standard.

**3. What `?1021`, `?1022` and `?1023` report.** [[source: ctlseqs]]
(992-994, 1519-1524) calls them read-only and ties them to the resources
"utf8, cjkWidth, emojiWidth, and privateWidth respectively". xterm:
- **DECRQM answers 1 or 2 for all three, never 3 or 4.**
  [[source: xterm-misc]]'s `do_dec_rqm` (5466) answers each with `MdBool`
  (5845-5853). `MdBool` is `mdMaybeSet` (1) or `mdMaybeReset` (2), from the
  enum at 5387-5393.
- **Nothing changes them during a session.** `dpmodes`, `savemodes` and
  `restoremodes` all skip 1020-1023: "these are readonly, not saved or
  restored" ([[source: xterm-reset]] charproc.c 7998-8003, 8352-8357 and
  8800-8805). In the pinned files only start-up sets the resources
  (11668-11698); `menu.c` and `main.c` are not pinned. So xterm answers 1
  or 2 for values that DECSET cannot change.

| mode | resource | xterm's default | DECRQM in xterm | set means |
|---|---|---|---|---|
| `?1021` | cjkWidth | false (charproc.c 803) | `2` | East Asian Ambiguous characters take two columns |
| `?1022` | emojiWidth | false (808) | `2` | VS15 and VS16 set an emoji's width; listed emoji characters take two columns |
| `?1023` | privateWidth | **true** (809) | `1` | Private Use codes take one column even under cjkWidth |

- **`?1021`.** [[source: xterm-man]] 2900-2911: "When turned on,
  characters with East Asian Ambiguous (A) category in UTR 11 have a column
  width of 2." `decode_wcwidth` then selects `mk_wcwidth_cjk`
  ([[source: xterm-scroll]], util.c 6292-6318). While it is off, mkWidth
  chooses between the system's `wcwidth` and the built-in table (man
  4538-4557).

  The answer reports the resource. It does not report whether ambiguous
  characters are actually narrow: with both resources off, the system's
  table decides.
- **`?1022`.** Man 3708-3712: "Specifies whether to interpret Unicode
  variation selectors VS15 and VS16. Those override the character width of
  the immediately preceding character, if it is an Emoji, to one or two
  cells, respectively." The code does that:
  - VS15 makes the width 1 and VS16 makes it 2, for a base on its emoji
    list (util.c 5988-5995).
  - Within xterm's built-in tables, it also gives two columns to every
    listed character above U+00FF, bare (`mk_wcwidth`,
    [[source: xterm-wcwidth-c]] 864). The list comes from emoji-data and
    takes in U+2122 ™. When xterm uses the system's `wcwidth`, the system
    decides that width.

  baud's `Vs16Width::Two`, the default, matches neither answer:
  - It widens a listed base on U+FE0F.
  - It never narrows on U+FE0E (V15, after the mode 2027 draft).
  - It leaves a bare ™ one column.

  So "set" would claim two things baud does not do, and "reset" would deny
  one it does. By reading, `Vs16Width::One` behaves as emojiWidth false.
- **`?1023`.** Man 5202-5206: "Tells xterm whether to assume that codes in
  the Unicode PUA (private use area) are single-width, or neutral (matching
  the cjkWidth setting. The default is "true" to assume single-width."
  "Neutral" there means "follows cjkWidth", not EAW `N`. The chain:
  - `XTermWcInit` turns the resource into the `WcPrivateFullwidth` bit
    ([[source: xterm-xterm-h]] 1131-1134).
  - That bit sets `use_cjk_pua2` (wcwidth.c 777-779).
  - `mk_wcwidth_cjk` then returns 1 for a private-use code (887-888) before
    the ambiguous table is consulted. The table carries all three ranges
    (228-231).

  `mk_wcwidth` gives private use one column whatever is set (862-863). So
  in xterm's own tables a Nerd Fonts icon takes two columns only with
  cjkWidth true and privateWidth false. With cjkWidth false and the
  system's `wcwidth` in use, neither resource applies and the system
  decides.

  **A name trap.** [[source: xterm-wcwidth-h]] line 42 glosses the bit as
  "private-use codes can be fullwidth in CJK". When set, it does the
  opposite. The man page and `ptyx.h` 3449 ("true if PUA codes are always
  single-width") describe the code; the bit's comment does not.

**How `?1023` relates to the PUA answer.** It is the PUA question as a
mode, so each answer to point 2 fixes it:
- If PUA follows "wide", that is privateWidth false: `?1023` answers reset,
  whatever the switch says.
- If PUA always takes one column, that is privateWidth true, xterm's
  default: it answers set.
- If PUA gets its own knob, it answers the knob.

Under the narrow default all three draw PUA one column. `?1023` differs
only in what it says about "wide".

**What a user sees if PUA goes wide.** By reading the code; nothing was
drawn.
- The icons come from Symbols Nerd Font Mono, "sized to one cell"
  (`baud-app/src/embedded.rs`).
- In a two-cell span an icon keeps the grid's phase: `is_icon` in
  `baud-render/src/renderer.rs` (357), held by `an_icon_keeps_the_grid_phase`
  (`baud-render/tests/narrow_in_wide.rs`, 246). So each icon draws in the
  left cell, with its right cell blank.
- A powerline prompt shows a blank cell after every separator.
- Everything after an icon moves one column right for a program counting it
  as one. zsh counts U+E0B0 as one on this Mac (measured above).
- Of the committed recordings, only `sixel/yazi-walk.raw` carries PUA: 83
  icons, among them U+E0B4 28 times, U+E60D 23 and U+E0B6 17. None of the
  ten others has any.

**The options for the PUA question.** All of them are for Jev at 0.9 on
both asks, or for the owner.
1. **PUA follows "wide".** This is the data's default, and today's API
   already does it. **Conforming** (§ 6.1). `?1023` answers reset.
2. **PUA always one column**, xterm's default. **Conforming** as the
   tailoring § 2 invites, with the embedded face as the § 4.2/§ 5 context.
   `?1023` answers set.

   The cost is in the write path's table, `Classified` (merged.rs), and
   `Width::of` must agree with it. `Classified` keeps its width code in a
   whole byte: the palette's third byte, and `BMP_DIRECT`'s high byte. So
   a fifth code, "private use", fits there with no wider table, and only
   the `Wide` arm of `Classified::width` reads it.

   `Width::of`'s trie packs two bits a codepoint, and its four codes are
   all used (0 narrow, 1 zero, 2 wide, 3 ambiguous). So it needs either a
   test of the three ranges inside its `ambiguous == Wide` arm, or a wider
   trie.

   By reading, neither change touches the narrow default's path. The
   `ingest2` disassembly compare decides that.
3. **A knob of its own**, xterm's model, beside the switch. **Conforming**
   at either default. `?1023` answers the knob. Its default is a second
   choice of the same kind.
4. **Width from the font's advance.** This follows § 5's "associated font"
   literally, so it is **conforming** to the annex. It breaks baud's wire
   contract that width never reads a font (`DisplayWidth` in
   `baud-domain/src/width.rs`): the core has no font, and the program on
   the far end cannot see one.

**The DECRQM value: 1 or 2, or 3 or 4.** ctlseqs gives no value rule for
a read-only mode. These options are for Jev or the owner too.
- **(i) 1 or 2, as xterm answers.** Conforming. It matches baud's own
  `?1020`.
- **(ii) 3 or 4.** Conforming to ctlseqs' definitions ("3 - permanently
  set", "4 - permanently reset", 1510-1511), and true of a value read once
  at launch. baud's `?2027` already answers this way. It departs from
  xterm's practice: a program comparing against xterm sees 3 where xterm
  says 1.

Option 3a above leaned to (ii) before `misc.c` had been read.

**`?1022`** is a separate call. Options:
- **0**, under the house rule that an unimplemented mode answers "not
  recognized". baud has no emojiWidth.
- **2 under `Vs16Width::One`, and 0 under `Two`.** Truthful in both, but
  the answer then depends on configuration.
- **1 under `Two`.** Departs: it claims VS15 narrowing and a two-column
  bare ™.

**What V14's build needs.**
1. The calls above: the PUA rule, the DECRQM values and `?1022`.
2. The config key, as in note V's design step 1, plus a second key under
   option 3.
3. Core:
   - The width policy carries the PUA rule.
   - Under options 2 and 3, `Classified` gets a fifth width code for
     private use (`scripts/gen-merged-table.py`).
   - `Width::of` gets the range test, so
     `the_merged_table_agrees_with_both_tries` still holds.
   - `fill_cells3` follows the policy (V14's first defect).
4. Modes:
   - `PrivateMode` names 1021, 1022 and 1023. Today they are `Other` and
     answered 0 (`baud-domain/src/vt/mode.rs` 694).
   - DECRQM answers as decided.
   - DECSET and DECRST of them are counted in `unsupported_param`, as
     `?1020` is.
   - They get no `PrivateMode::slot`, as xterm's `savemodes` keeps none.
5. Renderer:
   - Two-cell box and block masks, as in design step 4.
   - Under option 1, whether a wide icon is centred joins that step's open
     question. Today `an_icon_keeps_the_grid_phase` pins it left.
6. Tests:
   - Note V's list.
   - DECRQM for each of the three modes under both policies.
   - DECSET `?1021` changes nothing and is counted.
   - XTSAVE keeps no slot for any of them.
   - U+E0B0, U+F0186 and U+100000 under Wide, against ± as the control,
     per the PUA rule picked.
7. Docs:
   - Coverage rows for `?1021`, `?1022` and `?1023`.
   - The `Cold::ambiguous` doc (term.rs 615-617) when it lands.
   - Two stale lines in `baud-core/src/ops.rs` (2883-2884) say "xterm's
     `DECRQM` handler is not pinned". It is: `do_dec_rqm`, as above.
     It answers `?1047` and `?1049` from `whichBuf`, and `?47` too unless
     ReGIS print graphics is configured (misc.c 5610-5617), as baud's
     choice does.
8. Measurement: if `Classified` or `Width::of` changes, run the
   disassembly compare at the default and, if the loop differs, the
   interleaved sweep, as above.

**Built 2026-10-15** (thread t-0015; `docs/record.md`, "The ambiguous-width
switch, with `?1021` to `?1023`"): items 1-4, 6, 7 and 8. Item 3 needed no
fifth code: `emit_merged_table` derives the code by asking `Width::of` under
both policies, so with the range test in its `Wide` arm private use comes
out narrow, code 0, and `Classified::width` is unchanged. Item 5, the
renderer's two-cell masks, is not built.

### V15. Mode 2027 and U+FE0F: stop the false claim, then make it a real switch

**The question**

Mode 2027 is a draft proposal (from the contour terminal project) for how
terminals handle Unicode text. A program asks about it with DECRQM, the "is
mode N on?" query. baud answers **3, "permanently set"**: "I always do this".
The draft says that while the mode is on, U+FE0F (VARIATION SELECTOR-16,
"VS16", an invisible marker meaning "show the character before me as a colour
emoji") must make that character two columns wide. baud keeps it one column.
So baud's answer is false. Decide: stop claiming, widen always, or widen only
while a program has switched the mode on.

**What you would notice**

Today:
- U+2764 (heart) followed by VS16 is one column. By reading the renderer,
  which picks a font by the first character alone, the colour glyph can spill
  onto its neighbour; not looked at on screen.
- In the corpus, cat-cjk carries 1,014 VS16s and less-cjk 267, every one
  inside the rainbow-flag sequence after U+1F3F3 (white flag). No other
  recording has any.

Under the recommendation:
- Step one (answer 4): nothing on screen changes.
- Step two (a real mode, off by default): `cat` output is unchanged. A program
  that switches the mode on, such as Neovim, gets U+2764 plus VS16 drawn two
  columns wide, matching what it counts.

Your shell, measured by the reviewer (zsh 5.9.2, not pinned):
- It counts U+2764 plus VS16, the keycap 1 plus VS16 plus U+20E3, and U+263A
  plus VS16 as one column each. That matches baud today and the off default.
- It counts U+1F3F3 alone as 2 (Unicode 17 and baud say 1), the rainbow flag
  as 4 and a three-person family as 6: it adds up codepoints and does not
  group them. So flags and families disagree with your shell under **every**
  option.

**Today**

The claim:
- `mode_state` answers
  `PrivateMode::GraphemeClustering => ModeState::PermanentlySet`
  (`baud-core/src/ops.rs`, line 1567); the doc at `baud-domain/src/vt/mode.rs`
  lines 302-304 says "baud always clusters".
- `decrqm_reports_grapheme_clustering_as_permanent` in
  `baud-core/tests/encoding/width.rs` pins the 3. `docs/vt-coverage.md` lines
  219 and 359 record it as done.
- `?2027h` and `?2027l` are counted in `unsupported_param` (ops.rs:1109), not
  silently dropped.

How VS16 is handled:
- It is a combining mark (category Mn), so the generator makes it zero-width
  (`scripts/gen-width-tables.py`, line 77).
- It goes to `Paint::attach` (`baud-core/src/paint.rs`, line 756, on a cold
  path, meaning rarely run and kept out of the hot loop). That joins it to the
  cell's cluster in `ClusterStore` and never changes the base's width.
  `baud-domain/src/width.rs` lines 5-15 record the gap.
- The tests in this area check codepoints, not width:
  `a_flag_sequence_keeps_all_of_its_codepoints` in `baud-core/tests/marks.rs`
  checks only that nothing was dropped.

Already matching the draft: characters Unicode marks as emoji by default are
two columns; VS15 (the "show as text" marker) changes no width; ZWJ families
(emoji joined by U+200D ZERO WIDTH JOINER) form one cell. A mark sent after
the cursor has moved away and back still finds its base
(`a_mark_finds_its_base_after_the_cursor_has_been_away`); the draft is silent
there, and ghostty does the same ([[source: ghostty-terminal-9bbb9b2]]
Terminal.zig:1226-1265).

Two existing problems the review found, both found by reading:
- **A mark after the last column with autowrap off lands on the wrong cell.**
  With DECAWM (autowrap, `?7`) reset, `Paint::advance` (paint.rs:600-605)
  leaves the cursor *on* the character just written in the last column, and
  `attach` always takes the cell one to the left of the cursor (paint.rs:775).
  So an accent sent there joins the neighbour. It is wrong today for combining
  marks; no test covers a mark after `?7l`. Pinned ghostty checks whether the
  cursor's own cell holds the base (Terminal.zig:1242-1246).
  **Fixed in two halves**: the joined mark on 2026-10-10 (`docs/record.md`,
  "With `DECAWM` reset the character just written is under the cursor"), and
  the policy path, a mark after a control or a `U+200B`, on 2026-10-11 ("A
  zero-width character after a control goes where it goes with autowrap
  set"), by the pin kept fresh across controls rather than by ghostty's
  content check, which cannot tell a cell just written from one the cursor
  moved onto.
- **`ModeBit::LAST` is stale.** It names `AltSendsEscape` (mode.rs:219), but
  `Column132`, `SynchronizedOutput` and `InbandResize` are declared after it,
  so the guard against overflowing the 32-bit mode word checks the wrong
  variant. Harmless at 19 bits; adding a bit must fix it. (`Modes` has 19
  variants, lines 124-204, not 18.)

A research note is wrong about its own source:
`docs/research/notes/g-text-encoding.md` lines 432-436 say the draft "says
nothing about how to compute a cluster's width". It has a VS16 rule, a VS15
rule and a ZWJ rule, and the note's recommendation to answer 3 rests on that
misreading. The 3 also cites Windows Terminal doing the same
(width.rs:139-141, note G lines 537-539, UNPINNED); whether Windows Terminal
widens on VS16 is UNVERIFIED, so it cannot vouch for the answer.

**What others do**

The draft, UNPINNED (contour-terminal/terminal-unicode-core at 64f53851,
sha256 f23237de…, matching the pin note G proposed; not in the manifest):
- "The following set of semantics MUST be adhered to if this VT mode 2027 is
  enabled."
- VS16 "will force the grapheme cluster's width to be 2", which may wrap it to
  the next line at the right margin with autowrap on.
- VS15 "will NOT change the underlying width."
- "ZWJ emoji are required to be displayed as a single image with a width of 2
  grid cells."
- With the mode off everything is "undefined"; the program must enable it;
  DECSTR (soft reset) and RIS (full reset) reset it to undefined.

The reply values: 1 set, 2 reset, 3 permanently set, 4 permanently reset
([[source: ctlseqs]] lines 1502-1511).

Unicode:
- [[source: uts51-emoji]]: U+FE0F is the emoji presentation selector (ED-9);
  emoji are square, about the width of a CJK ideograph (§2.2); a text-default
  emoji should get VS16 when emoji presentation is wanted (§2.7.1). By ED-15
  and ED-15a, an unqualified U+2764 ZWJ U+1F525 (no VS16) is still an emoji
  ZWJ sequence. A skin-tone modifier sequence "should be displayed with an
  emoji presentation by default".
- [[source: emoji-data]]: U+2764 and U+1F3F3 are emoji but not
  emoji-by-default. Nine skin-tone-capable bases are also not
  emoji-by-default: U+261D, 26F9, 270C, 270D, 1F3CB, 1F3CC, 1F574, 1F575,
  1F590. By reading, U+261D plus a skin tone stays one column in baud.
- [[source: uax11]] §5 note: variation sequences can specify a narrow or wide
  presentation that differs from the base's width.
- emoji-variation-sequences.txt 17.0.0, UNPINNED: 371 characters take VS16.
  152 are already emoji-by-default (two columns). The other 219 are exactly
  "emoji and not emoji-by-default" (by EAW: 168 N, 33 A, 12 Na, 6 W).

Terminals, pinned:
- **kitty** ([[source: kitty-screen-0-49]]): widens a one-column cell when
  VS16 directly follows an emoji-capable base (screen.c:1215-1218, testing the
  character just before VS16); halves a wide one on VS15 (1219-1231, against
  the draft); widens on the Thai/Lao SARA AM mark (1232-1237). kitty does not
  implement 2027 (note G, UNPINNED).
- **ghostty**: its whole grapheme path, clustering and width, runs only while
  2027 is set (Terminal.zig:1220-1227). "Wide" wraps a base in the last column
  onto the next row, leaving a spacer cell that reflow drops (1291-1360, the
  spacer at 1319). [[source: ghostty-config]] (Config.zig:492-517):
  *grapheme-width-method* defaults to unicode, a program enabling 2027 forces
  it, and the doc warns of shell desync. That 2027 is on by default rests on
  the unpinned modes.zig.
- **VTE**: vte.cc 0.84.1 has no reference to U+FE0F
  ([[source: vte-vte-cc-0-84]]; its other files were not read).
- **foot**: *grapheme-width-method* defaults to double-width
  ([[source: foot-config]]).

UNPINNED:
- ghostty's grapheme.zig: VS16 widens only after an emoji-capable base, and
  any other non-zero-width continuation widens too.
- wezterm: widens on explicit emoji presentation from Unicode version 14, but
  its docs say the default version is 9, so by default it does not.
- **Neovim** (master, fetched 2026-09-25): it asks about ?2027, sends `?2027h`
  only when the reply says the mode is off, and `?2027l` on exit
  (tui.c:256-261, 677-678). It counts a character plus VS16 as two cells under
  its own `emoji` option, whatever the terminal answers (mbyte.c:650-657), and
  its *print_cell_at_pos* clears two cells and repositions the cursor after
  one, so baud's one-column drawing does not break its layout. **The real cost
  of answering 3 is that Neovim never sends `?2027h`**, so a baud that
  implemented the widening could never be switched on by it.

**Options**

- **A. Keep answering 3, do not widen (today).** No work. The answer is false.
- **B. Answer 4, "permanently reset", do not widen.** Small: one line, a test,
  the coverage row. Honest at once, no rendering or pin change. Changes
  nothing for Neovim, which pads anyway.
- **C. Widen always, keep answering 3.** kitty-like. Desyncs programs that add
  up `wcwidth` (which gives VS16 zero), with no way out, and needs the golden
  digests (pinned whole-screen hashes of each recording, in
  `baud-core/tests/golden.rs`) and the sfte oracle (a second terminal baud is
  compared against) re-pinned for cat-cjk and less-cjk.
- **D. A real mode, off at power-on, widening only while on.** The draft's own
  model: the program opts in. Shells keep today's widths; Neovim gets the
  widths it counts; the answer is true in both states; pins unchanged. Medium.
- **D'. The same mode, on at power-on.** ghostty-like (UNPINNED). C's shell
  desync by default, the same re-pin, and it departs from the draft's reset
  rule.
- **G, with D: also widen on a joined or skin-toned emoji.** A skin-tone
  modifier, or a pictograph joined by ZWJ, after a one-column base. The
  reviewer's point: once D answers 1, the draft's ZWJ rule is part of the
  claim, so leaving G out repeats the false claim D exists to remove.
- **Excluded**: narrowing on VS15 (the draft forbids it) and widening on
  spacing marks (disputed; kitty's maintainer said Devanagari vowel signs
  "definitely dont double grapheme width").

**Recommendation**

**Yes to two steps: answer 4 now (a one-line honest fix), then build D, a real
mode 2027 that is off at power-on and after DECSTR or RIS, with the
joined-emoji and skin-tone rule (G) included rather than deferred, and after
fixing the two existing problems first.**

Your zsh counting U+2764 plus VS16 as one column is what makes off the right
default. If you prefer `cat` to show emoji at two columns and accept the shell
desync, the answer is D' instead.

**Design**

Step one, now: `mode_state` answers 4 for mode 2027. Replace the test that
pins 3, with the reason written down, and correct note G §4.5, its
recommendation 4, and the coverage prose at vt-coverage.md:358-364.

Step two, the real mode:
1. **Pin first.** Add the mode 2027 draft and emoji-variation-sequences 17.0.0
   to `docs/research/sources/MANIFEST.toml` and run
   `scripts/check-sources.sh`. Until then the code cannot cite them.
2. **Fix what exists, each with a red test**: `attach` finds the base as the
   cursor's own cell when the cursor is pinned on it with autowrap off
   (ghostty's check); `ModeBit::LAST` names the real last variant.
3. **baud-domain**: a new `ModeBit::GraphemeWidth`, reset in
   `Modes::default()`, with LAST updated to it. A rewritten
   `GraphemeClustering` doc: clustering is always on, the bit gates the width
   rules. A generated two-stage lookup table (a small index into bit blocks,
   so no search) of the 219 "emoji and not emoji-by-default" characters,
   checked at all 1,114,112 codepoints against the pinned variation-sequences
   file. Deliberately not a bit in the per-character table the hot loop reads,
   which the hot path never needs and whose change would move every payload's
   layout.
4. **baud-core, ops.rs**: `set_private_mode` gets an arm for 2027;
   `mode_state` answers 1 or 2 from the bit; `soft_reset` clears it (the
   draft's DECSTR sentence); `hard_reset` already resets all modes (line
   1307).
5. **baud-core, `attach`** (stays cold): widen when the mode is on, the owner
   cell is one column, and either the codepoint is VS16 after a character in
   the table, or (G) it is a skin-tone modifier or a ZWJ-joined pictograph.
   "After" means the codepoint immediately before, not the cluster's first, as
   kitty tests.
   - **The widen must not depend on storage.** `attach` returns early when a
     cluster is full (`ClusterSlot::CAPACITY`) and does nothing when the
     63-slot store refuses, which a long session does (72 refusals on
     cat-cjk). The widen runs before those exits, and the preceding codepoint
     comes from the stream, not only from stored marks; otherwise a long
     session silently stops widening while the program still counts two
     columns.
6. **A new `Paint::widen`** (cold, typed arguments to keep baud-core's zero
   signature primitives):
   - Room to the right: rewrite the owner with a `WIDE_TAIL` through
     `view.put_glyph` (which repairs a wide character it half-covers and
     raises the row extent), count the cell, move the cursor one, clear images
     there, update `row_written`.
   - At the right margin with autowrap on: move the cluster whole to the next
     row's left margin. What the vacated column holds is open: a styled blank
     reads as content to the extent and reflow rules, and ghostty leaves a
     spacer that reflow drops. Match what `glyph` leaves when a wide character
     does not fit (it leaves the column untouched, paint.rs:668), and prove it
     with a reflow round trip.
   - Autowrap off: stay narrow and count it in `emoji_unwidened`. Reachable
     only after the fix in step 2.
   - Insert mode (IRM): open a one-column gap first. This is baud's choice;
     ghostty's is UNVERIFIED. REP's `last_cp` is untouched.
7. **Renderer**: no geometry change. A widened cluster is a wide marked cell,
   the path ZWJ families already take. Steering VS16 clusters to the colour
   font is a renderer-track follow-up.
8. **Docs**: vt-coverage.md (219 and 355-366), width.rs's gap note, CLAUDE.md.
   Record VS15 not narrowing as conformance.

**Tests and measurement**

Step one: DECRQM ?2027 answers 4.

Step two, each red before its fix:
1. DECRQM: 2 at power-on, 1 after `?2027h`, 2 after `?2027l`, 2 after DECSTR,
   2 after RIS.
2. With the mode on, U+2764 VS16 then X puts the cursor at column 3 (base,
   tail, X). The control with the mode off gives column 2, byte-identical to
   today.
3. x plus VS16 stays one column. U+2764 VS15 VS16 stays one column, which
   tells "the codepoint before" from "the first codepoint".
4. Keycaps (1 and # with VS16 and U+20E3) take two columns, with 1, 2 and 3+
   ASCII characters before them, reaching both ASCII write paths
   (paint.rs:528-531 and `blit`).
5. The rainbow flag takes two columns. With G: unqualified U+2764 ZWJ U+1F525,
   and U+261D with a skin tone, take two columns with the mode on and one with
   it off.
6. U+231A VS15 stays two columns.
7. Right margin with autowrap on: wide on the next row, row marked wrapped,
   cursor at column 2, plus the reflow round trip. Autowrap off: narrow,
   `emoji_unwidened` reads 1.
8. The existing bug: a base in the last column with `?7l`, then U+0301, joins
   the base, not its neighbour.
9. A full cluster store still widens.
10. Orphan repair (中 at columns 1-2, the heart at 0, then VS16), and IRM
    shifting the text right.
11. Row-extent gates: `every_row_is_uniform_past_its_mark` over the new
    fixtures, the styled-tail variant (row_extent.rs:306) extended with a
    margin widen in colour, and `copy_row`'s debug assertion reached by a
    region scroll.
12. Chunk invariance at read sizes 1, 2, 3, 5 and whole, including the base in
    one read and VS16 in the next.
13. The generator gate against the variation-sequences file, and property ops
    for base plus VS16, non-base plus VS16, and `?2027h`/`?2027l`.
14. With V14's "wide" (if built) and this mode both on, the 33 ambiguous
    bases.

Perturbations, each red at its own test: widen without the mode check; without
the base check; testing the first codepoint; no wrap; no orphan repair; DECSTR
or RIS not clearing the mode; no IRM gap; the widen not raising the extent;
the widen gated on the store accepting.

Measurement: at the default the new code runs only inside the cold `attach`,
so `cells_written` must be byte-identical on all nine recordings and mix. Run
interleaved `ingest2`, both arms built at one path, five rounds or more with
the first discarded and an A/A copy; all nine (the new table is new read-only
data that can shift layout), watching cat-cjk, less-cjk and vim-unicode. With
the mode on, a cells count is a different program, not a throughput figure.
The frame benches are not owed, and the golden and sfte pins do not move.

**Effort and risk**

- Effort: step one small. Step two medium: the pins, the generated table and
  its gate, the mode wiring, the widen with its wrap, orphan, insert-mode and
  extent handling (the bulk), rule G, the two existing fixes, about eighteen
  tests and the docs.
- Risk: medium on the margin wrap, which moves an already-written cell with
  its cluster (ghostty's version shows how many corners that has). Low on the
  default path: one compare on a cold path, no pin changes.

**What would change it / depends on**

- The falsity of today's answer is certain; the confidence is medium only on
  the default. Evidence that your shell counts U+2764 plus VS16 as two columns
  would make D' or C free at the prompt; the measurement says one. A revised
  draft or a Unicode Terminal Working Group rule (L2/25-263) would supersede
  this.
- Step one depends on nothing. Step two depends on pinning the draft and the
  variation-sequences file, shares generator work with V14, and leaves
  colour-font steering to the renderer track.

## Part 5. Colour

### V16. A light-mode set of the sixteen ANSI colours

**Re-researched and decided in note Y** (`docs/research/notes/y-palettes.md`),
which supersedes this section where they differ. The owner's decisions of
2026-09-29: no vogix-specific work until vogix 1.0.0-beta.1; scheme files by
each format's published mapping (base16 and base24 by tinted-theming's table,
ghostty-style files keeping their own page and text) onto a scheme type later
formats can join; Modus Operandi's sixteen values as the built-in light set,
credited, no file text copied; the palette following System Settings >
Appearance while the glass follows Icon & widget style; an OS flip under
`auto` resetting program colours (OSC 4/10/11/12), with 996/997 reporting the
class of what OSC 10/11 answer; a contrast floor on by default, trigger about
1.2, lifted to 4.5:1; dim_light measured, then set; a readable dark set
researched next; Reduce Transparency drawing backgrounds opaque and Increase
Contrast raising the floor; faint blended toward the background. Corrections
note Y makes to this section: ghostty's floor snaps to black or white rather
than nudging; iTerm2 applies Minimum Contrast per appearance; the WCAG formula
is pinned; the owner's `ls` is lsd.

**The question**

Programs colour their text with sixteen numbered "ANSI" colours: black, red,
green, yellow, blue, magenta, cyan and white, plus a bright version of each.
The program sends only the number; the terminal picks the actual colour. In
light mode baud draws dark text on near-white but keeps xterm's sixteen, and
xterm's yellow, cyan and green barely show on white. Should baud let you set a
separate sixteen for light and dark, ship its own light set by default, or fix
contrast automatically at draw time? And whose opinion are the numbers?

**What you would notice**

A *contrast ratio* compares how bright the text is against its background: 1:1
is invisible, 21:1 is black on white. WCAG, the web's accessibility guideline,
asks for 4.5:1 for body text; that figure is quoted in [[source: kitty-conf]].
Its 3:1 large-text figure and the formula itself are UNPINNED.

- **Today, in light mode**, GNU `ls --color` draws executables in bold green
  at **1.19:1**, symlinks in bold cyan at **1.09:1** and directories in bold
  blue at 4.12:1. So executables and symlinks are effectively invisible. This
  assumes GNU ls, which the owner's PATH has (coreutils 9.11). A stock macOS
  `ls -G` uses other defaults, UNPINNED.
- **Across the palette**: 10 of the 12 coloured entries are below 4.5:1 in
  light mode and 7 are below 3:1. In dark mode it is 4 and 1; the one below
  3:1 is plain blue at 2.02:1.
- **These are the best case: an opaque background.** At the default glass
  setting (`[glass] dim_light = 0.12`), most of a light-mode background is the
  glass material, not baud's colour. On the one screenshot CLAUDE.md records,
  even ordinary default text reads about 2.0:1. That figure is indicative,
  from one wallpaper.
- **Scope.** The owner runs system Dark with `theme = "auto"`, so the light
  palette only appears when macOS is light. On Linux, `auto` always resolves
  to dark.
- **After the recommended change**: nothing changes by default. If you set a
  light list, it is used while macOS is light and swapped out when it goes
  dark, repainted at once.

**Today**

- **The theme.** `Theme::light()` is text 0x27 on background 0xEF, with
  xterm's table (`baud-render/src/color.rs`, lines 184-193; the table at
  321-356). Its doc says the gap is known and that "Choosing the numbers is
  the owner's call and is not made here" (lines 182-183). It defers the
  decision; it does not refuse one.
- **Lookup and bold.** A cell's colour is the application's OSC 4 override if
  there is one, else the theme's table (lines 282-289). *OSC* is a family of
  escape sequences a program sends to set or ask for terminal-wide things; OSC
  4 repaints one numbered colour. Bold turns entries 0-7 into 8-15 (line 242),
  which is xterm's default ([[source: xterm-reset]] charproc.c 633).
- **No palette in the config** (`baud-app/src/config.rs`).
  `config.toml.example` only says an application can repaint the colours with
  OSC 4.
- **OSC 4 overrides do not follow the appearance.** They live in the core
  (`baud-core/src/osc.rs`, lines 120-136). A light/dark switch goes
  `apply_style` to `Renderer::set_theme` and `tell_defaults`
  (`baud-app/src/app.rs` around 4640-4655, `baud-render/src/renderer.rs`
  763-768, `baud-app/src/host.rs` 148-151). None of them touches the
  overrides, so a light-mode fix sent with OSC 4 stays in force in dark mode.
- **A program cannot read the theme's sixteen.** The core answers `OSC 4;n;?`
  only for an entry an application overrode (osc.rs, lines 248-264).
- **What baud does tell programs.** It answers `OSC 11;?` with the theme's
  background, which is the query theme-aware programs use to tell light from
  dark.
- **A theme is built in three places.** In `App::new` for a pinned
  `theme = "light"` or `"dark"` (app.rs, lines 577-579); in `theme_for` for
  `auto` (app.rs, 1742-1752); and in `baud-app/src/linux.rs` (lines 116-118).

**What others do**

Nobody read here ships a light ANSI set by default. They give the user a
per-palette config, and two offer a contrast floor that is off by default.

- **xterm** uses the same named palette ([[source: xterm-reset]] charproc.c
  650-663). Its default text colours are the X toolkit defaults (lines 541,
  543), which resolve to black on white; that last step is UNPINNED. So
  xterm's own default look is this palette on white. baud's light theme is
  xterm's default with 0xEF where xterm has 0xFF.
- **kitty** ([[source: kitty-conf]]) has one default palette, on black; the
  user sets color0-15. Its contrast floor, *text_fg_override_threshold*,
  defaults to 0 (off), and its docs warn it breaks block-character graphics.
  The doc's navigation names automatic light/dark theme switching; the theme
  files themselves are UNPINNED.
- **ghostty** ([[source: ghostty-config]]):
  - one default palette;
  - `theme = light:X,dark:Y` switches between named built-in themes with the
    desktop (around lines 590-598);
  - *minimum-contrast* defaults to 1 (off) and exempts emoji and images
    (around 775-786);
  - *bold-color* defaults to null (around 3828-3845). That null means "bold is
    not bright" is an inference; the doc does not say;
  - its *palette-generate* doc says legacy programs hardcode xterm's 256
    colours (around 808-827). So entries 16-255 must stay xterm's.
- **foot** ([[source: foot-config]]) has `[colors-dark]` and `[colors-light]`
  sections, but its default light theme is a copy of the dark one. It ships no
  light palette.
- **iTerm2** (UNPINNED): per-profile colours for light and dark, and a Minimum
  Contrast setting that pushes text toward black or white. Its API docs say
  that setting is not applied when separate light and dark colours are on.
- **Terminal.app** (UNPINNED, from Wikipedia's table): darker yellow, cyan and
  green, still 2.6-2.8:1 on 0xEF. Whether its profile switches with the
  appearance is UNVERIFIED.
- **VTE / GNOME Terminal**: not read.
- **Programs** choose colour numbers, not RGB values. Those that care about
  light and dark ask OSC 11 and switch their *own* colours; they do not
  repaint the sixteen for you.

**The arithmetic behind every option**

- The light theme's two ends, 0x27 and 0xEF, are 12.99:1 apart.
- For any entry between them, its contrast as text on 0xEF, times the contrast
  of 0x27 text on it as a background, is exactly 12.99.
- So no entry can reach 4.5:1 in both roles. The most it can guarantee both
  ways is about **3.60:1**; dark's figure is 3.65.

Any light set therefore picks, per entry, whether it serves as text, as a
background, or balances the two. Two example rules follow. Both are derived
arithmetic, not palettes anyone has looked at on screen:

- **Text-first.** Darken each entry, keeping its hue, until it reaches 4.5:1
  as text on 0xEF:
  - green 00CD00 becomes 007F00, yellow CDCD00 becomes 707000, cyan 00CDCD
    becomes 007979;
  - magenta becomes C100C1, bright red DC0000, bright blue 5757F2.

  Its costs: green, yellow and cyan *backgrounds* (`SGR 42/43/46`, the codes
  that set them) fall from 6.92, 8.77 and 7.55:1 to about 2.85:1 under default
  text. *Under this rule* the brights also collapse onto the plain entries. On
  the one glass screenshot these entries read about 1.40:1, where xterm's read
  3.4-4.3 (indicative).
- **Balanced** (the reviewer's addition). Put yellow, green and cyan near
  3.6:1 both ways. That clears 3:1 as text and as a background, and the
  brights can stay distinct. The cost: no text entry among the three reaches
  4.5:1.

**Options**

- **A. Keep xterm's sixteen and leave it to OSC 4.** This is today. It costs
  nothing. But OSC 4 is a program's lever, not yours, and an override sent
  from a shell rc follows you into dark mode.
- **B. Per-appearance sixteen in the config.** The defaults stay xterm's, and
  a light set is shipped only as a commented example. Effort M. Out of the box
  nothing changes, and OSC 4 stays blind to the appearance. It forecloses
  nothing.
- **B'. Named presets on top of B.** For example `palette = "..."`, like
  ghostty's built-in themes: friendlier than pasting 16 hex values, but more
  to maintain. It can be added after B.
- **C. Ship a derived light set as `Theme::light()`'s default.** Effort S, and
  it is fixed out of the box. But it bakes in a palette opinion, and the bound
  above means some role loses. It is tuned for an opaque 0xEF while the
  default glass is 0.12, so it can make the default glass case worse.
- **D. An opt-in contrast floor at draw time.** Effort M-L, across three
  render paths and the pixel oracle (the test comparing two GPU paths pixel
  for pixel). Under glass, the colour baud resolves is not the colour you see,
  so the ratio is computed against the wrong background. It also breaks block
  graphics and text-drawn pictures, so it would need baud's picture finder as
  an exemption. Both pinned terminals ship it off.
- **E. Fix the glass first.** Decide `dim_light`. At the default the dim is
  the bigger light-mode problem, and a palette chosen before it can be undone
  by it.

**Recommendation**

**Yes: add a `[palette]` section with `light` and `dark` lists of sixteen
colours, each defaulting to xterm's. Apply it through one function at all
three places baud builds a theme, and switch it with the appearance.
`Theme::light()` does not change, so light-mode `ls` stays as hard to read out
of the box as it is today.**

Two owner calls ride with it:

1. **Decide the light glass dim, `dim_light`, before or alongside this.** At
   the default it decides more of what you see than any palette does.
2. **Fork: which rule builds the commented example.**
   - (i) Balanced: about 3.6:1 both ways, brights kept distinct.
   - (ii) Text-first: at least 4.5:1 as text, backgrounds about 2.85:1.
   - (iii) Both, labelled.

   **Lean: (i) balanced.** It is the only rule that keeps every role above 3:1
   and does not merge the brights. Its numbers still need your eye on screen.

**Design**

- **Crate.** baud-app only. No change to baud-core, baud-domain, baud-render's
  code or the shader, and no `.metallib` rebuild: the cell path resolves
  colours on the CPU into its style table.
- **The config reader** (`baud-app/src/config.rs`).
  - `[palette]` takes `light` and `dark`, each a bracketed list of exactly
    sixteen `"#RRGGBB"` strings, read by the same list reader `fonts` uses.
  - The list parses into a new type, `AnsiSet([Argb; 16])`. The hex parser
    returns `Chan` or `Argb`, never bare `u8`s or an `(u8, u8, u8)`. baud-app
    sits at its audit floor of 1 raw site and 4 signature primitives, and the
    audit counts signatures as well as fields.
  - `Config` gains two optional sets, where `None` means xterm's.
  - A wrong count or a malformed entry goes on the reader's complaint list and
    falls back to xterm's. The same keys at the top level, or in another
    section, are not taken.
- **Applying it.**
  - One function in `baud-app/src/host.rs`, for example
    `with_sixteen(theme, set)`, writes entries 0-15 only, so 16-255 stay
    xterm's, per ghostty's note.
  - All three theme sites route through one "theme for this appearance and
    config" function.
  - **The macOS pinned-theme path in `App::new` is the one the first design
    missed.** `apply_style` returns early for a pinned choice (app.rs, around
    line 4640), so without this a user who pins `theme = "light"` never gets
    their list.
- **A live switch needs nothing new.** It already runs `set_theme`, which
  forces a full repaint, and then `tell_defaults`.
- **The example.** A commented `[palette]` block goes in `config.toml.example`
  **after** `fonts`, because a section header swallows every top-level key
  after it. Its rule and its cost are written beside it.
- **Behaviour to document.**
  - OSC 4 still wins over the config.
  - OSC 104 returns an entry to its configured colour.
  - `OSC 4;?` is still answered only for overrides.
  - On Linux the light list applies only with `theme = "light"` until an
    appearance signal is wired, and a pinned light theme there is drawn at
    `dim_dark`, not `dim_light` (linux.rs, line 124).

**Tests and measurement**

- **Red first:**
  - config.rs: *a_palette_section_reads_sixteen_colours_per_appearance*, which
    fails today because `[palette]` is an unknown section;
  - config.rs: *a_palette_of_fifteen_or_seventeen_is_refused_and_said*,
    *a_malformed_palette_entry_is_refused_and_said* and
    *a_top_level_light_list_does_not_reach_the_palette*;
  - host.rs: *the_light_set_is_used_under_light_and_the_dark_under_dark*,
    including the pinned-theme case through the shared function, and
    *entries_past_fifteen_stay_xterms*;
  - baud-render: *a_themes_sixteen_follow_set_theme*. An incremental frame
    after `set_theme` with a different entry 3 repaints an `SGR 33` (yellow
    text) cell. It guards the full-repaint assumption, because a palette swap
    moves no cell.
- **The example gate.** A test uncomments the example block, parses it, and
  asserts no complaints and that every entry meets the chosen rule's
  threshold: at least 3.0:1 both ways for balanced, or at least 4.5:1 as text
  for text-first. The threshold follows the fork.
  `the_committed_example_parses_to_what_it_documents` keeps the rest of the
  example honest.
- **Call sites.** The three sites run inside AppKit, so they are held by
  reading, or by a source gate in the shape of
  `every_reader_of_the_window_asks_the_clamp`.
- **Perturbations, each red at its own assertion.** A perturbation
  deliberately breaks one mechanism to prove a test goes red. Here:
  - the dark set applied under light;
  - the shared function skipped at each site, pinned macOS included;
  - fifteen entries accepted;
  - the keys leaking out of their section;
  - the set written into entries 16-31.
- **Measurement owed:**
  - ingest: none, by the repo's own rule, because baud-core does not change
    and `ingest2` does not link baud-app. Say so rather than sweep;
  - frame: nothing new per cell, since the palette read already exists; the
    claim is by construction;
  - startup: 32 hex strings parsed. At most one arm of
    `scripts/startup-bench.py` with an A/A copy (a byte-identical second
    binary, to measure noise) if anyone doubts it.

**Effort and risk**

- Effort: M, about a day: the reader, three call sites, the example, and about
  nine tests.
- Risk: low. The defaults do not change. The one trap, the block placed before
  `fonts`, is caught by the example gates.

**What would change it / depends on**

- **Changes it:**
  - if you want legibility out of the box, take C with its stated costs,
    preferably built by the balanced rule;
  - if `dim_light` is raised toward opaque, the 0xEF numbers become the real
    ones and C gets more attractive;
  - evidence that Terminal.app or iTerm2 ship a distinct light set by default
    (UNVERIFIED) would weaken "nobody ships one".
- **Depends on:**
  - the `dim_light` decision;
  - a Linux appearance signal, so that `auto` can reach the light list there;
  - turning bold-is-bright off is not the fix: executables only go from 1.19
    to 1.88:1 and symlinks from 1.09 to 1.72:1;
  - it is independent of V17.
- **Two separate items to record, not folded in:**
  - **Mode 2031 / DSR 996**, which tells a program the colour scheme changed.
    ghostty implements it (UNPINNED); baud has no mention of 2031. A program
    that read OSC 11 at startup is never told of a mid-session switch, which
    matters more once the palette follows the appearance.
  - **Resets.** xterm's soft and full resets (DECSTR and RIS) reset every OSC
    4 entry ([[source: xterm-reset]] charproc.c around 14402-14406). baud's
    keep every colour override, and no coverage row says so.

### V17. What "the background" means while the screen is reversed

**The question**

A program can flip the whole screen into reverse video with DECSCNM,
`ESC [ ? 5 h`, often written `?5`. The default text colour and the default
background trade places; it is how a "visual bell" flashes. While flipped, the
same program can set, ask for or reset three things:

- OSC 10, the default text colour;
- OSC 11, the default background;
- OSC 4, a numbered palette entry.

Does "the background" then mean the colour currently shown as the background,
or the one that would be the background if the screen were not flipped? baud
answers the second; xterm answers the first. Which should baud do?

**What you would notice**

- **Today a flipped-screen question gets the wrong role.** A program asking
  `OSC 11;?` to tell light from dark is answered with the colour baud is
  drawing as *text*, so it says "dark" about a screen that shows white.
- **Setting the background while flipped moves the text colour instead.**
- **baud is half-xterm, and the halves disagree.** The renderer swaps palette
  entries 0 and 7, and 8 and 15, under `?5`, as xterm does. OSC 4 writes the
  unreversed entry, as foot does. So `OSC 4;0;red` sent while flipped
  recolours the cells that asked for colour 7, while a query of entry 0
  answers red. No terminal read here does that.
- **After the recommended change**, set, query and reset act on the colour you
  see, as in xterm.
- **Scope.** No recording in the corpus sets `?5h` (0 of 10). The only one
  that sends colour OSCs, cava, sends 50 OSC 4s on entries 16 and up, never
  under `?5`. How common programs that hold reverse video are is not measured.

**Today**

- **Set and reset.** OSC 4 writes the stored palette override, and OSC 10/11
  write the stored default colours, whatever the mode (`baud-core/src/osc.rs`,
  `dispatch_osc`, lines 120-143).
- **Queries** read the same unreversed slot: `report_color` (lines 253-264)
  and `report_dynamic` (271-288). The latter falls back to the embedder's
  defaults, given through `Term::set_default_colors` (`baud-core/src/term.rs`,
  2146-2155).
- **The mode** is `Term::reverse_video` (term.rs, around 1682), flipped by
  `flip_reverse_video` (`baud-core/src/ops.rs`, line 938).
- **The flip is applied only when drawing**: `Video::entry` and
  `Theme::defaults` in `baud-render/src/color.rs` (around lines 111-146 and
  269-280), and `video_of` in `baud-render/src/renderer.rs` (line 279).
- **What the tests pin.** Only overrides set *before* the flip:
  `a_palette_override_travels_with_its_entry` and
  `the_dynamic_defaults_swap_too` in `baud-render/tests/reverse_video.rs`.
- **What the docs say.** `docs/vt-coverage.md` lists this as "Not built" in
  the `?5` row, pending xterm's `misc.c` being pinned.

**What others do**

- **xterm** maps to the colour as displayed. `misc.c` at 9489b205 was read,
  and is pinned as [[source: xterm-misc]].
  - The flip swaps *stored* colours: DECSCNM calls its ReverseVideo routine
    ([[source: xterm-reset]] charproc.c 7590-7595). That routine exchanges
    palette 0 and 7, 8 and 15, text foreground and background, and mouse
    foreground and background, as whole records with their defaults
    ([[source: xterm-scroll]] util.c 3211-3345).
  - In misc.c, OSC 10/11 set, query and reset then land on the displayed
    colour, and a reply keeps the code the program asked for (around lines
    3527-3750). OSC 4 does no mapping, which after the swap is also the
    displayed entry.
  - After `?5l`, whatever was set carries into the other role.
  - xterm keys this on the XOR of its `-rv` startup flag and DECSCNM. baud has
    no `-rv`, so "under `?5`" is the whole condition here.
- **foot** uses the unreversed slot. Its renderer swaps only the default text
  and background colours, not palette entries ([[source: foot-render-1-21]]
  render.c 721-739), and its osc.c uses the stored slots (UNPINNED).
- **wezterm** at b09b56c uses the unreversed slot: reverse video is only a
  flag (UNPINNED).
- **ghostty** also uses the unreversed slot. It has a reverse-colours mode
  ([[source: ghostty-terminal-9bbb9b2]]), and the reviewer found that its
  colour-operation handler in stream_handler.zig at 9bbb9b2 never reads it
  (UNPINNED). This was UNVERIFIED in the research and is now settled.
- **kitty**: DECSCNM is a render flag ([[source: kitty-screen-0-49]] screen.c
  2004-2010). What its Python colour callbacks do is UNVERIFIED.
- **alacritty** has no DECSCNM at all ([[source: alacritty-term-term-mod]]),
  so it cannot serve as an oracle (a second implementation to check baud
  against) for this.
- **Terminal.app, iTerm2, VTE**: not read.
- **Tally**: xterm alone maps to the displayed colour; foot, wezterm and
  ghostty use the unreversed slot. baud draws like xterm and handles OSC like
  foot, a mix none of them has.

**Options**

- **A. xterm's model, mapped at the OSC boundary in the core.** While `?5` is
  set:
  - OSC 10 and 11 trade slots, and OSC 4 trades entries 0/7 and 8/15, for set,
    query and reset;
  - a reply keeps the code or index the program asked for.

  Cost S. It completes the model baud already draws with, and needs no new
  state. It is equivalent to xterm for set, query, reset and flips, but **not
  across a reset**: see below. It diverges from the majority, and xterm's
  source is the only oracle.
- **B. The majority's model (foot, wezterm, ghostty).** Keep the unreversed
  slots and stop swapping palette entries 0/7. It is simple and matches three
  of four. It reverts committed, xterm-faithful drawing: the `?5` coverage
  row, `baud-render/tests/reverse_video.rs` and the pixel oracle's `?5` scene.
  A flipped-screen query still answers the text colour.
- **C. Keep today's mix and document it.** No code, but it stays internally
  inconsistent and matches no terminal.
- **D. Map only the query.** This fixes light/dark detection, but set 11 to X
  and a query of 11 then returns something other than X. No implementation
  does that.

**Recommendation**

**Yes: pin xterm's `misc.c`, then make OSC 4/10/11 set, query and reset act on
the colour shown while `?5` is set, as xterm does. Put the mapping in
baud-domain, where the core and the renderer share it, and have every reply
keep the code or index the program asked for.**

B is the majority now that ghostty is read. It still loses here, because baud
already draws like xterm and A is the only consistent completion of that.

**Design**

1. **Pin.** Add a manifest entry named xterm-misc beside xterm-scroll and
   xterm-reset: misc.c at 9489b205, sha256
   1ea4c69b2e1b0824956c4991eaa62da1ff93804b735dc0f9cba65b03a8b406ce, 204,973
   bytes. `scripts/check-sources.sh` gates it. foot's osc.c can be pinned too.
2. **baud-domain.** Move `Video` (Normal or Reverse, with `entry`) out of
   `baud-render/src/color.rs` into baud-domain, beside `DynamicColor` in
   `baud-domain/src/vt/osc.rs`. Add `dynamic(DynamicColor) -> DynamicColor`,
   which swaps Foreground and Background under Reverse and leaves Cursor
   alone. baud-render re-exports it (`baud-render/src/lib.rs`), so no caller
   changes. That makes one spelling of the mapping, with no `bool`, no raw
   primitive, and no change to the audit.
3. **baud-core, `dispatch_osc`.** Read the video once per string from the
   reverse-video mode bit, then:
   - **OSC 4:** the slot is `v.entry(index)`. A set or reset writes that slot.
     A query reads it and replies with the index *asked*.
   - **OSC 10/11:** the slot is `v.dynamic(which)`. A set writes the stored
     default colour at that slot. A query reads the stored colour, else the
     embedder's default, at the slot, and replies with the code *asked*. A
     reset clears the slot.
   - `ResetColor(None)`, a reset of all, is unchanged.
   - The new reporter signatures take the `Chan` and `DynamicColor` types,
     which keeps baud-core at 2 raw sites and 0 signature primitives.
   - No new field and no storage change. It all sits behind `str_end`'s cold,
     never-inlined boundary (osc.rs, lines 73-75).
   - **The trap:** the reply must carry the code asked, never the mapped one.
     Otherwise an `OSC 11;?` gets an `OSC 10` reply, which is the wrong-shaped
     answer `docs/vt-coverage.md` forbids.
4. **Docs.** The "Not built" clause in the `?5` row becomes done, the
   CLAUDE.md reverse-video section is updated, and `Video`'s doc cites misc.c.
   The claim is stated precisely: equivalent to xterm for set, query, reset
   and flips, **not across DECSTR/RIS**.
5. **Flash.** A `?5h`/`?5l` pair in one read is a renderer-only flash; the
   core's mode is unchanged after it. The core maps by the mode each byte saw,
   in order, which is what xterm's swap gives for the same bytes.

**Tests and measurement**

- **Red first** on today's tree, in `baud-core/tests/xterm/osc.rs`, with
  defaults of text e0 and background 11:
  - *under_reverse_video_osc_11_sets_the_background_drawn*: `?5h` then
    `OSC 11;rgb:aa/bb/cc` stores the value in the Foreground slot and leaves
    Background empty;
  - *under_reverse_video_osc_11_answers_the_background_drawn_in_the_code_asked*:
    `?5h` then `OSC 11;?` replies exactly `\e]11;rgb:e0e0/e0e0/e0e0\a`, and
    `OSC 10;?` replies `\e]10;rgb:1111/1111/1111\a`;
  - *under_reverse_video_osc_111_resets_the_background_drawn*;
  - *under_reverse_video_osc_4_on_entry_0_lands_on_entry_7*: entry 7 is set
    and entry 0 is empty; a query of 0 answers in index 0 with entry 7's
    value; entry 7 still holds it after `?5l`.
- **Controls:**
  - entries 1-6 and 9-255 and OSC 12 are unmapped;
  - without `?5` nothing is mapped, so
    `osc_11_query_answers_the_embedders_default` and
    `an_override_wins_the_query_and_reset_restores_the_default` stay green.
- **Read sizes and a second mechanism:**
  - chunk invariance (the same result wherever the input is split into reads)
    for `?5h OSC 11;X OSC 11;? ?5l` at read sizes 1, 3 and whole;
  - a property run at `PROPTEST_CASES=20000` over interleaved flips and colour
    OSCs, checked against a hand model that swaps stored values on each flip,
    as xterm does. **The model must leave resets out, or model baud's reset
    rather than xterm's**, or it fails for a reason unrelated to this change.
- **Other levels:**
  - baud-domain: `entry` touches only 0, 7, 8 and 15 and undoes itself;
    `dynamic` swaps text and background and keeps the cursor;
  - baud-render: an OSC 11 sent while reversed moves the background drawn, and
    after `?5l` it is the text drawn.
- **Perturbations, each red at its own test:**
  - drop the mapping on set, on query, on reset, or on OSC 4;
  - reply with the mapped code;
  - map whatever the mode, which turns the no-`?5` tests red;
  - map the cursor too.
- **Measurement owed** (this changes baud-core):
  - `cells_written` identical on all ten recordings and `mix`;
  - **either** the ingest loop's disassembly shown unchanged outside the OSC
    functions (cheaper and stronger now that every function starts on a
    64-byte boundary, commit 0987b98), **or** `ingest2` on cava, btop and
    asciiquarium, with both arms built at one path, at least five rounds
    interleaved, the first dropped, and an A/A copy. Say which proof was
    taken;
  - run `cargo test --workspace` for the sfte and alacritty oracles, and state
    that neither can check this: alacritty has no DECSCNM, and sfte was not
    checked for it.

**Effort and risk**

- Effort: S, about half a day including the pin.
- Risk: low. Behaviour changes only while `?5` is set, which no recording
  does, and storage is unchanged. The one real trap, the reply code, has its
  own test and perturbation.

**What would change it / depends on**

- **Changes it:**
  - you prefer the majority's model: take B, including reverting the palette
    swap so drawing and storage agree;
  - a real program that sets colours under a held `?5` and expects the
    unreversed slot;
  - reading iTerm2 or Terminal.app shows the field left xterm;
  - pinning `misc.c` yields a different file: re-check the sha256.
- **Depends on** pinning `misc.c`, and nothing else. It is independent of V16.
- **Left open:** xterm's cursor-colour rule under `?5` (util.c around 3293),
  and the reset gap. xterm's DECSTR and RIS reset every OSC 4 entry; baud's
  keep every colour override. That gap exists today and is its own item.
  *(Since closed: both resets clear every OSC 4 override as of 38f0247,
  2026-09-29; and the cursor-colour rule is built, 2026-10-11, the record's
  "A cursor colour equal to the text keeps its colour across `?5`".)*

**Built, 2026-10-11, for `OSC 10` and `11`** (the record, "`OSC 10` and
`11` under `?5` act on the colour shown"): option A, as recommended, in the
half that is the same under every reading of DEC's `DECSCNM`. `Video` moved
to baud-domain with `entry` and `role` (the design's `dynamic`), baud-render
re-exports it and its `Theme::defaults` reads through `role`; the core maps
every set, query and reset of `OSC 10`, `11`, `110` and `111` by the `?5`
bit, the reply keeping the code asked, and `CSI ? 996 n` follows (note Y
§ 7). The red tests went into `baud-core/tests/reverse_osc.rs` rather than
`tests/xterm/osc.rs`, beside a property against a model that swaps stored
values on each flip, as xterm does. **`OSC 4` and `104 ; c` are held**: DEC
STD 070 defines `DECSCNM` as reversing "the definitions of the foreground and
background colors" (page 5-94, PDF 349) and describes it as a per-cell
exchange of the attribute bits (PDF 362), and no DEC passage names entries
0/7 and 8/15, so the owner's "as recommended", given without the DEC text,
does not decide xterm's palette swap. The `OSC 4` mapping exists only under
that swap and waits on Jev (the question is in the record).

## Part 6. Images

### V18. Sixel modes ?80, ?1070 and ?8452: what baud answers when asked

**The question**

Sixel is DEC's old bitmap format: a picture sent inside an escape sequence.
Before sending one, a program can flip three switches, called private modes:

- **?80, DECSDM** ("sixel display mode"): draw the picture at the cursor and
  scroll the screen to make room, or pin it to the top-left corner and leave
  the cursor alone.
- **?1070**: give each picture a fresh set of colour registers, or reuse the
  previous picture's. A register is one numbered slot in the palette a Sixel
  paints with.
- **?8452**: after the picture, leave the cursor to its right, or below it.

baud already behaves in one fixed state of each switch. The decision is what
it should *say* when a program asks, through DECRQM (a request that the
terminal report a mode's state), and what it should do when a program asks for
the other state.

**What you would notice**

- **Today:** nothing wrong on screen from these switches. But DECRQM answers
  "not recognised" for modes whose state baud has. And chafa's harmless
  requests are logged as unsupported, so that counter cannot tell a harmless
  request from a real refusal.
- **After:** still nothing different on screen. A program that asks gets a
  true answer ("this is fixed"), and only requests for a state baud cannot
  give are counted.
- **Not this decision, but more visible:** baud leaves the cursor one row
  lower after a picture than DEC's manual says. That probably puts an extra
  blank line under every chafa image. See "Also for your list" below.

**Today**

- `baud-domain/src/vt/mode.rs` (lines 287-359): `PrivateMode` has no variant
  for 80, 1070 or 8452. All three become `PrivateMode::Other`.
- `baud-core/src/ops.rs`: the wildcard arm of `set_private_mode` (lines
  1104-1109) bumps `unsupported_param`, the generic "unknown request" counter,
  and `mode_state` answers 0, "not recognised" (lines 1567-1568).
- What baud actually does equals ?80 reset, ?1070 set and ?8452 reset:
  - The picture is placed at the cursor (`image_anchor` in
    `baud-core/src/sixel.rs`), and the cursor then moves down past it,
    scrolling at the bottom. The column stays at the picture's left edge.
  - `baud-domain/src/sixel.rs` line 167: the 256 registers are a local inside
    `Sixel::walk`, so every picture gets a fresh palette. They start
    transparent.
- Sixel is advertised in the DA reply (the terminal's "what am I" answer) only
  when the embedder (the app hosting the core, here baud-app) turns the claim
  on and image memory is provisioned. See `baud-core/src/write.rs` around line
  1541, which matches on `SixelClaim` (defined in
  `baud-domain/src/vt/report.rs`). The claim is off by default.
- Precedents in the code:
  - `sixel_p2_ignored` in `baud-domain/src/image.rs` is a Sixel-specific
    counter for a request baud refuses. It is cumulative: `ImageStore::reset`
    does not zero it.
  - ?2027 answers 3, "permanently set". But `?2027h` and `?2027l` both still
    fall to `unsupported_param`, so it is *not* a precedent for splitting
    refused from accepted requests.
- `docs/vt-coverage.md` has no row for any of the three.
- None of the 21 recordings sends any of the three, in either direction. The
  reviewer grepped them, with `?1049` in btop as the positive control (a
  search that must hit, proving the search works).

**What others do**

- **The numbering dispute** (which of `h` and `l` means "scroll"):
  - xterm: [[source: ctlseqs]] line 967 says `?80h` enables display mode,
    which means no scrolling; lines 1113-1115 say `?80l` turns on scrolling.
    [[source: xterm-reset]] starts DECSDM reset (scrolling on) and restores it
    only on a full reset (RIS).
  - foot: [[source: foot-csi]] sets `scrolling = !enable` and answers DECRQM
    from that; [[source: foot-term-reset]] restores it on a hard reset.
  - The DEC VT382 manuals, quoted in [[source: ctlseqs]] lines 3469-3495,
    agree with xterm.
  - The DEC VT330/VT340 manual, [[source: vt3xx-sixel]], says the opposite:
    set means scrolling on. It is the only source on that side.
  - So the independent sides are one DEC manual against the later DEC manuals,
    xterm and foot. It is not "three to one": ctlseqs and xterm's `charproc.c`
    are one project.
  - xterm inverted its numbering in patch #369, 2021 (changelog, UNPINNED).
    notcurses records each terminal's switch (UNPINNED).
- **What the two states do is not disputed.** [[source: vt3xx-sixel]]
  describes both: scrolling draws at the text cursor and scrolls; display mode
  draws at the top-left, ignores anything past the bottom margin, and leaves
  the cursor.
- **?1070:** xterm and foot default to private registers ([[source: ctlseqs]]
  3224-3225, [[source: xterm-reset]], [[source: foot-csi]],
  [[source: foot-term-reset]]). What "shared" means differs: xterm recolours
  pictures already drawn, foot only keeps the definitions, and Windows
  Terminal has no ?1070 at all (all three UNPINNED).
- **?8452:** [[source: ctlseqs]] lists it as "RLogin, xterm": the cursor goes
  to the right of the picture.
- **Clients** (all UNPINNED, read at master):
  - chafa's Sixel mode sends `?80l` then `?8452l`, which is exactly the state
    baud is in.
  - notcurses sends only `?8452h` to a terminal it cannot identify, and baud
    is one.
  - timg sends `CSI 80h` with no `?`, which is ANSI mode 80, not DECSDM.
  - yazi and lsix send none of the three.
- **xterm's DECRQM answer** for DECSDM comes from its flag, gated only at
  compile time (`misc.c`, UNPINNED, commit unverified). xterm gates *acting*
  on the mode at runtime, but not the answer.

**Options**

- **A. Status quo plus coverage rows.** No code. DECRQM keeps answering
  "unknown" for states baud has, and chafa's harmless requests keep counting
  as unsupported.
- **B. Permanent answers for ?80 and ?1070** (the researcher's
  recommendation). Small. Leaves ?8452 in the unsupported counter, so chafa's
  burst still counts half as a failure.
- **B+. B extended to ?8452** (the reviewer's revision). DECRQM answers ?80
  with 4 ("permanently reset"), ?1070 with 3 ("permanently set") and ?8452
  with 4. The request for the state baud is in is accepted silently; the
  request for the other state is refused and counted. How to count is a small
  fork:
  - **(i) New Sixel counters** in `ImageStore`, cumulative like
    `sixel_p2_ignored`. This fixes chafa's false refusals, and notcurses'
    `?8452h` would give real numbers. But the ?80 counter cannot tell a client
    using the old numbering (which sends `?80h` to mean "scroll", and gets it)
    from a client that really wants display mode.
  - **(ii) No new counters**, the ?2027 shape: both directions stay in
    `unsupported_param`. Cheaper, but chafa's harmless requests still read as
    failures.
- **C. Build ?80 display mode.** Effort M, plus an ingest sweep (a timed run
  of the parser over the recorded sessions). No surveyed client sends `?80h`
  to an unidentified terminal. Under xterm's numbering, an old-numbering
  client asking to scroll would get the corner placement.
- **D. Build shared registers (?1070 reset).** Implementations disagree on
  what shared means. baud stores finished colours (ARGB: alpha, red, green,
  blue), so xterm-style recolouring is impossible without storing register
  numbers per pixel, which t-images rejected. No surveyed client sends
  `?1070l`.
- **E. Follow the VT340 manual's numbering.** Every current implementation
  abandoned it, so current clients would read baud's answer backwards.

**Recommendation**

**Yes to B+, with counters shape (i):**
- **DECRQM answers ?80 = 4, ?1070 = 3 and ?8452 = 4**, but only while Sixel is
  claimed and provisioned, the same condition as the DA reply. Otherwise it
  answers 0, as today.
- **`?80h`, `?1070l` and `?8452h` are refused and counted in new Sixel
  counters.** `?80l`, `?1070h` and `?8452l` are accepted as no-ops.
- **Build neither display mode nor shared registers.**

I lean to (i) over (ii) because (ii) leaves the problem that motivates the
change. Record plainly that the ?80 counter is ambiguous.

One honest limit: the permanent *answer* is robust to the numbering dispute
only for requests. A client using the old numbering will read "4" as
"scrolling permanently off", the opposite of what baud does. That is a cost of
following xterm's numbering, which every current client uses.

**Design**

- **`baud-domain/src/vt/mode.rs`:** three new `PrivateMode` variants (for
  example `SixelDisplayMode`, `SixelPrivateRegisters` and `SixelCursorRight`),
  with wire arms for 80, 1070 and 8452. The ?80 doc comment names xterm's
  numbering and both sides of the conflict. No `bool` fields, which the
  baud-domain audit forbids.
- **`baud-domain/src/image.rs`:** three cumulative counters beside
  `sixel_p2_ignored`, named for example *sixel_display_mode_refused*,
  *sixel_shared_registers_refused* and *sixel_cursor_right_refused*.
  `ImageStore::reset` does not zero them, matching the existing counter.
- **`baud-core/src/ops.rs`, `set_private_mode`:**
  - while Sixel is claimed, the refused direction bumps its counter and the
    accepted direction does nothing;
  - while unclaimed, every one of these requests goes to `unsupported_param`,
    so the count agrees with DECRQM's 0.
- **`mode_state`:** answers the permanent state while claimed, and
  `NotRecognized` otherwise.
- **One claim helper**, returning the existing `SixelClaim`, used by both the
  DA reply and `mode_state`. Then DA and DECRQM cannot disagree. No new
  two-state type.
- **Document it as baud's own choice:** xterm gates *acting* on the mode at
  runtime, not the answer. Gating the answer on the claim keeps DECRQM from
  advertising Sixel while DA hides it.
- **No reset handling:** a permanent mode has no state to restore on RIS (full
  reset) or DECSTR (soft reset).
- **No byte-graph change:** modes are parsed from parameters in the cold
  `csi_final` path (the rarely-taken control-sequence handler, outside the
  per-byte loop). So the stride ladder
  (`the_alphabet_reduces_as_the_stride_doubles`, a test pinning how many byte
  classes the parser tells apart) does not move. `baud-app` does not change.
- **Documents:**
  - `docs/vt-coverage.md` gains three rows. Each states the permanence rather
    than a bare "done". For ?80: permanently reset, Sixel scrolling only;
    display mode refused and counted; numbering follows xterm, with the VT340
    manual named as the conflict.
  - A source-conflict paragraph cites [[source: vt3xx-sixel]] against
    [[source: ctlseqs]], [[source: xterm-reset]] and [[source: foot-csi]]. It
    is recorded the same way as the DECAWM conflict (DECAWM is auto-wrap at
    the right margin, and DEC and xterm disagree on its default).
  - The two open items in `docs/research/notes/s-sixel.md` (lines 580-589) are
    struck as taken. Note S itself only said "pick one"; it is
    `docs/research/notes/t-images.md` (lines 280-281) that says "follow
    xterm".
  - `baud-core/src/sixel.rs` lines 18-20 credit mode 8452 to
    [[source: vt3xx-sixel]], and that chapter has no 8452. Re-cite it to
    [[source: ctlseqs]].

**Tests and measurement**

All in `baud-core/tests/sixel.rs`, using that file's own `run` helper at chunk
sizes 1, 3, 7 and whole, so a request split across reads cannot change a
count. The `each_read_size` helper lives in two other test files. Each test is
red first at an assertion:

1. **The permanent answers.** Claimed and provisioned: `CSI ? 80 $ p` answers
   `CSI ? 80 ; 4 $ y`, ?1070 answers 3 and ?8452 answers 4. Two control arms
   answer 0: unclaimed, and claimed but unprovisioned. Red today at the
   claimed arm.
2. **A display-mode request is refused, counted, and the picture still
   scrolls.** With a cell size, send `?80h`, move the cursor near the bottom
   and send `swirl.raw`. The anchor is the cursor, not the corner, the
   window's top line moved, and the counter reads 1. Then `?80l` and a second
   picture: the counter stays 1.
3. **A shared-register request is refused, and registers stay private.**
   Picture A defines register 5; then `?1070l`; picture B paints with 5
   undefined. B's pixel equals a control picture that uses an undefined
   register. The comparison is relative, so a later default-palette change
   cannot break it.
4. **A cursor-right request is refused, counted, and the cursor still lands
   below.**
5. **While unclaimed**, all three requests bump `unsupported_param` and no
   Sixel counter.

Perturbations (deliberately breaking one mechanism to prove a test notices),
each turning a test red:
- answer plain set or reset → 1;
- drop the claim gate → 1's controls;
- count the accepted direction → 2;
- anchor at the corner under `?80h` → 2;
- make the registers persistent → 3;
- move the cursor right under `?8452h` → 4;
- send unclaimed requests to the Sixel counters → 5.

The existing `spec_inventory` probe uses parameter 1 only, so it neither
guards nor threatens this change. `coverage.rs` and the docs gate must stay
green.

**Measurement.** No recording sends these modes, so a timed sweep can only see
code layout (where functions happen to land in the binary).
- Build base and change at one path, and compare the ingest loop's disassembly
  and hot-function offsets.
- If they are identical, that is the result.
- If the loop moved, run the three-arm `ingest2` sweep
  (`baud-bench/benches/ingest2.rs`): base, change and an A/A copy (a
  byte-identical copy of the change, timed alongside to show the noise). Seven
  rounds, order rotated, first dropped, on mains power, with `cells_written`
  identical so every arm does the same work.

**Effort and risk**

- **Effort:** S, about half a day.
- **Risk:** low. Nothing drawn changes. The wrong-answer risk is an
  old-numbering client reading "4" backwards. The worst request case is a
  new-numbering client that wanted the corner placement getting scrolling, and
  that is counted.

**What would change it / depends on**

- **Take C or D** if a program you run needs the corner placement, or reuses
  registers across pictures. The ?80 counter alone cannot prove the first,
  because old-numbering clients also send `?80h`.
- **Depends on nothing.** It is independent of V19.

**Also for your list** (raised by the review; separate items, none blocking)

- **Cursor row after a picture.** [[source: vt3xx-sixel]] says "When sixel
  mode is exited, the text cursor is set to the current sixel cursor
  position", which is the picture's last band (a band is a six-pixel-tall
  strip). baud puts the cursor one row below. xterm uses the last row
  (UNPINNED). chafa treats row-below as an "overshoot" quirk, and for
  terminals it does not know it writes extra vertical space, so baud probably
  gets **an extra blank line under every chafa image**. That is UNVERIFIED
  live; check it before deciding. It is more visible than V18.
- **Where painting starts.** baud's registers start transparent, and painting
  starts at register 0. xterm starts at register 3, and xterm, foot and
  Windows Terminal fill 0-15 with the VT340 default colours (all UNPINNED). So
  a Sixel that paints before choosing a colour draws nothing in baud. The fix
  needs the VT340 manual's colour chapter pinned first.
- **A silent fold.** `baud-domain/src/sixel.rs` line 186 folds every register
  number above 255 onto 255: a wrong colour, drawn with no count. DEC's range
  is 0-255, so refusing and counting is consistent with the spec. xterm has
  1024 registers (UNPINNED). It also blocks a truthful XTSMGRAPHICS answer
  (the query for how many registers a terminal has).

### V19. A PNG and zlib decoder for Kitty graphics: does it exist, where does it go, and build it now?

**The question**

Kitty's image protocol is used by `kitten icat`, and by timg and chafa in
Kitty mode. It sends a picture inside an APC, an escape-sequence envelope
written `ESC _ G ... ESC \`. The picture arrives as raw pixels, as a PNG file,
or zlib-compressed. zlib is the "deflate" compression that PNG itself uses.

baud's core allocates no heap memory, forbids `unsafe` Rust, and takes no
outside engine libraries. The questions:
- Can a PNG and zlib decoder live under those rules?
- Where would it live?
- Should the images work build it now?

You parked Kitty graphics on 2026-09-23.

**What you would notice**

- **Today:** Kitty-protocol pictures draw nothing. baud reads and counts the
  commands. yazi and notcurses-based tools already work through Sixel or text
  art.
- **After this decision:** nothing on screen changes. The documents stop
  saying the decoder is impossible, and the first step for the day you unpark
  is on record.

**Today**

- **A suitable decoder already exists:** `baud-font`'s `png.rs`, at
  `baud-domain/src/png.rs` since step 2 (2026-10-15).
  - It is `no_std` (builds without Rust's standard library),
    `forbid(unsafe_code)`, and works over buffers the caller provides.
  - It handles every colour type and bit depth, the five filters, every
    deflate block kind, and both checksums (CRC-32 on each PNG chunk, Adler-32
    on the zlib stream).
  - ~~It refuses Adam7 interlacing, a PNG storage order that sends the
    picture in seven passes.~~ It reads Adam7 since 2026-10-15 (the record,
    "png.rs reads Adam7").
  - Its inflate (decompression) must fill its output buffer exactly (line
    336). That caps a "decompression bomb", a small input that expands to a
    huge output.
  - It decodes Huffman codes (the variable-length codes deflate compresses
    with) one bit at a time (`Code::decode`, around line 733), a choice made
    because emoji strikes are a few kilobytes.
  - It needs its whole input in one contiguous piece; it cannot resume across
    chunks.
- **Its gates:**
  - `baud-font/tests/bitmap.rs` decodes more than 30,000 Apple Color Emoji
    PNGs with both baud and the `png` crate and requires equal pixels. ~~It
    also requires `interlaced_refused` to be 0.~~ Since 2026-10-15 it holds
    60 interlaced fixtures and the interlaced system PNGs to the same
    oracle instead.
  - `baud-font/tests/hostile.rs` feeds it hostile bytes.
  - Which colour types the emoji corpus actually reaches is unmeasured.
- **Its only consumer** is `baud-render/src/atlas.rs`.
- **The core cannot reach it.** `baud-core/Cargo.toml` depends on baud-domain,
  plus the ISA crates on aarch64 through the default `neon-scan` feature, and
  not on baud-font.
- **Kitty today:** `APC G` is consumed and counted (`docs/vt-coverage.md` line
  712). There is no base64 decoder (base64 turns 4 text characters back into 3
  bytes). It is parked (`docs/vt-coverage.md`'s Kitty graphics row, `out`).
- **Image pixels have no alpha (partial transparency), and that is the real
  gap:**
  - `ColorWord` (`baud-domain/src/style.rs`) holds a 24-bit colour or
    "nothing", and the image arena is made of them
    (`baud-domain/src/image.rs`).
  - `blit_image` in `baud-render/src/composite.rs` draws a pixel fully or
    skips it.
  - CLAUDE.md's CPU-path rule depends on that: every image pixel is opaque or
    clear.
  - Kitty's default format is RGBA, and PNGs carry partial transparency.
- **Budget:** `baud-app/src/host.rs` gives images 4 MiB by default
  (`[images] pixels_mb` overrides it). One 1887x1088 window is 2,053,056
  pixels, so the default cannot hold one full-window picture.
- **Six places say the decoder cannot exist, or point to the wrong place:**
  - `docs/vt-coverage.md` line 712;
  - `docs/research/notes/s-sixel.md` lines 10-19, which says the decoder's
    *shape* is unsettled rather than that none exists;
  - `docs/research/notes/t-images.md`'s seam rows (lines 54-55) and "only PNG
    and zlib cannot" (around line 66);
  - t-images' "the core cannot even size the destination" (around line 73);
  - the comment in `docs/research/sources/MANIFEST.toml` around line 6616;
  - png.rs's own module doc, which says the interlace refusals are counted in
    `baud-font/tests/oracle.rs` when the count lives in
    `baud-font/tests/bitmap.rs` (both gone since 2026-10-15: png.rs reads
    Adam7).

  The work list's entry for these was already correct and needed no change.

**What others do**

- **The protocol** ([[source: kitty-graphics]]):
  - A terminal must understand 24-bit RGB, 32-bit RGBA and PNG, and `f=32`
    RGBA is the default format.
  - Compression (`o=z`) may apply to any format, and PNG with compression must
    carry its size `S`.
  - Payloads are base64, in chunks of at most 4096 bytes, each a multiple of 4
    except the last.
  - The cursor moves past a placed picture unless `C=1`.
  - Support is detected by whether a query (`a=q`) is answered before the DA
    reply (the terminal's "what am I" answer), and query replies must be
    immediate.
  - Transmission mediums: `t=d` direct (the default), `t=f` a file, `t=t` a
    temporary file the terminal deletes, and `t=s` shared memory.
  - kitty's quota is 320 MB, and it deletes older images when over. baud would
    refuse and count instead, a divergence to record.
- **The formats:** [[source: rfc1950-zlib]], [[source: rfc1951-deflate]] and
  [[source: w3c-png-2003]], which png.rs already cites.
- **Every output size is known before decoding:**
  - raw pixels with compression inflate to width x height x 3 or 4;
  - PNG with compression inflates to `S`;
  - a PNG's own data inflates to a size read from its header (IHDR), which is
    parsed before any image data.

  So exact-fill works, and bounds a bomb, provided each declared size is
  checked against the budget *before* memory is reserved.
- **Third-party inflaters (UNPINNED):**
  - miniz_oxide: `forbid(unsafe_code)`, a `no_std` core over caller buffers,
    resumable across chunks, a 10-bit fast Huffman table, and it needs
    `adler2`.
  - zune-inflate and zune-png allocate unconditionally, so they do not fit.
- **Other terminals** (UNVERIFIED, from t-images' surveys): ghostty makes PNG
  decode an injected, nullable function; rio, wezterm and contour use the Rust
  `image` crate; foot declines Kitty graphics; sfte uses stb_image.
- **CLAUDE.md, "kitty's protocol is all-or-nothing":** a partial
  implementation that answers the query is a false claim.

**Options**

- **A. Keep Kitty parked, fix the six stale places, and record the unpark
  design as contingent.** Docs only. No pictures yet.
- **B. Move png.rs now,** into baud-domain or a new crate, with no Kitty code.
  Effort M for nothing a user sees, and code with no consumer, against
  CLAUDE.md's rule "a consumer that exists or a decision that is pinned".
- **C. The embedder injects the decoder,** ghostty's shape (the embedder is
  the app hosting the core, here baud-app). The seam's original reason is
  gone: it assumed a decoder has to allocate, and ours does not. A *different*
  embedder hook may still be needed, for the file and shared-memory mediums,
  because a `no_std` core cannot open files.
- **D. A third-party inflater (miniz_oxide).** It breaks "no engine
  dependencies" and duplicates an inflater baud already owns and gates.
- **E. Unpark and build everything:** alpha, decoder, base64, the protocol,
  queries and deletion. Effort L. You parked it, the budget cannot hold one
  full-window picture, and no recording carries Kitty bytes to measure
  against.

**Recommendation**

**Yes to A: Kitty stays parked. Correct the six stale places now, and record
the unpark design in `docs/research/notes/t-images.md` as contingent on
measurements, not as settled.**

**Design**

**Now, documents only:** each of the six places says instead that
`baud-font`'s `png.rs` (`baud-domain/src/png.rs` since step 2) is a
`no_std`, alloc-free, `forbid(unsafe)` PNG and
zlib decoder, with its limits (~~no Adam7;~~ Adam7 read since 2026-10-15;
bit-at-a-time Huffman; whole input contiguous). The Kitty row's reason becomes "parked by the owner; the real
prerequisite is alpha".

**At unpark, in order.** Each step with a choice is a fork for you, with my
lean:

0. ~~**Record first.** Record a `kitten icat` session, and take three numbers
   before any code: png.rs decode time on a full-window PNG; the Adam7 share
   of what is sent; the memory one full-window picture needs. The arena needs
   about 8.2 MB, raw scratch about 8.2 MB more, plus the payload: roughly
   three pictures' worth transiently.~~ **Done 2026-10-15**, on kitten
   0.49.2 at the app's 17x34 cell (the record, "A `kitten icat` session,
   recorded, and the three numbers V19 owed";
   `scripts/record-kitten-icat.py`):
   - **Decode: 39.5-39.6 ms** (39.3-40.1 over an A/A pair, 476.7 M
     instructions) for a real 1920x1080 RGB PNG of 961 KB, the one kitten
     sent whole to a 113x32 window (1921x1088 pixels); medians of 67.4-70.1
     ms for a high-entropy 1972x1094 one of 4.2 MB and 334 ms for a
     3840x2160 RGBA one of 13.7 MB. Step 3's bar is 8.3 ms, so a full window is about 4.7 frames,
     and step 3's own lean puts a table-driven Huffman fast path next.
   - **Adam7: 2.9% of the PNGs kitten would send as they are, 1.1% of their
     bytes**, over macOS 27's `/System/Library` and this Mac's
     `/Applications` (155 of the 5,377 no wider than 1921 pixels; system
     assets, not user pictures; all 155 interlaced ones pass at every window
     measured). kitten sends a PNG's own bytes when it needs no transform and
     is no wider than the window, whatever its height, since `--fit`
     defaults to `width`; it scales anything else and sends it raw
     (`[[source: kitty-icat-process]]`, `[[source: kitty-icat-options]]`).
     So Adam7 reaches the terminal only as a user's own file, and recorded
     it did, byte for byte. The share decides nothing: "Decoders are required to be able to
     read interlaced images" (`[[source: w3c-png-2003]]` 13.10, and 15.2.3's
     "all the standardized ... interlace methods"), so png.rs's refusal is a
     departure to close before `f=100` is answered. **Closed 2026-10-15**:
     png.rs reads Adam7 (the record, "png.rs reads Adam7").
   - **Memory: payload + raw + arena, exactly**, since png.rs allocates
     nothing; a process's peak footprint agreed within 79 KB, under 0.1%.
     1920x1080
     `f=100` RGB: 0.96 + 6.22 + 8.29 = **15.5 MB** (raw is 8.30 for RGBA).
     The 4K wallpaper kitten scales to 1921x1080 and sends as `f=24,o=z`: 4.22 +
     6.22 inflated + 8.30 = **18.7 MB**, the largest. At 80x24, 8.2-9.5 MB.
     The app's `ImageBudget`, 1,048,576 pixels, holds no 1920x1080 picture
     (2,073,600 pixels); kitten's 1360x765 at 80x24 fills 99.2% of it.
     Fitted to width alone, a tall PNG arrives at its full height, so the
     window does not bound a picture's memory.
   - **What kitten sends against the protocol's text** (step 5 has to take a
     position): chunks of 131,072 base64 bytes where `[[source:
     kitty-graphics]]` says "no larger than 4096 bytes"; `a=T` repeated on
     every chunk (`[[source: kitty-graphics-command]]`) where "Subsequent
     chunks must have only the m and optionally q keys" (`[[source:
     kitty-graphics]]`); unpadded base64, which
     the text allows, since only "All chunks, except the last" must be a
     multiple of 4.
     - *Fork, for Jev at 0.9 or you:* **(a)** accept a chunk of any length
       and ignore a continuation chunk's other keys: kitten works, and the
       text binds the client and says nothing of what a terminal does with a
       longer chunk, so no text is departed from. **(b)** refuse what breaks
       the client's rules: the letter enforced, and kitten 0.49.2 shows no
       picture whose base64 is longer than 4096 bytes. ~~No lean.~~ **Jev
       decided (a), 2026-10-15, 0.90/0.92**; the owner can veto.
1. **Alpha.**
   - *Fork:* a full alpha arena, or thresholding to opaque-or-clear, which is
     cheap, touches no render path and gives jagged edges. **Lean: full
     alpha**, because RGBA is the protocol's default format.
   - *Sub-question:* one stored convention, straight or premultiplied alpha
     (whether colours are stored already multiplied by their opacity). The CPU
     compositor keeps straight alpha and the Metal path blends premultiplied,
     so pick one and state the conversion on each path.
   - *Shape:* a new pixel type in baud-domain replaces `ColorWord` in
     `ImageStore`. Sixel writes full-alpha pixels, so its output is unchanged.
   - Each image is marked opaque or partial at decode. Opaque keeps today's
     cheap repeatable blit; partial repaints the rows it covers each frame.
   - **Built 2026-10-15** as decided: the owner's "Full alpha", and Jev's
     straight in the store, premultiplied once where it blends (1.00 and
     1.00). `Pixel` is straight `0xAARRGGBB`, each image `OpaqueOrClear` or
     `Partial` (the record, "Images keep straight alpha, and a partial one
     repaints its rows").
2. ~~**Where the decoder lives.**~~ **Done 2026-10-15**, in baud-domain
   (the owner's pick): `baud-domain/src/png.rs`, `baud-domain/src/zlib.rs`
   (inflate fed a read at a time, over any source of slices) and
   `Base64::feed` (chunks of any length, Jev's (a)); the record, "The
   decoders move to baud-domain, and inflate resumes across reads".
   - *Fork:* baud-domain, or a new leaf codec crate. **Lean: baud-domain**,
     which baud-core and baud-font both already depend on, and where t-images
     puts image decode. A new crate is viable too: `scripts/check-features.sh`
     now fails if a crate with features is missing from its list.
   - Either way it carries three things:
     - a zlib inflate generic over its input source;
     - PNG decode writing straight into the arena;
     - an incremental base64 decoder.
   - It is ported off baud-font's byte views (`baud-font/src/bytes.rs`), and
     baud-font then calls it.
3. **When it decodes.**
   - *Fork:* synchronously inside `feed` (the core's byte-ingest call), or
     deferred: read the size, reserve, place and move the cursor now, and
     inflate later between reads or on an embedder thread. The spec requires
     an immediate reply only for queries.
   - **Lean: synchronous if the bench is under one 120 Hz frame (8.3 ms).** If
     not, try a table-driven Huffman fast path first, then deferred decode.
   - Deferred decode cannot cover PNG with compression, whose header is itself
     compressed.
   - **The table-driven Huffman fast path is built, 2026-10-15, and 8.3 ms
     is not met** (the record, "Huffman codes decode through a table, and a
     full window still takes four frames"). mosaic-blue decodes in 32.5 ms
     against base's 36.5, 355.6 M instructions against 412.7 M: 32.5 ms,
     3.9 frames; 24.2 ms over the bar. The symbol loop, its lookups
     included, is now about a tenth of a decode; the Adler-32 is about two
     fifths, unfiltering a fifth, and the byte-at-a-time back-reference
     copy an eighth. By this step's own
     rule it falls to deferred decode, which is a design question for Jev.
4. **Transmission mediums.**
   - *Fork:* direct payloads (`t=d`) only, since the spec expects clients to
     probe and fall back; or an embedder hook for `t=f`, `t=t` and `t=s`, with
     a security policy on which files a stream may make baud read or delete.
   - **Lean: `t=d` only at first.**
   - ~~Which medium `kitten icat` uses against an unknown local terminal is
     UNVERIFIED.~~ Recorded 2026-10-15 (step 0): its default `detect` sends
     an `a=q` per medium, then a Primary DA. Answered `OK` for `t=d` alone,
     it sends `t=d`; with no `OK` it exits, "This terminal does not support
     the graphics protocol" (`[[source: kitty-icat-main]]`). It reads only a
     DA whose CSI is longer than three bytes (`[[source: kitty-icat-detect]]`),
     so against baud's default `CSI ? 6 c` it waits out its 10 s timeout
     first; `CSI ? 6 ; 4 c` (`[images] advertise_sixel`) fails at once.
5. **The protocol in baud-core.**
   - An `APC G` hook of the `Act::SixelStart` shape, and a payload scratch in
     `Storage` of the `sixel_body` shape.
   - Raw formats without compression base64-decode straight into the reserved
     arena.
   - ~~Compressed formats and PNG go through the scratch, because png.rs's
     inflate is not resumable.~~ Since step 2, `o=z` can inflate straight
     into the arena as its chunks arrive (`zlib::Inflate::feed`); PNG still
     goes through the scratch, because png.rs reads its container whole.
   - Every declared size (width x height, `S`, the PNG header's width and
     height) is checked against `ImageBudget` before anything is reserved, and
     a refusal is counted.
   - Placements, cursor movement and deletion reuse `ImageStore`.
6. **Answer no query until raw formats and PNG both decode.** This reverses
   t-images' order, which answered queries a step early.

**Tests and measurement**

**Now:** the docs gate (`baud-core/tests/docs.rs`) and
`scripts/check-sources.sh`. A grep for the six stale phrases returns none.

**At unpark,** each red first:
- `baud-font/tests/bitmap.rs`, unchanged and green through the moved decoder.
- A new oracle against the `png` crate over PngSuite (the standard PNG
  test-image set), pinned first: non-interlaced files match byte for byte, and
  interlaced ones are counted exactly.
- An inflate that refuses output which does not exactly fill, both short and
  long. This is the bomb bound.
- A zlib-only hostile property (a randomised test) at `PROPTEST_CASES=20000`.
  The existing arbitrary-bytes property rarely gets past PNG's signature and
  checksum, so it barely reaches inflate.
- A declared size past the budget is refused before reserving, for each of
  width x height, `S` and the PNG header.
- Compressed RGBA and PNG transfers are chunk-invariant (the same result
  however the bytes are split across reads) at read sizes 1 to whole.
- `a=q` is unanswered until both formats decode.
- The GPU pixel oracle gains a half-transparent picture. Both GPU arms agree,
  and the CPU incremental frame equals the full frame.

Perturbations (deliberately breaking one mechanism to prove a test notices):
drop the exact-fill check, drop Adler-32, drop alpha in the GPU upload,
reserve before the size check. Each turns its test red.

**Measurements owed before code:**
- the decode bench (`harness = false`, with an A/A copy: a byte-identical
  binary timed alongside to show the noise);
- the Adam7 share on the recorded session;
- the memory budget.

Once baud-core links the decoder, an `ingest2` sweep (a timed run of the
parser over the recordings): base, change and an A/A copy built at one path,
with cells identical.

**Effort and risk**

- **Effort:** now S (docs). At unpark: alpha and three render paths M-L; the
  decoder move and base64 M; the Kitty protocol L.
- **Risk:** none now. At unpark the risks are:
  - decode time on megabyte payloads is unmeasured;
  - a partly transparent picture costs CPU-path repaints while it is on
    screen;
  - linking the decoder into the core brings the code-layout risk CLAUDE.md
    records on `cat`.

**What would change it / depends on**

- **Changes it:**
  - you unpark Kitty;
  - the bench shows png.rs over 8.3 ms even with a fast table, which puts
    deferred decode or a table-driven inflater on the table;
  - a recorded session shows a real Adam7 share;
  - you relax "no engine dependencies".
- **Depends on:** your unpark; a recorded `kitten icat` session; your
  confirmation of ARGB storage (t-images.md, "The storage fork is decided ARGB in this register"), which becomes ARGB
  with alpha; and a new image budget. It is independent of V18.

## Part 7. Platform, process and the record

### V20. Linux GPU, X11 and Windows: confirm the 12 September answers

**The question**

On 12 September you answered three platform questions out loud.
`docs/research/notes/j-rendering.md` marked all three "unconfirmed" (its
owner-calls table) when this was asked; it records them confirmed since.

- **Call 2.** On Linux, is Vulkan the only GPU interface, at least for the
  first version? Vulkan is the modern cross-vendor way to drive a graphics
  card; OpenGL is the older one.
- **Call 3.** Does baud run on X11 desktops, or only on Wayland ones? Wayland
  is the current Linux display system; X11 (Xorg) is the old one.
- **Call 4.** Is Windows out until after a first release?

**What you would notice**

- **Today:** Linux baud runs only on Wayland, and it draws on the processor
  (the "CPU path"). There is no GPU path on Linux yet. Confirming changes
  nothing you can see.
- **After, as recommended:** on an X11-only desktop baud exits with a message
  that says why, instead of today's raw "wayland: No such file or directory".
  A Linux machine with no working Vulkan driver keeps the CPU path. Windows
  users get nothing yet.

**Today**

- `baud-present-wayland/src/lib.rs` (line 4) says "The GPU path (Vulkan over
  dmabuf) is not here." It draws through `wl_shm`, a shared-memory buffer the
  compositor (the program that owns the screen) reads from.
- `baud-app/src/main.rs` (lines 53-62) runs the macOS app on macOS and the
  Linux driver on Linux. Anything else prints that baud presents on macOS and
  on Linux over Wayland, "so far".
- `nix/rust.nix` (lines 37-40) names two Linux targets, so the Linux driver is
  lint-checked on this Mac (checked by clippy, Rust's linter, without being
  run).
- There is no Vulkan, X11 or Windows code anywhere.
- The Linux driver has run once, on weston headless (a compositor with no
  screen). Nobody has looked at its pixels, and it takes no mouse input.
- The only written trace of your three answers is an auto-memory note:
  "Vulkan-only Linux over pure-protocol Wayland; X11 = XWayland fallback; no
  Windows in v1". The session record itself is not in the repo.

**What others do**

- **Vulkan-only is a choice, not something the register's D13 forces.** D13 is
  the register's earlier decision to speak Wayland's wire protocol directly,
  with no libwayland (the C client library).
  - Two pinned sources show that the Wayland *surface* hookups need
    libwayland-client: `VK_KHR_wayland_surface` takes a `wl_display*` and a
    `wl_surface*` [[source: vulkan-vk-khr-wayland-surface]], and the
    window-system code of Mesa (the open-source Linux GPU driver stack)
    includes `wayland-client.h` [[source: mesa-wsi-common-wayland]].
  - Neither says anything about rendering into a buffer baud exports itself.
    The Wayland book says a library-free client can render with GL into a
    dmabuf (a GPU buffer shared by file descriptor) and hand it over through
    the linux-dmabuf protocol itself, using `EGL_MESA_image_dma_buf_export`
    (UNPINNED). EGL is the glue between OpenGL and a window system, and GBM is
    Mesa's GPU buffer allocator; which libraries that EGL/GBM route loads is
    UNVERIFIED.
  - So D13's own wording overstates it too: "rules out OpenGL on Linux"
    (j-rendering.md lines 203-206, and the evidence note at lines 559-560).
  - UNVERIFIED: Mesa's Vulkan drivers are commonly built with their own
    Wayland support and may link libwayland-client themselves. If so, Vulkan
    does not keep libwayland out of the *process* either; D13's rule is about
    baud's own code.
- **The library-free Vulkan route** is what Mesa does inside its own drivers:
  a `VkImage` with a DRM format modifier (a tag naming the GPU's memory
  layout), exported as a dmabuf
  [[source: vulkan-vk-ext-image-drm-format-modifier]]
  [[source: vulkan-vk-ext-external-memory-dma-buf]], and handed to the
  compositor through linux-dmabuf [[source: wp-linux-dmabuf-v1]].
- **"XWayland fallback" is the repo's own phrase.** The register's D13 ends
  with "Owner calls: ... X11 first-class or XWayland fallback" (j-rendering.md
  lines 227-228), and the owner-calls list asks "X11: first-class, or XWayland
  fallback?" (line 265). You picked one of two options the note offered.
  - The wording misleads. XWayland is an X11 server that runs *inside* a
    Wayland desktop so old X11 programs keep working. It does nothing for a
    Wayland program like baud on an X11-only desktop.
- **Native X11 is a second presentation stack.**
  - A pure-Rust connection exists [[source: x11rb-rust-connection-mod]].
  - The CPU path would be MIT-SHM, X11's shared-memory image extension, with
    one unavoidable server-side copy [[source: xorg-mit-shm]]
    [[source: xcb-proto-shm]].
  - The GPU path would be DRI3 plus Present, X11's extensions for sharing GPU
    buffers and timing their display [[source: xorg-dri3proto]]
    [[source: xorg-presentproto]].
  - The keymap arrives through the XKB extension (X11's keyboard-layout
    extension) [[source: xcb-proto-xkb]], a different input from the text
    keymap `baud-xkb` compiles today. How much of `baud-xkb` carries over is
    UNVERIFIED.
- **Wayland-only terminals ship.** foot calls itself "The fast, lightweight
  and minimalistic Wayland terminal emulator" and lists "Wayland native"
  [[source: foot-readme]]. That foot has no X11 path at all is UNVERIFIED.
- **Windows** needs a flip-model swapchain (the rotating set of buffers a
  window shows) whose buffers the CPU cannot write directly, so its floor is
  one GPU copy [[source: ms-dxgi-flip-model]]
  [[source: wt-atlas-atlasengine-r]]. It also needs ConPTY (Windows'
  pseudo-terminal) in place of forkpty (the Unix call that starts a program on
  a pty, the pseudo-terminal a shell talks through), a user32 window (the
  Windows window API), per-monitor DPI handling [[source: ms-high-dpi]] and
  its own font discovery.
- **Where desktops are going** (all UNPINNED, from the web):
  - GNOME 49 disabled its X11 session by default at build time, and GNOME 50
    removes the code. Fedora 43 shipped GNOME Wayland-only.
  - KDE announced on 2025-11-26 that Plasma 6.8, due 2026-10-14, drops its X11
    session.
  - X11 sessions remain on Xfce, MATE, Cinnamon and i3-style window managers.

**Options**

- **A. Confirm all three, scoped to v1 and reworded.** Cost: a doc edit.
  Forecloses nothing: the `baud-present-*` crate seam lets X11 or Windows
  arrive later as new crates. Accepts that X11-only users and machines without
  Vulkan get less.
- **B. Add OpenGL/EGL as a second Linux GPU backend.** Wider coverage on old
  GPUs. EGL's Wayland *platform* pulls in libwayland-client, which breaks the
  register's D13; a GL route rendering into a dmabuf baud exports may not
  (UNVERIFIED). Either way it is a third renderer to keep pixel-equal, and the
  CPU path already covers those machines.
- **C. Native X11 in v1.** Serves X11-only sessions. Roughly doubles the Linux
  presentation code (j-rendering.md lines 265-266), with its own input path,
  tests and pixel checks.
- **C2. Native X11 later, as a secondary target.** The one coherent reading of
  "XWayland fallback": an X11 backend developed and tested under XWayland on a
  Wayland desktop, which also runs on a real X11 session. A later, cheaper
  form of C.
- **D. Windows in v1.** The largest and most distinct surface. No zero-copy
  CPU present, and it delays the Linux GPU work that is not started yet.

**Recommendation**

**Yes to A for calls 2 and 4. Call 3 is a fork; lean (a).**

- **Call 2:** "For v1, the Linux GPU path is Vulkan only, rendering into a
  dmabuf with a self-managed swapchain. The CPU renderer is the floor where
  Vulkan is missing. This is a one-GPU-API choice; the register's D13 alone
  does not force it."
- **Call 4:** "No Windows until after a first release."
- **Call 3, pick one:**
  - **(a) Wayland-only for v1**, with a clear exit message on X11-only
    sessions. **This is the lean.**
  - (b) A native X11 backend later, as a secondary target (option C2).
  - (c) Native X11 in v1 (option C).
  - Drop the words "XWayland fallback" whichever you pick.

**Design**

- **Now, a doc edit.** Move calls 2-4 to settled in the table at
  j-rendering.md lines 282-289, in the wording above. Correct D13's "rules out
  OpenGL" wording at lines 203-206 and in the evidence note.
- **Call 3 (a), one small code change.** On Linux with no Wayland socket,
  print "baud needs a Wayland session; X11 is not supported" instead of the
  raw error. The message could also name a workaround: run a nested Wayland
  compositor inside the X11 session (weston, or a wlroots compositor on its
  X11 backend) and baud inside that. The workaround is UNVERIFIED and
  untested.
- **Later, the Vulkan path:**
  - **One module holds all the `unsafe`**, as
    `baud-present-macos/src/metal.rs` does on macOS. It goes in
    `baud-present-wayland` or a sibling crate.
  - **It reuses `baud-render`'s outputs unchanged**: the 20-byte instance
    records, the background bitmap, and the cell path's grid, style table and
    glyph map. These are not tied to any GPU API.
  - **Shaders:** a SPIR-V twin (Vulkan's compiled shader format) of
    `baud-present-macos/src/shaders.metal`, committed and held to its source
    by a digest test, as `baud-present-macos/tests/metallib.rs` holds the
    Metal library. Regenerating it needs a shader compiler on some machine,
    the same toolchain question the Metal library had with Xcode.
  - **Loading:** open `libvulkan.so.1` at run time rather than linking it, so
    the binary starts without it and falls back to the CPU path with a logged
    reason. That mirrors `Gpu::new` returning `None` on macOS. It needs
    `dlopen`, which `baud-app` would declare in `baud-app/src/sys.rs`, since
    it has no libc crate.
  - **Open sub-decision, bindings:** the `ash` crate follows the objc2-metal
    precedent, and its loader brings in `libloading`. Hand-declared entry
    points follow the precedent of removing libc.
  - **Presentation:** pick the modifier from linux-dmabuf's feedback, export
    the image, attach it through linux-dmabuf's buffer-params request, and
    pace on `wl_surface.frame` as the CPU path already does. Sync implicitly,
    or with `linux-drm-syncobj-v1` (explicit sync: the GPU telling the
    compositor when a buffer is finished) where a driver needs it.

**Tests and measurement**

- Confirming owes nothing, and nothing was run.
- The exit message owes a unit test that a missing socket yields it, perturbed
  by removing the message.
- The Vulkan work, later, owes:
  - **A pixel oracle** (a test that draws one scene two ways and requires
    every pixel equal): the Linux twin of
    `the_cell_arm_draws_what_the_instance_arm_draws` in
    `baud-present-macos/tests/cells.rs`, through the Vulkan instance arm and
    the Vulkan cell arm. Perturb it as the Metal one was: the shader slot
    counts, the wide-tail rule, a dropped pan. Whether Mesa's software Vulkan
    (lavapipe) can run it in CI is UNVERIFIED.
  - **`frame_gpu` arms** (the GPU frame benchmark) on a Linux GPU machine
    against the CPU path, with an A/A copy in the sweep. An A/A copy is a
    byte-identical copy of one build, run in the same sweep to show how much
    the machine alone moves a number.
  - **`idle-cpu`** (the idle CPU measurement), and discrete-GPU placement
    arms. No such machine exists today.

**Effort and risk**

- **Effort:** confirming is S. The Vulkan path, native X11 and Windows are
  each L.
- **Risk:** none in code from confirming. The product risks you accept:
  - X11-only users cannot run baud.
  - Machines without Vulkan get the CPU path. CLAUDE.md measures it at about
    0.56-2.1 ms of CPU a frame on this Mac, against 40-190 us for the Metal
    cell path as it ships (14-19 us before it drew ligatures and marks).
  - dmabuf modifier support and explicit sync on NVIDIA vary by driver
    (UNVERIFIED).

**What would change it / depends on**

- **Changes it:** a real X11 user base, such as your own Linux machine on an
  X11 session (then (b) or (c)); the CPU path too slow on a Linux box with no
  Vulkan; libwayland-client becoming acceptable (reopens EGL and
  `VK_KHR_wayland_surface`); a Windows user or sponsor.
- **Depends on:** call 1, the strict engine rule (settled), and the register's
  D13. By order only, Linux pixels and pointer input come before any Vulkan
  work.

### V21. Let the later pre-push checks run after a failed unit test

**The question**

Before every push, and in CI, `devenv test` runs about ten checks in a chain.
When the unit tests fail, the five checks after them are skipped:

- the bare-metal build (proof the core builds with no operating system);
- the Linux lint;
- the type audit;
- the feature-arm check (each measurement build variant compiled on its own);
- the path check.

Should those five run anyway, so one failed push reports everything that is
wrong? The push stays blocked either way.

**What you would notice**

- **Today:** a failed push names only the first broken category. You fix the
  unit test, push again, and only then learn the audit was also red. Each
  round costs a full `devenv test`.
- **After:** a failed push lists every independent failure at once. A red run
  takes somewhat longer; how much is unmeasured.

**Today**

- The chain is in `devenv.nix` (lines 257-335): fmt, clippy, check, unit,
  bare, linux, audit, features, paths. `test:sources` hangs off fmt.
- Every edge is a plain `after`, which in devenv means "run only if the task
  before succeeded".
- The comment at lines 252-256 gives **three** reasons for the order:
  - cargo invocations do not contend for the single lock over `./target`;
  - the cheapest check runs first ("a formatting failure costs no compilation
    at all");
  - each task reuses what the previous one compiled.
- The comment at lines 277-282 says `--no-fail-fast` (added 2026-09-22) stops
  one red test binary hiding the rest *inside* cargo, but "does not un-skip
  the gates chained after this task".
- `test:paths` runs no cargo: `scripts/check-paths.sh` only reads manifests.
- `enterTest` (line 338) names all ten tasks. The pre-push hook runs
  `devenv test`, and so does CI (`.github/workflows/ci.yml` line 103), after
  installing an unpinned `nixpkgs#devenv` (line 81). The CLI on this Mac is
  devenv 2.2.0.

**What others do**

This is about devenv's own rules. The evidence is its docs and its v2.2
source, fetched read-only; none of it is pinned in the manifest (UNPINNED).
Whether the installed 2.2.0 binary matches that source is UNVERIFIED, and so
is CI's devenv version.

- **`@completed`** on an edge means "run after the other task finishes,
  whatever its exit code". devenv calls it a soft dependency.
  - It needs devenv 2.0 or later. v1.11 rejects any unknown suffix with an
    error, so an old CLI fails loudly, not silently.
  - The installed 2.2.0 binary contains the suffix.
- **What a "soft failure" is** (*is_soft_failure*): a failed task counts as
  soft only if it was not named on the command line (it is not a "root") *and*
  every one of its outgoing edges is `@completed`.
- **How the exit code is set:** *has_failures* leaves soft failures out, and
  the run exits 1 only when *has_failures* is true (in devenv's *tasks_run*).
- **Consequence, safe today:** `enterTest` names every task, so each one is a
  root, and a red `test:unit` stays a hard failure. `devenv test` still exits
  1, now after running the other gates.
- **Consequence, the trap:** suppose `enterTest` is ever shortened, or someone
  runs a single later gate alone. `devenv tasks run` runs the whole chain
  before a task by default. Then `test:unit` is not a root and its only
  outgoing edge is soft, so the run **exits 0 with red unit tests**.

**Options**

- **A. Keep as is.** The fastest red result, and no devenv subtlety. One push
  shows one category.
- **B. `@completed` on the edges after test:unit; keep fmt, clippy, check and
  unit hard.** One push reports every independent gate. Cargo stays
  serialised, and a formatting or compile error still stops early. Costs: a
  red run takes longer; the exit-0 trap must be guarded; and a compile error
  makes bare and linux fail again on the same error, which is noise.
- **C. `@completed` on every edge.** The most complete report. But a
  formatting typo then costs a full release test build, against the
  cheapest-first reason at lines 252-256.
- **D. Remove the edges.** Maximum parallelism. But every cargo run fights for
  the lock over `./target`, the output interleaves, and the reuse of compiled
  work is lost.
- **Add-on 1: hang `test:paths` off `test:fmt`**, as `test:sources` already
  is. It runs no cargo, so a cargo failure can never skip it and it needs no
  soft edge. Cheap, and it works with A or B.
- **Add-on 2: a final "all gates" task with a hard edge from every gate.** A
  gate with a hard outgoing edge can never be a soft failure, so a red gate
  stays a hard failure even when it is not named. It protects only runs that
  schedule that final task, and it is untested.

**Recommendation**

**Yes to B.** Make the edges after test:unit `@completed`, and keep the first
four checks hard.

Guard the trap in the same change:

- a comment above `enterTest` saying every task must stay named;
- a one-time perturbation check before merging.

**A second, separate yes/no: add-on 1, moving `test:paths` after `test:fmt`?
Lean yes.** It is independent of B and costs one line.

Add-on 2 is worth trying later if the comment feels too weak, once it has been
tested.

**Design**

- **`devenv.nix` edges for B:**
  - line 294: `test:bare` after `test:unit@completed`;
  - line 301: `test:linux` after `test:bare@completed`;
  - line 316: `test:audit` after `test:linux@completed`;
  - line 323: `test:features` after `test:audit@completed`;
  - line 333: `test:paths` after `test:features@completed`, or, with add-on 1,
    after `test:fmt`.
- **Comments to rewrite:**
  - lines 277-282, which say a red unit run stops everything after it;
  - lines 252-256, to say that order and failure propagation are now separate
    things. Keep all three reasons.
- **New comment above `enterTest` (line 338):** a task is a root only when
  named, and a non-root failure whose outgoing edges are all `@completed` is
  soft, so the run would exit 0. Every gate must therefore stay named. Name
  devenv's *is_soft_failure*.
- **CI:** `nixpkgs#devenv` at ci.yml line 81 is unpinned. Pin a 2.x version,
  or accept that an older CLI stops with an error on the suffix.
- **Known noise:** when `test:check` fails, bare and linux still run and fail
  on the same compile error. Harmless.
- **No crate is touched**, so no throughput measurement is owed.

**Tests and measurement**

All owed; none was run. Each is a temporary edit of `devenv.nix`, reverted
afterwards:

1. Set `test:unit` to run `false`. `devenv test` must exit non-zero, and the
   log must show bare, linux, audit, features and paths as run, not as
   "dependency failed".
2. Set `test:bare` to `false`. The later gates must run, and the exit must be
   non-zero.
3. Control, all green: exit 0.
4. Show the hazard on purpose. With unit red, run
   `devenv tasks run test:features` alone and record that it exits 0. That is
   the reason for the `enterTest` comment.
5. CI's first run after the change confirms its devenv accepts the suffix.

The one measurement is the extra time a red run now spends in the later gates.
It is unmeasured.

**Found while checking, out of scope:**

- `test:clippy` (line 263) runs without `--all-targets`, so the pre-push chain
  does not lint tests or benches.
- The hand-written `dev-ci` script (lines 85-108) is a different pipeline from
  the pre-push hook of the same name, which runs `devenv test`. The script
  stops at its first failure and skips the feature check, the path check and
  the baud-font bare build.

**Effort and risk**

- **Effort:** S.
- **Risk:** low. What remains: the exit-0 trap if `enterTest` is shortened
  (the comment and check 4 cover it), and CI's unpinned devenv. A red run also
  takes longer.

**What would change it / depends on**

- **Changes it:** the later gates are slow enough that you prefer fast failure
  on push; devenv changes its soft-failure rule; or CI's devenv lacks the
  suffix and you do not want to pin it.
- **Depends on:** nothing else here. It relates to the pty SIGKILL flake: a
  flaky red unit test still blocks the push, but you now see the other gates'
  results in the same run.

### V22. Resize: keep reflow as the only model

**The question**

When you drag the window bigger or smaller, baud keeps the text the same size
and changes how many rows and columns fit. Long lines re-wrap to the new
width; this is called "reflow". The alternative keeps the rows and columns
fixed and grows or shrinks the font to fill the window. Should baud offer that
second mode?

**What you would notice**

- **Reflow (today):** a bigger window gives vim, htop and tmux more room. A
  narrower one wraps long lines, and whatever no longer fits scrolls into the
  history, where the wheel reaches it.
- **Scale-font:** the layout never changes, only the text size. Programs never
  gain room, and every step of a drag re-renders every glyph at a new size.
- **Where the item came from:** a report after running fastfetch, that output
  "disappears up and does not wrap". CLAUDE.md's resize section concludes this
  was the build before the scale fix, which reported about 298 columns on a
  shrunk window. On the fixed build, wrapping works and nothing is lost. That
  conclusion is an inference: the reproduction ran on the fixed build, not
  with you.

**Today**

- **Reflow is the only resize path.** `relayout` (`baud-app/src/app.rs`,
  line 4374) computes the grid from the window's pixels with `grid_in`. Then
  `resized_window` (line 2002) calls `Term::resize_holding`, which reflows in
  place.
- **A fixed-grid mode exists, for benchmarks only.** `Sizing::Fixed`
  (line 296) is chosen with `BAUD_GRID=COLSxROWS` (lines 3648-3658). It keeps
  the grid and tells the pty (the pseudo-terminal the shell talks through)
  only the new pixel size (lines 4424-4427). It does not scale the font.
- **Zoom is the font lever.** Cmd+, Cmd- and Cmd-0 go through `zoom` (line
  4568), bounded to 6-72 points (lines 4009-4010). Zoom uses `rescale` (line
  1462): new metrics, an atlas reset (the cache of drawn glyphs is emptied),
  and the new cell size sent to the core. A zoom waits for a frame in flight
  (`Deferred::Zoom`, line 4588).

**What others do**

- **Every pinned implementation reflows:** foot (*grid_resize_and_reflow*)
  [[source: foot-grid-c-1-21]], alacritty
  [[source: alacritty-grid-resize-0-26]], wezterm (*rewrap_lines*)
  [[source: wezterm-screen-b09b56c]], ghostty
  [[source: ghostty-pagelist-9bbb9b2]] and kitty
  [[source: kitty-screen-0-49]].
- **kitty's configuration reference** [[source: kitty-conf]] has
  `resize_debounce_time` (when to ask the program to redraw) and
  `change_font_size`. It has no font-scale-on-resize option. Every "scale" on
  that page is about box drawing, the window logo, background images or
  fractional scaling.
- **Opt-in precedents** (UNPINNED, from their web manuals):
  - PuTTY 0.83 (manual section 4.7.2) offers: change rows and columns (the
    default), change the font size, change the font size only when maximised,
    or forbid resizing.
  - mintty scales the font when you hold Shift while resizing
    (`ZoomFontWithWindow`, on by default). Its manual calls this "not a
    precise operation".
- **Two corrections to CLAUDE.md's resize section:**
  - "It lies to TUIs about the size" is wrong. A TUI is a full-screen text
    program such as vim or htop. Under a fixed grid the pty's rows and columns
    stay true, and only the pixel reports change; `Sizing::Fixed` already
    sends them. What a program loses is room, not truth.
  - "No mainstream terminal does this on resize" is too strong. PuTTY and
    mintty offer it, with reflow as their default.

**Options**

- **A. Reflow only; close the item.** No code. Correct for every TUI, and the
  fastest drag. There is no fixed-grid mode for demos or screen recording.
- **B. A config key, `resize = "reflow" | "scale-font"`, default reflow.**
  About M of code. Programs never gain room. Every changed drag step resets
  the atlas; the per-step cost is unmeasured, and for comparison, the first
  frame after a reset on a page of CJK text (Chinese, Japanese, Korean) costs
  about 3 ms. Rounded cells cannot fill the window exactly, so there are
  letterbox margins (empty bands at the edges).
- **C. Hold a modifier during a live resize to scale instead, as mintty
  does.** Needs the modifier state during AppKit's live resize, which
  `macos-liquid-glass` does not expose today. Hard to discover.
- **D. Scale only in fullscreen, as PuTTY's "when maximised" does.** A niche
  with B's costs.

**Recommendation**

**Yes to A: keep reflow as the only resize model, and close the item.** Cmd+,
Cmd- and Cmd-0 stay the way to change text size. Revisit B only if you want a
fixed-grid presentation mode for recording or demos.

If content still disappears on a shrink-then-grow with the current build,
report that as a bug. It is not a reason for scale-font.

**Design**

- **For A, a doc edit:**
  - close the work list's resize item;
  - in CLAUDE.md's resize section, change "lies to TUIs" to "gives programs no
    room";
  - name PuTTY and mintty as opt-in precedents where CLAUDE.md says "no
    mainstream terminal".
- **For B, if you want it later.** It touches `baud-app` only; `baud-core` is
  untouched, so no ingest measurement is owed.
  - A `ScaleFont` variant of `Sizing`.
  - A pure function, *font_px_to_fit*, that returns the largest whole-pixel
    font size whose rounded cell fits the window's width over the columns and
    height over the rows.
  - `relayout` rescales only when that size changes, so most drag steps cost
    nothing.
  - The pixel size reaches the pty through the existing `tell_pty`.
  - A `resize` key, covered by the config reader's complaint list and the
    example-config test.
  - The same logic in the Linux driver's configure (`baud-app/src/linux.rs`).

**Tests and measurement**

- **For A:** nothing is owed, and nothing was run.
- **For B:**
  - unit tests of the fit function (it never overflows, it is monotone in the
    window size, it moves in whole-pixel steps), each perturbed;
  - the example-config test;
  - the drag-step cost, through `baud-bench/benches/reshape.rs` with a
    scale-font arm and an A/A copy;
  - the first frame after a rescale on a CJK page.

**Effort and risk**

- **Effort:** A is S (a doc edit). B is M. C is M to L, because it needs a
  window-crate change.
- **Risk:** none for A. Anyone who wants a fixed layout can already launch
  with `BAUD_GRID`, though without font scaling.

**What would change it / depends on**

- **Changes it:** you want to present or record at a fixed 80x24; a user asks
  for it; or a shrink-then-grow on the current build still loses content.
- **Depends on:** nothing else here. Option C would need a
  `macos-liquid-glass` change.

### V23. The two lost long recordings: re-capture, or fix the record

**The question**

On 2026-09-13 two 55-second recordings were made at 200x50: `less` paging with
the space bar through baud's own Rust source, and `vim -u NONE` paging through
the same text with Ctrl-F. They fed one table in CLAUDE.md on how often the
ligature window cache misses over a long session. That cache holds shaped
character sequences such as `->` and `//`. Then the recordings were lost with
a scratch directory.

Should they be recorded again and committed, or should the table be marked as
no longer checkable?

**What you would notice**

Nothing on screen either way. This is about two things:

- whether a number in CLAUDE.md can be checked;
- whether the recording collection covers paging through code on the alternate
  screen (the separate screen that full-screen programs such as less and vim
  draw on).

**Today**

- **The table** is CLAUDE.md's "A long session says what the window cache
  holds" (line 12897). Per frame:
  - `less` over code under JetBrains Mono: 62 windows, 17 missing;
  - `vim`: 31 windows, 11 missing;
  - `less` under FiraCode: 50 windows, 37 missing.
- **Its prose** says "a quarter to a third of the windows miss" (line 12915).
  That holds for the two JetBrains Mono rows (17 of 62, 11 of 31). It does not
  hold for the FiraCode row, which is 37 of 50, or 74%.
- **The committed corpus** is nine recordings, about 3.0 MB. Every gate names
  its recordings in a hand-written list:
  - `baud-core/tests/golden/recordings.txt`;
  - `baud-render/tests/support/mod.rs`;
  - `baud-sfte-difftest/src/lib.rs`;
  - `bench-others/multibench`;
  - `baud-bench/benches/ingest2.rs`.
- **The capture tool** is `scripts/capture-tui.py`, which takes the seconds,
  rows, columns, keys and key delay.

**What others do**

This is baud's own corpus, so the evidence is internal.

- **The committed recordings already give figures of the same order.**
  - `tests/recordings/ascii/cat-scroll.raw` is `cat` of 437 KB of Rust source
    at 200x50, about 110 frames at one 4096-byte read a frame.
  - CLAUDE.md's row-7 table ("A ligature window's miss is 3 us") has its
    window figures: 61 windows and 18 misses a frame under JetBrains Mono, 54
    and 37 under FiraCode. git-log reads 27 and 15, and less-pager 56 and 23.
  - The lost rows were 62/17 under JetBrains Mono and 50/37 under FiraCode.
    These come from different sessions and builds, so they are "the same
    order", not "reproduced".
- **"Long" meant wall-clock time, not frames.** At 153 and 136 frames, the
  lost captures are about the length of cava (~155), btop (~128) and git-log
  (~123).
- **What a re-capture would add is coverage, not reproducibility:** paging
  through code on the alternate screen under a ligature face (a font that
  joins sequences such as `->`). The committed pager recording, less-pager, is
  30 KB, about 8 frames.
- **Golden pins would pin baud against itself.** A golden pin is a stored
  digest the test compares the rendering against. `baud-core/tests/golden.rs`
  is documented as "What `baud-core2` rendered", the deleted previous core.
  Its recording digests were recomputed from baud-core when the digest's fold
  changed, and its non-blank counts are still core2's. New rows would be a
  regression pin with no outside check.
- **A precedent for "no longer checkable":** when the binary behind `opcost`'s
  per-shape figures was deleted, CLAUDE.md kept them for their ordering and
  forbade quoting them as current.
- **The decision the table supported stands without it.** Closing the
  "bootstrap cache" idea also rests on two other measurements: the faces map
  in 0.2 ms, and the first prompt frame has one map miss.
- **Size, if captured:** about 0.62-0.63 MB and 0.55-0.56 MB at one 4096-byte
  read a frame, provided the frame counts are right; roughly +40% on the
  corpus.

**Options**

- **C+. Do not re-capture; fix the record.**
  - Mark the table "from captures no longer held; ordering only".
  - Point its steady-state claim at the row-7 figures, which can be re-taken.
  - Scope "a quarter to a third" to JetBrains Mono.
  - Close the TODO item.
  - Cost: a doc edit. Forecloses nothing.
- **B. Re-capture and commit as a separate "long" tier**, a new long/ folder
  beside ascii/ and utf8/.
  - Pin their hashes, and use them in the benches.
  - Keep them out of the per-read-size sweeps, and state that at every list. A
    per-read-size sweep feeds a recording at many read sizes to prove the
    screen does not depend on where reads split.
  - Adds the coverage. Costs about two hours and 1.2 MB, and a deliberately
    partial gate scope that must be written at every list.
- **A. Re-capture as full members of every list.** Maximum coverage. The extra
  suite time is unmeasured, in every per-read-size sweep and in the sfte
  differential (the test comparing baud's screen with another terminal's,
  sfte, at every read boundary). The golden rows would be self-pins.
- **D. Re-capture into a scratch directory again.** It repeats the loss that
  created this item.

**Recommendation**

**Yes to C+: mark the table, point to the committed recordings' figures, fix
the sentence, and close the item.** This reverses the researcher's lean toward
re-capturing, because the committed `cat-scroll` recording already covers the
steady-state claim.

Separately, one yes/no: **do you want alternate-screen paging of code in the
corpus?** If yes:

- capture both sessions into the repository, never a scratch directory;
- pin their length and hash in the README;
- pass them to `frame` and `frame_gpu` by path;
- keep them out of `recordings.txt` unless those rows are labelled as
  self-pins.

**Design**

- **For C+:** edit CLAUDE.md's long-session section (a status line, a pointer
  to the row-7 table, and the scoped sentence), and close the work list's
  entry for the two lost recordings.
- **If captured:**
  - **Pin the input first.** Either generate the paged text with a committed
    script at a named commit, or commit the text itself, as `cjk-corpus.txt`
    was. The 2026-09-13 input is known only as "43,841 lines of this repo's
    own Rust", with no commit.
  - **Capture** with `scripts/capture-tui.py` at 55 seconds, 50 rows and 200
    columns: the space bar for less, Ctrl-F for vim.
  - **A "long" section in `tests/recordings/README.md`** giving each file's
    command, commit, input, bytes, non-ASCII share, scroll count, what it
    exercises, and which gates include or exclude it and why.
  - **Re-take the table** with `frame_gpu`'s async arm under JetBrains Mono
    and FiraCode: three launches, first round discarded, on mains. Replace the
    old table and say so.
  - **Two corpus rules** the new captures must follow: never concatenate
    recordings, because they end mid-session; and never derive a number from
    an edited recording.

**Tests and measurement**

- **For C+:** none.
- **If captured:**
  - flip one byte of a file, and the length/hash pin must fail;
  - if the files join a list with a count control, delete a line, and the
    count must fail;
  - the table re-take above;
  - one timing of the suite with and without the files in the per-read-size
    sweeps, before promoting them further.
- No capture, cargo run or perturbation was done.

**Effort and risk**

- **Effort:** C+ is S. Capturing is S to M: about two minutes of capture, 1-2
  hours of pins and README, and 30 minutes to re-take the table.
- **Risk:** low. If captured, the main risk is the partial gate scope, which
  stating it at every list mitigates.

**What would change it / depends on**

- **Changes it:** you want pager-over-code coverage (take the capture), or
  full membership turns out nearly free in suite time.
- **Depends on:** nothing else here.

### V24. Delete the old backup branches and the stash

**The question**

There are five leftover local branches, made as safety copies before squashes
(combining several commits into one) and history rewrites, and one stash (a
set-aside change from August). Can they be deleted, together with twelve old
branches already merged into `master`?

`smooth-scroll` is already pushed: it tracks `origin/smooth-scroll` at
`0987b98`. So the precondition this item waited on is met.

**What you would notice**

`git branch` and `git stash list` would show only `master` and
`smooth-scroll`. Nothing in the code changes.

**Today**

- `smooth-scroll` is at 0987b98 and on origin. `master` equals `origin/master`
  (9a60cc3).
- **The six candidates:**
  - `backup-pre-squash-smooth-scroll` (959cdbe);
  - `backup-pre-fixup-squash` (ce70108);
  - `backup-pre-split-ris-prompt` (713493c);
  - `sfte-oracle` (0d7c5ba);
  - `backup-pre-trailer-rewrite` (0cc2fa7);
  - `stash@{0}` (c5f2198): "graph-walks-everything: -45% btop, -23%
    asciiquarium, +3% cat-cjk".
- **Twelve more branches are fully merged into `master`:** `row0` through
  `row10`, and `note-n-and-gate-audit`, which also exists on origin.
- None of these holds the `$S/` build-artefact blobs that the history rewrite
  removed.

**What others do**

Each claim was checked read-only with git; there is no outside practice to
cite.

- **`backup-pre-squash-smooth-scroll`.** Its code is already on
  `smooth-scroll`: its tip tree is identical to that of 6b51c36, which is an
  ancestor. What is unique to it is 63 pre-squash commit messages, 88,673
  bytes, which were condensed to 30,173 on purpose under the
  one-commit-per-feature rule. That is about 58 KB of prose.
- **`backup-pre-fixup-squash`.** It has the same tip tree. Its 11 commit
  messages are byte-identical to `smooth-scroll`'s, and 3 of its 11 trees are
  intermediate states from before the fixups.
- **`backup-pre-split-ris-prompt`.** It is an ancestor of `smooth-scroll`,
  with nothing of its own.
- **`sfte-oracle`.** 7 of its 10 commits are patch-identical on
  `smooth-scroll`. The other 3 are present in squashed form, and their
  findings are in MEASUREMENTS.md.
- **`backup-pre-trailer-rewrite`.** It matches master's commits in tree and
  subject. It differs only by 12 AI co-author trailer lines, which you had
  removed, so keeping it keeps exactly what you deleted.
- **The stash.** It was made on 2026-08-29, on the previous core's parser
  code, which no longer exists, so it cannot be applied. Its finding was
  re-measured on the current core (CLAUDE.md, "The scan is not the
  protocol's": btop 503 against 884 with `probe-no-fast-csi`).
- **What "recoverable after deleting" really means.**
  - `git branch -D` also deletes that branch's own reflog (git's log of where
    a ref pointed). Recovery then rests on HEAD's reflog and on the timing of
    git's garbage collection, which uses git's defaults because this repo sets
    none.
  - A dropped stash is recoverable only through `git fsck`.
  - That is why the one branch with unique content gets a bundle: a single
    file holding a branch's commits.

**Options**

- **A. Delete all six, and the twelve merged branches.** A clean list. The 58
  KB of original commit messages is lost once garbage collection prunes it.
- **B. The same, but bundle `backup-pre-squash-smooth-scroll` first**, into
  one file outside the repo. It keeps the original messages and is reversible,
  at the cost of one extra file. A bundle is a different form from the
  existing mirror-clone backup (`~/src/baud-backup-pre-S-rewrite.git`); it is
  not the same convention.
- **B2. Keep a tag instead of a bundle**, such as
  `archive/pre-squash-smooth-scroll`, if you want the messages kept inside the
  repo.
- **C. Keep everything.** Zero risk, but clutter, and the trailer backup keeps
  text you asked to have removed.

**Recommendation**

**Yes to B.** Bundle the pre-squash backup outside the repository and verify
the bundle. Then delete the five branches, drop the stash, and delete the
twelve merged branches.

Deleting `note-n-and-gate-audit` on origin is a separate call and is not
included.

**Design**

The commands, in order:

1. `git bundle create ~/src/baud-pre-squash-smooth-scroll.bundle backup-pre-squash-smooth-scroll`
2. `git bundle verify ~/src/baud-pre-squash-smooth-scroll.bundle`
3. `git branch -D backup-pre-squash-smooth-scroll backup-pre-fixup-squash backup-pre-split-ris-prompt sfte-oracle backup-pre-trailer-rewrite`
4. `git stash drop stash@{0}`
5. `git branch -d row0-keystroke-instrument row1-shader-cache row2-bundle-icon row3-early-spawn row4-overlap-nsapp row5-dead-strip row6-pace-by-completion row7-window-miss row8-map-warm row9-local-echo row10-stale-redraws note-n-and-gate-audit`

Step 5 uses `-d`, which refuses any branch that is not merged, so it checks
itself. The check works because HEAD (`smooth-scroll`) contains `master`.

Separately, a docs commit for stale text found along the way:

- The work list said both "Pushed 2026-09-25" and "`smooth-scroll` is
  local only, with no branch on origin", a contradiction in a file no commit
  carries.
- README.md's status table, found by reading it while writing this note:
  - "Grid: two storage representations" (line 41) describes the deleted core's
    two storage modes;
  - the Scrollback row (line 43) still says nothing in the app scrolls to the
    history, though the app has a wheel and a scrollbar;
  - "Renderer" and "Font shaping" (lines 46-47) still read "not started";
  - the VT row (line 39) still says 147 done and 2 partial, which CLAUDE.md
    records as stale.

**Tests and measurement**

- **Already run, read-only:**
  - ancestry (`merge-base --is-ancestor`);
  - tree identity;
  - patch equivalence (`git cherry`);
  - commit-message byte counts;
  - the absence of `$S/`.
- **During:** `git bundle verify` must pass before anything is deleted.
- **After:** `git branch` and `git stash list` show only `master` and
  `smooth-scroll`.
- No cargo was run.

**Effort and risk**

- **Effort:** S.
- **Risk:** low. These are local refs only, and the bundle covers the one ref
  with unique content.

**What would change it / depends on**

- **Changes it:** you want the pre-squash messages kept inside the repo (then
  option B2).
- **Depends on:** the push, which is done.

### V25. Decode-then-write on the ingest path: close it as a direction

**The question**

An old idea says baud would read program output faster in two passes. First it
would turn UTF-8 bytes into characters (decode); then it would put the
characters into screen cells (write). Reading output into the screen is called
"ingest". The idea was written for the previous core. Should it be measured
again, or closed?

**What you would notice**

Nothing. Closing it saves weeks of risky work in the most
performance-sensitive code. Pursuing it would target a step that measures at
0-1% of the time.

**Today**

- **The proposal is baud-core2's.** CLAUDE.md lines 5532-5535 call it "a
  research-note-scale project" and say not to start it "without putting the
  choice to the owner". That core is deleted.
- **Runs of characters already work this way on the current core.** `put_run`
  (`baud-core/src/write.rs`, documented at lines 1050-1057) is "Decoded first,
  then written by whichever writer fits".
  - It decodes a run into a buffer of characters (line 1072) with a vector
    decode (`stride_three_quad`, line 1095).
  - It then picks the writer that stores eight cells at a time.
- **Single characters go through `put_one`** (line 967), which decodes one
  character in a few shifts.
- **What has been priced so far:**
  - The existing probe `probe-double-decode` (write.rs line 1026, declared at
    `baud-core/Cargo.toml` line 90) is an additive probe: a build that does
    one step twice, to price it. It covers only `put_one`'s decode.
  - `put_run`'s vector decode has no such probe.
  - The `ceiling-no-decode` arm changes the screen, so CLAUDE.md records it as
    pricing nothing.
- **The work list** asked for a re-price on the current build before any
  proposal.

**What others do**

The evidence is internal measurement (CLAUDE.md), all taken on earlier builds
of this core.

- **The new core's cost map** (line 2872): the per-character decode, done
  twice, costs 0%. The per-character write is 9.5%.
- **The ISA inventory** (line 6255): the per-character decode is 0.0-0.9%
  (cat-cjk -0.2%, vim 0.9%).
- **After the GB11 clustering work** (line 8234; joining emoji sequences into
  one character): the cost that mattered was the run gate refusing bulk
  writes, at 33.7%. The run gate is the check that lets a run of characters
  use a bulk writer. Most of that cost was recovered (+18.6%).
- **baud-core2's general form lost twice:** a class-agnostic write path, and
  two vectorised block decoders. The lesson recorded there, keep the writes
  specialised by character class, is the shape `put_run` has now.
- **Where the time goes instead:** 84% of cat-cjk's multi-byte runs are one
  character long. The lever the record keeps naming is bytes per loop
  iteration, not the decode.
- **Caveat:** both figures predate the GB11 work and the 64-byte function
  alignment (0987b98).

**Options**

- **A. Close it; no re-price now.** No cost, consistent with two measurements,
  and it removes a stale framing. The figures are not from today's exact
  build.
- **B. A housekeeping re-price, then close.** One sweep of `ingest2` (the
  in-memory ingest benchmark) with `probe-double-decode`, plus a new probe for
  `put_run`'s vector decode, with an A/A copy. About 30 minutes of machine
  time and a small probe. Almost certain to confirm A.
- **C. Start the two-pass project.** Weeks of work in the hottest code, aimed
  at a component priced at 0-1%. The general form lost twice before.

**Recommendation**

**Yes to A: close "separate decode from write" as a direction.** If you want a
current number for the record, run B when the machine is free. Reopen only if
the two decode probes together read clearly outside the A/A spread on the CJK
recordings (cat-cjk and less-cjk).

**Design**

- **For A, a doc edit:**
  - strike that work-list entry;
  - annotate CLAUDE.md lines 5532-5535 as baud-core2's proposal, superseded by
    `put_run`'s decode-then-write shape and by the 0-0.9% figures.
- **For B, if taken:**
  - Declare a new feature, *probe-double-run-decode*, beside
    `probe-double-decode` in `baud-core/Cargo.toml`.
  - In write.rs, just after the `stride_three_quad` decode, redo the decode
    into a scratch buffer and pass it to `core::hint::black_box`. That changes
    no state and no counter.
  - Price the scratch buffer on its own. That is the `probe-csi-scratch`
    lesson: a zeroed buffer can be the cost a probe reports.
  - `scripts/check-features.sh` then builds the new arm on its own.

**Tests and measurement**

- **For A:** nothing is owed.
- **For B:**
  - `ingest2` on cat-cjk, less-cjk, vim, btop and cava;
  - arms: base, each probe, and an A/A copy;
  - 5-7 rounds, the order rotated, the first round discarded, on mains, with
    rep counts sized to each recording, and every arm built at one path;
  - `cells_written` (the core's count of cells written, which proves the arms
    did the same work) byte-identical across all arms;
  - a range that overlaps the A/A copy's is no result.
- Nothing was run.

**Effort and risk**

- **Effort:** A is S. B is S.
- **Risk:** none for A, beyond closing on older figures, which B would retire.

**What would change it / depends on**

- **Changes it:** a re-price shows the decode clearly outside the A/A spread
  on CJK, or a new ingest target is set. Even then, the lever that has been
  priced is loop iterations, not the decode.
- **Depends on:** nothing else here. Option B waits for a free machine.
