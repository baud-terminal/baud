//! Canonical composition and decomposition, `OUT_DIR/compose.rs`, from
//! UnicodeData.txt (`[[source: unicode-data]]`) field 5, the untagged --
//! canonical -- decompositions, and field 3, the canonical combining
//! class, and DerivedNormalizationProps.txt's Full_Composition_Exclusion
//! (`[[source: unicode-dnp]]`):
//!
//! * `COMPOSE`: `(first, second, composed)` for every canonical pair
//!   decomposition whose composite is not excluded -- the primary
//!   composites -- sorted by the pair. Hangul syllables are not listed;
//!   they compose by arithmetic.
//! * `DECOMPOSE`: `(codepoint, first, second)` for every canonical
//!   decomposition, a singleton's `second` 0, sorted by code point. One
//!   level deep: a consumer that needs the full decomposition applies it
//!   again to `first`.
//! * `CCC`: `(first, last, class)` for every code point whose class is not
//!   zero, consecutive lines of one class merged -- what decides whether an
//!   earlier mark blocks a later one from composing with the starter.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{Check, Emit, Errors, Input, Kind, Source, binary_ranges};
use std::collections::HashMap;
use std::fmt::Write as _;

pub const UNICODE_DATA: Input = Input {
    file: "unicode-unicodedata.txt",
    key: "unicode-data",
    header: "0000;<control>;Cc;0;BN;;;;;N;NULL;;;;",
};

pub const NORMALIZATION_PROPS: Input = Input {
    file: "unicode-derivednormalizationprops.txt",
    key: "unicode-dnp",
    header: "# DerivedNormalizationProps-17.0.0.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 2] = [UNICODE_DATA, NORMALIZATION_PROPS];

/// The counts the tables were compiled with: primary composites,
/// canonical decompositions, excluded code points and combining-class
/// ranges.
const COUNTS: [(&str, usize); 4] = [
    ("primary composites", 961),
    ("canonical decompositions", 2081),
    ("Full_Composition_Exclusion code points", 1120),
    ("combining-class ranges", 403),
];

/// What the three tables hold.
#[derive(Debug, Default)]
pub struct Tables {
    pub compose: Vec<(u32, u32, u32)>,
    pub decompose: Vec<(u32, u32, u32)>,
    pub ccc: Vec<(u32, u32, u8)>,
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([ud, dnp]) = <[Source; 2]>::try_from(loaded) else {
        return Err(errs);
    };
    let excluded = match binary_ranges(&dnp, "Full_Composition_Exclusion") {
        Ok(r) => r,
        Err(e) => {
            errs.extend(e);
            Vec::new()
        }
    };
    let is_excluded = |cp: u32| {
        let i = excluded.partition_point(|r| r.hi < cp);
        excluded.get(i).is_some_and(|r| r.lo <= cp)
    };
    let mut t = Tables::default();
    for entry in ud.unicode_data() {
        let Some(e) = errs.take(entry) else {
            continue;
        };
        let class = e.row.field(3);
        let Some(class) = errs.take(class.parse::<u8>().map_err(|_| {
            e.row.error(
                Kind::Malformed,
                3,
                format!("{class:?} is not a combining class, 0 to 254"),
            )
        })) else {
            continue;
        };
        let d = e.row.field(5);
        if !e.range.is_one() && (class != 0 || !d.is_empty()) {
            errs.push(e.row.error(
                Kind::Invariant,
                3,
                "a First/Last range has a combining class or a decomposition",
            ));
            continue;
        }
        let cp = e.range.lo;
        if class != 0 {
            match t.ccc.last_mut() {
                Some(last) if last.1 + 1 == cp && last.2 == class => last.1 = cp,
                _ => t.ccc.push((cp, cp, class)),
            }
        }
        if d.is_empty() || d.starts_with('<') {
            continue;
        }
        let Some(parts) = errs.take(e.row.code_points(5, 0)) else {
            continue;
        };
        match parts[..] {
            [a] => t.decompose.push((cp, a, 0)),
            [a, b] => t.decompose.push((cp, a, b)),
            _ => errs.push(e.row.error(
                Kind::Invariant,
                5,
                format!(
                    "a canonical decomposition of {} code points, 1 or 2 expected",
                    parts.len()
                ),
            )),
        }
    }
    t.decompose.sort_unstable();
    t.compose = t
        .decompose
        .iter()
        .filter(|&&(cp, _, b)| b != 0 && !is_excluded(cp))
        .map(|&(cp, a, b)| (a, b, cp))
        .collect();
    t.compose.sort_unstable();
    for w in t.compose.windows(2) {
        if (w[0].0, w[0].1) == (w[1].0, w[1].1) {
            errs.push(ud.error(
                Kind::Duplicate,
                format!(
                    "U+{:04X} U+{:04X} composes to U+{:04X} and to U+{:04X}",
                    w[0].0, w[0].1, w[0].2, w[1].2
                ),
            ));
        }
    }
    let exclusions: u32 = excluded.iter().map(|r| r.hi - r.lo + 1).sum();
    let got = [
        t.compose.len(),
        t.decompose.len(),
        usize::try_from(exclusions).expect("a count of code points"),
        t.ccc.len(),
    ];
    for ((what, want), got) in COUNTS.into_iter().zip(got) {
        errs.count(ud.path(), what, got, want);
    }
    controls(&ud, &t, &mut errs);
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut out = Emit::new(
        "compose",
        "crates/baud-font/build.rs (build/compose.rs)",
        &[&ud, &dnp],
    );
    out.code(&rust(&t));
    errs.or(out)
}

/// Positive controls: e + U+0301 and A + U+030A compose to U+00E9 and
/// U+00C5; U+0915 + U+093C does not compose to U+0958, which is excluded
/// and still decomposes to it; U+212B ANGSTROM SIGN decomposes to U+00C5
/// alone; and U+0301, U+0323 and U+0F71 have classes 230, 220 and 129,
/// where `a` has none.
fn controls(ud: &Source, t: &Tables, errs: &mut Errors) {
    let cm: HashMap<(u32, u32), u32> = t.compose.iter().map(|&(a, b, c)| ((a, b), c)).collect();
    let dm: HashMap<u32, (u32, u32)> = t.decompose.iter().map(|&(c, a, b)| (c, (a, b))).collect();
    let class = |cp: u32| {
        t.ccc
            .iter()
            .find(|&&(lo, hi, _)| lo <= cp && cp <= hi)
            .map(|&(_, _, c)| c)
    };
    let checks: [(bool, String); 9] = [
        (
            cm.get(&(0x65, 0x301)) == Some(&0xE9),
            "e + U+0301 composes to U+00E9".into(),
        ),
        (
            cm.get(&(0x41, 0x30A)) == Some(&0xC5),
            "A + U+030A composes to U+00C5".into(),
        ),
        (
            !cm.contains_key(&(0x915, 0x93C)),
            "U+0915 + U+093C composes to nothing, U+0958 being excluded".into(),
        ),
        (
            dm.get(&0x958) == Some(&(0x915, 0x93C)),
            "U+0958 decomposes to U+0915 U+093C".into(),
        ),
        (
            dm.get(&0x212B) == Some(&(0xC5, 0)),
            "U+212B decomposes to U+00C5 alone".into(),
        ),
        (class(0x0301) == Some(230), "U+0301 has class 230".into()),
        (class(0x0323) == Some(220), "U+0323 has class 220".into()),
        (class(0x0061).is_none(), "U+0061 has class 0".into()),
        (class(0x0F71) == Some(129), "U+0F71 has class 129".into()),
    ];
    for (holds, what) in checks {
        errs.require(Check::from(holds), || {
            ud.error(Kind::Control, format!("{what}: it does not"))
        });
    }
}

/// The three tables, as const slices.
pub fn rust(t: &Tables) -> String {
    let mut rs = String::from(
        "/// `(first, second, composed)`, sorted by `(first, second)`: the primary\n\
         /// composites. Hangul syllables are not listed; they compose by\n\
         /// arithmetic.\n\
         pub const COMPOSE: &[(u32, u32, u32)] = &[\n",
    );
    for (a, b, c) in &t.compose {
        let _ = writeln!(rs, "    (0x{a:04X}, 0x{b:04X}, 0x{c:04X}),");
    }
    rs.push_str(
        "];\n\n\
         /// `(codepoint, first, second)`, sorted by codepoint; `second` is 0 for a\n\
         /// singleton. One level deep.\n\
         pub const DECOMPOSE: &[(u32, u32, u32)] = &[\n",
    );
    for (cp, a, b) in &t.decompose {
        let _ = writeln!(rs, "    (0x{cp:04X}, 0x{a:04X}, 0x{b:04X}),");
    }
    rs.push_str(
        "];\n\n\
         /// `(first, last, class)`, sorted, inclusive, for every non-zero canonical\n\
         /// combining class.\n\
         pub const CCC: &[(u32, u32, u8)] = &[\n",
    );
    for (lo, hi, c) in &t.ccc {
        let _ = writeln!(rs, "    (0x{lo:04X}, 0x{hi:04X}, {c}),");
    }
    rs.push_str("];\n");
    rs
}
