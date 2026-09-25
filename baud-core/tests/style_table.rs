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
use baud_domain::{Chan, ColorWord, StyleId, StyleWord};

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
        let x = t.row(Row::from_index(1))[0];
        assert_eq!(t.style_of(x).fg().as_rgb(), Some(fg));
        assert_eq!(StyleId::bucket(t.style_of(x), len), 0);
        for (i, c) in t.row(Row::from_index(0))[..5].iter().enumerate() {
            assert_eq!(
                t.style_of(*c),
                StyleWord::DEFAULT,
                "`hello`'s column {i} took the colour of a rendition interned after it"
            );
        }
        let untouched = t.row(Row::from_index(5))[0];
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
            let tilde = t.row(Row::from_index(1))[0];
            assert_ne!(t.style_of(tilde), StyleWord::DEFAULT);
            assert_eq!(StyleId::bucket(t.style_of(tilde), 1_024), 0);
            for (i, c) in t.row(Row::from_index(0))[..10].iter().enumerate() {
                assert_eq!(
                    t.style_of(*c),
                    StyleWord::DEFAULT,
                    "the banner's column {i} took the prompt's colour"
                );
            }
        });
    }
}

/// **A full table evicts at the bucket, and the bucket of the default is
/// slot 0.** Fill a 1,024-slot table with 1,023 distinct renditions -- every
/// slot but the reserved one -- then intern one more that buckets to 0. The
/// table is full, so it evicts; it must evict something other than the
/// default, or every default cell takes its colour.
///
/// The renditions are written over one cell, backspacing between them, so
/// nothing scrolls and `hello` stays on row 0.
#[test]
fn a_full_table_never_evicts_slot_zero() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    support::with_term(&ws, |t| {
        let len = t.styles().len();
        assert_eq!(len, 1_024);
        let zero = fg_at_bucket_zero(len);
        support::feed(t, b"hello\r\n");
        // Distinct renditions over one cell until the table is full: every
        // slot but the reserved one, 1,023 of them.
        let mut plane = (0u8..=255)
            .flat_map(|g| (1u8..=255).map(move |b| (0u8, g, b)))
            .filter(|&c| c != zero);
        while !t.styles().iter().all(Option::is_some) {
            let mut one = sgr_fg(plane.next().expect("the plane has room"));
            one.extend(b"X\x08");
            support::feed(t, &one);
        }
        // The control: the table is full before the last rendition arrives,
        // so the one after it has to evict.
        assert!(
            t.styles().iter().all(Option::is_some),
            "the table is not full, so nothing below evicts"
        );
        let mut last = sgr_fg(zero);
        last.extend(b"Y");
        support::feed(t, &last);
        let y = t.row(Row::from_index(1))[0];
        assert_eq!(t.style_of(y).fg().as_rgb(), Some(zero));
        for (i, c) in t.row(Row::from_index(0))[..5].iter().enumerate() {
            assert_eq!(
                t.style_of(*c),
                StyleWord::DEFAULT,
                "an eviction took the default's slot: `hello`'s column {i} changed colour"
            );
        }
        assert_eq!(t.style_of(PackedCell::EMPTY), StyleWord::DEFAULT);
    });
}

/// **A restored cursor takes its rendition again, not the slot it had.**
/// `DECSC` saves the rendition and the id it was interned at; an eviction
/// between the save and the restore can give that id to another rendition,
/// and restoring the id then writes in the other rendition's colours.
#[test]
fn a_restored_cursor_takes_its_rendition_not_its_old_slot() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    support::with_term(&ws, |t| {
        let len = t.styles().len();
        let saved = (200u8, 10u8, 10u8);
        // The saved rendition, written once so its slot can be read, then
        // DECSC over the cell it was written in.
        let mut bytes = sgr_fg(saved);
        bytes.extend(b"S\x08\x1b7");
        support::feed(t, &bytes);
        let k = t.row(Row::from_index(0))[0].style;
        // Distinct renditions until the table is full.
        let mut plane = (0u8..=255).flat_map(|g| (1u8..=255).map(move |b| (0u8, g, b)));
        while !t.styles().iter().all(Option::is_some) {
            let mut one = sgr_fg(plane.next().expect("the plane has room"));
            one.extend(b"X\x08");
            support::feed(t, &one);
        }
        // Then one that buckets to the saved rendition's slot, so the full
        // table evicts exactly that slot.
        let thief = plane
            .find(|&(r, g, b)| {
                let w = StyleWord::DEFAULT.with_fg(ColorWord::rgb(
                    Chan::from_u8(r),
                    Chan::from_u8(g),
                    Chan::from_u8(b),
                ));
                StyleId::bucket(w, len) == k.index()
            })
            .expect("some rendition buckets to the saved slot");
        let mut one = sgr_fg(thief);
        one.extend(b"X\x08");
        support::feed(t, &one);
        // The control: the saved rendition is gone from the table, so its
        // old id names the thief.
        assert!(
            !t.styles()
                .iter()
                .flatten()
                .any(|w| w.fg().as_rgb() == Some(saved)),
            "no eviction took the saved rendition's slot, so the restore is not tested"
        );
        // DECRC, then a cell written in the restored rendition.
        support::feed(t, b"\x1b8Z");
        let z = t.row(Row::from_index(0))[0];
        assert_eq!(t.style_of(z).fg().as_rgb(), Some(saved));
    });
}
