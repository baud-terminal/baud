//! **A window over the history, drawn where it is and nowhere else.** The
//! CPU path paints the window it is handed, so a scrolled-back frame shows
//! the lines behind the screen. And the window is the primary screen's: the
//! alternate screen keeps no history, so while it is up the window a driver
//! hands the renderer -- left scrolled back when a program entered the
//! alternate screen -- names lines the screen does not have. The renderer
//! draws what `Term::view` draws, and these hold its other readers to the
//! same clamp: the cursor, and a selection's text.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Blend, Damage, Grain, PxSize, Renderer, Scrolled, Selection, Span, Stack, Target, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 10,
    ws_row: 4,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Six lines on the primary, two of them retained, then the alternate
/// screen with four rows of its own and the cursor homed onto `p`.
fn on_the_alternate_screen(t: &mut Term<'_>) {
    feed(t, b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix");
    assert_eq!(t.scrollback().len(), 2, "two lines retained");
    feed(t, b"\x1b[?1049h\x1b[Hp\r\nq\r\nr\r\ns\x1b[H");
    assert_eq!(
        t.scrollback().len(),
        2,
        "the history is still the primary's, and still reported"
    );
}

/// The block cursor is drawn on the alternate screen whatever window the
/// renderer is handed: the program that owns the screen owns the cursor,
/// and a window left scrolled back on the primary does not reach it.
#[test]
fn the_cursor_is_drawn_on_the_alternate_screen_under_a_stale_window() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let new = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    on_the_alternate_screen(&mut t);
    let mut live = new();
    let _ = live.convert(&t, Rows::ZERO, Damage::Full);
    let live = live.background().to_vec();
    // The control: a cursor is drawn at all, so the cell under it is not
    // the word an ordinary blank is.
    assert_ne!(live[0], live[9], "the cursor swaps the cell it is on");
    let mut stale = new();
    let _ = stale.convert(&t, Rows::of_count(2), Damage::Full);
    assert_eq!(
        stale.background(),
        &live[..],
        "a window two lines up is the live screen here, cursor and all"
    );
}

/// A selection over lines above the alternate screen's top reads none of
/// them. Read through the view, which the alternate screen clamps to
/// itself, every one of them came back as the live screen's top row -- so
/// a copy of four rows repeated `p` where nothing was.
#[test]
fn a_selection_above_the_alternate_screen_copies_none_of_those_lines() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    on_the_alternate_screen(&mut t);
    let scrolled = Scrolled::START.advanced(Rows::of_count(8));
    let view = t.view(Rows::of_count(2));
    let start = Span::cell(
        view.row(Row::ZERO),
        scrolled.line(Rows::of_count(2), Row::ZERO),
        Col::at_index(0),
    );
    let live = t.viewport();
    let end = Span::cell(
        live.row(Row::from_index(1)),
        scrolled.line(Rows::ZERO, Row::from_index(1)),
        Col::at_index(9),
    );
    let mut s = Selection::at(start, Grain::Cell);
    s.extend(end);
    assert_eq!(s.text(&t, scrolled), "\n\np\nq");
}

/// The CPU path paints the window it is handed: two lines up, the frame is
/// the four lines a terminal that never scrolled shows live. It painted the
/// live screen whatever the window, so with the GPU off the wheel moved the
/// thumb, the selection and the link under a click into history the text
/// never showed.
#[test]
fn the_cpu_path_paints_the_window_it_is_handed() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let new = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let paint = |t: &mut Term<'_>, back: Rows| -> Vec<u32> {
        let mut r = new();
        let (w, h) = r.pixel_size();
        let mut pixels = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
        {
            let mut target = Target::new(&mut pixels, w, w, h).unwrap();
            let seal = t.seal_writes();
            r.render_at(t, seal, back, &mut target, Damage::Full);
        }
        pixels
    };
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25lone\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix",
    );
    assert_eq!(t.scrollback().len(), 2, "two lines retained");
    let mut shallow = Owned::new(&WS);
    let mut u = Term::new(shallow.storage(), &WS);
    feed(&mut u, b"\x1b[?25lone\r\ntwo\r\nthree\r\nfour");
    assert_eq!(u.scrollback().len(), 0, "the reference never scrolled");
    let back = paint(&mut t, Rows::of_count(2));
    assert_ne!(
        back,
        paint(&mut t, Rows::ZERO),
        "the control: two lines up is not the live screen"
    );
    assert_eq!(
        back,
        paint(&mut u, Rows::ZERO),
        "two lines up is the four lines behind and above the screen"
    );
}
