//! The decoder against `[[source: dbus-spec]]`: "Message Format" (the
//! byte order, version 1, a non-zero serial, at most 2^27 bytes, NUL to
//! 8), "Header Fields" (each code's type, the required fields, unknown
//! codes skipped), "Byte order and alignment" (padding is NUL),
//! "Marshalling basic types" (strings hold no NUL and end in one),
//! "Marshalling containers" (arrays at most 2^26 bytes, variants at most 64
//! deep), the "Summary of types" (a BOOLEAN is 0 or 1) and "Invalid
//! Protocol and Spec Extensions" (unknown message types and flags are
//! ignored).
//!
//! The oracle messages are copied from `baud-app/fixtures/bus-replies.hex`,
//! the lines named after each const, an independent marshaller's output, which
//! writes only `l`. The big-endian cases are built here from the spec's
//! own examples and are marked so.

use baud_domain::dbus::{Align, Emit, NameError, SigError};
use baud_domain::dbus::{
    Array, Body, BodyError, Boolean, BusName, Byte, FieldCode, Flags, Frame, Header, Inbound,
    InterfaceName, Malformed, MemberName, MessageKind, MessageLen, MethodCall, ObjectPath, Order,
    Serials, Sig, Str, U32, decode, frame,
};

/// `notify_reply`: `Notify`'s reply to serial 6, the id 42.
const NOTIFY_REPLY: &str =
    "6c020001040000000c0000000f000000050175000600000008016700017500002a000000";

/// `notify_error`: `ServiceUnknown` in reply to serial 6, "no server".
const NOTIFY_ERROR: &str = "6c0300010e0000000c0000004700000004017300290000006f72672e667265656465736b746f702e444275732e4572726f722e53657276696365556e6b6e6f776e0000000000000005017500060000000801670001730000090000006e6f2073657276657200";

/// `action_invoked`: `ActionInvoked(42, "1")`.
const ACTION_INVOKED: &str = "6c0400010a0000000d0000007000000001016f001e0000002f6f72672f667265656465736b746f702f4e6f74696669636174696f6e730000020173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e73000000030173000d000000416374696f6e496e766f6b656400000008016700027573002a000000010000003100";

/// `notification_closed`: `NotificationClosed(42, 2)`.
const NOTIFICATION_CLOSED: &str = "6c040001080000000e0000007800000001016f001e0000002f6f72672f667265656465736b746f702f4e6f74696669636174696f6e730000020173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e7300000003017300120000004e6f74696669636174696f6e436c6f73656400000000000008016700027575002a00000002000000";

/// `capabilities_reply`: `GetCapabilities`' reply to serial 5, four names.
const CAPABILITIES_REPLY: &str = "6c0200013c0000000b00000010000000050175000500000008016700026173003800000007000000616374696f6e730004000000626f6479000000000b000000626f64792d6d61726b7570000b00000070657273697374656e636500";

/// `capabilities_bare`: `GetCapabilities`' reply naming only `body`.
const CAPABILITIES_BARE: &str =
    "6c0200010d0000000b00000010000000050175000500000008016700026173000900000004000000626f647900";

/// `capabilities_error`: `ServiceUnknown` in reply to serial 5.
const CAPABILITIES_ERROR: &str = "6c03000152000000070000004700000004017300290000006f72672e667265656465736b746f702e444275732e4572726f722e53657276696365556e6b6e6f776e00000000000000050175000500000008016700017300004d000000546865206e616d65206f72672e667265656465736b746f702e4e6f74696669636174696f6e7320776173206e6f742070726f766964656420627920616e79202e736572766963652066696c657300";

/// `show_items_ok`: an empty reply to serial 6, for `:1.42`, so no
/// SIGNATURE field.
const SHOW_ITEMS_OK: &str =
    "6c020001000000000300000016000000050175000600000006017300050000003a312e3432000000";

/// `name_acquired`: the bus's `NameAcquired(":1.42")` to `:1.42`.
const NAME_ACQUIRED: &str = "6c0400010a000000020000006f00000001016f00150000002f6f72672f667265656465736b746f702f4442757300000002017300140000006f72672e667265656465736b746f702e4442757300000000030173000c0000004e616d6541637175697265640000000006017300050000003a312e34320000000801670001730000050000003a312e343200";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// The byte order a test message is written in.
#[derive(Clone, Copy)]
enum Endian {
    L,
    B,
}

/// A message written by hand, value by value, for the cases no encoder
/// writes: the header as "Message Format" lays it out, the fields as
/// "Header Fields" does, alignment counted from the first byte.
struct W {
    b: Vec<u8>,
    e: Endian,
}

impl W {
    fn pad(&mut self, to: Align) {
        while !self.b.len().is_multiple_of(to.bytes()) {
            self.b.push(0);
        }
    }
    fn byte(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.pad(Align::Four);
        match self.e {
            Endian::L => self.b.extend_from_slice(&v.to_le_bytes()),
            Endian::B => self.b.extend_from_slice(&v.to_be_bytes()),
        }
    }
    fn str(&mut self, s: &[u8]) {
        self.u32(u32::try_from(s.len()).expect("short"));
        self.b.extend_from_slice(s);
        self.b.push(0);
    }
    fn sig(&mut self, s: &[u8]) {
        self.b.push(u8::try_from(s.len()).expect("short"));
        self.b.extend_from_slice(s);
        self.b.push(0);
    }
    /// One `(yv)` header field: on 8, the code, the value's signature,
    /// then `value`.
    fn field(&mut self, code: u8, sig: &[u8], value: impl FnOnce(&mut W)) {
        self.pad(Align::Eight);
        self.byte(code);
        self.sig(sig);
        value(self);
    }
    fn put(&mut self, at: Length, v: u32) {
        let b = match self.e {
            Endian::L => v.to_le_bytes(),
            Endian::B => v.to_be_bytes(),
        };
        self.b[at.span()].copy_from_slice(&b);
    }
}

/// The fixed header's two lengths, each a `UINT32` at its place in "Message
/// Format": the body's after the first four bytes, the field array's
/// after the serial.
#[derive(Clone, Copy)]
enum Length {
    Body,
    Fields,
}

impl Length {
    fn span(self) -> core::ops::Range<usize> {
        match self {
            Length::Body => 4..8,
            Length::Fields => 12..16,
        }
    }
}

/// The fixed header's fields a test varies, each as "Message Format" types
/// it: the type, flags and version a `BYTE`, the serial a `UINT32`. A test
/// gives them values no encoder writes.
#[derive(Clone, Copy)]
struct Fixed {
    e: Endian,
    kind: Byte,
    flags: Byte,
    version: Byte,
    serial: U32,
}

const SIGNAL: Fixed = Fixed {
    e: Endian::L,
    kind: Byte::new(4),
    flags: Byte::new(0),
    version: Byte::new(1),
    serial: U32::new(9),
};

const RETURN: Fixed = Fixed {
    kind: Byte::new(2),
    ..SIGNAL
};

/// A message: `head`, the fields `fields` writes, NUL to 8, the body
/// `body` writes, and both lengths filled in.
fn message(head: Fixed, fields: impl FnOnce(&mut W), body: impl FnOnce(&mut W)) -> Vec<u8> {
    let mut w = W {
        b: Vec::new(),
        e: head.e,
    };
    w.byte(match head.e {
        Endian::L => b'l',
        Endian::B => b'B',
    });
    w.byte(head.kind.get());
    w.byte(head.flags.get());
    w.byte(head.version.get());
    w.u32(0);
    w.u32(head.serial.get());
    w.u32(0);
    fields(&mut w);
    let flen = u32::try_from(w.b.len() - 16).expect("short");
    w.put(Length::Fields, flen);
    w.pad(Align::Eight);
    let start = w.b.len();
    body(&mut w);
    let blen = u32::try_from(w.b.len() - start).expect("short");
    w.put(Length::Body, blen);
    w.b
}

/// A signal's three required fields, the path, interface and member.
fn signal_fields(w: &mut W) {
    w.field(1, b"o", |w| w.str(b"/org/example/A"));
    w.field(2, b"s", |w| w.str(b"org.example.A"));
    w.field(3, b"s", |w| w.str(b"Changed"));
}

/// A reply to serial 6 whose body has signature `sig` and is `body`.
fn reply_with(sig: &[u8], body: impl FnOnce(&mut W)) -> Vec<u8> {
    message(
        RETURN,
        |w| {
            w.field(5, b"u", |w| w.u32(6));
            w.field(8, b"g", |w| w.sig(sig));
        },
        body,
    )
}

fn refused(bytes: &[u8]) -> Malformed {
    match decode(bytes) {
        Ok(m) => panic!("decoded {m:?}"),
        Err(e) => e,
    }
}

fn reply(bytes: &[u8]) -> baud_domain::dbus::Return<'_> {
    match decode(bytes) {
        Ok(Inbound::Return(r)) => r,
        other => panic!("not a reply: {other:?}"),
    }
}

fn signal(bytes: &[u8]) -> baud_domain::dbus::Signal<'_> {
    match decode(bytes) {
        Ok(Inbound::Signal(s)) => s,
        other => panic!("not a signal: {other:?}"),
    }
}

// The oracle's messages.

#[test]
fn notify_s_reply_is_its_id() {
    let bytes = hex(NOTIFY_REPLY);
    let r = reply(&bytes);
    assert_eq!(r.reply_to.get(), 6);
    assert_eq!(r.header().serial().get(), 12);
    assert_eq!(r.header().order(), Order::Little);
    assert_eq!(r.header().kind(), Some(MessageKind::MethodReturn));
    assert_eq!(r.header().flags(), Flags::NONE);
    assert_eq!(r.header().sender(), None);
    assert_eq!(r.body().sig(), Sig::lit("u"));
    let (id,) = r.body().read::<(U32,)>().expect("a u");
    assert_eq!(id, U32::new(42));
}

#[test]
fn a_body_is_read_only_as_its_own_signature() {
    let bytes = hex(NOTIFY_REPLY);
    let r = reply(&bytes);
    assert_eq!(
        r.body().read::<(Str,)>().map(|_| ()),
        Err(BodyError::Signature)
    );
    assert_eq!(
        r.body().read::<(U32, U32)>().map(|_| ()),
        Err(BodyError::Signature)
    );
    assert_eq!(r.body().read::<()>().map(|_| ()), Err(BodyError::Signature));
}

#[test]
fn notify_s_error_is_its_name_and_message() {
    let bytes = hex(NOTIFY_ERROR);
    let Ok(Inbound::ErrorReply(e)) = decode(&bytes) else {
        panic!("not an error reply");
    };
    assert_eq!(e.reply_to.get(), 6);
    assert_eq!(
        e.name,
        baud_domain::dbus::ErrorName::lit("org.freedesktop.DBus.Error.ServiceUnknown")
    );
    assert_eq!(e.message().map(|m| m.as_str()), Some("no server"));
    let (m,) = e.body().read::<(Str,)>().expect("an s");
    assert_eq!(m.as_str(), "no server");

    let bytes = hex(CAPABILITIES_ERROR);
    let Ok(Inbound::ErrorReply(e)) = decode(&bytes) else {
        panic!("not an error reply");
    };
    assert_eq!(e.reply_to.get(), 5);
    assert_eq!(
        e.message().map(|m| m.as_str()),
        Some("The name org.freedesktop.Notifications was not provided by any .service files")
    );
}

#[test]
fn a_click_is_its_id_and_key() {
    let bytes = hex(ACTION_INVOKED);
    let s = signal(&bytes);
    assert_eq!(s.path, ObjectPath::lit("/org/freedesktop/Notifications"));
    assert_eq!(
        s.interface,
        InterfaceName::lit("org.freedesktop.Notifications")
    );
    assert_eq!(s.member, MemberName::lit("ActionInvoked"));
    assert_eq!(s.header().serial().get(), 13);
    let (id, key) = s.body().read::<(U32, Str)>().expect("us");
    assert_eq!((id.get(), key.as_str()), (42, "1"));
}

#[test]
fn a_close_is_its_id_and_reason() {
    let bytes = hex(NOTIFICATION_CLOSED);
    let s = signal(&bytes);
    assert_eq!(s.member, MemberName::lit("NotificationClosed"));
    let (id, reason) = s.body().read::<(U32, U32)>().expect("uu");
    assert_eq!((id.get(), reason.get()), (42, 2));
}

#[test]
fn capabilities_are_an_iterator_of_strings() {
    let bytes = hex(CAPABILITIES_REPLY);
    let r = reply(&bytes);
    assert_eq!(r.reply_to.get(), 5);
    let (caps,) = r.body().read::<(Array<Str>,)>().expect("as");
    let caps: Vec<&str> = caps.map(|s| s.as_str()).collect();
    assert_eq!(caps, ["actions", "body", "body-markup", "persistence"]);

    let bytes = hex(CAPABILITIES_BARE);
    let (caps,) = reply(&bytes).body().read::<(Array<Str>,)>().expect("as");
    assert_eq!(caps.map(|s| s.as_str()).collect::<Vec<_>>(), ["body"]);
}

#[test]
fn an_empty_reply_has_the_empty_signature() {
    let bytes = hex(SHOW_ITEMS_OK);
    let r = reply(&bytes);
    assert_eq!(r.body().sig(), Sig::EMPTY);
    assert_eq!(r.header().signature(), Sig::EMPTY);
    assert_eq!(r.header().destination(), Some(BusName::lit(":1.42")));
    r.body().read::<()>().expect("nothing to read");
}

#[test]
fn the_bus_s_own_signal_names_its_destination() {
    let bytes = hex(NAME_ACQUIRED);
    let s = signal(&bytes);
    assert_eq!(s.member, MemberName::lit("NameAcquired"));
    assert_eq!(s.header().destination(), Some(BusName::lit(":1.42")));
    let (name,) = s.body().read::<(Str,)>().expect("s");
    assert_eq!(name.as_str(), ":1.42");
}

#[test]
fn every_oracle_message_frames_as_its_own_length() {
    for m in [
        NOTIFY_REPLY,
        NOTIFY_ERROR,
        ACTION_INVOKED,
        NOTIFICATION_CLOSED,
        CAPABILITIES_REPLY,
        CAPABILITIES_BARE,
        CAPABILITIES_ERROR,
        SHOW_ITEMS_OK,
        NAME_ACQUIRED,
    ] {
        let bytes = hex(m);
        let len = MessageLen::new(bytes.len()).expect("small");
        assert_eq!(frame(&bytes), Frame::Whole(len));
        let mut two = bytes.clone();
        two.extend_from_slice(&bytes);
        assert_eq!(frame(&two), Frame::Whole(len), "the first of two");
        for cut in [0, 1, 15, 16, bytes.len() - 1] {
            assert_eq!(frame(&bytes[..cut]), Frame::NeedMore, "{cut} bytes");
        }
        assert_eq!(refused(&bytes[..bytes.len() - 1]), Malformed::Length);
        assert_eq!(refused(&two), Malformed::Length);
    }
}

#[test]
fn what_baud_encodes_decodes_to_what_it_said() {
    const CLOSE: MethodCall<'static> = MethodCall {
        destination: BusName::lit("org.freedesktop.Notifications"),
        path: ObjectPath::lit("/org/freedesktop/Notifications"),
        interface: InterfaceName::lit("org.freedesktop.Notifications"),
        member: MemberName::lit("CloseNotification"),
    };
    let body = Body::of(&(U32::new(42),)).expect("small");
    let sent = CLOSE.encode(&body, &mut Serials::new()).expect("small");
    let Ok(Inbound::Called(c)) = decode(sent.bytes()) else {
        panic!("not a call");
    };
    assert_eq!(c.path, CLOSE.path);
    assert_eq!(c.member, CLOSE.member);
    assert_eq!(c.header().interface(), Some(CLOSE.interface));
    assert_eq!(c.header().destination(), Some(CLOSE.destination));
    assert_eq!(c.header().serial(), sent.serial());
    assert_eq!(c.body().read::<(U32,)>().expect("u").0.get(), 42);

    let emit = Emit {
        path: ObjectPath::lit("/org/example/A"),
        interface: InterfaceName::lit("org.example.A"),
        member: MemberName::lit("Changed"),
    };
    let names = [Str::lit("a"), Str::lit("bc")];
    let body = Body::of(&(Array::new(&names), Byte::new(7), Boolean::True)).expect("small");
    let sent = emit.encode(&body, &mut Serials::new()).expect("small");
    let s = signal(sent.bytes());
    assert_eq!(
        (s.path, s.interface, s.member),
        (emit.path, emit.interface, emit.member)
    );
    let (names, byte, yes) = s
        .body()
        .read::<(Array<Str>, Byte, Boolean)>()
        .expect("asyb");
    assert_eq!(names.map(|s| s.as_str()).collect::<Vec<_>>(), ["a", "bc"]);
    assert_eq!((byte.get(), yes), (7, Boolean::True));
}

// The refusals.

#[test]
fn decode_refuses_version_two() {
    let m = message(
        Fixed {
            version: Byte::new(2),
            ..SIGNAL
        },
        signal_fields,
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::Version);
    assert_eq!(frame(&m), Frame::Malformed(Malformed::Version));
    let ok = message(SIGNAL, signal_fields, |_| {});
    assert!(
        matches!(decode(&ok), Ok(Inbound::Signal(_))),
        "version 1 decodes"
    );
}

#[test]
fn decode_refuses_a_byte_order_other_than_l_or_b() {
    let mut m = message(SIGNAL, signal_fields, |_| {});
    m[0] = b'L';
    assert_eq!(refused(&m), Malformed::Endian);
    assert_eq!(frame(&m[..1]), Frame::Malformed(Malformed::Endian));
}

#[test]
fn decode_refuses_interface_as_u() {
    let m = message(
        SIGNAL,
        |w| {
            w.field(1, b"o", |w| w.str(b"/org/example/A"));
            w.field(2, b"u", |w| w.u32(7));
            w.field(3, b"s", |w| w.str(b"Changed"));
        },
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::FieldType(FieldCode::Interface));
}

#[test]
fn decode_refuses_a_signal_without_member() {
    let m = message(
        SIGNAL,
        |w| {
            w.field(1, b"o", |w| w.str(b"/org/example/A"));
            w.field(2, b"s", |w| w.str(b"org.example.A"));
        },
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::Missing(FieldCode::Member));
    let m = message(
        SIGNAL,
        |w| {
            w.field(1, b"o", |w| w.str(b"/org/example/A"));
            w.field(3, b"s", |w| w.str(b"Changed"));
        },
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::Missing(FieldCode::Interface));
}

#[test]
fn each_message_type_needs_its_own_fields() {
    let call = Fixed {
        kind: Byte::new(1),
        ..SIGNAL
    };
    let m = message(call, |w| w.field(3, b"s", |w| w.str(b"Ping")), |_| {});
    assert_eq!(refused(&m), Malformed::Missing(FieldCode::Path));
    let m = message(call, |w| w.field(1, b"o", |w| w.str(b"/a")), |_| {});
    assert_eq!(refused(&m), Malformed::Missing(FieldCode::Member));
    let m = message(
        call,
        |w| {
            w.field(1, b"o", |w| w.str(b"/a"));
            w.field(3, b"s", |w| w.str(b"Ping"));
        },
        |_| {},
    );
    let Ok(Inbound::Called(c)) = decode(&m) else {
        panic!("a call needs no INTERFACE");
    };
    assert_eq!(c.header().interface(), None);

    let m = message(RETURN, |_| {}, |_| {});
    assert_eq!(refused(&m), Malformed::Missing(FieldCode::ReplySerial));
    let error = Fixed {
        kind: Byte::new(3),
        ..SIGNAL
    };
    let m = message(error, |w| w.field(5, b"u", |w| w.u32(6)), |_| {});
    assert_eq!(refused(&m), Malformed::Missing(FieldCode::ErrorName));
    let m = message(
        error,
        |w| w.field(4, b"s", |w| w.str(b"org.example.Error.Failed")),
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::Missing(FieldCode::ReplySerial));
}

#[test]
fn decode_refuses_non_nul_padding() {
    // The header's padding to 8: the fields end 2 bytes short of 8.
    let mut m = message(
        RETURN,
        |w| {
            w.field(5, b"u", |w| w.u32(6));
            w.field(6, b"s", |w| w.str(b":1.42"));
        },
        |_| {},
    );
    assert_eq!(m.len(), 40, "the fields end at 38");
    assert!(matches!(decode(&m), Ok(Inbound::Return(_))));
    m[39] = 1;
    assert_eq!(refused(&m), Malformed::Padding);
    // The body's padding before a word.
    let mut m = reply_with(b"yu", |w| {
        w.byte(1);
        w.u32(2);
    });
    assert!(matches!(decode(&m), Ok(Inbound::Return(_))));
    let at = m.len() - 6;
    m[at] = 0xff;
    assert_eq!(refused(&m), Malformed::Padding);
}

#[test]
fn decode_refuses_nul_in_a_string() {
    let m = reply_with(b"s", |w| w.str(b"a\0b"));
    assert_eq!(refused(&m), Malformed::Nul);
    let m = reply_with(b"s", |w| w.str(b"ab"));
    assert_eq!(
        reply(&m).body().read::<(Str,)>().expect("s").0.as_str(),
        "ab"
    );
}

#[test]
fn decode_refuses_a_string_not_ended_by_nul() {
    let mut m = reply_with(b"s", |w| w.str(b"ab"));
    let last = m.len() - 1;
    m[last] = b'c';
    assert_eq!(refused(&m), Malformed::Terminator);
}

#[test]
fn decode_refuses_a_string_that_is_not_utf8() {
    let m = reply_with(b"s", |w| w.str(&[0xc3, 0x28]));
    assert_eq!(refused(&m), Malformed::Utf8);
}

#[test]
fn decode_refuses_b_two() {
    let m = reply_with(b"b", |w| w.u32(2));
    assert_eq!(refused(&m), Malformed::Boolean);
    let m = reply_with(b"b", |w| w.u32(1));
    assert_eq!(
        reply(&m).body().read::<(Boolean,)>().expect("b").0,
        Boolean::True
    );
}

#[test]
fn decode_refuses_reply_serial_zero() {
    let m = message(RETURN, |w| w.field(5, b"u", |w| w.u32(0)), |_| {});
    assert_eq!(refused(&m), Malformed::ZeroSerial);
    let m = message(
        Fixed {
            serial: U32::new(0),
            ..SIGNAL
        },
        signal_fields,
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::ZeroSerial);
}

#[test]
fn frame_refuses_a_length_of_two_to_the_27_plus_one() {
    // The fixed header alone says how long the message is: 16 bytes, no
    // fields, and a body that brings the whole to the bound and one past.
    let at_bound = (1u32 << 27) - 16;
    let mut head = vec![b'l', 4, 0, 1];
    head.extend_from_slice(&at_bound.to_le_bytes());
    head.extend_from_slice(&1u32.to_le_bytes());
    head.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(frame(&head), Frame::NeedMore, "2^27 bytes is the bound");
    head[4..8].copy_from_slice(&(at_bound + 1).to_le_bytes());
    assert_eq!(frame(&head), Frame::Malformed(Malformed::TooLong));
    assert_eq!(refused(&head), Malformed::TooLong);
    head[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(frame(&head), Frame::Malformed(Malformed::TooLong));
}

#[test]
fn frame_refuses_a_field_array_past_two_to_the_26() {
    let mut head = vec![b'l', 4, 0, 1];
    head.extend_from_slice(&0u32.to_le_bytes());
    head.extend_from_slice(&1u32.to_le_bytes());
    head.extend_from_slice(&((1u32 << 26) + 1).to_le_bytes());
    assert_eq!(frame(&head), Frame::Malformed(Malformed::ArrayTooLong));
    head[12..16].copy_from_slice(&(1u32 << 26).to_le_bytes());
    assert_eq!(frame(&head), Frame::NeedMore);
}

#[test]
fn decode_refuses_a_body_array_past_two_to_the_26() {
    let m = reply_with(b"ay", |w| w.u32((1 << 26) + 1));
    assert_eq!(refused(&m), Malformed::ArrayTooLong);
}

#[test]
fn decode_refuses_field_code_zero() {
    let m = message(
        SIGNAL,
        |w| {
            signal_fields(w);
            w.field(0, b"u", |w| w.u32(1));
        },
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::InvalidField);
}

#[test]
fn decode_refuses_a_field_given_twice() {
    let m = message(
        SIGNAL,
        |w| {
            signal_fields(w);
            w.field(7, b"s", |w| w.str(b":1.7"));
            w.field(7, b"s", |w| w.str(b":1.8"));
        },
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::Repeated(FieldCode::Sender));
}

#[test]
fn decode_refuses_a_name_that_breaks_its_grammar() {
    let m = message(
        SIGNAL,
        |w| {
            w.field(1, b"o", |w| w.str(b"/a//b"));
            w.field(2, b"s", |w| w.str(b"org.example.A"));
            w.field(3, b"s", |w| w.str(b"Changed"));
        },
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::Name(NameError::EmptyElement));
    let m = reply_with(b"o", |w| w.str(b"a"));
    assert_eq!(refused(&m), Malformed::Name(NameError::NotAbsolute));
    let m = reply_with(b"g", |w| w.sig(b"()"));
    assert_eq!(refused(&m), Malformed::Sig(SigError::EmptyStruct));
    let m = message(RETURN, |w| w.field(8, b"g", |w| w.sig(b"(i")), |_| {});
    assert_eq!(refused(&m), Malformed::Sig(SigError::StructUnclosed));
}

#[test]
fn unknown_codes_types_and_flags_are_passed_over() {
    let m = message(
        Fixed {
            flags: Byte::new(0x80 | 0x1),
            ..SIGNAL
        },
        |w| {
            signal_fields(w);
            w.field(200, b"(si)", |w| {
                w.pad(Align::Eight);
                w.str(b"later");
                w.u32(3);
            });
            // A known field in a message that does not use it.
            w.field(5, b"u", |w| w.u32(6));
        },
        |_| {},
    );
    let s = signal(&m);
    assert_eq!(s.header().flags(), Flags::NO_REPLY_EXPECTED);
    assert_eq!(s.header().reply_serial().map(|s| s.get()), Some(6));

    let m = message(
        Fixed {
            kind: Byte::new(9),
            ..SIGNAL
        },
        |_| {},
        |_| {},
    );
    let Ok(Inbound::Unknown(h)) = decode(&m) else {
        panic!("an unknown type is passed over");
    };
    assert_eq!((h.kind(), h.serial().get()), (None, 9));

    let m = message(
        Fixed {
            kind: Byte::new(0),
            ..SIGNAL
        },
        signal_fields,
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::Kind);
}

#[test]
fn an_unknown_field_must_still_be_well_formed() {
    let m = message(
        SIGNAL,
        |w| {
            signal_fields(w);
            w.field(200, b"b", |w| w.u32(2));
        },
        |_| {},
    );
    assert_eq!(refused(&m), Malformed::Boolean);
    let m = message(
        Fixed {
            kind: Byte::new(9),
            ..SIGNAL
        },
        |_| {},
        |w| w.u32(1),
    );
    assert_eq!(
        refused(&m),
        Malformed::Trailing,
        "a body with no signature is empty"
    );
}

#[test]
fn a_body_is_exactly_its_signature() {
    let m = reply_with(b"u", |w| {
        w.u32(1);
        w.byte(0);
    });
    assert_eq!(refused(&m), Malformed::Trailing);
    let m = reply_with(b"uu", |w| w.u32(1));
    assert_eq!(refused(&m), Malformed::Overrun);
    // An array whose length ends inside an element.
    let m = reply_with(b"au", |w| {
        w.u32(6);
        w.u32(1);
        w.u32(2);
    });
    assert_eq!(refused(&m), Malformed::Overrun);
}

#[test]
fn a_variant_holds_one_complete_type() {
    let m = reply_with(b"v", |w| {
        w.sig(b"ii");
        w.u32(1);
        w.u32(2);
    });
    assert_eq!(refused(&m), Malformed::NotSingle);
    let m = reply_with(b"v", |w| {
        w.sig(b"i");
        w.u32(1);
    });
    assert!(matches!(decode(&m), Ok(Inbound::Return(_))));
}

#[test]
fn variants_nest_a_body_at_most_64_deep() {
    let nested = |n: usize| {
        reply_with(b"v", move |w| {
            for _ in 1..n {
                w.sig(b"v");
            }
            w.sig(b"u");
            w.u32(5);
        })
    };
    assert!(matches!(decode(&nested(64)), Ok(Inbound::Return(_))));
    assert_eq!(refused(&nested(65)), Malformed::TooDeep);
}

#[test]
fn a_descriptor_index_is_below_unix_fds() {
    let with_fds = |n: u32, index: u32| {
        message(
            RETURN,
            |w| {
                w.field(5, b"u", |w| w.u32(6));
                w.field(8, b"g", |w| w.sig(b"h"));
                w.field(9, b"u", |w| w.u32(n));
            },
            move |w| w.u32(index),
        )
    };
    let m = with_fds(1, 0);
    let r = reply(&m);
    assert_eq!(r.header().fds().get(), 1);
    assert_eq!(refused(&with_fds(1, 1)), Malformed::FdIndex);
    assert_eq!(refused(&with_fds(0, 0)), Malformed::FdIndex);
}

#[test]
fn the_spec_s_three_strings_read_back() {
    // "Marshalling basic types": 'foo', '+' and 'bar' from an offset that
    // is a multiple of 8, little-endian, two bytes of padding before 'bar'.
    let spec: [u8; 22] = [
        0x03, 0, 0, 0, b'f', b'o', b'o', 0, 0x01, 0, 0, 0, b'+', 0, 0, 0, 0x03, 0, 0, 0, b'b', b'a',
    ];
    let m = reply_with(b"sss", |w| {
        w.b.extend_from_slice(&spec);
        w.b.extend_from_slice(b"r\0");
    });
    let (a, b, c) = reply(&m).body().read::<(Str, Str, Str)>().expect("sss");
    assert_eq!([a.as_str(), b.as_str(), c.as_str()], ["foo", "+", "bar"]);
}

// Big-endian, built from the spec's own examples: the oracle writes only
// `l`.

const BIG: Fixed = Fixed {
    e: Endian::B,
    ..RETURN
};

/// A big-endian reply to serial 6 whose body is `sig` and `body`.
fn big_reply(sig: &[u8], body: &[u8]) -> Vec<u8> {
    message(
        BIG,
        |w| {
            w.field(5, b"u", |w| w.u32(6));
            w.field(8, b"g", |w| w.sig(sig));
        },
        |w| w.b.extend_from_slice(body),
    )
}

#[test]
fn big_endian_the_spec_s_array_of_one_int64() {
    // "Marshalling containers": big-endian, at a multiple of 8, an array
    // holding only the 64-bit integer 5.
    let spec = [0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5];
    let m = big_reply(b"at", &spec);
    assert_eq!(m[0], b'B');
    let r = reply(&m);
    assert_eq!(r.header().order(), Order::Big);
    assert_eq!(r.reply_to.get(), 6);
    assert_eq!(r.body().sig(), Sig::lit("at"));
    assert_eq!(r.body().bytes(), &spec);
    let mut bad = spec;
    bad[5] = 1;
    assert_eq!(refused(&big_reply(b"at", &bad)), Malformed::Padding);
}

#[test]
fn big_endian_the_spec_s_variant_of_one_int64() {
    // "Marshalling containers": a variant holding the 64-bit integer 5.
    let spec = [1, b't', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5];
    let r_bytes = big_reply(b"v", &spec);
    let r = reply(&r_bytes);
    assert_eq!(r.body().sig(), Sig::lit("v"));
    let mut bad = spec;
    bad[4] = 7;
    assert_eq!(refused(&big_reply(b"v", &bad)), Malformed::Padding);
}

#[test]
fn big_endian_a_close_reads_as_the_little_endian_one_does() {
    // The header laid out as "Message Format" gives it, in `B`.
    let m = message(
        Fixed {
            e: Endian::B,
            ..SIGNAL
        },
        |w| {
            w.field(1, b"o", |w| w.str(b"/org/freedesktop/Notifications"));
            w.field(2, b"s", |w| w.str(b"org.freedesktop.Notifications"));
            w.field(3, b"s", |w| w.str(b"NotificationClosed"));
            w.field(8, b"g", |w| w.sig(b"uu"));
        },
        |w| {
            w.u32(42);
            w.u32(2);
        },
    );
    assert_eq!(&m[4..8], &[0, 0, 0, 8], "the body's length, big-endian");
    let s = signal(&m);
    assert_eq!(s.header().order(), Order::Big);
    assert_eq!(s.header().serial().get(), 9);
    let (id, reason) = s.body().read::<(U32, U32)>().expect("uu");
    assert_eq!((id.get(), reason.get()), (42, 2));
    let little = hex(NOTIFICATION_CLOSED);
    let (lid, lreason) = signal(&little).body().read::<(U32, U32)>().expect("uu");
    assert_eq!((id, reason), (lid, lreason));
}

#[test]
fn big_endian_framing_reads_the_lengths_in_b() {
    let m = big_reply(b"u", &[0, 0, 0, 42]);
    assert_eq!(
        frame(&m),
        Frame::Whole(MessageLen::new(m.len()).expect("small"))
    );
    assert_eq!(reply(&m).body().read::<(U32,)>().expect("u").0.get(), 42);
}

#[test]
fn the_header_reads_back_every_field() {
    let m = message(
        Fixed {
            kind: Byte::new(1),
            ..SIGNAL
        },
        |w| {
            w.field(1, b"o", |w| w.str(b"/a"));
            w.field(2, b"s", |w| w.str(b"org.example.A"));
            w.field(3, b"s", |w| w.str(b"Ping"));
            w.field(6, b"s", |w| w.str(b"org.example.B"));
            w.field(7, b"s", |w| w.str(b":1.7"));
        },
        |_| {},
    );
    let Ok(Inbound::Called(c)) = decode(&m) else {
        panic!("a call");
    };
    let h: &Header<'_> = c.header();
    assert_eq!(h.path(), Some(ObjectPath::lit("/a")));
    assert_eq!(h.member(), Some(MemberName::lit("Ping")));
    assert_eq!(h.destination(), Some(BusName::lit("org.example.B")));
    assert_eq!(h.sender(), Some(BusName::lit(":1.7")));
    assert_eq!(h.error_name(), None);
    assert_eq!(h.reply_serial(), None);
    assert_eq!(h.fds().get(), 0);
}
