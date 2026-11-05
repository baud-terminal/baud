//! HarfBuzz's and FreeType's drawings of the bearing cases, as baud-font's
//! left-side-bearing oracle: every glyph of `tests/var/bearing-cases.ttf`
//! and `bearing-cases-bit1.ttf` at the default instance and at wght 100,
//! 650 and 900, drawn by HarfBuzz and by FreeType as the component oracle
//! draws them, one row per engine, with the font's file name first. A
//! glyph FreeType refuses is written `ERROR` with its message.
//!
//! Each glyph's `start` row is where its left phantom starts at the
//! default instance, `xMin - lsb` -- a USE_MY_METRICS component's, the last
//! one's, where a composite has one -- read from `glyf` and `hmtx` by
//! read-fonts, so baud's test does not take it from the code it checks.

use std::path::Path;

use read_fonts::tables::glyf::{CompositeGlyphFlags, Glyph};
use read_fonts::types::GlyphId;
use read_fonts::{FontRef, TableProvider};

use crate::oracle::component::SETTINGS;
use crate::{BINDING, FREETYPE, HARFBUZZ, Oracle, contours_hb, fmt, ft, ft_row, hb, name};

/// `bearings.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "bearing",
    file: "baud-font/tests/var/bearings.txt",
    lines,
};

/// The reader the `start` rows are read with.
pub const READER: &str = "read-fonts 0.46.0";

const FONTS: [&str; 2] = ["bearing-cases.ttf", "bearing-cases-bit1.ttf"];

/// Where `glyph`'s left phantom starts at the default instance, as the
/// script's fontTools `start` computed it: `xMin` (0 for an empty glyph)
/// minus the hmtx left side bearing, replaced, for a composite, by each
/// USE_MY_METRICS component's own start in turn, so the last one's stands.
pub fn start(font: &FontRef, glyph: GlyphId) -> i32 {
    let (loca, glyf, hmtx) = (
        font.loca(None).expect("loca"),
        font.glyf().expect("glyf"),
        font.hmtx().expect("hmtx"),
    );
    let lsb = hmtx.side_bearing(glyph).expect("an hmtx entry");
    let g = loca.get_glyf(glyph, &glyf).expect("a glyf entry");
    let x_min = g.as_ref().map_or(0, Glyph::x_min);
    let mut at = i32::from(x_min) - i32::from(lsb);
    if let Some(Glyph::Composite(c)) = &g {
        for part in c.components() {
            if part.flags.contains(CompositeGlyphFlags::USE_MY_METRICS) {
                at = start(font, part.glyph);
            }
        }
    }
    at
}

fn lines(root: &Path) -> Vec<String> {
    let lib = ft::Library::new();
    crate::pinned(Some(&lib));
    let dir = root.join("baud-font/tests/var");
    let mut out = vec![
        format!(
            "# font  glyph  name  wght (or - for the default)  engine  contours  (HarfBuzz {HARFBUZZ}, {BINDING}, FreeType {FREETYPE})"
        ),
        format!("# font  glyph  name  start  pp1 at the default instance ({READER})"),
        crate::generated_by("bearing"),
    ];
    for file in FONTS {
        let path = dir.join(file);
        let blob = hb::Blob::from_file(&path);
        let hface = hb::Face::new(&blob);
        let names: Vec<String> = {
            let font = hb::Font::new(&hface);
            (0..hface.glyph_count()).map(|g| name(&font, g)).collect()
        };
        let data = std::fs::read(&path).expect("the cases font");
        let font = FontRef::new(&data).expect("a font");
        for gid in 1..hface.glyph_count() {
            let s = start(&font, GlyphId::new(gid));
            out.push(format!("{file} {gid} {} start {s}", names[gid as usize]));
        }
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
                    "{file} {gid} {n} {l} hb {}",
                    fmt(&contours_hb(&font, gid))
                ));
                out.push(format!(
                    "{file} {gid} {n} {l} ft {}",
                    ft_row(&mut face, gid)
                ));
            }
        }
    }
    out
}
