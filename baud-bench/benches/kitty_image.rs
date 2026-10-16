//! **Milliseconds per kitty graphics transmission, end to end**, for note
//! V's V19 steps 5-6: a full-window picture fed through `Term::feed` as
//! `kitten icat` sends it and decoded by `Term::decode_step` to completion.
//!
//! Two arms from one PNG: `f=100`, the file's own bytes, and `f=32`, its
//! pixels raw, as kitten sends a picture it scales. Both are kitten's
//! shape (`[[source: kitty-graphics-command]]`): unpadded base64 in chunks
//! of 131,072 bytes, `a=T` on every one, here with `q=2`. The stream is fed
//! in reads of `read` bytes; each transmission is followed by `a=d,d=A`, so
//! the arena is free for the next. Printed per arm: the median and range of
//! the whole transmission, of the feeding alone, and of the steps, the first
//! round dropped, with an FNV-1a of the stored pixels so two builds can be
//! shown to store the same picture.
//!
//!     cargo bench -p baud-bench --bench kitty_image -- <rounds> <reps> <read> <png>

use baud_core::{BytePos, Owned, ReadLen, Term};
use baud_domain::png::Png;
use baud_domain::{DecodeBudget, DecodeProgress, ImageBudget, ImagePx, WinSize};
use std::time::{Duration, Instant};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_unpadded(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 3 * 4 + 4);
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        for (d, s) in b.iter_mut().zip(q) {
            *d = *s;
        }
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..=q.len() {
            let six = usize::try_from((n >> (18 - 6 * k)) & 63).unwrap();
            out.push(B64[six]);
        }
    }
    out
}

fn chunked(keys: &str, encoded: &[u8]) -> Vec<u8> {
    let pieces: Vec<&[u8]> = encoded.chunks(131_072).collect();
    let mut v = Vec::new();
    for (k, piece) in pieces.iter().enumerate() {
        let m = if k + 1 == pieces.len() { 0 } else { 1 };
        v.extend_from_slice(format!("\x1b_G{keys},m={m};").as_bytes());
        v.extend_from_slice(piece);
        v.extend_from_slice(b"\x1b\\");
    }
    v
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
    let reps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5);
    let read: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(65_536);
    let path = args.next().expect("a PNG");
    let file = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let png = Png::parse(&file).expect("a PNG");
    let size = png.size();
    let mut raw = vec![0u8; png.raw_len().unwrap()];
    let mut rgba = vec![0u8; png.rgba_len().unwrap()];
    png.decode(&mut raw, &mut rgba).expect("decodes");
    let (w, h) = (size.width, size.height);
    let arms = [
        ("f=100", chunked("a=T,f=100,q=2", &b64_unpadded(&file))),
        (
            "f=32",
            chunked(&format!("a=T,f=32,s={w},v={h},q=2"), &b64_unpadded(&rgba)),
        ),
    ];
    let pixels = usize::try_from(w * h).unwrap();
    let ws = WinSize {
        ws_row: 32,
        ws_col: 113,
    };
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(pixels));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
    let clear = b"\x1b_Ga=d,d=A\x1b\\\x1b[H";
    let mut times: Vec<[Vec<Duration>; 3]> = arms.iter().map(|_| Default::default()).collect();
    let mut steps = vec![0usize; arms.len()];
    let mut hash = vec![0u64; arms.len()];
    for round in 0..=rounds {
        for (a, (_, stream)) in arms.iter().enumerate() {
            for _ in 0..reps {
                let start = Instant::now();
                for piece in stream.chunks(read) {
                    t.write_at(BytePos::ZERO, piece);
                    t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
                }
                let fed = Instant::now();
                let mut n = 0;
                while t.decode_step(DecodeBudget::of(1 << 18)) == DecodeProgress::More {
                    n += 1;
                }
                let done = Instant::now();
                hash[a] = fnv(&t);
                steps[a] = n;
                assert_eq!(t.images().placements().count(), 1, "stored and placed");
                t.write_at(BytePos::ZERO, clear);
                t.feed(BytePos::ZERO, ReadLen::from_index(clear.len()));
                t.clear_reply();
                if round > 0 {
                    times[a][0].push(done - start);
                    times[a][1].push(fed - start);
                    times[a][2].push(done - fed);
                }
            }
        }
    }
    println!("{path}: {w}x{h}, {} B; reads of {read} B", file.len());
    for (a, (name, stream)) in arms.iter().enumerate() {
        let [all, fed, step] = &mut times[a];
        let (am, al, ah) = median(all);
        let (fm, fl, fh) = median(fed);
        let (sm, sl, sh) = median(step);
        println!(
            "{name:>6}: {} B sent; total {am:.2} ({al:.2}-{ah:.2}) ms, feed {fm:.2} ({fl:.2}-{fh:.2}), \
             steps {sm:.2} ({sl:.2}-{sh:.2}) over {} steps of 256 KiB; fnv {:016x}",
            stream.len(),
            steps[a] + 1,
            hash[a]
        );
    }
}
