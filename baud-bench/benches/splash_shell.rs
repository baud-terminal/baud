//! `splash_shell`: a shell's recorded start under the launch logo, frame by
//! frame on the CPU path, and through the instance path's convert (note AJ,
//! section 18).
//!
//! **The recording is a shell's first output with its timing**: each read
//! as a little-endian `f64` of seconds since the shell was forked, a `u32`
//! length and the bytes, as `scripts/record-shell-start.py` writes it. A
//! shell's output names its machine and its user, so no such recording is
//! in the corpus; the bench takes a path.
//!
//! **Each frame is a display interval's**: the reads whose time has come
//! by the frame's vsync are fed first, then the splash is set for the
//! frame's age and the frame rendered with [`Damage::Incremental`] -- the
//! order both drivers keep, feed then draw. The logo's first frame is at
//! `SPLASH_AT` seconds after the fork (0.21 by default: the shell spawned
//! at 14 ms and the logo first drawn at 228 ms in a launch measured with
//! `BAUD_STARTUP=1`), and the frames run until the logo is gone and every
//! read has been drawn.
//!
//! **What it prints**, a line a size and round: the frames, the worst
//! frame's milliseconds and the frame it was, the median, the frames over
//! budget, the frames a read was fed on and the worst of them, and the
//! frame the shell's first visible cell was drawn on and its milliseconds.
//! `convert` lines time
//! [`Renderer::convert`], the main thread's share of a GPU instance frame.
//! The first round is a warm-up and should be dropped.
//!
//! Usage: `splash_shell <recording> [rounds]`.

use baud_bench::flash::Rate;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, Content, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Blend, Damage, FaceRole, FaceStyle, PxSize, Renderer, SplashLook, SplashPhase, Stack, Target,
    Theme,
};
use std::time::Instant;

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The recording's reads: seconds since the fork, and the bytes.
fn reads(path: &str) -> Vec<(f64, Vec<u8>)> {
    let d = std::fs::read(path).expect("a recording");
    let mut out = Vec::new();
    let mut i = 0;
    while i + 12 <= d.len() {
        let t = f64::from_le_bytes(d[i..i + 8].try_into().unwrap());
        let n = usize::try_from(u32::from_le_bytes(d[i + 8..i + 12].try_into().unwrap())).unwrap();
        i += 12;
        out.push((t, d[i..i + n].to_vec()));
        i += n;
    }
    out
}

/// Whether the screen holds a character other than a space.
fn drawn(t: &Term<'_>) -> bool {
    let v = t.view(t.window_back());
    (0..v.rows().index()).any(|r| {
        v.row(Row::from_index(r)).iter().any(|c| {
            let k: Content = c.content;
            !k.is_empty() && k.as_codepoint().is_none_or(|p| p.value() != 0x20)
        })
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Path {
    Cpu,
    Convert,
}

struct Size {
    name: &'static str,
    ws: WinSize,
    px: f32,
    rate: Rate,
}

const SIZES: [Size; 3] = [
    Size {
        name: "120x36@2x 60Hz",
        ws: WinSize {
            ws_col: 120,
            ws_row: 36,
        },
        px: 28.0,
        rate: Rate(60),
    },
    Size {
        name: "120x36@2x 120Hz",
        ws: WinSize {
            ws_col: 120,
            ws_row: 36,
        },
        px: 28.0,
        rate: Rate(120),
    },
    Size {
        name: "200x50@2x 120Hz",
        ws: WinSize {
            ws_col: 200,
            ws_row: 50,
        },
        px: 28.0,
        rate: Rate(120),
    },
];

/// One life of the splash with the shell's reads fed as they come.
struct Life {
    /// Each frame's milliseconds, and whether a read was fed on it.
    ms: Vec<(f64, Fed)>,
    /// The frame the first visible cell was drawn on, and its milliseconds.
    content: Option<(usize, f64)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Fed {
    Read,
    Nothing,
}

fn life(
    faces: &[Face<'_>],
    roles: &[FaceRole],
    size: &Size,
    rec: &[(f64, Vec<u8>)],
    at: f64,
    path: Path,
) -> Life {
    let ws = WinSize { ..size.ws };
    let mut r = Renderer::new(
        Stack::with_roles(faces, roles).unwrap(),
        PxSize::of(size.px).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let frame_s = 1.0 / f64::from(size.rate.0);
    let mut ms = Vec::new();
    let mut next = 0;
    let mut content = None;
    for f in 0usize.. {
        let vsync = at + f as f64 * frame_s;
        let age = (f as f64 * frame_s) as f32;
        if age >= baud_render::splash::GONE && next == rec.len() {
            break;
        }
        let start = Instant::now();
        let fed = match next < rec.len() && rec[next].0 <= vsync {
            true => Fed::Read,
            false => Fed::Nothing,
        };
        while next < rec.len() && rec[next].0 <= vsync {
            let b = &rec[next].1;
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            next += 1;
        }
        r.set_splash((age < baud_render::splash::GONE).then_some(SplashLook {
            phase: SplashPhase::Plasma(age),
        }));
        match path {
            Path::Cpu => {
                let mut target = Target::new(&mut px, w, w, h).unwrap();
                r.render(&mut t, &mut target, Damage::Incremental);
            }
            Path::Convert => {
                let _ = r.convert(&t, t.window_back(), Damage::Incremental);
            }
        }
        let took = start.elapsed().as_secs_f64() * 1e3;
        if content.is_none() && drawn(&t) {
            content = Some((f, took));
        }
        ms.push((took, fed));
    }
    Life { ms, content }
}

fn main() {
    let mut args = baud_bench::args();
    let path = args.next().expect("splash_shell <recording> [rounds]");
    let rounds: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5);
    let at: f64 = std::env::var("SPLASH_AT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.21);
    let rec = reads(&path);
    let files = [
        font("baud-app/fonts/built/Regular.ttf"),
        font("baud-app/fonts/built/Bold.ttf"),
        font("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
    ];
    let faces: Vec<Face<'_>> = files.iter().map(|d| Face::parse(d).unwrap()).collect();
    let roles = [
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Bold),
        FaceRole::Fallback,
    ];
    println!(
        "round\tpath\tsize\tframes\tworst_ms\tworst_at\tmedian_ms\tover\tfed\tfed_worst_ms\tcontent_at\tcontent_ms"
    );
    for round in 0..rounds {
        for p in [Path::Cpu, Path::Convert] {
            for size in &SIZES {
                let l = life(&faces, &roles, size, &rec, at, p);
                let budget = 1e3 / f64::from(size.rate.0);
                let over = l.ms.iter().filter(|m| m.0 > budget).count();
                let (worst_at, worst) =
                    l.ms.iter()
                        .map(|m| m.0)
                        .enumerate()
                        .max_by(|a, b| a.1.total_cmp(&b.1))
                        .unwrap();
                let fed: Vec<f64> =
                    l.ms.iter()
                        .filter(|m| m.1 == Fed::Read)
                        .map(|m| m.0)
                        .collect();
                let fed_worst = fed.iter().copied().fold(0.0, f64::max);
                let mut sorted: Vec<f64> = l.ms.iter().map(|m| m.0).collect();
                sorted.sort_by(f64::total_cmp);
                let (cat, cms) = l
                    .content
                    .map_or(("-".to_string(), 0.0), |(f, m)| (f.to_string(), m));
                println!(
                    "{round}\t{}\t{}\t{}\t{worst:.2}\t{worst_at}\t{:.2}\t{over}\t{}\t{fed_worst:.2}\t{cat}\t{cms:.2}",
                    match p {
                        Path::Cpu => "cpu",
                        Path::Convert => "convert",
                    },
                    size.name,
                    l.ms.len(),
                    sorted[sorted.len() / 2],
                    fed.len(),
                );
            }
        }
    }
}
