//! **`frame_gpu`'s trail arm draws every effect, and the same frame each
//! time.** The arm prices each effect's frame over [`baud_bench::trail`]'s
//! fixed jump; a look that drew nothing would price the frame without a
//! trail under the effect's name. This draws the jump on the CPU path over a
//! screen of text: each effect's frame differs from the trail-off frame, the
//! look is the same when built again, and the trail taken off again draws
//! the trail-off frame, the control that says the difference is the trail's.
//! The bench checks its own GPU frames the same way when it runs.

use baud_bench::trail;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::effect::Effect;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};

const WS: WinSize = WinSize {
    ws_col: 64,
    ws_row: 20,
};

/// One frame of `t`, every pixel repainted.
fn draw(r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Vec<u32> {
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).expect("a size")];
    {
        let mut target = Target::new(&mut px, w, w, h).expect("a target");
        let seal = t.seal_writes();
        r.render(t, seal, &mut target, Damage::Full);
    }
    px
}

#[test]
fn every_effect_on_the_fixed_jump_draws_the_same_frame_each_time() {
    let face = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the committed face");
    let faces = [Face::parse(&face).expect("a face")];
    let (cols, rows) = (
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    );
    let mut r = Renderer::new(
        Stack::new(&faces).expect("a stack"),
        PxSize::of(14.0).expect("a size"),
        Theme::dark(),
        Blend::Native,
        cols,
        rows,
    )
    .expect("metrics");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut text = Vec::new();
    for i in 0..WS.ws_row - 1 {
        text.extend_from_slice(format!("{i:>4}  drwxr-xr-x  baud  staff  src\r\n").as_bytes());
    }
    t.write_at(BytePos::ZERO, &text);
    t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
    r.set_trail(None);
    let off = draw(&mut r, &mut t);
    assert!(off.iter().any(|&p| p != off[0]), "the screen drew its text");
    let jump = trail::jump(cols, rows);
    let mut names = vec![trail::name(None)];
    for effect in Effect::ALL {
        let look = trail::look(&r, effect, jump);
        assert_eq!(
            look,
            trail::look(&r, effect, jump),
            "{effect:?}: the same look every rep"
        );
        r.set_trail(Some(look));
        let lit = draw(&mut r, &mut t);
        let changed = lit.iter().zip(&off).filter(|(a, b)| a != b).count();
        eprintln!(
            "{}: {changed} pixels not the trail-off frame's",
            trail::name(Some(effect))
        );
        assert!(changed > 0, "{effect:?} drew nothing over the jump");
        names.push(trail::name(Some(effect)));
    }
    r.set_trail(None);
    assert!(
        draw(&mut r, &mut t) == off,
        "the control: the trail taken off draws the trail-off frame"
    );
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "each line's name is its own: {names:?}"
    );
}

/// The bench builds its trail frames from this module's look and draws the
/// renderer's trail records with the frame's own.
#[test]
fn the_bench_draws_the_fixed_look_into_its_records() {
    let code: String = include_str!("../benches/frame_gpu.rs")
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for call in [
        "baud_bench::trail::look(",
        "baud_bench::trail::jump(",
        "records.extend_from_slice(r.trail())",
        "r.set_trail(",
    ] {
        assert!(code.contains(call), "frame_gpu calls `{call}`");
    }
}
