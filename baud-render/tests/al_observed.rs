//! **Note AL's two analyses of a trail's flash bound, computed here and held
//! to what they printed.** The first is the least opacity at which a trail
//! over an opaque ground makes a WCAG 2.3.1 transition, and how far out each
//! effect term stays above it (`[[source: al-obs-thresholds]]`). The second
//! is the caps prototype O2 carries, the method [`under_threshold`] ships.
//! Python scripts did both once (git blobs a11dc088 and 895ae106), and each
//! line below is a line of theirs: the two blends, each a different one, their
//! rounding and their float widths are kept as they ran.
//!
//! Relative luminance and u'v' are WCAG 2.2's and the Understanding page's,
//! as `baud-bench/src/flash.rs` reads them (`[[source: wcag22]]`):
//!
//! * general: |L(composite) - L(ground)| >= 0.10 with the darker of the two
//!   below 0.80;
//! * red: u'v' distance over the bound and either state a saturated red,
//!   R/(R+G+B) >= 0.8 on the linear values. Black has the white point's u'v'.
//!
//! This is a model of the formulas, not a measurement of frames. No decision
//! here rests on a libm's last bit: over the 2,856 composites either blend
//! reaches, the nearest |dL| to 0.10 is 8.6e-5 off, the nearest u'v' distance
//! to a bound 1.4e-4, the nearest redness to 0.8 8.2e-4 and the nearest darker
//! state to 0.80 9.0e-4; and the nearest printed luminance is 7.2e-6 from a
//! rounding boundary, the nearest printed term 6.5e-5.
use baud_domain::Chan;
use baud_render::{Argb, under_threshold};

/// What the first analysis printed, as committed.
const THRESHOLDS: &str = include_str!("../../docs/research/sources/al-observed/thresholds-out.txt");

/// **What the second printed: 16 lines, and no file holds them**, because the
/// run was never saved as an observation. These are its lines; the a* column
/// is what `under_threshold_is_pinned_for_every_pair` in `src/flash.rs` pins.
const O2_ASTAR: &str = "\
dark     default  gen=0.3775 red@0.2=never red@0.2=never a*=95 (0.3725)
dark     white    gen=0.3159 red@0.2=never red@0.2=never a*=79 (0.3098)
dark     black    gen=never red@0.2=never red@0.2=never a*=255 (1.0000)
dark     red      gen=0.7092 red@0.2=0.2448 red@0.2=0.2448 a*=61 (0.2392)
light    default  gen=0.0625 red@0.2=never red@0.2=never a*=14 (0.0549)
light    white    gen=never red@0.2=never red@0.2=never a*=255 (1.0000)
light    black    gen=0.0523 red@0.2=never red@0.2=never a*=12 (0.0471)
light    red      gen=0.0690 red@0.2=0.7343 red@0.2=0.7343 a*=16 (0.0627)
red      default  gen=0.4468 red@0.2=0.6458 red@0.1=0.3403 a*=85 (0.3333)
red      white    gen=0.3392 red@0.2=0.6059 red@0.1=0.3078 a*=77 (0.3020)
red      black    gen=0.4902 red@0.2=0.9976 red@0.1=0.9976 a*=124 (0.4863)
red      red      gen=never red@0.2=never red@0.1=never a*=255 (1.0000)
darkred  default  gen=0.4005 red@0.2=0.1505 red@0.1=0.0208 a*=4 (0.0157)
darkred  white    gen=0.3384 red@0.2=0.1275 red@0.1=0.0176 a*=3 (0.0118)
darkred  black    gen=never red@0.2=0.9808 red@0.1=0.9808 a*=249 (0.9765)
darkred  red      gen=0.6921 red@0.2=never red@0.1=never a*=175 (0.6863)
";

type Rgb = [u8; 3];

const GROUNDS: [(&str, Rgb); 4] = [
    ("dark", [0x10, 0x10, 0x10]),
    ("light", [0xEF, 0xEF, 0xEF]),
    ("red", [0xCD, 0x00, 0x00]),
    ("darkred", [0x1A, 0x00, 0x00]),
];

/// The first analysis scans the alpha in steps of 1/2000.
const STEPS: u16 = 2000;

/// The red bound on a saturated-red ground, halved from 0.2.
const HALVED: f64 = 0.1;

/// The theme's default foreground is the default ink: #D8D8D8 on the dark
/// theme (dark, red, darkred grounds) and #272727 on the light one.
fn inks(ground: &str) -> [(&'static str, Rgb); 4] {
    let default = if ground == "light" {
        [0x27, 0x27, 0x27]
    } else {
        [0xD8, 0xD8, 0xD8]
    };
    [
        ("default", default),
        ("white", [255, 255, 255]),
        ("black", [0, 0, 0]),
        ("red", [255, 0, 0]),
    ]
}

fn lin(c8: u8) -> f64 {
    let c = f64::from(c8) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear(rgb: Rgb) -> [f64; 3] {
    [lin(rgb[0]), lin(rgb[1]), lin(rgb[2])]
}

fn lum(rgb: Rgb) -> f64 {
    let [r, g, b] = linear(rgb);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn uv(rgb: Rgb) -> [f64; 2] {
    let [r, g, b] = linear(rgb);
    let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b;
    let den = x + 15.0 * y + 3.0 * z;
    if den == 0.0 {
        return uv([255, 255, 255]);
    }
    [4.0 * x / den, 9.0 * y / den]
}

fn sat_red(rgb: Rgb) -> bool {
    let [r, g, b] = linear(rgb);
    let s = r + g + b;
    s > 0.0 && r / s >= 0.8
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// A rounded channel's byte, the arithmetic having kept it in range.
fn byte(v: f64) -> u8 {
    u8::try_from(v as i64).expect("a composite is a byte")
}

fn cell(v: Option<f64>) -> String {
    v.map_or_else(|| "never".to_string(), |v| format!("{v:.4}"))
}

/// The census's blend: premultiplied source over the stored byte, in encoded
/// space, rounded half to even to a byte (`BGRA8Unorm`).
fn over_rounded(ink: Rgb, ground: Rgb, a: f64) -> Rgb {
    let chan = |i: usize| {
        let (c, d) = (f64::from(ink[i]), f64::from(ground[i]));
        byte(((c / 255.0 * a + d / 255.0 * (1.0 - a)) * 255.0).round_ties_even())
    };
    [chan(0), chan(1), chan(2)]
}

/// Least alpha, to 1/`steps`, for a general and for a red transition.
fn least(ink: Rgb, ground: Rgb, steps: u16) -> (Option<f64>, Option<f64>) {
    let (lg, ug) = (lum(ground), uv(ground));
    let (mut general, mut red) = (None, None);
    for i in 0..=steps {
        let a = f64::from(i) / f64::from(steps);
        let c = over_rounded(ink, ground, a);
        let lc = lum(c);
        if general.is_none() && (lc - lg).abs() >= 0.10 && lc.min(lg) < 0.80 {
            general = Some(a);
        }
        if red.is_none() && dist(uv(c), ug) > 0.2 && (sat_red(c) || sat_red(ground)) {
            red = Some(a);
        }
        if general.is_some() && red.is_some() {
            break;
        }
    }
    (general, red)
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Largest x in [lo, hi] with f(x) >= target, f decreasing; `None` if never.
fn solve(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64, target: f64) -> Option<f64> {
    if f(lo) < target {
        return None;
    }
    if f(hi) >= target {
        return Some(hi);
    }
    for _ in 0..60 {
        let m = (lo + hi) / 2.0;
        if f(m) >= target {
            lo = m;
        } else {
            hi = m;
        }
    }
    Some(lo)
}

/// One effect term against the least flashing opacity.
enum Term {
    /// The distance out to which it stays at or above that opacity.
    Reach(Option<f64>),
    /// The opacity it is drawn at.
    Opacity(f64),
}

/// Each term alone at full envelope (burn, env = 1), distances in cell
/// heights from the term's own edge, read from the shaders and the Rust
/// models in baud-render.
fn terms(a_star: f64) -> Vec<(String, Term)> {
    // Fire (fire_flame). Halo: 0.12 exp(-2.5 d0), d0 cells outside the quad.
    let mut out = vec![(
        "fire halo, outside the quad".to_string(),
        Term::Reach(solve(|d| 0.12 * (-2.5 * d).exp(), 0.0, 2.0, a_star)),
    )];
    // Body inside the quad at d0 < -0.25: heat = 0.8 (0.5 + 0.6 n), n the fbm.
    for n in [0.3, 0.5, 0.7] {
        let heat = 0.8 * (0.5 + 0.6 * n);
        let a = (smoothstep(0.05, 0.95, heat) + 0.12).min(1.0);
        out.push((
            format!("fire body inside the quad, n={n}: opacity"),
            Term::Opacity(a),
        ));
    }
    // A tongue: body 1 (the lifted sample deep in the quad), cooling with d0:
    // heat = (1 - 0.85 d0/1.6)(0.5 + 0.6 n); opacity smoothstep + halo.
    for n in [0.6, 0.85] {
        let f = |d: f64| {
            let heat = (1.0 - 0.85 * (d / 1.6).min(1.0)) * (0.5 + 0.6 * n);
            (smoothstep(0.05, 0.95, heat) + 0.12 * (-2.5 * d).exp()).min(1.0)
        };
        out.push((
            format!("fire tongue tip reach above the quad, n={n}"),
            Term::Reach(solve(f, 0.0, 1.6, a_star)),
        ));
    }
    // Orb (orb::light): body 0.45..1.0 inside r = 0.40; halo 0.35 exp(-hd^2),
    // hd = (d - r) / 0.35, cells; comet 0.55 at its head.
    out.push((
        "orb halo reach past its radius (0.40)".to_string(),
        Term::Reach(solve(
            |d| 0.35 * (-(d / 0.35).powi(2)).exp(),
            0.0,
            2.0,
            a_star,
        )),
    ));
    out.push(("orb comet peak".to_string(), Term::Opacity(0.55)));
    out.push(("orb body rim".to_string(), Term::Opacity(0.45)));
    out
}

/// The first analysis's whole output.
fn thresholds(grounds: &[(&'static str, Rgb); 4], steps: u16) -> String {
    let mut out = String::from(
        "Least opacity for a transition, ink over ground (encoded blend, byte-rounded)\n",
    );
    out += &format!(
        "{:<8} {:<8} {:>9} {:>7} {:>11} {:>8}\n",
        "ground", "ink", "ground L", "ink L", "general a*", "red a*"
    );
    let mut table = Vec::new();
    for (g, grgb) in grounds {
        for (i, irgb) in inks(g) {
            let (general, red) = least(irgb, *grgb, steps);
            table.push((*g, i, general));
            out += &format!(
                "{g:<8} {i:<8} {:9.4} {:7.4} {:>11} {:>8}\n",
                lum(*grgb),
                lum(irgb),
                cell(general),
                cell(red)
            );
        }
    }
    out += "\nPer term, at the least general opacity of four pairs (cells from the edge):\n";
    for (ground, ink) in [
        ("light", "black"),
        ("light", "default"),
        ("dark", "white"),
        ("dark", "default"),
    ] {
        let a_star = table
            .iter()
            .find(|(g, i, _)| *g == ground && *i == ink)
            .and_then(|(_, _, general)| *general)
            .expect("each of the four pairs flashes");
        out += &format!("\n  {ink} on {ground}: a* = {a_star:.4}\n");
        for (name, term) in terms(a_star) {
            out += &match term {
                Term::Opacity(v) => {
                    let says = if v >= a_star { "flashes" } else { "does not" };
                    format!("    {name:<52} {v:.3}  ({says})\n")
                }
                Term::Reach(None) => format!("    {name:<52} never\n"),
                Term::Reach(Some(v)) => format!("    {name:<52} {v:.3} cells\n"),
            };
        }
    }
    out
}

/// Through an `f32`, as the census's blend computes.
fn single(x: f64) -> f64 {
    f64::from(x as f32)
}

/// The census's `over` (baud-present-macos/tests/cells.rs), in f32:
/// premultiplied ink over the ground byte, rounded to a byte.
fn over_single(ink: Rgb, ground: Rgb, a: f64) -> Rgb {
    let a = single(a);
    let chan = |i: usize| {
        let (c, d) = (f64::from(ink[i]), f64::from(ground[i]));
        let s = single(single(c / 255.0) * a);
        let dd = single(d / 255.0);
        let v = single(single(s + single(dd * single(1.0 - a))) * 255.0);
        byte((v + 0.5).floor().clamp(0.0, 255.0))
    };
    [chan(0), chan(1), chan(2)]
}

/// **The composites an alpha in [0, 1] can give are one per interval between
/// these**: as the alpha rises continuously the composite changes only where
/// a channel crosses a half integer.
fn breakpoints(ink: Rgb, ground: Rgb) -> Vec<f64> {
    let mut pts = vec![0.0, 1.0];
    for i in 0..3 {
        let (c, d) = (ink[i], ground[i]);
        for n in c.min(d)..c.max(d) {
            let a = (f64::from(n) + 0.5 - f64::from(d)) / (f64::from(c) - f64::from(d));
            if 0.0 < a && a < 1.0 {
                pts.push(a);
            }
        }
    }
    pts.sort_by(f64::total_cmp);
    pts.dedup();
    pts
}

#[derive(Clone, Copy)]
enum Rule {
    General,
    Red(f64),
}

/// The start of the first interval whose composite transitions from the
/// ground under `rule`.
fn first(ink: Rgb, ground: Rgb, rule: Rule) -> Option<f64> {
    let (lg, ug) = (lum(ground), uv(ground));
    let pts = breakpoints(ink, ground);
    pts.windows(2).find_map(|w| {
        let c = over_single(ink, ground, f64::midpoint(w[0], w[1]));
        let flashes = match rule {
            Rule::General => {
                let lc = lum(c);
                (lc - lg).abs() >= 0.10 && lc.min(lg) < 0.80
            }
            Rule::Red(bound) => dist(uv(c), ug) > bound && (sat_red(c) || sat_red(ground)),
        };
        flashes.then_some(w[0])
    })
}

/// a* = ceil(255 a_f) - 2: the largest byte strictly below the first flashing
/// alpha, less one step. 255 when nothing flashes.
struct Cap {
    ground: &'static str,
    ground_rgb: Rgb,
    ink: &'static str,
    ink_rgb: Rgb,
    general: Option<f64>,
    red: Option<f64>,
    halved: f64,
    red_halved: Option<f64>,
    a_star: Chan,
}

fn caps(grounds: &[(&'static str, Rgb); 4], halved_bound: f64) -> Vec<Cap> {
    let mut out = Vec::new();
    for (ground, grgb) in grounds {
        for (ink, irgb) in inks(ground) {
            let general = first(irgb, *grgb, Rule::General);
            let red = first(irgb, *grgb, Rule::Red(0.2));
            let halved = if sat_red(*grgb) { halved_bound } else { 0.2 };
            let red_halved = first(irgb, *grgb, Rule::Red(halved));
            let a_star = [general, red_halved]
                .into_iter()
                .flatten()
                .reduce(f64::min)
                .map_or(255, |a| byte(((a * 255.0).ceil() - 2.0).max(0.0)));
            let a_star = Chan::from_u8(a_star);
            out.push(Cap {
                ground,
                ground_rgb: *grgb,
                ink,
                ink_rgb: irgb,
                general,
                red,
                halved,
                red_halved,
                a_star,
            });
        }
    }
    out
}

/// The second analysis's whole output.
fn o2_astar(caps: &[Cap]) -> String {
    let mut out = String::new();
    for c in caps {
        out += &format!(
            "{:8} {:8} gen={} red@0.2={} red@{}={} a*={} ({:.4})\n",
            c.ground,
            c.ink,
            cell(c.general),
            cell(c.red),
            c.halved,
            cell(c.red_halved),
            c.a_star.get(),
            f64::from(c.a_star.get()) / 255.0
        );
    }
    out
}

/// Where two texts part: 1-based, and the two lines. A text that is a prefix
/// of the other parts at the line after it.
fn first_difference(want: &str, got: &str) -> Option<String> {
    let (mut w, mut g) = (want.split('\n'), got.split('\n'));
    let mut line = 0;
    loop {
        line += 1;
        match (w.next(), g.next()) {
            (None, None) => return None,
            (a, b) if a == b => {}
            (a, b) => return Some(format!("line {line}: want {a:?}, got {b:?}")),
        }
    }
}

fn line_of(difference: Option<String>) -> String {
    let d = difference.expect("the perturbed output must part from the observation");
    d.split(':')
        .next()
        .expect("a split has a first")
        .to_string()
}

fn with_ground(name: &str, rgb: Rgb) -> [(&'static str, Rgb); 4] {
    GROUNDS.map(|(g, was)| (g, if g == name { rgb } else { was }))
}

#[test]
fn the_thresholds_are_what_the_observation_holds() {
    let got = thresholds(&GROUNDS, STEPS);
    assert_eq!(first_difference(THRESHOLDS, &got), None);
    // The positive control: the file is the report, every line of it.
    assert_eq!(THRESHOLDS.lines().count(), 64);
    assert!(THRESHOLDS.contains("dark     white       0.0052  1.0000      0.3160    never"));
}

#[test]
fn the_caps_are_what_the_prototype_printed() {
    let got = o2_astar(&caps(&GROUNDS, HALVED));
    assert_eq!(first_difference(O2_ASTAR, &got), None);
    assert_eq!(O2_ASTAR.lines().count(), 16);
}

/// **What the prototype computed is what ships**, for all sixteen pairs, the
/// line that makes these caps more than a remembered table.
#[test]
fn the_caps_are_what_the_renderer_computes() {
    let argb = |c: Rgb| {
        Argb::rgb(
            Chan::from_u8(c[0]),
            Chan::from_u8(c[1]),
            Chan::from_u8(c[2]),
        )
    };
    let all = caps(&GROUNDS, HALVED);
    assert_eq!(all.len(), 16);
    for c in all {
        assert_eq!(
            under_threshold(argb(c.ink_rgb), argb(c.ground_rgb)).get(),
            c.a_star.get(),
            "{} on {}",
            c.ink,
            c.ground
        );
    }
}

/// A ground one byte off moves the first data row and everything below it.
#[test]
fn a_ground_a_byte_off_parts_from_the_thresholds() {
    let got = thresholds(&with_ground("dark", [0x11, 0x11, 0x11]), STEPS);
    assert_eq!(line_of(first_difference(THRESHOLDS, &got)), "line 3");
}

/// A coarser scan rounds the least alpha up: 0.3775 to 0.3780.
#[test]
fn a_coarser_scan_parts_from_the_thresholds() {
    let got = thresholds(&GROUNDS, 1000);
    assert_eq!(line_of(first_difference(THRESHOLDS, &got)), "line 3");
    assert!(got.contains("0.3780"), "{got}");
}

/// The bound unhalved takes the red ground's default ink from 85 to the
/// general rule's cap, 112, and leaves the rows above it as they were.
#[test]
fn an_unhalved_bound_parts_from_the_caps() {
    let got = o2_astar(&caps(&GROUNDS, 0.2));
    assert_eq!(line_of(first_difference(O2_ASTAR, &got)), "line 9");
    assert!(got.contains("red      default  gen=0.4468 red@0.2=0.6458 red@0.2=0.6458 a*=112"));
}

/// One byte of the dark red ground parts at its second row: the rows above
/// it are other grounds', and its first holds to four decimals.
#[test]
fn a_ground_a_byte_off_parts_from_the_caps() {
    let got = o2_astar(&caps(&with_ground("darkred", [0x1B, 0, 0]), HALVED));
    assert_eq!(line_of(first_difference(O2_ASTAR, &got)), "line 14");
}

/// One character of an observation, and a line it does not carry, are both
/// seen: the comparison is of the whole text.
#[test]
fn a_changed_observation_parts_from_the_computation() {
    let got = thresholds(&GROUNDS, STEPS);
    let changed = THRESHOLDS.replace("0.3160", "0.3161");
    assert_ne!(changed, THRESHOLDS);
    assert_eq!(line_of(first_difference(&changed, &got)), "line 4");
    let short = THRESHOLDS.strip_suffix('\n').expect("a final newline");
    assert_eq!(line_of(first_difference(short, &got)), "line 65");
}
