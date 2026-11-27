//! Builds the text-contrast curve table (`src/contrast.rs`):
//! 128 x 64 rows of 256 coverages, DirectWrite's grayscale curve. Native code
//! does in milliseconds what const evaluation took half a minute over, and
//! `tests/contrast.rs` holds every entry to `contrast::exact`, so this
//! spelling of the formula cannot drift from that one unnoticed.

const K_STEPS: usize = 128;
const F_STEPS: usize = 64;
const G: [f64; 4] = [0.148054421, -0.894594550, 1.47590804, -0.324668258];

fn exact(a: f64, k: f64, f: f64) -> f64 {
    let a1 = a * (k + 1.0) / (a * k + 1.0);
    let a2 = a1 + a1 * (1.0 - a1) * ((G[0] * f + G[1]) * a1 + (G[2] * f + G[3]));
    a2.clamp(0.0, 1.0)
}

/// `contrast.bin`'s bytes, which `main` writes to `OUT_DIR`.
pub fn build() -> Vec<u8> {
    let mut out = Vec::with_capacity(K_STEPS * F_STEPS * 256);
    for kq in 0..K_STEPS {
        for fq in 0..F_STEPS {
            let k = kq as f64 / (K_STEPS - 1) as f64;
            let f = fq as f64 / (F_STEPS - 1) as f64;
            for a in 0..256 {
                out.push((exact(a as f64 / 255.0, k, f) * 255.0 + 0.5) as u8);
            }
        }
    }
    out
}
