//! EA and EF, `CSI Ps O` and `CSI Ps N`: erase in the active qualified
//! area and in the active field.
//!
//! `[[source: ecma48]]` 8.3.37: "EA causes some or all character positions
//! in the active qualified area (the qualified area in the presentation
//! component which contains the active presentation position) to be put into
//! the erased state ... 0 the active presentation position and the character
//! positions up to the end of the qualified area ... 1 ... from the
//! beginning of the qualified area up to and including the active
//! presentation position ... 2 all character positions of the qualified
//! area". 8.3.40 says the same of the active field, and 4.2.40 defines a
//! field: "the character position at a character tabulation stop
//! (beginning of the field) and the character positions up to, but not
//! including, the character position at the following character tabulation
//! stop". Both erase guarded positions only under ERM set.
//!
//! baud's qualified areas are SPA and EPA's guarded areas. The active area is
//! the run, in the character path, of positions sharing the active
//! position's guard: a guarded area, or the unguarded positions between two
//! or between one and the page's edge (Jev's lean, RUN). A field stays in its
//! line: the line's start and end bound the positions before its first stop
//! and after its last (Jev's lean, LINE_BOUNDED). Each reads its first
//! parameter and counts the rest (decided by Jev, FIRST_ONLY, 0.90/0.90).

use crate::support;
use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

/// Rows 0 to 2 as text, `.` for an empty position, the guard as `_` where
/// the position is empty, and the two counters.
fn after(bytes: &[u8]) -> (Vec<String>, usize, usize) {
    support::with_term(&WS, |t| {
        support::feed(t, bytes);
        let rows = (0..3)
            .map(|y| {
                t.row(Row::from_index(y)).unwrap()[..20]
                    .iter()
                    .map(|c| match c.content.as_codepoint() {
                        Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                        None if c.flags.is_guarded() => '_',
                        None => '.',
                    })
                    .collect()
            })
            .collect();
        (rows, t.unsupported_param, t.unsupported)
    })
}

/// Row 0 `xyz`, row 1 `abcdefgh` with `def` guarded (SPA before `d`, EPA
/// after `f`), the cursor then placed by the caller.
const PAGE: &[u8] = b"xyz\r\nabc\x1bVdef\x1bWgh";

fn ea(at_and_op: &[u8]) -> (Vec<String>, usize, usize) {
    after(&[PAGE, at_and_op].concat())
}

/// **Outside every guarded area the area is the unguarded run**: from the
/// page's start, across row 0, to the position before `d`.
#[test]
fn ea_outside_a_guarded_area_erases_the_unguarded_run() {
    let (rows, ..) = ea(b"\x1b[2;2H\x1b[O");
    assert_eq!(
        (rows[0].as_str(), rows[1].as_str()),
        ("xyz.................", "a..defgh............")
    );
    let (rows, ..) = ea(b"\x1b[2;2H\x1b[1O");
    assert_eq!(
        (rows[0].as_str(), rows[1].as_str()),
        ("....................", "..cdefgh............")
    );
    let (rows, ..) = ea(b"\x1b[2;2H\x1b[2O");
    assert_eq!(
        (rows[0].as_str(), rows[1].as_str()),
        ("....................", "...defgh............")
    );
    // After the guarded area, the run from `g` to the page's end.
    let (rows, ..) = ea(b"\x1b[2;8H\x1b[2O");
    assert_eq!(
        (rows[0].as_str(), rows[1].as_str()),
        ("xyz.................", "abcdef..............")
    );
}

/// **Inside a guarded area, ERM decides**: reset, its initial state, spares
/// every position of it; set erases them, guard kept, and nothing outside.
#[test]
fn ea_inside_a_guarded_area_follows_erm() {
    let (rows, ..) = ea(b"\x1b[2;5H\x1b[2O");
    assert_eq!(rows[1], "abcdefgh............");
    let (rows, ..) = ea(b"\x1b[6h\x1b[2;5H\x1b[2O");
    assert_eq!(
        (rows[0].as_str(), rows[1].as_str()),
        ("xyz.................", "abc___gh............")
    );
    let (rows, ..) = ea(b"\x1b[6h\x1b[2;5H\x1b[O");
    assert_eq!(rows[1], "abcd__gh............");
}

/// With no guarded area the run is the whole page.
#[test]
fn ea_with_no_guarded_area_erases_the_page() {
    let (rows, ..) = after(b"xyz\r\nabc\r\nq\x1b[2;2H\x1b[2O");
    assert_eq!(
        rows,
        [
            "....................",
            "....................",
            "...................."
        ]
    );
}

/// **The first parameter alone**, the rest counted; a value with no case
/// refuses the sequence, as ED's does.
#[test]
fn ea_and_ef_read_their_first_parameter() {
    let (rows, skipped, _) = ea(b"\x1b[2;2H\x1b[1;2O");
    assert_eq!((rows[1].as_str(), skipped), ("..cdefgh............", 1));
    let (rows, _, refused) = ea(b"\x1b[2;2H\x1b[5O");
    assert_eq!((rows[1].as_str(), refused), ("abcdefgh............", 1));
    let (rows, skipped, _) = after(b"abcdefghijklmnopqrst\x1b[1;12H\x1b[2;1N");
    assert_eq!((rows[0].as_str(), skipped), ("abcdefgh........qrst", 1));
}

const LINE: &[u8] = b"abcdefghijklmnopqrst\r\nABCDEFGHIJKLMNOPQRST";

/// **A field runs from a stop to the next**: the stops are at columns 9
/// and 17, so with the cursor at 12 the field is 9 to 16.
#[test]
fn ef_erases_within_the_field() {
    let field = |op: &[u8]| after(&[LINE, b"\x1b[1;12H", op].concat()).0;
    assert_eq!(field(b"\x1b[N")[0], "abcdefghijk.....qrst");
    assert_eq!(field(b"\x1b[1N")[0], "abcdefgh....mnopqrst");
    assert_eq!(field(b"\x1b[2N")[0], "abcdefgh........qrst");
    assert_eq!(field(b"\x1b[2N")[1], "ABCDEFGHIJKLMNOPQRST", "one line");
}

/// **The line bounds a field**: before the first stop the field starts at
/// the line's start, after the last it ends at the line's end and goes no
/// further, and with no stops the line is one field.
#[test]
fn ef_fields_stay_within_their_line() {
    let (rows, ..) = after(&[LINE, b"\x1b[1;3H\x1b[2N"].concat());
    assert_eq!(rows[0], "........ijklmnopqrst");
    let (rows, ..) = after(&[LINE, b"\x1b[1;18H\x1b[2N"].concat());
    assert_eq!(
        (rows[0].as_str(), rows[1].as_str()),
        ("abcdefghijklmnop....", "ABCDEFGHIJKLMNOPQRST")
    );
    let (rows, ..) = after(&[LINE, b"\x1b[3g\x1b[1;12H\x1b[2N"].concat());
    assert_eq!(
        (rows[0].as_str(), rows[1].as_str()),
        ("....................", "ABCDEFGHIJKLMNOPQRST")
    );
}

/// Under ERM reset EF spares a guarded position, as ED does.
#[test]
fn ef_spares_a_guarded_position_under_erm_reset() {
    let (rows, ..) = after(b"abcdefgh\x1bVijk\x1bWlmno\x1b[1;12H\x1b[2N");
    assert_eq!(rows[0], "abcdefghijk.........");
}

/// **On a double-width line EA and EF stop at the line's end**, its half,
/// and leave the cells the half hides: `[[source: dec-std-070]]` erases a
/// line "x <= end_of_line(line)" (EL, p. 5-155), and EL here does the
/// same. A field's stops are the line's character positions, as HT reads
/// them, by the line's column (p. 5-68), so on a
/// double line the stop at column 9 is its ninth character. Row 1 is
/// `LINE`'s first row made double-width, and a DECIC of 3 from row 2 has
/// pushed its `hij` past the half, where they are kept
/// (`decic_and_decdc_shift_a_sized_line_to_the_right_margin`).
#[test]
fn ea_and_ef_on_a_double_width_line_stop_at_its_half() {
    let dw = |op: &[u8]| after(&[LINE, b"\x1b[1;1H\x1b#6\x1b[2;1H\x1b[3'}", op].concat()).0;
    assert_eq!(
        dw(b"")[0],
        "...abcdefghij.......",
        "the control: hij past the half"
    );
    let ea0 = dw(b"\x1b[1;4H\x1b[O");
    assert_eq!(ea0[0], "..........hij.......", "EA 0, the line to its half");
    assert_eq!(ea0[1], "....................", "EA 0, the next line whole");
    assert_eq!(dw(b"\x1b[1;4H\x1b[K")[0], ea0[0], "the control: EL 0");
    assert_eq!(dw(b"\x1b[2;4H\x1b[1O")[0], ea0[0], "EA 1");
    assert_eq!(dw(b"\x1b[2O")[0], ea0[0], "EA 2");
    assert_eq!(dw(b"\x1b[1;15H\x1b[1O")[0], ea0[0], "EA 1 past the half");
    assert_eq!(
        dw(b"\x1b[1;4H\x1b[N")[0],
        "........fghij.......",
        "EF 0, to the stop"
    );
    assert_eq!(
        dw(b"\x1b[1;12H\x1b[2N")[0],
        "...abcde..hij.......",
        "EF 2, to the half"
    );
    assert_eq!(dw(b"\x1b[1;12H\x1b[1N")[0], "...abcde..hij.......", "EF 1");
    assert_eq!(
        dw(b"\x1b[1;12H\x1b[2N")[1],
        "...ABCDEFGHIJKLMNOPQ",
        "EF, one line"
    );
}
