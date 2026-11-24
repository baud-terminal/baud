//! D-Bus, sans I/O: bytes in, typed messages out; typed messages in, bytes
//! out (`[[source: dbus-spec]]`, D-Bus Specification 0.43). The caller owns
//! the socket, its reads and writes, and any file descriptors.
//!
//! Every rule of the protocol the codec relies on is held by a type: a
//! signature by [`Sig`] and [`SigCodes`], a string by [`Str`], and each name
//! by the newtype of its grammar, [`ObjectPath`], [`InterfaceName`],
//! [`MemberName`], [`ErrorName`] and [`BusName`]. None is built except
//! through its check, and a literal that breaks one does not compile.

mod name;
mod sig;

pub use name::{
    BusKind, BusName, ErrorName, InterfaceName, MemberName, NameError, ObjectPath, OwnedBusName,
    Str,
};
pub use sig::{Depth, Sig, SigCodes, SigError, SigLen};
