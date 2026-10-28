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
mod badge;
mod bidi;
pub mod boxes;
mod cache;
mod caret;
mod cellmap;
mod color;
mod composite;
pub mod det;
pub mod effect;
mod find;
pub mod fire;
mod flash;
mod guide;
pub mod ice;
mod instance;
pub mod lightning;
pub mod lights;
mod linked;
pub mod liquid;
mod metrics;
pub mod orb;
mod overlay;
mod picture;
pub mod plasma;
pub mod print;
mod rebreak;
mod redraw;
mod renderer;
mod scroll;
mod scrollbar;
mod select;
pub mod splash;
mod stack;
pub mod status;
mod stress_data;
mod trail;
mod transition;
pub mod ufo;
mod units;

pub use atlas::{
    Atlas, AtlasStats, Code, Dirty, Generation, Kind, Replaced, Rescaled, SIZES, Side, SizeKey,
    Slot, Stretch, Tex, TexRect, VAR_POINTS_MAX,
};
pub use badge::{BADGE, BOX, BadgeLook, MARGINS, POINTS};
pub use bidi::{
    Around, CONTEXT, Changes, CursorSide, Joined, Joins, Mirrored, Orders, PairEnd, PairPart,
    Rejoin, Reorder, RowJoins, WordCell, WordIx, Work as BidiWork, Written,
};
pub use cache::{CacheStats, PLACED_MAX, Placed, RUN_CELLS, RunCache, Shaped};
pub use caret::{CursorPhase, Play};
pub use cellmap::{
    BlockCursor, BlockPaint, Candidate, CellKey, CellSpan, CursorLook, Curves, Drawn, GlyphMap,
    Ground, Hilite, Ink, Line, MAP_BITS, MAP_SLOTS, MapEntry, MapGeneration, MapStats, NO_INK,
    Reach, Resolved, Selected, Shapes, Swaps, Text, WalkKey, copy_for_shader, takes_stress_mark,
};
pub use color::{
    AppBg, Argb, Backdrop, Floor, MODUS_OPERANDI, PictureBg, SixelBg, Theme, Video, dark_palette,
    light_palette, xterm_palette,
};
pub use composite::{Blend, Blit, Page, Target, Texels};
pub use find::{Current, FIELD_COLS, Field, Find, FindLook, Mark, Position, Query, Typing};
pub use flash::{Held, Points, Strength, capped, under_threshold, under_threshold_over};
pub use guide::{CURSOR_GUIDE, GuideLook};
pub use instance::{Instance, Screen};
pub use lights::{
    Activity, ActivityLights, ByShown, GROUND_DARK, GROUND_LIGHT, Glow, Light, LightsImage,
    LightsLook, Shown,
};
pub use linked::{LinkLook, LinkSpan};
pub use metrics::{Metrics, PxSize};
pub use overlay::{
    Caret, Composition, Hints, Laid, MASK_BARE, MASK_BIDI, MASK_PICTURE, MASK_RECORD,
    MASK_SELECTED, MASK_SIZED, Measure, Overlay, OverlayStats, Scene,
};
pub use picture::{PICTURE_COLOURS, Pictured, Pictures};
pub use rebreak::{Display, WordWrap};
pub use redraw::{REDRAW_ROWS, Redraw, RowSet};
pub use renderer::{
    Blink, BlinkPhases, Blinking, ClusterGlyph, Converted, Damage, Decorations, Evicted,
    Exceptions, Frame, GlyphSource, Glyphs, ImageComposite, ImageLayer, ImagePlacement, ImageScale,
    ImageUpload, PairRole, Pairs, Pass, Renderer, Single, StyleStamp, StyleSync, VideoFlash,
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
pub use splash::{SplashLook, SplashPhase};
pub use stack::{
    EmojiDefault, FaceIx, FaceRole, FaceStyle, FaceTable, FontSlot, Presentation, Stack, TextFace,
};
pub use status::{Leaving, StatusMarks};
pub use stress_data::{STRESS_SKIP_BLOCKS, STRESS_SKIP_INDEX};
pub use trail::{
    Corners, Pt, Rect, Segment, Shape, Smear, Trail, TrailLook, coverage as quad_coverage,
};
pub use transition::Transition;
pub use units::{Band, Coverage, Digest, Tally};
