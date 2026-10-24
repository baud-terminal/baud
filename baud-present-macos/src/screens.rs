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

use baud_core::StatusLine;
use baud_domain::{LineRendition, PackedCell, Row, Stride, Viewport};
use baud_render::WalkKey;

use crate::journal::{SetIx, Wheel};
use std::sync::atomic::{AtomicU64, Ordering};

/// Whether a frame is built from the last frame's screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Built {
    Last,
    New,
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
        }
    }

    /// The screen the last frame was built from.
    pub fn now(&self) -> ScreenGen {
        self.now
    }

    /// The screen `set`'s copy holds, or none.
    pub fn holds(&self, set: SetIx) -> Option<ScreenGen> {
        self.copied.get(set.0).copied().flatten()
    }

    /// **The frame about to be built into `set`**: whether `view` and
    /// `status` are the last frame's screen, and that screen copied into
    /// `copies[set]` at `stride` cells a row -- the window's rows in screen
    /// order, then the status line's row -- unless that set's copy already
    /// holds it. A frame that slides is a screen of its own: its copy holds
    /// a leaving row and its walk another window.
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
        sliding: bool,
    ) -> Built {
        let built = match (sliding, self.last.and_then(|ps| copies.get(ps))) {
            (false, Some(copy)) => self.is_last(copy, view, status, stride),
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
        let (cols, rows) = (view.cols().index(), view.rows().index());
        let stride = stride.index();
        let same = self.status == status.map(|l| l.rendition)
            && self.renditions.len() == rows
            && (0..rows).all(|y| {
                let r = Row::from_index(y);
                self.renditions[y] == view.rendition(r)
                    && copy.get(y * stride..y * stride + cols) == view.row(r).get(..cols)
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
    use super::{Built, Screens};
    use crate::journal::SetIx;
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, PackedCell, ReadLen, Row, WinSize};

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
            take(&mut s, &t, &mut sets, 0, false),
            Built::New,
            "the first"
        );
        assert_eq!(sets[0], copy(&t), "set 0 holds it");
        let first = s.now();
        assert_eq!(
            take(&mut s, &t, &mut sets, 1, false),
            Built::Last,
            "unchanged"
        );
        assert_eq!(s.now(), first, "the same screen");
        assert_eq!(sets[1], copy(&t), "set 1 holds it too");
        assert_eq!(take(&mut s, &t, &mut sets, 0, false), Built::Last);
        // A cell changed behind the record's back in set 1, which holds the
        // screen: the next frame into it compares against set 0 and does
        // not copy, so the cell stays -- the copy was skipped.
        sets[1][1] = PackedCell::EMPTY;
        assert_eq!(take(&mut s, &t, &mut sets, 1, false), Built::Last);
        assert_ne!(sets[1], copy(&t), "set 1 was not copied again");
        sets[1] = copy(&t);
        feed(&mut t, b" world");
        assert_eq!(
            take(&mut s, &t, &mut sets, 0, false),
            Built::New,
            "a cell written"
        );
        assert_ne!(s.now(), first, "a new screen");
        assert_eq!(sets[0], copy(&t), "copied into the set it goes to");
        let second = s.now();
        assert_eq!(take(&mut s, &t, &mut sets, 1, true), Built::New, "a slide");
        assert_ne!(s.now(), second, "is a screen of its own");
        assert_eq!(sets[1], copy(&t));
    }
}
