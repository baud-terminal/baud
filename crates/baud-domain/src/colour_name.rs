//! A colour name found in a table built at build time: two hashes and one
//! comparison, never a scan.
//!
//! `build/colour_names.rs` compiles each table into three arrays: a seed
//! for each of 256 buckets, 1024 [`ColourSlot`]s of a name's place in the
//! pool, its length and its colour, and the pool of lower-cased names end
//! to end.
//! FNV-1a over the name, lower-cased, picks the bucket; the bucket's seed
//! picks the slot; the slot's name is compared, case ignored. kitty's table
//! and the X server's are both looked up here.

/// One slot of a table: a name, as its offset and length in the pool, and
/// its colour; a length of 0 is an empty slot. Six bytes at an alignment
/// of two, the fields where the `(u16, u8, [u8; 3])` it replaces kept them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub(crate) struct ColourSlot {
    pub(crate) at: u16,
    pub(crate) len: u8,
    pub(crate) rgb: [u8; 3],
}

const _: () = {
    assert!(core::mem::size_of::<ColourSlot>() == 6);
    assert!(core::mem::align_of::<ColourSlot>() == 2);
    assert!(core::mem::offset_of!(ColourSlot, len) == 2);
    assert!(core::mem::offset_of!(ColourSlot, rgb) == 3);
};

/// The colour `name` has in the table, or `None`; case is ignored and
/// nothing else is.
pub(crate) fn find(
    seeds: &[u16; 256],
    slots: &[ColourSlot; 1024],
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
    let ColourSlot {
        at,
        len,
        rgb: colour,
    } = slots[usize::try_from(fnv(bucket) % 1024).ok()?];
    let at = usize::from(at);
    let stored = pool.get(at..at + usize::from(len))?;
    (len != 0 && stored.eq_ignore_ascii_case(name)).then_some(colour)
}
