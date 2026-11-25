//! **The notifications a window has handed to its platform**, one slot per
//! [`NoticeLiveIdx`]: the slot a [`NoticeToken`] names in the desk is the
//! slot it holds here, so the desk and the platform's table forget the same
//! notification when a new one takes its slot.

use core::time::Duration;

use crate::{
    Expiry, HostTime, NOTICE_BUTTONS, NOTICE_LIVE, NoticeButton, NoticeLiveIdx, NoticeToken,
};

/// **The buttons a notification showed**, each by its number: a click is
/// read back against them ([`NoticeButtonSet::numbered`]), and a number
/// naming none of them names nothing.
///
/// ```
/// let none: baud_domain::NoticeButtonSet = baud_domain::NoticeButtonSet::of([]);
/// assert_eq!(none, baud_domain::NoticeButtonSet::NONE);
/// assert_eq!(none.numbered(1), None);
/// ```
///
/// Outside this crate no set is written as a mask:
///
/// ```compile_fail,E0423
/// let all: baud_domain::NoticeButtonSet = baud_domain::NoticeButtonSet(0xff);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct NoticeButtonSet(u8);

const _: () = assert!(NOTICE_BUTTONS <= u8::BITS as usize);

impl NoticeButtonSet {
    /// No button.
    pub const NONE: NoticeButtonSet = NoticeButtonSet(0);

    /// The set of `buttons`.
    pub fn of(buttons: impl IntoIterator<Item = NoticeButton>) -> NoticeButtonSet {
        NoticeButtonSet(
            buttons
                .into_iter()
                .fold(0, |m, b| m | 1 << (b.number() - 1)),
        )
    }

    /// The button numbered `number`, when it is one of these.
    pub fn numbered(self, number: u8) -> Option<NoticeButton> {
        let b = NoticeButton::of_index(usize::from(number.checked_sub(1)?))?;
        (self.0 & 1 << (b.number() - 1) != 0).then_some(b)
    }
}

/// **A set of [`NoticeSlots`]' slots**, read in slot order: the ones whose
/// deadline passed ([`NoticeSlots::due`]) or the ones the platform's list
/// no longer holds ([`NoticeSlots::absent`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NoticeSlotSet(u32);

const _: () = assert!(NOTICE_LIVE <= u32::BITS as usize);

impl NoticeSlotSet {
    /// No slot.
    pub const EMPTY: NoticeSlotSet = NoticeSlotSet(0);

    /// Whether it holds no slot.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn with(self, i: NoticeLiveIdx) -> NoticeSlotSet {
        NoticeSlotSet(self.0 | 1 << i.index())
    }

    fn without(self, i: NoticeLiveIdx) -> NoticeSlotSet {
        NoticeSlotSet(self.0 & !(1 << i.index()))
    }
}

impl Iterator for NoticeSlotSet {
    type Item = NoticeLiveIdx;

    fn next(&mut self) -> Option<NoticeLiveIdx> {
        let i = NoticeLiveIdx::of_index(self.0.trailing_zeros() as usize)?;
        self.0 &= self.0 - 1;
        Some(i)
    }
}

/// Whether the platform has said it scheduled a shown notification: one
/// posted pending is not yet looked for in the platform's list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scheduling {
    Pending,
    Scheduled,
}

/// **What a post asked of the platform**: the expiry and the buttons it
/// shows.
#[derive(Clone, Copy, Debug)]
struct Asked {
    expiry: Expiry,
    buttons: NoticeButtonSet,
}

/// **One slot's notification**, through the states it passes.
#[derive(Clone, Debug)]
enum NoticeSlot<P> {
    Free,
    /// Sent to the platform, its id not yet answered: the expiry it asked
    /// and the buttons it shows.
    Posting {
        token: NoticeToken,
        expiry: Expiry,
        buttons: NoticeButtonSet,
    },
    /// On the platform as `id`, closing at `due` where its expiry gave a
    /// deadline; shown, or posted pending, at `at`. An update of it sent
    /// and not yet answered is `update`, what that post asked.
    Shown {
        token: NoticeToken,
        id: P,
        due: Option<HostTime>,
        at: HostTime,
        buttons: NoticeButtonSet,
        scheduling: Scheduling,
        update: Option<Asked>,
    },
    /// Its deadline passed and its close asked; alive until the platform
    /// says it closed, so a click still reads back. An update of it sent
    /// and not yet answered is `update`.
    Closing {
        token: NoticeToken,
        id: P,
        buttons: NoticeButtonSet,
        update: Option<Asked>,
    },
}

impl<P> NoticeSlot<P> {
    fn token(&self) -> Option<NoticeToken> {
        match self {
            NoticeSlot::Free => None,
            NoticeSlot::Posting { token, .. }
            | NoticeSlot::Shown { token, .. }
            | NoticeSlot::Closing { token, .. } => Some(*token),
        }
    }

    /// The id and buttons of one on the platform.
    fn shown(&self) -> Option<(NoticeToken, &P, NoticeButtonSet)> {
        match self {
            NoticeSlot::Shown {
                token, id, buttons, ..
            }
            | NoticeSlot::Closing {
                token, id, buttons, ..
            } => Some((*token, id, *buttons)),
            NoticeSlot::Free | NoticeSlot::Posting { .. } => None,
        }
    }

    fn due(&self) -> Option<HostTime> {
        match self {
            NoticeSlot::Shown { due, .. } => *due,
            _ => None,
        }
    }
}

/// **The notifications on the platform or sent to it, by slot**, with the
/// nearest deadline `w` set kept as each one is entered or left, so a turn
/// with none reads one field.
///
/// A slot is read only by a token that holds it: one a newer notification
/// took reads `None`. A notification is let go of by the handle
/// ([`NoticeHeld`]) its token gives, which `free` consumes:
///
/// ```
/// # let mut area = vec![0u8; baud_domain::NoticeBudget::with_icon(0).bytes()];
/// # let mut desk = baud_domain::NoticeDesk::NEW;
/// # desk.set_support(Some(baud_domain::NoticeSupport {
/// #     actions: baud_domain::Activation::FOCUS,
/// #     close_events: baud_domain::CloseEvents::Tracked,
/// #     occasions: baud_domain::Occasions::Always,
/// #     urgencies: baud_domain::Urgencies::None,
/// #     sounds: baud_domain::Sounds::SystemAndSilent,
/// #     expiry: baud_domain::ExpirySupport::None,
/// #     icons: baud_domain::IconSupport::NamesOnly,
/// #     buttons: baud_domain::ButtonSupport::NotShown,
/// # }));
/// # desk.simple(&mut area, baud_domain::NoticeFrom::Osc9, b"done", b"");
/// # let t = desk.notices(&area).next().unwrap().token;
/// let mut slots: baud_domain::NoticeSlots<u32> = baud_domain::NoticeSlots::new();
/// let now = baud_domain::HostTime::ZERO;
/// slots.posting(t, baud_domain::Expiry::System, baud_domain::NoticeButtonSet::NONE);
/// assert_eq!(slots.notified(t, 7, now), Some(t));
/// let h: baud_domain::NoticeHeld<'_, u32> = slots.get(t).unwrap();
/// assert_eq!(h.id(), Some(&7));
/// h.free();
/// assert_eq!(slots.id(t), None);
/// ```
///
/// and nothing is read through a handle once it is freed:
///
/// ```compile_fail,E0382
/// # let mut area = vec![0u8; baud_domain::NoticeBudget::with_icon(0).bytes()];
/// # let mut desk = baud_domain::NoticeDesk::NEW;
/// # desk.set_support(Some(baud_domain::NoticeSupport {
/// #     actions: baud_domain::Activation::FOCUS,
/// #     close_events: baud_domain::CloseEvents::Tracked,
/// #     occasions: baud_domain::Occasions::Always,
/// #     urgencies: baud_domain::Urgencies::None,
/// #     sounds: baud_domain::Sounds::SystemAndSilent,
/// #     expiry: baud_domain::ExpirySupport::None,
/// #     icons: baud_domain::IconSupport::NamesOnly,
/// #     buttons: baud_domain::ButtonSupport::NotShown,
/// # }));
/// # desk.simple(&mut area, baud_domain::NoticeFrom::Osc9, b"done", b"");
/// # let t = desk.notices(&area).next().unwrap().token;
/// let mut slots: baud_domain::NoticeSlots<u32> = baud_domain::NoticeSlots::new();
/// let now = baud_domain::HostTime::ZERO;
/// slots.posting(t, baud_domain::Expiry::System, baud_domain::NoticeButtonSet::NONE);
/// assert_eq!(slots.notified(t, 7, now), Some(t));
/// let h: baud_domain::NoticeHeld<'_, u32> = slots.get(t).unwrap();
/// h.free();
/// assert_eq!(h.id(), Some(&7));
/// ```
#[derive(Clone, Debug)]
pub struct NoticeSlots<P> {
    slots: [NoticeSlot<P>; NOTICE_LIVE],
    /// The nearest deadline any slot holds.
    earliest: Option<HostTime>,
    /// The slots that hold a deadline.
    timed: NoticeSlotSet,
    /// The deadlines looked at, for the test that an idle turn looks at
    /// none.
    #[cfg(test)]
    looks: core::cell::Cell<u32>,
}

/// **The notification a token holds**, from [`NoticeSlots::get`] or
/// [`NoticeSlots::at`]: read, then let go of by [`NoticeHeld::free`],
/// which takes the handle.
#[derive(Debug)]
pub struct NoticeHeld<'a, P> {
    slots: &'a mut NoticeSlots<P>,
    i: NoticeLiveIdx,
}

impl<P> NoticeHeld<'_, P> {
    /// Its token.
    pub fn token(&self) -> NoticeToken {
        self.slots.slots[self.i.index()]
            .token()
            .expect("a held slot holds a token")
    }

    /// The platform's id for it; `None` while it is still being sent.
    pub fn id(&self) -> Option<&P> {
        self.slots.slots[self.i.index()]
            .shown()
            .map(|(_, id, _)| id)
    }

    /// Let go of: the slot is free.
    pub fn free(self) {
        let _ = self.slots.put(self.i, NoticeSlot::Free);
    }
}

impl<P> NoticeSlots<P> {
    pub fn new() -> NoticeSlots<P> {
        NoticeSlots {
            slots: core::array::from_fn(|_| NoticeSlot::Free),
            earliest: None,
            timed: NoticeSlotSet::EMPTY,
            #[cfg(test)]
            looks: core::cell::Cell::new(0),
        }
    }

    /// `token` sent to the platform, its id not yet answered, with the
    /// expiry it asked and the buttons it shows. An update of one on the
    /// platform keeps it as it is -- its id, its buttons and its deadline
    /// -- until the platform answers ([`NoticeSlots::notified`]) or refuses
    /// ([`NoticeSlots::refused`]) the update; whatever else its slot held is
    /// forgotten.
    pub fn posting(&mut self, token: NoticeToken, expiry: Expiry, buttons: NoticeButtonSet) {
        let i = token.slot();
        match &mut self.slots[i.index()] {
            NoticeSlot::Shown {
                token: t, update, ..
            }
            | NoticeSlot::Closing {
                token: t, update, ..
            } if *t == token => *update = Some(Asked { expiry, buttons }),
            _ => {
                let _ = self.put(
                    i,
                    NoticeSlot::Posting {
                        token,
                        expiry,
                        buttons,
                    },
                );
            }
        }
    }

    /// **The platform's refusal of a post of `token`**: a refused update
    /// leaves the one on the platform as it was, its id, its buttons and
    /// its deadline; a post never shown lets its slot go. Nothing changes
    /// unless its slot holds `token`.
    pub fn refused(&mut self, token: NoticeToken) {
        let i = token.slot();
        match &mut self.slots[i.index()] {
            NoticeSlot::Posting { token: t, .. } if *t == token => {
                let _ = self.put(i, NoticeSlot::Free);
            }
            NoticeSlot::Shown {
                token: t, update, ..
            }
            | NoticeSlot::Closing {
                token: t, update, ..
            } if *t == token => *update = None,
            _ => {}
        }
    }

    /// **The platform's answer to a post: `token` shown as `id` at `now`**,
    /// closing at `now` plus its expiry where it has one. `None`, and
    /// nothing changed, unless its slot holds `token`. An update's answer
    /// shows it as `id` with the expiry and buttons the update asked. A
    /// second answer with no update waiting -- a post sent again before the
    /// first answer came -- is shown as the later id, its deadline kept.
    pub fn notified(&mut self, token: NoticeToken, id: P, now: HostTime) -> Option<NoticeToken> {
        let i = token.slot();
        let next = match &self.slots[i.index()] {
            NoticeSlot::Posting {
                token: t,
                expiry,
                buttons,
            } if *t == token => NoticeSlot::Shown {
                token,
                id,
                due: deadline(*expiry, now),
                at: now,
                buttons: *buttons,
                scheduling: Scheduling::Scheduled,
                update: None,
            },
            NoticeSlot::Shown {
                token: t,
                update: Some(asked),
                ..
            }
            | NoticeSlot::Closing {
                token: t,
                update: Some(asked),
                ..
            } if *t == token => NoticeSlot::Shown {
                token,
                id,
                due: deadline(asked.expiry, now),
                at: now,
                buttons: asked.buttons,
                scheduling: Scheduling::Scheduled,
                update: None,
            },
            NoticeSlot::Shown {
                token: t,
                due,
                at,
                buttons,
                scheduling,
                update: None,
                ..
            } if *t == token => NoticeSlot::Shown {
                token,
                id,
                due: *due,
                at: *at,
                buttons: *buttons,
                scheduling: *scheduling,
                update: None,
            },
            NoticeSlot::Closing {
                token: t,
                buttons,
                update: None,
                ..
            } if *t == token => NoticeSlot::Closing {
                token,
                id,
                buttons: *buttons,
                update: None,
            },
            _ => return None,
        };
        self.put(i, next);
        Some(token)
    }

    /// **`token` posted as the platform's `id` at `now`, not yet
    /// scheduled**: alive, closing at `now` plus its expiry where it has
    /// one, and not looked for in the platform's list until
    /// [`NoticeSlots::scheduled`] says it is held. Whatever its slot held is
    /// forgotten.
    pub fn pending(
        &mut self,
        token: NoticeToken,
        id: P,
        expiry: Expiry,
        now: HostTime,
        buttons: NoticeButtonSet,
    ) {
        self.put(
            token.slot(),
            NoticeSlot::Shown {
                token,
                id,
                due: deadline(expiry, now),
                at: now,
                buttons,
                scheduling: Scheduling::Pending,
                update: None,
            },
        );
    }

    /// The slots that hold a notification, sent or shown.
    pub fn held(&self) -> NoticeSlotSet {
        NoticeLiveIdx::ALL
            .into_iter()
            .filter(|i| self.slots[i.index()].token().is_some())
            .fold(NoticeSlotSet::EMPTY, NoticeSlotSet::with)
    }

    /// The token held by the slot `i`, when one is.
    pub fn token_at(&self, i: NoticeLiveIdx) -> Option<NoticeToken> {
        self.slots[i.index()].token()
    }

    /// The handle to the notification `token` holds; `None` once a newer
    /// one took its slot or it was let go of.
    pub fn get(&mut self, token: NoticeToken) -> Option<NoticeHeld<'_, P>> {
        let i = token.slot();
        (self.slots[i.index()].token() == Some(token)).then_some(NoticeHeld { slots: self, i })
    }

    /// The handle to the notification the slot `i` holds, when one does.
    pub fn at(&mut self, i: NoticeLiveIdx) -> Option<NoticeHeld<'_, P>> {
        let token = self.slots[i.index()].token()?;
        self.get(token)
    }

    /// The platform's id for `token`, once it answered.
    pub fn id(&self, token: NoticeToken) -> Option<&P> {
        match self.slots[token.slot().index()].shown() {
            Some((t, id, _)) if t == token => Some(id),
            _ => None,
        }
    }

    /// **The button numbered `number` that `token` showed**: `None` for a
    /// number naming none of its buttons, and for one not on the platform.
    pub fn button(&self, token: NoticeToken, number: u8) -> Option<NoticeButton> {
        match self.slots[token.slot().index()].shown() {
            Some((t, _, buttons)) if t == token => buttons.numbered(number),
            _ => None,
        }
    }

    /// **The nearest deadline**, for the driver's wait: kept as slots are
    /// entered and left, so reading it looks at no slot.
    pub fn next_due(&self) -> Option<HostTime> {
        self.earliest
    }

    /// **The ones whose deadline has passed by the clock `now` reads**, each
    /// now closing: alive until the platform says it closed. With no
    /// deadline held the clock is not read; with one held and none passed,
    /// one deadline is looked at.
    pub fn due(&mut self, now: impl FnOnce() -> HostTime) -> NoticeSlotSet {
        let Some(earliest) = self.earliest else {
            return NoticeSlotSet::EMPTY;
        };
        let now = now();
        self.looked();
        if earliest > now {
            return NoticeSlotSet::EMPTY;
        }
        let mut fired = NoticeSlotSet::EMPTY;
        for i in self.timed {
            self.looked();
            if self.slots[i.index()].due().is_some_and(|d| d <= now) {
                fired = fired.with(i);
            }
        }
        for i in fired {
            self.close(i);
        }
        fired
    }

    /// The slot `i`, its deadline spent, now closing.
    fn close(&mut self, i: NoticeLiveIdx) {
        let next = match self.put(i, NoticeSlot::Free) {
            NoticeSlot::Shown {
                token,
                id,
                buttons,
                update,
                ..
            } => NoticeSlot::Closing {
                token,
                id,
                buttons,
                update,
            },
            other => other,
        };
        self.put(i, next);
    }

    /// **The slot `i` holding `next` in place of what it held**, which is
    /// returned: the one place a slot changes, so the deadlines held and
    /// the nearest of them change with it. Leaving the nearest looks again
    /// at the slots still timed; nothing else looks at any.
    fn put(&mut self, i: NoticeLiveIdx, next: NoticeSlot<P>) -> NoticeSlot<P> {
        let due = next.due();
        let old = core::mem::replace(&mut self.slots[i.index()], next);
        if old.due().is_some() {
            self.timed = self.timed.without(i);
            if old.due() == self.earliest {
                let earliest = self
                    .timed
                    .filter_map(|j| {
                        self.looked();
                        self.slots[j.index()].due()
                    })
                    .min();
                self.earliest = earliest;
            }
        }
        if let Some(d) = due {
            self.timed = self.timed.with(i);
            self.earliest = Some(self.earliest.map_or(d, |e| e.min(d)));
        }
        old
    }

    #[cfg(test)]
    fn looked(&self) {
        self.looks.set(self.looks.get() + 1);
    }

    #[cfg(not(test))]
    fn looked(&self) {}
}

impl<P: PartialEq> NoticeSlots<P> {
    /// The token the platform's `id` names: a scan, run per answer the
    /// platform sends, never per turn.
    pub fn token(&self, id: &P) -> Option<NoticeToken> {
        self.slots
            .iter()
            .find_map(|s| s.shown().filter(|(_, i, _)| *i == id).map(|(t, _, _)| t))
    }

    /// **The pending one the platform's `id` names, scheduled at `now`**;
    /// `None` for an id none pending has.
    pub fn scheduled(&mut self, id: &P, now: HostTime) -> Option<NoticeToken> {
        self.slots.iter_mut().find_map(|s| match s {
            NoticeSlot::Shown {
                token,
                id: i,
                at,
                scheduling: scheduling @ Scheduling::Pending,
                ..
            } if *i == *id => {
                *scheduling = Scheduling::Scheduled;
                *at = now;
                Some(*token)
            }
            _ => None,
        })
    }

    /// **The ones the platform no longer has**: each scheduled at least
    /// `settled` before `asked`, the moment its list was asked for, and
    /// missing from `present`, the list. A post joins the list
    /// asynchronously, so one scheduled just before the ask may not be in
    /// it yet, and one still pending is in no list.
    pub fn absent(&self, present: &[P], asked: HostTime, settled: Duration) -> NoticeSlotSet {
        NoticeLiveIdx::ALL
            .into_iter()
            .filter(|i| match &self.slots[i.index()] {
                NoticeSlot::Shown {
                    id,
                    at,
                    scheduling: Scheduling::Scheduled,
                    ..
                } => at.plus(settled) <= asked && !present.contains(id),
                _ => false,
            })
            .fold(NoticeSlotSet::EMPTY, NoticeSlotSet::with)
    }
}

impl<P> Default for NoticeSlots<P> {
    fn default() -> Self {
        NoticeSlots::new()
    }
}

/// When a notification shown at `now` with `expiry` closes.
fn deadline(expiry: Expiry, now: HostTime) -> Option<HostTime> {
    match expiry {
        Expiry::After(d) => Some(now.plus(d.duration())),
        Expiry::System | Expiry::Never => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(serial: u32, slot: usize) -> NoticeToken {
        NoticeToken::of_parts(serial, slot)
    }

    fn at(ms: u64) -> HostTime {
        HostTime::ZERO.plus(Duration::from_millis(ms))
    }

    fn after(ms: u32) -> Expiry {
        Expiry::After(crate::NonZeroMillis::new(ms).expect("non-zero"))
    }

    /// The deadlines looked at after the last call.
    fn looks(s: &NoticeSlots<u32>) -> u32 {
        s.looks.replace(0)
    }

    /// **An idle turn compares no deadline**: with all 32 slots shown and
    /// none closing on its own, the wait's `next_due` and the turn's `due`
    /// look at no deadline and `due` reads no clock; with one far deadline
    /// among 31 without, `due` reads the clock once and looks at one
    /// deadline, `next_due` at none. Break: a scan over the slots, 32 looks
    /// a call.
    #[test]
    fn an_idle_turn_compares_no_deadline() {
        let mut s: NoticeSlots<u32> = NoticeSlots::new();
        for i in 0..NOTICE_LIVE {
            let t = token(1 + i as u32, i);
            s.posting(t, Expiry::System, NoticeButtonSet::NONE);
            assert_eq!(s.notified(t, i as u32, at(0)), Some(t));
        }
        looks(&s);
        let mut clock = 0;
        assert_eq!(s.next_due(), None);
        assert_eq!(
            s.due(|| {
                clock += 1;
                at(10)
            }),
            NoticeSlotSet::EMPTY
        );
        assert_eq!((looks(&s), clock), (0, 0), "idle: no look, no clock");

        let far = token(100, 5);
        s.posting(far, after(60_000), NoticeButtonSet::NONE);
        assert_eq!(s.notified(far, 5, at(0)), Some(far));
        looks(&s);
        assert_eq!(s.next_due(), Some(at(60_000)));
        assert_eq!(looks(&s), 0, "the wait reads the field");
        assert_eq!(
            s.due(|| {
                clock += 1;
                at(10)
            }),
            NoticeSlotSet::EMPTY
        );
        assert_eq!((looks(&s), clock), (1, 1), "one deadline held: one look");
        assert_eq!(s.id(far), Some(&5), "the control: it is held");
    }

    /// The nearest deadline over every slot, read slot by slot.
    fn scanned(s: &NoticeSlots<u32>) -> Option<HostTime> {
        s.slots.iter().filter_map(NoticeSlot::due).min()
    }

    /// **Posting is bounded**: 33 posts the platform never answers leave
    /// 32 being sent, the 33rd in the slot its token names, the one it took
    /// the slot from forgotten. Break: a list of posts awaiting an answer,
    /// which grows by one a post.
    #[test]
    fn posting_is_bounded() {
        let mut s: NoticeSlots<u32> = NoticeSlots::new();
        let tokens: [NoticeToken; NOTICE_LIVE + 1] =
            core::array::from_fn(|i| token(1 + i as u32, i % NOTICE_LIVE));
        for t in tokens {
            s.posting(t, Expiry::System, NoticeButtonSet::NONE);
        }
        assert_eq!(s.held().count(), NOTICE_LIVE);
        assert_eq!(
            s.token_at(tokens[NOTICE_LIVE].slot()),
            Some(tokens[NOTICE_LIVE])
        );
        assert!(s.get(tokens[0]).is_none(), "the one superseded");
        assert_eq!(s.notified(tokens[0], 1, at(0)), None, "its answer refused");
        assert_eq!(
            s.notified(tokens[NOTICE_LIVE], 2, at(0)),
            Some(tokens[NOTICE_LIVE])
        );
        assert_eq!(s.id(tokens[NOTICE_LIVE]), Some(&2));
    }

    /// **A deadline closes it, and it stays alive until the platform says
    /// so**: due once, then closing, its id and buttons still read; the
    /// nearest deadline moves to the next when the nearest leaves; one with
    /// no deadline is never due. A token whose slot a newer one took reads
    /// nothing.
    #[test]
    fn a_deadline_closes_and_the_next_one_is_nearest() {
        let mut s: NoticeSlots<u32> = NoticeSlots::new();
        let (a, b, c) = (token(1, 0), token(2, 1), token(3, 2));
        let two = NoticeButtonSet::of(NoticeButton::of_index(1));
        s.posting(a, after(1500), two);
        s.posting(b, after(3000), NoticeButtonSet::NONE);
        s.posting(c, Expiry::Never, NoticeButtonSet::NONE);
        assert_eq!(s.next_due(), None, "nothing shown yet");
        assert_eq!(s.id(a), None, "no id before the answer");
        assert_eq!(s.notified(a, 7, at(0)), Some(a));
        assert_eq!(s.notified(b, 8, at(0)), Some(b));
        assert_eq!(s.notified(c, 9, at(0)), Some(c));
        assert_eq!(s.next_due(), Some(at(1500)));
        assert!(s.due(|| at(1000)).is_empty());
        let fired: [Option<NoticeLiveIdx>; 2] = {
            let mut f = s.due(|| at(2000));
            [f.next(), f.next()]
        };
        assert_eq!(fired, [Some(a.slot()), None]);
        assert_eq!(s.next_due(), Some(at(3000)), "the next one is nearest");
        assert_eq!(s.id(a), Some(&7), "alive until the platform says");
        assert_eq!(s.token(&7), Some(a));
        assert_eq!(s.button(a, 2).map(NoticeButton::number), Some(2));
        assert_eq!(s.button(a, 1), None, "a button it did not show");
        assert!(s.due(|| at(2500)).is_empty(), "due once");
        s.get(b).expect("held").free();
        assert_eq!(s.next_due(), None, "c never closes on its own");
        assert!(s.due(|| at(u64::MAX >> 24)).is_empty());
        let newer = token(4, 2);
        s.posting(newer, Expiry::System, NoticeButtonSet::NONE);
        assert_eq!((s.id(c), s.token(&9)), (None, None), "c forgotten");
        assert!(s.get(c).is_none());
    }

    /// **An update sent again before the first answer came** is shown as
    /// the later id, with the expiry and buttons the later post asked.
    #[test]
    fn an_update_answered_twice_is_the_later_id() {
        let mut s: NoticeSlots<u32> = NoticeSlots::new();
        let t = token(1, 3);
        s.posting(t, after(1000), NoticeButtonSet::NONE);
        s.posting(
            t,
            after(5000),
            NoticeButtonSet::of(NoticeButton::of_index(0)),
        );
        assert_eq!(s.notified(t, 10, at(0)), Some(t));
        assert_eq!(s.notified(t, 11, at(100)), Some(t));
        assert_eq!((s.id(t), s.token(&10)), (Some(&11), None));
        assert_eq!(s.next_due(), Some(at(5000)));
        assert_eq!(s.button(t, 1).map(NoticeButton::number), Some(1));
    }

    /// **An update keeps the one shown until the platform answers it**: its
    /// id, its buttons and its deadline are read while the update is sent,
    /// so a click or a close on it is heard and its deadline still closes
    /// it; the answer shows the update's id, buttons and deadline. Break: a
    /// post that empties the slot before the answer, the id read `None`.
    #[test]
    fn an_update_keeps_the_one_shown_until_answered() {
        let mut s: NoticeSlots<u32> = NoticeSlots::new();
        let t = token(1, 4);
        let two = NoticeButtonSet::of(NoticeButton::of_index(1));
        s.posting(t, after(1000), two);
        assert_eq!(s.notified(t, 7, at(0)), Some(t));
        s.posting(t, after(5000), NoticeButtonSet::NONE);
        assert_eq!(s.id(t), Some(&7), "the one shown, while the update is sent");
        assert_eq!(s.token(&7), Some(t), "a click or a close on it is heard");
        assert_eq!(s.button(t, 2).map(NoticeButton::number), Some(2));
        assert_eq!(s.next_due(), Some(at(1000)), "its deadline kept");
        let fired: [Option<NoticeLiveIdx>; 2] = {
            let mut f = s.due(|| at(1500));
            [f.next(), f.next()]
        };
        assert_eq!(fired, [Some(t.slot()), None], "and it still closes it");
        assert_eq!(s.notified(t, 8, at(2000)), Some(t));
        assert_eq!((s.id(t), s.token(&7)), (Some(&8), None));
        assert_eq!(s.next_due(), Some(at(7000)), "the update's deadline");
        assert_eq!(s.button(t, 2), None, "the update's buttons");
    }

    /// **A refused update leaves the one shown as it was**: its id, its
    /// buttons and its deadline, and a later update replaces it; a refused
    /// first post lets its slot go. Break: a refusal that frees the slot in
    /// any state, the one on screen forgotten.
    #[test]
    fn a_refused_update_leaves_the_one_shown() {
        let mut s: NoticeSlots<u32> = NoticeSlots::new();
        let (t, first) = (token(1, 4), token(2, 5));
        let two = NoticeButtonSet::of(NoticeButton::of_index(1));
        s.posting(t, after(1000), two);
        assert_eq!(s.notified(t, 7, at(0)), Some(t));
        s.posting(t, after(5000), NoticeButtonSet::NONE);
        s.refused(t);
        assert_eq!(s.id(t), Some(&7), "still on screen");
        assert_eq!(s.token(&7), Some(t));
        assert_eq!(s.button(t, 2).map(NoticeButton::number), Some(2));
        assert_eq!(s.next_due(), Some(at(1000)), "its deadline kept");
        s.posting(t, after(3000), NoticeButtonSet::NONE);
        assert_eq!(s.notified(t, 8, at(100)), Some(t), "a later update");
        assert_eq!((s.id(t), s.next_due()), (Some(&8), Some(at(3100))));
        s.posting(first, Expiry::System, NoticeButtonSet::NONE);
        s.refused(first);
        assert_eq!(s.token_at(first.slot()), None, "a first post refused");
        assert_eq!(s.notified(first, 9, at(0)), None);
    }

    /// **The platform's list**: a pending post is in no list; one scheduled
    /// less than the settling time before the ask may not be in it yet;
    /// one scheduled before that and missing is gone.
    #[test]
    fn absent_reads_only_the_settled_scheduled_ones() {
        let settled = Duration::from_secs(1);
        let mut s: NoticeSlots<u32> = NoticeSlots::new();
        let (a, b, c) = (token(1, 0), token(2, 1), token(3, 2));
        for (t, id) in [(a, 1), (b, 2), (c, 3)] {
            s.pending(t, id, Expiry::System, at(0), NoticeButtonSet::NONE);
        }
        assert_eq!(s.scheduled(&1, at(0)), Some(a));
        assert_eq!(s.scheduled(&1, at(0)), None, "scheduled once");
        assert_eq!(s.scheduled(&2, at(4000)), Some(b));
        let gone: [Option<NoticeLiveIdx>; 2] = {
            let mut g = s.absent(&[], at(4500), settled);
            [g.next(), g.next()]
        };
        assert_eq!(gone, [Some(a.slot()), None], "b too new, c pending");
        assert!(s.absent(&[1], at(4500), settled).is_empty(), "a listed");
        let mut later = s.absent(&[], at(9000), settled);
        assert_eq!(
            (later.next(), later.next()),
            (Some(a.slot()), Some(b.slot()))
        );
        let held = s.at(b.slot()).expect("b held");
        assert_eq!(held.token(), b);
        held.free();
        assert_eq!(s.token_at(b.slot()), None);
    }

    /// **The nearest deadline is the nearest any slot holds**, after every
    /// step of a long run of posts, answers, pending posts, updates,
    /// refusals, frees and turns at a moving clock. Break: a slot left without
    /// putting its deadline back (a free that keeps the nearest).
    #[test]
    fn the_nearest_deadline_is_kept_through_every_change() {
        let mut s: NoticeSlots<u32> = NoticeSlots::new();
        let mut serial = [1u32; NOTICE_LIVE];
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut now = 0u64;
        let mut fired = 0;
        for step in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let slot = (x >> 8) as usize % NOTICE_LIVE;
            let t = token(serial[slot], slot);
            let expiry = match (x >> 16) % 4 {
                0 => Expiry::System,
                1 => Expiry::Never,
                k => after(1 + ((x >> 20) % (500 * k)) as u32),
            };
            match (x >> 32) % 8 {
                0 => {
                    serial[slot] += 1;
                    s.posting(token(serial[slot], slot), expiry, NoticeButtonSet::NONE);
                }
                1 => s.posting(t, expiry, NoticeButtonSet::NONE),
                2 | 3 => {
                    let _ = s.notified(t, step, at(now));
                }
                4 => s.pending(t, step, expiry, at(now), NoticeButtonSet::NONE),
                5 => {
                    if let Some(h) = s.get(t) {
                        h.free();
                    }
                }
                6 => s.refused(t),
                _ => {
                    now += (x >> 40) % 300;
                    let due = s.due(|| at(now));
                    fired += due.count();
                    assert!(s.next_due().is_none_or(|d| d > at(now)), "step {step}");
                }
            }
            assert_eq!(s.next_due(), scanned(&s), "step {step}");
            let timed = NoticeLiveIdx::ALL
                .into_iter()
                .filter(|i| s.slots[i.index()].due().is_some())
                .fold(NoticeSlotSet::EMPTY, NoticeSlotSet::with);
            assert_eq!(s.timed, timed, "step {step}");
        }
        assert!(fired > 1000, "the premise: deadlines fired ({fired})");
    }
}
