//! A two-level perfect hash over a fixed set of keys.

use crate::{Error, Kind};
use std::cmp::Reverse;

/// 32-bit FNV-1a of `key`, its offset basis xored with `seed`.
pub fn fnv1a(key: &[u8], seed: u32) -> u32 {
    key.iter().fold(0x811C_9DC5 ^ seed, |h, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// A perfect hash: `fnv1a(key, 0) % buckets` picks a bucket, and that
/// bucket's seed `s` puts the key at `fnv1a(key, s) % slots`.
#[derive(Debug, PartialEq, Eq)]
pub struct PerfectHash {
    /// Each bucket's seed; 0 for a bucket no key falls in.
    pub seeds: Vec<u16>,
    /// The key at each slot, by its index in the keys given.
    pub slots: Vec<Option<usize>>,
}

/// The perfect hash of `keys`, found deterministically: buckets are placed
/// from the fullest down, ties in bucket order (a stable sort), and each
/// takes the first seed from 1 up whose slots are distinct and free.
/// `name` names the table in a failure.
pub fn perfect_hash(
    name: &str,
    keys: &[&[u8]],
    buckets: usize,
    slots: usize,
) -> Result<PerfectHash, Error> {
    let modulo = |h: u32, n: usize| usize::try_from(h).map_or(0, |h| h % n);
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); buckets];
    for (i, k) in keys.iter().enumerate() {
        members[modulo(fnv1a(k, 0), buckets)].push(i);
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
        let found = (1..=u16::MAX).find_map(|seed| {
            let at: Vec<usize> = members[b]
                .iter()
                .map(|&k| modulo(fnv1a(keys[k], u32::from(seed)), slots))
                .collect();
            let distinct = at.iter().enumerate().all(|(i, a)| !at[..i].contains(a));
            (distinct && at.iter().all(|&a| out.slots[a].is_none())).then_some((seed, at))
        });
        let Some((seed, at)) = found else {
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
