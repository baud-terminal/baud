//! **baud's brand pictures**: every file in `assets/logo/` and the
//! repository's link picture, `assets/readme/social.svg` and `.png`,
//! drawn from one mark ([`brand`]); and the README logo's plasma, the
//! walls' geometry ([`geometry`]) and each frame's strands ([`plasma`]),
//! which `scripts/make-readme-logo.py` films.
//!
//! Each step is the port of what drew them before, call for call:
//!
//! | module | is | ported from |
//! |---|---|---|
//! | [`outline`] | glyf outlines as SVG path data and ink boxes, in `f64` | fontTools 4.63.0's `TransformPen`, `SVGPathPen`, `BoundsPen` |
//! | [`raster`] | an SVG drawn to RGBA | the resvg 0.48.1 command line, as a library |
//! | [`png`] | the PNG bytes | Pillow 12.3.0's PNG writer and `ZipEncode.c` |
//! | [`zlib`] | deflate, inflate, CRC-32 | zlib 1.3.2 through `extern "C"`, the version Pillow carried |
//! | [`resample`] | the mark's light scaled to the picture | Pillow 12.3.0's LANCZOS on mode `F`, as compiled |
//! | [`geometry`] | the walls' signed distance, normals and counters | the NumPy program that wrote them before |
//! | [`plasma`] | each frame's strands and the run's report | the NumPy program that wrote them before |
//! | [`det`] | `sin`, `cos`, `exp` from IEEE's operations | that program's own, on fdlibm's kernels |
//! | [`numpy`], [`py`] | the arithmetic the generator's NumPy 2.5.2 and CPython 3.14.7 did | their C, read at those tags |
//!
//! Run from the repository's root, in the shell that pins zlib:
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
//! `geometry <walls.png> <out dir>` and `plasma <geometry dir> <dark|light>
//! <on_ms> <off_ms> <out>` are the film's two steps.

pub mod brand;
pub mod det;
pub mod geometry;
pub mod json;
pub mod numpy;
pub mod outline;
pub mod plasma;
pub mod png;
pub mod py;
pub mod raster;
pub mod resample;
pub mod zip;
pub mod zlib;

/// SHA-256, the one the repository's gates use.
#[path = "../../../xtask/src/sha256.rs"]
pub mod sha256;
