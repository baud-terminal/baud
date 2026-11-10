//! **What a read did, recorded by the core as it ran it** (`Term::take_record`).
//!
//! An embedder that decides who wrote each line of the screen -- baud's link
//! layer, which keeps a link a remote program printed from opening a file on
//! this machine -- needs to know which lines a read may have written. Read
//! from the bytes, that is a second parser, and the two drift: every escape
//! the scanner and the core read differently is a line filed under the wrong
//! writer. So the core, which runs every byte, says it, and says it only at
//! the events that take the cursor or the text off the path a read of text
//! and line feeds takes.
//!
//! **The forward path costs nothing.** A read that prints and feeds lines,
//! and sets renditions, writes only from the line it began on to the line it
//! ended on, which the embedder reads from the cursor itself. Only what
//! leaves that path is recorded: a climb of the cursor (`Paint::set_row`), a
//! write or erase on rows other than the cursor's (`Paint::edit_reaches`,
//! `Paint::wrote_rows`), text moved between lines, a screen switched, the
//! encoding changed, a folder reported, a prompt marked -- each a control
//! function, none a byte of text.
//!
//! **A line is named on its surface** ([`Screen`]): the primary's lines by
//! the line at the top of its window plus the row, a count only a reflow
//! renumbers (`RIS` erases the screen under it and keeps it); the alternate
//! screen's by a count of its own, of the rows it scrolled, reset where its
//! text is cleared. So a read that switches screens is two spans, one a
//! surface, and no count of one ring is read against the other's. The status
//! display is a line of neither: what a read does on it names nothing
//! (`Paint::record_rows`; a band move takes two rows, and a side-margin
//! scroll on the line erases it, `Paint::scroll_rect`, rather than move
//! text within it), and the alternate screen's count, which a scroll on the
//! line would move, waits parked while the line is written.
//!
//! **A line rendition changed on the screen that is not up names
//! nothing.** `CSI ? 69 h` makes every double-width line single on both
//! screens (`Paint::single_width_pages`); on the screen that is up the rows
//! are written and recorded, and on the parked one only the rendition
//! changes. Its cells do not, and the ones it shows again were written on
//! that same line before it went double-width -- the cursor cannot write
//! past the half meanwhile -- so they are already the line's.
//!
//! **What does not fit is said, never guessed.** More folder reports, marks
//! or moves in one read than the record holds, a reflow, a read that took
//! back more lines than it can count: each is a [`Renumbered`] reason or an
//! overflowed [`Events`], and an embedder that reads one vouches for nothing
//! the record would have said.

// Built without `read-record`, only the record that vouches for nothing is
// made, and the rest of the recording is never reached.
#![cfg_attr(not(feature = "read-record"), allow(dead_code))]

use crate::term::Screen;
use baud_domain::{BytePos, LineIdx, Rows};

/// Lines from the first to the last of one surface, both included.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    first: LineIdx,
    last: LineIdx,
}

impl Span {
    /// From `a` to `b`, whichever way round.
    pub fn of(a: LineIdx, b: LineIdx) -> Span {
        Span {
            first: a.min(b),
            last: a.max(b),
        }
    }

    pub fn first(self) -> LineIdx {
        self.first
    }

    pub fn last(self) -> LineIdx {
        self.last
    }

    /// Both spans and the lines between.
    pub fn hull(self, other: Span) -> Span {
        Span {
            first: self.first.min(other.first),
            last: self.last.max(other.last),
        }
    }

    /// Whether `line` is in the span.
    pub fn holds(self, line: LineIdx) -> bool {
        self.first <= line && line <= self.last
    }

    /// The span moved by `by`, or none where a line would fall before the
    /// first.
    pub fn moved(self, by: LineShift) -> Option<Span> {
        Some(Span {
            first: by.of(self.first)?,
            last: by.of(self.last)?,
        })
    }
}

/// A line on a surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct At {
    pub screen: Screen,
    pub line: LineIdx,
}

/// **How far text moved between lines, and which way.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineShift {
    /// To lines with smaller numbers: a scroll up, `DL`.
    Up(Rows),
    /// To lines with larger numbers: a scroll down, `IL`, and the rows
    /// under a region anchored at the top as it retires a line.
    Down(Rows),
}

impl LineShift {
    /// Where `line` goes.
    pub fn of(self, line: LineIdx) -> Option<LineIdx> {
        match self {
            LineShift::Up(n) => line.minus(n),
            LineShift::Down(n) => Some(line.plus(n)),
        }
    }

    /// Where the text now on `line` came from: the shift undone.
    fn back(self, line: LineIdx) -> Option<LineIdx> {
        match self {
            LineShift::Up(n) => Some(line.plus(n)),
            LineShift::Down(n) => line.minus(n),
        }
    }

    /// This shift and then `next`, where both go the same way.
    fn then(self, next: LineShift) -> Option<LineShift> {
        match (self, next) {
            (LineShift::Up(a), LineShift::Up(b)) => {
                Some(LineShift::Up(Rows::of_count(a.index() + b.index())))
            }
            (LineShift::Down(a), LineShift::Down(b)) => {
                Some(LineShift::Down(Rows::of_count(a.index() + b.index())))
            }
            _ => None,
        }
    }
}

/// **Text moved between lines of one surface** by a scroll of part of the
/// screen, `IL`, `DL`, `DECCRA`, a pinned prompt, or a region anchored at
/// the top retiring a line under which the rows keep their text and take
/// new numbers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    /// The text on `from` now stands on `from` moved `by`, the rows it left
    /// written by the move: exact.
    Exact {
        screen: Screen,
        from: Span,
        by: LineShift,
    },
    /// Text moved within `band` more than one way: any line of the band may
    /// now hold text any line of it held. A run of scrolls of one region
    /// folds to this.
    Within { screen: Screen, band: Span },
}

impl Move {
    /// The screen the text moved on.
    pub fn screen(self) -> Screen {
        match self {
            Move::Exact { screen, .. } | Move::Within { screen, .. } => screen,
        }
    }

    /// The lines the move read from and wrote to.
    pub fn band(self) -> Span {
        match self {
            Move::Exact { from, by, .. } => from.moved(by).map_or(from, |to| from.hull(to)),
            Move::Within { band, .. } => band,
        }
    }

    /// **This move and then `next` as one**, where they fold: a chain that
    /// carries the same text the same way again, exactly, as the rows under
    /// a top region do at each line it retires; or two moves within one
    /// band, as a region scrolled line by line, as moves within it.
    fn then(self, next: Move) -> Option<Move> {
        if self.screen() != next.screen() {
            return None;
        }
        if let (
            Move::Exact { screen, from, by },
            Move::Exact {
                from: f2, by: b2, ..
            },
        ) = (self, next)
        {
            if from.moved(by) == Some(f2) {
                if let Some(by) = by.then(b2) {
                    return Some(Move::Exact { screen, from, by });
                }
            }
        }
        match self.band() == next.band() || self.band().hull(next.band()) == self.band() {
            true => Some(Move::Within {
                screen: self.screen(),
                band: self.band(),
            }),
            false => None,
        }
    }
}

/// What a folder report did ([`FolderEvent`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Folder {
    /// `OSC 7` or an iTerm2 `CurrentDir` the core kept.
    Kept,
    /// One it refused.
    Refused,
    /// One that cleared what was kept.
    Forgot,
}

/// **A folder report the core acted on**, where its terminator is in the
/// read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FolderEvent {
    pub at: BytePos,
    pub folder: Folder,
}

/// **An `OSC 133` mark the core acted on**: where its terminator is in the
/// read, and its letter.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MarkEvent {
    pub at: BytePos,
    pub letter: u8,
}

/// The offset shown as a number.
impl core::fmt::Debug for FolderEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FolderEvent")
            .field("at", &self.at.index())
            .field("folder", &self.folder)
            .finish()
    }
}

/// The offset shown as a number.
impl core::fmt::Debug for MarkEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MarkEvent")
            .field("at", &self.at.index())
            .field("letter", &char::from(self.letter))
            .finish()
    }
}

/// Whether a record's list held every event, or more came than it holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fill {
    Room,
    Overflowed,
}

/// **Up to `N` events in the order they ran**, and whether more came.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Events<T: Copy, const N: usize> {
    items: [Option<T>; N],
    fill: Fill,
}

impl<T: Copy, const N: usize> Events<T, N> {
    pub const EMPTY: Events<T, N> = Events {
        items: [None; N],
        fill: Fill::Room,
    };

    /// The events, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.items.iter().map_while(|e| *e)
    }

    pub fn fill(&self) -> Fill {
        self.fill
    }

    fn last_mut(&mut self) -> Option<&mut T> {
        self.items.iter_mut().rev().find_map(|e| e.as_mut())
    }

    /// Only the events `keep` keeps, in order.
    fn keep(&mut self, keep: impl Fn(&T) -> bool) {
        let mut kept = Events::EMPTY;
        self.iter().filter(|e| keep(e)).for_each(|e| kept.push(e));
        kept.fill = self.fill;
        *self = kept;
    }

    fn push(&mut self, e: T) {
        match self.items.iter_mut().find(|s| s.is_none()) {
            Some(slot) => *slot = Some(e),
            None => self.fill = Fill::Overflowed,
        }
    }
}

/// Whether every byte the read ran, ran under UTF-8.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadEncoding {
    Utf8,
    /// A byte ran under ISO 8859-1 (`ESC % @`), where `0x80`-`0x9F` are
    /// controls: the read began under it or switched to it.
    Other,
}

/// **Where the parser stood**, as far as a read of the stream can count on
/// it: at ANSI ground, at VT52's, or inside a sequence or a character.
/// `Node::AfterControl` is ground: every C0 control, a line feed among
/// them, leads there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grounded {
    Ansi,
    Vt52,
    Inside,
}

/// Whether every line feed the read ran moved the cursor or scrolled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feeds {
    Moved,
    /// One did neither: the cursor on the last row, below a region that
    /// ends higher, where `IND` has nothing to do.
    Stuck,
}

/// **Whether the read wrote a cell of the line it began on**: text while
/// the cursor had not yet left that line, an erase or edit reaching it at
/// any point (`Paint::wrote_rows`), a reset, or any control function not
/// known to write no cell run with the cursor on it
/// (`Paint::control_may_write`) -- each dispatch names those that write
/// none, exhaustively, so a writer no list names still says the line was
/// written. Not a write: a cursor move, a carriage return, a line feed, a
/// rendition, a mode that changes no cell, a report. A return to the line
/// once the cursor has left it is a climb, which the record names off the
/// path instead ([`ReadRecord::off_path`]); text moved onto it is one of
/// the read's moves ([`ReadRecord::moves`]). What tells a command whose
/// first read only feeds off the prompt line from one that writes over it
/// is all three: an embedder takes the line as untouched only where this
/// says so and neither of the others names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartRow {
    Untouched,
    Wrote,
}

/// **Whether text written now may still be on the line the read began
/// on**: open until the read's first text, which decides it, so a run of
/// text tests it once a read and never again (`Paint::hooks`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StartWindow {
    Open,
    Closed,
}

/// [`StartRow`] as the record keeps it while the read runs: the line not
/// written yet with the read's text still able to land on it, or with its
/// text off it, or written. One byte in the record's padding, where the
/// window and the fact apart would take a word of `Cold`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StartState {
    Open,
    Left,
    Wrote,
}

/// Whether the alternate screen's text was cleared in the read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cleared {
    No,
    Alternate,
}

/// **Why the record's lines cannot be read against the embedder's**: its
/// numbering moved in a way no count says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Renumbered {
    /// A change of columns re-cut the primary's lines (`Term::resize`).
    Resize,
    /// The core was built without the record (`read-record` off): it
    /// vouches for nothing.
    Unrecorded,
}

/// **What one read did**, from [`crate::Term::take_record`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadRecord {
    start: At,
    end: At,
    start_row: StartState,
    primary: Option<Span>,
    alternate: Option<Span>,
    encoding: ReadEncoding,
    began: Grounded,
    ended: Grounded,
    feeds: Feeds,
    cleared: Cleared,
    renumbered: Option<Renumbered>,
    folders: Events<FolderEvent, 4>,
    marks: Events<MarkEvent, 8>,
    moves: Events<Move, 8>,
}

impl ReadRecord {
    /// Where the cursor was as the read began.
    pub fn start(&self) -> At {
        self.start
    }

    /// Where it was as the read ended.
    pub fn end(&self) -> At {
        self.end
    }

    /// **The lines of `screen` the read may have written off its forward
    /// path**, none if it wrote only where the cursor went forward: what a
    /// climb left, rows an erase or an edit reached, a switch's lines.
    pub fn off_path(&self, screen: Screen) -> Option<Span> {
        match screen {
            Screen::Normal => self.primary,
            Screen::Alternate => self.alternate,
        }
    }

    pub fn encoding(&self) -> ReadEncoding {
        self.encoding
    }

    /// Where the parser stood as the read began and as it ended.
    pub fn grounded(&self) -> (Grounded, Grounded) {
        (self.began, self.ended)
    }

    /// Whether the read wrote a cell of the line it began on ([`StartRow`]).
    pub fn start_row(&self) -> StartRow {
        match self.start_row {
            StartState::Open | StartState::Left => StartRow::Untouched,
            StartState::Wrote => StartRow::Wrote,
        }
    }

    pub fn feeds(&self) -> Feeds {
        self.feeds
    }

    pub fn cleared(&self) -> Cleared {
        self.cleared
    }

    pub fn renumbered(&self) -> Option<Renumbered> {
        self.renumbered
    }

    pub fn folders(&self) -> &Events<FolderEvent, 4> {
        &self.folders
    }

    pub fn marks(&self) -> &Events<MarkEvent, 8> {
        &self.marks
    }

    pub fn moves(&self) -> &Events<Move, 8> {
        &self.moves
    }

    /// **The lines whose text the read's moves may have carried onto
    /// `line`** of `screen`, `line` itself among them: the moves undone,
    /// newest first. An exact move takes back the part of each source that
    /// lies where it wrote; a band that meets a source holds every line of
    /// it. The one composition of the moves: an embedder that files a line
    /// by where its text came from reads these, and the record's tests
    /// prove the moves against the same function.
    pub fn carried_from(&self, screen: Screen, line: LineIdx) -> Sources {
        let mut out = Sources::of(Span::of(line, line));
        let moves = self.moves.items.iter().rev().filter_map(|m| *m);
        for m in moves.filter(|m| m.screen() == screen) {
            match m {
                Move::Exact { from, by, .. } => {
                    let Some(to) = from.moved(by) else { continue };
                    for s in out.spans() {
                        let (a, b) = (s.first.max(to.first), s.last.min(to.last));
                        if a > b {
                            continue;
                        }
                        if let (Some(a), Some(b)) = (by.back(a), by.back(b)) {
                            out.add(Span::of(a, b));
                        }
                    }
                }
                Move::Within { band, .. } => {
                    if out
                        .spans()
                        .any(|s| s.first <= band.last && band.first <= s.last)
                    {
                        out.add(band);
                    }
                }
            }
        }
        out
    }
}

/// How many spans [`Sources`] keeps before it holds their hull instead.
const SOURCES: usize = 16;

/// **Lines a line's text may have come from**, on one surface
/// ([`ReadRecord::carried_from`]): up to sixteen spans, and past that their
/// hull, which holds every line they held and the lines between.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sources {
    spans: [Option<Span>; SOURCES],
}

impl Sources {
    fn of(s: Span) -> Sources {
        let mut spans = [None; SOURCES];
        spans[0] = Some(s);
        Sources { spans }
    }

    /// The spans, as a copy, so a caller may add while it walks them.
    fn spans(&self) -> impl Iterator<Item = Span> + use<> {
        let spans = self.spans;
        spans.into_iter().map_while(|s| s)
    }

    /// `s` among the sources: nothing where one already holds it, the hull
    /// of all where there is no room.
    fn add(&mut self, s: Span) {
        if self.spans().any(|t| t.first <= s.first && s.last <= t.last) {
            return;
        }
        match self.spans.iter_mut().find(|t| t.is_none()) {
            Some(slot) => *slot = Some(s),
            None => *self = Sources::of(self.spans().fold(s, Span::hull)),
        }
    }

    /// The spans, in the order they were found.
    pub fn iter(&self) -> impl Iterator<Item = Span> + '_ {
        self.spans.iter().map_while(|s| *s)
    }

    /// Whether `line` is one of the sources.
    pub fn holds(&self, line: LineIdx) -> bool {
        self.iter().any(|s| s.holds(line))
    }
}

/// **The record being kept**, in `Cold`: what the read so far did, and where
/// it began.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Recording {
    rec: ReadRecord,
    /// Where the control string ending now ends in the read: the offset a
    /// folder report or a mark it carries is recorded at.
    str_at: BytePos,
}

/// The record so far, the string offset as a number.
impl core::fmt::Debug for Recording {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Recording")
            .field("rec", &self.rec)
            .field("str_at", &self.str_at.index())
            .finish()
    }
}

impl Recording {
    /// A record begun at `start`, the parser at `began` under `encoding`.
    pub(crate) fn begun(start: At, began: Grounded, encoding: ReadEncoding) -> Recording {
        Recording {
            rec: ReadRecord {
                start,
                end: start,
                start_row: StartState::Open,
                primary: None,
                alternate: None,
                encoding,
                began,
                ended: began,
                feeds: Feeds::Moved,
                cleared: Cleared::No,
                renumbered: None,
                folders: Events::EMPTY,
                marks: Events::EMPTY,
                moves: Events::EMPTY,
            },
            str_at: BytePos::ZERO,
        }
    }

    /// Whether the read's text may still land on its first line.
    pub(crate) fn start_window(&self) -> StartWindow {
        match self.rec.start_row {
            StartState::Open => StartWindow::Open,
            StartState::Left | StartState::Wrote => StartWindow::Closed,
        }
    }

    /// Whether the read has written its first line yet.
    pub(crate) fn start_row(&self) -> StartRow {
        self.rec.start_row()
    }

    /// **The read wrote its first line**, and so text decides nothing more.
    pub(crate) fn start_wrote(&mut self) {
        self.rec.start_row = StartState::Wrote;
    }

    /// **The window as it was before a write that was not the read's text
    /// at the cursor** (`Paint::overwrote_unprinted`): open again if it was
    /// and the line is still not written.
    pub(crate) fn reopened(&mut self, was: StartWindow) {
        if was == StartWindow::Open && self.rec.start_row == StartState::Left {
            self.rec.start_row = StartState::Open;
        }
    }

    /// **The read's text is off its first line**, unless it wrote it: a
    /// return to it is a climb, which is recorded off the path.
    pub(crate) fn start_left(&mut self) {
        if self.rec.start_row == StartState::Open {
            self.rec.start_row = StartState::Left;
        }
    }

    /// The record, ended at `end` with the parser at `ended`.
    pub(crate) fn ended(&self, end: At, ended: Grounded) -> ReadRecord {
        ReadRecord {
            end,
            ended,
            // Handed out as the fact alone: whether text could still have
            // landed on the line is the read's, not the record's.
            start_row: match self.rec.start_row {
                StartState::Open | StartState::Left => StartState::Left,
                StartState::Wrote => StartState::Wrote,
            },
            ..self.rec
        }
    }

    /// **Lines `span` of `screen` were, or may have been, written** off the
    /// forward path.
    pub(crate) fn reached(&mut self, screen: Screen, span: Span) {
        let slot = match screen {
            Screen::Normal => &mut self.rec.primary,
            Screen::Alternate => &mut self.rec.alternate,
        };
        *slot = Some(slot.map_or(span, |s| s.hull(span)));
    }

    /// A byte is about to run under ISO 8859-1.
    pub(crate) fn latin1(&mut self) {
        self.rec.encoding = ReadEncoding::Other;
    }

    /// A line feed did not move the cursor.
    pub(crate) fn stuck(&mut self) {
        self.rec.feeds = Feeds::Stuck;
    }

    /// The alternate screen's text was cleared: whatever was named of its
    /// lines before is gone, so lines it held before in this read are too.
    pub(crate) fn cleared_alternate(&mut self) {
        self.rec.cleared = Cleared::Alternate;
        self.rec.alternate = None;
        self.rec.moves.keep(|m| m.screen() != Screen::Alternate);
    }

    pub(crate) fn renumbered(&mut self, why: Renumbered) {
        self.rec.renumbered = Some(why);
    }

    /// A control string ends at `at` in the read.
    pub(crate) fn string_ended(&mut self, at: BytePos) {
        self.str_at = at;
    }

    /// The folder report ending now did `folder`.
    pub(crate) fn folder(&mut self, folder: Folder) {
        self.rec.folders.push(FolderEvent {
            at: self.str_at,
            folder,
        });
    }

    /// The `OSC 133` mark ending now, of `letter`, ran.
    pub(crate) fn mark_ran(&mut self, letter: baud_domain::StreamByte) {
        self.rec.marks.push(MarkEvent {
            at: self.str_at,
            letter: letter.raw_byte(),
        });
    }

    /// **Text moved**: folded into the last move where the two fold, a new
    /// one otherwise. **Past the last slot every move of a surface folds to
    /// one**, within the hull of their bands, so a read of more moves than
    /// the record holds -- a progress display redrawn in a region, line by
    /// line -- still says which lines its text moved among, and nothing is
    /// dropped.
    pub(crate) fn moved(&mut self, m: Move) {
        if let Some(last) = self.rec.moves.last_mut() {
            if let Some(both) = last.then(m) {
                *last = both;
                return;
            }
        }
        if self.rec.moves.items.iter().all(Option::is_some) {
            let hull = |screen: Screen| {
                self.rec
                    .moves
                    .iter()
                    .chain(core::iter::once(m))
                    .filter(|x| x.screen() == screen)
                    .map(Move::band)
                    .reduce(Span::hull)
                    .map(|band| Move::Within { screen, band })
            };
            let folded = [hull(Screen::Normal), hull(Screen::Alternate)];
            self.rec.moves = Events::EMPTY;
            folded
                .into_iter()
                .flatten()
                .for_each(|w| self.rec.moves.push(w));
            return;
        }
        self.rec.moves.push(m);
    }
}

impl Grounded {
    /// **Where `node` stands**: ANSI or VT52 ground, a control's aftermath
    /// counted as ground (every C0 control leads there), or inside.
    pub(crate) const fn of(node: crate::graph::Node) -> Grounded {
        use crate::graph::Node;
        match node {
            Node::Ground | Node::AfterControl => Grounded::Ansi,
            Node::Vt52Ground | Node::Vt52AfterControl => Grounded::Vt52,
            _ => Grounded::Inside,
        }
    }
}

#[cfg(feature = "read-record")]
impl crate::paint::Paint<'_, '_> {
    /// **Row `r` of the live screen as a line of the record**: the primary's
    /// window-top line plus the row, the alternate screen's own count plus
    /// it.
    pub(crate) fn record_line(&self, r: baud_domain::Row) -> At {
        let rows = Rows::of_count(r.index());
        match self.cold.screen {
            Screen::Normal => At {
                screen: Screen::Normal,
                line: self.book.top.plus(rows),
            },
            Screen::Alternate => At {
                screen: Screen::Alternate,
                line: self.cold.alt_top.plus(rows),
            },
        }
    }

    /// **Rows `a` to `b` of the live screen were, or may have been,
    /// written** off the forward path. Out of line: a control function's
    /// cost, never a glyph's. On the status display, nothing: its one
    /// row is no line of either screen's.
    #[cold]
    #[inline(never)]
    pub(crate) fn record_rows(&mut self, a: baud_domain::Row, b: baud_domain::Row) {
        if self.on_status() {
            return;
        }
        let (a, b) = (self.record_line(a), self.record_line(b));
        self.cold.record.reached(a.screen, Span::of(a.line, b.line));
    }

    /// **Rows `a` to `b` of the live screen were written**, tested against
    /// the line the read began on ([`StartRow`]). Out of line, and reached
    /// until the read has written that line: by an erase or edit, a control
    /// function's cost, and by text once a read (`Paint::hooks`).
    #[cold]
    #[inline(never)]
    pub(crate) fn record_start_rows(&mut self, a: baud_domain::Row, b: baud_domain::Row) {
        if self.on_status() {
            return;
        }
        let start = self.cold.record.rec.start;
        let (a, b) = (self.record_line(a), self.record_line(b));
        if a.screen == start.screen && a.line <= start.line && start.line <= b.line {
            self.cold.record.start_wrote();
        }
    }

    /// **The cursor climbs to `to`**: the rows from there to the row it
    /// leaves, the lowest it reached since it last climbed, may hold what
    /// the read wrote, whichever way the read goes after.
    #[cold]
    #[inline(never)]
    pub(crate) fn record_climb(&mut self, to: baud_domain::Row) {
        self.record_rows(to, self.cy);
    }

    /// Text moved on the live screen, as rows (`from` and `by`).
    #[cold]
    #[inline(never)]
    pub(crate) fn record_move(
        &mut self,
        from: (baud_domain::Row, baud_domain::Row),
        by: LineShift,
    ) {
        let (a, b) = (self.record_line(from.0), self.record_line(from.1));
        self.cold.record.moved(Move::Exact {
            screen: a.screen,
            from: Span::of(a.line, b.line),
            by,
        });
    }
}

impl crate::term::Term<'_> {
    /// **What the reads fed since the last call did**, and a record begun
    /// again where the cursor stands: one call after each part of a read an
    /// embedder files on its own (`crate::record`). Built without
    /// `read-record`, it vouches for nothing ([`Renumbered::Unrecorded`]).
    pub fn take_record(&mut self) -> ReadRecord {
        let at = self.record_at();
        let ground = Grounded::of(self.node);
        let encoding = match self.cold.encoding {
            baud_domain::InputEncoding::Utf8 => ReadEncoding::Utf8,
            _ => ReadEncoding::Other,
        };
        #[cfg(feature = "read-record")]
        {
            let rec = self.cold.record.ended(at, ground);
            self.cold.record = Recording::begun(at, ground, encoding);
            rec
        }
        #[cfg(not(feature = "read-record"))]
        {
            let mut r = Recording::begun(at, ground, ReadEncoding::Other);
            r.renumbered(Renumbered::Unrecorded);
            let _ = encoding;
            r.ended(at, ground)
        }
    }

    /// The cursor as a line of the record.
    fn record_at(&self) -> At {
        let rows = Rows::of_count(self.cy.index());
        match self.cold.screen {
            Screen::Normal => At {
                screen: Screen::Normal,
                line: self.store.book.top.plus(rows),
            },
            #[cfg(feature = "read-record")]
            Screen::Alternate => At {
                screen: Screen::Alternate,
                line: self.cold.alt_top.plus(rows),
            },
            #[cfg(not(feature = "read-record"))]
            Screen::Alternate => At {
                screen: Screen::Alternate,
                line: LineIdx::FIRST.plus(rows),
            },
        }
    }
}

#[cfg(feature = "read-record")]
impl crate::term::Cold {
    /// The live screen, and the line its top row takes in the record, the
    /// primary's window top being `top`.
    fn record_base(&self, top: LineIdx) -> (Screen, LineIdx) {
        match self.screen {
            Screen::Normal => (Screen::Normal, top),
            Screen::Alternate => (Screen::Alternate, self.alt_top),
        }
    }

    /// **Rows `from` to `to` moved `k` rows `way` in place**, the primary's
    /// window top at `top`: the text that stays in the band, recorded with
    /// where it went. The rows it leaves are blanked, and recorded as
    /// written where they are (`Paint::path_rows`).
    pub(crate) fn record_band(
        &mut self,
        (from, to): (baud_domain::Row, baud_domain::Row),
        k: Rows,
        way: crate::ops::Scroll,
        top: LineIdx,
    ) {
        let (first, last, by) = match way {
            crate::ops::Scroll::Up => (from.index() + k.index(), to.index(), LineShift::Up(k)),
            crate::ops::Scroll::Down => match to.index().checked_sub(k.index()) {
                Some(last) => (from.index(), last, LineShift::Down(k)),
                None => return,
            },
        };
        if first > last {
            return;
        }
        let (screen, base) = self.record_base(top);
        let from = Span::of(
            base.plus(Rows::of_count(first)),
            base.plus(Rows::of_count(last)),
        );
        self.record.moved(Move::Exact { screen, from, by });
    }

    /// **Text moved within rows `from` to `to` more than one way**, as a
    /// scroll of a rectangle under side margins moves part of each row.
    pub(crate) fn record_within(
        &mut self,
        (from, to): (baud_domain::Row, baud_domain::Row),
        top: LineIdx,
    ) {
        let (screen, base) = self.record_base(top);
        let band = Span::of(
            base.plus(Rows::of_count(from.index())),
            base.plus(Rows::of_count(to.index())),
        );
        self.record.moved(Move::Within { screen, band });
    }
}

/// **Where the alternate screen's count starts**, and starts again where its
/// text is cleared: far from the first line, so a scroll down from it still
/// has a line to number.
#[cfg(feature = "read-record")]
pub(crate) const ALT_BASE: LineIdx = LineIdx::of(1 << 40);

#[cfg(feature = "read-record")]
impl crate::paint::Paint<'_, '_> {
    /// **The alternate screen took a line back** (a scroll down of the
    /// whole screen): its count goes back one, or, at its first line, its
    /// text moves down a line under the numbers it had.
    #[cold]
    #[inline(never)]
    pub(crate) fn alt_unretired(&mut self) {
        match self.cold.alt_top.minus(Rows::of_count(1)) {
            Some(t) => self.cold.alt_top = t,
            None => self.record_move(
                (
                    baud_domain::Row::ZERO,
                    baud_domain::Row::from_index(self.rows.index().saturating_sub(2)),
                ),
                LineShift::Down(Rows::of_count(1)),
            ),
        }
    }

    /// **The alternate screen's text was cleared**: its count starts again,
    /// and the record forgets what it named of its lines.
    pub(crate) fn alt_cleared(&mut self) {
        self.cold.alt_top = ALT_BASE;
        // Both copies: `RIS` clears from the line as well.
        self.strs.status.park.alt_top = ALT_BASE;
        self.cold.record.cleared_alternate();
    }
}
