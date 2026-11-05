//! A two-level perfect hash over a fixed set of keys.

use crate::{Error, Kind};
use std::cmp::Reverse;
use std::ops::RangeInclusive;

/// 32-bit FNV-1a of `key`, its offset basis xored with `seed`.
pub fn fnv1a(key: &[u8], seed: u32) -> u32 {
    key.iter().fold(0x811C_9DC5 ^ seed, |h, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// How many buckets a perfect hash has, each with a seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Buckets(pub usize);

/// How many slots a perfect hash puts its keys in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slots(pub usize);

/// One slot, by its place among the [`Slots`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot(pub usize);

/// A perfect hash: a key's bucket has a seed, and the seed puts the key in
/// a slot of its own.
#[derive(Debug, PartialEq, Eq)]
pub struct PerfectHash {
    /// Each bucket's seed; 0 for a bucket no key falls in.
    pub seeds: Vec<u16>,
    /// The key at each slot, by its index in the keys given.
    pub slots: Vec<Option<usize>>,
}

/// The perfect hash of `keys` under FNV-1a: `fnv1a(key, 0) % buckets`
/// picks a bucket, and its seed `s` puts the key at `fnv1a(key, s) %
/// slots`, the seeds tried from 1 up ([`place`]).
pub fn perfect_hash(
    name: &str,
    keys: &[&[u8]],
    buckets: Buckets,
    slots: Slots,
) -> Result<PerfectHash, Error> {
    let modulo = |h: u32, n: usize| usize::try_from(h).map_or(0, |h| h % n);
    let bucket: Vec<usize> = keys
        .iter()
        .map(|k| modulo(fnv1a(k, 0), buckets.0))
        .collect();
    place(name, &bucket, buckets, slots, 1..=u16::MAX, |k, seed| {
        Slot(modulo(fnv1a(keys[k], u32::from(seed)), slots.0))
    })
}

/// The perfect hash of the keys `0..bucket.len()`, key `k` in bucket
/// `bucket[k]`, found deterministically: buckets are placed from the
/// fullest down, ties in bucket order (a stable sort), a bucket's keys in
/// their order, and each bucket takes the first of `seeds` at which
/// `slot(k, seed)` is distinct for its keys and free. A bucket past
/// `buckets` and a slot past `slots` are refused; `name` names the table
/// in a failure.
pub fn place(
    name: &str,
    bucket: &[usize],
    buckets: Buckets,
    slots: Slots,
    seeds: RangeInclusive<u16>,
    slot: impl Fn(usize, u16) -> Slot,
) -> Result<PerfectHash, Error> {
    let (Buckets(buckets), Slots(slots)) = (buckets, slots);
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); buckets];
    for (k, &b) in bucket.iter().enumerate() {
        let Some(m) = members.get_mut(b) else {
            return Err(Error::new(
                Kind::Invariant,
                name,
                format!("key {k} is in bucket {b}, past the last of {buckets}"),
            ));
        };
        m.push(k);
    }
    let mut order: Vec<usize> = (0..buckets).collect();
    order.sort_by_key(|&b| Reverse(members[b].len()));
    let mut out = PerfectHash {
        seeds: vec![0; buckets],
        slots: vec![None; slots],
    };
    for b in order {
        if members[b].is_empty() {
            continue;
        }
        let mut past = None;
        let found = seeds.clone().find_map(|seed| {
            let at: Vec<usize> = members[b].iter().map(|&k| slot(k, seed).0).collect();
            if let Some(&a) = at.iter().find(|&&a| a >= slots) {
                past = Some(a);
                return Some(None);
            }
            let distinct = at.iter().enumerate().all(|(i, a)| !at[..i].contains(a));
            (distinct && at.iter().all(|&a| out.slots[a].is_none())).then_some(Some((seed, at)))
        });
        if let Some(a) = past {
            return Err(Error::new(
                Kind::Invariant,
                name,
                format!("slot {a} is past the last of {slots}"),
            ));
        }
        let Some(Some((seed, at))) = found else {
            return Err(Error::new(
                Kind::Invariant,
                name,
                format!("no seed for bucket {b}"),
            ));
        };
        for (&a, &k) in at.iter().zip(&members[b]) {
            out.slots[a] = Some(k);
        }
        out.seeds[b] = seed;
    }
    Ok(out)
}
