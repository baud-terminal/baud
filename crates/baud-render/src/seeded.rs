//! **The hasher every map in the renderer uses**: SipHash-1-3, the
//! algorithm the standard library's `RandomState` runs, keyed by one seed for
//! the whole process. The seed is random unless [`fix_hash_seed`] fixed it
//! first, so a map keyed by what a program prints -- a cluster, a codepoint,
//! a colour -- keeps the collision resistance a random key gives it, and a
//! benchmark that fixes the seed gets the same probes, and so the same
//! instructions, on every run.

use std::hash::{BuildHasher, DefaultHasher, Hasher};
use std::sync::OnceLock;

/// The key every renderer map hashes with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashSeed(pub u64);

static SEED: OnceLock<HashSeed> = OnceLock::new();

/// **Fix the seed every renderer map hashes with**, before the first map is
/// made: a measurement that must repeat exactly (the frame bench under
/// callgrind) or a test. Returns whether it took: the seed is set once, at
/// the first hash or the first call, whichever comes first.
pub fn fix_hash_seed(seed: HashSeed) -> bool {
    SEED.set(seed).is_ok()
}

fn seed() -> HashSeed {
    *SEED.get_or_init(|| HashSeed(std::collections::hash_map::RandomState::new().hash_one(0u8)))
}

/// The renderer's maps' hasher: [`DefaultHasher`] with the seed written
/// first, which keys it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Seeded;

impl BuildHasher for Seeded {
    type Hasher = DefaultHasher;
    fn build_hasher(&self) -> DefaultHasher {
        let mut h = DefaultHasher::new();
        h.write_u64(seed().0);
        h
    }
}

/// A map hashed by [`Seeded`].
pub type Map<K, V> = std::collections::HashMap<K, V, Seeded>;

#[cfg(test)]
mod tests {
    use super::*;

    /// **One key for every map, set once**: two hashers hash a value alike,
    /// so two maps built from the same writes probe alike; and once the
    /// seed is in use a later `fix_hash_seed` doesn't change it under a map.
    #[test]
    fn every_map_hashes_with_one_seed_set_once() {
        let a = Seeded.hash_one(("cluster", 0x1F600u32));
        let b = Seeded.hash_one(("cluster", 0x1F600u32));
        assert_eq!(a, b);
        assert_ne!(
            a,
            Seeded.hash_one(("cluster", 0x1F601u32)),
            "the control: keys differ"
        );
        let now = seed();
        assert!(
            !fix_hash_seed(HashSeed(now.0.wrapping_add(1))),
            "the seed is already in use"
        );
        assert_eq!(seed(), now);
        let mut m: Map<u32, u32> = Map::default();
        m.insert(7, 70);
        assert_eq!(m.get(&7), Some(&70));
    }
}
