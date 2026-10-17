//! The contrast floor (note Y, decision 6; section 8).
//!
//! The pixel oracle cannot check it -- both GPU arms read one resolved table,
//! so a wrong floor is drawn identically twice -- so it is held here to the
//! WCAG 2.2 formula computed in `f64`, through the renderer's own resolve.
//! Which of a slot's two texts a cell draws under the glass is chosen per
//! cell on each arm, and `glass_floor_gate` in `baud-present-macos`'s
//! `cells.rs` compares the three.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Row, Rows, StyleWord, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Blend, BlinkPhases, BlockCursor, Floor, Line, Pictured, Pictures, PxSize, Renderer,
    Resolved, Selected, Shapes, Stack, Swaps, Theme, Video, xterm_palette,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};

fn lin(c: Chan) -> f64 {
    let v = f64::from(c.get()) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn lum(c: Argb) -> f64 {
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn contrast(a: Argb, b: Argb) -> f64 {
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn rgb(c: Argb) -> [u8; 3] {
    [c.r().get(), c.g().get(), c.b().get()]
}

/// Every cell written by `sgrs`, one `X` each after `SGR 0;s`, with its
/// rendition word, what the theme resolves it to unfloored, and the resolved
/// slot the renderer draws it from.
fn cells(theme: Theme, sgrs: &[String]) -> Vec<(String, StyleWord, (Argb, Argb), Resolved)> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    let mut bytes = Vec::new();
    for sgr in sgrs {
        bytes.extend_from_slice(format!("\x1b[0;{sgr}mX").as_bytes());
    }
    assert!(sgrs.len() < usize::from(WS.ws_col) * usize::from(WS.ws_row));
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut table = vec![Resolved::default(); t.styles().len()];
    r.resolve_styles_into(&t, &mut table);
    let mut out = Vec::new();
    'rows: for y in 0..usize::from(WS.ws_row) {
        for cell in t.row(Row::from_index(y)).expect("a row on the grid") {
            if out.len() == sgrs.len() {
                break 'rows;
            }
            let style = t.style_of(*cell);
            let raw = theme.resolve(style, Video::Normal, |_| None, |_| None);
            let slot = table[cell.style.index()];
            out.push((sgrs[out.len()].clone(), style, raw, slot));
        }
    }
    assert_eq!(out.len(), sgrs.len(), "every case drew a cell");
    out
}

/// Every foreground in {default, 0-15} on every background in
/// {default, 0-15}, plain, bold, faint and reversed: the pairs the rule
/// covers, a superset of the 201 program pairs note Y counted.
fn sixteen_sweep() -> Vec<String> {
    let fgs: Vec<String> = std::iter::once("39".into())
        .chain((0..8).map(|i| format!("{}", 30 + i)))
        .chain((0..8).map(|i| format!("{}", 90 + i)))
        .collect();
    let bgs: Vec<String> = std::iter::once("49".into())
        .chain((0..8).map(|i| format!("{}", 40 + i)))
        .chain((0..8).map(|i| format!("{}", 100 + i)))
        .collect();
    let mut out = Vec::new();
    for f in &fgs {
        for b in &bgs {
            for a in ["", ";1", ";2", ";7"] {
                out.push(format!("{f};{b}{a}"));
            }
        }
    }
    out
}

fn words(style: StyleWord) -> (baud_domain::ColorWord, baud_domain::ColorWord) {
    if style.attrs().has(baud_domain::Attrs::REVERSE) {
        (style.bg(), style.fg())
    } else {
        (style.fg(), style.bg())
    }
}

/// **The reference test.** Over the sweep, under both themes: a covered pair
/// below 1.2 is lifted to 4.5 or above, and no further than one encoded step
/// needs; its hue kept; every other pair drawn exactly as resolved.
#[test]
fn every_covered_pair_below_the_trigger_reaches_the_target() {
    let sgrs = sixteen_sweep();
    let mut lifted = 0;
    for theme in [Theme::light(), Theme::dark()] {
        for (sgr, style, (fg, bg), slot) in cells(theme, &sgrs) {
            let (tw, pw) = words(style);
            let before = contrast(fg, bg);
            if Floor::covers(tw, pw) && before < 1.2 {
                let after = contrast(slot.fg, bg);
                assert!(after >= 4.5, "{sgr}: lifted to {after:.3}");
                lifted += 1;
                // Just far enough: stepping any channel one back toward the
                // original colour would fall short, unless it hit an extreme.
                assert!(
                    after < 5.2 || lum(slot.fg) < 0.001 || lum(slot.fg) > 0.999,
                    "{sgr}: {after:.3} is past the target"
                );
                // The hue kept: the channels' order does not change.
                let (a, b) = (rgb(fg), rgb(slot.fg));
                for i in 0..3 {
                    for j in 0..3 {
                        if u16::from(a[i]) > u16::from(a[j]) + 8 {
                            assert!(b[i] >= b[j], "{sgr}: channel order moved: {a:?} -> {b:?}");
                        }
                    }
                }
            } else {
                assert_eq!(
                    rgb(slot.fg),
                    rgb(fg),
                    "{sgr}: not covered or not below, untouched"
                );
            }
        }
    }
    // Note Y computed 5 below 1.2 under Modus light and more under xterm
    // dark over its 201; the sweep holds many more pairs than that.
    assert!(
        lifted > 40,
        "only {lifted} pairs lifted: the sweep reached the case"
    );
}

/// The trigger is strict: xterm's bright green on the light page is 1.1934,
/// lifted at 1.2 and not at 1.19.
#[test]
fn the_trigger_is_a_strict_bound() {
    let mut xl = Theme::light();
    xl.palette = xterm_palette();
    for (trigger, lifted) in [(1.2, true), (1.19, false)] {
        let mut theme = xl.clone();
        theme.floor = Floor::Lift {
            trigger,
            target: 4.5,
        };
        let (_, _, (fg, bg), slot) = cells(theme, &["92".into()]).remove(0);
        assert!((contrast(fg, bg) - 1.1934).abs() < 0.001);
        assert_eq!(rgb(slot.fg) != rgb(fg), lifted, "trigger {trigger}");
    }
}

/// What the rule leaves alone: the same word on both sides, two explicit
/// colours, and anything with the floor off.
#[test]
fn chosen_pairs_and_an_off_floor_are_untouched() {
    for sgr in [
        "33;43",
        "38;5;100;48;5;100",
        "38;2;100;100;100;48;2;101;101;101",
    ] {
        let (_, _, (fg, bg), slot) = cells(Theme::dark(), &[sgr.into()]).remove(0);
        assert!(
            contrast(fg, bg) < 1.2,
            "{sgr}: the control, below the trigger"
        );
        assert_eq!(rgb(slot.fg), rgb(fg), "{sgr}");
    }
    let mut off = Theme::dark();
    off.floor = Floor::Off;
    let (_, _, (fg, _), slot) = cells(off, &["30".into()]).remove(0);
    assert_eq!(rgb(slot.fg), rgb(fg), "off lifts nothing");
}

/// SGR 30 on the dark page (1.10:1), faint black there, and an underline
/// following floored text.
#[test]
fn black_on_the_dark_page_is_lifted_and_the_underline_follows() {
    for sgr in ["30", "2;30", "4;30"] {
        let (_, _, (fg, bg), slot) = cells(Theme::dark(), &[sgr.into()]).remove(0);
        assert!(contrast(fg, bg) < 1.2, "{sgr}: the control");
        assert!(contrast(slot.fg, bg) >= 4.5, "{sgr}");
        if sgr == "4;30" {
            assert!(slot.shapes.has(Shapes::UNDERLINE_FOLLOWS_FG));
            assert_eq!(slot.underline, slot.fg);
        }
    }
}

/// An explicit `SGR 58` underline colour below the trigger is lifted too.
#[test]
fn an_explicit_underline_colour_is_lifted() {
    let (_, _, (_, bg), slot) = cells(Theme::dark(), &["4;58;5;0".into()]).remove(0);
    assert!(contrast(slot.underline, bg) >= 4.5);
    assert!(!slot.shapes.has(Shapes::UNDERLINE_FOLLOWS_FG));
}

/// btop's graph greys on the default dark page: untouched at 1.2, lifted at
/// 3.0, the Increase Contrast trigger (note Y, decision 9).
#[test]
fn btops_greys_are_lifted_only_at_the_stricter_trigger() {
    for (trigger, lifted) in [(1.2, false), (3.0, true)] {
        let mut theme = Theme::dark();
        theme.floor = Floor::Lift {
            trigger,
            target: 4.5,
        };
        let got = cells(theme, &["38;2;48;48;48".into(), "38;2;64;64;64".into()]);
        for (sgr, _, (fg, _), slot) in got {
            assert_eq!(rgb(slot.fg) != rgb(fg), lifted, "{sgr} at {trigger}");
        }
    }
}

#[test]
fn covers_is_the_rule_as_written() {
    use baud_domain::ColorWord as W;
    let i = |n| W::indexed(Chan::from_u8(n));
    let t = |v| W::rgb(Chan::from_u8(v), Chan::from_u8(v), Chan::from_u8(v));
    assert!(Floor::covers(W::DEFAULT, W::DEFAULT), "two defaults");
    assert!(Floor::covers(i(0), W::DEFAULT));
    assert!(
        Floor::covers(t(20), W::DEFAULT),
        "truecolour on the default page"
    );
    assert!(Floor::covers(i(3), i(11)));
    assert!(
        Floor::covers(W::DEFAULT, t(20)),
        "default text on a page the application set"
    );
    assert!(!Floor::covers(i(3), i(3)), "the same word");
    assert!(!Floor::covers(t(20), t(21)), "two explicit colours");
    assert!(!Floor::covers(i(100), i(101)), "two cube colours");
    assert!(
        !Floor::covers(i(1), i(239)),
        "palette text on an explicit page: chafa's red on grey"
    );
}

/// Note Y's worked examples, on the light page under xterm's sixteen:
/// `#00ff00` to `#007f00` (4.53), `#00ffff` to `#007979` (4.55), `#ffffff` to
/// `#6c6c6c` (4.57) -- toward black, the hue kept.
#[test]
fn the_worked_examples_lift_to_their_colours() {
    let mut theme = Theme::light();
    theme.palette = xterm_palette();
    let got = cells(theme, &["92".into(), "96".into(), "97".into()]);
    let want = [[0x00, 0x7f, 0x00], [0x00, 0x79, 0x79], [0x6c, 0x6c, 0x6c]];
    for ((sgr, _, (_, bg), slot), want) in got.into_iter().zip(want) {
        assert_eq!(rgb(slot.fg), want, "{sgr}");
        assert!(contrast(slot.fg, bg) >= 4.5, "{sgr}");
    }
}

/// Every glyph cell of a recording: its unfloored colours and its slot.
fn replayed(theme: Theme, bytes: &[u8]) -> Vec<((Argb, Argb), Resolved)> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut table = vec![Resolved::default(); t.styles().len()];
    r.resolve_styles_into(&t, &mut table);
    let mut out = Vec::new();
    for y in 0..usize::from(WS.ws_row) {
        for cell in t.row(Row::from_index(y)).expect("a row on the grid") {
            if cell.content.is_empty() {
                continue;
            }
            let style = t.style_of(*cell);
            let raw = theme.resolve(style, Video::Normal, |_| None, |_| None);
            out.push((raw, table[cell.style.index()]));
        }
    }
    out
}

/// **Pictures drawn in text are left alone.** The two chafa captures set
/// both sides of every cell as explicit colours, so the rule covers none of
/// them, though hundreds of their neighbouring pixels are below the trigger
/// -- the positive control that the floor had something to touch.
#[test]
fn chafa_previews_draw_the_same_with_the_floor() {
    for name in ["chafa-240.ans", "chafa-truecolour.ans"] {
        let bytes = std::fs::read(format!(
            "{}/../tests/previews/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("the capture");
        for theme in [Theme::dark(), Theme::light()] {
            let cells = replayed(theme, &bytes);
            let below = cells
                .iter()
                .filter(|((fg, bg), _)| contrast(*fg, *bg) < 1.2)
                .count();
            assert!(below > 200, "{name}: only {below} cells below the trigger");
            for ((fg, _), slot) in &cells {
                assert_eq!(rgb(slot.fg), rgb(*fg), "{name}: a picture cell was floored");
            }
        }
    }
}

/// **Words, not colours, decide what is exempt.** `OSC 11` sets the page to
/// black and `SGR 30` draws black text on it: two different words -- index 0
/// and the default -- resolving to one colour, 1.00:1. A rule exempting equal
/// *colours* would leave it invisible; the words differ, so it is lifted.
#[test]
fn black_text_on_a_black_page_set_by_osc_11_is_lifted() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    let bytes = b"\x1b]11;rgb:00/00/00\x07\x1b[30mX";
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut table = vec![Resolved::default(); t.styles().len()];
    r.resolve_styles_into(&t, &mut table);
    let slot = table[t.row(Row::from_index(0)).expect("a row on the grid")[0]
        .style
        .index()];
    assert_eq!(rgb(slot.bg), [0, 0, 0], "the control: the page is black");
    assert!(
        contrast(slot.fg, slot.bg) >= 4.5,
        "lifted: {:?}",
        rgb(slot.fg)
    );
}

/// The old default dim, 0.22 of 255 (alpha 56; the default is 0.70 since
/// 2026-10-17): the alpha every background is drawn at under
/// `app_backgrounds = "dim"`.
const DIM: u8 = 56;

fn glass() -> Theme {
    Theme {
        bg_alpha: Chan::from_u8(DIM),
        ..Theme::dark()
    }
}

/// A slot's text as a frame draws it with nothing laid over the cell, or
/// with the cell inside a picture drawn in text, or selected.
fn drawn_fg(slot: Resolved, pictured: Pictured, selected: Selected) -> (Argb, Argb) {
    let d = slot.drawn(Swaps {
        pictured,
        line: Line::No,
        selected,
        block: BlockCursor::No,
        blink: BlinkPhases::SHOWN,
        cursor: Chan::from_u8(0xFF),
    });
    (d.fg, d.bg)
}

/// A slot's text as a cell of `line` draws it unswapped, inside a picture
/// or not.
fn drawn_as(slot: Resolved, pictured: Pictured, line: Line) -> (Argb, Argb) {
    let d = slot.drawn(Swaps {
        pictured,
        line,
        selected: Selected::No,
        block: BlockCursor::No,
        blink: BlinkPhases::SHOWN,
        cursor: Chan::from_u8(0xFF),
    });
    (d.fg, d.bg)
}

/// The page a dimmed background is taken to be drawn as: `bg` at the dim
/// over the default page, channel by channel in encoded sRGB, rounded as
/// `Argb::mix` rounds -- computed here and not through it.
fn dimmed(bg: Argb, page: Argb) -> Argb {
    let a = u32::from(DIM);
    let m = |x: Chan, y: Chan| {
        let v = (u32::from(x.get()) * a + u32::from(y.get()) * (255 - a) + 127) / 255;
        Chan::from_u8(u8::try_from(v).unwrap())
    };
    Argb::rgb(
        m(bg.r(), page.r()),
        m(bg.g(), page.g()),
        m(bg.b(), page.b()),
    )
}

/// **Text the dim makes unreadable is lifted, on herdr's own themes**
/// (`tests/panels/herdr-themes.raw`; the record, "herdr's themes under the
/// glass"). At every read boundary of the capture, under the glass at the
/// old default dim, alpha 56: every glyph on a page the program set, whose pair reads at
/// the trigger or above as asked, reads at the trigger or above against
/// that page dimmed over the default page, and one asked at 3 or above
/// reads at 3 or above. Box-drawing lines are drawn as asked and left out.
#[test]
fn text_the_dim_makes_unreadable_on_herdrs_themes_is_lifted() {
    let bytes = std::fs::read(format!(
        "{}/../tests/panels/herdr-themes.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the capture");
    let theme = glass();
    let page = theme.bg;
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    let (mut below, mut below3, mut lifted, mut asked_set) = (0usize, 0usize, 0usize, 0usize);
    for chunk in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        let mut table = vec![Resolved::default(); t.styles().len()];
        r.resolve_styles_into(&t, &mut table);
        for y in 0..usize::from(WS.ws_row) {
            for cell in t.row(Row::from_index(y)).expect("a row on the grid") {
                let glyph = t.base_of(*cell).as_codepoint();
                if glyph.is_none_or(|c| c.value() == u32::from(b' ')) {
                    continue;
                }
                // A line is drawn as asked, which the capture test pins.
                if Line::of(cell.content) == Line::Yes {
                    continue;
                }
                let style = t.style_of(*cell);
                if words(style).1.is_default() {
                    continue;
                }
                let (fg, bg) = theme.resolve(
                    style,
                    Video::Normal,
                    |ch| t.palette(ch),
                    |d| t.dynamic_color(d),
                );
                if contrast(fg, bg) < 1.2 {
                    continue;
                }
                asked_set += 1;
                let (text, _) = drawn_fg(table[cell.style.index()], Pictured::No, Selected::No);
                let drawn = contrast(text, dimmed(bg, page));
                if drawn < 1.2 {
                    below += 1;
                }
                if contrast(fg, bg) >= 3.0 && drawn < 3.0 {
                    below3 += 1;
                }
                if rgb(text) != rgb(fg) {
                    lifted += 1;
                }
            }
        }
    }
    assert!(
        asked_set > 8_000,
        "the premise: {asked_set} set-page glyphs"
    );
    assert_eq!(
        below, 0,
        "{below} glyphs read below the trigger as drawn ({lifted} lifted)"
    );
    assert_eq!(
        below3, 0,
        "{below3} glyphs asked at 3:1 or above read below it as drawn"
    );
    assert!(lifted > 0, "the positive control: nothing was lifted");
}

/// **The glass trigger is 3** (the owner, 2026-10-17): catppuccin's
/// selected row in herdr, `#181825` on `#89b4fa`, reads 8.34:1 as asked and
/// 1.40:1 dimmed over the dark page -- above 1.2, so the floor's own
/// trigger left it, and below 3 -- and is lifted to the target against the
/// page as drawn.
#[test]
fn catppuccins_selected_row_is_lifted_at_the_glass_trigger() {
    let sgr = "38;2;24;24;37;48;2;137;180;250";
    let (_, _, (fg, bg), slot) = cells(glass(), &[sgr.into()]).remove(0);
    let drawn = dimmed(bg, glass().bg);
    assert!(
        contrast(fg, bg) >= 3.0,
        "the premise: 3:1 or above as asked"
    );
    let before = contrast(fg, drawn);
    assert!(
        (1.2..3.0).contains(&before),
        "the premise: between the two triggers as drawn, {before:.3}"
    );
    let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
    assert!(
        contrast(text, drawn) >= 4.5,
        "lifted against the dimmed page: {:?}, {:.2}",
        rgb(text),
        contrast(text, drawn)
    );
}

/// **The glass trigger never lifts a pair asked below it.** btop's
/// `#4e4e4e` on its own `#000000`, the darkest grey of its process list's
/// fade, reads 2.52:1 as asked and below 3 as drawn, but above 1.2 both
/// ways: drawn as asked.
#[test]
fn a_pair_asked_below_the_glass_trigger_is_left_alone() {
    let sgr = "38;2;78;78;78;48;2;0;0;0";
    let (_, _, (fg, bg), slot) = cells(glass(), &[sgr.into()]).remove(0);
    let drawn = dimmed(bg, glass().bg);
    assert!(
        (1.2..3.0).contains(&contrast(fg, bg)),
        "the premise: {:.3} as asked",
        contrast(fg, bg)
    );
    assert!(
        (1.2..3.0).contains(&contrast(fg, drawn)),
        "the premise: {:.3} as drawn",
        contrast(fg, drawn)
    );
    let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
    assert_eq!(rgb(text), rgb(fg));
}

/// The pixels of the cell at column `col` of row 0.
fn cell_pixels(r: &Renderer<'_, '_>, c: &support::Canvas, col: usize) -> Vec<u32> {
    let m = r.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    (0..ch)
        .flat_map(|y| (col * cw..(col + 1) * cw).map(move |x| (y, x)))
        .map(|(y, x)| c.pixels[y * c.width + x])
        .collect()
}

/// **Box drawing is left out of the floor; block elements are not** (the
/// owner, 2026-10-17). herdr draws its dividers `#15161b` on the default
/// dark page, 1.05:1, and the floor lifted them to a grey that glares. The
/// Box Drawing block, `U+2500..=U+257F`, where DEC Special Graphics' line
/// drawing lands, is drawn as asked with the floor on: each cell's pixels
/// equal the floor off's, on the plain and the glass theme. Block Elements,
/// `U+2580..=U+259F`, which btop and cava draw graphs with, keep the floor:
/// their pixels move. `U+257F` and `U+2580` are the two sides of the edge.
/// The CPU arm; `baud-present-macos`'s `cells.rs` holds the other two to it.
#[test]
fn box_drawing_is_drawn_as_asked_and_block_elements_are_floored() {
    let glyphs = [
        '\u{2500}', '\u{2502}', '\u{256d}', '\u{257f}', '\u{2580}', '\u{2588}',
    ];
    let lines = 4;
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    for theme in [Theme::dark(), glass()] {
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut bytes = b"\x1b[?25l\x1b[38;2;21;22;27m".to_vec();
        for g in glyphs {
            bytes.extend(format!("{g} ").into_bytes());
        }
        t.write_at(BytePos::ZERO, &bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let mut frames = Vec::new();
        for floor in [Floor::DEFAULT, Floor::Off] {
            r.set_theme(Theme {
                floor,
                ..theme.clone()
            });
            let mut canvas = support::Canvas::for_renderer(&r);
            support::paint(&mut r, &mut t, &mut canvas, baud_render::Damage::Full);
            frames.push(
                (0..2 * glyphs.len())
                    .map(|k| cell_pixels(&r, &canvas, k))
                    .collect::<Vec<_>>(),
            );
        }
        for (k, g) in glyphs.iter().enumerate() {
            // Against the blank cell after it.
            let (cell, blank) = (&frames[1][2 * k], &frames[1][2 * k + 1]);
            let ink = cell.iter().zip(blank).filter(|(p, q)| p != q).count();
            assert!(ink > 0, "{g:?}: the control, the glyph drew");
            let same = frames[0][2 * k] == frames[1][2 * k];
            assert_eq!(
                same,
                k < lines,
                "U+{:04X}: {}",
                u32::from(*g),
                if k < lines {
                    "floored"
                } else {
                    "left out of the floor"
                }
            );
        }
    }
}

/// **A bar or underline cursor over a box-drawing line keeps the colour it
/// always had.** The owner's call 3 draws a line as asked; it names lines,
/// not the cursor. herdr's divider `#15161b` reads 1.05:1 on the default
/// page, and a bar over it was drawn in that colour, invisible, where base
/// lifted it. The bar's pixels over `U+2500` equal the bar's over a letter
/// of the same colour with the floor on, and differ from the floor off's.
/// The CPU arm; `glass_floor_gate` in `baud-present-macos`'s `cells.rs`
/// holds the other two to it.
#[test]
fn a_bar_cursor_on_a_line_takes_the_floored_colour() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    for shape in ["\x1b[6 q", "\x1b[4 q"] {
        let mut bar = Vec::new();
        for (g, floor) in [
            ('\u{2500}', Floor::DEFAULT),
            ('x', Floor::DEFAULT),
            ('\u{2500}', Floor::Off),
        ] {
            let mut r = Renderer::new(
                Stack::new(&faces).unwrap(),
                PxSize::of(14.0).unwrap(),
                Theme {
                    floor,
                    ..Theme::dark()
                },
                Blend::Native,
                Cols::of_index(usize::from(WS.ws_col)),
                Rows::of_index(usize::from(WS.ws_row)),
            )
            .unwrap();
            let mut s = Owned::new(&WS);
            let mut t = Term::new(s.storage(), &WS);
            let bytes = format!("\x1b[38;2;21;22;27m{g}\x1b[1;1H{shape}\x1b[?25h").into_bytes();
            t.write_at(BytePos::ZERO, &bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let mut canvas = support::Canvas::for_renderer(&r);
            support::paint(&mut r, &mut t, &mut canvas, baud_render::Damage::Full);
            // The cursor's own pixels: a bar's column 0 on the cell's top
            // row, an underline's last row, either clear of the glyph.
            let m = r.metrics();
            let (cw, ch) = (
                usize::try_from(m.cell_w.value()).unwrap(),
                usize::try_from(m.cell_h.value()).unwrap(),
            );
            let (x, y) = if shape.contains('6') {
                (0, 0)
            } else {
                (cw / 2, ch - 1)
            };
            bar.push(canvas.pixels[y * canvas.width + x]);
        }
        assert_eq!(
            bar[0], bar[1],
            "{shape:?}: a line under the cursor takes a letter's floored colour"
        );
        assert_ne!(bar[0], bar[2], "{shape:?}: the control, the floor moved it");
    }
}

/// **A program's own low-contrast pair stays as asked.** `#222222` text on
/// a `#1e1e1e` page reads 1.04:1 as asked and 1.17:1 dimmed over the dark
/// page: below the trigger both ways, so the program chose it and it is
/// drawn as chosen, inside a picture or out.
#[test]
fn a_programs_own_low_contrast_pair_is_left_alone_under_the_glass() {
    let (_, _, (fg, bg), slot) = cells(glass(), &["38;2;34;34;34;48;2;30;30;30".into()]).remove(0);
    assert!(contrast(fg, bg) < 1.2, "the control: below as asked");
    let drawn = dimmed(bg, glass().bg);
    assert!(
        contrast(fg, drawn) < 1.2,
        "the control: below as drawn, {:.3}",
        contrast(fg, drawn)
    );
    for p in [Pictured::No, Pictured::Yes] {
        assert_eq!(rgb(drawn_fg(slot, p, Selected::No).0), rgb(fg), "{p:?}");
    }
}

/// **Light-theme text on the glass**: gruvbox-light's text on its page,
/// 10.2:1 as asked and 1.14:1 dimmed over the dark page, is lifted until it
/// reads at the target against the page as drawn -- and only where the page
/// is drawn dimmed: inside a picture, selected, on an opaque window, or with
/// `app_backgrounds = "opaque"` the text is the colour asked.
#[test]
fn light_theme_text_on_the_glass_reaches_the_target_where_the_page_is_dimmed() {
    let sgr = "38;2;60;56;54;48;2;251;241;199";
    let (_, _, (fg, bg), slot) = cells(glass(), &[sgr.into()]).remove(0);
    let drawn = dimmed(bg, glass().bg);
    assert!(contrast(fg, bg) >= 4.5, "the premise: readable as asked");
    assert!(
        contrast(fg, drawn) < 1.2,
        "the premise: unreadable as drawn"
    );
    let (text, page) = drawn_fg(slot, Pictured::No, Selected::No);
    assert!(
        contrast(text, drawn) >= 4.5,
        "lifted against the dimmed page: {:?}, {:.2}",
        rgb(text),
        contrast(text, drawn)
    );
    assert_eq!(page.a().get(), DIM, "the page stays glass");
    let (text, page) = drawn_fg(slot, Pictured::Yes, Selected::No);
    assert_eq!((rgb(text), page.a().get()), (rgb(fg), 0xFF), "a picture");
    let (text, page) = drawn_fg(slot, Pictured::No, Selected::Yes);
    assert_eq!((rgb(text), rgb(page)), (rgb(bg), rgb(fg)), "selected");
    for theme in [
        Theme::dark(),
        Theme {
            app_bg: baud_render::AppBg::Opaque,
            ..glass()
        },
    ] {
        let (_, _, _, slot) = cells(theme, &[sgr.into()]).remove(0);
        let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
        assert_eq!(rgb(text), rgb(fg), "a page drawn solid");
    }
}

/// Every `.raw` recording in `tests/recordings`, held to the directory so a
/// new one is read too.
const RECORDINGS: [&str; 12] = [
    "ascii/asciiquarium.raw",
    "ascii/cat-scroll.raw",
    "ascii/git-log.raw",
    "ascii/less-pager.raw",
    "kitty/grad-80x24-placeholder.raw",
    "sixel/swirl.raw",
    "sixel/yazi-walk.raw",
    "utf8/btop.raw",
    "utf8/cat-cjk.raw",
    "utf8/cava.raw",
    "utf8/less-cjk.raw",
    "utf8/vim-unicode.raw",
];

/// **What moves on every capture, and why.** Every `.raw` recording,
/// `tests/panels/`'s four captures and the two chafa previews, each fed a
/// read at a time under the glass: at every read boundary, a glyph whose
/// text is drawn otherwise than [`Floor::lift`] made it is either a
/// box-drawing line, drawn as asked, or sits on a page the program set and
/// reads at a trigger or above on that page and below it against the page
/// dimmed -- 1.2, or [`Floor::GLASS_TRIGGER`] for a pair asked at 3 or
/// above. The glyphs so moved on each final screen are counted and pinned,
/// and the lines the floor and the glass lift would have moved, over every
/// read. The chafa
/// previews are found as pictures and drawn solid, so nothing in them
/// moves.
#[test]
fn every_capture_moves_only_text_the_dim_made_unreadable() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests");
    let mut on_disk: Vec<String> = Vec::new();
    for dir in std::fs::read_dir(format!("{root}/recordings")).expect("the recordings") {
        let dir = dir.expect("an entry").path();
        for f in std::fs::read_dir(&dir).into_iter().flatten() {
            let f = f.expect("an entry").path();
            if f.extension().is_some_and(|e| e == "raw") {
                let sub = dir.file_name().expect("a name").to_string_lossy();
                let name = f.file_name().expect("a name").to_string_lossy();
                on_disk.push(format!("{sub}/{name}"));
            }
        }
    }
    on_disk.sort();
    assert_eq!(on_disk, RECORDINGS, "every recording is read");
    let read = |p: String| std::fs::read(p).expect("the stream");
    let streams: Vec<(&str, Vec<u8>)> = RECORDINGS
        .iter()
        .map(|r| (*r, read(format!("{root}/recordings/{r}"))))
        .chain(
            ["helix", "vimdiff", "vimhl", "herdr-themes"]
                .map(|p| (p, read(format!("{root}/panels/{p}.raw")))),
        )
        .chain(["chafa-240.ans", "chafa-truecolour.ans"].map(|p| (p, placed(p))))
        .collect();
    let theme = glass();
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut got = Vec::new();
    for (name, bytes) in &streams {
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let (mut moved, mut lines, mut glass_lines) = (0, 0, 0);
        for chunk in bytes.chunks(support::CHUNK) {
            t.write_at(BytePos::ZERO, chunk);
            t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
            let mut table = vec![Resolved::default(); t.styles().len()];
            r.resolve_styles_into(&t, &mut table);
            let mut pictures = Pictures::new();
            pictures.find(&t, &t.view(Rows::ZERO), Video::Normal);
            moved = 0;
            for y in 0..usize::from(WS.ws_row) {
                let row = t.row(Row::from_index(y)).expect("a row on the grid");
                for (x, cell) in row.iter().enumerate() {
                    let glyph = t.base_of(*cell).as_codepoint();
                    if glyph.is_none_or(|c| c.value() == u32::from(b' ')) {
                        continue;
                    }
                    let slot = table[cell.style.index()];
                    let pictured = pictures.at(Row::from_index(y), Col::at_index(x));
                    let line = Line::of(cell.content);
                    let (text, _) = drawn_as(slot, pictured, line);
                    let style = t.style_of(*cell);
                    if line == Line::Yes {
                        let (asked, _) = theme.resolve(
                            style,
                            Video::Normal,
                            |ch| t.palette(ch),
                            |d| t.dynamic_color(d),
                        );
                        assert_eq!(rgb(text), rgb(asked), "{name}: a line was floored");
                        let (other, _) = drawn_as(slot, pictured, Line::No);
                        if rgb(slot.fg) != rgb(asked) {
                            lines += 1;
                        } else if rgb(other) != rgb(asked) {
                            glass_lines += 1;
                        }
                        continue;
                    }
                    if rgb(text) == rgb(slot.fg) {
                        continue;
                    }
                    moved += 1;
                    let bg = slot.bg.with_alpha(Chan::from_u8(0xFF));
                    assert!(!words(style).1.is_default(), "{name}: the default page");
                    let (asked, drawn) = (
                        contrast(slot.fg, bg),
                        contrast(slot.fg, dimmed(bg, theme.bg)),
                    );
                    assert!(
                        (asked >= 1.2 && drawn < 1.2) || (asked >= 3.0 && drawn < 3.0),
                        "{name}: {asked:.2} asked, {drawn:.2} drawn"
                    );
                    assert!(contrast(text, dimmed(bg, theme.bg)) >= 4.5, "{name}");
                }
            }
        }
        got.push((*name, moved, lines, glass_lines));
    }
    // On the final screens: vimdiff's `habamax` greys, `#1c1c1c` on
    // `#767676` and on `#9e9e9e`; btop's one grey of its fade that crosses
    // 3; yazi-walk's status bar and powerline caps; herdr's after its
    // settings close. Over its reads, herdr's lines: 2546 its dividers on
    // the default page, which the floor lifted, and 3150 its panels'
    // borders on pages it set, which the glass trigger would lift (3.11-5.82
    // as asked, 1.54-2.69 drawn).
    let want: Vec<(&str, usize, usize, usize)> = streams
        .iter()
        .map(|(n, _)| {
            let (moved, lines, glass) = match *n {
                "vimdiff" => (45, 0, 0),
                "utf8/btop.raw" => (53, 0, 0),
                "sixel/yazi-walk.raw" => (27, 0, 0),
                "herdr-themes" => (1, 2546, 3150),
                _ => (0, 0, 0),
            };
            (*n, moved, lines, glass)
        })
        .collect();
    assert_eq!(got, want);
}

/// A preview placed as yazi places it, each line at the pane's origin a
/// row further down: `pictures.rs`'s placement, where chafa's lines are one
/// picture in every cell.
fn placed(name: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(format!(
        "{}/../tests/previews/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the capture");
    let mut bytes = b"\x1b[1;1H~/photos $ yazi".to_vec();
    for (i, line) in text.split('\n').filter(|l| !l.is_empty()).enumerate() {
        bytes.extend(format!("\x1b[{};21H", i + 4).into_bytes());
        bytes.extend(line.as_bytes());
    }
    bytes
}
