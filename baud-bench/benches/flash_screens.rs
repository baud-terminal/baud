//! `flash_screens`: how often a cursor trail's repeat pass draws nothing,
//! over the screens programs leave (note AL, option O2).
//!
//! **The question.** Above the size gate a repeat pass is capped against
//! every colour under it, and draws nothing while a ground's cap is not yet
//! scanned (two a frame), over more grounds than a frame reads, or over an
//! image under the text. On the glass a translucent ground is read over a
//! white and a black desktop as well as solid, so it counts three times.
//! This counts, on the default config, the passes each reason blanks.
//!
//! **The screens** are every `.raw` recording's last screen, replayed at its
//! capture size (a resize recording by its `.steps`), then shown the cursor
//! (`?25h`, the app draws no trail for a hidden one) and moved by the flash
//! census's seven motions (`benches/flash.rs`), from nine origins: the
//! first cell at the top, middle and foot of the screen and at its left,
//! middle and right, so the motions cross what the program drew. The
//! trail is the default, the plasma, driven by `baud-app`'s own
//! `trail.rs`; the renderer is the shipped one, its cap and its reasons
//! read after the styles are synced, as on every path. No pixel is drawn:
//! what blanks a pass is the cells, so one scale is enough, 1x.
//!
//! **The default config**: the dark theme dimmed to 0.70 (alpha 179) and
//! the light to 0.60 (153), `app_backgrounds = "dim"`, `pictures` opaque
//! and the floor at its default -- `Theme`'s own fields, `bg_alpha` set as
//! `half_theme` sets it -- and beside each the same theme opaque, the
//! control that says what the glass adds.
//!
//! **Cold and warm.** Each case runs on a fresh renderer, the first pass
//! over a screen in a session, and then again on the same renderer, its
//! scanned caps kept.
//!
//! One tab-separated line a case on stdout. Usage: `flash_screens [sizes]
//! [recordings]`, sizes such as `13,24,32` (points, 1x), recordings a comma
//! list of paths under the repository root or `all`.

#[allow(dead_code, unused_imports)]
#[path = "../../baud-app/src/trail.rs"]
mod trail;

/// `baud-app/src/bell.rs`'s `Moment`, as `benches/flash.rs` spells it.
mod bell {
    use baud_domain::HostTime;
    use core::time::Duration;
    use std::time::Instant;

    pub trait Moment: Copy + Ord {
        fn after(self, d: Duration) -> Self;
        fn elapsed_since(self, earlier: Self) -> Duration;
    }

    impl Moment for HostTime {
        fn after(self, d: Duration) -> HostTime {
            self.plus(d)
        }

        fn elapsed_since(self, earlier: HostTime) -> Duration {
            self.since(earlier).unwrap_or(Duration::ZERO)
        }
    }

    impl Moment for Instant {
        fn after(self, d: Duration) -> Instant {
            self + d
        }

        fn elapsed_since(self, earlier: Instant) -> Duration {
            self.saturating_duration_since(earlier)
        }
    }
}

/// `baud-app/src/caret.rs`'s `Owes`, the other item `trail.rs` names.
mod caret {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Owes {
        Frame,
        Nothing,
    }
}

use baud_core::{Owned, Term};
use baud_domain::{BytePos, CacheGeometry, Chan, Col, Cols, HostTime, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::effect::Effect;
use baud_render::{
    Blend, Held, Points, PxSize, Renderer, Resolved, Stack, Strength, StyleStamp, Tally, Theme,
    TrailLook,
};
use core::time::Duration;

/// The recordings, with the size each was captured at (the READMEs beside
/// them); a resize recording's sizes are its `.steps`.
const RECORDINGS: [(&str, u16, u16); 30] = [
    ("tests/recordings/ascii/asciiquarium.raw", 160, 45),
    ("tests/recordings/ascii/cat-scroll.raw", 200, 50),
    ("tests/recordings/ascii/git-log.raw", 200, 50),
    ("tests/recordings/ascii/less-pager.raw", 200, 50),
    ("tests/recordings/utf8/btop.raw", 160, 45),
    ("tests/recordings/utf8/cat-cjk.raw", 200, 50),
    ("tests/recordings/utf8/cava.raw", 200, 50),
    ("tests/recordings/utf8/less-cjk.raw", 200, 50),
    ("tests/recordings/utf8/vim-unicode.raw", 200, 50),
    ("tests/recordings/sixel/swirl.raw", 200, 50),
    ("tests/recordings/sixel/yazi-walk.raw", 200, 50),
    ("tests/recordings/kitty/grad-80x24-placeholder.raw", 80, 24),
    ("tests/panels/helix.raw", 200, 50),
    ("tests/panels/herdr-themes.raw", 200, 50),
    ("tests/panels/vimdiff.raw", 200, 50),
    ("tests/panels/vimhl.raw", 200, 50),
    ("tests/trail/helix-ge-gg.raw", 200, 50),
    ("tests/trail/helix-wheel.raw", 80, 24),
    ("tests/cursor/helix-exit.raw", 200, 50),
    ("tests/cursor/vim-exit.raw", 200, 50),
    ("tests/scheme/tmux-start.raw", 200, 50),
    ("tests/palette/ls-lsd-bat.raw", 200, 50),
    ("tests/reset/seq-reset.raw", 60, 20),
    ("tests/resize/drag.raw", 0, 0),
    ("tests/resize/fresh-burst.raw", 0, 0),
    ("tests/resize/fresh-drag.raw", 0, 0),
    ("tests/resize/fresh-zoom.raw", 0, 0),
    ("tests/resize/live-drag.raw", 0, 0),
    ("tests/resize/short.raw", 0, 0),
    ("tests/resize/zoom.raw", 0, 0),
];

/// The bytes of a recording and the size each run of them arrived at.
fn steps(root: &str, path: &str, size: Option<WinSize>) -> Vec<(WinSize, Vec<u8>)> {
    let raw = std::fs::read(format!("{root}/{path}")).unwrap_or_else(|e| panic!("{path}: {e}"));
    if let Some(ws) = size {
        return vec![(ws, raw)];
    }
    let text = std::fs::read_to_string(format!("{root}/{}", path.replace(".raw", ".steps")))
        .expect("steps");
    let mut out = Vec::new();
    let mut at = 0;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (size, len) = line.split_once(' ').expect("COLSxROWS LEN");
        let (c, r) = size.split_once('x').expect("COLSxROWS");
        let len: usize = len.parse().expect("a length");
        let ws = WinSize {
            ws_col: c.parse().expect("cols"),
            ws_row: r.parse().expect("rows"),
        };
        out.push((ws, raw[at..at + len].to_vec()));
        at += len;
    }
    assert_eq!(at, raw.len(), "{path}: the steps hold every byte");
    out
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for chunk in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
}

/// How the cursor moves, as the census names it.
#[derive(Clone, Copy)]
enum Motion {
    Long(Path),
    Retarget(Duration, Path),
    Tri(Duration),
}

#[derive(Clone, Copy)]
enum Path {
    Diagonal,
    Across,
}

impl Motion {
    fn name(self) -> String {
        let p = |p: Path| match p {
            Path::Diagonal => "diag",
            Path::Across => "across",
        };
        match self {
            Motion::Long(path) => format!("long-{}", p(path)),
            Motion::Retarget(d, path) => format!("every-{}ms-{}", d.as_millis(), p(path)),
            Motion::Tri(d) => format!("tri-{}ms", d.as_millis()),
        }
    }
}

/// One case's counts: frames by what the cap was, and repeat passes -- the
/// repeat frames of one move -- by what they drew.
#[derive(Default, Clone, Copy)]
struct Count {
    frames: Tally,
    repeat: Tally,
    capped: Tally,
    bound: Tally,
    imaged: Tally,
    grounds: Tally,
    scanning: Tally,
    passes: Tally,
    /// Every frame of the pass drew nothing, by the first frame's reason.
    out_grounds: Tally,
    out_scanning: Tally,
    out_imaged: Tally,
    out_bound: Tally,
    /// Drew nothing on its first frames, then drew.
    late: Tally,
    /// The most frames a late pass waited.
    late_most: Tally,
    drawn: Tally,
    /// Times the scanned caps were emptied.
    cleared: Tally,
}

/// One pass's frames, as `(held)` a frame.
fn close(pass: &mut Vec<Held>, c: &mut Count) {
    if pass.is_empty() {
        return;
    }
    c.passes.bump();
    let blank = |h: &Held| *h != Held::Capped;
    if pass.iter().all(blank) {
        match pass[0] {
            Held::Grounds => c.out_grounds.bump(),
            Held::Imaged => c.out_imaged.bump(),
            Held::Bound => c.out_bound.bump(),
            _ => c.out_scanning.bump(),
        }
    } else if blank(&pass[0]) {
        c.late.bump();
        let n = Tally::of(pass.iter().take_while(|h| blank(h)).count());
        if n.value() > c.late_most.value() {
            c.late_most = n;
        }
    } else {
        c.drawn.bump();
    }
    pass.clear();
}

fn main() {
    let mut args = baud_bench::args();
    let list = |a: Option<String>| a.filter(|s| s != "all");
    let sizes: Vec<f32> = list(args.next())
        .unwrap_or_else(|| "13,24,32".to_string())
        .split(',')
        .map(|s| s.parse().expect("points"))
        .collect();
    let picked = list(args.next());
    let root = format!("{}/..", env!("CARGO_MANIFEST_DIR"));
    let face_data = std::fs::read(format!(
        "{root}/baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
    ))
    .expect("face");
    let faces = [Face::parse(&face_data).expect("parse")];
    // What one cap over text costs: the theme's text over its page, solid
    // and over each desktop at the shipped dims, the cursor in the text's
    // colour, the scan a repeat pass pays a ground at most twice a frame.
    for (theme, alpha) in [(Theme::dark(), 179u8), (Theme::light(), 153)] {
        let ink = theme.fg.with_alpha(Chan::from_u8(0xFF));
        let solid = theme.bg.with_alpha(Chan::from_u8(0xFF));
        for page in [
            solid,
            solid.mix(baud_render::Argb(0xFFFF_FFFF), Chan::from_u8(alpha)),
            solid.mix(baud_render::Argb(0xFF00_0000), Chan::from_u8(alpha)),
        ] {
            let t0 = std::time::Instant::now();
            let n = 20u32;
            let mut k = Chan::from_u8(0);
            for _ in 0..n {
                k = std::hint::black_box(baud_render::under_threshold_over(ink, ink, page));
            }
            eprintln!(
                "scan: text {:08x} over {:08x}: cap {}, {:.2} ms",
                ink.0,
                page.0,
                k.get(),
                t0.elapsed().as_secs_f64() * 1e3 / f64::from(n)
            );
        }
    }
    let every = |ms, path| Motion::Retarget(Duration::from_millis(ms), path);
    let motions = [
        Motion::Long(Path::Diagonal),
        every(100, Path::Diagonal),
        every(200, Path::Diagonal),
        Motion::Long(Path::Across),
        every(100, Path::Across),
        every(200, Path::Across),
        Motion::Tri(Duration::from_millis(100)),
    ];
    println!(
        "recording\tpt\ttheme\talpha\tmotion\trate\torigin\trun\tframes\trepeat\tcapped\tbound\timaged\tgrounds\tscanning\tpasses\tout_grounds\tout_scanning\tout_imaged\tout_bound\tlate\tlate_most\tdrawn\tcleared"
    );
    for (path, cols, rows) in RECORDINGS {
        if picked
            .as_ref()
            .is_some_and(|l| !l.split(',').any(|p| p == path))
        {
            continue;
        }
        let size = (cols > 0).then_some(WinSize {
            ws_col: cols,
            ws_row: rows,
        });
        let steps = steps(&root, path, size);
        let max = WinSize {
            ws_col: steps.iter().map(|s| s.0.ws_col).max().unwrap_or(80),
            ws_row: steps.iter().map(|s| s.0.ws_row).max().unwrap_or(24),
        };
        let mut owned = Owned::with_geometry(&max, &CacheGeometry::UNKNOWN);
        let mut term = Term::new(owned.storage(), &steps[0].0);
        for (ws, bytes) in &steps {
            if term.geometry().0.index() != usize::from(ws.ws_col)
                || term.geometry().1.index() != usize::from(ws.ws_row)
            {
                let _ = term.resize(ws);
            }
            feed(&mut term, bytes);
        }
        feed(&mut term, b"\x1b[?25h");
        let (gc, gr) = (term.geometry().0.index(), term.geometry().1.index());
        let name = path.rsplit('/').next().unwrap_or(path);
        for &pt in &sizes {
            for (theme_name, alpha) in [
                ("dark", 179u8),
                ("dark", 255),
                ("light", 153),
                ("light", 255),
            ] {
                let theme = || {
                    let mut t = match theme_name {
                        "dark" => Theme::dark(),
                        _ => Theme::light(),
                    };
                    t.bg_alpha = Chan::from_u8(alpha);
                    t
                };
                let fresh = || {
                    Renderer::new(
                        Stack::new(&faces).unwrap(),
                        PxSize::of(pt).expect("a size"),
                        theme(),
                        Blend::Native,
                        Cols::of_index(gc),
                        Rows::of_index(gr),
                    )
                    .unwrap()
                };
                let probe = fresh();
                let m = probe.metrics();
                let (cw, ch) = (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()));
                let cell = (cw as f32, ch as f32);
                let height = Points(cell.1);
                // The census's path: a window's diagonal, 341 x 256 points.
                let cells = |v: f64| usize::try_from(v.round() as i64).unwrap_or(0);
                let dc = cells(341.0 / cw).min(gc.saturating_sub(1));
                let dr = cells(256.0 / ch).min(gr.saturating_sub(1));
                let spots = |n: usize, d: usize| {
                    let hi = n.saturating_sub(1 + d);
                    [0, hi / 2, hi]
                };
                let mut styles = vec![Resolved::default(); term.styles().len()];
                for motion in motions {
                    for rate in [60u32, 120] {
                        for (oi, (r0, c0)) in spots(gr, dr)
                            .into_iter()
                            .flat_map(|r| spots(gc, dc).into_iter().map(move |c| (r, c)))
                            .enumerate()
                        {
                            let a = (Row::from_index(r0), Col::at_index(c0));
                            let stops: Vec<(Row, Col)> = match motion {
                                Motion::Tri(_) => {
                                    let third = |d: usize| cells(d as f64 / 3.0);
                                    let (sc, sr) = (third(dc), third(dr));
                                    vec![
                                        a,
                                        (Row::from_index(r0 + sr), Col::at_index(c0 + sc)),
                                        (Row::from_index(r0 + 2 * sr), Col::at_index(c0 + 2 * sc)),
                                    ]
                                }
                                Motion::Long(Path::Diagonal)
                                | Motion::Retarget(_, Path::Diagonal) => {
                                    vec![a, (Row::from_index(r0 + dr), Col::at_index(c0 + dc))]
                                }
                                _ => vec![a, (Row::from_index(r0), Col::at_index(c0 + dc))],
                            };
                            let (moves, end): (Vec<f64>, f64) = match motion {
                                Motion::Long(_) => (vec![0.2], 2.2),
                                Motion::Retarget(d, _) | Motion::Tri(d) => {
                                    let p = d.as_secs_f64();
                                    let n = (3.0 / p).round() as i64;
                                    ((0..=n).map(|j| 0.2 + j as f64 * p).collect(), 4.4)
                                }
                            };
                            let mut r = fresh();
                            for run in ["cold", "warm"] {
                                let mut stamp = StyleStamp::default();
                                let mut chase = trail::Chase::<HostTime>::new(
                                    trail::Style::Effect(Effect::Plasma),
                                );
                                let go = |t: &mut Term<'_>, (row, col): (Row, Col)| {
                                    let cup =
                                        format!("\x1b[{};{}H", row.index() + 1, col.index() + 1);
                                    feed(t, cup.as_bytes());
                                };
                                go(&mut term, a);
                                let mut c = Count::default();
                                let mut pass: Vec<Held> = Vec::new();
                                let (mut next, mut at) = (0usize, 0usize);
                                let mut cached = r.flash_cached();
                                let frames = (end * f64::from(rate)).round() as i64;
                                // Later runs start a second on, so the
                                // first run's ledger has expired.
                                let t0 = if run == "cold" { 1000.0 } else { 1010.0 };
                                for k in 0..frames {
                                    let now = k as f64 / f64::from(rate);
                                    while next < moves.len() && moves[next] <= now + 1e-9 {
                                        at = (at + 1) % stops.len();
                                        go(&mut term, stops[at]);
                                        next += 1;
                                        close(&mut pass, &mut c);
                                    }
                                    let (cr, cc) = term.cursor();
                                    let rect = r.cursor_rect(cr, cc, term.cursor_style());
                                    let look = chase.frame(
                                        Some(rect),
                                        HostTime::of_secs(t0 + now).expect("a time"),
                                        cell,
                                        height,
                                    );
                                    let strength = match look {
                                        Some(TrailLook::Vfx(v)) => v.emitter.strength,
                                        Some(TrailLook::Smear(s)) => s.strength,
                                        Some(TrailLook::Ghost(g)) => g.strength,
                                        None => Strength::Full,
                                    };
                                    r.set_trail(look);
                                    r.set_images(&term, Rows::ZERO);
                                    r.sync_styles(&term, &mut styles, &mut stamp);
                                    c.frames.bump();
                                    let now_cached = r.flash_cached();
                                    if now_cached.value() < cached.value() {
                                        c.cleared.bump();
                                    }
                                    cached = now_cached;
                                    if strength == Strength::Full {
                                        continue;
                                    }
                                    c.repeat.bump();
                                    let held = r.flash_held();
                                    match held {
                                        Held::Capped => c.capped.bump(),
                                        Held::Bound => c.bound.bump(),
                                        Held::Imaged => c.imaged.bump(),
                                        Held::Grounds => c.grounds.bump(),
                                        Held::Scanning => c.scanning.bump(),
                                        Held::Full => panic!("a repeat read as full"),
                                    }
                                    pass.push(held);
                                }
                                close(&mut pass, &mut c);
                                println!(
                                    "{name}\t{pt}\t{theme_name}\t{alpha}\t{}\t{rate}\t{oi}\t{run}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                    motion.name(),
                                    c.frames.value(),
                                    c.repeat.value(),
                                    c.capped.value(),
                                    c.bound.value(),
                                    c.imaged.value(),
                                    c.grounds.value(),
                                    c.scanning.value(),
                                    c.passes.value(),
                                    c.out_grounds.value(),
                                    c.out_scanning.value(),
                                    c.out_imaged.value(),
                                    c.out_bound.value(),
                                    c.late.value(),
                                    c.late_most.value(),
                                    c.drawn.value(),
                                    c.cleared.value(),
                                );
                            }
                        }
                    }
                }
            }
        }
        eprintln!("{name}: done");
    }
}
