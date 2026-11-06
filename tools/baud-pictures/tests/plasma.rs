//! The README logo's geometry and plasma against the Python and NumPy
//! that wrote them before (CPython 3.14.7, NumPy 2.5.2, Pillow 12.3.0, the
//! pinned nixpkgs `7dd199b0`, run under its no-libm guard): every output
//! pinned by the SHA-256 that program's own output had on the same input,
//! the committed walls and two perturbations of them, and the arithmetic
//! underneath on vectors built to tell the near-misses apart.

use baud_pictures::geometry::{self, Fields, H, W};
use baud_pictures::plasma::{self, Plasma, Stop};
use baud_pictures::{det, numpy, png, py, sha256};

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn walls() -> Vec<u8> {
    let b = std::fs::read(root().join("assets/readme/logo-walls.png")).expect("the walls");
    let im = png::decode(&b).expect("a PNG");
    assert_eq!((im.w, im.h, im.mode), (W, H, png::Mode::Rgb));
    im.px
}

/// The four files' SHA-256, in the order sdf, nx, ny, holes.
fn geometry_digests(f: &Fields) -> Vec<String> {
    geometry::files(f)
        .iter()
        .map(|(_, b)| sha256::hex(b))
        .collect()
}

fn clone(f: &Fields) -> Fields {
    Fields {
        sdf: f.sdf.clone(),
        nx: f.nx.clone(),
        ny: f.ny.clone(),
        holes: f.holes.clone(),
    }
}

/// Both halves' strands and reports, SHA-256 each.
fn strands_digests(f: &Fields) -> [String; 4] {
    let (dark, dark_report) = Plasma::new(clone(f), true).strands().expect("strands");
    let (light, light_report) = Plasma::new(clone(f), false).strands().expect("strands");
    [
        sha256::hex(dark.as_bytes()),
        sha256::hex(dark_report.as_bytes()),
        sha256::hex(light.as_bytes()),
        sha256::hex(light_report.as_bytes()),
    ]
}

/// **The committed walls: the four geometry files and both halves'
/// strands and reports are the Python's bytes.** The strands are what the
/// film draws, the report what the film's driver prints and checks.
#[test]
fn the_walls_geometry_and_both_halves_are_the_pythons() {
    let f = geometry::fields(&walls());
    assert_eq!(
        geometry_digests(&f),
        [
            "b7694dbbce02c1a660ed0bfa7e5f1136dbd762a7719b0460dd1ac08141eebfec",
            "a60df26c85326ac6f925ffedfb355cebd9633450d082e7bd3fd2cfa7b1c96cad",
            "8c334763942b5fce8f23bd88f7f98bb0bfc34baf20a810f0cbaa038b0b144233",
            "7f7914e6ca63d7543e84022c537e2b9a262eee3c127f6b7439b3f027fbae9df2",
        ]
    );
    assert_eq!(
        strands_digests(&f),
        [
            "2f22ef539e274a0f210993acfbac013ed3435c3829a901e8c801c4eb332c9e67",
            "5bd8a0f0838ec9e2311b0b1865a5604388e1957709b5b8c6accf8dbd338b6160",
            "b8a14063f02b45d2fd71f58b0650ea860be70c23a62a6800ee049266844779cd",
            "5bd8a0f0838ec9e2311b0b1865a5604388e1957709b5b8c6accf8dbd338b6160",
        ]
    );
}

/// **The film is the whole loop, and the frame after its last is its
/// first**: on both halves, the committed walls' chain, every frame of the
/// grid's strands the same a whole loop later, so the wrap draws what the
/// next frame draws; twenty 500 ms cycles of eight frames a phase, the last
/// line the time it wraps at, and strands in every frame, so none is the bare
/// logo.
#[test]
fn the_film_is_the_whole_loop_and_repeats() {
    let f = geometry::fields(&walls());
    for dark in [true, false] {
        let p = Plasma::new(clone(&f), dark);
        assert_eq!(p.first_unrepeated().expect("a chain"), None, "{dark}");
        let (text, report) = p.whole_loop().expect("a chain");
        assert!(
            report.starts_with(
                "{\"seed\": 2241, \"period_cs\": 1000, \"events\": 14, \"frames\": 320,"
            ),
            "{report}"
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.last(), Some(&"E 1000"), "where it wraps");
        let frames: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.starts_with("F ") || l.starts_with("E "))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(frames.len(), 321, "the frames and the end");
        for w in frames.windows(2) {
            assert!(w[1] > w[0] + 1, "a frame with no strands: {}", lines[w[0]]);
        }
        let times: Vec<u32> = lines
            .iter()
            .filter_map(|l| l.strip_prefix("F "))
            .map(|l| {
                l.split(' ')
                    .nth(1)
                    .expect("a time")
                    .parse()
                    .expect("a number")
            })
            .collect();
        assert_eq!(times[..9], [0, 3, 6, 9, 12, 15, 18, 21, 25], "the grid");
        assert_eq!(times[319], 996, "the last frame, 40 ms to the wrap");
    }
}

/// **A dot of the word's ink on the page moves the geometry as it moved
/// the Python's**: a 5 by 5 block at canvas (300, 100), clear of the
/// letters, so the signed distance and the normals change round it and
/// the counters do not.
#[test]
fn a_dot_on_the_walls_moves_the_geometry_as_the_pythons() {
    let mut px = walls();
    for y in 100..105 {
        for x in 300..305 {
            px[(y * W + x) * 3..(y * W + x) * 3 + 3].copy_from_slice(&[0xF3, 0xF4, 0xF6]);
        }
    }
    assert_eq!(
        geometry_digests(&geometry::fields(&px)),
        [
            "3b97110bbd9ba417de6e6b64065d8a905ce934154ffbddf6aef740dd88c71f65",
            "a9b3f25b51400416f5189ec9c979c696d8d90365b2d0a129a44ab307457050d5",
            "e3805acd1669c786c514f69ac4b7a47b9643fcc1fa3dccf2cef244ee8499a06b",
            "7f7914e6ca63d7543e84022c537e2b9a262eee3c127f6b7439b3f027fbae9df2",
        ]
    );
}

/// **The walls a pixel to the right: the geometry, and a chain from a
/// later seed, both the Python's.** Every distance moves, seeds 2241 to
/// 2492 fail or miss the targets, and 2493 is the first that fits -- the
/// search's refusals and fall-through, not only its first pick.
#[test]
fn the_walls_a_pixel_right_land_on_the_pythons_later_seed() {
    let src = walls();
    let mut px = src.clone();
    for y in 0..H {
        for x in 1..W {
            let (to, from) = ((y * W + x) * 3, (y * W + x - 1) * 3);
            px[to..to + 3].copy_from_slice(&src[from..from + 3]);
        }
    }
    let f = geometry::fields(&px);
    assert_eq!(
        geometry_digests(&f),
        [
            "cb5bb2108334b3ef8f460bff094a5fb849f904a9f705b500980230539a444471",
            "fa3353649316d4fd61e47ad4f49ec4472feaa8b7276993acfcd6167764518a68",
            "2abfafaef558c049e1d4e4cd045ce86641bba170b86b59908d04404b959a1350",
            "6793514b4f0a19b6ea5ecddc8618123698847e670276b10e38d7a0328ed824d1",
        ]
    );
    let (_, report) = Plasma::new(clone(&f), true).strands().expect("strands");
    assert!(
        report.starts_with("{\"seed\": 2493, \"start_cs\": 250, \"events\": 8,"),
        "{report}"
    );
    assert_eq!(
        strands_digests(&f),
        [
            "c74cb582715a1e0f8164b99072639135ba0f10ccd345b869cd1eb5f8dfeae3d4",
            "3a62696a5b7750dc8bbf8f9dc444cd79e21bec6aeca640367af835df85948e14",
            "783bb8c371cf2df399616f54e8cd1b9fa1d5090d569c0f0ed5749497b6553fc1",
            "3a62696a5b7750dc8bbf8f9dc444cd79e21bec6aeca640367af835df85948e14",
        ]
    );
}

/// **The refusals are the Python's words**, but for the usage line, which
/// names this program: a blink not of 250 ms phases, a field of the wrong
/// size, and the wrong arguments.
#[test]
fn the_plasma_refuses_as_the_python_did() {
    let msg = |args: &[&str]| match plasma::run(
        &args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
    ) {
        Err(Stop::Exit(m)) => m,
        other => panic!("not refused: {other:?}"),
    };
    assert_eq!(
        msg(&["g", "dark", "300", "250", "o"]),
        "plasma: the frame grid is built on 250 ms phases, not 300 and 250"
    );
    assert_eq!(
        msg(&["g", "grey", "250", "250", "o"]),
        "usage: baud-pictures plasma <geometry dir> <dark|light> <on_ms> <off_ms> <out>"
    );
    let dir = std::env::temp_dir().join(format!("baud-pictures-plasma-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    std::fs::write(dir.join("sdf.f32"), [0u8; 400]).expect("a short field");
    let d = dir.to_str().expect("a UTF-8 path");
    assert_eq!(
        msg(&[d, "dark", "250", "250", "o"]),
        "plasma: sdf.f32 holds 100 values, not 487200"
    );
    std::fs::remove_dir_all(&dir).expect("removed");
}

// ---- the arithmetic underneath --------------------------------------------

/// splitmix64, the vectors' source, as the Python that pinned them wrote it.
struct Mix(u64);

impl Mix {
    fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// A float of either sign over 2^-30 to 2^30.
    fn wide(&mut self) -> f64 {
        let u = self.unit() - 0.5;
        let e = (self.u64() % 61) as i64 - 30;
        u * f64::from_bits(((e + 1023) as u64) << 52)
    }
}

fn h(v: impl IntoIterator<Item = f64>) -> String {
    let b: Vec<u8> = v.into_iter().flat_map(f64::to_le_bytes).collect();
    sha256::hex(&b)
}

/// **NumPy's sums and means, Python's `sum`, `math.fsum`, rounding,
/// formatting, `%` and `//`, `arange` and `det`'s functions, each the
/// pinned Python's bits** over vectors made to separate the near-misses:
/// sums of 1 to 300 values across 61 binades (so the pairwise blocks,
/// the 8-accumulator path and the halving all run), lists that cancel,
/// exact ties at every rounding place, the multiples of pi/4 either side,
/// and both signs. A naive loop in place of the pairwise sum, a plain
/// sum in place of `fsum` or the compensated `sum`, Python's `round` for
/// NumPy's, or Rust's `%` for Python's each turns a row red.
#[test]
#[allow(clippy::too_many_lines)]
fn the_arithmetic_is_the_pinned_pythons_and_numpys() {
    let mut want = std::collections::BTreeMap::new();
    for line in [
        "np_sum b04280336291da1dcc9b28e88d182c58baabcd3fdcfe3e6004150e445a56d576",
        "np_mean d37c2572e746f23b3a71a34fd3e7d70425c199971b2abfe6fa322c78d2f45711",
        "py_sum ca2118a22c925a5ae13a5897dda02f62d9119d39a5202b8c6527a967a7322afc",
        "fsum e5e0a5de0a4c651df08797d042cb4e390191fa9fc0e8df8c0b594e2f5f7168cc",
        "det_sin 8fe93e41b65699a70605c7ad8a2d8617d8338930528fb39243a62bd9bc80ff00",
        "det_cos 3ad8aea168fae86e8317067ac2b331d192dd1827409dcee852303bb4f93f9018",
        "det_exp d07de33bfec3a0a93faf0e639c7797e59836796c24a0445ef69c82057601a8da",
        "det_hypot a88d2e4592c70015e9e72b802ba7b5034b274c8b975f079b28f2800d31b5dff0",
        "round1 eff1c1dca575f5f06536b721e31054265b9d0d63e6135df31c4e3e523b9d3a35",
        "round3 f5e45ee9070cfad8eb6bab39528a4d55bf39c8ef5b4120d6c9dcdfd38cf1d19e",
        "round4 0bcab7485f6a01e48f4908afea455052bf6c96f08a7adb53bb4582bcecba2742",
        "np_round1 f36de0c2067209cdbc958ca5fa79a5c1d742c04acb66fad79e6c3bc5da79127e",
        "round0 921fcdabea3913425093ccbdeec3ef3b96bc987fc7b29c3cabecaebe6891f26e",
        "np_rint 85ceca9d26d34c3a4574e869c26318dc7f933a20669024de401448b8a54c3a6e",
        "fmt4 a7afad9c7ecf5fe148f3d66c831b1bfc97bcaa6ff8d14314b9a7d6faaed5d63a",
        "repr 58432a86645183d57e374f4e9886890a25a0d15a929ab35eb5c47836c306d55d",
        "arange 0b741773d41b41983eae3c8c4ed118bb1af80371d59561ce9c70bab2edbbd0fe",
        "py_mod ab211fe942480c98d266b9fa8a6af0d90c510400cad104c9ea6257ecfb8c255a",
        "py_floordiv 1d1ceb6f2861624a31c34a2d484e1bb14d56c01f6c6e7929e5a00211c44d5f86",
    ] {
        let (k, v) = line.split_once(' ').expect("a row");
        want.insert(k, v.to_string());
    }
    let mut got = std::collections::BTreeMap::new();

    // NumPy's sum and mean over 1 to 300 values; a strided column of a
    // two-dimensional array reduces the same, NumPy giving the same digest.
    let mut r = Mix(1);
    let (mut sums, mut means) = (Vec::new(), Vec::new());
    for n in 1..=300 {
        let a: Vec<f64> = (0..n).map(|_| r.wide()).collect();
        sums.push(numpy::sum(&a));
        means.push(numpy::mean(&a));
    }
    got.insert("np_sum", h(sums));
    got.insert("np_mean", h(means));

    // the builtin sum, cancelling
    let mut r = Mix(2);
    let mut bs = Vec::new();
    for n in 0..200 {
        let mut v = Vec::new();
        for _ in 0..n {
            let x = r.wide();
            v.push(x);
            if r.unit() < 0.3 {
                v.push(-x * (1.0 + r.unit() * 1e-12));
            }
        }
        bs.push(py::sum(v).unwrap_or(0.0));
    }
    got.insert("py_sum", h(bs));

    // math.fsum, with exact cancellations and ties across partials
    let mut r = Mix(3);
    let mut fs = Vec::new();
    for n in 0..200 {
        let mut v = Vec::new();
        for _ in 0..n {
            let x = r.wide();
            v.push(x);
            if r.unit() < 0.3 {
                v.push(-x);
            }
            if r.unit() < 0.1 {
                let s = if r.unit() < 0.5 { 1.0 } else { -1.0 };
                v.push(f64::from_bits((1023 - 60) << 52) * s);
            }
        }
        fs.push(py::fsum(v));
    }
    fs.push(py::fsum([1e-16, 1.0, 1e16]));
    fs.push(py::fsum([1.0, 1e100, 1.0, -1e100]));
    got.insert("fsum", h(fs));

    // det
    let mut r = Mix(4);
    let mut xs: Vec<f64> = (0..20000).map(|_| r.unit() * 200.0 - 100.0).collect();
    for k in -40..=40 {
        for d in [-1e-12, 0.0, 1e-12] {
            xs.push(f64::from(k) * std::f64::consts::PI / 4.0 + d);
        }
    }
    got.insert("det_sin", h(xs.iter().map(|&x| det::sin(x))));
    got.insert("det_cos", h(xs.iter().map(|&x| det::cos(x))));
    let mut es: Vec<f64> = (0..20000).map(|_| r.unit() * 720.0 - 700.0).collect();
    es.extend([0.0, -0.0, 1.0, -1.0]);
    got.insert("det_exp", h(es.iter().map(|&x| det::exp(x))));
    let hs: Vec<(f64, f64)> = (0..5000).map(|_| (r.wide(), r.wide())).collect();
    got.insert("det_hypot", h(hs.iter().map(|&(a, b)| det::hypot(a, b))));

    // rounding and formatting, ties at every place
    let mut r = Mix(5);
    let mut vals: Vec<f64> = (0..20000).map(|_| r.wide() * 1000.0).collect();
    vals.extend((-640..=640).map(|k| f64::from(k) / 64.0));
    vals.extend((-300..=300).map(|k| f64::from(k) / 1e5));
    vals.extend([-0.0, 0.0]);
    for step in [0.05, 0.0005, 0.00005] {
        vals.extend((-4000..=4000).map(|k| f64::from(k) * step));
    }
    got.insert("round1", h(vals.iter().map(|&v| py::round_to(v, 1))));
    got.insert("round3", h(vals.iter().map(|&v| py::round_to(v, 3))));
    got.insert("round4", h(vals.iter().map(|&v| py::round_to(v, 4))));
    got.insert("np_round1", h(vals.iter().map(|&v| numpy::round_to(v, 1))));
    got.insert("round0", h(vals.iter().map(|&v| py::round(v) as f64)));
    got.insert("np_rint", h(vals.iter().map(|&v| v.round_ties_even())));
    let text =
        |f: &dyn Fn(f64) -> String| vals.iter().map(|&v| f(v)).collect::<Vec<_>>().join("\n");
    got.insert("fmt4", sha256::hex(text(&|v| format!("{v:.4}")).as_bytes()));
    got.insert("repr", sha256::hex(text(&py::repr_float).as_bytes()));

    // arange
    let mut ar = numpy::arange(0.0, 10.0, 0.01);
    for (a, b, s) in [
        (34.0, 100.0, 1.5),
        (40.0, 232.0, 1.5),
        (30.0, 270.0, 4.0),
        (30.0, 104.0, 4.0),
        (1.0, 14.0, 1.0),
    ] {
        ar.extend(numpy::arange(a, b, s));
    }
    got.insert("arange", h(ar));

    // float % and //, both signs
    let mut r = Mix(6);
    let pairs: Vec<(f64, f64)> = (0..20000)
        .map(|_| {
            let a = r.wide() * 100.0;
            let b = r.unit() * 20.0 - 10.0;
            (a, if b == 0.0 { 1.0 } else { b })
        })
        .collect();
    got.insert("py_mod", h(pairs.iter().map(|&(a, b)| py::rem(a, b))));
    got.insert(
        "py_floordiv",
        h(pairs.iter().map(|&(a, b)| py::floordiv(a, b))),
    );

    let bad: Vec<_> = want
        .iter()
        .filter(|(k, v)| got.get(*k) != Some(*v))
        .map(|(k, _)| *k)
        .collect();
    assert!(bad.is_empty(), "not the pinned bits: {bad:?}");
    assert_eq!(got.len(), want.len());
}

/// **The pairwise sum's own shape**: the 9-value mean the plasma's hug
/// takes runs the 8-accumulator path -- here on values where it differs
/// from the plain loop -- and the hug itself only ever means 0s and 1s,
/// whose sum is exact in any order.
#[test]
fn nine_values_take_the_eight_accumulators() {
    let a: [f64; 9] = [1e16, 1.0, -1e16, 1.0, 0.5, 0.25, 3.0, -3.0, 1.0];
    let plain = a.iter().fold(0.0f64, |s, &x| s + x);
    let pairwise = ((a[0] + a[1]) + (a[2] + a[3])) + ((a[4] + a[5]) + (a[6] + a[7])) + a[8];
    assert_ne!(
        plain.to_bits(),
        pairwise.to_bits(),
        "the control tells the two apart"
    );
    assert_eq!(numpy::sum(&a).to_bits(), (0.0 + pairwise).to_bits());
    assert_eq!(
        numpy::mean(&[1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
        6.0 / 9.0
    );
}

/// The calls a source whose numbers reach the strands must not spell:
/// each is the platform's libm, or a fused or repeated multiply that
/// Python and NumPy never make.
const PLATFORM: [&str; 30] = [
    ".exp(",
    ".exp2(",
    ".exp_m1(",
    ".ln(",
    ".ln_1p(",
    ".log2(",
    ".log10(",
    ".log(",
    ".sin(",
    ".cos(",
    ".tan(",
    ".sin_cos(",
    ".asin(",
    ".acos(",
    ".atan(",
    ".atan2(",
    ".hypot(",
    ".powf(",
    ".powi(",
    ".cbrt(",
    ".tanh(",
    ".sinh(",
    ".cosh(",
    ".mul_add(",
    "f64::exp",
    "f64::sin",
    "f64::cos",
    "f64::powf",
    "f64::hypot",
    "libm::",
];

fn platform_calls(src: &str) -> Vec<&'static str> {
    PLATFORM
        .iter()
        .copied()
        .filter(|c| src.contains(c))
        .collect()
}

/// **The logo generator calls no platform math**: the geometry, the
/// plasma, `det` and the Python and NumPy semantics they lean on spell
/// none of std's libm calls, nor a fused multiply-add, so the strands are
/// the same on every platform and are the Python's, which ran with its
/// platform's functions taken away. What they do call is IEEE's own:
/// `+ - * /`, `sqrt`, `floor`, rounding to an integer, and `fmod`
/// (`%`), which C defines as exact. Perturbation: a `.exp(` put back in
/// the parcels' settling fails it; the control below shows the scan fires.
#[test]
fn the_logo_generator_calls_no_platform_math() {
    for (name, src) in [
        ("det.rs", include_str!("../src/det.rs")),
        ("geometry.rs", include_str!("../src/geometry.rs")),
        ("plasma.rs", include_str!("../src/plasma.rs")),
        ("numpy.rs", include_str!("../src/numpy.rs")),
        ("py.rs", include_str!("../src/py.rs")),
    ] {
        assert!(src.len() > 1000 && src.starts_with("//!"), "{name} is read");
        assert_eq!(
            platform_calls(src),
            Vec::<&str>::new(),
            "{name} calls the platform's math"
        );
    }
    // The positive control: the scan finds each spelling it bars.
    assert_eq!(platform_calls("let w = (-age / 0.06).exp();"), [".exp("]);
    assert_eq!(
        platform_calls("a.mul_add(b, c) + f64::sin(x)"),
        [".mul_add(", "f64::sin"]
    );
}
