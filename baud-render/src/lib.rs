//! The compositor: `Term::viewport` in, pixels out.
//!
//! **The shape every shipping GPU terminal shares**, and the one measured
//! bottleneck among them (`docs/research/notes/j-rendering.md` D2): the CPU
//! turns cells into flat fixed-size instance records, one memcpy puts them in
//! driver-visible memory, one or two instanced draws expand them into
//! atlas-textured quads. The GPU is never the bound; the conversion is. So
//! the conversion is built first, on the CPU, where `baud-bench`'s `frame`
//! harness can price it without a GPU.
//!
//! **The pipeline, per frame.** [`Renderer::render`] reads the viewport,
//! finds the rows that changed since the last frame (D12: row-granular,
//! outside the cell, by comparing against the previous frame's copy),
//! and for each one: splits the row into runs of cells shaped by one face
//! (D8: fallback is per cluster, and a run ends where the face changes),
//! shapes each run through `baud-font` behind the run cache that ghostty
//! measured at 96% of frame time without, maps glyphs to cells by Windows
//! Terminal's rule -- a cluster's glyphs draw at its first cell -- and emits
//! one [`Instance`] per drawn glyph plus one background word per cell.
//! Then the compositor paints the dirty rows into the caller's pixel
//! buffer: background first, then every glyph's atlas mask over it (D7:
//! coverage stays linear and the blend is a policy).
//!
//! Placement of the instance buffer is a per-architecture startup decision
//! (D3), and nothing here maps the live grid as a GPU buffer (D4).
#![forbid(unsafe_code)]

mod atlas;
mod cache;
mod color;
mod composite;
mod instance;
mod metrics;
mod renderer;
mod stack;
mod units;

pub use atlas::{Atlas, AtlasStats, Kind, Slot, Tex};
pub use cache::{CacheStats, PLACED_MAX, Placed, RUN_CELLS, RunCache, Shaped};
pub use color::{Argb, Theme};
pub use composite::{Blend, Target};
pub use instance::Instance;
pub use metrics::{Metrics, PxSize};
pub use renderer::{Damage, Frame, Renderer};
pub use stack::{FaceIx, Stack};
pub use units::{Band, Coverage, Digest, Tally};
