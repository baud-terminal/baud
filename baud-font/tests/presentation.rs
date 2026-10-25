//! **The presentation forms a face without `GSUB` forms draws through**,
//! read back from the generated table: the characters UnicodeData.txt
//! decomposes to a letter in a form (`[[source: unicode-data]]`), and the
//! lam-alef ligatures U+FEF5..U+FEFC.

use baud_domain::{Codepoint, JoiningForm, LamAlef};
use baud_font::{lam_alef_form, presentation_form};

fn cp(v: u32) -> Codepoint {
    Codepoint::new(v).unwrap()
}

/// Beh's four forms are U+FE8F..U+FE92 in Table 9-5's order of the
/// decomposition tags; peh's are in Presentation Forms-A; alef has only
/// two, and a letter of Arabic Extended-A, which no presentation form
/// encodes, has none.
#[test]
fn each_form_is_the_character_that_decomposes_to_it() {
    use JoiningForm::*;
    let beh: Vec<u32> = [Isolated, Final, Initial, Medial]
        .iter()
        .map(|&f| presentation_form(cp(0x0628), f).unwrap().value())
        .collect();
    assert_eq!(beh, [0xFE8F, 0xFE90, 0xFE91, 0xFE92]);
    assert_eq!(presentation_form(cp(0x067E), Medial), Some(cp(0xFB59)));
    assert_eq!(presentation_form(cp(0x0627), Final), Some(cp(0xFE8E)));
    assert_eq!(presentation_form(cp(0x0627), Initial), None);
    assert_eq!(presentation_form(cp(0x0750), Isolated), None);
    assert_eq!(presentation_form(cp(0x0041), Isolated), None);
}

/// Lam with each of the four alefs Presentation Forms-B encodes it with,
/// nominal and joined; lam with an alef it does not is none.
#[test]
fn the_lam_alef_ligatures_are_fef5_to_fefc() {
    let got: Vec<(u32, u32)> = [0x0622, 0x0623, 0x0625, 0x0627]
        .iter()
        .map(|&a| {
            (
                lam_alef_form(cp(a), LamAlef::Nominal).unwrap().value(),
                lam_alef_form(cp(a), LamAlef::Joined).unwrap().value(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (0xFEF5, 0xFEF6),
            (0xFEF7, 0xFEF8),
            (0xFEF9, 0xFEFA),
            (0xFEFB, 0xFEFC)
        ]
    );
    assert_eq!(lam_alef_form(cp(0x0671), LamAlef::Nominal), None);
}
