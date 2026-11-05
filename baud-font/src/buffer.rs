//! The glyph buffer a shaping pass works on: a caller-provided slice, no
//! allocation, glyphs that remember which cell they came from.
//!
//! **A glyph carries its cells.** Input glyphs are one per codepoint, each
//! tagged with the cell (the input cluster) it belongs to; a ligature that
//! consumes several keeps the first cell as its `cluster` and the last as
//! `last`, so the cell mapper knows the span it covers. A mark keeps its
//! base's cluster, which is how it inherits the base's cell. That is the
//! whole of the bookkeeping Windows Terminal's rule (D8) needs.
//!
//! **A pass shrinks the buffer, and grows it only by a multiple
//! substitution.** Every other lookup type this crate implements
//! substitutes one-for-one or many-for-one, and `morx` type 2 deletes
//! components. `GSUB` type 2 puts glyphs in after the one it replaces, in
//! the slice the caller handed over: a sequence that does not fit is not
//! substituted, so the buffer never reaches past the caller's slice.

use crate::bytes::{Count, Index};
use crate::ignorable::DEFAULT_IGNORABLE;
use crate::layout::{Gdef, GlyphClass};
use crate::shape::CombiningClass;
use crate::syllabic::Syllabic;
use baud_domain::{Codepoint, GlyphId, JoiningForm};

/// A cell index within the run: the input cluster a glyph came from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Cluster(pub u16);

/// Whether a glyph came from a default-ignorable codepoint (a joiner, a
/// variation selector, a tag): matched when a pattern names it, skipped
/// otherwise, and drawn never.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ignorable {
    No,
    Yes,
}

impl Ignorable {
    /// Whether `c` is `Default_Ignorable_Code_Point`, by binary search over
    /// the compiled ranges.
    pub fn of(c: Codepoint) -> Ignorable {
        let v = c.value();
        let i = DEFAULT_IGNORABLE.partition_point(|&(_, end)| end < v);
        match DEFAULT_IGNORABLE.get(i) {
            Some(&(start, _)) if start <= v => Ignorable::Yes,
            _ => Ignorable::No,
        }
    }
}

/// Whether a glyph is an attachment -- came after its cluster's base --
/// and so draws in the base's cell rather than advancing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attached {
    No,
    Yes,
}

/// Whether a positioning lookup placed this glyph, or the fallback still
/// owes it a position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Positioned {
    No,
    Yes,
}

/// Whether a glyph is a mark for advance zeroing: its `GDEF` class when
/// the face has one, else Unicode's word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    No,
    Yes,
}

/// One glyph in the buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Glyph {
    pub id: GlyphId,
    /// The first cell this glyph covers.
    pub cluster: Cluster,
    /// The last cell this glyph covers; equal to `cluster` unless it is a
    /// ligature.
    pub last: Cluster,
    pub ignorable: Ignorable,
    /// The glyph's `GDEF` class, read once when the glyph is written so
    /// a lookup's filter does not search the class table for every
    /// position it tests; `Other` on a face with no classes.
    pub class: GlyphClass,
    pub attached: Attached,
    pub positioned: Positioned,
    pub mark: Mark,
    /// The canonical combining class of the codepoint this glyph came
    /// from, which the fallback placer reads.
    pub ccc: CombiningClass,
    /// Positioning offsets from the glyph's cell origin, in font units:
    /// the pen position within the cluster, plus whatever a lookup or
    /// the fallback moved the glyph by.
    pub dx: i16,
    pub dy: i16,
    /// What a syllabic shaper keeps of the glyph ([`Syllabic`]): written
    /// from its character when it is pushed, and read by
    /// [`crate::indic`]; unread in any other run.
    pub syllabic: Syllabic,
}

/// **The joining form of each cell a pass shapes**, by the cell's index in
/// the run: kept beside the buffer rather than in each glyph, as a form is
/// a cell's and not a glyph's. A cell's form is its base glyph's: the
/// glyph of the cell that is not attached. (A glyph was sixteen bytes when
/// this was decided; its [`Syllabic`] properties, which move with it
/// through every reordering and substitution, made it twenty-two.)
#[derive(Clone, Copy)]
pub struct Forms {
    by_cell: [Option<JoiningForm>; Forms::CELLS],
}

impl Forms {
    /// The most cells a run gives forms to; a cell past it takes none. A
    /// renderer shapes a joined cell, or a lam-alef pair, with at most 16
    /// cells of its word either side, 34 cells for a lam-alef pair, and
    /// any other run of its is 32 cells at most (`baud-render`'s
    /// `RUN_CELLS`).
    pub const CELLS: usize = 64;

    pub const NONE: Forms = Forms {
        by_cell: [None; Forms::CELLS],
    };

    pub fn set(&mut self, cell: Cluster, form: JoiningForm) {
        if let Some(slot) = self.by_cell.get_mut(usize::from(cell.0)) {
            *slot = Some(form);
        }
    }

    /// The form glyph `g` is drawn in: its cell's, for the cell's base.
    pub fn of(&self, g: &Glyph) -> Option<JoiningForm> {
        match g.attached {
            Attached::No => self
                .by_cell
                .get(usize::from(g.cluster.0))
                .copied()
                .flatten(),
            Attached::Yes => None,
        }
    }
}

impl Glyph {
    pub const fn new(id: GlyphId, cluster: Cluster, ignorable: Ignorable) -> Glyph {
        Glyph {
            id,
            cluster,
            last: cluster,
            ignorable,
            class: GlyphClass::Other,
            attached: Attached::No,
            positioned: Positioned::No,
            mark: Mark::No,
            ccc: CombiningClass::STARTER,
            dx: 0,
            dy: 0,
            syllabic: Syllabic::NONE,
        }
    }

    pub const fn attached(mut self) -> Glyph {
        self.attached = Attached::Yes;
        self
    }
}

/// **Where each cluster's pen starts.** A grid gives every cluster a cell
/// of its own, so the shaper starts each cluster's pen again at its cell's
/// origin and an advance moves only the glyphs after it in its cluster.
/// Text set as a run starts each cluster where the one before it ended,
/// and an advance moves every glyph after it, as `[[source:
/// ms-opentype-gpos-2026-10-23]]`'s "if the first glyph has an advance
/// width of 100, then the second glyph begins at 100,0" reads for a run
/// and as hb-shape reports positions: what a word drawn whole at its
/// natural advances needs. Left to right only; a right-to-left run keeps
/// its cells' pens.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pens {
    /// Each cluster from its own cell's origin.
    #[default]
    Cell,
    /// Each cluster from the end of the one before, every offset from the
    /// run's start.
    Run,
}

/// **Which cell a glyph keeps where a shaper reorders or joins the glyphs
/// of several cells**: HarfBuzz's cluster levels
/// (`[[source: harfbuzz-14-5-1-buffer-h]]`), the two a terminal reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ClusterLevel {
    /// HarfBuzz's default, `MONOTONE_GRAPHEMES`: what the Indic engine moves
    /// across cells merges them, so every glyph of a reordered syllable
    /// names its first cell and the cells stay in order -- what a grid
    /// draws a syllable whole from.
    #[default]
    Monotone,
    /// `CHARACTERS`: each glyph keeps the cell it came from, "not merged
    /// into monotone order", and a ligature the first matched glyph's, so
    /// a renderer knows which cell each glyph of a syllable is: what a
    /// syllable whose cells are in two renditions is drawn from. On every
    /// line of the Devanagari fixtures the glyphs and their pens are the
    /// default level's and only the cells differ
    /// (`the_characters_level_keeps_cells_as_harfbuzz_does`); a mark on a
    /// ligature takes its component by cell (gpos.rs), so a font that
    /// attaches one there can differ.
    Characters,
}

/// The buffer: `len` live glyphs at the front of a caller's slice.
pub struct Buffer<'b> {
    glyphs: &'b mut [Glyph],
    len: Count,
    pens: Pens,
    clusters: ClusterLevel,
}

impl<'b> Buffer<'b> {
    /// An empty buffer over `storage`.
    pub fn new(storage: &'b mut [Glyph]) -> Buffer<'b> {
        Buffer {
            glyphs: storage,
            len: Count::ZERO,
            pens: Pens::Cell,
            clusters: ClusterLevel::Monotone,
        }
    }

    /// Where each cluster's pen starts ([`Pens`]).
    pub fn pens(&self) -> Pens {
        self.pens
    }

    /// Lays the clusters as `pens` says: set before the buffer is
    /// positioned.
    pub fn set_pens(&mut self, pens: Pens) {
        self.pens = pens;
    }

    /// Which cell a reordered or joined glyph keeps ([`ClusterLevel`]).
    pub fn cluster_level(&self) -> ClusterLevel {
        self.clusters
    }

    /// Keeps the cells as `level` says: set before the buffer is shaped.
    pub fn set_cluster_level(&mut self, level: ClusterLevel) {
        self.clusters = level;
    }

    /// Appends a glyph; `None` when the storage is full.
    pub fn push(&mut self, g: Glyph) -> Option<()> {
        *self.glyphs.get_mut(self.len.slot())? = g;
        self.len = Count::of(self.len.value().checked_add(1)?);
        Some(())
    }

    /// Every glyph's class read again from `gdef`, after a pass that
    /// wrote glyph ids without one.
    pub fn retag(&mut self, gdef: &Gdef<'_>) {
        let mut at = Index::ZERO;
        while let Some(g) = self.get_mut(at) {
            g.class = gdef.class(g.id);
            at = at.next();
        }
    }

    pub fn len(&self) -> Count {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len.is_zero()
    }

    pub fn get(&self, i: Index) -> Option<Glyph> {
        self.as_slice().get(i.slot()).copied()
    }

    pub fn get_mut(&mut self, i: Index) -> Option<&mut Glyph> {
        let n = self.len.slot();
        self.glyphs.get_mut(..n)?.get_mut(i.slot())
    }

    /// The live glyphs.
    pub fn as_slice(&self) -> &[Glyph] {
        self.glyphs.get(..self.len.slot()).unwrap_or(&[])
    }

    pub fn as_mut_slice(&mut self) -> &mut [Glyph] {
        let n = self.len.slot();
        self.glyphs.get_mut(..n).unwrap_or(&mut [])
    }

    /// How many more glyphs the storage holds.
    pub fn room(&self) -> Count {
        Count::of(u32::try_from(self.glyphs.len().saturating_sub(self.len.slot())).unwrap_or(0))
    }

    /// Puts `g` in after glyph `i`, moving the glyphs after it up one;
    /// `None`, and nothing moved, when the storage is full or `i` is not a
    /// live glyph.
    pub fn insert(&mut self, i: Index, g: Glyph) -> Option<()> {
        if !self.len.holds(i) || self.room().is_zero() {
            return None;
        }
        let (at, n) = (i.slot() + 1, self.len.slot());
        self.glyphs.copy_within(at..n, at + 1);
        *self.glyphs.get_mut(at)? = g;
        self.len = Count::of(self.len.value().checked_add(1)?);
        Some(())
    }

    /// Puts `g` in at position `i`, before the glyph there, moving it and
    /// the glyphs after it up one; at the end when `i` is the length.
    /// `None`, and nothing moved, when the storage is full or `i` is past
    /// the end.
    pub fn put(&mut self, i: Index, g: Glyph) -> Option<()> {
        let (at, n) = (i.slot(), self.len.slot());
        if at > n || self.room().is_zero() {
            return None;
        }
        self.glyphs.copy_within(at..n, at + 1);
        *self.glyphs.get_mut(at)? = g;
        self.len = Count::of(self.len.value().checked_add(1)?);
        Some(())
    }

    /// Removes glyph `i`, closing the gap.
    pub fn remove(&mut self, i: Index) -> Option<Glyph> {
        if !self.len.holds(i) {
            return None;
        }
        let (at, n) = (i.slot(), self.len.slot());
        let g = self.glyphs[at];
        self.glyphs.copy_within(at + 1..n, at);
        self.len = Count::of(self.len.value().saturating_sub(1));
        Some(g)
    }

    /// **Deletes glyph `i`**, as a lookup that substitutes it with nothing
    /// does. At [`ClusterLevel::Characters`] a cell no glyph beside it
    /// keeps passes back to the glyphs before it that share one cell, where
    /// it is the lower of the two, as HarfBuzz's `delete_glyph()` merges a
    /// cluster backward at every level
    /// (`[[source: harfbuzz-14-5-1-buffer-cc]]`); at the monotone level
    /// every cell is left as it was.
    pub fn delete(&mut self, i: Index) -> Option<Glyph> {
        let at = i.slot();
        let g = self.get(i)?;
        if self.clusters == ClusterLevel::Characters {
            let n = self.len.slot();
            let next = self.glyphs.get(at + 1).filter(|_| at + 1 < n);
            let before = at.checked_sub(1).and_then(|b| self.glyphs.get(b));
            let survives = next.is_some_and(|h| h.cluster == g.cluster)
                || before.is_some_and(|h| h.cluster == g.cluster);
            if let Some(&b) = before.filter(|b| !survives && g.cluster < b.cluster) {
                for h in self.glyphs[..at].iter_mut().rev() {
                    if h.cluster != b.cluster {
                        break;
                    }
                    h.cluster = g.cluster;
                }
            }
        }
        self.remove(i)
    }
}
