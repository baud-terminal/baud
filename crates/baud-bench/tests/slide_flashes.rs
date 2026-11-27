//! **The alternate-screen slide's flashes, counted on the frames it draws**
//! (WCAG 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]:
//! a switch to the alternate screen and straight back, each the slide a
//! driver draws -- the new screen sliding in under the faint veil for a
//! quarter of a second, the veil fading off over 0.15 s once it has landed
//! -- at 60 frames a second on the CPU path, the pixels the Linux driver
//! shows and the macOS pixel oracle holds the GPU arms to, with the screens
//! still for half a second before, between and after: two slides inside
//! one second. On the dark and the light theme, over dense screens: every
//! cell written, as code in colour, as box drawing, as rows in reverse
//! video whose parity differs on the two screens, and as truecolor reds,
//! text and background bands, that the faint veil's inks do not name.
//!
//! **It passes on both readings**: the census's verdict, the text's rule
//! with disjoint pairs, and its strict reading, with chained pairs.
//!
//! The controls: the same slides drawn at full contrast, sfte's, fail the
//! verdict, so the census sees the slide's own flashes; and the screens
//! switched outright every 125 ms count more than three flashes at a
//! pixel.

use baud_bench::flash::{Census, Flashes, Outcome, Rate, Scale};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Blend, Damage, PxSize, Renderer, SLIDE_S, ScreenSlide, Stack, Target, Theme, Toward,
    Transition, compose_slide, slide_offsets,
};

const WS: WinSize = WinSize {
    ws_col: 64,
    ws_row: 20,
};

const RATE: u32 = 60;

/// The veil's fade once the screen has landed, in frames: 0.15 s.
const FADE_FRAMES: u16 = 9;

fn feed(t: &mut Term<'_>, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
}

#[derive(Clone, Copy, Debug)]
enum Content {
    Code,
    Boxes,
    Zebra,
    /// Reds the palette and the cube's corners do not name: truecolor
    /// text, and bands of a truecolor red background.
    Reds,
}

/// Every row of a screen written to its last column.
fn dense(content: Content, alternate: bool) -> String {
    let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
    let mut s = String::from("\x1b[H");
    for row in 0..rows {
        s.push_str(&format!("\x1b[{};1H", row + 1));
        let shift = row + usize::from(alternate);
        match content {
            Content::Code => {
                let line: String = format!(
                    "{:>3} fn step_{row}(x: u32) -> u32 {{ x.wrapping_mul({row}) }} drwxr-xr-x ",
                    row + 1
                )
                .chars()
                .cycle()
                .take(cols)
                .collect();
                s.push_str(&format!("\x1b[{}m{line}\x1b[0m", 31 + shift % 6));
            }
            Content::Boxes => {
                let pat = if alternate {
                    "│█▇▆▅▄▃▂▁ ┼─"
                } else {
                    "╭─┬─╮│ ├┼┤█▓▒░"
                };
                let line: String = pat.chars().cycle().skip(row).take(cols).collect();
                s.push_str(&format!("\x1b[{}m{line}\x1b[0m", 31 + shift % 6));
            }
            Content::Reds => match shift % 2 {
                0 => s.push_str(&format!(
                    "\x1b[38;2;200;0;0m{}\x1b[0m",
                    "fn red(x: u32) -> u32 { x } "
                        .chars()
                        .cycle()
                        .take(cols)
                        .collect::<String>()
                )),
                _ => s.push_str(&format!("\x1b[48;2;180;0;0m{}\x1b[0m", " ".repeat(cols))),
            },
            Content::Zebra => match shift % 2 {
                0 => s.push_str(&format!("\x1b[7m{}\x1b[0m", " ".repeat(cols))),
                _ => s.push_str(&" ".repeat(cols)),
            },
        }
    }
    s
}

/// How the slides are drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drawn {
    /// As a driver draws them: under the faint veil, which fades off.
    Faint,
    /// At full contrast, as sfte draws them: the control.
    Full,
}

/// The census of a switch there and back in `theme` over `content`, drawn
/// as `drawn`; or, for the control, the screens switched outright every
/// `toggle` frames.
fn count(
    theme: &Theme,
    content: Content,
    drawn: Drawn,
    toggle: Option<u32>,
) -> baud_bench::flash::Report {
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
    feed(&mut t, b"\x1b[?25l");
    feed(&mut t, dense(content, false).as_bytes());
    let (w, h) = r.pixel_size();
    let (wu, hu) = (
        usize::try_from(w.value()).unwrap(),
        usize::try_from(h.value()).unwrap(),
    );
    let mut census = Census::new(w, h, Scale(1), Rate(RATE));
    let mut live = vec![0u32; wu * hu];
    let paint = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32]| {
        let mut target = Target::new(px, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render(t, seal, &mut target, Damage::Incremental);
    };
    let switch = |t: &mut Term<'_>, to: Toward| match to {
        Toward::Alternate => {
            feed(t, b"\x1b[?1049h\x1b[?25l");
            feed(t, dense(content, true).as_bytes());
        }
        Toward::Primary => feed(t, b"\x1b[?1049l\x1b[?25l"),
    };
    if let Some(every) = toggle {
        let mut to = Toward::Alternate;
        for f in 0..2 * RATE {
            if f % every == 0 && f > 0 {
                switch(&mut t, to);
                to = match to {
                    Toward::Alternate => Toward::Primary,
                    Toward::Primary => Toward::Alternate,
                };
            }
            paint(&mut r, &mut t, &mut live);
            census.frame(&live);
        }
        return census.finish();
    }
    let still = RATE / 2;
    for _ in 0..still {
        paint(&mut r, &mut t, &mut live);
        census.frame(&live);
    }
    let sliding: Vec<f32> = (0u16..)
        .map(|f| f32::from(f) / 60.0 / SLIDE_S)
        .take_while(|&p| p < 1.0)
        .collect();
    for toward in [Toward::Alternate, Toward::Primary] {
        let kept = live.clone();
        switch(&mut t, toward);
        for &p in &sliding {
            if drawn == Drawn::Faint {
                r.set_screen_fade(Some(Transition::faint(0.0)));
            }
            paint(&mut r, &mut t, &mut live);
            let o = slide_offsets(
                ScreenSlide {
                    progress: p,
                    toward,
                },
                h,
            );
            let mut frame = vec![0u32; wu * hu];
            assert!(compose_slide(&live, &kept, &mut frame, w, h, o));
            census.frame(&frame);
        }
        if drawn == Drawn::Faint {
            for f in 0..FADE_FRAMES {
                r.set_screen_fade(Some(Transition::faint(
                    f32::from(f) / f32::from(FADE_FRAMES),
                )));
                paint(&mut r, &mut t, &mut live);
                census.frame(&live);
            }
            r.set_screen_fade(None);
        }
        for _ in 0..still {
            paint(&mut r, &mut t, &mut live);
            census.frame(&live);
        }
    }
    census.finish()
}

#[test]
fn the_slide_is_below_the_flash_thresholds() {
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for content in [Content::Code, Content::Boxes, Content::Zebra, Content::Reds] {
            let at = format!("{name} {content:?}");
            let r = count(&theme, content, Drawn::Faint, None);
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
    // The controls: sfte's slide at full contrast, and the screens
    // switched outright every 125 ms.
    let full = count(&Theme::light(), Content::Code, Drawn::Full, None);
    assert_eq!(full.outcome(), Outcome::Fails, "the control: {full:?}");
    let c = count(&Theme::dark(), Content::Code, Drawn::Full, Some(RATE / 8));
    assert!(
        c.general.flashes > Flashes(3),
        "the control: {:?}",
        c.general
    );
}
