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
mod caret;
mod cellmap;
mod color;
mod composite;
pub mod effect;
mod find;
pub mod fire;
mod flash;
pub mod ice;
mod instance;
pub mod lightning;
pub mod liquid;
mod metrics;
pub mod orb;
mod overlay;
mod picture;
pub mod plasma;
pub mod print;
mod renderer;
mod scroll;
mod scrollbar;
mod select;
mod stack;
mod stress_data;
mod trail;
mod transition;
pub mod ufo;
mod units;

pub use atlas::{
    Atlas, AtlasStats, Code, Dirty, Generation, Kind, Replaced, Side, Slot, Stretch, Tex, TexRect,
    VAR_POINTS_MAX,
};
pub use cache::{CacheStats, PLACED_MAX, Placed, RUN_CELLS, RunCache, Shaped};
pub use caret::{CursorPhase, Play};
pub use cellmap::{
    BlockCursor, Candidate, CellKey, CellSpan, Curves, Drawn, GlyphMap, Ground, Hilite, Ink, Line,
    MAP_BITS, MAP_SLOTS, MapEntry, MapGeneration, MapStats, NO_INK, Reach, Resolved, Selected,
    Shapes, Swaps, Text, copy_for_shader, takes_stress_mark,
};
pub use color::{
    AppBg, Argb, Backdrop, Floor, MODUS_OPERANDI, PictureBg, Theme, Video, dark_palette,
    light_palette, xterm_palette,
};
pub use composite::{Blend, Blit, Page, Target, Texels};
pub use find::{Current, FIELD_COLS, Field, Find, FindLook, Mark, Position};
pub use flash::{Held, Points, Strength, capped, under_threshold, under_threshold_over};
pub use instance::{Instance, Screen};
pub use metrics::{Metrics, PxSize};
pub use overlay::{
    Caret, Composition, Hints, MASK_BARE, MASK_PICTURE, MASK_RECORD, MASK_SIZED, Overlay,
    OverlayStats, Scene,
};
pub use picture::{PICTURE_COLOURS, Pictured, Pictures};
pub use renderer::{
    Blink, BlinkPhases, Blinking, ClusterGlyph, Converted, Damage, Evicted, Exceptions, Frame,
    GlyphSource, Glyphs, ImageLayer, ImagePlacement, ImageScale, ImageUpload, PairRole, Pairs,
    Pass, Renderer, Single, StyleStamp, StyleSync, VideoFlash,
};
pub use scroll::{SLIDE_SECS_PER_ROW, Slide};
pub use scrollbar::{
    Bar, Hover, Look, Place, Scroll, Scroller, Thumb, Widths, Window, back_for, bar, gutter, place,
    thumb, track,
};
pub use select::{
    AbsLine, Clip, Grain, Highlight, Point, Press, Region, Scrolled, Selection, Span, char_col,
    row_of,
};
pub use stack::{EmojiDefault, FaceIx, FaceRole, FaceStyle, FaceTable, Presentation, Stack};
pub use stress_data::{STRESS_SKIP_BLOCKS, STRESS_SKIP_INDEX};
pub use trail::{
    Corners, Pt, Rect, Segment, Shape, Smear, Trail, TrailLook, coverage as quad_coverage,
};
pub use transition::Transition;
pub use units::{Band, Coverage, Digest, Tally};
