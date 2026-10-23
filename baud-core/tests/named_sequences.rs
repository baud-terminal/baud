//! **Every emoji sequence the Unicode Standard names is kept whole, however
//! many are on the screen at once**: a family, a flag, a keycap, a skin
//! tone, a tag flag and a variation sequence, each read back from its cell
//! as the base and continuations that were sent.
//!
//! The sequences are read here from the pinned files themselves, UTS #51's
//! RGI lists (`[[source: emoji17-sequences]]`,
//! `[[source: emoji17-zwj-sequences]]`) and the standardized variation
//! sequences (`[[source: emoji-variation-sequences]]`), not from the table
//! the terminal steps through, so a sequence the generator lost is a
//! failure here. A cell names a listed sequence's node and no slot of the
//! cluster store; a cluster that goes on past a listed one is interned
//! whole, as every cluster was (docs/record.md, "A named emoji sequence is
//! a trie step, not an intern").

#[path = "support/mod.rs"]
mod support;

use baud_core::{ClusterSlot, Col, Content, Owned, PackedCell, Row, Term, WinSize};

/// Every sequence of two codepoints or more the three files list.
fn named() -> Vec<Vec<u32>> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources/");
    let mut out = Vec::new();
    for f in [
        "emoji17-sequences.txt",
        "emoji17-zwj-sequences.txt",
        "unicode-emoji-variation-sequences.txt",
    ] {
        let text = std::fs::read_to_string(format!("{dir}{f}")).expect("a pinned source");
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let field = line.split(';').next().unwrap_or("").trim();
            if field.is_empty() || field.contains("..") {
                continue;
            }
            let cps: Vec<u32> = field
                .split_whitespace()
                .map(|h| u32::from_str_radix(h, 16).expect("a hex codepoint"))
                .collect();
            if cps.len() > 1 {
                out.push(cps);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn utf8(cps: &[u32]) -> String {
    cps.iter()
        .map(|&c| char::from_u32(c).expect("a scalar value"))
        .collect()
}

/// The cluster the cell at `(r, c)` holds, base first.
fn held(t: &Term<'_>, r: usize, c: usize) -> Vec<u32> {
    let cell: PackedCell = t.viewport().row(Row::from_index(r))[c];
    let base: Content = t.base_of(cell);
    let mut out = vec![base.bits()];
    out.extend(t.marks(Row::from_index(r), Col::at_index(c)));
    out
}

/// Fifty clusters a row, four columns apart, each placed by `CUP` so a
/// control ends the one before it.
const PER_ROW: usize = 50;

/// **2,500 distinct named sequences on one screen, and every one reads back
/// whole.** The store holds 1024 clusters, and before the trie every
/// growing prefix of each took a slot, so a screen of them refused clusters
/// and left their cells short. A sequence longer than a base and
/// `ClusterSlot::CAPACITY` continuations -- a kiss with two skin tones is
/// ten codepoints -- keeps its first nine and counts the tenth dropped, as
/// it did.
#[test]
fn every_named_sequence_is_kept_however_many_are_on_the_screen() {
    let ws = support::CORPUS;
    let all = named();
    assert!(all.len() > 3000, "{} sequences", all.len());
    let per_screen = PER_ROW * usize::from(ws.ws_row);
    let keep = 1 + ClusterSlot::CAPACITY;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    let mut long = 0;
    for page in all.chunks(per_screen) {
        let mut bytes = b"\x1b[H\x1b[2J".to_vec();
        for (i, s) in page.iter().enumerate() {
            let (r, c) = (i / PER_ROW, (i % PER_ROW) * 4);
            bytes.extend(format!("\x1b[{};{}H{}", r + 1, c + 1, utf8(s)).bytes());
            long += usize::from(s.len() > keep);
        }
        support::feed(&mut t, &bytes);
        for (i, s) in page.iter().enumerate() {
            let (r, c) = (i / PER_ROW, (i % PER_ROW) * 4);
            assert_eq!(
                held(&t, r, c),
                s[..s.len().min(keep)],
                "{} at row {r}, column {c}",
                utf8(s)
            );
        }
    }
    assert!(long > 0, "some listed sequence is longer than a slot holds");
    assert_eq!(
        t.marks_dropped, long,
        "only the codepoints past a slot's capacity"
    );
}

/// **A cluster that goes on past a listed sequence keeps every codepoint**:
/// every prefix of every listed sequence, followed by a combining acute, a
/// text presentation selector and a skin tone wherever the files do not
/// list the result, read back base and all. These leave the trie and are
/// interned whole, so a fresh terminal takes each fifty, well under the
/// store's 1024 slots, and a prefix already at a slot's capacity keeps
/// itself and counts the codepoint dropped.
#[test]
fn a_cluster_that_goes_on_past_a_named_sequence_keeps_every_codepoint() {
    let ws = WinSize {
        ws_row: 2,
        ws_col: 200,
    };
    let all = named();
    let listed: std::collections::HashSet<Vec<u32>> = all
        .iter()
        .flat_map(|s| (2..=s.len()).map(move |n| s[..n].to_vec()))
        .collect();
    let mut prefixes: Vec<Vec<u32>> = listed.iter().cloned().collect();
    prefixes.sort();
    let keep = 1 + ClusterSlot::CAPACITY;
    let mut cases = Vec::new();
    for p in prefixes.iter().filter(|p| p.len() <= keep) {
        for extra in [0x0301, 0xFE0E, 0x1F3FB] {
            let mut s = p.clone();
            s.push(extra);
            if !listed.contains(&s) {
                cases.push(s);
            }
        }
    }
    assert!(cases.len() > 10_000, "{} cases", cases.len());
    for batch in cases.chunks(PER_ROW) {
        let mut st = Owned::new(&ws);
        let mut t = Term::new(st.storage(), &ws);
        let mut bytes = Vec::new();
        for (i, s) in batch.iter().enumerate() {
            bytes.extend(format!("\x1b[1;{}H{}", i * 4 + 1, utf8(s)).bytes());
        }
        support::feed(&mut t, &bytes);
        let mut long = 0;
        for (i, s) in batch.iter().enumerate() {
            long += usize::from(s.len() > keep);
            assert_eq!(held(&t, 0, i * 4), s[..s.len().min(keep)], "{:x?}", s);
        }
        assert_eq!(t.marks_dropped, long, "{:x?}", batch);
    }
}
