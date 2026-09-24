//! The scrollbar's fade and hover on the CPU path, which the pixel oracle
//! cannot reach: the oracle compares the two GPU arms, so a rule both lost
//! would pass it. `docs/research/notes/l-scrollbar.md` is the spec.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Blend, Damage, Hover, Look, PxSize, Renderer, Scroll, Stack, Target, Theme, Widths, Window,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 10,
};

/// AppKit's widths at 2x.
const WIDTHS: Widths = Widths {
    rest: Px::of(12),
    hovered: Px::of(22),
    inset: Px::of(6),
    strip: Px::of(34),
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn look(shown: u8, hover: Hover) -> Look {
    Look {
        shown: Chan::from_u8(shown),
        hover,
    }
}

/// Forty lines on a ten-row screen, looked at three lines back.
fn with_history(f: impl FnOnce(&mut Term<'_>, Window)) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let lines: Vec<u8> = (0..40)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect();
    feed(&mut t, &lines);
    let window = Window {
        retained: Rows::of_count(t.scrollback().len()),
        back: Rows::of_count(3),
        rows: Rows::of_index(usize::from(WS.ws_row)),
    };
    f(&mut t, window);
}

struct Painter<'f> {
    faces: &'f [Face<'f>],
}

impl<'f> Painter<'f> {
    fn renderer(&self) -> Renderer<'f, 'f> {
        Renderer::new(
            Stack::new(self.faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    }
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, pixels: &mut [u32], damage: Damage) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(pixels, w, w, h).unwrap();
    r.render_at(t, Rows::of_count(3), &mut target, damage);
}

/// **A fade and a hover repaint incrementally what a full frame paints.**
/// The bar's equality is the damage rule: a fade changes only its alpha and
/// a hover only its width and track, so a rule that compared the knob's
/// geometry alone would leave the old bar on screen through an incremental
/// frame -- and the track, which spans every row, reaches rows the knob never
/// touched.
#[test]
fn a_fade_and_a_hover_repaint_incrementally_what_a_full_frame_paints() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let p = Painter { faces: &faces };
    with_history(|t, window| {
        let mut r = p.renderer();
        let (w, h) = r.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let mut live = vec![0u32; n];
        let scroll = |l: Look| Scroll {
            window,
            look: l,
            widths: WIDTHS,
        };
        r.set_scrollbar(Some(scroll(look(255, Hover::Rest))));
        paint(&mut r, t, &mut live, Damage::Full);
        let mut previous = live.clone();
        for (l, what) in [
            (look(128, Hover::Rest), "halfway faded"),
            (look(255, Hover::Hovered), "hovered"),
            (look(40, Hover::Rest), "nearly gone, narrow again"),
            (look(0, Hover::Rest), "gone"),
        ] {
            r.set_scrollbar(Some(scroll(l)));
            paint(&mut r, t, &mut live, Damage::Incremental);
            let mut fresh = p.renderer();
            let mut full = vec![0u32; n];
            fresh.set_scrollbar(Some(scroll(l)));
            paint(&mut fresh, t, &mut full, Damage::Full);
            assert!(
                live == full,
                "an incremental frame {what} is not the full frame"
            );
            assert!(live != previous, "{what} changed no pixel");
            previous = live.clone();
        }
    });
}

/// **Hovered, the track spans every row and the knob widens**: the pixels
/// that change between rest and hover lie in the widened band's columns
/// only, and down rows the knob never covers.
#[test]
fn a_hover_draws_the_track_down_every_row_in_the_widened_band() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let p = Painter { faces: &faces };
    with_history(|t, window| {
        let mut frames = Vec::new();
        for hover in [Hover::Rest, Hover::Hovered] {
            let mut r = p.renderer();
            let (w, h) = r.pixel_size();
            let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
            r.set_scrollbar(Some(Scroll {
                window,
                look: look(255, hover),
                widths: WIDTHS,
            }));
            paint(&mut r, t, &mut px, Damage::Full);
            frames.push((px, w.value(), h.value()));
        }
        let (rest, w, h) = &frames[0];
        let (over, _, _) = &frames[1];
        let (w, h) = (usize::try_from(*w).unwrap(), usize::try_from(*h).unwrap());
        let band = (w - 22 - 6)..(w - 6);
        let mut rows = 0;
        for y in 0..h {
            let mut row_changed = false;
            for x in 0..w {
                if rest[y * w + x] != over[y * w + x] {
                    assert!(
                        band.contains(&x),
                        "a hover changed ({x}, {y}), outside the band"
                    );
                    row_changed = true;
                }
            }
            if row_changed {
                rows += 1;
            }
        }
        assert_eq!(rows, h, "the track is on every row");
    });
}

/// **A knob that moves under a hover repaints only the rows it touched.**
/// The track spans every row, so a damage rule that compared whole bars
/// would repaint the whole frame each time output moved the knob; the rule
/// compares what the bar draws in each row's band, so the rows the track
/// alone covers, identical before and after, stay clean.
#[test]
fn a_knob_that_moves_under_a_hover_repaints_only_the_rows_it_touched() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let p = Painter { faces: &faces };
    with_history(|t, window| {
        let mut r = p.renderer();
        let (w, h) = r.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let mut live = vec![0u32; n];
        let hovered = look(255, Hover::Hovered);
        let scroll = |window: Window| Scroll {
            window,
            look: hovered,
            widths: WIDTHS,
        };
        r.set_scrollbar(Some(scroll(window)));
        paint(&mut r, t, &mut live, Damage::Full);
        // Twenty more lines retained behind the same view: the knob shrinks
        // and moves, the text does not.
        let moved = Window {
            retained: Rows::of_count(window.retained.index() + 20),
            ..window
        };
        r.set_scrollbar(Some(scroll(moved)));
        let converted = r.convert(t, Rows::of_count(3), Damage::Incremental);
        let dirty = converted.rows_dirty.value();
        let rows = u64::from(WS.ws_row);
        assert!(dirty > 0, "the knob moved and nothing repainted");
        assert!(
            dirty < rows,
            "{dirty} of {rows} rows repainted for a knob that moved"
        );
    });
}
