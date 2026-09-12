//! `loca` and `glyf`: TrueType outlines (`[[source: ms-opentype-loca]]`,
//! `[[source: ms-opentype-glyf]]`).
//!
//! **The emission rule is the one every reader agrees on**, and it is what
//! makes an exact differential possible: start a contour at its first
//! on-curve point; two consecutive off-curve points imply an on-curve point at
//! their midpoint; a contour that starts off-curve starts at the midpoint of
//! its first two points if both are off-curve, or at the point after; and the
//! contour closes back to where it began, through the last control point if
//! one is pending. `ttf-parser` spells that rule the same way this file does,
//! and `tests/oracle.rs` holds the two to bit-identical output.
//!
//! **What is bounded.** Composite recursion at 32 deep -- the specification
//! sets no limit and a malformed font can point a component at itself; a
//! `loca` entry is used only if it is ascending and inside `glyf`; every
//! coordinate array's length is derived from the flags before a point is
//! read, so a lying flag cannot walk past the glyph.
//!
//! **What is deliberately not done.** Point-matching composites (arguments
//! that name points rather than offsets) are read and placed at zero offset
//! rather than matched -- FreeType matches them, the oracle does not read them
//! at all, and no face on this Mac uses them; `SCALED_COMPONENT_OFFSET` and
//! `ROUND_XY_TO_GRID` are ignored as every reference reader ignores them.

use crate::bytes::{Array, Bytes, Count, Index, Len, Offset};
use crate::outline::{Sink, Transform};
use baud_domain::GlyphId;

const MAX_DEPTH: u8 = 32;

/// `head.indexToLocFormat`: two-byte halved offsets, or four-byte offsets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LocaFormat {
    Short,
    Long,
}

#[derive(Clone, Copy)]
pub struct Loca<'a> {
    data: Bytes<'a>,
    format: LocaFormat,
    entries: Count,
}

impl<'a> Loca<'a> {
    /// `numGlyphs + 1` entries, or as many as the table actually holds --
    /// a short table is read as far as it goes rather than refused.
    pub fn parse(data: Bytes<'a>, format: LocaFormat, glyphs: Count) -> Loca<'a> {
        let size = match format {
            LocaFormat::Short => 2u32,
            LocaFormat::Long => 4u32,
        };
        let available = Count::of(len_value(data.len()) / size);
        let wanted = glyphs.as_index().next();
        let entries = if available.holds(wanted) {
            Count::of(index_value(wanted))
        } else {
            available
        };
        Loca {
            data,
            format,
            entries,
        }
    }

    fn entry(&self, i: Index) -> Option<Offset> {
        match self.format {
            LocaFormat::Short => {
                let arr: Array<'a, u16> = self.data.array(Offset::ZERO, self.entries)?;
                Some(Offset::of(u32::from(arr.get(i)?).checked_mul(2)?))
            }
            LocaFormat::Long => {
                let arr: Array<'a, u32> = self.data.array(Offset::ZERO, self.entries)?;
                Some(Offset::of(arr.get(i)?))
            }
        }
    }

    /// Where glyph `g` lives in `glyf`, or `None` when the entry is absent,
    /// empty or descending.
    pub fn range(&self, g: GlyphId) -> Option<(Offset, Len)> {
        let i = Index::of(u32::try_from(g.index()).ok()?);
        let next = i.next();
        if !self.entries.holds(next) {
            return None;
        }
        let start = self.entry(i)?;
        let end = self.entry(next)?;
        let len = end.since(start)?;
        (!len.is_zero()).then_some((start, len))
    }
}

fn len_value(l: Len) -> u32 {
    l.value()
}

fn index_value(i: Index) -> u32 {
    i.value()
}

/// The contour-emission state of one simple glyph, or one component.
struct Contour<'s, S: Sink> {
    sink: &'s mut S,
    transform: Transform,
    first_on: Option<(f32, f32)>,
    first_off: Option<(f32, f32)>,
    last_off: Option<(f32, f32)>,
}

fn mid(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    // `a + 0.5 * (b - a)`, the reference's lerp, kept in that form so the
    // rounding is the same rounding.
    (a.0 + 0.5 * (b.0 - a.0), a.1 + 0.5 * (b.1 - a.1))
}

impl<S: Sink> Contour<'_, S> {
    fn tx(&self, p: (f32, f32)) -> (f32, f32) {
        if self.transform.is_identity() {
            p
        } else {
            self.transform.apply(p.0, p.1)
        }
    }

    fn move_to(&mut self, p: (f32, f32)) {
        let p = self.tx(p);
        self.sink.move_to(p.0, p.1);
    }

    fn line_to(&mut self, p: (f32, f32)) {
        let p = self.tx(p);
        self.sink.line_to(p.0, p.1);
    }

    fn quad_to(&mut self, c: (f32, f32), p: (f32, f32)) {
        let c = self.tx(c);
        let p = self.tx(p);
        self.sink.quad_to(c.0, c.1, p.0, p.1);
    }

    fn push(&mut self, p: (f32, f32), on_curve: OnCurve, last: LastPoint) {
        if self.first_on.is_none() {
            if on_curve == OnCurve::Yes {
                self.first_on = Some(p);
                self.move_to(p);
            } else if let Some(off) = self.first_off {
                let m = mid(off, p);
                self.first_on = Some(m);
                self.last_off = Some(p);
                self.move_to(m);
            } else {
                self.first_off = Some(p);
            }
        } else {
            match (self.last_off, on_curve) {
                (Some(off), OnCurve::Yes) => {
                    self.last_off = None;
                    self.quad_to(off, p);
                }
                (Some(off), OnCurve::No) => {
                    self.last_off = Some(p);
                    let m = mid(off, p);
                    self.quad_to(off, m);
                }
                (None, OnCurve::Yes) => self.line_to(p),
                (None, OnCurve::No) => self.last_off = Some(p),
            }
        }
        if last == LastPoint::Yes {
            self.finish();
        }
    }

    fn finish(&mut self) {
        if let (Some(off1), Some(off2)) = (self.first_off, self.last_off) {
            self.last_off = None;
            let m = mid(off2, off1);
            self.quad_to(off2, m);
        }
        if let (Some(p), Some(off1)) = (self.first_on, self.first_off) {
            self.quad_to(off1, p);
        } else if let (Some(p), Some(off2)) = (self.first_on, self.last_off) {
            self.quad_to(off2, p);
        } else if let Some(p) = self.first_on {
            self.line_to(p);
        }
        self.first_on = None;
        self.first_off = None;
        self.last_off = None;
        self.sink.close();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OnCurve {
    Yes,
    No,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LastPoint {
    Yes,
    No,
}

/// A simple glyph's flag byte.
#[derive(Clone, Copy)]
struct Flags(u8);

impl Flags {
    fn on_curve(self) -> OnCurve {
        if self.0 & 0x01 != 0 {
            OnCurve::Yes
        } else {
            OnCurve::No
        }
    }
    fn x_short(self) -> bool {
        self.0 & 0x02 != 0
    }
    fn y_short(self) -> bool {
        self.0 & 0x04 != 0
    }
    fn repeat(self) -> bool {
        self.0 & 0x08 != 0
    }
    fn x_same_or_positive(self) -> bool {
        self.0 & 0x10 != 0
    }
    fn y_same_or_positive(self) -> bool {
        self.0 & 0x20 != 0
    }
}

/// A composite record's flag word.
#[derive(Clone, Copy)]
struct Component(u16);

impl Component {
    fn words(self) -> bool {
        self.0 & 0x0001 != 0
    }
    fn xy_values(self) -> bool {
        self.0 & 0x0002 != 0
    }
    fn scale(self) -> bool {
        self.0 & 0x0008 != 0
    }
    fn more(self) -> bool {
        self.0 & 0x0020 != 0
    }
    fn xy_scale(self) -> bool {
        self.0 & 0x0040 != 0
    }
    fn two_by_two(self) -> bool {
        self.0 & 0x0080 != 0
    }
}

/// A 2.14 fixed-point value, as composite scales are stored.
fn f2dot14(v: i16) -> f32 {
    f32::from(v) / 16384.0
}

/// Walks the flags once to learn how long the two coordinate arrays are,
/// which the file does not say. Returns `(x bytes, y bytes, flag bytes)`.
fn coord_lengths(flags: Bytes<'_>, points: u32) -> Option<(Len, Len, Len)> {
    let mut left = points;
    let mut at = Offset::ZERO;
    let mut xs = 0u32;
    let mut ys = 0u32;
    while left > 0 {
        let f = Flags(flags.read(at)?);
        at = at.plus(Len::of(1))?;
        let repeats = if f.repeat() {
            let r: u8 = flags.read(at)?;
            at = at.plus(Len::of(1))?;
            u32::from(r).checked_add(1)?
        } else {
            1
        };
        if repeats > left {
            return None;
        }
        if f.x_short() {
            xs = xs.checked_add(repeats)?;
        } else if !f.x_same_or_positive() {
            xs = xs.checked_add(repeats.checked_mul(2)?)?;
        }
        if f.y_short() {
            ys = ys.checked_add(repeats)?;
        } else if !f.y_same_or_positive() {
            ys = ys.checked_add(repeats.checked_mul(2)?)?;
        }
        left -= repeats;
    }
    Some((Len::of(xs), Len::of(ys), at.since_start()))
}

/// One coordinate stream, decoded delta by delta.
struct Coords<'a> {
    data: Bytes<'a>,
    at: Offset,
    prev: i16,
}

/// One coordinate's encoding, from its two flag bits: one byte with the sign
/// in the flag, no bytes and the previous value repeated, or two bytes signed.
#[derive(Clone, Copy)]
enum Delta {
    Short(Sign),
    Same,
    Long,
}

#[derive(Clone, Copy)]
enum Sign {
    Positive,
    Negative,
}

impl Flags {
    fn x_delta(self) -> Delta {
        Delta::of(self.x_short(), self.x_same_or_positive())
    }
    fn y_delta(self) -> Delta {
        Delta::of(self.y_short(), self.y_same_or_positive())
    }
}

impl Delta {
    fn of(short: bool, same_or_positive: bool) -> Delta {
        match (short, same_or_positive) {
            (true, true) => Delta::Short(Sign::Positive),
            (true, false) => Delta::Short(Sign::Negative),
            (false, true) => Delta::Same,
            (false, false) => Delta::Long,
        }
    }
}

impl Coords<'_> {
    fn next(&mut self, delta: Delta) -> i16 {
        // Past the end reads as zero: the lengths were derived from the flags
        // and checked against the glyph, so this is the reference's fallback
        // and not a live path.
        let n: i16 = match delta {
            Delta::Short(sign) => {
                let b: u8 = self.data.read(self.at).unwrap_or(0);
                self.at = self.at.plus(Len::of(1)).unwrap_or(self.at);
                match sign {
                    Sign::Positive => i16::from(b),
                    Sign::Negative => -i16::from(b),
                }
            }
            Delta::Same => 0,
            Delta::Long => {
                let v: i16 = self.data.read(self.at).unwrap_or(0);
                self.at = self.at.plus(Len::of(2)).unwrap_or(self.at);
                v
            }
        };
        self.prev = self.prev.wrapping_add(n);
        self.prev
    }
}

/// The table pair; `outline` is the entry point.
#[derive(Clone, Copy)]
pub struct Glyf<'a> {
    loca: Loca<'a>,
    data: Bytes<'a>,
}

impl<'a> Glyf<'a> {
    pub fn new(loca: Loca<'a>, data: Bytes<'a>) -> Glyf<'a> {
        Glyf { loca, data }
    }

    fn glyph(&self, g: GlyphId) -> Option<Bytes<'a>> {
        let (at, len) = self.loca.range(g)?;
        self.data.slice(at, len)
    }

    /// Emits `g` into `sink`. `Some(())` when the glyph exists and parsed,
    /// even if it drew nothing; `None` when it has no data or is malformed.
    pub fn outline<S: Sink>(&self, g: GlyphId, sink: &mut S) -> Option<()> {
        let data = self.glyph(g)?;
        self.outline_into(data, Transform::IDENTITY, 0, sink)
    }

    fn outline_into<S: Sink>(
        &self,
        data: Bytes<'a>,
        transform: Transform,
        depth: u8,
        sink: &mut S,
    ) -> Option<()> {
        if depth >= MAX_DEPTH {
            return None;
        }
        let contours: i16 = data.read(Offset::ZERO)?;
        // numberOfContours, then the bounding box, which is not trusted.
        let body = data.tail(Offset::of(10u32))?;
        if contours > 0 {
            self.simple(body, u16::try_from(contours).ok()?, transform, sink)
        } else if contours < 0 {
            self.composite(body, transform, depth, sink)
        } else {
            Some(())
        }
    }

    fn simple<S: Sink>(
        &self,
        body: Bytes<'a>,
        contours: u16,
        transform: Transform,
        sink: &mut S,
    ) -> Option<()> {
        let ends: Array<'a, u16> = body.array(Offset::ZERO, Count::of(contours))?;
        let last_end = ends.get(Count::of(contours).last()?)?;
        let points = u32::from(last_end).checked_add(1)?;
        if points == 1 {
            // A contour of one point draws nothing and is not an error.
            return Some(());
        }
        let after_ends = Offset::ZERO.at(Count::of(contours).as_index(), Len::of(2))?;
        let instructions: u16 = body.read(after_ends)?;
        let flags_at = after_ends
            .plus(Len::of(2))?
            .plus(Len::of(u32::from(instructions)))?;
        let flags = body.tail(flags_at)?;
        let (xs, ys, flag_bytes) = coord_lengths(flags, points)?;
        let flags = flags.slice(Offset::ZERO, flag_bytes)?;
        let x_at = flags_at.plus(flag_bytes)?;
        let y_at = x_at.plus(xs)?;
        let x_coords = body.slice(x_at, xs)?;
        let y_coords = body.slice(y_at, ys)?;

        let mut contour = Contour {
            sink,
            transform,
            first_on: None,
            first_off: None,
            last_off: None,
        };
        let mut xc = Coords {
            data: x_coords,
            at: Offset::ZERO,
            prev: 0,
        };
        let mut yc = Coords {
            data: y_coords,
            at: Offset::ZERO,
            prev: 0,
        };
        // Endpoints as a per-point "last" flag, the reference's iterator: the
        // first contour ends at endPts[0]; each later one at endPts[k], with
        // a malformed descending list read as an empty contour.
        let mut end_index = Index::ZERO;
        let mut left = u32::from(ends.get(Index::ZERO)?);
        let mut flag_at = Offset::ZERO;
        let mut repeats: u8 = 0;
        let mut f = Flags(0);
        for _ in 0..points {
            let last = if left == 0 {
                end_index = end_index.next();
                if let Some(end) = ends.get(end_index) {
                    let prev = end_index.prev().and_then(|p| ends.get(p)).unwrap_or(0);
                    left = u32::from(end.saturating_sub(prev).saturating_sub(1));
                }
                LastPoint::Yes
            } else {
                left -= 1;
                LastPoint::No
            };
            if repeats == 0 {
                f = Flags(flags.read(flag_at).unwrap_or(0));
                flag_at = flag_at.plus(Len::of(1)).unwrap_or(flag_at);
                if f.repeat() {
                    repeats = flags.read(flag_at).unwrap_or(0);
                    flag_at = flag_at.plus(Len::of(1)).unwrap_or(flag_at);
                }
            } else {
                repeats -= 1;
            }
            let x = xc.next(f.x_delta());
            let y = yc.next(f.y_delta());
            contour.push((f32::from(x), f32::from(y)), f.on_curve(), last);
        }
        Some(())
    }

    fn composite<S: Sink>(
        &self,
        body: Bytes<'a>,
        transform: Transform,
        depth: u8,
        sink: &mut S,
    ) -> Option<()> {
        let mut at = Offset::ZERO;
        loop {
            let flags = Component(body.read(at)?);
            let glyph: u16 = body.read(at.plus(Len::of(2))?)?;
            at = at.plus(Len::of(4))?;
            let mut t = Transform::IDENTITY;
            if flags.words() {
                if flags.xy_values() {
                    let e: i16 = body.read(at)?;
                    let f: i16 = body.read(at.plus(Len::of(2))?)?;
                    t.e = f32::from(e);
                    t.f = f32::from(f);
                }
                at = at.plus(Len::of(4))?;
            } else {
                if flags.xy_values() {
                    let e: u8 = body.read(at)?;
                    let f: u8 = body.read(at.plus(Len::of(1))?)?;
                    t.e = f32::from(i8::from_ne_bytes([e]));
                    t.f = f32::from(i8::from_ne_bytes([f]));
                }
                at = at.plus(Len::of(2))?;
            }
            if flags.two_by_two() {
                let a: i16 = body.read(at)?;
                let b: i16 = body.read(at.plus(Len::of(2))?)?;
                let c: i16 = body.read(at.plus(Len::of(4))?)?;
                let d: i16 = body.read(at.plus(Len::of(6))?)?;
                t.a = f2dot14(a);
                t.b = f2dot14(b);
                t.c = f2dot14(c);
                t.d = f2dot14(d);
                at = at.plus(Len::of(8))?;
            } else if flags.xy_scale() {
                let a: i16 = body.read(at)?;
                let d: i16 = body.read(at.plus(Len::of(2))?)?;
                t.a = f2dot14(a);
                t.d = f2dot14(d);
                at = at.plus(Len::of(4))?;
            } else if flags.scale() {
                let a: i16 = body.read(at)?;
                t.a = f2dot14(a);
                t.d = t.a;
                at = at.plus(Len::of(2))?;
            }
            if let Some(data) = self.glyph(GlyphId::from_shaper(glyph)) {
                let combined = Transform::combine(transform, t);
                self.outline_into(data, combined, depth + 1, sink)?;
            }
            if !flags.more() {
                return Some(());
            }
        }
    }
}
