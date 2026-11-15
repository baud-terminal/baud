//! A colour specification, as XParseColor reads it and an X server draws it.
//!
//! The text is the Xlib manual's "Color Strings"
//! (`[[source: xlib-ch06-b8d5a70]]`): "Color strings are case-insensitive";
//! `rgb:` digits are "scaled"; and for the `#` forms, "When fewer than 16 bits
//! each are specified, they represent the most significant bits of the value
//! (unlike the ``rgb:'' syntax, in which values are scaled). For example, the
//! string ``#3a7'' is the same as ``#3000a0007000''."
//!
//! The oracle is libX11 1.8.13 against a 24-bit TrueColor Xvfb
//! (`[[source: xparsecolor-obs-rgb-out]]`): each line is the spec, the
//! 16-bit value XParseColor returned and the pixel XAllocColor gave it, which
//! is the value's high byte -- the eight bits xterm draws.
use baud_domain::{Chan, XColor};
use proptest::prelude::*;

fn rgb(r: u8, g: u8, b: u8) -> XColor<'static> {
    XColor::Rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

/// The manual's own: `#3a7` is `#3000a0007000`, its digits the high bits.
#[test]
fn a_short_sharp_form_is_the_high_bits() {
    assert_eq!(XColor::parse(b"#3a7"), rgb(0x30, 0xa0, 0x70));
    assert_eq!(XColor::parse(b"#3000a0007000"), rgb(0x30, 0xa0, 0x70));
    assert_eq!(XColor::parse(b"#fff"), rgb(0xf0, 0xf0, 0xf0));
    assert_eq!(XColor::parse(b"#abcabcabc"), rgb(0xab, 0xab, 0xab));
    // `rgb:` scales: one digit is the whole range.
    assert_eq!(XColor::parse(b"rgb:f/f/f"), rgb(0xff, 0xff, 0xff));
}

/// The manual's `rgb:` examples, and its prefix in any case.
#[test]
fn a_prefix_is_read_in_any_case() {
    assert_eq!(XColor::parse(b"rgb:00/ff/00"), rgb(0, 0xff, 0));
    assert_eq!(XColor::parse(b"RGB:FF/00/00"), rgb(0xff, 0, 0));
    assert_eq!(XColor::parse(b"Rgb:ff/A5/0"), rgb(0xff, 0xa5, 0));
}

/// **Every line the oracle answered, as it answered.** A spec it refused is
/// [`XColor::Unresolved`]; the count is the positive control.
///
/// Two lines are libX11's and not the manual's: its `rgb:` reader stops after
/// the third field and never looks at what follows, so `rgb:1/2/3/` and
/// `rgb:1/2/3/4` are colours to it. The manual's grammar is
/// `rgb:<red>/<green>/<blue>`, and baud refuses both.
#[test]
fn every_spec_reads_as_the_oracle_drew_it() {
    let src = include_str!("../../docs/research/sources/xparsecolor-observed/rgb.out");
    let departs = ["rgb:1/2/3/", "rgb:1/2/3/4"];
    let (mut drawn, mut refused) = (0, 0);
    for line in src.lines() {
        let mut f = line.split('\t');
        let spec = f.next().unwrap();
        let answer = f.next().unwrap();
        if departs.contains(&spec) {
            assert_ne!(answer, "FAIL", "{spec}");
            assert_eq!(
                XColor::parse(spec.as_bytes()),
                XColor::Unresolved(spec.as_bytes())
            );
            continue;
        }
        if answer == "FAIL" {
            assert_eq!(
                XColor::parse(spec.as_bytes()),
                XColor::Unresolved(spec.as_bytes()),
                "{spec}"
            );
            refused += 1;
            continue;
        }
        let px = u32::from_str_radix(f.next().unwrap(), 16).unwrap();
        let [_, r, g, b] = px.to_be_bytes();
        assert_eq!(XColor::parse(spec.as_bytes()), rgb(r, g, b), "{spec}");
        drawn += 1;
    }
    assert_eq!((drawn, refused), (589, 16));
}

// ---- `rgbi:` and the device-independent forms ----
//
// The reference below is written from the texts with the standard library's
// arithmetic, not from the port: CIE 015:2018 Annex C and 8.2.2 for CIELAB
// and CIELUV (`[[source: cie-015-2018]]`), CSS Color 4's sRGB matrices,
// D65 and `gam_sRGB` (`[[source: css-color-4-20261009]]` 10.2), and for
// TekHVC Xcms's formulas (`HVC.c`) with libm's trigonometry in place of
// Xcms's own.

const TO_XYZ: [[f64; 3]; 3] = [
    [506752.0 / 1228815.0, 87881.0 / 245763.0, 12673.0 / 70218.0],
    [87098.0 / 409605.0, 175762.0 / 245763.0, 12673.0 / 175545.0],
    [7918.0 / 409605.0, 87881.0 / 737289.0, 1001167.0 / 1053270.0],
];
const FROM_XYZ: [[f64; 3]; 3] = [
    [12831.0 / 3959.0, -329.0 / 214.0, -1974.0 / 3959.0],
    [
        -851781.0 / 878810.0,
        1648619.0 / 878810.0,
        36519.0 / 878810.0,
    ],
    [705.0 / 12673.0, -2585.0 / 12673.0, 705.0 / 667.0],
];

fn mul(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    core::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn white() -> [f64; 3] {
    [TO_XYZ[0].iter().sum(), 1.0, TO_XYZ[2].iter().sum()]
}

fn uv(xyz: [f64; 3]) -> (f64, f64) {
    let d = xyz[0] + 15.0 * xyz[1] + 3.0 * xyz[2];
    (4.0 * xyz[0] / d, 9.0 * xyz[1] / d)
}

fn from_uvy(u: f64, v: f64, y: f64) -> [f64; 3] {
    let d = 6.0 * u - 16.0 * v + 12.0;
    let (x, yy) = (9.0 * u / d, 4.0 * v / d);
    [x * y / yy, y, (1.0 - x - yy) * y / yy]
}

fn cie_y(l: f64) -> f64 {
    let fy = (l + 16.0) / 116.0;
    if l > 8.0 {
        fy.powi(3)
    } else {
        (fy - 16.0 / 116.0) * 108.0 / 841.0
    }
}

/// The hue TekHVC measures from: the white's line to best red.
fn theta() -> f64 {
    let (un, vn) = uv(white());
    ((0.4931 - vn) / (0.7127 - un)).atan().to_degrees()
}

fn reference_xyz(spec: &str) -> [f64; 3] {
    let (p, v) = spec.split_once(':').unwrap();
    let v: Vec<f64> = v.split('/').map(|x| x.parse().unwrap()).collect();
    let w = white();
    let (un, vn) = uv(w);
    match p.to_ascii_lowercase().as_str() {
        "ciexyz" => [v[0], v[1], v[2]],
        "cieuvy" => from_uvy(v[0], v[1], v[2]),
        "ciexyy" => [v[0] * v[2] / v[1], v[2], (1.0 - v[0] - v[1]) * v[2] / v[1]],
        "cielab" => {
            let fy = (v[0] + 16.0) / 116.0;
            let inv = |f: f64| {
                if f > 24.0 / 116.0 {
                    f.powi(3)
                } else {
                    (f - 16.0 / 116.0) * 108.0 / 841.0
                }
            };
            [
                w[0] * inv(v[1] / 500.0 + fy),
                cie_y(v[0]),
                w[2] * inv(fy - v[2] / 200.0),
            ]
        }
        "cieluv" => from_uvy(
            v[1] / (13.0 * v[0]) + un,
            v[2] / (13.0 * v[0]) + vn,
            cie_y(v[0]),
        ),
        "tekhvc" => {
            let h = (v[0] + theta()).to_radians();
            let y = if v[1] < 7.99953624 {
                v[1] / 903.29
            } else {
                ((v[1] + 16.0) / 116.0).powi(3)
            };
            let k = v[2] / (v[1] * 7.50725);
            from_uvy(un + h.cos() * k, vn + h.sin() * k, y)
        }
        other => panic!("{other}"),
    }
}

fn encode(i: f64) -> u8 {
    let e = if i > 0.0031308 {
        1.055 * i.powf(1.0 / 2.4) - 0.055
    } else {
        12.92 * i
    };
    let level = (e.clamp(0.0, 1.0) * 255.0).round();
    (0..=u8::MAX).find(|&k| f64::from(k) == level).unwrap()
}

fn decode(b: u8) -> f64 {
    let e = f64::from(b) / 255.0;
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

/// The reference's colour, or `None` out of gamut.
fn reference(spec: &str) -> Option<[u8; 3]> {
    let lin = if spec.to_ascii_lowercase().starts_with("rgbi:") {
        let v: Vec<f64> = spec[5..].split('/').map(|x| x.parse().unwrap()).collect();
        [v[0], v[1], v[2]]
    } else {
        mul(&FROM_XYZ, reference_xyz(spec))
    };
    lin.iter()
        .all(|&c| (-0.001..=1.001).contains(&c))
        .then(|| lin.map(|c| encode(c.clamp(0.0, 1.0))))
}

/// TekHVC of a drawn colour, with libm: hue in degrees, value.
fn hue_value(c: [u8; 3]) -> (f64, f64) {
    let xyz = mul(&TO_XYZ, c.map(decode));
    let (u, v) = uv(xyz);
    let (un, vn) = uv(white());
    let h = ((v - vn).atan2(u - un).to_degrees() - theta()).rem_euclid(360.0);
    let y = xyz[1];
    let value = if y < 0.008856 {
        y * 903.29
    } else {
        116.0 * y.cbrt() - 16.0
    };
    (h, value)
}

fn drawn(spec: &str) -> Option<[u8; 3]> {
    match XColor::parse(spec.as_bytes()) {
        XColor::Rgb(r, g, b) => Some([r.get(), g.get(), b.get()]),
        _ => None,
    }
}

/// The manual's own examples, in the case it writes them.
#[test]
fn the_manuals_examples_are_colours() {
    assert_eq!(drawn("RGBi:1.0/0.0/0.0"), Some([0xff, 0, 0]));
    // L* 50 is Y 0.1842, sRGB 119 on each channel.
    assert_eq!(drawn("CIELuv:50.0/0.0/0.0"), Some([0x77, 0x77, 0x77]));
    let xyz = "CIEXYZ:0.3227/0.28133/0.2493";
    assert_eq!(drawn(xyz), reference(xyz));
    assert!(drawn(xyz).is_some());
    // White in every form, and black.
    for spec in [
        "CIELab:100/0/0",
        "CIELuv:100/0/0",
        "TekHVC:0/100/0",
        "rgbi:1/1/1",
        "ciexyy:0.3127/0.329/1",
    ] {
        assert_eq!(drawn(spec), Some([0xff; 3]), "{spec}");
    }
    for spec in [
        "CIELab:0/0/0",
        "CIELuv:0/0/0",
        "TekHVC:0/0/0",
        "RGBI:0/0/0",
        "CIEXYZ:0/0/0",
    ] {
        assert_eq!(drawn(spec), Some([0; 3]), "{spec}");
    }
}

/// **A colour inside sRGB is the reference's, to the level.** Every form,
/// over a grid that crosses L* 8 and spans the gamut; the counts are the
/// positive control.
#[test]
fn an_in_gamut_spec_is_the_texts_colour() {
    let mut specs = Vec::new();
    for l in [0.0, 3.0, 7.0, 8.0, 9.0, 25.0, 50.0, 75.0, 96.0, 100.0] {
        for (a, b) in [
            (0.0, 0.0),
            (20.0, -10.0),
            (-15.0, 25.0),
            (10.0, 10.0),
            (-30.0, -30.0),
        ] {
            specs.push(format!("CIELab:{l}/{a}/{b}"));
            specs.push(format!("CIELuv:{l}/{a}/{b}"));
        }
    }
    for h in (0..360).step_by(20) {
        for (v, c) in [
            (30.0, 10.0),
            (50.0, 20.0),
            (70.0, 15.0),
            (5.0, 2.0),
            (90.0, 5.0),
        ] {
            specs.push(format!("TekHVC:{h}/{v}/{c}"));
        }
    }
    for x in [0.05, 0.2, 0.35, 0.5, 0.8] {
        for y in [0.05, 0.2, 0.4, 0.7, 1.0] {
            for z in [0.05, 0.3, 0.6, 1.0] {
                specs.push(format!("CIEXYZ:{x}/{y}/{z}"));
            }
        }
    }
    for (u, v) in [(0.2, 0.47), (0.18, 0.45), (0.22, 0.5), (0.19, 0.4)] {
        for y in [0.05, 0.4, 0.9] {
            specs.push(format!("CIEuvY:{u}/{v}/{y}"));
        }
    }
    for (x, y) in [(0.3127, 0.329), (0.35, 0.35), (0.28, 0.3), (0.4, 0.4)] {
        for yy in [0.05, 0.4, 0.9] {
            specs.push(format!("CIExyY:{x}/{y}/{yy}"));
        }
    }
    for r in [0.0, 0.002, 0.1, 0.5, 0.9, 1.0] {
        for g in [0.0, 0.3, 1.0] {
            specs.push(format!("rgbi:{r}/{g}/0.7"));
        }
    }
    let mut checked = 0;
    for spec in &specs {
        let Some(want) = reference(spec) else {
            continue;
        };
        assert_eq!(drawn(spec), Some(want), "{spec}");
        checked += 1;
    }
    assert_eq!((specs.len(), checked), (332, 189));
}

/// **A colour outside sRGB keeps its TekHVC hue and value and loses
/// chroma**, as Xcms's default compression `XcmsTekHVCClipC` does, and lands
/// on the gamut's surface: one channel at 0 or 255. Hue and value are read
/// back from the drawn bytes with the reference's arithmetic, so the
/// tolerance is the eight-bit step's, not the port's.
#[test]
fn an_out_of_gamut_spec_keeps_its_hue_and_value() {
    let specs = [
        "CIELab:60/-80/60",
        "CIELab:50/80/0",
        "CIELab:30/40/-90",
        "CIELab:85/-20/90",
        "CIELuv:50/150/0",
        "CIELuv:70/-80/60",
        "TekHVC:0/50/90",
        "TekHVC:120/60/80",
        "TekHVC:240/40/70",
        "TekHVC:300/70/60",
        "CIExyY:0.2/0.6/0.4",
        "CIEXYZ:0.6/0.3/0.02",
    ];
    for spec in specs {
        assert_eq!(reference(spec), None, "{spec} is in gamut");
        let c = drawn(spec).unwrap_or_else(|| panic!("{spec} refused"));
        assert!(
            c.contains(&0) || c.contains(&255),
            "{spec}: {c:02x?} is inside"
        );
        let xyz = reference_xyz(spec);
        let (u, v) = uv(xyz);
        let (un, vn) = uv(white());
        let want_h = ((v - vn).atan2(u - un).to_degrees() - theta()).rem_euclid(360.0);
        let want_v = if xyz[1] < 0.008856 {
            xyz[1] * 903.29
        } else {
            116.0 * xyz[1].cbrt() - 16.0
        };
        let (h, val) = hue_value(c);
        let dh = (h - want_h + 540.0).rem_euclid(360.0) - 180.0;
        assert!(dh.abs() < 1.5, "{spec}: hue {h:.2} for {want_h:.2}");
        assert!(
            (val - want_v).abs() < 0.6,
            "{spec}: value {val:.2} for {want_v:.2}"
        );
    }
}

/// **What the text does not spell is refused**, among it what libX11's
/// `sscanf` takes: leading blanks, a fourth field, a hex float, `inf`;
/// `rgbi:` past [0, 1], which libX11 extrapolates its tables for
/// (`rgbi:1.1/0/0` draws 0b0000 there); and each space's range.
#[test]
fn a_spec_off_the_grammar_or_the_range_is_refused() {
    for spec in [
        "rgbi:",
        "rgbi:1/0",
        "rgbi:1//0",
        "rgbi:1/0/0/9",
        "rgbi: 1/0/0",
        "rgbi:1/0/0 ",
        "rgbi:0x1/0/0",
        "rgbi:inf/0/0",
        "rgbi:nan/0/0",
        "rgbi:./0/0",
        "rgbi:1e/0/0",
        "rgbi:1.1/0/0",
        "rgbi:-0.5/0/0",
        // In the grammar, which takes a trailing point; past the range.
        "rgbi:5./0/0",
        "CIEXYZ:0/1.5/0",
        "CIEXYZ:0/-0.5/0",
        "CIEuvY:0.2/0.4/1.5",
        "CIExyY:1.2/0.3/0.5",
        "CIExyY:0.3/-0.1/0.5",
        "CIELab:101/0/0",
        "CIELab:-1/0/0",
        "CIELab:100.00002/0/0",
        "CIELuv:105/0/0",
        "TekHVC:0/101/0",
        "TekHVC:0/-1/0",
        "TekHVC:0/50/-1",
        "CIELab:50",
        "cie:1/2/3",
        "CIELab:1e999/0/0",
    ] {
        assert_eq!(
            XColor::parse(spec.as_bytes()),
            XColor::Unresolved(spec.as_bytes()),
            "{spec}"
        );
    }
    // Xcms's tolerance on a range is 0.00001, and the grammar's forms.
    assert_eq!(drawn("CIELab:100.000005/0/0"), Some([0xff; 3]));
    assert_eq!(drawn("rgbi:.5/+5e-1/1E0"), drawn("rgbi:0.5/0.5/1"));
    assert_eq!(drawn("rgbi:1./0/0"), Some([0xff, 0, 0]));
    assert_eq!(drawn("TekHVC:-240/50/20"), drawn("TekHVC:120/50/20"));
    assert_eq!(drawn("TekHVC:840/50/20"), drawn("TekHVC:120/50/20"));
}

/// **Below L* 8 CIELAB is CIE's line, not libX11's**: libX11 gives
/// `CIELab:7/0/0` a Y a hundred times CIE's and draws it near white
/// (`[[source: xparsecolor-obs-xcms-out]]`); CIE's is Y 0.00775, sRGB 21.
#[test]
fn a_dark_lab_colour_is_dark() {
    assert_eq!(drawn("CIELab:7/0/0"), Some([0x15; 3]));
    assert_eq!(drawn("CIELab:7/0/0"), drawn("CIELuv:7/0/0"));
}

/// **A finite value that overflows inside the conversion is refused, not
/// looped on or drawn black.** Each of these is a number the grammar
/// takes whose product or quotient is infinite or NaN further in:
/// libX11's root iterations end on a NaN only because their test is
/// written the other way round, and a NaN channel passes a gamut check
/// built on `min` and `max`.
#[test]
fn an_overflow_inside_the_conversion_is_refused() {
    for spec in [
        "CIEXYZ:1e308/0.5/0",
        "CIEXYZ:-1e308/0.5/1e308",
        "TekHVC:0/0.00001/1e308",
        "CIELab:50/1e300/0",
        "CIELab:50/0/-1e300",
        "CIELuv:0.00001/1e308/0",
        "CIExyY:0.3/1e-300/0.5",
        "CIEuvY:1e308/0.4/0.5",
        "CIEuvY:0.2/1e308/0.5",
    ] {
        match XColor::parse(spec.as_bytes()) {
            XColor::Unresolved(_) => {}
            XColor::Rgb(r, g, b) => {
                // A colour must be the clip's: on the gamut's surface.
                let c = [r.get(), g.get(), b.get()];
                assert!(c.contains(&0) || c.contains(&255), "{spec}: {c:02x?}");
            }
            XColor::Query => panic!("{spec}"),
        }
    }
    // A hue is an angle: 1e308 degrees is 1e308 mod 360, which libX11's
    // loop of 360-degree steps never reaches, 1e308 - 360 being 1e308.
    let turned = format!("TekHVC:{}/50/20", 1e308_f64.rem_euclid(360.0));
    assert_eq!(drawn("TekHVC:1e308/50/20"), drawn(&turned));
    assert!(drawn("TekHVC:1e308/50/20").is_some());
}

/// A value the grammar takes: a plain decimal in the ranges the spaces use,
/// or a wild one with an exponent out to the edges of a double.
fn value() -> impl Strategy<Value = String> {
    prop_oneof![
        (-200i32..12_000, 0u8..3).prop_map(|(v, d)| {
            let x = f64::from(v) / 100.0;
            match d {
                0 => format!("{x:.0}"),
                1 => format!("{x:.1}"),
                _ => format!("{x:.2}"),
            }
        }),
        (
            prop::option::of(prop_oneof![Just('+'), Just('-')]),
            0u64..1_000_000,
            prop::option::of(0u64..1000),
            -320i32..=320
        )
            .prop_map(|(sign, int, frac, exp)| {
                let mut s = sign.map(String::from).unwrap_or_default();
                s += &int.to_string();
                if let Some(f) = frac {
                    s += &format!(".{f}");
                }
                s + &format!("e{exp}")
            }),
    ]
}

proptest! {
    /// **Every spec the grammar takes, in every space and any case,
    /// resolves or is refused, and never through a NaN**: the device
    /// asserts it sees none. Inside sRGB, all but TekHVC (whose trigonometry
    /// is Xcms's own) are the reference's colour.
    #[test]
    fn any_spec_in_the_grammar_resolves_or_is_refused(
        space in 0usize..7,
        upper in prop::collection::vec(any::<bool>(), 6),
        values in prop::collection::vec(value(), 3),
    ) {
        let names = ["rgbi", "ciexyz", "cieuvy", "ciexyy", "cielab", "cieluv", "tekhvc"];
        let prefix: String = names[space]
            .chars()
            .zip(&upper)
            .map(|(c, &u)| if u { c.to_ascii_uppercase() } else { c })
            .collect();
        let spec = format!("{prefix}:{}/{}/{}", values[0], values[1], values[2]);
        let got = drawn(&spec);
        prop_assert_eq!(got, drawn(&spec));
        if names[space] != "tekhvc" {
            if let Some(want) = reference_checked(&spec) {
                prop_assert_eq!(got, Some(want), "{}", spec);
            }
        }
    }
}

/// The reference's colour when every value is in its space's range and the
/// colour is inside sRGB.
fn reference_checked(spec: &str) -> Option<[u8; 3]> {
    let (p, v) = spec.split_once(':')?;
    let v: Vec<f64> = v.split('/').map(|x| x.parse().unwrap()).collect();
    let unit = |x: f64| (0.0..=1.0).contains(&x);
    let ok = match p.to_ascii_lowercase().as_str() {
        "rgbi" => v.iter().all(|&x| unit(x)),
        "ciexyz" => unit(v[1]),
        "cieuvy" => unit(v[2]),
        "ciexyy" => v.iter().all(|&x| unit(x)) && v[1] > 0.0,
        "cielab" => (0.0..=100.0).contains(&v[0]),
        "cieluv" => v[0] > 0.0 && v[0] <= 100.0,
        _ => false,
    };
    if !ok || v.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let r = reference(spec)?;
    let lin = if p.eq_ignore_ascii_case("rgbi") {
        [v[0], v[1], v[2]]
    } else {
        mul(&FROM_XYZ, reference_xyz(spec))
    };
    lin.iter()
        .all(|c| c.is_finite() && (-0.001..=1.001).contains(c))
        .then_some(r)
}
