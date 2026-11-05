//! **WCAG 2.3.1's census over an animation file's frames**: the method of
//! `tests/splash_flashes.rs` (`baud_bench::flash::Census`), fed the frames
//! a decoder read out of a file, so what is counted is what a browser
//! shows. `scripts/make-readme-logo.py` runs it on the README's logo.
//!
//! usage: `flash_census <frames> <repeats> <width> <height> <passes> <hold>
//! <rate> <scale>`
//!
//! `<frames>` is the file's frames as a decoder composed them, each pixel
//! three bytes, red, green and blue, row after row, opaque; `<repeats>`
//! is a line for each frame, the samples at `<rate>` a second it lasts on
//! the file's own timeline. They are fed `<passes>` times -- two for a
//! looping file, so the wrap is counted -- and the last then `<hold>` more
//! times: a file played once rests on it. `<scale>` is the device pixels a
//! point the file is shown at. Prints the report as one JSON object.

use baud_bench::flash::{Census, Outcome, Rate, Scale};
use baud_font::Px;
use std::io::Read;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, path, repeats, args @ ..] = a.as_slice() else {
        panic!(
            "usage: flash_census <frames> <repeats> <width> <height> <passes> <hold> <rate> <scale>"
        );
    };
    let n: Vec<u32> = args
        .iter()
        .map(|s| s.parse().expect("a whole number"))
        .collect();
    let [w, h, passes, hold, rate, scale] = n[..] else {
        panic!("six numbers after the frames' files");
    };
    let repeats: Vec<u32> = std::fs::read_to_string(repeats)
        .expect("the repeats")
        .lines()
        .map(|l| l.trim().parse().expect("a whole number a line"))
        .collect();
    let size = usize::try_from(w * h).expect("a frame fits in memory");
    let mut buf = vec![0u8; size * 3];
    let mut census = Census::new(Px::of(w), Px::of(h), Scale(scale), Rate(rate));
    let mut last = vec![0u32; size];
    for _ in 0..passes {
        let mut f = std::io::BufReader::new(std::fs::File::open(path).expect("the frames"));
        for &times in &repeats {
            f.read_exact(&mut buf).expect("a frame");
            for (p, c) in last.iter_mut().zip(buf.chunks_exact(3)) {
                *p = u32::from_be_bytes([0xFF, c[0], c[1], c[2]]);
            }
            for _ in 0..times {
                census.frame(&last);
            }
        }
    }
    for _ in 0..hold {
        census.frame(&last);
    }
    let r = census.finish();
    let verdict = |o: Outcome| match o {
        Outcome::Passes => "passes",
        Outcome::Fails => "fails",
    };
    println!(
        "{{\"general_flashes\": {}, \"general_transitions\": {}, \"general_chained\": {}, \
         \"red_flashes\": {}, \"red_encoded_flashes\": {}, \"area\": {:.1}, \"contiguous\": {:.1}, \
         \"strict\": {:.1}, \"threshold\": {:.1}, \"second\": {:.3}, \"frames\": {}, \
         \"translucent\": {}, \"outcome\": \"{}\", \"strict_outcome\": \"{}\"}}",
        r.general.flashes.0,
        r.general.transitions.0,
        r.general.chained.0,
        r.red.flashes.0,
        r.red_encoded.flashes.0,
        r.area.0,
        r.contiguous.0,
        r.strict.0,
        r.threshold.0,
        r.second,
        r.frames.0,
        r.translucent.value(),
        verdict(r.outcome()),
        verdict(r.strict_outcome())
    );
}
