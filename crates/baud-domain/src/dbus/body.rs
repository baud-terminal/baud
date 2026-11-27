//! A message body and the writer that marshals one (`[[source: dbus-spec]]`,
//! "Byte order and alignment", "Marshalling containers" and "Message
//! Format").
//!
//! A [`Body`] is built only from [`Args`], so its signature is the tuple's
//! constant and the SIGNATURE header field written from it says what the
//! bytes hold. The writer is the only thing that puts bytes down: it
//! aligns from the message's first byte, pads with NUL, measures each
//! array without its padding, and refuses what passes the spec's bounds.
//! Every value is written little-endian, `l`.

#[cfg(feature = "alloc")]
use alloc::vec::Vec;
use core::fmt;

#[cfg(feature = "alloc")]
use super::sig::{Depth, Sig, SigCodes};
#[cfg(feature = "alloc")]
use super::value::{Align, Args, FdCount, FdIndex};

/// The most bytes a message holds, header, padding and body ("Message
/// Format": 2^27).
const MESSAGE_MAX: u32 = 1 << 27;

/// The most bytes an array's elements hold ("Marshalling containers":
/// 2^26).
const ARRAY_MAX: u32 = 1 << 26;

/// A message's length in bytes, or a part of one: at most 2^27.
///
/// Only [`MessageLen::new`] makes one, and it refuses a length past the
/// bound:
///
/// ```compile_fail,E0423
/// use baud_domain::dbus::MessageLen;
/// let _ = MessageLen(1 << 28);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::MessageLen;
/// assert_eq!(MessageLen::new(1 << 27), Some(MessageLen::MAX));
/// assert_eq!(MessageLen::new((1 << 27) + 1), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageLen(u32);

impl MessageLen {
    /// The bound, 2^27.
    pub const MAX: MessageLen = MessageLen(MESSAGE_MAX);

    /// `n` bytes, or `None` past the bound.
    #[inline]
    pub const fn new(n: usize) -> Option<MessageLen> {
        if n > MESSAGE_MAX as usize {
            None
        } else {
            Some(MessageLen(n as u32))
        }
    }

    /// The length.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An array's length in bytes, its elements without the padding before
/// them: at most 2^26.
///
/// Only [`ArrayLen::new`] makes one, and it refuses a length past the
/// bound:
///
/// ```compile_fail,E0423
/// use baud_domain::dbus::ArrayLen;
/// let _ = ArrayLen(1 << 27);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::ArrayLen;
/// assert_eq!(ArrayLen::new(1 << 26), Some(ArrayLen::MAX));
/// assert_eq!(ArrayLen::new((1 << 26) + 1), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArrayLen(u32);

impl ArrayLen {
    /// The bound, 2^26.
    pub const MAX: ArrayLen = ArrayLen(ARRAY_MAX);

    /// `n` bytes, or `None` past the bound.
    #[inline]
    pub const fn new(n: usize) -> Option<ArrayLen> {
        if n > ARRAY_MAX as usize {
            None
        } else {
            Some(ArrayLen(n as u32))
        }
    }

    /// The length.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Why a body or a message was not encoded: each is a bound the spec
/// forbids sending past, and nothing is cut to fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EncodeError {
    /// The message, or the body alone, is past 2^27 bytes.
    TooLong,
    /// An array's elements are past 2^26 bytes.
    ArrayTooLong,
    /// Variants nest the message's containers past 64 deep.
    TooDeep,
}

impl EncodeError {
    /// The bound passed, as a sentence.
    pub const fn rule(self) -> &'static str {
        match self {
            EncodeError::TooLong => "a message is at most 2^27 bytes",
            EncodeError::ArrayTooLong => "an array is at most 2^26 bytes",
            EncodeError::TooDeep => "a message nests at most 64 containers",
        }
    }
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rule())
    }
}

/// The writer every value marshals through: the bytes so far, counted
/// from the first byte of the message (a body starts on 8, so its own
/// first byte keeps every boundary), how deeply containers nest here, and
/// the file descriptors named so far. Its fields and methods are the
/// module's own, so no caller writes a byte the types did not.
///
/// Alignment is not the caller's to choose:
///
/// ```compile_fail,E0624
/// use baud_domain::dbus::{Align, BodyWriter};
/// fn f(w: &mut BodyWriter<'_>) { w.align(Align::Eight) }
/// ```
///
/// and neither are the bytes:
///
/// ```compile_fail,E0616
/// use baud_domain::dbus::BodyWriter;
/// fn f(w: &mut BodyWriter<'_>) { w.buf.push(0) }
/// ```
///
/// The legal path: values, which align themselves.
///
/// ```
/// use baud_domain::dbus::{Body, Byte, Double};
/// let body = Body::of(&(Byte::new(1), Double::new(0.0))).expect("small");
/// assert_eq!(body.bytes(), &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
/// ```
#[cfg(feature = "alloc")]
pub struct BodyWriter<'a> {
    buf: &'a mut Vec<u8>,
    depth: Depth,
    fds: FdCount,
}

#[cfg(feature = "alloc")]
impl<'a> BodyWriter<'a> {
    /// A writer appending to `buf`, whose first byte is a message's first
    /// or on an 8-byte boundary from it.
    pub(super) fn new(buf: &'a mut Vec<u8>) -> BodyWriter<'a> {
        BodyWriter {
            buf,
            depth: Depth::ZERO,
            fds: FdCount::ZERO,
        }
    }

    /// The descriptors the values written so far name.
    pub(super) fn fds(&self) -> FdCount {
        self.fds
    }

    /// NUL to the next `to` boundary: the least padding, never more.
    pub(super) fn align(&mut self, to: Align) {
        let pad = self.buf.len().wrapping_neg() & (to.bytes() - 1);
        self.buf.resize(self.buf.len() + pad, 0);
    }

    /// One byte.
    pub(super) fn byte(&mut self, b: u8) {
        self.buf.push(b);
    }

    /// Bytes as they are, unaligned.
    pub(super) fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    /// A 32-bit word on 4.
    pub(super) fn word(&mut self, v: u32) {
        self.align(Align::Four);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// A STRING or OBJECT_PATH: its length on 4, its bytes, NUL.
    pub(super) fn string(&mut self, s: &str) -> Result<(), EncodeError> {
        let Some(len) = MessageLen::new(s.len()) else {
            return Err(EncodeError::TooLong);
        };
        self.word(len.get());
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
        Ok(())
    }

    /// A SIGNATURE: its length in one byte, its codes, NUL.
    pub(super) fn signature(&mut self, sig: Sig<'_>) {
        self.buf.push(sig.len().get());
        self.buf.extend_from_slice(sig.as_bytes());
        self.buf.push(0);
    }

    /// A UNIX_FD naming `index`, counted.
    pub(super) fn fd(&mut self, index: FdIndex) {
        self.fds = self.fds.covering(index);
        self.word(index.get());
    }

    /// `f` one container deeper, refused past 64.
    fn nest(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<(), EncodeError>,
    ) -> Result<(), EncodeError> {
        let outer = self.depth;
        let Some(inner) = outer.enter(Depth::TOTAL) else {
            return Err(EncodeError::TooDeep);
        };
        self.depth = inner;
        let done = f(self);
        self.depth = outer;
        done
    }

    /// An array: its length word, padding to `element`'s boundary, the
    /// elements `f` writes, and the length of the elements alone written
    /// back.
    pub(super) fn array(
        &mut self,
        element: Align,
        f: impl FnOnce(&mut Self) -> Result<(), EncodeError>,
    ) -> Result<(), EncodeError> {
        self.nest(|w| {
            w.word(0);
            let at = w.buf.len() - 4;
            w.align(element);
            let start = w.buf.len();
            f(w)?;
            let Some(len) = ArrayLen::new(w.buf.len() - start) else {
                return Err(EncodeError::ArrayTooLong);
            };
            if MessageLen::new(w.buf.len()).is_none() {
                return Err(EncodeError::TooLong);
            }
            w.buf[at..at + 4].copy_from_slice(&len.get().to_le_bytes());
            Ok(())
        })
    }

    /// A struct: on 8, then the fields `f` writes.
    pub(super) fn structure(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<(), EncodeError>,
    ) -> Result<(), EncodeError> {
        self.nest(|w| {
            w.align(Align::Eight);
            f(w)
        })
    }

    /// A variant: `sig`, then the one value of it `f` writes.
    pub(super) fn variant(
        &mut self,
        sig: Sig<'_>,
        f: impl FnOnce(&mut Self) -> Result<(), EncodeError>,
    ) -> Result<(), EncodeError> {
        self.nest(|w| {
            w.signature(sig);
            f(w)
        })
    }
}

/// A message body: its bytes, the signature they hold and how many file
/// descriptors they name. Its fields are private and [`Body::of`] is the
/// only way to fill them, so the signature a message's header carries is
/// always its body's own ("Message Format", "Header Fields").
///
/// A body cannot be assembled with a signature of the caller's choosing:
///
/// ```compile_fail,E0451
/// use baud_domain::dbus::{Body, FdCount, SigCodes};
/// let _ = Body { bytes: vec![], sig: SigCodes::EMPTY, fds: FdCount::ZERO };
/// ```
///
/// and a bare `&str`, unchecked for NUL, is not a value:
///
/// ```compile_fail,E0277
/// use baud_domain::dbus::Body;
/// let _ = Body::of(&("text",));
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Body, Sig, Str};
/// let body = Body::of(&(Str::lit("text"),)).expect("small");
/// assert_eq!(body.sig(), Sig::lit("s"));
/// assert_eq!(body.bytes(), b"\x04\0\0\0text\0");
/// ```
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    bytes: Vec<u8>,
    sig: SigCodes,
    fds: FdCount,
}

#[cfg(feature = "alloc")]
impl Body {
    /// No arguments: no bytes, the empty signature.
    pub const EMPTY: Body = Body {
        bytes: Vec::new(),
        sig: SigCodes::EMPTY,
        fds: FdCount::ZERO,
    };

    /// `args` marshalled, or the bound they pass.
    pub fn of<A: Args>(args: &A) -> Result<Body, EncodeError> {
        let mut bytes = Vec::new();
        let mut w = BodyWriter::new(&mut bytes);
        args.marshal_all(&mut w)?;
        let fds = w.fds();
        if MessageLen::new(bytes.len()).is_none() {
            return Err(EncodeError::TooLong);
        }
        Ok(Body {
            bytes,
            sig: A::SIG,
            fds,
        })
    }

    /// The signature the bytes hold.
    #[inline]
    pub fn sig(&self) -> Sig<'_> {
        self.sig.as_sig()
    }

    /// How many file descriptors the bytes name.
    #[inline]
    pub fn fds(&self) -> FdCount {
        self.fds
    }

    /// The bytes.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
