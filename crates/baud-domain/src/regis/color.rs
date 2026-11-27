//! The output map: what each index a page holds is shown as.

use crate::image::Pixel;
use crate::style::Chan;

/// How many indices a page holds: the VT340's "16 output map locations"
/// on "a 4-plane bitmap" (`[[source: vt3xx-screen-control]]`).
pub const REGIS_COLORS: usize = 16;

/// The VT340's default color map, `[[source: vt3xx-screen-control]]` Table 2-3, in RGB
/// percent: black, blue, red, green, magenta, cyan, yellow, gray 50%, gray
/// 25%, and the less saturated blue to yellow, and gray 75%.
const DEFAULT_PCT: [(u16, u16, u16); REGIS_COLORS] = [
    (0, 0, 0),
    (20, 20, 80),
    (80, 13, 13),
    (20, 80, 20),
    (80, 20, 80),
    (20, 80, 80),
    (80, 80, 20),
    (53, 53, 53),
    (26, 26, 26),
    (33, 33, 60),
    (60, 26, 26),
    (33, 60, 33),
    (60, 33, 60),
    (33, 60, 60),
    (60, 60, 33),
    (80, 80, 80),
];

/// The map at power-up, as a constant.
pub const REGIS_DEFAULT_MAP: [Pixel; REGIS_COLORS] = {
    let mut m = [Pixel::CLEAR; REGIS_COLORS];
    let mut i = 0;
    while i < REGIS_COLORS {
        let (r, g, b) = DEFAULT_PCT[i];
        m[i] = Pixel::opaque_bytes(pct(r), pct(g), pct(b));
        i += 1;
    }
    m
};

const fn pct(v: u16) -> u8 {
    let v = if v > 100 { 100 } else { v };
    ((v * 255 + 50) / 100) as u8
}

/// One part of an HLS specifier: missing, named without a number, or
/// given one -- § 8.6.2.2.2's defaults tell "specified" from "quantified".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comp {
    Absent,
    Named,
    Value(i32),
}

impl Comp {
    fn present(self) -> bool {
        !matches!(self, Comp::Absent)
    }

    fn value(self) -> Option<i32> {
        match self {
            Comp::Value(v) => Some(v),
            _ => None,
        }
    }
}

/// A colour named in a ReGIS colour specifier (§ 8.6.2.2.2): one of the
/// eight RGB letters, or a hue, lightness and saturation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisColor {
    /// "R, G, B, Y, C, M, W, or D for red, green, blue, yellow, cyan,
    /// magenta, white, or dark".
    Letter(u8),
    /// HLS, each part missing, named or quantified.
    Hls { h: Comp, l: Comp, s: Comp },
}

impl RegisColor {
    /// The colour, or `None` where the text takes "no action": an HLS with
    /// none of the three (§ 8.6.2.2.2 d) and a letter outside the alphabet.
    ///
    /// The HLS defaults are § 8.6.2.2.2's: "If H or S is specified, but L is
    /// not specified or not quantified, L defaults to 50. If H is specified
    /// and quantified, and S is not specified or not quantified, S defaults
    /// to 100. If H is not specified or not quantified, S defaults to 0; H
    /// has no default" -- a hue with none is drawn at 0. The hue origin is
    /// DEC's, blue at 0, red at 120, green at 240, which the text and the
    /// Sixel decoder share.
    pub fn pixel(self) -> Option<Pixel> {
        match self {
            RegisColor::Letter(c) => {
                let (r, g, b) = match c {
                    b'D' => (0, 0, 0),
                    b'R' => (100, 0, 0),
                    b'G' => (0, 100, 0),
                    b'B' => (0, 0, 100),
                    b'C' => (0, 100, 100),
                    b'Y' => (100, 100, 0),
                    b'M' => (100, 0, 100),
                    b'W' => (100, 100, 100),
                    _ => return None,
                };
                Some(crate::sixel::rgb(r, g, b))
            }
            RegisColor::Hls { h, l, s } => {
                if !h.present() && !l.present() && !s.present() {
                    return None;
                }
                let l = l.value().unwrap_or(50);
                let s = match (h, s.value()) {
                    (_, Some(s)) => s,
                    (Comp::Value(_), None) => 100,
                    _ => 0,
                };
                let h = h.value().unwrap_or(0).rem_euclid(360);
                let clamp = |v: i32, hi: i32| v.clamp(0, hi) as u16;
                crate::sixel::hls(clamp(h, 360), clamp(l, 100), clamp(s, 100))
            }
        }
    }
}

/// The map index whose colour is closest to `p`: § 8.6.2.2.1, "the color
/// map ... should be searched to find the closest available color", by
/// distance in RGB, the lowest index on a tie.
pub fn regis_closest(map: &[Pixel; REGIS_COLORS], p: Pixel) -> super::RegisIndex {
    let (r, g, b) = p.rgb();
    let mut best = (u32::MAX, 0usize);
    for (i, m) in map.iter().enumerate() {
        let (mr, mg, mb) = m.rgb();
        let d = |a: Chan, b: Chan| {
            let x = i32::from(a.get()) - i32::from(b.get());
            (x * x) as u32
        };
        let dist = d(r, mr) + d(g, mg) + d(b, mb);
        if dist < best.0 {
            best = (dist, i);
        }
    }
    super::RegisIndex::wrap(best.1 as i64)
}
