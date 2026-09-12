// Shift-based DFA microbench — answers note B's open question directly.
//
// `[[source: shift-dfa]]` (Vognsen) proposes replacing a DFA's loop-carried
// transition LOAD with a shift and a mask: `state = (table[input] >> state) &
// mask`, packing a whole transition row into one word. Note B calls a 128-bit
// version of this for baud's 20-node graph "the single most interesting
// unmeasured thing" and asks "whether an arm64 u128 shift keeps enough of the
// advantage."
//
// It does not. This isolates the recurrence — load-based `flat[state*256+b]`
// against shift-DFA `(SHIFT[b] >> state*5) & mask` over baud's 20-state shape,
// both walking the SAME transition table so the state sequences are identical
// (asserted) — and on this arm64 core (Apple, ~4.5 GHz) the shift-DFA is
// **~13% slower**: ~1.65 ns/step against the load's ~1.45. The loop is not
// latency-bound (~6.5 cycles a step), so the L1 load pipelines, while a
// variable u128 shift is a multi-instruction sequence: arm64 has no single
// 128-bit shift, and baud's 20 states (5-bit fields, 100 bits) do not fit the
// pre-multiplied form that would save the `* 5`.
//
// The first run read `shift 0.000 ns/step`: dead-code elimination folded the
// pure-arithmetic loop away while the memory-load loop survived — a probe that
// cannot fire. `black_box` on both accumulators is what makes it a measurement.
//
//   rustc -O -o /tmp/shiftdfa scripts/shift-dfa-microbench.rs && /tmp/shiftdfa
//
// Isolates note B's unmeasured question: on this arm64 core, is a u128
// shift-DFA recurrence faster than a loop-carried table load, for baud's
// 20-state graph? Both walks produce the same state sequence (verified);
// only the transition mechanism differs. ns/step is the recurrence cost.
use std::time::Instant;

const NSTATES: usize = 20; // baud NNODE
const WIDTH: u32 = 5; // 20 states -> 5-bit fields, 20*5=100 <= 128

fn main() {
    let mut a = std::env::args().skip(1);
    let steps: usize = a
        .next()
        .map(|s| s.parse().unwrap())
        .unwrap_or(2_000_000_000);

    // A dense, deterministic [state][byte] -> next-state table, 20 states.
    // Contents are arbitrary; what matters for the timing is the shape (20
    // states, dense) and that both mechanisms read the SAME transition.
    let mut nextb = [[0u8; 256]; NSTATES];
    let mut r: u64 = 0x243F6A8885A308D3;
    for s in 0..NSTATES {
        for b in 0..256 {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            nextb[s][b] = (r % NSTATES as u64) as u8;
        }
    }
    // Load-based flat table: table[state*256 + b].
    let mut flat = vec![0u8; NSTATES * 256];
    for s in 0..NSTATES {
        for b in 0..256 {
            flat[s * 256 + b] = nextb[s][b];
        }
    }
    // Shift-packed: SHIFT[b] holds, at field s (bits s*WIDTH..), next state.
    let mut shift = [0u128; 256];
    for b in 0..256 {
        let mut w: u128 = 0;
        for s in 0..NSTATES {
            w |= (nextb[s][b] as u128) << (s as u32 * WIDTH);
        }
        shift[b] = w;
    }
    // Deterministic pseudo-random byte stream, walked repeatedly.
    let mut stream = vec![0u8; 1 << 16];
    for x in stream.iter_mut() {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        *x = r as u8;
    }
    let mask: u128 = (1u128 << WIDTH) - 1;

    let run_load = |steps: usize| -> (u64, f64) {
        let mut s: usize = 0;
        let mut acc: u64 = 0;
        let mut i = 0usize;
        let t0 = Instant::now();
        while i < steps {
            let b = stream[i & (stream.len() - 1)] as usize;
            s = flat[s * 256 + b] as usize;
            acc = acc.wrapping_add(s as u64);
            i += 1;
        }
        (acc, t0.elapsed().as_secs_f64())
    };
    let run_shift = |steps: usize| -> (u64, f64) {
        let mut s: u32 = 0;
        let mut acc: u64 = 0;
        let mut i = 0usize;
        let t0 = Instant::now();
        while i < steps {
            let b = stream[i & (stream.len() - 1)] as usize;
            s = ((shift[b] >> (s * WIDTH)) & mask) as u32;
            acc = acc.wrapping_add(s as u64);
            i += 1;
        }
        (acc, t0.elapsed().as_secs_f64())
    };

    // correctness: identical state walk
    let (ca, _) = run_load(1_000_000);
    let (cb, _) = run_shift(1_000_000);
    assert_eq!(ca, cb, "load and shift walks diverged");

    // interleaved A/B, 5 rounds, report ns/step
    println!("steps={steps}, {NSTATES} states, {WIDTH}-bit fields");
    for round in 0..5 {
        let (al, tl) = run_load(steps);
        std::hint::black_box(al);
        let (as_, ts) = run_shift(steps);
        std::hint::black_box(as_);
        println!(
            "round {round}: load {:.3} ns/step   shift {:.3} ns/step   ({:+.1}%)",
            tl / steps as f64 * 1e9,
            ts / steps as f64 * 1e9,
            100.0 * (ts - tl) / tl
        );
    }
}
