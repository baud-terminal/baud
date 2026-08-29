#![no_std]

use baud_domain::*;

mod portable;

pub use portable::*;
// Gated to match the definitions in `avx2.rs`: an unconditional re-export of a
// `#[cfg(target_arch = "x86_64")]` item resolves to nothing elsewhere, and the
// host build never notices because the cfg is off there too. The bare-metal
// `x86_64-unknown-none` gate is what catches it.

/// A scan backend.
///
/// The three scans below are the *only* ISA-dependent code in the parser. Every
/// fast path is written once against this trait and monomorphised per backend,
/// so the portable and accelerated feeds cannot diverge in logic — which is the
/// bug this codebase has already had once, when `ESC 7`/`8`/`M` were
/// implemented in the AVX2 path only and `feed` silently disagreed with
/// `feed_avx2`.
///
/// # Safety
/// An implementation may require a target feature. The caller establishes it
/// once, at startup; `no_std` has no runtime feature detection.
/// **Domain-typed on both sides, deliberately.**
///
/// The input is `&[StreamByte]` and not `&[u8]` because that is the whole
/// reason this crate is separable: an instruction-set implementation is
/// reusable only if its surface cannot be got wrong, and a raw byte slice is
/// the surface that can. The output is `ByteLen` and the mask inside is a
/// `LaneMask` whose first set bit is a `Lane`, so a bitmask cannot be added to
/// a byte offset -- which compiles, and is why those newtypes exist.
///
/// Conversion at the boundary is free: `StreamByte` is `repr(transparent)`
/// over `u8`, so `slice_as_bytes` is what the vector loads take and costs
/// nothing.
pub trait Scan {
    /// Length of the leading printable-ASCII run.
    /// # Safety
    /// The caller must have established every target feature the chosen
    /// implementation requires -- `Portable` requires none, `Avx2` requires
    /// AVX2. `no_std` has no `is_x86_feature_detected!`, so this is the
    /// caller's decision made once at startup rather than per call.
    unsafe fn run(buf: &[StreamByte]) -> ByteLen;
    /// Offset of the first CSI final byte (`0x40..=0x7e`).
    /// # Safety
    /// The caller must have established every target feature the chosen
    /// implementation requires -- `Portable` requires none, `Avx2` requires
    /// AVX2. `no_std` has no `is_x86_feature_detected!`, so this is the
    /// caller's decision made once at startup rather than per call.
    unsafe fn final_byte(buf: &[StreamByte]) -> Option<ByteLen>;
    /// Offset of the first BEL, or the ESC of an ESC-backslash terminator.
    /// # Safety
    /// The caller must have established every target feature the chosen
    /// implementation requires -- `Portable` requires none, `Avx2` requires
    /// AVX2. `no_std` has no `is_x86_feature_detected!`, so this is the
    /// caller's decision made once at startup rather than per call.
    unsafe fn st(buf: &[StreamByte]) -> Option<ByteLen>;
}

/// Lane width. 16 bytes is what every SIMD ISA this targets has natively;
/// asking `core::simd` for more emits multiple registers per step and measured
/// no better.
///
/// **Re-tested for `run` specifically, which was scalar until it was
/// vectorised and so was never covered by the sentence above.** 32 lanes
/// measured *worse* on the streaming payload -- 1313/1374/1338 against
/// 1342/1410/1396 -- and flat on btop. Two NEON registers per step buys
/// nothing when the run being scanned averages ~32 bytes, because the loop
/// exits in its first or second iteration either way.
///
/// **aarch64 was the platform that had been paying for the scalar loop.**
/// x86-64 with AVX2 has always had `scan_avx2`, 32 bytes a step; the scalar
/// `run` was in `Portable`, which is what aarch64 uses -- so the machine this
/// project is developed and measured on was the one running the slow path.
pub const LANES: usize = 16;

/// How many codepoints one `Decode::block` call produces at most.
///
/// Four, because that is what one 16-byte load yields for three-byte text --
/// the case a CJK stream is made of -- and because four `u32` lanes is one
/// vector on every target baud builds for.
pub const DECODE_LANES: usize = 4;

/// The result of decoding one block of bytes into codepoints.
///
/// `DecodedBlock` and not `Decoded`: the domain already has a `Decoded` for a
/// single character. Two names for two things, checked before adding rather
/// than discovered by the compiler -- which is how `Encoding` and `ReadAs`
/// went, one collision earlier.
///
/// **Codepoints only -- decoding does not classify.** An earlier version also
/// returned widths, which meant a `Width::of` trie lookup per character: two
/// dependent loads that the path it was replacing does not do at all, because
/// `is_wide_ordinary` answers the same question in four range compares. That
/// alone made the decoder slower than the scalar code it replaced. Whoever
/// consumes these knows which question it actually needs answered.
///
/// `count` codepoints, consuming `bytes` of input. `count` of zero means the
/// block was not something the fast path handles -- a truncated sequence, an
/// overlong form, a surrogate, a control byte -- and the caller must fall back
/// to the scalar decoder, which owns every conformance decision.
pub struct DecodedBlock {
    pub cps: [Codepoint; DECODE_LANES],
    pub count: LaneCount,
    pub bytes: ByteLen,
}

/// Decode a block of UTF-8 to codepoints, without regard to character class.
///
/// **The counterpart of `Scan`, and the reason it is a separate trait.**
/// `Scan` finds boundaries; this turns bytes into codepoints. Splitting them
/// is the architecture: every *write* path in baud is deliberately
/// class-specialised, because a homogeneous run is what lets eight cells go to
/// one store -- a mixed-width writer was built and measured **-43% on
/// `payload::cjk`**. Decode has no such constraint. It has no stride, no
/// splat, and no reason to care whether the next character is ASCII or Han.
///
/// So the shape is: decode class-agnostically here, group the result by width,
/// and hand each group to the specialised writer that already exists. That is
/// the one split the measurements support.
///
/// Implementations are permitted to refuse. Returning `count == 0` costs the
/// caller a scalar retry and is always correct; it is how validation stays in
/// one place instead of being reimplemented per instruction set.
pub trait Decode {
    /// # Safety
    /// The caller must have established every target feature the chosen
    /// implementation requires, exactly as for `Scan`.
    unsafe fn block(buf: &[StreamByte]) -> DecodedBlock;
}
