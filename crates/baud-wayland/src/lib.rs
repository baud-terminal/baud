//! **The Wayland protocol, spoken directly over the socket.**
//!
//! `[[source: wayland]]` is the protocol; `[[source: wayland-backend-wire]]`
//! and `[[source: wayland-backend-socket]]` are the reference for the wire
//! and the socket, read and not linked -- there is no `libwayland-client`
//! here, which is the register's D13: the presentation layer speaks the
//! platform protocol itself, and the dependency floor on Linux is the
//! kernel's socket and, for the GPU path, `libvulkan`.
//!
//! Three layers, each testable without a compositor: the interface tables
//! (`protocol`, generated at build time from the pinned XMLs), the wire
//! codec (`wire`), and the connection (`client`) -- the socket, the object
//! ids, and the dispatch of events to whoever owns the object. The window
//! a terminal puts on those is `baud-present-wayland`, and the gate that
//! admits it is the same app on a Linux box; this crate's tests drive it
//! against a fake compositor over a socket pair, which runs anywhere.

#![deny(unsafe_code)]

pub mod client;
pub mod sys;
pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/protocol.rs"));
}
pub mod table;
pub mod wire;

pub use client::{Connection, Event, Handler, Sender};
pub use table::{ArgKind, EventKey, Interface, InterfaceIndex, Msg};
pub use wire::{Arg, Fixed, Header, ObjectId, Opcode, Read, WireError};
