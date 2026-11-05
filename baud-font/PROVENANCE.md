# Where baud-font's code came from

Every source file of `baud-font/src`, and six of `baud-render/src`, has a
class:

- **a**, transcribed from another project: its code follows an upstream
  function's names, structure or steps, and the notice names the licence
  text in `LICENSES/` the copy carries;
- **b**, written from the specification, with behaviour matched where the
  text leaves a choice open and another implementation's answer was taken;
- **c**, neither: written from the specification alone, or generated.

`cargo xtask third-party --check` reads this table. It fails when a file
of `baud-font/src` has no row, when a row names a file that is not there,
and when a file classed **a** is under no `[[tree]]` entry of
`docs/research/third-party.toml` with the notice its row names.

| file | class | notice |
|---|---|---|
| `baud-font/src/bake.rs` | a | `LICENSES/fonttools-MIT.txt` |
| `baud-font/src/bitmap.rs` | b | -- |
| `baud-font/src/buffer.rs` | c | -- |
| `baud-font/src/bytes.rs` | c | -- |
| `baud-font/src/cff.rs` | a | `LICENSES/ttf-parser-MIT.txt` |
| `baud-font/src/cff2.rs` | b | -- |
| `baud-font/src/cmap.rs` | b | -- |
| `baud-font/src/colr.rs` | c | -- |
| `baud-font/src/compose.rs` | c | -- |
| `baud-font/src/context.rs` | b | -- |
| `baud-font/src/describe.rs` | c | -- |
| `baud-font/src/face.rs` | b | -- |
| `baud-font/src/forms.rs` | c | -- |
| `baud-font/src/glyf.rs` | a | `LICENSES/harfbuzz-COPYING.txt`, `LICENSES/ttf-parser-MIT.txt` |
| `baud-font/src/gpos.rs` | b | -- |
| `baud-font/src/gsub.rs` | b | -- |
| `baud-font/src/gvar.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `baud-font/src/hvar.rs` | b | -- |
| `baud-font/src/ignorable.rs` | c | -- |
| `baud-font/src/indic.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `baud-font/src/indic_data.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `baud-font/src/ivs.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `baud-font/src/layout.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `baud-font/src/lib.rs` | c | -- |
| `baud-font/src/morx.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `baud-font/src/name.rs` | c | -- |
| `baud-font/src/outline.rs` | a | `LICENSES/ttf-parser-MIT.txt` |
| `baud-font/src/presentation.rs` | c | -- |
| `baud-font/src/raster.rs` | a | `LICENSES/Apache-2.0.txt` |
| `baud-font/src/script.rs` | b | -- |
| `baud-font/src/script_data.rs` | c | -- |
| `baud-font/src/shape.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `baud-font/src/standard_encoding.rs` | c | -- |
| `baud-font/src/stroke.rs` | c | -- |
| `baud-font/src/syllabic.rs` | a | `LICENSES/harfbuzz-COPYING.txt` |
| `baud-font/src/table_identity.rs` | c | -- |
| `baud-font/src/var.rs` | b | -- |
| `baud-render/src/atlas.rs` | b | -- |
| `baud-render/src/boxes.rs` | c | -- |
| `baud-render/src/cache.rs` | c | -- |
| `baud-render/src/composite.rs` | c | -- |
| `baud-render/src/metrics.rs` | c | -- |
| `baud-render/src/renderer.rs` | c | -- |
