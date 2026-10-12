# S. Sixel graphics

The spec for baud's Sixel image support, written before the code, which is
this repo's rule (`k-selection.md` and `l-scrollbar.md` are the precedent).
Every protocol claim traces to a pinned source: the DCS envelope and the
mode/DA surface to `[[source: ctlseqs]]`, and the raster grammar a decoder
implements to `[[source: vt3xx-sixel]]` -- the VT330/VT340 manual chapter
`ctlseqs` names and defers to (`xterm-ctlseqs.txt:3234`).

**Scope: Sixel only.** Kitty graphics is a separate protocol and is *not*
in this note, for one measured reason rather than as a matter of order:
Kitty's transmission formats include `f=100` PNG, which reaches a PNG +
zlib decoder over untrusted bytes, and baud-core is `no_std`, `alloc`-free
and `#![forbid(unsafe_code)]`. The shape of an image decoder under that
contract was a research-scale question the owner had not settled when this
was written. The Kitty note this paragraph asks for, `t-images.md`, takes up
the decoder seam, and the owner's answer to note V's V19 (2026-09-25) is to
build Kitty graphics to the spec; it is unbuilt (this cited the gitignored
`TODO.md` until 2026-10-10). Sixel needs *no*
external decoder -- it is a self-contained run-length format with no
compression -- so it is the tractable half and the sane first one. Nothing
here builds toward Kitty; a Kitty note settles the decoder question first.

**The design question is storage, not decoding.** Decoding a Sixel is a
stateful walk over bytes with no cleverness in it. What baud does not have,
and every other terminal takes for granted, is *somewhere to put an
image's pixels* -- baud-core allocates nothing, and a Sixel can be
arbitrarily large. That is the whole of the design, and the rest of this
note is its answer.

## The acceptance gate, before any parser code

Fix the acceptance test first, which is this repo's rule. Three gates, and
a change is not done until all three are green and pinned:

1. **Decode agreement.** A hand-authored Sixel of a known small bitmap --
   a two-colour checker, a run using `!Pn`, a `$`/`-` pair -- decodes to
   the exact `[u32]` ARGB bitmap written out by hand. This is the
   `decode_agrees.rs` shape: the decoder checked against a fixture it
   cannot argue with, at every read-chunk boundary, because a DCS body
   split across two `read(2)`s must decode identically (`chunk invariance`,
   which the control-string suite already holds for consumption and this
   extends to the decoded pixels).
2. **A golden frame digest.** One recording that carries a Sixel renders to
   a pinned FNV digest over the frame's pixels, the way
   `every_recording_paints_to_its_pinned_frame` already pins the nine text
   recordings. No such recording exists in the corpus; it is authored for
   this, small and committed, and it is the regression net.
3. **Placement across a scroll and a resize.** The two operations that move
   a cell out from under an image. An image placed, then the screen
   scrolled a line, must draw one row higher; scrolled past the top of the
   ring, it clips; a width resize that reflows the anchor line moves or
   drops it by the rule this note settles (below). Perturb each: a
   placement that ignores the scroll draws the image in the wrong row and
   the test goes red.
4. **A previewer replacing one image with the next.** The third way a cell
   moves out from under an image, and the one that shipped broken: a file
   manager (yazi) draws a Sixel, then positions the cursor back at the
   image's cell and **overwrites those cells with spaces** before drawing
   the next file's Sixel -- it does not use `ED`. Nothing freed an image on
   overwrite and `unplace` was dead code, so every preview added an image
   and a placement at the one cell until the 32-slot store filled and every
   later preview was refused; in the meantime the stale transparent-
   background images stacked at one position and composited into blocky
   garbage -- the scramble a previewer showed in baud, captured from yazi's
   real bytes and reproduced. The fix is that **writing new content over an
   image's cells frees it** -- the standard "text over a Sixel erases it",
   which is what a previewer relies on. `Term::set_cell_size` gives the core
   the cell width beside the height so it knows an image spans
   `ceil(w/cell_w)` by `ceil(h/cell_h)` cells; `ImageStore::overwrite` frees
   any placement a write's row and columns land on, gated on an O(1) live
   count so a stream with no image pays one predictable branch. It hooks
   overwrite, not `ED`, so the store's "survive one erase" behaviour stands.
   The decoder was never at fault: baud decodes yazi's real Sixel pixel-
   identically to libsixel's `sixel2png` (the `sixel_oracle.rs` gate), and
   the CPU and GPU display paths render it identically to the oracle -- the
   bug was entirely the placement leak.

The pixel oracle (`the_cell_arm_draws_what_the_instance_arm_draws`) must
also cover a placed image, because a Sixel drawn on one GPU arm and not the
other is exactly what that gate exists to catch -- the same requirement the
colon underlines and box drawing carried.

**Where the four stand (2026-09-22).** Gate 1 is `sixel_decode.rs`'s
hand-written fixtures, `sixel_oracle.rs` against libsixel, and
`a_sixel_is_chunk_invariant` for the read boundaries. Gate 2 is
`a_sixel_paints_to_its_pinned_frame`, which pins `swirl.raw`'s frame at home
after asserting the swirl drew. Gate 3's scroll half is
`a_sixel_scrolled_one_line_draws_one_row_higher` and
`a_sixel_scrolls_off_and_the_frame_changes`. Gate 4 is
`a_previewer_replacing_images_does_not_accumulate_them`. Gate 3's resize
half is `baud-core/tests/image_anchor.rs`, below. The scroll test
was perturbed to an image that moves two rows a scroll and fails alone; the
digest was perturbed to a CPU blit one pixel low, which the painted-pixel
count passes and the digest does not.

## The grammar

The envelope, from `[[source: ctlseqs]]`:

```
DCS Pa ; Pb ; Ph  q   <sixel data>   ST
```

`Pa` pixel aspect ratio, `Pb` background-colour option, `Ph` horizontal grid
size (xterm ignores it). **That `Ph` is not the raster command's `Ph`** below:
`[[source: ctlseqs]]` names the envelope's third parameter `Ph`, the VT3xx
chapter calls it `P3` and gives `Ph` to the raster command's horizontal
extent, and `[[source: dec-std-070]]` 070-9 calls the two Horizontal Grid Size
(§7.3) and Horizontal Extent (§8.1.2). Where this note and the code say the
grid size is dropped, they mean the envelope's. The `q` final with numeric
parameters and **no `$` intermediate** is what distinguishes a Sixel from
DECRQSS (`DCS $ q`, `[[source: ctlseqs]]`), which baud already acts on -- the
graph carries `Node::Dcs` and `Node::DcsDollar` (`graph.rs:280`), so the Sixel
hook is a `Node::Dcs` final `q` reached without passing through `DcsDollar`.
Every other DCS body is consumed and dropped today, so a Sixel is consumed and
dropped today; this note makes it decode instead.

The raster grammar, from `[[source: vt3xx-sixel]]`, is the part a decoder
implements:

| token | meaning |
|---|---|
| `?`..`~` (`0x3F`..`0x7E`) | a data byte: six vertical pixels, the low six bits of `value - 0x3F`, LSB at the top |
| `#Pc;Pu;Px;Py;Pz` | define colour register `Pc`: `Pu` = 1 HLS (`Px` hue 0-360, `Py` lightness, `Pz` saturation), 2 RGB (`Px;Py;Pz` intensities 0-100%) |
| `#Pc` | select colour register `Pc` for subsequent data |
| `!Pn` | repeat the *next* data byte `Pn` times |
| `$` | graphics carriage return: active position to the left, same sixel row |
| `-` | graphics new line: active position to the left of the next sixel row (down six pixels) |
| `"Pan;Pad;Ph;Pv` | raster attributes: aspect ratio `Pan/Pad`, and an extent `Ph`x`Pv` in grid pixels that "do not limit the size of the image" -- the rectangle a `P2` of 0 or 2 erases, and a summary of the size (below) |

A Sixel builds a paletted bitmap: 256 colour registers, each a `u32` ARGB
once resolved, and the data bytes paint six-pixel columns advancing the
active position rightward, `$` returning it and `-` dropping six rows. RGB
percentages scale to 0-255; HLS is converted once at register-define time
so the hot decode never does trigonometry.

## What is built: the decoder and its gate

The decode half of the first acceptance gate is landed as
`baud-domain/src/sixel.rs` -- pure, `no_std`, allocation-free, painting into a
caller's `SixelCanvas` over `&mut [ColorWord]` where `DEFAULT` is a transparent
unpainted pixel. It decodes the data bytes, the `#` colour registers, the `!`
repeat introducer, `$`/`-`, and the `"` raster attributes, and returns a
`SixelReport` of the used extent and two **refuse-and-count** figures. Nothing
of the storage, placement, render or parser-hook slices is built yet.

`baud-domain/tests/sixel_decode.rs` is the decode-agreement gate: ten fixtures
-- nine hand-authored, each token cited to `[[source: vt3xx-sixel]]`, plus one a
real Sixel emitted by ImageMagick (a red 4x4 with a green and a blue point),
which proves the decoder handles a mainstream encoder's output and not only
hand-crafted bytes, the "use real recordings" discipline for the gate. All
decode to the exact pixels the grammar prescribes. It was perturbed -- flipping
the
six-pixel bit order to most-significant-at-the-top -- and the discriminating
test failed, **confirmed against a forced rebuild**
(`rm -f target/debug/deps/sixel_decode-*`): cargo's fingerprint races a rapid
edit-and-test and reports the stale binary, so the intermediate red runs were
the old binary, not the code. The next person re-running the perturbation will
hit the same race.

**The report's extent is the data's reach, not the ink's.** `width`/`height`
count the columns the cursor advanced through and the bands a data byte
occupied, so a trailing `?` (all bits clear) still widens the image -- because
`[[source: ctlseqs]]` mode 8452 places the next text past the extent, and an
image that reported a short extent would be written over. The extent is
independent of the canvas; `refused` is what did not fit.

~~**RGB (`Pu = 2`) is implemented; HLS (`Pu = 1`) and any unrecognised
coordinate system are counted, not applied.**~~ **HLS is applied since
2026-10-11.** The chapter this paragraph waited on is pinned,
`[[source: vt3xx-screen-control]]`: "Blue: 0 degrees Red: 120 degrees Green:
240 degrees". The conversion is the double hexcone
`[[source: css-color-4-hsl]]` prints, turned 120 degrees, and it reproduces
that chapter's Table 2-3 within one percent on all sixteen rows; an HLS value
past its range ignores the whole command, `Pc` included, as
`[[source: dec-std-070]]` 070-9 §8.1.3 says. An unrecognised coordinate
system is still counted in `unapplied_registers`. What follows is the
reasoning that held HLS back. The source gives the HLS hue as
"0 to 360 degrees" and defers the origin to a chapter that is not pinned, and
DEC's HLS does not put red at 0 -- so a converted register would be a guess,
and "never record a mode you do not act on" says to count it
(`SixelReport::unapplied_registers`) rather than paint a wrong colour. Pinning
the VT330/VT340 manual's chapter 2 "Output Mapping" and implementing the
conversion is the follow-on. `!0` (a zero repeat, which the source does not
define) is treated as one ~~and flagged in the source as a guess~~: since
2026-10-11 that is sourced, `[[source: dec-std-070]]` 070-9 §8.1.1, "A
repeat count of zero implies a repeat count of 1" (PDF p. 920; note AN,
section 8), and `a_zero_repeat_draws_one_column` holds it.

**The pixel aspect ratio is honoured since 2026-10-11** where the two DEC
texts agree (note AN's item 8): `P1` omitted, 0, 1 or 5 is 2:1, 3 is 3:1 and
9 is 1:1, and a whole raster `Pan:Pad` before any data overrides it, so a
sixel row is that many pixels tall and the extent, the arena reservation and
the cursor's advance scale with it. Both Sixel streams in the tree send
`"1;1` and do not move. A ratio that is not whole, and the envelope's grid
size (`P3`, which ctlseqs calls `Ph`), are for Jev (the record, "A Sixel pixel
is drawn at its aspect ratio where both DEC texts agree"). For the first, a
VT340's own rows are practice beside note AN's item 12: vt340test gives `P1` 2
as 5 VT340 pixels, 4 as 3, 6 as 2 and 7 and 8 as 1
(`[[source: vt340test-physicalsixels]]` 223-229), the VT3xx column rounded,
alternative c, on the terminal that manual describes.

**The raster extent `Ph`x`Pv` has two jobs and baud does neither yet,
because both wait on the fill.** `[[source: vt3xx-sixel]]`: "Ph and Pv do not
limit the size of the image defined by the sixel data. However, Ph and Pv let
you omit background sixel data from the image definition and still have a
color background. They also provide a concise way for the application or
terminal to encode the size of an image" (298-301). 070-9 §8.1.2 says the
same of each, the device "will NOT clip the image" to it (PDF pp. 920-921).
The first job is the region a `P2` of 0 or 2 erases, the open question at the
end of this note. The second, a size known before the data, is what would
reserve the arena before the body is read, which is the streaming decoder's
change and not this one. Under `P2 = 1` neither job draws anything: no
position outside the data is set, and the extent clips nothing. And it is
not folded into the image's height, which the cursor's advance reads:
070-9 §11.3 moves the text row by "the vertical motion commands executed in
the Sixel printing" (PDF p. 928), not by `Pv`. Its unit is the grid's, not
the screen's: "The horizontal units are specified by the ANSI **SSU**
sequence and the vertical by the aspect ratio"
(`[[source: vt340test-physicalsixels]]` 193-195), so a `Pv` at 2:1 is twice
as many device rows.

## What is also built: the storage value types

The design below is landed as `baud-domain/src/image.rs` -- `ImageStore`, the
`ClusterStore` pattern a fourth time: a caller-provided ARGB arena
(`&mut [ColorWord]`), an image-slot table and a placement table, all
**refuse-and-count** with public counters (`images_refused`,
`placements_refused`, `images_dropped`, and since 2026-10-09 `refusals`,
`images_refused` split by the limit each refusal hit). `store` reserves a
`w`x`h` span and returns the slice to **decode into in place** -- so a Sixel
lands where a renderer will read it rather than in a scratch that is then
copied -- `place` adds an absolute-line/column/`z` placement and refcounts
the image, and the last placement leaving (or an explicit `free`, the Kitty
delete) returns the span. Reclaim is inherent: free space is the gaps between live spans, recomputed
per allocation, so a free is a slot cleared and nothing more, correct for every
free order rather than a stack. `docs/research/notes/t-images.md` records why
ARGB and not palette indices, and why the gap-scan over a free list.

`baud-domain/tests/image_store.rs` is the gate: ten fixtures. Nine check the
mechanics with synthetic images written distinct sentinels -- so an overlapping
allocation corrupts one and the test sees it -- covering non-overlap, the three
refuse-and-count axes, free-on-last-placement, `free`, `reset`, and gap reuse
proven by a full arena that still stores after a middle image is freed. The
tenth decodes the **real** ImageMagick capture below into the store in place.
Perturbed -- an allocator that ignores gaps and always returns offset 0 -- the
non-overlap and gap-reuse tests fail, confirmed against a forced rebuild.

**Wired to the parser and to `Storage`.** `Storage` now carries an
`ImageStore` and a `sixel_body` scratch (`baud-core/src/storage.rs`), and a
bare-`q` DCS routes to it: `Act::SixelStart` (a `q` from `Node::Dcs` reached
after numeric parameters `30-3B`, not after an intermediate -- so `DCS + q`
and `DCS $ q` stay XTGETTCAP and DECRQSS) renames the string kind, the body
accumulates in `sixel_body` across reads through the same `StrPut`/`StrEnd`
path an OSC body uses, and at the terminator `Paint::sixel_end`
(`baud-core/src/sixel.rs`) measures the body with `Sixel::extent`, reserves
the span, decodes in place and places the image at the cursor's absolute line
and column. `Term::images()` exposes the store; `RIS` resets it.

The gate is `baud-core/tests/sixel.rs`: the real `swirl.raw` decodes to one
64x48 placement, **chunk-invariant** across read sizes 1 through the whole file
(the body spans reads, which is the case no recording could show and the one a
reset-at-boundary parser breaks only in production); an unprovisioned store
refuses and counts; a body larger than the scratch is refused whole; and the
text after a Sixel is drawn, not swallowed. Recognising the bare-`q` DCS moved
the stride-k class ladder (958 -> 967 at stride 2, and up), which
`the_alphabet_reduces_as_the_stride_doubles` re-pins.

**Provisioned by the caller, defaulting absent.** `Owned::with_images(budget)`
sizes the pixel arena and the body scratch (an `ImageBudget`, one typed pair
rather than two raw sizes); `Owned::new` provisions neither, so a default
terminal recognises a Sixel and refuses it, counted. The size is the owner's
call the mechanism does not depend on -- a 64x48 image is 3072 `ColorWord`s, a
full screen ~1.4M.

**All three paths draw it, and the placement math is written once.**
`Renderer::set_images` computes each visible placement's screen rect --
`line - top` in cells, with `top` from `Term::image_origin(back)`, placements
on the other screen and off-screen ones skipped -- into one list
the CPU compositor and both GPU arms read, so three paths cannot disagree about
where an image goes (which the pixel oracle could not otherwise tell from a blit
bug).

* **CPU.** `Target::blit_image` blits the ARGB over the frame the way a
  `Kind::Color` glyph composites -- `blend.over`, opaque, a `DEFAULT` pixel
  transparent by omission -- **on a full frame only** (an incremental frame
  paints only its dirty rows, so per-placement damage is the follow-on). Gate:
  `baud-render/tests/image.rs`, the real `swirl.raw` into its 64x48 rect (pixels
  before any digest), scrolled off as a control.
* **GPU, both arms.** The collapse was that `glyph`/`Kind::Color` already draws
  a premultiplied-OVER textured quad, so an image is **one draw call through the
  existing pipeline -- no shader change, no `.metallib` rebuild**: its own
  texture (not the shared atlas -- an image is not glyph-sized) bound at fragment
  1, and a uniform whose `color` floats are the image size so the quad's uv spans
  it. The texture is uploaded once, keyed on `(ImageGen, ImageId)` and dropped
  whole when the store's epoch moves, so a slot reused after a `reset` cannot
  return a stale texture. It draws last, over the glyphs and records, matching
  the CPU path, and slides with the grid under smooth scroll (cell path) -- an
  image wholly inside the leaving row included, which `set_images`' cull
  dropped until it kept the row above the window. Gate:
  the pixel oracle's scene gains a homed Sixel, both arms must agree *and* it
  must change its rect against the same frame with no image -- perturbing either
  the cell-path draw (arms disagree) or the whole pass (rect unchanged) fires it.
  For the slide, a one-band Sixel mid-screen and in the leaving row: a
  full-row slide must be the screen one line up, drawn with no slide shader.
  Dropping the pan fires the first (96 pixels), the old cull the second (48).
* **The app provisions the arena** (`host::IMAGE_BUDGET`, 4 MiB pixels / 256 KiB
  body) on every path, so a Sixel is stored and shown rather than parsed and
  silently refused. The default is overridable through `[images] pixels_mb`
  and `[images] body_kb` -- two knobs and not one, because the decoded-pixel
  and raw-body limits are independent and a single knob scaling both would
  rest on an unmeasured guess about Sixel encoding density; a large image
  needs both raised. ~~The resolved budget is named on a startup line so a
  refusal is diagnosable without a rebuild.~~ **It was not diagnosable**,
  found 2026-10-09 when yazi's previews went blank: the startup line is
  `info`, which the default `warn` hides, and a refusal was one total --
  body, slot and arena together -- that nothing in the app read. The store
  now counts each refusal by its cause (`ImageStore::refusals`: the body
  with its true length, the arena with the image's size, the slot table, an
  image with no pixels), and the app logs one `warn` line a pump naming the
  limit, the sizes and the `[images]` key that raises it
  (`host::describe_refusals`). **It goes to stderr**, where all of
  baud-app's log goes: a launch from a terminal shows it, and a Dock launch,
  whose stderr is discarded, does not.

Two costs are named rather than hidden. **Only the GPU cell path slides images**
(with `pan`, so an anchored image moves with its line under smooth scroll); the
CPU compositor does not, and that is not a divergence in the three-paths-agree
invariant, because the CPU/full-frame `render()` slides *nothing* -- smooth
scroll is a cell-path feature the Linux and instance paths do not have either.
And **`set_images` snapshots each referenced image's pixels once**, not every
frame. The snapshot -- a `Vec<u32>` and a per-pixel `as_rgb` conversion of the
whole image -- is kept across frames and rebuilt only for a new `(epoch, id)`,
the same key the GPU texture cache uses, dropped whole when the store epoch
moves. So a static image on screen costs one snapshot however many frames it
draws, where the first version rebuilt it every frame: free on the corpus
(`64x48` is nothing) and a saving that grows with the image -- a `Vec<u32>`
and an `as_rgb` conversion of every pixel a frame, so hundreds of KB and a few
hundred thousand conversions for a mid-sized image, up to the arena's ~1M-pixel
ceiling. `Renderer::image_snapshots` counts snapshots built, and
`a_static_image_is_snapshotted_once_not_per_frame` gates it at one over five
frames (perturbed to a clear-every-frame renderer, which reads five); it is
unmeasured on the corpus because the corpus has no image large enough to show
the cost, and the counter is the evidence rather than a throughput sweep.

**And the cache is pruned, not just kept.** An image scrolled off screen is
dropped from the cache the next `set_images` rather than held until the epoch
changes -- the clear-every-frame version did this by accident, and keeping the
cache across frames would otherwise hold a large image's pixels forever on a
terminal showing one. The same test asserts the cache empties when the images
leave the screen (a build counter is blind to a leak, so the observable is the
cache length). An image that oscillates on and off re-snapshots on each
re-entry, which is the price of not holding it and far cheaper than a rebuild a
frame.

**The cursor advances past the image, in the core.** After placing, `sixel_end`
moves the cursor down `ceil(Pv / cell_h)` rows through the **existing scroll
path** (`Paint::index`), so an image at the bottom of the screen scrolls and
the window's top line moves with it -- which keeps the *next* image's anchor right,
and which assigning `cy` directly (the tempting shortcut) would not. The core
lacks only the cell height in pixels, which the renderer owns; the embedder
supplies it through `Term::set_cell_size`, re-set on every `rescale` (a stale
height mis-advances every image after a zoom, the geometry trap this repo has
hit before). A caller that never sets it -- a headless test, a bench -- leaves
it zero and the cursor unmoved, today's behaviour, which the sentinel gate
pins. The advance is settled in the core rather than the renderer for two
reasons a renderer-side advance gives up: the core owns cursor state, and the
advance must happen per **read** (`DCS ... ST` completes per read) not per
frame, or `cat a.six b.six` in one read would advance once. `[[source:
vt3xx-sixel]]`'s **mode 8452** -- the cursor to the *right* of the image rather
than below -- is not implemented and falls through to the unsupported-mode
counter. Gates in `baud-core/tests/sixel.rs`: the below-advance, the
bottom-of-screen scroll (which pins the scroll-path route, perturbed to a `cy`
assignment), the column staying at the image's left (`CUP` to a non-zero column
first, so an unchanged column is a decision and not an accident -- ~~the DEC
source for the exact column is unread and the test pins the choice~~ since
2026-10-11 sourced to `[[source: dec-std-070]]` 070-9 §11.3, "The Horizontal
Active position of the ANSI text is not affected by Sixel mode and set
exactly to what is was when Sixel mode was entered", PDF p. 928), the
sentinel, and chunk invariance.

**A second DEC text reads the other way, and a VT340 sides with 070-9.**
`[[source: vt3xx-sixel]]`'s "Sixel Scrolling Enabled": "When sixel mode is
exited, the text cursor is set to the current sixel cursor position" (441-444),
which read literally is the column after the last sixel drawn, not the one the
image began in. On the terminal that manual describes, vt340test's script
states the column is kept: "the text cursor is moved to the row of the last
sixel cursor position, but the column stays the same as it was before the
sixel image was sent" (`[[source: vt340test-textcursor-sh]]` 5-7), and its
capture (`[[source: vt340test-textcursor-png]]`) shows three images sent one
after another with a `CUF 1` between each, every one starting a column right
of where the last one started rather than right of its edge. So baud's column
conforms to 070-9, the standard, and to a VT340, and departs from that
sentence of the VT330/VT340 manual, which binds the VT300 rather than baud by
note AN's reading (section 8). It is recorded as a conflict between the texts
and not changed; the record, "A Sixel's background fill waits on Jev, and the
column has a second text", lists the options.

**The `Term` field and the `Paint::new` argument cost nothing, measured.** They
are per-`read(2)`, not per-byte -- `Paint::new` is called once a feed, not once
a character -- so unlike the `last_cp`-in-`Cold` case this repo prices at 16%,
they sit beneath the ingest floor by construction. Swept on `cat-cjk`, 40 reps,
five interleaved rounds with the first discarded: HEAD read ~2% below its parent
with HEAD first and at or above it with the order reversed, inside a ~2% A/A
spread of the same binary against itself -- a null, the ordering artefact this
repo records, not a regression.

**The anchor follows its row.** `sixel_end` records the line as `top + cy`,
where `top` is the absolute line at the top of the live window in that
screen's numbering -- `Book::top` on the primary, `Book::alt_top` on the
alternate screen -- and the placement carries which screen it is on. `top`
moves with the window both ways: up one for every row a scroll retires, down
one for every row a reverse scroll or a taller screen takes back, and a reflow
renumbers it with the rows. `Term::image_origin(back)` hands the renderer the
top of a view `back` lines up with the clamp `Term::view` uses, so an image
and the grid row under it are drawn from one answer. This replaced a monotonic
`scrolled` count that a shrink retiring rows never moved and a reverse scroll
never moved back: both drifted an image off its row, and a view scrolled back
after a shrink put it thirty rows off. `baud-core/tests/sixel.rs` pins the
count with `the_windows_top_line_matches_the_rotation_deltas` and
`two_sixels_past_a_full_ring_get_distinct_anchors`, and
`baud-core/tests/image_anchor.rs` pins the rows by their text -- see "What a
resize does to an image" below.

## The Sixel capture, in the corpus

`tests/recordings/sixel/swirl.raw` is a real 64x48 Sixel from ImageMagick
(`magick -size 64x48 gradient:blue-yellow -swirl 180 sixel:-`) -- 5458 bytes,
`"1;1;64;48` raster attributes, 233 RGB register defines, 184 `!` repeats and
eight six-pixel bands, all RGB. It is the golden-frame gate's payload (`s-sixel`
gate 2) and the storage gate's real input.
Recordings are enumerated by explicit lists (`golden.rs`, `support/mod.rs`), not
by a directory scan, so a `sixel/` capture joins a gate only when its name is
added to that gate -- it does not disturb the text corpus.

## Storage: a caller-provided image arena, refuse and count

This is the settled design and it is the `ClusterStore` / `LinkId` / `tmpl`
pattern a fourth time -- bounded by construction, **refuse rather than
evict, and count the refusal**. An evicted image is a wrong picture on
screen, the same argument that made the cluster store refuse a 64th
distinct cluster rather than drop somebody's accent.

Three bounded structures, all caller-owned on `Storage` (which already owns
the grid, the arena, the reply buffer -- baud-core allocates none of them):

* **An image pixel arena**, `[u32]`, caller-provided, holding decoded ARGB.
  A decode that would exceed it is refused whole (`images_refused` and
  `refusals.arena`, with the image's size), never truncated -- half an image
  is a wrong image. ~~The raster attributes
  `Ph`x`Pv` bound a decode before it starts, and an image with no raster
  attributes is bounded by the arena's free space and refused if it fills
  it.~~ What bounds a decode is the walk's own extent (`Sixel::extent`), the
  same with raster attributes or without: both DEC texts say `Ph`x`Pv` "do not
  limit the size of the image", so a decode sized from them would be cut
  wherever a sender's data ran past them. **Size is the owner's call**,
  flagged below; the mechanism does not depend on the number.
* **A placement table**, bounded, holding `{ image slot, anchor, z }`.
  Screen-space with its own storage, *not* a per-cell reference, because
  `CellFlags` is full at 16 bits with zero spare -- the note the images
  entry in the plan already carries. Kitty's own model is placements with a
  z-index, which is exactly this and avoids the cell-bits problem by
  construction.
* **An image slot table** mapping a live image to its span in the arena,
  freed when its last placement is gone, so the arena is a bump-or-reuse
  allocator over caller memory rather than a heap. A slot taken once for
  the arena's life, a distinct image refused past capacity, counted.

Reset on `RIS` alone, not on the alternate screen or a clear -- an image is
content a program may redraw over, and a placement outliving one erase is
the correct behaviour, the same call the link table makes.

## Placement: absolute line, column, z-index

An anchor is an **absolute line of one screen** plus a column. A Sixel is
placed at the cursor's cell when its `ST` arrives, and the anchor records
the line that cell is on: `top + cursor row`, where `top` is the line at the
top of that screen's window (`Book::top`, `Book::alt_top`). So:

* **Scroll is free.** The image draws at `line - top` rows from the top of
  the view, with `top` from `Term::image_origin(back)`. Output scrolling
  under it moves `top` and not the anchor, so the image moves up with its
  line and nothing re-registers.
* **Off the top clips**, and when its anchor row leaves what the ring
  retains the placement is dropped and counted in `placements_expired`.
* **The live view and the scrollback view** both draw it, because both are
  a rotation of the ring the anchor is expressed against -- the same
  property that made the scrollbar and selection survive `Term::view(back)`.

### What a resize does to an image

The owner's call, 2026-09-22: **an image follows the row it was placed on**,
foot's model and ghostty's. A survey of eight terminals, read in source and
adversarially checked, found four models -- follow the row (foot, ghostty),
keep the screen position and let text reflow under it (kitty), cut the image
into per-cell slices that reflow with the text (WezTerm), and let the
application redraw (xterm, mintty and the rest) -- and following the row is
the one where the picture stays beside the text it was printed with.

* **The window's top line moves with the window.** A shorter screen retires
  rows off the top and `top` moves on by as many; a taller one pulls them
  back and `top` moves back. The first implementation numbered placements by
  a monotonic count of scrolls, which neither moved, so a shrink slid every
  image down by the rows it retired and a view scrolled back after one put
  it thirty rows off. `the_windows_top_line_matches_the_rotation_deltas`
  pins the count to the rotation, both ways.
* **A reflow maps each anchor through the re-cut.** `View::reflow` takes a
  hook (`grid::Follow`) it calls once per line it keeps, before copying the
  line, with where each of its rows lands; the store stages every primary
  placement at its row from the oldest retained line, maps the ones on each
  line as it is visited, and writes them back against the same origin. A
  row lands where its first cell lands. The column is kept, so a column past
  the new edge is cropped where it is drawn rather than moved. An anchor at
  a line's start -- every image printed after a newline -- round-trips
  exactly, and widening back brings it whole. ~~An anchor on a wrapped
  continuation row does not: a round trip brings it back a row higher,
  because the first cell of the row it landed on belongs to the row above
  at the old width.~~ An anchor on a wrapped continuation row round-trips
  too, since 2026-10-11: the placement keeps where in its row the cell its
  row began with landed, and the next reflow asks by that cell rather than
  by the row's own first (docs/record.md, "An image on a continuation row
  follows the cell its row began with"). A line the reflow drops takes its
  image with it.
* **An image goes when its row does**: past the far end of the history, cut
  below a shorter window past what the ring keeps, blanked as a reverse
  scroll takes the newest history line back, or dropped by a reflow. Each is
  counted. Without this a placement in the history never left the 64-slot
  table, and once one was in it every text run and every scroll after it
  scanned the table.
* **Each screen owns its images.** A placement records its screen; the
  renderer draws only the live screen's; leaving the alternate screen
  deletes its images, which foot and ghostty both do.

**What it is gated by**: `baud-core/tests/image_anchor.rs`, where every
assertion is the text beside the image -- `MARK` on the row above, the prompt
on the row after its last -- because a reflow renumbers every row and a
line-number assertion can hold with the image on the wrong row. foot's trap
(an image after a blank line, which foot maps a row high) and WezTerm's (an
image on a row that also holds text, which WezTerm truncates) are pinned by
name, with a drag through forty sizes, an image in the history narrowed and
widened, a view asked for past the history, an image above a prompt block
the shell redraws, and ~~the continuation-row drift pinned as what it is~~
an anchor on a continuation row held to its row through any path of widths.
Eleven mechanisms were perturbed on their own, and each fails its own test --
two of them only after a test written for it, because the first cases could
not tell the mechanism apart (the view clamp at a history whose oldest line
was zero) or never reached it (no image test sent the shell's prompt marks).

**What it costs, measured** (`ingest2`, six rounds interleaved, first
discarded, A/A in the sweep, on mains, `cells_written` identical on every
arm): nothing on a stream with no image -- flat, lines, cat, less and btop
all overlap. And on `sixel-cat`, the committed Sixel followed by
`cat-scroll.raw`:

| arm | MB/s |
|---|---|
| before | 789.0 [774.5-792.0] |
| **after** | **1408.3** [1337.3-1447.0] |
| A/A copy of after | 1397.4 [1368.5-1427.5] |
| after, without the line bound | 1034.4 [1007.4-1057.3] |

The "before" arm kept its first image placed forever and scanned all 64
slots on every text run. In time a byte the expiry and a per-screen bound on
the lines placements lie between (`Reach`) share the win about equally --
1.27 ms a MB before, 0.97 with the expiry alone, 0.71 with both -- and the
bound is what lets the text-over-image check and the expiry answer from two
compares when the rows being written are past every image.

The arms do not do equal work, and the difference runs against "before": its
arena holds one image and nothing frees it, so it stores the first and
refuses the other 59, decoding and storing less than the arms it loses to.
The no-bound arm's figure is a whole-rep average -- each rep's image expires
partway through the rep -- so the cost while an image is present is larger
than it shows, and is not measured on its own.

**Rows that move inside the window move their images too.** A scrolling
region, `IL` and `DL` move rows without moving the window's top line, so
the anchor has to be moved by name: `ImageStore::scroll_band` shifts the
placements on the band's lines with the rows, and drops, counted, one whose
row left the band -- deleted off the top of a region that copies, or
pushed past its bottom. A region anchored at the top with fewer rows under
it than in it scrolls by rotating the whole grid and copying the rows under
it back, so the band there is the rows under the region, moved back down
one a scroll, and the line leaving its top is retired to the history with
its image; a shorter top-anchored region copies instead, and that line is
deleted with its image. Before, an image on a
status line under `DECSTBM 1 ; 49` walked up into the region a row a
scroll while its text stayed put, and a middle region copied its text and
left its images behind. No recording carries an image in a region; the
gates are fixtures in `baud-core/tests/image_anchor.rs`, the HT and SO/SI
precedent for what the corpus cannot contain, each red before the fix and
each of the band's mechanisms perturbed to fail its own. Every call site's
bounds were moved a row each way too, twenty perturbations: nineteen fail a
test, and the one that does not moves only a line below the window.

Z-index orders overlapping placements; a later image at the same cell draws
over an earlier one. Text is drawn, then images by ascending z, so an image
is over the cells it covers -- which is the Kitty convention and what a
program placing an image expects.

**The cursor after a Sixel** is a real interaction with the grid, not a
render detail. `[[source: ctlseqs]]` mode 8452 (sixel scrolling) leaves the
cursor to the right of the image; without it the cursor returns to the
image's left column on the line below. The design was to follow the mode;
what shipped is the default only, and 8452 falls through to the
unsupported-mode counter, as the section on the cursor advance above says --
a mode recorded and never read being the failure `docs/vt-coverage.md` rates
worst. The image occupies `ceil(Pv / cell_h)` rows and the grid
scrolls to make room the way `LF` past the bottom does, so a tall image at
the bottom of the screen pushes lines into scrollback and its own anchor
scrolls with them.

## Rendering: ARGB, its own region, all three paths

The renderer already rasterises `sbix`, `CBDT` and `COLR` glyphs into an
`ARGB` atlas sheet (`atlas.rs:1`, `Kind::Color` = "an ARGB image, painted
as it is; the instance's colour is unused"). A Sixel is ARGB pixels, so it
draws through the same `Kind::Color` path -- **but an image is not
glyph-sized**, so it does not go in the shared glyph atlas (a single Sixel
can be larger than the whole atlas). An image gets its own texture region,
uploaded once when decoded and referenced by the placement; the atlas's
generation-and-dirty upload machinery (`Atlas::take_dirty`) is the
precedent for uploading only what changed.

All three render paths carry it, because the design's whole point is that a
placement is grid-anchored data the paths read, not a fourth drawing
mechanism:

* **CPU compositor**: blit the region's ARGB over the cells the placement
  covers, after the glyphs, respecting z.
* **Instance path**: one textured quad per visible placement, chained after
  the cell quads the way the scrollbar and cursor already chain.
* **Cell path**: the same quad appended to the records the cell shader
  draws after its passes, the overlay-record precedent.

Damage is the rows a placement covers, dirtied when the placement moves
(a scroll) or its image changes -- the `bar_moved` / blink precedent, so an
image on a still screen costs nothing per frame and a scroll repaints only
the rows it spans.

## The vt-coverage row

`docs/vt-coverage.md`'s Sixel row is rewritten in the commit that starts
this work, per the plan. Its old reason was two clauses -- "an image
protocol, not a terminal protocol", which stands as a *classification*, and
"does not interact with the ingest architecture baud exists to test", which
was written when baud was a headless VT core and has expired: the renderer
now has a CPU compositor, two GPU paths and an ARGB atlas an image protocol
interacts with directly. The row moves from `out` to `todo`/`partial` as
the work lands, `done` only when the three gates above exist and would fail
if the operation regressed.

## Owner's calls, flagged not settled

* ~~**The image arena size.**~~ **Taken.** How much caller memory Sixel gets
  is `[images] pixels_mb` (decoded ARGB) and `[images] body_kb` (raw-Sixel
  scratch), defaulting to `host::IMAGE_BUDGET`'s 4 MiB / 256 KiB -- the
  owner's number, kept, with the knobs added around it rather than the
  default retuned. Two knobs because the two limits are independent: a
  1024x1024 image is 4 MiB decoded whatever its Sixel encodes to, and its
  raw body varies with colour count and run-length use, so a single knob
  scaling both would rest on an unmeasured density guess. ~~The resolved
  budget is logged at startup, so a reader who raises one knob and still
  hits a refusal can see which limit bit them.~~ The startup line is
  `info`, hidden at the default `warn`, and named only the budget; what
  says which limit bit is the refusal's own `warn` line
  (`host::describe_refusals`, 2026-10-09), on stderr, which a Dock launch
  discards.
* **The body default against a previewer -- open.** yazi 26.5.6, run
  headless against this budget (cell 17x34, Sixel advertised), caps a
  preview at 600x900 pixels and writes 1 to 4.6 bytes a pixel. Its small,
  flat and sparse previews (10-47 KiB of body) store; a UI screenshot
  (334-390 KiB at 600x375), a photo (474-680 KiB at 600x800) and a page of a
  text PDF (797-820 KiB at 600x776) are refused against the 256 KiB body
  though each fits the 4 MiB arena, and the pane is blank because yazi
  erases the old preview first. Raising `body_kb` alone (to 4 MiB) stored
  every one. Whether to raise the default or decode as the body streams, so
  no raw body is buffered, is the owner's call; nothing is changed here.
* ~~**Reflow.**~~ **Taken, 2026-09-22: the image follows the row it was
  placed on**, foot's model and ghostty's, over kitty's fixed screen
  position and WezTerm's per-cell slices. See "What a resize does to an
  image" below.
* ~~**The alternate screen's images.**~~ **Taken, 2026-09-22: each screen
  owns its images.** The shell's are not drawn while a full-screen program
  is up and are back when it exits; the program's are deleted when it
  leaves, the rule foot and ghostty both follow.
* **Private vs shared colour registers** (`[[source: ctlseqs]]` mode 1070).
  Whether each Sixel gets a fresh 256-register palette or shares one across
  the session. The ecosystem default is **private** (mode 1070 *set*) --
  xterm, foot and notcurses all default there, a fresh palette per image --
  which is both the de-facto default and the safer isolation. (An earlier
  version of this line said xterm defaults to shared; that was wrong, caught
  by the prior-art survey and corrected in the Images register.)

  **Answered 2026-09-25, unbuilt**: note V's V18 put this mode and DECSDM to
  the owner, and its "Decisions taken, 2026-09-25" records: "**V12, V14, V18,
  V19: built to the spec**, overriding their recommendations. `?1001`
  highlight tracking, the ambiguous-width switch with `?1021`, the Sixel modes
  `?80` and `?1070`, and the kitty graphics protocol." Neither mode is built:
  `PrivateMode::from_wire` names neither 80 nor 1070.
* **DECSDM** (mode 80). Its meaning inverted between VT330 and later xterm,
  and `[[source: ctlseqs]]` documents both. baud picks one and records the
  choice with the source conflict named, the DECAWM precedent.

  **Answered in part 2026-09-25, unbuilt**: note V's V18 decides that `?80`
  is to be built to the spec, with `?1070` (the bullet above). Which of the
  two meanings it takes is not in that answer, and stays open.
* **`Pb`/`P2` opaque background: counted, not built.** `P2` = 0 or 2 asks that
  unpainted pixels be drawn in an opaque background rather than left
  transparent. baud draws them transparent -- the `P2` = 1 behaviour, and the
  more useful default, since an image then composites over the text -- and the
  opaque fill is **not built**, because ~~*what colour* to fill with has no
  defensible pinned source (`[[source: vt3xx-sixel]]`'s background chapter is
  unread), and~~ a wrong opaque fill paints over content that should show
  through, which is worse than the current transparent; the texts are read
  now, and what they leave open is set out at the end of this item. So the gap
  is made visible rather than filled with a guess: the graph now routes the
  `Node::Dcs` parameters into `csi`, `SixelStart` records the `P2` selector on
  `Cold`, and the terminator bumps `ImageStore::sixel_p2_ignored` **after the
  image stores** -- not at parse time, so a Sixel later refused (which drew
  nothing for a fill to have helped) does not inflate the count. **How often a
  rendered image asks for it is the number that would justify building the
  fill** -- the `clipboard_reads_refused` / `mouse_unencodable` pattern.
  Routing the parameters moved the stride-k class counts +0.6% at stride 4
  (the pinned `the_alphabet_reduces_as_the_stride_doubles` trade), and the
  DECRQSS path (`DCS $ q`, and `DCS 1 $ q` with a digit before the `$`) is
  unchanged, which the conformance suite confirms. **Since 2026-10-11 an
  absent or empty `P2` is counted as 0 is**: `[[source: vt3xx-sixel]]` lists
  "0 or 2 (default)" and reads a missing parameter as 0, so `ESC P q` asks
  what `ESC P 0;0;0 q` asks, where the counter missed it. The fill is still
  not built, for Jev. The colour's texts are read now:
  `[[source: dec-std-070]]` 070-9 §7.2 calls it "device dependent" and §10
  initialises it to the "Device default background"; `[[source: vt3xx-sixel]]`
  says "the current background color", and on the VT340 that is an output-map
  location, chosen by ReGIS's `S(I...)` (`[[source: vt3xx-screen-control]]`),
  in the one map its Sixel registers also write -- which xterm follows as the
  image's register 0 (practice), where baud's registers are private to each
  image. The region is the image's rectangle, which `[[source: vt3xx-sixel]]`
  erases with `Ph` and `Pv` and 070-9 allows. And no text says how a
  background pixel draws on baud's translucent window, where every background
  carries the window's alpha and an image is composited over the glyphs: at
  the window's alpha the glyphs under the image show through, and opaque it is
  a slab the rest of the window is not. The record, "An absent Sixel P2 is P2
  = 0, and the fill is for Jev", sets out the options.

  **What a VT340 does, from vt340test (2026-10-11), practice and not a text.**
  The region: with raster attributes "the width and height are used to clear a
  background rectangle", and without them "On the VT340 using P2=0 clears the
  screen from the cursor to the bottom right corner"
  (`[[source: vt340test-p2effect-sh]]` 6-7, 12-13), which its capture shows
  (`[[source: vt340test-p2effect-png]]`), and which 070-9 §10's initial
  extent, "Distance from current ANSI text position to current ANSI right
  margin" and "to current ANSI bottom margin" (PDF p. 926), describes;
  `[[source: vt340test-physicalsixels]]` adds that the clear comes "upon
  receive the first sixel bit pattern" (185-188). The colour: "The background
  is always color index 0" (`[[source: vt340test-readme]]` 100-101), the
  extent is "the rectangle that is cleared to the text background color"
  (`[[source: vt340test-physicalsixels]]` 178-179), and under `?5`, where the
  screen shows grey, the fill is black, the map entry and not the reversed
  screen (`[[source: vt340test-background-select-2-png]]`). On that terminal a
  Sixel's `#0` does not name entry 0: "it is only the order of the assignment
  that matters ... the first sixel color defined is mapped to VT color #1, not
  color #0" (`[[source: vt340test-readme]]` 142-144), so xterm's fill from the
  image's register 0 follows neither the VT340 nor the VT3xx chapter's
  "current background color" where the registers are private to the image, as
  baud's are (a reading). With these, the region reads as settled by the texts
  -- the raster extent when one is sent, 070-9 §10's extent to the margins
  when none is, each conforming to both DEC texts and what a VT340 does, where
  a fill of the data's reach alone departs from 070-9 §7.2 (a reading) -- and
  the colour nearly so, baud's default background, with when it is read and
  what it is under `?5` still open. **How a filled pixel draws on a
  translucent window stays open too**: no text knows a window's alpha. The
  record, "A Sixel's background fill waits on Jev, and the column has a second
  text", holds the options with their marks.

## What this note does not cover

Kitty graphics (the decoder question, above), image animation, Sixel
*output* (baud is a terminal, not a Sixel-producing application), and the
ReGIS graphics `ctlseqs` documents beside Sixel -- a vector description
language with no bitmap, a far larger and separate surface, recorded `out`.
