//! DEC's text locator (`[[source: dec-std-070]]` 070-13): DECELR, DECSLE,
//! DECEFR and DECRQLP from the byte stream, and the pointer from the
//! embedder through [`Term::locator`], both writing DECLRP reports.
//!
//! **Cold on both sides.** The four controls reach here out of
//! `csi_final`, which is out of line already, and the pointer from an
//! embedder's event; nothing here is read by the per-byte loop. The state
//! is in `Cold` beside the mouse protocol it is one of.

use crate::paint::Paint;
use crate::term::{Cold, Term};
use baud_domain::*;

/// The locator's state beside [`MouseTracking::Locator`], which is the
/// switch: the reporting mode and units DECELR chose, the events DECSLE
/// chose, the filter rectangle, and the pointer the embedder last gave.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Locator {
    pub(crate) reporting: LocatorReporting,
    pub(crate) units: LocatorUnits,
    pub(crate) events: LocatorEvents,
    pub(crate) filter: LocatorFilter,
    /// Where the pointer was at the embedder's last event, `None` before
    /// the first and outside the text area; DECRQLP and a DECEFR with a
    /// corner omitted read it.
    pub(crate) at: Option<LocatorAt>,
    pub(crate) held: LocatorButtons,
    /// Whether the embedder has a pointer ([`Term::set_locator_device`]).
    pub(crate) device: LocatorDevice,
}

impl Locator {
    /// What `RIS` leaves, the power-up state: "When disabled (the power-up
    /// default)" (DECELR), "0-Respond only to explicit host requests
    /// (default)" (DECSLE), no rectangle. **The pointer is kept**: it is
    /// where the hand is, not terminal state. DECSTR's table (070-4 p. 31,
    /// PDF 235) names none of this, so it keeps it all, as xterm's soft
    /// reset keeps `send_mouse_pos` (`[[source: xterm-reset]]`'s file).
    pub(crate) fn reset(&mut self) {
        *self = Locator {
            at: self.at,
            held: self.held,
            device: self.device,
            ..Locator::default()
        };
    }
}

/// The locator's position in the units DECELR chose, or `None` where it is
/// undefined.
///
/// **In cells under DECOM, counted from the margins, and undefined outside
/// them**: 070-13 § 3.1 gives "outside the range of defined coordinates"
/// the example "the input cursor is outside the active scrolling region,
/// and the origin mode is set to relative", and CPR counts from the
/// margins the same way (`Paint::reported_cursor`). xterm ignores DECOM
/// here, practice not followed. **In pixels, from the text area's first
/// pixel whatever DECOM says**: device pixels are for dot addressing, which
/// origin mode does not move (Jev, PIXELS_ABSOLUTE, 0.96 and 0.94); a
/// pixel the embedder did not give is undefined, as `?1016` refuses one.
fn locator_pos(cold: &Cold, rows: Rows, cols: Cols) -> Option<LocatorPos> {
    let at = cold.locator.at?;
    match cold.locator.units {
        LocatorUnits::Pixels => at.px.and_then(LocatorPos::pixel),
        LocatorUnits::Cells => {
            if at.row.index() >= rows.index() || at.col.index() >= cols.index() {
                return None;
            }
            if !cold.modes.is_set(ModeBit::Origin) {
                return Some(LocatorPos::cell(at.row, at.col));
            }
            let inside = cold.top <= at.row
                && at.row <= cold.bot
                && cold.left <= at.col
                && at.col <= cold.right;
            inside.then(|| {
                LocatorPos::cell(
                    Row::from_index(at.row.index() - cold.top.index()),
                    Col::at_index(at.col.index() - cold.left.index()),
                )
            })
        }
    }
}

/// The locator's position while DEC locator reporting is enabled, in the
/// units it reports, line then column: what the status line's Graphics
/// Input Position Indicator shows, "if some graphic input mode is enabled
/// through ReGIS, Tektronix, or the Text Locator extension" (`[[source:
/// dec-std-070]]` 070-14 p. 14-6). `None` while it is off or the position
/// undefined.
pub(crate) fn shown(cold: &Cold, rows: Rows, cols: Cols) -> Option<(OneBased, OneBased)> {
    if cold.mouse != MouseTracking::Locator {
        return None;
    }
    locator_pos(cold, rows, cols).map(|p| (p.row, p.col))
}

/// The report `trigger` makes now, with the buttons held and the position.
fn report_of(cold: &Cold, rows: Rows, cols: Cols, trigger: LocatorTrigger) -> Report {
    Report::Locator(LocatorReport::At {
        trigger,
        buttons: cold.locator.held,
        pos: locator_pos(cold, rows, cols),
    })
}

/// After a DECLRP went out: one-shot reporting is over. "If locator
/// reporting has been enabled to one shot mode (DECELR with Ps = 2), a
/// single locator report (DECLRP) shall reset locator reporting mode to
/// disabled" (DECLRP note 1) -- any report, a request's answer and a
/// filter's included.
fn reported(cold: &mut Cold) {
    if cold.mouse == MouseTracking::Locator && cold.locator.reporting == LocatorReporting::OneShot {
        cold.mouse = MouseTracking::Off;
        cold.locator.filter = LocatorFilter::Off;
    }
}

/// Whether a DECRQLP or a DECEFR finds a locator: enabled by DECELR and
/// a pointer told. DECLRP's event 0 is "the locator is unavailable (not
/// enabled, or physically not present)" -- both halves.
fn available(cold: &Cold) -> bool {
    cold.mouse == MouseTracking::Locator && cold.locator.device == LocatorDevice::Present
}

impl Paint<'_, '_> {
    /// DECELR, DECSLE, DECEFR or DECRQLP: its parameters read here, as a
    /// report's are ([`Paint::answer_query`]).
    #[cold]
    #[inline(never)]
    pub(crate) fn locator_op(&mut self, op: LocatorOp, csi: &crate::write::Csi) {
        match op {
            LocatorOp::Enable => self.enable_locator(&csi.params()),
            LocatorOp::SelectEvents => csi.for_each_param(|p| self.select_locator_events(p)),
            LocatorOp::FilterRectangle => self.filter_rectangle(FilterCorners::of(&csi.params())),
            LocatorOp::RequestPosition => match locator_request(&csi.params()) {
                Some(()) => self.request_locator_position(),
                None => self.unsupported_param += 1,
            },
        }
    }

    /// DECELR. **Enabling replaces whichever mouse protocol is in force**,
    /// and `Ps` 0 turns off whichever is (ctlseqs' "mutually exclusive"
    /// protocols; [`MouseTracking::Locator`]). "DECELR always cancels any
    /// previous rectangle definition" (note 1, DECEFR). An undefined `Ps`
    /// or `Pu` ignores the sequence, counted.
    fn enable_locator(&mut self, p: &Params<'_>) {
        let Some(e) = LocatorEnable::of(p) else {
            self.unsupported_param += 1;
            return;
        };
        self.cold.locator.filter = LocatorFilter::Off;
        match e {
            LocatorEnable::Off => self.set_tracking(MouseTracking::Locator, ModeChange::Reset),
            LocatorEnable::On(reporting, units) => {
                self.cold.locator.reporting = reporting;
                self.cold.locator.units = units;
                self.set_tracking(MouseTracking::Locator, ModeChange::Set);
            }
        }
    }

    /// One DECSLE parameter, applied in order: "Ps ... Ps is one or more
    /// selective parameters". 0 also "cancels any pending filter
    /// rectangle"; a value the text does not give changes nothing, counted.
    fn select_locator_events(&mut self, p: Option<Param>) {
        match self.cold.locator.events.select(p) {
            Some(LocatorSelect::RequestsOnly) => {
                self.cold.locator.events = LocatorEvents::NONE;
                self.cold.locator.filter = LocatorFilter::Off;
            }
            Some(LocatorSelect::Events(e)) => self.cold.locator.events = e,
            None => self.unsupported_param += 1,
        }
    }

    /// DECEFR (070-13 pp. 10-11, PDF 1025-1026). With the locator not
    /// enabled, "the rectangle shall be triggered to send a report
    /// indicating that the locator is not available" (note 4), where xterm
    /// sends nothing. With the position undefined, a rectangle with a
    /// corner given triggers at once "with omitted coordinates", and one
    /// with every corner omitted waits for the locator to come back (note
    /// 2). Otherwise a rectangle that does not contain the locator reports
    /// "immediately and de-activate[s]", and one that does is armed.
    fn filter_rectangle(&mut self, corners: FilterCorners) {
        if !available(self.cold) {
            self.want_reply(Report::Locator(LocatorReport::Unavailable));
            return;
        }
        let pos = locator_pos(self.cold, self.rows, self.cols);
        let filter = match pos {
            None if !corners.any_given() => LocatorFilter::Pending,
            None => LocatorFilter::Off,
            Some(at) => match corners.at(at) {
                r if r.contains(at) => LocatorFilter::Armed(r),
                _ => LocatorFilter::Off,
            },
        };
        self.cold.locator.filter = filter;
        if filter == LocatorFilter::Off {
            self.locator_reply(LocatorTrigger::Outside);
        }
    }

    /// DECRQLP: "the terminal shall respond with a single DECLRP with an
    /// event code of 1", the position omitted where it is undefined; with
    /// reporting disabled, "an event code of 0 (indicating locator not
    /// available to avoid timing out the application)" (p. 8, PDF 1023).
    /// xterm, with a filter armed, also turns the locator off, "This might
    /// be a bug" (`GetLocatorPosition`, `[[source: xterm-button-c]]`); the
    /// text says no such thing, and the filter stays.
    fn request_locator_position(&mut self) {
        if !available(self.cold) {
            self.want_reply(Report::Locator(LocatorReport::Unavailable));
            return;
        }
        self.locator_reply(LocatorTrigger::Request);
    }

    /// One DECLRP from the byte stream, and its one-shot disable if it went
    /// in: a reply the buffer could not take is dropped and counted, and
    /// told the program nothing, as [`Term::locator`]'s.
    fn locator_reply(&mut self, t: LocatorTrigger) {
        let r = report_of(self.cold, self.rows, self.cols, t);
        let dropped = self.reply_overflows;
        self.want_reply(r);
        if self.reply_overflows == dropped {
            reported(self.cold);
        }
    }
}

impl Term<'_> {
    /// **Tell the core whether the embedder has a pointer** to feed
    /// [`Term::locator`]: `DSR ?55` answers `?50`, "Locator ready", and
    /// `?56` `?57;1`, a mouse, once it is [`LocatorDevice::Present`], and
    /// `?53`, "No locator", and `?57;0` until then, a DECRQLP or a DECEFR
    /// answering DECLRP's "unavailable". The embedder's to say: no
    /// sequence changes it, `RIS` included.
    pub fn set_locator_device(&mut self, d: LocatorDevice) {
        self.cold.locator.device = d;
    }

    /// What [`Term::set_locator_device`] last set.
    pub fn locator_device(&self) -> LocatorDevice {
        self.cold.locator.device
    }

    /// **The pointer, for DEC's text locator**: every move, enter, leave,
    /// press and release the embedder sees, with the buttons held after
    /// it. Returns whether a report went into the reply buffer.
    ///
    /// The position is kept whatever is enabled, so a DECRQLP or a DECEFR
    /// that arrives before the next move knows where the pointer is. With
    /// the locator enabled (DECELR, [`MouseTracking::Locator`]):
    ///
    /// * a press or a release is reported if DECSLE asked for that
    ///   transition, `CSI Pe ; Pb ; Pr ; Pc & w`, with the coordinates
    ///   omitted where the position is undefined -- over the padding, or
    ///   outside the margins under DECOM in cells -- as 070-13 § 3.1 says;
    ///   xterm sends `CSI 0 & w` there, a practice not followed;
    /// * then any event's position is checked against the filter
    ///   rectangle: outside it, or undefined, it reports event 10 and the
    ///   rectangle ends -- "Filter rectangles are always treated as one-shot
    ///   events" -- and a waiting rectangle reports once the position is
    ///   defined. A press outside the rectangle reports the press first:
    ///   "The order of reporting shall be by increasing event code number"
    ///   (§ 3);
    /// * a one-shot DECELR is disabled by the first report.
    ///
    /// A press the embedder gave to its selection (Shift, as for the mouse
    /// protocols) is passed as [`LocatorKind::Moved`]: its position and
    /// buttons are kept and nothing is reported for it. A report that does
    /// not fit the buffer is dropped and counted, as [`Term::mouse_report`]'s
    /// is, and a one-shot DECELR stays enabled for the next.
    ///
    /// **Input-side and cold**: the per-byte loop never calls this.
    pub fn locator(&mut self, e: LocatorInput) -> bool {
        // **ReGIS graphics input takes the pointer first**, and DEC's
        // locator reports after it: "the characters for the locator button
        // (if any) would be sent followed by the ReGIS position report, and
        // finally the ANSI locator report (DECLRP) if enabled" (070-13
        // § 5.5; `crate::regis`).
        let regis = self.regis_pointer(&e);
        self.dec_locator(e) | regis
    }

    /// [`Term::locator`] for DEC's text locator.
    fn dec_locator(&mut self, e: LocatorInput) -> bool {
        self.cold.locator.at = e.at;
        self.cold.locator.held = e.held;
        // The indicator shows where the locator is while it reports.
        self.redraw_indicator();
        if self.cold.mouse != MouseTracking::Locator {
            return false;
        }
        let events = self.cold.locator.events;
        let trigger = match e.kind {
            LocatorKind::Press(b) if events.has(LocatorEvents::DOWN) => {
                Some(LocatorTrigger::Down(b))
            }
            LocatorKind::Release(b) if events.has(LocatorEvents::UP) => Some(LocatorTrigger::Up(b)),
            _ => None,
        };
        let mut sent = false;
        if let Some(t) = trigger {
            sent = self.locator_report(t);
            if self.cold.mouse != MouseTracking::Locator {
                return sent;
            }
        }
        let pos = locator_pos(&self.cold, self.rows, self.cols);
        let fire = match (self.cold.locator.filter, pos) {
            (LocatorFilter::Off, _) | (LocatorFilter::Pending, None) => false,
            (LocatorFilter::Pending, Some(_)) | (LocatorFilter::Armed(_), None) => true,
            (LocatorFilter::Armed(r), Some(at)) => !r.contains(at),
        };
        if fire {
            self.cold.locator.filter = LocatorFilter::Off;
            sent |= self.locator_report(LocatorTrigger::Outside);
        }
        sent
    }

    /// One DECLRP from the pointer, and its one-shot disable if it went in.
    fn locator_report(&mut self, t: LocatorTrigger) -> bool {
        let r = report_of(&self.cold, self.rows, self.cols, t);
        let queued = self.queue(r);
        if queued {
            reported(&mut self.cold);
        }
        queued
    }
}
