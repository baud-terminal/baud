# P. The shader cache at launch: 350 ms cold, 12 warm, and what each remedy would buy

Row 1 of note N: "precompile the shaders (`.metallib`) and keep an
`MTLBinaryArchive`", against a research figure of ~50 ms per pipeline
chain on the first launch after an install or an OS update, unmeasured on
baud. This note is the measurement, taken before anything was built,
because the row's keep rule is 20 ms on launch and a remedy is only worth
its cost once the cost is a number.

## The instrument

`Gpu::new` builds the Metal device, compiles the shader source into a
library with `newLibraryWithSource`, and builds five render pipelines; it
runs on a probe thread that AppKit's initialisation overlaps, and the app
joins it once the window exists. Under `BAUD_STARTUP=1` the probe prints
what each step cost:

```
startup: gpu build: device D ms, library L, pipelines P, warm pass W, probe T
```

`scripts/shader-cache-bench.py` launches the same binary with the system's
shader caches in place and moved aside, interleaved, the first launch of
each arm discarded, and reports the medians of that line and of the wall
from spawn to the first present. macOS keeps the compiled shaders in two
per-user directories under `$(getconf DARWIN_USER_CACHE_DIR)`:
`com.apple.metalfe`, the front end's output, and `com.apple.metal`, the
per-GPU function and library binaries (`functions.data`, `libraries.data`).
A cold arm moves one or both aside for the launch and restores them after
it; that is what the first launch after an install or an OS update does,
and it is the only way to measure it without an install or an update.

## What it read

Apple M5 Max, macOS 27.0, on mains, seven launches an arm with the first
discarded, milliseconds:

| step | warm | cold, both caches aside | cold - warm |
|---|---|---|---|
| device | 9.3 [8.7-11.2] | 11.1 [9.5-11.5] | +1.8 |
| **library** (`newLibraryWithSource`) | 0.6 [0.5-0.9] | **301.9 [285.3-305.2]** | **+301.3** |
| **pipelines** (five `newRenderPipelineState`) | 1.1 [0.7-1.3] | **50.7 [48.0-56.5]** | **+49.6** |
| warm pass | 0.7 | 0.5 | -0.2 |
| the probe, whole | 12.2 [10.6-13.8] | 363.5 [343.7-370.5] | +351.3 |
| **wall to the first present** | **104.0 [97.0-108.3]** | **409.5 [393.1-420.1]** | **+305.5** |

The control holds: the device and the warm pass do not depend on the cache
and do not move. **A cold launch shows its window 306 ms later, and the
research figure undershot it by six times**: the 50 ms is the pipelines,
and the library compile in front of them is 300.

**The two caches split the cost three ways.** Moving only the front end's
cache aside changes nothing (library 0.6, pipelines 1.3: the finished
library in `com.apple.metal` is what serves a warm launch, and the front
end is consulted only when it misses); moving only the per-GPU cache aside
costs the library 81.8 ms and the pipelines 48.3, a 193 ms wall. So of the
cold launch's 350 ms:

| what | ms | where it is cached | what would spare it |
|---|---|---|---|
| the front end: source to AIR | ~220 | `com.apple.metalfe` | a precompiled `.metallib` shipped with the binary |
| the library's GPU binaries | ~82 | `com.apple.metal/<gpu>/libraries.data` | a binary archive, per GPU |
| the five pipelines' GPU binaries | ~48 | `com.apple.metal/<gpu>/functions.data` | a binary archive, per GPU |

**The cache keys on the shader source, not on the binary.** Every rebuild
of the day launched warm; a one-line comment added to the source and
nothing else made the next launch pay the library 76 ms (the pipelines
stayed at 0.8: the functions had not changed) and the one after it 0.5,
and reverting the comment was warm at once, the old entries still there.
So an app update with the shaders unchanged is a warm launch for every
user, a shader edit is one 76 ms launch for each, and a cold launch is
only ever a first install or an OS update.

## What Apple says, now that it is pinned

The measurement and the documentation agree on every point the row turns
on, and the documentation supplies the reasons.

* **A runtime source compile is two compiles.** "By default, your Metal
  shaders compile as a format called Metal intermediate representation
  (Metal IR) ... At your app's runtime, Metal compiles this bytecode to a
  GPU-specific binary for the host device. If you provide your shader
  functions as strings, they first compile to Metal IR on device, and then
  go through a secondary compilation for GPU" (`[[source: apple-metal-libraries]]`).
  That is the 220 and the 82 above, and "compiling Metal shading language
  source code is one of the most expensive stages in the lifetime of a
  Metal app" (`[[source: apple-metal-best-practices-functions-libraries]]`).
* **The cache is Apple's and it is the pipelines' binaries.** "Metal
  maintains a separate cache of pipeline states on behalf of each app that
  contains all compiled code; this cache is populated as compilation
  occurs" (`[[source: macos-sdk-mtlbinaryarchive-h]]`); apps "can not
  benefit from the Metal Shader Cache before it's been populated, so the
  upfront compilation time adds up potentially making the first time
  launch experience take longer than desired"
  (`[[source: apple-wwdc20-10615-transcript]]`). Where it lives and what it
  keys on are documented nowhere Apple writes; the location is two
  third-party sources and this box's listing
  (`[[source: gfx-rs-3716-comment-816575736]]`), and the key is the
  measurement above.
* **A binary archive is to be built at development time and shipped, not
  kept by the app** -- the header says so outright: "Updating a
  MTLBinaryArchive at runtime in a shipping app configuration is not
  recommended; such a scenario requires corruption resiliency, careful
  storage space management and may cache hard-to-reproduce errors. These
  kind of issues are handled transparently by the Metal maintained cache,
  therefore we recommend that MTLBinaryArchive is populated during
  development time and shipped as an asset"
  (`[[source: macos-sdk-mtlbinaryarchive-h]]`). And a shipped one is for
  "the same GPU, ... running the same operating system build. If there's a
  mismatch the Metal framework will fall back on runtime compilation"
  (`[[source: apple-wwdc20-10615-transcript]]`) -- against which WWDC22 says
  "Metal gracefully upgrades your binary archives during OS updates or at
  app install time" (`[[source: apple-wwdc22-10102-transcript]]`), a tension
  the two sessions leave open. A miss is never fatal: "if no archives are
  provided, or no archives contain the requested content, the pipeline
  state is created by compiling the code as usual"
  (`[[source: macos-sdk-mtlbinaryarchive-h]]`).
* **The compiler is a download.** "To build your Metal apps, download and
  install the optional Metal Toolchain for the platforms your app
  targets," with `xcodebuild -downloadComponent metalToolchain`
  (`[[source: apple-xcode-additional-components]]`); a library is built
  with `xcrun metal -c` and linked
  (`[[source: apple-metal-building-a-shader-library]]`), which is how
  ghostty builds and embeds its own and loads it with `newLibraryWithData`
  (`[[source: ghostty-metallib-step]]`).
* **The "~50 ms per pipeline" figure is arithmetic on one Apple demo.**
  WWDC20's Fortnite archive: "we go from spending one minute and twenty six
  seconds building pipelines to just three seconds" for "seventeen hundred
  pipelined state objects" on "a six core three gigahertz MAC mini"
  (`[[source: apple-wwdc20-10615-transcript]]`) -- 86 s over 1,700 is
  50.6 ms a pipeline, on an Intel machine, with no source compile in it.
  On this GPU a pipeline is ~10 ms and the source compile in front of the
  five is six times all of them together.

## What that decides about the row

* **A shipped `.metallib` is worth ~220 ms of the 306, and the proxy for
  it is measured** (and it is the form Apple's own guidance and ghostty
  take): the arm with the front end cached and the GPU binaries
  cold is exactly the launch a precompiled library gives -- 193 ms to the
  first present against 410. It needs the Metal compiler at build time,
  and this box does not have it: `xcrun metal` answers "cannot execute tool
  'metal' due to missing Metal Toolchain; use: xcodebuild -downloadComponent
  MetalToolchain". That download is a change to the owner's Xcode and is
  theirs to make; the shape once it exists is a generated
  `baud.metallib` committed beside the source with a hash of the source it
  was built from, gated the way this repo gates every generated table, and
  `newLibraryWithData` with the source as the fallback.
* **A binary archive the app keeps buys nothing the system cache does not
  already give.** The system cache survives rebuilds and keys on the
  source; the launches where it is cold -- a first install, an OS update --
  are the launches where an archive the app wrote earlier does not exist or
  is invalid too. A *shipped* archive would spare the remaining 130 ms on
  the GPU it was built for, and only that GPU family on that OS; for the
  owner's own build that is this machine until the next update, for
  anyone else it is nothing.
* **The remedy that needs no toolchain is not to wait.** The window is
  shown at 104 ms warm because the probe is joined once the window exists;
  cold, the join waits ~280 ms for the compiler. Presenting the first frames
  on the CPU path and adopting the GPU when its thread finishes would make
  a cold launch's first present the warm one's, on any machine, at the cost
  of a presenter switch after launch -- and the cell path lends the core its
  grid *from* the GPU's memory at construction, so that switch is a design
  change rather than a fallback, and is not taken here.

The keep rule is 20 ms; the cost is 306. The row stays open on the
toolchain, with its number.

## Re-measured on Xcode 27, and the block verified rather than quoted

**2026-09-19, macOS 27.0, Xcode 27.0 (27A266a).** The figures above were
taken on an earlier Xcode, and a shader-cache cost is exactly the kind of
number an OS update moves, so it was re-taken with the same instrument --
seven launches an arm, the first of each discarded:

| field | warm median | cold median | cold - warm | as recorded before |
|---|---|---|---|---|
| library | 0.6 ms | 277.6 | **+277.0** | +302 |
| pipelines | 1.1 | 45.1 | **+44.0** | +51 |
| probe, whole | 13.8 | 335.5 | +321.7 | -- |
| **wall to the first present** | **102.9** | **389.9** | **+287.0** | +306 |
| device (control) | 11.7 | 11.7 | **+0.0** | -- |
| warm pass (control) | 0.6 | 0.5 | **-0.1** | -- |

**The row's number holds and the two controls say the arms are what they
claim.** `device` and the warm pass do not touch the cache and do not
move; everything that does moves by two orders of magnitude. The cost is
6 % lower than it was, which is inside what this file would call drift
across an OS version rather than a finding.

**The toolchain is still absent, and it is verified three ways rather
than by one error string.** The `metal` binary *exists* --
`/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/metal`
-- and is a stub that answers *"cannot execute tool 'metal' due to
missing Metal Toolchain"*; no toolchain directory exists under
`/Library/Developer/Toolchains` or `~/Library/Developer/Toolchains`; and
`~/Library/Developer/Xcode/XcodeToMetalToolchainIndexMapping.plist` lists
every `metalToolchain` asset with affinity **`available`**, which is the
word for downloadable rather than installed.

**And the error this note quotes is not the one `xcrun` gives here, which
is a second masking worth writing down.** `DEVELOPER_DIR` on this box is
a nix `apple-sdk-14.4`, so `xcrun metal` answers **"tool 'metal' not
found"** and `xcodebuild` answers "tool 'xcodebuild' not found" -- the
same `xcode-select` shim this repo already records for `xctrace`. A
reader who retries the command in this note gets an error that reads like
*no Xcode at all* and would be wrong twice over: Xcode 27 is installed and
the missing thing is one component. **Reach the real tool by its full
path before concluding anything about it**, which is this repo's
positive-control rule applied to a toolchain instead of a search.

## The two remedies that need no toolchain, sized and both refused

The verdict above leaves "not to wait" as the remedy available without a
download. It is **not contained**, and the code says why rather than the
note asserting it.

* **A live terminal cannot change which memory it lives in.** `Term` is
  constructed *over* its storage exactly once, and the choice is the path:
  `Owned::storage_over(cells, alt_cells)` hands it the GPU's two shared
  buffers on the cell path, `Owned::storage()` hands it ordinary memory on
  the CPU and instance paths (`baud-app/src/app.rs`, the `match (request,
  probe)`). Presenting on the CPU and adopting the GPU later therefore
  means re-homing a live `Term` -- its ring, its alternate surface, its
  cursor, modes, book and cluster store -- and `baud-core` has no such
  operation. Adding one is a core API change in service of a launch that
  happens once per install.
* **It cannot be narrowed to "switch before anything is fed" either**, which
  would have avoided the re-homing. The probe lands at **335 ms** cold,
  measured above. The shell's first bytes are **not** measured cold and
  are derived: the pty is spawned at 19 ms, before AppKit and independent
  of the probe, and `zsh -l -i -c exit` on this box is 150-160 ms, so the
  bytes arrive around 170-180 ms whatever the GPU is doing. Both halves of
  that derivation are recorded launch figures; what is not taken is a cold
  launch printing the two marks together, and it is one run of the
  instrument above with `BAUD_CMD=shell` if anyone wants it pinned.
* **Splitting the library so the first frame's pipelines compile first is
  refused on the ratio.** The whole source is **226 lines, 9.3 KB, eight
  entry points**, and it takes 277 ms: that is the Metal front end's fixed
  cost rather than anything proportional to the text, so two libraries
  would pay it twice. **This is an inference from the ratio and not a
  measurement** -- what would settle it is a ceiling arm compiling a
  deliberately trimmed source and reading the cold `library` field, which
  needs no toolchain and has not been run.

**So the row is open on one command.** `xcodebuild -downloadComponent
MetalToolchain` is a change to the owner's Xcode; with it, the remedy is
the contained one this note already describes -- a generated
`baud.metallib` beside the source, hash-gated, with `newLibraryWithData`
and the source as the fallback -- and it is worth ~197 ms of the 287.
Without it there is no remedy here that is smaller than the problem.
