//! HarfBuzz's normalised variation coordinates, as baud-font's oracle: for
//! each committed variable test font, every wght design value from 0 to
//! 1000 in quarter units (the axis ranges are inside that, so both clamps
//! are exercised), with the 2.14 coordinate HarfBuzz computes. The OpenType
//! specification fixes the arithmetic -- 16.16 in, clamp, avar, then add 2
//! and shift right by 2 -- except the rounding of the 16.16 division, which
//! HarfBuzz decides here.

use std::path::Path;

use crate::{BINDING, HARFBUZZ, Oracle, hb};

/// `normalize.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "var",
    file: "crates/baud-font/tests/var/normalize.txt",
    lines,
};

const FONTS: [&str; 3] = [
    "FiraCode-VF.ttf",
    "JetBrainsMono-Italic-Variable.ttf",
    "JetBrainsMono-Variable.ttf",
];

fn lines(root: &Path) -> Vec<String> {
    crate::pinned(None);
    let mut out = vec![
        format!(
            "# font  wght-design-in-quarters  normalised-2.14-as-integer  (HarfBuzz {HARFBUZZ}, {BINDING})"
        ),
        crate::generated_by("var"),
    ];
    for name in FONTS {
        let blob = hb::Blob::from_file(&root.join("crates/baud-font/tests/fonts").join(name));
        let mut font = hb::Font::new(&hb::Face::new(&blob));
        for q in 0..=4000 {
            // `q / 4` is exact in a C float, so the value HarfBuzz receives
            // is the one the script's division made.
            font.set_variations(&[("wght", f64::from(q) / 4.0)]);
            let coords = font.var_coords_normalized();
            assert_eq!(coords.len(), 1, "{name}: one axis");
            out.push(format!("{name} {q} {}", coords[0]));
        }
    }
    out
}
