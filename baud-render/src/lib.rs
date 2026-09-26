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
pub mod boxes;
mod cache;
mod cellmap;
mod color;
mod composite;
pub mod fire;
mod instance;
mod metrics;
mod overlay;
mod picture;
mod renderer;
mod scroll;
mod scrollbar;
mod select;
mod stack;
mod trail;
mod transition;
mod units;

pub use atlas::{
    Atlas, AtlasStats, Code, Dirty, Generation, Kind, Replaced, Side, Slot, Tex, TexRect,
};
pub use cache::{CacheStats, PLACED_MAX, Placed, RUN_CELLS, RunCache, Shaped};
pub use cellmap::{
    BlockCursor, Candidate, CellKey, Drawn, GlyphMap, MAP_BITS, MAP_SLOTS, MapEntry, MapGeneration,
    MapStats, NO_INK, Resolved, Selected, Shapes, Swaps, Text,
};
pub use color::{AppBg, Argb, PictureBg, Theme, Video};
pub use composite::{Blend, Target};
pub use instance::{Instance, Screen};
pub use metrics::{Metrics, PxSize};
pub use overlay::{MASK_PICTURE, MASK_RECORD, Overlay, OverlayStats, Scene};
pub use picture::{PICTURE_COLOURS, Pictured, Pictures};
pub use renderer::{
    Blink, Blinking, ClusterGlyph, Converted, Damage, Evicted, Exceptions, Frame, ImagePlacement,
    ImageUpload, PairRole, Pairs, Pass, Renderer, Single, StyleStamp, StyleSync, VideoFlash,
};
pub use scroll::{SLIDE_SECS_PER_ROW, Slide};
pub use scrollbar::{
    Bar, Hover, Look, Place, Scroll, Scroller, Thumb, Widths, Window, back_for, bar, gutter, place,
    thumb, track,
};
pub use select::{
    AbsLine, Clip, Grain, Highlight, Point, Press, Region, Scrolled, Selection, Span,
};
pub use stack::{FaceIx, Stack};
pub use trail::{
    Corners, Pt, Rect, Segment, Shape, Smear, Trail, TrailLook, coverage as quad_coverage,
};
pub use transition::Transition;
pub use units::{Band, Coverage, Digest, Tally};
