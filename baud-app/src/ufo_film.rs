//! **The UFO logo, filmed without a window**: the lockup the launch splash
//! holds, `baud ❯ █`, drawn as the README's logo film draws it
//! ([`crate::logo_film`]), with the ufo trail's saucer in amber threading
//! the letters, in front of them and behind them: it hovers, tilts and
//! darts from gap to counter, lights the `b` with its beam, beams the block
//! up, streaks off left behind the word, comes back from the right, sets
//! the block down and dips home behind the word as the loop wraps.
//! `tools/baud-pictures logo --ufo` runs
//! [`film_the_ufo_logo`] for each half and encodes what it writes into
//! `assets/demo/ufo-logo-dark.gif` and `ufo-logo-light.gif`; nothing opens a
//! window.
//!
//! **What draws what.** The lockup is the CPU renderer's, the splash's held
//! records, drawn twice, the block shown and the block hidden, the block
//! taking the caret's phase at each frame's time as the logo film's does.
//! The saucer and its beam are the ufo trail's own shapes,
//! `baud_render::ufo`'s body and beam posed where the flight says, laid
//! over the lockup in the amber at their light: the beam over the letters
//! as the plasma's light reaches them from either side, and the body
//! hidden behind the letters by as much as the flight puts it behind them.
//! Where the saucer goes, how it leans, its beams' strengths and the block
//! it lifts are `tools/baud-pictures`'s flight (`ufo.rs` there), each frame
//! a line this film reads.

use crate::caret::Caret;
use crate::logo_film::{
    self, CANVAS, Cycles, block_on_canvas, canvas_size, cycles, default_caret, half, hundredths,
    on_stage, path_var, phase_over,
};
use baud_render::splash;
use baud_render::ufo::{self, Pose};
use baud_render::{Argb, CursorPhase, Pt, Rect};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// **One frame of the flight**, as `baud-pictures`'s line gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shot {
    /// Hundredths into the loop.
    pub(crate) cs: u32,
    /// The saucer's centre on the canvas.
    pub(crate) at: Pt,
    /// Its bank, radians, positive leaning right.
    pub(crate) bank: f32,
    /// How much of it is in front of the letters.
    pub(crate) front: f32,
    /// The free beam's strength, and the beam onto the block's.
    pub(crate) free: f32,
    pub(crate) onto_block: f32,
    /// Whether the block is away from its place, how far it has risen into
    /// the saucer, and its centre across as it rises.
    pub(crate) away: bool,
    pub(crate) lift: f32,
    pub(crate) lift_x: f32,
    /// The rim lights' turns, and the frame's time on the planner's clock,
    /// seconds, which the beam's rings move by.
    pub(crate) rim: f32,
    pub(crate) t: f32,
}

/// **The flight's lines**: the saucer's cell, every frame's shot, and the
/// time the loop wraps at.
pub(crate) fn flight(text: &str) -> (f32, Vec<Shot>, Duration) {
    let (mut cell, mut shots, mut wraps) = (None, Vec::new(), None);
    for l in text.lines() {
        let v: Vec<&str> = l.split_whitespace().collect();
        let num = |i: usize| -> f32 { v[i].parse().unwrap_or_else(|_| panic!("a number: {l}")) };
        match v.first() {
            Some(&"U") => cell = Some(num(1)),
            Some(&"F") => {
                assert_eq!(v.len(), 14, "a frame's thirteen numbers: {l}");
                assert_eq!(v[1].parse::<usize>().ok(), Some(shots.len()), "{l}");
                shots.push(Shot {
                    cs: v[2].parse().expect("hundredths"),
                    at: Pt {
                        x: num(3),
                        y: num(4),
                    },
                    bank: num(5),
                    front: num(6),
                    free: num(7),
                    onto_block: num(8),
                    away: match v[9] {
                        "0" => false,
                        "1" => true,
                        other => panic!("away is {other}: {l}"),
                    },
                    lift: num(10),
                    lift_x: num(11),
                    rim: num(12),
                    t: num(13),
                });
            }
            Some(&"E") => {
                wraps = Some(Duration::from_millis(
                    v[1].parse::<u64>().expect("hundredths") * 10,
                ));
            }
            _ => panic!("a line of the flight: {l}"),
        }
    }
    (
        cell.expect("the saucer's cell"),
        shots,
        wraps.expect("the time the loop wraps at"),
    )
}

/// **A channel's coverage by the lockup's ink**, each pixel of an RGB
/// canvas: how far it is from the `page` toward the nearer ink, 0 to 1.
fn coverage(bare: &[u8], page: [f32; 3], inks: [[f32; 3]; 2]) -> Vec<f32> {
    let dist = |a: [f32; 3], b: [f32; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    bare.as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            let q = p.map(f32::from);
            inks.iter()
                .map(|&k| (dist(q, page) / dist(k, page).max(1.0)).clamp(0.0, 1.0))
                .fold(0.0, f32::max)
        })
        .collect()
}

fn rgb(c: Argb) -> [u8; 3] {
    [c.r(), c.g(), c.b()].map(|v| v.get())
}

/// **A count of a canvas's pixels**: a side of it, or how many of it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Pixels(pub(crate) usize);

/// **What every frame is drawn over**: the lockup with the block shown and
/// with it hidden, each one's ink coverage, the block's box on the canvas,
/// the saucer's colour and the canvas's size.
pub(crate) struct Stage {
    on: Vec<u8>,
    off: Vec<u8>,
    cov_on: Vec<f32>,
    cov_off: Vec<f32>,
    block: Rect,
    amber: [u8; 3],
    w: Pixels,
    h: Pixels,
}

impl Stage {
    /// The stage of the half `page`, its word `ink` and the `amber`, from
    /// the two bare canvases.
    pub(crate) fn new(on: Vec<u8>, off: Vec<u8>, (page, ink, amber): (Argb, Argb, Argb)) -> Stage {
        let f = |c: Argb| rgb(c).map(f32::from);
        let inks = [f(ink), f(amber)];
        let [x0, y0, x1, y1] = block_on_canvas(0.0).map(|v| v as f32);
        let (w, h) = canvas_size();
        assert_eq!(
            (on.len(), off.len()),
            (w * h * 3, w * h * 3),
            "two canvases"
        );
        Stage {
            cov_on: coverage(&on, f(page), inks),
            cov_off: coverage(&off, f(page), inks),
            on,
            off,
            block: Rect {
                left: x0,
                top: y0,
                right: x1,
                bottom: y1,
            },
            amber: rgb(amber),
            w: Pixels(w),
            h: Pixels(h),
        }
    }
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let k = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

fn mix(a: f32, b: f32, s: f32) -> f32 {
    a + (b - a) * s
}

/// A canvas index range from `lo` to `hi` pixels, cut to `0..n`.
fn span(lo: f32, hi: f32, Pixels(n): Pixels) -> core::ops::Range<usize> {
    let at = |v: f32| usize::try_from(v.clamp(0.0, n as f32) as i64).unwrap_or(0);
    at(lo)..at(hi)
}

/// **One frame of the film**: `shot` of the flight on cells `cell` tall,
/// the block at the caret's `phase` unless it is away, as RGB.
pub(crate) fn draw(stage: &Stage, shot: &Shot, cell: f32, phase: CursorPhase) -> Vec<u8> {
    let c = cell;
    let t = shot.t;
    let shown = phase == CursorPhase::SHOWN && !shot.away;
    let (bare, cov) = match shown {
        true => (&stage.on, &stage.cov_on),
        false => (&stage.off, &stage.cov_off),
    };
    let mut buf = bare.clone();
    let amber = stage.amber.map(f32::from);
    let paint = |buf: &mut [u8], i: usize, a: f32| {
        for (q, k) in amber.iter().enumerate() {
            let b = f32::from(buf[3 * i + q]);
            buf[3 * i + q] =
                u8::try_from((b + (k - b) * a).round().clamp(0.0, 255.0) as i64).unwrap_or(0);
        }
    };
    let (Pixels(w), Pixels(h)) = (stage.w, stage.h);
    let block = stage.block;
    if shot.away {
        // Up into the saucer, shrinking to 0.3 of itself, fading as it goes.
        let k = 1.0 - 0.7 * shot.lift;
        let (bw, bh) = (
            (block.right - block.left) * k,
            (block.bottom - block.top) * k,
        );
        let cy = mix(
            (block.top + block.bottom) * 0.5,
            shot.at.y + 0.1 * c,
            shot.lift,
        );
        let alpha = 1.0 - smooth(0.55, 1.0, shot.lift);
        let cx = shot.lift_x;
        if alpha > 0.0 {
            for y in span(cy - bh / 2.0, cy + bh / 2.0, Pixels(h)) {
                for x in span(cx - bw / 2.0, cx + bw / 2.0, Pixels(w)) {
                    paint(&mut buf, y * w + x, alpha);
                }
            }
        }
    }
    let pose = Pose::posed(shot.at, shot.bank, 1.0, shot.rim);
    let at = shot.at;
    let free_hole = Rect {
        left: at.x - 0.25 * c,
        top: at.y + 1.5 * c,
        right: at.x + 0.25 * c,
        bottom: at.y + 2.5 * c,
    };
    let reach = 2.6 * c;
    for y in span((at.y - 0.8 * c).floor(), (at.y + reach).ceil(), Pixels(h)) {
        for x in span((at.x - reach).floor(), (at.x + reach).ceil(), Pixels(w)) {
            let q = Pt {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            };
            let mut beam = 0.0f32;
            if shot.free > 0.0 {
                beam = beam.max(shot.free * ufo::beam_at(q, &pose, free_hole, c, t));
            }
            if shot.onto_block > 0.0 {
                beam = beam.max(shot.onto_block * ufo::beam_at(q, &pose, block, c, t));
            }
            let body = ufo::body_at(q, &pose, c).clamp(0.0, 1.0);
            if beam <= 0.0 && body <= 0.0 {
                continue;
            }
            let i = y * w + x;
            // The light reaches the letters from either side, as the
            // plasma's does; only the saucer's body hides behind them.
            let seen = shot.front + (1.0 - shot.front) * (1.0 - cov[i]);
            if beam > 0.0 {
                paint(&mut buf, i, beam.clamp(0.0, 1.0));
            }
            if body > 0.0 {
                paint(&mut buf, i, (body * seen).clamp(0.0, 1.0));
            }
        }
    }
    buf
}

/// **The lockup bare, the block shown and hidden**, on the half's stage.
fn bare_canvases(theme: baud_render::Theme) -> (Vec<u8>, Vec<u8>) {
    on_stage(theme, |s| {
        s.r.set_splash_cursor(CursorPhase::SHOWN);
        let on = s.draw();
        s.r.set_splash_cursor(CursorPhase::HIDDEN);
        (on, s.draw())
    })
}

/// The caret's phase over each of `shots`, the last ending as the loop
/// wraps, and each one's delay; the blink's phases and its whole cycles.
fn phases(shots: &[Shot], wraps: Duration) -> (Vec<(CursorPhase, u16)>, Duration, Duration, u32) {
    let epoch = Instant::now();
    let (mut caret, on, off): (Caret<Instant>, _, _) = default_caret(epoch);
    let Cycles(n) = cycles(wraps, on, off);
    let at = |s: &Shot| Duration::from_millis(u64::from(s.cs) * 10);
    let mut out = Vec::new();
    for (i, s) in shots.iter().enumerate() {
        let end = shots.get(i + 1).map_or(wraps, at);
        assert!(end > at(s), "the frames run forward");
        out.push((
            phase_over(&mut caret, epoch, at(s), end),
            hundredths(end - at(s)).0,
        ));
    }
    (out, on, off, n)
}

/// **The UFO logo, one half**: `BAUD_LOGO_HALF` (`dark` or `light`) into
/// `BAUD_LOGO_OUT` -- `frames.rgb`, every frame's canvas as RGB one after
/// another, `still.rgb`, the bare logo with the block shown, and
/// `film.json`, each frame's delay in hundredths, the canvas's size, the
/// blink's phases and cycles, the block's box, the word's and the page.
/// Each frame is `BAUD_UFO_FLIGHT`'s, the flight's line, drawn over the
/// lockup at the caret's phase at the frame's time.
#[test]
#[ignore = "a tool: tools/baud-pictures logo --ufo runs it"]
fn film_the_ufo_logo() {
    let out = path_var("BAUD_LOGO_OUT");
    let (half, theme, page, ink, amber) = half();
    let text = std::fs::read_to_string(path_var("BAUD_UFO_FLIGHT")).expect("the flight");
    let (cell, shots, wraps) = flight(&text);
    let (phases, on, off, n) = phases(&shots, wraps);
    let (bare_on, bare_off) = bare_canvases(theme);
    std::fs::write(out.join("still.rgb"), &bare_on).expect("still.rgb");
    let stage = Stage::new(bare_on, bare_off, (page, ink, amber));
    let mut file =
        std::io::BufWriter::new(std::fs::File::create(out.join("frames.rgb")).expect("frames.rgb"));
    for (shot, (phase, _)) in shots.iter().zip(&phases) {
        let f = draw(&stage, shot, cell, *phase);
        std::io::Write::write_all(&mut file, &f).expect("a frame");
    }
    std::io::Write::flush(&mut file).expect("frames.rgb");
    let delays: Vec<String> = phases.iter().map(|p| p.1.to_string()).collect();
    let (cw, ch) = canvas_size();
    let [x0, y0, x1, y1] = block_on_canvas(0.0);
    let px = |v: f32, from: f32| ((v - from) * logo_film::SCALE).round();
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
         \"page\": \"#{:06X}\"}}",
        shots.len(),
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
    use baud_render::Theme;

    /// Shots of the flight: home behind the word, out in front over the `b`
    /// with its beam on, and three of the abduction, the block rising, gone
    /// and setting down.
    const LINES: &str = "U 56.0000\n\
        F 0 0 360.0000 288.0000 0.000000 0.0000 0.0000 0.0000 0 0.0000 587.5000 0.00000 0.00000\n\
        F 1 3 258.0000 172.0000 -0.080000 1.0000 1.0000 0.0000 0 0.0000 587.5000 1.75000 2.18750\n\
        F 2 600 587.5000 179.0000 0.000000 1.0000 0.0000 1.0000 1 0.3304 587.5000 5.10000 6.00000\n\
        F 3 694 300.0000 290.0000 -0.320000 0.0000 0.0000 0.0000 1 1.0000 587.5000 7.37500 6.93750\n\
        F 4 850 587.5000 179.0000 0.000000 1.0000 0.0000 1.0000 1 0.5000 587.5000 8.50000 8.50000\n\
        E 1000\n";

    /// **The saucer is drawn where its line puts it**: shots with no beam
    /// and the block gone, clear of the letters, level and banked either
    /// way, at fractions of a pixel; in each frame the amber's weight, read
    /// back off the pixels against the bare lockup, has its centroid within
    /// a pixel of the body's own coverage centroid with the saucer posed at
    /// the line's centre -- the trail's body sampled at the same pixels, its
    /// dome lifting that centroid about a pixel over the centre. It breaks
    /// on a film that draws the saucer anywhere but where the flight says:
    /// an offset, the axes swapped, another frame's centre.
    #[test]
    fn the_saucer_is_drawn_where_its_line_puts_it() {
        let lines = "U 56.0000\n\
            F 0 694 300.0000 159.0000 -0.320000 1.0000 0.0000 0.0000 1 1.0000 587.5000 7.37500 6.93750\n\
            F 1 700 150.3000 90.7000 0.000000 1.0000 0.0000 0.0000 1 1.0000 587.5000 7.43750 7.00000\n\
            F 2 706 700.6000 120.2000 0.160000 1.0000 0.0000 0.0000 1 1.0000 587.5000 7.50000 7.06250\n\
            E 1000\n";
        let (cell, shots, _) = flight(lines);
        let colours = (Argb(0xFF0D_1117), splash::DARK.wordmark, splash::DARK.amber);
        let mut theme = Theme::dark();
        theme.bg = colours.0;
        theme.bg_alpha = baud_domain::Chan::from_u8(0xFF);
        let (on, off) = bare_canvases(theme);
        let stage = Stage::new(on, off, colours);
        let amber = rgb(colours.2).map(f32::from);
        for shot in &shots {
            let f = draw(&stage, shot, cell, CursorPhase::SHOWN);
            let (mut w, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64);
            for (i, (p, b)) in f
                .as_chunks::<3>()
                .0
                .iter()
                .zip(stage.off.as_chunks::<3>().0)
                .enumerate()
            {
                // The channel the amber moves most off this ground.
                let (q, k) = (0..3)
                    .map(|q| (q, amber[q] - f32::from(b[q])))
                    .max_by(|x, y| x.1.abs().total_cmp(&y.1.abs()))
                    .expect("three channels");
                let a = f64::from(((f32::from(p[q]) - f32::from(b[q])) / k).clamp(0.0, 1.0));
                if a > 0.0 {
                    let (x, y) = ((i % stage.w.0) as f64 + 0.5, (i / stage.w.0) as f64 + 0.5);
                    w += a;
                    sx += a * x;
                    sy += a * y;
                }
            }
            assert!(w > 100.0, "{} cs: the saucer draws {w}", shot.cs);
            let (cx, cy) = (sx / w, sy / w);
            // The body's own centroid, posed at the line's centre.
            let pose = Pose::posed(shot.at, shot.bank, 1.0, shot.rim);
            let (mut mw, mut mx, mut my) = (0.0f64, 0.0f64, 0.0f64);
            for i in 0..stage.w.0 * stage.h.0 {
                let q = Pt {
                    x: (i % stage.w.0) as f32 + 0.5,
                    y: (i / stage.w.0) as f32 + 0.5,
                };
                let a = f64::from(ufo::body_at(q, &pose, cell).clamp(0.0, 1.0));
                mw += a;
                mx += a * f64::from(q.x);
                my += a * f64::from(q.y);
            }
            let (ex, ey) = (mx / mw, my / mw);
            eprintln!(
                "{} cs: the pixels' centroid ({cx:.3}, {cy:.3}), the body's at the line ({ex:.3}, {ey:.3}), the line ({}, {})",
                shot.cs, shot.at.x, shot.at.y
            );
            assert!(
                (cx - ex).hypot(cy - ey) <= 1.0,
                "{} cs: the saucer's centroid ({cx:.2}, {cy:.2}), the body's at its line ({ex:.2}, {ey:.2})",
                shot.cs
            );
        }
    }

    /// **Lines of the flight at the planner's poses**
    /// (`tools/baud-pictures/src/ufo.rs`, whose `tests/ufo.rs` holds all
    /// 320 to the designed poses): home behind the word; behind the `b`'s
    /// counter; over the `b`, its beam on; behind the `a`'s counter; arriving
    /// in the `u`'s opening, in front; behind the `d`'s counter; over the
    /// block, its beam on; gone off the left, the block away; the servo
    /// ring; mid-dart home behind the word -- the film's frames 5, 35, 75,
    /// 110, 117, 128, 145, 220, 240 and 300.
    const POSES: &str = "U 56.0000\n\
        F 0 15 360.0000 289.9134 0.006810 0.0000 0.0000 0.0000 0 0.0000 587.5000 0.12500 0.15625\n\
        F 1 109 259.0000 293.9134 0.030033 0.0000 0.0000 0.0000 0 0.0000 587.5000 0.87500 1.09375\n\
        F 2 234 258.0000 170.0866 0.090916 1.0000 0.7986 0.0000 0 0.0000 587.5000 1.87500 2.34375\n\
        F 3 343 326.0000 306.2834 0.258425 0.0000 0.0000 0.0000 0 0.0000 587.5000 2.75000 3.43750\n\
        F 4 365 395.0622 276.3751 0.279253 1.0000 0.0000 0.0000 0 0.0000 587.5000 2.92500 3.65625\n\
        F 5 400 465.0000 291.0204 0.268187 0.0000 0.0000 0.0000 0 0.0000 587.5000 3.20000 4.00000\n\
        F 6 453 587.5000 175.3806 0.024620 1.0000 0.0000 0.0000 0 0.0000 587.5000 3.62500 4.53125\n\
        F 7 687 -84.0000 285.0000 -0.254570 0.0000 0.0000 0.0000 1 1.0000 587.5000 7.50000 6.87500\n\
        F 8 750 586.3061 180.0000 0.000000 1.0000 0.0000 0.0000 1 1.0000 587.5000 8.00000 7.50000\n\
        F 9 937 369.7541 283.3695 -0.261799 0.0000 0.0000 0.0000 0 0.0000 587.5000 9.50000 9.37500\n\
        E 1000\n";

    /// A file of the UFO logo: the committed one, or under `BAUD_UFO_FILES`
    /// the one `baud-pictures logo --ufo --out` wrote there.
    fn ufo_file(name: &str) -> String {
        match std::env::var("BAUD_UFO_FILES") {
            Ok(dir) => format!("{dir}/{name}"),
            Err(_) => format!("{}/../assets/demo/{name}", env!("CARGO_MANIFEST_DIR")),
        }
    }

    /// The pixels of two RGB canvases more than [`OFF`] apart in a channel.
    fn apart(a: &[u8], b: &[u8]) -> Pixels {
        Pixels(
            a.as_chunks::<3>()
                .0
                .iter()
                .zip(b.as_chunks::<3>().0)
                .filter(|(p, q)| (0..3).any(|j| p[j].abs_diff(q[j]) > OFF))
                .count(),
        )
    }

    /// How far the GIF's palette may move a pixel of the film: the most
    /// the haze moves the page, 17 levels, with room for the saucer's
    /// gradients.
    const OFF: u8 = 32;

    /// **The UFO logos fly the planner's poses**
    /// (`assets/demo/ufo-logo-{dark,light}.gif`, decoded by baud's own GIF
    /// decoder): at each of [`POSES`], the GIF's frame at that time is the
    /// frame this film draws at the pose, the block at the default blink's
    /// phase, all but 50 pixels within [`OFF`] levels -- the saucer in
    /// front of the letters or hidden behind them as the pose says -- while
    /// the same pose drawn 6 pixels to the right is 100 or more off, and a
    /// pose behind the letters drawn in front of them is too, so the
    /// comparison sees the saucer's place and its depth. It breaks on a
    /// GIF whose saucer is not where the planner puts it, at its time.
    #[test]
    fn the_ufo_logos_fly_their_poses() {
        let (cell, shots, _) = flight(POSES);
        for (name, dark) in [("ufo-logo-dark.gif", true), ("ufo-logo-light.gif", false)] {
            let (page, colours, mut theme) = match dark {
                true => (Argb(0xFF0D_1117), splash::DARK, Theme::dark()),
                false => (Argb(0xFFFF_FFFF), splash::light(), Theme::light()),
            };
            theme.bg = page;
            theme.bg_alpha = baud_domain::Chan::from_u8(0xFF);
            let (on, off) = bare_canvases(theme);
            let stage = Stage::new(on, off, (page, colours.wordmark, colours.amber));
            let data = std::fs::read(ufo_file(name)).unwrap();
            let gif = baud_domain::gif::Gif::parse(&data).unwrap();
            let mut scratch = vec![0u8; gif.scratch_len()];
            let mut frames: Vec<(u32, u32, Vec<u8>)> = Vec::new();
            let mut at = 0u32;
            gif.decode(&mut scratch, |f| {
                let rgb = f
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[1], p[2]]);
                frames.push((at, f.gap_ms / 10, rgb.collect()));
                at += f.gap_ms / 10;
                true
            })
            .unwrap();
            assert_eq!(at, 1000, "{name} lasts ten seconds");
            for shot in &shots {
                let (_, _, got) = frames
                    .iter()
                    .find(|(s, d, _)| (*s..s + d).contains(&shot.cs))
                    .expect("a frame at the time");
                let phase = match (shot.cs / 25).is_multiple_of(2) {
                    true => CursorPhase::SHOWN,
                    false => CursorPhase::HIDDEN,
                };
                let Pixels(here) = apart(got, &draw(&stage, shot, cell, phase));
                let moved = Shot {
                    at: Pt {
                        x: shot.at.x + 6.0,
                        y: shot.at.y,
                    },
                    ..*shot
                };
                let flipped = Shot {
                    front: 1.0 - shot.front,
                    ..*shot
                };
                let Pixels(aside) = apart(got, &draw(&stage, &moved, cell, phase));
                let Pixels(other) = apart(got, &draw(&stage, &flipped, cell, phase));
                eprintln!(
                    "{name} at {} cs: {here} pixels off the pose, {aside} 6 px aside, {other} on the other side",
                    shot.cs
                );
                assert!(here <= 50, "{name} at {} cs: {here} pixels off", shot.cs);
                let on_canvas = shot.at.x > 0.0 && shot.at.x < stage.w.0 as f32;
                if on_canvas {
                    assert!(aside >= 100, "{name} at {} cs: aside {aside}", shot.cs);
                }
                if on_canvas && shot.front == 0.0 {
                    assert!(other >= 100, "{name} at {} cs: other {other}", shot.cs);
                }
            }
            println!(
                "ufo-logo-{}: {} poses found in their frames",
                if dark { "dark" } else { "light" },
                shots.len()
            );
        }
    }

    /// **The film is its inputs' alone**: two stages built from the
    /// renderer's two bare canvases, each drawing every shot of the flight
    /// at each blink phase, draw the same bytes, so a film made twice is the
    /// same film; and the shots differ from each other and from the bare
    /// lockup, so the comparison sees what they draw. It breaks on anything
    /// a frame reads besides its shot, its phase and its stage: a clock, a
    /// count of the frames drawn before, the threads.
    #[test]
    fn the_ufo_film_draws_the_same_frames_twice() {
        let (cell, shots, wraps) = flight(LINES);
        assert_eq!(
            (cell, shots.len(), wraps),
            (56.0, 5, Duration::from_secs(10))
        );
        let colours = (Argb(0xFF0D_1117), splash::DARK.wordmark, splash::DARK.amber);
        let mut theme = Theme::dark();
        theme.bg = colours.0;
        theme.bg_alpha = baud_domain::Chan::from_u8(0xFF);
        let stage = |_: usize| {
            let (on, off) = bare_canvases(theme.clone());
            Stage::new(on, off, colours)
        };
        let (a, b) = (stage(0), stage(1));
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for shot in &shots {
            for phase in [CursorPhase::SHOWN, CursorPhase::HIDDEN] {
                let f = draw(&a, shot, cell, phase);
                assert!(
                    f == draw(&b, shot, cell, phase),
                    "the frame at {} cs drawn twice",
                    shot.cs
                );
                assert!(f != a.on && f != a.off, "{} cs draws nothing", shot.cs);
                if phase == CursorPhase::SHOWN {
                    seen.push(f);
                }
            }
        }
        for (i, x) in seen.iter().enumerate() {
            for y in &seen[i + 1..] {
                assert!(x != y, "two shots drew one frame");
            }
        }
    }
}
