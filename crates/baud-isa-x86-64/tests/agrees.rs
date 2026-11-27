//! **`Avx2` must answer exactly what `Portable` answers.**
//!
//! The aarch64 twin of this file is `crates/baud-isa-aarch64/tests/agrees.rs`. Both
//! exist because the property that used to compare an arch implementation
//! against the portable one lived in `baud-core2` and went with that crate, so
//! between the deletion and these files **nothing tested either scanner**.
//!
//! **This one cannot run on the development box**, which is aarch64. It is
//! written for any x86-64 host with AVX2, where `devenv tasks run
//! test:x86-avx2` runs it and refuses a host without, and it is gated on
//! `target_feature = "avx2"` for the same reason `Avx2`'s safe surface is: AVX2
//! is not baseline, `no_std` has no `is_x86_feature_detected!`, and a build that
//! does not target the feature must not call these at all.
//!
//! Run it with `RUSTFLAGS="-C target-feature=+avx2" cargo test -p baud-isa-x86-64`.

#![cfg(all(target_arch = "x86_64", target_feature = "avx2"))]

use baud_domain::*;
use baud_isa::{Portable, Scan};
use baud_isa_x86_64::Avx2;

/// The bytes that decide a printable run: inside the range, the two bytes just
/// outside it, and a high byte that a signed compare would get wrong.
const ALPHABET: [u8; 5] = [0x41, 0x20, 0x7e, 0x1f, 0xe3];

fn both(bytes: &[u8]) -> (usize, usize) {
    let s = StreamByte::slice_from(bytes);
    // SAFETY: `Portable` requires no feature; `Avx2` requires AVX2, which this
    // file is `cfg`'d to.
    let p = unsafe { <Portable as Scan>::run(s) };
    (p.index(), Avx2::run(s).index())
}

/// **Exhaustive over the first eight bytes**, with a valid tail, so a
/// disagreement can be placed anywhere in the first vector step.
#[test]
fn avx2_agrees_with_portable_on_the_printable_run() {
    let tail = [0x41u8; 40];
    let mut block = [0u8; 48];
    block[8..].copy_from_slice(&tail);
    let mut checked = 0usize;
    for n in 0..5usize.pow(8) {
        let mut m = n;
        for lane in block.iter_mut().take(8) {
            *lane = ALPHABET[m % 5];
            m /= 5;
        }
        let (p, v) = both(&block);
        assert_eq!(p, v, "block {:02x?}", &block[..12]);
        checked += 1;
    }
    assert_eq!(checked, 390_625, "the sweep did not run");
}

/// Lengths either side of the 32-byte step, including zero and one.
#[test]
fn avx2_agrees_at_every_length_around_a_step() {
    let full = [0x41u8; 80];
    for len in 0..72usize {
        let (p, v) = both(&full[..len]);
        assert_eq!(p, v, "all-printable, len {len}");
        assert_eq!(p, len, "a fully printable buffer is one run");
    }
    // A stopper at each position, so the answer is the position itself.
    for stop in 0..40usize {
        let mut b = [0x41u8; 48];
        b[stop] = 0x07;
        let (p, v) = both(&b);
        assert_eq!(p, v, "stopper at {stop}");
        assert_eq!(p, stop, "the run must end at the stopper");
    }
}

/// The stride scans delegate to `Portable`, so they must be *equal*, not merely
/// consistent -- if that ever stops being true the delegation was replaced and
/// this file should be the thing that notices.
#[test]
fn the_stride_scans_are_the_portable_ones() {
    let cjk = "終端文字解析画面行列描画".as_bytes();
    let emoji = "🚀🚀🚀🚀🚀🚀".as_bytes();
    let s3 = StreamByte::slice_from(cjk);
    let s4 = StreamByte::slice_from(emoji);
    // SAFETY: as `both`.
    unsafe {
        assert_eq!(
            <Portable as Scan>::stride_three(s3).index(),
            <Avx2 as Scan>::stride_three(s3).index()
        );
        assert_eq!(
            <Portable as Scan>::stride_four(s4).index(),
            <Avx2 as Scan>::stride_four(s4).index()
        );
    }
}
