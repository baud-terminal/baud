//! **Milliseconds per PNG decode through `baud_domain::png`, kept
//! runnable** for note V's V19, step 0: whether decoding a full-window
//! picture inside `feed` fits one 120 Hz frame (8.3 ms).
//!
//! It was `baud-font`'s private module, compiled here by path; since V19
//! step 2 moved it to `baud-domain` it is called as the crate exports it.
//!
//! Each round decodes every file `reps` times, `Png::parse` then
//! `Png::decode` into two buffers sized once, the files rotated so none
//! always runs first. The first round is dropped and the median and range
//! of the rest are printed, with an FNV-1a of each file's pixels so two
//! builds can be shown to decode the same thing. `PNG_DUMP=dir` writes each
//! file's RGBA there, to compare against another decoder.
//!
//! `rounds = 0` decodes each file `reps` times more and prints nothing timed;
//! every run decodes each file once first, for its size and FNV. Run
//! `0 <reps>` and `0 0` under `/usr/bin/time -l`, one file each, and the
//! difference in instructions retired, over `reps`, is one decode's.
//!
//!     cargo bench -p baud-bench --bench png_decode -- <rounds> <reps> <png>...

use baud_domain::png::Png;
use std::hint::black_box;
use std::time::Instant;

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn main() {
    let mut args = baud_bench::args();
    let rounds: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5);
    let reps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(10);
    let paths: Vec<String> = args.collect();
    let files: Vec<Vec<u8>> = paths
        .iter()
        .map(|p| std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}")))
        .collect();
    let (mut raw, mut rgba) = (Vec::new(), Vec::new());
    for (p, f) in paths.iter().zip(&files) {
        let png = Png::parse(f).unwrap_or_else(|e| panic!("{p}: {e:?}"));
        let (r, a) = (png.raw_len().unwrap(), png.rgba_len().unwrap());
        raw.resize(raw.len().max(r), 0);
        rgba.resize(rgba.len().max(a), 0);
        let size = png.decode(&mut raw, &mut rgba).unwrap();
        println!(
            "{p}: {}x{}, {} bytes in, raw {r}, rgba {a}, fnv {:016x}",
            size.width,
            size.height,
            f.len(),
            fnv(&rgba[..a])
        );
        if let Some(dir) = std::env::var_os("PNG_DUMP") {
            let name = std::path::Path::new(p).file_name().unwrap();
            let out = std::path::Path::new(&dir).join(name).with_extension("rgba");
            std::fs::write(out, &rgba[..a]).unwrap();
        }
    }
    if rounds == 0 {
        for f in &files {
            for _ in 0..reps {
                let png = Png::parse(black_box(f)).unwrap();
                black_box(png.decode(&mut raw, &mut rgba).unwrap());
            }
            if reps == 0 {
                black_box(Png::parse(black_box(f)).unwrap());
            }
        }
        return;
    }
    let mut ms = vec![Vec::new(); files.len()];
    for round in 0..rounds {
        let mut line = format!("round {round}:");
        for k in 0..files.len() {
            let i = (k + round) % files.len();
            let start = Instant::now();
            for _ in 0..reps {
                let png = Png::parse(black_box(&files[i])).unwrap();
                black_box(png.decode(&mut raw, &mut rgba).unwrap());
            }
            let each = start.elapsed().as_secs_f64() * 1e3 / reps as f64;
            line += &format!(" [{i}] {each:.3}");
            if round > 0 {
                ms[i].push(each);
            }
        }
        println!("{line} ms");
    }
    for (p, mut m) in paths.iter().zip(ms) {
        m.sort_by(f64::total_cmp);
        if let (Some(lo), Some(hi)) = (m.first(), m.last()) {
            let med = (m[(m.len() - 1) / 2] + m[m.len() / 2]) / 2.0;
            println!(
                "{p}: median {med:.3} ms, range {lo:.3}-{hi:.3}, {} rounds kept",
                m.len()
            );
        }
    }
}
