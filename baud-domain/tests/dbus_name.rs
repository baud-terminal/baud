//! The name grammars of `[[source: dbus-spec]]`, "Valid Object Paths" and
//! "Valid Names", one refused case per bullet, and the string rule of "Basic
//! types": UTF-8 with no NUL.

use baud_domain::dbus::{
    BusKind, BusName, ErrorName, InterfaceName, MemberName, NameError, ObjectPath, Str,
};

#[test]
fn a_string_holds_no_nul() {
    assert_eq!(Str::parse("a\0b").map(|s| s.as_str()), Err(NameError::Nul));
    assert_eq!(Str::parse("").map(|s| s.as_str()), Ok(""));
    assert_eq!(
        Str::parse("\u{fdd0}\u{fffe}").map(|s| s.as_str()),
        Ok("\u{fdd0}\u{fffe}")
    );
}

#[test]
fn a_string_is_within_the_message_bound() {
    let at = "a".repeat(1 << 27);
    assert!(Str::parse(&at).is_ok());
    let past = "a".repeat((1 << 27) + 1);
    assert_eq!(Str::parse(&past).map(|_| ()), Err(NameError::TooLong));
}

#[test]
fn object_paths_follow_their_grammar() {
    for ok in ["/", "/a", "/org/freedesktop/Notifications", "/A_9/b"] {
        assert_eq!(ObjectPath::parse(ok).map(|p| p.as_str()), Ok(ok));
    }
    // The path must begin with an ASCII '/'.
    assert_eq!(
        ObjectPath::parse("a/b").map(|_| ()),
        Err(NameError::NotAbsolute)
    );
    assert_eq!(
        ObjectPath::parse("").map(|_| ()),
        Err(NameError::NotAbsolute)
    );
    // Each element must only contain "[A-Z][a-z][0-9]_".
    assert_eq!(ObjectPath::parse("/a-b").map(|_| ()), Err(NameError::Byte));
    assert_eq!(ObjectPath::parse("/a.b").map(|_| ()), Err(NameError::Byte));
    // No element may be the empty string; multiple '/' cannot occur in sequence.
    assert_eq!(
        ObjectPath::parse("/a//b").map(|_| ()),
        Err(NameError::EmptyElement)
    );
    assert_eq!(
        ObjectPath::parse("//").map(|_| ()),
        Err(NameError::EmptyElement)
    );
    // A trailing '/' is not allowed unless the path is the root path.
    assert_eq!(
        ObjectPath::parse("/a/").map(|_| ()),
        Err(NameError::TrailingSlash)
    );
    // The path may be of any length.
    let long = "/a".repeat(200);
    assert!(ObjectPath::parse(&long).is_ok());
}

fn interface_like(parse: fn(&str) -> Result<(), NameError>) {
    for ok in [
        "a.b",
        "org.freedesktop.Notifications",
        "org._7_zip.Plugin",
        "A_.b9",
    ] {
        assert_eq!(parse(ok), Ok(()), "{ok}");
    }
    // 2 or more elements separated by '.'.
    assert_eq!(parse("Notifications"), Err(NameError::OneElement));
    // All elements must contain at least one character.
    assert_eq!(parse("a..b"), Err(NameError::EmptyElement));
    assert_eq!(parse(".a.b"), Err(NameError::EmptyElement));
    assert_eq!(parse("a.b."), Err(NameError::EmptyElement));
    assert_eq!(parse(""), Err(NameError::Empty));
    // Each element must only contain "[A-Z][a-z][0-9]_" ...
    assert_eq!(parse("org.7-zip.Plugin"), Err(NameError::LeadingDigit));
    assert_eq!(parse("org.a-b.Plugin"), Err(NameError::Byte));
    assert_eq!(parse("org.a b"), Err(NameError::Byte));
    // ... and must not begin with a digit.
    assert_eq!(parse("org.7zip"), Err(NameError::LeadingDigit));
    // Must not exceed the maximum name length.
    let at = format!("a.{}", "b".repeat(253));
    assert_eq!(parse(&at), Ok(()));
    let past = format!("a.{}", "b".repeat(254));
    assert_eq!(parse(&past), Err(NameError::TooLong));
}

#[test]
fn interface_names_follow_their_grammar() {
    interface_like(|s| InterfaceName::parse(s).map(|_| ()));
}

#[test]
fn error_names_follow_the_interface_grammar() {
    interface_like(|s| ErrorName::parse(s).map(|_| ()));
}

#[test]
fn member_names_follow_their_grammar() {
    for ok in ["Notify", "a", "_9", "ActionInvoked"] {
        assert_eq!(MemberName::parse(ok).map(|m| m.as_str()), Ok(ok));
    }
    // Only "[A-Z][a-z][0-9]_".
    assert_eq!(
        MemberName::parse("No-tify").map(|_| ()),
        Err(NameError::Byte)
    );
    // May not begin with a digit.
    assert_eq!(
        MemberName::parse("9a").map(|_| ()),
        Err(NameError::LeadingDigit)
    );
    // Must not contain '.'.
    assert_eq!(MemberName::parse("a.b").map(|_| ()), Err(NameError::Period));
    // At least 1 byte.
    assert_eq!(MemberName::parse("").map(|_| ()), Err(NameError::Empty));
    // Not past the maximum name length.
    assert!(MemberName::parse(&"a".repeat(255)).is_ok());
    assert_eq!(
        MemberName::parse(&"a".repeat(256)).map(|_| ()),
        Err(NameError::TooLong)
    );
}

#[test]
fn bus_names_follow_their_grammar() {
    for ok in [
        ":1.4",
        ":1.7",
        ":a.b",
        ":1.2.3",
        "org.freedesktop.Notifications",
        "org.freedesktop.portal.Desktop",
        "a-b.c",
        "org._7_zip.Archiver",
    ] {
        assert_eq!(BusName::parse(ok).map(|b| b.as_str()), Ok(ok), "{ok}");
    }
    // Only "[A-Z][a-z][0-9]_-".
    assert_eq!(BusName::parse("org.a b").map(|_| ()), Err(NameError::Byte));
    assert_eq!(BusName::parse(":1.a:b").map(|_| ()), Err(NameError::Byte));
    // Only a unique name's elements may begin with a digit.
    assert_eq!(
        BusName::parse("org.7zip").map(|_| ()),
        Err(NameError::LeadingDigit)
    );
    // At least one '.', so two elements.
    assert_eq!(
        BusName::parse("org").map(|_| ()),
        Err(NameError::OneElement)
    );
    assert_eq!(BusName::parse(":1").map(|_| ()), Err(NameError::OneElement));
    // Must not begin with '.'; every element at least one character.
    assert_eq!(
        BusName::parse(".org.a").map(|_| ()),
        Err(NameError::EmptyElement)
    );
    assert_eq!(
        BusName::parse(":.1").map(|_| ()),
        Err(NameError::EmptyElement)
    );
    assert_eq!(
        BusName::parse("a..b").map(|_| ()),
        Err(NameError::EmptyElement)
    );
    assert_eq!(BusName::parse("").map(|_| ()), Err(NameError::Empty));
    // Not past the maximum name length, the colon included.
    let at = format!(":1.{}", "2".repeat(252));
    assert!(BusName::parse(&at).is_ok());
    let past = format!(":1.{}", "2".repeat(253));
    assert_eq!(BusName::parse(&past).map(|_| ()), Err(NameError::TooLong));
}

#[test]
fn a_unique_name_says_so() {
    assert_eq!(
        BusName::parse(":1.4").map(|b| b.kind()),
        Ok(BusKind::Unique)
    );
    assert_eq!(
        BusName::parse("org.a").map(|b| b.kind()),
        Ok(BusKind::WellKnown)
    );
}

const NOTIFICATIONS: BusName<'static> = BusName::lit("org.freedesktop.Notifications");
const PATH: ObjectPath<'static> = ObjectPath::lit("/org/freedesktop/Notifications");
const IFACE: InterfaceName<'static> = InterfaceName::lit("org.freedesktop.Notifications");
const NOTIFY: MemberName<'static> = MemberName::lit("Notify");
const FAILED: ErrorName<'static> = ErrorName::lit("org.freedesktop.DBus.Error.Failed");
const TEXT: Str<'static> = Str::lit("baud");

#[test]
fn literals_are_checked_in_const() {
    assert_eq!(NOTIFICATIONS.as_str(), "org.freedesktop.Notifications");
    assert_eq!(PATH.as_str(), "/org/freedesktop/Notifications");
    assert_eq!(IFACE.as_str(), "org.freedesktop.Notifications");
    assert_eq!(NOTIFY.as_str(), "Notify");
    assert_eq!(FAILED.as_str(), "org.freedesktop.DBus.Error.Failed");
    assert_eq!(TEXT.as_str(), "baud");
}
