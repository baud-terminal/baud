//! The eight oracles, one module each, every one the port of the Python
//! script that wrote its file: the same fonts, settings, glyph steps and
//! row order, and the same format, float by float.

pub mod advance;
pub mod bearing;
pub mod cff2;
pub mod component;
pub mod gvar;
pub mod marks;
pub mod var;
pub mod vertical;

use std::path::Path;

use crate::{Oracle, hb, label};

/// An axis setting as the scripts' dicts held it: each tag and its value.
pub type Setting = Vec<(&'static str, i32)>;

/// One `wght` setting per weight.
pub fn weights(w: &[i32]) -> Vec<Setting> {
    w.iter().map(|&w| vec![("wght", w)]).collect()
}

/// One font the `gvar` and `cff2` oracles sample: its path under
/// `crates/baud-font/tests`, the settings it is drawn at after the default, and
/// the glyph step, every glyph whose id is a multiple of it.
pub struct Sample {
    /// The font, relative to `crates/baud-font/tests`.
    pub font: &'static str,
    /// The settings after the default instance, in row order.
    pub settings: Vec<Setting>,
    /// The glyph ids drawn are `0, step, 2 * step, ...`.
    pub step: u32,
}

/// Each sample's rows, as both outline scripts wrote them: per setting,
/// the default first, a row `font setting glyph commands` per sampled
/// glyph, where `draw` gives the glyph's commands at the font's units per
/// em.
pub fn outline_rows(
    root: &Path,
    samples: &[Sample],
    draw: fn(&hb::Font, u32) -> Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for s in samples {
        let blob = hb::Blob::from_file(&root.join("crates/baud-font/tests").join(s.font));
        let face = hb::Face::new(&blob);
        for setting in std::iter::once(None).chain(s.settings.iter().map(Some)) {
            let mut font = hb::Font::new(&face);
            font.set_scale(face.upem(), face.upem());
            let mut tag = "-".to_string();
            if let Some(v) = setting {
                font.set_variations(&crate::as_float(v));
                tag = label(v);
            }
            for gid in (0..face.glyph_count()).step_by(s.step as usize) {
                out.push(format!(
                    "{} {tag} {gid} {}",
                    s.font,
                    draw(&font, gid).join(" ")
                ));
            }
        }
    }
    out
}

/// Every oracle, in the order the table in the crate's docs gives them.
pub const ALL: [Oracle; 8] = [
    advance::ORACLE,
    bearing::ORACLE,
    cff2::ORACLE,
    component::ORACLE,
    gvar::ORACLE,
    marks::ORACLE,
    var::ORACLE,
    vertical::ORACLE,
];
