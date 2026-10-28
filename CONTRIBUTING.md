# Contributing

## Setup

This project uses [devenv](https://devenv.sh) to pin the exact Rust toolchain
and tool versions CI uses:

```bash
direnv allow   # or: devenv shell
```

Everything below assumes you are inside that shell (or run each command as
`devenv shell -- <command>`).

## Before opening a PR

Run the full local pipeline:

```bash
dev-ci
```

This is not an approximation of CI — `.github/workflows/ci.yml` runs
`devenv test`, which runs the same `tasks."test:*"` set defined in
`devenv.nix`. A green `dev-ci` locally means a green PR check, because both
execute the same nix-defined suite. If you want to run one piece while
iterating, `enterShell` lists the individual scripts (`dev-fmt`, `dev-lint`,
`dev-check`, `dev-test`).

**Committing from a git worktree.** `treefmt` finds its tree root by walking
up from the working directory to the first `devenv.nix`, so it formats the
checkout you are in, worktree or not -- once the shell you are using has been
evaluated from a `devenv.nix` that says so. Passing `--tree-root` to it is an
error, because the wrapper already passes `--tree-root-file`. The pre-commit
hook still does not run in a worktree: `.pre-commit-config.yaml` is generated
by entering the shell and is absent there, and `prek` refuses to run without
it. In a worktree, run `treefmt --fail-on-change` from inside it (0 changed)
and commit with `PREK_ALLOW_NO_CONFIG=1`.

**Add a check by adding a task in `devenv.nix`, never by adding a step to the
workflow.** A step that exists only in CI is a step you cannot run locally, and
that is exactly the divergence this setup exists to prevent.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/). Nothing is
released (below), so no prefix decides a version or triggers a release.


A commit message says what changed, the tests that hold it and the
measurement, and nothing of how it was reviewed. Every sentence in it is a
claim someone can check.

## What baud is built to

baud implements the standards, specifications and protocols a terminal answers
to -- ECMA-48, the DEC manuals, xterm's ctlseqs, the Unicode Standard and its
annexes, the font formats, kitty's keyboard protocol -- and aims to run them
faster than any shipping terminal on ingest throughput, input latency, frame
rate under full-grid churn and memory, all at once. Only ingest is measured
against other terminals today (`multibench`, [MEASUREMENTS.md](MEASUREMENTS.md));
the other three are baud's own numbers.

**The standard is the requirement; speed is how it is met.** The work is the
standard compiled into tables indexed at build time, vectorised, and placed to
stay in L1-L3; every rule below about caches, tables and write amplification
is that. A result that is faster or looks cleaner because it departs from a
standard is a defect, not a win. A departure that is kept is a decision, and
[docs/protocol-decisions.md](docs/protocol-decisions.md) records it with its
reason.

**Which half a claim is about matters.** The core's numbers are quotable. The
frame path is built and measured on one Mac's unified-memory GPU, so a frame
number is that machine's and not a claim about a discrete GPU.

## The rule that overrides style

Two quantities gate a change to `baud-core/`:

1. **Write amplification** -- bytes written per byte ingested.
2. **Working set within cache** -- the structures touched per byte stay
   resident.

Before changing anything there, answer both: does this write more bytes per
input byte, and does it enlarge what is touched per byte? If either is yes, it
needs a measurement, not an argument.

**Rule 1 is a direction, not a bound.** `Term::amplification` returns bytes in
and cells written, and `ingest2` prints their ratio in cells per byte. A cell
is eight bytes, so text written as cells is at least 8 bytes out per byte of
ASCII in, and reading that as a regression is a category error.

**Blanking stops at the row's used extent, `RowExtent`, when the row's last
cell already equals the blank**; otherwise it fills the whole row. What it is
worth and the invariant it keeps -- where the row stops being uniform, not
where the glyphs are -- are in note F,
[docs/research/notes/f-row-extent.md](docs/research/notes/f-row-extent.md).

**Allocation is a proxy for these, not the thing.** "It doesn't allocate" is
not a defence.

### Profiling: which tool answers which question

**`sample` counts blocked time; `dtrace profile` does not, and they disagree by
20x.** `sample` put `read` at 82% of a PTY run and baud at 18%. `sudo dtrace -n
'profile-4999 /pid == N && arg1/ { @[ufunc(arg1)] = count(); }'` samples only
on-CPU userland and puts `read` at 3.7% and baud at 93%. Both are right about
different things: wall time is dominated by waiting for the child to produce
bytes, CPU time by baud's own work. Reach for `dtrace` when the question is
"what is the CPU doing" and `sample` when it is "where does the wall clock go".

`sample <pid> 20 -f out.txt` needs no dependency and no code change, and its
"Sort by top of stack" section is the self-time ranking. Watch for `__wait4`,
which is most of the process's life and sits outside the timed region.

**Hardware counters come through `xctrace record --template 'CPU Counters'`,
not dtrace, and need no `sudo`.** SIP blocks dtrace's `cpc`, `fbt` and `pid`;
only `profile` survives `sudo`. Run Xcode's own tool
(`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`): `/usr/bin/xctrace`
is an `xcode-select` shim, and under devenv's nix `DEVELOPER_DIR` it answers
"tool 'xctrace' not found", which is not a block. The export's twelve counter
columns are unlabelled. The controls in `scripts/counter-controls/` identify
idx0 and idx3 as cycles, idx6 as mispredicts, idx7 as back-end stall and
idx8/idx9 as cache misses; idx1, idx2 and idx4 count dispatched work and idx5
is unidentified, so no IPC and no "retiring" may be quoted from them. Never
assign a column from magnitudes.

### What a PTY row measures

**A PTY row on Darwin measures the tty, not the parser.** A consumer that only
reads and discards -- same fork, same payload, same transport, no parsing --
reaches 198.6 MB/s through a PTY and 6144 through a pipe, because the tty
returns 1024 bytes per `read(2)` where the pipe returns 4096. `null_ceiling`
in `transport` is that measurement. So a 20% improvement in baud's own work
moves a PTY number by under 4%, inside the noise those rows carry: parser
changes are measured in the pipe and memory columns, and the PTY column is a
claim about what a terminal delivers.

### Ceiling arms

**A ceiling arm removes work by definition; check it removed only the work you
named.** Stubbing `apply_sgr` once measured +22% on `payload::streaming`, and
four attempts to claim any of it were flat. The arm was not measuring SGR
alone: a frozen rendition means a frozen blank, so `fill_row`'s "has the
template changed" test succeeded every time and blanking stopped at the used
extent on every row; `cells_written` fell 39% under the arm. The
`ceiling-no-blank` arm in `baud-core/src/paint.rs` is built around that check:
both arms attribute the same cells and only the stores differ. **Read the
counters on both arms before believing the number.**

## Do not change these without a measurement

Each of these looks arbitrary and each cost something to learn.

| Thing | Why | If you change it |
|---|---|---|
| `-align-all-functions=6` in `[profile.release]` | Without it a function's offset in its cache line moves whenever anything linked before it changes size. Three builds of one change, whose ingest loop was instruction for instruction the same, read `cat` -8%, -9% and +3%; aligned, all three equalled the base, at no cost on eleven payloads and +2.0% on the app binary's size | Removing it brings the layout lottery back to every A/B. Changing the boundary is a new measurement, not a tidy-up. |
| `READ_CHUNK = 4096` | `N_TTY_BUF_SIZE` caps `read(2)` at 4095 bytes on Linux (`[[source: linux-n-tty]]`). On Darwin the tty hands back 1024, bounded by `TTYCLSIZE`, the output clist's capacity (`[[source: xnu-tty-h]]`; [note E](docs/research/notes/e-tty-layer.md)) | Larger buffers gain nothing: alacritty offers 1 MiB and receives 4095. The constant is right for both hosts. |
| All state in `Term` behind `&mut self` | `noalias` is presumed rather than shown | `#![forbid(unsafe_code)]` already bars `static mut`; do not move fields to atomic statics either. |
| `PackedCell` is 8 bytes: `Content(u32)` + `StyleId` + `CellFlags(u16)` | 4 bytes cannot hold a codepoint: `U+1F680 & 0xffff` is `U+F680` | Going back to 4 bytes silently corrupts astral-plane characters. |
| `CellFlags` is `u16`, not `u8` | Splitting it into `{u8 flags, u8 pad}` made the cell `{u32,u16,u8,u8}` and cost 4% of the cells path: LLVM stopped merging the store | A narrower flag field is a measured regression, not a saving. |
| `put_ascii_run` branches on `flags == NONE` outside the loop | Threading a runtime flag into the per-cell store cost a further 5%: 1049.5 MB/s against 1127.0 for the identical build with the flag spelled as a constant. DECSCA is rare, so the branch puts the cost on programs that use it | Merging the two loops to remove duplication costs 5% of the cells path. |
| `Content::EMPTY` / `WIDE_TAIL` are reserved values above `U+10FFFF` | The core must not decide that blank looks like `U+0020`: "nothing written" and "a space was written" differ for selection, reflow and equality | Storing 32 for blank breaks copy and paste of trailing whitespace. |
| `StyleId::bucket` takes the high bits of the multiplicative hash | SGR colours are multiples of `0x101010`, and masking the low bits collided them: 112,144 interns at 51.6% hit became 0 interns at 100% | `& (n-1)` on the product is the bug. |
| `Col`/`Row` are `usize`-backed, `PackedCell` fields are narrow | A `u16` cursor adds zero-extension and masking to every op on x86-64, unmeasured here. Narrow types earn their place only where thousands are stored | Narrowing `Col` needs that measurement first. |
| `tmpl` is sized to the screen, a power of two between `NSTYLE = 1024` and `NSTYLE_MAX = 1 << 16`, open-addressed over its whole length, swept at 7/8 load; slot 0 is the default and never evicted; every write is logged for the renderer's cache | A text-drawn image preview names one distinct rendition per cell, all live at once, so a table smaller than the screen evicts an on-screen cell's rendition and recolours it: a 1024-slot table scrambled 6976 of 8000 cells of a photo preview. Sizing to the screen costs about 2-3% on the most SGR-dense recording and nothing on `btop` | Do not shrink it back to a fixed 1024, and do not un-reserve slot 0: every blank cell names it without interning it. With more renditions named than it has slots, a cell naming an evicted slot is recoloured, an open bug. Every write goes through `write_slot`, or the renderer's copies go stale. |

## Things already tried that lost

Do not re-propose these without new evidence. The content-addressing,
snapshot, `splice()`, `SCHED_FIFO` and eBPF entries are a prototype's
(`git show d8262f55:README.md`, a single-core virtualised Xeon); the two scan
entries are `baud-core2`'s, the core this one replaced.

- **~~Vectorising the bulk scan.~~** This entry was wrong, and it is the
  reason to distrust the rest of this list without a re-measurement. `Scan::run`
  carried a comment that a `core::simd` version "measured a 12% regression on
  the streaming payload", and it kept the hottest loop in the project scalar.
  Re-measured with `ingest`, it is +41% on spans and +25% on cells, ranges not
  overlapping. The vector form is flat on btop and vim, whose runs are short
  because they dispatch one control sequence every 17 bytes; it is text that
  has long runs.
- **Content addressing or dedup on the hot path** -- -18% even at 100% dedup.
  Hashing is unconditional; dedup saves storage, not time. Correct for a cold
  tier, wrong for ingest.
- **Grid snapshots per line of ingest** -- -58% (persistent grid, structural
  sharing) to -93% (a full cell copy per line), on a span-model prototype. A
  copy per frame is a different thing, and the app ships one.
- **`splice()` on the PTY** -- 297 against 546 MB/s. It goes through the line
  discipline's ring anyway and adds a pipe hop.
- **`SCHED_FIFO`** -- p99 50% worse.
- **eBPF to widen the tty path** -- it cannot resize a kernel struct or skip
  the `queue_work()` hop. Useful for measuring the workqueue delay, useless
  for removing it.
- **The data-parallel FSM method of `[[source: dpfsm]]`, as a whole.** Its
  § 2.1 assumes the action function "can be invoked out of order". A
  terminal's actions cannot be: print-then-scroll is not scroll-then-print,
  and a rendition applies to what follows it. Satisfying the assumption means
  buffering every action and replaying it in order, the sequential dependence
  the method exists to remove. The scanning half survives -- boundaries and
  parameter spans carry no actions -- and is a different, unmeasured proposal.
- **Scanning a row before blanking it** -- -24% forward and vectorised, -66%
  backward, with `cells_blanked` unchanged: the blank carries the current
  style, so on coloured output the scan never matches and is a pure extra
  read. `RowExtent`'s one-cell test replaced it (note F).

## How to verify a change

Fix the acceptance test before touching anything. Then:

1. **Build base and change at one path.** `git archive` each into the same
   scratch directory and `touch` every extracted file, because the archive
   stamps its commit's time and cargo otherwise rebuilds nothing. Never a
   `git worktree`, which moved `cat` 6.5% by layout alone. Build an A/A copy of
   the change too, and copy each `ingest2` binary out of `target/` as it is
   built. `cmp` base against change before timing: identical means nothing
   rebuilt. If the two release binaries' machine code (`__TEXT,__text`) is
   byte-identical and every table keeps its size, that argument stands in for
   a timing.
2. **Run the copied binaries with the same `<payload> <reps>`**
   (`cargo bench -p baud-bench --bench ingest2 -- <payload> <reps>` builds and
   runs one), with the arms rotated each round, at least four rounds, the
   first dropped (a cold arm has read 47% below its settled rate), on mains
   power. Size the reps to the recording, not the harness default: 40 reps of
   the 30 KB `less-pager` was still ramping at round five, and 400 settled.
   Read `cells_written` on every arm before any rate, then compare medians and
   ranges.
3. **State what result would have proven the change wrong**, and whether it
   did.

Variance on this workload is wide, and several orderings in this project's
history flipped between runs. **If ranges overlap, there is no result.** Say so.

## Refactoring failures that already happened here

These are the specific ways a correct-looking change lost performance or
correctness silently.

- **A refactor deleted the SIMD scanner and nothing noticed.** A port to
  domain types dropped `feed_avx2` entirely, and the benchmark kept printing
  `scan path: AVX2` from a stale flag. Grep for `_mm256` after any port.
- **A view helper was constructed inside a per-cell loop.** `self.view()` in
  `blit` rebuilt a four-field borrow and re-checked bounds for every byte.
  Bulk operations belong on the view (`put_ascii_run`), not per cell.
- **Instrumentation outlived what fed it.** The bench printed
  `inf× amplification` for a long stretch because a port stopped feeding
  counters it still read, and it hid a style-hash bug the whole time. Broken
  instrumentation is worse than none.
- **String-replacement edits that silently no-op.** Three times a replacement
  did not match and said nothing, once because `rustfmt` had reflowed the
  anchor. Verify the edit landed, not just that it compiled; anchor on
  something `rustfmt` cannot reflow, or assert the anchor exists first.
- **A span-delete that took its neighbours with it.** Replacing "everything
  between `emit_num` and `resize`" removed eight other functions that lived in
  that span. The compiler caught it because they were called; a span-delete
  that swallows dead code is silent. Delete by name, not by range.
- **An attribute stranded from the function it was written for.** An insertion
  landed between a doc comment and its signature, so `#[inline(never)]`
  decorated the next function down -- the inverse of what both meant. It
  compiles, because a doc comment is an attribute.
  `no_attribute_is_stranded_above_a_doc_comment` checks every source file for
  it now.

## Never derive a number from a modified recording

**A recording is evidence. Editing it and measuring the result is measuring the
edit.** Deleting the emoji from `cat-cjk` deleted their cells too, so it priced
not drawing them -- 35.6% of the clock, and a ceiling was published on it. A
ceiling arm on the real bytes, still decoding and writing every cell and
skipping only the clustering and mark storage, priced those at 6.8%: five
times smaller, and the difference is the work of showing the character.

| | input | output | what it prices |
|---|---|---|---|
| edited recording | **changed** | a different screen | the edit |
| ceiling arm | **untouched** | wrong by construction | one component |

A ceiling arm feeds the real bytes and produces incorrect output on purpose,
and its counters announce it. **Read the counters on both arms; an edited
recording tells you nothing.** The same holds for payloads: use real
recordings, never a filtered copy of one.

## Measurement failures that already happened here

These are the specific ways this benchmark lies.

- **Comparing across machines.** Only same-box numbers count.
- **Mismatched optimisation flags.** `gcc -O2` against `rustc opt-level=3` with
  LTO is not a comparison. Rebuild both.
- **Runtime variant switches in the hot loop.** Bolting variants into one
  binary behind a runtime `if` produced a fake cost for a change that was free.
  Compile variants out (`cfg`) when measuring small deltas.
- **Payloads that don't exercise the case.** A TUI payload that addresses
  column 1 every time never forces a mid-line write.
- **Replayed buffers faking uniqueness.** `ingest2` feeds the same buffer every
  rep, so anything content-keyed hits about 100% after the first pass
  (`cat-scroll` read +56%, all of it fake). Measure it at one rep per process.
- **Memory pressure you created.** A 3 GB pool on a 3 GB box made the
  allocating variants look slower than they were. Size allocations to the
  workload.
- **A machine that drifts between arms.** Two binaries built from identical
  source read 407-418 and then 432-436 within minutes, a 6% spread wider than
  most effects measured here. Run the arms interleaved and quote ranges, and
  when a change is smaller than the drift, say unmeasured. A same-source A/A
  pair is the cheap way to find out which day you are having.
- **Per-operation rows cannot resolve a few nanoseconds.** `opcost`
  (`baud-bench/benches/opcost.rs`) ranks shapes against each other within one
  run; it does not compare builds.
- **Arithmetic that can't be right.** When the number is impossible, the
  harness is wrong, not the world.

## The protocol is enumerated

[docs/vt-coverage.md](docs/vt-coverage.md) is the scope definition and the
source of truth for what baud implements, and
[docs/input-coverage.md](docs/input-coverage.md) is its twin for keys and the
mouse. Read the table before adding an operation and update it in the same
commit; a row moves to **done** only when a test exists that would fail if the
operation regressed.

A sequence the parser recognises and then **drops** is worse than one it does
not implement: the terminal accepts it, does nothing, and gives the
application no way to find out. Two rules follow:

* **Never answer a query with the wrong shape.** An application parses
  whatever arrives as the report it asked for, so a wrong answer is worse than
  silence. That is why DA under-claims as a VT102 and an unimplemented DSR gets
  no reply.
* **Never record a mode you do not act on.** A bit that is set and never read
  is the same failure as a sequence parsed and dropped.

**Dispatch reaching a handler is not the same as reaching the right one.**
`CSI Ps ; Ps ; Ps ; Ps ; Ps T` once scrolled the screen: SD and XTHIMOUSE share
a final byte and differ only by arity. `a_final_with_two_arities_means_two_things`
requires each such pair to leave the terminal in different states.

**The specification checks baud, not the other way round.**
`baud-core/tests/conformance/spec_inventory.rs` reads `[[source: ctlseqs]]`'s
own lists and requires each entry to be acted on or counted, because a check
that reads baud's own table cannot show a row nobody wrote.

## Get the specification before writing the code

Every protocol claim traces to a primary source in `docs/research/sources/`,
recorded in its `MANIFEST.toml` with a SHA-256 and gated by
`scripts/check-sources.sh`: a note or doc cannot cite a key the manifest does
not carry, or one recorded as `gated` and therefore never read. A source the
repository does not hold is `external`, with its URL and digest.

It governs what is drawn as much as what is parsed: what a cell shows for a
character no font has is the Unicode Standard's (TUS §5.3: it "can only be
indicated by some placeholder"). Design research reads the governing standard
first, quotes it, and marks every option conforming or not. Other terminals are
evidence of practice, not of the standard: where one departs from the text,
that is recorded as its departure, never taken as a reason to follow it.

Do not implement a control function from memory. The spec has overruled
confident guesses here repeatedly:

* A bare `ESC` cancels a control string (`[[source: williams]]`).
* After `ESC` inside a control string, only `\` makes ST, and BEL also ends an
  OSC (`[[source: ctlseqs]]`).
* DECOM confines all cursor movement, not just absolute addressing
  (`[[source: vt510-decom]]`).

**A source you cannot search is not a source you have read.** ECMA-48's PDF
extracts letter-spaced, so an exact search for `framed` cannot match `f r amed`;
six probes missed and the misses were read as absence. Strip the
non-alphanumerics from both sides (`scripts/extract-ecma48.py`), and **run a
positive control before believing a miss**: search for something you already
know is in the document.

Where sources conflict, say so and name both. DECAWM's default is "No autowrap"
in the VT510 manual and on in xterm; baud follows xterm and
[docs/vt-coverage.md](docs/vt-coverage.md) records the choice.

## Unmeasured — do not assert

Say "unmeasured" rather than estimating.

1. **The run cache's steady-state hit rate over a long session.** Its cold
   rate is 15-90% across the corpus, and the default cell path's window cache
   misses a quarter to a third of windows on scrolling code.
2. **Any discrete GPU.** Every GPU figure is one Mac's unified memory.
3. **A `u16` cursor on x86-64**, the `Col` row of the table above.

## Style

- `baud-core/` stays `#![no_std]`, with `alloc` only behind its default-off
  feature; it depends only on first-party crates (`baud-domain`, and the ISA
  crates behind the scan features), plus `zerocopy` through `baud-domain`.
  Verify with
  `cargo build --release -p baud-core --target x86_64-unknown-none`.
- **`baud-core` has no `unsafe`**: `lib.rs` carries `#![forbid(unsafe_code)]`.
  The intrinsics live in `baud-isa-aarch64` and `baud-isa-x86-64`, and the core
  calls them only through their safe wrappers.
- **One entry point; AVX2 by build target.** `no_std` has no runtime feature
  detection, so the `avx2-scan` feature wires `Avx2::run` into `run_of_text`
  only under `target_feature = "avx2"`.
- Doc comments carry reasons with numbers, not rules. When you change a
  constant, change the reason.
- **Typing the hot path is free; changing its loop is not.** Newtypes are
  `repr(transparent)` and lower to the same instructions, so "it would cost
  performance" is not an argument against a type. A typed edit that also
  changes the loop's shape, such as a second induction variable, can cost:
  keep the untyped binary and A/B it before blaming the types.
- **The audit's budgets are a floor, not slack.** `audit_all.sh` sets each
  crate's raw-integer limit beside an enumeration of its sites, and gates
  `baud-domain` at zero `bool` fields, because a `bool` field is a missing
  two-state enum. Budgets come down by typing what was untyped; never raise one
  to make a change fit.
- Format with `treefmt`, not `cargo fmt`; CI gates on `treefmt`.

## Tests

Every test is deterministic. A flaky test is worse than none: it trains
everyone to re-run until green and hides the real defect. If a test fails
intermittently, the non-determinism is the bug.

**A green run on arm64 says nothing about the AVX2 scanner.**
`baud-isa-x86-64/tests/agrees.rs` (`Avx2` against `Portable`) runs on x86-64
in CI; `baud-isa-aarch64/tests/agrees.rs` is its NEON twin.

`.proptest-regressions` files are checked in on purpose: a property failure
must replay exactly, and a case worth finding once is worth pinning with a
deterministic test as well.

**A property that passes at 200 cases is not a property that holds.** Run new
properties at `PROPTEST_CASES=20000` before trusting them, and when one fails,
first ask whether the property is true. **A property is only as good as the
shapes its generator can reach**, which is a separate failure from too few
cases: a chunk-invariance property once passed at 20,000 cases while two
parsers disagreed past the sixteenth CSI parameter, because no generated
program carried a long list. When adding an operation, ask what shape of input
reaches it and whether `baud-core/tests/properties.rs` can build that shape:
parameter lists past capacity, lists with holes, values past `u16`, C0
controls inside a sequence, every SGR parameter, multi-byte text and its
malformed forms. Add a new atom at the end of the alphabet so existing
regression seeds keep their meaning.

A program ending mid-sequence is an **open state**, not a bug: `E2 86` renders
nothing, because a terminal cannot know the stream ended and the third byte
usually arrives in the next `read(2)`.
`an_incomplete_sequence_at_the_end_renders_nothing_until_flushed` pins it.

**Every public function in `baud-core` is called from a test**
(`every_public_function_in_the_new_core_is_called_by_a_test`). It is not line
coverage, and deliberately so: what it catches is the entry point nothing
reaches, as `Term::resize` once had no caller anywhere and, once reached, gave
up three bugs, one a panic on window shrink. The exemption list is empty on
purpose.

**Correctness comes before a benchmark.** Read the counters and the
differentials (`baud-difftest` against alacritty, `baud-sfte-difftest` against
sfte, cell for cell) before any rate; a test that only prints cannot fail.

## Releases

**baud is private and publishes nothing.** Every crate's `Cargo.toml` carries
`publish = false`, so `cargo publish` refuses each one, and a new crate must
carry it too. The release-plz jobs in `.github/workflows/ci.yml` still run on
master, and `release-plz.toml` keeps them to a no-op: `release` skips every
push that is not a Release PR's merge and finds nothing to release on one,
`release-pr` opens no Release PR, and neither tags nor makes a GitHub
release. Do not publish by hand. `cargo metadata --no-deps
--format-version 1` shows `"publish": []` for every package.

**Cutting a release, when there is one, raises two numbers in one commit.**
The version, `1.0.0-beta.N` in `[workspace.package]` of `Cargo.toml`, which
every crate, XTVERSION and the app's short version read; and the build
number in `packaging/macos/build-number`, one line raised by one, which
`scripts/bundle-macos.sh` writes as the app's `CFBundleVersion`. The
counter lives in the repository rather than coming from git, so a build
with no `.git`, from nix or a source tarball, writes the same number.
`baud-app/tests/version.rs` holds both to the shapes Apple allows and the
bundle to the counter.

## Code of Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
