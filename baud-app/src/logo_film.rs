//! **The README's logo, filmed without a window**: the lockup the launch
//! splash holds, `baud ❯ █`, at twice its 300-unit size on GitHub's own
//! page colour, with baud's plasma moving round and through it as if each
//! glyph were a solid wall, its block blinking as baud's cursor blinks by
//! default, for the whole of the plasma's loop, the file repeating so the
//! plasma never rests. `tools/baud-pictures logo` runs
//! [`film_the_readme_logo_bare`] and [`film_the_readme_logo`] for each
//! half and encodes what they write into `assets/readme/logo-dark.gif`
//! and `logo-light.gif`; nothing opens a window.
//!
//! **A test module run from a script, as `film.rs` is**, and not an
//! example: the blink is the app's own, `crate::caret`'s plan for the
//! config's defaults, and the app is a binary, whose modules no example
//! can reach. So the logo blinks with whatever baud's default blink is.
//!
//! **Every pixel is the CPU renderer's** (`Renderer::render`): the lockup
//! is the splash's held records (`baud_render::splash`), the block takes
//! the caret's phase through the cursor's own drawing
//! (`Renderer::set_splash_cursor`), and the plasma is the splash's layers
//! (`Renderer::set_splash_layers`), each strand one plasma ignition drawn
//! by the trail's own records, in the word's ink, behind the glyphs or in
//! front of them, through the glyphs as slabs ([`slabs`]). What the plasma
//! does -- its path, its depth, the walls, the current and its tail -- is
//! `tools/baud-pictures plasma`'s, which writes each frame's strands;
//! the film draws them.
//!
//! **Where it departs from a standard, knowingly.** WCAG 2.2's 2.2.2 Pause,
//! Stop, Hide (Level A) asks that moving or blinking content which starts by
//! itself, lasts more than five seconds and is shown beside other content
//! can be paused, stopped or hidden (`[[source: wcag22]]`). A README carries
//! no control of its own, so a film that repeats does not meet it on a
//! README's page; `tools/baud-pictures`'s `logo.rs` says the rest. WCAG
//! 2.3.1 is met, the census counting the repeat and its join.

use crate::caret::{Blinks, Caret, Cycle, Gate, NonBlinking};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::det::Det;
use baud_render::effect::{Effect, Ignition, Live, Seed, Vfx};
use baud_render::splash::{self, Depth, Knot, Layer, Rim, Slabs};
use baud_render::{
    Argb, Blend, Corners, CursorPhase, Damage, FaceRole, FaceStyle, Pt, PxSize, Rect, Renderer,
    Shape, Smear, SplashLook, SplashPhase, Stack, Strength, Target, Theme,
};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// The terminal the logo is drawn over: 250 by 37 cells at 14 px, a
/// frame of 2000 by 666 pixels, where the splash's scale is 2.
const WS: WinSize = WinSize {
    ws_col: 250,
    ws_row: 37,
};
const PX: f32 = 14.0;

/// The splash's scale the film is drawn at: two pixels a unit, so shown
/// 420 pixels wide the lockup is its own 300 at 1.4 of the width.
const SCALE: f32 = 2.0;

/// **The film's canvas**, in the lockup's units: its 300 by 130 box with
/// 60 units to each side, 75 above and 85 below -- 840 by 580 pixels.
const CANVAS: [f32; 4] = [-60.0, -75.0, 360.0, 215.0];

/// GitHub's page colours: its dark theme's canvas and its light one's.
const PAGE_DARK: Argb = Argb(0xFF0D_1117);
const PAGE_LIGHT: Argb = Argb(0xFFFF_FFFF);

/// The files the script writes: each half's GIF and SVG.
const GIFS: [(&str, Argb); 2] = [("logo-dark.gif", PAGE_DARK), ("logo-light.gif", PAGE_LIGHT)];
const SVGS: [&str; 2] = ["logo-dark.svg", "logo-light.svg"];

/// **The keyline round a letter in front of plasma**, in pixels: at most
/// 2, or 0.3 of the opening's half-width where that is less -- the
/// largest distance to the ink within 8 pixels -- and never under half a
/// pixel, with a pixel and a half of falloff. Two pixels keep 0.62 to 0.88
/// of the light behind a letter that shows through its openings, where
/// three took a third of it out of the narrow slots.
const KEYLINE: (f32, f32, f32, f32) = (2.0, 0.3, 0.5, 1.5);

/// **The knot cut**: a front strand's light from 0.5 to 0.85 is its core,
/// grown by 2 pixels, and the letter under it is taken 0.75 of the way to
/// the page. A core from 0.35 caught the strand's sheath too and greyed a
/// band across the letter.
const KNOT: Knot = Knot {
    core: [0.5, 0.85],
    grow: 2.0,
    strength: 0.75,
};

/// **The rim's gain**, on the glyphs' inner edge, 3.5 to 2 pixels in.
const RIM: f32 = 0.5;

/// A count of the blink's cycles, each a phase shown and one hidden.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Cycles(u32);

/// A GIF frame's delay, in hundredths of a second.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Hundredths(u16);

/// **The blink's whole cycles in a film of `end`**, which it has to hold
/// whole, so the first frame's phase follows the last's across the wrap.
fn cycles(end: Duration, on: Duration, off: Duration) -> Cycles {
    let cycle = (on + off).as_nanos();
    assert_eq!(
        end.as_nanos() % cycle,
        0,
        "the film of {end:?} is not whole cycles of {on:?} and {off:?}"
    );
    Cycles(u32::try_from(end.as_nanos() / cycle).expect("a count of cycles"))
}

/// `d` in hundredths of a second, which it has to be whole.
fn hundredths(d: Duration) -> Hundredths {
    assert_eq!(
        d.as_nanos() % 10_000_000,
        0,
        "{d:?} is not whole hundredths, which a GIF's delay is"
    );
    Hundredths(u16::try_from(d.as_millis() / 10).expect("a GIF's delay"))
}

/// The default blink's caret, fresh at `epoch`, and its two phases.
fn default_caret(epoch: Instant) -> (Caret<Instant>, Duration, Duration) {
    let cfg = crate::config::Config::defaults();
    let cycle = Cycle::of_config(cfg.blink_interval);
    let Cycle::Blinks { on, off } = cycle else {
        panic!("baud's default cursor does not blink");
    };
    let caret = Caret::new(
        epoch,
        cycle,
        Gate::of(cfg.cursor_blink, NonBlinking::Off),
        cfg.cursor_stop,
        cfg.blink_animation,
    );
    (caret, on.get(), off.get())
}

/// **The phase the caret shows over a frame from `at` to `end`**, asked
/// at its start and at its last moment: the film draws one picture a
/// frame, so a blink edge inside one is refused. `caret` is asked in time
/// order.
fn phase_over(
    caret: &mut Caret<Instant>,
    epoch: Instant,
    at: Duration,
    end: Duration,
) -> CursorPhase {
    let phase = caret.plan(epoch + at, Blinks::Yes);
    assert_eq!(
        caret.plan(epoch + end - Duration::from_nanos(1), Blinks::Yes),
        phase,
        "the blink moves within the frame from {at:?} to {end:?}"
    );
    phase
}

/// The block's box on the canvas, in pixels, `inset` in from each side.
fn block_on_canvas(inset: f32) -> [usize; 4] {
    let b = splash::BLOCK;
    let px = |v: f32, from: f32, k: f32| {
        usize::try_from(((v - from) * SCALE + k).round() as i64).expect("on the canvas")
    };
    [
        px(b.left, CANVAS[0], inset),
        px(b.top, CANVAS[1], inset),
        px(b.right, CANVAS[0], -inset),
        px(b.bottom, CANVAS[1], -inset),
    ]
}

/// The canvas's size in pixels.
fn canvas_size() -> (usize, usize) {
    let px = |v: f32| usize::try_from((v * SCALE).round() as i64).expect("a side");
    (px(CANVAS[2] - CANVAS[0]), px(CANVAS[3] - CANVAS[1]))
}

/// The canvas's top-left corner on the frame, the lockup's box at `at`.
fn canvas_at(at: Pt) -> (usize, usize) {
    let px = |v: f32| usize::try_from(v.round() as i64).expect("the canvas is on the frame");
    (px(at.x + CANVAS[0] * SCALE), px(at.y + CANVAS[1] * SCALE))
}

/// The canvas cut out of `frame`, `wide` a row, as RGB.
fn canvas(frame: &[u32], wide: Px, at: Pt) -> Vec<u8> {
    let wide = usize::try_from(wide.value()).expect("a width");
    let (x0, y0) = canvas_at(at);
    let (w, h) = canvas_size();
    let mut out = Vec::with_capacity(w * h * 3);
    for y in y0..y0 + h {
        for &p in &frame[y * wide + x0..y * wide + x0 + w] {
            let [_, r, g, b] = p.to_be_bytes();
            out.extend_from_slice(&[r, g, b]);
        }
    }
    out
}

/// `smoothstep(e0, e1, x)`.
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let k = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// **The lockup's glyphs as slabs, off the bare canvas** (`splash::Slabs`),
/// over the lockup's ink box, units 55 to 244 by 42.5 to 91, and 12 pixels
/// round it: each pixel's glyph coverage, solved along the line from the
/// page to the ink it is nearer, the word's or the amber's; the behind cut
/// that coverage grown by the [`KEYLINE`]; the [`KNOT`] cut, kept off the
/// block, whose amber parts it from the plasma already and which the cut
/// would take away; and the [`RIM`] on the glyphs' inner edge. `sdf` is
/// the canvas's signed distance to the outline in pixels
/// (`tools/baud-pictures geometry`).
fn slabs(bare: &[u8], at: Pt, (page, ink, amber): (Argb, Argb, Argb), sdf: &[f32]) -> Slabs {
    let (cw, _) = canvas_size();
    let (ox, oy) = canvas_at(at);
    let cx = |u: f32| usize::try_from(((u - CANVAS[0]) * SCALE).round() as i64).expect("on it");
    let cy = |v: f32| usize::try_from(((v - CANVAS[1]) * SCALE).round() as i64).expect("on it");
    let (x0, y0, x1, y1) = (cx(55.0) - 12, cy(42.5) - 12, cx(244.0) + 12, cy(91.0) + 12);
    let (w, h) = (x1 - x0, y1 - y0);
    let rgb = |c: Argb| [c.r(), c.g(), c.b()].map(|v| f32::from(v.get()));
    let (pg, wi, am) = (rgb(page), rgb(ink), rgb(amber));
    // A pixel's coverage along the page-to-`k` line, and how far off it.
    let along = |p: [f32; 3], k: [f32; 3]| {
        let d = [k[0] - pg[0], k[1] - pg[1], k[2] - pg[2]];
        let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let dot = (p[0] - pg[0]) * d[0] + (p[1] - pg[1]) * d[1] + (p[2] - pg[2]) * d[2];
        let c = (dot / dd).clamp(0.0, 1.0);
        let e = (0..3)
            .map(|i| (p[i] - (pg[i] + c * d[i])).abs())
            .fold(0.0f32, f32::max);
        (c, e)
    };
    let mut glyph = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = ((y0 + y) * cw + x0 + x) * 3;
            let p = [bare[i], bare[i + 1], bare[i + 2]].map(f32::from);
            let ((a, ea), (b, eb)) = (along(p, wi), along(p, am));
            glyph[y * w + x] = if ea <= eb { a } else { b };
        }
    }
    let sd = |x: usize, y: usize| sdf[(y0 + y) * cw + x0 + x];
    let (most, share, least, fall) = KEYLINE;
    let mut keyline = vec![most; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut best = f32::MIN;
            for yy in y.saturating_sub(8)..(y + 9).min(h) {
                for xx in x.saturating_sub(8)..(x + 9).min(w) {
                    let (dx, dy) = (xx.abs_diff(x), yy.abs_diff(y));
                    if dx * dx + dy * dy <= 64 {
                        best = best.max(sd(xx, yy));
                    }
                }
            }
            keyline[y * w + x] = (share * best).clamp(least, most);
        }
    }
    let mut cut = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let k = keyline[y * w + x];
            let mut best = 0.0f32;
            for yy in y.saturating_sub(4)..(y + 5).min(h) {
                for xx in x.saturating_sub(4)..(x + 5).min(w) {
                    let (dx, dy) = (xx as f32 - x as f32, yy as f32 - y as f32);
                    let d = (dx * dx + dy * dy).sqrt();
                    let wgt = (1.0 - (d - k) / fall).clamp(0.0, 1.0);
                    best = best.max(glyph[yy * w + xx] * wgt);
                }
            }
            cut[y * w + x] = best;
        }
    }
    let band = (0..w * h)
        .map(|i| glyph[i] * smooth(-3.5, -2.0, sd(i % w, i / w)))
        .collect();
    // No knot cut on the block, 2 pixels round it.
    let b = splash::BLOCK;
    let lo = |v: f32, from: f32| ((v - from) * SCALE).floor() as i64 - 2;
    let hi = |v: f32, from: f32| ((v - from) * SCALE).ceil() as i64 + 2;
    let bx = lo(b.left, CANVAS[0])..hi(b.right, CANVAS[0]);
    let by = lo(b.top, CANVAS[1])..hi(b.bottom, CANVAS[1]);
    let canvas_i = |v: usize| i64::try_from(v).expect("a canvas pixel");
    for (i, g) in glyph.iter_mut().enumerate() {
        let (x, y) = (canvas_i(x0 + i % w), canvas_i(y0 + i / w));
        if bx.contains(&x) && by.contains(&y) {
            *g = 0.0;
        }
    }
    let px = |v: usize| Px::of(u32::try_from(v).expect("a frame pixel"));
    Slabs {
        left: px(ox + x0),
        top: px(oy + y0),
        width: px(w),
        height: px(h),
        page,
        behind_cut: cut,
        glyph,
        knot: KNOT,
        rim: Rim {
            band,
            gain: RIM,
            ink,
        },
    }
}

/// **A plasma strand from `b` to `f`** (lockup units, the lockup's box at
/// `at`), `width` across, its plasma cell `unit`, its body `age` seconds
/// old on its own clock and its insides on `live`, at `opacity`: one
/// plasma ignition on a quad from `b` to `f`, its hole a sliver past its
/// reach ahead of `f`, as a trail's quad has.
fn strand(
    at: Pt,
    (b, f): (Pt, Pt),
    (width, unit): (f32, f32),
    (age, live): (f32, Live),
    seed: Seed,
    opacity: f32,
) -> Vfx {
    let len = (f.x - b.x).det_hypot(f.y - b.y).max(0.5);
    let dir = Pt {
        x: (f.x - b.x) / len,
        y: (f.y - b.y) / len,
    };
    let n = Pt {
        x: -dir.y,
        y: dir.x,
    };
    let px = |p: Pt| Pt {
        x: at.x + p.x * SCALE,
        y: at.y + p.y * SCALE,
    };
    let off = |p: Pt, a: f32, c: f32| Pt {
        x: p.x + dir.x * a + n.x * c,
        y: p.y + dir.y * a + n.y * c,
    };
    let h = 0.5 * width;
    let quad = [
        off(f, 0.0, -h),
        off(f, 0.0, h),
        off(b, 0.0, h),
        off(b, 0.0, -h),
    ];
    let ahead = px(off(f, 2.5 * unit, 0.0));
    let (hw, ht) = (
        (width * dir.y.abs()).max(0.5) * SCALE,
        (width * dir.x.abs()).max(0.5) * SCALE,
    );
    let hole = Rect {
        left: ahead.x - 0.5 * hw,
        right: ahead.x + 0.5 * hw,
        top: ahead.y - 0.5 * ht,
        bottom: ahead.y + 0.5 * ht,
    };
    let hp = px(f);
    let to = Rect {
        left: hp.x - 0.5,
        right: hp.x + 0.5,
        top: hp.y - 0.5,
        bottom: hp.y + 0.5,
    };
    let mut ignition = Ignition::ignite(px(b), to, seed, unit * SCALE).at(age);
    ignition.live = Some(live);
    Vfx {
        effect: Effect::Plasma,
        emitter: Smear {
            corners: Corners(quad.map(px)),
            hole,
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Chan::from_unit(opacity.clamp(0.0, 1.0)),
        },
        ignition,
        axis: dir,
    }
}

/// **One frame's layers off the generator's lines**, each `C front light
/// bx by fx fy width unit age phase fine k hz ions seed`: a strand behind
/// the glyphs at `light` times one less its `front` weight and in front of
/// them at `light` times it, either left out where it would draw under a
/// thousandth.
fn layers(at: Pt, lines: &[&str]) -> Vec<Layer> {
    let mut out = Vec::new();
    for l in lines {
        let mut it = l.split_whitespace();
        assert_eq!(it.next(), Some("C"), "a strand: {l}");
        let v: Vec<&str> = it.collect();
        assert_eq!(v.len(), 15, "a strand's fifteen numbers: {l}");
        let f: Vec<f32> = v[..14]
            .iter()
            .map(|x| x.parse().expect("a number"))
            .collect();
        let seed = Seed(v[14].parse().expect("a seed"));
        let live = Live {
            phase: f[9],
            fine: [f[10], f[11], f[12]],
            ions: f[13],
        };
        let (b, e) = (Pt { x: f[2], y: f[3] }, Pt { x: f[4], y: f[5] });
        for (w, depth) in [(1.0 - f[0], Depth::Behind), (f[0], Depth::Front)] {
            if w > 0.0 && f[1] * w > 1e-3 {
                let vfx = strand(at, (b, e), (f[6], f[7]), (f[8], live), seed, f[1] * w);
                out.push(Layer { vfx, depth });
            }
        }
    }
    out
}

/// A raw little-endian `f32` field of the canvas.
fn field(path: &std::path::Path) -> Vec<f32> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let f: Vec<f32> = b
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let (w, h) = canvas_size();
    assert_eq!(f.len(), w * h, "{}: a field of the canvas", path.display());
    f
}

/// The variable `k`, a path.
fn path_var(k: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set")))
}

/// The half `BAUD_LOGO_HALF` names: its name, its theme on its page, the
/// page, the word's ink and the amber.
fn half() -> (String, Theme, Argb, Argb, Argb) {
    let half = std::env::var("BAUD_LOGO_HALF").expect("BAUD_LOGO_HALF is not set");
    let (mut theme, page, c) = match half.as_str() {
        "dark" => (Theme::dark(), PAGE_DARK, splash::DARK),
        "light" => (Theme::light(), PAGE_LIGHT, splash::light()),
        other => panic!("BAUD_LOGO_HALF is {other}, not dark or light"),
    };
    theme.bg = page;
    theme.bg_alpha = Chan::from_u8(0xFF);
    (half, theme, page, c.wordmark, c.amber)
}

/// **A frame of the film being drawn**: the renderer with the splash
/// held, the term under it, the frame's pixels and the lockup's box.
struct Frame<'r, 'f, 't> {
    r: &'r mut Renderer<'f, 'f>,
    term: &'r mut Term<'t>,
    px: Vec<u32>,
    at: Pt,
}

impl Frame<'_, '_, '_> {
    /// The canvas as the renderer draws it now.
    fn draw(&mut self) -> Vec<u8> {
        let (w, h) = self.r.pixel_size();
        let mut target = Target::new(&mut self.px, w, w, h).expect("a target");
        self.r.render(self.term, &mut target, Damage::Full);
        canvas(&self.px, w, self.at)
    }
}

/// **`film` run on a frame of `theme`**: the faces the app embeds, its
/// regular and its bold, at [`PX`] over [`WS`], the lockup held at
/// [`SCALE`], an effect drawn on as many threads as the machine has.
fn on_stage<T>(theme: Theme, film: impl FnOnce(&mut Frame<'_, '_, '_>) -> T) -> T {
    let fonts = concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/built/");
    let regular = std::fs::read(format!("{fonts}Regular.ttf")).expect("Regular.ttf");
    let bold = std::fs::read(format!("{fonts}Bold.ttf")).expect("Bold.ttf");
    let faces = [
        Face::parse(&regular).expect("Regular"),
        Face::parse(&bold).expect("Bold"),
    ];
    let stack = Stack::with_roles(
        &faces,
        &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
    )
    .expect("a stack");
    let mut r = Renderer::new(
        stack,
        PxSize::of(PX).expect("a size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .expect("a renderer");
    if let Ok(n) = std::thread::available_parallelism() {
        r.set_threads(n);
    }
    let (w, h) = r.pixel_size();
    let frame_wh = (w.value() as f32, h.value() as f32);
    let scale = splash::scale(frame_wh);
    assert_eq!(scale, SCALE, "the lockup at two pixels a unit");
    let at = splash::origin(frame_wh, scale);
    let mut owned = Owned::new(&WS);
    let mut term = Term::new(owned.storage(), &WS);
    let hide = b"\x1b[?25l";
    term.write_at(BytePos::ZERO, hide);
    term.feed(BytePos::ZERO, ReadLen::from_index(hide.len()));
    r.set_splash(Some(SplashLook {
        phase: SplashPhase::Hold,
    }));
    let px = vec![0u32; usize::try_from(w.value() * h.value()).expect("a frame")];
    film(&mut Frame {
        r: &mut r,
        term: &mut term,
        px,
        at,
    })
}

/// **The README's logo, bare, one half**: `BAUD_LOGO_HALF` (`dark` or
/// `light`) into `BAUD_LOGO_OUT` -- `bare.rgb`, the canvas of the held
/// lockup with the block shown, RGB, which the geometry is read off, and
/// `bare.json`, the canvas's size and the default blink's phases, which
/// the generator builds its frames on.
#[test]
#[ignore = "a tool: tools/baud-pictures logo runs it"]
fn film_the_readme_logo_bare() {
    let out = path_var("BAUD_LOGO_OUT");
    let (half, theme, page, ..) = half();
    let epoch = Instant::now();
    let (mut caret, on, off) = default_caret(epoch);
    let shown = phase_over(&mut caret, epoch, Duration::ZERO, on);
    assert_eq!(shown, CursorPhase::SHOWN, "the blink starts shown");
    let bare = on_stage(theme, |s| {
        s.r.set_splash_cursor(shown);
        s.draw()
    });
    let [_, pr, pg, pb] = page.0.to_be_bytes();
    assert_eq!(bare[..3], [pr, pg, pb], "the canvas's corner is the page");
    std::fs::write(out.join("bare.rgb"), &bare).expect("bare.rgb");
    let (cw, ch) = canvas_size();
    let json = format!(
        "{{\"half\": \"{half}\", \"width\": {cw}, \"height\": {ch}, \"on_ms\": {}, \
         \"off_ms\": {}}}",
        on.as_millis(),
        off.as_millis(),
    );
    std::fs::write(out.join("bare.json"), json).expect("bare.json");
}

/// **The README's logo, one half**: `BAUD_LOGO_HALF` into `BAUD_LOGO_OUT`
/// -- `frames.rgb`, every frame's canvas as RGB one after another,
/// `still.rgb`, the bare logo with the block shown and no plasma, and
/// `film.json`, each frame's delay in hundredths, the canvas's size, the
/// blink's phases and cycles, the block's box and the most layers a frame
/// drew. Each frame is `BAUD_LOGO_PRIMS`'s, the generator's, its strands
/// drawn through the slabs read off the bare canvas and `BAUD_LOGO_GEOM`'s
/// distance field, its block the caret's phase at the frame's time. The
/// film is the whole loop, the blink's whole cycles, its last frame ending
/// at the time the generator wraps at, and no frame is the bare logo.
#[test]
#[ignore = "a tool: tools/baud-pictures logo runs it"]
fn film_the_readme_logo() {
    let out = path_var("BAUD_LOGO_OUT");
    let (half, theme, page, ink, amber) = half();
    let text = std::fs::read_to_string(path_var("BAUD_LOGO_PRIMS")).expect("the strands");
    let sdf = field(&path_var("BAUD_LOGO_GEOM").join("sdf.f32"));
    let mut film: Vec<(Duration, Vec<&str>)> = Vec::new();
    let mut wraps = None;
    for l in text.lines() {
        if let Some(cs) = l.strip_prefix("E ") {
            let cs: u64 = cs.parse().expect("the time the film wraps at");
            wraps = Some(Duration::from_millis(cs * 10));
        } else if let Some(rest) = l.strip_prefix("F ") {
            let cs: u64 = rest
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .expect("a frame's time");
            film.push((Duration::from_millis(cs * 10), Vec::new()));
        } else if !l.trim().is_empty() {
            film.last_mut().expect("a frame first").1.push(l);
        }
    }
    let wraps = wraps.expect("the time the film wraps at");
    let epoch = Instant::now();
    let (mut caret, on, off) = default_caret(epoch);
    let Cycles(n) = cycles(wraps, on, off);
    let mut phases = Vec::new();
    for (i, (at, _)) in film.iter().enumerate() {
        let end = film.get(i + 1).map_or(wraps, |f| f.0);
        assert!(end > *at, "the frames run forward");
        phases.push((
            phase_over(&mut caret, epoch, *at, end),
            hundredths(end - *at),
        ));
    }
    assert_eq!(
        (phases.first().map(|p| p.0), phases.last().map(|p| p.0)),
        (Some(CursorPhase::SHOWN), Some(CursorPhase::HIDDEN)),
        "the blink carries on across the wrap"
    );
    let most = on_stage(theme, |s| {
        s.r.set_splash_ink(Some(ink));
        s.r.set_splash_cursor(CursorPhase::SHOWN);
        let bare = s.draw();
        std::fs::write(out.join("still.rgb"), &bare).expect("still.rgb");
        let at = s.at;
        s.r.set_splash_slabs(Some(slabs(&bare, at, (page, ink, amber), &sdf)));
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(out.join("frames.rgb")).expect("frames.rgb"),
        );
        let mut most = 0usize;
        for ((_, lines), (phase, _)) in film.iter().zip(&phases) {
            let l = layers(at, lines);
            most = most.max(l.len());
            s.r.set_splash_layers(&l);
            s.r.set_splash_cursor(*phase);
            let f = s.draw();
            std::io::Write::write_all(&mut file, &f).expect("a frame");
        }
        std::io::Write::flush(&mut file).expect("frames.rgb");
        most
    });
    let delays: Vec<String> = phases.iter().map(|p| p.1.0.to_string()).collect();
    let (cw, ch) = canvas_size();
    let [x0, y0, x1, y1] = block_on_canvas(0.0);
    // The word's box with room round it, for the script's contrast check.
    let px = |v: f32, from: f32| ((v - from) * SCALE).round();
    let word = [
        px(splash::WORDMARK.x - 8.0, CANVAS[0]),
        px(35.0, CANVAS[1]),
        px(splash::SEAM.0, CANVAS[0]),
        px(97.0, CANVAS[1]),
    ];
    let mut json = String::new();
    write!(
        json,
        "{{\"half\": \"{half}\", \"width\": {cw}, \"height\": {ch}, \"frames\": {}, \
         \"delays_cs\": [{}], \"on_ms\": {}, \"off_ms\": {}, \"cycles\": {n}, \
         \"block\": [{x0}, {y0}, {x1}, {y1}], \"word\": [{}, {}, {}, {}], \
         \"page\": \"#{:06X}\", \"most_layers\": {most}}}",
        film.len(),
        delays.join(", "),
        on.as_millis(),
        off.as_millis(),
        word[0],
        word[1],
        word[2],
        word[3],
        page.0 & 0xFF_FFFF,
    )
    .expect("a string");
    std::fs::write(out.join("film.json"), json).expect("film.json");
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::gif::Gif;

    /// How far the encoder moves a pixel the film draws as the bare logo:
    /// gifski's palette, chosen frame by frame, and the snap of the page's
    /// near neighbours to the page.
    const PALETTE: u8 = 32;

    /// **A film holds the blink whole**: 10 s at the default 250 ms a phase
    /// is twenty cycles, and a length that is not whole cycles is refused,
    /// since its first frame's phase would not follow its last's.
    #[test]
    fn a_film_holds_the_blink_in_whole_cycles() {
        let ms = Duration::from_millis;
        assert_eq!(cycles(ms(10_000), ms(250), ms(250)), Cycles(20));
        assert_eq!(cycles(ms(1998), ms(333), ms(333)), Cycles(3));
        let across = std::panic::catch_unwind(|| cycles(ms(4750), ms(250), ms(250)));
        assert!(across.is_err(), "a film of 19 phases is refused");
    }

    /// **The default blink is the grid the generator builds its frames
    /// on**: 250 ms shown and 250 hidden, which `baud-pictures plasma`
    /// refuses to film otherwise; a frame inside a phase takes it, and one
    /// across a blink edge is refused.
    #[test]
    fn the_default_blink_is_the_generators_grid() {
        let epoch = Instant::now();
        let (mut caret, on, off) = default_caret(epoch);
        let ms = Duration::from_millis;
        assert_eq!((on, off), (ms(250), ms(250)));
        // Asked from the film's start, as the film asks it.
        assert_eq!(
            phase_over(&mut caret, epoch, ms(0), ms(20)),
            CursorPhase::SHOWN
        );
        assert_eq!(
            phase_over(&mut caret, epoch, ms(230), ms(250)),
            CursorPhase::SHOWN
        );
        assert_eq!(
            phase_over(&mut caret, epoch, ms(250), ms(270)),
            CursorPhase::HIDDEN
        );
        let across = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
            phase_over(&mut caret, epoch, ms(480), ms(510))
        }));
        assert!(across.is_err(), "a frame across the edge is refused");
    }

    /// The share of the canvas `rgba`'s box `b` within 16 of `c` in each
    /// channel.
    fn share_near(rgba: &[u8], b: [usize; 4], c: Argb) -> f32 {
        let (w, _) = canvas_size();
        let [_, cr, cg, cb] = c.0.to_be_bytes();
        let (mut n, mut all) = (0u32, 0u32);
        for y in b[1]..b[3] {
            for x in b[0]..b[2] {
                let p = &rgba[(y * w + x) * 4..][..3];
                all += 1;
                n += u32::from(
                    p.iter()
                        .zip([cr, cg, cb])
                        .all(|(&a, b)| a.abs_diff(b) <= 16),
                );
            }
        }
        n as f32 / all as f32
    }

    /// A file of the README's pictures: the committed one, or under
    /// `BAUD_README_FILES` the one `baud-pictures logo --out` wrote there.
    fn readme_file(name: &str) -> String {
        match std::env::var("BAUD_README_FILES") {
            Ok(dir) => format!("{dir}/{name}"),
            Err(_) => format!("{}/../assets/readme/{name}", env!("CARGO_MANIFEST_DIR")),
        }
    }

    /// A count of pixels that moved.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
    struct Moved(usize);

    /// The pixels of two canvases, RGBA, off the block and 2 pixels round
    /// it, which the blink alone moves, that differ past [`PALETTE`].
    fn moved_off_block(a: &[u8], b: &[u8]) -> Moved {
        let (cw, ch) = canvas_size();
        let [bx0, by0, bx1, by1] = block_on_canvas(-2.0);
        Moved(
            (0..cw * ch)
                .filter(|&i| {
                    let (x, y) = (i % cw, i / cw);
                    !((bx0..bx1).contains(&x) && (by0..by1).contains(&y))
                        && (0..3).any(|c| a[i * 4 + c].abs_diff(b[i * 4 + c]) > PALETTE)
                })
                .count(),
        )
    }

    /// The bare logo, RGBA: the SVG's base image, its ground.
    fn still_of(svg: &str) -> Vec<u8> {
        let tags: Vec<&str> = svg.split("<image ").skip(1).collect();
        let href = attr(tags[0], "href");
        let png = unbase64(href.strip_prefix("data:image/png;base64,").expect("a PNG"));
        let png = baud_domain::png::Png::parse(&png).unwrap();
        let mut raw = vec![0u8; png.raw_len().unwrap()];
        let mut rgba = vec![0u8; png.rgba_len().unwrap()];
        png.decode(&mut raw, &mut rgba).unwrap();
        rgba
    }

    /// **The README logos repeat, and the plasma never rests**
    /// (`logo-{dark,light}.gif`, decoded by baud's own GIF decoder): each
    /// plays for ever (`[[source: netscape-2-0-extension]]`), 840 by 580;
    /// no frame is the bare logo, the plasma off the block in every one;
    /// read off the block's pixels -- amber over half of them or over none,
    /// whatever plasma passes over it -- shown and hidden alternate from
    /// shown in whole cycles of the default phases, so the blink carries on
    /// across the wrap; and the wrap, from the last frame to the first,
    /// moves no more pixels than the film's busiest step, so it is no jump.
    #[test]
    fn the_readme_logos_repeat_and_the_plasma_never_rests() {
        let epoch = Instant::now();
        let (_, on, off) = default_caret(epoch);
        let (cw, ch) = canvas_size();
        let inner = block_on_canvas(2.0);
        for ((path, page), svg_path) in GIFS.into_iter().zip(SVGS) {
            let colours = if page == PAGE_DARK {
                splash::DARK
            } else {
                splash::light()
            };
            let still = still_of(&std::fs::read_to_string(readme_file(svg_path)).unwrap());
            let data = std::fs::read(readme_file(path)).unwrap();
            let gif = Gif::parse(&data).unwrap();
            assert_eq!(gif.plays(), 0, "{path} plays for ever");
            let size = gif.size();
            assert_eq!(
                (usize::try_from(size.width), usize::try_from(size.height)),
                (Ok(cw), Ok(ch)),
                "{path}"
            );
            let mut scratch = vec![0u8; gif.scratch_len()];
            let mut runs: Vec<(bool, u32)> = Vec::new();
            let (mut first, mut last): (Option<Vec<u8>>, Vec<u8>) = (None, Vec::new());
            let (mut busiest, mut least) = (Moved(0), Moved(usize::MAX));
            gif.decode(&mut scratch, |f| {
                least = least.min(moved_off_block(f.rgba, &still));
                if first.is_none() {
                    first = Some(f.rgba.to_vec());
                } else {
                    busiest = busiest.max(moved_off_block(f.rgba, &last));
                }
                last.clear();
                last.extend_from_slice(f.rgba);
                let amber = share_near(f.rgba, inner, colours.amber);
                assert!(
                    amber >= 0.5 || amber == 0.0,
                    "{path}: a frame neither shows nor hides the block, {amber}"
                );
                let shown = amber >= 0.5;
                match runs.last_mut() {
                    Some((s, ms)) if *s == shown => *ms += f.gap_ms,
                    _ => runs.push((shown, f.gap_ms)),
                }
                true
            })
            .unwrap();
            let join = moved_off_block(&last, &first.unwrap());
            println!(
                "{path}: {} blink runs; the plasma off the block, at least {} pixels a frame; \
                 the wrap moves {}, the busiest step {}",
                runs.len(),
                least.0,
                join.0,
                busiest.0
            );
            assert!(least > Moved(0), "{path}: a frame is the bare logo");
            assert!(join > Moved(0), "{path}: the wrap moves nothing");
            assert!(
                join <= busiest,
                "{path}: the wrap jumps, {join:?} of {busiest:?}"
            );
            let total: u32 = runs.iter().map(|r| r.1).sum();
            let Cycles(n) = cycles(Duration::from_millis(total.into()), on, off);
            let mut want = Vec::new();
            for _ in 0..n {
                want.push((true, u32::try_from(on.as_millis()).unwrap()));
                want.push((false, u32::try_from(off.as_millis()).unwrap()));
            }
            assert_eq!(runs, want, "{path}: the default blink's phases");
        }
    }

    /// The octets of base64 `text`.
    fn unbase64(text: &str) -> Vec<u8> {
        let mut b = baud_domain::Base64::NEW;
        let mut out = vec![0u8; text.len() / 4 * 3 + 3];
        let fed = b.feed(text.as_bytes(), &mut out).expect("base64");
        assert_eq!(fed.read, text.len(), "all of it read");
        out.truncate(fed.wrote);
        out.extend_from_slice(b.finish().expect("base64").as_slice());
        out
    }

    /// The text of `tag`'s attribute `name`.
    fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
        let at = tag
            .find(&format!(" {name}=\""))
            .unwrap_or_else(|| panic!("{name} in {tag:.80}"));
        let rest = &tag[at + name.len() + 3..];
        &rest[..rest.find('"').expect("a closing quote")]
    }

    /// Hundredths from an SMIL clock value of whole hundredths, `4.50s`.
    fn clock(v: &str) -> Hundredths {
        let (s, cs) = v
            .strip_suffix('s')
            .and_then(|v| v.split_once('.'))
            .expect("seconds and hundredths");
        let at = s.parse::<u16>().expect("seconds") * 100 + cs.parse::<u16>().expect("hundredths");
        Hundredths(at)
    }

    /// **The README SVGs repeat at the GIFs' own times**
    /// (`logo-{dark,light}.svg`): nothing in either can run or fetch -- no
    /// script, every image a `data:` one; a base image, 840 by 580, the bare
    /// logo with the block shown; over it frame images, each hidden until
    /// its time and shown once a repeat by one discrete SMIL animation
    /// (`[[source: smil-animation]]`), repeating for ever over the GIF's
    /// length, from one of the film's frame times to the next, on the GIF's
    /// own grid; and from the first to the last no gap between them, so no
    /// moment is the bare logo and the wrap is one more step. It ties each
    /// SVG to its GIF by the frame times only, not by the frames' content;
    /// `baud-pictures logo --check` is what holds the content.
    #[test]
    fn the_readme_svgs_repeat_at_the_gifs_times() {
        let (cw, ch) = canvas_size();
        let inner = block_on_canvas(2.0);
        for (svg_path, (gif_path, page)) in SVGS.iter().zip(GIFS) {
            let colours = if page == PAGE_DARK {
                splash::DARK
            } else {
                splash::light()
            };
            let text = std::fs::read_to_string(readme_file(svg_path)).unwrap();
            for no in ["<script", "<set ", "<foreignObject", "url("] {
                assert!(!text.contains(no), "{svg_path}: {no}");
            }
            let hrefs: Vec<&str> = text.split(" href=\"").skip(1).collect();
            assert!(
                hrefs.iter().all(|h| h.starts_with("data:image/")),
                "{svg_path}: an image from elsewhere"
            );
            // The GIF's times: each frame's start, and its length.
            let data = std::fs::read(readme_file(gif_path)).unwrap();
            let gif = Gif::parse(&data).unwrap();
            let mut scratch = vec![0u8; gif.scratch_len()];
            let (mut starts, mut total) = (Vec::new(), 0u32);
            gif.decode(&mut scratch, |f| {
                starts.push(total);
                total += f.gap_ms / 10;
                true
            })
            .unwrap();
            let still = still_of(&text);
            assert_eq!(still.len(), cw * ch * 4, "{svg_path}: the base's size");
            assert!(
                share_near(&still, inner, colours.amber) >= 0.5,
                "{svg_path}: the block is shown in the bare logo"
            );
            let tags: Vec<&str> = text.split("<image ").skip(2).collect();
            let mut shows = Vec::new();
            for tag in &tags {
                assert_eq!(
                    attr(tag, "visibility"),
                    "hidden",
                    "{svg_path}: hidden until its time"
                );
                assert!(
                    attr(tag, "href").starts_with("data:image/webp;base64,"),
                    "{svg_path}"
                );
                let anim: Vec<&str> = tag.split("<animate ").skip(1).collect();
                assert_eq!(anim.len(), 1, "{svg_path}: one animation a frame");
                let a = anim[0];
                assert!(a.starts_with("attributeName=\"visibility\""), "{svg_path}");
                assert_eq!(attr(a, "calcMode"), "discrete", "{svg_path}");
                assert_eq!(attr(a, "repeatCount"), "indefinite", "{svg_path}");
                assert_eq!(
                    u32::from(clock(attr(a, "dur")).0),
                    total,
                    "{svg_path}: it repeats over the GIF's length"
                );
                let values: Vec<&str> = attr(a, "values").split(';').collect();
                let keys: Vec<u32> = attr(a, "keyTimes")
                    .split(';')
                    .map(|k| {
                        let at = k.parse::<f64>().expect("a time") * f64::from(total);
                        u32::try_from(at.round() as i64).expect("a proportion of the loop")
                    })
                    .collect();
                assert_eq!(values.len(), keys.len(), "{svg_path}: a time a value");
                assert_eq!(keys[0], 0, "{svg_path}: SMIL's first discrete time");
                let at = |v: &str| values.iter().position(|x| *x == v);
                let on = keys[at("visible").expect("shown")];
                let off = at("hidden")
                    .and_then(|_| values.iter().rposition(|x| *x == "hidden"))
                    .filter(|&i| i > at("visible").expect("shown"))
                    .map_or(total, |i| keys[i]);
                shows.push((on, off));
            }
            assert!(
                shows.len() > 100,
                "{svg_path}: the plasma moves, {} frames",
                shows.len()
            );
            for w in shows.windows(2) {
                assert_eq!(w[0].1, w[1].0, "{svg_path}: a gap or an overlap, {w:?}");
            }
            // The film's grid: a frame every 3 hundredths, 4 closing each
            // 25 hundredths' phase.
            let grid = |cs: u32| cs % 25 <= 21 && (cs % 25).is_multiple_of(3);
            for &(on, off) in &shows {
                assert!(on < off, "{svg_path}: a frame from {on} to {off} cs");
                assert!(
                    grid(on) && grid(off),
                    "{svg_path}: {on} to {off} cs off the grid"
                );
            }
            assert!(starts.iter().all(|&s| grid(s)), "{gif_path}: off the grid");
            assert_eq!(shows[0].0, 0, "{svg_path}: a frame from the first moment");
            assert_eq!(
                shows[shows.len() - 1].1,
                total,
                "{svg_path}: a frame to the last moment"
            );
        }
    }
}
