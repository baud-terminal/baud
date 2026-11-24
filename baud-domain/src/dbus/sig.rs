//! Signatures: the type codes that say what a block of values holds
//! (`[[source: dbus-spec]]`, "Type System", "Valid Signatures" and "Container
//! types").
//!
//! [`Sig`] borrows a signature that has been checked against every rule;
//! [`SigCodes`] owns one inline, so a marshalled type can name its signature
//! in a `const`. Both are built only through the check, so a signature in
//! hand is a valid one.

use core::fmt;

/// How many codes a signature holds, `0..=255`: the spec's maximum length,
/// and the one byte a SIGNATURE value's length is marshalled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SigLen(u8);

impl SigLen {
    /// No codes: the empty signature, an empty body's.
    pub const ZERO: SigLen = SigLen(0);
    /// The most codes a signature holds.
    pub const MAX: SigLen = SigLen(255);

    /// The count, as the byte a SIGNATURE value's length is written in.
    #[inline]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// How deeply containers nest: arrays and structs are each bounded at
/// [`Depth::NESTING`], so together a signature nests at most
/// [`Depth::TOTAL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Depth(u8);

impl Depth {
    /// Outside every container.
    pub const ZERO: Depth = Depth(0);
    /// The most array codes, and separately the most open parentheses, a
    /// signature nests.
    pub const NESTING: Depth = Depth(32);
    /// The most containers of all kinds a value nests, variants included.
    pub const TOTAL: Depth = Depth(64);

    /// The depth as a count.
    #[inline]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// One container deeper, or `None` past `limit`.
    #[inline]
    pub const fn enter(self, limit: Depth) -> Option<Depth> {
        if self.0 < limit.0 {
            Some(Depth(self.0 + 1))
        } else {
            None
        }
    }
}

/// Why a byte string is not a signature: each variant is one rule of
/// "Valid Signatures" or "Container types".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SigError {
    /// More than 255 codes.
    TooLong,
    /// A byte that is not a type code a signature may carry: the reserved
    /// `r`, `e`, `m`, `*`, `?`, `@`, `&` and `^`, NUL, and anything else.
    Code,
    /// An `a` with no element type after it.
    ArrayNoElement,
    /// A `(` with no `)`.
    StructUnclosed,
    /// A `)` or `}` that closes nothing.
    CloseUnopened,
    /// `()`: a struct holds at least one type.
    EmptyStruct,
    /// A `{` that is not an array's element type.
    DictOutsideArray,
    /// A dict entry whose key is not a basic type.
    DictKey,
    /// A dict entry without exactly a key and one value.
    DictArity,
    /// More than 32 arrays nested.
    ArraysTooDeep,
    /// More than 32 structs nested.
    StructsTooDeep,
}

impl SigError {
    /// The rule broken, as a sentence; also the panic of a refused
    /// [`Sig::lit`] or [`SigCodes`] constructor.
    pub const fn rule(self) -> &'static str {
        match self {
            SigError::TooLong => "a signature is at most 255 codes",
            SigError::Code => "a signature carries only type codes, parentheses and braces",
            SigError::ArrayNoElement => "an array code is followed by one complete type",
            SigError::StructUnclosed => "a struct's open parenthesis is closed",
            SigError::CloseUnopened => "a closing parenthesis or brace closes an open one",
            SigError::EmptyStruct => "a struct holds at least one type",
            SigError::DictOutsideArray => "a dict entry is only an array's element type",
            SigError::DictKey => "a dict entry's key is a basic type",
            SigError::DictArity => "a dict entry holds a key and exactly one value",
            SigError::ArraysTooDeep => "a signature nests at most 32 arrays",
            SigError::StructsTooDeep => "a signature nests at most 32 structs",
        }
    }
}

impl fmt::Display for SigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rule())
    }
}

/// A basic type's code: a fixed type or a string-like one ("Basic types").
const fn basic(c: u8) -> bool {
    matches!(
        c,
        b'y' | b'b' | b'n' | b'q' | b'i' | b'u' | b'x' | b't' | b'd' | b'h' | b's' | b'o' | b'g'
    )
}

/// The end of the single complete type that starts at `i`, `arrays` and
/// `structs` deep.
const fn single(s: &[u8], i: usize, arrays: Depth, structs: Depth) -> Result<usize, SigError> {
    if i >= s.len() {
        return Err(SigError::ArrayNoElement);
    }
    let c = s[i];
    if basic(c) || c == b'v' {
        return Ok(i + 1);
    }
    match c {
        b'a' => {
            let Some(arrays) = arrays.enter(Depth::NESTING) else {
                return Err(SigError::ArraysTooDeep);
            };
            if i + 1 >= s.len() || matches!(s[i + 1], b')' | b'}') {
                Err(SigError::ArrayNoElement)
            } else if s[i + 1] == b'{' {
                dict_entry(s, i + 2, arrays, structs)
            } else {
                single(s, i + 1, arrays, structs)
            }
        }
        b'(' => {
            let Some(structs) = structs.enter(Depth::NESTING) else {
                return Err(SigError::StructsTooDeep);
            };
            let mut j = i + 1;
            if j < s.len() && s[j] == b')' {
                return Err(SigError::EmptyStruct);
            }
            while j < s.len() && s[j] != b')' {
                j = match single(s, j, arrays, structs) {
                    Ok(j) => j,
                    Err(e) => return Err(e),
                };
            }
            if j >= s.len() {
                return Err(SigError::StructUnclosed);
            }
            Ok(j + 1)
        }
        b'{' => Err(SigError::DictOutsideArray),
        b')' | b'}' => Err(SigError::CloseUnopened),
        _ => Err(SigError::Code),
    }
}

/// The end of the dict entry whose key is at `i`, just past its `{`.
const fn dict_entry(s: &[u8], i: usize, arrays: Depth, structs: Depth) -> Result<usize, SigError> {
    if i >= s.len() || s[i] == b'}' {
        return Err(SigError::DictArity);
    }
    if !basic(s[i]) {
        return match s[i] {
            b'a' | b'(' | b'{' | b'v' => Err(SigError::DictKey),
            b')' => Err(SigError::CloseUnopened),
            _ => Err(SigError::Code),
        };
    }
    if i + 1 >= s.len() || s[i + 1] == b'}' {
        return Err(SigError::DictArity);
    }
    let j = match single(s, i + 1, arrays, structs) {
        Ok(j) => j,
        Err(e) => return Err(e),
    };
    if j >= s.len() || s[j] != b'}' {
        return Err(SigError::DictArity);
    }
    Ok(j + 1)
}

/// Checks `s` is a list of single complete types under every rule.
const fn check(s: &[u8]) -> Result<SigLen, SigError> {
    if s.len() > SigLen::MAX.0 as usize {
        return Err(SigError::TooLong);
    }
    let mut i = 0;
    while i < s.len() {
        i = match single(s, i, Depth::ZERO, Depth::ZERO) {
            Ok(i) => i,
            Err(e) => return Err(e),
        };
    }
    Ok(SigLen(s.len() as u8))
}

/// Panics, in const evaluation a compile error, with the rule `e` names.
const fn refuse(e: SigError) -> ! {
    panic!("{}", e.rule())
}

/// A signature, borrowed and checked: zero or more single complete types,
/// at most 255 codes, at most 32 arrays and 32 structs nested, no reserved
/// code, no empty struct, every dict entry an array's element with a basic
/// key and one value (`[[source: dbus-spec]]`, "Valid Signatures").
///
/// A signature that breaks a rule does not compile as a literal; 33 nested
/// arrays are one too many:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::Sig;
/// const S: Sig<'static> = Sig::lit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaai");
/// ```
///
/// and a struct holds at least one type:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::Sig;
/// const S: Sig<'static> = Sig::lit("()");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Sig, SigError};
/// const S: Sig<'static> = Sig::lit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaai");
/// const P: Sig<'static> = Sig::lit("(i(ii))");
/// assert_eq!(S.len().get(), 33);
/// assert_eq!(P.as_bytes(), b"(i(ii))");
/// assert_eq!(Sig::parse(b"()"), Err(SigError::EmptyStruct));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sig<'a>(&'a [u8]);

impl<'a> Sig<'a> {
    /// The empty signature.
    pub const EMPTY: Sig<'static> = Sig(b"");

    /// `s` as a signature, or the rule it breaks.
    pub const fn parse(s: &'a [u8]) -> Result<Sig<'a>, SigError> {
        match check(s) {
            Ok(_) => Ok(Sig(s)),
            Err(e) => Err(e),
        }
    }

    /// The codes.
    #[inline]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.0
    }

    /// How many codes.
    #[inline]
    pub const fn len(&self) -> SigLen {
        SigLen(self.0.len() as u8)
    }

    /// Whether there are no codes: an empty body's signature.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the signature is exactly one complete type, as a variant's
    /// and an array element's are.
    pub const fn is_single(&self) -> bool {
        !self.0.is_empty()
            && matches!(single(self.0, 0, Depth::ZERO, Depth::ZERO), Ok(n) if n == self.0.len())
    }
}

impl Sig<'static> {
    /// `s` as a signature; a signature that breaks a rule panics, which in
    /// a `const` is a compile error.
    pub const fn lit(s: &'static str) -> Sig<'static> {
        match Sig::parse(s.as_bytes()) {
            Ok(sig) => sig,
            Err(e) => refuse(e),
        }
    }
}

impl fmt::Debug for Sig<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match core::str::from_utf8(self.0) {
            Ok(s) => write!(f, "Sig({s:?})"),
            Err(_) => write!(f, "Sig({:?})", self.0),
        }
    }
}

/// A signature held inline, so a type can name its own in a `const`: 255
/// code bytes and their count, the tail past the count all NUL.
///
/// Every constructor checks its result and panics on a rule broken, which
/// in a `const` is a compile error; 196 codes and 196 more are past 255:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::{Sig, SigCodes};
/// const I: SigCodes = SigCodes::of(Sig::lit("iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiii"));
/// const S: SigCodes = I.concat(I);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Sig, SigCodes};
/// const S: SigCodes = SigCodes::of(Sig::lit("s"));
/// const HINTS: SigCodes = SigCodes::dict(S, SigCodes::of(Sig::lit("v")));
/// const ARGS: SigCodes = S.concat(SigCodes::array(S)).concat(HINTS);
/// const _: () = assert!(SigCodes::eq(ARGS, Sig::lit("sasa{sv}")));
/// ```
#[derive(Clone, Copy)]
pub struct SigCodes {
    codes: [u8; 255],
    len: SigLen,
}

impl SigCodes {
    /// No codes.
    pub const EMPTY: SigCodes = SigCodes {
        codes: [0; 255],
        len: SigLen::ZERO,
    };

    /// `codes` with `n` of them used, checked.
    const fn checked(codes: [u8; 255], n: usize) -> SigCodes {
        let (used, _) = codes.split_at(n);
        match check(used) {
            Ok(len) => SigCodes { codes, len },
            Err(e) => refuse(e),
        }
    }

    /// `sig`'s codes, held inline.
    pub const fn of(sig: Sig<'_>) -> SigCodes {
        let mut codes = [0; 255];
        let mut i = 0;
        while i < sig.0.len() {
            codes[i] = sig.0[i];
            i += 1;
        }
        SigCodes::checked(codes, sig.0.len())
    }

    /// These codes then `next`'s; panics past 255.
    pub const fn concat(self, next: SigCodes) -> SigCodes {
        let (a, b) = (self.len.0 as usize, next.len.0 as usize);
        if a + b > SigLen::MAX.0 as usize {
            refuse(SigError::TooLong);
        }
        let mut codes = self.codes;
        let mut i = 0;
        while i < b {
            codes[a + i] = next.codes[i];
            i += 1;
        }
        SigCodes::checked(codes, a + b)
    }

    /// An array of `element`, `a` then its codes; panics unless `element`
    /// is one complete type.
    pub const fn array(element: SigCodes) -> SigCodes {
        if !element.as_sig().is_single() {
            panic!("an array's element is one complete type");
        }
        SigCodes::framed(b"a", element, b"")
    }

    /// A struct of `fields`, in parentheses; panics if there are none.
    pub const fn structure(fields: SigCodes) -> SigCodes {
        if fields.len.0 == 0 {
            refuse(SigError::EmptyStruct);
        }
        SigCodes::framed(b"(", fields, b")")
    }

    /// An array of dict entries, `a{` key value `}`; panics unless `key` is
    /// one basic type and `value` one complete type.
    pub const fn dict(key: SigCodes, value: SigCodes) -> SigCodes {
        if key.len.0 != 1 || !basic(key.codes[0]) {
            refuse(SigError::DictKey);
        }
        if !value.as_sig().is_single() {
            refuse(SigError::DictArity);
        }
        SigCodes::framed(b"a{", key.concat(value), b"}")
    }

    /// `open`, `inner`'s codes, then `close`; checked.
    const fn framed(open: &[u8], inner: SigCodes, close: &[u8]) -> SigCodes {
        let n = inner.len.0 as usize;
        let total = open.len() + n + close.len();
        if total > SigLen::MAX.0 as usize {
            refuse(SigError::TooLong);
        }
        let mut codes = [0; 255];
        let mut i = 0;
        while i < open.len() {
            codes[i] = open[i];
            i += 1;
        }
        i = 0;
        while i < n {
            codes[open.len() + i] = inner.codes[i];
            i += 1;
        }
        i = 0;
        while i < close.len() {
            codes[open.len() + n + i] = close[i];
            i += 1;
        }
        SigCodes::checked(codes, total)
    }

    /// The codes as a signature.
    #[inline]
    pub const fn as_sig(&self) -> Sig<'_> {
        Sig(self.codes.split_at(self.len.0 as usize).0)
    }

    /// How many codes.
    #[inline]
    pub const fn len(&self) -> SigLen {
        self.len
    }

    /// Whether there are no codes.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len.0 == 0
    }

    /// Whether `codes` spell `sig`, usable in a `const` assertion.
    pub const fn eq(codes: SigCodes, sig: Sig<'_>) -> bool {
        let (a, b) = (codes.as_sig().0, sig.0);
        if a.len() != b.len() {
            return false;
        }
        let mut i = 0;
        while i < a.len() {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl PartialEq for SigCodes {
    fn eq(&self, other: &SigCodes) -> bool {
        self.as_sig() == other.as_sig()
    }
}

impl Eq for SigCodes {}

impl fmt::Debug for SigCodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SigCodes({:?})", self.as_sig())
    }
}
