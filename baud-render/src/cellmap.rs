//! What a GPU needs to draw the raw cell grid itself: a glyph map keyed by
//! codepoint and the rendition table resolved to colours, both in the
//! layouts a shader reads.
//!
//! This is D2's other arm. The instance path converts every dirty row on
//! the CPU -- style resolution, shaping through the run cache, one record
//! per glyph -- and uploads the records. This path uploads the 80 KB grid
//! as it is and lets a vertex shader resolve each cell: the style through
//! [`Resolved`], the glyph through [`GlyphMap`]. What the CPU still does is
//! keep the map complete -- every codepoint on screen has an entry -- and
//! that is a walk over the cells rather than a conversion of them.
//!
//! **It draws a smaller program, and says so.** A cell is drawn alone:
//! no ligature across cells, no attached marks, no mark positioning. The
//! entry is what the shaper answers for the codepoint as a cluster of one,
//! so a single-glyph substitution still applies, but a `->` stays two
//! glyphs and an accent that is a separate codepoint is not drawn.
//! [`MapStats::dropped_marks`] counts the cells whose marks were skipped;
//! nothing can count the ligatures that would have formed without shaping
//! the runs, which is the work this path exists not to do.

use crate::atlas::{Code, Kind, Slot, Tex};
use crate::color::Argb;
use crate::instance::Screen;
use crate::renderer::Renderer;
use crate::units::Tally;
use baud_domain::{Codepoint, Viewport};

/// The map's slot count, a power of two so the probe wraps by a mask.
pub const MAP_BITS: u32 = 12;
pub const MAP_SLOTS: usize = 1 << MAP_BITS;

/// A map entry's key: the codepoint, or [`CellKey::EMPTY`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CellKey(pub u32);

impl CellKey {
    pub const EMPTY: CellKey = CellKey(u32::MAX);
}

/// The kind code an entry carries for a codepoint that draws nothing.
pub const NO_INK: Code = Code(3);

/// One glyph as a cell draws it alone: the atlas rectangle and where it
/// goes relative to the cell's origin, with the shaper's offsets folded
/// in. Twenty-four bytes, `repr(C)`: `u32 key, kind; u16 x, y, w, h;
/// i16 left, top; u32 pad`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapEntry {
    pub key: CellKey,
    pub kind: Code,
    pub x: Tex,
    pub y: Tex,
    pub w: Tex,
    pub h: Tex,
    pub left: Screen,
    pub top: Screen,
    pub pad: Code,
}

impl MapEntry {
    pub const BYTES: usize = 24;

    const EMPTY: MapEntry = MapEntry {
        key: CellKey::EMPTY,
        kind: NO_INK,
        x: Tex(0),
        y: Tex(0),
        w: Tex(0),
        h: Tex(0),
        left: Screen(0),
        top: Screen(0),
        pad: Code(0),
    };

    fn of(key: CellKey, slot: Option<Slot>) -> MapEntry {
        match slot {
            None => MapEntry {
                key,
                ..MapEntry::EMPTY
            },
            Some(s) => MapEntry {
                key,
                kind: s.kind.code(),
                x: s.x,
                y: s.y,
                w: s.w,
                h: s.h,
                left: Screen(i16::try_from(s.left).unwrap_or(i16::MAX)),
                top: Screen(i16::try_from(s.top).unwrap_or(i16::MAX)),
                pad: Code(0),
            },
        }
    }

    /// The entry's kind as the atlas names it; `None` for no ink.
    pub fn kind(&self) -> Option<Kind> {
        match self.kind {
            Code(0) => Some(Kind::Mask),
            Code(1) => Some(Kind::Color),
            Code(2) => Some(Kind::Solid),
            _ => None,
        }
    }
}

/// Counts every change to the entries, so a copy on the GPU knows when it
/// is stale.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MapGeneration(u64);

#[derive(Clone, Copy, Debug, Default)]
pub struct MapStats {
    pub live: Tally,
    pub misses: Tally,
    /// Cells whose attached marks this path does not draw.
    pub dropped_marks: Tally,
    /// Times the map was emptied: the atlas grew under it, or it filled.
    pub cleared: Tally,
}

/// The map's entries live where the caller put them -- a shared GPU
/// buffer, so the shader probes the very slots the CPU fills and nothing
/// is copied -- and the map borrows them for its life.
pub struct GlyphMap<'m> {
    entries: &'m mut [MapEntry],
    generation: MapGeneration,
    /// The atlas's grow count the entries were resolved against; a grown
    /// atlas has moved every slot.
    grown: Tally,
    stats: MapStats,
}

/// A position in the map.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct MapSlot(usize);

impl MapSlot {
    /// The slot a codepoint hashes to; the shader computes the same.
    fn home(cp: Codepoint) -> MapSlot {
        let h = cp.value().wrapping_mul(0x9E37_79B1) >> (32 - MAP_BITS);
        MapSlot(usize::try_from(h).unwrap_or(0))
    }

    fn next(self) -> MapSlot {
        MapSlot((self.0 + 1) & (MAP_SLOTS - 1))
    }
}

impl<'m> GlyphMap<'m> {
    /// A map over `entries`, which must hold [`MAP_SLOTS`]; every slot is
    /// emptied first.
    ///
    /// # Panics
    /// If `entries` is not exactly [`MAP_SLOTS`] long: the probe wraps by
    /// a mask, so any other length would index past the end.
    pub fn over(entries: &'m mut [MapEntry]) -> GlyphMap<'m> {
        assert!(
            entries.len() == MAP_SLOTS,
            "a glyph map is MAP_SLOTS entries"
        );
        entries.fill(MapEntry::EMPTY);
        GlyphMap {
            entries,
            generation: MapGeneration::default(),
            grown: Tally::default(),
            stats: MapStats::default(),
        }
    }

    /// Memory for a map, for a caller with nowhere better to put it.
    pub fn backing() -> Vec<MapEntry> {
        vec![MapEntry::EMPTY; MAP_SLOTS]
    }

    /// Gives the entries back, for a map made afresh over the same memory.
    pub fn into_entries(self) -> &'m mut [MapEntry] {
        self.entries
    }

    pub fn entries(&self) -> &[MapEntry] {
        self.entries
    }

    pub fn generation(&self) -> MapGeneration {
        self.generation
    }

    pub fn stats(&self) -> MapStats {
        self.stats
    }

    fn clear(&mut self) {
        self.entries.fill(MapEntry::EMPTY);
        self.stats.live = Tally::default();
        self.stats.cleared.bump();
        self.generation = MapGeneration(self.generation.0.wrapping_add(1));
    }

    /// The entry for `cp`, if the map holds one: linear probing from its
    /// home slot to the first empty one.
    pub fn probe(&self, cp: Codepoint) -> Option<&MapEntry> {
        let key = CellKey(cp.value());
        let mut i = MapSlot::home(cp);
        for _ in 0..MAP_SLOTS {
            let e = &self.entries[i.0];
            if e.key == CellKey::EMPTY {
                return None;
            }
            if e.key == key {
                return Some(e);
            }
            i = i.next();
        }
        None
    }

    fn insert(&mut self, entry: MapEntry) {
        let mut i = MapSlot::home(Codepoint::new(entry.key.0).unwrap_or(Codepoint::REPLACEMENT));
        for _ in 0..MAP_SLOTS {
            if self.entries[i.0].key == CellKey::EMPTY {
                self.entries[i.0] = entry;
                self.stats.live.bump();
                return;
            }
            i = i.next();
        }
    }

    /// Makes sure every drawable codepoint on the screen has an entry,
    /// resolving the missing ones through the renderer's shaper and atlas.
    /// Empties the map first when the atlas grew since the last call, and
    /// whenever it passes three quarters full, so a probe stays short.
    pub fn ensure(&mut self, vp: Viewport<'_>, r: &mut Renderer<'_, '_>) {
        if r.atlas_stats().grown.value() != self.grown.value() {
            self.grown = r.atlas_stats().grown;
            self.clear();
        }
        let mut changed = false;
        // **The screen, not the ring.** The grid holds the scrollback behind
        // the window, and a walk over `Term::cells` took every retained row
        // -- 2,050 where the screen is 50 -- which doubled the cell path's
        // frame on a scrolling recording. The viewport is the two slices of
        // the ring the screen occupies, and nothing is copied to walk them.
        let (head, tail) = vp.parts();
        for cell in head.iter().chain(tail) {
            if cell.flags.is_marked() {
                self.stats.dropped_marks.bump();
            }
            let Some(cp) = cell.content.as_codepoint() else {
                continue;
            };
            if self.probe(cp).is_some() {
                continue;
            }
            self.stats.misses.bump();
            let slot = r.single(cp);
            if r.atlas_stats().grown.value() != self.grown.value() {
                // Resolving this glyph grew the atlas: every entry before
                // it names a slot that moved.
                self.grown = r.atlas_stats().grown;
                self.clear();
            }
            if self.stats.live.value().saturating_mul(4)
                >= u64::try_from(MAP_SLOTS * 3).unwrap_or(0)
            {
                self.clear();
            }
            self.insert(MapEntry::of(CellKey(cp.value()), slot));
            changed = true;
        }
        if changed {
            self.generation = MapGeneration(self.generation.0.wrapping_add(1));
        }
    }
}

/// Which rendition shapes a cell draws, as the shader tests them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(transparent)]
pub struct Shapes(pub u32);

impl Shapes {
    pub const UNDERLINE: Shapes = Shapes(1);
    pub const STRIKE: Shapes = Shapes(2);
    /// The underline draws in the resolved foreground rather than in
    /// `underline`: what `SGR 58` unset means, and the cursor's swap
    /// applies to it.
    pub const UNDERLINE_FOLLOWS_FG: Shapes = Shapes(4);
    /// A second underline below the first: `SGR 21`, doubly underlined.
    pub const DOUBLE_UNDERLINE: Shapes = Shapes(8);

    pub fn with(self, s: Shapes) -> Shapes {
        Shapes(self.0 | s.0)
    }
}

/// One rendition table slot resolved to what a cell draws with: the
/// foreground, the background with the default background's alpha applied,
/// the underline colour, and which shapes to draw. Sixteen bytes,
/// `repr(C)`, four `u32`s.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Resolved {
    pub fg: Argb,
    pub bg: Argb,
    pub underline: Argb,
    pub shapes: Shapes,
}

impl Resolved {
    pub const BYTES: usize = 16;
}
