//! **The effect trails drawn by the cell path**: an effect's records are
//! drawn by the effect's own fragment shader inside the cell frame's pass,
//! where the macOS cell path draws them -- after the cells and before
//! baud's own overlays -- each under the plasma's mix where the theme gives
//! one. Each frame is held to the CPU path's under the effects' bar
//! (`common::assert_frame_stacked`): a step a channel where a record
//! blends, two on llvmpipe where two or more do.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::WinSize;
use baud_font::Face;
use baud_present_vulkan::frame::Gpu;
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{Pt, Rect, Segment, Shape, Smear, Stack, Theme, TrailLook};
use common::{assert_frame_covered, lavapipe, radv};
use scenes::{Arms, feed, jetbrains_mono};

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 8,
};

/// **`effect` lit by a move from cell (2, 1) to cell (30, 5)**, `age` of
/// its life in, the emitter a quad between the two.
fn trail(a: &Arms<'_, '_, '_>, effect: Effect, age: f32) -> TrailLook {
    let m = a.renderer.metrics();
    let (cw, ch) = (m.cell_w.value() as f32, m.cell_h.value() as f32);
    let cell = |c: f32, r: f32| Rect {
        left: c * cw,
        top: r * ch,
        right: (c + 1.0) * cw,
        bottom: (r + 1.0) * ch,
    };
    let (from, to) = (cell(2.0, 1.0), cell(30.0, 5.0));
    let centre = |r: Rect| Pt {
        x: (r.left + r.right) * 0.5,
        y: (r.top + r.bottom) * 0.5,
    };
    let (fa, ta) = (centre(from), centre(to));
    let l = (ta.x - fa.x).hypot(ta.y - fa.y);
    let ignite = Ignition::ignite(fa, to, Seed(7), ch);
    let age = age * (effect.end(&ignite) / effect.life()).min(1.0);
    TrailLook::Vfx(Vfx {
        effect,
        emitter: Smear {
            corners: Segment::new(from.corners(), to).at(age),
            hole: to,
            shape: Shape::Quad,
            strength: baud_render::Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: ignite.at(age),
        axis: Pt {
            x: (ta.x - fa.x) / l,
            y: (ta.y - fa.y) / l,
        },
    })
}

/// **`effect` on `theme`, the cell frame held to the CPU frame**: drawn,
/// none of its records skipped, within the bar.
fn effect_on(gpu: &Gpu, theme: Theme, effect: Effect) {
    let polarity = theme.polarity();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let r = scenes::renderer(Stack::new(&faces).expect("a stack"), theme, WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"the text under the trail\r\nand more of it");
    let mut a = Arms::new(gpu, gpu.arm(1), r, &t);
    for age in [0.1f32, 0.4] {
        let look = trail(&a, effect, age);
        a.renderer.set_trail(Some(look));
        // The effect's pipelines, built on the worker: the frames until
        // they are leave the trail undrawn.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while a.cell_frame(&mut t).1.trails.0 > 0 {
            assert!(
                std::time::Instant::now() < until,
                "the effect's pipelines never built"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let f = a.draw(&mut t);
        let name = format!("{effect:?} at {age} on {polarity:?}");
        assert_eq!(f.skipped.trails.0, 0, "{name}: the trail's records undrawn");
        assert!(
            f.cell.iter().zip(&f.cpu).any(|(c, p)| c != p) || f.blended.iter().any(|&b| b),
            "{name}: the premise, the trail lights pixels"
        );
        assert_frame_covered(
            &format!("{name}, the cell frame against the CPU path's"),
            &f.cell,
            &f.cpu,
            (&f.covers, &f.ties),
            a.bar,
            f.width,
        );
    }
}

/// Every effect on the dark theme: the plasma screened over it.
fn every_effect(gpu: &Gpu) {
    for effect in Effect::ALL {
        effect_on(gpu, Theme::dark(), effect);
    }
    effect_on(gpu, Theme::light(), Effect::Plasma);
}

/// **Every effect, and the plasma on both polarities, as the CPU path
/// draws it**, on lavapipe. Named break: the trail's records left undrawn
/// by the cell path, which handed every frame with a trail to the CPU arm.
#[test]
fn every_effect_is_drawn_by_the_cell_path_as_the_cpu_path_draws_it() {
    every_effect(&lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn every_effect_is_drawn_by_the_cell_path_as_the_cpu_path_draws_it_on_radv() {
    every_effect(&radv());
}

/// **The allowance is the cover, both ways, under term-by-term rounding**:
/// a pixel three records blend passes three steps off and fails four; one
/// record's pixel fails two steps, so counting does not loosen the low
/// covers; an unblended pixel fails one. Each planted on a frame whose
/// other pixels are exact, through the helper the scenes use.
#[test]
fn the_cover_is_the_cap_both_ways() {
    use common::{Bar, Cover, Rounding};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let bar = Bar {
        rounding: Rounding::TermByTerm,
        driver: ash::vk::DriverId::MESA_LLVMPIPE,
    };
    let want = [0xff20_2020u32; 4];
    let covers = [Cover(0), Cover(1), Cover(3), Cover(1)];
    let ties = [common::Ties::AWAY; 4];
    let off =
        |steps: [u32; 4]| -> Vec<u32> { want.iter().zip(steps).map(|(&w, s)| w + s).collect() };
    let holds = |got: Vec<u32>| {
        catch_unwind(AssertUnwindSafe(|| {
            assert_frame_covered(
                "a control",
                &got,
                &want,
                (&covers, &ties),
                bar,
                baud_present_wayland::Px(4),
            )
        }))
        .is_ok()
    };
    assert!(holds(off([0, 1, 3, 0])), "each at its cap holds");
    assert!(!holds(off([0, 1, 4, 0])), "n + 1 steps at cover 3");
    assert!(!holds(off([0, 2, 3, 0])), "2 steps at cover 1");
    assert!(!holds(off([1, 1, 3, 0])), "a step at cover 0");
}

/// **The allowance is the bound term-by-term rounding reaches, and no
/// looser**: records blended over one page byte with the same source and
/// alpha on both arms -- the sum rounded once, as the CPU path blends, and
/// each term rounded to a byte, as lavapipe does -- end at most `n` steps
/// apart over a fixed sweep, and `n` steps apart at one pixel for each `n`
/// of one to four.
#[test]
fn the_allowance_is_the_bound_term_by_term_rounding_reaches() {
    use common::{Cover, Rounding, Tie, allowance};
    let apart = |page: u8, records: &[(u8, u8)]| -> u32 {
        let (mut once, mut terms) = (u32::from(page), u32::from(page));
        for &(alpha, source) in records {
            let (a, s) = (u32::from(alpha), u32::from(source));
            once = (a * s + (255 - a) * once + 127) / 255;
            terms = (a * s + 127) / 255 + ((255 - a) * terms + 127) / 255;
        }
        once.abs_diff(terms)
    };
    let cap = |n: usize| {
        allowance(
            Cover(u8::try_from(n).expect("a cover")),
            Tie::Away,
            Rounding::TermByTerm,
        )
    };
    let reached: [(u8, &[(u8, u8)]); 4] = [
        (90, &[(72, 240)]),
        (171, &[(98, 243), (11, 142)]),
        (253, &[(147, 195), (17, 157), (8, 13)]),
        (223, &[(124, 252), (40, 131), (13, 150), (18, 78)]),
    ];
    for (page, records) in reached {
        assert_eq!(
            apart(page, records),
            cap(records.len()),
            "{page} under {records:?}"
        );
    }
    let mut x = 0x2545_f491_u32;
    let mut byte = |below: u32| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        u8::try_from(x % below).expect("a byte")
    };
    for n in 1..=4 {
        for _ in 0..50_000 {
            let page = byte(256);
            let records: Vec<(u8, u8)> = (0..n).map(|_| (1 + byte(255), byte(256))).collect();
            assert!(apart(page, &records) <= cap(n), "{page} under {records:?}");
        }
    }
}

/// **The fireworks on `gpu`, drawn by the cell path as the CPU path draws
/// them**: a show from the middle of the grid, early and late in its hold,
/// over text, through the scenes' trail check (`Arms::check_trail`).
fn fireworks_on(gpu: &Gpu) {
    use baud_render::effect::Seed;
    use baud_render::fireworks::{BAUD, Form, HOLD, ORIGINS, Show};
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let r = scenes::renderer(Stack::new(&faces).expect("a stack"), Theme::dark(), WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"the text under the show\r\nand more of it");
    let mut a = Arms::new(gpu, gpu.arm(1), r, &t);
    let m = a.renderer.metrics();
    let at = Pt {
        x: 20.0 * m.cell_w.value() as f32,
        y: 4.0 * m.cell_h.value() as f32,
    };
    for age in [0.3f32, 1.2] {
        a.renderer.set_fireworks(&[Show {
            age,
            hold: HOLD,
            origins: [at; ORIGINS],
            hole: at,
            scale: 1.0,
            seed: Seed(41),
            look: BAUD,
            form: Form::Moving,
        }]);
        a.check_trail(&mut t, &format!("the fireworks at {age}"));
    }
}

/// **The fireworks drawn by the cell path as the CPU path draws them**, on
/// lavapipe. Named break: the stars left undrawn, the frame the CPU arm's.
#[test]
fn the_fireworks_are_drawn_by_the_cell_path_as_the_cpu_path_draws_them() {
    fireworks_on(&lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn the_fireworks_are_drawn_by_the_cell_path_as_the_cpu_path_draws_them_on_radv() {
    fireworks_on(&radv());
}

/// **A smear on `gpu`, quad and sweep, drawn by the cell path as the CPU
/// path draws it**: the corners part way from cell (40, 6) to the cursor,
/// over text, through the scenes' trail check (`Arms::check_trail`).
fn smears_on(gpu: &Gpu) {
    use baud_render::{Segment, Shape, Smear, Strength};
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let r = scenes::renderer(Stack::new(&faces).expect("a stack"), Theme::dark(), WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"the text under the smear\r\nand more of it");
    let mut a = Arms::new(gpu, gpu.arm(1), r, &t);
    for shape in [Shape::Quad, Shape::Sweep] {
        for p in [0.1f32, 0.5] {
            let (cr, cc) = t.cursor();
            let hole = a.renderer.cursor_rect(cr, cc, t.cursor_style());
            let from = a
                .renderer
                .cursor_rect(
                    baud_domain::Row::from_index(6),
                    baud_domain::Col::at_index(36),
                    t.cursor_style(),
                )
                .corners();
            a.renderer.set_trail(Some(TrailLook::Smear(Smear {
                corners: Segment::new(from, hole).at(p),
                hole,
                shape,
                strength: Strength::Full,
                opacity: Smear::OPAQUE,
            })));
            a.check_trail(&mut t, &format!("the {shape:?} at {p}"));
        }
    }
}

/// **The quad and the sweep drawn by the cell path as the CPU path draws
/// them**, on lavapipe. Named break: the smear left undrawn, the frame the
/// CPU arm's.
#[test]
fn a_smear_is_drawn_by_the_cell_path_as_the_cpu_path_draws_it() {
    smears_on(&lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_smear_is_drawn_by_the_cell_path_as_the_cpu_path_draws_it_on_radv() {
    smears_on(&radv());
}
