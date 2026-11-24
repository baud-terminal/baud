//! `Sig::parse` against the signature rules of `[[source: dbus-spec]]`,
//! "Valid Signatures" and "Container types": each refused example names the
//! rule it breaks, and the accepted ones are the spec's own.

use baud_domain::dbus::{Depth, Sig, SigCodes, SigError};

fn parse(s: &str) -> Result<Sig<'_>, SigError> {
    Sig::parse(s.as_bytes())
}

#[test]
fn the_spec_s_examples_are_accepted() {
    for s in [
        "",
        "i",
        "ii",
        "ai",
        "a(ii)",
        "aai",
        "(ii)",
        "(i(ii))",
        "a{sa{sv}}",
        "a{sv}",
        "susssasa{sv}i",
        "v",
        "ybnqiuxtdhsog",
    ] {
        let sig = parse(s).unwrap_or_else(|e| panic!("{s:?} refused: {e:?}"));
        assert_eq!(sig.as_bytes(), s.as_bytes());
        assert_eq!(usize::from(sig.len().get()), s.len());
    }
}

#[test]
fn an_array_needs_its_element() {
    assert_eq!(parse("aa"), Err(SigError::ArrayNoElement));
    assert_eq!(parse("a"), Err(SigError::ArrayNoElement));
    assert_eq!(parse("(a)"), Err(SigError::ArrayNoElement));
}

#[test]
fn a_struct_opens_and_closes() {
    assert_eq!(parse("(ii"), Err(SigError::StructUnclosed));
    assert_eq!(parse("ii)"), Err(SigError::CloseUnopened));
    assert_eq!(parse("a{ss}}"), Err(SigError::CloseUnopened));
}

#[test]
fn a_struct_is_never_empty() {
    assert_eq!(parse("()"), Err(SigError::EmptyStruct));
    assert_eq!(parse("a()"), Err(SigError::EmptyStruct));
}

#[test]
fn a_dict_entry_is_an_array_element_with_a_basic_key_and_one_value() {
    assert_eq!(parse("{ss}"), Err(SigError::DictOutsideArray));
    assert_eq!(parse("(i{ss})"), Err(SigError::DictOutsideArray));
    assert_eq!(parse("a{vs}"), Err(SigError::DictKey));
    assert_eq!(parse("a{(i)s}"), Err(SigError::DictKey));
    assert_eq!(parse("a{s}"), Err(SigError::DictArity));
    assert_eq!(parse("a{}"), Err(SigError::DictArity));
    assert_eq!(parse("a{sss}"), Err(SigError::DictArity));
    assert_eq!(parse("a{s"), Err(SigError::DictArity));
}

#[test]
fn reserved_and_unknown_codes_are_refused() {
    for s in ["r", "e", "m", "*", "?", "@", "&", "^", "z", "ai\0", " "] {
        assert_eq!(parse(s), Err(SigError::Code), "{s:?}");
    }
}

#[test]
fn nesting_stops_at_32_arrays_and_32_structs() {
    let arrays = |n| format!("{}i", "a".repeat(n));
    let structs = |n| format!("{}i{}", "(".repeat(n), ")".repeat(n));
    assert!(parse(&arrays(32)).is_ok());
    assert_eq!(parse(&arrays(33)), Err(SigError::ArraysTooDeep));
    assert!(parse(&structs(32)).is_ok());
    assert_eq!(parse(&structs(33)), Err(SigError::StructsTooDeep));
    // The two bounds are separate: 32 of each nest to 64.
    let both = format!("{}{}", "a(".repeat(32), "i".to_owned() + &")".repeat(32));
    assert!(parse(&both).is_ok());
    assert_eq!(Depth::NESTING.get(), 32);
}

#[test]
fn a_signature_is_at_most_255_codes() {
    assert!(parse(&"i".repeat(255)).is_ok());
    assert_eq!(parse(&"i".repeat(256)), Err(SigError::TooLong));
}

const NOTIFY: SigCodes = SigCodes::of(Sig::lit("sus"))
    .concat(SigCodes::of(Sig::lit("ss")))
    .concat(SigCodes::array(SigCodes::of(Sig::lit("s"))))
    .concat(SigCodes::dict(
        SigCodes::of(Sig::lit("s")),
        SigCodes::of(Sig::lit("v")),
    ))
    .concat(SigCodes::of(Sig::lit("i")));
const _: () = assert!(SigCodes::eq(NOTIFY, Sig::lit("susssasa{sv}i")));
const _: () = assert!(!SigCodes::eq(NOTIFY, Sig::lit("susssasa{sv}u")));

#[test]
fn codes_are_built_in_const_and_read_as_a_signature() {
    assert_eq!(NOTIFY.as_sig().as_bytes(), b"susssasa{sv}i");
    assert_eq!(SigCodes::EMPTY.as_sig().as_bytes(), b"");
    let pair = SigCodes::structure(SigCodes::of(Sig::lit("ii")));
    assert_eq!(pair.as_sig().as_bytes(), b"(ii)");
    assert!(SigCodes::eq(pair, Sig::lit("(ii)")));
    assert_eq!(pair, SigCodes::of(Sig::lit("(ii)")));
}

#[test]
#[should_panic(expected = "at most 255")]
fn a_concat_past_255_codes_panics() {
    let mut s = SigCodes::of(Sig::parse(&[b'i'; 200]).expect("200 codes"));
    s = s.concat(s);
    let _ = s;
}

#[test]
#[should_panic(expected = "one complete type")]
fn an_array_of_two_types_panics() {
    let _ = SigCodes::array(SigCodes::of(Sig::lit("ii")));
}
