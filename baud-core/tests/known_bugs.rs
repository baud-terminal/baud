//! Bugs found by inspection. After the port to baud-domain types, the UTF-8 ones
//! should PASS — the types make the old behaviour unrepresentable.
use baud_core::*;

fn boxed() -> Box<Storage> {
    Storage::boxed()
}
fn feed_all(t: &mut Term, bytes: &[u8]) {
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
}

#[test]
fn bug1_cjk_advances_two_columns_not_three() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, "日".as_bytes()); // 3 bytes, 2 display columns
    assert_eq!(
        t.cursor().1.index(),
        2,
        "was 3: cx += 1 per continuation byte"
    );
}

#[test]
fn bug1b_emoji_advances_two_columns_not_four() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, "🚀".as_bytes()); // 4 bytes, 2 columns
    assert_eq!(t.cursor().1.index(), 2, "was 4");
}

#[test]
fn combining_mark_occupies_no_column() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, "e\u{301}".as_bytes()); // e + combining acute
    assert_eq!(t.cursor().1.index(), 1, "combining mark must not advance");
}

#[test]
fn mixed_ascii_and_cjk_column_math() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, "ab日本cd".as_bytes()); // 2 + 2 + 2 + 2 = 8 columns
    assert_eq!(t.cursor().1.index(), 8);
}

#[test]
fn invalid_utf8_renders_as_replacement_not_dropped() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, b"a\xff\xfeb");
    assert!(
        t.replacements.count() >= 2,
        "invalid bytes must render, not vanish"
    );
    assert_eq!(t.cursor().1.index(), 4, "a + 2 replacements + b");
}

#[test]
fn truncated_utf8_at_chunk_end_recovers() {
    // a 3-byte sequence split across two feeds must still decode to one char
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let b = "日".as_bytes();
    for part in [&b[..1], &b[1..]] {
        feed_all(&mut t, part);
    }
    assert_eq!(t.cursor().1.index(), 2);
}

#[test]
fn bug3_maxrun_overflow_is_observable() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Runs);
    let mut input = Vec::new();
    for i in 0..20 {
        input.extend_from_slice(format!("\x1b[3{}mxy", i % 8).as_bytes());
    }
    feed_all(&mut t, &input);
    assert!(
        !t.maxrun_overflows.is_zero() || !t.promotions.is_zero(),
        "overflow must be counted, not silent"
    );
}

#[test]
#[ignore = "BUG 2: promoted lines still lose edits when scrolled off"]
fn bug2_cursor_addressed_edit_survives_scrolling_off() {
    // promote() has no write-back into the run pool before eviction.
}

/// DEL (`0x7f`) is not printable, wherever it lands in a read.
///
/// The AVX2 scanner once masked `v < 0x20 || v >= 0x80` and never tested
/// `0x7f`, so the 32-byte body wrote DEL as a cell while the scalar tail
/// substituted U+FFFD -- the same input rendered differently depending on
/// whether it fell in a vector block or the remainder. `Portable` cannot
/// reproduce it (it tests `< 0x20 || > 0x7e`, two comparisons, no separate DEL
/// term) but the shape is cheap to pin down permanently.
///
/// The two offsets straddle a 16-byte lane deliberately: one DEL inside the
/// first vector block, one far enough in to land in a later block, and the
/// leading text pushes a third into the scalar tail.
#[test]
fn del_is_never_printable_in_body_or_tail() {
    let mut data = Vec::new();
    data.extend_from_slice(b"ab\x7fcd"); // early: tail on a short read
    data.extend_from_slice(&[b'x'; 40]); // push past a lane boundary
    data.push(0x7f); // inside a vector block
    data.extend_from_slice(b"yz");

    let a = drive_bytes(&data, 1); // scalar tail only (chunk < 16 lanes)
    let b = drive_bytes(&data, 4096); // vector body
    assert_eq!(
        a, b,
        "DEL handled differently by the vector body and the scalar tail"
    );

    // Body-vs-tail agreement alone would still pass if both paths were wrong in
    // the same way, so pin the actual behaviour: DEL becomes U+FFFD and occupies
    // a cell.
    //
    // OPEN QUESTION, not asserted as correct. ECMA-48 makes DEL a character to
    // be *ignored*, and alacritty drops it — so this is a candidate for one of
    // the seven cases the differential test still fails. Deciding it is a
    // semantic change and needs its own differential run, not a test edit.
    let without: Vec<u8> = data.iter().copied().filter(|&c| c != 0x7f).collect();
    assert_ne!(
        a,
        drive_bytes(&without, 4096),
        "DEL now behaves as ignored; if that was deliberate, update this test \
         and check the differential against alacritty"
    );
}

fn drive_bytes(bytes: &[u8], chunk: usize) -> Vec<PackedCell> {
    let mut store = Storage::boxed();
    let mut t = Term::new(&mut store, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(READ_CHUNK).min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    t.viewport().to_vec()
}
