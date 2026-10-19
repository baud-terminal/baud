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
//! decomposes into parts it has, else stays whole
//! (`[[source: unicode-data]]`); a variation selector resolves
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
use crate::glyf::VarPoint;
use crate::layout::{Digest, Features};
use crate::outline::Drawn;
use crate::raster::{Bounds, InkBox};
use crate::script::{RunText, Script};
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

/// Every character with a canonical decomposition, as inclusive ranges in
/// ascending order: each listed one alone, then the Hangul syllables.
pub fn each_decomposable(mut f: impl FnMut(u32, u32)) {
    for &(x, _, _) in DECOMPOSE {
        f(x, x);
    }
    f(S_BASE, S_BASE + S_COUNT - 1);
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

    /// The key the shaper sorts marks by: HarfBuzz's modified combining
    /// class (`[[source: harfbuzz-hb-unicode]]`, hb-unicode.hh:333-349 at
    /// 64e2b53), which permutes Hebrew's fixed-position classes 10-26 so
    /// shin and sin dot, dagesh, rafe and holam come before the vowels
    /// below, the group order of the SBL Hebrew manual's Table 1
    /// (`[[source: sbl-hebrew-manual-1-5]]`). Every other class is its
    /// own key. A permutation of classes is canonically equivalent, so
    /// what the text means is unchanged (`[[source: unicode-ch3]]` D109 and
    /// P5, and the Unicode FAQ: "any permutation of the non-zero CCC values
    /// can be used", `[[source: unicode-faq-normalization]]`); what it
    /// changes is the order a font's `ccmp` meets the points in: Courier
    /// New's forms the shin-dot ligature from shin + shin dot, and not
    /// with a qamats between them (note AB, 11.4). P5 of the same chapter:
    /// "Rendering systems should be prepared to deal with common typed-in
    /// sequences and with canonically reordered sequences". The owner's
    /// pick, 2026-10-13, for Hebrew only: HarfBuzz's Arabic, Syriac and
    /// Brahmic modifications are not taken.
    fn sort_key(self) -> CombiningClass {
        match self.0 {
            k @ 10..=26 => HEBREW_SORT_KEY[usize::from(k - 10)],
            _ => self,
        }
    }

    /// The class the fallback places a mark by on a face with no `GPOS`:
    /// HarfBuzz's `recategorize_combining_class`
    /// (`[[source: harfbuzz-hb-ot-shape-fallback]]`, hb-ot-shape-fallback.cc:85-115
    /// at 64e2b53), which gives each of Hebrew's fixed-position classes
    /// 10-26 the side of its base it sits on. Unicode's classes 10-26 name
    /// an order, not a side, so read as they are every point fell to a
    /// centred default with no vertical move. HarfBuzz recategorises only a
    /// nonspacing mark (line 180); all nineteen characters of classes 10-26
    /// are Hebrew nonspacing marks (`[[source: unicode-data]]`), so the class
    /// alone selects exactly HarfBuzz's set. The owner's pick, 2026-10-15,
    /// for Hebrew only, as [`CombiningClass::sort_key`] is: HarfBuzz's
    /// Arabic, Syriac, Thai, Lao and Tibetan recategorisations are not
    /// taken.
    fn placement(self) -> CombiningClass {
        match self.0 {
            k @ 10..=26 => HEBREW_PLACEMENT[usize::from(k - 10)],
            _ => self,
        }
    }
}

/// HarfBuzz's `HB_MODIFIED_COMBINING_CLASS_CCC10` to `CCC26`, in that
/// order: the sort key of canonical class 10 + i.
const HEBREW_SORT_KEY: [CombiningClass; 17] = [
    CombiningClass(22), // 10 sheva
    CombiningClass(15), // 11 hataf segol
    CombiningClass(16), // 12 hataf patah
    CombiningClass(17), // 13 hataf qamats
    CombiningClass(23), // 14 hiriq
    CombiningClass(18), // 15 tsere
    CombiningClass(19), // 16 segol
    CombiningClass(20), // 17 patah
    CombiningClass(21), // 18 qamats, qamats qatan
    CombiningClass(14), // 19 holam, holam haser for vav
    CombiningClass(24), // 20 qubuts
    CombiningClass(12), // 21 dagesh
    CombiningClass(25), // 22 meteg
    CombiningClass(13), // 23 rafe
    CombiningClass(10), // 24 shin dot
    CombiningClass(11), // 25 sin dot
    CombiningClass(26), // 26 point varika
];

/// `recategorize_combining_class`'s Hebrew cases, in canonical-class order:
/// the placement class of canonical class 10 + i. HarfBuzz switches on its
/// modified classes; each case names the point, and the point's canonical
/// class is what indexes here. Dagesh's case is a `break`, which leaves it
/// at its own class: centred, with no vertical move.
const HEBREW_PLACEMENT: [CombiningClass; 17] = [
    ccc::BELOW,          // 10 sheva
    ccc::BELOW,          // 11 hataf segol
    ccc::BELOW,          // 12 hataf patah
    ccc::BELOW,          // 13 hataf qamats
    ccc::BELOW,          // 14 hiriq
    ccc::BELOW,          // 15 tsere
    ccc::BELOW,          // 16 segol
    ccc::BELOW,          // 17 patah
    ccc::BELOW,          // 18 qamats, qamats qatan
    ccc::ABOVE_LEFT,     // 19 holam, holam haser for vav
    ccc::BELOW,          // 20 qubuts
    CombiningClass(21),  // 21 dagesh
    ccc::BELOW,          // 22 meteg
    ccc::ATTACHED_ABOVE, // 23 rafe
    ccc::ABOVE_RIGHT,    // 24 shin dot
    ccc::ABOVE_LEFT,     // 25 sin dot
    ccc::ABOVE,          // 26 point varika
];

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
    /// Glyph boxes the fallback mark placement asked for and was refused,
    /// because the outline needs more scratch than [`Shaper::scratch`]
    /// gave it ([`Drawn::NeedsScratch`]): a base refused leaves its marks
    /// where the pen put them and a mark refused stays at the pen, as a
    /// glyph with no box does. Never refused silently: a caller that sees
    /// this above zero grows its scratch to [`Shaped::ink_need`] and
    /// shapes again, or counts what it would not grow for.
    pub ink_refused: Count,
    /// The most scratch any refused box needed, in [`VarPoint`]s.
    pub ink_need: Count,
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

/// A cluster's codepoints while it is normalised: at most `CLUSTER_MAX`,
/// the rest cut, as a cluster is.
#[derive(Clone, Copy)]
struct Seq {
    cps: [Codepoint; CLUSTER_MAX],
    len: Count,
}

impl Seq {
    fn empty() -> Seq {
        Seq {
            cps: [Codepoint::from_char(' '); CLUSTER_MAX],
            len: Count::ZERO,
        }
    }

    fn as_slice(&self) -> &[Codepoint] {
        &self.cps[..self.len.slot()]
    }

    /// `c` after the rest, unless the sequence is full.
    fn push(&mut self, c: Codepoint) {
        if let Some(slot) = self.cps.get_mut(self.len.slot()) {
            *slot = c;
            self.len = self.len.plus(Count::of(1u8)).unwrap_or(self.len);
        }
    }

    /// `c`'s full canonical decomposition after the rest (D68).
    fn decompose_fully(&mut self, c: Codepoint) {
        match decompose(c) {
            Some((a, b)) => {
                self.decompose_fully(a);
                if let Some(b) = b {
                    self.decompose_fully(b);
                }
            }
            None => self.push(c),
        }
    }

    /// The marks after each starter sorted stably by `key`, a starter's
    /// key being zero. Insertion sort, because a cluster holds at most
    /// `CLUSTER_MAX` codepoints.
    fn order(&mut self, key: impl Fn(Codepoint) -> CombiningClass) {
        let seq = &mut self.cps[..self.len.slot()];
        let mut i = 1usize;
        while i < seq.len() {
            let class = key(seq[i]).0;
            let mut j = i;
            while class != 0 && j > 0 {
                let before = key(seq[j - 1]).0;
                if before == 0 || before <= class {
                    break;
                }
                seq.swap(j - 1, j);
                j -= 1;
            }
            i += 1;
        }
    }

    /// The sequence with each mark composed onto its starter where `takes`
    /// accepts the composite. A mark is blocked from the starter by any
    /// mark between them of class zero or of its own class or higher
    /// (`[[source: unicode-ch3]]` D115); two adjacent starters compose as
    /// a Hangul jamo pair does. A default-ignorable never composes.
    fn composed(&self, takes: impl Fn(Codepoint) -> bool) -> Seq {
        let mut out = Seq::empty();
        let mut starter: Option<usize> = None;
        let mut blocking: u8 = 0;
        for &c in self.as_slice() {
            let class = combining_class(c).0;
            let m = out.len.slot();
            if let Some(s) = starter {
                let blocked = m > s + 1 && (blocking == 0 || blocking >= class);
                let pair = class == 0 && m == s + 1;
                if (class != 0 && !blocked || pair) && Ignorable::of(c) == Ignorable::No {
                    if let Some(composed) = compose(out.cps[s], c) {
                        if takes(composed) {
                            out.cps[s] = composed;
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
            out.push(c);
        }
        out
    }
}

/// A cluster as the shaper maps it on one face: a base and its
/// attachments after [`Shaper::push_cluster`]'s normalisation -- a
/// character the face lacks decomposed only into parts the face has,
/// else kept whole, the marks in the shaper's order
/// ([`CombiningClass::sort_key`]: canonical but for Hebrew points), and each
/// composed onto its starter where the face has the composite.
///
/// **The one spelling of those rules.** `push_cluster` maps exactly this
/// sequence, and a renderer asks the same value which face draws a cluster
/// ([`Normalised::covered`]) and what a face it keeps must leave out
/// ([`Normalised::without_unmapped`]) -- so a face is never judged to draw
/// a cluster the shaper would then box. `q` + U+0316 + U+0301 on JetBrains
/// Mono is `q`, a `.notdef` the face classes as a base with a full advance,
/// and the acute over the box one cell right: a judgement made on the
/// codepoints as typed, rather than on what is mapped, would differ from
/// this one exactly where decomposition or composition changed the
/// sequence.
#[derive(Clone, Copy)]
pub struct Normalised<'f, 'a> {
    face: &'f Face<'a>,
    seq: [Codepoint; CLUSTER_MAX],
    len: Count,
}

impl<'f, 'a> Normalised<'f, 'a> {
    /// `cps` -- a base followed by its attachments, at most `CLUSTER_MAX` of
    /// them read -- normalised for `face`.
    pub fn of(face: &'f Face<'a>, cps: &[Codepoint]) -> Normalised<'f, 'a> {
        // A character the face lacks comes apart only into parts it has,
        // as HarfBuzz's `decompose` takes it: each level's mark mapped, its
        // first part mapped or coming apart the same way. Otherwise it
        // stays whole, and is the one `.notdef` that stands for it
        // (`[[source: unicode18-ch5]]` 5.3), in its own cell -- rather than
        // a base and a box for the part the face lacks, which a full
        // advance put in the next cell.
        let mut seq = Seq::empty();
        for &c in cps.iter().take(CLUSTER_MAX) {
            let mut parts = Seq::empty();
            let taken = face.glyph(c).is_none()
                && Normalised::parts_by(&mut |x| face.glyph(x).is_some(), c, &mut parts).is_some();
            match taken {
                true => parts.as_slice().iter().for_each(|&part| seq.push(part)),
                false => seq.push(c),
            }
        }
        // Mark ordering: marks after a starter sort stably by
        // `CombiningClass::sort_key` -- the Canonical Ordering Algorithm
        // (`[[source: unicode-ch3]]` D109) over a permutation of the
        // classes that differs from Unicode's only for Hebrew points -- so
        // grave + dot-below composes the same as dot-below + grave.
        seq.order(|c| combining_class(c).sort_key());
        // Compose onto the starter while the face has the result, so
        // `a` + dot-below + acute composes to `a-acute` + dot-below when
        // the face lacks `a-dot-below`, as HarfBuzz does.
        let out = seq.composed(|composed| face.glyph(composed).is_some());
        Normalised {
            face,
            seq: out.cps,
            len: out.len,
        }
    }

    /// `cps` in Normalization Form C (`[[source: unicode18-ch3]]` 3.11):
    /// every character taken apart all the way (D68), the marks put in
    /// canonical order (D109) and composed back by the Canonical
    /// Composition Algorithm, whatever any face maps. Two canonical
    /// equivalents come out the same.
    fn canonical(cps: &[Codepoint]) -> Seq {
        let mut seq = Seq::empty();
        for &c in cps.iter().take(CLUSTER_MAX) {
            seq.decompose_fully(c);
        }
        seq.order(combining_class);
        seq.composed(|_| true)
    }

    /// Whether `c`, a character `face` lacks, comes apart into parts it
    /// has: what [`Normalised::covered`] answers for `c` alone, without
    /// normalising -- the parts are mapped by construction, and composing
    /// them takes only composites the face maps.
    pub fn comes_apart(face: &Face<'_>, c: Codepoint) -> bool {
        Normalised::comes_apart_by(c, &mut |x| face.glyph(x).is_some())
    }

    /// [`Normalised::comes_apart`] for a face whose `cmap` answers `has`, so
    /// a caller asking it of many characters can remember the answers --
    /// the parts of 256 Hangul syllables are 67 jamo.
    pub fn comes_apart_by(c: Codepoint, has: &mut impl FnMut(Codepoint) -> bool) -> bool {
        Normalised::parts_by(has, c, &mut Seq::empty()).is_some()
    }

    /// `c` -- a character the face whose `cmap` answers `has` lacks --
    /// taken apart onto `out` as far as the face needs; `None` when a part
    /// the face lacks would be left over. The second part of each level
    /// must be mapped; the first is kept where the face maps it and taken
    /// apart again where it does not (U+1E09 is U+00E7 and an acute, U+00E7
    /// a `c` and a cedilla). A singleton is its one part.
    fn parts_by(
        has: &mut impl FnMut(Codepoint) -> bool,
        c: Codepoint,
        out: &mut Seq,
    ) -> Option<()> {
        let (a, b) = decompose(c)?;
        if b.is_some_and(|b| !has(b)) {
            return None;
        }
        match has(a) {
            true => out.push(a),
            false => Normalised::parts_by(has, a, out)?,
        }
        if let Some(b) = b {
            out.push(b);
        }
        Some(())
    }

    /// The sequence the shaper maps, base first.
    pub fn as_slice(&self) -> &[Codepoint] {
        &self.seq[..self.len.slot()]
    }

    /// Whether the face has a glyph for every codepoint of the sequence that
    /// is not default-ignorable, so the cluster draws from it with no
    /// `.notdef`. A joiner, a mark of direction or a variation selector the
    /// face lacks is hidden whatever face draws it ([`Ignorable`]), so it is
    /// never required: Ghostty and foot fell back, or drew U+FFFD, for the
    /// joiners and the marks of direction a face did not map (note AB,
    /// section 8).
    pub fn covered(&self) -> bool {
        self.as_slice()
            .iter()
            .all(|&c| Ignorable::of(c) == Ignorable::Yes || self.face.glyph(c).is_some())
    }

    /// `cps` without the marks `face` has no glyph for, and how many were
    /// left out: the codepoints to hand [`Shaper::push_cluster`], whose
    /// normalisation of them maps nothing after the first that is not
    /// default-ignorable and that the face lacks. The first codepoint of
    /// the normalised sequence is never left out: it is the base, whole
    /// where the face cannot draw it in parts, and a base no face draws is
    /// the `.notdef` that says so. (A cluster that begins with a mark --
    /// which only a caller of `Renderer::cluster` can build, since the core
    /// hangs a mark on the cell before it -- is the exception: the mark
    /// order can sort a lower-class mark the face lacks to the front, and
    /// that is kept as a `.notdef`.)
    ///
    /// **It starts from the cluster's canonical form**, so canonical
    /// equivalents lose the same marks: A + U+0311, on a face with neither
    /// Ȃ nor U+0311, is Ȃ, one `.notdef` and nothing left out, as Ȃ is.
    ///
    /// **One pass is not enough, so it passes until nothing is dropped.**
    /// What is left is normalised again, and a composition a dropped mark
    /// blocked takes place -- `a` + U+0305 + U+0309 on JetBrains Mono draws
    /// the `ả` it has for `a` + U+0309, which is what the letter looks like
    /// with the overline never typed. So each pass normalises what the last
    /// one kept and drops from that, and the codepoints returned are the
    /// last pass's input, whose normalisation was judged and found to need
    /// nothing dropped: `push_cluster` maps exactly what was judged. The
    /// walk ends because a pass that does not end it drops a codepoint, and
    /// a cluster holds at most `CLUSTER_MAX`; a cap of `CLUSTER_MAX` passes
    /// bounds a hang the argument rules out.
    pub fn without_unmapped(face: &'f Face<'a>, cps: &[Codepoint]) -> Kept {
        let mut kept = Kept {
            seq: [Codepoint::from_char(' '); CLUSTER_MAX],
            len: Count::ZERO,
            dropped: Count::ZERO,
        };
        // **From the cluster's canonical form**, so a letter typed as a
        // base and a mark the face lacks is the precomposed letter it is
        // canonically equivalent to (C6): Ȃ, and A + U+0311 as Ȃ, are one
        // `.notdef` the face has for neither, not an `A` with its breve
        // left out.
        let canonical = Normalised::canonical(cps);
        kept.seq = canonical.cps;
        kept.len = canonical.len;
        for _ in 0..CLUSTER_MAX {
            let normalised = Normalised::of(face, kept.as_slice());
            let mut next = [Codepoint::from_char(' '); CLUSTER_MAX];
            let mut m = 0usize;
            let mut dropped = Count::ZERO;
            for (i, &c) in normalised.as_slice().iter().enumerate() {
                if i > 0 && Ignorable::of(c) == Ignorable::No && face.glyph(c).is_none() {
                    dropped = dropped.plus(Count::of(1u8)).unwrap_or(dropped);
                    continue;
                }
                next[m] = c;
                m += 1;
            }
            if dropped.is_zero() {
                break;
            }
            kept.seq = next;
            kept.len = Count::of(u8::try_from(m).unwrap_or(0));
            kept.dropped = kept.dropped.plus(dropped).unwrap_or(kept.dropped);
        }
        kept
    }
}

/// A cluster with the marks its face cannot draw left out
/// ([`Normalised::without_unmapped`]).
#[derive(Clone, Copy)]
pub struct Kept {
    seq: [Codepoint; CLUSTER_MAX],
    len: Count,
    dropped: Count,
}

impl Kept {
    /// The codepoints to hand [`Shaper::push_cluster`].
    pub fn as_slice(&self) -> &[Codepoint] {
        &self.seq[..self.len.slot()]
    }

    /// How many codepoints were left out, over every pass.
    pub fn dropped(&self) -> Count {
        self.dropped
    }
}

/// A shaping pass over one face: push clusters, then finish.
pub struct Shaper<'f, 'a, 'b> {
    face: &'f Face<'a>,
    buf: Buffer<'b>,
    missing: Count,
    /// Per-lookup digests for `GSUB`, when the caller keeps them.
    digests: &'f [Digest],
    /// The caller's scratch for an outline that needs it, which a varied
    /// face's glyph does ([`Face::outline_at`]); empty until
    /// [`Shaper::scratch`] hands some over.
    scratch: &'b mut [VarPoint],
    ink_refused: Count,
    ink_need: Count,
    /// What the run's characters say about its direction, which `morx`
    /// reads; noted only on a face that shapes through `morx`.
    run: RunText,
}

impl<'f, 'a, 'b> Shaper<'f, 'a, 'b> {
    pub fn new(face: &'f Face<'a>, storage: &'b mut [Glyph]) -> Self {
        Shaper {
            face,
            buf: Buffer::new(storage),
            missing: Count::ZERO,
            digests: &[],
            scratch: &mut [],
            ink_refused: Count::ZERO,
            ink_need: Count::ZERO,
            run: RunText::EMPTY,
        }
    }

    /// Hands the shaper scratch for the glyph boxes the fallback mark
    /// placement reads: a face read at a variation draws its glyphs only
    /// through [`Face::outline_at`], as any face draws a composite that
    /// places a component by matched points, and a box it is refused is
    /// counted in
    /// [`Shaped::ink_refused`] with the size that would have drawn it.
    pub fn scratch(&mut self, scratch: &'b mut [VarPoint]) {
        self.scratch = scratch;
    }

    /// A glyph's control-point box in font units, at the face's
    /// coordinates. `None` for a glyph that draws nothing, or for one
    /// whose outline the scratch is too small for, which is counted.
    fn ink_box(&mut self, g: GlyphId) -> Option<InkBox> {
        let mut b = Bounds::new();
        if let Drawn::NeedsScratch(need) = self.face.outline_at(g, self.scratch, &mut b) {
            self.ink_refused = Count::of(self.ink_refused.value().saturating_add(1));
            self.ink_need = self.ink_need.max(need);
            return None;
        }
        b.ink()
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
    /// `cell`, mapping the sequence [`Normalised::of`] makes of it. `None`
    /// when the storage is full, leaving the buffer as it was before the
    /// call; the run's direction has still noted the refused cluster's
    /// characters.
    pub fn push_cluster(&mut self, cps: &[Codepoint], cell: Cluster) -> Option<()> {
        if self.face.gsub().is_none() && self.face.morx().is_some() {
            for &c in cps {
                self.run.push(c);
            }
        }
        let normalised = Normalised::of(self.face, cps);
        let out = normalised.as_slice();
        let m = out.len();
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
    /// first. A Hebrew point is placed by the class HarfBuzz recategorises
    /// it to ([`CombiningClass::placement`]), which is also the class whose
    /// running box it rides: sheva and meteg stack below as one class, sin
    /// dot and holam above left. Marks on a ligature attach to the whole
    /// ligature where HarfBuzz attaches them to its last component; no
    /// fixture reaches the difference.
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
                base = self.ink_box(g.id).map(|ink| BaseBox::of(g, self.face, ink));
            } else if g.ccc != CombiningClass::STARTER
                && g.ignorable == Ignorable::No
                && g.positioned == Positioned::No
            {
                let mark = self.ink_box(g.id);
                if let (Some(b), Some(mark)) = (base.as_mut(), mark) {
                    let (x, y) = b.place(Extents::of(mark), g.ccc.placement());
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

    /// Substitutes, positions, hides, and hands the buffer back. `script` is
    /// the run's script, settled as [`crate::resolve`] settles it: it picks
    /// the `GSUB` and `GPOS` Script tables ([`crate::layout`]). A run is in
    /// one script; a caller splits where it changes, or where it changes
    /// which lookups apply ([`crate::Gsub::lookups`]).
    pub fn finish(mut self, features: Features, script: Script) -> (Buffer<'b>, Shaped) {
        // `morx` stands in for `GSUB` on a face that has none -- HarfBuzz's
        // rule, and the one CoreText's own faces are built for; a face
        // carrying both shapes through `GSUB`.
        let mut declined = match (self.face.gsub(), self.face.morx()) {
            (Some(gsub), _) => gsub.apply_with(&mut self.buf, script, features, self.digests),
            (None, Some(morx)) => {
                // `morx` writes ids without classes; the positioning
                // filters read them, so they are read again once here.
                let declined = morx.apply(&mut self.buf, self.run.direction(script));
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
                let d = gpos.apply(&mut self.buf, script, features, self.face.coords());
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
                ink_refused: self.ink_refused,
                ink_need: self.ink_need,
            },
        )
    }
}
