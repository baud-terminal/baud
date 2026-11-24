//! The values a body carries and how each is marshalled
//! (`[[source: dbus-spec]]`, "Basic types", "Container types", "Byte order
//! and alignment", "Marshalling basic types" and "Marshalling containers").
//!
//! Every value type names its own signature and alignment in a `const`, so
//! a tuple of them, [`Args`], knows its body's signature at compile time,
//! and the SIGNATURE header field is that constant rather than a string a
//! caller writes. Only the types here are [`Marshal`]: `&str` is not, so a
//! string reaches a body only as a checked [`Str`].
//!
//! Each type also names what reading it from a received body yields, so a
//! tuple of them reads a body whose signature is the tuple's own.

#[cfg(feature = "alloc")]
use super::body::{BodyWriter, EncodeError};
use super::decode::{ArrayRead, BodyReader, Malformed, Unread};
use super::name::{ObjectPath, Str};
use super::sig::{Sig, SigCodes};

/// The boundary a value starts on, counted from the message's first byte:
/// natural for a basic type, 8 for a struct or a dict entry, a variant's 1
/// from its signature ("Byte order and alignment").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    /// Any byte: `y`, `g` and `v`.
    One,
    /// A multiple of 4: `b`, `i`, `u`, `h`, `s`, `o` and an array's length.
    Four,
    /// A multiple of 8: `d`, a struct and a dict entry.
    Eight,
}

impl Align {
    /// The boundary in bytes.
    #[inline]
    pub const fn bytes(self) -> usize {
        match self {
            Align::One => 1,
            Align::Four => 4,
            Align::Eight => 8,
        }
    }
}

mod sealed {
    /// The value types of this module, and no others.
    pub trait Value {}
    /// The tuples of value types, and no others.
    pub trait Values {}
}

/// A value a body can carry: its signature, one complete type, and its
/// alignment are constants, and it writes exactly what the signature
/// says. Implemented only by this module's types, so no value's bytes can
/// disagree with its signature.
pub trait Marshal: sealed::Value {
    /// Where the value starts.
    const ALIGN: Align;
    /// The value's type, one complete type.
    const SIG: SigCodes;
    /// What reading the value from a received body yields.
    type Read<'a>;
    /// Writes the value, aligned, at the writer's end.
    #[cfg(feature = "alloc")]
    fn marshal(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError>;
    /// Reads the value, aligned, at the reader's place.
    fn read<'a>(r: &mut BodyReader<'a>) -> Result<Self::Read<'a>, Malformed>;
}

/// A basic type: the only types a dict entry's key may be ("Container
/// types").
pub trait Basic: Marshal {}

/// The arguments of a body: a tuple of 0 to 12 [`Marshal`] values, whose
/// signature is its elements' in order.
pub trait Args: sealed::Values {
    /// The body's signature.
    const SIG: SigCodes;
    /// The arguments as a body read from received bytes yields them.
    type Read<'a>;
    /// Writes each argument in order.
    #[cfg(feature = "alloc")]
    fn marshal_all(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError>;
    /// Reads each argument in order.
    fn read_all<'a>(r: &mut BodyReader<'a>) -> Result<Self::Read<'a>, Malformed>;
}

impl sealed::Values for () {}

impl Args for () {
    const SIG: SigCodes = SigCodes::EMPTY;
    type Read<'a> = ();
    #[cfg(feature = "alloc")]
    fn marshal_all(&self, _: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        Ok(())
    }
    fn read_all<'a>(_: &mut BodyReader<'a>) -> Result<(), Malformed> {
        Ok(())
    }
}

/// `Args` for one tuple arity: the signature concatenated, the elements
/// written in order.
macro_rules! args {
    ($($t:ident $i:tt),+) => {
        impl<$($t: Marshal),+> sealed::Values for ($($t,)+) {}

        impl<$($t: Marshal),+> Args for ($($t,)+) {
            const SIG: SigCodes = SigCodes::EMPTY $(.concat(<$t as Marshal>::SIG))+;
            type Read<'a> = ($(<$t as Marshal>::Read<'a>,)+);
            #[cfg(feature = "alloc")]
            fn marshal_all(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
                $(self.$i.marshal(w)?;)+
                Ok(())
            }
            fn read_all<'a>(r: &mut BodyReader<'a>) -> Result<Self::Read<'a>, Malformed> {
                Ok(($(<$t as Marshal>::read(r)?,)+))
            }
        }
    };
}

args!(A 0);
args!(A 0, B 1);
args!(A 0, B 1, C 2);
args!(A 0, B 1, C 2, D 3);
args!(A 0, B 1, C 2, D 3, E 4);
args!(A 0, B 1, C 2, D 3, E 4, F 5);
args!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
args!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
args!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
args!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
args!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
args!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);

/// `Notify`'s arguments (`[[source: freedesktop-notification-spec]]`):
/// application name, the id replaced, icon, summary, body, the actions as
/// key and label pairs, the hints, and the timeout in milliseconds.
pub type NotifyArgs<'a> = (
    Str<'a>,
    U32,
    Str<'a>,
    Str<'a>,
    Str<'a>,
    FlatPairs<'a>,
    Dict<'a, Str<'a>, Variant<'a>>,
    I32,
);

const _: () = assert!(SigCodes::eq(
    <NotifyArgs<'_> as Args>::SIG,
    Sig::lit("susssasa{sv}i")
));

/// One basic type's `Marshal`: its signature from `$code`, its alignment,
/// the writer call that puts it down, and the reader call that takes it
/// up, yielding `$rt`.
macro_rules! basic {
    (
        $ty:ty, $code:literal, $align:expr,
        |$v:ident, $w:ident| $write:expr,
        <$l:lifetime> $rt:ty = |$r:ident| $read:expr
    ) => {
        impl sealed::Value for $ty {}

        impl Marshal for $ty {
            const ALIGN: Align = $align;
            const SIG: SigCodes = SigCodes::of(Sig::lit($code));
            type Read<$l> = $rt;
            #[cfg(feature = "alloc")]
            fn marshal(&self, $w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
                let $v = self;
                $write
            }
            fn read<$l>($r: &mut BodyReader<$l>) -> Result<$rt, Malformed> {
                Ok($read)
            }
        }

        impl Basic for $ty {}
    };
}

/// A BYTE, `y`: 8 bits, unsigned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Byte(u8);

impl Byte {
    /// `v` as a BYTE.
    #[inline]
    pub const fn new(v: u8) -> Byte {
        Byte(v)
    }

    /// The byte.
    #[inline]
    pub const fn get(self) -> u8 {
        self.0
    }
}

basic!(Byte, "y", Align::One, |v, w| {
    w.byte(v.0);
    Ok(())
}, <'a> Byte = |r| Byte(r.byte()?));

/// A BOOLEAN, `b`: a 32-bit word that is 0 or 1 and nothing else ("Basic
/// types"), so it is two states rather than a number.
///
/// There is no conversion from a word, which could be 2:
///
/// ```compile_fail,E0277
/// use baud_domain::dbus::Boolean;
/// let _ = Boolean::from(2u32);
/// ```
///
/// The legal path: a variant, or the language's two states.
///
/// ```
/// use baud_domain::dbus::{Body, Boolean, Sig};
/// assert_eq!(Boolean::from(true), Boolean::True);
/// let body = Body::of(&(Boolean::True,)).expect("one word");
/// assert_eq!(body.sig(), Sig::lit("b"));
/// assert_eq!(body.bytes(), &[1, 0, 0, 0]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Boolean {
    /// 0.
    False,
    /// 1.
    True,
}

impl From<bool> for Boolean {
    fn from(b: bool) -> Boolean {
        if b { Boolean::True } else { Boolean::False }
    }
}

basic!(Boolean, "b", Align::Four, |v, w| {
    w.word(match v {
        Boolean::False => 0,
        Boolean::True => 1,
    });
    Ok(())
}, <'a> Boolean = |r| r.boolean()?);

/// An INT32, `i`: 32 bits, two's complement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct I32(i32);

impl I32 {
    /// `v` as an INT32.
    #[inline]
    pub const fn new(v: i32) -> I32 {
        I32(v)
    }

    /// The integer.
    #[inline]
    pub const fn get(self) -> i32 {
        self.0
    }
}

basic!(I32, "i", Align::Four, |v, w| {
    w.word(u32::from_le_bytes(v.0.to_le_bytes()));
    Ok(())
}, <'a> I32 = |r| I32(i32::from_ne_bytes(r.word()?.to_ne_bytes())));

/// A UINT32, `u`: 32 bits, unsigned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U32(u32);

impl U32 {
    /// `v` as a UINT32.
    #[inline]
    pub const fn new(v: u32) -> U32 {
        U32(v)
    }

    /// The integer.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

basic!(U32, "u", Align::Four, |v, w| {
    w.word(v.0);
    Ok(())
}, <'a> U32 = |r| U32(r.word()?));

/// A DOUBLE, `d`: an IEEE 754 double, on 8.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Double(f64);

impl Double {
    /// `v` as a DOUBLE.
    #[inline]
    pub const fn new(v: f64) -> Double {
        Double(v)
    }

    /// The number.
    #[inline]
    pub const fn get(self) -> f64 {
        self.0
    }
}

basic!(Double, "d", Align::Eight, |v, w| {
    w.align(Align::Eight);
    w.bytes(&v.0.to_le_bytes());
    Ok(())
}, <'a> Double = |r| Double(f64::from_bits(r.quad()?)));

/// Which of a message's file descriptors a UNIX_FD names: an index into
/// the array of descriptors that accompany it ("Summary of D-Bus
/// marshalling").
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FdIndex(u32);

impl FdIndex {
    /// The first descriptor.
    pub const ZERO: FdIndex = FdIndex(0);

    /// The descriptor at `v`.
    #[inline]
    pub const fn new(v: u32) -> FdIndex {
        FdIndex(v)
    }

    /// The index.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// How many file descriptors accompany a message: its UNIX_FDS field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FdCount(u32);

impl FdCount {
    /// None: a message with no UNIX_FDS field.
    pub const ZERO: FdCount = FdCount(0);

    /// The count.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// A received UNIX_FDS field's count.
    pub(super) const fn received(n: u32) -> FdCount {
        FdCount(n)
    }

    /// The count that covers `index` as well as these: every index a body
    /// names is below its message's UNIX_FDS.
    #[cfg(feature = "alloc")]
    #[inline]
    pub(crate) const fn covering(self, index: FdIndex) -> FdCount {
        let need = index.0.saturating_add(1);
        if need > self.0 { FdCount(need) } else { self }
    }
}

/// A UNIX_FD, `h`: the index of a file descriptor the message carries
/// out of band. Only a connection that has agreed to pass descriptors
/// makes one, so a body holding one is sent only where descriptors can
/// follow it ("NEGOTIATE_UNIX_FD Command"); a body counts the descriptors
/// its values name.
///
/// There is no public constructor:
///
/// ```compile_fail,E0423
/// use baud_domain::dbus::{FdIndex, UnixFd};
/// let _ = UnixFd(FdIndex::ZERO);
/// ```
///
/// The legal path: a body without descriptors names none.
///
/// ```
/// use baud_domain::dbus::{Body, FdCount, U32};
/// let body = Body::of(&(U32::new(1),)).expect("one word");
/// assert_eq!(body.fds(), FdCount::ZERO);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnixFd(FdIndex);

impl UnixFd {
    /// The descriptor at `index` of those the message carries.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "a connection that agreed to pass descriptors makes it"
        )
    )]
    pub(crate) const fn new(index: FdIndex) -> UnixFd {
        UnixFd(index)
    }

    /// Which descriptor.
    #[inline]
    pub const fn index(self) -> FdIndex {
        self.0
    }
}

basic!(UnixFd, "h", Align::Four, |v, w| {
    w.fd(v.0);
    Ok(())
}, <'a> FdIndex = |r| r.fd()?);

basic!(Str<'_>, "s", Align::Four, |v, w| w.string(v.as_str()),
    <'a> Str<'a> = |r| r.str()?);

basic!(ObjectPath<'_>, "o", Align::Four, |v, w| w.string(v.as_str()),
    <'a> ObjectPath<'a> = |r| r.path()?);

basic!(Sig<'_>, "g", Align::One, |v, w| {
    w.signature(*v);
    Ok(())
}, <'a> Sig<'a> = |r| r.signature()?);

/// An ARRAY of `T`, `a` then `T`'s type: a length in bytes, padding to
/// `T`'s boundary even when there is no element, then the elements
/// ("Marshalling containers"). At most 2^26 bytes of elements.
#[derive(Debug, PartialEq)]
pub struct Array<'a, T>(&'a [T]);

impl<'a, T: Marshal> Array<'a, T> {
    /// The array of `items`.
    #[inline]
    pub const fn new(items: &'a [T]) -> Array<'a, T> {
        Array(items)
    }

    /// The elements.
    #[inline]
    pub const fn items(&self) -> &'a [T] {
        self.0
    }
}

impl<T> Clone for Array<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Array<'_, T> {}

impl<T: Marshal> sealed::Value for Array<'_, T> {}

impl<T: Marshal> Marshal for Array<'_, T> {
    const ALIGN: Align = Align::Four;
    const SIG: SigCodes = SigCodes::array(T::SIG);
    type Read<'a> = ArrayRead<'a, T>;
    fn read<'a>(r: &mut BodyReader<'a>) -> Result<ArrayRead<'a, T>, Malformed> {
        ArrayRead::read(r)
    }
    #[cfg(feature = "alloc")]
    fn marshal(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        w.array(T::ALIGN, |w| {
            for item in self.0 {
                item.marshal(w)?;
            }
            Ok(())
        })
    }
}

/// An ARRAY of BYTE, `ay`, from bytes: one copy rather than a value at a
/// time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    /// The array of `bytes`.
    #[inline]
    pub const fn new(bytes: &'a [u8]) -> Bytes<'a> {
        Bytes(bytes)
    }

    /// The bytes.
    #[inline]
    pub const fn get(&self) -> &'a [u8] {
        self.0
    }
}

impl sealed::Value for Bytes<'_> {}

impl Marshal for Bytes<'_> {
    const ALIGN: Align = Align::Four;
    const SIG: SigCodes = SigCodes::array(Byte::SIG);
    type Read<'a> = Bytes<'a>;
    fn read<'a>(r: &mut BodyReader<'a>) -> Result<Bytes<'a>, Malformed> {
        Ok(Bytes(r.bytes()?))
    }
    #[cfg(feature = "alloc")]
    fn marshal(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        w.array(Align::One, |w| {
            w.bytes(self.0);
            Ok(())
        })
    }
}

/// Pairs of strings sent flat, as `as`: each pair's first string, then its
/// second, as a notification's actions are a key then its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FlatPairs<'a>(&'a [(Str<'a>, Str<'a>)]);

impl<'a> FlatPairs<'a> {
    /// The array of `pairs`, flattened.
    #[inline]
    pub const fn new(pairs: &'a [(Str<'a>, Str<'a>)]) -> FlatPairs<'a> {
        FlatPairs(pairs)
    }
}

impl sealed::Value for FlatPairs<'_> {}

impl Marshal for FlatPairs<'_> {
    const ALIGN: Align = Align::Four;
    const SIG: SigCodes = SigCodes::array(Str::SIG);
    type Read<'a> = ArrayRead<'a, Str<'static>>;
    fn read<'a>(r: &mut BodyReader<'a>) -> Result<Self::Read<'a>, Malformed> {
        ArrayRead::read(r)
    }
    #[cfg(feature = "alloc")]
    fn marshal(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        w.array(Align::Four, |w| {
            for (k, v) in self.0 {
                w.string(k.as_str())?;
                w.string(v.as_str())?;
            }
            Ok(())
        })
    }
}

/// A dict, `a{KV}`: an array whose elements are dict entries, each a basic
/// key and one value on an 8-byte boundary ("Container types"). The entry
/// has no type of its own: it exists only as this array's element, as the
/// spec allows it nowhere else.
///
/// A key that is not a basic type does not compile:
///
/// ```compile_fail,E0277
/// use baud_domain::dbus::{Array, Dict, U32, Variant};
/// let _ = Dict::<Array<U32>, Variant>::new(&[]);
/// ```
///
/// and there is no entry type to build one outside a dict:
///
/// ```compile_fail,E0425
/// use baud_domain::dbus::Dict;
/// let _: DictEntry;
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Body, Dict, Sig, Str, U32, Variant};
/// let hints = [(Str::lit("x"), Variant::U32(U32::new(3)))];
/// let body = Body::of(&(Dict::new(&hints),)).expect("small");
/// assert_eq!(body.sig(), Sig::lit("a{sv}"));
/// ```
#[derive(Debug, PartialEq)]
pub struct Dict<'a, K: Basic, V>(&'a [(K, V)]);

impl<'a, K: Basic, V: Marshal> Dict<'a, K, V> {
    /// The dict of `entries`, in their order.
    #[inline]
    pub const fn new(entries: &'a [(K, V)]) -> Dict<'a, K, V> {
        Dict(entries)
    }

    /// The entries.
    #[inline]
    pub const fn entries(&self) -> &'a [(K, V)] {
        self.0
    }
}

impl<K: Basic, V> Clone for Dict<'_, K, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: Basic, V> Copy for Dict<'_, K, V> {}

impl<K: Basic, V: Marshal> sealed::Value for Dict<'_, K, V> {}

impl<K: Basic, V: Marshal> Marshal for Dict<'_, K, V> {
    const ALIGN: Align = Align::Four;
    const SIG: SigCodes = SigCodes::dict(K::SIG, V::SIG);
    type Read<'a> = Unread<'a>;
    fn read<'a>(r: &mut BodyReader<'a>) -> Result<Unread<'a>, Malformed> {
        r.unread(Self::SIG.as_sig())
    }
    #[cfg(feature = "alloc")]
    fn marshal(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        w.array(Align::Eight, |w| {
            for (k, v) in self.0 {
                w.align(Align::Eight);
                k.marshal(w)?;
                v.marshal(w)?;
            }
            Ok(())
        })
    }
}

/// A STRUCT of `T`'s values, `(` their types `)`, on an 8-byte boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Struct<T: Args>(T);

impl<T: Args> Struct<T> {
    /// The struct of `fields`.
    #[inline]
    pub const fn new(fields: T) -> Struct<T> {
        Struct(fields)
    }

    /// The fields.
    #[inline]
    pub const fn fields(&self) -> &T {
        &self.0
    }
}

impl<T: Args> sealed::Value for Struct<T> {}

impl<T: Args> Marshal for Struct<T> {
    const ALIGN: Align = Align::Eight;
    const SIG: SigCodes = SigCodes::structure(T::SIG);
    type Read<'a> = T::Read<'a>;
    fn read<'a>(r: &mut BodyReader<'a>) -> Result<T::Read<'a>, Malformed> {
        r.align(Align::Eight)?;
        T::read_all(r)
    }
    #[cfg(feature = "alloc")]
    fn marshal(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        w.structure(|w| self.0.marshal_all(w))
    }
}

/// A notification's `image-data` hint, `(iiibiiay)`: width, height, bytes
/// per row, whether there is an alpha channel, bits per sample, channels,
/// and the pixels (`[[source: freedesktop-notification-spec]]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageData<'a> {
    /// Pixels across.
    pub width: I32,
    /// Pixels down.
    pub height: I32,
    /// Bytes from one row's start to the next's.
    pub rowstride: I32,
    /// Whether each pixel has an alpha sample.
    pub alpha: Boolean,
    /// Bits in each sample.
    pub bits_per_sample: I32,
    /// Samples in each pixel.
    pub channels: I32,
    /// The rows, top first.
    pub data: Bytes<'a>,
}

impl sealed::Value for ImageData<'_> {}

impl Marshal for ImageData<'_> {
    const ALIGN: Align = Align::Eight;
    const SIG: SigCodes = <Struct<(I32, I32, I32, Boolean, I32, I32, Bytes<'static>)>>::SIG;
    type Read<'a> = ImageData<'a>;
    fn read<'a>(r: &mut BodyReader<'a>) -> Result<ImageData<'a>, Malformed> {
        let (width, height, rowstride, alpha, bits_per_sample, channels, data) =
            <Struct<(I32, I32, I32, Boolean, I32, I32, Bytes<'static>)>>::read(r)?;
        Ok(ImageData {
            width,
            height,
            rowstride,
            alpha,
            bits_per_sample,
            channels,
            data,
        })
    }
    #[cfg(feature = "alloc")]
    fn marshal(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        let fields = (
            self.width,
            self.height,
            self.rowstride,
            self.alpha,
            self.bits_per_sample,
            self.channels,
            self.data,
        );
        Struct(fields).marshal(w)
    }
}

/// A VARIANT, `v`: the signature of one complete type, then a value of it
/// ("Container types"). Each arm carries a type whose signature is a
/// `const` single complete type, so a variant cannot hold a list of types
/// or bytes that disagree with what it says it holds.
///
/// There is no arm for a signature and bytes chosen by the caller:
///
/// ```compile_fail,E0599
/// use baud_domain::dbus::{Sig, Variant};
/// let _ = Variant::Raw(Sig::lit("ii"), &[]);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Body, I32, Variant};
/// let body = Body::of(&(Variant::I32(I32::new(-1)),)).expect("small");
/// assert_eq!(body.bytes(), &[1, b'i', 0, 0, 0xff, 0xff, 0xff, 0xff]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant<'a> {
    /// `y`.
    Byte(Byte),
    /// `b`.
    Boolean(Boolean),
    /// `s`.
    Str(Str<'a>),
    /// `u`.
    U32(U32),
    /// `i`.
    I32(I32),
    /// `d`.
    Double(Double),
    /// `as`.
    Strs(Array<'a, Str<'a>>),
    /// `(iiibiiay)`.
    Image(ImageData<'a>),
    /// `a{sv}`.
    Dict(Dict<'a, Str<'a>, Variant<'a>>),
}

impl Variant<'_> {
    /// The signature the variant writes before its value.
    pub const fn sig(&self) -> SigCodes {
        match self {
            Variant::Byte(_) => Byte::SIG,
            Variant::Boolean(_) => Boolean::SIG,
            Variant::Str(_) => Str::SIG,
            Variant::U32(_) => U32::SIG,
            Variant::I32(_) => I32::SIG,
            Variant::Double(_) => Double::SIG,
            Variant::Strs(_) => <Array<'_, Str<'_>>>::SIG,
            Variant::Image(_) => ImageData::SIG,
            Variant::Dict(_) => <Dict<'_, Str<'_>, Variant<'_>>>::SIG,
        }
    }
}

impl sealed::Value for Variant<'_> {}

impl Marshal for Variant<'_> {
    const ALIGN: Align = Align::One;
    const SIG: SigCodes = SigCodes::of(Sig::lit("v"));
    type Read<'a> = Unread<'a>;
    fn read<'a>(r: &mut BodyReader<'a>) -> Result<Unread<'a>, Malformed> {
        r.unread(Self::SIG.as_sig())
    }
    #[cfg(feature = "alloc")]
    fn marshal(&self, w: &mut BodyWriter<'_>) -> Result<(), EncodeError> {
        let sig = self.sig();
        w.variant(sig.as_sig(), |w| match self {
            Variant::Byte(v) => v.marshal(w),
            Variant::Boolean(v) => v.marshal(w),
            Variant::Str(v) => v.marshal(w),
            Variant::U32(v) => v.marshal(w),
            Variant::I32(v) => v.marshal(w),
            Variant::Double(v) => v.marshal(w),
            Variant::Strs(v) => v.marshal(w),
            Variant::Image(v) => v.marshal(w),
            Variant::Dict(v) => v.marshal(w),
        })
    }
}
