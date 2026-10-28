# Measuring baud

Which instrument answers which question, how to run each, and the rules a
number has to pass before it goes in [MEASUREMENTS.md](../../MEASUREMENTS.md).
The rules are CONTRIBUTING.md's
[How to verify a change](../../CONTRIBUTING.md#how-to-verify-a-change) and
[Measurement failures that already happened here](../../CONTRIBUTING.md#measurement-failures-that-already-happened-here);
this page is the commands.

## Which instrument

| question | instrument |
|---|---|
| did a core change make ingest faster or slower? | `ingest2`, in memory, no fork and no pty |
| what does the transport cost with no parser behind it? | the `transport` bench |
| what does one character shape cost? | the `opcost` bench |
| how does baud's core compare with ghostty's and alacritty's? | `bench-others/multibench` |
| how do whole terminals compare through a pty? | `scripts/vt-compare.sh` |
| how long from a key to the screen? | `scripts/key-latency.py`, and `scripts/key-pixel.swift` for the panel |
| how long is a launch, warm and cold? | `scripts/shader-cache-bench.py` |
| what does an idle window cost? | `scripts/idle-cpu.py` |

The rest of `baud-bench/benches/` (`frame`, `frame_gpu`, `glyphs`, `reshape`,
`pictures`, `png_decode`, `kitty_image`, `flash`, `winch`, `ceiling`) each
open with a comment saying what they time.

## In this repository

```sh
# One payload, in memory, no fork and no pty: the instrument CONTRIBUTING.md's
# "How to verify a change" times a core change with. A recording name, or a ladder rung
# (flat/wrap/lines/sgr), or a synthetic payload (cjk/streaming), or `mix`.
cargo bench -p baud-bench --bench ingest2 -- cat-cjk 250
devenv shell -- dev-bench cat-cjk   # the same harness, as a devenv script

# The transport with no parser behind it -- the ceiling a PTY row is measured
# against, and the reason a PTY row is a claim about the tty rather than the
# parser.
cargo bench -p baud-bench --bench transport -- utf8/btop.raw 48
cargo bench -p baud-bench --bench ceiling    # the reference ceilings, on mains
cargo bench -p baud-bench --bench opcost     # what one character shape costs
```

## Against other implementations

The cross-implementation harnesses live in `bench-others/`, outside the
workspace; [bench-others/README.md](../../bench-others/README.md) says what
each needs.

```sh
# cross-implementation, real capture
cd bench-others/multibench
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory \
  RECORDINGS="utf8/btop.raw,utf8/cat-cjk.raw" cargo run --release

# cross-implementation, generated payloads
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib TRANSPORT=memory cargo run --release
```

`multibench` takes two recordings at a time. Without `GHOSTTY_LIB_DIR` the
ghostty row reports `n/a` rather than being silently dropped.
[MEASUREMENTS.md](../../MEASUREMENTS.md#reproducing) has the sfte ceiling arm
as well.

`alacritty-bench` splits alacritty's own synthetic payloads with and without
a pty. It once pinned alacritty at a sibling checkout no machine here has,
and being outside the workspace nothing noticed; it takes the registry
version now. A single run of it is a check that it builds and runs, not a
measurement: one launch, no interleaving and no control, so no number from it
belongs in a table.

## Before a number is written down

- **Build both arms at one path**, and `cmp` them: identical binaries mean
  nothing rebuilt.
- **Interleave the arms and discard the first round.** A cold arm has read
  47% below its settled rate. Run at least four rounds.
- **Read the counters before the rate.** `ingest2` prints `cells_written`;
  if it differs between arms, the arms do different work.
- **If the ranges overlap there is no result**, and the record says so.
- **Same machine only.** A number from another box is not comparable.
- **A PTY row on macOS measures the tty**, which hands over 1024 bytes a
  read; a parser change shows up in memory, not through a pty.
