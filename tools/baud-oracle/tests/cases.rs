//! **Every cases font is its recipe's.** `baud-oracle cases` rebuilds each
//! committed cases font byte for byte, which pins the font to the recipe
//! it was made from; a changed font fails the check; and what a font is
//! there to test survives the places a writer could have chosen
//! otherwise: the points a sparse gvar tuple leaves to inference, and the
//! OS/2 version the fonts carry.

use read_fonts::types::GlyphId;
use read_fonts::{FontRef, TableProvider};

use baud_oracle::cases::{ALL, check};
use baud_oracle::repo;

#[test]
fn every_case_rebuilds_its_committed_fonts() {
    for case in &ALL {
        check(case, &repo()).unwrap_or_else(|e| panic!("cases {}: {e}", case.name));
    }
}

/// **The positive control of `--check`**: each case's fonts written under
/// a fresh root pass, and with one byte of the first font changed, fail.
#[test]
fn a_changed_font_fails_the_check() {
    for case in &ALL {
        let root = std::env::temp_dir().join(format!(
            "baud-oracle-cases-{}-{}",
            std::process::id(),
            case.name
        ));
        let _ = std::fs::remove_dir_all(&root);
        let fonts = (case.fonts)();
        for (path, bytes) in &fonts {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().expect("a directory")).expect("a directory");
            std::fs::write(file, bytes).expect("write the font");
        }
        assert!(check(case, &root).is_ok(), "{}: as built", case.name);
        let (path, bytes) = &fonts[0];
        let mut changed = bytes.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        std::fs::write(root.join(path), changed).expect("write the font");
        let result = check(case, &root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(result.is_err(), "{}: a changed byte passed", case.name);
    }
}

fn built(name: &str) -> Vec<(&'static str, Vec<u8>)> {
    let case = ALL.iter().find(|c| c.name == name).expect("a case");
    (case.fonts)()
}

/// Each tuple of glyph `g`: the points it references, or `None` for all.
fn referenced(font: &FontRef, g: u32) -> Vec<Option<Vec<u16>>> {
    let data = font
        .gvar()
        .expect("gvar")
        .glyph_variation_data(GlyphId::new(g))
        .expect("readable")
        .expect("tuples");
    data.tuples()
        .map(|t| (!t.has_deltas_for_all_points()).then(|| t.deltas().map(|d| d.position).collect()))
        .collect()
}

/// **bake's sq keeps its sparse tuples.** write-fonts packs a tuple with
/// every point when that is smaller; the bake's inference rule is reached
/// only through points a tuple leaves out, so sq's second tuple names
/// points 1, 4 and 5 and its third point 2, as the recipe gives them.
#[test]
fn bake_sq_leaves_its_points_to_inference() {
    let fonts = built("bake");
    let font = FontRef::new(&fonts[0].1).expect("a font");
    assert_eq!(
        referenced(&font, 1),
        vec![None, Some(vec![1, 4, 5]), Some(vec![2])]
    );
}

/// **Every cases font is OS/2 version 3**, the version patched over the
/// version 4 write-fonts names.
#[test]
fn every_case_is_os2_version_3() {
    for case in &ALL {
        for (path, bytes) in (case.fonts)() {
            let font = FontRef::new(&bytes).expect("a font");
            assert_eq!(font.os2().expect("OS/2").version(), 3, "{path}");
        }
    }
}
