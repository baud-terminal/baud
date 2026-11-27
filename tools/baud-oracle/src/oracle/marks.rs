//! HarfBuzz's mark positions at a variation, as baud-font's GPOS oracle:
//! each committed variable test font shaped by HarfBuzz with the mark
//! features on and everything else a grid cannot use off -- the feature
//! set `tests/shape/gpos.txt` uses -- at the default instance and at the
//! settings below. Each row is one text: its codepoints, then per glyph
//! the id, cluster, x and y offset and x advance, in font units (the
//! font's scale is its units per em). The texts are every letter of the
//! fonts' Latin coverage under one mark from a set of combining marks above
//! and below, and some stacks of two, which is where mark-to-mark anchors
//! move. The default rows are the control that the two agree before a
//! varied row is compared.

use std::path::Path;

use crate::{BINDING, HARFBUZZ, Oracle, hb, label};

/// `marks.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "marks",
    file: "crates/baud-font/tests/var/marks.txt",
    lines,
};

/// The features, in the script's dict order: on, then off.
const FEATURES: [(&str, u32); 10] = [
    ("mark", 1),
    ("mkmk", 1),
    ("liga", 0),
    ("clig", 0),
    ("calt", 0),
    ("kern", 0),
    ("locl", 0),
    ("rclt", 0),
    ("dlig", 0),
    ("hlig", 0),
];

const MARKS: [u32; 16] = [
    0x0300, 0x0301, 0x0302, 0x0303, 0x0304, 0x0306, 0x0307, 0x0308, 0x030A, 0x030B, 0x030C, 0x0323,
    0x0326, 0x0327, 0x0328, 0x0331,
];

fn texts() -> Vec<Vec<u32>> {
    let bases = (0x41..0x5B).chain(0x61..0x7B).chain([0x131, 0x237]);
    let mut out: Vec<Vec<u32>> = bases
        .flat_map(|b| MARKS.iter().map(move |&m| vec![b, m]))
        .collect();
    for b in [0x61, 0x6F, 0x75, 0x41, 0x4F, 0x69] {
        for (m1, m2) in [
            (0x0308, 0x0301),
            (0x0302, 0x0303),
            (0x0323, 0x0302),
            (0x0304, 0x0300),
            (0x0327, 0x0306),
        ] {
            out.push(vec![b, m1, m2]);
        }
    }
    out
}

/// `var/ink-cases.ttf` has no GPOS, so its marks are placed by the fallback
/// against varied glyph boxes; it maps two bases and six marks, so it takes
/// its own texts.
fn ink_texts() -> Vec<Vec<u32>> {
    let marks = [0x0300, 0x0301, 0x0304, 0x030A, 0x0323, 0x0327];
    let mut out: Vec<Vec<u32>> = [0x61, 0x6F]
        .into_iter()
        .flat_map(|b| marks.iter().map(move |&m| vec![b, m]))
        .collect();
    for b in [0x61, 0x6F] {
        for (m1, m2) in [
            (0x0304, 0x0301),
            (0x0301, 0x030A),
            (0x0323, 0x0301),
            (0x0327, 0x0300),
        ] {
            out.push(vec![b, m1, m2]);
        }
    }
    out
}

type Setting = Vec<(&'static str, i32)>;

/// A font, the settings it is shaped at, and the texts.
type Run = (&'static str, Vec<Setting>, Vec<Vec<u32>>);

fn weights(w: &[i32]) -> Vec<Setting> {
    w.iter().map(|&w| vec![("wght", w)]).collect()
}

fn lines(root: &Path) -> Vec<String> {
    crate::pinned(None);
    let tests = root.join("crates/baud-font/tests");
    let fonts: [Run; 4] = [
        (
            "fonts/JetBrainsMono-Variable.ttf",
            weights(&[100, 350, 700, 800]),
            texts(),
        ),
        (
            "fonts/JetBrainsMono-Italic-Variable.ttf",
            weights(&[100, 700, 800]),
            texts(),
        ),
        ("fonts/FiraCode-VF.ttf", weights(&[300, 550, 700]), texts()),
        (
            "var/ink-cases.ttf",
            weights(&[100, 250, 650, 900]),
            ink_texts(),
        ),
    ];
    let features: Vec<hb::Feature> = FEATURES.iter().map(|&(t, v)| hb::feature(t, v)).collect();
    let mut out = vec![
        format!(
            "# font  axis=value,... (or - for the default)  codepoints  glyph,cluster,dx,dy,ax;...  (HarfBuzz {HARFBUZZ}, {BINDING})"
        ),
        crate::generated_by("marks"),
    ];
    for (name, settings, texts) in &fonts {
        let blob = hb::Blob::from_file(&tests.join(name));
        let face = hb::Face::new(&blob);
        for s in std::iter::once(&Vec::new()).chain(settings) {
            let mut font = hb::Font::new(&face);
            if !s.is_empty() {
                font.set_variations(&crate::as_float(s));
            }
            let l = label(s);
            for text in texts {
                let mut buf = hb::Buffer::new();
                buf.add_codepoints(text);
                buf.shape(&font, &features);
                let glyphs = buf
                    .glyphs()
                    .iter()
                    .map(|(i, p)| {
                        format!(
                            "{},{},{},{},{}",
                            i.codepoint, i.cluster, p.x_offset, p.y_offset, p.x_advance
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(";");
                let cps = text
                    .iter()
                    .map(|c| format!("{c:04X}"))
                    .collect::<Vec<_>>()
                    .join(",");
                out.push(format!("{name} {l} {cps} {glyphs}"));
            }
        }
    }
    out
}
