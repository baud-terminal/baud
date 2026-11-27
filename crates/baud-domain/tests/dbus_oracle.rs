//! The encoder against an independent marshaller's bytes: every hex string
//! below is copied from `crates/baud-app/fixtures/bus-replies.hex` (the lines
//! named after each const), from `bus-read-all-interface.hex` beside it,
//! or, for `HELLO`, from the const of that name in `crates/baud-app/src/bus.rs`'s
//! tests, all of them dbus-next 0.2.3's output
//! (`[[source: dbus-spec]]`, "Message Format", "Header Fields",
//! "Marshalling containers").

use baud_domain::dbus::{
    Args, Array, Body, Boolean, BusName, Byte, Bytes, Dict, FdCount, FlatPairs, HELLO, I32,
    ImageData, InterfaceName, MemberName, MethodCall, NotifyArgs, ObjectPath, Serial, Serials, Sig,
    SigCodes, Str, U32, Variant,
};

/// `notify_call`: `Notify` as serial 6, with every hint type the
/// notification half sends and two action pairs.
const NOTIFY_CALL: &str = "6c01000110010000060000009b00000001016f001e0000002f6f72672f667265656465736b746f702f4e6f74696669636174696f6e730000020173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e7300000003017300060000004e6f746966790000060173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e73000000080167000d73757373736173617b73767d69000000000000040000006261756400000000000000000c0000006469616c6f672d6572726f7200000000050000004275696c64000000120000006d616b653b206d616b6520696e7374616c6c0000280000000700000064656661756c7400040000004f70656e00000000010000003100000003000000596573009400000007000000757267656e637900017900020e00000073757070726573732d736f756e6400016200000001000000000000000a000000736f756e642d6e616d650001730000000400000062656c6c000000000a000000696d6167652d64617461000a2869696962696961792900000000000002000000010000000800000001000000080000000400000008000000ff0000ff00ff008088130000";

/// `notify_replace_call`: `Notify` as serial 7, replacing 42, with empty
/// strings, no actions, no hints and the server's timeout.
const NOTIFY_REPLACE_CALL: &str = "6c01000134000000070000009b00000001016f001e0000002f6f72672f667265656465736b746f702f4e6f74696669636174696f6e730000020173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e7300000003017300060000004e6f746966790000060173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e73000000080167000d73757373736173617b73767d6900000000000000000000000000002a000000000000000000000005000000416761696e00000000000000000000000000000000000000ffffffff";

/// `close_notification_call`: `CloseNotification(42)` as serial 8.
const CLOSE_NOTIFICATION_CALL: &str = "6c01000104000000080000009f00000001016f001e0000002f6f72672f667265656465736b746f702f4e6f74696669636174696f6e730000020173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e730000000301730011000000436c6f73654e6f74696669636174696f6e00000000000000060173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e7300000008016700017500002a000000";

/// `get_capabilities_call`: `GetCapabilities()` as serial 5, no body and so
/// no SIGNATURE field.
const GET_CAPABILITIES_CALL: &str = "6c01000100000000050000008e00000001016f001e0000002f6f72672f667265656465736b746f702f4e6f74696669636174696f6e730000020173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e73000000030173000f0000004765744361706162696c697469657300060173001d0000006f72672e667265656465736b746f702e4e6f74696669636174696f6e73000000";

/// `add_match_call`: `AddMatch` for the settings portal's signal, serial 2.
const ADD_MATCH_CALL: &str = "6c0100017d000000020000007f00000001016f00150000002f6f72672f667265656465736b746f702f4442757300000002017300140000006f72672e667265656465736b746f702e444275730000000003017300080000004164644d61746368000000000000000006017300140000006f72672e667265656465736b746f702e4442757300000000080167000173000078000000747970653d277369676e616c272c696e746572666163653d276f72672e667265656465736b746f702e706f7274616c2e53657474696e6773272c6d656d6265723d2753657474696e674368616e676564272c706174683d272f6f72672f667265656465736b746f702f706f7274616c2f6465736b746f702700";

/// The fixture line notify_add_match_call: `AddMatch` for the notification
/// server's signals, serial 4.
const NOTIFY_ADD_MATCH_CALL: &str = "6c01000162000000040000007f00000001016f00150000002f6f72672f667265656465736b746f702f4442757300000002017300140000006f72672e667265656465736b746f702e444275730000000003017300080000004164644d61746368000000000000000006017300140000006f72672e667265656465736b746f702e444275730000000008016700017300005d000000747970653d277369676e616c272c696e746572666163653d276f72672e667265656465736b746f702e4e6f74696669636174696f6e73272c706174683d272f6f72672f667265656465736b746f702f4e6f74696669636174696f6e732700";

/// `crates/baud-app/fixtures/bus-read-all-interface.hex`: the settings portal's
/// `ReadAll` with two namespaces, serial 3.
const READ_ALL_INTERFACE_CALL: &str = "6c01000144000000030000009000000001016f001f0000002f6f72672f667265656465736b746f702f706f7274616c2f6465736b746f7000020173001f0000006f72672e667265656465736b746f702e706f7274616c2e53657474696e677300030173000700000052656164416c6c00060173001e0000006f72672e667265656465736b746f702e706f7274616c2e4465736b746f7000000801670002617300400000001a0000006f72672e667265656465736b746f702e617070656172616e636500001b0000006f72672e676e6f6d652e6465736b746f702e696e7465726661636500";

/// `HELLO`: `Hello()` to the bus as serial 1.
const HELLO_CALL: &str = "6c01000100000000010000006d00000001016f00150000002f6f72672f667265656465736b746f702f4442757300000002017300140000006f72672e667265656465736b746f702e4442757300000000030173000500000048656c6c6f00000006017300140000006f72672e667265656465736b746f702e4442757300000000";

const NOTIFY: MethodCall<'static> = MethodCall {
    destination: BusName::lit("org.freedesktop.Notifications"),
    path: ObjectPath::lit("/org/freedesktop/Notifications"),
    interface: InterfaceName::lit("org.freedesktop.Notifications"),
    member: MemberName::lit("Notify"),
};

const CLOSE_NOTIFICATION: MethodCall<'static> = MethodCall {
    member: MemberName::lit("CloseNotification"),
    ..NOTIFY
};

const GET_CAPABILITIES: MethodCall<'static> = MethodCall {
    member: MemberName::lit("GetCapabilities"),
    ..NOTIFY
};

const ADD_MATCH: MethodCall<'static> = MethodCall {
    destination: BusName::lit("org.freedesktop.DBus"),
    path: ObjectPath::lit("/org/freedesktop/DBus"),
    interface: InterfaceName::lit("org.freedesktop.DBus"),
    member: MemberName::lit("AddMatch"),
};

const _: () = assert!(SigCodes::eq(
    <NotifyArgs as Args>::SIG,
    Sig::lit("susssasa{sv}i")
));

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// A counter whose next serial is `n`, drawn the way a connection draws
/// them: `Hello` took 1, and each call before this one took the next.
fn serials_at(n: u32) -> Serials {
    let mut s = Serials::new();
    for _ in 2..n {
        let _ = s.next();
    }
    s
}

#[test]
fn notify_is_the_oracle_s_bytes() {
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
    let args: NotifyArgs<'_> = (
        Str::lit("baud"),
        U32::new(0),
        Str::lit("dialog-error"),
        Str::lit("Build"),
        Str::lit("make; make install"),
        FlatPairs::new(&actions),
        Dict::new(&hints),
        I32::new(5000),
    );
    let body = Body::of(&args).expect("a body within the bounds");
    assert_eq!(body.sig(), Sig::lit("susssasa{sv}i"));
    assert_eq!(body.fds(), FdCount::ZERO);
    let e = NOTIFY
        .encode(&body, &mut serials_at(6))
        .expect("a message within the bound");
    assert_eq!(e.serial().get(), 6);
    assert_eq!(e.fds(), FdCount::ZERO);
    assert_eq!(e.bytes(), hex(NOTIFY_CALL).as_slice());
}

#[test]
fn a_replacing_notify_is_the_oracle_s_bytes() {
    let args: NotifyArgs<'_> = (
        Str::EMPTY,
        U32::new(42),
        Str::EMPTY,
        Str::lit("Again"),
        Str::EMPTY,
        FlatPairs::new(&[]),
        Dict::new(&[]),
        I32::new(-1),
    );
    let body = Body::of(&args).expect("a body within the bounds");
    let e = NOTIFY
        .encode(&body, &mut serials_at(7))
        .expect("a message within the bound");
    assert_eq!(e.serial().get(), 7);
    assert_eq!(e.into_bytes(), hex(NOTIFY_REPLACE_CALL));
}

#[test]
fn close_notification_is_the_oracle_s_bytes() {
    let body = Body::of(&(U32::new(42),)).expect("a body within the bounds");
    assert_eq!(body.sig(), Sig::lit("u"));
    let e = CLOSE_NOTIFICATION
        .encode(&body, &mut serials_at(8))
        .expect("a message within the bound");
    assert_eq!(e.bytes(), hex(CLOSE_NOTIFICATION_CALL).as_slice());
}

#[test]
fn get_capabilities_is_the_oracle_s_bytes() {
    assert_eq!(Body::EMPTY.sig(), Sig::EMPTY);
    assert_eq!(Body::of(&()).expect("empty").sig(), Sig::EMPTY);
    let e = GET_CAPABILITIES
        .encode(&Body::EMPTY, &mut serials_at(5))
        .expect("a message within the bound");
    assert_eq!(e.bytes(), hex(GET_CAPABILITIES_CALL).as_slice());
}

#[test]
fn add_match_is_the_oracle_s_bytes() {
    let portal = Body::of(&(Str::lit(
        "type='signal',interface='org.freedesktop.portal.Settings',\
         member='SettingChanged',path='/org/freedesktop/portal/desktop'",
    ),))
    .expect("a body within the bounds");
    let e = ADD_MATCH
        .encode(&portal, &mut serials_at(2))
        .expect("a message within the bound");
    assert_eq!(e.bytes(), hex(ADD_MATCH_CALL).as_slice());

    let notifications = Body::of(&(Str::lit(
        "type='signal',interface='org.freedesktop.Notifications',\
         path='/org/freedesktop/Notifications'",
    ),))
    .expect("a body within the bounds");
    let e = ADD_MATCH
        .encode(&notifications, &mut serials_at(4))
        .expect("a message within the bound");
    assert_eq!(e.bytes(), hex(NOTIFY_ADD_MATCH_CALL).as_slice());
}

#[test]
fn hello_is_the_oracle_s_bytes_and_serial_one() {
    assert_eq!(HELLO, hex(HELLO_CALL).as_slice());
    assert_eq!(Serial::HELLO.get(), 1);
    let mut serials = Serials::new();
    assert_eq!(serials.next().get(), 2, "the first call after Hello is 2");
    assert_eq!(serials.next().get(), 3);
}

#[test]
fn an_array_of_strings_is_as() {
    let names = [
        Str::lit("org.freedesktop.appearance"),
        Str::lit("org.gnome.desktop.interface"),
    ];
    let body = Body::of(&(Array::new(&names),)).expect("a body within the bounds");
    assert_eq!(body.sig(), Sig::lit("as"));
    let read_all = MethodCall {
        destination: BusName::lit("org.freedesktop.portal.Desktop"),
        path: ObjectPath::lit("/org/freedesktop/portal/desktop"),
        interface: InterfaceName::lit("org.freedesktop.portal.Settings"),
        member: MemberName::lit("ReadAll"),
    };
    let e = read_all
        .encode(&body, &mut serials_at(3))
        .expect("a message within the bound");
    assert_eq!(e.bytes(), hex(READ_ALL_INTERFACE_CALL).as_slice());
}
