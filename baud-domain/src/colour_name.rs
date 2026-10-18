//! A colour name found in a table built at build time: two hashes and one
//! comparison, never a scan.
//!
//! `scripts/gen-colour-names.py` writes each table as three arrays: a seed
//! for each of 256 buckets, 1024 slots of a name's place in the pool, its
//! length and its colour, and the pool of lower-cased names end to end.
//! FNV-1a over the name, lower-cased, picks the bucket; the bucket's seed
//! picks the slot; the slot's name is compared, case ignored. kitty's table
//! and the X server's are both looked up here.

/// The colour `name` has in the table, or `None`; case is ignored and
/// nothing else is.
pub(crate) fn find(
    seeds: &[u16; 256],
    slots: &[(u16, u8, [u8; 3]); 1024],
    pool: &[u8],
    name: &[u8],
) -> Option<[u8; 3]> {
    if name.len() > usize::from(u8::MAX) {
        return None;
    }
    let fnv = |seed: u16| {
        name.iter().fold(0x811c_9dc5 ^ u32::from(seed), |h, &b| {
            (h ^ u32::from(b.to_ascii_lowercase())).wrapping_mul(0x0100_0193)
        })
    };
    let bucket = seeds[usize::try_from(fnv(0) % 256).ok()?];
    let (at, len, colour) = slots[usize::try_from(fnv(bucket) % 1024).ok()?];
    let at = usize::from(at);
    let stored = pool.get(at..at + usize::from(len))?;
    (len != 0 && stored.eq_ignore_ascii_case(name)).then_some(colour)
}
