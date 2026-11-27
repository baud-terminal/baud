//! **What the app keeps over the screen, across a read**: where the live
//! ring stood before it, so a selection's lines are renumbered by what the
//! read moved and dropped by what it wrote over, and what a switch, a reset
//! or an erased history did. Pure, over a real `Term`, so the rules are
//! one for both drivers and a test reaches them; moved here from `app.rs`
//! when the Linux driver took a selection.

use baud_core::{HistoryMark, Resource, Screen, SurfaceLine, Term};
use baud_domain::{Col, Cols, Row, RowRot, Rows};
use baud_render::{AbsLine, Clip, Grain, Point, Scrolled, SelectedSpan, Selection, Span};

pub(crate) use baud_core::Moved as Ring;

/// Where the live ring stood before a read or a resize, for the
/// selection's line count and a slide, and the counts a read is judged by.
/// The window over the history is the core's, so nothing here
/// anchors it.
#[derive(Clone, Copy)]
pub(crate) struct Before {
    pub(crate) live: RowRot,
    pub(crate) epoch: baud_core::ScreenEpoch,
    /// Read by the macOS pump's slide and both drivers' trail ride.
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

/// **What a copy copies** -- Cmd-C on macOS, Ctrl+Shift+C and a finished
/// drag on Linux: the selection's text, or the text taken when it was made
/// while `?1040` kept it, however a read carried it since; without one, the
/// text output or a click took a selection's highlight away from while
/// `?1040` kept it ([`KeptText`]); `None` for none, or no text. The driver
/// hands what this returns to its clipboard, the one call this leaves.
pub(crate) fn copied(
    sel: Option<&Selection>,
    kept: &KeptText,
    term: &Term<'_>,
    scrolled: Scrolled,
    wrap: baud_render::WordWrap,
) -> Option<String> {
    let text = match sel {
        Some(s) => match kept.text(Some(s)) {
            Some(t) => t.to_owned(),
            None => s.text_as(term, scrolled, wrap),
        },
        None => kept.text(None)?.to_owned(),
    };
    (!text.is_empty()).then_some(text)
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

/// Whether every line a read retired left with the whole screen: no scroll
/// of a region smaller than the screen sent one to the history. A region
/// anchored at the top retires its line the way the whole screen does, and
/// the rows under it stay where they were -- so a slide that pans the whole
/// grid would carry a status line or a dashboard up with the text above it,
/// and a trail that rode would leave the text it was drawn over.
/// By the core's count rather than by the region at the read's end, because
/// a program can set a region, scroll it and reset it inside one read.
pub(crate) fn whole_screen_scrolled(term: &Term<'_>, before: Before) -> bool {
    term.region_scrolls() == before.region
}

/// How the live surface moved over a read, for the selection's line count,
/// a slide and a trail's ride: **nothing across a switch**, whose two
/// rotations are two rings' -- the primary's, round thousands of rows, and
/// the alternate screen's, round the rows the storage is provisioned with,
/// or the alternate screen's before and after an entry reset it -- so their
/// difference is no movement of anything, and read as one it subtracted
/// past zero. A switch drops the selection and settles a slide, whichever
/// way the read went. The rotation is read round the ring, and the
/// alternate screen's ring is its provisioned rows, not the window's: 150
/// in both drivers (`MAX_GRID`), so a read's count there is the lines it
/// scrolled modulo 150, and past 75 of those it reads as lines taken back
/// (`the_alternate_screen_rides_round_its_provisioned_rows`).
pub(crate) fn live_moved(term: &Term<'_>, before: Before) -> Ring {
    if switched(term, before) {
        return Ring::Retired(Rows::ZERO);
    }
    term.moved_since(before.live)
}

/// The oldest line a selection may name: as far behind the live screen's
/// top as the live surface keeps -- the primary's history, and nothing on
/// the alternate screen, where the history count is still the primary's.
pub(crate) fn oldest(term: &Term<'_>, scrolled: Scrolled) -> AbsLine {
    let retained = Rows::of_count(term.scrollback().len());
    scrolled.oldest(term.looking_back(retained))
}

/// **The count each surface numbers its lines by**, one for the primary
/// and one for the alternate screen, so a line of one is never numbered as
/// a line of the other. The live surface's is the drivers' `scrolled`,
/// which a selection, the lines at rest, a hover and the printed paths'
/// facts are all numbered by, so what is filed is what is looked up.
///
/// - **The primary's** moves only by what its window retired into the
///   history or took back (`Term::history_moved_since`), counted by the line
///   at the window's top rather than read round a ring. So it is exact
///   however far a read went, and across a switch: what the alternate
///   screen scrolled moves it not at all, and leaving it needs no
///   re-anchoring. One count shared with the alternate screen moved by its
///   scrolling and was never moved back, so after a pager scrolled, every
///   primary row was looked up that many lines off.
/// - **The alternate screen's** is the core's own number for its top line,
///   read from the record of the last part fed (`ReadRecord::end`, the
///   cursor's line, less its row) while it is up: never read round its
///   ring, which past half of it reads lines retired as lines taken back,
///   and back at its start whenever the core clears it.
///
/// A count made before its terminal is ([`Counts::default`]) is armed at
/// the first read ([`Counts::arm`]): nothing moved before it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Counts {
    primary: Scrolled,
    mark: Option<HistoryMark>,
    /// The primary's window-top line and the count that numbered it as the
    /// last batch began -- the mark and the count the numbering before it
    /// left -- and whether that batch's count stayed on the core's
    /// numbers: what each part's lines are placed by (`CoreLines`).
    base: Option<(baud_domain::LineIdx, Scrolled)>,
    placed: Placed,
    alternate: Scrolled,
}

/// Whether the last numbering kept the primary's count on the core's
/// numbers, or clamped it at the session's first line
/// ([`Numbered::PastStart`]), where no count places the core's lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Placed {
    #[default]
    Yes,
    No,
}

/// **The live surface's count after a read or a resize** ([`Counts`]), or
/// that the primary's window was taken back past the session's first line
/// -- a fresh terminal's scroll down, which no count numbers -- and starts
/// again at it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Numbered {
    At(Scrolled),
    PastStart(Scrolled),
}

impl Numbered {
    /// The count, either way.
    pub(crate) fn scrolled(self) -> Scrolled {
        match self {
            Numbered::At(s) | Numbered::PastStart(s) => s,
        }
    }
}

impl Counts {
    /// Both counts at their start, the primary's window where it stands.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn new(term: &Term<'_>) -> Counts {
        Counts {
            mark: Some(term.history_mark()),
            ..Counts::default()
        }
    }

    /// Both counts, the primary's at `primary`, for a test whose window
    /// already holds history.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn starting(term: &Term<'_>, primary: Scrolled) -> Counts {
        Counts {
            primary,
            ..Counts::new(term)
        }
    }

    /// The primary's window where it stands, if no read has armed it yet.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn arm(&mut self, term: &Term<'_>) {
        if self.mark.is_none() {
            self.mark = Some(term.history_mark());
        }
    }

    /// The primary's count, whatever screen is up.
    pub(crate) fn primary(&self) -> Scrolled {
        self.primary
    }

    /// **Where the core's lines fall on these counts** (`paths::CoreLines`),
    /// on `term`'s window: each part's primary lines placed against its own
    /// window top from the last batch's base, and none after a numbering
    /// that clamped the count at the session's first line, which every
    /// line from the base on is renumbered by.
    pub(crate) fn core_lines(&self, term: &Term<'_>) -> crate::paths::CoreLines {
        let numbers = match self.placed {
            Placed::Yes => crate::paths::Numbers::Followed,
            Placed::No => crate::paths::Numbers::Clamped,
        };
        crate::paths::CoreLines::of(self.base).on(term.geometry().1, self.alternate, numbers)
    }

    /// [`Counts::core_lines`] with the primary's window numbered by `held`
    /// as it stands now: a test whose count stands still while the core
    /// scrolls.
    #[cfg(test)]
    pub(crate) fn core_lines_at(&self, held: Scrolled, term: &Term<'_>) -> crate::paths::CoreLines {
        crate::paths::CoreLines::of(self.mark.map(|m| (m.line(), held))).on(
            term.geometry().1,
            self.alternate,
            crate::paths::Numbers::Followed,
        )
    }

    /// The alternate screen's count, whatever screen is up.
    pub(crate) fn alternate(&self) -> Scrolled {
        self.alternate
    }

    /// **Both counts moved by what the core did since the last call**, and
    /// the live surface's: the primary's by its window's top, and the
    /// alternate screen's from `last`, the record of the last part fed
    /// (`reads::Part::recorded`), when it ended on the alternate screen and
    /// the screen is still up. None fed, the alternate screen's stays.
    pub(crate) fn numbered(
        &mut self,
        term: &Term<'_>,
        last: Option<&baud_core::ReadRecord>,
    ) -> Numbered {
        let mut past = false;
        self.base = self.mark.map(|m| (m.line(), self.primary));
        match self.mark.map(|m| term.history_moved_since(m)) {
            None => {}
            Some(Ring::Retired(n)) => self.primary = self.primary.advanced(n),
            Some(Ring::Restored(n)) => match self.primary.retreated(n) {
                Some(s) => self.primary = s,
                None => {
                    self.primary = Scrolled::START;
                    past = true;
                }
            },
        }
        self.mark = Some(term.history_mark());
        let up = term.screen();
        if let Some(end) = last.map(baud_core::ReadRecord::end) {
            if (end.screen, up) == (Screen::Alternate, Screen::Alternate) {
                let top = end.line.minus(Rows::of_count(term.cursor().0.index()));
                let top = top.map_or(0, baud_domain::LineIdx::value);
                let top = usize::try_from(top).unwrap_or(usize::MAX);
                self.alternate = Scrolled::START.advanced(Rows::of_count(top));
            }
        }
        self.placed = match past {
            true => Placed::No,
            false => Placed::Yes,
        };
        match (up, past) {
            (Screen::Normal, true) => Numbered::PastStart(self.primary),
            (Screen::Normal, false) => Numbered::At(self.primary),
            (Screen::Alternate, _) => Numbered::At(self.alternate),
        }
    }
}

/// **The selection carried with its text across a read**, the count that
/// numbers it already moved to `scrolled`: a region's scroll moves it with
/// its rows and trims what it takes out of the region, a scroll down trims
/// what it pushes under the screen, the alternate screen's top trims what
/// leaves it, the rows under a top region keep theirs, and `IL` and `DL`
/// move it below the cursor (`Term::watched_lines`). Unchanged by
/// a read that moved no rows in place; the caller has dropped one the
/// watch reached.
pub(crate) fn followed(term: &Term<'_>, selection: &mut Option<Selection>, scrolled: Scrolled) {
    if let (Some(sel), Some(w)) = (selection.as_mut(), term.watched_lines()) {
        let last = Col::at_index(term.geometry().0.index().saturating_sub(1));
        sel.follow(scrolled, w, last);
    }
}

/// **What a window resize leaves of the selection, and how the count that
/// numbers it moves**: on the primary the count moves by the
/// rows the resize retired into the history or took back from it, as a
/// read's would -- the core counts them by the window's top line
/// (`Term::history_moved_since`), however many -- and across a change of
/// columns too, where the re-cut renumbers that line: the count numbers
/// the next selection as well, and one left behind by a re-cut numbered the
/// top of the history short, so a click there highlighted one line and
/// copied the next. Across a change of height alone the selection is kept
/// on its lines. A change of columns re-cuts the lines, so the columns a
/// selection names no longer name its cells and it goes; on the alternate
/// screen the program redraws for the new size and the screen has no
/// history to number by, so it goes; and it
/// goes where it reaches a row the shrink cut below the screen, which may
/// come back or be blanked when history is pulled instead. A line the ring
/// recycled is clipped, as after a read. Until 2026-10-20 every change of
/// grid cleared it. `cols` and `screen` are read before the resize, and
/// `counts` are moved by it whatever screen is up ([`Counts::numbered`]),
/// by `rec`, the core's record of the resize (`Term::take_record`): the
/// primary's re-cut renumbers its lines while the alternate screen is up
/// too, and a change of height may move the alternate screen's rows, whose
/// top the record says. The Linux seat's; the macOS driver's relayout
/// says what the selection owes ([`kept_across_resize_owing`]).
#[cfg(any(target_os = "linux", test))]
pub(crate) fn kept_across_resize(
    term: &Term<'_>,
    was: (Cols, Screen),
    selection: Option<Selection>,
    counts: &mut Counts,
    rec: &baud_core::ReadRecord,
) -> (Option<Selection>, Scrolled) {
    kept_across_resize_owing(term, was, selection, counts, rec, Carry::Settled)
}

/// **Whether the selection still owes a read in flight its carry**
/// ([`followed`]): from the readings a pump takes before its feed, which
/// arm the core's watch on its lines ([`Before::watched`]), to the block
/// after the feed that carries it by what the watch says. A resize inside
/// that span -- a relayout the read asked for: the status line's row, the
/// scrollbar's gutter, a program's font -- meets a selection whose lines
/// the read has not yet moved ([`kept_across_resize_owing`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Carry {
    Settled,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Owed,
}

/// `kept_across_resize` for a selection that may still owe a read in
/// flight its carry ([`Carry`]). Owed, the count is numbered and the drops
/// that hold wherever the read moved its lines are made -- a screen other
/// than the primary, a count taken back past the session's first line, a
/// change of columns -- and the two that read its lines are left to the
/// block that carries it first: a line under the screen's bottom
/// ([`below_the_screen`]) and the clip at the history's end. Judged on
/// the lines the read had not yet moved, a selection a region's scroll
/// carried up from the bottom row went with the row a shrink cut under
/// the screen, and one clipped before its carry was moved from where the
/// clip had put it.
pub(crate) fn kept_across_resize_owing(
    term: &Term<'_>,
    (cols, screen): (Cols, Screen),
    selection: Option<Selection>,
    counts: &mut Counts,
    rec: &baud_core::ReadRecord,
    carry: Carry,
) -> (Option<Selection>, Scrolled) {
    let numbered = counts.numbered(term, Some(rec));
    if screen != Screen::Normal || term.screen() != Screen::Normal {
        return (None, numbered.scrolled());
    }
    let scrolled = match numbered {
        Numbered::At(s) => s,
        Numbered::PastStart(s) => return (None, s),
    };
    if term.geometry().0 != cols {
        return (None, scrolled);
    }
    let Some(mut sel) = selection else {
        return (None, scrolled);
    };
    match carry {
        Carry::Owed => return (Some(sel), scrolled),
        Carry::Settled => {}
    }
    if below_the_screen(term, &sel, scrolled) {
        return (None, scrolled);
    }
    match sel.clip(oldest(term, scrolled)) {
        Clip::Kept => (Some(sel), scrolled),
        Clip::Cleared => (None, scrolled),
    }
}

/// **Whether `sel` reaches a line under the screen's bottom**, numbered by
/// `scrolled`: a row a shrink cut below the screen, which may come back or
/// be blanked when history is pulled instead.
pub(crate) fn below_the_screen(term: &Term<'_>, sel: &Selection, scrolled: Scrolled) -> bool {
    let bottom = Row::from_index(term.geometry().1.index().saturating_sub(1));
    sel.ordered().1.line > scrolled.line(Rows::ZERO, bottom)
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
) -> Option<SelectedSpan> {
    let h = term.highlight()?;
    let point = |c: baud_core::CellAt| Point::new(scrolled.line(Rows::ZERO, c.row), c.col);
    let (a, b) = (point(h.start), point(h.end));
    let span = Span {
        start: a.min(b),
        end: a.max(b),
    };
    let view = term.view(back);
    Selection::at(span, Grain::Cell).span(scrolled.top(back), view.rows(), view.cols())
}

/// Whether a read reset the terminal -- `RIS`, from either screen -- by the
/// core's count, because nothing else across a read says so: a reset scrolls
/// nothing and keeps the rotation, so on a terminal with no history both ends
/// of the read look alike, and [`erased`] moves on `CSI 3 J` too, which keeps
/// the screen and a selection on it.
pub(crate) fn reset_in(term: &Term<'_>, before: Before) -> bool {
    term.resets() != before.resets
}

/// A key's or a paste's snap: the primary's window to the live screen, where
/// the key's effect is, and whether that moved it. On the alternate screen
/// the live screen is already what is drawn, and the primary's window is
/// kept for when the program leaves, as foot keeps a view per grid and
/// resets only the live one on a key.
/// The policy is the app's and the window the core's.
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

    /// The program's region on the window `back` lines up, clipped to it.
    fn tracked_highlight(t: &Term<'_>, scrolled: Scrolled, back: Rows) -> Option<Highlight> {
        let view = t.view(back);
        super::tracked_highlight(t, scrolled, back)
            .map(|s| Highlight::of(s, view.rows(), view.cols()))
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

/// Where the counts place the core's lines (`Counts::core_lines`).
#[cfg(test)]
mod counts_tests {
    use super::{Counts, Numbered};
    use baud_core::{Owned, Screen, Term};
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 6,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// **A numbering that clamps the primary's count at the session's
    /// first line places no part's lines** (D2): a count started over a
    /// window that already holds history, at its first line; then, in one
    /// batch, a part that retires four lines and one that scrolls down
    /// six, taking back more than the count has -- `PastStart`. The first
    /// part's top was a line the count numbers, and still nothing is
    /// placed: the clamp moved every number after it. The control: the
    /// same batch with the count started before the history, the first
    /// part placed. Named break: place the lines after a clamp as after
    /// any numbering -- the first part's top is placed.
    #[test]
    fn a_count_clamped_at_the_first_line_places_nothing() {
        let lines = b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni\r\nj\r\n";
        for clamped in [false, true] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut counts = Counts::new(&t);
            feed(&mut t, lines);
            match clamped {
                true => counts = Counts::new(&t),
                false => {
                    let _ = counts.numbered(&t, None);
                }
            }
            feed(&mut t, b"k\r\nl\r\nm\r\nn\r\n");
            let first = t.history_mark().line();
            feed(&mut t, b"\x1b[6T");
            let numbered = counts.numbered(&t, None);
            let placed = counts
                .core_lines(&t)
                .part(Some(first))
                .line(Screen::Normal, first);
            match clamped {
                true => {
                    assert!(matches!(numbered, Numbered::PastStart(_)), "{numbered:?}");
                    assert_eq!(placed, None, "nothing placed past the start");
                }
                false => {
                    assert!(matches!(numbered, Numbered::At(_)), "{numbered:?}");
                    assert!(placed.is_some(), "the control: placed");
                }
            }
        }
    }
}

/// The order of the find's re-selection and the block's carry, when a
/// re-cut falls inside a pump (`app.rs`'s `Reselect`).
#[cfg(all(test, target_os = "macos"))]
mod reselect_tests {
    use super::{Before, Carry, Counts, followed, kept_across_resize_owing, stale_in_moved_rows};
    use crate::find::Finding;
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Codepoint, ReadLen, Row, Rows, WinSize};
    use baud_render::Scrolled;

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// **A re-cut inside the turn re-selects the find's match after the
    /// block's carry, not before it**: forty lines on a ten-row screen, the
    /// find's one match on row 9, selected, and the pump's watch armed on
    /// that selection; a read scrolls a region of the lower five rows up
    /// one, which takes the match's text to row 8, and leaves the cursor on
    /// row 6; a relayout gives a row up, owed its carry. Re-selected at the
    /// re-cut -- the order before the row -- the match's new selection is
    /// then carried by where the read moved the old one's text: up a row
    /// more, and held to the old one's line, it is no longer the match.
    /// Re-selected after the block's carry, it is the match. Break:
    /// `relayout` re-selects under `Carry::Owed` (`app.rs`'s
    /// `a_recut_inside_the_turn_leaves_the_finds_reselection_to_the_pump`).
    #[test]
    fn a_recut_inside_the_turn_reselects_the_match_after_the_carry() {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 10,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let lines = (0..40)
            .map(|i| match i {
                38 => format!("needle {i:02}"),
                _ => format!("line {i:02}"),
            })
            .collect::<Vec<_>>()
            .join("\r\n");
        feed(&mut t, lines.as_bytes());
        let mut counts = Counts::starting(
            &t,
            Scrolled::START.advanced(Rows::of_count(t.scrollback().len())),
        );
        let start = counts.numbered(&t, None).scrolled();
        let line = |r: usize| start.line(Rows::ZERO, Row::from_index(r));
        let query = "needle".chars().map(Codepoint::from_char).collect();
        let mut f = Finding::open(&t, query);
        let found = f.after_resize(&t, start).expect("the match selected");
        assert_eq!(found.text(&t, start), "needle", "the control: the match");
        assert_eq!(found.ordered().0.line, line(8), "the control: on row 9");
        // The pump's readings, armed on the selection as it stands.
        let _ = Before::watched(&mut t, Some(&found), start);
        feed(&mut t, b"\x1b[6;10r\x1b[10;1H\n\x1b[r\x1b[6;1H");
        assert!(
            !stale_in_moved_rows(&t),
            "the control: the region's scroll carried the line"
        );
        // The relayout inside the turn: a row given up, the selection owed
        // its carry.
        let was = (t.geometry().0, t.screen());
        let _ = t.resize(&WinSize {
            ws_col: 20,
            ws_row: 9,
        });
        let rec = t.take_record();
        let (kept, scrolled) =
            kept_across_resize_owing(&t, was, Some(found), &mut counts, &rec, Carry::Owed);
        assert_eq!(scrolled, start, "the re-cut cut a row and retired none");
        // Before the row: the match selected again at the re-cut, then
        // carried by the block.
        let mut before = f.after_resize(&t, scrolled);
        let reselected = before.expect("the match selected again");
        assert_eq!(
            reselected.ordered().0.line,
            line(7),
            "the control: on the settled grid the match is on row 8"
        );
        assert_eq!(reselected.text(&t, scrolled), "needle");
        followed(&t, &mut before, scrolled);
        let carried = before.expect("kept");
        assert_ne!(
            carried, reselected,
            "carried a second time, it left the match"
        );
        assert_ne!(carried.text(&t, scrolled), "needle");
        // The row's order: the block carries the selection it armed on,
        // then the pump selects the match again.
        let mut after = kept;
        followed(&t, &mut after, scrolled);
        assert_eq!(
            after.map(|s| s.ordered().0.line),
            Some(line(7)),
            "the control: the carry took the old selection to the match"
        );
        let after = f.after_resize(&t, scrolled).or(after);
        assert_eq!(after, Some(reselected), "on the match");
        assert_eq!(after.map(|s| s.text(&t, scrolled)), Some("needle".into()));
    }
}
