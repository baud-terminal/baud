//! **The lockup's geometry, read off the renderer's own drawing of it.**
//!
//! `baud-pictures geometry <walls.png> <out dir>`
//!
//! `walls.png` is the logo film's canvas of the held lockup on the dark
//! half, 840 by 580, its block shown (`baud-app/src/logo_film.rs`):
//! `assets/readme/logo-walls.png`, the drawing the plasma's path was
//! chosen against. Writes into `out dir`, each 840 by 580 row by row:
//!
//! - `sdf.f32`: the signed distance to the glyphs' outline in canvas
//!   pixels, positive outside the ink and negative inside, zero on the
//!   contour where the coverage is a half, sub-pixel by the coverage
//!   within a pixel of it; little-endian `f32`;
//! - `nx.f32`, `ny.f32`: its gradient normalised, the outline's outward
//!   normal;
//! - `holes.u8`: 1 inside a closed counter -- the open pixels no path from
//!   the canvas's border reaches, the bowls of b, a and d -- and 0
//!   elsewhere.
//!
//! Each pixel's coverage is solved along the line from the page to the ink
//! it is nearest, the word's or the amber's: the renderer draws each glyph
//! source-over in its one colour on the page. Canvas pixels are lockup
//! units moved by (60, 75) and doubled, as the film draws it. Every step
//! is exact arithmetic or a correctly rounded square root, so the files
//! are the same bits on every platform. Ported from the NumPy program
//! that wrote them, operation for operation.

use crate::numpy;
use std::collections::VecDeque;
use std::path::Path;

/// The canvas.
pub const W: usize = 840;
/// The canvas.
pub const H: usize = 580;
const PAGE: [f64; 3] = [13.0, 17.0, 23.0];
const INKS: [[f64; 3]; 2] = [[243.0, 244.0, 246.0], [244.0, 163.0, 44.0]];

/// What the plasma reads: each field 840 by 580, row by row.
pub struct Fields {
    /// The signed distance, canvas pixels.
    pub sdf: Vec<f64>,
    /// The outward normal's x.
    pub nx: Vec<f64>,
    /// The outward normal's y.
    pub ny: Vec<f64>,
    /// 1 inside a closed counter.
    pub holes: Vec<u8>,
}

/// Each pixel's coverage, 0 to 1, along its nearer ink's line; `img` is
/// RGB, row by row.
fn coverage(img: &[u8]) -> Vec<f64> {
    let n = W * H;
    let mut best_c = vec![0.0; n];
    let mut best_e = vec![0.0; n];
    for (which, ink) in INKS.iter().enumerate() {
        let d = [ink[0] - PAGE[0], ink[1] - PAGE[1], ink[2] - PAGE[2]];
        let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        for i in 0..n {
            let px = [
                f64::from(img[3 * i]),
                f64::from(img[3 * i + 1]),
                f64::from(img[3 * i + 2]),
            ];
            let q = [px[0] - PAGE[0], px[1] - PAGE[1], px[2] - PAGE[2]];
            let c = numpy::clip((q[0] * d[0] + q[1] * d[1] + q[2] * d[2]) / dd, 0.0, 1.0);
            let mut e = (px[0] - (PAGE[0] + c * d[0])).abs();
            for k in 1..3 {
                let ek = (px[k] - (PAGE[k] + c * d[k])).abs();
                e = numpy::maximum(e, ek);
            }
            // The word's ink wins a tie, as it is tried first.
            if which == 0 || e < best_e[i] {
                best_c[i] = c;
                best_e[i] = e;
            }
        }
    }
    best_c
}

/// The 4-connected components of `mask`: each pixel's label, 0 outside.
fn components(mask: &[bool]) -> (Vec<u32>, u32) {
    let mut lab = vec![0u32; W * H];
    let mut n = 0;
    for start in 0..W * H {
        if !mask[start] || lab[start] != 0 {
            continue;
        }
        n += 1;
        lab[start] = n;
        let mut q = VecDeque::from([start]);
        while let Some(p) = q.pop_front() {
            let (y, x) = (p / W, p % W);
            let mut try_at = |yy: usize, xx: usize| {
                let i = yy * W + xx;
                if mask[i] && lab[i] == 0 {
                    lab[i] = n;
                    q.push_back(i);
                }
            };
            if y + 1 < H {
                try_at(y + 1, x);
            }
            if y > 0 {
                try_at(y - 1, x);
            }
            if x + 1 < W {
                try_at(y, x + 1);
            }
            if x > 0 {
                try_at(y, x - 1);
            }
        }
    }
    (lab, n)
}

/// Felzenszwalb and Huttenlocher's lower envelope of parabolas over one
/// line of `f`, the squared distance.
#[allow(clippy::cast_precision_loss, reason = "indices under 1000")]
fn d1(f: &[f64]) -> Vec<f64> {
    const INF: f64 = 1e12;
    let n = f.len();
    let mut d = vec![0.0; n];
    let mut v = vec![0usize; n];
    let mut z = vec![0.0; n + 1];
    let mut k = 0usize;
    z[0] = -INF;
    z[1] = INF;
    // Python's ints, each exact as the float it meets.
    let s_at = |q: usize, vk: usize| {
        ((f[q] + (q * q) as f64) - (f[vk] + (vk * vk) as f64)) / (2 * q - 2 * vk) as f64
    };
    for q in 1..n {
        let mut s = s_at(q, v[k]);
        while s <= z[k] {
            k = k.checked_sub(1).expect("the envelope never empties");
            s = s_at(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = INF;
    }
    k = 0;
    for (q, dq) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let r = q.abs_diff(v[k]);
        *dq = (r * r) as f64 + f[v[k]];
    }
    d
}

/// The exact Euclidean distance from every pixel to the nearest pixel not
/// in `inside` (0 outside it), down the columns and then along the rows.
fn edt(inside: &[bool]) -> Vec<f64> {
    let f: Vec<f64> = inside.iter().map(|&i| if i { 1e12 } else { 0.0 }).collect();
    let mut g = vec![0.0; W * H];
    for x in 0..W {
        let col: Vec<f64> = (0..H).map(|y| f[y * W + x]).collect();
        for (y, v) in d1(&col).into_iter().enumerate() {
            g[y * W + x] = v;
        }
    }
    let mut out = vec![0.0; W * H];
    for y in 0..H {
        let row = d1(&g[y * W..(y + 1) * W]);
        out[y * W..(y + 1) * W].copy_from_slice(&row);
    }
    out.iter().map(|v| v.sqrt()).collect()
}

/// The signed distance, the normal's two components and the holes, from
/// the RGB canvas.
#[must_use]
pub fn fields(img: &[u8]) -> Fields {
    assert_eq!(img.len(), W * H * 3, "an 840 by 580 RGB canvas");
    let cov = coverage(img);
    let ink: Vec<bool> = cov.iter().map(|&c| c >= 0.5).collect();
    let open: Vec<bool> = ink.iter().map(|&i| !i).collect();
    let (olab, on) = components(&open);
    let mut border = vec![false; on as usize + 1];
    for x in 0..W {
        border[olab[x] as usize] = true;
        border[olab[(H - 1) * W + x] as usize] = true;
    }
    for y in 0..H {
        border[olab[y * W] as usize] = true;
        border[olab[y * W + W - 1] as usize] = true;
    }
    let holes: Vec<u8> = olab
        .iter()
        .map(|&l| u8::from(l != 0 && !border[l as usize]))
        .collect();
    let (din, dout) = (edt(&ink), edt(&open));
    let sdf: Vec<f64> = (0..W * H)
        .map(|i| {
            let s = if ink[i] {
                -(din[i] - 0.5)
            } else {
                dout[i] - 0.5
            };
            // Within a pixel of the contour the coverage says where it is.
            if s.abs() <= 1.0 { 0.5 - cov[i] } else { s }
        })
        .collect();
    let (gy, gx) = numpy::gradient(&sdf, H, W);
    let mut nx = vec![0.0; W * H];
    let mut ny = vec![0.0; W * H];
    for i in 0..W * H {
        let gn = (gx[i] * gx[i] + gy[i] * gy[i]).sqrt() + 1e-9;
        nx[i] = gx[i] / gn;
        ny[i] = gy[i] / gn;
    }
    Fields { sdf, nx, ny, holes }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "`astype('<f4')` rounds to nearest"
)]
fn f32_bytes(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|&x| (x as f32).to_le_bytes()).collect()
}

/// The four files' bytes, named.
#[must_use]
pub fn files(f: &Fields) -> [(&'static str, Vec<u8>); 4] {
    [
        ("sdf.f32", f32_bytes(&f.sdf)),
        ("nx.f32", f32_bytes(&f.nx)),
        ("ny.f32", f32_bytes(&f.ny)),
        ("holes.u8", f.holes.clone()),
    ]
}

/// `geometry <walls.png> <out dir>`.
///
/// # Errors
/// The refusal's text, when the picture is not an 840 by 580 RGB canvas or
/// a file cannot be read or written.
pub fn run(walls: &Path, out: &Path) -> Result<(), String> {
    let bytes = std::fs::read(walls).map_err(|e| format!("{}: {e}", walls.display()))?;
    let pic =
        crate::png::decode(&bytes).map_err(|e| format!("geometry: {}: {e}", walls.display()))?;
    let mode = match pic.mode {
        crate::png::Mode::Rgb => "RGB",
        crate::png::Mode::Rgba => "RGBA",
    };
    if (pic.w, pic.h) != (W, H) || mode != "RGB" {
        return Err(format!(
            "geometry: {} is ({}, {}) {mode}, not an 840 by 580 RGB canvas",
            walls.display(),
            pic.w,
            pic.h
        ));
    }
    let f = fields(&pic.px);
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for (name, b) in files(&f) {
        let p = out.join(name);
        std::fs::write(&p, b).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}
