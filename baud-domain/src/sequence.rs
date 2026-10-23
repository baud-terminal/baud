//! The sequences the Unicode Standard names, as clusters no store holds.
//!
//! A ZWJ family, a flag, a keycap, a skin-toned emoji and a variation
//! sequence are each a grapheme cluster `[[source: uax29]]` makes of
//! several codepoints, and each is listed: UTS #51's RGI emoji sequences
//! (`[[source: emoji17-sequences]]`, `[[source: emoji17-zwj-sequences]]`)
//! and the standardized emoji variation sequences
//! (`[[source: emoji-variation-sequences]]`). Every prefix of one, up to a
//! base and [`ClusterSlot::CAPACITY`] continuations, is a node of a trie
//! built from those files (`scripts/gen-sequence-table.py`), and a cell
//! whose cluster is a node names the node: [`ClusterId::SEQUENCES`] plus
//! its number, above the store's slots. Each node's slot is built at
//! compile time, so [`ClusterStore::get`] answers for a node as for a slot.
//!
//! **It decides storage, never a boundary.** The cluster machine decides
//! that a codepoint joins the cell before it, as it did; this answers only
//! where the cluster that results is kept. A cluster that leaves the trie
//! -- a combining mark after a family, a sequence no file lists -- goes to
//! the [`ClusterStore`] whole, base and every codepoint, as it always went.
//!
//! **A step is one bucket seed, one entry and a compare.** The edge from a
//! cell's word and the next codepoint to the child node is a perfect hash
//! the generator built, so nothing probes and nothing searches. The store
//! re-hashed the whole growing cluster on every continuation, six times a
//! growing list for a family of seven (docs/record.md, "A named emoji
//! sequence is a trie step, not an intern").

use crate::sequence_data::*;
use crate::*;

/// Every node's slot: its base, then its continuations.
static SLOTS: [ClusterSlot; NODES] = {
    let mut out = [ClusterSlot::EMPTY; NODES];
    let mut i = 0;
    while i < NODES {
        let s = NODE_SPAN[i];
        let (off, len) = ((s & 0xffff) as usize, (s >> 16) as usize);
        let (_, rest) = SPANS.split_at(off);
        let (seq, _) = rest.split_at(len);
        out[i] = ClusterSlot::named(seq);
        i += 1;
    }
    out
};

/// The slot of the node `content` names, if it names one.
#[inline(always)]
pub(crate) fn slot(content: Content) -> Option<&'static ClusterSlot> {
    let node = content.bits().checked_sub(WORD0)?;
    SLOTS.get(node as usize)
}

/// Whether some sequence continues with a codepoint of `cp`'s block of
/// 256 (`CONTINUES`): a Hebrew point, a Devanagari sign or a kana voicing
/// mark is answered by this one bit.
#[inline(always)]
pub(crate) fn continues(cp: Codepoint) -> bool {
    let block = (cp.value() >> 8) as usize;
    CONTINUES
        .get(block >> 3)
        .is_some_and(|b| b & (1 << (block & 7)) != 0)
}

/// The node `cp` makes of the cluster a cell with content `at` holds, and
/// its slot, if the result is a prefix of a named sequence.
#[inline(always)]
pub(crate) fn step(at: Content, cp: Codepoint) -> Option<(ClusterId, &'static ClusterSlot)> {
    // A codepoint or a node's word: 21 bits. Anything above is a slot's
    // word past the nodes or a placeholder's, and neither is in the trie.
    let w = at.bits();
    if w >= 1 << 21 {
        return None;
    }
    let c = cp.value();
    let key = (u64::from(w) << 21) | u64::from(c);
    let bucket = (key.wrapping_mul(M1) >> (64 - BUCKET_BITS)) as usize;
    let seed = u64::from(*SEEDS.get(bucket)?);
    let at = ((key ^ seed).wrapping_mul(M2) >> (64 - EDGE_BITS)) as usize;
    let e = *EDGES.get(at)?;
    // An empty entry is zero and no key is: `c` is a continuation, so it
    // is above zero.
    if e >> 16 != key {
        return None;
    }
    let node = (e & 0xffff) as usize - 1;
    Some((ClusterId::sequence(node), SLOTS.get(node)?))
}

// The generator's word for node 0 is the cell word `ClusterId::SEQUENCES`
// names, `0x8000`, so every node's id fits a `u16` with the 0x7FFF the
// generator allows it.
const _: () = assert!(WORD0 == Content::cluster_word(ClusterId::SEQUENCES));
const _: () = assert!(NODES <= 0x7FFF);
