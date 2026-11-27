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
//! * **The appearance and the text scale**: `AddMatch` for the portal's
//!   `SettingChanged`, then `ReadAll` of `org.freedesktop.appearance` and
//!   `org.gnome.desktop.interface`, whose `text-scaling-factor` (a `d`) baud
//!   sizes a point by (`typesize.rs`) and whose `enable-animations` (a `b`)
//!   off reduces motion as `reduced-motion` does -- `ReadAll` being the
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
//!   before, and handed over with it. The calls are `baud_domain::dbus`'s
//!   (`FdoNotify::encode`, `CLOSE_NOTIFICATION`, `GET_CAPABILITIES`,
//!   `ADD_MATCH`), each serial drawn from the session's one counter; a
//!   reply finds its call in the slot its serial names, and a `Notify`
//!   whose slot a call still unanswered holds is refused, counted and not
//!   sent. Replies and signals are decoded by it too, each body read only
//!   as its own signature. The server's signals are acted on only from
//!   the connection that owns its name: the SENDER of its first reply,
//!   which "is controlled by the message bus" (`[[source: dbus-spec]]`,
//!   "Header Fields"), until the bus's own `NameOwnerChanged` names
//!   another connection or none, the bus asked for that once the first
//!   holder is known. No reply names the holder after the bus has.
//! * **Progress**: `com.canonical.Unity.LauncherEntry`'s `Update`
//!   signal, `progress` and `progress-visible`, and `urgent` for an
//!   attention request, the interface docks read for a launcher
//!   (`[[source: libunity-launcher-entry]]`), encoded by
//!   `baud_domain::dbus` as an `Emit`.
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
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use baud_domain::dbus::{
    ADD_MATCH, Array, AuthSent, AuthStep, Body, Boolean, BusName, Byte, Bytes, CLOSE_NOTIFICATION,
    Connection, Dict, Double, Emit, EncodeError, Encoded, FdAnswer, FdIndex, FdoNotify, FdsAgreed,
    GET_CAPABILITIES, Handle, HandleToken, Handshake, HolderChange, I32, ImageData, Inbound,
    InterfaceName, MemberName, MethodCall, NOTIFY, Negotiating, NoFds, ObjectPath, ParentWindow,
    Peer, PortalResponse, RequestPath, ResponseCode, Return, Signal, Str, Trusted, U32, UnixFd,
    Variant, auth_line,
};

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

/// **The desktop's `enable-animations`** (`org.gnome.desktop.interface`),
/// a `b`: animations off asks for as little motion as reduced motion does
/// (`appearance::motion_of`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Animations {
    On,
    Off,
}

impl Animations {
    /// A boolean's word: 0 off, anything else on.
    fn of(v: Word) -> Animations {
        match v.0 {
            0 => Animations::Off,
            _ => Animations::On,
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
#[derive(PartialEq, Debug)]
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
    /// The desktop's `enable-animations`, read or changed, the same way.
    Animations(Animations),
    /// The desktop's text-scaling factor (`org.gnome.desktop.interface
    /// text-scaling-factor`), read or changed; a reply
    /// without one says nothing of it, and the driver keeps 1.
    TextScale(TextScale),
    /// The file manager took a reveal.
    Revealed,
    /// It did not: the error's name.
    NotRevealed(String),
    /// The notification server's capabilities: there is one.
    Caps(baud_domain::ServerCaps),
    /// `GetCapabilities` failed, or its reply was not an `as`: no
    /// notification server, the error's name or what the reply was.
    NoServer(String),
    /// A notification shown, as the server's id.
    Notified(baud_core::NoticeToken, NotifyId),
    /// It was not: the error's name.
    NotNotified(baud_core::NoticeToken, String),
    /// A click on a notification, or on one of its actions: the key, and
    /// the activation token the server sent for it just before, if any.
    Invoked(NotifyId, String, Option<baud_domain::XdgToken>),
    /// A notification closed, for any reason.
    NotifyClosed(NotifyId),
    /// Whether the print portal is there: its `version`, or the error's
    /// name.
    PrintPortal(Result<PortalVersion, String>),
    /// The print dialog's choices: the paper, and the token that prints
    /// with them.
    PrintReady(PrintToken, PageSetup),
    /// A print exchange ended: printed, cancelled, or failed and why.
    PrintEnded(PrintEnd),
    /// A `Response` on the waited-for path from a connection that does
    /// not hold the portal's name: refused, the exchange still waiting,
    /// with who sent it and how many so far.
    PrintRefused(String),
    /// The handshake failed or the bus went away; nothing more will come.
    Closed(String),
}

/// The token `PreparePrint`'s answer carries, which `Print` passes back to
/// print "with the settings from the PreparePrint call that the token
/// refers to" (`[[source: xdg-portal-print]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PrintToken(Word);

/// The print portal's `version`, 4 for the interface pinned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PortalVersion(pub(crate) u32);

/// The paper the user chose, as the portal's `page-setup` gives it: each
/// key in millimetres, `None` where it is left out or no length.
pub(crate) use baud_domain::dbus::PageSetup;

/// **How many `Response`s carried a code the Request protocol does not
/// give** (`[[source: xdg-portal-request]]` gives 0, 1 and 2): each ended
/// its exchange as failed, and the failure says the count.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct UnknownCodes(u32);

impl std::fmt::Display for UnknownCodes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl UnknownCodes {
    /// One more.
    const fn counted(self) -> UnknownCodes {
        UnknownCodes(self.0.saturating_add(1))
    }
}

/// **How many `Response`s came on a waited-for path from a connection
/// that does not hold the portal's name**: each refused, ending nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct RefusedResponses(u32);

impl std::fmt::Display for RefusedResponses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl RefusedResponses {
    /// One more.
    const fn counted(self) -> RefusedResponses {
        RefusedResponses(self.0.saturating_add(1))
    }
}

/// How a print exchange ended: the `Response` codes of
/// `[[source: xdg-portal-request]]`, 0 success, 1 "the user cancelled the
/// interaction", 2 "the user interaction was ended in some other way", and
/// a method's error.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum PrintEnd {
    Printed,
    Cancelled,
    Failed(String),
}

/// A notification's id, as the server gave it: "The returned ID is always
/// greater than zero".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NotifyId(Word);

impl NotifyId {
    /// The id a server's reply or signal carries.
    fn of(id: U32) -> NotifyId {
        NotifyId(Word(id.get()))
    }
}

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
    /// What a launcher entry is sent as.
    pub(crate) fn of(e: baud_domain::LauncherEntry) -> Launcher {
        Launcher {
            progress: e.bar.fraction(),
            urgent: match e.urgent {
                baud_domain::LauncherUrgent::Yes => Truth::True,
                baud_domain::LauncherUrgent::No => Truth::False,
            },
        }
    }
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
/// The interface namespace the portal relays, whose
/// `text-scaling-factor` baud sizes a point by (`typesize.rs`).
const INTERFACE: &str = "org.gnome.desktop.interface";
/// The launcher's `Update`, from the object a launcher's signal comes
/// from: `launcherentry/` and a name of the sender's choosing (libunity
/// hashes the URI).
const LAUNCHER_UPDATE: Emit<'static> = Emit {
    path: ObjectPath::lit("/com/canonical/unity/launcherentry/baud"),
    interface: InterfaceName::lit("com.canonical.Unity.LauncherEntry"),
    member: MemberName::lit("Update"),
};
/// The URI docks match to an installed desktop file: the Wayland app id's.
const LAUNCHER_URI: &str = "application://org.baud.terminal.desktop";
/// The name of baud's desktop file without `.desktop`, the `desktop-entry` hint a
/// notification carries: "the prefix used for the application's .desktop file"
/// (`[[source: freedesktop-notification-spec]]`), so a server can take baud's
/// icon and name from it.
const DESKTOP_ENTRY: &str = "org.baud.terminal";
const FILE_MANAGER: &str = "org.freedesktop.FileManager1";
const FILE_MANAGER_PATH: &str = "/org/freedesktop/FileManager1";
/// The bus's `RemoveMatch`, its body the rule, `s`.
const REMOVE_MATCH: MethodCall<'static> = MethodCall {
    member: MemberName::lit("RemoveMatch"),
    ..ADD_MATCH
};
/// The print portal's `PreparePrint`, `ssa{sv}a{sv}a{sv}`.
const PREPARE_PRINT: MethodCall<'static> = MethodCall {
    destination: baud_domain::dbus::PORTAL,
    path: baud_domain::dbus::PORTAL_PATH,
    interface: baud_domain::dbus::PRINT,
    member: MemberName::lit("PreparePrint"),
};
/// Its `Print`, `ssha{sv}`.
const PRINT_CALL: MethodCall<'static> = MethodCall {
    member: MemberName::lit("Print"),
    ..PREPARE_PRINT
};
/// `Properties.Get` on the portal's object, `ss`.
const PROPERTIES_GET: MethodCall<'static> = MethodCall {
    interface: InterfaceName::lit("org.freedesktop.DBus.Properties"),
    member: MemberName::lit("Get"),
    ..PREPARE_PRINT
};

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

    /// A `d`, eight-aligned: the print tests' `Response`.
    #[cfg(test)]
    fn double(&mut self, v: f64) {
        self.align(Align::EIGHT);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// An array: its length word, the padding to its elements' boundary,
    /// the elements, and the length -- of the elements alone -- written:
    /// the print tests' `Response`.
    #[cfg(test)]
    fn array(&mut self, elem: Align, f: impl FnOnce(&mut Writer)) {
        self.word(Word(0));
        let at = self.buf.len() - 4;
        self.align(elem);
        let start = self.buf.len();
        f(self);
        let len = u32::try_from(self.buf.len() - start).unwrap_or(u32::MAX);
        self.buf[at..at + 4].copy_from_slice(&len.to_le_bytes());
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
/// broadcast signal" (`[[source: dbus-spec]]`): the print tests'
/// `Response`.
#[cfg(test)]
fn marshal_signal(call: &Call<'_>, serial: Serial) -> Vec<u8> {
    header_and_body(SIGNAL, call, serial)
}

/// The message: its fixed header, its fields, and the body.
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

    /// A `d`, eight-aligned, in the message's byte order.
    fn double(&mut self) -> Option<f64> {
        self.align(Align::EIGHT)?;
        let b: [u8; 8] = self.take(Len(8))?.try_into().ok()?;
        Some(match self.order {
            Order::Big => f64::from_be_bytes(b),
            Order::Little => f64::from_le_bytes(b),
        })
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
    /// The interface namespace's `text-scaling-factor`.
    text_scale: Option<TextScale>,
    /// The interface namespace's `enable-animations`.
    animations: Option<Animations>,
}

/// **The desktop's text-scaling factor**, kept in thousandths so [`Heard`]
/// stays `Eq`: 1.25 is 1250.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct TextScale(u32);

impl TextScale {
    /// The factor `f`, or `None` for one that is not a finite positive
    /// number.
    pub(crate) fn of(f: f64) -> Option<TextScale> {
        (f.is_finite() && f > 0.0)
            .then(|| (f * 1000.0).round())
            .and_then(|t| u32::try_from(t as i64).ok())
            .map(TextScale)
    }

    /// The factor itself, which the Linux driver sizes its point by; the
    /// macOS driver reads none.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn factor(self) -> f64 {
        f64::from(self.0) / 1000.0
    }
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
                (false, "text-scaling-factor", "d") if namespace == INTERFACE => {
                    found.text_scale = TextScale::of(r.double()?);
                }
                (false, "enable-animations", "b") if namespace == INTERFACE => {
                    found.animations = Some(Animations::of(r.word()?));
                }
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
        .chain(a.text_scale.map(Heard::TextScale))
        .chain(a.animations.map(Heard::Animations))
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
    if (namespace, key, sig) == (INTERFACE, "text-scaling-factor", "d") {
        return TextScale::of(r.double()?).map(Heard::TextScale);
    }
    if (namespace, key, sig) == (INTERFACE, "enable-animations", "b") {
        return Some(Heard::Animations(Animations::of(r.word()?)));
    }
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

/// `GetCapabilities`' `as`: the capabilities it lists, read only as `as`.
/// A reply of another signature says nothing of what the server shows, so
/// nothing is claimed.
fn caps_in(r: &Return<'_>) -> Heard {
    match r.body().read::<(Array<Str>,)>() {
        Ok((caps,)) => Heard::Caps(baud_domain::ServerCaps::heard(caps.map(|c| c.as_str()))),
        Err(e) => Heard::NoServer(format!(
            "GetCapabilities answered `{}`: {e}",
            String::from_utf8_lossy(r.body().sig().as_bytes())
        )),
    }
}

/// `Notify`'s `u`, read only as `u`: the server's id for `token`.
fn id_in(r: &Return<'_>, token: baud_core::NoticeToken) -> Heard {
    match r.body().read::<(U32,)>() {
        Ok((id,)) => Heard::Notified(token, NotifyId::of(id)),
        Err(e) => Heard::NotNotified(token, format!("a reply that is not its id: {e}")),
    }
}

/// What one of the server's signals says.
enum FromServer {
    Heard(Heard),
    /// `ActivationToken(u, s)`: kept for the click on that notification.
    Token(NotifyId, baud_domain::XdgToken),
}

/// `ActionInvoked(u, s)`, `ActivationToken(u, s)` and
/// `NotificationClosed(u, u)`, from the connection that owns the server's
/// name, each read only as its own signature.
fn fdo_signal(t: Trusted<'_>) -> Option<FromServer> {
    let s = t.signal();
    if s.interface != NOTIFY.interface || s.path != NOTIFY.path {
        return None;
    }
    let body = s.body();
    match s.member.as_str() {
        "ActionInvoked" => {
            let (id, key) = body.read::<(U32, Str)>().ok()?;
            Some(FromServer::Heard(Heard::Invoked(
                NotifyId::of(id),
                key.as_str().to_owned(),
                None,
            )))
        }
        "ActivationToken" => {
            let (id, token) = body.read::<(U32, Str)>().ok()?;
            Some(FromServer::Token(
                NotifyId::of(id),
                baud_domain::XdgToken::heard(token.as_str()),
            ))
        }
        "NotificationClosed" => {
            let (id, _reason) = body.read::<(U32, U32)>().ok()?;
            Some(FromServer::Heard(Heard::NotifyClosed(NotifyId::of(id))))
        }
        _ => None,
    }
}

/// **Where the connection is**: the handshake in its state, the
/// connection it began, with file passing agreed or not, or the end.
/// A call that carries a file is made only on [`Link::Fds`], the one
/// state whose [`FdsAgreed`] makes a [`baud_domain::dbus::UnixFd`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Link {
    /// `AUTH` sent; waiting for `OK`, or for `DATA` when no identity was
    /// sent with it.
    Sent(Handshake<AuthSent>),
    /// `NEGOTIATE_UNIX_FD` sent after `OK`; waiting for `AGREE_UNIX_FD` or
    /// `ERROR`, and either begins.
    Negotiating(Handshake<Negotiating>),
    /// `BEGIN` and `Hello` sent, files agreed: messages flow.
    Fds(Connection<FdsAgreed>),
    /// `BEGIN` and `Hello` sent, files refused: messages flow, and a call
    /// that carries a file is not made.
    NoFds(Connection<NoFds>),
    Failed,
}

/// The notification server's calls whose replies this waits for, each
/// kept in the slot its serial names (`baud_domain::dbus::Awaiting`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Waiting {
    Caps,
    Notify(baud_core::NoticeToken),
}

/// Slots for the notification server's replies: 32 calls in flight.
type Notifying = baud_domain::dbus::Awaiting<Waiting, 32>;

/// Whether the bus has been asked to say when a watched name (the
/// notification server's, the portal's) changes hands: asked once, with
/// the first holder known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HolderWatch {
    Unasked,
    Asked,
}

/// **Who holds a watched name** (the notification server's, the
/// portal's), and who said so.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Holder {
    /// Nothing has named it.
    Unknown,
    /// The SENDER of the first reply from the name, which the bus sets.
    Replied(Peer),
    /// What the bus's own `NameOwnerChanged` said last: a connection, or
    /// none. No reply names the holder after it.
    Told(Option<Peer>),
}

impl Holder {
    /// The connection whose signals are acted on, if one is known.
    fn peer(&self) -> Option<&Peer> {
        match self {
            Holder::Replied(p) | Holder::Told(Some(p)) => Some(p),
            Holder::Unknown | Holder::Told(None) => None,
        }
    }
}

/// **Why a `Notify` was not sent**: its reply's slot held by a call still
/// unanswered (refused and counted), a text that is not a STRING, or a
/// message past its bound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NotSent {
    Held,
    Text(baud_domain::dbus::NameError),
    Bound(baud_domain::dbus::EncodeError),
}

impl std::fmt::Display for NotSent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotSent::Held => {
                f.write_str("its reply's slot is held by a call the server has not answered")
            }
            NotSent::Text(e) => write!(f, "a text that is not a D-Bus string: {e}"),
            NotSent::Bound(e) => write!(f, "{e}"),
        }
    }
}

/// The other calls whose replies this waits for, by serial.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Asked {
    ReadAll(Serial),
    ShowItems(Serial),
}

/// Which call a print exchange's `Response` answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PrintStep {
    Prepare,
    Print,
}

/// **The print exchange waiting for its `Response`**: the step, the call
/// whose reply names the Request's path, the path predicted from the
/// unique name and the token sent, the path that reply named instead when
/// it named another, and the match rule added for the path answered on,
/// removed when it answers.
struct Pending {
    step: PrintStep,
    call: baud_domain::dbus::Serial,
    predicted: RequestPath,
    moved: Option<String>,
    rule: String,
}

impl Pending {
    /// The path the `Response` comes on.
    fn path(&self) -> &str {
        match &self.moved {
            Some(p) => p,
            None => self.predicted.as_path().as_str(),
        }
    }
}

/// The match rule for a `Response` on `path`, sent by the portal's name:
/// the bus delivers it only from the connection that holds the name.
fn response_rule(path: &str) -> String {
    format!(
        "type='signal',sender='{}',interface='{}',member='Response',path='{path}'",
        baud_domain::dbus::PORTAL.as_str(),
        baud_domain::dbus::REQUEST.as_str()
    )
}

/// The match rule for the bus's `NameOwnerChanged` of the portal's name.
const PORTAL_HOLDER_RULE: &str = "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',path='/org/freedesktop/DBus',arg0='org.freedesktop.portal.Desktop'";

/// The print portal's `version`, from `Properties.Get`'s reply: a `v`
/// holding a `u`.
fn version_in(r: &Return<'_>) -> Result<PortalVersion, String> {
    r.body()
        .read::<(Variant<'static>,)>()
        .ok()
        .and_then(|(v,)| v.read::<U32>())
        .map(|v| PortalVersion(v.get()))
        .ok_or_else(|| "a version that is not a u".to_owned())
}

/// A print exchange ended as failed, for `why`.
fn print_failed(why: impl Into<String>) -> Heard {
    Heard::PrintEnded(PrintEnd::Failed(why.into()))
}

/// **The protocol over bytes**: what to send, and what the bytes that came
/// mean. No I/O, so a test drives it with a peer's bytes.
pub(crate) struct Session {
    link: Link,
    out: Vec<u8>,
    /// The files that go with the bytes of `out` from each offset on: one
    /// message's, sent with its first byte.
    out_fds: Vec<(Len, Vec<OwnedFd>)>,
    /// Calls made before the handshake ended, and their files, sent behind
    /// `Hello`.
    queued: Vec<(Vec<u8>, Vec<OwnedFd>)>,
    inb: Vec<u8>,
    /// Every message's serial after `Hello`, drawn as it is encoded. Calls
    /// made during the handshake queue behind `Hello` already encoded, so
    /// they draw here from the session's start, and the connection's own
    /// counter is not drawn from.
    serials: baud_domain::dbus::Serials,
    /// The notification server's calls waited for.
    awaiting: Notifying,
    /// The connection that owns the notification server's name: the
    /// SENDER of the first reply from it, which the bus sets, until the
    /// bus's own `NameOwnerChanged` names another or none. The server's
    /// signals are acted on only from it.
    holder: Holder,
    watch: HolderWatch,
    asked: Vec<Asked>,
    /// The last `ActivationToken` the notification server sent, for the
    /// notification it named: handed over with that notification's next
    /// click, and dropped by a click or a close of it.
    token: baud_domain::TokenFor<NotifyId>,
    /// A dialog asked for before the name was known, its title.
    prepare_after_hello: Option<(String, String)>,
    /// The print exchange waiting for its `Response`.
    print: Option<Pending>,
    /// The `Properties.Get` calls for the print portal's `version`.
    version_asked: Vec<baud_domain::dbus::Serial>,
    /// The `Response`s whose code the Request protocol does not give.
    unknown_codes: UnknownCodes,
    /// The connection that owns the portal's name: the SENDER of the
    /// first reply from it, until the bus's own `NameOwnerChanged` names
    /// another or none. A `Response` is acted on only from it.
    portal: Holder,
    portal_watch: HolderWatch,
    /// The `Response`s refused for coming from another connection.
    refused: RefusedResponses,
    /// The handle tokens sent so far, so each Request path is new.
    handles: Word,
}

impl Session {
    /// A session whose first bytes are the handshake: the credentials byte
    /// and `AUTH EXTERNAL`, with the user ID as the identity when it is
    /// known, and then the appearance asked for, behind it.
    pub(crate) fn new(uid: Option<Uid>) -> Session {
        let (h, first) = Handshake::start(uid.map(|u| baud_domain::dbus::Uid::new(u.0)));
        let mut s = Session {
            link: Link::Sent(h),
            out: first.bytes().to_vec(),
            out_fds: Vec::new(),
            queued: Vec::new(),
            inb: Vec::new(),
            // `Hello` is serial 1, sent at `BEGIN`; the rest queue behind
            // it.
            serials: baud_domain::dbus::Serials::new(),
            awaiting: Notifying::new(),
            holder: Holder::Unknown,
            watch: HolderWatch::Unasked,
            asked: Vec::new(),
            token: baud_domain::TokenFor::new(),
            prepare_after_hello: None,
            print: None,
            version_asked: Vec::new(),
            unknown_codes: UnknownCodes::default(),
            portal: Holder::Unknown,
            portal_watch: HolderWatch::Unasked,
            refused: RefusedResponses::default(),
            handles: Word(0),
        };
        s.ask_appearance();
        s.ask_notifications();
        s
    }

    fn next_serial(&mut self) -> Serial {
        Serial(Word(self.serials.next().get()))
    }

    fn call(&mut self, call: &Call<'_>) -> Serial {
        let serial = self.next_serial();
        let bytes = marshal(call, serial);
        self.send(bytes, Vec::new());
        serial
    }

    /// A message's bytes and its files out, or queued behind `Hello` while
    /// the handshake goes on.
    fn send(&mut self, bytes: Vec<u8>, fds: Vec<OwnedFd>) {
        match self.link {
            Link::Fds(_) | Link::NoFds(_) => {
                if !fds.is_empty() {
                    self.out_fds.push((Len(self.out.len()), fds));
                }
                self.out.extend_from_slice(&bytes);
            }
            Link::Sent(_) | Link::Negotiating(_) => self.queued.push((bytes, fds)),
            Link::Failed => {}
        }
    }

    /// `AddMatch` for the portal's setting changes, then one `ReadAll` of
    /// the appearance and the interface namespace (the text-scaling
    /// factor). One rule with no `arg0`: a match rule names one namespace
    /// at most, and `heard_in_signal` takes only the two it reads, so the
    /// calls keep their serials (`ReadAll` is 3).
    fn ask_appearance(&mut self) {
        let mut rule = Writer::default();
        rule.string(&format!(
            "type='signal',interface='{SETTINGS}',member='SettingChanged',path='{PORTAL_PATH}'"
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
        names.strings(&[APPEARANCE, INTERFACE]);
        let serial = self.call(&Call {
            destination: PORTAL,
            path: PORTAL_PATH,
            interface: SETTINGS,
            member: "ReadAll",
            signature: "as",
            body: names.buf,
        });
        self.asked.push(Asked::ReadAll(serial));
    }

    /// An encoded call out, its reply waited for as `w`; refused, counted
    /// and not sent when a call still unanswered holds its slot.
    fn send_awaited(&mut self, e: Encoded, w: Waiting) -> Result<(), NotSent> {
        if self.awaiting.insert(e.serial(), w).is_err() {
            return Err(NotSent::Held);
        }
        self.send(e.into_bytes(), Vec::new());
        Ok(())
    }

    /// `AddMatch` for the notification server's signals (`ActionInvoked`,
    /// `ActivationToken`, `NotificationClosed`), then `GetCapabilities`.
    fn ask_notifications(&mut self) {
        const RULE: Str<'static> = Str::lit(
            "type='signal',interface='org.freedesktop.Notifications',\
             path='/org/freedesktop/Notifications'",
        );
        if let Ok(e) = Body::of(&(RULE,)).and_then(|b| ADD_MATCH.encode(&b, &mut self.serials)) {
            self.send(e.into_bytes(), Vec::new());
        }
        if let Ok(e) = GET_CAPABILITIES.encode(&Body::EMPTY, &mut self.serials) {
            // The fifth serial of a new session: its slot is free.
            let _ = self.send_awaited(e, Waiting::Caps);
        }
    }

    /// The notification server's holder from `r`, a reply to one of its
    /// calls, where nothing has named one; with it known, `AddMatch` for
    /// the bus's `NameOwnerChanged` of the server's name, which names the
    /// holder from then on.
    fn hold(&mut self, r: &Return<'_>) {
        if self.holder != Holder::Unknown {
            return;
        }
        let Some(peer) = Peer::from_reply(r) else {
            return;
        };
        self.holder = Holder::Replied(peer);
        if self.watch == HolderWatch::Asked {
            return;
        }
        self.watch = HolderWatch::Asked;
        const RULE: Str<'static> = Str::lit(
            "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',path='/org/freedesktop/DBus',arg0='org.freedesktop.Notifications'",
        );
        if let Ok(e) = Body::of(&(RULE,)).and_then(|b| ADD_MATCH.encode(&b, &mut self.serials)) {
            self.send(e.into_bytes(), Vec::new());
        }
    }

    /// The portal's holder from `r`, a reply to one of its calls, where
    /// nothing has named one; with it known, `AddMatch` for the bus's
    /// `NameOwnerChanged` of the portal's name, which names the holder
    /// from then on.
    fn hold_portal(&mut self, r: &Return<'_>) {
        if self.portal != Holder::Unknown {
            return;
        }
        let Some(peer) = Peer::from_reply(r) else {
            return;
        };
        self.portal = Holder::Replied(peer);
        if self.portal_watch == HolderWatch::Asked {
            return;
        }
        self.portal_watch = HolderWatch::Asked;
        self.match_rule(&ADD_MATCH, PORTAL_HOLDER_RULE);
    }

    /// **The bus says the portal's name changed hands**: the holder from
    /// then on. A print exchange waiting for its `Response` when the
    /// holder goes or changes ends, its match removed, with the change
    /// said: the Request it waits on belonged to the connection gone.
    fn portal_changed(&mut self, change: HolderChange) -> Option<Heard> {
        let now = match change {
            HolderChange::To(peer) => Some(peer),
            HolderChange::Gone => None,
        };
        let before = self.portal.peer().copied();
        self.portal = Holder::Told(now);
        if before == now {
            return None;
        }
        let pending = self.print.take()?;
        self.match_rule(&REMOVE_MATCH, &pending.rule);
        let name =
            |p: Option<Peer>| p.map_or_else(|| "none".to_owned(), |p| p.name().as_str().to_owned());
        Some(print_failed(match now {
            Some(_) => format!(
                "ended: the print portal's name passed from {} to {} while the dialog was open",
                name(before),
                name(now)
            ),
            None => format!(
                "ended: the print portal's name was released by {} while the dialog was open",
                name(before)
            ),
        }))
    }

    /// **Show a notification**: `Notify`, its reply the server's id for
    /// `token`; not sent, and why, where its reply's slot is held or an
    /// argument breaks a bound.
    pub(crate) fn notify(
        &mut self,
        n: &Notification,
        token: baud_core::NoticeToken,
    ) -> Result<(), NotSent> {
        fn text(s: &str) -> Result<Str<'_>, NotSent> {
            Str::parse(s).map_err(NotSent::Text)
        }
        // A side past `i32` is an image past an array's bound.
        fn image(img: &Image) -> Result<ImageData<'_>, NotSent> {
            let too_long = |_| NotSent::Bound(EncodeError::ArrayTooLong);
            let [width, height, rowstride] = [
                img.size.width,
                img.size.height,
                img.size.width.saturating_mul(4),
            ]
            .map(|v| i32::try_from(v).map(I32::new));
            Ok(ImageData {
                width: width.map_err(too_long)?,
                height: height.map_err(too_long)?,
                rowstride: rowstride.map_err(too_long)?,
                alpha: Boolean::True,
                bits_per_sample: I32::new(8),
                channels: I32::new(4),
                data: Bytes::new(&img.rgba),
            })
        }
        let actions = n
            .actions
            .iter()
            .map(|(k, l)| Ok((text(k)?, text(l)?)))
            .collect::<Result<Vec<_>, NotSent>>()?;
        let mut hints = Vec::with_capacity(n.hints.len());
        for (k, v) in &n.hints {
            let v = match v {
                Hint::Byte(b) => Variant::Byte(Byte::new(b.0)),
                Hint::Bool(Truth::False) => Variant::Boolean(Boolean::False),
                Hint::Bool(Truth::True) => Variant::Boolean(Boolean::True),
                Hint::Str(s) => Variant::Str(text(s)?),
                Hint::Image(img) => Variant::Image(image(img)?),
            };
            hints.push((text(k)?, v));
        }
        let call = FdoNotify {
            app_name: text(&n.app_name)?,
            replaces: n.replaces.map_or(0, |r| r.0.0),
            app_icon: text(&n.app_icon)?,
            summary: text(&n.summary)?,
            body: text(&n.body)?,
            actions: &actions,
            hints: &hints,
            expire: match n.expire {
                Expire::Server => -1,
                Expire::Never => 0,
                Expire::After(d) => i32::try_from(d.as_millis()).unwrap_or(i32::MAX),
            },
        };
        let e = call.encode(&mut self.serials).map_err(NotSent::Bound)?;
        self.send_awaited(e, Waiting::Notify(token))
    }

    /// **Close one**: `CloseNotification`, whose `NotificationClosed`
    /// follows.
    pub(crate) fn close_notification(&mut self, id: NotifyId) {
        let body = Body::of(&(U32::new(id.0.0),));
        if let Ok(e) = body.and_then(|b| CLOSE_NOTIFICATION.encode(&b, &mut self.serials)) {
            self.send(e.into_bytes(), Vec::new());
        }
    }

    /// **The launcher's progress and urgency**, as libunity's
    /// `LauncherEntry` sends them: `Update(app_uri, properties)`.
    pub(crate) fn launcher(&mut self, l: Launcher) {
        let truth = |t| match t {
            Truth::False => Boolean::False,
            Truth::True => Boolean::True,
        };
        let shown = match l.progress {
            Some(_) => Truth::True,
            None => Truth::False,
        };
        let properties = [
            (
                Str::lit("progress"),
                Variant::Double(Double::new(l.progress.unwrap_or(0.0))),
            ),
            (Str::lit("progress-visible"), Variant::Boolean(truth(shown))),
            (Str::lit("urgent"), Variant::Boolean(truth(l.urgent))),
        ];
        let body = Body::of(&(Str::lit(LAUNCHER_URI), Dict::new(&properties)));
        if let Ok(e) = body.and_then(|b| LAUNCHER_UPDATE.encode(&b, &mut self.serials)) {
            self.send(e.into_bytes(), Vec::new());
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
        self.asked.push(Asked::ShowItems(serial));
    }

    /// The `Response`s whose code the Request protocol does not give.
    #[cfg(test)]
    fn unknown_codes(&self) -> UnknownCodes {
        self.unknown_codes
    }

    /// The `Response`s refused for coming from another connection.
    #[cfg(test)]
    fn refused(&self) -> RefusedResponses {
        self.refused
    }

    /// Whether the session has failed and will say nothing more.
    pub(crate) fn failed(&self) -> bool {
        self.link == Link::Failed
    }

    /// **A call that carries one file**, its body built around the `h`
    /// that names it, which only a connection that agreed to pass files
    /// makes: refused, and why, where the bus does not pass them or the
    /// message breaks a bound.
    fn call_with_file(
        &mut self,
        call: &MethodCall<'_>,
        body: impl FnOnce(UnixFd) -> Result<Body, EncodeError>,
        file: OwnedFd,
    ) -> Result<baud_domain::dbus::Serial, String> {
        let Link::Fds(c) = &self.link else {
            return Err("the session bus does not pass files".into());
        };
        let fd = c.fds().fd(FdIndex::ZERO);
        let e = body(fd)
            .and_then(|b| call.encode(&b, &mut self.serials))
            .map_err(|e| e.to_string())?;
        let serial = e.serial();
        self.send(e.into_bytes(), vec![file]);
        Ok(serial)
    }

    /// **What goes next**: the bytes up to the next message that carries
    /// files, or that message on with its files when it is first. A file
    /// rides with its message's first byte, so the bus reads it with that
    /// message.
    pub(crate) fn next_send(&self) -> (&[u8], &[OwnedFd]) {
        match self.out_fds.first() {
            Some((Len(0), fds)) => (&self.out, fds),
            Some((at, _)) => (&self.out[..at.0.min(self.out.len())], &[]),
            None => (&self.out, &[]),
        }
    }

    /// The bytes waiting to be sent.
    pub(crate) fn outgoing(&self) -> &[u8] {
        &self.out
    }

    /// `n` of them went, and the files sent with the first of them.
    pub(crate) fn sent(&mut self, n: Len) {
        let n = n.0.min(self.out.len());
        if n == 0 {
            return;
        }
        self.out.drain(..n);
        if matches!(self.out_fds.first(), Some((Len(0), _))) {
            self.out_fds.remove(0);
        }
        for (at, _) in &mut self.out_fds {
            at.0 = at.0.saturating_sub(n);
        }
    }

    /// `BEGIN` and `Hello`, the bytes `begun` gives, the connection begun
    /// as `link`, and what was queued behind them; a queued call with
    /// files is dropped where the bus refused them.
    fn begin(&mut self, begun: &[u8], link: Link) {
        self.out.extend_from_slice(begun);
        self.link = link;
        let agreed = matches!(link, Link::Fds(_));
        for (bytes, fds) in std::mem::take(&mut self.queued) {
            if fds.is_empty() || agreed {
                self.send(bytes, fds);
            }
        }
    }

    /// This connection's unique name, once `Hello` has answered: the
    /// Request paths the portal makes are under it.
    fn unique(&self) -> Option<BusName<'_>> {
        match &self.link {
            Link::Fds(c) => c.unique(),
            Link::NoFds(c) => c.unique(),
            Link::Sent(_) | Link::Negotiating(_) | Link::Failed => None,
        }
    }

    /// **`Hello`'s reply**, read through the codec: the unique name kept
    /// by the connection, and a dialog asked for before it then asked.
    /// `None` for any other message.
    fn hello_heard(&mut self, m: &Inbound<'_>) -> Option<Vec<Heard>> {
        let Inbound::Return(r) = m else {
            return None;
        };
        if r.reply_to != baud_domain::dbus::Serial::HELLO || self.unique().is_some() {
            return None;
        }
        let kept = match &mut self.link {
            Link::Fds(c) => c.on_hello(r),
            Link::NoFds(c) => c.on_hello(r),
            Link::Sent(_) | Link::Negotiating(_) | Link::Failed => return None,
        };
        Some(match (self.prepare_after_hello.take(), kept) {
            (Some((parent, title)), Ok(())) => self
                .prepare_print_named(&parent, &title)
                .into_iter()
                .collect(),
            (Some(_), Err(e)) => vec![Heard::PrintEnded(PrintEnd::Failed(format!(
                "the bus gave no unique name: {e}"
            )))],
            (None, _) => Vec::new(),
        })
    }

    /// **Ask whether the print portal is there**: its `version` property,
    /// heard as [`Heard::PrintPortal`].
    pub(crate) fn ask_print_portal(&mut self) {
        let body = Body::of(&(
            Str::lit(baud_domain::dbus::PRINT.as_str()),
            Str::lit("version"),
        ));
        if let Ok(e) = body.and_then(|b| PROPERTIES_GET.encode(&b, &mut self.serials)) {
            self.version_asked.push(e.serial());
            self.send(e.into_bytes(), Vec::new());
        }
    }

    /// The token for the next call's `handle_token`, numbered so each
    /// Request path is new, and the path it answers on, predicted from the
    /// unique name; why not, where there is no unique name or it makes no
    /// path.
    fn next_request(&mut self) -> Result<(HandleToken, RequestPath), String> {
        let n = self.handles.0.wrapping_add(1);
        let token = HandleToken::numbered(n);
        let unique = self.unique().ok_or("the bus gave no unique name")?;
        let path = RequestPath::predict(unique, &token)
            .ok_or_else(|| format!("the unique name {} makes no Request path", unique.as_str()))?;
        self.handles = Word(n);
        Ok((token, path))
    }

    /// `AddMatch` or `RemoveMatch`, `call`, for `rule`.
    fn match_rule(&mut self, call: &MethodCall<'_>, rule: &str) {
        let Ok(rule) = Str::parse(rule) else {
            return;
        };
        if let Ok(e) = Body::of(&(rule,)).and_then(|b| call.encode(&b, &mut self.serials)) {
            self.send(e.into_bytes(), Vec::new());
        }
    }

    /// The match for a Request's `Response` on `path`, sent **before** the
    /// call it answers, so an answer that comes before the method's reply
    /// is not missed (`[[source: xdg-portal-request]]`: the path is
    /// predictable "to let applications subscribe to the Response signal
    /// before making the initial portal call"). The rule, removed when the
    /// call fails or the Request answers.
    fn subscribe(&mut self, path: &RequestPath) -> String {
        let rule = response_rule(path.as_path().as_str());
        self.match_rule(&ADD_MATCH, &rule);
        rule
    }

    /// **The print dialog**, titled `title` and opened on `parent`:
    /// `PreparePrint` with nothing preset, offering PDF to print to a file,
    /// as that is what the job becomes. Before `Hello`'s reply it waits for
    /// it. The exchange's end, and why, where it cannot be asked.
    pub(crate) fn prepare_print(&mut self, parent: ParentWindow<'_>, title: &str) -> Option<Heard> {
        self.prepare_print_named(&parent.to_string(), title)
    }

    /// [`Session::prepare_print`] with the parent's text, as it is kept
    /// across `Hello`'s reply.
    fn prepare_print_named(&mut self, parent: &str, title: &str) -> Option<Heard> {
        if self.unique().is_none() {
            self.prepare_after_hello = Some((parent.to_owned(), title.to_owned()));
            return None;
        }
        let (Ok(parent), Ok(title)) = (Str::parse(parent), Str::parse(title)) else {
            return Some(print_failed(
                "a parent window or title that is not a D-Bus string",
            ));
        };
        let (token, predicted) = match self.next_request() {
            Ok(t) => t,
            Err(e) => return Some(print_failed(e)),
        };
        let Ok(handle) = Str::parse(token.as_str()) else {
            return Some(print_failed("a handle token that is not a D-Bus string"));
        };
        let formats = [Str::lit("pdf")];
        let options = [
            (Str::lit("handle_token"), Variant::Str(handle)),
            (
                Str::lit("supported_output_file_formats"),
                Variant::Strs(Array::new(&formats)),
            ),
        ];
        let none: [(Str<'_>, Variant<'_>); 0] = [];
        let body = match Body::of(&(
            parent,
            title,
            Dict::new(&none),
            Dict::new(&none),
            Dict::new(&options),
        )) {
            Ok(b) => b,
            Err(e) => return Some(print_failed(e.to_string())),
        };
        let rule = self.subscribe(&predicted);
        match PREPARE_PRINT.encode(&body, &mut self.serials) {
            Ok(e) => {
                self.print = Some(Pending {
                    step: PrintStep::Prepare,
                    call: e.serial(),
                    predicted,
                    moved: None,
                    rule,
                });
                self.send(e.into_bytes(), Vec::new());
                None
            }
            Err(e) => {
                self.match_rule(&REMOVE_MATCH, &rule);
                Some(print_failed(e.to_string()))
            }
        }
    }

    /// **The document, printed with the dialog's choices**: `Print` with
    /// `pdf`, open for reading at its start, and the token
    /// `PreparePrint` gave. Not sent, and why, where the bus does not pass
    /// files, the name is not known or makes no Request path, or a text
    /// is not a D-Bus string.
    pub(crate) fn print(
        &mut self,
        parent: ParentWindow<'_>,
        title: &str,
        pdf: OwnedFd,
        token: PrintToken,
    ) -> Result<(), String> {
        if !matches!(self.link, Link::Fds(_)) {
            return Err("the session bus does not pass files".into());
        }
        let parent = parent.to_string();
        let (Ok(parent), Ok(title)) = (Str::parse(&parent), Str::parse(title)) else {
            return Err("a parent window or title that is not a D-Bus string".into());
        };
        let (handle, predicted) = self.next_request()?;
        let handle = Str::parse(handle.as_str()).map_err(|e| e.to_string())?;
        let options = [
            (Str::lit("handle_token"), Variant::Str(handle)),
            (Str::lit("token"), Variant::U32(U32::new(token.0.0))),
        ];
        let rule = self.subscribe(&predicted);
        let body = |fd| Body::of(&(parent, title, fd, Dict::new(&options)));
        match self.call_with_file(&PRINT_CALL, body, pdf) {
            Ok(call) => {
                self.print = Some(Pending {
                    step: PrintStep::Print,
                    call,
                    predicted,
                    moved: None,
                    rule,
                });
                Ok(())
            }
            Err(e) => {
                self.match_rule(&REMOVE_MATCH, &rule);
                Err(e)
            }
        }
    }

    /// **The reply to `PreparePrint` or `Print`**: the Request path it
    /// returns, against the one predicted. Where it is another, the match
    /// moves to it (`[[source: xdg-portal-request]]`: "verify that the
    /// returned handle is what it expected, and update its signal
    /// subscription if it isn't").
    fn handle_returned(&mut self, r: &Return<'_>) {
        let Some(mut p) = self.print.take() else {
            return;
        };
        let moved = match (
            p.call == r.reply_to,
            r.body().read::<(ObjectPath<'static>,)>(),
        ) {
            (true, Ok((returned,))) => match p.predicted.matches(returned) {
                Handle::Moved(to) => Some(to.as_str().to_owned()),
                Handle::Expected => None,
            },
            _ => None,
        };
        if let Some(to) = moved {
            self.match_rule(&REMOVE_MATCH, &p.rule);
            p.rule = response_rule(&to);
            self.match_rule(&ADD_MATCH, &p.rule);
            p.moved = Some(to);
        }
        self.print = Some(p);
    }

    /// **A Request's `Response`**, if it is the one waited for: the match
    /// removed, and what its code means for the step it answers
    /// ([`ResponseCode`]): success, cancelled, or failed with the code
    /// said, a code the protocol does not give counted as well.
    fn heard_response(&mut self, s: &Signal<'_>) -> Option<Heard> {
        let ours = s.interface == baud_domain::dbus::REQUEST
            && s.member == baud_domain::dbus::RESPONSE
            && self.print.as_ref().map(Pending::path) == Some(s.path.as_str());
        if !ours {
            return None;
        }
        let Some(trusted) = self.portal.peer().and_then(|p| p.vouch(s)) else {
            self.refused = self.refused.counted();
            let from = s.header().sender().map_or("no sender", |b| b.as_str());
            return Some(Heard::PrintRefused(format!(
                "a Response from {from}, which does not hold the print portal's name, refused ({} so far)",
                self.refused
            )));
        };
        let pending = self.print.take()?;
        self.match_rule(&REMOVE_MATCH, &pending.rule);
        let Some(response) = PortalResponse::from_trusted(&trusted) else {
            return Some(print_failed("a Response that is not ua{sv}"));
        };
        let mut setup = PageSetup::default();
        let mut token = None;
        for (key, value) in response.results() {
            match key.as_str() {
                "token" => token = value.read::<U32>().map(|t| PrintToken(Word(t.get()))),
                "page-setup" => {
                    if let Some(d) = value.read::<Dict<'static, Str<'static>, Variant<'static>>>() {
                        setup = PageSetup::read(d);
                    }
                }
                _ => {}
            }
        }
        Some(match (response.code, pending.step, token) {
            (ResponseCode::Success, PrintStep::Prepare, Some(token)) => {
                Heard::PrintReady(token, setup)
            }
            (ResponseCode::Success, PrintStep::Prepare, None) => {
                print_failed("a dialog's answer with no token")
            }
            (ResponseCode::Success, PrintStep::Print, _) => Heard::PrintEnded(PrintEnd::Printed),
            (ResponseCode::Cancelled, ..) => Heard::PrintEnded(PrintEnd::Cancelled),
            (ResponseCode::Ended, ..) => {
                print_failed("the dialog ended with response 2, ended in some other way")
            }
            (ResponseCode::Unknown(n), ..) => {
                self.unknown_codes = self.unknown_codes.counted();
                print_failed(format!(
                    "the dialog ended with response {}, a code the Request protocol does not give ({} so far)",
                    n.get(),
                    self.unknown_codes
                ))
            }
        })
    }

    /// **What a message means to the print half**, read through the
    /// codec: the portal's `version`, a print call's reply or error, or
    /// the Request's `Response`. `None` for any other message.
    fn print_heard(&mut self, m: &Inbound<'_>) -> Option<Vec<Heard>> {
        match m {
            Inbound::Return(r) => {
                if let Some(i) = self.version_asked.iter().position(|s| *s == r.reply_to) {
                    self.version_asked.swap_remove(i);
                    self.hold_portal(r);
                    return Some(vec![Heard::PrintPortal(version_in(r))]);
                }
                if self.print.as_ref().is_some_and(|p| p.call == r.reply_to) {
                    self.hold_portal(r);
                    self.handle_returned(r);
                    return Some(Vec::new());
                }
                None
            }
            Inbound::ErrorReply(e) => {
                let name = e.name.as_str().to_owned();
                if let Some(i) = self.version_asked.iter().position(|s| *s == e.reply_to) {
                    self.version_asked.swap_remove(i);
                    return Some(vec![Heard::PrintPortal(Err(name))]);
                }
                if !self.print.as_ref().is_some_and(|p| p.call == e.reply_to) {
                    return None;
                }
                let p = self.print.take()?;
                self.match_rule(&REMOVE_MATCH, &p.rule);
                Some(vec![print_failed(name)])
            }
            Inbound::Signal(s)
                if let Some(change) = s
                    .from_bus()
                    .and_then(|bus| HolderChange::of(&bus, baud_domain::dbus::PORTAL)) =>
            {
                Some(self.portal_changed(change).into_iter().collect())
            }
            Inbound::Signal(s) => self.heard_response(s).map(|h| vec![h]),
            Inbound::Called(_) | Inbound::Unknown(_) => None,
        }
    }

    /// **Bytes the bus sent**, and what they meant.
    pub(crate) fn received(&mut self, bytes: &[u8]) -> Vec<Heard> {
        self.inb.extend_from_slice(bytes);
        let mut heard = Vec::new();
        // The handshake's lines, as many as came: after `OK`, file passing
        // is asked for (`NEGOTIATE_UNIX_FD`), and its answer, agreed or an
        // `ERROR`, begins.
        while matches!(self.link, Link::Sent(_) | Link::Negotiating(_)) {
            let Some(line) = auth_line(&self.inb) else {
                return heard;
            };
            let said = String::from_utf8_lossy(line.bytes()).into_owned();
            let consumed = line.consumed().index();
            let refused = match std::mem::replace(&mut self.link, Link::Failed) {
                Link::Sent(h) => match h.on_line(line) {
                    AuthStep::Data(h, answer) | AuthStep::Unknown(h, answer) => {
                        self.out.extend_from_slice(answer);
                        self.link = Link::Sent(h);
                        None
                    }
                    AuthStep::Ok(h) => {
                        let (h, ask) = h.negotiate_fds();
                        self.out.extend_from_slice(ask);
                        self.link = Link::Negotiating(h);
                        None
                    }
                    AuthStep::Refused(r) => Some(r),
                },
                Link::Negotiating(h) => match h.on_line(line) {
                    FdAnswer::Agreed(b) => {
                        self.begin(b.bytes(), Link::Fds(b.into_connection()));
                        None
                    }
                    FdAnswer::Refused(b) => {
                        self.begin(b.bytes(), Link::NoFds(b.into_connection()));
                        None
                    }
                    FdAnswer::Unexpected(r) => Some(r),
                },
                link @ (Link::Fds(_) | Link::NoFds(_) | Link::Failed) => {
                    self.link = link;
                    None
                }
            };
            self.inb.drain(..consumed);
            if let Some(r) = refused {
                heard.push(Heard::Closed(format!(
                    "the bus refused the handshake: {r}: {said}"
                )));
                return heard;
            }
        }
        while let Some(len) = message_len(&self.inb) {
            let Ok(len) = len else {
                self.link = Link::Failed;
                heard.push(Heard::Closed(
                    "the bus sent a message this cannot read".into(),
                ));
                return heard;
            };
            if self.inb.len() < len.0 {
                break;
            }
            let bytes: Vec<u8> = self.inb.drain(..len.0).collect();
            if let Some(h) = baud_domain::dbus::decode(&bytes).ok().and_then(|m| {
                self.hello_heard(&m)
                    .or_else(|| self.notifications_heard(&m))
                    .or_else(|| self.print_heard(&m))
            }) {
                heard.extend(h);
                continue;
            }
            let Some(m) = parse(&bytes) else {
                continue;
            };
            heard.extend(self.heard(&m));
        }
        heard
    }

    /// **What a message means to the notification half**, read through
    /// the codec: a reply to a call it waits for, found in the slot its
    /// serial names; the server's signal, acted on only from the
    /// connection that owns the name; the bus saying the name changed
    /// hands. `None` for any other message.
    fn notifications_heard(&mut self, m: &Inbound<'_>) -> Option<Vec<Heard>> {
        match m {
            Inbound::Return(r) => {
                let waited = self.awaiting.take(r.reply_to)?;
                self.hold(r);
                Some(vec![match waited {
                    Waiting::Caps => caps_in(r),
                    Waiting::Notify(token) => id_in(r, token),
                }])
            }
            Inbound::ErrorReply(e) => {
                let waited = self.awaiting.take(e.reply_to)?;
                let name = e.name.as_str().to_owned();
                Some(vec![match waited {
                    Waiting::Caps => Heard::NoServer(name),
                    Waiting::Notify(token) => Heard::NotNotified(token, name),
                }])
            }
            Inbound::Signal(s)
                if let Some(change) = s
                    .from_bus()
                    .and_then(|bus| HolderChange::of(&bus, NOTIFY.destination)) =>
            {
                self.holder = Holder::Told(match change {
                    HolderChange::To(peer) => Some(peer),
                    HolderChange::Gone => None,
                });
                Some(Vec::new())
            }
            Inbound::Signal(s) => Some(match fdo_signal(self.holder.peer()?.vouch(s)?)? {
                FromServer::Token(id, token) => {
                    self.token.keep(id, token);
                    Vec::new()
                }
                FromServer::Heard(Heard::Invoked(id, key, _)) => {
                    let token = self.token.take_for(id);
                    vec![Heard::Invoked(id, key, token)]
                }
                FromServer::Heard(h @ Heard::NotifyClosed(id)) => {
                    let _ = self.token.take_for(id);
                    vec![h]
                }
                FromServer::Heard(h) => vec![h],
            }),
            Inbound::Called(_) | Inbound::Unknown(_) => None,
        }
    }

    /// What one message means, if it is one this waits for or a change of
    /// the appearance: a `ReadAll` reply can say three things.
    fn heard(&mut self, m: &Message<'_>) -> Vec<Heard> {
        if m.kind == SIGNAL {
            return heard_in_signal(m).into_iter().collect();
        }
        let error = (m.kind == ERROR).then(|| m.error.unwrap_or("an unnamed error").to_owned());
        let Some(reply) = m.reply_serial else {
            return Vec::new();
        };
        let Some(i) = self.asked.iter().position(|w| match w {
            Asked::ReadAll(s) | Asked::ShowItems(s) => *s == reply,
        }) else {
            return Vec::new();
        };
        let waited = self.asked.swap_remove(i);
        match (waited, m.kind, error) {
            (Asked::ReadAll(_), METHOD_RETURN, _) => heard_in_read_all(m),
            (Asked::ReadAll(_), _, Some(e)) => vec![Heard::NoScheme(e)],
            (Asked::ShowItems(_), METHOD_RETURN, _) => vec![Heard::Revealed],
            (Asked::ShowItems(_), _, Some(e)) => vec![Heard::NotRevealed(e)],
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

    /// Show a notification; not sent, and why, where it cannot be.
    pub(crate) fn notify(
        &mut self,
        n: &Notification,
        token: baud_core::NoticeToken,
    ) -> Result<(), NotSent> {
        let sent = self.session.notify(n, token);
        let _ = self.flush();
        sent
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

    #[cfg(target_os = "linux")]
    /// Ask whether the print portal is there.
    pub(crate) fn ask_print_portal(&mut self) {
        self.session.ask_print_portal();
        let _ = self.flush();
    }

    #[cfg(target_os = "linux")]
    /// The print dialog.
    pub(crate) fn prepare_print(&mut self, parent: ParentWindow<'_>, title: &str) -> Option<Heard> {
        let ended = self.session.prepare_print(parent, title);
        let _ = self.flush();
        ended
    }

    #[cfg(target_os = "linux")]
    /// The document printed with the dialog's choices; why not where it
    /// cannot be sent.
    pub(crate) fn print(
        &mut self,
        parent: ParentWindow<'_>,
        title: &str,
        pdf: OwnedFd,
        token: PrintToken,
    ) -> Result<(), String> {
        let sent = self.session.print(parent, title, pdf, token);
        let _ = self.flush();
        sent
    }

    /// What the kernel takes now: plain bytes by `write`, and a message
    /// that carries files by `sendmsg` with them (`baud_wayland::sys`).
    fn flush(&mut self) -> io::Result<()> {
        while !self.session.outgoing().is_empty() {
            let (bytes, fds) = self.session.next_send();
            let went = match fds.is_empty() {
                true => self.stream.write(bytes),
                #[cfg(target_os = "linux")]
                false => {
                    let fds: Vec<BorrowedFd<'_>> = fds.iter().map(AsFd::as_fd).collect();
                    baud_wayland::sys::send(&self.stream, bytes, &fds)
                }
                // The macOS build of this module is its tests', with no
                // call that carries a file.
                #[cfg(not(target_os = "linux"))]
                false => Err(io::ErrorKind::Unsupported.into()),
            };
            match went {
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
            self.session.link = Link::Failed;
            return vec![Heard::Closed(format!("the session bus: {e}"))];
        }
        let mut heard = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    self.session.link = Link::Failed;
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
                    self.session.link = Link::Failed;
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

/// This process's user ID, the identity `EXTERNAL` sends: the uid that owns
/// `/proc/self`, which is the process's own.
#[cfg(target_os = "linux")]
fn uid() -> Option<Uid> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| Uid(m.uid()))
}

/// **One notification as `Notify` takes it**: the arguments
/// `baud_domain::FdoNotify::of` decides, each hint under the name the
/// specification gives it (`[[source: freedesktop-notification-spec]]`),
/// the picture decoded (`Delivery::image_data`) as `image-data`, the id it
/// replaces, and baud's desktop entry.
pub(crate) fn notification(
    p: &crate::notify::Posted,
    replaces: Option<NotifyId>,
    caps: baud_domain::ServerCaps,
    image: Option<(baud_domain::png::Size, Vec<u8>)>,
) -> Notification {
    use baud_domain::{FdoExpire, FdoNotify, NoticeSound};
    use {Expire, Hint, Image, Octet, Truth};
    let n = FdoNotify::of(&p.fdo(), caps);
    let mut hints: Vec<(&'static str, Hint)> =
        vec![("urgency", Hint::Byte(Octet(n.urgency_level())))];
    match n.sound() {
        NoticeSound::System => {}
        NoticeSound::Silent => hints.push(("suppress-sound", Hint::Bool(Truth::True))),
        NoticeSound::Named(name) => hints.push(("sound-name", Hint::Str(name.to_string()))),
    }
    if let Some(kind) = n.category() {
        hints.push(("category", Hint::Str(kind.to_string())));
    }
    if let Some((size, rgba)) = image {
        hints.push(("image-data", Hint::Image(Image { size, rgba })));
    }
    hints.push(("desktop-entry", Hint::Str(DESKTOP_ENTRY.to_string())));
    Notification {
        app_name: n.app_name().to_string(),
        replaces,
        app_icon: n.icon().map(|i| i.as_str().to_string()).unwrap_or_default(),
        summary: n.summary().to_string(),
        body: n.body().collect(),
        actions: n
            .actions()
            .iter()
            .map(|(key, label)| (key.to_string(), label.to_string()))
            .collect(),
        hints,
        expire: match n.expire() {
            FdoExpire::Server => Expire::Server,
            FdoExpire::Never => Expire::Never,
            FdoExpire::After(d) => Expire::After(d.duration()),
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

    /// `ReadAll(["org.freedesktop.appearance", "org.gnome.desktop.interface"])`,
    /// the call the session sends, as serial 3, marshalled by another D-Bus
    /// implementation.
    const READ_ALL_INTERFACE: &str = include_str!("../fixtures/bus-read-all-interface.hex");

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
        let mut both = Writer::default();
        both.strings(&[APPEARANCE, INTERFACE]);
        let read_both = marshal(
            &Call {
                destination: PORTAL,
                path: PORTAL_PATH,
                interface: SETTINGS,
                member: "ReadAll",
                signature: "as",
                body: both.buf,
            },
            Serial(Word(3)),
        );
        assert_eq!(read_both, hex(READ_ALL_INTERFACE.trim()));
    }

    /// **The handshake, then `Hello`, then the calls queued behind it**:
    /// the identity in hex, file passing asked for after `OK`, `BEGIN` only
    /// after the answer, and the empty identity answered with an empty
    /// `DATA`. A refusal fails the session, said.
    #[test]
    fn the_handshake_sends_begin_and_hello_before_any_call() {
        let mut s = Session::new(Some(Uid(1000)));
        assert_eq!(s.outgoing(), b"\0AUTH EXTERNAL 31303030\r\n");
        let n = s.outgoing().len();
        s.sent(Len(n));
        assert_eq!(s.received(b"OK 1234deadbeef\r\n"), []);
        assert_eq!(s.outgoing(), b"NEGOTIATE_UNIX_FD\r\n");
        let n = s.outgoing().len();
        s.sent(Len(n));
        assert_eq!(s.received(b"AGREE_UNIX_FD\r\n"), []);
        let out = s.outgoing().to_vec();
        assert!(out.starts_with(b"BEGIN\r\n"), "{out:?}");
        assert_eq!(&out[7..7 + hex(HELLO).len()], hex(HELLO).as_slice());
        let after = &out[7 + hex(HELLO).len()..];
        let add = message_len(after).unwrap().unwrap();
        assert_eq!(&after[..add.0], reply_bytes("add_match_call").as_slice());
        let rest = &after[add.0..];
        let read_all = message_len(rest).unwrap().unwrap();
        assert_eq!(
            &rest[..read_all.0],
            hex(READ_ALL_INTERFACE.trim()).as_slice()
        );
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
        s.received(b"OK 1234deadbeef\r\nAGREE_UNIX_FD\r\n");
        let n = s.outgoing().len();
        s.sent(Len(n));
        s
    }

    /// **File passing is asked for, and a bus that refuses it is still a
    /// bus**: `OK` and `AGREE_UNIX_FD` in one read begin; an `ERROR` to
    /// the ask begins too, and a call that carries a file is then refused
    /// rather than sent without it (`[[source: dbus-spec]]`, "a client
    /// should not send any UNIX FDs ... if the server did not agree").
    #[test]
    fn file_passing_is_negotiated_before_begin() {
        let mut s = Session::new(Some(Uid(1000)));
        let n = s.outgoing().len();
        s.sent(Len(n));
        assert_eq!(s.received(b"OK 12\r\nAGREE_UNIX_FD\r\n"), []);
        assert!(s.outgoing().starts_with(b"NEGOTIATE_UNIX_FD\r\nBEGIN\r\n"));
        let mut no = Session::new(Some(Uid(1000)));
        let n = no.outgoing().len();
        no.sent(Len(n));
        no.received(b"OK 12\r\n");
        assert_eq!(no.received(b"ERROR not here\r\n"), []);
        assert!(no.outgoing().starts_with(b"NEGOTIATE_UNIX_FD\r\nBEGIN\r\n"));
        assert!(!no.failed());
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(
            no.call_with_file(&PRINT_CALL, one_file, file.into())
                .is_err()
        );
    }

    /// A body that is one file.
    fn one_file(fd: UnixFd) -> Result<Body, EncodeError> {
        Body::of(&(fd,))
    }

    /// **A message that carries a file says how many in its header**, the
    /// `UNIX_FDS` field (code 9, a `u`) after the signature, and the file
    /// goes with that message's first byte, not before it.
    #[test]
    fn a_call_with_a_file_names_it_in_its_header() {
        let mut s = ready();
        s.reveal("file:///tmp/x");
        let ahead = s.outgoing().len();
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(s.call_with_file(&PRINT_CALL, one_file, file.into()).is_ok());
        let (bytes, fds) = s.next_send();
        assert_eq!((bytes.len(), fds.len()), (ahead, 0), "the reveal first");
        s.sent(Len(ahead));
        let (bytes, fds) = s.next_send();
        assert_eq!(fds.len(), 1, "the file with the call's first byte");
        let field = [9u8, 1, b'u', 0, 1, 0, 0, 0];
        let header = &bytes[..16 + usize::from(bytes[12])];
        assert!(
            header.windows(8).any(|w| w == field),
            "no UNIX_FDS 1 in {header:?}"
        );
        let n = bytes.len();
        s.sent(Len(n));
        let (bytes, fds) = s.next_send();
        assert!(bytes.is_empty() && fds.is_empty());
    }

    /// **The file reaches the other end with the call**: over a socket
    /// pair, the peer reads the call and one descriptor for the same file.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_crosses_the_socket_with_its_call() {
        use std::os::unix::fs::MetadataExt;
        let (ours, theirs) = UnixStream::pair().unwrap();
        let mut bus = Bus::over(ours, Some(Uid(1000))).unwrap();
        bus.session.received(b"OK 12\r\nAGREE_UNIX_FD\r\n");
        let _ = bus.flush();
        let mut sink = vec![0u8; 8192];
        let mut fds = Vec::new();
        let _ = baud_wayland::sys::recv(&theirs, &mut sink, &mut fds).unwrap();
        let file = std::fs::File::open("/dev/null").unwrap();
        let ino = file.metadata().unwrap().ino();
        let serial = bus
            .session
            .call_with_file(&PRINT_CALL, one_file, file.into());
        assert!(serial.is_ok());
        bus.flush().unwrap();
        let n = baud_wayland::sys::recv(&theirs, &mut sink, &mut fds).unwrap();
        assert!(message_len(&sink[..n]).is_some(), "a whole call");
        assert_eq!(fds.len(), 1);
        let got = std::fs::File::from(fds.remove(0));
        assert_eq!(got.metadata().unwrap().ino(), ino);
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

    /// **The desktop's animations setting is read and followed**:
    /// `org.gnome.desktop.interface enable-animations`, a `b`, read beside
    /// the appearance in the one `ReadAll` -- here a reply with no
    /// `reduced-motion`, as a portal without it gives -- and followed through
    /// its `SettingChanged`, off and on. Fixtures are another D-Bus
    /// implementation's bytes. Perturbation: drop the signal's arm -- the
    /// change is unheard.
    #[test]
    fn the_desktops_animations_setting_is_read_and_followed() {
        let read = |fixture: &str| ready().received(&reply_bytes(fixture));
        let appearance = || {
            vec![
                Heard::Scheme(Scheme::Dark, Origin::Read),
                Heard::Contrast(Contrast::NoPreference),
            ]
        };
        for (fixture, on) in [
            ("read_all_animations_off", Animations::Off),
            ("read_all_animations_on", Animations::On),
        ] {
            let mut want = appearance();
            want.push(Heard::Animations(on));
            assert_eq!(read(fixture), want, "{fixture}");
        }
        let mut s = ready();
        for (fixture, want) in [
            ("changed_animations_off", Heard::Animations(Animations::Off)),
            ("changed_animations_on", Heard::Animations(Animations::On)),
        ] {
            assert_eq!(s.received(&reply_bytes(fixture)), [want], "{fixture}");
        }
    }

    /// **The portal's text-scaling factor is read and followed**:
    /// `org.gnome.desktop.interface text-scaling-factor`, a `d`, read
    /// beside the appearance in the one `ReadAll` (the namespace's other
    /// keys are passed over) and followed through its `SettingChanged`; a
    /// factor that is not a positive number says nothing, and a reply
    /// without one says nothing of it (`read_all_accessible`, above).
    /// Fixtures are another D-Bus implementation's bytes. Perturbation: drop
    /// the signal's interface arm -- the change is unheard.
    #[test]
    fn the_portals_text_scale_is_read_and_followed() {
        let read = |fixture: &str| ready().received(&reply_bytes(fixture));
        let appearance = || {
            vec![
                Heard::Scheme(Scheme::Dark, Origin::Read),
                Heard::Contrast(Contrast::Higher),
                Heard::Motion(Motion::Reduced),
            ]
        };
        let mut with = appearance();
        with.push(Heard::TextScale(TextScale::of(1.25).unwrap()));
        assert_eq!(read("read_all_text_scale"), with);
        assert_eq!(read("read_all_text_scale_zero"), appearance());
        let mut s = ready();
        assert_eq!(
            s.received(&reply_bytes("changed_text_scale")),
            [Heard::TextScale(TextScale::of(1.5).unwrap())]
        );
    }

    /// **The text scale is heard, never polled**: the one `ReadAll` is
    /// asked once, when the session is made (`Session::new`), and changes
    /// arrive as `SettingChanged`; nothing asks again on a timer or a turn.
    /// Fed control: the same check over this file with a second ask added
    /// fails, so the check can fire. The needles are spelt in pieces so
    /// this test's own text does not count.
    #[test]
    fn the_text_scale_is_heard_not_polled() {
        const BUS: &str = include_str!("bus.rs");
        let ask = concat!(".ask_", "appearance()");
        let asked_once = |src: &str| src.matches(ask).count() == 1;
        assert!(asked_once(BUS), "one ask, at the session's start");
        let after = concat!("s.ask_", "notifications();");
        let fed = BUS.replacen(after, &format!("{after}\n        s{ask};"), 1);
        assert!(!asked_once(&fed), "the control: a second ask is seen");
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
    /// once. The bus's end is closed by `shutdown`, which ends the
    /// connection whoever else holds a copy of it: another test's child,
    /// forked in this process and not yet exec'd, holds one of every
    /// descriptor open at its fork, and a drop alone left the connection
    /// open until that child exec'd, so the closed bus was not said (1 full
    /// `cargo test` run of 16). A copy held here for the whole close
    /// makes that so on every run. Perturbations: take a read of nothing for
    /// no news -- the closed bus is never said; drop the bus's end without
    /// the shutdown -- the same.
    #[test]
    fn the_bus_moves_bytes_without_waiting() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        let mut bus = Bus::over(ours, Some(Uid(1000))).unwrap();
        assert!(bus.wants_write());
        assert_eq!(bus.pump(), []);
        let mut got = vec![0u8; 64];
        let n = theirs.read(&mut got).unwrap();
        assert_eq!(&got[..n], b"\0AUTH EXTERNAL 31303030\r\n");
        theirs.write_all(b"OK 0123\r\nAGREE_UNIX_FD\r\n").unwrap();
        assert_eq!(bus.pump(), []);
        let mut sink = vec![0u8; 4096];
        let n = theirs.read(&mut sink).unwrap();
        assert!(
            sink[..n].starts_with(b"NEGOTIATE_UNIX_FD\r\nBEGIN\r\n"),
            "{:?}",
            &sink[..n]
        );
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
        let copy = theirs.try_clone().unwrap();
        theirs.shutdown(std::net::Shutdown::Both).unwrap();
        drop(theirs);
        assert!(matches!(bus.pump().as_slice(), [Heard::Closed(_)]));
        assert_eq!(bus.pump(), [], "said once");
        drop(copy);
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
        t.set_notice_support(baud_domain::notice_support(
            baud_domain::NoticeSetting::On,
            baud_domain::NoticePlatform::MacOs,
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
        )
        .expect("sent");
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
        )
        .expect("sent");
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

    /// **`FdoNotify::encode` is the oracle fixtures' bytes**
    /// (`fixtures/bus-replies.hex`), the serial drawn from the connection's
    /// counter: the call with every hint type, then the one that replaces
    /// 42 with the server's timeout.
    #[test]
    fn the_notify_entry_point_is_the_oracles_bytes() {
        use baud_domain::dbus::{
            Boolean, Byte, Bytes, FdoNotify, I32, ImageData, Serials, Str, Variant,
        };
        let mut serials = Serials::new();
        for _ in 2..6 {
            let _ = serials.next();
        }
        let actions = [
            (Str::lit("default"), Str::lit("Open")),
            (Str::lit("1"), Str::lit("Yes")),
        ];
        let pixels = [255, 0, 0, 255, 0, 255, 0, 128];
        let hints = [
            (Str::lit("urgency"), Variant::Byte(Byte::new(2))),
            (Str::lit("suppress-sound"), Variant::Boolean(Boolean::True)),
            (Str::lit("sound-name"), Variant::Str(Str::lit("bell"))),
            (
                Str::lit("image-data"),
                Variant::Image(ImageData {
                    width: I32::new(2),
                    height: I32::new(1),
                    rowstride: I32::new(8),
                    alpha: Boolean::True,
                    bits_per_sample: I32::new(8),
                    channels: I32::new(4),
                    data: Bytes::new(&pixels),
                }),
            ),
        ];
        let first = FdoNotify {
            app_name: Str::lit("baud"),
            replaces: 0,
            app_icon: Str::lit("dialog-error"),
            summary: Str::lit("Build"),
            body: Str::lit("make; make install"),
            actions: &actions,
            hints: &hints,
            expire: 5000,
        };
        let e = first.encode(&mut serials).expect("within the bound");
        assert_eq!(e.serial().get(), 6);
        assert_eq!(e.bytes(), reply_bytes("notify_call").as_slice());
        let again = FdoNotify {
            app_name: Str::EMPTY,
            replaces: 42,
            app_icon: Str::EMPTY,
            summary: Str::lit("Again"),
            body: Str::EMPTY,
            actions: &[],
            hints: &[],
            expire: -1,
        };
        let e = again.encode(&mut serials).expect("within the bound");
        assert_eq!(e.serial().get(), 7);
        assert_eq!(e.bytes(), reply_bytes("notify_replace_call").as_slice());
    }

    /// **A `Notify` whose reply slot is held is refused, counted and not
    /// sent**: `GetCapabilities` (serial 5) unanswered while 31 closes draw
    /// 6 to 36, so the next call, 37, finds slot 5 held. The one after it
    /// has a free slot and goes. Perturbation: send it though the slot is
    /// held -- the first notify is `Ok`.
    #[test]
    fn a_notify_whose_reply_slot_is_held_is_refused_and_counted() {
        let mut s = ready();
        for _ in 6..37 {
            s.close_notification(NotifyId(Word(42)));
        }
        let n = s.outgoing().len();
        s.sent(Len(n));
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
        let tok = token();
        assert_eq!(s.notify(&quiet, tok), Err(NotSent::Held));
        assert_eq!(s.outgoing(), b"", "a refused call is not sent");
        assert_eq!(s.awaiting.refusals().get(), 1);
        assert_eq!(s.notify(&quiet, tok), Ok(()));
        assert_eq!(last_serial(&s), Serial(Word(38)));
        assert_eq!(s.awaiting.refusals().get(), 1);
    }

    /// A notification of nothing much, for the replies' tests.
    fn quiet() -> Notification {
        Notification {
            app_name: String::new(),
            replaces: None,
            app_icon: String::new(),
            summary: "x".into(),
            body: String::new(),
            actions: Vec::new(),
            hints: Vec::new(),
            expire: Expire::Never,
        }
    }

    /// **What the server says back**: its capabilities, a notification's
    /// id or an error, a click with its key, a close. Fixtures are
    /// the oracle marshaller's bytes, the server's carrying its SENDER, `:1.7`, as the
    /// bus sets it. Perturbation: read `ActionInvoked`'s key before its id
    /// -- the click is unheard.
    #[test]
    fn the_servers_answers_are_heard() {
        use baud_domain::ServerCaps;
        let mut s = ready();
        assert_eq!(
            s.received(&reply_bytes("capabilities_reply_holder")),
            [Heard::Caps(ServerCaps::heard([
                "actions",
                "body",
                "body-markup",
                "persistence",
            ]))]
        );
        let mut s = ready();
        assert_eq!(
            s.received(&reply_bytes("capabilities_bare_holder")),
            [Heard::Caps(ServerCaps::heard(["body"]))]
        );
        let tok = token();
        s.notify(&quiet(), tok).expect("sent");
        assert_eq!(
            s.received(&reply_bytes("notify_reply_holder")),
            [Heard::Notified(tok, NotifyId(Word(42)))]
        );
        assert_eq!(
            s.received(&reply_bytes("action_invoked_holder")),
            [Heard::Invoked(NotifyId(Word(42)), "1".into(), None)]
        );
        assert_eq!(
            s.received(&reply_bytes("notification_closed_holder")),
            [Heard::NotifyClosed(NotifyId(Word(42)))]
        );
        let mut none = ready();
        assert_eq!(
            none.received(&reply_bytes("capabilities_error")),
            [Heard::NoServer(
                "org.freedesktop.DBus.Error.ServiceUnknown".into()
            )]
        );
        let mut s = ready();
        s.notify(&quiet(), tok).expect("sent");
        assert_eq!(
            s.received(&reply_bytes("notify_error")),
            [Heard::NotNotified(
                tok,
                "org.freedesktop.DBus.Error.ServiceUnknown".into()
            )]
        );
    }

    /// **The server's signals are acted on only from the connection that
    /// owns its name** (`[[source: dbus-spec]]`, "Header Fields": SENDER
    /// "is controlled by the message bus"): the holder is the SENDER of the
    /// server's first reply. The same click from another connection, or
    /// with no SENDER at all, says nothing; the holder's is heard.
    #[test]
    fn a_signal_from_a_connection_that_does_not_own_the_name_is_not_acted_on() {
        let mut s = ready();
        assert!(matches!(
            s.received(&reply_bytes("capabilities_reply_holder"))
                .as_slice(),
            [Heard::Caps(_)]
        ));
        assert_eq!(s.received(&reply_bytes("action_invoked_other")), []);
        assert_eq!(s.received(&reply_bytes("action_invoked")), [], "no SENDER");
        assert_eq!(s.received(&reply_bytes("notification_closed")), []);
        assert_eq!(
            s.received(&reply_bytes("action_invoked_holder")),
            [Heard::Invoked(NotifyId(Word(42)), "1".into(), None)],
            "the control: the holder's click"
        );
        // A reply with no SENDER names no holder, so nothing is acted on.
        let mut bare = ready();
        assert!(matches!(
            bare.received(&reply_bytes("capabilities_reply")).as_slice(),
            [Heard::Caps(_)]
        ));
        assert_eq!(bare.received(&reply_bytes("action_invoked_holder")), []);
    }

    /// **A change of hands names the new holder**: once the bus says
    /// `org.freedesktop.Notifications` passed from `:1.7` to `:1.9`, the
    /// old holder's click says nothing and `:1.9`'s is heard. The bus is asked for the
    /// change once the first holder is known, as serial 6.
    #[test]
    fn after_the_name_changes_hands_the_old_holders_signal_is_not_acted_on() {
        let mut s = ready();
        let _ = s.received(&reply_bytes("capabilities_reply_holder"));
        assert_eq!(s.received(&reply_bytes("name_changed_hands")), []);
        assert_eq!(
            s.received(&reply_bytes("action_invoked_holder")),
            [],
            "the old holder"
        );
        let watch = s.outgoing().to_vec();
        assert_eq!(last_serial(&s), Serial(Word(6)));
        let rule = b"member='NameOwnerChanged',path='/org/freedesktop/DBus',arg0='org.freedesktop.Notifications'";
        assert!(watch.windows(rule.len()).any(|w| w == rule), "{watch:?}");
        let n = s.outgoing().len();
        s.sent(Len(n));
        let tok = token();
        s.notify(&quiet(), tok).expect("sent");
        assert_eq!(last_serial(&s), Serial(Word(7)));
        let n = s.outgoing().len();
        s.sent(Len(n));
        // Serial 7 goes unanswered; the new holder answers 8.
        s.notify(&quiet(), tok).expect("sent");
        assert_eq!(last_serial(&s), Serial(Word(8)));
        assert_eq!(
            s.received(&reply_bytes("notify_reply_other")),
            [Heard::Notified(tok, NotifyId(Word(43)))]
        );
        assert_eq!(
            s.received(&reply_bytes("action_invoked_other")),
            [Heard::Invoked(NotifyId(Word(42)), "1".into(), None)],
            "the new holder"
        );
        assert_eq!(s.received(&reply_bytes("action_invoked_holder")), []);
    }

    /// **A reply never names the holder once the bus has**: `:1.7`
    /// answers serial 7, sent before the name passed to `:1.9`, after the
    /// bus says so. The reply reaches its call, and `:1.7`'s click still
    /// says nothing while `:1.9`'s is heard.
    #[test]
    fn a_late_reply_from_the_old_holder_does_not_make_it_the_holder_again() {
        let mut s = ready();
        let _ = s.received(&reply_bytes("capabilities_reply_holder"));
        let n = s.outgoing().len();
        s.sent(Len(n));
        let tok = token();
        s.notify(&quiet(), tok).expect("sent");
        assert_eq!(last_serial(&s), Serial(Word(7)));
        let n = s.outgoing().len();
        s.sent(Len(n));
        assert_eq!(s.received(&reply_bytes("name_changed_hands")), []);
        assert_eq!(
            s.received(&reply_bytes("notify_reply_holder")),
            [Heard::Notified(tok, NotifyId(Word(42)))]
        );
        assert_eq!(
            s.received(&reply_bytes("action_invoked_holder")),
            [],
            "the old holder"
        );
        assert_eq!(
            s.received(&reply_bytes("action_invoked_other")),
            [Heard::Invoked(NotifyId(Word(42)), "1".into(), None)],
            "the holder the bus names"
        );
    }

    /// **A reply is read only as its own signature**: `GetCapabilities`
    /// answered with an `s` rather than its `as` claims nothing, as no
    /// server does.
    #[test]
    fn a_reply_whose_body_is_not_its_methods_signature_is_refused() {
        let mut s = ready();
        assert_eq!(
            s.received(&reply_bytes("capabilities_not_as_holder")),
            [Heard::NoServer(
                "GetCapabilities answered `s`: a body is read only as its own signature".into()
            )]
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
        theirs.write_all(b"OK 0123\r\nAGREE_UNIX_FD\r\n").unwrap();
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
        )
        .expect("sent");
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
        use crate::notify::{Icons, Posted};
        use baud_domain::ServerCaps;
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut owned =
            baud_core::Owned::new(&ws).with_notices(baud_core::NoticeBudget::with_icon(1024));
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        let caps = ServerCaps::heard(["actions", "body-markup"]);
        t.set_notice_support(baud_domain::notice_support(
            baud_domain::NoticeSetting::On,
            baud_domain::NoticePlatform::Linux(caps),
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
                ("desktop-entry", Hint::Str(DESKTOP_ENTRY.into())),
            ]
        );
        assert_eq!(
            n.expire,
            Expire::After(core::time::Duration::from_millis(2500))
        );
        let plain = notification(&p, Some(NotifyId(Word(9))), ServerCaps::heard([]), None);
        assert_eq!(plain.body, "a < b & c");
        assert_eq!(plain.replaces, Some(NotifyId(Word(9))));
    }

    /// **Every notification names baud's desktop file** in `desktop-entry`, its
    /// name without `.desktop` -- the file `nix/packaging/linux` installs and the
    /// launcher's URI names -- whatever application name a program gave, so a
    /// server shows baud's icon where the program named none. Perturbation:
    /// leave the hint out -- no hint names the entry.
    #[test]
    fn a_notification_names_baud_s_desktop_entry() {
        use crate::notify::{Icons, Posted};
        use baud_domain::ServerCaps;
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut owned =
            baud_core::Owned::new(&ws).with_notices(baud_core::NoticeBudget::with_icon(1024));
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        t.set_notice_support(baud_domain::notice_support(
            baud_domain::NoticeSetting::On,
            baud_domain::NoticePlatform::Linux(ServerCaps::heard([])),
        ));
        let b = "\x1b]99;;Done\x1b\\".as_bytes();
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        let p = t
            .notices()
            .map(|n| Posted::of(&n, &mut Icons::default()))
            .next()
            .expect("posted");
        let n = notification(&p, None, ServerCaps::heard([]), None);
        assert!(
            n.hints
                .contains(&("desktop-entry", Hint::Str(DESKTOP_ENTRY.into()))),
            "{:?}",
            n.hints
        );
        assert_eq!(
            LAUNCHER_URI,
            format!("application://{DESKTOP_ENTRY}.desktop")
        );
        let file = format!(
            "{}/../../nix/packaging/linux/{DESKTOP_ENTRY}.desktop",
            env!("CARGO_MANIFEST_DIR")
        );
        assert!(std::path::Path::new(&file).exists(), "{file}");
    }

    /// **iTerm2's subtitle opens the body as a paragraph of its own**, a
    /// blank line before the message, escaped with it for a server that
    /// reads markup; with no title it is the summary; with no message it is
    /// the body.
    #[test]
    fn an_iterm2_subtitle_opens_the_body_or_stands_as_the_summary() {
        use crate::notify::{Icons, Posted};
        use baud_domain::ServerCaps;
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let caps = ServerCaps::heard(["actions", "body-markup"]);
        let sent = |b64s: &str| {
            let mut owned =
                baud_core::Owned::new(&ws).with_notices(baud_core::NoticeBudget::with_icon(1024));
            let mut t = baud_core::Term::new(owned.storage(), &ws);
            t.set_notice_support(baud_domain::notice_support(
                baud_domain::NoticeSetting::On,
                baud_domain::NoticePlatform::Linux(caps),
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
        use crate::notify::{Icons, Posted};
        use baud_domain::ServerCaps;
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let caps = ServerCaps::heard(["actions"]);
        let offered = |a: &str| {
            let mut owned =
                baud_core::Owned::new(&ws).with_notices(baud_core::NoticeBudget::with_icon(1024));
            let mut t = baud_core::Term::new(owned.storage(), &ws);
            t.set_notice_support(baud_domain::notice_support(
                baud_domain::NoticeSetting::On,
                baud_domain::NoticePlatform::Linux(caps),
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
    /// carries none. The signals are the oracle marshaller's bytes from the holder of
    /// the server's name, known from its `GetCapabilities` reply.
    #[test]
    fn an_activation_token_comes_with_its_notifications_click() {
        let id = |n| NotifyId(Word(n));
        let mut s = ready();
        let _ = s.received(&reply_bytes("capabilities_reply_holder"));
        assert_eq!(s.received(&reply_bytes("token_42_holder")), []);
        assert_eq!(
            s.received(&reply_bytes("invoked_42_default_holder")),
            [Heard::Invoked(
                id(42),
                "default".into(),
                Some(baud_domain::XdgToken::heard("tok-42"))
            )]
        );
        assert_eq!(
            s.received(&reply_bytes("invoked_42_default_holder")),
            [Heard::Invoked(id(42), "default".into(), None)],
            "a click takes the token"
        );
        assert_eq!(s.received(&reply_bytes("token_7_holder")), []);
        assert_eq!(
            s.received(&reply_bytes("invoked_42_default_holder")),
            [Heard::Invoked(id(42), "default".into(), None)],
            "another notification's token"
        );
        assert_eq!(
            s.received(&reply_bytes("invoked_7_1_holder")),
            [Heard::Invoked(
                id(7),
                "1".into(),
                Some(baud_domain::XdgToken::heard("tok-7"))
            )],
            "kept for its own"
        );
        assert_eq!(s.received(&reply_bytes("token_7b_holder")), []);
        assert_eq!(
            s.received(&reply_bytes("closed_7_holder")),
            [Heard::NotifyClosed(id(7))]
        );
        assert_eq!(
            s.received(&reply_bytes("invoked_7_default_holder")),
            [Heard::Invoked(id(7), "default".into(), None)],
            "a close drops it"
        );
    }

    /// A method's reply to `serial`, of `sig` and `body`, as a bus sends
    /// it: the reply serial and the signature in its header.
    fn reply_to(serial: Serial, kind: Kind, sig: &str, body: Vec<u8>, error: &str) -> Vec<u8> {
        let mut w = Writer::default();
        w.buf.extend_from_slice(&[b'l', kind.0, 0, 1]);
        w.word(Word(u32::try_from(body.len()).unwrap()));
        w.word(Word(900));
        w.word(Word(0));
        let start = w.buf.len();
        w.align(Align::EIGHT);
        w.buf.push(FIELD_REPLY_SERIAL.0);
        w.signature("u");
        w.word(serial.0);
        if !error.is_empty() {
            w.align(Align::EIGHT);
            w.buf.push(FIELD_ERROR_NAME.0);
            w.signature("s");
            w.string(error);
        }
        if !sig.is_empty() {
            w.align(Align::EIGHT);
            w.buf.push(FIELD_SIGNATURE.0);
            w.signature("g");
            w.signature(sig);
        }
        let len = u32::try_from(w.buf.len() - start).unwrap();
        w.buf[12..16].copy_from_slice(&len.to_le_bytes());
        w.align(Align::EIGHT);
        w.buf.extend_from_slice(&body);
        w.buf
    }

    /// A session that has its unique name, `:1.42`.
    fn named() -> Session {
        let mut s = ready();
        let mut b = Writer::default();
        b.string(":1.42");
        assert_eq!(
            s.received(&reply_to(Serial(Word(1)), METHOD_RETURN, "s", b.buf, "")),
            []
        );
        s
    }

    /// The serial of the last call in what `s` would send.
    fn last_serial(s: &Session) -> Serial {
        let mut rest = s.outgoing();
        let mut last = Serial(Word(0));
        while let Some(Ok(n)) = message_len(rest) {
            last = Serial(word_in(rest[8..12].try_into().unwrap(), Order::Little));
            rest = &rest[n.0..];
        }
        last
    }

    const PATH_1: &str = "/org/freedesktop/portal/desktop/request/1_42/baud_1";

    /// A `Response` on `path` from `:1.9`, the portal's holder in
    /// [`with_portal`]: `code`, and the dialog's page setup and token.
    fn response(path: &str, code: Word, token: Option<Word>) -> Vec<u8> {
        response_from(":1.9", path, code, token)
    }

    /// A `Response` on `path` with no SENDER: `code`, and the dialog's page
    /// setup and token.
    fn unsent_response(path: &str, code: Word, token: Option<Word>) -> Vec<u8> {
        setup_response(
            path,
            code,
            token,
            &[("Width", 210.0), ("Height", 297.0), ("MarginTop", 6.35)],
        )
    }

    /// A `Response` on `path` with no SENDER: `code`, the page setup's
    /// `setup` keys, each a `d`, and the token.
    fn setup_response(
        path: &str,
        code: Word,
        token: Option<Word>,
        setup: &[(&str, f64)],
    ) -> Vec<u8> {
        let mut b = Writer::default();
        b.word(code);
        b.array(Align::EIGHT, |w| {
            w.align(Align::EIGHT);
            w.string("page-setup");
            w.signature("a{sv}");
            w.array(Align::EIGHT, |w| {
                for &(k, v) in setup {
                    w.align(Align::EIGHT);
                    w.string(k);
                    w.signature("d");
                    w.double(v);
                }
                w.align(Align::EIGHT);
                w.string("Orientation");
                w.signature("s");
                w.string("portrait");
            });
            if let Some(t) = token {
                w.align(Align::EIGHT);
                w.string("token");
                w.signature("u");
                w.word(t);
            }
        });
        marshal_signal(
            &Call {
                destination: "",
                path,
                interface: "org.freedesktop.portal.Request",
                member: "Response",
                signature: "ua{sv}",
                body: b.buf,
            },
            Serial(Word(901)),
        )
    }

    fn a4() -> PageSetup {
        PageSetup {
            width: baud_domain::PaperMm::of(210.0),
            height: baud_domain::PaperMm::of(297.0),
            top: baud_domain::PaperMm::of(6.35),
            ..PageSetup::default()
        }
    }

    /// **The dialog's `Response` is matched before the dialog is asked
    /// for**, on the path the unique name and the token predict; and an
    /// answer that comes before `PreparePrint`'s own reply is heard.
    /// Perturbation: add the match after the call -- the first assertion
    /// fails.
    #[test]
    fn the_dialog_s_answer_is_matched_before_it_is_asked() {
        let mut s = with_portal();
        let n = s.outgoing().len();
        s.sent(Len(n));
        let export = exported("h-1");
        s.prepare_print(export.parent(), "baud \u{2014} print");
        let out = s.outgoing().to_vec();
        let at = |needle: &[u8]| out.windows(needle.len()).position(|w| w == needle);
        let rule = at(PATH_1.as_bytes()).expect("the match names the path");
        let call = at(b"PreparePrint").expect("the call");
        assert!(rule < call, "the match before the call");
        assert!(at(b"supported_output_file_formats").is_some());
        assert!(
            at(b"wayland:h-1").is_some_and(|p| p < at(b"supported").unwrap()),
            "the parent window"
        );
        let serial = last_serial(&s);
        s.sent(Len(out.len()));
        // The answer first, then the method's reply.
        assert_eq!(
            s.received(&response(PATH_1, Word(0), Some(Word(7)))),
            [Heard::PrintReady(PrintToken(Word(7)), a4())]
        );
        assert!(
            s.outgoing().windows(11).any(|w| w == b"RemoveMatch"),
            "the match removed"
        );
        let mut path = Writer::default();
        path.string(PATH_1);
        assert_eq!(
            s.received(&reply_to(serial, METHOD_RETURN, "o", path.buf, "")),
            []
        );
    }

    /// **A dialog asked for before the name is known waits for it.**
    #[test]
    fn a_dialog_asked_for_before_the_name_waits_for_it() {
        let mut s = ready();
        s.prepare_print(ParentWindow::Unnamed, "t");
        assert!(!s.outgoing().windows(12).any(|w| w == b"PreparePrint"));
        let mut b = Writer::default();
        b.string(":1.42");
        s.received(&reply_to(Serial(Word(1)), METHOD_RETURN, "s", b.buf, ""));
        assert!(s.outgoing().windows(12).any(|w| w == b"PreparePrint"));
        assert!(
            s.outgoing()
                .windows(PATH_1.len())
                .any(|w| w == PATH_1.as_bytes())
        );
    }

    /// **The document goes with the token, and the exchange ends as the
    /// dialog says**: printed, cancelled, or an error to the call, which
    /// removes the match too.
    #[test]
    fn the_document_prints_with_the_dialog_s_token() {
        let path_2 = "/org/freedesktop/portal/desktop/request/1_42/baud_2";
        let mut s = with_portal();
        s.prepare_print(ParentWindow::Unnamed, "t");
        s.received(&response(PATH_1, Word(0), Some(Word(7))));
        let n = s.outgoing().len();
        s.sent(Len(n));
        let pdf = std::fs::File::open("/dev/null").unwrap();
        assert_eq!(
            s.print(
                exported("h-1").parent(),
                "t",
                pdf.into(),
                PrintToken(Word(7))
            ),
            Ok(())
        );
        let (bytes, fds) = s.next_send();
        assert!(fds.is_empty(), "the match goes first, alone");
        assert!(bytes.windows(path_2.len()).any(|w| w == path_2.as_bytes()));
        let n = bytes.len();
        s.sent(Len(n));
        let (bytes, fds) = s.next_send();
        assert_eq!(fds.len(), 1, "the document with the call");
        assert!(
            bytes.windows(11).any(|w| w == b"wayland:h-1"),
            "the parent window"
        );
        assert!(bytes.windows(5).any(|w| w == b"Print"));
        assert_eq!(
            s.received(&response(path_2, Word(0), None)),
            [Heard::PrintEnded(PrintEnd::Printed)]
        );

        let mut c = with_portal();
        c.prepare_print(ParentWindow::Unnamed, "t");
        assert_eq!(
            c.received(&response(PATH_1, Word(1), None)),
            [Heard::PrintEnded(PrintEnd::Cancelled)]
        );

        let mut e = with_portal();
        e.prepare_print(ParentWindow::Unnamed, "t");
        let serial = last_serial(&e);
        let n = e.outgoing().len();
        e.sent(Len(n));
        let refused = reply_to(
            serial,
            ERROR,
            "",
            Vec::new(),
            "org.freedesktop.DBus.Error.UnknownMethod",
        );
        assert_eq!(
            e.received(&refused),
            [Heard::PrintEnded(PrintEnd::Failed(
                "org.freedesktop.DBus.Error.UnknownMethod".into()
            ))]
        );
        assert!(e.outgoing().windows(11).any(|w| w == b"RemoveMatch"));
        assert_eq!(
            e.received(&response(PATH_1, Word(0), Some(Word(1)))),
            [],
            "no longer waited for"
        );
    }

    /// **Whether there is a print portal**: its `version`, or the error.
    #[test]
    fn the_print_portal_s_version_is_asked() {
        let mut s = ready();
        s.ask_print_portal();
        let serial = last_serial(&s);
        let mut b = Writer::default();
        b.signature("u");
        b.word(Word(4));
        assert_eq!(
            s.received(&reply_to(serial, METHOD_RETURN, "v", b.buf, "")),
            [Heard::PrintPortal(Ok(PortalVersion(4)))]
        );
        s.ask_print_portal();
        let serial = last_serial(&s);
        let no = reply_to(
            serial,
            ERROR,
            "",
            Vec::new(),
            "org.freedesktop.DBus.Error.InvalidArgs",
        );
        assert_eq!(
            s.received(&no),
            [Heard::PrintPortal(Err(
                "org.freedesktop.DBus.Error.InvalidArgs".into()
            ))]
        );
    }

    /// **A handle the portal moved is followed**: `PreparePrint`'s reply
    /// names another Request path, the match moves to it, and its
    /// `Response` is heard (`[[source: xdg-portal-request]]`: "verify that
    /// the returned handle is what it expected, and update its signal
    /// subscription if it isn't"). Perturbation: ignore the reply's path --
    /// no match names the moved path and its answer is not heard.
    #[test]
    fn a_moved_handle_is_followed() {
        let moved = "/org/freedesktop/portal/desktop/request/1_42/elsewhere";
        let mut s = with_portal();
        s.prepare_print(ParentWindow::Unnamed, "t");
        let serial = last_serial(&s);
        let n = s.outgoing().len();
        s.sent(Len(n));
        let mut path = Writer::default();
        path.string(moved);
        assert_eq!(
            s.received(&reply_to(serial, METHOD_RETURN, "o", path.buf, "")),
            []
        );
        let out = s.outgoing().to_vec();
        let at = |needle: &[u8]| out.windows(needle.len()).position(|w| w == needle);
        let removed = at(b"RemoveMatch").expect("the old match removed");
        let added = at(b"AddMatch").expect("a match for the moved path");
        assert!(removed < added, "removed, then added");
        assert!(
            at(moved.as_bytes()).is_some_and(|p| p > added),
            "the new match names the moved path"
        );
        assert_eq!(
            s.received(&response(PATH_1, Word(0), Some(Word(7)))),
            [],
            "the predicted path no longer answers"
        );
        assert_eq!(
            s.received(&response(moved, Word(0), Some(Word(7)))),
            [Heard::PrintReady(PrintToken(Word(7)), a4())]
        );
    }

    /// **Code 2, and a code the text does not give, end the exchange as
    /// failed**, each with its code said; the unknown code is counted
    /// (`[[source: xdg-portal-request]]`: 0, 1 and 2 are the codes).
    #[test]
    fn an_ended_or_unknown_response_fails_and_the_unknown_is_counted() {
        for (code, counted) in [(2, UnknownCodes(0)), (7, UnknownCodes(1))] {
            let mut s = with_portal();
            s.prepare_print(ParentWindow::Unnamed, "t");
            let h = s.received(&response(PATH_1, Word(code), Some(Word(1))));
            let [Heard::PrintEnded(PrintEnd::Failed(e))] = h.as_slice() else {
                panic!("{code}: {h:?}");
            };
            assert!(e.contains(&code.to_string()), "{e}");
            assert_eq!(s.unknown_codes(), counted, "{code}");
            assert_eq!(
                s.received(&response(PATH_1, Word(0), Some(Word(1)))),
                [],
                "{code}: the exchange is over"
            );
        }
    }

    /// `msg` with a SENDER field, as the bus sets it on every message it
    /// routes (`[[source: dbus-spec]]`, "Header Fields"): field code 7, a
    /// STRING, after the fields already there.
    fn with_sender(msg: &[u8], sender: &str) -> Vec<u8> {
        let size =
            |at| usize::try_from(u32::from_le_bytes(msg[at..at + 4].try_into().unwrap())).unwrap();
        let body = &msg[msg.len() - size(4)..];
        let mut w = Writer::default();
        w.buf.extend_from_slice(&msg[..16 + size(12)]);
        w.align(Align::EIGHT);
        w.buf.push(7);
        w.signature("s");
        w.string(sender);
        let len = u32::try_from(w.buf.len() - 16).unwrap();
        w.buf[12..16].copy_from_slice(&len.to_le_bytes());
        w.align(Align::EIGHT);
        w.buf.extend_from_slice(body);
        w.buf
    }

    /// A `Response` on `path` from the connection `sender`.
    fn response_from(sender: &str, path: &str, code: Word, token: Option<Word>) -> Vec<u8> {
        with_sender(&unsent_response(path, code, token), sender)
    }

    /// The bus's `NameOwnerChanged`: the print portal's name passed from
    /// `old` to `new`, each empty for none.
    fn portal_changed_hands(old: &str, new: &str) -> Vec<u8> {
        let mut b = Writer::default();
        b.string("org.freedesktop.portal.Desktop");
        b.string(old);
        b.string(new);
        let signal = marshal_signal(
            &Call {
                destination: "",
                path: "/org/freedesktop/DBus",
                interface: "org.freedesktop.DBus",
                member: "NameOwnerChanged",
                signature: "sss",
                body: b.buf,
            },
            Serial(Word(902)),
        );
        with_sender(&signal, "org.freedesktop.DBus")
    }

    /// An export that has its handle, `h`.
    fn exported(h: &str) -> baud_domain::Export {
        let mut e = baud_domain::Export::new();
        let _ = e.ask();
        let _ = e.handle(h);
        e
    }

    /// A session named `:1.42` whose print portal answered its `version`
    /// from `:1.9`, which holds the portal's name; what it sent, sent.
    fn with_portal() -> Session {
        let mut s = named();
        s.ask_print_portal();
        let serial = last_serial(&s);
        let mut b = Writer::default();
        b.signature("u");
        b.word(Word(4));
        let version = with_sender(&reply_to(serial, METHOD_RETURN, "v", b.buf, ""), ":1.9");
        assert_eq!(
            s.received(&version),
            [Heard::PrintPortal(Ok(PortalVersion(4)))]
        );
        assert_eq!(
            watches_portal(&s).len(),
            1,
            "the bus asked to watch the name"
        );
        let n = s.outgoing().len();
        s.sent(Len(n));
        s
    }

    /// Where, in what `s` would send, it asks the bus to watch the
    /// portal's name: each offset of the rule.
    fn watches_portal(s: &Session) -> Vec<Len> {
        let rule = PORTAL_HOLDER_RULE.as_bytes();
        s.outgoing()
            .windows(rule.len())
            .enumerate()
            .filter(|(_, w)| *w == rule)
            .map(|(at, _)| Len(at))
            .collect()
    }

    /// **A page setup past the text lays out on A4 with the default
    /// margins**: the portal's `Width` of -5, `Height` of NaN and
    /// `MarginLeft` of -1 (`[[source: xdg-portal-print]]`: millimetres)
    /// are no lengths, and the sheet is the one a setup that left them out
    /// gives.
    #[test]
    fn a_page_setup_past_the_text_lays_out_on_a4() {
        let mut s = with_portal();
        s.prepare_print(ParentWindow::Unnamed, "t");
        let bad = setup_response(
            PATH_1,
            Word(0),
            Some(Word(7)),
            &[("Width", -5.0), ("Height", f64::NAN), ("MarginLeft", -1.0)],
        );
        let h = s.received(&with_sender(&bad, ":1.9"));
        let [Heard::PrintReady(_, setup)] = h.as_slice() else {
            panic!("{h:?}");
        };
        assert_eq!(*setup, PageSetup::default());
        assert_eq!(
            crate::print::sheet(*setup),
            crate::print::sheet(PageSetup::default())
        );
    }

    /// **A `Response` is the portal's only from the connection that holds
    /// its name**: `:1.66`'s, on the predicted path, ends nothing; `:1.9`'s,
    /// the holder's, ends the exchange.
    #[test]
    fn a_response_from_another_connection_ends_nothing() {
        let mut s = with_portal();
        s.prepare_print(ParentWindow::Unnamed, "t");
        let h = s.received(&response_from(":1.66", PATH_1, Word(0), Some(Word(7))));
        assert!(
            !h.iter()
                .any(|h| matches!(h, Heard::PrintReady(..) | Heard::PrintEnded(_))),
            "{h:?}"
        );
        assert_eq!(s.refused(), RefusedResponses(1));
        let [Heard::PrintRefused(e)] = h.as_slice() else {
            panic!("{h:?}");
        };
        assert!(e.contains(":1.66"), "{e}");
        assert_eq!(
            s.received(&response_from(":1.9", PATH_1, Word(0), Some(Word(7)))),
            [Heard::PrintReady(PrintToken(Word(7)), a4())],
            "the holder's"
        );
        assert_eq!(s.refused(), RefusedResponses(1));
    }

    /// **After the name passes from `:1.9` to `:1.10`, the old holder's
    /// `Response` ends nothing**, a later reply from it does not make it
    /// the holder again, and the new holder's ends the exchange. The bus
    /// is asked once for the name's changes.
    #[test]
    fn after_the_portal_changes_hands_the_old_holder_s_response_ends_nothing() {
        let mut s = with_portal();
        assert_eq!(s.received(&portal_changed_hands(":1.9", ":1.10")), []);
        s.prepare_print(ParentWindow::Unnamed, "t");
        let serial = last_serial(&s);
        let mut path = Writer::default();
        path.string(PATH_1);
        let late = with_sender(&reply_to(serial, METHOD_RETURN, "o", path.buf, ""), ":1.9");
        assert_eq!(s.received(&late), []);
        assert_eq!(watches_portal(&s), [], "asked once");
        let h = s.received(&response_from(":1.9", PATH_1, Word(0), Some(Word(7))));
        assert!(
            !h.iter()
                .any(|h| matches!(h, Heard::PrintReady(..) | Heard::PrintEnded(_))),
            "the old holder: {h:?}"
        );
        assert_eq!(
            s.received(&response_from(":1.10", PATH_1, Word(0), Some(Word(7)))),
            [Heard::PrintReady(PrintToken(Word(7)), a4())],
            "the new holder"
        );
    }

    /// **A change of hands while the dialog is open ends the exchange**,
    /// with the change said, and the match removed: neither holder's
    /// `Response` is waited for after it. A name released ends it too.
    #[test]
    fn a_change_of_hands_while_the_dialog_is_open_ends_it() {
        for (new, said) in [(":1.10", ":1.10"), ("", "released")] {
            let mut s = with_portal();
            s.prepare_print(ParentWindow::Unnamed, "t");
            let n = s.outgoing().len();
            s.sent(Len(n));
            let h = s.received(&portal_changed_hands(":1.9", new));
            let [Heard::PrintEnded(PrintEnd::Failed(e))] = h.as_slice() else {
                panic!("{new:?}: {h:?}");
            };
            assert!(e.contains(said) && e.contains(":1.9"), "{e}");
            assert!(s.outgoing().windows(11).any(|w| w == b"RemoveMatch"));
            for sender in [":1.9", ":1.10"] {
                let h = s.received(&response_from(sender, PATH_1, Word(0), Some(Word(7))));
                assert!(
                    !h.iter()
                        .any(|h| matches!(h, Heard::PrintReady(..) | Heard::PrintEnded(_))),
                    "{new:?}, {sender}: {h:?}"
                );
            }
        }
    }

    /// **The `Response` match names the portal as its sender**, so the bus
    /// delivers that signal only from the name's holder; and before any
    /// reply has named the holder, no `Response` is the portal's.
    #[test]
    fn the_response_match_names_the_portal_as_sender() {
        let mut s = named();
        s.prepare_print(ParentWindow::Unnamed, "t");
        let rule = format!(
            "type='signal',sender='org.freedesktop.portal.Desktop',interface='org.freedesktop.portal.Request',member='Response',path='{PATH_1}'"
        );
        let out = s.outgoing();
        assert!(
            out.windows(rule.len()).any(|w| w == rule.as_bytes()),
            "{}",
            String::from_utf8_lossy(out)
        );
        let h = s.received(&response(PATH_1, Word(0), Some(Word(7))));
        assert!(
            matches!(h.as_slice(), [Heard::PrintRefused(_)]),
            "no reply has named the portal's holder: {h:?}"
        );
    }
}
