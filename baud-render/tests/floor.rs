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
    Argb, Blend, BlinkPhases, BlockCursor, Floor, Pictured, Pictures, PxSize, Renderer, Resolved,
    Selected, Shapes, Stack, Swaps, Theme, Video, xterm_palette,
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

/// `[glass] dim_dark`'s default, 0.22 of 255 rounded: the alpha every
/// background is drawn at under `app_backgrounds = "dim"`.
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
        selected,
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
/// default dim: every glyph on a page the program set, whose pair reads at
/// the trigger or above as asked, reads at the trigger or above against
/// that page dimmed over the default page.
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
    let (mut below, mut lifted, mut asked_set) = (0usize, 0usize, 0usize);
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
                if contrast(text, dimmed(bg, page)) < 1.2 {
                    below += 1;
                }
                if rgb(text) != rgb(fg) {
                    lifted += 1;
                }
            }
        }
    }
    assert!(
        asked_set > 10_000,
        "the premise: {asked_set} set-page glyphs"
    );
    assert_eq!(
        below, 0,
        "{below} glyphs read below the trigger as drawn ({lifted} lifted)"
    );
    assert!(lifted > 0, "the positive control: nothing was lifted");
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

/// **What moves on every capture, and why.** The nine recordings,
/// `tests/panels/`'s four captures and the two chafa previews, each fed a
/// read at a time under the glass: at every read boundary, a glyph whose
/// text is drawn otherwise than [`Floor::lift`] made it sits on a page the
/// program set, reads at the trigger or above on that page, and below it
/// against the page dimmed. The glyphs so moved on each final screen are
/// counted and pinned. The chafa previews are found as pictures and drawn
/// solid, so nothing in them moves.
#[test]
fn every_capture_moves_only_text_the_dim_made_unreadable() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests");
    let read = |p: String| std::fs::read(p).expect("the stream");
    let streams: Vec<(&str, Vec<u8>)> = support::RECORDINGS
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
        let mut moved = 0;
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
                    let (text, _) = drawn_fg(slot, pictured, Selected::No);
                    if rgb(text) == rgb(slot.fg) {
                        continue;
                    }
                    moved += 1;
                    let bg = slot.bg.with_alpha(Chan::from_u8(0xFF));
                    let style = t.style_of(*cell);
                    assert!(!words(style).1.is_default(), "{name}: the default page");
                    assert!(contrast(slot.fg, bg) >= 1.2, "{name}: asked below");
                    assert!(
                        contrast(slot.fg, dimmed(bg, theme.bg)) < 1.2,
                        "{name}: readable as drawn"
                    );
                    assert!(contrast(text, dimmed(bg, theme.bg)) >= 4.5, "{name}");
                }
            }
        }
        got.push((*name, moved));
    }
    // vimdiff's 22: `habamax`'s `#1c1c1c` on `#767676`, 3.75:1 asked and
    // 1.13:1 drawn. herdr's lifts are on the screens before its last,
    // which `text_the_dim_makes_unreadable_on_herdrs_themes_is_lifted` reads.
    let want: Vec<(&str, usize)> = streams
        .iter()
        .map(|(n, _)| (*n, if *n == "vimdiff" { 22 } else { 0 }))
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
