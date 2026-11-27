//! A narrow character written over one half of a wide character blanks the
//! other half, **at every read size and on every way the cursor reached the
//! row**.
//!
//! The rule is the one `put_glyph`, `put_ascii_run` and the run writers
//! already apply: a cell holding a wide character's lead or its `WIDE_TAIL`
//! is half of one character, and writing into it leaves the other half
//! orphaned unless the writer blanks it. What these tests hold is the
//! shortcut in front of that repair. `Paint` keeps how far the cursor's row
//! is known to be written, and a write at or past it takes the path that
//! skips the repair, because past a row's extent there is nothing to repair.
//! That mark was set to zero at the start of every read and on every row
//! change -- "the row it lands on is one nothing has written or one the
//! scroll has just blanked" -- and neither is true. A read starting on a row
//! that holds text, a wrap or a `LF` onto one, `CUU`, `CUD`, `VPA`, `RI`, a
//! reverse wrap, `SU` and `SD` moving text under the cursor, a screen swap
//! and VT52's cursor keys all landed the cursor on a written row with the
//! mark at zero, and the next write orphaned what it half covered.
//!
//! The read-start case is the chunk-invariance defect found first --
//! `U+1100 CR a` keeps the tail at reads 1 to 4 -- and the others are the
//! same mark going stale inside one read, so the whole read was no oracle:
//! `ESC [ 2 ; 1 H`, four wide characters, `ESC [ 1 ; 20 H b © ~the` lost
//! the `©` in one read, where `put_ascii_run` found the tail the fresh write
//! of the `©` had left and blanked the `©` as its lead. So every test here
//! asserts the screen the rule gives, at every read size, rather than
//! comparing two chunkings.
//!
//! **Both encodings, and the wide character is on the screen in both.** The
//! setup writes it under UTF-8; the ISO 8859-1 arm then sends `ESC % @`
//! before the overwrite, so the write over the half is Latin-1's. A program
//! prefixed with `ESC % @` whole would put no wide character on the screen
//! at all, and its arm would pass whatever the write path did. Each test
//! checks the setup alone first, the positive control.

mod support;

use baud_core::*;

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

/// Eight columns, enough for a wide character and a cut, and four rows.
const EIGHT: WinSize = WinSize {
    ws_row: 4,
    ws_col: 8,
};
/// Twenty columns, for the cases that need a wider row than `EIGHT`.
const TWENTY: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

/// The first `want.len()` rows of a terminal at `ws` fed `bytes` in reads
/// of `read` bytes.
fn rows(ws: &WinSize, bytes: &[u8], read: ReadLen, want: &[&str]) -> Vec<String> {
    let cols = usize::from(ws.ws_col);
    support::with_term(ws, |t| {
        for piece in bytes.chunks(read.index()) {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        }
        (0..want.len())
            .map(|y| {
                t.row(Row::from_index(y)).expect("a row on the grid")[..cols]
                    .iter()
                    .map(|&c| glyph(t, c))
                    .collect()
            })
            .collect()
    })
}

/// `setup` alone shows `before`; `setup` then `utf8`, and `setup` then
/// `ESC % @` then `latin1`, each show `want` at every read size. Returns the
/// first wrong screen of each arm, so a table of cases reports every case
/// that fails rather than the first.
fn wrong_at_some_read_size(
    ws: &WinSize,
    setup: &str,
    before: &[&str],
    utf8: &[u8],
    latin1: &[u8],
    want: &[&str],
) -> Vec<String> {
    let whole = ReadLen::from_index(setup.len());
    assert_eq!(
        rows(ws, setup.as_bytes(), whole, before),
        before,
        "the positive control: the wide character is on the screen before the overwrite"
    );
    let arms = [
        ("UTF-8", [setup.as_bytes(), utf8].concat()),
        ("ISO 8859-1", [setup.as_bytes(), b"\x1b%@", latin1].concat()),
    ];
    let mut wrong = Vec::new();
    for (enc, bytes) in arms {
        if let Some((read, got)) = (1..=bytes.len())
            .map(ReadLen::from_index)
            .map(|read| (read, rows(ws, &bytes, read, want)))
            .find(|(_, got)| got != want)
        {
            wrong.push(format!(
                "{enc}, {}-byte reads of {bytes:02x?}: {got:?}, not {want:?}",
                read.index()
            ));
        }
    }
    wrong
}

fn at_every_read_size(
    ws: &WinSize,
    setup: &str,
    before: &[&str],
    utf8: &[u8],
    latin1: &[u8],
    want: &[&str],
) {
    let wrong = wrong_at_some_read_size(ws, setup, before, utf8, latin1, want);
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The first case found: `CR` and then `a` over a wide lead, which kept the
/// tail at reads 1 to 4 -- every split that put the `a` in a read of its own
/// or behind the `CR`.
#[test]
fn a_letter_after_cr_blanks_the_tail_it_cuts() {
    at_every_read_size(
        &EIGHT,
        "\u{1100}",
        &["\u{1100}]......"],
        b"\ra",
        b"\ra",
        &["a......."],
    );
}

/// The second case found: `CUB 2` and a two-byte letter, `é` as `C3 A9`
/// under UTF-8 and `E9` under ISO 8859-1, so the narrow non-ASCII writer
/// takes the cut in both.
#[test]
fn a_letter_after_cub_blanks_the_tail_it_cuts() {
    at_every_read_size(
        &EIGHT,
        "\u{1100}",
        &["\u{1100}]......"],
        "\x1b[2Dé".as_bytes(),
        b"\x1b[2D\xe9",
        &["é......."],
    );
}

/// Defect (a): with `DECAWM` reset the cursor pins to the last column, which
/// holds the tail, and `a` there blanks the lead. One byte at a time kept the
/// lead. A regional-indicator pair, two cells by `Emoji_Presentation`, takes
/// the same path.
#[test]
fn a_letter_on_the_tail_at_the_margin_under_decawm_reset_blanks_the_lead() {
    let xs = "x".repeat(18);
    // The pair's lead cell shows its first indicator; the second is a mark.
    for (wide, lead) in [
        ("\u{4e16}", "\u{4e16}"),
        ("\u{1f1ef}\u{1f1f5}", "\u{1f1ef}"),
    ] {
        let before = format!("{xs}{lead}]");
        at_every_read_size(
            &TWENTY,
            &format!("{xs}{wide}"),
            &[&before],
            b"\x1b[?7lab",
            b"\x1b[?7lab",
            &["xxxxxxxxxxxxxxxxxx.b"],
        );
    }
}

/// **The mark is where the write ended, not where the cursor stops.** With
/// `DECAWM` reset already, a wide character written into the last two
/// columns leaves the cursor pinned on the last, its tail; the mark followed
/// the cursor and stopped one short, so the next letter took the fresh path
/// in the same read and kept the lead. A `U+FE0F` that widens the character
/// before it into the last column is the same shape. Found by a `?7l` atom
/// tried in `tests/properties.rs` once the read-start mark was fixed (it
/// goes in as `decawm-reset` with the next commit), where the atoms fed one
/// at a time agreed with the rule and the same program in one read did not.
///
/// **Each setup is written twice, the second time after a `CUP` home.** A
/// read starts its mark past every column, so a read that holds the whole
/// program repairs the first row whatever the writes raise the mark to; the
/// `CUP` reads the row's real mark, zero, inside the read, and from there
/// only the raise keeps the next letter off the fresh path.
#[test]
fn a_letter_after_a_wide_character_in_the_last_two_columns_blanks_the_lead() {
    for home in ["", "\x1b[H"] {
        at_every_read_size(
            &EIGHT,
            &format!("{home}\x1b[?7lxxxxxx\u{4e16}"),
            &["xxxxxx\u{4e16}]"],
            b"a",
            b"a",
            &["xxxxxx.a"],
        );
        at_every_read_size(
            &EIGHT,
            &format!("{home}\x1b[?7lxxxxxx\u{2620}\u{fe0f}"),
            &["xxxxxx\u{2620}]"],
            b"a",
            b"a",
            &["xxxxxx.a"],
        );
    }
}

/// Defect (b): `CUP` home onto a lead, then a base with a combining mark in
/// the next read. In three reads the tail stayed. Under ISO 8859-1 there is
/// no combining mark, and the base alone takes the same read-start path.
#[test]
fn a_base_and_its_mark_on_a_lead_blank_the_tail() {
    at_every_read_size(
        &TWENTY,
        "\u{4e16}\u{4e16}\u{4e16}\u{4e16}\x1b[H",
        &["\u{4e16}]\u{4e16}]\u{4e16}]\u{4e16}]............"],
        "e\u{301}".as_bytes(),
        b"e",
        &["e.\u{4e16}]\u{4e16}]\u{4e16}]............"],
    );
}

/// Defect (c): `©` wraps onto row 1's first lead, then `~the` runs over the
/// rest. The `©` must keep its cell, the `t` cuts the second character's
/// tail, which `h` overwrites, and the `e` cuts the third's. One read lost
/// the `©`; split reads kept the third character's tail.
#[test]
fn a_wrap_onto_a_lead_blanks_the_tail_and_keeps_the_character() {
    at_every_read_size(
        &TWENTY,
        "\x1b[2;1H\u{4e16}\u{4e16}\u{4e16}\u{4e16}\x1b[1;20Hb",
        &[
            "...................b",
            "\u{4e16}]\u{4e16}]\u{4e16}]\u{4e16}]............",
        ],
        "©~the".as_bytes(),
        b"\xa9~the",
        &["...................b", "©~the.\u{4e16}]............"],
    );
}

/// Defect (d): a ZWJ family read as ISO 8859-1. Its bytes `F0 9F 91 A8 E2
/// 80 8D F0 9F 91 A9` are `ð`, an `APC` the `PU1` after it cancels, `¨` in
/// the last column, `â` wrapped onto row 1, `PAD`, then `8D`, a C1 `RI`, up
/// onto row 0's first tail, where `ð` must take the lead with it; `©` cuts
/// the next character, whose tail `t` overwrites. Re-derived from
/// `tests/properties.rs` with the bare `ESC % @` atom on the parent of the
/// commit that fixed it, as `[wide-three x 3, latin1-enter, zwj-family,
/// ascii]`: one byte at a time kept the lead at column 0 and the `©`, one
/// read kept the lead and lost the `©`. The UTF-8 arm is the same screen
/// spelled in 7 bits, `ESC M` for the `RI`.
#[test]
fn a_reverse_index_onto_a_tail_under_iso_8859_1_blanks_the_lead() {
    let wide = "\u{4e16}".repeat(9);
    let before = format!("{}..", "\u{4e16}]".repeat(9));
    let fox = "the quick brown fox";
    at_every_read_size(
        &TWENTY,
        &wide,
        &[&before],
        format!("ð¨â\x1bMð©{fox}").as_bytes(),
        &[
            // ISO Latin-1 Supplemental into G2, which GR invokes, so each
            // byte is its ISO 8859-1 character as when this was found.
            b"\x1b.A".as_slice(),
            b"\xf0\x9f\x91\xa8\xe2\x80\x8d\xf0\x9f\x91\xa9",
            fox.as_bytes(),
        ]
        .concat(),
        &[".ð©the quick brown f", "ox.................."],
    );
}

/// One case of the table below: its name, the setup, what the setup shows,
/// the move and the letter (the same bytes in both encodings), and what
/// they show.
type Case<'a> = (&'a str, &'a str, &'a [&'a str], &'a [u8], &'a [&'a str]);

/// **Every way onto a written row, inside one read.** Each control moves the
/// cursor onto a row whose first cell is a wide lead, or moves that row
/// under the cursor, and `a` is written there; the tail must go. These were
/// wrong in one read as well as split, because the mark the move left was
/// the row before's, or zero. `DECIC` is the one that moves text along the
/// cursor's own row, so its letter goes onto the tail the insert pushed
/// past the mark, and the lead must go.
#[test]
fn every_way_onto_a_written_row_blanks_the_tail() {
    let wide = "\u{4e16}]......";
    let cases: &[Case<'_>] = &[
        // Down, onto row 1, from home.
        (
            "LF",
            "\x1b[2;1H\u{4e16}\x1b[H",
            &["........", wide],
            b"\na",
            &["........", "a......."],
        ),
        (
            "CUD",
            "\x1b[2;1H\u{4e16}\x1b[H",
            &["........", wide],
            b"\x1b[Ba",
            &["........", "a......."],
        ),
        (
            "CNL",
            "\x1b[2;1H\u{4e16}\x1b[H",
            &["........", wide],
            b"\x1b[Ea",
            &["........", "a......."],
        ),
        (
            "VPA",
            "\x1b[2;1H\u{4e16}\x1b[H",
            &["........", wide],
            b"\x1b[2da",
            &["........", "a......."],
        ),
        (
            "VPR",
            "\x1b[2;1H\u{4e16}\x1b[H",
            &["........", wide],
            b"\x1b[ea",
            &["........", "a......."],
        ),
        // `CUP` from a row with nothing on it, so the mark the move leaves
        // is that row's, zero, unless the move reads its own.
        (
            "CUP",
            "\u{4e16}",
            &[wide],
            b"\n\x1b[Ha",
            &["a.......", "........"],
        ),
        // Up, onto row 0.
        (
            "CUU",
            "\u{4e16}\x1b[2;1H",
            &[wide],
            b"\x1b[Aa",
            &["a......."],
        ),
        (
            "CPL",
            "\u{4e16}\x1b[2;3H",
            &[wide],
            b"\x1b[Fa",
            &["a......."],
        ),
        ("RI", "\u{4e16}\x1b[2;1H", &[wide], b"\x1bMa", &["a......."]),
        // `?45`: a backspace at column 0 goes to the last column of the row
        // above, which holds a tail and wrapped into this one.
        (
            "reverse-wrap BS",
            "\x1b[1;7H\u{4e16}x\x1b[?45h\x1b[2;1H",
            &["......\u{4e16}]"],
            b"\x08a",
            &[".......a"],
        ),
        // The row moves under the cursor.
        (
            "SU",
            "\x1b[2;1H\u{4e16}\x1b[H",
            &["........", wide],
            b"\x1b[Sa",
            &["a......."],
        ),
        (
            "SD",
            "\u{4e16}\x1b[2;1H",
            &[wide],
            b"\x1b[Ta",
            &["........", "a......."],
        ),
        // `?47` keeps the cursor: down a row and back up on the alternate
        // screen, then back to the primary's row 0.
        // `?47 h` keeps the cursor too, on a row of the alternate screen
        // that holds what the last visit left: entered from home on the
        // primary, whose row 0 is blank.
        (
            "?47 h",
            "\x1b[?47h\u{4e16}\x1b[?47l",
            &["........"],
            b"\x1b[H\x1b[?47ha",
            &["a......."],
        ),
        (
            "?47 l",
            "\u{4e16}\x1b[?47h\x1b[2;1H\x1b[A",
            &["........"],
            b"\x1b[?47la",
            &["a......."],
        ),
        // VT52's cursor keys, entered after the encoding switch so that
        // `ESC % @` is not read as a VT52 command.
        (
            "VT52 down",
            "\x1b[2;1H\u{4e16}\x1b[H",
            &["........", wide],
            b"\x1b[?2l\x1bBa",
            &["........", "a......."],
        ),
        (
            "VT52 up",
            "\u{4e16}\x1b[2;1H",
            &[wide],
            b"\x1b[?2l\x1bAa",
            &["a......."],
        ),
        // DECSLRM homes the cursor.
        (
            "DECSLRM",
            "\u{4e16}\x1b[2;3H\x1b[?69h",
            &[wide],
            b"\x1b[1;8sa",
            &["a......."],
        ),
        // `DECIC` moves the cursor's row right under it, so the wide
        // character it pushed lies past the mark the row had.
        (
            "DECIC",
            "ab\u{4e16}\x1b[H",
            &["ab\u{4e16}]...."],
            b"\x1b[2'}\x1b[6Gx",
            &["..ab.x.."],
        ),
        // Below the scrolling region on the last row, `LF` stays put, on a
        // row that holds text.
        (
            "LF below the region",
            "\x1b[4;1H\u{4e16}\x1b[1;3r\x1b[4;1H",
            &["........", "........", "........", wide],
            b"\na",
            &["........", "........", "........", "a......."],
        ),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .flat_map(|&(name, setup, before, tail, want)| {
            wrong_at_some_read_size(&EIGHT, setup, before, tail, tail, want)
                .into_iter()
                .map(move |w| format!("{name}: {w}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}
