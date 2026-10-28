//! **A CPU replica of the `flash` bench, for attribution**: which part of an
//! effect's drawing makes the area WCAG 2.3.1 counts.
//!
//! Everything that decides the frames is baud's own code, compiled from its
//! source: the driver is `baud-app/src/trail.rs` (`#[path]`, with the two
//! shims the bench gives it), the records are `Vfx::instances`, the counter is
//! `baud-bench/src/flash.rs` (`#[path]`). What is not baud's shipped code is
//! the per-pixel shading: the GPU's fragment functions are replaced by the
//! pixel oracle's references (`baud-present-macos/tests/cells.rs`, copied in
//! `refs.rs`), which the oracle holds to the GPU within one step of 255 on
//! at most 20 pixels a scene, composited by its `over` (encoded,
//! byte-rounded). So the "all" arm must reproduce the census's row before any
//! other arm is read.
//!
//! Every other arm is a **ceiling arm**: the same driver, the same moves, the
//! same records, with one term of the shading forced to zero -- wrong output
//! by construction, priced against the "all" arm on the same frames.
//!
//! Usage: flashrep <effect> <pt>x<scale> <rate> <ink> <ground> <motion> <arms>
//!   motion: long-diag | every-100ms-diag | every-200ms-diag | long-across |
//!           every-100ms-across | every-200ms-across
//!   arms:   comma list; see `Arm::parse`.

#[allow(dead_code, unused_imports, clippy::all)]
#[path = "../../baud-app/src/trail.rs"]
mod trail;

#[allow(dead_code, clippy::all)]
#[path = "../../baud-bench/src/flash.rs"]
mod flash;

mod refs;

#[allow(dead_code, clippy::all)]
mod trailx;

/// `baud-app/src/bell.rs`'s `Moment`, as the bench spells it.
mod bell {
    use baud_domain::HostTime;
    use core::time::Duration;

    pub trait Moment: Copy + Ord {
        fn elapsed_since(self, earlier: Self) -> Duration;
    }

    impl Moment for HostTime {
        fn elapsed_since(self, earlier: HostTime) -> Duration {
            self.since(earlier).unwrap_or(Duration::ZERO)
        }
    }
}

/// `baud-app/src/caret.rs`'s `Owes`.
mod caret {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Owes {
        Frame,
        Nothing,
    }
}

use baud_domain::{Cols, HostTime, Rows};
use baud_font::{Face, Px};
use baud_render::effect::{Effect, Vfx};
use baud_render::{Blend, Instance, Kind, PxSize, Rect, Renderer, Stack, Theme, TrailLook};
use flash::{Census, Rate, Scale};
use refs::Arm;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 7 {
        eprintln!("usage: flashrep <effect> <pt>x<scale> <rate> <ink> <ground> <motion> <arms>");
        std::process::exit(2);
    }
    let smear = match args[0].as_str() {
        "quad" => Some(baud_render::Shape::Quad),
        "sweep" => Some(baud_render::Shape::Sweep),
        _ => None,
    };
    let effect = match args[0].as_str() {
        "quad" | "sweep" => Effect::Fire,
        "fire" => Effect::Fire,
        "plasma" => Effect::Plasma,
        "lightning" => Effect::Lightning,
        "ice" => Effect::Ice,
        "ufo" => Effect::Ufo,
        "orb" => Effect::Orb,
        "liquid" => Effect::Liquid,
        e => panic!("effect {e}"),
    };
    let (pt, scale): (f32, u32) = {
        let (a, b) = args[1].split_once('x').expect("PTxSCALE");
        (a.parse().unwrap(), b.parse().unwrap())
    };
    let rate: u32 = args[2].parse().unwrap();
    let ink = args[3].as_str();
    let ground = args[4].as_str();
    let motion = args[5].as_str();
    let arms: Vec<Arm> = args[6].split(',').map(Arm::parse).collect();
    // An eighth argument runs the scratch driver `trailx` with that retarget
    // variant (0 its copy of the shipped rule); none runs `trail.rs` itself.
    let variant: Option<u8> = args.get(7).map(|v| v.parse().unwrap());
    if let Some(v) = variant {
        trailx::VARIANT.store(v, core::sync::atomic::Ordering::Relaxed);
    }

    let ground_word: u32 = match ground {
        "dark" => 0xFF10_1010,
        "light" => 0xFFEF_EFEF,
        "red" => 0xFFCD_0000,
        "darkred" => 0xFF1A_0000,
        g => panic!("ground {g}"),
    };
    let fill: u32 = match ink {
        "default" if ground == "light" => 0xFF27_2727,
        "default" => 0xFFD8_D8D8,
        "white" => 0xFFFF_FFFF,
        "black" => 0xFF00_0000,
        "red" => 0xFFFF_0000,
        i => panic!("ink {i}"),
    };

    // The cell and the frame, from the renderer at the size, as the bench
    // takes them.
    let face_data =
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
        ))
            .expect("face");
    let faces = [Face::parse(&face_data).expect("parse")];
    let px = PxSize::of(pt * scale as f32).expect("a size");
    let s = f64::from(scale);
    let probe = Renderer::new(
        Stack::new(&faces).unwrap(),
        px,
        Theme::dark(),
        Blend::Native,
        Cols::of_index(10),
        Rows::of_index(10),
    )
    .unwrap();
    let m = probe.metrics();
    let (cw, ch) = (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()));
    let cell = (m.cell_w.value() as f32, m.cell_h.value() as f32);
    let dc = (341.0 * s / cw).round() as usize;
    let dr = (256.0 * s / ch).round() as usize;
    let (mc, mr) = (12usize, 6usize);
    let (cols, rows) = (dc + 2 * mc + 1, dr + 2 * mr + 1);
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        px,
        Theme::dark(),
        Blend::Native,
        Cols::of_index(cols),
        Rows::of_index(rows),
    )
    .unwrap();
    let (fw, fh) = renderer.frame_size();
    let (pw, ph) = renderer.pixel_size();
    let (w, h) = (fw.value() as usize, fh.value() as usize);
    let rect_at = |row: usize, col: usize| Rect {
        left: (col as f32) * cell.0,
        top: (row as f32) * cell.1,
        right: (col as f32 + 1.0) * cell.0,
        bottom: (row as f32 + 1.0) * cell.1,
    };
    let a = rect_at(mr, mc);
    let b = if motion.ends_with("diag") {
        rect_at(mr + dr, mc + dc)
    } else {
        rect_at(mr, mc + dc)
    };
    eprintln!(
        "{pt} pt x{scale}: cell {cw}x{ch}, grid {cols}x{rows}, frame {w}x{h} (cells {}x{}), path {dc} cols {dr} rows",
        pw.value(),
        ph.value()
    );

    let (moves, end): (Vec<f64>, f64) = match motion {
        "long-diag" | "long-across" => (vec![0.2], 2.2),
        m => {
            let p = if m.starts_with("every-100ms") { 0.1 } else { 0.2 };
            let n = (3.0f64 / p).round() as i64;
            ((0..=n).map(|j| 0.2 + j as f64 * p).collect(), 4.4)
        }
    };

    for arm in arms {
        let t0 = std::time::Instant::now();
        let mut chase = match (variant, smear) {
            (None, None) => Driver::Shipped(trail::Chase::new(trail::Style::Effect(effect))),
            (None, Some(sh)) => Driver::Shipped(trail::Chase::new(trail::Style::Smear(sh))),
            (Some(_), None) => Driver::Scratch(trailx::Chase::new(trailx::Style::Effect(effect))),
            (Some(_), Some(sh)) => Driver::Scratch(trailx::Chase::new(trailx::Style::Smear(sh))),
        };
        let r = f64::from(rate);
        let frames = (end * r).round() as i64;
        // `SUB=n`: count only every n-th frame, as a census at rate / n:
        // whether a lower rate is the same animation sampled more sparsely.
        let sub: u32 = std::env::var("SUB").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        let mut census = Census::new(fw, fh, Scale(scale), Rate(rate / sub));
        for rr in [a, b] {
            let p = |v: f32| Px::of(v.max(0.0) as u32);
            census.exclude(
                p(rr.left),
                p(rr.top),
                p(rr.right - rr.left),
                p(rr.bottom - rr.top),
            );
        }
        let mut next = 0usize;
        let mut at_b = false;
        let mut seeds = std::collections::BTreeSet::new();
        let mut lit_most = 0usize;
        let mut vfx_frames = 0usize;
        let mut pixels = vec![ground_word; w * h];
        let mut records: Vec<Instance> = Vec::new();
        for k in 0..frames {
            let now = k as f64 / r;
            while next < moves.len() && moves[next] <= now + 1e-9 {
                at_b = !at_b;
                next += 1;
            }
            let cursor = if at_b { b } else { a };
            let look = chase.frame(
                Some(cursor),
                HostTime::of_secs(1000.0 + now).expect("a time"),
                cell,
            );
            // The frame: the ground, the cursor, then the trail's records in
            // order, as the cell path and its records draw them.
            pixels.fill(ground_word);
            for y in cursor.top as usize..cursor.bottom as usize {
                for x in cursor.left as usize..cursor.right as usize {
                    pixels[y * w + x] = fill;
                }
            }
            if let Some(TrailLook::Vfx(v)) = look {
                vfx_frames += 1;
                seeds.insert(v.ignition.seed.0);
                records.clear();
                v.instances((pw, ph), baud_render::Argb(fill), &mut records);
                draw(&v, &records, arm, &mut pixels, w, h);
            }
            if let Some(TrailLook::Smear(sm)) = look {
                vfx_frames += 1;
                draw_smear(&sm, fill, arm, &mut pixels, w, h);
            }
            let mut lit = 0usize;
            for y in 0..h {
                for x in 0..w {
                    let p = pixels[y * w + x];
                    let inside = |r: Rect| {
                        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                        fx > r.left && fx < r.right && fy > r.top && fy < r.bottom
                    };
                    if p != ground_word && !inside(a) && !inside(b) {
                        lit += 1;
                    }
                }
            }
            lit_most = lit_most.max(lit);
            if k % i64::from(sub) == 0 {
                census.frame(&pixels);
            }
        }
        let rep = census.finish();
        println!(
            "{}{}\t{pt}\t{scale}\t{rate}\t{ground}\t{ink}\t{motion}\t{}\tframes={}\tvfx={vfx_frames}\tignitions={}\tlit={lit_most}\tgen_flashes={}\tred_flashes={}\tgen_over={:.0}\tred_over={:.0}\tgen_ever={:.0}\tarea={:.0}\tcontiguous={:.0}\tsecond={:.2}\tapart={:.0}\tstrict={:.0}\tapart_strict={:.0}\tms={:.0}",
            args[0],
            variant.map(|v| format!("/v{v}")).unwrap_or_default(),
            arm.name(),
            frames,
            seeds.len(),
            rep.general.flashes.0,
            rep.red.flashes.0,
            rep.general.over.0,
            rep.red.over.0,
            rep.general.ever.0,
            rep.area.0,
            rep.contiguous.0,
            rep.second,
            rep.apart.0,
            rep.strict.0,
            rep.apart_strict.0,
            t0.elapsed().as_secs_f64() * 1e3
        );
    }
}

/// The records over the frame, in order, each pixel by the reference's rule
/// under `arm`: bands by the effect's band rule, particles by its particle
/// rule -- `effect_frame_by` in the pixel oracle.
fn draw(v: &Vfx, records: &[Instance], arm: Arm, out: &mut [u32], w: usize, h: usize) {
    for r in records {
        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
        let (rw, rh) = (i32::from(r.w.0), i32::from(r.h.0));
        let color = refs::unit_color(r.color);
        let particle = r.kind == Kind::Particle;
        if particle && !arm.particles() {
            continue;
        }
        for y in y0.max(0)..(y0 + rh).min(h as i32) {
            for x in x0.max(0)..(x0 + rw).min(w as i32) {
                let at = y as usize * w + x as usize;
                let centre = baud_render::Pt {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                let src = if particle {
                    let uv = baud_render::Pt {
                        x: (centre.x - x0 as f32) / rw as f32 * 2.0 - 1.0,
                        y: (centre.y - y0 as f32) / rh as f32 * 2.0 - 1.0,
                    };
                    refs::particle(v.effect, uv, color, arm)
                } else {
                    refs::band(v, centre, color, arm)
                };
                out[at] = refs::over(src, out[at]);
            }
        }
    }
}

/// The driver: `baud-app/src/trail.rs` itself, or the scratch copy with a
/// retarget variant.
enum Driver {
    Shipped(trail::Chase<HostTime>),
    Scratch(trailx::Chase<HostTime>),
}

impl Driver {
    fn frame(&mut self, cursor: Option<Rect>, now: HostTime, cell: (f32, f32)) -> Option<TrailLook> {
        match self {
            Driver::Shipped(c) => c.frame(cursor, now, cell),
            Driver::Scratch(c) => c.frame(cursor, now, cell),
        }
    }
}

/// A quad or a sweep: its one record, opaque in the cursor's colour, cut to
/// the shape by `trail::coverage` (the CPU compositor's rule, which
/// `quad_fragment` is).
fn draw_smear(s: &baud_render::Smear, fill: u32, arm: Arm, out: &mut [u32], w: usize, h: usize) {
    // `quad_instance`'s record: the smear's bounds, the fill opaque.
    let (bx0, by0, bx1, by1) = s.bounds();
    let (x0, y0) = (bx0.value() as i32, by0.value() as i32);
    let (rw, rh) = (bx1.value() as i32 - x0, by1.value() as i32 - y0);
    let color = refs::unit_color(baud_render::Argb(fill | 0xFF00_0000));
    for y in y0.max(0)..(y0 + rh).min(h as i32) {
        for x in x0.max(0)..(x0 + rw).min(w as i32) {
            let at = y as usize * w + x as usize;
            let p = baud_render::Pt {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            };
            let a = color[3] * baud_render::quad_coverage(s, p) * refs::scale(arm);
            out[at] = refs::over([color[0] * a, color[1] * a, color[2] * a, a], out[at]);
        }
    }
}
