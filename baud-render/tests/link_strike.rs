//! **A struck link, on the CPU path**: struck through in the colour its text
//! is drawn in, that text in `[link] missing_color` only where it reads
//! 4.5:1 on the cell in the colours in force at draw, and nothing of it in a
//! cell, so no query of the core's reads it back.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, LinkLine, MissingPaint, PxSize, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], damage: Damage) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(px, w, w, h).unwrap();
    r.render(t, &mut target, damage);
}

/// The run `from..=to` of row 0, in its cells' rendition.
fn run(t: &Term<'_>, from: usize, to: usize) -> LinkLine {
    let cell = t.view(Rows::ZERO).row(Row::ZERO)[from];
    LinkLine {
        row: Row::ZERO,
        from: Col::at_index(from),
        to: Col::at_index(to),
        word: t.styles().one().word_of(cell),
    }
}

/// The cell's width and the row's stride, in pixels.
fn geometry(r: &Renderer<'_, '_>) -> (usize, usize) {
    (
        usize::try_from(r.metrics().cell_w.value()).unwrap(),
        usize::try_from(r.pixel_size().0.value()).unwrap(),
    )
}

/// The pixels of row 0's cells `from..=to` at line `y` of the row.
fn line_at(
    px: &[u32],
    (cw, stride): (usize, usize),
    (from, to): (usize, usize),
    y: Px,
) -> Vec<u32> {
    let y = usize::try_from(y.value()).unwrap();
    (from * cw..(to + 1) * cw)
        .map(|x| px[y * stride + x] & 0x00FF_FFFF)
        .collect()
}

/// **The strike takes `missing_color` where it reads, and the text's colour
/// where it does not, measured at draw**: red text struck, with colour 8 set
/// by `OSC 4` to a light gray that reads on the dark page, is struck in that
/// gray and its glyphs move; with colour 8 set to `#202020`, which does not
/// read on that page, the strike is the text's red. With `"none"` it is red
/// whatever colour 8 is. Taken away, the frame is the plain one. Each case
/// would fail if the colour were read from the config rather than the
/// palette in force (the `OSC 4` case), or if the contrast were not checked
/// (the dark case). Perturbations: resolve colour 8 from the theme's own
/// palette in `missing_rendition`; drop the contrast test there; drop
/// `strikes_changed` from the convert's damage.
#[test]
fn the_strike_reads_where_gray_reads_and_keeps_the_text_colour_elsewhere() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m",
    );
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut plain = vec![0u32; n];
    paint(&mut r, &mut t, &mut plain, Damage::Full);
    let m = r.metrics();
    let strike = usize::try_from(m.strike.value()).unwrap();
    let cells = (4, 24);
    let at = geometry(&r);
    let link = run(&t, cells.0, cells.1);
    // Gray that reads on the dark page: the strike is it, the glyphs move.
    feed(&mut t, b"\x1b]4;8;rgb:c0/c0/c0\x1b\\");
    r.set_link_strikes(&[link], &[]);
    let mut grey = vec![0u32; n];
    paint(&mut r, &mut t, &mut grey, Damage::Full);
    assert!(
        line_at(&grey, at, cells, m.strike)
            .iter()
            .all(|&p| p == 0x00C0_C0C0),
        "struck in colour 8 as OSC 4 set it"
    );
    let glyphs = |px: &[u32]| {
        (0..usize::try_from(m.cell_h.value()).unwrap())
            .filter(|&y| y < strike || y >= strike + usize::try_from(m.thickness.value()).unwrap())
            .flat_map(|y| line_at(px, at, cells, Px::of(u32::try_from(y).unwrap())))
            .collect::<Vec<u32>>()
    };
    assert_ne!(glyphs(&grey), glyphs(&plain), "the text drawn in gray");
    // Gray that does not read there: the strike is the text's red.
    feed(&mut t, b"\x1b]4;8;rgb:20/20/20\x1b\\");
    let mut dark = vec![0u32; n];
    paint(&mut r, &mut t, &mut dark, Damage::Full);
    assert!(
        line_at(&dark, at, cells, m.strike)
            .iter()
            .all(|&p| p == 0x00FF_0000),
        "under 4.5:1 the text keeps its colour, and the strike is in it"
    );
    assert!(
        !glyphs(&dark).contains(&0x0020_2020),
        "no glyph drawn in a gray that does not read"
    );
    // `"none"`: the text's own colour, whatever colour 8 is.
    feed(&mut t, b"\x1b]4;8;rgb:c0/c0/c0\x1b\\");
    r.set_link_missing(MissingPaint::Kept);
    let mut kept = vec![0u32; n];
    paint(&mut r, &mut t, &mut kept, Damage::Full);
    assert!(
        line_at(&kept, at, cells, m.strike)
            .iter()
            .all(|&p| p == 0x00FF_0000),
        "none: struck in the text's red"
    );
    // Gone: the plain frame, incremental as full.
    r.set_link_missing(MissingPaint::Gray);
    r.set_link_strikes(&[], &[]);
    let mut after = grey.clone();
    paint(&mut r, &mut t, &mut after, Damage::Incremental);
    let mut full = vec![0u32; n];
    paint(&mut r, &mut t, &mut full, Damage::Full);
    assert!(after == full, "an incremental frame is the full one");
    assert!(
        line_at(&after, at, cells, m.strike) == line_at(&plain, at, cells, m.strike),
        "nothing of the strike is left behind"
    );
}

/// **No program can read a strike back**: `DECRQCRA` answers the same with
/// struck runs drawn as without, and the core's cells are the same to the
/// byte, since the grey is lent in the renderer's own copy and nothing of it
/// is a cell's. The control: the same text written underlined, an
/// attribute the checksum weighs (`SGR 9` is not one), moves the answer.
/// Perturbation: lend the struck slots over the core's own rows.
#[test]
fn decrqcra_answers_the_same_with_a_link_struck() {
    let checksum = |t: &mut Term<'_>| {
        t.clear_reply();
        feed(t, b"\x1b[7;1;1;1;1;80*y");
        t.reply().to_vec()
    };
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25lsee https://example.org/x");
    let before = checksum(&mut t);
    assert!(!before.is_empty(), "the control: the query answers");
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let link = run(&t, 4, 24);
    let cells = t.view(Rows::ZERO).row(Row::ZERO).to_vec();
    r.set_link_strikes(&[link], &[link]);
    paint(&mut r, &mut t, &mut px, Damage::Full);
    assert_eq!(checksum(&mut t), before, "the same with the strike up");
    assert_eq!(
        t.view(Rows::ZERO).row(Row::ZERO),
        &cells[..],
        "no cell changed"
    );
    feed(&mut t, b"\x1b[1;5H\x1b[4mhttps\x1b[m");
    assert_ne!(checksum(&mut t), before, "the control: as cells it moves");
}

/// The renderer the tests below draw with, over `faces`, at `WS`.
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

/// **A struck link's text is drawn grey by its own cells, and a selected
/// cell of it keeps the selection's colours**: the cells' text colour is
/// colour 8 as `OSC 4` set it, each through the slot the frame lent its
/// rendition, with no field over them; a cell the selection covers is the
/// colour the same selection draws with nothing struck, its ground
/// included, which reverse video takes from the text. The text never moves
/// out of the cells, so no record of baud's own is drawn over them.
/// Perturbation, run red: leave the selected cells in the struck cells
/// `Renderer::take_struck` finds.
#[test]
fn a_struck_links_text_is_grey_in_its_cells_and_the_selection_wins() {
    use baud_render::{Highlight, Region};
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m\x1b]4;8;rgb:c0/c0/c0\x1b\\",
    );
    let mut r = renderer(&faces);
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let cols = usize::from(WS.ws_col);
    let sel = Highlight::new(
        (Row::ZERO, Col::at_index(10)),
        (Row::ZERO, Col::at_index(12)),
        Region::Stream,
    );
    // The selection with nothing struck: the reference for its cells.
    r.set_highlight(Some(sel));
    paint(&mut r, &mut t, &mut px, Damage::Full);
    let selected = |r: &Renderer<'_, '_>| {
        (10..=12)
            .map(|x| (r.foreground()[x], r.background()[x]))
            .collect::<Vec<_>>()
    };
    let reference = selected(&r);
    r.set_link_strikes(&[run(&t, 4, 24)], &[]);
    paint(&mut r, &mut t, &mut px, Damage::Full);
    let grey = |x: usize| r.foreground()[x].0 & 0x00FF_FFFF;
    for x in (4..=9).chain(13..=24) {
        assert_eq!(grey(x), 0x00C0_C0C0, "cell {x} drawn in colour 8");
    }
    assert_eq!(selected(&r), reference, "the selection's colours win");
    assert_eq!(
        grey(3),
        r.foreground()[cols].0 & 0x00FF_FFFF,
        "the space before keeps its own"
    );
    assert_eq!(
        r.chrome().value(),
        0,
        "the text stays in its cells: no field over it"
    );
}

/// The renderer [`renderer`] is, on a window drawn at the default dim,
/// `0.70`, over [`baud_render::Backdrop::any`]: the glass every cell of the
/// default page is drawn on.
fn glass<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    let mut theme = Theme::dark();
    theme.bg_alpha = baud_domain::Chan::from_u8(0xB3);
    theme.backdrop = baud_render::Backdrop::any();
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// **On the glass a struck cell is as dim as its neighbours**: its text is
/// colour 8, white here, which reads on the dimmed page, and its ground is
/// the unstruck space's beside it, alpha and all. Perturbation, run red:
/// resolve the struck rendition on the page made opaque.
#[test]
fn on_the_glass_a_struck_cells_ground_is_its_neighbours() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m\x1b]4;8;rgb:ff/ff/ff\x1b\\",
    );
    let mut r = glass(&faces);
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    r.set_link_strikes(&[run(&t, 4, 24)], &[]);
    paint(&mut r, &mut t, &mut px, Damage::Full);
    let page = r.background()[3];
    assert!(page.a().get() < 0xFF, "the premise: the page is dimmed");
    for x in 4..=24 {
        assert_eq!(
            r.foreground()[x].0 & 0x00FF_FFFF,
            0x00FF_FFFF,
            "cell {x} struck in colour 8"
        );
        assert_eq!(
            r.background()[x],
            page,
            "cell {x} on its neighbour's ground"
        );
    }
}

/// **A program's underline colour survives the strike**: a link written
/// underlined in `SGR 58`'s green is struck in colour 8 and underlined in
/// the green still. Perturbation, run red: give the struck rendition its
/// text's colour as the underline's.
#[test]
fn a_struck_links_sgr_58_underline_keeps_its_colour() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[4;58;2;0;255;0;38;2;255;0;0mhttps://example.org/x\x1b[m\x1b]4;8;rgb:c0/c0/c0\x1b\\",
    );
    let mut r = renderer(&faces);
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    r.set_link_strikes(&[run(&t, 4, 24)], &[]);
    paint(&mut r, &mut t, &mut px, Damage::Full);
    let m = r.metrics();
    let at = geometry(&r);
    assert!(
        line_at(&px, at, (4, 24), m.strike)
            .iter()
            .all(|&p| p == 0x00C0_C0C0),
        "the control: struck in colour 8"
    );
    let line = line_at(&px, at, (4, 24), m.underline);
    assert!(
        line.iter().all(|&p| p == 0x0000_FF00),
        "underlined in SGR 58's green: {:x?}",
        line.iter()
            .filter(|&&p| p != 0x0000_FF00)
            .take(4)
            .collect::<Vec<_>>()
    );
}

/// **The grey is weighed against the page shown, the dimmed one**: colour 8
/// at `#c0c0c0` reads 9:1 on the dark page drawn solid and under 4:1 on the
/// same page at 0.70 over a white desktop, so the solid window strikes the
/// link grey and the glass one keeps its red. Perturbation, run red: weigh
/// the grey against the page made opaque.
#[test]
fn the_grey_is_weighed_against_the_dimmed_page() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let bytes: &[u8] =
        b"\x1b[?25lsee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m\x1b]4;8;rgb:c0/c0/c0\x1b\\";
    let struck = |r: &mut Renderer<'_, '_>| {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes);
        r.fit(&t);
        let (w, h) = r.pixel_size();
        let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
        r.set_link_strikes(&[run(&t, 4, 24)], &[]);
        paint(r, &mut t, &mut px, Damage::Full);
        (4..=24)
            .map(|x| r.foreground()[x].0 & 0x00FF_FFFF)
            .collect::<Vec<_>>()
    };
    assert!(
        struck(&mut renderer(&faces))
            .iter()
            .all(|&c| c == 0x00C0_C0C0),
        "the control: on the solid page the grey reads"
    );
    assert!(
        struck(&mut glass(&faces)).iter().all(|&c| c == 0x00FF_0000),
        "on the dimmed page it does not, and the text keeps its red"
    );
}

/// A renderer over `bytes` at `px` pixels, JetBrains Mono, fitted.
fn struck_frame<'f>(faces: &'f [Face<'f>], t: &Term<'_>, px: f32) -> Renderer<'f, 'f> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(px).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(t);
    r
}

/// **A struck link is underlined as a live one is**: struck at rest, its
/// run carries the strike in the text's colour and, under it, the live
/// line at `Metrics::struck_underline` in the box's colour, the colour a
/// live link's line at rest takes beside it; struck by the gesture under
/// the pointer, its line takes the text's colour, as a live line under the
/// pointer does. With `[link] line = "off"` it is struck and has no line.
/// Red before: a struck run drew the strike alone. Perturbations: no line
/// under a struck run (the first two fail); the held run's line in the
/// box's colour (the second fails); the line under a struck run whatever
/// `line` says (the third fails).
#[test]
fn a_struck_link_is_underlined_as_a_live_one_is() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25l\x1b]4;8;rgb:c0/c0/c0\x1b\\see https://example.org/x https://example.org/y",
    );
    let mut r = struck_frame(&faces, &t, 14.0);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let m = r.metrics();
    let at = geometry(&r);
    let (struck, live) = ((4, 24), (26, 46));
    let mut plain = vec![0u32; n];
    paint(&mut r, &mut t, &mut plain, Damage::Full);
    // At rest: a live line beside the struck run gives the box's colour.
    r.set_link_lines(&[run(&t, live.0, live.1)], baud_render::LineStyle::Solid);
    r.set_link_strikes(&[run(&t, struck.0, struck.1)], &[]);
    let mut rest = vec![0u32; n];
    paint(&mut r, &mut t, &mut rest, Damage::Full);
    let boxed = line_at(&rest, at, live, m.underline);
    assert!(
        boxed.iter().all(|&p| p == boxed[0]) && boxed[0] != 0x00C0_C0C0,
        "the control: the live line at rest, in one colour not the text's"
    );
    assert!(
        line_at(&rest, at, struck, m.strike)
            .iter()
            .all(|&p| p == 0x00C0_C0C0),
        "the strike, in the text's colour"
    );
    assert!(
        line_at(&rest, at, struck, m.struck_underline())
            .iter()
            .all(|&p| p == boxed[0]),
        "the struck run's line at rest, in the box's colour"
    );
    // Struck by the gesture under the pointer: the line in the text's colour.
    r.set_link_strikes(&[], &[run(&t, struck.0, struck.1)]);
    let mut held = vec![0u32; n];
    paint(&mut r, &mut t, &mut held, Damage::Full);
    assert!(
        line_at(&held, at, struck, m.struck_underline())
            .iter()
            .all(|&p| p == 0x00C0_C0C0),
        "under the pointer, the line in the text's colour"
    );
    // `line = "off"`: struck, no line.
    r.set_link_lines(&[run(&t, live.0, live.1)], baud_render::LineStyle::Off);
    r.set_link_strikes(&[run(&t, struck.0, struck.1)], &[]);
    let mut off = vec![0u32; n];
    paint(&mut r, &mut t, &mut off, Damage::Full);
    assert!(
        line_at(&off, at, struck, m.strike)
            .iter()
            .all(|&p| p == 0x00C0_C0C0),
        "still struck"
    );
    assert!(
        !line_at(&off, at, struck, m.struck_underline()).contains(&boxed[0]),
        "and no line"
    );
    assert_eq!(
        line_at(&off, at, live, m.underline),
        line_at(&plain, at, live, m.underline),
        "the control: no live line either"
    );
}

/// **A cell a program underlines keeps its own line, struck**: a struck
/// run written with `SGR 4` is struck and gets no link line in the box's
/// colour over it, its own underline drawn by the cell instead. The
/// control: the same run without `SGR 4` gets the box-coloured line.
/// Perturbation: draw the link line over a cell's own underline.
#[test]
fn a_struck_cell_a_program_underlines_keeps_its_own_line() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[4mhttps://example.org/x\x1b[m https://example.org/y",
    );
    let mut r = struck_frame(&faces, &t, 14.0);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let m = r.metrics();
    let at = geometry(&r);
    let (own, bare) = ((4, 24), (26, 46));
    r.set_link_lines(&[run(&t, bare.0, bare.1)], baud_render::LineStyle::Solid);
    let mut live = vec![0u32; n];
    paint(&mut r, &mut t, &mut live, Damage::Full);
    let boxed = line_at(&live, at, bare, m.underline)[0];
    r.set_link_lines(&[], baud_render::LineStyle::Solid);
    r.set_link_strikes(&[run(&t, own.0, own.1), run(&t, bare.0, bare.1)], &[]);
    let mut px = vec![0u32; n];
    paint(&mut r, &mut t, &mut px, Damage::Full);
    assert!(
        line_at(&px, at, bare, m.struck_underline())
            .iter()
            .all(|&p| p == boxed),
        "the control: a bare struck run's line, in the box's colour"
    );
    assert!(
        !line_at(&px, at, own, m.struck_underline()).contains(&boxed),
        "no link line over a cell's own underline"
    );
}

/// **JetBrains Mono's struck line never touches its strike**: at every
/// size from 6 to 40 pixels the underline's top is at least a pixel under
/// the strike's bottom, at the face's own underline (the guard moves
/// nothing), inside the cell.
#[test]
fn jetbrains_monos_struck_line_clears_its_strike_at_every_size() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let t = Term::new(owned.storage(), &WS);
    for size in 6..=40u16 {
        let r = struck_frame(&faces, &t, f32::from(size));
        let m = r.metrics();
        let under = m.struck_underline();
        assert!(
            under.value() > m.strike.value() + m.thickness.value(),
            "{size} px: {m:?}"
        );
        assert_eq!(under, m.underline, "{size} px: the face's own place");
        assert!(under.value() + m.thickness.value() <= m.cell_h.value());
    }
}
