//! `Video`, the one spelling of xterm's `ReverseVideo` colour swap
//! (`[[source: xterm-scroll]]` util.c 3286-3294): which palette entries and
//! which dynamic colours trade places under `?5`, read by the renderer to
//! draw and by the core to set, query and reset (note V, V17).

use baud_domain::{Chan, DynamicColor, Video};

#[test]
fn reverse_swaps_entries_0_and_7_and_8_and_15_and_no_other() {
    for i in 0..=255u8 {
        let want = match i {
            0 => 7,
            7 => 0,
            8 => 15,
            15 => 8,
            n => n,
        };
        assert_eq!(Video::Reverse.entry(Chan::from_u8(i)).get(), want, "{i}");
        assert_eq!(Video::Normal.entry(Chan::from_u8(i)).get(), i, "{i}");
    }
}

#[test]
fn each_mapping_undoes_itself() {
    for v in [Video::Normal, Video::Reverse] {
        for i in 0..=255u8 {
            let c = Chan::from_u8(i);
            assert_eq!(v.entry(v.entry(c)), c);
        }
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
