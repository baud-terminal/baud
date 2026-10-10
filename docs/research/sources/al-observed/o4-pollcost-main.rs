//! What one poll of Dim flashing lights costs, beside Reduce Motion's, the
//! control (the record puts Reduce Motion at about 0.04 us). Five rounds of
//! 200,000 calls each, interleaved, the first dropped; ns a call.
use macos_liquid_glass::accessibility::{dim_flashing_lights, reduce_motion};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    // NSWorkspace wants an app object for its cached answer, as the app has.
    let _ = black_box(dim_flashing_lights());
    let _ = black_box(reduce_motion());
    const N: u32 = 200_000;
    let mut dim = Vec::new();
    let mut red = Vec::new();
    for round in 0..6 {
        let t = Instant::now();
        let mut on = 0u32;
        for _ in 0..N {
            on += u32::from(black_box(dim_flashing_lights()));
        }
        let d = t.elapsed().as_nanos() as f64 / f64::from(N);
        let t = Instant::now();
        for _ in 0..N {
            on += u32::from(black_box(reduce_motion()));
        }
        let r = t.elapsed().as_nanos() as f64 / f64::from(N);
        black_box(on);
        if round > 0 {
            dim.push(d);
            red.push(r);
        }
    }
    let range = |v: &[f64]| {
        let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut s = v.to_vec();
        s.sort_by(f64::total_cmp);
        (lo, s[s.len() / 2], hi)
    };
    let (dl, dm, dh) = range(&dim);
    let (rl, rm, rh) = range(&red);
    println!("dim_flashing_lights: {dm:.1} ns a call (range {dl:.1}-{dh:.1}), value {}", dim_flashing_lights());
    println!("reduce_motion:       {rm:.1} ns a call (range {rl:.1}-{rh:.1}), value {}", reduce_motion());
}
