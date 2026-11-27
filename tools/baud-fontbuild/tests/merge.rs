//! The Devanagari merge from its inputs: each style's Hebrew stage (built
//! from its committed source) with the Noto instance at its weight (baked
//! from the variable font in the cache, `tests/common`) merges into the
//! committed built face byte for byte, every table of which the
//! comparison expands.

mod common;

use baud_fontbuild::instance::instance;
use baud_fontbuild::merge::merge;
use baud_fontbuild::reach;
use baud_fontbuild::stage::build;
use common::{EPOCH, noto_vf, read};

fn stage(style: &str) -> Vec<u8> {
    let base = read(&format!(
        "crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-{style}.ttf"
    ));
    let src = String::from_utf8(read(&format!(
        "crates/baud-app/fonts/glyphs/languages/hebrew/stage/{style}.txt"
    )))
    .unwrap();
    build(&base, &src).unwrap()
}

#[test]
fn every_style_merges_into_the_committed_face() {
    let Some(vf) = noto_vf() else { return };
    let (n400, n700) = (
        instance(&vf, 400, EPOCH).unwrap(),
        instance(&vf, 700, EPOCH).unwrap(),
    );
    for (style, n) in [
        ("Regular", &n400),
        ("Bold", &n700),
        ("Italic", &n400),
        ("BoldItalic", &n700),
    ] {
        let m = merge(&stage(style), n, style, EPOCH).unwrap_or_else(|e| panic!("{style}: {e}"));
        assert_eq!((m.kept, m.code_points), (783, 225), "{style}");
        let committed = read(&format!("crates/baud-app/fonts/built/{style}.ttf"));
        assert!(m.font == committed, "{style}: not the committed file");
        let r = reach(&m.font).unwrap();
        assert!(r.bytes_only.is_empty(), "{style}: {:?}", r.bytes_only);
    }
}

/// The merge refuses a style it has no weight and slope for, and a Noto
/// face that is not the width-only instance (the variable font itself).
#[test]
fn the_merge_refuses_what_it_does_not_take() {
    let Some(vf) = noto_vf() else { return };
    let st = stage("Regular");
    let n = instance(&vf, 400, EPOCH).unwrap();
    assert!(
        merge(&st, &n, "Light", EPOCH)
            .unwrap_err()
            .contains("style")
    );
    let err = merge(&st, &vf, "Regular", EPOCH).unwrap_err();
    assert!(err.contains("wdth"), "{err}");
}
