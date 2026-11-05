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

/// Every presentation form UnicodeData.txt gives, read here with a reader
/// of its own rather than the build's, is the one drawn for its letter and
/// form, and every lam-alef ligature for its alef; and no other pair of a
/// letter and a form has one.
#[test]
fn every_presentation_form_is_the_pinned_files() {
    use JoiningForm::*;
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/unicode-unicodedata.txt"
    );
    let text = std::fs::read_to_string(path).expect("the pinned UnicodeData.txt");
    let forms = [
        ("<isolated> ", Isolated),
        ("<final> ", Final),
        ("<initial> ", Initial),
        ("<medial> ", Medial),
    ];
    let mut single = std::collections::HashMap::new();
    let mut ligatures = 0;
    for line in text.lines() {
        let f: Vec<&str> = line.split(';').collect();
        let Some((tag, form)) = forms.iter().find(|(t, _)| f[5].starts_with(t)) else {
            continue;
        };
        let at = u32::from_str_radix(f[0], 16).unwrap();
        let of: Vec<u32> = f[5][tag.len()..]
            .split(' ')
            .map(|s| u32::from_str_radix(s, 16).unwrap())
            .collect();
        match of[..] {
            [letter] => {
                assert_eq!(presentation_form(cp(letter), *form), Some(cp(at)), "{line}");
                single.insert((letter, tag.len()), at);
            }
            [0x0644, alef] if (0xFEF5..=0xFEFC).contains(&at) => {
                let kind = if *form == Isolated {
                    LamAlef::Nominal
                } else {
                    LamAlef::Joined
                };
                assert_eq!(lam_alef_form(cp(alef), kind), Some(cp(at)), "{line}");
                ligatures += 1;
            }
            _ => {}
        }
    }
    assert_eq!(single.len(), 242);
    assert_eq!(ligatures, 8);
    let mut drawn = 0;
    for letter in (0..=0x10FFFF).filter_map(Codepoint::new) {
        for (tag, form) in &forms {
            if let Some(p) = presentation_form(letter, *form) {
                assert_eq!(single.get(&(letter.value(), tag.len())), Some(&p.value()));
                drawn += 1;
            }
        }
    }
    assert_eq!(drawn, 242);
}
