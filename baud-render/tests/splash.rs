//! The launch splash on the CPU path, which the Linux driver draws with
//! (note AJ): what holds the effect's slot while it emerges and vanishes,
//! the colour its plasma is drawn in, the trail it holds back, and the
//! rows it repaints.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::splash::{DARK, LIGHT, end};
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

/// **While it emerges or vanishes the splash's plasma is the frame's
/// effect, in the lockup's amber**, on each half, and the trail the cursor
/// lit meanwhile
/// is held back -- not drawn when the plasma is over, but from the frame
/// after it lands (`set_trail(None)`). Held, the splash takes no slot: the
/// cursor's trail draws. The controls: the effect's records before and
/// after are the cursor's, in its colour.
#[test]
fn the_vanish_holds_the_effects_slot_in_amber() {
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
        for phase in [SplashPhase::Emerge(0.1), SplashPhase::Vanish(0.1)] {
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
        // The vanish is over; the cursor's trail is still in flight.
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
    assert!(end() > 0.5);
}

/// **The emerge's first frame shows the whole logo, faintly, in the
/// plasma**: every record drawn at the ghost's opacity of its colour, and
/// the plasma lit; at its end every record whole. The control: held, every
/// record is whole.
#[test]
fn the_emerges_first_frame_shows_the_logo_faintly_in_the_plasma() {
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
    r.set_splash(look(SplashPhase::Emerge(0.0)));
    let ghost = baud_render::splash::at_alpha(DARK.amber, baud_render::splash::GHOST)
        .a()
        .get();
    assert_eq!(
        alphas(&r),
        vec![ghost; 6],
        "every record, on the first frame"
    );
    assert_eq!(r.vfx().map(|v| v.effect), Some(Effect::Plasma));
    r.set_splash(look(SplashPhase::Emerge(end())));
    assert!(alphas(&r).iter().all(|&a| a >= 254), "{:?}", alphas(&r));
    assert_eq!(r.vfx(), None, "its plasma out");
}

/// **The vanish is drawn under the flash thresholds, the emerge is not**:
/// the vanish passes over the box the emerge's plasma lit less than a
/// second before, so it is drawn as a trail's repeat pass is -- its bands
/// alone, each at most the cap -- and the cap is the amber's over the
/// lockup as well as over the page, so it is no more than the amber's
/// cap over the wordmark and over the amber. The control: the emerge is
/// full strength and carries its ions, records that are not bands.
#[test]
fn the_vanish_is_drawn_under_the_flash_thresholds() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    for (theme, c) in [(Theme::dark(), DARK), (Theme::light(), LIGHT)] {
        let page = theme.bg;
        let mut r = renderer(&faces, theme);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut px = Vec::new();
        r.set_splash(look(SplashPhase::Emerge(0.05)));
        let _ = paint(&mut r, &mut t, &mut px, Damage::Full);
        let v = r.vfx().expect("the emerge's plasma");
        assert_eq!(v.emitter.strength, Strength::Full, "the emerge");
        assert!(
            r.trail().iter().any(|i| i.kind != Kind::Band),
            "the emerge's ions"
        );
        // A frame scans so many caps; a few frames scan every ground.
        for age in [0.10, 0.11, 0.12, 0.13, 0.14, 0.15] {
            r.set_splash(look(SplashPhase::Vanish(age)));
            let _ = paint(&mut r, &mut t, &mut px, Damage::Full);
        }
        let v = r.vfx().expect("the vanish's plasma");
        assert_eq!(v.emitter.strength, Strength::UnderThreshold, "the vanish");
        assert_eq!(r.flash_held(), baud_render::Held::Capped);
        let k = r.flash_cap();
        assert!(k.get() > 0);
        let opaque = |a: baud_render::Argb| a.with_alpha(baud_domain::Chan::from_u8(0xFF));
        for text in [c.wordmark, c.amber] {
            let over =
                baud_render::under_threshold_over(opaque(c.amber), opaque(text), opaque(page));
            assert!(k.get() <= over.get(), "{text:?}: {k:?} over {over:?}");
        }
        assert!(!r.trail().is_empty());
        for i in r.trail() {
            assert_eq!(i.kind, Kind::Band, "its bands alone");
            assert!(i.color.a().get() <= k.get(), "{:?} over {k:?}", i.color);
        }
    }
}
