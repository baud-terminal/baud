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
//! **Bytes that spell a `CUP` were not always dispatched as one.** baud's
//! `CSI` nodes consume an `ESC` and stay in the sequence, where
//! `[[source: williams]]`'s anywhere rule abandons it, so `CSI ESC [ 3 ; 7 H`
//! dispatches `CSI [` and prints `3;7H`. A hide after that places the cursor
//! where the text left it -- on the grid, because a cursor off it, the
//! pending-wrap column no `CUP` leaves, is refused. The grammar is its own
//! change, and `pinned_a_move_whose_esc_arrived_inside_a_csi_places_where_the_text_left_it`
//! goes red when it lands.
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
//! a `DECRC` are not moves here. **A scroll does not move it**: a placement
//! names a screen position, and output that scrolls under a hidden cursor
//! leaves it naming the row the text scrolled onto until the next hide.

use baud_domain::{BytePos, Col, Cols, ReadLen, Row, StreamByte};

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
    placed: Option<(Row, Col)>,
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

    /// A `?25l` whose final byte is at `at` found the cursor at `cursor` on a
    /// screen `cols` wide: placed there if its bytes are a placing hide and
    /// the cursor is on the grid, and not placed otherwise. A cursor past the
    /// last column is in pending wrap, which no `CUP` leaves.
    pub(crate) fn hidden(&mut self, arena: &[u8], at: BytePos, cursor: (Row, Col), cols: Cols) {
        let on_grid = cursor.1.index() < cols.index();
        self.placed = (on_grid && self.placing(arena, at)).then_some(cursor);
    }

    /// A show, a screen switch, a reset or a change of geometry. The tail is
    /// the stream's and stays.
    pub(crate) fn forget(&mut self) {
        self.placed = None;
    }

    pub(crate) fn placed(self) -> Option<(Row, Col)> {
        self.placed
    }
}
