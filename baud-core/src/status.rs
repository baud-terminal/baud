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
//! settings" (070-14 §14.2). A read that writes it swaps the [`View`] onto
//! its surface and the main display's own state into [`Parked`], as `?1049`
//! swaps the alternate screen's surface, so the per-character loop never
//! learns which display is up: on a screen of one row, `CUP`'s line is
//! clamped away, `CUU` and `CUD` move nowhere, `CPR` reports line 1, and a
//! line feed at the bottom scrolls the one row out, which 070-14 asks for
//! -- "If scrolling occurs as a result of a Format Effector, a single line
//! Host Writable Status Line is erased". What each display keeps is
//! 070-14's list, §14.2.2: the active position and its pending wrap, the
//! rendition and DECSCA, origin mode, G0-G3, GL, GR and the single shifts,
//! and the `DECSC` slot -- the character sets separate where the VT420
//! shares them, the wrap's mode shared and its pending flag each display's.
//! The cursor's visibility is each display's too, 070-14 item 14, "Each of
//! the Main and Status Display cursors may be individually enabled or
//! disabled". **SLH's line home and SLL's line limit are each display's**,
//! parked with its margins: with the line selected "most ANSI control
//! functions received will affect only the Status Display" (070-14
//! §14.2.3, p. 14-8), and SLH and SLL are in none of its exceptions, so a
//! home or a limit set while one display is written leaves the other's
//! where it was, a lean where the texts are silent, re-examined on
//! 2026-11-02 with xterm's practice read from its code; ECMA-48's "remains
//! in effect until the next occurrence of SLH in the data stream"
//! (8.3.122), one stream for both displays, weighs the other way. xterm,
//! which has no SLH or SLL, keeps the inverse split: `CursorSave2()` copies
//! all of `xw->flags` into each display's saved cursor and
//! `CursorRestore2()` restores them with `ALL_FLAGS`, DECAWM, LNM, DECOM,
//! IRM, DECSCLM, DECSCNM and DECLRMM among them, while its margins stay the
//! screen's (`[[source: xterm-cursor-c]]` 398-416, 460-512;
//! `[[source: xterm-ptyx-h]]` 2346-2363, 2833-2836, 3757-3768), departing
//! from 070-14's items 9, 10 and 16 and the VT420's list. baud's split is
//! positions and regions each display's, and modes the terminal's except
//! those a text gives each display, origin mode and the cursor's
//! visibility. The rest is shared: tabs, IRM, DECAWM and LNM, DECSCLM,
//! DECSCNM, the width, and BDSM and DCSM, the terminal's, under which each
//! display's limit bounds or not and the line is laid out. IRM "Affects the
//! main display and the status line" and DECAWM "Affects the main display
//! and status line" (`[[source: vt420-rm]]` p. 221), and LNM, which neither
//! text names, sets the Return key, which is one for both displays ("All
//! keyboard functions are executed regardless of the selected display",
//! 070-14 item 17). BDSM is both
//! displays' by the 2026-10-26 layout decision, which lays the line out
//! "under the terminal's BDSM" (`docs/bidi.md`, the status line's row),
//! and DCSM is kept with it; §14.2.3's general clause lists neither among
//! its exceptions, an open question (`docs/vt-coverage.md`, the SLH / SLL
//! row). **DECLRMM is the terminal's and the side margins each display's**,
//! so the mode's reset on either display puts back both displays' margins,
//! the parked one's here ([`Parked::reset_side_margins`]), and its set
//! makes both displays' lines single-width, the line's row with the main
//! display's (`Paint::single_width_pages`), a wrap pending at a
//! double-width row's half kept waiting there ([`Parked::single_width`]).
//! Which display the mode belongs to is an open question, read both ways.
//! For both displays, DECLRMM's own text speaks of "every page" and of
//! "Page Memory for the Session" (070-5 p. 5-29). For the line alone, with
//! the line selected "most ANSI control functions received will affect
//! only the Status Display" (070-14 §14.2.3, p. 14-8), and DECLRMM is not
//! among its exceptions; and the VT420's "most of the control functions
//! that affect the main display also affect the status line"
//! (`[[source: vt420-rm]]` p. 221) carries an exceptions list in which
//! DECSCLM and IRM read "Affects the main display and the status line",
//! and tab stops and DECAWM the same, so its rule has a control act on the
//! line rather than on both displays (the `?69` row). DECCOLM puts back
//! the line's margins and homes its cursor too, whatever `?95` says.
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
use crate::term::{Cold, GuardStart, Pinned, Saved, Screen, Watched};
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
    /// SLH's home and SLL's limit, the display's own (`crate::bounds`).
    bounds: crate::bounds::LineBounds,
    last_cp: Option<(Codepoint, Width, Pen)>,
    cluster: ClusterBreak,
    guard_open: Option<GuardStart>,
    pinned: Pinned,
    left_edge: Row,
    watched: Watched,
    armed: Option<crate::margin::Line>,
    reach: Cols,
    /// The screen: the main display's, or for the line the alternate
    /// screen's, which keeps no history, so a scroll on the line retires
    /// nothing.
    screen: Screen,
}

impl Parked {
    /// The link indexes the parked display keeps -- its open link and the
    /// pen built from it -- for the link table's sweep (`links.rs`), which
    /// must not free a slot it will write with again.
    pub(crate) fn links(&self) -> [LinkId; 2] {
        [self.link, self.flags.link()]
    }

    /// The main display's reach past the live edge and its screen, while
    /// the line is the one written: where the link table's sweep finds the
    /// primary's cells past a narrower edge (`links.rs`).
    pub(crate) fn past_edge(&self) -> (Cols, Screen) {
        (self.reach, self.screen)
    }

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
            bounds: crate::bounds::LineBounds::NONE,
            last_cp: None,
            cluster: ClusterBreak::new(),
            guard_open: None,
            pinned: Pinned::Before,
            left_edge: Row::ZERO,
            watched: Watched::Nothing,
            armed: None,
            reach: cols,
            screen: Screen::Alternate,
        }
    }
}

impl Parked {
    /// **The parked display's side margins put back**, by DECLRMM's reset
    /// or DECCOLM on the other: DECLRMM is the terminal's and the margins
    /// each display's, so its reset leaves neither display with a margin no
    /// mode admits to. A wrap pending at the old right margin keeps waiting
    /// on it, as on the display written ([`crate::ops::kept_at_margin`]);
    /// the fence is taken again when the display is swapped in.
    pub(crate) fn reset_side_margins(&mut self, cols: Cols) {
        let right = Col::at_index(cols.index().saturating_sub(1));
        self.cx = crate::ops::kept_at_margin(self.cx, self.right, cols);
        self.left = Col::ZERO;
        self.right = right;
        self.fence = right;
    }

    /// **The fence the parked display's cursor is read against** on a row
    /// of rendition `r`, as a report reads the display written
    /// ([`crate::bounds::report_fence`]): the right margin, the half's last
    /// column on a double-width row ([`crate::paint::fence_of`]), or the
    /// display's SLL limit while a wrap waits there.
    pub(crate) fn report_fence(&self, r: LineRendition, cold: &Cold, cols: Cols) -> Col {
        let own = crate::paint::fence_of(r, self.right, cols);
        match self.pinned == Pinned::AtLimit {
            true => crate::bounds::limited_by(own, self.bounds, cold, cols),
            false => own,
        }
    }

    /// **The line's row made single-width by DECLRMM's set**, `r` its
    /// rendition until then: a wrap pending at a double-width row's half
    /// keeps waiting there, as on the main display
    /// ([`crate::ops::kept_at_half`]). The fence is taken again when the
    /// line is swapped in.
    pub(crate) fn single_width(&mut self, r: LineRendition, cold: &Cold, cols: Cols) {
        if !r.is_double() {
            return;
        }
        let fence = self.report_fence(r, cold, cols);
        if let Some(cx) = crate::ops::kept_at_half(self.cx, fence, self.right, cols) {
            self.cx = cx;
            self.pinned = Pinned::Before;
        }
    }

    /// **The parked display's cursor home**, by DECCOLM on the other: a
    /// move, so it ends a wrap waiting, as [`Paint::set_col`] ends the
    /// written display's.
    fn home(&mut self) {
        self.cy = Row::ZERO;
        self.cx = CursorCol::moved(Col::ZERO);
        self.pinned = Pinned::Before;
    }

    /// **DECSTR's reset of the line's own state**, kept parked: the
    /// rendition, DECSCA, the character sets, origin mode, the cursor
    /// shown, nothing saved and margins at its edges, as the soft reset
    /// leaves the main display's; its position stays, as the main
    /// display's does, and its line home and limit, which `DECSTR` keeps. A
    /// wrap pending at a right margin narrower than the line keeps waiting
    /// on it, as the main display's does ([`Paint::reset_side_margins`]).
    fn soft_reset(&mut self, cols: Cols) {
        self.reset_side_margins(cols);
        let fresh = Parked::fresh(cols);
        *self = Parked {
            cy: self.cy,
            cx: self.cx,
            bounds: self.bounds,
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
    /// The string the display not being written has open, which a sweep of
    /// SDS's and SRS's table keeps (`crate::strings`).
    pub(crate) fn parked_string(&self) -> StringId {
        self.park.style_raw.string()
    }

    /// 070-14's "No Status Line (factory default)", baud's power-up
    /// type.
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
    /// column the cell is drawn at, and a pending wrap's the one CPR
    /// reports -- the right margin, the half's last column on a
    /// double-width row `r`, or the line's own SLL limit while a wrap waits
    /// there ([`Parked::report_fence`]).
    pub(crate) fn park_cursor(&self, r: LineRendition, cold: &Cold, cols: Cols) -> Option<Col> {
        let fence = self.park.report_fence(r, cold, cols);
        (self.park.cursor == ModeChange::Set).then(|| self.park.cx.reported(fence, cols))
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
    /// `text` from `at`, nothing written past the line's end; the column
    /// after it. [`Line::whole`] measures a field by writing it to a line with no cells.
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

    /// **A field whole or not at all**: `field` written from `at` when its
    /// last character is on the line, else nothing. Measured first on a
    /// line of no cells, where it writes nothing and ends where it would.
    fn whole(&mut self, at: Col, field: impl Fn(&mut Line<'_>, Col) -> Col) {
        let mut none = Line {
            cells: &mut [],
            style: self.style,
        };
        if field(&mut none, at).index() <= self.cells.len() {
            field(self, at);
        }
    }
}

/// Write the indicator into `cells`, `cols` of them, in `style`.
///
/// **Each field is shown whole or left out whole**, so no field on the
/// line is a cut, wrong value: cut at the window's edge, 52 columns read
/// `Printer: No` and 12 columns, with the cursor on row 100, column 12,
/// read `1(100,01`, a column 1 the cursor is not on. The Active Position
/// Indicator "Displays the current cursor position, row and column" and
/// the Printer Status Indicator "Indicates the status of the attached
/// printer" (070-14 p. 14-6). The fixed columns leave the printer out
/// first, below 54 to 60 columns by its value, then the locator and
/// Wait, and the cursor position last, below 13 or 14.
///
/// **This is the less wrong form, and not conformance.** The line has
/// "the same number of columns as the main Logical Display (usually 80
/// or 132)", where the extension "provides the ability to display at
/// least one additional line of 80 characters" (p. 14-4), and below 80
/// columns no layout of the line meets that: a field the window cannot
/// hold is a required indicator not shown, as a cut one is a wrong one.
/// What is shown is never wrong.
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
    line.whole(Col::at_index(AT_PAGE), |l, x| {
        let x = l.put(x, b"1(");
        let x = l.number(x, ind.at.0, Cols::of_index(2));
        let x = l.put(x, b",");
        let x = l.number(x, ind.at.1, Cols::of_index(3));
        l.put(x, b")")
    });
    if ind.wait == ModeChange::Set {
        line.whole(Col::at_index(AT_WAIT), |l, x| l.put(x, b"Wait"));
    }
    if let Some((y, x)) = ind.locator {
        line.whole(Col::at_index(AT_LOCATOR), |l, at| {
            let at = l.put(at, b"[");
            let at = l.number(at, y, Cols::of_index(1));
            let at = l.put(at, b",");
            let at = l.number(at, x, Cols::of_index(1));
            l.put(at, b"]")
        });
    }
    line.whole(Col::at_index(AT_PRINTER), |l, x| {
        l.put(x, ind.printer.text())
    });
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
        core::mem::swap(&mut self.cold.bounds, &mut p.bounds);
        core::mem::swap(&mut self.last_cp, &mut p.last_cp);
        core::mem::swap(&mut self.cluster, &mut p.cluster);
        core::mem::swap(&mut self.cold.guard_open, &mut p.guard_open);
        core::mem::swap(&mut self.cold.pinned, &mut p.pinned);
        core::mem::swap(&mut self.cold.left_edge, &mut p.left_edge);
        core::mem::swap(&mut self.cold.watched, &mut p.watched);
        core::mem::swap(&mut self.cold.bell.armed, &mut p.armed);
        core::mem::swap(&mut self.book.reach, &mut p.reach);
        core::mem::swap(&mut self.cold.screen, &mut p.screen);
        self.strs.status.park = p;
        self.strs.status.swapped = into;
        self.view.swap_surface(&mut self.status);
        self.row_changed_to(self.cy);
        // **What this display's home and limit do is taken again**: they
        // came back with it, and BDSM and DCSM, both displays', may have
        // changed while it was parked. So the modes that read them, the
        // fence and a wrap left waiting at a limit that no longer bounds
        // follow, as on the display written (`crate::bounds`), and the
        // graph is chosen again. Parked, the fence kept the limit it was
        // left with, and the home and limit were one for both displays.
        self.refresh_presentation_edits();
        // DECHEBM, global, may have changed while this display was parked,
        // and with it how GL's ASCII is drawn (`crate::hebrew_keys`).
        if self.gl_set() != self.charset {
            self.invoke();
        }
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
        // The active position goes to another display, which ends an
        // aligned string.
        self.effector_ends_string();
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
    /// the cursor home. **`?95` withholds the clear and not the home**, as
    /// it withholds only the main display's clear ("When enabled, a column
    /// mode change ... does not clear the screen", `[[source:
    /// vt510-decncsm]]`). Called with the main display live.
    pub(crate) fn status_column_mode(&mut self) {
        if self.strs.status.kind != StatusType::HostWritable {
            return;
        }
        if !self.cold.modes.is_set(ModeBit::NoClearOnColumn) {
            if let Some(u) = self.status.used.first_mut() {
                *u = RowExtent::ZERO;
            }
            self.status.cells.fill(PackedCell::EMPTY);
        }
        self.strs.status.park.home();
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
                self.cx.reported(self.report_fence(), self.cols).one_based(),
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
    /// reads, so the line's state is the parked one. **A wrap pending on
    /// the line stays pending**, on the column it waited on clamped to the
    /// line's new end, as the main display's does
    /// ([`Term::resize_cursor_and_margins`]): the line dropped one waiting
    /// at a right margin, and the next character went one past it, or at
    /// its last column, and the next character went over it.
    pub(crate) fn status_resized(&mut self, old: Cols) {
        let cols = self.cols;
        let (lo, hi) = (old.index().min(cols.index()), old.index().max(cols.index()));
        if let Some(cells) = self.store.status.cells.get_mut(lo..hi) {
            cells.fill(PackedCell::EMPTY);
        }
        let r = self
            .store
            .status
            .used
            .first()
            .map_or(LineRendition::Single, |u| u.rendition());
        let right = Col::at_index(cols.index().saturating_sub(1));
        let p = &mut self.store.strs.status.park;
        let was = p.report_fence(r, &self.cold, old);
        let waited = match p.cx.at(was, old) {
            CursorAt::Pending(_) => Some(p.cx.reported(was, old)),
            CursorAt::Column(_) | CursorAt::Placed(_) => None,
        };
        p.left = Col::ZERO;
        p.right = right;
        p.fence = right;
        p.reach = cols;
        let clamped = p.cx.col().min(right);
        if clamped != p.cx.raw() {
            p.pinned = Pinned::Before;
        }
        p.cx = CursorCol::moved(clamped);
        let own = crate::paint::fence_of(r, right, cols);
        if p.cx.raw() > own {
            p.cx = CursorCol::moved(own);
        }
        if let Some(w) = waited {
            let w = w.min(own);
            p.cx = match w == own {
                true => CursorCol::written(w.after()),
                false => CursorCol::waiting(w),
            };
            p.pinned = Pinned::Before;
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
                self.cx.reported(self.report_fence(), self.cols).one_based(),
            ),
            wait: ModeChange::of(self.cold.modes.is_set(ModeBit::KeyboardAction)),
            locator: crate::regis::indicated(self.store.strs.regis, self.regis_inside())
                .or_else(|| crate::locator::shown(&self.cold, self.rows, self.cols)),
            printer: PrinterShown::of(&self.store.strs.print),
        };
        compose(self.store.status.cells, self.cols, style, ind);
    }
}
