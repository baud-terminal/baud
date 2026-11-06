//! HarfBuzz's CFF2 outlines, as baud-font's CFF2 oracle: every glyph whose
//! id is a multiple of 7 in Source Code VF
//! (`tests/fonts/SourceCodeVF-Upright.otf`), and every glyph of
//! `tests/var/cff2-cases.otf`, drawn by HarfBuzz at the default instance
//! and at the settings below, as the commands its draw functions emit, in
//! font units (the font's scale is its units per em). Every coordinate is
//! printed as Python's `repr` of the C `float` HarfBuzz handed over, widened
//! to a double, which is the float exactly, so the comparison can be exact;
//! a cubic carries all six of its coordinates. The default rows are the
//! control: they say HarfBuzz's emission rule is baud's before a varied row
//! is compared.

use std::path::Path;

use super::{Sample, Setting, outline_rows, weights};
use crate::pyfmt::repr;
use crate::{BINDING, HARFBUZZ, Oracle, hb};

/// `cff2-outlines.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "cff2",
    file: "baud-font/tests/var/cff2-outlines.txt",
    lines,
};

/// The cases font's grid crosses each of its regions' peaks and edges on
/// both axes.
fn cases() -> Vec<Setting> {
    let mut out = Vec::new();
    for w in [100, 250, 400, 525, 650, 900] {
        for d in [50, 100, 137, 200] {
            if (w, d) != (400, 100) {
                out.push(vec![("wght", w), ("wdth", d)]);
            }
        }
    }
    out
}

struct Commands(Vec<String>);

fn r(x: f32) -> String {
    repr(f64::from(x))
}

impl hb::Pen for Commands {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(format!("M {} {}", r(x), r(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(format!("L {} {}", r(x), r(y)));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.0
            .push(format!("Q {} {} {} {}", r(cx), r(cy), r(x), r(y)));
    }
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.0.push(format!(
            "C {} {} {} {} {} {}",
            r(c1x),
            r(c1y),
            r(c2x),
            r(c2y),
            r(x),
            r(y)
        ));
    }
    fn close(&mut self) {
        self.0.push("Z".to_string());
    }
}

/// The fonts `cff2-outlines.txt` samples, in row order. Source Code VF's
/// one axis is wght 200-900, default 200; 637 sits between masters.
pub fn samples() -> Vec<Sample> {
    vec![
        Sample {
            font: "fonts/SourceCodeVF-Upright.otf",
            settings: weights(&[250, 400, 637, 900]),
            step: 7,
        },
        Sample {
            font: "var/cff2-cases.otf",
            settings: cases(),
            step: 1,
        },
    ]
}

/// `samples`' data rows, the file without its header.
pub fn rows(root: &Path, samples: &[Sample]) -> Vec<String> {
    crate::pinned(None);
    outline_rows(root, samples, |font, gid| {
        let mut c = Commands(Vec::new());
        font.draw(gid, &mut c);
        c.0
    })
}

fn lines(root: &Path) -> Vec<String> {
    let mut out = vec![
        format!(
            "# font  axis=value,... (or - for the default)  glyph  commands  (HarfBuzz {HARFBUZZ}, {BINDING})"
        ),
        crate::generated_by("cff2"),
    ];
    out.extend(rows(root, &samples()));
    out
}
