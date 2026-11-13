//! **A link's look changed with nothing written is drawn on the next
//! frame**, on the CPU path, which the Linux driver draws with. Each test
//! here is a staleness control: one input the lines at rest or the strikes
//! are drawn with is set again to another value, no cell and no run moved,
//! and the incremental frame after it draws, pixel for pixel and ground for
//! ground, what a renderer of its own draws in one full frame given the
//! same inputs; and its solids are that renderer's. The rows a line is on
//! compare as unchanged, since nothing in them was written, so only the
//! setter can say they draw otherwise.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, RowStyles, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Blend, Damage, Ground, Highlight, Instance, LineColour, LineStyle, LinkLine,
    MessageColours, MessagePaint, MissingPaint, PxSize, Region, Renderer, Stack, Target, Theme,
};
use support::font;

const COLS: usize = 40;
const ROWS: usize = 10;
const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

/// The link's row.
const LINK_ROW: usize = 2;

/// The link's cells on its row: `https://example.org/x`, in red.
const LINK: (usize, usize) = (4, 24);

/// The link's row, as a [`Row`].
fn row() -> Row {
    Row::from_index(LINK_ROW)
}

/// `bytes` fed to the terminal, in reads no longer than the app's.
fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for read in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
}

/// **The screen**: a row of text on every row, the link's row a URL in red
/// between plain words, the cursor hidden.
fn screen() -> Vec<u8> {
    let mut out = b"\x1b[?25l".to_vec();
    for i in 0..ROWS {
        out.extend(format!("\x1b[{};1Hrow {i} abcdefghijklmnopqrstuvwxyz", i + 1).as_bytes());
    }
    out.extend(
        format!(
            "\x1b[{};1H\x1b[2Ksee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m here",
            LINK_ROW + 1
        )
        .as_bytes(),
    );
    out
}

/// The link's run, in its cells' rendition.
fn link(t: &Term<'_>) -> LinkLine {
    let cell = t.view(Rows::ZERO).row(row())[LINK.0];
    LinkLine {
        row: row(),
        from: Col::at_index(LINK.0),
        to: Col::at_index(LINK.1),
        word: t.style_in(RowStyles::Live, cell),
    }
}

/// Whether the link is struck, and so lent no slot: the selection covers
/// its row, and a selected cell keeps the selection's colours
/// (`Renderer::struck_cells`), so the strike is all `missing_color` moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Strike {
    No,
    Selected,
}

/// **What the frames' links are drawn with**, as a driver sets them.
#[derive(Clone, Copy, Debug)]
struct Look {
    style: LineStyle,
    colour: LineColour,
    message: MessageColours,
    missing: MissingPaint,
    strike: Strike,
}

/// The defaults, the link a line at rest and not struck.
const AT_REST: Look = Look {
    style: LineStyle::Solid,
    colour: LineColour::Box,
    message: MessageColours {
        page: MessagePaint::Baud,
        text: MessagePaint::Baud,
    },
    missing: MissingPaint::Gray,
    strike: Strike::No,
};

/// Every input of `look` set on `r`, as a driver sets them each frame.
fn set(r: &mut Renderer<'_, '_>, t: &Term<'_>, look: &Look) {
    let line = [link(t)];
    r.set_link_lines(&line, look.style);
    r.set_line_colour(look.colour);
    r.set_message_colours(look.message);
    r.set_link_missing(look.missing);
    match look.strike {
        Strike::No => {
            r.set_link_strikes(&[], &[]);
            r.set_highlight(None);
        }
        Strike::Selected => {
            r.set_link_strikes(&line, &[]);
            r.set_highlight(Some(Highlight::new(
                (row(), Col::at_index(0)),
                (row(), Col::at_index(COLS - 1)),
                Region::Stream,
            )));
        }
    }
}

/// A renderer over `faces` for the grid here.
fn made<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(COLS),
        Rows::of_index(ROWS),
    )
    .unwrap()
}

/// **What a CPU frame drew**: its pixels, each cell's ground, and the
/// links' solids it placed.
#[derive(Clone, PartialEq)]
struct Drawn {
    pixels: Vec<u32>,
    grounds: Vec<Ground>,
    solids: Vec<Instance>,
}

/// One CPU frame of `t`'s screen onto `px`, as `damage` says.
fn draw(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut Vec<u32>, damage: Damage) -> Drawn {
    r.fit(t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    if px.len() != n {
        *px = vec![0; n];
    }
    {
        let mut target = Target::new(&mut px[..], w, w, h).unwrap();
        r.render_at(t, Rows::ZERO, &mut target, damage);
    }
    let mut grounds = Vec::new();
    r.grounds(&mut grounds);
    Drawn {
        pixels: px.clone(),
        grounds,
        solids: r.link_solids().to_vec(),
    }
}

/// **What a renderer of its own draws** under `look`, in one full frame:
/// it has no copy of the window to keep.
fn fresh(faces: &[Face<'_>], t: &mut Term<'_>, look: &Look) -> Drawn {
    let mut r = made(faces);
    r.fit(t);
    set(&mut r, t, look);
    let mut px = Vec::new();
    draw(&mut r, t, &mut px, Damage::Full)
}

/// What `now` gets wrong against `full`, by name.
fn differs(now: &Drawn, full: &Drawn, frame: &str, wrong: &mut Vec<String>) {
    if now.pixels != full.pixels {
        wrong.push(format!("{frame}: pixels"));
    }
    if now.grounds != full.grounds {
        wrong.push(format!("{frame}: grounds"));
    }
    if now.solids != full.solids {
        wrong.push(format!("{frame}: the solids"));
    }
}

/// **`from` drawn to a steady frame, then `to` set with nothing written**:
/// the steady frame is a fresh renderer's under `from`; a fresh renderer
/// draws `to` otherwise -- the control, that the change shows -- and the
/// two incremental frames after it are each a fresh renderer's full frame
/// under `to`. What it got wrong, if anything.
fn changed(from: Look, to: Look) -> Option<String> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &screen());
    let mut r = made(&faces);
    r.fit(&t);
    set(&mut r, &t, &from);
    let mut px = Vec::new();
    draw(&mut r, &mut t, &mut px, Damage::Full);
    let mut steady = None;
    for _ in 0..2 {
        steady = Some(draw(&mut r, &mut t, &mut px, Damage::Incremental));
    }
    let before = fresh(&faces, &mut t, &from);
    let mut wrong = Vec::new();
    if let Some(steady) = &steady {
        differs(steady, &before, "the steady frame", &mut wrong);
    }
    let after = fresh(&faces, &mut t, &to);
    if after.pixels == before.pixels {
        return Some("the control: the change shows".to_string());
    }
    set(&mut r, &t, &to);
    for frame in ["the frame after", "the frame after that"] {
        let now = draw(&mut r, &mut t, &mut px, Damage::Incremental);
        differs(&now, &after, frame, &mut wrong);
    }
    (!wrong.is_empty()).then(|| wrong.join(", "))
}

/// **`[link] line` from solid to dotted, nothing written, redraws the
/// lines**. Named break: the style set with no repaint.
#[test]
fn a_line_style_changed_redraws_the_lines() {
    let to = Look {
        style: LineStyle::Dotted,
        ..AT_REST
    };
    assert_eq!(changed(AT_REST, to), None);
}

/// **The lines' colour from the box's to the text's, nothing written,
/// recolours them**: amber to the link's red. Named break: the colour set
/// with no repaint.
#[test]
fn a_line_colour_changed_recolours_the_lines() {
    let to = Look {
        colour: LineColour::Text,
        ..AT_REST
    };
    assert_eq!(changed(AT_REST, to), None);
}

/// **`[link] missing_color` changed, nothing written, recolours the
/// strike**: the struck link under the selection, which lends its cells
/// no slot, so the strike is the one thing the colour moves; both colours
/// read 4.5:1 on the page. Named break: the colour set with no repaint.
#[test]
fn a_missing_colour_changed_recolours_the_strikes() {
    let from = Look {
        missing: MissingPaint::Colour(Argb(0xFFFF_8080)),
        strike: Strike::Selected,
        ..AT_REST
    };
    let to = Look {
        missing: MissingPaint::Colour(Argb(0xFF80_FF80)),
        ..from
    };
    assert_eq!(changed(from, to), None);
}

/// **`[message]`'s colours changed, nothing written, recolour the lines at
/// rest**, which are drawn in the box's colour: amber to white, under
/// black text, which reads. Named break: the colours set with no repaint.
#[test]
fn message_colours_changed_recolour_the_lines_at_rest() {
    let to = Look {
        message: MessageColours {
            page: MessagePaint::Colour(Argb(0xFFFF_FFFF)),
            text: MessagePaint::Colour(Argb(0xFF00_0000)),
        },
        ..AT_REST
    };
    assert_eq!(changed(AT_REST, to), None);
}
