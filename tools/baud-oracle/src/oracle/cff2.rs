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
//!
//! The port of `scripts/cff2-oracle.py`, which stays beside it while
//! LICENSE names it. **Run it with `--check`**: written, the file's header
//! would name this bin, and LICENSE says the script wrote the file.

use std::path::Path;

use crate::pyfmt::repr;
use crate::{BINDING, HARFBUZZ, Oracle, hb, label};

/// `cff2-outlines.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "cff2",
    file: "baud-font/tests/var/cff2-outlines.txt",
    lines,
};

type Setting = Vec<(&'static str, i32)>;

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

fn lines(root: &Path) -> Vec<String> {
    crate::pinned(None);
    // Source Code VF's one axis is wght 200-900, default 200; 637 sits
    // between masters.
    let weights: Vec<Setting> = [250, 400, 637, 900]
        .iter()
        .map(|&w| vec![("wght", w)])
        .collect();
    let fonts: [(&str, Vec<Setting>, u32); 2] = [
        ("fonts/SourceCodeVF-Upright.otf", weights, 7),
        ("var/cff2-cases.otf", cases(), 1),
    ];
    let mut out = vec![
        format!(
            "# font  axis=value,... (or - for the default)  glyph  commands  (HarfBuzz {HARFBUZZ}, {BINDING})"
        ),
        crate::generated_by("cff2"),
    ];
    for (name, settings, step) in &fonts {
        let blob = hb::Blob::from_file(&root.join("baud-font/tests").join(name));
        let face = hb::Face::new(&blob);
        for setting in std::iter::once(None).chain(settings.iter().map(Some)) {
            let mut font = hb::Font::new(&face);
            font.set_scale(face.upem(), face.upem());
            let mut l = "-".to_string();
            if let Some(s) = setting {
                font.set_variations(&crate::as_float(s));
                l = label(s);
            }
            for gid in (0..face.glyph_count()).step_by(*step as usize) {
                let mut c = Commands(Vec::new());
                font.draw(gid, &mut c);
                out.push(format!("{name} {l} {gid} {}", c.0.join(" ")));
            }
        }
    }
    out
}
