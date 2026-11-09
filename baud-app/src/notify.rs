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
//!   a PNG, a JPEG or a GIF of at most 1024 pixels a side, decoded by
//!   baud's own decoders; the rest are counted ([`Delivery::image_data`]).
//! * **iTerm2's picture**: the local file its `image` names is read only
//!   when the notification is shown, through the reader kitty's file
//!   medium reads through, with every check it makes
//!   ([`Delivery::image`]), and shown: on Linux as an icon a program sent
//!   is, on macOS attached once baud's own decoders read it, checked off
//!   the main thread ([`Checker`]) and posted when the check is done
//!   ([`Delivery::mac_checked`]) or its window closes
//!   ([`Delivery::mac_closing`]); nothing about it goes back to the
//!   program.
//!   A notification with nothing a platform can show is not posted, and
//!   counted ([`Delivery::bare_line`]).

use std::collections::VecDeque;
use std::time::Duration;

use baud_core::{
    Activation, Attention, ButtonSupport, CloseEvents, CloseReport, Expiry, ExpirySupport,
    IconSupport, Notice, NoticeAction, NoticeButton, NoticeFrom, NoticeSupport, NoticeToken,
    Occasion, Occasions, Progress, ReportCount, Sounds, Term, Urgencies, Urgency,
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
    /// iTerm2's `subtitle`.
    pub subtitle: String,
    /// The path of the local file iTerm2's `image` names, read only when
    /// the notification is shown ([`Delivery::image`]).
    pub picture: Option<Vec<u8>>,
    /// The buttons it shows, each with the number the core gave it: a
    /// label of controls alone shows nothing and is left out, and the
    /// rest keep their numbers, which a click reports.
    pub buttons: Vec<(baud_core::NoticeButton, String)>,
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
            subtitle: text(n.subtitle),
            picture: (!n.picture.is_empty()).then(|| n.picture.to_vec()),
            buttons: n
                .buttons()
                .map(|(k, b)| (k, text(b)))
                .filter(|(_, b)| !b.is_empty())
                .collect(),
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
    alive: Vec<Alive<T, P>>,
}

/// One shown notification: its token, the platform's id, its deadline,
/// when it was shown and the buttons it showed.
type Alive<T, P> = (NoticeToken, P, Option<T>, T, Vec<NoticeButton>);

impl<T: Moment, P: Clone + PartialEq> Shown<T, P> {
    pub fn new() -> Shown<T, P> {
        Shown { alive: Vec::new() }
    }

    /// `token` shown as the platform's `id` with `buttons`, closing at
    /// `now` plus its expiry where it has one.
    pub fn shown(
        &mut self,
        token: NoticeToken,
        id: P,
        expiry: Expiry,
        now: T,
        buttons: Vec<NoticeButton>,
    ) {
        let due = match expiry {
            Expiry::After(d) => Some(now.after(d)),
            Expiry::System | Expiry::Never => None,
        };
        self.alive.retain(|(t, _, _, _, _)| *t != token);
        self.alive.push((token, id, due, now, buttons));
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
            .find(|(t, _, _, _, _)| *t == token)
            .map(|(_, id, _, _, _)| id.clone())
    }

    /// **The button of `token` a click's action names**, by the number the
    /// presenter put in it: one of the buttons that notification showed,
    /// or none. A number that parses but names no button it showed is
    /// dropped, never reported, as is any click on one no longer shown.
    pub fn read_back(&self, token: NoticeToken, number: &str) -> Option<NoticeButton> {
        let n: u8 = number.parse().ok()?;
        let (_, _, _, _, shown) = self.alive.iter().find(|(t, _, _, _, _)| *t == token)?;
        shown.iter().copied().find(|b| b.number() == n)
    }

    /// The token the platform's `id` names.
    pub fn token(&self, id: &P) -> Option<NoticeToken> {
        self.alive
            .iter()
            .find(|(_, i, _, _, _)| i == id)
            .map(|(t, _, _, _, _)| *t)
    }

    /// Forget `token`.
    pub fn gone(&mut self, token: NoticeToken) {
        self.alive.retain(|(t, _, _, _, _)| *t != token);
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
            .partition(|(_, id, _, at, _)| at.after(SETTLED) <= asked && !present.contains(id));
        self.alive = kept;
        gone.into_iter().map(|(t, _, _, _, _)| t).collect()
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
        for (t, id, d, _, _) in &mut self.alive {
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
        self.alive.iter().filter_map(|(_, _, d, _, _)| *d).min()
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
    /// Posted and not yet answered, with the expiry each asked and the
    /// buttons each shows.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub waiting: Vec<(NoticeToken, Expiry, Vec<NoticeButton>)>,
    progress: ReportCount,
    attention: ReportCount,
    /// How many the bound had dropped when it last said so, and in which
    /// burst.
    said: (Dropped, Bursts),
    /// The images not shown -- on Linux ([`Delivery::image_data`]), and
    /// iTerm2's pictures on macOS ([`Delivery::mac_checked`]) -- and
    /// whether the log has said so.
    pub unshown: Unshown,
    unshown_said: Said,
    /// The notifications not posted for want of anything the platform
    /// shows ([`untitled`] on Linux; on macOS no text and no picture it
    /// attaches), and whether the log has said so.
    pub bare: Unshown,
    bare_said: Said,
    /// The most pixels iTerm2's picture may decode to on macOS, the user's
    /// `[images] pixels_mb` ([`attaches`]).
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    pictures: baud_domain::ImageBudget,
    /// The macOS notifications whose picture is being checked off the main
    /// thread, each posted when its check comes back
    /// ([`Delivery::mac_checked`]), or when its window closes
    /// ([`Delivery::mac_closing`]).
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    checking: Vec<Posted>,
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
    pub fn new(
        setting: Setting,
        term: &Term<'_>,
        pictures: baud_domain::ImageBudget,
    ) -> Delivery<T, P> {
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
            bare: Unshown(0),
            bare_said: Said::Not,
            pictures,
            checking: Vec::new(),
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

    /// **The image a notification shows, as its bytes**: the one sent with
    /// it or cached under its `g`, else the picture iTerm2's `image`
    /// names, read now through `files`, which is only when it is shown.
    pub fn image(&mut self, p: &Posted, files: &mut dyn PictureReader) -> Option<Vec<u8>> {
        if p.icon.is_some() {
            return p.icon.clone();
        }
        let path = p.picture.as_deref()?;
        files
            .picture(path)
            .map_err(|why| {
                crate::warn!(
                    "notifications: the picture {:?} was not read: {why}",
                    String::from_utf8_lossy(path)
                );
            })
            .ok()
    }

    /// **The image Linux sends as `image-data`**: a PNG, a JPEG or a GIF
    /// of at most 1024 pixels a side, decoded ([`rgba`]). Any other image
    /// -- another format, a larger one, or one that does not decode -- is
    /// not shown, and counted.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn image_data(
        &mut self,
        p: &Posted,
        files: &mut dyn PictureReader,
    ) -> Option<(baud_domain::png::Size, Vec<u8>)> {
        let b = self.image(p, files)?;
        let shown = rgba(&b);
        if shown.is_none() {
            self.unshown.0 += 1;
        }
        shown
    }

    /// A line for the log the first time an image was not shown, and
    /// never again.
    pub fn image_line(&mut self) -> Option<String> {
        (self.unshown.0 > 0 && self.unshown_said == Said::Not).then(|| {
            self.unshown_said = Said::Once;
            format!(
                "notifications: images not shown, {} so far: a PNG, JPEG or GIF is shown when it decodes, on Linux at most 1024 pixels a side, and an iTerm2 picture on macOS within [images] pixels_mb; said once",
                self.unshown.0
            )
        })
    }

    /// A line for the log the first time a notification was not posted for
    /// want of anything to show, and never again.
    pub fn bare_line(&mut self) -> Option<String> {
        (self.bare.0 > 0 && self.bare_said == Said::Not).then(|| {
            self.bare_said = Said::Once;
            format!(
                "notifications: {} not posted, nothing in them to show: a picture alone needs a title or a subtitle on Linux, and on macOS a PNG, JPEG or GIF that was read and decodes; said once",
                self.bare.0
            )
        })
    }

    /// The expiry `token` asked and the buttons it shows, taken from the
    /// waiting list.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn answered(&mut self, token: NoticeToken) -> (Expiry, Vec<NoticeButton>) {
        match self.waiting.iter().position(|(t, _, _)| *t == token) {
            Some(i) => {
                let (_, e, b) = self.waiting.swap_remove(i);
                (e, b)
            }
            None => (Expiry::System, Vec::new()),
        }
    }
}

/// **One call the macOS driver makes**, decided here so a test can read
/// the calls without making them: posting a notification, removing one,
/// closing one at its expiry, the Dock tile's bar and an attention
/// request, and checking iTerm2's picture off the main thread.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub enum MacCall {
    Post(MacPost),
    /// Check this picture on the window's [`Checker`]; its notification
    /// posts when the check comes back ([`Delivery::mac_checked`]).
    Check(Check),
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
    pub subtitle: String,
    pub body: String,
    pub silent: Silence,
    /// `u=0` is passive, the rest active: critical needs an entitlement.
    pub passive: Passive,
    pub buttons: Vec<(baud_core::NoticeButton, String)>,
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
/// (`TerminalPage::TaskbarState`, `[[source: wt-terminalpage]]`
/// 4244-4262; the order is `TaskbarState::Priority`, `[[source:
/// wt-6676c6f-taskbarstate]]`).
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
    /// kitty id, which keeps the token, replaces it. `files` reads iTerm2's
    /// picture for a post it makes, and a picture that has to be decoded
    /// before it is attached is handed to the window's [`Checker`]
    /// (`MacCall::Check`), its notification posted when the check comes
    /// back ([`Delivery::mac_checked`]), so no decode runs here.
    #[allow(clippy::too_many_arguments)]
    pub fn mac(
        &mut self,
        term: &mut Term<'_>,
        posted: Vec<Posted>,
        window: (Focus, Visible),
        now: T,
        prefix: &str,
        files: &mut dyn PictureReader,
        mut reply: impl FnMut(&[u8]),
    ) -> Vec<MacCall> {
        let mut calls = Vec::new();
        for p in posted {
            match plan(&p, self.setting, window.0, window.1, &mut self.bucket, now) {
                Plan::Show => {
                    // A kitty icon is attached by its signature; iTerm2's
                    // picture once it decodes, which is checked off this
                    // thread when its bytes say it is a picture.
                    let image = match &p.icon {
                        Some(b) => image_kind(b).map(|k| (b.clone(), k)),
                        None => match self.image(&p, files) {
                            Some(bytes) if image_kind(&bytes).is_some() => {
                                calls.push(MacCall::Check(Check {
                                    token: p.token,
                                    bytes,
                                    pixels: self.pictures,
                                }));
                                self.checking.push(p);
                                continue;
                            }
                            Some(_) => {
                                self.unshown.0 += 1;
                                None
                            }
                            None => None,
                        },
                    };
                    self.post_mac(term, p, image, now, prefix, &mut calls, &mut reply);
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

    /// **A notification whose picture came back from the [`Checker`],
    /// posted now**: with the picture when its bytes decode within the
    /// pixels `[images] pixels_mb` lets an image decode to ([`attaches`]),
    /// else without it, counted, the text posted; a picture alone that
    /// does not decode is not posted, and counted. Nothing is posted
    /// before the check is done: a second post under the same identifier
    /// "will alert for the new notification request and replace the
    /// existing delivered notification" (`[[source:
    /// macos-sdk-unusernotificationcenter-h]]`), so text first and the
    /// picture after would show and sound twice. A notification with
    /// nothing to check posts as it comes, ahead of one still checked.
    pub fn mac_checked(
        &mut self,
        term: &mut Term<'_>,
        checked: Checked,
        now: T,
        prefix: &str,
        mut reply: impl FnMut(&[u8]),
    ) -> Vec<MacCall> {
        let mut calls = Vec::new();
        let Some(i) = self.checking.iter().position(|p| p.token == checked.token) else {
            return calls;
        };
        let p = self.checking.remove(i);
        let image = checked.kind.map(|k| (checked.bytes, k));
        if image.is_none() {
            self.unshown.0 += 1;
        }
        self.post_mac(term, p, image, now, prefix, &mut calls, &mut reply);
        calls
    }

    /// **The notifications a closing window still holds for their
    /// pictures' checks, posted now**, so none goes with the window, text
    /// and all: each one whose check came back (`finished`) with its
    /// picture as checked ([`Delivery::mac_checked`]), then the rest with
    /// their text alone, each picture counted as not shown, and a picture
    /// alone that never came back not posted, and counted. Nothing waits
    /// for a check. The close a shell's exit asks for waits until none is
    /// out ([`Delivery::checks`]), so the ones that come here still out
    /// are a close the person made.
    pub fn mac_closing(
        &mut self,
        term: &mut Term<'_>,
        finished: Vec<Checked>,
        now: T,
        prefix: &str,
        mut reply: impl FnMut(&[u8]),
    ) -> Vec<MacCall> {
        let mut calls = Vec::new();
        for checked in finished {
            calls.extend(self.mac_checked(term, checked, now, prefix, &mut reply));
        }
        for p in core::mem::take(&mut self.checking) {
            self.unshown.0 += 1;
            self.post_mac(term, p, None, now, prefix, &mut calls, &mut reply);
        }
        calls
    }

    /// Whether a notification waits for its picture's check, which the
    /// close a shell's exit asks for waits on, as it waits for the log.
    pub fn checks(&self) -> Checks {
        match self.checking.is_empty() {
            true => Checks::Done,
            false => Checks::Waiting,
        }
    }

    /// **`p` posted with `image`**, the core told it shown; or, a picture
    /// alone with nothing to show, dropped and counted.
    #[allow(clippy::too_many_arguments)]
    fn post_mac(
        &mut self,
        term: &mut Term<'_>,
        p: Posted,
        image: Option<(Vec<u8>, &'static str)>,
        now: T,
        prefix: &str,
        calls: &mut Vec<MacCall>,
        reply: &mut impl FnMut(&[u8]),
    ) {
        // iTerm2's picture alone, unread or not decoded: nothing to show.
        if untitled(&p) && p.body.is_empty() && image.is_none() {
            self.bare.0 += 1;
            term.notice_dropped(p.token);
            return;
        }
        let id = self
            .shown
            .id(p.token)
            .unwrap_or_else(|| format!("{prefix}-{:?}", p.token));
        calls.push(MacCall::Post(MacPost {
            id: id.clone(),
            title: p.title,
            subtitle: p.subtitle,
            body: p.body,
            silent: match p.sound {
                Sound::Silent => Silence::Silent,
                _ => Silence::Sound,
            },
            passive: match p.urgency {
                Urgency::Low => Passive::Passive,
                _ => Passive::Active,
            },
            buttons: p.buttons.clone(),
            image,
        }));
        if let Expiry::After(d) = p.expiry {
            calls.push(MacCall::Expire(id.clone(), d));
        }
        let buttons = p.buttons.iter().map(|(n, _)| *n).collect();
        self.shown.shown(p.token, id, p.expiry, now, buttons);
        crate::reads::input(term, |t| t.notice_shown(p.token), reply);
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

/// **What a click's action identifier names**: the notification itself,
/// or the button whose number follows `button-`, read back by `read`
/// against the buttons that notification showed ([`Shown::read_back`]).
/// `None`, and nothing reported, for an action naming no button it showed.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn clicked(
    action: &str,
    default: &str,
    read: impl FnOnce(&str) -> Option<NoticeButton>,
) -> Option<Option<NoticeButton>> {
    if action == default {
        return Some(None);
    }
    read(action.strip_prefix("button-")?).map(Some)
}

/// **Whether Linux has no summary to give it**: neither a title nor a
/// subtitle, which is iTerm2's picture alone.
pub fn untitled(p: &Posted) -> bool {
    p.title.is_empty() && p.subtitle.is_empty()
}

/// **The reader of the file iTerm2's `image` names**: both drivers lend
/// the one kitty's file medium reads through (`kitty_files::LocalFiles`),
/// with every check it makes, and a test a reader of its own.
pub trait PictureReader {
    /// The file's bytes, at most [`PICTURE_MAX`], or why not, for the log.
    fn picture(&mut self, path: &[u8]) -> Result<Vec<u8>, String>;
}

/// The most of a picture read: Apple's limit for an image attachment,
/// "10 MB" (`[[source: apple-unnotificationattachment]]`), taken as
/// decimal, the smaller reading.
pub const PICTURE_MAX: usize = 10_000_000;

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

/// **A notification's image as RGBA**, if it is a PNG, a JPEG or a GIF of
/// at most 1024 pixels a side -- the three kitty's `p=icon` names, which
/// baud's own decoders read, a GIF its first frame, a JPEG turned as its
/// orientation says -- at a size an icon has.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn rgba(b: &[u8]) -> Option<(baud_domain::png::Size, Vec<u8>)> {
    rgba_within(b, |s| s.width <= 1024 && s.height <= 1024)
}

/// **A PNG, a JPEG or a GIF as RGBA, if its size `fits`**, decoded by
/// baud's own decoders, a GIF its first frame, a JPEG turned as its
/// orientation says: [`rgba`] for Linux's `image-data`. macOS keeps no
/// pixels, and checks with [`decodes_within`].
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn rgba_within(
    b: &[u8],
    fits: impl Fn(baud_domain::png::Size) -> bool,
) -> Option<(baud_domain::png::Size, Vec<u8>)> {
    use baud_domain::{gif::Gif, jpeg::Jpeg, png::Png};
    if Gif::sniff(b) {
        let g = Gif::parse(b).ok()?;
        let size = g.size();
        if !fits(size) {
            return None;
        }
        let mut scratch = vec![0u8; g.scratch_len()];
        let mut first = None;
        g.decode(&mut scratch, |f| {
            first = Some(f.rgba.to_vec());
            false
        })
        .ok()?;
        return first.map(|px| (size, px));
    }
    if Jpeg::sniff(b) {
        let j = Jpeg::parse(b).ok()?;
        let size = j.size();
        if !fits(size) {
            return None;
        }
        let area = usize::try_from(size.width).ok()? * usize::try_from(size.height).ok()?;
        let mut scratch = vec![0u8; j.scratch_len()?];
        let mut px = vec![baud_domain::Pixel::CLEAR; area];
        j.decode(&mut scratch, &mut px).ok()?;
        let out = px
            .iter()
            .flat_map(|p| {
                let (r, g, b) = p.rgb();
                [r.get(), g.get(), b.get(), p.alpha().get()]
            })
            .collect();
        return Some((size, out));
    }
    let p = Png::parse(b).ok()?;
    let size = p.size();
    if !fits(size) {
        return None;
    }
    let mut raw = vec![0u8; p.raw_len()?];
    let mut out = vec![0u8; p.rgba_len()?];
    p.decode(&mut raw, &mut out).ok()?;
    Some((size, out))
}

/// **Whether a PNG, a JPEG or a GIF decodes, if its size `fits`**, as
/// strictly as [`rgba_within`] and making no RGBA copy: a PNG's scanlines
/// reconstructed, which "refuses what [`Png::decode`] refuses for the same
/// bytes" and leaves the pixels unmade (`Png::reconstruct`); a GIF to its
/// first frame, left in the decoder's canvas; a JPEG into the decoder's
/// own pixels, the one form its decoder decodes to, which are dropped.
/// What it holds at once is the decoder's work: a PNG's inflated
/// scanlines, a GIF's two canvases, a JPEG's scratch and its pixels.
///
/// [`Png::decode`]: baud_domain::png::Png::decode
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn decodes_within(b: &[u8], fits: impl Fn(baud_domain::png::Size) -> bool) -> bool {
    use baud_domain::{gif::Gif, jpeg::Jpeg, png::Png};
    if Gif::sniff(b) {
        let Ok(g) = Gif::parse(b) else {
            return false;
        };
        if !fits(g.size()) {
            return false;
        }
        let mut scratch = vec![0u8; g.scratch_len()];
        let mut first = false;
        let decoded = g.decode(&mut scratch, |_| {
            first = true;
            false
        });
        return decoded.is_ok() && first;
    }
    if Jpeg::sniff(b) {
        let Ok(j) = Jpeg::parse(b) else {
            return false;
        };
        let size = j.size();
        if !fits(size) {
            return false;
        }
        let (Ok(w), Ok(h)) = (usize::try_from(size.width), usize::try_from(size.height)) else {
            return false;
        };
        let Some(len) = j.scratch_len() else {
            return false;
        };
        let mut scratch = vec![0u8; len];
        let mut px = vec![baud_domain::Pixel::CLEAR; w * h];
        return j.decode(&mut scratch, &mut px).is_ok();
    }
    let Ok(p) = Png::parse(b) else {
        return false;
    };
    if !fits(p.size()) {
        return false;
    }
    let Some(len) = p.raw_len() else {
        return false;
    };
    let mut raw = vec![0u8; len];
    p.reconstruct(&mut raw).is_ok()
}

/// **The extension iTerm2's picture attaches as on macOS, if it does**:
/// one of kitty's three formats by its first bytes ([`image_kind`]) that
/// decodes ([`decodes_within`]) within the pixels `[images] pixels_mb`
/// lets an image decode to. iTerm2 attaches a picture only once it
/// decodes (`notifyRich:`, `[[source:
/// iterm2-964a75d-notification-controller]]`), and a file the system
/// cannot read costs the whole request: "If you attach a file to a local
/// notification request that's corrupted, invalid, or of an unsupported
/// file type, the system doesn't schedule your request" (`[[source:
/// apple-unnotificationattachment]]`). The window's [`Checker`] runs it,
/// off the main thread. A 6000x4000 JPEG sampled 4:2:0, within the
/// default `[images] pixels_mb`, holds 36 MB of scratch and 96 MB of
/// pixels while it is checked, 132,018,000 bytes, where the RGBA copy
/// took it to 228,018,000; the file's 10 MB do not bound these, since a
/// picture of one colour compresses to far less than a byte a pixel.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn attaches(b: &[u8], pixels: baud_domain::ImageBudget) -> Option<&'static str> {
    let most = u64::try_from(pixels.pixels).unwrap_or(u64::MAX);
    let fits = |s: baud_domain::png::Size| u64::from(s.width) * u64::from(s.height) <= most;
    image_kind(b).filter(|_| decodes_within(b, fits))
}

/// iTerm2's picture to check off the main thread: its notification's
/// token, its bytes and the pixels it may decode to.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub struct Check {
    pub token: NoticeToken,
    pub bytes: Vec<u8>,
    pub pixels: baud_domain::ImageBudget,
}

/// A picture checked: its notification's token, its bytes, and the
/// extension they attach as, `None` when they do not
/// ([`Delivery::mac_checked`]).
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub struct Checked {
    pub token: NoticeToken,
    pub bytes: Vec<u8>,
    pub kind: Option<&'static str>,
}

/// Whether any of a window's notifications waits for its picture's
/// check ([`Delivery::checks`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub enum Checks {
    /// None waits.
    Done,
    /// One or more is parked until its check comes back.
    Waiting,
}

/// What a [`Checker`] checks a picture with: [`attaches`], or a test's.
type CheckWith = dyn Fn(&[u8], baud_domain::ImageBudget) -> Option<&'static str> + Send + Sync;

/// **The window's checker of iTerm2's pictures, off the main thread**:
/// one thread, started at the first picture and fed through a channel,
/// as the printer list is read, which checks one picture at a time in the
/// order they came, sends each back and wakes the main thread; the
/// driver takes what came back on the next turn of its loop
/// ([`Checker::finished`]) and never waits for it. One at a time, so a
/// burst of four pictures, as many as the bound lets through at once,
/// holds one check's buffers and not four. A check that panics leaves the
/// picture out, so its notification posts its text, as a host-name read
/// that panics leaves the names `gethostname` gives. A thread that cannot
/// be made leaves the check to the calling thread, as the machine's names
/// are read when no thread can be made. Dropped with its window, once the
/// driver has posted the notifications still waiting for it
/// ([`Delivery::mac_closing`]), it takes the channel with it, and the
/// thread ends after the check it is on.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub struct Checker {
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    check: std::sync::Arc<CheckWith>,
    /// The worker's queue, once it runs.
    ask: Option<std::sync::mpsc::Sender<Check>>,
    back: std::sync::mpsc::Sender<Checked>,
    done: std::sync::mpsc::Receiver<Checked>,
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
impl Checker {
    /// A checker that checks with `check` and calls `wake` after each
    /// check, from the thread it ran on.
    pub fn new(
        wake: impl Fn() + Send + Sync + 'static,
        check: impl Fn(&[u8], baud_domain::ImageBudget) -> Option<&'static str> + Send + Sync + 'static,
    ) -> Checker {
        let (back, done) = std::sync::mpsc::channel();
        Checker {
            wake: std::sync::Arc::new(wake),
            check: std::sync::Arc::new(check),
            ask: None,
            back,
            done,
        }
    }

    /// `c` handed to the worker, which is started now if it is not
    /// running; returns at once.
    pub fn start(&mut self, c: Check) {
        let c = match &self.ask {
            Some(ask) => match ask.send(c) {
                Ok(()) => return,
                // The worker ended: start another.
                Err(std::sync::mpsc::SendError(c)) => c,
            },
            None => c,
        };
        let (ask, jobs) = std::sync::mpsc::channel::<Check>();
        let (wake, check, back) = (self.wake.clone(), self.check.clone(), self.back.clone());
        let spawned = std::thread::Builder::new()
            .name("notification-pictures".into())
            .spawn(move || {
                for c in jobs {
                    let kind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        check(&c.bytes, c.pixels)
                    }))
                    .unwrap_or(None);
                    let done = Checked {
                        token: c.token,
                        bytes: c.bytes,
                        kind,
                    };
                    if back.send(done).is_err() {
                        return;
                    }
                    wake();
                }
            });
        match spawned {
            Ok(_) => {
                let _ = ask.send(c);
                self.ask = Some(ask);
            }
            Err(_) => {
                self.ask = None;
                let kind = (self.check)(&c.bytes, c.pixels);
                let _ = self.back.send(Checked {
                    token: c.token,
                    bytes: c.bytes,
                    kind,
                });
                (self.wake)();
            }
        }
    }

    /// The pictures checked since the last call, never waited for.
    pub fn finished(&mut self) -> Vec<Checked> {
        self.done.try_iter().collect()
    }
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

    /// The pixels a test's iTerm2 picture may decode to: `PNG_2X1` and more.
    const PIXELS: baud_domain::ImageBudget = baud_domain::ImageBudget { pixels: 1 << 10 };

    /// A 2x1 RGBA PNG: one red pixel, one half-transparent blue.
    const PNG_2X1: [u8; 71] = [
        0x89, 0x50, 0x4e, 0x47, 0xd, 0xa, 0x1a, 0xa, 0x0, 0x0, 0x0, 0xd, 0x49, 0x48, 0x44, 0x52,
        0x0, 0x0, 0x0, 0x2, 0x0, 0x0, 0x0, 0x1, 0x8, 0x6, 0x0, 0x0, 0x0, 0xf4, 0x22, 0x7f, 0x8a,
        0x0, 0x0, 0x0, 0xe, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x0, 0x42,
        0xd, 0x0, 0xf, 0x7a, 0x3, 0x7e, 0x77, 0xe9, 0x7f, 0x97, 0x0, 0x0, 0x0, 0x0, 0x49, 0x45,
        0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

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

    /// A reader with no files, for tests that name none: asked, it fails.
    struct NoFiles;

    impl PictureReader for NoFiles {
        fn picture(&mut self, _: &[u8]) -> Result<Vec<u8>, String> {
            Err("no files".into())
        }
    }

    /// A reader that counts what it is asked for and answers `bytes`.
    struct Files {
        asked: Vec<Vec<u8>>,
        bytes: Result<Vec<u8>, String>,
    }

    impl PictureReader for Files {
        fn picture(&mut self, path: &[u8]) -> Result<Vec<u8>, String> {
            self.asked.push(path.to_vec());
            self.bytes.clone()
        }
    }

    /// Whole seconds after the clock's start.
    struct Secs(u64);

    fn at(s: Secs) -> HostTime {
        HostTime::ZERO.plus(Duration::from_secs(s.0))
    }

    /// `calls` with each picture checked here, as the window's [`Checker`]
    /// checks it, and its notification's calls in the check's place.
    fn checked_now(
        d: &mut Delivery<HostTime, String>,
        t: &mut Term<'_>,
        calls: Vec<MacCall>,
        now: HostTime,
    ) -> Vec<MacCall> {
        let mut out = Vec::new();
        for c in calls {
            match c {
                MacCall::Check(c) => {
                    let kind = attaches(&c.bytes, c.pixels);
                    let back = Checked {
                        token: c.token,
                        bytes: c.bytes,
                        kind,
                    };
                    out.extend(d.mac_checked(t, back, now, "baud-1", |_| {}));
                }
                c => out.push(c),
            }
        }
        out
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
        s.shown(p[0].token, 7, p[0].expiry, at(Secs(0)), Vec::new());
        s.shown(p[1].token, 8, p[1].expiry, at(Secs(0)), Vec::new());
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
    /// The labels a click test sends: ordinary, control-only, empty.
    const LABELS: [&str; 3] = ["Go", "\u{85}", ""];

    /// Every list of one to four labels drawn from [`LABELS`].
    fn label_lists() -> Vec<Vec<&'static str>> {
        let mut out: Vec<Vec<&str>> = vec![vec![]];
        let mut all = Vec::new();
        for _ in 0..4 {
            out = out
                .iter()
                .flat_map(|l| {
                    LABELS.iter().map(move |x| {
                        let mut l = l.clone();
                        l.push(*x);
                        l
                    })
                })
                .collect();
            all.extend(out.iter().cloned());
        }
        all
    }

    /// The report a click on the shown button `shown` of a notification
    /// whose program sent `labels` sends back, and that button's place
    /// among the labels the program sent, which the program counts.
    fn click_each(labels: &[&str]) -> Vec<(Vec<u8>, usize)> {
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        let mut s = support(Setting::On, Platform::MacOs).expect("supported");
        s.actions = baud_core::Activation::FOCUS_AND_REPORT;
        t.set_notice_support(Some(s));
        // Each ordinary label numbered so it can be found, `Go0`, `Go3`;
        // a control-only one stays control-only.
        let named: Vec<String> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| match *l {
                "Go" => format!("Go{i}"),
                l => l.to_string(),
            })
            .collect();
        let seq = format!(
            "\x1b]99;i=n:a=report:d=0;T\x1b\\\x1b]99;i=n:p=buttons;{}\x1b\\",
            named.join("\u{2028}")
        );
        feed(&mut t, seq.as_bytes());
        let posted: Vec<Posted> = t
            .notices()
            .map(|n| Posted::of(&n, &mut Icons::default()))
            .collect();
        let p = &posted[0];
        let mut shown: Shown<HostTime, u32> = Shown::new();
        let numbers = p.buttons.iter().map(|(n, _)| *n).collect();
        shown.shown(p.token, 1, p.expiry, at(Secs(0)), numbers);
        let sent: Vec<&String> = named.iter().filter(|l| !l.is_empty()).collect();
        let mut out = Vec::new();
        for (n, label) in &p.buttons {
            let place = 1 + sent
                .iter()
                .position(|l| text(l.as_bytes()) == *label)
                .expect("a shown label was sent");
            // The action a presenter names the button by.
            let action = format!("button-{}", n.number());
            let read = |n: &str| shown.read_back(p.token, n);
            let button = clicked(&action, DEFAULT, read).expect("a button's action");
            t.clear_reply();
            t.notice_activated(p.token, button);
            out.push((t.reply().to_vec(), place));
        }
        out
    }

    /// **A click reports the label's place in the program's list**, the
    /// number the program counts its buttons by, over every list of one to
    /// four labels mixing ordinary, control-only and empty ones: a
    /// control-only label is not shown, and the labels after it keep their
    /// places. It reported its place among the labels shown, so for `Go`,
    /// a control-only label and `Go`, the second `Go` reported 2, not 3.
    #[test]
    fn a_click_reports_the_labels_place_in_the_programs_list() {
        let mut wrong = Vec::new();
        let mut clicks = 0;
        for labels in label_lists() {
            for (reply, place) in click_each(&labels) {
                clicks += 1;
                let want = format!("\x1b]99;i=n;{place}\x1b\\").into_bytes();
                if reply != want {
                    wrong.push(format!("{labels:?}: {}", reply.escape_ascii()));
                }
            }
        }
        assert!(clicks > 100, "the premise: buttons were clicked ({clicks})");
        assert!(
            wrong.is_empty(),
            "{} wrong:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// **A click naming a button the notification did not show reports
    /// nothing**: `button-5` on one that showed three is dropped, on macOS's
    /// action and Linux's key alike, where the number was reported as a
    /// button. The control: `button-3` is its third.
    #[test]
    fn a_click_on_a_button_it_did_not_show_reports_nothing() {
        let p = posted(
            "\x1b]99;i=n:a=report:d=0;T\x1b\\\x1b]99;i=n:p=buttons;A\u{2028}B\u{2028}C\x1b\\"
                .as_bytes(),
            &mut Icons::default(),
        );
        let p = &p[0];
        assert_eq!(p.buttons.len(), 3, "the premise: three shown");
        let mut shown: Shown<HostTime, u32> = Shown::new();
        let numbers = p.buttons.iter().map(|(n, _)| *n).collect();
        shown.shown(p.token, 1, p.expiry, at(Secs(0)), numbers);
        let mac = |action: &str| {
            clicked(action, DEFAULT, |n| shown.read_back(p.token, n))
                .map(|b| b.map(NoticeButton::number))
        };
        assert_eq!(mac("button-3"), Some(Some(3)), "the control");
        assert_eq!(mac("button-5"), None, "macOS: a button it did not show");
        let linux = |key: &str| shown.read_back(p.token, key).map(NoticeButton::number);
        assert_eq!(linux("3"), Some(3), "the control");
        assert_eq!(linux("5"), None, "Linux: a button it did not show");
    }

    /// `OSC 9` dropped and the core told, the Dock's bar and an attention
    /// request. Nothing reaches the notification center or the Dock: the
    /// calls are only read.
    #[test]
    fn the_macos_driver_makes_the_calls_the_plan_says() {
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        let mut d: Delivery<HostTime, String> = Delivery::new(Setting::On, &t, PIXELS);
        let png = [0x89, b'P', b'N', b'G', 1, 2];
        let seq = "\x1b]99;i=j:d=0:c=1:u=0:w=3000:s=c2lsZW50;Build\x1b\\\x1b]99;i=j:d=0:p=buttons;Open\u{2028}Skip\x1b\\\x1b]99;i=j:p=icon:e=1;iVBORwEC\x1b\\\x1b]9;4;1;40\x07\x1b]1337;RequestAttention=once\x07";
        feed(&mut t, seq.as_bytes());
        let mut icons = Icons::default();
        let posted: Vec<Posted> = t.notices().map(|n| Posted::of(&n, &mut icons)).collect();
        let token = posted[0].token;
        t.clear_notices();
        let mut replies = Vec::new();
        let calls = d.mac(
            &mut t,
            posted,
            (Focus::Focused, Visible::Seen),
            at(Secs(0)),
            "baud-1",
            &mut NoFiles,
            |b| replies.extend_from_slice(b),
        );
        let id = match &calls[0] {
            MacCall::Post(p) => {
                assert_eq!(p.title, "Build");
                let labels: Vec<&str> = p.buttons.iter().map(|(_, l)| l.as_str()).collect();
                assert_eq!(labels, ["Open", "Skip"]);
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
        // A click read back against the buttons it showed.
        let click = |action: &str| {
            clicked(action, DEFAULT, |n| d.shown.read_back(token, n))
                .map(|b| b.map(NoticeButton::number))
        };
        assert_eq!(click(DEFAULT), Some(None));
        assert_eq!(click("button-2"), Some(Some(2)));
        assert_eq!(click("button-0"), None);
        assert_eq!(click("button-3"), None, "a number it did not show");

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
            &mut NoFiles,
            |_| {},
        );
        assert!(
            matches!(&calls[0], MacCall::Post(p) if p.id == id && p.title == "Again"),
            "{calls:?}"
        );
        assert_eq!(calls[1], MacCall::Remove(id));
        assert_eq!(calls[2], MacCall::Bar(None));
        assert_eq!(calls.len(), 3, "the OSC 9 was dropped");
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
        let mut d: Delivery<HostTime, u32> = Delivery::new(Setting::On, &t, PIXELS);
        d.waiting
            .push((tok, Expiry::After(Duration::from_millis(900)), Vec::new()));
        assert_eq!(d.answered(tok).0, Expiry::After(Duration::from_millis(900)));
        assert_eq!(d.answered(tok).0, Expiry::System, "taken once");
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
        let mut d: Delivery<HostTime, u32> = Delivery::new(Setting::On, &t, PIXELS);
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
        // On Linux the PNG is sent; a GIF and a JPEG that do not decode and
        // a PNG wider than 1024 pixels are not, and are counted, and the
        // log says so once.
        let mut owned = Owned::new(&WS).with_notices(BUDGET);
        let t = Term::new(owned.storage(), &WS);
        let mut d: Delivery<HostTime, u32> = Delivery::new(Setting::On, &t, PIXELS);
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
        assert_eq!(d.image_data(&none, &mut NoFiles), None);
        assert_eq!(d.image_line(), None, "no image, nothing to say");
        assert_eq!(d.image_data(&with(&PNG), &mut NoFiles), Some((size, px)));
        assert_eq!(d.image_line(), None);
        assert_eq!(d.image_data(&with(b"GIF89a\x01\x00"), &mut NoFiles), None);
        assert_eq!(
            d.image_data(&with(&[0xff, 0xd8, 0xff, 0xe0]), &mut NoFiles),
            None
        );
        assert_eq!(d.image_data(&with(&wide), &mut NoFiles), None);
        assert_eq!(d.unshown, Unshown(3));
        assert!(
            d.image_line()
                .expect("said")
                .starts_with("notifications: images not shown, 3 so far")
        );
        assert_eq!(d.image_data(&with(b"GIF87a"), &mut NoFiles), None);
        assert_eq!(d.unshown, Unshown(4));
        assert_eq!(d.image_line(), None, "said once");
        assert_eq!(escape_markup("a <b> & c"), "a &lt;b&gt; &amp; c");
    }

    /// **A JPEG and a GIF are decoded for Linux too**, by baud's own
    /// decoders: the JPEG as the domain's decoder draws it, every pixel
    /// opaque; the GIF's first frame as ImageMagick's reference has it; and
    /// one past 1024 pixels a side is not.
    #[test]
    fn a_jpeg_and_a_gif_decode_for_linux() {
        let jpg = include_bytes!("../../baud-domain/tests/jpeg/small.jpg");
        let (size, px) = rgba(jpg).expect("a JPEG decodes");
        assert_eq!((size.width, size.height, px.len()), (29, 21, 29 * 21 * 4));
        assert!(px.as_chunks::<4>().0.iter().all(|p| p[3] == 255), "opaque");
        let j = baud_domain::jpeg::Jpeg::parse(jpg).expect("parses");
        let mut scratch = vec![0u8; j.scratch_len().expect("a layout")];
        let mut want = vec![baud_domain::Pixel::CLEAR; 29 * 21];
        j.decode(&mut scratch, &mut want).expect("decodes");
        let (r, g, b) = want[0].rgb();
        assert_eq!(px[..3], [r.get(), g.get(), b.get()]);
        let gif = include_bytes!("../../baud-domain/tests/gif/scene2.gif");
        let (size, px) = rgba(gif).expect("a GIF decodes");
        assert_eq!((size.width, size.height), (61, 45));
        assert_eq!(
            px,
            include_bytes!("../../baud-domain/tests/gif/scene2.rgba")
        );
        let mut wide = gif.to_vec();
        wide[6..8].copy_from_slice(&1025u16.to_le_bytes());
        let parsed = baud_domain::gif::Gif::parse(&wide).expect("refused for its size alone");
        assert_eq!(parsed.size().width, 1025);
        assert_eq!(rgba(&wide), None, "past 1024 pixels a side");
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
        let mut d: Delivery<HostTime, String> = Delivery::new(Setting::On, &t, PIXELS);
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
                &mut NoFiles,
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

    /// iTerm2's `Notification` as a test sends it: each part base64'd.
    fn iterm(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let b64 = |d: &[u8]| -> String {
            const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut out = String::new();
            for c in d.chunks(3) {
                let w = (u32::from(c[0]) << 16)
                    | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                    | u32::from(*c.get(2).unwrap_or(&0));
                for i in 0..4 {
                    out.push(match i <= c.len() {
                        true => char::from(
                            A[usize::try_from((w >> (18 - 6 * i)) & 63).expect("six bits")],
                        ),
                        false => '=',
                    });
                }
            }
            out
        };
        let pairs: Vec<String> = parts
            .iter()
            .map(|(k, v)| format!("{k}={}", b64(v)))
            .collect();
        format!("\x1b]1337;Notification={}\x07", pairs.join(";")).into_bytes()
    }

    /// **On macOS iTerm2's subtitle is the post's subtitle and its picture
    /// is read, when the post is made, through the reader kitty's file
    /// medium reads through, and attached**: a PNG file the test owns, at
    /// every check `LocalFiles` makes, posted beside the text; a picture
    /// refused -- under `/dev`, or not a picture the center attaches --
    /// leaves the text posted without it; a subtitle with no title leaves
    /// the title empty, as iTerm2 leaves it; a picture alone posts when it
    /// can be shown and is dropped, and counted, when it cannot; and a
    /// notification the window's focus drops is never read.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn an_iterm2_subtitle_and_picture_reach_the_macos_post() {
        let dir = std::env::temp_dir().join(format!("baud-notify-pic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory");
        let png = dir.join("p.png");
        std::fs::write(&png, PNG_2X1).expect("written");
        let path = std::fs::canonicalize(&png).expect("resolved");
        let path = path.to_str().expect("UTF-8").as_bytes().to_vec();
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        let mut d: Delivery<HostTime, String> = Delivery::new(Setting::On, &t, PIXELS);
        let mut files = crate::kitty_files::LocalFiles::new();
        // Each post two seconds after the last, under the bound's rate.
        let clock = core::cell::Cell::new(0);
        let post = |t: &mut Term<'_>,
                    d: &mut Delivery<HostTime, String>,
                    files: &mut dyn PictureReader,
                    focus: Focus,
                    bytes: &[u8]| {
            feed(t, bytes);
            let posted: Vec<Posted> = t
                .notices()
                .map(|n| Posted::of(&n, &mut Icons::default()))
                .collect();
            t.clear_notices();
            let now = at(Secs(clock.replace(clock.get() + 2)));
            let calls = d.mac(
                t,
                posted,
                (focus, Visible::Covered),
                now,
                "baud-1",
                files,
                |_| {},
            );
            checked_now(d, t, calls, now)
                .into_iter()
                .filter_map(|c| match c {
                    MacCall::Post(p) => Some(p),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let full = iterm(&[
            ("title", b"Build"),
            ("subtitle", b"main"),
            ("message", b"finished in 3 s"),
            ("image", &path),
        ]);
        let p = post(&mut t, &mut d, &mut files, Focus::Unfocused, &full);
        assert_eq!(p.len(), 1);
        assert_eq!(
            (
                p[0].title.as_str(),
                p[0].subtitle.as_str(),
                p[0].body.as_str()
            ),
            ("Build", "main", "finished in 3 s")
        );
        assert_eq!(p[0].image, Some((PNG_2X1.to_vec(), "png")), "the file read");
        // Refused: under `/dev`, and a file that is no picture the center
        // attaches. The text posts without it.
        let txt = dir.join("p.txt");
        std::fs::write(&txt, b"plain text").expect("written");
        for bad in [
            b"/dev/zero".to_vec(),
            txt.to_str().expect("UTF-8").as_bytes().to_vec(),
        ] {
            let msg = iterm(&[("title", b"Build"), ("image", &bad)]);
            let p = post(&mut t, &mut d, &mut files, Focus::Unfocused, &msg);
            assert_eq!(p.len(), 1, "{:?}", String::from_utf8_lossy(&bad));
            assert_eq!((p[0].title.as_str(), &p[0].image), ("Build", &None));
        }
        // A subtitle with no title: no title made up.
        let sub = iterm(&[("subtitle", b"main"), ("message", b"done")]);
        let p = post(&mut t, &mut d, &mut files, Focus::Unfocused, &sub);
        assert_eq!(
            (
                p[0].title.as_str(),
                p[0].subtitle.as_str(),
                p[0].body.as_str()
            ),
            ("", "main", "done")
        );
        // A picture alone posts when it shows, and is dropped, counted,
        // when it does not.
        let alone = iterm(&[("image", &path)]);
        let p = post(&mut t, &mut d, &mut files, Focus::Unfocused, &alone);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].image, Some((PNG_2X1.to_vec(), "png")));
        assert_eq!((p[0].title.as_str(), p[0].body.as_str()), ("", ""));
        let gone = iterm(&[("image", b"/dev/zero")]);
        assert!(post(&mut t, &mut d, &mut files, Focus::Unfocused, &gone).is_empty());
        assert_eq!(d.bare, Unshown(1));
        assert!(
            d.bare_line()
                .expect("said")
                .starts_with("notifications: 1 not posted")
        );
        assert_eq!(d.bare_line(), None, "said once");
        // A focused window drops it, as an `OSC 9`: nothing is read.
        let mut counting = Files {
            asked: Vec::new(),
            bytes: Ok(PNG_2X1.to_vec()),
        };
        assert!(post(&mut t, &mut d, &mut counting, Focus::Focused, &full).is_empty());
        assert!(counting.asked.is_empty(), "read only when shown");
        let p = post(&mut t, &mut d, &mut counting, Focus::Unfocused, &full);
        assert_eq!(counting.asked, core::slice::from_ref(&path));
        assert_eq!(p[0].image, Some((PNG_2X1.to_vec(), "png")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **On macOS iTerm2's picture is decoded before it is attached, as
    /// iTerm2 decodes it, so a file the center would refuse does not cost
    /// the notification its text**: a PNG's signature over bytes that do
    /// not decode, a JPEG whose headers read and whose Huffman table names
    /// a fifth table, which only the whole decode reaches, and a PNG past
    /// the pixels `[images] pixels_mb` allows, are left out and counted,
    /// the text posted; one within them is attached; a picture alone that
    /// does not decode is not posted, and counted as both; the log says so
    /// once.
    #[test]
    fn an_iterm2_picture_that_does_not_decode_is_left_out_on_macos() {
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        // `PNG_2X1`'s two pixels fit the one and not the other.
        let two = baud_domain::ImageBudget::of(2);
        let one = baud_domain::ImageBudget::of(1);
        let mut d: Delivery<HostTime, String> = Delivery::new(Setting::On, &t, two);
        let mut tight: Delivery<HostTime, String> = Delivery::new(Setting::On, &t, one);
        let clock = core::cell::Cell::new(0);
        let post =
            |t: &mut Term<'_>, d: &mut Delivery<HostTime, String>, bytes: &[u8], msg: &[u8]| {
                let mut files = Files {
                    asked: Vec::new(),
                    bytes: Ok(bytes.to_vec()),
                };
                feed(t, msg);
                let posted: Vec<Posted> = t
                    .notices()
                    .map(|n| Posted::of(&n, &mut Icons::default()))
                    .collect();
                t.clear_notices();
                let now = at(Secs(clock.replace(clock.get() + 2)));
                let calls = d.mac(
                    t,
                    posted,
                    (Focus::Unfocused, Visible::Covered),
                    now,
                    "baud-1",
                    &mut files,
                    |_| {},
                );
                let p = checked_now(d, t, calls, now)
                    .into_iter()
                    .filter_map(|c| match c {
                        MacCall::Post(p) => Some(p),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(files.asked.len(), 1, "read once");
                p
            };
        let msg = iterm(&[("title", b"Build"), ("image", b"/tmp/p.png")]);
        let mut garbage = PNG_2X1[..16].to_vec();
        garbage.extend([0x5a; 40]);
        assert_eq!(image_kind(&garbage), Some("png"), "a PNG by its signature");
        let p = post(&mut t, &mut d, &garbage, &msg);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].title.as_str(), &p[0].image), ("Build", &None));
        assert_eq!((d.unshown, d.bare), (Unshown(1), Unshown(0)));
        // `small.jpg`'s first `DHT` (at 177) as table 4: T.81 numbers
        // them 0 to 3 (B.2.4.2), and the headers up to the first scan
        // never read it.
        let jpg = include_bytes!("../../baud-domain/tests/jpeg/small.jpg");
        assert_eq!(attaches(jpg, PIXELS), Some("jpg"), "the control");
        let mut table = jpg.to_vec();
        assert_eq!((table[177], table[178], table[181]), (0xff, 0xc4, 0x00));
        table[181] = 0x04;
        assert!(
            baud_domain::jpeg::Jpeg::parse(&table).is_ok(),
            "its headers read"
        );
        let mut roomy: Delivery<HostTime, String> = Delivery::new(Setting::On, &t, PIXELS);
        let jmsg = iterm(&[("title", b"Photo"), ("image", b"/tmp/p.jpg")]);
        let p = post(&mut t, &mut roomy, &table, &jmsg);
        assert_eq!(
            (p.len(), p[0].title.as_str(), &p[0].image),
            (1, "Photo", &None)
        );
        assert_eq!((roomy.unshown, roomy.bare), (Unshown(1), Unshown(0)));
        let p = post(&mut t, &mut roomy, jpg, &jmsg);
        assert_eq!(
            p[0].image,
            Some((jpg.to_vec(), "jpg")),
            "the control posts it"
        );
        let p = post(&mut t, &mut d, &PNG_2X1, &msg);
        assert_eq!(p[0].image, Some((PNG_2X1.to_vec(), "png")), "within");
        assert_eq!(d.unshown, Unshown(1));
        let p = post(&mut t, &mut tight, &PNG_2X1, &msg);
        assert_eq!((p.len(), &p[0].image), (1, &None), "past the pixels");
        assert_eq!((tight.unshown, tight.bare), (Unshown(1), Unshown(0)));
        // A picture alone that does not decode has nothing left to show.
        let alone = iterm(&[("image", b"/tmp/p.png")]);
        assert!(post(&mut t, &mut d, &garbage, &alone).is_empty());
        assert_eq!((d.unshown, d.bare), (Unshown(2), Unshown(1)));
        assert!(
            d.image_line()
                .expect("said")
                .starts_with("notifications: images not shown, 2 so far")
        );
        assert_eq!(d.image_line(), None, "said once");
    }

    /// **On macOS a picture is checked before its notification posts, and
    /// nothing is decoded where the notices are delivered**: the read's
    /// picture comes back as a check, with its bytes and the pixels
    /// `[images] pixels_mb` allows, its notification neither posted nor
    /// shown; one with nothing to check, after it in the same read, posts
    /// at once; the check's answer posts it with its picture, once, and
    /// one that does not decode posts its text alone.
    #[test]
    fn a_picture_is_checked_before_its_notification_posts() {
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        let mut d: Delivery<HostTime, String> = Delivery::new(Setting::On, &t, PIXELS);
        let mut files = Files {
            asked: Vec::new(),
            bytes: Ok(PNG_2X1.to_vec()),
        };
        feed(
            &mut t,
            &iterm(&[("title", b"Build"), ("image", b"/tmp/p.png")]),
        );
        feed(&mut t, &iterm(&[("title", b"Later")]));
        let posted: Vec<Posted> = t
            .notices()
            .map(|n| Posted::of(&n, &mut Icons::default()))
            .collect();
        t.clear_notices();
        let calls = d.mac(
            &mut t,
            posted,
            (Focus::Unfocused, Visible::Covered),
            at(Secs(0)),
            "baud-1",
            &mut files,
            |_| {},
        );
        let checks: Vec<Check> = calls
            .iter()
            .filter_map(|c| match c {
                MacCall::Check(c) => Some(c.clone()),
                _ => None,
            })
            .collect();
        let titles: Vec<&str> = calls
            .iter()
            .filter_map(|c| match c {
                MacCall::Post(p) => Some(p.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(checks.len(), 1, "{calls:?}");
        assert_eq!(
            (&checks[0].bytes[..], checks[0].pixels),
            (&PNG_2X1[..], PIXELS)
        );
        assert_eq!(titles, ["Later"], "the one with nothing to check, at once");
        let token = checks[0].token;
        assert_eq!(d.shown.id(token), None, "not shown while it is checked");
        let back = Checked {
            token,
            bytes: PNG_2X1.to_vec(),
            kind: attaches(&PNG_2X1, PIXELS),
        };
        let calls = d.mac_checked(&mut t, back.clone(), at(Secs(1)), "baud-1", |_| {});
        let [MacCall::Post(p)] = &calls[..] else {
            panic!("one post: {calls:?}");
        };
        assert_eq!(p.title, "Build");
        assert_eq!(p.image, Some((PNG_2X1.to_vec(), "png")));
        assert_eq!(d.shown.id(token).as_deref(), Some(p.id.as_str()));
        assert!(
            d.mac_checked(&mut t, back, at(Secs(1)), "baud-1", |_| {})
                .is_empty(),
            "posted once"
        );
        // One that does not decode: the text alone, counted.
        feed(
            &mut t,
            &iterm(&[("title", b"Again"), ("image", b"/tmp/p.png")]),
        );
        let posted: Vec<Posted> = t
            .notices()
            .map(|n| Posted::of(&n, &mut Icons::default()))
            .collect();
        t.clear_notices();
        let calls = d.mac(
            &mut t,
            posted,
            (Focus::Unfocused, Visible::Covered),
            at(Secs(2)),
            "baud-1",
            &mut files,
            |_| {},
        );
        let [MacCall::Check(c)] = &calls[..] else {
            panic!("one check: {calls:?}");
        };
        let back = Checked {
            token: c.token,
            bytes: c.bytes.clone(),
            kind: None,
        };
        let calls = d.mac_checked(&mut t, back, at(Secs(3)), "baud-1", |_| {});
        let [MacCall::Post(p)] = &calls[..] else {
            panic!("one post: {calls:?}");
        };
        assert_eq!((p.title.as_str(), &p.image), ("Again", &None));
        assert_eq!(d.unshown, Unshown(1));
    }

    /// **A window that closes while a picture is checked still posts its
    /// notification**: the one whose check came back before the close
    /// posts with its picture, first; one still out posts its text alone,
    /// its picture counted; a picture alone still out is not posted, and
    /// counted; nothing is left waiting, a second close posts nothing, and
    /// an answer that comes back after the close posts nothing again. The
    /// control: while they are parked the delivery says a check is out.
    #[test]
    fn a_closing_window_posts_what_it_was_checking() {
        let mut owned = Owned::new(&WS).with_notices(NoticeBudget::with_icon(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        let mut d: Delivery<HostTime, String> = Delivery::new(Setting::On, &t, PIXELS);
        let mut files = Files {
            asked: Vec::new(),
            bytes: Ok(PNG_2X1.to_vec()),
        };
        assert_eq!(d.checks(), Checks::Done, "nothing out yet");
        for msg in [
            iterm(&[("title", b"Back"), ("image", b"/tmp/p.png")]),
            iterm(&[("title", b"Out"), ("image", b"/tmp/p.png")]),
            iterm(&[("image", b"/tmp/p.png")]),
        ] {
            feed(&mut t, &msg);
        }
        let posted: Vec<Posted> = t
            .notices()
            .map(|n| Posted::of(&n, &mut Icons::default()))
            .collect();
        t.clear_notices();
        let calls = d.mac(
            &mut t,
            posted,
            (Focus::Unfocused, Visible::Covered),
            at(Secs(0)),
            "baud-1",
            &mut files,
            |_| {},
        );
        let checks: Vec<Check> = calls
            .iter()
            .filter_map(|c| match c {
                MacCall::Check(c) => Some(c.clone()),
                _ => None,
            })
            .collect();
        assert_eq!((calls.len(), checks.len()), (3, 3), "{calls:?}");
        assert_eq!(d.checks(), Checks::Waiting, "the control");
        let back = |c: &Check| Checked {
            token: c.token,
            bytes: c.bytes.clone(),
            kind: attaches(&c.bytes, c.pixels),
        };
        // The first answer came back; its wake had not.
        let calls = d.mac_closing(
            &mut t,
            vec![back(&checks[0])],
            at(Secs(1)),
            "baud-1",
            |_| {},
        );
        let posts: Vec<&MacPost> = calls
            .iter()
            .filter_map(|c| match c {
                MacCall::Post(p) => Some(p),
                _ => None,
            })
            .collect();
        let [back_post, out_post] = &posts[..] else {
            panic!("two posts: {calls:?}");
        };
        assert_eq!(
            (back_post.title.as_str(), &back_post.image),
            ("Back", &Some((PNG_2X1.to_vec(), "png")))
        );
        assert_eq!((out_post.title.as_str(), &out_post.image), ("Out", &None));
        assert_eq!((d.unshown, d.bare), (Unshown(2), Unshown(1)));
        assert!(d.shown.id(checks[0].token).is_some());
        assert!(d.shown.id(checks[1].token).is_some());
        assert_eq!(d.checks(), Checks::Done, "nothing left waiting");
        assert!(
            d.image_line()
                .expect("said")
                .starts_with("notifications: images not shown, 2 so far")
        );
        assert!(d.bare_line().is_some());
        assert!(
            d.mac_closing(&mut t, Vec::new(), at(Secs(2)), "baud-1", |_| {})
                .is_empty(),
            "a second close"
        );
        assert!(
            d.mac_checked(&mut t, back(&checks[1]), at(Secs(2)), "baud-1", |_| {})
                .is_empty(),
            "posted once"
        );
    }

    /// **A window's close loses no notification still being checked**:
    /// the close a shell's exit asks for waits, as it waits for the log,
    /// until no picture is being checked, so the turn the checker's wake
    /// brings posts it first; and the driver's drop, which every close
    /// reaches, posts what is still out through the calls
    /// `deliver_notices` makes, touching nothing of the view, whose
    /// responder is being replaced. The drivers' own code is read, since
    /// no test runs them.
    #[test]
    fn a_window_closes_after_its_pictures_are_checked() {
        let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let mac = bare(include_str!("app.rs"));
        let between = |from: &str, to: &str| -> String {
            mac.split_once(from)
                .and_then(|(_, rest)| rest.split_once(to))
                .map(|(b, _)| b.to_owned())
                .expect(from)
        };
        // The close is `Driver::ended`'s, which closes or holds (`held.rs`).
        let exit = between("ifdrained.stream==Stream::Closed{", "self.ended();");
        assert!(
            exit.ends_with(
                "==crate::hostlog::Finished::Yes&&self.app.notes.checks()==crate::notify::Checks::Done{"
            ),
            "the exit's close waits for the log and the checks: {exit}"
        );
        assert!(
            mac.contains("implDropforDriver{fndrop(&mutself){self.notices_at_close();}}"),
            "the drop posts what is out"
        );
        let at_close = between("fnnotices_at_close(&mutself){", "fnnotice_calls(");
        assert!(
            at_close.contains(
                "letcalls=self.app.notes.mac_closing(&mutself.app.term,self.app.checker.finished(),"
            ) && at_close.contains("self.notice_calls(calls);"),
            "what came back and what is out, posted: {at_close}"
        );
        let deliver = between("fndeliver_notices(", "fnnotices_at_close(");
        assert!(
            deliver.contains("self.notice_calls(calls);"),
            "the same calls"
        );
        let calls = between("fnnotice_calls(", "fnnotice_turn(");
        assert!(
            calls.contains(
                "MacCall::Post(p)=>{ifletSome(c)=&self.app.center{c.post(mac_post(p));}}"
            ),
            "a post reaches the center"
        );
        for part in [&at_close, &calls] {
            assert!(!part.contains("self.view"), "the view is not touched");
        }
    }

    /// **The checker checks a picture on a thread of its own and the
    /// hand-over returns first**: a check held at a gate has sent nothing
    /// back when `start` returns, and once the gate opens the worker sends
    /// the picture back, its bytes unchanged, and wakes the window; the
    /// next picture goes to the same worker, in order. The check never
    /// runs on the thread that handed it over.
    #[test]
    fn the_checker_returns_before_a_slow_check_is_done() {
        use std::sync::{Arc, Mutex, mpsc};
        let mut icons = Icons::default();
        let token = posted(&iterm(&[("title", b"Build")]), &mut icons)[0].token;
        let caller = std::thread::current().id();
        let (open, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let (woke, wakes) = mpsc::channel::<()>();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&ran);
        let mut c = Checker::new(
            move || {
                let _ = woke.send(());
            },
            move |b, px| {
                let here = std::thread::current().id();
                seen.lock().expect("the list").push(here);
                // Held only off the caller, so a check on it fails the
                // test rather than hanging it.
                if here != caller {
                    let _ = gate.lock().map(|g| g.recv());
                }
                attaches(b, px)
            },
        );
        let wait = || {
            wakes
                .recv_timeout(Duration::from_secs(60))
                .expect("the worker woke the window")
        };
        c.start(Check {
            token,
            bytes: PNG_2X1.to_vec(),
            pixels: PIXELS,
        });
        assert_eq!(c.finished(), [], "nothing back while the check is held");
        open.send(()).expect("the gate");
        wait();
        let back = Checked {
            token,
            bytes: PNG_2X1.to_vec(),
            kind: Some("png"),
        };
        assert_eq!(c.finished(), [back]);
        c.start(Check {
            token,
            bytes: b"plain text".to_vec(),
            pixels: PIXELS,
        });
        open.send(()).expect("the gate");
        wait();
        let back = Checked {
            token,
            bytes: b"plain text".to_vec(),
            kind: None,
        };
        assert_eq!(c.finished(), [back]);
        let ran = ran.lock().expect("the list");
        assert_eq!(ran.len(), 2);
        assert!(ran.iter().all(|t| *t != caller), "never on the caller");
        assert_eq!(ran[0], ran[1], "one worker");
    }

    /// **A check that panics leaves the picture out and the worker
    /// goes on**: the picture comes back with no extension, so its
    /// notification posts its text, and the next picture is checked by
    /// the same worker. The control: the next one attaches.
    #[test]
    fn a_check_that_panics_leaves_the_picture_out() {
        use std::sync::mpsc;
        let mut icons = Icons::default();
        let token = posted(&iterm(&[("title", b"Build")]), &mut icons)[0].token;
        let (woke, wakes) = mpsc::channel::<()>();
        let mut c = Checker::new(
            move || {
                let _ = woke.send(());
            },
            |b, px| {
                assert!(b != b"panic", "a decoder's panic");
                attaches(b, px)
            },
        );
        let wait = || {
            wakes
                .recv_timeout(Duration::from_secs(60))
                .expect("the worker woke the window")
        };
        for (bytes, kind) in [(&b"panic"[..], None), (&PNG_2X1[..], Some("png"))] {
            c.start(Check {
                token,
                bytes: bytes.to_vec(),
                pixels: PIXELS,
            });
            wait();
            let back = Checked {
                token,
                bytes: bytes.to_vec(),
                kind,
            };
            assert_eq!(c.finished(), [back]);
        }
    }

    /// **A picture is checked holding the decoder's work and no RGBA copy**:
    /// what each format's check holds at once, counted by the allocator,
    /// is a PNG's inflated scanlines, a GIF's two canvases, and a JPEG's
    /// scratch and the decoder's own pixels; the RGBA Linux's
    /// `image-data` is made of ([`rgba`]) is never made. The control: the
    /// RGBA decode holds the scanlines and the RGBA both. The allocator's
    /// count is the macOS app's (`heap.rs`).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_picture_is_checked_holding_no_rgba() {
        use crate::heap::peak_during;
        use baud_domain::{gif::Gif, jpeg::Jpeg, png::Png};
        let all = baud_domain::ImageBudget::of(usize::MAX);
        let bytes = |n: usize| isize::try_from(n).expect("small");
        let png = Png::parse(&PNG_2X1).expect("parses");
        let (raw, px) = (png.raw_len().expect("raw"), png.rgba_len().expect("rgba"));
        assert_eq!(
            peak_during(|| attaches(&PNG_2X1, all)),
            (bytes(raw), Some("png"))
        );
        let (held, made) = peak_during(|| rgba(&PNG_2X1));
        assert_eq!(
            (held, made.map(|m| m.1.len())),
            (bytes(raw + px), Some(px)),
            "the control"
        );
        let jpg = include_bytes!("../../baud-domain/tests/jpeg/small.jpg");
        let j = Jpeg::parse(jpg).expect("parses");
        let pixels = 29 * 21 * core::mem::size_of::<baud_domain::Pixel>();
        let work = j.scratch_len().expect("a layout") + pixels;
        assert_eq!(
            peak_during(|| attaches(jpg, all)),
            (bytes(work), Some("jpg"))
        );
        let gif = include_bytes!("../../baud-domain/tests/gif/scene2.gif");
        let canvases = Gif::parse(gif).expect("parses").scratch_len();
        assert_eq!(
            peak_during(|| attaches(gif, all)),
            (bytes(canvases), Some("gif"))
        );
    }

    /// **Both drivers act on what [`Delivery`] decides and log what it
    /// counts**: Linux's arm for a picture alone comes before the one that
    /// posts, drops it and counts it, and each driver logs the
    /// notifications not posted and the images not shown, the lines said
    /// once; on macOS iTerm2's picture goes to the window's [`Checker`],
    /// what it sends back is posted, and the driver decodes nothing. The
    /// drivers' own code is read, since no test runs them.
    #[test]
    fn the_drivers_drop_and_log_what_delivery_counts() {
        let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let linux = bare(include_str!("linux.rs"));
        const DROP: &str = r#"
            Plan::Show if crate::notify::untitled(&p) => {
                self.notes.bare.0 += 1;
                self.term.notice_dropped(p.token);
            }
        "#;
        let dropped = linux.find(&bare(DROP)).expect("the drop arm, as written");
        let posts = linux
            .find("Plan::Show=>matchself.bus.as_mut()")
            .expect("the arm that posts");
        assert!(dropped < posts, "the drop arm is tried first");
        for line in ["bare_line()", "image_line()"] {
            assert!(
                linux.contains(&format!(
                    "ifletSome(line)=self.notes.{line}{{crate::warn!(\"{{line}}\");}}"
                )),
                "Linux logs {line}"
            );
        }
        let mac = bare(include_str!("app.rs"));
        let deliver = mac
            .split_once("fndeliver_notices(")
            .and_then(|(_, rest)| rest.split_once("fnnotice_turn("))
            .map(|(b, _)| b)
            .expect("the macOS delivery");
        for line in ["bare_line()", "image_line()"] {
            assert!(
                deliver.contains(&format!(
                    "ifletSome(line)=self.app.notes.{line}{{crate::warn!(\"{{line}}\");}}"
                )),
                "macOS logs {line}"
            );
        }
        // iTerm2's picture goes to the checker and comes back from it, and
        // nothing on the main thread decodes one.
        assert!(
            mac.contains(
                "checker:crate::notify::Checker::new(move||waker.wake(),crate::notify::attaches),"
            ),
            "the checker checks with `attaches` and wakes the window"
        );
        assert!(
            deliver.contains("MacCall::Check(c)=>self.app.checker.start(c),"),
            "a check is handed to the checker"
        );
        assert!(
            deliver.contains(
                "forcheckedinself.app.checker.finished(){calls.extend(self.app.notes.mac_checked("
            ),
            "what it sends back is posted"
        );
        for decode in ["attaches(", "decodes_within(", "rgba_within(", "rgba("] {
            assert!(!mac.contains(decode), "app.rs calls {decode}");
        }
        // Every turn of the loop delivers, not the display link's tick
        // alone: the checker's wake is a turn, and a covered window's link
        // is paused.
        let pump = mac
            .split_once("fnpump(&mutself,trigger:Trigger){")
            .and_then(|(_, rest)| rest.split_once("fnkey(&mutself,key:&KeyPress)->Handled{"))
            .map(|(b, _)| b)
            .expect("the macOS loop's turn");
        // How deep in the turn's blocks a statement sits: 0 is under no
        // condition at all.
        let depth = |stmt: &str| -> i32 {
            let at = pump.find(stmt).expect(stmt);
            pump[..at]
                .chars()
                .map(|c| match c {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                })
                .sum()
        };
        assert_eq!(depth("self.notice_turn();"), 1, "the control: on the tick");
        assert_eq!(
            depth("self.deliver_notices(posted);"),
            0,
            "delivered every turn"
        );
    }

    /// **On Linux the picture is read when it is posted and sent as
    /// `image-data`, as a program's icon is**, the icon a program sent
    /// first and then nothing read; and one with neither title nor
    /// subtitle -- a picture alone -- has no summary to give `Notify`
    /// ([`untitled`]), the test the driver's arm that drops it reads
    /// (`the_drivers_drop_and_log_what_delivery_counts`).
    #[test]
    fn an_iterm2_picture_is_read_for_linux_and_a_bare_one_is_not_posted() {
        let mut owned = Owned::new(&WS).with_notices(BUDGET);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_notice_support(support(Setting::On, Platform::MacOs));
        let mut d: Delivery<HostTime, u32> = Delivery::new(Setting::On, &t, PIXELS);
        let mut files = Files {
            asked: Vec::new(),
            bytes: Ok(PNG_2X1.to_vec()),
        };
        feed(
            &mut t,
            &iterm(&[("title", b"Build"), ("image", b"/tmp/p.png")]),
        );
        let p = t
            .notices()
            .map(|n| Posted::of(&n, &mut Icons::default()))
            .next()
            .expect("posted");
        assert!(!untitled(&p));
        let (size, px) = d.image_data(&p, &mut files).expect("decoded");
        assert_eq!((size.width, size.height, px.len()), (2, 1, 8));
        assert_eq!(files.asked, [b"/tmp/p.png".to_vec()]);
        let mut icon = p.clone();
        icon.icon = Some(PNG_2X1.to_vec());
        assert!(d.image_data(&icon, &mut files).is_some());
        assert_eq!(files.asked.len(), 1, "an icon sent is used, nothing read");
        let mut bare = p.clone();
        bare.title.clear();
        assert!(untitled(&bare), "no summary for Notify");
        bare.subtitle = "main".into();
        assert!(!untitled(&bare), "the subtitle stands as the summary");
    }
}
