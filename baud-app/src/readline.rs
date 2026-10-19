//! xterm's readline modes (`[[source: ctlseqs]]`): `?2001`, a click on the
//! cursor's line moves the shell's point there; `?2005`, each pasted byte
//! quoted with the tty's literal-next character; `?2006`, a paste's newlines
//! kept. Pure, so the tests run on every platform; the macOS driver, which
//! has the pointer and the paste, calls them.

use baud_core::{Col, Cols, ImagePx, Row, Wrap};

/// Which cursor key a `?2001` click sends, and how many times.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrow {
    Left,
    Right,
}

/// The cursor keys a click at `click` sends to move the point from the
/// cursor at `cursor`, as xterm's `ReadLineMovePoint` counts them
/// (`[[source: xterm-button]]`): the click's column taken at the nearest
/// cell boundary, plus a whole line of `cols` for each row between it and
/// the cursor, less the cursor's column. **Only on the cursor's own line**,
/// the rows between joined by wraps (`rowOnCurrentLine`): a click on another
/// line, or on the cursor, sends nothing.
pub fn point_move(
    wrapped: impl Fn(Row) -> Wrap,
    cursor: (Row, Col),
    click: (Row, Col),
    cols: Cols,
) -> Option<(Arrow, usize)> {
    let (cr, cc) = (cursor.0.index(), cursor.1.index());
    let (kr, kc) = (click.0.index(), click.1.index());
    let (first, last) = (cr.min(kr), cr.max(kr));
    if (first..last).any(|r| wrapped(Row::from_index(r)) == Wrap::Ended) {
        return None;
    }
    let to = kc + kr * cols.index();
    let from = cc + cr * cols.index();
    match to.cmp(&from) {
        core::cmp::Ordering::Equal => None,
        core::cmp::Ordering::Greater => Some((Arrow::Right, to - from)),
        core::cmp::Ordering::Less => Some((Arrow::Left, from - to)),
    }
}

/// The column a click names for `?2001`: the cell boundary nearest the
/// pixel `x`, "Correct by half a width - we are acting on a boundary, not
/// on a cell" (`eventColBetween`, `[[source: xterm-button]]`).
pub fn boundary(x: ImagePx, cell_w: ImagePx) -> Col {
    let w = cell_w.index().max(1);
    Col::at_index((x.index() + (w - 1) / 2) / w)
}

/// How a paste's newlines go to the child.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Newlines {
    /// As typed input carries them: a line break is CR (`?2006` reset).
    AsReturn,
    /// As they are (`?2006` set).
    Literal,
}

/// The body of an unbracketed paste's line breaks for `newlines`, and each
/// byte then preceded by `quote` under `?2005`, as xterm turns `\n` into
/// `\r` and then quotes byte by byte (`_WriteSelectionData` and
/// `_qWriteSelectionData`, `[[source: xterm-button]]`).
pub fn body(text: &str, newlines: Newlines, quote: Option<Quote>) -> Vec<u8> {
    let plain = match newlines {
        Newlines::AsReturn => text.replace("\r\n", "\r").replace('\n', "\r"),
        Newlines::Literal => text.to_owned(),
    };
    quoted(plain.as_bytes(), quote)
}

/// Each byte of `bytes` preceded by `quote`, or `bytes` as they are.
pub fn quoted(bytes: &[u8], quote: Option<Quote>) -> Vec<u8> {
    match quote {
        None => bytes.to_vec(),
        Some(Quote(q)) => bytes.iter().flat_map(|&b| [q, b]).collect(),
    }
}

/// The byte a `?2005` paste puts before each of its bytes: the tty's
/// literal-next character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quote(pub u8);

impl Quote {
    /// When the tty gives none: xterm's `XTERM_LNEXT`, Control-V.
    pub const LNEXT: Quote = Quote(0x16);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at((r, c): (usize, usize)) -> (Row, Col) {
        (Row::from_index(r), Col::at_index(c))
    }

    /// **A click on the cursor's line moves the point by the columns
    /// between**: right of the cursor, Right; left, Left; on it, nothing.
    #[test]
    fn a_click_on_the_cursors_line_moves_the_point_by_the_columns_between() {
        let none = |_| Wrap::Ended;
        assert_eq!(
            point_move(none, at((3, 10)), at((3, 15)), Cols::of_index(80)),
            Some((Arrow::Right, 5))
        );
        assert_eq!(
            point_move(none, at((3, 10)), at((3, 2)), Cols::of_index(80)),
            Some((Arrow::Left, 8))
        );
        assert_eq!(
            point_move(none, at((3, 10)), at((3, 10)), Cols::of_index(80)),
            None
        );
    }

    /// **Rows joined by wraps are one line**, a whole width for each: from
    /// column 70 of row 2 to column 5 of row 4 across two wrapped rows is
    /// 15 left of the end and 85 more. A row between that ended stops it.
    #[test]
    fn a_wrapped_line_counts_a_whole_width_a_row_and_an_ended_row_stops_it() {
        let wraps = |r: Row| match r.index() {
            2 | 3 => Wrap::Continues,
            _ => Wrap::Ended,
        };
        assert_eq!(
            point_move(wraps, at((2, 70)), at((4, 5)), Cols::of_index(80)),
            Some((Arrow::Right, 95))
        );
        assert_eq!(
            point_move(wraps, at((4, 5)), at((2, 70)), Cols::of_index(80)),
            Some((Arrow::Left, 95))
        );
        assert_eq!(
            point_move(wraps, at((4, 5)), at((5, 0)), Cols::of_index(80)),
            None
        );
        assert_eq!(
            point_move(wraps, at((1, 5)), at((2, 0)), Cols::of_index(80)),
            None
        );
    }

    /// The boundary nearest the pointer, xterm's arithmetic exactly: in a
    /// 20-pixel cell, `(x + 9) / 20`, so pixels 0-10 name the cell's own
    /// column and 11-19 the next.
    #[test]
    fn a_click_names_the_nearest_cell_boundary() {
        let b = |x| boundary(ImagePx::of(x), ImagePx::of(20));
        assert_eq!(b(0), Col::at_index(0));
        assert_eq!(b(10), Col::at_index(0));
        assert_eq!(b(11), Col::at_index(1));
        assert_eq!(b(30), Col::at_index(1));
        assert_eq!(b(31), Col::at_index(2));
    }

    /// `?2006` reset turns line breaks into CR, set keeps them; `?2005`
    /// quotes every byte, a multi-byte character's each byte, after.
    #[test]
    fn a_paste_body_follows_2005_and_2006() {
        assert_eq!(body("a\nb\r\nc", Newlines::AsReturn, None), b"a\rb\rc");
        assert_eq!(body("a\nb\r\nc", Newlines::Literal, None), b"a\nb\r\nc");
        assert_eq!(
            body("a\nb", Newlines::AsReturn, Some(Quote::LNEXT)),
            b"\x16a\x16\r\x16b"
        );
        assert_eq!(
            body("é", Newlines::Literal, Some(Quote::LNEXT)),
            b"\x16\xc3\x16\xa9"
        );
    }
}
