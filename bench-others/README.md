# Cross-implementation benchmarks

| harness | measures | needs |
|---|---|---|
| `multibench/` | baud vs ghostty, alacritty, wezterm, termwiz, vt100, **sfte** | a ghostty checkout, for ghostty only; sfte comes through `baud-sfte-difftest`'s lib and needs nothing |
| `kitty-bench/` | kitty's parser, through the interpreter inside kitty | a kitty built or installed, found at run time |
| `alacritty-bench/` | alacritty with and without a PTY | nothing -- crates.io |
| `ghostty-oracle/` | not a benchmark: V10's oracle, baud-core against libghostty-vt cell for cell at every read size, the screen, the cursor and the pending wrap, with each difference pinned to its reason | `GHOSTTY_LIB_DIR` naming a libghostty-vt built at d9840f3; the build fails without it |

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

Measured, in `kitty-bench/`, a Rust crate that loads kitty's own interpreter.
**Python still runs here, inside kitty, as part of what is measured**, because it
cannot be otherwise. This repository holds no Python file and runs no Python
command for this row.

The old claim here was "kitty has no standalone parser, it is a CPython
extension." Half right: `kitty/vt-parser.c` *is* a separate file with a clean C
API. But `Parser` is a CPython object, and `alloc_vt_parser` resolves its
dispatch target through kitty's global window registry, so there is nothing to
link against.

What kitty does expose is the buffer-protocol cycle its own test suite uses,
which is the same shape as baud's `Term::write_at` / `Term::feed`: bytes placed
in the terminal's arena, the way a `read(2)` would, and parsed there. Those are
three methods: `Screen.test_create_write_buffer`,
`Screen.test_commit_write_buffer` and `Screen.test_parse_written_data`.

So `kitty-bench` drives kitty's real parser over its real buffers, at a cost of
three calls into the interpreter per read of up to 4096 bytes. That cost is
reported, not subtracted: the harness prints a `ceiling` row, the identical PTY
loop with a sink that discards and before any of kitty is loaded. It is another
operating point and not a bound, because a reader that discards blocks in
`read(2)` more often than one that parses.

**Licence.** kitty is GPL-3 and this repository is not. The crate links nothing
of kitty's at build time: it has no build script and no link line names kitty or
Python. It opens kitty's extension and the libpython that extension brings with
`dlopen`, at run time, from a kitty the user built or installed, and copies no
source, text or grammar of kitty's. It names the calls above and nothing more.

Building kitty from source needs harfbuzz, lcms2, xxhash, simde, Go, OpenSSL and
Python >= 3.12. A prebuilt one is far less work:

```sh
K=$(nix build --no-link --print-out-paths nixpkgs#kitty)

mkdir -p /tmp/payloads
(cd bench-others/multibench && DUMP_PAYLOADS=/tmp/payloads cargo run --release)
cd bench-others/kitty-bench
KITTY_ROOT=$K cargo run --release -- /tmp/payloads
```

`KITTY_ROOT` names a package root, an install prefix or an app bundle, and then
nothing else is tried, so a kitty that is not there never turns into a
measurement of a different one. With none set, the harness takes `kitty` on
`PATH`, followed through its links, then `/Applications/kitty.app`.
`KITTY_LIBPYTHON` names the libpython of a kitty whose extension does not link
one. Only nix's kitty on macOS has been run; the other layouts are found by
tests on empty files and have not been loaded.

The table goes to stdout. On stderr are the kitty and the interpreter it used,
the bytes each rep read and parsed, and how many callbacks the parser asked for.
A bad argument, a kitty that will not load, a broken cycle, a wrong byte count
or a short read exits 1 with the reason on stderr and no row for what failed; a
kitty that is not found also lists where it looked, and a failure inside the
interpreter prints its traceback first.

`cargo test` here needs no kitty and lists the two tests that do as ignored.
`KITTY_ROOT=$K cargo test -- --include-ignored` runs them, and with no kitty
they fail.

### Payloads are dumped, not reimplemented

`kitty-bench` does not generate payloads. `multibench` writes the exact bytes it
measures when `DUMP_PAYLOADS` is set, and `kitty-bench` reads them. Two generators
that agree today are two generators that can drift; one file cannot.

## foot

Still absent. It is Linux/Wayland-only and cannot be built on macOS at all.
`foot/vt.c` additionally needs stubs for `utf8proc`, `sixel.h` and foot's own
terminal struct.
