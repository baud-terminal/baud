//! The run cache: shaped runs by content, 256 sets of 8 ways.
//!
//! ghostty measured shaping at 96% of frame time before its cache existed
//! (`docs/research/notes/j-rendering-evidence.md` E8), and Fira Code's
//! chained singles are 3.5x a plain ligature. So the cache is load-bearing
//! from the first frame: a run's key is its face and every cell's content
//! word and attached codepoints, hashed once, and a hit answers the whole
//! run's glyph placement without touching the font.

use crate::units::Tally;
use baud_domain::GlyphId;

/// The most cells one run shapes; a longer row is shaped in pieces, each
/// its own cache entry.
pub const RUN_CELLS: usize = 32;
/// The most glyphs a run may place: a cell yields a base and at most one
/// glyph per attached codepoint, and the core stores eight.
pub const PLACED_MAX: usize = 96;

/// A cell offset within a run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellIx(pub u8);

/// One glyph the shaper placed: at which cell of the run it starts, the
/// last cell it spans, and its offset from that cell's origin in font
/// units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placed {
    pub id: GlyphId,
    pub cell: CellIx,
    pub last: CellIx,
    pub dx: i16,
    pub dy: i16,
}

impl Placed {
    pub const NONE: Placed = Placed {
        id: GlyphId::NOTDEF,
        cell: CellIx(0),
        last: CellIx(0),
        dx: 0,
        dy: 0,
    };
}

/// How many glyphs a run placed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlacedCount(u8);

/// A run's placement, fixed-size so a cache slot is a plain copy.
#[derive(Clone, Copy)]
pub struct Shaped {
    placed: [Placed; PLACED_MAX],
    len: PlacedCount,
}

impl Shaped {
    pub const EMPTY: Shaped = Shaped {
        placed: [Placed::NONE; PLACED_MAX],
        len: PlacedCount(0),
    };

    /// Appends a glyph; `None` once the run is full, which the caller
    /// treats as a run it cannot cache or draw beyond.
    pub fn push(&mut self, p: Placed) -> Option<()> {
        let slot = self.placed.get_mut(usize::from(self.len.0))?;
        *slot = p;
        self.len = PlacedCount(self.len.0.checked_add(1)?);
        Some(())
    }

    pub fn glyphs(&self) -> &[Placed] {
        self.placed.get(..usize::from(self.len.0)).unwrap_or(&[])
    }
}

/// A run's identity: an FNV-1a fold over its face and cells. Zero is
/// reserved for an empty slot and never produced.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key(u64);

impl Key {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub fn start() -> Key {
        Key(Key::OFFSET)
    }

    pub fn fold(self, word: u32) -> Key {
        let mut h = self.0;
        for b in word.to_le_bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(Key::PRIME);
        }
        Key(h)
    }

    /// The key as stored: never zero, which marks an empty slot.
    fn finish(self) -> Key {
        if self.0 == 0 { Key(1) } else { self }
    }

    const EMPTY: Key = Key(0);
}

/// A set's index.
#[derive(Clone, Copy)]
struct SetIx(usize);

#[derive(Clone, Copy)]
struct Entry {
    key: Key,
    shaped: Shaped,
}

const SETS: usize = 256;
const WAYS: usize = 8;

/// How the cache has been doing, for the frame report.
#[derive(Clone, Copy, Default, Debug)]
pub struct CacheStats {
    pub hits: Tally,
    pub misses: Tally,
}

pub struct RunCache {
    sets: Vec<[Entry; WAYS]>,
    /// Round-robin victim per set.
    next: Vec<u8>,
    stats: CacheStats,
}

impl Default for RunCache {
    fn default() -> Self {
        RunCache::new()
    }
}

impl RunCache {
    pub fn new() -> RunCache {
        RunCache {
            sets: vec![
                [Entry {
                    key: Key::EMPTY,
                    shaped: Shaped::EMPTY,
                }; WAYS];
                SETS
            ],
            next: vec![0; SETS],
            stats: CacheStats::default(),
        }
    }

    fn set_of(key: Key) -> SetIx {
        // The high bits, as `StyleId::bucket` learned: the low ones of a
        // multiplicative fold are the least mixed.
        SetIx(usize::try_from(key.0.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 56).unwrap_or(0) % SETS)
    }

    pub fn get(&mut self, key: Key) -> Option<Shaped> {
        let k = key.finish();
        let set = &self.sets[Self::set_of(k).0];
        match set.iter().find(|e| e.key == k) {
            Some(e) => {
                self.stats.hits.bump();
                Some(e.shaped)
            }
            None => {
                self.stats.misses.bump();
                None
            }
        }
    }

    pub fn put(&mut self, key: Key, shaped: Shaped) {
        let k = key.finish();
        let s = Self::set_of(k).0;
        let way = usize::from(self.next[s]) % WAYS;
        self.next[s] = self.next[s].wrapping_add(1);
        self.sets[s][way] = Entry { key: k, shaped };
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Forgets everything: a font or size change.
    pub fn clear(&mut self) {
        for set in &mut self.sets {
            for e in set.iter_mut() {
                e.key = Key::EMPTY;
            }
        }
    }
}
