//! **`perfect_hash` is the hash kitty's colour-name table was found with**:
//! on X.Org's rgb.txt at 749182c (`[[source: xorg-rgb-749182c]]`) and
//! `debianred`, the 256 seeds are [`SEEDS`], the ones the table was
//! committed with before `baud-domain`'s build compiled it, and every key
//! lands in a slot of its own. `place`, which it is built on, tries the
//! seeds it is given in order, so a hash whose seeds start at 0 is
//! another table, and refuses a bucket or a slot past the last.

use baud_tablegen::{Buckets, Kind, Slot, Slots, fnv1a, perfect_hash, place};

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

/// The 256 seeds of kitty's colour-name table as it was committed, and as
/// `baud-domain`'s build compiles it.
#[rustfmt::skip]
const SEEDS: [u16; 256] = [
    1, 5, 4, 1, 3, 4, 1, 4, 3, 11, 1, 5, 1, 2, 3, 1,
    7, 3, 5, 3, 5, 8, 0, 2, 2, 5, 2, 1, 4, 1, 8, 2,
    15, 1, 1, 4, 1, 16, 1, 0, 3, 4, 3, 2, 1, 7, 5, 15,
    1, 1, 3, 13, 9, 3, 4, 6, 2, 1, 4, 2, 1, 2, 6, 3,
    1, 2, 2, 3, 13, 1, 23, 14, 0, 13, 11, 4, 6, 4, 4, 7,
    3, 22, 3, 1, 4, 2, 0, 1, 2, 8, 4, 1, 3, 2, 11, 1,
    1, 14, 0, 10, 15, 0, 1, 5, 42, 2, 6, 1, 3, 1, 0, 1,
    5, 2, 9, 1, 4, 0, 5, 1, 17, 8, 4, 4, 0, 15, 4, 3,
    11, 38, 1, 1, 4, 1, 3, 1, 1, 1, 2, 4, 11, 1, 3, 7,
    6, 5, 1, 7, 2, 1, 1, 5, 1, 2, 8, 1, 11, 6, 22, 4,
    2, 1, 13, 0, 3, 3, 1, 14, 3, 0, 1, 3, 6, 10, 1, 15,
    4, 8, 1, 2, 1, 4, 1, 5, 16, 2, 3, 7, 2, 1, 2, 6,
    3, 1, 12, 6, 2, 1, 41, 1, 1, 4, 6, 3, 1, 11, 33, 1,
    1, 8, 1, 3, 22, 2, 7, 0, 20, 11, 1, 2, 18, 5, 28, 20,
    2, 4, 4, 11, 2, 9, 1, 1, 1, 0, 2, 5, 42, 13, 1, 2,
    5, 0, 2, 0, 7, 1, 6, 2, 0, 3, 10, 3, 2, 6, 3, 1,
];

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
    let h = perfect_hash("colour names", &keys, Buckets(256), Slots(1024)).expect("a perfect hash");
    assert_eq!(h.seeds, SEEDS);
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
    let e = perfect_hash("tight", &keys, Buckets(1), Slots(2)).expect_err("three keys, two slots");
    assert_eq!(e.kind, Kind::Invariant);
    assert_eq!(e.to_string(), "tight: no seed for bucket 0");
}

/// Two keys in one bucket, whose slot is the seed: seed 0 puts both in
/// slot 0, so from 0 the first seed that parts them is 1; from 1 it is 1
/// too, but a bucket of one key takes 0 where it may.
#[test]
fn place_tries_the_seeds_it_is_given_in_order() {
    let slot = |k: usize, seed: u16| Slot((k * usize::from(seed)) % 4);
    let two = place("t", &[0, 0], Buckets(1), Slots(4), 0..=u16::MAX, slot).expect("placed");
    assert_eq!(two.seeds, [1]);
    assert_eq!(two.slots, [Some(0), Some(1), None, None]);
    let one = |first| {
        place("t", &[0], Buckets(1), Slots(4), first..=u16::MAX, |_, s| {
            Slot(usize::from(s) % 4)
        })
        .expect("placed")
        .seeds
    };
    assert_eq!(one(0), [0]);
    assert_eq!(one(1), [1]);
    // The fullest bucket first: bucket 1, keys 1 and 2, takes slots 0
    // and 1 at seed 0, so bucket 0's key, whose slot is its seed, takes
    // seed 2; placed in bucket order, it would have taken seed 0.
    let both = place(
        "t",
        &[0, 1, 1],
        Buckets(2),
        Slots(4),
        0..=u16::MAX,
        |k, s| Slot((usize::from(s) + k.saturating_sub(1)) % 4),
    )
    .expect("placed");
    assert_eq!(both.seeds, [2, 0]);
    assert_eq!(both.slots, [Some(1), Some(2), Some(0), None]);
}

#[test]
fn place_refuses_a_bucket_or_a_slot_past_the_last() {
    let e = place("t", &[0, 2], Buckets(2), Slots(4), 0..=9, |_, _| Slot(0))
        .expect_err("bucket 2 of 2");
    assert_eq!(e.kind, Kind::Invariant);
    assert_eq!(e.to_string(), "t: key 1 is in bucket 2, past the last of 2");
    let e = place("t", &[0], Buckets(1), Slots(4), 0..=9, |_, _| Slot(4)).expect_err("slot 4 of 4");
    assert_eq!(e.to_string(), "t: slot 4 is past the last of 4");
    let e = place("t", &[0, 0], Buckets(1), Slots(4), 0..=9, |_, _| Slot(3))
        .expect_err("one slot for two");
    assert_eq!(e.to_string(), "t: no seed for bucket 0");
}
