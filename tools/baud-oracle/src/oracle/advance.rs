//! HarfBuzz's advances at a variation, as baud-font's HVAR oracle: for each
//! committed variable test font, the horizontal advance HarfBuzz gives each
//! glyph -- every glyph of the cases fonts, every 11th of the real ones --
//! at the default instance and at the settings below, and one glyph id past
//! the face at each setting whose coordinates are not all zero, in font
//! units (the font's scale is its units per em). The default rows are the
//! control: they must be the hmtx advance, which says the scale is units
//! per em before a varied row is compared.
//!
//! One glyph id past the face, where a coordinate is not the default:
//! HarfBuzz's hmtx answers 0 there, to which HVAR still adds its delta (the
//! map clamps). At the default instance baud answers ttf-parser's nothing,
//! which baud-font's `tests/oracle.rs` holds.

use std::path::Path;

use crate::{BINDING, HARFBUZZ, Oracle, hb, label};

/// `advances.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "advance",
    file: "baud-font/tests/var/advances.txt",
    lines,
};

type Setting = Vec<(&'static str, i32)>;

/// The cases fonts' grid; wght 650 is 0.5 exactly, which puts a unit delta
/// on a half.
fn cases() -> Vec<Setting> {
    let mut out = vec![Vec::new()];
    for w in [100, 150, 400, 525, 650, 775, 900] {
        for d in [50, 75, 100, 150, 200] {
            if (w, d) == (400, 100) {
                continue;
            }
            out.push(if d != 100 {
                vec![("wght", w), ("wdth", d)]
            } else {
                vec![("wght", w)]
            });
        }
    }
    out
}

fn weights(w: &[i32]) -> Vec<Setting> {
    std::iter::once(Vec::new())
        .chain(w.iter().map(|&w| vec![("wght", w)]))
        .collect()
}

fn lines(root: &Path) -> Vec<String> {
    crate::pinned(None);
    let tests = root.join("baud-font/tests");
    let real = weights(&[100, 350, 650, 700, 800]);
    let fira = weights(&[300, 450, 550, 700]);
    let mut fonts: Vec<(String, Vec<Setting>, u32)> = vec![
        ("fonts/JetBrainsMono-Variable.ttf".into(), real.clone(), 11),
        ("fonts/JetBrainsMono-Italic-Variable.ttf".into(), real, 11),
        ("fonts/FiraCode-VF.ttf".into(), fira, 11),
        ("var/gvar-cases.ttf".into(), cases(), 1),
    ];
    let mut hvar: Vec<String> = std::fs::read_dir(tests.join("var/hvar"))
        .expect("var/hvar")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|n| n.ends_with(".ttf"))
        .collect();
    // Python's `sorted` on str: by code point, which `Ord` on `String` is.
    hvar.sort();
    fonts.extend(
        hvar.into_iter()
            .map(|n| (format!("var/hvar/{n}"), cases(), 1)),
    );

    let mut out = vec![
        format!(
            "# font  axis=value,... (or - for the default)  glyph  advance  (HarfBuzz {HARFBUZZ}, {BINDING})"
        ),
        crate::generated_by("advance"),
    ];
    for (name, settings, step) in &fonts {
        let blob = hb::Blob::from_file(&tests.join(name));
        let face = hb::Face::new(&blob);
        for s in settings {
            let mut font = hb::Font::new(&face);
            if !s.is_empty() {
                font.set_variations(&crate::as_float(s));
            }
            let l = label(s);
            for gid in (0..face.glyph_count()).step_by(*step as usize) {
                out.push(format!("{name} {l} {gid} {}", font.h_advance(gid)));
            }
            if font.var_coords_normalized().iter().any(|&c| c != 0) {
                let gid = face.glyph_count();
                out.push(format!("{name} {l} {gid} {}", font.h_advance(gid)));
            }
        }
    }
    out
}
