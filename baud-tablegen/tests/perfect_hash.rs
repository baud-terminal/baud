//! **`perfect_hash` is the hash `scripts/gen-colour-names.py` finds**: on
//! X.Org's rgb.txt at 749182c (`[[source: xorg-rgb-749182c]]`) and
//! `debianred`, the 256 seeds are the ones committed in
//! `baud-domain/src/themefile/colour_names.rs`, and every key lands in a
//! slot of its own.

use baud_tablegen::{Kind, fnv1a, perfect_hash};

fn repo(path: &str) -> String {
    let p = format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// The names in the generator's order: each lower-cased name where it
/// first occurs, then `debianred`.
fn names() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in repo("docs/research/sources/xorg-rgb-749182c.txt").lines() {
        if line.is_empty() || line.starts_with('!') {
            continue;
        }
        let name = line
            .split_whitespace()
            .skip(3)
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out.push("debianred".to_string());
    out
}

fn committed_seeds() -> Vec<u16> {
    let text = repo("baud-domain/src/themefile/colour_names.rs");
    let body = text
        .split_once("SEEDS: [u16; 256] = [")
        .and_then(|(_, rest)| rest.split_once("];"))
        .map(|(body, _)| body)
        .expect("the SEEDS array");
    body.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().expect("a seed"))
        .collect()
}

#[test]
fn fnv1a_is_the_reference_function() {
    // FNV-1a 32 of "" and "a", from the algorithm's published vectors.
    assert_eq!(fnv1a(b"", 0), 0x811C_9DC5);
    assert_eq!(fnv1a(b"a", 0), 0xE40C_292C);
    assert_eq!(fnv1a(b"foobar", 0), 0xBF9C_F968);
}

#[test]
fn the_colour_names_perfect_hash_is_the_committed_one() {
    let names = names();
    assert_eq!(names.len(), 753);
    let keys: Vec<&[u8]> = names.iter().map(|n| n.as_bytes()).collect();
    let h = perfect_hash("colour names", &keys, 256, 1024).expect("a perfect hash");
    assert_eq!(h.seeds, committed_seeds());
    let mut placed = vec![0usize; keys.len()];
    for k in h.slots.iter().flatten() {
        placed[*k] += 1;
    }
    assert!(placed.iter().all(|&n| n == 1), "every key in one slot");
    let modulo = |v: u32, n: usize| usize::try_from(v).expect("a 32-bit value") % n;
    for (i, k) in keys.iter().enumerate() {
        let seed = h.seeds[modulo(fnv1a(k, 0), 256)];
        assert_eq!(h.slots[modulo(fnv1a(k, u32::from(seed)), 1024)], Some(i));
    }
}

#[test]
fn a_bucket_no_seed_places_is_refused() {
    let keys: Vec<&[u8]> = vec![b"a", b"b", b"c"];
    let e = perfect_hash("tight", &keys, 1, 2).expect_err("three keys, two slots");
    assert_eq!(e.kind, Kind::Invariant);
    assert_eq!(e.to_string(), "tight: no seed for bucket 0");
}
