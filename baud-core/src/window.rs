//! XTWINOPS, `CSI Ps ; Ps ; Ps t` (`[[source: ctlseqs]]`, "Window
//! manipulation (XTWINOPS)"): the window operations, the reports, the title
//! stack and the refusals, out of the dispatch's line.
//!
//! **The reports answer from what the embedder has said** (`Term::set_window`,
//! `Term::set_cell_size`) and are silent and counted where it has said
//! nothing. **`CSI 20 t` and `CSI 21 t` are refused and counted on their
//! own**: their reply is `OSC L label ST` and `OSC l label ST`, text a
//! program set put back into the input stream, which a remote program can
//! use to make the terminal type for it. **The window operations, 1-10,
//! and DECSLPP, 24 and up, are queued for the embedder** ([`WindowAsks`]),
//! which has the window: the owner's call of 2026-10-19, option 4, on by
//! default and refused, counted, while the embedder has them off
//! ([`WindowOps::Off`]).

use baud_domain::{
    ByteLen, InputEncoding, ModeChange, Osc52Writes, ParamIdx, PasteControls, Report, TitleMode,
    TitleModes, WindowAbilities, WindowOp, WindowOps, WindowRequest,
};

use crate::paint::Paint;
use crate::storage::{OSC_MAX, TITLE_SLOTS};
use crate::write::Csi;

/// How many window requests the queue holds between two drains. An embedder
/// drains after every read, as it drains the reply, so a read would have to
/// hold more than this many operations for one to be dropped -- and each
/// drop is counted ([`WindowRequestsDropped`]).
pub const WINDOW_QUEUE: usize = 16;

/// A count of the window operations refused because the embedder turned
/// them off, as a reading to compare: two equal readings saw no request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowOpsRefused(u32);

/// A count of the window requests dropped because the queue was full, as a
/// reading to compare.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowRequestsDropped(u32);

/// A count of the `OSC 3` properties dropped because no window here has
/// them, as a reading to compare: every name but the window's and the
/// icon's, and a `UTF8_STRING` value that is not UTF-8.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PropertiesDropped(u32);

impl PropertiesDropped {
    pub(crate) const FIRST: PropertiesDropped = PropertiesDropped(0);

    pub(crate) fn next(self) -> PropertiesDropped {
        PropertiesDropped(self.0.wrapping_add(1))
    }
}

/// The window requests queued for the embedder since its last drain, the
/// setting that lets them through, and the two counts of the ones that did
/// not go.
#[derive(Clone, Copy)]
pub(crate) struct WindowAsks {
    queue: [WindowRequest; WINDOW_QUEUE],
    len: QueueLen,
    pub(crate) ops: WindowOps,
    /// What the embedder carries out ([`crate::Term::set_window_abilities`]),
    /// which `OSC 61` reads.
    pub(crate) abilities: WindowAbilities,
    /// What the embedder does with an `OSC 52` write
    /// ([`crate::Term::set_osc52_writes`]), which `OSC 61` reads.
    pub(crate) clipboard: Osc52Writes,
    /// What the embedder replaces in a paste
    /// ([`crate::Term::set_paste_controls`]), which `OSC 61` reads.
    pub(crate) paste: PasteControls,
    pub(crate) refused: WindowOpsRefused,
    pub(crate) dropped: WindowRequestsDropped,
}

/// How many of [`WindowAsks`]'s slots hold a request.
#[derive(Clone, Copy)]
struct QueueLen(usize);

impl WindowAsks {
    pub(crate) const NONE: WindowAsks = WindowAsks {
        queue: [WindowRequest::Refresh; WINDOW_QUEUE],
        len: QueueLen(0),
        ops: WindowOps::On,
        abilities: WindowAbilities::NONE,
        clipboard: Osc52Writes::On,
        paste: PasteControls::Untold,
        refused: WindowOpsRefused(0),
        dropped: WindowRequestsDropped(0),
    };

    /// Count one more operation refused while they are off: a window
    /// request, or an `OSC 3`, which xterm gates under the same
    /// `allowWindowOps` (`Paint::set_property`).
    pub(crate) fn refuse(&mut self) {
        self.refused = WindowOpsRefused(self.refused.0.wrapping_add(1));
    }

    /// Queue `r`, or count it refused while the operations are off, or
    /// dropped when the queue is full.
    fn ask(&mut self, r: WindowRequest) {
        match (self.ops, self.len.0 < WINDOW_QUEUE) {
            (WindowOps::Off, _) => self.refuse(),
            (WindowOps::On, false) => {
                self.dropped = WindowRequestsDropped(self.dropped.0.wrapping_add(1))
            }
            (WindowOps::On, true) => {
                self.queue[self.len.0] = r;
                self.len = QueueLen(self.len.0 + 1);
            }
        }
    }

    /// The requests queued since the last [`WindowAsks::clear`], oldest
    /// first.
    pub(crate) fn queued(&self) -> &[WindowRequest] {
        &self.queue[..self.len.0]
    }

    pub(crate) fn clear(&mut self) {
        self.len = QueueLen(0);
    }
}

/// Which halves of each title-stack slot hold a name, how long each is
/// and the encoding its bytes are in: `None` is xterm's `NULL`, a half the
/// push did not take, which a pop fills from an older slot. `used` is
/// xterm's `saved_titles.used`, the count of pushes not yet popped.
#[derive(Clone, Copy)]
pub(crate) struct TitleStack {
    icon: [Option<(ByteLen, InputEncoding)>; TITLE_SLOTS],
    title: [Option<(ByteLen, InputEncoding)>; TITLE_SLOTS],
    used: Pushes,
}

impl TitleStack {
    pub(crate) const EMPTY: TitleStack = TitleStack {
        icon: [None; TITLE_SLOTS],
        title: [None; TITLE_SLOTS],
        used: Pushes(0),
    };
}

/// xterm's `saved_titles.used`: pushes not yet popped, which names the slot
/// the next push takes and the last pop finds, modulo [`TITLE_SLOTS`].
#[derive(Clone, Copy)]
struct Pushes(usize);

impl Pushes {
    fn slot(self) -> TitleSlot {
        TitleSlot(self.0 % TITLE_SLOTS)
    }
}

/// One of the stack's ten slots.
#[derive(Clone, Copy)]
struct TitleSlot(usize);

impl TitleSlot {
    /// The slot `back` slots older than this one, round the ring.
    fn older(self, back: TitleSlot) -> TitleSlot {
        TitleSlot((self.0 + TITLE_SLOTS - back.0) % TITLE_SLOTS)
    }

    /// Where its icon name or its title sits in the stack's bytes, `n` of
    /// them.
    fn bytes(self, half: Half, n: ByteLen) -> core::ops::Range<usize> {
        let half = match half {
            Half::Icon => 0,
            Half::Title => 1,
        };
        let at = (self.0 * 2 + half) * OSC_MAX;
        at..at + n.index()
    }
}

/// A slot's two names.
#[derive(Clone, Copy)]
enum Half {
    Icon,
    Title,
}

/// Which slot a push or a pop names: the next one, xterm's push and pop, or
/// one of the ten directly, "without pushing/popping".
#[derive(Clone, Copy)]
enum Slot {
    Next,
    Direct(TitleSlot),
}

/// Whether a pop took its slot off the stack or read it directly.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Popped {
    Yes,
    No,
}

/// Which names a push or a pop takes: `Ps` 0, 1 or 2 -- "icon and window
/// title", "icon title", "window title".
#[derive(Clone, Copy, PartialEq, Eq)]
enum Names {
    Both,
    Icon,
    Title,
}

impl Names {
    fn icon(self) -> bool {
        self != Names::Title
    }
    fn title(self) -> bool {
        self != Names::Icon
    }
}

impl Paint<'_, '_> {
    /// One XTWINOPS operation: a window request queued, a report, a push or
    /// a pop of the title stack, or a refusal.
    #[cold]
    #[inline(never)]
    pub(crate) fn window_op(&mut self, op: WindowOp, csi: &Csi) {
        match op {
            WindowOp::TitleReport => {
                self.strs.title_reports_refused = self.strs.title_reports_refused.next()
            }
            WindowOp::PushTitle | WindowOp::PopTitle => {
                let p = csi.params();
                let names = match p.raw(ParamIdx::FIRST.plus(1), 0) {
                    0 => Names::Both,
                    1 => Names::Icon,
                    _ => Names::Title,
                };
                // "Parameters in the range 1 through 10, may be used to store
                // the title into the stack or retrieve the title from the
                // stack without pushing/popping"; 0 or none pushes or pops.
                let slot = match p.raw(ParamIdx::FIRST.plus(2), 0) {
                    0 => Slot::Next,
                    n @ 1..=10 => Slot::Direct(TitleSlot(usize::from(n) - 1)),
                    _ => {
                        self.unsupported_param += 1;
                        return;
                    }
                };
                match op {
                    WindowOp::PushTitle => self.push_title(names, slot),
                    _ => self.pop_title(names, slot),
                }
            }
            _ => match WindowRequest::of(op, &csi.params()) {
                Some(r) => self.strs.window_asks.ask(r),
                None => self.window_report(op),
            },
        }
    }

    /// A report, answered from what the embedder handed over, or counted.
    fn window_report(&mut self, op: WindowOp) {
        match Report::window(
            op,
            self.rows,
            self.cols,
            self.cell_width,
            self.cell_height,
            &self.strs.window,
        ) {
            Some(r) => self.want_reply(r),
            None => self.unsupported_param += 1,
        }
    }

    /// `CSI 22 ; Ps t`: the names `Ps` picks into a slot, the next one or
    /// `direct`'s, as xterm's `xtermPushTitle` stores them (`[[source:
    /// xterm-misc]]`): the whole slot replaced, a half not taken left empty,
    /// and the eleventh push overwriting the oldest.
    fn push_title(&mut self, names: Names, slot: Slot) {
        let s = &mut *self.strs;
        let slot = match slot {
            Slot::Next => {
                let next = s.title_stack.used.slot();
                s.title_stack.used = Pushes(s.title_stack.used.0 + 1);
                next
            }
            Slot::Direct(d) => d,
        };
        s.title_stack.icon[slot.0] = None;
        s.title_stack.title[slot.0] = None;
        if names.icon() {
            let n = self.icon_len;
            s.titles[slot.bytes(Half::Icon, n)].copy_from_slice(&s.icon[..n.index()]);
            s.title_stack.icon[slot.0] = Some((n, s.icon_enc));
        }
        if names.title() {
            let n = self.title_len;
            s.titles[slot.bytes(Half::Title, n)].copy_from_slice(&s.title[..n.index()]);
            s.title_stack.title[slot.0] = Some((n, s.title_enc));
        }
    }

    /// `CSI 23 ; Ps t`: the names `Ps` picks back from the last slot pushed,
    /// or `direct`'s without popping, as xterm's `xtermPopTitle` reads them:
    /// a pop of an empty stack does nothing, a half the push did not take is
    /// filled from the nearest older slot that has one, and a half still
    /// empty leaves that name as it is, as `ChangeGroup` ignores a `NULL`.
    /// Writes `title_len` and `icon_len` on `Paint`, the copies the feed
    /// hands back to the terminal.
    fn pop_title(&mut self, names: Names, slot: Slot) {
        let s = &mut *self.strs;
        let (slot, popped) = match (slot, s.title_stack.used) {
            (Slot::Next, Pushes(0)) => return,
            (Slot::Next, Pushes(used)) => {
                s.title_stack.used = Pushes(used - 1);
                (s.title_stack.used.slot(), Popped::Yes)
            }
            (Slot::Direct(d), _) => (d, Popped::No),
        };
        // The slot itself, then each older one: xterm's `TryHigher`, ten
        // slots at most, on a pop and never on the write path.
        let find = |halves: &[Option<(ByteLen, InputEncoding)>; TITLE_SLOTS]| {
            (0..TITLE_SLOTS)
                .map(|k| slot.older(TitleSlot(k)))
                .find_map(|from| halves[from.0].map(|half| (from, half)))
        };
        let icon = find(&s.title_stack.icon).filter(|_| names.icon());
        if let Some((from, (len, enc))) = icon {
            s.icon[..len.index()].copy_from_slice(&s.titles[from.bytes(Half::Icon, len)]);
            s.icon_enc = enc;
            self.icon_len = len;
        }
        let title = find(&s.title_stack.title).filter(|_| names.title());
        if let Some((from, (len, enc))) = title {
            s.title[..len.index()].copy_from_slice(&s.titles[from.bytes(Half::Title, len)]);
            s.title_enc = enc;
            self.title_len = len;
        }
        if popped == Popped::Yes {
            s.title_stack.icon[slot.0] = None;
            s.title_stack.title[slot.0] = None;
        }
    }

    /// XTTITLEPOS, `CSI # S`: the pushes not yet popped and the stack's
    /// depth, `CSI Pn ; Pm # S`, as xterm's `xtermReportTitleStack` writes
    /// them (`[[source: xterm-misc]]`). Parameters are not read, as xterm
    /// reads none.
    pub(crate) fn report_title_position(&mut self) {
        let index = u16::try_from(self.strs.title_stack.used.0).unwrap_or(u16::MAX);
        self.want_reply(Report::TitlePosition {
            index,
            max: u16::try_from(TITLE_SLOTS).unwrap_or(u16::MAX),
        });
    }

    /// XTSMTITLE `CSI > Pm t` and XTRMTITLE `CSI > Pm T`, as xterm's
    /// `CASE_XTERM_SM_TITLE` and `CASE_XTERM_RM_TITLE` read them
    /// (`[[source: xterm-reset]]`'s file): each parameter sets or resets one
    /// mode, one past 3 or omitted is skipped, and **with none given every
    /// mode returns to the default**, "If no parameters are given, title
    /// mode features are set to the initial (compiled-in) default"
    /// (`[[source: ctlseqs]]`). xterm's own branch for that never runs, its
    /// `CSI` entry counting one omitted parameter, so its bare form changes
    /// nothing: a departure from the text not followed. **The two query modes are refused and counted
    /// with the title reports they shape**, never recorded (Jev,
    /// REFUSE_COUNT, 0.90/0.90); resetting one changes nothing, as it is
    /// never set. A value past 3 is counted.
    #[cold]
    #[inline(never)]
    pub(crate) fn title_modes(&mut self, change: ModeChange, csi: &Csi) {
        let mut given = false;
        csi.for_each_param(|v| given |= v.is_some());
        if !given {
            self.strs.title_modes = TitleModes::DEFAULT;
            return;
        }
        csi.for_each_param(|v| match v.map(TitleMode::from_param) {
            None => {}
            Some(Some(m)) if m.is_query() && change == ModeChange::Set => {
                self.strs.title_reports_refused = self.strs.title_reports_refused.next();
            }
            Some(Some(m)) if m.is_query() => {}
            Some(Some(m)) => self.strs.title_modes = self.strs.title_modes.with(m, change),
            Some(None) => self.unsupported_param += 1,
        });
    }
}

/// The encoding a title or icon name's bytes are in, as xterm's
/// `ChangeGroup` decides it (`[[source: xterm-misc]]`): **read as UTF-8
/// where the UTF-8 title mode is in force and the bytes are valid UTF-8,
/// else as ISO 8859-1.** Under UTF-8 the mode is always in force, as
/// xterm's `utf8Title` resource follows the encoding by default (Jev,
/// RESOURCE_FOLLOWS, 0.96/0.98), and a title that arrived as text, not in
/// hexadecimal, is UTF-8 there, its bytes having come through as a
/// character's.
pub(crate) fn label_encoding(
    string: InputEncoding,
    modes: TitleModes,
    sent: Spelled,
    bytes: &[u8],
) -> InputEncoding {
    let utf8_titles = string == InputEncoding::Utf8 || modes.has(TitleMode::SetUtf8);
    match (string, sent) {
        (InputEncoding::Utf8, Spelled::Text) => InputEncoding::Utf8,
        _ if utf8_titles && core::str::from_utf8(bytes).is_ok() => InputEncoding::Utf8,
        _ => InputEncoding::Latin1,
    }
}

/// How a name arrived: as its own bytes, or in hexadecimal (XTSMTITLE 0).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spelled {
    Text,
    Hex,
}

/// Whether an `OSC` body kept so far names a title or an icon name, `0;`,
/// `1;` or `2;`: under ISO 8859-1 with the UTF-8 title mode set, the rest
/// of such a body keeps its GR bytes (`Paint::osc_gr`).
pub(crate) fn names_a_label(kept: &[u8]) -> bool {
    matches!(kept, [b'0' | b'1' | b'2', b';', ..])
}
