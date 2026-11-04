//! The rendition table's reserved slot.
//!
//! [`StyleId::DEFAULT`] is slot 0, and a cell carries it without anything
//! being interned: an empty cell, an erased one, and every cell written
//! before the first `SGR`. So slot 0 has to hold [`StyleWord::DEFAULT`] for
//! as long as the table exists. Nothing reserved it: the first rendition to
//! probe into slot 0 took it, and every default cell on the screen then
//! resolved to that rendition's colours. And a full table evicts at the
//! bucket, so a rendition bucketing to 0 evicted the default itself.
//!
//! `StyleWord::DEFAULT`'s key is 0, so the default always buckets to slot 0:
//! the reservation is the slot the default would take anyway, taken before
//! anything else can.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::{Back, Chan, Codepoint, ColorWord, StyleId, StyleWord};

/// A truecolour foreground whose rendition buckets to slot 0 in a table of
/// `len` slots, found by walking the green/blue plane in order so the
/// fixture is the same on every run.
fn fg_at_bucket_zero(len: usize) -> (u8, u8, u8) {
    for g in 0u8..=255 {
        for b in 1u8..=255 {
            let word = StyleWord::DEFAULT.with_fg(ColorWord::rgb(
                Chan::from_u8(0),
                Chan::from_u8(g),
                Chan::from_u8(b),
            ));
            if StyleId::bucket(word, len) == 0 {
                return (0, g, b);
            }
        }
    }
    panic!("no truecolour foreground buckets to 0 in {len} slots");
}

fn sgr_fg((r, g, b): (u8, u8, u8)) -> Vec<u8> {
    format!("\x1b[38;2;{r};{g};{b}m").into_bytes()
}

/// Every cell of row 0's `hello`, and a cell nothing wrote, resolve to the
/// default rendition after a rendition that buckets to 0 is interned.
fn default_cells_stay_default(ws: &WinSize, len: usize) {
    support::with_term(ws, |t| {
        assert_eq!(
            t.styles().len(),
            len,
            "the table is the size this case is about"
        );
        let fg = fg_at_bucket_zero(len);
        let mut bytes = b"hello\r\n".to_vec();
        bytes.extend(sgr_fg(fg));
        bytes.extend(b"X");
        support::feed(t, &bytes);
        // The control: the rendition really does bucket to 0, and the cell
        // it was written in really does carry it -- so the probe reached the
        // slot this test is about.
        let x = t.row(Row::from_index(1)).expect("a row on the grid")[0];
        assert_eq!(t.style_of(x).fg().as_rgb(), Some(fg));
        assert_eq!(StyleId::bucket(t.style_of(x), len), 0);
        for (i, c) in t.row(Row::from_index(0)).expect("a row on the grid")[..5]
            .iter()
            .enumerate()
        {
            assert_eq!(
                t.style_of(*c),
                StyleWord::DEFAULT,
                "`hello`'s column {i} took the colour of a rendition interned after it"
            );
        }
        let untouched = t.row(Row::from_index(5)).expect("a row on the grid")[0];
        assert_eq!(t.style_of(untouched), StyleWord::DEFAULT);
        assert_eq!(t.style_of(PackedCell::EMPTY), StyleWord::DEFAULT);
    });
}

#[test]
fn slot_zero_holds_the_default_before_anything_is_interned() {
    // The corpus geometry: 16,384 slots.
    default_cells_stay_default(&support::CORPUS, 16_384);
    // The app's provisioned grid: 65,536 slots.
    default_cells_stay_default(
        &WinSize {
            ws_row: 150,
            ws_col: 400,
        },
        65_536,
    );
    // The floor: 1,024 slots.
    default_cells_stay_default(
        &WinSize {
            ws_row: 10,
            ws_col: 40,
        },
        1_024,
    );
}

/// **The hand model: a banner, then a prompt in a 256-colour.** Of 1,450
/// renditions a shell and `ls --color` send -- every attribute alone, the 16
/// colours in both grounds with bold, dim, underline and reverse, all 256
/// indexed foregrounds and backgrounds, bold with each indexed foreground,
/// and every 16x16 pair with and without bold -- two bucket to 0 in a
/// 1,024-slot table, `38;5;104` and `1;38;5;170`, and none in 16,384 or
/// 65,536. So a login that prints its banner in the default colours and
/// then a prompt opening with `38;5;104` turned the banner purple: nothing
/// before the prompt had sent `SGR 0`, which would have interned the default
/// into slot 0 first.
#[test]
fn a_prompt_in_a_256_colour_does_not_recolour_the_banner_above_it() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    for prompt in [
        &b"\x1b[38;5;104m~ \x1b[0m$ "[..],
        b"\x1b[1;38;5;170m~ \x1b[0m$ ",
    ] {
        support::with_term(&ws, |t| {
            assert_eq!(t.styles().len(), 1_024);
            let mut bytes = b"Last login: Thu Sep 25\r\n".to_vec();
            bytes.extend(prompt);
            support::feed(t, &bytes);
            // The control: the prompt's rendition buckets to 0.
            let tilde = t.row(Row::from_index(1)).expect("a row on the grid")[0];
            assert_ne!(t.style_of(tilde), StyleWord::DEFAULT);
            assert_eq!(StyleId::bucket(t.style_of(tilde), 1_024), 0);
            for (i, c) in t.row(Row::from_index(0)).expect("a row on the grid")[..10]
                .iter()
                .enumerate()
            {
                assert_eq!(
                    t.style_of(*c),
                    StyleWord::DEFAULT,
                    "the banner's column {i} took the prompt's colour"
                );
            }
        });
    }
}

/// `n` distinct renditions, each on a cell of its own and the rows pushed up
/// as they fill, so the history keeps every one of them named: a table they
/// fill holds no slot a sweep can free.
fn named_renditions(n: usize, mut plane: impl Iterator<Item = (u8, u8, u8)>) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..n {
        out.extend(sgr_fg(plane.next().expect("the plane has room")));
        out.extend(b"X");
        if (i + 1) % 40 == 0 {
            out.extend(b"\r\n");
        }
    }
    out
}

/// `hello`, the oldest line of the history.
fn oldest_line(t: &Term<'_>) -> Vec<PackedCell> {
    let sb = t.scrollback();
    let mut n = 0;
    while sb.line(Back::of(n + 1)).is_some() {
        n += 1;
    }
    sb.line(Back::of(n)).expect("the oldest line").0.to_vec()
}

/// **A full table never gives a named slot away, slot 0 least of all.** Fill
/// a 1,024-slot table with 1,023 distinct renditions -- every slot but the
/// reserved one -- each on a cell of its own and kept in the history, so every
/// slot stays named, on a storage lent no generations; then intern one more
/// that buckets to 0. The eviction this replaced wrote it over a slot some
/// cell named, and over slot 0 every default cell took its colour. Refused,
/// the last cell takes the default, and every cell already drawn keeps its
/// rendition: `[[source: dec-std-070]]`'s SGR, page 5-104, note 3.
#[test]
fn a_full_table_refuses_rather_than_give_a_named_slot_away() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    support::with_term_unfrozen(&ws, |t| {
        let len = t.styles().len();
        assert_eq!(len, 1_024);
        let zero = fg_at_bucket_zero(len);
        support::feed(t, b"hello\r\n");
        let plane = (0u8..=255)
            .flat_map(|g| (1u8..=255).map(move |b| (0u8, g, b)))
            .filter(|&c| c != zero);
        support::feed(t, &named_renditions(len - 1, plane));
        // The control: the table is full before the last rendition arrives,
        // and nothing was refused on the way, so the one after it has to be.
        assert!(
            t.styles().iter().all(Option::is_some),
            "the table is not full, so nothing below refuses"
        );
        let table = t.styles().to_vec();
        assert_eq!(t.style_reclaims().refusals, 0);
        let mut last = b"\r\n".to_vec();
        last.extend(sgr_fg(zero));
        last.extend(b"Y");
        support::feed(t, &last);
        assert_eq!(t.style_reclaims().refusals, 1, "the control: it refused");
        assert_eq!(t.styles(), &table[..], "a slot was written");
        let (_, rows) = t.geometry();
        let y = t
            .row(Row::from_index(rows.index() - 1))
            .expect("a row on the grid")[0];
        assert_eq!(t.style_of(y), StyleWord::DEFAULT, "the refused rendition");
        let hello = oldest_line(t);
        assert_eq!(hello[0].content.as_codepoint(), Codepoint::new(0x68));
        for (i, c) in hello[..5].iter().enumerate() {
            assert_eq!(
                t.style_of(*c),
                StyleWord::DEFAULT,
                "`hello`'s column {i} changed colour"
            );
        }
        assert_eq!(t.style_of(PackedCell::EMPTY), StyleWord::DEFAULT);
    });
}

/// **With generations, the same stream freezes the history and places the
/// last rendition**: the table passes its load, what lies off the screen
/// moves into a generation, the sweep frees its slots, and the cell takes
/// the colour it was written in. Every history line still reads its own.
#[test]
fn a_table_filled_by_the_history_freezes_it_and_places_the_rendition() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    support::with_term(&ws, |t| {
        let len = t.styles().len();
        let zero = fg_at_bucket_zero(len);
        support::feed(t, b"hello\r\n");
        let plane = (0u8..=255)
            .flat_map(|g| (1u8..=255).map(move |b| (0u8, g, b)))
            .filter(|&c| c != zero);
        let colours: Vec<_> = plane.clone().take(len - 1).collect();
        support::feed(t, &named_renditions(len - 1, plane));
        let mut last = b"\r\n".to_vec();
        last.extend(sgr_fg(zero));
        last.extend(b"Y");
        support::feed(t, &last);
        let r = t.style_reclaims();
        assert_eq!(r.refusals, 0, "{r:?}");
        assert!(r.rewrites > 0, "the control: it froze: {r:?}");
        let (_, rows) = t.geometry();
        let y = t
            .row(Row::from_index(rows.index() - 1))
            .expect("a row on the grid")[0];
        assert_eq!(t.style_of(y).fg().as_rgb(), Some(zero));
        // Every cell of the history and the screen in the colour it was
        // written in, read through its row's table.
        let sb = t.scrollback();
        let mut read = Vec::new();
        for back in (0..sb.len()).rev() {
            let (cells, _) = sb.line(Back::of(back)).expect("a retained line");
            let table = sb.styles(Back::of(back));
            read.extend(
                cells
                    .iter()
                    .filter(|c| c.content.as_codepoint() == Codepoint::new(0x58))
                    .map(|c| table.word(t.styles(), c.style)),
            );
        }
        let view = t.viewport();
        for y in 0..rows.index() {
            let r = Row::from_index(y);
            let table = view.styles(r);
            read.extend(
                view.row(r)
                    .iter()
                    .filter(|c| c.content.as_codepoint() == Codepoint::new(0x58))
                    .map(|c| table.word(t.styles(), c.style)),
            );
        }
        assert_eq!(read.len(), colours.len(), "every X read back");
        for (i, (w, c)) in read.iter().zip(&colours).enumerate() {
            assert_eq!(w.fg().as_rgb(), Some(*c), "cell {i} changed colour");
        }
        let hello = oldest_line(t);
        assert_eq!(hello[0].content.as_codepoint(), Codepoint::new(0x68));
    });
}

/// The saved rendition, written once so its slot can be read, then `DECSC`
/// over the cell it was written in, then the cell overwritten: its slot is
/// named by nothing but the save.
fn save_over_a_dead_cell(t: &mut Term<'_>, saved: (u8, u8, u8)) -> StyleId {
    let mut bytes = sgr_fg(saved);
    bytes.extend(b"S\x08\x1b7");
    support::feed(t, &bytes);
    let k = t.row(Row::from_index(0)).expect("a row on the grid")[0].style;
    support::feed(t, b"\x1b[0mT\x08");
    k
}

fn restored(t: &mut Term<'_>) -> StyleWord {
    support::feed(t, b"\x1b8Z");
    let z = t.row(Row::from_index(0)).expect("a row on the grid")[0];
    t.style_of(z)
}

/// **A restored cursor takes its rendition again, not the slot it had.**
/// `DECSC` saves the rendition, not the id it was interned at: a sweep
/// between the save and the restore frees that id, since the save is a word
/// and no cell names it, and a later rendition can take it. Restoring the id
/// would then write in the other rendition's colours.
#[test]
fn a_restored_cursor_takes_its_rendition_not_its_old_slot() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    support::with_term(&ws, |t| {
        let len = t.styles().len();
        let saved = (200u8, 10u8, 10u8);
        let k = save_over_a_dead_cell(t, saved);
        // Renditions over one cell on another row until the saved slot holds
        // another: the sweeps free it and the probes reach it.
        let mut plane = (0u8..=255).flat_map(|g| (1u8..=255).map(move |b| (0u8, g, b)));
        let mut n = 0;
        while t.styles()[k.index()].is_some_and(|w| w.fg().as_rgb() == Some(saved)) || n < len {
            let mut one = b"\x1b[5;1H".to_vec();
            one.extend(sgr_fg(plane.next().expect("the plane has room")));
            one.extend(b"X");
            support::feed(t, &one);
            n += 1;
        }
        // The control: a sweep ran, the saved rendition is gone from the
        // table, and its old id names another.
        assert!(t.style_reclaims().sweeps > 0);
        assert!(
            !t.styles()
                .iter()
                .flatten()
                .any(|w| w.fg().as_rgb() == Some(saved)),
            "the saved rendition is still in the table, so the restore is not tested"
        );
        assert_eq!(restored(t).fg().as_rgb(), Some(saved));
    });
}

/// **The same through a full table**: renditions on cells of their own,
/// kept in the history, fill the table past its load, so what lies off the
/// screen is frozen and the saved rendition's slot freed; the restore interns
/// its word again, refusing nothing, and the cell takes the saved colour.
#[test]
fn a_cursor_restored_into_a_full_table_takes_its_rendition() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    support::with_term(&ws, |t| {
        let len = t.styles().len();
        let saved = (200u8, 10u8, 10u8);
        save_over_a_dead_cell(t, saved);
        support::feed(t, b"\x1b[2;1H");
        let plane = (0u8..=255).flat_map(|g| (1u8..=255).map(move |b| (0u8, g, b)));
        support::feed(t, &named_renditions(len + 40, plane));
        // The control: the table froze what was off the screen, and the
        // saved rendition is gone from it.
        assert!(t.style_reclaims().rewrites > 0);
        assert!(
            !t.styles()
                .iter()
                .flatten()
                .any(|w| w.fg().as_rgb() == Some(saved)),
            "the saved rendition is still in the table"
        );
        assert_eq!(restored(t).fg().as_rgb(), Some(saved));
        assert_eq!(t.style_reclaims().refusals, 0, "the restore was refused");
    });
}
