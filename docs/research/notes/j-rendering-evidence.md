# Rendering -- the evidence behind the register

> **Group:** J · **Status:** draft · **Companion:** `j-rendering.md` is the
> decisions register; this note is what each decision rests on, numbered E1-E15
> to match D1-D15, with the sizing, the seams and the things that are still
> unverified at the end. Every citation is a manifest key. Every LOC figure is
> a `wc -l` on a pinned checkout, every latency figure is someone else's box,
> and nothing here has been measured on baud.

## E0. How the evidence was gathered

Six sweeps ran in parallel on 2026-09-12: font formats, shaping,
rasterisation, compositing, memory placement, presentation. Each read the
primary specification and the source of the shipping terminals at pinned
commits -- alacritty `d692748`, contour `6777ff0`, foot `2705e36`, ghostty
`9bbb9b2`, kitty `9710eb4` (GPL-3, read for design only), rio `7ae0875`,
Windows Terminal `dcfbcdd`, wezterm `2afb836` -- and the reference engines
HarfBuzz `64e2b53`, rustybuzz, HarfRust, swash, allsorts, FreeType, Skia,
Vello, Pathfinder. Two sweeps measured the fonts on this Mac directly
(`/System/Library/Fonts`, 375 faces) and drove CoreText through a ctypes
probe for ground truth. kitty's GPL source is cited as design evidence and
nothing from it may be copied.

The two Adobe specifications are RC4-encrypted PDFs with an empty user
password; a zlib-only extraction finds zero streams and every probe misses,
which is the letter-spaced-ECMA-48 failure shape. `scripts/extract-adobe-cff.py`
decrypts them and runs five positive controls each (`[[source: adobe-5176-cff]]`,
`[[source: adobe-5177-type2]]`).

## E1. Crates, and why the core is untouched

`baud-core` is `no_std`, `alloc`-free, zero-dependency and `forbid(unsafe)`,
and `no_std` has no runtime ISA detection -- CLAUDE.md records that the
parser's AVX2 arm had to be chosen once at startup by a caller for exactly
that reason. A compositor needs the opposite: pixman's shape is one chain
chosen at startup from `cpuid`/`auxv` plus a per-operation fast-path table
plus a scalar fallback, and Rust's `multiversion` or hand-rolled
`is_*_feature_detected` gives the same thing outside `no_std`. Atlases and
caches need an allocator. So the split is forced by the core's own contract,
not by taste: `baud-font` (parse, shape, raster -- `no_std` + `alloc`, zero
deps, the pure part; font-rs and ab_glyph are `no_std`-able proofs),
`baud-render` (compositor with CPU and GPU backends), `baud-present-*`
(one per platform, system libraries only). The core's read surface already
exists: `Term::viewport()` returns the materialised grid at `Term::stride()`,
`Term::marks()` the attached codepoints, and the rendition table resolves a
`StyleId`. The renderer needs per-cell state the cell cannot hold --
`CellFlags` is fully allocated -- so damage lives outside the cell (E12).

## E2. Instance records and the background bitmap

Every GPU terminal read is the same machine
(`[[source: wt-atlas-backendd3d]]`, `[[source: ghostty-renderer-generic]]`,
`[[source: kitty-shaders]]`, `[[source: alacritty-renderer-text-glsl3]]`,
`[[source: wezterm-src-quad]]`, `[[source: contour-rhirenderer]]`,
`[[source: rio-grid-mod]]`): the CPU converts the grid into flat fixed-size
per-instance records, one memcpy puts them into mapped or shared memory, and
one or two instanced draws expand `instance_id`/grid position into a quad
whose fragment shader samples the atlas.

| terminal | per-glyph instance | background | who gets an instance |
|---|---|---|---|
| Windows Terminal | 20 B (`QuadInstance`: u16 shading, i16x2 pos, u16x2 size, u16x2 texcoord, u32 colour) | 4 B/cell R8G8B8A8 texture, one full-screen quad | drawn glyphs only; whitespace emits nothing |
| ghostty | 32 B `CellText` | 4 B/cell `CellBg`, full-screen quad indexes it | non-blank cells |
| alacritty | 36 B (GLSL3) / 96 B (GLES2) | in the instance | non-empty cells |
| kitty | 20 B `GPUCell` | in the instance | **every** cell, blanks included |
| wezterm | 4 × 68 B vertices ≈ 272 B/quad, ≈ 544 B/cell with bg | in the quad | every cell |

Sources: `[[source: wt-atlas-backendd3d-h]]` L87-100 for the record,
`[[source: wt-atlas-backendd3d]]` for `_drawBackground` and `_drawText`,
`[[source: ghostty-renderer-metal-shaders]]` L260-286,
`[[source: kitty-cell]]` L36-41, `[[source: alacritty-renderer-text-glsl3]]`
L284-318, `[[source: wezterm-src-quad]]` L33-43. Windows Terminal's
performance PR (`[[source: wt-atlas-readme]]`, and the 20-byte record's own
comment) states that `i16` pixel positions "made the largest impact on
performance and power draw", and that skipping the background upload when its
generation is unchanged "halves the GPU load".

Upload policy is uniform: the whole instance buffer is re-emitted per frame
(kitty, alacritty, wezterm, contour, ghostty's buffer), and the CPU-side
rebuild is what is restricted to dirty rows (ghostty `rebuildCells`
L2529-2640, Windows Terminal's `ShapedRow` cache, rio). **Nobody uploads
partial byte ranges**: 10k cells × 32 B is 320 KB and a memcpy of it beats the
bookkeeping. The bottleneck every one of them names or exhibits is the CPU
cell-to-instance conversion -- style resolution, shaping, atlas lookup -- not
the GPU (`[[source: ghostty-font-shaper-cache]]` puts shaping at 96% of frame
time before its cache existed).

**The candidate nobody ships.** No terminal maps its own cell store as the GPU
buffer. The closest are kitty, whose `line->gpu_cells` is already in GPU
layout and is memcpy'd row by row into a mapped orphaned buffer
(`[[source: kitty-screen]]` L3880, `[[source: kitty-gl]]` L676), and ghostty on
UMA, where the memcpy into `.shared` contents is the only copy. The design
that removes the conversion is a cell layout that *is* the instance layout.
baud's `PackedCell` -- `Content(u32)`, `StyleId`, `CellFlags(u16)` -- is 8 B and
already carries the two indices a shader would need: the style into a
rendition table and the codepoint into a glyph→atlas map. Shipping the 80 KB
grid and resolving both in the shader from two GPU-resident tables is
therefore expressible; it costs a table probe per fragment, needs the atlas
map on the GPU, and has no prior art. It is the one design that satisfies
"zero-copy" literally, and it is unmeasured.

## E3. Placement by synchronisation, not bandwidth

The buffer is 80 KB (10k cells × 8 B) or 320 KB of instances. Moving it:

| path | rate | 80 KB costs |
|---|---|---|
| PCIe Gen3 x16, pinned | ~12 GB/s | ~7 µs |
| PCIe Gen4 x16 | ~25 GB/s | ~3 µs |
| Apple UMA (M4 Pro / Max) | 273 / 410-546 GB/s | ~0.2 µs |
| CPU memcpy on this box (`ceiling.rs`) | 62-69 GB/s | ~1.2 µs |

Every row is under 0.1% of an 8.3 ms frame, so bandwidth cannot decide
placement. What differs is where the GPU *reads* from and how many
synchronisation points the CPU pays. The vendors' own guidance:

* Vulkan: `DEVICE_LOCAL` is "most efficient for device access"; at least one
  `HOST_VISIBLE | HOST_COHERENT` type is guaranteed
  (`[[source: vulkan-memory]]`). VMA: a `DEVICE_LOCAL | HOST_VISIBLE` type is
  "likely the best choice" where it exists, the BAR heap is "256 MB by default
  unless ReBAR", and GPU reads of host-local memory over PCIe "may be limited"
  (`[[source: vma-usage-patterns]]`); on Windows all three vendors expose every
  `HOST_VISIBLE` type as `HOST_COHERENT`, so no flush
  (`[[source: vma-memory-mapping]]`). Sawicki's survey: Intel iGPU one heap with
  every type `DL|HV`; AMD APU a 256 MB `DL` heap plus the rest; NVIDIA disjoint;
  ReBAR makes the whole `DL` heap host-visible; write-combined memory wants
  sequential memcpy, never reads, never strides
  (`[[source: sawicki-vulkan-memory-types]]`). AMD: Fury X 512 GB/s device
  memory against ≤16 GB/s PCIe (`[[source: amd-vulkan-device-memory]]`).
  NVIDIA: host-visible VRAM writes "may be slower", CPU reads "significantly
  slower", watch BAR1 traffic (`[[source: nvidia-vulkan-dos-donts]]`).
* Metal: Apple GPUs "have a unified memory model"; `.shared` is the default on
  Apple silicon; "populate on the CPU, access frequently on the GPU → Shared";
  `makeBuffer(bytesNoCopy:)` takes a page-aligned single VM region with no
  copy; `.writeCombined` is for "CPU writes, never reads"
  (`[[source: apple-storage-mode-apple-gpus]]`,
  `[[source: apple-storagemodeshared]]`). The Best Practices guide: small data
  updated every frame → Shared, because "copying to video memory may be more
  expensive than GPU accessing system memory directly"
  (`[[source: apple-metal-resource-options]]`).
* D3D12: `GPU_UPLOAD` is CPU-visible VRAM, write-combined on discrete,
  identical to `UPLOAD` on UMA, gated by `OPTIONS16.GPUUploadHeapSupported`
  (`[[source: ms-d3d12-gpu-upload-heaps]]`); `UPLOAD` is "CPU-write-once,
  GPU-read-once", with "dynamic constant buffer read repeatedly" as the typical
  use (`[[source: ms-d3d12-heap-type]]`); `CacheCoherentUMA` makes upload heaps
  write-back and write-combining "a detriment"
  (`[[source: ms-d3d12-architecture]]`). AMD: `GPU_UPLOAD` reads as fast as
  `DEFAULT`, CPU reads extremely slow, strided writes wider than 32 DWORDs up to
  2× slower, keep allocations ≥64 KB (`[[source: amd-d3d12-gpu-upload]]`).
* OpenGL 4.4: `glBufferStorage(MAP_PERSISTENT | MAP_COHERENT)` with a fence,
  or flush ranges when non-coherent (`[[source: arb-buffer-storage]]`).

What the terminals do: Windows Terminal `D3D11_USAGE_DYNAMIC` +
`MAP_WRITE_DISCARD` + memcpy, 64 KB rounding; ghostty `.shared` if
`hasUnifiedMemory` else `.managed` + `didModifyRange`, three frames in flight
on Metal, one on GL (`[[source: ghostty-renderer-metal]]` L37, L79-82,
`[[source: ghostty-renderer-metal-buffer]]` L75-125); alacritty
`GL_STREAM_DRAW` + `glBufferSubData` in batches of 65,536; kitty orphan + map
per frame; wezterm three rotating persistently-mapped buffers
(`[[source: wezterm-src-renderstate]]` L176-260).

**The startup rule, per API.** Metal: `hasUnifiedMemory` → one `.shared` ring
of three (A/B `.writeCombined`), else `.shared` is still legal and `.managed`
with dirty-row `didModifyRange` is the B arm. Vulkan: `INTEGRATED_GPU`, or a
`DL|HV|HC` heap of ≥1 GiB → UMA-shared; else a `DL|HV` type → BAR device-local
(≤64 MB per allocation per AMD, handle OOM by falling through); else `HV|HC`
ring persistently mapped, read once per frame, with a staging copy as the
measured B arm. D3D12: `CacheCoherentUMA` → custom write-back heap; `UMA` →
`UPLOAD`; `GPUUploadHeapSupported` → `GPU_UPLOAD`; else `UPLOAD`. GL:
persistent-mapped ring, orphan + `glBufferSubData` fallback. **The atlas is
always device-local**: a few MB, static, randomly sampled -- staged once on
discrete, in the one memory on UMA; never sample a host-visible image on
discrete. Universal: write whole dirty rows (1.6 KB) with memcpy, never
per-cell scatter; never read the mapped pointer; ring depth equals frames in
flight; the fence or semaphore is the only synchronisation.

**Unmeasured and decisive for one arm:** no vendor publishes the per-access
latency of a vertex fetch from host-visible memory on discrete without ReBAR.
Apple's `.shared` cache maintenance is undocumented. MoltenVK's memory types
on Apple silicon are unverified.

## E4. `bytesNoCopy` over the live grid

`makeBuffer(bytesNoCopy:)` would let the GPU read `Term::viewport()`'s slice
directly, and `Term::viewport` un-rotates the grid *in place* while the parser
keeps writing the next read into it. The GPU would read a grid being mutated,
and making that safe needs either a ring of grids inside the core or a
synchronisation scheme that costs more than the 1.2 µs copy it replaces. The
copy into ring slot N is the recommendation of the memory sweep and of every
terminal's practice. The owner's own refinement -- "or have the memory where
it's faster" -- is what settles it.

## E5. CUDA

A kernel launch is 3-50 µs; a vertex shader composites the whole grid in less.
NVIDIA's own guidance lists zero-copy mapped pinned memory as "Low Priority"
and "for integrated GPUs" (`[[source: nvidia-cuda-best-practices]]`);
`cudaMallocManaged` first-touch faulting costs 2.12 ms against 24.5 µs
resident and 26.1 µs prefetched on a P100
(`[[source: nvidia-cuda-unified-memory]]`); Tegra recommends mapped pinned
memory for small buffers, uncached on the iGPU and I/O-coherent since Xavier
(`[[source: nvidia-cuda-tegra]]`); Grace Hopper makes plain `malloc`
cache-line coherent over NVLink-C2C at 900 GB/s bidirectional, of which a
measured ≤60% is reached (`[[source: nvidia-grace-hopper]]`). None of that
helps a compositor; all of it says the discrete NVIDIA path is Vulkan with the
ReBAR heap (E3). An import path -- `VK_KHR_external_memory` +
`cudaImportExternalMemory` + external semaphores -- stays available for a
future consumer of the frame. CLAUDE.md's own GPU section reaches the same
conclusion for the parser from the same launch cost.

## E6. Rasterising on the CPU

Five accumulation rasterisers were read at the line level and they are one
algorithm: for each flattened segment walk the scanlines it crosses, deposit
signed trapezoid areas into the cells it touches, then prefix-sum each row and
quantise `min(|acc|, 1) · 255`. font-rs `raster.rs` L44-105 with the SSE
prefix-sum in `accumulate.rs` L29-67 (`[[source: font-rs-raster]]`,
`[[source: font-rs-accumulate]]`); fontdue's `add(height, mid_x)`
(`[[source: fontdue-raster]]`); ab_glyph's fork, whose "avx2" variant is the
scalar body under a target-feature flag (`[[source: ab-glyph-raster]]`
L308-317); stb_truetype's v2 rasteriser with its documented over-coverage on
overlapping shapes (`[[source: stb-truetype]]` L3024-3394, L5004). FreeType's
`ftgrays.c` is the same exact-area idea with `(cover, area)` cells in a banded
pool -- its header says "inspired by LibArt" and that curve coverage is
"calculated approximately" because conics are still split to lines
(`[[source: freetype-ftgrays]]`); AGG's cell rasteriser credits the same
authors (`[[source: agg-rasterizer-cells-aa]]`) and zeno is the literal Rust
port swash uses (`[[source: zeno-raster]]`). Skia's AAA is the other exact
family, with edge intersections so nonzero winding is exact per strip at
~2,500 lines (`[[source: skia-skscan-aaapath]]`). Vello's strips accumulate
winding as `f32x4` per column and quantise the same way, which is the GPU
twin (`[[source: vello-strip]]` L379-466, `[[source: vello-fine]]`).

| reference | lines | includes |
|---|---|---|
| font-rs raster + accumulate + geom | **410** | quads, SSE scan; no cubics, no even-odd |
| fontdue raster + simd | 240 | same |
| ab_glyph raster + geometry | **490** | + cubics, `no_std` |
| stb_truetype v2 + driver + flatten | 680 | signed-area scanline, edge sort |
| ftgrays.c core / file | ~1,290 / 2,160 | cells, banding, both fill rules |
| zeno raster.rs | 831 | ftgrays in safe Rust |
| Skia AAA + analytic edge | ~2,500 | exact nonzero |

Flattening: font-rs's fixed count `n = 1 + ⌊(3·|p0 − 2p1 + p2|²)^¼⌋` in pixel
units is branch-free and known before emitting
(`[[source: font-rs-raster]]` L106-129); Levien's analytic parabola form
distributes error evenly and vectorises (`[[source: levien-flatten-quadbez]]`);
Vello flattens at 0.25 px (`[[source: vello-flatten]]` L15-19), stb at 0.35 px
by midpoint subdivision, FreeType at a quarter pixel. For a cached glyph err
fine (0.1 px): the cost is per cache miss, not per frame.

SIMD lands in three places and not a fourth: the prefix-sum/quantise pass
(font-rs's shifted-add scan; 4-wide NEON via `vaddq_f32` + `vextq`, 8-wide
AVX2), the flatten (evaluate all `t_i` in lanes), and the byte pack. The line
splat is scalar in every reference, and cell-sweep rasterisers are scalar by
construction. A cell-sized glyph buffer (12×24 px = 1.2 KB of f32) is the
working set, which is why the dense array beats ftgrays' linked cells and
banding -- those exist for large glyphs on small memories.

GPU-from-outline, ranked for 10-16 px monospace: Slug -- bands of quadratic
Béziers in a float16 texture, two rays per pixel, exact winding, AA a
two-ray estimate, "ignores font hinting"; the patent dedicated to the public
domain 2026-03-17 and the reference shaders MIT/Apache
(`[[source: slug-jcgt-2017]]`, `[[source: slug-readme]]`,
`[[source: slug-license]]`); Vello's compute fine stage, exact area on the GPU
but a scene renderer needing compute (`[[source: vello-fine]]`); Pathfinder's
CPU tiling plus GPU area-LUT fill, quantised and dormant
(`[[source: pathfinder-fill-area-inc]]`); Loop-Blinn's implicit-function
triangles with a 1 px AA ramp (`[[source: loop-blinn-2005]]`); vector textures
(`[[source: dobbie-vector-textures]]`); SDF/MSDF, where msdfgen's README says
`screenPxRange()` under 2 gives "a high probability that the anti-aliasing will
fail" -- a 12 px glyph from a 32 px field with range 4 is 1.5
(`[[source: green-2007-sdf]]`, `[[source: msdfgen-readme]]`). Every shipping
terminal composites CPU bitmaps from an atlas
(`[[source: ghostty-shaders-common]]`, `[[source: kitty-shaders]]`,
`[[source: alacritty-text-f]]`); Vello's own direction for small scenes moved
geometry back to CPU SIMD and kept the GPU for compositing.

## E7. Hinting, subpixel and gamma

FreeType: `FT_LOAD_TARGET_LIGHT` is vertical-only snapping via the auto-hinter,
"just like Adobe's proprietary font engine", and `NO_HINTING` gives "blurrier
bitmap glyphs" (`[[source: freetype-glyph-retrieval]]`,
`[[source: freetype-text-rendering-general]]`); the TrueType bytecode
interpreter is 7,627 lines and is not needed for light hinting. DirectWrite's
`NATURAL` mode is the same contract -- vertical hinting kept, horizontal free
(`[[source: ms-dwrite-rendering-mode]]`). macOS does no hinting and dilates
instead: ghostty `setShouldSmoothFonts` with a 0-255 strength, subpixel
positioning on, quantisation off (`[[source: ghostty-font-face-coretext]]`
L478-493); kitty smooths unconditionally (`[[source: kitty-core-text]]` L820).
Linux: kitty maps fontconfig hintstyle to `NORMAL`/`LIGHT`/none
(`[[source: kitty-freetype]]` L129-134); ghostty defaults `light` as "the
effective default with most Fontconfig-aware software" and disables hinting
for cell-constrained glyphs (`[[source: ghostty-config]]` L9834-9840,
`[[source: ghostty-font-face-freetype]]` L359-385).

Gamma: ghostty's `alpha-blending` is `native` / `linear` / `linear-corrected`,
default `native` on macOS and `linear-corrected` elsewhere, because linear
blending "eliminates the darkening artifacts" but makes dark-on-light text
thinner (`[[source: ghostty-config]]`, `[[source: ghostty-shaders-common]]`
L31-32, L112-135); kitty's `text_composition_strategy` has the same shape with
gamma and contrast knobs and an sRGB LUT (`[[source: kitty-shaders]]`
L495-496, L1089-1100); alacritty blends raw sRGB with dual-source LCD
(`[[source: alacritty-text-f]]` L28-31). Two of three found physically-linear
blending wrong for small text. Coverage therefore stays a linear fraction out
of the rasteriser and the policy lives in the compositor, shipped with the
dilation control because the two compensate for each other. LCD subpixel AA:
alacritty only, only when fontconfig's `rgba` is set; 3× horizontal oversample
plus a 5-tap filter and dual-source blend if ever wanted.

## E8. Shaping

**What a shaper is for a terminal.** The core hands clusters -- base plus
attached marks and joiners, width 1 or 2 -- and the shaper chooses a glyph per
cluster, optionally merges adjacent clusters for a programming ligature, and
places marks. HarfBuzz's own boundary statement says it does no bidi, run
segmentation or line breaking (`[[source: harfbuzz-what-it-doesnt-do]]`), and
the pipeline it runs is `hb-ot-shape.cc`: Unicode props, cluster formation,
normalisation, GSUB or morx, advances, GPOS or kerx, fallback mark
positioning, hide default-ignorables (`[[source: harfbuzz-hb-ot-shape]]`,
`[[source: harfbuzz-hb-ot-shape-normalize]]`,
`[[source: harfbuzz-hb-ot-shape-fallback]]`). Cluster level 0 attaches marks,
modifiers and ZWJ to the closest preceding cluster
(`[[source: harfbuzz-usermanual-clusters]]`).

**The data model that fits baud is swash's**: input is pre-clustered
`CharCluster`s, fallback is per cluster without reshaping, and output is
`GlyphCluster { glyphs, components }` where a ligature is a cluster with
components. baud-core already resolves the cluster and its width, so it
supplies (1) and (5) of the common design below for free.

**Cell mapping, five ways.** kitty groups by glyph *names* and empty glyphs,
with a spacer strategy detected per font by shaping `===` and `###`
(`[[source: kitty-fonts]]`); ghostty draws an N-cell ligature as one entry at
the first cell and resets alignment on a "first codepoint in cluster and not
after a glyph from the current or next cluster" heuristic
(`[[source: ghostty-font-shaper-harfbuzz]]`); wezterm gives a cluster the
Unicode column width of its bytes and shares cells by advance ratio
(`[[source: wezterm-shaper-harfbuzz]]`); Windows Terminal computes the
expected advance as the cell span and adds the difference to the last glyph
(`[[source: wt-atlas-atlasengine]]`); foot never crosses a cell. The common
design: resolve the face per cluster first (ghostty `indexForCell`, wezterm's
per-cluster retry that will not "fragment runs of unresolved codepoints",
kitty per cell, WT `MapCharacters`); shape maximal same-font runs with cluster
= cell index; map back so the glyph covering k cells draws at the first and
the rest draw nothing; absorb the advance mismatch WT's way; zero-advance
marks inherit the base's cell. Runs never cross a row and break on face
change, face-changing style, wide spacers and optionally the cursor cell
(`[[source: ghostty-font-shaper-run]]`).

**Programming ligatures.** Fira Code's `calt` is a chain of *single*
substitutions through empty full-advance spacer glyphs, so glyph count and
total advance never change and monospace is preserved by construction; its
own benchmark puts that form at 3.5× a plain `sub 1 2 3 4 by liga`. That fixes
the GSUB subset -- types 1, 4, 6 (formats 1/2/3), 7, with `lookupFlag` mark
skipping, Coverage 1/2 and ClassDef 1/2 (`[[source: ms-opentype-gsub]]`,
`[[source: ms-opentype-chapter2]]`, `[[source: ms-opentype-featurelist]]`) --
and it is why the run cache is load-bearing. ghostty's cache is keyed on a
Wyhash of (codepoint, cluster-relative position) pairs plus run length and
font index, 256 × 8-way (`[[source: ghostty-font-shaper-cache]]`); kitty's on
exact per-cell bytes plus codepoints (`[[source: kitty-shaped-run-cache]]`).
baud's key is the run's `PackedCell` words plus attached-codepoint ids plus
face and style, hashed once. `liga` from prose fonts is almost always unwanted
in a grid -- ghostty hard-breaks `f→l` and `s→t` so it never forms -- and
`calt` is the carrier (`[[source: ghostty-font-shaper-feature]]`).

**Emoji.** Noto's build encodes every sequence class -- ZWJ, skin tone, flags,
keycaps, tags -- as GSUB type-4 ligatures under `ccmp`, longest first, with
`FE0F` stripped at build time (`[[source: noto-emoji-add-emoji-gsub]]`,
`[[source: noto-emoji-add-glyphs]]`). So the shaper must run `ccmp`, skip
default-ignorables during matching (HarfBuzz `may_skip`, L871 of
`[[source: harfbuzz-hb-ot-layout-gsubgpos]]`), zero-advance and hide the
leftovers, and keep the cluster one cell pair whatever the glyph count
(`[[source: uts51-emoji]]` ED-16, ED-13, ED-9a, ED-14). `cmap` format 14
resolves `FE0E`/`FE0F` when present (`[[source: ms-opentype-cmap]]`).

**Marks.** GPOS type 4 mark-to-base and 6 mark-to-mark with GDEF classes
(`[[source: ms-opentype-gpos]]`, `[[source: ms-opentype-gdef]]`); HarfBuzz's
fallback centres a mark on the base's ink box by combining class with a
`y_scale/16` gap, adequate for Latin, Greek and Cyrillic. Recommendation: the
fallback as floor, GPOS when `mark`/`mkmk` exist.

**Sizing.** HarfBuzz's OT layout is ~19.3k lines, rustybuzz ~17k excluding
generated tables, swash 22.8k, allsorts 46.1k; HarfBuzz's Arabic shaper is
1,441 lines, Indic 2,278, USE 2,074. The minimal subset above -- cmap 4/12/14
~300, GSUB/GPOS common ~600, GSUB 1/4/6/7 with ignorable skipping ~600, GPOS
4/6 plus fallback marks ~500, run/cluster mapper and cache ~400 -- is
**2.5-3.5k**. rustybuzz is archived (2026-07-26); HarfRust is its successor
and reports under 25% slower than HarfBuzz.

## E9. Apple Color Emoji and morx, measured

`Apple Color Emoji.ttc`: 192,120,460 B, two fonts, `sbix` v1 with nine PNG
strikes (20-160 ppem), 3,844 glyphs, **cmap only (0,4) format 12 -- no format
14**, **no GSUB**, GPOS 1,086 B, `morx` v3 475,632 B. One chain, 25 subtables:
13 ligature (type 2), 4 non-contextual (4), 4 contextual (1), 2 insertion (5),
2 rearrangement (0). `defaultFlags` 0x3 turns on 17: all 13 ligature tables,
two non-contextual, two contextual; insertions and rearrangements are off
(`[[source: apple-ttrm-morx]]`, `[[source: apple-ttrm-feat]]`,
`[[source: apple-ttrm-tables]]` for the STX header and lookup formats 0/2/4/6/8/10,
`[[source: apple-ttrm-appendixf]]` for the selector registry).

CoreText ground truth through `CTLine` at 32 pt: the three-person family →
one glyph `u1F46A`; a flag → `u1F1EC_u1F1E7`; keycap → `u0031_u20E3`; thumbs
up with tone → `u1F44D.3`; `263A FE0F` → the emoji glyph; `263A FE0E` →
Helvetica's `whitesmilingface`, which is *font fallback*, not morx; the
mixed-tone holding-hands family → **two glyphs**, `u1F469.3.L` at advance 0
and `u1F468.4.RA` at advance 32; a tag-sequence flag → one glyph. HarfBuzz
knows the last case: "If we're using morx shaping, we cancel mark position
adjustment because Apple Color Emoji assumes this will NOT be done"
(`[[source: harfbuzz-hb-ot-shape]]`, issue 2967), and HarfBuzz prefers morx
over GSUB for horizontal text whenever a font has it
(`[[source: harfbuzz-hb-aat-layout]]`, issue 2124).

`Menlo.ttc`: no GSUB or GPOS, `morx` v2 with four ligature subtables,
`defaultFlags` 0x1 = feature 27 "Unicode Decomposition", selector 0 canonical
composition, on; "Common Ligatures" off. CoreText: `e` + U+0301 → `eacute`;
`fi`, `->`, `=>`, `!=` stay separate. So Menlo's morx is canonical
composition, and NFC before cmap lookup reproduces it. `SFNSMono.ttf` has a
714 B GSUB and no morx.

What the terminals do on macOS: ghostty defaults to CoreText shaping
(`[[source: ghostty-font-backend]]`, `[[source: ghostty-font-shaper-coretext]]`)
with HarfBuzz-over-CoreText optional; kitty shapes through HarfBuzz over a
CoreText font, which is HarfBuzz's AAT path; wezterm's vendored HarfBuzz has
AAT compiled in; alacritty asks CoreText for glyphs per character and renders
sequences separately. None implements morx and none substitutes a GSUB emoji
font.

The morx subset baud would need: chain header and feature entries (flags =
`defaultFlags` only, no user features), subtable headers with the v3 coverage
byte, lookup formats 0/2/4/6/8/10, the extended state-table driver with the
four reserved classes and `DontAdvance`, the type-2 ligature driver
(`SetComponent` pushes, `PerformAction` pops through the action list, `Store`
/`Last` replaces and deletes components), type 1 contextual with `SetMark`,
type 4 as one lookup; parse-and-skip 0/5/`kerx`/`trak`. HarfBuzz's
implementation is 1,091 + 1,080 + 106 + 269 lines
(`[[source: harfbuzz-hb-aat-layout-morx-table]]`,
`[[source: harfbuzz-hb-aat-layout-common]]`, `[[source: harfbuzz-hb-aat-map]]`);
rustybuzz's ~1.3k, swash's ~1.9k, allsorts' ~1.7k. baud's subset is
**~700-1,000 lines**, about the GSUB core's size. The three options and their
prices are in D9; the differential oracle is the CoreText probe already
written (`scripts/`-bound, currently in the scratchpad) plus HarfBuzz's AAT
tests off-Mac.

## E10. Colour emoji formats

`sbix` (macOS, PNG strikes, skip the undocumented `flip`/`emjc` graphic
types), `CBDT`/`CBLC` formats 17/18/19 (Noto on Linux and Android, 10.7 MB
at v2.051) and `COLRv0` + `CPAL` (layered glyphs, trivial) share one PNG
decoder and a blit; allsorts spends 2,484 + 1,651 + 327 lines on the three
(`[[source: ms-opentype-sbix]]`, `[[source: apple-ttrm-sbix]]`,
`[[source: ms-opentype-cbdt]]`, `[[source: ms-opentype-cblc]]`,
`[[source: ms-opentype-colr]]`, `[[source: ms-opentype-cpal]]`). `COLRv1` is
32 paint formats with gradients, transforms and a compositing DAG -- a vector
paint engine with its own hazard class (cycles, `PaintColrGlyph` recursion)
-- and Fedora's default Noto is COLRv1 at 5 MB. Whether that font carries a
v0 fallback is unverified. Segoe UI Emoji on Windows 11 is a v1/v0/mono
hybrid.

## E11. Variable fonts

SF Mono, Cascadia and JetBrains Mono are variable. `fvar` and `STAT` are
required to read a variable font and `glyf` already holds the default
instance; `gvar` is optional and `CFF2` variation needs `HVAR` too
(`[[source: ms-opentype-fvar]]`, `[[source: ms-opentype-gvar]]`,
`[[source: ms-opentype-avar]]`, `[[source: ms-opentype-hvar]]`,
`[[source: ms-opentype-cff2]]`). ttf-parser's variation code is 3,287 lines;
FreeType's `ttgxvar.c` 5,081. The delta-parsing hazard is the one that was
exploited: CVE-2025-27363, a signed-short to unsigned-long cast in GX subglyph
parsing giving an undersized heap allocation and an out-of-bounds write
(FreeType ≤ 2.13.0). Of 375 faces on this Mac, 29 carry `fvar`, one `CFF2`.

## E12. Damage

Where the terminals keep it: ghostty a per-row dirty bool array outside the
cell (`[[source: ghostty-renderer-generic]]`); Windows Terminal an invalidated
row range plus per-row pixel extents, and a memcmp of the colour-bitmap rows
before bumping the generation (`[[source: wt-atlas-atlasengine]]`); alacritty
per-line `[left, right]` column bounds in a two-frame ring
(`[[source: alacritty-display-damage]]`, `[[source: alacritty-term-term-mod]]`
L217); wezterm per-line sequence number plus shape hash; kitty one screen
bool (`[[source: kitty-screen]]`); foot a `clean:1` bit *inside* its 12-byte
cell plus a row dirty flag (`[[source: foot-terminal]]` L42-72,
`[[source: foot-render]]` L3300-3320). baud's cell has no spare bit, and the
GPU terminals do not need one: `RowExtent` and `RowRot` exist per physical
row, and the two candidates -- a generation counter bumped on every write
path, or a frame-to-frame memcmp of the 80 KB materialised viewport against
the last uploaded copy -- differ by a measurement.

Forwarding: alacritty's `swap_buffers_with_damage` on Wayland/EGL
(`[[source: alacritty-display-mod]]` L613-616), Windows Terminal's `Present1`
with one dirty rect and an optional scroll rect
(`[[source: wt-atlas-atlasengine-r]]` L470-508), foot's
`wl_surface.damage_buffer` per box; kitty declined pending benchmarks. foot's
benchmark on an i9-9900 (`[[source: foot-benchmark]]`): dense 45.5 ms against
alacritty's 97.5, light 4.4 against 12.8, full-screen scroll 5.7 against
10.9, cursor motion 14.1 against 25.0 -- a CPU renderer with damage beating a
GPU renderer without it, which is the idle-power and latency argument in
numbers. foot's own wiki concedes a CPU renderer "should always lose out" on
a full-screen refresh; its wins are damage and the parser.

## E13. Presentation, per platform

**Wayland.** The wire format is a 32-bit object id plus a 32-bit `size<<16 |
opcode` header with typed arguments, fds over `SCM_RIGHTS`, on
`$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` (`[[source: wayland-protocol-book]]`).
wayland-backend's `rs` backend speaks it with no C library --
`wire.rs` and `socket.rs` are ~600 lines over `rustix`
(`[[source: wayland-backend-wire]]`, `[[source: wayland-backend-socket]]`). The
interfaces to bind are those foot binds (`[[source: foot-wayland]]`): core
(`[[source: wayland]]`, `wl_surface` v7 with `damage_buffer` ≥4, `wl_shm` v3
with `release` ≥2, `wl_keyboard` keymap fd `xkb_v1`), `xdg-shell` v7 with
mandatory ping/pong and `ack_configure` before the first commit
(`[[source: wp-xdg-shell]]`), `wp_presentation` v2 whose `presented` event
carries `vsync | hw_clock | hw_completion | zero_copy` and whose text defines
zero-copy as "direct scanout of a fullscreen surface and a surface on a
hardware overlay" -- "compositing with OpenGL counts as copying, even if
textured directly from the client buffer" (`[[source: wp-presentation-time]]`),
`fractional-scale-v1` with `viewporter` (`[[source: wp-fractional-scale-v1]]`,
`[[source: wp-viewporter]]`), `xdg-decoration` for server-side decorations on
KDE and Hyprland (`[[source: wp-xdg-decoration-unstable-v1]]`), and the
pacing set: `fifo-v1`, `commit-timing-v1`, `linux-drm-syncobj-v1`,
`tearing-control-v1` (`[[source: wp-fifo-v1]]`, `[[source: wp-commit-timing-v1]]`,
`[[source: wp-linux-drm-syncobj-v1]]`, `[[source: wp-tearing-control-v1]]`),
plus `cursor-shape-v1` and `text-input-v3` for IME
(`[[source: wp-cursor-shape-v1]]`, `[[source: wp-text-input-unstable-v3]]`).
Hyprland advertises `wl_compositor` 6, `wl_shm` **2**, `xdg_wm_base` 7,
`zwp_linux_dmabuf_v1` **5** (not 6), `wp_presentation` 2, tearing, fifo,
syncobj driver-gated, and needs no Hyprland-specific protocol
(`[[source: hyprland-protocolmanager]]`).

CPU path: foot's `shm.c` verbatim -- `memfd_create` with sealing (`GROW |
SHRINK | SEAL`, not `FUTURE_WRITE` because libwayland maps `PROT_WRITE`),
`ftruncate`, `mmap`, one `wl_shm_pool`, buffers marked busy until
`wl_buffer.release`, pixman drawing straight into the map, scroll by moving
the offset inside a memfd with `PUNCH_HOLE` (`[[source: foot-shm]]` L38-58,
L656-935). Whether it is zero-copy is the compositor's choice: wlroots' GLES2
renderer uploads shm with `glTexSubImage2D` and imports dmabuf as an
`EGLImage` without a copy (`[[source: wlroots-gles2-texture]]` ~L96); KWin now
wraps the memfd in a udmabuf and imports without a copy *if* pool offset and
size are page multiples and the stride is a multiple of 256
(`[[source: kwin-making-wl-shm-fast]]`). Bake that alignment in.

GPU path and its consequence: `VK_KHR_wayland_surface` takes a `wl_display*`
and `wl_surface*` (`[[source: vulkan-vk-khr-wayland-surface]]`), and Mesa's
WSI includes `wayland-client.h` and binds dmabuf, presentation, fifo,
commit-timing, syncobj and tearing itself (`[[source: mesa-wsi-common-wayland]]`)
-- so a pure-protocol client cannot use the Vulkan or EGL surface extensions
without pulling `libwayland-client`. The alternative is what Mesa does
internally: `VkImage` with `VK_EXT_image_drm_format_modifier` chosen from the
dmabuf feedback tranches, exported with `VK_EXT_external_memory_dma_buf`,
handed to `zwp_linux_buffer_params` (`[[source: wp-linux-dmabuf-v1]]`,
`[[source: vulkan-vk-ext-image-drm-format-modifier]]`,
`[[source: vulkan-vk-ext-external-memory-dma-buf]]`), with implicit sync or
syncobj timelines and `VK_KHR_present_wait`-style pacing done by hand
(`[[source: vulkan-vk-khr-present-wait]]`,
`[[source: vulkan-vk-ext-swapchain-maintenance1]]`). That rules out OpenGL on
Linux. No terminal read speaks raw protocol: foot uses libwayland-client,
alacritty winit + glutin, ghostty GTK.

**X11.** x11rb's `RustConnection` is "a pure-rust implementation of a
connection to an X11 server" over a Unix or TCP stream with no libxcb
(`[[source: x11rb-rust-connection-mod]]`). CPU: MIT-SHM 1.2 `AttachFd` /
`CreateSegment` plus `ShmPutImage`, one server-side copy and no way around it
(`[[source: xcb-proto-shm]]`, `[[source: xorg-mit-shm]]`). GPU: DRI3 1.2
`PixmapFromBuffers` with modifiers and `GetSupportedModifiers`, then Present
`PresentPixmap` with `target_msc`/`divisor`, `CompleteNotify` ust/msc and
`IdleNotify` for buffer reuse (`[[source: xorg-dri3proto]]`,
`[[source: xorg-presentproto]]`, `[[source: xcb-proto-dri3]]`,
`[[source: xcb-proto-present]]`); `VK_KHR_xcb_surface` pulls libxcb
(`[[source: vulkan-vk-khr-xcb-surface]]`). XKB over the `xkb` extension
(`[[source: xcb-proto-xkb]]`). The udmabuf-memfd-to-DRI3 CPU path is an
inference, unverified.

**macOS.** Apple's documentation: `CAMetalLayer.nextDrawable` blocks up to one
second, `maximumDrawableCount` 2 or 3, `displaySyncEnabled` default true,
`presentsWithTransaction` for tear-free resize
(`[[source: apple-cametallayer]]`, `[[source: apple-mtldrawable-present-at]]`);
`IOSurface` is "a framebuffer object suitable for sharing across process
boundaries" (`[[source: apple-iosurface]]`); `makeTexture(descriptor:iosurface:plane:)`
since 10.11 (`[[source: apple-mtldevice-maketexture-iosurface]]`);
`CALayer.contents` accepts a layer image (`[[source: apple-calayer-contents]]`);
`NSView.displayLink(target:selector:)` on 14+ replaces `CVDisplayLink`,
deprecated in 15 (`[[source: apple-nsview-displaylink]]`). ghostty does not use
`CAMetalLayer`: it renders into an IOSurface-backed `MTLTexture` -- `.shared`
if `hasUnifiedMemory`, else `.managed`, write-combined -- with three in
rotation, and presents by setting a plain `CALayer.contents` to the IOSurface
on the main thread with `contentsGravity` top-left, discarding mis-sized
surfaces (`[[source: ghostty-renderer-metal-iosurfacelayer]]`,
`[[source: ghostty-renderer-metal-target]]`, `[[source: ghostty-renderer-metal]]`);
its view sets `layer.contentsScale = backingScaleFactor` in
`viewDidChangeBackingProperties` and implements `NSTextInputClient`
(`[[source: ghostty-surfaceview-appkit]]`); it paces with a display link and
warns that disabling window vsync can kernel-panic on 14.4+. That design is
the only one where a CPU renderer writing under `IOSurfaceLock` and a GPU
renderer drawing into the same texture share one zero-copy present, and it
composes with `macos-liquid-glass` through `set_content_view`. The floor is
`libobjc`, AppKit, QuartzCore, IOSurface and Metal -- the OS, not middleware.

**Inside the owner's crate, read from source.** `macos-liquid-glass`
1.0.0-beta.4 depends on `objc2` 0.6, `objc2-app-kit` 0.3.2,
`objc2-foundation` 0.3 and `objc2-core-foundation` 0.3 and nothing else
(`[[source: mlg-cargo]]`). `GlassWindow::new` is an `NSWindow` with
`FullSizeContentView`, a transparent titlebar, `setOpaque(false)` and a clear
background; `::borderless` adds a `canBecomeKeyWindow` override; the public
surface is `set_content_view`, `content_view`, size and origin, appearance,
title, `follow_icon_style` and `ns_window()` as the escape hatch for what is
not wrapped (`[[source: mlg-window]]`). `GlassSurface` wraps
`NSGlassEffectView` with style, corner radius and tint, and its
`set_content_view` is the view's `contentView` (`[[source: mlg-glass]]`).
Apple guarantees placement only for that child: "arbitrary subviews aren't
guaranteed specific behavior with regard to z-order in relation to the content
view or glass effect" (`[[source: apple-nsglasseffectview-contentview]]`), and
several glass shapes belong in one `NSGlassEffectContainerView` to reduce
render passes (`[[source: apple-nsglasseffectcontainerview]]`). The crate has
no DPI, display-link, event or drawable-layer helper today; its example
content view is `NSTextField` labels.

**The compositing constraint is measured, by the owner.** The glass samples
what is behind the effect view and the content view is composited on top of
the material, so an opaque layer filling the content view hides the glass
entirely. The crate's own measurements record that darkening "entirely by an
opaque paper paints the glass out", that a `textBackgroundColor` fill
"rendered the light terminal as a flat white slab with no glass left in it",
and that the material lightens when the window resigns key with no public
knob (`[[source: mlg-measurements]]`). So baud's layer is non-opaque
(`[[source: apple-calayer-isopaque]]`; `isOpaque = true` lets Core Animation
drop the alpha channel), clears to the terminal background at a configured
alpha rather than to an opaque colour, and the darkening goes on the material
through `tintColor` with a translucent dim in the content -- the recipe the
owner shipped. Whether Core Animation's legibility treatment for a
`contentView` alters the colours of a Metal or IOSurface layer is
undocumented; measure luma with the crate's harness before trusting a colour.

**DPI, pacing, input -- the three helpers the crate lacks.**
`NSWindow.backingScaleFactor` is 2.0 on high-resolution modes
(`[[source: apple-nswindow-backingscalefactor]]`), and the view learns of a
change in `viewDidChangeBackingProperties`
(`[[source: apple-nsview-viewdidchangebackingproperties]]`), where ghostty sets
`layer.contentsScale` inside a transaction with actions disabled and resizes
the surface to bounds × scale. Pacing on the crate's macOS 26 floor is
`NSView.displayLink(target:selector:)` (`[[source: apple-nsview-displaylink]]`);
`CAMetalLayer.presentsWithTransaction` is the knob for a present that rides
the Core Animation transaction, tear-free on resize
(`[[source: apple-cametallayer-presentswithtransaction]]`), and the IOSurface
path has no vsync property at all because the `contents` commit is the
present. Input: `keyDown` forwarded through `interpretKeyEvents`, which routes
to the input manager and returns `insertText` and `doCommand(by:)`
(`[[source: apple-nsresponder-interpretkeyevents]]`), with the view
implementing `NSTextInputClient` so the candidate window lands at the cursor
cell via `firstRect(forCharacterRange:)` (`[[source: apple-nstextinputclient]]`).
ghostty's view is the worked reference for all three.

**What to add to the crate, all within its dependency family.** A drawable
content view (`wantsLayer`, `makeBackingLayer` returning either a
`CAMetalLayer` or an IOSurface-content `CALayer` with `contentsGravity`
top-left, `acceptsFirstResponder`, a resize callback that does the
contentsScale transaction); a `backing_scale_factor()` accessor and a delegate
hook for backing-property changes; a display-link wrapper over
`NSView.displayLink`; a responder trait for key, mouse and scroll plus the
`NSTextInputClient` forwarding and an `NSTrackingArea` for mouse-moved; small
window additions (`make_first_responder`, `set_min_size` for cell-multiple
sizing, `acceptsMouseMovedEvents`); optionally an
`NSGlassEffectContainerView` wrapper for splits. Each needs only
`objc2-quartz-core`, `objc2-metal`, `objc2-io-surface` from the same family.

**Windows.** Flip model: `FLIP_DISCARD`/`FLIP_SEQUENTIAL`, `BufferCount` ≥ 2,
DirectFlip and MPO bypass DWM, the frame-latency waitable object gives one
frame of latency in Independent Flip, `ALLOW_TEARING` lower
(`[[source: ms-dxgi-flip-model]]`, `[[source: ms-dxgi-swap-chain-flag]]`,
`[[source: ms-dxgi-frame-latency-waitable]]`, `[[source: ms-dxgi-vrr]]`);
`Present1` takes dirty rects and a scroll rect (`[[source: ms-dxgi-present1]]`);
composition swapchains for DirectComposition
(`[[source: ms-dxgi-composition-swapchain]]`); per-monitor DPI v2 with
`WM_DPICHANGED` (`[[source: ms-high-dpi]]`). Windows Terminal's recipe
(`[[source: wt-atlas-atlasengine-r]]`): three buffers (two can lock during
capture or move), `FLIP_SEQUENTIAL` by default because DWM asked for it so
Panel Self Refresh works with `Present1` dirty rects, the waitable object with
`SetMaximumFrameLatency(1)` and a 100 ms `WaitForSingleObjectEx` before
drawing, `Present1(1, 0, &params)`, `B8G8R8A8_UNORM`, `SCALING_NONE`.
Swapchain buffers are not CPU-mappable, so a CPU renderer's floor is one GPU
copy from an upload-heap texture (memory-to-memory on UMA), or a GDI
`CreateDIBSection` blit that forfeits DirectFlip.

**XKB.** Wayland and X hand over an `xkb_v1` keymap as text; `libxkbcommon` is
the conventional floor; `xkbcommon-rs` 0.2.0 and `xkb-core` 0.1.0 are
pure-Rust ports of unverified maturity.

**The seam.** One trait, two target kinds, presentation owning every
allocation and the renderer never allocating: `acquire()` blocks or polls on
buffer-free (`wl_buffer.release`, Present `IdleNotify`, IOSurface not in use,
the DXGI waitable) and returns either a CPU target (pointer, stride, format,
size: the mapped shm slice, the MIT-SHM segment, `IOSurfaceLock`'s base, a
D3D12 upload map) or a GPU target (the `VkImage` on an exportable dmabuf, the
`MTLTexture` from an IOSurface, the D3D12 back buffer); the renderer returns
a damage list and, for GPU, a fence. `present(target, damage, sync)` is
`attach + damage_buffer + commit`, `PresentPixmap` with an update region,
`CALayer.contents = surface` in a transaction after the command buffer
completes, or `Present1` with rects. `feedback()` returns presented time,
refresh and flags so the pacer can schedule before vblank, and reports
`zero_copy` honestly. Format and modifier negotiation is presentation's;
alignment (page-aligned pools with stride % 256, IOSurface `bytesPerRow`) is
baked in. Damage flows both ways: the platform gives buffer age and scroll
hints, the renderer gives dirty rects.

## E14. Pacing and the latency claim

foot: `wl_surface.frame` callback gates the next frame, one in flight, with a
delayed-render timer and `wp_presentation` feedback logged
(`[[source: foot-render]]`, `[[source: foot-config]]`). Windows Terminal: the
waitable object at latency 1 before every frame. ghostty: `displaySyncEnabled`
plus a display link. Tearing is opt-in on every platform
(`[[source: wp-tearing-control-v1]]`, `[[source: ms-dxgi-vrr]]`). The only
published latency figures are software-measured: Typometer on Xorg, 2024 --
xterm 5.3 ms, st 6.2, alacritty 6.9, kitty tuned 10.7, kitty default 23.8,
wezterm 26.1, gnome-terminal 30.2 (`[[source: lkhrs-terminal-latency]]`); Dan
Luu's 2017 p50 idle -- Terminal.app 6, st 25, alacritty 31, iTerm2 44
(`[[source: danluu-term-latency]]`). No photon-level measurement exists for
any target terminal, so baud may not claim one without a rig.

## E15. Not in v1, each with its cost

| omitted | what it would cost | why not now |
|---|---|---|
| Arabic joining | ~1.5k lines + `ArabicShaping.txt` tables; joining crosses cells | explicit-mode bidi applications send presentation forms; owner call |
| Indic / USE / Khmer / Myanmar | 2.1-2.3k lines each + ~2.2k generated tables | a syllable is one visual unit over N `wcwidth` cells; no terminal maps it sanely |
| bidi | wezterm-bidi is `no_std` + `alloc`, MIT, 100% BidiTest | a grid-level concern under `[[source: terminal-wg-bidi]]` (`BDSM`, `SPD`, `?2500`/`?2501`), owned by the VT core's modes; only VTE implements; ghostty and kitty force LTR |
| COLRv1 | a vector paint engine with cycle and recursion hazards | E10 |
| `gvar`/`avar`/`HVAR` | ~3.3k lines + the delta hazard class (CVE-2025-27363) | E11 |
| hinting | auto-hinter ~15k, bytecode 7.6k | E7; vertical snap ~100-200 lines later |
| LCD subpixel AA | 3× oversample + 5-tap filter + dual-source blend, ~100 lines | only alacritty, only on request |
| `kerx`, `trak`, `kern` | ~1k in HarfBuzz | fixed advance makes pair kerning meaningless |
| WOFF/WOFF2 | 1,049 lines + brotli in allsorts | disk fonts only |
| SVG-in-OpenType | an SVG renderer | nothing needs it |
| OpenGL on Linux | EGL pulls `libwayland-client` | E13 |
| CUDA compositing | a launch per frame at 3-50 µs | E5 |
| prose `liga` | nothing; it is the *absence* of a feature | unwanted in a grid |
| Slug / GPU-from-outline | curve + band textures, two-ray AA | E6; stated as out rather than implied |

## Unverified, so not decided

The per-access PCIe latency of a host-visible vertex fetch on discrete
without ReBAR; Apple `.shared` cache maintenance and `.writeCombined` for
80 KB/frame; MoltenVK's memory types on Apple silicon; ReBAR prevalence on
laptops; whether Noto-COLRv1 carries a v0 fallback; non-Noto emoji feature
tags (Twemoji, Segoe); Fira Code's spacer metrics; the maturity of the
pure-Rust XKB ports; the udmabuf-to-DRI3 CPU path on X11; MicrosoftDocs'
licence on the OpenType HTML; any shaping cost in ns per cell beyond ghostty's
96% anecdote; Windows Terminal's 20-byte record, which is field arithmetic
with no `static_assert`; ghostty's frame rate; contour's dirty tracking;
~~whether Core Animation's legibility treatment for a glass `contentView`
recolours Metal or IOSurface content~~ -- measured since, in part: on baud's
IOSurface set as `CALayer.contents`, under the `Regular` material, with
opaque cells, seven bands captured byte for byte and luma exact, so there it
does not (CLAUDE.md, "Luma under glass, measured"). A `CAMetalLayer`, the
`Clear` material that is the default since 2026-09-19, and translucent cells
are unmeasured.
