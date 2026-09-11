//! **`Neon` must answer exactly what `Portable` answers**, because `Portable`
//! is the definition and this crate is an optimisation of it.
//!
//! This is the property `baud-core2` used to carry and that went with it: the
//! arch implementations had no test at all between that deletion and this file.
//! `baud-isa/tests/decode_agrees.rs` exercises `Portable` alone, so a `Neon`
//! that returned a plausible wrong length would have been caught by nothing.

#![cfg(target_arch = "aarch64")]

use baud_domain::*;
use baud_isa::{Portable, Scan};
use baud_isa_aarch64::Neon;

/// The four bytes that decide a stride-three run, and nothing else matters:
/// a three-byte lead, a continuation, a byte that is neither, and a four-byte
/// lead -- which is the one an `0xf0` mask could confuse with `0xe0`.
const ALPHABET: [u8; 4] = [0xe3, 0x81, 0x41, 0xf0];

fn both(bytes: &[u8]) -> (usize, usize) {
    let s = StreamByte::slice_from(bytes);
    // SAFETY: NEON is baseline on aarch64 and this file is `cfg`'d to it;
    // `Portable` requires nothing.
    unsafe {
        (
            Portable::stride_three(s).index(),
            Neon::stride_three(s).index(),
        )
    }
}

/// **Exhaustive over the first eight lanes**, which is 65,536 blocks.
///
/// **The tail has to continue the stride, and the first draft did not.** It was
/// `e3 81 82 e3 81 82 e3 81` from lane 8, which puts a *lead* at lane 8 -- a
/// continuation position, since lanes 6, 7 and 8 are the third character. So no
/// input this sweep could build ever reached a run of four or five, and
/// perturbing `AT_LEAD`'s lane 12 left it green while the cap test below went
/// red. A test that cannot reach the lanes it claims to cover is the
/// probe-that-cannot-fire failure, and it took the perturbation to see it.
///
/// Aligned: lane 8 completes the third character, 9-11 are the fourth and
/// 12-14 the fifth, so a valid prefix reaches the cap through this path.
#[test]
fn neon_agrees_with_portable_on_every_prefix_of_a_four_symbol_alphabet() {
    let tail = [0x82u8, 0xe3, 0x81, 0x82, 0xe3, 0x81, 0x82, 0x41];
    let mut checked = 0usize;
    let mut block = [0u8; 16];
    block[8..].copy_from_slice(&tail);
    for n in 0..4usize.pow(8) {
        let mut m = n;
        for lane in block.iter_mut().take(8) {
            *lane = ALPHABET[m % 4];
            m /= 4;
        }
        let (p, v) = both(&block);
        assert_eq!(p, v, "block {block:02x?}");
        checked += 1;
    }
    assert_eq!(checked, 65_536, "the sweep did not run");
}

/// A short buffer has no block in it, and both must say so rather than read
/// past the end.
#[test]
fn neither_reads_past_a_short_buffer() {
    let full = [0xe3u8, 0x81, 0x82].repeat(6);
    for len in 0..16usize {
        let (p, v) = both(&full[..len]);
        assert_eq!(p, v, "len {len}");
        assert_eq!(p, 0, "a partial block is not a run");
    }
}

/// The cap is the register, so a block that is nothing but valid characters
/// answers five and not six.
#[test]
fn a_full_block_of_valid_characters_is_capped_at_five() {
    let block = [0xe3u8, 0x81, 0x82].repeat(6);
    let (p, v) = both(&block[..16]);
    assert_eq!((p, v), (5, 5));
}
