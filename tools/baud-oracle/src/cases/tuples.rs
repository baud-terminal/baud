//! **Written here: gvar, packed as the cases fonts carried it.** write-fonts
//! chooses a tuple's point numbers by size (`GlyphDeltas::new`): a sparse
//! tuple that is no smaller written dense becomes dense, its unreferenced
//! points written as the values given for them, and a point set one tuple
//! uses is never shared. The gvar, hvar, ink and vertical cases are there
//! to reach what a sparse tuple leaves to inference -- one referenced
//! point in a contour, two that share a coordinate, phantoms alone -- and
//! the readers of shared and private point numbers, so their gvar is
//! packed here, field for field as fontTools 4.63.0 packed it
//! (`TupleVariation.compile` and `compileTupleVariationStore`,
//! `table__g_v_a_r.compile`):
//!
//! - the shared tuples are the peaks more than one tuple names, most named
//!   first, ties in the order of their bytes;
//! - each glyph shares the point set that saves the most bytes, the first
//!   one when none saves any, so a glyph of one tuple shares its points;
//! - a tuple's points are every point (a count of 0) when none is left
//!   out, else the referenced ones in runs of byte or word deltas, a run
//!   starting a word run only when its first delta does not fit a byte;
//! - deltas are runs of zeros, bytes or words, a byte run ending before
//!   two zeros and a word run before a zero or two values that fit bytes;
//! - each glyph's data is padded to an even length, and the offsets are
//!   short when every one fits.
//!
//! The fields are `[[source: ms-opentype-gvar]]`'s and
//! `[[source: ms-opentype-otvarcommonformats]]`'s.

/// One tuple: a (start, peak, end) region per axis, and per point and
/// phantom its delta, `None` for a point left to inference.
pub struct Tuple {
    pub region: Vec<(f64, f64, f64)>,
    pub deltas: Vec<Option<(i16, i16)>>,
}

const EMBEDDED_PEAK_TUPLE: u16 = 0x8000;
const INTERMEDIATE_REGION: u16 = 0x4000;
const PRIVATE_POINT_NUMBERS: u16 = 0x2000;
const SHARED_POINT_NUMBERS: u16 = 0x8000;

/// fontTools' `fl2fi(v, 14)`: rounded half up.
fn f2dot14(v: f64) -> i16 {
    (v * 16384.0 + 0.5).floor() as i16
}

fn be(out: &mut Vec<u8>, v: i16) {
    out.extend(v.to_be_bytes());
}

impl Tuple {
    fn peak(&self) -> Vec<u8> {
        let mut out = vec![];
        for &(_, p, _) in &self.region {
            be(&mut out, f2dot14(p));
        }
        out
    }

    /// The start and end coordinates, when any differs from the one its
    /// peak implies.
    fn intermediate(&self) -> Option<Vec<u8>> {
        let implied = |&(s, p, e): &(f64, f64, f64)| s == p.min(0.0) && e == p.max(0.0);
        if self.region.iter().all(implied) {
            return None;
        }
        let mut out = vec![];
        for &(s, _, _) in &self.region {
            be(&mut out, f2dot14(s));
        }
        for &(_, _, e) in &self.region {
            be(&mut out, f2dot14(e));
        }
        Some(out)
    }

    /// The referenced points, or `None` for all of them.
    fn used(&self) -> Option<Vec<u16>> {
        if self.deltas.iter().all(Option::is_some) {
            return None;
        }
        Some(
            self.deltas
                .iter()
                .enumerate()
                .filter(|(_, d)| d.is_some())
                .map(|(i, _)| i as u16)
                .collect(),
        )
    }
}

fn points(used: &Option<Vec<u16>>) -> Vec<u8> {
    let Some(points) = used else {
        return vec![0];
    };
    let n = points.len();
    let mut out = vec![];
    if n < 0x80 {
        out.push(n as u8);
    } else {
        out.push((n >> 8) as u8 | 0x80);
        out.push(n as u8);
    }
    let (mut pos, mut last) = (0, 0u16);
    while pos < n {
        let header = out.len();
        out.push(0);
        let mut run = 0;
        let mut bytes = None;
        while pos < n && run <= 127 {
            let delta = points[pos] - last;
            let byte = *bytes.get_or_insert(delta <= 0xFF);
            if byte && delta > 0xFF {
                break;
            }
            if byte {
                out.push(delta as u8);
            } else {
                out.extend(delta.to_be_bytes());
            }
            last = points[pos];
            pos += 1;
            run += 1;
        }
        out[header] = (run - 1) as u8 | if bytes == Some(true) { 0 } else { 0x80 };
    }
    out
}

fn deltas(values: &[i16], out: &mut Vec<u8>) {
    let fits = |v: i16| (-128..=127).contains(&v);
    let n = values.len();
    let mut pos = 0;
    while pos < n {
        let start = pos;
        let (flag, width) = if values[pos] == 0 {
            while pos < n && values[pos] == 0 {
                pos += 1;
            }
            (0x80, 0)
        } else if fits(values[pos]) {
            while pos < n && fits(values[pos]) {
                if values[pos] == 0 && pos + 1 < n && values[pos + 1] == 0 {
                    break;
                }
                pos += 1;
            }
            (0x00, 1)
        } else {
            while pos < n && values[pos] != 0 {
                if fits(values[pos]) && pos + 1 < n && fits(values[pos + 1]) {
                    break;
                }
                pos += 1;
            }
            (0x40, 2)
        };
        for chunk in values[start..pos].chunks(64) {
            out.push(flag | (chunk.len() - 1) as u8);
            for &v in chunk {
                match width {
                    1 => out.push(v as u8),
                    2 => out.extend(v.to_be_bytes()),
                    _ => {}
                }
            }
        }
    }
}

/// One glyph's GlyphVariationData.
fn glyph(tuples: &[Tuple], shared: &[Vec<u8>]) -> Vec<u8> {
    if tuples.is_empty() {
        return vec![];
    }
    // The point set that saves the most bytes, the first among equals.
    let mut sets: Vec<(Option<Vec<u16>>, usize)> = vec![];
    for t in tuples {
        let u = t.used();
        match sets.iter_mut().find(|(s, _)| *s == u) {
            Some((_, n)) => *n += 1,
            None => sets.push((u, 1)),
        }
    }
    let mut best = 0;
    for (i, (s, n)) in sets.iter().enumerate() {
        let saves = |s: &Option<Vec<u16>>, n: usize| points(s).len() * (n - 1);
        if saves(s, *n) > saves(&sets[best].0, sets[best].1) {
            best = i;
        }
    }
    let shared_points = &sets[best].0;
    let (mut headers, mut data) = (vec![], points(shared_points));
    for t in tuples {
        let mut aux = vec![];
        let mut flags = 0;
        let peak = t.peak();
        let embedded = match shared.iter().position(|s| *s == peak) {
            Some(i) => {
                flags |= i as u16;
                None
            }
            None => {
                flags |= EMBEDDED_PEAK_TUPLE;
                Some(peak)
            }
        };
        let intermediate = t.intermediate();
        if intermediate.is_some() {
            flags |= INTERMEDIATE_REGION;
        }
        let used = t.used();
        if used != *shared_points {
            flags |= PRIVATE_POINT_NUMBERS;
            aux.extend(points(&used));
        }
        let present: Vec<(i16, i16)> = t.deltas.iter().flatten().copied().collect();
        deltas(&present.iter().map(|d| d.0).collect::<Vec<_>>(), &mut aux);
        deltas(&present.iter().map(|d| d.1).collect::<Vec<_>>(), &mut aux);
        headers.extend((aux.len() as u16).to_be_bytes());
        headers.extend(flags.to_be_bytes());
        headers.extend(embedded.unwrap_or_default());
        headers.extend(intermediate.unwrap_or_default());
        data.extend(aux);
    }
    let mut out = vec![];
    out.extend((tuples.len() as u16 | SHARED_POINT_NUMBERS).to_be_bytes());
    out.extend((4 + headers.len() as u16).to_be_bytes());
    out.extend(headers);
    out.extend(data);
    if out.len() % 2 != 0 {
        out.push(0);
    }
    out
}

/// A tuple over `region`, (start, peak, end) per axis.
pub fn tuple(region: &[(f64, f64, f64)], deltas: &[Option<(i16, i16)>]) -> Tuple {
    Tuple {
        region: region.to_vec(),
        deltas: deltas.to_vec(),
    }
}

/// gvar over `axes` axes: each named glyph's tuples, none for a glyph not
/// named.
pub fn gvar(order: &[&str], axes: u16, tuples: Vec<(&str, Vec<Tuple>)>) -> Vec<u8> {
    let mut glyphs: Vec<Vec<Tuple>> = order.iter().map(|_| vec![]).collect();
    for (n, t) in tuples {
        glyphs[super::gid(order, n).to_u32() as usize] = t;
    }
    pack(axes, &glyphs)
}

/// gvar over `axes` axes, each glyph's tuples in glyph order.
fn pack(axes: u16, glyphs: &[Vec<Tuple>]) -> Vec<u8> {
    let mut counts: Vec<(Vec<u8>, usize)> = vec![];
    for t in glyphs.iter().flatten() {
        let p = t.peak();
        match counts.iter_mut().find(|(c, _)| *c == p) {
            Some((_, n)) => *n += 1,
            None => counts.push((p, 1)),
        }
    }
    counts.retain(|(_, n)| *n > 1);
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let shared: Vec<Vec<u8>> = counts.into_iter().map(|(c, _)| c).collect();
    let data: Vec<Vec<u8>> = glyphs.iter().map(|g| glyph(g, &shared)).collect();
    let mut offsets = vec![0u32];
    for d in &data {
        offsets.push(offsets.last().expect("an offset") + d.len() as u32);
    }
    let long = *offsets.last().expect("an offset") > 0xFFFF * 2;
    let mut packed = vec![];
    for o in &offsets {
        if long {
            packed.extend(o.to_be_bytes());
        } else {
            packed.extend(((o >> 1) as u16).to_be_bytes());
        }
    }
    let shared_at = 20 + packed.len() as u32;
    let shared_len: usize = shared.iter().map(Vec::len).sum();
    let mut out = vec![];
    out.extend(1u16.to_be_bytes());
    out.extend(0u16.to_be_bytes());
    out.extend(axes.to_be_bytes());
    out.extend((shared.len() as u16).to_be_bytes());
    out.extend(shared_at.to_be_bytes());
    out.extend((glyphs.len() as u16).to_be_bytes());
    out.extend((long as u16).to_be_bytes());
    out.extend((shared_at + shared_len as u32).to_be_bytes());
    out.extend(packed);
    out.extend(shared.concat());
    out.extend(data.concat());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point list and a run of deltas packed as fontTools 4.63.0's
    /// `compilePoints` and `compileDeltaValues_` pack them: these are their
    /// outputs for the same input.
    #[test]
    fn points_and_deltas_pack_as_fonttools_packs_them() {
        assert_eq!(points(&None), [0]);
        assert_eq!(points(&Some(vec![1, 4, 5])), [3, 2, 1, 3, 1]);
        assert_eq!(points(&Some(vec![300])), [1, 0x80, 0x01, 0x2C]);
        let mut d = vec![];
        deltas(&[0, 0, 0, 1, 0, 0, 300, 2, -3], &mut d);
        assert_eq!(
            d,
            [0x82, 0x00, 0x01, 0x81, 0x40, 0x01, 0x2C, 0x01, 0x02, 0xFD]
        );
    }
}
