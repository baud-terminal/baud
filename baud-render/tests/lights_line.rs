//! **The activity lights on a line of their own** (`[window]
//! activity_lights = "bottom_line"` or `"top_line"`): a band of the whole
//! frame outside the grid, only as tall as the lights' picture and its
//! margins, below the grid, DEC's status line and the notice -- the VT420's
//! keyboard indicator line, "at the bottom of the screen, below the status
//! line" -- or above the grid; the title strip's pictures drawn into it on
//! a plate of the theme's background, every lamp pixel one of the theme's
//! sixteen exactly and each lit lamp at 3:1 against the plate; a light
//! that changes naming another picture; nothing a program sends moving
//! it; and DEC's own status line as it was.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::lights::{
    LINE_MARGIN, PLATE_INSET, PLATE_RADIUS_LINE, draw_led, draw_plate, layout, line_picture,
    line_places, plate_span,
};
use baud_render::{
    Ansi, Argb, Backdrop, Blend, ByLed, Damage, Glow, Halo, Led, Light, LightsColours, LightsImage,
    LightsLook, LineEdge, LineRows, PxSize, Renderer, Shown, Stack, Target, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 60,
    ws_row: 8,
};

/// The scale the line is drawn at here: a Retina display's.
const SCALE: f32 = 2.0;

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], px: f32, theme: Theme) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(px).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// One CPU frame of the whole -- the grid's frame and the line -- at
/// `damage`, and its width and height.
fn whole(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, damage: Damage) -> (Vec<u32>, usize, usize) {
    r.fit(t);
    let (w, h) = r.whole_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(t, &mut target, damage);
    }
    let n = |p: Px| usize::try_from(p.value()).unwrap();
    (px, n(w), n(h))
}

/// The rows `top..top + h` of a frame `w` wide.
fn rows(px: &[u32], w: usize, top: Px, h: Px) -> Vec<u32> {
    let (a, b) = (
        usize::try_from(top.value()).unwrap(),
        usize::try_from(top.value() + h.value()).unwrap(),
    );
    px[a * w..b * w].to_vec()
}

fn rd_lit() -> Shown {
    ByLed {
        rd: Light::Lit,
        oh: Light::Lit,
        cd: Light::Unread,
        ..Shown::DARK
    }
}

/// The look the renderer makes for its theme, as the title strip's is.
fn look_of(r: &Renderer<'_, '_>, glow: Glow) -> LightsLook {
    r.lights_look(glow)
}

/// FNV-1a over the pixels, as `golden.rs` pins its frames.
fn digest(px: &[u32]) -> u64 {
    px.iter().fold(0xcbf2_9ce4_8422_2325, |h, &p| {
        p.to_le_bytes()
            .iter()
            .fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
    })
}

/// **The line is the lights' picture and its margins, and no row**: 11
/// points of picture and 2 of margin above and below, 30 pixels at 2x and
/// 15 at 1x, whatever the font's size, so the frame is that much taller
/// and holds the same rows -- at the 13 point default, at 2x a 26-pixel
/// font, less than a text row; the line's top is the frame's bottom, or
/// the whole's top with the grid below it. Perturbation: size the line
/// from the cell -- it grows with the font -- and this goes red.
#[test]
fn the_line_is_the_lights_picture_and_its_margins_and_no_row() {
    assert_eq!(line_picture(), 11.0);
    assert_eq!(LINE_MARGIN, 2.0);
    let at2 = LineRows::at(2.0).unwrap();
    assert_eq!(
        (at2.picture, at2.margin, at2.height()),
        (Px::of(22), Px::of(4), Px::of(30))
    );
    assert_eq!(LineRows::at(1.0).unwrap().height(), Px::of(15));
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let t = Term::new(owned.storage(), &WS);
    for px in [14.0, 26.0, 40.0] {
        let mut r = renderer(&faces, px, Theme::dark());
        r.fit(&t);
        let (frame, plain, grid) = (r.frame_size(), r.whole_size(), r.grid());
        assert_eq!(r.lights_height(), Px::of(0), "no line by default");
        for edge in [LineEdge::Bottom, LineEdge::Top] {
            r.set_lights_line(Some(edge), SCALE);
            r.fit(&t);
            assert_eq!(r.lights_edge(), Some(edge));
            assert_eq!(r.lights_height(), Px::of(30), "{px}: the points' pixels");
            assert_eq!(r.frame_size(), frame, "{px}: the grid's frame is its own");
            assert_eq!(r.grid(), grid, "{px}: no row more");
            assert_eq!(r.whole_size(), (plain.0, Px::of(plain.1.value() + 30)));
            let (line_top, grid_top) = match edge {
                LineEdge::Bottom => (frame.1, Px::of(0)),
                LineEdge::Top => (Px::of(0), Px::of(30)),
            };
            assert_eq!(
                (r.lights_top(), r.grid_top()),
                (line_top, grid_top),
                "{edge:?}"
            );
        }
        if px == 26.0 {
            // JetBrains Mono at the 13 point default on a 2x display: a
            // 34-pixel row, four more than the line.
            assert_eq!(r.metrics().cell_h, Px::of(34), "the default's row");
        }
        r.set_lights_line(None, SCALE);
        assert_eq!(r.whole_size(), plain, "{px}: off gives the pixels back");
    }
}

/// **The line's lights are the title strip's pictures, on its plate**: on
/// a CPU frame, each placed light's rectangle on the line holds the
/// picture `draw_led` draws for its state in the theme's look, the same
/// function the title strip's views show, opaque where the picture is and
/// the plate where it is clear and the plate covers wholly; the plate is
/// `draw_plate`'s from the first light less its pad to the last and its
/// pad, a point in from the line's top and bottom; the rest of the line is
/// the page's clear colour; and the line's pixels are pinned, as
/// `golden.rs` pins frames.
#[test]
fn the_lines_lights_are_the_title_strips_pictures() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"$ ls\r\nfiles\r\n$ ");
    let mut r = renderer(&faces, 26.0, Theme::dark());
    r.set_lights_line(Some(LineEdge::Bottom), SCALE);
    r.set_lights(rd_lit(), Glow::Full);
    let (px, w, _) = whole(&mut r, &mut t, Damage::Full);
    let line = rows(&px, w, r.lights_top(), r.lights_height());
    let clear = r.clear_color().0;
    let look = look_of(&r, Glow::Full);
    let margin = usize::try_from(LineRows::at(SCALE).unwrap().margin.value()).unwrap();
    let mut covered = vec![false; line.len()];
    let mut opaque = 0;
    // The plate: its picture, and where the line holds it.
    let (at, _) = layout(&faces[0], SCALE).unwrap();
    let xs = ByLed::of(|l| r.lights_places().get(l).map(|p| p.0));
    let (px0, pw) = plate_span(&at, &xs, SCALE).expect("a plate");
    let inset_px = u32::try_from((PLATE_INSET * SCALE).round() as i64).unwrap();
    let inset = usize::try_from(inset_px).unwrap();
    let ph = r.lights_height().value() - 2 * inset_px;
    let ph_rows = usize::try_from(ph).unwrap();
    let mut plate = LightsImage::new();
    draw_plate(&look, pw, Px::of(ph), PLATE_RADIUS_LINE * SCALE, &mut plate).unwrap();
    let plate_at = |i: usize| {
        let (x, y) = (i % w, i / w);
        let x0 = usize::try_from(px0.value()).unwrap();
        let pw = usize::try_from(pw.value()).unwrap();
        (x >= x0 && x < x0 + pw && y >= inset && y < inset + ph_rows)
            .then(|| plate.pixels[(y - inset) * pw + (x - x0)])
    };
    for (led, place) in r.lights_places().iter() {
        let (x, pw) = place.expect("every light on a 60-column line");
        let mut img = LightsImage::new();
        draw_led(
            &faces[0],
            &look,
            led,
            *rd_lit().get(led),
            SCALE,
            line_picture(),
            &mut img,
        )
        .unwrap();
        assert_eq!(img.width, pw, "{led:?}: its slot's width");
        let (iw, ih) = (
            usize::try_from(img.width.value()).unwrap(),
            usize::try_from(img.height.value()).unwrap(),
        );
        let x = usize::try_from(x.value()).unwrap();
        for y in 0..ih {
            for i in 0..iw {
                let at = (margin + y) * w + x + i;
                covered[at] = true;
                let p = img.pixels[y * iw + i];
                match p.a().get() {
                    0xFF => {
                        opaque += 1;
                        assert_eq!(line[at], p.0, "{led:?} ({i}, {y}) opaque");
                    }
                    0 => match plate_at(at) {
                        Some(p) if p.a().get() == 0xFF => {
                            assert_eq!(line[at], look.plate.0, "{led:?} ({i}, {y}) plate")
                        }
                        Some(_) => {}
                        None => assert_eq!(line[at], clear, "{led:?} ({i}, {y}) clear"),
                    },
                    _ => {}
                }
            }
        }
    }
    assert!(
        opaque > 500,
        "the control: the pictures reached the line, {opaque}"
    );
    let mut on_plate = 0;
    for (i, &p) in line.iter().enumerate() {
        if covered[i] {
            continue;
        }
        match plate_at(i) {
            Some(q) if q.a().get() == 0xFF => {
                on_plate += 1;
                assert_eq!(p, look.plate.0, "pixel {i} of the line, on the plate");
            }
            Some(_) => {}
            None => assert_eq!(p, clear, "pixel {i} of the line, off the plate"),
        }
    }
    assert!(on_plate > 1000, "the control: the plate, {on_plate}");
    assert_eq!(
        digest(&line),
        4_123_545_115_173_808_163,
        "the line's pixels"
    );
}

/// **DEC's status line sits between the grid and the lights' line, and
/// nothing a program sends moves the line**: with a host-writable status
/// line shown (DECSSDT 2, its text written under DECSASD 1), the frame is
/// the grid and the status row, with its text, and the line below both; a
/// program taking the status line away (DECSSDT 0), showing the
/// indicator (DECSSDT 1), a soft reset (DECSTR) and a hard one (RIS) each
/// leave the line's pixels as they were, at the bottom of whatever the
/// frame holds. Perturbation: put the line above the status row -- the
/// status text is drawn on the line's rows -- and this goes red.
#[test]
fn the_status_line_sits_between_the_grid_and_the_line_and_programs_leave_it() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces, 26.0, Theme::dark());
    r.set_lights_line(Some(LineEdge::Bottom), SCALE);
    r.set_lights(rd_lit(), Glow::Full);
    feed(&mut t, b"\x1b[2$~\x1b[1$}STATUS TEXT\x1b[0$}");
    let (px, w, h) = whole(&mut r, &mut t, Damage::Full);
    let ch = r.metrics().cell_h;
    let main = r.main_rows();
    assert_eq!(
        t.status_rows(),
        Rows::of_count(1),
        "the status line is shown"
    );
    let status_top = Px::of(u32::try_from(main.index()).unwrap() * ch.value());
    assert_eq!(
        r.lights_top(),
        Px::of(status_top.value() + ch.value()),
        "below the status row"
    );
    assert_eq!(h, usize::try_from(r.lights_top().value() + 30).unwrap());
    let clear = r.clear_color().0;
    let status = rows(&px, w, status_top, ch);
    assert!(
        status.iter().any(|&p| p != clear),
        "the status text is drawn on its row"
    );
    let line = rows(&px, w, r.lights_top(), r.lights_height());
    for (what, bytes) in [
        ("DECSSDT 0", &b"\x1b[0$~"[..]),
        ("DECSSDT 1", b"\x1b[1$~"),
        ("DECSTR", b"\x1b[!p"),
        ("RIS", b"\x1bc"),
        ("DECSSDT 2", b"\x1b[2$~"),
    ] {
        feed(&mut t, bytes);
        let (px, w, h) = whole(&mut r, &mut t, Damage::Full);
        assert_eq!(r.lights_height(), Px::of(30), "{what}: the line's height");
        assert_eq!(
            h,
            usize::try_from(r.frame_size().1.value() + 30).unwrap(),
            "{what}"
        );
        assert_eq!(r.lights_top(), r.frame_size().1, "{what}: at the bottom");
        assert_eq!(
            rows(&px, w, r.lights_top(), r.lights_height()),
            line,
            "{what}: the line's pixels"
        );
    }
}

/// **A light that changes names another picture and draws nothing**: with
/// every picture of the look drawn on the first frame, a frame where only
/// RD went dark moves no sheet -- the atlas's generation stays -- changes
/// pixels only inside RD's rectangle, and says it repainted the line; a
/// frame where nothing changed repaints no pixel of it. Perturbation: draw
/// only the state shown -- RD's dark picture is placed on the second frame
/// and the generation moves -- and this goes red.
#[test]
fn a_light_that_changes_names_another_picture_and_draws_nothing() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces, 26.0, Theme::dark());
    r.set_lights_line(Some(LineEdge::Bottom), SCALE);
    r.set_lights(rd_lit(), Glow::Full);
    let (fw, fh) = r.whole_size();
    let mut px = vec![0u32; usize::try_from(fw.value() * fh.value()).unwrap()];
    let mut frame = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, d: Damage| {
        let mut target = Target::new(&mut px, fw, fw, fh).unwrap();
        r.render(t, &mut target, d);
        let line = rows(
            &px,
            usize::try_from(fw.value()).unwrap(),
            r.lights_top(),
            r.lights_height(),
        );
        (line, r.painted().collect::<Vec<_>>())
    };
    let (before, _) = frame(&mut r, &mut t, Damage::Full);
    let generation = r.atlas().generation();
    let (rd_x, rd_w) = r.lights_places().rd.unwrap();
    r.set_lights(
        ByLed {
            rd: Light::Dark,
            ..rd_lit()
        },
        Glow::Full,
    );
    let (after, painted) = frame(&mut r, &mut t, Damage::Incremental);
    assert_eq!(
        r.atlas().generation(),
        generation,
        "nothing placed on the sheet"
    );
    let w = usize::try_from(fw.value()).unwrap();
    let mut changed = 0;
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        if a != b {
            changed += 1;
            let x = i % w;
            let (x0, x1) = (
                usize::try_from(rd_x.value()).unwrap(),
                usize::try_from(rd_x.value() + rd_w.value()).unwrap(),
            );
            assert!((x0..x1).contains(&x), "pixel {i} outside RD's rectangle");
        }
    }
    assert!(changed > 50, "the control: RD went dark, {changed} pixels");
    let line = baud_render::Band {
        top: r.lights_top(),
        bottom: Px::of(r.lights_top().value() + 30),
    };
    assert!(painted.contains(&line), "the line repainted: {painted:?}");
    let (_, painted) = frame(&mut r, &mut t, Damage::Incremental);
    assert!(!painted.contains(&line), "nothing changed: {painted:?}");
}

/// **A top line moves the grid's frame down by its height and nothing
/// else**: the whole frame with the line on top is the line, then the
/// frame drawn with no line at all, row for row; and the rows a later
/// frame repaints are the line-less frame's, that much lower.
/// Perturbation: paint the grid at the whole's top -- the line covers its
/// first rows -- and this goes red.
#[test]
fn a_top_line_moves_the_grid_down_by_its_height() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l$ ls\r\nfiles\r\n$ ");
    let mut bare = renderer(&faces, 26.0, Theme::dark());
    let mut top = renderer(&faces, 26.0, Theme::dark());
    top.set_lights_line(Some(LineEdge::Top), SCALE);
    top.set_lights(rd_lit(), Glow::Full);
    let (a, w, h) = whole(&mut bare, &mut t, Damage::Full);
    let (b, w2, h2) = whole(&mut top, &mut t, Damage::Full);
    assert_eq!((w2, h2), (w, h + 30));
    assert_eq!(&b[30 * w..], &a[..], "the grid's frame, 30 rows down");
    let clear = top.clear_color().0;
    assert!(
        b[..30 * w].iter().any(|&p| p != clear),
        "the lights on the top line"
    );
    feed(&mut t, b"more");
    let _ = whole(&mut bare, &mut t, Damage::Incremental);
    let _ = whole(&mut top, &mut t, Damage::Incremental);
    let shifted: Vec<_> = bare
        .painted()
        .map(|b| (b.top.value() + 30, b.bottom.value() + 30))
        .collect();
    let lined: Vec<_> = top
        .painted()
        .map(|b| (b.top.value(), b.bottom.value()))
        .collect();
    assert!(!shifted.is_empty(), "the control: the text repainted a row");
    assert_eq!(lined, shifted);
}

/// **A narrow window drops the leftmost lights first**, as the title strip
/// does: on a line of 300 pixels at 2x the strip, 516 pixels wide, does not
/// fit, so the lights that would start within the line's end gap of its
/// left are left out and the rest keep their places from the right end;
/// the rightmost is always placed. The same rule on the renderer's line:
/// a 20-column grid keeps SYN and drops HS.
#[test]
fn a_narrow_line_drops_the_leftmost_lights_first() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let (at, strip) = baud_render::lights::layout(&faces[0], SCALE).unwrap();
    let end = Px::of(18);
    let wide = line_places(Px::of(2000), &at, strip, end);
    assert!(Led::ALL.iter().all(|&l| wide.get(l).is_some()));
    let narrow = line_places(Px::of(300), &at, strip, end);
    let kept: Vec<Led> = Led::ALL
        .into_iter()
        .filter(|&l| narrow.get(l).is_some())
        .collect();
    assert!(!kept.is_empty() && kept.len() < 9, "{kept:?}");
    assert_eq!(kept.last(), Some(&Led::Syn), "the rightmost stays");
    let first = Led::ALL
        .iter()
        .position(|&l| narrow.get(l).is_some())
        .unwrap();
    assert!(
        Led::ALL[first..].iter().all(|&l| narrow.get(l).is_some()),
        "the leftmost go"
    );
    for &l in &kept {
        let (n, w) = (narrow.get(l).unwrap().value(), wide.get(l).unwrap().value());
        assert_eq!(2000 - w, 300 - n, "{l:?} keeps its place from the right");
        assert!(n >= end.value());
    }
    let mut owned = Owned::new(&WS);
    let t = Term::new(owned.storage(), &WS);
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(26.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(20),
        Rows::of_index(4),
    )
    .unwrap();
    r.set_lights_line(Some(LineEdge::Bottom), SCALE);
    r.set_lights(Shown::DARK, Glow::Full);
    r.place_lights(&t);
    let p = r.lights_places();
    assert!(p.syn.is_some() && p.hs.is_none(), "{p:?}");
    assert_eq!(
        r.lights_records().len(),
        1 + Led::ALL.iter().filter(|&&l| p.get(l).is_some()).count(),
        "the plate, then the lights placed"
    );
}

/// One frame of the line as the app draws it at 2x in `theme` with the
/// lights in `set` at `light` and `glow`, at the page's `alpha`, in
/// `video`: the line's rows, its width, where each light is, and the look.
#[allow(clippy::too_many_arguments)]
fn set_line(
    faces: &[Face<'_>],
    theme: &Theme,
    alpha: Chan,
    set: LightsColours,
    halo: Halo,
    light: Light,
    glow: Glow,
    video: Video,
) -> (AppLine, LightsLook) {
    let mut theme = theme.clone();
    theme.bg_alpha = alpha;
    let mut r = renderer(faces, 26.0, theme);
    r.set_lights_colours(set, halo);
    r.set_lights_line(Some(LineEdge::Bottom), SCALE);
    r.set_lights(Shown::all(light), glow);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    if video == Video::Reverse {
        feed(&mut t, b"\x1b[?5h");
    }
    let (px, w, _) = whole(&mut r, &mut t, Damage::Full);
    let line = rows(&px, w, r.lights_top(), r.lights_height());
    ((line, w, r.lights_places()), r.lights_look(glow))
}

/// **Every full-coverage lamp pixel on the line is exactly its palette
/// entry, on a plate that is the theme's background**: on the CPU frame,
/// for every set, both built-in themes and xterm's sixteen on white,
/// opaque and at the dark dim, in both videos, every state, both glows
/// and the strongest halo, each pixel the lamp's dot, half-size dot or
/// ring covers wholly is the entry its set gives it and one of the
/// theme's sixteen, and the line's pixel just right of SYN's picture, on
/// the plate, is the theme's background, opaque, though the page round
/// the plate is translucent or reversed. The controls: the mask moved 40
/// pixels finds pixels that are not the lamp's, and the band off the plate
/// at the dim is not the plate. Perturbation: blend the line's pictures
/// with a tint, or draw the plate at the page's alpha, and this goes red.
#[test]
fn every_lamp_pixel_on_the_line_is_its_palette_entry_on_the_background() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let (at, _) = layout(&faces[0], SCALE).unwrap();
    let mut xterm = Theme::light();
    xterm.palette = baud_render::xterm_palette();
    let margin = LineRows::at(SCALE).unwrap().margin.value();
    let mut read = 0;
    for (name, theme) in [
        ("dark", Theme::dark()),
        ("light", Theme::light()),
        ("xterm on white", xterm),
    ] {
        let sixteen: Vec<u32> = Ansi::ALL.iter().map(|a| a.of(&theme.palette).0).collect();
        for set in LightsColours::ALL {
            for alpha in [Chan::from_u8(0xFF), Chan::from_u8(179)] {
                for video in [Video::Normal, Video::Reverse] {
                    for (glow, halo) in
                        [(Glow::Full, Halo::of(1.0)), (Glow::Dimmed, Halo::default())]
                    {
                        for light in Light::ALL {
                            let ((line, w, places), look) =
                                set_line(&faces, &theme, alpha, set, halo, light, glow, video);
                            let mut moved = 0;
                            for led in Led::ALL {
                                let (x0, _) = places.get(led).expect("placed");
                                let bare = LightsLook {
                                    label: Argb(0),
                                    glow: Glow::Dimmed,
                                    ..look
                                };
                                let mut img = LightsImage::new();
                                draw_led(
                                    &faces[0],
                                    &bare,
                                    led,
                                    light,
                                    SCALE,
                                    line_picture(),
                                    &mut img,
                                )
                                .unwrap();
                                let want = match light {
                                    Light::Lit => *look.lit.get(led),
                                    Light::Dark => look.unlit,
                                    Light::Unread => look.ring,
                                };
                                let iw = img.width.value();
                                for (i, p) in img.pixels.iter().enumerate() {
                                    if p.a().get() != 0xFF {
                                        continue;
                                    }
                                    let i = u32::try_from(i).unwrap();
                                    let (x, y) = (x0.value() + i % iw, margin + i / iw);
                                    let got = line[usize::try_from(y).unwrap() * w
                                        + usize::try_from(x).unwrap()];
                                    assert_eq!(
                                        got, want.0,
                                        "{name} {set:?} {video:?} {light:?} {led:?}"
                                    );
                                    assert!(sixteen.contains(&got), "{name} {set:?}: {got:08X}");
                                    let shifted = line[usize::try_from(y).unwrap() * w
                                        + usize::try_from(x + 40).unwrap()];
                                    moved += usize::from(shifted != want.0);
                                    read += 1;
                                }
                            }
                            assert!(moved > 0, "{name} {set:?}: the moved mask fires");
                            // On the plate, just right of SYN's picture, mid line.
                            let (sx, _) = places.syn.unwrap();
                            let x = usize::try_from(sx.value() + at.syn.width.value()).unwrap() + 2;
                            let y = line.len() / w / 2;
                            assert_eq!(
                                line[y * w + x],
                                look.plate.0,
                                "{name} {set:?} a{} {video:?}: the plate",
                                alpha.get()
                            );
                            assert_eq!(look.plate, theme.bg.with_alpha(Chan::from_u8(0xFF)));
                            if alpha.get() < 0xFF {
                                assert_ne!(
                                    line[0], look.plate.0,
                                    "the control: the band is not the plate"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(read > 100_000, "the scan read the lamps, {read}");
}

/// **DEC's status line is as it was**: the indicator line (DECSSDT 1) with
/// the state lights' words on it (`status_line_lights = "states"`, SYN and
/// CD lit, OH unknown), with the lights' own line under it in every set,
/// draws the status row's pixels pinned before the lights took the
/// palette's entries, in both themes: the words keep their look and no
/// plate or halo reaches DEC's line. Perturbation: draw the words in the
/// set's lit entries and this goes red.
#[test]
fn decs_status_line_is_as_it_was() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut got = Vec::new();
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let mut row = None;
        for set in LightsColours::ALL {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut r = renderer(&faces, 26.0, theme.clone());
            r.set_lights_colours(set, Halo::of(1.0));
            r.set_lights_line(Some(LineEdge::Bottom), SCALE);
            r.set_lights(Shown::all(Light::Lit), Glow::Full);
            feed(&mut t, b"\x1b[1$~");
            t.set_indicator_lamps(Some(baud_core::IndicatorLamps {
                syn: baud_core::LampState::Lit,
                cd: baud_core::ReadLamp::Read(baud_core::LampState::Lit),
                oh: baud_core::ReadLamp::Unknown,
                ..baud_core::IndicatorLamps::DARK
            }));
            let (px, w, _) = whole(&mut r, &mut t, Damage::Full);
            let ch = r.metrics().cell_h;
            let main = r.main_rows();
            let top = Px::of(u32::try_from(main.index()).unwrap() * ch.value());
            let status = rows(&px, w, top, ch);
            assert!(
                status.iter().any(|&p| p != status[0]),
                "the words are drawn"
            );
            match &row {
                None => row = Some(status),
                Some(first) => assert_eq!(first, &status, "{name} {set:?}: one row for every set"),
            }
        }
        got.push((name, digest(&row.unwrap())));
    }
    assert_eq!(got, [("dark", DEC_DARK), ("light", DEC_LIGHT)]);
}

/// The status row's digests, taken on the build before the lights' colours
/// came from the palette.
const DEC_DARK: u64 = 11_289_522_458_324_036_478;
const DEC_LIGHT: u64 = 14_633_064_495_723_483_375;

/// **The line's pixels in every state, both themes and both glows**,
/// pinned from the line as it is drawn on its plate in the theme's
/// sixteen: the line's lamps are palette entries, its own, on both
/// systems. Perturbation: lift the line's lit colours to 5:1, or mix its
/// lamps from the letters' colour, and this goes red.
#[test]
fn the_lines_pixels_are_pinned_in_every_state() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut got = Vec::new();
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for glow in [Glow::Full, Glow::Dimmed] {
            for light in Light::ALL {
                let mut owned = Owned::new(&WS);
                let mut t = Term::new(owned.storage(), &WS);
                let mut r = renderer(&faces, 26.0, theme.clone());
                r.set_lights_line(Some(LineEdge::Bottom), SCALE);
                r.set_lights(Shown::all(light), glow);
                let (px, w, _) = whole(&mut r, &mut t, Damage::Full);
                let line = rows(&px, w, r.lights_top(), r.lights_height());
                got.push((name, glow, light, digest(&line)));
            }
        }
    }
    let want = [
        ("dark", Glow::Full, Light::Dark, 7_849_512_638_526_911_931),
        ("dark", Glow::Full, Light::Lit, 5_312_797_589_331_973_419),
        ("dark", Glow::Full, Light::Unread, 402_903_640_830_943_323),
        ("dark", Glow::Dimmed, Light::Dark, 7_849_512_638_526_911_931),
        ("dark", Glow::Dimmed, Light::Lit, 1_018_366_718_110_630_459),
        ("dark", Glow::Dimmed, Light::Unread, 402_903_640_830_943_323),
        ("light", Glow::Full, Light::Dark, 14_218_837_881_295_335_315),
        ("light", Glow::Full, Light::Lit, 8_291_315_892_717_512_407),
        (
            "light",
            Glow::Full,
            Light::Unread,
            8_845_114_105_503_958_811,
        ),
        (
            "light",
            Glow::Dimmed,
            Light::Dark,
            14_218_837_881_295_335_315,
        ),
        ("light", Glow::Dimmed, Light::Lit, 7_907_669_469_043_253_295),
        (
            "light",
            Glow::Dimmed,
            Light::Unread,
            8_845_114_105_503_958_811,
        ),
    ];
    assert_eq!(got, want);
}

/// The app's two faces: the text face and the bold the line's letters
/// are drawn in, the badge face.
fn app_faces() -> (Vec<u8>, Vec<u8>) {
    let built = |f: &str| {
        std::fs::read(format!(
            "{}/../baud-app/fonts/built/{f}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    };
    (built("Regular.ttf"), built("Bold.ttf"))
}

/// The two built-in themes at the dims the app ships, `dim_dark` 0.70
/// and `dim_light` 0.60, each with the standard material's greys under
/// it, the backdrop macOS reads its text against, and opaque.
fn shipped() -> [(&'static str, Theme, Chan, Backdrop); 4] {
    let opaque = Chan::from_u8(0xFF);
    [
        ("dark", Theme::dark(), opaque, Backdrop::under_window_dark()),
        (
            "dark",
            Theme::dark(),
            Chan::from_unit(0.70),
            Backdrop::under_window_dark(),
        ),
        (
            "light",
            Theme::light(),
            opaque,
            Backdrop::under_window_light(),
        ),
        (
            "light",
            Theme::light(),
            Chan::from_unit(0.60),
            Backdrop::under_window_light(),
        ),
    ]
}

/// The line's rows, its width, and where each light is on it.
type AppLine = (Vec<u32>, usize, ByLed<Option<(Px, Px)>>);

/// The screen's video: normal, or reversed by `?5` (DECSCNM).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Video {
    Normal,
    Reverse,
}

/// One frame as the app draws it at 2x, the 13 point type and the line's
/// letters in the badge face, the lights in `set` at `light` and `glow`,
/// in reverse video when `video` is [`Video::Reverse`]: the line's rows,
/// the line's width, and where each light is.
#[allow(clippy::too_many_arguments)]
fn app_line(
    faces: &[Face<'_>],
    theme: &Theme,
    alpha: Chan,
    backdrop: Backdrop,
    set: LightsColours,
    light: Light,
    glow: Glow,
    video: Video,
) -> AppLine {
    use baud_render::{FaceRole, FaceStyle};
    let mut theme = theme.clone();
    theme.bg_alpha = alpha;
    theme.backdrop = backdrop;
    let stack = Stack::with_roles(
        faces,
        &[
            FaceRole::Fallback,
            FaceRole::Style(FaceStyle::Bold),
            FaceRole::Badge,
        ],
    )
    .unwrap();
    let mut r = Renderer::new(
        stack,
        PxSize::of(13.0 * SCALE).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    r.set_lights_colours(set, Halo::default());
    r.set_lights_line(Some(LineEdge::Bottom), SCALE);
    r.set_lights(Shown::all(light), glow);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    if video == Video::Reverse {
        feed(&mut t, b"\x1b[?5h");
    }
    let (px, w, _) = whole(&mut r, &mut t, Damage::Full);
    let line = rows(&px, w, r.lights_top(), r.lights_height());
    (line, w, r.lights_places())
}

/// **The line draws the same pixels on macOS and on Linux**: its colours
/// are the theme's, lifted against anything from black to white behind
/// the page, on both systems, though the text of the window is read
/// against the standard material's narrower greys on macOS. The lights'
/// line on macOS's default, the standard material, is byte for byte the
/// line on Linux's, whose material is not modelled, in both themes at the
/// shipped dims, both videos, both glows and every state. Perturbation:
/// lift the line against the theme's backdrop again, and the dark theme's
/// state lights are #3771FF on macOS where Linux draws #8DA2FF, and this
/// goes red.
#[test]
fn the_line_draws_the_same_pixels_on_macos_and_on_linux() {
    let (reg, bold) = app_faces();
    let faces = [
        Face::parse(&reg).unwrap(),
        Face::parse(&bold).unwrap(),
        Face::parse(&bold).unwrap(),
    ];
    let mut differ = Vec::new();
    let mut compared = 0;
    for (name, theme, alpha, material) in shipped() {
        for video in [Video::Normal, Video::Reverse] {
            for glow in [Glow::Full, Glow::Dimmed] {
                for light in Light::ALL {
                    let set = LightsColours::Meaning;
                    let line = |b| app_line(&faces, &theme, alpha, b, set, light, glow, video).0;
                    let (macos, linux) = (line(material), line(Backdrop::any()));
                    assert!(macos.iter().any(|&p| p != macos[0]), "something is drawn");
                    compared += 1;
                    if macos != linux {
                        differ.push((name, alpha.get(), video, glow, light));
                    }
                }
            }
        }
    }
    assert_eq!(compared, 48);
    assert_eq!(differ, [], "lines that differ between the two systems");
}

/// WCAG 2.2's relative luminance of an opaque encoded sRGB colour
/// (`[[source: wcag22]]`).
fn luminance(c: [u8; 3]) -> f64 {
    let l = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
}

/// WCAG 2.2's contrast ratio of two opaque colours.
fn ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// A drawn pixel as the window shows it over grey `g`: premultiplied by
/// its alpha, then source-over the grey, in eight bits.
fn shown(p: u32, g: u8) -> [u8; 3] {
    let [a, r, gr, b] = p.to_be_bytes();
    let o = |c: u8| {
        let pm = baud_domain::premultiply_chan(Chan::from_u8(c), Chan::from_u8(a)).get();
        let v = f64::from(pm) + f64::from(g) * (1.0 - f64::from(a) / 255.0);
        Chan::from_unit((v / 255.0) as f32).get()
    };
    [o(r), o(gr), o(b)]
}

/// **On the drawn pixels, each lit lamp reads at 3:1 against the plate and
/// the letters at 4.5:1, over every grey behind the glass**: SC 1.4.11's
/// minimum for a component's state and SC 1.4.3's for text
/// (`[[source: wcag22]]`), read where the window shows them -- the dot's
/// centre and the letters' most covered pixel, coverage included, each
/// over grey 0 to 255 against the plate under it -- in every set, both
/// themes, opaque and at the shipped dims, in both videos, at 2x. The
/// plate is the theme's background and opaque, so neither the dim nor
/// reverse video moves what a lamp reads against. Perturbation: quantise
/// coverage by truncation, or draw the plate at the page's alpha, and
/// this goes red.
#[test]
fn the_lines_drawn_lamps_read_at_three_to_one_and_letters_at_four_and_a_half() {
    let (reg, bold) = app_faces();
    let faces = [
        Face::parse(&reg).unwrap(),
        Face::parse(&bold).unwrap(),
        Face::parse(&bold).unwrap(),
    ];
    let mut short = Vec::new();
    let mut lamps = 0;
    for (name, theme, alpha, material) in shipped() {
        for (video, set) in [Video::Normal, Video::Reverse]
            .into_iter()
            .flat_map(|v| LightsColours::ALL.map(|s| (v, s)))
        {
            let frame = |light| {
                app_line(
                    &faces,
                    &theme,
                    alpha,
                    material,
                    set,
                    light,
                    Glow::Full,
                    video,
                )
            };
            let (lit, w, places) = frame(Light::Lit);
            let (dark, _, _) = frame(Light::Dark);
            let h = lit.len() / w;
            for led in Led::ALL {
                let (x, _) = places.get(led).expect("every light is placed");
                let (x0, slot) = (
                    usize::try_from(x.value()).unwrap(),
                    usize::try_from(places.get(led).unwrap().1.value()).unwrap(),
                );
                // The dot: where the lit light differs from the dark one.
                let dot: Vec<(usize, usize)> = (0..h)
                    .flat_map(|y| (x0..x0 + slot).map(move |x| (x, y)))
                    .filter(|&(x, y)| lit[y * w + x] != dark[y * w + x])
                    .collect();
                let lit = &lit;
                let left = dot.iter().map(|p| p.0).min().unwrap();
                let (right, top, bottom) = (
                    dot.iter().map(|p| p.0).max().unwrap(),
                    dot.iter().map(|p| p.1).min().unwrap(),
                    dot.iter().map(|p| p.1).max().unwrap(),
                );
                let core = lit[(top + bottom) / 2 * w + (left + right) / 2];
                // The plate, mid line, in the gap left of the light.
                let band = lit[h / 2 * w + x0 - 2];
                let letters: Vec<u32> = (0..h)
                    .flat_map(|y| (x0..left - 1).map(move |x| lit[y * w + x]))
                    .collect();
                for g in 0..=255u8 {
                    let under = shown(band, g);
                    let lamp = ratio(shown(core, g), under);
                    let text = letters
                        .iter()
                        .map(|&p| ratio(shown(p, g), under))
                        .fold(0.0, f64::max);
                    lamps += 1;
                    if lamp < 3.0 {
                        short.push(format!(
                            "{name} a{} {video:?} {set:?} {led:?} lamp {lamp:.4} at grey {g}",
                            alpha.get()
                        ));
                    }
                    if text < 4.5 {
                        short.push(format!(
                            "{name} a{} {video:?} {set:?} {led:?} letters {text:.4} at grey {g}",
                            alpha.get()
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(lamps, 4 * 2 * 3 * 9 * 256);
    assert_eq!(short, Vec::<String>::new());
}
