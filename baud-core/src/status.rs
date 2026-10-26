//! **The status line**: DECSSDT (`CSI Ps $ ~`) and DECSASD (`CSI Ps $ }`),
//! to `[[source: vt420-rm]]` pp. 219-221 and `[[source: dec-std-070]]`
//! 070-14.
//!
//! "The line below the last line of the user window is reserved for the
//! status line ... as an indicator of the terminal's current state, or as
//! a window the host can use to display application-specific messages"
//! (VT420 p. 219). It is a row beside the main display, never one of its
//! rows: the main display keeps its geometry, and the embedder adds the
//! row to its window ([`crate::Term::status_rows`]), as 070-14 allows, "On
//! a windowed display, the size of the terminal window may change to
//! include the status line".
//!
//! **The host-writable line is a second display of one row**, "a permanent
//! separate logical display, that contains its own ANSI text state and
//! settings" (070-14 §14.2). A read that writes it swaps the [`View`]
//! onto its surface and the main display's own state into [`Parked`], as
//! `?1049` swaps the alternate screen's surface, so the per-character loop
//! never learns which display is up: on a screen of one row, `CUP`'s line
//! is clamped away, `CUU` and `CUD` move nowhere, `CPR` reports line 1, and
//! a line feed at the bottom scrolls the one row out, which 070-14 asks
//! for -- "If scrolling occurs as a result of a Format Effector, a single
//! line Host Writable Status Line is erased". What each display keeps is
//! 070-14's list, §14.2.2: the active position and its pending wrap, the
//! rendition and DECSCA, origin mode, G0-G3, GL, GR and the single shifts,
//! and the `DECSC` slot -- the character sets separate where the VT420
//! shares them (Jev, SEPARATE, 0.98 and 1.00), the wrap's mode shared and
//! its pending flag each display's (Jev, MODE_SHARED_FLAG_SEPARATE, 0.94
//! and 0.96). The cursor's visibility is each display's too, 070-14 item
//! 14, "Each of the Main and Status Display cursors may be individually
//! enabled or disabled". The rest is shared: tabs, IRM, DECAWM, DECSCLM,
//! DECSCNM, the width.
//!
//! **Between reads the terminal is the main display's**: a read that ends
//! on the line swaps back before it writes its state to [`crate::Term`],
//! so every accessor, the geometry, the cursor and the view, is the main
//! display's, and [`crate::Term::status_line`] says where the line's own
//! cursor is.
//!
//! **What belongs to the main display's lines does not follow the
//! swap**: the history, the images and their numbering, a selection's
//! watch, the margin bell's line and the prompt marks are the main
//! display's, so a scroll on the line erases it without retiring a line,
//! an image control or a prompt mark there is counted and not acted on,
//! and the watch and the bell are parked with the main display's state.

use crate::paint::{Paint, Pen};
use crate::term::Term;
use crate::term::{GuardStart, Pinned, Saved, Screen, Watched};
use baud_domain::*;

/// The line's row, lent by the embedder: `cells` at least the pitch, and
/// one extent. An empty row declines the line, as an empty `cells3`
/// declines that table: DECSSDT 1 and 2 are then counted and show nothing.
pub struct StatusBuf<'b> {
    pub(crate) cells: &'b mut [PackedCell],
    pub(crate) used: &'b mut [RowExtent],
}

impl<'b> StatusBuf<'b> {
    /// The row, for [`crate::Storage::with_status_line`].
    pub fn new(cells: &'b mut [PackedCell], used: &'b mut [RowExtent]) -> StatusBuf<'b> {
        StatusBuf { cells, used }
    }

    /// No row: the line cannot be shown.
    pub(crate) fn none() -> StatusBuf<'b> {
        StatusBuf {
            cells: &mut [],
            used: &mut [],
        }
    }
}

/// Which display's state the live fields hold inside a read: the main
/// display's, or the line's after a swap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Swapped {
    Main,
    Status,
}

/// **What one display keeps while the other is written**: 070-14's list,
/// and what belongs to the main display's lines, parked with it.
#[derive(Clone, Copy)]
pub(crate) struct Parked {
    cy: Row,
    cx: CursorCol,
    rows: Rows,
    style: StyleId,
    style_raw: StyleWord,
    flags: CellFlags,
    protect: CellFlags,
    link: LinkId,
    charset: Charset,
    g: [Charset; 4],
    gl: GSlot,
    gr: GSlot,
    single: GSlot,
    desig: [Designation; 4],
    hebrew: HebrewEncoding,
    restored_shift: Option<GSlot>,
    origin: ModeChange,
    cursor: ModeChange,
    saved: Option<Saved>,
    top: Row,
    bot: Row,
    left: Col,
    right: Col,
    fence: Col,
    last_cp: Option<(Codepoint, Width, Pen)>,
    cluster: ClusterBreak,
    guard_open: Option<GuardStart>,
    pinned: Pinned,
    watched: Watched,
    armed: Option<crate::margin::Line>,
    reach: Cols,
    /// The screen: the main display's, or for the line the alternate
    /// screen's, which keeps no history, so a scroll on the line retires
    /// nothing.
    screen: Screen,
}

impl Parked {
    /// The line as a type change leaves it: home, the default rendition,
    /// the power-up sets, origin mode reset, the cursor on, nothing saved,
    /// margins at its edges.
    fn fresh(cols: Cols) -> Parked {
        let right = Col::at_index(cols.index().saturating_sub(1));
        Parked {
            cy: Row::ZERO,
            cx: CursorCol::moved(Col::ZERO),
            rows: Rows::of_count(1),
            style: StyleId::DEFAULT,
            style_raw: StyleWord::DEFAULT,
            flags: CellFlags::NONE,
            protect: CellFlags::NONE,
            link: LinkId::NONE,
            charset: Charset::Ascii,
            g: crate::term::POWER_UP_SETS,
            gl: GSlot::G0,
            gr: GSlot::G2,
            single: GSlot::G2,
            desig: crate::term::POWER_UP_DESIGNATIONS,
            hebrew: HebrewEncoding::Multinational,
            restored_shift: None,
            origin: ModeChange::Reset,
            cursor: ModeChange::Set,
            saved: None,
            top: Row::ZERO,
            bot: Row::ZERO,
            left: Col::ZERO,
            right,
            fence: right,
            last_cp: None,
            cluster: ClusterBreak::new(),
            guard_open: None,
            pinned: Pinned::Before,
            watched: Watched::Nothing,
            armed: None,
            reach: cols,
            screen: Screen::Alternate,
        }
    }
}

impl Parked {
    /// **DECSTR's reset of the line's own state**, kept parked: the
    /// rendition, DECSCA, the character sets, origin mode, the cursor
    /// shown, nothing saved and margins at its edges, as the soft reset
    /// leaves the main display's; its position stays, as the main
    /// display's does.
    fn soft_reset(&mut self, cols: Cols) {
        let fresh = Parked::fresh(cols);
        *self = Parked {
            cy: self.cy,
            cx: self.cx,
            last_cp: self.last_cp,
            cluster: self.cluster,
            ..fresh
        };
    }
}

/// Whether the storage lent the line a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Room {
    Lent,
    Declined,
}

/// The line's state: whether it has a row, its type, which display
/// DECSASD selected, which one the live fields hold, the other's parked
/// state, and the rendition the indicator is drawn in.
#[derive(Clone, Copy)]
pub(crate) struct Status {
    pub(crate) room: Room,
    pub(crate) kind: StatusType,
    pub(crate) active: ActiveDisplay,
    pub(crate) swapped: Swapped,
    pub(crate) park: Parked,
    /// The indicator's reverse-video rendition, interned at its first
    /// drawing in a read; its cells name it, so a sweep keeps it.
    pub(crate) mark: Option<StyleId>,
}

impl Status {
    /// 070-14's "No Status Line (factory default)", baud's power-up type
    /// (Jev's lean, NONE, 0.78 and 0.86, then 0.80 and 0.92).
    pub(crate) fn power_on(cols: Cols, room: Room) -> Status {
        Status {
            room,
            kind: StatusType::None,
            active: ActiveDisplay::Main,
            swapped: Swapped::Main,
            park: Parked::fresh(cols),
            mark: None,
        }
    }

    /// The line's cursor between reads, when its DECTCEM shows it: the
    /// column the cell is drawn at, a pending wrap's the last.
    pub(crate) fn park_cursor(&self, cols: Cols) -> Option<Col> {
        (self.park.cursor == ModeChange::Set).then(|| self.park.cx.reported(self.park.right, cols))
    }

    /// The style ids a sweep must keep beside the cells: the parked
    /// display's rendition.
    pub(crate) fn parked_style(&self) -> StyleId {
        self.park.style
    }
}

/// What the indicator shows, each true of baud, at fixed columns with a
/// space between (070-14 §14.2.1.2, "The indicators shall be placed in
/// fixed positions on the Indicator Status Line, and there shall be at
/// least one space (SP, 2/0) character between indicators").
#[derive(Clone, Copy)]
pub(crate) struct Indicators {
    /// The active position, as `1(line,column)`: the page, baud's one,
    /// then the cursor.
    pub(crate) at: (OneBased, OneBased),
    /// KAM: set, the keyboard is locked and the VT420's Wait indicator is
    /// shown.
    pub(crate) wait: ModeChange,
    /// The locator's position while DEC locator reporting is enabled, in
    /// the units it reports: 070-14's Graphics Input Position Indicator.
    pub(crate) locator: Option<(OneBased, OneBased)>,
    /// The printer status indicator's value.
    pub(crate) printer: PrinterShown,
}

/// **The printer field's five values**, the VT420's Table 4-2, field 3
/// (`[[source: vt420-uu-ch4]]`), and 070-14's list (p. 14-7): "Printer:
/// None", "The printer is off or not connected"; "Printer: Ready", "can
/// receive data for printing"; "Printer: Not Ready", "not ready to receive
/// data"; "Printer: Auto Print" and "Printer: Controller", the two print
/// modes. "Busy" and "Assigned to" are the VT420's for two sessions, and
/// baud has one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PrinterShown {
    None,
    Ready,
    NotReady,
    AutoPrint,
    Controller,
}

impl PrinterShown {
    /// What `p` shows: a run's mode while one is on, else the port's
    /// state, 070-7 § 7.3.2's three.
    pub(crate) fn of(p: &crate::print::Printer) -> PrinterShown {
        match (p.run, p.status()) {
            (PrintRun::AutoPrint, _) => PrinterShown::AutoPrint,
            (PrintRun::Controller, _) => PrinterShown::Controller,
            (PrintRun::Idle, crate::print::Status::Ready) => PrinterShown::Ready,
            (PrintRun::Idle, crate::print::Status::NotReady) => PrinterShown::NotReady,
            (PrintRun::Idle, crate::print::Status::NotAvailable) => PrinterShown::None,
        }
    }

    fn text(self) -> &'static [u8] {
        match self {
            PrinterShown::None => b"Printer: None",
            PrinterShown::Ready => b"Printer: Ready",
            PrinterShown::NotReady => b"Printer: Not Ready",
            PrinterShown::AutoPrint => b"Printer: Auto Print",
            PrinterShown::Controller => b"Printer: Controller",
        }
    }
}

/// The column each indicator starts at: the VT340's example line, "2
/// 1(24,008) Edit Overstrike Mode Printer: None Modem: DSR" (070-14 p.
/// 14-6), with the session digit left out, as baud has one session, and
/// the Wait and locator indicators where its edit mode indicators would
/// stand, which baud has none of. **The page is shown, 1**: the VT420's
/// field 1 is the page always, 070-14's "if more than one page is
/// available" asks for it only then. **The modem field is blank**, as the
/// VT420's is "unless you have a modem connected ... and Modem Control is
/// selected", 070-14's "not displayed if Modem Controls are not selected":
/// a pty has no modem.
const AT_PAGE: usize = 4;
const AT_WAIT: usize = 16;
const AT_LOCATOR: usize = 23;
const AT_PRINTER: usize = 41;

/// The line `cols` wide as the indicator, every cell in `style`.
struct Line<'c> {
    cells: &'c mut [PackedCell],
    style: StyleId,
}

impl Line<'_> {
    /// `text` from `at`, cut at the line's end; the column after it.
    fn put(&mut self, at: Col, text: &[u8]) -> Col {
        let mut x = at;
        for b in text {
            if let Some(c) = self.cells.get_mut(x.index()) {
                *c = PackedCell {
                    content: Content::codepoint(Codepoint::from_ascii(StreamByte::from_stream(*b))),
                    style: self.style,
                    flags: CellFlags::NONE,
                };
            }
            x = x.after();
        }
        x
    }

    /// `n` in decimal from `at`, zero-padded to `width` digits.
    fn number(&mut self, at: Col, n: OneBased, width: Cols) -> Col {
        let mut digits = [b'0'; 5];
        let mut k = digits.len();
        let mut v = n.get();
        loop {
            k -= 1;
            digits[k] = b'0' + u8::try_from(v % 10).unwrap_or(0);
            v /= 10;
            if v == 0 || k == 0 {
                break;
            }
        }
        let start = k.min(digits.len().saturating_sub(width.index()));
        self.put(at, &digits[start..])
    }
}

/// Write the indicator into `cells`, `cols` of them, in `style`.
pub(crate) fn compose(cells: &mut [PackedCell], cols: Cols, style: StyleId, ind: Indicators) {
    let n = cols.index().min(cells.len());
    let blank = PackedCell {
        content: Content::EMPTY,
        style,
        flags: CellFlags::NONE,
    };
    cells[..n].fill(blank);
    let mut line = Line {
        cells: &mut cells[..n],
        style,
    };
    let x = line.put(Col::at_index(AT_PAGE), b"1(");
    let x = line.number(x, ind.at.0, Cols::of_index(2));
    let x = line.put(x, b",");
    let x = line.number(x, ind.at.1, Cols::of_index(3));
    line.put(x, b")");
    if ind.wait == ModeChange::Set {
        line.put(Col::at_index(AT_WAIT), b"Wait");
    }
    if let Some((y, x)) = ind.locator {
        let at = line.put(Col::at_index(AT_LOCATOR), b"[");
        let at = line.number(at, y, Cols::of_index(1));
        let at = line.put(at, b",");
        let at = line.number(at, x, Cols::of_index(1));
        line.put(at, b"]");
    }
    line.put(Col::at_index(AT_PRINTER), ind.printer.text());
}

impl Paint<'_, '_> {
    /// Whether the live fields are the line's.
    pub(crate) fn on_status(&self) -> bool {
        self.strs.status.swapped == Swapped::Status
    }

    /// **The two displays exchanged**: the surface the view writes, and
    /// every field 070-14 gives each its own. What belongs to the main
    /// display's lines -- the watch on a selection, the margin bell's line,
    /// what lies past the edge -- goes with the main display and comes back
    /// with it.
    pub(crate) fn swap_display(&mut self) {
        // A form the last character took leaves with its display's
        // rendition, not after it.
        self.form_ends();
        self.cold.placement.forget();
        let into = match self.strs.status.swapped {
            Swapped::Main => Swapped::Status,
            Swapped::Status => Swapped::Main,
        };
        let mut p = self.strs.status.park;
        core::mem::swap(&mut self.cy, &mut p.cy);
        core::mem::swap(&mut self.cx, &mut p.cx);
        core::mem::swap(&mut self.rows, &mut p.rows);
        core::mem::swap(&mut self.style, &mut p.style);
        core::mem::swap(&mut self.style_raw, &mut p.style_raw);
        core::mem::swap(&mut self.flags, &mut p.flags);
        core::mem::swap(&mut self.cold.protect, &mut p.protect);
        core::mem::swap(&mut self.link, &mut p.link);
        core::mem::swap(&mut self.charset, &mut p.charset);
        core::mem::swap(&mut self.cold.g, &mut p.g);
        core::mem::swap(&mut self.cold.gl, &mut p.gl);
        core::mem::swap(&mut self.cold.gr, &mut p.gr);
        core::mem::swap(&mut self.cold.single, &mut p.single);
        core::mem::swap(&mut self.cold.desig, &mut p.desig);
        core::mem::swap(&mut self.cold.hebrew, &mut p.hebrew);
        core::mem::swap(&mut self.cold.restored_shift, &mut p.restored_shift);
        self.rendition_changed();
        let (origin, cursor) = (
            ModeChange::of(self.cold.modes.is_set(ModeBit::Origin)),
            ModeChange::of(self.cold.modes.is_set(ModeBit::CursorVisible)),
        );
        self.cold
            .modes
            .apply(ModeBit::Origin, p.origin == ModeChange::Set);
        self.cold
            .modes
            .apply(ModeBit::CursorVisible, p.cursor == ModeChange::Set);
        (p.origin, p.cursor) = (origin, cursor);
        core::mem::swap(&mut self.cold.saved, &mut p.saved);
        core::mem::swap(&mut self.cold.top, &mut p.top);
        core::mem::swap(&mut self.cold.bot, &mut p.bot);
        core::mem::swap(&mut self.cold.left, &mut p.left);
        core::mem::swap(&mut self.cold.right, &mut p.right);
        core::mem::swap(&mut self.cold.fence, &mut p.fence);
        core::mem::swap(&mut self.last_cp, &mut p.last_cp);
        core::mem::swap(&mut self.cluster, &mut p.cluster);
        core::mem::swap(&mut self.cold.guard_open, &mut p.guard_open);
        core::mem::swap(&mut self.cold.pinned, &mut p.pinned);
        core::mem::swap(&mut self.cold.watched, &mut p.watched);
        core::mem::swap(&mut self.cold.bell.armed, &mut p.armed);
        core::mem::swap(&mut self.book.reach, &mut p.reach);
        core::mem::swap(&mut self.cold.screen, &mut p.screen);
        self.strs.status.park = p;
        self.strs.status.swapped = into;
        self.view.swap_surface(&mut self.status);
        self.row_changed_to(self.cy);
        self.rehook();
    }

    /// DECSSDT: "When the Status Line Type is changed, it is initialized.
    /// Specifically, this means that the Host Writable (Remote) Status Line
    /// is cleared, and the Indicator (Local) Status Line is set to the
    /// Set-Up indicators" (070-14 p. 14-12); "If the Status Display Type is
    /// chosen to be something other than "Host Writable", the Status
    /// Display will be exited" (p. 14-9). A type it has already is no
    /// change. Not at Level 1, and not on a storage with no row for it.
    pub(crate) fn select_status_type(&mut self, t: StatusType) {
        if self.cold.level == ConformanceLevel::One
            || (t != StatusType::None && self.strs.status.room == Room::Declined)
        {
            self.unsupported_total += 1;
            return;
        }
        if t != StatusType::HostWritable {
            self.exit_status();
        }
        if t == self.strs.status.kind {
            return;
        }
        self.strs.status.kind = t;
        self.clear_status();
    }

    /// DECSASD: "Select whether new characters are written to the Main
    /// Display or the Status Display", and "the Host Writable Status Line
    /// must be selected through DECSSDT for DECSASD to be able to select
    /// the Host Writable Status Line" (070-14 p. 14-11). Not at Level 1.
    pub(crate) fn select_active_display(&mut self, d: ActiveDisplay) {
        if self.cold.level == ConformanceLevel::One {
            self.unsupported_total += 1;
            return;
        }
        if d == ActiveDisplay::Status && self.strs.status.kind != StatusType::HostWritable {
            return;
        }
        if d == self.strs.status.active {
            return;
        }
        self.strs.status.active = d;
        self.swap_display();
    }

    /// Back to the main display, the line kept: DECSTR's "The Status
    /// Display will be exited, but not erased", DECSCL's soft reset, and a
    /// type change away from host-writable.
    pub(crate) fn exit_status(&mut self) {
        if self.strs.status.active == ActiveDisplay::Status {
            self.strs.status.active = ActiveDisplay::Main;
            if self.on_status() {
                self.swap_display();
            }
        }
    }

    /// DECSTR and DECSCL: the line left, not erased, and its own state
    /// reset where it waits, as the rest of the soft reset resets the main
    /// display's.
    pub(crate) fn soft_reset_status(&mut self) {
        self.exit_status();
        let cols = self.cols;
        self.strs.status.park.soft_reset(cols);
    }

    /// RIS: "The Status Display will be erased and exited" (070-14 p.
    /// 14-10), and the type goes back to the power-up type, none.
    pub(crate) fn reset_status(&mut self) {
        self.exit_status();
        self.strs.status.kind = StatusType::None;
        self.clear_status();
    }

    /// The line blanked and its state as a type change leaves it, while
    /// the main display is live.
    pub(crate) fn clear_status(&mut self) {
        let cols = self.cols;
        if let Some(u) = self.status.used.first_mut() {
            *u = RowExtent::ZERO;
        }
        self.status.cells.fill(PackedCell::EMPTY);
        self.strs.status.park = Parked::fresh(cols);
        self.strs.status.mark = None;
    }

    /// DECCOLM on the line's side: "Receipt of this sequence will clear
    /// both the Main Display and the Host Writable Status Line. The Status
    /// Display cursor is moved to the home position" (070-14 p. 14-9).
    /// Its state stays, as the main display's does; only the cells go, and
    /// the cursor home. Called with the main display live.
    pub(crate) fn status_column_mode(&mut self) {
        if self.strs.status.kind != StatusType::HostWritable {
            return;
        }
        if let Some(u) = self.status.used.first_mut() {
            *u = RowExtent::ZERO;
        }
        self.status.cells.fill(PackedCell::EMPTY);
        self.strs.status.park.cy = Row::ZERO;
        self.strs.status.park.cx = CursorCol::moved(Col::ZERO);
    }

    /// `f` on the main display, the line swapped out around it: what acts
    /// on the main display's lines -- the alternate screen, DECCOLM, the
    /// history's erase -- received while the line is selected.
    pub(crate) fn on_main(&mut self, f: impl FnOnce(&mut Self)) {
        let swapped = self.on_status();
        if swapped {
            self.swap_display();
        }
        f(self);
        if swapped && self.strs.status.active == ActiveDisplay::Status {
            self.swap_display();
        }
    }

    /// **A scroll on the line erases it**, within the side margins, and
    /// retires nothing: "If scrolling occurs as a result of a Format
    /// Effector, a single line Host Writable Status Line is erased" (070-14
    /// p. 14-9). `IL`, `DL`, `SU` and `SD` move its one row out the same
    /// way.
    pub(crate) fn status_scrolled(&mut self) {
        let (a, b) = match self.fenced() {
            true => (self.cold.left, self.cold.right.after()),
            false => (Col::ZERO, Col::at_index(self.cols.index())),
        };
        let blank = self.blank();
        self.erase_span(Row::ZERO, a, b, blank);
        self.row_changed_to(self.cy);
    }

    /// **A read's end**: the main display's state back in the live fields
    /// for the terminal to keep, and the indicator drawn from it. Out of
    /// line, behind the feed's one compare of the type.
    #[cold]
    #[inline(never)]
    pub(crate) fn read_ends(&mut self) {
        if self.on_status() {
            self.swap_display();
        }
        self.draw_indicator();
    }

    /// The indicator drawn for this read's end, the main display live.
    pub(crate) fn draw_indicator(&mut self) {
        if self.strs.status.kind != StatusType::Indicator {
            return;
        }
        let style = match self.strs.status.mark {
            Some(s) => s,
            None => {
                let s = self.intern_style(StyleWord::DEFAULT.with_attrs(Attrs::REVERSE));
                self.strs.status.mark = Some(s);
                s
            }
        };
        let ind = Indicators {
            at: (
                self.cy.one_based(),
                self.cx.reported(self.cold.right, self.cols).one_based(),
            ),
            wait: ModeChange::of(self.cold.modes.is_set(ModeBit::KeyboardAction)),
            // ReGIS's input position while its input runs, the envelope
            // read as open while the string ReGIS began is the last.
            locator: crate::regis::indicated(
                self.strs.regis,
                match self.cold.str_kind {
                    crate::osc::StrKind::Regis => RegisEnvelope::Open,
                    _ => RegisEnvelope::Closed,
                },
            )
            .or_else(|| crate::locator::shown(self.cold, self.rows, self.cols)),
            printer: PrinterShown::of(&self.strs.print),
        };
        compose(self.status.cells, self.cols, style, ind);
    }
}

impl Term<'_> {
    /// **The line after a resize**: the main display's width, the cells past
    /// the narrower of the two widths blanked, its margins at its edges and
    /// its cursor inside them, "If Column Mode (DECCOLM) is changed, the
    /// width of the Status Display changes" (070-14 p. 14-5). Between
    /// reads, so the line's state is the parked one.
    pub(crate) fn status_resized(&mut self, old: Cols) {
        let cols = self.cols;
        let (lo, hi) = (old.index().min(cols.index()), old.index().max(cols.index()));
        if let Some(cells) = self.store.status.cells.get_mut(lo..hi) {
            cells.fill(PackedCell::EMPTY);
        }
        let right = Col::at_index(cols.index().saturating_sub(1));
        let p = &mut self.store.strs.status.park;
        p.left = Col::ZERO;
        p.right = right;
        p.fence = right;
        p.reach = cols;
        if p.cx.col() > right {
            p.cx = CursorCol::moved(right);
        }
        self.redraw_indicator();
    }

    /// The indicator drawn again between reads, for what changes outside
    /// one: the locator moved or turned off by its report, the cursor
    /// clamped by a resize, the printer the embedder has. In the
    /// rendition a read interned for it; before any read drew it there is
    /// none, and nothing to draw.
    pub(crate) fn redraw_indicator(&mut self) {
        let st = &self.store.strs.status;
        let (StatusType::Indicator, Some(style)) = (st.kind, st.mark) else {
            return;
        };
        let ind = Indicators {
            at: (
                self.cy.one_based(),
                self.cx.reported(self.cold.right, self.cols).one_based(),
            ),
            wait: ModeChange::of(self.cold.modes.is_set(ModeBit::KeyboardAction)),
            locator: crate::regis::indicated(self.store.strs.regis, self.regis_inside())
                .or_else(|| crate::locator::shown(&self.cold, self.rows, self.cols)),
            printer: PrinterShown::of(&self.store.strs.print),
        };
        compose(self.store.status.cells, self.cols, style, ind);
    }
}
