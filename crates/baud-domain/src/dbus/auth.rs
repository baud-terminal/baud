//! The handshake before the messages: the credentials byte, SASL `EXTERNAL`,
//! file passing asked for, `BEGIN` and `Hello`, as typestate, each state
//! with only its next step (`[[source: dbus-spec]]`, "Authentication
//! Protocol", "Special credentials-passing nul byte", "AUTH command", "OK
//! Command", "NEGOTIATE_UNIX_FD Command", "AGREE_UNIX_FD Command", "BEGIN
//! Command", "Future Extensions" and the client's "Authentication state
//! diagrams"). Then the connection it begins: its serials, whether files
//! pass on it, and its unique name ("Message Bus Names", "Method:
//! org.freedesktop.DBus.Hello").
//!
//! Lines in are the caller's: [`auth_line`] says where the next whole one
//! ends, and each state reads one. Bytes out are this module's, each step
//! handing back the line it sends.

use core::fmt;

use super::decode::{BodyError, Malformed, Return};
use super::header::{HELLO, Serials};
use super::name::{BusName, OwnedBusName, Str};
use super::value::{FdIndex, UnixFd};
use crate::ReadLen;

/// A Unix user ID: the identity `EXTERNAL` sends, as its decimal digits
/// ("Authentication Protocol": "an authorization identity consisting
/// entirely of ASCII decimal digits represents a numeric user ID").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Uid(u32);

impl Uid {
    /// The user `id`.
    #[inline]
    pub const fn new(id: u32) -> Uid {
        Uid(id)
    }

    /// The ID.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// `\0AUTH EXTERNAL `, then two hex digits for each of a `u32`'s ten
/// decimal digits, then `\r\n`.
const AUTH_MAX: usize = 15 + 2 * 10 + 2;

/// How many bytes an [`AuthLine`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AuthLen(u8);

/// The client's first bytes: the credentials byte, NUL, then `AUTH
/// EXTERNAL` with the identity, the uid's decimal digits each as two hex
/// digits, or with none, leaving the server to take the socket's.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AuthLine {
    bytes: [u8; AUTH_MAX],
    len: AuthLen,
}

impl AuthLine {
    const fn of(uid: Option<Uid>) -> AuthLine {
        const HEAD: &[u8] = b"\0AUTH EXTERNAL";
        let mut line = AuthLine {
            bytes: [0; AUTH_MAX],
            len: AuthLen(0),
        };
        let mut i = 0;
        while i < HEAD.len() {
            line.push(HEAD[i]);
            i += 1;
        }
        if let Some(Uid(id)) = uid {
            line.push(b' ');
            let mut digits = [0u8; 10];
            let mut n = 0;
            let mut rest = id;
            loop {
                digits[n] = (rest % 10) as u8;
                n += 1;
                rest /= 10;
                if rest == 0 {
                    break;
                }
            }
            while n > 0 {
                n -= 1;
                // An ASCII digit is 0x30 to 0x39: `3`, then the digit.
                line.push(b'3');
                line.push(b'0' + digits[n]);
            }
        }
        line.push(b'\r');
        line.push(b'\n');
        line
    }

    const fn push(&mut self, b: u8) {
        self.bytes[self.len.0 as usize] = b;
        self.len.0 += 1;
    }

    /// The bytes to send.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len.0)]
    }
}

impl fmt::Debug for AuthLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AuthLine({:?})", self.bytes().escape_ascii())
    }
}

/// One line the server sent, its `\r\n` removed, and how many received
/// bytes it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineIn<'a> {
    line: &'a [u8],
    consumed: ReadLen,
}

impl<'a> LineIn<'a> {
    /// The line, without its `\r\n`.
    #[inline]
    pub const fn bytes(&self) -> &'a [u8] {
        self.line
    }

    /// How many bytes of those given the line took, its `\r\n` included.
    #[inline]
    pub const fn consumed(&self) -> ReadLen {
        self.consumed
    }
}

/// The first whole line of `received`, or `None` until its `\r\n` has
/// arrived ("Protocol Overview": "each line ends with \r\n").
pub fn auth_line(received: &[u8]) -> Option<LineIn<'_>> {
    let end = received.windows(2).position(|w| w == b"\r\n")?;
    Some(LineIn {
        line: &received[..end],
        consumed: ReadLen::from_index(end + 2),
    })
}

/// Why the handshake ended without a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// `REJECTED`: the server did not take `EXTERNAL`, the one mechanism
    /// baud offers.
    Rejected,
    /// `ERROR` in answer to `AUTH`.
    Error,
    /// `DATA` after the identity was sent, a challenge `EXTERNAL` has no
    /// answer to.
    Challenge,
    /// A line that answers nothing asked: anything but `AGREE_UNIX_FD` or
    /// `ERROR` to `NEGOTIATE_UNIX_FD`.
    Unexpected,
    /// A line that is not the protocol's: a byte past ASCII, a NUL, or no
    /// command of `[A-Z_]` at its start.
    Line,
}

impl Refusal {
    /// What the server did, as a sentence.
    pub const fn rule(self) -> &'static str {
        match self {
            Refusal::Rejected => "the bus rejected EXTERNAL authentication",
            Refusal::Error => "the bus answered AUTH with ERROR",
            Refusal::Challenge => "the bus sent EXTERNAL a challenge after its response",
            Refusal::Unexpected => "the bus answered NEGOTIATE_UNIX_FD with another command",
            Refusal::Line => "the bus sent a line that is not an ASCII command",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rule())
    }
}

/// The commands a client reads ("Protocol Overview").
#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Ok,
    Data,
    Rejected,
    Error,
    AgreeUnixFd,
    Other,
}

impl Command {
    /// The command `line` begins with: ASCII without NUL, a name of
    /// `[A-Z_]`, then a space and its arguments or the end.
    fn of(line: &[u8]) -> Result<Command, Refusal> {
        if line.iter().any(|&b| b == 0 || !b.is_ascii()) {
            return Err(Refusal::Line);
        }
        let name = line.split(|&b| b == b' ').next().unwrap_or_default();
        if name.is_empty() || !name.iter().all(|&b| b.is_ascii_uppercase() || b == b'_') {
            return Err(Refusal::Line);
        }
        Ok(match name {
            b"OK" => Command::Ok,
            b"DATA" => Command::Data,
            b"REJECTED" => Command::Rejected,
            b"ERROR" => Command::Error,
            b"AGREE_UNIX_FD" => Command::AgreeUnixFd,
            _ => Command::Other,
        })
    }
}

/// What the client waits for after `AUTH`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    /// "WaitingForData": `AUTH EXTERNAL` went without an identity.
    Data,
    /// "WaitingForOK": the identity, or the empty response, was sent.
    Ok,
}

/// `AUTH` sent; waiting for `OK`, or for `DATA` when no identity was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthSent(Expect);

/// `OK` received: authenticated, files not yet asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Authed(());

/// `NEGOTIATE_UNIX_FD` sent; waiting for `AGREE_UNIX_FD` or `ERROR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Negotiating(());

/// A connection on which no file descriptor passes: not asked for, or
/// refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoFds(());

/// File passing agreed (`AGREE_UNIX_FD`): the one maker of [`UnixFd`], so
/// a body names a descriptor only on a connection that can carry it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdsAgreed(());

impl FdsAgreed {
    /// The descriptor at `index` of those a message carries.
    #[inline]
    pub const fn fd(self, index: FdIndex) -> UnixFd {
        UnixFd::new(index)
    }
}

/// The SASL exchange in state `S`, each state with only its next step:
/// NUL and `AUTH EXTERNAL`, `OK`, then file passing asked for and its
/// answer, or not, then `BEGIN` and `Hello`. baud offers `EXTERNAL` alone,
/// so where the client's state diagram would `CANCEL` and try the next
/// mechanism the list is exhausted, and the handshake ends with a
/// [`Refusal`] for the caller to close the connection on.
///
/// `BEGIN` is not sent before `OK`:
///
/// ```compile_fail,E0599
/// use baud_domain::dbus::{AuthSent, Handshake};
/// fn f(h: Handshake<AuthSent>) { h.begin(); }
/// ```
///
/// and files are not asked for before it either:
///
/// ```compile_fail,E0599
/// use baud_domain::dbus::{AuthSent, Handshake};
/// fn f(h: Handshake<AuthSent>) { h.negotiate_fds(); }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{AuthStep, Handshake, Uid, auth_line};
/// let (h, first) = Handshake::start(Some(Uid::new(1000)));
/// assert_eq!(first.bytes(), b"\0AUTH EXTERNAL 31303030\r\n");
/// let line = auth_line(b"OK 1234deadbeef\r\n").expect("a whole line");
/// let AuthStep::Ok(h) = h.on_line(line) else { panic!("authenticated") };
/// let begun = h.begin();
/// assert!(begun.bytes().starts_with(b"BEGIN\r\n"));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handshake<S> {
    state: S,
}

/// What a line read after `AUTH` leads to.
#[must_use = "the handshake goes on in the state returned"]
#[derive(Debug, PartialEq, Eq)]
pub enum AuthStep {
    /// `DATA` to an `AUTH EXTERNAL` without an identity: answered with the
    /// empty response, these bytes, and then `OK` is waited for.
    Data(Handshake<AuthSent>, &'static [u8]),
    /// A command this does not know: answered with these bytes, `ERROR`,
    /// and waited past ("Future Extensions").
    Unknown(Handshake<AuthSent>, &'static [u8]),
    /// `OK`: authenticated.
    Ok(Handshake<Authed>),
    /// The handshake ended.
    Refused(Refusal),
}

/// What the line read after `NEGOTIATE_UNIX_FD` leads to: `BEGIN` either
/// way, or the end.
#[must_use = "the connection begins in the state returned"]
#[derive(Debug, PartialEq, Eq)]
pub enum FdAnswer {
    /// `AGREE_UNIX_FD`.
    Agreed(Begun<FdsAgreed>),
    /// `ERROR`: the connection begins without file passing.
    Refused(Begun<NoFds>),
    /// Anything else: the handshake ended.
    Unexpected(Refusal),
}

impl Handshake<AuthSent> {
    /// The handshake's first bytes, NUL and `AUTH EXTERNAL` with `uid` as
    /// the identity, and the state waiting for their answer.
    pub const fn start(uid: Option<Uid>) -> (Handshake<AuthSent>, AuthLine) {
        let expect = match uid {
            Some(_) => Expect::Ok,
            None => Expect::Data,
        };
        (
            Handshake {
                state: AuthSent(expect),
            },
            AuthLine::of(uid),
        )
    }

    /// The server's answer to `AUTH`, or to the `DATA` sent.
    pub fn on_line(self, line: LineIn<'_>) -> AuthStep {
        let command = match Command::of(line.line) {
            Ok(c) => c,
            Err(r) => return AuthStep::Refused(r),
        };
        match (self.state.0, command) {
            (_, Command::Ok) => AuthStep::Ok(Handshake { state: Authed(()) }),
            (_, Command::Rejected) => AuthStep::Refused(Refusal::Rejected),
            (_, Command::Error) => AuthStep::Refused(Refusal::Error),
            (Expect::Data, Command::Data) => AuthStep::Data(
                Handshake {
                    state: AuthSent(Expect::Ok),
                },
                b"DATA\r\n",
            ),
            (Expect::Ok, Command::Data) => AuthStep::Refused(Refusal::Challenge),
            (_, Command::AgreeUnixFd | Command::Other) => AuthStep::Unknown(self, b"ERROR\r\n"),
        }
    }
}

/// `BEGIN`, then `Hello` as serial 1.
const BEGUN: [u8; 7 + HELLO.len()] = {
    const BEGIN: &[u8; 7] = b"BEGIN\r\n";
    let mut out = [0; 7 + HELLO.len()];
    let mut i = 0;
    while i < BEGIN.len() {
        out[i] = BEGIN[i];
        i += 1;
    }
    while i < out.len() {
        out[i] = HELLO[i - BEGIN.len()];
        i += 1;
    }
    out
};

impl Handshake<Authed> {
    /// `BEGIN` without asking for file passing.
    pub const fn begin(self) -> Begun<NoFds> {
        Begun { files: NoFds(()) }
    }

    /// Asks for file passing: these bytes, `NEGOTIATE_UNIX_FD`, and the
    /// state waiting for the answer.
    pub const fn negotiate_fds(self) -> (Handshake<Negotiating>, &'static [u8]) {
        (
            Handshake {
                state: Negotiating(()),
            },
            b"NEGOTIATE_UNIX_FD\r\n",
        )
    }
}

impl Handshake<Negotiating> {
    /// The server's answer to `NEGOTIATE_UNIX_FD`.
    pub fn on_line(self, line: LineIn<'_>) -> FdAnswer {
        match Command::of(line.line) {
            Ok(Command::AgreeUnixFd) => FdAnswer::Agreed(Begun {
                files: FdsAgreed(()),
            }),
            Ok(Command::Error) => FdAnswer::Refused(Begun { files: NoFds(()) }),
            Ok(_) => FdAnswer::Unexpected(Refusal::Unexpected),
            Err(r) => FdAnswer::Unexpected(r),
        }
    }
}

/// The handshake over: `BEGIN` and `Hello` to send, then the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Begun<F> {
    files: F,
}

impl<F> Begun<F> {
    /// `BEGIN\r\n` and then `Hello` as serial 1, built at compile time;
    /// what the caller sends next draws its serial from the connection.
    #[inline]
    pub const fn bytes(&self) -> &'static [u8] {
        &BEGUN
    }

    /// The connection, its next serial 2.
    pub fn into_connection(self) -> Connection<F> {
        Connection {
            files: self.files,
            serials: Serials::new(),
            unique: None,
        }
    }
}

/// A connection past `BEGIN`: its serials, whether files pass on it (`F`,
/// [`FdsAgreed`] or [`NoFds`]) and, once `Hello` has answered, its unique
/// name.
///
/// Descriptors are named only on a connection that agreed to pass them:
///
/// ```compile_fail,E0599
/// use baud_domain::dbus::{Connection, NoFds};
/// fn f(c: &Connection<NoFds>) { c.fds(); }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Connection, FdIndex, FdsAgreed, UnixFd};
/// fn f(c: &Connection<FdsAgreed>) -> UnixFd { c.fds().fd(FdIndex::ZERO) }
/// ```
///
/// The unique name is `Hello`'s reply, never a string's:
///
/// ```compile_fail,E0599
/// use baud_domain::dbus::BusName;
/// let _ = BusName::unique_from(":1.4");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{AuthStep, BusName, Handshake, Inbound, Uid, auth_line, decode};
/// let (h, _) = Handshake::start(Some(Uid::new(1000)));
/// let ok = auth_line(b"OK 1234deadbeef\r\n").expect("a whole line");
/// let AuthStep::Ok(h) = h.on_line(ok) else { panic!("authenticated") };
/// let mut c = h.begin().into_connection();
/// // Hello's reply: REPLY_SERIAL 1, SIGNATURE `s`, the body ":1.4".
/// let mut m = vec![b'l', 2, 0, 1, 9, 0, 0, 0, 2, 0, 0, 0, 15, 0, 0, 0];
/// m.extend_from_slice(&[5, 1, b'u', 0, 1, 0, 0, 0, 8, 1, b'g', 0, 1, b's', 0, 0]);
/// m.extend_from_slice(&[4, 0, 0, 0, b':', b'1', b'.', b'4', 0]);
/// let Ok(Inbound::Return(hello)) = decode(&m) else { panic!("a reply") };
/// c.on_hello(&hello).expect("a unique name");
/// assert_eq!(c.unique(), Some(BusName::lit(":1.4")));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connection<F> {
    files: F,
    serials: Serials,
    unique: Option<OwnedBusName>,
}

impl<F> Connection<F> {
    /// The serials every message after `Hello` draws from.
    #[inline]
    pub const fn serials(&mut self) -> &mut Serials {
        &mut self.serials
    }

    /// Keeps the unique name `Hello`'s reply carries: one STRING, a bus
    /// name, beginning with `:`. A reply that is not is refused and
    /// nothing is kept.
    pub fn on_hello(&mut self, reply: &Return<'_>) -> Result<(), Malformed> {
        let (name,) = reply
            .body()
            .read::<(Str<'static>,)>()
            .map_err(|e| match e {
                BodyError::Signature => Malformed::Reply,
                BodyError::Malformed(m) => m,
            })?;
        let name = BusName::parse(name.as_str()).map_err(Malformed::Name)?;
        self.unique = Some(OwnedBusName::unique(name).ok_or(Malformed::NotUnique)?);
        Ok(())
    }

    /// This connection's unique name, once `Hello` has answered.
    #[inline]
    pub fn unique(&self) -> Option<BusName<'_>> {
        self.unique.as_ref().map(OwnedBusName::as_name)
    }
}

impl Connection<FdsAgreed> {
    /// The maker of the descriptors a message on this connection names.
    #[inline]
    pub const fn fds(&self) -> FdsAgreed {
        self.files
    }
}
