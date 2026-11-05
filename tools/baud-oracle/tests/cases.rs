//! **Every cases font is its recipe's.** `baud-oracle cases` rebuilds each
//! committed cases font byte for byte, which pins the font to the recipe
//! it was made from; a changed font fails the check; and what a font is
//! there to test survives the places a writer could have chosen
//! otherwise: the points a sparse gvar tuple leaves to inference and the
//! point numbers a glyph shares, joining's subtable formats, phantom's
//! truncated glyph, and the OS/2 version the fonts carry.

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

/// The font `path` of case `name`.
fn font_of<'a>(fonts: &'a [(&'static str, Vec<u8>)], path: &str) -> FontRef<'a> {
    let (_, bytes) = fonts
        .iter()
        .find(|(p, _)| p.ends_with(path))
        .unwrap_or_else(|| panic!("no {path}"));
    FontRef::new(bytes).expect("a font")
}

/// **The gvar cases leave their points to inference.** Each sparse tuple
/// names the points its recipe gives, where write-fonts would have
/// written a tuple no smaller dense: sq's one-point tuple, two's pair
/// whose x is shared, and hvar's phantoms alone. A dense tuple carries
/// the deltas inference would give, so no readout moves, and the
/// inference these fonts are there to reach goes unread.
#[test]
fn the_gvar_cases_leave_their_points_to_inference() {
    let gvar = built("gvar");
    let font = font_of(&gvar, "gvar-cases.ttf");
    let s = |v: &[u16]| Some(v.to_vec());
    assert_eq!(
        referenced(&font, 1),
        vec![None, s(&[0, 3, 5]), s(&[2]), None, s(&[1])],
        "sq"
    );
    assert_eq!(
        referenced(&font, 2),
        vec![s(&[0, 2, 5]), s(&[2, 4]), s(&[5, 6]), s(&[1, 3, 6]), None],
        "two"
    );
    assert_eq!(referenced(&font, 5), vec![s(&[1])], "nest");
    let hvar = built("hvar");
    for (path, _) in &hvar {
        let font = font_of(&hvar, path);
        assert_eq!(referenced(&font, 1)[0], s(&[4, 5]), "{path}");
    }
}

/// **Every glyph of the gvar cases shares its point numbers**, a glyph of
/// one tuple included, as the fonts were packed: the readers of shared
/// and private point numbers are both reached.
#[test]
fn every_gvar_case_glyph_shares_its_point_numbers() {
    for name in ["gvar", "hvar", "ink", "vertical"] {
        let fonts = built(name);
        for (path, bytes) in &fonts {
            let font = FontRef::new(bytes).expect("a font");
            let gvar = font.gvar().expect("gvar");
            let mut glyphs = 0;
            for g in 0..font.maxp().expect("maxp").num_glyphs() {
                let Some(data) = gvar.data_for_gid(GlyphId::new(g.into())).expect("data") else {
                    continue;
                };
                let count = u16::from_be_bytes([data.as_bytes()[0], data.as_bytes()[1]]);
                assert_ne!(count & 0x8000, 0, "{path} gid {g}");
                glyphs += 1;
            }
            assert!(glyphs > 0, "{path}: no glyph varies");
        }
    }
}

/// **joining keeps the subtable formats its feature text compiled to**:
/// init and medi a single substitution by delta (format 1), fina by list
/// (format 2), calt a chained context by coverage (format 3) calling a
/// single substitution, mark one mark-to-base subtable, and GDEF's glyph
/// classes as ranges (format 2).
#[test]
fn joining_keeps_its_subtable_formats() {
    let fonts = built("joining");
    let font = font_of(&fonts, "joining-cases.ttf");
    let at = |d: &[u8], o: usize| u16::from_be_bytes([d[o], d[o + 1]]) as usize;
    let formats = |d: &[u8]| -> Vec<(usize, usize)> {
        let list = at(d, 8);
        (0..at(d, list))
            .map(|i| {
                let lookup = list + at(d, list + 2 + 2 * i);
                assert_eq!(at(d, lookup + 4), 1, "one subtable");
                (at(d, lookup), at(d, lookup + at(d, lookup + 6)))
            })
            .collect()
    };
    let table = |t: &[u8; 4]| {
        font.table_data(read_fonts::types::Tag::new(t))
            .expect("the table")
            .as_bytes()
            .to_vec()
    };
    assert_eq!(
        formats(&table(b"GSUB")),
        [(1, 1), (1, 1), (1, 2), (6, 3), (1, 1)]
    );
    assert_eq!(formats(&table(b"GPOS")), [(4, 1)]);
    let gdef = table(b"GDEF");
    assert_eq!(at(&gdef, at(&gdef, 4)), 2, "GDEF's glyph classes");
}

/// **phantom's `broken` is twelve bytes**: a header claiming three
/// contours and two bytes of the first end point, its truncation the
/// length loca gives it.
#[test]
fn phantom_broken_is_truncated_at_twelve_bytes() {
    let fonts = built("hvar");
    let font = font_of(&fonts, "phantom.ttf");
    let loca = font.loca(None).expect("loca");
    let (a, b) = (
        loca.get_raw(7).expect("start"),
        loca.get_raw(8).expect("end"),
    );
    assert_eq!(b - a, 12);
}
