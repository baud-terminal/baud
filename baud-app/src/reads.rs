//! The child's reads into the core, and the core's answers out behind them.
//! Pure -- reads in, a `Term` and a sink, bytes out -- because both drivers'
//! loops are platform-bound and no test reaches them, the shape `wheel.rs`
//! took for the wheel.

use baud_core::{BytePos, ReadLen, Term};

/// Feed each read to the core, in order, and hand `sink` every byte it
/// answered, behind whatever it already owed.
///
/// **The reply buffer is drained after every read**, not once after the
/// batch, and once before the first, so answers an earlier writer left owed
/// do not share it with the first read's. It is `REPLY`, 256 bytes, and the
/// macOS pump fed every read the
/// reader thread had queued and then drained once, so the answers of all
/// but the first reads were dropped: a burst of `OSC 4` palette queries, one
/// to each 600 bytes of drawing, had 10 of 256 answered. One read's answers
/// can still overflow it on their own -- herdr's 256 queries arrive in one
/// 4096-byte read -- and how to hold those is the owner's question in note
/// AG; this stops the loss between reads. `sink` is the outbox's `push`,
/// which makes no system call, so draining a read at a time writes no more
/// often than draining a batch did.
pub fn feed<'r>(
    term: &mut Term<'_>,
    reads: impl IntoIterator<Item = &'r [u8]>,
    mut sink: impl FnMut(&[u8]),
) {
    owed(term, &mut sink);
    for bytes in reads {
        term.write_at(BytePos::ZERO, bytes);
        term.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        owed(term, &mut sink);
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

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{Owned, READ_CHUNK, REPLY, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 40,
        ws_col: 120,
    };

    /// **Every palette query is answered, in order, however many reads the
    /// pump takes at once.** herdr 0.9.3 asks `OSC 4 ; n ; ?` for every `n`
    /// at startup and after a resize. Here each query sits in 600 bytes of
    /// drawing, so no read of `READ_CHUNK` holds more answers than the
    /// 256-byte reply buffer -- the test asserts that -- and a loss can only
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
        feed(&mut t, stream.chunks(READ_CHUNK), |b| {
            child.extend_from_slice(b)
        });
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
        feed(&mut t, [read.as_slice()], |b| child.extend_from_slice(b));
        let mut want = b"\x1b[0n".repeat(57);
        want.extend_from_slice(b"\x1b]4;1;rgb:1212/3434/5656\x07");
        want.extend_from_slice(b"\x1b]4;2;rgb:1212/3434/5656\x1b\\");
        assert_eq!(child, want, "the earlier answers, then the read's");
        assert_eq!(t.reply_overflows, before, "and nothing overflowed");
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
}
