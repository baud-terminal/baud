//! HarfBuzz's varied outlines, as baud-font's gvar oracle: for each
//! committed variable test font, a sample of its glyphs -- every glyph
//! whose id is a multiple of 37, which reaches simple and composite glyphs
//! alike -- and every glyph of `tests/var/gvar-cases.ttf`, drawn by
//! HarfBuzz at the default instance and at the settings below, as the
//! commands its draw functions emit, in font units (the font's scale is
//! its units per em). The default rows are the control: they say
//! HarfBuzz's emission rule is baud's before a varied row is compared.

use std::path::Path;

use super::{Sample, Setting, outline_rows, weights};
use crate::pyfmt::fixed;
use crate::{BINDING, HARFBUZZ, Oracle, hb};

/// `outlines.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "gvar",
    file: "baud-font/tests/var/outlines.txt",
    lines,
};

/// The cases font's grid crosses each of its regions' edges on both axes.
fn cases() -> Vec<Setting> {
    let mut out = Vec::new();
    for w in [100, 250, 400, 460, 520, 580, 640, 700, 900] {
        for d in [50, 75, 100, 150, 200] {
            if (w, d) != (400, 100) {
                out.push(vec![("wght", w), ("wdth", d)]);
            }
        }
    }
    out
}

/// The draw functions' commands: `M`, `L` and `Q` with every coordinate
/// `{:.4f}`, a cubic as a bare `C`, and `Z`.
struct Commands(Vec<String>);

fn f(x: f32) -> String {
    fixed(f64::from(x), 4)
}

impl hb::Pen for Commands {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(format!("M {} {}", f(x), f(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(format!("L {} {}", f(x), f(y)));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.0
            .push(format!("Q {} {} {} {}", f(cx), f(cy), f(x), f(y)));
    }
    fn cubic_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.0.push("C".to_string());
    }
    fn close(&mut self) {
        self.0.push("Z".to_string());
    }
}

/// The fonts `outlines.txt` samples, in row order. 700 is the weight the
/// embedded Bold is baked at; the others sit between masters and at the
/// axis ends.
pub fn samples() -> Vec<Sample> {
    let sample = |font, settings, step| Sample {
        font,
        settings,
        step,
    };
    vec![
        sample(
            "fonts/JetBrainsMono-Variable.ttf",
            weights(&[100, 350, 700, 800]),
            37,
        ),
        sample(
            "fonts/JetBrainsMono-Italic-Variable.ttf",
            weights(&[100, 700, 800]),
            37,
        ),
        sample("fonts/FiraCode-VF.ttf", weights(&[300, 550, 700]), 37),
        sample("var/gvar-cases.ttf", cases(), 1),
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
        crate::generated_by("gvar"),
    ];
    out.extend(rows(root, &samples()));
    out
}
