//! Received variants and dicts read as their types, against
//! `[[source: dbus-spec]]`, "Marshalling containers": a VARIANT is "the
//! marshalled SIGNATURE of a single complete type, followed by a
//! marshalled value with the type given in the signature", and a
//! DICT_ENTRY is "marshalled like a STRUCT" in an ARRAY. A value is read
//! only as the type its signature names, and one of another type is
//! passed over by that signature, so the next entry still reads.

use baud_domain::dbus::{
    Body, Boolean, BusName, Bytes, Dict, Double, I32, ImageData, Inbound, InterfaceName,
    MemberName, MethodCall, ObjectPath, Serials, Str, U32, Variant, VariantRef, decode,
};

const CALL: MethodCall<'static> = MethodCall {
    destination: BusName::lit("org.example.Vardict"),
    path: ObjectPath::lit("/org/example/Vardict"),
    interface: InterfaceName::lit("org.example.Vardict"),
    member: MemberName::lit("Take"),
};

/// One call whose body is `(a{sv},)`: a page setup's shape, with a
/// nested `a{sv}`, a `d`, a `u`, a `s` and a struct the reader asks for
/// as nothing.
fn vardict() -> Vec<u8> {
    let pixels = [0u8; 4];
    let inner = [
        (Str::lit("Width"), Variant::Double(Double::new(210.0))),
        (Str::lit("Name"), Variant::Str(Str::lit("iso_a4"))),
    ];
    let outer = [
        (
            Str::lit("icon"),
            Variant::Image(ImageData {
                width: I32::new(1),
                height: I32::new(1),
                rowstride: I32::new(4),
                alpha: Boolean::True,
                bits_per_sample: I32::new(8),
                channels: I32::new(4),
                data: Bytes::new(&pixels),
            }),
        ),
        (Str::lit("token"), Variant::U32(U32::new(7))),
        (Str::lit("page-setup"), Variant::Dict(Dict::new(&inner))),
        (Str::lit("scale"), Variant::Double(Double::new(1.5))),
    ];
    let body = Body::of(&(Dict::new(&outer),)).expect("small");
    CALL.encode(&body, &mut Serials::new())
        .expect("small")
        .bytes()
        .to_vec()
}

fn entries(bytes: &[u8]) -> Vec<(String, VariantRef<'_>)> {
    let Ok(Inbound::Called(c)) = decode(bytes) else {
        panic!("a call")
    };
    let (dict,) = c
        .body()
        .read::<(Dict<'_, Str<'_>, Variant<'_>>,)>()
        .expect("a{sv}");
    dict.map(|(k, v)| (k.as_str().to_owned(), v)).collect()
}

/// **Each value reads as the type its signature names.**
#[test]
fn each_value_reads_as_its_signature_s_type() {
    let bytes = vardict();
    let all = entries(&bytes);
    let keys: Vec<_> = all.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["icon", "token", "page-setup", "scale"]);
    let get = |key: &str| all.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    assert_eq!(
        get("token").and_then(|v| v.read::<U32>()).map(U32::get),
        Some(7)
    );
    assert_eq!(
        get("scale")
            .and_then(|v| v.read::<Double>())
            .map(Double::get),
        Some(1.5)
    );
    assert_eq!(
        get("token").map(|v| v.sig().as_bytes().to_vec()),
        Some(b"u".to_vec())
    );
}

/// **A value asked for as another type is `None`**, and asking reads no
/// byte: the entries after it read as before.
#[test]
fn a_value_of_another_type_is_none_and_the_rest_still_read() {
    let bytes = vardict();
    let all = entries(&bytes);
    let token = &all[1].1;
    assert!(token.read::<Double>().is_none(), "a u is not a d");
    assert!(token.read::<Str<'_>>().is_none(), "nor an s");
    assert!(all[0].1.read::<U32>().is_none(), "a struct is not a u");
    assert_eq!(all[3].1.read::<Double>().map(Double::get), Some(1.5));
}

/// **A nested `a{sv}` reads as a dict of its own**, by key.
#[test]
fn a_nested_vardict_reads_by_key() {
    let bytes = vardict();
    let all = entries(&bytes);
    let mut setup = all[2]
        .1
        .read::<Dict<'_, Str<'_>, Variant<'_>>>()
        .expect("a{sv}");
    let width = setup
        .clone()
        .find(|(k, _)| k.as_str() == "Width")
        .and_then(|(_, v)| v.read::<Double>());
    assert_eq!(width.map(Double::get), Some(210.0));
    let name = setup
        .find(|(k, _)| k.as_str() == "Name")
        .and_then(|(_, v)| v.read::<Str<'_>>());
    assert_eq!(name.map(|s| s.as_str()), Some("iso_a4"));
}
