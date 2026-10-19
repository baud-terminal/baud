//! The child's reads into the core, and the core's answers out behind them.
//! Pure -- reads in, a `Term` and a sink, bytes out -- because both drivers'
//! loops are platform-bound and no test reaches them, the shape `wheel.rs`
//! took for the wheel.

use baud_core::{BytePos, ReadLen, Rest, Term};
use baud_domain::{DecodeBudget, DecodeProgress};

/// Feed each read to the core, in order, and hand `sink` every byte it
/// answered, behind whatever it already owed.
///
/// **The reply buffer is drained after every read**, not once after the
/// batch, and once before the first, so answers an earlier writer left owed
/// do not share it with the first read's. It is `REPLY` bytes, the longest
/// single reply (9,139 since 2026-10-19; it was 256), and the macOS pump fed
/// every read the reader thread had queued and then drained once, so the
/// answers of all but the first reads were dropped: a burst of `OSC 4`
/// palette queries, one to each 600 bytes of drawing, had 10 of 256
/// answered. Several long replies from one read can still overflow it on
/// their own, and how to hold those is the owner's question in note AG;
/// this stops the loss between reads. `sink` is the outbox's `push`,
/// which makes no system call, so draining a read at a time writes no more
/// often than draining a batch did.
///
/// **`after` sees the core after each read**, for what is an event rather
/// than state: an `OSC 52` write (`clip.rs`). The core keeps only the last
/// write it finished, so a batch whose reads each finish one would hand a
/// poll after the batch only the last; a title is state and is read once
/// after the batch, beside this.
///
/// **And the rest of a read a page-width request held** (`Term::take_held`,
/// under `WidthHold::On`): `after` sees the core at the request, resizes it
/// if it honours one, and the rest of the read is fed after -- so a program
/// that asks for 132 columns and draws in the same read draws at 132.
pub fn feed<'r>(
    term: &mut Term<'_>,
    reads: impl IntoIterator<Item = &'r [u8]>,
    mut sink: impl FnMut(&[u8]),
    mut after: impl FnMut(&mut Term<'_>),
) {
    owed(term, &mut sink);
    for bytes in reads {
        term.write_at(BytePos::ZERO, bytes);
        let mut next = Some(Rest {
            at: BytePos::ZERO,
            len: ReadLen::from_index(bytes.len()),
        });
        while let Some(r) = next {
            term.feed(r.at, r.len);
            owed(term, &mut sink);
            next = term.take_held();
            after(term);
        }
    }
}

/// One input event's answer -- a key, a focus report -- queued into an empty
/// buffer, and handed to `sink` behind everything the core already owed.
/// Returns what `queue` said: whether the event put anything in.
///
/// **Drained first, so the event always fits.** A key is at most
/// `KeyBytes::MAX_LEN` = 48 bytes and a report `Report::MAX_LEN` = 29, both
/// under `REPLY` (asserted below), so into an empty buffer neither can
/// overflow, and a `false` is only an event that sends nothing: a key
/// ctlseqs gives no encoding, a repeat DECARM refuses, a focus change the
/// application did not ask for. That keeps every key without a retry, which
/// could not tell those apart from a full buffer -- `Term::key` answers
/// `false` for all of them. The Linux loop queued keys behind a read's
/// answers and drained once after both, and behind 252 bytes of them a
/// six-byte key was dropped.
pub fn input(
    term: &mut Term<'_>,
    queue: impl FnOnce(&mut Term<'_>) -> bool,
    mut sink: impl FnMut(&[u8]),
) -> bool {
    owed(term, &mut sink);
    let queued = queue(term);
    owed(term, &mut sink);
    queued
}

const _: () = assert!(baud_domain::KeyBytes::MAX_LEN <= baud_core::REPLY);
const _: () = assert!(baud_domain::Report::MAX_LEN <= baud_core::REPLY);

/// The core's reply buffer to `sink`, and emptied.
pub fn owed(term: &mut Term<'_>, sink: &mut impl FnMut(&[u8])) {
    if !term.reply().is_empty() {
        sink(term.reply());
        term.clear_reply();
    }
}

/// How much of a deferred kitty decode one turn of a driver's loop does: a
/// MiB of compressed input fed or of scanlines made. mosaic-blue's
/// 1920x1080 decode is 31 ms over 60 steps of a quarter of this
/// (`baud-bench`'s `kitty_image`), so a turn spends about 2 ms of a 120 Hz
/// frame's 8.3.
pub const KITTY_DECODE_STEP: DecodeBudget = DecodeBudget::MIB;

/// Whether a step changed what the images show: a picture completed, or a
/// failed one freed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    Same,
    Changed,
}

/// **One step of a deferred kitty decode** (note V, V19 step 3: deferred
/// inside the core, in bounded steps): a picture is placed when its size
/// is known and its pixels are made here, [`KITTY_DECODE_STEP`] a call.
/// The macOS loop calls it on each display-link tick and the Linux loop on
/// each turn, between reads; the reply a decode owes is in the core's reply
/// buffer for the caller's flush. A command that arrives first finishes the
/// decode itself, so a late step only shows it late.
pub fn decode_step(term: &mut Term<'_>) -> (DecodeProgress, Shown) {
    let epoch = term.images().generation();
    let progress = term.decode_step(KITTY_DECODE_STEP);
    match term.images().generation() == epoch {
        true => (progress, Shown::Same),
        false => (progress, Shown::Changed),
    }
}

/// **How long a loop that waits on descriptors may block while a decode is
/// pending: not at all.** The next turn is scheduled at once, so the step
/// runs between reads rather than after the next one, and nothing sleeps;
/// with nothing pending the wait is the loop's own. The macOS loop waits on
/// no descriptor: its display link is the turn.
#[cfg(any(target_os = "linux", test))]
pub fn decode_wait(progress: DecodeProgress) -> Option<std::time::Duration> {
    match progress {
        DecodeProgress::More => Some(std::time::Duration::ZERO),
        DecodeProgress::Idle => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{Owned, READ_CHUNK, REPLY, WinSize};
    use std::time::Duration;

    const WS: WinSize = WinSize {
        ws_row: 40,
        ws_col: 120,
    };

    /// **Every palette query is answered, in order, however many reads the
    /// pump takes at once.** herdr 0.9.3 asks `OSC 4 ; n ; ?` for every `n`
    /// at startup and after a resize. Here each query sits in 600 bytes of
    /// drawing, so no read of `READ_CHUNK` holds more answers than the
    /// reply buffer -- the test asserts that -- and a loss can only
    /// be the pump's. The macOS pump fed every queued read and drained once,
    /// so the answers to every read but the first few were dropped.
    #[test]
    fn a_burst_of_reads_has_every_palette_query_answered_in_order() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let table: [(u8, u8, u8); 256] = core::array::from_fn(|i| {
            let i = u8::try_from(i).expect("256 entries");
            (i, 255 - i, i ^ 0x5a)
        });
        t.set_default_palette(&table);
        // Even indices end in `BEL`, odd in `ST`: the answer carries the
        // query's own terminator.
        let (mut stream, mut want, mut ends) = (Vec::new(), Vec::new(), Vec::new());
        for (n, &(r, g, b)) in table.iter().enumerate() {
            let end = if n % 2 == 0 { "\x07" } else { "\x1b\\" };
            stream.extend_from_slice(format!("\x1b]4;{n};?{end}").as_bytes());
            ends.push(stream.len());
            stream.extend_from_slice(&[b'x'; 600]);
            want.extend_from_slice(
                format!("\x1b]4;{n};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}{end}")
                    .as_bytes(),
            );
        }
        // The precondition: the queries that complete in any one read
        // answer in at most `REPLY` bytes, at the longest answer's 28.
        let longest = "\x1b]4;255;rgb:ffff/ffff/ffff\x1b\\".len();
        let mut at = 0;
        for read in stream.chunks(READ_CHUNK) {
            let done = ends
                .iter()
                .filter(|&&e| e > at && e <= at + read.len())
                .count();
            assert!(done * longest <= REPLY, "a read holds {done} queries");
            at += read.len();
        }
        let before = t.reply_overflows;
        let mut child = Vec::new();
        feed(
            &mut t,
            stream.chunks(READ_CHUNK),
            |b| child.extend_from_slice(b),
            |_| {},
        );
        let answered = child.windows(4).filter(|w| w == b"\x1b]4;").count();
        assert_eq!(answered, 256, "every index is answered");
        assert_eq!(child, want, "in order, each with its query's terminator");
        assert_eq!(t.reply_overflows, before, "and nothing overflowed");
        assert!(t.reply().is_empty(), "nothing is left behind");
    }

    /// **What the core already owed goes ahead of a read's answers, and out
    /// of their way.** Answers left pending by an earlier writer -- 228 bytes
    /// of `DSR 5` here -- plus a read's two palette answers, 25 and 26 bytes,
    /// are more than the buffer holds, though either alone fits; `feed`
    /// drains before the first read as well as after each.
    #[test]
    fn what_the_core_already_owed_goes_ahead_of_a_reads_answers() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_default_palette(&[(0x12, 0x34, 0x56); 256]);
        let earlier = b"\x1b[5n".repeat(57);
        t.write_at(BytePos::ZERO, &earlier);
        t.feed(BytePos::ZERO, ReadLen::from_index(earlier.len()));
        assert_eq!(t.reply().len(), 228, "the answers are pending");
        let before = t.reply_overflows;
        let read = b"\x1b]4;1;?\x07\x1b]4;2;?\x1b\\";
        let mut child = Vec::new();
        feed(
            &mut t,
            [read.as_slice()],
            |b| child.extend_from_slice(b),
            |_| {},
        );
        let mut want = b"\x1b[0n".repeat(57);
        want.extend_from_slice(b"\x1b]4;1;rgb:1212/3434/5656\x07");
        want.extend_from_slice(b"\x1b]4;2;rgb:1212/3434/5656\x1b\\");
        assert_eq!(child, want, "the earlier answers, then the read's");
        assert_eq!(t.reply_overflows, before, "and nothing overflowed");
    }

    /// **A read a page-width request held is fed on after `after`**, which
    /// resized the core between the two: the mark drawn after the request
    /// lands at the width asked for. The control: `after` runs twice, at the
    /// request and after the rest, and the width it saw first is the old one.
    #[test]
    fn the_rest_of_a_held_read_is_fed_after_the_embedder_resizes() {
        use baud_core::{Row, WidthHold};
        let room = WinSize {
            ws_row: 40,
            ws_col: 132,
        };
        let mut s = Owned::new(&room);
        let mut t = Term::new(s.storage(), &WS);
        t.set_width_hold(WidthHold::On);
        let read = b"\x1b[132$|\x1b[1;132HX";
        let mut widths = Vec::new();
        feed(
            &mut t,
            [read.as_slice()],
            |_| {},
            |t| {
                widths.push(t.geometry().0.index());
                if t.geometry().0.index() != 132 {
                    assert_eq!(t.resize(&room), baud_core::Resized::Ok);
                }
            },
        );
        assert_eq!(widths, [120, 132], "at the request, then after the rest");
        let row = t.row(Row::ZERO).expect("row 0");
        assert_eq!(
            row[131].content.as_codepoint().map(|c| c.value()),
            Some(u32::from(b'X'))
        );
    }

    /// **A key behind answers that nearly fill the buffer is kept.** The
    /// Linux loop fed a read, then queued the keys that arrived beside it,
    /// and drained once after both: behind 63 `DSR 5` answers -- 252 bytes --
    /// a six-byte arrow key, `CSI 1 ; 6 A`, did not fit, was dropped and counted, and
    /// `Term::key`'s `false` was ignored. Here the answers are left pending,
    /// the way any writer that did not drain would leave them.
    #[test]
    fn a_key_behind_pending_answers_reaches_the_child() {
        use baud_core::{KeyCode, KeyEvent, Modifiers};
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let read = b"\x1b[5n".repeat(63);
        t.write_at(BytePos::ZERO, &read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
        assert_eq!(t.reply().len(), 252, "the answers are pending");
        let before = t.reply_overflows;
        let mut child = Vec::new();
        let up = KeyEvent::press(KeyCode::Up, Modifiers::SHIFT.with(Modifiers::CTRL));
        let queued = input(&mut t, |t| t.key(up), |b| child.extend_from_slice(b));
        assert!(queued, "the key is queued");
        let mut want = b"\x1b[0n".repeat(63);
        want.extend_from_slice(b"\x1b[1;6A");
        assert_eq!(child, want, "the answers, then the key");
        assert_eq!(t.reply_overflows, before, "and nothing overflowed");
    }

    /// Base64, padded, for the kitty payload below.
    fn b64(data: &[u8]) -> Vec<u8> {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for q in data.chunks(3) {
            let n = q
                .iter()
                .enumerate()
                .fold(0u32, |n, (k, &b)| n | u32::from(b) << (16 - 8 * k));
            for k in 0..4 {
                let six = usize::try_from((n >> (18 - 6 * k)) & 63).unwrap_or(0);
                out.push(if k <= q.len() { A[six] } else { b'=' });
            }
        }
        out
    }

    /// **A transmitted PNG completes with no display link**: fed the way
    /// both drivers feed, it is placed and not shown, and [`decode_step`]
    /// called as the Linux loop calls it -- each turn, the wait not blocking
    /// while [`decode_wait`] says so -- shows it and owes its `OK`.
    #[test]
    fn a_kitty_png_completes_through_the_loops_step() {
        const PNG: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/png/rgba16-33x17-n.png"
        ));
        let mut owned = Owned::new(&WS).with_images(baud_domain::ImageBudget::of(1 << 12));
        let mut t = Term::new(owned.storage(), &WS);
        let mut cmd = b"\x1b_Ga=T,f=100,i=7;".to_vec();
        cmd.extend(b64(PNG));
        cmd.extend_from_slice(b"\x1b\\");
        let mut out = Vec::new();
        feed(
            &mut t,
            cmd.chunks(READ_CHUNK),
            |b| out.extend_from_slice(b),
            |_| {},
        );
        let (_, p) = t.images().placements().next().expect("placed at once");
        let image = p.image;
        assert!(
            t.images().image(image).is_none(),
            "not shown before its steps"
        );
        assert!(out.is_empty(), "nothing owed yet: {out:?}");
        let mut turns = 0;
        let mut shown = Shown::Same;
        let mut wait = Some(Duration::ZERO);
        while wait == Some(Duration::ZERO) {
            let (progress, s) = decode_step(&mut t);
            if s == Shown::Changed {
                shown = s;
            }
            owed(&mut t, &mut |b: &[u8]| out.extend_from_slice(b));
            wait = decode_wait(progress);
            turns += 1;
            assert!(turns < 100, "a decode that never ends");
        }
        assert!(turns >= 2, "stepped, not decoded in the feed: {turns}");
        assert_eq!(shown, Shown::Changed, "the frame is owed");
        let v = t.images().image(image).expect("shown");
        assert_eq!((v.width.raw(), v.height.raw()), (33, 17));
        assert_eq!(out, b"\x1b_Gi=7;OK\x1b\\");
        assert_eq!(decode_wait(DecodeProgress::Idle), None, "idle waits");
    }
}
