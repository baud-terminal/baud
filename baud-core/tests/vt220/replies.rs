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

use crate::support;
use baud_core::*;

fn replies(bytes: &[u8]) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
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

/// **A `DA` with a parameter other than 0 is not a request, and gets no
/// answer**: `[[source: ecma48]]` 8.3.24, "With a parameter value not equal
/// to 0, DA is used to identify the device which sends the DA ... If the
/// parameter value is 0, DA is used to request an identifying DA from a
/// device"; xterm answers only `GetParam(0) <= 0` (`CASE_DA1`,
/// `[[source: xterm-reset]]`'s file). Each is counted in `unsupported`, so a
/// refusal is not invisible. The controls are the bare form, `0`, and `0`
/// with a second parameter, which xterm answers too.
#[test]
fn a_da_with_a_nonzero_parameter_is_not_answered_and_is_counted() {
    let ws = support::CORPUS;
    for (bytes, answered) in [
        (&b"\x1b[c"[..], true),
        (b"\x1b[0c", true),
        (b"\x1b[0;1c", true),
        (b"\x1b[1c", false),
        (b"\x1b[2c", false),
        (b"\x1b[6c", false),
        (b"\x1b[62c", false),
        (b"\x1b[65535c", false),
        (b"\x1b[1;0c", false),
    ] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let before = t.unsupported;
        support::feed(&mut t, bytes);
        let name = String::from_utf8_lossy(bytes).escape_debug().to_string();
        match answered {
            true => {
                assert_eq!(t.reply(), b"\x1b[?6c", "{name}: answered");
                assert_eq!(t.unsupported, before, "{name}: not counted");
            }
            false => {
                assert_eq!(t.reply(), b"", "{name}: no answer");
                assert_eq!(t.unsupported, before + 1, "{name}: counted");
            }
        }
    }
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
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
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
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b[5n");
    t.feed(BytePos::ZERO, ReadLen::from_index((b"\x1b[5n").len()));

    assert_eq!(t.reply(), b"\x1b[0n");
    t.clear_reply();
    assert!(t.reply().is_empty(), "a drained reply must not repeat");
}

/// Queries whose replies queue exactly `n` bytes: DSR 5's `CSI 0 n`, which is
/// `short` bytes in whichever C1 form is in force, and DA1's `CSI ? 6 c`, one
/// byte longer -- `n % short` DA1s and DSR 5s for the rest. It reaches every
/// `n` from `(short - 1) * (short + 1)` up: 15 in seven-bit form, 8 in eight.
pub(crate) fn fill(n: usize, short: usize) -> Vec<u8> {
    let long = n % short;
    let mut q = b"\x1b[c".repeat(long);
    q.extend_from_slice(&b"\x1b[5n".repeat((n - long * (short + 1)) / short));
    q
}

/// **A reply is sent whole or not at all, wherever the buffer stands.**
/// After `setup`, `answer` is made once into an empty buffer -- the control,
/// and the reply it must produce -- then behind every pending length from
/// `short * short` to one short of `REPLY`, which crosses the point where it stops
/// fitting. Below that point the tail is the whole reply and nothing is
/// counted; past it the tail is empty and `reply_overflows` rose by exactly
/// one. What was already owed is never touched.
///
/// DECRQSS failed this until 2026-10-09: its emitter checked each fragment
/// on its own, so with 200 bytes pending the child got a DCS with no `ST`,
/// and with 252 an `ESC P` and a `;` (note AG section 1.3).
pub(crate) fn whole_or_nothing(setup: &[u8], answer: impl Fn(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, setup);
    support::feed(&mut t, b"\x1b[5n");
    let short = t.reply().len();
    t.clear_reply();
    answer(&mut t);
    let full = t.reply().to_vec();
    t.clear_reply();
    assert!(
        !full.is_empty(),
        "the answer into an empty buffer is the control"
    );
    let (mut whole, mut absent) = (0, 0);
    for pending in short * short..REPLY {
        support::feed(&mut t, &fill(pending, short));
        assert_eq!(t.reply().len(), pending, "the filler queues exactly that");
        let owed = t.reply().to_vec();
        let before = t.reply_overflows;
        answer(&mut t);
        let r = t.reply();
        assert_eq!(
            &r[..pending],
            &owed[..],
            "what was owed is untouched at {pending}"
        );
        let tail = &r[pending..];
        match pending + full.len() <= REPLY {
            true => {
                assert_eq!(
                    tail,
                    &full[..],
                    "a reply that fits is sent whole at {pending}"
                );
                assert_eq!(
                    t.reply_overflows, before,
                    "and nothing is counted at {pending}"
                );
                whole += 1;
            }
            false => {
                assert!(
                    tail.is_empty(),
                    "a reply that does not fit is not sent at all at {pending}: {tail:02x?}"
                );
                assert_eq!(
                    t.reply_overflows,
                    before + 1,
                    "and counted once at {pending}"
                );
                absent += 1;
            }
        }
        t.clear_reply();
    }
    assert!(
        whole > 0 && absent > 0,
        "the sweep crosses the boundary: {whole} whole, {absent} absent"
    );
}

/// **Every other writer already copies one encoded reply under one check**:
/// `want_reply` a `Report`, `queue` a `Report` for the reports outside `feed`,
/// and `key` a `KeyBytes`. These hold that, through each: a CPR at the
/// bottom-right of the corpus screen (`CSI 50 ; 200 R`, ten bytes), the
/// palette's last entry (`OSC 4`, 28 bytes with `ST`; set by an `OSC 4` first,
/// because an entry with neither an override nor an embedder's table is not
/// answered), a focus report, and a key with two modifiers (`CSI 1 ; 6 A`).
#[test]
fn every_other_reply_is_sent_whole_or_not_at_all() {
    whole_or_nothing(b"\x1b[50;200H", |t| support::feed(t, b"\x1b[6n"));
    whole_or_nothing(b"\x1b]4;255;rgb:ff/00/80\x1b\\", |t| {
        support::feed(t, b"\x1b]4;255;?\x1b\\");
    });
    whole_or_nothing(b"\x1b[?1004h", |t| {
        t.focus(Focus::Gained);
    });
    let up = KeyEvent::press(KeyCode::Up, Modifiers::SHIFT.with(Modifiers::CTRL));
    whole_or_nothing(b"", |t| {
        t.key(up);
    });
}

/// A caller that never drains loses answers, but **countably**. Silent loss is
/// the failure this whole file exists to prevent.
#[test]
fn overflow_is_counted_not_silent() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    // Each CPR is 6-8 bytes; REPLY is 256, so well over 64 queries overflow it.
    let flood: Vec<u8> = std::iter::repeat_n(b"\x1b[6n".as_slice(), 200)
        .flatten()
        .copied()
        .collect();
    support::feed(&mut t, &flood);
    assert!(
        t.reply().len() <= 256,
        "the buffer must not grow past REPLY"
    );
    assert!(t.reply_overflows > 0, "dropped replies must be observable");
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
    let full: Vec<u8> = std::iter::repeat_n(b'x', 200).collect();
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
    let full: Vec<u8> = std::iter::repeat_n(b'x', 200).collect();
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
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
