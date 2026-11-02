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

use crate::atlas::{Code, Kind, Tex};
use crate::color::Argb;
use crate::instance::Screen;
use crate::picture::Pictured;
use crate::renderer::{BlinkPhases, PairRole, Renderer, Single};
use crate::units::Tally;
use baud_core::Term;
use baud_domain::contrast::CurveRow;
use baud_domain::themefile::CellPaint;
use baud_domain::{Chan, Codepoint, Col, Cols, Content, GlyphId, PackedCell, Row, Viewport};

/// A cell that ends an adjacent pair the face's rules can begin on: the
/// second of the two, on the screen the map was ensured for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub row: Row,
    pub col: Col,
    /// The face both cells draw on, as the map entry codes it.
    pub face: Code,
}

/// The map's slot count, a power of two so the probe wraps by a mask.
pub const MAP_BITS: u32 = 14;
pub const MAP_SLOTS: usize = 1 << MAP_BITS;

/// The face style and font a cell's text is drawn from, read from the
/// resolved table the frame draws with -- the one spelling of that read on
/// the CPU, as the shader's `(shapes >> FACE_SHIFT) & 3` and `(shapes >>
/// FONT_SHIFT)` are on the GPU.
pub fn face_of(styles: &[Resolved], cell: baud_domain::PackedCell) -> crate::TextFace {
    styles
        .get(cell.style.index())
        .map_or(crate::TextFace::default(), |s| s.shapes.text_face())
}

/// **A row copied for the cell shader to draw from the map alone**: each
/// cell as `src` holds it, but a cell with continuations given its base in
/// place of its slot's word.
///
/// The shader draws a cell's glyph by looking its content up in the map,
/// and draws no glyph for content above U+10FFFF -- which a cell naming a
/// cluster now holds. On a screen row that is right: the overlay masks the
/// cell and a record draws its whole cluster. The leaving row of a slide is
/// masked the same way on its own row of the mask, the one after the
/// screen's (`Overlay::leaving`), and where that row is not masked -- no
/// walk ran over it -- a marked cell there draws its base, what it drew
/// when the base was the cell's content; its stress mark is judged from
/// the base too. The app copies that row with this; `term` is what `src`
/// was read from, in the same borrow.
pub fn copy_for_shader(term: &Term<'_>, src: &[PackedCell], dst: &mut [PackedCell]) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d = match s.flags.is_marked() {
            true => PackedCell {
                content: term.base_of(*s).drawn(),
                ..*s
            },
            false => *s,
        };
    }
}

/// Which side of a cell's colours its glyph is drawn in: its own
/// foreground, or its background after an odd number of swaps -- the
/// selection's and the block cursor's (`Resolved::drawn`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ink {
    Own,
    Swapped,
}

impl Swaps {
    /// Which side the glyph is drawn in: each swap exchanges the two
    /// colours and `OSC 12` replaces only the background, so the glyph is
    /// the background's colour exactly when the swaps are odd. Held to
    /// `Resolved::drawn` by `the_ink_is_the_drawn_foregrounds_row`.
    pub fn ink(self) -> Ink {
        let selected = self.selected == Selected::Yes;
        // A fading block counts as a swap from half way: the glyph's colour
        // is then nearer the swapped side, and the shader's `glyph_curve`
        // takes the same half.
        let block = matches!(self.block, BlockCursor::Yes(_)) && self.cursor.get() >= 128;
        if selected != block {
            Ink::Swapped
        } else {
            Ink::Own
        }
    }
}

/// How many cells a character takes on the grid.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CellSpan {
    One,
    Two,
}

impl CellSpan {
    /// The span of the character at `x`: two when the next cell is its
    /// wide tail. Read from the grid, because the grid says it there --
    /// `CellFlags::is_wide` is the cell table's advance, and on a grid cell
    /// its two bits are `PROTECTED` and `GRAPHICS`, which made every DEC
    /// graphics cell "wide" and the CPU and instance paths draw every
    /// other one.
    pub fn at(row: &[baud_domain::PackedCell], x: Col) -> CellSpan {
        match row.get(x.index() + 1) {
            Some(c) if c.content == Content::WIDE_TAIL => CellSpan::Two,
            _ => CellSpan::One,
        }
    }

    /// How many cells the span takes.
    pub fn cells(self) -> Cols {
        match self {
            CellSpan::One => Cols::of_index(1),
            CellSpan::Two => Cols::of_index(2),
        }
    }

    /// The column past a character at `x` that takes this span.
    pub fn after(self, x: Col) -> Col {
        match self {
            CellSpan::One => Col::at_index(x.index() + 1),
            CellSpan::Two => Col::at_index(x.index() + 2),
        }
    }
}

/// A map entry's key: the codepoint with the face style above it, and the
/// span above that, or [`CellKey::EMPTY`]. A codepoint is 21 bits, so the
/// style's two bits and the span's one fit above it and the key stays one
/// word; the cell shader composes the
/// same key from the cell and its resolved slot (`KEY_STYLE_SHIFT`). The
/// font ([`crate::FontSlot`]) takes the bits left: its Fraktur bit between
/// the style and the span, its alternative in the top four, clear of
/// [`crate::boxes::Ring::KEY`]'s bit 27, so a primary-font key is the word
/// it was before fonts were drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CellKey(pub u32);

impl CellKey {
    pub const EMPTY: CellKey = CellKey(u32::MAX);
    pub const STYLE_SHIFT: u32 = 21;
    /// One bit: set when the character spans two cells, whose glyph is
    /// centred in them (`Renderer::phase_in`) and so is another slot.
    pub const SPAN_SHIFT: u32 = 26;
    /// One bit: `SGR 20`'s Fraktur face is over the font.
    pub const FRAKTUR_SHIFT: u32 = 23;
    /// Four bits: the alternative font, `SGR 11`-`19`, or none.
    pub const FONT_SHIFT: u32 = 28;

    pub fn of(cp: Codepoint, face: impl Into<crate::TextFace>, span: CellSpan) -> CellKey {
        let face = face.into();
        let two = match span {
            CellSpan::One => 0,
            CellSpan::Two => 1,
        };
        let font = face.font.code().0;
        CellKey(
            cp.value()
                | (face.style.code().0 << CellKey::STYLE_SHIFT)
                | ((font >> 4) << CellKey::FRAKTUR_SHIFT)
                | (two << CellKey::SPAN_SHIFT)
                | ((font & 15) << CellKey::FONT_SHIFT),
        )
    }
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

    /// The pad word: the glyph the entry draws, and its pair role, so the
    /// walk that probes the map learns both from the entry it holds.
    fn pad_of(single: Option<Single>) -> Code {
        let Some(s) = single else {
            return Code(0);
        };
        let glyph = u32::from(u16::try_from(s.glyph.index()).unwrap_or(0));
        let face = u32::from(u8::try_from(s.face.index()).unwrap_or(0)) << MapEntry::FACE_SHIFT;
        let role = match s.role {
            PairRole::Neither => 0,
            PairRole::Starts => MapEntry::STARTS,
            PairRole::Ends => MapEntry::ENDS,
            PairRole::Both => MapEntry::STARTS | MapEntry::ENDS,
        };
        let several = match s.glyphs {
            crate::Glyphs::One => 0,
            crate::Glyphs::Several => MapEntry::SEVERAL,
        };
        let past = match s.overflow {
            crate::fit::Overflow::Inside => 0,
            crate::fit::Overflow::Past => MapEntry::PAST,
        };
        Code(glyph | role | face | several | past)
    }

    const STARTS: u32 = 1 << 16;
    const ENDS: u32 = 1 << 17;
    const FACE_SHIFT: u32 = 18;
    /// Past the face's eight bits: the glyph is the first of several.
    const SEVERAL: u32 = 1 << 26;
    /// The glyph is a letter of a joining script reaching past its cell,
    /// which rule D may fit (`crate::fit`).
    const PAST: u32 = 1 << 27;

    /// The face the entry's glyph is on, as the code the renderer indexes
    /// its faces by.
    pub fn face(&self) -> Code {
        Code((self.pad.0 >> MapEntry::FACE_SHIFT) & 0xff)
    }

    fn of(key: CellKey, single: Option<Single>) -> MapEntry {
        let pad = MapEntry::pad_of(single);
        match single.and_then(|s| s.slot) {
            None => MapEntry {
                key,
                pad,
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
                pad,
            },
        }
    }

    /// An entry drawing `slot` under `key`, with no glyph behind it: an
    /// encircled run's outline ([`crate::boxes::Ring`]).
    fn of_slot(key: CellKey, slot: Option<crate::atlas::Slot>) -> MapEntry {
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

    /// The glyph the entry draws.
    pub fn glyph(&self) -> GlyphId {
        GlyphId::from_shaper(u16::try_from(self.pad.0 & 0xffff).unwrap_or(0))
    }

    pub fn starts_pair(&self) -> bool {
        self.pad.0 & MapEntry::STARTS != 0
    }

    pub fn ends_pair(&self) -> bool {
        self.pad.0 & MapEntry::ENDS != 0
    }

    /// **Whether the overlay draws the cell whole**, by a record: the
    /// codepoint shapes to several glyphs alone, which an entry cannot
    /// draw, or it is a letter of a joining script reaching past its cell,
    /// which rule D draws full size or fitted by the cells beside it
    /// (`crate::fit`), which an entry cannot know.
    pub fn listed(&self) -> bool {
        self.glyphs() == crate::Glyphs::Several || self.reaches_past()
    }

    /// Whether the glyph is a letter of a joining script reaching past its
    /// cell, which rule D draws full size or fitted by the cells beside it
    /// (`crate::fit`).
    pub fn reaches_past(&self) -> bool {
        self.pad.0 & MapEntry::PAST != 0
    }

    /// How many glyphs the codepoint shapes to alone: the entry draws
    /// the first, which is all of it only for one.
    pub fn glyphs(&self) -> crate::Glyphs {
        match self.pad.0 & MapEntry::SEVERAL {
            0 => crate::Glyphs::One,
            _ => crate::Glyphs::Several,
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

/// How far the ink of the glyphs a map holds reaches from a cell's origin
/// on its baseline, in pixels: the least left and top edge of any, and the
/// most right and bottom. Where a glyph the map draws can lie.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reach {
    pub left: Screen,
    pub top: Screen,
    pub right: Screen,
    pub bottom: Screen,
}

impl Reach {
    /// No glyph: every edge past the others, so nothing lies inside.
    pub const NONE: Reach = Reach {
        left: Screen(i16::MAX),
        top: Screen(i16::MAX),
        right: Screen(i16::MIN),
        bottom: Screen(i16::MIN),
    };

    fn with(self, e: &MapEntry) -> Reach {
        let far = |a: i16, b: u16| Screen(a.saturating_add(i16::try_from(b).unwrap_or(i16::MAX)));
        Reach {
            left: Screen(self.left.0.min(e.left.0)),
            top: Screen(self.top.0.min(e.top.0)),
            right: Screen(self.right.0.max(far(e.left.0, e.w.0).0)),
            bottom: Screen(self.bottom.0.max(far(e.top.0, e.h.0).0)),
        }
    }
}

/// **What the glyph map's walk over a screen read**, beside the screen's
/// cells: a frame whose cells and line renditions are the last walk's, and
/// whose key is equal, skips the walk. The walk only adds an entry for a
/// glyph the screen draws and lists the screen's candidates; on the same
/// cells, under the same rendition table (`table`), with the map as the
/// walk left it (`map`), no atlas grown under it (`grown`) and no cluster
/// slot reused (`clusters`), it would find every entry present and list
/// the same candidates. What the caller checks of the cells is its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WalkKey {
    pub table: crate::StyleStamp,
    pub map: MapGeneration,
    pub grown: Tally,
    pub clusters: baud_domain::ClusterEpoch,
    pub back: baud_domain::Rows,
    pub cols: baud_domain::Cols,
    pub rows: baud_domain::Rows,
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
    /// Adjacent cells whose entries could begin and end a pair, so the
    /// face's digest was consulted.
    pub pair_tests: Tally,
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
    /// The cells whose codepoint shapes to several glyphs alone, and those
    /// holding a letter of a joining script that reaches past its cell
    /// ([`MapEntry::listed`]), in screen order, from the last walk: the
    /// overlay draws them by records.
    several: Vec<(Row, Col)>,
    /// How many entries since the map was last emptied draw the first of
    /// several glyphs or a letter that reaches past its cell.
    decomposed: Tally,
    /// Where the ink of the entries since the map was last emptied lies.
    reach: Reach,
}

/// A position in the map.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct MapSlot(usize);

impl MapSlot {
    /// The slot a key hashes to; the shader computes the same.
    fn home(key: CellKey) -> MapSlot {
        let h = key.0.wrapping_mul(0x9E37_79B1) >> (32 - MAP_BITS);
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
            several: Vec::new(),
            decomposed: Tally::default(),
            reach: Reach::NONE,
        }
    }

    /// Memory for a map, for a caller with nowhere better to put it.
    pub fn backing() -> Vec<MapEntry> {
        vec![MapEntry::EMPTY; MAP_SLOTS]
    }

    /// Gives the entries back, for a map made afresh over the same memory
    /// -- or, since a rescale invalidates every entry and `over` fills all
    /// 16,384 slots, for a caller holding a *second* buffer to build the
    /// next map over that one instead. Filling the buffer the shader is
    /// probing is the write race; ping-ponging two costs a pointer and no
    /// allocation.
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

    /// Where a glyph the map draws can lie, from its cell's origin on the
    /// baseline: no entry it holds reaches further.
    pub fn reach(&self) -> Reach {
        self.reach
    }

    /// The cells on the last walk's screen whose codepoint shapes to
    /// several glyphs alone, which no entry can draw whole, and those
    /// holding a letter of a joining script that reaches past its cell,
    /// which rule D judges against the cells beside it, in screen order.
    pub fn several(&self) -> &[(Row, Col)] {
        &self.several
    }

    fn clear(&mut self) {
        self.entries.fill(MapEntry::EMPTY);
        self.stats.live = Tally::default();
        self.decomposed = Tally::default();
        self.reach = Reach::NONE;
        self.stats.cleared.bump();
        self.generation = MapGeneration(self.generation.0.wrapping_add(1));
    }

    /// The entry for `cp` drawn in `face`, if the map holds one: linear
    /// probing from its home slot to the first empty one.
    pub fn probe(
        &self,
        cp: Codepoint,
        face: impl Into<crate::TextFace>,
        span: CellSpan,
    ) -> Option<&MapEntry> {
        self.probe_key(CellKey::of(cp, face, span))
    }

    /// The entry under `key`, if the map holds one.
    fn probe_key(&self, key: CellKey) -> Option<&MapEntry> {
        let mut i = MapSlot::home(key);
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
        if entry.kind().is_some() {
            self.reach = self.reach.with(&entry);
        }
        let mut i = MapSlot::home(entry.key);
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
    /// Every cell's entry present, and the cells that end a pair the
    /// face's rules can begin on, in screen order, into `candidates` --
    /// found on the same probe, from the two bits each entry carries: the
    /// digests are consulted only where a cell that can end a pair follows
    /// one that can start one.
    ///
    /// `styles` is the resolved rendition table the frame draws with,
    /// synced before this walk: a cell's face style is read from its slot,
    /// as the shader reads it, so the two build and probe the same keys.
    ///
    /// `term` is what `vp` was read from, in the same borrow: a cell with
    /// continuations holds its cluster's slot rather than its base, and its
    /// base -- which takes an entry like any other cell's and can begin or
    /// end a pair -- is read back from the store ([`Term::base_of`]).
    pub fn ensure(
        &mut self,
        term: &Term<'_>,
        vp: Viewport<'_>,
        styles: &[Resolved],
        r: &mut Renderer<'_, '_>,
        candidates: &mut Vec<Candidate>,
    ) {
        if r.atlas_stats().grown.value() != self.grown.value() {
            self.grown = r.atlas_stats().grown;
            self.clear();
        }
        let mut changed = self.rings(r);
        // The cells a decomposed letter or one reaching past its cell is in
        // are listed only while the map holds an entry for one, so a screen
        // that never showed one walks as it did before they were listed.
        // The walk that meets the map's first such entry lists none before
        // it, and walks again.
        let tallies = (self.stats.dropped_marks, self.stats.pair_tests);
        let listed = self.decomposed.value() > 0;
        changed |= match listed {
            false => self.walk::<false>(term, vp, styles, r, candidates),
            true => self.walk::<true>(term, vp, styles, r, candidates),
        };
        if !listed && self.decomposed.value() > 0 {
            (self.stats.dropped_marks, self.stats.pair_tests) = tallies;
            changed |= self.walk::<true>(term, vp, styles, r, candidates);
        }
        // A clear on the walk took the rings' entries with it.
        changed |= self.rings(r);
        if changed {
            self.generation = MapGeneration(self.generation.0.wrapping_add(1));
        }
    }

    /// One walk over the screen for [`GlyphMap::ensure`]: every cell's
    /// entry made present, the pair candidates into `candidates`, and with
    /// `LIST` the cells the overlay draws whole ([`MapEntry::listed`]) into
    /// [`GlyphMap::several`]. Whether an entry was added.
    fn walk<const LIST: bool>(
        &mut self,
        term: &Term<'_>,
        vp: Viewport<'_>,
        styles: &[Resolved],
        r: &mut Renderer<'_, '_>,
        candidates: &mut Vec<Candidate>,
    ) -> bool {
        candidates.clear();
        self.several.clear();
        let mut changed = false;
        let cols = vp.cols().index();
        for y in 0..vp.rows().index() {
            let row = vp.row(Row::from_index(y));
            let mut prev: Option<(GlyphId, Code)> = None;
            for (x, cell) in row.iter().enumerate().take(cols) {
                if cell.flags.is_marked() {
                    self.stats.dropped_marks.bump();
                }
                let Some(cp) = crate::renderer::drawn_base(term, *cell) else {
                    if cell.content != Content::WIDE_TAIL {
                        prev = None;
                    }
                    continue;
                };
                let style = face_of(styles, *cell);
                let span = CellSpan::at(row, Col::at_index(x));
                let (glyph, starts, ends, face) = match self.probe(cp, style, span).copied() {
                    Some(e) => {
                        if LIST && e.listed() {
                            self.several.push((Row::from_index(y), Col::at_index(x)));
                        }
                        (e.glyph(), e.starts_pair(), e.ends_pair(), e.face())
                    }
                    None => {
                        let e = self.miss(r, cp, style, span);
                        changed = true;
                        if LIST && e.listed() {
                            self.several.push((Row::from_index(y), Col::at_index(x)));
                        }
                        (e.glyph(), e.starts_pair(), e.ends_pair(), e.face())
                    }
                };
                if let Some((pg, pf)) = prev {
                    if ends && pf == face {
                        self.stats.pair_tests.bump();
                        if r.admits(face, pg, glyph) {
                            candidates.push(Candidate {
                                row: Row::from_index(y),
                                col: Col::at_index(x),
                                face,
                            });
                        }
                    }
                }
                prev = starts.then_some((glyph, face));
            }
        }
        changed
    }

    /// The entry for a cell the map has none for, made and inserted. Out
    /// of line, so each copy of the walk holds only the probe: inlined into
    /// both, it cost 0.7% of the instructions of a `frame_gpu` run of btop
    /// on the cell path and 1.0% of vim's.
    #[cold]
    #[inline(never)]
    fn miss(
        &mut self,
        r: &mut Renderer<'_, '_>,
        cp: Codepoint,
        style: crate::TextFace,
        span: CellSpan,
    ) -> MapEntry {
        self.stats.misses.bump();
        let single = r.single(cp, style, span);
        if r.atlas_stats().grown.value() != self.grown.value() {
            self.grown = r.atlas_stats().grown;
            self.clear();
        }
        if self.stats.live.value().saturating_mul(4) >= u64::try_from(MAP_SLOTS * 3).unwrap_or(0) {
            self.clear();
        }
        let e = MapEntry::of(CellKey::of(cp, style, span), single);
        self.insert(e);
        if e.listed() {
            self.decomposed.bump();
        }
        e
    }

    /// Makes sure the map holds the three parts of an encircled run's
    /// outline under their keys ([`crate::boxes::Ring::key`]), which the
    /// cell shader probes as it probes a cell's glyph -- once any slot the
    /// renderer resolved has been encircled, so a screen that never was
    /// draws no ring into the atlas. Whether an entry was added.
    fn rings(&mut self, r: &mut Renderer<'_, '_>) -> bool {
        use crate::boxes::Ring;
        if !r.rings_used() {
            return false;
        }
        let mut changed = false;
        // Twice at most: a part drawn into a full atlas grows it, which
        // moves every slot and empties the map, and the parts already in
        // go in again at the new places.
        for _ in 0..2 {
            let mut grew = false;
            for ring in [Ring::Starts, Ring::Ends, Ring::Alone] {
                let key = ring.key();
                if self.probe_key(key).is_some() {
                    continue;
                }
                let slot = r.ring_slot(ring);
                if r.atlas_stats().grown.value() != self.grown.value() {
                    self.grown = r.atlas_stats().grown;
                    self.clear();
                    grew = true;
                }
                self.insert(MapEntry::of_slot(key, slot));
                changed = true;
            }
            if !grew {
                break;
            }
        }
        changed
    }
}

/// Whether a character takes `SGR 64`'s stress mark: one "drawn once for
/// each typographic character unit" (`[[source: css-text-decor-3]]`), so
/// not a blank and not a wide character's tail (its lead draws the mark
/// across both cells), and not one the web's emphasis marks leave bare.
/// `base` is the character's base -- a cell's own codepoint, or its
/// cluster's (`Term::base_of`) -- and `marks` what attaches to it.
///
/// A unit is classed by its base: Z*, P* outside CSS's exceptions, Cc, Cf
/// and Cn are bare, which `STRESS_SKIP_INDEX` and `STRESS_SKIP_BLOCKS`
/// hold as a two-stage bit trie, two loads. The one way a bare base
/// takes the mark is the Z* bullet's own: "emphasis marks are drawn for a
/// space that combines with any combining characters", a combining
/// character being gc M* (`[[source: unicode17-ch3]]` D52), so a comma
/// with an acute accent stays bare and a space with one is marked. A base
/// above U+10FFFF, a slot the store no longer holds, takes it, as the
/// cell shader's `takes_stress_mark` does for a cluster the overlay has
/// not marked bare ([`crate::MASK_BARE`]).
pub fn takes_stress_mark(base: Content, marks: impl IntoIterator<Item = u32>) -> bool {
    use crate::stress_data::{
        COMBINING_BLOCKS, COMBINING_INDEX, SEPARATORS, STRESS_SKIP_BLOCKS, STRESS_SKIP_INDEX,
    };
    if base.is_empty() || base.is_wide_tail() {
        return false;
    }
    let Some(c) = base.as_codepoint() else {
        return true;
    };
    if !in_trie(c, STRESS_SKIP_INDEX, STRESS_SKIP_BLOCKS) {
        return true;
    }
    SEPARATORS.binary_search(&c.value()).is_ok()
        && marks
            .into_iter()
            .filter_map(Codepoint::new)
            .any(|m| in_trie(m, COMBINING_INDEX, COMBINING_BLOCKS))
}

/// Whether `c` is in a two-stage bit trie: stage 1 maps the codepoint's
/// high bits (`>> 8`) to a block, stage 2 holds each block's 256 bits,
/// low bit first.
fn in_trie(c: Codepoint, index: &[u8], blocks: &[u8]) -> bool {
    let v = c.value();
    let block = usize::try_from(v >> 8)
        .ok()
        .and_then(|i| index.get(i))
        .map_or(0, |&b| usize::from(b));
    let at = block * 32 + usize::try_from((v & 0xff) >> 3).unwrap_or(0);
    blocks
        .get(at)
        .is_some_and(|&byte| (byte >> (v & 7)) & 1 == 1)
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
    /// `SGR 4:3` -- curly. A zigzag one thickness in amplitude across the
    /// cell, drawn in segments so it needs no texel; the shape a diagnostic
    /// squiggle is.
    pub const CURLY_UNDERLINE: Shapes = Shapes(16);
    /// `SGR 4:4` -- dotted: one thickness on, one off, across the cell.
    pub const DOTTED_UNDERLINE: Shapes = Shapes(32);
    /// `SGR 4:5` -- dashed: two thicknesses on, one off.
    pub const DASHED_UNDERLINE: Shapes = Shapes(64);
    /// This rendition's background is drawn opaque whatever the window's
    /// dim: [`crate::Theme::opaque_bg`], resolved once a slot so the
    /// shader reads one bit.
    pub const OPAQUE_BG: Shapes = Shapes(128);
    /// `SGR 5`: the text draws only in the slow blink's on phase. The phase
    /// is the frame's and not the slot's, so a slot resolves the same in both
    /// and a flip re-resolves nothing; each path hides the text where it
    /// draws it ([`Resolved::text`]). `SHAPE_BLINK` in the cell shader,
    /// pinned by `the_hidden_text_bits_match_the_shader`.
    pub const BLINK: Shapes = Shapes(256);
    /// `SGR 8`: the text never draws. `SHAPE_CONCEAL` in the cell shader.
    pub const CONCEAL: Shapes = Shapes(512);
    /// `SGR 6`: the text draws only in the rapid blink's on phase, a phase of
    /// its own ([`crate::BlinkPhases`]). Above the face style's two bits.
    /// `SHAPE_RAPID_BLINK` in the cell shader.
    pub const RAPID_BLINK: Shapes = Shapes(4096);
    /// The background this rendition draws is the default background
    /// colour ([`crate::Theme::default_bg`]): what a kitty image below
    /// `INT32_MIN/2` shows through. Resolved once a slot, so no cell bit
    /// holds it. `SHAPE_DEFAULT_BG` in the cell shader, pinned by
    /// `the_default_background_bit_matches_the_shader`.
    pub const DEFAULT_BG: Shapes = Shapes(8192);
    /// A line across the top of the cell: `SGR 53`, overlined, and `SGR
    /// 62`, "ideogram overline", which horizontal text draws alike
    /// (`[[source: ecma48]]` § 8.3.117). In the text's colour, as the
    /// strike is. `SHAPE_OVERLINE` in the cell shader.
    pub const OVERLINE: Shapes = Shapes(16384);
    /// A second line below the overline: `SGR 63`, "ideogram double
    /// overline". `SHAPE_DOUBLE_OVERLINE` in the cell shader.
    pub const DOUBLE_OVERLINE: Shapes = Shapes(32768);
    /// `SGR 51`, framed: a line across the top and bottom of the cell, and
    /// a side where the run of framed cells starts and where it ends, so a
    /// run is one box. `SHAPE_FRAMED` in the cell shader.
    pub const FRAMED: Shapes = Shapes(65536);
    /// `SGR 64`, "ideogram stress marking": a filled dot over each
    /// character the web marks ([`takes_stress_mark`]), in the text's
    /// colour, one across a wide character's two cells. `SHAPE_STRESS` in
    /// the cell shader.
    pub const STRESS: Shapes = Shapes(131072);
    /// `SGR 52`, encircled: one enclosure a run, as the frame is one box a
    /// run -- the frame's top and bottom lines between the ends and a half
    /// ellipse at each end ([`crate::boxes::Ring`]). `SHAPE_ENCIRCLED` in
    /// the cell shader.
    pub const ENCIRCLED: Shapes = Shapes(262144);
    /// The shapes drawn at a cell's edges and over it, whose neighbours
    /// some of them read: the overline, its double, the frame, an
    /// encircled run and the stress mark. A cell with none skips them all
    /// on one test.
    pub const EDGES: Shapes = Shapes(
        Shapes::OVERLINE.0
            | Shapes::DOUBLE_OVERLINE.0
            | Shapes::FRAMED.0
            | Shapes::ENCIRCLED.0
            | Shapes::STRESS.0,
    );
    /// Every shape the cell shader's decorations pass draws for a cell, the
    /// bits its slots test: the underline, the strike, the double
    /// underline, the curly, dotted and dashed underlines' segments, and
    /// [`Shapes::EDGES`]. A table with no slot carrying one leaves that pass
    /// undrawn but for the cursor's line.
    pub const DECORATIONS: Shapes = Shapes(
        Shapes::UNDERLINE.0
            | Shapes::STRIKE.0
            | Shapes::DOUBLE_UNDERLINE.0
            | Shapes::CURLY_UNDERLINE.0
            | Shapes::DOTTED_UNDERLINE.0
            | Shapes::DASHED_UNDERLINE.0
            | Shapes::EDGES.0,
    );
    /// Both blink shapes: a slot holds at most one, since `SGR 5` and `6`
    /// replace each other (`Attrs::BLINKS`).
    pub const BLINKS: Shapes = Shapes(Shapes::BLINK.0 | Shapes::RAPID_BLINK.0);

    /// Where the face style's two bits sit: the style whose face the cell's
    /// text is drawn from, already fallen back by [`crate::Stack::effective`],
    /// so the cell shader keys its glyph map on it without knowing the stack.
    /// `FACE_SHIFT` in the cell shader, pinned by
    /// `the_face_style_bits_match_the_shader`.
    pub const FACE_SHIFT: u32 = 10;

    pub fn with(self, s: Shapes) -> Shapes {
        Shapes(self.0 | s.0)
    }

    /// Where the font's five bits sit ([`crate::FontSlot`]): the
    /// alternative in the low four and Fraktur above, already resolved by
    /// [`crate::Stack::font_of`] to what the stack has faces for.
    /// `FONT_SHIFT` in the cell shader, pinned by
    /// `the_face_style_bits_match_the_shader`.
    pub const FONT_SHIFT: u32 = 19;

    pub fn with_face(self, style: crate::FaceStyle) -> Shapes {
        Shapes((self.0 & !(3 << Shapes::FACE_SHIFT)) | (style.code().0 << Shapes::FACE_SHIFT))
    }

    pub fn face(self) -> crate::FaceStyle {
        crate::FaceStyle::from_code(Code(self.0 >> Shapes::FACE_SHIFT))
    }

    pub fn with_font(self, font: crate::FontSlot) -> Shapes {
        Shapes((self.0 & !(31 << Shapes::FONT_SHIFT)) | (font.code().0 << Shapes::FONT_SHIFT))
    }

    pub fn font(self) -> crate::FontSlot {
        crate::FontSlot::from_code(Code(self.0 >> Shapes::FONT_SHIFT))
    }

    /// The style and the font together, what a glyph is looked up by.
    pub fn text_face(self) -> crate::TextFace {
        crate::TextFace {
            style: self.face(),
            font: self.font(),
        }
    }

    /// Whether every bit of `s` is set.
    pub fn has(self, s: Shapes) -> bool {
        self.0 & s.0 == s.0
    }

    /// Whether the slot blinks at either rate.
    pub fn blinks(self) -> bool {
        self.0 & Shapes::BLINKS.0 != 0
    }

    /// Whether any of `s`'s shapes is set.
    pub fn any(self, s: Shapes) -> bool {
        self.0 & s.0 != 0
    }
}

/// One rendition table slot resolved to what a cell draws with: the
/// foreground, the background with the default background's alpha applied,
/// the underline colour, which shapes to draw, the text-contrast curve
/// rows its glyph takes on either side ([`Curves`]), the foreground and
/// curves again for text over the page as the window's dim draws it, and
/// once more as asked, for a line ([`Line`]). Thirty-six bytes, `repr(C)`,
/// nine `u32`s.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Resolved {
    /// The text over the page drawn solid: a picture's, an opaque
    /// rendition's, an opaque window's, and both sides of a swap.
    pub fg: Argb,
    pub bg: Argb,
    pub underline: Argb,
    pub shapes: Shapes,
    pub curves: Curves,
    /// The text over the page drawn at the window's dim: `fg` lifted by
    /// [`crate::Floor::lift_on_glass`] where the dim makes it unreadable,
    /// else `fg` itself.
    pub glass: Argb,
    /// [`Resolved::curves`] with `glass`'s row in the own half.
    pub glass_curves: Curves,
    /// The text as the program asked for it, before either floor: what a
    /// box-drawing line draws ([`Line`]).
    pub line: Argb,
    /// [`Resolved::curves`] with `line`'s row in the own half.
    pub line_curves: Curves,
}

/// **Whether a cell is a box-drawing line**, which the contrast floor
/// leaves as asked: its character is in Unicode's Box Drawing block,
/// `U+2500..=U+257F`, every one of whose 128 names in `UnicodeData.txt`
/// begins `BOX DRAWINGS` (`[[source: unicode-data]]`), where DEC Special
/// Graphics stores its lines, corners and tees; its scan lines, `o p r s`,
/// are `U+23BA..=U+23BD`, outside it. herdr draws its dividers below 1.2:1
/// on the default page, and the floor lifted them into grey rules at
/// 4.5:1. WCAG 2.2 exempts text that is "pure decoration" from SC 1.4.3
/// and asks 3:1 of graphical objects needed to understand the content (SC
/// 1.4.11; `[[source: wcag22]]`); neither settles a divider, and leaving
/// lines out is baud's choice, not the standard's. **Block Elements,
/// `U+2580..=U+259F`, are not lines**: btop and cava draw graphs with
/// them, and they keep the floor. A line carrying a mark is a cluster,
/// drawn as other text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    No,
    Yes,
}

impl Line {
    /// The first and last character of the Box Drawing block. The cell
    /// shader's `is_line` spells the same range, pinned by
    /// `the_line_range_matches_the_shader`.
    pub const FIRST: u32 = 0x2500;
    pub const LAST: u32 = 0x257F;

    pub fn of(content: Content) -> Line {
        let v = content.bits();
        if (Line::FIRST..=Line::LAST).contains(&v) {
            Line::Yes
        } else {
            Line::No
        }
    }
}

/// The contrast curve rows a glyph of one rendition is drawn with
/// (`baud_domain::contrast`): the foreground's in the low half, the
/// background's in the high half, for a glyph drawn swapped. Chosen once,
/// when the slot is resolved, so a pixel only looks its row up; the cell
/// shader takes one half by the swaps' parity ([`Swaps::ink`]).
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Curves(pub u32);

impl Curves {
    pub fn of(own: CurveRow, swapped: CurveRow) -> Curves {
        Curves(u32::from(own.get()) | (u32::from(swapped.get()) << 16))
    }

    /// The row a glyph drawn on `ink`'s side takes.
    pub fn row(self, ink: Ink) -> CurveRow {
        let half = match ink {
            Ink::Own => self.0 & 0xffff,
            Ink::Swapped => self.0 >> 16,
        };
        CurveRow::at(u16::try_from(half).unwrap_or(0)).unwrap_or_default()
    }
}

/// `Resolved::BYTES` is what the cell shader's `struct Style` is laid out
/// against; a field added here and not there would drop the app off the cell
/// path, so the two are held equal at compile time.
const _: () = assert!(core::mem::size_of::<Resolved>() == Resolved::BYTES);

impl Resolved {
    pub const BYTES: usize = 36;

    /// Whether the cell's own text draws: its glyph, its underline and its
    /// strike. **Hidden text draws its background and nothing of its own**,
    /// which is not the same as drawing the glyph in the background's colour
    /// once backgrounds are translucent: the glyph lands on the background it
    /// was meant to vanish into, at another alpha, and its silhouette shows.
    /// xterm draws it in the background (`updatedXtermGC`, `fg_pix = bg_pix`
    /// for `BLINK` off and `INVISIBLE`), over a background that is opaque.
    ///
    /// A blinking cell is hidden in its own rate's off phase: `SGR 5` in the
    /// slow one, `SGR 6` in the rapid one ([`BlinkPhases::hidden`]); or
    /// faint, where the frame draws that off phase faint
    /// ([`BlinkPhases::faint`]). `SGR 8` hides it in every phase.
    pub fn text(self, blink: BlinkPhases) -> Text {
        let off = self.shapes.0 & blink.hidden().0 != 0;
        if off || self.shapes.has(Shapes::CONCEAL) {
            Text::Hidden
        } else if self.shapes.0 & blink.faint().0 != 0 {
            Text::Faint
        } else {
            Text::Drawn
        }
    }

    /// The cell as this frame draws it: the rendition, then the swaps the
    /// frame lays over it, then whether its text shows. **The one spelling of
    /// that order**, which the CPU and instance paths take a cell at a time,
    /// the overlay's records take for the cells they draw, and the cell
    /// shader's `colors()` and `hidden()` mirror -- the pixel oracle compares
    /// the two GPU arms to say so.
    ///
    /// * **A picture's or an opaque rendition's background is solid.** Every
    ///   background carries the window's dim; a picture drawn in text keeps
    ///   its backgrounds solid under it, or it splits into slivers and holes.
    ///   **Text over a page left dimmed is [`Resolved::glass`]**, and over a
    ///   solid one [`Resolved::fg`]; a swap takes `fg`, since it draws both
    ///   sides solid. **A box-drawing line is [`Resolved::line`]** on either
    ///   page; what a swap hands the background stays `fg`, floored, as a
    ///   selected letter's, and pinned on the CPU arm by
    ///   `a_selected_line_takes_the_floored_background_a_letter_does`.
    /// * **The selection swaps, then a block cursor swaps again**, and both
    ///   leave the two colours opaque: the foreground a swap hands back is
    ///   the old background, which carries the dim, and a glyph at 0.22 over
    ///   its own block is barely there. A block cursor fills with `OSC 12`'s
    ///   colour when the application set one.
    /// * **Hiding comes last**, after the swaps, as xterm's does: hidden
    ///   first, a block cursor would paint its block in the colour the text
    ///   vanished into and disappear with it. **A faint off phase takes the
    ///   same place**: the text as the swaps leave it, half way to the page
    ///   they leave it on, as `SGR 2` mixes (`Argb::faint_on`), so under a
    ///   block cursor the glyph dims toward the block.
    ///
    /// **Inlined always**: CPU instructions a 200x50 glass frame of herdr,
    /// before the glass text 20.26M as compiled and 19.91M inlined, with it
    /// 20.49M as compiled and 19.97M inlined.
    #[inline(always)]
    pub fn drawn(self, s: Swaps) -> Drawn {
        let opaque = Chan::from_u8(0xFF);
        let solid = self.shapes.has(Shapes::OPAQUE_BG) || s.pictured == Pictured::Yes;
        let line = s.line == Line::Yes;
        let (mut fg, mut bg) = if solid {
            (self.fg, self.bg.with_alpha(opaque))
        } else {
            (self.glass, self.bg)
        };
        // What a bar or underline cursor falls back to over this cell: the
        // text without the line rule, so a line under a bar cursor keeps the
        // floored colour the cursor always had (the line rule covers lines,
        // not the cursor).
        let plain = fg;
        if line {
            fg = self.line;
        }
        // The text a swap hands to the background: the solid one.
        let mut ink = self.fg;
        if s.selected == Selected::Yes {
            (fg, bg) = s.hilite.paint(bg, ink);
            ink = fg;
        }
        let cursor_fg = if line && s.selected == Selected::No {
            plain
        } else {
            fg
        };
        if let BlockCursor::Yes(paint) = s.block {
            let off = (fg, bg);
            // The cell as the block finds it: its text and its page.
            let cell = (ink.with_alpha(opaque), bg.with_alpha(opaque));
            (fg, bg) = (cell.1, cell.0);
            // A cursor colour -- a program's `OSC 12`, the config's, a theme
            // file's -- fills the block, and a cursor text colour is the
            // glyph's, each a colour or the cell's own.
            if let Some(c) = paint.fill {
                bg = paint_of(c, cell);
            }
            if let Some(t) = paint.text {
                fg = paint_of(t.map(|(c, _)| c), cell);
            }
            // A block part way through a fade: each colour `cursor` of the
            // way from the cell without it to the cell with it, which over
            // one glyph coverage is the two pixels mixed.
            if s.cursor != opaque {
                (fg, bg) = (fg.mix(off.0, s.cursor), bg.mix(off.1, s.cursor));
            }
        }
        // The curve rows the glyph takes from, while it is drawn unswapped:
        // a line's, else the glass text's over a page left dimmed; else the
        // solid text's. The cell shader's `glyph_curve` mirrors it.
        let side = s.ink();
        let own = s.selected == Selected::No && side == Ink::Own;
        let curves = if own && line {
            self.line_curves
        } else if own && !solid {
            self.glass_curves
        } else {
            self.curves
        };
        // A faint off phase, after every swap: the glyph half way to the
        // page it is left on. The cell shader's `colors` ends the same way.
        let text = self.text(s.blink);
        if text == Text::Faint {
            fg = fg.faint_on(bg);
        }
        Drawn {
            fg,
            bg,
            cursor_fg,
            text,
            curve: self.hilite_curve(s).unwrap_or_else(|| curves.row(side)),
        }
    }

    /// The curve row of a selected cell's glyph under a highlight colour,
    /// `None` under reverse video: the colour the glyph is drawn in is the
    /// selection's text, or, swapped again by a block cursor, its page, and
    /// each is a highlight colour's row where one is set and the cell's own
    /// text's otherwise. The cell shader's `glyph_curve` spells the same.
    ///
    /// **Inlined**: called once a cell, and out of line it cost the frame
    /// bench about 1% of its instructions (`frame vim 5 full`, 4,177 M and
    /// 4,183 M in two runs against 4,139 M inlined, before the cold path
    /// below moved out of it).
    #[inline(always)]
    fn hilite_curve(self, s: Swaps) -> Option<CurveRow> {
        if let BlockCursor::Yes(BlockPaint { text: Some(t), .. }) = s.block {
            if s.cursor.get() >= 128 {
                if let Some(row) = block_text_curve(self.curves, t, s.selected, s.hilite) {
                    return Some(row);
                }
            }
        }
        if s.selected == Selected::No || s.hilite == Hilite::REVERSE {
            return None;
        }
        Some(row_of(
            self.curves,
            match s.ink() {
                Ink::Swapped => s.hilite.text(),
                Ink::Own => s.hilite.page(),
            },
        ))
    }
}

/// The curve row of a block's glyph in a text colour of its own: a colour's
/// own row, or the cell's solid text as the selection leaves it; `None` for
/// the cell's page, the swap's, the default. Out of line, and handed the
/// solid curves alone, so a cell's loop keeps its rendition in registers:
/// one cell a frame takes it, and only with a theme's cursor text.
#[cold]
#[inline(never)]
fn block_text_curve(
    curves: Curves,
    t: CellPaint<(Argb, CurveRow)>,
    selected: Selected,
    hilite: Hilite,
) -> Option<CurveRow> {
    match t {
        CellPaint::Colour((_, row)) => Some(row),
        CellPaint::Text if selected == Selected::No => Some(curves.row(Ink::Own)),
        CellPaint::Text if hilite == Hilite::REVERSE => Some(curves.row(Ink::Swapped)),
        CellPaint::Text => Some(row_of(curves, hilite.text())),
        CellPaint::Page => None,
    }
}

/// The curve row a selected glyph in `p` takes: a colour's own, or the
/// cell's solid text's or page's.
fn row_of(curves: Curves, p: CellPaint<(Argb, CurveRow)>) -> CurveRow {
    match p {
        CellPaint::Colour((_, row)) => row,
        CellPaint::Text => curves.row(Ink::Own),
        CellPaint::Page => curves.row(Ink::Swapped),
    }
}

/// **Whether a cell draws the default background**, for a kitty image
/// below `INT32_MIN/2`: "Negative z-index values below INT32_MIN/2
/// (-1,073,741,824) will be drawn under cells with non-default background
/// colors" (`[[source: kitty-graphics]]`, "Controlling displayed image
/// layout"). [`Ground::Set`] cells are drawn over such an image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ground {
    Default,
    Set,
}

impl Resolved {
    /// The cell's [`Ground`] this frame: its rendition's
    /// [`Shapes::DEFAULT_BG`], and not selected and not under a block
    /// cursor, each of which draws another colour as the background, as
    /// kitty's `compute_background` makes them "special"
    /// (`[[source: kitty-background-slang]]`). The cell shader's
    /// `ground_set` mirrors it.
    pub fn ground(self, selected: Selected, block: BlockCursor) -> Ground {
        match (self.shapes.has(Shapes::DEFAULT_BG), selected, block) {
            (true, Selected::No, BlockCursor::No) => Ground::Default,
            _ => Ground::Set,
        }
    }
}

/// Whether a cell's own text -- its glyph, underline and strike -- draws,
/// and how.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Text {
    #[default]
    Drawn,
    Hidden,
    /// Drawn faint: a blinking cell's off phase under dimmed flashing
    /// lights ([`crate::Blink::Faint`]). Its colours are already faint in
    /// [`Drawn`]; a rule in a colour of its own takes [`Drawn::rule`].
    Faint,
}

impl Text {
    /// Whether the cell's own text draws at all: a faint one does.
    pub fn draws(self) -> bool {
        match self {
            Text::Drawn | Text::Faint => true,
            Text::Hidden => false,
        }
    }
}

/// Whether the selection covers a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selected {
    No,
    Yes,
}

/// **How the cursor is drawn this frame**: in its own shape, or as a
/// hollow box round its cell whatever its shape -- the `[cursor]
/// unfocused_hollow` setting, while another window has the keyboard. A
/// hollow cursor swaps no colours: the cell keeps its text and page, and
/// the box is drawn over them in the bar's colour, as kitty, ghostty,
/// foot, alacritty and WezTerm draw an unfocused cursor (note AZ).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorLook {
    #[default]
    Shaped,
    Hollow,
}

/// Whether a block cursor sits on a cell, and what it is drawn in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockCursor {
    No,
    Yes(BlockPaint),
}

/// **What a block cursor is drawn in**: the fill -- a program's `OSC 12`,
/// else the configured colour, else a theme file's, which may name the
/// cell's own -- and the text under it, each `None` for the cell's colours
/// swapped, the default. The text carries the contrast curve row a glyph
/// in its colour takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BlockPaint {
    pub fill: Option<CellPaint<Argb>>,
    pub text: Option<CellPaint<(Argb, CurveRow)>>,
}

impl BlockPaint {
    /// The cell's two colours swapped: no colour of the cursor's own.
    pub const SWAP: BlockPaint = BlockPaint {
        fill: None,
        text: None,
    };

    /// A fill and a text colour, each made opaque, the text with its curve
    /// row.
    pub fn of(fill: Option<CellPaint<Argb>>, text: Option<CellPaint<Argb>>) -> BlockPaint {
        BlockPaint {
            fill: fill.map(|p| p.map(|c| c.with_alpha(Chan::from_u8(0xFF)))),
            text: text.map(|p| p.map(with_curve)),
        }
    }
}

/// A colour made opaque, with the curve row it takes as text.
fn with_curve(c: Argb) -> (Argb, CurveRow) {
    let c = c.with_alpha(Chan::from_u8(0xFF));
    (c, CurveRow::of(c.r().get(), c.g().get(), c.b().get()))
}

/// A paint on a cell whose text and page are `cell`: a colour, or one of
/// the two.
fn paint_of(p: CellPaint<Argb>, cell: (Argb, Argb)) -> Argb {
    match p {
        CellPaint::Colour(c) => c,
        CellPaint::Text => cell.0,
        CellPaint::Page => cell.1,
    }
}

/// The colours a selection is drawn in: `OSC 17`'s page and `OSC 19`'s text,
/// each a colour, opaque and with the contrast curve row a glyph in it
/// takes, or the cell's own text or page (a theme file's `CellForeground`
/// and `CellBackground`), or none of either. [`Hilite::REVERSE`], neither,
/// is reverse video, the default.
///
/// [`Resolved::drawn`] takes the page from `bg` where it is set and the
/// cell's text otherwise ([`Hilite::page`]), and the text from `fg` where
/// it is set, else the cell's own text on a page that is not the cell's
/// text, else the cell's page ([`Hilite::text`]): so a colour in `bg` alone
/// draws each cell's text on the highlight, and `fg` alone the highlight
/// text on the reversed page. The cell shader's `colors()` spells the same.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hilite {
    pub bg: Option<CellPaint<(Argb, CurveRow)>>,
    pub fg: Option<CellPaint<(Argb, CurveRow)>>,
}

impl Hilite {
    pub const REVERSE: Hilite = Hilite { bg: None, fg: None };

    /// The pair from two paints, each colour made opaque, with the curve
    /// row its colour takes as text.
    pub fn of(bg: Option<CellPaint<Argb>>, fg: Option<CellPaint<Argb>>) -> Hilite {
        Hilite {
            bg: bg.map(|p| p.map(with_curve)),
            fg: fg.map(|p| p.map(with_curve)),
        }
    }

    /// The page as a paint: `bg`, else the cell's text.
    pub fn page(self) -> CellPaint<(Argb, CurveRow)> {
        self.bg.unwrap_or(CellPaint::Text)
    }

    /// The text as a paint: `fg`, else the cell's page where the page is
    /// the cell's text -- reverse video -- and the cell's text where it is
    /// not.
    pub fn text(self) -> CellPaint<(Argb, CurveRow)> {
        self.fg.unwrap_or(match self.page() {
            CellPaint::Text => CellPaint::Page,
            CellPaint::Colour(_) | CellPaint::Page => CellPaint::Text,
        })
    }

    /// The page and text a selected cell is drawn in, from the cell's page
    /// and text as the selection finds them.
    pub fn paint(self, page: Argb, text: Argb) -> (Argb, Argb) {
        let opaque = Chan::from_u8(0xFF);
        let cell = (text.with_alpha(opaque), page.with_alpha(opaque));
        (
            paint_of(self.text().map(|(c, _)| c), cell),
            paint_of(self.page().map(|(c, _)| c), cell),
        )
    }
}

/// What a frame lays over a cell's resolved rendition.
#[derive(Clone, Copy, Debug)]
pub struct Swaps {
    pub pictured: Pictured,
    /// Whether the cell is a box-drawing line, by its content: [`Line::of`].
    pub line: Line,
    pub selected: Selected,
    /// The colours [`Swaps::selected`] draws in: reverse video, or `OSC 17`
    /// and `19`'s.
    pub hilite: Hilite,
    pub block: BlockCursor,
    pub blink: BlinkPhases,
    /// The block cursor's opacity, 255 drawn whole: below it during a fade
    /// ([`crate::CursorPhase`]), and ignored where no block sits.
    pub cursor: Chan,
}

/// A cell as a frame draws it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Drawn {
    pub fg: Argb,
    pub bg: Argb,
    /// The colour a bar or underline cursor takes when the application set
    /// none: [`Drawn::fg`] as it would be with [`Line::No`].
    pub cursor_fg: Argb,
    pub text: Text,
    /// The contrast curve row the glyph takes: the slot's row for the side
    /// the swaps leave the glyph's colour on.
    pub curve: CurveRow,
}

impl Drawn {
    /// An underline in a colour of its own (`SGR 58`) as this frame draws
    /// it: faint with the text, half way to the page.
    pub fn rule(self, colour: Argb) -> Argb {
        match self.text {
            Text::Faint => colour.faint_on(self.bg),
            Text::Drawn | Text::Hidden => colour,
        }
    }
}
