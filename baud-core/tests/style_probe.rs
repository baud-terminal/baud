//! The rendition table's probe on a table with no empty slot: a new
//! rendition reads to its bucket's reach and no further, and a rendition
//! already in the table is found however far past its bucket it lies.
//!
//! A linear probe stops at the rendition it looks for or at an empty slot.
//! A full table has no empty slot, so before the reach a miss there read
//! all of it: 65,536 slots a new colour at the app's provisioning, which put
//! a picture viewer printing into the shell 95% inside the probe. Each test
//! names the mechanism it holds.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::{Chan, ColorWord, StyleWord};

/// The 1,024-slot floor, so a program fills it in a few thousand renditions.
const SMALL: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    support::feed(t, bytes);
    t.clear_reply();
}

/// `SGR 0`, then a truecolour foreground and background, both from `i`, so
/// every `(i, tag)` is a distinct rendition for `i` below 65,536.
fn rendition(i: usize, tag: u8) -> String {
    format!(
        "\x1b[0;38;2;{tag};{};{};48;2;{};{};{}m",
        i & 255,
        (i >> 8) & 255,
        (i * 7) & 255,
        tag,
        (i >> 3) & 255
    )
}

/// The word [`rendition`] folds to.
fn word(i: usize, tag: u8) -> StyleWord {
    let c = |n: usize| Chan::from_u8(n.to_le_bytes()[0]);
    StyleWord::DEFAULT
        .with_fg(ColorWord::rgb(c(usize::from(tag)), c(i), c(i >> 8)))
        .with_bg(ColorWord::rgb(c(i * 7), c(usize::from(tag)), c(i >> 3)))
}

/// 1,023 renditions on cells of their own, so every slot but the default's
/// is named and no sweep frees one: on a storage lent no generations, the
/// table is full and stays full. The
/// line feed is in the default rendition, so a scroll interns nothing.
fn fill(t: &mut Term<'_>, tag: u8) -> Vec<u8> {
    let len = t.styles().one().len();
    let mut out = Vec::new();
    for i in 0..len - 1 {
        out.extend(format!("{}X", rendition(i, tag)).as_bytes());
        if (i + 1) % 40 == 0 {
            out.extend(b"\x1b[0m\r\n");
        }
    }
    feed(t, &out);
    out
}

/// **A new rendition on a full table reads to its bucket's reach, not the
/// whole table.** The table, lent no generations, is filled with named
/// renditions, then 512 more are minted on fresh cells: each is a miss with
/// no empty slot to stop it, and each is refused. Reading the whole table, each read 1,024 slots;
/// bounded by its bucket's reach, each reads the inline probe's 64 and a
/// few more.
#[test]
fn a_miss_on_a_full_table_reads_to_its_buckets_reach() {
    support::with_term_unfrozen(&SMALL, |t| {
        let len = t.styles().one().len();
        assert_eq!(len, 1024, "the test is sized to the floor");
        fill(t, 21);
        assert!(
            t.styles().one().slots().iter().all(Option::is_some),
            "the control: full"
        );
        let before = (t.style_reclaims(), t.style_probes());
        let mut out = Vec::new();
        for i in 0..512 {
            out.extend(format!("{}N", rendition(len + i, 21)).as_bytes());
        }
        feed(t, &out);
        let r = t.style_reclaims();
        let read = t.style_probes().since(before.1);
        assert!(
            r.refusals - before.0.refusals >= 480,
            "the control: the 512 were misses on a full table: {r:?}"
        );
        assert!(
            read < 512 * (len as u64) / 8,
            "{read} slots read by 512 misses on a {len}-slot table"
        );
    });
}

/// **A rendition far past its bucket is found, not interned again.** The
/// renditions that filled the table last lie farthest past their buckets.
/// Writing every one of them again after the table is full, each is a hit:
/// no slot is written and nothing is refused.
#[test]
fn a_full_tables_renditions_are_found_however_far_they_lie() {
    support::with_term_unfrozen(&SMALL, |t| {
        let program = fill(t, 23);
        let before = (t.style_writes(), t.style_reclaims(), t.style_probes());
        feed(t, b"\x1b[H");
        feed(t, &program);
        assert_eq!((t.style_writes(), t.style_reclaims()), (before.0, before.1));
        assert!(
            t.style_probes() > before.2,
            "positive control: some rendition lies past the inline probe"
        );
    });
}

/// **A sweep's kept renditions stay findable.** On the full table, minting
/// goes on until a full table's sweep runs and frees nothing, which sets
/// each bucket's reach again from the renditions it keeps. The renditions
/// that filled the table are written again over their own cells, after the
/// sweep, and each is a hit.
#[test]
fn a_sweep_keeps_every_kept_rendition_findable() {
    support::with_term_unfrozen(&SMALL, |t| {
        let len = t.styles().one().len();
        fill(t, 25);
        let at_full = t.style_reclaims();
        // Minted over one cell at the bottom right: each new one is a miss,
        // and the cell it replaces names only the one before.
        let mut i = len;
        while t.style_reclaims().sweeps == at_full.sweeps {
            assert!(i < 2 * len, "no sweep ran on a full table");
            feed(t, format!("\x1b[10;40H{}N", rendition(i, 25)).as_bytes());
            i += 1;
        }
        let swept = (t.style_writes(), t.style_reclaims(), t.style_probes());
        assert_eq!(swept.1.freed, at_full.freed, "the control: all named");
        // Every rendition the table still holds, written again in the
        // default's place at the bottom right.
        let held: Vec<usize> = (0..i)
            .filter(|&n| t.styles().one().slots().contains(&Some(word(n, 25))))
            .collect();
        assert!(held.len() > len / 2, "the control: the table holds them");
        for &n in &held {
            feed(t, format!("\x1b[10;40H{}K", rendition(n, 25)).as_bytes());
        }
        assert_eq!((t.style_writes(), t.style_reclaims()), (swept.0, swept.1));
        assert!(
            t.style_probes() > swept.2,
            "positive control: some rendition lies past the inline probe"
        );
    });
}
