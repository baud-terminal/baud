//! Where a program put the cursor before it hid it.
//!
//! **Helix draws every block cursor itself and hides the terminal's** ("all
//! block cursors are drawn manually", [[source: helix-editor]]), but it still
//! moves the terminal cursor to the primary selection each frame and then
//! hides it ([[source: helix-tui-terminal]], `draw`: `set_cursor` then
//! `hide_cursor`). So every one of its frames ends `CUP` then `?25l`, and an
//! embedder's cursor trail, following only a shown cursor, draws nothing on
//! `gg` or `ge`. kitty follows the position and fades its trail out while the
//! cursor is hidden ([[source: kitty-cursor-trail]]), so it draws none there
//! either.
//!
//! Following every hidden cursor is not the answer: a program draws with the
//! cursor hidden, and on 4096-byte reads the hidden cursor makes a long move
//! on 127 of `btop.raw`'s 128 reads. What separates a placed cursor from a
//! drawing one is the bytes: **a hide is a placement when it is exactly
//! `ESC [ ? 2 5 l` and the bytes straight before it are a complete `CUP` or
//! `HVP`** -- digits and semicolons between `ESC [` and `H` or `f`. The
//! placement is where the cursor is, which is where the move put it: nothing
//! between two adjacent sequences can move it. That is every one of helix's
//! hides in both committed captures and none of the corpus's twenty
//! (`baud-core/tests/placed_cursor.rs`).
//!
//! **Bytes that spell a `CUP` are dispatched as one.** They were not while
//! baud's `CSI` nodes consumed an `ESC` and stayed in the sequence, where
//! `[[source: williams]]`'s anywhere rule abandons it: `CSI ESC [ 3 ; 7 H`
//! dispatched `CSI [` and printed `3;7H`, and a hide after it placed the
//! cursor where the text left it. With the rule in, those bytes are a move
//! (`a_move_whose_esc_arrived_inside_a_csi_is_a_move`), and a cursor off the
//! grid -- the pending-wrap column no `CUP` leaves -- is reachable from no
//! byte stream. It is still refused, and the unit test below holds the
//! refusal, since no test of bytes can.
//!
//! **It is decided at the hide and nowhere else.** Two designs came before
//! it. The first stored the move's position on every `CUP` and compared it at
//! the hide, which placed vim's start-up `CSI H`, `CSI 2J`, `?25l`. The second
//! also took the stored move on every control sequence but an `SGR`, and the
//! store and the take together cost vim, whose finals are 88.6% `CUP`, 2.9% of
//! its ingest -- work or `csi_final`'s block layout, not attributed further,
//! because neither needs to exist. Looking back at the bytes costs a control
//! sequence nothing measurable: the final byte's position is an argument both
//! callers of `csi_final` already hold, and the look-back runs on a `?25l`,
//! seven times in helix's capture and seventeen in vim's.
//!
//! **A read boundary is what the tail is for.** The move and the hide can
//! arrive in two reads, or be cut by one, and the arena holds only the read
//! being parsed. So the stream's last bytes before it are kept, one short copy
//! a read, and the look-back reads through them -- the same answer at any read
//! size, which the tests hold at one byte. The window is [`TAIL`] bytes ending
//! at the hide's `l`, which is always in the read, so the tail keeps one fewer:
//! the hide's six bytes and a `CUP` of up to twenty-six, `ESC [ 65535 ; 65535 H`
//! with room to spare; a longer one is not a placement at any read size. The
//! first cut of this rule let a whole read reach further back than the tail,
//! so a long move placed at one read size and not another; the cap is the fix.
//!
//! **Every hide decides**, a hide of a hidden cursor too: helix hides once a
//! frame and never shows the cursor between, so a hook on the change from
//! shown to hidden would record its first frame's selection and never the
//! next. A hide after anything else clears the placement. A show, a screen
//! switch, either reset and a change of geometry forget it.
//!
//! **What is not a placement.** A hide with other modes in its sequence
//! (`?25;7l`), the 8-bit `CSI` hide under ISO 8859-1 -- five bytes, so the
//! hide's own bytes are checked and not only its final `l`, or a byte in
//! front of it would put the move's final byte where a 7-bit hide's is -- a
//! `CUP` with a marker, an intermediate or a colon, and any byte between the
//! move and the hide, an `SGR` included. Relative moves, `ESC Y` under VT52 and
//! a `DECRC` are not moves here.
//!
//! **Output that scrolls moves it with its text**, as it moves an image: a
//! placement is a line of its screen's numbering and a column, the anchor an
//! image and an open SPA's start take (`Paint::image_anchor`). A full-screen
//! scroll moves the window over the lines and the placement stays on its
//! line; a region, `IL` and `DL` move the lines in their band and the
//! placement with them, and side margins move only a placement between
//! them. **A placement whose text leaves the screen is gone**: deleted with
//! a band, scrolled off the alternate screen, which keeps no lines, or onto
//! the row a scroll down blanks. On the primary a line scrolled into the
//! history is off the screen and names no position; the only way back is a
//! scroll down, which brings the row back blank, and that forgets it.
//! Helix's next frame places the cursor again either way.

use baud_domain::{
    BandMove, BytePos, Col, Cols, ImageScreen, LineIdx, ReadLen, Row, Rows, StreamByte,
};

/// Where a placing hide found the cursor: a line of its screen's numbering
/// and a column, so a scroll that moves the text moves it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Placed {
    pub(crate) screen: ImageScreen,
    pub(crate) line: LineIdx,
    pub(crate) col: Col,
}

/// How far a hide looks back, its own bytes included: the window ends at the
/// hide's `l`, which is always in the read, so the tail keeps one fewer.
const TAIL: usize = 32;

/// `ESC [ ? 2 5 l`, the one spelling of a hide that places, last byte first.
const HIDE_BACK: [StreamByte; 6] = [
    StreamByte::from_stream(b'l'),
    StreamByte::from_stream(b'5'),
    StreamByte::from_stream(b'2'),
    StreamByte::from_stream(b'?'),
    StreamByte::from_stream(b'['),
    StreamByte::ESC,
];

/// A placement, and the stream bytes a hide looks back through.
#[derive(Clone, Copy)]
pub(crate) struct Placement {
    /// Where the cursor was when a placing hide found it.
    placed: Option<Placed>,
    /// The stream's last bytes before the read being parsed, as many as a
    /// look-back can reach: `NUL` before the first, which none matches.
    tail: [StreamByte; TAIL - 1],
    /// Where the read being parsed starts in the arena.
    read: BytePos,
}

impl Placement {
    pub(crate) const NONE: Placement = Placement {
        placed: None,
        tail: [StreamByte::NUL; TAIL - 1],
        read: BytePos::ZERO,
    };

    /// A read starting at `base` is about to be parsed.
    pub(crate) fn begin(&mut self, base: BytePos) {
        self.read = base;
    }

    /// The read just parsed: its last bytes join the tail.
    pub(crate) fn end(&mut self, read: &[StreamByte]) {
        let keep = self.tail.len();
        match read.len() >= keep {
            true => self.tail.copy_from_slice(&read[read.len() - keep..]),
            false => {
                self.tail.copy_within(read.len().., 0);
                self.tail[keep - read.len()..].copy_from_slice(read);
            }
        }
    }

    /// The byte `k` before the one at `at` in the stream: from the read in
    /// the arena, or before it, from the tail -- and nothing past [`TAIL`]
    /// bytes, even where the read holds more, so the window is the stream's
    /// and not the read's.
    fn back(&self, arena: &[u8], at: BytePos, k: ReadLen) -> Option<StreamByte> {
        let k = k.index();
        if k >= TAIL {
            return None;
        }
        let here = at.index() - self.read.index();
        match k <= here {
            true => Some(StreamByte::from_stream(arena[at.index() - k])),
            false => self
                .tail
                .get(self.tail.len().checked_sub(k - here)?)
                .copied(),
        }
    }

    /// Whether the bytes ending at `at` are exactly `ESC [ ? 2 5 l` with a
    /// complete `CUP` or `HVP` straight before them.
    fn placing(&self, arena: &[u8], at: BytePos) -> bool {
        let b = |k| self.back(arena, at, ReadLen::from_index(k));
        let hide = HIDE_BACK.len();
        if !HIDE_BACK.iter().enumerate().all(|(k, h)| b(k) == Some(*h)) {
            return false;
        }
        let moved = b(hide).is_some_and(|f| {
            f.is(StreamByte::from_stream(b'H')) || f.is(StreamByte::from_stream(b'f'))
        });
        if !moved {
            return false;
        }
        let mut k = hide + 1;
        while b(k).is_some_and(StreamByte::is_dcs_param_byte) {
            k += 1;
        }
        b(k) == Some(StreamByte::from_stream(b'[')) && b(k + 1) == Some(StreamByte::ESC)
    }

    /// A `?25l` whose final byte is at `at` found the cursor on line `line`
    /// of `screen`'s numbering, at `col` on a screen `cols` wide: placed there
    /// if its bytes are a placing hide and the cursor is on the grid, and not
    /// placed otherwise. A cursor past the last column is in pending wrap,
    /// which no `CUP` leaves.
    pub(crate) fn hidden(
        &mut self,
        arena: &[u8],
        at: BytePos,
        (screen, line): (ImageScreen, LineIdx),
        col: Col,
        cols: Cols,
    ) {
        let on_grid = col.index() < cols.index();
        self.placed = (on_grid && self.placing(arena, at)).then_some(Placed { screen, line, col });
    }

    /// Whether a placement is held: the one test the scrolls pay.
    #[inline(always)]
    pub(crate) fn is_placed(&self) -> bool {
        self.placed.is_some()
    }

    /// **The lines `[lo, hi)` of `screen` moved `k` `way`**: a region's
    /// scroll by copy, `IL`, `DL`, a top region's rows put back. A placement
    /// in the band moves with its line, and one the move pushed out of the
    /// band was deleted with it and goes -- `ImageStore::scroll_band`'s
    /// rule. Side margins pass the column range a placement must be in.
    pub(crate) fn band_moved(
        &mut self,
        screen: ImageScreen,
        (lo, hi): (LineIdx, LineIdx),
        within: (Col, Col),
        k: Rows,
        way: BandMove,
    ) {
        let Some(p) = self.placed.as_mut() else {
            return;
        };
        if p.screen != screen || p.line < lo || p.line >= hi || p.col < within.0 || p.col > within.1
        {
            return;
        }
        let moved = match way {
            BandMove::Up => p.line.minus(k).filter(|l| *l >= lo),
            BandMove::Down => Some(p.line.plus(k)).filter(|l| *l < hi),
        };
        match moved {
            Some(l) => p.line = l,
            None => self.placed = None,
        }
    }

    /// `screen`'s lines all moved down `k` under a window that could not
    /// move back over them, at line zero (`ImageStore::shift`).
    pub(crate) fn shift(&mut self, screen: ImageScreen, k: Rows) {
        if let Some(p) = self.placed.as_mut().filter(|p| p.screen == screen) {
            p.line = p.line.plus(k);
        }
    }

    /// `screen`'s window moved, and `[lo, hi)` are the lines its text is
    /// still in -- the window, and on the primary the history behind it --
    /// with the row on `blanked`, if any, brought in blank: a placement on a
    /// line outside them, or blanked, is forgotten.
    pub(crate) fn scrolled(
        &mut self,
        screen: ImageScreen,
        (lo, hi): (LineIdx, LineIdx),
        blanked: Option<LineIdx>,
    ) {
        let gone = self.placed.is_some_and(|p| {
            p.screen == screen && (Some(p.line) == blanked || p.line < lo || p.line >= hi)
        });
        if gone {
            self.placed = None;
        }
    }

    /// A show, a screen switch, a reset or a change of geometry. The tail is
    /// the stream's and stays.
    pub(crate) fn forget(&mut self) {
        self.placed = None;
    }

    /// The placement, as a row of a window starting at line `top` of
    /// `screen`, `rows` tall: none while its line is off it.
    pub(crate) fn placed(
        &self,
        screen: ImageScreen,
        top: LineIdx,
        rows: Rows,
    ) -> Option<(Row, Col)> {
        let p = self.placed.filter(|p| p.screen == screen)?;
        let r = p.line.since(top).filter(|r| r.index() < rows.index())?;
        Some((Row::from_index(r.index()), p.col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A cursor off the grid is not placed.** No byte stream reaches this
    /// since an `ESC` abandons a `CSI` -- bytes that spell a move are one, and
    /// a move lands on the grid -- so the refusal is held here, against the
    /// bytes of a placing hide, with the cursor put where no move could put
    /// it. The control is the same hide with the cursor on the grid.
    #[test]
    fn a_cursor_off_the_grid_is_not_placed() {
        let arena = b"\x1b[3;7H\x1b[?25l";
        let at = BytePos::from_index(arena.len() - 1);
        let cols = Cols::of_index(200);
        let (screen, rows) = (ImageScreen::Primary, Rows::of_count(50));
        let line = |r| (screen, LineIdx::FIRST.plus(Rows::of_count(r)));
        let mut p = Placement::NONE;
        p.begin(BytePos::ZERO);
        p.hidden(arena, at, line(2), Col::at_index(6), cols);
        assert_eq!(
            p.placed(screen, LineIdx::FIRST, rows),
            Some((Row::from_index(2), Col::at_index(6))),
            "the control"
        );
        p.hidden(arena, at, line(0), Col::at_index(200), cols);
        assert_eq!(
            p.placed(screen, LineIdx::FIRST, rows),
            None,
            "pending wrap, one past the last column"
        );
    }
}
