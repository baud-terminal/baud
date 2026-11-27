//! Signals from the bus itself, and the one of them that says which
//! connection holds a name: `NameOwnerChanged` (`[[source: dbus-spec]]`,
//! "Message Bus Overview", "Header Fields" and "Signal:
//! org.freedesktop.DBus.NameOwnerChanged").

use super::decode::{Peer, Signal};
use super::name::{BusName, InterfaceName, MemberName, ObjectPath, Str};

/// The bus's own name.
const BUS: BusName<'static> = BusName::lit("org.freedesktop.DBus");
/// The object the bus emits its signals from.
const BUS_PATH: ObjectPath<'static> = ObjectPath::lit("/org/freedesktop/DBus");
/// The interface the bus emits its signals on.
const BUS_INTERFACE: InterfaceName<'static> = InterfaceName::lit("org.freedesktop.DBus");
/// The signal that a name has passed to another connection, or to none.
const NAME_HOLDER_CHANGED: MemberName<'static> = MemberName::lit("NameOwnerChanged");

/// A signal the bus itself sent: its SENDER is the bus's own name, and it
/// comes from the bus's object on the bus's interface. "The bus itself
/// owns a special name, org.freedesktop.DBus, with an object located at
/// /org/freedesktop/DBus that implements the org.freedesktop.DBus
/// interface" (`[[source: dbus-spec]]`, "Message Bus Overview"); "That
/// object path is also used when emitting signals such as
/// NameOwnerChanged" ("Message Bus Interface: org.freedesktop.DBus"); and
/// SENDER "is controlled by the message bus, so it is as reliable and
/// trustworthy as the message bus itself" ("Header Fields"). A client's
/// SENDER is its unique name, set by the bus, so no client's signal is
/// taken for the bus's.
///
/// A signal is not passed where one from the bus is asked for:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::{FromBus, Signal};
/// fn watch(_: &FromBus<'_>) {}
/// fn f(signal: Signal<'_>) { watch(&signal) }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{FromBus, Signal};
/// fn watch(_: &FromBus<'_>) {}
/// fn f(signal: Signal<'_>) {
///     if let Some(bus) = signal.from_bus() {
///         watch(&bus);
///     }
/// }
/// ```
///
/// and is not made the bus's by naming it so:
///
/// ```compile_fail,E0451
/// use baud_domain::dbus::{FromBus, Signal};
/// fn f(signal: Signal<'_>) { let _ = FromBus { signal }; }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{FromBus, Signal};
/// fn f(signal: Signal<'_>) { let _: Option<FromBus<'_>> = signal.from_bus(); }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FromBus<'a> {
    signal: Signal<'a>,
}

impl<'a> FromBus<'a> {
    /// The signal.
    #[inline]
    pub const fn signal(&self) -> Signal<'a> {
        self.signal
    }
}

impl<'a> Signal<'a> {
    /// This signal as the bus's own, if its SENDER is `org.freedesktop.DBus`
    /// and it comes from `/org/freedesktop/DBus` on the interface
    /// `org.freedesktop.DBus`: `None` for any other sender, none named, or
    /// another object or interface.
    pub fn from_bus(&self) -> Option<FromBus<'a>> {
        if self.header().sender()? != BUS
            || self.path != BUS_PATH
            || self.interface != BUS_INTERFACE
        {
            return None;
        }
        Some(FromBus { signal: *self })
    }
}

/// Which connection holds a watched name after the bus's
/// `NameOwnerChanged`.
#[expect(
    clippy::large_enum_variant,
    reason = "a `Peer` holds its name inline, as `Option<Peer>` does, and the crate builds without an allocator to box it"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HolderChange {
    /// The connection with this unique name holds it now.
    To(Peer),
    /// No connection holds it.
    Gone,
}

impl HolderChange {
    /// What `bus` says of `watched`, if it is `NameOwnerChanged` about
    /// that name. Its body is three STRINGs: the name, the connection that
    /// held it and the one that holds it now, each empty for none
    /// (`[[source: dbus-spec]]`, "Signal:
    /// org.freedesktop.DBus.NameOwnerChanged"). An empty third is
    /// [`HolderChange::Gone`] and a unique name is [`HolderChange::To`].
    /// `None` for another member, another name, a body that is not `sss`,
    /// or a third that is not empty and not a unique name.
    pub fn of(bus: &FromBus<'_>, watched: BusName<'_>) -> Option<HolderChange> {
        let signal = bus.signal;
        if signal.member != NAME_HOLDER_CHANGED {
            return None;
        }
        let (name, _, now) = signal
            .body()
            .read::<(Str<'static>, Str<'static>, Str<'static>)>()
            .ok()?;
        if name.as_str() != watched.as_str() {
            return None;
        }
        if now.as_str().is_empty() {
            return Some(HolderChange::Gone);
        }
        let now = BusName::parse(now.as_str()).ok()?;
        Peer::unique(now).map(HolderChange::To)
    }
}
