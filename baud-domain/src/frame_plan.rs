//! **What a frame shows, as its plan read it**: the cursor, the selected
//! span, where the input method's window opens, the thumb's window over the
//! history, the screen the trail moves over, the splash's gate, the
//! synchronised update's state, and the effects the plan owes the window.
//! Facts only: the plan fills them from the terminal before the frame is
//! drawn, and the frame reads them and never the terminal.

use crate::{Col, Cols, Content, CursorStyle, PointerShape, Row, Rows};

/// Whether a cursor is on the screen being drawn: the live screen's while
/// `DECTCEM` shows it, or the status line's while DECSASD selects the line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShown {
    Yes,
    No,
}

/// **The cursor as a frame presents it**: its cell, `None` where it is not
/// drawn, and its style, the shape with the blink `?12` folds in. What the
/// caret compares from one frame to the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PresentedCursor {
    pub at: Option<(Row, Col)>,
    pub style: CursorStyle,
}

/// The cursor a trail follows, while the window is on the live screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrailCursor {
    /// The cursor as it is drawn.
    Shown(Row, Col),
    /// Where a program put the cursor before it hid it: an editor's
    /// selection, where the editor draws its own block. Never drawn by the
    /// terminal, because the program hid it.
    Placed(Row, Col),
}

impl TrailCursor {
    pub fn cell(self) -> (Row, Col) {
        match self {
            TrailCursor::Shown(r, c) | TrailCursor::Placed(r, c) => (r, c),
        }
    }

    /// The shape the trail arrives at: the drawn cursor's own, and the whole
    /// cell for a placement, because the program draws a block there whatever
    /// shape the terminal's cursor is configured to -- the smear's hole has to
    /// be that block, or a bar-shaped hole leaves the trail over it.
    pub fn style(self, drawn: CursorStyle) -> CursorStyle {
        match self {
            TrailCursor::Shown(..) => drawn,
            TrailCursor::Placed(..) => CursorStyle::SteadyBlock,
        }
    }
}

/// **The cursor a frame draws**: the cell it is drawn in, its style, whether
/// one is on the screen, and the cursor as the caret compares it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorPlan {
    /// The cell the cursor is drawn in: the status line's row while
    /// DECSASD selects the line, else the input method's caret in its
    /// composition while one is drawn at the cursor, else the cursor's.
    pub at: (Row, Col),
    pub style: CursorStyle,
    pub shown: CursorShown,
    pub presented: PresentedCursor,
    /// The cursor a trail follows, `None` while the window looks back.
    pub followed: Option<TrailCursor>,
}

impl CursorPlan {
    /// The cell the cursor is drawn in, `None` where none is on the screen.
    pub fn drawn(self) -> Option<(Row, Col)> {
        match self.shown {
            CursorShown::Yes => Some(self.at),
            CursorShown::No => None,
        }
    }
}

/// **The cells an input method's candidate window is asked to open over**:
/// its caret's or its range's in the composition as drawn, the cursor's
/// with none, the find field's caret while the field is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImeRect {
    pub at: (Row, Col),
    pub width: Cols,
}

/// Whether a synchronised update is open, and if it is, whether the driver
/// still honours it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Update {
    /// No update is open.
    Closed,
    /// One is open and its hold younger than the driver's bound: its frames
    /// are held.
    Honoured,
    /// One is open and its hold older than the bound: the driver draws
    /// through it, and what it held is on screen.
    Outlived,
}

/// Where the window is: the lines retained behind the live screen, how
/// many of them it is up, and the screen's rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScrollWindow {
    pub retained: Rows,
    pub back: Rows,
    pub rows: Rows,
}

/// **The thumb's window over the history**, read by the plan while the
/// program shows a scrollbar.
#[derive(Clone, Copy, Debug)]
pub struct ScrollbarPlan {
    pub window: ScrollWindow,
}

/// **How far a program's scroll took the text under the cursor**, rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Carry {
    Up(Rows),
    Down(Rows),
}

impl Carry {
    /// Where it took row `r`: never above the first.
    pub fn row(self, r: Row) -> Row {
        match self {
            Carry::Up(n) => Row::from_index(r.index().saturating_sub(n.index())),
            Carry::Down(n) => Row::from_index(r.index() + n.index()),
        }
    }
}

/// **The screen the trail moves over**: its size, the rows the window
/// looks back, and how far the program's scroll took the followed
/// cursor's line since the last plan.
#[derive(Clone, Copy, Debug)]
pub struct ScreenMove {
    pub cols: Cols,
    pub rows: Rows,
    pub back: Rows,
    pub carried: Option<Carry>,
}

/// **Whether the shell has drawn**, read by the frame's plan while the
/// splash holds for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplashGate {
    Waiting,
    Drawn,
}

/// **A stored line counted from the window's top**: a row of the window,
/// or a line that many above its top or past its last row. The display's
/// word wrap draws cells of such lines on the window's own: the rows of
/// its first paragraph above it on its first line, and, while it looks
/// back, the rows of its last paragraph below it on its last; a selection,
/// a find mark and a pointer name them by this.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct WindowLine(isize);

impl WindowLine {
    /// Window row `r`.
    pub fn of(r: Row) -> WindowLine {
        WindowLine(isize::try_from(r.index()).unwrap_or(isize::MAX))
    }

    /// The line `n` above this one.
    pub fn up(self, n: Rows) -> WindowLine {
        WindowLine(
            self.0
                .saturating_sub(isize::try_from(n.index()).unwrap_or(isize::MAX)),
        )
    }

    /// The line `n` below this one.
    pub fn down(self, n: Rows) -> WindowLine {
        WindowLine(
            self.0
                .saturating_add(isize::try_from(n.index()).unwrap_or(isize::MAX)),
        )
    }

    /// The window row this line is, of a window `rows` tall; none above its
    /// top or past its last row.
    pub fn row(self, rows: Rows) -> Option<Row> {
        usize::try_from(self.0)
            .ok()
            .filter(|&y| y < rows.index())
            .map(Row::from_index)
    }

    /// How many lines this is below `earlier`; none above it.
    pub fn since(self, earlier: WindowLine) -> Option<Rows> {
        let d = self.0.checked_sub(earlier.0)?;
        usize::try_from(d).ok().map(Rows::of_count)
    }
}

/// Whether a selected span runs in reading order or is a rectangle of
/// columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpanShape {
    /// Contiguous, from one cell to another in row-major order.
    Stream,
    /// Every row between the endpoints, over one range of columns.
    Block,
}

/// **What a frame draws selected**: the selection's ends, or `?1001`'s
/// region's, sorted, each line counted from the window's top and
/// unclipped -- above the window or past its last row included -- and its
/// shape. A stream's ends are in row-major order, a block's its top-left
/// and bottom-right corners; both are inclusive. The shape is part of
/// equality, so a switch between the two over the same ends repaints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelectedSpan {
    pub from: (WindowLine, Col),
    pub to: (WindowLine, Col),
    pub shape: SpanShape,
}

impl SelectedSpan {
    /// A span of the window's rows alone, from `from` to `to`.
    pub fn on_window(from: (Row, Col), to: (Row, Col), shape: SpanShape) -> SelectedSpan {
        SelectedSpan {
            from: (WindowLine::of(from.0), from.1),
            to: (WindowLine::of(to.0), to.1),
            shape,
        }
    }

    /// Whether the cell at `(r, c)` is inside the span's geometry.
    pub fn contains(&self, r: Row, c: Col) -> bool {
        self.contains_line(WindowLine::of(r), c)
    }

    /// Whether column `c` of stored line `l`, the window's or one past it,
    /// is inside the span's geometry.
    pub fn contains_line(&self, l: WindowLine, c: Col) -> bool {
        let ((fl, fc), (tl, tc)) = (self.from, self.to);
        match self.shape {
            SpanShape::Stream => {
                let p = (l, c.index());
                p >= (fl, fc.index()) && p <= (tl, tc.index())
            }
            SpanShape::Block => {
                l >= fl && l <= tl && c.index() >= fc.index() && c.index() <= tc.index()
            }
        }
    }

    /// Whether the cell at `(r, c)`, holding `content`, is drawn selected:
    /// a wide tail takes its lead's answer, so a character is never half
    /// selected where a block's edge cuts it -- one straddling the left
    /// edge is out, one straddling the right edge is in whole.
    pub fn selects(&self, r: Row, c: Col, content: Content) -> bool {
        self.selects_line(WindowLine::of(r), c, content)
    }

    /// [`SelectedSpan::selects`] for column `c` of stored line `l`, the
    /// window's or one past it that the display draws on a window line.
    pub fn selects_line(&self, l: WindowLine, c: Col, content: Content) -> bool {
        let c = if content.is_wide_tail() && c.index() > 0 {
            Col::at_index(c.index() - 1)
        } else {
            c
        };
        self.contains_line(l, c)
    }
}

/// **The pointer's state over the window**: the shape the session
/// resolved, a previewed link's or the program's, or none for the window's
/// own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerState {
    Shown(Option<PointerShape>),
}

/// **What the plan asks of the window**, applied by the frame once.
/// Every applier handles every effect: a match that leaves one out does
/// not build.
///
/// ```compile_fail,E0004
/// fn apply(e: baud_domain::WindowEffect) {
///     match e {
///         baud_domain::WindowEffect::Title(_) => {}
///     }
/// }
/// ```
///
/// The legal path:
///
/// ```
/// fn apply(e: baud_domain::WindowEffect) {
///     match e {
///         baud_domain::WindowEffect::Title(_) => {}
///         baud_domain::WindowEffect::Pointer(_) => {}
///     }
/// }
/// ```
#[cfg(feature = "alloc")]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WindowEffect {
    /// The window's title, pushed when it changed.
    Title(alloc::string::String),
    /// The pointer's shape over the window.
    Pointer(PointerState),
}

/// **The window effects owed**, pushed by the plan and drained by the
/// frame, in the order pushed; the queue keeps its allocation.
#[cfg(feature = "alloc")]
#[derive(Default, Debug)]
pub struct Effects(pub alloc::vec::Vec<WindowEffect>);

#[cfg(feature = "alloc")]
impl Effects {
    pub fn push(&mut self, effect: WindowEffect) {
        self.0.push(effect);
    }

    /// Every effect owed, once: none is left for the next frame.
    pub fn drain(&mut self) -> alloc::vec::Drain<'_, WindowEffect> {
        self.0.drain(..)
    }
}
