//! The encoder's bounds and alignment against `[[source: dbus-spec]]`:
//! "Byte order and alignment", "Marshalling containers" (an array is at
//! most 2^26 bytes, its first element padded even when there is none),
//! "Message Format" (a message is at most 2^27 bytes) and "Container types"
//! (variants nest a message at most 64 deep).

use baud_domain::dbus::{
    Array, ArrayLen, Body, Boolean, BusName, Byte, Bytes, Dict, Double, EncodeError, InterfaceName,
    MemberName, MessageLen, MethodCall, ObjectPath, Serials, Sig, Str, Struct, U32, Variant,
};

const CALL: MethodCall<'static> = MethodCall {
    destination: BusName::lit("org.example.Bounds"),
    path: ObjectPath::lit("/org/example/Bounds"),
    interface: InterfaceName::lit("org.example.Bounds"),
    member: MemberName::lit("Take"),
};

const ARRAY_MAX: usize = 1 << 26;
const MESSAGE_MAX: usize = 1 << 27;

#[test]
fn the_length_types_hold_the_spec_s_bounds() {
    assert_eq!(ArrayLen::new(ARRAY_MAX).map(ArrayLen::get), Some(1 << 26));
    assert_eq!(ArrayLen::new(ARRAY_MAX + 1), None);
    assert_eq!(
        MessageLen::new(MESSAGE_MAX).map(MessageLen::get),
        Some(1 << 27)
    );
    assert_eq!(MessageLen::new(MESSAGE_MAX + 1), None);
}

#[test]
fn an_array_is_at_most_two_to_the_26_bytes() {
    let words = vec![U32::new(7); ARRAY_MAX / 4];
    let body = Body::of(&(Array::new(&words),)).expect("2^26 bytes is the bound");
    assert_eq!(body.bytes().len(), 4 + ARRAY_MAX);
    let over = vec![U32::new(7); ARRAY_MAX / 4 + 1];
    assert_eq!(
        Body::of(&(Array::new(&over),)).map(|_| ()),
        Err(EncodeError::ArrayTooLong)
    );
    let bytes = vec![0u8; ARRAY_MAX + 1];
    assert!(Body::of(&(Bytes::new(&bytes[..ARRAY_MAX]),)).is_ok());
    assert_eq!(
        Body::of(&(Bytes::new(&bytes),)).map(|_| ()),
        Err(EncodeError::ArrayTooLong)
    );
}

#[test]
fn a_message_is_at_most_two_to_the_27_bytes() {
    let zeros = vec![0u8; ARRAY_MAX];
    let full = Bytes::new(&zeros);
    // The header of a call carrying `ayay`, whatever its body.
    let small = Body::of(&(Bytes::new(&[]), Bytes::new(&[]))).expect("small");
    let header = CALL
        .encode(&small, &mut Serials::new())
        .expect("small")
        .bytes()
        .len()
        - small.bytes().len();
    assert_eq!(header % 8, 0, "the body starts on 8");
    // Two arrays: 4 + 2^26, then 4 + rest.
    let rest = MESSAGE_MAX - header - (4 + ARRAY_MAX) - 4;
    let exact = Body::of(&(full, Bytes::new(&zeros[..rest]))).expect("within 2^27");
    let e = CALL
        .encode(&exact, &mut Serials::new())
        .expect("exactly 2^27 is the bound");
    assert_eq!(e.bytes().len(), MESSAGE_MAX);
    let over = Body::of(&(full, Bytes::new(&zeros[..rest + 1]))).expect("within 2^27");
    let mut serials = Serials::new();
    assert_eq!(
        CALL.encode(&over, &mut serials).map(|_| ()),
        Err(EncodeError::TooLong)
    );
    assert_eq!(serials.next().get(), 2, "a message refused draws no serial");
    // A body past 2^27 alone is refused before any header.
    assert_eq!(
        Body::of(&(full, full, Bytes::new(&zeros[..8]))).map(|_| ()),
        Err(EncodeError::TooLong)
    );
}

/// `Dict<Str, Variant>` nested `levels` more times around `value`, as a
/// one-argument body.
fn nest(levels: usize, value: Variant<'_>) -> Result<Body, EncodeError> {
    let entry = [(Str::lit("k"), value)];
    let dict = Dict::new(&entry);
    if levels == 0 {
        Body::of(&(dict,))
    } else {
        nest(levels - 1, Variant::Dict(dict))
    }
}

#[test]
fn variants_nest_a_message_at_most_64_deep() {
    // Each level is an array and a variant: 2 + 2 * 31 = 64.
    assert!(nest(31, Variant::Byte(Byte::new(1))).is_ok());
    let none: [Str<'_>; 0] = [];
    assert_eq!(
        nest(31, Variant::Strs(Array::new(&none))).map(|_| ()),
        Err(EncodeError::TooDeep)
    );
    assert_eq!(
        nest(32, Variant::Byte(Byte::new(1))).map(|_| ()),
        Err(EncodeError::TooDeep)
    );
}

#[test]
fn values_align_naturally_and_padding_is_nul() {
    let body = Body::of(&(Byte::new(0xaa), Double::new(1.0))).expect("small");
    assert_eq!(body.sig(), Sig::lit("yd"));
    let mut want = vec![0xaa, 0, 0, 0, 0, 0, 0, 0];
    want.extend_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(body.bytes(), want.as_slice());

    let body = Body::of(&(Byte::new(1), Struct::new((Byte::new(2),)))).expect("small");
    assert_eq!(body.sig(), Sig::lit("y(y)"));
    assert_eq!(body.bytes(), &[1, 0, 0, 0, 0, 0, 0, 0, 2]);

    let body = Body::of(&(Boolean::True, Boolean::False)).expect("small");
    assert_eq!(body.bytes(), &[1, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn an_empty_array_pads_to_its_element_s_boundary() {
    let none: [Double; 0] = [];
    let body = Body::of(&(Byte::new(1), Array::new(&none))).expect("small");
    assert_eq!(body.sig(), Sig::lit("yad"));
    assert_eq!(body.bytes(), &[1, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn a_variant_s_value_aligns_after_its_signature() {
    let body = Body::of(&(Variant::Double(Double::new(2.0)),)).expect("small");
    assert_eq!(body.sig(), Sig::lit("v"));
    let mut want = vec![1, b'd', 0, 0, 0, 0, 0, 0];
    want.extend_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(body.bytes(), want.as_slice());
}
