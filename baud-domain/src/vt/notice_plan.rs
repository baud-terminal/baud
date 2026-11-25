//! **What a window decides about each notification it delivers**, before
//! any platform call: whether it is shown ([`notice_plan`]) -- the setting,
//! its occasion against what the platform sees of the window
//! ([`NoticeSight`]), and the flood bound ([`NoticeBucket`]) -- what the
//! core is told the platform delivers ([`notice_support`]), what a
//! freedesktop notification server said it supports ([`ServerCaps`]), and
//! the counts the log tells of once ([`NoticeSaidOnce`]). What the driver
//! does about it comes back as [`NoticeEffect`]s, for the driver to carry
//! out.

use core::time::Duration;

use crate::{
    Activation, ButtonSupport, CloseEvents, ExpirySupport, HostTime, IconSupport, NoticeAction,
    NoticeSupport, NoticeToken, Occasion, Occasions, Sounds, Urgencies,
};

/// `[screen] notifications`: whether a program's notifications are
/// delivered at all. Off, the core is told none is ([`notice_support`]),
/// so it refuses every one and leaves a program's query unanswered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeSetting {
    On,
    Off,
}

/// **The window as a platform sees it**, which a notification's occasion
/// is read against: the `o` key, "always is the default and always honors
/// the request", `unfocused` once the window is not focused, `invisible`
/// once it is not seen (`[[source: kitty-desktop-notifications]]`).
pub trait NoticeSight: Copy {
    /// Whether a notification sent for `occasion` shows now.
    fn shows(self, occasion: Occasion) -> bool;
}

/// **The window as Linux sees it**: focused or not. Nothing on Linux says
/// whether a window is seen, so `invisible` is read as `unfocused`, the
/// text's "best guess" for a value the terminal does not claim.
///
/// ```
/// let sight = baud_domain::LinuxSight::Unfocused;
/// assert!(baud_domain::NoticeSight::shows(sight, baud_domain::Occasion::Invisible));
/// ```
///
/// There is no covered window on Linux:
///
/// ```compile_fail,E0599
/// let sight = baud_domain::LinuxSight::Covered;
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinuxSight {
    Focused,
    Unfocused,
}

impl NoticeSight for LinuxSight {
    fn shows(self, occasion: Occasion) -> bool {
        match self {
            LinuxSight::Focused => occasion == Occasion::Always,
            LinuxSight::Unfocused => true,
        }
    }
}

/// **The window as macOS sees it**: focused; or not focused and seen, at
/// least in part; or not focused and covered, as its occlusion state
/// says. A focused window is seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacSight {
    Focused,
    Seen,
    Covered,
}

impl NoticeSight for MacSight {
    fn shows(self, occasion: Occasion) -> bool {
        match (self, occasion) {
            (_, Occasion::Always) => true,
            (MacSight::Focused, _) => false,
            (MacSight::Seen, Occasion::Unfocused) => true,
            (MacSight::Seen, Occasion::Invisible) => false,
            (MacSight::Covered, _) => true,
        }
    }
}

/// What to do with one notification.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticePlan {
    Show,
    Close,
    Drop(NoticeWhy),
}

/// Why one was not shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeWhy {
    /// Its occasion: the window is focused, or seen.
    Occasion,
    /// The flood bound.
    Rate,
    /// The setting is off.
    Off,
}

/// **Whether to show a notification**: a close is always carried out;
/// then the setting, then its occasion against `sight`, and then the
/// bound, which only a notification about to be shown spends.
pub fn notice_plan<S: NoticeSight>(
    action: NoticeAction,
    occasion: Occasion,
    setting: NoticeSetting,
    sight: S,
    bucket: &mut NoticeBucket,
    now: HostTime,
) -> NoticePlan {
    if action == NoticeAction::Close {
        return NoticePlan::Close;
    }
    if setting == NoticeSetting::Off {
        return NoticePlan::Drop(NoticeWhy::Off);
    }
    if !sight.shows(occasion) {
        return NoticePlan::Drop(NoticeWhy::Occasion);
    }
    match bucket.take(now) {
        NoticeTaken::Yes => NoticePlan::Show,
        NoticeTaken::No => NoticePlan::Drop(NoticeWhy::Rate),
    }
}

/// Whether the bound let one through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeTaken {
    Yes,
    No,
}

/// The burst the bound allows, and how fast it refills.
pub const NOTICE_BURST: Duration = Duration::from_secs(4);
pub const NOTICE_EACH: Duration = Duration::from_secs(1);

/// A count of notifications, or of the images in them, kept for the log.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct NoticeCount(u64);

impl NoticeCount {
    pub const ZERO: NoticeCount = NoticeCount(0);

    pub const fn get(self) -> u64 {
        self.0
    }

    fn bump(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
}

/// A count of bursts: a flood that never lets the bucket fill is one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NoticeBursts(u64);

impl NoticeBursts {
    pub const ZERO: NoticeBursts = NoticeBursts(0);

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// **The flood bound**: credit in time, at most [`NOTICE_BURST`], each
/// notification shown spending [`NOTICE_EACH`], credit coming back as the
/// clock runs; and what it dropped, which it says once a burst
/// ([`NoticeBucket::newly_dropped`]).
///
/// ```
/// let mut b = baud_domain::NoticeBucket::new();
/// assert_eq!(b.take(baud_domain::HostTime::ZERO), baud_domain::NoticeTaken::Yes);
/// assert_eq!(b.dropped(), baud_domain::NoticeCount::ZERO);
/// ```
///
/// Its credit is only its own:
///
/// ```compile_fail,E0451
/// let b = baud_domain::NoticeBucket {
///     credit: core::time::Duration::ZERO,
///     at: None,
///     dropped: baud_domain::NoticeCount::ZERO,
///     bursts: baud_domain::NoticeBursts::ZERO,
///     said_dropped: baud_domain::NoticeCount::ZERO,
///     said_bursts: baud_domain::NoticeBursts::ZERO,
/// };
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticeBucket {
    credit: Duration,
    at: Option<HostTime>,
    dropped: NoticeCount,
    /// One begins at a take that finds the bucket full.
    bursts: NoticeBursts,
    /// What it had dropped when it last said so, and in which burst.
    said_dropped: NoticeCount,
    said_bursts: NoticeBursts,
}

impl NoticeBucket {
    pub const fn new() -> NoticeBucket {
        NoticeBucket {
            credit: NOTICE_BURST,
            at: None,
            dropped: NoticeCount::ZERO,
            bursts: NoticeBursts::ZERO,
            said_dropped: NoticeCount::ZERO,
            said_bursts: NoticeBursts::ZERO,
        }
    }

    /// One notification shown at `now`, if the credit allows; else
    /// dropped and counted.
    pub fn take(&mut self, now: HostTime) -> NoticeTaken {
        if let Some(at) = self.at {
            self.credit = (self.credit + now.since(at).unwrap_or(Duration::ZERO)).min(NOTICE_BURST);
        }
        self.at = Some(now);
        if self.credit == NOTICE_BURST {
            self.bursts.0 = self.bursts.0.saturating_add(1);
        }
        match self.credit >= NOTICE_EACH {
            true => {
                self.credit -= NOTICE_EACH;
                NoticeTaken::Yes
            }
            false => {
                self.dropped.bump();
                NoticeTaken::No
            }
        }
    }

    /// How many it has dropped.
    pub const fn dropped(&self) -> NoticeCount {
        self.dropped
    }

    /// How many bursts it has seen.
    pub const fn bursts(&self) -> NoticeBursts {
        self.bursts
    }

    /// **The drops it has not said yet**, when there are some and the
    /// burst is one it has not spoken in: once a burst, not once a
    /// notification, and counting every drop after the last saying.
    pub fn newly_dropped(&mut self) -> Option<NoticeCount> {
        let (d, b) = (self.dropped, self.bursts);
        (d != self.said_dropped && b != self.said_bursts).then(|| {
            let n = NoticeCount(d.0 - self.said_dropped.0);
            (self.said_dropped, self.said_bursts) = (d, b);
            n
        })
    }
}

impl Default for NoticeBucket {
    fn default() -> Self {
        NoticeBucket::new()
    }
}

/// Whether a count said once has been.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Said {
    Not,
    Once,
}

/// **A count the log tells of once**: the first time it is past zero,
/// and never again however it grows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticeSaidOnce {
    count: NoticeCount,
    said: Said,
}

impl NoticeSaidOnce {
    pub const NEW: NoticeSaidOnce = NoticeSaidOnce {
        count: NoticeCount::ZERO,
        said: Said::Not,
    };

    /// One more.
    pub fn add(&mut self) {
        self.count.bump();
    }

    pub const fn count(&self) -> NoticeCount {
        self.count
    }

    /// The count, the first time it is past zero; after that, nothing.
    pub fn say(&mut self) -> Option<NoticeCount> {
        (self.count != NoticeCount::ZERO && self.said == Said::Not).then(|| {
            self.said = Said::Once;
            self.count
        })
    }
}

impl Default for NoticeSaidOnce {
    fn default() -> Self {
        NoticeSaidOnce::NEW
    }
}

/// **A line for the log**, its wording the driver's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeLine {
    /// The bound dropped this many after it last said so
    /// ([`NoticeBucket::newly_dropped`]).
    Rate(NoticeCount),
    /// This many were not posted, nothing in them for the platform to
    /// show; said once.
    Bare(NoticeCount),
    /// This many images were not shown; said once.
    ImagesUnshown(NoticeCount),
}

/// **What a driver does about the notifications**, in the order it does
/// it.
#[must_use]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeEffect {
    /// The core forgets it, not shown or gone where nothing will say it
    /// closed (`Term::notice_dropped`).
    Dropped(NoticeToken),
    /// A line for the log.
    Line(NoticeLine),
}

/// **One of the capabilities a freedesktop notification server lists**
/// in `GetCapabilities`' answer: the ten the specification defines
/// (`[[source: freedesktop-notification-spec]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdoCap {
    ActionIcons,
    Actions,
    Body,
    BodyHyperlinks,
    BodyImages,
    BodyMarkup,
    IconMulti,
    IconStatic,
    Persistence,
    Sound,
}

/// Each capability and the name a server lists it by: the one place a
/// capability is spelled.
const FDO_CAPS: [(FdoCap, &str); 10] = [
    (FdoCap::ActionIcons, "action-icons"),
    (FdoCap::Actions, "actions"),
    (FdoCap::Body, "body"),
    (FdoCap::BodyHyperlinks, "body-hyperlinks"),
    (FdoCap::BodyImages, "body-images"),
    (FdoCap::BodyMarkup, "body-markup"),
    (FdoCap::IconMulti, "icon-multi"),
    (FdoCap::IconStatic, "icon-static"),
    (FdoCap::Persistence, "persistence"),
    (FdoCap::Sound, "sound"),
];

const _: () = assert!(FDO_CAPS.len() <= u16::BITS as usize);
const _: () = {
    let mut i = 0;
    while i < FDO_CAPS.len() {
        assert!(
            FDO_CAPS[i].0 as usize == i,
            "the table is in the enum's order"
        );
        i += 1;
    }
};

impl FdoCap {
    /// Every capability, in the table's order.
    pub const ALL: [FdoCap; 10] = {
        let mut all = [FdoCap::ActionIcons; 10];
        let mut i = 0;
        while i < FDO_CAPS.len() {
            all[i] = FDO_CAPS[i].0;
            i += 1;
        }
        all
    };

    /// The name a server lists it by.
    pub const fn name(self) -> &'static str {
        FDO_CAPS[self as usize].1
    }

    fn bit(self) -> u16 {
        1 << self as u16
    }
}

/// **What a freedesktop notification server said it supports**, read
/// from its answer and from nothing else ([`ServerCaps::heard`]).
///
/// ```
/// let caps = baud_domain::ServerCaps::heard(["actions", "x-vendor-foo-cap"]);
/// assert!(caps.lists(baud_domain::FdoCap::Actions));
/// assert!(caps.markup().is_none());
/// ```
///
/// Outside this crate no set is written as a mask:
///
/// ```compile_fail,E0423
/// let caps = baud_domain::ServerCaps(3);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ServerCaps(u16);

impl ServerCaps {
    /// **The capabilities `names` lists**, a server's answer: a name the
    /// specification does not define -- a vendor's own, which "start with
    /// x-vendor", or any other -- is passed over.
    pub fn heard<'a>(names: impl IntoIterator<Item = &'a str>) -> ServerCaps {
        ServerCaps(names.into_iter().fold(0, |m, n| {
            FDO_CAPS
                .iter()
                .find(|(_, name)| *name == n)
                .map_or(m, |(c, _)| m | c.bit())
        }))
    }

    /// Whether the server listed `cap`.
    pub fn lists(self, cap: FdoCap) -> bool {
        self.0 & cap.bit() != 0
    }

    /// **The proof that the server reads markup in a body**, where it
    /// listed `body-markup`: "If marked up text is sent to a server that
    /// does not give this cap, the markup will show through as regular
    /// text".
    pub fn markup(self) -> Option<MarkupListed> {
        self.lists(FdoCap::BodyMarkup).then_some(MarkupListed(()))
    }
}

/// **That a server listed `body-markup`**, made only by
/// [`ServerCaps::markup`].
///
/// ```
/// let caps = baud_domain::ServerCaps::heard(["body-markup"]);
/// let proof: Option<baud_domain::MarkupListed> = caps.markup();
/// assert!(proof.is_some());
/// ```
///
/// No proof is made anywhere else:
///
/// ```compile_fail,E0423
/// let proof = baud_domain::MarkupListed(());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarkupListed(());

/// **Whether the notification server has answered `GetCapabilities`**,
/// and with what: one that listed nothing is told apart from one not yet
/// asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdoCaps {
    Unasked,
    Answered(ServerCaps),
}

impl FdoCaps {
    /// What the server is taken to have: nothing before it answers.
    pub fn known(self) -> ServerCaps {
        match self {
            FdoCaps::Unasked => ServerCaps(0),
            FdoCaps::Answered(caps) => caps,
        }
    }
}

/// What a platform delivers, as the core answers a program's query.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticePlatform {
    /// macOS: a click focuses the window and can be reported; closes are
    /// not told; the occlusion state says what is seen; two urgencies
    /// (`critical` needs an entitlement baud does not have); the default
    /// sound or none; images as attachments.
    MacOs,
    /// Linux, with what the notification server said it supports.
    Linux(ServerCaps),
}

/// **What the core is told**: `None` while the setting is off, else what
/// the platform delivers. On Linux a click on a notification that asked
/// `focus` activates the window with the token the server's
/// `ActivationToken` gives, where it sends one -- "Clients should not
/// assume the server will generate this signal"
/// (`[[source: freedesktop-notification-spec]]`) -- and the compositor
/// decides what that does (`[[source: xdg-activation-v1]]`). Neither can
/// be known when a program asks, so only `report` is claimed, and it and
/// the buttons only where the server lists `actions`.
pub fn notice_support(setting: NoticeSetting, platform: NoticePlatform) -> Option<NoticeSupport> {
    if setting == NoticeSetting::Off {
        return None;
    }
    Some(match platform {
        NoticePlatform::MacOs => NoticeSupport {
            actions: Activation::FOCUS_AND_REPORT,
            close_events: CloseEvents::Untracked,
            occasions: Occasions::UnfocusedAndInvisible,
            urgencies: Urgencies::LowAndNormal,
            sounds: Sounds::SystemAndSilent,
            expiry: ExpirySupport::Expires,
            icons: IconSupport::NamesAndImages,
            buttons: ButtonSupport::Shown,
        },
        NoticePlatform::Linux(caps) => {
            // Buttons are `Notify`'s actions too, shown only by a server
            // that lists `actions`.
            let actions = caps.lists(FdoCap::Actions);
            NoticeSupport {
                actions: match actions {
                    true => Activation::REPORT,
                    false => Activation::NONE,
                },
                close_events: CloseEvents::Tracked,
                occasions: Occasions::Unfocused,
                urgencies: Urgencies::All,
                sounds: Sounds::SystemAndSilent,
                expiry: ExpirySupport::Expires,
                icons: IconSupport::NamesAndImages,
                buttons: match actions {
                    true => ButtonSupport::Shown,
                    false => ButtonSupport::NotShown,
                },
            }
        }
    })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::time::Duration;
    use std::vec::Vec;

    use crate::{
        ButtonSupport, CloseEvents, FdoCap, FdoCaps, HostTime, LinuxSight, MacSight, NoticeAction,
        NoticeBucket, NoticeCount, NoticeEffect, NoticeLine, NoticePlan, NoticePlatform,
        NoticeSaidOnce, NoticeSetting, NoticeSight, NoticeTaken, NoticeWhy, Occasion, Occasions,
        ServerCaps, notice_plan, notice_support,
    };

    fn at(s: u64) -> HostTime {
        HostTime::ZERO.plus(Duration::from_secs(s))
    }

    fn go(o: Occasion, sight: impl NoticeSight) -> NoticePlan {
        notice_plan(
            NoticeAction::Show,
            o,
            NoticeSetting::On,
            sight,
            &mut NoticeBucket::new(),
            at(0),
        )
    }

    /// **Each platform's sight against each occasion**: `always` shows
    /// whatever the window, a focused window shows nothing else,
    /// `unfocused` shows once the window is not focused, and `invisible`
    /// once it is not seen. Linux cannot tell what is seen, so `invisible`
    /// reads there as `unfocused`.
    #[test]
    fn the_occasion_decides_against_each_platforms_sight() {
        use NoticePlan::{Drop, Show};
        let skip = Drop(NoticeWhy::Occasion);
        let mac = [
            (Occasion::Always, [Show, Show, Show]),
            (Occasion::Unfocused, [skip, Show, Show]),
            (Occasion::Invisible, [skip, skip, Show]),
        ];
        for (o, want) in mac {
            let got = [MacSight::Focused, MacSight::Seen, MacSight::Covered].map(|s| go(o, s));
            assert_eq!(got, want, "macOS, {o:?}");
        }
        let linux = [
            (Occasion::Always, [Show, Show]),
            (Occasion::Unfocused, [skip, Show]),
            (Occasion::Invisible, [skip, Show]),
        ];
        for (o, want) in linux {
            let got = [LinuxSight::Focused, LinuxSight::Unfocused].map(|s| go(o, s));
            assert_eq!(got, want, "Linux, {o:?}");
        }
    }

    /// **A close is always carried out; off drops the rest, before the
    /// occasion and before the bound**, which an off notice does not spend.
    #[test]
    fn a_close_is_carried_out_and_off_drops_the_rest() {
        let mut b = NoticeBucket::new();
        let plan = |a, setting, b: &mut NoticeBucket| {
            notice_plan(a, Occasion::Always, setting, MacSight::Covered, b, at(0))
        };
        assert_eq!(
            plan(NoticeAction::Close, NoticeSetting::Off, &mut b),
            NoticePlan::Close
        );
        for _ in 0..10 {
            assert_eq!(
                plan(NoticeAction::Show, NoticeSetting::Off, &mut b),
                NoticePlan::Drop(NoticeWhy::Off)
            );
        }
        assert_eq!(b.dropped(), NoticeCount::ZERO, "off spends nothing");
        let shown = (0..5)
            .map(|_| plan(NoticeAction::Replace, NoticeSetting::On, &mut b))
            .collect::<Vec<_>>();
        assert_eq!(
            shown,
            [
                NoticePlan::Show,
                NoticePlan::Show,
                NoticePlan::Show,
                NoticePlan::Show,
                NoticePlan::Drop(NoticeWhy::Rate)
            ]
        );
    }

    /// Four at once, then one a second; a quiet spell refills it, no further
    /// than four.
    #[test]
    fn the_bound_lets_four_through_then_one_a_second() {
        let mut b = NoticeBucket::new();
        for _ in 0..4 {
            assert_eq!(b.take(at(0)), NoticeTaken::Yes);
        }
        assert_eq!(b.take(at(0)), NoticeTaken::No);
        assert_eq!(b.take(at(1)), NoticeTaken::Yes);
        assert_eq!(b.take(at(1)), NoticeTaken::No);
        let ok = (0..10)
            .filter(|_| b.take(at(100)) == NoticeTaken::Yes)
            .count();
        assert_eq!(ok, 4);
        assert_eq!(b.dropped().get(), 8);
    }

    /// **The bound says what it dropped once a burst**, however often it
    /// is asked: a flood that keeps going is one burst, and the next
    /// saying waits for the bucket to fill again, then counts every drop
    /// after the last.
    #[test]
    fn the_bound_says_once_a_burst() {
        let mut b = NoticeBucket::new();
        assert_eq!(b.newly_dropped(), None, "nothing dropped");
        let mut flood = |n, s| {
            let mut said = Vec::new();
            for _ in 0..n {
                if b.take(at(s)) == NoticeTaken::No {
                    said.extend(b.newly_dropped().map(NoticeCount::get));
                }
            }
            said.extend(b.newly_dropped().map(NoticeCount::get));
            said
        };
        assert_eq!(flood(6, 0), [1], "said at the first drop");
        assert_eq!(flood(3, 1), [0u64; 0], "the same burst");
        assert_eq!(flood(3, 2), [0u64; 0], "the same burst");
        assert_eq!(flood(5, 100), [6], "a new burst, every drop since");
    }

    /// **A count said once**: nothing while it is zero, the count the first
    /// time it is not, and never again however it grows.
    #[test]
    fn a_count_is_said_once() {
        let mut c = NoticeSaidOnce::NEW;
        assert_eq!(c.say(), None);
        c.add();
        c.add();
        assert_eq!(c.count().get(), 2);
        assert_eq!(c.say().map(NoticeCount::get), Some(2));
        c.add();
        assert_eq!(c.say(), None, "said once");
        assert_eq!(c.count().get(), 3);
        let e = NoticeEffect::Line(NoticeLine::Bare(c.count()));
        assert_ne!(e, NoticeEffect::Line(NoticeLine::ImagesUnshown(c.count())));
    }

    /// The query's claims follow the platform and the setting: macOS
    /// focuses and reports a click and cannot say when one closed; Linux
    /// says when one closed, claims a click and the buttons only where the
    /// server lists `actions`, and never focus.
    #[test]
    fn support_claims_what_the_platform_delivers() {
        let none = ServerCaps::heard([]);
        let listed = ServerCaps::heard(["actions"]);
        assert_eq!(
            notice_support(NoticeSetting::Off, NoticePlatform::MacOs),
            None
        );
        assert_eq!(
            notice_support(NoticeSetting::Off, NoticePlatform::Linux(listed)),
            None
        );
        let mac = notice_support(NoticeSetting::On, NoticePlatform::MacOs).expect("on");
        assert!(mac.actions.focuses() && mac.actions.reports());
        assert_eq!(mac.close_events, CloseEvents::Untracked);
        assert_eq!(mac.occasions, Occasions::UnfocusedAndInvisible);
        let linux = notice_support(NoticeSetting::On, NoticePlatform::Linux(none)).expect("on");
        assert!(!linux.actions.focuses() && !linux.actions.reports());
        assert_eq!(linux.close_events, CloseEvents::Tracked);
        assert_eq!(linux.occasions, Occasions::Unfocused);
        assert_eq!(linux.buttons, ButtonSupport::NotShown);
        let clicks = notice_support(NoticeSetting::On, NoticePlatform::Linux(listed)).expect("on");
        assert!(clicks.actions.reports() && !clicks.actions.focuses());
        assert_eq!(clicks.buttons, ButtonSupport::Shown);
    }

    /// **A server's capabilities are the ten the specification names**,
    /// read from its answer: a vendor's own and an unknown name are passed
    /// over, and a server that answered with none is told apart from one
    /// not yet asked.
    #[test]
    fn a_servers_capabilities_are_read_from_its_answer() {
        let names = [
            "action-icons",
            "actions",
            "body",
            "body-hyperlinks",
            "body-images",
            "body-markup",
            "icon-multi",
            "icon-static",
            "persistence",
            "sound",
        ];
        assert_eq!(FdoCap::ALL.map(FdoCap::name), names);
        for (c, n) in FdoCap::ALL.into_iter().zip(names) {
            let one = ServerCaps::heard([n, "x-vendor-foo-cap", "Actions", ""]);
            for other in FdoCap::ALL {
                assert_eq!(one.lists(other), other == c, "{n} lists {other:?}");
            }
            assert_eq!(one.markup().is_some(), c == FdoCap::BodyMarkup, "{n}");
        }
        let all = ServerCaps::heard(names);
        assert!(FdoCap::ALL.into_iter().all(|c| all.lists(c)));
        let empty = ServerCaps::heard([]);
        assert!(FdoCap::ALL.into_iter().all(|c| !empty.lists(c)));
        assert_ne!(FdoCaps::Answered(empty), FdoCaps::Unasked);
        assert_eq!(FdoCaps::Unasked.known(), empty, "nothing before it answers");
        assert_eq!(FdoCaps::Answered(all).known(), all);
    }
}
