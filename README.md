# baud

<div align="center">

[![crates.io](https://img.shields.io/crates/v/baud?logo=rust&logoColor=white)](https://crates.io/crates/baud)
[![Downloads](https://img.shields.io/crates/d/baud?logo=rust&logoColor=white)](https://crates.io/crates/baud)
[![MSRV](https://img.shields.io/crates/msrv/baud?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![CI](https://img.shields.io/github/actions/workflow/status/i-am-logger/baud/ci.yml?branch=master&label=CI&logo=githubactions&logoColor=white)](https://github.com/i-am-logger/baud/actions/workflows/ci.yml)
[![docs.rs](https://img.shields.io/docsrs/baud?logo=docsdotrs&logoColor=white)](https://docs.rs/baud)

[![Nix](https://img.shields.io/badge/Nix-2b2b2b?logo=nixos&logoColor=white)](https://nixos.org)
[![devenv](https://img.shields.io/badge/devenv-2b2b2b?logo=nixos&logoColor=white)](https://devenv.sh)
[![License: CC BY-NC-SA 4.0](https://img.shields.io/badge/CC%20BY--NC--SA%204.0-2b2b2b?logo=creativecommons&logoColor=white)](LICENSE)

</div>

> A terminal emulator ingest core, and the measurements that produced it.

Named for Émile Baudot, whose 1874 code gave the unit its name. The lineage this
sits in runs teletype → glass tty → terminal emulator, and a baud is a measure of
how fast symbols cross a line — which is what every number below is about.

**This is not a terminal.** It is the byte path — PTY read through VT parse into
a grid — built from whatever measurement pointed at rather than from what read
nicely, plus the harness that produced those measurements. Rendering, font
shaping, and roughly 56 of the 66 VT operations a real terminal implements are
absent. See [Not done](#not-done) before believing any of the numbers mean a
faster terminal.

## What we're trying to build

A terminal emulator that is faster than the existing ones on all four axes at
once — ingest throughput, input latency, frame rate under full-grid churn, and
memory — rather than winning one and losing the rest. No shipping terminal holds
all four today, and the measurements below suggest the reason is not a hard
tradeoff. It is that each project committed to one architecture early:

| | foot | kitty | ghostty | alacritty |
|---|---|---|---|---|
| parser | scalar C | scalar C | **SIMD** (Zig) | scalar (`vte`) |
| event loop | epoll reactor | poll reactor | **libxev / io_uring** | winit + `polling` |
| render | CPU raster | GPU | GPU | GPU |
| grid | cell array | cell array | cell array | cell array |

Nobody has combined byte-span storage, lazy viewport materialisation,
damage-adaptive dispatch between the CPU and GPU render paths, and an
unthrottled present. Each piece exists somewhere; the combination doesn't exist
anywhere. That gap is the thesis.

## The one invariant

Every result in this repo is predicted by two things, and neither is the thing
people usually optimise:

1. **Write amplification ≤ 1.** Bytes written to memory per byte ingested.
2. **Working set ≤ cache.** The structures touched per byte must stay resident.

Allocation is a *proxy* for these, not the thing itself. An immutable flat log
that allocates nothing still cost 22%, because the array grew past cache.
Content addressing is nothing but indices, and lost 9–23% even at a 100% dedup
hit rate, because the table it indexes is bigger than what it replaces.

## What's measured

Single-core virtualised Xeon @ 2.1 GHz, 3 GB RAM, kernel 6.18, L2 2 MiB.
Absolute figures are not bare metal; the ratios are the finding. Re-run on your
hardware before quoting anything.

### The kernel is the ceiling, and it is not reachable

| | MB/s |
|---|---|
| pipe, same two processes | 8155 |
| SIMD scan, isolated | 3383 |
| scalar state machine, isolated | 596 |
| **kernel PTY, raw, null reader** | **569** |
| kernel PTY, `n_tty` cooked | 223 |

The tty layer costs **14× a pipe** for the same bytes between the same two
processes. A naive byte-at-a-time state machine already saturates it, so parser
speed is not the binding constraint — SIMD scanning is 6× past the wall.

`read(2)` returns **at most 4095 bytes** regardless of the buffer you offer,
because `N_TTY_BUF_SIZE` is 4096 (`drivers/tty/n_tty.c:59`) and it is a fixed
array embedded in `struct n_tty_data`. Offering 1 MiB (as alacritty does) gets
you 4095. That is 4,229 syscalls per 16 MB.

Not reachable from userspace: eBPF cannot resize a kernel struct or skip the
`queue_work()` hop; `splice()` measured *worse* (297 vs 546 MB/s); no sysctl
exists. `SCHED_FIFO` made p99 latency **50% worse**.

### Latency is the display, not the kernel

```
PTY write → echo → read, 20k samples
  p50 5.7µs   p99 12.3µs   p99.9 31.9µs   max 89µs
```

Against a 16,667µs vblank quantum, the entire kernel round trip is **0.04% of
the input-latency budget**. Latency is won at `wp_tearing_control_v1`, VRR,
direct scanout, and higher refresh — not in the terminal.

### Representation is the whole game

| grid representation | % of kernel ceiling | memory written per 256 MB in |
|---|---|---|
| 16-byte cell `{cp,fg,bg,attr}` | 36% | 3.96 GB |
| 8-byte cell `{cp,style_id}` | 48% | 1.98 GB |
| 4-byte codepoints + style spans | 50% | 0.99 GB |
| byte runs into an arena | 68% | 0.25 GB |
| **+ zero-copy, 1 MiB arena** | **80%** | 0 |

Arena size sweep: 256 KiB → 458, **1 MiB → 479**, 8 MiB → 472, 64 MiB → 441.
Zero-copy is *slower* than copying if the buffer isn't resident.

### Things that lost

| | vs baseline |
|---|---|
| content addressing, 100% dedup rate | −18% |
| content addressing, unique lines | −23% |
| immutable flat log, windowed arena | −22% |
| immutable flat log, true history | −31% |
| persistent grid, structural sharing | −58% |
| full cell snapshot per line | −93% |
| fixed-stride A×B matrix (4 B/cell) | −20% |

Immutable *lines* work. Immutable *grids* do not — the grid is derivable.

### Render is not the constraint

| | per frame | max fps, one core |
|---|---|---|
| CPU raster, full damage (DOOM-fire case) | 2.00 ms | 501 |
| GPU instance build, full grid | 0.025 ms | 40,783 |

foot achieves 199 fps and kitty 273. Neither is render-bound; both are bounded
by the present path. foot's `render.c:5165` gates on `frame_callback == NULL`
with the callback created inside the render, so it cannot structurally exceed
vblank. Viewport→GPU transcode costs 0.19% of a core at 240 fps — there is
nothing there worth buying with 20% of ingest.

### Against a real terminal, same box, same payload

alacritty's `Term` + `vte`, benchmarked headless. **It is fed from memory with
no PTY at all; baud pays syscalls and three kernel copies.**

| payload | alacritty_terminal | baud | ratio |
|---|---|---|---|
| streaming text | 51.7–87.3 MB/s | 201.1 | ~2.3–3.9× |
| TUI redraw | 126.0 MB/s | 159.4 | **1.3×** |

Those `baud-core` figures are *after* the port to `baud-domain` types, which added real
UTF-8 decoding, wide-character handling, and an 8-byte cell holding a full
codepoint. The pre-port numbers (278.5 / 230.1) were measured against a core
that advanced the cursor one column per UTF-8 continuation byte and truncated
`U+1F680` to `U+F680`. They were faster and wrong.

The margin is workload-dependent and that is the thesis confirming itself:
alacritty is *2.4× faster on TUI than on streaming* because TUI payloads are
mostly escape sequences and produce fewer cell writes. Its bottleneck is writing
its `Cell`. baud goes slightly the other way, having no per-cell writes to
save. Two measurements, opposite directions, one variable.

**alacritty implements 66 VT operations; `baud-core` implements about 22.** Some of
the gap is still work not being done — the differential test now matches on
13 of 20 cases, up from 1, and the nine that differ are named operations
(`CSI P`, `CSI @`, tab stops, scroll-region edge cases) rather than a fog. It also does damage tracking, selection,
vi-mode, and hyperlink tracking on every write, which baud does not.

## What of the protocol is implemented

`docs/vt-coverage.md` is the answer, enumerated against primary sources rather
than summarised. It carries a status per operation, and a row reaches **done**
only when a test exists that would fail if the operation regressed.

The status worth knowing about is `parsed`: a sequence the parser recognises and
then **drops**. That is worse than one it does not implement, because the
terminal accepts it, does nothing, and gives the application no way to find out.
Three operations sat in that state — DECSTBM, DL and SM/RM — while this file
advertised "~22 operations".

Implemented since: scrolling regions and the operations that share them (DL, SU,
SD, IL, IND, RI, NEL, LF with real LNM state); cursor positioning (CNL, CPL,
CHA, HPA, VPA, HPR, VPR, SCOSC, SCORC); a real tab-stop table behind HT, HTS,
TBC, CHT and CBT; the C0 controls; four missing parser states, including the one
whose absence let an APC payload execute as commands; DECSTR; DECOM, DECAWM and
DECTCEM; and a reply channel carrying DA and DSR.

`docs/protocol-roadmap.md` recommends what to add next and what to decline, with
reasons for both.

Every claim traces to a source in `docs/research/sources/`, hash-pinned in
`MANIFEST.toml` and gated by `scripts/check-sources.sh`: xterm's `ctlseqs`,
Paul Flo Williams' DEC ANSI parser, ECMA-48, and the VT510 programmer
reference. Control functions are not implemented from memory here — the spec has
overruled confident guesses more than once, and the commit log says where.

## Layout

```
baud-domain/    #![no_std], no alloc, #![forbid(unsafe_code)]. The type system:
                every quantity is a type, every invariant the compiler can hold
                is held by the compiler.
baud-core/      #![no_std], no alloc. The ingest core, written in those types.
baud-bench/     std harness. Forks a PTY, generates payloads, times ingest.
baud-difftest/  Differential test against alacritty's `Term`.
```

`baud-domain` depends on `zerocopy` (derive only) so records crossing the mmap
and GPU boundaries have compiler-checked layout; proc macros don't ship in the
binary. `baud-core` depends on `baud-domain` and nothing else. Its `alloc`
feature adds `Storage::boxed()` for std callers and is off by default. Both
build for `x86_64-unknown-none`.

The core builds for `x86_64-unknown-none`. Storage being a caller-owned
`&mut Storage` is the point, not ceremony: the arena is a `&mut [u8]`, so it can
equally be a shared-memory mapping another process writes into — which is the
only route that removes the tty layer from the hot path.

## Development

The toolchain is pinned by [devenv](https://devenv.sh) — Rust **nightly**, plus
the `x86_64-unknown-none` target the `no_std` gate builds against.

```bash
direnv allow   # or: devenv shell

dev-ci         # full pipeline: treefmt + clippy + check + test + audit
dev-test       # tests
dev-bare       # the no_std gate: build baud-core for x86_64-unknown-none
dev-audit      # the type-system gate (audit_all.sh)
```

`dev-ci` is not an approximation of CI. `.github/workflows/ci.yml` runs
`devenv test`, which runs the `tasks."test:*"` set defined in `devenv.nix` —
the same suite `dev-ci` walks. Add a check by adding a task, never by adding a
step to the workflow, and the two cannot drift apart.

See [BUILD.md](BUILD.md) for what each gate proves, and [CLAUDE.md](CLAUDE.md)
for the constants that must not move without a measurement.

## Running

```sh
cargo run --release -p baud-bench              # real PTY, median of 9
cargo build --release -p baud-core --target x86_64-unknown-none   # no_std proof
cargo run --release -p baud-difftest           # cell-by-cell diff vs alacritty
```

`baud-bench` builds and runs on Linux and macOS, x86-64 and aarch64 — but it
prints a warning off Linux and you should believe it. The tty layer is the thing
being measured, Darwin has no `n_tty`, and there is no SIMD scanner outside
x86-64. Nothing measured off Linux is comparable to [MEASUREMENTS.md](MEASUREMENTS.md).

`baud-difftest` needs an alacritty checkout whose path is set in its manifest,
so it is excluded from the workspace. `bench-others/` holds the
cross-implementation harnesses and their requirements.

## Not done

In rough order of how likely each is to invalidate the above:

- **Font shaping.** Partly measured now, and the shape of the risk changed:
  a warm glyph-cache hit is **1.1 ns**, a fontconfig fallback resolution is
  **245 µs** — a 220,000× cliff. Seventeen misses blow a frame at 240 fps. So
  the risk is not shaping being slow, it is the *miss path*. Warm ASCII×styles
  at startup from a persisted 10.5 KB table, resolve misses on a worker, and
  cache negatives.
- **Grapheme clusters.** `Width` is currently computed per codepoint, and the
  domain model says it should be per cluster. A variation selector changes the
  width of the *preceding* codepoint; a ZWJ family sequence is five codepoints
  rendering as one 2-column glyph. No per-codepoint table can get these right,
  including `unicode-width`.
- **The width table itself.** Hand-rolled, and measured **5.6% wrong across
  194,528 codepoints** against `unicode-width`. Replace with generated Unicode
  data.
- **The rest of the VT operations.** `docs/vt-coverage.md` enumerates them by
  tier with a status each; the count in this section predates that file. Several (`insert_blank_lines`,
  `delete_chars`, `erase_chars`, scrolling regions) are in-place mutations of
  completed lines — exactly what span storage is worst at.
- **Rendering.** No renderer exists. All render figures above are isolated
  cost models, not a running pipeline.
- **Reflow on resize.** Unmeasured, and the classic terminal hang. Byte spans
  should help — rewrapping a span is cheaper than rewrapping cells — but 10,000
  lines is still 10,000 lines.
- **Selection, search, OSC 8.** All need random access by `(line, column)` into
  variable-length spans, which needs an index that fixed-stride arrays don't.
- **Concurrency.** The publish protocol is free on the write side (measured
  214.9 vs 215.3), but the payoff — a render thread reading a published head
  lock-free — cannot be evaluated on a single-core box.
- **GPU submission floor.** No GPU available. That one number decides whether
  the renderer needs a CPU/GPU hybrid or can be GPU-only.

## Reproducing the comparison

The synthetic payloads in `bench/src/payload.rs` set the control-to-text ratio
arbitrarily, and the style-reuse sweep showed the template cache's payoff moves
with it. Capture real streams before designing around any percentage:

```sh
script -c "vim src/lib.rs" /tmp/vim.cast
script -c "htop" /tmp/htop.cast
```

Then replay those bytes through the harness instead of `payload::tui`.

## Releases

Automated with [release-plz](https://release-plz.dev). Commits follow
[Conventional Commits](https://www.conventionalcommits.org/); release-plz opens
a `chore: release` PR that bumps the version and writes `CHANGELOG.md`, and
**merging that PR is what authorises the publish to crates.io.** Versions are
never edited by hand — release-plz reads its baseline from crates.io, so a
manual bump or publish desynchronises it.

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Funding

Solo-maintained, and sponsorship buys maintenance time rather than obligations.
See [FUNDING.md](FUNDING.md) — including how to arrange a commercial licence,
which the NonCommercial clause below makes a separate conversation.

## License

Creative Commons Attribution-NonCommercial-ShareAlike (CC BY-NC-SA) 4.0 International

See [LICENSE](LICENSE) for details.
