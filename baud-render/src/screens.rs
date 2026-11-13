//! **The window copied into a frame's own set, and which screen each set
//! holds** -- research decision D4, as the app runs it and `frame_gpu`'s
//! `app` arm measures it.
//!
//! The cell shader does not read the core's grid: the core rewrites the
//! rows of the live window in place, so a frame gets a still copy of the
//! window, laid out flat in screen order, in the set of buffers no frame
//! in flight holds (`baud-present-macos`'s `Wheel`). A frame where only the
//! cursor, its blink animation or its trail moved is built from the screen
//! the last frame was. Such a frame skips the copy into a set that already
//! holds that screen, and the caller skips the glyph map's walk when
//! nothing else the walk reads changed ([`Screens::walked`]).
//!
//! **Which rows changed is the core's to say, not a compare's.** Every
//! store to a cell or a mark stamps its row with the core's write
//! generation (`baud_domain::SurfaceStore`), and a driver seals one
//! generation a frame (`Term::seal_writes`). Each set keeps the [`Seal`]
//! its copy was last brought up to date under, with the window it copied
//! ([`WindowAt`]) and what else the copy was drawn under; a row is copied
//! again only when it was written after that seal ([`RowGens::after`]), so
//! a frame where nothing was written reads no cell. Two kinds of row are
//! drawn from more than their stored cells and are still compared cell
//! for cell: a row the bidirectional layout drew in another order, whose
//! order reads the terminal's state beyond the row, and every row of a
//! display the layout re-broke (`crate::rebreak`).
//!
//! Each set has its own key, and the frame's answer -- whether its screen
//! is the last frame's -- is the last set's: a frame built into one set
//! after another set was brought up to date compares against the one its
//! own copy was taken under for the copy, and against the last frame's
//! for the answer.
//!
//! The same rule says which rows changed, when a frame asks
//! ([`Compare::Rows`]): with BDSM reset, after the renderer's first letter
//! that takes a joining form, a row's joins are found again only where its
//! cells changed (`Renderer::rejoin_bidi`).

use crate::counters::{Counter, Site, add, compared};
use crate::{Leaving, Lending, Rejoin, Tally, WalkKey};
use baud_core::{StatusLine, Term};
use baud_domain::{
    Cols, LineRendition, PackedCell, Row, RowGens, Rows, Seal, StatusGen, Stride, StyleId,
    Viewport, WindowAt,
};

use std::sync::atomic::{AtomicU64, Ordering};

/// **The buffer sets a frame is built into**: one for the frame the GPU
/// reads and one for the frame being built, `baud-present-macos`'s
/// `Wheel::SETS`, whose reason it is.
pub const SETS: usize = 2;

/// Which of the per-frame buffer sets a frame is built into.
///
/// The buffers are the caller's -- the resolved styles and the overlay
/// mask, which live in the app -- so this names a set rather than holding
/// one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetIx(pub usize);

/// Whether a frame is built from the last frame's screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Built {
    Last,
    New,
}

/// **What a frame's take answers about its rows**: whether the screen is
/// the last's, or that and which rows differ -- for a frame whose joins
/// are kept from the last ([`Compare::for_rejoin`]).
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

/// Whether the layout drew a row in another order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Reordered {
    No,
    Yes,
}

impl Reordered {
    /// **Rows `0..n`, each as `drawn` says it was laid out**, into `out`,
    /// whose buffer is kept: the window's rows, then the status line's.
    pub(crate) fn rows_into(out: &mut Vec<Reordered>, n: Rows, drawn: impl Fn(Row) -> bool) {
        out.clear();
        out.extend((0..n.index()).map(|y| match drawn(Row::from_index(y)) {
            true => Reordered::Yes,
            false => Reordered::No,
        }));
    }
}

/// **Whether the layout drew a display it re-broke** (`crate::rebreak`):
/// every row then a display line, not a stored row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Held {
    No,
    Yes,
}

impl Held {
    /// Whether `orders` hold a re-broken display.
    pub fn of(orders: &crate::Orders) -> Held {
        match orders.display().is_some() {
            true => Held::Yes,
            false => Held::No,
        }
    }
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

/// **The frame a set is taken for**: the terminal; the seal its driver
/// closed the frame's writes with (`Term::seal_writes`), which the copy is
/// recorded under; how far its window looks back, which names the stored
/// rows and their write generations (`Term::row_gens`); the window as the
/// layout drew it and its status line; which of those rows the layout
/// drew in another order -- the status line's at the row after the
/// window's -- and whether it re-broke a display; the pitch of the sets'
/// copies; and whether a smooth scroll's leaving row is drawn.
pub struct Taking<'a, 'v, 't> {
    pub term: &'a Term<'t>,
    pub seal: Seal,
    pub back: Rows,
    pub view: &'a Viewport<'v>,
    pub status: Option<&'a StatusLine<'v>>,
    pub reordered: &'a dyn Fn(Row) -> bool,
    pub held: Held,
    pub stride: Stride,
    pub leaving: Leaving,
}

/// **The scalar part of what a copy was drawn under**: its size, its
/// pitch, and whether the status line was shown and in which rendition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Scalar {
    cols: Cols,
    rows: Rows,
    stride: Stride,
    status: Option<LineRendition>,
}

/// **What a set's copy was last brought up to date under**: the seal, the
/// window, the scalars, whether a display was held, and the screen it is.
#[derive(Clone, Copy, Debug)]
struct Head {
    seal: Seal,
    at: WindowAt,
    scalar: Scalar,
    held: Held,
    screen: ScreenGen,
}

/// **One copy's key** -- a set's, or the CPU path's own copy of the window
/// (`Renderer::convert`) -- its head while the copy holds a screen, and
/// the window's line renditions and reordered rows at that build, in
/// buffers kept across frames so a frame allocates none.
#[derive(Default)]
pub(crate) struct SetKey {
    head: Option<Head>,
    renditions: Vec<LineRendition>,
    reordered: Vec<Reordered>,
}

/// **This frame, as a key compares it.**
pub(crate) struct Now<'g> {
    gens: RowGens<'g>,
    status: Option<StatusGen>,
    scalar: Scalar,
    held: Held,
}

impl<'g> Now<'g> {
    /// The window `back` lines into `term`'s history, drawn as `view` with
    /// `status` below it, laid out as `held` says, copied at `stride`.
    pub(crate) fn of(
        term: &'g Term<'_>,
        back: Rows,
        (view, status): (&Viewport<'_>, Option<&StatusLine<'_>>),
        held: Held,
        stride: Stride,
    ) -> Now<'g> {
        Now {
            gens: term.row_gens(back),
            status: term.status_gen(),
            scalar: Scalar {
                cols: view.cols(),
                rows: view.rows(),
                stride,
                status: status.map(|l| l.rendition),
            },
            held,
        }
    }
}

/// How much of a copy a key keeps: all of it, the rows its rule names, or
/// none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Keeps {
    All,
    Rows,
    Nothing,
}

impl SetKey {
    /// **How much of the copy this key was taken under still holds for
    /// `now`**: nothing for no key, another window or other scalars; all of
    /// it when nothing was written since its seal and no row is drawn from
    /// more than its cells, now or then; else the rows [`SetKey::dirty`]
    /// does not name.
    pub(crate) fn keeps(&self, term: &Term<'_>, now: &Now<'_>, reordered: &[Reordered]) -> Keeps {
        let Some(h) = &self.head else {
            return Keeps::Nothing;
        };
        if h.at != now.gens.window() || h.scalar != now.scalar {
            return Keeps::Nothing;
        }
        let drawn = |r: &[Reordered]| r.contains(&Reordered::Yes);
        match term.wrote_since(&h.seal)
            || h.held == Held::Yes
            || now.held == Held::Yes
            || drawn(&self.reordered)
            || drawn(reordered)
        {
            false => Keeps::All,
            true => Keeps::Rows,
        }
    }

    /// **Whether row `r` of the copy may not be the window's now**: written
    /// after the key's seal, drawn in another order now or then, a display
    /// held now or then, or another line rendition. Row `rows` is the
    /// status line's, written when its own generation says so.
    pub(crate) fn dirty(
        &self,
        now: &Now<'_>,
        reordered: &[Reordered],
        view: &Viewport<'_>,
        r: Row,
    ) -> bool {
        let Some(h) = &self.head else {
            return true;
        };
        let drawn = |rows: &[Reordered]| rows.get(r.index()) == Some(&Reordered::Yes);
        if drawn(&self.reordered) || drawn(reordered) {
            return true;
        }
        if r.index() == view.rows().index() {
            return now.status.is_none_or(|g| g.after(&h.seal));
        }
        h.held == Held::Yes
            || now.held == Held::Yes
            || now.gens.after(r, &h.seal)
            || self.renditions.get(r.index()) != Some(&view.rendition(r))
    }

    /// **This key, for a copy brought up to date now**: under `seal`, the
    /// term's `Term::sealed` as the copy was read, holding `screen`.
    pub(crate) fn record(
        &mut self,
        (seal, screen): (Seal, ScreenGen),
        now: &Now<'_>,
        view: &Viewport<'_>,
        reordered: &[Reordered],
    ) {
        self.head = Some(Head {
            seal,
            at: now.gens.window(),
            scalar: now.scalar,
            held: now.held,
            screen,
        });
        self.renditions.clear();
        self.renditions
            .extend((0..view.rows().index()).map(|y| view.rendition(Row::from_index(y))));
        self.reordered.clear();
        self.reordered.extend_from_slice(reordered);
    }

    /// The copy no longer holds a screen.
    pub(crate) fn forget(&mut self) {
        self.head = None;
    }

    /// Whether row `r` is drawn from more than its stored cells, now or at
    /// this key's build, so that only a compare of its cells says it is the
    /// same.
    fn compared(&self, now: &Now<'_>, reordered: &[Reordered], r: Row) -> bool {
        let held = self.head.is_some_and(|h| h.held == Held::Yes) || now.held == Held::Yes;
        let drawn = |rows: &[Reordered]| rows.get(r.index()) == Some(&Reordered::Yes);
        held || drawn(&self.reordered) || drawn(reordered)
    }
}

/// Which screen each set's copy holds, and what the last map walk read.
pub struct Screens {
    /// The screen the last frame was built from.
    now: ScreenGen,
    /// Each set's key.
    keys: Vec<SetKey>,
    /// The set the last frame was built into, whose key is the last
    /// frame's.
    last: Option<usize>,
    /// This frame's reordered rows, the status line's after the window's,
    /// in a buffer kept across frames.
    reordered: Vec<Reordered>,
    /// The last copy's row read through a lending's undo, kept across
    /// frames.
    scratch: Vec<PackedCell>,
    /// What the last walk read, as it left the map; none after a frame
    /// that slid, whose walks listed another window. The caller's to keep.
    pub walked: Option<WalkKey>,
    /// What the next frame's take answers. The caller's to set.
    pub compare: Compare,
    /// Each row of the last frame taken under [`Compare::Rows`]: whether
    /// it may differ from the copy the frame before left.
    changed: Vec<RowChange>,
    /// **The lending the last frame's copy was drawn under**
    /// (`baud_render::Lent`): a frame whose screen is the last's is built
    /// from it only under the same lending, since a lent id means what it
    /// means only under its lending. The caller's to keep.
    pub lending: Lending,
}

impl Default for Screens {
    fn default() -> Screens {
        Screens::new()
    }
}

impl Screens {
    /// One key a buffer set, none holding a screen.
    pub fn new() -> Screens {
        Screens {
            now: ScreenGen::default(),
            keys: (0..SETS).map(|_| SetKey::default()).collect(),
            last: None,
            reordered: Vec::new(),
            scratch: Vec::new(),
            walked: None,
            compare: Compare::Screen,
            changed: Vec::new(),
            lending: Lending::default(),
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
        if let Some(k) = self.keys.get_mut(set.0) {
            k.forget();
        }
    }

    /// The screen `set`'s copy holds, or none.
    pub fn holds(&self, set: SetIx) -> Option<ScreenGen> {
        self.keys.get(set.0).and_then(|k| k.head).map(|h| h.screen)
    }

    /// The set the last frame was built into.
    pub fn last_set(&self) -> Option<SetIx> {
        self.last.map(SetIx)
    }

    /// **The frame about to be built into `set`**: whether `w`'s window
    /// and status line are the last frame's screen, and that window
    /// brought up to date in `copies[set]` at `w.stride` cells a row -- the
    /// window's rows in screen order, then the status line's row -- by
    /// copying the rows written since that set's copy was last brought up
    /// to date, and nothing when none was or when the frame is the last
    /// frame's screen and the set already holds it. A frame that slides --
    /// whose `leaving` row is drawn -- is a screen of its own, copied
    /// whole: its copy holds a leaving row and its walk another window.
    ///
    /// Under [`Compare::Rows`] each row's answer is kept, against the last
    /// frame's key -- `set` may be the set the last frame was built into,
    /// so the answer is found before this frame's copy is touched -- and a
    /// frame that slides is answered too; each row is
    /// [`Screens::changed`] where there is no last frame.
    pub fn take(
        &mut self,
        copies: &mut [&mut [PackedCell]],
        set: SetIx,
        w: &Taking<'_, '_, '_>,
    ) -> Built {
        self.take_lent(copies, set, w, &[])
    }

    /// **[`Screens::take`], the last frame's copy read through `undo`**
    /// where a row of it is compared: the cells a lending rewrote in it, by
    /// row of the copy and column, with the id each held
    /// (`baud_render::Lent::rewrites`), so a copy lent a struck link's
    /// slots compares as the screen it was copied from. Its own set's,
    /// which no frame in flight reads, is put back before
    /// (`Lent::restore`), and so names none.
    pub fn take_lent(
        &mut self,
        copies: &mut [&mut [PackedCell]],
        set: SetIx,
        w: &Taking<'_, '_, '_>,
        undo: &[(usize, usize, StyleId)],
    ) -> Built {
        let view = w.view;
        let (cols, rows, stride) = (view.cols().index(), view.rows().index(), w.stride.index());
        let now = Now::of(w.term, w.back, (view, w.status), w.held, w.stride);
        let lined = usize::from(w.status.is_some());
        Reordered::rows_into(
            &mut self.reordered,
            Rows::of_index(rows + lined),
            w.reordered,
        );
        let reordered = &self.reordered[..];
        // **The last frame's answer, before this frame's copy is touched.**
        self.changed.clear();
        let mut same = true;
        let last = self
            .last
            .and_then(|l| Some((self.keys.get(l)?, copies.get(l)?)));
        match last.map(|(k, c)| (k, c, k.keeps(w.term, &now, reordered))) {
            None | Some((_, _, Keeps::Nothing)) => {
                same = false;
                self.changed.resize(rows, RowChange::Changed);
            }
            Some((_, _, Keeps::All)) => self.changed.resize(rows, RowChange::Same),
            Some((k, copy, Keeps::Rows)) => {
                let scratch = &mut self.scratch;
                let mut differs = |y: usize| {
                    if !k.dirty(&now, reordered, view, Row::from_index(y)) {
                        return false;
                    }
                    if !k.compared(&now, reordered, Row::from_index(y)) {
                        return true;
                    }
                    let (at, n) = match y == rows {
                        true => (
                            rows * stride,
                            w.status.map_or(0, |l| cols.min(l.cells.len())),
                        ),
                        false => (y * stride, cols),
                    };
                    let kept = copy.get(at..at + n);
                    let drawn = match y == rows {
                        true => w.status.and_then(|l| l.cells.get(..n)),
                        false => view.row(Row::from_index(y)).get(..n),
                    };
                    !row_same(scratch, undo, (kept, drawn), Row::from_index(y))
                };
                for y in 0..rows {
                    let d = differs(y);
                    same &= !d;
                    self.changed.push(match d {
                        true => RowChange::Changed,
                        false => RowChange::Same,
                    });
                }
                if lined == 1 && differs(rows) {
                    same = false;
                }
            }
        }
        let built = match (w.leaving, same) {
            (Leaving::None, true) => Built::Last,
            _ => Built::New,
        };
        if self.compare == Compare::Screen {
            self.changed.clear();
        }
        // **This frame's copy, against its own set's key**, and none at
        // all into a set that already holds the screen this frame is: the
        // last frame's, found above, which every frame since that set's
        // build has been, so its copy is that screen's cells, the rows
        // drawn in another order among them.
        let holds = built == Built::Last && self.holds(set) == Some(self.now);
        if let (Some(copy), Some(k)) = (copies.get_mut(set.0), self.keys.get(set.0)) {
            let keeps = match (w.leaving, holds) {
                (Leaving::Drawn, _) => Keeps::Nothing,
                (Leaving::None, true) => Keeps::All,
                (Leaving::None, false) => k.keeps(w.term, &now, reordered),
            };
            let mut copied = 0;
            for y in 0..rows + lined {
                let kept = match keeps {
                    Keeps::All => true,
                    Keeps::Rows => !k.dirty(&now, reordered, view, Row::from_index(y)),
                    Keeps::Nothing => false,
                };
                if kept {
                    continue;
                }
                let at = y * stride;
                // **The status line's row, after the main display's**, at
                // index `rows`: the shader draws `rows` and the line's row
                // (`Renderer::grid`), its backgrounds from here and its
                // glyphs from the records `Overlay::status` adds. No slide
                // runs while it is shown, so the leaving row never wants
                // the index.
                let src = match y == rows {
                    true => w
                        .status
                        .map_or(&[][..], |l| &l.cells[..cols.min(l.cells.len())]),
                    false => &view.row(Row::from_index(y))[..cols],
                };
                copy[at..at + src.len()].copy_from_slice(src);
                copied += src.len();
            }
            add(Counter::CellsCopied, Site::Screens, Tally::of(copied));
        }
        if built == Built::New {
            self.now = ScreenGen::fresh();
        }
        if let Some(k) = self.keys.get_mut(set.0) {
            k.record((w.seal, self.now), &now, view, &self.reordered);
        }
        self.last = Some(set.0);
        built
    }

    /// Whether row `r` of the last frame taken under [`Compare::Rows`] may
    /// differ from the copy the frame before left: every row when there
    /// was none, or the frame was taken under [`Compare::Screen`].
    pub fn changed(&self, r: Row) -> bool {
        self.changed.get(r.index()) != Some(&RowChange::Same)
    }

    /// **The last frame was built as a screen of its own** when `built`
    /// says so, whatever its take answered: a caller that lent its copy
    /// otherwise than the frame before (`baud_render::Lent`). The copy
    /// itself is the window's still, so its set keeps its key and takes the
    /// new screen.
    pub fn built(&mut self, built: Built) {
        if built == Built::Last {
            return;
        }
        self.now = ScreenGen::fresh();
        if let Some(h) = self
            .last
            .and_then(|l| self.keys.get_mut(l))
            .and_then(|k| k.head.as_mut())
        {
            h.screen = self.now;
        }
    }
}

/// **Whether `kept`, a row of the last copy, is the screen's `now`**: as
/// copied, or once every rewrite `undo` names on its row `r` is undone,
/// the last first, in `scratch`. Every cell either compare reads counted.
fn row_same(
    scratch: &mut Vec<PackedCell>,
    undo: &[(usize, usize, StyleId)],
    (kept, now): (Option<&[PackedCell]>, Option<&[PackedCell]>),
    r: Row,
) -> bool {
    compared(Site::Screens, kept, now);
    if kept == now {
        return true;
    }
    let (Some(kept), Some(now)) = (kept, now) else {
        return false;
    };
    if !undo.iter().any(|&(ry, _, _)| ry == r.index()) {
        return false;
    }
    scratch.clear();
    scratch.extend_from_slice(kept);
    for &(ry, x, was) in undo.iter().rev() {
        if let (true, Some(c)) = (ry == r.index(), scratch.get_mut(x)) {
            c.style = was;
        }
    }
    compared(Site::Screens, Some(scratch), Some(now));
    scratch[..] == now[..]
}

#[cfg(test)]
mod tests {
    use super::{Built, Compare, Held, Screens, SetIx, Taking};
    use crate::Leaving;
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, PackedCell, ReadLen, Row, Rows, WinSize};

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

    /// Two sets, as the app keeps them.
    fn sets(t: &Term<'_>) -> [Vec<PackedCell>; 2] {
        let len = t.stride().index() * (usize::from(WS.ws_row) + 1);
        [vec![PackedCell::EMPTY; len], vec![PackedCell::EMPTY; len]]
    }

    /// **A frame taken as a driver takes it**: one generation sealed, then
    /// the live window and its status line taken into `set`, the rows in
    /// `drawn` laid out in another order and a display `held` or not.
    fn frame(
        s: &mut Screens,
        t: &mut Term<'_>,
        sets: &mut [Vec<PackedCell>; 2],
        set: SetIx,
        (slide, drawn, held): (Leaving, &[usize], Held),
    ) -> Built {
        let seal = t.seal_writes();
        let [a, b] = sets;
        let mut copies: [&mut [PackedCell]; 2] = [a, b];
        let view = t.viewport();
        let status = t.status_line();
        let reordered = |r: Row| drawn.contains(&r.index());
        s.take(
            &mut copies,
            set,
            &Taking {
                term: t,
                seal,
                back: Rows::ZERO,
                view: &view,
                status: status.as_ref(),
                reordered: &reordered,
                held,
                stride: t.stride(),
                leaving: slide,
            },
        )
    }

    const PLAIN: (Leaving, &[usize], Held) = (Leaving::None, &[], Held::No);

    /// **A frame whose screen is the last frame's is built from it**, and
    /// any change to what the shader reads of it is a new screen: a cell, a
    /// line's rendition, the status line appearing or changing. A cursor
    /// move is the last screen.
    #[test]
    fn the_last_screen_is_its_cells_line_renditions_and_status_line() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut sets = sets(&t);
        let mut s = Screens::new();
        let mut step = |t: &mut Term<'_>, bytes: &[u8]| {
            feed(t, bytes);
            frame(&mut s, t, &mut sets, SetIx(0), PLAIN)
        };
        assert_eq!(step(&mut t, b"hello\r\nworld"), Built::New, "the first");
        assert_eq!(step(&mut t, b""), Built::Last, "the same screen");
        assert_eq!(
            step(&mut t, b"\x1b[1;3H"),
            Built::Last,
            "the cursor moved alone"
        );
        assert_eq!(step(&mut t, b"X"), Built::New, "a cell written");
        assert_eq!(
            step(&mut t, b"\x1b[2;1H\x1b#6"),
            Built::New,
            "a line made double width"
        );
        assert_eq!(
            step(&mut t, b"\x1b[2$~"),
            Built::New,
            "the status line shown"
        );
        assert_eq!(step(&mut t, b""), Built::Last, "and the same again");
        assert_eq!(
            step(&mut t, b"\x1b[1$}status\x1b[0$}"),
            Built::New,
            "the status line written"
        );
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
        let mut sets = sets(&t);
        let mut s = Screens::new();
        feed(&mut t, b"hello");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        assert_eq!(sets[0], copy(&t), "set 0 holds it");
        let first = s.now();
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(1), PLAIN),
            Built::Last,
            "unchanged"
        );
        assert_eq!(s.now(), first, "the same screen");
        assert_eq!(sets[1], copy(&t), "set 1 holds it too");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::Last
        );
        // A cell changed behind the key's back in set 1, which holds the
        // screen: nothing was written since its key's seal, so the next
        // frame into it reads no cell and copies none, and the cell stays.
        sets[1][1] = PackedCell::EMPTY;
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(1), PLAIN),
            Built::Last
        );
        assert_ne!(sets[1], copy(&t), "set 1 was not copied again");
        sets[1] = copy(&t);
        feed(&mut t, b" world");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::New,
            "a cell written"
        );
        assert_ne!(s.now(), first, "a new screen");
        assert_eq!(sets[0], copy(&t), "copied into the set it goes to");
        let second = s.now();
        let slide = (Leaving::Drawn, &[][..], Held::No);
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(1), slide),
            Built::New,
            "a slide"
        );
        assert_ne!(s.now(), second, "is a screen of its own");
        assert_eq!(sets[1], copy(&t));
    }

    /// **Under [`Compare::Rows`] the take names the rows that changed**:
    /// against the last frame's key, also when this frame goes into the
    /// same set and overwrites its copy, and on a frame that slides; every
    /// row with no last frame, or when the frame was not answered by row.
    #[test]
    fn the_rows_compare_names_the_rows_that_changed() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut sets = sets(&t);
        let mut s = Screens::new();
        s.compare = Compare::Rows;
        let rows = usize::from(WS.ws_row);
        let changed = |s: &Screens| -> Vec<usize> {
            (0..rows)
                .filter(|&y| s.changed(Row::from_index(y)))
                .collect()
        };
        let every: Vec<usize> = (0..rows).collect();
        feed(&mut t, b"hello\r\nworld");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        assert_eq!(changed(&s), every, "no last frame");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::Last
        );
        assert_eq!(changed(&s), Vec::<usize>::new(), "the same screen");
        feed(&mut t, b"\x1b[2;1HW");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        assert_eq!(changed(&s), vec![1], "one row, into the set it was in");
        assert_eq!(sets[0], copy(&t), "and copied after it was answered");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(1), PLAIN),
            Built::Last
        );
        assert_eq!(changed(&s), Vec::<usize>::new(), "the other set");
        feed(&mut t, b"\x1b[1;1HH");
        let slide = (Leaving::Drawn, &[][..], Held::No);
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), slide),
            Built::New
        );
        assert_eq!(changed(&s), vec![0], "a slide, answered by row");
        s.compare = Compare::Screen;
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(1), PLAIN),
            Built::Last
        );
        assert_eq!(changed(&s), every, "not answered by row");
    }

    /// **A set's copy is brought up to date by the rows written after its
    /// own seal, and by no other**: a cell scribbled into two rows of the
    /// copy behind the key's back, one of the two then written in the
    /// terminal; the written row is copied again and the other is not
    /// read, so its scribble stays.
    #[test]
    fn a_set_copies_the_rows_written_after_its_seal_and_no_other() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut sets = sets(&t);
        let mut s = Screens::new();
        feed(&mut t, b"one\r\ntwo\r\nthree");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        let stride = t.stride().index();
        let scribble = PackedCell::EMPTY;
        sets[0][stride] = scribble;
        sets[0][2 * stride] = scribble;
        feed(&mut t, b"\x1b[2;1HT");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        let now = copy(&t);
        assert_eq!(
            sets[0][stride..2 * stride],
            now[stride..2 * stride],
            "the written row copied"
        );
        assert_eq!(
            sets[0][2 * stride],
            scribble,
            "the row not written not read"
        );
        assert_ne!(now[2 * stride], scribble, "the control: it differs");
    }

    /// **A, a write, B, C on set 0**: A into set 0, a row written, B into
    /// set 1, and C into set 0 again with nothing written. C is the last
    /// frame's screen -- B's, which has the write -- and set 0's copy,
    /// taken under A's seal, is brought up to date with the row.
    #[test]
    fn a_set_taken_again_after_another_copies_what_was_written_between() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut sets = sets(&t);
        let mut s = Screens::new();
        feed(&mut t, b"one\r\ntwo");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::New,
            "A"
        );
        feed(&mut t, b"\x1b[2;1HT");
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(1), PLAIN),
            Built::New,
            "B"
        );
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN),
            Built::Last,
            "C is B's screen"
        );
        assert_eq!(sets[0], copy(&t), "and set 0 holds the row written after A");
    }

    /// **A row drawn in another order at a set's build, then in its own
    /// with nothing written** -- BDSM reset (`CSI 8 l`) -- and every row
    /// of a display held at the build, then not -- word wrap off: none was
    /// stamped, and each such row is copied again, since the copy holds it
    /// as it was drawn then.
    #[test]
    fn a_row_drawn_otherwise_at_the_build_is_copied_again_with_nothing_written() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut sets = sets(&t);
        let mut s = Screens::new();
        feed(&mut t, b"one\r\ntwo\r\nthree");
        let stride = t.stride().index();
        let drawn = (Leaving::None, &[1usize][..], Held::No);
        assert_eq!(
            frame(&mut s, &mut t, &mut sets, SetIx(0), drawn),
            Built::New
        );
        // What the reordered row's copy held, which the term's stored row
        // is not.
        let scribble = PackedCell::EMPTY;
        sets[0][stride] = scribble;
        sets[0][2 * stride] = scribble;
        let _ = frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN);
        let now = copy(&t);
        assert_eq!(
            sets[0][stride..2 * stride],
            now[stride..2 * stride],
            "the row drawn in another order copied"
        );
        assert_eq!(sets[0][2 * stride], scribble, "a row drawn as stored kept");
        let held = (Leaving::None, &[][..], Held::Yes);
        let _ = frame(&mut s, &mut t, &mut sets, SetIx(0), held);
        sets[0][2 * stride] = scribble;
        let _ = frame(&mut s, &mut t, &mut sets, SetIx(0), PLAIN);
        assert_eq!(sets[0], copy(&t), "every row of a display held copied");
    }
}
