//! **Multicell blocks in eight-byte cells**: text drawn across a block of
//! cells at a [`TextSize`]. The block's top-left cell, its head, names a
//! [`MulticellSlot`] that holds the text and its size; every other cell of
//! the block is a tail, whose word says how far it is from the head, so a
//! reader finds the head from any cell without reading the store.
//!
//! The words sit above the placeholder range and below `0x1000_0000`:
//!
//! | words | what |
//! |---|---|
//! | `0x0900_0001..=0x0900_01B0` | a tail, `y << 6 \| x` from its head |
//! | `0x0900_0200` | the head of a block with no slot ([`MulticellId::REFUSED`]) |
//! | `0x0900_0201..=0x0900_0600` | the head of a block in slot `id - 1` |
//!
//! Every cell of a block keeps its own rendition and flags, so a rendition,
//! a link and protection apply to each cell as they do to any other.

use crate::*;

impl Content {
    /// Where the tail range starts: one past the placeholder range's last
    /// word, `0x08FF_FFFF`.
    const TAILS: u32 = 0x0900_0000;
    /// Where the head range starts: past every tail word.
    const HEADS: u32 = Content::TAILS + (1 << (TailAt::X_BITS + TailAt::Y_BITS));

    /// The word of a block's cell at `at` from its head.
    pub fn tail(at: TailAt) -> Content {
        Content(Content::TAILS + at.word())
    }
    /// How far this cell is from its block's head, if it is a tail.
    pub fn tail_at(self) -> Option<TailAt> {
        let v = self.0.wrapping_sub(Content::TAILS);
        match v < Content::HEADS - Content::TAILS {
            true => TailAt::of_word(v),
            false => None,
        }
    }
    /// The word of a block's head whose text is in `id`'s slot.
    pub fn multicell(id: MulticellId) -> Content {
        Content(Content::HEADS + u32::from(id.0))
    }
    /// The block this cell is the head of, if it is one.
    pub fn multicell_id(self) -> Option<MulticellId> {
        let v = self.0.wrapping_sub(Content::HEADS);
        match v <= Content::IDS {
            true => u16::try_from(v).ok().map(MulticellId),
            false => None,
        }
    }
    /// How many ids a head word names: [`MulticellId::REFUSED`] and one a
    /// slot.
    const IDS: u32 = MulticellStore::SLOTS_MAX as u32;
    /// Whether this cell continues a character to its left: the right half
    /// of a double-width character, or a block's tail. A reader that skips
    /// what the cell before it draws skips these.
    pub fn is_continuation(self) -> bool {
        self == Content::WIDE_TAIL || self.tail_at().is_some()
    }
}

// The head range ends below the top four bits, which stay zero.
const _: () = assert!(Content::HEADS + Content::IDS < 0x1000_0000);
// The farthest tail's word is below the head range.
const _: () = assert!(
    ((TailAt::Y_MAX as u32) << TailAt::X_BITS | TailAt::X_MAX as u32)
        < (1 << (TailAt::X_BITS + TailAt::Y_BITS))
);

/// Where a tail is from its block's head: `x` cells right and `y` rows
/// down, never both zero. A block is at most `7 * 7` cells wide and 7
/// tall, so `x` is at most 48 and `y` at most 6.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TailAt {
    x: u8,
    y: u8,
}

impl TailAt {
    const X_BITS: u32 = 6;
    const Y_BITS: u32 = 3;
    /// The farthest a tail is across from its head.
    pub const X_MAX: usize = SizeScale::MAX * SizeCells::MAX - 1;
    /// The farthest a tail is down from its head.
    pub const Y_MAX: usize = SizeScale::MAX - 1;

    pub fn new(x: usize, y: usize) -> Option<TailAt> {
        if (x == 0 && y == 0) || x > TailAt::X_MAX || y > TailAt::Y_MAX {
            return None;
        }
        Some(TailAt {
            x: u8::try_from(x).ok()?,
            y: u8::try_from(y).ok()?,
        })
    }
    pub fn x(self) -> usize {
        usize::from(self.x)
    }
    pub fn y(self) -> usize {
        usize::from(self.y)
    }
    fn word(self) -> u32 {
        u32::from(self.y) << TailAt::X_BITS | u32::from(self.x)
    }
    fn of_word(v: u32) -> Option<TailAt> {
        let x = v & ((1 << TailAt::X_BITS) - 1);
        let y = v >> TailAt::X_BITS;
        TailAt::new(usize::try_from(x).ok()?, usize::try_from(y).ok()?)
    }
}

/// Which slot a block's head names.
///
/// **Zero is a head with no slot**, [`MulticellId::REFUSED`]: a block the
/// store had no room for is still written at its size, with this head, so
/// every tail still has a head and a write over the block erases it as a
/// block. Slot `i` is id `i + 1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MulticellId(u16);

impl MulticellId {
    /// The head of a block written with no text: it draws nothing and
    /// copies nothing, and no sweep counts it as naming a slot.
    pub const REFUSED: MulticellId = MulticellId(0);
    pub fn is_refused(self) -> bool {
        self == MulticellId::REFUSED
    }
    fn of_slot(i: usize) -> MulticellId {
        MulticellId(u16::try_from(i + 1).unwrap_or(0))
    }
    fn slot(self) -> Option<usize> {
        usize::from(self.0).checked_sub(1)
    }
}

/// One block's text and size.
#[derive(Clone, Copy)]
pub struct MulticellSlot {
    size: TextSize,
    len: u8,
    cps: [u32; MulticellSlot::CAPACITY],
}

impl MulticellSlot {
    /// The most code points a block's text keeps; the rest are cut and
    /// counted ([`MulticellStore::cut`]).
    pub const CAPACITY: usize = 16;

    pub const EMPTY: MulticellSlot = MulticellSlot {
        size: TextSize::DEFAULT,
        len: 0,
        cps: [0; MulticellSlot::CAPACITY],
    };

    /// Whether the slot is free: a block's text is never empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn size(&self) -> TextSize {
        self.size
    }
    /// The text, as the slot keeps it.
    pub fn text(&self) -> &[u32] {
        &self.cps[..usize::from(self.len)]
    }
    fn is(&self, size: TextSize, cps: &[u32]) -> bool {
        usize::from(self.len) == cps.len() && self.size == size && self.text() == cps
    }
}

/// How many times a store's slots may have been handed to other blocks:
/// once a sweep that freed any, and once a reset. A reader holding cells
/// across reads compares it, since a freed slot's id names the next block
/// it is given to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MulticellEpoch(u32);

/// Why an insert found no slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MulticellRefused {
    /// No text, or a store with no slots: nothing a sweep can change.
    Shape,
    /// All [`MulticellStore::PROBES`] of the block's slots hold others. A
    /// sweep can free one.
    Full,
}

/// Which slots some head names, for a sweep: one bit a slot.
pub struct MulticellLive {
    bits: [u64; MulticellStore::SLOTS_MAX / 64],
}

impl MulticellLive {
    pub fn new() -> MulticellLive {
        MulticellLive {
            bits: [0; MulticellStore::SLOTS_MAX / 64],
        }
    }
    /// Every slot a head among `cells` names.
    pub fn mark(&mut self, cells: &[PackedCell]) {
        for c in cells {
            if let Some(i) = c.content.multicell_id().and_then(MulticellId::slot) {
                if let Some(w) = self.bits.get_mut(i / 64) {
                    *w |= 1 << (i % 64);
                }
            }
        }
    }
    fn has(&self, i: usize) -> bool {
        self.bits
            .get(i / 64)
            .is_some_and(|w| w & (1 << (i % 64)) != 0)
    }
}

impl Default for MulticellLive {
    fn default() -> Self {
        MulticellLive::new()
    }
}

/// A bounded, content-addressed store of blocks, indexed from the head
/// cell.
///
/// **It refuses rather than evicts**: an evicted slot would put another
/// block's text in a head. A block it has no slot for is written with
/// [`MulticellId::REFUSED`] and counted in [`MulticellStore::refused`].
///
/// **It reclaims what no head names, once every slot is filled**, as the
/// cluster store does: when a block finds its slots taken and every slot
/// holds a block, the caller may [`MulticellStore::sweep`] the cells that
/// can name one -- both screens and the history -- and every slot none of
/// them names is freed. A sweep runs at most once every
/// [`MulticellStore::DUE`] inserts that wanted a slot, so a flood of
/// distinct blocks into a store whose every slot is named walks the grid
/// once every `DUE` sequences and not on each.
pub struct MulticellStore<'b> {
    slots: &'b mut [MulticellSlot],
    /// Blocks written with no slot. Never silent.
    pub refused: usize,
    /// Code points of a block's text past [`MulticellSlot::CAPACITY`], and
    /// marks after a block with a full slot or none, left out.
    pub cut: usize,
    /// Inserts that wanted a slot -- a fill or a refusal for want of one --
    /// since the last sweep.
    since: u32,
    /// Slots holding a block.
    filled: usize,
    epoch: MulticellEpoch,
    swept: ClusterSweeps,
}

impl<'b> MulticellStore<'b> {
    /// How many slots an insert reads at most.
    pub const PROBES: usize = 8;
    /// The most slots a store uses, and the size of the sweep's live set:
    /// a larger slice is used to this length.
    pub const SLOTS_MAX: usize = 1024;
    /// How many inserts that wanted a slot come between two sweeps at
    /// least.
    pub const DUE: u32 = (MulticellStore::SLOTS_MAX / 16) as u32;

    /// A store over `slots`, as they are: the slots already filled are
    /// counted here, once.
    pub fn new(slots: &'b mut [MulticellSlot]) -> MulticellStore<'b> {
        let n = slots.len().min(MulticellStore::SLOTS_MAX);
        let filled = slots[..n].iter().filter(|s| !s.is_empty()).count();
        MulticellStore {
            slots,
            refused: 0,
            cut: 0,
            since: MulticellStore::DUE,
            filled,
            epoch: MulticellEpoch::default(),
            swept: ClusterSweeps::default(),
        }
    }

    fn len(&self) -> usize {
        self.slots.len().min(MulticellStore::SLOTS_MAX)
    }

    /// The slot a head names, if it names one that holds a block. A
    /// refused head names none.
    pub fn get(&self, content: Content) -> Option<&MulticellSlot> {
        let i = content.multicell_id()?.slot()?;
        if i >= self.len() {
            return None;
        }
        self.slots.get(i).filter(|s| !s.is_empty())
    }

    /// A head's text: its slot's, or none.
    pub fn text_of(&self, content: Content) -> &[u32] {
        match self.get(content) {
            Some(s) => s.text(),
            None => &[],
        }
    }

    /// Insert a block of `cps` at `size`, its text cut to
    /// [`MulticellSlot::CAPACITY`], returning its id or why there is none.
    /// The cut is counted only when a slot is found, so a caller that
    /// sweeps and inserts again counts it once; a refusal counts nothing,
    /// and the caller decides whether it is final.
    ///
    /// **Content-addressed: the same text at the same size is one slot.**
    /// Every probe is read before a free slot is taken, since a sweep frees
    /// slots in the middle of a run of filled ones.
    pub fn try_insert(
        &mut self,
        size: TextSize,
        cps: &[u32],
    ) -> Result<MulticellId, MulticellRefused> {
        let n = self.len();
        if cps.is_empty() || n == 0 {
            return Err(MulticellRefused::Shape);
        }
        let kept = &cps[..cps.len().min(MulticellSlot::CAPACITY)];
        let over = cps.len() - kept.len();
        let mut h: u64 = crate::cell::FNV_OFFSET;
        h ^= u64::from(size.packed());
        h = h.wrapping_mul(crate::cell::FNV_PRIME);
        for c in kept {
            h ^= u64::from(*c);
            h = h.wrapping_mul(crate::cell::FNV_PRIME);
        }
        let start = usize::try_from(h.wrapping_mul(crate::cell::FIBONACCI) >> 32).unwrap_or(0) % n;
        let mut free = None;
        for k in 0..MulticellStore::PROBES.min(n) {
            let i = (start + k) % n;
            let s = &self.slots[i];
            if s.is(size, kept) {
                self.cut += over;
                return Ok(MulticellId::of_slot(i));
            }
            if free.is_none() && s.is_empty() {
                free = Some(i);
            }
        }
        self.since = self.since.saturating_add(1);
        let Some(i) = free else {
            return Err(MulticellRefused::Full);
        };
        let mut s = MulticellSlot::EMPTY;
        s.size = size;
        s.len = u8::try_from(kept.len()).unwrap_or(0);
        s.cps[..kept.len()].copy_from_slice(kept);
        self.slots[i] = s;
        self.filled += 1;
        self.cut += over;
        Ok(MulticellId::of_slot(i))
    }

    /// [`MulticellStore::try_insert`], with a refusal counted and answered
    /// with [`MulticellId::REFUSED`].
    pub fn insert(&mut self, size: TextSize, cps: &[u32]) -> MulticellId {
        match self.try_insert(size, cps) {
            Ok(id) => id,
            Err(_) => {
                self.refused += 1;
                MulticellId::REFUSED
            }
        }
    }

    /// **A combining mark after the block `id`**: the block's text with
    /// `cp` after it, at the block's size, as a slot of its own, since
    /// other heads may name `id`'s. A block whose slot is full, or a
    /// refused head, keeps its id, and the mark is cut and counted.
    pub fn try_extend(
        &mut self,
        id: MulticellId,
        cp: u32,
    ) -> Result<MulticellId, MulticellRefused> {
        let slot = id
            .slot()
            .filter(|&i| i < self.len())
            .and_then(|i| self.slots.get(i))
            .filter(|s| !s.is_empty())
            .copied();
        let Some(s) = slot else {
            self.cut += 1;
            return Ok(id);
        };
        if s.text().len() >= MulticellSlot::CAPACITY {
            self.cut += 1;
            return Ok(id);
        }
        let mut cps = [0u32; MulticellSlot::CAPACITY];
        let n = s.text().len();
        cps[..n].copy_from_slice(s.text());
        cps[n] = cp;
        self.try_insert(s.size, &cps[..n + 1])
    }

    /// Whether a refusal for want of a slot should sweep: only when every
    /// slot holds a block, and then only once [`MulticellStore::DUE`]
    /// inserts have wanted a slot since the last sweep.
    pub fn sweep_due(&self) -> bool {
        self.filled >= self.len() && self.since >= MulticellStore::DUE
    }

    /// How many slots hold a block.
    pub fn filled(&self) -> usize {
        self.filled
    }

    /// **Free every slot no head in `live` names**, and say how many.
    /// `live` must have marked every cell that can name a slot.
    pub fn sweep(&mut self, live: &MulticellLive) -> usize {
        let mut freed = 0;
        let n = self.len();
        for (i, s) in self.slots[..n].iter_mut().enumerate() {
            if !s.is_empty() && !live.has(i) {
                *s = MulticellSlot::EMPTY;
                freed += 1;
            }
        }
        self.since = 0;
        self.filled -= freed;
        self.swept.sweeps += 1;
        self.swept.freed += freed;
        if freed > 0 {
            self.epoch = MulticellEpoch(self.epoch.0.wrapping_add(1));
        }
        freed
    }

    /// See [`MulticellEpoch`].
    pub fn epoch(&self) -> MulticellEpoch {
        self.epoch
    }

    /// How many sweeps ran and how many slots they freed.
    pub fn sweeps(&self) -> ClusterSweeps {
        self.swept
    }

    /// Forget everything. Every head that named a slot is being blanked.
    pub fn reset(&mut self) {
        for s in self.slots.iter_mut() {
            *s = MulticellSlot::EMPTY;
        }
        self.since = MulticellStore::DUE;
        self.filled = 0;
        self.epoch = MulticellEpoch(self.epoch.0.wrapping_add(1));
    }
}
