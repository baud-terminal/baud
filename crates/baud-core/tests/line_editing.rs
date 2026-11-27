//! **VEM, `CSI 7 h` / `CSI 7 l`** (`[[source: ecma48]]` 7.2.19): whether
//! a line insertion or deletion shifts the lines after the active line or
//! before it. RM 7 selects FOLLOWING, the state at power-on, and SM 7
//! PRECEDING. Under PRECEDING `IL` pushes the active line and the ones
//! before it toward the top, and `DL` removes the active line and the ones
//! before it and closes the gap from the top. Either way the cursor goes to
//! the line home of the active line.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 3,
};

/// `a` to `f`, one to a row, with nothing after `f` so nothing scrolls.
const FILL: &str = "a\r\nb\r\nc\r\nd\r\ne\r\nf";

/// Every row of the screen, `.` for an empty cell, rows joined by spaces.
fn lines(t: &Term<'_>) -> String {
    (0..usize::from(WS.ws_row))
        .map(|y| {
            t.viewport().row(Row::from_index(y))[..usize::from(WS.ws_col)]
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or('.')
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `FILL`, the cursor on `d`'s row at column 2, then `then`: the screen and
/// the cursor.
fn edited(then: &str) -> (String, (Row, Col)) {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{FILL}\x1b[4;3H{then}").as_bytes());
        (lines(t), t.cursor())
    })
}

/// The line home of `d`'s row.
fn home() -> (Row, Col) {
    (Row::from_index(3), Col::ZERO)
}

/// **`IL` under PRECEDING pushes the active line and the ones before it
/// toward the top** (7.2.19: "a line insertion causes the contents of the
/// active line ... and of the preceding lines to be shifted in the
/// direction opposite to that of the line progression"), the first `n`
/// leaving the screen, the active line and the `n - 1` before it erased
/// (8.3.67: "the n-1 preceding or following lines"); the cursor goes to
/// the line home. Under FOLLOWING, the state at power-on, the lines after
/// it. With DECSTBM the top margin bounds the shift.
#[test]
fn il_under_preceding_pushes_toward_the_top() {
    assert_eq!(
        edited("\x1b[7h\x1b[L"),
        ("b.. c.. d.. ... e.. f..".into(), home())
    );
    assert_eq!(
        edited("\x1b[7h\x1b[2L"),
        ("c.. d.. ... ... e.. f..".into(), home())
    );
    assert_eq!(
        edited("\x1b[7h\x1b[9L"),
        ("... ... ... ... e.. f..".into(), home())
    );
    assert_eq!(edited("\x1b[L"), ("a.. b.. c.. ... d.. e..".into(), home()));
    assert_eq!(
        edited("\x1b[2;5r\x1b[4;3H\x1b[7h\x1b[L"),
        ("a.. c.. d.. ... e.. f..".into(), home()),
        "the top margin bounds PRECEDING as the bottom one bounds FOLLOWING"
    );
}

/// **`DL` under PRECEDING removes the active line and the `n - 1` before it
/// and closes the gap from the top** (7.2.19: "a line deletion causes the
/// contents of the lines preceding the active line to be shifted in the
/// direction of the line progression"; 8.3.32: "At the other end of the
/// shifted part, n lines are put into the erased state"); the cursor goes
/// to the line home. Under FOLLOWING the lines after it close the gap.
/// With DECSTBM the top margin bounds the shift.
#[test]
fn dl_under_preceding_closes_the_gap_from_the_top() {
    assert_eq!(
        edited("\x1b[7h\x1b[M"),
        ("... a.. b.. c.. e.. f..".into(), home())
    );
    assert_eq!(
        edited("\x1b[7h\x1b[2M"),
        ("... ... a.. b.. e.. f..".into(), home())
    );
    assert_eq!(
        edited("\x1b[7h\x1b[9M"),
        ("... ... ... ... e.. f..".into(), home())
    );
    assert_eq!(
        edited("\x1b[2M"),
        ("a.. b.. c.. f.. ... ...".into(), home())
    );
    assert_eq!(
        edited("\x1b[2;5r\x1b[4;3H\x1b[7h\x1b[M"),
        ("a.. ... b.. c.. e.. f..".into(), home()),
        "the top margin bounds PRECEDING as the bottom one bounds FOLLOWING"
    );
}

/// **`CSI 7 l` returns `IL` and `DL` to FOLLOWING** (7.2.19, FOLLOWING: "a
/// line insertion causes the contents of the active line ... and of the
/// following lines ... to be shifted in the direction of the line
/// progression"): one edit under PRECEDING, then RM 7, then the same edit
/// on the same row shifts the lines after it.
#[test]
fn rm_7_returns_il_and_dl_to_following() {
    assert_eq!(
        edited("\x1b[7h\x1b[L\x1b[4;3H\x1b[7l\x1b[L"),
        ("b.. c.. d.. ... ... e..".into(), home())
    );
    assert_eq!(
        edited("\x1b[7h\x1b[M\x1b[4;3H\x1b[7l\x1b[M"),
        ("... a.. b.. e.. f.. ...".into(), home())
    );
    assert_eq!(edited("\x1b[7h\x1b[7l\x1b[2L"), edited("\x1b[2L"));
}

/// **`DECRQM` answers VEM** (7.2.19, "FOLLOWING" and "PRECEDING"): reset,
/// `2`, at power-on; set, `1`, after `SM 7`. `RIS` resets it and `DECSTR`
/// keeps it, as it keeps HEM.
#[test]
fn decrqm_answers_vem() {
    let reply = |then: &str| {
        support::with_term(&WS, |t| {
            support::feed(t, format!("{then}\x1b[7$p").as_bytes());
            String::from_utf8_lossy(t.reply()).into_owned()
        })
    };
    assert_eq!(reply(""), "\x1b[7;2$y");
    assert_eq!(reply("\x1b[7h"), "\x1b[7;1$y");
    assert_eq!(reply("\x1b[7h\x1b[7l"), "\x1b[7;2$y");
    assert_eq!(reply("\x1b[7h\x1bc"), "\x1b[7;2$y");
    assert_eq!(reply("\x1b[7h\x1b[!p"), "\x1b[7;1$y");
}

/// **`RIS` returns `IL` and `DL` to FOLLOWING** (7.2.19, FOLLOWING, the
/// state 7.1 recommends at reset): SM 7, then `RIS`, then the screen
/// refilled, inserts as at power-on; SM 7, then `DECSTR`, still inserts
/// under PRECEDING.
#[test]
fn ris_returns_line_editing_to_following() {
    assert_eq!(
        edited(&format!("\x1b[7h\x1bc{FILL}\x1b[4;3H\x1b[2L")),
        ("a.. b.. c.. ... ... d..".into(), home())
    );
    assert_eq!(
        edited("\x1b[7h\x1b[!p\x1b[4;3H\x1b[2L"),
        ("c.. d.. ... ... e.. f..".into(), home())
    );
}
