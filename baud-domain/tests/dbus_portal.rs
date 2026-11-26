//! The desktop portal's Request protocol against `[[source:
//! xdg-portal-request]]` and `[[source: xdg-portal-print]]`: the predicted
//! Request path, the returned handle compared with it, the response codes,
//! a `Response` read only from a vouched signal, and a page setup's
//! millimetres.

use baud_domain::PaperMm;
use baud_domain::dbus::{
    Body, BusName, Dict, Double, Emit, Handle, HandleToken, Inbound, ObjectPath, PageSetup, Peer,
    PortalResponse, REQUEST, RESPONSE, RequestPath, ResponseCode, Serials, Signal, Str, U32,
    Variant, decode,
};

/// **The path is the text's own example's shape**: the unique name with
/// its `:` dropped and every `.` made `_`, then the token.
#[test]
fn the_request_path_is_predicted_from_the_unique_name_and_token() {
    let token = HandleToken::parse("t1").expect("a token");
    let path = RequestPath::predict(BusName::lit(":1.42"), &token).expect("unique");
    assert_eq!(
        path.as_path().as_str(),
        "/org/freedesktop/portal/desktop/request/1_42/t1"
    );
    let numbered = HandleToken::numbered(4_294_967_295);
    assert_eq!(numbered.as_str(), "baud_4294967295");
    let path = RequestPath::predict(BusName::lit(":1.7"), &numbered).expect("unique");
    assert_eq!(
        path.as_path().as_str(),
        "/org/freedesktop/portal/desktop/request/1_7/baud_4294967295"
    );
}

/// **No path from a well-known name or one an element cannot hold.**
#[test]
fn a_well_known_name_or_a_dash_predicts_no_path() {
    let token = HandleToken::numbered(1);
    assert!(RequestPath::predict(BusName::lit("org.example.App"), &token).is_none());
    assert!(RequestPath::predict(BusName::lit(":1.4-2"), &token).is_none());
}

/// **A token is an object path element**: `[A-Za-z0-9_]`, one at least,
/// and at most `MAX` here.
#[test]
fn a_token_is_an_object_path_element() {
    assert!(HandleToken::parse("").is_none());
    assert!(HandleToken::parse("a.b").is_none());
    assert!(HandleToken::parse("a/b").is_none());
    assert!(HandleToken::parse(&"a".repeat(HandleToken::MAX)).is_some());
    assert!(HandleToken::parse(&"a".repeat(HandleToken::MAX + 1)).is_none());
}

/// **The returned handle is checked**: the predicted one stands, another
/// moves the subscription ("verify that the returned handle is what it
/// expected, and update its signal subscription if it isn't").
#[test]
fn a_returned_handle_is_expected_or_moved() {
    let token = HandleToken::numbered(3);
    let path = RequestPath::predict(BusName::lit(":1.42"), &token).expect("unique");
    assert_eq!(path.matches(path.as_path()), Handle::Expected);
    let other = ObjectPath::lit("/org/freedesktop/portal/desktop/request/1_42/other");
    assert_eq!(path.matches(other), Handle::Moved(other));
}

/// **The three codes the text gives, and any other kept and not success.**
#[test]
fn response_codes_are_the_text_s_three_and_the_rest_unknown() {
    assert_eq!(ResponseCode::of(U32::new(0)), ResponseCode::Success);
    assert_eq!(ResponseCode::of(U32::new(1)), ResponseCode::Cancelled);
    assert_eq!(ResponseCode::of(U32::new(2)), ResponseCode::Ended);
    assert_eq!(
        ResponseCode::of(U32::new(3)),
        ResponseCode::Unknown(U32::new(3))
    );
}

/// `Response(code, {page-setup: {Width, Height, MarginTop, ..}})` from
/// `sender` on `path`.
fn response(sender: &str, path: &'static str, code: u32, width: f64) -> Vec<u8> {
    let setup = [
        (Str::lit("Width"), Variant::Double(Double::new(width))),
        (Str::lit("Height"), Variant::Double(Double::new(297.0))),
        (Str::lit("MarginTop"), Variant::Double(Double::new(-1.0))),
        (Str::lit("MarginLeft"), Variant::U32(U32::new(5))),
        (Str::lit("MarginRight"), Variant::Double(Double::new(6.35))),
    ];
    let results = [(Str::lit("page-setup"), Variant::Dict(Dict::new(&setup)))];
    let body = Body::of(&(U32::new(code), Dict::new(&results))).expect("small");
    let emit = Emit {
        path: ObjectPath::lit(path),
        interface: REQUEST,
        member: RESPONSE,
    };
    let mut bytes = emit
        .encode(&body, &mut Serials::new())
        .expect("small")
        .bytes()
        .to_vec();
    with_sender(&mut bytes, sender);
    bytes
}

/// Adds a SENDER field, as the bus does, to the message in `bytes`.
fn with_sender(bytes: &mut Vec<u8>, sender: &str) {
    let fields_len =
        usize::try_from(u32::from_le_bytes(bytes[12..16].try_into().expect("4"))).expect("small");
    let body_at = (16 + fields_len).next_multiple_of(8);
    let body = bytes.split_off(body_at);
    bytes.truncate(16 + fields_len);
    while bytes.len() % 8 != 0 {
        bytes.push(0);
    }
    bytes.extend_from_slice(&[7, 1, b's', 0]);
    bytes.extend_from_slice(&u32::try_from(sender.len()).expect("small").to_le_bytes());
    bytes.extend_from_slice(sender.as_bytes());
    bytes.push(0);
    let fields_len = bytes.len() - 16;
    bytes[12..16].copy_from_slice(&u32::try_from(fields_len).expect("small").to_le_bytes());
    while bytes.len() % 8 != 0 {
        bytes.push(0);
    }
    bytes.extend_from_slice(&body);
}

fn signal(bytes: &[u8]) -> Signal<'_> {
    match decode(bytes) {
        Ok(Inbound::Signal(s)) => s,
        other => panic!("a signal, not {other:?}"),
    }
}

/// The portal's peer: the SENDER of a reply from `:1.9`.
fn portal() -> Peer {
    let reply = b"l\x02\x00\x01\x00\x00\x00\x00\x02\x00\x00\x00\x15\x00\x00\x00\x05\x01u\x00\x01\x00\x00\x00\x07\x01s\x00\x04\x00\x00\x00:1.9\x00\x00\x00\x00";
    match decode(reply) {
        Ok(Inbound::Return(r)) => Peer::from_reply(&r).expect("a unique SENDER"),
        other => panic!("a reply, not {other:?}"),
    }
}

/// **A `Response` is read only from the portal**: one from another
/// connection on the same path is not vouched for, so it is never read.
#[test]
fn a_response_is_read_only_when_its_sender_is_vouched_for() {
    let path = "/org/freedesktop/portal/desktop/request/1_42/baud_1";
    let real = response(":1.9", path, 0, 210.0);
    let forged = response(":1.66", path, 1, 210.0);
    let p = portal();
    assert!(p.vouch(&signal(&forged)).is_none(), "another connection");
    let real = signal(&real);
    let trusted = p.vouch(&real).expect("the portal's");
    let r = PortalResponse::from_trusted(&trusted).expect("a Response");
    assert_eq!(r.code, ResponseCode::Success);
    assert_eq!(r.path.as_str(), path);
}

/// **A page setup keeps only millimetres that are lengths**: a negative
/// margin, a margin of another type and a zero width are `None`.
#[test]
fn a_page_setup_keeps_finite_lengths_in_millimetres() {
    let path = "/org/freedesktop/portal/desktop/request/1_42/baud_1";
    for (width, want) in [(210.0, PaperMm::of(210.0)), (0.0, None), (f64::NAN, None)] {
        let bytes = response(":1.9", path, 0, width);
        let s = signal(&bytes);
        let trusted = portal().vouch(&s).expect("the portal's");
        let r = PortalResponse::from_trusted(&trusted).expect("a Response");
        let (_, setup) = r
            .results()
            .find(|(k, _)| k.as_str() == "page-setup")
            .expect("page-setup");
        let setup = PageSetup::read(
            setup
                .read::<Dict<'_, Str<'_>, Variant<'_>>>()
                .expect("a{sv}"),
        );
        assert_eq!(setup.width, want, "width {width}");
        assert_eq!(setup.height, PaperMm::of(297.0));
        assert_eq!(setup.top, None, "a negative margin");
        assert_eq!(setup.left, None, "a margin that is a u");
        assert_eq!(setup.right, PaperMm::of(6.35));
        assert_eq!(setup.bottom, None, "absent");
    }
    let a4 = PaperMm::of(210.0).expect("a length").to_pt().get();
    assert!((a4 - 595.275_590_551).abs() < 1e-6, "{a4}");
}
