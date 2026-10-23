//! With `DECAWM` reset, a character written into the last column leaves the
//! cursor **on** that column, and what comes next has to know it.
//!
//! `[[source: vt510-decawm]]`: "If the DECAWM function is reset, then graphic
//! characters received when the cursor is at the right border of the page
//! replace characters already on the page." `Paint::advance` holds it by
//! leaving the cursor on the last column, with no pending-wrap sentinel, so
//! the next character lands there. Two paths did not
//! hold that, and the `decawm-reset` atom in `tests/properties.rs` found both
//! at once, shrinking to `?7l`, nineteen letters, `e U+0301` on twenty
//! columns:
//!
//! - **A continuation joins the character before the cursor**, `View::lead`
//!   of `cx - 1`. On a pinned cursor that is the column before the character
//!   just written, so `e U+0301` in the last column put the accent on the
//!   letter left of the `e`, and a `U+FE0F` after `☠ a` widened the `☠` over
//!   the `a`. `[[source: uax29]]` puts the mark in the `e`'s cluster.
//! - **A run that fits left the cursor past the margin.** `Paint::blit` and
//!   `Paint::run` set the cursor to the run's end, so a run ending in the last
//!   column put it one past, the sentinel `advance` exists to refuse, and one
//!   character at a time did not: `ICH` then inserted nowhere. Whichever path
//!   the bytes took decided which column the next write and the accent used,
//!   so the screen depended on where the reads fell.
//!
//! Every case is asserted at every read size. The accent cases have no
//! ISO 8859-1 arm, because that set has no combining character; the `ICH`
//! cases run under both.

mod support;

use baud_core::*;

const EIGHT: WinSize = WinSize {
    ws_row: 4,
    ws_col: 8,
};

/// One cell as a character: `.` for a blank and `]` for a wide tail, and a
/// cell with continuations its base, read back from `t`'s store.
fn glyph(t: &Term<'_>, c: PackedCell) -> char {
    if c.content.is_wide_tail() {
        return ']';
    }
    match t.base_of(c).as_codepoint() {
        Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
        None => '.',
    }
}

/// Row 0 and the marks on its last two columns, fed in reads of `read`.
fn seen(bytes: &[u8], read: ReadLen) -> (String, Vec<u32>, Vec<u32>) {
    support::with_term(&EIGHT, |t| {
        for piece in bytes.chunks(read.index()) {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        }
        let row = t.row(Row::ZERO).expect("a row on the grid")[..8]
            .iter()
            .map(|&c| glyph(t, c))
            .collect();
        let before = t.marks(Row::ZERO, Col::at_index(6)).collect();
        let last = t.marks(Row::ZERO, Col::at_index(7)).collect();
        (row, before, last)
    })
}

/// `bytes` shows `want` at every read size; each element of `arms` is one
/// encoding's spelling.
fn at_every_read_size(arms: &[(&str, &[u8])], want: (&str, &[u32], &[u32])) {
    let mut wrong = Vec::new();
    for &(enc, bytes) in arms {
        if let Some((read, got)) = (1..=bytes.len())
            .map(ReadLen::from_index)
            .map(|read| (read, seen(bytes, read)))
            .find(|(_, got)| (got.0.as_str(), got.1.as_slice(), got.2.as_slice()) != want)
        {
            wrong.push(format!(
                "{enc}, {}-byte reads of {bytes:02x?}: {got:x?}, not {want:x?}",
                read.index()
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The atom's own shape on eight columns, with the base written three ways:
/// an ASCII letter after a run, a two-byte letter, and the end of a run that
/// fills the row, which `blit` writes whole when the read holds it.
#[test]
fn a_mark_after_a_letter_in_the_last_column_joins_that_letter() {
    for (base, want) in [("e", "xxxxxxxe"), ("\u{e9}", "xxxxxxx\u{e9}")] {
        let bytes = format!("\x1b[?7lxxxxxxx{base}\u{301}");
        at_every_read_size(&[("UTF-8", bytes.as_bytes())], (want, &[], &[0x301]));
    }
}

/// `U+FE0F` after `☠ a` in the last two columns belongs to the `a`, which
/// no selector widens: the `☠` keeps one cell and the `a` is not
/// overwritten.
#[test]
fn a_selector_after_a_letter_in_the_last_column_leaves_the_character_before_it_alone() {
    let bytes = "\x1b[?7lxxxxxx\u{2620}a\u{fe0f}";
    at_every_read_size(
        &[("UTF-8", bytes.as_bytes())],
        ("xxxxxx\u{2620}a", &[], &[0xfe0f]),
    );
}

/// A run that ends in the last column leaves the cursor on it, so `ICH`
/// inserts a blank there and the last character goes. The ASCII run is
/// `blit`'s, under both encodings.
#[test]
fn ich_after_an_ascii_run_that_ends_in_the_last_column_inserts_in_that_column() {
    at_every_read_size(
        &[
            ("UTF-8", b"\x1b[?7labcdefgh\x1b[@"),
            ("ISO 8859-1", b"\x1b%@\x1b[?7labcdefgh\x1b[@"),
        ],
        ("abcdefg.", &[], &[]),
    );
}

/// The box-drawing run is `put_narrow_run`'s. Under ISO 8859-1 each `é` is
/// one character, through `advance`, which was right already: the control.
#[test]
fn ich_after_a_narrow_run_that_ends_in_the_last_column_inserts_in_that_column() {
    let boxes = "\x1b[?7l\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\x1b[@";
    at_every_read_size(
        &[("UTF-8", boxes.as_bytes())],
        (
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}.",
            &[],
            &[],
        ),
    );
    at_every_read_size(
        &[(
            "ISO 8859-1",
            b"\x1b%@\x1b[?7l\xe9\xe9\xe9\xe9\xe9\xe9\xe9\xe9\x1b[@",
        )],
        ("\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}.", &[], &[]),
    );
}

/// Four wide characters are `put_wide_run`'s; they fill the row and leave
/// the cursor on the last one's tail, and `ICH` there cuts the character,
/// so both halves go.
#[test]
fn ich_after_a_wide_run_that_ends_in_the_last_column_inserts_in_that_column() {
    let wide = "\x1b[?7l\u{4e16}\u{4e16}\u{4e16}\u{4e16}\x1b[@";
    at_every_read_size(
        &[("UTF-8", wide.as_bytes())],
        ("\u{4e16}]\u{4e16}]\u{4e16}]..", &[], &[]),
    );
}

/// Every row's cells, and each cell that carries marks with its marks.
type Screen = (Vec<String>, Vec<(usize, usize, Vec<u32>)>);

/// Every row's cells and where the marks are, after `bytes` on `from`
/// in reads of `read`, a resize to `to` and then `after` in one read.
fn across_a_resize(
    from: &WinSize,
    to: &WinSize,
    bytes: &[u8],
    read: ReadLen,
    after: &[u8],
) -> Screen {
    let room = WinSize {
        ws_row: from.ws_row.max(to.ws_row),
        ws_col: from.ws_col.max(to.ws_col),
    };
    let mut owned = Owned::new(&room);
    let mut t = Term::new(owned.storage(), from);
    for piece in bytes.chunks(read.index()) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    assert_eq!(t.resize(to), Resized::Ok, "the resize took");
    t.write_at(BytePos::ZERO, after);
    t.feed(BytePos::ZERO, ReadLen::from_index(after.len()));
    let (rows, cols) = (usize::from(to.ws_row), usize::from(to.ws_col));
    let text = (0..rows)
        .map(|y| {
            t.row(Row::from_index(y)).expect("a row on the grid")[..cols]
                .iter()
                .map(|&c| glyph(&t, c))
                .collect()
        })
        .collect();
    let marks = (0..rows)
        .flat_map(|y| (0..cols).map(move |x| (y, x)))
        .map(|(y, x)| {
            let m: Vec<u32> = t.marks(Row::from_index(y), Col::at_index(x)).collect();
            (y, x, m)
        })
        .filter(|(_, _, m)| !m.is_empty())
        .collect();
    (text, marks)
}

/// **A resize moves the margin and not the cursor's column**, so the pin is
/// still the last write's. `e` in the last of eight columns, the screen
/// widened to ten, then `U+0301`: the cluster machine joined the mark to the
/// `e`, and it goes on the `e`'s cell, column 7, not the `x` before it. A
/// test that the cursor sits on the right margin as well as on a pin put it
/// on column 6 once the margin had moved to 9.
#[test]
fn a_mark_after_a_resize_wider_still_joins_the_letter_written_last() {
    const TEN: WinSize = WinSize {
        ws_row: 4,
        ws_col: 10,
    };
    let bytes = b"\x1b[?7lxxxxxxxe";
    let want_row = "xxxxxxxe..";
    for read in (1..=bytes.len()).map(ReadLen::from_index) {
        let (text, marks) = across_a_resize(&EIGHT, &TEN, bytes, read, "\u{301}".as_bytes());
        assert_eq!(text[0], want_row, "{}-byte reads", read.index());
        assert_eq!(
            marks,
            [(0, 7, vec![0x301])],
            "{}-byte reads: the mark is on the e",
            read.index()
        );
    }
}

/// Narrower, the line reflows and the cursor goes with the `e` it was on,
/// to column 1 of the next row: the mark goes there too.
#[test]
fn a_mark_after_a_resize_narrower_joins_the_letter_written_last() {
    const TEN: WinSize = WinSize {
        ws_row: 4,
        ws_col: 10,
    };
    let bytes = b"\x1b[?7lxxxxxxxxxe";
    for read in (1..=bytes.len()).map(ReadLen::from_index) {
        let (text, marks) = across_a_resize(&TEN, &EIGHT, bytes, read, "\u{301}".as_bytes());
        assert_eq!(
            text[..2],
            ["xxxxxxxx", "xe......"],
            "{}-byte reads",
            read.index()
        );
        assert_eq!(
            marks,
            [(1, 1, vec![0x301])],
            "{}-byte reads: the mark is on the e",
            read.index()
        );
    }
}

/// Every cell on the first two rows that carries marks, with its marks.
type Marks = Vec<(usize, usize, Vec<u32>)>;

/// The marks on rows 0 and 1 after `bytes` in reads of `read`.
fn marks(bytes: &[u8], read: usize) -> Marks {
    support::with_term(&EIGHT, |t| {
        for piece in bytes.chunks(read) {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        }
        (0..2)
            .flat_map(|y| (0..8).map(move |x| (y, x)))
            .map(|(y, x)| {
                let m: Vec<u32> = t.marks(Row::from_index(y), Col::at_index(x)).collect();
                (y, x, m)
            })
            .filter(|(_, _, m)| !m.is_empty())
            .collect()
    })
}

/// `between` after `xxxxxxxe`, which ends in the last column, then `tail`:
/// the marks are `want` under `DECAWM` set and reset, at every read size.
fn in_both_modes(before: &str, between: &str, tail: &str, want: &[(usize, usize, &[u32])]) {
    let want: Marks = want.iter().map(|&(y, x, m)| (y, x, m.to_vec())).collect();
    let mut wrong = Vec::new();
    for mode in ["\x1b[?7h", "\x1b[?7l"] {
        let s = format!("{before}{mode}xxxxxxxe{between}{tail}");
        let b = s.as_bytes();
        if let Some((read, got)) = (1..=b.len())
            .map(|read| (read, marks(b, read)))
            .find(|(_, got)| *got != want)
        {
            wrong.push(format!("{s:?} in {read}-byte reads: {got:x?}"));
        }
    }
    assert!(wrong.is_empty(), "want {want:x?}: {wrong:#?}");
}

/// The cell a `z` lands in after `s`, on any of the four rows, at reads of
/// `read`.
fn where_a_character_lands(s: &str, read: usize) -> Option<(usize, usize)> {
    let b = format!("{s}z");
    support::with_term(&EIGHT, |t| {
        for piece in b.as_bytes().chunks(read) {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        }
        (0..4)
            .flat_map(|y| (0..8).map(move |x| (y, x)))
            .find(|&(y, x)| {
                let c = t.row(Row::from_index(y)).expect("a row")[x];
                c.content
                    .as_codepoint()
                    .is_some_and(|cp| cp.value() == u32::from(b'z'))
            })
    })
}

/// **After a control, a combining mark is a cluster of its own and takes the
/// cell a character would** (`[[source: uax29]]` GB4/GB5; TUS D57's
/// defective combining character sequence; note AB, Q24 item 1, Jev
/// OWN_CELL_MARK_ALONE, 0.96/0.99): for each of `s`, under both `DECAWM`
/// modes after `prefix`, the only marked cell on the screen is where a `z`
/// put in the mark's place lands, at every read size. Until 2026-10-23 the
/// mark went to the cell before the cursor, the character the control had
/// stepped past or the pin's; these tests probed the pin with it.
fn as_a_character(prefix: &str, body: &str) {
    let mut wrong = Vec::new();
    for mode in ["\x1b[?7h", "\x1b[?7l"] {
        let s = format!("{prefix}{mode}{body}");
        as_a_character_after(&s, &mut wrong);
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

fn as_a_character_after(s: &str, wrong: &mut Vec<String>) {
    let b = format!("{s}\u{301}");
    for read in 1..=b.len() {
        let want: Vec<(usize, usize, Vec<u32>)> = where_a_character_lands(s, read)
            .filter(|&(y, _)| y < 2)
            .map(|(y, x)| (y, x, vec![0x301]))
            .into_iter()
            .collect();
        let got = marks(b.as_bytes(), read);
        if got != want {
            wrong.push(format!(
                "{b:?} in {read}-byte reads: {got:x?}, not {want:x?}"
            ));
        }
    }
}

/// **A zero-width codepoint the cluster machine does not join**, after a
/// control that does not move the cursor -- an `SGR`, a `BEL`, a `DECSCA`,
/// an `OSC` -- went on the character just written; since 2026-10-23 a
/// combining mark there is a cluster of its own and takes the cell a
/// character would ([`as_a_character`]): the next row's first column with
/// autowrap set, the `e`'s with it reset. The name is from when `DECAWM`
/// decided nothing here.
#[test]
fn a_mark_after_a_control_goes_where_it_goes_with_autowrap_set() {
    for between in ["\x1b[m", "\x07", "\x1b[1\"q", "\x1b]0;t\x07"] {
        as_a_character("", &format!("xxxxxxxe{between}"));
    }
}

/// `U+200B` is zero-width and GB class Control, so the machine breaks
/// before it with no control between: the same path, and the same cell.
#[test]
fn a_zero_width_character_the_machine_breaks_before_goes_to_the_last_letter() {
    in_both_modes("", "", "\u{200b}", &[(0, 7, &[0x200b])]);
}

/// **A wide character in the last two columns, then a control**: the mark
/// takes the cell a character would, the wide character's no more
/// ([`as_a_character`]). It joined the character, which both of its columns
/// reported, until 2026-10-23.
#[test]
fn a_mark_after_a_control_beside_a_wide_character_in_the_last_two_columns_takes_its_own_cell() {
    as_a_character("", "xxxxxx\u{4e16}\x1b[m");
}

/// **A vertical move keeps the column**, and a mark after it takes the cell
/// a character would ([`as_a_character`]); it went to the last column of
/// the row the cursor was on until 2026-10-23.
#[test]
fn a_vertical_move_keeps_the_last_column() {
    let rows = "\x1b[2;1Hyyyyyyyy\x1b[1;1H";
    for between in ["\x1b[B", "\n", "\x1b[B\x1b[A"] {
        as_a_character(rows, &format!("xxxxxxxe{between}"));
    }
}

/// **Guards: a horizontal move ends it.** The cursor is on a cell of its
/// choosing, and the mark takes it as a character would
/// ([`as_a_character`]), even where the move lands back on the last column:
/// `CUF`, `CHA`, `CUP`, `HT`, `HPR`, `CUB` then `CUF`, and VT52's cursor
/// right. Each is the only move in its stream, so each `Paint::set_col` call
/// has a case of its own. The mark went to the cell before until 2026-10-23.
#[test]
fn a_horizontal_move_ends_it() {
    for between in [
        "\x1b[C",
        "\x1b[8G",
        "\x1b[1;8H",
        "\t",
        "\x1b[a",
        "\x1b[D\x1b[C",
        "\x1b[?2l\x1bC\x1b<",
    ] {
        as_a_character("", &format!("xxxxxxxe{between}"));
    }
}

/// And to column 0: `CR`, `NEL`, `CNL`, `CPL`, `CBT`, `LF` under `LNM`, and
/// `DECSLRM`, which homes the cursor. The mark takes column 0 as a character
/// would ([`as_a_character`]); it was refused there, with no cell before the
/// cursor, until 2026-10-23.
#[test]
fn a_move_to_the_first_column_ends_it() {
    let rows = "\x1b[2;1Hyyyyyyyy\x1b[1;1H";
    for between in [
        "\r",
        "\x1bE",
        "\x1b[E",
        "\x1b[F",
        "\x1b[Z",
        "\x1b[20h\n",
        "\x1b[?69h\x1b[1;8s",
    ] {
        as_a_character(rows, &format!("xxxxxxxe{between}"));
    }
}

/// **A pin from before a `?7h` is not read after the `?7l` that follows.**
/// With `DECAWM` set a write leaves `Cold::pinned` alone, so `abc` wrapping
/// onto row 1 left the pin naming the `e`; the mark after the `?7l` belongs
/// to the `c`. `DECSTR` sets `DECAWM` again and, since 2026-10-14, leaves
/// the cursor on the `e` ("DECSTR has no affect on the Active Position",
/// `[[source: dec-std-070]]` 070-4, note c), so it is `?7h` here: the pin it
/// leaves is not read while `DECAWM` is set, and the `?7l` ends it. It homed
/// until then, and `abc` landed on row 0 with the mark on the `c` at 0;2.
///
/// Since 2026-10-23 the `SGR` before the mark makes it a cluster of its own,
/// which takes the cell a character would, after the `c` at 1;3
/// ([`as_a_character`]). The `?7l` is a control too, so no mark here joins
/// the `c` any longer; the pin is held by the joined cases above.
#[test]
fn a_change_of_autowrap_ends_it() {
    for between in ["\x1b[?7h", "\x1b[!p"] {
        let mut wrong = Vec::new();
        as_a_character_after(
            &format!("\x1b[?7lxxxxxxxe{between}abc\x1b[?7l\x1b[m"),
            &mut wrong,
        );
        assert!(wrong.is_empty(), "{wrong:#?}");
    }
}

/// `BS`, `CUB` and VT52's cursor left too. With autowrap reset the cursor
/// steps back onto column 6, with it set from the pending-wrap column onto
/// column 7, and the mark takes that cell as a character would
/// ([`as_a_character`]), where it joined the letter before it until
/// 2026-10-23.
#[test]
fn a_step_back_ends_it() {
    for back in ["\x08", "\x1b[D", "\x1b[?2l\x1bD\x1b<"] {
        as_a_character("", &format!("xxxxxxxe{back}"));
    }
}

/// **A resize that clamps the cursor moves it**, so it ends the pin as any
/// horizontal move does. The alternate screen does not reflow: `abcdefgh`
/// cut to six columns leaves the cursor on the `f`, which was not written
/// last, and a mark the machine joins goes to the `e` before it under both
/// modes, as `cx - 1` of the clamped column puts it with autowrap set,
/// through `attach_continuing`. After a control it is a cluster of its own
/// and takes the cursor's cell, the `f`'s, as a character would (since
/// 2026-10-23).
#[test]
fn a_resize_that_clamps_the_cursor_ends_it() {
    const SIX: WinSize = WinSize {
        ws_row: 4,
        ws_col: 6,
    };
    for (after, at) in [("\u{301}", 4), ("\x1b[m\u{301}", 5)] {
        for mode in ["\x1b[?7h", "\x1b[?7l"] {
            let s = format!("\x1b[?1049h{mode}abcdefgh");
            for read in (1..=s.len()).map(ReadLen::from_index) {
                let (_, marks) =
                    across_a_resize(&EIGHT, &SIX, s.as_bytes(), read, after.as_bytes());
                assert_eq!(
                    marks,
                    [(0, at, vec![0x301])],
                    "{s:?} then {after:?}, in {}-byte reads",
                    read.index()
                );
            }
        }
    }
}
