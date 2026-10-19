//! XTWINOPS, `CSI Ps ; Ps ; Ps t` (`[[source: ctlseqs]]`, "Window
//! manipulation (XTWINOPS)"): the reports, the title stack and the refusals,
//! out of the dispatch's line.
//!
//! **The reports answer from what the embedder has said** (`Term::set_window`,
//! `Term::set_cell_size`) and are silent and counted where it has said
//! nothing. **`CSI 20 t` and `CSI 21 t` are refused and counted on their
//! own**: their reply is `OSC L label ST` and `OSC l label ST`, text a
//! program set put back into the input stream, which a remote program can
//! use to make the terminal type for it. **The window-changing operations,
//! 1-10, and DECSLPP, 24 and up, are counted**: whether baud builds them is
//! the owner's call.

use baud_domain::{ByteLen, ParamIdx, Report, WindowOp};

use crate::paint::Paint;
use crate::storage::{OSC_MAX, TITLE_SLOTS};
use crate::write::Csi;

/// Which halves of each title-stack slot hold a name, and how long each is:
/// `None` is xterm's `NULL`, a half the push did not take, which a pop
/// fills from an older slot. `used` is xterm's `saved_titles.used`, the
/// count of pushes not yet popped.
#[derive(Clone, Copy)]
pub(crate) struct TitleStack {
    icon: [Option<ByteLen>; TITLE_SLOTS],
    title: [Option<ByteLen>; TITLE_SLOTS],
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
    /// One XTWINOPS operation: a report, a push or a pop of the title stack,
    /// or a refusal.
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
            _ => match Report::window(
                op,
                self.rows,
                self.cols,
                self.cell_width,
                self.cell_height,
                &self.strs.window,
            ) {
                Some(r) => self.want_reply(r),
                None => self.unsupported_param += 1,
            },
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
            s.title_stack.icon[slot.0] = Some(n);
        }
        if names.title() {
            let n = self.title_len;
            s.titles[slot.bytes(Half::Title, n)].copy_from_slice(&s.title[..n.index()]);
            s.title_stack.title[slot.0] = Some(n);
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
        let find = |halves: &[Option<ByteLen>; TITLE_SLOTS]| {
            (0..TITLE_SLOTS)
                .map(|k| slot.older(TitleSlot(k)))
                .find_map(|from| halves[from.0].map(|len| (from, len)))
        };
        let icon = find(&s.title_stack.icon).filter(|_| names.icon());
        if let Some((from, len)) = icon {
            s.icon[..len.index()].copy_from_slice(&s.titles[from.bytes(Half::Icon, len)]);
            self.icon_len = len;
        }
        let title = find(&s.title_stack.title).filter(|_| names.title());
        if let Some((from, len)) = title {
            s.title[..len.index()].copy_from_slice(&s.titles[from.bytes(Half::Title, len)]);
            self.title_len = len;
        }
        if popped == Popped::Yes {
            s.title_stack.icon[slot.0] = None;
            s.title_stack.title[slot.0] = None;
        }
    }
}
