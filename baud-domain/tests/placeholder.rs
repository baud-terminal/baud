//! The Unicode placeholder diacritic table and the placeholder word
//! (`[[source: kitty-graphics]]`, "Unicode placeholders";
//! `[[source: kitty-rowcolumn-diacritics]]`): the compiled table is the
//! pinned list (`[[source: kitty-placeholder-diacritics]]`), every
//! diacritic maps to its line and back and nothing else maps at all, and
//! a cell's placeholder word holds exactly the diacritics written into it.

use baud_domain::{
    ClusterId, ClusterSlot, ClusterStore, Codepoint, Content, Diacritic, PlaceholderMarks,
};

/// The pinned list's codepoints, in order.
fn pinned() -> Vec<u32> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/kitty-placeholder-diacritics.txt"
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| u32::from_str_radix(l.split(';').next().unwrap(), 16).unwrap())
        .collect()
}

/// **Every listed diacritic is its line's number, and back; no other
/// codepoint is a diacritic.** The text's own two: "U+305 is the diacritic
/// corresponding to the number 0 and U+30D corresponds to 1", and U+30E
/// "corresponding to the number 2".
#[test]
fn every_diacritic_is_its_lines_number_and_nothing_else_is_one() {
    let list = pinned();
    assert_eq!(list.len(), Diacritic::COUNT);
    for (n, &cp) in list.iter().enumerate() {
        let c = Codepoint::new(cp).unwrap();
        assert_eq!(
            Diacritic::of(c).map(Diacritic::index),
            Some(n),
            "U+{cp:04X}"
        );
        assert_eq!(Diacritic::at(n).unwrap().codepoint().value(), cp, "{n}");
    }
    let num = |cp: u32| Diacritic::of(Codepoint::new(cp).unwrap()).map(Diacritic::index);
    assert_eq!(
        (num(0x305), num(0x30D), num(0x30E)),
        (Some(0), Some(1), Some(2))
    );
    let found = (0..=0x10FFFF)
        .filter_map(Codepoint::new)
        .filter(|c| Diacritic::of(*c).is_some())
        .count();
    assert_eq!(found, list.len(), "a codepoint off the list maps");
    assert!(Diacritic::at(Diacritic::COUNT).is_none());
}

/// **A placeholder word holds what was written, and nothing else reads as
/// one.** Every length, the largest numbers, and a codepoint, a cluster
/// word and the reserved states, which are not placeholder words.
#[test]
fn a_placeholder_word_holds_exactly_its_diacritics() {
    let d = |n| Diacritic::at(n).unwrap();
    let last = Diacritic::COUNT - 1;
    for (row, col, msb) in [
        (None, None, None),
        (Some(0), None, None),
        (Some(3), Some(last), None),
        (Some(last), Some(last), Some(last)),
        (Some(1), Some(0), Some(255)),
    ] {
        let marks = PlaceholderMarks {
            row: row.map(d),
            col: col.map(d),
            msb: msb.map(d),
        };
        let w = Content::placeholder(marks);
        assert_eq!(w.placeholder_marks(), Some(marks), "{marks:?}");
        assert!(w.cluster_id().is_none() && w.as_codepoint().is_none());
    }
    for c in [
        Content::EMPTY,
        Content::WIDE_TAIL,
        Content::cluster(ClusterId::from_index(1023)),
        Content::codepoint(Codepoint::new(0x10EEEE).unwrap()),
    ] {
        assert_eq!(c.placeholder_marks(), None, "{c:?}");
    }
}

/// The diacritic whose field holds `n`: absent for 0, else the one
/// numbered `n - 1`.
fn field(n: usize) -> Option<Diacritic> {
    n.checked_sub(1).map(|i| Diacritic::at(i).unwrap())
}

/// The marks whose row, column and top-byte fields hold `row`, `col` and
/// `msb`.
fn marks(row: usize, col: usize, msb: usize) -> PlaceholderMarks {
    PlaceholderMarks {
        row: field(row),
        col: field(col),
        msb: field(msb),
    }
}

/// **Row 63, the row field's 64, reads back as row 63.** The field's bit 6
/// is the base's own bit, and the word holds it.
#[test]
fn row_63_reads_back() {
    let m = marks(64, 0, 0);
    assert_eq!(Content::placeholder(m).placeholder_marks(), Some(m));
}

/// **Every bit of every field reads back.** Each field in turn takes
/// absent, each single-bit value from 1 to 256, and the largest, 297,
/// while the other two hold 65 (bits 6 and 0); then the full cross of
/// absent, 1, 64 and 297 over the three fields.
#[test]
fn every_bit_of_every_field_reads_back() {
    let mut values = vec![0];
    values.extend((0..9).map(|k| 1 << k));
    values.push(Diacritic::COUNT);
    assert_eq!(Diacritic::COUNT, 297);
    for &v in &values {
        for m in [marks(v, 65, 65), marks(65, v, 65), marks(65, 65, v)] {
            let w = Content::placeholder(m);
            assert_eq!(w.placeholder_marks(), Some(m), "{m:?}");
        }
    }
    let cross = [0, 1, 64, Diacritic::COUNT];
    for row in cross {
        for col in cross {
            for msb in cross {
                let m = marks(row, col, msb);
                let w = Content::placeholder(m);
                assert_eq!(w.placeholder_marks(), Some(m), "{m:?}");
                assert!(w.is_placeholder_base(), "{m:?}");
            }
        }
    }
}

/// **The placeholder range touches no codepoint and no reserved word, and
/// leaves a word's top four bits zero.** Its low end is the word with no
/// marks, `0x0100_0000`; its high end the word with 297 in every field,
/// the base plus 297 shifted to each field's place.
#[test]
fn the_placeholder_range_is_disjoint_and_below_the_top_four_bits() {
    let n = u32::try_from(Diacritic::COUNT).unwrap();
    let low = Content::placeholder(PlaceholderMarks::NONE).bits();
    let high = Content::placeholder(marks(297, 297, 297)).bits();
    assert_eq!(low, 0x0100_0000);
    assert_eq!(high, low + (n << 18) + (n << 9) + n);
    let top_cluster = Content::cluster_word(ClusterId::from_index(usize::from(u16::MAX)));
    assert_eq!(top_cluster, 0x0012_FFFF);
    for reserved in [
        0x0010_FFFF,
        Content::EMPTY.bits(),
        Content::WIDE_TAIL.bits(),
        Content::DEFECTIVE.bits(),
        top_cluster,
    ] {
        assert!(reserved < low, "{reserved:#x} reaches {low:#x}");
    }
    assert!(high < 1 << 28, "{high:#x} sets a top bit");
}

/// **A mark joins a placeholder's word while it is a diacritic and there
/// is room**, and the store reads the word back as U+10EEEE with its
/// marks, as a copy must.
#[test]
fn a_diacritic_joins_the_word_and_reads_back() {
    let cp = |v| Codepoint::new(v).unwrap();
    let base = Content::codepoint(cp(0x10EEEE));
    let one = base.with_diacritic(cp(0x30D)).unwrap();
    let two = one.with_diacritic(cp(0x305)).unwrap();
    let three = two.with_diacritic(cp(0x30E)).unwrap();
    assert!(three.with_diacritic(cp(0x305)).is_none(), "a fourth");
    assert!(
        one.with_diacritic(cp(0x301)).is_none(),
        "U+0301 is not listed"
    );
    assert!(
        Content::codepoint(cp(0x41))
            .with_diacritic(cp(0x305))
            .is_none(),
        "not a placeholder"
    );
    let mut slots = [ClusterSlot::EMPTY; 4];
    let store = ClusterStore::new(&mut slots);
    assert_eq!(store.base_of(three), base);
    assert_eq!(
        store.marks_of(three).collect::<Vec<_>>(),
        vec![0x30D, 0x305, 0x30E]
    );
    assert_eq!(store.marks_of(one).collect::<Vec<_>>(), vec![0x30D]);
    assert!(store.marks_of(base).next().is_none());
}
