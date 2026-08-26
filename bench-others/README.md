# Cross-implementation benchmarks

Each of these needs a checkout of the terminal it measures, as a sibling of the
`baud` directory. Edit the paths in the manifests if yours differ.

| harness | measures | needs |
|---|---|---|
| `multibench/` | baud vs alacritty, wezterm, termwiz, vt100 | `../../../alacritty` |
| `alacritty-bench/` | alacritty with and without a PTY | `../../../alacritty` |
| `ghostty-bench/` | ghostty's terminal module | Zig 0.16, `../../../ghostty` |

## ghostty-bench is incomplete

It compiles ghostty's `terminal` module — uucode fetched and configured the way
ghostty configures it, kitty-graphics gated off to drop the `wuffs` dependency —
and stops at `unicode_tables`, which ghostty generates at build time with two
codegen executables (`props-unigen`, `symbols-unigen`; see
`ghostty/src/build/UnicodeTables.zig`). Wiring those in is the remaining work.
The PTY harness itself is written and compiles.

foot and kitty are not here. `foot/vt.c` needs stubs for `utf8proc`, `sixel.h`
and foot's terminal struct; kitty has no standalone parser, it is a CPython
extension.
