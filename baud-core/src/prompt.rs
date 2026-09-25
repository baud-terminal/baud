//! The prompt block: the rows the shell will redraw, tracked so that a
//! redraw mark replaces exactly them.
//!
//! zsh keeps one number about the screen -- the rows from its prompt's
//! first line to the cursor -- and on `SIGWINCH` moves up that many,
//! erases to the end of the screen and redraws. That count goes stale
//! whenever a redraw was computed for a width the screen has left, and
//! it clamps at row 0 whenever the block is taller than the window. The
//! terminal knows where the block begins, as a ring row, so at the mark
//! the block is replaced from where it *is*: the cursor is put on its
//! first row, the window is moved back over any of its rows that
//! scrolled off, and every row of it is erased wherever it now sits.
//! Nothing above the block is touched and nothing the terminal made is
//! left in the history. The recordings that each rule answers to are in
//! `docs/research/notes/m-resize-and-reflow.md`; the tests that fail
//! without it are `a_block_never_moves_once_it_is_drawn`,
//! `no_more_than_two_blank_rows_come_between_consecutive_prompts`,
//! `a_gesture_leaves_one_prompt_on_screen` and
//! `a_gesture_on_a_fresh_terminal_creates_no_history`.

use crate::paint::Paint;
use baud_domain::*;

/// Where the block the shell is at begins.
///
/// A ring row, so a scroll or a resize's rotation leaves it naming the
/// same cells; a screen row went stale at the first scroll. Meaningful
/// only while its screen's [`Shell::zone`] is [`crate::term::Zone::Prompt`].
#[derive(Clone, Copy)]
pub(crate) struct PromptBlock {
    start: RowRot,
}

impl PromptBlock {
    pub(crate) const NONE: PromptBlock = PromptBlock {
        start: RowRot::NONE,
    };

    pub(crate) fn start(self) -> RowRot {
        self.start
    }
}

/// What the marks have said about **one screen**: its zone, and where its
/// block begins.
///
/// **One per screen, swapped by `?1049` as the kitty stacks are.** The
/// block start is a row of one surface's ring, so read against the other
/// ring it names rows the shell never drew: a redraw on the alternate
/// screen erased its rows by the primary's block start (a subtraction that
/// underflows whenever that start, a primary ring row, lies past the
/// alternate screen's cursor row plus its ring -- a primary turned three
/// rows with its prompt on the last of twenty is enough), a
/// block opened there was redrawn on the primary against the alternate
/// ring's numbers and took the history with it, and a mark there stopped
/// the *primary* re-cutting, because `Term::resize` asked the zone of
/// whichever screen last sent one. The alternate screen's state starts
/// fresh on every entry: `?1049` clears the screen, and `?47`, which keeps
/// it, keeps text a program drew, not a prompt the shell will count rows
/// from -- a block start left from the last visit is not carried across.
#[derive(Clone, Copy)]
pub(crate) struct Shell {
    pub(crate) zone: crate::term::Zone,
    pub(crate) block: PromptBlock,
}

impl Shell {
    /// A screen no mark has spoken about: output, which is what a stream
    /// with no shell integration is.
    pub(crate) const FRESH: Shell = Shell {
        zone: crate::term::Zone::Output,
        block: PromptBlock::NONE,
    };
}

impl Paint<'_, '_> {
    /// The block begins on the cursor's row: an initial mark.
    pub(crate) fn open_prompt_block(&mut self) {
        let (rot, ring) = (self.view.rotation(), self.view.ring());
        self.cold.shell.block = PromptBlock {
            start: rot.advance(self.cy.index(), ring),
        };
    }

    /// The shell is redrawing the block it has: replace it where it is.
    ///
    /// Refused only when the block's first row is further back than the
    /// history holds -- a stale start is not this block's -- or on the
    /// alternate screen, whose ring keeps no history. Rows of the block
    /// behind the window are taken back by moving the window over them
    /// when the rows it gives up below show nothing (the shell's `ED`
    /// has just cleared them); otherwise they are erased where they lie.
    pub(crate) fn replace_prompt_block(&mut self) {
        let (rot, ring) = (self.view.rotation(), self.view.ring());
        let (cy, r, rows) = (self.cy.index(), ring.index(), self.rows.index());
        let start = self.cold.shell.block.start();
        // Rows from the block's first row to the cursor, in ring order.
        let d = (rot.advance(cy, ring).index() + r - start.index()) % r;
        let above = d.saturating_sub(cy);
        let retained =
            self.cold.screen == crate::term::Screen::Normal && above <= self.book.hist.index();
        if above > 0 && !retained {
            return;
        }
        let gives_up = rows - above.min(rows)..rows;
        if above > 0
            && gives_up
                .into_iter()
                .all(|y| self.view.row_is_blank(Row::from_index(y)))
        {
            self.view.retreat_rot(above);
            let hist = self.book.hist.index() - above;
            self.book.hist = Rows::of_count(hist);
            self.book.hidden =
                Rows::of_count((self.book.hidden.index() + above).min(r - rows - hist));
            // The window's top line moves back with it -- `above` is within
            // the history, and the top line is never below the history's
            // depth, so it cannot run out -- and rows the window gave up past
            // what the ring keeps below take their images with them.
            self.book.top = self
                .book
                .top
                .minus(Rows::of_count(above))
                .unwrap_or(LineIdx::FIRST);
            if self.images.any_placed() {
                self.expire_primary_images(None);
            }
        }
        self.cy = Row::from_index(cy.saturating_sub(d));
        let blank = PackedCell::empty_with(self.style);
        for k in 0..=d {
            let n = self.view.fill_at(start.advance(k, ring), blank);
            self.cells = self.cells.plus(n.index());
        }
        self.prompt_erases += 1;
    }
}
