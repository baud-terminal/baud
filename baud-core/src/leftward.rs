//! **DECRLM, `CSI ? 34 h` / `CSI ? 34 l`** (`[[source: vt510-decrlm]]`):
//! "This control function starts/stops the right-to-left cursor
//! direction."
//!
//! **A character is written at the cursor and the cursor moves left**:
//! "The next character position is defined as the current cursor position
//! minus 1", and "DECRLM does not affect column numbering within the line",
//! so the cells hold the characters in the order shown, the first sent at
//! the right. A wide character takes the cursor's column and the one to its
//! left, its lead on the left as everywhere, and the cursor moves two. Under
//! IRM "All characters from the current cursor position to the left margin
//! inclusively are shifted one position towards the left. The character
//! previously displayed at the left margin disappears." A character written
//! at the left margin leaves a wrap pending there
//! ([`crate::term::Pinned::LeftEdge`]); with DECAWM set "a graphic character
//! received when the cursor is at column number one appears at the right
//! margin column of the next line", scrolling at the region's end, and with
//! it reset it overwrites the margin. `BS` moves right, `CR` to the right
//! margin, `HT` to the previous tab stop (Jev, LEFTWARD, 0.92/0.94), and the
//! margins are DECLRMM's (Jev's lean, MARGINS, the orders apart, by
//! mean probability 0.39 against 0.37); "ICH, DCH, ED, EL, DECSED, DECSEL, ECH ... CUP, HVP, CUF, CUB" are
//! not affected. A combining mark joins the character the last write put
//! down, the cell right of the cursor.
//!
//! **Each row written so is drawn as its cells hold** (Jev, AS_STORED,
//! 0.97/0.97): its [`RowWritten`] mark keeps UAX #9 off it, so a Hebrew word
//! written under DECRLM reads as the VT510 shows it, until the row is
//! erased whole -- by ED, EL, ECH or a fill from its first column to its
//! end, a scroll or IL and DL bringing it in, or DECSED and DECSEL leaving
//! no protected character on it -- when the mark goes and its path stays.
//!
//! **What it costs while reset: nothing on the text path.** While set the
//! core chooses the one-by-one graph, and SAPV's writer, which the text
//! path's writers are not, sends each character here
//! ([`crate::paint::Writer`]).

use baud_domain::{Codepoint, Col, Count, CursorCol, ModeBit, PackedCell, Row, RowWritten, Width};

use crate::paint::{Paint, Pen};
use crate::term::Pinned;

impl Paint<'_, '_> {
    /// **The column a write under DECRLM stops at**: the left margin for a
    /// cursor at or right of it, the screen's first column left of it.
    fn left_limit(&self, c: Col) -> Col {
        match self.cold.modes.is_set(ModeBit::SideMargins) && c >= self.cold.left {
            true => self.cold.left,
            false => Col::ZERO,
        }
    }

    /// **The wrap under DECRLM**: the row continues onto the next, which
    /// the cursor enters at its right margin, the region scrolling at its
    /// end; autoprint and a selected character path as [`Paint::wrap`]
    /// takes them.
    fn wrap_leftward(&mut self) {
        if self.cold.modes.is_set(ModeBit::AutoPrint) {
            self.autoprint(crate::print::LineEnd::LineFeed);
        }
        self.view.set_wrapped(self.cy);
        self.index();
        if self.cold.modes.is_set(ModeBit::PathSelected) {
            self.wrapped_into_path();
        }
        self.refresh_fence();
        self.cx = CursorCol::moved(self.line_fence());
    }

    /// **One graphic character under DECRLM.**
    #[cold]
    #[inline(never)]
    pub(crate) fn glyph_leftward(&mut self, cp: Codepoint, w: Width, pen: Pen) {
        let need = 1 + usize::from(w == Width::WIDE);
        let wraps = self.cold.modes.is_set(ModeBit::AutoWrap);
        if self.at_left_edge() {
            self.cold.pinned = Pinned::Before;
            if wraps {
                self.wrap_leftward();
            }
        }
        // A cursor past the line's end writes at its last column.
        self.refresh_fence();
        let fence = self.line_fence();
        let mut c = self.cx.col().min(fence);
        let mut left = self.left_limit(c);
        // A wide character needs a column left of the cursor.
        if need == 2 && c.index() < left.index() + 1 {
            match wraps {
                true => {
                    self.wrap_leftward();
                    c = self.cx.col();
                    left = self.left_limit(c);
                }
                false => c = Col::at_index(left.index() + 1),
            }
        }
        if need == 2 && c.index() < left.index() + 1 {
            return;
        }
        let lead_col = Col::at_index(c.index() + 1 - need);
        let style = self.style;
        let flags = match pen {
            Pen::Gl => self.flags,
            Pen::Decoded => self.decoded_pen(),
            Pen::Kept(set) => self.flags.with_received(set),
        };
        let blank = PackedCell::empty_with(style);
        if self.inserting() {
            let after = Col::at_index(c.index() + 1);
            let b = self.blank();
            self.view
                .delete_cells(self.cy, left, Count::of(need), after, b);
            self.erase_images(self.cy, left, after);
        }
        let tail = match w == Width::WIDE {
            true => Some(PackedCell::wide_tail(style, flags)),
            false => None,
        };
        let lead = PackedCell::glyph(cp, style, flags);
        self.last_cp = Some((cp, w, pen));
        self.view.put_glyph(self.cy, lead_col, lead, tail, blank);
        let row = self.view.bidi(self.cy);
        if row.written() != RowWritten::RightToLeft {
            self.view
                .set_bidi(self.cy, row.with_written(RowWritten::RightToLeft));
        }
        self.cells = self.cells.plus(need);
        let end = Col::at_index(c.index() + 1);
        self.overwrote(lead_col, end);
        self.row_written = self.row_written.max(end);
        match lead_col > left {
            // The last write is right of the cursor now, so no pin a write
            // with DECAWM reset left before DECRLM names it.
            true => {
                self.cx = CursorCol::moved(lead_col.before());
                if self.cold.pinned != Pinned::Before {
                    self.cold.pinned = Pinned::Before;
                }
            }
            false => {
                self.cx = CursorCol::moved(left);
                self.cold.pinned = Pinned::LeftEdge;
                self.cold.left_edge = self.cy;
            }
        }
    }

    /// **A selective erase over row `r`'s whole line ends DECRLM's mark
    /// when it left no character standing**: a protected one DECSCA kept
    /// stands where it was written, and the row is not erased whole; a
    /// row of spaces is (Jev, AS_STORED: drawn as its cells hold "until it
    /// is erased whole"). ED and EL's fills end it in the grid
    /// ([`baud_domain::RowExtent::erased`]).
    pub(crate) fn selectively_erased_whole(&mut self, r: Row, from: Col, to: Col) {
        let line = self.view.bidi(r);
        if line.written() != RowWritten::RightToLeft || from != Col::ZERO {
            return;
        }
        let end = self.view.rendition(r).end_of_line(self.cols).index();
        if to.index() < end {
            return;
        }
        let stands = (0..end).any(|x| {
            let c = self.view.get(r, Col::at_index(x));
            c.is_protected() && !c.content.is_empty()
        });
        if !stands {
            self.view
                .set_bidi(r, line.with_written(RowWritten::InOrder));
        }
    }

    /// Whether a write under DECRLM left a wrap waiting where the cursor is.
    pub(crate) fn at_left_edge(&self) -> bool {
        self.cold.pinned == Pinned::LeftEdge && self.cold.left_edge == self.cy
    }

    /// **`BS` under DECRLM**: one column right, "thus moving the cursor on
    /// the screen one position to the right", stopping at the right margin
    /// or the line's end.
    #[cold]
    #[inline(never)]
    pub(crate) fn back_space_leftward(&mut self) {
        let c = self.cx.col();
        let fence = self.line_fence();
        let to = match c < fence {
            true => c.after(),
            false => c.min(fence),
        };
        self.set_col(CursorCol::moved(to));
    }

    /// **`HT` under DECRLM**: to the previous tab stop, the direction the
    /// cursor writes in.
    #[cold]
    #[inline(never)]
    pub(crate) fn tab_leftward(&mut self) {
        self.tab_back(Count::ONE);
    }

    /// **`CR` under DECRLM**: "the cursor moves to the last right-most
    /// column of the current line", the right margin inside DECLRMM's
    /// margins as the left margin is `CR`'s without it.
    pub(crate) fn return_leftward(&mut self) {
        let x = self.cx.col().index();
        let y = self.cy.index();
        let inside = self.cold.modes.is_set(ModeBit::SideMargins)
            && (self.cold.modes.is_set(ModeBit::Origin)
                || (x <= self.cold.right.index()
                    && y >= self.cold.top.index()
                    && y <= self.cold.bot.index()));
        let to = match inside {
            true => self.cold.right,
            false => self.line_fence(),
        };
        self.set_col(CursorCol::moved(to));
    }

    /// **The column after the character the last write put down, under
    /// DECRLM**: the cell right of the cursor, or the margin's own when a
    /// wrap waits there.
    pub(crate) fn after_leftward_write(&self) -> Col {
        let c = self.cx.col();
        match self.at_left_edge() {
            true => c.after(),
            false => Col::at_index(c.index() + 2),
        }
    }
}
