//! Received messages: how long one is, whether it keeps every rule, and
//! its values (`[[source: dbus-spec]]`, "Message Format", "Header Fields",
//! "Message Types", "Byte order and alignment", "Marshalling basic types",
//! "Marshalling containers" and "Invalid Protocol and Spec Extensions").
//!
//! [`frame`] reads a message's length from its first 16 bytes. [`decode`]
//! checks one whole message, header, padding and body against its
//! signature, before it hands out anything, so every value a [`BodyRef`]
//! reads has been checked. A message that breaks a rule is refused with
//! the rule it broke, [`Malformed`], and the connection it came on is
//! closed, as "Invalid Protocol and Spec Extensions" asks. The extension
//! points are passed over: unknown flags, header field codes and message
//! types, each still checked for form.
//!
//! Both byte orders are read: each message in its own.

use core::fmt;
use core::marker::PhantomData;

use super::body::{ArrayLen, MessageLen};
use super::header::{FieldCode, Flags, MessageKind, Serial, VERSION};
use super::name::{
    BusName, ErrorName, InterfaceName, MemberName, NameError, ObjectPath, OwnedBusName, Str,
};
use super::sig::{Depth, Sig, SigError, single};
use super::value::{Align, Args, Boolean, FdCount, FdIndex, Marshal};

/// The fixed part of a header: the four bytes, the body's length, the
/// serial and the field array's length.
const FIXED: usize = 16;

/// A message's byte order, its first header byte: `l` or `B`. Both the
/// header and the body are in it ("Message Format").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Order {
    /// `l`: least significant byte first.
    Little,
    /// `B`: most significant byte first.
    Big,
}

impl Order {
    /// The order a first header byte names.
    const fn of(b: u8) -> Option<Order> {
        match b {
            b'l' => Some(Order::Little),
            b'B' => Some(Order::Big),
            _ => None,
        }
    }

    const fn half(self, b: [u8; 2]) -> u16 {
        match self {
            Order::Little => u16::from_le_bytes(b),
            Order::Big => u16::from_be_bytes(b),
        }
    }

    const fn word(self, b: [u8; 4]) -> u32 {
        match self {
            Order::Little => u32::from_le_bytes(b),
            Order::Big => u32::from_be_bytes(b),
        }
    }

    const fn quad(self, b: [u8; 8]) -> u64 {
        match self {
            Order::Little => u64::from_le_bytes(b),
            Order::Big => u64::from_be_bytes(b),
        }
    }
}

/// The rule a received message breaks. Each is a "must" of the spec, and
/// a message that breaks one is not read further.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Malformed {
    /// The first byte is neither `l` nor `B`.
    Endian,
    /// The major protocol version is not 1.
    Version,
    /// The message type is 0, INVALID.
    Kind,
    /// The serial, or a REPLY_SERIAL, is 0.
    ZeroSerial,
    /// The message is past 2^27 bytes.
    TooLong,
    /// An array, the header's fields included, is past 2^26 bytes.
    ArrayTooLong,
    /// The bytes given are not exactly one message: fewer, or more.
    Length,
    /// A header field with code 0, INVALID.
    InvalidField,
    /// A header field given twice.
    Repeated(FieldCode),
    /// A known header field whose value has another type than its code's.
    FieldType(FieldCode),
    /// A field the message's type requires is not there.
    Missing(FieldCode),
    /// Alignment padding that is not NUL.
    Padding,
    /// A BOOLEAN other than 0 or 1.
    Boolean,
    /// A string holding a NUL.
    Nul,
    /// A string or signature not followed by its NUL.
    Terminator,
    /// A string that is not UTF-8.
    Utf8,
    /// An object path or name that breaks its grammar.
    Name(NameError),
    /// A signature that breaks a rule of "Valid Signatures".
    Sig(SigError),
    /// A variant's signature that is not one complete type.
    NotSingle,
    /// Containers nested past 64.
    TooDeep,
    /// A value that runs past its array, the header's fields or the
    /// message.
    Overrun,
    /// Bytes after the last value the body's signature names.
    Trailing,
    /// A UNIX_FD index not below the message's UNIX_FDS.
    FdIndex,
    /// A reply whose body is not its method's out arguments.
    Reply,
    /// `Hello`'s reply naming a well-known name, not a unique one.
    NotUnique,
}

impl Malformed {
    /// The rule broken, as a sentence.
    pub const fn rule(self) -> &'static str {
        match self {
            Malformed::Endian => "a message's first byte is 'l' or 'B'",
            Malformed::Version => "a message's major protocol version is 1",
            Malformed::Kind => "a message's type is not 0",
            Malformed::ZeroSerial => "a serial is not 0",
            Malformed::TooLong => "a message is at most 2^27 bytes",
            Malformed::ArrayTooLong => "an array is at most 2^26 bytes",
            Malformed::Length => "a message is exactly its header's lengths",
            Malformed::InvalidField => "a header field's code is not 0",
            Malformed::Repeated(_) => "a header field appears once",
            Malformed::FieldType(_) => "a header field's value has its code's type",
            Malformed::Missing(_) => "a message has the fields its type requires",
            Malformed::Padding => "alignment padding is NUL",
            Malformed::Boolean => "a BOOLEAN is 0 or 1",
            Malformed::Nul => "a string holds no NUL",
            Malformed::Terminator => "a string or signature ends in NUL",
            Malformed::Utf8 => "a string is UTF-8",
            Malformed::Name(e) => e.rule(),
            Malformed::Sig(e) => e.rule(),
            Malformed::NotSingle => "a variant holds one complete type",
            Malformed::TooDeep => "a message nests at most 64 containers",
            Malformed::Overrun => "a value ends inside its array and its message",
            Malformed::Trailing => "a body is exactly the values its signature names",
            Malformed::FdIndex => "a UNIX_FD names one of the message's descriptors",
            Malformed::Reply => "a reply's body has its method's signature",
            Malformed::NotUnique => "Hello's reply is a unique name, beginning with ':'",
        }
    }
}

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rule())
    }
}

/// What the bytes received so far hold: not yet a whole message, one whole
/// message of a length, or the start of one that breaks a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frame {
    /// The bytes end before the message does.
    NeedMore,
    /// The first message is this many bytes, all of them here.
    Whole(MessageLen),
    /// The fixed header breaks a rule, so no length can be trusted.
    Malformed(Malformed),
}

/// How long the message at the start of `bytes` is, from its fixed header:
/// the header padded to 8 and then the body ("Message Format"). The byte
/// order, the version and both bounds are checked here, so a length past
/// 2^27 is refused before its bytes are waited for:
///
/// ```
/// use baud_domain::dbus::{frame, Frame, Malformed};
/// let mut head = vec![b'l', 4, 0, 1];
/// head.extend_from_slice(&((1u32 << 27) - 16 + 1).to_le_bytes());
/// head.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]);
/// assert_eq!(frame(&head), Frame::Malformed(Malformed::TooLong));
/// head[4..8].copy_from_slice(&((1u32 << 27) - 16).to_le_bytes());
/// assert_eq!(frame(&head), Frame::NeedMore);
/// ```
pub fn frame(bytes: &[u8]) -> Frame {
    let Some(&first) = bytes.first() else {
        return Frame::NeedMore;
    };
    let Some(order) = Order::of(first) else {
        return Frame::Malformed(Malformed::Endian);
    };
    let Some(&version) = bytes.get(3) else {
        return Frame::NeedMore;
    };
    if version != VERSION {
        return Frame::Malformed(Malformed::Version);
    }
    let (Some(body), Some(fields)) = (word_at(bytes, 4, order), word_at(bytes, 12, order)) else {
        return Frame::NeedMore;
    };
    let Some(fields) = usize::try_from(fields).ok().and_then(ArrayLen::new) else {
        return Frame::Malformed(Malformed::ArrayTooLong);
    };
    let header = (FIXED + fields.get() as usize).next_multiple_of(Align::Eight.bytes());
    let total = usize::try_from(body)
        .ok()
        .and_then(|b| header.checked_add(b))
        .and_then(MessageLen::new);
    match total {
        None => Frame::Malformed(Malformed::TooLong),
        Some(len) if bytes.len() < len.get() as usize => Frame::NeedMore,
        Some(len) => Frame::Whole(len),
    }
}

/// The word at `at` in `order`, if the bytes reach past it.
fn word_at(bytes: &[u8], at: usize, order: Order) -> Option<u32> {
    match bytes.get(at..at + 4) {
        Some(&[a, b, c, d]) => Some(order.word([a, b, c, d])),
        _ => None,
    }
}

/// Where a reader is: a byte offset from the message's first byte, or
/// from the body's, which is on 8 from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct At(usize);

/// The reader every received value is taken through: the bytes, where it
/// is, their order, how many descriptors came with them and how deeply
/// containers nest here. Its fields and methods are the module's own, so
/// nothing reads a value except as its type says.
#[derive(Clone, Debug)]
pub struct BodyReader<'a> {
    bytes: &'a [u8],
    at: At,
    order: Order,
    fds: FdCount,
    depth: Depth,
}

impl<'a> BodyReader<'a> {
    /// A reader at the start of `bytes`, whose first byte is a message's
    /// first or on an 8-byte boundary from it.
    const fn new(bytes: &'a [u8], order: Order, fds: FdCount) -> BodyReader<'a> {
        BodyReader {
            bytes,
            at: At(0),
            order,
            fds,
            depth: Depth::ZERO,
        }
    }

    /// The next `n` bytes.
    fn take(&mut self, n: usize) -> Result<&'a [u8], Malformed> {
        let end = self.at.0.checked_add(n).ok_or(Malformed::Overrun)?;
        let taken = self.bytes.get(self.at.0..end).ok_or(Malformed::Overrun)?;
        self.at = At(end);
        Ok(taken)
    }

    fn array_of<const N: usize>(&mut self) -> Result<[u8; N], Malformed> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    /// Past the padding to the next multiple of `to` bytes, which must be
    /// NUL.
    fn pad(&mut self, to: usize) -> Result<(), Malformed> {
        let n = self.at.0.wrapping_neg() & (to - 1);
        if self.take(n)?.iter().any(|&b| b != 0) {
            return Err(Malformed::Padding);
        }
        Ok(())
    }

    /// Past the padding to `to`.
    pub(super) fn align(&mut self, to: Align) -> Result<(), Malformed> {
        self.pad(to.bytes())
    }

    /// A BYTE.
    pub(super) fn byte(&mut self) -> Result<u8, Malformed> {
        let [b] = self.array_of()?;
        Ok(b)
    }

    /// A 16-bit value on 2.
    fn half(&mut self) -> Result<u16, Malformed> {
        self.pad(2)?;
        Ok(self.order.half(self.array_of()?))
    }

    /// A 32-bit word on 4.
    pub(super) fn word(&mut self) -> Result<u32, Malformed> {
        self.align(Align::Four)?;
        Ok(self.order.word(self.array_of()?))
    }

    /// A 64-bit value on 8.
    pub(super) fn quad(&mut self) -> Result<u64, Malformed> {
        self.align(Align::Eight)?;
        Ok(self.order.quad(self.array_of()?))
    }

    /// A BOOLEAN: 0 or 1, and no other word.
    pub(super) fn boolean(&mut self) -> Result<Boolean, Malformed> {
        match self.word()? {
            0 => Ok(Boolean::False),
            1 => Ok(Boolean::True),
            _ => Err(Malformed::Boolean),
        }
    }

    /// A STRING's text: its length, that many bytes of UTF-8 with no NUL,
    /// then NUL.
    fn text(&mut self) -> Result<&'a str, Malformed> {
        let n = usize::try_from(self.word()?).map_err(|_| Malformed::TooLong)?;
        let text = self.take(n)?;
        if self.byte()? != 0 {
            return Err(Malformed::Terminator);
        }
        if text.contains(&0) {
            return Err(Malformed::Nul);
        }
        core::str::from_utf8(text).map_err(|_| Malformed::Utf8)
    }

    /// A STRING.
    pub(super) fn str(&mut self) -> Result<Str<'a>, Malformed> {
        Str::parse(self.text()?).map_err(Malformed::Name)
    }

    /// An OBJECT_PATH.
    pub(super) fn path(&mut self) -> Result<ObjectPath<'a>, Malformed> {
        ObjectPath::parse(self.text()?).map_err(Malformed::Name)
    }

    /// A SIGNATURE: its length in one byte, its codes, NUL.
    pub(super) fn signature(&mut self) -> Result<Sig<'a>, Malformed> {
        let n = self.byte()?;
        let codes = self.take(usize::from(n))?;
        if self.byte()? != 0 {
            return Err(Malformed::Terminator);
        }
        Sig::parse(codes).map_err(Malformed::Sig)
    }

    /// A UNIX_FD: an index below the message's UNIX_FDS.
    pub(super) fn fd(&mut self) -> Result<FdIndex, Malformed> {
        let index = self.word()?;
        if index >= self.fds.get() {
            return Err(Malformed::FdIndex);
        }
        Ok(FdIndex::new(index))
    }

    /// An array's length and the padding to its element's boundary; where
    /// its elements end.
    fn array(&mut self, element: usize) -> Result<At, Malformed> {
        let n = usize::try_from(self.word()?)
            .ok()
            .and_then(ArrayLen::new)
            .ok_or(Malformed::ArrayTooLong)?;
        self.pad(element)?;
        let end = self
            .at
            .0
            .checked_add(n.get() as usize)
            .ok_or(Malformed::Overrun)?;
        if end > self.bytes.len() {
            return Err(Malformed::Overrun);
        }
        Ok(At(end))
    }

    /// An ARRAY of BYTE's bytes.
    pub(super) fn bytes(&mut self) -> Result<&'a [u8], Malformed> {
        let end = self.array(1)?;
        self.take(end.0 - self.at.0)
    }

    /// `f` one container deeper, refused past 64.
    fn nest<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, Malformed>,
    ) -> Result<T, Malformed> {
        let outer = self.depth;
        self.depth = outer.enter(Depth::TOTAL).ok_or(Malformed::TooDeep)?;
        let done = f(self);
        self.depth = outer;
        done
    }

    /// One value of `sig`, a single complete type, passed over.
    pub(super) fn unread(&mut self, sig: Sig<'_>) -> Result<Unread<'a>, Malformed> {
        let start = self.at;
        walk(self, sig.as_bytes(), 0)?;
        Ok(Unread(&self.bytes[start.0..self.at.0]))
    }
}

/// The boundary a value of the type whose code is `c` starts on.
const fn boundary(c: u8) -> usize {
    match c {
        b'n' | b'q' => 2,
        b'b' | b'i' | b'u' | b'h' | b's' | b'o' | b'a' => 4,
        b'x' | b't' | b'd' | b'(' | b'{' => 8,
        _ => 1,
    }
}

/// Checks one value of the single complete type at `sig[i]` and passes
/// it; the index just past that type.
fn walk(r: &mut BodyReader<'_>, sig: &[u8], i: usize) -> Result<usize, Malformed> {
    let Some(&c) = sig.get(i) else {
        return Err(Malformed::Sig(SigError::ArrayNoElement));
    };
    match c {
        b'y' => {
            r.byte()?;
        }
        b'b' => {
            r.boolean()?;
        }
        b'n' | b'q' => {
            r.half()?;
        }
        b'i' | b'u' => {
            r.word()?;
        }
        b'h' => {
            r.fd()?;
        }
        b'x' | b't' | b'd' => {
            r.quad()?;
        }
        b's' => {
            r.text()?;
        }
        b'o' => {
            r.path()?;
        }
        b'g' => {
            r.signature()?;
        }
        b'v' => {
            let inner = r.signature()?;
            if !inner.is_single() {
                return Err(Malformed::NotSingle);
            }
            r.nest(|r| walk(r, inner.as_bytes(), 0))?;
        }
        b'a' => {
            let past = single(sig, i, Depth::ZERO, Depth::ZERO).map_err(Malformed::Sig)?;
            let element = sig.get(i + 1).map_or(1, |&e| boundary(e));
            r.nest(|r| {
                let end = r.array(element)?;
                while r.at < end {
                    walk(r, sig, i + 1)?;
                }
                if r.at == end {
                    Ok(())
                } else {
                    Err(Malformed::Overrun)
                }
            })?;
            return Ok(past);
        }
        b'(' => {
            return r.nest(|r| {
                r.align(Align::Eight)?;
                let mut j = i + 1;
                while sig.get(j).is_some_and(|&c| c != b')') {
                    j = walk(r, sig, j)?;
                }
                Ok(j + 1)
            });
        }
        b'{' => {
            r.align(Align::Eight)?;
            let j = walk(r, sig, i + 1)?;
            let j = walk(r, sig, j)?;
            return Ok(j + 1);
        }
        _ => return Err(Malformed::Sig(SigError::Code)),
    }
    Ok(i + 1)
}

/// The elements of a received ARRAY, read one at a time as `T` reads.
/// The message was checked whole when it was decoded, so every element
/// reads.
pub struct ArrayRead<'a, T> {
    r: BodyReader<'a>,
    end: At,
    element: PhantomData<fn() -> T>,
}

impl<'a, T: Marshal> ArrayRead<'a, T> {
    /// The array at the reader's place, which moves past it.
    pub(super) fn read(r: &mut BodyReader<'a>) -> Result<ArrayRead<'a, T>, Malformed> {
        let end = r.array(T::ALIGN.bytes())?;
        let elements = ArrayRead {
            r: r.clone(),
            end,
            element: PhantomData,
        };
        r.at = end;
        Ok(elements)
    }

    /// Whether no element is left.
    pub fn is_empty(&self) -> bool {
        self.r.at >= self.end
    }
}

impl<'a, T: Marshal> Iterator for ArrayRead<'a, T> {
    type Item = T::Read<'a>;

    fn next(&mut self) -> Option<T::Read<'a>> {
        if self.is_empty() {
            return None;
        }
        match T::read(&mut self.r) {
            Ok(v) => Some(v),
            Err(_) => {
                self.r.at = self.end;
                None
            }
        }
    }
}

impl<T> Clone for ArrayRead<'_, T> {
    fn clone(&self) -> Self {
        ArrayRead {
            r: self.r.clone(),
            end: self.end,
            element: PhantomData,
        }
    }
}

impl<T> fmt::Debug for ArrayRead<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArrayRead")
            .field("at", &self.r.at.0)
            .field("end", &self.end.0)
            .finish()
    }
}

/// A received value of a type this module reads only as its bytes: a
/// variant or a dict, checked against its signature when the message was
/// decoded and passed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Unread<'a>(&'a [u8]);

impl<'a> Unread<'a> {
    /// The value's bytes, in the message's byte order.
    pub const fn bytes(&self) -> &'a [u8] {
        self.0
    }
}

/// A received message's header: its byte order, type, flags and serial,
/// and its fields, each of its code's type. Only [`decode`] makes one, so
/// a header's byte order and flags are the ones its bytes carried:
///
/// ```compile_fail,E0451
/// use baud_domain::dbus::{Header, Order};
/// fn f(h: Header<'_>) { let _ = Header { order: Order::Big, ..h }; }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{decode, Flags, Inbound, Order};
/// let mut m = vec![b'l', 2, 0, 1, 0, 0, 0, 0, 3, 0, 0, 0, 8, 0, 0, 0];
/// m.extend_from_slice(&[5, 1, b'u', 0, 2, 0, 0, 0]);
/// let Ok(Inbound::Return(r)) = decode(&m) else { panic!("a reply") };
/// assert_eq!(r.header().order(), Order::Little);
/// assert_eq!(r.header().flags(), Flags::NONE);
/// assert_eq!((r.header().serial().get(), r.reply_to.get()), (3, 2));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header<'a> {
    order: Order,
    kind: Option<MessageKind>,
    flags: Flags,
    serial: Serial,
    path: Option<ObjectPath<'a>>,
    interface: Option<InterfaceName<'a>>,
    member: Option<MemberName<'a>>,
    error_name: Option<ErrorName<'a>>,
    reply_serial: Option<Serial>,
    destination: Option<BusName<'a>>,
    sender: Option<BusName<'a>>,
    signature: Option<Sig<'a>>,
    fds: Option<FdCount>,
}

impl<'a> Header<'a> {
    /// The byte order the message is in.
    pub const fn order(&self) -> Order {
        self.order
    }

    /// The message's type, or `None` for a type this specification does
    /// not define.
    pub const fn kind(&self) -> Option<MessageKind> {
        self.kind
    }

    /// The flags this specification defines, of those set.
    pub const fn flags(&self) -> Flags {
        self.flags
    }

    /// The message's serial.
    pub const fn serial(&self) -> Serial {
        self.serial
    }

    /// PATH.
    pub const fn path(&self) -> Option<ObjectPath<'a>> {
        self.path
    }

    /// INTERFACE.
    pub const fn interface(&self) -> Option<InterfaceName<'a>> {
        self.interface
    }

    /// MEMBER.
    pub const fn member(&self) -> Option<MemberName<'a>> {
        self.member
    }

    /// ERROR_NAME.
    pub const fn error_name(&self) -> Option<ErrorName<'a>> {
        self.error_name
    }

    /// REPLY_SERIAL.
    pub const fn reply_serial(&self) -> Option<Serial> {
        self.reply_serial
    }

    /// DESTINATION.
    pub const fn destination(&self) -> Option<BusName<'a>> {
        self.destination
    }

    /// SENDER, which a bus sets.
    pub const fn sender(&self) -> Option<BusName<'a>> {
        self.sender
    }

    /// SIGNATURE: the body's signature, empty when the field is absent.
    pub const fn signature(&self) -> Sig<'a> {
        match self.signature {
            Some(s) => s,
            None => Sig::EMPTY,
        }
    }

    /// UNIX_FDS: how many descriptors came with the message, none when the
    /// field is absent.
    pub const fn fds(&self) -> FdCount {
        match self.fds {
            Some(n) => n,
            None => FdCount::ZERO,
        }
    }

    /// The fields from the reader's place to `end`, each on 8 and each
    /// a code and a variant; known codes into their slots, unknown codes
    /// checked and passed over.
    fn read_fields(&mut self, r: &mut BodyReader<'a>, end: At) -> Result<(), Malformed> {
        r.nest(|r| {
            while r.at < end {
                r.nest(|r| {
                    r.align(Align::Eight)?;
                    let code = r.byte()?;
                    let sig = r.signature()?;
                    if !sig.is_single() {
                        return Err(Malformed::NotSingle);
                    }
                    r.nest(|r| match FieldCode::from_byte(code) {
                        Some(field) => self.read_field(r, field, sig),
                        None if code == 0 => Err(Malformed::InvalidField),
                        None => walk(r, sig.as_bytes(), 0).map(|_| ()),
                    })
                })?;
            }
            if r.at == end {
                Ok(())
            } else {
                Err(Malformed::Overrun)
            }
        })
    }

    /// One known field's value, of type `sig`, into its slot.
    fn read_field(
        &mut self,
        r: &mut BodyReader<'a>,
        field: FieldCode,
        sig: Sig<'_>,
    ) -> Result<(), Malformed> {
        if sig != field.value_sig() {
            return Err(Malformed::FieldType(field));
        }
        let name = Malformed::Name;
        match field {
            FieldCode::Path => fill(&mut self.path, r.path()?, field),
            FieldCode::Interface => fill(
                &mut self.interface,
                InterfaceName::parse(r.text()?).map_err(name)?,
                field,
            ),
            FieldCode::Member => fill(
                &mut self.member,
                MemberName::parse(r.text()?).map_err(name)?,
                field,
            ),
            FieldCode::ErrorName => fill(
                &mut self.error_name,
                ErrorName::parse(r.text()?).map_err(name)?,
                field,
            ),
            FieldCode::ReplySerial => fill(
                &mut self.reply_serial,
                Serial::received(r.word()?).ok_or(Malformed::ZeroSerial)?,
                field,
            ),
            FieldCode::Destination => fill(
                &mut self.destination,
                BusName::parse(r.text()?).map_err(name)?,
                field,
            ),
            FieldCode::Sender => fill(
                &mut self.sender,
                BusName::parse(r.text()?).map_err(name)?,
                field,
            ),
            FieldCode::Signature => fill(&mut self.signature, r.signature()?, field),
            FieldCode::UnixFds => fill(&mut self.fds, FdCount::received(r.word()?), field),
        }
    }

    /// The field `field` requires, or [`Malformed::Missing`].
    fn needs<T>(value: Option<T>, field: FieldCode) -> Result<T, Malformed> {
        value.ok_or(Malformed::Missing(field))
    }
}

/// `value` into an empty `slot`; a field given twice is refused.
fn fill<T>(slot: &mut Option<T>, value: T, field: FieldCode) -> Result<(), Malformed> {
    if slot.is_some() {
        return Err(Malformed::Repeated(field));
    }
    *slot = Some(value);
    Ok(())
}

/// Why a body did not read as the arguments asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BodyError {
    /// The body's signature is not the arguments'.
    Signature,
    /// A value did not read; a body from [`decode`] was checked whole, so
    /// this is never its answer.
    Malformed(Malformed),
}

impl BodyError {
    /// The rule broken, as a sentence.
    pub const fn rule(self) -> &'static str {
        match self {
            BodyError::Signature => "a body is read only as its own signature",
            BodyError::Malformed(m) => m.rule(),
        }
    }
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rule())
    }
}

/// A received message's body, checked against its signature: read only as
/// [`Args`] whose signature is the body's own.
///
/// ```
/// use baud_domain::dbus::{decode, BodyError, Inbound, Str, U32};
/// let mut m = vec![b'l', 2, 0, 1, 4, 0, 0, 0, 3, 0, 0, 0, 15, 0, 0, 0];
/// m.extend_from_slice(&[5, 1, b'u', 0, 2, 0, 0, 0, 8, 1, b'g', 0, 1, b'u', 0, 0]);
/// m.extend_from_slice(&42u32.to_le_bytes());
/// let Ok(Inbound::Return(r)) = decode(&m) else { panic!("a reply") };
/// assert_eq!(r.body().read::<(U32,)>(), Ok((U32::new(42),)));
/// assert_eq!(r.body().read::<(Str,)>().map(|_| ()), Err(BodyError::Signature));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyRef<'a> {
    sig: Sig<'a>,
    bytes: &'a [u8],
    order: Order,
    fds: FdCount,
}

impl<'a> BodyRef<'a> {
    /// The body's signature.
    pub const fn sig(&self) -> Sig<'a> {
        self.sig
    }

    /// The body's bytes, in the message's byte order.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The body as `A`, refused unless its signature is `A`'s.
    pub fn read<A: Args>(&self) -> Result<A::Read<'a>, BodyError> {
        let sig = A::SIG;
        if sig.as_sig() != self.sig {
            return Err(BodyError::Signature);
        }
        let mut r = BodyReader::new(self.bytes, self.order, self.fds);
        let values = A::read_all(&mut r).map_err(BodyError::Malformed)?;
        if r.at.0 != self.bytes.len() {
            return Err(BodyError::Malformed(Malformed::Trailing));
        }
        Ok(values)
    }
}

/// A METHOD_RETURN: the serial of the call it answers, required, and what
/// the call returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Return<'a> {
    /// REPLY_SERIAL: the call answered.
    pub reply_to: Serial,
    header: Header<'a>,
    body: BodyRef<'a>,
}

/// An ERROR: the serial of the message it answers and the error's name,
/// both required, and its arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ErrorReply<'a> {
    /// REPLY_SERIAL: the message answered.
    pub reply_to: Serial,
    /// ERROR_NAME.
    pub name: ErrorName<'a>,
    header: Header<'a>,
    body: BodyRef<'a>,
}

impl<'a> ErrorReply<'a> {
    /// The error's message: its first argument, when that is a STRING
    /// ("Message Types": Errors).
    pub fn message(&self) -> Option<Str<'a>> {
        if self.body.sig.as_bytes().first() != Some(&b's') {
            return None;
        }
        BodyReader::new(self.body.bytes, self.body.order, self.body.fds)
            .str()
            .ok()
    }
}

/// A SIGNAL: the object, interface and member it was emitted as, each
/// required, so none is an `Option`:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::{InterfaceName, Signal};
/// fn f(signal: Signal<'_>) { let _: Option<InterfaceName<'_>> = signal.interface; }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{decode, Emit, Inbound, InterfaceName, MemberName, ObjectPath, Body, Serials};
/// let emit = Emit {
///     path: ObjectPath::lit("/org/example/A"),
///     interface: InterfaceName::lit("org.example.A"),
///     member: MemberName::lit("Changed"),
/// };
/// let sent = emit.encode(&Body::EMPTY, &mut Serials::new()).expect("small");
/// let Ok(Inbound::Signal(signal)) = decode(sent.bytes()) else { panic!("a signal") };
/// let interface: InterfaceName<'_> = signal.interface;
/// assert_eq!(interface, emit.interface);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Signal<'a> {
    /// PATH: the object emitting it.
    pub path: ObjectPath<'a>,
    /// INTERFACE.
    pub interface: InterfaceName<'a>,
    /// MEMBER: the signal.
    pub member: MemberName<'a>,
    header: Header<'a>,
    body: BodyRef<'a>,
}

/// A METHOD_CALL to this connection: the object and the method, required;
/// its INTERFACE, which a call may leave out, is the header's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Called<'a> {
    /// PATH: the object called.
    pub path: ObjectPath<'a>,
    /// MEMBER: the method.
    pub member: MemberName<'a>,
    header: Header<'a>,
    body: BodyRef<'a>,
}

/// The header and body accessors every received message has.
macro_rules! received {
    ($($ty:ident),+) => {$(
        impl<'a> $ty<'a> {
            /// The whole header.
            pub const fn header(&self) -> &Header<'a> {
                &self.header
            }

            /// The body.
            pub const fn body(&self) -> BodyRef<'a> {
                self.body
            }
        }
    )+};
}

received!(Return, ErrorReply, Signal, Called);

/// The unique name that owns a well-known name: the SENDER of a reply
/// from it, which the bus sets ("Header Fields": SENDER "is controlled by
/// the message bus, so it is as reliable and trustworthy as the message
/// bus itself"; "Message Bus Names": no other connection can take a
/// unique name). A signal is acted on only once its own SENDER is this
/// name ([`Peer::vouch`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    name: OwnedBusName,
}

impl Peer {
    /// The connection that sent `reply`, if its SENDER is there and is a
    /// unique name.
    pub fn from_reply(reply: &Return<'_>) -> Option<Peer> {
        Peer::unique(reply.header.sender()?)
    }

    /// The connection whose unique name is `name`; a well-known name is
    /// `None`.
    pub(crate) const fn unique(name: BusName<'_>) -> Option<Peer> {
        match OwnedBusName::unique(name) {
            Some(name) => Some(Peer { name }),
            None => None,
        }
    }

    /// The peer's unique name.
    #[inline]
    pub fn name(&self) -> BusName<'_> {
        self.name.as_name()
    }

    /// `signal`, trusted, if this peer sent it: `None` for a signal with
    /// no SENDER or another one.
    pub fn vouch<'a>(&self, signal: &Signal<'a>) -> Option<Trusted<'a>> {
        if signal.header.sender()? != self.name() {
            return None;
        }
        Some(Trusted { signal: *signal })
    }
}

/// A signal whose SENDER is the [`Peer`] it was checked against: the only
/// form in which a signal is acted on, so one from another connection,
/// or from none named, is never taken for one from the connection that
/// holds the name.
///
/// A signal is not passed where a trusted one is asked for:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::{Signal, Trusted};
/// fn fdo_signal(_: Trusted<'_>) {}
/// fn f(signal: Signal<'_>) { fdo_signal(&signal) }
/// ```
///
/// and is not made trusted by naming it so:
///
/// ```compile_fail,E0451
/// use baud_domain::dbus::{Signal, Trusted};
/// fn f(signal: Signal<'_>) { let _ = Trusted { signal }; }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Peer, Signal, Trusted};
/// fn fdo_signal(_: Trusted<'_>) {}
/// fn f(peer: &Peer, signal: &Signal<'_>) {
///     if let Some(trusted) = peer.vouch(signal) {
///         fdo_signal(trusted);
///     }
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Trusted<'a> {
    signal: Signal<'a>,
}

impl<'a> Trusted<'a> {
    /// The signal.
    #[inline]
    pub const fn signal(&self) -> Signal<'a> {
        self.signal
    }
}

/// One received message, by its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Inbound<'a> {
    /// A method's reply.
    Return(Return<'a>),
    /// An error reply.
    ErrorReply(ErrorReply<'a>),
    /// A signal.
    Signal(Signal<'a>),
    /// A method call.
    Called(Called<'a>),
    /// A message of a type this specification does not define, checked and
    /// passed over ("Invalid Protocol and Spec Extensions").
    Unknown(Header<'a>),
}

/// `bytes`, exactly one message as [`frame`] measures it, checked against
/// every rule and read. The checks are strict: a BOOLEAN of 2, a NUL
/// inside a string, padding that is not NUL, a known field of another
/// type, a missing required field or a serial of 0 refuse the message
/// whole ("Invalid Protocol and Spec Extensions").
///
/// ```
/// use baud_domain::dbus::{decode, Malformed};
/// // A reply to serial 2 whose body, `b`, is 2.
/// let mut m = vec![b'l', 2, 0, 1, 4, 0, 0, 0, 3, 0, 0, 0, 15, 0, 0, 0];
/// m.extend_from_slice(&[5, 1, b'u', 0, 2, 0, 0, 0, 8, 1, b'g', 0, 1, b'b', 0, 0]);
/// m.extend_from_slice(&2u32.to_le_bytes());
/// assert_eq!(decode(&m), Err(Malformed::Boolean));
/// m[32] = 1;
/// assert!(decode(&m).is_ok());
/// ```
pub fn decode(bytes: &[u8]) -> Result<Inbound<'_>, Malformed> {
    match frame(bytes) {
        Frame::Whole(len) if len.get() as usize == bytes.len() => {}
        Frame::Malformed(m) => return Err(m),
        Frame::Whole(_) | Frame::NeedMore => return Err(Malformed::Length),
    }
    let order = Order::of(bytes[0]).ok_or(Malformed::Endian)?;
    let kind = match bytes[1] {
        0 => return Err(Malformed::Kind),
        k => MessageKind::from_byte(k),
    };
    let mut r = BodyReader::new(bytes, order, FdCount::ZERO);
    let _fixed = r.take(FIXED - 4)?;
    let fields_len = r.word()?;
    let serial = word_at(bytes, 8, order).and_then(Serial::received);
    let mut header = Header {
        order,
        kind,
        flags: Flags::received(bytes[2]),
        serial: serial.ok_or(Malformed::ZeroSerial)?,
        path: None,
        interface: None,
        member: None,
        error_name: None,
        reply_serial: None,
        destination: None,
        sender: None,
        signature: None,
        fds: None,
    };
    let end = At(FIXED + fields_len as usize);
    header.read_fields(&mut r, end)?;
    r.align(Align::Eight)?;
    let body = BodyRef {
        sig: header.signature(),
        bytes: &bytes[r.at.0..],
        order,
        fds: header.fds(),
    };
    let mut b = BodyReader::new(body.bytes, order, body.fds);
    let codes = body.sig.as_bytes();
    let mut i = 0;
    while i < codes.len() {
        i = walk(&mut b, codes, i)?;
    }
    if b.at.0 != body.bytes.len() {
        return Err(Malformed::Trailing);
    }
    Ok(match kind {
        None => Inbound::Unknown(header),
        Some(MessageKind::MethodCall) => Inbound::Called(Called {
            path: Header::needs(header.path, FieldCode::Path)?,
            member: Header::needs(header.member, FieldCode::Member)?,
            header,
            body,
        }),
        Some(MessageKind::MethodReturn) => Inbound::Return(Return {
            reply_to: Header::needs(header.reply_serial, FieldCode::ReplySerial)?,
            header,
            body,
        }),
        Some(MessageKind::Error) => Inbound::ErrorReply(ErrorReply {
            name: Header::needs(header.error_name, FieldCode::ErrorName)?,
            reply_to: Header::needs(header.reply_serial, FieldCode::ReplySerial)?,
            header,
            body,
        }),
        Some(MessageKind::Signal) => Inbound::Signal(Signal {
            path: Header::needs(header.path, FieldCode::Path)?,
            interface: Header::needs(header.interface, FieldCode::Interface)?,
            member: Header::needs(header.member, FieldCode::Member)?,
            header,
            body,
        }),
    })
}
