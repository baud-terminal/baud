//! The run cache: shaped runs by content, 256 sets of 8 ways.
//!
//! ghostty measured shaping at 96% of frame time before its cache existed
//! (`docs/research/notes/j-rendering-evidence.md` E8), and Fira Code's
//! chained singles are 3.5x a plain ligature. So the cache is load-bearing
//! from the first frame: a run's key is its face and every cell's content
//! word and attached codepoints, hashed once, and a hit answers the whole
//! run's glyph placement without touching the font.

use crate::stack::FaceIx;
use crate::units::Tally;
use baud_domain::{Codepoint, GlyphId};

/// The most cells one run shapes; a longer row is shaped in pieces, each
/// its own cache entry.
pub const RUN_CELLS: usize = 32;
/// The most glyphs a run may place: a cell yields a base and at most one
/// glyph per attached codepoint, and the core stores eight.
pub const PLACED_MAX: usize = 96;

/// A cell offset within a run.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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
    /// Whether the run held a letter that takes a joining form, which the
    /// shaper tested as it took each cluster: a run without one is never
    /// fitted to its cells (`crate::fit`), and is asked nothing more.
    joins: crate::bidi::Joins,
}

impl Shaped {
    pub const EMPTY: Shaped = Shaped {
        placed: [Placed::NONE; PLACED_MAX],
        len: PlacedCount(0),
        joins: crate::bidi::Joins::Unseen,
    };

    /// Whether the run held a letter that takes a joining form.
    pub(crate) fn joins(&self) -> crate::bidi::Joins {
        self.joins
    }

    /// Records whether the run held a letter that takes a joining form.
    pub(crate) fn set_joins(&mut self, joins: crate::bidi::Joins) {
        self.joins = joins;
    }

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

/// A [`ClusterMemo`] slot's index.
#[derive(Clone, Copy)]
struct SlotIx(usize);

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

/// How many clusters a [`ClusterMemo`] remembers, direct-mapped: a power
/// of two so a slot is the key's top bits. A memo is 17 KB -- a 16-byte key
/// and a one-byte answer, 1,024 times -- and a renderer holds two, so 34 KB
/// of working set that only a marked cell touches; what that costs beside
/// the run cache is unmeasured. The payload the memo was measured on
/// (`scripts/gen-marks-payload.py`: twelve letters under six mark sets, and
/// one `q` with U+0316 and U+0301) names 73 distinct marked clusters; how
/// many a real session names is uncounted. Two that share a slot cost a
/// recomputation, never a wrong answer.
const CLUSTER_SLOTS: usize = 1024;

/// The codepoints a cluster key holds exactly: with a leading one, a bit
/// for the question and the face's byte, five of 21 bits fill 115 of the
/// 128, so an exact key never reaches the top bit a folded one carries.
const EXACT_CPS: usize = 5;

/// A cluster's identity in a [`ClusterMemo`]: the face asked about and the
/// codepoints, packed exactly behind a leading one -- so two lengths never
/// meet and the key is never zero, which marks an empty slot -- for every
/// cluster of up to [`EXACT_CPS`] codepoints. A longer one is the run
/// cache's fold with the top bit set, trusted as the run cache trusts it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ClusterKey(u128);

impl ClusterKey {
    const EMPTY: ClusterKey = ClusterKey(0);

    fn of(question: Question, face: FaceIx, cps: &[Codepoint]) -> ClusterKey {
        let face = u8::try_from(face.index()).unwrap_or(u8::MAX);
        let asked: u8 = match question {
            Question::Face => 0,
            Question::Cover => 1,
        };
        if cps.len() <= EXACT_CPS {
            let mut k = (1u128 << 9) | (u128::from(asked) << 8) | u128::from(face);
            for c in cps {
                k = (k << 21) | u128::from(c.value());
            }
            return ClusterKey(k);
        }
        let mut h = Key::start().fold(u32::from(asked)).fold(u32::from(face));
        for c in cps {
            h = h.fold(c.value());
        }
        ClusterKey((1u128 << 127) | u128::from(h.0))
    }

    /// The slot: both halves folded and multiplied, the high bits taken, as
    /// the run cache's sets take them.
    fn slot(self) -> SlotIx {
        let halves = u64::try_from(self.0 >> 64).unwrap_or(0)
            ^ u64::try_from(self.0 & u128::from(u64::MAX)).unwrap_or(0);
        let top = u64::BITS - CLUSTER_SLOTS.trailing_zeros();
        SlotIx(usize::try_from(halves.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> top).unwrap_or(0))
    }
}

/// Which question a [`ClusterMemo`] answers, folded into its keys so the
/// two memos' keys could never be confused if they shared a table.
#[derive(Clone, Copy)]
pub enum Question {
    /// Which face draws the cluster, asked with its base's face.
    Face,
    /// Whether the face asked about draws all of it.
    Cover,
}

/// Whether a face draws all of a cluster -- its base and every mark that is
/// not default-ignorable -- or leaves some of it out (`Normalised::covered`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cover {
    Whole,
    Partial,
}

/// One answer about a marked cluster, remembered: which face draws it
/// ([`crate::Stack::select_cluster`]), or whether a face draws all of it.
/// **Per-cell work is a lookup**, and either answer underneath is a
/// normalisation and a `cmap` lookup per codepoint per face tried --
/// `Face::glyph` walks the `cmap` subtables each time, which is why the base's
/// face is a table ([`crate::FaceTable`]). In an earlier session, on a payload
/// of 2,547 marked cells a frame, every mark one JetBrains Mono has, before the
/// memo held the second answer: asking it on every shaping was 115 us of a
/// 1,462 us convert (medians of five settled rounds, against an arm that never
/// asked).
///
/// **What marked cells pay is not separated from what the build moved.** On
/// that payload (`scripts/gen-marks-payload.py covered`: nothing moves face
/// and nothing is dropped, so both arms do the same work), against the code
/// before any of this, with an A/A copy of the new binary, interleaved,
/// seven settled rounds on mains: convert 1,307 us [1,296-1,326] before and
/// 1,328 [1,316-1,345] after, +1.7% by the median paired round over before's
/// median. Every paired round was above before, but the ranges overlap, and
/// the A/A pair differed by -2.7 (2 of 7 above). The same two binaries
/// converted text with no marks faster on equal work -- asciiquarium 533 us
/// [531-546] before and 502 [493-516] after, disjoint; git-log below before
/// in every round, overlapping -- and about two thirds of this payload's
/// cells carry no marks (15% are spaces, 60% of letters bare), so that shift,
/// larger and of the other sign, can sit under the +1.7% too. What the change
/// adds for a marked cell, priced by no arm, is a memo probe and a second
/// read of its marks: the run path reads them once for its face
/// (`Renderer::face_of_cell`) and once where it gathers the run, so that a
/// cell with no marks reads nothing it did not read before. The second read
/// goes onto the stack: a version that read into a `Vec` measured +4% in an
/// earlier session, on battery. Why plain text converted faster is
/// unattributed -- the binaries differ in the gather loop, which now calls
/// `face_of_cell` out of line, and in `shape` -- and it is not claimed.
///
/// **Nothing invalidates it.** A key holds the face asked about, which
/// already carries the style and the presentation -- so a change of
/// [`crate::EmojiDefault`] keys anew -- and the stack it was asked of is the
/// renderer's for the renderer's life.
pub struct ClusterMemo<T> {
    question: Question,
    keys: Vec<ClusterKey>,
    answers: Vec<T>,
}

impl<T: Copy> ClusterMemo<T> {
    /// An empty memo for `question`; `blank` fills the unused answers.
    pub fn new(question: Question, blank: T) -> ClusterMemo<T> {
        ClusterMemo {
            question,
            keys: vec![ClusterKey::EMPTY; CLUSTER_SLOTS],
            answers: vec![blank; CLUSTER_SLOTS],
        }
    }

    /// The answer for `cps` asked with `face`: from its slot when the slot
    /// holds it, else `ask`'s, kept.
    pub fn answer(&mut self, face: FaceIx, cps: &[Codepoint], ask: impl FnOnce() -> T) -> T {
        let k = ClusterKey::of(self.question, face, cps);
        let SlotIx(slot) = k.slot();
        if self.keys[slot] == k {
            return self.answers[slot];
        }
        let answer = ask();
        self.keys[slot] = k;
        self.answers[slot] = answer;
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cps(s: &[u32]) -> Vec<Codepoint> {
        s.iter().filter_map(|&c| Codepoint::new(c)).collect()
    }

    /// The exact key's three claims: two lengths never meet, even when the
    /// longer one ends in U+0000; the key is never the empty slot's zero;
    /// and only a cluster past [`EXACT_CPS`] is folded, under the top bit.
    #[test]
    fn a_cluster_key_is_exact_up_to_five_codepoints() {
        let key = |q, s: &[u32]| ClusterKey::of(q, FaceIx::PRIMARY, &cps(s)).0;
        let f = Question::Face;
        assert_ne!(key(f, &[0x71, 0x301]), key(f, &[0x71, 0x301, 0]));
        assert_ne!(key(f, &[0]), key(f, &[0, 0]));
        assert_ne!(key(f, &[0x71, 0x301, 0x323]), key(f, &[0x71, 0x323, 0x301]));
        assert_ne!(key(f, &[0x71, 0x301]), key(Question::Cover, &[0x71, 0x301]));
        assert_ne!(key(f, &[]), 0, "never the empty slot");
        let five = [0x10FFFF; EXACT_CPS];
        assert_eq!(key(f, &five) >> 127, 0, "five of the largest stay exact");
        assert_eq!(key(f, &[0x71; EXACT_CPS + 1]) >> 127, 1, "six are folded");
    }

    /// A memo answers from its slot and asks again for any other key.
    #[test]
    fn a_memo_asks_once_per_cluster() {
        let mut memo = ClusterMemo::new(Question::Cover, Cover::Whole);
        let mut asked = 0;
        for s in [
            &[0x71, 0x301][..],
            &[0x71, 0x301],
            &[0x71, 0x316],
            &[0x71, 0x301],
        ] {
            memo.answer(FaceIx::PRIMARY, &cps(s), || {
                asked += 1;
                Cover::Partial
            });
        }
        assert_eq!(asked, 2);
    }
}
