//! **The activity lights on a line of their own** (`[window]
//! activity_lights = "bottom_line"` or `"top_line"`): a band of the whole
//! frame outside the grid, only as tall as the lights' picture and its
//! margins, below the grid, DEC's status line and the notice -- the VT420's
//! keyboard indicator line, "at the bottom of the screen, below the status
//! line" -- or above the grid; the title strip's pictures drawn into it in
//! the theme's colours, each lit light at 3:1 against it; a light that
//! changes naming another picture; and nothing a program sends moving it.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::lights::{LINE_MARGIN, draw_led, line_colours, line_picture, line_places};
use baud_render::{
    Argb, Backdrop, Blend, ByLed, Damage, Glow, Led, Light, LightsImage, LightsLook, LineEdge,
    LineRows, PxSize, Renderer, Shown, Stack, Target, Theme, contrast_over,
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

/// The look the renderer makes for its theme: `line_colours` over the
/// page it clears to against every grey, the letters at the floor's
/// 4.5:1, dimmed caps over every grey.
fn look_of(r: &Renderer<'_, '_>, glow: Glow) -> LightsLook {
    let theme = r.theme();
    let page = r.clear_color().with_alpha(Chan::from_u8(0xFF));
    let (lit, label) = line_colours(
        &theme.palette,
        theme.fg,
        page,
        theme.bg_alpha,
        Backdrop::any(),
        4.5,
    );
    LightsLook::new(lit, lit, label, page, glow, Backdrop::any())
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

/// **The line's lights are the title strip's pictures**: on a CPU frame,
/// each placed light's rectangle on the line holds the picture
/// `draw_led` draws for its state in the theme's look, the same function
/// the title strip's views show, opaque where the picture is and the
/// page's clear colour where it is clear; the rest of the line is that
/// colour; and the line's pixels are pinned, as `golden.rs` pins frames.
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
                    0 => assert_eq!(line[at], clear, "{led:?} ({i}, {y}) clear"),
                    _ => {}
                }
            }
        }
    }
    assert!(
        opaque > 500,
        "the control: the pictures reached the line, {opaque}"
    );
    for (i, &p) in line.iter().enumerate() {
        if !covered[i] {
            assert_eq!(p, clear, "pixel {i} of the line, off every light");
        }
    }
    assert_eq!(
        digest(&line),
        9_605_429_306_933_611_913,
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
        Led::ALL.iter().filter(|&&l| p.get(l).is_some()).count()
    );
}

/// **Each lit light reads at 3:1 against the line, and the letters at
/// 4.5:1**, under the dark and light themes and under xterm's sixteen on
/// a black and a white page, where xterm's yellow on white reads 1.70:1
/// as asked, and under the two themes over the glass at the shipped dims,
/// against anything from black to white behind them: SC 1.4.11's minimum for a component's state, and SC 1.4.3's
/// for text. Each keeps the hue it was asked in, so in, out and state stay
/// three colours. Perturbation: return the asked colour from
/// `line_colours` -- the yellow on white is drawn at 1.70:1 -- and this
/// goes red.
#[test]
fn each_lit_light_reads_at_three_to_one_against_the_line() {
    let opaque = Chan::from_u8(0xFF);
    let white = Argb(0xFF_FF_FF_FF);
    let black = Argb(0xFF_00_00_00);
    let xterm = baud_render::xterm_palette();
    let cases = [("dark", Theme::dark()), ("light", Theme::light())]
        .map(|(n, t)| (n, t.palette, t.fg, t.bg));
    let more = [
        ("xterm on white", xterm, black, white),
        ("xterm on black", xterm, white, black),
    ];
    let mut reached_yellow = false;
    for (name, palette, fg, page) in cases.into_iter().chain(more) {
        let page = page.with_alpha(opaque);
        let (lit, label) = line_colours(&palette, fg, page, opaque, Backdrop::any(), 4.5);
        for led in Led::ALL {
            let c = contrast_over(*lit.get(led), page, opaque, Backdrop::any());
            assert!(c >= 3.0, "{name} {led:?}: {c:.2}");
        }
        let c = contrast_over(label, page, opaque, Backdrop::any());
        assert!(c >= 4.5, "{name} letters: {c:.2}");
        assert_ne!(lit.rd, lit.sd, "{name}: in and out differ");
        assert_ne!(lit.sd, lit.oh, "{name}: out and state differ");
        if name == "xterm on white" {
            let asked = contrast_over(palette[3], page, opaque, Backdrop::any());
            assert!(
                asked < 3.0,
                "the control: xterm's yellow on white, {asked:.2}"
            );
            reached_yellow = lit.sd != palette[3];
        }
    }
    assert!(reached_yellow, "the yellow was lifted");
    // Over the glass at the shipped dims, `dim_dark` 0.70 (alpha 179) and
    // `dim_light` 0.60 (alpha 153), over anything from black to white:
    // the default Linux page, whose material is not modelled.
    for (name, theme, alpha) in [
        ("dark on glass", Theme::dark(), 179),
        ("light on glass", Theme::light(), 153),
    ] {
        let (alpha, any) = (Chan::from_u8(alpha), Backdrop::any());
        let page = theme.bg.with_alpha(opaque);
        let (lit, label) = line_colours(&theme.palette, theme.fg, page, alpha, any, 4.5);
        for led in Led::ALL {
            let c = contrast_over(*lit.get(led), page, alpha, any);
            assert!(c >= 3.0, "{name} {led:?}: {c:.2}");
        }
        let c = contrast_over(label, page, alpha, any);
        assert!(c >= 4.5, "{name} letters: {c:.2}");
    }
}

/// **The line's pixels in every state, both themes and both glows**,
/// pinned from the line as it was drawn before the title strip's colours
/// were lifted apart from it, and again when coverage came to be rounded
/// to the nearest level: the line's lamps and lit colours are one colour
/// each, its own, on both systems. Perturbation: lift the line's
/// lit colours to 5:1, or mix its lamps from the letters' colour, and this
/// goes red.
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
        ("dark", Glow::Full, Light::Dark, 6_216_219_791_535_923_001),
        ("dark", Glow::Full, Light::Lit, 9_089_374_480_762_148_789),
        (
            "dark",
            Glow::Full,
            Light::Unread,
            11_266_776_276_634_529_285,
        ),
        ("dark", Glow::Dimmed, Light::Dark, 6_216_219_791_535_923_001),
        ("dark", Glow::Dimmed, Light::Lit, 16_177_629_012_481_577_289),
        (
            "dark",
            Glow::Dimmed,
            Light::Unread,
            2_404_356_500_493_307_921,
        ),
        ("light", Glow::Full, Light::Dark, 2_193_085_022_500_298_417),
        ("light", Glow::Full, Light::Lit, 17_740_578_590_105_699_057),
        ("light", Glow::Full, Light::Unread, 386_417_699_130_134_333),
        (
            "light",
            Glow::Dimmed,
            Light::Dark,
            2_193_085_022_500_298_417,
        ),
        (
            "light",
            Glow::Dimmed,
            Light::Lit,
            14_815_031_926_811_245_793,
        ),
        (
            "light",
            Glow::Dimmed,
            Light::Unread,
            8_050_924_772_632_740_793,
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
/// letters in the badge face, at `light` and `glow`, in reverse video
/// when `video` is [`Video::Reverse`]: the line's rows, the line's width,
/// and where each light is.
#[allow(clippy::too_many_arguments)]
fn app_line(
    faces: &[Face<'_>],
    theme: &Theme,
    alpha: Chan,
    backdrop: Backdrop,
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
                    let line = |b| app_line(&faces, &theme, alpha, b, light, glow, video).0;
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

/// **On the drawn pixels, each lit lamp reads at 3:1 against the line and
/// the letters at 4.5:1, over every grey behind the glass**: SC 1.4.11's
/// minimum for a component's state and SC 1.4.3's for text
/// (`[[source: wcag22]]`), read where the window shows them -- the dot's
/// centre and the letters' most covered pixel, coverage included, each
/// over grey 0 to 255 against the band under it -- in both themes, opaque
/// and at the shipped dims, in both videos, at 2x. The lift sets each
/// colour at its target exactly, so a drawn pixel a level short of its
/// colour reads under it: the dot's inside drawn at coverage 254 read FAX
/// and SYN at 2.97 over white, and the letters at 4.4999. One arm cannot
/// reach its letters' target with any colour: the light theme reversed
/// on the glass, whose letters are white already against a band that
/// runs from #171717 to #7D7D7D; there the letters are drawn white
/// exactly, as far as the lift goes. Perturbation: quantise coverage by
/// truncation, or lift the line against the standard material's greys,
/// and this goes red.
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
        for video in [Video::Normal, Video::Reverse] {
            let frame = |light| app_line(&faces, &theme, alpha, material, light, Glow::Full, video);
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
                let band = lit[x0];
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
                            "{name} a{} {video:?} {led:?} lamp {lamp:.4} at grey {g}",
                            alpha.get()
                        ));
                    }
                    if text < 4.5
                        && !(name == "light" && video == Video::Reverse && alpha.get() < 0xFF)
                    {
                        short.push(format!(
                            "{name} a{} {video:?} {led:?} letters {text:.4} at grey {g}",
                            alpha.get()
                        ));
                    }
                }
                if name == "light" && video == Video::Reverse && alpha.get() < 0xFF {
                    // The arm no colour reaches: the letters are white,
                    // and some pixel of them is drawn white exactly.
                    if !letters.contains(&0xFFFF_FFFF) {
                        short.push(format!("{name} reversed {led:?}: no letter pixel white"));
                    }
                }
            }
        }
    }
    assert_eq!(lamps, 4 * 2 * 9 * 256);
    assert_eq!(short, Vec::<String>::new());
}
