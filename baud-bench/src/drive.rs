use crate::harness::*;
use crate::*;
use baud_core::*;

/// Eight parameters because eight things vary between rows of the table this
/// prints, and a config struct would put them behind a name instead of at the
/// call site. The audit gate already caps how many may be raw integers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bench_t(
    store: &mut Storage,
    label: &str,
    mode: OpMode,
    which: &P,
    total: usize,
    reps: usize,
    avx2: bool,
    tr: Tr,
) {
    let mut v = Vec::with_capacity(reps);
    for _ in 0..reps {
        v.push(run_once_t(store, mode, which, total, avx2, tr).0);
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "  {:<34} {:7.1}  [{:5.1}-{:5.1}]",
        label,
        v[reps / 2],
        v[0],
        v[reps - 1]
    );
}

pub(crate) fn bench(
    store: &mut Storage,
    label: &str,
    mode: OpMode,
    which: &P,
    total: usize,
    reps: usize,
    avx2: bool,
) {
    let mut v = Vec::with_capacity(reps);
    let (mut promo, mut hits, mut builds) = (0u64, 0u64, 0u64);
    for _ in 0..reps {
        let (mbs, p, h, b) = run_once(store, mode, which, total, avx2);
        v.push(mbs);
        promo = p;
        hits = h;
        builds = b;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    print!(
        "  {:<32} {:7.1}  [{:5.1}-{:5.1}]",
        label,
        v[reps / 2],
        v[0],
        v[reps - 1]
    );
    print!(
        "   styles {} interned / {} hits ({:.1}%)",
        builds,
        hits,
        100.0 * hits as f64 / (hits + builds).max(1) as f64
    );
    if promo > 0 {
        print!("   promotions {}", promo);
    }
    println!();
}
