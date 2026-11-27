//! The trails' flash bound in the renderer (note AL, option O2): a repeat
//! pass's records, the cap they carry and the ground it is read from, and
//! the CPU path's quad. The thresholds are WCAG 2.2's (`[[source: wcag22]]`):
//! a general transition is a change in relative luminance of 0.10 or more
//! where the darker state is below 0.80.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ImageBudget, ImagePx, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{
    Argb, Blend, Damage, Held, Kind, Mix, Pt, PxSize, Renderer, Segment, Shape, Smear, Stack,
    Strength, Target, Theme, TrailLook, under_threshold, under_threshold_by, under_threshold_over,
    under_threshold_over_by,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 10,
};

fn px(p: Px) -> usize {
    usize::try_from(p.value()).unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The colour `theme`'s trails are drawn in: its cursor's, baud's amber on
/// the built-in themes, else its text.
fn ink_of(theme: &Theme) -> Argb {
    theme
        .cursor
        .and_then(baud_domain::themefile::CellPaint::colour)
        .unwrap_or(theme.fg)
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// A frame on the CPU path, which reads the styles and so sets the cap.
fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Vec<u32> {
    let (w, h) = r.pixel_size();
    let mut pixels = vec![0u32; px(w) * px(h)];
    let mut target = Target::new(&mut pixels, w, w, h).unwrap();
    let seal = t.seal_writes();
    r.render(t, seal, &mut target, Damage::Full);
    pixels
}

/// A quad `t` seconds into a move from `from` to the block cursor at `to`.
fn smear(r: &Renderer<'_, '_>, from: (usize, usize), to: (usize, usize), t: f32) -> Smear {
    let style = baud_core::CursorStyle::SteadyBlock;
    let rect =
        |(row, col): (usize, usize)| r.cursor_rect(Row::from_index(row), Col::at_index(col), style);
    let hole = rect(to);
    Smear {
        corners: Segment::new(rect(from).corners(), hole).at(t),
        hole,
        shape: Shape::Quad,
        strength: Strength::Full,
        opacity: Smear::OPAQUE,
    }
}

/// An effect riding that quad, `t` seconds after it lit.
fn vfx(r: &Renderer<'_, '_>, effect: Effect, t: f32) -> Vfx {
    let emitter = smear(r, (1, 2), (7, 20), t);
    let from = emitter.corners.0;
    let tail = Pt {
        x: (from[2].x + from[0].x) * 0.5,
        y: (from[0].y + from[1].y) * 0.5,
    };
    let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
    Vfx {
        effect,
        emitter,
        ignition: Ignition::ignite(tail, emitter.hole, Seed(5), unit).at(t),
        axis: Pt { x: 1.0, y: 0.0 },
    }
}

fn repeat(look: TrailLook) -> TrailLook {
    match look {
        TrailLook::Vfx(v) => TrailLook::Vfx(Vfx {
            emitter: Smear {
                strength: Strength::UnderThreshold,
                ..v.emitter
            },
            ..v
        }),
        TrailLook::Smear(s) => TrailLook::Smear(Smear {
            strength: Strength::UnderThreshold,
            ..s
        }),
        TrailLook::Ghost(_) => unreachable!("no ghost here"),
    }
}

/// Whether two records' rectangles share a pixel.
fn overlap(a: &baud_render::Instance, b: &baud_render::Instance) -> bool {
    let span = |p: i16, n: u16| (i32::from(p), i32::from(p) + i32::from(n));
    let (ax, ay, bx, by) = (
        span(a.x.0, a.w.0),
        span(a.y.0, a.h.0),
        span(b.x.0, b.w.0),
        span(b.y.0, b.h.0),
    );
    ax.0 < bx.1 && bx.0 < ax.1 && ay.0 < by.1 && by.0 < ay.1
}

fn opaque(r: u8, g: u8, b: u8) -> Argb {
    Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

/// **A repeat pass draws one record a pixel, none over the cap**, for
/// every effect at three ages: its bands, which are disjoint, and not its
/// particles, which stack over them and over each other, so that under
/// source-over their alphas would compound past any one record's cap. The
/// cap is the cursor's colour's over the page under it, the only ground on
/// a blank screen. The controls: the same look at full strength has records
/// that overlap and alphas over the cap.
#[test]
fn a_repeat_pass_draws_one_record_a_pixel_under_the_cap() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    let mut r = renderer(&faces);
    let _ = paint(&mut r, &mut t);
    let (ink, page) = (ink_of(&Theme::dark()), Theme::dark().bg);
    let k = under_threshold(
        ink.with_alpha(Chan::from_u8(0xFF)),
        page.with_alpha(Chan::from_u8(0xFF)),
    );
    assert!(
        k.get() < 0xFF,
        "the control: this ink over this page has a cap"
    );
    let mut stacked = 0;
    for effect in Effect::ALL {
        for age in [0.02f32, 0.1, 0.3] {
            let full = TrailLook::Vfx(vfx(&r, effect, age));
            r.set_trail(Some(full));
            let _ = paint(&mut r, &mut t);
            let records = r.trail().to_vec();
            assert!(
                records.iter().any(|i| i.color.a().get() > k.get()),
                "{effect:?} at {age}: the control, a full pass over the cap"
            );
            if records.iter().any(|i| i.kind == Kind::Particle) {
                stacked += 1;
            }
            r.set_trail(Some(repeat(full)));
            let _ = paint(&mut r, &mut t);
            // The plasma's cap is scanned over its `screen`, a step under
            // source-over's on this page.
            let k = under_threshold_by(
                ink.with_alpha(Chan::from_u8(0xFF)),
                page.with_alpha(Chan::from_u8(0xFF)),
                r.trail_mix(),
            );
            assert_eq!(r.flash_cap(), k, "{effect:?} at {age}: the page's cap");
            let records = r.trail();
            assert!(!records.is_empty(), "{effect:?} at {age}: a repeat draws");
            for (n, a) in records.iter().enumerate() {
                assert!(
                    a.color.a().get() <= k.get(),
                    "{effect:?} at {age}: a record at {} over the cap {}",
                    a.color.a().get(),
                    k.get()
                );
                for b in &records[n + 1..] {
                    assert!(
                        !overlap(a, b),
                        "{effect:?} at {age}: two records share a pixel, {:?} and {:?}",
                        a.kind,
                        b.kind
                    );
                }
            }
        }
    }
    assert!(stacked > 0, "the control: full passes draw particles");
}

/// **The cap is read from the cells under the pass**: a quad over cells
/// whose background is `#CD0000` takes the black cursor's cap over that red
/// (124 of 255), where the same pass over the page takes the page's (255:
/// black over `#101010` never flashes); over `X`s in the default colour it
/// takes the least cap over every mixture a glyph edge draws of that text
/// over the page, which is under both ends'.
#[test]
fn a_repeat_pass_is_capped_against_the_cells_under_it() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let black = opaque(0, 0, 0);
    let page = Theme::dark().bg.with_alpha(Chan::from_u8(0xFF));
    let text = Theme::dark().fg.with_alpha(Chan::from_u8(0xFF));
    let mut caps = Vec::new();
    for under in ["", "\x1b[48;2;205;0;0m", "X"] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l\x1b]12;rgb:00/00/00\x07");
        if under == "X" {
            for row in 1..9 {
                feed(
                    &mut t,
                    format!("\x1b[{};1H{}", row, "X".repeat(30)).as_bytes(),
                );
            }
        } else if !under.is_empty() {
            for row in 1..9 {
                feed(
                    &mut t,
                    format!("\x1b[{row};1H{under}{}\x1b[0m", " ".repeat(30)).as_bytes(),
                );
            }
        }
        let mut r = renderer(&faces);
        let _ = paint(&mut r, &mut t);
        r.set_trail(Some(repeat(TrailLook::Smear(smear(
            &r,
            (1, 2),
            (6, 20),
            0.05,
        )))));
        let _ = paint(&mut r, &mut t);
        caps.push(r.flash_cap());
    }
    assert_eq!(caps[0], under_threshold(black, page), "over the page");
    assert_eq!(
        caps[0].get(),
        0xFF,
        "the control: black never flashes on #101010"
    );
    assert_eq!(
        caps[1],
        under_threshold(black, opaque(0xCD, 0, 0)),
        "over red cells"
    );
    assert_eq!(caps[1].get(), 124, "note AL's cap for black on red");
    let mixed = under_threshold_over(black, text, page);
    assert_eq!(caps[2], mixed, "over text");
    assert!(
        mixed.get() <= under_threshold(black, text).get().min(caps[0].get()),
        "a mixture's cap is under both ends'"
    );
}

/// WCAG's relative luminance of a word's three bytes.
fn lum(p: u32) -> f64 {
    let [_, r, g, b] = p.to_be_bytes();
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// **The CPU path's quad carries the cap**: where the app has no GPU, or
/// `BAUD_GPU=0`, every trail is the quad, painted by `Target::quad` from the
/// look rather than from its record, so the record's cap alone would bound
/// nothing there. No pixel of a repeat quad over the page is a general
/// transition from the frame without it; the control, the full quad, makes
/// them.
#[test]
fn a_repeat_quad_on_the_cpu_path_makes_no_transition() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    let mut r = renderer(&faces);
    let bare = paint(&mut r, &mut t);
    let transitions = |frame: &[u32]| {
        frame
            .iter()
            .zip(&bare)
            .filter(|&(&p, &q)| {
                let (a, b) = (lum(p), lum(q));
                (a - b).abs() >= 0.10 && a.min(b) < 0.80
            })
            .count()
    };
    let quad = TrailLook::Smear(smear(&r, (1, 2), (7, 20), 0.05));
    r.set_trail(Some(quad));
    let full = paint(&mut r, &mut t);
    assert!(transitions(&full) > 0, "the control: the full quad flashes");
    r.set_trail(Some(repeat(quad)));
    let repeated = paint(&mut r, &mut t);
    assert!(
        repeated.iter().zip(&bare).any(|(p, q)| p != q),
        "the control: the repeat quad draws"
    );
    assert_eq!(transitions(&repeated), 0, "a repeat quad's pixel flashes");
}

/// **Every effect on the CPU path carries the cap**, as the quad does: the
/// CPU path paints an effect's records by its light, each at its record's
/// alpha, so a repeat pass's pixels are the capped records'. No pixel of
/// a repeat pass of any effect over the page is a general transition from
/// the frame without it; the controls: every effect's full pass makes
/// them, 45 to 224 pixels, and its repeat pass still draws.
/// Perturbation: paint the records at full alpha -- the repeat passes
/// flash.
#[test]
fn a_repeat_effect_on_the_cpu_path_makes_no_transition() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    let mut r = renderer(&faces);
    let bare = paint(&mut r, &mut t);
    let transitions = |frame: &[u32]| {
        frame
            .iter()
            .zip(&bare)
            .filter(|&(&p, &q)| {
                let (a, b) = (lum(p), lum(q));
                (a - b).abs() >= 0.10 && a.min(b) < 0.80
            })
            .count()
    };
    for effect in Effect::ALL {
        let full = TrailLook::Vfx(vfx(&r, effect, 0.05));
        r.set_trail(Some(full));
        assert!(
            transitions(&paint(&mut r, &mut t)) > 0,
            "{effect:?}: the control: the full pass flashes"
        );
        r.set_trail(Some(repeat(full)));
        // The cap is scanned over the frames that follow the first.
        let _ = paint(&mut r, &mut t);
        let repeated = paint(&mut r, &mut t);
        assert!(
            repeated.iter().zip(&bare).any(|(p, q)| p != q),
            "{effect:?}: the control: the repeat pass draws"
        );
        assert_eq!(
            transitions(&repeated),
            0,
            "{effect:?}: a repeat pass's pixel flashes"
        );
    }
}

/// `src`, premultiplied and in channels 0 to 1, over the pixel `dst` by
/// `mix`, rounded to bytes as the GPU's blend unit stores them, W3C
/// Compositing's section 6 in premultiplied numbers as the CPU path draws
/// an effect by over an opaque page: `cs + cb (1 - as)` source-over, `cs +
/// cb - cs cb` by `screen`, `cs (1 - ab) + cb (1 - as) + cs cb` by
/// `multiply`.
fn mixed_premultiplied(src: [f32; 4], dst: u32, mix: Mix) -> u32 {
    let ch = |k: u32| f32::from(u8::try_from((dst >> k) & 0xff).unwrap()) / 255.0;
    let (a, ab) = (src[3], ch(24));
    let byte = |v: f32| u32::try_from((v * 255.0).round().clamp(0.0, 255.0) as i64).unwrap();
    let colour = |s: f32, d: f32| {
        byte(match mix {
            Mix::Normal => s + d * (1.0 - a),
            Mix::Screen => s + d - s * d,
            Mix::Multiply => s * (1.0 - ab) + d * (1.0 - a) + s * d,
        })
    };
    (byte(a + ab * (1.0 - a)) << 24)
        | (colour(src[0], ch(16)) << 16)
        | (colour(src[1], ch(8)) << 8)
        | colour(src[2], ch(0))
}

/// **The plasma's repeat pass is clamped at its cap, not scaled by it**,
/// on the CPU path, at three ages over the page, on the dark theme and the
/// light: every pixel is the cursor's colour at `min(light, cap)` over the
/// frame without the pass, by the frame's mix (`Renderer::trail_mix`), the
/// cap the one each band record carries. So no pixel is past the cap's own
/// composite over the page, and a pixel whose
/// light is under the cap is drawn at its whole light, where the scale drew
/// it at the light times the cap. The cap is the one the renderer scans
/// over the plasma's mix, 94 over the dark page by `screen` and 14 over the
/// light by `multiply`. The controls: at every age some hundreds of pixels'
/// light is under the cap, and over the three ages more than 50 pixels' is
/// past it; and the fire's repeat,
/// which was not censused clamped, is still scaled. Perturbations: the
/// clamp scaled, or its `min` dropped.
#[test]
fn a_repeat_plasma_is_clamped_at_its_cap_not_scaled_by_it() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    for theme in [Theme::dark(), Theme::light()] {
        let (ink, page) = (ink_of(&theme), theme.bg);
        let mut r = renderer(&faces);
        r.set_theme(theme);
        r.set_trail(None);
        let _ = paint(&mut r, &mut t);
        let bare = paint(&mut r, &mut t);
        let width = px(r.pixel_size().0);
        let channel = |p: u32, s: u32| i32::try_from((p >> s) & 0xff).unwrap();
        let mut past = 0usize;
        for age in [0.04f32, 0.06, 0.1] {
            let at = format!("{page:?} at {age}");
            let full = vfx(&r, Effect::Plasma, age);
            let TrailLook::Vfx(v) = repeat(TrailLook::Vfx(full)) else {
                unreachable!()
            };
            r.set_trail(Some(TrailLook::Vfx(v)));
            let _ = paint(&mut r, &mut t);
            let drawn = paint(&mut r, &mut t);
            // Scanned over the plasma's mix, as the renderer scans it.
            let cap = under_threshold_by(
                ink.with_alpha(Chan::from_u8(0xFF)),
                page.with_alpha(Chan::from_u8(0xFF)),
                r.trail_mix(),
            );
            let k = f32::from(cap.get()) / 255.0;
            assert_eq!(r.flash_cap(), cap, "{at}: the page's cap");
            let records = r.trail().to_vec();
            assert!(
                !records.is_empty()
                    && records
                        .iter()
                        .all(|i| i.kind == Kind::Band && i.color.a() == cap),
                "{at}: bands alone, each at the cap"
            );
            let mix = r.trail_mix();
            let held = mixed_premultiplied(
                [
                    f32::from(ink.r().get()) / 255.0 * k,
                    f32::from(ink.g().get()) / 255.0 * k,
                    f32::from(ink.b().get()) / 255.0 * k,
                    k,
                ],
                page.with_alpha(Chan::from_u8(0xFF)).0,
                mix,
            );
            let mut want = bare.clone();
            let (mut whole, mut clamped) = (0usize, 0usize);
            for rec in &records {
                let c = rec.color.0;
                let unit = |v: u32| f32::from(u8::try_from(v & 0xff).unwrap()) / 255.0;
                let (cr, cg, cb) = (unit(c >> 16), unit(c >> 8), unit(c));
                let (x0, y0) = (i32::from(rec.x.0), i32::from(rec.y.0));
                for y in y0.max(0)..y0 + i32::from(rec.h.0) {
                    for x in x0.max(0)..(x0 + i32::from(rec.w.0)).min(i32::try_from(width).unwrap())
                    {
                        let light = v.light(Pt {
                            x: x as f32 + 0.5,
                            y: y as f32 + 0.5,
                        });
                        if light <= 0.0 {
                            continue;
                        }
                        whole += usize::from(light > 0.02 && light < k);
                        clamped += usize::from(light > k);
                        let a = light.min(k);
                        let i = usize::try_from(y).unwrap() * width + usize::try_from(x).unwrap();
                        want[i] = mixed_premultiplied([cr * a, cg * a, cb * a, a], want[i], mix);
                    }
                }
            }
            assert!(
                whole > 200,
                "{at}: the control, {whole} pixels lit under the cap"
            );
            past += clamped;
            let off = drawn.iter().zip(&want).filter(|(a, b)| a != b).count();
            assert_eq!(off, 0, "{at}: {off} pixels are not min(light, cap)");
            for (i, (&p, &q)) in drawn.iter().zip(&bare).enumerate() {
                for s in [0, 8, 16] {
                    let (rise, most) = (
                        (channel(p, s) - channel(q, s)).abs(),
                        (channel(held, s) - channel(q, s)).abs(),
                    );
                    assert!(
                        rise <= most,
                        "{at}: pixel {i} moved {rise} past the cap's {most}"
                    );
                }
            }
        }
        assert!(
            past > 50,
            "{page:?}: the control, {past} pixels lit past the cap"
        );
        let fire = vfx(&r, Effect::Fire, 0.06);
        let TrailLook::Vfx(f) = repeat(TrailLook::Vfx(fire)) else {
            unreachable!()
        };
        assert_eq!(
            f.cap(),
            baud_render::effect::Cap::Scale,
            "the control: the fire's repeat scales"
        );
    }
}

/// **The plasma's repeat in front of the text makes no transition**: over
/// a screen of `X`s in the default colour, the cursor's colour, at three
/// ages, on the dark theme and the light, no pixel of the plasma's repeat
/// pass is a general transition from the frame without it. The cap is
/// scanned over the composite the plasma is drawn by, `screen` on the dark
/// theme and `multiply` on the light: source-over leaves a letter in the
/// cursor's own colour as it was, and its cap let the screened letters and
/// their edges flash: in the census, 11,245,366 pixel-frames of repeats
/// over the dark theme's default text, a luminance change of up to 0.118. The fire's repeat over the same cells draws first, so a
/// cap cached under source-over cannot stand in for the plasma's. The
/// controls: the plasma's full pass flashes, its repeat draws, and the
/// cap under the plasma's mix over its own text is below source-over's.
/// Perturbations: the cap scanned source-over, or cached without its mix.
#[test]
fn a_repeat_plasma_in_front_of_the_text_makes_no_transition() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut bytes = b"\x1b[?25l".to_vec();
    for row in 0..usize::from(WS.ws_row) {
        bytes.extend(format!("\x1b[{};1H", row + 1).bytes());
        bytes.extend(core::iter::repeat_n(b'X', usize::from(WS.ws_col)));
    }
    feed(&mut t, &bytes);
    for (theme, mix) in [
        (Theme::dark(), Mix::Screen),
        (Theme::light(), Mix::Multiply),
    ] {
        let (ink, page) = (
            theme.fg.with_alpha(Chan::from_u8(0xFF)),
            theme.bg.with_alpha(Chan::from_u8(0xFF)),
        );
        if mix == Mix::Screen {
            assert!(
                under_threshold_over_by(ink, ink, page, mix).get()
                    < under_threshold_over(ink, ink, page).get(),
                "{mix:?}: the control, a lower cap under the mix"
            );
        }
        let mut r = renderer(&faces);
        r.set_theme(theme);
        r.set_trail(None);
        let _ = paint(&mut r, &mut t);
        let bare = paint(&mut r, &mut t);
        let transitions = |frame: &[u32]| {
            frame
                .iter()
                .zip(&bare)
                .filter(|&(&p, &q)| {
                    let (a, b) = (lum(p), lum(q));
                    (a - b).abs() >= 0.10 && a.min(b) < 0.80
                })
                .count()
        };
        // The fire's repeat over the same cells, source-over, first.
        r.set_trail(Some(repeat(TrailLook::Vfx(vfx(&r, Effect::Fire, 0.06)))));
        let _ = paint(&mut r, &mut t);
        let _ = paint(&mut r, &mut t);
        for age in [0.04f32, 0.06, 0.1] {
            let full = TrailLook::Vfx(vfx(&r, Effect::Plasma, age));
            r.set_trail(Some(full));
            assert!(
                transitions(&paint(&mut r, &mut t)) > 0,
                "{mix:?} at {age}: the control, the full pass flashes"
            );
            r.set_trail(Some(repeat(full)));
            let _ = paint(&mut r, &mut t);
            let repeated = paint(&mut r, &mut t);
            assert_eq!(r.trail_mix(), mix, "{mix:?} at {age}: the mix");
            assert!(
                repeated.iter().zip(&bare).any(|(p, q)| p != q),
                "{mix:?} at {age}: the control, the repeat draws"
            );
            assert_eq!(
                transitions(&repeated),
                0,
                "{mix:?} at {age}: a repeat pass's pixel flashes"
            );
        }
    }
}

/// **The cap's work is bounded a frame, and the bound stays sound while it
/// is**: over more than 64 distinct grounds -- a row of cells in 70
/// background colours -- a repeat pass draws nothing, a cap of 0, rather
/// than scan every one, which over a picture cost 42-97 ms a frame. Over a
/// few grounds, two new caps are scanned a frame and the pass draws nothing
/// until all are: three colours under it read 0 on the first frame and
/// their least cap on the second. The control: the same three colours'
/// least cap, scanned directly. And the renderer says why ([`Held`]): over
/// 70, too many grounds on both frames; over three, still scanning, then
/// capped.
#[test]
fn a_repeat_pass_over_many_grounds_draws_nothing() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let black = opaque(0, 0, 0);
    let colour = |n: u8| opaque(0x40 + n, 0x80, 0x20 + 2 * n);
    let (mut caps, mut held) = (Vec::new(), Vec::new());
    for count in [70u8, 3] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l\x1b]12;rgb:00/00/00\x07");
        for row in 1..9 {
            let mut line = format!("\x1b[{row};1H");
            for col in 0..30u8 {
                let c = colour((col + 30 * (row - 1)) % count);
                line.push_str(&format!(
                    "\x1b[48;2;{};{};{}m ",
                    c.r().get(),
                    c.g().get(),
                    c.b().get()
                ));
            }
            feed(&mut t, format!("{line}\x1b[0m").as_bytes());
        }
        let mut r = renderer(&faces);
        let _ = paint(&mut r, &mut t);
        // A quad over the eight coloured rows whole, the cursor below them.
        let style = baud_core::CursorStyle::SteadyBlock;
        let (a, b) = (
            r.cursor_rect(Row::from_index(1), Col::at_index(0), style),
            r.cursor_rect(Row::from_index(8), Col::at_index(29), style),
        );
        let hole = r.cursor_rect(Row::from_index(9), Col::at_index(29), style);
        let over = baud_render::Rect {
            left: a.left,
            top: a.top,
            right: b.right,
            bottom: b.bottom,
        };
        r.set_trail(Some(repeat(TrailLook::Smear(Smear {
            corners: over.corners(),
            hole,
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Smear::OPAQUE,
        }))));
        let _ = paint(&mut r, &mut t);
        let first = (r.flash_cap(), r.flash_held());
        let _ = paint(&mut r, &mut t);
        caps.push((first.0, r.flash_cap()));
        held.push((first.1, r.flash_held()));
    }
    assert_eq!(caps[0], (Chan::from_u8(0), Chan::from_u8(0)), "70 grounds");
    assert_eq!(held[0], (Held::Grounds, Held::Grounds), "70 grounds");
    assert_eq!(held[1], (Held::Scanning, Held::Capped), "three grounds");
    let least = (0..3)
        .map(|n| under_threshold(black, colour(n)).get())
        .min()
        .unwrap();
    assert!(least > 0, "the control: these grounds have a cap");
    assert_eq!(
        caps[1].0.get(),
        0,
        "three grounds, two scanned on frame one"
    );
    assert_eq!(caps[1].1.get(), least, "every ground scanned on frame two");
}

fn b64(data: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            if k <= q.len() {
                out.push(A[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}

/// One kitty graphics command.
fn apc(t: &mut Term<'_>, ctrl: &str, payload: &[u8]) {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend_from_slice(payload);
    }
    v.extend_from_slice(b"\x1b\\");
    feed(t, &v);
}

/// **An image under the text is a ground the cap cannot read**, and a
/// repeat pass over one draws nothing (the route a picture takes): the
/// cells under a kitty image hold the page's styles while the pixels the
/// pass lands on are the image's, so the page's cap over a `#404040` image
/// made 672 general transitions on a 30 by 10 grid. An image over the text
/// hides the pass, which keeps the page's cap: the control for the layer.
/// The control for the pass: the same quad on a bare page draws at the
/// page's cap. The renderer says which: imaged under the text, capped
/// otherwise.
#[test]
fn a_repeat_pass_over_an_image_under_the_text_draws_nothing() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let page = under_threshold(
        ink_of(&Theme::dark()).with_alpha(Chan::from_u8(0xFF)),
        Theme::dark().bg.with_alpha(Chan::from_u8(0xFF)),
    );
    assert!(page.get() > 0, "the control: the page has a cap");
    // No image, then one over the text, under it, below the backgrounds.
    for (z, cap, why) in [
        (None, page, Held::Capped),
        (Some(1i32), page, Held::Capped),
        (Some(-1), Chan::from_u8(0), Held::Imaged),
        (Some(-1_073_741_825), Chan::from_u8(0), Held::Imaged),
    ] {
        let mut r = renderer(&faces);
        let m = r.metrics();
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(
            ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
            ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
        );
        feed(&mut t, b"\x1b[?25l");
        if let Some(z) = z {
            let grey = [0x40, 0x40, 0x40, 0xFF].repeat(24);
            apc(&mut t, "a=t,f=32,s=6,v=4,i=1,q=2", &b64(&grey));
            feed(&mut t, b"\x1b[2;1H");
            apc(&mut t, &format!("a=p,i=1,p=1,q=2,c=30,r=8,z={z}"), b"");
            feed(&mut t, b"\x1b[1;1H");
        }
        let bare = paint(&mut r, &mut t);
        r.set_trail(Some(repeat(TrailLook::Smear(smear(
            &r,
            (1, 2),
            (7, 20),
            0.05,
        )))));
        let _ = paint(&mut r, &mut t);
        let frame = paint(&mut r, &mut t);
        let transitions = frame
            .iter()
            .zip(&bare)
            .filter(|&(&p, &q)| {
                let (a, b) = (lum(p), lum(q));
                (a - b).abs() >= 0.10 && a.min(b) < 0.80
            })
            .count();
        let at = format!("z {z:?}");
        assert_eq!(r.flash_cap(), cap, "the cap, {at}");
        assert_eq!(r.flash_held(), why, "why, {at}");
        assert_eq!(
            transitions, 0,
            "a repeat pass flashes over the ground, {at}"
        );
        let drew = frame.iter().zip(&bare).any(|(p, q)| p != q);
        assert_eq!(drew, z.is_none(), "it draws only on a bare page, {at}");
    }
}

/// The pixels of `frame` that are a general transition from `bare`: a
/// change in relative luminance of 0.10 or more, the darker below 0.80.
fn general(frame: &[u32], bare: &[u32]) -> usize {
    frame
        .iter()
        .zip(bare)
        .filter(|&(&p, &q)| {
            let (a, b) = (lum(p), lum(q));
            (a - b).abs() >= 0.10 && a.min(b) < 0.80
        })
        .count()
}

/// Whether two words are a red transition, the Understanding page's
/// working definition: either a saturated red, R/(R+G+B) of 0.8 or more on
/// the linear values, and their u'v' more than 0.2 apart.
fn red(p: u32, q: u32) -> bool {
    let see = |w: u32| {
        let [_, r, g, b] = w.to_be_bytes();
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (lin(r), lin(g), lin(b));
        let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
        let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
        let z = 0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b;
        let d = x + 15.0 * y + 3.0 * z;
        let (u, v) = if d > 0.0 {
            (4.0 * x / d, 9.0 * y / d)
        } else {
            (0.197_8, 0.468_3)
        };
        let sum = r + g + b;
        (u, v, sum > 0.0 && r / sum >= 0.8)
    };
    let ((u, v, a), (s, t, b)) = (see(p), see(q));
    (a || b) && (u - s).hypot(v - t) > 0.2
}

/// A quad over rows 1 to 8 whole, its hole on the last row, away from them.
fn over_rows(r: &Renderer<'_, '_>) -> TrailLook {
    let style = baud_core::CursorStyle::SteadyBlock;
    let (a, b) = (
        r.cursor_rect(Row::from_index(1), Col::at_index(0), style),
        r.cursor_rect(Row::from_index(8), Col::at_index(29), style),
    );
    repeat(TrailLook::Smear(Smear {
        corners: baud_render::Rect {
            left: a.left,
            top: a.top,
            right: b.right,
            bottom: b.bottom,
        }
        .corners(),
        hole: r.cursor_rect(Row::from_index(9), Col::at_index(29), style),
        shape: Shape::Quad,
        strength: Strength::Full,
        opacity: Smear::OPAQUE,
    }))
}

/// **A repeat pass over a selection is capped against the selection's
/// colours**: selected blank cells on the dark page draw the theme's text,
/// `#D8D8D8`, as their background, and a black cursor over that ground has
/// a cap well under the page's 255, at which black never flashes on
/// `#101010`. No pixel of the pass is a general transition from the frame
/// without it; the controls: the pass draws, and the same pass with no
/// selection keeps the page's cap.
#[test]
fn a_repeat_pass_over_a_selection_is_capped_against_its_colours() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let black = opaque(0, 0, 0);
    let selected = Theme::dark().fg.with_alpha(Chan::from_u8(0xFF));
    for selection in [false, true] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l\x1b]12;rgb:00/00/00\x07");
        let mut r = renderer(&faces);
        r.set_highlight(
            selection
                .then_some(baud_render::Highlight::new(
                    (Row::from_index(2), Col::at_index(0)),
                    (Row::from_index(7), Col::at_index(29)),
                    baud_render::Region::Stream,
                ))
                .map(|h| h.span()),
        );
        let bare = paint(&mut r, &mut t);
        r.set_trail(Some(over_rows(&r)));
        let _ = paint(&mut r, &mut t);
        let frame = paint(&mut r, &mut t);
        if !selection {
            assert_eq!(r.flash_cap().get(), 0xFF, "the control: the page's cap");
            continue;
        }
        assert!(
            frame.iter().zip(&bare).any(|(p, q)| p != q),
            "the control: the pass draws"
        );
        assert_eq!(
            general(&frame, &bare),
            0,
            "a pixel over the selection flashes"
        );
        let k = under_threshold(black, selected);
        assert!(k.get() < 0x40, "the control: a cap over the selection");
        assert_eq!(r.flash_cap(), k, "the cap over the selected cells");
    }
}

/// **A repeat pass over a block cursor is capped against the block's
/// colours**: a block on a blank cell whose text is `#1A0000` on the
/// page's own colour fills with that dark red, the reverse video a block
/// draws, where the cell's own rendition draws only the page. The theme's
/// cursor over dark red takes 4 of 255 (note AL's table, by the red rule),
/// against the page's 95, at which its pixels over the block are red
/// transitions. The pass here is a constructed one, its hole away from the
/// cursor, since on the CPU path a quad always spares the cursor's cell; on
/// the GPU an effect's bands can cover it, and they do here (the control
/// below), which the cap is for.
#[test]
fn a_repeat_pass_over_a_block_cursor_is_capped_against_the_block() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let ink = Theme::dark().fg.with_alpha(Chan::from_u8(0xFF));
    let page = Theme::dark().bg.with_alpha(Chan::from_u8(0xFF));
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // Both colours set as truecolour, the page's own `#101010` under the
    // dark red, so the contrast floor leaves the pair as asked.
    feed(
        &mut t,
        b"\x1b[2 q\x1b[5;11H\x1b[38;2;26;0;0;48;2;16;16;16m \x1b[0m\x1b[5;11H",
    );
    // The block of no colour of its own, the cell's dark red swapped in.
    let mut r = renderer(&faces);
    r.set_theme(support::classic(Theme::dark()));
    let bare = paint(&mut r, &mut t);
    r.set_trail(Some(over_rows(&r)));
    let _ = paint(&mut r, &mut t);
    let frame = paint(&mut r, &mut t);
    let k = under_threshold(ink, opaque(0x1A, 0, 0));
    assert_eq!(
        k.get(),
        4,
        "note AL's cap for the theme's cursor on dark red"
    );
    assert!(
        under_threshold(ink, page).get() > 0x40,
        "the control: the page's cap is far above it"
    );
    let (w, _) = r.pixel_size();
    let cell = r.cursor_rect(
        Row::from_index(4),
        Col::at_index(10),
        baud_core::CursorStyle::SteadyBlock,
    );
    let inside = |i: usize| {
        let (x, y) = ((i % px(w)) as f32 + 0.5, (i / px(w)) as f32 + 0.5);
        x > cell.left && x < cell.right && y > cell.top && y < cell.bottom
    };
    let (mut drew, mut flashed) = (false, 0);
    for (i, (p, q)) in frame.iter().zip(&bare).enumerate() {
        if inside(i) {
            drew |= p != q;
            flashed += general(&[*p], &[*q]) + usize::from(red(*p, *q));
        }
    }
    assert!(drew, "the control: the pass draws over the block");
    assert_eq!(flashed, 0, "a pixel over the block makes a transition");
    assert_eq!(r.flash_cap(), k, "the cap over the block");
    // The GPU's case: a repeat fire landed on the cursor, its bands over
    // the cursor's cell, reads the block too.
    let style = baud_core::CursorStyle::SteadyBlock;
    let hole = r.cursor_rect(Row::from_index(4), Col::at_index(10), style);
    let from = r.cursor_rect(Row::from_index(1), Col::at_index(2), style);
    let emitter = Smear {
        corners: Segment::new(from.corners(), hole).at(0.3),
        hole,
        shape: Shape::Quad,
        strength: Strength::Full,
        opacity: Smear::OPAQUE,
    };
    let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
    let tail = Pt {
        x: (from.left + from.right) * 0.5,
        y: (from.top + from.bottom) * 0.5,
    };
    r.set_trail(Some(repeat(TrailLook::Vfx(Vfx {
        effect: Effect::Fire,
        emitter,
        ignition: Ignition::ignite(tail, hole, Seed(5), unit).at(0.3),
        axis: Pt { x: 1.0, y: 0.0 },
    }))));
    let _ = paint(&mut r, &mut t);
    let _ = paint(&mut r, &mut t);
    let covers = r.trail().iter().any(|i| {
        let (x, y) = (f32::from(i.x.0), f32::from(i.y.0));
        x < hole.right
            && hole.left < x + f32::from(i.w.0)
            && y < hole.bottom
            && hole.top < y + f32::from(i.h.0)
    });
    assert!(
        covers,
        "the control: the fire's bands cover the cursor's cell"
    );
    assert_eq!(r.flash_cap(), k, "the fire's cap over the block");
}

/// The frame as a screen shows it over a flat desktop `d`: each straight
/// pixel premultiplied as the presenter does, then over `d`.
fn on_desktop(frame: &[u32], d: u8) -> Vec<u32> {
    frame
        .iter()
        .map(|&p| {
            let [a, r, g, b] = p.to_be_bytes();
            let f = |c: u8| {
                let pc = baud_domain::premultiply_chan(Chan::from_u8(c), Chan::from_u8(a)).get();
                let v = u32::from(pc) + (u32::from(d) * (255 - u32::from(a)) + 127) / 255;
                u8::try_from(v.min(255)).unwrap()
            };
            u32::from_be_bytes([0xFF, f(r), f(g), f(b)])
        })
        .collect()
}

/// **A repeat pass over the dimmed glass page is capped against the page
/// as the screen shows it**: the page over the desktop behind the window,
/// which note AS bounds by a flat white and a flat black, the material
/// darkening nothing. At the shipped dims, 0.70 dark and 0.60 light (alpha
/// 179 and 153), the dark page over white is a mid grey that the theme's
/// cursor at the opaque page's cap of 95 flashes, and the light page over
/// black is a grey that a white cursor, which never flashes on the opaque
/// light page, flashes at any alpha. No pixel of either pass is a general
/// transition over either desktop; the control: each pass draws.
#[test]
fn a_repeat_pass_over_the_glass_page_reads_the_desktop() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut flashed = Vec::new();
    for (theme, alpha, ink) in [
        (Theme::dark(), 179u8, &b""[..]),
        (Theme::light(), 153, &b"\x1b]12;rgb:ff/ff/ff\x07"[..]),
    ] {
        let mut theme = theme;
        theme.bg_alpha = Chan::from_u8(alpha);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l");
        feed(&mut t, ink);
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme,
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        let bare = paint(&mut r, &mut t);
        r.set_trail(Some(over_rows(&r)));
        let _ = paint(&mut r, &mut t);
        let frame = paint(&mut r, &mut t);
        let at = format!("alpha {alpha}, cap {}", r.flash_cap().get());
        assert!(
            frame.iter().zip(&bare).any(|(p, q)| p != q),
            "the control: the pass draws, {at}"
        );
        for d in [0xFF, 0] {
            let (seen, under) = (on_desktop(&frame, d), on_desktop(&bare, d));
            let n = general(&seen, &under);
            if n > 0 {
                flashed.push(format!("{n} over the desktop {d:#04x}, {at}"));
            }
        }
    }
    assert!(flashed.is_empty(), "pixels flash: {flashed:?}");
}

/// **A repeat ghost is drawn under the cap**: the ghost's one record, a
/// cell-sized box that starts at alpha 176, carries at most the cap of the
/// cursor's colour over the page, and no pixel of it is a general
/// transition from the frame without it. The control: the same sweep at
/// full strength flashes.
#[test]
fn a_repeat_ghost_is_drawn_under_the_cap() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    let mut r = renderer(&faces);
    let bare = paint(&mut r, &mut t);
    let k = under_threshold(
        ink_of(&Theme::dark()).with_alpha(Chan::from_u8(0xFF)),
        Theme::dark().bg.with_alpha(Chan::from_u8(0xFF)),
    );
    let mut flashed = Vec::new();
    for strength in [Strength::Full, Strength::UnderThreshold] {
        r.set_trail(Some(TrailLook::Ghost(baud_render::Trail {
            from: (Row::from_index(1), Col::at_index(1)),
            to: (Row::from_index(8), Col::at_index(25)),
            progress: 0.1,
            origin_dy: 0.0,
            lift: baud_font::Px::of(0),
            strength,
        })));
        let _ = paint(&mut r, &mut t);
        let frame = paint(&mut r, &mut t);
        let drawn = r.trail().iter().map(|i| i.color.a().get()).max();
        flashed.push((general(&frame, &bare), drawn));
    }
    assert!(flashed[0].0 > 0, "the control: a full ghost flashes");
    assert!(
        flashed[0].1.is_some_and(|a| a > k.get()),
        "the control: a full ghost is over the cap"
    );
    assert_eq!(r.flash_cap(), k, "the page's cap");
    assert!(
        flashed[1].1.is_some_and(|a| a > 0 && a <= k.get()),
        "the repeat's record: {:?} against the cap {}",
        flashed[1].1,
        k.get()
    );
    assert_eq!(flashed[1].0, 0, "a repeat ghost's pixel flashes");
}

/// **The renderer says when the bound itself draws nothing**: over a ground
/// where every alpha of the cursor's colour flashes -- the darkest red a
/// white cursor's red transition leaves no byte under -- the scanned cap is
/// 0, [`Held::Bound`], not a ground waiting to be read; the same pass at
/// full strength is [`Held::Full`]. The control: such a ground exists, and
/// one shade lighter has a cap.
#[test]
fn a_repeat_pass_says_when_the_bound_draws_nothing() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let white = opaque(0xFF, 0xFF, 0xFF);
    let red = (1..=0x40u8)
        .find(|&v| under_threshold(white, opaque(v, 0, 0)).get() == 0)
        .expect("the control: a dark red no white alpha is safe over");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        format!("\x1b[?25l\x1b]12;rgb:ff/ff/ff\x07\x1b[48;2;{red};0;0m\x1b[2J").as_bytes(),
    );
    let mut r = renderer(&faces);
    let _ = paint(&mut r, &mut t);
    let look = over_rows(&r);
    let full = match look {
        TrailLook::Smear(s) => TrailLook::Smear(Smear {
            strength: Strength::Full,
            ..s
        }),
        other => other,
    };
    r.set_trail(Some(look));
    let _ = paint(&mut r, &mut t);
    let _ = paint(&mut r, &mut t);
    assert_eq!(
        (r.flash_cap().get(), r.flash_held()),
        (0, Held::Bound),
        "over #{red:02X}0000"
    );
    r.set_trail(Some(full));
    let _ = paint(&mut r, &mut t);
    assert_eq!(r.flash_held(), Held::Full, "a full pass after it");
}

/// **A blinking cell in its faint phase is capped against the colour it
/// draws**: under Dim Flashing Lights an `SGR 5` cell's off phase draws its
/// text half way to the page (`Blink::Faint`), and a repeat pass over it
/// is held to that faint text over the page, not to the full text it
/// draws in the other phase. The theme's `#D8D8D8` over `#101010` is
/// `#74` grey faint, and a black cursor over that ground takes a cap well
/// over the full text's. The controls: the shown phase takes the full
/// text's cap, and the two caps differ.
#[test]
fn a_repeat_pass_over_faint_blinking_text_is_capped_against_the_faint_colour() {
    use baud_render::{Blink, BlinkPhases};
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let black = opaque(0, 0, 0);
    let page = Theme::dark().bg.with_alpha(Chan::from_u8(0xFF));
    let text = Theme::dark().fg.with_alpha(Chan::from_u8(0xFF));
    let half = |x: u8, y: u8| u8::try_from((u16::from(x) + u16::from(y)).div_ceil(2)).unwrap();
    let faint = opaque(
        half(text.r().get(), page.r().get()),
        half(text.g().get(), page.g().get()),
        half(text.b().get(), page.b().get()),
    );
    let mut caps = Vec::new();
    for phase in [Blink::Shown, Blink::Faint] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l\x1b]12;rgb:00/00/00\x07");
        for row in 1..9 {
            feed(
                &mut t,
                format!("\x1b[{row};1H\x1b[5m{}\x1b[0m", "X".repeat(30)).as_bytes(),
            );
        }
        let mut r = renderer(&faces);
        r.set_blink(BlinkPhases {
            slow: phase,
            rapid: phase,
        });
        let _ = paint(&mut r, &mut t);
        r.set_trail(Some(over_rows(&r)));
        let _ = paint(&mut r, &mut t);
        let _ = paint(&mut r, &mut t);
        caps.push(r.flash_cap());
    }
    let full = under_threshold_over(black, text, page);
    let drawn = under_threshold_over(black, faint, page);
    assert_eq!(faint, opaque(0x74, 0x74, 0x74), "the faint colour");
    assert_eq!(caps[0], full, "the control: the shown phase's cap");
    assert!(full.get() < drawn.get(), "the control: the two caps differ");
    assert_eq!(caps[1], drawn, "the faint phase's cap");
}

/// **A repeat pass over the cursor guide is capped against it**: the guide
/// over the page is a ground no cell names, at its middle's alpha and its
/// lines'. Black never flashes on the page, `#101010`; the guide over it is
/// lighter, and black over that takes a cap under 255. The control: with no
/// guide the same pass takes the page's 255.
#[test]
fn a_repeat_pass_over_the_cursor_guide_is_capped_against_it() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let black = opaque(0, 0, 0);
    let page = Theme::dark().bg.with_alpha(Chan::from_u8(0xFF));
    let mut caps = Vec::new();
    for guide in [false, true] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        // A bar cursor at row 5, column 25, out of the pass's columns.
        feed(&mut t, b"\x1b]12;rgb:00/00/00\x07\x1b[6 q\x1b[6;26H");
        let mut r = renderer(&faces);
        r.set_cursor_guide(guide.then_some(baud_render::GuideLook {
            colour: baud_render::CURSOR_GUIDE,
            line: Px::of(1),
        }));
        let _ = paint(&mut r, &mut t);
        let style = baud_core::CursorStyle::SteadyBlock;
        let (a, b) = (
            r.cursor_rect(Row::from_index(1), Col::at_index(0), style),
            r.cursor_rect(Row::from_index(8), Col::at_index(15), style),
        );
        r.set_trail(Some(repeat(TrailLook::Smear(Smear {
            corners: baud_render::Rect {
                left: a.left,
                top: a.top,
                right: b.right,
                bottom: b.bottom,
            }
            .corners(),
            hole: r.cursor_rect(Row::from_index(5), Col::at_index(25), style),
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Smear::OPAQUE,
        }))));
        // Two new caps are scanned a frame: enough frames for them all.
        for _ in 0..4 {
            let _ = paint(&mut r, &mut t);
        }
        assert_eq!(r.flash_held(), Held::Capped, "guide {guide}");
        caps.push(r.flash_cap());
    }
    assert_eq!(caps[0].get(), 0xFF, "the control: the page's cap");
    let guide = baud_render::CURSOR_GUIDE.with_alpha(Chan::from_u8(0xFF));
    let want = [64u8, 112]
        .into_iter()
        .map(|a| guide.mix(page, Chan::from_u8(a)))
        .flat_map(|g| {
            [
                under_threshold_over(black, g, g),
                under_threshold_over(black, page, g),
            ]
        })
        .min_by_key(|c| c.get())
        .unwrap();
    assert!(
        want.get() < 0xFF,
        "the control: the guide is a ground black flashes on"
    );
    assert_eq!(caps[1], want, "the cap over the guide");
}
