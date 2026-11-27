//! `trail`: a cursor trail's frames on the CPU path, timed one by one
//! (note AJ section 23).
//!
//! **Every frame is the one the Linux driver draws** for a cursor that
//! moves: the trail set for the frame's age ([`Renderer::set_trail`]) and the
//! frame rendered with [`Damage::Incremental`] after one full frame, over
//! a few lines of output, on the dark theme -- the trail's whole life at
//! the display link's rate, from its first frame to the one after it is
//! out, which clears it.
//!
//! **The moves**: `jump`, from the third row and column to three rows and
//! ten columns short of the bottom-right corner, the window's diagonal,
//! and `step`, 30 columns right and 3 rows down from a fifth of the way
//! in, the `frame` bench's move scaled to the window.
//!
//! **The trails**: the seven effects (fire, plasma, lightning, ice, ufo,
//! orb, liquid) riding the quad, the ghost, the sweep and the quad. **The
//! sizes**: 64 by 20 cells at 14 px (1x) at 60 frames a second, 200 by
//! 50 at 28 px (2x) at 120, and 360 by 90 at 28 px, the 6120 by 3330
//! pixel frame (6K), at 60.
//!
//! **What it prints**, a line a size, move, trail and round: the worst frame,
//! the median, the sum, how many frames are over the rate's budget, and
//! FNV-1a over every frame's pixels, so two builds can be shown to draw
//! the same frames byte for byte. The first round is a warm-up and should
//! be dropped; `trail <rounds>` sets how many (default 5).
//! `TRAIL_DAMAGE=full` renders every frame with [`Damage::Full`],
//! `TRAIL_THREADS=<n>` draws an effect on at most `n` threads
//! ([`Renderer::set_threads`]; the machine's by default), and
//! `TRAIL_SIZE=<text>`, `TRAIL_MOVE=<text>` and `TRAIL_ONLY=<text>` run
//! only the sizes, moves and trails whose name holds it.

use baud_bench::flash::Rate;
use baud_core::{CursorStyle, Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{
    Blend, Damage, FaceRole, FaceStyle, Pt, PxSize, Renderer, Segment, Shape, Smear, Stack,
    Strength, Target, Theme, Trail, TrailLook,
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

struct Size {
    name: &'static str,
    ws: WinSize,
    px: f32,
    rate: Rate,
}

const SIZES: [Size; 3] = [
    Size {
        name: "64x20@1x 60Hz",
        ws: WinSize {
            ws_col: 64,
            ws_row: 20,
        },
        px: 14.0,
        rate: Rate(60),
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
    Size {
        name: "360x90@2x 60Hz",
        ws: WinSize {
            ws_col: 360,
            ws_row: 90,
        },
        px: 28.0,
        rate: Rate(60),
    },
];

/// The trails, by the names the configuration gives them.
#[derive(Clone, Copy)]
enum Kind {
    Effect(Effect),
    Ghost,
    Smear(Shape),
}

const KINDS: [(&str, Kind); 10] = [
    ("fire", Kind::Effect(Effect::Fire)),
    ("plasma", Kind::Effect(Effect::Plasma)),
    ("lightning", Kind::Effect(Effect::Lightning)),
    ("ice", Kind::Effect(Effect::Ice)),
    ("ufo", Kind::Effect(Effect::Ufo)),
    ("orb", Kind::Effect(Effect::Orb)),
    ("liquid", Kind::Effect(Effect::Liquid)),
    ("ghost", Kind::Ghost),
    ("sweep", Kind::Smear(Shape::Sweep)),
    ("quad", Kind::Smear(Shape::Quad)),
];

/// Where the cursor moves: across the window's diagonal, or a short step.
#[derive(Clone, Copy)]
enum Move {
    Jump,
    Step,
}

/// The seconds a ghost takes to land: the app's `GHOST_S`.
const GHOST_S: f32 = 0.1;

/// The trail's whole life at `size`: each frame's milliseconds and the
/// hash over every frame's pixels.
fn life(faces: &[Face<'_>], size: &Size, mv: Move, kind: Kind, damage: Damage) -> (Vec<f64>, u64) {
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
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    if let Some(n) = std::env::var("TRAIL_THREADS")
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
    let (rows, cols) = (usize::from(ws.ws_row), usize::from(ws.ws_col));
    let ((r0, c0), (r1, c1)) = match mv {
        Move::Jump => ((2, 2), (rows - 3, cols - 10)),
        Move::Step => ((rows / 5, cols / 5), (rows / 5 + 3, cols / 5 + 30)),
    };
    let (from, to) = (
        (Row::from_index(r0), Col::at_index(c0)),
        (Row::from_index(r1), Col::at_index(c1)),
    );
    let style = CursorStyle::SteadyBlock;
    let (a, b) = (
        r.cursor_rect(from.0, from.1, style),
        r.cursor_rect(to.0, to.1, style),
    );
    let seg = match kind {
        Kind::Smear(Shape::Sweep) => Segment::sweep(a.corners(), b),
        _ => Segment::new(a.corners(), b),
    };
    let unit = u16::try_from(r.metrics().cell_h.value()).map_or(f32::MAX, f32::from);
    let tail = Pt {
        x: (a.left + a.right) * 0.5,
        y: (a.top + a.bottom) * 0.5,
    };
    let ignition = Ignition::ignite(tail, b, Seed(7), unit);
    let end = match kind {
        Kind::Effect(e) => e.end(&ignition),
        Kind::Ghost => GHOST_S,
        Kind::Smear(_) => seg.lasts(),
    };
    let frame_s = 1.0 / size.rate.0 as f32;
    let mut ms = Vec::new();
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for f in 0u16.. {
        let age = f32::from(f) * frame_s;
        let look = match age <= end {
            false => None,
            true => {
                let emitter = |shape| Smear {
                    corners: seg.at(age),
                    hole: b,
                    shape,
                    strength: Strength::Full,
                    opacity: Smear::OPAQUE,
                };
                Some(match kind {
                    Kind::Effect(effect) => TrailLook::Vfx(Vfx {
                        effect,
                        emitter: emitter(Shape::Quad),
                        ignition: ignition.at(age),
                        axis: ignition.dir,
                    }),
                    Kind::Ghost => TrailLook::Ghost(Trail {
                        from,
                        to,
                        progress: age / GHOST_S,
                        origin_dy: 0.0,
                        lift: Px::of(0),
                        strength: Strength::Full,
                    }),
                    Kind::Smear(shape) => TrailLook::Smear(emitter(shape)),
                })
            }
        };
        let start = Instant::now();
        r.set_trail(look);
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render(&t, seal, &mut target, damage);
        ms.push(start.elapsed().as_secs_f64() * 1e3);
        hash = fnv(hash, &px);
        if look.is_none() {
            break;
        }
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
    let damage = match std::env::var("TRAIL_DAMAGE").as_deref() {
        Ok("full") => Damage::Full,
        _ => Damage::Incremental,
    };
    let only_size = std::env::var("TRAIL_SIZE").unwrap_or_default();
    let only_move = std::env::var("TRAIL_MOVE").unwrap_or_default();
    let only = std::env::var("TRAIL_ONLY").unwrap_or_default();
    println!(
        "round\tsize\tmove\ttrail\tframes\tworst_ms\tmedian_ms\tsum_ms\tover_budget\tbudget_ms\tfnv"
    );
    for round in 0..rounds {
        for size in SIZES.iter().filter(|s| s.name.contains(only_size.as_str())) {
            for (name_mv, mv) in [("jump", Move::Jump), ("step", Move::Step)] {
                if !name_mv.contains(only_move.as_str()) {
                    continue;
                }
                for &(name, kind) in KINDS.iter().filter(|k| k.0.contains(only.as_str())) {
                    let (mut ms, hash) = life(&faces, size, mv, kind, damage);
                    let budget = 1e3 / f64::from(size.rate.0);
                    let over = ms.iter().filter(|&&m| m > budget).count();
                    let sum: f64 = ms.iter().sum();
                    let frames = ms.len();
                    ms.sort_by(f64::total_cmp);
                    println!(
                        "{round}\t{}\t{name_mv}\t{name}\t{frames}\t{:.2}\t{:.2}\t{sum:.2}\t{over}\t{budget:.1}\t{hash:016x}",
                        size.name,
                        ms[frames - 1],
                        ms[frames / 2],
                    );
                }
            }
        }
    }
}
