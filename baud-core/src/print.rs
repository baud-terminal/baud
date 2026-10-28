//! Printing: Media Copy, autoprint, printer controller mode, DECPFF and
//! DECPEX, `[[source: dec-std-070]]` 070-7 (the Printer Port Extension) and
//! the VT420's chapter 11 (`[[source: vt420-rm]]`, pp. 209-215).
//!
//! **The core writes 070-7's print stream and hands it over as jobs; the
//! embedder is the printer.** It says whether there is one
//! ([`PrinterPort`]), and 070-7 § 7.3.2's three states follow: ready while
//! there is; "not ready" while there is none but there was one since
//! power-up or RIS, a print then collected as a job, "print operations
//! shall be queued"; "not available" while there has been none, every
//! print function then ignored and counted, "all print commands which
//! require the printer port are ignored". DSR `?15` answers `?10n`, `?11n`
//! or `?13n` to match.
//!
//! **What a job holds**: the stream 070-7 § 7.4 and PRINT PAGE's algorithm
//! (pp. 7-39 to 7-47) give, `ESC [ 0 m` at the start, `ESC # 5`, `6`, `3` or
//! `4` at each line, `ESC [ 0 ; Ps ... m` when the rendition changes with
//! `Ps` 1 bold, 4 underscore, 5 blink and 7 reverse (§ 7.4.2), invisible
//! characters as spaces, trailing spaces of the normal rendition left off (§
//! 7.4.3), `CR LF` after each line, `FF` under DECPFF and `ESC [ 0 m` at the
//! end -- with the characters in UTF-8 and no character-set designations,
//! since a cell holds Unicode, which none of § 7.4's three printer styles
//! can carry. The VT420's p. 215 lists SGR 2 for underline; STD 070's 4 is
//! followed. A printer controller run's bytes are the host's own, under ISO
//! 8859-1 carried into UTF-8 so a job is always UTF-8.
//!
//! **What one job is**: a print screen, page, line or all pages; an
//! autoprint run from `CSI ? 5 i` to `CSI ? 4 i`, a print in the middle of
//! it ending it there and starting another after (an autoprint run is one
//! job from `CSI ? 5 i` to `CSI ? 4 i`, or to the next print, or `RIS`); a
//! controller run from `CSI 5 i` to `CSI 4 i`. Finished jobs wait in the
//! area for [`crate::Term::print_jobs`], at most [`PRINT_QUEUE`] of them,
//! one past that joined to the last after a form feed, so nothing printed is
//! lost to a slow embedder.
//!
//! **The area is bounded**, the embedder's [`PrintBudget`]: bytes past it are
//! dropped from the job collecting them, counted, and the job marked
//! [`PrintCut::Cut`]. A controller run stays a run past the bound, its output
//! hidden until `CSI 4 i` as 070-7 § 7.3.6 says, where ending it there would
//! draw the program's printer data on the screen.
//!
//! **Printer controller mode is a node of the graph** (`Node::Printing`),
//! every byte `Act::Printer`, whose step takes the rest of the read here: no
//! other byte's path tests anything. Autoprint is one bit of the mode word
//! `LF` already reads for LNM (`ModeBit::AutoPrint`).

use crate::paint::Paint;
use baud_domain::*;

/// The longest control sequence a controller run holds back while it
/// decides whether it is `CSI 4 i`: `ESC [`, a parameter's digits and the
/// final. Longer, it was never `CSI 4 i` or `CSI 5 i`, and goes to the
/// printer.
const MATCH_MAX: usize = 16;

/// The most UTF-8 a cell's marks take: four bytes each, as many as a
/// cluster slot holds, and the two a placeholder's diacritics take fit.
const MARKS_BYTES: usize = 4 * ClusterSlot::CAPACITY;

/// The printer's state, in `StrBuf` beside its area: the port, the two
/// modes, the run, the job being collected and the jobs finished.
#[derive(Clone, Copy)]
pub(crate) struct Printer {
    pub(crate) port: PrinterPort,
    /// Whether the port has had a printer since power-up or RIS.
    seen: Seen,
    pub(crate) form_feed: PrintFormFeed,
    pub(crate) extent: PrintExtent,
    /// DECGEPM, `?43`.
    pub(crate) size: PrintSize,
    /// The other four graphics print choices, the user's.
    pub(crate) graphics: GraphicsPrint,
    pub(crate) run: PrintRun,
    open: Collecting,
    done: [Option<Done>; PRINT_QUEUE],
    matcher: Matcher,
    pub(crate) refusals: PrintRefusals,
    /// Whether a DSR `?15` stops the read, and whether one is owed.
    pub(crate) hold: PrinterHold,
    pub(crate) asked: PrinterAsked,
}

/// Whether the port has had a printer since power-up or RIS, which tells
/// "not ready" from "not available" (070-7 § 7.3.2).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seen {
    Never,
    Since,
}

/// 070-7 § 7.3.2's printer status: "PRINTER_READY, PRINTER_NOT_READY,
/// PRINTER_NOT_AVAILABLE".
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Ready,
    NotReady,
    NotAvailable,
}

/// The job being collected, which starts where the last finished one ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Collecting {
    Nothing,
    Job {
        /// The bytes it holds in the area.
        len: ReadLen,
        cut: PrintCut,
        join: Join,
    },
}

/// Whether a job finishes as one of its own or joins the last finished,
/// the queue being full when it began.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    Own,
    Last,
}

/// A finished job: where it ends in the area, its start being the end of
/// the one before.
#[derive(Clone, Copy)]
struct Done {
    end: ReadLen,
    cut: PrintCut,
}

/// How far a controller run has got into what may be `CSI 4 i`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Match {
    Idle,
    Esc,
    Csi,
}

#[derive(Clone, Copy)]
struct Matcher {
    at: Match,
    held: [u8; MATCH_MAX],
    len: ByteLen,
}

impl Matcher {
    const IDLE: Matcher = Matcher {
        at: Match::Idle,
        held: [0; MATCH_MAX],
        len: ByteLen::ZERO,
    };
}

impl Printer {
    pub(crate) const NONE: Printer = Printer {
        port: PrinterPort::Absent,
        seen: Seen::Never,
        form_feed: PrintFormFeed::Off,
        extent: PrintExtent::ScrollingRegion,
        size: PrintSize::Compressed,
        graphics: GraphicsPrint::DEFAULT,
        run: PrintRun::Idle,
        open: Collecting::Nothing,
        done: [None; PRINT_QUEUE],
        matcher: Matcher::IDLE,
        refusals: PrintRefusals::NONE,
        hold: PrinterHold::Off,
        asked: PrinterAsked::No,
    };

    /// The embedder's printer, which a printer that is there marks seen.
    pub(crate) fn set_port(&mut self, port: PrinterPort) {
        self.port = port;
        if port == PrinterPort::Ready {
            self.seen = Seen::Since;
        }
    }

    /// 070-7 § 7.3.2's state, from the port and whether it was seen.
    pub(crate) fn status(&self) -> Status {
        match (self.port, self.seen) {
            (PrinterPort::Ready, _) => Status::Ready,
            (PrinterPort::Absent, Seen::Since) => Status::NotReady,
            (PrinterPort::Absent, Seen::Never) => Status::NotAvailable,
        }
    }

    /// What DSR `?15` answers now.
    pub(crate) fn reply(&self) -> DecReply {
        match self.status() {
            Status::Ready => DecReply::PrinterReady,
            Status::NotReady => DecReply::PrinterNotReady,
            Status::NotAvailable => DecReply::NoPrinter,
        }
    }

    /// Where the job being collected starts: the end of the last finished.
    fn base(&self) -> ReadLen {
        self.done
            .iter()
            .rev()
            .find_map(|d| d.map(|d| d.end))
            .unwrap_or(ReadLen::ZERO)
    }

    /// Where the job being collected ends in the area: where a hard copy
    /// writes its dump. `None` with none open.
    pub(crate) fn open_end(&self) -> Option<ReadLen> {
        match self.open {
            Collecting::Job { len, .. } => Some(self.base().plus(len)),
            Collecting::Nothing => None,
        }
    }

    /// `n` more bytes written to the job being collected past
    /// [`Printer::open_end`], and `lost` more that did not fit, which cut
    /// it.
    pub(crate) fn wrote(&mut self, n: ReadLen, lost: ReadLen) {
        if let Collecting::Job { len, cut, .. } = &mut self.open {
            *len = len.plus(n);
            if !lost.is_zero() {
                *cut = PrintCut::Cut;
                let l = u64::try_from(lost.index()).unwrap_or(u64::MAX);
                self.refusals.dropped = self.refusals.dropped.saturating_add(l);
            }
        }
    }

    /// The finished jobs, oldest first, read from `area`.
    pub(crate) fn jobs<'a>(&'a self, area: &'a [u8]) -> impl Iterator<Item = PrintJob<'a>> + 'a {
        let mut start = ReadLen::ZERO;
        self.done.iter().map_while(move |d| {
            let d = (*d)?;
            let job = PrintJob {
                bytes: &area[start.index()..d.end.index()],
                cut: d.cut,
            };
            start = d.end;
            Some(job)
        })
    }

    /// The job being collected finished: queued, joined to the last, or,
    /// holding nothing, forgotten -- an autoprint run that printed no line
    /// opens no panel.
    fn finish(&mut self) {
        let Collecting::Job { len, cut, join } = self.open else {
            return;
        };
        self.open = Collecting::Nothing;
        let empty = match join {
            Join::Own => len.is_zero(),
            Join::Last => len.index() <= 1,
        };
        if empty {
            return;
        }
        let end = self.base().plus(len);
        match join {
            Join::Own => {
                if let Some(slot) = self.done.iter_mut().find(|d| d.is_none()) {
                    *slot = Some(Done { end, cut });
                }
            }
            Join::Last => {
                if let Some(last) = self.done.iter_mut().rev().find_map(|d| d.as_mut()) {
                    last.end = end;
                    if cut == PrintCut::Cut {
                        last.cut = PrintCut::Cut;
                    }
                }
                self.refusals.merged += 1;
            }
        }
    }

    /// Forget the finished jobs, the embedder having taken them, and move
    /// the one being collected to the front of `area`.
    pub(crate) fn clear(&mut self, area: &mut [u8]) {
        let base = self.base();
        if let Collecting::Job { len, .. } = self.open {
            area.copy_within(base.index()..base.index() + len.index(), 0);
        }
        self.done = [None; PRINT_QUEUE];
        if let Collecting::Job { join, .. } = &mut self.open {
            *join = Join::Own;
        }
    }
}

/// The ending a printed line takes: `CR LF` for a print, `CR` and the
/// character that moved the cursor off it for autoprint (070-7 § 7.3.7,
/// "terminated by a Carriage Return (CR), followed by the same character
/// that initiated the print operation (LF, VT, FF, or LF if Auto Wrap)").
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineEnd {
    LineFeed,
    LineTab,
    FormFeed,
}

impl LineEnd {
    fn byte(self) -> StreamByte {
        match self {
            LineEnd::LineFeed => StreamByte::LF,
            LineEnd::LineTab => StreamByte::VT,
            LineEnd::FormFeed => StreamByte::FF,
        }
    }
}

/// Which rows a print of the page takes: DECPEX's, or the whole screen,
/// which print screen takes whatever DECPEX says.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PageRows {
    ByExtent,
    Screen,
}

/// Which columns of a line a print takes: the whole line, or under DECPEX
/// reset the side margins, as 070-7's PRINT LINE says of DECSLRM's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Span {
    Line,
    Margins,
}

/// The four attributes 070-7 § 7.4.2 sends, as they stand.
const PRINTED: Attrs = Attrs::BOLD
    .with(Attrs::UNDERLINES)
    .with(Attrs::BLINKS)
    .with(Attrs::REVERSE);

impl Paint<'_, '_> {
    /// MC, `CSI Ps i` and `CSI ? Ps i`, and VT52's six print commands.
    #[cold]
    #[inline(never)]
    pub(crate) fn media_copy(&mut self, m: MediaCopy, node: &mut crate::graph::Node) {
        if m == MediaCopy::Refused {
            self.strs.print.refusals.unsafe_kinds += 1;
            return;
        }
        if self.strs.print.status() == Status::NotAvailable {
            self.strs.print.refusals.no_printer += 1;
            return;
        }
        match m {
            MediaCopy::PrintPage => self.print_page(PageRows::ByExtent),
            MediaCopy::PrintAllPages => self.print_page(PageRows::ByExtent),
            MediaCopy::PrintScreen => self.print_page(PageRows::Screen),
            MediaCopy::PrintLine => {
                self.end_run_job();
                self.begin_job();
                let span = match self.strs.print.extent {
                    PrintExtent::Page => Span::Line,
                    PrintExtent::ScrollingRegion => Span::Margins,
                };
                self.put(b"\x1b[0m");
                self.print_rows(self.cy, self.cy, span, LineEnd::LineFeed);
                self.put(b"\x1b[0m");
                self.finish_job();
                self.resume_run_job();
            }
            MediaCopy::AutoPrintOn => {
                if self.strs.print.run == PrintRun::Idle {
                    self.strs.print.run = PrintRun::AutoPrint;
                    self.cold.modes.set(ModeBit::AutoPrint);
                    self.begin_job();
                }
            }
            MediaCopy::AutoPrintOff => {
                if self.strs.print.run == PrintRun::AutoPrint {
                    self.strs.print.run = PrintRun::Idle;
                    self.cold.modes.reset(ModeBit::AutoPrint);
                    self.finish_job();
                }
            }
            // "Printer controller mode cancels autoprint mode" (`[[source:
            // vt420-rm]]` p. 212): an autoprint run's job ends here and the
            // controller run's begins.
            MediaCopy::ControllerOn => {
                if self.strs.print.run == PrintRun::AutoPrint {
                    self.cold.modes.reset(ModeBit::AutoPrint);
                    self.finish_job();
                }
                self.strs.print.run = PrintRun::Controller;
                self.strs.print.matcher = Matcher::IDLE;
                self.begin_job();
                *node = crate::graph::Node::Printing;
            }
            // Outside a run, reached only from the screen: inside one the
            // run's own step takes it (`Paint::printer_bytes`).
            MediaCopy::ControllerOff => {}
            // A hard copy always goes to the printer (`hardcopy.rs`).
            MediaCopy::GraphicsToPrinter => {}
            MediaCopy::Refused => {}
        }
    }

    /// Print page (`CSI 0 i`), all pages (`CSI ? 11 i`, baud's page memory
    /// being the one page) and the composed display (`CSI ? 10 i`, which
    /// DECPEX does not affect), each `FF`-ended under DECPFF.
    fn print_page(&mut self, rows: PageRows) {
        let (top, bottom) = match (rows, self.strs.print.extent) {
            (PageRows::ByExtent, PrintExtent::ScrollingRegion) => (self.cold.top, self.cold.bot),
            _ => (Row::ZERO, Row::from_index(self.rows.index() - 1)),
        };
        self.end_run_job();
        self.begin_job();
        self.put(b"\x1b[0m");
        self.print_rows(top, bottom, Span::Line, LineEnd::LineFeed);
        if self.strs.print.form_feed == PrintFormFeed::On {
            self.put(b"\x0c");
        }
        self.put(b"\x1b[0m");
        self.finish_job();
        self.resume_run_job();
    }

    /// A print in the middle of an autoprint run ends the run's job there.
    pub(crate) fn end_run_job(&mut self) {
        if self.strs.print.run == PrintRun::AutoPrint {
            self.finish_job();
        }
    }

    /// And the run goes on into a job of its own after the print.
    pub(crate) fn resume_run_job(&mut self) {
        if self.strs.print.run == PrintRun::AutoPrint {
            self.begin_job();
        }
    }

    /// **Autoprint**: the cursor's line, leaving by `LF`, `VT`, `FF` or an
    /// autowrap, printed with the whole of 070-7 § 7.4's initialisation and
    /// closing ("the complete initialization / transmission / closing
    /// sequence is used for each line printed"), ended by `CR` and `end`.
    #[cold]
    #[inline(never)]
    pub(crate) fn autoprint(&mut self, end: LineEnd) {
        if self.strs.print.run != PrintRun::AutoPrint {
            return;
        }
        self.put(b"\x1b[0m");
        self.print_rows(self.cy, self.cy, Span::Line, end);
        self.put(b"\x1b[0m");
    }

    /// Rows `top` to `bottom` of the screen shown, each with its line
    /// attribute, its characters and their renditions, and `CR` and `end`.
    fn print_rows(&mut self, top: Row, bottom: Row, span: Span, end: LineEnd) {
        let mut now = Attrs::NONE;
        for y in top.index()..=bottom.index() {
            let r = Row::from_index(y);
            let rendition = self.view.rendition(r);
            self.put(match rendition {
                LineRendition::Single => b"\x1b#5",
                LineRendition::DoubleWidth => b"\x1b#6",
                LineRendition::DoubleTop => b"\x1b#3",
                LineRendition::DoubleBottom => b"\x1b#4",
            });
            let line_end = rendition.end_of_line(self.cols).index();
            let (left, right) = match span {
                Span::Line => (0, line_end),
                Span::Margins => (
                    self.cold.left.index(),
                    (self.cold.right.index() + 1).min(line_end),
                ),
            };
            let mut last = right;
            while last > left && self.trailing(r, Col::at_index(last - 1)) {
                last -= 1;
            }
            for x in left..last {
                let cell = self.view.get(r, Col::at_index(x));
                let attrs = self.word_of(cell.style).attrs();
                let base = self.clusters.base_of(cell.content);
                let hidden = attrs.has(Attrs::HIDDEN) || self.excluded(cell);
                if base.is_wide_tail() && !hidden {
                    continue;
                }
                let shown = attrs.only(PRINTED);
                if shown != now {
                    self.put_rendition(shown);
                    now = shown;
                }
                // A combining mark stored alone prints as the mark, as a
                // copy returns it (`Content::DEFECTIVE`).
                let defective = base == Content::DEFECTIVE;
                match (hidden, base.as_codepoint()) {
                    (false, cp) if cp.is_some() || defective => {
                        // Every stored codepoint is a `char`; a mark that
                        // were not would print as U+FFFD.
                        if let Some(cp) = cp {
                            let c = char::from_u32(cp.value());
                            self.put_char(c.unwrap_or(char::REPLACEMENT_CHARACTER));
                        }
                        let mut marks = [0u8; MARKS_BYTES];
                        let mut n = 0;
                        for m in self.clusters.marks_of(cell.content) {
                            let c = char::from_u32(m).unwrap_or(char::REPLACEMENT_CHARACTER);
                            n += c.encode_utf8(&mut marks[n..]).len();
                        }
                        self.put(&marks[..n]);
                    }
                    _ => self.put(b" "),
                }
            }
            self.put(&[b'\r', end.byte().raw_byte()]);
        }
    }

    /// Whether the cell at `(r, x)` is a trailing space: blank, or a space,
    /// with none of the four printed attributes (070-7 § 7.4.3).
    fn trailing(&self, r: Row, x: Col) -> bool {
        let cell = self.view.get(r, x);
        let base = self.clusters.base_of(cell.content);
        let blank = base.is_empty() || base.as_codepoint().is_some_and(|c| c.value() == 0x20);
        (blank || self.excluded(cell))
            && self.word_of(cell.style).attrs().only(PRINTED) == Attrs::NONE
    }

    /// **Whether a print leaves out a cell's contents**: a guarded position
    /// while GATM is GUARD, its reset state. "Only the contents of
    /// unguarded areas in an eligible area are transmitted or transferred"
    /// (`[[source: ecma48]]` 7.2.7), a guarded area's contents "excluded
    /// ... from transfer to an auxiliary input/output device" (6.5.2.2),
    /// which MC initiates (6.6). Each such position prints as a space, as
    /// an invisible character does, so the columns after it keep their
    /// places; every guarded position a print reaches is left out, an area
    /// only partly inside it too, which 7.3.1 g leaves to the
    /// implementation; and every print of screen contents is a transfer,
    /// page, screen, all pages, line and autoprint.
    fn excluded(&self, cell: PackedCell) -> bool {
        cell.flags.is_guarded() && !self.cold.modes.is_set(ModeBit::TransferGuarded)
    }

    /// `ESC [ 0 ; Ps ... m` for the attributes now in force, 070-7's SEND
    /// GRAPHIC RENDITION.
    fn put_rendition(&mut self, a: Attrs) {
        self.put(b"\x1b[0");
        if a.has(Attrs::BOLD) {
            self.put(b";1");
        }
        if a.only(Attrs::UNDERLINES) != Attrs::NONE {
            self.put(b";4");
        }
        if a.only(Attrs::BLINKS) != Attrs::NONE {
            self.put(b";5");
        }
        if a.has(Attrs::REVERSE) {
            self.put(b";7");
        }
        self.put(b"m");
    }

    fn put_char(&mut self, c: char) {
        let mut b = [0u8; 4];
        self.put(c.encode_utf8(&mut b).as_bytes());
    }

    /// A job begins where the last finished one ends; with the queue full
    /// it will join that one, after a form feed.
    pub(crate) fn begin_job(&mut self) {
        let p = &mut self.strs.print;
        let join = match p.done.iter().all(Option::is_some) {
            true => Join::Last,
            false => Join::Own,
        };
        p.open = Collecting::Job {
            len: ReadLen::ZERO,
            cut: PrintCut::Whole,
            join,
        };
        if join == Join::Last {
            self.put(b"\x0c");
        }
    }

    pub(crate) fn finish_job(&mut self) {
        self.strs.print.finish();
    }

    /// Append `bytes` to the job being collected, as far as the area holds
    /// them; the rest are dropped and counted, and the job is cut.
    pub(crate) fn put(&mut self, bytes: &[u8]) {
        let s = &mut *self.strs;
        let base = s.print.base();
        let Collecting::Job { len, cut, .. } = &mut s.print.open else {
            return;
        };
        let at = base.index() + len.index();
        let room = s.print_area.len().saturating_sub(at);
        let n = bytes.len().min(room);
        s.print_area[at..at + n].copy_from_slice(&bytes[..n]);
        *len = len.plus(ReadLen::from_index(n));
        if n < bytes.len() {
            *cut = PrintCut::Cut;
            let lost = u64::try_from(bytes.len() - n).unwrap_or(u64::MAX);
            s.print.refusals.dropped = s.print.refusals.dropped.saturating_add(lost);
        }
    }

    /// A byte of a controller run: as received under UTF-8, carried into
    /// UTF-8 under ISO 8859-1.
    fn put_received(&mut self, b: StreamByte) {
        match (self.cold.encoding, b.raw_byte() < 0x80) {
            (InputEncoding::Latin1, false) => self.put_char(char::from(b.raw_byte())),
            _ => self.put(&[b.raw_byte()]),
        }
    }

    /// **A printer controller run's step** (`Act::Printer`): the rest of the
    /// read goes to the job, but NUL, XON and XOFF, and the sequences that
    /// turn the mode on and off, which "the terminal must intercept ... and
    /// must execute the sequence which turns Printer Controller Mode off.
    /// These control sequences are not passed to the printer" (070-7 §
    /// 7.3.6). `CSI 4 i` ends the run, its job and autoprint with it (the
    /// reset algorithm, p. 7-34); `CSI 5 i` is swallowed; in VT52, `ESC X`
    /// and `ESC W`. A sequence that turns out to be neither goes to the
    /// printer whole. What was held back at the read's end waits for the
    /// next read, so where the reads fall changes nothing.
    ///
    /// A loop over the bytes, not a search: every byte is work, copied or
    /// held, and the matcher's state is three values.
    #[cold]
    #[inline(never)]
    pub(crate) fn printer_bytes(
        &mut self,
        from: BytePos,
        left: ReadLen,
        node: &mut crate::graph::Node,
    ) -> ReadLen {
        let vt52 = self.cold.terminal == TerminalMode::Vt52;
        let latin1 = self.cold.encoding == InputEncoding::Latin1;
        let mut i = 0usize;
        while i < left.index() {
            let b = StreamByte::from_stream(self.arena[from.index() + i]);
            i += 1;
            // NUL, DC1 and DC3, wherever they fall.
            if matches!(b.raw_byte(), 0x00 | 0x11 | 0x13) {
                continue;
            }
            let m = self.strs.print.matcher.at;
            let next = match (m, b.raw_byte(), vt52) {
                (Match::Idle, 0x1b, _) => Some(Match::Esc),
                (Match::Idle, 0x9b, false) if latin1 => Some(Match::Csi),
                (Match::Idle, _, _) => {
                    self.put_received(b);
                    continue;
                }
                (Match::Esc, b'[', false) => Some(Match::Csi),
                (Match::Esc, b'X', true) => {
                    return self.controller_off(ReadLen::from_index(i), node);
                }
                (Match::Esc, b'W', true) => {
                    self.strs.print.matcher = Matcher::IDLE;
                    continue;
                }
                (Match::Csi, b'0'..=b'9', false) => Some(Match::Csi),
                (Match::Csi, b'i', false) => match MediaCopy::standard(Some(self.held_parameter()))
                {
                    Some(MediaCopy::ControllerOff) => {
                        return self.controller_off(ReadLen::from_index(i), node);
                    }
                    Some(MediaCopy::ControllerOn) => {
                        self.strs.print.matcher = Matcher::IDLE;
                        continue;
                    }
                    _ => {
                        self.flush_held();
                        self.put(b"i");
                        continue;
                    }
                },
                _ => None,
            };
            match next {
                Some(at) if self.hold(b) => self.strs.print.matcher.at = at,
                // Not the sequence, or too long to be it: what was held goes
                // to the printer and the byte is looked at again from the
                // start.
                _ => {
                    self.flush_held();
                    i -= 1;
                }
            }
        }
        left
    }

    /// Hold `b` while a sequence may be `CSI 4 i`; false when the matcher
    /// is full, and it is then not one.
    fn hold(&mut self, b: StreamByte) -> bool {
        let m = &mut self.strs.print.matcher;
        let n = m.len.index();
        if n == MATCH_MAX {
            return false;
        }
        m.held[n] = b.raw_byte();
        m.len = ByteLen::from_index(n + 1);
        true
    }

    /// The held sequence to the printer, as received.
    fn flush_held(&mut self) {
        let m = self.strs.print.matcher;
        self.strs.print.matcher = Matcher::IDLE;
        for &b in &m.held[..m.len.index()] {
            self.put_received(StreamByte::from_stream(b));
        }
    }

    /// The held control sequence's parameter, its digits after `ESC [` or
    /// `CSI`, an omitted one 0.
    fn held_parameter(&self) -> Param {
        let m = &self.strs.print.matcher;
        m.held[..m.len.index()]
            .iter()
            .filter(|b| b.is_ascii_digit())
            .fold(Param::from_raw(0), |v, &d| {
                v.push(Digit::of_ascii(StreamByte::from_stream(d)))
            })
    }

    /// `CSI 4 i` (or VT52's `ESC X`) at `i` bytes into the step: the run
    /// ends, its job with it, and the parser goes on from after a control.
    fn controller_off(&mut self, taken: ReadLen, node: &mut crate::graph::Node) -> ReadLen {
        self.strs.print.matcher = Matcher::IDLE;
        self.strs.print.run = PrintRun::Idle;
        self.cold.modes.reset(ModeBit::AutoPrint);
        self.finish_job();
        *node = match self.cold.terminal {
            TerminalMode::Ansi => crate::graph::Node::AfterControl,
            TerminalMode::Vt52 => crate::graph::Node::Vt52AfterControl,
        };
        taken
    }

    /// `RIS`: the printer to its power-up state. An autoprint run's job
    /// ends, kept for the embedder; the port is the embedder's and stays,
    /// and is seen again only if a printer is there now, 070-7 p. 7-32's
    /// "has not been on since the last power-up or reset (RIS)".
    pub(crate) fn print_reset(&mut self) {
        self.end_run_job();
        let p = &mut self.strs.print;
        p.run = PrintRun::Idle;
        p.form_feed = PrintFormFeed::Off;
        p.extent = PrintExtent::ScrollingRegion;
        p.size = PrintSize::Compressed;
        p.matcher = Matcher::IDLE;
        p.seen = Seen::Never;
        p.set_port(p.port);
        self.cold.modes.reset(ModeBit::AutoPrint);
    }
}
