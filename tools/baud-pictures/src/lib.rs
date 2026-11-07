//! **baud's brand pictures**: every file in `assets/logo/` and the
//! repository's link picture, `assets/readme/social.svg` and `.png`,
//! drawn from one mark ([`brand`]); and the README's header logo
//! ([`logo`]): the walls' geometry ([`geometry`]) and each frame's
//! strands ([`plasma`]), filmed by baud's renderer and encoded, the GIFs
//! by gifski and the SVGs' frames as WebP.
//!
//! Each step is the port of what drew them before, call for call:
//!
//! | module | is | ported from |
//! |---|---|---|
//! | [`outline`] | glyf outlines as SVG path data and ink boxes, in `f64` | fontTools 4.63.0's `TransformPen`, `SVGPathPen`, `BoundsPen` |
//! | [`raster`] | an SVG drawn to RGBA | the resvg 0.49.0 command line, as a library |
//! | [`png`] | the PNG bytes | Pillow 12.3.0's PNG writer and `ZipEncode.c` |
//! | [`zlib`] | deflate, inflate, CRC-32 | zlib 1.3.2 through `extern "C"`, the version Pillow carried |
//! | [`webp`] | the SVG frames' WebP, and reading them back | libwebp 1.6.0 through `extern "C"`, as Pillow 12.3.0's `_webp.c` calls the build it linked |
//! | [`gif`] | gifski's GIF, its palette snapped to the page and its delays the film's | the driver's own byte edits |
//! | [`logo`] | the film, the encoders and every check on the four files | the Python driver that ran them before |
//! | [`resample`] | the mark's light scaled to the picture | Pillow 12.3.0's LANCZOS on mode `F`, as compiled |
//! | [`geometry`] | the walls' signed distance, normals and counters | the NumPy program that wrote them before |
//! | [`plasma`] | each frame's strands and the run's report | the NumPy program that wrote them before |
//! | [`det`] | `sin`, `cos`, `exp` from IEEE's operations | that program's own, on fdlibm's kernels |
//! | [`numpy`], [`py`] | the arithmetic the generator's NumPy 2.5.2 and CPython 3.14.7 did | their C, read at those tags |
//!
//! Run from the repository's root, in the shell that pins zlib and
//! libwebp:
//!
//! ```text
//! nix-shell tools/shell.nix --run \
//!   'cargo run --release --manifest-path tools/Cargo.toml --bin baud-pictures -- brand --check'
//! ```
//!
//! `brand` writes every file; `--check` writes nothing and fails unless
//! each equals the committed file byte for byte, they are every file in
//! `assets/logo/`, no SVG names a font, and both GitHub pictures meet
//! GitHub's limits. `--root DIR` reads and writes another tree.
//! `logo` films and encodes the README's four logo files and `logo
//! --check` compares them with the committed ones, writing none; `--keep
//! DIR` films into the empty DIR and leaves the frames, strands and
//! encodes there.
//! `geometry <walls.png> <out dir>` and `plasma <geometry dir> <dark|light>
//! <on_ms> <off_ms> <out>` are the film's two steps, run alone.

pub mod brand;
pub mod det;
pub mod geometry;
pub mod gif;
pub mod json;
pub mod logo;
pub mod numpy;
pub mod outline;
pub mod plasma;
pub mod png;
pub mod py;
pub mod raster;
pub mod resample;
pub mod webp;
pub mod zip;
pub mod zlib;

/// SHA-256, the one the repository's gates use.
#[path = "../../../xtask/src/sha256.rs"]
pub mod sha256;
