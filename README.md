<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/baud-lockup-dark.svg">
  <img alt="baud" src="assets/logo/baud-lockup-light.svg" width="300">
</picture>

</div>

<div align="center">

[![Nix](https://img.shields.io/badge/Nix-2b2b2b?logo=nixos&logoColor=white)](https://nixos.org)
[![devenv](https://img.shields.io/badge/devenv-2b2b2b?logo=nixos&logoColor=white)](https://devenv.sh)
[![License: CC BY-NC-SA 4.0](https://img.shields.io/badge/CC%20BY--NC--SA%204.0-2b2b2b?logo=creativecommons&logoColor=white)](LICENSE)

</div>

> A terminal emulator written in Rust. It runs on macOS; the Linux port has run
> only headless.

Named for Émile Baudot, whose 1874 code gave the unit its name. The lineage runs
teletype → glass tty → terminal emulator, and a baud measures how fast symbols
cross a line — which is what this is about.

## What it is

baud implements the standards, specifications and protocols a terminal answers
to — ECMA-48, the DEC manuals, xterm's ctlseqs, the Unicode Standard and its
annexes, the font formats, kitty's keyboard protocol — with the goal of running
them faster than any shipping terminal on ingest throughput, input latency,
frame rate under full-grid churn and memory **at once**.

**The standard is the requirement; speed is how it is met.** The work is the
standard compiled into tables indexed at build time, vectorised, and laid out to
stay in cache. A result that is faster because it departs from a standard is a
defect, not a win, and the departures known today are
[listed below](#not-built-and-open-departures) as such.

Only ingest is measured against other terminals today ([the number](#the-number)).
Latency, frame time and memory are baud's own numbers, on one Mac
([MEASUREMENTS.md](MEASUREMENTS.md)).

## Status

**A terminal emulator that runs on macOS, with a Linux port that has run only
headless.** The VT core is measured, and in its coverage table every row for
an operation it implements is done except nine at partial, DECSC, DECRC, the
DEC Special Graphics table, the `dcs_param` and `dcs_passthrough` states, GR
bytes under ISO 8859-1 in a sequence or an OSC, APC or PM body, the `escape`
and `escape_intermediate` states, and `DEL` in `csi_entry` or
`csi_intermediate`; the renderer, the shaper and the
input path are built and run on macOS.

| | |
|---|---|
| **VT protocol** | ✅ [enumerated against primary sources](docs/vt-coverage.md): every row for an operation baud implements is **done** except nine at **partial**: DECSC/DECRC, which save G0-G3 and which of them GL invokes but not GR (baud does not model it) or a single shift (SS2/SS3 are not implemented); the DEC Special Graphics table, which leaves `0x5F` as `_`; the `dcs_param` state, which collects `3A` as a parameter separator where `[[source: williams]]` (589) and DEC STD 070 ignore the string and ECMA-48 lets 03/10 separate only within a control sequence's parameter, for Jev; the `dcs_passthrough` state, which ignores the C0 other than `BEL` and the format effectors where `[[source: williams]]` passes them on (388) and DEC STD 070 recommends ignoring them, the texts conflicting; GR bytes under ISO 8859-1, read as their 7-bit bytes in a DCS prefix and not yet in an escape or control sequence or in an OSC, APC or PM body, where every text read reads them so, as xterm does in a sequence; and three rows for a `DEL` after `ESC` or `CSI`, the `escape` and `escape_intermediate` states and `DEL` in `csi_entry` or `csi_intermediate`, where baud drops the sequence and `[[source: williams]]` ignores the `DEL` and stays (286, 293, 306, 328). What it does not implement is **out** with the reason in its row, or refused and counted. Some operations have no row yet, among them SS2/SS3 and the locking shifts LS2, LS3 and LS1R-LS3R, which count as unsupported |
| **Parser** | ✅ all 13 DEC ANSI states, chunk-invariant, property-tested |
| **Text** | ✅ UTF-8 decoding against the standard's own vectors, width generated from `EastAsianWidth.txt`, and grapheme clustering that passes `GraphemeBreakTest.txt`. Marks past what the cluster store can hold are dropped and counted, an open departure (below) |
| **Grid** | ✅ one representation: cells in a ring the screen is a window into, so a scroll copies nothing and `Term::scrollback` reads the lines behind the window. `Depth::DEFAULT` is 2,000 lines, and `Term::resize` reflows the primary in place |
| **Throughput** | ✅ [ahead of ghostty on all nine recordings](#the-number) — 1.19–5.80× on committed capture |
| **Replies** | ✅ answers DA (as a VT102), DSR 5/6, DECRQM, DECRQSS and XTVERSION. A query baud will not answer is counted, each refusal for a named reason — a wrong-shaped answer is worse than silence — and an `OSC 52` clipboard read gets silence |
| **Input** | ✅ [enumerated against sources](docs/input-coverage.md). `baud_domain::encode_key` is the base layer, `modifyOtherKeys` and kitty's progressive enhancement in full, and `Term::key` reads the modes off the terminal and calls it. The app sends mouse reports and does stream and block selection and paste |
| **Rendering** | ✅ `baud-render` draws on the CPU and builds what `baud-present-macos` draws on Metal, by two paths a pixel oracle holds against each other. Every frame figure is this Mac's unified-memory GPU |
| **Shaping** | ✅ `baud-font` parses, shapes through GSUB, GPOS and morx, and rasterises with no engine dependency, exact against `hb-shape` on 480 fixtures, none of which separates per-script shaping from `latn`: it shapes every script as `latn` or `DFLT`, an open departure (below) |
| **History in the app** | ✅ the wheel, a scrollbar that fades and widens as AppKit's does, and selection across the history |
| **Linux** | ◐ ran headless under weston: no pointer input, nobody has looked at its pixels, and the GPU half is unstarted |

[The record](docs/record.md) holds where each of these was measured and
decided.

## What is next

In the order [CLAUDE.md](CLAUDE.md) leaves it ("Next"):

1. **Linux, to where a person can use it.** The seat binds only the keyboard,
   so there is no pointer: no wheel, no mouse reports, no selection. Then a
   look at a frame's pixels, which the headless run could not give, then the
   GPU half, Vulkan over dmabuf, on a machine with a GPU.
2. **macOS key releases and modifier keys.** They wait on `keyUp:` and
   `flagsChanged:` in the window crate; the core's side, kitty's event types,
   is done.
3. **A reset count the app acts on, then the core keeps the window over the
   history** — V1 and V2 of the owner's decided list,
   `docs/research/notes/v-open-decisions.md`.

The open departures below are not on this list because each is the owner's
call, with the options set out in the note it names.

## Running it

```sh
devenv shell -- scripts/bundle-macos.sh         # target/baud.app: the terminal, `baud`, as an app
devenv test                                     # the whole gate, as CI runs it
devenv shell -- dev-bench cat-cjk               # baud-core alone on one recording, in memory
```

`dev-bench` is `cargo bench -p baud-bench --bench ingest2 -- <payload> <reps>`,
the instrument [CLAUDE.md](CLAUDE.md) says to verify a core change with
("How to verify a change").

## Embedding the core

`baud-core` is a `no_std` library and is usable on its own — it is the engine
`baud-app` is built on, and any program that needs to interpret a VT stream
can embed it.

```rust
use baud_core::{BytePos, Owned, ReadLen, Term, WinSize};

// Storage is the caller's, and its size is the geometry: no allocator on
// the ingest path. `Owned` is the convenience behind the `alloc` feature;
// without it, `Storage::new` takes the same buffers as borrows.
let ws = WinSize {
    ws_row: 50,
    ws_col: 200,
};
let mut store = Owned::new(&ws);
let mut term = Term::new(store.storage(), &ws);

// Feed bytes exactly as they arrive from read(2) -- split anywhere. The
// core never copies on the parse path: a read writes into the arena and
// `feed` parses it in place.
let bytes = b"\x1b[1;32mhello\x1b[m";
term.write_at(BytePos::ZERO, bytes);
term.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

// A terminal is not write-only: drain anything it owes the host.
let owed = term.reply().len(); // e.g. the answer to a DSR
let cells = term.cells().len(); // the grid, ring and all
```

There is one representation: the core always materialises cells, into a ring
the screen is a window into. `Term::scrollback` reads the history behind the
window and `Term::resize` reflows the primary.

## The two quantities

Two quantities gate a change to the core:

1. **Write amplification** — bytes written to memory per byte ingested.
2. **Working set ≤ cache** — the structures touched per byte stay resident.

Allocation is a *proxy* for these, not the thing: "it doesn't allocate" is not
a defence. A cell core cannot hold write amplification to 1 and is not meant
to — one printable byte becomes an eight-byte cell, so 8x is a floor by
construction ([the record](docs/record.md), "The pure-ASCII rung is at 74% of
the machine's store bandwidth").

## The number

*Apple M5 Max, in-memory transport, medians of 9, interleaved and rotated.
Every core is driven with the same bytes and given 10,000 lines of scrollback.
Full tables, other hosts, and every caveat in
[MEASUREMENTS.md](MEASUREMENTS.md).*

| recording | baud-core | ghostty | alacritty | **vs ghostty** |
|---|---|---|---|---|
| `ascii/less-pager.raw` | **1684.5** | 290.2 | 241.2 | **5.80x** |
| `ascii/cat-scroll.raw` | **1425.1** | 424.3 | 203.3 | **3.36x** |
| `ascii/git-log.raw` | **1637.9** | 516.9 | 211.5 | **3.17x** |
| `ascii/asciiquarium.raw` | **524.2** | 197.2 | 243.6 | **2.66x** |
| `utf8/less-cjk.raw` | **498.0** | 247.2 | 271.3 | **2.01x** |
| `utf8/vim-unicode.raw` | **636.4** | 346.8 | 293.0 | **1.84x** |
| `utf8/cava.raw` | **554.1** | 318.9 | 362.2 | **1.74x** |
| `utf8/btop.raw` | **871.3** | 511.2 | 330.9 | **1.70x** |
| `utf8/cat-cjk.raw` | **459.4** | 385.1 | 221.7 | **1.19x** |

Ahead on all nine, **1.19x to 5.80x**.

**Re-taken 2026-09-20 with baud at the same 10,000 lines as every other core.**
The table before this one gave baud 2,000 -- the sentence above it said
otherwise -- and the two scroll-heavy ASCII rows read 11% and 19% higher for
it. The correction and the mechanism are in
[MEASUREMENTS.md](MEASUREMENTS.md).

**On this table alacritty is ahead of ghostty on three rows** — asciiquarium,
cava and less-cjk — and behind it on the other six.

**The table's history is in [MEASUREMENTS.md](MEASUREMENTS.md)**, beside the
same table: the NEON run scan's gain on the two CJK rows when it went on by
default, the three rows that fell when grapheme clustering landed — a ZWJ
family is one glyph now where it was three people — and what `baud-core2`, the
core this one replaced, read on the same bytes before it was deleted.

**There is one core.** `baud-core2` was the previous implementation and the
reference this one was checked against cell for cell; it is deleted, and every
row above is `baud-core`. Its rendering is frozen as data in
`baud-core/tests/golden.rs`, which is strictly weaker — a golden capture finds
regressions only on bytes somebody wrote down — and two oracles outside this
codebase can still disagree: `baud-difftest` against alacritty and
`baud-sfte-difftest` against sfte, cell for cell.

These are committed recordings of real programs, in `tests/recordings/`.

And on generated payloads, where every core is driven through the same
harness. The streaming payload models `cat`, logs and build output;
the TUI payload models a full-screen redraw. `multibench` generates both, so
every core is handed the same bytes.

**This table predates grapheme clustering (71715d3) and the scrollback ring
(0a44e3f) in the core it names, and has not been re-taken.** It was taken at
`e8035f6` (2026-09-09). MEASUREMENTS.md's table
for the same harness is older still: 406141e (2026-08-29), on the core later
renamed `baud-core2`.

| core | streaming | TUI redraw |
|---|---|---|
| **baud-core** | **3206.7** [3172.1-3234.1] | **2340.4** [2269.8-2377.7] |
| ghostty (libghostty-vt) | 640.5 [582.1-681.8] | 1061.8 [1043.0-1078.9] |
| termwiz (wezterm) parse only | 380.5 [371.5-383.4] | 366.7 [360.8-370.2] |
| alacritty_terminal | 218.3 [201.3-227.4] | 273.7 [268.0-276.5] |
| vt100 crate | 168.4 [166.6-170.3] | 218.6 [214.0-219.2] |
| wezterm (full terminal) | 17.5 [17.3-17.7] | 32.5 [32.1-32.7] |

MB/s. **Generated payloads read higher than any real capture** and their escape
density matches no real program, so the recordings above are the guide.

Two things belong with the number rather than under it:

* **baud clusters, ghostty does not.** libghostty-vt's mode 2027 carries no
  default, so it does no grapheme segmentation unless an application asks.
  baud always does. When clustering landed in `baud-core` it cost a third on
  the two CJK recordings — cat-cjk -33%, less-cjk -31% — and gating the bulk
  path per run instead of strictly brought back 18.6% and 17.6%
  ([the record](docs/record.md), "GB11 and the rest of UAX #29: every glyph gap
  is zero, and it costs a third"). The CJK rows do not compare equal work.
* **Recordings are the guide, not payloads.** On `baud-core2` the cost had two
  independent drivers, escape density and wide or emoji content, and neither
  predicted a figure by itself ([the record](docs/record.md), "The path split
  is the whole gap, and neither way of closing it works").

## Not built, and open departures

**Not built:**

- **Linux, past headless.** No pointer input, so no wheel, mouse reports or
  selection there; nobody has looked at a frame's pixels; the GPU half, Vulkan
  over dmabuf, is unstarted.
- **macOS key releases and modifier keys**, waiting on the window crate.
- **Bidirectional text.** No UAX #9 and no ECMA-48 bidirectional controls
  ([note AB](docs/research/notes/ab-right-to-left.md)).
- **Search.**
- **Any discrete GPU.** Every GPU figure is this Mac's unified memory.

**Open departures from a standard**, each recorded as a defect and each the
owner's call:

- **DECSC/DECRC save G0-G3 and which of them GL invokes, but not GR, which
  baud does not model, and no single shift, since SS2/SS3 are not
  implemented** (`[[source: vt510-decsc]]` lists "Character sets (G0, G1, G2,
  or G3) currently in GL and GR" and "Any single shift 2 (SS2) or single shift
  3 (SS3) functions sent").
- **DEC Special Graphics leaves `0x5F` as `_`**, where every DEC text read
  replaces it with Blank; which code point Blank stores is the owner's call
  ([the record](docs/record.md), "DEC Special Graphics begins at 0x5F, and what
  it stores there is the owner's call").
- **A DCS body ignores the C0 that `[[source: williams]]`'s `dcs passthrough`
  row puts (388), `BEL` and the format effectors `08-0D` excepted**, where
  `[[source: dec-std-070]]` recommends they be ignored; the texts conflict and
  the call is the owner's, with SO and SI and GR bytes in a body beside it
  ([docs/vt-coverage.md](docs/vt-coverage.md), `dcs_passthrough`).
- **Every script is shaped as `latn` or `DFLT`.** OpenType directs an
  application to the script table for the text's own script and to `DFLT`
  where there is none; `baud-font` takes no script argument
  ([note AB](docs/research/notes/ab-right-to-left.md), Q6).
- **Marks past what the cluster store holds are dropped**, and counted
  ([note AB](docs/research/notes/ab-right-to-left.md), §7.2 and Q7).
- **When no face draws a cluster whole, each attached code point its base's
  face lacks is dropped**, and counted, where the Unicode Standard asks for a
  placeholder ([note AD](docs/research/notes/ad-unrenderable-in-a-cell.md)).
- **The rendition table never frees a slot**, so a session that mints more
  distinct renditions than it has slots recolours cells that still name an
  evicted one.

## Layout

```
baud-domain/          the type system: quantities, VT operations, reports. no_std, no unsafe.
baud-core/            the ingest core: parser and grid. no_std, no unsafe.
baud-isa*/            the run scans by instruction set: the trait, aarch64 NEON, x86-64 AVX2.
baud-font/            parsing, shaping and rasterisation, with no engine dependency. no_std.
baud-render/          the compositor: cells to instances to pixels, on the CPU and the GPU.
baud-present-macos/   the macOS window, display-link pacing and AppKit input, on Metal.
baud-wayland/         the Wayland wire protocol, with no libwayland.
baud-xkb/             an XKB keymap compiler, with no libxkbcommon.
baud-present-wayland/ the Wayland window, on wl_shm.
baud-app/             the terminal, `baud`: a pty, the core, the renderer and a window.
baud-bench/           ingest2, the single-payload instrument; frame and opcost; transport, the PTY ceiling.
baud-difftest/        differential test against alacritty_terminal.
baud-sfte-difftest/   differential test against sfte.
bench-others/         the cross-implementation comparison.
```

`baud-core` depends on `baud-domain`, which brings `zerocopy`, and on the
`baud-isa*` crates only behind its run-scan features (`neon-scan`, the
default, and `avx2-scan`). It is `no_std`, with `alloc` only behind its
default-off feature, and CI builds it for `x86_64-unknown-none` to prove it.

## Building

Requires [Nix](https://nixos.org) and [devenv](https://devenv.sh) — the
toolchain is nightly-pinned and reproduced from `devenv.nix`.

```sh
devenv test                 # the gate: what CI and the pre-push hook run
devenv shell -- dev-ci      # the same checks as one script, less the per-feature and path tasks
devenv shell -- dev-test    # tests only
```

`.github/workflows/ci.yml` runs `devenv test`, so a green gate here is the
same task set CI runs. **Add a check by adding a task, never by adding a step
to the workflow.** The same suite runs on `git push` via a pre-push hook.

More in [BUILD.md](BUILD.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

## Where to read more

[docs/README.md](docs/README.md) indexes everything. The short version:

| | |
|---|---|
| [docs/architecture.md](docs/architecture.md) | the core's data path and the constraints that shaped it |
| [docs/protocols/](docs/protocols/) | one document per protocol level, mirroring the code and tests |
| [docs/vt-coverage.md](docs/vt-coverage.md) | what of the protocol is implemented, per operation |
| [docs/input-coverage.md](docs/input-coverage.md) | what baud must send, enumerated the same way |
| [docs/protocol-decisions.md](docs/protocol-decisions.md) | why the hard calls went the way they did |
| [docs/research/notes/](docs/research/notes/) | each source turned into an argument about baud, and the owner's open questions |
| [MEASUREMENTS.md](MEASUREMENTS.md) | every current number, with the host that produced it |
| [docs/record.md](docs/record.md) | the dated record of every measured change, and every decision |
| [CLAUDE.md](CLAUDE.md) | working notes: the rules, and what was tried and lost |

Every protocol claim traces to a primary source in `docs/research/sources/`,
hash-pinned in `MANIFEST.toml` and gated by `scripts/check-sources.sh`. Control
functions are not implemented from memory here — the spec has overruled
confident guesses more than once, and the record says where.

## Releases

Automated with [release-plz](https://release-plz.dev). Commits follow
[Conventional Commits](https://www.conventionalcommits.org/); release-plz opens
a `chore: release` PR that bumps the version and writes `CHANGELOG.md`, and
**merging that PR is what authorises the publish to crates.io.** Versions are
never edited by hand — release-plz reads its baseline from crates.io, so a
manual bump or publish desynchronises it.

## Funding

Solo-maintained, and sponsorship buys maintenance time rather than obligations.
See [FUNDING.md](FUNDING.md) — including how to arrange a commercial licence,
which the NonCommercial clause below makes a separate conversation.

## License

Creative Commons Attribution-NonCommercial-ShareAlike (CC BY-NC-SA) 4.0 International

See [LICENSE](LICENSE) for details.
