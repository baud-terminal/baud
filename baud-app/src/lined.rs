//! **A link's line at rest**: the scan that finds, on a frame, what is drawn
//! underlined with no modifier held -- an `OSC 8` run and a URI printed as
//! plain text -- pure over the core's cells, so its tests run on every
//! platform and the `frame` bench compiles this file as it stands.
//!
//! **Only what changed is read.** [`Lines`] keeps the window's cells as the
//! last scan saw them, each row by the line it shows ([`AbsLine`]), and reads
//! a row only when its cells, its wrap or its line size differ from what that
//! line held, with the rest of the logical line it belongs to, since a URI
//! runs across the rows a soft wrap joined. A frame that moved only the
//! cursor reads no row; it asks again only whether a row running on into the
//! next joins it (`Below`), two cells and a provenance a row so ended, since a
//! program is named after it wrote. **A scroll reads only the rows it brought
//! in**: the rows it moved keep their lines, moved with them
//! ([`Lines::scan`]). A logical line read past the window's bottom is read
//! again when a row below the window it went by is written over, as a look
//! back lets the output below it be.
//!
//! **Bounded whatever a program sends.** One pass over each changed row's
//! cells, in which a line with no scheme's name and `:` and no `OSC 8` cell
//! costs one compare a cell. A logical line crossing the window's edge is
//! read no further than [`crate::link::MAX`] characters past it, and the
//! walk's end is an [`Edge::Cut`], which refuses what touches it. A run of URI
//! characters past `MAX` is refused whole, as a click on it would be.
//!
//! **`admit` is the only gate** ([`Reader::admits`]): a link it refuses is
//! drawn as no link, and so is one `find`'s rules refuse (`crate::link`).
//!
//! **The seam is [`Reader`]**: what finds links in a line's text. [`Uris`]
//! finds URIs, and `paths::Links` URIs and printed paths in one pass, the
//! paths drawn from what a worker thread found (`paths.rs`): the text tells
//! a reader which of its units the window shows, and which lines and screen
//! they are on, and nothing else here changed for them.
//!
//! **An overlay and nothing else.** No filesystem call, no process and no
//! LaunchServices query happens here, and nothing is written to a cell, so no
//! query of the core's -- `DECRQCRA` for one -- reads a line back.
//!
//! A cell a program already underlines keeps its own line and gets none of
//! these, and a concealed cell gets none. A double-size line gets none: no
//! overlay of a single cell's width lies on its cells.
//!
//! **A link baud knows would open nothing is struck and underlined**
//! ([`Lines::struck`], [`Mark::Struck`]): what the reader says, from the
//! worker's answers it was handed, never from a call made here. At rest that
//! is a link whose kind has no app to open it, and an `OSC 8` `file://` link
//! naming this machine by name whose file the worker found gone (`paths.rs`
//! has the rules). A cell a program already strikes with `SGR 9` keeps its
//! own strike, and a concealed cell and a double-size line get none, as for
//! the line. Its line is the live line's -- `[link] line`'s style, in the
//! box's colour at rest and the text's under the pointer -- kept clear of
//! the strike, and a cell a program underlines keeps its own. Struck or
//! not, the cells are the program's: a struck link selects and copies as
//! it is written.
#![forbid(unsafe_code)]

use core::ops::Range;

use baud_core::{Screen, Term};
use baud_domain::{
    Attrs, ClusterEpoch, Col, Cols, LineRendition, LinkEpoch, LinkId, LinkSet, LinkSlot,
    PackedCell, Row, RowExtent, RowGens, RowLinks, RowStyles, Rows, Seal, SlotMarks, StyleWord,
    WalkSteps, WindowAt, Wrap,
};
use baud_render::{AbsLine, LinkLine, Point, Scrolled, row_of, row_styles_of};

use crate::link::{
    Class, Edge, Joins, MAX, Target, Unit, admit_link, class, found_in, may_be_cut, runs_on,
    scheme_at, scheme_ends, unit,
};
use crate::rules::{Rules, Tail};

/// **A logical line's text**, as much of it as was read.
#[derive(Clone, Copy, Debug)]
pub struct Text<'a> {
    pub units: &'a [Unit],
    pub head: Edge,
    pub tail: Edge,
    /// The units on the window's rows, from and to: what is read past the
    /// window's edges is read for a link that runs onto them, and no more.
    pub shown: (usize, usize),
    /// The line's first and last line, as read.
    pub lines: (AbsLine, AbsLine),
    /// Where each unit was read from, one a unit: what a link found in the
    /// text spans ([`Text::lines_of`]). Empty for a text read without them.
    pub points: &'a [Point],
    /// The screen it is on.
    pub screen: Screen,
}

impl Text<'_> {
    /// **The first and last line the units `span` were read from**: the rows
    /// a link found there covers, whose writers its origin is judged by,
    /// at rest as at its click. The whole line's where no points were kept.
    pub fn lines_of(&self, span: &Range<usize>) -> (AbsLine, AbsLine) {
        let first = self.points.get(span.start).map(|p| p.line);
        let last = span
            .end
            .checked_sub(1)
            .and_then(|e| self.points.get(e))
            .map(|p| p.line);
        match (first, last) {
            (Some(a), Some(b)) => (a, b),
            _ => self.lines,
        }
    }
}

/// **How a link is drawn at rest**: underlined, as a link a click opens, or
/// struck, as one baud knows would open nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Line,
    Struck,
}

/// **A link a reader found**: its units in the line's text, its mark, and
/// whether its line waits on an answer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Run {
    pub units: Range<usize>,
    pub mark: Mark,
    pub key: Key,
}

/// **Whether a run's line waits on an answer**: a URI's never does; a
/// printed path's is drawn only while the worker found its file, so the
/// scan keeps it, drawn or not, with the path it was looked up as, and a
/// batch of answers redraws it with no row read ([`Lines::answered`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Key {
    Fixed,
    /// The path, as the reader's [`Reader::key`] spells it, and whether
    /// its answer draws it now.
    Path {
        at: Range<usize>,
        drawn: Drawn,
    },
}

/// Whether a printed path's line is drawn: its file found, or not (yet).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drawn {
    Yes,
    No,
}

/// Whether a kept path's run crosses a row its writer ended, which a link
/// across a join takes from the reads of each line alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Crosses {
    No,
    Yes,
}

/// **A printed path's run the scan keeps**: the path, in [`Lines`]'s keys,
/// the pieces of line it draws on the window, in its pieces, and whether
/// they are drawn.
#[derive(Clone, Debug)]
struct Wait {
    key: Range<usize>,
    pieces: Range<usize>,
    drawn: Drawn,
    crosses: Crosses,
}

/// Whether this scan has read the view's rows holding each slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum SlotsRead {
    #[default]
    No,
    Yes,
}

/// Whether a batch of answers came since the last scan, whose waiting runs
/// it redraws.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Answers {
    #[default]
    None,
    Came,
}

/// **Where an `OSC 8` link's cell is read at rest**: its line, its slot, the
/// rows of the view holding that slot -- the cells its look draws, which its
/// origin is judged over (`paths::Paths::link_origin`) -- and the screen,
/// which says whether output there is known to be this machine's. Made only
/// by the scan, from one [`SlotRows`] of the view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct At<'a> {
    line: AbsLine,
    id: LinkId,
    rows: &'a [AbsLine],
    screen: Screen,
}

impl<'a> At<'a> {
    /// The cell's line.
    pub fn line(&self) -> AbsLine {
        self.line
    }

    /// The slot.
    pub fn id(&self) -> LinkId {
        self.id
    }

    /// The rows of the view holding the slot, top first.
    pub fn rows(&self) -> &'a [AbsLine] {
        self.rows
    }

    /// The first and last of those rows.
    pub fn span(&self) -> (AbsLine, AbsLine) {
        let first = self.rows.first().copied().unwrap_or(self.line);
        let last = self.rows.last().copied().unwrap_or(self.line);
        (first, last)
    }

    /// The screen it is on.
    pub fn screen(&self) -> Screen {
        self.screen
    }
}

/// **The rows of a view holding each `OSC 8` slot**, built in one pass over
/// the view's cells: what a link's look draws, so what its origin is judged
/// over, once a slot a scan and bounded by the view however long a link
/// left open has run.
#[derive(Clone, Debug, Default)]
pub struct SlotRows {
    /// Each slot's rows, by slot and then line, one entry a row.
    pairs: Vec<(usize, AbsLine)>,
    lines: Vec<AbsLine>,
}

impl SlotRows {
    /// **The view's rows holding a slot**: every slot's, or `only`'s, each
    /// cell read counted in `read`.
    pub fn of(
        term: &Term<'_>,
        scrolled: Scrolled,
        back: Rows,
        only: Option<LinkId>,
        read: &mut WalkSteps,
    ) -> SlotRows {
        let mut s = SlotRows::default();
        s.fill(term, scrolled, back, only, read);
        s
    }

    fn fill(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        back: Rows,
        only: Option<LinkId>,
        read: &mut WalkSteps,
    ) {
        self.pairs.clear();
        let rows = term.view(back).rows().index();
        for y in 0..rows {
            let line = scrolled.line(back, Row::from_index(y));
            let Some((cells, _)) = row_of(term, scrolled, line) else {
                continue;
            };
            let start = self.pairs.len();
            for c in cells {
                read.bump();
                let id = c.flags.link();
                let wanted = id != LinkId::NONE && only.is_none_or(|o| o == id);
                if wanted && !self.pairs[start..].iter().any(|&(i, _)| i == id.index()) {
                    self.pairs.push((id.index(), line));
                }
            }
        }
        self.pairs.sort_unstable_by_key(|&(i, l)| (i, l));
        self.lines.clear();
        self.lines.extend(self.pairs.iter().map(|&(_, l)| l));
    }

    /// **The rows holding each slot, from each row's slots**, `sets` the
    /// window's rows top first: no cell read.
    fn fill_from(&mut self, sets: &[LinkSet], scrolled: Scrolled, back: Rows) {
        self.pairs.clear();
        for (y, set) in sets.iter().enumerate() {
            let line = scrolled.line(back, Row::from_index(y));
            self.pairs
                .extend(set.slots().map(|s| (s.id().index(), line)));
        }
        self.pairs.sort_unstable_by_key(|&(i, l)| (i, l));
        self.lines.clear();
        self.lines.extend(self.pairs.iter().map(|&(_, l)| l));
    }

    /// Slot `id`'s rows, top first.
    pub fn rows(&self, id: LinkId) -> &[AbsLine] {
        let from = self.pairs.partition_point(|&(i, _)| i < id.index());
        let to = self.pairs.partition_point(|&(i, _)| i <= id.index());
        &self.lines[from..to]
    }
}

/// **What finds links in a line's text.** Only ever called with what the
/// frame path may read: it calls nothing that blocks.
pub trait Reader {
    /// Whether the line is worth reading, from `tail`: the last eight ASCII
    /// bytes read, lowered, the newest in the low byte, a byte outside ASCII
    /// a zero. Called for each cell of a changed row, so it answers at once.
    fn wants(&self, tail: Tail) -> bool;

    /// The links in `text`, as ranges of its units with their marks, in
    /// order, none overlapping. Returns how many candidates it looked at.
    fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> crate::paths::Looked;

    /// How an `OSC 8` link's target at `at` is drawn: none for one that
    /// would not open.
    fn marked(&mut self, uri: &[u8], at: At<'_>) -> Option<Mark>;

    /// A scan found a row of the window changed: called once a scan, before
    /// any row is read, so it costs a frame one call however many changed.
    fn changed(&mut self) {}

    /// Whether the row the writer ended at `above` may be read on into the
    /// next, as `PathsAt::joins` says: yes with nothing to go by.
    fn joins(&self, _screen: Screen, _above: AbsLine) -> Joins {
        Joins::Yes
    }

    /// The path a [`Key::Path`] run the last `read` found was looked up as.
    fn key(&self, _at: Range<usize>) -> &[u8] {
        &[]
    }

    /// Whether the answer kept for `key` draws its line now.
    fn drawn(&self, _key: &[u8]) -> Drawn {
        Drawn::No
    }

    /// **The lines whose facts a read's filing moved since the last scan**,
    /// taken: what a link on them reads may have moved with no change to
    /// their cells (`Paths::written_remote`). None with nothing to go by.
    fn refiled(&mut self) -> Option<crate::paths::LineSpan> {
        None
    }
}

/// **URIs, by `crate::link`'s rules**: `find`'s for what one is and `admit`'s
/// for whether it opens.
///
/// **This machine's name is given to it and never read by it.** The call that
/// answers may wait on a lookup, so the driver reads it at startup, on macOS
/// on a thread of its own (`Hosts`), and hands them over with [`crate::link::hand_names`].
/// Until then a `file://` link naming a host is what `admit` makes of an
/// unknown host: refused, with no line, and `file:///` and `localhost` open.
///
/// **Your `[link.*]` rules are handed to it too** ([`Uris::set_rules`]): a
/// scheme a rule adds is read and drawn as baud's own are, and admitted by
/// the same gate a click goes through.
#[derive(Debug, Default)]
pub struct Uris {
    host: Option<crate::link::MachineNames>,
    bytes: Vec<u8>,
    rules: Rules,
}

impl Uris {
    /// This machine's names, once known: a line they change is read again
    /// by the caller's [`Lines::forget`].
    pub fn set_names(&mut self, names: crate::link::MachineNames) {
        self.host = Some(names);
    }

    /// Whether this machine's name has been handed over yet.
    #[cfg(test)]
    pub fn host_known(&self) -> bool {
        self.host.is_some()
    }

    /// What `admit` reads a `file://` host against: none before the name
    /// is known.
    pub fn names(&self) -> &crate::link::MachineNames {
        static NONE: crate::link::MachineNames = crate::link::MachineNames::NONE;
        self.host.as_ref().unwrap_or(&NONE)
    }

    /// Your `[link.*]` rules, as a load read them: a line they change is
    /// read again by the caller's [`Lines::forget`].
    pub fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
    }

    /// The rules the lines, and so a click, admit with.
    pub fn rules(&self) -> &Rules {
        &self.rules
    }
}

/// URIs alone, with no worker to have named who printed a line: every line
/// taken as this machine's. The drivers read through `paths::Links`, so
/// only tests read through this.
#[cfg(test)]
impl Reader for Uris {
    #[inline]
    fn wants(&self, tail: Tail) -> bool {
        Uris::wants(self, tail)
    }

    fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> crate::paths::Looked {
        let local = |_| crate::paths::LinkOrigin::given(crate::paths::Origin::Local);
        self.read_marking(text, found, &local, &|_| None, &|_| Some(Mark::Line))
    }

    fn marked(&mut self, uri: &[u8], _: At<'_>) -> Option<Mark> {
        let local = crate::paths::LinkOrigin::given(crate::paths::Origin::Local);
        admit_link(uri, self.names(), &self.rules, local)
            .is_ok()
            .then_some(Mark::Line)
    }
}

impl Uris {
    /// Whether a URI may end where `tail` does: a scheme's name runs back
    /// from it ([`Reader::wants`]).
    #[inline]
    pub fn wants(&self, tail: Tail) -> bool {
        scheme_ends(tail, &self.rules)
    }

    /// [`Reader::read`], each URI a click opens marked as `mark` says of its
    /// target, judged as its click is with `origin` of the lines it covers,
    /// where the program that printed them ran ([`crate::link::admit_link`]),
    /// the most cautious over them, so a `file://` URI
    /// a click refuses is unmarked: a reader that knows which kinds have no
    /// app, or which rules' commands are missing, strikes those. One a
    /// click opens on the machine of the ssh session that printed it,
    /// which `session` finds for its origin, is lined, and one it opens
    /// nothing for there is not ([`crate::link::remote`]).
    pub fn read_marking<'s>(
        &mut self,
        text: Text<'_>,
        found: &mut Vec<Run>,
        origin: &dyn Fn((AbsLine, AbsLine)) -> crate::paths::LinkOrigin,
        session: &dyn Fn(crate::paths::LinkOrigin) -> Option<&'s crate::ssh::Session>,
        mark: &dyn Fn(&Target<'_>) -> Option<Mark>,
    ) -> crate::paths::Looked {
        let Text {
            units, head, tail, ..
        } = text;
        let (n, mut i, mut candidates) = (units.len(), 0, 0);
        while i < n {
            if !class(units[i]).has(Class::URI) {
                i += 1;
                continue;
            }
            let lo = i;
            while i < n && class(units[i]).has(Class::URI) {
                i += 1;
            }
            let hi = i;
            // A run past the cap, or one the walk's end touches, is refused
            // whole, as a click anywhere in it may be.
            if hi - lo > MAX || (lo == 0 && head == Edge::Cut) || (hi == n && tail == Edge::Cut) {
                continue;
            }
            let (mut from, mut q) = (lo, lo);
            while q < hi {
                let starts = (q == 0 || !class(units[q - 1]).has(Class::SCHEME))
                    && scheme_at(&units[q..hi], &self.rules).is_some();
                if !starts {
                    q += 1;
                    continue;
                }
                candidates += 1;
                let Some(f) = found_in(
                    units,
                    (crate::link::Ix(lo), crate::link::Ix(hi)),
                    crate::link::Ix(from),
                    crate::link::Ix(q),
                    (head, tail),
                    &self.rules,
                ) else {
                    from = q + 1;
                    q = from;
                    continue;
                };
                let (span, verdict) = f.parts();
                let end = span.end.max(q + 1);
                if verdict.is_ok() {
                    self.bytes.clear();
                    self.bytes
                        .extend(units[span.clone()].iter().filter_map(|u| match u {
                            Unit::Char(c) => u8::try_from(*c).ok(),
                            Unit::Marked | Unit::Gap => None,
                        }));
                    let bytes = core::mem::take(&mut self.bytes);
                    let lines = text.lines_of(&span);
                    let by = origin(lines);
                    let (names, rules) = (self.names(), &self.rules);
                    let m = match crate::link::remote(&bytes, names, rules, by, session(by)) {
                        Some(window) => window.ok().map(|_| Mark::Line),
                        None => admit_link(&bytes, names, rules, by)
                            .ok()
                            .and_then(|t| mark(&t)),
                    };
                    if let Some(m) = m {
                        found.push(Run {
                            units: span,
                            mark: m,
                            key: Key::Fixed,
                        });
                    }
                    self.bytes = bytes;
                }
                from = end;
                q = end;
            }
        }
        crate::paths::Looked(candidates)
    }
}

/// **What a scan read**, each a count for this frame: what the bound tests and
/// the `frame` bench read, the same counters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Scan {
    /// Rows read, the window's and past it.
    pub rows: usize,
    /// Cells read in them.
    pub cells: usize,
    /// Places a reader looked at for a link.
    pub candidates: usize,
    /// Runs of lines this scan added.
    pub spans: usize,
    /// Runs struck this scan added.
    pub struck: usize,
    /// Printed paths' runs a batch of answers drew or took away, with no
    /// row read.
    pub remarks: usize,
    /// Batches of answers that read every row again instead: one changed a
    /// run across a row its writer ended.
    pub rereads: usize,
    /// Cells read for the link slots they name, by the view's syncs after
    /// the last scan ([`ViewLinks::sync`]): a row's, when it was written or
    /// came into view, to the row's mark and its last cell.
    pub slot_cells: usize,
    /// Cells this scan's `OSC 8` runs were read from: a linked row's, when
    /// it was read, to the row's mark, and to its end only where its last
    /// cell names a link.
    pub link_cells: usize,
}

/// How far the window moved over the lines since the last scan, in rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shift {
    /// Output retired lines: what row `y + k` showed is on row `y`.
    Up(usize),
    /// A look back: what row `y` showed is on row `y + k`.
    Down(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Read {
    #[default]
    Kept,
    Again,
}

/// **What the kept copy (`cells`, `wraps`, `sizes`) was taken under**: the
/// seal the scan that took it read the terminal at, and the physical rows
/// its window showed, moved with the copy by each shift since. A row of
/// the copy is kept with no compare only under both ([`Lines::unchanged`]).
#[derive(Clone, Copy, Debug, Default)]
enum Copied {
    /// Nothing to trust: every row is compared.
    #[default]
    Nothing,
    /// `term.sealed()` at the scan that took the copy, and the window it
    /// was taken over.
    Under { seal: Seal, at: WindowAt },
}

/// **Whether a kept row holds what its physical row held at the seal**: a
/// row a shift opened or blanked holds nothing it was copied from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    Copy,
    Open,
}

/// **What the view's link slots were read under**: the count of lines
/// retired, how far it looks back, the grid, what names a slot, and the
/// core's counts of bytes taken and cells written. Each of them the same is
/// a view no byte, write, resize or sweep has touched after the read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ViewKey {
    scrolled: Scrolled,
    back: Rows,
    geometry: (Cols, Rows),
    epoch: LinkEpoch,
    taken: crate::link::Taken,
    written: crate::link::Taken,
}

impl ViewKey {
    fn of(term: &Term<'_>, scrolled: Scrolled) -> ViewKey {
        let (taken, written) = term.amplification();
        ViewKey {
            scrolled,
            back: term.window_back(),
            geometry: term.geometry(),
            epoch: term.link_epoch(),
            taken: crate::link::Taken(taken),
            written: crate::link::Taken(written),
        }
    }
}

/// The seal and the window a [`ViewLinks`] read its rows under, and the
/// view it read.
#[derive(Clone, Copy, Debug)]
struct Synced {
    seal: Seal,
    window: WindowAt,
    key: ViewKey,
}

/// **The link slots each row of the view shows**, one [`LinkSet`] a row,
/// kept by the rows' write generations: a row's cells are read again only
/// when a store touched the row after the seal it was read under, a move of
/// the window brought it in, or a sweep moved what a slot names. What the
/// scan ([`Lines::scan`]) and the link under the pointer
/// ([`crate::link::Osc8::view_rows`]) read the rows holding a slot from,
/// through a [`Fresh`].
#[derive(Debug, Default)]
pub struct ViewLinks {
    sets: Vec<LinkSet>,
    /// Whether each row's set is of the physical row the window shows
    /// there, as read: a row a move of the window brought in is not.
    held: Vec<Held>,
    synced: Option<Synced>,
    /// Cells read for the slots they name that no scan has counted yet
    /// ([`Scan::slot_cells`]).
    unscanned: core::cell::Cell<usize>,
}

impl ViewLinks {
    /// **The sets brought to the view shown now**, `scrolled` the driver's
    /// count of lines retired: at once, reading nothing, while the view is
    /// the one last read; else the rows the window's move kept are moved
    /// with it, and each row brought in or written after the last read's
    /// seal is read again.
    pub fn sync<'v>(&'v mut self, term: &Term<'_>, scrolled: Scrolled) -> Fresh<'v> {
        let key = ViewKey::of(term, scrolled);
        if let Some(s) = self.synced.as_mut().filter(|s| s.key == key) {
            // Nothing written after the read: what a later seal closes was
            // read, so the rows are kept under it.
            s.seal = term.sealed();
            return Fresh { links: self, key };
        }
        let back = key.back;
        let view = term.view(back);
        let rows = view.rows().index();
        let gens = term.row_gens(back);
        let now = gens.window();
        let kept = match self.synced {
            Some(s) if s.key.geometry == key.geometry && s.key.epoch == key.epoch => {
                self.moved(s.window, now)
            }
            _ => Kept::Nothing,
        };
        if kept == Kept::Nothing || self.sets.len() != rows {
            self.sets.clear();
            self.sets.resize(rows, LinkSet::EMPTY);
            self.held.clear();
            self.held.resize(rows, Held::Open);
        }
        for y in 0..rows {
            let r = Row::from_index(y);
            let written = self.synced.is_none_or(|s| gens.after(r, &s.seal));
            if self.held[y] == Held::Open || written {
                let row = view.row(r);
                let reach = slot_reach(row, view.extent(r));
                let mut set = LinkSet::of_cells(reach);
                let mut read = reach.len();
                // Past the mark every cell is the last one.
                if read < row.len() && view.extent(r).links() == RowLinks::May {
                    set = set.union(LinkSet::of_cells(&row[row.len() - 1..]));
                    read += 1;
                }
                self.sets[y] = set;
                self.unscanned.set(self.unscanned.get() + read);
            }
        }
        self.held.fill(Held::Copy);
        self.synced = Some(Synced {
            seal: term.sealed(),
            window: now,
            key,
        });
        Fresh { links: self, key }
    }

    /// **The sets as last read, while the view shown now is the one they
    /// were read of**; none once a byte, a write, a resize, a sweep or a
    /// move of the window came after.
    pub fn fresh(&self, term: &Term<'_>, scrolled: Scrolled) -> Option<Fresh<'_>> {
        let key = ViewKey::of(term, scrolled);
        self.synced
            .filter(|s| s.key == key)
            .map(|_| Fresh { links: self, key })
    }

    /// **The rows `was` showed moved to where `now` shows them**: the
    /// window moved by fewer rows than it has over one ring, and the rows
    /// it brought in marked to be read. Nothing kept where the window is
    /// on another screen, another ring or another height, or moved a
    /// window or more.
    fn moved(&mut self, was: WindowAt, now: WindowAt) -> Kept {
        if was == now {
            return Kept::Rows;
        }
        if was.screen != now.screen || was.ring != now.ring || was.rows != now.rows {
            return Kept::Nothing;
        }
        let (ring, rows) = (now.ring.index(), now.rows.index());
        if self.sets.len() != rows || ring == 0 {
            return Kept::Nothing;
        }
        let d = (now.top.index() + ring - was.top.index()) % ring;
        if d < rows {
            self.sets.copy_within(d.., 0);
            self.held.copy_within(d.., 0);
            self.held[rows - d..].fill(Held::Open);
        } else if ring - d < rows {
            let k = ring - d;
            self.sets.copy_within(..rows - k, k);
            self.held.copy_within(..rows - k, k);
            self.held[..k].fill(Held::Open);
        } else {
            return Kept::Nothing;
        }
        Kept::Rows
    }
}

/// Whether a move of the window kept any row's set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kept {
    Nothing,
    Rows,
}

/// **The view's link slots, read for the view shown now**: made only by
/// [`ViewLinks::sync`], or by [`ViewLinks::fresh`] while nothing moved.
/// A reader holding one reads the rows holding a slot with no cell read.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'v> {
    links: &'v ViewLinks,
    key: ViewKey,
}

impl<'v> Fresh<'v> {
    /// Row `r`'s slots; none past the view.
    pub fn row(&self, r: Row) -> LinkSet {
        self.links
            .sets
            .get(r.index())
            .copied()
            .unwrap_or(LinkSet::EMPTY)
    }

    /// **The lines of the rows showing slot `s`**, top first.
    pub fn rows_of(&self, s: LinkSlot) -> impl Iterator<Item = AbsLine> + 'v {
        let (scrolled, back) = (self.key.scrolled, self.key.back);
        self.links
            .sets
            .iter()
            .enumerate()
            .filter(move |(_, set)| set.has(s))
            .map(move |(y, _)| scrolled.line(back, Row::from_index(y)))
    }

    /// The rows showing a slot of `set`, top first.
    pub fn meeting(&self, set: LinkSet) -> impl Iterator<Item = Row> + 'v {
        self.links
            .sets
            .iter()
            .enumerate()
            .filter(move |(_, s)| s.meets(set))
            .map(|(y, _)| Row::from_index(y))
    }

    /// Whether these are the slots of the view `scrolled` lines retired and
    /// `back` lines into the history.
    pub fn shows(&self, scrolled: Scrolled, back: Rows) -> bool {
        (self.key.scrolled, self.key.back) == (scrolled, back)
    }
}
/// **The lines on the window, and the window as the last scan saw it.**
#[derive(Debug, Default)]
pub struct Lines {
    /// The view's rows holding each `OSC 8` slot, read once a scan when a
    /// row read holds a link, and each slot's mark as judged this scan.
    slot_rows: SlotRows,
    slots_read: SlotsRead,
    judged: SlotMarks<Option<Mark>>,
    /// The slots this scan found shown on other rows than the last scan
    /// did, or on a row read again: every row showing one is read again.
    slots_moved: LinkSet,
    cells: Vec<PackedCell>,
    /// The slots each row of `cells` names, the view's for the row when
    /// the row is copied, and moved with it.
    sets: Vec<LinkSet>,
    wraps: Vec<Wrap>,
    sizes: Vec<LineRendition>,
    size: (usize, usize),
    epochs: Option<(LinkEpoch, ClusterEpoch)>,
    /// Which rows this scan reads.
    again: Vec<Read>,
    /// The runs to underline, as [`Lines::lines`] hands them out: the
    /// fixed ones and the waiting ones drawn, by row and column.
    lines: Vec<LinkLine>,
    /// The runs whose line waits on no answer: URIs' and `OSC 8` links'.
    fixed: Vec<LinkLine>,
    struck: Vec<LinkLine>,
    /// Printed paths' runs, drawn or not, and the paths and pieces of line
    /// they hold.
    waits: Vec<Wait>,
    keys: Vec<u8>,
    pieces: Vec<LinkLine>,
    /// One run's pieces, before they are kept.
    run_pieces: Vec<LinkLine>,
    answers: Answers,
    /// The count and the view the last scan numbered its rows by: what a
    /// row's line was then, so a row moved since is compared with what its
    /// line held.
    at: Option<(Scrolled, Rows)>,
    units: Vec<Unit>,
    points: Vec<Point>,
    found: Vec<Run>,
    groups: Vec<Range<usize>>,
    /// How each row of the window met the next as the last scan read it.
    below: Vec<Below>,
    /// The links a read across joined rows kept, first and last cell.
    taken: Vec<(Point, Point)>,
    /// **Where the top row's line began**, above the window, as the last
    /// read of it found: a trim of the history there takes that start, and
    /// the top row's part of a link with it, with no change to a cell of
    /// the window. `None` when the line began on the top row.
    head: Option<AbsLine>,
    /// **The rows below the window the last read of its bottom row went
    /// by**: the first such line, and each row's cells and wrap, packed. On
    /// a look back the screen below the window is written to, and the
    /// bottom row's links read on into it, or end where it starts, with no
    /// change to a cell of the window. `None` with no row below.
    past: Option<AbsLine>,
    past_cells: Vec<PackedCell>,
    past_rows: Vec<(usize, Wrap)>,
    /// Rows whose cells the last scan compared with what it kept: the
    /// window's, and those below it a look back went by
    /// ([`Lines::compared`]).
    compared: Compared,
    /// What the kept copy was taken under, and whether each of its rows
    /// holds what it was copied from: a row of the window no store has
    /// touched since is kept with no compare.
    copied: Copied,
    held: Vec<Held>,
}

/// **Rows a scan compared with what it kept** ([`Lines::compared`]): a
/// count of rows, none before the first scan.
#[derive(Clone, Copy, Debug)]
struct Compared(Rows);

impl Default for Compared {
    fn default() -> Compared {
        Compared(Rows::ZERO)
    }
}

impl Compared {
    /// One row more.
    fn one(&mut self) {
        self.0 = Rows::of_count(self.0.index() + 1);
    }
}

/// **How a row the writer ended meets the next**, as a scan read it: not
/// running on in a URI's characters (`link::runs_on`), running on and joined,
/// or running on and held apart by `[link] join_rows` or provenance -- which
/// a later scan asks again, since a program is named after it wrote.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Below {
    #[default]
    Apart,
    Joined,
    Held,
}

#[cfg(test)]
impl Lines {
    /// A scan through the view's slots read for it alone, whose reads are
    /// the sync's and not the scan's.
    pub(crate) fn scan_alone<R: Reader>(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        reader: &mut R,
    ) -> Scan {
        let mut views = ViewLinks::default();
        let fresh = views.sync(term, scrolled);
        fresh.links.unscanned.set(0);
        self.scan(term, scrolled, &fresh, reader)
    }
}

impl Lines {
    /// The runs to underline, by row and column.
    pub fn lines(&self) -> &[LinkLine] {
        &self.lines
    }

    /// The runs to strike, by row and column: links baud knows would open
    /// nothing.
    pub fn struck(&self) -> &[LinkLine] {
        &self.struck
    }

    /// **Rows whose cells the last scan compared with what it kept**: the
    /// window's, and those below it that the bottom row's last read went
    /// by. What a frame costs the scan where it reads no row; kept beside
    /// [`Scan`], whose equality says what a frame read.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn compared(&self) -> Rows {
        self.compared.0
    }

    /// Read every row at the next scan.
    pub fn forget(&mut self) {
        self.epochs = None;
    }

    /// **Whether window row `r` still holds what the copy kept**, with no
    /// compare: the copy taken over these physical rows exactly, the row
    /// not opened by a shift since, and no store to it after the seal the
    /// copy was taken under. The one way a row skips the compare.
    fn unchanged(&self, gens: &RowGens<'_>, r: Row) -> bool {
        match self.copied {
            Copied::Nothing => false,
            Copied::Under { seal, at } => {
                at == gens.window()
                    && self.held.get(r.index()) == Some(&Held::Copy)
                    && !gens.after(r, &seal)
            }
        }
    }

    /// **The last scan's rows moved `shift` rows**, to where the lines they
    /// showed are now: the cells, wraps, sizes and how each met the next, and
    /// the lines and struck runs on them; the rows left open hold nothing a row can equal but a
    /// blank one, which has no link.
    fn shift(&mut self, shift: Shift) {
        let (cols, rows) = self.size;
        let (k, up) = match shift {
            Shift::Up(k) => (k.min(rows), true),
            Shift::Down(k) => (k.min(rows), false),
        };
        let keep = rows - k;
        // The physical rows the copy is of move with it.
        if let Copied::Under { at, .. } = &mut self.copied {
            at.top = match up {
                true => at.top.advance(k, at.ring),
                false => at.top.retreat(k, at.ring),
            };
        }
        // The slots on the rows leaving the window are shown on fewer rows
        // now: each is judged again on the rows it keeps.
        let leaving = match up {
            true => 0..k,
            false => keep..rows,
        };
        for &set in &self.sets[leaving] {
            self.slots_moved = self.slots_moved.union(set);
        }
        // The row that ran on into the new top row left the window: what the
        // top row's links are now reads the lines above the window, which a
        // trim of the history may have taken, so the top row is read again.
        let ran_on = up
            && k > 0
            && (self.wraps[k - 1] == Wrap::Continues || self.below[k - 1] == Below::Joined);
        // A look back: the rows that leave the window below are the rows
        // below it now, as the last scan saw them -- the first, whose first
        // cell says whether the new bottom row runs on, and each after it
        // its line runs on into -- and the rows below those, while they ran
        // on too.
        if !up && k > 0 && keep > 0 {
            let continues =
                |y: usize| self.wraps[y] == Wrap::Continues || self.below[y] == Below::Joined;
            let mut n = 1;
            while n < k && continues(keep + n - 1) {
                n += 1;
            }
            match n == k && continues(rows - 1) && self.past.is_some() {
                true => {}
                false => {
                    self.past_cells.clear();
                    self.past_rows.clear();
                }
            }
            self.past_cells.splice(
                0..0,
                self.cells[keep * cols..(keep + n) * cols].iter().copied(),
            );
            self.past_rows
                .splice(0..0, (keep..keep + n).map(|y| (cols, self.wraps[y])));
        }
        if up {
            self.cells.copy_within(k * cols.., 0);
            self.sets.copy_within(k.., 0);
            self.wraps.copy_within(k.., 0);
            self.sizes.copy_within(k.., 0);
            self.below.copy_within(k.., 0);
            self.held.copy_within(k.., 0);
            self.held[keep..].fill(Held::Open);
            self.cells[keep * cols..].fill(PackedCell::EMPTY);
            self.sets[keep..].fill(LinkSet::EMPTY);
            self.wraps[keep..].fill(Wrap::Ended);
            self.sizes[keep..].fill(LineRendition::Single);
            self.below[keep..].fill(Below::Apart);
        } else {
            self.cells.copy_within(..keep * cols, k * cols);
            self.sets.copy_within(..keep, k);
            self.wraps.copy_within(..keep, k);
            self.sizes.copy_within(..keep, k);
            self.below.copy_within(..keep, k);
            self.held.copy_within(..keep, k);
            self.held[..k].fill(Held::Open);
            self.cells[..k * cols].fill(PackedCell::EMPTY);
            self.sets[..k].fill(LinkSet::EMPTY);
            self.wraps[..k].fill(Wrap::Ended);
            self.sizes[..k].fill(LineRendition::Single);
            self.below[..k].fill(Below::Apart);
        }
        if ran_on && keep > 0 {
            self.cells[..cols].fill(PackedCell::EMPTY);
            self.sets[0] = LinkSet::EMPTY;
            self.held[0] = Held::Open;
        }
        self.keep_waits(|r| {
            match up {
                true => r.index().checked_sub(k),
                false => Some(r.index() + k).filter(|&y| y < rows),
            }
            .map(Row::from_index)
        });
        for runs in [&mut self.fixed, &mut self.struck] {
            runs.retain_mut(|l| {
                let y = l.row.index();
                let moved = match up {
                    true => y.checked_sub(k),
                    false => Some(y + k).filter(|&y| y < rows),
                };
                match moved {
                    Some(y) => {
                        l.row = Row::from_index(y);
                        true
                    }
                    None => false,
                }
            });
        }
    }

    /// **`[link] urls = "off"`**: no line, and nothing read. The scan's
    /// memory is kept, since a config read again may turn it back on.
    pub fn off(&mut self) {
        self.lines.clear();
        self.fixed.clear();
        self.struck.clear();
        self.waits.clear();
        self.keys.clear();
        self.pieces.clear();
        self.forget();
    }

    /// **Whether the rows below the window the last read of its bottom row
    /// went by are not what it saw**: another line below it now, one gone,
    /// or one written over.
    fn past_moved(&mut self, term: &Term<'_>, scrolled: Scrolled, back: Rows) -> bool {
        let rows = self.size.1;
        let below = scrolled.line(back, Row::from_index(rows - 1)).below();
        let now = row_of(term, scrolled, below).map(|_| below);
        if now != self.past {
            return true;
        }
        let (mut line, mut at) = (below, 0);
        for &(len, wrap) in &self.past_rows {
            let Some((cells, w)) = row_of(term, scrolled, line) else {
                return true;
            };
            self.compared.one();
            if w != wrap || cells != &self.past_cells[at..at + len] {
                return true;
            }
            at += len;
            line = line.below();
        }
        false
    }

    /// **A batch of answers came**, with no cell changed: the next scan
    /// draws or takes away each printed path's run it keeps by the answer
    /// kept for its path, and reads no row for it.
    pub fn answered(&mut self) {
        self.answers = Answers::Came;
    }

    /// **The kept paths' runs moved by `moved`**, a row to where it is now
    /// or none for one off the window: a run with no piece left goes, and
    /// the paths and pieces are packed down over what went, in place.
    fn keep_waits(&mut self, mut moved: impl FnMut(Row) -> Option<Row>) {
        let (mut w, mut k, mut p) = (0, 0, 0);
        for i in 0..self.waits.len() {
            let wait = self.waits[i].clone();
            let from = p;
            for j in wait.pieces.clone() {
                let mut piece = self.pieces[j];
                if let Some(row) = moved(piece.row) {
                    piece.row = row;
                    self.pieces[p] = piece;
                    p += 1;
                }
            }
            if p == from {
                continue;
            }
            let len = wait.key.len();
            self.keys.copy_within(wait.key, k);
            self.waits[w] = Wait {
                key: k..k + len,
                pieces: from..p,
                ..wait
            };
            (w, k) = (w + 1, k + len);
        }
        self.waits.truncate(w);
        self.keys.truncate(k);
        self.pieces.truncate(p);
    }

    /// **The runs to underline, made again**: the fixed ones and the kept
    /// paths' runs drawn, by row and column.
    fn publish(&mut self) {
        self.lines.clear();
        self.lines.extend_from_slice(&self.fixed);
        for w in &self.waits {
            if w.drawn == Drawn::Yes {
                self.lines.extend_from_slice(&self.pieces[w.pieces.clone()]);
            }
        }
        self.lines
            .sort_unstable_by_key(|l| (l.row.index(), l.from.index()));
    }

    /// **The scan**, on the view shown now, `scrolled` the driver's count of
    /// lines retired, `fresh` the view's link slots read for it. Allocates
    /// nothing once its buffers have grown to the window and to the longest
    /// line read.
    pub fn scan<R: Reader>(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        fresh: &Fresh<'_>,
        reader: &mut R,
    ) -> Scan {
        self.slots_read = SlotsRead::No;
        self.slots_moved = LinkSet::EMPTY;
        // The slots the view's rows name are read before the scan, by the
        // syncs that made `fresh` and those before it: their reads not yet
        // counted are this scan's.
        let mut scan = Scan {
            slot_cells: fresh.links.unscanned.take(),
            ..Scan::default()
        };
        self.compared = Compared::default();
        let back = term.window_back();
        let view = term.view(back);
        let (cols, rows) = (view.cols().index(), view.rows().index());
        if self.size != (cols, rows) {
            self.size = (cols, rows);
            self.cells.clear();
            self.cells.resize(cols * rows, PackedCell::EMPTY);
            self.sets.clear();
            self.sets.resize(rows, LinkSet::EMPTY);
            self.wraps.clear();
            self.wraps.resize(rows, Wrap::Ended);
            self.sizes.clear();
            self.sizes.resize(rows, LineRendition::Single);
            self.again.clear();
            self.again.resize(rows, Read::Kept);
            self.below.clear();
            self.below.resize(rows, Below::Apart);
            self.held.clear();
            self.held.resize(rows, Held::Open);
            self.copied = Copied::Nothing;
            self.head = None;
            self.past = None;
            self.past_cells.clear();
            self.past_rows.clear();
            self.lines.clear();
            self.fixed.clear();
            self.struck.clear();
            self.waits.clear();
            self.keys.clear();
            self.pieces.clear();
            self.epochs = None;
            self.at = None;
        }
        // The window moved over the lines since the last scan, by output
        // that retired lines or by a look back: what the last scan saw of a
        // line is moved to the row showing it now, its lines with it.
        let window = Rows::of_index(rows);
        // Whether the runs handed out change this scan with no row read.
        let mut moved = false;
        if let Some((was, was_back)) = self.at {
            let top = scrolled.line(back, Row::ZERO);
            let was_top = was.line(was_back, Row::ZERO);
            match (
                was.window_row(was_back, top, window),
                scrolled.window_row(back, was_top, window),
            ) {
                (Some(up), _) if up.index() > 0 => {
                    self.shift(Shift::Up(up.index()));
                    moved = true;
                }
                (_, Some(down)) if down.index() > 0 => {
                    self.shift(Shift::Down(down.index()));
                    self.past = (down.index() < rows)
                        .then(|| scrolled.line(back, Row::from_index(rows - 1)).below());
                    moved = true;
                }
                // Not moved, or a window or more: each row is compared with
                // what the same row held, as cells alone say what a row's
                // links are.
                _ => {}
            }
        }
        self.at = Some((scrolled, back));
        // A slot of the link table or the cluster store freed and taken again
        // moves what a cell's word means, with no change to the word.
        let epochs = (term.link_epoch(), term.cluster_epoch());
        let all = self.epochs != Some(epochs);
        self.epochs = Some(epochs);
        let mut any = false;
        // A wrap come or gone on the row above moves where this row's logical
        // line starts, with no change to its cells: a wrap gone leaves it a
        // line of its own, whose old line must go.
        let mut wrap_moved = false;
        // A row no store has touched since the copy was taken is kept with
        // no compare of its cells; its wrap and size are read still.
        let gens = term.row_gens(back);
        for y in 0..rows {
            let r = Row::from_index(y);
            let row = view.row(r);
            let n = row.len().min(cols);
            let clean = !all && self.unchanged(&gens, r);
            let kept = &mut self.cells[y * cols..y * cols + n];
            let (wrap, size) = (view.wrapped(r), view.rendition(r));
            let compare = !all && !wrap_moved && !clean;
            if compare {
                self.compared.one();
            }
            let same = !all
                && !wrap_moved
                && (clean || kept == &row[..n])
                && self.wraps[y] == wrap
                && self.sizes[y] == size;
            wrap_moved = self.wraps[y] != wrap;
            self.again[y] = match same {
                true => Read::Kept,
                false => Read::Again,
            };
            if !same {
                kept.copy_from_slice(&row[..n]);
                // A slot on the row before or after is shown on other rows
                // now.
                let now = fresh.row(r);
                self.slots_moved = self.slots_moved.union(self.sets[y]).union(now);
                self.sets[y] = now;
                self.wraps[y] = wrap;
                self.sizes[y] = size;
                any = true;
            }
        }
        // Every row of the copy is what its physical row holds now: a store
        // between the last seal and this scan reads as after this seal
        // too, and is compared again next frame.
        self.copied = Copied::Under {
            seal: term.sealed(),
            at: gens.window(),
        };
        self.held.fill(Held::Copy);
        // A row running on into the next is asked again whether it joins,
        // with no change to either: the program that wrote it may have been
        // named since, and a remote row never joins a local one.
        let meets = |reader: &R, y: usize| {
            let line = scrolled.line(back, Row::from_index(y));
            match runs_on(term, scrolled, line) {
                false => Below::Apart,
                true => match reader.joins(term.screen(), line) {
                    Joins::Yes => Below::Joined,
                    Joins::No => Below::Held,
                },
            }
        };
        for y in 0..rows.saturating_sub(1) {
            let kept = self.again[y] == Read::Kept && self.again[y + 1] == Read::Kept;
            if kept && self.below[y] != Below::Apart && meets(reader, y) != self.below[y] {
                self.again[y] = Read::Again;
                self.again[y + 1] = Read::Again;
                any = true;
            }
        }
        // A row below the window that the bottom row's last read went by was
        // written over, or is another line now: the bottom row is read again.
        if rows > 0 && self.again[rows - 1] == Read::Kept && self.past_moved(term, scrolled, back) {
            self.again[rows - 1] = Read::Again;
            any = true;
        }
        // The history trimmed where the top row's line began: the top row is
        // read again, and its line now begins at the oldest line kept.
        if let (Some(head), Some(Read::Kept)) = (self.head, self.again.first()) {
            if row_of(term, scrolled, head).is_none() {
                self.again[0] = Read::Again;
                any = true;
            }
        }
        // A read filed facts over rows whose cells it left as they were: a
        // remote read of renditions alone over a row holding a `file://`
        // link. Those rows are read again, and with them, below, the rows
        // their joins and slots reach.
        if let Some(span) = reader.refiled() {
            for (y, again) in self.again.iter_mut().enumerate() {
                let line = scrolled.line(back, Row::from_index(y));
                if *again == Read::Kept && span.holds(line) {
                    *again = Read::Again;
                    any = true;
                }
            }
        }
        // A row that joined the next and changed takes the join with it: the
        // row below was read as part of its link, and is read again alone.
        for y in 0..rows.saturating_sub(1) {
            if self.again[y] == Read::Again && self.below[y] == Below::Joined {
                self.again[y + 1] = Read::Again;
            }
        }
        // A batch of answers came: each printed path's run kept is drawn by
        // the answer kept for its path, with no row read. One across a row
        // its writer ended took its cells from the reads of each line alone,
        // which only a read again gives back, so a change to one reads
        // every row.
        if self.answers == Answers::Came {
            self.answers = Answers::None;
            let mut across = Crosses::No;
            for w in &mut self.waits {
                let drawn = reader.drawn(&self.keys[w.key.clone()]);
                if drawn != w.drawn {
                    w.drawn = drawn;
                    scan.remarks += 1;
                    moved = true;
                    if w.crosses == Crosses::Yes {
                        across = Crosses::Yes;
                    }
                }
            }
            if across == Crosses::Yes {
                self.again.fill(Read::Again);
                scan.rereads += 1;
                any = true;
            }
        }
        // **A slot is judged over every row of the view showing it**, so a
        // row read again, or a row come or gone, holding a slot moves the
        // look of every row showing it: those are read again too, and one
        // slot has one look on every row.
        for y in 0..rows {
            if self.again[y] == Read::Again {
                self.slots_moved = self.slots_moved.union(self.sets[y]);
            }
        }
        if !self.slots_moved.is_empty() {
            for r in fresh.meeting(self.slots_moved) {
                if self.again[r.index()] == Read::Kept {
                    self.again[r.index()] = Read::Again;
                    any = true;
                }
            }
        }
        if !any {
            if moved {
                self.publish();
            }
            return scan;
        }
        reader.changed();
        for y in 0..rows.saturating_sub(1) {
            if self.again[y] == Read::Again || self.again[y + 1] == Read::Again {
                self.below[y] = meets(reader, y);
            }
        }
        if let Some(b) = self.below.last_mut() {
            *b = Below::Apart;
        }
        // A logical line with a changed row is read whole, and so is the one
        // just above one: whether its URI may have been cut, or runs on into
        // this row, reads this row's first cell (`may_be_cut`, `runs_on`). Rows
        // the writer ended that join are one line here, as a soft wrap's are.
        let continues =
            |y: usize| self.wraps[y] == Wrap::Continues || self.below[y] == Below::Joined;
        let mut groups = core::mem::take(&mut self.groups);
        loop {
            groups.clear();
            let mut top = 0;
            for y in 0..rows {
                if y + 1 < rows && continues(y) {
                    continue;
                }
                let (a, b) = (top, y);
                top = y + 1;
                let changed = self.again[a..=b].contains(&Read::Again);
                let under = b + 1 < rows && self.again[b + 1] == Read::Again;
                if changed || under {
                    groups.push(a..b + 1);
                }
            }
            for g in &groups {
                self.again[g.clone()].fill(Read::Again);
            }
            // A group read whole that holds a slot not yet seen this scan
            // reads every row showing it too, and their lines with them.
            let seen = self.slots_moved;
            for y in 0..rows {
                if self.again[y] == Read::Again {
                    self.slots_moved = self.slots_moved.union(self.sets[y]);
                }
            }
            if self.slots_moved == seen {
                break;
            }
            for r in fresh.meeting(self.slots_moved) {
                self.again[r.index()] = Read::Again;
            }
        }
        let again = &self.again;
        self.fixed.retain(|l| again[l.row.index()] == Read::Kept);
        self.struck.retain(|l| again[l.row.index()] == Read::Kept);
        // A run on a row read again is found again there; one with a piece
        // on such a row has all of them on its logical line, read whole.
        let again = core::mem::take(&mut self.again);
        self.drop_waits_on(&again);
        self.again = again;
        let mut found = core::mem::take(&mut self.found);
        for g in &groups {
            let rows = (Row::from_index(g.start), Row::from_index(g.end - 1));
            self.read_group(term, scrolled, back, rows, reader, &mut found, &mut scan);
        }
        self.found = found;
        self.groups = groups;
        self.fixed
            .sort_unstable_by_key(|l| (l.row.index(), l.from.index()));
        self.struck
            .sort_unstable_by_key(|l| (l.row.index(), l.from.index()));
        self.publish();
        scan
    }

    /// The kept paths' runs with a piece on a row `again` reads, taken out.
    fn drop_waits_on(&mut self, again: &[Read]) {
        let pieces = &self.pieces;
        let read = |w: &Wait| {
            pieces[w.pieces.clone()]
                .iter()
                .any(|l| again[l.row.index()] == Read::Again)
        };
        if !self.waits.iter().any(read) {
            return;
        }
        let mut keep = core::mem::take(&mut self.waits);
        keep.retain(|w| !read(w));
        self.waits = keep;
        // Packed down over what went, every row where it is.
        self.keep_waits(Some);
    }

    /// One logical line's links: rows `top..=bottom` of the window, and as
    /// much of it past the window as a URI on it could reach.
    ///
    /// **Rows the writer ended that join** (`link::joins_below`) are read
    /// twice: across the join, where only a link that crosses it is kept,
    /// and each line alone, as if never joined, for everything else. So a
    /// join changes nothing but a link that runs across it and admits there.
    #[allow(clippy::too_many_arguments)]
    fn read_group<R: Reader>(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        back: Rows,
        (top, bottom): (Row, Row),
        reader: &mut R,
        found: &mut Vec<Run>,
        scan: &mut Scan,
    ) {
        let (_, rows) = self.size;
        let first_row = scrolled.line(back, top);
        let last_row = scrolled.line(back, bottom);
        // Whether the line runs on from `l` into the next.
        let on = |reader: &R, l: AbsLine, wrap: Wrap| {
            wrap == Wrap::Continues
                || (runs_on(term, scrolled, l) && reader.joins(term.screen(), l) == Joins::Yes)
        };
        // Past the window's top, while the rows above run into it.
        let (mut first, mut head) = (first_row, Edge::Line);
        if top == Row::ZERO {
            let mut read = 0;
            while let Some(up) = first.above() {
                let Some((cells, wrap)) = row_of(term, scrolled, up) else {
                    break;
                };
                if !on(reader, up, wrap) {
                    break;
                }
                if read > MAX {
                    head = Edge::Cut;
                    break;
                }
                read += cells.len();
                first = up;
            }
            self.head = (first != first_row).then_some(first);
        }
        // Past the window's bottom, while the row runs on.
        let (mut last, mut tail) = (last_row, Edge::Line);
        if bottom.index() + 1 == rows {
            let mut read = 0;
            while let Some((cells, wrap)) = row_of(term, scrolled, last) {
                if !on(reader, last, wrap) {
                    break;
                }
                if read > MAX {
                    tail = Edge::Cut;
                    break;
                }
                let down = last.below();
                if row_of(term, scrolled, down).is_none() {
                    break;
                }
                read += cells.len();
                last = down;
            }
            // What this read went by below the window: the row below, whose
            // first cell says whether the bottom row runs on, and every row
            // read past it.
            let below = last_row.below();
            self.past = row_of(term, scrolled, below).map(|_| below);
            self.past_cells.clear();
            self.past_rows.clear();
            let mut line = below;
            while let Some((cells, wrap)) = row_of(term, scrolled, line) {
                if line > last.max(below) {
                    break;
                }
                self.past_cells.extend_from_slice(cells);
                self.past_rows.push((cells.len(), wrap));
                line = line.below();
            }
        }
        // The one compare a cell, across the rows; and whether the writer
        // ended any of them, which a join read on past.
        let (mut wanted, mut linked, mut bytes) = (false, false, 0u64);
        let mut joined = false;
        let mut line = first;
        while let Some((cells, wrap)) = row_of(term, scrolled, line) {
            scan.rows += 1;
            scan.cells += cells.len();
            for c in cells {
                if c.content.is_wide_tail() {
                    continue;
                }
                linked |= c.flags.link() != LinkId::NONE;
                let b = c
                    .content
                    .as_codepoint()
                    .and_then(|p| u8::try_from(p.value()).ok())
                    .filter(u8::is_ascii)
                    .map_or(0, |b| b.to_ascii_lowercase());
                bytes = (bytes << 8) | u64::from(b);
                wanted |= reader.wants(Tail(bytes));
            }
            if line == last {
                break;
            }
            joined |= wrap != Wrap::Continues;
            line = line.below();
        }
        if linked {
            self.osc8(term, scrolled, back, (top, bottom), reader, scan);
        }
        if !wanted {
            return;
        }
        let window = (first_row, last_row);
        self.taken.clear();
        if joined {
            // Across the joins: a link that crosses one and opens, and nothing
            // else; one struck there is read as if never joined, as the
            // pointer reads it (`link::read_under`).
            self.read_text(
                term,
                scrolled,
                (first, last),
                (head, tail),
                window,
                reader,
                found,
                scan,
            );
            for run in found.iter() {
                let ends = (run.units.start, run.units.end.saturating_sub(1));
                let (Some(&from), Some(&to)) = (self.points.get(ends.0), self.points.get(ends.1))
                else {
                    continue;
                };
                if run.mark == Mark::Line && crosses_a_join(term, scrolled, from, to) {
                    // A path not drawn takes nothing; kept, it reads every
                    // row again once an answer draws it.
                    if !matches!(
                        run.key,
                        Key::Path {
                            drawn: Drawn::No,
                            ..
                        }
                    ) {
                        self.taken.push((from, to));
                    }
                    self.push_run(term, scrolled, back, run, reader, Crosses::Yes, scan);
                }
            }
        }
        // Each line alone, as if never joined: all of it, but what a link
        // across a join already took.
        let mut a = first;
        loop {
            let mut b = a;
            while b < last {
                match row_of(term, scrolled, b) {
                    Some((_, Wrap::Continues)) => b = b.below(),
                    _ => break,
                }
            }
            let edges = (
                if a == first { head } else { Edge::Line },
                if b == last { tail } else { Edge::Line },
            );
            self.read_text(term, scrolled, (a, b), edges, window, reader, found, scan);
            for run in found.iter() {
                let taken = run.units.clone().any(|k| {
                    self.points.get(k).is_some_and(|&p| {
                        self.taken.iter().any(|&(from, to)| {
                            (from.line, from.col.index()) <= (p.line, p.col.index())
                                && (p.line, p.col.index()) <= (to.line, to.col.index())
                        })
                    })
                });
                if !taken {
                    self.push_run(term, scrolled, back, run, reader, Crosses::No, scan);
                }
            }
            if b == last {
                break;
            }
            a = b.below();
        }
    }

    /// The text of lines `a..=b` into the units and their points, read by
    /// `reader` into `found`; `window` the lines the window shows.
    #[allow(clippy::too_many_arguments)]
    fn read_text<R: Reader>(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        (a, b): (AbsLine, AbsLine),
        (head, tail): (Edge, Edge),
        (first_row, last_row): (AbsLine, AbsLine),
        reader: &mut R,
        found: &mut Vec<Run>,
        scan: &mut Scan,
    ) {
        // The text, with where each unit was read from, and which of it the
        // window shows.
        self.units.clear();
        self.points.clear();
        let mut shown = (usize::MAX, 0);
        let mut line = a;
        while let Some((cells, _)) = row_of(term, scrolled, line) {
            let on_window = line >= first_row && line <= last_row;
            if on_window && shown.0 == usize::MAX {
                shown.0 = self.units.len();
            }
            let styles = row_styles_of(term, scrolled, line);
            // The line's last row ends at its last cell with anything in it:
            // the empty cells after it are each a gap, which ends a link and
            // starts none, and a line not cut there is ended by its end too.
            let used = match (line == b, tail) {
                (true, Edge::Line) => cells
                    .iter()
                    .rposition(|c| !c.content.is_empty() || term.cluster_marks(*c).next().is_some())
                    .map_or(0, |p| p + 1),
                _ => cells.len(),
            };
            for (x, &c) in cells[..used].iter().enumerate() {
                if c.content.is_wide_tail() {
                    continue;
                }
                self.units.push(unit(term, styles, c));
                self.points.push(Point::new(line, Col::at_index(x)));
            }
            if on_window {
                shown.1 = self.units.len();
            }
            if line == b {
                break;
            }
            line = line.below();
        }
        if shown.0 == usize::MAX {
            shown.0 = shown.1;
        }
        found.clear();
        scan.candidates += reader
            .read(
                Text {
                    units: &self.units,
                    head,
                    tail,
                    shown,
                    lines: (a, b),
                    points: &self.points,
                    screen: term.screen(),
                },
                found,
            )
            .0;
    }

    /// One run a reader found, its cells on the window lined or struck; none
    /// for one a program may have cut (`may_be_cut`). A printed path's is
    /// kept with its path, drawn or not, `crosses` saying whether it runs
    /// across a row its writer ended.
    #[allow(clippy::too_many_arguments)]
    fn push_run<R: Reader>(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        back: Rows,
        run: &Run,
        reader: &R,
        crosses: Crosses,
        scan: &mut Scan,
    ) {
        let window = Rows::of_index(self.size.1);
        let span = &run.units;
        let Some(&end) = span.end.checked_sub(1).and_then(|e| self.points.get(e)) else {
            return;
        };
        if may_be_cut(term, scrolled, end) {
            return;
        }
        let mut pieces = core::mem::take(&mut self.run_pieces);
        pieces.clear();
        let mut counted = Scan::default();
        let (lines, counts) = match run.key {
            Key::Fixed => (&mut self.fixed, &mut *scan),
            Key::Path { .. } => (&mut pieces, &mut counted),
        };
        for k in span.clone() {
            let Some(&p) = self.points.get(k) else {
                continue;
            };
            let Some(y) = scrolled.window_row(back, p.line, window) else {
                continue;
            };
            let Some((cells, _)) = row_of(term, scrolled, p.line) else {
                continue;
            };
            let Some(&cell) = cells.get(p.col.index()) else {
                continue;
            };
            let styles = row_styles_of(term, scrolled, p.line);
            let size = self.sizes[y.index()];
            match run.mark {
                Mark::Line => push_cell(lines, term, styles, (y, p.col, cell), size, counts),
                Mark::Struck => {
                    if push_struck(&mut self.struck, term, styles, (y, p.col, cell), size) {
                        counts.struck += 1;
                    }
                }
            }
        }
        if let Key::Path { at, drawn } = &run.key {
            if !pieces.is_empty() {
                let key = reader.key(at.clone());
                let k = self.keys.len();
                self.keys.extend_from_slice(key);
                let p = self.pieces.len();
                self.pieces.extend_from_slice(&pieces);
                self.waits.push(Wait {
                    key: k..k + key.len(),
                    pieces: p..p + pieces.len(),
                    drawn: *drawn,
                    crosses,
                });
                if *drawn == Drawn::Yes {
                    scan.spans += counted.spans;
                }
            }
        }
        self.run_pieces = pieces;
    }

    /// The `OSC 8` runs on rows `top..=bottom`, each link's target admitted
    /// once a run.
    fn osc8<R: Reader>(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        back: Rows,
        (top, bottom): (Row, Row),
        reader: &mut R,
        scan: &mut Scan,
    ) {
        // The view's rows holding each slot, built once a scan, and each
        // slot's mark once judged: a slot is judged once a scan, over every
        // row of the view it is shown on.
        if self.slots_read == SlotsRead::No {
            self.slots_read = SlotsRead::Yes;
            self.slot_rows.fill_from(&self.sets, scrolled, back);
            self.judged.clear();
        }
        let screen = term.screen();
        let view = term.view(back);
        for y in top.index()..=bottom.index() {
            let r = Row::from_index(y);
            let line: AbsLine = scrolled.line(back, r);
            let Some((cells, _)) = row_of(term, scrolled, line) else {
                continue;
            };
            let cells = slot_reach(cells, view.extent(r));
            scan.link_cells += cells.len();
            let styles = row_styles_of(term, scrolled, line);
            for (x, &cell) in cells.iter().enumerate() {
                let id = cell.flags.link();
                let Some(slot) = id.slot() else {
                    continue;
                };
                let rows = &self.slot_rows;
                let mark = self.judged.get_or_judge(slot, || {
                    let at = At {
                        line,
                        id,
                        rows: rows.rows(id),
                        screen,
                    };
                    term.link_uri(id).and_then(|u| reader.marked(u, at))
                });
                let at = (r, Col::at_index(x), cell);
                match mark {
                    Some(Mark::Line) => {
                        push_cell(&mut self.fixed, term, styles, at, self.sizes[y], scan);
                    }
                    Some(Mark::Struck) => {
                        if push_struck(&mut self.struck, term, styles, at, self.sizes[y]) {
                            scan.struck += 1;
                        }
                    }
                    None => {}
                }
            }
        }
    }
}

/// **The cells of `row` that may name a link**, by the row's mark `e`:
/// none on a row the mark says holds no link; else those before the mark,
/// past which every cell is the row's last; and the whole row where that
/// last cell names one.
fn slot_reach(row: &[PackedCell], e: RowExtent) -> &[PackedCell] {
    if e.links() == RowLinks::None {
        return &[];
    }
    let to = e.index().min(row.len());
    match row.last() {
        Some(c) if to < row.len() && c.flags.link() != LinkId::NONE => row,
        _ => &row[..to],
    }
}

/// Whether a link from `from` to `to` runs across a row its writer ended.
fn crosses_a_join(term: &Term<'_>, scrolled: Scrolled, from: Point, to: Point) -> bool {
    let mut l = from.line;
    while l < to.line {
        match row_of(term, scrolled, l) {
            Some((_, Wrap::Continues)) => l = l.below(),
            Some(_) => return true,
            None => return false,
        }
    }
    false
}

/// The underlines a cell may carry of its own (`Attrs::UNDERLINES`, one each).
const OWN_LINES: [Attrs; 5] = [
    Attrs::UNDERLINE,
    Attrs::DOUBLE_UNDERLINE,
    Attrs::CURLY_UNDERLINE,
    Attrs::DOTTED_UNDERLINE,
    Attrs::DASHED_UNDERLINE,
];

/// **One cell into the struck runs**, joining the run it continues; none for
/// a cell a program strikes itself (`SGR 9`), a concealed one or one on a
/// double-size line. Whether it started a run.
pub(crate) fn push_struck(
    struck: &mut Vec<LinkLine>,
    term: &Term<'_>,
    styles: RowStyles<'_>,
    (row, col, cell): (Row, Col, PackedCell),
    size: LineRendition,
) -> bool {
    if size.is_double() {
        return false;
    }
    let word: StyleWord = term.style_in(styles, cell);
    let attrs = word.attrs();
    if attrs.has(Attrs::HIDDEN) || attrs.has(Attrs::STRIKE) {
        return false;
    }
    if let Some(l) = struck.last_mut() {
        if l.row == row && l.word == word && l.to.index() + 1 == col.index() {
            l.to = col;
            return false;
        }
    }
    struck.push(LinkLine {
        row,
        from: col,
        to: col,
        word,
    });
    true
}

/// **One cell into the lines**, joining the run it continues; none for a cell
/// a program underlines itself, a concealed one or one on a double-size line.
fn push_cell(
    lines: &mut Vec<LinkLine>,
    term: &Term<'_>,
    styles: RowStyles<'_>,
    (row, col, cell): (Row, Col, PackedCell),
    size: LineRendition,
    scan: &mut Scan,
) {
    if size.is_double() {
        return;
    }
    let word: StyleWord = term.style_in(styles, cell);
    let attrs = word.attrs();
    if attrs.has(Attrs::HIDDEN) || OWN_LINES.iter().any(|&o| attrs.has(o)) {
        return;
    }
    if let Some(l) = lines.last_mut() {
        if l.row == row && l.word == word && l.to.index() + 1 == col.index() {
            l.to = col;
            return;
        }
    }
    lines.push(LinkLine {
        row,
        from: col,
        to: col,
        word,
    });
    scan.spans += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Hover, admit, find, window};
    use baud_core::{Focus, Owned};
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 6,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        for c in bytes.chunks(4096) {
            t.write_at(BytePos::ZERO, c);
            t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
        }
    }

    /// Far enough into a session that the lines above the screen have numbers.
    fn at() -> Scrolled {
        Scrolled::START.advanced(Rows::of_count(1_000_000))
    }

    /// Each run as `(row, from, to)`.
    fn runs(l: &Lines) -> Vec<(usize, usize, usize)> {
        l.lines()
            .iter()
            .map(|s| (s.row.index(), s.from.index(), s.to.index()))
            .collect()
    }

    fn scanned(bytes: &[u8]) -> Vec<(usize, usize, usize)> {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes);
        let mut l = Lines::default();
        let _ = l.scan_alone(&t, at(), &mut Uris::default());
        runs(&l)
    }

    /// A scan as the driver makes it with `[link] join_rows = "off"`: through
    /// `paths::Links`, its `Paths` holding every row the writer ended apart.
    fn scan_apart(l: &mut Lines, t: &Term<'_>, u: &mut Uris) -> Scan {
        let mut p = crate::paths::Paths::default();
        let _ = p.set_join(crate::paths::Turned::Off);
        l.scan_alone(
            t,
            at(),
            &mut crate::paths::Links {
                uris: u,
                urls: crate::paths::Turned::On,
                paths: &mut p,
            },
        )
    }

    /// **What a URI in the text is, at rest, is what a click reads**: the
    /// four schemes get a line over their own cells, and every near-miss gets
    /// none: a longer scheme, a scheme alone, a `%` with no hex, a bracket out
    /// of place, a letter running on, a `mailto` with a hidden header, another
    /// host's `file`, a scheme off the list, a `<` never closed. Perturbation:
    /// take `found_in`'s run past its trailing prose; skip `admit` in
    /// `Uris::read`; drop the `verdict.is_ok()` test.
    #[test]
    fn near_misses_get_no_line() {
        type Case = (&'static [u8], &'static [(usize, usize, usize)]);
        let cases: [Case; 18] = [
            (b"see https://a.example/x today", &[(0, 4, 22)]),
            (b"http://a.example", &[(0, 0, 15)]),
            (b"mailto:me@a.example", &[(0, 0, 18)]),
            (b"file:///etc/hosts", &[(0, 0, 16)]),
            (b"HTTPS://A.EXAMPLE", &[(0, 0, 16)]),
            (b"(https://a.example/x), ok", &[(0, 1, 19)]),
            (b"https://a.example/x.", &[(0, 0, 18)]),
            (
                b"https://a.example https://b.example",
                &[(0, 0, 16), (0, 18, 34)],
            ),
            (b"xhttps://a.example", &[]),
            (b"https://", &[]),
            (b"mailto:", &[]),
            (b"https://a.example/%zz", &[]),
            (b"https://a.example/[x]", &[]),
            ("https://a.exampleé".as_bytes(), &[]),
            (b"mailto:me@a.example?bcc=x@y.example", &[]),
            (b"file://elsewhere.example/x", &[]),
            (b"ftp://a.example/x", &[]),
            (b"<https://a.example/x", &[]),
        ];
        for (bytes, want) in cases {
            assert_eq!(scanned(bytes), want, "{}", String::from_utf8_lossy(bytes));
        }
    }

    /// **An `OSC 8` run gets a line where it opens**, cut by nothing but its
    /// own cells, and a target `admit` refuses gets none. Perturbation: skip
    /// `reader.admits` in `osc8`.
    #[test]
    fn a_marked_link_gets_a_line_where_it_opens() {
        assert_eq!(
            scanned(b"\x1b]8;;https://a.example/\x1b\\word\x1b]8;;\x1b\\ then"),
            [(0, 0, 3)]
        );
        assert_eq!(
            scanned(b"\x1b]8;;ftp://a.example/\x1b\\word\x1b]8;;\x1b\\ then"),
            []
        );
    }

    /// **A program's own underline wins, cell by cell, and a concealed cell
    /// gets none.** Perturbation: drop the attribute test in `push_cell`.
    #[test]
    fn a_programs_own_underline_wins() {
        assert_eq!(scanned(b"\x1b[4mhttps://a.example\x1b[m"), []);
        assert_eq!(
            scanned(b"https://\x1b[4ma\x1b[m.example"),
            [(0, 0, 7), (0, 9, 16)]
        );
        assert_eq!(scanned(b"\x1b[8mhttps://a.example\x1b[m"), []);
    }

    /// An editor's soft-wrapped Markdown link, as it wrote it at 80x24: the
    /// first row ends in `logoColor=`, the wrap row is `↪ white)](URL)`, and
    /// both URLs are underlined by the editor itself (`SGR 4`).
    const WRAPPED: &[u8] = b"\x1b[16;8H\x1b[38;2;164;160;232;48;2;59;34;76m[![\x1b[38;2;236;205;186;48;2;59;34;76mNix\x1b[38;2;164;160;232;48;2;59;34;76m](\x1b[38;2;204;204;204;48;2;59;34;76m\x1b[4mhttps://img.shields.io/badge/Nix-2b2b2b?logo=nixos&logoColor=\x1b[17;3H\x1b[39;48;2;59;34;76m\x1b[24m   \x1b[17;8H\x1b[38;2;90;89;119;48;2;59;34;76m\xe2\x86\xaa \x1b[38;2;204;204;204;48;2;59;34;76m\x1b[4mwhite\x1b[38;2;164;160;232;48;2;59;34;76m\x1b[24m)](\x1b[38;2;204;204;204;48;2;59;34;76m\x1b[4mhttps://nixos.org\x1b[38;2;164;160;232;48;2;59;34;76m\x1b[24m)\x1b[38;2;90;89;119;48;2;59;34;76m ";

    /// The same editor's unwrapped row of that link, as it wrote it.
    const UNWRAPPED: &[u8] = b"\x1b[17;8H\x1b[7m\x1b[38;2;164;160;232;48;2;59;34;76m[\x1b[27m\x1b[38;2;236;205;186;48;2;59;34;76mNix\x1b[38;2;33;33;33;48;2;108;105;153m]\x1b[38;2;164;160;232;48;2;59;34;76m(\x1b[38;2;204;204;204;48;2;59;34;76m\x1b[4mhttps://nixos.org\x1b[38;2;164;160;232;48;2;59;34;76m\x1b[24m)\x1b[38;2;90;89;119;48;2;59;34;76m \x1b[38;2;164;160;232;48;2;59;34;76m(with\x1b[38;2;90;89;119;48;2;59;34;76m \x1b[38;2;164;160;232;48;2;59;34;76mflakes\x1b[38;2;90;89;119;48;2;59;34;76m \x1b[38;2;164;160;232;48;2;59;34;76mturned\x1b[38;2;90;89;119;48;2;59;34;76m \x1b[38;2;164;160;232;48;2;59;34;76mon).\x1b[38;2;90;89;119;48;2;59;34;76m ";

    /// `bytes` with every `SGR 4` taken out.
    fn not_underlined(bytes: &[u8]) -> Vec<u8> {
        let sgr4 = b"\x1b[4m";
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i..].starts_with(sgr4) {
                i += sgr4.len();
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        out
    }

    /// The lines at rest over `bytes` on an 80x24 grid, and the URI a click
    /// at `(row, col)` reads.
    fn at_80x24(bytes: &[u8], (row, col): (Row, Col)) -> (Vec<(usize, usize, usize)>, Vec<u8>) {
        let ws = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, bytes);
        let mut l = Lines::default();
        let _ = l.scan_alone(&t, at(), &mut Uris::default());
        let on = at().line(Rows::ZERO, row);
        let w = window(&t, at(), on, col).expect("a window");
        let bare = w.bare().expect("a URI under the click");
        (runs(&l), bare.uri().expect("one that opens").to_vec())
    }

    /// **A URL on a soft-wrap row is found as one on an unwrapped row is,
    /// and a program's own underline is all that keeps baud's line off
    /// either**: the wrap row's `↪` and the `](`/`)` around the URL end no
    /// read short. As written, a click on either row reads
    /// `https://nixos.org` and neither row gets a line of baud's; with the
    /// `SGR 4` taken out, each URL gets one line over exactly its cells.
    /// Perturbation: drop the attribute test in `push_cell` -- the two
    /// as-written arms light.
    #[test]
    fn a_url_on_a_soft_wrap_row_is_found_as_on_an_unwrapped_one() {
        let url = b"https://nixos.org".to_vec();
        // The wrap row: `↪` at column 7, the URL at 17..=33, a click at 25.
        assert_eq!(
            at_80x24(WRAPPED, (Row::from_index(16), Col::at_index(25))),
            (vec![], url.clone())
        );
        assert_eq!(
            at_80x24(
                &not_underlined(WRAPPED),
                (Row::from_index(16), Col::at_index(25))
            ),
            (vec![(15, 15, 75), (16, 17, 33)], url.clone())
        );
        // The unwrapped row: the URL at 13..=29, a click at 21.
        assert_eq!(
            at_80x24(UNWRAPPED, (Row::from_index(16), Col::at_index(21))),
            (vec![], url.clone())
        );
        assert_eq!(
            at_80x24(
                &not_underlined(UNWRAPPED),
                (Row::from_index(16), Col::at_index(21))
            ),
            (vec![(16, 13, 29)], url)
        );
    }

    /// **A URI across a soft wrap is read whole, and a change on one of its
    /// rows reads the others**: the scheme overwritten on the first row takes
    /// the line off the second, which did not change. Perturbation: read only
    /// the rows that changed in `scan`; the second row keeps its line.
    #[test]
    fn a_soft_wrap_joins_across_a_changed_neighbour() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"see https://a.example/");
        feed(&mut t, &[b'b'; 30]);
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 4, 39), (1, 0, 11)], "the control");
        // The row is one logical line with the next; only row 0's cells move.
        feed(&mut t, b"\x1b[1;5HX");
        let s = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [], "the second row did not change, and goes");
        assert_eq!(s.rows, 2, "the logical line's rows, no more");
        // And a change on the second row reads the first.
        feed(&mut t, b"\x1b[1;5Hh\x1b[2;6H ");
        let s = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 4, 39), (1, 0, 4)]);
        assert_eq!(s.rows, 2);
    }

    /// **A wrap that ends takes the line off the row below it**, whose cells
    /// did not change: it is a line of its own now, with no URI on it. `LF`
    /// ends the row it leaves and writes no cell. Read with `join_rows` off,
    /// which would otherwise join the full row the wrap left to the next. Perturbation: drop
    /// `wrap_moved` from `scan`; the `LF` half fails, the second row keeping
    /// its line. The erase after it holds with the wrap kept as well, and is
    /// no check of `wrap_moved`.
    #[test]
    fn a_wrap_that_ends_reads_the_row_below() {
        let wrapped = |t: &mut Term<'_>| {
            feed(t, b"see https://a.example/");
            feed(t, &[b'b'; 30]);
        };
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        wrapped(&mut t);
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = scan_apart(&mut l, &t, &mut u);
        assert_eq!(runs(&l), [(0, 4, 39), (1, 0, 11)], "the control");
        assert_eq!(t.view(Rows::ZERO).wrapped(Row::ZERO), Wrap::Continues);
        feed(&mut t, b"\x1b[1;1H\n");
        assert_eq!(
            t.view(Rows::ZERO).wrapped(Row::ZERO),
            Wrap::Ended,
            "the control: the wrap is gone, with no cell written"
        );
        let s = scan_apart(&mut l, &t, &mut u);
        assert_eq!(runs(&l), [], "cut by the line's end, and a fragment below");
        assert_eq!(s.rows, 2, "the row whose wrap went, and the one below");
        // An erase of the first row, by whichever wrap it leaves: the fragment
        // below it names no URI.
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        wrapped(&mut t);
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 4, 39), (1, 0, 11)], "the control");
        feed(&mut t, b"\x1b[1;1H\x1b[2K");
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [], "no fragment stays underlined");
    }

    /// **The scan is given this machine's name and never reads it**, so nothing
    /// on the frame path can wait on it, and until it comes a `file` link
    /// naming a host is refused as `admit` refuses an unknown host: no line.
    /// `file:///` and `localhost` need no name, and another machine's is refused
    /// after it comes. Perturbations: call `host_name()` in `scan_lines`,
    /// `read_hover_as` or the click; admit any host in `Uris::admits` while none
    /// is known.
    #[test]
    fn the_scan_is_given_the_host_and_an_unknown_one_opens_nothing() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"file://here.example/x\r\nfile:///y\r\nfile://localhost/z\r\nfile://other.example/w",
        );
        let (mut l, mut u) = (Lines::default(), Uris::default());
        assert!(!u.host_known());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(
            runs(&l),
            [(1, 0, 8), (2, 0, 17)],
            "unknown: named hosts refused"
        );
        u.set_names(crate::link::MachineNames::one("here.example"));
        assert!(u.host_known());
        let s = l.scan_alone(&t, at(), &mut u);
        assert_eq!(s, Scan::default(), "nothing changed, so nothing is read");
        l.forget();
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(
            runs(&l),
            [(0, 0, 20), (1, 0, 8), (2, 0, 17)],
            "this machine's opens, another's does not"
        );
        let src = crate::app_source::app_source();
        let layer = include_str!("link_layer.rs");
        // The app's scan and hover, and the shared frame and click the app
        // and the tap run, each read in its own fn.
        for (src, start) in [
            (src, concat!("fn scan_", "lines(&mut self) {")),
            (
                src,
                concat!(
                    "fn read_hover_",
                    "as(&mut self, chord: crate::link::Chord) -> bool {"
                ),
            ),
            (layer, concat!("pub(crate) fn ", "frame(")),
            (layer, concat!("pub(crate) fn ", "decide<'l: 'a, 'a>(")),
        ] {
            let body = crate::app_source::fn_body(src, start);
            assert!(body.len() > 2, "the control");
            assert!(!body.contains(concat!("host_", "name(")), "{start}");
        }
        let (_, at_tick) =
            crate::app_source::split_once(concat!("fn link_host_", "check(&mut self) {"))
                .expect("the driver hands the name over");
        let (at_tick, _) = at_tick.split_once("\n    }\n").expect("its end");
        assert!(
            at_tick.contains(concat!(".hosts", ".owed()")),
            "the thread's read"
        );
        let (_, hand) = crate::app_source::split_once(concat!(
            "fn hand_host_",
            "names(&mut self, names: Vec<String>) {"
        ))
        .expect("the hand");
        let (hand, _) = hand.split_once("\n    }\n").expect("its end");
        assert!(hand.contains(concat!("link::", "hand_names(")));
        assert!(hand.contains(concat!("lines.", "forget()")));
        // A click takes the answer the line took: the driver it runs in needs
        // a window, so the seam here is its source and `admit` on the same name.
        assert!(
            !src.contains(concat!("opener::", "host_name(")),
            "a click reads no name"
        );
        // The click's call, through its last argument: the window's own
        // `uris`, whose host the lines took; and the shared click admits
        // against that host, in its own fn.
        let open = crate::app_source::fn_body(
            src,
            concat!("fn open_", "link(&mut self, link: &crate::link::Under,"),
        );
        let decide =
            crate::no_appkit::fn_body(layer, concat!("pub(crate) fn ", "decide<'l: 'a, 'a>("));
        assert!(
            open.contains(concat!(
                "crate::link_layer::decide(link, &self.app.term, ",
                "&self.app.paths, &self.app.uris);"
            )) && decide.contains(concat!(
                "click_on(link, term, uris.",
                "names(), uris.rules(), paths)"
            )),
            "a click admits against the name the lines do"
        );
        let unknown = Uris::default();
        assert_eq!(
            admit(b"file://here.example/x", unknown.names()),
            Err(crate::link::Refused::Host),
            "unknown: a click refuses what has no line"
        );
        assert!(admit(b"file:///y", unknown.names()).is_ok());
    }

    /// **A frame that moved only the cursor reads nothing**: exactly 0 rows
    /// and 0 cells, the lines kept. Perturbation: read every row in `scan`, as
    /// the plain arm of the measurement does not.
    #[test]
    fn a_cursor_only_frame_scans_nothing() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://a.example\r\nand more");
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let first = l.scan_alone(&t, at(), &mut u);
        assert_eq!(
            first.rows, 6,
            "the control: the first scan reads the window"
        );
        assert_eq!(first.spans, 1);
        feed(&mut t, b"\x1b[3;3H");
        let s = l.scan_alone(&t, at(), &mut u);
        assert_eq!(s, Scan::default(), "no row, no cell, no candidate");
        assert_eq!(runs(&l), [(0, 0, 16)], "and the line stays");
    }

    /// **`urls = "off"` with `paths = "off"` scans nothing**: the config names
    /// no style, which is what a driver's scan is behind, and the lines go at
    /// once. Each driver goes through that one test. With `paths` on the scan
    /// stays, for paths only (`paths::tests`). Perturbation: scan and hide the
    /// line; call `scan` before the match in `scan_lines`.
    #[test]
    fn urls_off_scans_nothing() {
        use crate::config::{Link, LinkLineKey, LinkMissing, LinkPaths, LinkUrls};
        let off = Link {
            urls: LinkUrls::Off,
            paths: LinkPaths::Off,
            ..Link::default()
        };
        assert_eq!(off.lines(), None);
        assert_eq!(off.at_rest(), crate::link::AtRest::Off);
        assert_eq!(
            Link {
                line: LinkLineKey::Off,
                missing: LinkMissing::Off,
                ..Link::default()
            }
            .lines(),
            None,
            "no line to draw and none to strike, nothing to read for one"
        );
        assert!(Link::default().lines().is_some(), "the control");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://a.example");
        let mut l = Lines::default();
        let _ = l.scan_alone(&t, at(), &mut Uris::default());
        assert!(!l.lines().is_empty());
        l.off();
        assert!(l.lines().is_empty());
        // The app's frame is the shared one (`link_layer::frame`), given
        // the window's config, and scans nothing of its own; the shared
        // frame and Linux's each scan behind the config's answer, each
        // read in its own fn.
        let body = crate::app_source::fn_body;
        let app = body(
            crate::app_source::app_source(),
            concat!("    fn scan_", "lines(&mut self) {"),
        );
        assert!(
            app.contains(concat!("crate::link_layer::", "frame(")),
            "{app}"
        );
        assert!(
            app.contains(concat!("config: &self.", "link_config,")),
            "{app}"
        );
        assert!(!app.contains(concat!(".lines.", "scan(")), "{app}");
        for (name, body, config) in [
            (
                "link_layer.rs",
                body(
                    include_str!("link_layer.rs"),
                    concat!("pub(crate) fn ", "frame("),
                ),
                "layer.config.",
            ),
            (
                "linux.rs",
                body(
                    include_str!("linux.rs"),
                    concat!("    fn ", "plan(&mut self) -> Planned {"),
                ),
                "self.link_config.",
            ),
        ] {
            let lines = format!("{config}{}", concat!("lines", "()"));
            let at = body.find(lines.as_str()).expect(name);
            let scan = body.find(concat!(".lines.", "scan(")).expect(name);
            assert!(at < scan, "{name}: the scan is behind the config's answer");
        }
    }

    /// **The work is bounded whatever a program sends**: `yes https://x` with
    /// no newlines, a logical line far past the screen, costs one pass of the
    /// window and `MAX` either side; so does one URI past `LINK_MAX`, which
    /// gets no line. Constructed input for a bound: no figure is taken from it.
    /// Perturbation: drop the `read > MAX` stop above the window.
    #[test]
    fn a_flood_costs_a_bounded_read() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let flood = "https://x ".repeat(20_000);
        feed(&mut t, flood.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let s = l.scan_alone(&t, at(), &mut u);
        let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
        let limit = cols * rows + 2 * (MAX + 2 * cols);
        assert!(s.cells <= limit, "{} cells read, past {limit}", s.cells);
        assert!(!l.lines().is_empty(), "the control: it read links");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let long = format!("https://a.example/{}", "b".repeat(2 * MAX + 100));
        feed(&mut t, long.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let s = l.scan_alone(&t, at(), &mut u);
        assert!(s.cells <= limit, "{} cells read, past {limit}", s.cells);
        assert_eq!(runs(&l), [], "past the cap, refused whole");
        // A run of closers costs one pass (`found_in`'s counts).
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let closers = format!("https://a.example/{}", ")".repeat(200));
        feed(&mut t, closers.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 0, 17)], "the closers are prose");
    }

    /// **A line means a click opens those cells**: for every run the scan
    /// draws, `find` at each of its cells reads the same cells and `admit` lets
    /// them open, so a line never promises what a click refuses. Perturbation:
    /// widen a span in `Uris::read`; let `Uris::read` keep a refused verdict.
    #[test]
    fn a_line_is_what_a_click_opens() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"a (https://a.example/x) b https://c.example/ mailto:me@d.example.\r\n",
        );
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert!(!l.lines().is_empty(), "the control");
        for line in l.lines() {
            for x in line.from.index()..=line.to.index() {
                let on = at().line(Rows::ZERO, line.row);
                let w = window(&t, at(), on, Col::at_index(x)).expect("a window");
                let f = find(&w).expect("a URI under a line");
                let (_, verdict) = f.parts();
                assert_eq!(verdict, Ok(()), "column {x}");
                let bare = w.bare().expect("its bytes");
                assert!(
                    admit(bare.uri().unwrap(), &crate::link::MachineNames::NONE).is_ok(),
                    "column {x}"
                );
            }
        }
    }

    /// **With `join_rows` off, a URI a program may have cut is not drawn
    /// whole**: one reaching the last column with a line break after it and
    /// more URI characters on the next row gets no line. The controls: the
    /// same URI with a blank next row does, and so does one a soft wrap
    /// carried. A change on the next row reads the one above. Perturbation:
    /// drop `may_be_cut` from `push_run`; drop the `under` arm in `scan`; let
    /// `Paths` join with `join_rows` off -- the first assertion lines both.
    #[test]
    fn a_uri_a_program_cut_is_not_drawn_whole() {
        let edge = format!("https://a.example/{}", "b".repeat(22));
        assert_eq!(edge.len(), 40);
        let cut = format!("{edge}\x1b[2;1Hcontinued");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, cut.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = scan_apart(&mut l, &t, &mut u);
        assert_eq!(runs(&l), [], "cut by the program");
        feed(&mut t, b"\x1b[2;1H\x1b[2K");
        let _ = scan_apart(&mut l, &t, &mut u);
        assert_eq!(runs(&l), [(0, 0, 39)], "the next row blank: it was whole");
        let spaced = format!("{edge}\x1b[2;1H continued");
        assert_eq!(scanned(spaced.as_bytes()), [(0, 0, 39)]);
    }

    /// **A row the writer ended at the last column is read on into the
    /// next**, where that row starts in a URI's characters: the address over
    /// both rows is lined on both, whether the writer moved the cursor or
    /// sent CR LF, and real `fold -w 40` output reads so too. A change on the
    /// lower row alone reads the pair again. Perturbation: hold the rows
    /// apart at rest, `scan` grouping and `read_group` walking soft wraps
    /// alone -- nothing is lined.
    #[test]
    fn a_row_the_writer_ended_at_the_last_column_joins_the_next() {
        let edge = format!("https://a.example/{}", "b".repeat(22));
        for ends in ["\x1b[2;1H", "\r\n"] {
            let bytes = format!("{edge}{ends}continued ok");
            assert_eq!(
                scanned(bytes.as_bytes()),
                [(0, 0, 39), (1, 0, 8)],
                "{ends:?}"
            );
        }
        let fold =
            b"see https://example.com/a/very/long/path\r\n/to/some/resource/index.html ok\r\n";
        assert_eq!(scanned(fold), [(0, 4, 39), (1, 0, 27)], "fold -w 40");
        // The spaced control: the next row starts in a space.
        let spaced = format!("{edge}\x1b[2;1H continued");
        assert_eq!(scanned(spaced.as_bytes()), [(0, 0, 39)]);
        // A change on the lower row: the pair is read again.
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, format!("{edge}\x1b[2;1Hcontinued").as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 0, 39), (1, 0, 8)]);
        feed(&mut t, b"\x1b[2;1H ");
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(
            runs(&l),
            [(0, 0, 39)],
            "apart once the row starts in a space"
        );
    }

    /// **A row that does not run on into the next is read alone**: a change
    /// on a row reads it and the row above, whatever the rows around end in,
    /// where the row ends short of the last column or the next starts in a
    /// space. Perturbations: drop `runs_on`'s last-cell test -- the change on
    /// row 1 reads three rows; drop its first-cell test -- the change on row
    /// 4 does; join every row the writer ended -- both do.
    #[test]
    fn a_row_that_does_not_run_on_is_read_alone() {
        let full = "w".repeat(40);
        let screen = format!("see https://a.example/x\r\n/to/more ok\r\n{full}\r\n spaced\r\nnext");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, screen.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 4, 22)], "the control");
        feed(&mut t, b"\x1b[2;20Hx");
        assert_eq!(
            l.scan_alone(&t, at(), &mut u).rows,
            2,
            "row 1 and the row above"
        );
        feed(&mut t, b"\x1b[5;20Hx");
        assert_eq!(
            l.scan_alone(&t, at(), &mut u).rows,
            2,
            "row 4 and the row above"
        );
    }

    /// **A trim of the history above the window reads its top row again**:
    /// a window looked back to a history at its cap, its top row the last
    /// part of a link whose first row is the oldest line kept -- joined from
    /// rows the writer ended above it, or soft-wrapped from them -- and that
    /// first row trimmed by one more line of output. No cell of the window
    /// changed, and the top row's part of the link goes, as a scan from
    /// nothing finds it. Named breaks: drop the check of where the top row's
    /// line began -- all three stay red; check only the line just above the
    /// window -- the three-row link stays red, its middle row still there.
    #[test]
    fn a_trim_of_a_joined_row_above_the_window_reads_its_top_row_again() {
        let (kept, fresh) = trimmed_above(&[FIRST, SECOND], "\r\n");
        assert_eq!(kept, fresh, "the half link goes");
    }

    #[test]
    fn a_trim_of_a_wrapped_row_above_the_window_reads_its_top_row_again() {
        let (kept, fresh) = trimmed_above(&[FIRST, SECOND], "");
        assert_eq!(kept, fresh, "the half link goes");
    }

    #[test]
    fn a_trim_two_rows_above_the_window_reads_its_top_row_again() {
        let (kept, fresh) = trimmed_above(&[FIRST, MIDDLE, LAST], "\r\n");
        assert_eq!(kept, fresh, "the link's last part goes");
    }

    /// A link's rows: the first two fill the 40 columns.
    const FIRST: &str = "see https://example.com/a/very/long/path";
    const SECOND: &str = "/to/some/resource/index.html ok";
    const MIDDLE: &str = "/to/some/resource/and/a/further/segment/";
    const LAST: &str = "index.html ok";

    /// The link's `parts`, each after the last `ends`, at the top of a
    /// history filled to its cap, the window looked back to its last part;
    /// then one more line, which trims the first. The runs a scan kept, and
    /// the runs a scan from nothing finds.
    fn trimmed_above(parts: &[&str], ends: &str) -> (Runs, Runs) {
        let depth = 10;
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(depth),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut scrolled = at();
        let fed = |t: &mut Term<'_>, scrolled: &mut Scrolled, bytes: &[u8]| {
            let before = t.rotation();
            feed(t, bytes);
            if let baud_core::Moved::Retired(n) = t.moved_since(before) {
                *scrolled = scrolled.advanced(n);
            }
        };
        fed(&mut t, &mut scrolled, parts.join(ends).as_bytes());
        for n in 0..depth + usize::from(WS.ws_row) - parts.len() {
            fed(&mut t, &mut scrolled, format!("\r\nline {n}").as_bytes());
        }
        t.set_window_back(Rows::of_count(depth + 1 - parts.len()));
        let last = parts[parts.len() - 1];
        let link = last.find(' ').unwrap_or(last.len());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, scrolled, &mut u);
        assert_eq!(
            runs(&l),
            [(0, 0, link - 1)],
            "the control, the link's last part on the top row"
        );
        fed(&mut t, &mut scrolled, b"\r\none more");
        let top = t.view(t.window_back()).row(Row::ZERO).to_vec();
        assert_eq!(
            text(&t, &top),
            last,
            "the control, the window still on the link's last part"
        );
        let _ = l.scan_alone(&t, scrolled, &mut u);
        let mut fresh = Lines::default();
        let _ = fresh.scan_alone(&t, scrolled, &mut Uris::default());
        assert_eq!(runs(&fresh), [], "the control, a scan from nothing");
        (runs(&l), runs(&fresh))
    }

    type Runs = Vec<(usize, usize, usize)>;

    /// The row's characters, blanks trimmed at the end.
    fn text(t: &Term<'_>, cells: &[PackedCell]) -> String {
        let s: String = cells
            .iter()
            .map(|c| {
                t.base_of(*c)
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or(' ')
            })
            .collect();
        s.trim_end().to_string()
    }

    /// **A row the writer ended short of the last column never joins**,
    /// whatever the next row starts with; nor does a URI the join would
    /// refuse, which reads as unjoined -- cut, so no line -- and a URI that
    /// starts the next row after a word reaching the last column is that
    /// row's own. Perturbations: drop `may_be_cut` from `push_run` -- the
    /// refused join lines its cut row; read only across the join in
    /// `read_group`, never each line alone -- the third case lines nothing.
    #[test]
    fn a_join_holds_only_where_its_conditions_do() {
        let short = "see https://a.example/x\r\n/to/more ok";
        assert_eq!(
            scanned(short.as_bytes()),
            [(0, 4, 22)],
            "short of the last column"
        );
        let edge = format!("https://a.example/{}", "b".repeat(22));
        let refused = format!("{edge}\r\n%zz");
        assert_eq!(scanned(refused.as_bytes()), [], "the join refused: cut");
        let word = format!("{}\r\nhttps://e.example/x", "w".repeat(40));
        assert_eq!(scanned(word.as_bytes()), [(1, 0, 18)], "the next row's own");
    }

    /// **The window looking back reads the rows it shows, by their own
    /// tables**, and the bound holds. The scan reads again only the rows
    /// whose cells changed, with the line above each (`Lines::scan`): moved
    /// eight lines back, rows 0 and 5 hold other cells than the live
    /// screen's and row 4 is the line above row 5, while rows 1 to 3 hold
    /// the same "plain" and are kept. Perturbation: read `term.viewport()`
    /// where the scan reads the view -- nothing reads and no line is found.
    #[test]
    fn a_window_looking_back_reads_the_rows_it_shows() {
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(12),
        );
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://old.example\r\n");
        for _ in 0..12 {
            feed(&mut t, b"plain\r\n");
        }
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [], "the control: off the screen");
        // Fourteen lines on a screen of six: eight behind it.
        t.set_window_back(Rows::of_count(8));
        let s = l.scan_alone(&t, at(), &mut u);
        assert_eq!(
            s.rows, 3,
            "every row the window changed read, and none else"
        );
        assert_eq!(runs(&l), [(0, 0, 18)], "the line on the row it shows");
    }

    /// The window of [`a_scrolled_line_reads_one_row_of_slots`]: 200
    /// columns by 50 rows.
    const WIDE: WinSize = WinSize {
        ws_col: 200,
        ws_row: 50,
    };

    /// An `OSC 8` link to `https://e.example/<n>` on its own line.
    fn linked_line(n: Row) -> String {
        let n = n.index();
        format!("\r\nrow {n} \x1b]8;;https://e.example/{n}\x1b\\link {n}\x1b]8;;\x1b\\ after")
    }

    /// **A scrolled line reads one row of cells for its link slots**: at
    /// 200x50, a link of its own on every row, one line more scrolls the
    /// window by a row, and the scan reads the slots of the one row that
    /// changed, not every row's cells again, while what it keeps is what a
    /// scan from nothing finds and each row's slots, kept and synced, are
    /// its cells'. Two frames settle the view first, as a driver's leave it:
    /// a row written before a frame's seal is read once more at the next.
    /// The control: the line scrolled in holds a link, and it is
    /// underlined. Named breaks: read every row at each sync -- 10,000
    /// cells read; leave the sets where they were in `ViewLinks::moved` --
    /// a synced row holds the slots of the row below it.
    #[test]
    fn a_scrolled_line_reads_one_row_of_slots() {
        let mut owned = Owned::new(&WIDE);
        let mut t = Term::new(owned.storage(), &WIDE);
        let mut scrolled = at();
        let fed = |t: &mut Term<'_>, scrolled: &mut Scrolled, bytes: &[u8]| {
            let before = t.rotation();
            feed(t, bytes);
            if let baud_core::Moved::Retired(n) = t.moved_since(before) {
                *scrolled = scrolled.advanced(n);
            }
        };
        let rows = usize::from(WIDE.ws_row);
        for n in 0..rows {
            fed(
                &mut t,
                &mut scrolled,
                linked_line(Row::from_index(n)).as_bytes(),
            );
        }
        let (mut l, mut u, mut views) = (Lines::default(), Uris::default(), ViewLinks::default());
        // A driver's frame: the view's slots synced, the scan, the seal.
        let mut frame = |t: &mut Term<'_>, scrolled: Scrolled| {
            let s = l.scan(t, scrolled, &views.sync(t, scrolled), &mut u);
            let _ = t.seal_writes();
            s
        };
        let _ = frame(&mut t, scrolled);
        let _ = frame(&mut t, scrolled);
        fed(
            &mut t,
            &mut scrolled,
            linked_line(Row::from_index(rows)).as_bytes(),
        );
        let s = frame(&mut t, scrolled);
        let mut anew = Lines::default();
        let _ = anew.scan_alone(&t, scrolled, &mut Uris::default());
        assert_eq!(
            runs(&l),
            runs(&anew),
            "kept is what a scan from nothing finds"
        );
        let view = t.view(t.window_back());
        let synced = views.fresh(&t, scrolled).expect("the view as synced");
        for (y, &set) in l.sets.iter().enumerate() {
            let r = Row::from_index(y);
            let cells = LinkSet::of_cells(view.row(r));
            assert_eq!(set, cells, "row {y}'s slots, kept");
            assert_eq!(synced.row(r), cells, "row {y}'s slots, synced");
        }
        assert!(
            runs(&l).iter().any(|&(row, _, _)| row == rows - 1),
            "the control: the line scrolled in is underlined"
        );
        assert!(
            s.slot_cells <= usize::from(WIDE.ws_col),
            "{} cells read for slots, against one row's 200",
            s.slot_cells
        );
    }

    /// **The cells a frame reads for link slots are the same at any width**:
    /// the same lines, a link on each, at 80 and at 400 columns, frame by
    /// frame -- the first, a settled one, a line scrolled in, and a linked
    /// row written over in place -- read as many cells for their slots, and
    /// underline the same runs. The control: the frame a linked line came in
    /// reads some. Named breaks: read each synced row to the width in
    /// `ViewLinks::sync`; walk each linked row to the width in `osc8`.
    #[test]
    fn slot_cells_a_frame_do_not_grow_with_the_width() {
        let frames = |ws_col: u16| {
            let ws = WinSize { ws_col, ws_row: 8 };
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let mut scrolled = at();
            let fed = |t: &mut Term<'_>, scrolled: &mut Scrolled, bytes: &[u8]| {
                let before = t.rotation();
                feed(t, bytes);
                if let baud_core::Moved::Retired(n) = t.moved_since(before) {
                    *scrolled = scrolled.advanced(n);
                }
            };
            for n in 0..6 {
                fed(
                    &mut t,
                    &mut scrolled,
                    linked_line(Row::from_index(n)).as_bytes(),
                );
            }
            let (mut l, mut u, mut views) =
                (Lines::default(), Uris::default(), ViewLinks::default());
            let mut read = Vec::new();
            let mut frame = |t: &mut Term<'_>, scrolled: Scrolled| {
                let s = l.scan(t, scrolled, &views.sync(t, scrolled), &mut u);
                let _ = t.seal_writes();
                read.push((s.slot_cells, s.link_cells));
            };
            frame(&mut t, scrolled);
            frame(&mut t, scrolled);
            for n in 6..9 {
                fed(
                    &mut t,
                    &mut scrolled,
                    linked_line(Row::from_index(n)).as_bytes(),
                );
                frame(&mut t, scrolled);
                frame(&mut t, scrolled);
            }
            fed(
                &mut t,
                &mut scrolled,
                b"\x1b[3;1H\x1b]8;;https://e.example/again\x1b\\again\x1b]8;;\x1b\\",
            );
            frame(&mut t, scrolled);
            frame(&mut t, scrolled);
            drop(frame);
            (read, runs(&l))
        };
        let (narrow, wide) = (frames(80), frames(400));
        assert_eq!(narrow.1, wide.1, "the runs underlined");
        assert!(
            narrow.0[2].0 > 0 && narrow.0[2].1 > 0,
            "the control: a line come in reads slots and is underlined"
        );
        assert_eq!(narrow.0, wide.0, "cells read for slots, frame by frame");
    }

    /// **The hand shows only under the chord; the look shows without it**, and
    /// neither is on a link a click would refuse, under which the chord shows an
    /// arrow and the bar. A button down draws no look. A tick over a still
    /// screen reads no text. Perturbations: read `opens` regardless of the
    /// chord in `Hover::update`; drop the `Held` arm; drop `ReadOf`.
    #[test]
    fn the_look_and_the_hand_are_told_apart() {
        use crate::link::{AtRest, Chord, Held};
        use baud_core::PointerShape;
        use baud_render::HoverPaint;
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"https://a.example ftp://b.example \x1b]8;;ftp://c.example\x1b\\marked\x1b]8;;\x1b\\",
        );
        let (s, key, none) = (at(), Focus::Gained, &crate::link::MachineNames::NONE);
        let web = Some((Row::ZERO, Col::ZERO));
        let ftp = Some((Row::ZERO, Col::at_index(20)));
        let marked = Some((Row::ZERO, Col::at_index(35)));
        let mut h = Hover::default();
        assert!(
            !h.update(&t, web, s, Chord::Other, key, none),
            "off until told"
        );
        assert!(h.over().is_none());
        h.set_at_rest(AtRest::Look(HoverPaint::Box));
        assert!(h.update(&t, web, s, Chord::Other, key, none));
        assert_eq!(h.over().expect("the look").paint, HoverPaint::Box);
        assert!(h.look().is_none(), "no bar without the chord");
        assert_eq!(h.pointer(None), None, "no hand without the chord");
        // Still: a read of nothing new.
        let reads = h.reads();
        for _ in 0..50 {
            assert!(!h.refresh(&t, s, Chord::Other, key, none));
        }
        assert_eq!(h.reads(), reads, "a still screen is read once");
        // The chord adds the bar and the hand and keeps the look.
        assert!(h.update(&t, web, s, Chord::Cmd, key, none));
        assert!(h.over().is_some());
        assert!(h.look().is_some());
        assert_eq!(h.pointer(None), Some(PointerShape::Hand2));
        // A scheme off the list as plain text is no link at all.
        let _ = h.update(&t, ftp, s, Chord::Cmd, key, none);
        assert!(h.over().is_none());
        assert!(h.look().is_none());
        assert_eq!(h.pointer(None), None);
        // Marked refused: no look, and under the chord an arrow and no line.
        let _ = h.update(&t, marked, s, Chord::Other, key, none);
        assert!(h.over().is_none(), "no look at rest");
        assert!(h.look().is_none());
        let _ = h.update(&t, marked, s, Chord::Cmd, key, none);
        assert!(h.over().is_none(), "no look under the chord");
        let look = h.look().expect("the bar says why");
        assert!(look.spans.is_empty(), "no line as openable");
        assert_eq!(h.pointer(None), Some(PointerShape::Arrow));
        // A drag draws no look.
        let _ = h.update(&t, web, s, Chord::Other, key, none);
        assert!(h.over().is_some());
        h.set_held(Held::Yes);
        assert!(h.refresh(&t, s, Chord::Other, key, none));
        assert!(h.over().is_none(), "a button down");
        h.set_held(Held::No);
        assert!(h.refresh(&t, s, Chord::Other, key, none));
        assert!(h.over().is_some());
        // Off.
        h.set_at_rest(AtRest::Off);
        assert!(h.refresh(&t, s, Chord::Other, key, none));
        assert!(h.over().is_none());
    }

    /// **`KNOWN` is `admit`'s list and `find`'s**: every URI `admit` opens by
    /// each scheme is one `scheme_at` reads, by the same spelling. Perturbation:
    /// a fifth scheme in `admit` alone, which no longer compiles to one list.
    #[test]
    fn one_list_of_schemes() {
        for uri in [
            "https://a.example",
            "http://a.example",
            "mailto:me@a.example",
            "file:///x",
        ] {
            assert!(
                admit(uri.as_bytes(), &crate::link::MachineNames::NONE).is_ok(),
                "{uri}"
            );
            let units: Vec<Unit> = uri.chars().map(Unit::Char).collect();
            assert!(scheme_at(&units, &Rules::NONE).is_some(), "{uri}");
        }
        assert!(scheme_ends(
            Tail(u64::from_be_bytes(*b"\0\0\0http:")),
            &Rules::NONE
        ));
        assert!(scheme_ends(
            Tail(u64::from_be_bytes(*b"\0mailto:")),
            &Rules::NONE
        ));
        assert!(!scheme_ends(
            Tail(u64::from_be_bytes(*b"\0\0\0\0tel:")),
            &Rules::NONE
        ));
    }

    /// **A scheme a `[link.*]` rule adds is one list with `admit`'s too**:
    /// with the rule, `tel:` and `x-github-client://` get a line at rest over
    /// their cells, admitted by the gate a click goes through, and a
    /// `javascript:` beside them none; without it, none. An `ssh://` with a
    /// host that starts with `-` gets none, as a click refuses it.
    /// Perturbation: read added schemes in `wants` but not in `scheme_at`
    /// -- no line; or skip `admit_with` in `Uris::admits` -- the `ssh` line
    /// is drawn.
    #[test]
    fn a_scheme_a_rule_adds_gets_a_line_at_rest() {
        let sections = [
            crate::rules::Section {
                name: "tel".into(),
                fields: Vec::new(),
            },
            crate::rules::Section {
                name: "gh".into(),
                fields: vec![("scheme".into(), "\"x-github-client\"".into())],
            },
            crate::rules::Section {
                name: "ssh".into(),
                fields: Vec::new(),
            },
        ];
        // Read as `Config::apply` reads them, by the one function it calls.
        let (rules, said) = crate::rules::Section::read_all(&sections, None, None);
        assert_eq!(said, Vec::<String>::new());
        let bytes = b"tel:+15551234\r\nx-github-client://o/r\r\njavascript:x\r\nssh://-oX=y@h";
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes);
        let mut l = Lines::default();
        let mut u = Uris::default();
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [], "the control: no rule, no line");
        u.set_rules(rules);
        l.forget();
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 0, 12), (1, 0, 20)]);
    }

    /// **`off` takes a default's line away, marked or printed**: with
    /// `[link.mailto] action = "off"` neither a printed `mailto:` nor an
    /// `OSC 8` one gets a line, and a web link beside them keeps its own.
    /// Perturbation: admit `OSC 8` targets with `admit` and not the rules --
    /// the marked one keeps its line.
    #[test]
    fn off_takes_a_defaults_line_away_marked_or_printed() {
        let bytes = b"mailto:a@b.example\r\n\x1b]8;;mailto:c@d.example\x1b\\mail me\x1b]8;;\x1b\\\r\nhttps://e.example";
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes);
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 0, 17), (1, 0, 6), (2, 0, 16)], "the control");
        let sections = [crate::rules::Section {
            name: "mailto".into(),
            fields: vec![("action".into(), "\"off\"".into())],
        }];
        let (rules, said) = crate::rules::Section::read_all(&sections, None, None);
        assert_eq!(said, Vec::<String>::new());
        u.set_rules(rules);
        l.forget();
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(2, 0, 16)]);
    }

    /// **A rule's line at rest is an overlay, and nothing a program asks
    /// reads it**: `DECRQCRA` over a `tel:` link answers the same with the
    /// rule's line up as with no rule and no line. Perturbation: mark the
    /// cells the rule's link covers in the core -- the checksum moves.
    #[test]
    fn decrqcra_answers_the_same_with_a_rules_line_up() {
        let checksum = |t: &mut Term<'_>| {
            t.clear_reply();
            feed(t, b"\x1b[7;1;1;1;6;40*y");
            t.reply().to_vec()
        };
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25ltel:+15551234");
        let before = checksum(&mut t);
        assert!(!before.is_empty(), "the control: the query answers");
        let sections = [crate::rules::Section {
            name: "tel".into(),
            fields: Vec::new(),
        }];
        let (rules, said) = crate::rules::Section::read_all(&sections, None, None);
        assert_eq!(said, Vec::<String>::new());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        u.set_rules(rules);
        let _ = l.scan_alone(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 0, 12)], "the control: the line is up");
        assert_eq!(checksum(&mut t), before, "the same with the line up");
        feed(&mut t, b"\x1b[1;1HX");
        assert_ne!(checksum(&mut t), before, "the control: a cell moves it");
    }

    /// **Both drivers hand the rules to the lines and to the reads under the
    /// pointer together**, at start and on a reload, so a click admits with
    /// what the line drew. Perturbation: drop the paths' `set_rules` from one
    /// driver's reload arm.
    #[test]
    fn both_drivers_hand_both_copies_of_the_rules() {
        for (name, src, init) in [
            (
                "app.rs",
                crate::app_source::app_source(),
                "link_config.rules.clone()",
            ),
            (
                "linux.rs",
                include_str!("linux.rs"),
                "cfg.link.rules.clone()",
            ),
        ] {
            let at_start = format!("{}({init})", concat!("set_", "rules"));
            assert_eq!(src.matches(&at_start).count(), 2, "{name}: at start");
            for call in [
                concat!("uris.set_", "rules(c.link.rules.clone())"),
                concat!("paths.set_", "rules(c.link.rules.clone())"),
            ] {
                assert_eq!(src.matches(call).count(), 1, "{name}: {call}");
            }
        }
    }

    /// **Both drivers set the look and draw the lines.** Perturbation: drop
    /// `set_link_hover` from a driver.
    #[test]
    fn both_drivers_draw_the_links() {
        for (name, src) in [
            ("app.rs", crate::app_source::app_source()),
            ("linux.rs", include_str!("linux.rs")),
        ] {
            for call in [
                concat!("set_link_", "lines("),
                concat!("set_link_", "hover("),
                concat!("at_", "rest()"),
            ] {
                assert!(src.contains(call), "{name}: {call}");
            }
        }
    }

    /// `bytes` fed, `scrolled` moved by the lines the core retired: by
    /// nothing across a switch of screens, whose two rotations are two
    /// rings' and, read as one, subtract past zero. That is a selection's
    /// count (`keep::live_moved`), not the scan's in the app, which moves to
    /// the alternate screen's own number for its top line (`keep::Counts`):
    /// a switch here leaves the count where it was, so it never takes the
    /// scan's shift.
    fn fed(t: &mut Term<'_>, scrolled: &mut Scrolled, bytes: &[u8]) {
        let (before, epoch) = (t.rotation(), t.screen_epoch());
        feed(t, bytes);
        if t.screen_epoch() != epoch {
            return;
        }
        if let baud_core::Moved::Retired(n) = t.moved_since(before) {
            *scrolled = scrolled.advanced(n);
        }
    }

    /// **A reader of printed paths `/p<digits>`**, after a space or the
    /// line's start and before a space or its end, each drawn only while
    /// `found` holds it, as `paths.rs`'s `Links` draws one the worker found.
    #[derive(Default)]
    struct Found {
        found: Vec<Vec<u8>>,
    }

    impl Reader for Found {
        fn wants(&self, tail: Tail) -> bool {
            tail.0 & 0xff == u64::from(b'/')
        }

        fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> crate::paths::Looked {
            let u = text.units;
            let ch = |k: usize| match u.get(k) {
                Some(Unit::Char(c)) => Some(*c),
                _ => None,
            };
            let space = |k: Option<usize>| k.is_none_or(|k| k >= u.len() || ch(k) == Some(' '));
            let mut i = 0;
            while i < u.len() {
                if !(ch(i) == Some('/') && ch(i + 1) == Some('p') && space(i.checked_sub(1))) {
                    i += 1;
                    continue;
                }
                let mut e = i + 2;
                while ch(e).is_some_and(|c| c.is_ascii_digit()) {
                    e += 1;
                }
                let key: Vec<u8> = (i..e)
                    .filter_map(ch)
                    .filter_map(|c| u8::try_from(c).ok())
                    .collect();
                let shown = i < text.shown.1 && text.shown.0 < e;
                if e > i + 2 && space(Some(e)) && shown && self.found.contains(&key) {
                    found.push(Run {
                        units: i..e,
                        mark: Mark::Line,
                        key: Key::Fixed,
                    });
                }
                i = e;
            }
            crate::paths::Looked(0)
        }

        fn marked(&mut self, _: &[u8], _: At) -> Option<Mark> {
            None
        }
    }

    /// **A row below the window that the bottom row's link read on into,
    /// written over, reads the bottom row again**: looked back, the bottom
    /// row ends in `/p1` and runs on into `3 ok` below the window, a found
    /// `/p13`; the row below is written over with `/p10`, and the line goes,
    /// as a scan from nothing finds. Named break: compare the window's rows
    /// alone (no `past_moved`) -- `/p13`'s line stays.
    #[test]
    fn a_row_below_the_window_written_over_reads_the_bottom_row_again() {
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(24),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        for n in 0..10 {
            fed(&mut t, &mut at, format!("l{n}\r\n").as_bytes());
        }
        fed(
            &mut t,
            &mut at,
            format!("{:>40}\r\n3 ok\r\nz\r\ny", "/p1").as_bytes(),
        );
        t.set_window_back(Rows::of_count(3));
        let found = || Found {
            found: vec![b"/p13".to_vec()],
        };
        let (mut l, mut r) = (Lines::default(), found());
        let _ = l.scan_alone(&t, at, &mut r);
        let runs = |l: &Lines| -> Vec<_> {
            l.lines()
                .iter()
                .map(|x| (x.row.index(), x.from.index(), x.to.index()))
                .collect()
        };
        assert_eq!(runs(&l), [(5, 37, 39)], "the control: /p13 runs on below");
        fed(&mut t, &mut at, b"\x1b[4;1H\x1b[2K/p10");
        let _ = l.scan_alone(&t, at, &mut r);
        let mut fresh = Lines::default();
        let _ = fresh.scan_alone(&t, at, &mut found());
        assert_eq!(runs(&fresh), [], "a scan from nothing: /p1/p10 is none");
        assert_eq!(runs(&l), runs(&fresh));
        let s = l.scan_alone(&t, at, &mut r);
        assert_eq!(s, Scan::default(), "seen once, read once");
    }

    /// **A look back hands the rows that leave the window below to the
    /// rows below it**, as the last scan saw them: `/p1` runs on into `3 ok`
    /// on the window, a found `/p13`; a look back of two puts `3 ok` below
    /// the window, kept with no read; written over with `/p10`, the bottom
    /// row is read again and the line goes. Named break: hand none over --
    /// the row written over is compared with nothing and the line stays.
    #[test]
    fn a_row_a_look_back_puts_below_the_window_is_watched() {
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(24),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        for n in 0..10 {
            fed(&mut t, &mut at, format!("l{n}\r\n").as_bytes());
        }
        fed(
            &mut t,
            &mut at,
            format!("{:>40}\r\n3 ok\r\nz", "/p1").as_bytes(),
        );
        let found = || Found {
            found: vec![b"/p13".to_vec()],
        };
        let (mut l, mut r) = (Lines::default(), found());
        let _ = l.scan_alone(&t, at, &mut r);
        let runs = |l: &Lines| -> Vec<_> {
            l.lines()
                .iter()
                .map(|x| (x.row.index(), x.from.index(), x.to.index()))
                .collect()
        };
        assert_eq!(runs(&l), [(3, 37, 39), (4, 0, 0)], "the control: /p13");
        t.set_window_back(Rows::of_count(2));
        let s = l.scan_alone(&t, at, &mut r);
        assert_eq!(runs(&l), [(5, 37, 39)], "moved down two");
        assert!(s.rows <= 2, "the rows brought in at the top: {}", s.rows);
        fed(&mut t, &mut at, b"\x1b[5;1H\x1b[2K/p10");
        let _ = l.scan_alone(&t, at, &mut r);
        let mut fresh = Lines::default();
        let _ = fresh.scan_alone(&t, at, &mut found());
        assert_eq!(runs(&fresh), [], "a scan from nothing: /p1/p10 is none");
        assert_eq!(runs(&l), runs(&fresh));
    }

    /// **A reader of URIs and of printed paths `/p<digits>`, and of names
    /// `<digits>`**, after a space or the line's start and before a space or
    /// its end, each drawn while `found` holds it, as `paths.rs`'s `Links`
    /// draws one the worker found. A name begins the row a path runs on
    /// into across a row its writer ended, as a relative path does.
    #[derive(Default)]
    struct Answering {
        uris: Uris,
        found: Vec<Vec<u8>>,
        keys: Vec<u8>,
    }

    impl Answering {
        /// `path` found if it was not, and not if it was.
        fn toggle(&mut self, path: &str) {
            match self.found.iter().position(|f| f == path.as_bytes()) {
                Some(i) => {
                    self.found.remove(i);
                }
                None => self.found.push(path.as_bytes().to_vec()),
            }
        }
    }

    impl Reader for Answering {
        fn wants(&self, tail: Tail) -> bool {
            self.uris.wants(tail) || matches!(tail.0 & 0xff, 0x2f..=0x39)
        }

        fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> crate::paths::Looked {
            let n = self.uris.read(text, found).0;
            let uris = found.len();
            self.keys.clear();
            let u = text.units;
            let ch = |k: usize| match u.get(k) {
                Some(Unit::Char(c)) => Some(*c),
                _ => None,
            };
            let space = |k: Option<usize>| k.is_none_or(|k| k >= u.len() || ch(k) == Some(' '));
            let digit = |k: usize| ch(k).is_some_and(|c| c.is_ascii_digit());
            let mut i = 0;
            while i < u.len() {
                let digits = match (ch(i), ch(i + 1)) {
                    (Some('/'), Some('p')) => i + 2,
                    _ if digit(i) => i,
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                if !space(i.checked_sub(1)) {
                    i = digits.max(i + 1);
                    while digit(i) {
                        i += 1;
                    }
                    continue;
                }
                let mut e = digits;
                while digit(e) {
                    e += 1;
                }
                let shown = i < text.shown.1 && text.shown.0 < e;
                let over = found[..uris]
                    .iter()
                    .any(|f| f.units.start < e && i < f.units.end);
                if e > digits && space(Some(e)) && shown && !over {
                    let k = self.keys.len();
                    self.keys
                        .extend((i..e).filter_map(ch).filter_map(|c| u8::try_from(c).ok()));
                    let key = &self.keys[k..];
                    let drawn = match self.found.iter().any(|f| f == key) {
                        true => Drawn::Yes,
                        false => Drawn::No,
                    };
                    found.push(Run {
                        units: i..e,
                        mark: Mark::Line,
                        key: Key::Path {
                            at: k..self.keys.len(),
                            drawn,
                        },
                    });
                }
                i = e;
            }
            found.sort_unstable_by_key(|r| r.units.start);
            crate::paths::Looked(n)
        }

        fn marked(&mut self, uri: &[u8], at: At) -> Option<Mark> {
            self.uris.marked(uri, at)
        }

        fn key(&self, at: Range<usize>) -> &[u8] {
            self.keys.get(at).unwrap_or_default()
        }

        fn drawn(&self, key: &[u8]) -> Drawn {
            match self.found.iter().any(|f| f == key) {
                true => Drawn::Yes,
                false => Drawn::No,
            }
        }
    }

    /// The links and strikes a scan of `t` from nothing finds, by `reader`'s
    /// answers.
    fn fresh_by(
        t: &Term<'_>,
        scrolled: Scrolled,
        reader: &Answering,
    ) -> (Vec<LinkLine>, Vec<LinkLine>) {
        let mut l = Lines::default();
        let mut again = Answering {
            found: reader.found.clone(),
            ..Answering::default()
        };
        let _ = l.scan_alone(t, scrolled, &mut again);
        (l.lines().to_vec(), l.struck().to_vec())
    }

    /// **A batch of answers redraws the printed paths' runs and reads no
    /// row**: two paths on a row, one found, then the other, then the
    /// first gone, each scan reading nothing and drawing what a scan from
    /// nothing draws. Named break: read every row again on an answer
    /// (`answered` as `forget`) -- the scan reads the row.
    #[test]
    fn an_answer_redraws_paths_and_reads_no_row() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        fed(&mut t, &mut at, b"see /p1 and /p2 ok\r\nplain");
        let (mut l, mut r) = (Lines::default(), Answering::default());
        let s = l.scan_alone(&t, at, &mut r);
        assert!(s.rows > 0, "the control: the first scan reads");
        assert_eq!(l.lines(), &[] as &[LinkLine], "nothing found yet");
        for (path, lit) in [
            ("/p1", &[(4, 6)][..]),
            ("/p2", &[(4, 6), (12, 14)]),
            ("/p1", &[(12, 14)]),
        ] {
            r.toggle(path);
            l.answered();
            let s = l.scan_alone(&t, at, &mut r);
            assert_eq!((s.rows, s.remarks, s.rereads), (0, 1, 0), "{path}");
            let runs: Vec<_> = l
                .lines()
                .iter()
                .map(|x| (x.from.index(), x.to.index()))
                .collect();
            assert_eq!(runs, lit, "{path}");
            assert_eq!(
                (l.lines().to_vec(), l.struck().to_vec()),
                fresh_by(&t, at, &r)
            );
        }
        let s = l.scan_alone(&t, at, &mut r);
        assert_eq!(s, Scan::default(), "no answer, nothing done");
    }

    /// **The lines a scan keeps follow answers as a scan from nothing
    /// finds them**, over 4,000 steps from fixed seeds of output that
    /// scrolls and is written over, printed paths alone and across a row
    /// their writer ended, looks back, and answers that find a path or take
    /// it away. Named break: redraw a run across an ended row by its answer
    /// alone (no read of every row) -- a link the reads of each line alone
    /// kept stays under it.
    #[test]
    fn kept_lines_follow_answers_as_a_fresh_scan() {
        for seed in [3u64, 11, 99, 2027] {
            let mut x = seed;
            let mut next = |m: usize| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                usize::try_from(x % u64::try_from(m).unwrap_or(1)).unwrap_or(0)
            };
            let mut owned = Owned::with_history(
                &WS,
                &baud_domain::CacheGeometry::UNKNOWN,
                baud_domain::Depth::of(8),
            );
            let mut t = Term::new(owned.storage(), &WS);
            let mut at = at();
            let (mut l, mut r) = (Lines::default(), Answering::default());
            for step in 0..1000 {
                let bytes = match next(9) {
                    0 | 1 => format!("\r\nfile /p{} and /p{} ok", next(20), next(20)),
                    2 => format!("\r\nsee https://s{}.example/ /p{}", next(9), next(20)),
                    // A row the writer ended at the last column in a path,
                    // and the row it runs on into.
                    3 => format!("\r\n{:>40}\r\n{} ok /p{}", "/p1", next(10), next(20)),
                    4 => format!("\x1b[{};1H\x1b[2K/p{}", 1 + next(6), next(20)),
                    5 => {
                        t.set_window_back(Rows::of_count(next(4)));
                        String::new()
                    }
                    6 => "\x1b[2J\x1b[H".to_string(),
                    _ => {
                        for _ in 0..1 + next(3) {
                            match next(3) {
                                0 => r.toggle(&format!("{}", next(10))),
                                _ => r.toggle(&format!("/p{}", next(20))),
                            }
                        }
                        l.answered();
                        String::new()
                    }
                };
                fed(&mut t, &mut at, bytes.as_bytes());
                let _ = l.scan_alone(&t, at, &mut r);
                assert_eq!(
                    (l.lines().to_vec(), l.struck().to_vec()),
                    fresh_by(&t, at, &r),
                    "seed {seed}, step {step}: {bytes:?}"
                );
            }
        }
    }

    /// The links and strikes a scan of `t` from nothing finds.
    fn fresh(t: &Term<'_>, scrolled: Scrolled) -> (Vec<LinkLine>, Vec<LinkLine>) {
        let mut l = Lines::default();
        let _ = l.scan_alone(t, scrolled, &mut Uris::default());
        (l.lines().to_vec(), l.struck().to_vec())
    }

    /// **A scroll reads only the rows it brought in**: six rows of links,
    /// one line more, and the scan reads the new row and the one above it
    /// (whose link may have been cut, `may_be_cut`), the five moved rows
    /// keeping their lines, moved a row up, as a scan from nothing finds
    /// them. Named break: compare each row with what the same row held (no
    /// `shift`) -- all six are read.
    #[test]
    fn a_scroll_reads_only_the_rows_it_brought_in() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        for n in 0..6 {
            let nl = if n == 0 { "" } else { "\r\n" };
            fed(
                &mut t,
                &mut at,
                format!("{nl}see https://h{n}.example/ ok").as_bytes(),
            );
        }
        let mut l = Lines::default();
        let mut u = Uris::default();
        assert_eq!(
            l.scan_alone(&t, at, &mut u).rows,
            6,
            "the control: all read"
        );
        fed(&mut t, &mut at, b"\r\nsee https://new.example/ ok");
        let s = l.scan_alone(&t, at, &mut u);
        assert_eq!(s.rows, 2, "the new row and the one above it");
        assert_eq!(runs(&l).len(), 6);
        assert_eq!((l.lines().to_vec(), l.struck().to_vec()), fresh(&t, at));
        assert_eq!(
            l.scan_alone(&t, at, &mut u).rows,
            0,
            "a still frame reads nothing"
        );
    }

    /// **A moved row written over is read again**, and a look back moves the
    /// rows the other way: after a scroll, a row rewritten in place is read
    /// and its new link found, and with the window one line back every row
    /// but the one brought in keeps its line. Named break: keep a row by its
    /// line without comparing its cells -- the rewritten row keeps its old
    /// link.
    #[test]
    fn a_moved_row_written_over_is_read_again() {
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(12),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        for n in 0..8 {
            let nl = if n == 0 { "" } else { "\r\n" };
            fed(
                &mut t,
                &mut at,
                format!("{nl}see https://h{n}.example/ ok").as_bytes(),
            );
        }
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at, &mut u);
        fed(
            &mut t,
            &mut at,
            b"\r\nnext\x1b[3;1H\x1b[2Kgo https://over.example/",
        );
        let _ = l.scan_alone(&t, at, &mut u);
        assert_eq!((l.lines().to_vec(), l.struck().to_vec()), fresh(&t, at));
        assert!(
            runs(&l).contains(&(2, 3, 23)),
            "the row written over: {:?}",
            runs(&l)
        );
        t.set_window_back(Rows::of_count(1));
        let s = l.scan_alone(&t, at, &mut u);
        assert_eq!((l.lines().to_vec(), l.struck().to_vec()), fresh(&t, at));
        assert!(s.rows <= 2, "the row brought in, and its line: {}", s.rows);
    }

    /// **The lines a scan keeps are the lines a scan from nothing finds**,
    /// over 4,000 steps from fixed seeds (300,000 passed when written) of
    /// output that scrolls, rows written over in place, screens cleared,
    /// wrapped links, rows the writer ended in a link that runs on, and
    /// looks back and forward, over a history of 8 lines, so a trim takes
    /// the lines above a looked-back window. Named breaks: move the cells but
    /// not the lines in `shift` -- a moved row's line stays on the row it
    /// left; keep a moved top row whose line ran on from a row that left
    /// the window (`ran_on` off in `shift`) -- half a link stays once the
    /// trim takes its start.
    #[test]
    fn kept_lines_are_what_a_fresh_scan_finds() {
        for seed in [1u64, 7, 42, 2026] {
            let mut x = seed;
            let mut next = |m: usize| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                usize::try_from(x % u64::try_from(m).unwrap_or(1)).unwrap_or(0)
            };
            let mut owned = Owned::with_history(
                &WS,
                &baud_domain::CacheGeometry::UNKNOWN,
                baud_domain::Depth::of(8),
            );
            let mut t = Term::new(owned.storage(), &WS);
            let mut at = at();
            let (mut l, mut u) = (Lines::default(), Uris::default());
            for step in 0..1000 {
                let n = next(1000);
                let bytes = match next(8) {
                    0 | 1 => format!("\r\nsee https://s{n}.example/ ok"),
                    2 => format!("\r\nplain {n}"),
                    3 => format!("\x1b[{};1H\x1b[2Kat https://w{n}.example/", 1 + next(6)),
                    4 => format!("\r\n{}https://wrap{n}.example/", "x".repeat(30)),
                    // A row the writer ended at the last column in a URI,
                    // and the row it runs on into.
                    5 if next(2) == 0 => {
                        format!(
                            "\r\n{:>40}\r\nrest/{n} ok",
                            format!("https://j{n}.example/aaaa")
                        )
                    }
                    5 => "\r\n\r\n\r\n".to_string(),
                    6 => {
                        t.set_window_back(Rows::of_count(next(4)));
                        String::new()
                    }
                    _ => match next(3) {
                        0 => "\x1b[2J\x1b[H".to_string(),
                        _ => format!("\r\nmailto:m{n}@x.example"),
                    },
                };
                fed(&mut t, &mut at, bytes.as_bytes());
                let _ = l.scan_alone(&t, at, &mut u);
                assert_eq!(
                    (l.lines().to_vec(), l.struck().to_vec()),
                    fresh(&t, at),
                    "seed {seed}, step {step}: {bytes:?}"
                );
            }
        }
    }

    /// **A row that joined the next, written over, takes its link off the
    /// row below**: a URI the writer ended at the last column runs on into
    /// the next row and is one link across both; written over in place, the
    /// row below holds no link of its own, as a scan from nothing finds.
    /// Named break: drop the join's row from the rows read again -- the
    /// row below keeps its half of the old link.
    #[test]
    fn a_joined_row_written_over_takes_its_link_off_the_row_below() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = at();
        feed(
            &mut t,
            format!("top\r\n{:>40}\r\nrest/1 ok", "https://j1.example/aaaa").as_bytes(),
        );
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at, &mut u);
        assert!(
            runs(&l).iter().any(|r| r.0 == 2),
            "the control: the link runs on into row 2: {:?}",
            runs(&l)
        );
        feed(&mut t, b"\x1b[2;1H\x1b[2Kplain words");
        let _ = l.scan_alone(&t, at, &mut u);
        assert_eq!((l.lines().to_vec(), l.struck().to_vec()), fresh(&t, at));
        assert_eq!(runs(&l), []);
    }

    // The scan under seals: frames as the drivers make them, the scan and
    // then the seal the build takes, and each case asking whether the scan
    // reads what changed when no row of the window was written since the
    // seal the scan keeps.

    /// A frame as a driver makes it: the scan, then the seal. With the seal
    /// the scan read under, `term.sealed()` as it read.
    fn sealed_scan<R: Reader>(
        l: &mut Lines,
        t: &mut Term<'_>,
        at: Scrolled,
        r: &mut R,
    ) -> (Scan, baud_domain::Seal) {
        let kept = t.sealed();
        let s = l.scan_alone(t, at, r);
        let _ = t.seal_writes();
        (s, kept)
    }

    /// Two frames with nothing fed: the last one's seal covers every write
    /// before them, and is the one returned.
    fn settle<R: Reader>(
        l: &mut Lines,
        t: &mut Term<'_>,
        at: Scrolled,
        r: &mut R,
    ) -> baud_domain::Seal {
        let _ = sealed_scan(l, t, at, r);
        sealed_scan(l, t, at, r).1
    }

    /// The rows of the window drawn now written after `kept`.
    fn written(t: &Term<'_>, kept: &baud_domain::Seal) -> Vec<usize> {
        let back = t.window_back();
        let gens = t.row_gens(back);
        (0..t.view(back).rows().index())
            .filter(|&y| gens.after(Row::from_index(y), kept))
            .collect()
    }

    /// The lines and strikes `l` keeps.
    fn kept(l: &Lines) -> (Vec<LinkLine>, Vec<LinkLine>) {
        (l.lines().to_vec(), l.struck().to_vec())
    }

    /// **With no seal taken, a frame that reads no row compares every row
    /// with the copy**: six rows of links, every row written; the scan
    /// that reads every row compares none, and a frame that moves only
    /// the cursor reads nothing and compares all six. A row written once
    /// reads as written on every frame until a seal, so this holds with
    /// the write generation too. Named break: the window's compare not
    /// counted.
    #[test]
    fn an_unsealed_caret_frame_compares_every_row() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = at();
        let rows = usize::from(WS.ws_row);
        let screen: Vec<String> = (0..rows)
            .map(|n| format!("see https://u{n}.example/ ok"))
            .collect();
        feed(&mut t, screen.join("\r\n").as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan_alone(&t, at, &mut u);
        assert_eq!(runs(&l).len(), rows, "the control: a link a row");
        assert_eq!(
            l.compared(),
            Rows::ZERO,
            "a scan reading every row compares none"
        );
        feed(&mut t, b"\x1b[3;3H");
        assert_eq!(
            l.scan_alone(&t, at, &mut u),
            Scan::default(),
            "nothing read"
        );
        assert_eq!(l.compared(), Rows::of_count(rows), "every row compared");
    }

    /// **With no seal taken, a looked-back frame also compares the row
    /// below the window that the bottom row's read went by**: `/p13` on the
    /// bottom row of a window three lines back, running on into the row
    /// below; a frame with nothing written compares the six rows of the
    /// window and that one. Named break: the rows below not counted.
    #[test]
    fn an_unsealed_look_back_compares_the_row_below_it_went_by() {
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(24),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        for n in 0..10 {
            fed(&mut t, &mut at, format!("l{n}\r\n").as_bytes());
        }
        fed(
            &mut t,
            &mut at,
            format!("{:>40}\r\n3 ok\r\nz\r\ny", "/p1").as_bytes(),
        );
        t.set_window_back(Rows::of_count(3));
        let mut r = Found {
            found: vec![b"/p13".to_vec()],
        };
        let mut l = Lines::default();
        let _ = l.scan_alone(&t, at, &mut r);
        assert_eq!(runs(&l), [(5, 37, 39)], "the control: /p13 runs on below");
        assert_eq!(
            l.scan_alone(&t, at, &mut r),
            Scan::default(),
            "nothing read"
        );
        assert_eq!(
            l.compared(),
            Rows::of_count(usize::from(WS.ws_row) + 1),
            "the window's rows and the one below"
        );
    }

    /// **A look back reads the history rows it brings in, with no row
    /// written since the seal**: a link three lines into the history, the
    /// window looked back onto it; the rows brought in hold old
    /// generations, and the link is found on the row showing it. Named
    /// break: `shift` leaves the rows it empties trusted -- the link is
    /// never read.
    #[test]
    fn a_look_back_reads_the_history_rows_it_brings_in_under_a_seal() {
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(12),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        fed(&mut t, &mut at, b"see https://old.example/ ok");
        for n in 0..8 {
            fed(&mut t, &mut at, format!("\r\nplain {n}").as_bytes());
        }
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let seal = settle(&mut l, &mut t, at, &mut u);
        assert_eq!(runs(&l), [], "the control: the link is behind the window");
        t.set_window_back(Rows::of_count(3));
        assert_eq!(written(&t, &seal), [] as [usize; 0], "no row written");
        let _ = sealed_scan(&mut l, &mut t, at, &mut u);
        assert_eq!(kept(&l), fresh(&t, at));
        assert!(
            runs(&l).iter().any(|r| r.0 == 0),
            "the link on the row it shows: {:?}",
            runs(&l)
        );
    }

    /// **A switch of screens with nothing written reads the screen shown**:
    /// a link on the alternate screen, which `?47` does not clear, and one
    /// on the primary; each switch, with no row written since the seal,
    /// draws the lines a scan from nothing finds. Named break: trust a row
    /// by its generation alone, not the window it was kept over -- the
    /// other screen's lines stay.
    #[test]
    fn a_screen_switch_with_nothing_written_reads_the_screen_shown() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = at();
        feed(&mut t, b"see https://primary.example/ ok");
        feed(
            &mut t,
            b"\x1b[?47h\x1b[3;1Hgo https://alt.example/ ok\x1b[?47l",
        );
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let seal = settle(&mut l, &mut t, at, &mut u);
        assert_eq!(kept(&l), fresh(&t, at));
        assert!(
            runs(&l).iter().any(|r| r.0 == 0),
            "the control: the primary's link"
        );
        feed(&mut t, b"\x1b[?47h");
        assert_eq!(
            written(&t, &seal),
            [] as [usize; 0],
            "the alternate screen's rows were written before the seal"
        );
        let _ = sealed_scan(&mut l, &mut t, at, &mut u);
        assert_eq!(kept(&l), fresh(&t, at));
        assert!(
            runs(&l).iter().any(|r| r.0 == 2) && !runs(&l).iter().any(|r| r.0 == 0),
            "the alternate screen's link alone: {:?}",
            runs(&l)
        );
        let seal = sealed_scan(&mut l, &mut t, at, &mut u).1;
        feed(&mut t, b"\x1b[?47l");
        assert_eq!(written(&t, &seal), [] as [usize; 0], "back, none written");
        let _ = sealed_scan(&mut l, &mut t, at, &mut u);
        assert_eq!(kept(&l), fresh(&t, at));
        assert!(
            runs(&l).iter().any(|r| r.0 == 0) && !runs(&l).iter().any(|r| r.0 == 2),
            "the primary's link alone: {:?}",
            runs(&l)
        );
    }

    /// **A scroll the driver's count does not show reads every row**: the
    /// alternate screen's ring turns under output, and a count of the
    /// primary's lines does not move; the rows that moved up were written
    /// before the seal, and each link is found where it is now, every row's
    /// at another column. Named break: trust a row by its generation alone
    /// -- each row keeps the link of the row it showed before.
    #[test]
    fn a_scroll_the_count_does_not_show_reads_every_row() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = at();
        feed(&mut t, b"\x1b[?1049h");
        for n in 0..6 {
            let nl = if n == 0 { "" } else { "\r\n" };
            let pad = " ".repeat(n);
            feed(&mut t, format!("{nl}{pad}https://a{n}.example/").as_bytes());
        }
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let seal = settle(&mut l, &mut t, at, &mut u);
        assert_eq!(runs(&l).len(), 6, "the control: a link a row");
        let before = t.rotation();
        feed(&mut t, b"\r\n");
        assert!(
            matches!(t.moved_since(before), baud_core::Moved::Retired(_)),
            "the control: the ring turned"
        );
        assert_eq!(written(&t, &seal), [5], "only the row brought in");
        let _ = sealed_scan(&mut l, &mut t, at, &mut u);
        assert_eq!(kept(&l), fresh(&t, at));
        assert_eq!(runs(&l).len(), 5);
    }

    /// **`forget` reads every row, with none written**: what a folder
    /// reported, the machine's names handed, the config read again or an
    /// answer to every link asks. Named break: trust rows by their
    /// generations when the scan was told to read everything -- nothing is
    /// read.
    #[test]
    fn forget_reads_every_row_with_nothing_written() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = at();
        feed(
            &mut t,
            b"see https://f1.example/ ok\r\nplain\r\nmailto:m@x.example",
        );
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let seal = settle(&mut l, &mut t, at, &mut u);
        l.forget();
        assert_eq!(written(&t, &seal), [] as [usize; 0], "no row written");
        let (s, _) = sealed_scan(&mut l, &mut t, at, &mut u);
        assert!(s.rows >= 6, "every row read again: {}", s.rows);
        assert_eq!(kept(&l), fresh(&t, at));
    }

    /// **A sweep of the link table reads every row, with the rows kept
    /// unwritten**: a freed slot moves what a cell's word means with no
    /// change to the word, so the scan reads everything again. Built as
    /// `hyperlinks.rs` builds a sweep: two tables' worth of targets, the
    /// screen and history erased, a screen of text, then new targets on the
    /// bottom row. Named break: trust rows by their generations when the
    /// epoch moved -- only the bottom row is read.
    #[test]
    fn a_link_table_sweep_reads_every_row_with_none_written() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = at();
        let link = |uri: String, text: &str| format!("\x1b]8;;{uri}\x1b\\{text}\x1b]8;;\x1b\\");
        let mut v = String::new();
        for i in 0..2 * baud_domain::LinkBook::SLOTS {
            v.push_str(&link(format!("http://first.example/{i}"), "a"));
        }
        v.push_str("\x1b[H\x1b[2J\x1b[3J");
        v.push_str(&".".repeat(200 * 50));
        v.push_str("\x1b[H\x1b[2J\x1b[3J");
        v.push_str("see https://kept.example/ ok");
        feed(&mut t, v.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let seal = settle(&mut l, &mut t, at, &mut u);
        assert_eq!(runs(&l).len(), 1, "the control: the kept row's link");
        let epoch = t.link_epoch();
        let mut w = String::from("\x1b[6;1H");
        for i in 0..30 {
            w.push_str(&link(format!("http://second.example/{i}"), "b"));
        }
        feed(&mut t, w.as_bytes());
        assert_ne!(t.link_epoch(), epoch, "the control: a slot was freed");
        assert_eq!(written(&t, &seal), [5], "only the row the targets are on");
        let (s, _) = sealed_scan(&mut l, &mut t, at, &mut u);
        assert!(s.rows >= 6, "every row read again: {}", s.rows);
        assert_eq!(kept(&l), fresh(&t, at));
    }

    /// **An answer redraws a printed path with no row written**: a batch
    /// of answers moves no cell and no generation. Named break: end the
    /// scan when no row of the window was written since the seal and the
    /// window did not move -- the answer is never drawn.
    #[test]
    fn an_answer_with_nothing_written_still_redraws() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        fed(&mut t, &mut at, b"see /p1 and /p2 ok\r\nplain");
        let (mut l, mut r) = (Lines::default(), Answering::default());
        let seal = settle(&mut l, &mut t, at, &mut r);
        assert_eq!(l.lines(), &[] as &[LinkLine], "the control: none found");
        r.toggle("/p1");
        l.answered();
        assert_eq!(written(&t, &seal), [] as [usize; 0], "no row written");
        let (s, _) = sealed_scan(&mut l, &mut t, at, &mut r);
        assert_eq!(s.remarks, 1);
        assert_eq!(kept(&l), fresh_by(&t, at, &r));
        assert_eq!(runs(&l), [(0, 4, 6)]);
    }

    /// One CPU frame of `t`'s window onto `px`, the runs `l` keeps set on
    /// `r` as the driver sets them, as `damage` says.
    fn painted(
        r: &mut baud_render::Renderer<'_, '_>,
        t: &mut Term<'_>,
        l: &Lines,
        px: &mut Vec<u32>,
        damage: baud_render::Damage,
    ) {
        r.set_link_lines(l.lines(), baud_render::LineStyle::Solid);
        r.set_link_strikes(l.struck(), &[]);
        r.fit(t);
        let (w, h) = r.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        if px.len() != n {
            *px = vec![0; n];
        }
        let back = t.window_back();
        let mut target = baud_render::Target::new(&mut px[..], w, w, h).unwrap();
        let seal = t.seal_writes();
        let _ = r.render_at(t, seal, back, &mut target, damage);
    }

    /// **C6-19: an answer with nothing written draws its line on the next
    /// frame**, end to end: the reader's answer, `answered`, the scan, the
    /// setters the driver calls with its runs, and the renderer's next
    /// incremental CPU frame, which is a fresh renderer's full frame. The
    /// Paths generation reaches the renderer only through the runs the
    /// setters are handed. Named break: the renderer's lines copied with
    /// the key's inputs unmoved.
    #[test]
    fn an_answer_with_nothing_written_draws_its_line_the_next_frame() {
        use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).unwrap()];
        let made = || {
            Renderer::new(
                Stack::new(&faces).unwrap(),
                PxSize::of(14.0).unwrap(),
                Theme::dark(),
                Blend::Native,
                baud_domain::Cols::of_index(usize::from(WS.ws_col)),
                Rows::of_index(usize::from(WS.ws_row)),
            )
            .unwrap()
        };
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        fed(&mut t, &mut at, b"see /p1 and /p2 ok\r\nplain");
        let (mut l, mut a) = (Lines::default(), Answering::default());
        let (mut r, mut px) = (made(), Vec::new());
        for damage in [Damage::Full, Damage::Incremental, Damage::Incremental] {
            let _ = l.scan_alone(&t, at, &mut a);
            painted(&mut r, &mut t, &l, &mut px, damage);
        }
        assert_eq!(l.lines(), &[] as &[LinkLine], "the control: none found");
        let seal = t.sealed();
        let mut before = Vec::new();
        painted(&mut made(), &mut t, &l, &mut before, Damage::Full);
        a.toggle("/p1");
        l.answered();
        let _ = l.scan_alone(&t, at, &mut a);
        assert_eq!(runs(&l), [(0, 4, 6)], "the control: the answer draws");
        assert_eq!(written(&t, &seal), [] as [usize; 0], "no row written");
        let mut fresh = Vec::new();
        painted(&mut made(), &mut t, &l, &mut fresh, Damage::Full);
        assert!(fresh != before, "the control: the line shows");
        painted(&mut r, &mut t, &l, &mut px, Damage::Incremental);
        assert!(px == fresh, "the next frame draws the answer");
    }

    /// URIs, and one span of lines handed as refiled to the next scan.
    #[derive(Default)]
    struct Refiling {
        uris: Uris,
        span: Option<crate::paths::LineSpan>,
    }

    impl Reader for Refiling {
        fn wants(&self, tail: Tail) -> bool {
            self.uris.wants(tail)
        }

        fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> crate::paths::Looked {
            Reader::read(&mut self.uris, text, found)
        }

        fn marked(&mut self, uri: &[u8], at: At<'_>) -> Option<Mark> {
            Reader::marked(&mut self.uris, uri, at)
        }

        fn refiled(&mut self) -> Option<crate::paths::LineSpan> {
            self.span.take()
        }
    }

    /// **A span a read refiled is read again, with no row written**: a
    /// remote read filing renditions alone moves what a link on the lines
    /// reads and no cell. Named break: end the scan when no row of the
    /// window was written since the seal -- the row is not read.
    #[test]
    fn a_refiled_span_with_nothing_written_is_read_again() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = at();
        feed(&mut t, b"a\r\nb\r\nsee https://r.example/ ok\r\nd");
        let (mut l, mut r) = (Lines::default(), Refiling::default());
        let seal = settle(&mut l, &mut t, at, &mut r);
        let line = at.line(t.window_back(), Row::from_index(2));
        r.span = Some(crate::paths::LineSpan::of(line, line));
        assert_eq!(written(&t, &seal), [] as [usize; 0], "no row written");
        let (s, _) = sealed_scan(&mut l, &mut t, at, &mut r);
        assert!(s.rows >= 1, "the row refiled read again: {}", s.rows);
        assert_eq!(kept(&l), fresh(&t, at));
    }

    /// URIs, joining rows the writer ended as `joins` says.
    struct Joining {
        uris: Uris,
        joins: Joins,
    }

    impl Reader for Joining {
        fn wants(&self, tail: Tail) -> bool {
            self.uris.wants(tail)
        }

        fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> crate::paths::Looked {
            Reader::read(&mut self.uris, text, found)
        }

        fn marked(&mut self, uri: &[u8], at: At<'_>) -> Option<Mark> {
            Reader::marked(&mut self.uris, uri, at)
        }

        fn joins(&self, _: Screen, _: AbsLine) -> Joins {
            self.joins
        }
    }

    /// **A join answered otherwise reads both rows, with none written**: a
    /// URI the writer ended at the last column runs on into the next row
    /// while joins hold, and stops there once they do not, as when the
    /// program that wrote it is named. Named break: end the scan when no
    /// row of the window was written since the seal -- the link stays
    /// across both rows.
    #[test]
    fn a_join_answered_otherwise_with_nothing_written_reads_both_rows() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = at();
        feed(
            &mut t,
            format!("top\r\n{:>40}\r\nrest/1 ok", "https://j1.example/aaaa").as_bytes(),
        );
        let mut r = Joining {
            uris: Uris::default(),
            joins: Joins::Yes,
        };
        let mut l = Lines::default();
        let seal = settle(&mut l, &mut t, at, &mut r);
        assert!(
            runs(&l).iter().any(|x| x.0 == 2),
            "the control: joined, the link runs on into row 2: {:?}",
            runs(&l)
        );
        r.joins = Joins::No;
        assert_eq!(written(&t, &seal), [] as [usize; 0], "no row written");
        let _ = sealed_scan(&mut l, &mut t, at, &mut r);
        let mut apart = Lines::default();
        let _ = apart.scan_alone(
            &t,
            at,
            &mut Joining {
                uris: Uris::default(),
                joins: Joins::No,
            },
        );
        assert_eq!(kept(&l), kept(&apart));
        assert!(
            !runs(&l).iter().any(|x| x.0 == 2),
            "held apart, row 2 holds no part of it: {:?}",
            runs(&l)
        );
    }

    /// **A row below a looked-back window written over reads the bottom
    /// row again, with no row of the window written**: the rows below are
    /// outside the window's generations. Named break: end the scan when the
    /// window did not move and no row of the window is after the seal, read
    /// from the window's generations alone with nothing below or above it
    /// consulted -- `/p13`'s line stays.
    #[test]
    fn a_row_below_the_window_written_with_the_window_unwritten() {
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(24),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        for n in 0..10 {
            fed(&mut t, &mut at, format!("l{n}\r\n").as_bytes());
        }
        fed(
            &mut t,
            &mut at,
            format!("{:>40}\r\n3 ok\r\nz\r\ny", "/p1").as_bytes(),
        );
        t.set_window_back(Rows::of_count(3));
        let found = || Found {
            found: vec![b"/p13".to_vec()],
        };
        let (mut l, mut r) = (Lines::default(), found());
        let seal = settle(&mut l, &mut t, at, &mut r);
        assert_eq!(runs(&l), [(5, 37, 39)], "the control: /p13 runs on below");
        fed(&mut t, &mut at, b"\x1b[4;1H\x1b[2K/p10");
        assert_eq!(written(&t, &seal), [] as [usize; 0], "the window unwritten");
        let _ = sealed_scan(&mut l, &mut t, at, &mut r);
        let mut fresh = Lines::default();
        let _ = fresh.scan_alone(&t, at, &mut found());
        assert_eq!(runs(&fresh), [], "a scan from nothing: /p1/p10 is none");
        assert_eq!(runs(&l), runs(&fresh));
    }

    /// **A trim of the history where the top row's line began reads the
    /// top row again, with no row of the window written**: `trimmed_above`
    /// under seals. Named break: end the scan when the window did not move
    /// and no row of the window is after the seal, read from the window's
    /// generations alone with nothing below or above it consulted -- the
    /// half link stays.
    #[test]
    fn a_trim_above_the_window_with_the_window_unwritten() {
        let parts = [FIRST, SECOND];
        let depth = 10;
        let mut owned = Owned::with_history(
            &WS,
            &baud_domain::CacheGeometry::UNKNOWN,
            baud_domain::Depth::of(depth),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        fed(&mut t, &mut at, parts.join("\r\n").as_bytes());
        for n in 0..depth + usize::from(WS.ws_row) - parts.len() {
            fed(&mut t, &mut at, format!("\r\nline {n}").as_bytes());
        }
        t.set_window_back(Rows::of_count(depth + 1 - parts.len()));
        let link = SECOND.find(' ').unwrap_or(SECOND.len());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let seal = settle(&mut l, &mut t, at, &mut u);
        assert_eq!(
            runs(&l),
            [(0, 0, link - 1)],
            "the control, the link's last part on the top row"
        );
        fed(&mut t, &mut at, b"\r\none more");
        assert_eq!(written(&t, &seal), [] as [usize; 0], "the window unwritten");
        let _ = sealed_scan(&mut l, &mut t, at, &mut u);
        assert_eq!(runs(&l), [], "the half link goes");
        assert_eq!(kept(&l), fresh(&t, at));
    }

    /// **The lines a scan keeps under seals are the lines a scan from
    /// nothing finds**: `kept_lines_are_what_a_fresh_scan_finds`'s steps with
    /// a seal after every scan, as the drivers seal, and switches of screen
    /// (`?47`, which leaves the alternate screen as it was, and `?1049`)
    /// and a resize to the same size among them. The scan's count moves as
    /// the app moves it (`keep::Counts`, not [`fed`]): the primary's by what
    /// its window retired, the alternate screen's to its own number for its
    /// top line while it is up, so the alternate screen's own scrolling
    /// reaches the scan as a shift on that one screen. A switch moves the
    /// count too; the window the copy was kept over names the screen, so
    /// after a switch every row is compared whatever the shift. Controls:
    /// each seed looks back on the primary, and scrolls the alternate
    /// screen under its own count, at least once. Named breaks: `shift`
    /// leaving the rows it empties trusted, through the shifts on one
    /// screen (a look back on the primary, the alternate screen's own
    /// scrolling); `ran_on`'s row left trusted; trusting a row by its
    /// generation alone, not the window it was kept over, which a switch
    /// of screens reaches.
    #[test]
    fn kept_lines_are_what_a_fresh_scan_finds_under_seals() {
        for seed in [1u64, 7, 42, 2026] {
            let mut x = seed;
            let mut next = |m: usize| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                usize::try_from(x % u64::try_from(m).unwrap_or(1)).unwrap_or(0)
            };
            let mut owned = Owned::with_history(
                &WS,
                &baud_domain::CacheGeometry::UNKNOWN,
                baud_domain::Depth::of(8),
            );
            let mut t = Term::new(owned.storage(), &WS);
            let mut counts = crate::keep::Counts::new(&t);
            let mut at = counts.primary();
            let (mut l, mut u) = (Lines::default(), Uris::default());
            // Steps that shifted the window on one screen: a look back on
            // the primary, and the alternate screen's count moved.
            let (mut looked, mut alternate) = (0, 0);
            for step in 0..1000 {
                let (was_at, was_back, was_screen) = (at, t.window_back(), t.screen());
                let n = next(1000);
                let bytes = match next(10) {
                    0 | 1 => format!("\r\nsee https://s{n}.example/ ok"),
                    2 => format!("\r\nplain {n}"),
                    3 => format!("\x1b[{};1H\x1b[2Kat https://w{n}.example/", 1 + next(6)),
                    4 => format!("\r\n{}https://wrap{n}.example/", "x".repeat(30)),
                    5 if next(2) == 0 => {
                        format!(
                            "\r\n{:>40}\r\nrest/{n} ok",
                            format!("https://j{n}.example/aaaa")
                        )
                    }
                    5 => "\r\n\r\n\r\n".to_string(),
                    6 => {
                        t.set_window_back(Rows::of_count(next(4)));
                        String::new()
                    }
                    7 => match next(4) {
                        0 => "\x1b[?47h".to_string(),
                        1 => "\x1b[?47l".to_string(),
                        2 => "\x1b[?1049h".to_string(),
                        _ => "\x1b[?1049l".to_string(),
                    },
                    8 => {
                        let _ = t.resize(&WS);
                        String::new()
                    }
                    _ => match next(3) {
                        0 => "\x1b[2J\x1b[H".to_string(),
                        _ => format!("\r\nmailto:m{n}@x.example"),
                    },
                };
                feed(&mut t, bytes.as_bytes());
                let rec = t.take_record();
                at = counts.numbered(&t, Some(&rec)).scrolled();
                match (was_screen, t.screen()) {
                    (Screen::Normal, Screen::Normal) if t.window_back() > was_back => looked += 1,
                    (Screen::Alternate, Screen::Alternate) if at != was_at => alternate += 1,
                    _ => {}
                }
                let _ = sealed_scan(&mut l, &mut t, at, &mut u);
                assert_eq!(
                    kept(&l),
                    fresh(&t, at),
                    "seed {seed}, step {step}: {bytes:?}"
                );
            }
            assert!(looked > 0, "the control, seed {seed}: a look back");
            assert!(
                alternate > 0,
                "the control, seed {seed}: the alternate screen scrolled under its count"
            );
        }
    }

    /// **The view's synced slots are each row's cells' under seals**: from
    /// four seeds, 1,000 steps each of `OSC 8` links on new lines, at a
    /// cursor address and left open across a line, plain lines, erasures,
    /// looks back, switches of screen and a resize to the same size, with
    /// the frame sealed after the sync but one step in four; after every
    /// sync each row's set is `LinkSet::of_cells` of the row the view shows,
    /// and the sync is fresh until the next byte. The scan's count moves as
    /// the app moves it (`keep::Counts`). Control: each seed moves the
    /// window on one screen. Named breaks: leave the sets where they were in
    /// `ViewLinks::moved`; keep the rows a move brought in as read.
    #[test]
    fn synced_slots_are_each_rows_cells_under_seals() {
        for seed in [3u64, 11, 64, 2027] {
            let mut x = seed;
            let mut next = |m: usize| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                usize::try_from(x % u64::try_from(m).unwrap_or(1)).unwrap_or(0)
            };
            let mut owned = Owned::with_history(
                &WS,
                &baud_domain::CacheGeometry::UNKNOWN,
                baud_domain::Depth::of(8),
            );
            let mut t = Term::new(owned.storage(), &WS);
            let mut counts = crate::keep::Counts::new(&t);
            let mut at = counts.primary();
            let mut views = ViewLinks::default();
            let mut moved = 0;
            for step in 0..1000 {
                let (was_at, was_screen) = (at, t.screen());
                let n = next(1000);
                let link =
                    |n: usize| format!("\x1b]8;;https://v{n}.example/\x1b\\v{n}\x1b]8;;\x1b\\");
                let bytes = match next(10) {
                    0 | 1 => format!("\r\nsee {}", link(n % 90)),
                    2 => format!("\r\nplain {n}"),
                    3 => format!("\x1b[{};{}H{}", 1 + next(6), 1 + next(30), link(n % 90)),
                    4 => "\r\n\r\n\r\n".to_string(),
                    5 => {
                        t.set_window_back(Rows::of_count(next(4)));
                        String::new()
                    }
                    6 => match next(4) {
                        0 => "\x1b[?47h".to_string(),
                        1 => "\x1b[?47l".to_string(),
                        2 => "\x1b[?1049h".to_string(),
                        _ => "\x1b[?1049l".to_string(),
                    },
                    7 => {
                        let _ = t.resize(&WS);
                        String::new()
                    }
                    8 => "\x1b[2J\x1b[H".to_string(),
                    _ => format!(
                        "\r\n\x1b]8;;https://o{n}.example/\x1b\\open\r\nstill\x1b]8;;\x1b\\"
                    ),
                };
                feed(&mut t, bytes.as_bytes());
                let rec = t.take_record();
                at = counts.numbered(&t, Some(&rec)).scrolled();
                if t.screen() == was_screen && at != was_at {
                    moved += 1;
                }
                let fresh = views.sync(&t, at);
                let view = t.view(t.window_back());
                for y in 0..view.rows().index() {
                    let r = Row::from_index(y);
                    assert_eq!(
                        fresh.row(r),
                        LinkSet::of_cells(view.row(r)),
                        "seed {seed}, step {step}, row {y}: {bytes:?}"
                    );
                }
                assert!(views.fresh(&t, at).is_some(), "fresh until the next byte");
                if next(4) != 0 {
                    let _ = t.seal_writes();
                }
            }
            assert!(moved > 0, "the control, seed {seed}: the window moved");
        }
    }

    /// **A `Fresh` is made only by a sync, or by `ViewLinks::fresh` while
    /// nothing moved**: no reader holds slots it did not get from the view's
    /// own sync. baud-app runs no `compile_fail`, so the source is read:
    /// every struct literal of it in lined.rs is in `ViewLinks::sync` or
    /// `ViewLinks::fresh`, and no other file of the crate spells one.
    /// Positive control: the reader finds the three those two make. Named
    /// break: a `Fresh` built in `Lines::scan` -- found outside them.
    #[test]
    fn a_fresh_is_made_only_by_a_sync() {
        // Spelled apart, so this test's own text is not what it finds.
        let lit = concat!("Fresh", " {");
        let src = include_str!("lined.rs");
        let code = &src[..src.find("#[cfg(test)]\nmod tests").expect("the tests")];
        let body = crate::no_appkit::fn_body;
        let made = body(code, concat!("    pub fn ", "sync<'v>("))
            .matches(lit)
            .count()
            + body(code, concat!("    pub fn ", "fresh(&self"))
                .matches(lit)
                .count();
        assert_eq!(made, 3, "the control: sync's two and fresh's one");
        assert_eq!(code.matches(lit).count(), made, "one made elsewhere");
        for (name, other) in [
            ("app.rs", crate::app_source::app_source()),
            ("link.rs", include_str!("link.rs")),
            ("link_layer.rs", include_str!("link_layer.rs")),
            ("linux.rs", include_str!("linux.rs")),
            ("paths.rs", include_str!("paths.rs")),
            ("probe.rs", include_str!("probe.rs")),
            ("seat.rs", include_str!("seat.rs")),
        ] {
            assert!(!other.contains(lit), "{name} makes one");
        }
    }

    /// **A caret frame compares no row**: 200 by 50, a link on every row at
    /// its own column, settled under seals; a frame that moves only the
    /// cursor reads nothing and compares no row with the copy, where the
    /// scan before the write generation compared all 50. Control: one cell
    /// written mid-screen compares that row alone, and the lines are a
    /// scan from nothing's. Named break: the generation ignored -- every row
    /// compared.
    #[test]
    fn a_caret_frame_compares_no_row() {
        const BIG: WinSize = WinSize {
            ws_col: 200,
            ws_row: 50,
        };
        let mut owned = Owned::new(&BIG);
        let mut t = Term::new(owned.storage(), &BIG);
        let at = at();
        let screen: Vec<String> = (0..50)
            .map(|n| format!("{}see https://c{n}.example/ ok", " ".repeat(n)))
            .collect();
        feed(&mut t, screen.join("\r\n").as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = settle(&mut l, &mut t, at, &mut u);
        assert_eq!(runs(&l).len(), 50, "the control: a link a row");
        feed(&mut t, b"\x1b[3;3H");
        let (s, _) = sealed_scan(&mut l, &mut t, at, &mut u);
        assert_eq!(s, Scan::default(), "nothing read");
        assert_eq!(l.compared(), Rows::ZERO, "no row compared");
        feed(&mut t, b"\x1b[25;100Hx");
        let _ = sealed_scan(&mut l, &mut t, at, &mut u);
        assert_eq!(l.compared(), Rows::of_count(1), "the row written alone");
        assert_eq!(kept(&l), fresh(&t, at));
    }
}
