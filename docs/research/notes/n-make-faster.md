# N. What baud can make faster, as a makefaster checklist

`jjcm/makefaster` is a website tool -- its metric is Lighthouse LCP and
its catalog is boot payloads, image ladders and cache headers, none of
which apply here. What transfers is the **loop**, and it is this repo's
own measurement discipline with one thing added: a **ranked checklist
walked in order**, one hypothesis per iteration, each kept only if it
beats a stated noise floor by a stated margin, or reverted completely;
then up to five hypotheses of the runner's own; then a record of which
techniques paid, by how much, ranked by how often they paid. Its keep
rule -- *beats the noise floor and moves the north-star metric by >= 5 %
or >= 20 ms* -- is the same shape as "if ranges overlap there is no
result", with a floor under what counts as a result at all.

This note is that checklist for baud. Nothing in it is done; every row
says what would be measured, what it rests on today, and what the
repo's own record says the row is worth. The catalog's own warning
applies to it: *the checklist is ranked by what worked elsewhere -- not
guaranteed per site.*

## Step 0: the metric is user-felt, and most of baud's numbers are not

Ingest throughput is the number this repo has spent most of its history
on, and it is the wrong north star for this loop: the pty delivers
**157-177 MB/s** and baud reads 89 % of that, so the parser at ~500-2000
MB/s in memory is thirty times faster than anything a user can feel.
The loop needs the metrics a person at the keyboard feels, and baud has
an instrument for some and none for others:

| metric | what a person feels | instrument today | baseline |
|---|---|---|---|
| **launch to first prompt** | opening a window | `scripts/startup-bench.py`, `BAUD_STARTUP=1` | ~100 ms to first present, ~210 ms to the prompt (150-160 ms is the shell's own rc) |
| **keystroke to pixel** | typing | **none** | unmeasured |
| **frame under churn** | btop, `cat` of a big file | `frame_gpu` (`async` arm is the app's loop) | 15-190 us CPU a frame, ~275 us GPU round trip |
| **resize to settled frame** | dragging a corner | `BAUD_DEBUG_RESIZE=1`, `delta.swift` stall detector | freeze fixed; per-step cost unmeasured as a number |
| **scroll to frame** | wheel over history | none beyond frame cost | a scrolled-back frame costs a live one |
| **idle CPU / wakeups** | battery, fan | `top` in the five-terminal comparison | lowest of five on 4 of 5 rows |

**Row 0 of the checklist is therefore an instrument, not a change:**
keystroke-to-pixel latency, measured as the app measures its own
frames -- timestamp the key event, the pty write, the first byte back
and the present it lands in -- median of many, spread as the noise
floor. Without it, half the rows below cannot be kept or reverted.

## The keep rule, translated

- Median of >= 5 interleaved rounds per arm, first discarded, an A/A
  copy in the sweep (this repo's rule; makefaster says >= 3 and no A/A,
  and this box has produced 1 % phantoms without one).
- The spread of the A/A arm is the noise floor.
- Keep only if the arms are disjoint **and** the median moves by >= 5 %
  or >= 20 ms (launch) / >= 1 frame at 120 Hz (8 ms, frames) / >= 1 ms
  (input), whichever is larger for the metric.
- Revert means revert. No half-kept arms on `master`; the compiled-arm
  convention stays for what a future re-measure needs.
- A keep that moves one metric and regresses another is not a keep.

## The checklist, ranked

Ranked by expected user-felt gain over confidence, from the repo's own
numbers. "Rests on" is the measurement the expectation comes from.

| # | technique | metric | expected | rests on |
|---|---|---|---|---|
| 0 | **Build the keystroke-to-pixel instrument** | input | enables rows 6, 9, 11 | nothing measures typing today |
| 1 | **Precompile the shaders (`.metallib`) and keep an `MTLBinaryArchive`** | launch, first run after install or OS update | ~50 ms per pipeline chain, once per install | research figure, unmeasured on baud |
| 2 | **Ship as a bundle with `CFBundleIconFile`** instead of `setApplicationIconImage` | launch (first tick) | 14-20 ms off the first display-link tick | measured: 10-13 ms of vImage on the main thread |
| 3 | **Present the first frame before the glass and the menu land** -- already done; extend to the *shell's* first bytes | first prompt | the shell's 150-160 ms is not baud's; what is baud's is the ~20 ms between the pty spawn and the run loop -- spawn earlier still | `pty spawned` at 16-19 ms |
| 4 | **Overlap `sharedApplication` with the window creation** | launch | the two are 18 + 17-21 ms serial; the research says the first titled window is the WindowServer connection, so at most one of them hides behind the other | measured stages |
| 5 | **Cut the ~600 extra dylibs AppKit loads** (`-dead_strip_dylibs`, drop the nix `libiconv`) | launch | ~1 ms measured; the 600 dylibs are the platform's, not baud's | 1,296 against 688 for a bare AppKit binary |
| 6 | **Pace the GPU by completion, never by the display link, for a keystroke** | input | a key's frame waits up to one tick (8-16 ms) today when the tick is the pacer | design D13; `pump` draws "now" on a wake only if a frame's time passed |
| 7 | **Hybrid cell path: hand only the ligature windows to the instance path** -- done for windows; the remaining cost is the miss | frame under churn | a miss is ~7 us; 17-37 misses a frame on scrolling code | window-cache counters |
| 8 | **Warm the glyph map for the face's script blocks at load** | first frames of a CJK page | 53 map misses a frame on cat-cjk's first frames, each a shape and a raster | `MAP_BITS` 14 finding |
| 9 | **Draw a key's echo before the pty round trip** (local echo prediction, Mosh-style) | input | removes the pty round trip (~5.7 us) *and* the shell's turn from the felt latency; large, risky, needs row 0 first | design; unmeasured |
| 10 | **Coalesce the shell's stale-width redraws at the app** (kitty's settle delay) | resize | the core now survives stale redraws; the *work* per drag step is N redraws x erase; a number is owed before this is taken, and the repo's rule is no timer that behaves like a debounce | note M |
| 11 | **Skip the present when nothing changed** on the cell path | idle CPU | the cell path re-walks the map every tick; a screen with no dirty rows and no blink should commit nothing | `Renderer::blink_seen` already gates blink |
| 12 | **Reflow's copy is 0.6-0.7 ms per width change** -- leave it | resize | once per width step; below the noise of a drag | measured |
| 13 | **The ring's 7 % on scrolling payloads** -- leave it | ingest | 13 L1 misses a scroll are what retaining a line costs, in any design; not user-felt behind a 177 MB/s pty | counters |
| 14 | **The parser** -- leave it | ingest | thirty times the transport; every remaining lever is priced at 0-2 % and the dead ends fill a table below | CONTRIBUTING.md and MEASUREMENTS.md |

## What not to do (this repo's own lab, in makefaster's table)

| temptation | what actually happened |
|---|---|
| a faster parser for a felt win | the pty hands back 1024 bytes a `read(2)`; 89 % of its ceiling already |
| a wider vector scan | +8 % on a synthetic, -2 to -3 % on every recording |
| a bootstrap cache image of the renderer's tables | faces map in 0.2 ms, the renderer builds in 0.4, the first prompt's frame has one map miss; nothing to cache |
| prefetching the scroll's incoming row | hides the cliff on `flat`, costs `cat` 0.62x |
| any table on a predictable branch chain | two SGR tables lost; a load beats an *unpredictable* branch only |
| a debounce on resize | the freeze was a deferred SIGWINCH, not a missing timer; per-step is correct under the contract |
| the accessory activation policy for launch | shortens the run loop's entry, after the frame |
| a `.app` bundle by itself | same 1,296 dylibs, same stages; the icon is the one thing a bundle buys |

## How the loop would run here

Not `npx makefaster` -- it drives Lighthouse and there is nothing for it
to load. The shape, by hand or as a script a session follows -- none exists
yet, and a path in backticks here is a path the docs gate requires: baseline every metric above into a `results.json`
beside `MEASUREMENTS.md`, walk rows 0-11 in order, one branch per row,
`dev-ci` green and the A/A sweep before any keep, revert otherwise, and
append the kept rows to `MEASUREMENTS.md` with their deltas -- which is
the "improvement leaderboard" for a project with one site. The five
extras are whatever the instrument from row 0 says, once it exists.

## Unmeasured, stated

**As written, 2026-09-13.** Rows 1, 4, 6, 9 and 10 rested on research
figures or design reasoning, not on a number taken on baud; each is a
measurement before it is a change. Keystroke-to-pixel had no baseline at
all. The five-terminal comparison measures CPU seconds, not latency, and
says nothing about what a person feels while typing.

**Since:** every row has its number. Rows 6, 9 and 10 were measured on
baud; row 1's research figure is replaced by one taken here
(`docs/research/notes/p-shader-cache.md`); row 4 was measured and
reverted. Keystroke-to-pixel has its baseline (`BAUD_KEYLAT=1` and
`scripts/key-latency.py`, row 0) and idle CPU has one
(`scripts/idle-cpu.py`, row 11). **Row 1 closed on 2026-09-19**, when the Metal toolchain was installed: the shaders are compiled at build time to a
committed `baud.metallib` and a cold launch went 389.9 ms to 133.5, the
cold-cache penalty from +287 ms to +34. **Every row of this checklist is
finished.**
