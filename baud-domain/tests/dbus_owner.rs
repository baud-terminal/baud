//! Signals from the bus itself, and which connection holds a watched name,
//! against `[[source: dbus-spec]]`: "Message Bus Overview" (the bus holds
//! `org.freedesktop.DBus`, with its object at `/org/freedesktop/DBus` and
//! its interface `org.freedesktop.DBus`), "Header Fields" (SENDER is set by
//! the bus), "Message Bus Names" (no connection takes a name beginning with
//! `:`) and "Signal: org.freedesktop.DBus.NameOwnerChanged" (the name, the
//! connection that held it and the one that holds it now, each a STRING,
//! empty for none).
//!
//! The messages are built here, little-endian, with the fields as "Header
//! Fields" lays them out.

use baud_domain::dbus::{BusName, HolderChange, Inbound, Signal, decode};

const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const WATCHED: BusName<'static> = BusName::lit("org.freedesktop.Notifications");

/// A header field's value, as "Header Fields" types it.
enum Value<'a> {
    Path(&'a str),
    Str(&'a str),
}

/// The signal's header fields: PATH, INTERFACE, MEMBER and SENDER.
struct Head<'a> {
    path: &'a str,
    interface: &'a str,
    member: &'a str,
    sender: &'a str,
}

/// `NameOwnerChanged`'s header as the bus sends it.
const FROM_THE_BUS: Head<'static> = Head {
    path: BUS_PATH,
    interface: BUS,
    member: "NameOwnerChanged",
    sender: BUS,
};

fn pad(m: &mut Vec<u8>, to: usize) {
    while !m.len().is_multiple_of(to) {
        m.push(0);
    }
}

fn string(m: &mut Vec<u8>, s: &str) {
    pad(m, 4);
    m.extend_from_slice(&u32::try_from(s.len()).expect("small").to_le_bytes());
    m.extend_from_slice(s.as_bytes());
    m.push(0);
}

/// A little-endian SIGNAL, serial 3, with `head`'s fields, then SIGNATURE
/// `sig` when it is not empty, then `body`.
fn signal_bytes(head: &Head<'_>, sig: &str, body: &[u8]) -> Vec<u8> {
    let mut m = vec![b'l', 4, 0, 1];
    m.extend_from_slice(&u32::try_from(body.len()).expect("small").to_le_bytes());
    m.extend_from_slice(&3u32.to_le_bytes());
    m.extend_from_slice(&[0; 4]);
    let fields = [
        (1, Value::Path(head.path)),
        (2, Value::Str(head.interface)),
        (3, Value::Str(head.member)),
        (7, Value::Str(head.sender)),
    ];
    for (code, value) in &fields {
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

/// The STRINGs `strs`, one after another, as a body.
fn strings(strs: &[&str]) -> Vec<u8> {
    let mut b = Vec::new();
    for s in strs {
        string(&mut b, s);
    }
    b
}

/// `NameOwnerChanged(name, from, to)` with `head`'s fields.
fn changed(head: &Head<'_>, name: &str, from: &str, to: &str) -> Vec<u8> {
    signal_bytes(head, "sss", &strings(&[name, from, to]))
}

fn signal(bytes: &[u8]) -> Signal<'_> {
    match decode(bytes) {
        Ok(Inbound::Signal(s)) => s,
        other => panic!("a signal: {other:?}"),
    }
}

/// What the bus's signal in `bytes` says of `WATCHED`.
fn change(bytes: &[u8]) -> Option<HolderChange> {
    let s = signal(bytes);
    let bus = s.from_bus().expect("from the bus");
    assert_eq!(bus.signal(), s);
    HolderChange::of(&bus, WATCHED)
}

/// **The bus's `NameOwnerChanged` to a unique name names the connection
/// that now holds the watched name.**
#[test]
fn the_bus_names_the_new_holder() {
    let m = changed(&FROM_THE_BUS, WATCHED.as_str(), ":1.7", ":1.9");
    match change(&m) {
        Some(HolderChange::To(peer)) => assert_eq!(peer.name(), BusName::lit(":1.9")),
        other => panic!("a new holder: {other:?}"),
    }
    let appeared = changed(&FROM_THE_BUS, WATCHED.as_str(), "", ":1.12");
    match change(&appeared) {
        Some(HolderChange::To(peer)) => assert_eq!(peer.name(), BusName::lit(":1.12")),
        other => panic!("a first holder: {other:?}"),
    }
}

/// **An empty new holder means the name is gone.**
#[test]
fn an_empty_new_holder_is_gone() {
    let m = changed(&FROM_THE_BUS, WATCHED.as_str(), ":1.7", "");
    assert_eq!(change(&m), Some(HolderChange::Gone));
}

/// **A change to another name says nothing of the watched one.**
#[test]
fn another_name_is_none() {
    let m = changed(&FROM_THE_BUS, "org.example.Other", ":1.7", ":1.9");
    assert_eq!(change(&m), None);
    let unique = changed(&FROM_THE_BUS, ":1.9", "", ":1.9");
    assert_eq!(change(&unique), None);
}

/// **A non-empty new holder that is not a unique name is no connection.**
#[test]
fn a_well_known_new_holder_is_none() {
    let m = changed(&FROM_THE_BUS, WATCHED.as_str(), ":1.7", "org.example.Other");
    assert_eq!(change(&m), None);
}

/// **Only `NameOwnerChanged`, with three STRINGs, is read.**
#[test]
fn another_member_or_body_is_none() {
    let lost = Head {
        member: "NameLost",
        ..FROM_THE_BUS
    };
    let m = changed(&lost, WATCHED.as_str(), ":1.7", ":1.9");
    assert_eq!(change(&m), None);

    let two = signal_bytes(&FROM_THE_BUS, "ss", &strings(&[WATCHED.as_str(), ":1.9"]));
    assert_eq!(change(&two), None);
    let four = signal_bytes(
        &FROM_THE_BUS,
        "ssss",
        &strings(&[WATCHED.as_str(), ":1.7", ":1.9", ""]),
    );
    assert_eq!(change(&four), None);
    let empty = signal_bytes(&FROM_THE_BUS, "", &[]);
    assert_eq!(change(&empty), None);
}

/// **A signal is from the bus only when its SENDER is the bus's own name**:
/// a client that sends `NameOwnerChanged` from the bus's path and
/// interface does not speak for the bus.
#[test]
fn a_client_sender_is_not_the_bus() {
    for sender in [":1.9", "org.example.Other"] {
        let forged = Head {
            sender,
            ..FROM_THE_BUS
        };
        let m = changed(&forged, WATCHED.as_str(), ":1.7", ":1.9");
        assert!(signal(&m).from_bus().is_none(), "SENDER {sender}");
    }
    let ours = changed(&FROM_THE_BUS, WATCHED.as_str(), ":1.7", ":1.9");
    assert!(signal(&ours).from_bus().is_some(), "the positive control");
}

/// **The bus's signals come from its own object and interface**; the same
/// SENDER on another path or interface is not taken for them.
#[test]
fn a_wrong_path_or_interface_is_not_the_bus() {
    let path = Head {
        path: "/org/freedesktop/DBus/Local",
        ..FROM_THE_BUS
    };
    let m = changed(&path, WATCHED.as_str(), ":1.7", ":1.9");
    assert!(signal(&m).from_bus().is_none(), "another path");
    let interface = Head {
        interface: "org.freedesktop.DBus.Properties",
        ..FROM_THE_BUS
    };
    let m = changed(&interface, WATCHED.as_str(), ":1.7", ":1.9");
    assert!(signal(&m).from_bus().is_none(), "another interface");
}
