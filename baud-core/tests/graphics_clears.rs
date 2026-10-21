//! **What clearing the whole screen does to the pictures on it**: DECCOLM's
//! clear and the alternate screen's, read back from the image store. The
//! VT330 and VT340 keep text and graphics in one bitmap, DECCOLM "causes
//! the display data to be cleared" (`[[source: dec-std-070]]` p. 5-71),
//! and xterm's `ClearScreen` clears the graphics over every row, so a Sixel
//! goes and the ReGIS page loses its pixels, as under ED 2.
//!
//! Cells are 10 by 20 pixels throughout.

use baud_core::*;
use baud_domain::{ImageBudget, ImagePx, Pixel};

mod support;

/// A 40 by 36 Sixel at 1:1, every pixel register 1's red.
const RED: &[u8] = b"\x1bP9q#1;2;100;0;0#1!40~-!40~-!40~-!40~-!40~-!40~\x1b\\";

/// A ReGIS fill over the page's upper-left corner.
const FILL: &[u8] = b"\x1bPpF(V[0,0][96,0][96,48][0,48])\x1b\\";

/// A 1 by 1 kitty image, placed at the cursor.
const KITTY: &[u8] = b"\x1b_Ga=T,f=32,s=1,v=1;AAAA/w==\x1b\\";

fn term_run(bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    support::feed(&mut t, bytes);
    f(&mut t);
}

/// How many placements are live.
fn placed(t: &Term<'_>) -> usize {
    t.images().placements().count()
}

/// The pixels drawn on the images placed, all of them.
fn lit(t: &Term<'_>) -> usize {
    let imgs = t.images();
    imgs.placements()
        .filter_map(|(_, p)| imgs.image(p.image))
        .map(|v| {
            v.pixels
                .iter()
                .filter(|px| px.bits() != Pixel::CLEAR.bits())
                .count()
        })
        .sum()
}

/// **DECCOLM's clear takes the Sixel with it**, set or reset, as ED 2
/// does; under `?95` the screen is not cleared and the Sixel stays, the
/// control.
#[test]
fn deccolm_s_clear_frees_a_sixel() {
    for (mode, what) in [(&b"\x1b[?3h"[..], "set"), (b"\x1b[?3l", "reset")] {
        term_run(b"\x1b[?40h\x1b[H", |t| {
            support::feed(t, RED);
            assert_eq!(placed(t), 1, "{what}: the control, drawn");
            support::feed(t, mode);
            assert_eq!(placed(t), 0, "{what}: cleared with the screen");
        });
        term_run(b"\x1b[?40h\x1b[?95h\x1b[H", |t| {
            support::feed(t, RED);
            support::feed(t, mode);
            assert_eq!(placed(t), 1, "{what}: ?95 keeps the screen, and it");
        });
    }
}

/// **DECCOLM's clear leaves the ReGIS page with nothing drawn**, and the
/// control without it keeps the fill.
#[test]
fn deccolm_s_clear_clears_the_regis_page() {
    term_run(b"\x1b[?40h", |t| {
        support::feed(t, FILL);
        assert!(lit(t) > 0, "the control, drawn");
        support::feed(t, b"\x1b[?3h");
        assert_eq!(lit(t), 0, "cleared with the screen");
    });
}

/// **`?1049 h` on the alternate screen clears it, the Sixel there too**:
/// xterm's set arm is `CursorSave; ToAlternate; ClearScreen`. A Sixel the
/// `?47` screen holds goes; `?47 h` again, which clears nothing, keeps it.
#[test]
fn the_1049_clear_on_the_alternate_screen_frees_its_sixel() {
    term_run(b"\x1b[?47h\x1b[H", |t| {
        support::feed(t, RED);
        support::feed(t, b"\x1b[?47h");
        assert_eq!(placed(t), 1, "?47 h again keeps it");
        support::feed(t, b"\x1b[?1049h");
        assert_eq!(placed(t), 0, "?1049 h clears it");
    });
}

/// **The primary's pictures wait behind the alternate screen**: entering
/// it with `?1049` clears the alternate screen and nothing on the
/// primary, which shows its Sixel again on the way back.
#[test]
fn entering_the_alternate_screen_leaves_the_primary_s_sixel() {
    term_run(b"\x1b[H", |t| {
        support::feed(t, RED);
        let before = lit(t);
        support::feed(t, b"\x1b[?1049h\x1b[?1049l");
        assert_eq!(placed(t), 1, "still placed");
        assert_eq!(lit(t), before, "every pixel kept");
    });
    term_run(b"\x1b[H", |t| {
        support::feed(t, KITTY);
        support::feed(t, b"\x1b[?1049h\x1b[?1049l");
        assert_eq!(placed(t), 1, "the primary's kitty image kept");
    });
}

/// **DECCOLM takes the kitty placements on the screen**, as ED 2 does and
/// as kitty's DECCOLM, which runs its ED 2, does; `?95`'s control keeps
/// it.
#[test]
fn deccolm_s_clear_deletes_a_kitty_placement() {
    term_run(b"\x1b[?40h\x1b[H", |t| {
        support::feed(t, KITTY);
        assert_eq!(placed(t), 1, "the control, placed");
        support::feed(t, b"\x1b[?3h");
        assert_eq!(placed(t), 0, "cleared with the screen");
    });
    term_run(b"\x1b[?40h\x1b[?95h\x1b[H", |t| {
        support::feed(t, KITTY);
        support::feed(t, b"\x1b[?3h");
        assert_eq!(placed(t), 1, "?95 keeps it");
    });
}
