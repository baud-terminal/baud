# Building and running baud

## The engine

```sh
cargo build --release                # baud-core + harnesses
cargo test  --release -p baud-core   # 17 tests: properties, known bugs, perf floors
bash audit_all.sh                    # the type-system gate
cargo build --release -p baud-core --target x86_64-unknown-none   # bare metal
```

`baud-domain` and `baud-core` are `#![no_std]`, no allocator, no runtime
dependencies. `baud-domain` uses `zerocopy` for derive-only layout checking;
proc macros do not ship in the binary. `baud-core`'s `alloc` feature adds
`Storage::boxed()` for std callers and is off by default — the bare-metal build
above proves it.

## Benchmarks

```sh
./target/release/baud-bench      # baud alone, real PTY, median of 9
cargo run --release -p baud-difftest   # cell-by-cell diff against alacritty
```

The test suite is layered, and each layer catches a different class:

| suite | catches |
|---|---|
| `vt_*.rs` | per-operation semantics: defaults, clamping, and the cases where two operations are easily conflated |
| `properties.rs` | chunk invariance — the kernel hands you arbitrary slices, and a sequence split across two reads must parse identically |
| `known_bugs.rs` | specific defects that have happened here, so they cannot happen twice |
| `perf_floor.rs` | throughput floors, as a regression alarm rather than a benchmark |
| `baud-difftest` | cell-by-cell agreement with alacritty |

Every test is deterministic. `.proptest-regressions` is checked in so a property
failure replays exactly, and a case worth finding once is also pinned by a
deterministic test rather than left depending on a saved seed.

`baud-difftest` needs an alacritty checkout; edit the path in its `Cargo.toml`.
The cross-implementation comparison (`multibench`) and the with/without-PTY
split (`alacritty-bench`) ship separately, in `bench-others/`.

## The source gate

`scripts/check-sources.sh`, run by `tasks."test:sources"`, fails if the research
corpus and its manifest have drifted apart — a missing file, a file whose
contents no longer hash to the recorded SHA-256, a file with no manifest entry,
or a doc citing a key the manifest does not carry.

The last one is the point: it stops a design document resting on a citation
nobody could check. A claim about a control function must trace to a primary
source in `docs/research/sources/`, cited as `[[source: <key>]]`. Sources marked
`gated` were never retrieved and **cannot** be cited.

Do not implement a control function from memory. `docs/vt-coverage.md` records
what the specifications say, including where two of them disagree.

## The audit gate

`audit_all.sh` fails if raw integers appear outside the places they belong. Each
per-file limit has its reason written into the script — array dimensions, SIMD
lane masks, the syscall ABI. A rise means something untyped was reintroduced,
not that the limit was too tight.
