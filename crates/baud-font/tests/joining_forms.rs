//! **The joining test face's substitutions, glyph by glyph**:
//! `joining-cases.ttf` (`tools/baud-oracle cases joining`) takes beh and
//! sad to their initial and medial forms by a single substitution by delta
//! (format 1), beh, reh and sad to their final forms by a list (format 2),
//! and has one `calt` rule, a chained context by coverage (format 3), that
//! begins on a sad before a reh. Each form shaped must be the glyph the
//! face names for it, and the pairs the grid features begin on must be
//! read from the chained rule.

use baud_domain::{Codepoint, GlyphId, JoiningForm};
use baud_font::{Cluster, Digest, Face, Features, Glyph, Ignorable, Script, Shaper, Walked};

const BEH: char = '\u{0628}';
const REH: char = '\u{0631}';
const SAD: char = '\u{0635}';

fn read() -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/var/joining-cases.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn cp(c: char) -> Codepoint {
    Codepoint::new(u32::from(c)).unwrap()
}

/// Glyph ids, in the face's glyph order.
fn ids(ns: &[u16]) -> Vec<GlyphId> {
    ns.iter().map(|&n| GlyphId::from_shaper(n)).collect()
}

/// The glyphs of `word`, each letter a cell in its form, shaped as one run.
fn shaped(face: &Face<'_>, word: &[(char, JoiningForm)]) -> Vec<GlyphId> {
    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 16];
    let mut shaper = Shaper::new(face, &mut storage);
    for (k, &(c, form)) in word.iter().enumerate() {
        let at = Cluster(u16::try_from(k).unwrap());
        shaper.push_joined(&[cp(c)], at, form).unwrap();
    }
    let (buf, done) = shaper.finish(Features::JOINING, Script::of(cp(word[0].0)));
    assert!(done.declined.is_zero(), "{word:?}: lookups declined");
    buf.as_slice().iter().map(|g| g.id).collect()
}

/// The face's glyph order: the letters `cmap` maps, each followed by its
/// forms.
#[test]
fn the_face_maps_its_letters_where_its_forms_follow() {
    let data = read();
    let face = Face::parse(&data).unwrap();
    assert_eq!(
        face.glyph(cp(BEH)),
        ids(&[2]).first().copied(),
        "beh, then .init .medi .fina"
    );
    assert_eq!(
        face.glyph(cp(REH)),
        ids(&[6]).first().copied(),
        "reh, then .fina"
    );
    assert_eq!(
        face.glyph(cp(SAD)),
        ids(&[8]).first().copied(),
        "sad, then .init .medi .fina"
    );
}

/// `init` and `medi` by delta, `fina` by list: three sads and three behs
/// joined are each letter's initial, medial and final glyphs.
#[test]
fn a_joined_word_takes_each_letters_initial_medial_and_final_glyph() {
    use JoiningForm::{Final, Initial, Medial};
    let data = read();
    let face = Face::parse(&data).unwrap();
    assert_eq!(
        shaped(&face, &[(SAD, Initial), (SAD, Medial), (SAD, Final)]),
        ids(&[9, 10, 11]),
        "sad.init sad.medi sad.fina"
    );
    assert_eq!(
        shaped(&face, &[(BEH, Initial), (BEH, Medial), (BEH, Final)]),
        ids(&[3, 4, 5]),
        "beh.init beh.medi beh.fina"
    );
}

/// `fina` by list reaches a right-joining letter too, and `calt`'s rule on
/// a sad before a reh, which begins only on the isolated sad, leaves the
/// initial sad as `init` made it.
#[test]
fn a_sad_joined_to_a_reh_is_its_initial_and_the_reh_its_final() {
    use JoiningForm::{Final, Initial};
    let data = read();
    let face = Face::parse(&data).unwrap();
    assert_eq!(
        shaped(&face, &[(SAD, Initial), (REH, Final)]),
        ids(&[9, 7]),
        "sad.init reh.fina"
    );
}

/// The one pair the grid features' rules begin on is `calt`'s, read from
/// its chained context by coverage: a sad, then a reh.
#[test]
fn the_chained_rule_by_coverage_gives_its_pair() {
    let data = read();
    let face = Face::parse(&data).unwrap();
    let gsub = face.gsub().expect("the face's GSUB");
    let mut found: Vec<(GlyphId, Digest)> = Vec::new();
    let walked = gsub.pairs(Features::GRID, &mut |first, d| found.push((first, d)));
    assert_eq!(walked, Walked::Derived, "every subtable walked");
    let g = ids(&[8, 6]);
    assert_eq!(found, [(g[0], Digest::of(g[1]))], "sad, then reh");
}
