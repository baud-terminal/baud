//! **baud's compiled xterm-256color entry is tic's, byte for byte.**
//! [`XTERM_256COLOR`] is held to `tests/tic/xterm-256color-baud`, the file
//! `tic -x -o <dir>` (ncurses 6.0.20150808) writes for the pinned entry
//! with `Se` set to `\E[0 q`. `tests/tic/xterm-256color-pinned` is the same
//! compiler's file for the pinned entry as it is; the two differ in the
//! one byte of `Se`'s value, which shows the comparison sees a change of
//! one byte.

use baud_domain::terminfo::compiled::{BOOLEANS, NUMBERS, STRINGS, XTERM_256COLOR};

/// tic's compile of baud's entry.
const TIC: &[u8] = include_bytes!("tic/xterm-256color-baud");

/// tic's compile of the pinned entry, `Se` unchanged.
const TIC_PINNED: &[u8] = include_bytes!("tic/xterm-256color-pinned");

/// Where `Se`'s value, `\E[0 q`, starts in both files.
const SE_AT: usize = 2710;

#[test]
fn baud_s_entry_is_tic_s_byte_for_byte() {
    let ours: &[u8] = XTERM_256COLOR;
    if let Some(at) = ours.iter().zip(TIC).position(|(a, b)| a != b) {
        panic!(
            "byte {at} is {:#04x} here and {:#04x} in tic's",
            ours[at], TIC[at]
        );
    }
    assert_eq!(ours.len(), TIC.len());
    assert_eq!(TIC.len(), 3402);
}

#[test]
fn the_comparison_sees_se_s_one_byte() {
    assert_eq!(&TIC[SE_AT..SE_AT + 6], b"\x1b[0 q\0");
    assert_eq!(&TIC_PINNED[SE_AT..SE_AT + 6], b"\x1b[2 q\0");
    let differ: Vec<usize> = TIC
        .iter()
        .zip(TIC_PINNED)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(differ, [SE_AT + 2]);
    assert_eq!(TIC.len(), TIC_PINNED.len());
    assert_ne!(&XTERM_256COLOR[..], TIC_PINNED);
}

#[test]
fn the_capabilities_are_in_term_h_s_order() {
    assert_eq!(
        (BOOLEANS.len(), NUMBERS.len(), STRINGS.len()),
        (44, 39, 414)
    );
    assert_eq!(
        (
            BOOLEANS[0],
            BOOLEANS[37],
            NUMBERS[13],
            STRINGS[0],
            STRINGS[413]
        ),
        ("bw", "OTbs", "colors", "cbt", "box1")
    );
}
