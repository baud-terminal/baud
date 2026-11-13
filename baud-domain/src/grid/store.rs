//! **One store owns every surface's cells and marks, and every store to them
//! stamps its row.**
//!
//! A renderer keeps a copy of the screen and, each frame, wants only the rows
//! that changed. Comparing every cell of the copy with the screen finds them
//! and costs a read of both, every frame, whether anything was written or
//! not. The core knows which rows it wrote: every store to a cell or a mark
//! goes through here, and each one records the current [`WriteGen`] in its
//! row's [`RowGen`]; the surface's last write takes it when the borrow that
//! wrote ends ([`Stamped`]'s `Drop`). A reader keeps the [`Seal`] its copy
//! was built under and asks [`RowGens::after`] per row, or
//! [`SurfaceStore::wrote_since`] once.
//!
//! That is sound only if no store skips the stamp, so nothing else can reach
//! a surface's memory: the cells, marks and generations are private here, and
//! the only mutable ways in are tokens minted here, after the stamp --
//! [`Stamped`] for a surface, [`RowMut`] for a row's cells, [`MarkMut`] for
//! its mark. `baud-core/tests/one_store.rs` holds both crates' source to it.
//!
//! **The stamp is unconditional.** It never asks whether the cell or the mark
//! changed: a stamp too many is a row redrawn, one too few is a row left
//! stale, and only the first is safe.

use crate::*;
use core::ops::{Deref, DerefMut, Range};
use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(not(target_has_atomic = "64"))]
compile_error!(
    "SealEpoch draws from a process-wide 64-bit counter; a target without 64-bit atomics would have to share epochs between terminals"
);

/// **The core's current write generation.** Every store records it; only a
/// seal ([`SurfaceStore::seal`]) moves it. Starts at 1, with every row's
/// generation and every surface's last write at 0, so a fresh store has
/// written nothing after any seal it can give.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WriteGen(u32);

impl WriteGen {
    /// The generation a fresh store writes under.
    pub const FIRST: WriteGen = WriteGen(1);
    /// The last generation before a seal wraps ([`SurfaceStore::seal`]).
    pub const LAST: WriteGen = WriteGen(u32::MAX);

    /// `n` generations before the wrap: what the probe that reaches it
    /// starts from (`write-gen-probe`).
    pub const fn before_wrap(n: u32) -> WriteGen {
        WriteGen(u32::MAX - n)
    }
}

/// **A row's last stamp**, opaque. No comparison, no field and no method: a
/// generation means something only beside the epoch it was stamped in, so
/// the only question anything outside this file can ask of one is
/// [`RowGens::after`] or [`StatusGen::after`], which read the epoch first.
///
/// ```compile_fail,E0369
/// use baud_domain::RowGen;
/// let _ = RowGen::ZERO < RowGen::ZERO;
/// ```
/// ```compile_fail,E0369
/// use baud_domain::RowGen;
/// let _ = RowGen::ZERO == RowGen::ZERO;
/// ```
/// ```compile_fail,E0616
/// use baud_domain::RowGen;
/// let _ = RowGen::ZERO.0;
/// ```
/// ```
/// use baud_domain::RowGen;
/// let _ = RowGen::ZERO;
/// ```
#[derive(Clone, Copy, Debug)]
pub struct RowGen(u32);

impl RowGen {
    /// What a caller allocating a surface's generations fills them with.
    pub const ZERO: RowGen = RowGen(0);
}

/// **Which run of generations a seal belongs to**: drawn fresh for every
/// store ([`SurfaceStore::new`]) and at every wrap, from one process-wide
/// counter, as `ScreenGen::fresh` in `baud-render` draws a screen's. So a
/// seal from another terminal, or from before a wrap, never names this
/// store's current run, and every row reads as written against it.
///
/// A wrap draws a fresh value rather than adding one: adding would land on
/// the next store's first epoch. The counter is a seed read once a store
/// and once a wrap, never per byte; the epoch itself is a field behind
/// `&mut self`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealEpoch(u64);

impl SealEpoch {
    /// An epoch no store has carried yet.
    pub fn fresh() -> SealEpoch {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        SealEpoch(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// **What a reader keeps beside its copy**: the generation the copy was
/// built under, and the epoch. Built only here ([`SurfaceStore::seal`],
/// [`SurfaceStore::sealed`]); no comparison is public.
///
/// ```compile_fail,E0451
/// use baud_domain::{Seal, WriteGen, SealEpoch};
/// let _ = Seal { closed: WriteGen::FIRST, epoch: SealEpoch::fresh() };
/// ```
/// ```compile_fail,E0369
/// fn f(a: baud_domain::Seal, b: baud_domain::Seal) -> bool { a == b }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Seal {
    closed: WriteGen,
    epoch: SealEpoch,
}

/// One of the three surfaces: the primary's ring, the alternate screen, or
/// the status line's row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Which {
    Primary,
    Alternate,
    Status,
}

/// Whether a stored row was written after a seal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowDirt {
    Clean,
    Written,
}

/// **A window row's physical row on its surface.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowAt {
    pub screen: Which,
    pub p: RowRot,
}

/// **Which physical rows a window shows**: its surface, the physical row
/// drawn at its top, the ring those rows wrap in, and how many. A window's
/// rows are consecutive physical rows of one ring, so this names every one,
/// and two windows are the same rows iff these are equal. `top` is the row
/// *drawn*, with any look back into the history already clamped to what the
/// history holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowAt {
    pub screen: Which,
    pub top: RowRot,
    pub ring: Rows,
    pub rows: Rows,
}

impl WindowAt {
    /// Window row `y`'s physical row.
    pub fn at(&self, y: Row) -> RowAt {
        RowAt {
            screen: self.screen,
            p: self.top.advance(y.index(), self.ring),
        }
    }
}

/// **A window's rows' generations, read in place**, with the store's current
/// epoch. The one per-row question is [`RowGens::after`]; nothing yields a
/// bare [`RowGen`] to compare.
///
/// ```compile_fail,E0277
/// fn f(g: baud_domain::RowGens<'_>) { for _ in g {} }
/// ```
/// ```
/// fn f(g: baud_domain::RowGens<'_>, s: &baud_domain::Seal) -> bool { g.after(baud_domain::Row::ZERO, s) }
/// ```
pub struct RowGens<'t> {
    at: WindowAt,
    epoch: SealEpoch,
    gens: &'t [RowGen],
}

impl RowGens<'_> {
    /// The window these are the rows of.
    pub fn window(&self) -> WindowAt {
        self.at
    }

    /// **Whether window row `y` was written after `kept`.** The epoch first:
    /// a seal from another store, or from before a wrap, reads every row as
    /// written. A row past the window reads as written, the safe answer.
    pub fn after(&self, y: Row, kept: &Seal) -> bool {
        if kept.epoch != self.epoch || y.index() >= self.at.rows.index() {
            return true;
        }
        let p = self.at.at(y).p.index();
        self.gens.get(p).is_none_or(|g| g.0 > kept.closed.0)
    }
}

/// **The status line's generation**, with the store's current epoch.
#[derive(Clone, Copy, Debug)]
pub struct StatusGen {
    row: RowGen,
    epoch: SealEpoch,
}

impl StatusGen {
    /// Whether the status line was written after `kept`, the epoch first.
    pub fn after(&self, kept: &Seal) -> bool {
        kept.epoch != self.epoch || self.row.0 > kept.closed.0
    }
}

/// One surface's memory and its last write.
struct Plane<'b> {
    cells: &'b mut [PackedCell],
    used: &'b mut [RowExtent],
    last: WriteGen,
}

/// **Every surface's cells, marks and generations, and the core's clock.**
/// Held by `baud-core`'s `Storage`, built once, fields private: the way in
/// to write is [`SurfaceStore::split`] or [`SurfaceStore::stamped`], each a
/// [`Stamped`].
///
/// ```compile_fail,E0616
/// fn f(s: &mut baud_domain::SurfaceStore<'_>) { let _ = &mut s.prim; }
/// ```
pub struct SurfaceStore<'b> {
    prim: Plane<'b>,
    alt: Plane<'b>,
    status: Plane<'b>,
    prim_gens: &'b mut [RowGen],
    alt_gens: &'b mut [RowGen],
    /// The status line has at most one row, so its generation is held here
    /// by value and its lender passes none.
    status_gens: [RowGen; 1],
    now: WriteGen,
    epoch: SealEpoch,
    /// The pitch every surface's rows are laid out at, narrow so a
    /// [`Stamped`] carries it beside `now` in one word.
    pitch: u32,
}

impl<'b> SurfaceStore<'b> {
    /// The primary's ring and the alternate screen, each with a generation
    /// a row; no status line until [`SurfaceStore::lend_status`].
    ///
    /// # Panics
    /// If a surface has fewer generations than rows, or the pitch does not
    /// fit a `u32`.
    pub fn new(
        stride: Stride,
        prim: (&'b mut [PackedCell], &'b mut [RowExtent], &'b mut [RowGen]),
        alt: (&'b mut [PackedCell], &'b mut [RowExtent], &'b mut [RowGen]),
    ) -> SurfaceStore<'b> {
        let (pc, pu, pg) = prim;
        let (ac, au, ag) = alt;
        assert!(
            pg.len() >= pu.len() && ag.len() >= au.len(),
            "a surface has fewer generations than rows"
        );
        let (np, na) = (pu.len(), au.len());
        SurfaceStore {
            prim: Plane {
                cells: pc,
                used: pu,
                last: WriteGen(0),
            },
            alt: Plane {
                cells: ac,
                used: au,
                last: WriteGen(0),
            },
            status: Plane {
                cells: &mut [],
                used: &mut [],
                last: WriteGen(0),
            },
            prim_gens: &mut pg[..np],
            alt_gens: &mut ag[..na],
            status_gens: [RowGen::ZERO],
            now: WriteGen::FIRST,
            epoch: SealEpoch::fresh(),
            pitch: u32::try_from(stride.index()).expect("a pitch past u32"),
        }
    }

    /// The status line's row, lent: its cells at least the pitch and its
    /// extent. One row at most is a surface's: an extent past the first is
    /// not read.
    pub fn lend_status(&mut self, cells: &'b mut [PackedCell], used: &'b mut [RowExtent]) {
        let n = used.len().min(1);
        self.status = Plane {
            cells,
            used: &mut used[..n],
            last: WriteGen(0),
        };
    }

    /// **The three surfaces at once**: the one `up` names, the other screen,
    /// and the status line, as three disjoint borrows. What a read takes.
    ///
    /// # Panics
    /// If `up` is [`Which::Status`]: the status line is never the screen up.
    pub fn split(&mut self, up: Which) -> (Stamped<'_>, Stamped<'_>, Stamped<'_>) {
        let (now, pitch) = (self.now, self.pitch);
        let prim = Stamped::of(&mut self.prim, &mut *self.prim_gens, now, pitch);
        let alt = Stamped::of(&mut self.alt, &mut *self.alt_gens, now, pitch);
        let n = self.status.used.len();
        let status = Stamped::of(&mut self.status, &mut self.status_gens[..n], now, pitch);
        match up {
            Which::Primary => (prim, alt, status),
            Which::Alternate => (alt, prim, status),
            Which::Status => panic!("the status line is never the screen up"),
        }
    }

    /// One surface, to write.
    pub fn stamped(&mut self, w: Which) -> Stamped<'_> {
        let (now, pitch) = (self.now, self.pitch);
        match w {
            Which::Primary => Stamped::of(&mut self.prim, &mut *self.prim_gens, now, pitch),
            Which::Alternate => Stamped::of(&mut self.alt, &mut *self.alt_gens, now, pitch),
            Which::Status => {
                let n = self.status.used.len();
                Stamped::of(&mut self.status, &mut self.status_gens[..n], now, pitch)
            }
        }
    }

    fn plane(&self, w: Which) -> &Plane<'b> {
        match w {
            Which::Primary => &self.prim,
            Which::Alternate => &self.alt,
            Which::Status => &self.status,
        }
    }

    fn gens(&self, w: Which) -> &[RowGen] {
        match w {
            Which::Primary => self.prim_gens,
            Which::Alternate => self.alt_gens,
            Which::Status => &self.status_gens[..self.status.used.len()],
        }
    }

    /// A surface's cells, every row at the pitch.
    pub fn cells(&self, w: Which) -> &[PackedCell] {
        self.plane(w).cells
    }

    /// A surface's marks, one a physical row.
    pub fn used(&self, w: Which) -> &[RowExtent] {
        self.plane(w).used
    }

    /// A surface's physical rows: zero for a status line not lent.
    pub fn rows(&self, w: Which) -> Rows {
        Rows::of_index(self.plane(w).used.len())
    }

    /// The pitch.
    pub fn stride(&self) -> Stride {
        Stride::of_cells(self.pitch as usize)
    }

    /// The generation stores are stamped with now.
    pub fn now(&self) -> WriteGen {
        self.now
    }

    /// The current epoch.
    pub fn epoch(&self) -> SealEpoch {
        self.epoch
    }

    /// The generation of a surface's last write.
    pub fn last(&self, w: Which) -> WriteGen {
        self.plane(w).last
    }

    /// **Close the current generation**: everything written so far carries
    /// a generation at or below the seal's, and everything written after it
    /// a greater one. Only the store can seal, because it owns every gen; a
    /// [`Stamped`] borrows the store, so none is live across a seal, and
    /// neither is a [`RowMut`] or a [`MarkMut`] minted from one.
    ///
    /// **At the last generation it wraps**: every row of every surface goes
    /// to generation 0 and every last write to 0, `now` to 1, and the epoch
    /// is drawn fresh. A reader keeping any older seal then reads every row
    /// as written, by the epoch; one keeping this seal reads them clean, and
    /// they were written before it. O(ring), once in 2^32 seals.
    pub fn seal(&mut self) -> Seal {
        if self.now.0 == u32::MAX {
            self.prim_gens.fill(RowGen::ZERO);
            self.alt_gens.fill(RowGen::ZERO);
            self.status_gens = [RowGen::ZERO];
            self.prim.last = WriteGen(0);
            self.alt.last = WriteGen(0);
            self.status.last = WriteGen(0);
            self.now = WriteGen::FIRST;
            self.epoch = SealEpoch::fresh();
            return Seal {
                closed: WriteGen(0),
                epoch: self.epoch,
            };
        }
        let s = Seal {
            closed: self.now,
            epoch: self.epoch,
        };
        self.now = WriteGen(self.now.0 + 1);
        s
    }

    /// **The seal a copy read now is built under**: the generation before
    /// `now`. A store after the read carries `now` or later and so reads as
    /// after it; one between the last seal and the read carries `now` too,
    /// and reads as after it again next frame -- a row redrawn, the safe
    /// direction.
    pub fn sealed(&self) -> Seal {
        Seal {
            closed: WriteGen(self.now.0 - 1),
            epoch: self.epoch,
        }
    }

    /// Whether any surface was written after `kept`: the epoch, then the
    /// three last writes. A surface's last write is when a [`Stamped`] of it
    /// was last dropped, so a borrow that wrote nothing reads as a write --
    /// the safe direction, and a reader then asks each row's generation.
    pub fn wrote_since(&self, kept: &Seal) -> bool {
        kept.epoch != self.epoch
            || [&self.prim, &self.alt, &self.status]
                .iter()
                .any(|p| p.last.0 > kept.closed.0)
    }

    /// The window `at`'s rows' generations.
    pub fn row_gens(&self, at: WindowAt) -> RowGens<'_> {
        RowGens {
            at,
            epoch: self.epoch,
            gens: self.gens(at.screen),
        }
    }

    /// The status line's generation, when a row is lent.
    pub fn status_gen(&self) -> Option<StatusGen> {
        match self.status.used.is_empty() {
            true => None,
            false => Some(StatusGen {
                row: self.status_gens[0],
                epoch: self.epoch,
            }),
        }
    }

    /// **Every physical row of a surface**, live or not, with its cells at
    /// the pitch, its mark, and whether it was written after `kept`.
    pub fn stored_rows(
        &self,
        w: Which,
        kept: &Seal,
    ) -> impl Iterator<Item = (RowAt, &[PackedCell], RowExtent, RowDirt)> + '_ {
        let (plane, gens, s) = (self.plane(w), self.gens(w), self.pitch as usize);
        let foreign = kept.epoch != self.epoch;
        let closed = kept.closed.0;
        (0..plane.used.len()).map(move |p| {
            let dirt = match foreign || gens[p].0 > closed {
                true => RowDirt::Written,
                false => RowDirt::Clean,
            };
            (
                RowAt {
                    screen: w,
                    p: RowRot(p),
                },
                &plane.cells[p * s..(p + 1) * s],
                plane.used[p],
                dirt,
            )
        })
    }

    /// **Set the generation stores are stamped with**, and nothing else:
    /// how a test reaches the wrap without 2^32 seals. Never in a build that
    /// ships.
    #[cfg(feature = "write-gen-probe")]
    pub fn start_writes_at(&mut self, at: WriteGen) {
        self.now = at;
    }
}

/// **One surface, borrowed to write.** Every method that hands out a way to
/// store stamps the rows it hands out before it returns: the row's
/// generation takes `now`. The surface's last write takes `now` once, when
/// this is dropped, whether or not anything was written. Reads are free.
/// Minted only by [`SurfaceStore`], so it never outlives a seal.
///
/// ```compile_fail,E0616
/// fn f(s: &mut baud_domain::Stamped<'_>) { let _ = &mut s.cells; }
/// ```
pub struct Stamped<'a> {
    cells: &'a mut [PackedCell],
    used: &'a mut [RowExtent],
    gens: &'a mut [RowGen],
    last: &'a mut WriteGen,
    /// By value: only a seal moves it, and a seal cannot run while this
    /// borrows the store.
    now: WriteGen,
    pitch: u32,
    /// **The physical row this last stamped**, or [`NONE_STAMPED`]: a
    /// store to the same row again skips the generation's store for one
    /// compare. Sound because a row stamped through this borrow holds `now`
    /// until a seal, no seal runs while this borrows the store, and the
    /// generations are per physical row of this surface alone. Reset at
    /// every mint, and carried with the surface: [`View::swap_surface`]
    /// swaps the whole `Stamped`, so a row cached on one screen is never
    /// read as stamped on the other.
    ///
    /// The CJK path writes one character a call, most of a row's
    /// characters one after another on that row, and each call took the
    /// generation's bounds check, its loads and its store.
    stamped: usize,
}

/// What [`Stamped::stamped`] holds before the first stamp: no physical row.
const NONE_STAMPED: usize = usize::MAX;

/// **The surface's last write, at the end of the borrow.** Every write
/// through a [`Stamped`] happens while it lives, `now` cannot move while it
/// lives, so one store here covers every write the borrow made, and the
/// per-byte writers carry no store of it. It is unconditional: a borrow
/// that wrote nothing reports a write, which costs a reader a compare of
/// each row's generation and never a copy, where a flag every writer had to
/// set would be one more store on each of them, and one writer that forgot
/// it a row left stale. Nothing forgets a `Stamped` or what holds one
/// (`baud-core/tests/one_store.rs`), so this always runs.
impl Drop for Stamped<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        *self.last = self.now;
    }
}

impl<'a> Stamped<'a> {
    fn of(
        plane: &'a mut Plane<'_>,
        gens: &'a mut [RowGen],
        now: WriteGen,
        pitch: u32,
    ) -> Stamped<'a> {
        Stamped {
            cells: &mut *plane.cells,
            used: &mut *plane.used,
            gens,
            last: &mut plane.last,
            now,
            pitch,
            stamped: NONE_STAMPED,
        }
    }

    /// **The stamp**, and the only one: row `p`'s generation takes `now`.
    /// Unconditional on what the store changes; skipped only for the row
    /// this borrow stamped last, which holds `now` already
    /// ([`Stamped::stamped`]). The surface's last write is the `Drop`'s.
    ///
    /// A test and a cold block: [`Stamped::stamp_new`] is inlined behind
    /// `core::hint::cold_path`, so a store to the row stamped last carries
    /// one load, one compare and one branch, and no store, in every writer.
    #[inline(always)]
    fn stamp(&mut self, p: usize) {
        if p != self.stamped {
            core::hint::cold_path();
            self.stamp_new(p);
        }
    }

    /// **Row `p` stamped, a row this borrow has not stamped last**: the only
    /// store to a row's generation but [`Stamped::stamp_all`]'s, and the only
    /// writer of [`Stamped::stamped`] after the mint, which it sets after the
    /// store. It stores no last write: that has one writer, the `Drop`.
    ///
    /// Inline in a block the hint lays out cold, not a call: out of line it
    /// saved four more callee-saved register pairs in each run writer, hit
    /// or miss, and a miss cost 6 to 20 instructions more a call
    /// (put_glyph_fresh 52 against 45 to the mark, put_ascii_run 113 to 93).
    #[inline(always)]
    fn stamp_new(&mut self, p: usize) {
        self.gens[p] = RowGen(self.now.0);
        self.stamped = p;
    }

    #[inline(always)]
    fn s(&self) -> usize {
        self.pitch as usize
    }

    // ---- reads ----

    /// Every cell, every row at the pitch.
    #[inline(always)]
    pub fn cells(&self) -> &[PackedCell] {
        self.cells
    }

    /// Every mark, one a physical row.
    #[inline(always)]
    pub fn used(&self) -> &[RowExtent] {
        self.used
    }

    /// Physical rows.
    #[inline(always)]
    pub fn rows(&self) -> usize {
        self.used.len()
    }

    /// The pitch.
    #[inline(always)]
    pub fn stride(&self) -> Stride {
        Stride::of_cells(self.s())
    }

    /// Row `p` at the pitch.
    #[inline(always)]
    pub fn row(&self, p: usize) -> &[PackedCell] {
        let s = self.s();
        &self.cells[p * s..(p + 1) * s]
    }

    /// Row `p`'s mark.
    #[inline(always)]
    pub fn mark_at(&self, p: usize) -> RowExtent {
        self.used[p]
    }

    // ---- writes, each stamping what it hands out ----

    /// **Row `p`, stamped, to read and store a cell at a time**, and its mark:
    /// what the one-character writers take, once a call ([`RowWrite`]). The
    /// row's base is computed here, once; each cell is one compare against
    /// the surface's cells, and no row is cut.
    #[inline(always)]
    pub fn row_at(&mut self, p: usize) -> RowWrite<'_> {
        self.stamp(p);
        let pitch = self.s();
        RowWrite {
            cells: &mut *self.cells,
            mark: &mut self.used[p],
            base: p * pitch,
            pitch,
        }
    }

    /// **Row `p` at the pitch and its mark, one stamp**: what every
    /// per-byte writer takes, once a call.
    #[inline(always)]
    pub fn row_and_mark(&mut self, p: usize) -> (RowMut<'_>, MarkMut<'_>) {
        self.stamp(p);
        let s = self.s();
        (
            RowMut(&mut self.cells[p * s..(p + 1) * s]),
            MarkMut(&mut self.used[p]),
        )
    }

    /// Row `p`'s cells at the pitch.
    #[inline(always)]
    pub fn row_mut(&mut self, p: usize) -> RowMut<'_> {
        self.stamp(p);
        let s = self.s();
        RowMut(&mut self.cells[p * s..(p + 1) * s])
    }

    /// Row `p`'s cells, when the surface has that row: the status line's
    /// callers, whose row may not be lent.
    pub fn get_row_mut(&mut self, p: usize) -> Option<RowMut<'_>> {
        let s = self.s();
        if p >= self.used.len() || self.cells.len() < (p + 1) * s {
            return None;
        }
        Some(self.row_mut(p))
    }

    /// Columns `a..b` of row `p`.
    #[inline(always)]
    pub fn span_mut(&mut self, p: usize, at: Range<usize>) -> RowMut<'_> {
        self.stamp(p);
        let base = p * self.s();
        RowMut(&mut self.cells[base + at.start..base + at.end])
    }

    /// Columns `a..b` of row `p`, when the surface has that row.
    pub fn get_span_mut(&mut self, p: usize, at: Range<usize>) -> Option<RowMut<'_>> {
        if p >= self.used.len() || at.end > self.s() || self.cells.len() < (p + 1) * self.s() {
            return None;
        }
        Some(self.span_mut(p, at))
    }

    /// Column `x` of row `p`.
    #[inline(always)]
    pub fn cell_mut(&mut self, p: usize, x: usize) -> &mut PackedCell {
        self.stamp(p);
        let i = p * self.s() + x;
        &mut self.cells[i]
    }

    /// Row `p`'s mark.
    #[inline(always)]
    pub fn mark_mut(&mut self, p: usize) -> MarkMut<'_> {
        self.stamp(p);
        MarkMut(&mut self.used[p])
    }

    /// Row `p`'s mark set to `e`.
    #[inline(always)]
    pub fn mark(&mut self, p: usize, e: RowExtent) {
        self.stamp(p);
        self.used[p] = e;
    }

    /// Row `p`'s mark through `f`.
    #[inline(always)]
    pub fn mark_with(&mut self, p: usize, f: impl FnOnce(RowExtent) -> RowExtent) {
        self.stamp(p);
        self.used[p] = f(self.used[p]);
    }

    /// Columns `at` of row `src` copied to the same columns of row `dst`,
    /// two different rows. Stamps `dst`.
    pub fn copy_row(&mut self, src: usize, dst: usize, at: Range<usize>) {
        assert_ne!(src, dst, "copy_row between two different rows");
        self.stamp(dst);
        let s = self.s();
        let (lo, hi) = (src.min(dst), src.max(dst));
        let (a, b) = self.cells.split_at_mut(hi * s);
        let (low, high) = (&mut a[lo * s..(lo + 1) * s], &mut b[..s]);
        match src < dst {
            true => high[at.clone()].copy_from_slice(&low[at]),
            false => low[at.clone()].copy_from_slice(&high[at]),
        }
    }

    /// **Row `src` to read and row `dst` to write, one stamp**: `src`'s cells
    /// and mark, and `dst`'s cells and mark, two different rows. What a row
    /// copy that reads the source while it writes the destination takes, so
    /// the region repair's per-scroll copy stamps once.
    #[inline(always)]
    pub fn from_into(
        &mut self,
        src: usize,
        dst: usize,
    ) -> (&[PackedCell], RowExtent, RowMut<'_>, MarkMut<'_>) {
        assert_ne!(src, dst, "from_into between two different rows");
        self.stamp(dst);
        let s = self.s();
        let from = self.used[src];
        let (lo, hi) = (src.min(dst), src.max(dst));
        let (a, b) = self.cells.split_at_mut(hi * s);
        let (low, high) = (&mut a[lo * s..(lo + 1) * s], &mut b[..s]);
        let (read, write) = match src < dst {
            true => (&*low, high),
            false => (&*high, low),
        };
        (read, from, RowMut(write), MarkMut(&mut self.used[dst]))
    }

    /// Columns `at` of row `src` moved to columns starting at `to` of row
    /// `dst`, rows that may be the same. Stamps `dst`.
    pub fn copy_within_rows(&mut self, src: usize, at: Range<usize>, dst: usize, to: usize) {
        self.stamp(dst);
        let s = self.s();
        self.cells
            .copy_within(src * s + at.start..src * s + at.end, dst * s + to);
    }

    /// Row `src`'s mark copied to row `dst`. Stamps `dst`.
    #[inline(always)]
    pub fn copy_mark(&mut self, src: usize, dst: usize) {
        self.stamp(dst);
        self.used[dst] = self.used[src];
    }

    /// **Every row, one token each for the cells and the marks**: a full
    /// reset, a ring erased, a whole surface frozen. Stamps every row,
    /// O(rows), once.
    pub fn all_mut(&mut self) -> (RowMut<'_>, MarksMut<'_>) {
        self.stamp_all();
        (RowMut(&mut *self.cells), MarksMut(&mut *self.used))
    }

    /// Each row of the physical range `rows`, in turn, as its cells and its
    /// mark, each stamped.
    pub fn rows_mut(
        &mut self,
        rows: Range<usize>,
        mut f: impl FnMut(usize, RowMut<'_>, MarkMut<'_>),
    ) {
        for p in rows {
            let (r, m) = self.row_and_mark(p);
            f(p, r, m);
        }
    }

    /// **Every row stamped, nothing written**: what a resize ends with, since
    /// it moves rows by more paths than are worth naming one by one.
    pub fn stamp_all(&mut self) {
        let now = RowGen(self.now.0);
        self.gens.fill(now);
    }
}

/// **A row's cells, stamped when it was minted.** A writer may hold it and
/// store through it as often as it likes: the row's generation was set to
/// `now` before this was handed out, and `now` cannot move while it lives --
/// it borrows a [`Stamped`], which borrows the store (through `Paint` or
/// `&mut Term`), and only the store seals. So it cannot outlive a seal.
/// Built only in this file.
///
/// ```compile_fail,E0423
/// fn f(c: &mut [baud_domain::PackedCell]) -> baud_domain::RowMut<'_> { baud_domain::RowMut(c) }
/// ```
/// ```compile_fail,E0499
/// use baud_domain::*;
/// let (mut c, mut u, mut g) = ([PackedCell::EMPTY; 4], [RowExtent::ZERO; 1], [RowGen::ZERO; 1]);
/// let (mut ac, mut au, mut ag) = ([PackedCell::EMPTY; 4], [RowExtent::ZERO; 1], [RowGen::ZERO; 1]);
/// let mut store = SurfaceStore::new(Stride::of_cells(4), (&mut c, &mut u, &mut g), (&mut ac, &mut au, &mut ag));
/// let mut prim = store.stamped(Which::Primary);
/// let mut row = prim.row_mut(0);
/// let _ = store.seal();
/// row[0] = PackedCell::EMPTY;
/// ```
/// ```
/// use baud_domain::*;
/// let (mut c, mut u, mut g) = ([PackedCell::EMPTY; 4], [RowExtent::ZERO; 1], [RowGen::ZERO; 1]);
/// let (mut ac, mut au, mut ag) = ([PackedCell::EMPTY; 4], [RowExtent::ZERO; 1], [RowGen::ZERO; 1]);
/// let mut store = SurfaceStore::new(Stride::of_cells(4), (&mut c, &mut u, &mut g), (&mut ac, &mut au, &mut ag));
/// let kept = store.seal();
/// {
///     let mut prim = store.stamped(Which::Primary);
///     let mut row = prim.row_mut(0);
///     row[0] = PackedCell::EMPTY;
/// }
/// let _ = store.seal();
/// assert!(store.wrote_since(&kept));
/// ```
pub struct RowMut<'a>(&'a mut [PackedCell]);

impl Deref for RowMut<'_> {
    type Target = [PackedCell];
    #[inline(always)]
    fn deref(&self) -> &[PackedCell] {
        self.0
    }
}

impl DerefMut for RowMut<'_> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [PackedCell] {
        self.0
    }
}

/// **One row of a surface, stamped when it was minted, a cell at a time.**
/// What a writer storing one character takes ([`Stamped::row_at`]): the
/// row's generation was set to `now` before this was handed out, and `now`
/// cannot move while it lives, as for [`RowMut`]. It cuts no row: a cell's
/// index is the row's base plus its column, checked once against the
/// surface's cells, where a row cut per call cost a multiply, two range
/// checks and a check against the pitch.
///
/// **It holds the surface's cells and the row's mark, not the `Stamped`.**
/// Through a `&mut Stamped` each store had to load the cells' pointer and
/// length again after the one before it, which might have written them;
/// held here by value, both stay in registers for the writer's every store.
///
/// **A column stays inside the row by the view's geometry**: every writer
/// tests `x < cols` first, and `View::new` and `View::set_geometry` assert
/// `cols <= pitch`, so `x < pitch` and the store lands in row `p`. Built
/// only in this file.
///
/// ```compile_fail,E0499
/// use baud_domain::*;
/// let (mut c, mut u, mut g) = ([PackedCell::EMPTY; 8], [RowExtent::ZERO; 2], [RowGen::ZERO; 2]);
/// let (mut ac, mut au, mut ag) = ([PackedCell::EMPTY; 8], [RowExtent::ZERO; 2], [RowGen::ZERO; 2]);
/// let mut store = SurfaceStore::new(Stride::of_cells(4), (&mut c, &mut u, &mut g), (&mut ac, &mut au, &mut ag));
/// let mut prim = store.stamped(Which::Primary);
/// let mut row = prim.row_at(0);
/// let _ = prim.row_mut(1);
/// row.set(0, PackedCell::EMPTY);
/// ```
/// ```compile_fail,E0451
/// use baud_domain::*;
/// fn f<'s>(cells: &'s mut [PackedCell], mark: &'s mut RowExtent) -> RowWrite<'s> {
///     RowWrite { cells, mark, base: 0, pitch: 4 }
/// }
/// ```
/// ```
/// use baud_domain::*;
/// let (mut c, mut u, mut g) = ([PackedCell::EMPTY; 8], [RowExtent::ZERO; 2], [RowGen::ZERO; 2]);
/// let (mut ac, mut au, mut ag) = ([PackedCell::EMPTY; 8], [RowExtent::ZERO; 2], [RowGen::ZERO; 2]);
/// let mut store = SurfaceStore::new(Stride::of_cells(4), (&mut c, &mut u, &mut g), (&mut ac, &mut au, &mut ag));
/// let mut prim = store.stamped(Which::Primary);
/// let mut row = prim.row_at(0);
/// row.set(0, PackedCell::EMPTY);
/// let _ = prim.row_mut(1);
/// ```
pub struct RowWrite<'s> {
    cells: &'s mut [PackedCell],
    mark: &'s mut RowExtent,
    base: usize,
    /// For the debug assert alone: a release build reads it nowhere.
    pitch: usize,
}

impl RowWrite<'_> {
    /// Column `x`'s cell.
    #[inline(always)]
    pub fn get(&self, x: usize) -> PackedCell {
        self.cells[self.base + x]
    }

    /// Column `x` set to `c`.
    #[inline(always)]
    pub fn set(&mut self, x: usize, c: PackedCell) {
        debug_assert!(x < self.pitch, "a column past the pitch");
        self.cells[self.base + x] = c;
    }

    /// The row's mark.
    #[inline(always)]
    pub fn mark(&mut self) -> MarkMut<'_> {
        MarkMut(&mut *self.mark)
    }
}

/// **A row's mark, stamped when it was minted**, as [`RowMut`] is. Built
/// only in this file.
///
/// ```compile_fail,E0423
/// fn f(e: &mut baud_domain::RowExtent) -> baud_domain::MarkMut<'_> { baud_domain::MarkMut(e) }
/// ```
pub struct MarkMut<'a>(&'a mut RowExtent);

impl Deref for MarkMut<'_> {
    type Target = RowExtent;
    #[inline(always)]
    fn deref(&self) -> &RowExtent {
        self.0
    }
}

impl DerefMut for MarkMut<'_> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut RowExtent {
        self.0
    }
}

/// **Every mark of a surface, every row stamped** ([`Stamped::all_mut`]).
/// Built only in this file.
///
/// ```compile_fail,E0423
/// fn f(e: &mut [baud_domain::RowExtent]) -> baud_domain::MarksMut<'_> { baud_domain::MarksMut(e) }
/// ```
pub struct MarksMut<'a>(&'a mut [RowExtent]);

impl Deref for MarksMut<'_> {
    type Target = [RowExtent];
    fn deref(&self) -> &[RowExtent] {
        self.0
    }
}

impl DerefMut for MarksMut<'_> {
    fn deref_mut(&mut self) -> &mut [RowExtent] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The wrap**: at the last generation a seal stamps every row of every
    /// surface 0, sets every last write to 0 and `now` to 1, and draws a
    /// fresh epoch; a seal kept from before reads every row as written.
    #[test]
    fn a_seal_at_the_last_generation_wraps_to_a_fresh_epoch() {
        let (mut c, mut u, mut g) = (
            [PackedCell::EMPTY; 8],
            [RowExtent::ZERO; 2],
            [RowGen::ZERO; 2],
        );
        let (mut ac, mut au, mut ag) = (
            [PackedCell::EMPTY; 8],
            [RowExtent::ZERO; 2],
            [RowGen::ZERO; 2],
        );
        let (mut sc, mut su) = ([PackedCell::EMPTY; 4], [RowExtent::ZERO; 1]);
        let mut s = SurfaceStore::new(
            Stride::of_cells(4),
            (&mut c, &mut u, &mut g),
            (&mut ac, &mut au, &mut ag),
        );
        s.lend_status(&mut sc, &mut su);
        s.now = WriteGen(u32::MAX - 1);
        let pre = s.seal();
        for w in [Which::Primary, Which::Alternate, Which::Status] {
            let _ = s.stamped(w).row_mut(0);
        }
        let old = s.epoch;
        let new = s.seal();
        assert!(
            s.prim_gens
                .iter()
                .chain(s.alt_gens.iter())
                .chain(&s.status_gens)
                .all(|g| g.0 == 0)
        );
        assert_eq!([s.prim.last, s.alt.last, s.status.last], [WriteGen(0); 3]);
        assert_eq!(s.now, WriteGen(1));
        assert_ne!(s.epoch, old, "the wrap draws a fresh epoch");
        assert_eq!(new.closed, WriteGen(0));
        assert!(s.wrote_since(&pre));
        assert!(!s.wrote_since(&new));
        let at = WindowAt {
            screen: Which::Primary,
            top: RowRot::NONE,
            ring: Rows::of_index(2),
            rows: Rows::of_index(2),
        };
        assert!(s.row_gens(at).after(Row::from_index(0), &pre));
        assert!(!s.row_gens(at).after(Row::from_index(0), &new));
        assert!(
            s.status_gen()
                .is_some_and(|g| g.after(&pre) && !g.after(&new))
        );
    }

    /// Two surfaces of four columns and two rows each, no status line.
    type Mem = ([PackedCell; 8], [RowExtent; 2], [RowGen; 2]);

    fn mem() -> Mem {
        (
            [PackedCell::EMPTY; 8],
            [RowExtent::ZERO; 2],
            [RowGen::ZERO; 2],
        )
    }

    fn view(s: Stamped<'_>) -> View<'_> {
        View::new(
            s,
            Cols::of_index(4),
            Rows::of_index(2),
            Rows::of_index(2),
            RowRot::NONE,
        )
    }

    /// **A borrow that only marks a row wrapped has written**: no cell
    /// moves, so nothing a count of cells written sees, and the surface's
    /// last write is still the borrow's, at its `Drop`.
    #[test]
    fn a_borrow_that_only_marks_a_row_wrapped_has_written() {
        let ((mut c, mut u, mut g), (mut ac, mut au, mut ag)) = (mem(), mem());
        let mut s = SurfaceStore::new(
            Stride::of_cells(4),
            (&mut c, &mut u, &mut g),
            (&mut ac, &mut au, &mut ag),
        );
        let kept = s.seal();
        let before: [PackedCell; 8] = s.cells(Which::Primary).try_into().expect("8 cells");
        view(s.stamped(Which::Primary)).set_wrapped(Row::ZERO);
        assert_eq!(s.cells(Which::Primary), &before[..], "no cell moved");
        assert!(s.used(Which::Primary)[0].wrapped(), "the row is marked");
        assert!(s.wrote_since(&kept));
    }

    /// **The row a borrow stamped last does not ride a swap**: a view
    /// writes its row 0, takes the other surface (`?1049`, DECSASD) and
    /// writes that surface's row 0, whose generation must move as well.
    #[test]
    fn the_row_a_borrow_stamped_last_does_not_ride_a_swap() {
        let ((mut c, mut u, mut g), (mut ac, mut au, mut ag)) = (mem(), mem());
        let mut s = SurfaceStore::new(
            Stride::of_cells(4),
            (&mut c, &mut u, &mut g),
            (&mut ac, &mut au, &mut ag),
        );
        let kept = s.seal();
        {
            let (live, away, _status) = s.split(Which::Primary);
            let mut v = view(live);
            let mut other = Surface::new(away, Rows::of_index(2), RowRot::NONE);
            let x = PackedCell::glyph(Codepoint::from_char('x'), StyleId::DEFAULT, CellFlags::NONE);
            v.put(Row::ZERO, Col::ZERO, x);
            v.swap_surface(&mut other);
            v.put(Row::ZERO, Col::ZERO, x);
        }
        let at = |screen| WindowAt {
            screen,
            top: RowRot::NONE,
            ring: Rows::of_index(2),
            rows: Rows::of_index(2),
        };
        for w in [Which::Primary, Which::Alternate] {
            let g = s.row_gens(at(w));
            assert!(g.after(Row::ZERO, &kept), "{w:?}'s row 0");
            assert!(!g.after(Row::from_index(1), &kept), "{w:?}: the control");
        }
    }

    /// Two surfaces of four columns and six rows each, no status line.
    type Mem6 = ([PackedCell; 24], [RowExtent; 6], [RowGen; 6]);

    fn mem6() -> Mem6 {
        (
            [PackedCell::EMPTY; 24],
            [RowExtent::ZERO; 6],
            [RowGen::ZERO; 6],
        )
    }

    fn gen_is(g: RowGen, w: WriteGen) -> bool {
        g.0 == w.0
    }

    /// **A second store to the row stamped last does not stamp again**, and
    /// a store to it after another row does. The generation is put back to
    /// zero by hand between the two, so the test sees whether the second
    /// mint stored, not only that the row reads written.
    #[test]
    fn a_second_store_to_the_row_does_not_stamp_again() {
        let ((mut c, mut u, mut g), (mut ac, mut au, mut ag)) = (mem6(), mem6());
        let mut s = SurfaceStore::new(
            Stride::of_cells(4),
            (&mut c, &mut u, &mut g),
            (&mut ac, &mut au, &mut ag),
        );
        let now = s.now;
        let mut st = s.stamped(Which::Primary);
        let x = PackedCell::glyph(Codepoint::from_char('x'), StyleId::DEFAULT, CellFlags::NONE);
        st.row_at(3).set(0, x);
        assert!(gen_is(st.gens[3], now), "the first store stamps");
        st.gens[3] = RowGen::ZERO;
        st.row_at(3).set(1, x);
        assert!(gen_is(st.gens[3], WriteGen(0)), "the second does not");
        st.row_at(4).set(0, x);
        st.row_at(3).set(2, x);
        assert!(gen_is(st.gens[3], now), "after another row it does");
        assert!(gen_is(st.gens[4], now));
    }

    /// **A fresh borrow stamps its first row**, whatever the borrow before
    /// it stamped last, and so does one after a seal; row 0 too, so a mint
    /// that started at row 0 rather than at none would show.
    #[test]
    fn a_fresh_stamped_stamps_its_first_row() {
        let ((mut c, mut u, mut g), (mut ac, mut au, mut ag)) = (mem6(), mem6());
        let mut s = SurfaceStore::new(
            Stride::of_cells(4),
            (&mut c, &mut u, &mut g),
            (&mut ac, &mut au, &mut ag),
        );
        let x = PackedCell::glyph(Codepoint::from_char('x'), StyleId::DEFAULT, CellFlags::NONE);
        let now = s.now;
        s.stamped(Which::Primary).row_at(3).set(0, x);
        s.prim_gens[3] = RowGen::ZERO;
        s.stamped(Which::Primary).row_at(3).set(1, x);
        assert!(gen_is(s.prim_gens[3], now), "a second borrow stamps row 3");
        s.stamped(Which::Primary).row_at(0).set(0, x);
        assert!(gen_is(s.prim_gens[0], now), "a fresh borrow stamps row 0");
        let _ = s.seal();
        let after = s.now;
        s.stamped(Which::Primary).row_at(3).set(2, x);
        assert!(gen_is(s.prim_gens[3], after), "after a seal row 3 moves");
        s.stamped(Which::Primary).row_at(0).set(1, x);
        assert!(gen_is(s.prim_gens[0], after), "after a seal row 0 moves");
    }

    /// **A view never spans more columns than the pitch**: a column past it
    /// would be stored in the next row, unstamped.
    #[test]
    #[should_panic(expected = "a view wider than its pitch")]
    fn a_view_wider_than_its_pitch_is_refused() {
        let ((mut c, mut u, mut g), (mut ac, mut au, mut ag)) = (mem(), mem());
        let mut s = SurfaceStore::new(
            Stride::of_cells(4),
            (&mut c, &mut u, &mut g),
            (&mut ac, &mut au, &mut ag),
        );
        let _ = View::new(
            s.stamped(Which::Primary),
            Cols::of_index(5),
            Rows::of_index(2),
            Rows::of_index(2),
            RowRot::NONE,
        );
    }

    /// The same, for a view whose geometry changes after it is built.
    #[test]
    #[should_panic(expected = "a view wider than its pitch")]
    fn a_geometry_wider_than_the_pitch_is_refused() {
        let ((mut c, mut u, mut g), (mut ac, mut au, mut ag)) = (mem(), mem());
        let mut s = SurfaceStore::new(
            Stride::of_cells(4),
            (&mut c, &mut u, &mut g),
            (&mut ac, &mut au, &mut ag),
        );
        let mut v = view(s.stamped(Which::Primary));
        v.set_geometry(Cols::of_index(4), Rows::of_index(2));
        v.set_geometry(Cols::of_index(5), Rows::of_index(2));
    }
}
