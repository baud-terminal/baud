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
