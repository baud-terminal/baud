# R. sfte as the second oracle

`[[source: sfte]]` is a single-file C11 terminal emulator -- 10,221 lines of
source, about the size of `baud-core` alone, doing the whole terminal in that
budget: parser, grid, reflow, fonts, a CPU renderer, Sixel and Kitty images,
XKB input, a Wayland backend, behind ~90 compile-time macros. It is pinned
whole at upstream commit `5d7ac25a4180a4adbb049ec228dc89e03dae2690` and
compiled by `baud-sfte-difftest`, which drives it and `baud-core` over the
same bytes and compares the visible grid, the cursor and the reply bytes at
every 4096-byte read boundary. Every line number below is in `sfte_dev.h`,
the readable source the amalgamation is generated from at that commit; the
parser region of the two is byte-identical.

## Why it is worth having

Deleting `baud-core2` left one instrument that can disagree with `baud-core`
from outside it, `baud-difftest` against alacritty. This is the second, and
it checks two axes that one does not: the cursor and the replies. It found
nothing in baud on its first run, and that is a result rather than a shrug:
six of nine recordings agree at every one of their boundaries -- 82, 110,
123, 8, 128 and 156 comparisons -- and the three that diverge do so for one
reason read off the differing cells, which is that sfte has no UAX #29
grapheme clustering (a ZWJ family is one cluster in two cells for baud and
four separate people in eight for sfte; a regional-indicator pair is one
flag against two; a Devanagari mark attaches in baud and takes a cell in
sfte). Those three are pinned at their counts with that reason beside them.

## What had to be true before a number meant anything

Three things, each established by compiling rather than reading, and each an
assert in the shim rather than a comment:

* **Geometry.** `sfte_resize` takes pixels and divides by the font's cell
  width (`:9586`), which is 0 with no font loaded -- undefined behaviour that
  reads 200x50 at -O2 and 1x1 at -O0. The init defaults
  (`SFTE_TERM_INIT_COLS/_ROWS`) are the one font-free route with no division,
  and `sfte_shim_new` refuses a context at any other size.
* **Locale.** sfte's width is libc `wcwidth` (`:1594`, no `#ifndef`), and
  its only `setlocale` is inside the Wayland loop a headless build compiles
  out (`:8965`). In the C locale Darwin answers -1 for every non-ASCII
  codepoint and sfte clamps it to 1. Three tables are reachable on one Mac
  (`C`, `en_US.UTF-8` with marks at 1, `C.UTF-8` with marks at 0 and emoji
  at 2); the shim sets `C.UTF-8` and the harness refuses any other answer.
* **Every boundary, not the final screen.** `git-log.raw`'s three U+2705 sit
  at 17.5% through the file and are thousands of lines gone by the end. A
  final-screen probe read "0 of 10,000 cells differ on six of nine" and said
  nothing about width; the harness compares after every read.

## The one patch

sfte's per-byte parser entry handles `'\a'` in a C0 switch that runs before
the state switch and returns in every state (`:6261-6263`), so the terminator
arms in `VT_OSC` and `VT_DCS` (`:6330`) are unreachable and `ESC ] 0 ; title
BEL` swallows every byte until the next `ESC`. That is the commonest
window-title spelling in the wild. `baud-sfte-difftest/patches/0001` guards
the arm on `VT_OSC` alone -- one condition -- and `build.rs` applies
it into `OUT_DIR` as an exact-anchor replacement that fails the build if the
anchor is found zero or several times; the pristine header stays hashed in
the manifest. The `BEL-terminated OSC` fixture is the live control: with the
patch removed, five cells and the cursor diverge. The corpus could not have
shown it -- `cava.raw`'s one such OSC swallows only its own title and agrees
at all 156 boundaries either way -- which is why the fixture exists.

The patch's first version guarded `VT_DCS` as well, and sfte's `VT_DCS` is
the state `ESC P`, `ESC _` and `ESC ^` all enter: a DCS, an APC and a PM, none
of which a BEL ends. `[[source: ctlseqs]]` gives BEL as a terminator for OSC
alone (2023-2024), and `[[source: williams]]` puts it in a DCS body (388) and
ignores it in an APC or PM (426). baud-core ended all of them on BEL too until
72e5ce0, so the two agreed in the departure; once baud followed the texts, the
patch was narrowed to the OSC, and four fixtures, a BEL inside a DCS, a
DECRQSS, an APC and a PM, hold it there. Under the old condition sfte drew the
byte after the BEL in each.

## The controls

Four fixtures must differ, each pinned at exact values rather than a count,
because a dead harness returning zeros cannot produce a specific value on
both sides. Two are sfte's HT landing one past every stop (`:6005-6008`
post-increments inside the test). One is DA: sfte answers `CSI ? 62 c`, a
VT220, where baud under-claims as a VT102 on purpose. One is the alternate
screen: sfte does not restore the cursor on `?1049 l`, where baud does.

## Normalisations, counted

Two, derived from sfte's source and pinned at exact counts over the whole
run: the second cell of a wide character (sfte stamps the lead rune again and
flags it `_SFTE_ATTR_DUMMY` at `:2851`; keyed on the flag, never on equality,
or the character would be seen twice and called a match), and the blank fold
(sfte's grid starts at 0 and its clear-cells routine writes `' '` at `:3290`,
so the two are indistinguishable within sfte -- a blindness, stated as one,
because it gives up the trailing-whitespace fidelity baud's reserved sentinel
exists to protect). The foreground is not compared; the background is
compared as default-or-set, which folds by construction and is
one-directional, since sfte's default background is the colour 0.

## The throughput row, and what a replay found

`multibench` drives sfte as its eighth core through this crate's lib, and
its sfte row carries a screen digest compared against baud-core's after one
full rep of the same replay -- the admissibility gate a throughput number
needs when one arm has no `cells_written`. Five of nine rows pass it and
are quoted; four are marked "different screen" in the row itself and are
not. The numbers and the mechanism are in `MEASUREMENTS.md`.

Two things came out of the row that the single-pass differential could not
have found. **A replay re-enters whatever the recording left open**: every
recording is truncated mid-session, `less-pager.raw` never leaves the
alternate screen, and sfte cleared the screen on a second `?1049 h` where
baud and alacritty treated it as a no-op. Two whole passes still agreed,
because the pager redraws everything; the harness's cut 1,934 bytes into a
pass was where 155 cells showed. **Since `3024b2b` baud clears on re-entry
too**, as xterm's set arm does, and that cut agrees. What differs now is the
fill: xterm and baud clear in the background in force (`ClearCurBackground`),
sfte in the default, and `btop.raw`'s second pass re-enters under a coloured
rendition -- 2,800 cells, in background only. And sfte homes the cursor on
re-entry where xterm and baud do not. All three are pinned. And **sfte's scroll-heavy rows are
85-88% damage marking, not parsing**: its grid-scroll routine marks every
cell of the whole region dirty on every one-line scroll (10,000 byte stores
at stride 36), and `patches/ceiling-no-dirty-rows.md` -- applied only when
`SFTE_CEILING=no-dirty-rows` names it at build time -- removes exactly that
call with every cell byte-identical, which this crate's tests prove by
passing under the arm. That corrected the mechanism the plan predicted:
per-character write bookkeeping is real and is second-order on those rows.

## What it cannot see

A differential is silent on everything neither side implements, and sfte's
surface is ~27-30 CSI finals against baud's ~99 sequence rows, so the silent
region is most of the protocol. "N of 9 agree" is a statement about that
intersection and must not stand as a coverage claim.

## The alternate-screen slide, read at source, and what it flashes

Read 2026-10-30 in `sfte.h` ([[source: sfte]]). `SFTE_TERM_ANIMATE_SCREEN`
(13474-13478, off by default) starts on `?1047`/`?1049` set (34085-34089,
`anim_dir = 1`) and reset (34152-34156, `anim_dir = -1`). Its offsets
(36050-36078): `progress = elapsed / SFTE_TERM_ANIM_DUR_MS` (250, 13484),
`ease = 1 - (1 - progress)^3`, `slide = (uint32_t)(ease * height)`; set,
the outgoing screen at `-slide` and the incoming at `height - slide`;
reset, `slide` and `slide - height`. The passes (36084-36166) wipe to the
background, draw the parked grid at the outgoing offset with its cursor
hidden, then the live grid over it at the incoming offset; output arriving
meanwhile is drawn in the live pass at once. No trail is drawn while it
runs (38264-38268). One line moves the outgoing screen back:
`passes[0].y_off = (int32_t)ctx->term.scroll_y_offset;` (36161), after the
two-pass block, under `SFTE_TERM_SCROLL_SMOOTH`, which defaults to 1
(13532-13533). As built, sfte holds the old screen still and slides the
new one over it, and that is what baud draws.

**At full contrast it flashes.** Counted by the WCAG 2.3.1 census
([[source: wcag22]]) on CPU frames at 60 a second, over dense screens,
as a share of the 25%-of-a-field limit on the verdict's reading:

| case | 1x, 64x20 cells | 2x, 120x40 cells |
|---|---|---|
| one slide, box drawing, dark | 30% | 103%, fails |
| one slide, rows in reverse video | 22% | 164%, fails |
| one slide, code, light | 47% | 51% |
| in and back within a second, code, light | 185%, fails | 157%, fails |
| in and back, box drawing | 145-157%, fails | 263-274%, fails |
| in, then back instantly or veiled, box drawing | 51-82% | 158-181%, fails |

Moving text crosses each pixel several times in a quarter of a second, so
holding the slide to one a second does not bound it. baud slides the new
screen in under a veil in the default background at the least alpha that
leaves none of the palette's first 16 colours, the foreground or the colour
cube's corners a transition from the background or from one another
(`faint_alpha` in `baud-render/src/transition.rs`: 38 of 255 kept on the
dark theme, 12 on the light, where the sliding screen is close to
invisible), then fades the veil off over 0.15 s. Every case above, 1x and
2x, both themes, then counts at most one flash at a pixel and 0.0% of the
limit; `slide_flashes.rs` in `baud-bench/tests` holds the 1x cases on both
readings, with truecolor reds the veil's inks do not name. Under Reduce Motion the switch is the
fade alone. The old screen is the last frame drawn before the switch,
drawn again at the switch with its cursor, its blink glow, the cursor guide
and the trail taken off, as sfte's pass hides the cursor (36151), and
unslid when a switch comes in the middle of a slide; `screen_slide.rs` holds
it on the CPU path and the pixel oracle's slide tests on both Metal paths; the
oracle drives the cell path's rebuild through its own copy of the app's,
which no app-level test reaches.
