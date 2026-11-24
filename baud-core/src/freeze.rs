//! **A full rendition table freezes what lies off the active screen**, and
//! never gives a slot a drawn cell names to another rendition.
//!
//! `[[source: ecma48]]` § 8.3.117: "SGR is used to establish one or more
//! graphic rendition aspects for subsequent text." `[[source: dec-std-070]]`'s
//! SGR, page 5-104, note 3: "In no way does the selection of current
//! rendition affect the rendition of characters already entered into the
//! display." And `[[source: itu-t416]]` § 8.1.7, the text ECMA-48's 38 and 48
//! point to: "For each character, the character image is imaged in a colour
//! referred to as the character foreground colour and the character image
//! background is imaged in a colour referred to as the character background
//! colour." A table that evicted a slot some cell still named gave those
//! characters another's colours, after it had drawn them right: a picture
//! printed into the shell read back 50-92% of its history in other colours
//! against a terminal that keeps each cell's colour in the cell.
//!
//! **The bound.** What must resolve through the table is the active screen:
//! at most a screen of cells, the status row, slot 0, the current rendition
//! and the parked display's -- 61,611 slots at the app's 400x150 at a pitch of
//! 408, and the table is sized to hold that for every screen up to the
//! 65,536-slot ceiling (`Owned::with_history`). Everything else -- the
//! history, the rows a shorter window cut, the screen not up -- can be moved
//! into a **generation** ([`baud_domain::FrozenStore`]): its cells rewritten
//! to dense ids into the generation's own words, and tagged with the line
//! they hold. Then the sweep, which marks only the rows that still resolve
//! through the table, frees everything else, and no slot a cell names is
//! ever written.
//!
//! **What keeps a tag honest.** A row's tag is believed only while the row
//! holds the line it was frozen as, standing where it stood (behind the
//! window, on the parked screen, or past the bottom). So a line feed that
//! reuses a row writes nothing here. What brings a row's cells back onto
//! the active screen thaws it -- interns its words again and rewrites its ids
//! -- or blanks it: a screen switch ([`Paint::thaw_screen`]), a taller window
//! and a re-cut (`Term::resize`'s `frozen_resized`); a reverse scroll and the
//! prompt's retreat blank the rows they take back. What takes a screen row
//! off clears its tag, so a row that came back blank is never read through
//! its old generation ([`Paint::frozen_unretired`]).
//!
//! **What a storage without generations does, and a screen over the bound.**
//! A storage lent no pool freezes nothing, and a screen of more than 65,536
//! less its pitch less three cells (65,125 at the 408 pitch) can name more
//! renditions than the table holds. Either way a table with no slot
//! left after its sweep refuses the next rendition: the text that follows
//! takes the default rendition, counted in [`Reclaims::refusals`], and no cell
//! already drawn changes. `[[source: ecma48]]` § 8.3.117 leaves which
//! combinations a terminal shows to it: "The usable combinations of parameter
//! values are determined by the implementation."
//!
//! Everything here is out of line and cold: the per-byte path is unchanged.

use crate::bands::Slots;
use crate::paint::Paint;
use crate::term::Screen;
use baud_domain::*;

/// Which screen the main display has up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Up {
    Primary,
    Alternate,
}

/// What a freeze did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Froze {
    /// Nothing new lay off the screen, or no pool was lent.
    Nothing,
    /// Rows moved into a generation: a sweep can free their slots now.
    Rows,
}

/// **The main display's geometry**: the screen it has up, and the primary's
/// window over its ring -- whichever display is being written.
#[derive(Clone, Copy)]
pub(crate) struct Main {
    pub(crate) up: Up,
    pub(crate) rows: Rows,
    pub(crate) top: LineIdx,
    pub(crate) hist: Rows,
    pub(crate) hidden: Rows,
    pub(crate) rot: RowRot,
    pub(crate) ring: Rows,
}

impl Main {
    /// The line physical row `p` of the primary holds, when it is retained.
    pub(crate) fn line_of(&self, p: RowRot) -> Option<LineIdx> {
        let ring = self.ring.index();
        let off = (p.index() + ring - self.rot.index()) % ring;
        if off < self.rows.index() + self.hidden.index() {
            return Some(self.top.plus(Rows::of_count(off)));
        }
        let back = ring - off;
        match back <= self.hist.index() {
            true => self.top.minus(Rows::of_count(back)),
            false => None,
        }
    }

    /// The physical row line `l` is at.
    pub(crate) fn phys(&self, l: LineIdx) -> RowRot {
        match l.since(self.top) {
            Some(n) => self.rot.advance(n.index(), self.ring),
            None => self
                .rot
                .retreat(self.top.since(l).map_or(0, |n| n.index()), self.ring),
        }
    }

    pub(crate) fn at(&self, l: LineIdx) -> FrozenAt {
        FrozenAt::of(l, self.top, self.rows)
    }

    pub(crate) fn screen(&self) -> PrimaryScreen {
        match self.up {
            Up::Primary => PrimaryScreen::Up,
            Up::Alternate => PrimaryScreen::Parked,
        }
    }

    /// Whether a row standing `at` may be frozen: off the screen, or
    /// anywhere while the alternate screen is up.
    pub(crate) fn may_freeze(&self, at: FrozenAt) -> bool {
        at != FrozenAt::Screen || self.up == Up::Alternate
    }

    /// The retained lines, oldest first.
    pub(crate) fn lines(&self) -> impl Iterator<Item = LineIdx> {
        let lo = self.top.minus(self.hist).unwrap_or(LineIdx::FIRST);
        let n = self.hist.index() + self.rows.index() + self.hidden.index();
        (0..n).map(move |i| lo.plus(Rows::of_count(i)))
    }

    /// Whether `tag`, on physical row `p`, still holds.
    pub(crate) fn holds(&self, p: RowRot, tag: RowFrozen) -> bool {
        self.line_of(p) == Some(tag.line) && self.at(tag.line) == tag.at && self.may_freeze(tag.at)
    }
}

/// The surfaces a freeze rewrites, split from the rest of `Paint`.
struct Parts<'p, 's, 'b> {
    prim: &'p mut Stamped<'s>,
    alt: &'p mut Stamped<'s>,
    frz: &'p mut FrozenStore<'b>,
    tmpl: &'p [StyleSlot],
    stride: Stride,
}

/// Whether any cell names a slot but the default: a row that names none
/// reads the same through every table, so it is never frozen.
fn names_any(cells: &[PackedCell]) -> bool {
    cells.iter().any(|c| c.style != StyleId::DEFAULT)
}

/// The tags that no longer hold, cleared.
fn drop_stale(frz: &mut FrozenStore<'_>, m: &Main) {
    frz.retain_tags(|p, t| m.holds(p, t));
}

/// **Every generation rebuilt to the ids its rows still name**, when a
/// compaction left the pool short: a generation keeps every word it was
/// built with while its rows expire one at a time, so many such can hold
/// more words than the cells that name them. The rows' ids are rewritten,
/// and each generation gets a new serial.
fn rebuild_all(frz: &mut FrozenStore<'_>, prim: &mut Stamped<'_>, alt: &mut Stamped<'_>, m: &Main) {
    let ring = m.ring.index();
    for i in 0..frz.gen_records() {
        let Some(g) = frz.gen_at(i) else {
            continue;
        };
        let mut named = SlotSet::new();
        for p in 0..ring {
            if frz
                .tag(RowRot::NONE.advance(p, m.ring))
                .is_some_and(|t| t.generation == g)
            {
                named.mark(prim.row(p));
            }
        }
        if frz.alt() == Some(g) {
            named.mark(alt.cells());
        }
        frz.rebuild(g, &named);
        for p in 0..ring {
            if frz
                .tag(RowRot::NONE.advance(p, m.ring))
                .is_some_and(|t| t.generation == g)
            {
                for c in prim.row_mut(p).iter_mut() {
                    c.style = frz.rebuilt(c.style);
                }
            }
        }
        if frz.alt() == Some(g) {
            for c in alt.all_mut().0.iter_mut() {
                c.style = frz.rebuilt(c.style);
            }
        }
        frz.forget_map();
    }
}

impl<'a, 'b, M: crate::bands::BandMode> Paint<'a, 'b, M> {
    /// **The main display's geometry**: on the status line, the display
    /// parked while it is written.
    pub(crate) fn main(&self) -> Main {
        let on_status = self.on_status();
        let (screen, rows) = match on_status {
            true => (self.strs.status.park_screen(), self.strs.status.park_rows()),
            false => (self.cold.screen, self.rows),
        };
        let up = match screen {
            Screen::Normal => Up::Primary,
            Screen::Alternate => Up::Alternate,
        };
        let (rot, ring) = match (on_status, up) {
            (false, Up::Primary) => (self.view.rotation(), self.view.ring()),
            (true, Up::Primary) => (self.status.rot(), self.status.ring()),
            (_, Up::Alternate) => (self.other.rot(), self.other.ring()),
        };
        Main {
            up,
            rows,
            top: self.book.top,
            hist: self.book.hist,
            hidden: self.book.hidden,
            rot,
            ring,
        }
    }

    fn parts(&mut self) -> Option<Parts<'_, 'a, 'b>> {
        let stride = self.view.stride();
        let screen = match self.on_status() {
            true => (true, self.strs.status.park_screen()),
            false => (false, self.cold.screen),
        };
        let frz = self.style_log.frozen.as_mut()?;
        let tmpl: &[StyleSlot] = self.tmpl.as_slice();
        let (prim, alt): (&mut Stamped<'a>, &mut Stamped<'a>) = match screen {
            (false, Screen::Normal) => (self.view.store_mut(), self.other.store_mut()),
            (false, Screen::Alternate) => (self.other.store_mut(), self.view.store_mut()),
            (true, Screen::Normal) => (
                self.status.write(&mut self.strs.status.composed),
                self.other.store_mut(),
            ),
            (true, Screen::Alternate) => (
                self.other.store_mut(),
                self.status.write(&mut self.strs.status.composed),
            ),
        };
        Some(Parts {
            prim,
            alt,
            frz,
            tmpl,
            stride,
        })
    }

    /// Whether any row or surface names a generation.
    pub(crate) fn any_frozen(&self) -> bool {
        self.style_log.frozen.as_ref().is_some_and(|f| f.any())
    }

    /// **Move everything off the active screen that still resolves through
    /// the table into a generation**: the alternate screen while the primary
    /// is up, and every retained row of the primary off its screen -- every
    /// row of it while the alternate screen is up -- that is not frozen
    /// already and names a slot but the default. Returns at once when
    /// nothing has moved since the last freeze, so a full table that misses
    /// on every rendition walks no row. Nothing in the banded mode, whose
    /// bands hold every rendition their rows name and freeze only across a
    /// change of width ([`Paint::freeze_bands`]).
    #[cold]
    #[inline(never)]
    pub(crate) fn freeze_off_screen(&mut self) -> Froze {
        if M::BANDED {
            return Froze::Nothing;
        }
        let m = self.main();
        let Some(parts) = self.parts() else {
            return Froze::Nothing;
        };
        let (prim, alt, frz, tmpl, stride) =
            (parts.prim, parts.alt, parts.frz, parts.tmpl, parts.stride);
        if frz.thawing() == Thawing::Busy || frz.unmoved(m.top) {
            return Froze::Nothing;
        }
        frz.mark(m.top);
        let s = stride.index();
        drop_stale(frz, &m);
        let alt_due = m.up == Up::Primary && frz.alt().is_none() && names_any(alt.cells());
        let mut due = 0;
        for l in m.lines() {
            let (at, p) = (m.at(l), m.phys(l));
            if m.may_freeze(at) && frz.valid(p, l, at).is_none() && names_any(prim.row(p.index())) {
                due += 1;
            }
        }
        if !alt_due && due == 0 {
            return Froze::Nothing;
        }
        // Room for every row due and a default word each, before a row is
        // touched: the dead generations' words first, then every
        // generation's words no row names.
        let need = due * (s + 1) + if alt_due { alt.cells().len() + 1 } else { 0 };
        if frz.room() < need {
            frz.reclaim();
        }
        if frz.room() < need {
            rebuild_all(frz, prim, alt, &m);
            frz.reclaim();
        }
        let mut froze = Froze::Nothing;
        // The alternate screen as one generation, so it thaws as one when
        // it is up again; a screen whose cells pass the table cannot be.
        if alt_due {
            match (alt.cells().len() < tmpl.len())
                .then(|| frz.begin())
                .flatten()
            {
                Some(g) => {
                    if frz.freeze_row(g, alt.all_mut().0, tmpl) == RowFreeze::Frozen {
                        frz.set_alt(Some(g));
                        froze = Froze::Rows;
                    }
                    frz.finish();
                }
                None => frz.counted_unfrozen(),
            }
        }
        let mut g = None;
        for l in m.lines() {
            let (at, p) = (m.at(l), m.phys(l));
            if !m.may_freeze(at) || frz.valid(p, l, at).is_some() {
                continue;
            }
            if !names_any(prim.row(p.index())) {
                continue;
            }
            let placed = loop {
                let cur = match g.or_else(|| frz.begin()) {
                    Some(c) => c,
                    None => break None,
                };
                g = Some(cur);
                match frz.freeze_row(cur, prim.row_mut(p.index()), tmpl) {
                    RowFreeze::Frozen => break Some(cur),
                    // A generation that took nothing has no room in the
                    // pool either: no later row fits.
                    RowFreeze::NoRoom if frz.table(cur).words().len() <= 1 => break None,
                    RowFreeze::NoRoom => {
                        frz.finish();
                        g = None;
                    }
                }
            };
            match placed {
                Some(cur) => {
                    frz.set_tag(
                        p,
                        Some(RowFrozen {
                            line: l,
                            at,
                            generation: cur,
                        }),
                    );
                    frz.counted_frozen();
                    froze = Froze::Rows;
                }
                None => frz.counted_unfrozen(),
            }
        }
        if g.is_some() {
            frz.finish();
        }
        if froze == Froze::Rows {
            frz.counted_freeze();
            self.style_log.book_mut().sweep.rewrote();
        }
        froze
    }

    /// **The sweep's marks**: every row that resolves through the table --
    /// the active screen, the status row, and whatever off it is not frozen
    /// -- blanks past a row's extent included, since an erase leaves its
    /// rendition there. A row the ring holds no line in names nothing a
    /// reader can reach: whatever comes into it next is written or blanked
    /// first.
    /// **The ids band `b`'s rows name**, in the banded mode: its surface's
    /// rows in its span, on whichever store holds that surface now, and
    /// the roots ([`crate::bands::BandRoots`]). No other row names its
    /// slots, and nothing is frozen.
    pub(crate) fn mark_band_live(
        &self,
        live: &mut SlotSet,
        b: Band,
        roots: crate::bands::BandRoots,
    ) {
        use crate::bands::Surface;
        let m = self.main();
        let s = self.view.stride().index();
        let on_status = self.on_status();
        let (other, line) = (self.other.store().cells(), self.status.store().cells());
        let (prim, alt, status): (&[PackedCell], &[PackedCell], &[PackedCell]) =
            match (on_status, m.up) {
                (false, Up::Primary) => (self.view.ring_cells(), other, line),
                (false, Up::Alternate) => (other, self.view.ring_cells(), line),
                (true, Up::Primary) => (line, other, self.view.ring_cells()),
                (true, Up::Alternate) => (other, line, self.view.ring_cells()),
            };
        let (surface, rows) = self.style_log.layout.span_of(b);
        let cells = match surface {
            Surface::Primary => prim,
            Surface::Alt => alt,
            Surface::Status => status,
        };
        let from = (rows.start * s).min(cells.len());
        let to = rows.end.saturating_mul(s).min(cells.len());
        live.mark(&cells[from..to]);
        for id in roots.ids() {
            live.insert(id);
        }
        // A thaw's interned slots, which the cells it has not rewritten yet
        // do not name ([`Paint::thaw_bands`]).
        if let Some(f) = self
            .style_log
            .frozen
            .as_ref()
            .filter(|f| f.thawing() == Thawing::Busy)
        {
            f.pin(live);
        }
    }

    pub(crate) fn mark_live(&self, live: &mut SlotSet) {
        let m = self.main();
        let s = self.view.stride().index();
        let on_status = self.on_status();
        match on_status {
            true => live.mark(self.view.ring_cells()),
            false => live.mark(self.status.store().cells()),
        }
        let (other, line) = (self.other.store().cells(), self.status.store().cells());
        let (prim, alt): (&[PackedCell], &[PackedCell]) = match (on_status, m.up) {
            (false, Up::Primary) => (self.view.ring_cells(), other),
            (false, Up::Alternate) => (other, self.view.ring_cells()),
            (true, Up::Primary) => (line, other),
            (true, Up::Alternate) => (other, line),
        };
        let frz = self.style_log.frozen.as_ref();
        if frz.is_none_or(|f| f.alt().is_none()) {
            live.mark(alt);
        }
        // **A thaw's rows are not the table's until they are rewritten**: a
        // row it holds in the spare still names frozen ids, which read as
        // slots would keep whatever slot shares a number, and the slots it
        // has interned so far are named by cells it rewrote (`thaw_screen`).
        if let Some(f) = frz.filter(|f| f.thawing() == Thawing::Busy) {
            f.pin(live);
        }
        for l in m.lines() {
            let (at, p) = (m.at(l), m.phys(l));
            let frozen = m.may_freeze(at) && frz.is_some_and(|f| f.valid(p, l, at).is_some());
            let thawing = frz.is_some_and(|f| f.spare(p).is_some());
            if !frozen && !thawing {
                live.mark(&prim[p.index() * s..(p.index() + 1) * s]);
            }
        }
    }

    /// The surface `up` names, whole -- the primary's ring or the alternate
    /// screen's buffer -- and the frozen store beside it, two disjoint
    /// borrows.
    /// **Rows a View operation wrote with another band's ids, translated
    /// into their own band's** ([`crate::bands::Moves`]): each recorded
    /// row's columns whose ids name a band other than the row's, read
    /// through that band's table and interned into the row's, and then the
    /// cursor's band again. Nothing in the single mode.
    #[inline(always)]
    pub(crate) fn moved(&mut self, x: &M::Cross) {
        if M::BANDED {
            self.moved_banded(x);
        }
    }

    #[cold]
    #[inline(never)]
    fn moved_banded(&mut self, x: &M::Cross) {
        use crate::bands::{Origin, Recorded};
        let ran = self.style_log.band();
        for (to, origin, cols) in x.moves() {
            let from = match origin {
                Origin::Row(p) => self.band_of_phys(p),
                Origin::Blank => ran,
            };
            if self.band_of_phys(to) != from {
                self.translate_cells(to, from, cols);
            }
        }
        self.band_for(self.cy);
    }

    /// **Columns `cols` of physical row `p`, whose ids name band `from`'s
    /// slots, interned into `p`'s band**: their words read first and their
    /// ids reset to the default, so no sweep while they are interned keeps
    /// a slot of `p`'s band for an id that is `from`'s.
    #[cold]
    #[inline(never)]
    fn translate_cells(&mut self, p: PhysRow, from: Band, cols: core::ops::Range<usize>) {
        let (row, p) = (p, p.index());
        const WIDE: usize = widest_stride_cells(crate::storage::MAX_COLS);
        let up = self.main().up;
        let n = cols.len().min(WIDE);
        let mut ids = [StyleId::DEFAULT; WIDE];
        for (i, c) in self.surface(up).0.row(p)[cols.start..cols.start + n]
            .iter()
            .enumerate()
        {
            ids[i] = c.style;
        }
        let mut words = [StyleWord::DEFAULT; WIDE];
        let src = M::band(&self.tmpl, from);
        for i in 0..n {
            words[i] = src
                .get(ids[i].index())
                .and_then(|s| s.word())
                .unwrap_or(StyleWord::DEFAULT);
        }
        for c in self.surface(up).0.row_mut(p)[cols.start..cols.start + n].iter_mut() {
            c.style = StyleId::DEFAULT;
        }
        let to = self.band_of_phys(row);
        self.enter_band(to);
        for (i, &w) in words[..n].iter().enumerate() {
            if ids[i] == StyleId::DEFAULT {
                continue;
            }
            let id = self.intern_style(w);
            self.surface(up).0.row_mut(p)[cols.start + i].style = id;
        }
    }

    fn surface(&mut self, up: Up) -> (&mut Stamped<'a>, Option<&mut FrozenStore<'b>>) {
        let on_status = self.on_status();
        let screen = match on_status {
            true => self.strs.status.park_screen(),
            false => self.cold.screen,
        };
        let frozen = self.style_log.frozen.as_mut();
        let s = match (on_status, screen, up) {
            (false, Screen::Normal, Up::Primary) | (false, Screen::Alternate, Up::Alternate) => {
                self.view.store_mut()
            }
            (true, Screen::Normal, Up::Primary) | (true, Screen::Alternate, Up::Alternate) => {
                self.status.write(&mut self.strs.status.composed)
            }
            _ => self.other.store_mut(),
        };
        (s, frozen)
    }

    /// Frozen id `id` of generation `g` interned into the table, once a
    /// thaw.
    fn thaw_id(&mut self, g: GenIdx, id: StyleId) -> StyleId {
        if let Some(l) = self.style_log.frozen.as_ref().and_then(|f| f.thawed(id)) {
            return l;
        }
        let w = self
            .style_log
            .frozen
            .as_ref()
            .map_or(StyleWord::DEFAULT, |f| f.word(g, id));
        let l = self.intern_style(w);
        if let Some(f) = self.style_log.frozen.as_mut() {
            f.set_thawed(id, l);
        }
        l
    }

    /// Physical row `p` of surface `up` thawed through generation `g`, in
    /// **two passes and one stamp**. The first reads each id and interns it
    /// (`thaw_id`), which may sweep; the second takes the row once and
    /// rewrites each id from the map the first filled. Nothing between the
    /// two forgets the map: a sweep while the thaw is busy keeps every slot
    /// it interned (`FrozenStore::pin`), the freezes that forget it return at
    /// once while it is busy, and `thaw_screen` forgets it only after this
    /// returns. An id the map lacks in the second pass would leave a frozen
    /// id read through the live table -- the recolour the `tmpl` rule
    /// forbids -- so it is a real assert, on a cold path.
    fn thaw_row(&mut self, up: Up, g: GenIdx, p: RowRot) {
        let s = self.view.stride().index();
        for i in 0..s {
            let id = self.surface(up).0.row(p.index())[i].style;
            if id != StyleId::DEFAULT {
                let _ = self.thaw_id(g, id);
            }
        }
        let (surface, frozen) = self.surface(up);
        let f = frozen.expect("a thaw runs only with a frozen store");
        for c in surface.row_mut(p.index()).iter_mut() {
            if c.style == StyleId::DEFAULT {
                continue;
            }
            c.style = f
                .thawed(c.style)
                .expect("every id the first pass met is in the thaw's map");
        }
        f.counted_thaw(s);
    }

    /// **The active screen's frozen rows interned again into the table**:
    /// the primary's rows a resize or a switch brought onto its screen
    /// still frozen, or the whole alternate screen. Room first: when the
    /// table cannot take the incoming cells under its load, what lies off
    /// the new screen is frozen and the table swept. The rows' generations
    /// are held in the spare while their ids are rewritten, so no compaction
    /// moves them, and no freeze runs until the thaw is done.
    #[cold]
    #[inline(never)]
    pub(crate) fn thaw_screen(&mut self) {
        if self.on_status() || M::BANDED {
            return;
        }
        let m = self.main();
        let s = self.view.stride().index();
        let Some(frz) = self.style_log.frozen.as_mut() else {
            return;
        };
        let mut cells = 0;
        let screen = (0..m.rows.index()).map(|y| m.top.plus(Rows::of_count(y)));
        match m.up {
            Up::Primary => {
                for l in screen {
                    let p = m.phys(l);
                    if let Some(g) = frz.valid(p, l, FrozenAt::Screen) {
                        frz.set_spare(p, Some(g));
                        frz.set_tag(p, None);
                        cells += s;
                    }
                }
            }
            Up::Alternate => {
                if frz.alt().is_some() {
                    cells = self.view.ring_cells().len();
                }
            }
        }
        if cells == 0 {
            return;
        }
        let len = self.tmpl.as_slice().len();
        let filled = self.style_log.book_mut().sweep.filled;
        if (filled + cells.min(len)) * SweepBook::LOAD.1 > len * SweepBook::LOAD.0 {
            let _ = self.freeze_off_screen();
            if let Some(f) = self.style_log.frozen.as_mut() {
                f.set_thawing(Thawing::Busy);
            }
            let _ = self.sweep_styles_opt(None);
        }
        let Some(frz) = self.style_log.frozen.as_mut() else {
            return;
        };
        frz.set_thawing(Thawing::Busy);
        match m.up {
            Up::Primary => {
                for y in 0..m.rows.index() {
                    let p = m.phys(m.top.plus(Rows::of_count(y)));
                    let g = self.style_log.frozen.as_ref().and_then(|f| f.spare(p));
                    if let Some(g) = g {
                        self.thaw_row(Up::Primary, g, p);
                        if let Some(f) = self.style_log.frozen.as_mut() {
                            f.set_spare(p, None);
                            f.forget_map();
                        }
                    }
                }
            }
            Up::Alternate => {
                // The alternate screen stays its generation's until every
                // row is rewritten, so no sweep reads its frozen ids as
                // slots; the slots interned meanwhile are pinned by the map.
                let g = self.style_log.frozen.as_ref().and_then(|f| f.alt());
                if let Some(g) = g {
                    let ring = self.view.ring();
                    for k in 0..ring.index() {
                        self.thaw_row(Up::Alternate, g, RowRot::NONE.advance(k, ring));
                    }
                    if let Some(f) = self.style_log.frozen.as_mut() {
                        f.set_alt(None);
                        f.forget_map();
                    }
                }
            }
        }
        if let Some(f) = self.style_log.frozen.as_mut() {
            f.set_thawing(Thawing::Idle);
            f.moved();
        }
        self.style_log.book_mut().sweep.rewrote();
    }

    /// **Every retained row of the primary that names a slot but the
    /// default, frozen through its own band's table**, in the banded mode,
    /// before a change of width moves cells from row to row and so from
    /// band to band: a row's ids name its band's slots and no other's. A
    /// generation holds one band's rows, so its map reads one table. Room
    /// is found before a row is touched: `Short` when the pool lent cannot
    /// take every row, and then nothing is frozen.
    #[cold]
    #[inline(never)]
    pub(crate) fn freeze_bands(&mut self) -> FreezeBands {
        use crate::bands::Surface;
        let m = self.main();
        let s = self.view.stride().index();
        let layout = self.style_log.layout;
        let screen = match self.on_status() {
            true => (true, self.strs.status.park_screen()),
            false => (false, self.cold.screen),
        };
        let prim: &mut Stamped<'a> = match screen {
            (false, Screen::Normal) => self.view.store_mut(),
            (true, Screen::Normal) => self.status.write(&mut self.strs.status.composed),
            (_, Screen::Alternate) => self.other.store_mut(),
        };
        let Some(frz) = self.style_log.frozen.as_mut() else {
            return FreezeBands::Short;
        };
        let tmpl = &self.tmpl;
        let due = m
            .lines()
            .filter(|&l| names_any(prim.row(m.phys(l).index())))
            .count();
        if due == 0 {
            return FreezeBands::Nothing;
        }
        // Every row due and a default word for each generation, which is at
        // most a row each.
        let need = due * (s + 1);
        if frz.room() < need {
            frz.reclaim();
        }
        if frz.room() < need || frz.free_gens() < due {
            return FreezeBands::Short;
        }
        let mut cur: Option<(Band, GenIdx)> = None;
        for l in m.lines() {
            let (at, p) = (m.at(l), m.phys(l));
            if !names_any(prim.row(p.index())) {
                continue;
            }
            let b = layout.band_of(Surface::Primary, PhysRow::of(p.index()));
            let table = M::band(tmpl, b);
            let placed = loop {
                let g = match cur {
                    Some((cb, g)) if cb == b => g,
                    _ => {
                        if cur.is_some() {
                            frz.finish();
                        }
                        match frz.begin() {
                            Some(g) => {
                                cur = Some((b, g));
                                g
                            }
                            None => break None,
                        }
                    }
                };
                match frz.freeze_row(g, prim.row_mut(p.index()), table) {
                    RowFreeze::Frozen => break Some(g),
                    RowFreeze::NoRoom if frz.table(g).words().len() <= 1 => break None,
                    RowFreeze::NoRoom => {
                        frz.finish();
                        cur = None;
                    }
                }
            };
            match placed {
                Some(g) => {
                    frz.set_tag(
                        p,
                        Some(RowFrozen {
                            line: l,
                            at,
                            generation: g,
                        }),
                    );
                    frz.counted_frozen();
                }
                // Not reached: the room above holds every row due, a
                // generation each at worst. A row left on its old ids would
                // read another rendition once its table is emptied, so a
                // test build stops here.
                None => {
                    frz.counted_unfrozen();
                }
            }
        }
        if cur.is_some() {
            frz.finish();
        }
        frz.counted_freeze();
        FreezeBands::Froze
    }

    /// **After a change of width in the banded mode, every row a band
    /// freeze tagged interned again into the band it stands in now**
    /// (`Term::frozen_resized` has tagged each by where its line landed).
    /// Every primary band's table is emptied to its default first: no row
    /// names its slots while all are frozen, and an emptied table holds
    /// every rendition its rows can name ([`crate::Provision`]), so a thaw
    /// never needs a slot a stale one holds. Then the pen is interned again
    /// from its word into the cursor's band -- the display parked keeps a
    /// word and no id ([`crate::bands::BandRoots`]) -- and each primary
    /// band's table is a new one to a drawer that kept its slots. The pool
    /// is released.
    #[cold]
    #[inline(never)]
    pub(crate) fn thaw_bands(&mut self) {
        use crate::bands::Surface;
        let layout = self.style_log.layout;
        let (first, end) = layout.surface_bands(Surface::Primary);
        let primary = first.up_to(end);
        for b in primary.clone() {
            M::at(&mut self.tmpl, b);
            crate::bands::clear_written(&mut self.tmpl.as_mut()[1..], StyleSlot::Empty);
            self.style_log.emptied(b);
        }
        let ring = self.surface(Up::Primary).0.rows();
        if let Some(f) = self.style_log.frozen.as_mut() {
            f.forget_map();
            f.set_thawing(Thawing::Busy);
        }
        let mut last: Option<(Band, GenIdx)> = None;
        for k in 0..ring {
            let p = RowRot::NONE.advance(k, Rows::of_index(ring));
            let Some(t) = self.style_log.frozen.as_ref().and_then(|f| f.tag(p)) else {
                continue;
            };
            let b = layout.band_of(Surface::Primary, PhysRow::of(k));
            if last != Some((b, t.generation)) {
                if let Some(f) = self.style_log.frozen.as_mut() {
                    f.forget_map();
                }
                M::at(&mut self.tmpl, b);
                self.style_log.set_band(b);
                last = Some((b, t.generation));
            }
            self.thaw_row(Up::Primary, t.generation, p);
        }
        if let Some(f) = self.style_log.frozen.as_mut() {
            f.forget_map();
            f.clear_tags();
            f.clear_spare();
            f.set_thawing(Thawing::Idle);
            f.release();
        }
        // The pen, into the band the write path is in.
        self.enter_band(self.band_of(self.cy));
        for b in primary {
            self.style_log.reminted(b);
        }
        self.style_log.book_mut().sweep.rewrote();
        self.pen_repaired();
    }

    /// **A reverse scroll on the primary took a row back at the top and
    /// pushed one past the bottom**, both blank or on screen: the row it
    /// took in is blanked as it arrives, and the row it pushed out was on
    /// the screen, so neither holds a frozen line. At line zero the
    /// numbering cannot follow the rotation, so every row holds the line
    /// after the one it held, and every tag moves on with it.
    #[cold]
    #[inline(never)]
    pub(crate) fn frozen_unretired(&mut self, renumbered: Renumbered) {
        let (rot, ring, rows) = (self.view.rotation(), self.view.ring(), self.rows);
        if let Some(f) = self.style_log.frozen.as_mut() {
            if renumbered == Renumbered::Yes {
                f.renumber_up();
            }
            f.set_tag(rot, None);
            f.set_tag(rot.advance(rows.index(), ring), None);
            f.moved();
        }
    }

    /// **Rows the window took back at the top and gave up at the bottom**,
    /// `n` of each: the prompt's retreat, which erases the rows it takes
    /// back and gives up rows that show nothing.
    #[cold]
    #[inline(never)]
    pub(crate) fn frozen_retreated(&mut self, n: Rows) {
        let (rot, ring, rows) = (self.view.rotation(), self.view.ring(), self.rows);
        if let Some(f) = self.style_log.frozen.as_mut() {
            for k in 0..n.index() {
                f.set_tag(rot.advance(k, ring), None);
                f.set_tag(rot.advance(rows.index() + k, ring), None);
            }
            f.moved();
        }
    }

    /// **The history erased** (`ED 3`): its rows hold no line now, and the
    /// generations only they named are freed at once.
    #[cold]
    #[inline(never)]
    pub(crate) fn frozen_history_erased(&mut self) {
        let m = self.main();
        if let Some(f) = self.style_log.frozen.as_mut() {
            drop_stale(f, &m);
            f.reclaim();
            f.moved();
        }
    }

    /// **A full reset**: the whole ring and the alternate screen erased, so
    /// no cell names a generation and every one is freed at once.
    #[cold]
    #[inline(never)]
    pub(crate) fn frozen_reset(&mut self) {
        if let Some(f) = self.style_log.frozen.as_mut() {
            f.release();
        }
    }

    /// The alternate screen cleared on entry: it names no generation.
    pub(crate) fn frozen_alt_cleared(&mut self) {
        if let Some(f) = self.style_log.frozen.as_mut() {
            f.set_alt(None);
            f.moved();
        }
    }

    /// The screens switched: what was up is off the screen now.
    pub(crate) fn frozen_switched(&mut self) {
        if let Some(f) = self.style_log.frozen.as_mut() {
            f.moved();
        }
    }
}

/// What a band freeze did ([`Paint::freeze_bands`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FreezeBands {
    /// Every row that names a slot but the default is frozen.
    Froze,
    /// No row names one: nothing to carry.
    Nothing,
    /// No pool is lent, or it cannot take every row: nothing is frozen.
    Short,
}

/// Whether a reverse scroll moved the rotation without the numbering: at
/// line zero there is no line before the first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Renumbered {
    No,
    Yes,
}

#[cfg(feature = "probe-band-pin")]
impl<M: crate::bands::BandMode> Paint<'_, '_, M> {
    /// **`n` renditions no cell names interned into band `band` and held
    /// there** (`probe-band-pin`): each its own, and kept by the band's
    /// sweep, so they take `n` slots for as long as the terminal lives.
    /// The slots held, fewer when the band refused one.
    pub(crate) fn pin_band(
        &mut self,
        band: Band,
        n: crate::storage::BandSlots,
    ) -> crate::storage::BandSlots {
        M::at(&mut self.tmpl, band);
        self.style_log.set_band(band);
        self.style_log.pins.band = band;
        let n = u16::try_from(n.0.min(crate::bands::Pins::CAP)).unwrap_or(0);
        let mut held = 0;
        for i in 0..n {
            let w = StyleWord::DEFAULT
                .with_fg(ColorWord::rgb(
                    Chan::from_u8(251),
                    Chan::from_u8(252),
                    Chan::from_offset(i, 0),
                ))
                .with_bg(ColorWord::rgb(
                    Chan::from_u8(253),
                    Chan::from_u8(254),
                    Chan::from_offset(i, 0),
                ));
            let id = self.intern_style(w);
            if id == StyleId::DEFAULT {
                break;
            }
            self.style_log.pins.hold(id);
            held += 1;
        }
        self.enter_band(self.band_of(self.cy));
        crate::storage::BandSlots(held)
    }
}

impl<M: crate::bands::BandMode> Paint<'_, '_, M> {
    /// **Banded, whether the pen is its word in the band it writes**: the
    /// band the write path is in is the cursor row's on the display the
    /// live fields hold, and the pen's id names a slot of that band holding
    /// the pen's word -- or the band refused it, and the pen is the default.
    /// What `swap_display` and `thaw_bands` leave, checked after each
    /// ([`Paint::pen_repaired`]), so a pen left on another band's id is
    /// counted there and not repaired unseen by a later band entry. True in
    /// the single mode.
    pub(crate) fn pen_is_its_word(&self) -> bool {
        if !M::BANDED {
            return true;
        }
        let band = self.live_band();
        let held = self
            .tmpl
            .as_slice()
            .get(self.style.index())
            .and_then(|s| s.word());
        let refused =
            self.style == StyleId::DEFAULT && self.style_log.book().sweep.reclaims.refusals > 0;
        self.style_log.band() == band && (held == Some(self.style_raw) || refused)
    }

    /// The band of the cursor's row on the display the live fields hold.
    fn live_band(&self) -> Band {
        use crate::bands::Surface;
        let surface = match (self.on_status(), self.cold.screen) {
            (true, _) => Surface::Status,
            (false, Screen::Alternate) => Surface::Alt,
            (false, Screen::Normal) => Surface::Primary,
        };
        self.style_log
            .layout
            .band_of(surface, self.view.phys_row(self.cy))
    }

    /// **The pen put right where it is not its word in its band**, and
    /// counted ([`baud_domain::BandRepairs`]): interned again from its word
    /// into the band of the cursor's row. A shipped terminal repairs a pen
    /// a band entry missed rather than stop; a test reads the count as 0.
    pub(crate) fn pen_repaired(&mut self) {
        if !self.pen_is_its_word() {
            self.enter_band(self.live_band());
            self.style_log.repairs.pens += 1;
        }
    }
}
