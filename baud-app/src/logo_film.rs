//! **The README's logo, filmed without a window**: the lockup the launch
//! splash holds, `baud ❯ █`, at twice its 300-unit size on GitHub's own
//! page colour, its block blinking as baud's cursor blinks by default, for
//! under five seconds, then resting shown. `scripts/make-readme-logo.py`
//! runs [`film_the_readme_logo`] for each half and encodes what it writes
//! into `assets/readme/logo-dark.gif` and `logo-light.gif`; nothing opens
//! a window.
//!
//! **A test module run from a script, as `film.rs` is**, and not an
//! example: the blink is the app's own, `crate::caret`'s plan for the
//! config's defaults, and the app is a binary, whose modules no example
//! can reach. So the logo blinks with whatever baud's default blink is.
//!
//! **Every pixel is the CPU renderer's** (`Renderer::render`): the lockup
//! is the splash's held records (`baud_render::splash`), and the block
//! takes the caret's phase through the cursor's own drawing
//! (`Renderer::set_splash_cursor`).
//!
//! **One frame a phase.** The film asks the caret for each phase at its
//! start, its middle and its last moment, and stops if they differ: a
//! default whose hidden phase animates needs more than one frame a phase,
//! which this film does not draw. A GIF's delays are hundredths of a
//! second, so each phase has to be whole hundredths.
//!
//! **Why it stops before five seconds.** WCAG 2.2's 2.2.2 Pause, Stop,
//! Hide (Level A) asks that blinking which starts by itself, lasts more
//! than five seconds and is shown beside other content can be paused,
//! stopped or hidden (`[[source: wcag22]]`). A README carries no control
//! of its own, so the film blinks the most whole cycles that end before
//! [`BOUND`] and rests on the block shown, played once.

use crate::caret::{Blinks, Caret, Cycle, Gate, NonBlinking};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::splash;
use baud_render::{
    Argb, Blend, CursorPhase, Damage, FaceRole, FaceStyle, Pt, PxSize, Renderer, SplashLook,
    SplashPhase, Stack, Target, Theme,
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

/// WCAG 2.2.2's five seconds: the blink ends before it.
const BOUND: Duration = Duration::from_secs(5);

/// The files the script writes, from this crate's directory.
const GIFS: [(&str, Argb); 2] = [
    ("../assets/readme/logo-dark.gif", PAGE_DARK),
    ("../assets/readme/logo-light.gif", PAGE_LIGHT),
];

/// A count of the blink's cycles, each a phase shown and one hidden.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Cycles(u32);

/// A GIF frame's delay, in hundredths of a second.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Hundredths(u16);

/// **The blink's whole cycles that end before [`BOUND`]**: the most `n`
/// with `n (on + off) < 5 s`.
fn cycles(on: Duration, off: Duration) -> Cycles {
    let n = (BOUND.as_nanos() - 1) / (on + off).as_nanos();
    Cycles(u32::try_from(n).expect("a count of cycles"))
}

/// `d` in hundredths of a second, which it has to be whole.
fn hundredths(d: Duration) -> Hundredths {
    assert_eq!(
        d.as_nanos() % 10_000_000,
        0,
        "a phase of {d:?} is not whole hundredths, which a GIF's delay is"
    );
    Hundredths(u16::try_from(d.as_millis() / 10).expect("a GIF's delay"))
}

/// One frame of the film: the caret's phase it shows and how long, in
/// hundredths.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Shot {
    phase: CursorPhase,
    cs: Hundredths,
}

/// **The film's frames**: each phase of [`cycles`] of the blink, the
/// caret's plan at its start, the same at its middle and its last moment,
/// then the frame it rests on, the next cycle's start, which is shown.
/// `caret` is fresh at `epoch`, and asked in time order.
fn shots(caret: &mut Caret<Instant>, epoch: Instant, on: Duration, off: Duration) -> Vec<Shot> {
    let edge = |k: u32| (on + off) * (k / 2) + on * (k % 2);
    let Cycles(n) = cycles(on, off);
    assert!(n > 0, "a cycle of {:?} is five seconds or more", on + off);
    let mut out = Vec::new();
    for k in 0..2 * n {
        let (a, b) = (edge(k), edge(k + 1));
        let phase = caret.plan(epoch + a, Blinks::Yes);
        for t in [a + (b - a) / 2, b - Duration::from_nanos(1)] {
            assert_eq!(
                caret.plan(epoch + t, Blinks::Yes),
                phase,
                "the blink moves within phase {k}; the film draws one frame a phase"
            );
        }
        out.push(Shot {
            phase,
            cs: hundredths(b - a),
        });
    }
    let rest = caret.plan(epoch + edge(2 * n), Blinks::Yes);
    assert_eq!(
        rest,
        CursorPhase::SHOWN,
        "the film rests on the block shown"
    );
    out.push(Shot {
        phase: rest,
        cs: hundredths(on),
    });
    out
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

/// How a canvas's pixels are laid out: the film's RGB or a decoder's RGBA.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layout {
    Rgb,
    Rgba,
}

/// How far a channel may move and still be the same pixel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Tolerance(u8);

/// **Whether two canvases differ only by the cursor**: every pixel
/// outside the block's box grown by 2 pixels, for its antialiased edge,
/// within `tolerance` in each channel.
fn only_the_cursor_moves(a: &[u8], b: &[u8], layout: Layout, tolerance: Tolerance) -> bool {
    let step = match layout {
        Layout::Rgb => 3,
        Layout::Rgba => 4,
    };
    let (w, h) = canvas_size();
    let g = block_on_canvas(-2.0);
    (0..h).all(|y| {
        (0..w).all(|x| {
            let i = (y * w + x) * step;
            let inside = (g[0]..g[2]).contains(&x) && (g[1]..g[3]).contains(&y);
            inside
                || a[i..i + 3]
                    .iter()
                    .zip(&b[i..i + 3])
                    .all(|(p, q)| p.abs_diff(*q) <= tolerance.0)
        })
    })
}

/// The canvas cut out of `frame`, `wide` a row, as RGB.
fn canvas(frame: &[u32], wide: Px, at: Pt) -> Vec<u8> {
    let wide = usize::try_from(wide.value()).expect("a width");
    let px = |v: f32| usize::try_from(v.round() as i64).expect("the canvas is on the frame");
    let (x0, y0) = (px(at.x + CANVAS[0] * SCALE), px(at.y + CANVAS[1] * SCALE));
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

/// **The README's logo, one half**: `BAUD_LOGO_HALF` (`dark` or `light`)
/// into `BAUD_LOGO_OUT` -- `frames.rgb`, every frame's canvas as RGB one
/// after another, and `film.json`, each frame's delay in hundredths, the
/// canvas's size, the blink's phases and cycles and the block's box.
#[test]
#[ignore = "a tool: scripts/make-readme-logo.py runs it"]
fn film_the_readme_logo() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
    let out = std::path::PathBuf::from(var("BAUD_LOGO_OUT"));
    let half = var("BAUD_LOGO_HALF");
    let (mut theme, page) = match half.as_str() {
        "dark" => (Theme::dark(), PAGE_DARK),
        "light" => (Theme::light(), PAGE_LIGHT),
        other => panic!("BAUD_LOGO_HALF is {other}, not dark or light"),
    };
    theme.bg = page;
    theme.bg_alpha = Chan::from_u8(0xFF);
    let epoch = Instant::now();
    let (mut caret, on, off) = default_caret(epoch);
    let film = shots(&mut caret, epoch, on, off);

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
    let (w, h) = r.pixel_size();
    let wide = usize::try_from(w.value()).expect("a width");
    let frame_wh = (w.value() as f32, h.value() as f32);
    let scale = splash::scale(frame_wh);
    assert_eq!(scale, SCALE, "the lockup at two pixels a unit");
    let at = splash::origin(frame_wh, scale);
    let mut owned = Owned::new(&WS);
    let mut term = Term::new(owned.storage(), &WS);
    let hide = b"\x1b[?25l";
    term.write_at(BytePos::ZERO, hide);
    term.feed(BytePos::ZERO, ReadLen::from_index(hide.len()));
    let mut px = vec![0u32; wide * usize::try_from(h.value()).expect("a height")];
    r.set_splash(Some(SplashLook {
        phase: SplashPhase::Hold,
    }));

    let mut frames =
        std::io::BufWriter::new(std::fs::File::create(out.join("frames.rgb")).expect("frames.rgb"));
    let mut first: Option<Vec<u8>> = None;
    let mut last: Option<Vec<u8>> = None;
    for s in &film {
        r.set_splash_cursor(s.phase);
        let mut target = Target::new(&mut px, w, w, h).expect("a target");
        r.render(&mut term, &mut target, Damage::Full);
        let f = canvas(&px, w, at);
        if last.is_none() {
            let [_, pr, pg, pb] = page.0.to_be_bytes();
            assert_eq!(f[..3], [pr, pg, pb], "the canvas's corner is the page");
        }
        assert!(
            last.as_ref() != Some(&f),
            "two frames running are the same picture"
        );
        let ground = first.get_or_insert_with(|| f.clone());
        assert!(
            only_the_cursor_moves(ground, &f, Layout::Rgb, Tolerance(0)),
            "a frame draws more than the cursor's change"
        );
        std::io::Write::write_all(&mut frames, &f).expect("a frame");
        last = Some(f);
    }
    std::io::Write::flush(&mut frames).expect("frames.rgb");
    assert!(first == last, "the film does not rest on its first frame");

    let delays: Vec<String> = film.iter().map(|s| s.cs.0.to_string()).collect();
    let (cw, ch) = canvas_size();
    let [x0, y0, x1, y1] = block_on_canvas(0.0);
    let mut json = String::new();
    write!(
        json,
        "{{\"half\": \"{half}\", \"width\": {cw}, \"height\": {ch}, \"frames\": {}, \
         \"delays_cs\": [{}], \"on_ms\": {}, \"off_ms\": {}, \"cycles\": {}, \
         \"block\": [{x0}, {y0}, {x1}, {y1}], \"page\": \"#{:06X}\"}}",
        film.len(),
        delays.join(", "),
        on.as_millis(),
        off.as_millis(),
        cycles(on, off).0,
        page.0 & 0xFF_FFFF,
    )
    .expect("a string");
    std::fs::write(out.join("film.json"), json).expect("film.json");
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::gif::Gif;

    /// How far gifski's palette, chosen frame by frame, moves a pixel the
    /// film draws the same in two frames: up to 27 levels on the dark
    /// half's glyph edges, none on the light's. The film itself holds the
    /// frames equal outside the cursor exactly (`film_the_readme_logo`).
    const PALETTE: Tolerance = Tolerance(32);

    /// **The blink ends before five seconds, by whole cycles**: at 250 ms a
    /// phase ten cycles end at exactly 5 s, which is not before it, so nine
    /// play; at 333 seven, 4.66 s.
    #[test]
    fn the_blink_plays_the_whole_cycles_before_five_seconds() {
        let ms = Duration::from_millis;
        assert_eq!(cycles(ms(250), ms(250)), Cycles(9));
        assert_eq!(cycles(ms(333), ms(333)), Cycles(7));
        assert_eq!(cycles(ms(2500), ms(2499)), Cycles(1));
        assert_eq!(cycles(ms(2500), ms(2500)), Cycles(0));
    }

    /// **The film is the default blink, one frame a phase**: shown and
    /// hidden by turns, each its phase's length, then the block shown.
    #[test]
    fn the_film_is_the_default_blink_then_shown() {
        let epoch = Instant::now();
        let (mut caret, on, off) = default_caret(epoch);
        let film = shots(&mut caret, epoch, on, off);
        let Cycles(n) = cycles(on, off);
        assert_eq!(film.len(), 2 * usize::try_from(n).unwrap() + 1);
        for (k, s) in film.iter().enumerate() {
            let (phase, len) = match k % 2 {
                0 => (CursorPhase::SHOWN, on),
                _ => (CursorPhase::HIDDEN, off),
            };
            assert_eq!(
                *s,
                Shot {
                    phase,
                    cs: hundredths(len)
                },
                "frame {k}"
            );
        }
    }

    /// Whether every pixel of the canvas `rgba`'s box `b` is within 24 of
    /// `c` in each channel.
    fn all_near(rgba: &[u8], b: [usize; 4], c: Argb) -> bool {
        let (w, _) = canvas_size();
        let [_, cr, cg, cb] = c.0.to_be_bytes();
        (b[1]..b[3]).all(|y| {
            (b[0]..b[2]).all(|x| {
                let p = &rgba[(y * w + x) * 4..][..3];
                p.iter()
                    .zip([cr, cg, cb])
                    .all(|(&a, b)| a.abs_diff(b) <= 24)
            })
        })
    }

    /// **The committed README logos blink the default blink and rest
    /// shown** (`assets/readme/logo-{dark,light}.gif`, decoded by baud's
    /// own GIF decoder): each plays once, 840 by 580; outside the block's
    /// box no frame differs from the first, so only the cursor moves; read
    /// off the block's pixels, shown and hidden alternate from shown, each
    /// run the default phase's length, the blink over before five seconds;
    /// and the last frame is the first, the logo with the block shown,
    /// which the file rests on.
    #[test]
    fn the_readme_logos_blink_the_default_and_rest_shown() {
        let epoch = Instant::now();
        let (_, on, off) = default_caret(epoch);
        let Cycles(n) = cycles(on, off);
        let mut want: Vec<(bool, u32)> = Vec::new();
        for _ in 0..n {
            want.push((true, u32::try_from(on.as_millis()).unwrap()));
            want.push((false, u32::try_from(off.as_millis()).unwrap()));
        }
        let (cw, ch) = canvas_size();
        let inner = block_on_canvas(2.0);
        for (path, page) in GIFS {
            let colours = if page == PAGE_DARK {
                splash::DARK
            } else {
                splash::LIGHT
            };
            let data = std::fs::read(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
            let gif = Gif::parse(&data).unwrap();
            assert_eq!(gif.plays(), 1, "{path} plays once");
            let size = gif.size();
            assert_eq!(
                (usize::try_from(size.width), usize::try_from(size.height)),
                (Ok(cw), Ok(ch)),
                "{path}"
            );
            let mut scratch = vec![0u8; gif.scratch_len()];
            let mut runs: Vec<(bool, u32)> = Vec::new();
            let mut first: Option<Vec<u8>> = None;
            let mut last = Vec::new();
            gif.decode(&mut scratch, |f| {
                let ground = first.get_or_insert_with(|| f.rgba.to_vec());
                assert!(
                    only_the_cursor_moves(ground, f.rgba, Layout::Rgba, PALETTE),
                    "{path}: a frame moves more than the cursor"
                );
                last.clear();
                last.extend_from_slice(f.rgba);
                let shown = all_near(f.rgba, inner, colours.amber);
                let hidden = all_near(f.rgba, inner, page);
                assert!(
                    shown != hidden,
                    "{path}: a frame neither shows nor hides the block"
                );
                match runs.last_mut() {
                    Some((s, ms)) if *s == shown => *ms += f.gap_ms,
                    _ => runs.push((shown, f.gap_ms)),
                }
                true
            })
            .unwrap();
            let (rest, blink) = runs.split_last().unwrap();
            println!("{path}: {blink:?}, then {rest:?} and still");
            assert!(rest.0, "{path}: the last frame hides the block");
            let first = first.unwrap();
            let (w, h) = canvas_size();
            let whole = (0..w * h * 4)
                .step_by(4)
                .all(|i| (0..3).all(|c| first[i + c].abs_diff(last[i + c]) <= PALETTE.0));
            assert!(whole, "{path}: the last frame is not the logo the first is");
            assert_eq!(blink, &want[..], "{path}: the default blink's phases");
            let ms: u32 = blink.iter().map(|r| r.1).sum();
            assert!(
                u128::from(ms) < BOUND.as_millis(),
                "{path}: it blinks for {ms} ms"
            );
        }
    }
}
