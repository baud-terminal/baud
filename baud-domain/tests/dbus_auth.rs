//! The handshake, the connection it begins and whom a signal is trusted
//! from, against `[[source: dbus-spec]]`: "Special credentials-passing nul
//! byte", "AUTH command", "OK Command", "NEGOTIATE_UNIX_FD Command",
//! "AGREE_UNIX_FD Command", "BEGIN Command", "Future Extensions", the
//! client's "Authentication state diagrams", "Message Bus Names", "Method:
//! org.freedesktop.DBus.Hello" and "Header Fields" (SENDER).
//!
//! The handshake's bytes are the ones `baud-app/src/bus.rs`'s test
//! `the_handshake_sends_begin_and_hello_before_any_call` checks, and
//! `HELLO_CALL` is the hex of its `HELLO` const. The two senderless
//! messages are copied from `baud-app/fixtures/bus-replies.hex`, the lines
//! named after each const; the messages with a SENDER are built here, in
//! the same byte order, with the field as "Header Fields" lays it out.

use baud_domain::ReadLen;
use baud_domain::dbus::{
    AuthStep, BusName, FdAnswer, FdIndex, Handshake, Inbound, Malformed, NameError, Peer, Refusal,
    Return, Serial, Signal, Uid, auth_line, decode,
};

/// `HELLO`: `Hello()` to the bus as serial 1.
const HELLO_CALL: &str = "6c01000100000000010000006d00000001016f00150000002f6f72672f667265656465736b746f702f4442757300000002017300140000006f72672e667265656465736b746f702e4442757300000000030173000500000048656c6c6f00000006017300140000006f72672e667265656465736b746f702e4442757300000000";

/// `notify_reply`: `Notify`'s reply, id 42, to serial 12, with no SENDER.
const NOTIFY_REPLY: &str =
    "6c020001040000000c0000000f000000050175000600000008016700017500002a000000";

/// `action_invoked`: `ActionInvoked(42, "1")` with no SENDER.
const ACTION_INVOKED: &str = "6c0400010a0000000d0000007000000001016f001e0000002f6f72672f667265656465736b746f702f4e6f74696669636174696f6e730000020173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e73000000030173000d000000416374696f6e496e766f6b656400000008016700027573002a000000010000003100";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// One received line, whole.
fn line(bytes: &[u8]) -> baud_domain::dbus::LineIn<'_> {
    let l = auth_line(bytes).expect("a whole line");
    assert_eq!(l.consumed(), ReadLen::of(bytes));
    l
}

/// `BEGIN` and then `Hello` as serial 1.
fn begin_and_hello() -> Vec<u8> {
    let mut v = b"BEGIN\r\n".to_vec();
    v.extend_from_slice(&hex(HELLO_CALL));
    v
}

/// A header field's value, as "Header Fields" types it.
enum Value<'a> {
    Path(&'a str),
    Str(&'a str),
    U32(u32),
}

/// A little-endian message of `kind` and `serial` with `fields`, then
/// SIGNATURE `sig` when it is not empty, then `body`.
fn message(kind: u8, serial: u32, fields: &[(u8, Value<'_>)], sig: &str, body: &[u8]) -> Vec<u8> {
    let mut m = vec![b'l', kind, 0, 1];
    m.extend_from_slice(&u32::try_from(body.len()).expect("small").to_le_bytes());
    m.extend_from_slice(&serial.to_le_bytes());
    m.extend_from_slice(&[0; 4]);
    let pad = |m: &mut Vec<u8>, to: usize| {
        while !m.len().is_multiple_of(to) {
            m.push(0);
        }
    };
    let string = |m: &mut Vec<u8>, s: &str| {
        m.extend_from_slice(&u32::try_from(s.len()).expect("small").to_le_bytes());
        m.extend_from_slice(s.as_bytes());
        m.push(0);
    };
    for (code, value) in fields {
        pad(&mut m, 8);
        m.push(*code);
        match value {
            Value::Path(s) => {
                m.extend_from_slice(&[1, b'o', 0]);
                string(&mut m, s);
            }
            Value::Str(s) => {
                m.extend_from_slice(&[1, b's', 0]);
                string(&mut m, s);
            }
            Value::U32(v) => {
                m.extend_from_slice(&[1, b'u', 0]);
                m.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    if !sig.is_empty() {
        pad(&mut m, 8);
        m.extend_from_slice(&[8, 1, b'g', 0, u8::try_from(sig.len()).expect("short")]);
        m.extend_from_slice(sig.as_bytes());
        m.push(0);
    }
    let fields_len = u32::try_from(m.len() - 16).expect("small");
    m[12..16].copy_from_slice(&fields_len.to_le_bytes());
    pad(&mut m, 8);
    m.extend_from_slice(body);
    m
}

/// A STRING body.
fn string_body(s: &str) -> Vec<u8> {
    let mut b = u32::try_from(s.len())
        .expect("small")
        .to_le_bytes()
        .to_vec();
    b.extend_from_slice(s.as_bytes());
    b.push(0);
    b
}

/// A reply to `reply_to` from `sender`, if any, whose body is the STRING
/// `s`.
fn string_reply(reply_to: u32, sender: Option<&str>, s: &str) -> Vec<u8> {
    let mut fields = vec![(5, Value::U32(reply_to))];
    if let Some(sender) = sender {
        fields.push((7, Value::Str(sender)));
    }
    message(2, 9, &fields, "s", &string_body(s))
}

/// `ActionInvoked(42, "1")` from `sender`.
fn action_invoked(sender: &str) -> Vec<u8> {
    let mut body = 42u32.to_le_bytes().to_vec();
    body.extend_from_slice(&string_body("1"));
    message(
        4,
        13,
        &[
            (1, Value::Path("/org/freedesktop/Notifications")),
            (2, Value::Str("org.freedesktop.Notifications")),
            (3, Value::Str("ActionInvoked")),
            (7, Value::Str(sender)),
        ],
        "us",
        &body,
    )
}

fn reply(bytes: &[u8]) -> Return<'_> {
    match decode(bytes) {
        Ok(Inbound::Return(r)) => r,
        other => panic!("a reply: {other:?}"),
    }
}

fn signal(bytes: &[u8]) -> Signal<'_> {
    match decode(bytes) {
        Ok(Inbound::Signal(s)) => s,
        other => panic!("a signal: {other:?}"),
    }
}

/// **NUL, `AUTH EXTERNAL` with the uid's decimal digits in hex, `OK`, file
/// passing asked and agreed, then `BEGIN` and `Hello` as serial 1**: the
/// bytes of Figure 7, and the app's handshake test, and the next serial 2.
#[test]
fn the_handshake_sends_the_apps_bytes_and_hello_is_serial_one() {
    let (h, first) = Handshake::start(Some(Uid::new(1000)));
    assert_eq!(first.bytes(), b"\0AUTH EXTERNAL 31303030\r\n");
    let AuthStep::Ok(h) = h.on_line(line(b"OK 1234deadbeef\r\n")) else {
        panic!("OK authenticates");
    };
    let (h, ask) = h.negotiate_fds();
    assert_eq!(ask, b"NEGOTIATE_UNIX_FD\r\n");
    let FdAnswer::Agreed(begun) = h.on_line(line(b"AGREE_UNIX_FD\r\n")) else {
        panic!("AGREE_UNIX_FD agrees");
    };
    assert_eq!(begun.bytes(), begin_and_hello().as_slice());
    let mut c = begun.into_connection();
    assert_eq!(c.serials().next().get(), 2, "Hello was 1");
    assert_eq!(c.serials().next().get(), 3);
    let fd = c.fds().fd(FdIndex::new(2));
    assert_eq!(fd.index(), FdIndex::new(2));
}

/// **Without an identity**: `AUTH EXTERNAL` alone, the server's `DATA`
/// answered with the empty `DATA` (the client is then waiting for `OK`),
/// and `BEGIN` straight after `OK`, without file passing (Figure 1).
#[test]
fn the_empty_identity_answers_data_with_empty_data() {
    let (h, first) = Handshake::start(None);
    assert_eq!(first.bytes(), b"\0AUTH EXTERNAL\r\n");
    let AuthStep::Data(h, answer) = h.on_line(line(b"DATA\r\n")) else {
        panic!("DATA is answered");
    };
    assert_eq!(answer, b"DATA\r\n");
    // The empty response sent, the client waits for OK: a second
    // challenge is not answered again.
    let AuthStep::Ok(h) = h.on_line(line(b"OK 1234deadbeef\r\n")) else {
        panic!("OK authenticates");
    };
    let begun = h.begin();
    assert_eq!(begun.bytes(), begin_and_hello().as_slice());
    let mut c = begun.into_connection();
    assert_ne!(c.serials().next(), Serial::HELLO, "Hello was 1");
}

/// The uid's digits, every one of them: 0, and `u32::MAX`'s ten.
#[test]
fn the_identity_is_the_uid_s_decimal_digits_in_hex() {
    let (_, zero) = Handshake::start(Some(Uid::new(0)));
    assert_eq!(zero.bytes(), b"\0AUTH EXTERNAL 30\r\n");
    let (_, max) = Handshake::start(Some(Uid::new(u32::MAX)));
    assert_eq!(
        max.bytes(),
        b"\0AUTH EXTERNAL 34323934393637323935\r\n",
        "4294967295"
    );
}

/// **File passing refused is still a connection**: `ERROR` to
/// `NEGOTIATE_UNIX_FD` begins without it (Figure 8); anything else ends
/// the handshake ("WaitingForAgreeUnixFD").
#[test]
fn an_error_to_the_ask_begins_without_files() {
    let authed = || {
        let (h, _) = Handshake::start(Some(Uid::new(1000)));
        let AuthStep::Ok(h) = h.on_line(line(b"OK 1234deadbeef\r\n")) else {
            panic!("OK authenticates");
        };
        h.negotiate_fds().0
    };
    let FdAnswer::Refused(begun) = authed().on_line(line(b"ERROR Not supported on this OS\r\n"))
    else {
        panic!("ERROR refuses file passing");
    };
    assert_eq!(begun.bytes(), begin_and_hello().as_slice());
    let FdAnswer::Unexpected(why) = authed().on_line(line(b"OK 1234deadbeef\r\n")) else {
        panic!("OK is not an answer to the ask");
    };
    assert_eq!(why, Refusal::Unexpected);
}

/// **A refusal ends the handshake, said**: `REJECTED` (no other mechanism
/// to try), `ERROR` to the `AUTH`, a second challenge after the response
/// was sent, and a line that is not the protocol's (a NUL, a byte past
/// ASCII, a command in lower case).
#[test]
fn refusals_end_the_handshake() {
    let refusal = |uid: Option<Uid>, l: &[u8]| {
        let (h, _) = Handshake::start(uid);
        match h.on_line(line(l)) {
            AuthStep::Refused(r) => r,
            _ => panic!("{l:?} is refused"),
        }
    };
    let me = Some(Uid::new(1000));
    assert_eq!(
        refusal(me, b"REJECTED KERBEROS_V4 SKEY\r\n"),
        Refusal::Rejected
    );
    assert_eq!(refusal(None, b"REJECTED\r\n"), Refusal::Rejected);
    assert_eq!(refusal(me, b"ERROR\r\n"), Refusal::Error);
    assert_eq!(refusal(me, b"DATA 8799cabb2ea93e\r\n"), Refusal::Challenge);
    assert_eq!(refusal(me, b"OK\0\r\n"), Refusal::Line);
    assert_eq!(refusal(me, b"OK \xff\r\n"), Refusal::Line);
    assert_eq!(refusal(me, b"ok 12\r\n"), Refusal::Line);
    assert_eq!(refusal(me, b"\r\n"), Refusal::Line);
    assert!(!Refusal::Rejected.rule().is_empty());
}

/// **An unknown command is answered with `ERROR` and is not fatal**
/// ("Future Extensions"; "Receive anything else → send ERROR"): the
/// handshake waits on, and `OK` still authenticates.
#[test]
fn an_unknown_command_is_answered_with_error() {
    let (h, _) = Handshake::start(Some(Uid::new(1000)));
    let AuthStep::Unknown(h, answer) = h.on_line(line(b"EXTENSION_COM_EXAMPLE_HI there\r\n"))
    else {
        panic!("an unknown command is answered");
    };
    assert_eq!(answer, b"ERROR\r\n");
    assert!(matches!(
        h.on_line(line(b"OK 1234deadbeef\r\n")),
        AuthStep::Ok(_)
    ));
}

/// **A line is whole only with its `\r\n`**, and says how many bytes it
/// took, so two lines in one read are read one after the other.
#[test]
fn a_line_is_read_to_its_crlf() {
    assert!(auth_line(b"OK 12").is_none());
    assert!(auth_line(b"OK 12\r").is_none());
    assert!(auth_line(b"").is_none());
    let both = b"OK 12\r\nAGREE_UNIX_FD\r\n";
    let first = auth_line(both).expect("a line");
    assert_eq!(first.consumed(), ReadLen::from_index(7));
    let (h, _) = Handshake::start(Some(Uid::new(1000)));
    let AuthStep::Ok(h) = h.on_line(first) else {
        panic!("OK authenticates");
    };
    let rest = &both[7..];
    let second = auth_line(rest).expect("a line");
    assert_eq!(second.consumed(), ReadLen::of(rest));
    assert!(matches!(
        h.negotiate_fds().0.on_line(second),
        FdAnswer::Agreed(_)
    ));
}

/// **The unique name is `Hello`'s reply, and only a unique name**: kept
/// past the reply's bytes; a well-known name, a name off the grammar or a
/// reply of another signature is refused and nothing is kept.
#[test]
fn on_hello_keeps_only_a_unique_name() {
    let (h, _) = Handshake::start(Some(Uid::new(1000)));
    let AuthStep::Ok(h) = h.on_line(line(b"OK 1234deadbeef\r\n")) else {
        panic!("OK authenticates");
    };
    let mut c = h.begin().into_connection();
    assert_eq!(c.unique(), None);

    let well_known = string_reply(1, Some("org.freedesktop.DBus"), "org.example.Name");
    assert_eq!(c.on_hello(&reply(&well_known)), Err(Malformed::NotUnique));
    assert_eq!(c.unique(), None);

    let off_grammar = string_reply(1, Some("org.freedesktop.DBus"), ":1..4");
    assert_eq!(
        c.on_hello(&reply(&off_grammar)),
        Err(Malformed::Name(NameError::EmptyElement))
    );
    assert_eq!(c.unique(), None);

    let number = hex(NOTIFY_REPLY);
    assert_eq!(c.on_hello(&reply(&number)), Err(Malformed::Reply));
    assert_eq!(c.unique(), None);

    let named = {
        let bytes = string_reply(1, Some("org.freedesktop.DBus"), ":1.42");
        c.on_hello(&reply(&bytes)).expect("a unique name");
        bytes
    };
    drop(named);
    assert_eq!(c.unique(), Some(BusName::lit(":1.42")));
}

/// **A signal is trusted only from the unique name that answered for the
/// well-known one** (decision 1, "Header Fields": SENDER is set by the
/// bus): the senderless oracle messages are the forged cases.
#[test]
fn a_signal_is_vouched_for_only_from_the_peer() {
    assert!(
        Peer::from_reply(&reply(&hex(NOTIFY_REPLY))).is_none(),
        "a reply without SENDER names no peer"
    );
    let from_name = string_reply(12, Some("org.freedesktop.Notifications"), "x");
    assert!(
        Peer::from_reply(&reply(&from_name)).is_none(),
        "a well-known SENDER is not a connection"
    );
    let answered = string_reply(12, Some(":1.7"), "x");
    let peer = Peer::from_reply(&reply(&answered)).expect("a unique SENDER");
    drop(answered);
    assert_eq!(peer.name(), BusName::lit(":1.7"));

    let ours = action_invoked(":1.7");
    let trusted = peer.vouch(&signal(&ours)).expect("from the peer");
    assert_eq!(trusted.signal().member.as_str(), "ActionInvoked");
    assert_eq!(trusted.signal(), signal(&ours));

    let forged = action_invoked(":1.8");
    assert!(peer.vouch(&signal(&forged)).is_none(), "another connection");
    let senderless = hex(ACTION_INVOKED);
    assert!(peer.vouch(&signal(&senderless)).is_none(), "no SENDER");
}
