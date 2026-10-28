//! **What the app keeps over the screen, across a read**: where the live
//! ring stood before it, so a selection's lines are renumbered by what the
//! read moved and dropped by what it wrote over, and what a switch, a reset
//! or an erased history did. Pure, over a real `Term`, so the rules are
//! one for both drivers and a test reaches them; moved here from `app.rs`
//! when the Linux driver took a selection.

use baud_core::{HistoryMark, Resource, Screen, SurfaceLine, Term};
use baud_domain::{Col, Cols, Row, RowRot, Rows};
use baud_render::{AbsLine, Clip, Grain, Highlight, Point, Scrolled, Selection, Span};

pub(crate) use baud_core::Moved as Ring;

/// Where the live ring stood before a read or a resize, for the
/// selection's line count and a slide, and the counts a read is judged by.
/// The window over the history is the core's (note V, V2), so nothing here
/// anchors it.
#[derive(Clone, Copy)]
pub(crate) struct Before {
    pub(crate) live: RowRot,
    pub(crate) epoch: baud_core::ScreenEpoch,
    /// Read by the macOS pump's slide and both drivers' reset reveal.
    #[cfg_attr(
        target_os = "linux",
        expect(dead_code, reason = "the Linux driver draws no slide")
    )]
    pub(crate) region: baud_core::RegionScrolls,
    pub(crate) erased: baud_core::HistoryErasures,
    pub(crate) resets: baud_core::Resets,
}

impl Before {
    pub(crate) fn of(term: &Term<'_>) -> Before {
        Before {
            live: term.rotation(),
            epoch: term.screen_epoch(),
            region: term.region_scrolls(),
            erased: term.history_erasures(),
            resets: term.resets(),
        }
    }

    /// The readings a pump takes before it feeds, with the core's watch on
    /// the history's erasures armed at them, so the first erasure the feed
    /// makes is kept for `app.rs`'s `restart` whatever erases after it -- and its
    /// watch on the lines of the live surface `selection` names, numbered by
    /// `scrolled`, so [`stale_in_moved_rows`] learns whether a move in place,
    /// a write, an erase or a line taken back reached one, and [`numbered`]
    /// how far the read moved the surface.
    pub(crate) fn watched(
        term: &mut Term<'_>,
        selection: Option<&Selection>,
        scrolled: Scrolled,
    ) -> Before {
        term.watch_erasures();
        let lines = selection.and_then(|s| watched_lines(s, term, scrolled));
        term.watch_lines(lines);
        Before::of(term)
    }
}

/// The first and last lines of the live surface `sel` names, numbered by
/// `scrolled` -- rows of the screen, or lines of the history the ring still
/// holds -- or `None` when it names none the ring holds. A selection in the
/// history is watched too: a scroll down takes the newest saved line back
/// and blanks it, and a read that retires it again nets no movement, so
/// nothing at the read's two ends says its text went.
pub(crate) fn watched_lines(
    sel: &Selection,
    term: &Term<'_>,
    scrolled: Scrolled,
) -> Option<(SurfaceLine, SurfaceLine)> {
    let rows = term.geometry().1;
    let (start, end) = sel.ordered();
    let first = start.line.max(oldest(term, scrolled));
    let bottom = Row::from_index(rows.index().checked_sub(1)?);
    let last = end.line.min(scrolled.line(Rows::ZERO, bottom));
    if first > last {
        return None;
    }
    Some((
        scrolled.surface_line(first, rows)?,
        scrolled.surface_line(last, rows)?,
    ))
}

/// Whether a read wrote or erased on a row of the live screen the selection
/// names -- text, a mark, `EL`, `ED`, `ECH`, `ICH`, `DCH`, the selective
/// erases, `DECIC`, `DECDC`, `DECALN` -- whatever it left there, as xterm
/// disowns a selection a write or an erase lands on. A copy reads the cells
/// as they are at `Cmd-C`, so a selection kept there would copy what
/// replaced its text. Or whether a read moved, in place, rows the selection
/// names: an `IL` or `DL`, or a region scroll that copies its rows -- a
/// region in the middle of the screen, a short top region on the alternate
/// screen, `SD` and `RI` in any partial region, and every vertical scroll
/// under side margins, the whole screen included, which moves only the
/// rectangle. Rows are tested and columns are not, so a selection beside
/// the rectangle on its rows is stale too, though its text did not move. No
/// ring moves, so the selection's lines still name the same screen rows
/// while the text on them has moved, and it would copy what took their
/// place. Or whether a region anchored at the top scrolled over rows of the
/// selection under it: the ring moved and the text under the region did
/// not, so the count would renumber the selection onto the row below's
/// text. Or whether a line taken back erased a line of the selection, on
/// the screen or in the history: the whole screen's scroll down shows the
/// newest saved line on the top row and blanks it, and gives up the last
/// row to a line feed that brings it back blank, and a redraw mark erases
/// the block it takes back, so a read that retires as many lines again nets
/// none taken back and nothing at its two ends says the text went. The core
/// judges each write and erase by the rows it lands on, and each move by
/// its own rows -- the region's, from the cursor to the region's bottom for
/// `IL` and `DL`, the rows under a top region, the rows a line taken back
/// erases -- against the lines [`Before::watched`] armed it on, numbered
/// through every line the read retired or took back before it, as the core
/// counts them. Not by the region the read ends in, nor by its net
/// movement: a read can scroll in one region and then set another, or set
/// it back, and take lines back and retire them again. So the answer is the
/// one the same bytes give fed in any number of reads, and a selection the
/// watch did not reach holds its text however far the read moved the
/// surface, and is renumbered by [`numbered`] -- back as well as on. Until
/// 2026-10-17 the pump dropped a selection on a read that netted lines
/// taken back, and on rows under the region a read ended in, both judged at
/// the read's two ends, and the watch was reached by a read past half the
/// ring whatever it moved.
pub(crate) fn stale_in_moved_rows(term: &Term<'_>) -> bool {
    term.watched_rows_changed()
}

/// **The text a selection held, kept past its highlight while `?1040` is
/// set**, xterm's keepSelection: "keep the selection even after the
/// selected area was touched by some output to the terminal"
/// (`[[source: xterm-man]]`). xterm saves the text when the selection is
/// made (`SaltTextAway`, `[[source: xterm-button]]`) and never again for
/// that selection: output that rewrites its cells takes the highlight away
/// and leaves the text to paste (`ScrnDisownSelection`,
/// `[[source: xterm-screen]]`), a scroll that trims it moves only its ends
/// (`[[source: xterm-scroll]]`, `adjustHiliteOnFwdScroll`) and one
/// scrolled past the saved lines is disowned, which takes the highlight
/// alone, and a click that makes no selection takes the highlight at the
/// press and disowns nothing at the release (`StartSelect`, then `SelectSet` on a null
/// selection, whose `ScrnDisownSelection` finds no highlight). Here the
/// text is taken before the first read after the selection is made -- no
/// output reaches the cells between -- and kept while the selection is
/// carried; a copy takes it while the selection is up and after its
/// highlight went. Nothing is taken while `?1040` is reset.
#[derive(Default)]
pub(crate) struct KeptText {
    taken: Taken,
    /// The display's wrap the window is drawn with, which a copy under
    /// `?96` reads its lines by ([`Selection::text_as`]).
    wrap: baud_render::WordWrap,
}

/// What [`KeptText`] holds.
#[derive(Default)]
enum Taken {
    #[default]
    Nothing,
    /// A selection a gesture made, with no read since: its cells are the
    /// ones it was made over, so its text is taken when it is first asked.
    Made(Selection),
    /// The text of the selection up now, taken when it was made.
    Of(Selection, String),
    /// The text of a selection whose highlight went: output wrote over
    /// it, or a click took it away.
    Unhighlighted(String),
}

impl KeptText {
    /// **Before a read**: the text of `sel`, numbered by `scrolled`, taken
    /// when it is not the selection taken last; nothing while `?1040` is
    /// reset. A press that has not moved is not a selection yet and takes
    /// nothing. A selection gone with neither a read's output on it nor a
    /// click -- a switch, a reset, a resize -- leaves nothing kept.
    pub(crate) fn take(&mut self, sel: Option<&Selection>, term: &Term<'_>, scrolled: Scrolled) {
        let wrap = self.wrap;
        if !term.resource(Resource::KeepSelection) {
            self.taken = Taken::Nothing;
            return;
        }
        match (sel, &self.taken) {
            (Some(s), _) if s.is_click() => {}
            (Some(s), Taken::Of(from, _)) if from == s => {}
            (Some(s), _) => self.taken = Taken::Of(*s, s.text_as(term, scrolled, wrap)),
            (None, Taken::Of(..) | Taken::Made(_)) => self.taken = Taken::Nothing,
            (None, _) => {}
        }
    }

    /// **A gesture changed the selection to `sel`**: made, moved,
    /// dismissed or replaced, with no read between. A selection made is the
    /// one whose text is kept from now on; a press, or a click that made
    /// none, takes the highlight and leaves the text of the selection it
    /// took to copy, as xterm's does (`StartSelect`, `SelectSet`), while
    /// `?1040` is set.
    pub(crate) fn gestured(
        &mut self,
        sel: Option<&Selection>,
        term: &Term<'_>,
        scrolled: Scrolled,
    ) {
        if !term.resource(Resource::KeepSelection) {
            self.taken = Taken::Nothing;
            return;
        }
        self.taken = match (sel, core::mem::take(&mut self.taken)) {
            (Some(s), _) if !s.is_click() => Taken::Made(*s),
            (_, Taken::Of(_, text)) => Taken::Unhighlighted(text),
            (_, Taken::Made(was)) => Taken::Unhighlighted(was.text_as(term, scrolled, self.wrap)),
            (_, kept) => kept,
        };
    }

    /// **A resize changed the selection**: what was kept is not the newer
    /// selection's, so it goes, and the next read takes the newer one's
    /// text. The Linux seat's; on macOS a resize pumps, and the pump takes
    /// the text.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn changed(&mut self) {
        self.taken = Taken::Nothing;
    }

    /// **A read's output took the selection's highlight away**, or the
    /// end of the history clipped it away: while `?1040` is set its text
    /// stays to copy, and reset it goes.
    pub(crate) fn dropped(&mut self, term: &Term<'_>) {
        let taken = core::mem::take(&mut self.taken);
        if let (true, Taken::Of(_, text)) = (term.resource(Resource::KeepSelection), taken) {
            self.taken = Taken::Unhighlighted(text);
        }
    }

    /// **A read carried the selection with its text** (`followed`), maybe
    /// trimming it: the text kept is still the one taken when it was made,
    /// as xterm moves only the ends of a selection it truncates.
    pub(crate) fn carried(&mut self, sel: Option<&Selection>) {
        if let (Some(s), Taken::Of(from, _)) = (sel, &mut self.taken) {
            *from = *s;
        }
    }

    /// **The display's wrap the window is drawn with** (`wrap_words`):
    /// the text taken after this reads a copy under `?96` by it.
    pub(crate) fn set_word_wrap(&mut self, wrap: baud_render::WordWrap) {
        self.wrap = wrap;
    }

    /// The display's wrap a copy reads by ([`KeptText::set_word_wrap`]),
    /// for the Linux seat's copy; the macOS driver asks its renderer.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn word_wrap(&self) -> baud_render::WordWrap {
        self.wrap
    }

    /// The text a copy takes: with `sel` up, the text taken when it was
    /// made, if that is held; with none, what lost its highlight while
    /// `?1040` kept it.
    pub(crate) fn text(&self, sel: Option<&Selection>) -> Option<&str> {
        match (sel, &self.taken) {
            (Some(s), Taken::Of(from, t)) if from == s => Some(t),
            (None, Taken::Unhighlighted(t)) if !t.is_empty() => Some(t),
            _ => None,
        }
    }
}

/// **Whether a read scrolled the selection off the top while `?1040` is
/// reset**: the lines the primary retired took its first line above the
/// screen while its last was on it, where xterm's reset disowns the
/// selection and its set moves it into the saved lines
/// (`[[source: xterm-scroll]]`, `adjustHiliteOnFwdScroll`, 451-456:
/// `endH.row >= top_marg && startH.row - amount < top_marg`). `top_was`
/// and `top_now` are the live screen's top line before the read and after
/// it, numbered as the selection is. A region's scroll is the core's to
/// judge (`Term::watched_rows_changed`).
pub(crate) fn scrolled_off_the_top(
    term: &Term<'_>,
    sel: &Selection,
    (top_was, top_now): (AbsLine, AbsLine),
) -> bool {
    let (start, end) = sel.ordered();
    !term.resource(Resource::KeepSelection)
        && term.screen() == Screen::Normal
        && top_now > top_was
        && end.line >= top_was
        && start.line < top_now
}

/// Whether a read switched surfaces -- by the core's epoch, not by the
/// screen at its two ends: a read that left the alternate screen and
/// entered it again ends where it began, on a surface `?1049` cleared or
/// `?47` showed as it was left some time before, and a selection or a slide
/// from before names rows it cannot vouch for.
pub(crate) fn switched(term: &Term<'_>, before: Before) -> bool {
    term.screen_epoch() != before.epoch
}

/// The oldest line a selection may name: as far behind the live screen's
/// top as the live surface keeps -- the primary's history, and nothing on
/// the alternate screen, where the history count is still the primary's.
pub(crate) fn oldest(term: &Term<'_>, scrolled: Scrolled) -> AbsLine {
    let retained = Rows::of_count(term.scrollback().len());
    scrolled.oldest(term.looking_back(retained))
}

/// How the selection's numbering moves across a read: by the core's count
/// of the lines the live surface retired or took back since the watch was
/// armed on the selection (`Term::watched_moved`), which is exact however
/// far the read went -- where the rotation is read round the ring, so past
/// half of it lines retired read as lines taken back, and a ring and a line
/// as one line. With no watch armed, or one a line it names reached, the
/// rotation, which numbers nothing kept. Nothing across a switch or a
/// resize, after which no selection is kept.
pub(crate) fn numbered(
    term: &Term<'_>,
    before: Before,
    resized: Option<crate::width::Grid>,
) -> Ring {
    match (resized, switched(term, before), term.watched_moved()) {
        (Some(_), _, _) | (None, true, _) => Ring::Retired(Rows::ZERO),
        (None, false, Some(m)) => m,
        (None, false, None) => term.moved_since(before.live),
    }
}

/// **The selection carried with its text across a read**, the count that
/// numbers it already moved to `scrolled`: a region's scroll moves it with
/// its rows and trims what it takes out of the region, a scroll down trims
/// what it pushes under the screen, the alternate screen's top trims what
/// leaves it, the rows under a top region keep theirs, and `IL` and `DL`
/// move it below the cursor (`Term::watched_lines`, note V). Unchanged by
/// a read that moved no rows in place; the caller has dropped one the
/// watch reached.
pub(crate) fn followed(term: &Term<'_>, selection: &mut Option<Selection>, scrolled: Scrolled) {
    if let (Some(sel), Some(w)) = (selection.as_mut(), term.watched_lines()) {
        let last = Col::at_index(term.geometry().0.index().saturating_sub(1));
        sel.follow(scrolled, w, last);
    }
}

/// **What a window resize leaves of the selection, and how the count that
/// numbers it moves** (note V, V3): on the primary, across a change of
/// height alone, the selection is kept on its lines and the count moves by
/// the rows the resize retired into the history or took back from it, as a
/// read's would -- the core counts them by the window's top line
/// (`Term::history_moved_since`), however many. A change of columns
/// re-cuts the lines, so the columns a selection names no longer name its
/// cells and it goes; on the alternate screen the program redraws for the
/// new size and the screen has no history to number by, so it goes; and it
/// goes where it reaches a row the shrink cut below the screen, which may
/// come back or be blanked when history is pulled instead. A line the ring
/// recycled is clipped, as after a read. Until 2026-10-20 every change of
/// grid cleared it. `mark`, `cols` and `screen` are read before the
/// resize.
pub(crate) fn kept_across_resize(
    term: &Term<'_>,
    (mark, cols, screen): (HistoryMark, Cols, Screen),
    selection: Option<Selection>,
    scrolled: Scrolled,
) -> (Option<Selection>, Scrolled) {
    let (now_cols, rows) = term.geometry();
    if now_cols != cols || screen != Screen::Normal || term.screen() != Screen::Normal {
        return (None, scrolled);
    }
    let scrolled = match term.history_moved_since(mark) {
        Ring::Retired(n) => scrolled.advanced(n),
        Ring::Restored(n) => match scrolled.retreated(n) {
            Some(s) => s,
            None => return (None, Scrolled::START),
        },
    };
    let Some(mut sel) = selection else {
        return (None, scrolled);
    };
    let bottom = Row::from_index(rows.index().saturating_sub(1));
    if sel.ordered().1.line > scrolled.line(Rows::ZERO, bottom) {
        return (None, scrolled);
    }
    match sel.clip(oldest(term, scrolled)) {
        Clip::Kept => (Some(sel), scrolled),
        Clip::Cleared => (None, scrolled),
    }
}

/// **The lines past the window `back` lines up that the display may draw
/// on its own** under the display's wrap `wrap` (`baud_render::Beyond`),
/// which a selection's highlight and find's marks reach: none with the
/// wrap off, which draws every row where it is stored.
pub(crate) fn beyond(
    term: &Term<'_>,
    back: Rows,
    wrap: baud_render::WordWrap,
) -> baud_render::Beyond {
    match wrap {
        baud_render::WordWrap::On => baud_render::Beyond::of(term, back),
        baud_render::WordWrap::Off => baud_render::Beyond::NONE,
    }
}

/// **`?1001`'s region, through the selection's highlight**, on the view
/// `back` lines up: `[[source: ctlseqs]]`'s highlight tracking "highlights
/// the region covered by the mouse within that range until button
/// release", and the core says what that region is (`Term::highlight`).
/// Its cells are the live screen's, numbered as a selection's are, and the
/// region is a stream from the earlier of its two ends to the later -- the
/// core keeps them in the order the program and the mouse gave them. `None`
/// when nothing is tracked or none of it is on this view. Drawn in place of
/// the selection while it lasts and never stored as one, so Cmd-C still
/// copies the user's selection and the user's selection is back after the
/// release.
pub(crate) fn tracked_highlight(
    term: &Term<'_>,
    scrolled: Scrolled,
    back: Rows,
) -> Option<Highlight> {
    let h = term.highlight()?;
    let point = |c: baud_core::CellAt| Point::new(scrolled.line(Rows::ZERO, c.row), c.col);
    let (a, b) = (point(h.start), point(h.end));
    let span = Span {
        start: a.min(b),
        end: a.max(b),
    };
    let view = term.view(back);
    Selection::at(span, Grain::Cell).highlight(scrolled.top(back), view.rows(), view.cols())
}

/// Whether a read reset the terminal -- `RIS`, from either screen -- by the
/// core's count, because nothing else across a read says so: a reset scrolls
/// nothing and keeps the rotation, so on a terminal with no history both ends
/// of the read look alike, and [`erased`] moves on `CSI 3 J` too, which keeps
/// the screen and a selection on it (note V, V1).
pub(crate) fn reset_in(term: &Term<'_>, before: Before) -> bool {
    term.resets() != before.resets
}

/// A key's or a paste's snap: the primary's window to the live screen, where
/// the key's effect is, and whether that moved it. On the alternate screen
/// the live screen is already what is drawn, and the primary's window is
/// kept for when the program leaves, as foot keeps a view per grid and
/// resets only the live one on a key (`docs/research/notes/l-scrollbar.md`).
/// The policy is the app's and the window the core's (note V, V2).
pub(crate) fn snap_live(term: &mut Term<'_>) -> bool {
    let moved = term.screen() == Screen::Normal && term.window_back().index() != 0;
    if moved {
        term.window_live();
    }
    moved
}

/// `?1001`'s region on screen: the core's handshake drawn through the
/// selection's highlight (`tracked_highlight`).
#[cfg(test)]
mod highlight_tests {
    use super::tracked_highlight;
    use baud_core::{CellAt, MouseButton, MouseEvent, MouseKind, MouseMods, Owned, Term};
    use baud_domain::{BytePos, Col, ReadLen, Row, Rows, WinSize};
    use baud_render::{Highlight, Region, Scrolled};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 10,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// A cell by column and row, zero-based.
    const fn at(col: Col, row: Row) -> CellAt {
        CellAt { col, row }
    }

    fn left(t: &mut Term<'_>, kind: MouseKind, at: CellAt) {
        t.mouse_report(MouseEvent {
            button: MouseButton::Left,
            mods: MouseMods::NONE,
            kind,
            col: at.col,
            row: at.row,
            px: None,
        });
        t.clear_reply();
    }

    /// The stream highlight from one cell to another, as the view draws it.
    fn drawn(from: CellAt, to: CellAt) -> Option<Highlight> {
        Some(Highlight::new(
            (from.row, from.col),
            (to.row, to.col),
            Region::Stream,
        ))
    }

    /// **From the answer to the release, the region is what is drawn**, a
    /// stream from the earlier end to the later, whichever way the mouse
    /// went; nothing before the answer and nothing after the release.
    #[test]
    fn the_program_s_region_is_drawn_through_the_selection_highlight() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let live = |t: &Term<'_>| tracked_highlight(t, Scrolled::START, Rows::ZERO);
        feed(&mut t, b"\x1b[?1001h");
        left(
            &mut t,
            MouseKind::Press,
            at(Col::at_index(3), Row::from_index(2)),
        );
        assert_eq!(live(&t), None, "nothing while the answer is owed");
        feed(&mut t, b"\x1b[1;4;3;1;10T");
        left(
            &mut t,
            MouseKind::Motion,
            at(Col::at_index(7), Row::from_index(5)),
        );
        assert_eq!(
            live(&t),
            drawn(
                at(Col::at_index(3), Row::from_index(2)),
                at(Col::at_index(7), Row::from_index(5))
            )
        );
        left(
            &mut t,
            MouseKind::Motion,
            at(Col::at_index(1), Row::from_index(1)),
        );
        assert_eq!(
            live(&t),
            drawn(
                at(Col::at_index(1), Row::from_index(1)),
                at(Col::at_index(3), Row::from_index(2))
            ),
            "sorted for drawing"
        );
        left(
            &mut t,
            MouseKind::Release,
            at(Col::at_index(1), Row::from_index(1)),
        );
        assert_eq!(live(&t), None, "gone at the release");
    }

    /// **On a view scrolled back it sits where the live rows are drawn**,
    /// and what falls below the view is clipped to it.
    #[test]
    fn on_a_view_scrolled_back_it_moves_down_with_the_live_rows() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let lines: Vec<u8> = (0..40)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        assert_eq!(t.scrollback().len(), 31);
        // The count the driver keeps: the live top is line 31.
        let scrolled = Scrolled::START.advanced(Rows::of_count(31));
        feed(&mut t, b"\x1b[?1001h");
        left(
            &mut t,
            MouseKind::Press,
            at(Col::at_index(3), Row::from_index(2)),
        );
        feed(&mut t, b"\x1b[1;4;3;1;10T");
        left(
            &mut t,
            MouseKind::Motion,
            at(Col::at_index(7), Row::from_index(4)),
        );
        assert_eq!(
            tracked_highlight(&t, scrolled, Rows::of_count(3)),
            drawn(
                at(Col::at_index(3), Row::from_index(5)),
                at(Col::at_index(7), Row::from_index(7))
            ),
            "three lines back, three rows down"
        );
        left(
            &mut t,
            MouseKind::Motion,
            at(Col::at_index(7), Row::from_index(8)),
        );
        let h = tracked_highlight(&t, scrolled, Rows::of_count(3)).expect("drawn");
        let want = drawn(
            at(Col::at_index(3), Row::from_index(5)),
            at(Col::at_index(19), Row::from_index(9)),
        )
        .expect("drawn");
        assert_eq!(
            (h.from, h.to, h.region),
            (want.from, want.to, want.region),
            "the end below the view is clipped to its last cell"
        );
        assert_eq!(
            h.ends.1,
            (
                baud_render::WindowLine::of(Row::from_index(11)),
                Col::at_index(7)
            ),
            "and kept where it is, two lines past the view's last row"
        );
    }
}
