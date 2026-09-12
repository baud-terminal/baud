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
//! **A pass can only shrink the buffer.** The lookup types this crate
//! implements substitute one-for-one or many-for-one, and `morx` type 2
//! deletes components; nothing inserts. So the slice the caller hands over
//! for the input is enough for every output, and there is no growth path
//! to get wrong.

use crate::bytes::{Count, Index};
use crate::ignorable::DEFAULT_IGNORABLE;
use crate::shape::CombiningClass;
use baud_domain::{Codepoint, GlyphId};

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
    /// the generated ranges.
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
}

impl Glyph {
    pub const fn new(id: GlyphId, cluster: Cluster, ignorable: Ignorable) -> Glyph {
        Glyph {
            id,
            cluster,
            last: cluster,
            ignorable,
            attached: Attached::No,
            positioned: Positioned::No,
            mark: Mark::No,
            ccc: CombiningClass::STARTER,
            dx: 0,
            dy: 0,
        }
    }

    pub const fn attached(mut self) -> Glyph {
        self.attached = Attached::Yes;
        self
    }
}

/// The buffer: `len` live glyphs at the front of a caller's slice.
pub struct Buffer<'b> {
    glyphs: &'b mut [Glyph],
    len: Count,
}

impl<'b> Buffer<'b> {
    /// An empty buffer over `storage`.
    pub fn new(storage: &'b mut [Glyph]) -> Buffer<'b> {
        Buffer {
            glyphs: storage,
            len: Count::ZERO,
        }
    }

    /// Appends a glyph; `None` when the storage is full.
    pub fn push(&mut self, g: Glyph) -> Option<()> {
        *self.glyphs.get_mut(self.len.slot())? = g;
        self.len = Count::of(self.len.value().checked_add(1)?);
        Some(())
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
}
