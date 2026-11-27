//! Strings and the names a message carries, each a newtype over the grammar
//! it follows (`[[source: dbus-spec]]`, "Basic types", "Valid Object Paths"
//! and "Valid Names").
//!
//! Every type here is built only through its check, `parse` at run time or
//! `lit` in a `const`, where a name that breaks its grammar is a compile
//! error. A message holding one of these types holds a name the spec allows
//! it to send.

use core::fmt;

/// The most bytes a message holds, and so the longest string one carries
/// ("Message Format": 2^27).
const MESSAGE_MAX: usize = 1 << 27;

/// The most bytes a bus, interface, error or member name holds ("Valid
/// Names").
const NAME_MAX: usize = 255;

/// Why a string is not the name or string asked for: each variant is one
/// bullet of the grammar it breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameError {
    /// A name with no bytes.
    Empty,
    /// Past the maximum: 255 bytes for a name, the message bound for a
    /// string or an object path.
    TooLong,
    /// A string holding U+0000.
    Nul,
    /// An object path that does not begin with `/`.
    NotAbsolute,
    /// An empty element: two separators in sequence, or one at the start
    /// or the end of a dotted name.
    EmptyElement,
    /// An object path other than `/` ending in `/`.
    TrailingSlash,
    /// A byte outside the grammar's set: `[A-Za-z0-9_]`, with `-` added for
    /// a bus name.
    Byte,
    /// An element beginning with a digit, which only a unique bus name's
    /// elements may.
    LeadingDigit,
    /// A dotted name of one element: interface, error and bus names have
    /// at least two.
    OneElement,
    /// A member name holding `.`.
    Period,
}

impl NameError {
    /// The rule broken, as a sentence; also the panic of a refused `lit`.
    pub const fn rule(self) -> &'static str {
        match self {
            NameError::Empty => "a name is at least one byte",
            NameError::TooLong => "a name is at most 255 bytes, a string at most 2^27",
            NameError::Nul => "a string holds no NUL",
            NameError::NotAbsolute => "an object path begins with '/'",
            NameError::EmptyElement => "no element of a name or path is empty",
            NameError::TrailingSlash => "only the root path ends in '/'",
            NameError::Byte => "a name holds only [A-Za-z0-9_], and '-' in a bus name",
            NameError::LeadingDigit => "only a unique bus name's elements begin with a digit",
            NameError::OneElement => "an interface, error or bus name has two elements or more",
            NameError::Period => "a member name holds no '.'",
        }
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rule())
    }
}

/// Panics, in const evaluation a compile error, with the rule `e` names.
const fn refuse(e: NameError) -> ! {
    panic!("{}", e.rule())
}

/// Whether `c` is in `[A-Za-z0-9_]`.
const fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// A string's rule: no NUL, within the message bound.
const fn check_str(s: &[u8]) -> Result<(), NameError> {
    if s.len() > MESSAGE_MAX {
        return Err(NameError::TooLong);
    }
    let mut i = 0;
    while i < s.len() {
        if s[i] == 0 {
            return Err(NameError::Nul);
        }
        i += 1;
    }
    Ok(())
}

/// "Valid Object Paths": `/`, then elements of `[A-Za-z0-9_]` separated by
/// single slashes, no trailing slash but the root's. Any length the message
/// bound allows.
const fn check_path(s: &[u8]) -> Result<(), NameError> {
    if s.len() > MESSAGE_MAX {
        return Err(NameError::TooLong);
    }
    if s.is_empty() || s[0] != b'/' {
        return Err(NameError::NotAbsolute);
    }
    if s.len() == 1 {
        return Ok(());
    }
    let mut start = Element::Opening;
    let mut i = 1;
    while i < s.len() {
        let c = s[i];
        if c == b'/' {
            if matches!(start, Element::Opening) {
                return Err(NameError::EmptyElement);
            }
            start = Element::Opening;
        } else if word(c) {
            start = Element::Inside;
        } else {
            return Err(NameError::Byte);
        }
        i += 1;
    }
    match start {
        Element::Opening => Err(NameError::TrailingSlash),
        Element::Inside => Ok(()),
    }
}

/// Where a scan is in a dotted name or a path.
#[derive(Clone, Copy)]
enum Element {
    /// At an element's first byte.
    Opening,
    /// Past it.
    Inside,
}

/// The grammar a dotted name's elements follow.
#[derive(Clone, Copy)]
enum Dotted {
    /// Interface and error names: `[A-Za-z0-9_]`, no leading digit.
    Interface,
    /// Well-known bus names: `[A-Za-z0-9_-]`, no leading digit.
    WellKnown,
    /// A unique bus name's elements, after its `:`: `[A-Za-z0-9_-]`, a
    /// leading digit allowed.
    Unique,
}

/// Two or more non-empty elements separated by `.`, each in `grammar`'s set.
const fn check_dotted(s: &[u8], grammar: Dotted) -> Result<(), NameError> {
    let mut start = Element::Opening;
    let mut periods = 0usize;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'.' {
            if matches!(start, Element::Opening) {
                return Err(NameError::EmptyElement);
            }
            periods += 1;
            start = Element::Opening;
            i += 1;
            continue;
        }
        let dash = !matches!(grammar, Dotted::Interface) && c == b'-';
        if !(word(c) || dash) {
            return Err(NameError::Byte);
        }
        if matches!(start, Element::Opening)
            && c.is_ascii_digit()
            && !matches!(grammar, Dotted::Unique)
        {
            return Err(NameError::LeadingDigit);
        }
        start = Element::Inside;
        i += 1;
    }
    if matches!(start, Element::Opening) {
        return Err(NameError::EmptyElement);
    }
    if periods == 0 {
        return Err(NameError::OneElement);
    }
    Ok(())
}

/// A name's length: at least a byte, at most 255.
const fn check_name_len(s: &[u8]) -> Result<(), NameError> {
    if s.is_empty() {
        Err(NameError::Empty)
    } else if s.len() > NAME_MAX {
        Err(NameError::TooLong)
    } else {
        Ok(())
    }
}

/// "Interface names", which "Error names" share.
const fn check_interface(s: &[u8]) -> Result<(), NameError> {
    match check_name_len(s) {
        Ok(()) => check_dotted(s, Dotted::Interface),
        Err(e) => Err(e),
    }
}

/// "Bus names": a unique name's `:` and its elements, or a well-known name.
const fn check_bus(s: &[u8]) -> Result<(), NameError> {
    if let Err(e) = check_name_len(s) {
        return Err(e);
    }
    match s.split_first() {
        Some((b':', rest)) => check_dotted(rest, Dotted::Unique),
        _ => check_dotted(s, Dotted::WellKnown),
    }
}

/// "Member names": `[A-Za-z0-9_]`, no leading digit, no `.`, 1 to 255
/// bytes.
const fn check_member(s: &[u8]) -> Result<(), NameError> {
    if let Err(e) = check_name_len(s) {
        return Err(e);
    }
    if s[0].is_ascii_digit() {
        return Err(NameError::LeadingDigit);
    }
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'.' {
            return Err(NameError::Period);
        }
        if !word(s[i]) {
            return Err(NameError::Byte);
        }
        i += 1;
    }
    Ok(())
}

/// A STRING value: UTF-8, which `&str` already is, with no U+0000, and no
/// longer than a message ("Basic types").
///
/// A string holding NUL does not compile as a literal:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::Str;
/// const S: Str<'static> = Str::lit("a\0b");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{NameError, Str};
/// const S: Str<'static> = Str::lit("ab");
/// assert_eq!(S.as_str(), "ab");
/// assert_eq!(Str::parse("a\0b"), Err(NameError::Nul));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Str<'a>(&'a str);

impl<'a> Str<'a> {
    /// The empty string.
    pub const EMPTY: Str<'static> = Str("");

    /// `s` as a STRING, or the rule it breaks.
    pub const fn parse(s: &'a str) -> Result<Str<'a>, NameError> {
        match check_str(s.as_bytes()) {
            Ok(()) => Ok(Str(s)),
            Err(e) => Err(e),
        }
    }

    /// The text.
    #[inline]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }
}

impl Str<'static> {
    /// `s` as a STRING; one holding NUL panics, which in a `const` is a
    /// compile error.
    pub const fn lit(s: &'static str) -> Str<'static> {
        match Str::parse(s) {
            Ok(v) => v,
            Err(e) => refuse(e),
        }
    }
}

/// An OBJECT_PATH: `/`, then elements of `[A-Za-z0-9_]` joined by single
/// slashes, with no trailing slash but the root's ("Valid Object Paths").
///
/// A path with an empty element does not compile as a literal:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::ObjectPath;
/// const P: ObjectPath<'static> = ObjectPath::lit("/a//b");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{NameError, ObjectPath};
/// const P: ObjectPath<'static> = ObjectPath::lit("/a/b");
/// assert_eq!(P.as_str(), "/a/b");
/// assert_eq!(ObjectPath::parse("/a//b"), Err(NameError::EmptyElement));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectPath<'a>(&'a str);

impl<'a> ObjectPath<'a> {
    /// `s` as an object path, or the rule it breaks.
    pub const fn parse(s: &'a str) -> Result<ObjectPath<'a>, NameError> {
        match check_path(s.as_bytes()) {
            Ok(()) => Ok(ObjectPath(s)),
            Err(e) => Err(e),
        }
    }

    /// The path.
    #[inline]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }
}

impl ObjectPath<'static> {
    /// `s` as an object path; one off the grammar panics, which in a
    /// `const` is a compile error.
    pub const fn lit(s: &'static str) -> ObjectPath<'static> {
        match ObjectPath::parse(s) {
            Ok(v) => v,
            Err(e) => refuse(e),
        }
    }
}

/// An interface name: two or more elements of `[A-Za-z0-9_]` joined by `.`,
/// none empty or beginning with a digit, at most 255 bytes ("Interface
/// names").
///
/// A name of one element does not compile as a literal:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::InterfaceName;
/// const I: InterfaceName<'static> = InterfaceName::lit("Notifications");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::InterfaceName;
/// const I: InterfaceName<'static> = InterfaceName::lit("org.freedesktop.Notifications");
/// assert_eq!(I.as_str(), "org.freedesktop.Notifications");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InterfaceName<'a>(&'a str);

impl<'a> InterfaceName<'a> {
    /// `s` as an interface name, or the rule it breaks.
    pub const fn parse(s: &'a str) -> Result<InterfaceName<'a>, NameError> {
        match check_interface(s.as_bytes()) {
            Ok(()) => Ok(InterfaceName(s)),
            Err(e) => Err(e),
        }
    }

    /// The name.
    #[inline]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }
}

impl InterfaceName<'static> {
    /// `s` as an interface name; one off the grammar panics, which in a
    /// `const` is a compile error.
    pub const fn lit(s: &'static str) -> InterfaceName<'static> {
        match InterfaceName::parse(s) {
            Ok(v) => v,
            Err(e) => refuse(e),
        }
    }
}

/// A member name, a method's or a signal's: `[A-Za-z0-9_]`, not beginning
/// with a digit, no `.`, 1 to 255 bytes ("Member names").
///
/// A dotted member does not compile as a literal:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::MemberName;
/// const M: MemberName<'static> = MemberName::lit("a.b");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{MemberName, NameError};
/// const M: MemberName<'static> = MemberName::lit("Notify");
/// assert_eq!(M.as_str(), "Notify");
/// assert_eq!(MemberName::parse("a.b"), Err(NameError::Period));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MemberName<'a>(&'a str);

impl<'a> MemberName<'a> {
    /// `s` as a member name, or the rule it breaks.
    pub const fn parse(s: &'a str) -> Result<MemberName<'a>, NameError> {
        match check_member(s.as_bytes()) {
            Ok(()) => Ok(MemberName(s)),
            Err(e) => Err(e),
        }
    }

    /// The name.
    #[inline]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }
}

impl MemberName<'static> {
    /// `s` as a member name; one off the grammar panics, which in a `const`
    /// is a compile error.
    pub const fn lit(s: &'static str) -> MemberName<'static> {
        match MemberName::parse(s) {
            Ok(v) => v,
            Err(e) => refuse(e),
        }
    }
}

/// An error name, which follows the interface name's grammar ("Error
/// names").
///
/// An element beginning with a digit does not compile as a literal:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::ErrorName;
/// const E: ErrorName<'static> = ErrorName::lit("org.7zip.Error.Failed");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::ErrorName;
/// const E: ErrorName<'static> = ErrorName::lit("org.freedesktop.DBus.Error.Failed");
/// assert_eq!(E.as_str(), "org.freedesktop.DBus.Error.Failed");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ErrorName<'a>(&'a str);

impl<'a> ErrorName<'a> {
    /// `s` as an error name, or the rule it breaks.
    pub const fn parse(s: &'a str) -> Result<ErrorName<'a>, NameError> {
        match check_interface(s.as_bytes()) {
            Ok(()) => Ok(ErrorName(s)),
            Err(e) => Err(e),
        }
    }

    /// The name.
    #[inline]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }
}

impl ErrorName<'static> {
    /// `s` as an error name; one off the grammar panics, which in a `const`
    /// is a compile error.
    pub const fn lit(s: &'static str) -> ErrorName<'static> {
        match ErrorName::parse(s) {
            Ok(v) => v,
            Err(e) => refuse(e),
        }
    }
}

/// Which kind of bus name: the connection's own, or one it may own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BusKind {
    /// Begins with `:`: the name the bus gives a connection for its
    /// lifetime.
    Unique,
    /// Any other: a name a connection requests and may lose.
    WellKnown,
}

/// A bus name: a unique name, `:` then elements of `[A-Za-z0-9_-]` that may
/// begin with a digit, or a well-known name, whose elements may not; two
/// elements or more joined by `.`, none empty, at most 255 bytes ("Bus
/// names").
///
/// A name with no `.` does not compile as a literal:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::BusName;
/// const B: BusName<'static> = BusName::lit("org");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{BusKind, BusName};
/// const B: BusName<'static> = BusName::lit("org.freedesktop.Notifications");
/// assert_eq!(B.kind(), BusKind::WellKnown);
/// assert_eq!(BusName::parse(":1.4").map(|b| b.kind()), Ok(BusKind::Unique));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BusName<'a>(&'a str);

impl<'a> BusName<'a> {
    /// `s` as a bus name, or the rule it breaks.
    pub const fn parse(s: &'a str) -> Result<BusName<'a>, NameError> {
        match check_bus(s.as_bytes()) {
            Ok(()) => Ok(BusName(s)),
            Err(e) => Err(e),
        }
    }

    /// The name.
    #[inline]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }

    /// Unique or well-known.
    #[inline]
    pub const fn kind(&self) -> BusKind {
        match self.0.as_bytes().first() {
            Some(b':') => BusKind::Unique,
            _ => BusKind::WellKnown,
        }
    }
}

impl BusName<'static> {
    /// `s` as a bus name; one off the grammar panics, which in a `const` is
    /// a compile error.
    pub const fn lit(s: &'static str) -> BusName<'static> {
        match BusName::parse(s) {
            Ok(v) => v,
            Err(e) => refuse(e),
        }
    }
}

/// How many bytes an [`OwnedBusName`] holds, `1..=255`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NameLen(u8);

/// A unique bus name kept past the message it arrived in: its bytes inline
/// and their count.
#[derive(Clone, Copy)]
pub struct OwnedBusName {
    bytes: [u8; NAME_MAX],
    len: NameLen,
}

impl OwnedBusName {
    /// `name` kept, if it is a unique name; a well-known name is `None`.
    pub(crate) const fn unique(name: BusName<'_>) -> Option<OwnedBusName> {
        if !matches!(name.kind(), BusKind::Unique) {
            return None;
        }
        let s = name.0.as_bytes();
        let mut bytes = [0; NAME_MAX];
        let mut i = 0;
        while i < s.len() {
            bytes[i] = s[i];
            i += 1;
        }
        Some(OwnedBusName {
            bytes,
            len: NameLen(s.len() as u8),
        })
    }

    /// The name, borrowed.
    pub fn as_name(&self) -> BusName<'_> {
        match core::str::from_utf8(&self.bytes[..usize::from(self.len.0)]) {
            Ok(s) => BusName(s),
            Err(_) => unreachable!("an OwnedBusName holds a checked bus name"),
        }
    }
}

impl PartialEq for OwnedBusName {
    fn eq(&self, other: &OwnedBusName) -> bool {
        self.as_name() == other.as_name()
    }
}

impl Eq for OwnedBusName {}

impl fmt::Debug for OwnedBusName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OwnedBusName({:?})", self.as_name().as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{BusName, OwnedBusName};

    #[test]
    fn only_a_unique_name_is_kept() {
        let kept = OwnedBusName::unique(BusName::lit(":1.42")).expect("a unique name");
        assert_eq!(kept.as_name().as_str(), ":1.42");
        assert_eq!(
            OwnedBusName::unique(BusName::lit("org.freedesktop.Notifications")),
            None
        );
        let mut text = [b'2'; 255];
        text[..3].copy_from_slice(b":1.");
        let long = core::str::from_utf8(&text).expect("ASCII");
        let long = BusName::parse(long).expect("255 bytes");
        assert_eq!(long.as_str().len(), 255);
        let kept = OwnedBusName::unique(long).expect("a unique name");
        assert_eq!(kept.as_name(), long);
    }
}
