//! **A link's look changed with nothing written is drawn on the next
//! frame**, on the CPU path, which the Linux driver draws with. Each test
//! here is a staleness control: one input the lines at rest or the strikes
//! are drawn with is set again to another value, no cell and no run moved,
//! and the incremental frame after it draws, pixel for pixel and ground for
//! ground, what a renderer of its own draws in one full frame given the
//! same inputs; and its solids are that renderer's. The rows a line is on
//! compare as unchanged, since nothing in them was written, so only the
//! setter can say they draw otherwise.
//!
//! The rest hold every other input the links' lines and strikes are drawn
//! from -- the driver's lists and hover, the colours the terminal and the
//! theme resolve, the metrics, each row's order, and a struck link's lent
//! slot -- the same way, each stating which window rows its steps wrote
//! against the seal the steady frame was drawn under; and caret frames
//! over a struck link, on the CPU path and the cell path, each what a
//! fresh renderer draws.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Col, Cols, PackedCell, ReadLen, Row, RowExtent, RowStyles, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Argb, Blend, Candidate, Damage, GlyphMap, Ground, Highlight, HoverPaint, Instance, Leaving,
    Lent, LineColour, LineStyle, LinkHover, LinkLine, LinkSpan, MapEntry, MessageColours,
    MessagePaint, MissingPaint, Overlay, PxSize, Region, Renderer, Resolved, Stack, StyleStamp,
    Tally, Target, Theme, WordWrap,
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

// The staleness controls of every other input the links are drawn from,
// each green on master: the driver's lists, the hover, the colours the
// terminal and the theme resolve, the metrics, and each row's order. Each
// case states, against the seal the steady frame was drawn under, which
// window rows its steps wrote, so the input it moves is the one a kept
// placement would have to see.

/// A link the screens here write, by where it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum At {
    /// [`screen`]'s red link.
    First,
    /// [`two_links`]'s second link, in palette colour 4, three rows below.
    Second,
    /// The link on the row a paragraph's first row runs onto
    /// ([`a_write_elsewhere_in_the_paragraph_moves_the_line`]).
    RunOn,
    /// "then", on the paragraph's second row as the terminal cut it
    /// ([`cut_paragraph`]).
    Then,
}

/// A link's cells as the screen holds them: its row before any look
/// back, and its first and last columns.
#[derive(Clone, Copy, Debug)]
struct Run {
    row: Row,
    from: Col,
    to: Col,
}

impl At {
    /// Its cells.
    fn run(self) -> Run {
        let (row, from, to) = match self {
            At::First => (LINK_ROW, LINK.0, LINK.1),
            At::Second => (SECOND_ROW, 4, 24),
            At::RunOn => (RUN_ON_ROW, 4, 24),
            At::Then => (3, 6, 9),
        };
        Run {
            row: Row::from_index(row),
            from: Col::at_index(from),
            to: Col::at_index(to),
        }
    }

    /// Its row before any look back, counted from the screen's top.
    fn row(self) -> usize {
        self.run().row.index()
    }
}

/// [`At::First`].
const FIRST: At = At::First;

/// [`At::Second`].
const SECOND: At = At::Second;

/// [`At::Second`]'s row.
const SECOND_ROW: usize = 5;

/// **[`screen`] with a second link**, blue from the palette, on
/// [`SECOND`]'s row.
fn two_links() -> String {
    let mut out = String::from_utf8(screen()).unwrap();
    out.push_str(&format!(
        "\x1b[{};1H\x1b[2Kand \x1b[34mhttps://example.org/y\x1b[m too",
        SECOND.row() + 1
    ));
    out
}

/// **What a driver hands the renderer for the links**, every frame.
#[derive(Clone, Copy, Debug)]
struct Links {
    style: LineStyle,
    colour: LineColour,
    message: MessageColours,
    missing: MissingPaint,
    lines: &'static [At],
    strikes: &'static [At],
    held: &'static [At],
    hover: Option<(At, HoverPaint)>,
    /// A link's row selected whole.
    selected: Option<At>,
}

/// Both links a line at rest, none struck, none under the pointer.
const RESTING: Links = Links {
    style: AT_REST.style,
    colour: AT_REST.colour,
    message: AT_REST.message,
    missing: AT_REST.missing,
    lines: &[FIRST, SECOND],
    strikes: &[],
    held: &[],
    hover: None,
    selected: None,
};

/// A `[link] missing_color` that reads 4.5:1 on the page, so a struck
/// cell no selection covers is lent a slot.
const PINK: MissingPaint = MissingPaint::Colour(Argb(0xFFFF_8080));

/// [`FIRST`] struck in [`PINK`], and lent a slot.
const STRUCK: Links = Links {
    missing: PINK,
    strikes: &[FIRST],
    ..RESTING
};

/// The theme a host draws in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Palette {
    Dark,
    Light,
}

/// **Everything a driver sets that the links are drawn with**: the links,
/// the theme, the size of the text, whether paragraphs are drawn at their
/// words, and how far the window looks back.
#[derive(Clone, Copy, Debug)]
struct Host {
    links: Links,
    theme: Palette,
    size: TextSize,
    wrap: WordWrap,
    back: Rows,
}

/// The size the text is drawn at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TextSize {
    /// [`made`]'s, 14 pixels an em.
    Base,
    /// 16 pixels an em.
    Larger,
}

impl TextSize {
    /// Its pixels an em.
    fn px(self) -> PxSize {
        PxSize::of(match self {
            TextSize::Base => 14.0,
            TextSize::Larger => 16.0,
        })
        .unwrap()
    }
}

/// A renderer as [`made`] makes it, the links at rest.
const BASE: Host = Host {
    links: RESTING,
    theme: Palette::Dark,
    size: TextSize::Base,
    wrap: WordWrap::On,
    back: Rows::ZERO,
};

/// [`BASE`] with `links`.
fn with(links: Links) -> Host {
    Host { links, ..BASE }
}

/// [`BASE`] with the terminal's cut drawn as it is, so no display is held
/// and each row's order is its own paragraph's alone.
fn as_cut(links: Links) -> Host {
    Host {
        links,
        wrap: WordWrap::Off,
        ..BASE
    }
}

/// `at` as the window `back` lines into the history shows it.
fn line_at(t: &Term<'_>, back: Rows, at: At) -> LinkLine {
    let run = at.run();
    let row = Row::from_index(run.row.index() + back.index());
    let cell = t.view(back).row(row)[run.from.index()];
    LinkLine {
        row,
        from: run.from,
        to: run.to,
        word: t.style_in(RowStyles::Live, cell),
    }
}

/// `h`'s links set on `r`, as a driver sets them each frame.
fn set_links(r: &mut Renderer<'_, '_>, t: &Term<'_>, h: &Host) {
    let l = &h.links;
    let of = |ats: &[At]| -> Vec<LinkLine> { ats.iter().map(|&a| line_at(t, h.back, a)).collect() };
    r.set_link_lines(&of(l.lines), l.style);
    r.set_line_colour(l.colour);
    r.set_message_colours(l.message);
    r.set_link_missing(l.missing);
    r.set_link_strikes(&of(l.strikes), &of(l.held));
    let hover = l.hover.map(|(a, paint)| {
        let line = line_at(t, h.back, a);
        LinkHover {
            spans: vec![LinkSpan {
                row: line.row,
                from: line.from,
                to: line.to,
            }],
            paint,
        }
    });
    r.set_link_hover(hover.as_ref());
    r.set_highlight(l.selected.map(|a| {
        let row = line_at(t, h.back, a).row;
        Highlight::new(
            (row, Col::at_index(0)),
            (row, Col::at_index(COLS - 1)),
            Region::Stream,
        )
    }));
}

/// The theme `p` names.
fn theme(p: Palette) -> Theme {
    match p {
        Palette::Dark => Theme::dark(),
        Palette::Light => Theme::light(),
    }
}

/// `now`'s theme, size and wrap set on `r` where they differ from `was`'s,
/// as a driver sets them on a change.
fn set_host(r: &mut Renderer<'_, '_>, now: &Host, was: &Host) {
    if now.theme != was.theme {
        r.set_theme(theme(now.theme));
    }
    if now.size != was.size {
        r.rescale(now.size.px()).unwrap();
    }
    if now.wrap != was.wrap {
        r.set_word_wrap(now.wrap);
    }
}

/// One CPU frame of the window `back` lines into `t`'s history onto `px`,
/// the links as `h` hands them, as `damage` says.
fn frame(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    h: &Host,
    px: &mut Vec<u32>,
    damage: Damage,
) -> Drawn {
    r.fit(t);
    set_links(r, t, h);
    let (w, ht) = r.pixel_size();
    let n = usize::try_from(w.value() * ht.value()).unwrap();
    if px.len() != n {
        *px = vec![0; n];
    }
    {
        let mut target = Target::new(&mut px[..], w, w, ht).unwrap();
        r.render_at(t, h.back, &mut target, damage);
    }
    let mut grounds = Vec::new();
    r.grounds(&mut grounds);
    Drawn {
        pixels: px.clone(),
        grounds,
        solids: r.link_solids().to_vec(),
    }
}

/// **What a renderer of its own draws** under `h`, in one full frame.
fn fresh_under(faces: &[Face<'_>], t: &mut Term<'_>, h: &Host) -> Drawn {
    let mut r = made(faces);
    set_host(&mut r, h, &BASE);
    let mut px = Vec::new();
    frame(&mut r, t, h, &mut px, Damage::Full)
}

/// One step of a case.
enum Step {
    /// Bytes fed to the terminal.
    Bytes(String),
    /// The driver's inputs set to these.
    Host(Host),
    /// Renditions written into the last row's first cell, each over the
    /// one before, until the slot the strike was lent -- the top one the
    /// live table holds nothing in -- holds one: the strike is lent
    /// another, with nothing written on the struck rows.
    TakeTheStrikesSlot,
}

/// A premise a case's steps move, read on the renderer after a frame.
type Premise = fn(&Renderer<'_, '_>) -> bool;

/// **One input the links are drawn from**: a screen, the driver's inputs,
/// the steps -- an incremental frame between each two -- the window rows
/// they write, and the premise they move, if any.
struct Case {
    what: &'static str,
    setup: String,
    host: Host,
    steps: Vec<Step>,
    writes: &'static [usize],
    moves: Option<Premise>,
}

/// [`Case`] on [`two_links`], writing nothing.
fn input(what: &'static str, host: Host, steps: Vec<Step>) -> Case {
    Case {
        what,
        setup: two_links(),
        host,
        steps,
        writes: &[],
        moves: None,
    }
}

/// The window rows written after `seal`, the window `back` lines back.
fn written(t: &Term<'_>, back: Rows, seal: &baud_domain::Seal) -> Vec<usize> {
    let g = t.row_gens(back);
    (0..ROWS)
        .filter(|&y| g.after(Row::from_index(y), seal))
        .collect()
}

/// **One case on the CPU path**: a full frame and two incremental ones,
/// the steady frame a fresh renderer's; the steps; the rows they wrote and
/// the premise they moved; a fresh renderer drawing otherwise than before
/// them, the control; then two incremental frames, each a fresh
/// renderer's full frame. What it got wrong, if anything.
fn kept(case: &Case) -> Option<String> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, case.setup.as_bytes());
    let mut host = case.host;
    let mut r = made(&faces);
    set_host(&mut r, &host, &BASE);
    let mut px = Vec::new();
    frame(&mut r, &mut t, &host, &mut px, Damage::Full);
    let mut steady = None;
    for _ in 0..2 {
        steady = Some(frame(&mut r, &mut t, &host, &mut px, Damage::Incremental));
    }
    let before = fresh_under(&faces, &mut t, &host);
    let mut wrong = Vec::new();
    if let Some(steady) = &steady {
        differs(steady, &before, "the steady frame", &mut wrong);
    }
    let premise = case.moves.map(|m| m(&r));
    let (seal, back) = (t.sealed(), host.back);
    for (k, step) in case.steps.iter().enumerate() {
        if k > 0 {
            frame(&mut r, &mut t, &host, &mut px, Damage::Incremental);
        }
        match step {
            Step::Bytes(b) => feed(&mut t, b.as_bytes()),
            Step::Host(h) => {
                set_host(&mut r, h, &host);
                host = *h;
            }
            Step::TakeTheStrikesSlot => {
                let n = t.styles().len();
                let top = (1..n).rev().find(|&s| t.styles()[s].is_none()).unwrap();
                for i in 0..n * 7 / 8 - 16 {
                    if t.styles()[top].is_some() {
                        break;
                    }
                    let (red, green, blue) =
                        ((i * 37) % 256, (i * 91 + 7) % 256, (i * 53 + 11) % 251);
                    let cell = format!("\x1b[{ROWS};1H\x1b[38;2;{red};{green};{blue}mq\x1b[m");
                    feed(&mut t, cell.as_bytes());
                }
                if t.styles()[top].is_none() {
                    return Some(format!("{}: the control, the slot is taken", case.what));
                }
            }
        }
    }
    let rows = written(&t, back, &seal);
    if rows != case.writes {
        return Some(format!(
            "{}: the isolation, window rows {rows:?} written, not {:?}",
            case.what, case.writes
        ));
    }
    let after = fresh_under(&faces, &mut t, &host);
    if after.pixels == before.pixels {
        return Some(format!("{}: the control, the change shows", case.what));
    }
    for frame_is in ["the frame after", "the frame after that"] {
        let now = frame(&mut r, &mut t, &host, &mut px, Damage::Incremental);
        differs(&now, &after, frame_is, &mut wrong);
    }
    if let (Some(m), Some(was)) = (case.moves, premise) {
        if m(&r) == was {
            return Some(format!(
                "{}: the control, the premise did not move",
                case.what
            ));
        }
    }
    (!wrong.is_empty()).then(|| format!("{}: {}", case.what, wrong.join(", ")))
}

/// Every case's failure, if any.
fn each(cases: &[Case]) -> Vec<String> {
    cases.iter().filter_map(kept).collect()
}

/// **The control that `h`'s struck link is lent a slot** on the screen
/// `setup` leaves: a cell frame under `h` lends one and is short of none.
/// The cell path lends the same struck cells the CPU path does, and a
/// strike's solid moves with `missing_color` whether or not a slot is
/// lent, so a frame's pixels cannot say it.
fn lent_a_slot(setup: &str, h: Host) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, setup.as_bytes());
    let mut r = made(&faces);
    set_host(&mut r, &h, &BASE);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let first = cell_frame(&mut r, &mut t, &h, &mut cells, SetIx(0));
    assert!(
        first.lent.value() > 0 && first.short.value() == 0,
        "the control: the struck link is lent a slot, none short"
    );
}

/// **C6-1: a strike that arrives with nothing written is drawn**, as a
/// Paths answer strikes a link. Named break: the strikes copied with the
/// key's inputs unmoved.
#[test]
fn a_strike_with_nothing_written_is_drawn() {
    let to = with(Links {
        strikes: &[FIRST],
        ..RESTING
    });
    let case = input("a strike arrives", BASE, vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// **C6-2: a strike taken away with nothing written goes.** Named break:
/// the strikes copied with the key's inputs unmoved.
#[test]
fn a_strike_taken_away_with_nothing_written_goes() {
    let from = with(Links {
        strikes: &[FIRST],
        ..RESTING
    });
    let case = input("a strike goes", from, vec![Step::Host(BASE)]);
    assert_eq!(kept(&case), None);
}

/// **C6-3: a line gone with nothing written goes**: the scan's list
/// shrinks, as a forgotten origin takes a link's line away. Named break:
/// the lines copied with the key's inputs unmoved.
#[test]
fn a_line_gone_with_nothing_written_goes() {
    let to = with(Links {
        lines: &[FIRST],
        ..RESTING
    });
    let case = input("a line goes", BASE, vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// **C6-4: a strike the chord holds, with nothing written, takes the
/// text's colour**: the link under the pointer, under `hover = "line"`,
/// struck by the gesture's modifier. Named break: the held strikes copied
/// with the key's inputs unmoved.
#[test]
fn a_held_strike_with_nothing_written_takes_the_texts_colour() {
    let from = with(Links {
        hover: Some((FIRST, HoverPaint::Line)),
        ..RESTING
    });
    let to = with(Links {
        held: &[FIRST],
        ..from.links
    });
    let case = input("a strike held", from, vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// **C6-5: the hover moved to the other link, with nothing written, moves
/// its colour**: the lines at rest in their text's colour, so the box's
/// colour the hover lays over its link's line shows where it is -- under
/// the box's colour at rest the two lines would look alike. Named break:
/// the hover copied with the key's inputs unmoved.
#[test]
fn the_hover_moved_with_nothing_written_moves_its_colour() {
    let text = Links {
        colour: LineColour::Text,
        ..RESTING
    };
    let from = with(Links {
        hover: Some((FIRST, HoverPaint::Line)),
        ..text
    });
    let to = with(Links {
        hover: Some((SECOND, HoverPaint::Line)),
        ..text
    });
    let case = input("the hover moves", from, vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// **C6-6: the hover's paint from the box to a line draws its line.**
/// Named break: the hover copied with the key's inputs unmoved.
#[test]
fn the_hover_paint_changed_draws_its_line() {
    let from = with(Links {
        hover: Some((FIRST, HoverPaint::Box)),
        ..RESTING
    });
    let to = with(Links {
        hover: Some((FIRST, HoverPaint::Line)),
        ..RESTING
    });
    let case = input("the hover's paint", from, vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// **C6-10: a palette entry or the default ground set with nothing
/// written recolours the links**: `OSC 4` moves the second link's colour
/// 4, which its line takes under `LineColour::Text`, and `OSC 11` the
/// page. Named break: the resolved colours left out of the key.
#[test]
fn a_palette_change_with_nothing_written_recolours_the_links() {
    let text = with(Links {
        colour: LineColour::Text,
        ..RESTING
    });
    let wrong = each(&[
        input(
            "OSC 4",
            text,
            vec![Step::Bytes("\x1b]4;4;rgb:00/ff/00\x1b\\".into())],
        ),
        input(
            "OSC 11",
            BASE,
            vec![Step::Bytes("\x1b]11;rgb:30/30/60\x1b\\".into())],
        ),
    ]);
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// **C6-11: a theme set with nothing written recolours the links.** Named
/// break: the resolved colours left out of the key.
#[test]
fn a_theme_change_recolours_the_links() {
    let to = Host {
        theme: Palette::Light,
        ..BASE
    };
    let case = input("the theme", BASE, vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// **C6-12: reverse video set with nothing written recolours the links**:
/// `?5` is a mode bit, and moves no colour epoch. Named break: the
/// resolved colours left out of the key.
#[test]
fn reverse_video_with_nothing_written_recolours_the_links() {
    let case = input("?5", BASE, vec![Step::Bytes("\x1b[?5h".into())]);
    assert_eq!(kept(&case), None);
}

/// **C6-13: the selection's colours set with nothing written recolour the
/// lines at rest**, which are drawn in the box's colour, the message
/// pair's, here `[message]`'s "none" -- the selection's own: `OSC 17` and
/// `OSC 19` move no colour epoch. Named break: the selection's colours
/// left out of the key.
#[test]
fn a_selection_colour_set_with_nothing_written_recolours_the_lines_at_rest() {
    let selection = with(Links {
        message: MessageColours {
            page: MessagePaint::Selection,
            text: MessagePaint::Selection,
        },
        ..RESTING
    });
    let case = input(
        "OSC 17 and 19",
        selection,
        vec![Step::Bytes(
            "\x1b]17;rgb:00/30/60\x1b\\\x1b]19;rgb:ff/ff/ff\x1b\\".into(),
        )],
    );
    assert_eq!(kept(&case), None);
}

/// **C6-15: a rescale places the links again**, at the new cells. Named
/// break: the metrics left out of the key.
#[test]
fn a_rescale_places_the_links_again() {
    let to = Host {
        size: TextSize::Larger,
        ..BASE
    };
    let case = input("a rescale", BASE, vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// [`FIRST`]'s row begun with three Hebrew letters, so its paragraph runs
/// right to left and the link is drawn elsewhere than it is stored while
/// the bidirectional order is drawn.
fn hebrew_first(bdsm: &str) -> String {
    format!(
        "{bdsm}{}\x1b[{};1H\u{5D0}\u{5D1}\u{5D2}",
        two_links(),
        FIRST.row() + 1
    )
}

/// Whether [`FIRST`]'s row is drawn in another order than stored.
fn first_reordered(r: &Renderer<'_, '_>) -> bool {
    r.bidi().reordered(Row::from_index(FIRST.row()))
}

/// **C6-16: BDSM set with nothing written moves the line**, to the cells
/// the right-to-left paragraph draws the link on: a mode, which stamps no
/// row. Named break: no test of the rows' order now when a placement is
/// kept.
#[test]
fn bdsm_toggled_with_nothing_written_moves_the_line() {
    let case = Case {
        setup: hebrew_first("\x1b[8l"),
        moves: Some(first_reordered),
        ..input(
            "BDSM set",
            as_cut(RESTING),
            vec![Step::Bytes("\x1b[8h".into())],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **C6-20: BDSM reset with nothing written moves the line back**: the
/// placement was made with its row reordered, and the row is drawn as
/// stored now. Named break: a placement kept that was made with a row in
/// another order.
#[test]
fn bdsm_reset_with_nothing_written_moves_the_line_back() {
    let case = Case {
        setup: hebrew_first("\x1b[8h"),
        moves: Some(first_reordered),
        ..input(
            "BDSM reset",
            as_cut(RESTING),
            vec![Step::Bytes("\x1b[8l".into())],
        )
    };
    assert_eq!(kept(&case), None);
}

/// The row a link is on that a paragraph's first row runs onto.
const RUN_ON: At = At::RunOn;

/// [`RUN_ON`]'s row.
const RUN_ON_ROW: usize = 7;

/// The paragraph's first row, which runs onto [`RUN_ON`]'s.
const ABOVE_RUN_ON: usize = RUN_ON_ROW - 1;

/// **C6-17: a strong right-to-left letter written on the paragraph's
/// other row moves the line**: the link's row runs on from the row above,
/// and is not written, so its paragraph turns right to left by a store
/// to another row. Named break: no test of the rows' order now when a
/// placement is kept.
#[test]
fn a_write_elsewhere_in_the_paragraph_moves_the_line() {
    let setup = format!(
        "{}\x1b[{};1H\x1b[2K\x1b[{};1H\x1b[2K{}see \x1b[38;2;255;0;0mhttps://example.org/z\x1b[m end",
        two_links(),
        RUN_ON.row() + 1,
        RUN_ON.row(),
        "a".repeat(COLS),
    );
    let host = as_cut(Links {
        lines: &[RUN_ON],
        ..RESTING
    });
    let case = Case {
        setup,
        writes: &[ABOVE_RUN_ON],
        moves: Some(|r| r.bidi().reordered(Row::from_index(RUN_ON.row()))),
        ..input(
            "a write on the row above",
            host,
            vec![Step::Bytes(format!("\x1b[{};1H\u{5D0}", RUN_ON.row()))],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **C6-21: a write in a link's row that reorders it moves the line**:
/// three Hebrew letters over the row's first word, the link's cells
/// unwritten -- a caret frame's write, on the link's own row. Named
/// breaks: a placement kept whatever was written, and no test of the
/// rows' order now.
#[test]
fn a_write_in_a_link_row_that_reorders_it_moves_the_line() {
    let case = Case {
        writes: &[LINK_ROW],
        moves: Some(first_reordered),
        ..input(
            "Hebrew written on the link's row",
            as_cut(RESTING),
            vec![Step::Bytes(format!(
                "\x1b[{};1H\u{5D0}\u{5D1}\u{5D2}",
                FIRST.row() + 1
            ))],
        )
    };
    assert_eq!(kept(&case), None);
}

/// A paragraph the terminal's cut splits inside a word, "do|g" at the
/// fortieth column, on rows 2 and 3, finished by the prompt on the last
/// row: drawn at its words, "dog" moves whole to the second row.
fn cut_paragraph() -> String {
    "\x1b[?25l\x1b[3;1Ha quick brown fox jumps over the lazy dog and then runs away\
     \x1b[10;1H\x1b]133;A\x07$ "
        .to_string()
}

/// [`At::Then`].
const THEN: At = At::Then;

/// **C6-18: word wrap turned on with nothing written moves the line to
/// the row and cells its display draws the word on.** Named break: a
/// placement kept with a display held.
#[test]
fn word_wrap_turned_on_moves_the_line_to_its_display_row() {
    let links = Links {
        lines: &[THEN],
        ..RESTING
    };
    let case = Case {
        setup: cut_paragraph(),
        moves: Some(|r| r.bidi().display().is_some()),
        ..input(
            "wrap_words on",
            as_cut(links),
            vec![Step::Host(with(links))],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **C14-1: a selection over a struck link, nothing written, keeps the
/// selection's colours**: the cells it covers are lent nothing. Named
/// break: the struck cells kept with the selection out of their key.
#[test]
fn a_selection_over_a_struck_link_keeps_the_selections_colours() {
    lent_a_slot(&two_links(), with(STRUCK));
    let to = with(Links {
        selected: Some(FIRST),
        ..STRUCK
    });
    let case = input("a selection", with(STRUCK), vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// **C14-2: a write in a struck row finds its cells again**: a cell past
/// the link written, the link's own cells not. Named break: the struck
/// cells kept with no row's generation asked.
#[test]
fn a_write_in_a_struck_row_finds_its_cells_again() {
    lent_a_slot(&two_links(), with(STRUCK));
    let case = Case {
        writes: &[LINK_ROW],
        ..input(
            "a write in the struck row",
            with(STRUCK),
            vec![Step::Bytes(format!("\x1b[{};27HX", FIRST.row() + 1))],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **C14-3: the window looked back, nothing written, finds the struck
/// cells again**, a row lower: two lines of history under the screen.
/// Named break: the struck cells kept with the window's rows out of their
/// key.
#[test]
fn a_look_back_finds_the_struck_cells_again() {
    let mut setup = String::new();
    for i in 0..ROWS + 2 {
        setup.push_str(&format!("line {i}\r\n"));
    }
    setup.push_str(&two_links());
    let to = Host {
        back: Rows::of_count(1),
        ..with(STRUCK)
    };
    lent_a_slot(&setup, with(STRUCK));
    lent_a_slot(&setup, to);
    let case = Case {
        setup,
        ..input("a look back", with(STRUCK), vec![Step::Host(to)])
    };
    assert_eq!(kept(&case), None);
}

/// **C14-4: a strike taken away on the CPU path redraws the text in its
/// own colour**: the rows the lending rewrote are compared again. Named
/// break: a lent row kept whatever the lending.
#[test]
fn a_strike_taken_away_on_the_cpu_path_redraws_the_text() {
    lent_a_slot(&two_links(), with(STRUCK));
    let case = input("a lent strike goes", with(STRUCK), vec![Step::Host(BASE)]);
    assert_eq!(kept(&case), None);
}

/// The window's last row, where [`Step::TakeTheStrikesSlot`] writes.
const LAST_ROW: usize = ROWS - 1;

/// **C14-5: a rendition that takes the strike's slot, with nothing written
/// on the struck rows, repaints them**: the strike is lent another slot,
/// so the lending's number moves. Named break: a lent row kept with the
/// lending's number out of the test.
#[test]
fn a_slot_taken_from_the_strike_repaints_the_struck_rows() {
    lent_a_slot(&two_links(), with(STRUCK));
    let case = Case {
        writes: &[LAST_ROW],
        ..input(
            "the strike's slot taken",
            with(STRUCK),
            vec![Step::TakeTheStrikesSlot],
        )
    };
    assert_eq!(kept(&case), None);
}

/// [`two_links`] with the cursor shown, a steady block, so no phase of a
/// blink can tell one frame from another.
fn caret_screen() -> String {
    format!("{}\x1b[2 q\x1b[?25h", two_links())
}

/// **C14-6: caret frames over a struck link draw a fresh frame**, each:
/// the cursor moved, and nothing else. Named break: the lending's
/// rewrites not put back before the next lend, which then reads lent ids
/// as the cells' own.
#[test]
fn struck_caret_frames_draw_a_fresh_frame() {
    let setup = caret_screen();
    lent_a_slot(&setup, with(STRUCK));
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, setup.as_bytes());
    let host = with(STRUCK);
    let mut r = made(&faces);
    let mut px = Vec::new();
    frame(&mut r, &mut t, &host, &mut px, Damage::Full);
    let mut wrong = Vec::new();
    for k in 0..6 {
        let seal = t.sealed();
        feed(&mut t, format!("\x1b[{ROWS};{}H", k + 1).as_bytes());
        let now = frame(&mut r, &mut t, &host, &mut px, Damage::Incremental);
        assert!(
            written(&t, Rows::ZERO, &seal).is_empty(),
            "the isolation: a caret frame"
        );
        let full = fresh_under(&faces, &mut t, &host);
        differs(&now, &full, &format!("caret frame {k}"), &mut wrong);
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// The cell path's buffers, one set a frame in flight as the app keeps
/// them.
struct Cells<'m> {
    map: GlyphMap<'m>,
    exceptions: Overlay,
    screens: Screens,
    copies_mem: [Vec<PackedCell>; SETS],
    styles_mem: [Vec<Resolved>; SETS],
    overlay_mem: [Vec<u8>; SETS],
    style_stamps: [StyleStamp; SETS],
    lent: [Lent; SETS],
    candidates: Vec<Candidate>,
    records: Vec<Instance>,
    grounds: Vec<Instance>,
    bidi_cells: Vec<PackedCell>,
    bidi_used: Vec<RowExtent>,
    lent_cells: Vec<PackedCell>,
    lent_used: Vec<RowExtent>,
    used: Vec<RowExtent>,
}

impl<'m> Cells<'m> {
    fn new(backing: &'m mut [MapEntry], t: &Term<'_>) -> Cells<'m> {
        let stride = t.stride().index();
        Cells {
            map: GlyphMap::over(backing),
            exceptions: Overlay::new(),
            screens: Screens::new(),
            copies_mem: core::array::from_fn(|_| vec![PackedCell::EMPTY; (ROWS + 2) * stride]),
            styles_mem: core::array::from_fn(|_| vec![Resolved::default(); t.styles().len()]),
            overlay_mem: core::array::from_fn(|_| vec![0u8; COLS * (ROWS + 1)]),
            style_stamps: [StyleStamp::default(); SETS],
            lent: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            bidi_cells: Vec::new(),
            bidi_used: Vec::new(),
            lent_cells: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
        }
    }
}

/// **What a cell frame draws a struck link with**: the lent view's cells,
/// which the walk and the overlay read each cell's id from, the slots
/// lent and left short, and the links' solids.
#[derive(PartialEq, Debug)]
struct LentDrawn {
    cells: Vec<PackedCell>,
    lent: Tally,
    short: Tally,
    solids: Vec<Instance>,
}

/// **The cell frame `r` builds into set `at`**, as the app's `present`
/// builds it with no composition, prediction or slide, the links as `h`
/// hands them, the frame's writes sealed first (`Term::seal_writes`).
fn cell_frame(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    h: &Host,
    cell: &mut Cells<'_>,
    at: SetIx,
) -> LentDrawn {
    let seal = t.seal_writes();
    let t = &*t;
    let back = h.back;
    r.fit(t);
    set_links(r, t, h);
    r.set_images(t, back);
    r.splash_yields_to(t, Px::of(0));
    r.place_marks(t, back, Leaving::None, Px::of(0));
    r.place_lights(t);
    let live = t
        .cursor_visible()
        .then(|| t.cursor())
        .map(|(row, col)| (row, col, t.cursor_style()));
    let inputs = CellInputs {
        back,
        predictions: core::iter::empty(),
        live,
        composition: &None,
        highlight: None,
        leaving: Leaving::None,
        pan: Px::of(0),
    };
    let mut copies = cell.copies_mem.each_mut().map(|c| &mut c[..]);
    let mut styles = cell.styles_mem.each_mut().map(|c| &mut c[..]);
    let mut overlay = cell.overlay_mem.each_mut().map(|c| &mut c[..]);
    build(
        r,
        t,
        seal,
        &inputs,
        &mut CellParts {
            map: &mut cell.map,
            exceptions: &mut cell.exceptions,
            screens: &mut cell.screens,
            copies_mem: &mut copies,
            styles_mem: &mut styles,
            overlay_mem: &mut overlay,
            style_stamps: &mut cell.style_stamps,
            lent: &mut cell.lent,
            candidates: &mut cell.candidates,
            records: &mut cell.records,
            grounds: &mut cell.grounds,
            bidi_cells: &mut cell.bidi_cells,
            bidi_used: &mut cell.bidi_used,
            lent_cells: &mut cell.lent_cells,
            lent_used: &mut cell.lent_used,
        },
        &mut cell.used,
        || Some(at),
    )
    .expect("the set is free");
    let n = ROWS * t.stride().index();
    LentDrawn {
        cells: cell.lent_cells.get(..n).unwrap_or_default().to_vec(),
        lent: cell.lent[at.0].lent(),
        short: cell.lent[at.0].short(),
        solids: r.link_solids().to_vec(),
    }
}

/// **C14-6, on the cell path: caret frames over a struck link lend what a
/// fresh renderer's first cell frame lends**, each into the next set in
/// turn: the same cells in the lent view, a slot lent and none short, and
/// the same solids. Named break: the lending's rewrites not put back
/// before the next lend.
#[test]
fn struck_caret_frames_lend_a_fresh_cell_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, caret_screen().as_bytes());
    let host = with(STRUCK);
    let mut r = made(&faces);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let mut sets = (0..).map(|k| SetIx(k % SETS));
    let mut next = || sets.next().expect("a set");
    for _ in 0..=SETS {
        cell_frame(&mut r, &mut t, &host, &mut cells, next());
    }
    let mut wrong = Vec::new();
    for k in 0..2 * SETS {
        feed(&mut t, format!("\x1b[{ROWS};{}H", k + 1).as_bytes());
        let now = cell_frame(&mut r, &mut t, &host, &mut cells, next());
        let mut fresh = made(&faces);
        let mut fresh_backing = GlyphMap::backing();
        let mut fresh_cells = Cells::new(&mut fresh_backing, &t);
        let first = cell_frame(&mut fresh, &mut t, &host, &mut fresh_cells, SetIx(0));
        assert!(
            first.lent.value() > 0 && first.short.value() == 0,
            "the control: a slot lent, none short"
        );
        if now != first {
            wrong.push(k);
        }
    }
    assert!(wrong.is_empty(), "caret frames {wrong:?} lend otherwise");
}
