//! **The side margins an embedder reads** (`Term::side_margins`): the
//! columns `DECSLRM` (`CSI Pl ; Pr s` under `DECLRMM`, `?69`) set, and the
//! screen's own edges when none are set, as `Term::scroll_region` gives
//! the rows `DECSTBM` set.
mod support;

use baud_core::*;
use baud_domain::{BytePos, Col, ReadLen};

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 4,
};

fn margins(bytes: &[u8]) -> (Col, Col) {
    support::with_term(&WS, |t| {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        t.side_margins()
    })
}

/// **The margins `DECSLRM` set, and the screen's edges otherwise**: none
/// set, the whole row; `?69h` and `CSI 3 ; 10 s`, columns 2 to 9; `?69l`
/// after, the whole row again; `CSI 3 ; 10 s` without `?69h`, which is
/// `SCOSC`, the whole row. Named break: answer the screen's edges always --
/// the set margins read as the whole row.
#[test]
fn the_side_margins_are_what_decslrm_set() {
    let (first, last) = (Col::at_index(0), Col::at_index(19));
    assert_eq!(margins(b""), (first, last), "none set");
    assert_eq!(
        margins(b"\x1b[?69h\x1b[3;10s"),
        (Col::at_index(2), Col::at_index(9)),
        "set"
    );
    assert_eq!(
        margins(b"\x1b[?69h\x1b[3;10s\x1b[?69l"),
        (first, last),
        "?69l"
    );
    assert_eq!(margins(b"\x1b[3;10s"), (first, last), "SCOSC, not DECSLRM");
}
