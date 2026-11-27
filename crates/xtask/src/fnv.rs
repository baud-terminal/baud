//! FNV-1a, 64-bit: the digests `tools/scripts/build-metallib.sh` records beside
//! the Metal library, and `crates/baud-present-macos/tests/metallib.rs` checks.

use std::fmt;

/// The offset basis.
pub const OFFSET: Digest = Digest(0xCBF2_9CE4_8422_2325);

/// The prime.
pub const PRIME: Digest = Digest(0x0100_0000_01B3);

/// A 64-bit FNV-1a digest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Digest(pub u64);

/// The digest of `bytes`.
pub fn fnv1a(bytes: &[u8]) -> Digest {
    Digest(
        bytes
            .iter()
            .fold(OFFSET.0, |h, &b| (h ^ u64::from(b)).wrapping_mul(PRIME.0)),
    )
}

/// Sixteen lowercase hex digits, as the `.fnv` files hold it before their
/// newline.
impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}
