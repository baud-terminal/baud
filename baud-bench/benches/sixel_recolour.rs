//! **Milliseconds a read of Sixel palettes costs over stored shared
//! pictures.** Two 3840x2160 Sixels drawn under shared colour registers
//! (`?1070` reset), the most the app's 95 MiB image budget holds, then
//! bodies that only redefine register 1, a new colour each, which every
//! pixel of both pictures was drawn with. Three arms: one body in one
//! read, a hundred in one read, and a hundred in a read each. Register 1
//! goes back to red, untimed, before each arm, so every round does the
//! same work. Printed per arm: the median and range of the rounds, the
//! first dropped, and an FNV-1a of the stored pixels at the end, so two
//! builds can be shown to leave the same pictures.
//!
//!     cargo bench -p baud-bench --bench sixel_recolour -- <rounds>

use baud_core::{BytePos, Owned, READ_CHUNK, ReadLen, Term};
use baud_domain::{ImageBudget, SixelClaim, WinSize};
use std::time::{Duration, Instant};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for piece in bytes.chunks(READ_CHUNK) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
}

fn palette(b: u32) -> Vec<u8> {
    format!("\x1bP0;1;0q#1;2;0;0;{b}\x1b\\").into_bytes()
}

fn fnv(t: &Term<'_>) -> u64 {
    let imgs = t.images();
    imgs.placements()
        .filter_map(|(_, p)| imgs.image(p.image))
        .flat_map(|v| v.pixels.iter().map(|p| p.bits()))
        .fold(0xcbf2_9ce4_8422_2325, |h, w| {
            (h ^ u64::from(w)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn median(v: &mut [Duration]) -> (f64, f64, f64) {
    v.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    (ms(v[v.len() / 2]), ms(v[0]), ms(v[v.len() - 1]))
}

fn main() {
    let mut args = baud_bench::args();
    let rounds: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5);
    let ws = WinSize {
        ws_row: 135,
        ws_col: 480,
    };
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(95 << 18));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_sixel_claim(SixelClaim::Yes);
    let bands = "!3840~-".repeat(360);
    let picture = format!(
        "\x1bP0;1;0q\"1;1;3840;2160#1;2;100;0;0#1{}\x1b\\",
        &bands[..bands.len() - 1]
    );
    feed(&mut t, b"\x1b[?1070l");
    feed(&mut t, picture.as_bytes());
    feed(&mut t, picture.as_bytes());
    assert_eq!(t.images().placements().count(), 2, "both stored and placed");
    let red = b"\x1bP0;1;0q#1;2;100;0;0\x1b\\";
    let burst: Vec<Vec<u8>> = (1..=100).map(palette).collect();
    let whole = burst.concat();
    assert!(whole.len() <= READ_CHUNK, "a hundred bodies in one read");
    let arms: [(&str, Vec<&[u8]>); 3] = [
        ("1 body, 1 read", vec![burst[99].as_slice()]),
        ("100 bodies, 1 read", vec![whole.as_slice()]),
        (
            "100 bodies, 100 reads",
            burst.iter().map(Vec::as_slice).collect(),
        ),
    ];
    let mut times: Vec<Vec<Duration>> = arms.iter().map(|_| Vec::new()).collect();
    for round in 0..=rounds {
        for (a, (_, reads)) in arms.iter().enumerate() {
            feed(&mut t, red);
            let start = Instant::now();
            for r in reads {
                feed(&mut t, r);
            }
            if round > 0 {
                times[a].push(start.elapsed());
            }
        }
    }
    println!(
        "two 3840x2160 shared Sixels, {} B each; {} B of palettes",
        picture.len(),
        whole.len()
    );
    for (a, (name, _)) in arms.iter().enumerate() {
        let (m, lo, hi) = median(&mut times[a]);
        println!("{name:>22}: {m:.2} ms ({lo:.2}-{hi:.2})");
    }
    println!("fnv {:016x}", fnv(&t));
}
