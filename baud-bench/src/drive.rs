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
    // `Stat`, not three bare `u64`s. `hits` and `builds` are adjacent, the
    // same width, and feed a *ratio* -- transposing them silently reports the
    // inverse hit rate, which looks entirely plausible. That rate is the
    // diagnostic that caught `StyleId::bucket` taking the wrong bits (112,144
    // interns at 51.6% against 0 at 100%), so a plausible-looking wrong value
    // there is expensive.
    let (mut promo, mut hits, mut builds) = (Stat::ZERO, Stat::ZERO, Stat::ZERO);
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
        builds.count(),
        hits.count(),
        // `ratio_to` rather than the division written out again: it is the
        // helper this repo already wrote for hit rates, it saturates at zero
        // instead of dividing by it, and naming both operands is what makes a
        // swap visible rather than merely possible.
        100.0 * hits.ratio_to(builds)
    );
    if !promo.is_zero() {
        print!("   promotions {}", promo.count());
    }
    println!();
}
