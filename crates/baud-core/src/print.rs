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
//! **What a print takes of the rows it names is ECMA-48's**: SSA and ESA
//! mark selected areas, and SATM, MATM and TTM choose what is eligible to
//! be "transferred to an auxiliary input/output device" (`[[source:
//! ecma48]]` 6.5.1, 7.2.12, 7.2.14, 7.2.18 and 7.3.1), which MC is (6.6,
//! 8.3.82); GATM then leaves the guarded positions out (7.3.1 g, h). An
//! ineligible position prints as a space, as a guarded one does, so what
//! is eligible keeps its columns. With no area defined, or the active
//! position in none under SINGLE, everything is eligible: 7.3.1 leaves
//! cases a and d "not defined" there, and baud prints as it did before any
//! area. [`Eligible`].
//!
//! **FETM decides whether a print of screen contents carries formator
//! functions** (`[[source: ecma48]]` 7.2.6): under INSERT, the reset state,
//! the stream above; under EXCLUDE, no `ESC [ 0 m`, no rendition, no `CR`
//! and line end, no DECPFF `FF` and no form feed joining it to the last job,
//! so the lines run together. The DEC line sizes, `ESC # 5`, `6`, `3` and
//! `4`, stay: they are private functions, neither a format effector (8.2.4)
//! nor a presentation control function (8.2.5). A controller run is a
//! relay of the host's bytes, MC 5's "relay of the received data stream"
//! (8.3.82), and a hard copy is not screen text: FETM leaves both as they
//! are ([`JobKind`]).
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
    /// SATM, MATM and TTM.
    pub(crate) transfer: AreaTransfer,
    /// FETM.
    pub(crate) formators: FormatorTransfer,
    /// The selected areas SSA and ESA marked.
    pub(crate) areas: SelectedAreas,
}

/// One end of a selected area, kept as SPA's start is: the screen, the
/// line in its numbering, which moves with the text, and the position
/// along it as `CPR` reports it ([`Paint::guard_position`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct AreaEnd {
    screen: ImageScreen,
    line: LineIdx,
    col: Col,
}

impl AreaEnd {
    fn at(self) -> (LineIdx, Col) {
        (self.line, self.col)
    }
}

/// A selected area: its first and last positions, the first not after the
/// last in the order of the lines and their columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Area {
    screen: ImageScreen,
    first: (LineIdx, Col),
    last: (LineIdx, Col),
}

/// **The selected areas**: the one SSA opened, and the ones ESA ended, at
/// most [`SELECTED_AREAS`]. Cold, read by a print, by SSA and ESA, and by
/// the moves of lines inside the window, which test [`Self::held`] first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct SelectedAreas {
    open: Option<AreaEnd>,
    kept: [Option<Area>; SELECTED_AREAS],
    held: Held,
}

/// Whether an area is open or kept: the one test a scroll pays for them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    Nothing,
    Some,
}

impl SelectedAreas {
    /// None: the state at power-on, after `RIS` and after a change of
    /// geometry.
    pub(crate) const NONE: SelectedAreas = SelectedAreas {
        open: None,
        kept: [None; SELECTED_AREAS],
        held: Held::Nothing,
    };

    /// Whether an area is open or kept, on either screen.
    #[inline(always)]
    pub(crate) fn held(&self) -> bool {
        self.held == Held::Some
    }

    fn refresh(&mut self) {
        self.held = match self.open.is_some() || self.kept.iter().any(Option::is_some) {
            true => Held::Some,
            false => Held::Nothing,
        };
    }

    /// **Every area on `screen` goes**, and an open start there: the
    /// alternate screen's on leaving it, whose numbering starts again with
    /// the next entry, as its images go.
    pub(crate) fn forget_screen(&mut self, screen: ImageScreen) {
        self.open = self.open.filter(|e| e.screen != screen);
        for slot in &mut self.kept {
            *slot = slot.filter(|a| a.screen != screen);
        }
        self.refresh();
    }

    /// **The lines `[lo, hi)` of `screen` moved `k` `way` inside the
    /// window**, and the ends on them between the columns `within` with
    /// them: IL and DL shift "any occurrences of the start or end of a
    /// selected area ... in the shifted part" (`[[source: ecma48]]` 8.3.67,
    /// 8.3.32), and a region's scroll moves them as it moves SPA's start.
    /// An end that leaves the band was on a line removed and takes its
    /// area, as SPA's start is forgotten there; an area whose ends a move
    /// under side margins took past each other is ordered again.
    fn band_moved(
        &mut self,
        screen: ImageScreen,
        (lo, hi): (LineIdx, LineIdx),
        within: (Col, Col),
        k: Rows,
        way: BandMove,
    ) {
        let moved = |(line, col): (LineIdx, Col)| {
            if line < lo || line >= hi || col < within.0 || col > within.1 {
                return Some((line, col));
            }
            match way {
                BandMove::Up => line.minus(k).filter(|l| *l >= lo),
                BandMove::Down => Some(line.plus(k)).filter(|l| *l < hi),
            }
            .map(|l| (l, col))
        };
        if let Some(e) = self.open.filter(|e| e.screen == screen) {
            self.open = moved(e.at()).map(|(line, col)| AreaEnd { screen, line, col });
        }
        for slot in &mut self.kept {
            let Some(a) = slot.filter(|a| a.screen == screen) else {
                continue;
            };
            *slot = match (moved(a.first), moved(a.last)) {
                (Some(f), Some(l)) => Some(Area {
                    screen,
                    first: f.min(l),
                    last: f.max(l),
                }),
                _ => None,
            };
        }
        self.refresh();
    }

    /// **Every area on `screen` with its last end at or past line `below`
    /// goes, and an open start there**: a full-screen scroll down pushed
    /// those lines below the screen, and the line a later scroll up brings
    /// in at that number is erased, as a region's scroll drops an end it
    /// pushes out of its band ([`Self::band_moved`]). An area's first end
    /// is not after its last, so the last is the one to test.
    fn pushed_below(&mut self, screen: ImageScreen, below: LineIdx) {
        let on = |(line, _): (LineIdx, Col)| line < below;
        self.open = self.open.filter(|e| e.screen != screen || on(e.at()));
        for slot in &mut self.kept {
            *slot = slot.filter(|a| a.screen != screen || on(a.last));
        }
        self.refresh();
    }

    /// **Every end on `screen` down `k` lines**: the window moved back over
    /// a row its numbering cannot count below line zero, so the text went
    /// down instead, as an open SPA's start does.
    fn shift(&mut self, screen: ImageScreen, k: Rows) {
        if let Some(e) = self.open.as_mut().filter(|e| e.screen == screen) {
            e.line = e.line.plus(k);
        }
        for a in self
            .kept
            .iter_mut()
            .flatten()
            .filter(|a| a.screen == screen)
        {
            a.first.0 = a.first.0.plus(k);
            a.last.0 = a.last.0.plus(k);
        }
    }
}

/// **A screen position as a print reads it**: its row and column, ordered
/// by row and then column, the order of the character path along a line.
type Pos = (usize, usize);

/// **What a print may take**, 7.3.1 a to f, worked out once a print from
/// the modes, the areas on the screen shown and the active position: the
/// positions [`Scope`] names, before `before` where there is one (TTM
/// CURSOR).
#[derive(Clone, Copy)]
struct Eligible {
    scope: Scope,
    /// The areas, each from its first to its last position, read under
    /// [`Scope::Areas`].
    areas: [Option<(Pos, Pos)>; SELECTED_AREAS],
    before: Option<Pos>,
}

/// Where the eligible positions are: everywhere, cases c and f, and a and
/// d with the active position in no selected area, which 7.3.1 leaves "not
/// defined"; or in the areas listed, cases a, b, d and e.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Everywhere,
    Areas,
}

impl Eligible {
    fn everywhere(before: Option<Pos>) -> Eligible {
        Eligible {
            scope: Scope::Everywhere,
            areas: [None; SELECTED_AREAS],
            before,
        }
    }

    fn admits(&self, p: Pos) -> bool {
        self.before.is_none_or(|c| p < c)
            && match self.scope {
                Scope::Everywhere => true,
                Scope::Areas => self.areas.iter().flatten().any(|(a, b)| *a <= p && p <= *b),
            }
    }
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
/// the queue being full when it began: after a form feed, or, a print of
/// screen contents under FETM's EXCLUDE, with nothing between.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    Own,
    Last,
    LastRunOn,
}

/// What a job carries: screen contents, whose formator functions FETM
/// governs, or the bytes of a controller run or a hard copy, which it
/// leaves as they are.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobKind {
    ScreenText,
    Other,
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
        transfer: AreaTransfer::RESET,
        formators: FormatorTransfer::Insert,
        areas: SelectedAreas::NONE,
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
            Join::Own | Join::LastRunOn => len.is_zero(),
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
            Join::Last | Join::LastRunOn => {
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

impl<M: crate::bands::BandMode> Paint<'_, '_, M> {
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
                self.begin_job(JobKind::ScreenText);
                let span = match self.strs.print.extent {
                    PrintExtent::Page => Span::Line,
                    PrintExtent::ScrollingRegion => Span::Margins,
                };
                self.put_formator(b"\x1b[0m");
                self.print_rows(self.cy, self.cy, span, LineEnd::LineFeed);
                self.put_formator(b"\x1b[0m");
                self.finish_job();
                self.resume_run_job();
            }
            MediaCopy::AutoPrintOn => {
                if self.strs.print.run == PrintRun::Idle {
                    self.strs.print.run = PrintRun::AutoPrint;
                    self.cold.modes.set(ModeBit::AutoPrint);
                    self.begin_job(JobKind::ScreenText);
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
                self.begin_job(JobKind::Other);
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
        self.begin_job(JobKind::ScreenText);
        self.put_formator(b"\x1b[0m");
        self.print_rows(top, bottom, Span::Line, LineEnd::LineFeed);
        if self.strs.print.form_feed == PrintFormFeed::On {
            self.put_formator(b"\x0c");
        }
        self.put_formator(b"\x1b[0m");
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
            self.begin_job(JobKind::ScreenText);
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
        self.put_formator(b"\x1b[0m");
        self.print_rows(self.cy, self.cy, Span::Line, end);
        self.put_formator(b"\x1b[0m");
    }

    /// Rows `top` to `bottom` of the screen shown, each with its line
    /// attribute, its characters and their renditions, and `CR` and `end`;
    /// a position not eligible prints as a space ([`Eligible`]).
    fn print_rows(&mut self, top: Row, bottom: Row, span: Span, end: LineEnd) {
        let mut now = Attrs::NONE;
        let eligible = self.eligible();
        for y in top.index()..=bottom.index() {
            let r = Row::from_index(y);
            // The position along the line as SSA and ESA name it: drawn,
            // on a line holding a string with BDSM reset.
            let map = match self.guards_drawn() {
                true => self.line_map(r),
                false => None,
            };
            let takes = |x: usize| {
                let c = map
                    .as_ref()
                    .map_or(Col::at_index(x), |m| m.presentation(Col::at_index(x)));
                eligible.admits((y, c.index()))
            };
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
            while last > left && (self.trailing(r, Col::at_index(last - 1)) || !takes(last - 1)) {
                last -= 1;
            }
            for x in left..last {
                let cell = self.view.get(r, Col::at_index(x));
                let attrs = self.word_in(r, cell.style).attrs();
                let base = self.clusters.base_of(cell.content);
                // A wide character's tail goes with its lead.
                let lead = match base.is_wide_tail() && x > 0 {
                    true => x - 1,
                    false => x,
                };
                let hidden = attrs.has(Attrs::HIDDEN) || self.excluded(cell) || !takes(lead);
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
            self.put_formator(&[b'\r', end.byte().raw_byte()]);
        }
    }

    /// Whether the cell at `(r, x)` is a trailing space: blank, or a space,
    /// with none of the four printed attributes (070-7 § 7.4.3).
    fn trailing(&self, r: Row, x: Col) -> bool {
        let cell = self.view.get(r, x);
        let base = self.clusters.base_of(cell.content);
        let blank = base.is_empty() || base.as_codepoint().is_some_and(|c| c.value() == 0x20);
        (blank || self.excluded(cell))
            && self.word_in(r, cell.style).attrs().only(PRINTED) == Attrs::NONE
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

    /// **What 7.3.1 makes eligible now**: SATM ALL takes every position,
    /// before the active one under TTM CURSOR (cases f and c); SELECT the
    /// selected areas on the screen shown, every one under MATM MULTIPLE
    /// (b, e), the one holding the active position under SINGLE (a, d),
    /// before the active position under CURSOR. With SINGLE and the active
    /// position in no area every position is eligible, as with no area
    /// defined: "If the active presentation position is not within a
    /// selected area, the format of the data stream in the first and fourth
    /// case above is not defined by this Standard", and baud prints as it
    /// did before any area (a reading the text leaves open).
    fn eligible(&self) -> Eligible {
        let t = self.strs.print.transfer;
        let at = (self.cy.index(), self.guard_position().index());
        let before = match t.end {
            TransferEnd::Cursor => Some(at),
            TransferEnd::All => None,
        };
        if t.selection == AreaSelection::All {
            return Eligible::everywhere(before);
        }
        let (screen, top) = self.image_top();
        let (rows, cols) = (self.rows.index(), self.cols.index());
        // Each area's ends as screen positions, an end above the screen its
        // first position and one below it its last; an area wholly above
        // or below none. Wholly above is a last end above the screen, which
        // would otherwise read as an area of the first position alone.
        let place = |(line, col): (LineIdx, Col)| match line.since(top) {
            None => (0, 0),
            Some(r) if r.index() >= rows => (rows, 0),
            Some(r) => (r.index(), col.index().min(cols - 1)),
        };
        let mut on = [None; SELECTED_AREAS];
        for (slot, area) in on.iter_mut().zip(self.strs.print.areas.kept) {
            *slot = area
                .filter(|a| a.screen == screen && a.last.0.since(top).is_some())
                .map(|a| (place(a.first), place(a.last)))
                .filter(|(a, b)| a < b || (a == b && a.0 < rows))
                .map(|(a, b)| (a, (b.0.min(rows), b.1)));
        }
        if t.count == AreaCount::Single {
            let active = on
                .iter()
                .flatten()
                .rev()
                .find(|(a, b)| *a <= at && at <= *b)
                .copied();
            let Some(area) = active else {
                return Eligible::everywhere(None);
            };
            on = [None; SELECTED_AREAS];
            on[0] = Some(area);
        }
        Eligible {
            scope: Scope::Areas,
            areas: on,
            before,
        }
    }

    /// **SSA**: the active position starts a selected area (6.5.1, "the
    /// active presentation position after the receipt of SSA is the first
    /// character position"), kept as SPA's start is. A second SSA before
    /// ESA starts it again. **Written to the status line it starts none**,
    /// nor does ESA end one there: the status line is a display of its own
    /// (`[[source: dec-std-070]]` 070-14), not the page a print takes, and
    /// a line of it would name the main display's (a reading the text
    /// leaves open).
    #[cold]
    #[inline(never)]
    pub(crate) fn start_selected_area(&mut self) {
        if self.on_status() {
            return;
        }
        let (screen, line) = self.image_anchor();
        let col = self.guard_position();
        self.strs.print.areas.open = Some(AreaEnd { screen, line, col });
        self.strs.print.areas.held = Held::Some;
    }

    /// **The selected areas after the rows `[from, to]` of the live screen
    /// moved `k` `way`** with the ends between the columns `within`: IL, DL,
    /// a region's scroll by copy, a shell block's move, a rectangle's
    /// scroll and the rows under a top-anchored region put back
    /// ([`SelectedAreas::band_moved`]). Out of line; the callers test
    /// [`SelectedAreas::held`].
    #[cold]
    #[inline(never)]
    pub(crate) fn areas_band_moved(
        &mut self,
        (from, to): (Row, Row),
        within: (Col, Col),
        k: Rows,
        way: BandMove,
    ) {
        let (screen, top) = self.image_top();
        let lo = top.plus(Rows::of_count(from.index()));
        let hi = top.plus(Rows::of_count(to.index() + 1));
        self.strs
            .print
            .areas
            .band_moved(screen, (lo, hi), within, k, way);
    }

    /// The same across the full width of the screen.
    pub(crate) fn areas_rows_moved(&mut self, from: Row, to: Row, k: Rows, way: BandMove) {
        let within = (Col::ZERO, Col::at_index(self.cols.index()));
        self.areas_band_moved((from, to), within, k, way);
    }

    /// **The selected areas on `screen` after the window moved back over a
    /// row its numbering cannot count below line zero**: down a row with
    /// their text ([`SelectedAreas::shift`]).
    #[cold]
    #[inline(never)]
    pub(crate) fn areas_shift(&mut self, screen: ImageScreen) {
        self.strs.print.areas.shift(screen, Rows::of_count(1));
    }

    /// **The selected areas after a full-screen scroll down**, on the
    /// screen shown: one with an end the scroll pushed below the screen
    /// goes ([`SelectedAreas::pushed_below`]). Once a scroll, after its
    /// last line: the window only moves back and the ends only move down,
    /// so an end below after one line is below after the last. The status
    /// line moves none of the main display's lines. Out of line; the
    /// callers test [`SelectedAreas::held`].
    #[cold]
    #[inline(never)]
    pub(crate) fn areas_scrolled_down(&mut self) {
        if self.on_status() {
            return;
        }
        let (screen, top) = self.image_top();
        self.strs
            .print
            .areas
            .pushed_below(screen, top.plus(self.rows));
    }

    /// **ESA**: the area SSA opened ends at the active position, "the
    /// active presentation position before the receipt of ESA" being "the
    /// last character position of the selected area" (6.5.1), the two in
    /// the order of the lines whichever came first, as EPA orders a guarded
    /// area's. With none open, or one opened on the other screen, it does
    /// nothing. Past [`SELECTED_AREAS`] the areas wholly scrolled above
    /// their screen go first, which no print can reach; with none of those
    /// the area is refused and counted.
    #[cold]
    #[inline(never)]
    pub(crate) fn end_selected_area(&mut self) {
        if self.on_status() {
            return;
        }
        let Some(open) = self.strs.print.areas.open.take() else {
            return;
        };
        let (screen, line) = self.image_anchor();
        if open.screen != screen {
            self.strs.print.areas.refresh();
            return;
        }
        let end = AreaEnd {
            screen,
            line,
            col: self.guard_position(),
        };
        let (first, last) = match open.at() <= end.at() {
            true => (open.at(), end.at()),
            false => (end.at(), open.at()),
        };
        let area = Area {
            screen,
            first,
            last,
        };
        if !self.strs.print.areas.kept.iter().any(Option::is_none) {
            let (primary, alternate) = (self.book.top, self.book.alt_top);
            for slot in &mut self.strs.print.areas.kept {
                let gone = slot.is_some_and(|a| {
                    let top = match a.screen {
                        ImageScreen::Primary => primary,
                        ImageScreen::Alternate => alternate,
                    };
                    a.last.0.since(top).is_none()
                });
                if gone {
                    *slot = None;
                }
            }
        }
        match self.strs.print.areas.kept.iter_mut().find(|a| a.is_none()) {
            Some(slot) => *slot = Some(area),
            None => {
                let r = &mut self.strs.print.refusals.areas_refused;
                *r = r.saturating_add(1);
            }
        }
        self.strs.print.areas.refresh();
    }

    /// `ESC [ 0 ; Ps ... m` for the attributes now in force, 070-7's SEND
    /// GRAPHIC RENDITION; nothing under FETM's EXCLUDE, `SGR` being a
    /// formator function.
    fn put_rendition(&mut self, a: Attrs) {
        if self.strs.print.formators == FormatorTransfer::Exclude {
            return;
        }
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

    /// A formator function the print stream inserts, `SGR` or a format
    /// effector: put under FETM's INSERT, left out under EXCLUDE.
    fn put_formator(&mut self, bytes: &[u8]) {
        if self.strs.print.formators == FormatorTransfer::Insert {
            self.put(bytes);
        }
    }

    fn put_char(&mut self, c: char) {
        let mut b = [0u8; 4];
        self.put(c.encode_utf8(&mut b).as_bytes());
    }

    /// A job begins where the last finished one ends; with the queue full
    /// it will join that one, after a form feed, which a print of screen
    /// contents under FETM's EXCLUDE leaves out.
    pub(crate) fn begin_job(&mut self, kind: JobKind) {
        let p = &mut self.strs.print;
        let full = p.done.iter().all(Option::is_some);
        let join = match (full, kind, p.formators) {
            (false, _, _) => Join::Own,
            (true, JobKind::ScreenText, FormatorTransfer::Exclude) => Join::LastRunOn,
            (true, _, _) => Join::Last,
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
        // SATM, MATM, TTM and FETM to their reset states and no area
        // selected: `RIS` returns the device "to its initial state"
        // (`[[source: ecma48]]` 8.3.105), and 7.1 recommends the reset
        // states as the initial ones. `DECSTR` keeps them, as its table
        // names none.
        p.transfer = AreaTransfer::RESET;
        p.formators = FormatorTransfer::Insert;
        p.areas = SelectedAreas::NONE;
        self.cold.modes.reset(ModeBit::AutoPrint);
    }
}
