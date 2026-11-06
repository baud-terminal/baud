//! baud-font's partial bake against fontTools 4.63.0: Noto Sans
//! Devanagari's variable font pinned at wght 400 and 700, its width axis
//! kept, is the instance `scripts/build-fonts.sh` pins, and compares equal
//! to fontTools' `instantiateVariableFont(font, {"wght": w})`
//! (`[[source: fonttools-4-63-0-instancer]]`) of the same file, every table
//! expanded and none left as bytes. The variable font is
//! read from the cache the build fetches it into and fontTools' instances
//! from `BAUD_FONTTOOLS_NOTO` (`tests/common`); neither is in the
//! repository, and a test without one says so.

mod common;

use baud_fontbuild::instance::instance;
use baud_fontbuild::{compare, reach};
use common::{EPOCH, fonttools, noto_vf, sha256};

/// The instances `scripts/build-fonts.sh` pins.
const PINNED: [(i32, &str); 2] = [
    (
        400,
        "e2569116e656d46710fe1133548fcbcda9decdc94ec5d3c3219213a046d13da7",
    ),
    (
        700,
        "7eea4776889a270ad2cdf4afa89b169ab3b513990abb838fc702cb03aa6052bb",
    ),
];

#[test]
fn the_noto_instances_are_the_pinned_bytes() {
    let Some(vf) = noto_vf() else { return };
    for (w, want) in PINNED {
        let ours = instance(&vf, w, EPOCH).unwrap();
        assert_eq!(sha256(&ours), want, "wght {w}");
        assert!(
            ours == instance(&vf, w, EPOCH).unwrap(),
            "wght {w}: not deterministic"
        );
    }
}

#[test]
fn the_noto_instances_are_fonttools_in_every_table() {
    let Some(vf) = noto_vf() else { return };
    for (w, _) in PINNED {
        let Some(theirs) = fonttools(w) else { return };
        let ours = instance(&vf, w, EPOCH).unwrap();
        let report = compare(&theirs, &ours).unwrap();
        assert!(report.is_equal(), "wght {w}: {:?}", report.differences);
        for font in [&ours, &theirs] {
            let r = reach(font).unwrap();
            assert!(r.bytes_only.is_empty(), "wght {w}: {:?}", r.bytes_only);
            for t in [
                "glyf", "gvar", "HVAR", "GDEF", "GPOS", "fvar", "avar", "STAT", "name",
            ] {
                assert!(
                    r.tables.iter().any(|(n, lines)| n == t && *lines > 0),
                    "wght {w}: {t} not expanded"
                );
            }
        }
        // The control: the comparison sees the weight.
        let other = fonttools(1100 - w).unwrap();
        let r = compare(&other, &ours).unwrap();
        assert!(
            r.differences.iter().any(|d| d.starts_with("glyf:")),
            "{:?}",
            r.differences
        );
        assert!(
            r.differences.iter().any(|d| d.starts_with("gvar:")),
            "{:?}",
            r.differences
        );
    }
}

/// A face of one axis is the full bake's, and the partial bake refuses it.
#[test]
fn a_face_of_one_axis_is_refused() {
    let jbm = common::read("baud-font/tests/fonts/JetBrainsMono-Variable.ttf");
    let err = instance(&jbm, 400, EPOCH).unwrap_err();
    assert!(err.contains("Axes"), "{err}");
}
