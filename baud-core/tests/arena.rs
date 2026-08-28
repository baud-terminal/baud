#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! The arena is a ring, and no test ever went round it.
//!
//! `ARENA` is 1 MiB and the largest payload any other test feeds is eight
//! kilobytes — a hundred and thirty times short of one lap. `read_slot` calls
//! `next_slot(ARENA)` and then slices `arena[base..base + READ_CHUNK]`, so a
//! wrap that lands wrong does not return bad data, it panics on the slice.
//!
//! The benchmark laps it constantly, at 48 MB a repetition, and asserts
//! nothing about what came out. That is the same shape as `Term::resize`,
//! which the property generator could not reach because it emits bytes rather
//! than calls — and that one had a panic in it.

use baud_core::*;

fn drive(bytes: &[u8]) -> (Vec<PackedCell>, (usize, usize)) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    let (r, c) = t.cursor();
    (t.viewport().to_vec(), (r.index(), c.index()))
}

/// How many lines it takes to lap the ring several times over.
fn lines_for(laps: usize) -> Vec<u8> {
    let mut v = Vec::new();
    let mut n = 0usize;
    while v.len() < ARENA * laps {
        v.extend_from_slice(format!("line {n}\r\n").as_bytes());
        n += 1;
    }
    v
}

/// **Lapping the arena changes nothing about what ends up on screen.**
///
/// A terminal that has scrolled shows only its last `rows` lines, so the same
/// tail fed to a fresh terminal must produce the same grid. If the ring wrap
/// dropped, duplicated or misaligned a byte, the two would differ — and if it
/// wrapped to a base that does not leave `READ_CHUNK` of room, this panics
/// before it can compare.
#[test]
fn lapping_the_arena_does_not_change_the_result() {
    let long = lines_for(3);
    assert!(
        long.len() >= ARENA * 3,
        "the payload must actually lap the ring: {} bytes against a {ARENA}-byte arena",
        long.len()
    );

    let (grid, cursor) = drive(&long);

    // The tail alone: everything after the last screenful is what survives.
    let text = String::from_utf8(long.clone()).expect("ascii");
    let all: Vec<&str> = text.split("\r\n").collect();
    let keep = 200.min(all.len());
    let tail = all[all.len() - keep..].join("\r\n");

    let (grid_tail, cursor_tail) = drive(tail.as_bytes());
    assert_eq!(
        cursor, cursor_tail,
        "the cursor after three laps must match the cursor after the tail alone"
    );
    assert_eq!(
        grid, grid_tail,
        "three laps of the ring must leave the same screen as the tail alone"
    );
}

/// The same, one byte at a time, so every slot boundary falls mid-sequence.
///
/// `read_slot` hands out a fresh slot per read, so a byte-at-a-time driver
/// laps the ring on *slot count* rather than byte count — it is the harshest
/// wrap schedule available, and the one a real `read(2)` never produces.
#[test]
fn lapping_the_arena_one_byte_at_a_time_agrees() {
    let payload = b"\x1b[1;1Habc\x1b[2;1Hdef\x1b[31mghi\r\n".repeat(64);

    let bulk = drive(&payload);

    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    for b in &payload {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&[*b]));
        t.feed(base, f.len());
        t.commit_read(f.len());
    }
    let (r, c) = t.cursor();
    let single = (t.viewport().to_vec(), (r.index(), c.index()));

    assert_eq!(bulk.1, single.1, "cursor");
    assert_eq!(bulk.0, single.0, "grid");
}

/// A slot is always `READ_CHUNK` and always inside the arena.
///
/// The invariant `read_slot`'s slice depends on: `next_slot` must never leave
/// a base with less than `READ_CHUNK` of room, or the slice panics. Checked
/// over enough slots to lap several times rather than argued from the code.
#[test]
fn every_slot_fits_inside_the_arena() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let slots = (ARENA / READ_CHUNK) * 3 + 7;
    for _ in 0..slots {
        let mut slot = t.read_slot();
        let base = slot.base();
        assert!(
            base.index() + READ_CHUNK <= ARENA,
            "slot at {} + {READ_CHUNK} runs past the {ARENA}-byte arena",
            base.index()
        );
        let f = slot.write(StreamByte::slice_from(b"x"));
        t.feed(base, f.len());
        t.commit_read(f.len());
    }
}
