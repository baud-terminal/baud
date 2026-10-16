//! The Unicode placeholder diacritic table and the placeholder word
//! (`[[source: kitty-graphics]]`, "Unicode placeholders";
//! `[[source: kitty-rowcolumn-diacritics]]`): the generated table is the
//! pinned list, every diacritic maps to its line and back and nothing else
//! maps at all, and a cell's placeholder word holds exactly the diacritics
//! written into it.

use baud_domain::{
    ClusterId, ClusterSlot, ClusterStore, Codepoint, Content, Diacritic, PlaceholderMarks,
};

/// The pinned list's codepoints, in order.
fn pinned() -> Vec<u32> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/kitty-rowcolumn-diacritics.txt"
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| u32::from_str_radix(l.split(';').next().unwrap(), 16).unwrap())
        .collect()
}

/// The generated table matches the pinned list.
#[test]
fn placeholder_table_matches_the_pinned_list() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-placeholder-table.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "placeholder_data.rs is stale -- rerun scripts/gen-placeholder-table.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
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
