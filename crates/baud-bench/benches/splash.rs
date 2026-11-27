//! `splash`: the launch logo's frames on the CPU path, timed one by one
//! (note AJ section 14).
//!
//! **Every frame is the one the Linux driver draws**: the splash set for
//! the frame's age ([`Renderer::set_splash`]) and yielded to the shell's
//! cells ([`Renderer::splash_yields_to`]), and the frame rendered with
//! [`Damage::Incremental`] after one full frame, over a prompt and a few
//! lines of output under the logo, on the dark theme -- the splash's whole
//! life at the display link's rate, from its first frame to
//! [`baud_render::splash::GONE`].
//!
//! **The sizes**: 64 by 20 cells at 14 px (1x), 200 by 50 at 28 px (2x)
//! at 60 and 120 frames a second, and 360 by 90 at 28 px, the 6120 by
//! 3330 pixel frame (6K), at 60.
//!
//! **What it prints**, a line a size and round: the worst frame, the
//! median, how many frames are over the rate's budget, and FNV-1a over
//! every frame's pixels, so two builds can be shown to draw the same
//! frames byte for byte. The first round is a warm-up and should be
//! dropped; `splash <rounds>` sets how many (default 5).
//! `SPLASH_DAMAGE=full` renders every frame with [`Damage::Full`].
//! `SPLASH_THREADS=<n>` draws the plasma on at most `n` threads
//! ([`Renderer::set_threads`]; the machine's by default),
//! `SPLASH_SIZE=<text>` runs only the sizes whose name holds it, and
//! `SPLASH_FRAMES=1` prints each frame's milliseconds after its line.
//! Two arms more at 2x and 60 Hz: the light theme, and the dark theme's
//! page translucent, as the glass draws it.

use baud_bench::flash::Rate;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Blend, Damage, FaceRole, FaceStyle, PxSize, Renderer, SplashLook, SplashPhase, Stack, Target,
    Theme,
};
use std::time::Instant;

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// FNV-1a over the frame's words, a word a step, folded into `h`.
fn fnv(mut h: u64, px: &[u32]) -> u64 {
    for &w in px {
        h ^= u64::from(w);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

#[derive(Clone, Copy)]
enum Ground {
    Dark,
    Light,
    /// The dark theme's page at the default glass's alpha.
    Glass,
}

struct Size {
    name: &'static str,
    ground: Ground,
    ws: WinSize,
    px: f32,
    rate: Rate,
}

const SIZES: [Size; 6] = [
    Size {
        name: "64x20@1x 60Hz dark",
        ground: Ground::Dark,
        ws: WinSize {
            ws_col: 64,
            ws_row: 20,
        },
        px: 14.0,
        rate: Rate(60),
    },
    Size {
        name: "200x50@2x 60Hz dark",
        ground: Ground::Dark,
        ws: WinSize {
            ws_col: 200,
            ws_row: 50,
        },
        px: 28.0,
        rate: Rate(60),
    },
    Size {
        name: "200x50@2x 120Hz dark",
        ground: Ground::Dark,
        ws: WinSize {
            ws_col: 200,
            ws_row: 50,
        },
        px: 28.0,
        rate: Rate(120),
    },
    Size {
        name: "360x90@2x 60Hz dark",
        ground: Ground::Dark,
        ws: WinSize {
            ws_col: 360,
            ws_row: 90,
        },
        px: 28.0,
        rate: Rate(60),
    },
    Size {
        name: "200x50@2x 60Hz light",
        ground: Ground::Light,
        ws: WinSize {
            ws_col: 200,
            ws_row: 50,
        },
        px: 28.0,
        rate: Rate(60),
    },
    Size {
        name: "200x50@2x 60Hz glass",
        ground: Ground::Glass,
        ws: WinSize {
            ws_col: 200,
            ws_row: 50,
        },
        px: 28.0,
        rate: Rate(60),
    },
];

/// The splash's whole life at `size`: each frame's milliseconds and the
/// hash over every frame's pixels.
fn life(faces: &[Face<'_>], size: &Size, damage: Damage) -> (Vec<f64>, u64) {
    let ws = WinSize {
        ws_col: size.ws.ws_col,
        ws_row: size.ws.ws_row,
    };
    let mut r = Renderer::new(
        Stack::with_roles(
            faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap(),
        PxSize::of(size.px).unwrap(),
        match size.ground {
            Ground::Dark => Theme::dark(),
            Ground::Light => Theme::light(),
            Ground::Glass => Theme {
                bg_alpha: Chan::from_u8(0xC0),
                ..Theme::dark()
            },
        },
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    if let Some(n) = std::env::var("SPLASH_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        r.set_threads(n);
    }
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let mut text = Vec::new();
    for i in 0..ws.ws_row / 2 {
        text.extend_from_slice(
            format!("total {i:>4}  drwxr-xr-x  baud  staff  4096 Oct 29 12:00 src\r\n").as_bytes(),
        );
    }
    text.extend_from_slice(b"\x1b[1;32mbaud\x1b[0m \x1b[33m\xe2\x9d\xaf\x1b[0m ");
    t.write_at(BytePos::ZERO, &text);
    t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render(&t, seal, &mut target, Damage::Full);
    }
    let frame_s = 1.0 / size.rate.0 as f32;
    let mut ms = Vec::new();
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for f in 0u16.. {
        let age = f32::from(f) * frame_s;
        if age >= baud_render::splash::GONE {
            break;
        }
        let start = Instant::now();
        r.set_splash(Some(SplashLook {
            phase: SplashPhase::Plasma(age),
        }));
        // Around the shell's cells, as each driver hands it.
        r.splash_yields_to(&t, Px::of(0));
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render(&t, seal, &mut target, damage);
        ms.push(start.elapsed().as_secs_f64() * 1e3);
        hash = fnv(hash, &px);
    }
    (ms, hash)
}

fn main() {
    baud_bench::warm::refuse_while_warm();
    let rounds: usize = baud_bench::args()
        .next()
        .and_then(|a| a.parse().ok())
        .unwrap_or(5);
    let (regular, bold) = (
        font("crates/baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let damage = match std::env::var("SPLASH_DAMAGE").as_deref() {
        Ok("full") => Damage::Full,
        _ => Damage::Incremental,
    };
    println!("round\tsize\tframes\tworst_ms\tmedian_ms\tover_budget\tbudget_ms\tfnv");
    let only = std::env::var("SPLASH_SIZE").unwrap_or_default();
    let frames_too = std::env::var("SPLASH_FRAMES").is_ok_and(|v| v == "1");
    for round in 0..rounds {
        for size in SIZES.iter().filter(|s| s.name.contains(only.as_str())) {
            let (mut ms, hash) = life(&faces, size, damage);
            let each: Vec<String> = ms.iter().map(|m| format!("{m:.1}")).collect();
            let budget = 1e3 / f64::from(size.rate.0);
            let over = ms.iter().filter(|&&m| m > budget).count();
            let frames = ms.len();
            ms.sort_by(f64::total_cmp);
            println!(
                "{round}\t{}\t{frames}\t{:.2}\t{:.2}\t{over}\t{budget:.1}\t{hash:016x}",
                size.name,
                ms[frames - 1],
                ms[frames / 2],
            );
            if frames_too {
                println!("#\t{}", each.join(" "));
            }
        }
    }
}
