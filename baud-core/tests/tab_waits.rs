//! **The stops from a wrap waiting at the line's end**: a character
//! written at the last column, or at a DECSLRM right margin, with DECAWM
//! set leaves the active position on that column, "the Active Position
//! cannot be advanced" (`[[source: dec-std-070]]` D.6.1), as ECMA-48's
//! implicit movement moves the active position on only when it "is not
//! the last character position of the active line" (`[[source: ecma48]]`
//! 6.1.6). So `HTS` and `TBC 0` set and clear the stop "at the active
//! presentation position" (8.3.62, 8.3.154) on that column, and `CBT`
//! moves to "the n-th preceding character tabulation stop" (8.3.7) from
//! it, as xterm's `TabSet()`, `TabClear()` and `TabPrev()` take the cursor
//! column, which stays there while the wrap waits
//! (`[[source: xterm-reset]]` 4304 and 5458, `[[source: xterm-tabs-c]]`).
//! `HTS` is not among the controls D.6.1 lists as ending the wait, so the
//! wrap still waits.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

const TWENTY: &[u8] = b"abcdefghijklmnopqrst";

/// What `bytes` and then `query` answer, and whether a wrap waits after
/// `bytes`.
fn answer(bytes: &[u8], query: &[u8]) -> (String, WrapPending) {
    support::render(&WS, bytes, |t| {
        let w = t.wrap_pending();
        support::feed(t, query);
        (String::from_utf8_lossy(t.reply()).into_owned(), w)
    })
}

/// Every case whose answer is not the one it names, each named.
fn wrong(cases: &[(&[u8], &[u8], &str, WrapPending)]) -> Vec<String> {
    cases
        .iter()
        .filter(|(b, q, want, w)| answer(b, q) != ((*want).into(), *w))
        .map(|(b, q, want, w)| {
            let got = answer(b, q);
            format!(
                "{:?} then {:?}: {:?} {:?}, not {want:?} {w:?}",
                String::from_utf8_lossy(b),
                String::from_utf8_lossy(q),
                got.0,
                got.1
            )
        })
        .collect()
}

const TABS: &[u8] = b"\x1b[2$w";
const CPR: &[u8] = b"\x1b[6n";

/// **`HTS` and `TBC 0` at a wrap waiting at the last column or at a
/// DECSLRM right margin set and clear the stop on that column**, and the
/// wrap still waits after `HTS`: the stops cleared, twenty letters, `HTS`,
/// and DECTABSR names position 20; margins 1 to 10, ten letters, position
/// 10; under BDSM reset, the cursor's presentation position, position 20.
/// `TBC 0` with a stop at 20, or at 10 under the margins, leaves the
/// default stops. The controls: an `HTS` from position 19, which is no
/// wait; one from a cursor a `CUP` placed one past the margin, at 11, a
/// column and no wait; and one after DECAWM is reset, the wait gone and
/// the column the report names.
#[test]
fn hts_and_tbc_at_a_wrap_waiting_at_the_lines_end_name_its_column() {
    let yes = WrapPending::Yes;
    let no = WrapPending::No;
    let cases: &[(&[u8], &[u8], &str, WrapPending)] = &[
        (
            &[b"\x1b[3g", TWENTY, b"\x1bH"].concat(),
            TABS,
            "\x1bP2$u20\x1b\\",
            yes,
        ),
        (
            &[b"\x1b[3g", TWENTY, b"\x1b[0W"].concat(),
            TABS,
            "\x1bP2$u20\x1b\\",
            yes,
        ),
        (
            &[b"\x1b[3g\x1b[?69h\x1b[1;10s", &TWENTY[..10], b"\x1bH"].concat(),
            TABS,
            "\x1bP2$u10\x1b\\",
            yes,
        ),
        (
            &[b"\x1b[3g\x1b[8l", TWENTY, b"\x1bH"].concat(),
            TABS,
            "\x1bP2$u20\x1b\\",
            yes,
        ),
        (
            &[b"\x1b[1;20H\x1bH\x1b[1;1H", TWENTY, b"\x1b[g"].concat(),
            TABS,
            "\x1bP2$u9/17\x1b\\",
            yes,
        ),
        (
            &[
                b"\x1b[1;10H\x1bH\x1b[?69h\x1b[1;10s",
                &TWENTY[..10],
                b"\x1b[g",
            ]
            .concat(),
            TABS,
            "\x1bP2$u9/17\x1b\\",
            yes,
        ),
        // The controls: no wait, the cursor's own column.
        (
            &[b"\x1b[3g", &TWENTY[..19], b"\x1bH"].concat(),
            TABS,
            "\x1bP2$u20\x1b\\",
            no,
        ),
        (
            b"\x1b[3g\x1b[?69h\x1b[1;10s\x1b[1;11H\x1bH",
            TABS,
            "\x1bP2$u11\x1b\\",
            no,
        ),
        (
            &[b"\x1b[3g", TWENTY, b"\x1b[?7l\x1bH"].concat(),
            TABS,
            "\x1bP2$u20\x1b\\",
            no,
        ),
    ];
    let bad = wrong(cases);
    assert!(
        bad.is_empty(),
        "{} of {}:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

/// **`CBT` from a wrap waiting at the last column or at a DECSLRM right
/// margin counts from that column**: a stop there, and `CBT` goes to the
/// stop before it, `CBT 2` to the one before that, under BDSM reset as
/// well; and `HT`, `CHT` and `CHT 2`, which find no stop after it, stay on
/// it with the wait ended. The controls: `CBT` from position 19; from a
/// cursor a `CUP` placed one past the margin, at 11, which counts from 11
/// to the stop on the margin;
/// after DECAWM is reset, from the column the report names; and with
/// `?41` set, `HT` and `CHT` from the wait take the wrap first, as xterm's
/// `TabNext()` does.
#[test]
fn cbt_from_a_wrap_waiting_at_the_lines_end_counts_from_its_column() {
    let no = WrapPending::No;
    const LAST: &[u8] = b"\x1b[1;20H\x1bH\x1b[1;1Habcdefghijklmnopqrst";
    const MARGIN: &[u8] = b"\x1b[1;10H\x1bH\x1b[?69h\x1b[1;10sabcdefghij";
    let cases: &[(&[u8], &[u8], &str, WrapPending)] = &[
        (&[LAST, b"\x1b[Z"].concat(), CPR, "\x1b[1;17R", no),
        (&[LAST, b"\x1b[2Z"].concat(), CPR, "\x1b[1;9R", no),
        (
            &[b"\x1b[8l", LAST, b"\x1b[Z"].concat(),
            CPR,
            "\x1b[1;17R",
            no,
        ),
        (&[MARGIN, b"\x1b[Z"].concat(), CPR, "\x1b[1;9R", no),
        (&[MARGIN, b"\x1b[2Z"].concat(), CPR, "\x1b[1;1R", no),
        (&[LAST, b"\t"].concat(), CPR, "\x1b[1;20R", no),
        (&[LAST, b"\x1b[I"].concat(), CPR, "\x1b[1;20R", no),
        (&[LAST, b"\x1b[2I"].concat(), CPR, "\x1b[1;20R", no),
        (&[MARGIN, b"\t"].concat(), CPR, "\x1b[1;10R", no),
        (&[MARGIN, b"\x1b[I"].concat(), CPR, "\x1b[1;10R", no),
        // The controls: no wait, the stop before the cursor's column.
        (
            b"\x1b[1;20H\x1bH\x1b[1;1Habcdefghijklmnopqrs\x1b[Z",
            CPR,
            "\x1b[1;17R",
            no,
        ),
        (
            b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;10H\x1bH\x1b[?69h\x1b[1;10s\x1b[1;11H\x1b[Z",
            CPR,
            "\x1b[1;10R",
            no,
        ),
        (&[LAST, b"\x1b[?7l\x1b[Z"].concat(), CPR, "\x1b[1;17R", no),
        (&[b"\x1b[?41h", LAST, b"\t"].concat(), CPR, "\x1b[2;9R", no),
        (
            &[b"\x1b[?41h", LAST, b"\x1b[I"].concat(),
            CPR,
            "\x1b[2;9R",
            no,
        ),
    ];
    let bad = wrong(cases);
    assert!(
        bad.is_empty(),
        "{} of {}:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}
