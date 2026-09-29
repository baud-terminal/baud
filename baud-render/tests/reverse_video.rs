//! `?5` DECSCNM, reverse video, reaches the pixels -- and every background,
//! the reversed default one included, keeps the window's dim.
//!
//! The rule is xterm's `ReverseVideo` (util.c, `[[source: xterm-scroll]]`'s
//! file): the default
//! foreground and background swap, palette entries 0 and 7 swap and so do 8
//! and 15, and everything drawn "in the foreground" or "in the background" --
//! the margin, the scrollbar, a cursor trail, a screen fade -- follows. The
//! owner's call on the dim, 2026-09-25: the same alpha for every background,
//! so the reversed default background is as translucent as the normal one.
//!
//! Every test here renders at a translucent `bg_alpha`, because at `0xFF` the
//! alpha half of the rule is invisible.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Argb, Blend, Damage, Floor, Hover, Kind, Look, PxSize, Renderer, Scroll, Scroller, Stack,
    Theme, Trail, TrailLook, Transition, Widths, Window,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 12,
};

/// `[glass] dim_dark`'s default, 0.22 of 255 rounded.
const DIM: u8 = 56;

fn alpha(c: Argb) -> u8 {
    c.0.to_be_bytes()[0]
}

fn rgb(c: Argb) -> u32 {
    c.0 & 0x00FF_FFFF
}

fn dim_theme() -> Theme {
    let mut t = Theme::dark();
    t.bg_alpha = Chan::from_u8(DIM);
    t
}

fn renderer<'f>(faces: &'f [Face<'f>], ws: &WinSize) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        dim_theme(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// One full frame of `bytes` on a fresh terminal, handed to `f`.
fn render(bytes: &[u8], f: impl FnOnce(&Renderer<'_, '_>)) {
    render_with(dim_theme(), bytes, f);
}

/// [`render`] under `theme` -- the floor off, for the tests about the swap.
fn render_with(theme: Theme, bytes: &[u8], f: impl FnOnce(&Renderer<'_, '_>)) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    r.set_theme(theme);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    f(&r);
}

/// The glyph colours of the frame, in the order the row's glyphs were
/// emitted -- column order for one run of plain letters.
fn glyph_colours(r: &Renderer<'_, '_>) -> Vec<Argb> {
    r.instances()
        .filter(|i| i.kind == Kind::Mask)
        .map(|i| i.color)
        .collect()
}

/// **The default colours swap, and the reversed background keeps the
/// dim.** The cursor is parked on row 1 so it cannot swap either cell back.
#[test]
fn the_default_colours_swap_and_the_background_keeps_the_dim() {
    let theme = dim_theme();
    // The control: without `?5` a blank is the theme's background.
    render(b"A\x1b[2;12H", |r| {
        assert_eq!(rgb(r.background()[1]), rgb(theme.bg));
        assert_eq!(glyph_colours(r), vec![theme.fg]);
    });
    render(b"A\x1b[?5h\x1b[2;12H", |r| {
        let blank = r.background()[1];
        assert_eq!(
            rgb(blank),
            rgb(theme.fg),
            "a blank under ?5 is the foreground"
        );
        assert_eq!(alpha(blank), DIM, "and it carries the window's dim");
        let glyphs = glyph_colours(r);
        assert_eq!(glyphs.len(), 1, "the one glyph, and no other");
        assert_eq!(rgb(glyphs[0]), rgb(theme.bg), "a letter is the background");
        assert_eq!(alpha(glyphs[0]), 0xFF, "a foreground is never translucent");
    });
}

/// `SGR 7` inside `?5` is reversed twice, which draws a normal cell -- and
/// its background, the theme's own, still carries the dim.
#[test]
fn reverse_inside_reverse_video_draws_a_normal_cell() {
    let theme = dim_theme();
    render(b"\x1b[?5h\x1b[7mA\x1b[0m\x1b[2;12H", |r| {
        let bg = r.background()[0];
        assert_eq!(rgb(bg), rgb(theme.bg));
        assert_eq!(alpha(bg), DIM);
        assert_eq!(glyph_colours(r), vec![theme.fg]);
    });
}

/// **Palette entries 0 and 7 swap, and so do 8 and 15**, whichever way the
/// colour was asked for -- an SGR colour, the bright forms, `38;5` -- and
/// after bold's brightening. Nothing else moves.
#[test]
fn the_palette_swaps_black_and_white_and_nothing_else() {
    let theme = dim_theme();
    let p = |i: usize| rgb(theme.palette[i]);
    // Backgrounds: 40, 47, 100, 107, 41, 48;5;7, 48;5;16 and a truecolour.
    let bgs = b"\x1b[?5h\x1b[40m \x1b[47m \x1b[100m \x1b[107m \x1b[41m \x1b[48;5;7m \x1b[48;5;16m \x1b[48;2;1;2;3m \x1b[0m\x1b[2;12H";
    render(bgs, |r| {
        let got: Vec<u32> = r.background()[..8].iter().map(|&c| rgb(c)).collect();
        let want = vec![p(7), p(0), p(15), p(8), p(1), p(0), p(16), 0x0001_0203];
        assert_eq!(got, want);
        for (i, &c) in r.background()[..8].iter().enumerate() {
            assert_eq!(alpha(c), DIM, "cell {i} keeps the dim");
        }
    });
    // Foregrounds: 30, bold 30 (brightened to 8, then swapped to 15), 37,
    // and 38;5;7.
    let fgs = b"\x1b[?5h\x1b[30mA\x1b[1mB\x1b[0;37mC\x1b[38;5;7mD\x1b[0m\x1b[2;12H";
    let mut off = dim_theme();
    off.floor = Floor::Off;
    render_with(off, fgs, |r| {
        let got: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        assert_eq!(got, vec![p(7), p(15), p(0), p(0)]);
    });
    // **With the floor on, `SGR 30` under `?5` is lifted**: it swaps to entry
    // 7, `#e5e5e5`, on the reversed page `#d8d8d8` -- 1.13:1, the flash case
    // the floor exists for (note Y, decision 6) -- and draws `#5e5e5e`, at
    // 4.5:1 or better. The other three read well above the trigger.
    render(fgs, |r| {
        let got: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        assert_eq!(got, vec![0x005e_5e5e, p(15), p(0), p(0)]);
    });
    // The control: the same bytes without `?5` draw the table unswapped.
    render(&bgs[5..], |r| {
        let got: Vec<u32> = r.background()[..8].iter().map(|&c| rgb(c)).collect();
        assert_eq!(
            got,
            vec![p(0), p(7), p(8), p(15), p(1), p(7), p(16), 0x0001_0203]
        );
    });
}

/// An underline colour asked for by index, `SGR 58;5;0`, swaps with the
/// table too: xterm's underline reads the same colour table its cells do.
#[test]
fn an_indexed_underline_colour_swaps_with_the_table() {
    let theme = dim_theme();
    let underline = |bytes: &[u8]| {
        let mut got = Vec::new();
        render(bytes, |r| {
            got = r
                .instances()
                .filter(|i| i.kind == Kind::Solid)
                .map(|i| rgb(i.color))
                .collect();
        });
        got
    };
    // With the floor off: the swap alone. Black on the dark page is 1.10:1,
    // so with it on the floor lifts the underline, which `floor.rs` tests.
    let underline_off = |bytes: &[u8]| {
        let mut off = dim_theme();
        off.floor = Floor::Off;
        let mut got = Vec::new();
        render_with(off, bytes, |r| {
            got = r
                .instances()
                .filter(|i| i.kind == Kind::Solid)
                .map(|i| rgb(i.color))
                .collect();
        });
        got
    };
    let line = b"\x1b[4;58;5;0mA\x1b[0m\x1b[2;12H";
    assert_eq!(
        underline_off(line),
        vec![rgb(theme.palette[0])],
        "the control"
    );
    let reversed = [&b"\x1b[?5h"[..], line].concat();
    assert_eq!(underline_off(&reversed), vec![rgb(theme.palette[7])]);
    // With the floor on, the reversed underline is entry 7 on the reversed
    // page, the same 1.13:1 pair the text is, and is lifted the same way.
    assert_eq!(underline(&reversed), vec![0x005e_5e5e]);
}

/// An `OSC 4` override of entry 7 is what `SGR 30` draws under `?5`: xterm
/// swaps the colours the table *holds*, so an override set before the flip
/// travels with the slot it was set on.
#[test]
fn a_palette_override_travels_with_its_entry() {
    render(
        b"\x1b]4;7;rgb:12/34/56\x07\x1b[?5h\x1b[40m \x1b[0m\x1b[2;12H",
        |r| {
            assert_eq!(rgb(r.background()[0]), 0x0012_3456);
        },
    );
}

/// `OSC 10` and `OSC 11` swap with the theme's defaults: the default a
/// program moved is the one `?5` puts on the other side.
#[test]
fn the_dynamic_defaults_swap_too() {
    let theme = dim_theme();
    render(b"\x1b]11;rgb:aa/bb/cc\x07\x1b[?5hA\x1b[2;12H", |r| {
        assert_eq!(
            rgb(r.background()[1]),
            rgb(theme.fg),
            "the blank is the foreground, which nothing moved"
        );
        assert_eq!(
            glyph_colours(r).into_iter().map(rgb).collect::<Vec<_>>(),
            vec![0x00aa_bbcc],
            "the letter is the background OSC 11 set"
        );
        assert_eq!(rgb(r.clear_color()), rgb(theme.fg), "and so is the margin");
        assert_eq!(alpha(r.clear_color()), DIM);
    });
}

/// **The margin is the default background the screen draws**, `OSC 11`'s
/// when a program set one -- the colour `clear_color` hands a presenter for
/// the letterbox and the gutter. It was the theme's whatever `OSC 11` said,
/// so a program that moved the background got a frame of the old colour
/// around its cells.
#[test]
fn the_margin_follows_osc_11() {
    render(b"\x1b]11;rgb:aa/bb/cc\x07A\x1b[2;12H", |r| {
        assert_eq!(rgb(r.clear_color()), 0x00aa_bbcc);
        assert_eq!(alpha(r.clear_color()), DIM);
        assert_eq!(
            rgb(r.background()[1]),
            0x00aa_bbcc,
            "the control: the cells had it"
        );
    });
}

/// **The flip is a visual bell, so it must repaint on incremental damage**:
/// no cell word and no style slot changes, so row damage alone would find
/// nothing. Both directions equal a full repaint of the same state, and the
/// flip back is the frame before it.
#[test]
fn a_flip_repaints_every_row_and_equals_a_full_repaint() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);

    feed(&mut t, b"A\x1b[41mB\x1b[0m\r\nC");
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let normal = c.digest();
    let quiet = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        quiet.rows_dirty.value(),
        0,
        "a quiet frame repaints nothing"
    );

    feed(&mut t, b"\x1b[?5h");
    let f = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        f.rows_dirty.value(),
        f.rows.value(),
        "?5 h repaints every row"
    );
    assert_ne!(c.digest(), normal, "and the frame changed");
    let mut fresh = renderer(&faces, &WS);
    let mut full = support::Canvas::for_renderer(&fresh);
    support::paint(&mut fresh, &mut t, &mut full, Damage::Full);
    assert_eq!(c.digest(), full.digest(), "the flip equals a full repaint");

    feed(&mut t, b"\x1b[?5l");
    let f = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        f.rows_dirty.value(),
        f.rows.value(),
        "?5 l repaints every row"
    );
    assert_eq!(c.digest(), normal, "and the flip back is the frame before");
}

/// **What a driver reads before it renders is what the renderer answers
/// after.** The Linux frame is the window's size and the renderer paints only
/// the grid, so the driver fills the strips past it itself, from
/// `clear_color_of` -- which has to agree with the `clear_color` the convert
/// leaves, or a flip leaves the strips the old colour.
#[test]
fn the_margin_a_driver_reads_first_is_the_one_the_convert_leaves() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    for bytes in [
        &b"A"[..],
        b"\x1b[?5h",
        b"\x1b]11;rgb:aa/bb/cc\x07",
        b"\x1b[?5l",
        b"\x1b]111\x07",
    ] {
        feed(&mut t, bytes);
        let before = r.clear_color_of(&t);
        r.convert(&t, Rows::ZERO, Damage::Incremental);
        assert_eq!(before, r.clear_color(), "after {bytes:?}");
    }
    // And the sequence moved it, so the equality above was about a margin
    // that changes: the last step is the theme's again.
    assert_eq!(rgb(r.clear_color()), rgb(dim_theme().bg));
}

const TALL: WinSize = WinSize {
    ws_row: 10,
    ws_col: 30,
};

/// **What is drawn in the foreground or the background follows on the frame
/// that flips**, not the frame after: the scrollbar, the cursor trail and
/// the screen fade are set before a convert, so the flip has to rebuild the
/// ones already set. And the margin comes with them.
#[test]
fn the_overlays_follow_the_flip_on_the_frame_it_happens() {
    let theme = dim_theme();
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &TALL);
    let mut owned = Owned::new(&TALL);
    let mut t = Term::new(owned.storage(), &TALL);
    let lines: Vec<u8> = (0..40)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect();
    feed(&mut t, &lines);
    let window = Window {
        retained: Rows::of_count(t.scrollback().len()),
        back: Rows::of_count(3),
        rows: Rows::of_index(usize::from(TALL.ws_row)),
    };
    let widths = Widths {
        rest: Px::of(12),
        hovered: Px::of(22),
        inset: Px::of(6),
        strip: Px::of(34),
    };
    let set_all = |r: &mut Renderer<'_, '_>| {
        r.set_scrollbar(Some(Scroll {
            window,
            look: Look {
                shown: Chan::from_u8(255),
                hover: Hover::Rest,
            },
            widths,
            scroller: Scroller::Overlay,
        }));
        r.set_trail(Some(TrailLook::Ghost(Trail {
            from: (Row::from_index(1), Col::at_index(1)),
            to: (Row::from_index(4), Col::at_index(20)),
            progress: 0.5,
            origin_dy: 0.0,
            lift: Px::of(0),
        })));
        r.set_screen_fade(Some(Transition { progress: 0.5 }));
    };
    let colours = |r: &Renderer<'_, '_>| {
        (
            r.scrollbar()
                .iter()
                .map(|i| rgb(i.color))
                .collect::<Vec<_>>(),
            r.trail().iter().map(|i| rgb(i.color)).collect::<Vec<_>>(),
            r.screen_fade()
                .iter()
                .map(|i| rgb(i.color))
                .collect::<Vec<_>>(),
            rgb(r.clear_color()),
        )
    };

    set_all(&mut r);
    r.convert(&t, Rows::ZERO, Damage::Full);
    let (bar, trail, fade, margin) = colours(&r);
    assert!(!bar.is_empty() && !trail.is_empty() && !fade.is_empty());
    assert!(bar.iter().chain(&trail).all(|&c| c == rgb(theme.fg)));
    assert_eq!(fade, vec![rgb(theme.bg)]);
    assert_eq!(margin, rgb(theme.bg));

    // The driver sets the overlays for the next frame *before* it converts,
    // so they are built under the video the last frame drew.
    set_all(&mut r);
    feed(&mut t, b"\x1b[?5h");
    r.convert(&t, Rows::ZERO, Damage::Incremental);
    let (bar, trail, fade, margin) = colours(&r);
    assert!(
        bar.iter().chain(&trail).all(|&c| c == rgb(theme.bg)),
        "the scrollbar and the trail are the reversed foreground"
    );
    assert_eq!(
        fade,
        vec![rgb(theme.fg)],
        "the fade is the reversed background"
    );
    assert_eq!(margin, rgb(theme.fg), "and so is the margin");
    assert_eq!(alpha(r.clear_color()), DIM, "at the window's dim");

    // And on the frame after, where nothing flipped and the overlays are set
    // again: they are built in the reversed colours, not rebuilt into them.
    set_all(&mut r);
    r.convert(&t, Rows::ZERO, Damage::Incremental);
    let (bar, trail, fade, _) = colours(&r);
    assert!(bar.iter().chain(&trail).all(|&c| c == rgb(theme.bg)));
    assert_eq!(fade, vec![rgb(theme.fg)]);
}
