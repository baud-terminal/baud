//! **The window copied into a frame's own set, and which screen each set
//! holds** -- research decision D4, as the app runs it and `frame_gpu`'s
//! `app` arm measures it.
//!
//! The cell shader does not read the core's grid: the core rewrites the
//! rows of the live window in place, so a frame gets a still copy of the
//! window, laid out flat in screen order, in the set of buffers no frame
//! in flight holds ([`crate::journal::Wheel`]). A frame where only the
//! cursor, its blink animation or its trail moved is built from the screen
//! the last frame was: its cells, its status line and its line renditions
//! are the ones the last frame copied, compared row for row against that
//! frame's copy. Such a frame skips the copy into a set that already holds
//! that screen, and the caller skips the glyph map's walk when nothing
//! else the walk reads changed ([`Screens::walked`]).
//!
//! The same compare says which rows changed, when a frame asks
//! ([`Compare::Rows`]): with BDSM reset, after the renderer's first letter
//! that takes a joining form, a row's joins are found again only where its
//! cells changed (`Renderer::rejoin_bidi`), and no other compare reads the
//! window for them.

use baud_core::StatusLine;
use baud_domain::{LineRendition, PackedCell, Row, Stride, Viewport};
use baud_render::{Leaving, Rejoin, WalkKey};

use crate::journal::{SetIx, Wheel};
use std::sync::atomic::{AtomicU64, Ordering};

/// Whether a frame is built from the last frame's screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Built {
    Last,
    New,
}

/// **What a frame's compare with the last frame's copy answers**: whether
/// the screen is the last's, stopping at the first row that differs, or
/// that and which rows differ, every row compared -- for a frame whose
/// joins are kept from the last ([`Compare::for_rejoin`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compare {
    Screen,
    Rows,
}

impl Compare {
    /// [`Compare::Rows`] when the layout kept the last frame's joins for
    /// the rows that changed (`Renderer::lay_out_bidi_copied`).
    pub fn for_rejoin(rejoin: Option<Rejoin>) -> Compare {
        match rejoin {
            Some(_) => Compare::Rows,
            None => Compare::Screen,
        }
    }
}

/// Whether a row differs from the last frame's copy of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RowChange {
    Same,
    Changed,
}

/// A screen frames were built from: a new one whenever a frame's screen
/// differs from the last's, so two frames that carry the same one show the
/// same cells. **Unique in the process**, not counted per [`Screens`]: the
/// GPU keeps the last committed frame's across a rebuilt record, and two
/// records counting from one each would name different screens alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ScreenGen(u64);

impl ScreenGen {
    /// A screen no frame has carried yet.
    pub fn fresh() -> ScreenGen {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        ScreenGen(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Which screen each set's copy holds, and what the last map walk read.
pub struct Screens {
    /// The screen the last frame was built from.
    now: ScreenGen,
    /// The screen each set's copy holds, or none.
    copied: Vec<Option<ScreenGen>>,
    /// The set the last frame was built into, whose copy is its screen,
    /// and that screen's line renditions and its status line's, if shown.
    last: Option<usize>,
    renditions: Vec<LineRendition>,
    status: Option<LineRendition>,
    /// What the last walk read, as it left the map; none after a frame
    /// that slid, whose walks listed another window. The caller's to keep.
    pub walked: Option<WalkKey>,
    /// What the next frame's compare answers. The caller's to set.
    pub compare: Compare,
    /// Each row of the last frame taken under [`Compare::Rows`]: whether
    /// it differs from the copy the frame before left.
    changed: Vec<RowChange>,
}

impl Default for Screens {
    fn default() -> Screens {
        Screens::new()
    }
}

impl Screens {
    /// One record a buffer set, none holding a screen.
    pub fn new() -> Screens {
        Screens {
            now: ScreenGen::default(),
            copied: vec![None; Wheel::SETS],
            last: None,
            renditions: Vec::new(),
            status: None,
            walked: None,
            compare: Compare::Screen,
            changed: Vec::new(),
        }
    }

    /// The screen the last frame was built from.
    pub fn now(&self) -> ScreenGen {
        self.now
    }

    /// **`set`'s copy no longer holds a screen**: the next frame built into
    /// it copies the window again. A frame that lent its frozen rows slots
    /// rewrote their ids in the copy (`baud_render::Lent`), so the copy is
    /// not the screen's cells.
    pub fn forget(&mut self, set: SetIx) {
        if let Some(c) = self.copied.get_mut(set.0) {
            *c = None;
        }
    }

    /// The screen `set`'s copy holds, or none.
    pub fn holds(&self, set: SetIx) -> Option<ScreenGen> {
        self.copied.get(set.0).copied().flatten()
    }

    /// **The frame about to be built into `set`**: whether `view` and
    /// `status` are the last frame's screen, and that screen copied into
    /// `copies[set]` at `stride` cells a row -- the window's rows in screen
    /// order, then the status line's row -- unless that set's copy already
    /// holds it. A frame that slides -- whose `leaving` row is drawn -- is a
    /// screen of its own: its copy holds a leaving row and its walk another
    /// window.
    ///
    /// Under [`Compare::Rows`] every row is compared with the last copy's,
    /// before this frame's copy can overwrite it -- `set` may be the set the
    /// last frame was built into -- and a frame that slides is compared too,
    /// its window's rows being where the last copy has them; each row is
    /// [`Screens::changed`] where there is no last copy.
    ///
    /// `Viewport::row` resolves a row through the rotation and the ring,
    /// so the wrap at the ring's end costs nothing here: it is two slices
    /// either side of it, and this walks rows rather than one span.
    pub fn take(
        &mut self,
        copies: &mut [&mut [PackedCell]],
        set: SetIx,
        view: &Viewport<'_>,
        status: Option<&StatusLine<'_>>,
        stride: Stride,
        leaving: Leaving,
    ) -> Built {
        let last = self.last.and_then(|ps| copies.get(ps));
        self.changed.clear();
        if self.compare == Compare::Rows {
            let (cols, stride) = (view.cols().index(), stride.index());
            self.changed.extend((0..view.rows().index()).map(|y| {
                let now = view.row(Row::from_index(y)).get(..cols);
                match last.map(|copy| copy.get(y * stride..y * stride + cols)) {
                    Some(kept) if kept == now => RowChange::Same,
                    _ => RowChange::Changed,
                }
            }));
        }
        let built = match (leaving, last) {
            (Leaving::None, Some(copy)) => match self.compare {
                Compare::Screen => self.is_last(copy, view, status, stride),
                Compare::Rows => self.is_last_by(copy, view, status, stride, |y, _| {
                    self.changed[y] == RowChange::Same
                }),
            },
            _ => Built::New,
        };
        self.built(built, view, status);
        let (cols, rows, stride) = (view.cols().index(), view.rows().index(), stride.index());
        let held = self.holds(set);
        if let (Some(copy), false) = (copies.get_mut(set.0), held == Some(self.now)) {
            for y in 0..rows {
                let src = view.row(Row::from_index(y));
                let at = y * stride;
                copy[at..at + cols].copy_from_slice(&src[..cols]);
            }
            // **The status line's row, after the main display's**, at index
            // `rows`: the shader draws `rows` and the line's row
            // (`Renderer::grid`), its backgrounds from here and its glyphs
            // from the records `Overlay::status` adds. No slide runs while
            // it is shown, so the leaving row never wants the index.
            if let Some(line) = status {
                let at = rows * stride;
                let n = cols.min(line.cells.len());
                copy[at..at + n].copy_from_slice(&line.cells[..n]);
            }
            if let Some(c) = self.copied.get_mut(set.0) {
                *c = Some(self.now);
            }
        }
        self.last = Some(set.0);
        built
    }

    /// Whether `view` and `status` show the screen the last frame was built
    /// from, read from `copy`, the copy that frame left at `stride` cells a
    /// row: every row's cells and line rendition, and the status line's.
    pub fn is_last(
        &self,
        copy: &[PackedCell],
        view: &Viewport<'_>,
        status: Option<&StatusLine<'_>>,
        stride: Stride,
    ) -> Built {
        let (cols, at) = (view.cols().index(), stride.index());
        self.is_last_by(copy, view, status, stride, |y, r| {
            copy.get(y * at..y * at + cols) == view.row(r).get(..cols)
        })
    }

    /// [`Screens::is_last`], with whether row `y` holds the cells it held
    /// the last frame answered by `cells_same`.
    fn is_last_by(
        &self,
        copy: &[PackedCell],
        view: &Viewport<'_>,
        status: Option<&StatusLine<'_>>,
        stride: Stride,
        cells_same: impl Fn(usize, Row) -> bool,
    ) -> Built {
        let (cols, rows) = (view.cols().index(), view.rows().index());
        let stride = stride.index();
        let same = self.status == status.map(|l| l.rendition)
            && self.renditions.len() == rows
            && (0..rows).all(|y| {
                let r = Row::from_index(y);
                self.renditions[y] == view.rendition(r) && cells_same(y, r)
            })
            && status.is_none_or(|line| {
                let n = cols.min(line.cells.len());
                copy.get(rows * stride..rows * stride + n) == line.cells.get(..n)
            });
        match same {
            true => Built::Last,
            false => Built::New,
        }
    }

    /// Whether row `r` of the last frame taken under [`Compare::Rows`]
    /// differs from the copy the frame before left: every row when there
    /// was none, or the frame was taken under [`Compare::Screen`].
    pub fn changed(&self, r: Row) -> bool {
        self.changed.get(r.index()) != Some(&RowChange::Same)
    }

    /// A frame built from `view` and `status`, the last frame's screen when
    /// `built` says so, else a screen of its own.
    pub fn built(&mut self, built: Built, view: &Viewport<'_>, status: Option<&StatusLine<'_>>) {
        if built == Built::Last {
            return;
        }
        self.now = ScreenGen::fresh();
        self.renditions.clear();
        self.renditions
            .extend((0..view.rows().index()).map(|y| view.rendition(Row::from_index(y))));
        self.status = status.map(|l| l.rendition);
    }
}

#[cfg(test)]
mod tests {
    use super::{Built, Compare, Screens};
    use crate::journal::SetIx;
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, PackedCell, ReadLen, Row, WinSize};
    use baud_render::Leaving;

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 6,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// The window and the status line copied as the app copies them.
    fn copy(t: &Term<'_>) -> Vec<PackedCell> {
        let stride = t.stride().index();
        let view = t.viewport();
        let (cols, rows) = (view.cols().index(), view.rows().index());
        let mut out = vec![PackedCell::EMPTY; stride * (rows + 1)];
        for y in 0..rows {
            out[y * stride..y * stride + cols]
                .copy_from_slice(&view.row(Row::from_index(y))[..cols]);
        }
        if let Some(line) = t.status_line() {
            let n = cols.min(line.cells.len());
            out[rows * stride..rows * stride + n].copy_from_slice(&line.cells[..n]);
        }
        out
    }

    fn built_from(t: &Term<'_>) -> (Screens, Vec<PackedCell>) {
        let mut s = Screens::new();
        s.built(Built::New, &t.viewport(), t.status_line().as_ref());
        (s, copy(t))
    }

    fn is_last(s: &Screens, c: &[PackedCell], t: &Term<'_>) -> Built {
        s.is_last(c, &t.viewport(), t.status_line().as_ref(), t.stride())
    }

    /// **A frame whose screen is the last frame's skips the walk and the
    /// copy**, and any change to what the shader reads of it is a new
    /// screen: a cell, a line's rendition, the status line appearing or
    /// changing. A cursor move is the last screen.
    #[test]
    fn the_last_screen_is_its_cells_line_renditions_and_status_line() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"hello\r\nworld");
        let (s, c) = built_from(&t);
        assert_eq!(is_last(&s, &c, &t), Built::Last, "the same screen");
        feed(&mut t, b"\x1b[1;3H");
        assert_eq!(is_last(&s, &c, &t), Built::Last, "the cursor moved alone");
        feed(&mut t, b"X");
        assert_eq!(is_last(&s, &c, &t), Built::New, "a cell written");
        let (s, c) = built_from(&t);
        feed(&mut t, b"\x1b[2;1H\x1b#6");
        assert_eq!(is_last(&s, &c, &t), Built::New, "a line made double width");
        let (s, c) = built_from(&t);
        feed(&mut t, b"\x1b[2$~");
        assert_eq!(is_last(&s, &c, &t), Built::New, "the status line shown");
        let (s, c) = built_from(&t);
        assert_eq!(is_last(&s, &c, &t), Built::Last, "and the same again");
        feed(&mut t, b"\x1b[1$}status\x1b[0$}");
        assert_eq!(is_last(&s, &c, &t), Built::New, "the status line written");
    }

    /// **Each set's copy is the screen its frame was built from**, and a
    /// set already holding the screen is not copied again: two sets taken
    /// in turn, the screen copied into each once, a frame on an unchanged
    /// screen the last's, and a changed one a new screen in the set it goes
    /// to. A frame that slides is a new screen whatever it shows.
    #[test]
    fn a_set_is_copied_once_a_screen_and_holds_what_it_was_built_from() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let stride = t.stride();
        let len = stride.index() * (usize::from(WS.ws_row) + 1);
        let mut sets = [vec![PackedCell::EMPTY; len], vec![PackedCell::EMPTY; len]];
        let mut s = Screens::new();
        let take = |s: &mut Screens, t: &Term<'_>, sets: &mut [Vec<PackedCell>; 2], set, slide| {
            let [a, b] = sets;
            let mut copies: [&mut [PackedCell]; 2] = [a, b];
            s.take(&mut copies, SetIx(set), &t.viewport(), None, stride, slide)
        };
        feed(&mut t, b"hello");
        assert_eq!(
            take(&mut s, &t, &mut sets, 0, Leaving::None),
            Built::New,
            "the first"
        );
        assert_eq!(sets[0], copy(&t), "set 0 holds it");
        let first = s.now();
        assert_eq!(
            take(&mut s, &t, &mut sets, 1, Leaving::None),
            Built::Last,
            "unchanged"
        );
        assert_eq!(s.now(), first, "the same screen");
        assert_eq!(sets[1], copy(&t), "set 1 holds it too");
        assert_eq!(take(&mut s, &t, &mut sets, 0, Leaving::None), Built::Last);
        // A cell changed behind the record's back in set 1, which holds the
        // screen: the next frame into it compares against set 0 and does
        // not copy, so the cell stays -- the copy was skipped.
        sets[1][1] = PackedCell::EMPTY;
        assert_eq!(take(&mut s, &t, &mut sets, 1, Leaving::None), Built::Last);
        assert_ne!(sets[1], copy(&t), "set 1 was not copied again");
        sets[1] = copy(&t);
        feed(&mut t, b" world");
        assert_eq!(
            take(&mut s, &t, &mut sets, 0, Leaving::None),
            Built::New,
            "a cell written"
        );
        assert_ne!(s.now(), first, "a new screen");
        assert_eq!(sets[0], copy(&t), "copied into the set it goes to");
        let second = s.now();
        assert_eq!(
            take(&mut s, &t, &mut sets, 1, Leaving::Drawn),
            Built::New,
            "a slide"
        );
        assert_ne!(s.now(), second, "is a screen of its own");
        assert_eq!(sets[1], copy(&t));
    }

    /// **Under [`Compare::Rows`] the compare names the rows that changed**:
    /// against the last frame's copy, also when this frame goes into the
    /// same set and overwrites it, and on a frame that slides; every row
    /// with no last copy, or when the frame was not compared by row.
    #[test]
    fn the_rows_compare_names_the_rows_that_changed() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let stride = t.stride();
        let len = stride.index() * (usize::from(WS.ws_row) + 1);
        let mut sets = [vec![PackedCell::EMPTY; len], vec![PackedCell::EMPTY; len]];
        let mut s = Screens::new();
        s.compare = Compare::Rows;
        let take = |s: &mut Screens, t: &Term<'_>, sets: &mut [Vec<PackedCell>; 2], set, slide| {
            let [a, b] = sets;
            let mut copies: [&mut [PackedCell]; 2] = [a, b];
            s.take(&mut copies, SetIx(set), &t.viewport(), None, stride, slide)
        };
        let rows = usize::from(WS.ws_row);
        let changed = |s: &Screens| -> Vec<usize> {
            (0..rows)
                .filter(|&y| s.changed(Row::from_index(y)))
                .collect()
        };
        let every: Vec<usize> = (0..rows).collect();
        feed(&mut t, b"hello\r\nworld");
        assert_eq!(take(&mut s, &t, &mut sets, 0, Leaving::None), Built::New);
        assert_eq!(changed(&s), every, "no last copy");
        assert_eq!(take(&mut s, &t, &mut sets, 0, Leaving::None), Built::Last);
        assert_eq!(changed(&s), Vec::<usize>::new(), "the same screen");
        feed(&mut t, b"\x1b[2;1HW");
        assert_eq!(take(&mut s, &t, &mut sets, 0, Leaving::None), Built::New);
        assert_eq!(changed(&s), vec![1], "one row, into the set it was in");
        assert_eq!(sets[0], copy(&t), "and copied after it was compared");
        assert_eq!(take(&mut s, &t, &mut sets, 1, Leaving::None), Built::Last);
        assert_eq!(changed(&s), Vec::<usize>::new(), "the other set");
        feed(&mut t, b"\x1b[1;1HH");
        assert_eq!(take(&mut s, &t, &mut sets, 0, Leaving::Drawn), Built::New);
        assert_eq!(changed(&s), vec![0], "a slide, compared by row");
        s.compare = Compare::Screen;
        assert_eq!(take(&mut s, &t, &mut sets, 1, Leaving::None), Built::Last);
        assert_eq!(changed(&s), every, "not compared by row");
    }
}
