//! The terminal's state, and the one borrow the parser holds.

use crate::graph::Node;
use crate::storage::{MAX_COLS, MAX_ROWS, Storage};
use baud_domain::*;

/// A terminal: a cursor, a rendition, a grid, and where the parser stands.
///
/// **All of it behind one `&mut self`.** A C prototype of this loop used
/// file-scope globals and ran 16-34% slower; `__restrict` recovered only 2.5%
/// of that. The presumed mechanism is LLVM's `noalias`, and the rule that
/// falls out is that state does not migrate to statics.
pub struct Term<'b> {
    pub(crate) store: Storage<'b>,

    /// Where the parser stands in [`crate::graph`].
    pub(crate) node: Node,
    /// Which byte protocol is in force. A second sub-graph, not a flag.
    pub(crate) encoding: InputEncoding,

    pub(crate) cy: Row,
    pub(crate) cx: Col,
    pub(crate) rows: Rows,
    pub(crate) cols: Cols,
    /// Screen row `y` lives at physical row `(y + rot) % rows`. A scroll moves
    /// this and nothing else, so retiring a line is O(1) and history is free
    /// until something reads it.
    pub(crate) rot: RowRot,

    pub(crate) style: StyleId,
    pub(crate) flags: CellFlags,
    pub(crate) charset: Charset,

    /// The bits of a multi-byte character so far.
    ///
    /// Beside the node rather than inside it, because a graph is indexed by a
    /// small integer and an accumulator is not one. Safe because the only
    /// edges *into* the UTF-8 nodes write it: it is set on entry to the only
    /// nodes that read it, so it cannot be stale.
    pub(crate) utf8: Utf8Acc,
    /// The shortest-form floor for the lead byte's length. Without it every
    /// overlong decodes to a real character -- `E0 80 AF` renders `/`.
    pub(crate) utf8_min: Utf8Min,

    /// Bytes ingested, the denominator of every amplification figure.
    pub(crate) bytes_in: usize,
    /// Cells written, the numerator. Two arms of an experiment that disagree
    /// here are not doing the same work, whatever their timings say.
    pub(crate) cells_written: usize,
    /// Run-length and encoded-width histograms; see `Paint::runs`.
    #[cfg(feature = "diag")]
    pub runs: [usize; 8],
    #[cfg(feature = "diag")]
    pub run_len: [usize; 5],
}

impl<'b> Term<'b> {
    /// Geometry comes from a [`WinSize`], which is the one place `ws_col` and
    /// `ws_row` are destructured -- so they cannot be transposed here.
    pub fn new(store: Storage<'b>, ws: &WinSize) -> Term<'b> {
        let (cols, rows) = ws.geometry();
        let (cols, rows) = (cols.clamp(MAX_COLS), rows.clamp(MAX_ROWS));
        Term {
            store,
            node: Node::Ground,
            encoding: InputEncoding::Utf8,
            cy: Row::ZERO,
            cx: Col::ZERO,
            rows,
            cols,
            rot: RowRot::NONE,
            style: StyleId::DEFAULT,
            flags: CellFlags::NONE,
            charset: Charset::Ascii,
            utf8: Utf8Acc::lead(0),
            utf8_min: Utf8Min::Two,
            bytes_in: 0,
            cells_written: 0,
            #[cfg(feature = "diag")]
            runs: [0; 8],
            #[cfg(feature = "diag")]
            run_len: [0; 5],
        }
    }

    /// The grid, as the writers see it.
    #[inline(always)]
    pub(crate) fn view(&mut self) -> View<'_> {
        View::new(
            self.store.vp,
            self.store.used,
            Cols::from_stride(MAX_COLS),
            self.cols,
            self.rows,
            self.rot,
        )
    }

    /// Bytes ingested and cells written, for the amplification ratio rule #1
    /// is about.
    pub fn amplification(&self) -> (usize, usize) {
        (self.bytes_in, self.cells_written)
    }

    /// The materialised viewport, un-rotated so a renderer gets a contiguous
    /// slice in screen order.
    ///
    /// **`&mut` is not a wart.** The rotation is undone here, once a *frame*,
    /// rather than on every scroll -- which is where the laziness that makes a
    /// scroll O(1) is paid for.
    pub fn viewport(&mut self) -> &[PackedCell] {
        let stride = Cols::from_stride(MAX_COLS);
        let live = RowBlock::leading(self.rows, stride);
        if self.rot.index() != 0 {
            // Un-rotated in place, once a *frame*, where the scroll that made
            // the rotation happens once a *scroll*. `RowBlock` names both
            // spans so the row count and the stride cannot be transposed.
            let turned = RowBlock::leading(Rows::of_index(self.rot.index()), stride);
            live.rotate_left_by(self.store.vp, turned);
            self.store.used[..self.rows.index()].rotate_left(self.rot.index());
            self.rot = RowRot::NONE;
        }
        live.of(self.store.vp)
    }

    /// Place bytes in the arena at `base`, the way a `read(2)` would.
    ///
    /// The core never copies on the parse path: a read writes here and the
    /// parser reads here. This is the seam, and it exists so a caller can hand
    /// the kernel `&mut arena[base..]` directly.
    pub fn write_at(&mut self, base: BytePos, src: &[u8]) {
        let from = base.index();
        self.store.arena[from..from + src.len()].copy_from_slice(src);
    }

    pub fn cursor(&self) -> (Row, Col) {
        (self.cy, self.cx)
    }
}
