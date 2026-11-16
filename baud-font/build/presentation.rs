//! The presentation-form tables, `OUT_DIR/presentation.rs`: the Arabic
//! presentation forms a face with no `GSUB` forms is drawn through. Each
//! character whose UnicodeData.txt decomposition is `<isolated>`,
//! `<final>`, `<initial>` or `<medial>` of one code point, keyed by that
//! code point and the form, and the eight lam-alef ligatures U+FEF5..
//! U+FEFC, `<isolated>` or `<final>` of lam and a letter of the ALEF
//! joining group (`[[source: unicode-data]]`,
//! `[[source: unicode-arabicshaping]]`).
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{Check, Emit, Errors, Input, Kind, PerLine, Source};
use std::collections::{BTreeMap, HashMap};

pub const UNICODE_DATA: Input = Input {
    file: "unicode-unicodedata.txt",
    key: "unicode-data",
    header: "0000;<control>;Cc;0;BN;;;;;N;NULL;;;;",
};

pub const ARABIC_SHAPING: Input = Input {
    file: "unicode-arabicshaping.txt",
    key: "unicode-arabicshaping",
    header: "# ArabicShaping-17.0.0.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 2] = [UNICODE_DATA, ARABIC_SHAPING];

/// The decomposition tags of the four forms (UAX #44 Table 14,
/// `[[source: uax44]]`), in the order the key's low two bits number them.
pub const FORMS: [&str; 4] = ["<isolated>", "<final>", "<initial>", "<medial>"];

/// Lam, the first of a lam-alef ligature's decomposition.
const LAM: u32 = 0x0644;

/// The ALEF joining group's letters, from ArabicShaping.txt's field 3.
fn alefs(shaping: &Source, errs: &mut Errors) -> Vec<u32> {
    let mut out = Vec::new();
    for row in shaping.rows() {
        let cp = errs.take(row.expect(4).and_then(|()| row.code_point(0)));
        if let Some(cp) = cp.filter(|_| row.field(3) == "ALEF") {
            out.push(cp);
        }
    }
    out
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([ud, shaping]) = <[Source; 2]>::try_from(loaded) else {
        return Err(errs);
    };
    let alef = alefs(&shaping, &mut errs);
    let mut single: HashMap<(u32, u32), (u32, usize)> = HashMap::new();
    let mut lam_alef: BTreeMap<u32, [u32; 2]> = BTreeMap::new();
    for entry in ud.unicode_data() {
        let Some(e) = errs.take(entry) else {
            continue;
        };
        let d = e.row.field(5);
        let Some((form, tag)) = FORMS
            .iter()
            .enumerate()
            .find_map(|(i, t)| d.strip_prefix(t).map(|_| (i, t.len())))
        else {
            continue;
        };
        let Some(of) = errs.take(e.row.code_points(5, tag)) else {
            continue;
        };
        let form = u32::try_from(form).expect("four forms");
        let cp = e.range.lo;
        if of.len() == 1 {
            if let Some((was, line)) = single.get(&(of[0], form)) {
                errs.push(e.row.error(
                    Kind::Duplicate,
                    5,
                    format!(
                        "U+{:04X} in that form is already U+{was:04X} on line {line} ({:?})",
                        of[0],
                        ud.line(*line)
                    ),
                ));
                continue;
            }
            single.insert((of[0], form), (cp, e.row.line()));
        } else if of.len() == 2 && of[0] == LAM && alef.contains(&of[1]) {
            if form > 1 {
                errs.push(e.row.error(
                    Kind::Invariant,
                    5,
                    "a lam-alef ligature is <isolated> or <final>",
                ));
                continue;
            }
            lam_alef.entry(of[1]).or_insert([0, 0])[usize::try_from(form).expect("0 or 1")] = cp;
        }
    }
    // Positive controls: beh's four forms are U+FE8F..U+FE92, peh's
    // (Presentation Forms-A) U+FB56..U+FB59, and lam with alef U+FEFB and
    // U+FEFC, of four.
    for (letter, want) in [
        (0x0628, [0xFE8F, 0xFE90, 0xFE91, 0xFE92]),
        (0x067E, [0xFB56, 0xFB57, 0xFB58, 0xFB59]),
    ] {
        let got: Vec<Option<u32>> = (0..4)
            .map(|f| single.get(&(letter, f)).map(|&(cp, _)| cp))
            .collect();
        errs.require(Check::from(got == want.map(Some)), || {
            ud.error(
                Kind::Control,
                format!("U+{letter:04X}'s forms are {got:X?}, {want:X?} expected"),
            )
        });
    }
    let got = lam_alef.get(&0x0627).copied();
    errs.require(Check::from(got == Some([0xFEFB, 0xFEFC])), || {
        ud.error(
            Kind::Control,
            format!("lam with alef is {got:X?}, [FEFB, FEFC] expected"),
        )
    });
    errs.count(ud.path(), "lam-alef ligatures' alefs", lam_alef.len(), 4);
    if !errs.is_empty() {
        return Err(errs);
    }

    let mut keys: Vec<(u32, u32)> = single
        .iter()
        .map(|(&(base, form), &(cp, _))| ((base << 2) | form, cp))
        .collect();
    keys.sort_unstable();
    let ligatures: Vec<(u32, u32, u32)> = lam_alef.iter().map(|(&a, v)| (a, v[0], v[1])).collect();
    let mut out = Emit::new(
        "presentation",
        "baud-font/build.rs (build/presentation.rs)",
        &[&ud, &shaping],
    );
    out.array(
        "FORMS",
        "(u32, u32)",
        &[
            "Each presentation form whose UnicodeData.txt decomposition is",
            "`<isolated>`, `<final>`, `<initial>` or `<medial>` of one code point",
            "(`[[source: unicode-data]]`), sorted by key: the code point shifted",
            "left two, with the form below it (0 isolated, 1 final, 2 initial,",
            "3 medial), and the presentation form.",
        ],
        &keys,
        PerLine(4),
        |(k, p)| format!("(0x{k:06X}, 0x{p:04X})"),
    );
    out.array(
        "LAM_ALEF",
        "(u32, u32, u32)",
        &[
            "The lam-alef ligatures U+FEF5..U+FEFC: each alef lam forms one with,",
            "and the ligature's `<isolated>` and `<final>` forms.",
        ],
        &ligatures,
        PerLine(1),
        |(a, n, j)| format!("(0x{a:04X}, 0x{n:04X}, 0x{j:04X})"),
    );
    errs.or(out)
}
