//! **The specs fed to libX11, computed here and held to the first column of
//! what it answered**: `[[source: xparsecolor-obs-rgb-out]]` and
//! `[[source: xparsecolor-obs-xcms-out]]`, each line the spec the probe
//! (`[[source: xparsecolor-obs-probe-c]]`) read from stdin. Python scripts
//! wrote both corpora once (git blobs 23c5314e and cc086db0); the lists below
//! are theirs, in their order, so what `x_parse_color.rs` and
//! `src/vt/xcms/tests.rs` hold baud to is a corpus that can be read and
//! extended rather than a file of unknown origin.

const RGB_OUT: &str = include_str!("../../docs/research/sources/xparsecolor-observed/rgb.out");
const XCMS_OUT: &str = include_str!("../../docs/research/sources/xparsecolor-observed/xcms.out");

/// One to four hex digits a component, and the stride each width is walked at.
const STRIDES: [(usize, usize); 4] = [(1, 1), (2, 1), (3, 13), (4, 251)];

/// The values each `rgbi:` component takes on the grid.
const RGBI_GRID: [f64; 6] = [0.0, 0.05, 0.2, 0.5, 0.8, 1.0];

/// The spec column of a probe's output.
fn specs_of(out: &str) -> String {
    let mut specs = String::new();
    for line in out.lines() {
        specs += line.split('\t').next().expect("a split has a first");
        specs.push('\n');
    }
    specs
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
    let d = difference.expect("the perturbed corpus must part from the observation");
    d.split(':')
        .next()
        .expect("a split has a first")
        .to_string()
}

fn lines_of(specs: &[String]) -> String {
    let mut out = specs.join("\n");
    out.push('\n');
    out
}

/// Each three values a colour, the last padded with zeros.
fn pack(vals: &[u32], w: usize, fmt: impl Fn(&[String]) -> String) -> Vec<String> {
    vals.chunks(3)
        .map(|t| {
            let mut hex: Vec<String> = t.iter().map(|v| format!("{v:0w$x}")).collect();
            while hex.len() < 3 {
                hex.push(format!("{:0w$x}", 0));
            }
            fmt(&hex)
        })
        .collect()
}

/// The `rgb:` and `#` corpus: the manual's examples, prefix case, every one-
/// and two-digit value, a stride of three- and four-digit values, then mixed
/// widths and malformed forms.
fn rgb_specs(strides: &[(usize, usize)]) -> String {
    let mut l: Vec<String> = [
        "#3a7",
        "#3000a0007000",
        "rgb:00/ff/00",
        "rgb:ea/75/52",
        "rgb:ccc/320/320",
        "rgb:ff/a5/0",
        "rgb:ccc/32/0",
        "RGB:FF/00/00",
        "Rgb:ff/A5/0",
        "rGb:CcC/320/320",
        "#3A7",
        "#ABCDEF",
    ]
    .map(String::from)
    .to_vec();
    for &(w, step) in strides {
        let top = 1_u32 << (4 * w);
        let mut vals: Vec<u32> = (0..top).step_by(step).collect();
        // The top value is on the list however the stride falls, and twice
        // when it lands there.
        vals.push(top - 1);
        l.extend(pack(&vals, w, |t| format!("rgb:{}", t.join("/"))));
        l.extend(pack(&vals, w, |t| format!("#{}", t.join(""))));
    }
    l.extend(
        [
            "rgb:f/ff/fff",
            "rgb:fff/ff/f",
            "rgb:1/22/333",
            "rgb:8000/800/80",
            "rgb:0081/009/0",
            "rgb:",
            "rgb:1//2",
            "rgb:/1/2",
            "rgb:1/2",
            "rgb:12345/0/0",
            "rgb:g/0/0",
            "rgb:1/2/3/",
            "rgb:1/2/3/4",
            "#",
            "#f",
            "#ff",
            "#ffff",
            "#fffff",
            "#fffffffffffffff",
            "#ggg",
            "# fff",
            "#fff ",
            "rgb: 1/2/3",
        ]
        .map(String::from),
    );
    lines_of(&l)
}

/// A number as the corpus spells it: a whole one bare, and otherwise four
/// decimals with the zeros at the end taken off.
fn f(v: f64) -> String {
    if v == v.trunc() {
        format!("{}", v as i64)
    } else {
        format!("{v:.4}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

/// `prefix:a/b/c` for each of the three lists' product, the last fastest.
fn product(l: &mut Vec<String>, prefix: &str, xs: &[f64], ys: &[f64], zs: &[f64]) {
    for x in xs {
        for y in ys {
            for z in zs {
                l.push(format!("{prefix}:{}/{}/{}", f(*x), f(*y), f(*z)));
            }
        }
    }
}

/// The `rgbi:` and device-independent corpus: the manual's examples, prefix
/// case, a grid in each space, then axis and wrap cases and malformed forms.
fn xcms_specs(rgbi_grid: &[f64]) -> String {
    let mut l: Vec<String> = [
        "CIEXYZ:0.3227/0.28133/0.2493",
        "RGBi:1.0/0.0/0.0",
        "CIELuv:50.0/0.0/0.0",
        "ciexyz:0.3227/0.28133/0.2493",
        "rgbi:1/0/0",
        "CieLab:50/0/0",
        "TEKHVC:0/100/0",
        "cieUVY:0.2/0.5/0.5",
        "CIExyy:0.3127/0.329/0.5",
    ]
    .map(String::from)
    .to_vec();
    product(&mut l, "rgbi", rgbi_grid, rgbi_grid, rgbi_grid);
    for i in (0..101).step_by(4) {
        let v = f(f64::from(i) / 100.0);
        l.push(format!("rgbi:{v}/{v}/{v}"));
    }
    product(
        &mut l,
        "CIEXYZ",
        &[0.0, 0.1, 0.3, 0.6, 0.95],
        &[0.0, 0.05, 0.3, 0.7, 1.0],
        &[0.0, 0.1, 0.5, 1.1],
    );
    product(
        &mut l,
        "CIEuvY",
        &[0.15, 0.19, 0.25, 0.4],
        &[0.35, 0.45, 0.5, 0.55],
        &[0.05, 0.3, 0.8],
    );
    product(
        &mut l,
        "CIExyY",
        &[0.2, 0.3127, 0.45, 0.6],
        &[0.2, 0.329, 0.45, 0.6],
        &[0.1, 0.5, 1.0],
    );
    product(
        &mut l,
        "CIELab",
        &[0.0, 3.0, 7.0, 8.0, 20.0, 50.0, 75.0, 100.0],
        &[-0.5, -0.1, 0.0, 0.1, 0.4],
        &[-0.5, 0.0, 0.3],
    );
    product(
        &mut l,
        "CIELab",
        &[30.0, 60.0],
        &[-80.0, 40.0],
        &[-60.0, 70.0],
    );
    product(
        &mut l,
        "CIELuv",
        &[0.0, 5.0, 10.0, 50.0, 90.0, 100.0],
        &[-0.5, 0.0, 0.2, 0.6],
        &[-0.4, 0.0, 0.3],
    );
    let hues: Vec<f64> = (0..360).step_by(30).map(f64::from).collect();
    product(
        &mut l,
        "TekHVC",
        &hues,
        &[5.0, 20.0, 50.0, 80.0, 95.0],
        &[0.0, 10.0, 30.0, 60.0, 100.0],
    );
    l.extend(
        [
            "TekHVC:-30/50/20",
            "TekHVC:390/50/20",
            "TekHVC:725.5/40/10",
            "TekHVC:0/0/0",
            "TekHVC:0/100/50",
            "rgbi:.5/5./+1e-1",
            "rgbi:1E0/0/0",
            "CIELab:5e1/0/0",
            "CIELab:-0/0/0",
            "rgbi:",
            "rgbi:1/0",
            "rgbi:1//0",
            "rgbi:a/0/0",
            "rgbi:./0/0",
            "rgbi:1e/0/0",
            "rgbi:-0.5/0/0",
            "CIELab:101/0/0",
            "CIELab:-1/0/0",
            "CIEXYZ:0/1.5/0",
            "CIEXYZ:0/-0.5/0",
            "CIEuvY:0.2/0.4/1.5",
            "CIExyY:1.2/0.3/0.5",
            "CIExyY:0.3/-0.1/0.5",
            "TekHVC:0/101/0",
            "TekHVC:0/-1/0",
            "TekHVC:0/50/-1",
            "CIELuv:-5/0/0",
            "CIELuv:105/0/0",
            "cie:1/2/3",
            "xyz:1/2/3",
            "CIEXYZ:",
            "CIELab:50/0",
        ]
        .map(String::from),
    );
    lines_of(&l)
}

#[test]
fn the_rgb_corpus_is_the_column_the_oracle_answered() {
    let want = specs_of(RGB_OUT);
    assert_eq!(first_difference(&want, &rgb_specs(&STRIDES)), None);
    // The positive control: the manifest's count of the file's lines.
    assert_eq!(want.lines().count(), 607);
}

#[test]
fn the_xcms_corpus_is_the_column_the_oracle_answered() {
    let want = specs_of(XCMS_OUT);
    assert_eq!(first_difference(&want, &xcms_specs(&RGBI_GRID)), None);
    assert_eq!(want.lines().count(), 979);
}

/// A stride of 14 for the three-digit values parts at their first line:
/// `rgb:000/00d/01a` against `rgb:000/00e/01c`.
#[test]
fn a_changed_stride_parts_from_the_rgb_corpus() {
    let got = rgb_specs(&[(1, 1), (2, 1), (3, 14), (4, 251)]);
    assert_eq!(
        line_of(first_difference(&specs_of(RGB_OUT), &got)),
        "line 197"
    );
    assert!(got.contains("rgb:000/00e/01c\n"));
}

/// The four-digit walk taken off parts where its first line would have been.
#[test]
fn a_missing_width_parts_from_the_rgb_corpus() {
    let got = rgb_specs(&STRIDES[..3]);
    assert_eq!(
        line_of(first_difference(&specs_of(RGB_OUT), &got)),
        "line 409"
    );
}

/// One grid value moved parts at the grid's second line, the first it reaches.
#[test]
fn a_changed_grid_parts_from_the_xcms_corpus() {
    let grid = [0.0, 0.06, 0.2, 0.5, 0.8, 1.0];
    let got = xcms_specs(&grid);
    assert_eq!(
        line_of(first_difference(&specs_of(XCMS_OUT), &got)),
        "line 11"
    );
}

/// A spec's spelling is part of it: `f`'s stripping is what the file has.
#[test]
fn a_number_is_spelled_as_the_corpus_spells_it() {
    assert_eq!(f(0.0), "0");
    assert_eq!(f(-80.0), "-80");
    assert_eq!(f(0.28), "0.28");
    assert_eq!(f(-0.5), "-0.5");
    assert_eq!(f(0.3127), "0.3127");
    assert!(XCMS_OUT.contains("CIELab:60/-80/70\t"));
    assert!(XCMS_OUT.contains("rgbi:0.28/0.28/0.28\t"));
}

/// One character of a spec in the observation is seen, and so is a line it
/// no longer has: the comparison is of the whole column.
#[test]
fn a_changed_observation_parts_from_the_corpus() {
    let got = xcms_specs(&RGBI_GRID);
    let changed = specs_of(XCMS_OUT).replace("rgbi:1/0/0\n", "rgbi:1/0/1\n");
    assert_eq!(line_of(first_difference(&changed, &got)), "line 5");
    let short = specs_of(XCMS_OUT);
    let short = short.strip_suffix('\n').expect("a final newline");
    assert_eq!(line_of(first_difference(short, &got)), "line 980");
}
