//! The trie of named sequences, against the pinned files it is built from:
//! every prefix of every listed sequence is one step from the prefix before
//! it, and nothing else is a step at all (`sequence.rs`).
//!
//! The files are parsed here, independently of the build: UTS #51's
//! RGI lists (`[[source: emoji17-sequences]]`,
//! `[[source: emoji17-zwj-sequences]]`) and the standardized variation
//! sequences (`[[source: emoji-variation-sequences]]`).

use baud_domain::{ClusterSlot, ClusterStore, Codepoint, Content};
use std::collections::HashSet;

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

fn cp(v: u32) -> Codepoint {
    Codepoint::new(v).expect("a codepoint")
}

/// **Every prefix of every listed sequence, up to a base and a slot's
/// capacity, is one step from the one before**, reads back as itself
/// through the store's own reads, and takes no slot of the store; the codepoint after that capacity steps
/// nowhere, and the caller refuses it as it refuses any.
#[test]
fn every_prefix_of_a_named_sequence_is_one_step() {
    let mut slots = vec![ClusterSlot::EMPTY; ClusterStore::SLOTS_MAX];
    let store = ClusterStore::new(&mut slots);
    let keep = 1 + ClusterSlot::CAPACITY;
    let mut steps = 0;
    for s in named() {
        let mut at = Content::codepoint(cp(s[0]));
        for n in 2..=s.len() {
            let step = store.named(at, cp(s[n - 1]));
            if n > keep {
                assert!(step.is_none(), "{:x?} past capacity", &s[..n]);
                break;
            }
            let (id, seq) = step.unwrap_or_else(|| panic!("{:x?} is one step", &s[..n]));
            assert_eq!(seq.base.bits(), s[0]);
            assert_eq!(seq.cps(), &s[1..n]);
            assert_eq!(
                store.get(Content::cluster(id)).map(|g| g.cps()),
                Some(&s[1..n])
            );
            at = Content::cluster(id);
            assert_eq!(store.base_of(at).bits(), s[0], "{:x?}", &s[..n]);
            assert_eq!(store.marks_of(at).collect::<Vec<_>>(), &s[1..n]);
            steps += 1;
        }
    }
    assert!(steps > 8000, "{steps} steps");
    assert_eq!(store.filled(), 0);
}

/// **Nothing else is a step**: from every base that begins a listed
/// sequence, every listed prefix and a letter, each codepoint any sequence
/// continues with, plus a combining acute and a Hebrew point, steps exactly
/// where the files say a prefix continues, and nowhere otherwise.
#[test]
fn nothing_but_a_listed_prefix_is_a_step() {
    let mut slots = vec![ClusterSlot::EMPTY; ClusterStore::SLOTS_MAX];
    let store = ClusterStore::new(&mut slots);
    let keep = 1 + ClusterSlot::CAPACITY;
    let all = named();
    let listed: HashSet<Vec<u32>> = all
        .iter()
        .flat_map(|s| (2..=s.len().min(keep)).map(move |n| s[..n].to_vec()))
        .collect();
    let mut nexts: Vec<u32> = listed.iter().map(|p| p[p.len() - 1]).collect();
    nexts.extend([0x0301, 0x05B8, 0x3099]);
    nexts.sort();
    nexts.dedup();
    // Each parent as the cell word a terminal would hold for it.
    let mut parents: Vec<(Vec<u32>, Content)> = Vec::new();
    for b in all.iter().map(|s| s[0]).chain([0x61, 0x5D0]) {
        parents.push((vec![b], Content::codepoint(cp(b))));
    }
    for p in &listed {
        let mut at = Content::codepoint(cp(p[0]));
        for &c in &p[1..] {
            at = Content::cluster(store.named(at, cp(c)).expect("a listed step").0);
        }
        parents.push((p.clone(), at));
    }
    parents.sort_by(|a, b| a.0.cmp(&b.0));
    parents.dedup_by(|a, b| a.0 == b.0);
    let (mut yes, mut no) = (0, 0);
    for (p, at) in &parents {
        for &c in &nexts {
            let mut q = p.clone();
            q.push(c);
            let want = listed.contains(&q);
            let got = store.named(*at, cp(c));
            assert_eq!(got.is_some(), want, "{:x?}", q);
            if let Some((_, seq)) = got {
                assert_eq!((seq.base.bits(), seq.cps()), (q[0], &q[1..]));
                yes += 1;
            } else {
                no += 1;
            }
        }
    }
    assert_eq!(yes, listed.len(), "each listed prefix, once");
    assert!(no > 500_000, "{no} refusals");
}
