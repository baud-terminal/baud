//! `Video`, the one spelling of what `?5` DECSCNM does to colour: every
//! cell's foreground and background exchanged (`[[source: dec-std-070]]` §
//! 5.6.3.2), and which dynamic colour is shown in each role -- read by the
//! renderer to draw and by the core to set, query and reset.

use baud_domain::{Attrs, DynamicColor, Video};

/// The pair is exchanged when exactly one of `?5` and the cell's `SGR 7`
/// reverses it, and kept when neither or both do. No other attribute
/// enters.
#[test]
fn a_cell_is_exchanged_by_exactly_one_reversal() {
    let plain = Attrs::NONE;
    let rev = Attrs::NONE.with(Attrs::REVERSE);
    assert_eq!(Video::Normal.drawn(plain, 'f', 'b'), ('f', 'b'));
    assert_eq!(Video::Normal.drawn(rev, 'f', 'b'), ('b', 'f'));
    assert_eq!(Video::Reverse.drawn(plain, 'f', 'b'), ('b', 'f'));
    assert_eq!(Video::Reverse.drawn(rev, 'f', 'b'), ('f', 'b'));
    for other in [Attrs::BOLD, Attrs::DIM, Attrs::HIDDEN, Attrs::BLINK] {
        let a = Attrs::NONE.with(other);
        assert_eq!(Video::Reverse.drawn(a, 'f', 'b'), ('b', 'f'), "{other:?}");
        assert_eq!(Video::Normal.drawn(a, 'f', 'b'), ('f', 'b'), "{other:?}");
    }
}

#[test]
fn each_role_mapping_undoes_itself() {
    for v in [Video::Normal, Video::Reverse] {
        for w in [
            DynamicColor::Foreground,
            DynamicColor::Background,
            DynamicColor::Cursor,
        ] {
            assert_eq!(v.role(v.role(w)), w);
        }
    }
}

/// The text and the background trade places; the cursor colour is its own,
/// as xterm's `oppositeColor` leaves `TEXT_CURSOR` alone
/// (`[[source: xterm-misc]]` 3527-3566).
#[test]
fn reverse_swaps_the_text_and_the_background_and_keeps_the_cursor() {
    use DynamicColor::*;
    assert_eq!(Video::Reverse.role(Foreground), Background);
    assert_eq!(Video::Reverse.role(Background), Foreground);
    assert_eq!(Video::Reverse.role(Cursor), Cursor);
    for w in [Foreground, Background, Cursor] {
        assert_eq!(Video::Normal.role(w), w);
    }
}
