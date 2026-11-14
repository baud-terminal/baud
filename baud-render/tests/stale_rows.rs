//! **A row a frame keeps is the row the terminal holds** (FS-C5): a frame
//! takes from the terminal again only the rows written since the seal its
//! copy was taken under, and those drawn from more than their stored
//! cells, so every other kind of change has to reach the copy some other
//! way -- through the row's stamp, the window, the scalars, the line
//! renditions, the rows veiled or lent last frame, or the rows drawn in
//! another order. Each test here is a staleness control: an incremental
//! frame after the change draws, pixel for pixel and ground for ground,
//! what a renderer of its own draws in one full frame, which has no copy
//! to keep; and on the cell path the set the frame is taken into is a new
//! screen and holds the window as it stands.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Codepoint, Col, Cols, PackedCell, ReadLen, Row, RowStyles, Rows, WinSize,
};
use baud_font::Face;
use baud_render::screens::{Built, SETS, Screens, SetIx, Taking};
use baud_render::{
    Blend, Caret, Composition, Damage, Field, FindLook, Ground, Hints, HoverPaint, Leaving,
    LineStyle, LinkHover, LinkLine, LinkLook, LinkSpan, Measure, Orders, PxSize, Renderer, Stack,
    Target, Theme, TipArea, TipLook,
};
use support::font;

const COLS: usize = 40;
const ROWS: usize = 10;
const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

/// The cursor hidden, so no frame here draws one.
const HIDE: &[u8] = b"\x1b[?25l";

/// `bytes` fed to the terminal, in reads no longer than the app's.
fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for read in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
}

fn text(s: &str) -> Vec<Codepoint> {
    s.chars().map(Codepoint::from_char).collect()
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

/// **What a CPU frame drew**: its pixels, and each cell's ground as the
/// convert drew it (`Renderer::grounds`), which reads the last frame's
/// copy of the window.
#[derive(Clone, PartialEq)]
struct Drawn {
    pixels: Vec<u32>,
    grounds: Vec<Ground>,
}

/// One CPU frame of `t`'s window `back` lines into its history, onto
/// `px`, as `damage` says. A canvas of another size starts blank.
fn draw(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    back: Rows,
    px: &mut Vec<u32>,
    damage: Damage,
) -> Drawn {
    r.fit(t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    if px.len() != n {
        *px = vec![0; n];
    }
    {
        let mut target = Target::new(&mut px[..], w, w, h).unwrap();
        r.render_at(t, back, &mut target, damage);
    }
    let mut grounds = Vec::new();
    r.grounds(&mut grounds);
    Drawn {
        pixels: px.clone(),
        grounds,
    }
}

/// **What a renderer of its own draws**, in one full frame: it has no
/// copy of the window to keep.
fn fresh(faces: &[Face<'_>], t: &mut Term<'_>, back: Rows) -> Drawn {
    let mut r = made(faces);
    let mut px = Vec::new();
    draw(&mut r, t, back, &mut px, Damage::Full)
}

/// What `now` gets wrong against `full`, by name.
fn differs(now: &Drawn, full: &Drawn, frame: &str, wrong: &mut Vec<String>) {
    if now.pixels != full.pixels {
        wrong.push(format!("{frame}: pixels"));
    }
    if now.grounds != full.grounds {
        wrong.push(format!("{frame}: grounds"));
    }
}

/// Each row of the screen, text the width of most of it.
fn rows_of_text() -> Vec<u8> {
    (0..ROWS)
        .flat_map(|i| format!("\x1b[{};1Hrow {i} abcdefghijklmnopqrstuvwxyz", i + 1).into_bytes())
        .collect()
}

/// The screen of text, the cursor hidden.
fn base() -> Vec<u8> {
    [HIDE, &rows_of_text()].concat()
}

/// `cells` cells, each in a background colour of its own from the
/// `first`th on, the blue channel `blue`.
fn coloured(first: usize, cells: usize, blue: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for k in 0..cells {
        let i = first + k;
        out.extend(format!("\x1b[0;48;2;{};{};{blue}mq", i % 41, (i / 41) % 41).as_bytes());
    }
    out.extend(b"\x1b[0m");
    out
}

/// What a case does to the terminal.
enum Op {
    Bytes(Vec<u8>),
    Resize(WinSize),
}

impl Op {
    fn apply(&self, t: &mut Term<'_>) {
        match self {
            Op::Bytes(b) => feed(t, b),
            Op::Resize(ws) => {
                let _ = t.resize(ws);
            }
        }
    }
}

/// How a case's control knows the operation did something: the picture
/// changed, or the core froze renditions -- which draws the same picture
/// through other ids, so only a row taken again draws it right.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shows {
    Drawn,
    Froze,
}

/// **One family of stores** (r3 2.2): a screen set up, then one operation,
/// on the window `back` lines into the history.
struct Case {
    what: &'static str,
    setup: Vec<u8>,
    op: Op,
    back: Rows,
    shows: Shows,
}

/// The families, one case each.
fn families() -> Vec<Case> {
    let on = |what: &'static str, extra: &[u8], op: &[u8]| Case {
        what,
        setup: [&base()[..], extra].concat(),
        op: Op::Bytes(op.to_vec()),
        back: Rows::ZERO,
        shows: Shows::Drawn,
    };
    // **Freeze**: thirteen lines of history, each cell a rendition of its
    // own, under a screen of plain text, and the window over the last ten
    // of them; then the screen written in place, three times over, each
    // cell a rendition of its own, which fills the table past what a sweep
    // frees and freezes the history, and takes the slots it freed.
    let mut frozen = HIDE.to_vec();
    for line in 0..13 {
        frozen.extend(coloured(line * COLS, COLS, 20));
        frozen.extend(b"\r\n");
    }
    frozen.extend(b"plain 0");
    for k in 1..ROWS {
        frozen.extend(format!("\r\nplain {k}").as_bytes());
    }
    let mut freezing = Vec::new();
    for pass in 0..3 {
        freezing.extend(b"\x1b[H");
        freezing.extend(coloured(pass * COLS * ROWS, COLS * ROWS, 21));
    }
    vec![
        on("text", b"", b"\x1b[3;5HXYZ"),
        // The cursor left after the `e`, which the mark joins.
        on(
            "a mark attached to the cell before",
            b"\x1b[4;1Hcafe",
            "\u{301}".as_bytes(),
        ),
        on("EL", b"", b"\x1b[3;5H\x1b[K"),
        on("ED", b"", b"\x1b[5;5H\x1b[J"),
        on("ECH", b"", b"\x1b[3;3H\x1b[4X"),
        on("DCH", b"", b"\x1b[3;3H\x1b[2P"),
        on("ICH", b"", b"\x1b[3;3H\x1b[2@"),
        on("IL", b"", b"\x1b[3;1H\x1b[2L"),
        on("DL", b"", b"\x1b[3;1H\x1b[2M"),
        on("SU", b"", b"\x1b[2S"),
        on("SD", b"", b"\x1b[2T"),
        on("RI in a region", b"", b"\x1b[3;7r\x1b[3;1H\x1bM\x1b[r"),
        on(
            "a DECSLRM band scrolled",
            b"",
            b"\x1b[?69h\x1b[5;20s\x1b[3;7r\x1b[7;5H\n\x1b[r\x1b[?69l",
        ),
        on("DECALN", b"", b"\x1b#8"),
        on("DECDWL", b"", b"\x1b[5;1H\x1b#6"),
        on("DECCRA", b"", b"\x1b[1;1;2;12;1;6;3;1$v"),
        on("DECCARA", b"", b"\x1b[2;2;4;12;7$r"),
        on("REP", b"", b"\x1b[3;3HQ\x1b[5b"),
        on("IRM", b"", b"\x1b[4h\x1b[3;3HINS\x1b[4l"),
        on("?1049 set", b"", b"\x1b[?1049h\x1b[2;2Halt"),
        on("?1049 reset", b"\x1b[?1049h\x1b[2;2Halt", b"\x1b[?1049l"),
        on("?47 set", b"", b"\x1b[?47h\x1b[2;2Halt"),
        on("?47 reset", b"\x1b[?47h\x1b[2;2Halt", b"\x1b[?47l"),
        on(
            "the status line written",
            b"\x1b[2$~\x1b[1$}old status\x1b[0$}",
            b"\x1b[1$}new status\x1b[0$}",
        ),
        // The indicator line shows the cursor's place, composed at a read's
        // end.
        on("the indicator line", b"\x1b[1$~", b"\x1b[5;9H"),
        on("RIS", b"", b"\x1bc"),
        Case {
            what: "resize",
            setup: base(),
            op: Op::Resize(WinSize {
                ws_col: 30,
                ws_row: 8,
            }),
            back: Rows::ZERO,
            shows: Shows::Drawn,
        },
        Case {
            what: "freeze",
            setup: frozen,
            op: Op::Bytes(freezing),
            back: Rows::of_index(ROWS),
            shows: Shows::Froze,
        },
    ]
}

/// **One case on the CPU path**: a full frame and two incremental ones,
/// the operation, then two incremental frames, each the frame a fresh
/// renderer draws. What it got wrong, if anything.
fn on_the_cpu_path(faces: &[Face<'_>], case: &Case) -> Option<String> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &case.setup);
    let back = case.back;
    let mut r = made(faces);
    let mut px = Vec::new();
    draw(&mut r, &mut t, back, &mut px, Damage::Full);
    for _ in 0..2 {
        draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
    }
    let before = fresh(faces, &mut t, back);
    let rewrites = t.style_reclaims().rewrites;
    case.op.apply(&mut t);
    let shown = match case.shows {
        Shows::Drawn => fresh(faces, &mut t, back).pixels != before.pixels,
        Shows::Froze => t.style_reclaims().rewrites > rewrites,
    };
    if !shown {
        return Some(format!(
            "{}: the control, the operation shows ({:?})",
            case.what, case.shows
        ));
    }
    let mut wrong = Vec::new();
    for frame in ["the frame after", "the frame after that"] {
        let now = draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
        let full = fresh(faces, &mut t, back);
        differs(&now, &full, frame, &mut wrong);
    }
    (!wrong.is_empty()).then(|| format!("{} on the CPU path: {}", case.what, wrong.join(", ")))
}

/// **A frame taken into `set` as the cell path's build takes it**: one
/// generation sealed, then the window `back` lines into the history and
/// the status line, drawn through `orders`, maps no layout writes: no row
/// in another order and no display held.
fn take(
    s: &mut Screens,
    t: &mut Term<'_>,
    (back, orders): (Rows, &Orders),
    sets: &mut [Vec<PackedCell>; SETS],
    set: SetIx,
) -> Built {
    let seal = t.seal_writes();
    let t = &*t;
    let mut copies = sets.each_mut().map(|c| &mut c[..]);
    let view = t.view(back);
    let status = t.status_line();
    s.take(
        &mut copies,
        set,
        &Taking {
            term: t,
            seal,
            back,
            view: &view,
            status: status.as_ref(),
            orders,
            stride: t.stride(),
            leaving: Leaving::None,
        },
    )
}

/// **One case on the cell path**: a frame into each set in turn until
/// every set was built from, and a steady one, the last screen -- the
/// control -- then the operation and a frame into the next set, which is
/// a new screen whose copy holds the window and the status line as they
/// stand. What it got wrong, if anything.
fn on_the_cell_path(case: &Case) -> Option<String> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &case.setup);
    let back = case.back;
    let len = t.stride().index() * (ROWS + 1);
    let mut sets: [Vec<PackedCell>; SETS] = core::array::from_fn(|_| vec![PackedCell::EMPTY; len]);
    let mut s = Screens::new();
    let orders = Orders::default();
    let mut order = (0..).map(|k| SetIx(k % SETS));
    let mut next = || order.next().expect("a set");
    for _ in 0..=SETS {
        take(&mut s, &mut t, (back, &orders), &mut sets, next());
    }
    if take(&mut s, &mut t, (back, &orders), &mut sets, next()) != Built::Last {
        return Some(format!(
            "{}: the control, a steady frame is the last screen",
            case.what
        ));
    }
    case.op.apply(&mut t);
    let set = next();
    let built = take(&mut s, &mut t, (back, &orders), &mut sets, set);
    let mut wrong = Vec::new();
    if built != Built::New {
        wrong.push("built from the last screen".to_string());
    }
    let view = t.view(back);
    let (cols, rows, stride) = (view.cols().index(), view.rows().index(), t.stride().index());
    let copy = &sets[set.0];
    for y in 0..rows {
        if copy[y * stride..y * stride + cols] != view.row(Row::from_index(y))[..cols] {
            wrong.push(format!("row {y} stale"));
        }
    }
    if let Some(line) = t.status_line() {
        let n = cols.min(line.cells.len());
        if copy[rows * stride..rows * stride + n] != line.cells[..n] {
            wrong.push("the status line stale".to_string());
        }
    }
    (!wrong.is_empty()).then(|| format!("{} on the cell path: {}", case.what, wrong.join(", ")))
}

/// **R2: a row any family of stores wrote is drawn from the terminal**, on
/// the CPU path pixel for pixel and ground for ground as a fresh renderer
/// draws it, and on the cell path as a new screen whose copy holds it --
/// one case a family: text, a mark attached, EL, ED, ECH, DCH, ICH, IL,
/// DL, SU, SD, RI in a region, a DECSLRM band, DECALN, DECDWL, DECCRA,
/// DECCARA, REP, IRM, the alternate screen set and reset by `?1049` and
/// `?47`, the status line written, the indicator line, RIS, a resize and a
/// freeze. Every case is run and every wrong one named. Named breaks: the
/// keys reading no row as written since their seal (the stamped
/// families), and the keys reading no other window, scalar or line
/// rendition (`?47`, DECDWL, the resize).
#[test]
fn a_row_any_family_wrote_is_drawn_from_the_terminal() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let cases = families();
    assert_eq!(cases.len(), 28, "every family has its case");
    let wrong: Vec<String> = cases
        .iter()
        .flat_map(|c| [on_the_cpu_path(&faces, c), on_the_cell_path(c)])
        .flatten()
        .collect();
    assert!(wrong.is_empty(), "drawn stale: {wrong:#?}");
}

/// **R2c: a row written is drawn from the terminal on the frame after it,
/// and on the one after that**: a row written with the cursor put back,
/// an incremental frame, then the cursor moved onto that row with nothing
/// written -- which repaints the row from the CPU path's copy -- and
/// another. Both are a fresh renderer's full frame, pixels and grounds,
/// and the two differ -- the control that the second repaints. Named
/// break: the convert's end swapping its two buffers, as it did, rather
/// than bringing the last frame's up to this one's.
#[test]
fn a_row_written_is_drawn_from_the_terminal_on_the_frames_after() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &rows_of_text());
    feed(&mut t, b"\x1b[1;1H");
    let mut r = made(&faces);
    let mut px = Vec::new();
    draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Full);
    for _ in 0..2 {
        draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
    }
    let mut wrong = Vec::new();
    feed(&mut t, b"\x1b[3;1HWRITTEN ROW\x1b[1;1H");
    let now = draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
    let first = fresh(&faces, &mut t, Rows::ZERO);
    differs(&now, &first, "the frame written", &mut wrong);
    feed(&mut t, b"\x1b[3;5H");
    let now = draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
    let second = fresh(&faces, &mut t, Rows::ZERO);
    differs(&now, &second, "the frame after", &mut wrong);
    assert!(
        first.pixels != second.pixels,
        "the control: the cursor moved onto the row written"
    );
    assert!(wrong.is_empty(), "drawn stale: {wrong:?}");
}

/// A field of `s` at `row`, `col`, every character in the message pair.
fn field(row: usize, col: usize, s: &str) -> Field {
    let text = text(s);
    Field {
        row: Row::from_index(row),
        col: Col::at_index(col),
        hints: vec![Hints::MESSAGE; text.len()],
        caret: Col::at_index(col + text.len()),
        text,
    }
}

/// What a frame draws over or lends rows of the CPU path's copy with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Over {
    Composition,
    Find,
    LinkBar,
    Tip,
    Hover,
    Strike,
}

/// `over` drawn on `r`'s next frames, or not.
fn set_over(r: &mut Renderer<'_, '_>, t: &Term<'_>, over: Over, on: bool) {
    match over {
        Over::Composition => r.set_composition(on.then(|| Composition {
            row: Row::from_index(2),
            col: Col::at_index(3),
            text: text("nihao"),
            measure: Measure::default(),
            hints: Vec::new(),
            caret: Caret::End,
        })),
        Over::Find => r.set_find(
            t,
            on.then(|| FindLook {
                marks: Vec::new(),
                field: field(ROWS - 1, 0, "Find abc"),
            }),
        ),
        Over::LinkBar => r.set_link(on.then(|| LinkLook {
            spans: Vec::new(),
            bar: field(4, 2, "https://example.org/a"),
        })),
        Over::Tip => r.set_tip(on.then(|| TipLook {
            lines: vec![field(5, 10, "a word's meaning")],
            area: TipArea {
                top: Row::from_index(5),
                bottom: Row::from_index(6),
                left: Col::at_index(10),
                right: Col::at_index(COLS - 1),
            },
        })),
        Over::Hover => {
            let hover = LinkHover {
                spans: vec![LinkSpan {
                    row: Row::from_index(2),
                    from: Col::at_index(6),
                    to: Col::at_index(12),
                }],
                paint: HoverPaint::Box,
            };
            r.set_link_hover(on.then_some(&hover));
        }
        Over::Strike => {
            let cell = t.view(Rows::ZERO).row(Row::ZERO)[4];
            let lines = [LinkLine {
                row: Row::ZERO,
                from: Col::at_index(4),
                to: Col::at_index(10),
                word: t.style_in(RowStyles::Live, cell),
            }];
            let lines = if on { &lines[..] } else { &[] };
            r.set_link_lines(lines, LineStyle::Solid);
            r.set_link_strikes(lines, &[]);
        }
    }
}

/// **`over` drawn on one frame and gone on the next, nothing written**:
/// the frames to a steady one, a frame with it, which draws other pixels
/// -- the control -- and a frame without it, which is a fresh renderer's
/// full frame, pixels and grounds. What it got wrong, if anything.
fn drawn_over_then_not(faces: &[Face<'_>], over: Over) -> Option<String> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &base());
    let mut r = made(faces);
    let mut px = Vec::new();
    draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Full);
    let mut steady = None;
    for _ in 0..2 {
        steady = Some(draw(
            &mut r,
            &mut t,
            Rows::ZERO,
            &mut px,
            Damage::Incremental,
        ));
    }
    set_over(&mut r, &t, over, true);
    let with = draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
    if steady.is_some_and(|s| s.pixels == with.pixels) {
        return Some(format!("{over:?}: the control, it is drawn"));
    }
    set_over(&mut r, &t, over, false);
    let now = draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
    let full = fresh(faces, &mut t, Rows::ZERO);
    let mut wrong = Vec::new();
    differs(&now, &full, "the frame after", &mut wrong);
    (!wrong.is_empty()).then(|| format!("{over:?}: {}", wrong.join(", ")))
}

/// **R2d: a row veiled, composed over or found over in one frame is drawn
/// from the terminal in the next, with nothing written**: the input
/// method's composition, the find field, the link preview's bar, the
/// hover tip and the link under the pointer, each on a row of its own.
/// Named break: the rows the last frame veiled left out of the rows the
/// next takes again.
#[test]
fn a_row_veiled_last_frame_is_drawn_from_the_terminal() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let wrong: Vec<String> = [
        Over::Composition,
        Over::Find,
        Over::LinkBar,
        Over::Tip,
        Over::Hover,
    ]
    .into_iter()
    .filter_map(|o| drawn_over_then_not(&faces, o))
    .collect();
    assert!(wrong.is_empty(), "drawn stale: {wrong:#?}");
}

/// **R2d: a frame that lent every row of the CPU path's copy is followed by
/// one drawn from the terminal**: a link struck, which lends the copy's
/// cells slots of the renderer's table (`Renderer::lend_cpu`), on one
/// frame, and not on the next. Named break: the last frame's lending left
/// out of the rows the next takes again.
#[test]
fn a_frame_after_one_that_lent_its_copy_is_drawn_from_the_terminal() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    assert_eq!(drawn_over_then_not(&faces, Over::Strike), None);
}

/// **R2e: a window looking back past a history since erased is drawn as
/// the screen it shows**: thirty lines on ten rows, the window five lines
/// back, then ED 3, which erases the history, and a frame asked for the
/// same five lines back, which the terminal clamps to the live screen.
/// It is a fresh renderer's full frame, and the full frames before and
/// after differ -- the control. Then one row of the window written and
/// nothing else, the same five lines back, and a frame again: it is a
/// fresh renderer's full frame, and the full frames before and after the
/// write differ -- its control. These frames are the CPU path's, whose
/// convert clamps the look back before it reads the rows' generations
/// (`Renderer::convert_with`), as the app's cell path does by passing
/// `Term::window_back`; so no break of `Term::row_gens`' own clamp
/// reaches this test.
#[test]
fn a_window_back_past_an_erased_history_is_drawn_as_the_screen_it_shows() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, HIDE);
    for k in 0..30 {
        let nl = if k == 0 { "" } else { "\r\n" };
        feed(&mut t, format!("{nl}line {k} of the history").as_bytes());
    }
    let back = Rows::of_index(5);
    let mut r = made(&faces);
    let mut px = Vec::new();
    draw(&mut r, &mut t, back, &mut px, Damage::Full);
    for _ in 0..2 {
        draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
    }
    let before = fresh(&faces, &mut t, back);
    feed(&mut t, b"\x1b[3J");
    let full = fresh(&faces, &mut t, back);
    assert!(
        before.pixels != full.pixels,
        "the control: the window shows the screen once the history is erased"
    );
    let now = draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
    let mut wrong = Vec::new();
    differs(&now, &full, "the frame after", &mut wrong);
    assert!(wrong.is_empty(), "drawn stale: {wrong:?}");
    feed(&mut t, b"\x1b[4;1Hone row written, no other");
    let written = fresh(&faces, &mut t, back);
    assert!(
        full.pixels != written.pixels,
        "the control: the row written shows"
    );
    let now = draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
    differs(&now, &written, "the frame after the write", &mut wrong);
    assert!(wrong.is_empty(), "drawn stale: {wrong:?}");
}

/// **R3: a row whose drawn order a write to another row changes is drawn
/// again**: one line wrapped over two rows, the first ending in Hebrew and
/// the second beginning with digits, a space and Hebrew; then the first
/// row's Hebrew written over in Latin. Nothing is written to the second
/// row, but its digits, which followed a right-to-left letter (UAX #9
/// W7), now follow a left-to-right one, so its order changes: its band
/// differs between the full frames before and after -- the control --
/// and the incremental frame is a fresh renderer's full frame. Named
/// break: a row drawn in another order kept by its stored row's
/// generation alone.
#[test]
fn a_row_another_row_reorders_is_drawn_again() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let first = format!("{} \u{5d0}\u{5d1}\u{5d2}", "x".repeat(COLS - 4));
    feed(&mut t, HIDE);
    feed(
        &mut t,
        format!("\x1b[1;1H{first}123 \u{5d3}\u{5d4}\u{5d5} xyz").as_bytes(),
    );
    let mut r = made(&faces);
    let mut px = Vec::new();
    draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Full);
    for _ in 0..2 {
        draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
    }
    let before = fresh(&faces, &mut t, Rows::ZERO);
    feed(&mut t, format!("\x1b[1;{}Habc", COLS - 2).as_bytes());
    let full = fresh(&faces, &mut t, Rows::ZERO);
    let width = usize::try_from(r.pixel_size().0.value()).unwrap();
    let band = usize::try_from(r.metrics().cell_h.value()).unwrap() * width;
    assert!(
        before.pixels[band..2 * band] != full.pixels[band..2 * band],
        "the control: the second row's order follows the first's"
    );
    let now = draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
    let mut wrong = Vec::new();
    differs(&now, &full, "the frame after", &mut wrong);
    assert!(wrong.is_empty(), "drawn stale: {wrong:?}");
}
