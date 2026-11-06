//! **The compiled tables are the committed ones**, element for element:
//! `build/dec_graphics.rs`, `build/nrcs.rs`, `build/dec_sets.rs` and
//! `build/iso96.rs`'s output in `OUT_DIR` against the files the Python
//! generators wrote, before those files are deleted.

#[path = "dec_graphics.rs"]
mod committed_graphics;
#[path = "iso96_data.rs"]
mod committed_iso96;
#[path = "nrcs_data.rs"]
mod committed_nrcs;
#[path = "dec_sets_data.rs"]
mod committed_sets;

use crate::Nrcs;

fn same<T: PartialEq + core::fmt::Debug>(
    name: &'static str,
    a: &[T],
    b: &[T],
) -> (&'static str, usize) {
    assert_eq!(a.len(), b.len(), "{name}: lengths differ");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert_eq!(x, y, "{name}[{i}] differs");
    }
    (name, a.len())
}

#[test]
fn the_compiled_tables_are_the_committed_ones() {
    use crate::{dec_sets_data as s1, iso96_data as i1};
    use committed_iso96 as i0;
    use committed_sets as s0;
    assert_eq!(
        crate::DEC_ERROR_CHARACTER,
        committed_nrcs::DEC_ERROR_CHARACTER
    );
    let mut n = alloc::vec![
        same(
            "DEC_GRAPHICS",
            &crate::DEC_GRAPHICS,
            &committed_graphics::DEC_GRAPHICS
        ),
        same(
            "DEC_SUPPLEMENTAL",
            &s1::DEC_SUPPLEMENTAL,
            &s0::DEC_SUPPLEMENTAL
        ),
        same("JIS_KATAKANA", &s1::JIS_KATAKANA, &s0::JIS_KATAKANA),
        same("JIS_ROMAN", &s1::JIS_ROMAN, &s0::JIS_ROMAN),
        same("LATIN_1", &i1::LATIN_1, &i0::LATIN_1),
        same("LATIN_2", &i1::LATIN_2, &i0::LATIN_2),
        same("GREEK", &i1::GREEK, &i0::GREEK),
        same("HEBREW", &i1::HEBREW, &i0::HEBREW),
        same("LATIN_CYRILLIC", &i1::LATIN_CYRILLIC, &i0::LATIN_CYRILLIC),
        same("LATIN_5", &i1::LATIN_5, &i0::LATIN_5),
        same("UK", crate::uk_table(), committed_nrcs::uk_table()),
    ];
    // Every variant: the match is exhaustive in both, so a set added to
    // `Nrcs` and not here fails to compile there first.
    let sets = [
        ("Dutch", Nrcs::Dutch),
        ("Finnish", Nrcs::Finnish),
        ("French", Nrcs::French),
        ("FrenchCanadian", Nrcs::FrenchCanadian),
        ("German", Nrcs::German),
        ("Italian", Nrcs::Italian),
        ("NorwegianDanish", Nrcs::NorwegianDanish),
        ("Portuguese", Nrcs::Portuguese),
        ("Spanish", Nrcs::Spanish),
        ("Swedish", Nrcs::Swedish),
        ("Swiss", Nrcs::Swiss),
        ("Greek", Nrcs::Greek),
        ("Hebrew", Nrcs::Hebrew),
        ("Turkish", Nrcs::Turkish),
    ];
    for (name, set) in sets {
        n.push(same(
            name,
            crate::nrcs_table(set),
            committed_nrcs::nrcs_table(set),
        ));
    }
    extern crate std;
    let line: std::vec::Vec<std::string::String> =
        n.iter().map(|(k, v)| std::format!("{k} {v}")).collect();
    std::println!("identical: {} elements", line.join(", "));
}
