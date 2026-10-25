# BC. How the frame path becomes a ring: D4 revisited

The owner, 2026-10-24: "so i think in D4 we need a better research how to do
that properly as i thought its all ring bufferred". This note answers it. It
covers what zero-copy means on a Mac, what baud does today, what D4 gave up,
what the copy costs now, what Apple and five other projects do, three designs
that would let the GPU read the core's own ring, and which of them to build.

Everything below was read at `f3d71d10`, on 2026-10-24. Every external source
is pinned in `docs/research/sources/MANIFEST.toml`. The decision this revisits
is D4 in `docs/research/notes/j-rendering.md`. Its history is the record,
"Nothing waits and nothing is copied: the frame loop is event driven, and the
grid is the GPU's".

**The answer, in short.** The grid is a ring, but there are two rings in the
frame path and they do different jobs. The core's ring is a ring of *lines*:
it makes a scroll free. Apple's ring is a ring of *whole frames*: each frame
gets its own buffer, and the CPU writes a buffer only after the GPU has let it
go. A copy joins the two, about 3 us of CPU a frame at 200x50. The only
designs that remove the copy without a race or a stall put a check on every
write the parser makes. On every precedent measured so far, a check like that
costs more than the copy it saves. The copy stays. The work that pays now is
removing a second copy that nothing needs. The measurement that would reverse
this is in section 8.

## 1. What zero-copy means on unified memory, and what baud does today

On Apple silicon the CPU and GPU share one memory: "Apple GPUs have a unified
memory model in which the CPU and the GPU share system memory"
(`[[source: apple-storage-mode-apple-gpus]]`). So zero-copy means one precise
thing: the GPU reads bytes where the CPU wrote them, and nobody writes them a
second time. Shared memory does not make a concurrent read safe. That needs
the synchronisation in section 4.

**The core's ring.** The primary screen and its history live in one ring of
rows at a fixed pitch (`Storage.vp`, `baud-core/src/storage.rs`). Screen row
*y* is physical row `(y + rot) % ring`, computed by a conditional subtract in
`View::phys` (`baud-domain/src/grid.rs`), whose comment says the `%` it
replaced "cost ~16% of the cells path". A full-screen scroll advances `rot`
and blanks the one row coming in (`Paint::index`, `baud-core/src/paint.rs`).
Nothing moves. At the app's default 2,000 lines of history and `MAX_GRID`
400x150, the primary ring is 2,150 rows, 6,880,000 B. The alternate screen is
its own surface of 150 rows, 480,000 B, and its ring is exactly its rows.

**The frame side.** It is a ring too: `Wheel` in
`baud-present-macos/src/journal.rs`, with `Wheel::SETS = 2` because D14 keeps
one frame in flight. Each set holds a copy of the window, a resolved rendition
table and an overlay. On each frame whose screen changed, the app copies the
window out of the core's ring into the set that no frame in flight holds
(`baud-app/src/app.rs`, around line 2222). It skips the copy when that set
already holds the current screen. The copy is laid out flat in screen order
and drawn at `RowRot::NONE`. The same copy is where bidi's visual order, the
status line, the notice row and a slide's leaving row are put into one buffer.

**What is left of the zero-copy path.** The core still writes into Metal
shared buffers (`CellMemory.grid` and `.alt`, lent through `Gpu::grid`), but
no GPU pass binds them. They are kept as `#[expect(dead_code)]` to keep the
allocation alive. The cell shader's `cell_at` still applies a rotation and a
modulo. The app hands it the identity over the copy.

**Why the line ring cannot be the frame ring as it stands.** The core rewrites
rows of the live window in place: the cursor row until the next line feed,
every row of the alternate screen (where vim, btop, less and cava run),
erases, IL/DL, the repair after a scroll region anchored at the top, and
reflow. Any of those writes races a frame that is reading the same row. The
rotation alone keeps a *retired* row safe for 2,100 more line feeds at 200x50,
but nothing protects the rows still on screen.

## 2. What D4 gave up, and why

The first design (2026-09-14) had the cell shader read the live grid under the
core's rotation, with no copy. It was safe only because `pump` refused to feed
the parser while a frame was in flight. That refusal was the freeze, and
`800694c` replaced the design with the copy (D4). In September the copy cost
"about one microsecond" (the record) and D4 quotes 1.2 us. That figure is
`ceiling.rs`'s memcpy rate, 62-69 GB/s (`j-rendering-evidence.md`, E4).

September also recorded that the GPU read a fresh copy 30-60 us faster than
the live grid. That was on five of seven rows, disjoint on asciiquarium and
cat-scroll, "nothing else running", with the mechanism unverified. D4 says it
reverses if "baud-core grows a ring of grids (three viewports)".

## 3. What the copy costs today

`frame_gpu` was re-run on 2026-10-24 at `f3d71d10` on this M5 Max. Base and
fixed binaries were built from `git archive` copies (release, with
`-align-all-functions=6`), one arm per process, the arm order rotated each
round. That was 6 outer rounds with the first dropped and 4 inner rounds with
round 0 dropped, giving n = 15 per cell, at 200x50 and 14 px on mains. Four
arms ran: the live grid waited, the live grid with a completion handler
(`async`), the copy, and the copy from the unmodified binary as an A/A check.

**Conditions differ from September.** Every round overlapped other sessions'
builds, at load 5.65-6.09 in the first sweep and 4.26-5.01 in the second.
September ran with nothing else running. That is the drifting-machine case
CLAUDE.md records, so where today and September disagree below, the honest
reading is "not reproduced under load". An idle re-run decides it (section 8).

**A defect in the bench, found first.** `frame_gpu` makes the alternate
screen's shared buffer and drops it (`let (_, alt) = gpu.grid(..)`, line 414
of `baud-bench/benches/frame_gpu.rs`). It then always binds the primary's
buffer while passing `t.rotation()` and `t.ring()`, which follow whichever
surface is up (lines 599-604). Once a recording sets `?1049h`, the live arms
draw a blank primary while the core writes the alternate. Four checks showed
it:

* On btop the live arm drew 50 of 128 frames by rows, and the copy drew all
  128 whole.
* `alike` found the live arm's cells equal on every frame and the copy's
  unequal on 127 of 128.
* A hash of the last frame's pixels found 108 pixels off the background on
  btop's live arm, which is just the cursor, against 532,855 on the copy.
* With the fix, the two images are byte-identical on all five recordings at
  both provisionings.

btop, vim, asciiquarium and cava are affected. cat-scroll and cat-cjk stay on
the primary and are not. The defect came in with `0a44e3f5` (2026-09-13),
which gave the alternate screen its own buffer. At `f1565cc9`, the September
measurement, the alternate used the one lent grid, so September's live arm
drew the right screen. **Every `frame_gpu` live-arm GPU or wall figure on
btop, vim, asciiquarium or cava recorded since 2026-09-13 drew a blank
screen.** The fix is two hunks: keep the alternate buffer at line 414, and at
lines 599-604 bind it when `t.screen() == Screen::Alternate`. The tables below
are from the fixed binary. The fix landed after this note, with a pixel
check that holds it (section 10).

GPU start-to-end, us a frame, median [range]:

| recording | live, waited | live, `async` | copy | copy, A/A |
|---|---|---|---|---|
| btop | 164.5 [164.0-172.3] | 164.3 [164.0-166.0] | 164.2 [163.9-164.5] | 164.5 [164.2-165.6] |
| cat-scroll | 284.8 [283.8-297.3] | 284.2 [161.8-291.1] | 284.2 [283.9-287.8] | 284.2 [283.8-287.7] |
| asciiquarium | 284.4 [281.0-290.3] | 283.1 [281.1-295.4] | 281.9 [280.8-292.1] | 281.4 [128.2-290.2] |
| vim | 161.3 [161.0-161.9] | 161.5 [161.0-166.5] | 161.3 [161.1-176.4] | 161.3 [161.1-162.4] |
| cat-cjk, JetBrains Mono | 283.9 [121.9-290.1] | 284.3 [283.9-287.6] | 284.0 [283.7-287.9] | 284.7 [283.8-288.8] |
| cat-cjk, Menlo stack | 157.1 [156.9-166.7] | 157.1 [157.0-157.2] | 157.1 [157.0-162.6] | 157.0 [156.7-163.1] |

**The ranges overlap on every row, so there is no result.** Under today's
load, the GPU reads the live grid and the copy in the same time. That includes
cat-scroll, the primary-screen row the defect does not touch and September
recorded as disjoint. The GPU column sits near 157-164 us or near 284 us
whatever the workload, with outliers near 122 us. That looks like the GPU's
power state, not its work. So it cannot resolve tens of microseconds, and a
future GPU claim needs the state held fixed.

CPU, us a frame, median [range]:

| recording | `async` CPU | copy | stall while the live grid is read |
|---|---|---|---|
| btop | 60.8 [59.3-65.5] | 3.0 [2.9-3.1] | 354.6 [351.9-359.4] |
| cat-scroll | 125.8 [121.7-131.7] | 3.0 [2.9-3.1] | 469.2 [355.1-483.1] |
| asciiquarium | 368.5 [354.5-388.6] | 2.8 [2.2-3.2] | 473.0 [451.4-481.5] |
| vim | 48.0 [45.2-58.5] | 3.0 [2.7-3.4] | 344.8 [332.2-352.8] |
| cat-cjk, JetBrains Mono | 121.6 [118.7-136.0] | 3.0 [2.9-3.1] | 472.9 [455.5-482.1] |
| cat-cjk, Menlo stack | 229.8 [222.0-235.6] | 2.9 [2.8-3.0] | 354.7 [326.0-363.8] |

* **The copy is 2.8-3.2 us** for 80,000 B (50 rows x 200 cells x 8 B), about
  25-28 GB/s. It did not move between the 200x50 provisioning and
  `FRAME_GRID=400x150`, where it read 3.0-3.2 us. That second sweep still
  *drew* 200x50, so no copy of a drawn 400x150 screen has been measured. The
  move from September's ~1 us is unexplained and was taken under load.
* **The copy is about 0.8-6% of a frame's CPU**, which is the map walk, the
  style sync, the overlay walk and the bind and commit: 0.8% on asciiquarium,
  about 6% on vim.
* **The stall** is how long the bench could not feed the parser while the GPU
  read the live grid: 345-475 us a frame across both sweeps. That is the
  freeze D4 removed, measured.
* The waited and `async` walls overlap on every row, so September's
  "`waitUntilCompleted` itself costs 50-100 us" was not reproduced either.

**No arm runs the app's real shape**: two sets, the `Screens::is_last`
compare, the copy, `Done::Notify` and feeding during flight. The bench's copy
arm copies every frame into one buffer. The app's own per-frame cost was
therefore unmeasured until the `app` arm (section 10).

**Everything a changed frame touches on the cell path**, at 200x50 (80,000 B
of screen):

1. `Screens::is_last` compares the view against the last set's copy and stops
   at the first difference. It reads up to 160 KB.
2. The glyph-map walk reads the view, about 80 KB.
3. The copy writes 80 KB, plus the status row and the notice row.
4. `alike` in `draw_cells_since` compares the bound buffer against
   `CellLast.cells`, once the cheap fields match.
5. On every whole redraw, `CellLast.cells` is refilled from the bound buffer
   (`baud-present-macos/src/metal.rs`, around line 2768). **That is a second
   full copy of the screen**, into a CPU `Vec`, and it repeats what
   `Screens::is_last` and the copy have already established. Items 4 and 5
   are gone since section 10.
6. When any row is reordered, `Bidi::window` copies the whole window again,
   permuted (`baud-render/src/bidi.rs`).

So a changed frame writes two screen copies, 160 KB, and a third under bidi.
Against rule 1, by arithmetic: 160 KB at 120 Hz is 19.2 MB/s. Against a 198.6
MB/s PTY flood, that is about 0.097 B written per byte ingested, beside at
least 8 B of cell stores per ASCII byte. Frames are capped at the refresh
rate, so the ratio falls as ingest rises.

## 4. What Apple recommends

**Apple's ring is N whole instances.** "You create a pool of multiple
instances of a resource and use a different one each time you render a frame",
and "After you update a buffer instance, you don't access its data with the
CPU for the rest of the frame" (`[[source: apple-sync-cpu-gpu-work]]`). The
Best Practices Guide: "the CPU must finish writing the data before the GPU can
read it, and the GPU must finish reading that data before the CPU can
overwrite it". Its listing makes three separate buffers so "that the CPU and
GPU are never accessing the same buffer simultaneously"
(`[[source: apple-mtlbp-triple-buffering]]`). WWDC 2014's "Working with Metal:
Fundamentals": writing the buffer the GPU reads makes "a race condition. At
that point what the GPU reads is completely undefined", and the fix is "a pool
of three uniform buffers" and "a dispatch semaphore with a count of 3"
(`[[source: apple-wwdc14-604]]`).

**How many.** Three, tied to the drawables: "The default limit is 3, but you
can set it to 2 ... (2 and 3 are the only supported values) ... You donʼt need
to create more buffer instances than the maximum number of drawables"
(`[[source: apple-sync-cpu-gpu-work]]`;
`[[source: apple-cametallayer-maximumdrawablecount]]`). The slot is released
from the completion handler, which the driver calls "after the GPU finishes
executing the command buffer" (`[[source: apple-addcompletedhandler]]`). D14
runs one frame in flight, so baud's two sets are that pattern at depth two.

**When a CPU write becomes visible.** The macOS 27.0 SDK: "coherency is only
guaranteed at command buffer boundaries to minimize the required flushing of
CPU and GPU caches" (`[[source: apple-sdk27-mtlresource-h]]`, line 62). The
Metal Programming Guide (2016): the device "is only guaranteed to observe any
changes made by the host CPU ... if (and only if) those changes were made by
the host CPU before the MTLCommandBuffer object was committed"
(`[[source: apple-mpg-mem-obj]]`). The current sample: "You need to finalize
all CPU writes to one buffer instance before you commit a command buffer that
references it" (`[[source: apple-sync-cpu-gpu-work]]`).

**Reading memory the CPU is writing.** "If the GPU reads the resource while
the CPU is writing to it, the GPU reads incorrect resource data"
(`[[source: apple-sync-cpu-gpu-work]]`). The SDK: "access to buffers is not
synchronized", and contents "become undefined if both the CPU and GPU write to
the same buffer ... even when the regions written do not overlap"
(`[[source: apple-sdk27-mtlbuffer-h]]`, lines 27-31). That sentence is about
both processors *writing*. No Apple text read here addresses a CPU writing one
region of a buffer while the GPU reads a disjoint one. Apple's own code does
it: the Xcode 27.0 macOS Game template makes one shared buffer of
`alignedUniformsSize * maxBuffersInFlight` (3) and writes slot *k+1* through
`contents() + uniformBufferOffset` while frames in flight read other offsets
of the same buffer, in 256-B slots (`[[source: xcode27-game-template-metal]]`,
lines 10-49 and 189-244; `[[source: xcode27-game-template-metal4]]`). That is
sample practice, not a normative guarantee.

**No Metal mechanism makes a concurrent CPU write safe.** Hazard tracking
"delays write operations until previous read operations finish" between GPU
commands only (`[[source: apple-mtlhazardtrackingmode]]`). Shared storage
leaves it to the app: "you're responsible for synchronizing access"
(`[[source: apple-mtlstoragemode-shared]]`). An early commit with a GPU wait
on a CPU-signalled shared event (`[[source: apple-sync-events-gpu-cpu]]`,
`[[source: apple-encodewaitforevent]]`, `[[source: apple-mtlsharedevent]]`)
could overlap the core's writes with GPU scheduling. But no Apple text read
says writes made after the commit are visible to the passes after the wait,
and the SDK still says "only guaranteed at command buffer boundaries". That is
unresolved, and no design below relies on it.

**`bytesNoCopy` gains no exemption.** It needs "A page-aligned pointer" and a
length "that results in a page-aligned region", and the memory must lie
"within a single virtual memory (VM) region"
(`[[source: apple-makebuffer-bytesnocopy]]`). `hw.pagesize` is 16384 here. The
result is an ordinary shared buffer, so the rules above apply to it unchanged.

**Storage and cache mode.** Shared, with the default cache mode, is Apple's
answer for data the CPU fills and updates
(`[[source: apple-storage-mode-apple-gpus]]`). Write-combined is for memory
the CPU "writes into, but never reads" and "Read actions may perform very
poorly" (`[[source: apple-cpucachemode-writecombined]]`). The core reads its
own grid, the `RowExtent` last-cell test for one, so write-combined is wrong
for it. `setVertexBytes` is for "single-use data that's smaller than 4 KB"
(`[[source: apple-setvertexbytes]]`): an 80 KB screen is out, but a 200-600 B
row table is in. A buffer that is modified between bind and execution must not
be marked immutable (`[[source: apple-mtlmutability]]`).

**Numbers Apple does not give.** Apple gives no figure for the commit-boundary
flush, for event overhead or for what a frame-sized copy costs. The CPU guide
gives the Memory Cache, "shared by all agents on the chip that can access DRAM
directly, including the GPU", with 128-B lines and hits "in the range of 35ns"
against "95ns" to DRAM (`[[source: apple-cpu-guide]]` §4.6.5, p. 121). No
Apple text explains September's 30-60 us gap.

## 5. What other terminals do

None of the five lets the GPU read live terminal state. Each copies at least
once a frame. They solve two separate problems separately: the parser and the
renderer both reading the terminal (a lock, or one thread), and CPU writes
against GPU reads (per-frame buffers, or the GL driver's implicit sync).

| project | what the GPU reads | parser against renderer | CPU against GPU | copied a frame, at 200x50 (arithmetic from the types) |
|---|---|---|---|---|
| ghostty | instances it builds | a mutex with a demand handoff; dirty rows copied under it | 3 slots on Metal, a semaphore, released on completion | up to 80 KB under the lock, then the whole screen's instances to the GPU, ~40 KB of backgrounds plus 32 B a glyph |
| kitty | its own GPU-layout cells, copied | one thread: parse, then render | the GL driver | 200 KB of cells plus 10 KB of selection, on any changed frame |
| alacritty | instances | a fair mutex; a resolved list built under it | the GL driver | 36 B an instance a non-empty cell |
| wezterm | quads | quads built under the terminal mutex | 3 rotated buffers, no fence; on wgpu a new buffer each frame | 272 B a quad |
| Zed (GPUI) | scene primitives | a full clone under the lock | a pool released on completion, no cap on frames in flight | unmeasured |

* **ghostty** used to clone the screen each frame and stopped: "the clone time
  was repeatedly a bottleneck blocking IO"
  (`[[source: ghostty-d9840f3-render-state]]`, lines 28-33). It now copies
  only dirty rows under the lock and rebuilds only dirty rows of its
  instances. Its upload to the frame's buffer is still the whole screen
  (`[[source: ghostty-generic-d9840f3]]`, lines 1758-1760 and 2603-2608). Its
  slots are 3 on Metal with a semaphore (`[[source: ghostty-renderer-metal]]`,
  line 37; `[[source: ghostty-generic-d9840f3]]`, 300-310), and its frame
  buffers are write-combined because the CPU never reads them
  (`[[source: ghostty-renderer-metal]]`, 263-275;
  `[[source: ghostty-renderer-metal-buffer]]`). The parse thread yields to the
  renderer's snapshot for at most 1 ms, which ghostty calls "microseconds"
  (`[[source: ghostty-d9840f3-renderer-state]]`, 44-52). That figure is
  ghostty's own claim and is unmeasured here.
* **kitty** stores cells already in GPU layout, 20 B each
  (`[[source: kitty-6d5d0c4-line-h]]`), behind a row-index map, so its scroll
  is baud's (`[[source: kitty-6d5d0c4-line-buf-h]]`). It is the nearest to
  reading the grid in place, and it still `memcpy`s every row into the GL
  buffer on any changed frame (`[[source: kitty-6d5d0c4-screen]]`, 3892-3895
  and 4116; `[[source: kitty-6d5d0c4-shaders]]`). Parse and render run back to
  back on one thread (`[[source: kitty-6d5d0c4-child-monitor]]`, 1547-1560).
* **alacritty** collects the renderable cells under the terminal's fair mutex
  and drops it "as early as possible"
  (`[[source: alacritty-v0151-display-mod]]`, 760-764;
  `[[source: alacritty-v0151-sync]]`), then uploads instances outside the lock
  (`[[source: alacritty-v0151-glsl3]]`).
* **wezterm** builds quads under the terminal mutex
  (`[[source: wezterm-5046fc2-localpane]]`;
  `[[source: wezterm-5046fc2-render-pane]]`) into a `TripleVertexBuffer` of 3
  buffers rotated with no fence (`[[source: wezterm-5046fc2-renderstate]]`,
  326-332).
* **Zed** clones every viewport cell under the lock
  (`[[source: zed-347f171-terminal-alacritty]]`, 904-911). Each frame it takes
  a 2 MiB shared, write-combined buffer from a pool and returns it from the
  completion handler, with no semaphore
  (`[[source: zed-347f171-metal-renderer]]`, 57-111 and 562-608).

baud's 80 KB of `PackedCell`s into the free set is the smallest per-frame
transfer of the six. ghostty matches it only on the half under its lock, and
only when every row is dirty. baud's copy is not under a lock at all: the core
is `&mut self` on the main thread, and the copy runs between feeds.

## 6. Three designs that would make the GPU read the ring

### 6.1 Copy-on-write rows behind a row table, pinned by the frame in flight ("epoch-live")

**The design.** The cells move into a pool: the primary ring, the alternate
rows and 150 spare rows (`MAX_GRID`'s rows, enough because D14 pins at most
one window). Each surface gets a row map, one `PoolRow` per ring slot, 2,300
entries and 9.2 KB, with a CAPTURED bit. A read goes from the slot (today's
conditional subtract) to `map[slot]`. A write goes through one gate, which
tests CAPTURED and, on a hit, calls a cold copy-on-write. That takes a spare,
copies the row up to its extent (nothing when the caller is about to blank or
fill the whole row), repoints the map and retires the old pool row.
`RowExtent`, `Book::past` and image anchors stay indexed by slot, because they
describe the line, not where its bytes sit. The map is carried as one pointer
through `View` and `Paint`. That is the shape that recovered the `clusters`
field's cost on this core, where three words had cost 4.2% on asciiquarium and
2.9% on cava (`baud-core/src/paint.rs`). At commit the app calls a capture
that pins the window's rows and returns their pool rows as a 200-600 B table,
passed with `setFragmentBytes`. On landing it calls a release that frees the
retired rows. The shader reads `cells[rows[dr] * stride + col]`. The status
row, the leaving row and any bidi-reordered row become one-row copies into
scratch rows each set owns.

**Per byte.** Cell stores are unchanged, except that a row rewritten while its
frame is in flight is copied once. On `cat` that is about the cursor row, 1.6
KB a frame. On a TUI redrawing the alternate screen it approaches a full
window, but only during commit-to-completion, which is about 0.35-0.48 ms of
an 8.3 ms frame. Every read of cells in the core gains a dependent load
(`map[slot]`), and every write a test and branch. That includes the `fill_row`
last-cell test, the cluster attach that reads the previous cell,
wide-character fix-ups, selection and scrollback. `put` pays it per cell, the
run writers per run. btop dispatches a control sequence about every 17 bytes,
so that is about one load per 17 bytes there.

**Per frame.** It removes the 80 KB window copy, 2.8-3.2 us measured, and the
two sets of 483,200 B each. It adds about 50 map loads, read-modify- writes
and table stores at capture, up to 50 frees at landing, sub- microsecond by
operation count, plus the status, leaving and reordered rows. Spare and
scratch memory comes to about 0.5 MB against today's 966,400 B of sets, more
if bidi scratch is reserved for a whole window per set. The GPU gains one load
from a small table and loses the per-cell modulo. Section 3 found no GPU
difference between live grid and copy, so no GPU gain is expected.

**Race freedom.** Every pinned row was written before the commit that names
it, and none is written again until the app has observed completion through
`Gpu::in_flight`. That load is Acquire against the handler's AcqRel decrement
(`baud-present-macos/src/metal.rs`, around lines 1177 and 1832). That meets
the commit-boundary rule in section 4. The table is copied by Metal at encode,
so the live map is never read by the GPU. The CPU does write other rows of the
same buffer the GPU is reading. That is outside Apple's prose and inside its
Game template's practice. On this host the row pitch is an odd number of 128-B
lines (`store_stride`, `baud-domain/src/cache.rs`; `hw.cachelinesize` = 128;
400 cells = 25 lines), so no line holds both a pinned row and one being
written. That rests on Metal allocations being page-aligned, which holds in
practice and is not documented. Soundness of the GPU read now rests on safe
core code gating every write, which `#![forbid(unsafe_code)]` cannot check. A
missed gate is a torn frame, drawn silently.

**Corrections the judge made, each checked in the source:**

* A window resize does *not* wait for the frame in flight, on purpose.
  `relayout` (`baud-app/src/app.rs`, around line 11513) records that it used
  to, that the wait was "the freeze the owner reported", and that since D4
  "the reflow has nothing to collide with -- the frame in flight is reading
  its own buffer". Under 6.1 a reflow during flight must therefore
  copy-on-write the whole window. Waiting again is not an option, and neither
  copy-on-write design priced it.
* The race oracle proposed for it, a GPU waiting on a shared event while the
  CPU writes, needs post-commit writes to be visible. That is the open
  question in section 4, so it cannot reliably fire. The proof that maps onto
  Apple's rule is a CPU-side property: the rows the frozen table names do not
  change until landing.
* It does not carry over to a discrete GPU under Vulkan. There the core's
  memory would have to be host-visible and read over PCIe on every pixel, or
  else staged, which is the copy again.
* It puts capture and landing, frame semantics, into the `no_std` core's API.

### 6.2 Generation tags at row entry ("core-ring")

This is the same family: a pool, a row table and a check at row entry. The
check is a per-row generation tag compared against the last landed frame
instead of a bit. The tags extend to N frames in flight more cleanly, which is
its one advantage. Its costs and race argument are 6.1's. The judge ranked it
below 6.1 on specification:

* It proposes deferring reflow to landing, which would bring back the freeze
  the owner reported.
* It caches the pool row in `Paint` across a pin, which would bypass the gate.
* Its line-sharing risk assumed 64-B lines, which is wrong on this host.
* Its pixel oracle compares only the final image, which cannot see a race.

### 6.3 Each frame slot keeps its own rotation and copies only changed rows ("dirty-rows")

**The design.** Nothing in the core changes. Each set's copy becomes a ring of
`vrows` rows with its own rotation, and the status, notice and leaving rows
sit after it, outside the modulo. When the screen epoch, the window offset,
the size and the stride all match what the set was last written from,
`Term::moved_since` turns a scroll into a rotation of the slot. Then each row
is compared with what the slot holds and copied only if it differs. Otherwise
every row is copied, as today. Correctness does not depend on the rotation
guess: after the pass, every slot row equals its view row under the rotation
the shader will use, so a wrong guess only costs copies. It stays inside
Apple's documented pattern: separate instances, written only while no
committed frame holds them, all writes before the commit.

**Per byte.** Zero. No core code changes.

**Per frame.** On a frame where only the cursor changed, the copy is skipped
today and stays skipped. Under full churn every row is new every frame, so the
saving is zero. That is any line under about 1.1 KB at 55 KB a frame ingested,
which is 198.6 MB/s times about 275 us. On a frame with a few changed rows,
writes fall to the changed rows, but a clean row costs two full reads. Both a
compare and a copy fetch the slot lines the GPU last read, so on unified
memory the net is about zero to 1.5 us a frame, inside the noise. Rows that
differ only in their last cells, such as a clock at the right edge, cost up to
about 1.5x today's bytes.

**Its value is elsewhere.** On a discrete GPU, staging only the changed rows
saves PCIe bandwidth, and the self-correcting compare is exactly the property
that port wants. The judge parks it for the Vulkan port.

### 6.4 What none of them is

* **Three grids in the core**, D4's literal "ring of grids", is either the
  copy moved into the core (catch up the next grid at each frame) or three
  times the cell stores (write every grid). It removes no copy.
* **The gated in-place read** is the 2026-09-14 design, measured in section 3
  at 345-475 us of parser stall a frame.
* **Copy-on-write of the grid's pages by the VM** faults and copies whole 16
  KB pages and needs a new no-copy buffer each frame, so it is never cheaper
  than copying the rows touched. That is argued, not measured.

### 6.5 The break-even, by arithmetic

The copy is bounded by the refresh rate. A per-byte tax grows with throughput.
Take the measured copy at 3 us a frame, under load: at 120 Hz that is 360 us
of CPU a second. Parsing at R MB/s costs R / 1425 of a core on cat-scroll's
memory-column rate (`MEASUREMENTS.md`). So a tax *t* on parsing costs more
than the copy above R = 360e-6 x 1425 / t MB/s:

| tax *t* | break-even on cat-scroll | at the PTY's 198.6 MB/s |
|---|---|---|
| 0.26% | ~197 MB/s | about even |
| 0.3% | ~171 MB/s | loses |
| 1% | ~51 MB/s | loses ~4x |
| 3.5% | ~15 MB/s | loses |

Three precedents:

* **One store at row entry** measured +0.3-0.6% instructions retired on cat,
  btop, vim and less (`baud-core/src/paint.rs`). That is instructions, not
  time.
* **The row map** is the record's "The row map was built, measured at +21%,
  and withdrawn". It is weaker than the "-7% on cat-scroll" its headline
  gives: cat-scroll read 1678-1742 against 1490-1680, ranges that overlap, so
  by CLAUDE.md that is no result. Only cat-cjk was disjoint, 668-674 against
  657-661, about -2%. The same entry says a thin fixed-size reference
  "recovered most of it", and that is 6.1's carrying shape. It was measured on
  2026-08-30, a shape the current `View` has moved on from.
* **The `clusters` field**, on this core: 4.2% and 2.9% as three words,
  recovered as one pointer.

**So the tax of 6.1 on today's `View` is unmeasured.** The precedents say it
is at or above break-even at the PTY ceiling. These are 200x50 figures. If the
copy scales with the screen, a drawn 400x150 window copies about 18 us, a
linear estimate that is not measured, and the bar rises to about 1.5%. The
whole stake is small: about 0.04% of one core at 200x50.

## 7. The judge's ranking

1. **Today's copy, with the slack taken out.** None of it costs anything per
   byte.
2. **Copy-on-write rows with a CAPTURED bit (6.1).** It is the only zero-copy
   design worth a ceiling arm, and only once its resize handling copies the
   whole window instead of waiting.
3. **Dirty rows (6.3)**, deferred to the Vulkan discrete port.
4. **Generation tags (6.2)**, dominated by 6.1.

The judge found three gaps in today's code while checking the designs:

* `Wheel`'s `held` and `next` words use `Relaxed` throughout: the `building`
  load, the `committed` store and the `landed` compare-exchange
  (`baud-present-macos/src/journal.rs`). This is a gap in the language's
  memory model, not a tear anyone has seen. The fix costs nothing per byte:
  Release on the landing, Acquire in `building`. Fixed (section 10).
* Several comments still describe the in-place read and the gated feed:
  * `metal.rs`'s module doc ("what makes the zero-copy grid safe"), the `lend`
    safety text, and the "keep incoming bytes queued" comment;
  * `app.rs` near lines 1229 and 2003-2015, where "`pump` does hold the
    barrier" is false since `800694c`;
  * the `Reshape` doc in `journal.rs`;
  * `frame_gpu.rs`'s header;
  * the `CellMemory.copies` doc, which still cites the 30-60 us.
* The bench defect in section 3.

## 8. Recommendation, and the measurement that confirms it

**Keep the copy. D4 stands, with its reason restated.** The GPU cannot read
the core's ring, because the core rewrites the rows it would read. Every way
to stop that puts a cost on every byte, and the copy's cost is fixed per
frame. In order:

1. **Fix the instrument.** Land the two-hunk `frame_gpu` fix (section 3), and
   add an `app` arm with the app's shape: two sets, `Screens::is_last`, the
   copy, `Done::Notify`, no spin before the feed.
2. **Re-run September's comparison idle**, with nothing else running, using
   the fixed bench. It decides whether the 30-60 us GPU gap and the ~1 us copy
   were real. Today's overlapping ranges and 3 us were both taken under load
   4-6. Until it runs, both September figures stand as recorded, with "not
   reproduced under load" beside them.
3. **Take out the slack.** Remove the `CellLast.cells` refill and the cells
   half of `alike`. Carry the app's `Built::Last` instead, made relative to
   the last *committed* frame: `screens.last` is set before a draw that may be
   refused as Busy. Fix `Wheel`'s ordering. Rewrite the stale comments. Prove
   it with the pixel oracle and the `app` arm.
4. **Measure the tax, so the decision is not an argument.** Build an `ingest2`
   ceiling arm, selected by `cfg`, that carries 6.1's pool pointer through
   `View` with the identity map, reads through `map[slot]`, and compiles in
   the CAPTURED test, which never fires. Run cat-scroll, cat-cjk, btop, vim,
   cava, asciiquarium, and less-pager at 400 reps. Follow CLAUDE.md's
   protocol: `git archive` base, change and an A/A copy into one path, `touch`
   every file, `cmp` the binaries, rotate the arms, at least four rounds with
   the first dropped, on mains. `cells_written` must be identical on every
   arm.
5. **Measure C**, the copy per frame, with the `app` arm at a drawn 200x50 and
   a drawn 400x150.

**The decision rule.** 6.1 is licensed only if, on every recording, *t* x
(198.6 MB/s / that recording's memory rate) < C x 120 a second. At C = 3 us
that means *t* below about 0.26% on cat-scroll. At 200x50, a *t* that
`ingest2` cannot resolve counts as a loss, not a pass. If the ceiling arm is
flat within the A/A spread and C at 400x150 is at least about 15 us, build 6.1
for large windows, gated by the pin-audit property at `PROPTEST_CASES=20000`
with a red run on a build that writes in place, plus the full workspace tests
and `scripts/check-features.sh`. Otherwise the copy stands, and the record
says so with both tables.

**What would prove this recommendation wrong:** the ceiling arm flat within
the A/A spread on every recording *and* a drawn 400x150 copy at or above 15
us. Neither has been measured.

## 9. Open

* **Whether 6.1 conforms.** Apple's prose is silent on a CPU writing one
  region of a buffer while the GPU reads another, and Apple's Game template
  does exactly that at 256-B slots. Whether that counts as conforming is a
  departure call, so it goes to Jev at 0.9 both ways, and only if step 4
  licenses the design.
* **Whether writes after a commit are visible to a pass that waits on a
  CPU-signalled shared event.** It is unresolved in Apple's text. A test on
  one machine cannot settle a guarantee, and nothing here depends on it.

## 10. The follow-through, measured (2026-10-24)

Steps 1, 3 and 5 of section 8 are built. Step 2, the idle re-run, could
not be taken idle: every round below overlapped other sessions' builds, at
load 7.5-31.8 (logged before each run), on mains when checked after,
the power source not logged per run. Step 4, the ceiling arm for the
tax, is not built.

**What changed.** `frame_gpu`'s live arms bind the alternate screen's
buffer while it is up, and `FRAME_HASH=check` fails the run unless every
cell arm's last screen, drawn once more whole, hashes to the copied
arm's. Before the fix it failed on btop, vim, asciiquarium and cava;
after it, all six recordings agree at 200x50 provisioned at 200x50 and
400x150, and drawn at 400x150. The window copy and the record of which
screen each set holds moved into `baud-present-macos` as `Screens::take`,
and the new `app` arm runs it through a `Presenter`: the wheel's two
sets, three IOSurface targets, a commit with a completion handler, every
read fed while a frame is in flight. Its last surface hashes equal to the
copied arm's on all six recordings, drawn at 200x50 and 400x150.
`CellInput` carries the screen its grid holds, so the GPU no longer
keeps `CellLast.cells` or compares a frame's rows; the token is
kept only on a commit, so a frame refused as busy is not compared
against. The wheel's free is `Release` and its look `Acquire`.

**How it was run.** Three binaries built from `git archive` at one path:
base `d7ec0023` (the app arm, the old comparison), change `c0c751e1`, and
an A/A rebuild of the change, byte-identical to it. Five outer rounds with
the arm order rotated, the first dropped; four inner rounds a run, the
first dropped: n = 12 a cell. The app arm's reads arrive at 20 MB/s
(`FRAME_APP_MBS=20`), so a replay gives tens of frames; its medians leave
out each round's first frame, which walks a cold map. Medians [range], us
a frame.

GPU start-to-end, the change binary:

| recording | live, 200x50 | copy, 200x50 | live, 400x150 | copy, 400x150 |
|---|---|---|---|---|
| btop | 165.9 [164.4-169.6] | 166.9 [164.6-170.1] | 606.7 [602.9-610.2] | 609.1 [602.2-614.5] |
| vim | 161.7 [161.2-172.1] | 161.6 [161.3-175.2] | 794.0 [791.0-810.5] | 800.5 [792.0-811.6] |
| asciiquarium | 284.9 [281.7-289.7] | 285.8 [280.8-293.4] | 588.1 [586.6-597.9] | 591.1 [588.1-599.0] |
| cava | 171.1 [168.8-275.4] | 168.4 [165.7-173.7] | 591.5 [588.3-599.4] | 590.3 [588.6-596.4] |
| cat-scroll | 287.5 [128.8-291.9] | 287.9 [127.7-292.1] | 608.9 [603.8-613.0] | 609.2 [604.0-613.2] |
| cat-cjk | 288.1 [154.3-296.4] | 288.6 [125.1-292.5] | 609.8 [601.2-610.9] | 604.3 [600.2-612.6] |

**Live and copy overlap on every row at both sizes, so there is no GPU
result**, now with the live arms drawing the right screen. September's
30-60 us gap is not reproduced, under load.

The copy, a frame:

| recording | `copied` arm, 200x50 | `app` arm, 200x50 | `copied` arm, 400x150 | `app` arm, 400x150 |
|---|---|---|---|---|
| btop | 1.4 [1.2-2.0] | 2.5 [2.1-3.8] | 6.1 [5.4-7.4] | 9.4 [6.8-14.0] |
| vim | 1.8 [1.3-2.1] | 2.5 [1.6-3.6] | 6.0 [5.4-7.9] | 10.6 [6.5-21.5] |
| asciiquarium | 1.9 [1.5-2.4] | 2.8 [1.8-4.0] | 6.1 [5.5-11.2] | 8.6 [7.5-14.6] |
| cava | 1.6 [1.3-2.3] | 3.2 [2.6-5.2] | 5.8 [5.4-14.6] | 10.1 [7.2-13.1] |
| cat-scroll | 1.7 [1.4-2.3] | 2.6 [1.9-3.8] | 6.0 [5.5-14.3] | 11.4 [8.2-21.1] |
| cat-cjk | 1.9 [1.4-2.2] | 2.5 [1.8-3.7] | 6.4 [5.5-9.3] | 10.2 [7.5-25.6] |

**A drawn 400x150 copy is 5.8-11.4 us by median on the change binary**,
480,000 B. The `app` arm's figure includes the comparison with the last
screen and the status row, which the `copied` arm's loop does not. The
`copied` arm's loop is the same code in the base binary and read 7.1-14.8
us there, cava's the highest, so the figure moves with the code around
it, and ranges reach 26 us on the change binary's `app` arm (one
base-binary round of the `app` arm on cat-cjk reached 9,176 us).
Section 8's rule licenses 6.1 only with a flat ceiling arm *and* a
400x150 copy of at least about 15 us. The copy stands: every 400x150
median on the change binary is under the 15 us bar, 5.8-6.4 on the
`copied` arm and 8.6-11.4 on the `app` arm, the base binary's same loop
read up to 14.8, and step 4, the ceiling arm, is not built.

The bind and commit a frame, where the comparison and the refill were,
base against change, the A/A beside it. The `app` arm runs the same bench
code in both binaries. The `async` arm does not: the change also moved
the bench's own screen comparison for its live arms into `Screens::take`,
so its columns are not the library change alone.

| recording | `app`, 400x150, base | change | A/A | `async`, 400x150, base | change | A/A |
|---|---|---|---|---|---|---|
| btop | 45.5 [18.3-100.6] | 31.1 [10.6-80.1] | 28.6 [14.6-53.7] | 14.8 [8.0-65.5] | 10.8 [6.1-17.5] | 11.8 [6.4-16.8] |
| vim | 44.0 [19.0-99.4] | 35.3 [13.6-63.3] | 30.1 [17.4-51.1] | 14.9 [8.8-29.8] | 14.0 [8.9-17.2] | 13.8 [9.4-16.7] |
| asciiquarium | 54.9 [21.3-106.9] | 25.9 [15.2-60.8] | 59.0 [11.9-98.1] | 13.2 [6.2-31.4] | 8.1 [6.2-20.1] | 14.6 [6.3-37.1] |
| cava | 33.9 [19.7-85.2] | 19.6 [10.7-48.2] | 17.8 [10.5-56.3] | 13.8 [7.4-32.8] | 8.2 [5.7-15.9] | 11.2 [5.5-24.4] |
| cat-scroll | 44.4 [20.9-86.0] | 31.7 [11.6-90.8] | 40.5 [12.7-86.8] | 11.7 [5.2-58.3] | 9.1 [5.1-111.1] | 11.5 [5.3-55.2] |
| cat-cjk | 71.2 [19.7-144.2] | 41.8 [12.6-90.5] | 44.2 [13.2-93.3] | 13.4 [5.4-143.8] | 9.5 [5.9-26.4] | 11.6 [5.8-615.5] |

**The ranges overlap on every row, so the saving is unmeasured.** The
medians are lower on every row at 400x150, and the A/A medians, from the
same binary as the change, spread as far as the change moved them:
asciiquarium's 25.9 against 59.0. At 200x50 the medians go both ways.
What was removed is known by arithmetic: a whole frame's refill wrote
the drawn rows a second time, 480,000 B at 400x150, and a frame alike in
everything else read them twice more.

**What would have proven the changes wrong**: a hash that differs between
any cell arm and the copy (none did, on six recordings, drawn at 200x50
(provisioned at 200x50 and at 400x150) and at 400x150), a pixel of the
cursor-only oracle that differs from a whole draw (none, with a new step
that changes only cells; reading every screen as the last one fails it by
243 pixels), or a bind and commit slower on the change than the A/A spread
(not seen).
