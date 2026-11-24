//! D-Bus, sans I/O: bytes in, typed messages out; typed messages in, bytes
//! out (`[[source: dbus-spec]]`, D-Bus Specification 0.43). The caller owns
//! the socket, its reads and writes, and any file descriptors.
//!
//! Every rule of the protocol the codec relies on is held by a type: a
//! signature by [`Sig`] and [`SigCodes`], a string by [`Str`], and each name
//! by the newtype of its grammar, [`ObjectPath`], [`InterfaceName`],
//! [`MemberName`], [`ErrorName`] and [`BusName`]. None is built except
//! through its check, and a literal that breaks one does not compile.
//!
//! Encoding goes the same way. A value type names its signature and
//! alignment as constants ([`Marshal`]), a tuple of them is a body's
//! arguments ([`Args`]), and a `Body` built from them carries their
//! signature, which a [`MethodCall`] or [`Emit`] writes as its SIGNATURE
//! field; each header field's code fixes its value's type
//! ([`HeaderField`]), and the serial is drawn from the connection's
//! [`Serials`] as the message is encoded. Bodies and messages are built on
//! the allocator, behind the `alloc` feature.
//!
//! Decoding is the same rules read back. [`frame`] says how long the
//! message at the start of the received bytes is; [`decode`] checks one
//! whole message, in either byte order, and yields it by its type
//! ([`Inbound`]), each type's required fields held without `Option`; a
//! body reads only as the [`Args`] whose signature is its own
//! ([`BodyRef::read`]). Nothing here allocates.
//!
//! Before any message, the [`Handshake`] runs the authentication protocol
//! as typestate, each state with only its next step, and begins a
//! [`Connection`]: its [`Serials`], whether file descriptors pass on it
//! ([`FdsAgreed`], the one maker of a [`UnixFd`]) and its unique name,
//! kept from `Hello`'s reply. A reply finds the call it answers in an
//! [`Awaiting`] in one slot read, and a signal is acted on only as
//! [`Trusted`], vouched for by the [`Peer`] that owns the name it speaks
//! for. The bus's own signals are [`FromBus`], and its `NameOwnerChanged`
//! says which connection holds a watched name ([`HolderChange`]).

mod auth;
mod awaiting;
mod body;
mod bus;
mod decode;
mod header;
mod name;
mod sig;
mod value;

pub use auth::{
    AuthLine, AuthSent, AuthStep, Authed, Begun, Connection, FdAnswer, FdsAgreed, Handshake,
    LineIn, Negotiating, NoFds, Refusal, Uid, auth_line,
};
pub use awaiting::{Awaiting, Refusals, Refused};
pub use body::{ArrayLen, EncodeError, MessageLen};
#[cfg(feature = "alloc")]
pub use body::{Body, BodyWriter};
pub use bus::{FromBus, HolderChange};
pub use decode::{
    ArrayRead, BodyError, BodyReader, BodyRef, Called, ErrorReply, Frame, Header, Inbound,
    Malformed, Order, Peer, Return, Signal, Trusted, Unread, decode, frame,
};
#[cfg(feature = "alloc")]
pub use header::Encoded;
pub use header::{
    Emit, FieldCode, Flags, HELLO, HeaderField, MessageKind, MethodCall, Serial, Serials,
};
pub use name::{
    BusKind, BusName, ErrorName, InterfaceName, MemberName, NameError, ObjectPath, OwnedBusName,
    Str,
};
pub use sig::{Depth, Sig, SigCodes, SigError, SigLen};
pub use value::{
    Align, Args, Array, Basic, Boolean, Byte, Bytes, Dict, Double, FdCount, FdIndex, FlatPairs,
    I32, ImageData, Marshal, NotifyArgs, Struct, U32, UnixFd, Variant,
};
