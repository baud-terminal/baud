//! Straight alpha to premultiplied, the one conversion every presenter
//! makes at its edge: the renderer's frame holds `0xAARRGGBB` straight, and
//! both surfaces a presenter hands it to read premultiplied pixels -- a
//! `wl_shm` buffer, "for all wl_shm formats ... pre-multiplied alpha is used
//! for pixel values" (`[[source: wayland]]`, `wl_shm.format`), and Core
//! Animation's `IOSurface`.
//!
//! **Each channel rounds to the nearest byte, `(C a + 127) / 255`.** The
//! protocol names the encoding and not the rounding. The rounding is what
//! the GPU's 8-bit premultiplied store holds, which the CPU compositor's
//! glass blend is built to give back exactly (`Blend::over_glass` in
//! `baud-render`), and one function on both systems gives the same pixels
//! on every OS.

use crate::Chan;
use core::simd::Simd;

/// The words one vector step of [`premultiply_row`] converts.
pub const ROW_LANES: usize = 8;

/// `c` scaled by the alpha `a`, rounded to the nearest byte. `c a` is never
/// a half away from a multiple of 255, so the nearest is unique.
#[inline]
pub fn premultiply_chan(c: Chan, a: Chan) -> Chan {
    let v = scale(u32::from(c.get()), u32::from(a.get()));
    Chan::from_u8(u8::try_from(v).unwrap_or(u8::MAX))
}

/// A straight `0xAARRGGBB` word premultiplied: each colour channel scaled
/// as [`premultiply_chan`] scales it, the alpha kept. An opaque word is
/// itself. Spelled in shifts on the word, not in bytes: per glass pixel
/// per frame it is the truncating form's instructions plus the rounding.
#[inline]
pub fn premultiply(word: u32) -> u32 {
    let a = word >> 24;
    if a == 0xFF {
        return word;
    }
    let ch = |shift: u32| scale((word >> shift) & 0xFF, a) << shift;
    (a << 24) | ch(16) | ch(8) | ch(0)
}

/// **`src` premultiplied into `dst`**, word for word [`premultiply`]'s, over
/// the shorter of the two: the conversion a presenter makes of each row it
/// copies. [`ROW_LANES`] words a step, red with blue in one lane's two
/// halves and green alone, each product rounded as `(t + (t >> 8)) >> 8`
/// of `t = C a + 128`, which is the nearest byte for every `(C, a)`
/// (`a_row_is_each_word_premultiplied`); the rest one word at a time. In
/// sixteen-bit halves of 32-bit lanes it needs no feature past the
/// target's baseline.
pub fn premultiply_row(src: &[u32], dst: &mut [u32]) {
    type V = Simd<u32, ROW_LANES>;
    let n = src.len().min(dst.len());
    let (src, dst) = (&src[..n], &mut dst[..n]);
    let (lo, half) = (V::splat(0x00FF_00FF), V::splat(0x0080_0080));
    let round = |t: V| ((t + ((t >> 8) & lo)) >> 8) & lo;
    let (dv, dt) = dst.as_chunks_mut::<ROW_LANES>();
    let (sv, st) = src.as_chunks::<ROW_LANES>();
    for (d, s) in dv.iter_mut().zip(sv) {
        let w = V::from_array(*s);
        let a = w >> 24;
        let rb = round((w & lo) * a + half);
        let g = round(((w >> 8) & V::splat(0xFF)) * a + half);
        *d = (a << 24 | rb | g << 8).to_array();
    }
    for (d, &w) in dt.iter_mut().zip(st) {
        *d = premultiply(w);
    }
}

/// `c a / 255` rounded, for `c` and `a` at most 255: at most 255 itself.
///
/// **The division is spelled as the multiply LLVM makes of it**, `n *
/// 0x8081 >> 23`, which is `n / 255` for every `n` below `2^16`
/// (`the_reciprocal_is_the_division`). Written `/ 255`, LLVM narrows the
/// numerator to sixteen bits and masks each channel before the multiply.
/// The truncating form it replaced had no such mask.
#[inline]
const fn scale(c: u32, a: u32) -> u32 {
    ((c * a + 127) * 0x8081) >> 23
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};

    /// **The reciprocal is the division** over every numerator `scale`
    /// can form, `255 * 255 + 127` at most, and on to `2^16`.
    #[test]
    fn the_reciprocal_is_the_division() {
        for n in 0..(1u32 << 16) {
            assert_eq!((n * 0x8081) >> 23, n / 255, "{n}");
        }
    }

    /// **Every `(C, a)` pair is the nearest byte**, in each of R, G and B
    /// with other values beside it, the alpha passed through: `255 r` is
    /// within 127 of `c a`, which no other byte is, and `premultiply_chan`
    /// gives the same byte.
    #[test]
    fn every_pair_rounds_to_the_nearest() {
        let mut wrong = 0u32;
        for a in 0..=255u32 {
            for c in 0..=255u32 {
                let byte = |v: u32| Chan::from_u8(u8::try_from(v).unwrap());
                let one = u32::from(premultiply_chan(byte(c), byte(a)).get());
                let ok = [16u32, 8, 0].iter().all(|&shift| {
                    let rest = 0x5a_c3_96 & !(0xff << shift);
                    let got = premultiply((a << 24) | rest | (c << shift));
                    let ch = (got >> shift) & 0xff;
                    got >> 24 == a && (255 * ch).abs_diff(c * a) <= 127 && ch == one
                });
                if !ok {
                    wrong += 1;
                }
            }
        }
        assert_eq!(wrong, 0, "(C, a) pairs off the nearest, of 65536");
    }

    /// **A row is each word premultiplied**: [`premultiply_row`] gives
    /// [`premultiply`]'s word for every `(C, a)` pair in each of R, G and
    /// B, and for every length up to three vectors from every start, so the
    /// vector body and the tail both meet them; over the shorter slice when
    /// the two differ, the rest of `dst` untouched.
    #[test]
    fn a_row_is_each_word_premultiplied() {
        let mut words = Vec::new();
        for a in 0..=255u32 {
            for c in 0..=255u32 {
                for shift in [16u32, 8, 0] {
                    let rest = 0x5a_c3_96 & !(0xff << shift);
                    words.push((a << 24) | rest | (c << shift));
                }
            }
        }
        let want: Vec<u32> = words.iter().map(|&w| premultiply(w)).collect();
        let mut got = vec![0u32; words.len()];
        premultiply_row(&words, &mut got);
        let wrong = got.iter().zip(&want).filter(|(g, w)| g != w).count();
        assert_eq!(wrong, 0, "words off premultiply, of {}", words.len());
        for start in 0..ROW_LANES {
            for len in 0..=3 * ROW_LANES {
                let at = start * 977;
                let mut dst = vec![0u32; len];
                premultiply_row(&words[at..at + len], &mut dst);
                assert_eq!(dst, want[at..at + len], "from {start}, {len} words");
                let mut long = vec![7u32; len + ROW_LANES + 1];
                premultiply_row(&words[at..at + len], &mut long);
                assert_eq!(long[..len], want[at..at + len], "dst longer, {len}");
                assert!(long[len..].iter().all(|&w| w == 7), "past src, {len}");
            }
        }
    }
}
