//! `OSC 4 ; c ; ?` answers every entry.
//!
//! `[[source: xterm-reset]]`'s snapshot of `misc.c`, `ReportAnsiColorRequest`,
//! answers any entry's current colour; every other terminal read for note Y
//! does the same. baud answered only an entry a program had overridden,
//! because the core held no palette. The embedder now hands it the theme's
//! 256 entries beside the default foreground and background, and a query
//! answers the override, else the embedder's entry, else nothing.

mod support;

use baud_core::*;

/// A table whose every entry is distinct and easy to name: entry `i` is
/// `(i, 255 - i, i ^ 0x5a)`.
fn table() -> [(u8, u8, u8); 256] {
    core::array::from_fn(|i| {
        let i = u8::try_from(i).expect("256 entries");
        (i, 255 - i, i ^ 0x5a)
    })
}

/// The reply xterm sends for entry `i` of [`table`], BEL-terminated.
fn reply(i: u8) -> Vec<u8> {
    let (r, g, b) = table()[usize::from(i)];
    format!("\x1b]4;{i};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x07").into_bytes()
}

/// Feed `bytes` at every read size from one byte up, on a fresh terminal told
/// the table, and hand over the replies.
fn each_read_size(bytes: &[u8], f: impl Fn(&[u8])) {
    let ws = support::CORPUS;
    for size in [1, 3, 7, bytes.len()] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_default_palette(&table());
        for chunk in bytes.chunks(size.max(1)) {
            support::feed(&mut t, chunk);
        }
        f(t.reply());
    }
}

#[test]
fn an_entry_nothing_overrode_is_answered_from_the_embedders_table() {
    for i in [0u8, 1, 7, 15, 16, 231, 255] {
        let q = format!("\x1b]4;{i};?\x07");
        each_read_size(q.as_bytes(), |got| {
            assert_eq!(got, reply(i).as_slice(), "entry {i}");
        });
    }
}

#[test]
fn an_override_wins_over_the_table() {
    each_read_size(b"\x1b]4;9;rgb:ff/00/80\x07\x1b]4;9;?\x07", |got| {
        assert_eq!(got, b"\x1b]4;9;rgb:ffff/0000/8080\x07");
    });
}

#[test]
fn osc_104_returns_an_entry_to_the_table() {
    each_read_size(
        b"\x1b]4;9;rgb:ff/00/80\x07\x1b]104;9\x07\x1b]4;9;?\x07",
        |got| assert_eq!(got, reply(9).as_slice()),
    );
    each_read_size(
        b"\x1b]4;3;rgb:ff/00/80\x07\x1b]104\x07\x1b]4;3;?\x07",
        |got| assert_eq!(got, reply(3).as_slice()),
    );
}

/// The table is the embedder's, not the application's: telling the core it
/// does not change a colour any rendition resolves through, so the colour
/// epoch -- which tells the renderer to resolve again -- does not move. The
/// renderer draws from its own `Theme`, not from this copy.
#[test]
fn the_table_moves_no_colour_epoch_and_overrides_nothing() {
    support::with_corpus(|t| {
        let before = t.color_epoch();
        t.set_default_palette(&table());
        assert_eq!(t.color_epoch(), before, "the table moved the epoch");
        for i in 0..=255u8 {
            assert_eq!(
                t.palette(Chan::from_u8(i)),
                None,
                "entry {i} became an override"
            );
        }
    });
}

/// A second table replaces the first: a theme switch hands over new colours.
#[test]
fn a_second_table_replaces_the_first() {
    support::with_corpus(|t| {
        t.set_default_palette(&table());
        let mut other = table();
        other[4] = (1, 2, 3);
        t.set_default_palette(&other);
        support::feed(t, b"\x1b]4;4;?\x07");
        assert_eq!(t.reply(), b"\x1b]4;4;rgb:0101/0202/0303\x07");
    });
}
