//! HarfBuzz's and FreeType's drawings of the component cases, as
//! baud-font's point-matching and scaled-offset oracle: every glyph of
//! `tests/var/component-cases.ttf` at the default instance and at wght 100,
//! 650 and 900, drawn by HarfBuzz (its draw functions, font units) and by
//! FreeType (`FT_LOAD_NO_SCALE | FT_LOAD_NO_HINTING`, the outline's points
//! as loaded), one row per engine. Every contour of the font is a polygon
//! that starts on the curve, so a row is each contour's points in order,
//! and the two engines' rows and baud's compare point for point. A glyph
//! FreeType refuses to load is written `ERROR` with FreeType's message.
//! The two engines disagree on some glyphs on purpose; baud-font's
//! `tests/components.rs` says which the specification sides with.

use std::path::Path;

use crate::{BINDING, FREETYPE, HARFBUZZ, Oracle, contours_hb, fmt, ft, ft_row, hb, name};

/// `components.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "component",
    file: "baud-font/tests/var/components.txt",
    lines,
};

/// The default instance, then three weights.
pub const SETTINGS: [Option<i32>; 4] = [None, Some(100), Some(650), Some(900)];

fn lines(root: &Path) -> Vec<String> {
    let lib = ft::Library::new();
    crate::pinned(Some(&lib));
    let path = root.join("baud-font/tests/var/component-cases.ttf");
    let blob = hb::Blob::from_file(&path);
    let hface = hb::Face::new(&blob);
    let names: Vec<String> = {
        let font = hb::Font::new(&hface);
        (0..hface.glyph_count()).map(|g| name(&font, g)).collect()
    };
    let mut out = vec![
        format!(
            "# glyph  name  wght (or - for the default)  engine  contours  (HarfBuzz {HARFBUZZ}, {BINDING}, FreeType {FREETYPE})"
        ),
        crate::generated_by("component"),
    ];
    for w in SETTINGS {
        let mut font = hb::Font::new(&hface);
        font.set_scale(hface.upem(), hface.upem());
        let mut face = lib.face(&path);
        if let Some(w) = w {
            font.set_variations(&[("wght", f64::from(w))]);
            face.set_var_design(&[f64::from(w)])
                .unwrap_or_else(|e| panic!("{}", ft::exception(e)));
        }
        let l = w.map_or("-".to_string(), |w| w.to_string());
        for gid in 1..hface.glyph_count() {
            let n = &names[gid as usize];
            out.push(format!(
                "{gid} {n} {l} hb {}",
                fmt(&contours_hb(&font, gid))
            ));
            out.push(format!("{gid} {n} {l} ft {}", ft_row(&mut face, gid)));
        }
    }
    out
}
