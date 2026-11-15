//! The compositor: `Term::viewport` in, pixels out.
//!
//! **The shape every shipping GPU terminal shares**, and the one measured
//! bottleneck among them: the CPU
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
// The effect blend, the level quad's distances and the filament
// exponentials run in core::simd lanes, each lane by the scalar's own
// operations, as baud-domain uses core::simd.
#![feature(portable_simd)]

mod atlas;
mod badge;
pub mod bell;
mod bidi;
pub mod boxes;
mod cache;
mod caret;
pub mod cell_frame;
mod cellmap;
mod color;
mod composite;
pub mod counters;
pub mod det;
pub mod effect;
mod find;
pub mod fire;
pub mod fireworks;
mod fit;
mod flash;
mod frozen;
mod guide;
pub mod ice;
mod instance;
mod kept;
pub mod lightning;
pub mod lights;
mod linked;
pub mod liquid;
pub mod message;
mod metrics;
pub mod orb;
mod overlay;
mod per_row;
mod picture;
pub mod plasma;
pub mod print;
mod rebreak;
mod redraw;
mod renderer;
pub mod screens;
mod scroll;
mod scrollbar;
mod select;
pub mod splash;
mod stack;
pub mod status;
/// The codepoints `SGR 64`'s stress mark skips, compiled by `build.rs`
/// (`build/stress.rs`) from `[[source: gc-data]]` and
/// `[[source: unicode-data]]` by `[[source: css-text-decor-3]]`'s rule:
/// Z*, P* but the punctuation that normalizes to one of its exceptions,
/// Cc, Cf and Cn -- and, for a cluster on a skipped base, the separators
/// and combining characters that decide whether it is "a space that
/// combines with any combining characters".
/// `the_stress_mark_skips_what_css_leaves_bare` recomputes the skip set
/// from the pinned files at every codepoint with a reader of its own, and
/// the cell shader's copy of the skip trie is pinned to this one by
/// `the_stress_mark_table_matches_the_shader`.
mod stress_data {
    include!(concat!(env!("OUT_DIR"), "/stress.rs"));
}
mod syllable;
pub mod tip;
mod word;
pub use word::{How as WordHow, Side as WordSide, WordLaid};
mod trail;
mod transition;
pub mod ufo;
mod units;
pub mod wrapmark;

pub use atlas::{
    Atlas, AtlasStats, Code, Dirty, Generation, Kind, Replaced, Rescaled, SIZES, Side, SizeKey,
    Slot, Stretch, Tex, TexRect, VAR_POINTS_MAX,
};
pub use badge::{BADGE, BOX, BadgeLook, DRAWN_AT, MARGINS, POINTS, STROKE};
pub use bell::{
    BellFlash, BellKind, BellShown, BellTint, PairFlash, Rang, ScreenFlash, ScreenShows, VideoMoved,
};
pub use bidi::{
    Around, CONTEXT, Changes, CursorSide, DrawnRows, Joined, Joins, LaidSeal, Mirrored, Orders,
    PairEnd, PairPart, Rejoin, Reorder, RowJoins, WordCell, WordIx, Work as BidiWork, Written,
};
pub use cache::{CacheStats, PLACED_MAX, Placed, RUN_CELLS, RunCache, Shaped};
pub use caret::{CursorPhase, Play};
pub use cellmap::{
    BlockCursor, BlockPaint, Candidate, CellKey, CellSpan, CursorLook, Curves, Drawn, GlyphMap,
    Ground, Hilite, Ink, Line, MAP_BITS, MAP_SLOTS, MapEntry, MapGeneration, MapStats, NO_INK,
    Reach, Resolved, Selected, Shapes, Swaps, Text, WalkKey, copy_for_shader, takes_stress_mark,
};
pub use color::{
    AppBg, Argb, Backdrop, BaudCursor, CURSOR_CONTRAST, CursorMade, CursorPages, Floor,
    MODUS_OPERANDI, PictureBg, SixelBg, Theme, Video, contrast_over, dark_palette, light_palette,
    xterm_palette,
};
pub use composite::{Blend, Blit, Mix, Page, Target, Texels};
pub use find::{Current, FIELD_COLS, Field, Find, FindLook, Mark, Position, Query, Typing};
pub use flash::{
    Held, Points, Strength, capped, under_threshold, under_threshold_by, under_threshold_over,
    under_threshold_over_by,
};
pub use frozen::{Lend, Lending, Lent, RowSource, Words};
pub use guide::{CURSOR_GUIDE, GuideLook};
pub use instance::{Instance, Screen};
pub use lights::{
    AREA_LIMIT, Activity, ActivityLights, Ansi, Bevel, ByLed, ByLight, DEFAULT_GLOW, FlashClause,
    Glow, Half, Halo, Led, Level, Light, LightsColours, LightsImage, LightsLook, LineEdge,
    LineRows, PIXELS_A_POINT, Pace, READ_CEILING, Shown, StatusLineLights, WORD_CELLS,
    flash_clause, line_lamps,
};
pub use linked::{
    HoverPaint, LINK_TEXT_CONTRAST, LineColour, LineStyle, LinkHover, LinkLine, LinkLook, LinkSpan,
    MISSING_CONTRAST, MissingPaint,
};
pub use message::{MessageColours, MessagePaint, MessagePlace, Pair, SelectionPulse};
pub use metrics::{Metrics, PxSize};
pub use overlay::{
    Caret, Composition, CursorDrawn, Hints, Laid, MASK_BARE, MASK_BIDI, MASK_PICTURE, MASK_RECORD,
    MASK_SELECTED, MASK_SIZED, Measure, Overlay, OverlayStats, Scene,
};
pub use picture::{PICTURE_COLOURS, Pictured, Pictures};
pub use rebreak::{Display, WordWrap};
pub use redraw::{REDRAW_ROWS, Redraw, RowSet};
pub use renderer::{
    Blink, BlinkPhases, Blinking, ClusterGlyph, Converted, CopyOrder, Damage, Decorations, Evicted,
    Exceptions, Frame, GlyphSource, Glyphs, ImageComposite, ImageLayer, ImagePlacement, ImageScale,
    ImageUpload, PairRole, Pairs, Pass, Renderer, Single, StruckCells, StruckGen, StyleStamp,
    StyleSync, VideoFlash,
};
pub use scroll::{SLIDE_SECS_PER_ROW, Slide};
pub use scrollbar::{
    Bar, Hover, Look, Place, Scroll, Scroller, Thumb, Widths, Window, back_for, bar, gutter, place,
    thumb, track,
};
pub use select::{
    AbsLine, Beyond, Clip, Grain, Highlight, Point, Press, Region, Scrolled, Selection, Span,
    WindowLine, char_col, row_of, row_styles_of,
};
pub use splash::{SplashLook, SplashPhase};
pub use stack::{
    EmojiDefault, FaceIx, FaceRole, FaceStyle, FaceTable, FontSlot, Presentation, Stack, TextFace,
    narrower_widths,
};
pub use status::{Leaving, StatusMarks};
pub use stress_data::{STRESS_SKIP_BLOCKS, STRESS_SKIP_INDEX};
pub use tip::{
    MESSAGE_FOR, MessageAt, Opens, TIP_COLS, TipAnchor, TipArea, TipLook, message_look, tip_look,
    tip_look_opening,
};
pub use trail::{
    Corners, Pt, Rect, Segment, Shape, Smear, TAU_SLOW, Trail, TrailLook, coverage as quad_coverage,
};
pub use transition::{
    Frame as SlideFrame, Offsets, SLIDE_S, ScreenSlide, Toward, Transition, VeilFrom,
    compose as compose_slide, faint_alpha, offsets as slide_offsets, source as slide_source,
    travel as slide_travel,
};
pub use units::{Band, Coverage, Digest, Tally};
pub use wrapmark::WrapMarker;
