//! **Hover text baud draws in the frame**: what a word on DEC's indicator
//! line means, shown while the pointer is over it, on macOS and on Linux
//! alike, and what a light on the lights' own line means on Linux, whose
//! window has no tooltips of its own. Wayland has no tooltip protocol,
//! and its `xdg_popup` -- which "can be used to implement for example
//! menus, popovers, tooltips"
//! (`[[source: wp-xdg-shell]]`) -- would be a second surface with its own
//! buffer that `baud-present-wayland` does not make; drawn here, the tip
//! is cells of the frame every path already draws, as the link preview's
//! bar is ([`crate::linked`]), so its pixels are the renderer's on both
//! systems and a test reads them without a compositor.
//!
//! **The look**: the text wrapped at its words into lines of at most
//! [`TIP_COLS`] cells, a space either side of each, every line padded to
//! the widest, in the selection's colours as the link preview's bar and
//! the find field are; right-aligned at the window's right edge, where the
//! words are, and ending one row above the line, so the box reads apart
//! from the line's own reverse video. Its [`TipArea`] spans that gap row
//! too, so the pointer can move from the word up onto the tip without it
//! going: WCAG 2.2's 1.4.13, "the pointer can be moved over the additional
//! content without the additional content disappearing"
//! (`[[source: wcag22]]`). Over a line above the main display, the lights'
//! at the top, the box opens down instead ([`Opens::Down`]), its first
//! line a row below the main display's first.

use baud_domain::{Codepoint, Col, Cols, Row, Rows};

use crate::find::Field;
use crate::overlay::Hints;

/// The widest a tip's text runs, in cells: about the measure of a
/// paragraph a line, narrower than the 80 columns the words sit in.
pub const TIP_COLS: usize = 60;

/// **The cells a tip covers and the pointer may cross**: rows `top` to
/// `bottom` and columns `left` to `right`, the ends exclusive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TipArea {
    pub top: Row,
    pub bottom: Row,
    pub left: Col,
    pub right: Col,
}

impl TipArea {
    /// Whether cell `row`, `col` of the main display is in it.
    pub fn holds(&self, row: Row, col: Col) -> bool {
        (self.top.index()..self.bottom.index()).contains(&row.index())
            && (self.left.index()..self.right.index()).contains(&col.index())
    }
}

/// **What a tip draws on the next frame**: its lines, a field each, and
/// the area the pointer may cross without it going.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TipLook {
    pub lines: Vec<Field>,
    pub area: TipArea,
}

/// `text`'s words in lines of at most `width` characters, a word longer
/// than that cut into pieces of it.
fn wrap(text: &str, width: Cols) -> Vec<Vec<char>> {
    let width = width.index().max(1);
    let mut lines: Vec<Vec<char>> = Vec::new();
    let mut line: Vec<char> = Vec::new();
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let mut word: Vec<char> = word.chars().collect();
        while !word.is_empty() {
            let gap = usize::from(!line.is_empty());
            if line.len() + gap + word.len() <= width {
                if gap == 1 {
                    line.push(' ');
                }
                line.append(&mut word);
            } else if line.is_empty() {
                let rest = word.split_off(width.min(word.len()));
                lines.push(core::mem::replace(&mut word, rest));
            } else {
                lines.push(core::mem::take(&mut line));
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// **Which way a tip opens from the line it explains.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opens {
    /// Up from a line under the main display: DEC's, or the lights' at the
    /// bottom.
    Up,
    /// Down from a line above it: the lights' at the top.
    Down,
}

/// **`text` laid out as a tip above DEC's line**, under a main display
/// `rows` tall and `cols` wide: wrapped at [`TIP_COLS`] or the width less
/// its two spaces, right-aligned, its last line on the row above the main
/// display's last, which is left as it is between the box and the line.
/// With too few rows for the gap it sits on the last row, and with too few
/// for every line it shows the first of them that fit; none with no room
/// at all.
pub fn tip_look(text: &str, cols: Cols, rows: Rows) -> Option<TipLook> {
    tip_look_opening(text, cols, rows, Opens::Up)
}

/// **`text` laid out as a tip that `opens` up or down** from its line:
/// [`tip_look`]'s box, or for a line above the main display the same box
/// mirrored, its first line on the row below the main display's first,
/// that row left as the gap and spanned by the area.
pub fn tip_look_opening(text: &str, cols: Cols, rows: Rows, opens: Opens) -> Option<TipLook> {
    let (cols, rows) = (cols.index(), rows.index());
    let measure = TIP_COLS.min(cols.checked_sub(2)?);
    if measure == 0 {
        return None;
    }
    let lines = wrap(text, Cols::of_index(measure));
    let n = lines.len();
    if n == 0 || rows == 0 {
        return None;
    }
    let width = lines.iter().map(Vec::len).max().unwrap_or(0) + 2;
    let left = cols - width;
    let gap = usize::from(rows > n);
    let shown = n.min(rows - gap);
    // The lines' first row, and the rows the area spans: from the box to
    // the line's side of the main display, the gap included.
    let (top, area) = match opens {
        Opens::Up => (rows - gap - shown, (rows - gap - shown, rows)),
        Opens::Down => (gap, (0, gap + shown)),
    };
    let fields = lines[..shown]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let mut text: Vec<Codepoint> = Vec::with_capacity(width);
            text.push(Codepoint::from_char(' '));
            text.extend(line.iter().map(|&c| Codepoint::from_char(c)));
            text.resize(width, Codepoint::from_char(' '));
            Field {
                row: Row::from_index(top + i),
                col: Col::at_index(left),
                hints: vec![Hints::SELECTION; width],
                caret: Col::at_index(left + width),
                text,
            }
        })
        .collect();
    Some(TipLook {
        lines: fields,
        area: TipArea {
            top: Row::from_index(area.0),
            bottom: Row::from_index(area.1),
            left: Col::at_index(left),
            right: Col::at_index(cols),
        },
    })
}
