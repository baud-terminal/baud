//! **The alternate-screen slide, frame by frame**, on the CPU path the
//! Linux driver shows and the macOS pixel oracle holds the GPU arms to:
//! each screen's rows painted a background of their own, the frame shown
//! before the switch kept, and every frame of a quarter of a second at 60
//! frames a second composed as a driver composes it. Entering the
//! alternate screen the alternate's rows rise from below over the
//! primary's, which stay where they were; leaving it the primary's come
//! down from above over the alternate's -- sfte's slide as it draws it with
//! its default smooth scroll on ([[source: sfte]]).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ImageBudget, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Blend, CURSOR_GUIDE, CursorPhase, Damage, GuideLook, Offsets, PxSize, Renderer, SLIDE_S,
    ScreenSlide, Stack, Strength, Target, Theme, Toward, Trail, TrailLook, compose_slide,
    slide_offsets,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 8,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn px(p: Px) -> usize {
    usize::try_from(p.value()).unwrap()
}

/// The background row `row` of a screen paints: the primary's red ramp,
/// the alternate's blue one, every row its own.
fn colour(screen: Screen, row: usize) -> (u8, u8, u8) {
    let v = u8::try_from(40 + 20 * row).unwrap();
    match screen {
        Screen::Primary => (v, 0, 0),
        Screen::Alternate => (0, 0, v),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Primary,
    Alternate,
}

/// Every row of the screen up painted its own background whole, the
/// cursor hidden so no cell differs from its row.
fn paint_rows(t: &mut Term<'_>, screen: Screen) {
    let mut s = String::from("\x1b[?25l\x1b[H");
    for row in 0..usize::from(WS.ws_row) {
        let (r, g, b) = colour(screen, row);
        s.push_str(&format!("\x1b[{};1H\x1b[48;2;{r};{g};{b}m\x1b[2K", row + 1));
    }
    s.push_str("\x1b[0m");
    feed(t, s.as_bytes());
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

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, pixels: &mut [u32]) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(pixels, w, w, h).unwrap();
    r.render(t, &mut target, Damage::Incremental);
}

/// **A frame of the switch at `p`**, as a driver draws it: the live frame
/// painted as every frame is, and the slide composed from it and `kept`.
fn slid(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    live: &mut [u32],
    kept: &[u32],
    p: f32,
    toward: Toward,
) -> Vec<u32> {
    paint(r, t, live);
    let (w, h) = r.pixel_size();
    let o = slide_offsets(
        ScreenSlide {
            progress: p,
            toward,
        },
        h,
    );
    let mut out = vec![0u32; live.len()];
    assert!(compose_slide(live, kept, &mut out, w, h, o));
    out
}

/// Which screen's row a frame's pixel row `y` shows, read off its colour
/// in the middle of the row.
fn shown(frame: &[u32], w: usize, y: usize) -> Option<(Screen, usize)> {
    let p = frame[y * w + w / 2];
    let [_, r, g, b] = p.to_be_bytes();
    let row = |v: u8| (v >= 40 && (v - 40).is_multiple_of(20)).then(|| usize::from((v - 40) / 20));
    match (r, g, b) {
        (v, 0, 0) => row(v).map(|n| (Screen::Primary, n)),
        (0, 0, v) => row(v).map(|n| (Screen::Alternate, n)),
        _ => None,
    }
}

/// The frames of one switch: `p` at every sixtieth of a second until the
/// slide has settled.
fn steps() -> Vec<f32> {
    (0u16..)
        .map(|f| f32::from(f) / 60.0 / SLIDE_S)
        .take_while(|&p| p < 1.0)
        .chain([1.0])
        .collect()
}

/// **Each frame shows the incoming screen where the slide has brought it,
/// over the outgoing one in place**, every pixel row, both ways: in from
/// below toward the alternate screen, in from above toward the primary.
#[test]
fn the_incoming_screen_slides_in_over_the_outgoing_one() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces);
    let (w, h) = r.pixel_size();
    let (width, height) = (px(w), px(h));
    let ch = px(r.metrics().cell_h);
    assert_eq!(height, ch * usize::from(WS.ws_row));
    let mut live = vec![0u32; width * height];
    paint_rows(&mut t, Screen::Primary);
    for (toward, from, to, switch) in [
        (
            Toward::Alternate,
            Screen::Primary,
            Screen::Alternate,
            &b"\x1b[?1049h"[..],
        ),
        (
            Toward::Primary,
            Screen::Alternate,
            Screen::Primary,
            &b"\x1b[?1049l"[..],
        ),
    ] {
        // The frame shown before the switch: the outgoing screen.
        paint(&mut r, &mut t, &mut live);
        let kept = live.clone();
        assert_eq!(shown(&kept, width, 0), Some((from, 0)));
        feed(&mut t, switch);
        if to == Screen::Alternate {
            // The program draws its screen, which `?1049` cleared.
            paint_rows(&mut t, Screen::Alternate);
        }
        // Where the two screens meet, frame to frame: it only rises
        // toward the alternate screen and only falls toward the primary.
        let mut seam: Option<usize> = None;
        for p in steps() {
            let frame = slid(&mut r, &mut t, &mut live, &kept, p, toward);
            let o: Offsets = slide_offsets(
                ScreenSlide {
                    progress: p,
                    toward,
                },
                h,
            );
            let rows = i32::try_from(height).unwrap();
            for y in 0..height {
                let yi = i32::try_from(y).unwrap();
                let want = match yi - o.incoming {
                    v if (0..rows).contains(&v) => (to, usize::try_from(v).unwrap() / ch),
                    _ => (from, usize::try_from(yi - o.outgoing).unwrap() / ch),
                };
                assert_eq!(
                    shown(&frame, width, y),
                    Some(want),
                    "{toward:?} at {p}: pixel row {y}"
                );
            }
            let screen = |y: usize| shown(&frame, width, y).map(|(s, _)| s);
            // Which edge the new screen comes in at, read off the frame
            // alone: the bottom toward the alternate screen, the top toward
            // the primary, while the old one holds the other edge.
            if p > 0.0 && p < 1.0 {
                let (enters, holds) = match toward {
                    Toward::Alternate => (height - 1, 0),
                    Toward::Primary => (0, height - 1),
                };
                assert_eq!(
                    screen(enters),
                    Some(to),
                    "{toward:?} at {p}: the new screen's edge"
                );
                assert_eq!(
                    screen(holds),
                    Some(from),
                    "{toward:?} at {p}: the old screen's edge"
                );
            }
            let now = match toward {
                Toward::Alternate => (0..height).find(|&y| screen(y) == Some(to)),
                Toward::Primary => (0..height).rev().find(|&y| screen(y) == Some(to)),
            }
            .unwrap_or(match toward {
                Toward::Alternate => height,
                Toward::Primary => 0,
            });
            if let Some(was) = seam {
                match toward {
                    Toward::Alternate => assert!(now <= was, "{p}: the alternate fell back"),
                    Toward::Primary => assert!(now >= was, "{p}: the primary rose back"),
                }
            }
            seam = Some(now);
        }
        assert_eq!(
            seam,
            Some(match toward {
                Toward::Alternate => 0,
                Toward::Primary => height - 1,
            }),
            "{toward:?}: the incoming screen fills the frame"
        );
        // Settled: the incoming screen alone, in place.
        let frame = slid(&mut r, &mut t, &mut live, &kept, 1.0, toward);
        for row in 0..usize::from(WS.ws_row) {
            assert_eq!(shown(&frame, width, row * ch), Some((to, row)));
        }
    }
}

/// **Half way is seven eighths of the way**: half way through a switch to
/// the alternate screen, sfte's cubic has brought it 7/8 of the frame up,
/// so the primary's first row is still at the top, in place, and the
/// alternate's first seven fill the rest -- a linear slide would show half
/// of each, and a fade the alternate's first row at the top.
#[test]
fn half_way_is_seven_eighths_of_the_way() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces);
    let (w, h) = r.pixel_size();
    let (width, height) = (px(w), px(h));
    let ch = px(r.metrics().cell_h);
    let mut live = vec![0u32; width * height];
    paint_rows(&mut t, Screen::Primary);
    paint(&mut r, &mut t, &mut live);
    let kept = live.clone();
    feed(&mut t, b"\x1b[?1049h");
    paint_rows(&mut t, Screen::Alternate);
    let frame = slid(&mut r, &mut t, &mut live, &kept, 0.5, Toward::Alternate);
    assert_eq!(shown(&frame, width, 0), Some((Screen::Primary, 0)));
    assert_eq!(shown(&frame, width, ch), Some((Screen::Alternate, 0)));
    assert_eq!(
        shown(&frame, width, height - 1),
        Some((Screen::Alternate, 6))
    );
}

/// **Output arriving while the screen slides is drawn at once**, where the
/// incoming screen then is: a row the program repaints between two frames
/// is in the next frame, at the slide's offset, and nothing waits for the
/// slide to settle.
#[test]
fn output_during_the_slide_is_drawn_where_the_screen_is() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces);
    let (w, h) = r.pixel_size();
    let (width, height) = (px(w), px(h));
    let ch = px(r.metrics().cell_h);
    let mut live = vec![0u32; width * height];
    paint_rows(&mut t, Screen::Primary);
    paint(&mut r, &mut t, &mut live);
    let kept = live.clone();
    feed(&mut t, b"\x1b[?1049h");
    paint_rows(&mut t, Screen::Alternate);
    let before = slid(&mut r, &mut t, &mut live, &kept, 0.2, Toward::Alternate);
    // The program repaints its row 0 with row 5's colour.
    let (r5, g5, b5) = colour(Screen::Alternate, 5);
    feed(
        &mut t,
        format!("\x1b[1;1H\x1b[48;2;{r5};{g5};{b5}m\x1b[2K\x1b[0m").as_bytes(),
    );
    let after = slid(&mut r, &mut t, &mut live, &kept, 0.3, Toward::Alternate);
    let o = slide_offsets(
        ScreenSlide {
            progress: 0.3,
            toward: Toward::Alternate,
        },
        h,
    );
    let at = usize::try_from(o.incoming).unwrap();
    assert!(at < height - ch, "row 0 is on the frame at 0.3");
    assert_eq!(shown(&after, width, at), Some((Screen::Alternate, 5)));
    assert_ne!(shown(&before, width, at), Some((Screen::Alternate, 5)));
}

/// **The old screen is kept with its cursor hidden**, as sfte's outgoing
/// pass hides it: a block cursor on a letter, its blink glow lit, the
/// cursor guide on its row and a trail sweeping to it from two rows up,
/// all in the frame shown before the switch. Drawn again at the switch,
/// the frame kept is that screen drawn with the cursor hidden (`?25l`),
/// pixel for pixel, both ways, and every pixel row of the old screen a
/// frame of the slide shows is that frame's. The way back is taken before
/// the slide in is over, and keeps the alternate screen in place.
/// The control: the frame shown before the switch is not it.
#[test]
fn the_old_screen_is_kept_with_no_cursor() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces);
    let (w, h) = r.pixel_size();
    let (width, height) = (px(w), px(h));
    let guide = Some(GuideLook {
        colour: CURSOR_GUIDE,
        line: Px::of(1),
    });
    let glowing = CursorPhase {
        glow: Chan::from_u8(160),
        ..CursorPhase::SHOWN
    };
    let mut live = vec![0u32; width * height];
    // A blinking block on the `v` of `vim`, on the shell's third row.
    feed(
        &mut t,
        b"\x1b[1 q$ ls\r\nbaud-app docs\r\n$ vim main.rs\x1b[3;3H",
    );
    for (toward, switch, program) in [
        (
            Toward::Alternate,
            &b"\x1b[?1049h"[..],
            &b"\x1b[33m  1 \x1b[0mfn main() {}\r\n~\x1b[1;7H"[..],
        ),
        (Toward::Primary, &b"\x1b[?1049l"[..], &b""[..]),
    ] {
        // The same screen with the cursor hidden, drawn whole by a renderer
        // of its own.
        feed(&mut t, b"\x1b[?25l");
        let mut fresh = renderer(&faces);
        fresh.set_cursor_guide(guide);
        let mut want = vec![0u32; width * height];
        paint(&mut fresh, &mut t, &mut want);
        feed(&mut t, b"\x1b[?25h");
        let (row, col) = t.cursor();
        r.set_cursor_blink(glowing);
        r.set_cursor_guide(guide);
        r.set_trail(Some(TrailLook::Ghost(Trail {
            from: (
                Row::from_index(row.index().saturating_sub(2)),
                Col::at_index(0),
            ),
            to: (row, col),
            progress: 0.4,
            origin_dy: 0.0,
            lift: Px::of(0),
            strength: Strength::Full,
        })));
        paint(&mut r, &mut t, &mut live);
        let differ = live.iter().zip(&want).filter(|(a, b)| a != b).count();
        assert!(differ > 0, "{toward:?}: the frame shown draws the cursor");
        feed(&mut t, switch);
        feed(&mut t, program);
        {
            let mut target = Target::new(&mut live, w, w, h).unwrap();
            r.render_hidden(&t, &mut target);
        }
        let kept = live.clone();
        let rows_off: Vec<usize> = (0..height)
            .filter(|&y| kept[y * width..(y + 1) * width] != want[y * width..(y + 1) * width])
            .collect();
        assert!(
            rows_off.is_empty(),
            "{toward:?}: kept rows not the screen with no cursor: {rows_off:?}"
        );
        // The slide's frames, as a driver draws them: no trail.
        r.set_trail(None);
        for p in [0.0, 0.1, 0.3, 0.6] {
            let frame = slid(&mut r, &mut t, &mut live, &kept, p, toward);
            let o = slide_offsets(
                ScreenSlide {
                    progress: p,
                    toward,
                },
                h,
            );
            let rows = i32::try_from(height).unwrap();
            for y in 0..height {
                let yi = i32::try_from(y).unwrap();
                if (0..rows).contains(&(yi - o.incoming)) {
                    continue;
                }
                assert_eq!(
                    frame[y * width..(y + 1) * width],
                    want[y * width..(y + 1) * width],
                    "{toward:?} at {p}: old screen's pixel row {y}"
                );
            }
        }
    }
}

/// **An image the switch deletes stays on the kept rows**: leaving the
/// alternate screen deletes its images before the switch is seen, so the
/// row the cursor was on, painted again with no cursor, can blit nothing
/// over its share of a picture the frame shown had there. The frame kept
/// is still the alternate screen drawn with its cursor hidden, picture and
/// all, with the cursor on a letter clear of a Sixel and with it on a cell
/// the Sixel, drawn over the text, covers.
/// The control: the frame shown before the switch is not it, where the
/// cursor shows.
#[test]
fn an_image_the_switch_deletes_stays_on_the_kept_rows() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    for (at, shows) in [(&b"\x1b[3;2H"[..], true), (&b"\x1b[3;11H"[..], false)] {
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 20));
        let mut t = Term::new(owned.storage(), &WS);
        let mut r = renderer(&faces);
        let (w, h) = r.pixel_size();
        let (width, height) = (px(w), px(h));
        let ch = px(r.metrics().cell_h);
        // A picture three rows tall from the second row's tenth cell, and
        // text on its third row.
        let (pw, ph) = (40, 3 * ch);
        let mut bytes = b"\x1b[?1049h\x1b[2;10H".to_vec();
        bytes.extend_from_slice(format!("\x1bP0;1;0q\"1;1;{pw};{ph}#1;2;90;10;10").as_bytes());
        for _ in 0..ph.div_ceil(6) {
            bytes.extend_from_slice(format!("#1!{pw}~-").as_bytes());
        }
        bytes.extend_from_slice(b"\x1b\\\x1b[3;1Hab cd");
        bytes.extend_from_slice(at);
        feed(&mut t, &bytes);
        feed(&mut t, b"\x1b[?25l");
        let mut fresh = renderer(&faces);
        let mut want = vec![0u32; width * height];
        paint(&mut fresh, &mut t, &mut want);
        feed(&mut t, b"\x1b[?25h");
        let mut live = vec![0u32; width * height];
        paint(&mut r, &mut t, &mut live);
        assert_eq!(live != want, shows, "{at:?}: the frame shown");
        let red = |frame: &[u32]| {
            frame
                .iter()
                .filter(|&&p| {
                    let [_, r, g, b] = p.to_be_bytes();
                    r > 200 && g < 60 && b < 60
                })
                .count()
        };
        assert!(red(&want) > 0, "{at:?}: the picture is drawn");
        feed(&mut t, b"\x1b[?1049l");
        {
            let mut target = Target::new(&mut live, w, w, h).unwrap();
            r.render_hidden(&t, &mut target);
        }
        let rows_off: Vec<usize> = (0..height)
            .filter(|&y| live[y * width..(y + 1) * width] != want[y * width..(y + 1) * width])
            .collect();
        assert!(
            rows_off.is_empty(),
            "{at:?}: kept rows not the alternate screen with no cursor: {rows_off:?}"
        );
    }
}
