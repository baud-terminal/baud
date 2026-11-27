//! **The wire format**, as `[[source: wayland]]` and `[[source:
//! wayland-backend-wire]]` spell it: every message is a header of two
//! native-endian 32-bit words -- the sender's object id, then the size in
//! bytes in the high sixteen bits and the opcode in the low sixteen --
//! followed by the arguments, each padded to four bytes. A string is its
//! length including the NUL, then the bytes, then padding; an array the
//! same without the NUL; a file descriptor takes no bytes at all and rides
//! beside the message as ancillary data, in order.
//!
//! Nothing here allocates: a request is written into the caller's buffer
//! and an event is read as borrows of the receive buffer, which is the
//! shape a `no_std` core would want even though this crate is not one.

use crate::table::ArgKind;
use core::fmt;
use std::os::fd::{BorrowedFd, OwnedFd};

/// A protocol object id. Zero is the null object; the client allocates
/// from 1 upward and the server from `0xff00_0000`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u32);

impl ObjectId {
    pub const NULL: ObjectId = ObjectId(0);
    /// `wl_display`, the one object that exists before anything is bound.
    pub const DISPLAY: ObjectId = ObjectId(1);
    /// The first id the server allocates; ids below it are the client's.
    pub const SERVER_BASE: ObjectId = ObjectId(0xff00_0000);
    pub fn is_null(self) -> bool {
        self.0 == 0
    }
}

/// A request or event opcode within its interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opcode(pub u16);

/// A 24.8 signed fixed-point number, the protocol's coordinate type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fixed(pub i32);

impl Fixed {
    pub fn from_int(i: i32) -> Fixed {
        Fixed(i << 8)
    }
    pub fn to_f64(self) -> f64 {
        f64::from(self.0) / 256.0
    }
    pub fn from_f64(f: f64) -> Fixed {
        // A coordinate past 2^23 cells is not a coordinate; saturate.
        Fixed(
            (f * 256.0)
                .round()
                .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
        )
    }
}

/// An argument to write: borrows for the strings, arrays and fds.
#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    Int(i32),
    Uint(u32),
    Fixed(Fixed),
    Str(Option<&'a str>),
    Object(ObjectId),
    NewId(ObjectId),
    /// `wl_registry.bind`'s form: the interface name and version travel
    /// before the id.
    NewIdWith {
        interface: &'a str,
        version: u32,
        id: ObjectId,
    },
    Array(&'a [u8]),
    Fd(BorrowedFd<'a>),
}

/// An argument read: borrows of the receive buffer, and an fd owned.
#[derive(Debug)]
pub enum Read<'a> {
    Int(i32),
    Uint(u32),
    Fixed(Fixed),
    Str(Option<&'a str>),
    Object(ObjectId),
    NewId(ObjectId),
    NewIdWith {
        interface: &'a str,
        version: u32,
        id: ObjectId,
    },
    Array(&'a [u8]),
    Fd(OwnedFd),
}

/// The header of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub sender: ObjectId,
    pub opcode: Opcode,
    /// The whole message's size in bytes, header included.
    pub size: usize,
}

pub const HEADER_BYTES: usize = 8;

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    /// The buffer cannot take the message.
    BufferTooSmall,
    /// A message's stated size is shorter than its header or not a multiple of four.
    BadSize,
    /// The arguments end before the signature does.
    Truncated,
    /// A string is not NUL-terminated or not UTF-8.
    BadString,
    /// A null string or object where the signature forbids one.
    NullForbidden,
    /// A message with more `fd` arguments than fds arrived beside it.
    MissingFd,
    /// An argument of the wrong kind for its signature.
    Kind,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for WireError {}

fn pad4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

fn put(buf: &mut [u8], at: &mut usize, word: u32) -> Result<(), WireError> {
    let end = *at + 4;
    if end > buf.len() {
        return Err(WireError::BufferTooSmall);
    }
    buf[*at..end].copy_from_slice(&word.to_ne_bytes());
    *at = end;
    Ok(())
}

fn put_bytes(buf: &mut [u8], at: &mut usize, bytes: &[u8]) -> Result<(), WireError> {
    put(
        buf,
        at,
        u32::try_from(bytes.len()).map_err(|_| WireError::BufferTooSmall)?,
    )?;
    let padded = pad4(bytes.len());
    let end = *at + padded;
    if end > buf.len() {
        return Err(WireError::BufferTooSmall);
    }
    buf[*at..*at + bytes.len()].copy_from_slice(bytes);
    buf[*at + bytes.len()..end].fill(0);
    *at = end;
    Ok(())
}

/// Write a message into `buf` and its fds into `fds`, in order. Returns
/// the bytes written. `sig` is the request's signature; an argument that
/// does not match it is refused rather than written, so a table and a
/// caller cannot disagree in silence.
pub fn write<'a>(
    buf: &mut [u8],
    fds: &mut Vec<BorrowedFd<'a>>,
    sender: ObjectId,
    opcode: Opcode,
    sig: &[ArgKind],
    args: &[Arg<'a>],
) -> Result<usize, WireError> {
    if args.len() != sig.len() {
        return Err(WireError::Kind);
    }
    let mut at = HEADER_BYTES;
    if buf.len() < at {
        return Err(WireError::BufferTooSmall);
    }
    for (kind, arg) in sig.iter().zip(args) {
        match (kind, arg) {
            (ArgKind::Int, Arg::Int(i)) => put(buf, &mut at, u32::from_ne_bytes(i.to_ne_bytes()))?,
            (ArgKind::Uint, Arg::Uint(u)) => put(buf, &mut at, *u)?,
            (ArgKind::Fixed, Arg::Fixed(f)) => {
                put(buf, &mut at, u32::from_ne_bytes(f.0.to_ne_bytes()))?
            }
            (ArgKind::Str { nullable }, Arg::Str(s)) => match s {
                Some(s) => {
                    put(
                        buf,
                        &mut at,
                        u32::try_from(s.len() + 1).map_err(|_| WireError::BufferTooSmall)?,
                    )?;
                    let padded = pad4(s.len() + 1);
                    let end = at + padded;
                    if end > buf.len() {
                        return Err(WireError::BufferTooSmall);
                    }
                    buf[at..at + s.len()].copy_from_slice(s.as_bytes());
                    buf[at + s.len()..end].fill(0);
                    at = end;
                }
                None if *nullable => put(buf, &mut at, 0)?,
                None => return Err(WireError::NullForbidden),
            },
            (ArgKind::Object { nullable }, Arg::Object(o)) => {
                if o.is_null() && !nullable {
                    return Err(WireError::NullForbidden);
                }
                put(buf, &mut at, o.0)?
            }
            (ArgKind::NewId { interface: Some(_) }, Arg::NewId(o)) => put(buf, &mut at, o.0)?,
            (
                ArgKind::NewId { interface: None },
                Arg::NewIdWith {
                    interface,
                    version,
                    id,
                },
            ) => {
                let mut name = [0u8; 256];
                let n = interface.len() + 1;
                if n > name.len() {
                    return Err(WireError::BadString);
                }
                name[..interface.len()].copy_from_slice(interface.as_bytes());
                put_bytes(buf, &mut at, &name[..n])?;
                put(buf, &mut at, *version)?;
                put(buf, &mut at, id.0)?;
            }
            (ArgKind::Array, Arg::Array(a)) => put_bytes(buf, &mut at, a)?,
            (ArgKind::Fd, Arg::Fd(fd)) => fds.push(*fd),
            _ => return Err(WireError::Kind),
        }
    }
    let size = u32::try_from(at).map_err(|_| WireError::BufferTooSmall)?;
    if size > 0xffff {
        return Err(WireError::BufferTooSmall);
    }
    buf[0..4].copy_from_slice(&sender.0.to_ne_bytes());
    buf[4..8].copy_from_slice(&((size << 16) | u32::from(opcode.0)).to_ne_bytes());
    Ok(at)
}

fn word(buf: &[u8], at: &mut usize) -> Result<u32, WireError> {
    let end = *at + 4;
    if end > buf.len() {
        return Err(WireError::Truncated);
    }
    let w = u32::from_ne_bytes([buf[*at], buf[*at + 1], buf[*at + 2], buf[*at + 3]]);
    *at = end;
    Ok(w)
}

fn bytes<'a>(buf: &'a [u8], at: &mut usize) -> Result<&'a [u8], WireError> {
    let n = usize::try_from(word(buf, at)?).map_err(|_| WireError::Truncated)?;
    let end = *at + pad4(n);
    if end > buf.len() {
        return Err(WireError::Truncated);
    }
    let out = &buf[*at..*at + n];
    *at = end;
    Ok(out)
}

/// The string at `at`; `None` for the null string, length 0, which each
/// caller allows or refuses by its argument's kind.
fn string<'a>(buf: &'a [u8], at: &mut usize) -> Result<Option<&'a str>, WireError> {
    let b = bytes(buf, at)?;
    if b.is_empty() {
        return Ok(None);
    }
    match b.split_last() {
        Some((0, body)) => core::str::from_utf8(body)
            .map(Some)
            .map_err(|_| WireError::BadString),
        _ => Err(WireError::BadString),
    }
}

/// The header at the front of `buf`, if a whole one is there.
pub fn header(buf: &[u8]) -> Option<Header> {
    if buf.len() < HEADER_BYTES {
        return None;
    }
    let sender = ObjectId(u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]));
    let w = u32::from_ne_bytes([buf[4], buf[5], buf[6], buf[7]]);
    Some(Header {
        sender,
        opcode: Opcode(u16::try_from(w & 0xffff).expect("sixteen bits")),
        size: usize::try_from(w >> 16).expect("sixteen bits"),
    })
}

/// Read one message's arguments from `body` -- the bytes after its header,
/// exactly `size - 8` of them -- against `sig`, taking fds from the front
/// of `fds` as the signature asks for them. `out` receives the arguments.
pub fn read<'a>(
    body: &'a [u8],
    sig: &[ArgKind],
    fds: &mut std::collections::VecDeque<OwnedFd>,
    out: &mut Vec<Read<'a>>,
) -> Result<(), WireError> {
    if !body.len().is_multiple_of(4) {
        return Err(WireError::BadSize);
    }
    let mut at = 0;
    for kind in sig {
        out.push(match *kind {
            ArgKind::Int => Read::Int(i32::from_ne_bytes(word(body, &mut at)?.to_ne_bytes())),
            ArgKind::Uint => Read::Uint(word(body, &mut at)?),
            ArgKind::Fixed => Read::Fixed(Fixed(i32::from_ne_bytes(
                word(body, &mut at)?.to_ne_bytes(),
            ))),
            ArgKind::Str { nullable } => {
                let s = string(body, &mut at)?;
                if s.is_none() && !nullable {
                    return Err(WireError::NullForbidden);
                }
                Read::Str(s)
            }
            ArgKind::Object { nullable } => {
                let o = ObjectId(word(body, &mut at)?);
                if o.is_null() && !nullable {
                    return Err(WireError::NullForbidden);
                }
                Read::Object(o)
            }
            ArgKind::NewId { interface: Some(_) } => Read::NewId(ObjectId(word(body, &mut at)?)),
            ArgKind::NewId { interface: None } => {
                let interface = string(body, &mut at)?.ok_or(WireError::NullForbidden)?;
                let version = word(body, &mut at)?;
                let id = ObjectId(word(body, &mut at)?);
                Read::NewIdWith {
                    interface,
                    version,
                    id,
                }
            }
            ArgKind::Array => Read::Array(bytes(body, &mut at)?),
            ArgKind::Fd => Read::Fd(fds.pop_front().ok_or(WireError::MissingFd)?),
        });
    }
    if at != body.len() {
        return Err(WireError::Truncated);
    }
    Ok(())
}
