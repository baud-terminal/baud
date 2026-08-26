//! Bugs found by inspection. After the port to baud-domain types, the UTF-8 ones
//! should PASS — the types make the old behaviour unrepresentable.
use baud_core::*;

fn boxed() -> Box<Storage> { Storage::boxed() }
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
    feed_all(&mut t, "日".as_bytes());           // 3 bytes, 2 display columns
    assert_eq!(t.cursor().1.index(), 2, "was 3: cx += 1 per continuation byte");
}

#[test]
fn bug1b_emoji_advances_two_columns_not_four() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, "🚀".as_bytes());           // 4 bytes, 2 columns
    assert_eq!(t.cursor().1.index(), 2, "was 4");
}

#[test]
fn combining_mark_occupies_no_column() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, "e\u{301}".as_bytes());     // e + combining acute
    assert_eq!(t.cursor().1.index(), 1, "combining mark must not advance");
}

#[test]
fn mixed_ascii_and_cjk_column_math() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, "ab日本cd".as_bytes());     // 2 + 2 + 2 + 2 = 8 columns
    assert_eq!(t.cursor().1.index(), 8);
}

#[test]
fn invalid_utf8_renders_as_replacement_not_dropped() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed_all(&mut t, b"a\xff\xfeb");
    assert!(t.replacements.count() >= 2, "invalid bytes must render, not vanish");
    assert_eq!(t.cursor().1.index(), 4, "a + 2 replacements + b");
}

#[test]
fn truncated_utf8_at_chunk_end_recovers() {
    // a 3-byte sequence split across two feeds must still decode to one char
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let b = "日".as_bytes();
    for part in [&b[..1], &b[1..]] { feed_all(&mut t, part); }
    assert_eq!(t.cursor().1.index(), 2);
}

#[test]
fn bug3_maxrun_overflow_is_observable() {
    let mut s = boxed();
    let mut t = Term::new(&mut s, OpMode::Runs);
    let mut input = Vec::new();
    for i in 0..20 { input.extend_from_slice(format!("\x1b[3{}mxy", i % 8).as_bytes()); }
    feed_all(&mut t, &input);
    assert!(!t.maxrun_overflows.is_zero() || !t.promotions.is_zero(),
            "overflow must be counted, not silent");
}

#[test]
#[ignore = "BUG 2: promoted lines still lose edits when scrolled off"]
fn bug2_cursor_addressed_edit_survives_scrolling_off() {
    // promote() has no write-back into the run pool before eviction.
}
