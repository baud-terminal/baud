//! A message's header and the messages baud sends (`[[source: dbus-spec]]`,
//! "Message Format", "Header Fields" and "Message Types").
//!
//! The fixed header is `l`, the message type, the flags, version 1, the
//! body's length and the serial; then the fields, an array of `(yv)` on
//! 8, each field's code fixing its value's type; then NUL to 8, then the
//! body. A message is drawn its serial from the connection's counter as it
//! is encoded, so no two messages a connection sends share one until the
//! counter has gone round.

#[cfg(feature = "alloc")]
use alloc::vec::Vec;
use core::num::NonZeroU32;
use core::ops::BitOr;

use super::body::MessageLen;
#[cfg(feature = "alloc")]
use super::body::{Body, BodyWriter, EncodeError};
use super::name::{BusName, ErrorName, InterfaceName, MemberName, ObjectPath};
use super::sig::Sig;
use super::value::{Align, FdCount};

/// The byte order mark of every message baud writes: little-endian.
const LITTLE: u8 = b'l';

/// The major protocol version this specification describes.
pub(super) const VERSION: u8 = 1;

/// A message's type, its second header byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageKind {
    /// 1: a method call, which may prompt a reply.
    MethodCall,
    /// 2: a method's reply, with what it returned.
    MethodReturn,
    /// 3: an error reply.
    Error,
    /// 4: a signal.
    Signal,
}

impl MessageKind {
    /// The header byte.
    pub const fn byte(self) -> u8 {
        match self {
            MessageKind::MethodCall => 1,
            MessageKind::MethodReturn => 2,
            MessageKind::Error => 3,
            MessageKind::Signal => 4,
        }
    }

    /// The type a header byte names, or `None` for 0 and the types this
    /// specification does not define, which a receiver ignores.
    pub const fn from_byte(b: u8) -> Option<MessageKind> {
        match b {
            1 => Some(MessageKind::MethodCall),
            2 => Some(MessageKind::MethodReturn),
            3 => Some(MessageKind::Error),
            4 => Some(MessageKind::Signal),
            _ => None,
        }
    }
}

/// A message's flags, its third header byte; a receiver ignores the bits
/// it does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Flags(u8);

impl Flags {
    /// No flags: what baud sends.
    pub const NONE: Flags = Flags(0);
    /// 0x1: no reply is wanted.
    pub const NO_REPLY_EXPECTED: Flags = Flags(0x1);
    /// 0x2: the bus does not start a service to take the destination name.
    pub const NO_AUTO_START: Flags = Flags(0x2);
    /// 0x4: the caller will wait for interactive authorization.
    pub const ALLOW_INTERACTIVE_AUTHORIZATION: Flags = Flags(0x4);

    /// The flags of a received header byte `b`, the bits this
    /// specification does not define ignored.
    pub(super) const fn received(b: u8) -> Flags {
        Flags(
            b & (Flags::NO_REPLY_EXPECTED.0
                | Flags::NO_AUTO_START.0
                | Flags::ALLOW_INTERACTIVE_AUTHORIZATION.0),
        )
    }

    /// The header byte.
    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether every flag of `other` is set here.
    #[inline]
    pub const fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for Flags {
    type Output = Flags;
    fn bitor(self, other: Flags) -> Flags {
        Flags(self.0 | other.0)
    }
}

/// A header field's code, the byte before its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldCode {
    /// 1, an OBJECT_PATH.
    Path,
    /// 2, a STRING holding an interface name.
    Interface,
    /// 3, a STRING holding a member name.
    Member,
    /// 4, a STRING holding an error name.
    ErrorName,
    /// 5, a UINT32 serial.
    ReplySerial,
    /// 6, a STRING holding a bus name.
    Destination,
    /// 7, a STRING holding a unique bus name.
    Sender,
    /// 8, a SIGNATURE.
    Signature,
    /// 9, a UINT32 count.
    UnixFds,
}

impl FieldCode {
    /// The code's byte.
    pub const fn byte(self) -> u8 {
        match self {
            FieldCode::Path => 1,
            FieldCode::Interface => 2,
            FieldCode::Member => 3,
            FieldCode::ErrorName => 4,
            FieldCode::ReplySerial => 5,
            FieldCode::Destination => 6,
            FieldCode::Sender => 7,
            FieldCode::Signature => 8,
            FieldCode::UnixFds => 9,
        }
    }

    /// The type of the value this code takes, one complete type.
    pub const fn value_sig(self) -> Sig<'static> {
        match self {
            FieldCode::Path => Sig::lit("o"),
            FieldCode::Interface
            | FieldCode::Member
            | FieldCode::ErrorName
            | FieldCode::Destination
            | FieldCode::Sender => Sig::lit("s"),
            FieldCode::ReplySerial | FieldCode::UnixFds => Sig::lit("u"),
            FieldCode::Signature => Sig::lit("g"),
        }
    }

    /// The field a byte names, or `None` for 0 and the codes this
    /// specification does not define, which a receiver skips.
    pub const fn from_byte(b: u8) -> Option<FieldCode> {
        match b {
            1 => Some(FieldCode::Path),
            2 => Some(FieldCode::Interface),
            3 => Some(FieldCode::Member),
            4 => Some(FieldCode::ErrorName),
            5 => Some(FieldCode::ReplySerial),
            6 => Some(FieldCode::Destination),
            7 => Some(FieldCode::Sender),
            8 => Some(FieldCode::Signature),
            9 => Some(FieldCode::UnixFds),
            _ => None,
        }
    }
}

/// A field's value as it is written: a string, a signature or a word.
#[derive(Clone, Copy)]
enum FieldValue<'a> {
    Text(&'a str),
    Sig(Sig<'a>),
    Word(u32),
}

/// One header field: its code, and a value of the one type that code
/// takes ("Header Fields"). The code and the value come from one `match`
/// and the signature written into the field's variant from the code
/// ([`FieldCode::value_sig`], which the decoder checks a received field
/// against), so a field cannot carry a value of another type.
///
/// A path field holds an object path, not a string:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::{HeaderField, Str};
/// let _ = HeaderField::Path(Str::lit("/a"));
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{FieldCode, HeaderField, ObjectPath, Sig};
/// let f = HeaderField::Path(ObjectPath::lit("/a"));
/// assert_eq!(f.code(), FieldCode::Path);
/// assert_eq!(f.sig(), Sig::lit("o"));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeaderField<'a> {
    /// The object a call goes to or a signal comes from.
    Path(ObjectPath<'a>),
    /// The interface of the member.
    Interface(InterfaceName<'a>),
    /// The method or signal.
    Member(MemberName<'a>),
    /// The error an error reply carries.
    ErrorName(ErrorName<'a>),
    /// The serial of the message a reply answers.
    ReplySerial(Serial),
    /// The connection the message is for.
    Destination(BusName<'a>),
    /// The connection that sent it, as the bus sets it.
    Sender(BusName<'a>),
    /// The body's signature.
    Signature(Sig<'a>),
    /// How many file descriptors accompany the message.
    UnixFds(FdCount),
}

impl<'a> HeaderField<'a> {
    /// The code and the value, from one `match`, and the value's
    /// signature from the code.
    const fn parts(&self) -> (FieldCode, Sig<'static>, FieldValue<'a>) {
        let (code, value) = match *self {
            HeaderField::Path(p) => (FieldCode::Path, FieldValue::Text(p.as_str())),
            HeaderField::Interface(n) => (FieldCode::Interface, FieldValue::Text(n.as_str())),
            HeaderField::Member(n) => (FieldCode::Member, FieldValue::Text(n.as_str())),
            HeaderField::ErrorName(n) => (FieldCode::ErrorName, FieldValue::Text(n.as_str())),
            HeaderField::ReplySerial(s) => (FieldCode::ReplySerial, FieldValue::Word(s.get())),
            HeaderField::Destination(n) => (FieldCode::Destination, FieldValue::Text(n.as_str())),
            HeaderField::Sender(n) => (FieldCode::Sender, FieldValue::Text(n.as_str())),
            HeaderField::Signature(s) => (FieldCode::Signature, FieldValue::Sig(s)),
            HeaderField::UnixFds(n) => (FieldCode::UnixFds, FieldValue::Word(n.get())),
        };
        (code, code.value_sig(), value)
    }

    /// The field's code.
    pub const fn code(&self) -> FieldCode {
        self.parts().0
    }

    /// The type its value has.
    pub const fn sig(&self) -> Sig<'static> {
        self.parts().1
    }

    /// The field as a `(yv)` struct.
    #[cfg(feature = "alloc")]
    fn write(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        let (code, sig, value) = self.parts();
        w.structure(|w| {
            w.byte(code.byte());
            w.variant(sig, |w| match value {
                FieldValue::Text(s) => w.string(s),
                FieldValue::Sig(s) => {
                    w.signature(s);
                    Ok(())
                }
                FieldValue::Word(v) => {
                    w.word(v);
                    Ok(())
                }
            })
        })
    }
}

/// A message's serial: never 0, and drawn only from a connection's
/// [`Serials`] as the message is encoded, so a caller cannot send two
/// messages under one serial or pick the serial of a reply it awaits.
/// `Hello` is 1 ("Message Format").
///
/// A serial is not made from a number:
///
/// ```compile_fail,E0423
/// use baud_domain::dbus::Serial;
/// let _ = Serial(5);
/// ```
///
/// and a message is not encoded under a serial of the caller's choosing:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::{Body, BusName, InterfaceName, MemberName, MethodCall, ObjectPath, Serial};
/// let call = MethodCall {
///     destination: BusName::lit("org.example.A"),
///     path: ObjectPath::lit("/a"),
///     interface: InterfaceName::lit("org.example.A"),
///     member: MemberName::lit("Ping"),
/// };
/// let body = Body::EMPTY;
/// let _ = call.encode(&body, Serial::HELLO);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Body, BusName, InterfaceName, MemberName, MethodCall, ObjectPath, Serials};
/// let call = MethodCall {
///     destination: BusName::lit("org.example.A"),
///     path: ObjectPath::lit("/a"),
///     interface: InterfaceName::lit("org.example.A"),
///     member: MemberName::lit("Ping"),
/// };
/// let mut serials = Serials::new();
/// let sent = call.encode(&Body::EMPTY, &mut serials).expect("small");
/// assert_eq!(sent.serial().get(), 2);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Serial(NonZeroU32);

impl Serial {
    /// `Hello`'s, the first message on a connection.
    pub const HELLO: Serial = Serial(NonZeroU32::MIN);

    /// The serial as its header word.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// The serial a received header word names, or `None` for 0.
    pub(super) const fn received(w: u32) -> Option<Serial> {
        match NonZeroU32::new(w) {
            Some(n) => Some(Serial(n)),
            None => None,
        }
    }
}

/// A connection's serial counter: 2 first, as [`HELLO`] is 1, then each
/// next, `u32::MAX` followed by 1, as 0 is never a serial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Serials {
    next: Serial,
}

impl Serials {
    /// The counter of a connection that has sent [`HELLO`].
    pub const fn new() -> Serials {
        Serials {
            next: Serial(NonZeroU32::MIN.saturating_add(1)),
        }
    }

    /// The next serial, drawn.
    #[expect(
        clippy::should_implement_trait,
        reason = "the counter never ends, so an iterator's None would be unreachable"
    )]
    pub fn next(&mut self) -> Serial {
        let drawn = self.next;
        self.next = Serial(drawn.0.checked_add(1).unwrap_or(NonZeroU32::MIN));
        drawn
    }
}

impl Default for Serials {
    fn default() -> Serials {
        Serials::new()
    }
}

/// The most header fields baud writes into one message.
const FIELDS: usize = 6;

/// A message's header fields in the order they are written: by code, with
/// UNIX_FDS last.
#[derive(Clone, Copy)]
struct Fields<'a>([Option<HeaderField<'a>>; FIELDS]);

impl<'a> Fields<'a> {
    /// `named`, then SIGNATURE unless `sig` is empty and UNIX_FDS unless
    /// `fds` is none: an absent SIGNATURE is the empty signature, an
    /// absent UNIX_FDS no descriptors.
    const fn of(named: [Option<HeaderField<'a>>; 4], sig: Sig<'a>, fds: FdCount) -> Fields<'a> {
        let [a, b, c, d] = named;
        Fields([
            a,
            b,
            c,
            d,
            if sig.is_empty() {
                None
            } else {
                Some(HeaderField::Signature(sig))
            },
            if fds.get() == 0 {
                None
            } else {
                Some(HeaderField::UnixFds(fds))
            },
        ])
    }
}

/// An outgoing method call: where it goes and what it calls. The body's
/// signature is not a field here, so the SIGNATURE the header carries is
/// the body's own ("Header Fields"), and every field is a checked name, so
/// no string reaches the header unchecked ("Valid Names").
///
/// A call has no signature to set:
///
/// ```compile_fail,E0560
/// use baud_domain::dbus::{BusName, InterfaceName, MemberName, MethodCall, ObjectPath, Sig};
/// const NOTIFY: MethodCall<'static> = MethodCall {
///     destination: BusName::lit("org.freedesktop.Notifications"),
///     path: ObjectPath::lit("/org/freedesktop/Notifications"),
///     interface: InterfaceName::lit("org.freedesktop.Notifications"),
///     member: MemberName::lit("Notify"),
/// };
/// let _ = MethodCall { signature: Sig::lit("s"), ..NOTIFY };
/// ```
///
/// and its member is a member name, not a string:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::{BusName, InterfaceName, MemberName, MethodCall, ObjectPath};
/// const NOTIFY: MethodCall<'static> = MethodCall {
///     destination: BusName::lit("org.freedesktop.Notifications"),
///     path: ObjectPath::lit("/org/freedesktop/Notifications"),
///     interface: InterfaceName::lit("org.freedesktop.Notifications"),
///     member: MemberName::lit("Notify"),
/// };
/// let _ = MethodCall { member: "Notify", ..NOTIFY };
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Body, BusName, InterfaceName, MemberName, MethodCall, ObjectPath, Serials, U32};
/// const NOTIFY: MethodCall<'static> = MethodCall {
///     destination: BusName::lit("org.freedesktop.Notifications"),
///     path: ObjectPath::lit("/org/freedesktop/Notifications"),
///     interface: InterfaceName::lit("org.freedesktop.Notifications"),
///     member: MemberName::lit("Notify"),
/// };
/// const CLOSE: MethodCall<'static> = MethodCall {
///     member: MemberName::lit("CloseNotification"),
///     ..NOTIFY
/// };
/// let body = Body::of(&(U32::new(42),)).expect("one word");
/// let sent = CLOSE.encode(&body, &mut Serials::new()).expect("small");
/// assert!(sent.bytes().ends_with(&[8, 1, b'g', 0, 1, b'u', 0, 0, 42, 0, 0, 0]));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MethodCall<'a> {
    /// The connection the call is for.
    pub destination: BusName<'a>,
    /// The object called.
    pub path: ObjectPath<'a>,
    /// The interface of the method.
    pub interface: InterfaceName<'a>,
    /// The method.
    pub member: MemberName<'a>,
}

impl<'a> MethodCall<'a> {
    /// The header fields of this call with a body of `sig` naming `fds`
    /// descriptors.
    const fn fields(&self, sig: Sig<'a>, fds: FdCount) -> Fields<'a> {
        Fields::of(
            [
                Some(HeaderField::Path(self.path)),
                Some(HeaderField::Interface(self.interface)),
                Some(HeaderField::Member(self.member)),
                Some(HeaderField::Destination(self.destination)),
            ],
            sig,
            fds,
        )
    }

    /// The call with `body`, its serial drawn from `serials` once the
    /// message is known to fit.
    #[cfg(feature = "alloc")]
    pub fn encode(&self, body: &Body, serials: &mut Serials) -> Result<Encoded, EncodeError> {
        message(
            MessageKind::MethodCall,
            self.fields(body.sig(), body.fds()),
            body,
            serials,
        )
    }
}

/// An outgoing signal, sent to every connection that matches it: the
/// object, interface and member it is emitted as, each required
/// ("Message Types"), so a signal without its interface does not compile:
///
/// ```compile_fail,E0063
/// use baud_domain::dbus::{Emit, MemberName, ObjectPath};
/// const P: ObjectPath<'static> = ObjectPath::lit("/org/example/A");
/// const M: MemberName<'static> = MemberName::lit("Changed");
/// let _ = Emit { path: P, member: M };
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Emit, InterfaceName, MemberName, ObjectPath};
/// const P: ObjectPath<'static> = ObjectPath::lit("/org/example/A");
/// const M: MemberName<'static> = MemberName::lit("Changed");
/// let e = Emit { path: P, interface: InterfaceName::lit("org.example.A"), member: M };
/// assert_eq!(e.member, M);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Emit<'a> {
    /// The object emitting it.
    pub path: ObjectPath<'a>,
    /// The interface of the signal.
    pub interface: InterfaceName<'a>,
    /// The signal.
    pub member: MemberName<'a>,
}

impl<'a> Emit<'a> {
    /// The header fields of this signal with a body of `sig` naming `fds`
    /// descriptors.
    #[cfg(feature = "alloc")]
    const fn fields(&self, sig: Sig<'a>, fds: FdCount) -> Fields<'a> {
        Fields::of(
            [
                Some(HeaderField::Path(self.path)),
                Some(HeaderField::Interface(self.interface)),
                Some(HeaderField::Member(self.member)),
                None,
            ],
            sig,
            fds,
        )
    }

    /// The signal with `body`, its serial drawn from `serials` once the
    /// message is known to fit.
    #[cfg(feature = "alloc")]
    pub fn encode(&self, body: &Body, serials: &mut Serials) -> Result<Encoded, EncodeError> {
        message(
            MessageKind::Signal,
            self.fields(body.sig(), body.fds()),
            body,
            serials,
        )
    }
}

/// One encoded message: its bytes, the serial it was sent under, and how
/// many file descriptors must go with it.
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    bytes: Vec<u8>,
    serial: Serial,
    fds: FdCount,
}

#[cfg(feature = "alloc")]
impl Encoded {
    /// The message's bytes.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The serial it carries, which its reply names.
    #[inline]
    pub fn serial(&self) -> Serial {
        self.serial
    }

    /// How many file descriptors accompany it.
    #[inline]
    pub fn fds(&self) -> FdCount {
        self.fds
    }

    /// The bytes, kept.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// A message of `kind` with `fields` and `body`, little-endian, no flags;
/// the serial is drawn last, so a refused message takes none.
#[cfg(feature = "alloc")]
fn message(
    kind: MessageKind,
    fields: Fields<'_>,
    body: &Body,
    serials: &mut Serials,
) -> Result<Encoded, EncodeError> {
    let Some(body_len) = MessageLen::new(body.bytes().len()) else {
        return Err(EncodeError::TooLong);
    };
    let mut bytes = Vec::new();
    let mut w = BodyWriter::new(&mut bytes);
    w.bytes(&[LITTLE, kind.byte(), Flags::NONE.bits(), VERSION]);
    w.word(body_len.get());
    w.word(0);
    w.array(Align::Eight, |w| {
        for f in fields.0.iter().flatten() {
            f.write(w)?;
        }
        Ok(())
    })?;
    w.align(Align::Eight);
    if MessageLen::new(bytes.len() + body.bytes().len()).is_none() {
        return Err(EncodeError::TooLong);
    }
    bytes.extend_from_slice(body.bytes());
    let serial = serials.next();
    bytes[8..12].copy_from_slice(&serial.get().to_le_bytes());
    Ok(Encoded {
        bytes,
        serial,
        fds: body.fds(),
    })
}

/// Where a [`Fixed`] writes next.
#[derive(Clone, Copy)]
struct At(usize);

/// The most bytes a message built in a `const` holds.
const FIXED: usize = 256;

/// A message with no body built in a `const`, for the bytes a connection
/// sends before it can encode anything.
struct Fixed {
    bytes: [u8; FIXED],
    at: At,
}

impl Fixed {
    const fn push(&mut self, b: u8) {
        self.bytes[self.at.0] = b;
        self.at.0 += 1;
    }

    const fn align(&mut self, to: Align) {
        while !self.at.0.is_multiple_of(to.bytes()) {
            self.push(0);
        }
    }

    const fn word(&mut self, v: u32) {
        self.align(Align::Four);
        let b = v.to_le_bytes();
        let mut i = 0;
        while i < 4 {
            self.push(b[i]);
            i += 1;
        }
    }

    const fn codes(&mut self, s: &[u8]) {
        let mut i = 0;
        while i < s.len() {
            self.push(s[i]);
            i += 1;
        }
        self.push(0);
    }

    const fn field(&mut self, f: &HeaderField<'_>) {
        let (code, sig, value) = f.parts();
        self.align(Align::Eight);
        self.push(code.byte());
        self.push(sig.len().get());
        self.codes(sig.as_bytes());
        match value {
            FieldValue::Text(s) => {
                let Some(len) = MessageLen::new(s.len()) else {
                    panic!("a header string is within the message bound");
                };
                self.word(len.get());
                self.codes(s.as_bytes());
            }
            FieldValue::Sig(s) => {
                self.push(s.len().get());
                self.codes(s.as_bytes());
            }
            FieldValue::Word(v) => self.word(v),
        }
    }

    /// `kind` as `serial` with `fields` and no body.
    const fn message(kind: MessageKind, serial: Serial, fields: Fields<'_>) -> Fixed {
        let mut m = Fixed {
            bytes: [0; FIXED],
            at: At(0),
        };
        m.push(LITTLE);
        m.push(kind.byte());
        m.push(Flags::NONE.bits());
        m.push(VERSION);
        m.word(0);
        m.word(serial.get());
        m.word(0);
        let start = m.at.0;
        let mut i = 0;
        while i < FIELDS {
            if let Some(f) = &fields.0[i] {
                m.field(f);
            }
            i += 1;
        }
        let Some(len) = MessageLen::new(m.at.0 - start) else {
            panic!("a header is within the message bound");
        };
        let len = len.get().to_le_bytes();
        let mut i = 0;
        while i < 4 {
            m.bytes[12 + i] = len[i];
            i += 1;
        }
        m.align(Align::Eight);
        m
    }

    /// The first `N` bytes.
    const fn head<const N: usize>(&self) -> [u8; N] {
        let mut out = [0; N];
        let mut i = 0;
        while i < N {
            out[i] = self.bytes[i];
            i += 1;
        }
        out
    }
}

/// `Hello` to the bus, the first call on a connection, which answers with
/// the connection's unique name ("Message Bus Interface:
/// org.freedesktop.DBus").
const HELLO_CALL: MethodCall<'static> = MethodCall {
    destination: BusName::lit("org.freedesktop.DBus"),
    path: ObjectPath::lit("/org/freedesktop/DBus"),
    interface: InterfaceName::lit("org.freedesktop.DBus"),
    member: MemberName::lit("Hello"),
};

const HELLO_MESSAGE: Fixed = Fixed::message(
    MessageKind::MethodCall,
    Serial::HELLO,
    HELLO_CALL.fields(Sig::EMPTY, FdCount::ZERO),
);

const HELLO_BYTES: [u8; HELLO_MESSAGE.at.0] = HELLO_MESSAGE.head();

/// `Hello`'s bytes as serial 1, built at compile time: the message a
/// connection sends straight after `BEGIN`, before it can encode any other
/// (`[[source: dbus-spec]]`, "Message Bus Interface: org.freedesktop.DBus").
pub const HELLO: &[u8] = &HELLO_BYTES;

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::{Body, FdCount, HELLO, HELLO_CALL, NonZeroU32, Serial, Serials};
    use crate::dbus::value::{FdIndex, UnixFd};

    #[test]
    fn serials_wrap_past_u32_max_to_one() {
        let mut s = Serials {
            next: Serial(NonZeroU32::new(u32::MAX - 1).expect("not 0")),
        };
        assert_eq!(s.next().get(), u32::MAX - 1);
        assert_eq!(s.next().get(), u32::MAX);
        assert_eq!(s.next().get(), 1, "0 is never a serial");
        assert_eq!(s.next().get(), 2);
    }

    #[test]
    fn the_const_hello_is_the_encoder_s_hello() {
        let mut s = Serials {
            next: Serial::HELLO,
        };
        let e = HELLO_CALL.encode(&Body::EMPTY, &mut s).expect("small");
        assert_eq!(e.bytes(), HELLO);
        assert_eq!(e.serial(), Serial::HELLO);
        assert_eq!(s, Serials::new(), "the counter goes on at 2");
    }

    #[test]
    fn descriptors_are_counted_and_unix_fds_is_written_last() {
        let body = Body::of(&(UnixFd::new(FdIndex::ZERO), UnixFd::new(FdIndex::new(2))))
            .expect("two words");
        assert_eq!(body.fds().get(), 3, "every index named is below the count");
        let e = HELLO_CALL
            .encode(&body, &mut Serials::new())
            .expect("small");
        assert_eq!(e.fds().get(), 3);
        let bytes = e.bytes();
        let header_end = bytes.len() - body.bytes().len();
        // ... SIGNATURE `hh`, then UNIX_FDS 3, then the body on 8.
        let fields_end = 16
            + usize::try_from(u32::from_le_bytes([
                bytes[12], bytes[13], bytes[14], bytes[15],
            ]))
            .expect("small");
        assert_eq!(
            &bytes[fields_end - 8..fields_end],
            &[9, 1, b'u', 0, 3, 0, 0, 0]
        );
        assert!(bytes[fields_end..header_end].iter().all(|&b| b == 0));
        assert_eq!(&bytes[header_end..], &[0, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(Body::EMPTY.fds(), FdCount::ZERO);
    }
}
