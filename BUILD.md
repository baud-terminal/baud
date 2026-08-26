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

`baud-difftest` needs an alacritty checkout; edit the path in its `Cargo.toml`.
The cross-implementation comparison (`multibench`) and the with/without-PTY
split (`alacritty-bench`) ship separately, in `bench-others/`.

## The audit gate

`audit_all.sh` fails if raw integers appear outside the places they belong. Each
per-file limit has its reason written into the script — array dimensions, SIMD
lane masks, the syscall ABI. A rise means something untyped was reintroduced,
not that the limit was too tight.
