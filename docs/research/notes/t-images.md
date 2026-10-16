# T. Images: the phase register for Sixel and Kitty graphics

The Images phase covers two protocols -- Sixel and Kitty graphics -- and this
is its register, written before the code the way the renderer phases were
(`j-rendering.md` is the precedent, and `n-make-faster.md` the shape). It is
grounded in five prior-art surveys of shipping terminals, each claim traced to
a pinned source. `s-sixel.md` is the Sixel half's spec and its acceptance
gates; **this note settles the two things `s-sixel.md` deferred to "a Kitty
note" -- the decoder seam under baud's `no_std` contract, and the Kitty
protocol itself** -- and records the cross-protocol owner's calls.

**Status, 2026-10-10.** The Sixel half of the sequencing below is built but
for step 5: `a7ea193` (2026-09-21) committed this note with the decoder, the
image store in caller memory, placements anchored to an absolute line, all
three render paths, and the corpus's first two Sixel files,
`tests/recordings/sixel/swirl.raw` and
`tests/recordings/sixel/img2sixel-96x72.six`. Step 5, the decoder
pointer-walk, is not built: `SixelCanvas::paint` still computes
`y * width + x` per pixel. Kitty graphics is unbuilt; the owner's answer to
note V's V19 (2026-09-25) is to build it to the spec, and the PNG and zlib
decoder its `f=100` needs exists under the core's contract,
`baud-domain/src/png.rs`, moved from `baud-font` for V19 step 2 (the seam
section, corrected 2026-10-15). The sentences
below that called the corpus empty of Sixel were written before the capture
landed and are struck where they stand.

The one-sentence shape: **decoding is not the hard part; storage, placement and
the decoder *seam* are.** A Sixel decoder is a stateful byte walk with no
cleverness in it (`baud-domain/src/sixel.rs`, built and gated). What baud does
not have and every other terminal takes for granted is somewhere to put an
image's pixels under a core that allocates nothing -- which is `s-sixel.md`'s
subject -- and where each of an image protocol's decodes lives under
`#![forbid(unsafe_code)]`, `no_std`, `alloc`-free code, which is this note's.

## The gates lead, and chunk invariance is first-class for Kitty

`s-sixel.md` fixes three gates before any parser code -- decode agreement at
every read boundary, a golden frame digest, and placement across a scroll and a
resize -- plus the pixel oracle covering a placed image. All four carry to
Kitty unchanged. One gate is promoted to the front for Kitty specifically,
because it is not an afterthought there but a structural property:

**A Kitty `t=d` payload spans reads by construction.** The protocol chunks a
direct transmission into `<= 4096`-byte pieces with `m=1` on every chunk but
the last (`[[source: kitty-graphics]]`), and requires a client to finish all
chunks of one image before any other graphics command. So a `read(2)` boundary
lands mid-payload on *every* real transfer, and "a screen that depends on where
the reads fell" is this repo's single most-repeated bug class. The consequence
for the design is concrete: **the base64 decoder is incremental with carry
state**, not a decode-this-buffer call -- three input bytes span a chunk
boundary and the two carry bytes have to survive to the next `read`. The
control-string suite already holds chunk invariance for consumption; the Images
gate extends it to the decoded pixels, and it sits beside the golden digest and
the placement-across-scroll-and-resize test at the top of the phase, not after.

## The decode seam: what fits the contract, and what is injected

This is the question `s-sixel.md` left open and deferred here. The answer is a
boundary drawn by one property -- **does the decode allocate a heap of unknown
size** -- and every format the protocol defines falls inside it. This section
first put PNG and zlib outside, behind a decoder the embedder injects; the
workspace held the counterexample all along, `baud-font`'s `png.rs`
(2026-09-12; `baud-domain/src/png.rs` since 2026-10-15), which decodes colour-emoji strikes into two buffers its caller
sizes (corrected 2026-10-15):

| protocol input | decode | where |
|---|---|---|
| Sixel raster (`DCS ... q`) | RLE over 256 registers into a caller canvas | **`baud-domain`** -- alloc-free, built |
| Kitty `f=24` RGB, `f=32` RGBA | none: raw pixels, already final layout | **`baud-domain`** -- alloc-free |
| base64 (`t=d` payload) | 4 bytes -> 3, into a caller buffer, incremental | **`baud-domain`** -- alloc-free |
| Kitty `f=100` PNG | PNG + zlib over untrusted bytes, sized by its `IHDR` | **`baud-font`** (`png.rs`) -- alloc-free, built; out of baud-core's reach today |
| Kitty `o=z` zlib | inflate to a size the keys declare: `s` x `v` x 3 or 4, or `S` for a PNG | an inflater exists inside `png.rs`, private to its `IDAT` stream; a standalone one is unbuilt |

**The defensible claim is narrow, and stated as the constraint rather than as a
comparison.** rio, wezterm, ghostty and contour all push image decode to the
embedder because they chose the Rust `image` crate, which needs `std`,
allocates, and uses `unsafe` -- incompatible with baud-core on all three axes
(the `wezterm/contour/rio` survey confirmed each axis by compiling). That is
their *choice*, not a limit; stated as "baud does more in its core than they do"
it invites the same correction the ghostty-scrollback comparison needed. The
honest form: **every format the protocol defines can live under baud's
contract.** Sixel and raw RGB/RGBA are alloc-free by construction -- a fixed
256-register palette, a caller-provided output canvas, no compression -- and
PNG and zlib are alloc-free because every output size is declared before the
decode: `png.rs` reads a PNG's from its `IHDR` and decodes into two caller
buffers of exactly that size, an interlaced one's scratch counting each Adam7
pass's own filter bytes. Its limits are its own, not the contract's: it takes
the whole file as one slice, and its inflate reads only the `IDAT` stream and
cannot resume across reads. ~~It refuses Adam7 interlacing
(`PngError::Interlaced`).~~ It reads Adam7 since 2026-10-15 (the record,
"png.rs reads Adam7").

**The destination is sized before the decode for every format, `f=100`
included.** For a raw format the core knows the destination size from
`s=`/`v=` before a byte arrives. For PNG, `[[source: kitty-graphics]]` says
"The width and height of the image will be read from the PNG data itself",
and `Png::parse` reads them from `IHDR` before any image data, after which
`Png::raw_len` and `Png::rgba_len` give both buffers' exact lengths. Two
qualifiers: `Png::parse` takes the whole file as one slice, so the payload is
gathered into scratch first; and under `o=z` the PNG's own header is
compressed, which the protocol answers itself: "if you use both PNG and
compression, then you must provide the S key with the size of the PNG data."
ghostty's precedent -- PNG decode a nullable function pointer, null in its
VT-library build, the embedder handing back a buffer and its size -- rests on
a decoder that allocates, and `png.rs` does not. An embedder hook may still be
needed for a different reason, the file and shared-memory mediums (`t=f`,
`t=t`, `t=s`), which a `no_std` core cannot open (note V's V19, option C).
Where the decoder moves so baud-core can call it, baud-domain or a new crate,
is open (note V's V19, step 2): baud-core depends on baud-domain and not on
baud-font.

**Measured 2026-10-15** (note V's V19, step 0, which has the figures and
the record entry): `png.rs` decodes a full-window 1920x1080 PNG in about
39 ms, 4.7 frames at 120 Hz, so a synchronous decode inside `feed` is
over step 3's bar. The two sessions recorded at a 1920x1080 window needed
15.5 and 18.7 MB at once -- payload, raw scratch and arena -- and the
app's default arena of 1,048,576 pixels holds neither picture. Adam7 is
2.9% of the PNGs `kitten icat` would send as they are, and its refusal
departed from `[[source: w3c-png-2003]]` 13.10 whatever the share; png.rs
reads Adam7 since 2026-10-15, equal to ImageMagick on all 155 interlaced
system PNGs. `kitten
icat` sends a PNG's own bytes when it is no wider than the window and needs
no transform, whatever its height, and otherwise raw `f=24`/`f=32` under
`o=z`.

## Zero copy, corrected: it is one copy, and baud lands it in GPU memory

The steer named zero-copy, and the Kitty survey **refutes the premise in the
rendering sense** -- worth stating plainly because the MANIFEST comment and an
earlier reading both had it wrong. Kitty's `t=s` shared-memory medium is not
"render from the client's buffer": the spec requires the terminal to read the
data out of the shared-memory object and then `shm_unlink()` + close it
immediately (`[[source: kitty-graphics]]`), so the client cannot keep the
mapping alive and the terminal cannot render from client-owned memory. There is
**exactly one copy** -- shm to the terminal's image store -- not zero.

What `t=s` (and `f=32`/`f=24` uncompressed) actually buys, in this repo's
write-amplification terms, is real and quantifiable, and both halves must be
stated:

* **No base64** -- removes the `+33%` inflation of `t=d`.
* **No zlib** -- when the pixels are sent raw.
* **No pty write path and no 4096-byte chunk round-trips** -- the escape stream
  carries only the tiny control command; the bulk bytes never touch the tty.
* **One `memcpy`, no decode** -- for `f=32`/`f=24` the shm bytes are already in
  final pixel layout.

So the honest framing is "avoid the encode and the transport copy", not "share
the buffer". **baud's edge is what it does with that one unavoidable copy.**
foot decodes into an ARGB buffer and blits it into a `wl_shm` buffer the
compositor reads -- the zero-copy is on the *output* side, CPU pixman into
shared memory. baud can decode the raster (or memcpy the raw pixels) **directly
into GPU-visible memory** through the `Owned::storage_over` shared-buffer
lending the cell path already uses, so the shader samples the image with no
separate upload step. That is the concrete "better and faster than the others"
the steer asked for, and it is one copy into GPU memory against their one copy
into CPU memory plus a GPU upload -- not a zero the survey says does not exist.

## The storage fork: ARGB, decided on the shader-sampling ground

The foot survey names the central storage decision as a fork, and it is the one
architecture choice this register makes rather than flags:

| | **ARGB per pixel** (foot) | **palette index per pixel** (libsixel) |
|---|---|---|
| memory | 4 bytes/px | 1 byte/px |
| horizontal RLE (`!Nc`) | a pointer-walk run (a 4-byte word is not a `memset`) | a single `memset` |
| render / composite | a straight blit, no indirection | a palette lookup per pixel, or a convert pass |

baud chooses **ARGB**, and the reason is baud's own, not foot's. foot's reason
is that its bottleneck is compositing over text every frame on the CPU. baud's
reason is the **cell path**: its default renderer is a shader resolving cells
from a shared buffer, and an image there wants to be a texture the shader
samples. A palette index in a shader is a **second dependent texture fetch** --
sample the index, then sample the palette at that index -- and that is the exact
dependent-load depth argument that killed the palette trie (16% smaller, 1.5%
slower) and the stored mark index in `baud-core`. ARGB is one sample; a palette
is two, serialised. So the pixels are decoded straight to GPU-visible ARGB and
referenced by an id, which also connects the design to the never-re-upload gate
wezterm and rio both use -- hash or id the image, upload once, and a redraw of
the same image is a reference rather than a transfer.

**What ARGB gives up, stated:** 4x the memory, and libsixel's `memset`
run-length trick. The second is recovered in shape rather than in kind -- foot
batches the bounds and colour check once for an entire `!Nc` run and
pointer-writes the run (the decoder optimization below), which is the ARGB
answer to what `memset` is for palette indices.

## The Kitty protocol proper

`s-sixel.md` specs the Sixel grammar; this is the Kitty surface,
from `[[source: kitty-graphics]]`.

**The envelope** is `APC G <control data> ; <base64 payload> ST` -- `ESC _ G`,
comma-separated `key=value` control pairs (single-letter keys), a base64
payload, `ESC \`. It reaches the parser as a `Node::Apc` body whose first byte
is `G`, the way the Sixel hook is a `Node::Dcs` final `q`; every other APC body
is consumed and dropped today, so a Kitty command is consumed and dropped today.

**Formats and the seam** (repeated from the seam table for the reader who starts
here): `f=24` RGB and `f=32` RGBA are raw pixels needing no decoder and are the
fast path; `f=100` PNG and `o=z` zlib need the PNG and zlib decoder, which is
`png.rs`, out of baud-core's reach today.

**Placements are separate from images** and carry a signed 32-bit `z` index --
negative draws below text, and below roughly `INT32_MIN/2` draws below even
non-default cell backgrounds. This is exactly the screen-space placement table
`s-sixel.md` settles for Sixel (`{ image slot, anchor, z }`), and it is why
`CellFlags` being full at 16 bits with zero spare is not a blocker: Kitty's own
model never references an image from a cell. Re-sending the same `(i, p)` image-
and-placement pair **replaces** that placement, which is the flicker-free
move-and-resize primitive and maps onto the placement table as an update in
place.

**Support detection is a query plus a primary-DA request**, and this makes the
first commit sound. A client sends a graphics query immediately followed by
`ESC [ c`; if a `_G...;OK` (or error) comes back before the DA reply the
protocol is supported, and if only the DA reply arrives it is not. **baud
already answers DA.** So a first commit that recognises `APC G`, counts it, and
answers *no* graphics query yields exactly the correct "unsupported" verdict at
every client -- the DA reply arrives alone -- and no program breaks. **A
count-don't-answer first commit is therefore sound sequencing, not a half-
implementation hazard**, because the detection handshake is designed for a
terminal that does not understand the escape to be silent, and baud's silence is
indistinguishable from any other terminal's.

**Response suppression `q=` is partly unresolved and must not be guessed.** The
literal spec text is `q=1` suppresses `OK` responses and `q=2` suppresses
*failure* responses -- two different response classes, not cumulative -- but the
survey flagged that the intuitive reading ("`2` suppresses both") disagrees, and
across its own reads it could not settle which kitty's code implements. Getting
it backwards means a client that asked for silence gets error spam, or the
reverse. So: `q=0` and `q=1` per the literal text; **`q=2` is unimplemented and
counted**, not recorded-and-acted-on-wrong, which is the failure
`docs/vt-coverage.md` rates worst. Pinning kitty's source for the `q=2`
semantics is the follow-on, named in the coverage row.

**Deletion (`a=d`)** selects by a `d=` letter whose case is the free-or-keep
switch -- lowercase deletes placements and keeps the image data re-displayable,
uppercase also frees the image when it has no placements left. The selector set
(`a/i/n/c/p/q/x/y/z/r/f` and their uppercase) maps onto the placement and image-
slot tables `s-sixel.md` already describes; the free-on-last-placement rule is
the same refcount those tables carry.

**U+10EEEE virtual placements are explicitly out of the first slice, for two
reasons.** The Unicode placeholder is how Kitty makes an image scroll like text
and pass through multiplexers -- print U+10EEEE in a `c x r` cell grid with the
image id in the **foreground colour**, the placement id in the **underline
colour**, and row/column/high-id bits in combining diacritics. It is worth
recording that this is a *rendition-bits escape hatch*: `StyleWord` has spare
bits where `PackedCell` has none, so an image reference riding in the colour
fields is a thing baud's cell could carry, unlike the screen-space table. But it
is out of the first slice on evidence, not asserted in:

1. **The interaction with the style table must be verified, not assumed.** A
   24-bit image id as a truecolour foreground mints a *distinct rendition per
   image id*, against `baud-core`'s 1024-slot open-addressed `tmpl` table that is
   exact only below capacity and evicts a colliding style. Whether a screen full
   of placeholder cells at distinct ids fits that table, or forces evictions that
   corrupt real renditions, is a measurement to make before building on it.
2. **The diacritics table cannot be pinned as linked.** The spec links
   `rowcolumn-diacritics.txt` at a `_downloads/<hash>/` path that is a Sphinx
   build artefact whose hash changes on every rebuild -- `check-sources.sh` would
   reject it, and pinning it would pin a moving target. The repo-side generator
   in kitty's tree is the pinnable source, and locating it is prerequisite work.

So the first Kitty slice is real placements (`a=T`/`a=p` at the cursor cell,
anchored the `s-sixel.md` way), and U+10EEEE is a later slice with those two
questions answered first.

## The decoder, and the payload that does not exist yet

The foot survey is unambiguous that **the decoder is scalar, not vectorised** --
foot's `~33%` (PR #388) and `~5%` (PR #1632) came from deleting per-pixel
address arithmetic on a per-byte state machine, which rhymes with `baud-core`'s
own recorded finding that vectorising a latency-bound loop loses. The specific
shape is a band-write loop that **carries a running pixel pointer advanced by
`stride`** rather than recomputing `row * width + col` per pixel, plus **bounds
checks deferred to resize** (never per sixel character) and **batched `!Nc`**
(one size-and-colour check for the whole run).

`baud-domain/src/sixel.rs` does not have this shape, and the reason is worth
recording so nobody reads it as a loop tweak: **the canvas API forecloses it.**
`SixelCanvas::paint(x, y, colour)` computes `y.index() * width + x.index()` per
painted pixel -- a multiply per pixel, exactly what foot deleted. A band-writer
needs a canvas that hands out a running pointer and a stride, which is a
different `SixelCanvas` shape, not an edit inside `paint`.

**It is not changed yet, and the blocking prerequisite is measurement.** This
repo's rule is that an unmeasured change is an observation, not a claim, and
there was a harder gate underneath: ~~**no recording in the corpus contains a
single Sixel byte.**~~ That is the row-map lesson exactly -- "find or record a
payload that actually uses them; record one before building it again." So the
concrete next action ~~is~~ was not the pointer-walk but **recording a Sixel
capture**, which the golden-frame gate (`s-sixel.md` gate 2) needs anyway, and
which then lets the pointer-walk be a measured A/B against a real workload
rather than a faster-looking rewrite. The capture is recorded, `swirl.raw`
beside `img2sixel-96x72.six` in `tests/recordings/sixel/` since `a7ea193`;
the pointer-walk and its A/B are still to do (corrected 2026-10-10). The optimization is real and the prior art measures it;
baud measures it on baud before believing it.

## Prior art, and what each contributes

Every load-bearing pattern below is from a survey and cited to the terminal it
was read in. The through-line: **image and placement are separate, data is
refcounted and freed on last placement, the anchor is an absolute scrollback
row, and the GPU upload is id-gated so an image is never re-uploaded.**

| terminal | what it contributes | for baud |
|---|---|---|
| **ghostty** | image/placement split reconciled by a `generation: u64`; PNG decode a nullable injected fn (null in the VT-lib build) | the seam precedent, resting on a decoder that allocates; `png.rs` does not, so its premise is gone, and a hook may remain for the file and shared-memory mediums |
| **rio** | `rio-graphics`, a leaf value-type crate with no GPU/render deps; anchor by absolute scrollback row; store source rects raw and resolve per frame; hash/id-gate the upload | `rio-graphics` *is* `baud-domain`'s role, validated; the anchor is `select.rs`'s `AbsLine` |
| **wezterm** | sha256 content-address of image data; maps an image to actual grid cells | id-gated never-re-upload |
| **contour** | `RasterizedImage` -> `ImageFragment` carries *geometry, not pixels*; `z` as `{Below, Replace, Above}` where only `Replace` dies on a text write | the placement carries geometry the paths read, the `s-sixel.md` design |
| **foot** | ARGB store (blit-friendly), scalar pointer-walk decoder, off-grid image list sorted by end-row, lifetime tied to scrollback (evict on scroll-off), split-on-overwrite; DECSDM inversion and mode 1070 = private | the storage fork's ARGB side; the decoder shape; the Sixel semantics `s-sixel.md` records |
| **libsixel** | palette-index store, `memset` RLE -- the other fork | the memory-vs-indirection tradeoff, decided against for the shader reason above |
| **Kitty** | placements + `z` + `(i,p)` replace; U+10EEEE virtual placement; `t=s` one-copy medium; DA-handshake detection | the Kitty protocol itself, and the corrected zero-copy reading |

baud's unconsumed `AbsLine`/`Anchor` types in `baud-domain` (recorded in
`f-scrollback-ring.md` as substrate waiting for a consumer) are the rio anchor
model already half-built -- an image placement is a second consumer of exactly
what selection and the scrollbar consume.

## Owner's calls at the phase level

`s-sixel.md` carries the Sixel-specific calls (arena size, reflow, mode 1070 =
private, DECSDM = follow xterm with the conflict named). This register carries
the cross-protocol ones, and hands the owner a frame rather than a number where
the research gives a discipline rather than a value.

* **The image arena is a bounded-refuse-and-count table sized by measurement,
  and the question is the byte budget and the eviction policy, not a number
  pulled from a survey.** The surveys give two disciplines: foot's -- clamp
  dimensions at decode, tie lifetime to scrollback (an image scrolled off the
  ring is destroyed), and carry *no* separate byte cap -- and xterm.js's --
  register caps plus a `~100 MB` FIFO with a placeholder glyph for an evicted
  image. baud's own precedent points at the first: `ARENA = 1 MiB` peaked
  against L2 and went negative larger, `NLINK = 128`, and `ClusterStore`'s 63
  slots *refuse rather than evict and count the refusal* -- a bounded table
  sized against rule #2 (the touched set stays resident), which is
  `s-sixel.md`'s design. The owner's call is the byte figure and whether refuse-
  and-count (baud's instinct, a wrong-image-never-drawn guarantee) or evict-with-
  placeholder (xterm.js, an unbounded-history convenience) is the policy.
* **Kitty at all, and in what order.** foot is a standing precedent for *not*
  implementing Kitty -- its maintainer declined it deliberately, on dependencies
  and minimalism, and it is an argument *for* Sixel-first rather than a source to
  copy Kitty from. Sixel needs no decoder but its own and Kitty's `f=100` needs
  PNG and zlib, so Sixel is the tractable half regardless. The register's
  recommendation is **Sixel first (its gates are already fixed in `s-sixel.md`),
  then Kitty's raw-format and placement surface reusing the same storage, then
  PNG and zlib as a separate slice, then U+10EEEE last.** Whether Kitty ships at
  all was the owner's.

  **Answered 2026-09-25**: note V's V19 put this to the owner, and its
  "Decisions taken, 2026-09-25" records: "**V12, V14, V18, V19: built to the
  spec**, overriding their recommendations. `?1001` highlight tracking, the
  ambiguous-width switch with `?1021`, the Sixel modes `?80` and `?1070`, and
  the kitty graphics protocol." Kitty graphics is unbuilt (the status above).
  That answers whether Kitty ships; the order of its slices is not in it,
  and stays open.
* **The storage fork is decided ARGB in this register** on the shader-sampling
  ground above; the owner's confirmation is the point at which it moves from
  recommended to settled, and the 4x memory cost is the thing to weigh it
  against.

## Sequencing

1. **Record a Sixel capture** into the corpus -- the golden-frame gate needs it
   and the decoder optimization cannot be measured without it. Concrete first
   action, blocking both. Done in `a7ea193`, as were steps 2 to 4 (the status
   above; corrected 2026-10-10).
2. **Sixel storage** -- the caller-provided arena, placement table and image-
   slot table on `Storage`, refuse-and-count, per `s-sixel.md`. The arena size
   is the owner's call; the mechanism does not depend on it.
3. **Sixel placement** -- absolute-line anchor + `z`, surviving scroll and the
   `Term::view(back)` rotation, per `s-sixel.md`.
4. **The three render paths** -- CPU compositor, instance, cell -- carrying the
   ARGB region, decoded into GPU-visible memory, id-gated against re-upload.
5. **The decoder pointer-walk** -- measured against the recorded capture, an A/B
   with the canvas reshaped to a band-writer, only if it wins.
6. **Kitty recognise-and-count** -- `APC G` parsed, counted, no query answered;
   DA-safe, breaks nothing.
7. **Kitty raw formats and placements** -- `f=24`/`f=32` + base64 (incremental,
   chunk-invariant) into the same storage, `z` and `(i,p)` replace.
8. **PNG and zlib** -- `png.rs` moved where baud-core can call it, its inflate
   opened to `o=z`; the core sizes `f=100`'s destination from `IHDR`, or from
   `S` under `o=z` (the seam section). Where it moves is open (note V's V19,
   step 2).
9. **U+10EEEE** -- last, after the style-table interaction is measured and the
   diacritics generator is pinned.

Each step behind its own gate, a row in `docs/vt-coverage.md` moving to `done`
only when a test exists that would fail if it regressed.

## What this register does not cover

The Sixel grammar and its gates (`s-sixel.md`), image *animation* (Kitty's
`a=f`/`a=c`/`a=a` surface, a phase of its own), Sixel or Kitty *output* (baud is
a terminal, not an image-producing application), and ReGIS (a vector language
with no bitmap, recorded `out`).
