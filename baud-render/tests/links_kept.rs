//! **A link's look changed with nothing written is drawn on the next
//! frame**, on the CPU path, which the Linux driver draws with. Each test
//! here is a staleness control: one input the lines at rest or the strikes
//! are drawn with is set again to another value, no cell and no run moved,
//! and the incremental frame after it draws, pixel for pixel and ground for
//! ground, what a renderer of its own draws in one full frame given the
//! same inputs; and the solids it keeps for the lines, the strikes and
//! `hover = "line"` are those that renderer places. The rows a line is on
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
//!
//! The last hold the solids placed on rows in another order and under a
//! held display, which are kept as those in their own order are: caret
//! frames there place no link record, by count, on either path; and a
//! write that lays a line's runs out otherwise, with nothing the driver
//! hands moved, places them again.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, PackedCell, ReadLen, Row, RowExtent, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build_with};
use baud_render::counters::{self, Counter, Site};
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
        word: t.styles().one().word_of(cell),
    }
}

/// Whether the link is struck, and so lent no slot: the selection covers
/// its row, and a selected cell keeps the selection's colours
/// (`Renderer::take_struck`), so the strike is all `missing_color` moves.
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
            r.set_highlight(
                Some(Highlight::new(
                    (row(), Col::at_index(0)),
                    (row(), Col::at_index(COLS - 1)),
                    Region::Stream,
                ))
                .map(|h| h.span()),
            );
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
/// solids it keeps for the links ([`kept_solids`]).
#[derive(Clone, PartialEq)]
struct Drawn {
    pixels: Vec<u32>,
    grounds: Vec<Ground>,
    solids: Vec<Instance>,
}

/// **The solids `r` keeps for the lines at rest, the strikes and
/// `hover = "line"`**, as a path that draws records of its own takes them
/// (`Renderer::link_lines_lowered`, with no slide): what its last frame
/// placed, or kept from an earlier one while their key held.
fn kept_solids(r: &Renderer<'_, '_>) -> Vec<Instance> {
    let mut out = Vec::new();
    r.link_lines_lowered(Px::of(0), &mut out);
    out
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
        let seal = t.seal_writes();
        r.render_at(t, seal, Rows::ZERO, &mut target, damage);
    }
    let mut grounds = Vec::new();
    r.grounds(&mut grounds);
    Drawn {
        pixels: px.clone(),
        grounds,
        solids: kept_solids(r),
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
    if before.solids.is_empty() || after.solids.is_empty() {
        return Some("the control: a full frame places solids".to_string());
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
    /// "do", the cells of "dog" on the paragraph's first row as the
    /// terminal cut it ([`cut_paragraph`]), which its display draws on the
    /// row below.
    Do,
    /// The cells of [`At::First`]'s columns on the row above it, which the
    /// window one line back shows on [`At::First`]'s window row
    /// ([`wide_above`]).
    Above,
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
            At::Do => (2, 38, 39),
            At::Above => (LINK_ROW - 1, LINK.0, LINK.1),
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
    /// A link's row selected, from [`Links::selected_from`] to its end.
    selected: Option<At>,
    /// Where [`Links::selected`] starts on its row.
    selected_from: SelectionStart,
}

/// Where a selected row's selection starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SelectionStart {
    /// Its first column: the whole row.
    Row,
    /// [`EDGE`], inside [`FIRST`]'s link.
    Edge,
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
    selected_from: SelectionStart::Row,
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
        word: t.styles().one().word_of(cell),
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
    r.set_highlight(
        l.selected
            .map(|a| {
                let row = line_at(t, h.back, a).row;
                let from = match l.selected_from {
                    SelectionStart::Row => 0,
                    SelectionStart::Edge => EDGE,
                };
                Highlight::new(
                    (row, Col::at_index(from)),
                    (row, Col::at_index(COLS - 1)),
                    Region::Stream,
                )
            })
            .map(|h| h.span()),
    );
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
        let seal = t.seal_writes();
        r.render_at(t, seal, h.back, &mut target, damage);
    }
    let mut grounds = Vec::new();
    r.grounds(&mut grounds);
    Drawn {
        pixels: px.clone(),
        grounds,
        solids: kept_solids(r),
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
                let n = t.styles().one().len();
                let top = (1..n)
                    .rev()
                    .find(|&s| t.styles().one().slots()[s].word().is_none())
                    .unwrap();
                for i in 0..n * 7 / 8 - 16 {
                    if t.styles().one().slots()[top].word().is_some() {
                        break;
                    }
                    let (red, green, blue) =
                        ((i * 37) % 256, (i * 91 + 7) % 256, (i * 53 + 11) % 251);
                    let cell = format!("\x1b[{ROWS};1H\x1b[38;2;{red};{green};{blue}mq\x1b[m");
                    feed(&mut t, cell.as_bytes());
                }
                if t.styles().one().slots()[top].word().is_none() {
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
    if before.solids.is_empty() || after.solids.is_empty() {
        return Some(format!(
            "{}: the control, a full frame places solids",
            case.what
        ));
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

/// **C6-11: a theme set with nothing written recolours the links**: the
/// lines at rest in their text's colour, which the theme resolves -- in
/// the box's colour, `[message]`'s own, no line would move. Named break:
/// the resolved colours left out of the key.
#[test]
fn a_theme_change_recolours_the_links() {
    let from = with(Links {
        colour: LineColour::Text,
        ..RESTING
    });
    let to = Host {
        theme: Palette::Light,
        ..from
    };
    let case = input("the theme", from, vec![Step::Host(to)]);
    assert_eq!(kept(&case), None);
}

/// **C6-12: reverse video set with nothing written recolours the links**:
/// `?5` is a mode bit, and moves no colour epoch; the lines at rest in
/// their text's colour, which the video resolves -- in the box's colour,
/// `[message]`'s own, no line would move. Named break: the resolved
/// colours left out of the key.
#[test]
fn reverse_video_with_nothing_written_recolours_the_links() {
    let text = with(Links {
        colour: LineColour::Text,
        ..RESTING
    });
    let case = input("?5", text, vec![Step::Bytes("\x1b[?5h".into())]);
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
/// stored now. Named breaks: a placement made with a row in another order
/// kept on its key alone, the layout's seal not asked; and one kept
/// wherever every row is in its own order now, whatever it was then.
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
/// placement made with every row in its own order kept whatever the rows
/// are now, the layout's seal not asked.
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
/// the link written, the link's own cells not. That write moves no struck
/// cell, so the struck cells kept with no row's generation asked draw it
/// alike; that break is held by
/// [`a_wide_write_at_the_selections_edge_finds_the_struck_cells_again`].
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
/// The driver strikes the link on the row it is looked back to, which
/// moves its inputs, so the struck cells kept with the window's rows out
/// of their key are found again all the same; that break is held by
/// [`a_look_back_onto_a_wide_cell_at_the_selections_edge_finds_the_struck_cells_again`].
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
/// own colour**: the strike alone, `[link] missing_color` left as it was
/// -- a change to it draws the next frame whole -- so the struck row is
/// drawn again from the copy the lending rewrote, put back as the
/// terminal's. Named break: the lending's rewrites not put back before
/// the next frame reads the copy.
#[test]
fn a_strike_taken_away_on_the_cpu_path_redraws_the_text() {
    lent_a_slot(&two_links(), with(STRUCK));
    let case = input(
        "a lent strike goes",
        with(STRUCK),
        vec![Step::Host(with(Links {
            strikes: &[],
            ..STRUCK
        }))],
    );
    assert_eq!(kept(&case), None);
}

/// The window's last row, where [`Step::TakeTheStrikesSlot`] writes.
const LAST_ROW: usize = ROWS - 1;

/// **C14-5: a rendition that takes the strike's slot, with nothing written
/// on the struck rows, repaints them**: the strike is lent another slot,
/// so the lending's number moves. The slot it held names another
/// rendition now under a cell of the copy, which draws the frame whole
/// (`Lent::moved_from`), so a lent row kept with the lending's number out
/// of the test is drawn alike: that number is a guard no input moves
/// alone (`Renderer::convert`'s `lent_alike`).
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

/// **C14-7: BDSM set with nothing written moves the struck text**, to the
/// cells the right-to-left paragraph draws the struck link on: a mode,
/// which stamps no row, so the struck cells are found again by the rows'
/// order alone, laid out otherwise. Named break: the layout's seal not
/// asked when the struck cells are kept.
#[test]
fn bdsm_toggled_with_nothing_written_moves_the_struck_text() {
    lent_a_slot(&hebrew_first("\x1b[8l"), as_cut(STRUCK));
    lent_a_slot(&hebrew_first("\x1b[8h"), as_cut(STRUCK));
    let case = Case {
        setup: hebrew_first("\x1b[8l"),
        moves: Some(first_reordered),
        ..input(
            "BDSM set over a strike",
            as_cut(STRUCK),
            vec![Step::Bytes("\x1b[8h".into())],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **C14-8: BDSM reset with nothing written moves the struck text back**:
/// the struck cells were found with their row reordered, and the row is
/// drawn as stored now. Named break: the layout's seal not asked when the
/// struck cells are kept.
#[test]
fn bdsm_reset_with_nothing_written_moves_the_struck_text_back() {
    lent_a_slot(&hebrew_first("\x1b[8h"), as_cut(STRUCK));
    lent_a_slot(&hebrew_first("\x1b[8l"), as_cut(STRUCK));
    let case = Case {
        setup: hebrew_first("\x1b[8h"),
        moves: Some(first_reordered),
        ..input(
            "BDSM reset over a strike",
            as_cut(STRUCK),
            vec![Step::Bytes("\x1b[8l".into())],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **C14-9: word wrap turned on with nothing written moves the struck
/// text to the row and cells its display draws the word on**, and the
/// struck cells found there are kept while the display holds: the frames
/// after the step are drawn under a held display, each a fresh renderer's.
/// Named break: the layout's seal not asked when the struck cells are kept.
#[test]
fn word_wrap_turned_on_moves_the_struck_text_to_its_display_row() {
    let links = Links {
        lines: &[THEN],
        strikes: &[THEN],
        missing: PINK,
        ..RESTING
    };
    lent_a_slot(&cut_paragraph(), as_cut(links));
    lent_a_slot(&cut_paragraph(), with(links));
    let case = Case {
        setup: cut_paragraph(),
        moves: Some(|r| r.bidi().display().is_some()),
        ..input(
            "wrap_words on over a strike",
            as_cut(links),
            vec![Step::Host(with(links))],
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
/// the cursor moved, and nothing else. The lending's rewrites not put
/// back before the next lend, which then reads lent ids as the cells'
/// own, draws this link alike -- its struck rendition is the one a lent
/// id read as its own resolves to -- so that break is held by
/// [`struck_caret_frames_on_a_page_of_their_own_draw_a_fresh_frame`].
#[test]
fn struck_caret_frames_draw_a_fresh_frame() {
    caret_frames_fresh(&caret_screen());
}

/// [`caret_screen`] with [`FIRST`]'s link on a page of its own, a dark
/// blue its strike keeps.
fn caret_screen_paged() -> String {
    format!(
        "{}\x1b[{};1H\x1b[2Ksee \x1b[38;2;255;0;0;48;2;0;0;48mhttps://example.org/x\x1b[m here",
        caret_screen(),
        FIRST.row() + 1
    )
}

/// **C14-6c: caret frames over a struck link on a page of its own draw a
/// fresh frame**, each, as [`struck_caret_frames_draw_a_fresh_frame`]
/// does: the link's cells on a dark blue page, which the strike keeps, so
/// a lending that read a lent id left in the copy as a cell's own -- a
/// slot the live table holds nothing in -- would draw them on another
/// page. Named break: the lending's rewrites not put back before the next
/// lend.
#[test]
fn struck_caret_frames_on_a_page_of_their_own_draw_a_fresh_frame() {
    caret_frames_fresh(&caret_screen_paged());
}

/// **Caret frames over [`FIRST`] struck on `setup` each draw what a fresh
/// renderer draws in one full frame**: a full frame, then the cursor moved
/// six times, nothing else. The controls: the struck link is lent a slot,
/// none short; each caret frame writes no row; a full frame places solids.
fn caret_frames_fresh(setup: &str) {
    lent_a_slot(setup, with(STRUCK));
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
        assert!(
            !full.solids.is_empty(),
            "the control: a full frame places solids"
        );
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
            styles_mem: core::array::from_fn(|_| vec![Resolved::default(); t.styles().one().len()]),
            overlay_mem: core::array::from_fn(|_| vec![0u8; COLS * (ROWS + 1)]),
            style_stamps: [StyleStamp::default(); SETS],
            lent: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
        }
    }
}

/// **What a cell frame draws a struck link with**: the lent view's cells,
/// read from the set's copy in screen order through its ring, which the walk and the overlay read each cell's id from, the slots
/// lent and left short, and the solids it keeps for the links
/// ([`kept_solids`]).
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
    let damage = r.damage(t, inputs.back);
    let f = build_with(
        r,
        t,
        seal,
        &damage,
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
            lent_used: &mut cell.lent_used,
        },
        &mut cell.used,
        || Some(at),
    )
    .expect("the set is free");
    let stride = t.stride().index();
    let copy = &cell.copies_mem[at.0];
    LentDrawn {
        cells: (0..ROWS)
            .flat_map(|d| {
                let s = f.rot.advance(d, f.ring).index() * stride;
                copy[s..s + stride].iter().copied()
            })
            .collect(),
        lent: cell.lent[at.0].lent(),
        short: cell.lent[at.0].short(),
        solids: kept_solids(r),
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
        assert!(
            !first.solids.is_empty(),
            "the control: a fresh cell frame places solids"
        );
        if now != first {
            wrong.push(k);
        }
    }
    assert!(wrong.is_empty(), "caret frames {wrong:?} lend otherwise");
}

/// [`At::Do`].
const DO: At = At::Do;

/// **C14-4b: a strike taken away under a held display redraws the row the
/// display drew the struck text on**: "do", the cells of "dog" the
/// terminal's cut left on the paragraph's first row, struck and drawn at
/// its words on the row below, then the strike taken away. A strike names
/// its stored row, so the row it was lent on is dirtied by no change to
/// the strikes: it is compared because the lending that rewrote it is
/// gone. The control: a renderer under the strike draws its cells on the
/// row below theirs alone. Named break: a lent row kept whatever the
/// lending.
#[test]
fn a_strike_taken_away_under_a_held_display_redraws_its_display_row() {
    let links = Links {
        lines: &[DO],
        strikes: &[DO],
        missing: PINK,
        ..RESTING
    };
    lent_a_slot(&cut_paragraph(), with(links));
    {
        let data = font("JetBrainsMono-Regular.ttf");
        let faces = [Face::parse(&data).unwrap()];
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, cut_paragraph().as_bytes());
        let mut r = made(&faces);
        set_host(&mut r, &with(links), &BASE);
        let mut px = Vec::new();
        frame(&mut r, &mut t, &with(links), &mut px, Damage::Full);
        let run = DO.run();
        let runs = r.bidi().stored_runs(run.row, run.from, run.to);
        assert!(
            r.bidi().display().is_some()
                && !runs.is_empty()
                && runs.iter().all(|&(y, _, _)| y.index() == DO.row() + 1),
            "the control: the struck cells drawn on the row below theirs alone, {runs:?}"
        );
    }
    let case = Case {
        setup: cut_paragraph(),
        ..input(
            "a strike under a held display goes",
            with(links),
            vec![Step::Host(with(Links {
                strikes: &[],
                ..links
            }))],
        )
    };
    assert_eq!(kept(&case), None);
}

/// The column a selection on [`FIRST`]'s row starts at in the cases on
/// the selection's edge: inside the link, whose cells before it are struck
/// and lent a slot, and whose cells from it on are selected and lent none.
const EDGE: usize = 10;

/// [`FIRST`] struck in [`PINK`], the only line, and its row selected from
/// [`EDGE`] on.
const STRUCK_TO_EDGE: Links = Links {
    lines: &[FIRST],
    selected: Some(FIRST),
    selected_from: SelectionStart::Edge,
    ..STRUCK
};

/// **A wide character in the link's red written over [`EDGE`]'s cell and
/// the one before it**, on `row`: [`EDGE`]'s cell becomes its tail, and a
/// wide tail takes its lead's answer to the selection, so the cell
/// selected under the narrow character it held is not selected under the
/// tail.
fn wide_at_edge(row: usize) -> String {
    format!("\x1b[{};{}H\x1b[38;2;255;0;0m\u{4E2D}\x1b[m", row + 1, EDGE)
}

/// Whether `d`, a cell frame of the window `back` lines up, lent the cell
/// at `row` and `col` a slot: its id in the lent view is not the one the
/// terminal holds there.
fn lent_at(d: &LentDrawn, t: &Term<'_>, back: Rows, row: usize, col: usize) -> bool {
    let own = t.view(back).row(Row::from_index(row))[col].style;
    d.cells
        .get(row * t.stride().index() + col)
        .is_some_and(|c| c.style != own)
}

/// **The first cell frame a renderer of its own builds** under `h`, into
/// set 0. The controls: it lends a slot and is short of none, and places
/// solids.
fn fresh_cell_frame(faces: &[Face<'_>], t: &mut Term<'_>, h: &Host) -> LentDrawn {
    let mut fresh = made(faces);
    set_host(&mut fresh, h, &BASE);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, t);
    let first = cell_frame(&mut fresh, t, h, &mut cells, SetIx(0));
    assert!(
        first.lent.value() > 0 && first.short.value() == 0,
        "the control: a fresh cell frame lends a slot, none short"
    );
    assert!(
        !first.solids.is_empty(),
        "the control: a fresh cell frame places solids"
    );
    first
}

/// **C14-2b: a wide character written at the selection's edge in a struck
/// row lends what a fresh renderer lends, on the cell path**: [`FIRST`]
/// struck and its row selected from [`EDGE`] on, then a wide character
/// written over [`EDGE`]'s cell and the one before it. The struck cells
/// read a row's content only through the selection, so [`EDGE`]'s cell,
/// selected before the write, is struck after it, and nothing moved but
/// the row's write. Each cell frame after it, into each set in turn,
/// lends what a fresh renderer's first cell frame lends. The controls: a
/// fresh renderer lends [`EDGE`]'s cell after the write and not before
/// it, and the write laid no row out again. Named break: the struck cells
/// kept with no row's generation asked.
#[test]
fn a_wide_write_at_the_selections_edge_finds_the_struck_cells_again() {
    let setup = two_links();
    let host = with(STRUCK_TO_EDGE);
    lent_a_slot(&setup, host);
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, setup.as_bytes());
    let mut r = made(&faces);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let mut sets = (0..).map(|k| SetIx(k % SETS));
    let mut next = || sets.next().expect("a set");
    for _ in 0..=SETS {
        cell_frame(&mut r, &mut t, &host, &mut cells, next());
    }
    let before = fresh_cell_frame(&faces, &mut t, &host);
    let lent_before = lent_at(&before, &t, Rows::ZERO, FIRST.row(), EDGE);
    let laid = r.bidi().laid_seal();
    feed(&mut t, wide_at_edge(FIRST.row()).as_bytes());
    let after = fresh_cell_frame(&faces, &mut t, &host);
    assert!(
        !lent_before && lent_at(&after, &t, Rows::ZERO, FIRST.row(), EDGE),
        "the control: a fresh renderer lends the edge's cell after the write and not before it"
    );
    let mut wrong = Vec::new();
    for k in 0..SETS {
        let now = cell_frame(&mut r, &mut t, &host, &mut cells, next());
        assert!(
            !r.bidi().moved_since(&laid),
            "the control: the write laid no row out again"
        );
        if now != after {
            wrong.push(k);
        }
    }
    assert!(
        wrong.is_empty(),
        "cell frames {wrong:?} after the write lend otherwise"
    );
}

/// [`At::Above`].
const ABOVE: At = At::Above;

/// **Two lines of history under [`two_links`], and [`ABOVE`]'s row the
/// link again with [`wide_at_edge`]'s character**: the window one line
/// back shows that row where [`FIRST`]'s was, the link in the same red at
/// the same columns.
fn wide_above() -> String {
    let mut setup = String::new();
    for i in 0..ROWS + 2 {
        setup.push_str(&format!("line {i}\r\n"));
    }
    setup.push_str(&two_links());
    setup.push_str(&format!(
        "\x1b[{};1H\x1b[2Ksee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m here{}",
        ABOVE.row() + 1,
        wide_at_edge(ABOVE.row())
    ));
    setup
}

/// **C14-3b: the window looked back onto a wide character at the
/// selection's edge, the driver's inputs unmoved, lends what a fresh
/// renderer lends, on the cell path**: [`FIRST`] struck and its row
/// selected from [`EDGE`] on; the window one line back shows
/// [`ABOVE`]'s row on [`FIRST`]'s window row, the link in the same red
/// at the same columns with a wide character over [`EDGE`]'s cell, and
/// the driver strikes, lines and selects it there -- the same strikes,
/// lines and selection by value, so only the window's rows moved. Each
/// cell frame after the look-back, into each set in turn, lends what a
/// fresh renderer's first cell frame lends. The controls: the strikes and
/// the selection the same by value, no window row written and none laid
/// out again, and a fresh renderer lends [`EDGE`]'s cell looked back and
/// not before. Named break: the struck cells kept with the window's rows
/// out of their key.
#[test]
fn a_look_back_onto_a_wide_cell_at_the_selections_edge_finds_the_struck_cells_again() {
    let setup = wide_above();
    let was = with(STRUCK_TO_EDGE);
    let now = Host {
        links: Links {
            lines: &[ABOVE],
            strikes: &[ABOVE],
            selected: Some(ABOVE),
            ..STRUCK_TO_EDGE
        },
        back: Rows::of_count(1),
        ..BASE
    };
    lent_a_slot(&setup, was);
    lent_a_slot(&setup, now);
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, setup.as_bytes());
    assert_eq!(
        line_at(&t, was.back, FIRST),
        line_at(&t, now.back, ABOVE),
        "the control: the driver's strike, line and selected row the same by value"
    );
    let mut r = made(&faces);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let mut sets = (0..).map(|k| SetIx(k % SETS));
    let mut next = || sets.next().expect("a set");
    for _ in 0..=SETS {
        cell_frame(&mut r, &mut t, &was, &mut cells, next());
    }
    let (laid, seal) = (r.bidi().laid_seal(), t.sealed());
    let before = fresh_cell_frame(&faces, &mut t, &was);
    let after = fresh_cell_frame(&faces, &mut t, &now);
    assert!(
        !lent_at(&before, &t, was.back, FIRST.row(), EDGE)
            && lent_at(&after, &t, now.back, FIRST.row(), EDGE),
        "the control: a fresh renderer lends the edge's cell looked back and not before"
    );
    let mut wrong = Vec::new();
    for k in 0..SETS {
        let drawn = cell_frame(&mut r, &mut t, &now, &mut cells, next());
        assert!(
            written(&t, now.back, &seal).is_empty() && !r.bidi().moved_since(&laid),
            "the control: no window row written and none laid out again"
        );
        if drawn != after {
            wrong.push(k);
        }
    }
    assert!(
        wrong.is_empty(),
        "cell frames {wrong:?} looked back lend otherwise"
    );
}

// **The solids on rows in another order and under a held display**
// (FS-C6's follow-up): kept while what they were placed under holds --
// the key, and no row's order nor the display laid out otherwise since
// the layout's seal then -- as the solids on rows in their own order are.

/// The link records placed since this thread's counts were last read:
/// the solids of the lines at rest, the strikes and `hover = "line"`.
fn placed() -> Tally {
    counters::take().at(Site::Links, Counter::LinkInstancesBuilt)
}

/// **[`cut_paragraph`]'s links for the caret frames under its display**:
/// "then" and "do" lines at rest, "do" struck in [`PINK`] -- drawn on the
/// row below its cells -- and "then" under the pointer, drawn as a line.
const UNDER_DISPLAY: Links = Links {
    lines: &[THEN, DO],
    strikes: &[DO],
    missing: PINK,
    hover: Some((THEN, HoverPaint::Line)),
    ..RESTING
};

/// **Both links at rest on [`hebrew_first`]'s right-to-left row and the
/// row below**, [`FIRST`] struck in [`PINK`] and under the pointer, drawn
/// as a line.
const ON_REORDERED: Links = Links {
    strikes: &[FIRST],
    missing: PINK,
    hover: Some((FIRST, HoverPaint::Line)),
    ..RESTING
};

/// Whether a display is held and [`DO`]'s cells are drawn on the row
/// below theirs alone.
fn do_drawn_below(r: &Renderer<'_, '_>) -> bool {
    let run = DO.run();
    let runs = r.bidi().stored_runs(run.row, run.from, run.to);
    r.bidi().display().is_some()
        && !runs.is_empty()
        && runs.iter().all(|&(y, _, _)| y.index() == DO.row() + 1)
}

/// Whether [`FIRST`]'s row is drawn in another order, no display held.
fn first_reordered_alone(r: &Renderer<'_, '_>) -> bool {
    first_reordered(r) && r.bidi().display().is_none()
}

/// Whether a display is held.
fn display_held(r: &Renderer<'_, '_>) -> bool {
    r.bidi().display().is_some()
}

/// **What caret frames on `setup` under `h` placed**, on the CPU path and
/// the cell path, by path and frame: a full frame and one more on the CPU
/// path, a frame into each set and one more on the cell path, then the
/// cursor moved six times on each, nothing else. The controls: `premise`
/// holds after the first frame; the first frame places link records; each
/// caret frame writes no window row and lays no row's order nor the
/// display out otherwise.
fn caret_frames_placed(setup: &str, h: &Host, premise: Premise) -> Vec<String> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut wrong = Vec::new();
    {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, setup.as_bytes());
        let mut r = made(&faces);
        set_host(&mut r, h, &BASE);
        let mut px = Vec::new();
        let _ = counters::take();
        frame(&mut r, &mut t, h, &mut px, Damage::Full);
        assert!(
            placed().value() > 0,
            "the control: the first CPU frame places link records"
        );
        assert!(premise(&r), "the control: the premise, on the CPU path");
        frame(&mut r, &mut t, h, &mut px, Damage::Incremental);
        for k in 0..6 {
            let (seal, laid) = (t.sealed(), r.bidi().laid_seal());
            feed(&mut t, format!("\x1b[{ROWS};{}H", k + 1).as_bytes());
            let _ = counters::take();
            frame(&mut r, &mut t, h, &mut px, Damage::Incremental);
            let n = placed();
            assert!(
                written(&t, h.back, &seal).is_empty() && !r.bidi().moved_since(&laid),
                "the isolation: a CPU caret frame writes no row and lays none out otherwise"
            );
            if n.value() > 0 {
                wrong.push(format!("cpu caret frame {k}: {} placed", n.value()));
            }
        }
    }
    {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, setup.as_bytes());
        let mut r = made(&faces);
        set_host(&mut r, h, &BASE);
        let mut backing = GlyphMap::backing();
        let mut cells = Cells::new(&mut backing, &t);
        let mut sets = (0..).map(|k| SetIx(k % SETS));
        let mut next = || sets.next().expect("a set");
        let _ = counters::take();
        cell_frame(&mut r, &mut t, h, &mut cells, next());
        assert!(
            placed().value() > 0,
            "the control: the first cell frame places link records"
        );
        assert!(premise(&r), "the control: the premise, on the cell path");
        for _ in 0..SETS {
            cell_frame(&mut r, &mut t, h, &mut cells, next());
        }
        for k in 0..6 {
            let (seal, laid) = (t.sealed(), r.bidi().laid_seal());
            feed(&mut t, format!("\x1b[{ROWS};{}H", k + 1).as_bytes());
            let _ = counters::take();
            cell_frame(&mut r, &mut t, h, &mut cells, next());
            let n = placed();
            assert!(
                written(&t, h.back, &seal).is_empty() && !r.bidi().moved_since(&laid),
                "the isolation: a cell caret frame writes no row and lays none out otherwise"
            );
            if n.value() > 0 {
                wrong.push(format!("cell caret frame {k}: {} placed", n.value()));
            }
        }
    }
    wrong
}

/// **FS-C6-H: caret frames under a held display place no link record**,
/// on the CPU path and the cell path: [`cut_paragraph`] drawn at its
/// words, [`UNDER_DISPLAY`]'s lines, strike and line under the pointer,
/// "do" drawn on the row below its cells. The solids are placed from the
/// display and the rows' maps, which a caret frame lays out no otherwise,
/// so they are kept. Named break: the solids kept only where every row a
/// solid is drawn on is in its own order, no display held.
#[test]
fn caret_frames_under_a_held_display_place_no_link_record() {
    let wrong = caret_frames_placed(&cut_paragraph(), &with(UNDER_DISPLAY), do_drawn_below);
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// **FS-C6-R: caret frames over a link on a row in another order place
/// no link record**, on the CPU path and the cell path: [`FIRST`]'s row
/// begun with Hebrew under BDSM, as the terminal cut it, its link a line
/// at rest, struck and under the pointer drawn as a line. Named break: the
/// solids kept only where every row a solid is drawn on is in its own
/// order, no display held.
#[test]
fn caret_frames_over_a_reordered_link_row_place_no_link_record() {
    let wrong = caret_frames_placed(
        &hebrew_first("\x1b[8h"),
        &as_cut(ON_REORDERED),
        first_reordered_alone,
    );
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// **The runs `at`'s cells are drawn in, on `setup` under `h`**, by a
/// renderer of its own after one full frame, and whether `premise` holds
/// there.
fn runs_of(setup: &str, h: &Host, at: At, premise: Premise) -> (Vec<(Row, Col, Cols)>, bool) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, setup.as_bytes());
    let mut r = made(&faces);
    set_host(&mut r, h, &BASE);
    let mut px = Vec::new();
    frame(&mut r, &mut t, h, &mut px, Damage::Full);
    let run = at.run();
    (r.bidi().stored_runs(run.row, run.from, run.to), premise(&r))
}

/// **The control of a write that moves a line**: `at`'s cells drawn in
/// other runs once `write` is fed after `setup` than before it, by a
/// renderer of its own under `h` each time, `premise` holding before it.
fn the_write_moves(setup: &str, write: &str, h: &Host, at: At, premise: Premise) {
    let before = runs_of(setup, h, at, premise);
    let after = runs_of(&format!("{setup}{write}"), h, at, premise);
    assert!(
        before.1 && before.0 != after.0,
        "the control: the write moves {at:?}'s runs, the premise holding before it: \
         {before:?} -> {after:?}"
    );
}

/// **FS-C6-S1: a write on another row of a paragraph a held display
/// draws moves its line**: "y" over the space between "lazy" and "do" on
/// [`cut_paragraph`]'s first row joins them into one word, which the
/// display draws whole on the row below, so "then", on the paragraph's
/// second row and not written, is drawn elsewhere. Nothing the driver
/// hands moved; the display was laid out otherwise. Named break: the
/// solids kept on their key alone, the layout's seal not asked.
#[test]
fn a_write_on_another_row_of_a_held_paragraph_moves_the_line() {
    let host = with(Links {
        lines: &[THEN],
        ..RESTING
    });
    let write = "\x1b[3;38Hy";
    the_write_moves(&cut_paragraph(), write, &host, THEN, display_held);
    // The paragraph's first row, above "then"'s.
    let case = Case {
        setup: cut_paragraph(),
        writes: &[2],
        ..input(
            "y written on the row above",
            host,
            vec![Step::Bytes(write.into())],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **FS-C6-S2: a write in a line's row under a held display moves the
/// line**: a space over "g", the first cell of "then"'s row, ends "do" on
/// the paragraph's first row, so "dog" is no longer drawn on "then"'s row
/// and "then", its own cells unwritten, is drawn elsewhere. Named
/// break: the solids kept on their key alone, the layout's seal not
/// asked.
#[test]
fn a_write_in_a_line_row_under_a_held_display_moves_the_line() {
    let host = with(Links {
        lines: &[THEN],
        ..RESTING
    });
    let write = "\x1b[4;1H ";
    the_write_moves(&cut_paragraph(), write, &host, THEN, display_held);
    // "then"'s own row.
    let case = Case {
        setup: cut_paragraph(),
        writes: &[3],
        ..input(
            "a space written on the line's row",
            host,
            vec![Step::Bytes(write.into())],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **FS-C6-S3: a write in a reordered line's row moves the line**:
/// Hebrew over " here", after [`FIRST`]'s link on [`hebrew_first`]'s
/// right-to-left row, ends the left-to-right run the link is drawn in at
/// the link, so the row, still in another order, draws the link
/// elsewhere; its cells are unwritten. Named break: the solids kept on
/// their key alone, the layout's seal not asked.
#[test]
fn a_write_in_a_reordered_line_row_moves_the_line() {
    let host = as_cut(Links {
        lines: &[FIRST],
        ..RESTING
    });
    let write = format!(
        "\x1b[{};{}H\u{5D3}\u{5D4}\u{5D5}\u{5D6}\u{5D7}",
        FIRST.row() + 1,
        LINK.1 + 2
    );
    let setup = hebrew_first("\x1b[8h");
    the_write_moves(&setup, &write, &host, FIRST, first_reordered_alone);
    let case = Case {
        setup,
        writes: &[LINK_ROW],
        ..input(
            "Hebrew written after the link on its row",
            host,
            vec![Step::Bytes(write)],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **FS-C6-S4: word wrap turned off with nothing written moves the line
/// back to its cells**: the placement was made under a held display, and
/// none is held now. Named break: the solids kept on their key alone, the
/// layout's seal not asked.
#[test]
fn word_wrap_turned_off_moves_the_line_back_to_its_cells() {
    let links = Links {
        lines: &[THEN],
        ..RESTING
    };
    let case = Case {
        setup: cut_paragraph(),
        moves: Some(display_held),
        ..input(
            "wrap_words off",
            with(links),
            vec![Step::Host(as_cut(links))],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **FS-C6-S5: BDSM reset with nothing written under a held display
/// moves the line back to its cells**: a paragraph is drawn at its words
/// only while BDSM is set, so the display is let go, and "then" is drawn
/// where its cells are stored: a mode, which stamps no row. Named break:
/// the solids kept on their key alone, the layout's seal not asked.
#[test]
fn bdsm_reset_under_a_held_display_moves_the_line_back_to_its_cells() {
    let case = Case {
        setup: cut_paragraph(),
        moves: Some(display_held),
        ..input(
            "BDSM reset under a display",
            with(Links {
                lines: &[THEN],
                ..RESTING
            }),
            vec![Step::Bytes("\x1b[8l".into())],
        )
    };
    assert_eq!(kept(&case), None);
}

/// **FS-C6-G: a write that lays out a row no link is on places no link
/// record**: three Hebrew letters over the last row's first word turn it
/// right to left, on [`two_links`] as the terminal cut it, and every row a
/// line is on is still drawn in its own order, where a run is its own
/// cells whatever the maps hold. The frame draws what a renderer of its
/// own draws. The controls: the write is on that row alone and lays its
/// order out otherwise -- the maps moved -- and the first frame places
/// link records. Named break: the solids on rows in their own order kept
/// only while no row of the window is in another.
#[test]
fn a_write_that_lays_out_a_row_no_link_is_on_places_no_link_record() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, two_links().as_bytes());
    let host = as_cut(RESTING);
    let mut r = made(&faces);
    set_host(&mut r, &host, &BASE);
    let mut px = Vec::new();
    let _ = counters::take();
    frame(&mut r, &mut t, &host, &mut px, Damage::Full);
    assert!(
        placed().value() > 0,
        "the control: the first frame places link records"
    );
    frame(&mut r, &mut t, &host, &mut px, Damage::Incremental);
    let (seal, laid) = (t.sealed(), r.bidi().laid_seal());
    feed(
        &mut t,
        format!("\x1b[{};1H\u{5D0}\u{5D1}\u{5D2}", LAST_ROW + 1).as_bytes(),
    );
    let _ = counters::take();
    let now = frame(&mut r, &mut t, &host, &mut px, Damage::Incremental);
    let n = placed();
    assert_eq!(
        written(&t, Rows::ZERO, &seal),
        [LAST_ROW],
        "the isolation: the last row alone written"
    );
    assert!(
        r.bidi().moved_since(&laid) && r.bidi().reordered(Row::from_index(LAST_ROW)),
        "the control: the write lays the last row out in another order"
    );
    let mut wrong = Vec::new();
    differs(
        &now,
        &fresh_under(&faces, &mut t, &host),
        "the frame after",
        &mut wrong,
    );
    assert!(wrong.is_empty(), "{wrong:?}");
    assert_eq!(
        n.value(),
        0,
        "the frame after the write placed link records"
    );
}
