//! **The desktop's session bus, as far as the Linux driver speaks it**: the
//! appearance the Settings portal reports and the file manager's `ShowItems`,
//! over D-Bus with no library -- the wire protocol of `[[source:
//! dbus-spec]]`, little-endian, no file descriptors.
//!
//! * **The connection** is the session bus's address in
//!   `DBUS_SESSION_BUS_ADDRESS` -- `unix:path=` or `unix:abstract=`, the
//!   first that connects -- and where that is unset, `$XDG_RUNTIME_DIR/bus`,
//!   the platform's place for it. The spec's "starting one if a running
//!   instance cannot be found" is not followed: a terminal that launched a bus
//!   would own it, so with none the appearance stays as configured and a
//!   reveal is refused, each said.
//! * **The handshake** is SASL `EXTERNAL` with the user ID as the identity,
//!   "the ASCII decimal string form of the integer Unix user ID ... encoded
//!   in hex", then `BEGIN` and `Hello`, which every message waits behind.
//! * **The appearance**: `AddMatch` for the portal's `SettingChanged` on
//!   `org.freedesktop.appearance`, then `ReadAll` of that namespace -- the
//!   method every version of the interface has and the one whose reply
//!   holds each value inside one variant (`[[source: xdg-portal-settings-v2]]`;
//!   `Read`'s reply holds two, its own documentation says, and `ReadOne` is
//!   version 2's). The portal is `org.freedesktop.portal.Desktop` at
//!   `/org/freedesktop/portal/desktop` (`[[source:
//!   xdg-portal-api-reference]]` 11-12). `color-scheme` is 0 no preference,
//!   1 dark, 2 light; `contrast` 0 no preference (normal contrast), 1
//!   higher contrast; `reduced-motion` 0 no preference, 1 reduced motion;
//!   and for each "Unknown values should be treated as 0". A portal
//!   without the last two -- they are not in every version -- reads as no
//!   preference for them.
//! * **Notifications** (`notify.rs`): `Notify` on
//!   `org.freedesktop.Notifications`, "Sends a notification to the
//!   notification server", its reply the server's id; `CloseNotification`;
//!   `GetCapabilities` asked once, for `actions` and `body-markup`; and the
//!   `ActionInvoked` and `NotificationClosed` signals, matched with
//!   `AddMatch`, and `ActivationToken`, which "can be emitted before a
//!   `ActionInvoked` signal" and "carries an activation token that can be
//!   used to activate a toplevel" (`[[source:
//!   freedesktop-notification-spec]]`): kept for the click it comes
//!   before, and handed over with it.
//! * **Progress**: `com.canonical.Unity.LauncherEntry`'s `Update`
//!   signal, `progress` and `progress-visible`, and `urgent` for an
//!   attention request, the interface docks read for a launcher
//!   (`[[source: libunity-launcher-entry]]`).
//! * **A reveal**: `ShowItems` of one `file:` URI on
//!   `org.freedesktop.FileManager1`, which "is supposed to select the passed
//!   items within their respective parent folders" (`[[source:
//!   freedesktop-file-manager1]]`) -- what Finder's reveal is on macOS, and
//!   it launches nothing but the file manager.
//!
//! Nothing here blocks: the socket is non-blocking, the driver's one wait
//! watches it, and [`Bus::pump`] moves what the kernel will take and reads
//! what has come. [`Session`] is the protocol over bytes, so the tests run
//! on every platform without a bus.

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;

/// The portal's `color-scheme`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Scheme {
    NoPreference,
    Dark,
    Light,
}

impl Scheme {
    /// 1 dark, 2 light, and anything else no preference, as the portal says
    /// to treat an unknown value.
    fn of(v: Word) -> Scheme {
        match v.0 {
            1 => Scheme::Dark,
            2 => Scheme::Light,
            _ => Scheme::NoPreference,
        }
    }
}

/// The portal's `reduced-motion`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Motion {
    NoPreference,
    Reduced,
}

impl Motion {
    /// 1 reduced, anything else no preference.
    fn of(v: Word) -> Motion {
        match v.0 {
            1 => Motion::Reduced,
            _ => Motion::NoPreference,
        }
    }
}

/// The portal's `contrast`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Contrast {
    NoPreference,
    Higher,
}

impl Contrast {
    /// 1 higher, anything else no preference: normal contrast.
    fn of(v: Word) -> Contrast {
        match v.0 {
            1 => Contrast::Higher,
            _ => Contrast::NoPreference,
        }
    }
}

/// Whether a scheme is the one the desktop had at launch or a change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Origin {
    /// `ReadAll`'s answer: the state the window starts in.
    Read,
    /// `SettingChanged`: the user changed it.
    Changed,
}

/// What the bus said, for the driver.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Heard {
    /// The desktop's colour scheme, read or changed.
    Scheme(Scheme, Origin),
    /// The portal could not be read: no portal, no Settings, or no
    /// `color-scheme` in its reply. The error's name, or what was missing.
    NoScheme(String),
    /// The desktop's motion preference, read or changed; a reply without
    /// one says nothing of it.
    Motion(Motion),
    /// The desktop's contrast preference, read or changed, the same way.
    Contrast(Contrast),
    /// The file manager took a reveal.
    Revealed,
    /// It did not: the error's name.
    NotRevealed(String),
    /// The notification server's capabilities: there is one.
    Caps(crate::notify::ServerCaps),
    /// `GetCapabilities` failed: no notification server, the error's name.
    NoServer(String),
    /// A notification shown, as the server's id.
    Notified(baud_core::NoticeToken, NotifyId),
    /// It was not: the error's name.
    NotNotified(baud_core::NoticeToken, String),
    /// A click on a notification, or on one of its actions: the key, and
    /// the activation token the server sent for it just before, if any.
    Invoked(NotifyId, String, Option<String>),
    /// A notification closed, for any reason.
    NotifyClosed(NotifyId),
    /// The handshake failed or the bus went away; nothing more will come.
    Closed(String),
}

/// A notification's id, as the server gave it: "The returned ID is always
/// greater than zero".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NotifyId(Word);

/// A byte on the wire, a `y`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Octet(pub(crate) u8);

/// A `b`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Truth {
    False,
    True,
}

/// An RGBA image, as `image-data`'s `(iiibiiay)` carries it: "width,
/// height, rowstride, has alpha, bits per sample, channels and image
/// data".
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Image {
    pub(crate) size: baud_domain::png::Size,
    pub(crate) rgba: Vec<u8>,
}

/// A hint's value: the types the standard hints take.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Hint {
    Byte(Octet),
    Bool(Truth),
    Str(String),
    Image(Image),
}

/// One `Notify`: its arguments, in the order the method takes them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Notification {
    pub(crate) app_name: String,
    pub(crate) replaces: Option<NotifyId>,
    pub(crate) app_icon: String,
    pub(crate) summary: String,
    pub(crate) body: String,
    /// Key and label pairs, "Each even element ... the identifier ... Each
    /// odd element ... the localized string".
    pub(crate) actions: Vec<(String, String)>,
    pub(crate) hints: Vec<(&'static str, Hint)>,
    /// "If -1, the notification's expiration time is dependent on the
    /// notification server's settings ... If 0, never expire."
    pub(crate) expire: Expire,
}

/// `expire_timeout`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Expire {
    Server,
    Never,
    After(core::time::Duration),
}

/// What a launcher shows: its progress, and whether it asks for attention.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Launcher {
    /// "progress" from 0 to 1, shown only while `Some`.
    pub(crate) progress: Option<f64>,
    pub(crate) urgent: Truth,
}

impl Launcher {
    /// No bar, no request: what a launcher shows before a program says.
    pub(crate) const IDLE: Launcher = Launcher {
        progress: None,
        urgent: Truth::False,
    };
}

/// A message's type, the header's second byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Kind(u8);

/// A header field's code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Code(u8);

/// A 32-bit value on the wire: a length, a serial, a `u`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Word(u32);

/// A count of bytes, or an offset into a message.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Len(pub(crate) usize);

/// The boundary a type's value starts on, from the message's first byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Align(usize);

impl Align {
    const ONE: Align = Align(1);
    const TWO: Align = Align(2);
    const FOUR: Align = Align(4);
    const EIGHT: Align = Align(8);
}

/// A message's serial, which its reply names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Serial(Word);

/// A message's byte order, its first byte: `l` little, `B` big.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Order {
    Little,
    Big,
}

/// The user ID the handshake names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Uid(pub(crate) u32);

// The message types and header fields of `[[source: dbus-spec]]`.
const METHOD_CALL: Kind = Kind(1);
const METHOD_RETURN: Kind = Kind(2);
const ERROR: Kind = Kind(3);
const SIGNAL: Kind = Kind(4);
const FIELD_PATH: Code = Code(1);
const FIELD_INTERFACE: Code = Code(2);
const FIELD_MEMBER: Code = Code(3);
const FIELD_ERROR_NAME: Code = Code(4);
const FIELD_REPLY_SERIAL: Code = Code(5);
const FIELD_DESTINATION: Code = Code(6);
const FIELD_SIGNATURE: Code = Code(8);

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";
const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
const LAUNCHER_ENTRY: &str = "com.canonical.Unity.LauncherEntry";
/// The object a launcher's signal comes from, `launcherentry/` and a name
/// of the sender's choosing (libunity hashes the URI), and the URI docks
/// match to an installed desktop file: the Wayland app id's.
const LAUNCHER_PATH: &str = "/com/canonical/unity/launcherentry/baud";
const LAUNCHER_URI: &str = "application://org.baud.terminal.desktop";
const FILE_MANAGER: &str = "org.freedesktop.FileManager1";
const FILE_MANAGER_PATH: &str = "/org/freedesktop/FileManager1";

/// The longest message this reads, a guard on a length a broken peer
/// claims: the spec's own maximum, 2^27 bytes.
const MESSAGE_MAX: Len = Len(1 << 27);

/// A message body or header being written, little-endian, aligned from
/// its start as the spec aligns from the message's.
#[derive(Default)]
struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn align(&mut self, n: Align) {
        while !self.buf.len().is_multiple_of(n.0) {
            self.buf.push(0);
        }
    }

    fn word(&mut self, v: Word) {
        self.align(Align::FOUR);
        self.buf.extend_from_slice(&v.0.to_le_bytes());
    }

    fn string(&mut self, s: &str) {
        self.word(Word(u32::try_from(s.len()).unwrap_or(u32::MAX)));
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }

    fn signature(&mut self, s: &str) {
        self.buf.push(u8::try_from(s.len()).unwrap_or(u8::MAX));
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }

    /// A `y`.
    fn octet(&mut self, v: Octet) {
        self.buf.push(v.0);
    }

    /// A `b`, a word of 0 or 1.
    fn truth(&mut self, t: Truth) {
        self.word(Word(match t {
            Truth::False => 0,
            Truth::True => 1,
        }));
    }

    /// A `d`, eight-aligned.
    fn double(&mut self, v: f64) {
        self.align(Align::EIGHT);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// An array: its length word, the padding to its elements' boundary,
    /// the elements, and the length -- of the elements alone -- written.
    fn array(&mut self, elem: Align, f: impl FnOnce(&mut Writer)) {
        self.word(Word(0));
        let at = self.buf.len() - 4;
        self.align(elem);
        let start = self.buf.len();
        f(self);
        let len = u32::try_from(self.buf.len() - start).unwrap_or(u32::MAX);
        self.buf[at..at + 4].copy_from_slice(&len.to_le_bytes());
    }

    /// A dictionary of variants, `a{sv}`.
    fn hints(&mut self, items: &[(&str, Hint)]) {
        self.array(Align::EIGHT, |w| {
            for (k, v) in items {
                w.align(Align::EIGHT);
                w.string(k);
                match v {
                    Hint::Byte(b) => {
                        w.signature("y");
                        w.octet(*b);
                    }
                    Hint::Bool(t) => {
                        w.signature("b");
                        w.truth(*t);
                    }
                    Hint::Str(s) => {
                        w.signature("s");
                        w.string(s);
                    }
                    Hint::Image(img) => {
                        w.signature("(iiibiiay)");
                        w.align(Align::EIGHT);
                        let (width, height) = (img.size.width, img.size.height);
                        w.word(Word(width));
                        w.word(Word(height));
                        w.word(Word(width.saturating_mul(4)));
                        w.truth(Truth::True);
                        w.word(Word(8));
                        w.word(Word(4));
                        w.array(Align::ONE, |w| w.buf.extend_from_slice(&img.rgba));
                    }
                }
            }
        });
    }

    /// An array of strings, `as`.
    fn strings(&mut self, items: &[&str]) {
        self.word(Word(0));
        let at = self.buf.len() - 4;
        let start = self.buf.len();
        for s in items {
            self.string(s);
        }
        let len = u32::try_from(self.buf.len() - start).unwrap_or(u32::MAX);
        self.buf[at..at + 4].copy_from_slice(&len.to_le_bytes());
    }
}

/// A method call: where it goes, what it calls, its body's signature and
/// the body.
struct Call<'a> {
    destination: &'a str,
    path: &'a str,
    interface: &'a str,
    member: &'a str,
    signature: &'a str,
    body: Vec<u8>,
}

/// The bytes of `call` as message `serial`: the fixed header, the fields
/// in the order libdbus and dbus-next write them -- path, interface,
/// member, destination, signature -- and the body after the header's
/// padding to eight.
fn marshal(call: &Call<'_>, serial: Serial) -> Vec<u8> {
    header_and_body(METHOD_CALL, call, serial)
}

/// A signal's bytes: a call's, with no destination, "the message is a
/// broadcast signal" (`[[source: dbus-spec]]`).
fn marshal_signal(call: &Call<'_>, serial: Serial) -> Vec<u8> {
    header_and_body(SIGNAL, call, serial)
}

fn header_and_body(kind: Kind, call: &Call<'_>, serial: Serial) -> Vec<u8> {
    let mut w = Writer::default();
    w.buf.extend_from_slice(&[b'l', kind.0, 0, 1]);
    w.word(Word(u32::try_from(call.body.len()).unwrap_or(u32::MAX)));
    w.word(serial.0);
    w.word(Word(0));
    let start = w.buf.len();
    let field = |w: &mut Writer, code: Code, sig: &str, value: &str| {
        w.align(Align::EIGHT);
        w.buf.push(code.0);
        w.signature(sig);
        match sig {
            "g" => w.signature(value),
            _ => w.string(value),
        }
    };
    field(&mut w, FIELD_PATH, "o", call.path);
    field(&mut w, FIELD_INTERFACE, "s", call.interface);
    field(&mut w, FIELD_MEMBER, "s", call.member);
    if !call.destination.is_empty() {
        field(&mut w, FIELD_DESTINATION, "s", call.destination);
    }
    if !call.signature.is_empty() {
        field(&mut w, FIELD_SIGNATURE, "g", call.signature);
    }
    let len = u32::try_from(w.buf.len() - start).unwrap_or(u32::MAX);
    w.buf[12..16].copy_from_slice(&len.to_le_bytes());
    w.align(Align::EIGHT);
    w.buf.extend_from_slice(&call.body);
    w.buf
}

/// A message being read: its bytes, its byte order, and a cursor aligned
/// from the message's start.
struct Reader<'a> {
    buf: &'a [u8],
    at: Len,
    order: Order,
}

/// A 32-bit word in `order`.
fn word_in(b: [u8; 4], order: Order) -> Word {
    Word(match order {
        Order::Big => u32::from_be_bytes(b),
        Order::Little => u32::from_le_bytes(b),
    })
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8], order: Order) -> Reader<'a> {
        Reader {
            buf,
            at: Len(0),
            order,
        }
    }

    fn align(&mut self, n: Align) -> Option<()> {
        let to = self.at.0.next_multiple_of(n.0);
        (to <= self.buf.len()).then(|| self.at = Len(to))
    }

    fn take(&mut self, n: Len) -> Option<&'a [u8]> {
        let end = self.at.0.checked_add(n.0)?;
        let b = self.buf.get(self.at.0..end)?;
        self.at = Len(end);
        Some(b)
    }

    fn word(&mut self) -> Option<Word> {
        self.align(Align::FOUR)?;
        let b: [u8; 4] = self.take(Len(4))?.try_into().ok()?;
        Some(word_in(b, self.order))
    }

    /// A length word, as a count of bytes.
    fn len(&mut self) -> Option<Len> {
        usize::try_from(self.word()?.0).ok().map(Len)
    }

    fn string(&mut self) -> Option<&'a str> {
        let n = self.len()?;
        let s = self.take(n)?;
        self.take(Len(1))?;
        core::str::from_utf8(s).ok()
    }

    fn signature(&mut self) -> Option<&'a str> {
        let n = Len(usize::from(self.take(Len(1))?[0]));
        let s = self.take(n)?;
        self.take(Len(1))?;
        core::str::from_utf8(s).ok()
    }

    /// Skip one complete value of the single complete type at the start of
    /// `sig`, and say what is left of `sig`.
    fn skip<'s>(&mut self, sig: &'s str) -> Option<&'s str> {
        let (first, rest) = (sig.as_bytes().first()?, &sig[1..]);
        match first {
            b'y' => self.take(Len(1)).map(|_| rest),
            b'n' | b'q' => self
                .align(Align::TWO)
                .and_then(|()| self.take(Len(2)))
                .map(|_| rest),
            b'b' | b'i' | b'u' | b'h' => self.word().map(|_| rest),
            b'x' | b't' | b'd' => self
                .align(Align::EIGHT)
                .and_then(|()| self.take(Len(8)))
                .map(|_| rest),
            b's' | b'o' => self.string().map(|_| rest),
            b'g' => self.signature().map(|_| rest),
            b'v' => {
                let inner = self.signature()?;
                match self.skip(inner)? {
                    "" => Some(rest),
                    _ => None,
                }
            }
            b'a' => {
                let n = self.len()?;
                let after = one_type(rest)?;
                self.align(alignment(rest))?;
                self.take(n)?;
                Some(after)
            }
            b'(' | b'{' => {
                self.align(Align::EIGHT)?;
                let close = if *first == b'(' { b')' } else { b'}' };
                let mut inner = rest;
                while inner.as_bytes().first() != Some(&close) {
                    inner = self.skip(inner)?;
                }
                Some(&inner[1..])
            }
            _ => None,
        }
    }
}

/// What is left of `sig` after its first complete type.
fn one_type(sig: &str) -> Option<&str> {
    let b = sig.as_bytes();
    match b.first()? {
        b'a' => one_type(&sig[1..]),
        b'(' | b'{' => {
            let mut depth = 0usize;
            for (i, c) in b.iter().enumerate() {
                match c {
                    b'(' | b'{' => depth += 1,
                    b')' | b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(&sig[i + 1..]);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        _ => Some(&sig[1..]),
    }
}

/// The alignment of the type at the start of `sig`.
fn alignment(sig: &str) -> Align {
    match sig.as_bytes().first() {
        Some(b'y' | b'g' | b'v') => Align::ONE,
        Some(b'n' | b'q') => Align::TWO,
        Some(b'x' | b't' | b'd' | b'(' | b'{') => Align::EIGHT,
        _ => Align::FOUR,
    }
}

/// A message read off the wire: its type, the header fields this reads,
/// its byte order and its body.
struct Message<'a> {
    kind: Kind,
    order: Order,
    reply_serial: Option<Serial>,
    path: Option<&'a str>,
    interface: Option<&'a str>,
    member: Option<&'a str>,
    error: Option<&'a str>,
    signature: &'a str,
    body: &'a [u8],
}

/// How long the message at the start of `buf` is, once its fixed header
/// and its field array's length are in, or `None` while they are not.
fn message_len(buf: &[u8]) -> Option<Result<Len, ()>> {
    let order = match buf.first()? {
        b'l' => Order::Little,
        b'B' => Order::Big,
        _ => return Some(Err(())),
    };
    let mut r = Reader::new(buf.get(..16)?, order);
    r.take(Len(4))?;
    let body = r.len()?;
    r.word()?;
    let fields = r.len()?;
    let len = (16usize.saturating_add(fields.0))
        .next_multiple_of(8)
        .saturating_add(body.0);
    match Len(len) > MESSAGE_MAX {
        true => Some(Err(())),
        false => Some(Ok(Len(len))),
    }
}

/// The message `buf` holds, whole.
fn parse(buf: &[u8]) -> Option<Message<'_>> {
    let order = match buf.first()? {
        b'B' => Order::Big,
        _ => Order::Little,
    };
    let mut r = Reader::new(buf, order);
    r.take(Len(1))?;
    let kind = Kind(r.take(Len(1))?[0]);
    r.take(Len(2))?;
    let body_len = r.len()?;
    r.word()?;
    let fields_len = r.len()?;
    let end = Len(r.at.0.checked_add(fields_len.0)?);
    let mut m = Message {
        kind,
        order,
        reply_serial: None,
        path: None,
        interface: None,
        member: None,
        error: None,
        signature: "",
        body: &[],
    };
    while r.at < end {
        r.align(Align::EIGHT)?;
        let code = Code(r.take(Len(1))?[0]);
        let sig = r.signature()?;
        match (code, sig) {
            (FIELD_PATH, "o") => m.path = Some(r.string()?),
            (FIELD_INTERFACE, "s") => m.interface = Some(r.string()?),
            (FIELD_MEMBER, "s") => m.member = Some(r.string()?),
            (FIELD_ERROR_NAME, "s") => m.error = Some(r.string()?),
            (FIELD_REPLY_SERIAL, "u") => m.reply_serial = Some(Serial(r.word()?)),
            (FIELD_SIGNATURE, "g") => m.signature = r.signature()?,
            _ => {
                r.skip(sig)?;
            }
        }
    }
    r.at = end;
    r.align(Align::EIGHT)?;
    m.body = r.take(body_len)?;
    Some(m)
}

/// The appearance namespace's three keys a `ReadAll` reply holds as `u`s,
/// each `None` where it is absent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Appearance {
    scheme: Option<Scheme>,
    contrast: Option<Contrast>,
    motion: Option<Motion>,
}

/// **The appearance in a `ReadAll` reply's `a{sa{sv}}`**, in one pass, or
/// `None` when the reply cannot be read.
fn appearance_in_read_all(m: &Message<'_>) -> Option<Appearance> {
    if m.signature != "a{sa{sv}}" {
        return None;
    }
    let mut found = Appearance::default();
    let mut r = Reader::new(m.body, m.order);
    let outer = r.len()?;
    r.align(Align::EIGHT)?;
    let end = Len(r.at.0.checked_add(outer.0)?);
    while r.at < end {
        r.align(Align::EIGHT)?;
        let namespace = r.string()?;
        let inner = r.len()?;
        r.align(Align::EIGHT)?;
        let inner_end = Len(r.at.0.checked_add(inner.0)?);
        while r.at < inner_end {
            r.align(Align::EIGHT)?;
            let key = r.string()?;
            let sig = r.signature()?;
            match (namespace == APPEARANCE, key, sig) {
                (true, "color-scheme", "u") => found.scheme = Some(Scheme::of(r.word()?)),
                (true, "contrast", "u") => found.contrast = Some(Contrast::of(r.word()?)),
                (true, "reduced-motion", "u") => found.motion = Some(Motion::of(r.word()?)),
                _ => {
                    r.skip(sig)?;
                }
            }
        }
        r.at = inner_end;
    }
    Some(found)
}

/// What a `ReadAll` reply says: the scheme, or that there is none, then
/// the contrast and the motion where the reply holds them.
fn heard_in_read_all(m: &Message<'_>) -> Vec<Heard> {
    let Some(a) = appearance_in_read_all(m) else {
        return vec![Heard::NoScheme(
            "the portal's reply could not be read".into(),
        )];
    };
    let scheme = match a.scheme {
        Some(s) => Heard::Scheme(s, Origin::Read),
        None => Heard::NoScheme("no color-scheme in the portal's reply".into()),
    };
    core::iter::once(scheme)
        .chain(a.contrast.map(Heard::Contrast))
        .chain(a.motion.map(Heard::Motion))
        .collect()
}

/// An appearance key in a `SettingChanged` signal's `(ssv)`, as what it
/// means.
fn heard_in_signal(m: &Message<'_>) -> Option<Heard> {
    let ours = m.kind == SIGNAL
        && m.interface == Some(SETTINGS)
        && m.member == Some("SettingChanged")
        && m.path == Some(PORTAL_PATH)
        && m.signature == "ssv";
    if !ours {
        return None;
    }
    let mut r = Reader::new(m.body, m.order);
    let namespace = r.string()?;
    let key = r.string()?;
    let sig = r.signature()?;
    if namespace != APPEARANCE || sig != "u" {
        return None;
    }
    match key {
        "color-scheme" => Some(Heard::Scheme(Scheme::of(r.word()?), Origin::Changed)),
        "contrast" => Some(Heard::Contrast(Contrast::of(r.word()?))),
        "reduced-motion" => Some(Heard::Motion(Motion::of(r.word()?))),
        _ => None,
    }
}

/// `GetCapabilities`' `as`, as the two this reads.
fn caps_in_reply(m: &Message<'_>) -> crate::notify::ServerCaps {
    use crate::notify::{Caps, ServerCaps};
    let mut caps = ServerCaps {
        actions: Caps::Absent,
        markup: Caps::Absent,
    };
    if m.signature != "as" {
        return caps;
    }
    let mut r = Reader::new(m.body, m.order);
    let Some(n) = r.len() else {
        return caps;
    };
    let end = Len(r.at.0.saturating_add(n.0));
    while r.at < end {
        match r.string() {
            Some("actions") => caps.actions = Caps::Listed,
            Some("body-markup") => caps.markup = Caps::Listed,
            Some(_) => {}
            None => break,
        }
    }
    caps
}

/// `Notify`'s `u`.
fn id_in_reply(m: &Message<'_>) -> Option<NotifyId> {
    if m.signature != "u" {
        return None;
    }
    Reader::new(m.body, m.order).word().map(NotifyId)
}

/// What one of the server's signals says.
enum FromServer {
    Heard(Heard),
    /// `ActivationToken(u, s)`: kept for the click on that notification.
    Token(NotifyId, String),
}

/// `ActionInvoked(u, s)`, `ActivationToken(u, s)` and
/// `NotificationClosed(u, u)` from the server.
fn heard_from_server(m: &Message<'_>) -> Option<FromServer> {
    if m.interface != Some(NOTIFICATIONS) || m.path != Some(NOTIFICATIONS_PATH) {
        return None;
    }
    let mut r = Reader::new(m.body, m.order);
    match (m.member, m.signature) {
        (Some("ActionInvoked"), "us") => {
            let id = NotifyId(r.word()?);
            Some(FromServer::Heard(Heard::Invoked(
                id,
                r.string()?.to_string(),
                None,
            )))
        }
        (Some("ActivationToken"), "us") => {
            let id = NotifyId(r.word()?);
            Some(FromServer::Token(id, r.string()?.to_string()))
        }
        (Some("NotificationClosed"), "uu") => {
            Some(FromServer::Heard(Heard::NotifyClosed(NotifyId(r.word()?))))
        }
        _ => None,
    }
}

/// Where the handshake is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Auth {
    /// `AUTH` sent; waiting for `OK`, or for `DATA` when no identity was
    /// sent with it.
    Sent,
    /// `BEGIN` and `Hello` sent: messages flow.
    Done,
    Failed,
}

/// The calls whose replies this waits for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Waiting {
    ReadAll(Serial),
    ShowItems(Serial),
    Caps(Serial),
    Notify(Serial, baud_core::NoticeToken),
}

/// **The protocol over bytes**: what to send, and what the bytes that came
/// mean. No I/O, so a test drives it with a peer's bytes.
pub(crate) struct Session {
    auth: Auth,
    out: Vec<u8>,
    /// Calls made before the handshake ended, sent behind `Hello`.
    queued: Vec<Vec<u8>>,
    inb: Vec<u8>,
    serial: Serial,
    waiting: Vec<Waiting>,
    /// The last `ActivationToken` the notification server sent, for the
    /// notification it named: handed over with that notification's next
    /// click, and dropped by a click or a close of it.
    token: Option<(NotifyId, String)>,
}

impl Session {
    /// A session whose first bytes are the handshake: the credentials byte
    /// and `AUTH EXTERNAL`, with the user ID as the identity when it is
    /// known, and then the appearance asked for, behind it.
    pub(crate) fn new(uid: Option<Uid>) -> Session {
        let mut out = vec![0u8];
        match uid {
            Some(uid) => {
                let hex: String = uid
                    .0
                    .to_string()
                    .bytes()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                out.extend_from_slice(format!("AUTH EXTERNAL {hex}\r\n").as_bytes());
            }
            None => out.extend_from_slice(b"AUTH EXTERNAL\r\n"),
        }
        let mut s = Session {
            auth: Auth::Sent,
            out,
            queued: Vec::new(),
            inb: Vec::new(),
            // `Hello` is serial 1, sent at `BEGIN`; the rest queue behind
            // it.
            serial: Serial(Word(2)),
            waiting: Vec::new(),
            token: None,
        };
        s.ask_appearance();
        s.ask_notifications();
        s
    }

    fn next_serial(&mut self) -> Serial {
        let n = self.serial;
        self.serial = Serial(Word(n.0.0.wrapping_add(1).max(1)));
        n
    }

    fn call(&mut self, call: &Call<'_>) -> Serial {
        let serial = self.next_serial();
        let bytes = marshal(call, serial);
        match self.auth {
            Auth::Done => self.out.extend_from_slice(&bytes),
            Auth::Sent => self.queued.push(bytes),
            Auth::Failed => {}
        }
        serial
    }

    /// `AddMatch` for the appearance's changes, then `ReadAll` of it.
    fn ask_appearance(&mut self) {
        let mut rule = Writer::default();
        rule.string(&format!(
            "type='signal',interface='{SETTINGS}',member='SettingChanged',path='{PORTAL_PATH}',arg0='{APPEARANCE}'"
        ));
        self.call(&Call {
            destination: "org.freedesktop.DBus",
            path: "/org/freedesktop/DBus",
            interface: "org.freedesktop.DBus",
            member: "AddMatch",
            signature: "s",
            body: rule.buf,
        });
        let mut names = Writer::default();
        names.strings(&[APPEARANCE]);
        let serial = self.call(&Call {
            destination: PORTAL,
            path: PORTAL_PATH,
            interface: SETTINGS,
            member: "ReadAll",
            signature: "as",
            body: names.buf,
        });
        self.waiting.push(Waiting::ReadAll(serial));
    }

    /// `AddMatch` for the notification server's signals (`ActionInvoked`,
    /// `ActivationToken`, `NotificationClosed`), then `GetCapabilities`.
    fn ask_notifications(&mut self) {
        let mut rule = Writer::default();
        rule.string(&format!(
            "type='signal',interface='{NOTIFICATIONS}',path='{NOTIFICATIONS_PATH}'"
        ));
        self.call(&Call {
            destination: "org.freedesktop.DBus",
            path: "/org/freedesktop/DBus",
            interface: "org.freedesktop.DBus",
            member: "AddMatch",
            signature: "s",
            body: rule.buf,
        });
        let serial = self.call(&Call {
            destination: NOTIFICATIONS,
            path: NOTIFICATIONS_PATH,
            interface: NOTIFICATIONS,
            member: "GetCapabilities",
            signature: "",
            body: Vec::new(),
        });
        self.waiting.push(Waiting::Caps(serial));
    }

    /// **Show a notification**: `Notify`, its reply the server's id for
    /// `token`.
    pub(crate) fn notify(&mut self, n: &Notification, token: baud_core::NoticeToken) {
        let mut b = Writer::default();
        b.string(&n.app_name);
        b.word(n.replaces.map_or(Word(0), |r| r.0));
        b.string(&n.app_icon);
        b.string(&n.summary);
        b.string(&n.body);
        let actions: Vec<&str> = n
            .actions
            .iter()
            .flat_map(|(k, l)| [k.as_str(), l.as_str()])
            .collect();
        b.strings(&actions);
        b.hints(&n.hints);
        b.word(Word(match n.expire {
            Expire::Server => u32::MAX,
            Expire::Never => 0,
            Expire::After(d) => u32::try_from(d.as_millis())
                .unwrap_or(i32::MAX.unsigned_abs())
                .min(i32::MAX.unsigned_abs()),
        }));
        let serial = self.call(&Call {
            destination: NOTIFICATIONS,
            path: NOTIFICATIONS_PATH,
            interface: NOTIFICATIONS,
            member: "Notify",
            signature: "susssasa{sv}i",
            body: b.buf,
        });
        self.waiting.push(Waiting::Notify(serial, token));
    }

    /// **Close one**: `CloseNotification`, whose `NotificationClosed`
    /// follows.
    pub(crate) fn close_notification(&mut self, id: NotifyId) {
        let mut b = Writer::default();
        b.word(id.0);
        self.call(&Call {
            destination: NOTIFICATIONS,
            path: NOTIFICATIONS_PATH,
            interface: NOTIFICATIONS,
            member: "CloseNotification",
            signature: "u",
            body: b.buf,
        });
    }

    /// **The launcher's progress and urgency**, as libunity's
    /// `LauncherEntry` sends them: `Update(app_uri, properties)`.
    pub(crate) fn launcher(&mut self, l: Launcher) {
        let mut b = Writer::default();
        b.string(LAUNCHER_URI);
        b.array(Align::EIGHT, |w| {
            w.align(Align::EIGHT);
            w.string("progress");
            w.signature("d");
            w.double(l.progress.unwrap_or(0.0));
            w.align(Align::EIGHT);
            w.string("progress-visible");
            w.signature("b");
            w.truth(match l.progress {
                Some(_) => Truth::True,
                None => Truth::False,
            });
            w.align(Align::EIGHT);
            w.string("urgent");
            w.signature("b");
            w.truth(l.urgent);
        });
        let serial = self.next_serial();
        let bytes = marshal_signal(
            &Call {
                destination: "",
                path: LAUNCHER_PATH,
                interface: LAUNCHER_ENTRY,
                member: "Update",
                signature: "sa{sv}",
                body: b.buf,
            },
            serial,
        );
        match self.auth {
            Auth::Done => self.out.extend_from_slice(&bytes),
            Auth::Sent => self.queued.push(bytes),
            Auth::Failed => {}
        }
    }

    /// **Ask the file manager to show `uri` selected in its folder.**
    pub(crate) fn reveal(&mut self, uri: &str) {
        let mut body = Writer::default();
        body.strings(&[uri]);
        body.string("");
        let serial = self.call(&Call {
            destination: FILE_MANAGER,
            path: FILE_MANAGER_PATH,
            interface: FILE_MANAGER,
            member: "ShowItems",
            signature: "ass",
            body: body.buf,
        });
        self.waiting.push(Waiting::ShowItems(serial));
    }

    /// Whether the session has failed and will say nothing more.
    pub(crate) fn failed(&self) -> bool {
        self.auth == Auth::Failed
    }

    /// The bytes waiting to be sent.
    pub(crate) fn outgoing(&self) -> &[u8] {
        &self.out
    }

    /// `n` of them went.
    pub(crate) fn sent(&mut self, n: Len) {
        self.out.drain(..n.0.min(self.out.len()));
    }

    /// **Bytes the bus sent**, and what they meant.
    pub(crate) fn received(&mut self, bytes: &[u8]) -> Vec<Heard> {
        self.inb.extend_from_slice(bytes);
        let mut heard = Vec::new();
        if self.auth == Auth::Sent {
            let Some(nl) = self.inb.windows(2).position(|w| w == b"\r\n") else {
                return heard;
            };
            let line: Vec<u8> = self.inb.drain(..nl + 2).collect();
            let line = &line[..nl];
            if line.starts_with(b"OK ") || line == b"OK" {
                self.auth = Auth::Done;
                self.out.extend_from_slice(b"BEGIN\r\n");
                let hello = marshal(
                    &Call {
                        destination: "org.freedesktop.DBus",
                        path: "/org/freedesktop/DBus",
                        interface: "org.freedesktop.DBus",
                        member: "Hello",
                        signature: "",
                        body: Vec::new(),
                    },
                    Serial(Word(1)),
                );
                self.out.extend_from_slice(&hello);
                for q in self.queued.drain(..) {
                    self.out.extend_from_slice(&q);
                }
            } else if line.starts_with(b"DATA") {
                // The empty identity: the server takes the socket's own.
                self.out.extend_from_slice(b"DATA\r\n");
                return heard;
            } else {
                self.auth = Auth::Failed;
                let said = String::from_utf8_lossy(line).into_owned();
                heard.push(Heard::Closed(format!(
                    "the bus refused the handshake: {said}"
                )));
                return heard;
            }
        }
        while let Some(len) = message_len(&self.inb) {
            let Ok(len) = len else {
                self.auth = Auth::Failed;
                heard.push(Heard::Closed(
                    "the bus sent a message this cannot read".into(),
                ));
                return heard;
            };
            if self.inb.len() < len.0 {
                break;
            }
            let bytes: Vec<u8> = self.inb.drain(..len.0).collect();
            let Some(m) = parse(&bytes) else {
                continue;
            };
            heard.extend(self.heard(&m));
        }
        heard
    }

    /// The token kept for notification `id`, taken; a token kept for
    /// another stays.
    fn token_for(&mut self, id: NotifyId) -> Option<String> {
        match self.token.take() {
            Some((kept, token)) if kept == id => Some(token),
            other => {
                self.token = other;
                None
            }
        }
    }

    /// What one message means, if it is one this waits for or a change of
    /// the appearance: a `ReadAll` reply can say three things.
    fn heard(&mut self, m: &Message<'_>) -> Vec<Heard> {
        if m.kind == SIGNAL {
            if let Some(h) = heard_in_signal(m) {
                return vec![h];
            }
            return match heard_from_server(m) {
                Some(FromServer::Token(id, token)) => {
                    self.token = Some((id, token));
                    Vec::new()
                }
                Some(FromServer::Heard(Heard::Invoked(id, key, _))) => {
                    let token = self.token_for(id);
                    vec![Heard::Invoked(id, key, token)]
                }
                Some(FromServer::Heard(h @ Heard::NotifyClosed(id))) => {
                    let _ = self.token_for(id);
                    vec![h]
                }
                Some(FromServer::Heard(h)) => vec![h],
                None => Vec::new(),
            };
        }
        let Some(reply) = m.reply_serial else {
            return Vec::new();
        };
        let Some(i) = self.waiting.iter().position(|w| match w {
            Waiting::ReadAll(s)
            | Waiting::ShowItems(s)
            | Waiting::Caps(s)
            | Waiting::Notify(s, _) => *s == reply,
        }) else {
            return Vec::new();
        };
        let waited = self.waiting.swap_remove(i);
        let error = (m.kind == ERROR).then(|| m.error.unwrap_or("an unnamed error").to_owned());
        match (waited, m.kind, error) {
            (Waiting::ReadAll(_), METHOD_RETURN, _) => heard_in_read_all(m),
            (Waiting::ReadAll(_), _, Some(e)) => vec![Heard::NoScheme(e)],
            (Waiting::ShowItems(_), METHOD_RETURN, _) => vec![Heard::Revealed],
            (Waiting::ShowItems(_), _, Some(e)) => vec![Heard::NotRevealed(e)],
            (Waiting::Caps(_), METHOD_RETURN, _) => vec![Heard::Caps(caps_in_reply(m))],
            (Waiting::Caps(_), _, Some(e)) => vec![Heard::NoServer(e)],
            (Waiting::Notify(_, token), METHOD_RETURN, _) => match id_in_reply(m) {
                Some(id) => vec![Heard::Notified(token, id)],
                None => vec![Heard::NotNotified(token, "a reply with no id".into())],
            },
            (Waiting::Notify(_, token), _, Some(e)) => vec![Heard::NotNotified(token, e)],
            _ => Vec::new(),
        }
    }
}

/// Where the session bus is, from `DBUS_SESSION_BUS_ADDRESS`, else
/// `$XDG_RUNTIME_DIR/bus`: each `unix:` address with a `path` or an
/// `abstract` name, in order, the rest skipped.
pub(crate) fn addresses(env: Option<&str>, runtime: Option<&str>) -> Vec<Address> {
    let Some(env) = env.filter(|e| !e.is_empty() && *e != "autolaunch:") else {
        return runtime
            .map(|dir| Address::Path(format!("{dir}/bus")))
            .into_iter()
            .collect();
    };
    env.split(';')
        .filter_map(|a| a.strip_prefix("unix:"))
        .filter_map(|keys| {
            keys.split(',').find_map(|kv| match kv.split_once('=') {
                Some(("path", v)) => Some(Address::Path(unescape(v))),
                Some(("abstract", v)) => Some(Address::Abstract(unescape(v))),
                _ => None,
            })
        })
        .collect()
}

/// A value with the spec's `%xx` escapes undone.
fn unescape(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).copied().and_then(hex),
            b.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push(u8::try_from(h * 16 + l).unwrap_or(b'?'));
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A session bus address.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Address {
    Path(String),
    Abstract(String),
}

/// **The session bus, connected**: the socket and the session over it.
pub(crate) struct Bus {
    stream: UnixStream,
    session: Session,
}

impl Bus {
    /// The first address that connects, non-blocking, with the handshake
    /// queued; `None` with the reason when none does.
    #[cfg(target_os = "linux")]
    pub(crate) fn connect() -> Result<Bus, String> {
        use std::os::linux::net::SocketAddrExt;
        let env = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok();
        let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let all = addresses(env.as_deref(), runtime.as_deref());
        if all.is_empty() {
            return Err(
                "no session bus: DBUS_SESSION_BUS_ADDRESS and XDG_RUNTIME_DIR are unset".into(),
            );
        }
        let mut last = String::new();
        for a in all {
            let got = match &a {
                Address::Path(p) => UnixStream::connect(p),
                Address::Abstract(name) => std::os::unix::net::SocketAddr::from_abstract_name(name)
                    .and_then(|addr| UnixStream::connect_addr(&addr)),
            };
            match got {
                Ok(stream) => return Bus::over(stream, uid()).map_err(|e| e.to_string()),
                Err(e) => last = format!("{a:?}: {e}"),
            }
        }
        Err(format!("no session bus answered: {last}"))
    }

    /// The session over `stream`, made non-blocking.
    pub(crate) fn over(stream: UnixStream, uid: Option<Uid>) -> io::Result<Bus> {
        stream.set_nonblocking(true)?;
        Ok(Bus {
            stream,
            session: Session::new(uid),
        })
    }

    /// The socket, for the wait.
    pub(crate) fn as_fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }

    /// Whether the wait should wake for room to write.
    pub(crate) fn wants_write(&self) -> bool {
        !self.session.outgoing().is_empty()
    }

    /// Ask the file manager to show `uri`.
    pub(crate) fn reveal(&mut self, uri: &str) {
        self.session.reveal(uri);
        let _ = self.flush();
    }

    /// Show a notification.
    pub(crate) fn notify(&mut self, n: &Notification, token: baud_core::NoticeToken) {
        self.session.notify(n, token);
        let _ = self.flush();
    }

    /// Close one.
    pub(crate) fn close_notification(&mut self, id: NotifyId) {
        self.session.close_notification(id);
        let _ = self.flush();
    }

    /// The launcher's progress and urgency.
    pub(crate) fn launcher(&mut self, l: Launcher) {
        self.session.launcher(l);
        let _ = self.flush();
    }

    fn flush(&mut self) -> io::Result<()> {
        while !self.session.outgoing().is_empty() {
            match self.stream.write(self.session.outgoing()) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => self.session.sent(Len(n)),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// **Move what the kernel takes and read what has come**, never
    /// waiting; what the bus said. A closed or failed bus says so once and
    /// then nothing.
    pub(crate) fn pump(&mut self) -> Vec<Heard> {
        if self.session.failed() {
            return Vec::new();
        }
        if let Err(e) = self.flush() {
            self.session.auth = Auth::Failed;
            return vec![Heard::Closed(format!("the session bus: {e}"))];
        }
        let mut heard = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    self.session.auth = Auth::Failed;
                    heard.push(Heard::Closed(
                        "the session bus closed the connection".into(),
                    ));
                    return heard;
                }
                Ok(n) => {
                    heard.extend(self.session.received(&buf[..n]));
                    if self.session.failed() {
                        return heard;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    self.session.auth = Auth::Failed;
                    heard.push(Heard::Closed(format!("the session bus: {e}")));
                    return heard;
                }
            }
        }
        // The handshake's answer may have queued `BEGIN` and the calls.
        let _ = self.flush();
        heard
    }
}

/// This process's user ID, the identity `EXTERNAL` sends: the owner of
/// `/proc/self`, which is the process's own.
#[cfg(target_os = "linux")]
fn uid() -> Option<Uid> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| Uid(m.uid()))
}

/// **One notification as `Notify` takes it** (`[[source:
/// freedesktop-notification-spec]]`): the program's application name or
/// baud's; its first icon name as the theme names it; the text, escaped
/// where the server reads markup; a `default` action where a click is
/// reported or asked to focus the window, and its buttons numbered from 1; the urgency, the sound, the
/// type as `category` and the image, decoded (`Delivery::image_data`), as
/// `image-data`; and its expiry.
///
/// **iTerm2's subtitle**, which `Notify` has no argument for, opens the
/// body as its own paragraph, a blank line before the message -- "Each
/// line is a paragraph" -- as Chromium's Linux notification bridge sets a
/// notification's context line above its message (`[[source:
/// chromium-fc2ff62-notification-bridge-linux]]`); with no title it is
/// the summary, which is the one text argument the text does not call
/// optional.
pub(crate) fn notification(
    p: &crate::notify::Posted,
    replaces: Option<NotifyId>,
    caps: crate::notify::ServerCaps,
    image: Option<(baud_domain::png::Size, Vec<u8>)>,
) -> Notification {
    use crate::notify::{Caps, Sound};
    use {Expire, Hint, Image, Octet, Truth};
    let text = |s: &str| match caps.markup {
        Caps::Listed => crate::notify::escape_markup(s),
        Caps::Absent => s.to_string(),
    };
    let mut actions = Vec::new();
    if p.activation.reports() || p.activation.focuses() {
        actions.push(("default".to_string(), "Open".to_string()));
    }
    for (i, b) in p.buttons.iter().enumerate() {
        actions.push(((i + 1).to_string(), b.clone()));
    }
    let mut hints: Vec<(&'static str, Hint)> = vec![(
        "urgency",
        Hint::Byte(Octet(match p.urgency {
            baud_core::Urgency::Low => 0,
            baud_core::Urgency::Normal => 1,
            baud_core::Urgency::Critical => 2,
        })),
    )];
    match &p.sound {
        Sound::System => {}
        Sound::Silent => hints.push(("suppress-sound", Hint::Bool(Truth::True))),
        Sound::Named(n) => hints.push(("sound-name", Hint::Str(n.clone()))),
    }
    if !p.kind.is_empty() {
        hints.push(("category", Hint::Str(p.kind.clone())));
    }
    if let Some((size, rgba)) = image {
        hints.push(("image-data", Hint::Image(Image { size, rgba })));
    }
    let (summary, body) = match (p.title.is_empty(), p.subtitle.is_empty(), p.body.is_empty()) {
        (_, true, _) => (p.title.clone(), p.body.clone()),
        (true, false, _) => (p.subtitle.clone(), p.body.clone()),
        (false, false, true) => (p.title.clone(), p.subtitle.clone()),
        (false, false, false) => (p.title.clone(), format!("{}\n\n{}", p.subtitle, p.body)),
    };
    Notification {
        app_name: match p.app.is_empty() {
            true => "baud".to_string(),
            false => p.app.clone(),
        },
        replaces,
        app_icon: p
            .icon_names
            .first()
            .map(|n| crate::notify::icon_name(n).to_string())
            .unwrap_or_default(),
        summary,
        body: text(&body),
        actions,
        hints,
        expire: match p.expiry {
            baud_core::Expiry::System => Expire::Server,
            baud_core::Expiry::Never => Expire::Never,
            baud_core::Expiry::After(d) => Expire::After(d),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// `Hello` as dbus-next 0.2.3 marshals it, an implementation this
    /// shares no code with: `Message(destination='org.freedesktop.DBus',
    /// path='/org/freedesktop/DBus', interface='org.freedesktop.DBus',
    /// member='Hello', serial=1)._marshall()`.
    const HELLO: &str = "6c01000100000000010000006d00000001016f00150000002f6f72672f667265656465736b746f702f4442757300000002017300140000006f72672e667265656465736b746f702e4442757300000000030173000500000048656c6c6f00000006017300140000006f72672e667265656465736b746f702e4442757300000000";

    /// `ReadAll(["org.freedesktop.appearance"])` to the portal as serial 3,
    /// as dbus-next marshals it.
    const READ_ALL: &str = include_str!("../fixtures/bus-read-all.hex");

    /// **A call's bytes are another implementation's bytes.** Perturbation:
    /// leave the header's padding out -- both comparisons fail.
    #[test]
    fn calls_marshal_as_dbus_next_marshals_them() {
        let hello = marshal(
            &Call {
                destination: "org.freedesktop.DBus",
                path: "/org/freedesktop/DBus",
                interface: "org.freedesktop.DBus",
                member: "Hello",
                signature: "",
                body: Vec::new(),
            },
            Serial(Word(1)),
        );
        assert_eq!(hello, hex(HELLO));
        let mut names = Writer::default();
        names.strings(&[APPEARANCE]);
        let read_all = marshal(
            &Call {
                destination: PORTAL,
                path: PORTAL_PATH,
                interface: SETTINGS,
                member: "ReadAll",
                signature: "as",
                body: names.buf,
            },
            Serial(Word(3)),
        );
        assert_eq!(read_all, hex(READ_ALL.trim()));
    }

    /// **The handshake, then `Hello`, then the calls queued behind it**:
    /// the identity in hex, `BEGIN` only after `OK`, and the empty identity
    /// answered with an empty `DATA`. A refusal fails the session, said.
    #[test]
    fn the_handshake_sends_begin_and_hello_before_any_call() {
        let mut s = Session::new(Some(Uid(1000)));
        assert_eq!(s.outgoing(), b"\0AUTH EXTERNAL 31303030\r\n");
        let n = s.outgoing().len();
        s.sent(Len(n));
        assert_eq!(s.received(b"OK 1234deadbeef\r\n"), []);
        let out = s.outgoing().to_vec();
        assert!(out.starts_with(b"BEGIN\r\n"), "{out:?}");
        assert_eq!(&out[7..7 + hex(HELLO).len()], hex(HELLO).as_slice());
        let after = &out[7 + hex(HELLO).len()..];
        let add = message_len(after).unwrap().unwrap();
        assert_eq!(&after[..add.0], reply_bytes("add_match_call").as_slice());
        let rest = &after[add.0..];
        let read_all = message_len(rest).unwrap().unwrap();
        assert_eq!(&rest[..read_all.0], hex(READ_ALL.trim()).as_slice());
        // Then the notification server's signals matched and its
        // capabilities asked.
        let rest = &rest[read_all.0..];
        let add = message_len(rest).unwrap().unwrap();
        assert_eq!(
            &rest[..add.0],
            reply_bytes("notify_add_match_call").as_slice()
        );
        assert_eq!(
            &rest[add.0..],
            reply_bytes("get_capabilities_call").as_slice()
        );

        let mut anon = Session::new(None);
        assert_eq!(anon.outgoing(), b"\0AUTH EXTERNAL\r\n");
        let n = anon.outgoing().len();
        anon.sent(Len(n));
        assert_eq!(anon.received(b"DATA\r\n"), []);
        assert_eq!(anon.outgoing(), b"DATA\r\n");

        let mut no = Session::new(Some(Uid(1)));
        let h = no.received(b"REJECTED KERBEROS_V4\r\n");
        assert!(matches!(h.as_slice(), [Heard::Closed(_)]), "{h:?}");
        assert!(no.failed());
    }

    /// A message dbus-next 0.2.3 marshalled (`fixtures/bus-replies.hex`,
    /// one `name hex` a line).
    fn reply_bytes(name: &str) -> Vec<u8> {
        hex(include_str!("../fixtures/bus-replies.hex")
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{name} ")))
            .unwrap_or_else(|| panic!("no fixture {name}"))
            .trim())
    }

    fn ready() -> Session {
        let mut s = Session::new(Some(Uid(1000)));
        s.received(b"OK 1234deadbeef\r\n");
        let n = s.outgoing().len();
        s.sent(Len(n));
        s
    }

    /// **The portal's answer, read and changed**: `ReadAll`'s reply with
    /// `color-scheme` beside `accent-color`'s `(ddd)` and `contrast`, each
    /// value 0, 1 and 2, an unknown value as no preference, a change
    /// signalled, an error, and a reply split across reads. Fixtures are
    /// dbus-next's bytes. Perturbation: skip no padding before a dict
    /// entry -- every reply is unread.
    #[test]
    fn the_portals_scheme_is_read_and_followed() {
        const NORMAL: [Heard; 2] = [
            Heard::Contrast(Contrast::NoPreference),
            Heard::Motion(Motion::NoPreference),
        ];
        for (fixture, want) in [
            ("read_all_dark", Heard::Scheme(Scheme::Dark, Origin::Read)),
            ("read_all_light", Heard::Scheme(Scheme::Light, Origin::Read)),
            (
                "read_all_none",
                Heard::Scheme(Scheme::NoPreference, Origin::Read),
            ),
            (
                "read_all_unknown",
                Heard::Scheme(Scheme::NoPreference, Origin::Read),
            ),
        ] {
            let mut s = ready();
            let bytes = reply_bytes(fixture);
            let (a, b) = bytes.split_at(bytes.len() / 2);
            assert_eq!(s.received(a), [], "{fixture}: half a reply");
            let mut all = vec![want];
            all.extend(NORMAL);
            assert_eq!(s.received(b), all, "{fixture}");
        }
        let mut s = ready();
        let mut missing = vec![Heard::NoScheme(
            "no color-scheme in the portal's reply".into(),
        )];
        missing.extend(NORMAL);
        assert_eq!(s.received(&reply_bytes("read_all_missing")), missing);
        let mut s = ready();
        assert_eq!(
            s.received(&reply_bytes("read_all_error")),
            [Heard::NoScheme(
                "org.freedesktop.DBus.Error.ServiceUnknown".into()
            )]
        );
        let mut s = ready();
        assert_eq!(
            s.received(&reply_bytes("changed_light")),
            [Heard::Scheme(Scheme::Light, Origin::Changed)]
        );
        assert_eq!(s.received(&reply_bytes("name_acquired")), []);
    }

    /// **The desktop's contrast and motion preferences, read and
    /// followed** (`[[source: xdg-portal-settings-v2]]`): `ReadAll`'s
    /// reply holding both at 1 says higher contrast and reduced motion
    /// beside the scheme; an unknown value is no preference, as the
    /// interface says; a version-1 portal's reply, with neither key, says
    /// nothing of them; a reply with no scheme still says them; each key's
    /// `SettingChanged`, to 1 and back to 0. Fixtures are dbus-next's
    /// bytes. Perturbations: read `contrast` 2 as higher -- the unknown
    /// value's assertion fails; drop the signal's `reduced-motion` arm --
    /// the change is unheard.
    #[test]
    fn the_portals_contrast_and_motion_are_read_and_followed() {
        let read = |fixture: &str| ready().received(&reply_bytes(fixture));
        assert_eq!(
            read("read_all_accessible"),
            [
                Heard::Scheme(Scheme::Dark, Origin::Read),
                Heard::Contrast(Contrast::Higher),
                Heard::Motion(Motion::Reduced),
            ]
        );
        assert_eq!(
            read("read_all_unknown_access"),
            [
                Heard::Scheme(Scheme::Light, Origin::Read),
                Heard::Contrast(Contrast::NoPreference),
                Heard::Motion(Motion::NoPreference),
            ]
        );
        assert_eq!(
            read("read_all_v1"),
            [Heard::Scheme(Scheme::Dark, Origin::Read)]
        );
        assert_eq!(
            read("read_all_access_only"),
            [
                Heard::NoScheme("no color-scheme in the portal's reply".into()),
                Heard::Contrast(Contrast::Higher),
                Heard::Motion(Motion::Reduced),
            ]
        );
        let mut s = ready();
        for (fixture, want) in [
            ("changed_contrast", Heard::Contrast(Contrast::Higher)),
            (
                "changed_contrast_off",
                Heard::Contrast(Contrast::NoPreference),
            ),
            ("changed_motion", Heard::Motion(Motion::Reduced)),
            ("changed_motion_off", Heard::Motion(Motion::NoPreference)),
        ] {
            assert_eq!(s.received(&reply_bytes(fixture)), [want], "{fixture}");
        }
    }

    /// **A reveal is `ShowItems` of the one URI and an empty startup ID**,
    /// and its reply or error is said. Perturbation: send the URI as a bare
    /// string -- the signature and the bytes differ.
    #[test]
    fn a_reveal_asks_the_file_manager() {
        let mut s = ready();
        s.reveal("file:///tmp/x");
        let out = s.outgoing().to_vec();
        assert_eq!(out, reply_bytes("show_items_call"));
        let n = out.len();
        s.sent(Len(n));
        assert_eq!(s.received(&reply_bytes("show_items_ok")), [Heard::Revealed]);
        s.reveal("file:///tmp/x");
        assert_eq!(
            s.received(&reply_bytes("show_items_error")),
            [Heard::NotRevealed(
                "org.freedesktop.DBus.Error.ServiceUnknown".into()
            )]
        );
    }

    /// **The address**: the environment's `unix:` entries in order, with
    /// their escapes undone, else the runtime directory's `bus`; an
    /// address with neither a path nor an abstract name is skipped.
    #[test]
    fn the_session_bus_address_is_the_environments_or_the_runtime_dirs() {
        assert_eq!(
            addresses(
                Some("unix:path=/run/user/1000/b%2cus;tcp:host=x;unix:abstract=/tmp/dbus-X,guid=1"),
                Some("/run/user/1000")
            ),
            [
                Address::Path("/run/user/1000/b,us".into()),
                Address::Abstract("/tmp/dbus-X".into())
            ]
        );
        assert_eq!(
            addresses(None, Some("/run/user/7")),
            [Address::Path("/run/user/7/bus".into())]
        );
        assert_eq!(addresses(Some("autolaunch:"), None), []);
        assert_eq!(addresses(Some("unix:tmpdir=/tmp"), None), []);
    }

    /// **Over a socket**: the handshake goes out, the bus's answer and the
    /// reply come in, a reveal goes out at once, and a closed bus is said
    /// once. Perturbation: take a read of nothing for no news -- the closed
    /// bus is never said and the assertion after the drop fails.
    #[test]
    fn the_bus_moves_bytes_without_waiting() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        let mut bus = Bus::over(ours, Some(Uid(1000))).unwrap();
        assert!(bus.wants_write());
        assert_eq!(bus.pump(), []);
        let mut got = vec![0u8; 64];
        let n = theirs.read(&mut got).unwrap();
        assert_eq!(&got[..n], b"\0AUTH EXTERNAL 31303030\r\n");
        theirs.write_all(b"OK 0123\r\n").unwrap();
        assert_eq!(bus.pump(), []);
        let mut sink = vec![0u8; 4096];
        let n = theirs.read(&mut sink).unwrap();
        assert!(sink[..n].starts_with(b"BEGIN\r\n"), "{:?}", &sink[..n]);
        theirs.write_all(&reply_bytes("read_all_light")).unwrap();
        assert_eq!(
            bus.pump(),
            [
                Heard::Scheme(Scheme::Light, Origin::Read),
                Heard::Contrast(Contrast::NoPreference),
                Heard::Motion(Motion::NoPreference),
            ]
        );
        // A reveal goes out at once, behind nothing.
        bus.reveal("file:///tmp/x");
        assert!(!bus.wants_write());
        let mut call = vec![0u8; 256];
        let n = theirs.read(&mut call).unwrap();
        assert_eq!(&call[..n], reply_bytes("show_items_call").as_slice());
        use std::os::fd::AsRawFd;
        assert_eq!(bus.as_fd().as_raw_fd(), bus.stream.as_raw_fd());
        drop(theirs);
        assert!(matches!(bus.pump().as_slice(), [Heard::Closed(_)]));
        assert_eq!(bus.pump(), [], "said once");
    }

    /// The notice a program's `OSC 99 ; i=a ; x` publishes, for its token.
    fn token() -> baud_core::NoticeToken {
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut owned =
            baud_core::Owned::new(&ws).with_notices(baud_core::NoticeBudget::with_icon(1024));
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        t.set_notice_support(crate::notify::support(
            crate::notify::Setting::On,
            crate::notify::Platform::MacOs,
        ));
        let b = b"\x1b]99;i=a;x\x1b\\";
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        let tok = t.notices().next().expect("published").token;
        tok
    }

    /// **`Notify`, `CloseNotification` and the launcher's `Update` are
    /// dbus-next's bytes** (`[[source: freedesktop-notification-spec]]`,
    /// `[[source: libunity-launcher-entry]]`): every hint type the driver
    /// sends -- a byte, a boolean, a string and an image's `(iiibiiay)` --
    /// actions in pairs, `replaces_id`, `expire_timeout` in milliseconds
    /// and -1 for the server's. Perturbation: leave a dict entry's padding
    /// out -- the first call differs.
    #[test]
    fn notifications_marshal_as_dbus_next_marshals_them() {
        let mut s = ready();
        let tok = token();
        s.notify(
            &Notification {
                app_name: "baud".into(),
                replaces: None,
                app_icon: "dialog-error".into(),
                summary: "Build".into(),
                body: "make; make install".into(),
                actions: vec![
                    ("default".into(), "Open".into()),
                    ("1".into(), "Yes".into()),
                ],
                hints: vec![
                    ("urgency", Hint::Byte(Octet(2))),
                    ("suppress-sound", Hint::Bool(Truth::True)),
                    ("sound-name", Hint::Str("bell".into())),
                    (
                        "image-data",
                        Hint::Image(Image {
                            size: baud_domain::png::Size {
                                width: 2,
                                height: 1,
                            },
                            rgba: vec![255, 0, 0, 255, 0, 255, 0, 128],
                        }),
                    ),
                ],
                expire: Expire::After(core::time::Duration::from_millis(5000)),
            },
            tok,
        );
        assert_eq!(s.outgoing(), reply_bytes("notify_call").as_slice());
        let n = s.outgoing().len();
        s.sent(Len(n));
        s.notify(
            &Notification {
                app_name: String::new(),
                replaces: Some(NotifyId(Word(42))),
                app_icon: String::new(),
                summary: "Again".into(),
                body: String::new(),
                actions: Vec::new(),
                hints: Vec::new(),
                expire: Expire::Server,
            },
            tok,
        );
        assert_eq!(s.outgoing(), reply_bytes("notify_replace_call").as_slice());
        let n = s.outgoing().len();
        s.sent(Len(n));
        s.close_notification(NotifyId(Word(42)));
        assert_eq!(
            s.outgoing(),
            reply_bytes("close_notification_call").as_slice()
        );
        let n = s.outgoing().len();
        s.sent(Len(n));
        s.launcher(Launcher {
            progress: Some(0.5),
            urgent: Truth::False,
        });
        assert_eq!(s.outgoing(), reply_bytes("launcher_update").as_slice());
    }

    /// **What the server says back**: its capabilities, a notification's
    /// id or an error, a click with its key, a close. Fixtures are
    /// dbus-next's bytes. Perturbation: read `ActionInvoked`'s key before
    /// its id -- the click is unheard.
    #[test]
    fn the_servers_answers_are_heard() {
        use crate::notify::{Caps, ServerCaps};
        let mut s = ready();
        assert_eq!(
            s.received(&reply_bytes("capabilities_reply")),
            [Heard::Caps(ServerCaps {
                actions: Caps::Listed,
                markup: Caps::Listed,
            })]
        );
        let mut s = ready();
        assert_eq!(
            s.received(&reply_bytes("capabilities_bare")),
            [Heard::Caps(ServerCaps {
                actions: Caps::Absent,
                markup: Caps::Absent,
            })]
        );
        let tok = token();
        let quiet = Notification {
            app_name: String::new(),
            replaces: None,
            app_icon: String::new(),
            summary: "x".into(),
            body: String::new(),
            actions: Vec::new(),
            hints: Vec::new(),
            expire: Expire::Never,
        };
        s.notify(&quiet, tok);
        assert_eq!(
            s.received(&reply_bytes("notify_reply")),
            [Heard::Notified(tok, NotifyId(Word(42)))]
        );
        let mut none = ready();
        assert_eq!(
            none.received(&reply_bytes("capabilities_error")),
            [Heard::NoServer(
                "org.freedesktop.DBus.Error.ServiceUnknown".into()
            )]
        );
        let mut s = ready();
        s.notify(&quiet, tok);
        assert_eq!(
            s.received(&reply_bytes("notify_error")),
            [Heard::NotNotified(
                tok,
                "org.freedesktop.DBus.Error.ServiceUnknown".into()
            )]
        );
        assert_eq!(
            s.received(&reply_bytes("action_invoked")),
            [Heard::Invoked(NotifyId(Word(42)), "1".into(), None)]
        );
        assert_eq!(
            s.received(&reply_bytes("notification_closed")),
            [Heard::NotifyClosed(NotifyId(Word(42)))]
        );
    }

    /// **A notification, a close and a launcher update go out at once**
    /// over a socket, behind nothing.
    #[test]
    fn notifications_go_out_without_waiting() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        let mut bus = Bus::over(ours, Some(Uid(1000))).unwrap();
        assert_eq!(bus.pump(), []);
        let mut sink = vec![0u8; 4096];
        let _ = theirs.read(&mut sink).unwrap();
        theirs.write_all(b"OK 0123\r\n").unwrap();
        assert_eq!(bus.pump(), []);
        let _ = theirs.read(&mut sink).unwrap();
        bus.close_notification(NotifyId(Word(42)));
        let n = theirs.read(&mut sink).unwrap();
        assert_eq!(
            &sink[..n],
            reply_bytes("close_notification_first").as_slice()
        );
        bus.launcher(Launcher {
            progress: None,
            urgent: Truth::True,
        });
        let n = theirs.read(&mut sink).unwrap();
        assert!(n > 0 && !bus.wants_write());
        let tok = token();
        bus.notify(
            &Notification {
                app_name: String::new(),
                replaces: None,
                app_icon: String::new(),
                summary: "x".into(),
                body: String::new(),
                actions: Vec::new(),
                hints: Vec::new(),
                expire: Expire::Never,
            },
            tok,
        );
        assert!(theirs.read(&mut sink).unwrap() > 0);
    }

    /// **A program's notification as `Notify` takes it**: the type as the
    /// category, the urgency, `s=silent` as `suppress-sound`, a reported
    /// click as `default` with the buttons numbered from 1, the first icon
    /// name as the theme names it, the body escaped for a server that
    /// reads markup and not for one that does not, the expiry. Perturbation:
    /// escape for every server -- the plain body differs.
    #[test]
    fn a_posted_notification_is_notifys_arguments() {
        use crate::notify::{Caps, Icons, Posted, ServerCaps};
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut owned =
            baud_core::Owned::new(&ws).with_notices(baud_core::NoticeBudget::with_icon(1024));
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        let caps = ServerCaps {
            actions: Caps::Listed,
            markup: Caps::Listed,
        };
        t.set_notice_support(crate::notify::support(
            crate::notify::Setting::On,
            crate::notify::Platform::Linux(caps),
        ));
        let b = "\x1b]99;i=n:d=0:a=report:u=2:s=c2lsZW50:n=d2Fybg==:t=YnVpbGQ=:w=2500;Done\x1b\\\x1b]99;i=n:d=0:p=body;a < b & c\x1b\\\x1b]99;i=n:p=buttons;Retry\u{2028}Log\x1b\\".as_bytes();
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        let p = t
            .notices()
            .map(|n| Posted::of(&n, &mut Icons::default()))
            .next()
            .expect("posted");
        let n = notification(&p, None, caps, None);
        assert_eq!(n.summary, "Done");
        assert_eq!(n.body, "a &lt; b &amp; c");
        assert_eq!(n.app_icon, "dialog-warning");
        assert_eq!(n.app_name, "baud");
        assert_eq!(
            n.actions,
            [
                ("default".into(), "Open".into()),
                ("1".into(), "Retry".into()),
                ("2".into(), "Log".into())
            ]
        );
        assert_eq!(
            n.hints,
            [
                ("urgency", Hint::Byte(Octet(2))),
                ("suppress-sound", Hint::Bool(Truth::True)),
                ("category", Hint::Str("build".into())),
            ]
        );
        assert_eq!(
            n.expire,
            Expire::After(core::time::Duration::from_millis(2500))
        );
        let plain = notification(&p, Some(NotifyId(Word(9))), ServerCaps::NONE, None);
        assert_eq!(plain.body, "a < b & c");
        assert_eq!(plain.replaces, Some(NotifyId(Word(9))));
    }

    /// **iTerm2's subtitle opens the body as a paragraph of its own**, a
    /// blank line before the message, escaped with it for a server that
    /// reads markup; with no title it is the summary; with no message it is
    /// the body.
    #[test]
    fn an_iterm2_subtitle_opens_the_body_or_stands_as_the_summary() {
        use crate::notify::{Caps, Icons, Posted, ServerCaps};
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let caps = ServerCaps {
            actions: Caps::Listed,
            markup: Caps::Listed,
        };
        let sent = |b64s: &str| {
            let mut owned =
                baud_core::Owned::new(&ws).with_notices(baud_core::NoticeBudget::with_icon(1024));
            let mut t = baud_core::Term::new(owned.storage(), &ws);
            t.set_notice_support(crate::notify::support(
                crate::notify::Setting::On,
                crate::notify::Platform::Linux(caps),
            ));
            let b = format!("\x1b]1337;Notification={b64s}\x07");
            t.write_at(BytePos::ZERO, b.as_bytes());
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            let p = t
                .notices()
                .map(|n| Posted::of(&n, &mut Icons::default()))
                .next()
                .expect("posted");
            let n = notification(&p, None, caps, None);
            (n.summary, n.body)
        };
        // title=Build subtitle=a<b message=done
        let all = sent("title=QnVpbGQ=;subtitle=YTxi;message=ZG9uZQ==");
        assert_eq!(all, ("Build".into(), "a&lt;b\n\ndone".into()));
        let untitled = sent("subtitle=YTxi;message=ZG9uZQ==");
        assert_eq!(
            untitled,
            ("a<b".into(), "done".into()),
            "a summary takes no markup"
        );
        let unsaid = sent("title=QnVpbGQ=;subtitle=YTxi");
        assert_eq!(unsaid, ("Build".into(), "a&lt;b".into()));
        let plain = sent("title=QnVpbGQ=;message=ZG9uZQ==");
        assert_eq!(
            plain,
            ("Build".into(), "done".into()),
            "no subtitle, as before"
        );
    }

    /// **A click on the body is offered where the program asked for it**:
    /// kitty's default `a=focus`, `a=report`, both, and neither. A
    /// notification asking neither carries no `default` action.
    #[test]
    fn the_default_action_is_offered_for_focus_or_report() {
        use crate::notify::{Caps, Icons, Posted, ServerCaps};
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let caps = ServerCaps {
            actions: Caps::Listed,
            markup: Caps::Absent,
        };
        let offered = |a: &str| {
            let mut owned =
                baud_core::Owned::new(&ws).with_notices(baud_core::NoticeBudget::with_icon(1024));
            let mut t = baud_core::Term::new(owned.storage(), &ws);
            t.set_notice_support(crate::notify::support(
                crate::notify::Setting::On,
                crate::notify::Platform::Linux(caps),
            ));
            let b = format!("\x1b]99;i=n{a};Done\x1b\\");
            t.write_at(BytePos::ZERO, b.as_bytes());
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            let p = t
                .notices()
                .map(|n| Posted::of(&n, &mut Icons::default()))
                .next()
                .expect("posted");
            notification(&p, None, caps, None)
                .actions
                .iter()
                .any(|(k, _)| k == "default")
        };
        assert!(offered(""), "kitty's default, focus");
        assert!(offered(":a=report"));
        assert!(offered(":a=-focus,report"));
        assert!(!offered(":a=-focus"), "neither");
    }

    /// **An activation token is handed over with the click it came
    /// before**: `ActivationToken` for a notification, then its
    /// `ActionInvoked`, carries the token; a token for another
    /// notification is not handed to this one's click and stays for its
    /// own; a click takes it and a close drops it, so a later click
    /// carries none. The signals are marshalled here, by the marshaller
    /// that reproduces dbus-next's `ActionInvoked` byte for byte (the
    /// control below).
    #[test]
    fn an_activation_token_comes_with_its_notifications_click() {
        let signal = |member: &str, id: u32, s: &str, serial: u32| {
            let mut b = Writer::default();
            b.word(Word(id));
            b.string(s);
            marshal_signal(
                &Call {
                    destination: "",
                    path: NOTIFICATIONS_PATH,
                    interface: NOTIFICATIONS,
                    member,
                    signature: "us",
                    body: b.buf,
                },
                Serial(Word(serial)),
            )
        };
        assert_eq!(
            signal("ActionInvoked", 42, "1", 13),
            reply_bytes("action_invoked"),
            "the control: dbus-next's bytes"
        );
        let closed = |id: u32| {
            let mut b = Writer::default();
            b.word(Word(id));
            b.word(Word(2));
            marshal_signal(
                &Call {
                    destination: "",
                    path: NOTIFICATIONS_PATH,
                    interface: NOTIFICATIONS,
                    member: "NotificationClosed",
                    signature: "uu",
                    body: b.buf,
                },
                Serial(Word(20)),
            )
        };
        let id = |n| NotifyId(Word(n));
        let mut s = ready();
        assert_eq!(s.received(&signal("ActivationToken", 42, "tok-42", 14)), []);
        assert_eq!(
            s.received(&signal("ActionInvoked", 42, "default", 15)),
            [Heard::Invoked(
                id(42),
                "default".into(),
                Some("tok-42".into())
            )]
        );
        assert_eq!(
            s.received(&signal("ActionInvoked", 42, "default", 16)),
            [Heard::Invoked(id(42), "default".into(), None)],
            "a click takes the token"
        );
        assert_eq!(s.received(&signal("ActivationToken", 7, "tok-7", 17)), []);
        assert_eq!(
            s.received(&signal("ActionInvoked", 42, "default", 18)),
            [Heard::Invoked(id(42), "default".into(), None)],
            "another notification's token"
        );
        assert_eq!(
            s.received(&signal("ActionInvoked", 7, "1", 19)),
            [Heard::Invoked(id(7), "1".into(), Some("tok-7".into()))],
            "kept for its own"
        );
        assert_eq!(s.received(&signal("ActivationToken", 7, "tok-7b", 21)), []);
        assert_eq!(s.received(&closed(7)), [Heard::NotifyClosed(id(7))]);
        assert_eq!(
            s.received(&signal("ActionInvoked", 7, "default", 22)),
            [Heard::Invoked(id(7), "default".into(), None)],
            "a close drops it"
        );
    }
}
