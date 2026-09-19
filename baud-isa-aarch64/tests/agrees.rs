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
    // `Neon`'s inherent methods are safe; `Portable`'s trait method still needs
    // the block, because `Scan` is `unsafe fn` for `Avx2`'s sake.
    //
    // SAFETY: `Portable` requires no target feature at all.
    let portable = unsafe { Portable::stride_three(s) };
    (portable.index(), Neon::stride_three(s).index())
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

// **The four methods below ship or are offered, and none had a test.** This
// file's header says `Neon` must answer what `Portable` answers; until now it
// said it about `stride_three` alone. `run` is the sharper gap: `neon-scan` is
// in `baud-core`'s default features and `write.rs` calls `Neon::run` on every
// text run of every default build, so the one scan a user actually gets was
// the one nothing compared. `final_byte`, `st` and `stride_four` have no
// caller anywhere -- an offered API rather than a dead arm -- and a sweep is
// what makes them correct-if-used instead of merely present.

/// Exhaustive agreement over the first eight lanes of an alphabet, with a
/// fixed tail, for one scan.
///
/// **The alphabet is the test.** A first draft gave `stride_four` four symbols
/// and none of them was in `0xf8..=0xff`, so widening the lead mask from
/// `0xf8` to `0xf0` -- which makes every byte from `0xf8` up a lead -- left it
/// green. The sweep ran 65,536 blocks and could not distinguish the bug,
/// which is the failure this file's other header already records one level
/// down, in my own test rather than in the code under it. Every sweep below
/// names why each of its symbols is there, and each was perturbed until it
/// went red.
fn sweep(name: &str, alphabet: &[u8], tail: [u8; 8], f: impl Fn(&[u8]) -> (usize, usize)) {
    let k = alphabet.len();
    let total = k.pow(8);
    let mut block = [0u8; 16];
    block[8..].copy_from_slice(&tail);
    let mut checked = 0usize;
    for n in 0..total {
        let mut m = n;
        for lane in block.iter_mut().take(8) {
            *lane = alphabet[m % k];
            m /= k;
        }
        let (p, v) = f(&block);
        assert_eq!(p, v, "{name}: block {block:02x?}");
        checked += 1;
    }
    assert_eq!(checked, total, "{name}: the sweep did not run");
}

/// `run` is the printable-ASCII scan, and it is **on in every default build**.
#[test]
fn neon_run_agrees_with_portable() {
    // Printable, a C0 below the range, DEL at the top edge, and the space that
    // is the bottom edge -- the four bytes a `0x20..0x7f` test can disagree on.
    sweep(
        "run",
        &[0x41, 0x0a, 0x7f, 0x20],
        [0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41],
        |b| {
            let s = StreamByte::slice_from(b);
            // SAFETY: `Portable` requires no target feature at all.
            (unsafe { Portable::run(s) }.index(), Neon::run(s).index())
        },
    );
}

/// `final_byte` finds the byte that ends a `CSI`, `0x40..=0x7e`.
#[test]
fn neon_final_byte_agrees_with_portable() {
    // Both ends of the range and the byte just outside each.
    sweep(
        "final_byte",
        &[0x40, 0x7e, 0x3f, 0x7f],
        [0x30, 0x31, 0x3b, 0x32, 0x30, 0x31, 0x3b, 0x33],
        |b| {
            let s = StreamByte::slice_from(b);
            let at = |o: Option<ByteLen>| o.map_or(usize::MAX, |l| l.index());
            // SAFETY: `Portable` requires no target feature at all.
            (
                at(unsafe { Portable::final_byte(s) }),
                at(Neon::final_byte(s)),
            )
        },
    );
}

/// `st` finds a BEL, or the ESC of an `ESC \` terminator.
#[test]
fn neon_st_agrees_with_portable() {
    // BEL, ESC, the backslash that completes it, and an ordinary byte -- so the
    // sweep builds `ESC \`, a lone ESC, and a BEL in every arrangement.
    sweep(
        "st",
        &[0x07, 0x1b, 0x5c, 0x41],
        [0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41],
        |b| {
            let s = StreamByte::slice_from(b);
            let at = |o: Option<ByteLen>| o.map_or(usize::MAX, |l| l.index());
            // SAFETY: `Portable` requires no target feature at all.
            (at(unsafe { Portable::st(s) }), at(Neon::st(s)))
        },
    );
}

/// `stride_four` counts complete four-byte characters.
#[test]
fn neon_stride_four_agrees_with_portable() {
    // A four-byte lead, a continuation, ASCII, the three-byte lead a nibble
    // mask could confuse with it, and **`0xf8` -- the byte above the valid
    // lead range**, without which widening the mask from `0xf8` to `0xf0` is
    // invisible. Five symbols over eight lanes is 390,625 blocks.
    sweep(
        "stride_four",
        &[0xf0, 0x90, 0x41, 0xe3, 0xf8],
        [0x90, 0x80, 0xf0, 0x90, 0x80, 0x80, 0x41, 0x41],
        |b| {
            let s = StreamByte::slice_from(b);
            // SAFETY: `Portable` requires no target feature at all.
            (
                unsafe { Portable::stride_four(s) }.index(),
                Neon::stride_four(s).index(),
            )
        },
    );
}

/// None of the four may read past a buffer shorter than a block.
#[test]
fn none_of_the_four_reads_past_a_short_buffer() {
    let full = [0x41u8, 0x07, 0x1b, 0x5c, 0xf0, 0x90, 0x80, 0x80].repeat(3);
    for len in 0..16usize {
        let s = StreamByte::slice_from(&full[..len]);
        let at = |o: Option<ByteLen>| o.map_or(usize::MAX, |l| l.index());
        // SAFETY: `Portable` requires no target feature at all.
        unsafe {
            assert_eq!(Portable::run(s).index(), Neon::run(s).index(), "run {len}");
            assert_eq!(
                at(Portable::final_byte(s)),
                at(Neon::final_byte(s)),
                "final_byte {len}"
            );
            assert_eq!(at(Portable::st(s)), at(Neon::st(s)), "st {len}");
            assert_eq!(
                Portable::stride_four(s).index(),
                Neon::stride_four(s).index(),
                "stride_four {len}"
            );
        }
    }
}
