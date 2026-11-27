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
    Argb, Backdrop, Blend, BlinkPhases, BlockCursor, Floor, Hilite, Line, Pictured, Pictures,
    PxSize, Renderer, Resolved, Selected, Shapes, Stack, Swaps, Theme, Video, xterm_palette,
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
    let mut table = vec![Resolved::default(); t.styles().one().len()];
    r.resolve_styles_into(&t, &mut table);
    let mut out = Vec::new();
    'rows: for y in 0..usize::from(WS.ws_row) {
        for cell in t.row(Row::from_index(y)).expect("a row on the grid") {
            if out.len() == sgrs.len() {
                break 'rows;
            }
            let style = t.styles().one().word_of(*cell);
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
    let mut table = vec![Resolved::default(); t.styles().one().len()];
    r.resolve_styles_into(&t, &mut table);
    let mut out = Vec::new();
    for y in 0..usize::from(WS.ws_row) {
        for cell in t.row(Row::from_index(y)).expect("a row on the grid") {
            if cell.content.is_empty() {
                continue;
            }
            let style = t.styles().one().word_of(*cell);
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
            "{}/../../tools/tests/previews/{name}",
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
    let mut table = vec![Resolved::default(); t.styles().one().len()];
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

/// The shipped `dim_dark`, 0.70 (alpha 179): the alpha every background
/// is drawn at under `app_backgrounds = "dim"` and the dark theme.
const DIM: u8 = 179;

/// The shipped `dim_light`, 0.60 (alpha 153).
const LIGHT_DIM: u8 = 153;

fn glass() -> Theme {
    Theme {
        bg_alpha: Chan::from_u8(DIM),
        ..Theme::dark()
    }
}

fn light_glass() -> Theme {
    Theme {
        bg_alpha: Chan::from_u8(LIGHT_DIM),
        ..Theme::light()
    }
}

/// A slot's text as a frame draws it with nothing laid over the cell, or
/// with the cell inside a picture drawn in text, or selected.
fn drawn_fg(slot: Resolved, pictured: Pictured, selected: Selected) -> (Argb, Argb) {
    let d = slot.drawn(Swaps {
        pictured,
        line: Line::No,
        selected,
        hilite: Hilite::REVERSE,
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
        hilite: Hilite::REVERSE,
        block: BlockCursor::No,
        blink: BlinkPhases::SHOWN,
        cursor: Chan::from_u8(0xFF),
    });
    (d.fg, d.bg)
}

/// What can be behind a page the program set, drawn at `alpha` over the
/// glass, at the bound where the material darkens nothing (note AS): the
/// luminances of `bg` at `alpha` over black and over white, channel by
/// channel in encoded sRGB, rounded as `Argb::mix` rounds -- computed here
/// and not through it.
fn range(bg: Argb, alpha: u8) -> (f64, f64) {
    range_over(bg, alpha, ANY)
}

/// Every grey, black to white: the glass's backdrop, [`Backdrop::any`].
const ANY: (u8, u8) = (0, 255);

/// `bg`'s range at `alpha` over a material whose every channel lies
/// between the greys `dark` and `light`.
fn range_over(bg: Argb, alpha: u8, (dark, light): (u8, u8)) -> (f64, f64) {
    let a = u32::from(alpha);
    let over = |v: u32| {
        let m = |x: Chan| {
            let c = (u32::from(x.get()) * a + v * (255 - a) + 127) / 255;
            Chan::from_u8(u8::try_from(c).unwrap())
        };
        lum(Argb::rgb(m(bg.r()), m(bg.g()), m(bg.b())))
    };
    (over(u32::from(dark)), over(u32::from(light)))
}

/// `text`'s contrast against the part of `bg`'s range most like it, WCAG
/// F83's "those parts of the image that are most like the text": 1 when
/// the text falls inside the range.
fn behind(text: Argb, bg: Argb, alpha: u8) -> f64 {
    behind_over(text, bg, alpha, ANY)
}

/// [`behind`] over a material between the greys `greys`.
fn behind_over(text: Argb, bg: Argb, alpha: u8, greys: (u8, u8)) -> f64 {
    let ((lo, hi), l) = (range_over(bg, alpha, greys), lum(text));
    if l >= hi {
        (l + 0.05) / (hi + 0.05)
    } else if l <= lo {
        (lo + 0.05) / (l + 0.05)
    } else {
        1.0
    }
}

/// Whether any colour reads at `target` against all of `bg`'s range.
fn reachable(bg: Argb, alpha: u8, target: f64) -> bool {
    reachable_over(bg, alpha, target, ANY)
}

/// [`reachable`] over a material between the greys `greys`.
fn reachable_over(bg: Argb, alpha: u8, target: f64, greys: (u8, u8)) -> bool {
    let (lo, hi) = range_over(bg, alpha, greys);
    (lo + 0.05) / target - 0.05 >= 0.0 || target * (hi + 0.05) - 0.05 <= 1.0
}

/// Black or white: where the floor puts text no colour lifts to the
/// glass trigger against the range.
fn extreme(c: Argb) -> bool {
    matches!(rgb(c), [0, 0, 0] | [255, 255, 255])
}

/// **Text the dim makes unreadable is lifted, on herdr's own themes**
/// (`tools/tests/panels/herdr-themes.raw`). At every read boundary of the
/// capture, under the glass at the shipped dark dim, alpha 179: every
/// glyph on a page the program set, whose pair reads at the trigger or
/// above as asked, reads at the trigger or above against everything that
/// page can show over the glass, and one asked at 3 or above reads at 3 or
/// above -- unless no colour reaches the target against all of it, and
/// then the text reads at the glass trigger, or is black or white where no
/// colour reaches that either. Box-drawing lines are drawn as asked and
/// left out.
#[test]
fn text_the_dim_makes_unreadable_on_herdrs_themes_is_lifted() {
    let bytes = std::fs::read(format!(
        "{}/../../tools/tests/panels/herdr-themes.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the capture");
    let theme = glass();
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
    let mut beyond = 0usize;
    for chunk in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        let mut table = vec![Resolved::default(); t.styles().one().len()];
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
                let style = t.styles().one().word_of(*cell);
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
                let drawn = behind(text, bg, DIM);
                if rgb(text) != rgb(fg) {
                    lifted += 1;
                }
                if !reachable(bg, DIM, 4.5) {
                    if rgb(text) != rgb(fg) {
                        assert!(
                            if reachable(bg, DIM, 3.0) {
                                drawn >= 3.0
                            } else {
                                extreme(text)
                            },
                            "{:?} beyond the target, {drawn:.2}",
                            rgb(text)
                        );
                        beyond += 1;
                    }
                    continue;
                }
                if drawn < 1.2 {
                    below += 1;
                }
                if contrast(fg, bg) >= 3.0 && drawn < 3.0 {
                    below3 += 1;
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
    assert!(lifted > beyond, "the positive control: nothing was lifted");
}

/// **The glass trigger is 3**: gruvbox-light's text on its `bg3`,
/// `#3c3836` on `#bdae93`, reads 5.32:1 as asked and 2.75:1 against
/// everything its page can show over the glass at alpha 179 -- between
/// the two triggers -- and is lifted to the target against all of it.
/// (Catppuccin's selected row, which this test held at the old dim, reads
/// 4.26 against the range at 179 and is not lifted.)
#[test]
fn a_pair_between_the_triggers_against_the_range_is_lifted() {
    let sgr = "38;2;60;56;54;48;2;189;174;147";
    let (_, _, (fg, bg), slot) = cells(glass(), &[sgr.into()]).remove(0);
    assert!(
        contrast(fg, bg) >= 3.0,
        "the premise: 3:1 or above as asked"
    );
    let before = behind(fg, bg, DIM);
    assert!(
        (1.2..3.0).contains(&before),
        "the premise: between the two triggers against the range, {before:.3}"
    );
    let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
    assert!(
        behind(text, bg, DIM) >= 4.5,
        "lifted against the range: {:?}, {:.2}",
        rgb(text),
        behind(text, bg, DIM)
    );
}

/// **Where no colour reaches the target, as far as the glass trigger**:
/// gruvbox-light's text on its `bg4`, `#3c3836` on `#a89984`, reads
/// 4.17:1 as asked and 2.23:1 against the range at alpha 179. No
/// luminance is 4.5 times beyond either end of the range, so the text is
/// darkened only until it reads 3 against the page over black, keeping
/// its hue, and not to black. Where 3 is out of reach too -- `#87acf5` on
/// `#1e66f5` under the light glass, 2.16 as asked and reading 1.0 inside
/// the range -- it goes to the extreme that reads better, white.
#[test]
fn a_pair_no_colour_lifts_to_the_target_is_lifted_to_the_glass_trigger() {
    let sgr = "38;2;60;56;54;48;2;168;153;132";
    let (_, _, (fg, bg), slot) = cells(glass(), &[sgr.into()]).remove(0);
    assert!(!reachable(bg, DIM, 4.5), "the premise: 4.5 out of reach");
    assert!(reachable(bg, DIM, 3.0), "the premise: 3 in reach");
    assert!(
        (1.2..3.0).contains(&behind(fg, bg, DIM)),
        "the premise: {:.3} against the range",
        behind(fg, bg, DIM)
    );
    let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
    let got = behind(text, bg, DIM);
    assert!((3.0..3.05).contains(&got), "{:?}, {got:.3}", rgb(text));
    assert!(!extreme(text), "{:?}: not black", rgb(text));
    let sgr = "38;2;135;172;245;48;2;30;102;245";
    let (_, _, (fg, bg), slot) = cells(light_glass(), &[sgr.into()]).remove(0);
    assert!(contrast(fg, bg) >= 1.2, "the premise, as asked");
    assert!(
        !reachable(bg, LIGHT_DIM, 3.0),
        "the premise: 3 out of reach"
    );
    let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
    assert_eq!(rgb(text), [255, 255, 255]);
}

/// **Light text on a dark page under the light glass goes light** (note
/// AS, section 6). The page was taken to be drawn over the light default
/// page, so the floor lifted light text toward black: helix's `#dbbfef` on
/// `#452859` at the shipped light dim, alpha 153, to `#18131b`, which
/// reads 1.11:1 where the backdrop is dark; btop's `#cccccc` on `#000000`
/// at the old dim, alpha 56, to `#4b4b4b`, which reads 1:1 against a grey
/// backdrop. Against the range neither pair can reach 4.5. Helix's can
/// reach 3 and is lightened to it, `#f2e1ff`, its hue kept; btop's cannot and goes
/// white, which reads 1.69 where black would read 1.
#[test]
fn light_text_on_a_dark_page_under_the_light_glass_goes_light() {
    let theme = |alpha: u8| Theme {
        bg_alpha: Chan::from_u8(alpha),
        ..Theme::light()
    };
    for (sgr, alpha, want, ratio) in [
        (
            "38;2;219;191;239;48;2;69;40;89",
            LIGHT_DIM,
            [0xf2, 0xe1, 0xff],
            3.0,
        ),
        ("38;2;204;204;204;48;2;0;0;0", 56, [255, 255, 255], 1.69),
    ] {
        let (_, _, (fg, bg), slot) = cells(theme(alpha), &[sgr.into()]).remove(0);
        assert!(contrast(fg, bg) >= 3.0, "{sgr}: the premise, as asked");
        assert!(behind(fg, bg, alpha) < 3.0, "{sgr}: the premise, the range");
        assert!(
            !reachable(bg, alpha, 4.5),
            "{sgr}: the premise, out of reach"
        );
        let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
        assert_eq!(rgb(text), want, "{sgr}");
        let got = behind(text, bg, alpha);
        assert!((ratio..ratio + 0.05).contains(&got), "{sgr}: {got:.3}");
    }
}

/// **The glass trigger never lifts a pair asked below it.** `#595959` on
/// `#a0a0a0` reads 2.68:1 as asked and 1.41:1 against the range at alpha
/// 179, above 1.2 both ways: drawn as asked. (btop's `#4e4e4e` on its own
/// `#000000`, which this test held at the old dim, reads 1.03 against the
/// range at 179, below the floor's own trigger, and is lifted.)
#[test]
fn a_pair_asked_below_the_glass_trigger_is_left_alone() {
    let sgr = "38;2;89;89;89;48;2;160;160;160";
    let (_, _, (fg, bg), slot) = cells(glass(), &[sgr.into()]).remove(0);
    assert!(
        (1.2..3.0).contains(&contrast(fg, bg)),
        "the premise: {:.3} as asked",
        contrast(fg, bg)
    );
    assert!(
        (1.2..3.0).contains(&behind(fg, bg, DIM)),
        "the premise: {:.3} against the range",
        behind(fg, bg, DIM)
    );
    let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
    assert_eq!(rgb(text), rgb(fg));
}

/// A colour in OKLab, through the same sRGB decode as [`lum`]: the
/// distance between two of them is how far apart they look.
fn oklab(c: Argb) -> [f64; 3] {
    let (r, g, b) = (lin(c.r()), lin(c.g()), lin(c.b()));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

fn apart(a: Argb, b: Argb) -> f64 {
    let (x, y) = (oklab(a), oklab(b));
    ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt()
}

/// **A meter stays a meter under the glass.** btop draws its disk meters
/// in U+25A0, the used part in a red ramp and the empty track in
/// `#404040`, all on `#000000`. At alpha 179 both are lifted to the
/// target, and the room above it to white is under 3:1, so SC 1.4.11's
/// 3:1 between adjacent parts cannot be met by any lift there, and btop's
/// own pair is 1.28:1. The bar here is this file's, not WCAG's: the two
/// stay at least half as far apart to the eye (OKLab) as btop drew them,
/// which only the hue can give at one luminance. A linear-light mix to
/// the target washed the red out and drew every meter full.
#[test]
fn a_meters_used_part_stays_apart_from_its_track_under_the_glass() {
    let (used, track) = ("38;2;127;59;59;48;2;0;0;0", "38;2;64;64;64;48;2;0;0;0");
    let got = cells(glass(), &[used.into(), track.into()]);
    let (_, hi) = range(got[0].2.1, DIM);
    let low = 4.5 * (hi + 0.05) - 0.05;
    assert!(
        1.05 / (low + 0.05) < 3.0,
        "the premise: under 3:1 of room above the target"
    );
    let drawn: Vec<(Argb, Argb)> = got
        .iter()
        .map(|(sgr, _, (fg, bg), slot)| {
            let (text, _) = drawn_fg(*slot, Pictured::No, Selected::No);
            assert_ne!(rgb(text), rgb(*fg), "{sgr}: the premise, lifted");
            let reads = behind(text, *bg, DIM);
            assert!(reads >= 4.5, "{sgr}: {reads:.2} against the range");
            (*fg, text)
        })
        .collect();
    let ((u, u2), (t, t2)) = (drawn[0], drawn[1]);
    let (asked, now) = (apart(u, t), apart(u2, t2));
    assert!(
        now >= asked / 2.0,
        "{:?} and {:?}: {now:.3} apart, asked {asked:.3}",
        rgb(u2),
        rgb(t2)
    );
}

/// Linear sRGB of an OKLab colour, unclamped: the inverse of [`oklab`].
fn from_oklab([l, a, b]: [f64; 3]) -> [f64; 3] {
    let cube = |v: f64| v * v * v;
    let l_ = cube(l + 0.396_337_777_4 * a + 0.215_803_757_3 * b);
    let m_ = cube(l - 0.105_561_345_8 * a - 0.063_854_172_8 * b);
    let s_ = cube(l - 0.089_484_177_5 * a - 1.291_485_548 * b);
    [
        4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_,
        -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701 * s_,
    ]
}

/// Which way the glass floor moved a colour.
#[derive(Clone, Copy, PartialEq)]
enum Way {
    Lighter,
    Darker,
}

/// Where the glass floor lands `fg` moved `way` to reach the luminance
/// `y`, found by brute force: `fg`'s OKLab hue at the
/// lightness, bisected sixty times, whose colour first reads at `y`, its
/// chroma bisected forty times at each lightness to the largest sRGB holds.
/// The linear channels, before any rounding to a code.
fn landing(fg: Argb, way: Way, y: f64) -> [f64; 3] {
    let [l0, a, b] = oklab(fg);
    let inside = |c: [f64; 3]| c.iter().all(|v| (-1e-12..=1.0 + 1e-12).contains(v));
    let at = |l: f64| {
        let (mut lo, mut hi) = (0.0, 1.0);
        if inside(from_oklab([l, a, b])) {
            lo = 1.0;
        } else {
            for _ in 0..40 {
                let mid = (lo + hi) / 2.0;
                if inside(from_oklab([l, a * mid, b * mid])) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
        }
        from_oklab([l, a * lo, b * lo]).map(|v| v.clamp(0.0, 1.0))
    };
    let y_of = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let (mut lo, mut hi) = if way == Way::Lighter {
        (l0, 1.0)
    } else {
        (0.0, l0)
    };
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if y_of(at(mid)) >= y {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    at(if way == Way::Lighter { hi } else { lo })
}

/// **The glass floor lands where the brute force does, and reads where it
/// must.** Every colour eight steps a channel, on five pages at the shipped
/// alphas -- btop's black and helix's purple under the dark and light
/// glass, a white page and gruvbox-light's `bg4` -- through
/// [`Floor::lift_on_glass`]. Each lifted colour reads at its target against
/// the page's range and less than 2% above it -- just far enough, but for
/// the rounding to a code -- or is the extreme where no colour does, and
/// lies within 0.006 in OKLab of [`landing`]'s, a third of a
/// just-noticeable difference. The floor solves the lightness by Newton's
/// method or a secant and clips to the gamut in closed form, so this is
/// what holds the shortcut to the definition.
#[test]
fn the_glass_floor_lands_where_the_brute_force_does() {
    let level = |i: u8| i * 36 + 3;
    let pages = [
        (
            Argb::rgb(Chan::from_u8(0), Chan::from_u8(0), Chan::from_u8(0)),
            DIM,
        ),
        (
            Argb::rgb(
                Chan::from_u8(0x45),
                Chan::from_u8(0x28),
                Chan::from_u8(0x59),
            ),
            LIGHT_DIM,
        ),
        (
            Argb::rgb(
                Chan::from_u8(0xff),
                Chan::from_u8(0xff),
                Chan::from_u8(0xff),
            ),
            LIGHT_DIM,
        ),
        (
            Argb::rgb(
                Chan::from_u8(0xa8),
                Chan::from_u8(0x99),
                Chan::from_u8(0x84),
            ),
            DIM,
        ),
        (
            Argb::rgb(
                Chan::from_u8(0x45),
                Chan::from_u8(0x28),
                Chan::from_u8(0x59),
            ),
            DIM,
        ),
    ];
    let (mut lifted, mut worst, mut over) = (0, (0.0, [0u8; 3], [0u8; 3]), 0.0_f64);
    for (page, alpha) in pages {
        let (lo, hi) = range(page, alpha);
        let to = if reachable(page, alpha, 4.5) || !reachable(page, alpha, 3.0) {
            4.5
        } else {
            3.0
        };
        for (r, g, b) in
            (0..8).flat_map(|r| (0..8).flat_map(move |g| (0..8).map(move |b| (r, g, b))))
        {
            let fg = Argb::rgb(
                Chan::from_u8(level(r)),
                Chan::from_u8(level(g)),
                Chan::from_u8(level(b)),
            );
            let got = Floor::DEFAULT.lift_on_glass(fg, page, Chan::from_u8(alpha), Backdrop::any());
            if got == fg {
                continue;
            }
            lifted += 1;
            let way = if lum(got) > lum(fg) {
                Way::Lighter
            } else {
                Way::Darker
            };
            let y = if way == Way::Lighter {
                (to * (hi + 0.05) - 0.05).min(1.0)
            } else {
                ((lo + 0.05) / to - 0.05).max(0.0)
            };
            let reads = behind(got, page, alpha);
            if !extreme(got) {
                over = over.max(reads / to);
            }
            assert!(
                reads >= to * (1.0 - 1e-9) || extreme(got),
                "{:?} on {:?} at {alpha}: {:?} reads {reads:.4}",
                rgb(fg),
                rgb(page),
                rgb(got)
            );
            let want = landing(fg, way, y);
            let code = |v: f64| {
                (0..=u8::MAX)
                    .map(Chan::from_u8)
                    .min_by(|x, y| (lin(*x) - v).abs().total_cmp(&(lin(*y) - v).abs()))
                    .unwrap()
            };
            let want = Argb::rgb(code(want[0]), code(want[1]), code(want[2]));
            let off = apart(got, want);
            if off > worst.0 {
                worst = (off, rgb(got), rgb(want));
            }
        }
    }
    assert!(lifted > 200, "the premise: {lifted} colours lifted");
    assert!(worst.0 <= 0.006, "{worst:?}");
    assert!(over < 1.02, "{over:.4} times the target");
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

/// **Box drawing is left out of the floor; block elements are not**. herdr
/// draws its dividers `#15161b` on the default dark page, 1.05:1, and the
/// floor lifted them to a grey that glares. The Box Drawing block,
/// `U+2500..=U+257F`, where DEC Special Graphics' line drawing lands, is
/// drawn as asked with the floor on: each cell's pixels equal the floor
/// off's, on the plain and the glass theme. Block Elements,
/// `U+2580..=U+259F`, which btop and cava draw graphs with, keep the floor:
/// their pixels move. `U+257F` and `U+2580` are the two sides of the edge.
/// The CPU arm; `baud-present-macos`'s `cells.rs` holds the other two to
/// it.
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
/// always had.** The line rule draws a line as asked; it names lines, not
/// the cursor. herdr's divider `#15161b` reads 1.05:1 on the default page,
/// and a bar over it was drawn in that colour, invisible, where base
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
                // A cursor of no colour of its own, which takes the text's.
                Theme {
                    floor,
                    ..support::classic(Theme::dark())
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
/// a `#1e1e1e` page reads 1.04:1 as asked and 1:1 against the range, inside
/// it: below the trigger both ways, so the program chose it and it is
/// drawn as chosen, inside a picture or out.
#[test]
fn a_programs_own_low_contrast_pair_is_left_alone_under_the_glass() {
    let (_, _, (fg, bg), slot) = cells(glass(), &["38;2;34;34;34;48;2;30;30;30".into()]).remove(0);
    assert!(contrast(fg, bg) < 1.2, "the control: below as asked");
    let drawn = behind(fg, bg, DIM);
    assert!(
        drawn < 1.2,
        "the control: below against the range, {drawn:.3}"
    );
    for p in [Pictured::No, Pictured::Yes] {
        assert_eq!(rgb(drawn_fg(slot, p, Selected::No).0), rgb(fg), "{p:?}");
    }
}

/// **Light-theme text on the glass**: gruvbox-light's text on its `bg3`,
/// 5.32:1 as asked and 2.75:1 against the range at alpha 179, is lifted
/// until it reads at the target against all of it -- and only where the
/// page is drawn dimmed: inside a picture, selected, on an opaque window,
/// or with `app_backgrounds = "opaque"` the text is the colour asked.
#[test]
fn light_theme_text_on_the_glass_reaches_the_target_where_the_page_is_dimmed() {
    let sgr = "38;2;60;56;54;48;2;189;174;147";
    let (_, _, (fg, bg), slot) = cells(glass(), &[sgr.into()]).remove(0);
    assert!(contrast(fg, bg) >= 4.5, "the premise: readable as asked");
    assert!(
        behind(fg, bg, DIM) < 3.0,
        "the premise: below 3 against the range"
    );
    let (text, page) = drawn_fg(slot, Pictured::No, Selected::No);
    assert!(
        behind(text, bg, DIM) >= 4.5,
        "lifted against the range: {:?}, {:.2}",
        rgb(text),
        behind(text, bg, DIM)
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

/// Every `.raw` recording in `tools/tests/recordings`, held to the directory so a
/// new one is read too.
const RECORDINGS: [&str; 16] = [
    "ascii/cat-scroll.raw",
    "ascii/git-log.raw",
    "ascii/less-pager.raw",
    "ascii/pipes.raw",
    "ascii/tmux-logpane.raw",
    "kitty/grad-80x24-placeholder.raw",
    "kitty/icat-logo-dark-140.raw",
    "kitty/mpv-logo-40x6.raw",
    "kitty/mpv-status-in-chunk.raw",
    "sixel/swirl.raw",
    "sixel/yazi-walk.raw",
    "utf8/btop.raw",
    "utf8/cat-cjk.raw",
    "utf8/cava.raw",
    "utf8/less-cjk.raw",
    "utf8/vim-unicode.raw",
];

/// **What moves on every capture, and why.** Every `.raw` recording,
/// `tools/tests/panels/`'s four captures and the two chafa previews, each fed a
/// read at a time under the glass at the shipped dims, the dark theme at
/// alpha 179 and the light at 153: at every read boundary, a glyph whose
/// text is drawn otherwise than [`Floor::lift`] made it is either a
/// box-drawing line, drawn as asked; or sits on the default page, drawn as
/// [`Floor::at_target`] and [`Floor::lift_on_glass`] lift it against black
/// to white at the theme's own alpha; or sits on a page the program set and
/// reads at a trigger or above on that page and below it against the
/// range the page can show over the glass -- 1.2, or
/// [`Floor::GLASS_TRIGGER`] for a pair asked at 3 or above -- and now
/// reads at the target against all of it, or at the glass trigger where
/// no colour reaches the target, or is black or white where none reaches
/// that. The glyphs so moved on a page the program set on each final
/// screen are counted and pinned, and the lines on those pages the floor
/// and the glass lift would have moved, over every read. The chafa
/// previews are found as pictures and drawn solid, so nothing in them
/// moves.
#[test]
fn every_capture_moves_only_text_the_dim_made_unreadable() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/tests");
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
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut got = Vec::new();
    let mut on_default = 0;
    for ((half, theme, alpha), (name, bytes)) in
        [("dark", glass(), DIM), ("light", light_glass(), LIGHT_DIM)]
            .into_iter()
            .flat_map(|h| streams.iter().map(move |s| (h.clone(), s)))
    {
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
            let mut table = vec![Resolved::default(); t.styles().one().len()];
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
                    let style = t.styles().one().word_of(*cell);
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
                        } else if rgb(other) != rgb(asked) && !words(style).1.is_default() {
                            glass_lines += 1;
                        }
                        continue;
                    }
                    if rgb(text) == rgb(slot.fg) {
                        continue;
                    }
                    let bg = slot.bg.with_alpha(Chan::from_u8(0xFF));
                    if words(style).1.is_default() {
                        let a = Chan::from_u8(alpha);
                        let lifted =
                            theme
                                .floor
                                .at_target()
                                .lift_on_glass(slot.fg, bg, a, Backdrop::any());
                        assert_eq!(rgb(text), rgb(lifted), "{half} {name}: the default page");
                        assert_eq!(slot.bg.a().get(), alpha, "{half} {name}: the page's alpha");
                        on_default += 1;
                        continue;
                    }
                    moved += 1;
                    let (asked, drawn) = (contrast(slot.fg, bg), behind(slot.fg, bg, alpha));
                    assert!(
                        (asked >= 1.2 && drawn < 1.2) || (asked >= 3.0 && drawn < 3.0),
                        "{half} {name}: {asked:.2} asked, {drawn:.2} against the range"
                    );
                    let reads = behind(text, bg, alpha);
                    assert!(
                        reads >= 4.5
                            || (!reachable(bg, alpha, 4.5) && reads >= 3.0)
                            || (!reachable(bg, alpha, 3.0) && extreme(text)),
                        "{half} {name}: {:?}, {reads:.2}",
                        rgb(text)
                    );
                }
            }
        }
        got.push((half, *name, moved, lines, glass_lines));
    }
    assert!(
        on_default > 0,
        "the control: no text on the default page moved"
    );
    // On the final screens, at the shipped dims, on pages the program set:
    // btop's graph greys, helix's and vimdiff's panels; under the light
    // theme, light text on dark pages goes light. pipes draws on the
    // default page, which this count leaves out. Over every read, the
    // lines: herdr's dividers on the default page, which the floor lifted
    // (2546 dark, 1211 light), and
    // the borders on pages the program set that the glass trigger would
    // lift: btop's 156194, helix's 123 and herdr's panels.
    // yazi-walk's 17 on dark came with the dark sixteen's corrections of
    // 2026-10-18: its 11 blue letters on `SGR 47`, `#3771ff` on `#e5e5e5`
    // (3.36 asked, `#0000ee` was 7.46), and 6 reversed letters in the
    // page's `#101010` on the blue, 4.50 asked where `#0000ee` gave 2.02,
    // both below 3 over the glass.
    let want: Vec<(&str, &str, usize, usize, usize)> = ["dark", "light"]
        .into_iter()
        .flat_map(|h| streams.iter().map(move |(n, _)| (h, *n)))
        .map(|(h, n)| {
            let (moved, lines, glass) = match (h, n) {
                ("dark", "utf8/btop.raw") => (599, 0, 156_194),
                ("dark", "helix") => (1195, 0, 123),
                ("dark", "vimdiff") => (2667, 0, 0),
                ("dark", "vimhl") => (70, 0, 0),
                ("dark", "herdr-themes") => (2, 2546, 7610),
                ("dark", "sixel/yazi-walk.raw") => (17, 0, 0),
                ("light", "sixel/yazi-walk.raw") => (27, 0, 0),
                ("light", "utf8/btop.raw") => (722, 0, 156_194),
                ("light", "helix") => (1665, 0, 123),
                ("light", "vimdiff") => (5225, 0, 0),
                ("light", "vimhl") => (1344, 0, 0),
                ("light", "herdr-themes") => (2, 1211, 8673),
                _ => (0, 0, 0),
            };
            (h, n, moved, lines, glass)
        })
        .collect();
    assert_eq!(got, want);
}

/// A preview placed as yazi places it, each line at the pane's origin a
/// row further down: `pictures.rs`'s placement, where chafa's lines are one
/// picture in every cell.
fn placed(name: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(format!(
        "{}/../../tools/tests/previews/{name}",
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

/// The see-through standard material's greys (note AT's Sidebar recipe
/// over a black desktop and a white one, rounded outward), as
/// [`Backdrop::under_window_dark`] and [`Backdrop::under_window_light`]
/// carry them.
const UNDER_DARK: (u8, u8) = (33, 92);
const UNDER_LIGHT: (u8, u8) = (196, 235);

/// **Default text reads at 4.5:1 over the standard material, whatever the
/// desktop**, at the shipped dims: the text on the default page at alpha
/// 179 and 153, against the page over either end of its half's greys.
/// The ends bound every desktop, so this holds over each of note AT's 21
/// wallpapers; at the ends it is note AT's white and black rows, 10.52
/// and 12.77 dark, 12.78 and 11.10 light, to within the greys' rounding
/// outward and the mix's rounding to a code.
#[test]
fn default_text_reads_at_the_target_over_the_standard_material() {
    for (theme, alpha, greys, want) in [
        (Theme::dark(), DIM, UNDER_DARK, (10.52, 12.77)),
        (Theme::light(), LIGHT_DIM, UNDER_LIGHT, (11.10, 12.78)),
    ] {
        let (lo, hi) = range_over(theme.bg, alpha, greys);
        let t = lum(theme.fg);
        let ratio = |b: f64| (t.max(b) + 0.05) / (t.min(b) + 0.05);
        let (a, b) = (ratio(lo), ratio(hi));
        let (low, high) = (a.min(b), a.max(b));
        assert!(low >= 4.5, "{low:.2}");
        assert!(
            (low - want.0).abs() < 0.05 && (high - want.1).abs() < 0.05,
            "note AT's rows: {low:.2} / {high:.2}"
        );
    }
}

/// **Over the standard material the floor reads text against the
/// material's own range** (note AT): far narrower than the glass's black
/// to white, so far less is lifted. btop's `#404040` meter ground on its
/// black page reads 1.03:1 against the glass's range at alpha 179 and is
/// lifted to `#bcbcbc`; against the standard material's it reads 1.66:1,
/// above the trigger, and is drawn as btop drew it.
#[test]
fn btops_meter_ground_is_drawn_as_asked_over_the_standard_material() {
    let sgr = "38;2;64;64;64;48;2;0;0;0";
    let standard = Theme {
        bg_alpha: Chan::from_u8(DIM),
        backdrop: Backdrop::under_window_dark(),
        ..Theme::dark()
    };
    let (_, _, (fg, bg), slot) = cells(glass(), &[sgr.into()]).remove(0);
    let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
    assert_eq!(rgb(text), [0xbc; 3], "the control: lifted over the glass");
    assert!(behind_over(fg, bg, DIM, UNDER_DARK) >= 1.2, "the premise");
    let (_, _, _, slot) = cells(standard, &[sgr.into()]).remove(0);
    let (text, _) = drawn_fg(slot, Pictured::No, Selected::No);
    assert_eq!(rgb(text), rgb(fg), "drawn as asked");
}

/// **Every pair reads where the floor promises, over the standard
/// material.** Every colour eight steps a channel on the five pages of
/// [`the_glass_floor_lands_where_the_brute_force_does`], each half under
/// its own material's greys: a pair asked at a trigger or above that the
/// floor keeps reads at it against the range, and a lifted one reads at
/// its target -- 4.5, or 3 where 4.5 is out of reach -- or is the extreme.
/// Fewer are lifted than over the glass on every page.
#[test]
fn the_floor_over_the_standard_material_keeps_its_promise() {
    let level = |i: u8| i * 36 + 3;
    let page =
        |r: u8, g: u8, b: u8| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b));
    let dark = (Backdrop::under_window_dark(), UNDER_DARK, DIM);
    let light = (Backdrop::under_window_light(), UNDER_LIGHT, LIGHT_DIM);
    let cases = [
        (page(0, 0, 0), dark),
        (page(0x45, 0x28, 0x59), light),
        (page(0xff, 0xff, 0xff), light),
        (page(0xa8, 0x99, 0x84), dark),
        (page(0x45, 0x28, 0x59), dark),
    ];
    let mut total = 0;
    for (page, (backdrop, greys, alpha)) in cases {
        let to = if reachable_over(page, alpha, 4.5, greys)
            || !reachable_over(page, alpha, 3.0, greys)
        {
            4.5
        } else {
            3.0
        };
        let (mut lifted, mut over_glass) = (0, 0);
        for (r, g, b) in
            (0..8).flat_map(|r| (0..8).flat_map(move |g| (0..8).map(move |b| (r, g, b))))
        {
            let fg = Argb::rgb(
                Chan::from_u8(level(r)),
                Chan::from_u8(level(g)),
                Chan::from_u8(level(b)),
            );
            let a = Chan::from_u8(alpha);
            if Floor::DEFAULT.lift_on_glass(fg, page, a, Backdrop::any()) != fg {
                over_glass += 1;
            }
            let got = Floor::DEFAULT.lift_on_glass(fg, page, a, backdrop);
            let asked = contrast(fg, page);
            let k = if asked >= 3.0 { 3.0 } else { 1.2 };
            let reads = behind_over(got, page, alpha, greys);
            if got == fg {
                assert!(
                    asked < k || reads >= k,
                    "{:?} on {:?}: kept at {reads:.3}",
                    rgb(fg),
                    rgb(page)
                );
            } else {
                lifted += 1;
                assert!(
                    reads >= to * (1.0 - 1e-9) || extreme(got),
                    "{:?} on {:?}: {:?} reads {reads:.4}",
                    rgb(fg),
                    rgb(page),
                    rgb(got)
                );
            }
        }
        assert!(
            lifted < over_glass,
            "{:?}: {lifted} lifted, {over_glass} over the glass",
            rgb(page)
        );
        total += lifted;
    }
    assert!(total > 50, "the premise: {total} colours lifted");
}
