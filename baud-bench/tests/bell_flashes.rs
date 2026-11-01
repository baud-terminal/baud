//! **The visual bell's flashes, counted on the frames it draws** (WCAG
//! 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]: a `BEL`
//! rung over and over, at 60 frames a second on the CPU path -- the pixels
//! the Linux driver shows and the macOS pixel oracle holds the GPU arms to
//! -- through [`BellFlash`], the rule the drivers plan each frame's bell
//! by. Rung every frame, the fastest a program can ring it (the drivers
//! ring once a frame interval however many `BEL`s a read brings); every
//! 100 ms, the flash's own length; every 200 ms, xterm's
//! `bellSuppressTime` default (`[[source: xterm-man]]`), a bell a held key
//! or a repeated completion rings; and every half second. Each as the
//! other video, with no colour set, and in white and red, kitty's 0.6 at
//! the peak; on the dark and the light theme, over a dense screen: every
//! cell written, code in colour, rows in reverse video and a truecolor red
//! band.
//!
//! **It passes on both readings**: the census's verdict, the text's rule
//! with disjoint pairs, and its strict reading, with chained pairs. Every
//! ring period from one frame to sixty is held to the rule's own bound
//! too, at most two flashes in any second, on the shown alpha alone.
//!
//! The controls: kitty's own rule, the fade restarted at each ring with no
//! hold and no quiet (`screen_bell`, `[[source: kitty-6d5d0c4-screen]]`
//! 3146-3158), and baud's other-video flash as it was before the rule, a
//! ring holding it on for the length from the ring and nothing after, both
//! fail the verdict at 200 ms: the census sees the bell.

use std::collections::HashMap;
use std::time::Duration;

use baud_bench::flash::{Census, Outcome, Rate, Report, Scale};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::bell::{ease_in_out, tint_alpha};
use baud_render::{
    Argb, BellFlash, BellKind, BellShown, BellTint, Blend, Damage, PxSize, Rang, Renderer, Stack,
    Target, Theme,
};

const WS: WinSize = WinSize {
    ws_col: 48,
    ws_row: 16,
};

const RATE: u32 = 60;

/// The flash's length, the entry's `flash` padding `baud-app` reads
/// (`bell::LENGTH`, held to the pinned entry by its own test).
const LENGTH: Duration = Duration::from_millis(100);

/// Seconds the bell rings for in a run, with the screen still for half a
/// second before and after, so its start and its end are counted.
const SECONDS: u32 = 2;

/// The still frames before and after the rings.
const STILL: u32 = RATE / 2;

fn feed(t: &mut Term<'_>, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
}

/// Every row written to its last column: code in the palette's colours,
/// rows in reverse video, and a band of a truecolor red.
fn dense() -> String {
    let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
    let mut s = String::from("\x1b[?25l\x1b[H");
    for row in 0..rows {
        s.push_str(&format!("\x1b[{};1H", row + 1));
        let line: String = format!("{:>3} fn step_{row}(x: u32) -> u32 {{ x }} ", row + 1)
            .chars()
            .cycle()
            .take(cols)
            .collect();
        match row % 4 {
            0 => s.push_str(&format!("\x1b[7m{line}\x1b[0m")),
            1 => s.push_str(&format!("\x1b[48;2;180;0;0m{line}\x1b[0m")),
            _ => s.push_str(&format!("\x1b[{}m{line}\x1b[0m", 31 + row % 6)),
        }
    }
    s
}

/// How the bell is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Arm {
    /// No colour set: the other video.
    Automatic,
    /// `OSC 21 ; visual_bell=` this colour.
    Colour(u32),
}

impl std::fmt::Display for Arm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arm::Automatic => write!(f, "automatic"),
            Arm::Colour(c) => write!(f, "#{:06x}", c & 0x00FF_FFFF),
        }
    }
}

/// Which rule times the flash.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    /// [`BellFlash`], as the drivers plan it.
    Built,
    /// The control: kitty's fade restarted at each ring for a colour, and
    /// the other video held on for the length from each ring.
    Restarted,
}

/// What a frame draws of the bell, hashable for the frame cache.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Key {
    Dark,
    Video,
    Tint(u32, u8),
}

fn key(s: BellShown) -> Key {
    match s {
        BellShown::Dark => Key::Dark,
        BellShown::Video => Key::Video,
        BellShown::Tint(t) => Key::Tint(t.colour.0, t.alpha.get()),
    }
}

/// What each frame shows of a bell rung every `period` frames for
/// `frames` frames, after [`STILL`] and followed by as many, under `rule`.
fn timeline(arm: Arm, period: u32, rule: Rule, frames: u32) -> Vec<BellShown> {
    let kind = match arm {
        Arm::Automatic => BellKind::Video,
        Arm::Colour(c) => BellKind::Tint(Argb(c)),
    };
    let now = |f: u32| Duration::from_secs_f64(f64::from(f) / f64::from(RATE));
    let mut b = BellFlash::new(LENGTH);
    let mut last: Option<Duration> = None;
    (0..frames + 2 * STILL)
        .map(|f| {
            let rang = (STILL..STILL + frames).contains(&f) && (f - STILL).is_multiple_of(period);
            match rule {
                Rule::Built => {
                    let (n, s) = b.at(now(f), if rang { Rang::Yes } else { Rang::No }, kind);
                    b = n;
                    s
                }
                Rule::Restarted => {
                    if rang {
                        last = Some(now(f));
                    }
                    let Some(start) = last.filter(|&s| now(f) < s + LENGTH) else {
                        return BellShown::Dark;
                    };
                    let t = (now(f) - start).as_secs_f64() / LENGTH.as_secs_f64();
                    match kind {
                        BellKind::Video => BellShown::Video,
                        BellKind::Tint(colour) => {
                            let i = if t < 0.5 {
                                ease_in_out(t * 2.0)
                            } else {
                                1.0 - ease_in_out(t * 2.0 - 1.0)
                            };
                            BellShown::Tint(BellTint {
                                colour,
                                alpha: tint_alpha(colour, i),
                            })
                        }
                    }
                }
            }
        })
        .collect()
}

/// The census of `shown`, one a frame, drawn over the dense screen in
/// `theme`. **The screen is still**, so a frame is what the bell shows
/// over it: each distinct one is drawn once by the CPU path, incrementally
/// after the last, and counted wherever it recurs.
fn count(theme: &Theme, shown: &[BellShown]) -> Report {
    let mono = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, dense().as_bytes());
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut census = Census::new(w, h, Scale(1), Rate(RATE));
    let mut live = vec![0u32; n];
    let mut drawn: HashMap<Key, Vec<u32>> = HashMap::new();
    for &s in shown {
        let px = drawn.entry(key(s)).or_insert_with(|| {
            r.set_flash(s.video());
            r.set_bell_tint(s.tint());
            let mut target = Target::new(&mut live, w, w, h).unwrap();
            r.render(&mut t, &mut target, Damage::Incremental);
            live.clone()
        });
        census.frame(px);
    }
    census.finish()
}

const WHITE: u32 = 0xFFFF_FFFF;
const RED: u32 = 0xFFFF_0000;

/// **Every ring period, one frame to sixty, flashes at most twice in any
/// second**, read off the alpha alone: a flash is a run of lit frames
/// whose alpha rises to one peak and falls once, and no 60 frames hold the
/// start of more than two.
#[test]
fn no_ring_period_flashes_more_than_twice_a_second() {
    for arm in [Arm::Automatic, Arm::Colour(WHITE), Arm::Colour(RED)] {
        for period in 1..=60 {
            let shown = timeline(arm, period, Rule::Built, 4 * RATE);
            let level: Vec<u8> = shown
                .iter()
                .map(|s| match s {
                    BellShown::Dark => 0,
                    BellShown::Video => 255,
                    BellShown::Tint(t) => t.alpha.get(),
                })
                .collect();
            let starts: Vec<usize> = (0..level.len())
                .filter(|&i| level[i] > 0 && (i == 0 || level[i - 1] == 0))
                .collect();
            for (k, &s) in starts.iter().enumerate() {
                let end = starts.get(k + 1).copied().unwrap_or(level.len());
                let run = &level[s..end];
                let peak = run
                    .iter()
                    .position(|&v| v == *run.iter().max().unwrap())
                    .unwrap();
                assert!(
                    run[..=peak].windows(2).all(|p| p[0] <= p[1])
                        && run[peak..].windows(2).all(|p| p[0] >= p[1]),
                    "{arm} every {period}: one rise and one fall, {run:?}"
                );
            }
            for w in starts.windows(3) {
                assert!(
                    w[2] - w[0] >= usize::try_from(RATE).unwrap(),
                    "{arm} every {period}: three flashes in a second at {w:?}"
                );
            }
        }
    }
}

#[test]
fn the_bell_is_below_the_flash_thresholds() {
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for arm in [Arm::Automatic, Arm::Colour(WHITE), Arm::Colour(RED)] {
            for period in [1, 6, 12, 30] {
                let at = format!("{name} {arm} every {period} frames");
                let r = count(&theme, &timeline(arm, period, Rule::Built, SECONDS * RATE));
                let share = |a: f64| 100.0 * a / r.threshold.0;
                eprintln!(
                    "{at}: general {} red {}, area {:.1}% of the limit, strict {:.1}%",
                    r.general.flashes.0,
                    r.red.flashes.0,
                    share(r.area.0),
                    share(r.strict.0)
                );
                assert_eq!(r.translucent.value(), 0, "{at}: an opaque ground");
                assert_eq!(r.outcome(), Outcome::Passes, "{at}: {r:?}");
                assert_eq!(r.strict_outcome(), Outcome::Passes, "{at}: {r:?}");
            }
        }
    }
}

/// The controls: rung every 200 ms, kitty's restarted fade in white and in
/// red, and the other video held for the length after each ring, each fail
/// the verdict on both themes -- and each passes once the rule times it.
#[test]
fn without_the_rule_a_bell_rung_every_200_ms_fails() {
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for arm in [Arm::Automatic, Arm::Colour(WHITE), Arm::Colour(RED)] {
            let at = format!("{name} {arm}");
            let r = count(&theme, &timeline(arm, 12, Rule::Restarted, SECONDS * RATE));
            eprintln!(
                "{at} restarted: general {} red {}, area {:.0}",
                r.general.flashes.0, r.red.flashes.0, r.area.0
            );
            assert_eq!(r.outcome(), Outcome::Fails, "the control, {at}: {r:?}");
        }
    }
}
