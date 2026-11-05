//! **Pillow 12.3.0's LANCZOS resize of a mode `F` image**, ported from
//! `src/libImaging/Resample.c`: coefficients in `f64`, normalised per
//! output pixel, a horizontal pass over the rows the vertical pass reads,
//! then the vertical pass, each sum in `f64` and stored as `f32`.
//!
//! **The sums are the compiled loop's, not the C text's.** In the pinned
//! build (aarch64-darwin, nixpkgs `7dd199b0`) clang vectorises the
//! horizontal pass: whole groups of four taps are multiplied, each product
//! rounded, and added in order; the taps past the last group of four, and
//! every tap of a row with fewer than four, are fused multiply-adds
//! (`fmadd`). The vertical pass is never vectorised, and every one of its
//! taps is fused. Read in `_ImagingResampleHorizontal_32bpc` and
//! `_ImagingResampleVertical_32bpc`'s disassembly; [`Pass`] names the two.
//! On noise no choice of fusing moves a pixel, the sum's last bits rarely
//! reaching the `f32` it is stored as; on taps that cancel, only this split
//! gives Pillow's bits (`tests/lanczos.rs`).

/// Lanczos's support, `LANCZOS = {lanczos_filter, 3.0}`.
const SUPPORT: f64 = 3.0;

/// `sinc_filter`: `sin(πx) / (πx)`, 1 at 0.
fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * std::f64::consts::PI;
    x.sin() / x
}

/// `lanczos_filter`: the sinc truncated to [-3, 3).
fn lanczos(x: f64) -> f64 {
    if (-3.0..3.0).contains(&x) {
        sinc(x) * sinc(x / 3.0)
    } else {
        0.0
    }
}

/// `precompute_coeffs`: for each output pixel, its first input pixel, how
/// many it reads, and their weights, `ksize` slots each.
pub struct Coeffs {
    pub ksize: usize,
    pub bounds: Vec<(usize, usize)>,
    pub k: Vec<f64>,
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "C's (int) casts, which truncate toward zero"
)]
pub fn coeffs(in_size: usize, out_size: usize) -> Coeffs {
    // box = (0, 0, in_size, ...): in0 is 0 and in1 the size, as floats.
    let in0 = 0.0_f32;
    let in1 = in_size as f32;
    let scale = f64::from(in1 - in0) / out_size as f64;
    let filterscale = scale.max(1.0);
    let support = SUPPORT * filterscale;
    let ksize = (support.ceil() as usize) * 2 + 1;
    let inv = 1.0 / filterscale;
    let mut bounds = Vec::with_capacity(out_size);
    let mut kk = vec![0.0; out_size * ksize];
    for xx in 0..out_size {
        let center = f64::from(in0) + (xx as f64 + 0.5) * scale;
        let xmin = ((center - support + 0.5) as i64).max(0);
        let xmax = ((center + support + 0.5) as i64).min(in_size as i64) - xmin;
        let k = &mut kk[xx * ksize..(xx + 1) * ksize];
        let mut ww = 0.0;
        for (x, slot) in k.iter_mut().enumerate().take(xmax.max(0) as usize) {
            let w = lanczos(((x as i64 + xmin) as f64 - center + 0.5) * inv);
            *slot = w;
            ww += w;
        }
        if ww != 0.0 {
            for v in k.iter_mut().take(xmax.max(0) as usize) {
                *v /= ww;
            }
        }
        bounds.push((xmin as usize, xmax.max(0) as usize));
    }
    Coeffs {
        ksize,
        bounds,
        k: kk,
    }
}

/// Which compiled loop a sum is.
#[derive(Clone, Copy)]
enum Pass {
    /// Groups of four unfused, the rest fused.
    Horizontal,
    /// Every tap fused.
    Vertical,
}

fn sum(pass: Pass, taps: impl Iterator<Item = (f32, f64)>, n: usize) -> f64 {
    let whole = match pass {
        Pass::Horizontal if n >= 4 => n & !3,
        _ => 0,
    };
    let mut ss = 0.0_f64;
    for (i, (v, k)) in taps.enumerate() {
        let v = f64::from(v);
        ss = if i < whole {
            ss + v * k
        } else {
            v.mul_add(k, ss)
        };
    }
    ss
}

/// An `F` image: `w` by `h` `f32`s, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Float {
    pub w: usize,
    pub h: usize,
    pub px: Vec<f32>,
}

impl Float {
    /// `im.resize((w, h), Image.LANCZOS)`.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the sum is stored as FLOAT32, as the C assigns it"
    )]
    #[must_use]
    pub fn lanczos(&self, w: usize, h: usize) -> Float {
        let need_h = w != self.w;
        let need_v = h != self.h;
        let vert = coeffs(self.h, h);
        let first = vert.bounds[0].0;
        let last = vert.bounds[h - 1].0 + vert.bounds[h - 1].1;
        let mut src = self.clone();
        if need_h {
            let hor = coeffs(self.w, w);
            let rows = last - first;
            let mut px = vec![0.0_f32; w * rows];
            for yy in 0..rows {
                let line = &self.px[(yy + first) * self.w..(yy + first + 1) * self.w];
                for (xx, &(xmin, n)) in hor.bounds.iter().enumerate() {
                    let k = &hor.k[xx * hor.ksize..];
                    let taps = (0..n).map(|x| (line[x + xmin], k[x]));
                    px[yy * w + xx] = sum(Pass::Horizontal, taps, n) as f32;
                }
            }
            src = Float { w, h: rows, px };
        }
        if !need_v {
            return src;
        }
        // The rows the horizontal pass made start at `first`.
        let shift = if need_h { first } else { 0 };
        let mut px = vec![0.0_f32; src.w * h];
        for (yy, &(ymin, n)) in vert.bounds.iter().enumerate() {
            let k = &vert.k[yy * vert.ksize..];
            for xx in 0..src.w {
                let taps = (0..n).map(|y| (src.px[(y + ymin - shift) * src.w + xx], k[y]));
                px[yy * src.w + xx] = sum(Pass::Vertical, taps, n) as f32;
            }
        }
        Float { w: src.w, h, px }
    }
}
