//! **Renditions frozen off the active screen**, so the rendition table never
//! gives a slot a drawn cell names to another rendition.
//!
//! A cell holds a two-byte [`StyleId`] into a table of at most 65,536
//! slots, sized to the screen. A picture printed into the shell names one
//! rendition a cell, and once the screen and the history behind it name more
//! than the table has slots, the only room left is a slot some cell still
//! names. `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does
//! the selection of current rendition affect the rendition of characters
//! already entered into the display", and `[[source: itu-t416]]` § 8.1.7 gives
//! each character its own foreground and background. Giving the slot away
//! recolours those characters.
//!
//! So the table serves the **active screen** -- at most one screen of cells,
//! its status row and three reserved slots, which the table is sized to hold
//! -- and what lies off it (the history, the rows a shorter window cut, the
//! screen not up) can be moved into a **generation**: its cells are
//! rewritten to dense ids into the generation's own list of rendition words,
//! here, in caller memory. Then the table's sweep, marking only the rows that
//! still resolve through it, frees the rest.
//!
//! **A row finds its generation from a tag one a ring row**: the line it held
//! when it was frozen, where it stood (history, screen or past the bottom),
//! and its generation. A tag is believed only while the row still holds that
//! line where it stood, so the line feed, which reuses a row for a new line,
//! writes nothing here; every move that brings a row's cells back onto the
//! active screen thaws it, and every one that takes a screen row off clears
//! its tag (`baud-core`'s `freeze.rs`).

use crate::{LineIdx, RowMut, RowRot, Rows, StyleId, StyleWord};

/// **Which generation record a frozen row names.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct GenIdx(u32);

impl GenIdx {
    pub fn index(self) -> usize {
        self.0 as usize
    }
    fn at(i: usize) -> GenIdx {
        GenIdx(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// **A generation's identity, minted once and never reused**: what a reader
/// keys a resolved copy of its words by. A record that moves keeps it, since
/// its words are the same; one whose ids are renumbered gets a new one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(transparent)]
pub struct FrozenSerial(u64);

impl FrozenSerial {
    pub fn value(self) -> u64 {
        self.0
    }
}

/// **Where a primary row stood when its cells were frozen**, measured
/// against the window: behind it, on it while the alternate screen was up,
/// or past its bottom where a shorter window cut it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrozenAt {
    History,
    Screen,
    Hidden,
}

impl FrozenAt {
    /// Where line `l` stands against a window whose top line is `top`,
    /// `rows` tall.
    pub fn of(l: LineIdx, top: LineIdx, rows: Rows) -> FrozenAt {
        match (l < top, l < top.plus(rows)) {
            (true, _) => FrozenAt::History,
            (false, true) => FrozenAt::Screen,
            (false, false) => FrozenAt::Hidden,
        }
    }
}

/// **A frozen primary row**: the line it held, where it stood, and the
/// generation its cells name ids in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowFrozen {
    pub line: LineIdx,
    pub at: FrozenAt,
    pub generation: GenIdx,
}

/// Whether a generation record holds words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GenState {
    Dead,
    Live,
}

/// **One generation's words**: where they start in the pool, how many, and
/// whose they are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenGen {
    start: usize,
    len: usize,
    serial: FrozenSerial,
    state: GenState,
    /// Where a compaction moves this record: written, then read by every
    /// tag, then the record moved.
    to: usize,
}

impl FrozenGen {
    pub const EMPTY: FrozenGen = FrozenGen {
        start: 0,
        len: 0,
        serial: FrozenSerial(0),
        state: GenState::Dead,
        to: 0,
    };

    pub fn serial(&self) -> FrozenSerial {
        self.serial
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// **A live slot's dense id in the generation being built**, or a frozen
/// id's live slot in a thaw: one plus the id, so zeroed memory is a map
/// with nothing in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(transparent)]
pub struct Remap(u16);

impl Remap {
    pub const UNSET: Remap = Remap(0);
    fn of(id: StyleId) -> Remap {
        Remap(id.0.wrapping_add(1))
    }
    fn get(self) -> Option<StyleId> {
        self.0.checked_sub(1).map(StyleId)
    }
}

/// Whether a thaw is interning a generation's words, during which no freeze
/// may run: one would renumber or free the generation it reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Thawing {
    #[default]
    Idle,
    Busy,
}

/// **What the last freeze saw**: the primary's top line and the count of the
/// moves that can leave a row off the screen unfrozen without moving it
/// (`FrozenBook::moved`). A freeze that finds both where it left them has
/// nothing new to freeze and returns before walking a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeMark {
    top: LineIdx,
    moves: u64,
}

/// **What the generations have done**, counted for a reader and a test.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FreezeCounts {
    /// Freezes that moved at least one row into a generation.
    pub freezes: u64,
    /// Rows moved into a generation, over every freeze.
    pub rows_frozen: u64,
    /// Cells rewritten to a generation's ids or back to the table's: the
    /// bytes a freeze and a thaw write, eight a cell.
    pub cells_rewritten: u64,
    /// Rows interned again into the table as they came back onto the
    /// active screen.
    pub rows_thawed: u64,
    /// Generations freed because no row resolves through them.
    pub gens_freed: u64,
    /// Pool compactions: the dead generations' words given back.
    pub compactions: u64,
    /// Generations rebuilt to the words their rows still name, when the
    /// pool had no room left after a compaction.
    pub rebuilds: u64,
    /// Rows a freeze left in the table because the pool or the generation
    /// records had no room for them.
    pub rows_unfrozen: u64,
    /// The most words the pool has held at once.
    pub words_peak: usize,
    /// Lines a re-cut found naming more renditions across their tables
    /// than one generation holds, cut where the table changes rather than
    /// joined (`baud-core`'s `frozen_normalized`).
    pub lines_split: u64,
}

/// **What the generations hold now**, and what they have done.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FrozenReport {
    pub counts: FreezeCounts,
    /// Words the pool holds, twenty bytes each.
    pub words_held: usize,
    /// Generations that hold words.
    pub gens_alive: usize,
}

/// **The generations' bookkeeping, in the caller's memory beside the pool**,
/// as `StyleBook` is beside the table: a `Term` lent the storage again finds
/// the generations its cells name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenBook {
    used: usize,
    gens: usize,
    serial: u64,
    alt: Option<GenIdx>,
    mark: Option<FreezeMark>,
    moves: u64,
    thawing: Thawing,
    counts: FreezeCounts,
}

impl Default for FrozenBook {
    fn default() -> FrozenBook {
        FrozenBook::NEW
    }
}

impl FrozenBook {
    pub const NEW: FrozenBook = FrozenBook {
        used: 0,
        gens: 0,
        serial: 0,
        alt: None,
        mark: None,
        moves: 0,
        thawing: Thawing::Idle,
        counts: FreezeCounts {
            freezes: 0,
            rows_frozen: 0,
            cells_rewritten: 0,
            rows_thawed: 0,
            gens_freed: 0,
            compactions: 0,
            rebuilds: 0,
            rows_unfrozen: 0,
            words_peak: 0,
            lines_split: 0,
        },
    };
}

/// **How much memory the generations need** for a ring of `ring` rows
/// whose screen is `screen` rows, at `stride` cells a row: the most cells
/// that can lie off the active screen at once (the whole ring, while the
/// alternate screen is up), a word for each generation's default, and one
/// table's worth of room to build a generation in before the pool is
/// compacted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenBudget {
    pub words: usize,
    pub gens: usize,
    pub rows: usize,
}

impl FrozenBudget {
    /// No generations: a table that cannot place a rendition refuses it.
    pub const NONE: FrozenBudget = FrozenBudget {
        words: 0,
        gens: 0,
        rows: 0,
    };

    pub fn of(ring: Rows, stride: crate::Stride, slots: usize) -> FrozenBudget {
        // Off the active screen at once: the ring less the screen and the
        // alternate screen's cells while the primary is up, the whole ring
        // while the alternate screen is; the alternate screen is a screen's
        // cells, so the ring either way.
        let ring_cells = ring.index() * stride.index();
        let gens = ring.index() + 2;
        FrozenBudget {
            words: ring_cells + gens + slots,
            gens,
            rows: ring.index(),
        }
    }
}

#[cfg(feature = "alloc")]
impl FrozenBudget {
    /// The pool from the allocator's zeroed path, so a page is committed
    /// only when a generation first writes it and a terminal that never
    /// freezes pays for none of it.
    pub fn zeroed_words(&self) -> alloc::vec::Vec<StyleWord> {
        use zerocopy::FromZeros;
        match StyleWord::new_vec_zeroed(self.words) {
            Ok(v) => v,
            Err(_) => match core::alloc::Layout::array::<StyleWord>(self.words) {
                Ok(layout) => alloc::alloc::handle_alloc_error(layout),
                Err(_) => panic!("capacity overflow"),
            },
        }
    }

    /// The map, one a slot, from zeroed memory: zero is unset.
    pub fn zeroed_map(slots: usize) -> alloc::vec::Vec<Remap> {
        alloc::vec![Remap::UNSET; slots]
    }
}

/// **The caller memory a freeze writes**, lent to `Storage::with_frozen`.
pub struct FrozenBuf<'a> {
    /// Every generation's words, one after another.
    pub words: &'a mut [StyleWord],
    /// The generation records.
    pub gens: &'a mut [FrozenGen],
    /// One a row of the primary ring.
    pub tags: &'a mut [Option<RowFrozen>],
    /// One a row of the primary ring: what a resize reads each row's
    /// generation from while it moves the rows.
    pub spare: &'a mut [Option<GenIdx>],
    /// One a slot of the rendition table.
    pub map: &'a mut [Remap],
    pub book: &'a mut FrozenBook,
}

/// What [`FrozenStore::freeze_row`] did with a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowFreeze {
    /// Rewritten into the generation.
    Frozen,
    /// Left as it was: the generation or the pool has no room for it.
    NoRoom,
}

/// **The generations, lent**: the pool, the records, the tags and the map,
/// and the book. `baud-core` decides what to freeze; this keeps the words.
pub struct FrozenStore<'a> {
    words: &'a mut [StyleWord],
    gens: &'a mut [FrozenGen],
    tags: &'a mut [Option<RowFrozen>],
    spare: &'a mut [Option<GenIdx>],
    map: &'a mut [Remap],
    book: &'a mut FrozenBook,
    /// The slots the map holds an entry for, low and high, so finishing a
    /// generation clears those and not the whole map.
    dirty: Option<(usize, usize)>,
}

impl<'a> FrozenStore<'a> {
    /// # Panics
    /// If the tags and the spare are not one a row of a ring of `ring`
    /// rows, or the map not one a slot of a table of `slots`.
    pub fn new(buf: FrozenBuf<'a>, ring: Rows, slots: usize) -> FrozenStore<'a> {
        assert!(
            buf.tags.len() >= ring.index() && buf.spare.len() >= ring.index(),
            "the frozen tags are one a row of the ring"
        );
        assert!(buf.map.len() >= slots, "the frozen map is one a slot");
        assert!(!buf.gens.is_empty(), "a pool with no generation records");
        FrozenStore {
            words: buf.words,
            gens: buf.gens,
            tags: buf.tags,
            spare: buf.spare,
            map: buf.map,
            book: buf.book,
            dirty: None,
        }
    }

    pub fn counts(&self) -> FreezeCounts {
        self.book.counts
    }

    /// Words the pool holds now.
    pub fn words_held(&self) -> usize {
        self.book.used
    }

    /// Generations that hold words now.
    pub fn gens_alive(&self) -> usize {
        self.gens[..self.book.gens]
            .iter()
            .filter(|g| g.state == GenState::Live)
            .count()
    }

    /// Whether any row or surface names a generation.
    pub fn any(&self) -> bool {
        self.book.gens > 0
    }

    // ---- the tags --------------------------------------------------------

    pub fn tag(&self, p: RowRot) -> Option<RowFrozen> {
        self.tags.get(p.index()).copied().flatten()
    }

    pub fn set_tag(&mut self, p: RowRot, t: Option<RowFrozen>) {
        if let Some(at) = self.tags.get_mut(p.index()) {
            *at = t;
        }
    }

    /// The generation physical row `p`, holding line `l` standing `at`,
    /// resolves through: its tag's, when the tag names this line where it
    /// stands and a generation with words.
    pub fn valid(&self, p: RowRot, l: LineIdx, at: FrozenAt) -> Option<GenIdx> {
        valid_in(self.tags, self.gens, p, l, at)
    }

    /// Clear every tag.
    pub fn clear_tags(&mut self) {
        self.tags.fill(None);
    }

    /// The tag of every row `keep` refuses, cleared.
    pub fn retain_tags(&mut self, mut keep: impl FnMut(RowRot, RowFrozen) -> bool) {
        for (p, t) in self.tags.iter_mut().enumerate() {
            if let Some(f) = *t {
                if !keep(RowRot(p), f) {
                    *t = None;
                }
            }
        }
    }

    /// Every tag's line moved on by one: a reverse scroll at line zero moved
    /// the rotation and not the numbering, so every row holds the line after
    /// the one it held.
    pub fn renumber_up(&mut self) {
        for t in self.tags.iter_mut().flatten() {
            t.line = t.line.next();
        }
    }

    pub fn spare(&self, p: RowRot) -> Option<GenIdx> {
        self.spare.get(p.index()).copied().flatten()
    }

    pub fn set_spare(&mut self, p: RowRot, g: Option<GenIdx>) {
        if let Some(at) = self.spare.get_mut(p.index()) {
            *at = g;
        }
    }

    pub fn clear_spare(&mut self) {
        self.spare.fill(None);
    }

    // ---- the alternate screen ------------------------------------------

    /// The generation the alternate screen's cells name while it is not up.
    pub fn alt(&self) -> Option<GenIdx> {
        self.book.alt.filter(|g| self.live(*g))
    }

    pub fn set_alt(&mut self, g: Option<GenIdx>) {
        self.book.alt = g;
    }

    // ---- what a freeze saw -----------------------------------------------

    /// A move that can leave a row off the screen without a generation and
    /// without moving the top line: a reverse scroll, a resize, a screen
    /// switch, an erase of the history, a reset.
    pub fn moved(&mut self) {
        self.book.moves = self.book.moves.wrapping_add(1);
    }

    /// Whether a freeze at top line `top` would see what the last one saw.
    pub fn unmoved(&self, top: LineIdx) -> bool {
        self.book.mark
            == Some(FreezeMark {
                top,
                moves: self.book.moves,
            })
    }

    pub fn mark(&mut self, top: LineIdx) {
        self.book.mark = Some(FreezeMark {
            top,
            moves: self.book.moves,
        });
    }

    pub fn thawing(&self) -> Thawing {
        self.book.thawing
    }

    pub fn set_thawing(&mut self, t: Thawing) {
        self.book.thawing = t;
    }

    // ---- building a generation -------------------------------------------

    fn live(&self, g: GenIdx) -> bool {
        self.gens
            .get(g.index())
            .is_some_and(|r| g.index() < self.book.gens && r.state == GenState::Live)
    }

    /// The words the pool has room for past its end.
    pub fn room(&self) -> usize {
        self.words.len() - self.book.used
    }

    /// **A new generation at the pool's end**, its id 0 the default as the
    /// table's slot 0 is, or `None` when no record or word is free.
    pub fn begin(&mut self) -> Option<GenIdx> {
        if self.book.gens >= self.gens.len() || self.room() == 0 {
            return None;
        }
        let g = GenIdx::at(self.book.gens);
        self.book.serial += 1;
        self.gens[g.index()] = FrozenGen {
            start: self.book.used,
            len: 1,
            serial: FrozenSerial(self.book.serial),
            state: GenState::Live,
            to: 0,
        };
        self.words[self.book.used] = StyleWord::DEFAULT;
        self.book.used += 1;
        self.book.gens += 1;
        self.map_set(StyleId::DEFAULT, StyleId::DEFAULT);
        Some(g)
    }

    fn map_set(&mut self, k: StyleId, v: StyleId) {
        let i = k.index();
        self.map[i] = Remap::of(v);
        self.dirty = Some(match self.dirty {
            Some((lo, hi)) => (lo.min(i), hi.max(i)),
            None => (i, i),
        });
    }

    /// **Row `row`'s cells rewritten to generation `g`'s ids**, the row a
    /// [`RowMut`] already stamped, each live
    /// slot resolved through `table` and given the generation's next id the
    /// first time it is met. Nothing is written unless the whole row fits:
    /// the generation holds at most `table.len()` words, so its ids index a
    /// thaw's map, and the pool what it was lent.
    pub fn freeze_row(
        &mut self,
        g: GenIdx,
        mut row: RowMut<'_>,
        table: &[Option<StyleWord>],
    ) -> RowFreeze {
        let rec = self.gens[g.index()];
        if rec.len + row.len() > table.len() || self.room() < row.len() {
            return RowFreeze::NoRoom;
        }
        for c in row.iter_mut() {
            let k = c.style;
            // The default is id 0 in every table, the live one's reserved
            // slot and each generation's first word ([`FrozenStore::begin`]),
            // so a blank cell stays [`PackedCell::EMPTY`] whatever the map
            // has forgotten.
            if k == StyleId::DEFAULT {
                continue;
            }
            let d = match self.map.get(k.index()).copied().and_then(Remap::get) {
                Some(d) => d,
                None => {
                    let d = StyleId(u16::try_from(self.gens[g.index()].len).unwrap_or(u16::MAX));
                    self.words[self.book.used] = table
                        .get(k.index())
                        .copied()
                        .flatten()
                        .unwrap_or(StyleWord::DEFAULT);
                    self.book.used += 1;
                    self.gens[g.index()].len += 1;
                    self.map_set(k, d);
                    d
                }
            };
            c.style = d;
        }
        self.book.counts.cells_rewritten += row.len() as u64;
        RowFreeze::Frozen
    }

    /// **Row `row`'s cells, which name ids in generation `from` or, with
    /// none, the live table `live`, rewritten to generation `g`'s**: a
    /// re-cut's line that spans two tables made one. The map is keyed by the
    /// source's ids, so a caller forgets it ([`FrozenStore::forget_map`])
    /// whenever the source changes.
    pub fn refreeze_row(
        &mut self,
        g: GenIdx,
        mut row: RowMut<'_>,
        from: Option<GenIdx>,
        live: &[Option<StyleWord>],
    ) -> RowFreeze {
        let rec = self.gens[g.index()];
        if rec.len + row.len() > live.len() || self.room() < row.len() {
            return RowFreeze::NoRoom;
        }
        let src = from.and_then(|f| self.gens.get(f.index()).copied());
        for c in row.iter_mut() {
            let k = c.style;
            // Id 0 is the default in the source and in `g`, as in
            // [`FrozenStore::freeze_row`].
            if k == StyleId::DEFAULT {
                continue;
            }
            let d = match self.map.get(k.index()).copied().and_then(Remap::get) {
                Some(d) => d,
                None => {
                    let d = StyleId(u16::try_from(self.gens[g.index()].len).unwrap_or(u16::MAX));
                    let w = match src {
                        Some(r) => match k.index() < r.len {
                            true => self.words[r.start + k.index()],
                            false => StyleWord::DEFAULT,
                        },
                        None => live
                            .get(k.index())
                            .copied()
                            .flatten()
                            .unwrap_or(StyleWord::DEFAULT),
                    };
                    self.words[self.book.used] = w;
                    self.book.used += 1;
                    self.gens[g.index()].len += 1;
                    self.map_set(k, d);
                    d
                }
            };
            c.style = d;
        }
        self.book.counts.cells_rewritten += row.len() as u64;
        RowFreeze::Frozen
    }

    /// A re-cut's line too many renditions for one generation, cut where
    /// its table changes, counted.
    pub fn counted_split(&mut self) {
        self.book.counts.lines_split += 1;
    }

    /// What the generations hold and have done.
    pub fn report(&self) -> FrozenReport {
        FrozenReport {
            counts: self.book.counts,
            words_held: self.book.used,
            gens_alive: self.gens_alive(),
        }
    }

    /// The words a row of generation `g` can still take, by the table's
    /// bound and the pool's.
    pub fn gen_room(&self, g: GenIdx, slots: usize) -> usize {
        let len = self.gens[g.index()].len;
        slots.saturating_sub(len).min(self.room())
    }

    /// Forget the map's entries, which the next generation or source table
    /// does not share.
    pub fn forget_map(&mut self) {
        if let Some((lo, hi)) = self.dirty.take() {
            self.map[lo..=hi].fill(Remap::UNSET);
        }
    }

    /// The generation finished: its map entries forgotten, the peak kept.
    pub fn finish(&mut self) {
        self.forget_map();
        self.book.counts.words_peak = self.book.counts.words_peak.max(self.book.used);
    }

    /// A row of the primary frozen, counted.
    pub fn counted_frozen(&mut self) {
        self.book.counts.rows_frozen += 1;
    }

    /// A freeze that froze rows, counted.
    pub fn counted_freeze(&mut self) {
        self.book.counts.freezes += 1;
    }

    /// A row left in the table for want of room, counted.
    pub fn counted_unfrozen(&mut self) {
        self.book.counts.rows_unfrozen += 1;
    }

    // ---- a thaw ------------------------------------------------------------

    /// Generation `g`'s word for id `id`.
    pub fn word(&self, g: GenIdx, id: StyleId) -> StyleWord {
        self.table(g).word(id)
    }

    /// The live slot a thaw gave frozen id `id` already, if it did.
    pub fn thawed(&self, id: StyleId) -> Option<StyleId> {
        self.map.get(id.index()).copied().and_then(Remap::get)
    }

    /// **The slots a thaw has interned so far**, into `live`: a sweep while
    /// it runs keeps them, as the cells it rewrote name them.
    pub fn pin(&self, live: &mut crate::SlotSet) {
        if let Some((lo, hi)) = self.dirty {
            for r in &self.map[lo..=hi] {
                if let Some(k) = r.get() {
                    live.insert(k);
                }
            }
        }
    }

    /// Frozen id `id` interned as live slot `to`.
    pub fn set_thawed(&mut self, id: StyleId, to: StyleId) {
        self.map_set(id, to);
    }

    /// A row thawed, its cells rewritten.
    pub fn counted_thaw(&mut self, cells: usize) {
        self.book.counts.rows_thawed += 1;
        self.book.counts.cells_rewritten += cells as u64;
    }

    // ---- freeing -----------------------------------------------------------

    /// **Every generation no tag and no surface names, freed**, and the
    /// pool compacted: the live generations' words slid down in order,
    /// their records with them, and every tag renumbered. A record keeps
    /// its serial, since its words are the same. Only the tags are read:
    /// run it after the tags that no longer hold are cleared.
    pub fn reclaim(&mut self) {
        let n = self.book.gens;
        for g in self.gens[..n].iter_mut() {
            g.state = GenState::Dead;
        }
        let named = |gens: &mut [FrozenGen], g: GenIdx| {
            if let Some(r) = gens.get_mut(g.index()).filter(|_| g.index() < n) {
                r.state = GenState::Live;
            }
        };
        for t in self.tags.iter().flatten() {
            named(self.gens, t.generation);
        }
        for g in self.spare.iter().flatten() {
            named(self.gens, *g);
        }
        if let Some(a) = self.book.alt {
            named(self.gens, a);
        }
        let dead = self.gens[..n]
            .iter()
            .filter(|g| g.state == GenState::Dead)
            .count();
        let held: usize = self.gens[..n]
            .iter()
            .filter(|g| g.state == GenState::Live)
            .map(|g| g.len)
            .sum();
        if dead == 0 && held == self.book.used {
            return;
        }
        self.book.counts.gens_freed += dead as u64;
        self.book.counts.compactions += 1;
        // Where each live record goes, before any moves.
        let mut to = 0;
        for g in self.gens[..n].iter_mut() {
            if g.state == GenState::Live {
                g.to = to;
                to += 1;
            }
        }
        let moved = |gens: &[FrozenGen], g: GenIdx| -> Option<GenIdx> {
            gens.get(g.index())
                .filter(|r| r.state == GenState::Live)
                .map(|r| GenIdx::at(r.to))
        };
        for t in self.tags.iter_mut() {
            if let Some(f) = t {
                match moved(self.gens, f.generation) {
                    Some(g) => f.generation = g,
                    None => *t = None,
                }
            }
        }
        for s in self.spare.iter_mut() {
            if let Some(g) = *s {
                *s = moved(self.gens, g);
            }
        }
        self.book.alt = self.book.alt.and_then(|a| moved(self.gens, a));
        // Then the words and the records, down: a record never moves up.
        let mut at = 0;
        for i in 0..n {
            let g = self.gens[i];
            if g.state != GenState::Live {
                continue;
            }
            if g.start != at {
                self.words.copy_within(g.start..g.start + g.len, at);
            }
            self.gens[g.to] = FrozenGen { start: at, ..g };
            at += g.len;
        }
        for g in self.gens[to..n].iter_mut() {
            *g = FrozenGen::EMPTY;
        }
        self.book.gens = to;
        self.book.used = at;
    }

    /// **Generation `g` rebuilt to the ids its rows still name**: `named`
    /// says which ids are, the words kept in order and slid to `at`, and
    /// the rows' cells rewritten through `renumber` by the caller. Returns
    /// the new ids, one a kept old id, through the map; the caller rewrites
    /// the cells and then calls [`FrozenStore::finish`].
    pub fn rebuild(&mut self, g: GenIdx, named: &crate::SlotSet) -> FrozenSerial {
        let rec = self.gens[g.index()];
        let mut kept = 0;
        for id in 0..rec.len {
            if id == 0 || named.has(id) {
                let w = self.words[rec.start + id];
                self.words[rec.start + kept] = w;
                self.map_set(
                    StyleId(u16::try_from(id).unwrap_or(u16::MAX)),
                    StyleId(u16::try_from(kept).unwrap_or(u16::MAX)),
                );
                kept += 1;
            }
        }
        self.book.serial += 1;
        let serial = FrozenSerial(self.book.serial);
        self.gens[g.index()] = FrozenGen {
            len: kept,
            serial,
            ..rec
        };
        self.book.counts.rebuilds += 1;
        serial
    }

    /// The id a rebuild gave old id `id`.
    pub fn rebuilt(&self, id: StyleId) -> StyleId {
        self.thawed(id).unwrap_or(StyleId::DEFAULT)
    }

    /// The generation records in use, live and dead: what
    /// [`FrozenStore::gen_at`] is asked over.
    pub fn gen_records(&self) -> usize {
        self.book.gens
    }

    /// Record `i`, when it holds words.
    pub fn gen_at(&self, i: usize) -> Option<GenIdx> {
        (i < self.book.gens && self.gens[i].state == GenState::Live).then(|| GenIdx::at(i))
    }

    /// Everything released: no generation, no tag, no surface frozen.
    pub fn release(&mut self) {
        let n = self.book.gens;
        let live = self.gens[..n]
            .iter()
            .filter(|g| g.state == GenState::Live && g.len > 0)
            .count();
        self.book.counts.gens_freed += live as u64;
        self.gens[..n].fill(FrozenGen::EMPTY);
        self.book.gens = 0;
        self.book.used = 0;
        self.book.alt = None;
        self.tags.fill(None);
        self.spare.fill(None);
        self.forget_map();
        self.moved();
    }

    // ---- reading -----------------------------------------------------------

    /// Generation `g`'s words, as a reader resolves them.
    pub fn table(&self, g: GenIdx) -> FrozenTable<'_> {
        table_of(self.gens, self.words, g)
    }

    /// The read side: the tags, the records and the words, against a
    /// window whose top line is `top`, `rows` tall, with the primary's
    /// screen `screen`.
    pub fn rows(&self, top: LineIdx, rows: Rows, screen: PrimaryScreen) -> FrozenRows<'_> {
        FrozenRows {
            tags: self.tags,
            gens: &self.gens[..self.book.gens],
            words: &self.words[..self.book.used],
            top,
            rows,
            screen,
        }
    }
}

fn valid_in(
    tags: &[Option<RowFrozen>],
    gens: &[FrozenGen],
    p: RowRot,
    l: LineIdx,
    at: FrozenAt,
) -> Option<GenIdx> {
    let t = tags.get(p.index()).copied().flatten()?;
    let live = gens
        .get(t.generation.index())
        .is_some_and(|g| g.state == GenState::Live);
    (t.line == l && t.at == at && live).then_some(t.generation)
}

fn table_of<'w>(gens: &[FrozenGen], words: &'w [StyleWord], g: GenIdx) -> FrozenTable<'w> {
    match gens.get(g.index()) {
        Some(r) if r.state == GenState::Live => FrozenTable {
            serial: r.serial,
            start: r.start,
            words: words.get(r.start..r.start + r.len).unwrap_or(&[]),
        },
        _ => FrozenTable {
            serial: FrozenSerial(0),
            start: 0,
            words: &[],
        },
    }
}

/// **A generation's words, as a reader resolves a frozen cell**: its id is
/// an index into them, and an id past them is the default.
#[derive(Clone, Copy, Debug)]
pub struct FrozenTable<'a> {
    serial: FrozenSerial,
    start: usize,
    words: &'a [StyleWord],
}

impl<'a> FrozenTable<'a> {
    /// Where its words start in the pool every generation shares, which a
    /// reader mirroring the pool resolves them at: a row's id `k` is the
    /// pool's word `start + k` (`FrozenStore::pool`).
    pub fn start(&self) -> usize {
        self.start
    }

    pub fn serial(&self) -> FrozenSerial {
        self.serial
    }

    pub fn words(&self) -> &'a [StyleWord] {
        self.words
    }

    pub fn word(&self, id: StyleId) -> StyleWord {
        self.words
            .get(id.index())
            .copied()
            .unwrap_or(StyleWord::DEFAULT)
    }
}

/// **One band's live rendition table, as a reader sees it**: its slots,
/// indexed by [`StyleId`], `None` a slot nothing was minted in, which reads
/// as [`StyleWord::DEFAULT`]. A live row's [`RowStyles`] carries its band's,
/// so a reader resolves the row's ids through the table they name and no
/// other.
#[derive(Clone, Copy, Debug)]
pub struct LiveTable<'a> {
    slots: &'a [Option<StyleWord>],
}

impl<'a> LiveTable<'a> {
    pub fn of(slots: &'a [Option<StyleWord>]) -> LiveTable<'a> {
        LiveTable { slots }
    }

    /// The rendition slot `id` holds; the default where it holds none.
    pub fn word(&self, id: StyleId) -> StyleWord {
        self.slots
            .get(id.index())
            .copied()
            .flatten()
            .unwrap_or(StyleWord::DEFAULT)
    }

    /// The rendition a live cell of this band stands for.
    pub fn word_of(&self, cell: crate::PackedCell) -> StyleWord {
        self.word(cell.style)
    }

    /// The slots, for a reader that resolves every one once.
    pub fn slots(&self) -> &'a [Option<StyleWord>] {
        self.slots
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

/// **Which table a row's cells name ids in**: its band's live rendition
/// table, or a generation's words.
#[derive(Clone, Copy, Debug)]
pub enum RowStyles<'a> {
    Live(LiveTable<'a>),
    Frozen(FrozenTable<'a>),
}

impl RowStyles<'_> {
    /// **Where this row's ids start in a resolved table that mirrors the
    /// rendition table's `live` slots and then the pool**: zero on a live
    /// row, `live` and its generation's start on a frozen one. So a reader
    /// holding such a table resolves cell `c` as `table[base + c.style]`,
    /// one add a row.
    pub fn base(&self, live: usize) -> usize {
        match self {
            RowStyles::Live(_) => 0,
            RowStyles::Frozen(t) => live + t.start,
        }
    }

    /// The generation's serial, or `None` on a live row.
    pub fn serial(&self) -> Option<FrozenSerial> {
        match self {
            RowStyles::Live(_) => None,
            RowStyles::Frozen(t) => Some(t.serial),
        }
    }

    /// The rendition id `id` names in this row.
    pub fn word(&self, id: StyleId) -> StyleWord {
        match self {
            RowStyles::Live(t) => t.word(id),
            RowStyles::Frozen(t) => t.word(id),
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self, RowStyles::Live(_))
    }
}

/// Whether the primary's screen is the one up, so its rows always resolve
/// live, or parked while the alternate screen is up, when they may not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimaryScreen {
    Up,
    Parked,
}

/// **The generations as a reader of the primary sees them**: which table
/// physical row `p`, holding line `l`, resolves through.
#[derive(Clone, Copy)]
pub struct FrozenRows<'a> {
    tags: &'a [Option<RowFrozen>],
    gens: &'a [FrozenGen],
    words: &'a [StyleWord],
    top: LineIdx,
    rows: Rows,
    screen: PrimaryScreen,
}

impl<'a> FrozenRows<'a> {
    /// The table physical row `p`, holding line `l`, resolves through:
    /// its generation's words, or `live` where it is not frozen.
    pub fn styles(&self, p: RowRot, l: LineIdx, live: LiveTable<'a>) -> RowStyles<'a> {
        let at = FrozenAt::of(l, self.top, self.rows);
        if at == FrozenAt::Screen && self.screen == PrimaryScreen::Up {
            return RowStyles::Live(live);
        }
        match valid_in(self.tags, self.gens, p, l, at) {
            Some(g) => RowStyles::Frozen(table_of(self.gens, self.words, g)),
            None => RowStyles::Live(live),
        }
    }

    /// Whether any row may be frozen: a reader that finds none resolves
    /// every row live without asking.
    pub fn any(&self) -> bool {
        !self.gens.is_empty()
    }

    /// The words of every generation, one after another: what a reader
    /// that resolves a generation once, by its serial and where it starts,
    /// mirrors.
    pub fn pool(&self) -> &'a [StyleWord] {
        self.words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_stands_where_the_window_puts_it() {
        let top = LineIdx::of(10);
        let rows = Rows::of_count(3);
        assert_eq!(FrozenAt::of(LineIdx::of(9), top, rows), FrozenAt::History);
        assert_eq!(FrozenAt::of(LineIdx::of(10), top, rows), FrozenAt::Screen);
        assert_eq!(FrozenAt::of(LineIdx::of(12), top, rows), FrozenAt::Screen);
        assert_eq!(FrozenAt::of(LineIdx::of(13), top, rows), FrozenAt::Hidden);
    }

    #[test]
    fn a_zeroed_map_holds_nothing() {
        assert_eq!(Remap::default().get(), None);
        assert_eq!(Remap::of(StyleId(0)).get(), Some(StyleId(0)));
        assert_eq!(Remap::of(StyleId(7)).get(), Some(StyleId(7)));
    }
}
