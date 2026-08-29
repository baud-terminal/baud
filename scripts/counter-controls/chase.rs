// Pointer chase with known ground truth, for identifying counter columns.
//   arg1 = buffer size in KB, arg2 = steps in millions
// Per step: one dependent load, ~4 instructions. A buffer far larger than any
// cache gives exactly one miss a step; 32 KB gives none.
fn main() {
    let mut a = std::env::args().skip(1);
    let kb: usize = a.next().unwrap().parse().unwrap();
    let steps: usize = a.next().unwrap().parse::<usize>().unwrap() * 1_000_000;
    let n = kb * 1024 / 8;
    // A single cycle through all slots, in pseudo-random order.
    let mut idx: Vec<usize> = (0..n).collect();
    let mut s: u64 = 0x2545F4914F6CDD1D;
    for i in (1..n).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        idx.swap(i, (s as usize) % (i + 1));
    }
    let mut next = vec![0usize; n];
    for w in 0..n {
        next[idx[w]] = idx[(w + 1) % n];
    }
    let mut p = 0usize;
    let t = std::time::Instant::now();
    for _ in 0..steps {
        p = next[p];
    }
    let el = t.elapsed();
    println!(
        "{kb} KB, {steps} steps, {:.2} ns/step, sink {p}",
        el.as_nanos() as f64 / steps as f64
    );
}
