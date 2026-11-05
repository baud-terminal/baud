//! HarfBuzz's varied outlines, as baud-font's gvar oracle: for each
//! committed variable test font, a sample of its glyphs -- every glyph
//! whose id is a multiple of 37, which reaches simple and composite glyphs
//! alike -- and every glyph of `tests/var/gvar-cases.ttf`, drawn by
//! HarfBuzz at the default instance and at the settings below, as the
//! commands its draw functions emit, in font units (the font's scale is
//! its units per em). The default rows are the control: they say
//! HarfBuzz's emission rule is baud's before a varied row is compared.
//!
//! The port of `scripts/gvar-oracle.py`, which stays beside it while
//! LICENSE names it. **Run it with `--check`**: written, the file's header
//! would name this bin, and LICENSE says the script wrote the file.

use std::path::Path;

use crate::pyfmt::fixed;
use crate::{BINDING, HARFBUZZ, Oracle, hb, label};

/// `outlines.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "gvar",
    file: "baud-font/tests/var/outlines.txt",
    lines,
};

type Setting = Vec<(&'static str, i32)>;

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

fn weights(w: &[i32]) -> Vec<Setting> {
    w.iter().map(|&w| vec![("wght", w)]).collect()
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

fn lines(root: &Path) -> Vec<String> {
    crate::pinned(None);
    // 700 is the weight the embedded Bold is baked at; the others sit
    // between masters and at the axis ends.
    let fonts: [(&str, Vec<Setting>, u32); 4] = [
        (
            "fonts/JetBrainsMono-Variable.ttf",
            weights(&[100, 350, 700, 800]),
            37,
        ),
        (
            "fonts/JetBrainsMono-Italic-Variable.ttf",
            weights(&[100, 700, 800]),
            37,
        ),
        ("fonts/FiraCode-VF.ttf", weights(&[300, 550, 700]), 37),
        ("var/gvar-cases.ttf", cases(), 1),
    ];
    let mut out = vec![
        format!(
            "# font  axis=value,... (or - for the default)  glyph  commands  (HarfBuzz {HARFBUZZ}, {BINDING})"
        ),
        crate::generated_by("gvar"),
    ];
    for (name, settings, step) in &fonts {
        let blob = hb::Blob::from_file(&root.join("baud-font/tests").join(name));
        let face = hb::Face::new(&blob);
        for setting in std::iter::once(None).chain(settings.iter().map(Some)) {
            let mut font = hb::Font::new(&face);
            font.set_scale(face.upem(), face.upem());
            let mut tag = "-".to_string();
            if let Some(s) = setting {
                font.set_variations(&crate::as_float(s));
                tag = label(s);
            }
            for gid in (0..face.glyph_count()).step_by(*step as usize) {
                let mut c = Commands(Vec::new());
                font.draw(gid, &mut c);
                out.push(format!("{name} {tag} {gid} {}", c.0.join(" ")));
            }
        }
    }
    out
}
