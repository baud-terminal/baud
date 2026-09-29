//! `DECSTR` and `RIS` clear every `OSC 4` override and keep `OSC 10`/`11`/`12`.
//!
//! `[[source: xterm-reset]]`'s `ReallyReset` calls `ResetAnsiColorRequest` in
//! its common path, so both resets return every palette entry to the table,
//! and nothing in either path resets the dynamic colours. baud kept every
//! override across both, so a program that recolours the sixteen and is reset
//! left its colours on everything drawn after it.

mod support;

use baud_core::*;

/// Every override the test sets: two of the sixteen, one of the cube, the
/// last grey.
const SET: &[u8] = b"\x1b]4;1;rgb:12/34/56\x07\x1b]4;9;rgb:ab/cd/ef\x07\
\x1b]4;100;rgb:01/02/03\x07\x1b]4;255;rgb:fe/fd/fc\x07";

/// The dynamic colours the test sets, and the three it expects kept.
const DYN: &[u8] = b"\x1b]10;rgb:11/22/33\x07\x1b]11;rgb:44/55/66\x07\x1b]12;rgb:77/88/99\x07";

const DECSTR: &[u8] = b"\x1b[!p";
const RIS: &[u8] = b"\x1bc";

/// Feed `bytes` at read sizes 1, 3, 7 and whole, each on a fresh terminal.
fn each_read_size(bytes: &[u8], f: impl Fn(&Term<'_>)) {
    let ws = support::CORPUS;
    for size in [1, 3, 7, bytes.len()] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for chunk in bytes.chunks(size.max(1)) {
            support::feed(&mut t, chunk);
        }
        f(&t);
    }
}

fn overrides(t: &Term<'_>) -> Vec<u8> {
    (0..=255u8)
        .filter(|&i| t.palette(Chan::from_u8(i)).is_some())
        .collect()
}

#[test]
fn both_resets_clear_every_palette_override() {
    for (name, reset) in [("DECSTR", DECSTR), ("RIS", RIS)] {
        // The control: the overrides are there before the reset.
        each_read_size(SET, |t| assert_eq!(overrides(t), [1, 9, 100, 255]));
        each_read_size(&[SET, reset].concat(), |t| {
            assert_eq!(overrides(t), Vec::<u8>::new(), "{name} left overrides");
        });
    }
}

#[test]
fn both_resets_keep_the_dynamic_colours() {
    for (name, reset) in [("DECSTR", DECSTR), ("RIS", RIS)] {
        each_read_size(&[DYN, SET, reset].concat(), |t| {
            assert_eq!(
                t.dynamic_color(DynamicColor::Foreground),
                Some((0x11, 0x22, 0x33)),
                "{name}"
            );
            assert_eq!(
                t.dynamic_color(DynamicColor::Background),
                Some((0x44, 0x55, 0x66)),
                "{name}"
            );
            assert_eq!(
                t.dynamic_color(DynamicColor::Cursor),
                Some((0x77, 0x88, 0x99)),
                "{name}"
            );
        });
    }
}

/// A query after a reset answers the embedder's table, not the override.
#[test]
fn a_query_after_a_reset_answers_the_table() {
    for reset in [DECSTR, RIS] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_default_palette(&[(0x0a, 0x0b, 0x0c); 256]);
        support::feed(&mut t, &[SET, reset, b"\x1b]4;1;?\x07"].concat());
        assert_eq!(t.reply(), b"\x1b]4;1;rgb:0a0a/0b0b/0c0c\x07");
    }
}

/// The epoch moves when a reset changes a colour a rendition resolves
/// through, and not when there was nothing to clear: a renderer resolves the
/// table again under a new epoch, and a reset with no override must not make
/// it.
#[test]
fn a_reset_moves_the_colour_epoch_only_when_it_clears_something() {
    for (name, reset) in [("DECSTR", DECSTR), ("RIS", RIS)] {
        support::with_corpus(|t| {
            let before = t.color_epoch();
            support::feed(t, reset);
            assert_eq!(
                t.color_epoch(),
                before,
                "{name} with nothing set moved the epoch"
            );
            support::feed(t, SET);
            let set = t.color_epoch();
            assert_ne!(set, before, "the control: an override moves it");
            support::feed(t, reset);
            assert_ne!(
                t.color_epoch(),
                set,
                "{name} cleared overrides and moved nothing"
            );
        });
    }
}
