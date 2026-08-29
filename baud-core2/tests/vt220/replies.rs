//! Queries and their answers: DA, DSR, and the reply channel itself.
//!
//! A terminal is not write-only. `DA`, `DSR` and `DECRQM` are questions, and an
//! application that asks one and gets nothing back **hangs** rather than
//! degrading -- which is why `docs/vt-coverage.md` called the missing reply
//! channel the largest structural gap, bigger than any operation count.
//!
//! Response formats are from `[[source: ctlseqs]]` § "Functions using CSI",
//! not from memory: DSR 5 -> `CSI 0 n`, DSR 6 -> `CSI r ; c R`, and the
//! Primary DA identities are the list under `CSI Ps c`.

use baud_core2::*;

fn replies(bytes: &[u8]) -> Vec<u8> {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
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
    t.reply().to_vec()
}

// ── Device Attributes ───────────────────────────────────────────────────────

/// `CSI c` answers, and answers what baud actually is.
///
/// VT102: baud has DECSTBM (VT100) and IL/DL (VT102), and does not have sixel,
/// ReGIS, UDKs, NRCS or 132-column mode. Selective erase, a VT220 feature, has
/// since landed and the answer still does not move -- a DA identity promises
/// *everything* at that level, so one VT220 feature does not buy the claim.
/// Over-claiming would make applications send sequences baud silently drops;
/// under-claiming only makes them use less.
#[test]
fn da_identifies_as_vt102() {
    assert_eq!(replies(b"\x1b[c"), b"\x1b[?6c");
    assert_eq!(
        replies(b"\x1b[0c"),
        b"\x1b[?6c",
        "0 and omitted are the same"
    );
}

// ── Device Status Report ────────────────────────────────────────────────────

/// `CSI 5 n` is "are you there?" and the answer is OK.
#[test]
fn dsr_operating_status_is_ok() {
    assert_eq!(replies(b"\x1b[5n"), b"\x1b[0n");
}

/// `CSI 6 n` reports the cursor, 1-based like CUP.
#[test]
fn cpr_reports_a_one_based_cursor() {
    assert_eq!(replies(b"\x1b[6n"), b"\x1b[1;1R", "home is 1;1, not 0;0");
    assert_eq!(replies(b"\x1b[7;13H\x1b[6n"), b"\x1b[7;13R");
}

/// CPR round-trips: the report can be fed back as a CUP and lands where it came
/// from. If the two disagreed on 0- vs 1-basing, this would drift by one.
#[test]
fn cpr_round_trips_through_cup() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let feed = |t: &mut Term, b: &[u8]| {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(b));
        t.feed(base, f.len());
        t.commit_read(f.len());
    };
    feed(&mut t, b"\x1b[9;40H\x1b[6n");
    let report = t.reply().to_vec();
    t.clear_reply();
    // Turn `CSI r ; c R` into `CSI r ; c H` and replay it from elsewhere.
    let mut cup = report.clone();
    let last = cup.len() - 1;
    cup[last] = b'H';
    feed(&mut t, b"\x1b[1;1H");
    feed(&mut t, &cup);
    let (r, c) = t.cursor();
    assert_eq!(
        (r.index(), c.index()),
        (8, 39),
        "CPR must round-trip via CUP"
    );
}

/// A DSR baud does not implement gets **no** answer.
///
/// Replying with the wrong report shape is worse than silence: the application
/// parses whatever arrives as the report it asked for.
#[test]
fn an_unsupported_dsr_is_not_answered() {
    assert!(
        replies(b"\x1b[15n").is_empty(),
        "DSR 15 (printer) is not ours"
    );
    assert!(replies(b"\x1b[99n").is_empty());
}

/// Private DSR (`CSI ? n`) is a different namespace and is not answered as if
/// it were the ANSI one.
#[test]
fn private_dsr_is_a_different_namespace() {
    assert!(replies(b"\x1b[?6n").is_empty());
}

// ── the channel ─────────────────────────────────────────────────────────────

/// Replies accumulate in order, so a burst of queries in one read all get
/// answered.
#[test]
fn replies_accumulate_in_order() {
    assert_eq!(
        replies(b"\x1b[5n\x1b[c\x1b[6n"),
        b"\x1b[0n\x1b[?6c\x1b[1;1R"
    );
}

/// Draining clears the buffer; nothing is repeated.
#[test]
fn clear_reply_empties_the_buffer() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(b"\x1b[5n"));
    t.feed(base, f.len());
    t.commit_read(f.len());

    assert_eq!(t.reply(), b"\x1b[0n");
    t.clear_reply();
    assert!(t.reply().is_empty(), "a drained reply must not repeat");
}

/// A caller that never drains loses answers, but **countably**. Silent loss is
/// the failure this whole file exists to prevent.
#[test]
fn overflow_is_counted_not_silent() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    // Each CPR is 6-8 bytes; REPLY is 256, so well over 64 queries overflow it.
    let flood: Vec<u8> = std::iter::repeat(b"\x1b[6n".as_slice())
        .take(200)
        .flatten()
        .copied()
        .collect();
    let mut i = 0;
    while i < flood.len() {
        let n = READ_CHUNK.min(flood.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let fl = slot.write(StreamByte::slice_from(&flood[i..i + n]));
        t.feed(base, fl.len());
        t.commit_read(fl.len());
        i += n;
    }
    assert!(
        t.reply().len() <= 256,
        "the buffer must not grow past REPLY"
    );
    assert!(
        t.reply_overflows.count() > 0,
        "dropped replies must be observable"
    );
}

/// CPR never reports a column the page does not have.
///
/// With autowrap on, a character written into the last column leaves the
/// internal cursor one *past* it -- the deferred-wrap sentinel the next write
/// consumes. That sentinel is not a position:
/// `[[source: vt510-decawm]]` describes the wrap as applying to "graphic
/// characters received when the cursor is at the right border of the page", so
/// the cursor is at the border. A 200-column page has no column 201.
///
/// This reported `CSI 1 ; 201 R` until a property test walked into it. It is
/// the wrong-shaped-reply failure exactly: an application that asks where the
/// cursor is and restores it later drifts by one column, silently, and only on
/// lines that happen to be full.
#[test]
fn cpr_clamps_the_pending_wrap_sentinel() {
    let full: Vec<u8> = std::iter::repeat(b'x').take(200).collect();
    let mut q = full.clone();
    q.extend_from_slice(b"\x1b[6n");
    assert_eq!(
        replies(&q),
        b"\x1b[1;200R",
        "the last column, not one past it"
    );

    // The sentinel is still doing its job: the next character wraps.
    let mut w = full.clone();
    w.extend_from_slice(b"y\x1b[6n");
    assert_eq!(
        replies(&w),
        b"\x1b[2;2R",
        "the 201st character wrapped to row 2"
    );
}

/// The report round-trips from the pending-wrap state too.
///
/// Clamping would be worthless if the reported position could not be restored:
/// this is the drift the clamp exists to prevent, measured end to end.
#[test]
fn cpr_round_trips_from_a_full_line() {
    let full: Vec<u8> = std::iter::repeat(b'x').take(200).collect();
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let feed = |t: &mut Term, b: &[u8]| {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(b));
        t.feed(base, f.len());
        t.commit_read(f.len());
    };
    feed(&mut t, &full);
    feed(&mut t, b"\x1b[6n");
    let mut cup = t.reply().to_vec();
    t.clear_reply();
    let last = cup.len() - 1;
    cup[last] = b'H';
    feed(&mut t, b"\x1b[10;10H");
    feed(&mut t, &cup);
    let (r, c) = t.cursor();
    assert_eq!(
        (r.index(), c.index()),
        (0, 199),
        "restoring the reported position must land back on the last column"
    );
}

// ── S7C1T / S8C1T: which form baud *sends* ──────────────────────────────────

/// `ESC SP G` (S8C1T) switches replies to 8-bit C1 introducers.
///
/// `[[source: ctlseqs]]`: "8-bit controls (S8C1T), VT220. This tells the
/// terminal to send C1 control characters as 8-bit sequences, e.g., its
/// responses to queries."
///
/// **This is a different axis from what baud accepts.** baud decodes UTF-8, so
/// it cannot *parse* an 8-bit introducer — `9B` is a continuation byte there.
/// But a reply is not UTF-8 text; it is a control stream the application asked
/// for by sending S8C1T. Refusing to send what was asked for because baud could
/// not read it back conflates the two directions, and ctlseqs describes
/// S7C1T/S8C1T purely in terms of what the terminal sends.
#[test]
fn s8c1t_switches_replies_to_eight_bit() {
    let r = replies(b"\x1b G\x1b[c");
    assert_eq!(
        r.first(),
        Some(&0x9b),
        "CSI must be sent as 9B, got {r:02x?}"
    );
    assert_eq!(
        &r[1..],
        &replies(b"\x1b[c")[2..],
        "and the rest of the report is unchanged"
    );
}

/// `ESC SP F` (S7C1T) is the default and switches back.
#[test]
fn s7c1t_is_the_default_and_switches_back() {
    assert_eq!(replies(b"\x1b[c").first(), Some(&0x1b), "7-bit by default");
    assert_eq!(
        replies(b"\x1b G\x1b F\x1b[c"),
        replies(b"\x1b[c"),
        "S8C1T then S7C1T is the default again"
    );
}

/// DCS replies carry the mode too — introducer *and* terminator.
///
/// DECRQSS answers `DCS 1 $ r ... ST`, so an 8-bit reply must open with `90`
/// and close with `9C`. Threading the mode into CSI and forgetting DCS would
/// send an application a reply half in each form.
#[test]
fn s8c1t_applies_to_dcs_replies_including_the_terminator() {
    let r = replies(b"\x1b G\x1bP$qm\x1b\\");
    assert!(!r.is_empty(), "DECRQSS must still answer");
    assert_eq!(r.first(), Some(&0x90), "DCS introducer, got {r:02x?}");
    assert_eq!(r.last(), Some(&0x9c), "ST terminator, got {r:02x?}");
}
