//! `?5` DECSCNM, reverse video, reaches the pixels -- and every background,
//! the reversed default one included, keeps the window's dim.
//!
//! The rule is DEC STD 070's (`[[source: dec-std-070]]` § 5.6.3.2, page 5-107,
//! PDF 362): the mode "will affect which attribute bits from display memory are
//! used to render the text foreground and background color", so **every cell
//! draws with its foreground and background exchanged**, an explicit `SGR`
//! colour included -- the owner's decision of 2026-10-13, which took this over
//! xterm's palette swap. A cell's own `SGR 7` exchanges again. What is drawn
//! "in the foreground" or "in the background" -- the margin, the scrollbar, a
//! cursor trail, a screen fade -- takes the default pair as a default cell
//! shows it. The owner's call on the dim, 2026-09-25: the same alpha for every
//! background, so the reversed default background is as translucent as the
//! normal one.
//!
//! Every test here renders at a translucent `bg_alpha`, because at `0xFF` the
//! alpha half of the rule is invisible.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    AppBg, Argb, Blend, Damage, Floor, Hover, Kind, Look, PxSize, Renderer, Scroll, Scroller,
    Stack, Theme, Trail, TrailLook, Transition, Widths, Window,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 12,
};

/// The old default dim, 0.22 of 255 (alpha 56; the default is 0.70 since
/// 2026-10-17).
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
    render_in(theme, &WS, bytes, |r, _| f(r));
}

/// [`render_with`] at `ws`, handing over the painted canvas too: the CPU
/// path's own pixels, painted from the frame the instances describe.
fn render_in(
    theme: Theme,
    ws: &WinSize,
    bytes: &[u8],
    f: impl FnOnce(&Renderer<'_, '_>, &support::Canvas),
) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, ws);
    r.set_theme(theme);
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    feed(&mut t, bytes);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    f(&r, &canvas);
}

fn floor_off() -> Theme {
    let mut t = dim_theme();
    t.floor = Floor::Off;
    t
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

/// **Every cell exchanges its colours, an explicit one included** (DEC STD
/// 070 § 5.6.3.2; the owner, 2026-10-13): a coloured letter is drawn as a
/// coloured cell with the default page's colour for its text, and a coloured
/// background as coloured text on the reversed page -- whichever way the
/// colour was asked for, an SGR colour, the bright forms, `38;5`, `48;5`,
/// truecolour. Bold brightens the foreground's index first, then the pair
/// exchanges. No palette entry moves, so entries 0, 7, 8 and 15 are drawn
/// as themselves, which xterm's swap did not do.
#[test]
fn every_cell_exchanges_its_colours_explicit_ones_included() {
    let theme = dim_theme();
    let p = |i: usize| rgb(theme.palette[i]);
    // Backgrounds, each under a letter: 40, 47, 100, 107, 41, 48;5;7,
    // 48;5;16 and a truecolour. Each letter is drawn in its cell's
    // background and every cell's background is the default foreground.
    let bgs = b"\x1b[?5h\x1b[40mA\x1b[47mB\x1b[100mC\x1b[107mD\x1b[41mE\x1b[48;5;7mF\x1b[48;5;16mG\x1b[48;2;1;2;3mH\x1b[0m\x1b[2;12H";
    render_with(floor_off(), bgs, |r| {
        let text: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        let want = vec![p(0), p(7), p(8), p(15), p(1), p(7), p(16), 0x0001_0203];
        assert_eq!(text, want, "each letter is its cell's background");
        for (i, &c) in r.background()[..8].iter().enumerate() {
            assert_eq!(rgb(c), rgb(theme.fg), "cell {i} is the default foreground");
            assert_eq!(alpha(c), DIM, "cell {i} keeps the dim");
        }
    });
    // Foregrounds: 30, bold 30 (brightened to 8, then exchanged), 37,
    // 38;5;7, 31, 38;5;196 and a truecolour. Each cell is its letter's
    // colour, and every letter is the default background.
    let fgs = b"\x1b[?5h\x1b[30mA\x1b[1mB\x1b[0;37mC\x1b[38;5;7mD\x1b[31mE\x1b[38;5;196mF\x1b[38;2;200;100;50mG\x1b[0m\x1b[2;12H";
    render_with(floor_off(), fgs, |r| {
        let got: Vec<u32> = r.background()[..7].iter().map(|&c| rgb(c)).collect();
        let want = vec![p(0), p(8), p(7), p(7), p(1), p(196), 0x00c8_6432];
        assert_eq!(got, want, "each cell is its letter's colour");
        let text: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        assert_eq!(text, vec![rgb(theme.bg); 7], "every letter is the page");
    });
    // The control: the same bytes without `?5` draw each colour on its own
    // side.
    render_with(floor_off(), &fgs[5..], |r| {
        let text: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        let want = vec![p(0), p(8), p(7), p(7), p(1), p(196), 0x00c8_6432];
        assert_eq!(text, want);
    });
}

/// **The contrast floor reads the pair as it is drawn**, after the
/// exchange: `SGR 30` under `?5` is the default page's `#101010` on entry 0,
/// `#000000` -- 1.10:1 -- and the text is lifted to `#757575`; `SGR 47`'s
/// letter is entry 7, `#e5e5e5`, on the reversed page `#d8d8d8`, 1.13:1,
/// and is lifted to `#5e5e5e`.
///
/// Which side is the page decides whether the floor may touch the pair at
/// all (`Floor::covers`): `38;5;1` on a truecolour `48;2;200;0;0` is a page
/// the application chose, and its red letters are left at 1.03:1 on a
/// normal screen. Under `?5` the letter is the truecolour and the page is
/// entry 1, one of the sixteen, so the floor lifts it.
#[test]
fn the_floor_reads_the_exchanged_pair() {
    render(b"\x1b[?5h\x1b[30mA\x1b[0;47mB\x1b[0m\x1b[2;12H", |r| {
        let got: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        assert_eq!(got, vec![0x0075_7575, 0x005e_5e5e]);
    });
    let chosen = b"\x1b[38;5;1;48;2;200;0;0mA\x1b[0m\x1b[2;12H";
    render(chosen, |r| {
        let got: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        assert_eq!(got, vec![rgb(dim_theme().palette[1])], "the control");
    });
    render(&[&b"\x1b[?5h"[..], chosen].concat(), |r| {
        let got: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        assert_eq!(got.len(), 1);
        assert_ne!(got[0], 0x00c8_0000, "the letter was lifted off its page");
    });
}

/// `SGR 7` inside `?5` is exchanged twice, so a coloured cell draws as it
/// would on a normal screen: `SGR 7;31` red letters on the default page,
/// `SGR 7;41` default letters on red.
#[test]
fn a_coloured_reverse_cell_inside_reverse_video_draws_normal() {
    let theme = dim_theme();
    render_with(
        floor_off(),
        b"\x1b[?5h\x1b[7;31mA\x1b[0;7;41mB\x1b[0m\x1b[2;12H",
        |r| {
            let bg: Vec<u32> = r.background()[..2].iter().map(|&c| rgb(c)).collect();
            assert_eq!(bg, vec![rgb(theme.bg), rgb(theme.palette[1])]);
            let text: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
            assert_eq!(text, vec![rgb(theme.palette[1]), rgb(theme.fg)]);
        },
    );
}

/// **The CPU path paints the exchange**, in its own pixels and not only in
/// the frame it paints from: under `?5` a blank written under `SGR 31` is a
/// red cell, one under `SGR 41` is the reversed page, and so is the rest of
/// the row erased under `SGR 41` -- an erased area is blanks in that
/// rendition, and a blank's background is its default foreground once
/// exchanged. Painted opaque, so a pixel is the colour and not a blend.
#[test]
fn the_canvas_paints_each_cell_exchanged() {
    let mut theme = Theme::dark();
    theme.floor = Floor::Off;
    let bytes = b"\x1b[?5h\x1b[31m \x1b[0;41m \x1b[K\x1b[0m\x1b[2;12H";
    let fg = rgb(theme.fg);
    let red = rgb(theme.palette[1]);
    let check = |bytes: &[u8], want: [u32; 3]| {
        render_in(theme.clone(), &WS, bytes, |r, c| {
            let m = r.metrics();
            let (cw, ch) = (
                usize::try_from(m.cell_w.value()).unwrap(),
                usize::try_from(m.cell_h.value()).unwrap(),
            );
            let at = |col: usize| rgb(Argb(c.pixels[(ch / 2) * c.width + col * cw + cw / 2]));
            let last = usize::from(WS.ws_col) - 1;
            assert_eq!([at(0), at(1), at(last)], want, "{bytes:?}");
        });
    };
    check(bytes, [red, fg, fg]);
    // The control: without `?5` the first blank is the page and the band red.
    check(&bytes[5..], [rgb(theme.bg), red, red]);
}

/// An underline that follows the text follows it through the exchange, and
/// an `SGR 58` colour, which is neither the foreground nor the background,
/// is drawn as set: entry 0 is entry 0 under `?5`, where xterm's swap drew
/// entry 7.
#[test]
fn an_underline_follows_the_text_and_an_sgr_58_colour_stays() {
    let theme = dim_theme();
    let underline = |theme: Theme, bytes: &[u8]| {
        let mut got = Vec::new();
        render_with(theme, bytes, |r| {
            got = r
                .instances()
                .filter(|i| i.kind == Kind::Solid)
                .map(|i| rgb(i.color))
                .collect();
        });
        got
    };
    // `SGR 31;4` under `?5`: the letter is the default page's colour, and so
    // is its underline.
    assert_eq!(
        underline(floor_off(), b"\x1b[?5h\x1b[31;4mA\x1b[0m\x1b[2;12H"),
        vec![rgb(theme.bg)]
    );
    let line = b"\x1b[4;58;5;0mA\x1b[0m\x1b[2;12H";
    assert_eq!(
        underline(floor_off(), line),
        vec![rgb(theme.palette[0])],
        "the control"
    );
    let reversed = [&b"\x1b[?5h"[..], line].concat();
    assert_eq!(
        underline(floor_off(), &reversed),
        vec![rgb(theme.palette[0])]
    );
    // With the floor on: black on the reversed page `#d8d8d8` reads far
    // above the trigger, so nothing is lifted.
    assert_eq!(
        underline(dim_theme(), &reversed),
        vec![rgb(theme.palette[0])]
    );
}

/// An `OSC 4` override is drawn wherever its entry is named, exchanged like
/// any colour: entry 1 overridden is the text of an `SGR 41` cell and the
/// background of an `SGR 31` one. An override of entry 7 does not reach a
/// cell that names entry 0, as xterm's swap made it do.
#[test]
fn a_palette_override_is_drawn_where_its_entry_is_named() {
    render_with(
        floor_off(),
        b"\x1b]4;1;rgb:12/34/56\x07\x1b[?5h\x1b[41mA\x1b[0;31mB\x1b[0m\x1b[2;12H",
        |r| {
            let text: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
            assert_eq!(text[0], 0x0012_3456, "the SGR 41 letter");
            assert_eq!(rgb(r.background()[1]), 0x0012_3456, "the SGR 31 cell");
        },
    );
    render_with(
        floor_off(),
        b"\x1b]4;7;rgb:12/34/56\x07\x1b[?5h\x1b[30mA\x1b[0m\x1b[2;12H",
        |r| {
            assert_eq!(rgb(r.background()[0]), rgb(dim_theme().palette[0]));
        },
    );
}

/// **`OSC 10` and `11` under `?5` reach an explicitly coloured cell too**:
/// the colour `OSC 10` sets while reversed is the one drawn as text, and an
/// `SGR 31` cell's text under `?5` is its default background exchanged -- so
/// it moves, while the red stays the cell's background. `OSC 11` likewise
/// moves an `SGR 41` cell's page and leaves its red letter.
#[test]
fn osc_10_and_11_under_reverse_reach_an_exchanged_cell() {
    let theme = dim_theme();
    render_with(
        floor_off(),
        b"\x1b[?5h\x1b]10;rgb:12/34/56\x07\x1b]11;rgb:aa/bb/cc\x07\x1b[31mA\x1b[0;41mB\x1b[0m\x1b[2;12H",
        |r| {
            let text: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
            assert_eq!(text, vec![0x0012_3456, rgb(theme.palette[1])]);
            let bg: Vec<u32> = r.background()[..2].iter().map(|&c| rgb(c)).collect();
            assert_eq!(bg, vec![rgb(theme.palette[1]), 0x00aa_bbcc]);
        },
    );
}

/// Faint dims the text that is drawn toward the colour it is drawn on,
/// after the exchange: `SGR 2;31` under `?5` is the default page's
/// `#101010` mixed half way to red, `#6f0808`, on red. Before the exchange it
/// would dim the red toward the page and draw `#101010` on the dimmed red.
#[test]
fn faint_dims_the_exchanged_text() {
    let theme = dim_theme();
    render_with(floor_off(), b"\x1b[?5h\x1b[2;31mA\x1b[0m\x1b[2;12H", |r| {
        let text: Vec<u32> = glyph_colours(r).into_iter().map(rgb).collect();
        assert_eq!(text, vec![0x006f_0808]);
        assert_eq!(rgb(r.background()[0]), rgb(theme.palette[1]));
    });
}

/// **`app_backgrounds = "opaque"` keeps the background drawn**, which under
/// `?5` is the cell's foreground: an `SGR 31` cell's red is opaque, an `SGR
/// 41` cell's page is the default foreground and keeps the dim, and an `SGR
/// 7;31` cell, exchanged twice, is the default background and dims too.
#[test]
fn an_opaque_background_is_the_one_drawn() {
    let theme = Theme {
        app_bg: AppBg::Opaque,
        ..dim_theme()
    };
    let bytes = b"\x1b[?5h\x1b[31mA\x1b[0;41mB\x1b[0mC\x1b[7;31mD\x1b[0m\x1b[2;12H";
    render_with(theme.clone(), bytes, |r| {
        let a: Vec<u8> = r.background()[..4].iter().map(|&c| alpha(c)).collect();
        assert_eq!(a, vec![0xFF, DIM, DIM, DIM]);
    });
    // The control: on a normal screen the `SGR 41` and `SGR 7;31` cells are
    // the ones whose drawn background the application set.
    render_with(theme, &bytes[5..], |r| {
        let a: Vec<u8> = r.background()[..4].iter().map(|&c| alpha(c)).collect();
        assert_eq!(a, vec![DIM, 0xFF, DIM, 0xFF]);
    });
}

/// **A picture drawn in text is found by the background drawn**, which
/// under `?5` is each cell's foreground: a row of sixteen letters in sixteen
/// foreground colours is a picture there and is drawn solid, and a row of
/// sixteen blanks on sixteen backgrounds, whose drawn backgrounds are all
/// the default foreground, is not. On a normal screen it is the other way
/// round.
#[test]
fn a_picture_is_found_by_the_background_drawn() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 20,
    };
    let mut bytes = Vec::new();
    for i in 0..16 {
        bytes.extend_from_slice(format!("\x1b[48;5;{}m ", 16 + i * 7).as_bytes());
    }
    bytes.extend_from_slice(b"\x1b[0m\r\n");
    for i in 0..16 {
        bytes.extend_from_slice(format!("\x1b[38;5;{}mX", 16 + i * 7).as_bytes());
    }
    bytes.extend_from_slice(b"\x1b[0m\x1b[4;20H");
    let alphas = |bytes: &[u8]| {
        let mut got = (Vec::new(), Vec::new());
        render_in(dim_theme(), &ws, bytes, |r, _| {
            let bg = r.background();
            got = (
                bg[..16].iter().map(|&c| alpha(c)).collect::<Vec<_>>(),
                bg[20..36].iter().map(|&c| alpha(c)).collect::<Vec<_>>(),
            );
        });
        got
    };
    let (blanks, letters) = alphas(&bytes);
    assert_eq!(
        blanks,
        vec![0xFF; 16],
        "the control: the blanks are a picture"
    );
    assert_eq!(letters, vec![DIM; 16], "the control: the letters are not");
    let reversed = [&b"\x1b[?5h"[..], &bytes].concat();
    let (blanks, letters) = alphas(&reversed);
    assert_eq!(blanks, vec![DIM; 16], "under ?5 the blanks are the page");
    assert_eq!(letters, vec![0xFF; 16], "and the letters are the picture");
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

/// **A colour set *while* reversed is the colour drawn in the role it
/// named** (note V, V17): `OSC 11` under `?5` sets the background shown, the
/// blank and the margin, and `OSC 10` the text -- xterm's, whose swap of the
/// stored colours makes `misc.c`'s set land on the displayed slot
/// (`[[source: xterm-misc]]` 3662-3716). Before, each moved the other role.
#[test]
fn a_dynamic_colour_set_under_reverse_draws_in_the_role_it_named() {
    render(
        b"\x1b[?5h\x1b]11;rgb:aa/bb/cc\x07\x1b]10;rgb:12/34/56\x07A\x1b[2;12H",
        |r| {
            assert_eq!(rgb(r.background()[1]), 0x00aa_bbcc, "the blank");
            assert_eq!(rgb(r.clear_color()), 0x00aa_bbcc, "the margin");
            assert_eq!(
                glyph_colours(r).into_iter().map(rgb).collect::<Vec<_>>(),
                vec![0x0012_3456],
                "the letter"
            );
        },
    );
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
