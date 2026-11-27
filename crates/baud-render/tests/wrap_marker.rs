//! **The word wrap's continuation mark** (`[screen] wrap_marker`, on by
//! default with `wrap_words`): a hook arrow in the window's padding beside
//! each line that continues a paragraph the display re-wrapped at its
//! words, on the start side of the line; never in a cell, never copied,
//! the grid keeping every column; nothing on the alternate screen, on a
//! row the terminal cut, or where the padding has no room.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Argb, Backdrop, Blend, Damage, Instance, Leaving, PxSize, Renderer, Scrolled, Selection, Span,
    Stack, StatusMarks, Target, Theme, WordWrap, WrapMarker, contrast_over,
};
use support::font;

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The shell's prompt mark: what makes the text above it finished.
const PROMPT: &str = "\x1b]133;A\x07$ ";

fn history(rows: u16) -> String {
    (0..rows).map(|i| format!("h{i}\r\n")).collect()
}

/// The margin each side lends: a Retina display's 8 pt padding.
const BAND: Px = Px::of(16);

fn renderer<'f, 'a>(faces: &'f [Face<'a>], ws: &WinSize, theme: Theme) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap()
}

/// The window rows the marks are on, top first, and the side of each:
/// `L` in the left margin, `R` in the right.
fn marked(r: &Renderer<'_, '_>) -> Vec<(i32, char)> {
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let whole = i32::try_from(r.whole_size().0.value()).unwrap();
    let band = i32::try_from(r.margins().value()).unwrap();
    r.margin_records()
        .iter()
        .map(|i: &Instance| {
            let x = i32::from(i.x.0);
            let side = match x {
                x if x >= 0 && x + i32::from(i.w.0) <= band => 'L',
                x if x >= whole - band && x + i32::from(i.w.0) <= whole => 'R',
                _ => '?',
            };
            (i32::from(i.y.0).div_euclid(ch), side)
        })
        .collect()
}

const W10: WinSize = WinSize {
    ws_row: 8,
    ws_col: 10,
};

/// Three finished paragraphs above a prompt at ten columns, drawn on the
/// window's rows 0 to 6: a path the display re-wraps onto two lines,
/// `ls /home` then `/jordan`; a word wider than the line, whose break at
/// the edge is the terminal's own cut, so the display keeps it as stored,
/// `abcdefghij` then `klm`; and a sentence the terminal cut inside its
/// words, re-wrapped onto three, `a quick`, `brown fox`, `jumps`.
fn three_paragraphs() -> String {
    format!(
        "{}ls /home/jordan\r\nabcdefghijklm\r\na quick brown fox jumps\r\n{PROMPT}",
        history(8)
    )
}

/// **A mark beside each line the display re-wrapped, but its first**:
/// rows 1 (`/jordan`), 5 (`brown fox`) and 6 (`jumps`), each in the left
/// margin; none beside a paragraph's first line, none beside `klm`, a row
/// the terminal cut that the display draws as stored, and none beside the
/// prompt.
#[test]
fn a_mark_is_drawn_beside_each_line_the_display_re_wrapped() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, &W10, Theme::dark());
    r.set_margins(BAND);
    let mut owned = Owned::new(&W10);
    let mut t = Term::new(owned.storage(), &W10);
    feed(&mut t, &three_paragraphs());
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert_eq!(marked(&r), [(1, 'L'), (5, 'L'), (6, 'L')]);
    // With a smooth scroll's line above the window drawn, the marks ride
    // with their lines: the leaving line is row -1.
    r.place_marks(&t, Rows::ZERO, Leaving::Drawn, Px::of(3));
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let ys: Vec<i32> = r
        .margin_records()
        .iter()
        .map(|i| i32::from(i.y.0))
        .collect();
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    let still: Vec<i32> = r
        .margin_records()
        .iter()
        .map(|i| i32::from(i.y.0))
        .collect();
    assert_eq!(
        ys,
        still.iter().map(|y| y + 3).collect::<Vec<_>>(),
        "lowered by the pan"
    );
    assert!(ys.iter().all(|&y| y > -ch), "none on a line not drawn");
}

/// **A right-to-left line is marked on the right**, its start side: a
/// Hebrew paragraph re-wrapped at its words, its continuation lines laid
/// against the right edge.
#[test]
fn a_right_to_left_line_is_marked_in_the_right_margin() {
    let ws = WinSize {
        ws_row: 8,
        ws_col: 20,
    };
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, &ws, Theme::dark());
    r.set_margins(BAND);
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let he = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD} \u{5D6}\u{5D4} \u{5DE}\u{5E9}\u{5E4}\u{5D8} \u{5D0}\u{5E8}\u{5D5}\u{5DA} \u{5DE}\u{5D0}\u{5D5}\u{5D3}";
    feed(&mut t, &format!("{}{he}\r\n{PROMPT}", history(8)));
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    let got = marked(&r);
    assert!(!got.is_empty(), "the paragraph was re-wrapped");
    assert!(got.iter().all(|&(_, s)| s == 'R'), "{got:?}");
}

/// **Nothing on the alternate screen**, a full-screen program's, which the
/// display never re-wraps; the same text on the primary screen is marked,
/// the positive control.
#[test]
fn the_alternate_screen_has_no_marks() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, &W10, Theme::dark());
    r.set_margins(BAND);
    let mut owned = Owned::new(&W10);
    let mut t = Term::new(owned.storage(), &W10);
    feed(&mut t, &three_paragraphs());
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert!(!r.margin_records().is_empty(), "positive control");
    feed(&mut t, &format!("\x1b[?1049h{}", three_paragraphs()));
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert!(r.margin_records().is_empty());
}

/// **Nothing where the padding has no room**, and no margin then: with no
/// band lent, with one too narrow for the mark, with the setting off and
/// with the word wrap off. A band the mark fits is the positive control.
#[test]
fn without_room_or_with_either_setting_off_nothing_is_drawn() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, &W10, Theme::dark());
    let mut owned = Owned::new(&W10);
    let mut t = Term::new(owned.storage(), &W10);
    feed(&mut t, &three_paragraphs());
    let marks = |r: &mut Renderer<'_, '_>| {
        r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
        r.margin_records().len()
    };
    assert_eq!(marks(&mut r), 0, "no band lent");
    r.set_margins(Px::of(7));
    assert_eq!(marks(&mut r), 0, "a band too narrow");
    r.set_margins(Px::of(8));
    assert_eq!(
        marks(&mut r),
        3,
        "positive control: the narrowest that fits"
    );
    r.set_margins(BAND);
    r.set_wrap_marker(WrapMarker::Off);
    assert_eq!(r.margins(), Px::of(0), "no margin while off");
    assert_eq!(marks(&mut r), 0, "the setting off");
    r.set_wrap_marker(WrapMarker::On);
    r.set_word_wrap(WordWrap::Off);
    assert_eq!(r.margins(), Px::of(0));
    assert_eq!(marks(&mut r), 0, "the word wrap off");
}

/// **The grid keeps every column, and its pixels**: the margins widen the
/// whole frame by a band each side and nothing else -- the grid, its frame
/// and the cell are the same -- the grid is drawn a band in, beside the
/// exit-status strip when kept, and its pixels are the frame without
/// margins' at that offset. The margins hold the page but where a mark is.
#[test]
fn the_margins_take_no_column_and_leave_the_grid_as_it_was() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    for status in [StatusMarks::Off, StatusMarks::On] {
        let frame = |band: Px| {
            let mut r = renderer(&faces, &W10, Theme::dark());
            r.set_status_marks(status);
            r.set_margins(band);
            let mut owned = Owned::new(&W10);
            let mut t = Term::new(owned.storage(), &W10);
            feed(&mut t, &three_paragraphs());
            r.fit(&t);
            r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
            let (w, h) = r.whole_size();
            let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
            {
                let mut target = Target::new(&mut px, w, w, h).unwrap();
                let seal = t.seal_writes();
                r.render(&t, seal, &mut target, Damage::Full);
            }
            let facts = (r.grid(), r.frame_size(), r.metrics().cell_w, r.grid_left());
            (
                px,
                usize::try_from(w.value()).unwrap(),
                facts,
                r.clear_color(),
            )
        };
        let (plain, pw, (grid, fs, cw, left0), _) = frame(Px::of(0));
        let (wide, ww, (grid2, fs2, cw2, left), clear) = frame(BAND);
        assert_eq!(
            (grid, fs, cw),
            (grid2, fs2, cw2),
            "{status:?}: the grid as it was"
        );
        let b = usize::try_from(BAND.value()).unwrap();
        assert_eq!(ww, pw + 2 * b, "{status:?}");
        assert_eq!(left.value(), left0.value() + BAND.value(), "{status:?}");
        let h = plain.len() / pw;
        for y in 0..h {
            assert_eq!(
                &wide[y * ww + b..y * ww + b + pw],
                &plain[y * pw..(y + 1) * pw],
                "{status:?}: row {y} of the grid's frame"
            );
        }
        // The page in the margins, but on a marked row's left band.
        let ch = usize::try_from(fs.1.value()).unwrap() / usize::from(W10.ws_row);
        let marked_rows = [1usize, 5, 6];
        let page = clear.0;
        let mut inked = vec![false; usize::from(W10.ws_row)];
        for y in 0..h {
            let left_band = &wide[y * ww..y * ww + b];
            let right_band = &wide[y * ww + ww - b..(y + 1) * ww];
            assert!(
                right_band.iter().all(|&p| p == page),
                "{status:?}: right, row {y}"
            );
            if left_band.iter().any(|&p| p != page) {
                inked[y / ch] = true;
            }
        }
        let rows: Vec<usize> = (0..inked.len()).filter(|&r| inked[r]).collect();
        assert_eq!(
            rows, marked_rows,
            "{status:?}: the left margin's inked rows"
        );
    }
}

/// **A copy never holds the mark**: the paragraph re-wrapped and marked
/// copies as the program wrote it, the mark being in no cell.
#[test]
fn a_copy_of_a_marked_paragraph_is_the_stored_text() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, &W10, Theme::dark());
    r.set_margins(BAND);
    let mut owned = Owned::new(&W10);
    let mut t = Term::new(owned.storage(), &W10);
    feed(&mut t, &three_paragraphs());
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert!(!r.margin_records().is_empty(), "positive control: marked");
    // "ls /home/jordan" is stored on window rows 0 and 1, cut "ls /home/j"
    // and "ordan": the display draws it on the same two rows.
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    let line = |y: Row| scrolled.line(Rows::ZERO, y);
    let row = |y: Row| t.view(Rows::ZERO).row(y);
    let (a, b) = (Row::from_index(0), Row::from_index(1));
    let mut sel = Selection::at(
        Span::cell(row(a), line(a), Col::ZERO),
        baud_render::Grain::Cell,
    );
    sel.extend(Span::cell(row(b), line(b), Col::at_index(4)));
    let text = sel.text(&t, scrolled);
    assert_eq!(text, "ls /home/jordan");
    assert!(!text.contains('\u{21AA}'));
}

/// **The mark reads at 3:1 on the glass**, against every grey the
/// window's material shows under the page, in both themes, at the
/// default dim and at a dim of a quarter: palette entry 8 alone reads below
/// 3:1 at the quarter over the dark material, the positive control that
/// the mark is lifted where it must be.
#[test]
fn the_mark_reads_at_three_to_one_on_the_glass() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut lifted = 0;
    for (base, backdrop, dim) in [
        (Theme::dark(), Backdrop::under_window_dark(), 0.70),
        (Theme::light(), Backdrop::under_window_light(), 0.60),
        (Theme::dark(), Backdrop::under_window_dark(), 0.25),
        (Theme::light(), Backdrop::under_window_light(), 0.25),
    ] {
        let mut theme = base;
        theme.bg_alpha = Chan::from_unit(dim);
        theme.backdrop = backdrop;
        let asked: Argb = theme.palette[8];
        let mut r = renderer(&faces, &W10, theme);
        r.set_margins(BAND);
        let mut owned = Owned::new(&W10);
        let mut t = Term::new(owned.storage(), &W10);
        feed(&mut t, &three_paragraphs());
        r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
        let page = r.clear_color_of(&t);
        let opaque = page.with_alpha(Chan::from_u8(0xFF));
        let mark = r.margin_records()[0].color;
        let ratio = contrast_over(mark, opaque, page.a(), backdrop);
        assert!(ratio >= 3.0, "{dim}: {ratio}");
        if contrast_over(asked, opaque, page.a(), backdrop) < 3.0 {
            assert_ne!(mark, asked, "{dim}: lifted");
            lifted += 1;
        }
    }
    assert!(
        lifted > 0,
        "positive control: entry 8 alone fails somewhere"
    );
}
