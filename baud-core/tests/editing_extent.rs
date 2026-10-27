//! **SEE, `CSI Ps Q`** (`[[source: ecma48]]` 8.3.115): "SEE is used to
//! establish the editing extent for subsequent character or line insertion
//! or deletion ... 0 the shifted part is limited to the active page in the
//! presentation component 1 the shifted part is limited to the active line
//! ... 2 ... the active field ... 3 ... the active qualified area 4 the
//! shifted part consists of the relevant part of the entire presentation
//! component." Each test writes `abcdefghij`, `klmnopqrst` and `uvwxyz` on
//! a ten-column screen, then the edit.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

const TEXT: &str = "\x1b[1;1Habcdefghij\x1b[2;1Hklmnopqrst\x1b[3;1Huvwxyz";

/// The four rows as text, `.` for an empty position, `_` for a wide tail.
fn rows(then: &str) -> [String; 4] {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{TEXT}{then}").as_bytes());
        core::array::from_fn(|y| {
            t.viewport().row(Row::from_index(y))[..10]
                .iter()
                .map(|c| match c.content {
                    x if x.is_wide_tail() => '_',
                    x => x
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or('.'),
                })
                .collect()
        })
    })
}

fn cursor(then: &str) -> (usize, usize) {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{TEXT}{then}").as_bytes());
        let (r, c) = t.cursor();
        (r.index(), c.index())
    })
}

/// **Before any SEE, DEC's extent**: `ICH` shifts within the line and the
/// characters pushed past its end are lost (DEC STD 070 ICH note 1), as
/// SEE 1 does (Jev's lean, LINE).
#[test]
fn before_any_see_ich_shifts_within_the_line() {
    let dec = rows("\x1b[1;9H\x1b[2@");
    assert_eq!(
        dec,
        ["abcdefgh..", "klmnopqrst", "uvwxyz....", ".........."]
    );
    assert_eq!(rows("\x1b[1Q\x1b[1;9H\x1b[2@"), dec);
    assert_eq!(
        rows("\x1bc"),
        ["..........", "..........", "..........", ".........."]
    );
}

/// **SEE 0, the page**: what `ICH` pushes off a line's end goes on at the
/// next line's start, and the page's last positions leave; `DCH` pulls the
/// following lines back. `CSI Q`, the parameter omitted, is 0.
#[test]
fn see_page_shifts_across_line_ends() {
    assert_eq!(
        rows("\x1b[Q\x1b[1;9H\x1b[2@"),
        ["abcdefgh..", "ijklmnopqr", "stuvwxyz..", ".........."]
    );
    assert_eq!(
        rows("\x1b[0Q\x1b[1;9H\x1b[2P"),
        ["abcdefghkl", "mnopqrstuv", "wxyz......", ".........."]
    );
    assert_eq!(cursor("\x1b[Q\x1b[1;9H\x1b[2@"), (0, 8), "the cursor stays");
}

/// **SEE 4 is the page here** (Jev, AS_PAGE, 1.00/1.00).
#[test]
fn see_four_is_the_page() {
    assert_eq!(
        rows("\x1b[4Q\x1b[1;9H\x1b[2@"),
        rows("\x1b[0Q\x1b[1;9H\x1b[2@")
    );
}

/// **SEE 2, the field**: the stop at 8 bounds it, so `ICH` at column 2
/// loses `g h` and leaves `i j`, and `DCH` closes the gap before the stop.
#[test]
fn see_field_shifts_within_the_field() {
    assert_eq!(rows("\x1b[2Q\x1b[1;3H\x1b[2@")[0], "ab..cdefij");
    assert_eq!(rows("\x1b[2Q\x1b[1;3H\x1b[2P")[0], "abefgh..ij");
}

/// **SEE 3, the qualified area**: inside the guarded `d e f` a `DCH`
/// closes up there alone; in the unguarded run before it an `ICH` loses
/// `c` at the run's end.
#[test]
fn see_qualified_area_shifts_within_the_area() {
    let guard = "\x1b[1;4H\x1bV\x1b[1;6H\x1bW";
    assert_eq!(
        rows(&format!("{guard}\x1b[3Q\x1b[1;4H\x1b[P"))[0],
        "abcef.ghij"
    );
    assert_eq!(
        rows(&format!("{guard}\x1b[3Q\x1b[1;2H\x1b[@"))[0],
        "a.bdefghij"
    );
}

/// **Across lines the shift keeps within DEC's margins** (Jev's lean,
/// WITHIN_MARGINS): the region's last line ends it, the line below keeps
/// its text; between side margins a line's right margin goes on at the
/// next line's left margin.
#[test]
fn a_shift_across_lines_keeps_within_the_margins() {
    assert_eq!(
        rows("\x1b[1;2r\x1b[Q\x1b[1;9H\x1b[2@"),
        ["abcdefgh..", "ijklmnopqr", "uvwxyz....", ".........."]
    );
    assert_eq!(
        rows("\x1b[?69h\x1b[3;8s\x1b[Q\x1b[1;7H\x1b[2@"),
        ["abcdef..ij", "klghmnopst", "uvqrwxyz..", ".........."]
    );
    assert_eq!(
        rows("\x1b[1;2r\x1b[Q\x1b[3;1H\x1b[2@")[2],
        "uvwxyz....",
        "outside the margins it is ignored"
    );
}

/// **HEM's PRECEDING shifts from the extent's start to the cursor**:
/// `DCH` on `l` under the page takes it out and the page closes up from its
/// first position.
#[test]
fn preceding_shifts_from_the_extent_start() {
    assert_eq!(
        rows("\x1b[10h\x1b[Q\x1b[2;2H\x1b[P"),
        [".abcdefghi", "jkmnopqrst", "uvwxyz....", ".........."]
    );
}

/// **A write under IRM opens its gap along the extent**: under the page
/// `X` goes in at `i`, which goes on at the next line's start; under
/// PRECEDING the positions before the cursor move back, the first leaving.
#[test]
fn a_write_under_irm_shifts_along_the_extent() {
    assert_eq!(
        rows("\x1b[4h\x1b[Q\x1b[1;9HX"),
        ["abcdefghXi", "jklmnopqrs", "tuvwxyz...", ".........."]
    );
    assert_eq!(
        rows("\x1b[4h\x1b[10h\x1b[Q\x1b[2;2HX"),
        ["bcdefghijk", "lXmnopqrst", "uvwxyz....", ".........."]
    );
    assert_eq!(cursor("\x1b[4h\x1b[10h\x1b[Q\x1b[2;2HX"), (1, 1));
}

/// **`IL` and `DL` under SEE 1 and 2 erase the active line** (Jev,
/// ERASE_ACTIVE, 0.98/0.98): the shifted part is that line alone.
#[test]
fn il_and_dl_under_a_line_or_field_extent_erase_the_active_line() {
    let erased = ["abcdefghij", "..........", "uvwxyz....", ".........."];
    assert_eq!(rows("\x1b[1Q\x1b[2;5H\x1b[L"), erased);
    assert_eq!(rows("\x1b[2Q\x1b[2;5H\x1b[M"), erased);
    assert_eq!(cursor("\x1b[1Q\x1b[2;5H\x1b[L"), (1, 0));
    assert_eq!(
        rows("\x1b[2;5H\x1b[L"),
        ["abcdefghij", "..........", "klmnopqrst", "uvwxyz...."],
        "before any SEE, the region"
    );
}

/// **`IL` and `DL` under SEE 3 shift the area's lines** (Jev's lean,
/// AREA_LINES): an area over rows 1 and 2, so row 3 keeps its place.
#[test]
fn il_and_dl_under_a_qualified_area_shift_its_lines() {
    let guard = "\x1b[2;1H\x1bV\x1b[3;10H\x1bW";
    assert_eq!(
        rows(&format!("{guard}\x1b[4;1Hend\x1b[3Q\x1b[2;1H\x1b[L")),
        ["abcdefghij", "..........", "klmnopqrst", "end......."]
    );
    assert_eq!(
        rows(&format!("{guard}\x1b[4;1Hend\x1b[3Q\x1b[2;1H\x1b[M")),
        ["abcdefghij", "uvwxyz....", "..........", "end......."]
    );
}

/// **A wide character a line end would cut is erased**, both halves.
#[test]
fn a_wide_character_a_line_end_cuts_is_erased() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            "\x1b[1;1Habcdefgh中\x1b[2;1Hklm\x1b[Q\x1b[1;1H\x1b[@".as_bytes(),
        );
        let row = |y: usize| -> String {
            t.viewport().row(Row::from_index(y))[..10]
                .iter()
                .map(|c| match c.content {
                    x if x.is_wide_tail() => '_',
                    x => x
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or('.'),
                })
                .collect()
        };
        assert_eq!(row(0), ".abcdefgh.");
        assert_eq!(row(1), ".klm......");
    });
}

/// **A value past 4 is counted and keeps the extent** (Jev, COUNTED_KEEP,
/// 0.98/0.94); the private namespace is no SEE.
#[test]
fn see_with_an_undefined_value_is_counted() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[Q\x1b[5Q\x1b[?1Q");
        assert_eq!(t.unsupported_param, 1);
        assert_eq!(t.unsupported, 1);
    });
    assert_eq!(
        rows("\x1b[Q\x1b[5Q\x1b[1;9H\x1b[2@")[1],
        "ijklmnopqr",
        "the page still"
    );
}

/// **`RIS` returns to DEC's extent, `DECSTR` keeps it** (Jev, KEEP,
/// 0.96/0.96).
#[test]
fn ris_returns_to_decs_extent_and_decstr_keeps_it() {
    assert_eq!(rows("\x1b[Q\x1b[!p\x1b[1;9H\x1b[2@")[1], "ijklmnopqr");
    support::with_term(&WS, |t| {
        support::feed(t, format!("\x1b[Q\x1bc{TEXT}\x1b[1;9H\x1b[2@").as_bytes());
        let row1: String = t.viewport().row(Row::from_index(1))[..10]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect();
        assert_eq!(row1, "klmnopqrst");
    });
}

/// **DECRQSS does not answer SEE**, as it answers SCP: `DCS 0 $ r ST`.
#[test]
fn decrqss_answers_see_as_invalid() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2Q\x1bP$qQ\x1b\\");
        assert_eq!(t.reply(), b"\x1bP0$r\x1b\\");
    });
}

/// Row `y` after `bytes` alone, as [`rows`] draws it.
fn row_of(bytes: &str, y: usize) -> String {
    support::with_term(&WS, |t| {
        support::feed(t, bytes.as_bytes());
        t.viewport().row(Row::from_index(y))[..10]
            .iter()
            .map(|c| match c.content {
                x if x.is_wide_tail() => '_',
                x => x
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.'),
            })
            .collect()
    })
}

/// **An edit on a wide character's tail takes the whole character**, as
/// DEC's does: `ICH` and a write under IRM open the gap at its lead, and
/// `DCH` removes both halves. Under SEE 0 and SEE 2 the row is the one no
/// SEE leaves, on `a中cy` with the cursor on the tail.
#[test]
fn an_edit_on_a_wide_tail_takes_the_whole_character() {
    let text = "a\u{4e2d}cy\x1b[1;3H";
    let mut wrong = Vec::new();
    for (edit, want) in [
        ("\x1b[@", "a.\u{4e2d}_cy...."),
        ("\x1b[2@", "a..\u{4e2d}_cy..."),
        ("\x1b[P", "acy......."),
        ("\x1b[4hX", "a.X.cy...."),
    ] {
        assert_eq!(row_of(&format!("{text}{edit}"), 0), want, "{edit:?}");
        for see in ["\x1b[Q", "\x1b[2Q"] {
            let got = row_of(&format!("{see}{text}{edit}"), 0);
            if got != want {
                wrong.push(format!("{see:?} {edit:?}: {got}, not {want}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **`DCH` on a wide character's lead removes both halves** under SEE
/// 0, as with no SEE.
#[test]
fn dch_on_a_wide_lead_removes_both_halves() {
    let text = "a\u{4e2d}cy\x1b[1;2H\x1b[P";
    assert_eq!(row_of(text, 0), "acy.......");
    assert_eq!(row_of(&format!("\x1b[Q{text}"), 0), "acy.......");
}

/// **A wide character the shift would carry across a line end is erased**,
/// both halves: `DCH` under the page pulls `中` from row 1's start toward
/// row 0's last column, where its tail would land on row 1.
#[test]
fn a_wide_character_pulled_across_a_line_end_is_erased() {
    let text = "abcdefghi\x1b[2;1H\u{4e2d}z\x1b[Q\x1b[1;1H\x1b[P";
    assert_eq!(row_of(text, 0), "bcdefghi..");
    assert_eq!(row_of(text, 1), ".z........");
}

/// **Under HEM's PRECEDING an edit on either half takes the whole
/// character**: `ICH` shifts `中` toward the line's start, `a` leaving,
/// `DCH` removes both halves, `a` closing up, and a write under IRM goes
/// in after the character, a wide one pushing it off the line's start,
/// before any SEE and under SEE 0, 1 and 2, with the cursor on the lead
/// and on the tail.
#[test]
fn preceding_on_a_wide_character_takes_the_whole_character() {
    let mut wrong = Vec::new();
    for see in ["", "\x1b[Q", "\x1b[1Q", "\x1b[2Q"] {
        for col in [2, 3] {
            for (edit, want) in [
                ("\x1b[@", "中_.cy....."),
                ("\x1b[P", "..acy....."),
                ("\x1b[4hX", "中_Xcy....."),
                ("\x1b[4h一", ".一_cy....."),
            ] {
                let bytes = format!("a中cy{see}\x1b[10h\x1b[1;{col}H{edit}");
                let got = row_of(&bytes, 0);
                if got != want {
                    wrong.push(format!("{see:?} col {col} {edit:?}: {got}, not {want}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **A wide character the shifted part's end cuts is erased**, both
/// halves: under SEE 2 with a stop at column 3 the field is columns 1
/// and 2, and `ICH` pushes `a` onto the lead of the `中` whose tail is
/// past the field.
#[test]
fn a_wide_character_the_field_end_cuts_is_erased() {
    let text = "\x1b[3g\x1b[1;3H\x1bH\x1b[1;1Ha中中cy\x1b[2Q\x1b[1;1H\x1b[@";
    assert_eq!(row_of(text, 0), ".a.中_cy...");
}

/// **`DCH` of two or more on a wide character's tail removes the
/// character and the positions after the cursor**, as with no SEE
/// (`Grid::delete_cells`, ECMA-48 8.3.26): the snap to the lead does not
/// shorten the span; under HEM's PRECEDING the mirror holds on the lead.
#[test]
fn a_long_dch_on_a_wide_half_removes_as_many_positions_as_asked() {
    let mut wrong = Vec::new();
    for (text, n) in [
        ("a\u{4e2d}a\x1b[1;3H", 2),
        ("a\u{4e2d}cy\x1b[1;3H", 2),
        ("a\u{4e2d}cy\x1b[1;3H", 3),
        ("\u{4e2d}\u{4e2d}\x1b[1;2H", 2),
    ] {
        let dch = format!("{text}\x1b[{n}P");
        let plain = row_of(&dch, 0);
        for see in ["\x1b[Q", "\x1b[2Q"] {
            let got = row_of(&format!("{see}{dch}"), 0);
            if got != plain {
                wrong.push(format!("{see:?} {dch:?}: {got} want {plain}"));
            }
        }
    }
    for see in ["", "\x1b[Q", "\x1b[1Q"] {
        let preceding = row_of(&format!("a\u{4e2d}cy{see}\x1b[10h\x1b[1;2H\x1b[2P"), 0);
        if preceding != "...cy....." {
            wrong.push(format!(
                "{see:?} PRECEDING on the lead: {preceding} want ...cy....."
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **Under HEM's PRECEDING the line is the first line of the page**:
/// before any SEE and under SEE 1, `ICH`, `DCH` and a write under IRM
/// leave row 0 as SEE 0 leaves it, on lines of wide characters with the
/// cursor at every column, so a character the shift parts at the line's
/// start is erased whole and one at the cursor moves whole; and with
/// DECLRMM's margins at columns 3 and 9, where a wide character can
/// straddle either, with the cursor inside them.
#[test]
fn preceding_on_the_line_is_the_page_on_its_first_line() {
    let mut wrong = Vec::new();
    let margins = "\x1b[?69h\x1b[3;9s";
    let lines = ["a中cy", "a中中cy", "中中中中中", "ab中c中d", "中a中"]
        .map(|t| (t, "", 1..=10))
        .into_iter()
        .chain(["a中中中中c", "中中中中中", "ab中cd中"].map(|t| (t, margins, 3..=9)));
    for (text, set, cols) in lines {
        for col in cols {
            for edit in [
                "\x1b[@",
                "\x1b[2@",
                "\x1b[3@",
                "\x1b[P",
                "\x1b[2P",
                "\x1b[3P",
                "\x1b[4hX",
                "\x1b[4h一",
            ] {
                let at =
                    |see: &str| row_of(&format!("{text}{set}{see}\x1b[10h\x1b[1;{col}H{edit}"), 0);
                let page = at("\x1b[Q");
                for see in ["", "\x1b[1Q"] {
                    let got = at(see);
                    if got != page {
                        wrong.push(format!(
                            "{text} {see:?} col {col} {edit:?}: {got}, page {page}"
                        ));
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{} differ: {wrong:#?}", wrong.len());
}
