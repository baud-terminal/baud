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

use crate::buffer::{Attached, Buffer, Cluster, Glyph, Ignorable, Mark, Positioned};
use crate::bytes::Count;
use crate::cmap::Variation;
use crate::compose::{CCC, COMPOSE, DECOMPOSE};
use crate::face::Face;
use crate::layout::{Digest, Features};
use crate::raster::{Bounds, InkBox};
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

/// The canonical combining class of `c`; zero for a starter.
pub fn combining_class(c: Codepoint) -> CombiningClass {
    let v = c.value();
    let i = CCC.partition_point(|&(_, last, _)| last < v);
    match CCC.get(i) {
        Some(&(first, _, class)) if first <= v => CombiningClass(class),
        _ => CombiningClass::STARTER,
    }
}

/// A canonical combining class (`[[source: unicode-ch3]]` D104): zero for
/// a starter, and for a mark the order it sorts in and the side of its
/// base it is placed on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CombiningClass(pub u8);

impl CombiningClass {
    pub const STARTER: CombiningClass = CombiningClass(0);
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

/// The horizontal centre of a glyph's ink, in font units, from its
/// control-point box.
fn ink_box(face: &Face<'_>, g: GlyphId) -> Option<InkBox> {
    let mut b = Bounds::new();
    face.outline(g, &mut b);
    b.ink()
}

/// A glyph box in HarfBuzz's extents form: a left bearing and width, a
/// top bearing and a *negative* height running down to the bottom.
#[derive(Clone, Copy)]
struct Extents {
    x_bearing: i32,
    width: i32,
    y_bearing: i32,
    height: i32,
}

impl Extents {
    fn of(ink: InkBox) -> Extents {
        Extents {
            x_bearing: ink.left,
            width: ink.right.saturating_sub(ink.left),
            y_bearing: ink.top,
            height: ink.bottom.saturating_sub(ink.top),
        }
    }
}

/// The base a run of marks attaches to: its cell-frame pen, its box with
/// the horizontal replaced by the advance, and the running box of the
/// class currently stacking on it.
struct BaseBox {
    dx: i32,
    /// A sixteenth of an em: the air between a base and its mark.
    gap: i32,
    component: Extents,
    cluster: Extents,
    last_class: Option<CombiningClass>,
}

/// Unicode's canonical combining classes that HarfBuzz places by.
mod ccc {
    use super::CombiningClass as C;
    pub const ATTACHED_BELOW_LEFT: C = C(200);
    pub const ATTACHED_BELOW: C = C(202);
    pub const ATTACHED_ABOVE: C = C(214);
    pub const ATTACHED_ABOVE_RIGHT: C = C(216);
    pub const BELOW_LEFT: C = C(218);
    pub const BELOW: C = C(220);
    pub const BELOW_RIGHT: C = C(222);
    pub const ABOVE_LEFT: C = C(228);
    pub const ABOVE: C = C(230);
    pub const ABOVE_RIGHT: C = C(232);
    pub const DOUBLE_BELOW: C = C(233);
    pub const DOUBLE_ABOVE: C = C(234);
}

impl BaseBox {
    fn of(g: Glyph, face: &Face<'_>, ink: InkBox) -> BaseBox {
        let mut component = Extents::of(ink);
        component.y_bearing = component.y_bearing.saturating_add(i32::from(g.dy));
        // The advance, not the ink, is the horizontal box: it also works
        // for a base that draws nothing.
        component.x_bearing = 0;
        component.width = face.advance(g.id).map_or(0, |a| i32::from(a.value()));
        BaseBox {
            dx: i32::from(g.dx),
            gap: i32::from(face.units_per_em().value()) / 16,
            component,
            cluster: component,
            last_class: None,
        }
    }

    /// One mark's offset from the base origin, left-to-right, updating
    /// the class's running box.
    fn place(&mut self, mark: Extents, class: CombiningClass) -> (i32, i32) {
        if self.last_class != Some(class) {
            self.last_class = Some(class);
            self.cluster = self.component;
        }
        let gap = self.gap;
        let b = &mut self.cluster;
        let x = match class {
            ccc::DOUBLE_BELOW | ccc::DOUBLE_ABOVE => {
                b.x_bearing + b.width - mark.width / 2 - mark.x_bearing
            }
            ccc::ATTACHED_BELOW_LEFT | ccc::BELOW_LEFT | ccc::ABOVE_LEFT => {
                b.x_bearing - mark.x_bearing
            }
            ccc::ATTACHED_ABOVE_RIGHT | ccc::BELOW_RIGHT | ccc::ABOVE_RIGHT => {
                b.x_bearing + b.width - mark.width - mark.x_bearing
            }
            _ => b.x_bearing + (b.width - mark.width) / 2 - mark.x_bearing,
        };
        let mut y = 0i32;
        match class {
            ccc::DOUBLE_BELOW
            | ccc::BELOW_LEFT
            | ccc::BELOW
            | ccc::BELOW_RIGHT
            | ccc::ATTACHED_BELOW_LEFT
            | ccc::ATTACHED_BELOW => {
                if class != ccc::ATTACHED_BELOW_LEFT && class != ccc::ATTACHED_BELOW {
                    b.height -= gap;
                }
                y = b.y_bearing + b.height - mark.y_bearing;
                // Never shift up a below mark.
                if (gap > 0) == (y > 0) {
                    b.height -= y;
                    y = 0;
                }
                b.height += mark.height;
            }
            ccc::DOUBLE_ABOVE
            | ccc::ABOVE_LEFT
            | ccc::ABOVE
            | ccc::ABOVE_RIGHT
            | ccc::ATTACHED_ABOVE
            | ccc::ATTACHED_ABOVE_RIGHT => {
                if class != ccc::ATTACHED_ABOVE && class != ccc::ATTACHED_ABOVE_RIGHT {
                    b.y_bearing += gap;
                    b.height -= gap;
                }
                y = b.y_bearing - mark.y_bearing - mark.height;
                // Do not shift an above mark down too far: halve it.
                if (gap > 0) != (y > 0) {
                    let correction = -(y / 2);
                    b.y_bearing += correction;
                    b.height -= correction;
                    y += correction;
                }
                b.y_bearing -= mark.height;
                b.height += mark.height;
            }
            _ => {}
        }
        (x, y)
    }
}

/// A shaping pass over one face: push clusters, then finish.
pub struct Shaper<'f, 'a, 'b> {
    face: &'f Face<'a>,
    buf: Buffer<'b>,
    missing: Count,
    /// Per-lookup digests for `GSUB`, when the caller keeps them.
    digests: &'f [Digest],
}

impl<'f, 'a, 'b> Shaper<'f, 'a, 'b> {
    pub fn new(face: &'f Face<'a>, storage: &'b mut [Glyph]) -> Self {
        Shaper {
            face,
            buf: Buffer::new(storage),
            missing: Count::ZERO,
            digests: &[],
        }
    }

    /// Hands the shaper the digests [`crate::Gsub::digests`] filled for
    /// this face, so `finish` opens only the lookups that can match. A
    /// renderer keeps one slice per face; the result is the same either
    /// way, and `tests/shape.rs` says so on every fixture.
    pub fn accelerate(&mut self, digests: &'f [Digest]) {
        self.digests = digests;
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
        // Canonical ordering (`[[source: unicode-ch3]]` D109): marks after
        // a starter sort by combining class, stably, so grave + dot-below
        // composes the same as dot-below + grave. Insertion sort, because
        // a cluster holds at most `CLUSTER_MAX` codepoints.
        let mut i = 1usize;
        while i < n {
            let class = combining_class(seq[i]).0;
            let mut j = i;
            while class != 0 && j > 0 {
                let before = combining_class(seq[j - 1]).0;
                if before == 0 || before <= class {
                    break;
                }
                seq.swap(j - 1, j);
                j -= 1;
            }
            i += 1;
        }
        // Compose onto the starter while the face has the result. A mark is
        // blocked from the starter by any mark between them of class zero
        // or of its own class or higher (`[[source: unicode-ch3]]` D115),
        // so `a` + dot-below + acute composes to `a-acute` + dot-below when
        // the face lacks `a-dot-below`, as HarfBuzz does.
        let mut out = [Codepoint::from_char(' '); CLUSTER_MAX];
        let mut m = 0usize;
        let mut starter: Option<usize> = None;
        let mut blocking: u8 = 0;
        for &c in &seq[..n] {
            let class = combining_class(c).0;
            if let Some(s) = starter {
                let blocked = m > s + 1 && (blocking == 0 || blocking >= class);
                if class != 0 && !blocked && Ignorable::of(c) == Ignorable::No {
                    if let Some(composed) = compose(out[s], c) {
                        if self.face.glyph(composed).is_some() {
                            out[s] = composed;
                            continue;
                        }
                    }
                }
                if class == 0 && m == s + 1 && Ignorable::of(c) == Ignorable::No {
                    // Two starters adjacent: a Hangul jamo pair.
                    if let Some(composed) = compose(out[s], c) {
                        if self.face.glyph(composed).is_some() {
                            out[s] = composed;
                            continue;
                        }
                    }
                }
            }
            if class == 0 {
                starter = Some(m);
                blocking = 0;
            } else {
                blocking = blocking.max(class);
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
            let mut glyph = if i == 0 {
                Glyph::new(id, cell, ignorable)
            } else {
                Glyph::new(id, cell, ignorable).attached()
            };
            glyph.ccc = combining_class(c);
            // A face with `GDEF` classes names its marks; without them
            // HarfBuzz synthesises the class from Unicode, and so does
            // baud -- a default-ignorable is never a mark.
            let gdef = self.face.gdef();
            glyph.class = gdef.class(id);
            glyph.mark = if gdef.has_classes() {
                if gdef.is_mark(id) {
                    Mark::Yes
                } else {
                    Mark::No
                }
            } else if glyph.ccc != CombiningClass::STARTER && ignorable == Ignorable::No {
                Mark::Yes
            } else {
                Mark::No
            };
            if self.buf.push(glyph).is_none() {
                while self.buf.len().value() > start.value() {
                    self.buf.remove(self.buf.len().as_index().prev()?);
                }
                return None;
            }
            i += 1;
        }
        Some(())
    }

    /// Every glyph's `dx` becomes its pen position within its cluster --
    /// the advances of the glyphs before it in the same cluster, from the
    /// cell origin -- so that positioning can add anchor offsets to an
    /// absolute frame. Hidden ignorables and marks advance nothing, as
    /// HarfBuzz zeroes them by `GDEF` class.
    fn pen_pass(&mut self) {
        // With no `GPOS` the fallback zeroes every classed mark it places,
        // whatever `GDEF` calls the glyph.
        let no_gpos = self.face.gpos().is_none();
        let mut pen = 0i32;
        let mut cluster: Option<Cluster> = None;
        let mut at = crate::bytes::Index::ZERO;
        while let Some(g) = self.buf.get(at) {
            if cluster != Some(g.cluster) {
                cluster = Some(g.cluster);
                pen = 0;
            }
            let zeroed = g.ignorable == Ignorable::Yes
                || g.mark == Mark::Yes
                || (no_gpos && g.ccc != CombiningClass::STARTER);
            let advance = if zeroed {
                0
            } else {
                self.face.advance(g.id).map_or(0, |a| i32::from(a.value()))
            };
            if let Some(glyph) = self.buf.get_mut(at) {
                glyph.dx = i16::try_from(pen).unwrap_or(i16::MAX);
            }
            pen = pen.saturating_add(advance);
            at = at.next();
        }
    }

    /// Geometric mark placement for a face with no `GPOS`, HarfBuzz's
    /// `[[source: harfbuzz-hb-ot-shape-fallback]]` in baud's frame. Each
    /// mark with a combining class is placed against the box of the base
    /// before it: centred, left- or right-aligned by class across the
    /// base's *advance* rather than its ink, and stacked above its top or
    /// below its bottom with a gap of a sixteenth of an em, each class
    /// keeping its own running box so a second above-mark rides the
    /// first. Marks on a ligature attach to the whole ligature where
    /// HarfBuzz attaches them to its last component; no fixture reaches
    /// the difference.
    fn fallback_marks(&mut self) {
        let mut base: Option<BaseBox> = None;
        let mut at = crate::bytes::Index::ZERO;
        while let Some(g) = self.buf.get(at) {
            let starts_base = g.attached == Attached::No
                || (g.ccc == CombiningClass::STARTER
                    && g.mark == Mark::No
                    && g.ignorable == Ignorable::No);
            if starts_base {
                // A base without extents zeroes its marks and goes home.
                base = ink_box(self.face, g.id).map(|ink| BaseBox::of(g, self.face, ink));
            } else if g.ccc != CombiningClass::STARTER
                && g.ignorable == Ignorable::No
                && g.positioned == Positioned::No
            {
                if let (Some(b), Some(mark)) = (base.as_mut(), ink_box(self.face, g.id)) {
                    let (x, y) = b.place(Extents::of(mark), g.ccc);
                    if let Some(glyph) = self.buf.get_mut(at) {
                        glyph.dx = i16::try_from(b.dx.saturating_add(x)).unwrap_or(0);
                        glyph.dy = i16::try_from(y).unwrap_or(0);
                        glyph.positioned = Positioned::Yes;
                    }
                }
            }
            at = at.next();
        }
    }

    /// Substitutes, positions, hides, and hands the buffer back.
    pub fn finish(mut self, features: Features) -> (Buffer<'b>, Shaped) {
        // `morx` stands in for `GSUB` on a face that has none -- HarfBuzz's
        // rule, and the one CoreText's own faces are built for; a face
        // carrying both shapes through `GSUB`.
        let mut declined = match (self.face.gsub(), self.face.morx()) {
            (Some(gsub), _) => gsub.apply_with(&mut self.buf, features, self.digests),
            (None, Some(morx)) => {
                // `morx` writes ids without classes; the positioning
                // filters read them, so they are read again once here.
                let declined = morx.apply(&mut self.buf);
                self.buf.retag(&self.face.gdef());
                declined
            }
            (None, None) => Count::ZERO,
        };
        self.pen_pass();
        // HarfBuzz falls back to geometric mark placement only when the
        // face has no GPOS at all; a face with one that names no mark
        // lookup leaves its marks at the pen, and so does baud.
        match self.face.gpos() {
            Some(gpos) => {
                let d = gpos.apply(&mut self.buf, features);
                declined = Count::of(declined.value().saturating_add(d.value()));
            }
            None => self.fallback_marks(),
        }
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
