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
//! a frame where nothing was written reads no cell. **Which rows the
//! layout drew otherwise is the layout's to say**, the same way: each row
//! of its maps carries the generation that last laid it out
//! (`crate::Orders::laid_after`), and a key keeps the layout's seal and the
//! display it drew (`crate::rebreak`, held by `Rc`) beside the core's. A
//! row laid out again otherwise with none of its cells written is compared
//! cell for cell, since its cells in drawn order may have come out the
//! same; a row neither written nor laid out otherwise -- laid out again as
//! it was among them -- is neither compared nor copied, whatever order it
//! is drawn in.
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
use crate::rebreak::Display;
use crate::{LaidSeal, Leaving, Lending, Orders, Rejoin, Tally, WalkKey};
use baud_core::StatusLine;
use baud_domain::TermRead;
use baud_domain::{
    Cols, DirtyRows, LineRendition, PackedCell, Row, RowGens, Rows, Seal, StatusGen, Stride,
    StyleId, Viewport, WindowAt,
};

use std::rc::Rc;
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

/// **Why a row of a copy may not be the window's now** ([`SetKey::dirt`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Dirt {
    /// Neither written nor laid out again since the key's seals: the copy
    /// holds it, whatever order it is drawn in.
    Kept,
    /// Laid out again otherwise since, its cells not written: drawn from
    /// the same cells in an order or with joins that moved, which may leave
    /// its cells in drawn order as they were, so only a compare of its
    /// cells says whether they changed.
    Laid,
    /// Written since, drawn in another line rendition, or under no key.
    Written,
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
/// layout drew it and its status line; the layout's maps it was drawn
/// through, whose generations say which rows -- the status line's at the
/// row after the window's -- were laid out again and whether it re-broke a
/// display; the pitch of the sets' copies; and whether a smooth scroll's
/// leaving row is drawn.
pub struct Taking<'a, 'v, T> {
    pub term: &'a T,
    pub seal: Seal,
    pub back: Rows,
    pub view: &'a Viewport<'v>,
    pub status: Option<&'a StatusLine<'v>>,
    pub orders: &'a Orders,
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

/// **What a set's copy was last brought up to date under**: the core's
/// seal, the layout's, the window, the scalars, and the screen it is.
#[derive(Clone, Copy, Debug)]
struct Head {
    seal: Seal,
    laid: LaidSeal,
    at: WindowAt,
    scalar: Scalar,
    screen: ScreenGen,
}

/// **One copy's key** -- a set's, or the CPU path's own copy of the window
/// (`Renderer::convert`) -- its head while the copy holds a screen, the
/// display the layout drew it from, held so another display never meets
/// it at a freed address, and the window's line renditions at that build,
/// in a buffer kept across frames so a frame allocates none.
#[derive(Default)]
pub(crate) struct SetKey {
    head: Option<Head>,
    shown: Option<Rc<Display>>,
    renditions: Vec<LineRendition>,
}

/// **This frame, as a key compares it.**
pub(crate) struct Now<'g> {
    gens: RowGens<'g>,
    status: Option<StatusGen>,
    scalar: Scalar,
}

impl<'g> Now<'g> {
    /// The window `back` lines into `term`'s history, drawn as `view` with
    /// `status` below it, copied at `stride`.
    pub(crate) fn of(
        term: &'g impl TermRead,
        back: Rows,
        (view, status): (&Viewport<'_>, Option<&StatusLine<'_>>),
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
    /// `now`**, drawn through `orders`: nothing for no key, another window,
    /// other scalars or another display; all of it when nothing was written
    /// since its seal and no row laid out again since its layout's; else
    /// the rows [`SetKey::dirt`] does not name.
    pub(crate) fn keeps(&self, term: &impl TermRead, now: &Now<'_>, orders: &Orders) -> Keeps {
        let Some(h) = &self.head else {
            return Keeps::Nothing;
        };
        if h.at != now.gens.window()
            || h.scalar != now.scalar
            || !crate::bidi::same_shown(self.shown.as_ref(), orders.display_rc())
        {
            return Keeps::Nothing;
        }
        match term.wrote_since(&h.seal) || orders.moved_since(&h.laid) {
            false => Keeps::All,
            true => Keeps::Rows,
        }
    }

    /// **Why row `r` of the copy may not be the window's now**, drawn
    /// through `orders`: written after the key's seal, or in another line
    /// rendition; else laid out again after its layout's seal; else kept.
    /// Row `rows` is the status line's, written when its own generation
    /// says so, and laid out again when the maps' row after the window's
    /// was.
    pub(crate) fn dirt(&self, now: &Now<'_>, orders: &Orders, view: &Viewport<'_>, r: Row) -> Dirt {
        let Some(h) = &self.head else {
            return Dirt::Written;
        };
        let written = match r.index() == view.rows().index() {
            true => now.status.is_none_or(|g| g.after(&h.seal)),
            false => {
                now.gens.after(r, &h.seal)
                    || self.renditions.get(r.index()) != Some(&view.rendition(r))
            }
        };
        match (written, orders.laid_after(r, &h.laid)) {
            (true, _) => Dirt::Written,
            (false, true) => Dirt::Laid,
            (false, false) => Dirt::Kept,
        }
    }

    /// **Whether row `r` of the copy may not be the window's now with none
    /// of its cells written**, drawn through `orders`: laid out again
    /// after the key's layout's seal, or a window row in another line
    /// rendition; every row with no key. Row `rows` is the status line's.
    pub(crate) fn relaid(&self, orders: &Orders, view: &Viewport<'_>, r: Row) -> bool {
        let Some(h) = &self.head else {
            return true;
        };
        (r.index() < view.rows().index()
            && self.renditions.get(r.index()) != Some(&view.rendition(r)))
            || orders.laid_after(r, &h.laid)
    }

    /// **This key, for a copy brought up to date now**: under `seal`, the
    /// term's `Term::sealed` as the copy was read, and the seal of
    /// `orders`, the maps it was drawn through, holding `screen`.
    pub(crate) fn record(
        &mut self,
        (seal, screen): (Seal, ScreenGen),
        now: &Now<'_>,
        view: &Viewport<'_>,
        orders: &Orders,
    ) {
        self.head = Some(Head {
            seal,
            laid: orders.laid_seal(),
            at: now.gens.window(),
            scalar: now.scalar,
            screen,
        });
        self.shown = orders.display_rc().cloned();
        self.renditions.clear();
        self.renditions
            .extend((0..view.rows().index()).map(|y| view.rendition(Row::from_index(y))));
    }

    /// The copy no longer holds a screen.
    pub(crate) fn forget(&mut self) {
        self.head = None;
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
    /// **The rows of the last frame that may differ from the frame built
    /// before it**, the status line's after the window's, under either
    /// compare ([`Screens::differ`]).
    differ: Option<DirtyRows>,
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
            scratch: Vec::new(),
            walked: None,
            compare: Compare::Screen,
            changed: Vec::new(),
            differ: None,
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
        w: &Taking<'_, '_, impl TermRead>,
    ) -> Built {
        self.take_lent(copies, set, w, &[])
    }

    /// **[`Screens::take`], the last frame's copy read through `undo`**
    /// where a row of it is compared -- a row the layout laid out again with
    /// none of its cells written: the cells a lending rewrote in it, by row
    /// of the copy and column, with the id each held
    /// (`baud_render::Lent::rewrites`), so a copy lent a struck link's
    /// slots compares as the screen it was copied from. Its own set's,
    /// which no frame in flight reads, is put back before
    /// (`Lent::restore`), and so names none.
    pub fn take_lent(
        &mut self,
        copies: &mut [&mut [PackedCell]],
        set: SetIx,
        w: &Taking<'_, '_, impl TermRead>,
        undo: &[(usize, usize, StyleId)],
    ) -> Built {
        let view = w.view;
        let (cols, rows, stride) = (view.cols().index(), view.rows().index(), w.stride.index());
        let now = Now::of(w.term, w.back, (view, w.status), w.stride);
        let lined = usize::from(w.status.is_some());
        // **The last frame's answer, before this frame's copy is touched.**
        self.changed.clear();
        self.differ = None;
        let mut same = true;
        let last = self
            .last
            .and_then(|l| Some((self.keys.get(l)?, copies.get(l)?)));
        match last.map(|(k, c)| (k, c, k.keeps(w.term, &now, w.orders))) {
            None | Some((_, _, Keeps::Nothing)) => {
                same = false;
                self.changed.resize(rows, RowChange::Changed);
            }
            Some((_, _, Keeps::All)) => {
                self.changed.resize(rows, RowChange::Same);
                self.differ = Some(DirtyRows::NONE);
            }
            Some((k, copy, Keeps::Rows)) => {
                let scratch = &mut self.scratch;
                let mut differs = |y: usize| {
                    match k.dirt(&now, w.orders, view, Row::from_index(y)) {
                        Dirt::Kept => return false,
                        Dirt::Written => return true,
                        Dirt::Laid => {}
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
                let mut differ = Some(DirtyRows::NONE);
                let mut at = |y: usize, d: bool| {
                    if d {
                        differ = differ.and_then(|n| n.with(Row::from_index(y)));
                    }
                };
                for y in 0..rows {
                    let d = differs(y);
                    same &= !d;
                    at(y, d);
                    self.changed.push(match d {
                        true => RowChange::Changed,
                        false => RowChange::Same,
                    });
                }
                if lined == 1 && differs(rows) {
                    same = false;
                    at(rows, true);
                }
                self.differ = differ;
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
        // drawn in another order among them. A row is copied when it was
        // written or laid out again since that set's seals.
        let holds = built == Built::Last && self.holds(set) == Some(self.now);
        if let (Some(copy), Some(k)) = (copies.get_mut(set.0), self.keys.get(set.0)) {
            let keeps = match (w.leaving, holds) {
                (Leaving::Drawn, _) => Keeps::Nothing,
                (Leaving::None, true) => Keeps::All,
                (Leaving::None, false) => k.keeps(w.term, &now, w.orders),
            };
            let (mut copied, mut taken) = (0, 0);
            for y in 0..rows + lined {
                let kept = match keeps {
                    Keeps::All => true,
                    Keeps::Rows => k.dirt(&now, w.orders, view, Row::from_index(y)) == Dirt::Kept,
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
                taken += 1;
            }
            add(Counter::CellsCopied, Site::Screens, Tally::of(copied));
            add(Counter::RowsTaken, Site::Screens, Tally::of(taken));
        }
        if built == Built::New {
            self.now = ScreenGen::fresh();
        }
        if let Some(k) = self.keys.get_mut(set.0) {
            k.record((w.seal, self.now), &now, view, w.orders);
        }
        self.last = Some(set.0);
        built
    }

    /// **The rows of the last frame that may differ from the frame built
    /// before it**, the window's and the status line's after them, under
    /// either compare: none for a frame that is the last frame's screen,
    /// and `None`, every row, where there was no frame before or the two
    /// are not compared row by row.
    pub(crate) fn differ(&self) -> Option<DirtyRows> {
        self.differ
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
    use super::{Built, Compare, Screens, SetIx, Taking};
    use crate::{DrawnRows, Leaving, Reorder};
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

    /// **A frame taken as a driver takes it**: one generation sealed, the
    /// live window laid out by `o`, then the window in the order `o` draws
    /// it and its status line taken into `set`, sliding or not.
    fn frame(
        s: &mut Screens,
        t: &mut Term<'_>,
        o: &mut Reorder,
        sets: &mut [Vec<PackedCell>; 2],
        set: SetIx,
        slide: Leaving,
    ) -> Built {
        let seal = t.seal_writes();
        let t = &*t;
        o.lay_out(t, Rows::ZERO);
        let [a, b] = sets;
        let mut copies: [&mut [PackedCell]; 2] = [a, b];
        let mut drawn = DrawnRows::default();
        let view = o.orders().window_kept(t, Rows::ZERO, &mut drawn);
        let status = t.status_line();
        s.take(
            &mut copies,
            set,
            &Taking {
                term: t,
                seal,
                back: Rows::ZERO,
                view: &view,
                status: status.as_ref(),
                orders: o.orders(),
                stride: t.stride(),
                leaving: slide,
            },
        )
    }

    const PLAIN: Leaving = Leaving::None;

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
        let mut o = Reorder::new();
        let mut step = |t: &mut Term<'_>, bytes: &[u8]| {
            feed(t, bytes);
            frame(&mut s, t, &mut o, &mut sets, SetIx(0), PLAIN)
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
        let mut o = Reorder::new();
        feed(&mut t, b"hello");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        assert_eq!(sets[0], copy(&t), "set 0 holds it");
        let first = s.now();
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(1), PLAIN),
            Built::Last,
            "unchanged"
        );
        assert_eq!(s.now(), first, "the same screen");
        assert_eq!(sets[1], copy(&t), "set 1 holds it too");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::Last
        );
        // A cell changed behind the key's back in set 1, which holds the
        // screen: nothing was written since its key's seal, so the next
        // frame into it reads no cell and copies none, and the cell stays.
        sets[1][1] = PackedCell::EMPTY;
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(1), PLAIN),
            Built::Last
        );
        assert_ne!(sets[1], copy(&t), "set 1 was not copied again");
        sets[1] = copy(&t);
        feed(&mut t, b" world");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::New,
            "a cell written"
        );
        assert_ne!(s.now(), first, "a new screen");
        assert_eq!(sets[0], copy(&t), "copied into the set it goes to");
        let second = s.now();
        let slide = Leaving::Drawn;
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(1), slide),
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
        let mut o = Reorder::new();
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
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        assert_eq!(changed(&s), every, "no last frame");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::Last
        );
        assert_eq!(changed(&s), Vec::<usize>::new(), "the same screen");
        feed(&mut t, b"\x1b[2;1HW");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        assert_eq!(changed(&s), vec![1], "one row, into the set it was in");
        assert_eq!(sets[0], copy(&t), "and copied after it was answered");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(1), PLAIN),
            Built::Last
        );
        assert_eq!(changed(&s), Vec::<usize>::new(), "the other set");
        feed(&mut t, b"\x1b[1;1HH");
        let slide = Leaving::Drawn;
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), slide),
            Built::New
        );
        assert_eq!(changed(&s), vec![0], "a slide, answered by row");
        s.compare = Compare::Screen;
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(1), PLAIN),
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
        let mut o = Reorder::new();
        feed(&mut t, b"one\r\ntwo\r\nthree");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::New
        );
        let stride = t.stride().index();
        let scribble = PackedCell::EMPTY;
        sets[0][stride] = scribble;
        sets[0][2 * stride] = scribble;
        feed(&mut t, b"\x1b[2;1HT");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
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
        let mut o = Reorder::new();
        feed(&mut t, b"one\r\ntwo");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::New,
            "A"
        );
        feed(&mut t, b"\x1b[2;1HT");
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(1), PLAIN),
            Built::New,
            "B"
        );
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::Last,
            "C is B's screen"
        );
        assert_eq!(sets[0], copy(&t), "and set 0 holds the row written after A");
    }

    /// **A row another row of its paragraph orders is copied again when
    /// that row is written, with none of its own cells written, and a row of
    /// no paragraph the write reached is not read**: row 0, digits, drawn
    /// right to left by row 1's Hebrew, the soft wrap's paragraph (UAX #9
    /// P2-P3); row 1 written Latin, which lays row 0 out again in its own
    /// order. A cell scribbled into rows 0 and 3 of the copy behind the
    /// key's back: row 0 is copied again, row 3's scribble stays.
    #[test]
    fn a_row_laid_out_again_is_copied_again_with_none_of_its_cells_written() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut sets = sets(&t);
        let mut s = Screens::new();
        let mut o = Reorder::new();
        feed(
            &mut t,
            "1234567890 123456789\u{5d0}\u{5d1}\u{5d2}\r\n\r\nthree".as_bytes(),
        );
        let stride = t.stride().index();
        let _ = frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN);
        assert!(
            o.orders().reordered(Row::ZERO),
            "the control: row 0 is drawn in another order"
        );
        let scribble = PackedCell::EMPTY;
        sets[0][0] = scribble;
        sets[0][3 * stride] = scribble;
        let seal = t.seal_writes();
        feed(&mut t, b"\x1b[2;1Habc");
        assert!(
            !t.row_gens(Rows::ZERO).after(Row::ZERO, &seal),
            "the isolation: row 0 not written"
        );
        let _ = frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN);
        assert!(
            !o.orders().reordered(Row::ZERO),
            "the control: row 0 is in its own order now"
        );
        let now = copy(&t);
        assert_eq!(
            sets[0][..stride],
            now[..stride],
            "the row laid out again copied"
        );
        assert_eq!(
            sets[0][3 * stride],
            scribble,
            "a row not laid out again kept"
        );
    }

    /// **A row laid out again with none of its cells written is compared at
    /// the copy, and is the one row compared** -- the count every caret
    /// frame's zero at `Site::Screens` stands beside, seen to move: row 0,
    /// digits, drawn right to left by row 1's Hebrew, the soft wrap's
    /// paragraph (UAX #9 P2-P3); row 1 written Latin, which lays row 0 out
    /// again in its own order. Against the last frame's key the take asks
    /// each row why it may differ (`SetKey::dirt`): row 0 is laid out again
    /// and not written, so its 20 cells are compared with the last copy's
    /// (`row_same`); row 1 is written, so it is copied with no compare; no
    /// other row is either. So the frame compares 20 cells and copies 40,
    /// rows 0 and 1, and the frame after it, nothing written, compares and
    /// copies none. Named break: the compare's count in `row_same` removed.
    #[test]
    fn a_row_laid_out_again_unwritten_is_the_row_compared_at_the_copy() {
        use crate::counters::{self, Counter, Site};
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut sets = sets(&t);
        let mut s = Screens::new();
        let mut o = Reorder::new();
        feed(
            &mut t,
            "1234567890 123456789\u{5d0}\u{5d1}\u{5d2}\r\n\r\nthree".as_bytes(),
        );
        let _ = frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN);
        assert!(
            o.orders().reordered(Row::ZERO),
            "the control: row 0 is drawn in another order"
        );
        let seal = t.seal_writes();
        feed(&mut t, b"\x1b[2;1Habc");
        let gens = t.row_gens(Rows::ZERO);
        assert!(
            !gens.after(Row::ZERO, &seal) && gens.after(Row::from_index(1), &seal),
            "the isolation: row 1 written, row 0 not"
        );
        let _ = counters::take();
        let built = frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN);
        let laid = counters::take();
        assert_eq!(built, Built::New, "the control: a new screen");
        assert!(
            !o.orders().reordered(Row::ZERO),
            "the control: row 0 is laid out again, in its own order"
        );
        let cols = u64::from(WS.ws_col);
        let at = |c: &counters::Counts, n: Counter| c.at(Site::Screens, n).value();
        assert_eq!(
            at(&laid, Counter::CellsCompared),
            cols,
            "row 0, laid out again, compared whole, and no other row"
        );
        assert_eq!(
            at(&laid, Counter::CellsCopied),
            2 * cols,
            "rows 0 and 1 copied"
        );
        assert_eq!(
            frame(&mut s, &mut t, &mut o, &mut sets, SetIx(0), PLAIN),
            Built::Last,
            "the control: the frame after is the last screen"
        );
        let still = counters::take();
        assert_eq!(
            (
                at(&still, Counter::CellsCompared),
                at(&still, Counter::CellsCopied)
            ),
            (0, 0),
            "the frame after, nothing written, compares and copies none"
        );
    }

    /// **A row of the last copy a lending rewrote is compared again with
    /// the rewrite undone, and both compares are counted** at
    /// `Site::Screens` (`row_same`): a row of four cells whose cell 1 a
    /// lending gave another rendition, read through an undo that names its
    /// own. The first compare differs, so the row is compared again in the
    /// scratch with the undo applied, and is the screen's: 8 cells
    /// counted, the row's 4 twice. The control: with the undo on another
    /// row, the row differs after the one compare, 4 cells. Named breaks:
    /// either of `row_same`'s two counts removed.
    #[test]
    fn a_row_a_lending_rewrote_is_compared_again_through_its_undo() {
        use super::row_same;
        use crate::counters::{self, Counter, Site};
        use baud_domain::StyleId;
        let now = [PackedCell::EMPTY; 4];
        let mut kept = now;
        kept[1].style = StyleId::from_index(7);
        assert_ne!(kept, now, "the control: the lending rewrote cell 1");
        let compared = || {
            counters::take()
                .at(Site::Screens, Counter::CellsCompared)
                .value()
        };
        let mut scratch = Vec::new();
        let _ = counters::take();
        let undone = row_same(
            &mut scratch,
            &[(0, 1, now[1].style)],
            (Some(&kept[..]), Some(&now[..])),
            Row::ZERO,
        );
        assert!(undone, "the row, its rewrite undone, is the screen's");
        assert_eq!(compared(), 8, "compared as copied and again undone");
        let elsewhere = row_same(
            &mut scratch,
            &[(1, 1, now[1].style)],
            (Some(&kept[..]), Some(&now[..])),
            Row::ZERO,
        );
        assert!(!elsewhere, "the control: no undo on its row, it differs");
        assert_eq!(compared(), 4, "the control: compared once");
    }
}
