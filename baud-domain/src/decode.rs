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

/// Sum of grapheme widths across a logical line. The only route from a byte
/// range to a [`DisplayWidth`], so a cached width can never have been computed
/// from a byte count.
pub fn measure(units: impl Iterator<Item = Decoded>, _t: &WidthTable) -> DisplayWidth {
    DisplayWidth::from_cells(units.map(|d| d.width().cells() as u32).sum())
}

/// One byte as it arrived. Not a number: it has no arithmetic, only
/// classification. Everything that consumes wire data takes this.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct StreamByte(pub(crate) u8);

impl StreamByte {
    pub fn from_stream(b: u8) -> StreamByte {
        StreamByte(b)
    }
    pub fn is_printable(self) -> bool {
        (0x20..0x7f).contains(&self.0)
    }
    pub fn is_c0(self) -> bool {
        self.0 < 0x20
    }
    pub fn is_continuation(self) -> bool {
        self.0 & 0xc0 == 0x80
    }
    pub fn as_ascii_digit(self) -> Option<Digit> {
        self.0.is_ascii_digit().then(|| Digit(self.0 - b'0'))
    }
    /// For the atlas and the width tables, which index by codepoint.
    pub fn index(self) -> usize {
        self.0 as usize
    }
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
    pub fn from_stream(b: StreamByte) -> Option<FinalByte> {
        (0x40..=0x7e).contains(&b.0).then_some(FinalByte(b.0))
    }
    pub(crate) fn raw(self) -> u8 {
        self.0
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
    pub const SP: StreamByte = StreamByte(0x20);
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
    pub fn is_csi_intermediate(self) -> bool {
        (0x20..0x30).contains(&self.0)
    }
    /// Intermediate bytes after `ESC`, per `[[source: williams]]`'s
    /// `escape_intermediate`. Same range as the CSI intermediates: `ESC # 8` is
    /// DECALN, and without this the `#` is dropped and the `8` printed.
    pub fn is_esc_intermediate(self) -> bool {
        (0x20..0x30).contains(&self.0)
    }
    /// A byte that ends an escape sequence: `0x30..=0x7e`.
    pub fn is_esc_final(self) -> bool {
        (0x30..0x7f).contains(&self.0)
    }
    /// Lead byte of a UTF-8 sequence: how many continuation bytes follow, and
    /// the payload bits. `None` for ASCII and for stray continuation bytes.
    pub fn utf8_lead(self) -> Option<(u8, u32)> {
        match self.0 {
            0xc2..=0xdf => Some((1, (self.0 & 0x1f) as u32)),
            0xe0..=0xef => Some((2, (self.0 & 0x0f) as u32)),
            0xf0..=0xf4 => Some((3, (self.0 & 0x07) as u32)),
            _ => None,
        }
    }
    pub fn utf8_continuation(self) -> Option<u32> {
        (self.0 & 0xc0 == 0x80).then_some((self.0 & 0x3f) as u32)
    }
}

impl StreamByte {
    pub(crate) fn raw(self) -> u8 {
        self.0
    }
    /// For matching against ASCII literals in escape dispatch.
    pub fn raw_byte(self) -> u8 {
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
    pub fn from_ascii(b: StreamByte) -> Codepoint {
        Codepoint(b.raw() as u32)
    }
    pub fn value(self) -> u32 {
        self.0
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
    pub fn push(self, cont: u32) -> Utf8Acc {
        Utf8Acc((self.0 << 6) | cont)
    }
    /// The only exit. Malformed input becomes U+FFFD rather than an invalid
    /// codepoint, so the invariant holds without the caller branching.
    pub fn finish(self) -> Codepoint {
        Codepoint::decoded(self.0)
    }
}
