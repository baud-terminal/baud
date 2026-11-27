// A loop whose branch is either perfectly predictable or a coin flip, with the
// same instruction count either way. Ground truth for a mispredict counter.
//   arg1 = "pred" | "rand", arg2 = steps in millions
fn main() {
    let mut a = std::env::args().skip(1);
    let kind = a.next().unwrap();
    let steps: usize = a.next().unwrap().parse::<usize>().unwrap() * 1_000_000;
    let n = 1 << 16;
    let mut s: u64 = 0x2545F4914F6CDD1D;
    let v: Vec<u8> = (0..n)
        .map(|i| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            match kind.as_str() {
                // Alternating: the predictor learns it exactly.
                "pred" => (i % 2) as u8,
                // A coin flip it cannot learn.
                _ => (s & 1) as u8,
            }
        })
        .collect();
    // Out of line so LLVM keeps a branch instead of a conditional select --
    // the first version compiled to `cmov` and both arms measured the same,
    // which is a probe that cannot fire.
    #[inline(never)]
    fn f(x: u64) -> u64 {
        x.wrapping_add(3)
    }
    #[inline(never)]
    fn g(x: u64) -> u64 {
        x.wrapping_mul(3)
    }
    let mut x: u64 = 1;
    let t = std::time::Instant::now();
    for i in 0..steps {
        x = if v[i & (n - 1)] == 0 { f(x) } else { g(x) };
    }
    let el = t.elapsed();
    println!(
        "{kind}: {:.3} ns/step, sink {}",
        el.as_nanos() as f64 / steps as f64,
        x
    );
}
