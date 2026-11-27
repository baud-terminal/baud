//! A two-stage trie over every code point.

use crate::{Error, Kind};
use std::collections::HashMap;
use std::hash::Hash;

/// `values` as two stages: `index[cp >> shift]` names a block, and the
/// block holds the low bits' values. Blocks are numbered in the order they
/// are first seen, walking up from U+0000, so the table is one function of
/// the values; the map that dedups them is only looked up, never walked.
#[derive(Debug)]
pub struct Trie<T> {
    pub index: Vec<u8>,
    /// The distinct blocks, one after another.
    pub blocks: Vec<T>,
    pub distinct: usize,
}

impl<T: Copy + Eq + Hash> Trie<T> {
    /// The trie of `values` with blocks of `1 << shift`, checked against
    /// `values` at every position before it is returned, and refused when
    /// its blocks do not fit a `u8` stage 1. `name` names the table in a
    /// failure.
    pub fn build(name: &str, values: &[T], shift: u32) -> Result<Trie<T>, Error> {
        let block = 1usize << shift;
        if values.is_empty() || !values.len().is_multiple_of(block) {
            return Err(Error::new(
                Kind::Invariant,
                name,
                format!("{} values are not whole blocks of {block}", values.len()),
            ));
        }
        let mut seen: HashMap<&[T], u8> = HashMap::new();
        let mut index = Vec::with_capacity(values.len() / block);
        let mut blocks = Vec::new();
        let mut distinct = 0usize;
        for chunk in values.chunks(block) {
            let n = match seen.get(chunk) {
                Some(&n) => n,
                None => {
                    let n = u8::try_from(distinct).map_err(|_| {
                        Error::new(
                            Kind::Invariant,
                            name,
                            format!("more than {distinct} blocks will not fit a u8 stage 1"),
                        )
                    })?;
                    seen.insert(chunk, n);
                    blocks.extend_from_slice(chunk);
                    distinct += 1;
                    n
                }
            };
            index.push(n);
        }
        let trie = Trie {
            index,
            blocks,
            distinct,
        };
        if let Some(i) = (0..values.len()).find(|&i| trie.get(i, shift) != Some(values[i])) {
            return Err(Error::new(
                Kind::Invariant,
                name,
                format!("the trie disagrees with its values at {i:#X}"),
            ));
        }
        Ok(trie)
    }

    /// The value at position `i`, read back through both stages.
    pub fn get(&self, i: usize, shift: u32) -> Option<T> {
        let block = 1usize << shift;
        let b = usize::from(*self.index.get(i >> shift)?);
        self.blocks.get(b * block + (i & (block - 1))).copied()
    }
}
