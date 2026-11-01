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
        ("light", Theme::light(), LIGHT),
        ("light glass", light_glass, LIGHT),
    ] {
        let mut r = renderer(&faces, theme);
        let (w, h) = r.pixel_size();
        let side = |v: u32| f32::from(u16::try_from(v).unwrap());
        let frame = (side(w.value()), side(h.value()));
        let s = scale(frame);
        let o = origin(frame, s);
        let word = o.x + (WORDMARK.x + 4.0 * (WORDMARK.size * 0.6 + WORDMARK.tracking)) * s;
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
                let at = match i.kind {
                    Kind::Band => (l, rt),
                    _ => ((l + rt) * 0.5, (l + rt) * 0.5),
                };
                let colour = i.color.with_alpha(c.amber.a());
                if at.1 <= word {
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
