//! The rendition table's write log: what a renderer that caches the resolved
//! table replays instead of resolving every slot a frame.
//!
//! The contract is exact: a reader that copies the slots
//! `Term::styles_written` names since its count onto what it last copied
//! holds the table as it is now, at every read boundary, through sweeps and
//! freezes, and when it has fallen more than a log behind.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::{Count, StyleId, StyleWord, StyleWrites};

/// A terminal whose rendition table has the 1,024-slot floor, so a program
/// can fill it.
const SMALL: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

fn sgr_fg((r, g, b): (u8, u8, u8)) -> Vec<u8> {
    format!("\x1b[38;2;{r};{g};{b}m").into_bytes()
}

/// Where a program's cells go: over one cell, so each rendition dies when
/// the next replaces it and the table sweeps, or each on a cell of its own,
/// the rows pushed into the history, so every slot stays named and the
/// table freezes the history to make room.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cells {
    One,
    Own,
}

/// A program that mints `distinct` renditions, with a hit -- the rendition
/// before it again -- after every third, and an `SGR 0` after every tenth.
fn program(distinct: Count, cells: Cells) -> Vec<u8> {
    let step: &[u8] = match cells {
        Cells::One => b"\x08",
        Cells::Own => b"",
    };
    let mut out = Vec::new();
    let mut n = 0usize;
    'fill: for g in 0u8..=255 {
        for b in 1u8..=255 {
            out.extend(sgr_fg((7, g, b)));
            out.extend(b"X");
            out.extend(step);
            n += 1;
            if n.is_multiple_of(3) {
                out.extend(sgr_fg((7, g, b)));
                out.extend(b"Y");
                out.extend(step);
            }
            if n.is_multiple_of(10) {
                out.extend(b"\x1b[0mZ");
                out.extend(step);
                if cells == Cells::Own {
                    out.extend(b"\r\n");
                }
            }
            if n == distinct.index() {
                break 'fill;
            }
        }
    }
    out
}

/// Whether the log ever lapped a reader, so it was told `Written::Every`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lapped {
    Never,
    Yes,
}

/// A reader of the log: a copy of the table and the count it copied at.
struct Reader {
    copy: Vec<Option<StyleWord>>,
    at: StyleWrites,
    lapped: Lapped,
}

impl Reader {
    fn new(t: &Term<'_>) -> Reader {
        Reader {
            copy: vec![None; t.styles().one().len()],
            at: StyleWrites::NEVER,
            lapped: Lapped::Never,
        }
    }

    fn catch_up(&mut self, t: &Term<'_>) {
        match t.styles_written(self.at) {
            Written::Slots(parts) => {
                for &k in parts.iter().flat_map(|p| p.iter()) {
                    self.copy[k.index()] = t.styles().one().slots()[k.index()];
                }
            }
            Written::Every => {
                self.lapped = Lapped::Yes;
                self.copy.copy_from_slice(t.styles().one().slots());
            }
        }
        self.at = t.style_writes();
    }
}

/// **Every write is logged**: a reader that replays the log after every read
/// holds the table exactly, and so does one that catches up only every
/// 40,000 bytes -- about 1,500 renditions, more than the log holds -- at read
/// sizes 1, 3 and 4096, through a program that sweeps the table and one that
/// fills it with named slots and freezes.
///
/// A sweep logs every slot it frees, up to one fewer than the table has, and
/// then the rendition that asked for it: one write short of lapping a reader
/// that caught up just before. So a reader replaying after every read of one
/// or three bytes, which hold at most one `SGR`, is never lapped; a read of
/// 4096 bytes can hold a sweep and more mints than the rest of the log.
#[test]
fn every_write_to_the_rendition_table_is_logged() {
    for cells in [Cells::One, Cells::Own] {
        let bytes = program(Count::of(3_500), cells);
        for chunk in [1usize, 3, 4096] {
            support::with_term(&SMALL, |t| {
                let len = t.styles().one().len();
                assert_eq!(len, 1_024);
                let mut close = Reader::new(t);
                let mut far = Reader::new(t);
                let mut fed = 0usize;
                for (i, piece) in bytes.chunks(chunk).enumerate() {
                    t.write_at(BytePos::ZERO, piece);
                    t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
                    close.catch_up(t);
                    assert!(
                        close.copy == t.styles().one().slots(),
                        "{cells:?}, chunk {chunk}, read {i}: the replayed copy is not the table"
                    );
                    let before = fed;
                    fed += piece.len();
                    if fed / 40_000 != before / 40_000 {
                        far.catch_up(t);
                        assert!(
                            far.copy == t.styles().one().slots(),
                            "{cells:?}, chunk {chunk}, read {i}: far"
                        );
                    }
                }
                far.catch_up(t);
                assert!(far.copy == t.styles().one().slots());
                // The controls: the program reached the reclaim it is for,
                // the log lapped a reader that was a table behind, and the
                // close reader at the small reads never lapped.
                let r = t.style_reclaims();
                match cells {
                    Cells::One => assert!(r.sweeps > 0 && r.rewrites == 0, "{r:?}"),
                    // Named slots past the table: what lies off the screen
                    // was frozen, and the table swept after it.
                    Cells::Own => assert!(r.rewrites > 0 && r.refusals == 0, "{r:?}"),
                }
                assert_eq!(
                    far.lapped,
                    Lapped::Yes,
                    "{cells:?}, chunk {chunk}: the far reader never lapped"
                );
                if chunk < 4096 {
                    assert_eq!(
                        close.lapped,
                        Lapped::Never,
                        "{cells:?}, chunk {chunk}: the close reader lapped"
                    );
                }
            });
        }
    }
}

/// **Until the table is full, the log is its live slots, each once.**
#[test]
fn the_log_lists_every_live_slot_once_until_full() {
    support::with_term(&SMALL, |t| {
        support::feed(t, &program(Count::of(500), Cells::One));
        let Written::Slots(parts) = t.styles_written(StyleWrites::NEVER) else {
            panic!("500 renditions in 1,024 slots lapped the log");
        };
        let mut seen = vec![0u32; t.styles().one().len()];
        for &k in parts.iter().flat_map(|p| p.iter()) {
            seen[k.index()] += 1;
        }
        for (k, slot) in t.styles().one().slots().iter().enumerate() {
            let want = u32::from(slot.is_some());
            assert_eq!(seen[k], want, "slot {k} is listed {} times", seen[k]);
        }
        // Slot 0's reservation is the first write.
        assert_eq!(parts[0][0], StyleId::DEFAULT);
    });
}

/// **A reader more than a log behind is told every slot**, and one exactly a
/// log behind is still given the entries.
#[test]
fn a_lapped_reader_is_told_every() {
    // Lent no generations, so the table fills with named slots and no freeze
    // or sweep after it frees one.
    support::with_term_unfrozen(&SMALL, |t| {
        let len = t.styles().one().len();
        // Fill the table: slot 0 and 1,023 distinct renditions, each on a
        // cell of its own and kept in the history, so a sweep frees nothing
        // and logs nothing, and each rendition is one write.
        let mut plane = (0u8..=255).flat_map(|g| (1u8..=255).map(move |b| (9u8, g, b)));
        let mut n = 0;
        while !t.style_writes().past(len - 1) {
            let mut one = sgr_fg(plane.next().expect("room"));
            one.extend(b"X");
            n += 1;
            if n % 40 == 0 {
                one.extend(b"\r\n");
            }
            support::feed(t, &one);
        }
        assert_eq!(t.style_reclaims().freed, 0, "the control: nothing freed");
        // Exactly `len` writes: a reader at NEVER is a log behind and not
        // more, so it is given the entries.
        assert!(matches!(
            t.styles_written(StyleWrites::NEVER),
            Written::Slots(_)
        ));
        // A full table of named slots refuses and writes nothing, so a cell
        // overwritten in the default leaves its slot to the next sweep,
        // which the refusals bring due: it frees the slot and places the
        // rendition, two writes past the log.
        support::feed(t, b"\x1b[0m\x1b[5;1Hk");
        let mut tries = 0;
        while !t.style_writes().past(len) {
            assert!(tries < len / 16 + 2, "no sweep came due");
            let mut one = sgr_fg(plane.next().expect("room"));
            one.extend(b"X");
            support::feed(t, &one);
            tries += 1;
        }
        assert!(
            matches!(t.styles_written(StyleWrites::NEVER), Written::Every),
            "one write past a log is lapped"
        );
    });
}

/// **A hit writes nothing**: the rendition a cell already has, again, moves
/// no count, and so costs a reader nothing.
#[test]
fn a_hit_writes_nothing() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[31mred\x1b[0m");
        let at = t.style_writes();
        support::feed(t, b"\x1b[31mred again\x1b[0m\x1b[0m\x1b[31m");
        assert_eq!(
            t.style_writes(),
            at,
            "a rendition already in the table was written again"
        );
        support::feed(t, b"\x1b[32mgreen");
        assert!(
            t.style_writes() > at,
            "the control: a new rendition is a write"
        );
    });
}

/// **The colour epoch moves exactly when a colour a rendition resolves
/// through changes**: a palette entry or the default foreground or
/// background, set or reset, and only when the value changed. Not the cursor
/// colour, not a query, not a reset of the terminal, and not the defaults an
/// embedder reports for `OSC 10`/`11` queries.
#[test]
fn the_colour_epoch_moves_exactly_when_a_resolve_input_moves() {
    support::with_corpus(|t| {
        let mut last = t.color_epoch();
        let mut expect = |t: &Term<'_>, moved: bool, what: &str| {
            let now = t.color_epoch();
            assert_eq!(now != last, moved, "{what}: moved {}", now != last);
            last = now;
        };
        let steps: [(&[u8], bool, &str); 16] = [
            (b"\x1b]4;1;rgb:12/34/56\x07", true, "OSC 4 sets entry 1"),
            (
                b"\x1b]4;1;rgb:12/34/56\x07",
                false,
                "OSC 4 sets it to what it is",
            ),
            (b"\x1b]4;1;?\x07", false, "OSC 4 queries it"),
            (b"\x1b]104;1\x07", true, "OSC 104 resets entry 1"),
            (b"\x1b]104;1\x07", false, "OSC 104 resets it again"),
            (b"\x1b]4;9;rgb:ff/00/00\x07", true, "OSC 4 sets entry 9"),
            (b"\x1b]104\x07", true, "OSC 104 resets every entry"),
            (b"\x1b]104\x07", false, "OSC 104 with none set"),
            (
                b"\x1b]10;rgb:01/02/03\x07",
                true,
                "OSC 10 sets the foreground",
            ),
            (b"\x1b]10;rgb:01/02/03\x07", false, "OSC 10 to what it is"),
            (
                b"\x1b]11;rgb:04/05/06\x07",
                true,
                "OSC 11 sets the background",
            ),
            (b"\x1b]110\x07", true, "OSC 110 resets the foreground"),
            (b"\x1b]111\x07", true, "OSC 111 resets the background"),
            (
                b"\x1b]12;rgb:07/08/09\x07",
                false,
                "OSC 12 sets the cursor colour",
            ),
            (b"\x1b]112\x07", false, "OSC 112 resets it"),
            (b"\x1bc\x1b[!p", false, "RIS and DECSTR"),
        ];
        for (bytes, moved, what) in steps {
            support::feed(t, bytes);
            expect(t, moved, what);
        }
        t.set_default_colors((1, 2, 3), (4, 5, 6));
        expect(t, false, "the embedder's defaults for a query");
    });
}

/// **Neither reset writes the rendition table**, so neither moves its count:
/// a reader has nothing to replay after `RIS` or `DECSTR`.
#[test]
fn resets_leave_the_rendition_table_and_its_log() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[31mred\x1b[1;32mbold green\x1b[0m");
        let (styles, at) = (t.styles().one().slots().to_vec(), t.style_writes());
        support::feed(t, b"\x1bc");
        support::feed(t, b"\x1b[!p");
        assert!(
            t.styles().one().slots() == styles.as_slice(),
            "a reset wrote the table"
        );
        assert_eq!(t.style_writes(), at, "a reset moved the count");
    });
}

/// **A storage lent to a second terminal keeps its table's book**: the same
/// serial, the same count, the same slots -- a count that restarted would
/// tell a reader the slots were never written -- and a moved colour epoch,
/// because the dynamic colours restart with the terminal.
#[test]
fn a_relent_storage_keeps_its_book() {
    let mut owned = Owned::new(&support::CORPUS);
    let (serial, at, epoch, styles) = {
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        support::feed(&mut t, b"\x1b[31mred\x1b[1;32mbold\x1b[0m");
        (
            t.style_table(),
            t.style_writes(),
            t.color_epoch(),
            t.styles().one().slots().to_vec(),
        )
    };
    let t = Term::new(owned.storage(), &support::CORPUS);
    assert_eq!(t.style_table(), serial, "the same table has another serial");
    assert_eq!(t.style_writes(), at, "the count restarted");
    assert!(
        t.styles().one().slots() == styles.as_slice(),
        "the table changed on a new lend"
    );
    assert_ne!(
        t.color_epoch(),
        epoch,
        "the epoch did not move with the new Cold"
    );
    // Slot 0's reservation is not logged twice.
    let Written::Slots(parts) = t.styles_written(StyleWrites::NEVER) else {
        panic!("three writes lapped the log");
    };
    let zeros = parts
        .iter()
        .flat_map(|p| p.iter())
        .filter(|k| **k == StyleId::DEFAULT)
        .count();
    assert_eq!(zeros, 1, "slot 0 is logged {zeros} times");
}

/// **Two tables are two serials**, wherever they are allocated.
#[test]
fn a_new_table_has_a_new_serial() {
    let mut one = Owned::new(&support::CORPUS);
    let first = Term::new(one.storage(), &support::CORPUS).style_table();
    drop(one);
    let mut two = Owned::new(&support::CORPUS);
    let second = Term::new(two.storage(), &support::CORPUS).style_table();
    assert_ne!(first, second, "a new table took an old serial");
}
