//! The scrollbar's fade and hover on the CPU path, which the pixel oracle
//! cannot reach: the oracle compares the two GPU arms, so a rule both lost
//! would pass it. `docs/research/notes/l-scrollbar.md` is the spec.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Blend, Damage, Hover, Look, PxSize, Renderer, Scroll, Scroller, Stack, Target, Theme, Widths,
    Window,
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
            scroller: Scroller::Overlay,
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
                scroller: Scroller::Overlay,
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
            scroller: Scroller::Overlay,
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

/// A legacy frame at the live screen: the gutter set, the scrollbar legacy.
fn legacy_frame(
    p: &Painter<'_>,
    t: &mut Term<'_>,
    window: Option<Window>,
) -> (Vec<u32>, usize, usize, u32) {
    let mut r = p.renderer();
    r.set_gutter(WIDTHS.strip);
    r.set_scrollbar(window.map(|window| Scroll {
        window,
        look: Look::HIDDEN,
        widths: WIDTHS,
        scroller: Scroller::Legacy,
    }));
    let (w, h) = r.frame_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let mut target = Target::new(&mut px, w, w, h).unwrap();
    r.render_at(t, Rows::ZERO, &mut target, Damage::Full);
    let margin = r.clear_color().0;
    (
        px,
        usize::try_from(w.value()).unwrap(),
        usize::try_from(h.value()).unwrap(),
        margin,
    )
}

/// **Under the legacy style the gutter is the margin with the bar in it,
/// and the grid is untouched by it.** Past the last cell the frame is the
/// letterbox margin's colour -- not the last cell's, which the last column
/// here is red to tell apart -- except for the track, which is drawn on
/// every row whether or not there is history, and the knob. The cells are
/// the same pixels as a frame with no scrollbar at all.
#[test]
fn a_legacy_gutter_is_the_margin_with_the_track_and_knob_in_it() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let p = Painter { faces: &faces };
    let cols = usize::from(WS.ws_col);
    let check = |history: bool| {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        if history {
            let lines: Vec<u8> = (0..40)
                .flat_map(|i| format!("line {i}\r\n").into_bytes())
                .collect();
            feed(&mut t, &lines);
        }
        // Every row's last cell red, so a gutter that repeats the last
        // cell's background is caught on every row.
        let red: Vec<u8> = (1..=WS.ws_row)
            .flat_map(|y| format!("\x1b[{y};{cols}H\x1b[41m \x1b[0m").into_bytes())
            .collect();
        feed(&mut t, &red);
        let window = Window {
            retained: Rows::of_count(t.scrollback().len()),
            back: Rows::ZERO,
            rows: Rows::of_index(usize::from(WS.ws_row)),
        };
        let (bar, w, h, margin) = legacy_frame(&p, &mut t, Some(window));
        let (none, _, _, _) = legacy_frame(&p, &mut t, None);
        let grid_w = w - usize::try_from(WIDTHS.strip.value()).unwrap();
        let inset = usize::try_from(WIDTHS.inset.value()).unwrap();
        let wide = usize::try_from(WIDTHS.hovered.value()).unwrap();
        let track = (w - inset - wide)..(w - inset);
        let mut shades = std::collections::BTreeSet::new();
        for y in 0..h {
            let row = &bar[y * w..(y + 1) * w];
            assert_eq!(
                &row[..grid_w],
                &none[y * w..y * w + grid_w],
                "row {y}: the bar reached a cell (history {history})"
            );
            for (x, &px) in row.iter().enumerate().skip(grid_w) {
                match track.contains(&x) {
                    false => assert_eq!(
                        px, margin,
                        "row {y} column {x}: the gutter is not the margin (history {history})"
                    ),
                    true => assert_ne!(
                        px, margin,
                        "row {y} column {x}: no track (history {history})"
                    ),
                }
            }
            // Down the track's middle: the track alone, or the knob over it.
            shades.insert(row[track.start + wide / 2]);
        }
        shades.len()
    };
    assert_eq!(check(false), 1, "the track alone, with nothing to scroll");
    assert_eq!(check(true), 2, "the track and a knob over it, with history");
}

/// **A gutter that appears repaints every row.** The frame is wider, so an
/// incremental frame after the style changes has to paint the whole new
/// width rather than trust rows whose last frame had no gutter -- the same
/// frame, painted incrementally across the change, equals a fresh full one.
/// With a legacy bar its track dirties every row anyway, so the gutter is
/// also set with no bar at all, where only the renderer's own reset can
/// repaint the rows.
#[test]
fn a_gutter_that_appears_repaints_every_row() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let p = Painter { faces: &faces };
    with_history(|t, window| {
        let legacy = Scroll {
            window,
            look: Look::HIDDEN,
            widths: WIDTHS,
            scroller: Scroller::Legacy,
        };
        let frame = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut Vec<u32>, d: Damage| {
            let (w, h) = r.frame_size();
            px.resize(usize::try_from(w.value() * h.value()).unwrap(), 0);
            let mut target = Target::new(px, w, w, h).unwrap();
            r.render_at(t, Rows::of_count(3), &mut target, d);
        };
        for (bar, what) in [(None, "no bar"), (Some(legacy), "a legacy bar")] {
            let mut r = p.renderer();
            let mut live = Vec::new();
            frame(&mut r, t, &mut live, Damage::Full);
            r.set_gutter(WIDTHS.strip);
            r.set_scrollbar(bar);
            let mut wider = Vec::new();
            frame(&mut r, t, &mut wider, Damage::Incremental);
            let mut fresh = p.renderer();
            fresh.set_gutter(WIDTHS.strip);
            fresh.set_scrollbar(bar);
            let mut full = Vec::new();
            frame(&mut fresh, t, &mut full, Damage::Full);
            assert!(
                wider == full,
                "an incremental frame across a new gutter, {what}"
            );
        }
    });
}
