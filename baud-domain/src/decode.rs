use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// One unit pulled from the byte stream.
///
/// The arena is **not** a `str`. A PTY delivers `cat /bin/ls` as readily as
/// text, and escape sequences are not characters. So the stream decodes into
/// this rather than being validated up front — a terminal must render garbage,
/// not reject it.
///
/// Note what is missing: no variant exposes a byte count as a width. The only
/// route to [`Width`] is [`Decoded::width`], and `Control` and `Invalid` have
/// their widths fixed by the protocol rather than by their length.
pub enum Decoded {
    /// A grapheme cluster — base character plus any combining marks. The unit
    /// of wrapping: splitting one orphans a mark from its base.
    Grapheme { len: ByteLen, width: Width },
    /// Malformed UTF-8. Rendered as U+FFFD, one column, never dropped.
    Invalid { len: ByteLen },
    /// A C0/C1 control byte. Occupies no columns; may move the cursor by other
    /// means, which is the parser's business and not the cursor arithmetic's.
    Control { byte: u8 },
}

impl Decoded {
    /// The only constructor of a [`Width`] from stream content.
    pub fn width(&self) -> Width {
        match self {
            Decoded::Grapheme { width, .. } => *width,
            Decoded::Invalid { .. } => Width::NARROW,
            Decoded::Control { .. } => Width::ZERO,
        }
    }
    pub fn len(&self) -> ByteLen {
        match self {
            Decoded::Grapheme { len, .. } | Decoded::Invalid { len } => *len,
            Decoded::Control { .. } => ByteLen(1),
        }
    }
}

/// One byte as it arrived. Not a number: it has no arithmetic, only
/// classification. Everything that consumes wire data takes this.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct StreamByte(pub(crate) u8);

impl StreamByte {
    /// The inclusive bounds of a CSI final byte, `0x40..=0x7e`.
    ///
    /// Named constants rather than literals at the scanner, so the
    /// vectorised and scalar spellings of "is this a final byte" cannot
    /// drift apart -- which they have, once, and `states.rs` carries a
    /// test about it.
    pub const FINAL_LOW: StreamByte = StreamByte(0x40);
    pub const FINAL_HIGH: StreamByte = StreamByte(0x7e);

    pub const fn from_stream(b: u8) -> StreamByte {
        StreamByte(b)
    }
    pub fn is_printable(self) -> bool {
        (0x20..0x7f).contains(&self.0)
    }
    /// Equality, callable from a `const fn`.
    ///
    /// `PartialEq` is not a const trait yet, so a table built at compile time
    /// cannot write `b == StreamByte::ESC`. Without this the builder would
    /// compare raw bytes, which is exactly the untyped spelling the audit
    /// refuses -- and it would be a second definition of every control byte.
    pub const fn is(self, other: StreamByte) -> bool {
        self.0 == other.0
    }
    /// `7F`, which several states ignore rather than treat as a final.
    pub const fn is_del(self) -> bool {
        self.0 == 0x7f
    }
    /// A byte Latin-1 maps straight to the codepoint of the same value.
    pub const fn is_latin1_graphic(self) -> bool {
        self.0 >= 0xa0
    }
    /// `80..9F`: a C1 control, which outside UTF-8 introduces a sequence.
    pub const fn is_c1(self) -> bool {
        self.0 >= 0x80 && self.0 <= 0x9f
    }
    pub const fn is_c0(self) -> bool {
        self.0 < 0x20
    }
    pub fn is_continuation(self) -> bool {
        self.0 & 0xc0 == 0x80
    }
    pub fn as_ascii_digit(self) -> Option<Digit> {
        self.0.is_ascii_digit().then(|| Digit(self.0 - b'0'))
    }
    /// A byte `csi_param` accumulates rather than dispatches: a digit, or one
    /// of the two separators. The colon opens a sub-parameter and the
    /// semicolon a parameter, which is a difference the *action* cares about
    /// and the transition does not -- both stay in the same state.
    pub const fn is_csi_param_byte(self) -> bool {
        self.0.is_ascii_digit() || self.is(StreamByte::SEMICOLON) || self.is(StreamByte::COLON)
    }
    /// The same for a device control string, which has no sub-parameter form:
    /// `[[source: williams]]` sends a colon to `dcs_ignore`.
    pub const fn is_dcs_param_byte(self) -> bool {
        self.0.is_ascii_digit() || self.is(StreamByte::SEMICOLON)
    }
    /// For the atlas and the width tables, which index by codepoint.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
    /// Every stream byte, in order, so a compile-time table can enumerate them
    /// without a `u8` loop counter and an `as` cast at the boundary. The type
    /// that defines what a stream byte *is* is the one that should say how
    /// many there are.
    pub const ALL: [StreamByte; 256] = {
        let mut t = [StreamByte(0); 256];
        let mut i = 0;
        while i < 256 {
            t[i] = StreamByte(i as u8);
            i += 1;
        }
        t
    };
}

/// A decimal digit from a CSI parameter. Cannot be arithmetic'd with anything
/// else, and cannot exceed 9 by construction.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Digit(pub(crate) u8);

/// A CSI final byte, validated to the `0x40..=0x7e` range ECMA-48 allows.
/// Constructing one is the proof that a sequence terminated legally.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FinalByte(pub(crate) u8);

impl FinalByte {
    pub const fn from_stream(b: StreamByte) -> Option<FinalByte> {
        match b.0 >= 0x40 && b.0 <= 0x7e {
            true => Some(FinalByte(b.0)),
            false => None,
        }
    }
    pub(crate) fn raw(self) -> u8 {
        self.0
    }
    /// The byte, for an encoder that has to write it out.
    ///
    /// A `StreamByte` rather than a `u8`: it is going back onto the wire, and
    /// that is the type for a byte on the wire.
    /// `m`, the SGR final.
    ///
    /// Named rather than spelled `b'm'` at the call site: `Term::dispatch`
    /// takes a fast path for it before building anything, and that path has to
    /// stay in step with `Csi::parse_inner`'s arm. A constant is one place to
    /// change and one place to grep.
    pub const SGR: FinalByte = FinalByte(b'm');

    pub fn byte(self) -> StreamByte {
        StreamByte(self.0)
    }
}

impl StreamByte {
    pub const LF: StreamByte = StreamByte(b'\n');
    pub const CR: StreamByte = StreamByte(b'\r');
    pub const ESC: StreamByte = StreamByte(0x1b);
    pub const BS: StreamByte = StreamByte(0x08);
    pub const TAB: StreamByte = StreamByte(b'\t');
    pub const BEL: StreamByte = StreamByte(0x07);
    /// NUL. Discarded, never printed: `[[source: ecma48]]` § 8.3.88 makes it a
    /// fill character with no effect on the presentation.
    pub const NUL: StreamByte = StreamByte(0x00);
    /// DEL. `[[source: williams]]` gives it the `ignore` action inside escape
    /// and CSI sequences: it neither ends them nor contributes to them.
    pub const DEL: StreamByte = StreamByte(0x7f);
    /// `[[source: williams]]`'s "anywhere" block: CAN and SUB abandon whatever
    /// sequence is in progress, from any state. Named because `is_c0()` is
    /// `< 0x20` and therefore cannot distinguish them from a C0 to execute.
    /// The intermediate byte of `ESC SP F` / `ESC SP G` (S7C1T / S8C1T).
    /// `9C` — the 8-bit String Terminator, which exists only outside UTF-8.
    pub const ST_8BIT: StreamByte = StreamByte(0x9c);
    pub const SP: StreamByte = StreamByte(0x20);
    /// The intermediate byte of `ESC % @` / `ESC % G`.
    pub const PERCENT: StreamByte = StreamByte(0x25);
    pub const CAN: StreamByte = StreamByte(0x18);
    pub const SUB: StreamByte = StreamByte(0x1a);
    /// The intermediate of `ESC # 8` (DECALN).
    pub const HASH: StreamByte = StreamByte(b'#');
    /// VT. `[[source: ctlseqs]]` treats it as an index, like LF.
    pub const VT: StreamByte = StreamByte(0x0b);
    /// FF. Also an index -- a terminal does not eject a page.
    pub const FF: StreamByte = StreamByte(0x0c);
    /// SO / SI, the G1 and G0 shifts. Consumed so they are not printed; the
    /// charset they select is not implemented -- see docs/vt-coverage.md.
    pub const SO: StreamByte = StreamByte(0x0e);
    pub const SI: StreamByte = StreamByte(0x0f);
    pub const SEMICOLON: StreamByte = StreamByte(b';');

    /// The ISO 8613-6 sub-parameter separator. A *parameter* byte (0x3a), not
    /// an intermediate -- which is why it must be handled in the parameter
    /// loop and not by `is_csi_intermediate`.
    pub const COLON: StreamByte = StreamByte(b':');
    pub const QUESTION: StreamByte = StreamByte(b'?');
    /// `>`, the secondary CSI marker. A parameter byte like `?`, and a
    /// different namespace from it.
    pub const GT: StreamByte = StreamByte(b'>');
    pub const CSI_INTRO: StreamByte = StreamByte(b'[');
    pub const OSC_INTRO: StreamByte = StreamByte(b']');
    /// `ESC P`. Device Control String: a payload that must be consumed, never
    /// executed.
    pub const DCS_INTRO: StreamByte = StreamByte(b'P');
    /// `ESC X`, `ESC ^`, `ESC _`: SOS, PM, APC. Same rule as DCS.
    pub const SOS_INTRO: StreamByte = StreamByte(b'X');
    pub const PM_INTRO: StreamByte = StreamByte(b'^');
    pub const APC_INTRO: StreamByte = StreamByte(b'_');
    /// `ESC \`, the string terminator's final byte.
    pub const ST_FINAL: StreamByte = StreamByte(b'\\');
    pub const fn is_csi_intermediate(self) -> bool {
        self.0 >= 0x20 && self.0 < 0x30
    }
    /// Intermediate bytes after `ESC`, per `[[source: williams]]`'s
    /// `escape_intermediate`. Same range as the CSI intermediates: `ESC # 8` is
    /// DECALN, and without this the `#` is dropped and the `8` printed.
    pub const fn is_esc_intermediate(self) -> bool {
        self.0 >= 0x20 && self.0 < 0x30
    }
    /// A byte that ends an escape sequence: `0x30..=0x7e`.
    pub fn is_esc_final(self) -> bool {
        (0x30..0x7f).contains(&self.0)
    }
    /// Lead byte of a UTF-8 sequence: how many continuation bytes follow, and
    /// the payload bits. `None` for ASCII and for stray continuation bytes.
    /// **A load, not four range tests.** This is a branchy match evaluated once
    /// per non-ASCII character -- 81% of `cat-cjk.raw`'s bytes -- and it is the
    /// same shape as `ClusterBreak::decide`, which cost 8% until it became a
    /// table. A lead byte is one bounded input, so its answer is precomputable:
    /// 256 entries, low byte the continuation count with `0xff` for "not a
    /// lead", high byte the payload bits, and the shortest-form floor derived
    /// from the count.
    #[inline(always)]
    pub const fn utf8_lead(self) -> Option<(u8, u32, Utf8Min)> {
        let e = UTF8_LEAD[self.0 as usize];
        let rem = (e & 0xff) as u8;
        if rem == 0xff {
            return None;
        }
        Some((
            rem,
            (e >> 8) as u32,
            match rem {
                1 => Utf8Min::Two,
                2 => Utf8Min::Three,
                _ => Utf8Min::Four,
            },
        ))
    }
    /// `20..=7E`: a byte that stands for itself.
    pub fn is_printable_ascii(self) -> bool {
        (0x20..0x7f).contains(&self.0)
    }
    pub const fn utf8_continuation(self) -> Option<u32> {
        match self.0 & 0xc0 {
            0x80 => Some((self.0 & 0x3f) as u32),
            _ => None,
        }
    }
}

impl StreamByte {
    pub(crate) fn raw(self) -> u8 {
        self.0
    }
    /// For matching against ASCII literals in escape dispatch.
    pub const fn raw_byte(self) -> u8 {
        self.0
    }
    /// Reinterpret a byte slice as a stream. No copy and no `unsafe`:
    /// `repr(transparent)` plus zerocopy's `FromBytes` make this a checked
    /// no-op. This is the crate's one entry point for wire data.
    pub fn slice_from(b: &[u8]) -> &[StreamByte] {
        <[StreamByte]>::ref_from_bytes(b).expect("transparent over u8, always valid")
    }
    pub fn slice_as_bytes(s: &[StreamByte]) -> &[u8] {
        s.as_bytes()
    }
}

/// How many continuation bytes a UTF-8 sequence still needs. 1, 2 or 3 by
/// construction — never an arbitrary `u8` counter.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pending(pub(crate) u8);

impl Pending {
    pub fn new(n: u8) -> Pending {
        Pending(n.min(3))
    }
    /// How many continuation bytes are still owed. Needed because a table is
    /// indexed by the state, so the count has to be readable as one.
    pub fn owed(self) -> u8 {
        self.0
    }
    pub fn is_last(self) -> bool {
        self.0 == 1
    }
    pub fn dec(self) -> Pending {
        Pending(self.0.saturating_sub(1))
    }
}

/// A Unicode scalar value: `0..=0x10FFFF`, surrogates excluded.
///
/// Not a `u32`. `Width::of` and `Content::codepoint` both took one and could be
/// handed a lane mask, a byte count, or a packed colour with equal enthusiasm.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct Codepoint(pub(crate) u32);

impl Codepoint {
    /// A printable ASCII byte as a codepoint.
    ///
    /// `is_printable_ascii` has already established `0x20..0x7f`, so this
    /// cannot fail and does not re-check.
    #[inline(always)]
    pub fn ascii(b: StreamByte) -> Codepoint {
        debug_assert!(b.is_printable_ascii(), "not printable ASCII");
        Codepoint(u32::from(b.raw_byte()))
    }

    /// Decode one character from four gathered bytes and its length.
    ///
    /// **The validation is here, not in the vector code.** A block decoder
    /// gathers bytes without knowing what they mean; this is where a sequence
    /// becomes a codepoint or is refused. Refusing costs the caller a scalar
    /// retry, which is always correct, and keeps every conformance rule --
    /// shortest form, the surrogate hole, the U+10FFFF ceiling, continuations
    /// actually being continuations -- in one place rather than reimplemented
    /// per instruction set.
    ///
    /// `quad` holds the character's bytes little-endian, so byte zero is the
    /// lead. Lanes past `len` are zero and are not read.
    #[inline(always)]
    pub fn from_quad(quad: u32, len: ByteLen) -> Option<Codepoint> {
        let b = quad.to_le_bytes();
        let cont = |i: usize| (b[i] & 0xc0 == 0x80).then_some(u32::from(b[i] & 0x3f));
        let (v, min) = match len.index() {
            1 => match b[0] < 0x80 {
                true => (u32::from(b[0]), 0x0),
                false => return None,
            },
            2 => {
                let (rem, bits, _) = StreamByte(b[0]).utf8_lead()?;
                if rem != 1 {
                    return None;
                }
                ((bits << 6) | cont(1)?, 0x80)
            }
            3 => {
                let (rem, bits, _) = StreamByte(b[0]).utf8_lead()?;
                if rem != 2 {
                    return None;
                }
                ((bits << 12) | (cont(1)? << 6) | cont(2)?, 0x800)
            }
            4 => {
                let (rem, bits, _) = StreamByte(b[0]).utf8_lead()?;
                if rem != 3 {
                    return None;
                }
                (
                    (bits << 18) | (cont(1)? << 12) | (cont(2)? << 6) | cont(3)?,
                    0x10000,
                )
            }
            _ => return None,
        };
        // Shortest form: a value that would fit in fewer bytes is an overlong
        // encoding. `E0 80 AF` decoding to `/` is the path-traversal one, and
        // this repo has shipped that bug once already.
        if v < min {
            return None;
        }
        Codepoint::new(v)
    }

    /// Is this codepoint one byte in UTF-8, and therefore the business of the
    /// bulk ASCII scanner rather than of any decoder?
    pub fn is_ascii(self) -> bool {
        self.0 < 0x80
    }

    /// How many bytes this codepoint occupies in UTF-8.
    ///
    /// **Derivable because the decoder rejects overlong forms.**
    /// `Codepoint::from_quad` returns `None` when a value would fit in fewer
    /// bytes than it was spelled with -- `E0 80 AF` for `/` is the
    /// path-traversal one this repo has shipped once -- so for any codepoint
    /// that came out of a `DecodedBlock` the encoded length is a function of
    /// the value alone. That is what lets a block consumer stop part way
    /// through and still know where the next character starts, without the
    /// decoder carrying a parallel array of lengths.
    pub fn utf8_len(self) -> ByteLen {
        ByteLen::from_index(match self.0 {
            0x0000..0x0080 => 1,
            0x0080..0x0800 => 2,
            0x0800..0x1_0000 => 3,
            _ => 4,
        })
    }

    /// Is this codepoint unconditionally wide, ordinary, and in no lookback
    /// table -- so that both Unicode tries can be skipped?
    ///
    /// **Four range tests standing in for two two-stage tries.** The bulk wide
    /// collector asks `Width::of` and `GraphemeClass::of_with_lookback` per
    /// character. Over the three-byte space those answers are constant across
    /// four large runs, and this is them.
    ///
    /// The ranges are not hand-picked. They were found by walking the
    /// generated tables and taking the maximal runs where all three properties
    /// hold, and `the_wide_ordinary_ranges_agree_with_the_tables` re-derives
    /// that agreement over **every** codepoint from U+0800 to U+FFFF -- the
    /// whole of what three bytes can encode. That is the direction that
    /// matters: the predicate may be *incomplete*, and a codepoint it declines
    /// merely takes the slow path, but it must never be *wrong*, because a
    /// character it wrongly admits skips both tables and is written with the
    /// wrong width.
    ///
    /// Hiragana and katakana are two ranges rather than one because U+3099 and
    /// U+309A -- the combining voiced sound marks -- sit between them, and
    /// they are exactly the characters that must not skip the cluster machine.
    /// A codepoint already proven wide, ordinary and unlooked.
    ///
    /// The vectorised decoder range-tests four at once, so by the time it has
    /// them the general constructor's validity check is work it has already
    /// done. This is that knowledge spent. `debug_assert` rather than an
    /// `Option` because the caller is one function and the property is
    /// enumerated by a test.
    #[inline(always)]
    pub fn wide_ordinary(v: u32) -> Codepoint {
        debug_assert!(
            Codepoint(v).is_wide_ordinary(),
            "not a wide ordinary codepoint"
        );
        Codepoint(v)
    }

    #[inline(always)]
    pub fn is_wide_ordinary(self) -> bool {
        let v = self.0;
        // CJK from the enclosed forms through Yi, which is one run of 29,171.
        (0x329a..=0xa48c).contains(&v)
            // Hiragana, stopping before the two combining marks.
            || (0x3041..=0x3096).contains(&v)
            // Katakana, resuming after them.
            || (0x309b..=0x30ff).contains(&v)
            // Fullwidth forms.
            || (0xff01..=0xff60).contains(&v)
    }

    pub const REPLACEMENT: Codepoint = Codepoint(0xFFFD);
    pub const SPACE: Codepoint = Codepoint(0x20);

    /// The only fallible constructor. Rejects surrogates and out-of-range —
    /// both of which a UTF-8 decoder can produce from malformed input.
    pub fn new(v: u32) -> Option<Codepoint> {
        (v <= 0x10_FFFF && !(0xD800..0xE000).contains(&v)).then_some(Codepoint(v))
    }
    /// From a decoder that has already validated. Malformed input becomes
    /// U+FFFD rather than an invalid `Codepoint`, so the type's invariant holds
    /// without the caller having to branch.
    pub fn decoded(v: u32) -> Codepoint {
        Codepoint::new(v).unwrap_or(Codepoint::REPLACEMENT)
    }
    /// From a Rust `char`, which is by definition already a scalar value.
    ///
    /// Total and lossless, so no `Option`: `char` cannot hold a surrogate or a
    /// value above U+10FFFF. Exists so callers outside this crate do not write
    /// `c as u32` -- the cast belongs where the newtype is, which is here.
    pub fn from_char(c: char) -> Codepoint {
        Codepoint(c as u32)
    }
    pub fn from_ascii(b: StreamByte) -> Codepoint {
        Codepoint(b.raw() as u32)
    }
    pub fn value(self) -> u32 {
        self.0
    }
}

/// The smallest codepoint a UTF-8 sequence of a given length may encode.
///
/// **This is `[[source: unicode-ch3]]` Table 3-7's second-byte column, said as
/// the constraint it exists to enforce.** The table narrows the `80..BF`
/// continuation pattern for exactly four lead bytes — `E0`, `ED`, `F0`, `F4` —
/// and the standard notes the exceptions "occur only in the second byte of a
/// sequence". What all four encode is one rule: a sequence must be the
/// *shortest* form of the value it represents.
///
/// baud implemented only the first-byte column, so every overlong three- and
/// four-byte form decoded to a real character. `E0 80 AF` rendered `/` — the
/// canonical path-traversal overlong, which a filter checking for a slash would
/// not have seen. `Codepoint::new` could not catch it, structurally: a
/// non-shortest form still yields a legal scalar value, so nothing is wrong
/// with the *result*, only with the encoding of it.
///
/// Carried with the sequence rather than derived from the bytes remaining,
/// because `Pending` cannot tell "second byte of three" from "third byte of
/// four" — both have two continuations left. Deriving it from `Pending` was the
/// first attempt and it rejected `F0 90 80 80`, which is U+10000.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Utf8Min {
    /// `C2..DF`: two bytes, so at least U+0080.
    Two,
    /// `E0..EF`: three bytes, so at least U+0800.
    Three,
    /// `F0..F4`: four bytes, so at least U+10000.
    Four,
}

impl Utf8Min {
    pub fn floor(self) -> u32 {
        match self {
            Utf8Min::Two => 0x80,
            Utf8Min::Three => 0x800,
            Utf8Min::Four => 0x1_0000,
        }
    }
}

/// Codepoint bits accumulated mid-sequence, *before* they are known to be a
/// valid scalar value. Deliberately not a [`Codepoint`] — a partial sequence
/// has not earned that type yet, and calling it one would be a lie the width
/// tables would then believe.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Utf8Acc(pub(crate) u32);

impl Utf8Acc {
    pub fn lead(bits: u32) -> Utf8Acc {
        Utf8Acc(bits)
    }
    /// Combine a three-byte sequence as a tree rather than as a chain.
    ///
    /// **The same value, half the dependency depth.** `push` is
    /// `(acc << 6) | cont`, so three bytes accumulate in three *dependent*
    /// steps inside a loop whose bound is only known at run time, which stops
    /// it unrolling. Written out, the terms are independent and the ors are a
    /// tree of depth two, and the three byte loads can issue together.
    ///
    /// It matters because three-byte sequences are **81% of `cat-cjk.raw`**
    /// and the slow path is ~64% decode by a ceiling arm -- and because this
    /// loop is latency-bound, where shortening a chain is the only thing that
    /// has ever helped. Vectorising the decode *lengthened* it and lost twice.
    pub fn three(bits: u32, c1: u32, c2: u32) -> Utf8Acc {
        Utf8Acc((bits << 12) | (c1 << 6) | c2)
    }
    pub fn push(self, cont: u32) -> Utf8Acc {
        Utf8Acc((self.0 << 6) | cont)
    }
    /// The only exit. Malformed input becomes U+FFFD rather than an invalid
    /// codepoint, so the invariant holds without the caller branching.
    ///
    /// A value below `min.floor()` is a non-shortest form and is **ill-formed**
    /// even though it is a perfectly legal scalar value — see [`Utf8Min`].
    /// Conformance clause C10 requires it be treated as an error rather than
    /// interpreted as the character it spells.
    pub fn finish(self, min: Utf8Min) -> Codepoint {
        if self.0 < min.floor() {
            return Codepoint::REPLACEMENT;
        }
        Codepoint::decoded(self.0)
    }
}

/// How incoming bytes are interpreted as characters.
///
/// `[[source: ctlseqs]]`: `ESC % G` is "Select UTF-8 character set, ISO 2022"
/// and `ESC % @` is "Select default character set. That is ISO 8859-1 (ISO
/// 2022)." An application may switch at any point in the stream, so this is
/// live state rather than startup configuration.
///
/// An enum and not a `bool`, for the reason `baud-domain` is gated at zero bool
/// fields: `utf8: true` does not say what the alternative is, and there is
/// more than one alternative in principle.
///
/// xterm's `utf8` resource is four-valued and only an explicitly configured
/// `always` makes these two sequences inert — by default xterm honours them,
/// which is what baud does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputEncoding {
    /// Multi-byte, and the default. Bytes `80..FF` are lead and continuation
    /// bytes, so 8-bit C1 controls are unusable — see `docs/vt-coverage.md`.
    #[default]
    Utf8,
    /// ISO 8859-1: every byte is one character, `00..FF` mapping to
    /// `U+0000..U+00FF`. Single-byte and therefore fixed-width, which is the
    /// property [`Encoding::is_byte_per_column`] names.
    Latin1,
}

impl InputEncoding {
    /// The UTF-8 byte-order mark, `EF BB BF`.
    ///
    /// Unicode calls it a signature when it appears at the start of a stream.
    /// Mid-stream the same codepoint is `U+FEFF` ZERO WIDTH NO-BREAK SPACE and
    /// is text, which is why this is only meaningful at offset zero.
    pub const BOM: [u8; 3] = [0xef, 0xbb, 0xbf];

    /// Which encoding a sample is in, or `None` when the sample cannot say.
    ///
    /// **It reports; it does not switch.** Nothing in baud calls this — an
    /// embedder does, and decides. Guessing an encoding wrong corrupts text
    /// silently and gives the application no way to find out, which is the
    /// failure this project refuses everywhere else, so the guess is not
    /// baud's to make. xterm makes it a resource for the same reason.
    ///
    /// The three answers, and why the third is not a cop-out:
    ///
    /// * A UTF-8 BOM at offset zero is a signature, and decisive.
    /// * A byte sequence that is ill-formed UTF-8 rules UTF-8 out. ISO 8859-1
    ///   has no ill-formed bytes at all -- every one of the 256 is a character
    ///   -- so it is what remains.
    /// * A sample of nothing but ASCII returns **`None`**, because the two
    ///   encodings agree on every byte below `0x80`. There is no evidence
    ///   either way, and answering anyway would be inventing one. This is the
    ///   common case for a terminal, and it is why detection cannot be a
    ///   default: most of what a terminal sees decides nothing.
    pub fn detect(sample: &[u8]) -> Option<InputEncoding> {
        if sample.starts_with(&InputEncoding::BOM) {
            return Some(InputEncoding::Utf8);
        }
        let mut saw_multibyte = false;
        let mut i = 0;
        while i < sample.len() {
            let b = StreamByte::from_stream(sample[i]);
            let Some((rem, acc, min)) = b.utf8_lead() else {
                if sample[i] < 0x80 {
                    i += 1;
                    continue;
                }
                // A lead byte UTF-8 has no encoding for, or a stray
                // continuation. Latin-1 reads it as a character.
                return Some(InputEncoding::Latin1);
            };
            let n = rem as usize;
            if i + n >= sample.len() {
                // Truncated at the end of the sample: not evidence either way.
                break;
            }
            let mut v = Utf8Acc::lead(acc);
            for k in 1..=n {
                match StreamByte::from_stream(sample[i + k]).utf8_continuation() {
                    Some(bits) => v = v.push(bits),
                    None => return Some(InputEncoding::Latin1),
                }
            }
            if v.finish(min) == Codepoint::REPLACEMENT {
                // Overlong, surrogate, or out of range -- ill-formed UTF-8.
                return Some(InputEncoding::Latin1);
            }
            saw_multibyte = true;
            i += n + 1;
        }
        saw_multibyte.then_some(InputEncoding::Utf8)
    }

    /// The character a byte stands for, in a single-byte encoding.
    ///
    /// `None` for `80..9F`, which ISO 8859-1 reserves for C1 controls. baud
    /// does not act on 8-bit C1 — it cannot, having chosen UTF-8 as the other
    /// mode — so they are ignored rather than printed, which is what xterm's
    /// `CASE_IGNORE` does and what ghostty does citing xterm.
    /// The 7-bit escape a C1 byte stands for, in a single-byte encoding.
    ///
    /// **Only meaningful outside UTF-8**, which is the whole point. In UTF-8
    /// `9B` is a continuation byte and cannot be a control -- that is why
    /// `docs/vt-coverage.md` lists 8-bit C1 as out. In ISO 8859-1 it is
    /// unambiguously CSI, and `[[source: ctlseqs]]` describes the two forms as
    /// equivalent: `ESC [` is "a 7-bit equivalent to 8-bit" `9B`.
    ///
    /// So the refusal was scoped to the encoding without saying so. baud is not
    /// always in UTF-8 any more.
    ///
    /// Returns the byte the sequence would carry after `ESC`, so one table
    /// serves both forms and they cannot drift.
    pub fn c1_as_escape(self, b: StreamByte) -> Option<StreamByte> {
        if self == InputEncoding::Utf8 {
            return None;
        }
        let v = b.raw_byte();
        // C1 occupies `80..9F`, and its 7-bit form is `ESC` plus the byte
        // shifted down by 0x40: `9B` is `ESC [`, `9D` is `ESC ]`.
        (0x80..0xa0)
            .contains(&v)
            .then(|| StreamByte::from_stream(v - 0x40))
    }

    pub fn single_byte(self, b: StreamByte) -> Option<Codepoint> {
        match self {
            InputEncoding::Utf8 => None,
            InputEncoding::Latin1 => {
                let v = b.raw_byte();
                (!(0x80..0xa0).contains(&v)).then_some(Codepoint(v as u32))
            }
        }
    }
}

/// Low byte: continuation count, `0xff` when the byte cannot lead. High byte:
/// the lead's payload bits. Built at compile time from the same ranges the
/// match used, and `the_lead_table_matches_the_ranges` checks all 256.
const UTF8_LEAD: [u16; 256] = build_lead_table();

const fn build_lead_table() -> [u16; 256] {
    let mut t = [0x00ffu16; 256];
    let mut b = 0usize;
    while b < 256 {
        let v = b as u8;
        t[b] = match v {
            0xc2..=0xdf => 1 | (((v & 0x1f) as u16) << 8),
            0xe0..=0xef => 2 | (((v & 0x0f) as u16) << 8),
            0xf0..=0xf4 => 3 | (((v & 0x07) as u16) << 8),
            _ => 0x00ff,
        };
        b += 1;
    }
    t
}
