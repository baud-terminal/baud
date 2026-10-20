//! xterm's readline modes (`[[source: ctlseqs]]`): `?2001`, a click on the
//! cursor's line moves the shell's point there; `?2005`, each pasted byte
//! quoted with the tty's literal-next character; `?2006`, a paste's newlines
//! kept. And `OSC 133`'s click at a prompt: `cl=`'s keys and kitty's
//! `click_events=` report. Pure, so the tests run on every platform. Both
//! drivers call the click's and the paste's (`paste.rs`).

use baud_core::{ClickEvents, Col, Cols, ImagePx, PackedCell, Row, Rows, Wrap};

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

/// **The keys an `OSC 133 ; A ; cl=` click sends**, to the text of
/// `[[source: semantic-prompts]]`: "activated when the user clicks in an
/// input line containing the cursor"; "If the click is in the left prompt,
/// it is treated at the first character of the input area; if it is to the
/// right of input area (in the background area or right prompt) it is
/// treated as just after the input area"; "The terminal calculates the
/// character position of the click minus the character position of the
/// cursor. (Character position is like column position, but double-width
/// characters only count one.)"
///
/// The line is the cursor's rows joined by soft wraps. The input begins at
/// `input`, where `B` or `I` left the cursor, or at the line's start when
/// that is on another line; it ends at the first cell nothing ever wrote,
/// which is where the background begins -- a never-written cell is not a
/// typed space, so the gap before zsh's right prompt is background. The
/// one exception is the pad a wide character leaves in the last column
/// when it wraps: neither the end nor a character, it is skipped. A click
/// on a wide character's right half is that character. No input began, or
/// a click off the line or on the cursor's own character, sends nothing.
pub fn prompt_move<'a>(
    row: impl Fn(Row) -> &'a [PackedCell],
    wrapped: impl Fn(Row) -> Wrap,
    rows: Rows,
    cursor: (Row, Col),
    click: (Row, Col),
    input: Option<(Row, Col)>,
) -> Option<(Arrow, usize)> {
    let input = input?;
    let cr = cursor.0.index();
    let mut first = cr;
    while first > 0 && wrapped(Row::from_index(first - 1)) == Wrap::Continues {
        first -= 1;
    }
    let mut last = cr;
    while last + 1 < rows.index() && wrapped(Row::from_index(last)) == Wrap::Continues {
        last += 1;
    }
    let on = |r: Row| (first..=last).contains(&r.index());
    if !on(click.0) {
        return None;
    }
    // The line's cells in order, less the pad a wide character that wrapped
    // left in the last column of a row that continues: a cell nothing
    // wrote, and neither a character nor the input's end. Each row's first
    // cell in that order, so a position falls where its row's cells do.
    let mut cells: Vec<PackedCell> = Vec::new();
    let mut starts: Vec<(usize, usize)> = Vec::new();
    for r in first..=last {
        let mut of = row(Row::from_index(r));
        if wrapped(Row::from_index(r)) == Wrap::Continues
            && of.last().is_some_and(|c| c.content.is_empty())
        {
            of = &of[..of.len() - 1];
        }
        starts.push((cells.len(), of.len()));
        cells.extend_from_slice(of);
    }
    let lin = |(r, c): (Row, Col)| {
        let (at, len) = starts[r.index() - first];
        at + c.index().min(len)
    };
    let start = if on(input.0) { lin(input) } else { 0 };
    let end = start
        + cells[start..]
            .iter()
            .take_while(|c| !c.content.is_empty())
            .count();
    let mut to = lin(click).clamp(start, end);
    while to > start && cells.get(to).is_some_and(|c| c.content.is_wide_tail()) {
        to -= 1;
    }
    let from = lin(cursor);
    let (lo, hi, arrow) = match to.cmp(&from) {
        core::cmp::Ordering::Equal => return None,
        core::cmp::Ordering::Greater => (from, to, Arrow::Right),
        core::cmp::Ordering::Less => (to, from, Arrow::Left),
    };
    let n = (lo..hi)
        .filter(|&i| cells.get(i).is_none_or(|c| !c.content.is_wide_tail()))
        .count();
    (n > 0).then_some((arrow, n))
}

/// **The report a kitty `click_events=` click sends**: "kitty will thus send
/// a click event to the shell when the user clicks somewhere in the prompt
/// ... The click event is encoded in the SGR encoding from xterm", a left
/// press, `CSI < 0 ; x ; y M` (`[[source: kitty-6d5d0c4-mouse]]`, its
/// move of the cursor to a click at a shell prompt). Its row is the screen's
/// under `1`, and under `2` counted from "the top line of the current
/// prompt ... If the secondary prompt is on the same line or above the
/// mouse position, then the reported y will be with respect to that,
/// otherwise with respect to the primary prompt"
/// (`[[source: kitty-shell-integration]]`). A click above the prompt's
/// first row sends nothing, as kitty's and Ghostty's do not.
pub fn prompt_report(
    events: ClickEvents,
    top: Row,
    later: Option<Row>,
    click: (Row, Col),
) -> Option<Vec<u8>> {
    let (r, c) = (click.0.index(), click.1.index());
    if r < top.index() {
        return None;
    }
    let y = match events {
        ClickEvents::Absolute => r + 1,
        ClickEvents::Relative => {
            let base = later.filter(|l| l.index() <= r).unwrap_or(top);
            r - base.index() + 1
        }
    };
    Some(format!("\x1b[<0;{};{}M", c + 1, y).into_bytes())
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
    use baud_core::Content;

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

    fn cells(text: &str, width: Cols) -> Vec<PackedCell> {
        let mut out: Vec<PackedCell> = text
            .chars()
            .flat_map(|ch| {
                let cell = |c: Content| {
                    let mut p = PackedCell::EMPTY;
                    p.content = c;
                    p
                };
                let cp = baud_core::Codepoint::new(u32::from(ch)).expect("a codepoint");
                let lead = cell(Content::codepoint(cp));
                match ch {
                    '漢' => vec![lead, cell(Content::WIDE_TAIL)],
                    '_' => vec![PackedCell::EMPTY],
                    _ => vec![lead],
                }
            })
            .collect();
        out.resize(width.index(), PackedCell::EMPTY);
        out
    }

    /// **A `cl=` click moves by characters, not columns**: a wide character
    /// is one arrow, the left prompt is the input's first character, and
    /// past the input -- the background, a right prompt after its gap -- is
    /// just after it.
    #[test]
    fn a_prompt_click_counts_characters_and_keeps_to_the_input() {
        // `$ ` is the prompt, `a漢bc` the input, `_` cells nothing wrote,
        // `R` a right prompt.
        let row = cells("$ a漢bc____R", Cols::of_index(12));
        let rows = |_: Row| &row[..];
        let ended = |_: Row| Wrap::Ended;
        let input = Some(at((0, 2)));
        let mv = |cursor, click| {
            prompt_move(rows, ended, Rows::of_count(1), at(cursor), at(click), input)
        };
        // Cursor after `c` (column 7); the wide character's two cells are one.
        assert_eq!(mv((0, 7), (0, 2)), Some((Arrow::Left, 4)));
        assert_eq!(
            mv((0, 7), (0, 4)),
            Some((Arrow::Left, 3)),
            "its right half is it"
        );
        assert_eq!(
            mv((0, 7), (0, 0)),
            Some((Arrow::Left, 4)),
            "the left prompt"
        );
        assert_eq!(
            mv((0, 2), (0, 11)),
            Some((Arrow::Right, 4)),
            "the right prompt"
        );
        assert_eq!(
            mv((0, 2), (0, 9)),
            Some((Arrow::Right, 4)),
            "the background"
        );
        assert_eq!(mv((0, 7), (0, 7)), None);
        assert_eq!(
            prompt_move(rows, ended, Rows::of_count(1), at((0, 7)), at((0, 2)), None),
            None
        );
    }

    /// The line is the cursor's rows joined by soft wraps; a click on
    /// another line sends nothing.
    #[test]
    fn a_prompt_click_stays_on_the_cursors_line() {
        let r0 = cells("$ abcd", Cols::of_index(6));
        let r1 = cells("efgh", Cols::of_index(6));
        let r2 = cells("next", Cols::of_index(6));
        let rows = |r: Row| match r.index() {
            0 => &r0[..],
            1 => &r1[..],
            _ => &r2[..],
        };
        let wraps = |r: Row| match r.index() {
            0 => Wrap::Continues,
            _ => Wrap::Ended,
        };
        let input = Some(at((0, 2)));
        assert_eq!(
            prompt_move(
                rows,
                wraps,
                Rows::of_count(3),
                at((1, 2)),
                at((0, 3)),
                input
            ),
            Some((Arrow::Left, 5))
        );
        assert_eq!(
            prompt_move(
                rows,
                wraps,
                Rows::of_count(3),
                at((1, 2)),
                at((2, 1)),
                input
            ),
            None
        );
        // An input begun on another line: the line's own start bounds it.
        assert_eq!(
            prompt_move(
                rows,
                wraps,
                Rows::of_count(3),
                at((2, 3)),
                at((2, 0)),
                Some(at((0, 2)))
            ),
            Some((Arrow::Left, 3)),
            "the cursor's line is row 2 alone, so it starts at its first column"
        );
    }

    /// **A wide character that wraps leaves a pad, and the pad is not the
    /// input's end**: on 40 columns, `$ ` and 37 `a` fill column 38, so
    /// `漢` cannot fit and goes to the next row, leaving column 39 a cell
    /// nothing wrote in a row that continues. The input runs on past it to
    /// `b`, and the pad is no character.
    #[test]
    fn a_wide_character_wrapped_at_the_margin_leaves_no_character() {
        use baud_core::{BytePos, Owned, ReadLen, Term, WinSize};
        let ws = WinSize {
            ws_col: 40,
            ws_row: 8,
        };
        let mut o = Owned::new(&ws);
        let mut t = Term::new(o.storage(), &ws);
        let mut bytes = b"\x1b]133;A;cl=line\x07$ \x1b]133;B\x07".to_vec();
        bytes.extend(core::iter::repeat_n(b'a', 37));
        bytes.extend("漢b".as_bytes());
        t.write_at(BytePos::ZERO, &bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let view = t.viewport();
        assert!(
            view.row(Row::from_index(0))[39].content.is_empty(),
            "the pad"
        );
        assert_eq!(view.wrapped(Row::from_index(0)), Wrap::Continues);
        assert_eq!(t.cursor(), at((1, 3)));
        let input = t.prompt_click().expect("cl= asked").input;
        let mv = |click| {
            prompt_move(
                |r| view.row(r),
                |r| view.wrapped(r),
                Rows::of_count(8),
                t.cursor(),
                at(click),
                input,
            )
        };
        assert_eq!(mv((1, 0)), Some((Arrow::Left, 2)), "on 漢");
        assert_eq!(mv((1, 2)), Some((Arrow::Left, 1)), "on b");
        assert_eq!(mv((1, 10)), None, "past b, where the cursor is");
        assert_eq!(mv((0, 38)), Some((Arrow::Left, 3)), "on the last a");
    }

    /// kitty's report: a left press at the click's cell, the row the
    /// screen's or counted from the prompt's top, or its later line when
    /// that is at or above the click.
    #[test]
    fn a_prompt_report_is_an_sgr_press() {
        let top = Row::from_index(3);
        let later = Some(Row::from_index(5));
        let rep = |e, click| prompt_report(e, top, later, at(click)).map(String::from_utf8);
        assert_eq!(
            rep(ClickEvents::Absolute, (4, 9)),
            Some(Ok("\x1b[<0;10;5M".into()))
        );
        assert_eq!(
            rep(ClickEvents::Relative, (4, 9)),
            Some(Ok("\x1b[<0;10;2M".into()))
        );
        assert_eq!(
            rep(ClickEvents::Relative, (6, 0)),
            Some(Ok("\x1b[<0;1;2M".into()))
        );
        assert_eq!(rep(ClickEvents::Absolute, (2, 0)), None, "above the prompt");
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
