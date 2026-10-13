# Cross-implementation benchmarks

| harness | measures | needs |
|---|---|---|
| `multibench/` | baud vs ghostty, alacritty, wezterm, termwiz, vt100, **sfte** | a ghostty checkout, for ghostty only; sfte comes through `baud-sfte-difftest`'s lib and needs nothing |
| `kitty-bench/` | kitty's parser, through Python | a built kitty |
| `alacritty-bench/` | alacritty with and without a PTY | nothing -- crates.io |

`multibench` no longer needs sibling checkouts for its Rust cores: alacritty,
vt100, termwiz and wezterm come from crates.io at pinned versions, which are
also versions a reader can quote next to the numbers.

## ghostty

Measured, in `multibench`, alongside every other core.

This README used to say `ghostty-bench`, a separate Zig harness deleted on
2026-10-13, was blocked on ghostty's `unicode_tables`, generated at build time
by two codegen executables. That is obsolete: **libghostty-vt is a shipped
artifact** with a public C ABI, built by ghostty's own `build.zig`, which runs
that codegen itself.

```sh
cd ../../ghostty
zig build -Demit-lib-vt=true -Doptimize=ReleaseFast     # needs Zig 0.16
```

On macOS the final `xcodebuild -create-xcframework` step fails; `libghostty-vt.a`
is produced before it and is what gets linked. Then:

```sh
GHOSTTY_LIB_DIR=../../../ghostty/zig-out/lib cargo run --release -p multibench
```

`build.rs` copies the archive into `OUT_DIR` before linking. `zig-out/lib`
contains both `libghostty-vt.a` and `libghostty-vt.dylib`, and macOS `ld`
prefers the dylib even under rustc's `static=` — which links, then dies at
runtime with `Library not loaded: @rpath/libghostty-vt.dylib`. If the archive is
absent the ghostty row prints `n/a` rather than being silently dropped.

**`-Dsimd=false` does not make ghostty scalar.** `src/terminal/stream.zig` calls
`std.simd.suggestVectorLength` in `consumeApcString` and `consumeOscString`
ungated by `build_options.simd`. On a target where baud has no SIMD path, no
build of ghostty is a like-for-like comparison. See MEASUREMENTS.md.

## kitty

Measured, in `kitty-bench/`, through Python — because it cannot be otherwise.

The old claim here was "kitty has no standalone parser, it is a CPython
extension." Half right: `kitty/vt-parser.c` *is* a separate file with a clean C
API. But `Parser` is declared `PyObject_HEAD`, and
`alloc_vt_parser(id_type window_id)` resolves its dispatch target through
kitty's global window registry, so there is nothing to link against.

What kitty does expose is the buffer-protocol cycle its own test suite uses,
which is the same shape as baud's `read_slot` / `write` / `feed`:

```python
dest = screen.test_create_write_buffer()
n    = screen.test_commit_write_buffer(data, dest)
screen.test_parse_written_data()
```

So `kitty-bench/bench.py` drives kitty's real parser over its real buffers, at a
cost of three Python calls per 4096-byte chunk. That cost is reported, not
subtracted: the harness prints a `ceiling` row — the identical PTY loop with a
sink that discards — which bounds any kitty number, and the distance to it is
the Python.

Building kitty from source needs harfbuzz, lcms2, xxhash, simde, Go, OpenSSL and
Python >= 3.12. A prebuilt one is far less work:

```sh
K=$(nix build --no-link --print-out-paths nixpkgs#kitty)
# macOS; on Linux the package root is $K/lib/kitty
KR=$K/Applications/kitty.app/Contents/Resources/kitty
PY=$(otool -L $KR/kitty/fast_data_types.so | grep -o '/nix/store/[^ ]*python3-[^/]*')/bin/python3

DUMP_PAYLOADS=/tmp/payloads cargo run --release -p multibench
PYTHONPATH=$KR $PY bench.py /tmp/payloads
```

The extension must be run by the exact Python it was linked against.

### Payloads are dumped, not reimplemented

`kitty-bench` does not generate payloads. `multibench` writes the exact bytes it
measures when `DUMP_PAYLOADS` is set, and `bench.py` reads them. Two generators
that agree today are two generators that can drift; one file cannot.

## foot

Still absent. It is Linux/Wayland-only and cannot be built on macOS at all.
`foot/vt.c` additionally needs stubs for `utf8proc`, `sixel.h` and foot's own
terminal struct.
