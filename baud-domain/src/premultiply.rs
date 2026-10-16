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
//! `baud-render`), and one function on both systems is the owner's same
//! pixels on every OS: the Wayland presenter truncated, `C a / 255`, and
//! read one below macOS on 31,770 of the 65,536 `(C, a)` pairs.

use crate::Chan;

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
}
