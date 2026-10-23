//! The prompts a shell drew on the primary screen, each where it began and
//! how its command ended: what prompt navigation and an embedder read.
//!
//! **Kept by the line, not the row.** An entry is the absolute line its
//! prompt's first row is on ([`LineIdx`]), so it holds through every scroll
//! with no write, reads as gone once the ring recycles the line (a floor the
//! caller applies), and is renumbered by a reflow through
//! [`PromptLog::follow_begin`]'s three steps, the way image placements are.
//!
//! **Bounded and caller-owned**, a slice the embedder lends. Past it the
//! oldest entry is overwritten and counted ([`PromptLog::forgotten`]): the
//! oldest prompts are the first lines the ring recycles anyway, and a
//! refusal would lose the newest, which are the ones navigation reaches
//! first. baud-core's `Owned` sizes it one entry a ring row, the
//! most a ring can hold, so it never fills.
//!
//! **The commands are a stack, by `aid`.** "A command sequence may be
//! nested withing the output section of another command ... To help track
//! this nesting we make use of an optional application identifier (aid)"
//! (`[[source: semantic-prompts]]`). A `D` ends the innermost open command
//! with its `aid` and every one nested inside it, and its status goes on
//! that command's prompt; an `N` does the same before it opens its own.

use crate::*;

/// Which prompt of the log an entry or an open command is: a count of
/// prompts kept since the log began, so an entry overwritten or dropped is
/// told from one that is still there.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PromptSeq(u64);

/// What an entry of the log is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    /// A shell's prompt, which a command's `D` reports on.
    Prompt,
    /// A line a program marked to jump back to, iTerm2's `SetMark`
    /// (`[[source: iterm2-escape-codes]]`): no command, no status.
    Mark,
}

/// One prompt, or one mark: the line its first row is on and how its
/// command ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromptEntry {
    line: LineIdx,
    /// A prompt's own number. **A mark takes the number of the entry
    /// before it** (0 at the front), so the numbers stay in order and a
    /// prompt is the first entry with its number, which `find` bisects to.
    seq: PromptSeq,
    kind: EntryKind,
    status: ExitStatus,
    /// Where a reflow in progress put its line, rows from the oldest
    /// retained one: kept beside the line, which the walk bisects on and so
    /// must not move until the walk ends.
    landed: Landed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Landed {
    Unvisited,
    At(Rows),
}

impl PromptEntry {
    pub const EMPTY: PromptEntry = PromptEntry {
        line: LineIdx::FIRST,
        seq: PromptSeq(0),
        kind: EntryKind::Prompt,
        status: ExitStatus::Unreported,
        landed: Landed::Unvisited,
    };
    pub fn line(self) -> LineIdx {
        self.line
    }
    pub fn status(self) -> ExitStatus {
        self.status
    }
    pub fn kind(self) -> EntryKind {
        self.kind
    }
}

/// An open command: the `aid` that opened it and the prompt it began with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Open {
    aid: Aid,
    prompt: PromptSeq,
    /// Whether a `C` began its output: a `D` with none is iTerm2's abort.
    ran: Ran,
}

/// How deep commands nest before the outermost is let go: a shell in a
/// shell in a REPL is three.
pub const NOPEN: usize = 16;

/// The log; see the module.
pub struct PromptLog<'b> {
    slots: &'b mut [PromptEntry],
    /// The physical slot of the oldest entry, and how many there are, in
    /// line order from it.
    head: usize,
    len: usize,
    next_seq: PromptSeq,
    open: [Open; NOPEN],
    depth: usize,
    forgotten: PromptsForgotten,
}

/// How many prompts a log holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromptCapacity(usize);

impl PromptCapacity {
    pub fn of(n: usize) -> PromptCapacity {
        PromptCapacity(n)
    }
    pub fn index(self) -> usize {
        self.0
    }
}

/// Entries overwritten because the log was full.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromptsForgotten(usize);

impl PromptsForgotten {
    pub fn index(self) -> usize {
        self.0
    }
}

impl<'b> PromptLog<'b> {
    pub fn new(slots: &'b mut [PromptEntry]) -> PromptLog<'b> {
        PromptLog {
            slots,
            head: 0,
            len: 0,
            next_seq: PromptSeq(1),
            open: [Open {
                aid: Aid::EMPTY,
                prompt: PromptSeq(0),
                ran: Ran::NoOutput,
            }; NOPEN],
            depth: 0,
            forgotten: PromptsForgotten(0),
        }
    }

    pub fn forgotten(&self) -> PromptsForgotten {
        self.forgotten
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn at(&self, i: usize) -> PromptEntry {
        self.slots[(self.head + i) % self.slots.len()]
    }

    fn at_mut(&mut self, i: usize) -> &mut PromptEntry {
        let n = self.slots.len();
        &mut self.slots[(self.head + i) % n]
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> impl Iterator<Item = PromptEntry> + '_ {
        (0..self.len).map(move |i| self.at(i))
    }

    /// The entries on `line` and after it, oldest first, the first found by
    /// bisection: what a reader of a window of lines walks, stopping past
    /// the window's last, with no walk of the lines above it.
    pub fn entries_from(&self, line: LineIdx) -> impl Iterator<Item = PromptEntry> + '_ {
        (self.first_from(line)..self.len).map(move |i| self.at(i))
    }

    /// A new command's prompt, its first row on `line`, opened by `aid`.
    /// An `N` first finishes the innermost open command with the same
    /// `aid` and every one nested in it; an `A` nests in whatever is open.
    pub fn open(&mut self, line: LineIdx, aid: Aid, opener: Opener) {
        if opener == Opener::NewCommand {
            if let Some(d) = self.innermost(aid) {
                self.depth = d;
            }
        }
        let seq = self.push(line);
        if self.depth == NOPEN {
            self.open.copy_within(1.., 0);
            self.depth -= 1;
        }
        self.open[self.depth] = Open {
            aid,
            prompt: seq,
            ran: Ran::NoOutput,
        };
        self.depth += 1;
    }

    /// A prompt that opens no command: a `P` with none open, as the owner's
    /// `zsh` sends. It is a command of the empty `aid` all the same, so its
    /// `D` has somewhere to go.
    pub fn open_bare(&mut self, line: LineIdx) {
        self.open(line, Aid::EMPTY, Opener::FreshLineNewPrompt);
    }

    fn push(&mut self, line: LineIdx) -> PromptSeq {
        self.erased_from(line);
        let seq = self.next_seq;
        self.next_seq = PromptSeq(seq.0 + 1);
        if self.slots.is_empty() {
            self.forgotten.0 += 1;
            return seq;
        }
        if self.len == self.slots.len() {
            self.head = (self.head + 1) % self.slots.len();
            self.len -= 1;
            self.forgotten.0 += 1;
        }
        let entry = PromptEntry {
            line,
            seq,
            kind: EntryKind::Prompt,
            status: ExitStatus::Unreported,
            landed: Landed::Unvisited,
        };
        *self.at_mut(self.len) = entry;
        self.len += 1;
        seq
    }

    /// The shell redrew the newest prompt with its first row on `line`: a
    /// redraw mark, after which the block may begin on an earlier line.
    ///
    /// The newest *prompt*: a mark after it is on a row of its block, which
    /// the redraw erases, and goes with the rows.
    pub fn redrawn(&mut self, line: LineIdx) {
        let Some(k) = (0..self.len)
            .rev()
            .find(|&i| self.at(i).kind == EntryKind::Prompt)
        else {
            return;
        };
        let newest = self.at(k);
        self.len = k;
        self.erased_from(line);
        *self.at_mut(self.len) = PromptEntry { line, ..newest };
        self.len += 1;
    }

    /// **A mark on `line`**, iTerm2's `SetMark`, put in line order among the
    /// prompts, erasing none: the cursor can be above the newest prompt. A
    /// line that already has an entry is already one a jump stops at, and
    /// is left as it is. Past the slice the oldest entry is forgotten, as a
    /// prompt's push forgets it.
    pub fn mark(&mut self, line: LineIdx) {
        let at = self.first_from(line);
        if at < self.len && self.at(at).line == line {
            return;
        }
        if self.slots.is_empty() {
            self.forgotten.0 += 1;
            return;
        }
        let mut at = at;
        if self.len == self.slots.len() {
            self.forgotten.0 += 1;
            if at == 0 {
                // The mark is older than every entry kept: it is the one
                // forgotten.
                return;
            }
            self.head = (self.head + 1) % self.slots.len();
            self.len -= 1;
            at -= 1;
        }
        let seq = match at {
            0 => PromptSeq(0),
            _ => self.at(at - 1).seq,
        };
        let mut i = self.len;
        while i > at {
            let e = self.at(i - 1);
            *self.at_mut(i) = e;
            i -= 1;
        }
        *self.at_mut(at) = PromptEntry {
            line,
            seq,
            kind: EntryKind::Mark,
            status: ExitStatus::Unreported,
            landed: Landed::Unvisited,
        };
        self.len += 1;
    }

    /// **A prompt on `line` erased every prompt from it on**: they were on
    /// rows a clear or the shell's own erase took (`CSI H CSI 2 J`, zsh's
    /// Control-L), or that the prompt now covers. Keeps the entries in line
    /// order, which every bisection here reads.
    fn erased_from(&mut self, line: LineIdx) {
        self.len = self.first_from(line);
    }

    /// `C`: the current command's output began.
    pub fn output_began(&mut self) {
        if let Some(o) = self.open[..self.depth].last_mut() {
            o.ran = Ran::Output;
        }
    }

    /// `D`: the innermost open command with its `aid` ends, with the status
    /// `done` gives it, and every command nested in it ends unreported. "End
    /// of current command": with no open command of that `aid`, the
    /// innermost one.
    pub fn close(&mut self, done: Done) {
        if self.depth == 0 {
            return;
        }
        let d = self.innermost(done.aid).unwrap_or(self.depth - 1);
        let (seq, status) = (self.open[d].prompt, done.status(self.open[d].ran));
        self.depth = d;
        if let Some(i) = self.find(seq) {
            self.at_mut(i).status = status;
        }
    }

    /// The depth of the innermost open command with `aid`.
    fn innermost(&self, aid: Aid) -> Option<usize> {
        self.open[..self.depth].iter().rposition(|o| o.aid == aid)
    }

    /// The entry of prompt `seq`: the entries are in `seq` order too, and
    /// the prompt is the first with its number, the marks after it sharing
    /// it.
    fn find(&self, seq: PromptSeq) -> Option<usize> {
        let (mut lo, mut hi) = (0, self.len);
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.at(mid).seq < seq {
                true => lo = mid + 1,
                false => hi = mid,
            }
        }
        (lo < self.len && self.at(lo).seq == seq && self.at(lo).kind == EntryKind::Prompt)
            .then_some(lo)
    }

    /// The first entry whose line is at or after `line`, by bisection: the
    /// entries are in line order.
    fn first_from(&self, line: LineIdx) -> usize {
        let (mut lo, mut hi) = (0, self.len);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.at(mid).line < line {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// The newest prompt on a line before `line` and at or after `floor`.
    pub fn before(&self, line: LineIdx, floor: LineIdx) -> Option<PromptEntry> {
        let i = self.first_from(line);
        let e = self.at(i.checked_sub(1)?);
        (e.line >= floor).then_some(e)
    }

    /// The oldest prompt on a line after `line`.
    pub fn after(&self, line: LineIdx) -> Option<PromptEntry> {
        let i = self.first_from(line.next());
        (i < self.len).then(|| self.at(i))
    }

    /// Forget every prompt on a line before `line`: `CSI 3 J` erased them.
    pub fn forget_before(&mut self, line: LineIdx) {
        let n = self.first_from(line);
        if !self.slots.is_empty() {
            self.head = (self.head + n) % self.slots.len();
        }
        self.len -= n;
    }

    /// Forget everything: `RIS`.
    pub fn clear(&mut self) {
        self.len = 0;
        self.head = 0;
        self.depth = 0;
    }

    /// **Follow the prompts through a reflow**, in three steps around it, as
    /// image placements follow it: this drops the entries the ring has
    /// already recycled -- before `floor`, the oldest retained line -- and
    /// returns how many are left to map; [`PromptLog::follow_line`] maps
    /// each line's entries as the reflow visits it, in whichever order it
    /// visits them; [`PromptLog::follow_end`] renumbers them and drops the
    /// ones no line took. Each entry keeps its old line until the end, so a
    /// line's entries are found by bisection however many lines have been
    /// mapped, and each is mapped once though the old and new layouts share
    /// one numbering.
    pub fn follow_begin(&mut self, floor: LineIdx) -> usize {
        self.forget_before(floor);
        for i in 0..self.len {
            self.at_mut(i).landed = Landed::Unvisited;
        }
        self.len
    }

    /// Map the entries on one line of a reflow: its first `rows` rows from
    /// `from`, counted from `floor`, and `land`, where a cell of it lands.
    pub fn follow_line(
        &mut self,
        floor: LineIdx,
        from: Rows,
        rows: Rows,
        land: &dyn Fn(Rows, Col) -> (Rows, Col),
    ) {
        let lo = self.first_from(floor.plus(from));
        let hi = self.first_from(floor.plus(Rows::of_count(from.index() + rows.index())));
        for i in lo..hi {
            let r = self.at(i).line.since(floor).unwrap_or(Rows::ZERO);
            self.at_mut(i).landed = Landed::At(land(r, Col::ZERO).0);
        }
    }

    /// Renumber each mapped entry to where its line landed, from the same
    /// `floor`, and drop the ones no line took: the reflow dropped their
    /// lines.
    pub fn follow_end(&mut self, floor: LineIdx) {
        let mut kept = 0;
        for i in 0..self.len {
            let e = self.at(i);
            if let Landed::At(r) = e.landed {
                *self.at_mut(kept) = PromptEntry {
                    line: floor.plus(r),
                    landed: Landed::Unvisited,
                    ..e
                };
                kept += 1;
            }
        }
        self.len = kept;
    }

    /// Leave a follow the reflow never ran: no entry moved.
    pub fn follow_cancel(&mut self) {
        for i in 0..self.len {
            self.at_mut(i).landed = Landed::Unvisited;
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn line(n: u64) -> LineIdx {
        LineIdx::of(n)
    }

    fn lines(log: &PromptLog<'_>) -> Vec<u64> {
        log.entries().map(|e| e.line().value()).collect()
    }

    /// **A `D` ends the innermost command with its `aid`**, and its status
    /// lands on that command's prompt, past a nested one.
    #[test]
    fn a_status_goes_to_the_command_of_its_aid() {
        let mut slots = [PromptEntry::EMPTY; 8];
        let mut log = PromptLog::new(&mut slots);
        let zsh = Aid::EMPTY;
        let bash = Aid::of(b"4242");
        log.open(line(0), zsh, Opener::FreshLineNewPrompt);
        log.output_began();
        log.open(line(3), bash, Opener::FreshLineNewPrompt);
        log.open(line(5), bash, Opener::FreshLineNewPrompt);
        let done = |code, aid| Done {
            code: Some(ExitCode::of(code)),
            err: None,
            aid,
        };
        log.output_began();
        log.close(done(1, bash));
        log.close(done(0, zsh));
        let s: Vec<_> = log.entries().map(|e| e.status()).collect();
        assert_eq!(
            s,
            [
                ExitStatus::Exited(ExitCode::of(0)),
                ExitStatus::Unreported,
                ExitStatus::Exited(ExitCode::of(1)),
            ]
        );
    }

    /// **The entries from a line on**: the first at or after it, whatever
    /// the slot the ring has it in, and none past the newest.
    #[test]
    fn entries_from_a_line_start_at_its_first_prompt() {
        let mut slots = [PromptEntry::EMPTY; 4];
        let mut log = PromptLog::new(&mut slots);
        for n in [2, 5, 9, 12, 20] {
            log.open_bare(line(n));
        }
        let from = |n| -> Vec<u64> {
            log.entries_from(line(n))
                .map(|e| e.line().value())
                .collect()
        };
        assert_eq!(lines(&log), [5, 9, 12, 20], "the oldest let go");
        assert_eq!(from(0), [5, 9, 12, 20]);
        assert_eq!(from(9), [9, 12, 20]);
        assert_eq!(from(10), [12, 20]);
        assert!(from(21).is_empty());
    }

    /// An `N` finishes the innermost command of its `aid` and everything in
    /// it before it opens its own; an `A` nests.
    #[test]
    fn n_finishes_the_command_of_its_aid() {
        let mut slots = [PromptEntry::EMPTY; 8];
        let mut log = PromptLog::new(&mut slots);
        let a = Aid::of(b"a");
        log.open(line(0), a, Opener::NewCommand);
        log.open(line(1), Aid::of(b"b"), Opener::FreshLineNewPrompt);
        log.open(line(2), a, Opener::NewCommand);
        assert_eq!(log.depth, 1, "the N closed a and the b nested in it");
        log.close(Done {
            code: Some(ExitCode::of(0)),
            err: None,
            aid: Aid::of(b"zzz"),
        });
        assert_eq!(log.at(2).status(), ExitStatus::Cancelled, "a D with no C");
        assert_eq!(log.depth, 0);
    }

    /// **A mark goes in line order and erases nothing**: above the newest
    /// prompt as below it, a jump stops at it, a `D` still reports on its
    /// command's prompt past a mark with the prompt's number, a line already
    /// in the log keeps its entry, and a redraw takes the marks on the rows
    /// it erases.
    #[test]
    fn a_mark_goes_in_line_order_and_reports_nothing() {
        let mut slots = [PromptEntry::EMPTY; 8];
        let mut log = PromptLog::new(&mut slots);
        log.open_bare(line(2));
        log.output_began();
        log.mark(line(4));
        log.open_bare(line(10));
        log.mark(line(7));
        log.mark(line(10));
        assert_eq!(lines(&log), [2, 4, 7, 10]);
        let kinds: Vec<_> = log.entries().map(|e| e.kind()).collect();
        assert_eq!(
            kinds,
            [
                EntryKind::Prompt,
                EntryKind::Mark,
                EntryKind::Mark,
                EntryKind::Prompt
            ]
        );
        assert_eq!(
            log.before(line(10), line(0)).map(|e| e.line()),
            Some(line(7))
        );
        assert_eq!(log.after(line(4)).map(|e| e.line()), Some(line(7)));
        log.output_began();
        log.close(Done {
            code: Some(ExitCode::of(3)),
            err: None,
            aid: Aid::EMPTY,
        });
        assert_eq!(log.at(3).status(), ExitStatus::Exited(ExitCode::of(3)));
        assert_eq!(log.at(2).status(), ExitStatus::Unreported, "the mark");
        log.mark(line(12));
        log.redrawn(line(10));
        assert_eq!(
            lines(&log),
            [2, 4, 7, 10],
            "the redraw erased the mark at 12"
        );
        assert_eq!(log.at(3).kind(), EntryKind::Prompt);
        log.mark(line(0));
        assert_eq!(lines(&log), [0, 2, 4, 7, 10]);
    }

    /// **A full log forgets its oldest entry for a mark too**, and a mark
    /// older than every entry kept is the one forgotten.
    #[test]
    fn a_mark_in_a_full_log_forgets_the_oldest() {
        let mut slots = [PromptEntry::EMPTY; 3];
        let mut log = PromptLog::new(&mut slots);
        for n in [2, 5, 9] {
            log.open_bare(line(n));
        }
        log.mark(line(7));
        assert_eq!(lines(&log), [5, 7, 9]);
        log.mark(line(1));
        assert_eq!(lines(&log), [5, 7, 9]);
        assert_eq!(log.forgotten().index(), 2);
    }

    /// A prompt drawn above prompts the screen held -- after a clear --
    /// erases them, so the log stays in line order.
    #[test]
    fn a_prompt_above_others_erases_them() {
        let mut slots = [PromptEntry::EMPTY; 8];
        let mut log = PromptLog::new(&mut slots);
        for n in [10, 12, 15] {
            log.open_bare(line(n));
        }
        log.open_bare(line(10));
        assert_eq!(lines(&log), [10]);
        log.open_bare(line(14));
        log.redrawn(line(9));
        assert_eq!(lines(&log), [9]);
    }

    /// Full: the oldest goes, counted.
    #[test]
    fn a_full_log_forgets_its_oldest() {
        let mut slots = [PromptEntry::EMPTY; 3];
        let mut log = PromptLog::new(&mut slots);
        for n in 0..5 {
            log.open_bare(line(n * 10));
        }
        assert_eq!(lines(&log), [20, 30, 40]);
        assert_eq!(log.forgotten().index(), 2);
        assert_eq!(
            log.before(line(35), line(0)).map(|e| e.line().value()),
            Some(30)
        );
        assert_eq!(log.before(line(20), line(0)), None);
        assert_eq!(log.before(line(35), line(31)), None, "below the floor");
        assert_eq!(log.after(line(30)).map(|e| e.line().value()), Some(40));
        assert_eq!(log.after(line(40)), None);
        log.forget_before(line(31));
        assert_eq!(lines(&log), [40]);
    }

    /// A reflow renumbers each entry by where its line's first cell lands,
    /// and drops the entries on lines it drops.
    #[test]
    fn a_reflow_moves_each_prompt_with_its_line() {
        let mut slots = [PromptEntry::EMPTY; 4];
        let mut log = PromptLog::new(&mut slots);
        for n in [2, 3, 6, 9] {
            log.open_bare(line(100 + n));
        }
        let floor = line(100);
        assert_eq!(log.follow_begin(floor), 4);
        // Lines of two rows each from row 2 join into one row: rows 2-3
        // land on 1, 4-5 on 2, and so on; row 9 is never visited.
        let land = |r: Rows, c: Col| (Rows::of_count(r.index() / 2), c);
        // A narrowing reflow visits its lines last first.
        for from in [6usize, 4, 2] {
            log.follow_line(floor, Rows::of_count(from), Rows::of_count(2), &land);
        }
        log.follow_end(floor);
        assert_eq!(lines(&log), [101, 101, 103]);
    }
}
