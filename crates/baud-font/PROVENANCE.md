# Where baud-font's code came from

Every source file of `crates/baud-font/src`, and six of `crates/baud-render/src`, has a
class:

- **a**, transcribed from another project: its code follows an upstream
  function's names, structure or steps, and the notice names the licence
  text in `LICENSES/` the copy carries;
- **b**, written from the specification, with behaviour matched where the
  text leaves a choice open and another implementation's answer was taken;
- **c**, neither: written from the specification alone, or generated.

`cargo xtask third-party --check` reads this table. It fails when a file
of `crates/baud-font/src` has no row, when a row names a file that is not there,
and when a file classed **a** is under no `[[tree]]` entry of
`docs/research/third-party.toml` with the notice its row names.

| file | class | notice |
|---|---|---|
| `crates/baud-font/src/bake.rs` | a | `LICENSES/fonttools-MIT.txt` |
| `crates/baud-font/src/bitmap.rs` | b | -- |
| `crates/baud-font/src/buffer.rs` | c | -- |
| `crates/baud-font/src/bytes.rs` | c | -- |
| `crates/baud-font/src/cff.rs` | a | `LICENSES/ttf-parser-MIT.txt` |
| `crates/baud-font/src/cff2.rs` | b | -- |
| `crates/baud-font/src/cmap.rs` | b | -- |
| `crates/baud-font/src/colr.rs` | c | -- |
| `crates/baud-font/src/context.rs` | b | -- |
| `crates/baud-font/src/describe.rs` | c | -- |
| `crates/baud-font/src/face.rs` | b | -- |
| `crates/baud-font/src/forms.rs` | c | -- |
| `crates/baud-font/src/glyf.rs` | a | `LICENSES/harfbuzz-COPYING.txt`, `LICENSES/ttf-parser-MIT.txt` |
| `crates/baud-font/src/gpos.rs` | b | -- |
| `crates/baud-font/src/gsub.rs` | b | -- |
| `crates/baud-font/src/gvar.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `crates/baud-font/src/hvar.rs` | b | -- |
| `crates/baud-font/src/indic.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `crates/baud-font/src/ivs.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `crates/baud-font/src/layout.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `crates/baud-font/src/lib.rs` | c | -- |
| `crates/baud-font/src/morx.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `crates/baud-font/src/name.rs` | c | -- |
| `crates/baud-font/src/outline.rs` | a | `LICENSES/ttf-parser-MIT.txt` |
| `crates/baud-font/src/raster.rs` | a | `LICENSES/Apache-2.0.txt` |
| `crates/baud-font/src/script.rs` | b | -- |
| `crates/baud-font/src/shape.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `crates/baud-font/src/stroke.rs` | c | -- |
| `crates/baud-font/src/syllabic.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `crates/baud-font/src/var.rs` | b | -- |
| `crates/baud-render/src/atlas.rs` | b | -- |
| `crates/baud-render/src/boxes.rs` | c | -- |
| `crates/baud-render/src/cache.rs` | c | -- |
| `crates/baud-render/src/composite.rs` | c | -- |
| `crates/baud-render/src/metrics.rs` | c | -- |
| `crates/baud-render/src/renderer.rs` | c | -- |
