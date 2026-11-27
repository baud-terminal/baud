//! **A repeated `SGR` leaves the parser where the first one did.**
//!
//! `sgr-table` (off by default, kept for A/B) answers a plain `SGR` it has
//! seen before from a table keyed on its bytes, and returned from `fast_csi`
//! before the line that commits the node a control sequence ends in
//! (`Node::AfterControl`, the node the graph's `CsiFinal` edge leads to). The
//! node stayed where `Act::CsiStart` left it, inside the sequence, so the
//! byte after a repeated `SGR` was read as part of another control sequence:
//! `ESC [ 38;5;196 m A ESC [ 38;5;196 m B` ran the `B` as `CSI B`, a cursor
//! down, and wrote nothing. The default build has no table and was right.
//!
//! Each program is held against the same program with the repeated `SGR`
//! removed, which sets the rendition already in force and so changes
//! nothing, fed a byte a read, on the cursor, the screen, the bells and the
//! unsupported count, at every read size and every split into two reads.
//! The table learns a sequence of at least `SGR_MIN_BYTES` (8) bytes after
//! the `ESC [` only from a read that holds it whole, and answers only one held
//! whole, so a feed can go red only where both occurrences were.
//!
//! **These cases test the `sgr-table` arm only when it is built**:
//! `cargo nextest run -p baud-core --features sgr-table --test sgr_repeat`,
//! which `cargo xtask feature-suites` runs with the rest of the suite. Under
//! default features they hold the parse every `SGR` takes.
mod support;
use baud_core::*;

fn fed<R>(bytes: &[u8], cuts: &[usize], f: impl FnOnce(&Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 40,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let mut from = 0;
    for &to in cuts.iter().chain([bytes.len()].iter()) {
        support::feed(&mut t, &bytes[from..to]);
        from = to;
    }
    f(&t)
}

/// `got` at every read size and every two-read split, against `want` fed a
/// byte a read: the cursor, the screen, the bells and the unsupported count.
/// **The reference is fed a byte a read** because there no sequence is whole
/// in one read, so the table neither learns nor answers and `want` is the
/// graph's parse under either build.
fn same_at_every_split(what: &str, got: &[u8], want: &[u8]) {
    let every: Vec<usize> = (1..want.len()).collect();
    fed(want, &every, |w| {
        let check = |at: &str, t: &Term<'_>| {
            assert_eq!(t.cursor(), w.cursor(), "{what}, {at}: cursor");
            assert_eq!(t.unsupported, w.unsupported, "{what}, {at}: unsupported");
            assert_eq!(t.bells, w.bells, "{what}, {at}: bells");
            assert!(
                support::screen(t) == support::screen(w),
                "{what}, {at}: screen"
            );
        };
        for size in 1..=got.len() {
            let cuts: Vec<usize> = (size..got.len()).step_by(size).collect();
            fed(got, &cuts, |t| check(&format!("read size {size}"), t));
        }
        for at in 1..got.len() {
            fed(got, &[at], |t| check(&format!("split at byte {at}"), t));
        }
    });
}

/// **The byte after a repeated `SGR` is text.** `B` is a final byte, so a
/// parser left inside the sequence runs it as `CSI B`; `7` is a parameter
/// byte, so it is swallowed into the next sequence, and the `m` that follows
/// it ends one. A third `SGR` after the text shows the table's answer is the
/// rendition the parse gave.
#[test]
fn the_byte_after_a_repeated_sgr_is_text() {
    same_at_every_split(
        "a final byte after the repeat",
        b"\x1b[38;5;196mA\x1b[38;5;196mB",
        b"\x1b[38;5;196mAB",
    );
    same_at_every_split(
        "a parameter byte after the repeat",
        b"\x1b[38;5;196mA\x1b[38;5;196m7m",
        b"\x1b[38;5;196mA7m",
    );
    same_at_every_split(
        "a third repeat after text",
        b"\x1b[38;5;196mA\x1b[0mB\x1b[38;5;196mC\x1b[38;5;196mD",
        b"\x1b[38;5;196mA\x1b[0mB\x1b[38;5;196mCD",
    );
}

/// **The control: a sequence shorter than the table's floor is never
/// answered from it**, so it is green whether or not the node is committed.
/// `1;31m` is five bytes after the `ESC [`.
#[test]
fn a_short_repeated_sgr_is_parsed_each_time() {
    same_at_every_split("a short repeat", b"\x1b[1;31mA\x1b[1;31mB", b"\x1b[1;31mAB");
}
