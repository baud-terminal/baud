//! **A wide character on a line's last column, with `DECAWM` reset, writes
//! its lead alone there**: no tail past the right margin, a double line's
//! half or SLL's limit, which are positions no write may reach, "the Active
//! Position will not advance beyond the Right Margin or last column of the
//! Active Line" (`[[source: dec-std-070]]` p. 5-139), and on a double line
//! "character positions cannot be addressed which are more than half the
//! current value of the right edge of the display" (§ 5.6.1, p. 5-95). No
//! text gives a character two positions there: ECMA-48 6.1.6's NOTE leaves
//! the move to the implementation, and the VT382's DECAWM
//! (`[[source: vt382-rm]]` p. 10-4) has characters "replace characters
//! already on the screen". At the screen's edge the lead stood alone
//! already, and the other three lines are the same.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

const WIDE: &str = "\u{4e00}";

/// Row `y` as text: `W` the wide character's lead, `>` a tail, `.` an
/// empty position.
fn row(t: &Term<'_>, y: usize) -> String {
    t.viewport().row(Row::from_index(y))[..20]
        .iter()
        .map(|c| match c.content {
            x if x.is_wide_tail() => '>',
            x => x
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .map(|ch| if ch == '\u{4e00}' { 'W' } else { ch })
                .unwrap_or('.'),
        })
        .collect()
}

/// Rows 0 and 1, the cursor, and the cells written.
fn after(s: &str) -> (String, String, (usize, usize), usize) {
    support::render(&WS, s.as_bytes(), |t| {
        let (r, c) = t.cursor();
        (
            row(t, 0),
            row(t, 1),
            (r.index(), c.index()),
            t.amplification().1,
        )
    })
}

/// **On a double line the lead stands alone on the half's last column**:
/// `CUP 1;10` then `U+4E00` wrote its tail on 11, in the hidden half, and
/// `DECSWL` then showed the whole character in 10 and 11. Under SLL past
/// the half too.
#[test]
fn on_a_double_line_s_last_column_the_lead_stands_alone() {
    let (r0, _, cur, _) = after(&format!("\x1b[?7l\x1b#6\x1b[1;10H{WIDE}\x1b#5"));
    assert_eq!(r0, ".........W..........");
    assert_eq!(cur, (0, 9));
    let (r0, ..) = after(&format!("\x1b[?7l\x1b#6\x1b[15 V\x1b[1;10H{WIDE}\x1b#5"));
    assert_eq!(r0, ".........W..........", "SLL past the half");
    let (r0, ..) = after(&format!("\x1b[?7l\x1b#6\x1b[5 V\x1b[1;10H{WIDE}\x1b#5"));
    assert_eq!(r0, ".........W..........", "a cursor past SLL's limit");
}

/// **Every writer on the double line**: a run of three from 8 puts the
/// first whole on 8 and 9 and the last's lead alone on 10, where the tail
/// went to 11; `REP` repeats the character and the rule; under IRM the
/// gap is the one column the lead takes.
#[test]
fn every_writer_keeps_the_double_line_s_half() {
    let mut wrong = Vec::new();
    for (what, s) in [
        ("a run", format!("\x1b[1;8H{WIDE}{WIDE}{WIDE}")),
        ("REP", format!("\x1b[1;8H{WIDE}\x1b[2b")),
        ("IRM", format!("\x1b[4h\x1b[1;8H{WIDE}{WIDE}")),
    ] {
        let (r0, _, cur, _) = after(&format!("\x1b[?7l\x1b#6{s}\x1b#5"));
        if (r0.as_str(), cur) != (".......W>W..........", (0, 9)) {
            wrong.push(format!("{what}: {r0} {cur:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **At the right margin the column past it keeps what it held**: under
/// `DECSLRM 1;10` the tail was written on 11, over the `k` outside the
/// margins. `CUB 1` then goes to 9, from the margin the cursor stayed on.
#[test]
fn at_the_right_margin_nothing_is_written_past_it() {
    let set = "abcdefghijklmnopqrst\x1b[?7l\x1b[?69h\x1b[1;10s\x1b[1;10H";
    let (r0, _, cur, _) = after(&format!("{set}{WIDE}"));
    assert_eq!(r0, "abcdefghiWklmnopqrst");
    assert_eq!(cur, (0, 9));
    let (r0, ..) = after(&format!("{set}{WIDE}\x1b[Dx"));
    assert_eq!(r0, "abcdefghxWklmnopqrst");
}

/// **At SLL's limit the column past it keeps what it held**: limit 10,
/// and the tail was written on 11.
#[test]
fn at_the_line_limit_nothing_is_written_past_it() {
    let (r0, _, cur, _) = after(&format!(
        "abcdefghijklmnopqrst\x1b[?7l\x1b[10 V\x1b[1;10H{WIDE}"
    ));
    assert_eq!(r0, "abcdefghiWklmnopqrst");
    assert_eq!(cur, (0, 9));
}

/// **The cells written count the lead alone**: at the screen's edge the
/// grid dropped the tail and the count took two.
#[test]
fn the_cells_written_count_the_lead_alone() {
    let (r0, _, _, cells) = after(&format!("\x1b[?7l\x1b[1;20H{WIDE}"));
    assert_eq!(r0, "...................W");
    assert_eq!(cells, 1);
}

/// **With `DECAWM` set each wraps the character whole** to the next line,
/// the control: a double line, the right margin, the line limit and the
/// screen.
#[test]
fn with_autowrap_set_the_character_wraps_whole() {
    for (what, s) in [
        ("a double line", format!("\x1b#6\x1b[1;10H{WIDE}")),
        (
            "the right margin",
            format!("\x1b[?69h\x1b[1;10s\x1b[1;10H{WIDE}"),
        ),
        ("the line limit", format!("\x1b[10 V\x1b[1;10H{WIDE}")),
        ("the screen", format!("\x1b[1;20H{WIDE}")),
    ] {
        let (r0, r1, cur, _) = after(&s);
        assert_eq!(r0, "....................", "{what}");
        assert_eq!(r1, "W>..................", "{what}");
        assert_eq!(cur, (1, 2), "{what}");
    }
}

/// **A line home and limit one position apart hold no wide character**:
/// with `DECAWM` set it wraps, and on the next line's one position the
/// lead stands alone, where the tail went past the limit.
#[test]
fn on_a_line_of_one_position_the_lead_stands_alone() {
    let (_, r1, cur, _) = after(&format!("\x1b[5 U\x1b[5 V\x1b[1;5H{WIDE}"));
    assert_eq!(r1, "....W...............");
    assert_eq!(cur.0, 1);
}

/// **The three-byte cell table's read keeps the same columns**: under the
/// `cell-table` feature `U+4E00` is looked up rather than computed, and its
/// tail went past the half and the margin as the computing path's did.
#[test]
fn the_table_read_keeps_the_same_columns() {
    let mut wrong = Vec::new();
    for (s, want) in [
        (
            format!("\x1b[?7l\x1b#6\x1b[1;10H{WIDE}\x1b#5"),
            ".........W..........",
        ),
        (
            format!("abcdefghijklmnopqrst\x1b[?7l\x1b[?69h\x1b[1;10s\x1b[1;10H{WIDE}"),
            "abcdefghiWklmnopqrst",
        ),
    ] {
        let mut owned = Owned::with_cell_table(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        support::feed(&mut t, s.as_bytes());
        if row(&t, 0) != want {
            wrong.push(row(&t, 0));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
