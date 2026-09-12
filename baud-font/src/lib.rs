//! The font stack baud owns: parse, shape, rasterise.
//!
//! **Why this crate exists and what it refuses.** The owner's constraint is
//! *no engine dependencies*: HarfBuzz, FreeType, rustybuzz and swash are
//! reference literature, not dependencies, and this crate reads font files
//! itself. `docs/research/notes/j-rendering.md` is the decisions register
//! (D1 names the crate, D6-D11 decide what it parses and how it rasterises,
//! D8-D9 how it shapes) and `j-rendering-evidence.md` carries the pinned
//! specifications behind every table it reads.
//!
//! **The contract is the core's.** `no_std`, `forbid(unsafe_code)`, and one
//! dependency -- `baud-domain`, for the vocabulary a renderer and the core
//! already share (`Codepoint`, `Width`, `GlyphId`). Every table is a
//! bounds-checked view over borrowed bytes, and nothing in a file is trusted:
//! a font from disk is the same class of input as bytes from a pty.
//!
//! Empty until phase 1 of the approved plan lands; the crate exists first so
//! every gate -- audit, docs, sources, the bare-metal build -- is scoped to it
//! before there is code to miss.
#![no_std]
#![forbid(unsafe_code)]
