//! The notification server's calls (`[[source: freedesktop-notification-spec]]`):
//! one `Notify` as its arguments ([`FdoNotify`]), encoded with its serial
//! drawn from the connection's counter, and the calls around it --
//! [`CLOSE_NOTIFICATION`], [`GET_CAPABILITIES`] and the bus's
//! [`ADD_MATCH`] for the server's signals -- each a [`MethodCall`] whose
//! every field is a checked name.

#[cfg(feature = "alloc")]
use super::body::{Body, EncodeError};
use super::header::MethodCall;
#[cfg(feature = "alloc")]
use super::header::{Encoded, Serials};
use super::name::{BusName, InterfaceName, MemberName, ObjectPath, Str};
use super::value::Variant;
#[cfg(feature = "alloc")]
use super::value::{Dict, FlatPairs, I32, NotifyArgs, U32};

/// `org.freedesktop.Notifications.Notify`, on the server's object.
pub const NOTIFY: MethodCall<'static> = MethodCall {
    destination: BusName::lit("org.freedesktop.Notifications"),
    path: ObjectPath::lit("/org/freedesktop/Notifications"),
    interface: InterfaceName::lit("org.freedesktop.Notifications"),
    member: MemberName::lit("Notify"),
};

/// `CloseNotification`, its body the id, `u`.
pub const CLOSE_NOTIFICATION: MethodCall<'static> = MethodCall {
    member: MemberName::lit("CloseNotification"),
    ..NOTIFY
};

/// `GetCapabilities`, with no body.
pub const GET_CAPABILITIES: MethodCall<'static> = MethodCall {
    member: MemberName::lit("GetCapabilities"),
    ..NOTIFY
};

/// The bus's `AddMatch`, its body the rule, `s` (`[[source: dbus-spec]]`,
/// "Method: org.freedesktop.DBus.AddMatch").
pub const ADD_MATCH: MethodCall<'static> = MethodCall {
    destination: BusName::lit("org.freedesktop.DBus"),
    path: ObjectPath::lit("/org/freedesktop/DBus"),
    interface: InterfaceName::lit("org.freedesktop.DBus"),
    member: MemberName::lit("AddMatch"),
};

/// **One `Notify`'s arguments, in the order the method takes them**:
/// "app_name", "replaces_id" (0 for a new notification), "app_icon",
/// "summary", "body", "actions" as identifier and label pairs, "hints",
/// and "expire_timeout" in milliseconds, -1 for the server's and 0 for
/// never. Every text is a checked [`Str`], so no string reaches the body
/// unchecked:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::{FdoNotify, Str};
/// let n = FdoNotify {
///     app_name: Str::EMPTY,
///     replaces: 0,
///     app_icon: Str::EMPTY,
///     summary: Str::lit("Build"),
///     body: Str::EMPTY,
///     actions: &[],
///     hints: &[],
///     expire: -1,
/// };
/// let _ = FdoNotify { summary: "Build", ..n };
/// ```
///
/// The legal path, its signature the method's:
///
/// ```
/// use baud_domain::dbus::{FdoNotify, Serials, Str};
/// let n = FdoNotify {
///     app_name: Str::lit("baud"),
///     replaces: 0,
///     app_icon: Str::EMPTY,
///     summary: Str::parse("Build").expect("no NUL"),
///     body: Str::EMPTY,
///     actions: &[(Str::lit("default"), Str::lit("Open"))],
///     hints: &[],
///     expire: -1,
/// };
/// let mut serials = Serials::new();
/// let e = n.encode(&mut serials).expect("within the bound");
/// assert_eq!(e.serial().get(), 2);
/// let sig = b"\x08\x01g\x00\x0dsusssasa{sv}i\x00";
/// assert!(e.bytes().windows(sig.len()).any(|w| w == sig));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FdoNotify<'a> {
    /// `app_name`.
    pub app_name: Str<'a>,
    /// `replaces_id`: the server's id of the notification this replaces,
    /// or 0.
    pub replaces: u32,
    /// `app_icon`, empty for none.
    pub app_icon: Str<'a>,
    /// `summary`.
    pub summary: Str<'a>,
    /// `body`.
    pub body: Str<'a>,
    /// `actions`, each identifier with its label.
    pub actions: &'a [(Str<'a>, Str<'a>)],
    /// `hints`, each name with its value.
    pub hints: &'a [(Str<'a>, Variant<'a>)],
    /// `expire_timeout`.
    pub expire: i32,
}

#[cfg(feature = "alloc")]
impl FdoNotify<'_> {
    /// `Notify`'s message, its serial drawn from `serials` once it is
    /// known to fit.
    pub fn encode(&self, serials: &mut Serials) -> Result<Encoded, EncodeError> {
        let args: NotifyArgs<'_> = (
            self.app_name,
            U32::new(self.replaces),
            self.app_icon,
            self.summary,
            self.body,
            FlatPairs::new(self.actions),
            Dict::<Str, Variant>::new(self.hints),
            I32::new(self.expire),
        );
        NOTIFY.encode(&Body::of(&args)?, serials)
    }
}
