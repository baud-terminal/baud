//! **The `OSC 8` link table**: which slot a target takes, and the sweep
//! that gives back the slots nothing names any more.
//!
//! `[[source: osc8-spec]]` sets no limit on how many links a screen holds;
//! its "Length limits" are on the URI and the `id`. The bound here is the
//! cell's: the index rides in seven bits of `CellFlags`, 127 slots and
//! `NONE`, and the other nine bits of the word are taken.
//!
//! **A slot something names is never given to another target**, because a
//! click opens what the slot holds: an evicted rendition is a wrong colour,
//! an evicted link a wrong destination under text that did not change. So
//! a target with no free slot is refused and counted (`Term::links_refused`)
//! and its run is plain text -- and when every slot its probe may take is
//! held, the table is first swept: every slot that no cell, no open link
//! and no parked display names is freed, which only a cell overwritten,
//! scrolled out of the history or erased, or a link closed, can make true.
//! The rendition table and the cluster store reclaim the same way
//! (`Paint::sweep_styles`, `Paint::sweep_and_intern`).
//!
//! **Probed, where it was one slot a target.** A target hashed to a single
//! slot and was refused whenever another held it, so a listing lost links
//! long before the table filled: `ls --hyperlink` of a 37-entry directory
//! linked 34 and of a 227-entry one, 101 of its 202 targets, with 127
//! slots. The bookkeeping is `baud-domain`'s [`LinkBook`].

use baud_domain::{
    Cells, LinkBook, LinkId, LinkKey, LinkLive, LinkProbe, PastEdge, mark_linked_rows,
};

use crate::paint::Paint;
use crate::storage::LINK_MAX;
use crate::term::Screen;

impl<M: crate::bands::BandMode> Paint<'_, '_, M> {
    /// The slot for `uri` under `key`, taken if it is new: `NONE`, counted,
    /// when the table has none to give.
    pub(crate) fn intern_link(&mut self, key: LinkKey, uri: &[u8]) -> LinkId {
        let strs = &mut *self.strs;
        let book: &mut LinkBook = &mut strs.link_book;
        match book.probe(strs.links, strs.link_len, LINK_MAX, key, uri) {
            LinkProbe::Held(l) => l,
            LinkProbe::Free(l) => {
                book.take(l, key, uri, strs.links, strs.link_len, LINK_MAX);
                l
            }
            LinkProbe::Full => self.link_full(key, uri),
        }
    }

    /// A target every slot of whose window holds another: swept for, when a
    /// sweep is due, else refused.
    #[cold]
    #[inline(never)]
    fn link_full(&mut self, key: LinkKey, uri: &[u8]) -> LinkId {
        self.strs.link_book.wanted();
        let screen = Cells::of(self.rows.index() * self.cols.index());
        if self.strs.link_book.due(self.cells, screen) {
            self.sweep_links();
            let strs = &mut *self.strs;
            let book = &mut strs.link_book;
            if let LinkProbe::Free(l) = book.probe(strs.links, strs.link_len, LINK_MAX, key, uri) {
                book.take(l, key, uri, strs.links, strs.link_len, LINK_MAX);
                return l;
            }
        }
        self.links_refused += 1;
        LinkId::NONE
    }

    /// **The sweep**: every slot nothing can name is freed.
    ///
    /// **Everything that can name a slot is marked**, as the cluster sweep
    /// marks its cells, and more: every surface's rows that may hold a
    /// linked cell -- the live surface's screen and the primary's history,
    /// the other surface's, the status line's -- each to its mark and its
    /// last live cell ([`baud_domain::mark_linked_rows`]); the status line
    /// whole; the primary's cells past a narrower live edge, on the rows
    /// that hold any; the open link and the pen built from it; and the open
    /// link and pen of the display not being written, which it writes with
    /// when it is selected again. A holder this misses would give a cell
    /// written later another target. A saved cursor keeps no link a write
    /// can use: `DECRC` rebuilds the pen from the open link
    /// (`Paint::invoke`).
    ///
    /// **What it reads is what holds links, not the ring.** A row is
    /// marked when a linked cell is written into it (`Paint::overwrote`,
    /// [`baud_domain::View::put`], a fill under an open link) and carries
    /// the mark wherever the row moves; a sweep reading every cell of the
    /// buffer read about 940,000 in the app's storage, so a session that
    /// filled the table swept rarely enough that after `clear` the next
    /// listing waited 235 KB for one.
    #[cold]
    #[inline(never)]
    fn sweep_links(&mut self) {
        let mut live = LinkLive::new();
        let (s, cols) = (self.view.stride(), self.cols);
        let mut read = self.view.mark_links(&mut live).index();
        let plane = self.status.surface_mut(&mut self.strs.status.composed);
        for surface in [&mut self.other, plane] {
            read += mark_linked_rows(surface.store_mut(), cols.index(), &mut live).index();
        }
        // The status line whole, wherever it is: one row.
        let (line, main) = match self.on_status() {
            true => (self.view.ring_cells(), self.status.store().cells()),
            false => (self.status.store().cells(), self.view.ring_cells()),
        };
        live.mark(line);
        read += line.len();
        // Past a narrower edge, the primary's rows that hold anything there.
        // A marked row is read to its mark, which covers these cells where
        // the mark does; this read is kept as the safe direction, and no
        // test yet fails without it.
        let (reach, screen) = match self.on_status() {
            true => self.strs.status.park.past_edge(),
            false => (self.book.reach, self.cold.screen),
        };
        if reach.index() > cols.index() {
            let primary = match screen {
                Screen::Normal => main,
                Screen::Alternate => self.other.store().cells(),
            };
            let (a, b) = (cols.index(), reach.index().min(s.index()));
            for (p, f) in self.book.past.iter().enumerate() {
                if *f == PastEdge::Held && (p + 1) * s.index() <= primary.len() {
                    live.mark(&primary[p * s.index() + a..p * s.index() + b]);
                    read += b - a;
                }
            }
        }
        live.name(self.link);
        live.name(self.flags.link());
        for l in self.strs.status.park.links() {
            live.name(l);
        }
        let strs = &mut *self.strs;
        strs.link_book
            .sweep(strs.link_len, &live, self.cells, Cells::of(read));
    }
}
