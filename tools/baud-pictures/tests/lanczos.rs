//! The Lanczos resize against Pillow 12.3.0's, the pinned nixpkgs'
//! (`7dd199b0`, aarch64-darwin): six images of noise, their outputs pinned
//! by SHA-256, and three built so the sums cancel, which tell the compiled
//! loop's fused and unfused taps apart.

use baud_pictures::resample::{Float, coeffs};
use baud_pictures::sha256;

/// Pillow's test images: `0.0` under 0.3, else the LCG's top 31 bits over
/// 2^31, each stored as a mode `F` pixel stores it.
fn noise(w: usize, h: usize, seed: &mut u64) -> Float {
    let px = (0..w * h)
        .map(|_| {
            *seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let r = (*seed >> 33) as f64 / f64::from(1u32 << 31);
            (if r < 0.3 { 0.0 } else { r }) as f32
        })
        .collect();
    Float { w, h, px }
}

fn digest(f: &Float) -> String {
    let b: Vec<u8> = f.px.iter().flat_map(|v| v.to_le_bytes()).collect();
    sha256::hex(&b)
}

/// **Six resizes, every output pixel Pillow's**, down and up, one axis
/// and both.
#[test]
fn the_resize_is_pillows_on_noise() {
    let cases: [(usize, usize, usize, usize, &str); 6] = [
        (
            600,
            792,
            174,
            230,
            "b750e6a144768adb3bc4c5380c286a8aeac7176eb5786c4005b4240f796c2aa5",
        ),
        (
            37,
            23,
            11,
            7,
            "1526c0b2900fe86a79c0ddf4a2e4e5433b00655e824be0c5aa0e6671f205bf5f",
        ),
        (
            5,
            9,
            13,
            4,
            "be15d5f70eb4563f3ecab889b3ec6a8e4015335c1b0427b0dd872517a16e5283",
        ),
        (
            64,
            64,
            64,
            17,
            "c1d114f28b7964fb61a14cdcdef79490595440df060e9fa59852d69a18659f65",
        ),
        (
            50,
            3,
            7,
            3,
            "50a223db3c7f7133d4f2e5f73723c917d04efaffbc18872690148b5633a023e6",
        ),
        (
            200,
            150,
            199,
            151,
            "a684115eda5d99b3c7c71104a290ab61dcc527c5b4191009abaed2fd1eb0647c",
        ),
    ];
    let mut seed = 12_345u64;
    for (w, h, ow, oh, want) in cases {
        let out = noise(w, h, &mut seed).lanczos(ow, oh);
        assert_eq!((out.w, out.h), (ow, oh));
        assert_eq!(digest(&out), want, "{w}x{h} to {ow}x{oh}");
    }
}

/// Three taps whose products cancel far below a product's rounding, so
/// whether a tap is fused shows in the `f32` sum.
fn three(px: &mut [f32], at: [usize; 3], k: [f64; 3]) {
    let a = 1e20_f32;
    let b = (-(f64::from(a)) * k[0] / k[1]) as f32;
    let c = (-(f64::from(a) * k[0] + f64::from(b) * k[1]) / k[2]) as f32;
    px[at[0]] = a;
    px[at[1]] = b;
    px[at[2]] = c;
}

/// **The vertical pass fuses every tap; the horizontal fuses only those
/// past its groups of four.** The same twenty pixels down a column and
/// along a row give -2008.89 and -1336.97 in Pillow at the same output
/// pixel, and a third row puts one of the three taps in the horizontal
/// pass's fused tail. On noise no variant moved a pixel: a sum's last
/// bits rarely reach the `f32` it is stored as.
#[test]
fn the_passes_fuse_as_the_compiled_loops_do() {
    let c = coeffs(20, 7);
    let (m, n) = c.bounds[3];
    assert_eq!(n, 18, "eighteen taps: four groups of four and a fused two");
    let k = &c.k[3 * c.ksize..];
    let mut px = vec![0.0f32; 20];
    three(&mut px, [m + 7, m + 8, m + 9], [k[7], k[8], k[9]]);
    let down = Float {
        w: 1,
        h: 20,
        px: px.clone(),
    }
    .lanczos(1, 7);
    let along = Float { w: 20, h: 1, px }.lanczos(7, 1);
    let mut px = vec![0.0f32; 20];
    three(&mut px, [m + 7, m + 8, m + 17], [k[7], k[8], k[17]]);
    let tail = Float { w: 20, h: 1, px }.lanczos(7, 1);
    let bits = |f: &Float| f.px.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    // Pillow's outputs, as printed by `list(o.getdata())` and read back.
    let pillow = |v: [f64; 7]| v.map(|x| (x as f32).to_bits()).to_vec();
    assert_eq!(
        bits(&down),
        pillow([
            8.841349608002028e17,
            -3.86504780417912e18,
            1.879020951599422e19,
            -2008.8936767578125,
            -2.2683300089361859e18,
            2.473311549385605e17,
            283765184.0
        ])
    );
    assert_eq!(
        bits(&along),
        pillow([
            8.841349608002028e17,
            -3.86504780417912e18,
            1.879020951599422e19,
            -1336.9703369140625,
            -2.2683300089361859e18,
            2.473311549385605e17,
            283765184.0
        ])
    );
    assert_eq!(
        bits(&tail),
        pillow([
            8.841349608002028e17,
            -3.86504780417912e18,
            1.879020951599422e19,
            -399.4852294921875,
            -2.2683353690553713e18,
            2.4735185668092723e17,
            1047161949126656.0
        ])
    );
}
