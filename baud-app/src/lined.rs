//! **A link's line at rest**: the scan that finds, on a frame, what is drawn
//! underlined with no modifier held -- an `OSC 8` run and a URI printed as
//! plain text -- pure over the core's cells, so its tests run on every
//! platform and the `frame` bench compiles this file as it stands.
//!
//! **Only what changed is read.** [`Lines`] keeps the window's cells as the
//! last scan saw them and reads a row only when its cells, its wrap or its
//! line size differ, with the rest of the logical line it belongs to, since a
//! URI runs across the rows a soft wrap joined. A frame that moved only the
//! cursor reads no row; it asks again only whether a row running on into the
//! next joins it (`Below`), two cells and a provenance a row so ended, since a
//! program is named after it wrote. This is a record of what changed, and nothing is
//! kept against a row's content: a scroll moves every row's cells, and every
//! row is read again.
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
//! **A link baud knows would open nothing is struck, not underlined**
//! ([`Lines::struck`], [`Mark::Struck`]): what the reader says, from the
//! worker's answers it was handed, never from a call made here. At rest that
//! is a link whose kind has no app to open it, and an `OSC 8` `file://` link
//! naming this machine by name whose file the worker found gone (`paths.rs`
//! has the rules). A cell a program already strikes with `SGR 9` keeps its
//! own strike, and a concealed cell and a double-size line get none, as for
//! the line. Struck or not, the cells are the program's: a struck link
//! selects and copies as it is written.
#![forbid(unsafe_code)]

use core::ops::Range;

use baud_core::{Screen, Term};
use baud_domain::{
    Attrs, ClusterEpoch, Col, LineRendition, LinkEpoch, LinkId, PackedCell, Row, RowStyles, Rows,
    StyleWord, Wrap,
};
use baud_render::{AbsLine, LinkLine, Point, Scrolled, row_of, row_styles_of};

use crate::link::{
    Class, Edge, Host, Joins, MAX, Target, Unit, admit_with, class, found_in, may_be_cut, runs_on,
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
    /// The screen it is on.
    pub screen: Screen,
}

/// **How a link is drawn at rest**: underlined, as a link a click opens, or
/// struck, as one baud knows would open nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Line,
    Struck,
}

/// **A link a reader found**: its units in the line's text, and its mark.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Run {
    pub units: Range<usize>,
    pub mark: Mark,
}

/// Where an `OSC 8` run is: its line and the screen it is on, which say
/// whether output there is known to be this machine's (`paths.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct At {
    pub line: AbsLine,
    pub screen: Screen,
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
    fn marked(&mut self, uri: &[u8], at: At) -> Option<Mark>;

    /// A scan found a row of the window changed: called once a scan, before
    /// any row is read, so it costs a frame one call however many changed.
    fn changed(&mut self) {}

    /// Whether the row the writer ended at `above` may be read on into the
    /// next, as `PathsAt::joins` says: yes with nothing to go by.
    fn joins(&self, _above: AbsLine) -> Joins {
        Joins::Yes
    }
}

/// **URIs, by `crate::link`'s rules**: `find`'s for what one is and `admit`'s
/// for whether it opens.
///
/// **This machine's name is given to it and never read by it.** The call that
/// answers may wait on a lookup, so the driver reads it at startup, on macOS
/// on a thread of its own (`Hosts`), and hands it over with [`Uris::set_host`].
/// Until then a `file://` link naming a host is what `admit` makes of an
/// unknown host: refused, with no line, and `file:///` and `localhost` open.
///
/// **Your `[link.*]` rules are handed to it too** ([`Uris::set_rules`]): a
/// scheme a rule adds is read and drawn as baud's own are, and admitted by
/// the same gate a click goes through.
#[derive(Debug, Default)]
pub struct Uris {
    host: Option<String>,
    bytes: Vec<u8>,
    rules: Rules,
}

impl Uris {
    /// This machine's name, once known: a line it changes is read again by
    /// the caller's [`Lines::forget`].
    pub fn set_host(&mut self, host: String) {
        self.host = Some(host);
    }

    /// Whether this machine's name has been handed over yet.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn host_known(&self) -> bool {
        self.host.is_some()
    }

    /// What `admit` reads a `file://` host against.
    pub fn host(&self) -> &str {
        self.host.as_deref().unwrap_or("")
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

impl Reader for Uris {
    #[inline]
    fn wants(&self, tail: Tail) -> bool {
        scheme_ends(tail, &self.rules)
    }

    fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> crate::paths::Looked {
        crate::paths::Looked(self.read_marking(text, found, &|_| Mark::Line))
    }

    fn marked(&mut self, uri: &[u8], _: At) -> Option<Mark> {
        admit_with(uri, Host(self.host()), &self.rules)
            .is_ok()
            .then_some(Mark::Line)
    }
}

impl Uris {
    /// [`Reader::read`], each URI `admit` opens marked as `mark` says of its
    /// target: a reader that knows which kinds have no app, or which rules'
    /// commands are missing, strikes those.
    pub fn read_marking(
        &mut self,
        text: Text<'_>,
        found: &mut Vec<Run>,
        mark: &dyn Fn(&Target<'_>) -> Mark,
    ) -> usize {
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
                    (lo, hi),
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
                    if let Ok(t) = admit_with(&bytes, Host(self.host()), &self.rules) {
                        found.push(Run {
                            units: span,
                            mark: mark(&t),
                        });
                    }
                    self.bytes = bytes;
                }
                from = end;
                q = end;
            }
        }
        candidates
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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Read {
    #[default]
    Kept,
    Again,
}

/// **The lines on the window, and the window as the last scan saw it.**
#[derive(Debug, Default)]
pub struct Lines {
    cells: Vec<PackedCell>,
    wraps: Vec<Wrap>,
    sizes: Vec<LineRendition>,
    size: (usize, usize),
    epochs: Option<(LinkEpoch, ClusterEpoch)>,
    /// Which rows this scan reads.
    again: Vec<Read>,
    lines: Vec<LinkLine>,
    struck: Vec<LinkLine>,
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

    /// Read every row at the next scan.
    pub fn forget(&mut self) {
        self.epochs = None;
    }

    /// **`[link] urls = "off"`**: no line, and nothing read. The scan's
    /// memory is kept, since a config read again may turn it back on.
    pub fn off(&mut self) {
        self.lines.clear();
        self.struck.clear();
        self.forget();
    }

    /// **The scan**, on the view shown now, `scrolled` the driver's count of
    /// lines retired. Allocates nothing once its buffers have grown to the
    /// window and to the longest line read.
    pub fn scan<R: Reader>(&mut self, term: &Term<'_>, scrolled: Scrolled, reader: &mut R) -> Scan {
        let back = term.window_back();
        let view = term.view(back);
        let (cols, rows) = (view.cols().index(), view.rows().index());
        if self.size != (cols, rows) {
            self.size = (cols, rows);
            self.cells.clear();
            self.cells.resize(cols * rows, PackedCell::EMPTY);
            self.wraps.clear();
            self.wraps.resize(rows, Wrap::Ended);
            self.sizes.clear();
            self.sizes.resize(rows, LineRendition::Single);
            self.again.clear();
            self.again.resize(rows, Read::Kept);
            self.below.clear();
            self.below.resize(rows, Below::Apart);
            self.head = None;
            self.lines.clear();
            self.struck.clear();
            self.epochs = None;
        }
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
        for y in 0..rows {
            let r = Row::from_index(y);
            let row = view.row(r);
            let n = row.len().min(cols);
            let kept = &mut self.cells[y * cols..y * cols + n];
            let (wrap, size) = (view.wrapped(r), view.rendition(r));
            let same = !all
                && !wrap_moved
                && kept == &row[..n]
                && self.wraps[y] == wrap
                && self.sizes[y] == size;
            wrap_moved = self.wraps[y] != wrap;
            self.again[y] = match same {
                true => Read::Kept,
                false => Read::Again,
            };
            if !same {
                kept.copy_from_slice(&row[..n]);
                self.wraps[y] = wrap;
                self.sizes[y] = size;
                any = true;
            }
        }
        // A row running on into the next is asked again whether it joins,
        // with no change to either: the program that wrote it may have been
        // named since, and a remote row never joins a local one.
        let meets = |reader: &R, y: usize| {
            let line = scrolled.line(back, Row::from_index(y));
            match runs_on(term, scrolled, line) {
                false => Below::Apart,
                true => match reader.joins(line) {
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
        // The history trimmed where the top row's line began: the top row is
        // read again, and its line now begins at the oldest line kept.
        if let (Some(head), Some(Read::Kept)) = (self.head, self.again.first()) {
            if row_of(term, scrolled, head).is_none() {
                self.again[0] = Read::Again;
                any = true;
            }
        }
        let mut scan = Scan::default();
        if !any {
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
        let mut top = 0;
        let mut groups = core::mem::take(&mut self.groups);
        groups.clear();
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
        let again = &self.again;
        self.lines.retain(|l| again[l.row.index()] == Read::Kept);
        self.struck.retain(|l| again[l.row.index()] == Read::Kept);
        let mut found = core::mem::take(&mut self.found);
        for g in &groups {
            let rows = (g.start, g.end - 1);
            self.read_group(term, scrolled, back, rows, reader, &mut found, &mut scan);
        }
        self.found = found;
        self.groups = groups;
        self.lines
            .sort_unstable_by_key(|l| (l.row.index(), l.from.index()));
        self.struck
            .sort_unstable_by_key(|l| (l.row.index(), l.from.index()));
        scan
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
        (top, bottom): (usize, usize),
        reader: &mut R,
        found: &mut Vec<Run>,
        scan: &mut Scan,
    ) {
        let (_, rows) = self.size;
        let first_row = scrolled.line(back, Row::from_index(top));
        let last_row = scrolled.line(back, Row::from_index(bottom));
        // Whether the line runs on from `l` into the next.
        let on = |reader: &R, l: AbsLine, wrap: Wrap| {
            wrap == Wrap::Continues || (runs_on(term, scrolled, l) && reader.joins(l) == Joins::Yes)
        };
        // Past the window's top, while the rows above run into it.
        let (mut first, mut head) = (first_row, Edge::Line);
        if top == 0 {
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
        if bottom + 1 == rows {
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
                    self.taken.push((from, to));
                    self.push_run(term, scrolled, back, run, scan);
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
                    self.push_run(term, scrolled, back, run, scan);
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
            for (x, &c) in cells.iter().enumerate() {
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
                    screen: term.screen(),
                },
                found,
            )
            .0;
    }

    /// One run a reader found, its cells on the window lined or struck; none
    /// for one a program may have cut (`may_be_cut`).
    fn push_run(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        back: Rows,
        run: &Run,
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
                Mark::Line => {
                    push_cell(&mut self.lines, term, styles, (y, p.col, cell), size, scan)
                }
                Mark::Struck => {
                    if push_struck(&mut self.struck, term, styles, (y, p.col, cell), size) {
                        scan.struck += 1;
                    }
                }
            }
        }
    }

    /// The `OSC 8` runs on rows `top..=bottom`, each link's target admitted
    /// once a run.
    fn osc8<R: Reader>(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        back: Rows,
        (top, bottom): (usize, usize),
        reader: &mut R,
        scan: &mut Scan,
    ) {
        let mut last: Option<(LinkId, Option<Mark>)> = None;
        let screen = term.screen();
        for y in top..=bottom {
            let r = Row::from_index(y);
            let line: AbsLine = scrolled.line(back, r);
            let Some((cells, _)) = row_of(term, scrolled, line) else {
                continue;
            };
            let styles = row_styles_of(term, scrolled, line);
            for (x, &cell) in cells.iter().enumerate() {
                let id = cell.flags.link();
                if id == LinkId::NONE {
                    continue;
                }
                let mark = match last {
                    Some((l, m)) if l == id => m,
                    _ => {
                        let m = term
                            .link_uri(id)
                            .and_then(|u| reader.marked(u, At { line, screen }));
                        last = Some((id, m));
                        m
                    }
                };
                let at = (r, Col::at_index(x), cell);
                match mark {
                    Some(Mark::Line) => {
                        push_cell(&mut self.lines, term, styles, at, self.sizes[y], scan);
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
        let _ = l.scan(&t, at(), &mut Uris::default());
        runs(&l)
    }

    /// A scan as the driver makes it with `[link] join_rows = "off"`: through
    /// `paths::Links`, its `Paths` holding every row the writer ended apart.
    fn scan_apart(l: &mut Lines, t: &Term<'_>, u: &mut Uris) -> Scan {
        let mut p = crate::paths::Paths::default();
        let _ = p.set_join(crate::paths::Turned::Off);
        l.scan(
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
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 4, 39), (1, 0, 11)], "the control");
        // The row is one logical line with the next; only row 0's cells move.
        feed(&mut t, b"\x1b[1;5HX");
        let s = l.scan(&t, at(), &mut u);
        assert_eq!(runs(&l), [], "the second row did not change, and goes");
        assert_eq!(s.rows, 2, "the logical line's rows, no more");
        // And a change on the second row reads the first.
        feed(&mut t, b"\x1b[1;5Hh\x1b[2;6H ");
        let s = l.scan(&t, at(), &mut u);
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
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 4, 39), (1, 0, 11)], "the control");
        feed(&mut t, b"\x1b[1;1H\x1b[2K");
        let _ = l.scan(&t, at(), &mut u);
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
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(
            runs(&l),
            [(1, 0, 8), (2, 0, 17)],
            "unknown: named hosts refused"
        );
        u.set_host("here.example".to_string());
        assert!(u.host_known());
        let s = l.scan(&t, at(), &mut u);
        assert_eq!(s, Scan::default(), "nothing changed, so nothing is read");
        l.forget();
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(
            runs(&l),
            [(0, 0, 20), (1, 0, 8), (2, 0, 17)],
            "this machine's opens, another's does not"
        );
        let src = include_str!("app.rs");
        for start in [
            concat!("fn scan_", "lines(&mut self) {"),
            concat!(
                "fn read_hover_",
                "as(&mut self, chord: crate::link::Chord) -> bool {"
            ),
        ] {
            let (_, body) = src.split_once(start).expect(start);
            let (body, _) = body.split_once("\n    }\n").expect("its end");
            assert!(!body.is_empty(), "the control");
            assert!(!body.contains(concat!("host_", "name(")), "{start}");
        }
        let (_, at_tick) = src
            .split_once(concat!("fn link_host_", "check(&mut self) {"))
            .expect("the driver hands the name over");
        let (at_tick, _) = at_tick.split_once("\n    }\n").expect("its end");
        assert!(
            at_tick.contains(concat!(".hosts", ".names()")),
            "the thread's read"
        );
        assert!(at_tick.contains(concat!("uris.", "set_host(")));
        assert!(at_tick.contains(concat!("lines.", "forget()")));
        // A click takes the answer the line took: the driver it runs in needs
        // a window, so the seam here is its source and `admit` on the same name.
        assert!(
            !src.contains(concat!("opener::", "host_name(")),
            "a click reads no name"
        );
        assert!(
            src.contains(concat!(
                "admit_printed(\n            uri,\n            Host(self.app.uris.",
                "host()),\n            self.app.uris.rules(),"
            )),
            "a click admits against the name the lines do"
        );
        let unknown = Uris::default();
        assert_eq!(
            admit(b"file://here.example/x", Host(unknown.host())),
            Err(crate::link::Refused::Host),
            "unknown: a click refuses what has no line"
        );
        assert!(admit(b"file:///y", Host(unknown.host())).is_ok());
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
        let first = l.scan(&t, at(), &mut u);
        assert_eq!(
            first.rows, 6,
            "the control: the first scan reads the window"
        );
        assert_eq!(first.spans, 1);
        feed(&mut t, b"\x1b[3;3H");
        let s = l.scan(&t, at(), &mut u);
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
        let _ = l.scan(&t, at(), &mut Uris::default());
        assert!(!l.lines().is_empty());
        l.off();
        assert!(l.lines().is_empty());
        for (name, src) in [
            ("app.rs", include_str!("app.rs")),
            ("linux.rs", include_str!("linux.rs")),
        ] {
            let at = src
                .find(concat!("self.link_config.", "lines()"))
                .expect(name);
            let scan = src.find(concat!("self.lines.", "scan(")).expect(name);
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
        let s = l.scan(&t, at(), &mut u);
        let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
        let limit = cols * rows + 2 * (MAX + 2 * cols);
        assert!(s.cells <= limit, "{} cells read, past {limit}", s.cells);
        assert!(!l.lines().is_empty(), "the control: it read links");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let long = format!("https://a.example/{}", "b".repeat(2 * MAX + 100));
        feed(&mut t, long.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let s = l.scan(&t, at(), &mut u);
        assert!(s.cells <= limit, "{} cells read, past {limit}", s.cells);
        assert_eq!(runs(&l), [], "past the cap, refused whole");
        // A run of closers costs one pass (`found_in`'s counts).
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let closers = format!("https://a.example/{}", ")".repeat(200));
        feed(&mut t, closers.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan(&t, at(), &mut u);
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
        let _ = l.scan(&t, at(), &mut u);
        assert!(!l.lines().is_empty(), "the control");
        for line in l.lines() {
            for x in line.from.index()..=line.to.index() {
                let on = at().line(Rows::ZERO, line.row);
                let w = window(&t, at(), on, Col::at_index(x)).expect("a window");
                let f = find(&w).expect("a URI under a line");
                let (_, verdict) = f.parts();
                assert_eq!(verdict, Ok(()), "column {x}");
                let bare = w.bare().expect("its bytes");
                assert!(admit(bare.uri().unwrap(), Host("")).is_ok(), "column {x}");
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
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 0, 39), (1, 0, 8)]);
        feed(&mut t, b"\x1b[2;1H ");
        let _ = l.scan(&t, at(), &mut u);
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
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 4, 22)], "the control");
        feed(&mut t, b"\x1b[2;20Hx");
        assert_eq!(l.scan(&t, at(), &mut u).rows, 2, "row 1 and the row above");
        feed(&mut t, b"\x1b[5;20Hx");
        assert_eq!(l.scan(&t, at(), &mut u).rows, 2, "row 4 and the row above");
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
        let _ = l.scan(&t, scrolled, &mut u);
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
        let _ = l.scan(&t, scrolled, &mut u);
        let mut fresh = Lines::default();
        let _ = fresh.scan(&t, scrolled, &mut Uris::default());
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
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(runs(&l), [], "the control: off the screen");
        // Fourteen lines on a screen of six: eight behind it.
        t.set_window_back(Rows::of_count(8));
        let s = l.scan(&t, at(), &mut u);
        assert_eq!(
            s.rows, 3,
            "every row the window changed read, and none else"
        );
        assert_eq!(runs(&l), [(0, 0, 18)], "the line on the row it shows");
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
        let (s, key, none) = (at(), Focus::Gained, Host(""));
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
            assert!(admit(uri.as_bytes(), Host("")).is_ok(), "{uri}");
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
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(runs(&l), [], "the control: no rule, no line");
        u.set_rules(rules);
        l.forget();
        let _ = l.scan(&t, at(), &mut u);
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
        let _ = l.scan(&t, at(), &mut u);
        assert_eq!(runs(&l), [(0, 0, 17), (1, 0, 6), (2, 0, 16)], "the control");
        let sections = [crate::rules::Section {
            name: "mailto".into(),
            fields: vec![("action".into(), "\"off\"".into())],
        }];
        let (rules, said) = crate::rules::Section::read_all(&sections, None, None);
        assert_eq!(said, Vec::<String>::new());
        u.set_rules(rules);
        l.forget();
        let _ = l.scan(&t, at(), &mut u);
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
        let _ = l.scan(&t, at(), &mut u);
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
                include_str!("app.rs"),
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
            ("app.rs", include_str!("app.rs")),
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
}
