//! **Notifications and progress from programs**, wired: `OSC 9`, `OSC 777 ;
//! notify` and kitty's `OSC 99` published for the embedder to deliver,
//! ConEmu's `OSC 9 ; 4` progress kept for it to show, and `OSC 1337 ;
//! RequestAttention` (`baud_domain::NoticeDesk`).
//!
//! **Routed by a prefix, as `52;` is** (`clip.rs`): a kitty chunk's payload
//! may be 4096 bytes, past the 512 `pending` keeps, so once the body is
//! `99;` and its metadata has ended at the second `;`, every byte after it
//! goes to the desk's chunk, decoded as it arrives. The metadata stays in
//! `pending`, read at the payload's start for its encoding and again at the
//! terminator. `CAN`, `SUB` and an `ESC` that is not `ST` reach no
//! terminator, so a cancelled chunk changes nothing; the next chunk's start
//! begins afresh.
//!
//! **Delivery is the embedder's**: whether the window is focused, whether a
//! flood is cut off, whether the user has notifications on. The core
//! publishes what the programs sent ([`crate::Term::notices`]), answers the
//! query and `p=alive` in order with the rest of the read's replies, and
//! writes the reports the embedder's events call for.

use crate::osc::StrKind;
use crate::paint::Paint;
use baud_domain::*;

impl Paint<'_, '_> {
    /// The body is `99;` and its metadata just ended: what follows is the
    /// payload, which goes to the desk's chunk.
    #[cold]
    #[inline(never)]
    pub(crate) fn notice_start(&mut self) {
        self.cold.str_kind = StrKind::Notice;
        let n = self.cold.str_len.index();
        // `99;` and the metadata, the `;` that ended it just kept at `n - 1`.
        let meta = &self.strs.pending[3..n - 1];
        self.strs.notices.chunk_start(self.strs.notice_area, meta);
    }

    /// One payload byte.
    #[cold]
    #[inline(never)]
    pub(crate) fn notice_put(&mut self, b: StreamByte) {
        self.strs
            .notices
            .chunk_put(self.strs.notice_area, b.raw_byte());
    }

    /// The chunk's terminator.
    #[cold]
    #[inline(never)]
    pub(crate) fn notice_end(&mut self) {
        let n = self.cold.str_len.index();
        let meta_end = n - 1;
        let mut meta = [0u8; crate::storage::OSC_MAX];
        let m = meta_end - 3;
        meta[..m].copy_from_slice(&self.strs.pending[3..meta_end]);
        let end = self
            .strs
            .notices
            .chunk_end(self.strs.notice_area, &meta[..m]);
        self.notice_outcome(end);
    }

    /// What a desk's step did, counted or answered.
    pub(crate) fn notice_outcome(&mut self, end: ChunkEnd) {
        match end {
            ChunkEnd::Kept | ChunkEnd::Published => {}
            ChunkEnd::Replied => {
                if let Some(r) = self.strs.notices.take_reply(self.strs.notice_area) {
                    // Copied out first: the reply lives in the area, and the
                    // reply buffer is written through `self`.
                    let mut buf = [0u8; NOTICE_CHUNK_MAX];
                    let len = r.len();
                    buf[..len].copy_from_slice(r);
                    self.push_reply(&[&buf[..len]]);
                }
            }
            // Metadata that is not the text's form is a string this core
            // could not read, counted where any is; a notification refused
            // for want of an area, of support, of room or of content is
            // counted in the desk's own refusals alone, as a refused `OSC 52`
            // write is in the clipboard's.
            ChunkEnd::Refused(NoticeRefusal::Malformed) => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
            ChunkEnd::Refused(_) => {}
        }
    }

    /// `OSC 99` that arrived whole, never routed: an `OSC 99` is routed at
    /// its second `;`, so this is the shape the desk is fed by an embedder
    /// that parses without the core, kept for the same answer.
    pub(crate) fn notice_whole(&mut self, meta: &[u8], payload: &[u8]) {
        let mut m = [0u8; crate::storage::OSC_MAX];
        let ml = meta.len().min(m.len());
        m[..ml].copy_from_slice(&meta[..ml]);
        self.strs
            .notices
            .chunk_start(self.strs.notice_area, &m[..ml]);
        for &b in payload {
            self.strs.notices.chunk_put(self.strs.notice_area, b);
        }
        let end = self.strs.notices.chunk_end(self.strs.notice_area, &m[..ml]);
        self.notice_outcome(end);
    }

    /// `OSC 9`: a notification, a progress report, or a ConEmu form refused.
    /// A message cut at `OSC_MAX` is shown to its last whole character and
    /// ends in `…` ([`shown_cut`]).
    pub(crate) fn osc9(&mut self, o: Osc9<'_>, cut: crate::logging::Body) {
        match o {
            Osc9::Notify(text) => {
                let mut t = [0u8; CUT_SHOWN];
                let n = shown_cut(text, cut, &mut t).index();
                let end =
                    self.strs
                        .notices
                        .simple(self.strs.notice_area, NoticeFrom::Osc9, &t[..n], b"");
                self.notice_outcome(end);
            }
            Osc9::Progress(r) => {
                self.strs.progress = self.strs.progress.after(r);
                self.strs.progress_reports = self.strs.progress_reports.next();
            }
            Osc9::ProgressMalformed | Osc9::ConEmu(_) => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
    }

    /// `OSC 777`: `notify` published, any other extension refused. Cut at
    /// `OSC_MAX`, the part the cut fell in, the body or a title with none,
    /// is shown as `OSC 9`'s message is.
    pub(crate) fn osc777(&mut self, o: Osc777<'_>, cut: crate::logging::Body) {
        match o {
            Osc777::Notify { title, body } => {
                let mut t = [0u8; CUT_SHOWN];
                let whole = crate::logging::Body::Whole;
                let (title_cut, body_cut) = match body.is_empty() {
                    true => (cut, whole),
                    false => (whole, cut),
                };
                let tl = shown_cut(title, title_cut, &mut t).index();
                let bl = shown_cut(body, body_cut, &mut t[tl..]).index();
                let (title, body) = t[..tl + bl].split_at(tl);
                let end = self.strs.notices.simple(
                    self.strs.notice_area,
                    NoticeFrom::Osc777,
                    title,
                    body,
                );
                self.notice_outcome(end);
            }
            Osc777::Malformed | Osc777::Extension => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
    }
}

/// Room for a notification's text and the `…` a cut one ends in.
const CUT_SHOWN: usize = crate::storage::OSC_MAX + ELLIPSIS.len();

/// What a cut notification ends in, U+2026.
const ELLIPSIS: &[u8] = "\u{2026}".as_bytes();

/// **`text` into `out` as a notification shows it**, and how long it is:
/// whole as it came, or cut at `OSC_MAX` -- its last whole character
/// ([`whole_chars`]) and `…`, so a message the user may act on shows that
/// it was cut.
fn shown_cut(text: &[u8], cut: crate::logging::Body, out: &mut [u8]) -> ByteLen {
    let (text, tail) = match cut {
        crate::logging::Body::Whole => (text, &b""[..]),
        crate::logging::Body::Cut => (whole_chars(text), ELLIPSIS),
    };
    let n = text.len();
    out[..n].copy_from_slice(text);
    out[n..n + tail.len()].copy_from_slice(tail);
    ByteLen::from_index(n + tail.len())
}

/// A count of the reports a terminal has taken, as a reading to compare: an
/// embedder that reads it after a read and finds it moved reads the latest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReportCount(u32);

impl ReportCount {
    pub(crate) const FIRST: ReportCount = ReportCount(0);

    pub(crate) fn next(self) -> ReportCount {
        ReportCount(self.0.wrapping_add(1))
    }
}
