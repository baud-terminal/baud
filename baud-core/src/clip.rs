//! `OSC 52` writes: the clipboard body, streamed past `OSC_MAX` into its own
//! areas, decoded as it arrives, and published at the terminator for the
//! embedder to read (note V, V11).
//!
//! **Routed by a prefix, not a graph action.** Every other string kind is
//! named by the graph -- `Act::DcsQuery` and `Act::SixelStart` in `write.rs`
//! -- because each is told apart before its body begins. An OSC's number is
//! its body's first bytes, so `str_put` names this one when the third byte
//! makes the body `52;` and hands every byte after it here. Cancellation needs
//! no rule of its own: `CAN`, `SUB` and `ESC` start no new string and reach no
//! terminator, so nothing is published, and the next `52;` starts afresh.
//!
//! **Two areas**, because the embedder polls after each read and a second
//! copy may begin in the read the first one ended in (note V, V11 design 2).
//! The first half of [`StrBuf::clip_area`] collects the copy in flight; the
//! second holds the last one finished, written only at a terminator that
//! publishes.
//!
//! **What is refused and what is cleared.** A write is refused whole and
//! counted -- never cut -- when the storage has no area, when `Pc` names no
//! selection, and when the copy outgrows the area. A `Pd` that is neither
//! base64 nor `?` is not refused: `[[source: ctlseqs]]` says "the selection is
//! cleared", and a clear is published like any write.
//!
//! [`StrBuf::clip_area`]: crate::storage::StrBuf

use crate::paint::Paint;
use baud_domain::*;

/// A count of the `OSC 52` writes the terminal has published, as a reading
/// to compare: an embedder that reads it after a read and finds it moved
/// reads [`crate::Term::clipboard`] for the latest. Two writes finished in one
/// read move it by two and leave the second, which is what the selection
/// would hold after both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClipboardWrites(u32);

impl ClipboardWrites {
    pub(crate) const FIRST: ClipboardWrites = ClipboardWrites(0);

    fn next(self) -> ClipboardWrites {
        ClipboardWrites(self.0.wrapping_add(1))
    }
}

/// What the last published write left: which selections, what kind, and how
/// many octets of the second area are its.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shown {
    Nothing,
    Set(Selections, ReadLen),
    Cleared(Selections),
}

/// The clipboard's state, in [`StrBuf`] beside its areas: the string in
/// flight, the octets it has decoded so far, what was last published, and the
/// counts.
///
/// **Not in `Cold`**, whose size is pinned and which the per-glyph write
/// dereferences: this is written only inside a clipboard string, and
/// `StrBuf` is reached only through a control string. Moving it here was not
/// what took `ingest2`'s `cat` back to base, though: with the state in either
/// struct the per-byte loop was instruction for instruction base's, and the
/// cost followed the decoder's placement (`Base64::push`; `docs/record.md`,
/// "OSC 52 writes reach the pasteboard").
///
/// [`StrBuf`]: crate::storage::StrBuf
#[derive(Clone, Copy)]
pub(crate) struct Clip {
    /// `Pc ; Pd` after the `52;`, with the base64 decoder's carry.
    osc: Osc52,
    /// Octets decoded so far. Keeps rising past the area, as `sixel_len`
    /// does, so a copy that outgrew it is seen whole at the terminator.
    len: ReadLen,
    shown: Shown,
    writes: ClipboardWrites,
    refusals: ClipboardRefusals,
}

impl Clip {
    pub(crate) const NEW: Clip = Clip {
        osc: Osc52::NEW,
        len: ReadLen::ZERO,
        shown: Shown::Nothing,
        writes: ClipboardWrites::FIRST,
        refusals: ClipboardRefusals::NONE,
    };

    pub(crate) fn writes(&self) -> ClipboardWrites {
        self.writes
    }

    pub(crate) fn refusals(&self) -> ClipboardRefusals {
        self.refusals
    }

    /// The last write published, read from the second half of `area`.
    pub(crate) fn shown<'a>(&self, area: &'a [u8]) -> Option<ClipboardWrite<'a>> {
        let done = &area[area.len() / 2..];
        match self.shown {
            Shown::Nothing => None,
            Shown::Set(targets, len) => Some(ClipboardWrite {
                targets,
                content: ClipboardContent::Set(&done[..len.index()]),
            }),
            Shown::Cleared(targets) => Some(ClipboardWrite {
                targets,
                content: ClipboardContent::Cleared,
            }),
        }
    }
}

impl Paint<'_, '_> {
    /// The body just became `52;`: what follows is a clipboard string.
    #[cold]
    #[inline(never)]
    pub(crate) fn clip_start(&mut self) {
        self.cold.str_kind = crate::osc::StrKind::Clipboard;
        self.strs.clip.osc = Osc52::NEW;
        self.strs.clip.len = ReadLen::ZERO;
    }

    /// One body byte after `52;`.
    #[cold]
    #[inline(never)]
    pub(crate) fn clip_put(&mut self, b: StreamByte) {
        if let Base64Step::Octets(o) = self.strs.clip.osc.push(b.raw_byte()) {
            self.clip_keep(o);
        }
    }

    /// Decoded octets into the collecting half, counted past its end.
    fn clip_keep(&mut self, o: Octets) {
        let half = self.strs.clip_area.len() / 2;
        for &x in o.as_slice() {
            let n = self.strs.clip.len.index();
            if n < half {
                self.strs.clip_area[n] = x;
            }
            self.strs.clip.len = ReadLen::from_index(n + 1);
        }
    }

    /// The terminator: publish the write, refuse it, or count the read.
    #[cold]
    #[inline(never)]
    pub(crate) fn clip_end(&mut self) {
        let (end, tail) = self.strs.clip.osc.finish();
        self.clip_keep(tail);
        let half = self.strs.clip_area.len() / 2;
        let len = self.strs.clip.len;
        let (targets, shown) = match end {
            // `52;c` with no second `;`: not ctlseqs' form, so counted where
            // any OSC this core does not act on is.
            Osc52End::Malformed => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
                return;
            }
            // Silence, and counted. Any reply, empty included, tells a remote
            // program the feature works, and what it would read back is
            // whatever the user last copied.
            Osc52End::Read(_) => {
                self.clipboard_reads_refused += 1;
                return;
            }
            Osc52End::Write(t) => (t, Shown::Set(t, len)),
            Osc52End::Clear(t) => (t, Shown::Cleared(t)),
        };
        let refused = if half == 0 {
            Some(ClipboardRefusal::Unprovisioned)
        } else if !targets.names_any() {
            Some(ClipboardRefusal::NoSelection)
        } else if matches!(shown, Shown::Set(..)) && len.index() > half {
            Some(ClipboardRefusal::Body(len))
        } else {
            None
        };
        if let Some(why) = refused {
            self.strs.clip.refusals.count(why);
            self.clipboard_writes_refused += 1;
            return;
        }
        if let Shown::Set(_, n) = shown {
            let (collect, done) = self.strs.clip_area.split_at_mut(half);
            done[..n.index()].copy_from_slice(&collect[..n.index()]);
        }
        self.strs.clip.shown = shown;
        self.strs.clip.writes = self.strs.clip.writes.next();
    }
}
