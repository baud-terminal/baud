//! **The status line reaches the pixels**: a row below the main display,
//! the main display's rows kept (`Term::status_rows`). The indicator is
//! drawn in reverse video, "It appears in reverse video (negative image) on
//! the line below the last line of the user window" (`[[source:
//! vt420-rm]]` p. 219), and its background is "the reverse of the Main
//! Display background" (`[[source: dec-std-070]]` 070-14 p. 14-5); the
//! host-writable line "has the same background as the main display" (the
//! same page), and the cursor is drawn there while DECSASD selects it.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, Content, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Damage, Grain, Ground, Highlight, Instance, Kind, Renderer, Scrolled, Selection, Span,
    Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 12,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn rgb(c: Argb) -> u32 {
    c.0 & 0x00FF_FFFF
}

/// One full CPU frame of `bytes`, handed over with the canvas.
fn frame(bytes: &[u8], f: impl FnOnce(&Renderer<'_, '_>, &support::Canvas, &Term<'_>)) {
    frame_at(&WS, bytes, f);
}

/// The same frame of a terminal `ws` in size.
fn frame_at(
    ws: &WinSize,
    bytes: &[u8],
    f: impl FnOnce(&Renderer<'_, '_>, &support::Canvas, &Term<'_>),
) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = support::renderer(&faces);
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    feed(&mut t, bytes);
    r.fit(&t);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    f(&r, &canvas, &t);
}

/// The pixels of cell `(row, col)`'s band, in the canvas.
fn cell_pixels(r: &Renderer<'_, '_>, c: &support::Canvas, row: Row, col: Col) -> Vec<u32> {
    let m = r.metrics();
    let side = |p: baud_font::Px| usize::try_from(p.value()).unwrap();
    let (cw, ch) = (side(m.cell_w), side(m.cell_h));
    let (row, col) = (row.index(), col.index());
    let mut out = Vec::new();
    for y in row * ch..(row + 1) * ch {
        for x in col * cw..(col + 1) * cw {
            out.push(c.pixels[y * c.width + x] & 0x00FF_FFFF);
        }
    }
    out
}

/// The glyphs drawn on frame row `row`.
fn glyphs_on(r: &Renderer<'_, '_>, row: Row) -> Vec<Instance> {
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let row = i32::try_from(row.index()).unwrap();
    r.instances()
        .filter(|i| i.kind == Kind::Mask)
        .filter(|i| {
            let y = i32::from(i.y.0);
            y >= row * ch && y < (row + 1) * ch
        })
        .copied()
        .collect()
}

/// **The frame grows a row and the main display keeps its rows**: no line
/// at power-on, one row more with either type, and back with none.
#[test]
fn the_frame_grows_a_row_for_the_line_and_gives_it_back() {
    let ch = |r: &Renderer<'_, '_>| r.metrics().cell_h.value();
    frame(b"", |r, _, t| {
        assert_eq!(r.pixel_size().1.value(), 2 * ch(r));
        assert_eq!(t.geometry().1, Rows::of_count(2));
    });
    for open in [b"\x1b[1$~".as_slice(), b"\x1b[2$~"] {
        frame(open, |r, _, t| {
            assert_eq!(r.pixel_size().1.value(), 3 * ch(r));
            assert_eq!(r.grid(), (Cols::of_index(12), Rows::of_count(3)));
            assert_eq!(r.main_rows(), Rows::of_count(2));
            assert_eq!(t.geometry().1, Rows::of_count(2));
        });
    }
    frame(b"\x1b[1$~\x1b[0$~", |r, _, _| {
        assert_eq!(r.pixel_size().1.value(), 2 * ch(r));
    });
}

/// **The indicator in reverse video**: every cell of its row has the
/// theme's foreground for its background, where the main display's blank
/// cells have the background, and its text is drawn on its row. 13
/// columns, the width the cursor's field needs whole: at 12 it is left
/// out whole, and the row is drawn reversed with no glyph on it.
#[test]
fn the_indicator_is_drawn_in_reverse_video_below_the_main_display() {
    let theme = Theme::dark();
    frame(b"\x1b[1$~\x1b[?25l", |r, _, _| {
        for x in 0..12 {
            assert_eq!(rgb(r.background()[24 + x]), rgb(theme.fg), "line {x}");
        }
        assert_eq!(
            glyphs_on(r, Row::from_index(2)).len(),
            0,
            "1(01,001) needs 13"
        );
    });
    let ws = WinSize {
        ws_row: 2,
        ws_col: 13,
    };
    frame_at(&ws, b"\x1b[1$~\x1b[?25l", |r, c, _| {
        let cols = 13;
        for x in 0..cols {
            assert_eq!(rgb(r.background()[cols + x]), rgb(theme.bg), "main {x}");
            assert_eq!(rgb(r.background()[2 * cols + x]), rgb(theme.fg), "line {x}");
        }
        // "1(01,001)" from column 4: nine glyphs, all on the line's row.
        assert_eq!(glyphs_on(r, Row::from_index(2)).len(), 9);
        assert_eq!(
            glyphs_on(r, Row::ZERO).len() + glyphs_on(r, Row::from_index(1)).len(),
            0
        );
        // The pixels: a cell left of the text is the foreground whole, and
        // one under a digit holds ink.
        assert!(
            cell_pixels(r, c, Row::from_index(2), Col::at_index(0))
                .iter()
                .all(|&p| p == rgb(theme.fg))
        );
        assert!(
            cell_pixels(r, c, Row::from_index(2), Col::at_index(5))
                .iter()
                .any(|&p| p != rgb(theme.fg))
        );
        assert!(
            cell_pixels(r, c, Row::from_index(1), Col::at_index(5))
                .iter()
                .all(|&p| p == rgb(theme.bg))
        );
    });
}

/// **The host-writable line has the main display's background**, its
/// text on its own row, and the cursor drawn there while DECSASD selects
/// it: a block cursor fills the cell after the text with baud's amber,
/// and the main display's cursor cell is not filled.
#[test]
fn the_host_writable_line_draws_its_text_and_its_cursor() {
    let theme = Theme::dark();
    frame(b"\x1b[2$~\x1b[1$}AB", |r, c, _| {
        let cols = 12;
        assert_eq!(glyphs_on(r, Row::from_index(2)).len(), 2);
        assert_eq!(glyphs_on(r, Row::ZERO).len(), 0);
        assert_eq!(rgb(r.background()[2 * cols + 5]), rgb(theme.bg));
        assert_eq!(
            rgb(r.background()[2 * cols + 2]),
            rgb(baud_render::BaudCursor::dark().fill),
            "the cursor"
        );
        assert_eq!(
            rgb(r.background()[0]),
            rgb(theme.bg),
            "not on the main display"
        );
        assert!(
            cell_pixels(r, c, Row::from_index(2), Col::at_index(0))
                .iter()
                .any(|&p| p != rgb(theme.bg))
        );
    });
    // Back on the main display, the cursor is drawn there.
    frame(b"\x1b[2$~\x1b[1$}AB\x1b[0$}", |r, _, _| {
        let cols = 12;
        assert_eq!(rgb(r.background()[2 * cols + 2]), rgb(theme.bg));
        assert_eq!(
            rgb(r.background()[0]),
            rgb(baud_render::BaudCursor::dark().fill),
            "the main cursor"
        );
    });
}

/// **The status line's records are appended to the caller's** (FS-C13a),
/// which the cell path keeps across frames, so the line costs no buffer of
/// its own: a second call into the records of a first keeps them and adds
/// the same again. Named break: the caller's records cleared first.
#[test]
fn the_status_records_are_appended_to_the_callers() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = support::renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[2$~\x1b[1$}AB\x1b[0$}");
    r.fit(&t);
    let mut first = Vec::new();
    r.status_records(&t, None, &mut first);
    assert!(first.len() >= 2, "the line's two glyphs: {}", first.len());
    let mut out = first.clone();
    r.status_records(&t, None, &mut out);
    assert_eq!(out.len(), 2 * first.len(), "appended");
    assert_eq!(&out[..first.len()], &first[..], "the caller's kept");
    assert_eq!(&out[first.len()..], &first[..], "the line's again");
}

/// **The cell path draws the status line into the frame's records**
/// (FS-C13a): `Overlay::status` hands the renderer the records it was
/// given and makes no buffer of its own. A source check, since both
/// spellings draw the same. Named break: the records gathered into a Vec
/// of their own each frame and then appended.
#[test]
fn the_cell_path_draws_the_status_line_into_its_records() {
    let src = include_str!("../src/overlay.rs");
    let at = src.find("    pub fn status(").expect("Overlay::status");
    let end = at + src[at..].find("\n    }\n").expect("its body");
    let code = src[at..end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.contains("r.status_records(term, cursor, records);"),
        "{code}"
    );
    for made in ["Vec::new", "vec!", "Vec::with_capacity", ".to_vec()"] {
        assert!(
            !code.contains(made),
            "Overlay::status makes a buffer: {made}"
        );
    }
}

/// **A frame a row taller than its target paints what fits**: a window
/// that has not grown yet for the line, the frame the renderer draws for
/// the main display and the line, and the target the old size. The main
/// display's rows paint, the line's row falls past the target's bottom,
/// and nothing panics.
#[test]
fn a_target_a_row_short_takes_the_main_display_and_no_more() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = support::renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"main");
    r.fit(&t);
    let mut short = support::Canvas::for_renderer(&r);
    feed(&mut t, b"\x1b[1$~");
    r.fit(&t);
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    assert_eq!(short.height, 2 * ch, "the target is the main display's");
    support::paint(&mut r, &mut t, &mut short, Damage::Full);
    assert!(
        short.pixels.iter().any(|&p| p & 0x00FF_FFFF != 0),
        "the main display's rows are painted"
    );
}

/// A host-writable status line holding `AB`, and four lines on the main
/// display, two of them retained, handed to `f` on a terminal of its own.
fn scrolled_back<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25l\x1b[2$~\x1b[1$}AB\x1b[0$}one\r\ntwo\r\nthree\r\nfour",
    );
    assert_eq!(t.scrollback().len(), 2, "two lines retained");
    f(&mut t)
}

/// What one CPU frame drew on the status line's row, row 2, and on the
/// window's cell `(0, 5)`, the controls: the backgrounds, glyphs and
/// pixels it drew, and the grounds the renderer reports for them
/// (`Renderer::grounds`, the instance path's second background pass).
struct StatusDrawn {
    control: Argb,
    ground_control: Ground,
    backgrounds: Vec<u32>,
    glyphs: Vec<Instance>,
    pixels: Vec<u32>,
    grounds: Vec<Ground>,
}

/// One full CPU frame of [`scrolled_back`]'s window `back` lines up, on a
/// renderer of its own with `highlight` set.
fn status_drawn(faces: &[Face<'_>], back: Rows, highlight: Option<Highlight>) -> StatusDrawn {
    scrolled_back(|t| {
        let mut r = support::renderer(faces);
        r.fit(t);
        r.set_highlight(highlight.map(|h| h.span()));
        let mut c = support::Canvas::for_renderer(&r);
        {
            let mut target = c.target();
            let seal = t.seal_writes();
            r.render_at(t, seal, back, &mut target, Damage::Full);
        }
        let line = Row::from_index(2);
        let mut grounds = Vec::new();
        r.grounds(&mut grounds);
        StatusDrawn {
            control: r.background()[5],
            ground_control: grounds[5],
            backgrounds: r.background()[24..36].iter().map(|&a| rgb(a)).collect(),
            glyphs: glyphs_on(&r, line),
            pixels: (0..12)
                .flat_map(|x| cell_pixels(&r, &c, line, Col::at_index(x)))
                .collect(),
            grounds: grounds[24..36].to_vec(),
        }
    })
}

/// **A selection reaching below a scrolled-back window leaves the status
/// line unselected.** Two lines up, a selection from the window's first
/// cell to the live screen's last row ends past the window's bottom, so
/// the row under the window, the status line's, is inside its unclipped
/// ends (`Highlight::selects`), while its clipped corners, which the cell
/// shader draws the window's backgrounds by, stop at the window
/// (`Highlight::covers_row`). The status line is no line of the window,
/// and no selection reaches it: drawn with the selection, on the cell path
/// (`Renderer::status_records`) and on the CPU path (its row loop, which
/// never calls `status_records`), the line is what a renderer with no
/// selection draws, and the grounds the renderer reports for it after the
/// CPU frame (`Renderer::grounds`) are that renderer's too. The controls:
/// the selection is drawn on the window, and its ground reported there.
/// Named breaks: both gates dropped, `convert_row`'s and `ground`'s (every
/// arm red); `convert_row`'s dropped and the gate in `status_records`
/// alone (the CPU path's drawing red); `ground`'s alone dropped (the
/// line's grounds red).
#[test]
fn a_selection_reaching_below_the_window_leaves_the_status_line_unselected() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let back = Rows::of_count(2);
    let line = Row::from_index(2);
    // As the app takes it: the window's top, its rows and its columns.
    let h = scrolled_back(|t| {
        let scrolled = Scrolled::START.advanced(Rows::of_count(t.scrollback().len()));
        let view = t.view(back);
        let mut s = Selection::at(
            Span::cell(
                view.row(Row::ZERO),
                scrolled.line(back, Row::ZERO),
                Col::ZERO,
            ),
            Grain::Cell,
        );
        let live = t.viewport();
        s.extend(Span::cell(
            live.row(Row::from_index(1)),
            scrolled.line(Rows::ZERO, Row::from_index(1)),
            Col::at_index(3),
        ));
        let (cols, rows) = t.geometry();
        s.highlight(scrolled.top(back), rows, cols)
            .expect("on the window")
    });
    assert!(
        !h.covers_row(line),
        "the premise: the clipped corners stop at the window"
    );
    assert!(
        h.selects(line, Col::ZERO, Content::EMPTY),
        "the premise: the unclipped ends reach the line's row"
    );
    // The cell path: the line's records, as `Overlay::status` asks for them.
    let records = |highlight: Option<Highlight>| {
        scrolled_back(|t| {
            let mut r = support::renderer(&faces);
            r.fit(t);
            r.set_highlight(highlight.map(|h| h.span()));
            let mut out = Vec::new();
            r.status_records(t, None, &mut out);
            out
        })
    };
    let (cell, cell_fresh) = (records(Some(h)), records(None));
    assert!(
        cell_fresh.len() >= 2,
        "the line's two glyphs: {}",
        cell_fresh.len()
    );
    let (cpu, cpu_fresh) = (
        status_drawn(&faces, back, Some(h)),
        status_drawn(&faces, back, None),
    );
    assert_ne!(
        cpu.control, cpu_fresh.control,
        "the control: the selection is drawn on the window"
    );
    assert_ne!(
        cpu.ground_control, cpu_fresh.ground_control,
        "the control: the selection's ground is reported on the window"
    );
    let mut wrong = Vec::new();
    if cell != cell_fresh {
        wrong.push("cell path: the line's records");
    }
    if cpu.backgrounds != cpu_fresh.backgrounds {
        wrong.push("CPU path: the line's backgrounds");
    }
    if cpu.glyphs != cpu_fresh.glyphs {
        wrong.push("CPU path: the line's glyphs");
    }
    if cpu.pixels != cpu_fresh.pixels {
        wrong.push("CPU path: the line's pixels");
    }
    if cpu.grounds != cpu_fresh.grounds {
        wrong.push("grounds: the line's reported grounds");
    }
    assert!(
        wrong.is_empty(),
        "drawn selected, against a renderer with no selection: {wrong:?}"
    );
}
