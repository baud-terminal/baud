//! The launch splash on the CPU path, which the Linux driver draws with
//! (note AJ): what holds the effect's slot while it plays, the colour its
//! plasma is drawn in and how its remainder fades out, the trail it holds
//! back, and the rows it repaints.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::splash::{DARK, GONE, LEFT, LIGHT, OUT, glow};
use baud_render::{
    Blend, Damage, FaceRole, FaceStyle, Kind, PxSize, Rect, Renderer, Segment, Shape, Smear,
    SplashLook, SplashPhase, Stack, Strength, Target, Theme, TrailLook,
};

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn renderer<'f>(faces: &'f [Face<'f>], theme: Theme) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::with_roles(
            faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// A frame painted into `px`, which holds the last: an incremental frame
/// repaints only the rows it finds changed.
fn paint(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    px: &mut Vec<u32>,
    damage: Damage,
) -> Vec<u32> {
    let (w, h) = r.pixel_size();
    px.resize(usize::try_from(w.value() * h.value()).unwrap(), 0);
    let mut target = Target::new(px, w, w, h).unwrap();
    r.render(t, &mut target, damage);
    px.clone()
}

fn look(phase: SplashPhase) -> Option<SplashLook> {
    Some(SplashLook { phase })
}

/// A cursor trail's fire, a move across the top row.
fn cursor_fire(age: f32) -> TrailLook {
    let cell = |c: f32| Rect {
        left: c * 8.0,
        top: 0.0,
        right: c * 8.0 + 8.0,
        bottom: 17.0,
    };
    let (from, to) = (cell(2.0), cell(30.0));
    let ig = Ignition::ignite(baud_render::Pt { x: 20.0, y: 8.5 }, to, Seed(3), 17.0);
    TrailLook::Vfx(Vfx {
        effect: Effect::Fire,
        emitter: Smear {
            corners: Segment::new(from.corners(), to).at(age),
            hole: to,
            shape: Shape::Quad,
            strength: Strength::Full,
        },
        ignition: ig.at(age),
        axis: ig.dir,
    })
}

/// **While it plays the splash's plasma is the frame's effect, in the
/// lockup's amber**, on each half, and the trail the cursor
/// lit meanwhile
/// is held back -- not drawn when the plasma is over, but from the frame
/// after it lands (`set_trail(None)`). Held, the splash takes no slot: the
/// cursor's trail draws. The controls: the effect's records before and
/// after are the cursor's, in its colour.
#[test]
fn the_plasma_holds_the_effects_slot_in_amber() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    for (theme, amber) in [(Theme::dark(), DARK.amber), (Theme::light(), LIGHT.amber)] {
        let mut r = renderer(&faces, theme);
        r.set_trail(Some(cursor_fire(0.05)));
        r.set_splash(look(SplashPhase::Hold));
        assert_eq!(
            r.vfx().map(|v| v.effect),
            Some(Effect::Fire),
            "held: the cursor's"
        );
        for phase in [SplashPhase::Plasma(0.1), SplashPhase::Plasma(0.35)] {
            r.set_splash(look(phase));
            let v = r.vfx().expect("the splash's plasma");
            assert_eq!(v.effect, Effect::Plasma, "{phase:?}");
            let bands: Vec<_> = r.trail().iter().filter(|i| i.kind == Kind::Band).collect();
            assert!(!bands.is_empty());
            for b in bands {
                assert_eq!(b.color.with_alpha(amber.a()), amber, "the lockup's amber");
            }
        }
        r.set_trail(Some(cursor_fire(0.1)));
        assert_eq!(r.vfx().map(|v| v.effect), Some(Effect::Plasma));
        // The splash is over; the cursor's trail is still in flight.
        r.set_splash(None);
        r.set_trail(Some(cursor_fire(0.15)));
        assert_eq!(r.vfx(), None, "a trail lit under the plasma stays held");
        assert!(r.trail().is_empty());
        r.set_trail(None);
        r.set_trail(Some(cursor_fire(0.0)));
        assert_eq!(
            r.vfx().map(|v| v.effect),
            Some(Effect::Fire),
            "the next one draws"
        );
    }
}

/// **The held logo stays over output drawn under it, and goes whole**:
/// the shell's prompt drawn on the rows under the logo, painted
/// incrementally, keeps the logo over it -- the frame equals one painted
/// whole -- and taking the splash down repaints it away: the frame equals
/// the one never splashed. The control: the logo changed the frame.
#[test]
fn the_held_logo_stays_over_output_and_goes_whole() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces, Theme::dark());
    let mut px = Vec::new();
    let bare = paint(&mut r, &mut t, &mut px, Damage::Full);
    r.set_splash(look(SplashPhase::Hold));
    let held = paint(&mut r, &mut t, &mut px, Damage::Incremental);
    assert!(
        held.iter().zip(&bare).filter(|(a, b)| a != b).count() > 500,
        "the control: the logo is drawn"
    );
    feed(&mut t, b"\x1b[12;20Hunder the logo: a prompt and some text");
    let over = paint(&mut r, &mut t, &mut px, Damage::Incremental);
    let whole = paint(&mut r, &mut t, &mut px, Damage::Full);
    assert_eq!(over, whole, "drawn incrementally as whole");
    r.set_splash(None);
    let gone = paint(&mut r, &mut t, &mut px, Damage::Incremental);
    let mut owned2 = Owned::new(&WS);
    let mut t2 = Term::new(owned2.storage(), &WS);
    feed(
        &mut t2,
        b"\x1b[12;20Hunder the logo: a prompt and some text",
    );
    let mut r2 = renderer(&faces, Theme::dark());
    let mut px2 = Vec::new();
    assert_eq!(
        gone,
        paint(&mut r2, &mut t2, &mut px2, Damage::Full),
        "gone whole"
    );
}

/// **Its size is the first frame's**: a resize while it is up keeps the
/// glyphs at the size they were drawn at, centred again, so a window
/// dragged through sizes adds no glyph sizes to the atlas.
#[test]
fn its_size_is_the_first_frames() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut r = renderer(&faces, Theme::dark());
    r.set_splash(look(SplashPhase::Hold));
    let size = |r: &Renderer<'_, '_>| -> Vec<(u16, u16)> {
        r.splash().iter().map(|i| (i.w.0, i.h.0)).collect()
    };
    let first = size(&r);
    assert_eq!(first.len(), 6, "b, a, u, d, the prompt and the block");
    let at = |r: &Renderer<'_, '_>| (r.splash()[0].x.0, r.splash()[0].y.0);
    let before = at(&r);
    let big = WinSize {
        ws_col: 120,
        ws_row: 40,
    };
    let mut owned = Owned::new(&big);
    let t = Term::new(owned.storage(), &big);
    r.fit(&t);
    r.set_splash(look(SplashPhase::Hold));
    assert_eq!(size(&r), first, "the same glyph sizes");
    assert_ne!(at(&r), before, "centred again");
}

/// **The splash's first frame shows the whole logo, faintly, in the
/// plasma**: every record drawn at the ghost's opacity of its colour, and
/// the plasma lit; at its end every record whole. The control: held, every
/// record is whole.
#[test]
fn the_first_frame_shows_the_logo_faintly_in_the_plasma() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut r = renderer(&faces, Theme::dark());
    let alphas = |r: &Renderer<'_, '_>| -> Vec<u8> {
        r.splash().iter().map(|i| i.color.a().get()).collect()
    };
    r.set_splash(look(SplashPhase::Hold));
    assert_eq!(alphas(&r), vec![255; 6], "the control: held, whole");
    r.set_splash(look(SplashPhase::Plasma(0.0)));
    let ghost = baud_render::splash::at_alpha(DARK.amber, baud_render::splash::GHOST)
        .a()
        .get();
    assert_eq!(
        alphas(&r),
        vec![ghost; 6],
        "every record, on the first frame"
    );
    assert_eq!(r.vfx().map(|v| v.effect), Some(Effect::Plasma));
    r.set_splash(look(SplashPhase::Plasma(OUT)));
    assert!(alphas(&r).iter().all(|&a| a >= 250), "{:?}", alphas(&r));
    r.set_splash(look(SplashPhase::Plasma(GONE)));
    assert!(r.splash().is_empty(), "the logo carried out");
    assert_eq!(r.vfx(), None, "its plasma out");
}

/// **The splash's plasma is never capped as a repeat pass, and its
/// remainder fades out**: every frame of its life at 60 Hz draws one
/// plasma, the first pass's or the second's, at full strength, carries no cap and waits on no scan; once the logo is
/// carried out ([`LEFT`]) every record is drawn at no more than [`glow`]
/// of its own alpha, a band at exactly that; at [`GONE`] the plasma is
/// out. The control: before the logo is out a band is drawn whole.
#[test]
fn the_plasma_is_full_strength_and_its_remainder_fades_out() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    for theme in [Theme::dark(), Theme::light()] {
        let mut r = renderer(&faces, theme);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"$ ");
        let mut px = Vec::new();
        let swept = LEFT;
        let mut whole = false;
        for f in 0u16.. {
            let age = f32::from(f) / 60.0;
            if age >= GONE {
                break;
            }
            r.set_splash(look(SplashPhase::Plasma(age)));
            let _ = paint(&mut r, &mut t, &mut px, Damage::Full);
            let v = r.vfx().expect("the splash's plasma");
            assert_eq!(v.emitter.strength, Strength::Full, "{age}");
            assert_eq!(r.flash_held(), baud_render::Held::Full, "{age}");
            let k = baud_domain::Chan::from_unit(glow(age)).get();
            for i in r.trail() {
                assert!(i.color.a().get() <= k, "{age}: {:?} over {k}", i.color);
                if i.kind == Kind::Band {
                    assert_eq!(i.color.a().get(), k, "{age}: a band");
                }
            }
            whole |= age < swept && r.trail().iter().any(|i| i.color.a().get() == 0xFF);
        }
        assert!(
            whole,
            "the control: a band drawn whole before the sweep ends"
        );
        r.set_splash(look(SplashPhase::Plasma(GONE)));
        assert_eq!(r.vfx(), None, "out at its end");
    }
}
