#![feature(portable_simd)]
//! **The reference numbers the whole ladder is quoted against, made
//! reproducible.**
//!
//! The project notes' ladder section opened with four figures "measured on this
//! box rather than assumed" -- a 64 MB buffer read, `memcpy`, scalar eight-byte
//! stores into the grid, and `Simd<u64, 8>` stores -- and derived from the last
//! one that a materialising terminal cannot exceed **17,156 MB/s of input**,
//! that the honest maximum for the operation baud performs is **11,660**, and
//! that `put_ascii_run` is already at it so "there is nothing left" in it.
//!
//! **Nothing in the tree measured any of them.** They were taken with a
//! scratch program that was not kept, which is the state `opcost` is already
//! recorded in -- "the binary is gone ... cannot be re-measured" -- except that
//! these numbers are load-bearing in a way `opcost`'s per-shape rows are not:
//! they are the reason a whole path is closed to further work.
//!
//! So this is that program, kept. Run it before quoting any of those figures,
//! and on mains -- the harness says which.
//!
//! **What it is not.** These are isolated loops, not a terminal: a ceiling
//! bounds the build it was measured on and nothing else, which this project has
//! had to say about a ceiling once already.

use std::hint::black_box;
use std::simd::Simd;
use std::time::Instant;

/// 64 MB, matching the figure being reproduced.
const BUF: usize = 64 << 20;
/// The live grid: 200 x 50 eight-byte cells = 80 KB, as the ladder says.
const COLS: usize = 200;
const ROWS: usize = 50;
const CELLS: usize = COLS * ROWS;

/// **The units are this project's, which are mixed, and that is deliberate
/// here.** `ingest2` divides by `1 << 20`, so every "MB/s" in this repository is
/// a *mebibyte*; the ladder's "GB/s" is decimal. Nothing says so anywhere, and
/// the pair is recoverable only from the arithmetic: the ladder derives 17,156
/// MB/s of input from 143.9 GB/s, and that identity holds for `GB = 1e9` with
/// `MB = 2^20` and for no other combination of the two.
///
/// The first version of this file used `1e6` for both and reported the read and
/// `memcpy` rows **4.86% high** against figures in the other unit -- a
/// like-for-like comparison that was not one, which is the "mismatched
/// optimisation flags" entry in a new costume.
fn gb_s(bytes: usize, secs: f64) -> f64 {
    bytes as f64 / secs / 1e9
}
fn mb_s(bytes: usize, secs: f64) -> f64 {
    bytes as f64 / secs / (1 << 20) as f64
}

/// Run a closure until it has taken at least `floor` seconds, and report the
/// best rate seen.
///
/// **Best, not mean**, because these are ceilings: a slower round is the
/// machine doing something else, and the question is what the hardware can do
/// when it is left alone. Every other measurement in this project quotes a
/// median and a range because it is comparing two programs; this one is
/// comparing one program against the machine.
fn best<F: FnMut() -> usize>(name: &str, mut f: F, unit_mb: bool) {
    let mut top = 0.0f64;
    let mut rounds = 0;
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < 0.6 {
        let t = Instant::now();
        let bytes = f();
        let secs = t.elapsed().as_secs_f64();
        let r = if unit_mb {
            mb_s(bytes, secs)
        } else {
            gb_s(bytes, secs)
        };
        if r > top {
            top = r;
        }
        rounds += 1;
    }
    let u = if unit_mb { "MB/s" } else { "GB/s" };
    println!("  {name:<46} {top:>10.1} {u}   ({rounds} rounds)");
}

fn main() {
    println!("reference ceilings -- isolated loops, not a terminal");
    if let Ok(s) = std::process::Command::new("pmset")
        .arg("-g")
        .arg("batt")
        .output()
    {
        let t = String::from_utf8_lossy(&s.stdout);
        if t.contains("Battery Power") {
            println!("  BATTERY -- re-run on mains before quoting any of this");
        }
    }

    // 1. Read every byte of a 64 MB buffer.
    //
    // **Two arms, because the obvious one does not measure a read.** Summing
    // into a `u64` widens every byte eightfold, so sixteen bytes loaded become
    // 128 bytes of vector lanes before a single add -- the loop is bound by the
    // widening, not by memory, and it reported **slower than `memcpy`**. That
    // is impossible on its face: `memcpy` moves the same bytes in and the same
    // bytes out again, so it cannot beat a pure read. When the number is
    // impossible the harness is wrong, and this file already says so.
    //
    // The wide arm reads the same bytes eight at a time and folds them with one
    // `xor`, which is the least work a read can be asked to do while still
    // being observed.
    let src: Vec<u8> = (0..BUF).map(|i| (i & 0xff) as u8).collect();
    best(
        "read a 64 MB buffer, 8 bytes at a time",
        || {
            let mut acc = 0u64;
            for c in src.chunks_exact(8) {
                let w: [u8; 8] = c.try_into().expect("chunks_exact(8)");
                acc ^= u64::from_ne_bytes(w);
            }
            black_box(acc);
            BUF
        },
        true,
    );
    best(
        "the same, summed byte-at-a-time into a u64",
        || {
            let mut acc = 0u64;
            for &b in src.iter() {
                acc = acc.wrapping_add(u64::from(b));
            }
            black_box(acc);
            BUF
        },
        true,
    );

    // 2. memcpy, one byte in one byte out.
    let mut dst: Vec<u8> = vec![0; BUF];
    best(
        "memcpy, one byte in one byte out",
        || {
            dst.copy_from_slice(&src);
            black_box(&dst[0]);
            BUF
        },
        true,
    );

    // 3. Scalar eight-byte stores into the grid.
    let mut grid: Vec<u64> = vec![0; CELLS];
    let passes = 4096;
    // **A "scalar" store loop in Rust is not scalar**, and reproducing the
    // recorded 35.4 GB/s needs that said out loud. `for c in grid.iter_mut() {
    // *c = v }` auto-vectorises to exactly the arm below it and reads ~143 --
    // the same number, measured twice under two names, which would have
    // "confirmed" that a vector store buys nothing at all.
    //
    // `write_volatile` is what keeps it one store at a time. It also forbids
    // every other reordering, so this is a *floor* on the scalar form rather
    // than a fair measurement of one -- which is the honest way to use it and
    // why the vector figure is the one the ladder quotes.
    best(
        "scalar 8-byte stores, volatile so it stays scalar",
        || {
            for p in 0..passes {
                let v = p as u64;
                let ptr = grid.as_mut_ptr();
                for i in 0..CELLS {
                    // SAFETY: `i < CELLS` and `grid` holds `CELLS` elements.
                    unsafe { ptr.add(i).write_volatile(v) };
                }
            }
            black_box(&grid[0]);
            passes * CELLS * 8
        },
        false,
    );

    // 4. `Simd<u64, 8>`, eight cells a store -- the number the architecture is
    //    bounded against.
    best(
        "Simd<u64,8>, eight cells a store",
        || {
            for p in 0..passes {
                let v = Simd::<u64, 8>::splat(p as u64);
                for chunk in grid.chunks_exact_mut(8) {
                    let arr: &mut [u64; 8] = chunk.try_into().expect("chunks_exact_mut(8)");
                    *arr = v.to_array();
                }
            }
            black_box(&grid[0]);
            passes * CELLS * 8
        },
        false,
    );

    // 5. **The operation a terminal actually performs**: read a byte, widen it
    //    to an eight-byte cell, store eight cells at a time. This is the one
    //    the "honest maximum" is derived from -- baud does not splat a
    //    constant, it builds each cell from its byte.
    let text: Vec<u8> = (0..CELLS).map(|i| b' ' + (i % 90) as u8).collect();
    best(
        "read a byte, build a cell, eight a store",
        || {
            for _ in 0..passes {
                for (out, inp) in grid.chunks_exact_mut(8).zip(text.chunks_exact(8)) {
                    let mut a = [0u64; 8];
                    for k in 0..8 {
                        // The cell is the codepoint in the low word plus a
                        // rendition; the shape is what costs, not the values.
                        a[k] = u64::from(inp[k]) | (0x2020_0000_0000u64);
                    }
                    let v = Simd::<u64, 8>::from_array(a);
                    let dst: &mut [u64; 8] = out.try_into().expect("chunks_exact_mut(8)");
                    *dst = v.to_array();
                }
            }
            black_box(&grid[0]);
            passes * CELLS * 8
        },
        false,
    );

    println!();
    println!("  at one cell a byte, the last two convert to MB/s of *input* by /8:");
    println!("  that is the figure the ladder quotes as the honest maximum.");
}
