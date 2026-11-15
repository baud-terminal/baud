//! **A program's scroll, read off the screen, so a cursor trail can take it
//! away.** When the cursor a trail follows changes cell, the text under it
//! may have moved with it: helix redraws every row on a wheel step and
//! carries its cursor three rows with its line, and vim and neovim scroll a
//! region and do the same on a wheel step, `zz`, `zt` or `Ctrl-E`. A trail
//! across those rows draws a jump the cursor never made in its text.
//!
//! **No standard has the program say which it was.** ECMA-48's SU moves
//! "the data in the presentation component ... such that the data appear to
//! move up", and "The active presentation position is not affected by this
//! control function" (`[[source: ecma48]]` 8.3.147; SD, 8.3.113, the same
//! down), so a cursor that keeps its line through a scroll was moved by the
//! program afterwards, the way it is moved on a jump, and helix sends no
//! scroll at all. A trail is decoration over the cells, which no terminal
//! standard reaches, so this changes no cell, no cursor and no report.
//!
//! **The rule is Neovide's, with the amount read off the screen**: Neovide
//! moves its corners by the editor's own scroll and counts a jump only for
//! the movement beyond it (`[[source: neovide-cursor-renderer]]` 359-365).
//! A terminal is told no amount, but it has the screen before and after:
//!
//! * **The pane** is the columns between the nearest border lines either
//!   side of the cursor's old column -- a column that reads the same
//!   vertical line on every row but the first and the last, before and after,
//!   as a split's border does -- or the whole row.
//! * **Every written row of the pane that no other row repeated votes** for
//!   each distance at which it reads again after. The distance with the most
//!   votes, at least [`QUORUM`] and more than the rows that stayed, is the
//!   program's scroll -- if the cursor's old row went with it: its own text,
//!   or, for a blank or repeated row, a block round it that no written row
//!   left unmoved.
//! * **Clamped** so the cursor's old row stays on the screen.
//!
//! The trail then moves what it holds of the text by that much
//! ([`Chase::rebase`], the ghost's last cell and its sweep), so only what the
//! cursor moved beyond its text trails, long or snapped as typing is. Too
//! little left of the old screen to vote -- fewer than three written rows, as
//! after `zb` with the cursor on the top line -- and the move trails as it
//! did. Under `[cursor] trail_scroll = "stay"` nothing moves with the text,
//! so nothing is taken away; and a frame whose read scrolled the whole
//! screen is the ride's (`trail_rides`), which keeps the rest state where it
//! was.
//!
//! **The cost is a copy of the screen's words on a frame that read
//! something**, as only then can the text have moved, and nothing at all
//! while the trail is off, `trail_scroll` is `stay`, or no cursor is
//! followed; the vote runs once a move, through an index from a row's hash
//! to the row rather than a search over the distances.

use baud_core::{Content, Screen, Term};
use baud_domain::{Col, Cols, Row, Rows};

use crate::bell::Moment;
use crate::config::{CursorTrail, TrailScroll};
use crate::trail::Chase;

/// The least votes a distance needs to be the program's scroll: two rows
/// that match by chance at one distance are common on code, three rare.
pub const QUORUM: usize = 3;

/// The lines a split's border is drawn in: tmux's and vim's `│` and `|`,
/// the heavy and double rules, and the dashed ones.
const BORDERS: [u32; 8] = [0x7C, 0x2502, 0x2503, 0x2551, 0x2506, 0x2507, 0x250A, 0x250B];

/// A cell nothing was written to, read as the space it shows.
const BLANK: u32 = 0x20;

/// **How far a program's scroll took the text under the cursor**, rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Carry {
    Up(Rows),
    Down(Rows),
}

impl Carry {
    /// The pixels it moved the text, y down, on cells `cell_h` tall.
    pub(crate) fn px(self, cell_h: f32) -> f32 {
        match self {
            Carry::Up(n) => -(n.index() as f32) * cell_h,
            Carry::Down(n) => n.index() as f32 * cell_h,
        }
    }

    /// Where it took row `r`: never above the first.
    pub(crate) fn row(self, r: Row) -> Row {
        match self {
            Carry::Up(n) => Row::from_index(r.index().saturating_sub(n.index())),
            Carry::Down(n) => Row::from_index(r.index() + n.index()),
        }
    }
}

/// What the terminal had read and written when a screen was taken: one
/// that has read nothing since shows the same cells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fed(usize, usize);

/// A screen's geometry, surface and reading.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shape {
    cols: Cols,
    rows: Rows,
    screen: Screen,
    fed: Fed,
}

/// A row's hash over the pane's words.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Hash(u64);

/// **A hash of `words`**, FNV-1a's step over four lanes of two words
/// each, folded at the end: four independent multiplies an eight words
/// rather than one chain a word, whose latency would be the vote's whole
/// cost. Every match it finds is compared word for
/// word, so a collision costs a comparison and never a wrong vote.
fn hash(words: &[u32]) -> Hash {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut lanes = [0xCBF2_9CE4_8422_2325u64; 4];
    let (eights, rest) = words.as_chunks::<8>();
    for eight in eights {
        for (lane, pair) in lanes.iter_mut().zip(eight.as_chunks::<2>().0) {
            let word = u64::from(pair[0]) | u64::from(pair[1]) << 32;
            *lane = (*lane ^ word).wrapping_mul(PRIME);
        }
    }
    let mut h =
        lanes[0] ^ lanes[1].rotate_left(16) ^ lanes[2].rotate_left(32) ^ lanes[3].rotate_left(48);
    for &w in rest {
        h = (h ^ u64::from(w)).wrapping_mul(PRIME);
    }
    Hash(h)
}

/// **The screen the last frame that followed a cursor saw**, and where that
/// cursor was: what the next move is compared with.
pub(crate) struct Carried {
    /// That screen's words, row by row: each cell's content, a cell nothing
    /// was written to read as a space.
    before: Vec<u32>,
    /// The screen now, taken on a frame that read something.
    after: Vec<u32>,
    /// `before`'s geometry, surface and reading; `None` until one is taken.
    shape: Option<Shape>,
    /// The followed cursor at that frame.
    at: Option<(Row, Col)>,
    /// The vote's working rows, kept so a move allocates nothing: each
    /// row's hash before and after, the written rows before by hash, which
    /// of them vote, and the votes a distance.
    hb: Vec<Hash>,
    ha: Vec<Hash>,
    by_hash: Vec<(Hash, Row)>,
    voter: Vec<Voter>,
    tally: Vec<usize>,
    /// The screens taken, for the tests.
    #[cfg(test)]
    taken: crate::trail::Count,
}

/// Whether a row before votes: written, and no other row read the same.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Voter {
    Votes,
    Not,
}

impl Carried {
    pub(crate) fn new() -> Carried {
        Carried {
            before: Vec::new(),
            after: Vec::new(),
            shape: None,
            at: None,
            hb: Vec::new(),
            ha: Vec::new(),
            by_hash: Vec::new(),
            voter: Vec::new(),
            tally: Vec::new(),
            #[cfg(test)]
            taken: crate::trail::Count(0),
        }
    }

    /// Forget the screen and the cursor: the next frame compares nothing.
    pub(crate) fn forget(&mut self) {
        self.shape = None;
        self.at = None;
    }

    /// **The rows the program's scroll took the followed cursor's line**, at
    /// a frame whose followed cursor is `cursor`, after a read that scrolled
    /// the whole screen `ridden` lines: `None` unless the cursor changed
    /// cell and the text in its pane moved with its old line. A frame with
    /// no cursor to follow compares nothing and keeps the screen it had.
    pub(crate) fn frame(
        &mut self,
        term: &Term<'_>,
        cursor: Option<(Row, Col)>,
        ridden: Rows,
    ) -> Option<Carry> {
        if term.window_back().index() != 0 {
            self.forget();
            return None;
        }
        let to = cursor?;
        let (cols, rows) = term.geometry();
        let (read, written) = term.amplification();
        let now = Shape {
            cols,
            rows,
            screen: term.screen(),
            fed: Fed(read, written),
        };
        let fresh = self.shape != Some(now);
        if fresh {
            self.take(term, cols, rows);
        }
        let comparable = self
            .shape
            .is_some_and(|was| was.cols == cols && was.rows == rows && was.screen == now.screen);
        let carry = match self.at {
            Some(from) if from != to && fresh && comparable && ridden.index() == 0 => {
                self.find(from, cols, rows)
            }
            _ => None,
        };
        self.at = Some(to);
        if fresh {
            core::mem::swap(&mut self.before, &mut self.after);
            self.shape = Some(now);
        }
        carry
    }

    /// The screen now into `after`.
    fn take(&mut self, term: &Term<'_>, cols: Cols, rows: Rows) {
        #[cfg(test)]
        {
            self.taken.0 += 1;
        }
        self.after.clear();
        for y in 0..rows.index() {
            match term.row(Row::from_index(y)) {
                Some(cells) => self.after.extend(cells.iter().map(|c| match c.content {
                    Content::EMPTY => BLANK,
                    w => w.bits(),
                })),
                None => self.after.extend(core::iter::repeat_n(BLANK, cols.index())),
            }
        }
    }

    /// The pane round column `col`, from `before` and `after`: the first
    /// column and the one past the last.
    fn pane(&self, col: Col, cols: Cols, rows: Rows) -> (Col, Col) {
        let (w, n) = (cols.index(), rows.index());
        let (b, a) = (&self.before, &self.after);
        let border = |x: usize| {
            let line = b[(n / 2) * w + x];
            BORDERS.contains(&line)
                && (1..n.saturating_sub(1)).all(|y| b[y * w + x] == line && a[y * w + x] == line)
        };
        let lo = (0..col.index())
            .rev()
            .find(|&x| border(x))
            .map_or(0, |x| x + 1);
        let hi = (col.index() + 1..w).find(|&x| border(x)).unwrap_or(w);
        (Col::at_index(lo), Col::at_index(hi))
    }

    /// **The program's scroll under a cursor that went from `from`**, from
    /// `before` to `after`, on a screen `cols` by `rows`.
    fn find(&mut self, (r0, c0): (Row, Col), cols: Cols, rows: Rows) -> Option<Carry> {
        let (w, n) = (cols.index(), rows.index());
        let (r, c) = (r0.index(), c0.index());
        if n == 0 || r >= n || c >= w || self.before.len() != w * n {
            return None;
        }
        let (lo, hi) = self.pane(c0, cols, rows);
        let (lo, hi) = (lo.index(), hi.index());
        let (b, a) = (&self.before, &self.after);
        // Row `y` of the pane, in either screen.
        let span = |y: usize| y * w + lo..y * w + hi;
        let written = |buf: &[u32], y: usize| buf[span(y)].iter().any(|&x| x != BLANK);
        self.hb.clear();
        self.ha.clear();
        self.hb.extend((0..n).map(|y| hash(&b[span(y)])));
        self.ha.extend((0..n).map(|y| hash(&a[span(y)])));
        // The written rows before, by hash; a row votes when no other row
        // reads the same, each tie of hashes compared word for word.
        self.by_hash.clear();
        self.by_hash.extend(
            (0..n)
                .filter(|&y| written(b, y))
                .map(|y| (self.hb[y], Row::from_index(y))),
        );
        self.by_hash.sort_unstable();
        self.voter.clear();
        self.voter.resize(n, Voter::Not);
        let mut i = 0;
        while i < self.by_hash.len() {
            let h = self.by_hash[i].0;
            let end = i + self.by_hash[i..].partition_point(|e| e.0 == h);
            let group = &self.by_hash[i..end];
            for &(_, y) in group {
                let alone = group
                    .iter()
                    .all(|&(_, z)| z == y || b[span(z.index())] != b[span(y.index())]);
                if alone {
                    self.voter[y.index()] = Voter::Votes;
                }
            }
            i = end;
        }
        // The votes, a distance `k - n` rows down from 1 - n to n - 1.
        self.tally.clear();
        self.tally.resize(2 * n, 0);
        for j in 0..n {
            let h = self.ha[j];
            let start = self.by_hash.partition_point(|e| e.0 < h);
            for &(_, y) in self.by_hash[start..].iter().take_while(|e| e.0 == h) {
                let i = y.index();
                if self.voter[i] == Voter::Votes && a[span(j)] == b[span(i)] {
                    self.tally[j + n - i] += 1;
                }
            }
        }
        let stayed = self.tally[n];
        let mut best = (n, 0);
        for s in 1..n {
            for k in [n + s, n - s] {
                if self.tally[k] > best.1 {
                    best = (k, self.tally[k]);
                }
            }
        }
        if best.1 < QUORUM || best.1 <= stayed {
            return None;
        }
        let k = best.0;
        // Whether row `i` reads again at the distance.
        let at = |i: usize| i + k >= n && i + k - n < n && a[span(i + k - n)] == b[span(i)];
        let votes = |i: usize| self.voter[i] == Voter::Votes;
        let went = match votes(r) {
            true => at(r),
            false => {
                let mut held = false;
                for i in (0..r).rev().filter(|&i| votes(i)) {
                    if !at(i) {
                        break;
                    }
                    held = true;
                }
                for i in (r + 1..n).filter(|&i| votes(i)) {
                    if !at(i) {
                        break;
                    }
                    held = true;
                }
                held
            }
        };
        if !went {
            return None;
        }
        // Clamped so the old row stays on the screen.
        let to = (r + k).saturating_sub(n).min(n - 1);
        match to.cmp(&r) {
            core::cmp::Ordering::Greater => Some(Carry::Down(Rows::of_count(to - r))),
            core::cmp::Ordering::Less => Some(Carry::Up(Rows::of_count(r - to))),
            core::cmp::Ordering::Equal => None,
        }
    }
}

/// **A frame's trail, less the program's scroll**, for both drivers: when
/// the cursor `trail_cursor` follows, `cursor`, changed cell and the text
/// moved with its old line, the chase moves what it holds of the text with
/// it ([`Chase::rebase`]) and the ghost's last cell moves too, so the move
/// the frame then draws is what is left. Under `kind` off or `key` stay it
/// takes nothing and keeps no screen. `ridden` is the lines the whole
/// screen scrolled since the last frame (`trail_rides`). Returns the carry,
/// which the driver's ghost in flight moves by.
pub(crate) fn carry<M: Moment>(
    carried: &mut Carried,
    term: &Term<'_>,
    cursor: Option<(Row, Col)>,
    (kind, key): (CursorTrail, TrailScroll),
    ridden: Rows,
    (chase, last): (&mut Chase<M>, &mut Option<(Row, Col)>),
    cell_h: f32,
) -> Option<Carry> {
    if kind == CursorTrail::Off || key == TrailScroll::Stay {
        carried.forget();
        return None;
    }
    let c = carried.frame(term, cursor, ridden)?;
    chase.rebase(c.px(cell_h));
    *last = last.map(|(r, col)| (c.row(r), col));
    Some(c)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 12,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// A line of a numbered file, from 1.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Line(pub(crate) usize);

    /// Lines `first..first + 12` of a numbered file, the cursor on the
    /// first column of line `cursor`, as a redrawing editor paints them.
    pub(crate) fn view(first: Line, cursor: Line) -> Vec<u8> {
        let mut out = b"\x1b[H\x1b[2J".to_vec();
        for y in 0..12 {
            out.extend(format!("\x1b[{};1Hline {:03}", y + 1, first.0 + y).into_bytes());
        }
        out.extend(format!("\x1b[{};1H", cursor.0 - first.0 + 1).into_bytes());
        out
    }

    fn cursor(t: &Term<'_>) -> Option<(Row, Col)> {
        Some(t.cursor())
    }

    /// **A view scrolled three lines that took the cursor with its line is
    /// a carry of three rows; the same view with the cursor sent to another
    /// line is the same carry, and a jump over a still view is none.** The
    /// control: the jump moves the cursor as far as the carry does.
    #[test]
    fn a_scroll_that_takes_the_cursors_line_is_a_carry() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut c = Carried::new();
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, &view(Line(10), Line(13)));
        assert_eq!(c.frame(&t, cursor(&t), Rows::ZERO), None, "first seen");
        // Up the file: the view starts three lines earlier, so `line 013`,
        // where the cursor was, is three rows down, and so is the cursor.
        feed(&mut t, &view(Line(7), Line(13)));
        assert_eq!(
            c.frame(&t, cursor(&t), Rows::ZERO),
            Some(Carry::Down(Rows::of_count(3)))
        );
        // A jump: the view stays, the cursor goes three rows down.
        feed(&mut t, &view(Line(7), Line(16)));
        assert_eq!(c.frame(&t, cursor(&t), Rows::ZERO), None, "a jump");
        // Down the file, the cursor kept on its line: a carry up.
        feed(&mut t, &view(Line(12), Line(16)));
        assert_eq!(
            c.frame(&t, cursor(&t), Rows::ZERO),
            Some(Carry::Up(Rows::of_count(5)))
        );
    }

    /// **Too few rows left to vote, or the cursor's line not carried, is no
    /// carry.** Two written rows that move are under the quorum; a view
    /// that scrolled while the cursor's own line did not go with it -- the
    /// row it would have reached rewritten -- did not take it. The control:
    /// three rows that move are a carry.
    #[test]
    fn a_scroll_short_of_the_quorum_or_without_the_line_is_none() {
        for (lines, want) in [(2, None), (3, Some(Carry::Down(Rows::of_count(1))))] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut c = Carried::new();
            let paint = |top: usize| {
                let mut out = b"\x1b[H\x1b[2J".to_vec();
                for y in 0..lines {
                    out.extend(format!("\x1b[{};1Hrow {y}", top + y).into_bytes());
                }
                out.extend(format!("\x1b[{top};1H").into_bytes());
                out
            };
            feed(&mut t, &paint(2));
            c.frame(&t, cursor(&t), Rows::ZERO);
            feed(&mut t, &paint(3));
            assert_eq!(c.frame(&t, cursor(&t), Rows::ZERO), want, "{lines} rows");
        }
        // The text moves three rows down, and `line 013`, the cursor's, is
        // not where it would have gone: row 7 says something else.
        for (rewrite, want) in [
            (&b"\x1b[7;1H\x1b[2Kelse\x1b[8;1H"[..], None),
            (&b"\x1b[8;1H"[..], Some(Carry::Down(Rows::of_count(3)))),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut c = Carried::new();
            feed(&mut t, &view(Line(10), Line(13)));
            c.frame(&t, cursor(&t), Rows::ZERO);
            feed(&mut t, &view(Line(7), Line(14)));
            feed(&mut t, rewrite);
            assert_eq!(c.frame(&t, cursor(&t), Rows::ZERO), want);
        }
    }

    /// **The pane is the cursor's split**: two panes side by side, the left
    /// scrolled three lines with its cursor and the right still. Read
    /// across the whole row nothing moved, since every row also holds the
    /// right pane's text; read in the left pane, between the screen's edge
    /// and the border, it is a carry. The control: with no border column
    /// the same screens are no carry.
    #[test]
    fn the_vote_reads_the_cursors_pane() {
        for (bar, want) in [
            ("\u{2502}", Some(Carry::Down(Rows::of_count(3)))),
            (" ", None),
        ] {
            let paint = |first: usize, row: usize| {
                let mut out = b"\x1b[H\x1b[2J".to_vec();
                for y in 0..12 {
                    out.extend(
                        format!(
                            "\x1b[{};1Hline {:03}\x1b[{};20H{bar}r{y}",
                            y + 1,
                            first + y,
                            y + 1
                        )
                        .into_bytes(),
                    );
                }
                out.extend(format!("\x1b[{row};1H").into_bytes());
                out
            };
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut c = Carried::new();
            feed(&mut t, &paint(10, 4));
            c.frame(&t, cursor(&t), Rows::ZERO);
            feed(&mut t, &paint(7, 7));
            assert_eq!(c.frame(&t, cursor(&t), Rows::ZERO), want, "{bar:?}");
        }
    }

    /// **A blank line goes with the block round it**: the cursor on an
    /// empty line between written ones, the view scrolled two lines with
    /// it. The control: the same scroll with the written lines either side
    /// of it left where they were is none.
    #[test]
    fn a_blank_line_is_carried_with_its_block() {
        // Lines 10 to 21, every fifth left blank, the two either side of
        // line 15 pinned to screen rows 5 and 7 when `pins` says so.
        let paint = |first: usize, row: usize, pins: Option<(usize, usize)>| {
            let mut out = b"\x1b[H\x1b[2J".to_vec();
            for y in 0..12 {
                let n = first + y;
                if !n.is_multiple_of(5) {
                    out.extend(format!("\x1b[{};1Hline {n:03}", y + 1).into_bytes());
                }
            }
            if let Some((a, b)) = pins {
                out.extend(format!("\x1b[{a};1H\x1b[2Kup\x1b[{b};1H\x1b[2Kdown").into_bytes());
            }
            out.extend(format!("\x1b[{row};1H").into_bytes());
            out
        };
        for (pins, want) in [
            (None, Some(Carry::Down(Rows::of_count(2)))),
            (Some((5, 7)), None),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut c = Carried::new();
            // Line 15 is blank, on row 6.
            feed(&mut t, &paint(10, 6, pins));
            c.frame(&t, cursor(&t), Rows::ZERO);
            feed(&mut t, &paint(8, 8, pins));
            assert_eq!(c.frame(&t, cursor(&t), Rows::ZERO), want, "{pins:?}");
        }
    }

    /// **The carry keeps the cursor's old row on the screen**: the cursor
    /// on a blank line on row 10, the view moved three lines down with the
    /// block above it, is carried to the last row, two down. The control: a
    /// blank line on row 6 is carried all three.
    #[test]
    fn a_carry_stops_at_the_screens_edge() {
        // Lines from `first`, `blank` left out, the cursor on row `row`.
        let paint = |first: usize, blank: usize, row: usize| {
            let mut out = b"\x1b[H\x1b[2J".to_vec();
            for y in 0..12 {
                if first + y != blank {
                    out.extend(format!("\x1b[{};1Hline {:03}", y + 1, first + y).into_bytes());
                }
            }
            out.extend(format!("\x1b[{row};1H").into_bytes());
            out
        };
        for (blank, want) in [(19, 2), (15, 3)] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut c = Carried::new();
            feed(&mut t, &paint(10, blank, blank - 9));
            c.frame(&t, cursor(&t), Rows::ZERO);
            feed(&mut t, &paint(7, blank, 12));
            assert_eq!(
                c.frame(&t, cursor(&t), Rows::ZERO),
                Some(Carry::Down(Rows::of_count(want))),
                "line {blank}"
            );
        }
    }

    /// **A frame takes the screen only when something was read since**:
    /// three frames over one read take it once, and a read takes it again.
    /// A frame the whole screen's scroll rode compares nothing, and a
    /// window looking back forgets the screen.
    #[test]
    fn a_screen_is_taken_once_a_read() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut c = Carried::new();
        feed(&mut t, &view(Line(10), Line(13)));
        for _ in 0..3 {
            c.frame(&t, cursor(&t), Rows::ZERO);
        }
        assert_eq!(c.taken.0, 1);
        feed(&mut t, &view(Line(7), Line(13)));
        assert_eq!(
            c.frame(&t, cursor(&t), Rows::of_count(1)),
            None,
            "ridden: the ride's frame"
        );
        assert_eq!(c.taken.0, 2);
        feed(&mut t, &view(Line(4), Line(13)));
        assert_eq!(
            c.frame(&t, cursor(&t), Rows::ZERO),
            Some(Carry::Down(Rows::of_count(3))),
            "the control: the next frame compares"
        );
        assert_eq!(c.frame(&t, None, Rows::ZERO), None, "no cursor");
        assert_eq!(c.taken.0, 3, "a frame with no cursor takes nothing");
    }

    /// **Off or `stay`, nothing is taken away and no screen kept; on and
    /// `ride`, the carried move snaps.** A quad sees a cursor carried three
    /// rows down with its line: under `ride` the chase is rebased by the
    /// carry, the ghost's last cell moved with it, and the quad takes no
    /// long move; under `stay` -- the control -- it takes the move as it
    /// always did, and keeps no screen. Off keeps none either.
    #[test]
    fn off_or_stay_takes_nothing() {
        use crate::trail::Style;
        use baud_render::Shape as Smear;
        use std::time::{Duration, Instant};
        let rect = |(r, c): (Row, Col)| baud_render::Rect {
            left: c.index() as f32 * 10.0,
            top: r.index() as f32 * 20.0,
            right: c.index() as f32 * 10.0 + 10.0,
            bottom: r.index() as f32 * 20.0 + 20.0,
        };
        for (kind, key, want, longs) in [
            (CursorTrail::Off, TrailScroll::Ride, None, 1),
            (CursorTrail::Quad, TrailScroll::Stay, None, 1),
            (
                CursorTrail::Quad,
                TrailScroll::Ride,
                Some(Carry::Down(Rows::of_count(3))),
                0,
            ),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut c = Carried::new();
            let mut chase: Chase<Instant> = Chase::new(Style::Smear(Smear::Quad));
            let t0 = Instant::now();
            let mut last = None;
            let mut got = None;
            for (k, bytes) in [view(Line(10), Line(13)), view(Line(7), Line(13))]
                .iter()
                .enumerate()
            {
                feed(&mut t, bytes);
                let now = cursor(&t);
                got = carry(
                    &mut c,
                    &t,
                    now,
                    (kind, key),
                    Rows::ZERO,
                    (&mut chase, &mut last),
                    20.0,
                );
                if k == 1 {
                    let moved = Some((Row::from_index(6), Col::at_index(0)));
                    assert_eq!(
                        last == moved,
                        want.is_some(),
                        "{kind:?} {key:?}: the last cell"
                    );
                }
                last = now;
                for n in 0..30u32 {
                    let at = t0 + Duration::from_millis(u64::from(n) * 8 + 500 * k as u64);
                    chase.frame(now.map(rect), at, (10.0, 20.0), baud_render::Points(10.0));
                }
            }
            assert_eq!(got, want, "{kind:?} {key:?}");
            assert_eq!(chase.longs.0, longs, "{kind:?} {key:?}: the long moves");
            assert_eq!(
                c.shape.is_some(),
                want.is_some(),
                "{kind:?} {key:?}: a screen kept"
            );
        }
    }

    /// **Both drivers take the carry, and macOS off the ride's frames
    /// alone**, read in their source because the macOS app's `present`,
    /// `frame_trail` and `ride_trail` are AppKit's and its tests run on
    /// macOS only: `frame_trail` takes the carry before it frames the
    /// chase, `present` hands it the lines the ride took since the last
    /// frame and starts the count again, `ride_trail` counts them, and a
    /// screen switch and both resizes forget the screen; the Linux `plan`
    /// takes it after its ride. The control: each anchor reads its file.
    #[test]
    fn both_drivers_take_the_carry() {
        let app = include_str!("app.rs");
        let timed = include_str!("timed.rs");
        let body = |src: &'static str, sig: &str| {
            let from = src
                .find(sig)
                .unwrap_or_else(|| panic!("the control: {sig}"));
            let rest = &src[from..];
            &rest[..rest.find("\n}\n").map_or(rest.len(), |e| e + 3)]
        };
        let call = concat!("crate::carried::", "carry(");
        let frame = body(app, "fn frame_trail(");
        let (carry, chase) = (frame.find(call), frame.find("chase_frame(chase"));
        assert!(
            carry.zip(chase).is_some_and(|(c, f)| c < f),
            "frame_trail takes the carry before it frames the chase"
        );
        assert!(
            app.contains("let ridden = core::mem::replace(&mut self.ridden, Rows::ZERO);"),
            "present hands frame_trail the ride's lines and starts again"
        );
        let ride = body(app, "fn ride_trail(");
        assert!(
            ride.contains("self.app.ridden = Rows::of_count(self.app.ridden.index() + n.index());"),
            "ride_trail counts the lines it rode"
        );
        assert_eq!(
            app.matches(concat!("self.app.carried", ".forget();"))
                .count(),
            3,
            "the switch, a program's resize and the window's forget the screen"
        );
        let plan = body(timed, "pub(crate) fn plan(");
        let (rode, took) = (plan.find("self.chase.ride("), plan.find(call));
        assert!(
            rode.zip(took).is_some_and(|(r, c)| r < c),
            "plan takes the carry after its ride"
        );
    }
}

/// **Thirteen real sessions, replayed on a 120 Hz clock**, for the
/// drivers' pins: `tests/trail/scroll/`, each program's output with the
/// time of every read and of every key, and what each key was.
#[cfg(test)]
pub(crate) mod captures {
    use crate::effects::trail_cursor;
    use crate::trail::Count;
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen, WinSize};

    /// The frames a second the replay draws, the display link's on this
    /// Mac.
    pub(crate) const FPS: f64 = 120.0;

    /// A stream's reads, each a time and a length, and its keys, each a
    /// time and a label.
    pub(crate) type Schedule<'a> = (&'a [(f64, usize)], &'a [(f64, char)]);

    macro_rules! capture {
        ($name:literal) => {
            (
                $name,
                include_bytes!(concat!("../../tests/trail/scroll/", $name, ".raw")).as_slice(),
                include_str!(concat!("../../tests/trail/scroll/", $name, ".steps")),
            )
        };
    }

    /// Each capture's name, output and step log.
    pub(crate) const ALL: [(&str, &[u8], &str); 13] = [
        capture!("fzf"),
        capture!("helix-code"),
        capture!("helix-num"),
        capture!("helix-rel"),
        capture!("less"),
        capture!("nvim-code"),
        capture!("tmux-copy"),
        capture!("tmux-split"),
        capture!("tmux-vim"),
        capture!("vim-arrows"),
        capture!("vim-code"),
        capture!("vim-num"),
        capture!("vim-rnu"),
    ];

    /// **What every move of the followed cursor did**, a capture a line,
    /// in its order: `T` a long move the trail took, `.` none -- a scroll
    /// taken away, a short move snapped. The keys' labels, `J` a jump, `S`
    /// a scroll, `P` an arrow key, `N` typing and `U` none within a second,
    /// are the step log's. The six `S` moves that still trail are `zb`
    /// with the cursor on the top line, which moves the view so far that
    /// fewer than three written rows of the old screen are left to vote; a
    /// page back in `vim-rnu`, a `J` that kept the cursor on its line, trails
    /// for the same reason. The `P` moves that trail although short are
    /// arrow keys at the screen's edge that pushed the text along, so the
    /// cursor moved three lines through the file.
    pub(crate) const PINNED: [(&str, &str, &str); 13] = [
        ("fzf", "UNN", "T.."),
        (
            "helix-code",
            "UJJSSSSSSSSSSSJJJJJJJJJJSSJ",
            "TTT...........TTTTTTTTTT..T",
        ),
        (
            "helix-num",
            "UJJSSSSSSSSSSSJJJJJJJJJJJJSSJ",
            "TTT...........TTTTTTT..TTT..T",
        ),
        (
            "helix-rel",
            "UJJSSSSSSSSSSSJJJJJJJJJJSSJ",
            "TTT...........TTTTTTTTTT..T",
        ),
        ("less", "UNJJJ", "TT.TT"),
        (
            "nvim-code",
            "JJSSSSSSSSSSSSSJJJJJJJJJJJJJJSSJ",
            "TT..........T..TTTTTTTTT.TTTT..T",
        ),
        ("tmux-copy", "UNJJJJJN", "TTT.T..."),
        (
            "tmux-split",
            "JJSSSSSSSSSSSSSJJJJJJJJJJJJJJSSJ",
            "TT..........T..TTTTTTTTTTTTTT..T",
        ),
        (
            "tmux-vim",
            "JJSSSSSSSSSSSSSJJJJJJJJJJJJJJJSSJ",
            "TT..........T..TTTTTTTTT.TTTTT..T",
        ),
        ("vim-arrows", "JPPPPPPPPPPPPJJ", "TTTTTTTTTTTTTTT"),
        (
            "vim-code",
            "JJSSSSSSSSSSSSSJJJJJJJJJJJJJJJSSJ",
            "TT..........T..TTTTTTTTTTTTTTT..T",
        ),
        (
            "vim-num",
            "JJSSSSSSSSSSSSSJJJJJJJJJJJSSJ",
            "TT..........T..TTTTTT..TTT..T",
        ),
        (
            "vim-rnu",
            "UJJSSSSSSSSSSSSSJJJJJJJJJJJJJJJSSJ",
            "TTT..........T..TTTTTTTTTTTTTTT..T",
        ),
    ];

    /// One move of the followed cursor: the label of the key before it,
    /// whether it was more than two cells on either axis, whether the trail
    /// took a long move for it, and the cells it went between.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Move {
        pub(crate) label: char,
        pub(crate) long: Long,
        pub(crate) trailed: Taken,
        pub(crate) from: (baud_domain::Row, baud_domain::Col),
        pub(crate) to: (baud_domain::Row, baud_domain::Col),
    }

    /// Whether a move was more than two cells.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum Long {
        Long,
        Short,
    }

    /// Whether the trail took a long move for it.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum Taken {
        Trailed,
        Not,
    }

    /// **A capture through a driver**: each read fed at its time, the frame
    /// drawn every 1/[`FPS`] s until 1.5 s after the last read, `frame(term,
    /// secs)` the driver's frame, which says how many long moves its chase
    /// has taken (`drive`).
    pub(crate) fn replay(
        (raw, steps): (&[u8], &str),
        frame: impl FnMut(&Term<'_>, f64) -> Count,
    ) -> Vec<Move> {
        let mut lines = steps.lines();
        let geometry = lines.next().and_then(|h| h.split_whitespace().nth(1));
        let (c, r) = geometry
            .and_then(|g| g.split_once('x'))
            .expect("a geometry");
        let ws = WinSize {
            ws_col: c.parse().unwrap(),
            ws_row: r.parse().unwrap(),
        };
        let (mut reads, mut keys) = (Vec::new(), Vec::new());
        for l in lines {
            let f: Vec<&str> = l.split_whitespace().collect();
            let at: f64 = f[1].parse().unwrap();
            match f[0] {
                "R" => reads.push((at, f[2].parse::<usize>().unwrap())),
                "K" => keys.push((at, f[2].chars().next().unwrap())),
                _ => {}
            }
        }
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        drive(raw, &ws, (&reads, &keys), feed, frame)
    }

    /// **A stream through a driver on a 120 Hz clock**: each of `reads`, a
    /// time and a length, handed to `read` at its time, then `frame(term,
    /// secs)` every 1/[`FPS`] s until 1.5 s after the last. A move is a
    /// frame whose followed cursor is not where the last frame that
    /// followed one had it, labelled by the last of `keys` within a second
    /// before it; a long move the chase took at a frame is the last move's.
    pub(crate) fn drive(
        raw: &[u8],
        ws: &WinSize,
        (reads, keys): Schedule<'_>,
        mut read: impl FnMut(&mut Term<'_>, &[u8]),
        mut frame: impl FnMut(&Term<'_>, f64) -> Count,
    ) -> Vec<Move> {
        let mut owned = Owned::new(ws);
        let mut t = Term::new(owned.storage(), ws);
        let end = reads.last().map_or(0.0, |r| r.0) + 1.5;
        let (mut next, mut off) = (0, 0);
        let (mut anchor, mut longs) = (None, Count(0));
        let mut moves: Vec<Move> = Vec::new();
        for f in 0..(end * FPS) as u64 {
            let secs = f as f64 / FPS;
            while next < reads.len() && reads[next].0 <= secs {
                let n = reads[next].1.min(raw.len() - off);
                read(&mut t, &raw[off..off + n]);
                off += n;
                next += 1;
            }
            if let Some(to) = trail_cursor(&t).map(crate::effects::TrailCursor::cell) {
                if let Some(from) = anchor.filter(|&a| a != to) {
                    let label = keys
                        .iter()
                        .rev()
                        .find(|k| k.0 <= secs)
                        .filter(|k| secs - k.0 <= 1.0)
                        .map_or('U', |k| k.1);
                    let (from_r, from_c): (baud_domain::Row, baud_domain::Col) = from;
                    let cells = |a: usize, b: usize| a.abs_diff(b);
                    let long = match cells(from_r.index(), to.0.index()) > 2
                        || cells(from_c.index(), to.1.index()) > 2
                    {
                        true => Long::Long,
                        false => Long::Short,
                    };
                    moves.push(Move {
                        label,
                        long,
                        trailed: Taken::Not,
                        from,
                        to,
                    });
                }
                anchor = Some(to);
            }
            let now = frame(&t, secs);
            if now != longs {
                if let Some(m) = moves.last_mut() {
                    m.trailed = Taken::Trailed;
                }
                longs = now;
            }
        }
        moves
    }

    /// The labels and the outcomes as [`PINNED`] spells them.
    pub(crate) fn spelled(moves: &[Move]) -> (String, String) {
        let labels = moves.iter().map(|m| m.label).collect();
        let trails = moves
            .iter()
            .map(|m| match m.trailed {
                Taken::Trailed => 'T',
                Taken::Not => '.',
            })
            .collect();
        (labels, trails)
    }

    /// **Every jump trails and every scroll snaps, but for the ones
    /// [`PINNED`] names**: a long `J` move that does not trail, or a long
    /// `S` move that does beyond the pinned count for its capture.
    pub(crate) fn check(name: &str, moves: &[Move], case: &str) {
        let (labels, trails) = spelled(moves);
        let pinned = PINNED.iter().find(|p| p.0 == name).expect("pinned");
        assert_eq!(labels, pinned.1, "{case} {name}: the moves and their keys");
        assert_eq!(trails, pinned.2, "{case} {name}: what each move did");
        for (i, m) in moves.iter().enumerate() {
            if m.label == 'J' && m.long == Long::Long {
                assert_eq!(
                    m.trailed,
                    Taken::Trailed,
                    "{case} {name}: jump {i}, from {:?} to {:?}",
                    m.from,
                    m.to
                );
            }
        }
    }
}
