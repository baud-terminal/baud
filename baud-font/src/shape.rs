//! The shaper: from a run of clusters to glyphs that know their cells.
//!
//! **Input is what the core already resolved.** `baud-core` hands a
//! renderer one cluster per cell -- a base and its attached codepoints --
//! and the cell's width; the shaper never re-segments text. That is
//! swash's model (D8), and it is why the cluster index *is* the cell
//! index all the way through.
//!
//! **The pipeline is HarfBuzz's `hb-ot-shape`, cut to the grid.**
//! Normalise: a base and its marks compose canonically when the face has
//! the composed glyph, and a precomposed character the face lacks
//! decomposes (`[[source: unicode-data]]`); a variation selector resolves
//! through `cmap` format 14. Map through `cmap`. Substitute: `GSUB` with
//! the grid feature set, or `morx` where a face has that instead.
//! Position: `GPOS` marks, with a fallback. Hide the default-ignorables
//! that survived: they become the space glyph, as HarfBuzz leaves them, and
//! a renderer draws nothing for a glyph flagged ignorable.
//!
//! **Output is one glyph per drawn thing, each with the first and last cell
//! it covers.** A ligature over three cells draws at the first and the
//! other two draw nothing; a mark draws in its base's cell with the offset
//! positioning gave it; the mixed-tone family that shapes to two glyphs is
//! two glyphs in one cell pair. Windows Terminal's rule -- force a
//! cluster's advance to its cell span -- is the renderer's to apply, since
//! only it knows the cell size.

use crate::buffer::{Buffer, Cluster, Glyph, Ignorable};
use crate::bytes::Count;
use crate::cmap::Variation;
use crate::compose::{COMPOSE, DECOMPOSE};
use crate::face::Face;
use crate::layout::Features;
use baud_domain::{Codepoint, GlyphId};

/// Hangul syllables compose by arithmetic
/// (`[[source: unicode-ch3]]` 3.12).
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// The canonical composite of `a` followed by `b`, if there is one.
pub fn compose(a: Codepoint, b: Codepoint) -> Option<Codepoint> {
    let (a, b) = (a.value(), b.value());
    // Hangul: L + V, then LV + T.
    if (L_BASE..L_BASE + L_COUNT).contains(&a) && (V_BASE..V_BASE + V_COUNT).contains(&b) {
        return Codepoint::new(S_BASE + ((a - L_BASE) * V_COUNT + (b - V_BASE)) * T_COUNT);
    }
    if (S_BASE..S_BASE + S_COUNT).contains(&a)
        && (a - S_BASE).is_multiple_of(T_COUNT)
        && (T_BASE + 1..T_BASE + T_COUNT).contains(&b)
    {
        return Codepoint::new(a + (b - T_BASE));
    }
    let i = COMPOSE.partition_point(|&(x, y, _)| (x, y) < (a, b));
    match COMPOSE.get(i) {
        Some(&(x, y, c)) if x == a && y == b => Codepoint::new(c),
        _ => None,
    }
}

/// One level of canonical decomposition of `c`: the pair, or the singleton
/// with `None` second, or `None` for a character that does not decompose.
pub fn decompose(c: Codepoint) -> Option<(Codepoint, Option<Codepoint>)> {
    let v = c.value();
    if (S_BASE..S_BASE + S_COUNT).contains(&v) {
        let s = v - S_BASE;
        let t = s % T_COUNT;
        return if t == 0 {
            Some((
                Codepoint::new(L_BASE + s / N_COUNT)?,
                Codepoint::new(V_BASE + (s % N_COUNT) / T_COUNT),
            ))
        } else {
            Some((Codepoint::new(S_BASE + s - t)?, Codepoint::new(T_BASE + t)))
        };
    }
    let i = DECOMPOSE.partition_point(|&(x, _, _)| x < v);
    match DECOMPOSE.get(i) {
        Some(&(x, a, b)) if x == v => Some((
            Codepoint::new(a)?,
            if b == 0 { None } else { Codepoint::new(b) },
        )),
        _ => None,
    }
}

/// How many codepoints one cluster may hand the shaper: a base and the
/// attachments the core stores, with room for a decomposition.
const CLUSTER_MAX: usize = 16;

/// The variation-selector range `cmap` format 14 answers for.
fn is_variation_selector(c: Codepoint) -> bool {
    matches!(c.value(), 0xFE00..=0xFE0F | 0xE0100..=0xE01EF)
}

/// What shaping a run produced: the buffer's glyphs, and counts a caller
/// can read.
#[derive(Clone, Copy, Debug)]
pub struct Shaped {
    /// Lookups a feature named that the lookup set could not hold.
    pub declined: Count,
    /// Codepoints the face had no glyph for, drawn as `.notdef`.
    pub missing: Count,
}

/// A shaping pass over one face: push clusters, then finish.
pub struct Shaper<'f, 'a, 'b> {
    face: &'f Face<'a>,
    buf: Buffer<'b>,
    missing: Count,
}

impl<'f, 'a, 'b> Shaper<'f, 'a, 'b> {
    pub fn new(face: &'f Face<'a>, storage: &'b mut [Glyph]) -> Self {
        Shaper {
            face,
            buf: Buffer::new(storage),
            missing: Count::ZERO,
        }
    }

    fn glyph_of(&mut self, c: Codepoint) -> GlyphId {
        match self.face.glyph(c) {
            Some(g) => g,
            None => {
                self.missing = Count::of(self.missing.value().saturating_add(1));
                GlyphId::NOTDEF
            }
        }
    }

    /// Adds one cluster -- a base followed by its attachments -- as cell
    /// `cell`. `None` when the storage is full, leaving the buffer as it
    /// was before the call.
    pub fn push_cluster(&mut self, cps: &[Codepoint], cell: Cluster) -> Option<()> {
        // Normalise into a small local sequence: compose what the face can
        // draw composed, decompose what it cannot draw at all.
        let mut seq = [Codepoint::from_char(' '); CLUSTER_MAX];
        let mut n = 0usize;
        for &c in cps.iter().take(CLUSTER_MAX) {
            if self.face.glyph(c).is_none() {
                if let Some((a, b)) = decompose(c) {
                    // One level, then again on the first part, which is how a
                    // doubly precomposed character (U+1E09) comes apart.
                    let (a, a2) = match decompose(a) {
                        Some((a0, Some(a1))) if self.face.glyph(a).is_none() => (a0, Some(a1)),
                        _ => (a, None),
                    };
                    for part in [Some(a), a2, b].into_iter().flatten() {
                        if n < CLUSTER_MAX {
                            seq[n] = part;
                            n += 1;
                        }
                    }
                    continue;
                }
            }
            if n < CLUSTER_MAX {
                seq[n] = c;
                n += 1;
            }
        }
        // Compose greedily onto the starter while the face has the result.
        let mut out = [Codepoint::from_char(' '); CLUSTER_MAX];
        let mut m = 0usize;
        for &c in &seq[..n] {
            if m > 0 && Ignorable::of(c) == Ignorable::No {
                if let Some(composed) = compose(out[m - 1], c) {
                    if self.face.glyph(composed).is_some() {
                        out[m - 1] = composed;
                        continue;
                    }
                }
            }
            out[m] = c;
            m += 1;
        }
        // Map through cmap, folding a variation selector into the glyph
        // before it where format 14 says to.
        let start = self.buf.len();
        let mut i = 0usize;
        while i < m {
            let c = out[i];
            let ignorable = Ignorable::of(c);
            let mut id = self.glyph_of(c);
            if i + 1 < m && is_variation_selector(out[i + 1]) {
                if let Variation::Glyph(g) = self.face.variation(c, out[i + 1]) {
                    id = g;
                }
            }
            if self.buf.push(Glyph::new(id, cell, ignorable)).is_none() {
                while self.buf.len().value() > start.value() {
                    self.buf.remove(self.buf.len().as_index().prev()?);
                }
                return None;
            }
            i += 1;
        }
        Some(())
    }

    /// Substitutes and hides, and hands the buffer back.
    pub fn finish(mut self, features: Features) -> (Buffer<'b>, Shaped) {
        let declined = match self.face.gsub() {
            Some(gsub) => gsub.apply(&mut self.buf, features),
            None => Count::ZERO,
        };
        // Default-ignorables that no lookup consumed become the space glyph,
        // as HarfBuzz leaves them; a renderer draws nothing for them either
        // way, and matching the oracle keeps the fixtures exact.
        if let Some(space) = self.face.glyph(Codepoint::from_char(' ')) {
            let mut at = crate::bytes::Index::ZERO;
            while let Some(g) = self.buf.get_mut(at) {
                if g.ignorable == Ignorable::Yes {
                    g.id = space;
                }
                at = at.next();
            }
        }
        (
            self.buf,
            Shaped {
                declined,
                missing: self.missing,
            },
        )
    }
}
