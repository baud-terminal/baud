//! **Notifications and progress from programs, delivered**: what both
//! drivers decide about each notification the core published
//! ([`Term::notices`]), before the platform's own calls -- the
//! session bus's `org.freedesktop.Notifications` on Linux (`bus.rs`),
//! `UNUserNotificationCenter` on macOS (`baud-present-macos`'s `notify`).
//!
//! * **The setting**, `[screen] notifications = "on" | "off"`: off tells
//!   the core nothing is delivered ([`support`]), so it refuses every
//!   notification and leaves kitty's query unanswered, as a terminal
//!   without them.
//! * **When**: kitty's `o` says ("always is the default and always honors
//!   the request"); `OSC 9` and `OSC 777`, whose texts say nothing, show
//!   while the window is not focused. [`plan`].
//! * **How often**: a bucket of four, refilled one a second
//!   ([`Bucket`]), so a loop printing notifications posts four and then
//!   one a second, the rest dropped, counted and logged once a burst
//!   ([`Delivery::rate_line`]).
//! * **What text**: lossy UTF-8 with every control taken out ([`text`]):
//!   kitty's "Escape code safe UTF-8" binds the sender, and a desktop
//!   notification has no use for a control either way.
//! * **Icons**: the images programs send under `g` are kept, a bounded
//!   cache that forgets the least recently used ([`Icons`]), as kitty's
//!   text allows: "implementations can impose a reasonable max size on
//!   the icon cache and evict icons in order of last used". Linux shows
//!   only a PNG of at most 1024 pixels a side; the rest are counted
//!   ([`Delivery::image_data`]).
//!
//! Note AX (`docs/research/notes/ax-program-notifications.md`) has the
//! texts and the forks.

use std::collections::VecDeque;
use std::time::Duration;

use baud_core::{
    Activation, Attention, ButtonSupport, CloseEvents, CloseReport, Expiry, ExpirySupport,
    IconSupport, Notice, NoticeAction, NoticeFrom, NoticeSupport, NoticeToken, Occasion, Occasions,
    Progress, ReportCount, Sounds, Term, Urgencies, Urgency,
};

use crate::bell::Moment;

/// `[screen] notifications`.
pub use crate::config::Notifications as Setting;

/// A notification taken from the core, owned, its text made safe.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Posted {
    pub token: NoticeToken,
    pub from: NoticeFrom,
    pub action: NoticeAction,
    pub title: String,
    pub body: String,
    pub buttons: Vec<String>,
    /// `f` and the first `t`.
    pub app: String,
    pub kind: String,
    pub icon_names: Vec<String>,
    /// The image to show: sent with it, or cached under its `g`.
    pub icon: Option<Vec<u8>>,
    pub sound: Sound,
    pub occasion: Occasion,
    pub urgency: Urgency,
    pub expiry: Expiry,
    pub activation: Activation,
    pub close: CloseReport,
}

/// `s`: the two every terminal plays, and the rest by name.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Sound {
    /// "The default system sound for a notification".
    System,
    /// "No sound must accompany the notification".
    Silent,
    /// "error", "warn", "info", "question" or a platform's own name.
    Named(String),
}

impl Sound {
    fn of(name: &str) -> Sound {
        match name {
            "" | "system" => Sound::System,
            "silent" => Sound::Silent,
            other => Sound::Named(other.to_string()),
        }
    }
}

impl Posted {
    /// One notice taken, its icon resolved against the cache: an image
    /// sent with it is cached under its `g` and used; a `g` alone uses the
    /// image cached under it.
    pub fn of(n: &Notice<'_>, icons: &mut Icons) -> Posted {
        let icon = match (n.icon.is_empty(), n.icon_key.is_empty()) {
            (false, false) => {
                icons.keep(n.icon_key, n.icon);
                Some(n.icon.to_vec())
            }
            (false, true) => Some(n.icon.to_vec()),
            (true, false) => icons.get(n.icon_key).map(<[u8]>::to_vec),
            (true, true) => None,
        };
        Posted {
            token: n.token,
            from: n.from,
            action: n.action,
            title: text(n.title),
            body: text(n.body),
            buttons: n.buttons().map(text).filter(|b| !b.is_empty()).collect(),
            app: text(n.app),
            kind: text(n.kind),
            icon_names: n.icon_names().map(text).collect(),
            icon,
            sound: Sound::of(&text(n.sound)),
            occasion: n.occasion,
            urgency: n.urgency,
            expiry: n.expiry,
            activation: n.activation,
            close: n.close,
        }
    }
}

/// Bytes a program sent as text: lossy UTF-8, every C0 and C1 control and
/// `DEL` taken out.
pub fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

/// The window, as a notification's occasion reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Focused,
    Unfocused,
}

/// Whether the window can be seen: on screen and not covered, as macOS's
/// occlusion state says; `Unknown` where the platform does not say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visible {
    /// Seen, at least in part: only macOS can say so.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    Seen,
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    Covered,
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    Unknown,
}

/// What to do with one notice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plan {
    Show,
    Close,
    Drop(Why),
}

/// Why one was not shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    /// Its occasion: the window is focused, or visible.
    Occasion,
    /// The flood bound.
    Rate,
    /// The setting is off.
    Off,
}

/// **Whether to show it**: its occasion, then the bound. A close is always
/// carried out. `o=invisible` on a platform that cannot tell what is
/// visible is read as `unfocused`, the text's "best guess" for a value the
/// terminal did not claim.
pub fn plan<T: Moment>(
    p: &Posted,
    setting: Setting,
    focus: Focus,
    visible: Visible,
    bucket: &mut Bucket<T>,
    now: T,
) -> Plan {
    if p.action == NoticeAction::Close {
        return Plan::Close;
    }
    if setting == Setting::Off {
        return Plan::Drop(Why::Off);
    }
    let shown = match (p.occasion, focus, visible) {
        (Occasion::Always, _, _) => true,
        (_, Focus::Focused, _) => false,
        (Occasion::Unfocused, Focus::Unfocused, _) => true,
        (Occasion::Invisible, Focus::Unfocused, Visible::Seen) => false,
        (Occasion::Invisible, Focus::Unfocused, Visible::Covered | Visible::Unknown) => true,
    };
    if !shown {
        return Plan::Drop(Why::Occasion);
    }
    match bucket.take(now) {
        Taken::Yes => Plan::Show,
        Taken::No => Plan::Drop(Why::Rate),
    }
}

/// Whether the bound let one through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Taken {
    Yes,
    No,
}

/// The burst the bound allows, and how fast it refills.
pub const BURST: Duration = Duration::from_secs(4);
pub const EACH: Duration = Duration::from_secs(1);

/// **The flood bound**: credit in time, at most [`BURST`], each
/// notification spending [`EACH`], credit coming back as the clock runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bucket<T> {
    credit: Duration,
    at: Option<T>,
    /// How many it dropped, for the log.
    pub dropped: Dropped,
    /// How many bursts it has seen, for the log: one begins at a take
    /// that finds the bucket full.
    pub bursts: Bursts,
}

/// A count of notifications the bound dropped.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Dropped(pub u64);

/// A count of bursts: a flood that never lets the bucket fill is one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Bursts(pub u64);

impl<T: Moment> Bucket<T> {
    pub fn new() -> Bucket<T> {
        Bucket {
            credit: BURST,
            at: None,
            dropped: Dropped(0),
            bursts: Bursts(0),
        }
    }

    pub fn take(&mut self, now: T) -> Taken {
        if let Some(at) = self.at {
            self.credit = (self.credit + now.elapsed_since(at)).min(BURST);
        }
        self.at = Some(now);
        if self.credit == BURST {
            self.bursts.0 += 1;
        }
        match self.credit >= EACH {
            true => {
                self.credit -= EACH;
                Taken::Yes
            }
            false => {
                self.dropped.0 += 1;
                Taken::No
            }
        }
    }
}

impl<T: Moment> Default for Bucket<T> {
    fn default() -> Self {
        Bucket::new()
    }
}

/// How many icons the cache keeps, and how many bytes in all.
pub const ICONS: usize = 16;
pub const ICON_BYTES: usize = 4 << 20;

/// **The icons programs sent under `g`**, the most recently used last.
#[derive(Clone, Debug, Default)]
pub struct Icons {
    kept: VecDeque<(Vec<u8>, Vec<u8>)>,
}

impl Icons {
    /// Keep `data` under `key`, forgetting the least recently used past
    /// [`ICONS`] or [`ICON_BYTES`].
    pub fn keep(&mut self, key: &[u8], data: &[u8]) {
        self.kept.retain(|(k, _)| k != key);
        self.kept.push_back((key.to_vec(), data.to_vec()));
        while self.kept.len() > ICONS
            || self.kept.iter().map(|(_, d)| d.len()).sum::<usize>() > ICON_BYTES
        {
            self.kept.pop_front();
        }
    }

    /// The image under `key`, now the most recently used.
    pub fn get(&mut self, key: &[u8]) -> Option<&[u8]> {
        let i = self.kept.iter().position(|(k, _)| k == key)?;
        let e = self.kept.remove(i)?;
        self.kept.push_back(e);
        self.kept.back().map(|(_, d)| &d[..])
    }
}

/// What a platform delivers, as the core answers kitty's query.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    /// macOS: a click focuses the window and can be reported; closes are
    /// not told; the occlusion state says what is visible; two urgencies
    /// (`critical` would need an entitlement baud does not have); the
    /// default sound or none; images as attachments.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    MacOs,
    /// Linux, with what the notification server said it supports:
    /// actions, and named sounds.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    Linux(ServerCaps),
}

/// The capabilities a freedesktop notification server listed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ServerCaps {
    pub actions: Caps,
    pub markup: Caps,
}

impl ServerCaps {
    /// What a server is taken to have before it answers: nothing.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub const NONE: ServerCaps = ServerCaps {
        actions: Caps::Absent,
        markup: Caps::Absent,
    };
}

/// Whether a server listed a capability.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub enum Caps {
    Listed,
    Absent,
}

/// **What the core is told**: `None` while the setting is off, else what
/// the platform delivers. On Linux a click on a notification that asked
/// `focus` activates the window with the token the server's
/// `ActivationToken` gives (`bus.rs`), where it sends one -- "Clients
/// should not assume the server will generate this signal"
/// (`[[source: freedesktop-notification-spec]]`) -- and the compositor
/// decides what that does (`[[source: xdg-activation-v1]]`). Neither can
/// be known when a program asks, so only `report` is claimed, and it and
/// the buttons only where the server lists actions.
pub fn support(setting: Setting, platform: Platform) -> Option<NoticeSupport> {
    if setting == Setting::Off {
        return None;
    }
    Some(match platform {
        Platform::MacOs => NoticeSupport {
            actions: Activation::FOCUS_AND_REPORT,
            close_events: CloseEvents::Untracked,
            occasions: Occasions::UnfocusedAndInvisible,
            urgencies: Urgencies::LowAndNormal,
            sounds: Sounds::SystemAndSilent,
            expiry: ExpirySupport::Expires,
            icons: IconSupport::NamesAndImages,
            buttons: ButtonSupport::Shown,
        },
        Platform::Linux(caps) => NoticeSupport {
            actions: match caps.actions {
                Caps::Listed => Activation::REPORT,
                Caps::Absent => Activation::NONE,
            },
            close_events: CloseEvents::Tracked,
            occasions: Occasions::Unfocused,
            urgencies: Urgencies::All,
            sounds: Sounds::SystemAndSilent,
            expiry: ExpirySupport::Expires,
            icons: IconSupport::NamesAndImages,
            // Buttons are `Notify`'s actions too, shown only by a server
            // that lists `actions`.
            buttons: match caps.actions {
                Caps::Listed => ButtonSupport::Shown,
                Caps::Absent => ButtonSupport::NotShown,
            },
        },
    })
}

/// The icon name a platform's theme has for one of kitty's "Universally
/// available icon names", from the freedesktop Icon Naming Specification
/// (`[[source: freedesktop-icon-naming-spec]]`) where it names one; any
/// other name -- an application's id, or `system-monitor`, which the
/// specification has no name for -- is passed as sent, for the theme to
/// resolve.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn icon_name(kitty: &str) -> &str {
    // A name, never a path or a URI: `app_icon` takes "a file:// URI" too,
    // and a program must not make the server open a file it names. kitty's
    // names are application ids and symbol names.
    if kitty.contains(['/', ':']) {
        return "";
    }
    match kitty {
        "error" => "dialog-error",
        "warn" | "warning" => "dialog-warning",
        "info" => "dialog-information",
        "question" => "dialog-question",
        "help" => "help-browser",
        "file-manager" => "system-file-manager",
        "text-editor" => "accessories-text-editor",
        other => other,
    }
}

/// How long after a post the platform's list of what it holds is trusted
/// to have it.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub const SETTLED: Duration = Duration::from_secs(1);

/// The notifications shown and alive, with the deadlines `w` set: the
/// driver closes one when its deadline passes, kitty's "guaranteed to be
/// closed once the expiry time has passed", whatever the platform does.
#[derive(Clone, Debug)]
pub struct Shown<T, P> {
    /// Each one's token, the platform's id, its deadline and when it was
    /// shown.
    alive: Vec<(NoticeToken, P, Option<T>, T)>,
}

impl<T: Moment, P: Clone + PartialEq> Shown<T, P> {
    pub fn new() -> Shown<T, P> {
        Shown { alive: Vec::new() }
    }

    /// `token` shown as the platform's `id`, closing at `now` plus its
    /// expiry where it has one.
    pub fn shown(&mut self, token: NoticeToken, id: P, expiry: Expiry, now: T) {
        let due = match expiry {
            Expiry::After(d) => Some(now.after(d)),
            Expiry::System | Expiry::Never => None,
        };
        self.alive.retain(|(t, _, _, _)| *t != token);
        self.alive.push((token, id, due, now));
        // On macOS nothing says one closed, so the oldest are forgotten
        // past the core's own live table.
        if self.alive.len() > baud_domain::NOTICE_LIVE {
            self.alive.remove(0);
        }
    }

    /// The platform's id for `token`.
    pub fn id(&self, token: NoticeToken) -> Option<P> {
        self.alive
            .iter()
            .find(|(t, _, _, _)| *t == token)
            .map(|(_, id, _, _)| id.clone())
    }

    /// The token the platform's `id` names.
    pub fn token(&self, id: &P) -> Option<NoticeToken> {
        self.alive
            .iter()
            .find(|(_, i, _, _)| i == id)
            .map(|(t, _, _, _)| *t)
    }

    /// Forget `token`.
    pub fn gone(&mut self, token: NoticeToken) {
        self.alive.retain(|(t, _, _, _)| *t != token);
    }

    /// **The ones the platform no longer has**, taken out: each shown at
    /// least [`SETTLED`] before `asked`, the moment the platform's list was
    /// asked for, and missing from `present`, the list -- a post is added
    /// to the list asynchronously, so one shown just before the ask may
    /// not be in it yet.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    pub fn absent(&mut self, present: &[P], asked: T) -> Vec<NoticeToken> {
        let (gone, kept): (Vec<_>, Vec<_>) = self
            .alive
            .drain(..)
            .partition(|(_, id, _, at)| at.after(SETTLED) <= asked && !present.contains(id));
        self.alive = kept;
        gone.into_iter().map(|(t, _, _, _)| t).collect()
    }

    /// Whether any is shown.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    pub fn any(&self) -> bool {
        !self.alive.is_empty()
    }

    /// The ones whose deadline has passed, their deadlines spent: each is
    /// still alive until the platform says it closed.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn due(&mut self, now: T) -> Vec<(NoticeToken, P)> {
        let mut due = Vec::new();
        for (t, id, d, _) in &mut self.alive {
            if d.is_some_and(|d| d <= now) {
                *d = None;
                due.push((*t, id.clone()));
            }
        }
        due
    }

    /// The nearest deadline, for the driver's wait.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn next_due(&self) -> Option<T> {
        self.alive.iter().filter_map(|(_, _, d, _)| *d).min()
    }
}

impl<T: Moment, P: Clone + PartialEq> Default for Shown<T, P> {
    fn default() -> Self {
        Shown::new()
    }
}

/// **What a driver keeps for the notifications it delivers**: the
/// setting, the bound, the icons, what is shown, what waits for the
/// platform's answer, and the last progress and attention it acted on.
#[derive(Debug)]
pub struct Delivery<T, P> {
    pub setting: Setting,
    pub bucket: Bucket<T>,
    pub icons: Icons,
    pub shown: Shown<T, P>,
    /// Posted and not yet answered, with the expiry each asked.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub waiting: Vec<(NoticeToken, Expiry)>,
    progress: ReportCount,
    attention: ReportCount,
    /// How many the bound had dropped when it last said so, and in which
    /// burst.
    said: (Dropped, Bursts),
    /// The images Linux did not show ([`Delivery::image_data`]), and
    /// whether the log has said so.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub unshown: Unshown,
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    unshown_said: Said,
}

/// A count of the images a notification carried that were not shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Unshown(pub u64);

/// Whether a line that is said once has been.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Said {
    Not,
    Once,
}

impl<T: Moment, P: Clone + PartialEq> Delivery<T, P> {
    pub fn new(setting: Setting, term: &Term<'_>) -> Delivery<T, P> {
        Delivery {
            setting,
            bucket: Bucket::new(),
            icons: Icons::default(),
            shown: Shown::new(),
            waiting: Vec::new(),
            progress: term.progress_reports(),
            attention: term.attention_requests(),
            said: (Dropped(0), Bursts(0)),
            unshown: Unshown(0),
            unshown_said: Said::Not,
        }
    }

    /// The progress, when a report has come since the last call.
    pub fn progress(&mut self, term: &Term<'_>) -> Option<Progress> {
        let now = term.progress_reports();
        (now != self.progress).then(|| {
            self.progress = now;
            term.progress()
        })
    }

    /// The attention request, when one has come since the last call.
    pub fn attention(&mut self, term: &Term<'_>) -> Option<Attention> {
        let now = term.attention_requests();
        if now == self.attention {
            return None;
        }
        self.attention = now;
        term.attention()
    }

    /// A line for the log when the bound has dropped more since it last
    /// said, in a burst it has not spoken in: once a burst, not once a
    /// notification nor once a read, and counting every drop since the
    /// last line.
    pub fn rate_line(&mut self) -> Option<String> {
        let (d, b) = (self.bucket.dropped, self.bucket.bursts);
        (d != self.said.0 && b != self.said.1).then(|| {
            let n = d.0 - self.said.0.0;
            self.said = (d, b);
            format!("notifications: {n} dropped, more than four at once or one a second")
        })
    }

    /// **The image Linux sends as `image-data`**: a PNG of at most 1024
    /// pixels a side, decoded ([`rgba`]). Any other image a program sent
    /// -- JPEG and GIF, which kitty's text also names and which wait for a
    /// decoder, a larger PNG, or one that does not decode -- is not shown,
    /// and counted.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn image_data(&mut self, p: &Posted) -> Option<(baud_domain::png::Size, Vec<u8>)> {
        let b = p.icon.as_deref()?;
        let shown = rgba(b);
        if shown.is_none() {
            self.unshown.0 += 1;
        }
        shown
    }

    /// A line for the log the first time an image was not shown, and
    /// never again.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn image_line(&mut self) -> Option<String> {
        (self.unshown.0 > 0 && self.unshown_said == Said::Not).then(|| {
            self.unshown_said = Said::Once;
            format!(
                "notifications: images not shown, {} so far: Linux shows a PNG of at most 1024 pixels a side, and JPEG and GIF wait for a decoder; said once",
                self.unshown.0
            )
        })
    }

    /// The expiry `token` asked, taken from the waiting list.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn answered(&mut self, token: NoticeToken) -> Expiry {
        match self.waiting.iter().position(|(t, _)| *t == token) {
            Some(i) => self.waiting.swap_remove(i).1,
            None => Expiry::System,
        }
    }
}

/// **One call the macOS driver makes**, decided here so a test can read
/// the calls without making them: posting a notification, removing one,
/// closing one at its expiry, the Dock tile's bar and an attention
/// request.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub enum MacCall {
    Post(MacPost),
    Remove(String),
    /// Remove it when `Duration` has passed, scheduled on the main queue.
    Expire(String, Duration),
    Bar(Option<DockBar>),
    Attention(AttentionCall),
}

/// A notification as the macOS center posts it.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub struct MacPost {
    pub id: String,
    pub title: String,
    pub body: String,
    pub silent: Silence,
    /// `u=0` is passive, the rest active: critical needs an entitlement.
    pub passive: Passive,
    pub buttons: Vec<String>,
    /// The image and the file extension its bytes say it is.
    pub image: Option<(Vec<u8>, &'static str)>,
}

/// Whether a post is silent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub enum Silence {
    Sound,
    Silent,
}

/// Whether a post is passive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub enum Passive {
    Passive,
    Active,
}

/// The Dock tile's bar.
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub struct DockBar {
    pub fraction: Option<f64>,
    pub tone: DockTone,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub enum DockTone {
    Normal,
    Error,
    Paused,
}

/// `RequestAttention`'s three, as `NSApplication` takes them: "yes ... the
/// dock icon indefinitely" is a critical request, "once ... a single
/// time" an informational one, "no" a cancel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub enum AttentionCall {
    Critical,
    Informational,
    Cancel,
}

/// The Dock tile's bar for a program's progress: none while it is hidden.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn dock_bar(p: Progress) -> Option<DockBar> {
    use baud_core::ProgressState as S;
    let tone = match p.state {
        S::Hidden => return None,
        S::Normal | S::Indeterminate => DockTone::Normal,
        S::Error => DockTone::Error,
        S::Paused => DockTone::Paused,
    };
    Some(DockBar {
        fraction: p.percent.map(baud_core::Percent::fraction),
        tone,
    })
}

/// How urgent a window's bar is on the one Dock tile, most urgent first:
/// the Windows taskbar's order for a group, `TBPF_ERROR`, `TBPF_PAUSED`,
/// `TBPF_NORMAL`, `TBPF_INDETERMINATE` (see [`DockBars`]). A bar with no
/// fraction draws as running with none, which is indeterminate's.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Urgent {
    Error,
    Paused,
    Determinate,
    Indeterminate,
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
impl Urgent {
    fn of(b: &DockBar) -> Urgent {
        match (b.tone, b.fraction) {
            (DockTone::Error, _) => Urgent::Error,
            (DockTone::Paused, _) => Urgent::Paused,
            (DockTone::Normal, Some(_)) => Urgent::Determinate,
            (DockTone::Normal, None) => Urgent::Indeterminate,
        }
    }
}

/// **The one Dock tile's bar, over every window's progress**, keyed by
/// window. The app has one tile and each window its own progress, so the
/// tile shows the rule the Windows taskbar applies when one button stands
/// for several windows -- the system `OSC 9;4` was made for ("set error
/// state in progress on Windows 7 taskbar", `[[source:
/// conemu-ansi-escape-codes]]`): "The taskbar button can show a progress
/// indicator for only one window at a time ... the taskbar button chooses
/// its progress display based on state priority", error, paused, normal,
/// indeterminate, and "In the case of a priority collision between two
/// windows that are broadcasting determinate progress, the window with the
/// least progress is used" (`[[source:
/// ms-itaskbarlist3-setprogressstate]]`). Windows Terminal orders its
/// tabs by the same state priority, keeping the first tab on a tie
/// (`TaskbarState::Priority`, `[[source: wt-6676c6f-taskbarstate]]`).
/// The least-progress tie-break is the taskbar's own. A window's close
/// takes its bar with it.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct DockBars<K> {
    each: Vec<(K, DockBar)>,
    shown: Option<DockBar>,
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
impl<K: PartialEq> DockBars<K> {
    pub const fn new() -> DockBars<K> {
        DockBars {
            each: Vec::new(),
            shown: None,
        }
    }

    /// Window `k`'s bar is now `bar`, `None` when it shows no progress;
    /// the tile's bar when that changes it.
    pub fn set(&mut self, k: K, bar: Option<DockBar>) -> Option<Option<DockBar>> {
        self.each.retain(|(w, _)| *w != k);
        if let Some(b) = bar {
            self.each.push((k, b));
        }
        self.chosen()
    }

    /// Window `k` has closed; the tile's bar when that changes it.
    pub fn gone(&mut self, k: K) -> Option<Option<DockBar>> {
        self.set(k, None)
    }

    /// The tile's bar, and whether it changed since the last shown.
    fn chosen(&mut self) -> Option<Option<DockBar>> {
        let top = self.each.iter().map(|(_, b)| Urgent::of(b)).min();
        let pick = top.and_then(|u| {
            self.each
                .iter()
                .map(|(_, b)| *b)
                .filter(|b| Urgent::of(b) == u)
                .min_by(|x, y| match (x.fraction, y.fraction) {
                    (Some(a), Some(b)) => a.total_cmp(&b),
                    (Some(_), None) => core::cmp::Ordering::Less,
                    (None, Some(_)) => core::cmp::Ordering::Greater,
                    (None, None) => core::cmp::Ordering::Equal,
                })
        });
        (pick != self.shown).then(|| {
            self.shown = pick;
            pick
        })
    }
}

/// The extension an image's first bytes name: kitty's three formats.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn image_kind(b: &[u8]) -> Option<&'static str> {
    match b {
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [0xff, 0xd8, ..] => Some("jpg"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        _ => None,
    }
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
impl<T: Moment> Delivery<T, String> {
    /// **What the macOS driver does with a read's notices**, and the
    /// core told what became of each: shown (and a program that asked
    /// told its close is untracked), dropped, or closed. A new one's
    /// identifier is `prefix` and its token, so a later update by the same
    /// kitty id, which keeps the token, replaces it.
    pub fn mac(
        &mut self,
        term: &mut Term<'_>,
        posted: Vec<Posted>,
        window: (Focus, Visible),
        now: T,
        prefix: &str,
        mut reply: impl FnMut(&[u8]),
    ) -> Vec<MacCall> {
        let mut calls = Vec::new();
        for p in posted {
            match plan(&p, self.setting, window.0, window.1, &mut self.bucket, now) {
                Plan::Show => {
                    let id = self
                        .shown
                        .id(p.token)
                        .unwrap_or_else(|| format!("{prefix}-{:?}", p.token));
                    calls.push(MacCall::Post(MacPost {
                        id: id.clone(),
                        title: p.title.clone(),
                        body: p.body.clone(),
                        silent: match p.sound {
                            Sound::Silent => Silence::Silent,
                            _ => Silence::Sound,
                        },
                        passive: match p.urgency {
                            Urgency::Low => Passive::Passive,
                            _ => Passive::Active,
                        },
                        buttons: p.buttons.clone(),
                        image: p
                            .icon
                            .as_ref()
                            .and_then(|b| image_kind(b).map(|k| (b.clone(), k))),
                    }));
                    if let Expiry::After(d) = p.expiry {
                        calls.push(MacCall::Expire(id.clone(), d));
                    }
                    self.shown.shown(p.token, id, p.expiry, now);
                    crate::reads::input(term, |t| t.notice_shown(p.token), &mut reply);
                }
                // Closed by baud, so the core forgets it at once: on macOS
                // nothing else would say so (`p=alive`).
                Plan::Close => {
                    if let Some(id) = self.shown.id(p.token) {
                        self.shown.gone(p.token);
                        term.notice_dropped(p.token);
                        calls.push(MacCall::Remove(id));
                    }
                }
                Plan::Drop(_) => term.notice_dropped(p.token),
            }
        }
        if let Some(pr) = self.progress(term) {
            calls.push(MacCall::Bar(dock_bar(pr)));
        }
        if let Some(a) = self.attention(term) {
            calls.push(MacCall::Attention(match a {
                Attention::Yes => AttentionCall::Critical,
                Attention::Once => AttentionCall::Informational,
                Attention::No => AttentionCall::Cancel,
            }));
        }
        calls
    }
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
impl<T: Moment> Delivery<T, String> {
    /// **The ones whose `w` has passed, forgotten by the core**: the
    /// removal itself is scheduled when it is posted (`MacCall::Expire`),
    /// and nothing on macOS says it happened.
    pub fn mac_expired(&mut self, term: &mut Term<'_>, now: T) {
        for (token, _) in self.shown.due(now) {
            self.shown.gone(token);
            term.notice_dropped(token);
        }
    }

    /// **The ones Notification Center no longer holds**, forgotten by the
    /// core, so `p=alive` names only what is still there: `present` is its
    /// list, asked for at `asked`.
    pub fn mac_absent(&mut self, term: &mut Term<'_>, present: &[String], asked: T) {
        for token in self.shown.absent(present, asked) {
            term.notice_dropped(token);
        }
    }

    /// Forget the posts the user did not allow, whose ids are `ids`.
    pub fn mac_refused(&mut self, term: &mut Term<'_>, ids: &[String]) {
        for id in ids {
            if let Some(token) = self.shown.token(id) {
                self.shown.gone(token);
                term.notice_dropped(token);
            }
        }
    }
}

/// What a click's action identifier names: the notification itself, or
/// its `n`th button, from 1.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn clicked(action: &str, default: &str) -> Option<Option<baud_core::NoticeButton>> {
    if action == default {
        return Some(None);
    }
    let n: usize = action.strip_prefix("button-")?.parse().ok()?;
    n.checked_sub(1)
        .and_then(baud_core::NoticeButton::of_index)
        .map(Some)
}

/// The area the core is lent for notifications: an icon of 256 KiB, a
/// 256x256 PNG kitty recommends with room to spare.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub const BUDGET: baud_core::NoticeBudget = baud_core::NoticeBudget::with_icon(256 << 10);

/// A launcher's progress for a program's: its fraction while it has one,
/// an empty bar while it runs with none, nothing while hidden. The
/// launcher has no states, so an error and a pause show as their fraction.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn launcher_progress(p: Progress) -> Option<f64> {
    match (p.state, p.percent) {
        (baud_core::ProgressState::Hidden, _) => None,
        (_, Some(pct)) => Some(pct.fraction()),
        (_, None) => Some(0.0),
    }
}

/// A notification's image as RGBA, if it is a PNG of at most 1024 pixels a
/// side: the format kitty's `p=icon` names that baud decodes, at a size an
/// icon has. JPEG and GIF, the other two it names, are not decoded here.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn rgba(png: &[u8]) -> Option<(baud_domain::png::Size, Vec<u8>)> {
    let p = baud_domain::png::Png::parse(png).ok()?;
    let size = p.size();
    if size.width > 1024 || size.height > 1024 {
        return None;
    }
    let mut raw = vec![0u8; p.raw_len()?];
    let mut out = vec![0u8; p.rgba_len()?];
    p.decode(&mut raw, &mut out).ok()?;
    Some((size, out))
}

/// Text for a server that reads markup: "Body text may contain markup",
/// and what a program sent is text, so `&`, `<` and `>` are escaped.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::{DockBar, DockBars, DockTone};

    fn bar(tone: DockTone, fraction: Option<f64>) -> Option<DockBar> {
        Some(DockBar { fraction, tone })
    }

    /// **The one Dock tile shows the most urgent window's bar, the Windows
    /// taskbar's group rule**: error over paused over determinate over
    /// indeterminate; among determinate, the least progress; indeterminate
    /// only while every window with progress is; a window that clears its
    /// progress or closes takes its bar away and the next one shows. The
    /// control: one window's bar alone is the tile's.
    #[test]
    fn the_dock_shows_the_most_urgent_windows_progress() {
        use DockTone::{Error, Normal, Paused};
        let mut d = DockBars::new();
        assert_eq!(
            d.set(1, bar(Normal, Some(0.7))),
            Some(bar(Normal, Some(0.7))),
            "the control"
        );
        assert_eq!(
            d.set(2, bar(Normal, None)),
            None,
            "a determinate bar over an indeterminate one"
        );
        assert_eq!(
            d.set(3, bar(Normal, Some(0.2))),
            Some(bar(Normal, Some(0.2))),
            "the least"
        );
        assert_eq!(
            d.set(4, bar(Paused, Some(0.9))),
            Some(bar(Paused, Some(0.9)))
        );
        assert_eq!(
            d.set(5, bar(Error, None)),
            Some(bar(Error, None)),
            "an error over all"
        );
        assert_eq!(
            d.set(3, bar(Normal, Some(0.25))),
            None,
            "a change under the error shows nothing new"
        );
        assert_eq!(
            d.gone(5),
            Some(bar(Paused, Some(0.9))),
            "the error's window closed"
        );
        assert_eq!(
            d.set(4, None),
            Some(bar(Normal, Some(0.25))),
            "paused cleared: the least again"
        );
        assert_eq!(d.gone(3), Some(bar(Normal, Some(0.7))));
        assert_eq!(
            d.gone(1),
            Some(bar(Normal, None)),
            "indeterminate once it is all there is"
        );
        assert_eq!(d.gone(2), Some(None), "the last window's bar gone: no bar");
        assert_eq!(d.gone(2), None, "a second close changes nothing");
    }
    use super::*;
    use baud_core::{NoticeBudget, Owned, Term};
    use baud_domain::{BytePos, HostTime, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 24,
        ws_col: 80,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// The notices `bytes` publish, taken as a driver takes them.
    fn posted(bytes: &[u8], icons: &mut Icons) -> Vec<Posted> {
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        feed(&mut t, bytes);
        t.notices().map(|n| Posted::of(&n, icons)).collect()
    }

    /// Whole seconds after the clock's start.
    struct Secs(u64);

    fn at(s: Secs) -> HostTime {
        HostTime::ZERO.plus(Duration::from_secs(s.0))
    }

    /// `OSC 9` waits for the window to lose focus; kitty's default
    /// `always` does not; `unfocused` and `invisible` do as they say.
    #[test]
    fn the_occasion_decides_against_the_focus() {
        let mut icons = Icons::default();
        let p = posted(
            b"\x1b]9;nine\x07\x1b]99;;always\x1b\\\x1b]99;o=unfocused;u\x1b\\\x1b]99;o=invisible;i\x1b\\",
            &mut icons,
        );
        let go = |p: &Posted, f, v| {
            let mut b = Bucket::new();
            plan(p, Setting::On, f, v, &mut b, at(Secs(0)))
        };
        let skip = Plan::Drop(Why::Occasion);
        assert_eq!(go(&p[0], Focus::Focused, Visible::Seen), skip);
        assert_eq!(go(&p[0], Focus::Unfocused, Visible::Seen), Plan::Show);
        assert_eq!(go(&p[1], Focus::Focused, Visible::Seen), Plan::Show);
        assert_eq!(go(&p[2], Focus::Focused, Visible::Seen), skip);
        assert_eq!(go(&p[2], Focus::Unfocused, Visible::Seen), Plan::Show);
        assert_eq!(go(&p[3], Focus::Unfocused, Visible::Seen), skip);
        assert_eq!(go(&p[3], Focus::Unfocused, Visible::Covered), Plan::Show);
        assert_eq!(
            go(&p[3], Focus::Unfocused, Visible::Unknown),
            Plan::Show,
            "the best guess"
        );
        assert_eq!(
            plan(
                &p[1],
                Setting::Off,
                Focus::Unfocused,
                Visible::Covered,
                &mut Bucket::new(),
                at(Secs(0))
            ),
            Plan::Drop(Why::Off)
        );
    }

    /// Four at once, then one a second; a quiet spell refills it, no further
    /// than four.
    #[test]
    fn the_bound_lets_four_through_then_one_a_second() {
        let mut b: Bucket<HostTime> = Bucket::new();
        for _ in 0..4 {
            assert_eq!(b.take(at(Secs(0))), Taken::Yes);
        }
        assert_eq!(b.take(at(Secs(0))), Taken::No);
        assert_eq!(b.take(at(Secs(1))), Taken::Yes);
        assert_eq!(b.take(at(Secs(1))), Taken::No);
        let mut ok = 0;
        for _ in 0..10 {
            if b.take(at(Secs(100))) == Taken::Yes {
                ok += 1;
            }
        }
        assert_eq!(ok, 4);
        assert_eq!(b.dropped, Dropped(8));
    }

    /// Controls come out of the text a program sent, and broken UTF-8 is
    /// replaced.
    #[test]
    fn text_has_no_controls() {
        assert_eq!(text(b"a\x1b[31mb\xc2\x9bc\xff"), "a[31mbc\u{fffd}");
    }

    /// An image sent under `g` is used by a later notification that names
    /// only `g`; the cache forgets the least recently used.
    #[test]
    fn icons_are_cached_under_their_key() {
        let mut icons = Icons::default();
        let png = b"\x89PNG..";
        let enc = "iVBORy4u";
        let seq = format!(
            "\x1b]99;i=a:d=0:g=k1;A\x1b\\\x1b]99;i=a:p=icon:e=1;{enc}\x1b\\\x1b]99;g=k1;B\x1b\\"
        );
        let p = posted(seq.as_bytes(), &mut icons);
        assert_eq!(p[0].icon.as_deref(), Some(&png[..]));
        assert_eq!(p[1].icon.as_deref(), Some(&png[..]));
        for i in 0..ICONS {
            icons.keep(format!("o{i}").as_bytes(), b"x");
        }
        assert_eq!(icons.get(b"k1"), None, "forgotten past the cache's size");
    }

    /// **The Linux driver activates the window on a click that asked
    /// focus, with the server's token, and only so**: the arm's activation
    /// compared whole with the spaces taken out, so a button's click, a
    /// notification that did not ask focus, or a token made up where none
    /// came each change the text; and it comes before the click is
    /// reported.
    #[test]
    fn the_linux_driver_activates_on_a_click_that_asked_focus() {
        let src = include_str!("linux.rs");
        let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let arm = src
            .split_once("Heard::Invoked(id, key, activation) => {")
            .and_then(|(_, rest)| rest.split_once("Heard::NotifyClosed(id) =>"))
            .map(|(b, _)| bare(b))
            .expect("the click's arm");
        const ACTIVATE: &str = r#"
            if let (None, true, Some(t)) =
                (button, self.term.notice_focuses(token), activation)
            {
                let _ = self.window.activate_with(&mut self.conn.sender(), &t);
            }
        "#;
        let at = arm
            .find(&bare(ACTIVATE))
            .expect("the activation, as written");
        let reported = arm
            .find("t.notice_activated(token,button)")
            .expect("the report");
        assert!(at < reported, "activated, then reported");
        assert_eq!(arm.matches("activate_with(").count(), 1, "one activation");
    }

    /// The query's claims follow the platform and the setting.
    #[test]
    fn support_claims_what_the_platform_delivers() {
        assert_eq!(support(Setting::Off, Platform::MacOs), None);
        let mac = support(Setting::On, Platform::MacOs).expect("on");
        assert!(mac.actions.focuses() && mac.actions.reports());
        assert_eq!(mac.close_events, CloseEvents::Untracked);
        let none = ServerCaps {
            actions: Caps::Absent,
            markup: Caps::Absent,
        };
        let linux = support(Setting::On, Platform::Linux(none)).expect("on");
        assert!(!linux.actions.focuses() && !linux.actions.reports());
        assert_eq!(linux.occasions, Occasions::Unfocused);
        assert_eq!(linux.buttons, ButtonSupport::NotShown);
        let listed = ServerCaps {
            actions: Caps::Listed,
            markup: Caps::Absent,
        };
        let clicks = support(Setting::On, Platform::Linux(listed)).expect("on");
        assert!(
            clicks.actions.reports() && !clicks.actions.focuses(),
            "focus is not claimed on Linux: no token is promised"
        );
        assert_eq!(clicks.buttons, ButtonSupport::Shown);
        // What kitty's query is answered, either way.
        let answer = |s: Option<NoticeSupport>| {
            let mut owned = Owned::new(&WS).with_notices(BUDGET);
            let mut t = Term::new(owned.storage(), &WS);
            t.set_notice_support(s);
            feed(&mut t, b"\x1b]99;p=?;\x1b\\");
            String::from_utf8_lossy(t.reply()).into_owned()
        };
        assert_eq!(
            answer(Some(clicks)),
            "\x1b]99;i=0:p=?;a=report:c=1:o=always,unfocused:\
             p=title,body,close,icon,?,alive,buttons:s=system,silent:u=0,1,2:w=1\x1b\\"
        );
        assert_eq!(
            answer(Some(linux)),
            "\x1b]99;i=0:p=?;c=1:o=always,unfocused:\
             p=title,body,close,icon,?,alive:s=system,silent:u=0,1,2:w=1\x1b\\"
        );
    }

    /// A deadline from `w` closes it; one with no `w` stays.
    #[test]
    fn an_expiry_is_a_deadline() {
        let mut s: Shown<HostTime, u32> = Shown::new();
        let p = posted(
            b"\x1b]99;i=a:w=1500;a\x1b\\\x1b]99;i=b;b\x1b\\",
            &mut Icons::default(),
        );
        s.shown(p[0].token, 7, p[0].expiry, at(Secs(0)));
        s.shown(p[1].token, 8, p[1].expiry, at(Secs(0)));
        assert_eq!(
            s.next_due(),
            Some(HostTime::ZERO.plus(Duration::from_millis(1500)))
        );
        assert!(s.due(at(Secs(1))).is_empty());
        assert_eq!(s.due(at(Secs(2))), vec![(p[0].token, 7)]);
        assert_eq!(s.token(&8), Some(p[1].token));
        assert_eq!(
            s.token(&7),
            Some(p[0].token),
            "alive until the platform says"
        );
        assert_eq!(s.id(p[1].token), Some(8));
        assert_eq!(s.next_due(), None);
        s.gone(p[1].token);
        assert_eq!(s.id(p[1].token), None);
    }

    /// **The macOS driver's calls, recorded**: a notification posted with
    /// its buttons, sound, level and image, a program that asked told its
    /// close is untracked, an expiry scheduled, an update by id posted
    /// under the same identifier, a close removed, a focused window's
    /// `OSC 9` dropped and the core told, the Dock's bar and an attention
    /// request. Nothing reaches the notification center or the Dock: the
    /// calls are only read.
    #[test]
    fn the_macos_driver_makes_the_calls_the_plan_says() {
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        let mut d: Delivery<HostTime, String> = Delivery::new(Setting::On, &t);
        let png = [0x89, b'P', b'N', b'G', 1, 2];
        let seq = "\x1b]99;i=j:d=0:c=1:u=0:w=3000:s=c2lsZW50;Build\x1b\\\x1b]99;i=j:d=0:p=buttons;Open\u{2028}Skip\x1b\\\x1b]99;i=j:p=icon:e=1;iVBORwEC\x1b\\\x1b]9;4;1;40\x07\x1b]1337;RequestAttention=once\x07";
        feed(&mut t, seq.as_bytes());
        let mut icons = Icons::default();
        let posted: Vec<Posted> = t.notices().map(|n| Posted::of(&n, &mut icons)).collect();
        t.clear_notices();
        let mut replies = Vec::new();
        let calls = d.mac(
            &mut t,
            posted,
            (Focus::Focused, Visible::Seen),
            at(Secs(0)),
            "baud-1",
            |b| replies.extend_from_slice(b),
        );
        let id = match &calls[0] {
            MacCall::Post(p) => {
                assert_eq!(p.title, "Build");
                assert_eq!(p.buttons, ["Open", "Skip"]);
                assert_eq!(p.silent, Silence::Silent);
                assert_eq!(p.passive, Passive::Passive);
                assert_eq!(p.image, Some((png.to_vec(), "png")));
                p.id.clone()
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(
            calls[1],
            MacCall::Expire(id.clone(), Duration::from_millis(3000))
        );
        assert_eq!(
            calls[2],
            MacCall::Bar(Some(DockBar {
                fraction: Some(0.4),
                tone: DockTone::Normal
            }))
        );
        assert_eq!(calls[3], MacCall::Attention(AttentionCall::Informational));
        assert_eq!(replies, b"\x1b]99;i=j:p=close;untracked\x1b\\");

        // The same id again replaces it under the same identifier; its
        // close removes it; a focused window's `OSC 9` is dropped.
        feed(
            &mut t,
            b"\x1b]99;i=j;Again\x1b\\\x1b]99;i=j:p=close;\x1b\\\x1b]9;quiet\x07\x1b]9;4\x07",
        );
        let posted: Vec<Posted> = t.notices().map(|n| Posted::of(&n, &mut icons)).collect();
        t.clear_notices();
        let calls = d.mac(
            &mut t,
            posted,
            (Focus::Focused, Visible::Seen),
            at(Secs(1)),
            "baud-1",
            |_| {},
        );
        assert!(
            matches!(&calls[0], MacCall::Post(p) if p.id == id && p.title == "Again"),
            "{calls:?}"
        );
        assert_eq!(calls[1], MacCall::Remove(id));
        assert_eq!(calls[2], MacCall::Bar(None));
        assert_eq!(calls.len(), 3, "the OSC 9 was dropped");
        assert_eq!(clicked(DEFAULT, DEFAULT), Some(None));
        assert_eq!(
            clicked("button-2", DEFAULT),
            Some(baud_core::NoticeButton::of_index(1))
        );
        assert_eq!(clicked("button-0", DEFAULT), None);
    }

    /// **The Linux driver's pieces**: the launcher's fraction for each
    /// state, the expiry a post waited with, the bound's log line once a
    /// burst, and a server that has not answered taken to list nothing.
    #[test]
    fn the_linux_drivers_pieces_say_what_they_should() {
        use baud_core::{Percent, ProgressState as S};
        let p = |state, pct: Option<u32>| Progress {
            state,
            percent: pct.map(Percent::clamped),
        };
        assert_eq!(launcher_progress(p(S::Hidden, Some(5))), None);
        assert_eq!(launcher_progress(p(S::Normal, Some(25))), Some(0.25));
        assert_eq!(launcher_progress(p(S::Indeterminate, None)), Some(0.0));
        let mut owned = Owned::new(&WS).with_notices(BUDGET);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::Linux(ServerCaps::NONE)));
        feed(&mut t, b"\x1b]99;w=900;a\x1b\\");
        let tok = t.notices().next().expect("one").token;
        let mut d: Delivery<HostTime, u32> = Delivery::new(Setting::On, &t);
        d.waiting
            .push((tok, Expiry::After(Duration::from_millis(900))));
        assert_eq!(d.answered(tok), Expiry::After(Duration::from_millis(900)));
        assert_eq!(d.answered(tok), Expiry::System, "taken once");
        assert_eq!(d.rate_line(), None);
        for _ in 0..6 {
            d.bucket.take(at(Secs(0)));
        }
        assert!(
            d.rate_line()
                .expect("said")
                .starts_with("notifications: 2 dropped")
        );
        assert_eq!(d.rate_line(), None, "once a burst");
        assert!(d.icons.get(b"none").is_none());
        assert_eq!(crate::bus::Launcher::IDLE.progress, None);
    }

    /// **The bound's log line comes once a burst**, however often it is
    /// asked: a flood that keeps going is one burst, and the next line
    /// waits for the bucket to fill again, then counts every drop since.
    #[test]
    fn the_bound_logs_once_a_burst() {
        let mut owned = Owned::new(&WS).with_notices(BUDGET);
        let t = Term::new(owned.storage(), &WS);
        let mut d: Delivery<HostTime, u32> = Delivery::new(Setting::On, &t);
        // `n` notifications in one read at second `s`, the line asked for
        // after each drop and after the read.
        let flood = |d: &mut Delivery<HostTime, u32>, n, s| {
            let mut lines = Vec::new();
            for _ in 0..n {
                if d.bucket.take(at(Secs(s))) == Taken::No {
                    lines.extend(d.rate_line());
                }
            }
            lines.extend(d.rate_line());
            lines
        };
        // Six at once: two dropped, one line, said at the first drop.
        let first = flood(&mut d, 6, 0);
        assert_eq!(first.len(), 1, "{first:?}");
        assert!(
            first[0].starts_with("notifications: 1 dropped"),
            "{first:?}"
        );
        // The flood goes on: one through a second, the rest dropped, and
        // nothing more said.
        assert_eq!(flood(&mut d, 3, 1), Vec::<String>::new());
        assert_eq!(flood(&mut d, 3, 2), Vec::<String>::new());
        // Quiet long enough to fill the bucket: the next burst says so,
        // counting the five dropped since the last line and its own one.
        let next = flood(&mut d, 5, 100);
        assert_eq!(next.len(), 1, "{next:?}");
        assert!(next[0].starts_with("notifications: 6 dropped"), "{next:?}");
    }

    /// A PNG decoded to the RGBA `image-data` carries, and a body escaped
    /// for a server that reads markup.
    #[test]
    fn an_image_decodes_and_markup_is_escaped() {
        const PNG: [u8; 71] = [
            0x89, 0x50, 0x4e, 0x47, 0xd, 0xa, 0x1a, 0xa, 0x0, 0x0, 0x0, 0xd, 0x49, 0x48, 0x44,
            0x52, 0x0, 0x0, 0x0, 0x2, 0x0, 0x0, 0x0, 0x1, 0x8, 0x6, 0x0, 0x0, 0x0, 0xf4, 0x22,
            0x7f, 0x8a, 0x0, 0x0, 0x0, 0xe, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf,
            0xc0, 0x0, 0x42, 0xd, 0x0, 0xf, 0x7a, 0x3, 0x7e, 0x77, 0xe9, 0x7f, 0x97, 0x0, 0x0, 0x0,
            0x0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let (size, px) = rgba(&PNG).expect("decodes");
        assert_eq!((size.width, size.height), (2, 1));
        assert_eq!(px, [255, 0, 0, 255, 0, 0, 255, 128]);
        assert_eq!(rgba(b"not a png"), None);
        assert_eq!(image_kind(&PNG), Some("png"));
        // On Linux the PNG is sent; a GIF, a JPEG and a PNG wider than
        // 1024 pixels are not, and are counted, and the log says so once.
        let mut owned = Owned::new(&WS).with_notices(BUDGET);
        let t = Term::new(owned.storage(), &WS);
        let mut d: Delivery<HostTime, u32> = Delivery::new(Setting::On, &t);
        let mut wide = PNG;
        wide[16..20].copy_from_slice(&1025u32.to_be_bytes());
        wide[29..33].copy_from_slice(&[0xaf, 0xcc, 0xb7, 0x0b]);
        assert!(
            baud_domain::png::Png::parse(&wide).is_ok(),
            "refused for its size, not its header"
        );
        let with = |icon: &[u8]| {
            let mut p = posted(b"\x1b]99;;x\x1b\\", &mut Icons::default()).remove(0);
            p.icon = Some(icon.to_vec());
            p
        };
        let none = posted(b"\x1b]99;;x\x1b\\", &mut Icons::default()).remove(0);
        assert_eq!(d.image_data(&none), None);
        assert_eq!(d.image_line(), None, "no image, nothing to say");
        assert_eq!(d.image_data(&with(&PNG)), Some((size, px)));
        assert_eq!(d.image_line(), None);
        assert_eq!(d.image_data(&with(b"GIF89a\x01\x00")), None);
        assert_eq!(d.image_data(&with(&[0xff, 0xd8, 0xff, 0xe0])), None);
        assert_eq!(d.image_data(&with(&wide)), None);
        assert_eq!(d.unshown, Unshown(3));
        assert!(
            d.image_line()
                .expect("said")
                .starts_with("notifications: images not shown, 3 so far")
        );
        assert_eq!(d.image_data(&with(b"GIF87a")), None);
        assert_eq!(d.unshown, Unshown(4));
        assert_eq!(d.image_line(), None, "said once");
        assert_eq!(escape_markup("a <b> & c"), "a &lt;b&gt; &amp; c");
    }

    /// **`p=alive` stays true on macOS**: a notification baud closed, one
    /// whose `w` passed and one Notification Center no longer holds are
    /// each forgotten by the core; one shown after the list was asked for
    /// is not; a refused post is forgotten.
    #[test]
    fn the_macos_driver_keeps_alive_true() {
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        let mut d: Delivery<HostTime, String> = Delivery::new(Setting::On, &t);
        let post = |t: &mut Term<'_>, d: &mut Delivery<HostTime, String>, b: &[u8], s: u64| {
            feed(t, b);
            let posted: Vec<Posted> = t
                .notices()
                .map(|n| Posted::of(&n, &mut Icons::default()))
                .collect();
            t.clear_notices();
            d.mac(
                t,
                posted,
                (Focus::Unfocused, Visible::Covered),
                at(Secs(s)),
                "p",
                |_| {},
            )
        };
        let alive = |t: &mut Term<'_>| {
            t.clear_reply();
            feed(t, b"\x1b]99;i=q:p=alive;\x1b\\");
            String::from_utf8_lossy(t.reply()).into_owned()
        };
        post(
            &mut t,
            &mut d,
            b"\x1b]99;i=a;1\x1b\\\x1b]99;i=b:w=1000;2\x1b\\\x1b]99;i=c;3\x1b\\",
            0,
        );
        assert_eq!(alive(&mut t), "\x1b]99;i=q:p=alive;a,b,c\x1b\\");
        post(&mut t, &mut d, b"\x1b]99;i=a:p=close;\x1b\\", 1);
        d.mac_expired(&mut t, at(Secs(2)));
        assert_eq!(alive(&mut t), "\x1b]99;i=q:p=alive;c\x1b\\");
        let calls = post(&mut t, &mut d, b"\x1b]99;i=e;5\x1b\\", 4);
        let ids: Vec<String> = Vec::new();
        d.mac_absent(&mut t, &ids, at(Secs(4)));
        assert_eq!(
            alive(&mut t),
            "\x1b]99;i=q:p=alive;e\x1b\\",
            "e was shown too lately before the ask to be taken as gone"
        );
        let Some(MacCall::Post(e)) = calls.first() else {
            panic!("{calls:?}");
        };
        assert!(d.shown.any());
        d.mac_refused(&mut t, std::slice::from_ref(&e.id));
        assert_eq!(alive(&mut t), "\x1b]99;i=q:p=alive;\x1b\\");
        assert!(!d.shown.any());
    }

    /// The default action's identifier, as the framework names it.
    const DEFAULT: &str = "com.apple.UNNotificationDefaultActionIdentifier";

    #[test]
    fn kittys_names_map_to_the_naming_specification() {
        assert_eq!(icon_name("warn"), "dialog-warning");
        assert_eq!(icon_name("system-monitor"), "system-monitor");
        assert_eq!(icon_name("org.example.App"), "org.example.App");
        assert_eq!(icon_name("file:///etc/passwd"), "", "a URI is not a name");
        assert_eq!(icon_name("/tmp/x.png"), "", "nor a path");
    }
}
