//! The launch splash on the CPU path, which the Linux driver draws with
//! (note AJ): what holds the effect's slot while it plays, the colour its
//! plasma is drawn in and how its remainder fades out, the trail it holds
//! back, and the rows it repaints.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::splash::{DARK, GONE, LEFT, OUT, glow, light};
use baud_render::{
    Blend, Damage, FaceRole, FaceStyle, Instance, Kind, PxSize, Rect, Renderer, Segment, Shape,
    Smear, SplashLook, SplashPhase, Stack, Strength, Target, Theme, TrailLook,
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
    let seal = t.seal_writes();
    r.render(t, seal, &mut target, damage);
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
            opacity: Smear::OPAQUE,
        },
        ignition: ig.at(age),
        axis: ig.dir,
    })
}

/// **While it plays the splash's plasma is the frame's effect**, on each
/// half, and the trail the cursor lit meanwhile
/// is held back -- not drawn when the plasma is over, but from the frame
/// after it lands (`set_trail(None)`). Held, the splash takes no slot: the
/// cursor's trail draws. The controls: the effect's records before and
/// after are the cursor's, in its colour.
#[test]
fn the_plasma_holds_the_effects_slot() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    for theme in [Theme::dark(), Theme::light()] {
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
            assert!(r.trail().iter().any(|i| i.kind == Kind::Band));
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

/// **The plasma is the colour of the glyph it is over**: on every frame of
/// the splash at 60 Hz, a record of the plasma left of the word's last
/// letter's right edge is the word's ink -- near-white on the dark page,
/// on the glass page over it too, and dark on the light page and its
/// glass -- and one
/// right of `❯`'s pen the amber, an ion by where its centre is; between
/// them the colour steps from the one to the other, never leaving the
/// span between the two, in steps of no more than an eighth of the way
/// ([`SLIVERS`]), so no straight colour edge crosses the glow. Each band
/// row is still one span, split without a gap or an overlap. The controls: both colours are drawn, on the frames
/// the plasma is over the word; and a cursor's trail is in the cursor's
/// colour, as before.
#[test]
fn the_plasma_is_the_colour_of_the_glyph_it_is_over() {
    use baud_render::splash::{PROMPT, SLIVERS, WORDMARK, origin, scale};
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let glass = Theme {
        bg_alpha: baud_domain::Chan::from_u8(0xC0),
        ..Theme::dark()
    };
    let light_glass = Theme {
        bg_alpha: baud_domain::Chan::from_u8(0xC0),
        ..Theme::light()
    };
    for (name, theme, c) in [
        ("dark", Theme::dark(), DARK),
        ("glass", glass, DARK),
        ("light", Theme::light(), light()),
        ("light glass", light_glass, light()),
    ] {
        let mut r = renderer(&faces, theme);
        let (w, h) = r.pixel_size();
        let side = |v: u32| f32::from(u16::try_from(v).unwrap());
        let frame = (side(w.value()), side(h.value()));
        let s = scale(frame);
        let o = origin(frame, s);
        // The first column `paint` turns the colour at: the seam's, rounded.
        let word =
            (o.x + (WORDMARK.x + 4.0 * (WORDMARK.size * 0.6 + WORDMARK.tracking)) * s).round();
        let mark = o.x + PROMPT.x * s;
        let between = |x: baud_render::Argb| {
            let ch = |v: baud_render::Argb| [v.r().get(), v.g().get(), v.b().get()];
            ch(x)
                .iter()
                .zip(ch(c.wordmark).iter().zip(ch(c.amber)))
                .all(|(&v, (&a, b))| v >= a.min(b) && v <= a.max(b))
        };
        let (mut inked, mut ambered) = (0, 0);
        for f in 0u16.. {
            let age = f32::from(f) / 60.0;
            if age >= GONE {
                break;
            }
            r.set_splash(look(SplashPhase::Plasma(age)));
            let recs = r.trail();
            assert!(!recs.is_empty(), "{name} at {age}");
            for i in recs {
                let (l, rt) = (f32::from(i.x.0), f32::from(i.x.0) + f32::from(i.w.0));
                // A band by its span, an ion by the whole pixel `paint`
                // takes as its centre.
                let (at, before) = match i.kind {
                    Kind::Band => ((l, rt), rt <= word),
                    _ => {
                        let c = l + f32::from(i.w.0 / 2);
                        ((c, c), c < word)
                    }
                };
                let colour = i.color.with_alpha(c.amber.a());
                if before {
                    assert_eq!(colour, c.wordmark, "{name} at {age}: {i:?} over the word");
                    inked += 1;
                } else if at.0 >= mark {
                    assert_eq!(colour, c.amber, "{name} at {age}: {i:?} over the mark");
                    ambered += 1;
                } else {
                    assert!(between(colour), "{name} at {age}: {i:?} over the gap");
                }
            }
            let mut bands: Vec<_> = recs.iter().filter(|i| i.kind == Kind::Band).collect();
            bands.sort_by_key(|i| (i.y.0, i.x.0));
            let ch = |v: baud_render::Argb| [v.r().get(), v.g().get(), v.b().get()].map(i32::from);
            let most = ch(c.wordmark)
                .iter()
                .zip(ch(c.amber))
                .map(|(&a, b)| (a - b).abs() / i32::from(SLIVERS) + 1)
                .collect::<Vec<_>>();
            for p in bands.windows(2) {
                if p[0].y.0 == p[1].y.0 {
                    assert_eq!(
                        i32::from(p[0].x.0) + i32::from(p[0].w.0),
                        i32::from(p[1].x.0),
                        "{name} at {age}: a band row is one span"
                    );
                    let step = ch(p[0].color)
                        .iter()
                        .zip(ch(p[1].color))
                        .zip(&most)
                        .all(|((&a, b), &m)| (a - b).abs() <= m);
                    assert!(
                        step,
                        "{name} at {age}: {:?} to {:?} in one step",
                        p[0].color, p[1].color
                    );
                }
            }
        }
        assert!(inked > 0 && ambered > 0, "{name}: {inked} {ambered}");
        r.set_splash(None);
        r.set_trail(Some(cursor_fire(0.05)));
        let cursor = r.trail()[0].color;
        assert!(
            r.trail()
                .iter()
                .all(|i| i.color.with_alpha(cursor.a()) == cursor),
            "{name}: the cursor's trail in one colour"
        );
    }
}

/// **Each pixel of the plasma is in one band record**: on every frame of
/// the splash at 60 Hz, at 1x and 2x, on the dark and the light page, the
/// band records the renderer hands every path -- the trail's, which the
/// CPU path composites, and the same records among the instances a GPU
/// path uploads -- cover no pixel twice, so no row of the glow is drawn
/// twice and no line shows where one band meets the next, and no row
/// between the first band and the last is left out. The lockup's scale
/// puts its band edges between pixel rows at both sizes. The control: the
/// bands cover the logo's rows on every frame.
#[test]
fn each_pixel_of_the_plasma_is_in_one_band() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    for (size, theme) in [
        (14.0, Theme::dark()),
        (14.0, Theme::light()),
        (28.0, Theme::dark()),
        (28.0, Theme::light()),
    ] {
        let mut r = Renderer::new(
            Stack::with_roles(
                &faces,
                &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
            )
            .unwrap(),
            PxSize::of(size).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        let (w, h) = r.pixel_size();
        let (w, h) = (
            usize::try_from(w.value()).unwrap(),
            usize::try_from(h.value()).unwrap(),
        );
        let cover = |recs: &mut dyn Iterator<Item = &baud_render::Instance>| {
            let mut n = vec![0u8; w * h];
            for i in recs.filter(|i| i.kind == Kind::Band) {
                let at = |v: i16| usize::try_from(v.max(0)).unwrap();
                let (x0, y0) = (at(i.x.0), at(i.y.0));
                let x1 = (x0 + usize::from(i.w.0)).min(w);
                let y1 = (y0 + usize::from(i.h.0)).min(h);
                for y in y0..y1 {
                    for x in x0..x1 {
                        n[y * w + x] += 1;
                    }
                }
            }
            n
        };
        for f in 0u16.. {
            let age = f32::from(f) / 60.0;
            if age >= GONE {
                break;
            }
            r.set_splash(look(SplashPhase::Plasma(age)));
            let what = format!("{size} px, {:?} at {age}", theme.polarity());
            for (path, n) in [
                ("the trail's", cover(&mut r.trail().iter())),
                ("the instances'", cover(&mut r.instances())),
            ] {
                let twice = n.iter().filter(|&&c| c > 1).count();
                let once = n.iter().filter(|&&c| c == 1).count();
                assert!(once > 1000, "{what}: {path} bands cover {once} pixels");
                assert_eq!(twice, 0, "{what}: {path} bands cover {twice} pixels twice");
                let rows: Vec<usize> = (0..h)
                    .filter(|&y| n[y * w..(y + 1) * w].iter().any(|&c| c > 0))
                    .collect();
                assert_eq!(
                    rows.last().unwrap() - rows[0] + 1,
                    rows.len(),
                    "{what}: {path} bands leave a row out"
                );
            }
        }
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

/// **The splash's plasma fills nothing**: its filaments and ions alone,
/// with no tube inside the lockup's box and no sheath around it. A cursor
/// trail's plasma keeps both. Over the box's top row, a fifth of a block
/// in from its top, the light is under 1/255 at 48 of 64 points or more
/// (the braid lights the rest), and a third of a block above the box it
/// is under 1/255 at every point; on the frame, the page at the box's top
/// centre is the page without the splash. The control: under a trail's
/// plasma over the same box, the points inside, but for the block it
/// leaves out, are lit over 0.2 by the tube, and those above over 0.01 by
/// the sheath. Perturbation: the splash's body filled again lights 57 of
/// the 64 points inside.
#[test]
fn the_splash_plasma_fills_nothing_round_its_strands() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut r = renderer(&faces, Theme::dark());
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut px = Vec::new();
    let bare = paint(&mut r, &mut t, &mut px, Damage::Full);
    let age = 0.05;
    r.set_splash(look(SplashPhase::Plasma(age)));
    let drawn = paint(&mut r, &mut t, &mut px, Damage::Full);
    let v = r.vfx().expect("the splash's plasma");
    let xs = v.emitter.corners.0.iter().map(|p| p.x);
    let ys = v.emitter.corners.0.iter().map(|p| p.y);
    let (left, right) = (
        xs.clone().fold(f32::MAX, f32::min),
        xs.fold(f32::MIN, f32::max),
    );
    let top = ys.fold(f32::MAX, f32::min);
    let unit = v.ignition.unit;
    let trail = Vfx {
        ignition: Ignition::ignite(v.ignition.tail, v.emitter.hole, v.ignition.seed, unit).at(age),
        ..v
    };
    let light = |v: &Vfx, x: f32, y: f32| baud_render::plasma::light(v, baud_render::Pt { x, y });
    let row = |y: f32| {
        (0u16..64)
            .map(move |i| left + (right - left) * (f32::from(i) + 0.5) / 64.0)
            .map(move |x| (x, y))
    };
    let inside = row(top + 0.2 * unit);
    let above = row(top - unit / 3.0);
    let unlit = inside
        .clone()
        .filter(|&(x, y)| light(&v, x, y) < 1.0 / 255.0)
        .count();
    assert!(
        unlit >= 48,
        "the splash's box lit at {} of 64 points",
        64 - unlit
    );
    for (x, y) in above.clone() {
        assert!(
            light(&v, x, y) < 1.0 / 255.0,
            "the sheath at ({x}, {y}): {}",
            light(&v, x, y)
        );
    }
    let hole = v.emitter.hole;
    let outside = |&(x, y): &(f32, f32)| !hole.contains(baud_render::Pt { x, y });
    for ((x, y), least) in inside
        .filter(outside)
        .map(|p| (p, 0.2))
        .chain(above.map(|p| (p, 0.01)))
    {
        assert!(
            light(&trail, x, y) > least,
            "the control at ({x}, {y}): {}",
            light(&trail, x, y)
        );
    }
    // The same point in the frame's pixels, from the lockup's records: the
    // box runs from the wordmark's first glyph to the block, its top the
    // block's.
    let (first, block) = (r.splash()[0], *r.splash().last().unwrap());
    let x = (i32::from(first.x.0) + i32::from(block.x.0) + i32::from(block.w.0)) / 2;
    let y = i32::from(block.y.0) + i32::from(block.h.0) / 5;
    let (w, _) = r.pixel_size();
    let i = usize::try_from(y * i32::try_from(w.value()).unwrap() + x).unwrap();
    assert_eq!(drawn[i], bare[i], "the page at the box's top centre");
}

/// Text on every row of the window, so a row the splash repaints in part
/// has glyphs on both sides of its records.
fn text_everywhere(t: &mut Term<'_>) {
    let line = "lorem ipsum dolor sit amet, consectetur adipiscing elit ".repeat(2);
    for r in 1..=WS.ws_row {
        let at = usize::from(r) % 7;
        let text = &line[at..at + usize::from(WS.ws_col)];
        feed(t, format!("\x1b[{r};1H{text}").as_bytes());
    }
}

/// The pixels of a `width`-wide frame each cell row `cell_h` high may
/// repaint for `records`: on each row, the columns from the leftmost pixel
/// a record meeting it reaches to the rightmost.
fn reach_of(records: &[Instance], width: usize, rows: usize, cell_h: usize) -> Vec<bool> {
    let mut out = vec![false; width * rows * cell_h];
    for row in 0..rows {
        let (top, bottom) = (row * cell_h, (row + 1) * cell_h);
        let meets = records.iter().filter(|r| {
            let y = i64::from(r.y.0);
            y < i64::try_from(bottom).unwrap() && y + i64::from(r.h.0) > i64::try_from(top).unwrap()
        });
        let (lo, hi) = meets.fold((i64::MAX, i64::MIN), |(lo, hi), r| {
            let x = i64::from(r.x.0);
            (lo.min(x), hi.max(x + i64::from(r.w.0)))
        });
        let clip = |v: i64| usize::try_from(v.max(0)).unwrap().min(width);
        if lo < hi {
            for y in top..bottom {
                out[y * width + clip(lo)..y * width + clip(hi)].fill(true);
            }
        }
    }
    out
}

/// **The splash repaints only the columns its records reach**: on every
/// frame of its life at 60 Hz, and on the frame it is gone, an incremental
/// frame writes no pixel outside, on each cell row, the columns from the
/// leftmost pixel one of its records (the plasma's and the logo's, this
/// frame's and the last's) reaches on the row to the rightmost -- every
/// other pixel is the last frame's, so a pixel there is left as it was,
/// even one no frame drew. Over text on every row. The control: the
/// repaint drew the plasma, a pixel inside changed on every frame it
/// burns.
#[test]
fn the_splash_repaints_only_the_columns_its_records_reach() {
    const POISON: u32 = 0x1234_5678;
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    text_everywhere(&mut t);
    let mut r = renderer(&faces, Theme::dark());
    let mut px = Vec::new();
    let _ = paint(&mut r, &mut t, &mut px, Damage::Full);
    let width = usize::try_from(r.pixel_size().0.value()).unwrap();
    let cell_h = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let rows = usize::from(WS.ws_row);
    let mut last: Vec<Instance> = Vec::new();
    let (mut outside, mut records, mut frames) = (0usize, 0usize, 0usize);
    let ages = (0u16..)
        .map(|f| f32::from(f) / 60.0)
        .take_while(|&a| a < GONE)
        .map(Some)
        .chain([None]);
    for age in ages {
        r.set_splash(age.and_then(|a| look(SplashPhase::Plasma(a))));
        let now: Vec<Instance> = r.trail().iter().chain(r.splash()).copied().collect();
        let both: Vec<Instance> = now.iter().chain(&last).copied().collect();
        let reach = reach_of(&both, width, rows, cell_h);
        for (p, &inside) in px.iter_mut().zip(&reach) {
            if !inside {
                *p = POISON;
            }
        }
        let before = px.clone();
        let _ = paint(&mut r, &mut t, &mut px, Damage::Incremental);
        outside += px
            .iter()
            .zip(&reach)
            .filter(|&(&p, &inside)| !inside && p != POISON)
            .count();
        let drawn = px
            .iter()
            .zip(&before)
            .zip(&reach)
            .any(|((a, b), &i)| i && a != b);
        assert!(
            drawn || age.is_none_or(|a| a >= LEFT),
            "the control at {age:?}"
        );
        records += now
            .iter()
            .map(|i| usize::from(i.w.0) * usize::from(i.h.0))
            .sum::<usize>();
        frames += 1;
        last = now;
    }
    assert_eq!(
        outside, 0,
        "{outside} pixels repainted outside the records' columns over {frames} frames, \
         whose records cover {records}"
    );
}

/// **Every frame of the splash drawn incrementally is the whole frame**,
/// pixel for pixel, over text on every row: under full motion every frame
/// of its life at 60 Hz, with a cursor trail lit under the plasma and held
/// until it lands, and the next one drawn once it is gone, over the text;
/// under reduced motion the held logo, its
/// fade and the frame it is gone -- on the dark, light and glass pages. A
/// frame drawn in part keeps every pixel no record moved over, and draws
/// the rest as the whole frame does. The control: the splash drew, and
/// differently from frame to frame.
#[test]
fn every_splash_frame_drawn_incrementally_is_the_whole_frame() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let glass = Theme {
        bg_alpha: baud_domain::Chan::from_u8(0xC0),
        ..Theme::dark()
    };
    for theme in [Theme::dark(), Theme::light(), glass] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        text_everywhere(&mut t);
        let (mut inc, mut full) = (renderer(&faces, theme.clone()), renderer(&faces, theme));
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let _ = paint(&mut inc, &mut t, &mut a, Damage::Full);
        let bare = a.clone();
        let plasma = (0u16..)
            .map(|f| f32::from(f) / 60.0)
            .take_while(|&a| a < GONE)
            .map(|a| {
                (
                    look(SplashPhase::Plasma(a)),
                    (a > 0.2).then(|| cursor_fire(a - 0.2)),
                )
            });
        // Gone, the trail lit under it held until it lands, then the next
        // one drawn.
        let gone = [Some(0.05f32), None, Some(0.0), Some(0.05), Some(0.1)]
            .map(|a| (None, a.map(cursor_fire)));
        let reduced = [0.0f32, 0.3, 0.6, 1.0]
            .map(|k| look(SplashPhase::Fade(k)))
            .into_iter()
            .chain([look(SplashPhase::Hold), None, None])
            .map(|s| (s, None));
        let mut frames = Vec::new();
        for (i, (splash, trail)) in plasma.chain(gone).chain(reduced).enumerate() {
            for r in [&mut inc, &mut full] {
                r.set_splash(splash);
                r.set_trail(trail);
            }
            let drawn = paint(&mut inc, &mut t, &mut a, Damage::Incremental);
            let whole = paint(&mut full, &mut t, &mut b, Damage::Full);
            let off = drawn.iter().zip(&whole).filter(|(x, y)| x != y).count();
            assert_eq!(
                off, 0,
                "frame {i} ({splash:?}): {off} pixels off the whole frame"
            );
            frames.push(drawn);
        }
        assert!(frames[0] != bare, "the control: the splash drew");
        assert!(frames[0] != frames[10], "the control: it moved");
        // The gone frames start 12 from the end: the next trail's second
        // frame against the frame with none.
        let gone = frames.len() - 12;
        assert!(
            frames[gone + 3] != frames[gone + 1],
            "the control: the next trail drew"
        );
    }
}

/// **A 2x splash drawn on threads is drawn as on one**: a 200 by 50 cell
/// window at 28 px (3400 by 1850), over text on every row, its renderer
/// at seven threads and at one, every frame incremental after a full one
/// -- the first frame, through each pass's hold, the sweep and the frame
/// it is gone -- pixel for pixel. The control: the first frame's band and
/// particle records cover at least two of the 65,536 pixels a thread is
/// started for, so the frame is cut into runs.
#[test]
fn a_2x_splash_on_threads_is_drawn_as_on_one() {
    const WIDE: WinSize = WinSize {
        ws_col: 200,
        ws_row: 50,
    };
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut owned = Owned::new(&WIDE);
    let mut t = Term::new(owned.storage(), &WIDE);
    let line = "lorem ipsum dolor sit amet, consectetur adipiscing elit ".repeat(5);
    for r in 1..=WIDE.ws_row {
        let at = usize::from(r) % 7;
        let text = &line[at..at + usize::from(WIDE.ws_col)];
        feed(&mut t, format!("\x1b[{r};1H{text}").as_bytes());
    }
    let wide = || {
        Renderer::new(
            Stack::with_roles(
                &faces,
                &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
            )
            .unwrap(),
            PxSize::of(28.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WIDE.ws_col)),
            Rows::of_index(usize::from(WIDE.ws_row)),
        )
        .unwrap()
    };
    let (mut one, mut seven) = (wide(), wide());
    one.set_threads(core::num::NonZeroUsize::new(1).unwrap());
    seven.set_threads(core::num::NonZeroUsize::new(7).unwrap());
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let _ = paint(&mut one, &mut t, &mut a, Damage::Full);
    let _ = paint(&mut seven, &mut t, &mut b, Damage::Full);
    let ages = [0.0, 1.0 / 60.0, 0.1, 0.2, 0.3, 0.4, LEFT]
        .map(Some)
        .into_iter()
        .chain([None]);
    for (i, age) in ages.enumerate() {
        for r in [&mut one, &mut seven] {
            r.set_splash(age.and_then(|a| look(SplashPhase::Plasma(a))));
        }
        if i == 0 {
            let lit: usize = seven
                .trail()
                .iter()
                .filter(|r| matches!(r.kind, Kind::Band | Kind::Particle))
                .map(|r| usize::from(r.w.0) * usize::from(r.h.0))
                .sum();
            assert!(lit >= 2 * 65_536, "the control: {lit} record pixels");
        }
        let x = paint(&mut one, &mut t, &mut a, Damage::Incremental);
        let y = paint(&mut seven, &mut t, &mut b, Damage::Incremental);
        let off = x.iter().zip(&y).filter(|(p, q)| p != q).count();
        assert_eq!(off, 0, "frame {i} ({age:?}): {off} pixels off one thread's");
    }
}

/// The cell at column `c` and row `r` of the splash tests' window.
fn trail_cell(c: f32, r: f32) -> Rect {
    Rect {
        left: c * 8.0,
        top: r * 17.0,
        right: c * 8.0 + 8.0,
        bottom: r * 17.0 + 17.0,
    }
}

/// The ignition of [`cursor_trail`]'s move.
fn trail_ignition() -> Ignition {
    let to = trail_cell(60.0, 9.0);
    Ignition::ignite(baud_render::Pt { x: 20.0, y: 25.5 }, to, Seed(5), 17.0)
}

/// A cursor trail of `effect` at `strength`, a move down and across the
/// window from row 1 to row 9.
fn cursor_trail(effect: Effect, strength: Strength, age: f32) -> TrailLook {
    let (from, to) = (trail_cell(2.0, 1.0), trail_cell(60.0, 9.0));
    let ig = trail_ignition();
    TrailLook::Vfx(Vfx {
        effect,
        emitter: Smear {
            corners: Segment::new(from.corners(), to).at(age),
            hole: to,
            shape: Shape::Quad,
            strength,
            opacity: Smear::OPAQUE,
        },
        ignition: ig.at(age),
        axis: ig.dir,
    })
}

/// **Every frame of every effect's trail drawn incrementally is the whole
/// frame**, pixel for pixel, over text on every row: each effect at full
/// strength and as a repeat pass (its bands alone, under the cap), from
/// its first frame at 60 Hz until it ends and on the frame it is gone. A
/// row its records leave keeps every pixel no record of this frame or the
/// last was over. The control: each trail drew, and moved.
#[test]
fn every_trail_frame_drawn_incrementally_is_the_whole_frame() {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    text_everywhere(&mut t);
    for effect in Effect::ALL {
        for strength in [Strength::Full, Strength::UnderThreshold] {
            let (mut inc, mut full) = (
                renderer(&faces, Theme::dark()),
                renderer(&faces, Theme::dark()),
            );
            let (mut a, mut b) = (Vec::new(), Vec::new());
            let bare = paint(&mut inc, &mut t, &mut a, Damage::Full);
            let end = effect.end(&trail_ignition());
            let ages = (0u16..)
                .map(|f| f32::from(f) / 60.0)
                .take_while(|&a| a <= end + 1.0 / 60.0)
                .map(Some)
                .chain([None]);
            let mut frames = Vec::new();
            for (i, age) in ages.enumerate() {
                let look = age.map(|a| cursor_trail(effect, strength, a));
                for r in [&mut inc, &mut full] {
                    r.set_trail(look);
                }
                let drawn = paint(&mut inc, &mut t, &mut a, Damage::Incremental);
                let whole = paint(&mut full, &mut t, &mut b, Damage::Full);
                let off = drawn.iter().zip(&whole).filter(|(x, y)| x != y).count();
                assert_eq!(
                    off, 0,
                    "{effect:?} {strength:?} frame {i}: {off} pixels off the whole frame"
                );
                frames.push(drawn);
            }
            assert!(
                frames[2] != bare,
                "the control: {effect:?} {strength:?} drew"
            );
            assert!(
                frames[2] != frames[8],
                "the control: {effect:?} {strength:?} moved"
            );
        }
    }
}

/// The ink box of an SVG path as `tools/baud-pictures` writes
/// one -- absolute `M`, `L`, `H`, `V`, `Q` and `Z`, fontTools' pen -- from
/// its points and each quadratic's extrema, not its control points.
fn path_ink(d: &str) -> [f64; 4] {
    let mut toks: Vec<String> = Vec::new();
    let mut num = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() {
            if !num.is_empty() {
                toks.push(core::mem::take(&mut num));
            }
            toks.push(c.to_string());
        } else if c == ' ' || c == ',' {
            if !num.is_empty() {
                toks.push(core::mem::take(&mut num));
            }
        } else {
            if c == '-' && !num.is_empty() {
                toks.push(core::mem::take(&mut num));
            }
            num.push(c);
        }
    }
    if !num.is_empty() {
        toks.push(num);
    }
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut take = |x: f64, y: f64| {
        b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    };
    let (mut i, mut cmd, mut at) = (0, 'M', (0.0f64, 0.0f64));
    let n = |i: &mut usize| -> f64 {
        *i += 1;
        toks[*i - 1].parse().expect("a number")
    };
    while i < toks.len() {
        if toks[i].chars().all(|c| c.is_ascii_alphabetic()) {
            cmd = toks[i].chars().next().unwrap();
            i += 1;
            if cmd == 'Z' {
                continue;
            }
        }
        match cmd {
            'M' | 'L' => at = (n(&mut i), n(&mut i)),
            'H' => at.0 = n(&mut i),
            'V' => at.1 = n(&mut i),
            'Q' => {
                let (c, p) = ((n(&mut i), n(&mut i)), (n(&mut i), n(&mut i)));
                for (a, k, z, axis) in [(at.0, c.0, p.0, 0), (at.1, c.1, p.1, 1)] {
                    let den = a - 2.0 * k + z;
                    if den != 0.0 {
                        let t = (a - k) / den;
                        if t > 0.0 && t < 1.0 {
                            let v = (1.0 - t) * (1.0 - t) * a + 2.0 * (1.0 - t) * t * k + t * t * z;
                            match axis {
                                0 => take(v, at.1),
                                _ => take(at.0, v),
                            }
                        }
                    }
                }
                at = p;
            }
            other => panic!("the path has a {other}, which the pen does not write"),
        }
        take(at.0, at.1);
    }
    b
}

/// **The held splash is `assets/logo/baud-lockup-dark.svg`'s lockup**,
/// drawn by the CPU renderer and read back to within one pixel at the
/// splash's scale, from the file itself: the block is its `<rect>`, the
/// ❯'s ink is its amber path's, and the word's ink stands where the word
/// path's does -- its top, its foot and its centre. The word is drawn in
/// the renderer's badge face, JetBrains Mono Bold, and the file traces it
/// at 500, Medium: the two share their ink's centre and height and the
/// Bold is 0.68 units wider each side, so the word's edges are not held.
/// Each box is the pixels at least half way from the page to the glyph's
/// colour, and the splash's glyph pens are whole pixels, so one pixel
/// covers both.
#[test]
fn the_held_splash_is_the_lockup_files() {
    let svg = include_str!("../../assets/logo/baud-lockup-dark.svg");
    let attr = |tag: &str, name: &str| -> f64 {
        let tag = format!(" {tag}");
        let at = tag.find(&format!(" {name}=\"")).expect(name) + name.len() + 3;
        tag[at..at + tag[at..].find('"').unwrap()].parse().unwrap()
    };
    let block_tag = svg
        .split("<rect ")
        .skip(1)
        .map(|t| &t[..t.find("/>").unwrap()])
        .find(|t| t.contains("fill=\"#F4A32C\""))
        .expect("the block's rect");
    let (bx, by) = (attr(block_tag, "x"), attr(block_tag, "y"));
    let block = [
        bx,
        by,
        bx + attr(block_tag, "width"),
        by + attr(block_tag, "height"),
    ];
    let paths: Vec<(&str, &str)> = svg
        .split("<path d=\"")
        .skip(1)
        .map(|p| {
            let end = p.find('"').unwrap();
            (&p[..end], &p[end..])
        })
        .collect();
    let word = path_ink(
        paths
            .iter()
            .find(|p| p.1.contains("#F3F4F6"))
            .expect("the word")
            .0,
    );
    let prompt = path_ink(
        paths
            .iter()
            .find(|p| p.1.contains("#F4A32C"))
            .expect("the ❯")
            .0,
    );

    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    // A window 1600 pixels wide, so the lockup is 1.6 pixels a unit.
    let ws = WinSize {
        ws_col: 200,
        ws_row: 60,
    };
    let mut theme = Theme::dark();
    theme.bg = baud_render::Argb(0xFF15_171B);
    let mut r = Renderer::new(
        Stack::with_roles(
            &faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, b"\x1b[?25l");
    r.set_splash(look(SplashPhase::Hold));
    let mut px = Vec::new();
    let f = paint(&mut r, &mut t, &mut px, Damage::Full);
    let (w, h) = r.pixel_size();
    let (wu, hu) = (
        usize::try_from(w.value()).unwrap(),
        usize::try_from(h.value()).unwrap(),
    );
    let frame = (wu as f32, hu as f32);
    let s = baud_render::splash::scale(frame);
    let o = baud_render::splash::origin(frame, s);
    assert!(
        (s - 1.6).abs() < 1e-6,
        "the lockup at 1.6 pixels a unit: {s}"
    );
    let (s, ox, oy) = (f64::from(s), f64::from(o.x), f64::from(o.y));
    let ground = [0x15, 0x17, 0x1B];
    let chans = |p: u32| {
        let [_, r, g, b] = p.to_be_bytes();
        [r, g, b]
    };
    // How far a pixel is from the page to `colour`, 0 to 1.
    let share = |p: u32, colour: [u8; 3]| -> f64 {
        let (c, mut dot, mut n2) = (chans(p), 0.0, 0.0);
        for i in 0..3 {
            let d = f64::from(colour[i]) - f64::from(ground[i]);
            dot += (f64::from(c[i]) - f64::from(ground[i])) * d;
            n2 += d * d;
        }
        dot / n2
    };
    // The pixel box, in lockup units, of the pixels at least half way to
    // `colour` between the columns `x0` and `x1` (lockup units).
    let drawn = |x0: f64, x1: f64, colour: [u8; 3]| -> [f64; 4] {
        let mut b = [usize::MAX, usize::MAX, 0, 0];
        let within = |x: usize| (ox + x0 * s..ox + x1 * s).contains(&(x as f64));
        for y in 0..hu {
            for x in (0..wu).filter(|&x| within(x)) {
                if share(f[y * wu + x], colour) >= 0.5 {
                    b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
                }
            }
        }
        assert!(b[0] < b[2], "something drawn in {x0}..{x1}");
        [
            (b[0] as f64 - ox) / s,
            (b[1] as f64 - oy) / s,
            (b[2] as f64 - ox) / s,
            (b[3] as f64 - oy) / s,
        ]
    };
    let (amber, ink) = ([0xF4, 0xA3, 0x2C], [0xF3, 0xF4, 0xF6]);
    let mid = 0.5 * (prompt[2] + block[0]);
    let near = |what: &str, have: [f64; 4], want: [f64; 4], edges: [bool; 4]| {
        for (i, side) in ["left", "top", "right", "bottom"].iter().enumerate() {
            let off = (have[i] - want[i]) * s;
            assert!(
                !edges[i] || off.abs() <= 1.0,
                "{what}'s {side}: drawn at {:.2}, the file's at {:.2}, {off:.2} px apart",
                have[i],
                want[i]
            );
        }
    };
    near("the block", drawn(mid, 300.0, amber), block, [true; 4]);
    let p = drawn(prompt[0] - 4.0, mid, amber);
    near("the ❯", p, prompt, [true; 4]);
    let wd = drawn(0.0, prompt[0] - 4.0, ink);
    near("the word", wd, word, [false, true, false, true]);
    let centre = |b: [f64; 4]| 0.5 * (b[0] + b[2]);
    let off = (centre(wd) - centre(word)) * s;
    assert!(
        off.abs() <= 1.0,
        "the word's centre is {off:.2} px from the file's"
    );
}

/// The dark half's held lockup and one pass of plasma over its word, for
/// the layer tests: the renderer, the term, the frame buffer, the held
/// frame, the pass and the frame-pixel box round the word.
struct Layered<'f> {
    r: Renderer<'f, 'f>,
    owned: Owned,
    px: Vec<u32>,
    held: Vec<u32>,
    vfx: Vfx,
    word: [Px; 4],
}

fn layered<'f>(faces: &'f [Face<'f>]) -> Layered<'f> {
    use baud_render::splash::{EMITTER, SEAM, SEED, origin, pass_over, placed, scale};
    let owned = Owned::new(&WS);
    let mut r = renderer(faces, Theme::dark());
    r.set_splash(look(SplashPhase::Hold));
    let (w, h) = r.pixel_size();
    let frame = (w.value() as f32, h.value() as f32);
    let (s, o) = (scale(frame), origin(frame, scale(frame)));
    let word = Rect {
        right: SEAM.0,
        ..EMITTER
    };
    let b = placed(word, o, s);
    let at = |v: f32| Px::of(u32::try_from(v.round() as i64).unwrap());
    let word_px = [
        at(b.left),
        at(b.top),
        at(b.right - b.left),
        at(b.bottom - b.top),
    ];
    Layered {
        r,
        owned,
        px: Vec::new(),
        held: Vec::new(),
        vfx: pass_over(word, o, s, SEED, 0.05),
        word: word_px,
    }
}

/// Every index of a `width`-wide frame inside the box `[left, top, w, h]`.
fn inside(width: Px, b: [Px; 4]) -> impl Fn(usize) -> bool {
    let v = |p: Px| usize::try_from(p.value()).unwrap();
    let (wd, x0, y0, x1, y1) = (
        v(width),
        v(b[0]),
        v(b[1]),
        v(b[0]) + v(b[2]),
        v(b[1]) + v(b[3]),
    );
    move |i| (x0..x1).contains(&(i % wd)) && (y0..y1).contains(&(i / wd))
}

/// **The splash's layers draw behind its glyphs or in front of them, on
/// the CPU path alone**: one pass of plasma over the word, a layer behind
/// the held lockup, leaves every pixel the word covers whole and lights the
/// ground round it; in front, it lights the word as well. No record a GPU
/// path draws moves (`trail`, `splash`). Taken away, the next frame, drawn
/// incrementally, is the held lockup's again; at no opacity a layer draws
/// nothing, and without the splash none is drawn.
#[test]
fn the_splash_layers_draw_behind_or_in_front() {
    use baud_render::splash::{Depth, Layer};
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut l = layered(&faces);
    let mut t = Term::new(l.owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    l.r.set_splash(None);
    let bare = paint(&mut l.r, &mut t, &mut l.px, Damage::Full);
    l.r.set_splash(look(SplashPhase::Hold));
    let held = paint(&mut l.r, &mut t, &mut l.px, Damage::Full);
    let records = (l.r.trail().to_vec(), l.r.splash().to_vec());
    let vfx = l.vfx;
    let ink = baud_render::Argb(0xFFF3_F4F6).0;
    let mut drawn = |r: &mut Renderer<'_, '_>, layers: &[Layer]| {
        r.set_splash_layers(layers);
        let f = paint(r, &mut t, &mut l.px, Damage::Incremental);
        assert_eq!(
            (r.trail().to_vec(), r.splash().to_vec()),
            records,
            "no record moved"
        );
        f
    };
    let behind = drawn(
        &mut l.r,
        &[Layer {
            vfx,
            depth: Depth::Behind,
        }],
    );
    let front = drawn(
        &mut l.r,
        &[Layer {
            vfx,
            depth: Depth::Front,
        }],
    );
    let inked: Vec<usize> = (0..held.len()).filter(|&i| held[i] == ink).collect();
    assert!(
        inked.len() > 200,
        "the premise: the word is drawn, {}",
        inked.len()
    );
    let lit = |f: &[u32]| f.iter().zip(&held).filter(|(a, b)| a != b).count();
    assert!(lit(&behind) > 500, "behind: lit round the word");
    assert!(
        inked.iter().all(|&i| behind[i] == ink),
        "behind: the word whole"
    );
    let over = inked.iter().filter(|&&i| front[i] != ink).count();
    assert!(over > 100, "in front: the word lit, {over} pixels");
    assert_eq!(drawn(&mut l.r, &[]), held, "taken away, the held lockup's");
    let none = Layer {
        vfx: Vfx {
            emitter: Smear {
                opacity: baud_domain::Chan::from_u8(0),
                ..vfx.emitter
            },
            ..vfx
        },
        depth: Depth::Front,
    };
    assert_eq!(drawn(&mut l.r, &[none]), held, "at no opacity, nothing");
    l.r.set_splash(None);
    l.r.set_splash_layers(&[Layer {
        vfx,
        depth: Depth::Front,
    }]);
    assert_eq!(
        paint(&mut l.r, &mut t, &mut l.px, Damage::Incremental),
        bare,
        "no splash, no layer"
    );
}

/// **The slabs cut the keyline, the knot and light the rim, inside their
/// box alone** (`splash::Slabs`), over the word's box:
///
/// * a behind cut of one takes back every pixel of light a behind layer
///   drew in the box, and none outside it;
/// * the knot cut takes the glyph pixels under a front layer's core
///   toward the page before the layer is drawn, so each is no brighter
///   than without it and some are darker, the box alone;
/// * the rim takes its band toward its ink by the layers' light, and at no
///   gain draws nothing;
/// * slabs whose fields do not hold a weight for each pixel are none, and
///   with no layer the slabs and the ink change no pixel.
#[test]
fn the_slabs_cut_the_keyline_and_the_knot_and_light_the_rim() {
    use baud_render::splash::{Depth, Knot, Layer, Rim, Slabs};
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut l = layered(&faces);
    let mut t = Term::new(l.owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    l.held = paint(&mut l.r, &mut t, &mut l.px, Damage::Full);
    let (w, _) = l.r.pixel_size();
    let in_box = inside(w, l.word);
    let n = usize::try_from(l.word[2].value() * l.word[3].value()).unwrap();
    let page = baud_render::Argb(Theme::dark().bg.0 | 0xFF00_0000);
    let slabs = |cut: f32, knot: f32, rim: f32| Slabs {
        left: l.word[0],
        top: l.word[1],
        width: l.word[2],
        height: l.word[3],
        page,
        behind_cut: vec![cut; n],
        glyph: vec![1.0; n],
        knot: Knot {
            core: [0.05, 0.1],
            grow: 1.0,
            strength: knot,
        },
        rim: Rim {
            band: vec![1.0; n],
            gain: rim,
            ink: baud_render::Argb(0xFFFF_0000),
        },
    };
    let vfx = l.vfx;
    let mut frame = |r: &mut Renderer<'_, '_>, depth: Option<Depth>, s: Option<Slabs>| {
        r.set_splash_slabs(s);
        let layers: Vec<Layer> = depth
            .map(|depth| Layer { vfx, depth })
            .into_iter()
            .collect();
        r.set_splash_layers(&layers);
        paint(r, &mut t, &mut l.px, Damage::Incremental)
    };
    let held = l.held.clone();
    let luma = |p: u32| {
        let [_, r, g, b] = p.to_be_bytes();
        u32::from(r) + u32::from(g) + u32::from(b)
    };
    // The behind cut.
    let plain = frame(&mut l.r, Some(Depth::Behind), None);
    let cut = frame(&mut l.r, Some(Depth::Behind), Some(slabs(1.0, 0.0, 0.0)));
    let lit_in = (0..held.len())
        .filter(|&i| in_box(i) && plain[i] != held[i])
        .count();
    assert!(
        lit_in > 200,
        "the premise: the layer lights the box, {lit_in}"
    );
    for i in 0..held.len() {
        match in_box(i) {
            true => assert_eq!(cut[i], held[i], "behind, cut: pixel {i} as held"),
            false => assert_eq!(cut[i], plain[i], "behind, outside: pixel {i} lit"),
        }
    }
    // The knot cut.
    let plain = frame(&mut l.r, Some(Depth::Front), None);
    let knot = frame(&mut l.r, Some(Depth::Front), Some(slabs(0.0, 1.0, 0.0)));
    let mut darker = 0;
    for i in 0..held.len() {
        match in_box(i) {
            true => {
                assert!(luma(knot[i]) <= luma(plain[i]), "knot: pixel {i} brighter");
                darker += usize::from(luma(knot[i]) < luma(plain[i]));
            }
            false => assert_eq!(knot[i], plain[i], "knot, outside: pixel {i}"),
        }
    }
    assert!(
        darker > 100,
        "the knot cuts the word under the strand, {darker}"
    );
    // The rim.
    let rim = frame(&mut l.r, Some(Depth::Front), Some(slabs(0.0, 0.0, 0.5)));
    let red = |p: u32| p.to_be_bytes()[1];
    let mut toward = 0;
    for i in 0..held.len() {
        match in_box(i) {
            true => {
                assert!(red(rim[i]) >= red(plain[i]).min(red(0xFFFF_0000)));
                toward += usize::from(rim[i] != plain[i]);
            }
            false => assert_eq!(rim[i], plain[i], "rim, outside: pixel {i}"),
        }
    }
    assert!(toward > 100, "the rim lights the band, {toward}");
    let none = frame(&mut l.r, Some(Depth::Front), Some(slabs(0.0, 0.0, 0.0)));
    assert_eq!(none, plain, "no knot, no rim: the layer alone");
    // Slabs that do not fit, and no layer.
    let mut short = slabs(1.0, 1.0, 0.5);
    short.glyph.pop();
    assert_eq!(
        frame(&mut l.r, Some(Depth::Front), Some(short)),
        plain,
        "slabs that do not fit are none"
    );
    l.r.set_splash_ink(Some(baud_render::Argb(0xFF00_FF00)));
    assert_eq!(
        frame(&mut l.r, None, Some(slabs(1.0, 1.0, 0.5))),
        held,
        "no layer: the slabs and the ink change nothing"
    );
}
