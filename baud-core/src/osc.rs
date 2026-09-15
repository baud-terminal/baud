//! Operating System Commands: titles, the palette, hyperlinks, the clipboard.
//!
//! **The parsing is `baud_domain::Osc` and both cores call it.** What is here
//! is the wiring -- accumulate a body, decide what it was, write the result
//! somewhere the embedder can read -- and that is the half each core has to
//! get right on its own. A core that recognised `OSC 8` and never set the
//! link index would open every URI it was sent and link no cell, and no test
//! of the parser could see it.
//!
//! **The two pinned differentials are structurally blind to all of this.**
//! The glyph differential read content and the rendition one
//! reads the resolved `StyleWord`; a window title changes no cell and a link
//! index rides in `CellFlags`, which neither reads. So the gate for this file
//! was the control-string differential against `baud-core2`, which compared the
//! title, the icon, the palette, the link index of a written cell, the URI it
//! resolves to, and the two refusal counters -- and it was perturbed both ways
//! before it was believed.

use crate::paint::Paint;
use crate::storage::{LINK_MAX, NLINK, OSC_MAX};
use baud_domain::*;

impl Paint<'_, '_> {
    /// Begin a control string: record which one, and empty the accumulator.
    ///
    /// **Reset here, at the introducer, and not at the terminator.** A string
    /// spans reads, so the accumulator outlives a `feed`; and a string that
    /// `CAN` or `SUB` cancels never reaches a terminator at all. Resetting
    /// where the string starts is one rule that covers both, and the
    /// alternative -- reset on completion, plus a second rule for cancellation
    /// -- is the shape that made `pre: Option<Classified>` break chunk
    /// invariance: a value carried between two functions and consumed by a
    /// later, different one, because the path has more exits than it looks
    /// like it has.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_start(&mut self, b: StreamByte) {
        self.cold.str_kind = StrKind::of(b);
        self.cold.str_len = ByteLen::ZERO;
    }

    /// One body byte, truncating rather than growing.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_put(&mut self, b: StreamByte) {
        let n = self.cold.str_len.index();
        if n < OSC_MAX {
            self.strs.pending[n] = b.raw_byte();
            self.cold.str_len = ByteLen::from_index(n + 1);
        } else {
            self.string_truncations += 1;
        }
    }

    /// The string is complete. Act on it if it is one this core acts on.
    ///
    /// Takes the terminating byte, because `[[source: ctlseqs]]` says a reply
    /// "uses the same terminator used in a query" -- so `BEL` and `ST` are one
    /// action with two shapes rather than two actions, and which one arrived
    /// has to travel with it.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_end(&mut self, b: StreamByte) {
        let term = match b.raw_byte() {
            0x07 => StringTerm::Bel,
            _ => StringTerm::St,
        };
        match self.cold.str_kind {
            StrKind::Osc => self.dispatch_osc(term),
            StrKind::Decrqss => {
                let n = self.cold.str_len.index();
                let target = StatusTarget::parse(&self.strs.pending[..n]);
                self.report_status(target);
            }
            // `SOS`, `PM` and `APC` have no defined content here, and `DCS`
            // has no prefix parser yet. **Consumed is the part that matters**:
            // a body that reached the screen as text would render an
            // application's private protocol as glyphs.
            StrKind::Other => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
        self.cold.str_len = ByteLen::ZERO;
    }

    fn dispatch_osc(&mut self, term: StringTerm) {
        let n = self.cold.str_len.index();
        // Split the borrow: `Osc::parse` holds the payload while the setters
        // want the other buffers. Every arm resolves to owned values first.
        match Osc::parse(&self.strs.pending[..n]) {
            // **No trailing-NUL trim, because a NUL cannot get here.**
            // `baud-core2` carries a `clip` helper for emitters that put a NUL
            // before the real terminator, and this core had one too until the
            // parser started ignoring C0 in the payload: `graph::string` drops
            // `00-06` outright, so there is no trailing NUL left to trim.
            // Neutering the helper changed no row of the differential, which
            // is a function that cannot fire -- the shape this repo has now
            // recorded seven times.
            Osc::SetTitle(t) => self.title_len = copy_bounded(self.strs.title, t),
            Osc::SetIcon(t) => self.icon_len = copy_bounded(self.strs.icon, t),
            Osc::SetIconAndTitle(t) => {
                self.title_len = copy_bounded(self.strs.title, t);
                self.icon_len = copy_bounded(self.strs.icon, t);
            }
            Osc::SetColor { index, spec } => match spec {
                XColor::Rgb(r, g, b) => {
                    self.strs.palette[index.index()] = ColorWord::rgb(r, g, b);
                }
                XColor::Query => self.report_color(index, term),
                // A colour *name*. Resolving it needs X11's `rgb.txt` and this
                // repo has no citable copy, so it is left alone rather than
                // set to something plausible-but-wrong -- which is the same
                // reason the DEC line-drawing table was refused until a
                // machine-readable source was cited.
                XColor::Unresolved(_) => {}
            },
            Osc::ResetColor(Some(c)) => self.strs.palette[c.index()] = ColorWord::DEFAULT,
            Osc::ResetColor(None) => self.strs.palette.fill(ColorWord::DEFAULT),
            // **Inline, because the URI borrows the payload buffer.** Both
            // live in `strs`, so resolving the slot and the length first is
            // what lets the write happen at all -- the same borrow split
            // `baud-core2` makes here for the same reason.
            Osc::Hyperlink { id, uri } => {
                let n = uri.len().min(LINK_MAX);
                if n == 0 {
                    self.link = LinkId::NONE;
                } else {
                    let slot = LinkId::bucket(id, uri, NLINK);
                    let at = slot.index() * LINK_MAX;
                    let mut tmp = [0u8; LINK_MAX];
                    tmp[..n].copy_from_slice(&uri[..n]);
                    self.strs.links[at..at + n].copy_from_slice(&tmp[..n]);
                    self.strs.link_len[slot.index()] = ByteLen::from_index(n);
                    self.link = slot;
                }
                self.refresh_flags();
            }
            // Refused, and **counted**. Silence is the right answer to the
            // application -- any reply, empty included, tells a remote program
            // the feature works and it will keep asking, and what it would
            // read back is whatever the user last copied. Being unable to tell
            // that one asked is a different thing, and not a feature.
            Osc::Clipboard(ClipboardOp::Read) => self.clipboard_reads_refused += 1,
            Osc::Clipboard(_) => self.clipboard_writes_dropped += 1,
            // Both counters: `strings_ignored` is what an embedder reads to
            // find OSC codes it might want to implement, and
            // `unsupported_total` is the one the conformance inventory asks.
            // They answer different questions and the second was missing.
            // **The one thing these marks are acted on for.** A shell
            // redraws its prompt after a resize by counting the rows it
            // occupies and erasing that many; re-cutting those rows to a
            // new width changes the count behind its back, so the erase
            // begins below the prompt's first line and that line
            // survives. Output has no such reader, and re-cutting it is
            // the part of a reflow worth keeping. `Term::resize` is what
            // reads this, and the note in `Zone` is why.
            Osc::Semantic(mark) => {
                self.cold.zone = match mark {
                    Semantic::PromptStart | Semantic::CommandStart => crate::term::Zone::Prompt,
                    Semantic::OutputStart => crate::term::Zone::Output,
                    // `D` says the command finished; the shell's next act
                    // is a prompt, which sends its own mark. Nothing is
                    // assumed here in between.
                    Semantic::CommandDone | Semantic::Other => self.cold.zone,
                };
                self.semantic_marks += 1;
            }
            Osc::Unsupported => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
    }

    /// Answer `OSC 4 ; c ; ?` in the same shape as the setter.
    ///
    /// `[[source: ctlseqs]]`: xterm "replies with a control sequence of the
    /// same form which can be used to set the corresponding color".
    ///
    /// **An entry this core never set gets no reply at all**, because there is
    /// no built-in palette to report. The mapping from index to pixels belongs
    /// to the renderer, and a second copy here would be one to keep in step --
    /// so answering would name a colour the renderer does not use, which is
    /// the wrong-shaped answer `docs/vt-coverage.md` forbids.
    fn report_color(&mut self, index: Chan, term: StringTerm) {
        let Some((r, g, b)) = self.strs.palette[index.index()].as_rgb() else {
            return;
        };
        self.want_reply(Report::Color {
            index,
            r,
            g,
            b,
            term,
        });
    }
}

fn copy_bounded(dst: &mut [u8], src: &[u8]) -> ByteLen {
    let n = src.len().min(OSC_MAX);
    dst[..n].copy_from_slice(&src[..n]);
    ByteLen::from_index(n)
}

/// Which control string is being accumulated.
///
/// Three states rather than five. `SOS`, `PM` and `APC` share `Other` because
/// neither core acts on any of them and their bodies are consumed identically;
/// `Decrqss` is separate because it is reached through a **prefix** -- `$ q`
/// after the introducer -- rather than through the introducer alone, so
/// `StrKind::of` cannot name it and `Act::DcsQuery` sets it two bytes later.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StrKind {
    Osc,
    /// A `DCS $ q`, which is the one DCS either core acts on.
    Decrqss,
    Other,
}

impl StrKind {
    pub(crate) const fn of(b: StreamByte) -> StrKind {
        match b.raw_byte() {
            // **Both spellings of the same introducer.** `ESC ]` is the
            // seven-bit form and `9D` the eight-bit one, and the graph reaches
            // the same node from either -- a Latin-1 `9D` is routed as
            // `escape(9D - 0x40)`, which is `]`. But this reads the byte back
            // out of the arena to name the kind, so it saw `9D` and answered
            // `Other`: the string parsed, its body was consumed, and the title
            // was silently dropped.
            //
            // The graph's delegation is what makes one row serve both, and
            // this is the one place downstream that still had to be told.
            0x5d | 0x9d => StrKind::Osc,
            _ => StrKind::Other,
        }
    }
}
