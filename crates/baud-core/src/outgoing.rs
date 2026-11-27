//! **The bytes the terminal owes the host, behind one owner.** Every reply
//! goes into the buffer through a door that frames it: a report encoded in
//! the C1 form S7C1T/S8C1T chose, a control string opened and closed by
//! [`StringReply`], a key's bytes, a control sequence's introducer. There
//! is no door for raw bytes, so no reply can spell `ESC ]` or `ESC \` by
//! hand and miss S8C1T, as the OSC answers and XTVERSION did.
//!
//! **Whole or nothing**: a reply that does not fit is dropped and counted,
//! never cut, since a truncated reply is a wrong answer -- an application
//! reads its first byte as the start of a sequence that never ends.
//!
//! **The C1 form is the live one, and no caller picks it.** The doors are
//! on [`Doors`], which [`Outgoing::lend`] builds from a borrow of the
//! terminal's `Cold` -- what S7C1T and S8C1T last set -- each time a reply
//! is written, so a reply written later than its query (a deferred kitty
//! decode) is framed in the form in force when it is written. No door
//! takes a `C1Transmit`.
//!
//! The fields are private to this module, which the compiler holds for
//! every other file of the crate; `tests/outgoing_owns_the_buffer.rs`
//! holds that no file names them and that no door names a C1 form.

use baud_domain::{ByteLen, C1Transmit, KeyBytes, Report, StringReply, VersionText};

/// **What became of the replies that did not go in** ([`Term::reply_counts`]).
///
/// [`Term::reply_counts`]: crate::Term::reply_counts
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ReplyCounts {
    /// Replies dropped whole because they did not fit, or could not be
    /// built whole. Counted rather than silently lost: a truncated reply is
    /// a wrong answer, and a wrong answer is worse than silence.
    pub overflows: usize,
}

/// The reply buffer, what of it is pending, and what did not go in.
pub(crate) struct Outgoing<'b> {
    buf: &'b mut [u8],
    len: ByteLen,
    counts: ReplyCounts,
}

impl<'b> Outgoing<'b> {
    pub(crate) fn new(buf: &'b mut [u8]) -> Outgoing<'b> {
        Outgoing {
            buf,
            len: ByteLen::ZERO,
            counts: ReplyCounts { overflows: 0 },
        }
    }

    /// The bytes pending.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.buf[..self.len.index()]
    }

    /// Drop what is pending, once sent.
    pub(crate) fn clear(&mut self) {
        self.len = ByteLen::ZERO;
    }

    /// What did not go in.
    pub(crate) fn counts(&self) -> ReplyCounts {
        self.counts
    }

    /// **Whether less room is left than the longest answer one control can
    /// owe** (`storage::REPLY_MOST`): where a read stops, under
    /// `ReplyHold::On`, so its answers can go out.
    pub(crate) fn low(&self) -> bool {
        self.len.index() > crate::storage::REPLY - crate::storage::REPLY_MOST
    }

    /// **The doors, framing in the live C1 form**: what S7C1T and S8C1T
    /// last set in the terminal's `cold`, read here. A borrow of the
    /// terminal's own state, so no caller can hand in a form of its own.
    pub(crate) fn lend(&mut self, cold: &crate::term::Cold) -> Doors<'_, 'b> {
        Doors {
            out: self,
            c1: cold.replies.c1(),
        }
    }

    /// `parts` appended whole, or the reply counted and dropped.
    fn put(&mut self, parts: &[&[u8]]) -> bool {
        let at = self.len.index();
        let n: usize = parts.iter().map(|p| p.len()).sum();
        if at + n > self.buf.len() {
            self.counts.overflows += 1;
            return false;
        }
        let mut w = at;
        for p in parts {
            self.buf[w..w + p.len()].copy_from_slice(p);
            w += p.len();
        }
        self.len = ByteLen::from_index(w);
        true
    }
}

/// **The ways into the reply buffer** ([`Outgoing::lend`]), each framing
/// in the C1 form the terminal sends in.
pub(crate) struct Doors<'o, 'b> {
    out: &'o mut Outgoing<'b>,
    c1: C1Transmit,
}

impl<'o, 'b> Doors<'o, 'b> {
    /// **A reply that could not be built whole**, counted as one that did
    /// not fit: its builder ran out of its own room.
    pub(crate) fn dropped(&mut self) {
        self.out.counts.overflows += 1;
    }

    /// A report.
    pub(crate) fn report(&mut self, r: Report) -> bool {
        let bytes = r.encode(self.c1);
        self.out.put(&[bytes.as_bytes()])
    }

    /// **XTVERSION's answer**, `DCS > | text ST`: the text the embedder
    /// told, which may be longer than a report holds.
    pub(crate) fn version(&mut self, v: &VersionText) -> bool {
        self.out.put(&v.reply_parts(self.c1))
    }

    /// A key's bytes, as [`baud_domain::encode_key`] made them, in the C1
    /// form the key state it was encoded with carried.
    pub(crate) fn key(&mut self, k: &KeyBytes) -> bool {
        self.out.put(&[k.as_bytes()])
    }

    /// A control sequence: `CSI`, then `rest`, its parameters and final,
    /// which the caller built.
    pub(crate) fn csi(&mut self, rest: &[u8]) -> bool {
        self.out.put(&[self.c1.csi(), rest])
    }

    /// A ReGIS input report: a key's or the null button's bytes, the
    /// position and a carriage return, or the carriage return alone.
    pub(crate) fn regis(&mut self, r: &crate::regis::RegisReport<'_>) -> bool {
        self.out.put(&[r.as_bytes()])
    }

    /// **A control string**: `frame`'s introducer, `body` and its
    /// terminator.
    pub(crate) fn string(self, frame: StringReply, body: &[&[u8]]) -> bool {
        let mut b = self.body(frame);
        for p in body {
            b.put(p);
        }
        b.finish()
    }

    /// **A control string written in place**: the introducer now, the body
    /// through the [`Body`] it returns, and the terminator at
    /// [`Body::finish`]. A body that does not fit rolls the reply back and
    /// counts it; one dropped unfinished rolls back and counts nothing,
    /// its writer having found nothing to say.
    pub(crate) fn body(self, frame: StringReply) -> Body<'o, 'b> {
        let start = self.out.len;
        let mut b = Body {
            out: self.out,
            frame,
            c1: self.c1,
            start,
            fit: Fit::Whole,
            done: Done::No,
        };
        b.put(frame.open(b.c1));
        b
    }
}

/// Whether a body has fitted so far.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    Whole,
    Overflowed,
}

/// Whether [`Body::finish`] has run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Done {
    No,
    Yes,
}

/// A control string's body being written ([`Doors::body`]).
pub(crate) struct Body<'o, 'b> {
    out: &'o mut Outgoing<'b>,
    frame: StringReply,
    c1: C1Transmit,
    start: ByteLen,
    fit: Fit,
    done: Done,
}

impl Body<'_, '_> {
    /// Append `bytes`, or mark the body as not fitting.
    pub(crate) fn put(&mut self, bytes: &[u8]) {
        let at = self.out.len.index();
        match self.out.buf.get_mut(at..at + bytes.len()) {
            Some(dst) if self.fit == Fit::Whole => {
                dst.copy_from_slice(bytes);
                self.out.len = ByteLen::from_index(at + bytes.len());
            }
            _ => self.fit = Fit::Overflowed,
        }
    }

    /// **Lend the room left, less the terminator's**, to an encoder that
    /// writes into a slice and says how much it wrote, or `None` when it
    /// did not fit.
    pub(crate) fn fill(&mut self, f: impl FnOnce(&mut [u8]) -> Option<ByteLen>) {
        if self.fit == Fit::Overflowed {
            return;
        }
        let at = self.out.len.index();
        let end = self
            .out
            .buf
            .len()
            .saturating_sub(self.frame.close(self.c1).len());
        match self.out.buf.get_mut(at..end).and_then(f) {
            Some(n) => self.out.len = ByteLen::from_index(at + n.index()),
            None => self.fit = Fit::Overflowed,
        }
    }

    /// **The terminator, and whether the reply went in whole.** One that
    /// did not fit is rolled back and counted.
    pub(crate) fn finish(mut self) -> bool {
        self.done = Done::Yes;
        self.put(self.frame.close(self.c1));
        match self.fit {
            Fit::Whole => true,
            Fit::Overflowed => {
                self.out.len = self.start;
                self.out.counts.overflows += 1;
                false
            }
        }
    }
}

impl Drop for Body<'_, '_> {
    /// A body dropped unfinished says nothing: rolled back, not counted.
    fn drop(&mut self) {
        if self.done == Done::No {
            self.out.len = self.start;
        }
    }
}
