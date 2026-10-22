//! **Notifications and progress from programs**: `OSC 9`, `OSC 777 ;
//! notify`, kitty's `OSC 99` and ConEmu's `OSC 9 ; 4` progress, and the one
//! `OSC 1337` subcommand of the same class, `RequestAttention`
//! (`docs/research/notes/ax-program-notifications.md`).
//!
//! * **`OSC 9 ; text`** is iTerm2's "Post a notification" (`[[source:
//!   iterm2-escape-codes]]`). **`OSC 9 ; n ; ...`** is ConEmu's family
//!   (`[[source: conemu-ansi-escape-codes]]`), which shares the number: a
//!   body in one of ConEmu's forms is ConEmu's, `9;4` progress built and the
//!   rest refused ([`ConEmuOp`]), and any other body is iTerm2's text (Jev's
//!   lean, 0.83 and 0.91). Bare `9;4` clears the bar, iTerm2's "abbreviated
//!   form".
//! * **`OSC 777 ; notify ; title ; body`** is the form Terminology and VTE's
//!   patch defined (`[[source: vte-notify-patch]]`); rxvt-unicode itself
//!   defines `OSC 777` only as a call into a perl extension (`[[source:
//!   urxvt-pm]]`). The body is everything after the title's `;`, and a title
//!   alone is a notification (Jev's lean, 0.85 and 0.67).
//! * **`OSC 99 ; metadata ; payload`** is kitty's protocol (`[[source:
//!   kitty-desktop-notifications]]`), chunked, with ids, a query, actions and
//!   close events: [`NoticeMeta`] reads a chunk's keys and [`NoticeDesk`]
//!   assembles, publishes and answers.
//!
//! Everything here is attacker-supplied, so every buffer is bounded and
//! every id is sanitised before it is echoed: "Terminals **must** sanitize
//! ids received from client programs before sending them back in
//! responses".

use crate::*;

/// The most of a title or a body kept: kitty keeps 1024 characters of each
/// (`limit_size`, `[[source: kitty-notifications-py]]`), and this keeps 1024
/// bytes, cut at a character's start.
pub const NOTICE_TEXT_MAX: usize = 1024;
/// The buttons' text, U+2028-separated as the payload sends it: kitty keeps
/// eight of at most 256 characters.
pub const NOTICE_BUTTONS_MAX: usize = 2048;
/// The most buttons a notification shows.
pub const NOTICE_BUTTONS: usize = 8;
/// The longest id kept. An id is echoed in every reply, so one longer than
/// this is refused rather than cut: a cut id names a different notification.
pub const NOTICE_ID_MAX: usize = 128;
/// An application name, a type, an icon name or a sound name, decoded.
pub const NOTICE_NAME_MAX: usize = 64;
/// How many icon names a notification keeps (`n` "can be specified multiple
/// times").
pub const NOTICE_ICON_NAMES: usize = 4;
/// One chunk's payload, decoded: "The size of the payload must be no longer
/// than 2048 bytes, *before being encoded* or 4096 encoded bytes".
pub const NOTICE_CHUNK_MAX: usize = 4096;
/// How many notifications are remembered as alive, for updates, closes,
/// `p=alive` and the reports. kitty keeps 128.
pub const NOTICE_LIVE: usize = 32;
/// How many notifications one read may publish before the embedder takes
/// them; the rest are dropped and counted.
pub const NOTICE_QUEUE: usize = 8;

/// The strings a notification carries besides its text: id, application
/// name, type, the icon names, the icon key and the sound.
const STRINGS: usize = NOTICE_ID_MAX * 2 + NOTICE_NAME_MAX * (3 + NOTICE_ICON_NAMES);
/// One published notification's strings, its icon aside.
const RECORD: usize = NOTICE_TEXT_MAX * 2 + NOTICE_BUTTONS_MAX + STRINGS;

// ---------------------------------------------------------------- OSC 9

/// What an `OSC 9` body is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc9<'a> {
    /// iTerm2's notification: the whole body is the message.
    Notify(&'a [u8]),
    /// ConEmu's `9 ; 4 ; st ; pr`, or iTerm2's bare `9 ; 4`.
    Progress(ProgressReport),
    /// `9 ; 4` with a state no text defines, or a progress that is not a
    /// number: dropped and counted, as Windows Terminal drops it (Jev, 0.98
    /// and 0.96).
    ProgressMalformed,
    /// One of ConEmu's other forms, refused and counted.
    ConEmu(ConEmuOp),
}

/// ConEmu's `OSC 9` forms other than progress (`[[source:
/// conemu-ansi-escape-codes]]`, "ConEmu specific OSC"). None is built:
/// `9;1` sleeps the console and `9;5` waits for a key, `9;2` opens a modal
/// box, `9;6` runs a GuiMacro, `9;7` runs a process and `9;8` writes an
/// environment variable's value back -- each unsafe or meaningless from a
/// remote program; `9;3` and `9;9` are the tab title and the directory,
/// which `OSC 0`-`2` and `OSC 7` carry; `9;10` switches ConEmu's own
/// emulation; `9;11` is "Just a 'comment', skip it"; and `9;12` marks a
/// prompt, which `OSC 133` does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConEmuOp {
    Sleep,
    MessageBox,
    TabTitle,
    WaitInput,
    GuiMacro,
    RunProcess,
    Environment,
    WorkingDirectory,
    XtermEmulation,
    Comment,
    PromptStart,
}

/// What a progress report sets: ConEmu's states 0-4, "0: remove progress
/// ... 1: set progress value to pr ... 2: set error state ... pr is
/// optional ... 3: set indeterminate state ... 4: set paused state, pr is
/// optional". Windows Terminal and iTerm2 3.6 name 4 "Warning"; iTerm2 3.7
/// names it paused, after ConEmu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgressState {
    Hidden,
    Normal,
    Error,
    Indeterminate,
    Paused,
}

/// A percentage, 0-100: a larger number is clamped to 100, as Windows
/// Terminal and ghostty clamp it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Percent(u8);

impl Percent {
    pub const ZERO: Percent = Percent(0);
    pub const FULL: Percent = Percent(100);

    /// The percentage `n` names, clamped.
    pub fn clamped(n: u32) -> Percent {
        Percent(n.min(100) as u8)
    }

    /// As a fraction of one.
    pub fn fraction(self) -> f64 {
        f64::from(self.0) / 100.0
    }

    /// The number.
    pub fn get(self) -> u8 {
        self.0
    }
}

/// One `OSC 9 ; 4` as sent: the state, and the percentage if one came.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProgressReport {
    pub state: ProgressState,
    pub percent: Option<Percent>,
}

/// The progress a terminal shows: hidden, or a state with a percentage
/// where the state has one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Progress {
    pub state: ProgressState,
    /// The percentage shown; `None` for an indeterminate bar, an error
    /// sent with none and a hidden bar.
    pub percent: Option<Percent>,
}

impl Progress {
    /// No bar.
    pub const HIDDEN: Progress = Progress {
        state: ProgressState::Hidden,
        percent: None,
    };

    /// What a report makes of the bar shown. State 1 with no percentage
    /// sets 0, as Windows Terminal and ghostty do (ConEmu's "set progress
    /// value to pr" gives none); state 2 with none is an error "without a
    /// specific percentage" and state 4 with none "the current percentage
    /// is kept and simply recolored" (`[[source: iterm2-escape-codes]]`,
    /// 3.7); state 3 "ignores the progress value".
    pub fn after(self, r: ProgressReport) -> Progress {
        let percent = match r.state {
            ProgressState::Hidden | ProgressState::Indeterminate => None,
            ProgressState::Normal => Some(r.percent.unwrap_or(Percent::ZERO)),
            ProgressState::Error => r.percent,
            ProgressState::Paused => r.percent.or(self.percent),
        };
        Progress {
            state: r.state,
            percent,
        }
    }
}

impl<'a> Osc9<'a> {
    /// The body after `9;`.
    pub fn parse(pt: &'a [u8]) -> Osc9<'a> {
        let (first, rest) = match pt.iter().position(|&b| b == b';') {
            Some(i) => (&pt[..i], Some(&pt[i + 1..])),
            None => (pt, None),
        };
        // ConEmu's table, each form spelled as it is there: a number, then
        // its own `;` where the form takes one, or nothing where it is bare.
        let op = match (first, rest.is_some()) {
            (b"4", _) => return Osc9::progress(rest),
            (b"1", true) => ConEmuOp::Sleep,
            (b"2", true) => ConEmuOp::MessageBox,
            (b"3", true) => ConEmuOp::TabTitle,
            (b"5", false) => ConEmuOp::WaitInput,
            (b"6", true) => ConEmuOp::GuiMacro,
            (b"7", true) => ConEmuOp::RunProcess,
            (b"8", true) => ConEmuOp::Environment,
            (b"9", true) => ConEmuOp::WorkingDirectory,
            (b"10", _) => ConEmuOp::XtermEmulation,
            (b"11", true) => ConEmuOp::Comment,
            (b"12", false) => ConEmuOp::PromptStart,
            _ => return Osc9::Notify(pt),
        };
        Osc9::ConEmu(op)
    }

    /// `4`, then `; st` and `; pr`, each optional.
    fn progress(rest: Option<&[u8]>) -> Osc9<'a> {
        let Some(rest) = rest else {
            return Osc9::Progress(ProgressReport {
                state: ProgressState::Hidden,
                percent: None,
            });
        };
        let (st, pr) = match rest.iter().position(|&b| b == b';') {
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
            None => (rest, None),
        };
        let state = match st {
            b"" | b"0" => ProgressState::Hidden,
            b"1" => ProgressState::Normal,
            b"2" => ProgressState::Error,
            b"3" => ProgressState::Indeterminate,
            b"4" => ProgressState::Paused,
            _ => return Osc9::ProgressMalformed,
        };
        let percent = match pr {
            None | Some(b"") => None,
            Some(p) => match decimal(p) {
                Some(n) => Some(Percent::clamped(n)),
                None => return Osc9::ProgressMalformed,
            },
        };
        Osc9::Progress(ProgressReport { state, percent })
    }
}

/// A run of ASCII digits as a number, saturating; `None` for anything else.
fn decimal(s: &[u8]) -> Option<u32> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(s.iter().fold(0u32, |v, &d| {
        v.saturating_mul(10).saturating_add(u32::from(d - b'0'))
    }))
}

// ---------------------------------------------------------------- OSC 777

/// What an `OSC 777` body is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc777<'a> {
    /// `notify ; title [; body]`.
    Notify { title: &'a [u8], body: &'a [u8] },
    /// `notify` with no title.
    Malformed,
    /// Any other prefix: a perl extension's, which baud does not run.
    Extension,
}

impl<'a> Osc777<'a> {
    /// The body after `777;`.
    pub fn parse(pt: &'a [u8]) -> Osc777<'a> {
        let Some(rest) = pt.strip_prefix(b"notify") else {
            return Osc777::Extension;
        };
        let Some(rest) = rest.strip_prefix(b";") else {
            return match rest.is_empty() {
                true => Osc777::Malformed,
                false => Osc777::Extension,
            };
        };
        let (title, body) = match rest.iter().position(|&b| b == b';') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        Osc777::Notify { title, body }
    }
}

// ---------------------------------------------------------------- OSC 1337

/// iTerm2's `RequestAttention` values (`[[source: iterm2-escape-codes]]`,
/// "Attention"): "`yes` to request attention by bouncing the dock icon
/// indefinitely, `once` to bounce it a single time, or `no` to cancel a
/// previous request".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attention {
    Yes,
    Once,
    No,
}

/// What an `OSC 1337` body is, as far as baud reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc1337 {
    Attention(Attention),
    /// `RequestAttention=fireworks`: "fireworks explode at the cursor's
    /// location", a drawn effect not built, counted.
    Fireworks,
    /// `RequestAttention` with a value the text does not define.
    AttentionMalformed,
    /// Every other subcommand: each has its row in `docs/vt-coverage.md`,
    /// and none is built yet. Counted.
    Other,
}

impl Osc1337 {
    /// The body after `1337;`.
    pub fn parse(pt: &[u8]) -> Osc1337 {
        match pt.strip_prefix(b"RequestAttention=") {
            Some(b"yes") => Osc1337::Attention(Attention::Yes),
            Some(b"once") => Osc1337::Attention(Attention::Once),
            Some(b"no") => Osc1337::Attention(Attention::No),
            Some(b"fireworks") => Osc1337::Fireworks,
            Some(_) => Osc1337::AttentionMalformed,
            None => Osc1337::Other,
        }
    }
}

// ---------------------------------------------------------------- OSC 99

/// What a chunk's payload is: `p`, "One of title, body, close, icon, ?,
/// alive, buttons", default `title`; any other value is a payload "of
/// unknown type", which "Terminal emulators should ignore".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Payload {
    Title,
    Body,
    Close,
    Icon,
    Query,
    Alive,
    Buttons,
    Unknown,
}

impl Payload {
    fn of(v: &[u8]) -> Payload {
        match v {
            b"title" => Payload::Title,
            b"body" => Payload::Body,
            b"close" => Payload::Close,
            b"icon" => Payload::Icon,
            b"?" => Payload::Query,
            b"alive" => Payload::Alive,
            b"buttons" => Payload::Buttons,
            _ => Payload::Unknown,
        }
    }
}

/// `o`: "When to honor the notification request". `always` is the default
/// "and always honors the request"; `unfocused` "when the window ... does
/// not have keyboard focus"; `invisible` "the window both is unfocused and
/// not visible to the user".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Occasion {
    Always,
    Unfocused,
    Invisible,
}

/// `u`: "0 is low, 1 is normal and 2 is critical. If not specified normal is
/// used."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

/// `w`: "-1 which means to use whatever expiry policy the OS has ... 0 means
/// the notification should never expire ... greater than zero specify the
/// number of milliseconds after which the notification should be
/// auto-closed".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expiry {
    System,
    Never,
    After(core::time::Duration),
}

/// What a click on the notification does (`a`): focus the window, report
/// to the program, both or neither. The default is `focus`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Activation(u8);

impl Activation {
    pub const NONE: Activation = Activation(0);
    pub const FOCUS: Activation = Activation(1);
    pub const REPORT: Activation = Activation(2);

    pub fn focuses(self) -> bool {
        self.0 & Activation::FOCUS.0 != 0
    }

    pub fn reports(self) -> bool {
        self.0 & Activation::REPORT.0 != 0
    }

    fn with(self, a: Activation) -> Activation {
        Activation(self.0 | a.0)
    }

    fn without(self, a: Activation) -> Activation {
        Activation(self.0 & !a.0)
    }

    /// `a`'s comma-separated changes applied: "Actions can be preceded by a
    /// negative sign to turn them off". An unknown action is ignored.
    fn changed(self, v: &[u8]) -> Activation {
        v.split(|&b| b == b',').fold(self, |acc, item| {
            let (off, name) = match item.strip_prefix(b"-") {
                Some(n) => (true, n),
                None => (false, item.strip_prefix(b"+").unwrap_or(item)),
            };
            let a = match name {
                b"focus" => Activation::FOCUS,
                b"report" => Activation::REPORT,
                _ => return acc,
            };
            match off {
                true => acc.without(a),
                false => acc.with(a),
            }
        })
    }
}

/// `c`: whether the program is told when the notification closes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseReport {
    Silent,
    Report,
}

/// `d`: "A value of 0 means the notification is not yet done ... A non-zero
/// value means the notification is done, and should be displayed."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeDone {
    More,
    Done,
}

/// `e`: "If set to 1 means the payload is base64 encoded UTF-8, otherwise
/// it is plain UTF-8 text".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayloadEncoding {
    Plain,
    Base64,
}

/// One chunk's metadata, read: "a section of colon separated key=value
/// pairs. Every key must be a single character from the set a-zA-Z and
/// every value must be a word consisting of characters from the set
/// a-zA-Z0-9-_/\+.,(){}[]*&^%$#@!`~". A part with no `=`, a key of more
/// than one letter or a value outside the set makes the metadata
/// [`NoticeMeta::Malformed`], and the command is ignored, as kitty's own
/// parser ignores one ("Malformed metadata section", `[[source:
/// kitty-notifications-py]]`). **The set takes `=` and `?` as well**,
/// each a value the text itself writes: its `f`, `n`, `s` and `t` values
/// are base64, which pads with `=`, and kitty's own client sends them
/// padded (`[[source: kitten-notify-go]]`); its query is `p=?`. The set as
/// written would refuse kitty's own tool and the text's own query (Jev's
/// lean, 0.30 and 0.48, over holding to the set exactly and over no
/// check). "a key
/// in the metadata it does not understand ... *must* be ignored", and so
/// is a known key's value this does not understand, the text's "best
/// guess".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeMeta<'a> {
    Keys(NoticeKeys<'a>),
    Malformed,
}

/// The keys of one chunk. The repeated keys (`n`, `t`) are read again from
/// `raw` by [`NoticeKeys::all`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticeKeys<'a> {
    pub raw: &'a [u8],
    pub payload: Payload,
    pub id: Option<&'a [u8]>,
    pub done: NoticeDone,
    pub encoding: PayloadEncoding,
    /// `a`'s value, applied to the notification's set by [`Activation`].
    pub actions: Option<&'a [u8]>,
    pub close: Option<CloseReport>,
    pub occasion: Option<Occasion>,
    pub urgency: Option<Urgency>,
    pub expiry: Option<Expiry>,
    /// `f`, `g` and `s`, as sent: `f` and `s` are base64.
    pub app: Option<&'a [u8]>,
    pub icon_key: Option<&'a [u8]>,
    pub sound: Option<&'a [u8]>,
}

impl<'a> NoticeKeys<'a> {
    /// Every value of key `k`, in order.
    pub fn all(&self, k: u8) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.raw
            .split(|&b| b == b':')
            .filter(move |p| p.len() >= 2 && p[0] == k && p[1] == b'=')
            .map(|p| &p[2..])
    }
}

/// A byte a value may hold: the text's set, `=` for the base64 it asks
/// for, and the `?` of its query.
fn value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-_/\\+.,(){}[]*&^%$#@!`~=?".contains(&b)
}

impl<'a> NoticeMeta<'a> {
    /// The bytes before the payload's `;`.
    pub fn parse(raw: &'a [u8]) -> NoticeMeta<'a> {
        let mut k = NoticeKeys {
            raw,
            payload: Payload::Title,
            id: None,
            done: NoticeDone::Done,
            encoding: PayloadEncoding::Plain,
            actions: None,
            close: None,
            occasion: None,
            urgency: None,
            expiry: None,
            app: None,
            icon_key: None,
            sound: None,
        };
        for part in raw.split(|&b| b == b':') {
            if part.is_empty() {
                continue;
            }
            if part.len() < 2 || !part[0].is_ascii_alphabetic() || part[1] != b'=' {
                return NoticeMeta::Malformed;
            }
            let v = &part[2..];
            if !v.iter().all(|&b| value_byte(b)) {
                return NoticeMeta::Malformed;
            }
            match part[0] {
                b'p' => k.payload = Payload::of(v),
                b'i' => k.id = Some(v),
                b'd' => {
                    k.done = if v == b"0" {
                        NoticeDone::More
                    } else {
                        NoticeDone::Done
                    }
                }
                b'e' => {
                    k.encoding = match v {
                        b"1" => PayloadEncoding::Base64,
                        _ => PayloadEncoding::Plain,
                    }
                }
                b'a' => k.actions = Some(v),
                b'c' => {
                    k.close = Some(if v == b"0" {
                        CloseReport::Silent
                    } else {
                        CloseReport::Report
                    })
                }
                b'o' => {
                    k.occasion = match v {
                        b"always" => Some(Occasion::Always),
                        b"unfocused" => Some(Occasion::Unfocused),
                        b"invisible" => Some(Occasion::Invisible),
                        _ => k.occasion,
                    }
                }
                b'u' => {
                    k.urgency = match v {
                        b"0" => Some(Urgency::Low),
                        b"1" => Some(Urgency::Normal),
                        b"2" => Some(Urgency::Critical),
                        _ => k.urgency,
                    }
                }
                b'w' => {
                    k.expiry = match v {
                        b"-1" => Some(Expiry::System),
                        b"0" => Some(Expiry::Never),
                        _ => match decimal(v) {
                            Some(ms) => Some(Expiry::After(core::time::Duration::from_millis(
                                u64::from(ms),
                            ))),
                            None => k.expiry,
                        },
                    }
                }
                b'f' => k.app = Some(v),
                b'g' => k.icon_key = Some(v),
                b's' => k.sound = Some(v),
                _ => {}
            }
        }
        NoticeMeta::Keys(k)
    }
}

/// An id with every byte outside `[a-zA-Z0-9_\-+.]` removed, written into
/// `out`: kitty's own sanitiser removes them (`sanitize_id`, `[[source:
/// kitty-notifications-py]]`), one of the two the text allows. `None` when
/// what is left does not fit, which refuses the notification rather than
/// echo a different id.
fn sanitised<'o>(id: &[u8], out: &'o mut [u8; NOTICE_ID_MAX]) -> Option<&'o [u8]> {
    let mut n = 0;
    for &b in id {
        if b.is_ascii_alphanumeric() || b"_-+.".contains(&b) {
            if n == NOTICE_ID_MAX {
                return None;
            }
            out[n] = b;
            n += 1;
        }
    }
    Some(&out[..n])
}

// ---------------------------------------------------------------- the desk

/// Where in the area a string lies: its start and its length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Span {
    at: usize,
    len: usize,
}

impl Span {
    const EMPTY: Span = Span { at: 0, len: 0 };

    fn of(self, area: &[u8]) -> &[u8] {
        &area[self.at..self.at + self.len]
    }
}

/// How much area a [`NoticeDesk`] is lent: the icon's bytes chosen by the
/// embedder, the rest fixed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticeBudget {
    icon: usize,
}

impl NoticeBudget {
    /// An icon of up to `bytes`, decoded: kitty recommends "an image size of
    /// 256x256", which a PNG holds in tens of kilobytes.
    pub const fn with_icon(bytes: usize) -> NoticeBudget {
        NoticeBudget { icon: bytes }
    }

    /// The area this budget needs: the chunk, the notification being
    /// assembled with its icon, the log one read publishes into (room for
    /// [`NOTICE_QUEUE`] notifications and one icon), and the live ids.
    pub const fn bytes(self) -> usize {
        NOTICE_CHUNK_MAX
            + RECORD
            + self.icon
            + (RECORD * NOTICE_QUEUE + self.icon)
            + NOTICE_LIVE * NOTICE_ID_MAX
    }

    /// The icon room an area of `len` bytes leaves.
    const fn of_area(len: usize) -> Option<NoticeBudget> {
        let fixed = NoticeBudget::with_icon(0).bytes();
        match len >= fixed {
            true => Some(NoticeBudget {
                icon: (len - fixed) / 2,
            }),
            false => None,
        }
    }
}

/// The regions of an area, from its budget.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Layout {
    chunk: usize,
    title: usize,
    body: usize,
    buttons: usize,
    strings: usize,
    icon: usize,
    icon_max: usize,
    log: usize,
    log_max: usize,
    live: usize,
}

impl Layout {
    fn of(len: usize) -> Option<Layout> {
        let b = NoticeBudget::of_area(len)?;
        let chunk = 0;
        let title = chunk + NOTICE_CHUNK_MAX;
        let body = title + NOTICE_TEXT_MAX;
        let buttons = body + NOTICE_TEXT_MAX;
        let strings = buttons + NOTICE_BUTTONS_MAX;
        let icon = strings + STRINGS;
        let log = icon + b.icon;
        let log_max = RECORD * NOTICE_QUEUE + b.icon;
        let live = log + log_max;
        Some(Layout {
            chunk,
            title,
            body,
            buttons,
            strings,
            icon,
            icon_max: b.icon,
            log,
            log_max,
            live,
        })
    }
}

/// Where a notification came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeFrom {
    Osc9,
    Osc777,
    Kitty,
}

/// A notification's handle, the embedder's to keep beside its own: the
/// same for every update a kitty id makes of one notification.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NoticeToken(u32);

/// What a published entry asks of the embedder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeAction {
    /// Show it, new.
    Show,
    /// Show it in place of the one this token names, "If the original
    /// notification is still displayed it will be replaced, otherwise a new
    /// notification is displayed".
    Replace,
    /// Close the one this token names (`p=close`).
    Close,
}

/// Which strings of a notification a [`Spans`] holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Spans {
    title: Span,
    body: Span,
    buttons: Span,
    id: Span,
    app: Span,
    kind: Span,
    names: [Span; NOTICE_ICON_NAMES],
    icon_key: Span,
    sound: Span,
    icon: Span,
}

impl Spans {
    const EMPTY: Spans = Spans {
        title: Span::EMPTY,
        body: Span::EMPTY,
        buttons: Span::EMPTY,
        id: Span::EMPTY,
        app: Span::EMPTY,
        kind: Span::EMPTY,
        names: [Span::EMPTY; NOTICE_ICON_NAMES],
        icon_key: Span::EMPTY,
        sound: Span::EMPTY,
        icon: Span::EMPTY,
    };
}

/// The settings a notification carries besides its strings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Settings {
    occasion: Occasion,
    urgency: Urgency,
    expiry: Expiry,
    activation: Activation,
    close: CloseReport,
}

impl Settings {
    const DEFAULT: Settings = Settings {
        occasion: Occasion::Always,
        urgency: Urgency::Normal,
        expiry: Expiry::System,
        activation: Activation::FOCUS,
        close: CloseReport::Silent,
    };
}

/// One entry the embedder takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Head {
    token: NoticeToken,
    from: NoticeFrom,
    action: NoticeAction,
    settings: Settings,
    spans: Spans,
}

/// A notification published, as the embedder reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Notice<'a> {
    pub token: NoticeToken,
    pub from: NoticeFrom,
    pub action: NoticeAction,
    pub title: &'a [u8],
    pub body: &'a [u8],
    /// The buttons' text, separated by U+2028 as the payload sent them:
    /// [`Notice::buttons`] splits them.
    buttons: &'a [u8],
    /// The program's id, sanitised; empty when it sent none.
    pub id: &'a [u8],
    /// `f` and the first `t`, decoded.
    pub app: &'a [u8],
    pub kind: &'a [u8],
    names: [&'a [u8]; NOTICE_ICON_NAMES],
    /// `g`, sanitised: the icon cached under it, or to cache [`Notice::icon`]
    /// under.
    pub icon_key: &'a [u8],
    /// The icon's image, as sent, if this notification carried one.
    pub icon: &'a [u8],
    /// `s`, decoded; empty for the default, "system".
    pub sound: &'a [u8],
    pub occasion: Occasion,
    pub urgency: Urgency,
    pub expiry: Expiry,
    pub activation: Activation,
    pub close: CloseReport,
}

impl<'a> Notice<'a> {
    /// The buttons, in order, at most [`NOTICE_BUTTONS`].
    pub fn buttons(&self) -> impl Iterator<Item = &'a [u8]> + 'a {
        let b = self.buttons;
        Utf8Split {
            rest: (!b.is_empty()).then_some(b),
        }
        .filter(|s| !s.is_empty())
        .take(NOTICE_BUTTONS)
    }

    /// The icon names, in the order sent.
    pub fn icon_names(&self) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.names.into_iter().filter(|n| !n.is_empty())
    }
}

/// A split at U+2028 LINE SEPARATOR, `E2 80 A8`.
struct Utf8Split<'a> {
    /// What is left to split; `None` once the last item is out.
    rest: Option<&'a [u8]>,
}

impl<'a> Iterator for Utf8Split<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        let rest = self.rest?;
        match rest.windows(3).position(|w| w == [0xe2, 0x80, 0xa8]) {
            Some(i) => {
                self.rest = Some(&rest[i + 3..]);
                Some(&rest[..i])
            }
            None => {
                self.rest = None;
                Some(rest)
            }
        }
    }
}

/// A notification remembered as alive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Live {
    token: NoticeToken,
    /// Its id's length in its slot of the live ids; zero for none.
    id: usize,
    activation: Activation,
    close: CloseReport,
    /// When it went in, to find the oldest.
    age: u32,
}

/// Whether a live slot holds one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Free,
    Held(Live),
}

/// The notification being assembled from kitty's chunks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flight {
    Idle,
    Open(Assembly),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Assembly {
    /// Its id's length in the strings' id slot.
    id: Option<usize>,
    settings: Settings,
    lens: Lens,
    /// The payload the last chunk carried and its decoder, which a chunk
    /// after it of the same payload carries on: "When the chunking is done
    /// after encoding", a quantum may straddle two.
    last: Payload,
    decoder: Base64,
    cut: Cut,
}

/// How much of each string of the assembly is filled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Lens {
    title: usize,
    body: usize,
    buttons: usize,
    app: usize,
    kind: usize,
    names: [usize; NOTICE_ICON_NAMES],
    names_n: usize,
    icon_key: usize,
    sound: usize,
    icon: usize,
}

impl Lens {
    const ZERO: Lens = Lens {
        title: 0,
        body: 0,
        buttons: 0,
        app: 0,
        kind: 0,
        names: [0; NOTICE_ICON_NAMES],
        names_n: 0,
        icon_key: 0,
        sound: 0,
        icon: 0,
    };
}

/// Whether the icon outgrew its room, which refuses the notification whole:
/// a cut image is no image. A text cut is kept cut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cut {
    Whole,
    IconCut,
}

/// The chunk being received: what its payload is, how it is encoded, and
/// how much of it has been kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Chunk {
    encoding: PayloadEncoding,
    decoder: Base64,
    len: usize,
    state: ChunkState,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChunkState {
    Taking,
    Overflowed,
    NotBase64,
}

/// What the terminal tells a program it supports, in the query's answer:
/// set by the embedder from what its platform delivers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticeSupport {
    /// The answer's `a`, `o`, `u` and `s` lists, and its `c` and `w`.
    pub actions: Activation,
    pub close_events: CloseEvents,
    pub occasions: Occasions,
    pub urgencies: Urgencies,
    pub sounds: Sounds,
    pub expiry: ExpirySupport,
    pub icons: IconSupport,
    /// The answer's `p` list names `buttons` only where they are shown.
    pub buttons: ButtonSupport,
}

/// Whether the platform says when a notification closes. Where it does not
/// -- macOS -- a program that asks is answered `p=close ; untracked`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseEvents {
    Tracked,
    Untracked,
}

/// The occasions the embedder can tell apart, beyond `always`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Occasions {
    Always,
    Unfocused,
    UnfocusedAndInvisible,
}

/// The urgencies the platform shows apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Urgencies {
    None,
    LowAndNormal,
    All,
}

/// The standard sounds the platform plays: `system` and `silent` always
/// ("Terminals *should* support at least system and silent"), and on a
/// platform with named sounds the rest of the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sounds {
    SystemAndSilent,
    Standard,
}

/// Whether the embedder closes a notification after `w`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpirySupport {
    None,
    Expires,
}

/// Whether the embedder shows a transmitted icon (`p=icon`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconSupport {
    NamesOnly,
    NamesAndImages,
}

/// Whether the embedder shows a notification's buttons (`p=buttons`): a
/// freedesktop server that does not list `actions` shows none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonSupport {
    Shown,
    NotShown,
}

/// The notifications refused, by why. Each is also counted where any
/// refused control string is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticeRefusals {
    /// The storage has no area for notifications.
    pub unprovisioned: usize,
    /// The embedder has said nothing about what it delivers, or has turned
    /// notifications off.
    pub off: usize,
    /// Metadata that is not the text's form, or a body with no second `;`.
    pub malformed: usize,
    /// A chunk past [`NOTICE_CHUNK_MAX`], an id past [`NOTICE_ID_MAX`], or
    /// an icon past its room.
    pub too_long: usize,
    /// An `e=1` payload that is not base64.
    pub not_base64: usize,
    /// "A notification with not title and no body is ignored."
    pub empty: usize,
    /// More published in one read than [`NOTICE_QUEUE`], or than the log
    /// holds.
    pub queue_full: usize,
    /// A notification being assembled that a chunk of another id replaced.
    pub abandoned: usize,
}

impl NoticeRefusals {
    pub const NONE: NoticeRefusals = NoticeRefusals {
        unprovisioned: 0,
        off: 0,
        malformed: 0,
        too_long: 0,
        not_base64: 0,
        empty: 0,
        queue_full: 0,
        abandoned: 0,
    };

    pub fn total(&self) -> usize {
        self.unprovisioned
            + self.off
            + self.malformed
            + self.too_long
            + self.not_base64
            + self.empty
            + self.queue_full
            + self.abandoned
    }
}

/// Why a notification or a chunk was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeRefusal {
    Unprovisioned,
    Off,
    Malformed,
    TooLong,
    NotBase64,
    Empty,
    QueueFull,
}

impl NoticeRefusals {
    fn count(&mut self, why: NoticeRefusal) {
        match why {
            NoticeRefusal::Unprovisioned => self.unprovisioned += 1,
            NoticeRefusal::Off => self.off += 1,
            NoticeRefusal::Malformed => self.malformed += 1,
            NoticeRefusal::TooLong => self.too_long += 1,
            NoticeRefusal::NotBase64 => self.not_base64 += 1,
            NoticeRefusal::Empty => self.empty += 1,
            NoticeRefusal::QueueFull => self.queue_full += 1,
        }
    }
}

/// What a chunk's end did: nothing to say, a reply to write, or a refusal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChunkEnd {
    Kept,
    Published,
    Replied,
    Refused(NoticeRefusal),
}

/// A reply a [`NoticeDesk`] composes, written by [`NoticeDesk::reply`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owed {
    Nothing,
    Query(Span),
    Alive(Span),
}

/// **The notifications' state**: the chunk in flight, the notification
/// being assembled, what one read published, what is alive, and the
/// support the embedder declared. The bytes live in an area the storage
/// lends ([`NoticeBudget::bytes`] long); this is the bookkeeping beside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticeDesk {
    support: Option<NoticeSupport>,
    flight: Flight,
    chunk: Chunk,
    heads: [Option<Head>; NOTICE_QUEUE],
    queued: usize,
    log_len: usize,
    live: [Slot; NOTICE_LIVE],
    next: u32,
    age: u32,
    owed: Owed,
    pub refusals: NoticeRefusals,
}

/// The reply a desk owes, borrowed from its area for the caller to write.
pub struct Reply<'a> {
    parts: [&'a [u8]; 5],
    n: usize,
}

impl<'a> Reply<'a> {
    pub fn parts(&self) -> &[&'a [u8]] {
        &self.parts[..self.n]
    }
}

/// The bytes a reply is written into before it is lent: the id, and for a
/// query its list, each in the area's chunk, which no chunk is using once
/// one has ended.
const ST: &[u8] = b"\x1b\\";

impl NoticeDesk {
    pub const NEW: NoticeDesk = NoticeDesk {
        support: None,
        flight: Flight::Idle,
        chunk: Chunk {
            encoding: PayloadEncoding::Plain,
            decoder: Base64::NEW,
            len: 0,
            state: ChunkState::Taking,
        },
        heads: [None; NOTICE_QUEUE],
        queued: 0,
        log_len: 0,
        live: [Slot::Free; NOTICE_LIVE],
        next: 1,
        age: 0,
        owed: Owed::Nothing,
        refusals: NoticeRefusals::NONE,
    };

    /// What the embedder delivers, or `None` -- nothing told, or the
    /// setting off -- under which every notification is refused and the
    /// query is not answered, as a terminal without the feature.
    pub fn set_support(&mut self, s: Option<NoticeSupport>) {
        self.support = s;
    }

    pub fn support(&self) -> Option<NoticeSupport> {
        self.support
    }

    fn refuse(&mut self, why: NoticeRefusal) -> ChunkEnd {
        self.refusals.count(why);
        ChunkEnd::Refused(why)
    }

    /// Whether an area of `len` bytes can hold a desk's state.
    pub fn provisioned(len: usize) -> bool {
        Layout::of(len).is_some()
    }

    // ----------------------------------------------------- kitty's chunks

    /// A chunk's payload begins, after `meta`'s `;`: decide its decoder.
    /// Base64 carries on from the last chunk when this one continues the
    /// same notification and the same payload, unless that one ended at
    /// its pads -- "chunking ... before encoding ... must include the base64
    /// padding bytes".
    pub fn chunk_start(&mut self, area: &[u8], meta: &[u8]) {
        let (encoding, decoder) = match (NoticeMeta::parse(meta), Layout::of(area.len())) {
            (NoticeMeta::Keys(k), Some(l)) => {
                let carried = match self.flight {
                    Flight::Open(a)
                        if a.last == k.payload
                            && continues(area, l, &a, k.id)
                            && !a.decoder.has_ended() =>
                    {
                        a.decoder
                    }
                    _ => Base64::NEW,
                };
                (k.encoding, carried)
            }
            (NoticeMeta::Keys(k), None) => (k.encoding, Base64::NEW),
            (NoticeMeta::Malformed, _) => (PayloadEncoding::Plain, Base64::NEW),
        };
        self.chunk = Chunk {
            encoding,
            decoder,
            len: 0,
            state: ChunkState::Taking,
        };
    }

    /// One byte of a chunk's payload.
    pub fn chunk_put(&mut self, area: &mut [u8], b: u8) {
        let Some(l) = Layout::of(area.len()) else {
            return;
        };
        if self.chunk.state != ChunkState::Taking {
            return;
        }
        match self.chunk.encoding {
            PayloadEncoding::Plain => self.chunk_keep(area, l, &[b]),
            PayloadEncoding::Base64 => match self.chunk.decoder.push(b) {
                Base64Step::Need => {}
                Base64Step::Octets(o) => self.chunk_keep(area, l, o.as_slice()),
                Base64Step::Invalid => self.chunk.state = ChunkState::NotBase64,
            },
        }
    }

    fn chunk_keep(&mut self, area: &mut [u8], l: Layout, bytes: &[u8]) {
        for &x in bytes {
            if self.chunk.len == NOTICE_CHUNK_MAX {
                self.chunk.state = ChunkState::Overflowed;
                return;
            }
            area[l.chunk + self.chunk.len] = x;
            self.chunk.len += 1;
        }
    }

    /// The chunk's terminator: merge it into the notification, publish one
    /// that is done, or answer `p=?` and `p=alive`, or close.
    pub fn chunk_end(&mut self, area: &mut [u8], meta: &[u8]) -> ChunkEnd {
        let Some(l) = Layout::of(area.len()) else {
            return self.refuse(NoticeRefusal::Unprovisioned);
        };
        let NoticeMeta::Keys(k) = NoticeMeta::parse(meta) else {
            return self.refuse(NoticeRefusal::Malformed);
        };
        let mut idbuf = [0u8; NOTICE_ID_MAX];
        let id = match k.id {
            Some(i) => match sanitised(i, &mut idbuf) {
                Some(s) => s,
                None => return self.refuse(NoticeRefusal::TooLong),
            },
            None => b"",
        };
        match k.payload {
            Payload::Query => {
                let Some(s) = self.support else {
                    return self.refuse(NoticeRefusal::Off);
                };
                return self.owe_query(area, l, id, s);
            }
            Payload::Alive => {
                if self.support.is_none() {
                    return self.refuse(NoticeRefusal::Off);
                }
                // "If no i key is specified" kitty answers nothing.
                if id.is_empty() {
                    return ChunkEnd::Kept;
                }
                return self.owe_alive(area, l, id);
            }
            Payload::Close => {
                if id.is_empty() {
                    return ChunkEnd::Kept;
                }
                return match self.live_by_id(area, l, id) {
                    Some(t) => self.publish_close(t),
                    None => ChunkEnd::Kept,
                };
            }
            _ => {}
        }
        if self.support.is_none() {
            self.flight = Flight::Idle;
            return self.refuse(NoticeRefusal::Off);
        }
        match self.chunk.state {
            ChunkState::Taking => {}
            ChunkState::Overflowed => {
                self.flight = Flight::Idle;
                return self.refuse(NoticeRefusal::TooLong);
            }
            ChunkState::NotBase64 => {
                self.flight = Flight::Idle;
                return self.refuse(NoticeRefusal::NotBase64);
            }
        }
        // Continue the assembly, or start one: a chunk of another id while
        // one is open abandons it, as kitty's single pending command per
        // window is replaced.
        let mut a = match self.flight {
            Flight::Open(a) if id_of(area, l, &a) == id => a,
            Flight::Open(_) => {
                self.refusals.abandoned += 1;
                Assembly::new()
            }
            Flight::Idle => Assembly::new(),
        };
        if a.id.is_none() {
            area[l.strings..l.strings + id.len()].copy_from_slice(id);
            a.id = Some(id.len());
        }
        self.merge_keys(area, l, &mut a, &k);
        // The chunk's octets into their payload's string.
        let data_len = self.chunk.len;
        let (dst, room, used) = match k.payload {
            Payload::Title => (l.title, NOTICE_TEXT_MAX, &mut a.lens.title),
            Payload::Body => (l.body, NOTICE_TEXT_MAX, &mut a.lens.body),
            Payload::Buttons => (l.buttons, NOTICE_BUTTONS_MAX, &mut a.lens.buttons),
            Payload::Icon => (l.icon, l.icon_max, &mut a.lens.icon),
            _ => (l.title, 0, &mut a.lens.title),
        };
        if room > 0 {
            let take = data_len.min(room - *used);
            let take = match k.payload {
                Payload::Icon if take < data_len => {
                    a.cut = Cut::IconCut;
                    0
                }
                Payload::Icon => take,
                _ => char_floor(&area[l.chunk..l.chunk + data_len], take),
            };
            area.copy_within(l.chunk..l.chunk + take, dst + *used);
            *used += take;
        }
        a.last = k.payload;
        a.decoder = self.chunk.decoder;
        if k.done == NoticeDone::More {
            self.flight = Flight::Open(a);
            return ChunkEnd::Kept;
        }
        self.flight = Flight::Idle;
        // A final quantum left unpadded is flushed, "There may or may not be
        // padding bytes at the end of the last chunk, terminals must handle
        // either case".
        if let Ok(tail) = a.decoder.finish() {
            let t = tail.as_slice();
            let (dst, room, used) = match a.last {
                Payload::Title => (l.title, NOTICE_TEXT_MAX, &mut a.lens.title),
                Payload::Body => (l.body, NOTICE_TEXT_MAX, &mut a.lens.body),
                Payload::Buttons => (l.buttons, NOTICE_BUTTONS_MAX, &mut a.lens.buttons),
                Payload::Icon => (l.icon, l.icon_max, &mut a.lens.icon),
                _ => (l.title, 0, &mut a.lens.title),
            };
            let take = t.len().min(room - *used);
            area[dst + *used..dst + *used + take].copy_from_slice(&t[..take]);
            *used += take;
        }
        if a.cut == Cut::IconCut {
            return self.refuse(NoticeRefusal::TooLong);
        }
        self.publish_assembly(area, l, a)
    }

    /// The keys of a chunk applied to the assembly: "You can specify the
    /// title or body multiple times and the terminal emulator will
    /// concatenate them"; a key a later chunk sends again takes its later
    /// value; the repeated keys accumulate.
    fn merge_keys(&self, area: &mut [u8], l: Layout, a: &mut Assembly, k: &NoticeKeys<'_>) {
        if let Some(v) = k.actions {
            a.settings.activation = a.settings.activation.changed(v);
        }
        if let Some(c) = k.close {
            a.settings.close = c;
        }
        if let Some(o) = k.occasion {
            a.settings.occasion = o;
        }
        if let Some(u) = k.urgency {
            a.settings.urgency = u;
        }
        if let Some(w) = k.expiry {
            a.settings.expiry = w;
        }
        let s = l.strings + NOTICE_ID_MAX;
        if let Some(v) = k.app {
            a.lens.app = decode_into(v, &mut area[s..s + NOTICE_NAME_MAX]);
        }
        if a.lens.kind == 0 {
            if let Some(v) = k.all(b't').next() {
                let at = s + NOTICE_NAME_MAX;
                a.lens.kind = decode_into(v, &mut area[at..at + NOTICE_NAME_MAX]);
            }
        }
        for v in k.all(b'n') {
            if a.lens.names_n == NOTICE_ICON_NAMES {
                break;
            }
            let at = s + NOTICE_NAME_MAX * (2 + a.lens.names_n);
            let n = decode_into(v, &mut area[at..at + NOTICE_NAME_MAX]);
            if n > 0 {
                a.lens.names[a.lens.names_n] = n;
                a.lens.names_n += 1;
            }
        }
        let after_names = s + NOTICE_NAME_MAX * (2 + NOTICE_ICON_NAMES);
        if let Some(g) = k.icon_key {
            let mut buf = [0u8; NOTICE_ID_MAX];
            if let Some(g) = sanitised(g, &mut buf) {
                area[after_names..after_names + g.len()].copy_from_slice(g);
                a.lens.icon_key = g.len();
            }
        }
        if let Some(v) = k.sound {
            let at = after_names + NOTICE_ID_MAX;
            a.lens.sound = decode_into(v, &mut area[at..at + NOTICE_NAME_MAX]);
        }
    }

    /// A finished notification into the log, the live table updated:
    /// "If a notification has no title, the body will be used as title. A
    /// notification with not title and no body is ignored."
    fn publish_assembly(&mut self, area: &mut [u8], l: Layout, a: Assembly) -> ChunkEnd {
        let (title, body) = match (a.lens.title, a.lens.body) {
            (0, 0) => return self.refuse(NoticeRefusal::Empty),
            (0, b) => (Span { at: l.body, len: b }, Span::EMPTY),
            (t, b) => (
                Span {
                    at: l.title,
                    len: t,
                },
                Span { at: l.body, len: b },
            ),
        };
        let s = l.strings + NOTICE_ID_MAX;
        let after_names = s + NOTICE_NAME_MAX * (2 + NOTICE_ICON_NAMES);
        let mut names = [Span::EMPTY; NOTICE_ICON_NAMES];
        for (i, n) in names.iter_mut().enumerate() {
            *n = Span {
                at: s + NOTICE_NAME_MAX * (2 + i),
                len: a.lens.names[i],
            };
        }
        let src = Spans {
            title,
            body,
            buttons: Span {
                at: l.buttons,
                len: a.lens.buttons,
            },
            id: Span {
                at: l.strings,
                len: a.id.unwrap_or(0),
            },
            app: Span {
                at: s,
                len: a.lens.app,
            },
            kind: Span {
                at: s + NOTICE_NAME_MAX,
                len: a.lens.kind,
            },
            names,
            icon_key: Span {
                at: after_names,
                len: a.lens.icon_key,
            },
            sound: Span {
                at: after_names + NOTICE_ID_MAX,
                len: a.lens.sound,
            },
            icon: Span {
                at: l.icon,
                len: a.lens.icon,
            },
        };
        self.publish(area, l, NoticeFrom::Kitty, a.settings, src)
    }

    /// An `OSC 9` or an `OSC 777` notification: the title and body copied
    /// into the assembly's strings and published as kitty's would be,
    /// focus on a click, nothing reported.
    pub fn simple(
        &mut self,
        area: &mut [u8],
        from: NoticeFrom,
        title: &[u8],
        body: &[u8],
    ) -> ChunkEnd {
        let Some(l) = Layout::of(area.len()) else {
            return self.refuse(NoticeRefusal::Unprovisioned);
        };
        if self.support.is_none() {
            return self.refuse(NoticeRefusal::Off);
        }
        let (title, body) = match (title.is_empty(), body.is_empty()) {
            (true, true) => return self.refuse(NoticeRefusal::Empty),
            (true, false) => (body, &body[body.len()..]),
            _ => (title, body),
        };
        // Staged in the chunk's room, which no kitty chunk holds between
        // two strings, so a kitty notification being assembled keeps its
        // title and body.
        let t = char_floor(title, title.len().min(NOTICE_TEXT_MAX));
        let b = char_floor(body, body.len().min(NOTICE_TEXT_MAX));
        let (tat, bat) = (l.chunk, l.chunk + NOTICE_TEXT_MAX);
        area[tat..tat + t].copy_from_slice(&title[..t]);
        area[bat..bat + b].copy_from_slice(&body[..b]);
        // Neither text says when to show them, so they show while the
        // window is not focused (Jev, 0.98 and 0.97).
        let settings = Settings {
            occasion: Occasion::Unfocused,
            ..Settings::DEFAULT
        };
        let mut src = Spans::EMPTY;
        src.title = Span { at: tat, len: t };
        src.body = Span { at: bat, len: b };
        self.publish(area, l, from, settings, src)
    }

    /// Copy `src`'s strings into the log and queue a head.
    fn publish(
        &mut self,
        area: &mut [u8],
        l: Layout,
        from: NoticeFrom,
        settings: Settings,
        src: Spans,
    ) -> ChunkEnd {
        let need = [
            src.title,
            src.body,
            src.buttons,
            src.id,
            src.app,
            src.kind,
            src.icon_key,
            src.sound,
            src.icon,
        ]
        .iter()
        .chain(src.names.iter())
        .map(|s| s.len)
        .sum::<usize>();
        if self.queued == NOTICE_QUEUE || self.log_len + need > l.log_max {
            return self.refuse(NoticeRefusal::QueueFull);
        }
        let mut at = l.log + self.log_len;
        let mut copy = |s: Span| -> Span {
            area.copy_within(s.at..s.at + s.len, at);
            let out = Span { at, len: s.len };
            at += s.len;
            out
        };
        let mut spans = Spans {
            title: copy(src.title),
            body: copy(src.body),
            buttons: copy(src.buttons),
            id: copy(src.id),
            app: copy(src.app),
            kind: copy(src.kind),
            names: [Span::EMPTY; NOTICE_ICON_NAMES],
            icon_key: copy(src.icon_key),
            sound: copy(src.sound),
            icon: copy(src.icon),
        };
        for (i, n) in src.names.iter().enumerate() {
            spans.names[i] = copy(*n);
        }
        self.log_len = at - l.log;
        // An id already alive is that notification updated: "To update a
        // previous notification simply send a new notification with the
        // same notification id". With no id, "no updating must take place".
        let found = self.live_by_id(area, l, spans.id.of(area));
        let (token, action) = match found {
            Some(t) => (t, NoticeAction::Replace),
            None => {
                let t = NoticeToken(self.next);
                self.next = self.next.wrapping_add(1).max(1);
                (t, NoticeAction::Show)
            }
        };
        self.remember(area, l, token, spans.id, settings);
        self.heads[self.queued] = Some(Head {
            token,
            from,
            action,
            settings,
            spans,
        });
        self.queued += 1;
        ChunkEnd::Published
    }

    fn publish_close(&mut self, token: NoticeToken) -> ChunkEnd {
        if self.queued == NOTICE_QUEUE {
            return self.refuse(NoticeRefusal::QueueFull);
        }
        self.heads[self.queued] = Some(Head {
            token,
            from: NoticeFrom::Kitty,
            action: NoticeAction::Close,
            settings: Settings::DEFAULT,
            spans: Spans::EMPTY,
        });
        self.queued += 1;
        ChunkEnd::Published
    }

    /// Put `token` in the live table, its id beside it; an update keeps its
    /// slot. A full table forgets its oldest.
    fn remember(&mut self, area: &mut [u8], l: Layout, token: NoticeToken, id: Span, s: Settings) {
        self.age = self.age.wrapping_add(1);
        let age = self.age;
        let slot = self
            .live
            .iter()
            .position(|e| matches!(e, Slot::Held(x) if x.token == token))
            .or_else(|| self.live.iter().position(|e| *e == Slot::Free))
            .unwrap_or_else(|| {
                (0..NOTICE_LIVE)
                    .max_by_key(|&i| match self.live[i] {
                        Slot::Held(x) => age.wrapping_sub(x.age),
                        Slot::Free => 0,
                    })
                    .unwrap_or(0)
            });
        let at = l.live + slot * NOTICE_ID_MAX;
        area.copy_within(id.at..id.at + id.len, at);
        self.live[slot] = Slot::Held(Live {
            token,
            id: id.len,
            activation: s.activation,
            close: s.close,
            age: self.age,
        });
    }

    fn live_by_id(&self, area: &[u8], l: Layout, id: &[u8]) -> Option<NoticeToken> {
        if id.is_empty() {
            return None;
        }
        self.live.iter().enumerate().find_map(|(i, e)| match e {
            Slot::Held(x)
                if x.id == id.len() && &area[l.live + i * NOTICE_ID_MAX..][..x.id] == id =>
            {
                Some(x.token)
            }
            _ => None,
        })
    }

    fn live_slot(&self, token: NoticeToken) -> Option<(usize, Live)> {
        self.live.iter().enumerate().find_map(|(i, e)| match e {
            Slot::Held(x) if x.token == token => Some((i, *x)),
            _ => None,
        })
    }

    // ----------------------------------------------------------- replies

    /// `p=?`: "A conforming terminal must respond with ... key=value parts
    /// [that] specify details about what the terminal implementation
    /// supports", each list only what the embedder delivers. The answer is
    /// composed in the chunk's room, which no chunk needs once one ended.
    fn owe_query(&mut self, area: &mut [u8], l: Layout, id: &[u8], s: NoticeSupport) -> ChunkEnd {
        let mut w = Writer {
            buf: &mut area[l.chunk..l.chunk + NOTICE_CHUNK_MAX],
            n: 0,
        };
        w.put(b"\x1b]99;i=");
        w.put(if id.is_empty() { b"0" } else { id });
        w.put(b":p=?;");
        let mut first = true;
        let mut key = |w: &mut Writer<'_>, k: &[u8]| {
            if !first {
                w.put(b":");
            }
            first = false;
            w.put(k);
        };
        match (s.actions.focuses(), s.actions.reports()) {
            (true, true) => key(&mut w, b"a=focus,report"),
            (true, false) => key(&mut w, b"a=focus"),
            (false, true) => key(&mut w, b"a=report"),
            (false, false) => {}
        }
        if s.close_events == CloseEvents::Tracked {
            key(&mut w, b"c=1");
        }
        key(
            &mut w,
            match s.occasions {
                Occasions::Always => b"o=always",
                Occasions::Unfocused => b"o=always,unfocused",
                Occasions::UnfocusedAndInvisible => b"o=always,unfocused,invisible",
            },
        );
        key(
            &mut w,
            match s.icons {
                IconSupport::NamesOnly => b"p=title,body,close,?,alive",
                IconSupport::NamesAndImages => b"p=title,body,close,icon,?,alive",
            },
        );
        // "supported payload types": buttons only where they are shown.
        if s.buttons == ButtonSupport::Shown {
            w.put(b",buttons");
        }
        key(
            &mut w,
            match s.sounds {
                Sounds::SystemAndSilent => b"s=system,silent",
                Sounds::Standard => b"s=system,silent,error,warn,warning,info,question",
            },
        );
        match s.urgencies {
            Urgencies::None => {}
            Urgencies::LowAndNormal => key(&mut w, b"u=0,1"),
            Urgencies::All => key(&mut w, b"u=0,1,2"),
        }
        if s.expiry == ExpirySupport::Expires {
            key(&mut w, b"w=1");
        }
        w.put(ST);
        let len = w.n;
        self.owed = Owed::Query(Span { at: l.chunk, len });
        ChunkEnd::Replied
    }

    /// `p=alive`: "The terminal will reply with ... a comma separated list
    /// of ids that are still alive". Only notifications sent with an id can
    /// be named.
    fn owe_alive(&mut self, area: &mut [u8], l: Layout, id: &[u8]) -> ChunkEnd {
        let (head, rest) = area.split_at_mut(l.live);
        let mut w = Writer {
            buf: &mut head[l.chunk..l.chunk + NOTICE_CHUNK_MAX],
            n: 0,
        };
        w.put(b"\x1b]99;i=");
        w.put(id);
        w.put(b":p=alive;");
        let mut first = true;
        for (i, e) in self.live.iter().enumerate() {
            if let Slot::Held(x) = e {
                if x.id == 0 {
                    continue;
                }
                if !first {
                    w.put(b",");
                }
                first = false;
                w.put(&rest[i * NOTICE_ID_MAX..][..x.id]);
            }
        }
        w.put(ST);
        let len = w.n;
        self.owed = Owed::Alive(Span { at: l.chunk, len });
        ChunkEnd::Replied
    }

    /// The reply a chunk's end composed, once; the caller writes it.
    pub fn take_reply<'a>(&mut self, area: &'a [u8]) -> Option<&'a [u8]> {
        let s = match core::mem::replace(&mut self.owed, Owed::Nothing) {
            Owed::Nothing => return None,
            Owed::Query(s) | Owed::Alive(s) => s,
        };
        Some(s.of(area))
    }

    /// The program's report of a click, if it asked for one (`a=report`):
    /// `OSC 99 ; i=identifier ; ST`, or `; button_number` for a button,
    /// from 1. "If no identifier was specified ... i=0 is used", and "The
    /// terminal *must not* send a response unless report is requested".
    pub fn activated<'a>(
        &self,
        area: &'a [u8],
        token: NoticeToken,
        button: Option<NoticeButton>,
    ) -> Option<Reply<'a>> {
        let l = Layout::of(area.len())?;
        let (slot, x) = self.live_slot(token)?;
        if !x.activation.reports() {
            return None;
        }
        let id = match x.id {
            0 => &b"0"[..],
            n => &area[l.live + slot * NOTICE_ID_MAX..][..n],
        };
        let n: &'a [u8] = match button {
            None => b"",
            Some(b) => BUTTON_DIGITS[usize::from(b.0)],
        };
        Some(Reply {
            parts: [b"\x1b]99;i=", id, b";", n, ST],
            n: 5,
        })
    }

    /// Whether a click should focus the window (`a`'s `focus`).
    pub fn focuses(&self, token: NoticeToken) -> bool {
        self.live_slot(token)
            .is_some_and(|(_, x)| x.activation.focuses())
    }

    /// The notification closed, as the platform says; forgotten, and the
    /// program told if it asked (`c=1`): `OSC 99 ; i=id : p=close ; ST`.
    pub fn closed<'a>(&mut self, area: &'a [u8], token: NoticeToken) -> Option<Reply<'a>> {
        let l = Layout::of(area.len())?;
        let (slot, x) = self.live_slot(token)?;
        self.live[slot] = Slot::Free;
        self.close_reply(area, l, slot, x, b"")
    }

    /// The notification was shown. On a platform that does not say when one
    /// closes, a program that asked is told so at once: "terminals reply
    /// with ... p=close ; untracked".
    pub fn shown<'a>(&self, area: &'a [u8], token: NoticeToken) -> Option<Reply<'a>> {
        let l = Layout::of(area.len())?;
        let s = self.support?;
        if s.close_events != CloseEvents::Untracked {
            return None;
        }
        let (slot, x) = self.live_slot(token)?;
        self.close_reply(area, l, slot, x, b"untracked")
    }

    /// The embedder did not show it (focus, rate, the platform's refusal):
    /// forgotten, nothing said, as kitty says nothing of one it does not
    /// show.
    pub fn dropped(&mut self, token: NoticeToken) {
        if let Some((slot, _)) = self.live_slot(token) {
            self.live[slot] = Slot::Free;
        }
    }

    fn close_reply<'a>(
        &self,
        area: &'a [u8],
        l: Layout,
        slot: usize,
        x: Live,
        tail: &'a [u8],
    ) -> Option<Reply<'a>> {
        if x.close != CloseReport::Report {
            return None;
        }
        let id = match x.id {
            0 => &b"0"[..],
            n => &area[l.live + slot * NOTICE_ID_MAX..][..n],
        };
        Some(Reply {
            parts: [b"\x1b]99;i=", id, b":p=close;", tail, ST],
            n: 5,
        })
    }

    // ------------------------------------------------------------ the log

    /// What this read published, in order.
    pub fn notices<'a>(&self, area: &'a [u8]) -> impl Iterator<Item = Notice<'a>> + 'a {
        let heads = self.heads;
        let n = self.queued;
        heads.into_iter().take(n).flatten().map(move |h| Notice {
            token: h.token,
            from: h.from,
            action: h.action,
            title: h.spans.title.of(area),
            body: h.spans.body.of(area),
            buttons: h.spans.buttons.of(area),
            id: h.spans.id.of(area),
            app: h.spans.app.of(area),
            kind: h.spans.kind.of(area),
            names: h.spans.names.map(|s| s.of(area)),
            icon_key: h.spans.icon_key.of(area),
            icon: h.spans.icon.of(area),
            sound: h.spans.sound.of(area),
            occasion: h.settings.occasion,
            urgency: h.settings.urgency,
            expiry: h.settings.expiry,
            activation: h.settings.activation,
            close: h.settings.close,
        })
    }

    /// The log emptied, after the embedder took it.
    pub fn clear(&mut self) {
        self.heads = [None; NOTICE_QUEUE];
        self.queued = 0;
        self.log_len = 0;
    }

    /// RIS: the notification being assembled is dropped; what is alive on
    /// the screen stays alive.
    pub fn reset(&mut self) {
        self.flight = Flight::Idle;
    }
}

/// A button of a notification, the first being 1: "button_number is a
/// number from 1 onwards, where 1 corresponds to the first button".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticeButton(u8);

impl NoticeButton {
    /// The button at `index` from 0 among those [`Notice::buttons`] gave,
    /// `None` past the last a notification can show.
    pub fn of_index(index: usize) -> Option<NoticeButton> {
        (index < NOTICE_BUTTONS).then(|| NoticeButton(index as u8 + 1))
    }
}

/// The digits a button report names, 1 to [`NOTICE_BUTTONS`].
const BUTTON_DIGITS: [&[u8]; NOTICE_BUTTONS + 1] =
    [b"", b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8"];

/// A bounded writer; what does not fit is dropped, and a reply that cannot
/// fit is not composed (a chunk's room exceeds every answer).
struct Writer<'a> {
    buf: &'a mut [u8],
    n: usize,
}

impl Writer<'_> {
    fn put(&mut self, s: &[u8]) {
        let take = s.len().min(self.buf.len() - self.n);
        self.buf[self.n..self.n + take].copy_from_slice(&s[..take]);
        self.n += take;
    }
}

impl Assembly {
    fn new() -> Assembly {
        Assembly {
            id: None,
            settings: Settings::DEFAULT,
            lens: Lens::ZERO,
            last: Payload::Title,
            decoder: Base64::NEW,
            cut: Cut::Whole,
        }
    }
}

fn id_of<'a>(area: &'a [u8], l: Layout, a: &Assembly) -> &'a [u8] {
    &area[l.strings..l.strings + a.id.unwrap_or(0)]
}

/// Whether a chunk with `id` continues the assembly: kitty merges when the
/// previous "is not done and its identifier is this one's"
/// (`parse_metadata`, `[[source: kitty-notifications-py]]`), no id matching
/// no id.
fn continues(area: &[u8], l: Layout, a: &Assembly, id: Option<&[u8]>) -> bool {
    let mut buf = [0u8; NOTICE_ID_MAX];
    let want = match id {
        Some(i) => match sanitised(i, &mut buf) {
            Some(w) => w,
            None => return false,
        },
        None => b"",
    };
    id_of(area, l, a) == want
}

/// A base64 value decoded into `out`, cut at a character's start; zero for
/// one that is not base64, which the text's best guess ignores.
fn decode_into(v: &[u8], out: &mut [u8]) -> usize {
    let mut d = Base64::NEW;
    let mut n = 0;
    for &b in v {
        match d.push(b) {
            Base64Step::Need => {}
            Base64Step::Octets(o) => {
                for &x in o.as_slice() {
                    if n < out.len() {
                        out[n] = x;
                    }
                    n += 1;
                }
            }
            Base64Step::Invalid => return 0,
        }
    }
    if let Ok(t) = d.finish() {
        for &x in t.as_slice() {
            if n < out.len() {
                out[n] = x;
            }
            n += 1;
        }
    } else {
        return 0;
    }
    char_floor(out, n.min(out.len()))
}

/// The largest length at most `n` that does not end inside a UTF-8
/// character of `s`.
fn char_floor(s: &[u8], n: usize) -> usize {
    if n >= s.len() {
        return s.len().min(n);
    }
    let mut i = n;
    while i > 0 && (s[i] & 0xc0) == 0x80 {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc9_text_and_conemu_forms_are_told_apart() {
        assert_eq!(Osc9::parse(b"Hello World"), Osc9::Notify(b"Hello World"));
        assert_eq!(
            Osc9::parse(b"5 minutes left"),
            Osc9::Notify(b"5 minutes left")
        );
        assert_eq!(Osc9::parse(b"42"), Osc9::Notify(b"42"));
        assert_eq!(
            Osc9::parse(b"9;C:/dir"),
            Osc9::ConEmu(ConEmuOp::WorkingDirectory)
        );
        assert_eq!(Osc9::parse(b"2;box"), Osc9::ConEmu(ConEmuOp::MessageBox));
        assert_eq!(Osc9::parse(b"7;rm -rf"), Osc9::ConEmu(ConEmuOp::RunProcess));
        assert_eq!(Osc9::parse(b"12"), Osc9::ConEmu(ConEmuOp::PromptStart));
        assert_eq!(Osc9::parse(b"10;1"), Osc9::ConEmu(ConEmuOp::XtermEmulation));
        // The bare forms that take a `;` in ConEmu's table are text.
        assert_eq!(Osc9::parse(b"2"), Osc9::Notify(b"2"));
        assert_eq!(Osc9::parse(b"5;x"), Osc9::Notify(b"5;x"));
    }

    #[test]
    fn progress_reads_every_state_and_clamps() {
        let p = |s, pr| {
            Osc9::Progress(ProgressReport {
                state: s,
                percent: pr,
            })
        };
        assert_eq!(Osc9::parse(b"4"), p(ProgressState::Hidden, None));
        assert_eq!(Osc9::parse(b"4;0"), p(ProgressState::Hidden, None));
        assert_eq!(
            Osc9::parse(b"4;1;50"),
            p(ProgressState::Normal, Some(Percent(50)))
        );
        assert_eq!(
            Osc9::parse(b"4;1;250"),
            p(ProgressState::Normal, Some(Percent(100)))
        );
        assert_eq!(Osc9::parse(b"4;2"), p(ProgressState::Error, None));
        assert_eq!(
            Osc9::parse(b"4;3;77"),
            p(ProgressState::Indeterminate, Some(Percent(77)))
        );
        assert_eq!(
            Osc9::parse(b"4;4;25"),
            p(ProgressState::Paused, Some(Percent(25)))
        );
        assert_eq!(Osc9::parse(b"4;9"), Osc9::ProgressMalformed);
        assert_eq!(Osc9::parse(b"4;1;x"), Osc9::ProgressMalformed);
    }

    #[test]
    fn progress_keeps_the_percentage_a_pause_does_not_name() {
        let shown = Progress::HIDDEN.after(ProgressReport {
            state: ProgressState::Normal,
            percent: Some(Percent(40)),
        });
        let paused = shown.after(ProgressReport {
            state: ProgressState::Paused,
            percent: None,
        });
        assert_eq!(paused.percent, Some(Percent(40)));
        let busy = paused.after(ProgressReport {
            state: ProgressState::Indeterminate,
            percent: Some(Percent(9)),
        });
        assert_eq!(busy.percent, None);
        let set = busy.after(ProgressReport {
            state: ProgressState::Normal,
            percent: None,
        });
        assert_eq!(set.percent, Some(Percent::ZERO));
    }

    #[test]
    fn osc777_takes_the_rest_as_the_body() {
        assert_eq!(
            Osc777::parse(b"notify;Command completed;make; make install"),
            Osc777::Notify {
                title: b"Command completed",
                body: b"make; make install"
            }
        );
        assert_eq!(
            Osc777::parse(b"notify;Title"),
            Osc777::Notify {
                title: b"Title",
                body: b""
            }
        );
        assert_eq!(Osc777::parse(b"notify"), Osc777::Malformed);
        assert_eq!(Osc777::parse(b"precmd"), Osc777::Extension);
        assert_eq!(Osc777::parse(b"notifyx;a"), Osc777::Extension);
    }

    #[test]
    fn osc1337_reads_request_attention_only() {
        assert_eq!(
            Osc1337::parse(b"RequestAttention=yes"),
            Osc1337::Attention(Attention::Yes)
        );
        assert_eq!(
            Osc1337::parse(b"RequestAttention=once"),
            Osc1337::Attention(Attention::Once)
        );
        assert_eq!(
            Osc1337::parse(b"RequestAttention=no"),
            Osc1337::Attention(Attention::No)
        );
        assert_eq!(
            Osc1337::parse(b"RequestAttention=fireworks"),
            Osc1337::Fireworks
        );
        assert_eq!(
            Osc1337::parse(b"RequestAttention=maybe"),
            Osc1337::AttentionMalformed
        );
        assert_eq!(Osc1337::parse(b"SetMark"), Osc1337::Other);
    }

    #[test]
    fn metadata_follows_the_texts_grammar() {
        let NoticeMeta::Keys(k) =
            NoticeMeta::parse(b"i=1:d=0:p=body:e=1:u=2:o=unfocused:w=5000:a=-focus,report")
        else {
            panic!("keys");
        };
        assert_eq!(k.id, Some(&b"1"[..]));
        assert_eq!(k.done, NoticeDone::More);
        assert_eq!(k.payload, Payload::Body);
        assert_eq!(k.encoding, PayloadEncoding::Base64);
        assert_eq!(k.urgency, Some(Urgency::Critical));
        assert_eq!(k.occasion, Some(Occasion::Unfocused));
        assert_eq!(
            k.expiry,
            Some(Expiry::After(core::time::Duration::from_millis(5000)))
        );
        let a = Activation::FOCUS.changed(k.actions.unwrap());
        assert!(!a.focuses() && a.reports());
        assert_eq!(NoticeMeta::parse(b"ii=1"), NoticeMeta::Malformed);
        assert_eq!(NoticeMeta::parse(b"i"), NoticeMeta::Malformed);
        // A value outside the set is malformed; a base64 value padded with
        // `=` is not, as kitty's own client sends it.
        assert_eq!(NoticeMeta::parse(b"i=a b"), NoticeMeta::Malformed);
        assert_eq!(NoticeMeta::parse(b"i=a'b"), NoticeMeta::Malformed);
        let NoticeMeta::Keys(k) = NoticeMeta::parse(b"f=bWFrZQ==:i=x") else {
            panic!("a padded value");
        };
        assert_eq!(k.app, Some(&b"bWFrZQ=="[..]));
        // An unknown key is ignored, and so is a known key's unknown value.
        let NoticeMeta::Keys(k) = NoticeMeta::parse(b"z=9:o=sometimes::p=nonsense") else {
            panic!("keys");
        };
        assert_eq!(k.occasion, None);
        assert_eq!(k.payload, Payload::Unknown);
    }

    #[test]
    fn ids_are_sanitised_and_refused_when_too_long() {
        let mut b = [0u8; NOTICE_ID_MAX];
        assert_eq!(sanitised(b"a$b(c)", &mut b), Some(&b"abc"[..]));
        let long = [b'a'; NOTICE_ID_MAX + 1];
        assert_eq!(sanitised(&long, &mut b), None);
    }

    #[test]
    fn a_cut_keeps_whole_characters() {
        let s = "aé".as_bytes();
        assert_eq!(char_floor(s, 2), 1);
        assert_eq!(char_floor(s, 3), 3);
    }
}
