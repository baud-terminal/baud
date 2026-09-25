# Rendering — the decisions register

> **Group:** J · **Status:** draft for the owner · **Sources:** the 236 entries
> under group J in `MANIFEST.toml`, each cited from
> `j-rendering-evidence.md`, which carries the evidence behind every line here.

## What this is

The owner's brief (2026-09-12): build the renderer **without engine
dependencies -- baud owns font parsing, shaping and rasterisation**; multi-arch
across CPU (NEON/AVX2) and GPU (Metal, Vulkan, DX12, GL, **NVIDIA/CUDA**);
**high performance**, meaning the fourth of baud's four wins, frame rate under
full-grid churn; **zero-copy wherever it is faster**, using CPU-GPU shared
memory where the architecture has it; **the fewest dependencies**; and a
separate **presentation layer** for Wayland/Hyprland, X11, macOS (via the
owner's `macos-liquid-glass`) and Windows.

Six research sweeps answered it -- font formats, shaping, rasterisation,
compositing, memory placement, presentation -- reading specs and the source of
seven shipping terminals at pinned commits, and measuring the fonts on this
Mac. This register is what they decide, numbered, each with the evidence that
decides it, what it costs, and what would reverse it. **Nothing here is code,
and nothing is measured on baud yet**: every LOC figure is a count of a
reference implementation, every latency figure is someone else's box. The
repo's rule applies -- say unmeasured, not a guess.

The single most useful thing the sweeps found is a shape, not a fact: **every
shipping terminal is the same machine.** CPU turns cells into a flat array of
fixed-size instance records, one memcpy puts it in driver-visible memory, and
one or two instanced draws expand each record into an atlas-textured quad. The
GPU is never the bottleneck; **CPU cell-to-instance conversion is**, on every
terminal measured. That reframes "high performance" for baud: the win is in the
conversion and the caches, and baud-core already does half of it -- the grapheme
cluster is the shaping unit, the `StyleId` is the rendition key, and both are
in the cell.

## The stack, sized (all counts are reference LOC, not baud's)

| layer | what baud owns | v1 scope | reference sizing |
|---|---|---|---|
| font file | sfnt/TTC directory, cmap 4/12/14, head/maxp/hhea/hmtx, OS/2, post header, loca/glyf, CFF Type 2, GSUB 1/4/6/7, GPOS 4/6 + GDEF, sbix, CBDT/CBLC, COLRv0/CPAL | as listed | ttf-parser 23.8k full, ~5.8k core; stb 5.1k incl. raster; **baud est. 10–17k before a rasteriser** |
| shaping | per-cluster glyph choice, `calt`/`liga` ligatures across cells, emoji sequences via GSUB `ccmp` + default-ignorable skipping, GPOS mark attach with HarfBuzz-style fallback, run cache | Latin, CJK, emoji, marks | **minimal ≈ 2.5–3.5k**; full (Arabic, Indic, morx) ≈ rustybuzz's 17k |
| rasteriser | signed-area accumulation into a per-glyph f32 buffer, fixed-count quad flattening, SIMD prefix-sum/quantise | grayscale, no hinting | **~400–500 lines**; font-rs 410, ab_glyph 490, ftgrays core ~1.3k |
| compositor | cells → instances, bg as a 4 B/cell texture, atlas, row-granular damage; CPU path pixman-shaped; GPU paths per API | CPU + Metal + Vulkan | WT/ghostty design; foot for CPU |
| presentation | window, surface, present, pacing, input, per platform, minimum system libs | Wayland, macOS; X11/Windows decided below | foot shm.c, ghostty IOSurface, WT swapchain |

## Decisions

### D1. Where it lives: new crates over `baud-core`, and the core is untouched
`baud-core` stays `no_std`, `alloc`-free, zero-dep, `forbid(unsafe)`. The
renderer needs an allocator (atlases, caches), runtime ISA detection (which
`no_std` lacks -- the parser already had to put its AVX2 arm outside), and
per-platform system libraries. So: `baud-font` (parse + shape + raster,
`no_std`+`alloc`, zero deps, the part that is pure), `baud-render` (compositor,
CPU + GPU backends), `baud-present-*` (one per platform). The seam into the
core is `Term::viewport()` plus `Term::marks()` and the rendition table, all of
which exist. **Reverses if:** the owner wants one crate; nothing technical
forces the split, it is the dependency boundary made visible.

### D2. Ship cells to the GPU as instance records, background as a 4 B/cell texture
Adopt the WT/ghostty split: a colour bitmap of `cols × rows × 4 B` drawn by one
full-screen quad, and one 16–20 B glyph instance **per non-blank cell** (WT's
`i16` pixel positions "made the largest impact on performance and power draw").
**Evidence:** every terminal measured; the two fastest use exactly this. 10k
cells is ~320 KB of instances at most; a memcpy of it is cheaper than any
dirty-range bookkeeping, and nobody uploads partial ranges. **Cost:** a
cell-to-instance pass per dirty row -- which is the thing to make fast (D8).
**Candidate, unmeasured -- the only true zero-copy design in the field:**
ship the 80 KB `PackedCell` grid itself and resolve `StyleId → colour` and
`codepoint → atlas rect` **in the shader** from two small GPU-side tables. No
terminal does this; it is baud's compile-time-table principle moved to the GPU.
It removes the conversion pass but moves a hash/table probe into every fragment
and needs the atlas map to be GPU-resident. It is worth one measured arm after
D2 ships, not a v1 bet. **Reverses if:** that arm wins.

### D3. Memory placement is chosen at startup, per architecture, by sync points -- not bandwidth
For an 80 KB buffer every placement costs 1–7 µs a frame (Apple UMA ~0.2 µs,
PCIe Gen3 ~7 µs, a plain memcpy ~1.2 µs on this box) -- under 0.1% of a frame.
What differs is synchronisation and whether GPU *reads* leave local memory.
So the rule is: Apple UMA and cache-coherent integrated parts → one shared
allocation in a ring of three; discrete with a BAR heap or ReBAR → write
straight into VRAM; discrete without → host-visible, read once per frame,
which every vendor's guidance permits for small per-frame data. **The atlas is
always device-local via one staging copy.** Universal rules: write whole dirty
rows with memcpy (write-combined memory penalises strided writes 2×), never
read the mapped pointer, ring depth = frames in flight, fence is the only sync.
The startup algorithm is written out in the evidence note (E3). **Reverses
if:** a measured A/B on an NVIDIA box without ReBAR shows the per-access PCIe
latency of a vertex fetch is worse than the vendors' guidance implies -- that
figure is unpublished.

### D4. `bytesNoCopy` over the live grid is not zero-copy, it is a race; copy into the ring
Mapping baud's viewport directly as the GPU buffer would have the GPU read a
grid the parser is mutating. The 1.2 µs copy into ring slot *N* is cheaper
than any synchronisation scheme that would make the live grid safe. **This is
the one place "zero-copy as much as possible" yields to "wherever is faster,"
by the owner's own refinement.** Reverses if: baud-core grows a ring of grids
(three viewports) -- a core change with its own cost, not a renderer decision.

### D5. No CUDA in the renderer; NVIDIA is served by Vulkan and DX12
A kernel launch is 3–50 µs against a whole-frame composite that a vertex
shader does in less; CUDA's zero-copy guidance is explicitly "low priority,
for integrated GPUs," and `cudaMallocManaged` first-touch faulting costs
2.1 ms against 24 µs resident. On discrete NVIDIA the right path is Vulkan
with the ReBAR heap (D3). Keep an optional CUDA *import* path
(`VK_KHR_external_memory` + `cudaImportExternalMemory`) for a future consumer,
never a compositor. **Reverses if:** the owner wants compute-side effects
(shaders as CUDA kernels) -- then interop, still not compositing.

### D6. Rasterise on the CPU, once per glyph, into a device-local atlas; composite on the GPU
The signed-area accumulator (font-rs form) is the smallest correct exact-area
rasteriser in the literature, ~400–500 lines, and it has a GPU twin in Vello's
strips. Every shipping GPU terminal composites CPU bitmaps from an atlas; none
renders from outlines on the GPU. Slug is now patent-free (dedicated to the
public domain 2026-03-17, MIT/Apache shaders) and is the only serious
from-outline option, but its AA is a two-ray estimate, it ignores hinting, and
it buys nothing over a cached bitmap at one fixed size. SDF/MSDF fails
msdfgen's own range rule at 10–16 px. **SIMD lands in the accumulate/quantise
pass and the flatten** -- the line splat is scalar in every reference, and
ab_glyph's "avx2" is the scalar body under a target-feature flag. **Reverses
if:** baud wants free zoom/rotation of text, which is not a terminal.

### D7. No hinting; coverage stays linear; gamma is the compositor's, and it is a policy with three modes
macOS does none plus dilation; Linux terminals follow fontconfig and default to
"light" (vertical-only snap). Two of three references (ghostty, kitty) found
that physically-linear blending needs a perceptual correction for small text.
So: no hinting in v1 (drops `cvt`/`fpgm`/`prep` and the 7.6k-line interpreter
class outright), an optional vertical-only snap later, LCD subpixel AA off
(only alacritty does it, only when fontconfig asks; macOS dropped it in 10.14),
and ghostty's `native` / `linear` / `linear-corrected` as the blend model.
**Reverses if:** 96-dpi Linux at 10–13 px looks worse than fontconfig-light
in a side-by-side -- the cheap test to run first.

### D8. Shaping is per run of same-font cells with cluster = cell index, and the cache is what makes ligature fonts affordable
baud-core already yields the cluster, the width and the rendition, so the
shaper's input is exactly swash's model (pre-clustered, per-cluster fallback,
`GlyphCluster.components` for ligatures) -- the closest published design to
baud's. The glyph→cell rule is **Windows Terminal's**: force each cluster's
advance sum to its cell span and draw at the first cell; it needs neither
kitty's glyph-name heuristics nor ghostty's "first codepoint missing" guess.
Programming ligatures keep one glyph per input glyph by construction (Fira
Code chains single substitutions through empty full-advance spacers under
`calt`), and Fira's own benchmark puts that form at 3.5× a plain ligature --
so the **run cache** (key: face, style, the run's cell words + attached
codepoint ids) is load-bearing, and ghostty measured shaping at 96% of frame
time before adding one. **Reverses if:** nothing; this is the consensus of
five implementations plus the one that matches baud's data model.

### D9. Emoji on macOS is a `morx` decision, and it collides with "no engine dependencies"
Measured on this Mac: **Apple Color Emoji has no GSUB and no cmap format 14**;
its 17 default-on `morx` subtables shape every UTS #51 sequence class, and a
mixed-skin-tone family legitimately shapes to **two glyphs with a
zero-advance half** in one two-cell cluster. Menlo also has no GSUB, but its
`morx` is canonical *composition*, which NFC before cmap lookup reproduces
without `morx`. Three priced options: **(1)** implement `morx` types 1/2/4
with default chain flags, ~700–1,000 lines, within 1.5× of the GSUB core,
verified by a CoreText differential on macOS (the probe exists) and HarfBuzz's
AAT tests elsewhere; **(2)** substitute a GSUB emoji font on macOS (Noto's
whole sequence set is GSUB type 4 under `ccmp`) -- one shaper, but not the
platform's emoji and a 10–24 MB font; **(3)** CoreText for that one font, as
ghostty does -- zero shaping code, a 1.9k-line second glyph→cell path, and a
platform framework inside the renderer. **The evidence recommends (1) if the
rule is strict**, with (2) as the non-Apple fallback policy anyway. **Owner
call:** is CoreText "the OS" (allowed, like libobjc) or "an engine"
(forbidden)? The same question decides `libxkbcommon` on Linux (D13).

### D10. Colour emoji is bitmap-first; COLRv1 is a paint engine and is not v1
`sbix` (macOS, PNG strikes), `CBDT` (Noto on Linux/Android) and `COLRv0`
share one PNG decoder and a blit, ~2.5k reference lines together. `COLRv1` is
32 paint formats with gradients and a compositing DAG -- a vector paint engine,
not a parser -- and Fedora's default Noto-COLRv1 needs it. **v1: sbix + CBDT +
COLRv0.** Whether Noto-COLRv1 carries a v0 fallback is unverified and decides
how much Linux loses meanwhile. **Reverses if:** it does not, and Linux colour
emoji matters for v1.

### D11. Variable fonts: default instance only in v1
SF Mono, Cascadia and JetBrains Mono are variable, and `gvar`/`avar`/`HVAR`
are ~3.3k reference lines plus the deltas hazard class that produced
CVE-2025-27363. `fvar` is required to *read* the default instance and `glyf`
already *is* it. **v1 parses `fvar`, ignores `gvar`; bold comes from the bold
face, not from `wght`.** Reverses if: the owner wants weight-as-axis.

### D12. Damage is row-granular, outside the cell, and forwarded to the presentation layer
The cell is full (`CellFlags` fully allocated), so a per-cell dirty bit is
unavailable and only foot has one anyway. Every GPU terminal is row-granular;
`RowExtent` and `RowRot` already exist per physical row. Two candidates, one
measurement apart: a per-row generation counter bumped by every write path
(ghostty/WT), or a frame-to-frame 80 KB memcmp of the materialised viewport
against the last uploaded copy (~µs, no bookkeeping). Forward the resulting
rects to the presentation layer (Present1 dirty/scroll rects, EGL
swap-with-damage, `wl_surface.damage_buffer`) -- that is where alacritty, WT
and foot get their idle-power and latency, and kitty declined it pending
benchmarks. **Reverses if:** the memcmp measures slower than the counter,
which decides the form, not the decision.

### D13. Presentation speaks the platform protocol directly; the dependency floor is named per platform
**Wayland:** pure protocol over the socket, no `libwayland` (wayland-backend's
`rs` backend is the existence proof, ~600 lines of wire code to mirror);
`wl_shm` from a page-aligned memfd with stride a multiple of 256 so KWin's new
udmabuf import makes it genuinely zero-copy, and `wp_presentation`'s
`zero_copy` flag as the honest definition of whether it was; Hyprland needs
nothing special but advertises `linux-dmabuf` **v5** and `wl_shm` **v2**, so
require those. **Consequence:** a pure-protocol client cannot use
`VK_KHR_wayland_surface` or EGL (both pull `libwayland-client`), so the Linux
GPU path is **Vulkan with dmabuf export and a self-managed swapchain** -- what
Mesa's WSI does internally, minus the library. No OpenGL on Linux. **macOS:**
the window is `macos-liquid-glass`; baud supplies the content view. Follow
ghostty, not the textbook: **one IOSurface-backed `MTLTexture`** (`.shared` on
Apple silicon) that the GPU draws into or the CPU writes under `IOSurfaceLock`,
presented by setting a plain `CALayer.contents` -- the only design where the
CPU and GPU renderers share a zero-copy present, and it avoids
`CAMetalLayer`'s drawable pool and timeout. Pace with `NSView.displayLink`
(14+). **One constraint from the owner's own measurements:** the glass
samples what is behind it and the content view is composited on top, so an
opaque layer paints the glass out -- baud's layer is non-opaque, clears to
the background at a configured alpha, and the darkening goes on the material
through `tintColor`. The crate lacks a drawable view, DPI, display-link and
input helpers; the evidence note lists the additions, all within its objc2
dependency family. **Windows:** `user32` + a DXGI flip-model swapchain copied from Windows
Terminal (3 buffers, `FLIP_SEQUENTIAL`, `Present1` dirty/scroll rects,
waitable object at latency 1); there is **no zero-copy CPU present on
Windows** -- one GPU copy from an upload heap is the floor. **X11:** x11rb's
pure-Rust connection makes it library-free; DRI3 + Present is the GPU path,
MIT-SHM the CPU one (one server-side copy, unavoidable). **Floors:** Linux
none-or-`libvulkan`, plus `libxkbcommon` unless a Rust XKB compiler is
accepted; macOS `libobjc` + AppKit/QuartzCore/IOSurface/Metal; Windows
`user32`/`dxgi`/`d3d12`. **Owner calls:** Vulkan-only Linux (yes/no); X11
first-class or XWayland fallback; `libxkbcommon` as floor or bet on a young
Rust port.

### D14. Frame pacing follows foot and Windows Terminal: one frame in flight, render on input, present at the platform's signal
Wayland `frame` callback + `wp_presentation` feedback (foot); DXGI waitable
object with max latency 1 and a wait before every frame (WT); `displaySyncEnabled`
+ display link on macOS (ghostty warns disabling it can kernel-panic on
14.4+). Tearing (`tearing-control-v1`, `ALLOW_TEARING`, `displaySyncEnabled =
false`) is an opt-in, never default. **No latency number may be claimed**: the
only published figures are software-measured (Typometer: xterm 5.3 ms,
alacritty 6.9, kitty default 23.8 on Xorg, 2024), and no photon-level
measurement exists for any target terminal. Reverses if: never; a rig would
add numbers, not change the design.

### D15. What v1 does not do, stated so it is a decision and not a gap
No Arabic joining, no Indic/USE reordering (even HarfBuzz users cannot map a
syllable to cells sanely), no bidi (a grid-level concern per the terminal-wg
spec, owned by the VT core's mode bits if ever), no COLRv1, no `gvar`, no
hinting, no LCD AA, no `kerx`/`trak`, no WOFF, no SVG-in-OpenType, no OpenGL,
no CUDA compositing, no text-font `liga` (prose ligatures are unwanted in a
grid; `calt` is the carrier). On `morx`: no insertion (type 5) or
rearrangement (type 0) subtables -- Apple Color Emoji ships both behind
feature flags its default selects nothing of -- no feature requests at all
(each chain's `defaultFlags` and nothing else, so Menlo's off-by-default
`fi` stays off and Helvetica's on-by-default one stays on), `GSUB` over
`morx` when a face carries both (HarfBuzz's rule, and the two mono faces
that matter have no `GSUB`), and a mark on a ligature attaches to the
whole ligature where HarfBuzz attaches it to the last component. Each is a
line in the evidence note with its cost, so re-opening one is a costed
change rather than a rediscovery.

## The owner's calls, minimal

1. **Constraint interpretation:** are OS frameworks (CoreText for one font,
   `libxkbcommon` for keymaps) "the OS", like `libobjc`, or "engines"? Decides
   D9's option and D13's XKB floor.
2. **Vulkan-only on Linux**, forced by pure-protocol presentation (D13).
3. **X11:** first-class, or XWayland fallback? It roughly doubles the Linux
   presentation surface.
4. **Windows** in v1 at all? It is the one platform with no zero-copy CPU
   present and the most distinct API surface.
5. **The IOSurface + `CALayer.contents` design for macOS** (D13) versus a
   `CAMetalLayer` in `macos-liquid-glass`'s content view -- both compose with
   the crate; only the first is zero-copy for the CPU renderer.
6. **What may punch through the glass.** The backdrop is visible only where
   the terminal leaves alpha below one. Is that the default background only,
   or may per-cell SGR backgrounds and images be translucent too? It decides
   whether alpha is a rendition property or a window property.

**Where the six stand (recorded 2026-09-24).** Three are settled by what was
built, and three have an answer in the session record that is not yet
confirmed here:

| call | status | evidence |
|---|---|---|
| 1. constraint interpretation | **strict**: CoreText and `libxkbcommon` count as engines. baud owns `morx` and the XKB compiler, `libxkbcommon` is an oracle only, and CoreText is unused; AppKit, Metal, QuartzCore and IOSurface remain the OS floor D13 lists | `baud-xkb` and `baud-font` depend on `baud-domain` alone; `baud-font/src/morx.rs`; `libxkbcommon` is loaded only by `scripts/xkb-oracle.py` |
| 2. Vulkan-only on Linux | answered *yes* 2026-09-12 in the session record; **unconfirmed here** | no Vulkan code exists yet, so nothing built contradicts or confirms it |
| 3. X11 | answered *XWayland fallback* 2026-09-12 in the session record; **unconfirmed here** | the Linux driver is Wayland-only (`baud-present-wayland`) |
| 4. Windows in v1 | answered *no* 2026-09-12 in the session record; **unconfirmed here** | no Windows code |
| 5. macOS present | **IOSurface + `CALayer.contents`** | `baud-present-macos`; no `CAMetalLayer` in any code |
| 6. what punches through the glass | **every background**, 2026-09-20; then `[glass] app_backgrounds` (default `dim`) and `pictures` (default `opaque`), 2026-09-22 | CLAUDE.md, "Every background is translucent now" and "A picture drawn in text is found from its block" |

Calls 2-4 move to settled when the owner confirms them.

Everything else above is decided by evidence and reverses on a measurement.
