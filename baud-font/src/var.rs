//! Font variations: the axes a variable font offers (`fvar`), the mapping
//! that bends them (`avar`), and the normalised coordinates a face is read
//! at. Note Z scopes the work; this module is its first step.
//!
//! `[[source: ms-opentype-otvaroverview]]` fixes the normalisation to the
//! bit: the user value in 16.16, clamped to the axis, mapped to -1..1 around
//! the default, clamped, bent by `avar`, clamped again, and converted to 2.14
//! by adding 2 and shifting right by 2 (`[[source: ms-opentype-fvar]]`,
//! `[[source: ms-opentype-avar]]`).

use crate::Tag;
use crate::bytes::{Bytes, Count, FromBe, Index, Len, Offset};

/// A 16.16 fixed-point value: how `fvar` stores an axis's range, and the
/// precision the normalisation is done in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Fixed(pub i32);

impl Fixed {
    pub const ZERO: Fixed = Fixed(0);
    pub const ONE: Fixed = Fixed(0x1_0000);

    /// A design value from a float, as the specification converts one:
    /// scaled by 65536 and rounded half up.
    pub fn from_f32(v: f32) -> Fixed {
        Fixed(core::f32::math::floor(v * 65536.0 + 0.5) as i32)
    }
}

impl FromBe for Fixed {
    const SIZE: Len = Len::of(4);
    fn parse_be(b: &[u8]) -> Option<Self> {
        b.get(..4)?
            .try_into()
            .ok()
            .map(|b| Fixed(i32::from_be_bytes(b)))
    }
}

/// A normalised coordinate in 2.14: -1 at an axis's minimum, 0 at its
/// default, 1 at its maximum.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct F2Dot14(pub i16);

impl F2Dot14 {
    pub const ZERO: F2Dot14 = F2Dot14(0);
}

impl FromBe for F2Dot14 {
    const SIZE: Len = Len::of(2);
    fn parse_be(b: &[u8]) -> Option<Self> {
        b.get(..2)?
            .try_into()
            .ok()
            .map(|b| F2Dot14(i16::from_be_bytes(b)))
    }
}

/// One variation axis, in the font's own user units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Axis {
    pub tag: Tag,
    pub min: Fixed,
    pub default: Fixed,
    pub max: Fixed,
}

/// A request to read a face at `value` on the axis `tag`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Variation {
    pub tag: Tag,
    pub value: Fixed,
}

/// The most axes a face's coordinates hold. A font with more reads the
/// ones past this at their default -- the axes registered and in use by
/// monospace fonts are a handful, and Roboto Flex, the largest in common
/// use, has thirteen. The bound is what keeps a `Face` free of allocation.
pub const MAX_AXES: usize = 16;

/// A face's normalised coordinates, one per axis, zero where not set.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Coords {
    axes: [F2Dot14; MAX_AXES],
}

impl Coords {
    /// The coordinate of axis `i`, zero past the bound.
    pub fn get(&self, i: Index) -> F2Dot14 {
        self.axes.get(i.slot()).copied().unwrap_or(F2Dot14::ZERO)
    }

    /// Whether every coordinate is the default: then no delta applies.
    pub fn is_default(&self) -> bool {
        self.axes.iter().all(|c| *c == F2Dot14::ZERO)
    }

    /// Sets axis `i` directly, for tests that build a position by hand.
    #[cfg(test)]
    pub(crate) fn set_for_test(&mut self, i: Index, c: F2Dot14) {
        self.set(i, c);
    }

    fn set(&mut self, i: Index, c: F2Dot14) {
        if let Some(slot) = self.axes.get_mut(i.slot()) {
            *slot = c;
        }
    }
}

/// `fvar`: the axes, in the order every other variation table indexes them.
#[derive(Clone, Copy)]
pub struct Fvar<'a> {
    axes: Bytes<'a>,
    count: Count,
    size: Len,
}

impl<'a> Fvar<'a> {
    /// `majorVersion` 1; `axesArrayOffset` at 4, `axisCount` at 8,
    /// `axisSize` at 10 (20 in version 1.0, larger sizes read their first
    /// twenty bytes).
    pub fn parse(t: Bytes<'a>) -> Option<Fvar<'a>> {
        let major: u16 = t.read(Offset::ZERO)?;
        if major != 1 {
            return None;
        }
        let at: u16 = t.read(Offset::of(4u32))?;
        let count: u16 = t.read(Offset::of(8u32))?;
        let size: u16 = t.read(Offset::of(10u32))?;
        if size < 20 {
            return None;
        }
        let count = Count::of(count);
        let size = Len::of(u32::from(size));
        let axes = t.slice(Offset::of(u32::from(at)), size.times(count)?)?;
        Some(Fvar { axes, count, size })
    }

    pub fn count(&self) -> Count {
        self.count
    }

    /// Axis `i`: tag, then min, default and max as 16.16.
    pub fn axis(&self, i: Index) -> Option<Axis> {
        if !self.count.holds(i) {
            return None;
        }
        let at = Offset::ZERO.at(i, self.size)?;
        let tag: u32 = self.axes.read(at)?;
        let field = |n: u32| at.plus(Len::of(n)).and_then(|o| self.axes.read::<Fixed>(o));
        let (min, default, max) = (field(4)?, field(8)?, field(12)?);
        // The specification requires min <= default <= max; an axis that
        // breaks it cannot be normalised and is not offered.
        (min <= default && default <= max).then_some(Axis {
            tag: Tag::from_bytes(tag.to_be_bytes()),
            min,
            default,
            max,
        })
    }
}

/// `avar` version 1: one segment map per axis. Version 2's additions are
/// not read (note Z's not-built rows); its version-1 part is.
#[derive(Clone, Copy)]
pub struct Avar<'a> {
    maps: Bytes<'a>,
    count: Count,
}

impl<'a> Avar<'a> {
    /// `majorVersion` 1 or 2, `axisCount` at 6, the segment maps from 8.
    pub fn parse(t: Bytes<'a>) -> Option<Avar<'a>> {
        let major: u16 = t.read(Offset::ZERO)?;
        if major != 1 && major != 2 {
            return None;
        }
        let count: u16 = t.read(Offset::of(6u32))?;
        let maps = t.slice(Offset::of(8u32), Len::of(t.len().value().checked_sub(8)?))?;
        Some(Avar {
            maps,
            count: Count::of(count),
        })
    }

    /// Axis `axis`'s segment map, as the bytes of its `AxisValueMap`
    /// records: four bytes each, from and to in 2.14.
    fn map(&self, axis: Index) -> Option<(Bytes<'a>, Count)> {
        if !self.count.holds(axis) {
            return None;
        }
        let mut at = Offset::ZERO;
        let mut i = Index::ZERO;
        loop {
            let n: u16 = self.maps.read(at)?;
            let n = Count::of(n);
            let records = at.plus(Len::of(2))?;
            if i == axis {
                return Some((self.maps.slice(records, Len::of(4).times(n)?)?, n));
            }
            at = records.plus(Len::of(4).times(n)?)?;
            i = i.next();
        }
    }
}

/// Axis `axis`'s value `value` in normalised 2.14, by the steps of
/// `[[source: ms-opentype-otvaroverview]]` "Coordinate Scales and
/// Normalization", in 16.16 throughout: clamp to the axis; map each side of
/// the default linearly to -1..0 and 0..1; clamp to -1..1; bend through
/// `avar`'s segment map and clamp again; then "add 0x00000002, and
/// sign-extend shift to the right by 2".
pub fn normalize(axis: Axis, index: Index, avar: Option<Avar<'_>>, value: Fixed) -> F2Dot14 {
    let v = i64::from(value.0.clamp(axis.min.0, axis.max.0));
    let (min, def, max) = (
        i64::from(axis.min.0),
        i64::from(axis.default.0),
        i64::from(axis.max.0),
    );
    let one = i64::from(Fixed::ONE.0);
    // The two divisions are in 16.16: the numerator shifted up by 16 so the
    // quotient keeps its sixteen fractional bits, rounded to the nearest
    // 16.16 value -- the one step the specification leaves open, which
    // HarfBuzz takes as `roundf` of the quotient
    // (`[[source: harfbuzz-12-3-0-ot-var]]`, `[[source: harfbuzz-12-3-0-fvar]]`;
    // Jev, 0.99), and HarfBuzz's `roundf` is its own `floorf (x + .5f)`
    // (`[[source: harfbuzz-12-3-0-hb]]`): a half rounds up, -2.5 to -2.
    // Truncating instead differs from HarfBuzz by one 2.14 unit in 1,482 of
    // the 12,003 fixture values. A side of zero width is only reached at the
    // default itself, which is exactly 0.
    let n = if v < def {
        div_round(Wide((v - def) * one), Wide(def - min)).0
    } else if v > def {
        div_round(Wide((v - def) * one), Wide(max - def)).0
    } else {
        0
    }
    // The specification's clamp. In exact integer arithmetic over a value
    // already clamped to the axis the quotient cannot leave -1..1, so no
    // test reaches it; it is kept because the specification names it and
    // its float route can overshoot.
    .clamp(-one, one);
    let n = match avar.and_then(|a| a.map(index)) {
        Some((map, count)) => bend(map, count, Wide(n)).0.clamp(-one, one),
        None => n,
    };
    // `n` is within -1..1 in 16.16, so its 2.14 fits an i16.
    F2Dot14(i16::try_from((n + 2) >> 2).unwrap_or(0))
}

/// `a / b` rounded to the nearest integer, a half up -- HarfBuzz's
/// `floorf (x + .5f)` on the exact quotient -- for `b > 0`: the floor of
/// `(2a + b) / 2b`.
fn div_round(a: Wide, b: Wide) -> Wide {
    Wide((2 * a.0 + b.0).div_euclid(2 * b.0))
}

/// A 16.16 value, or a product of two, in 64 bits: the normalisation's
/// intermediates, wide enough that no product overflows.
#[derive(Clone, Copy)]
struct Wide(i64);

/// `n` (16.16) through one axis's `AxisValueMap` records, each a from and a
/// to in 2.14: the first record whose from is at or past `n` is the end of
/// the segment; equal, its to is the answer; otherwise the answer lies on
/// the line to it from the record before. A map with no such segment --
/// fewer records than the three the specification requires, or not ending
/// at 1 -- leaves `n` as it is.
fn bend(map: Bytes<'_>, count: Count, n: Wide) -> Wide {
    let n = n.0;
    let at = |i: Index| -> Option<(i64, i64)> {
        let o = Offset::ZERO.at(i, Len::of(4))?;
        let from: F2Dot14 = map.read(o)?;
        let to: F2Dot14 = map.read(o.plus(Len::of(2))?)?;
        // 2.14 to 16.16: two more fractional bits.
        Some((i64::from(from.0) << 2, i64::from(to.0) << 2))
    };
    let mut i = Index::ZERO;
    while count.holds(i) {
        let Some((from, to)) = at(i) else {
            return Wide(n);
        };
        if from >= n {
            if from == n {
                return Wide(to);
            }
            let Some((sf, st)) = i.prev().and_then(at) else {
                return Wide(n);
            };
            // The point on the segment, rounded once to the nearest 16.16
            // value, as HarfBuzz rounds the mapped coordinate
            // (`[[source: harfbuzz-12-3-0-avar]]`, its 16.16 coordinate mapping).
            return Wide(st + div_round(Wide((n - sf) * (to - st)), Wide(from - sf)).0);
        }
        i = i.next();
    }
    Wide(n)
}

/// Sets `coords` for `settings` over the axes of `fvar`, normalised through
/// `avar`. A tag the font has no axis for is ignored; an axis set twice
/// takes the last value.
pub fn apply(fvar: Fvar<'_>, avar: Option<Avar<'_>>, settings: &[Variation], coords: &mut Coords) {
    let mut i = Index::ZERO;
    while fvar.count().holds(i) {
        if let Some(axis) = fvar.axis(i) {
            if let Some(v) = settings.iter().rev().find(|v| v.tag == axis.tag) {
                coords.set(i, normalize(axis, i, avar, v.value));
            }
        }
        i = i.next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A negative half, which none of the 12,003 fixture values reaches:
    /// an axis 0 / 2 / 3 at `2 - k / 65536` is `-k / 2` in 16.16. HarfBuzz
    /// 12.3.0 answers 0 at k = 5 (-2.5 rounds up to -2, then to 0 in 2.14)
    /// and -1 at k = 6 (`scripts/var-oracle.py` does not reach it; asked of
    /// uharfbuzz 0.53.2 directly). Rounding halves away from zero, as the
    /// first version of step 1 did, gives -1 at k = 5.
    #[test]
    fn a_negative_half_rounds_up_as_harfbuzz_rounds_it() {
        let axis = Axis {
            tag: Tag::from_bytes(*b"TEST"),
            min: Fixed(0),
            default: Fixed(2 << 16),
            max: Fixed(3 << 16),
        };
        for (k, want) in [(1, 0), (2, 0), (3, 0), (5, 0), (6, -1), (10, -1)] {
            let got = normalize(axis, Index::ZERO, None, Fixed((2 << 16) - k));
            assert_eq!(got, F2Dot14(want), "k = {k}");
        }
    }

    /// An `avar` of one axis whose map is `records`, each a from and a to in
    /// 2.14.
    fn avar(records: &[(i16, i16)]) -> [u8; 64] {
        let mut b = [0u8; 64];
        b[0..2].copy_from_slice(&1u16.to_be_bytes());
        b[6..8].copy_from_slice(&1u16.to_be_bytes());
        b[8..10].copy_from_slice(&u16::try_from(records.len()).unwrap().to_be_bytes());
        for (i, (from, to)) in records.iter().enumerate() {
            b[10 + 4 * i..12 + 4 * i].copy_from_slice(&from.to_be_bytes());
            b[12 + 4 * i..14 + 4 * i].copy_from_slice(&to.to_be_bytes());
        }
        b
    }

    const AXIS: Axis = Axis {
        tag: Tag::from_bytes(*b"wght"),
        min: Fixed(100 << 16),
        default: Fixed(400 << 16),
        max: Fixed(800 << 16),
    };

    fn at(bytes: &[u8], v: Fixed) -> F2Dot14 {
        let a = Avar::parse(Bytes::new(bytes)).unwrap();
        normalize(AXIS, Index::ZERO, Some(a), v)
    }

    /// A map whose target lies past 1 is clamped back to 1, as the
    /// specification requires after `avar`; a well-formed font never
    /// reaches this, which is why it is built by hand.
    #[test]
    fn a_target_past_one_is_clamped() {
        let b = avar(&[(-16384, -16384), (0, 0), (8192, 24576), (16384, 16384)]);
        // 600 is 0.5 of the way to 800: exactly the record mapped to 1.5.
        assert_eq!(at(&b, Fixed(600 << 16)), F2Dot14(16384));
        assert_eq!(
            at(&b, Fixed(800 << 16)),
            F2Dot14(16384),
            "the control: 1 maps to 1"
        );
    }

    /// A value equal to a record's from takes its to, even the first
    /// record's, which has no segment before it.
    #[test]
    fn a_value_on_a_records_from_takes_its_to() {
        let b = avar(&[(-16384, -8192), (0, 0), (16384, 16384)]);
        assert_eq!(at(&b, Fixed(100 << 16)), F2Dot14(-8192));
        assert_eq!(
            at(&b, Fixed(250 << 16)),
            F2Dot14(-4096),
            "halfway along the first segment"
        );
    }

    /// A map with no segment -- fewer records than a segment needs, or
    /// none reaching the value -- leaves the value as it is.
    #[test]
    fn a_map_with_no_segment_leaves_the_value() {
        let b = avar(&[(0, 0)]);
        assert_eq!(at(&b, Fixed(800 << 16)), F2Dot14(16384));
        assert_eq!(at(&b, Fixed(100 << 16)), F2Dot14(-16384));
        let b = avar(&[(-16384, -16384), (0, 0)]);
        assert_eq!(
            at(&b, Fixed(600 << 16)),
            F2Dot14(8192),
            "past the last record"
        );
    }
}
