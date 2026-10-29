//! **What follows the display's word wrap** (`[screen] wrap_words`) past
//! the window's own rows: the line a smooth scroll slides out, the cells a
//! trail's flash bound reads, the pictures anchored above the window and
//! the lines a copy reads under `?96`. Each reads the line the display
//! draws, not the row the terminal stores there.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Blend, Damage, Grain, PxSize, Renderer, Scrolled, Selection, Shape, Smear, Span, Stack,
    Strength, Target, Theme, TrailLook, WordWrap, under_threshold,
};
use support::font;

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The shell's prompt mark: what makes the text above it finished.
const PROMPT: &str = "\x1b]133;A\x07$ ";

/// Lines that scroll into the history first.
fn history(rows: u16) -> String {
    (0..rows).map(|i| format!("h{i}\r\n")).collect()
}

const WS: WinSize = WinSize {
    ws_row: 10,
    ws_col: 30,
};

/// Seven seven-letter words: 55 cells, two rows at 30 columns, and three
/// lines at its words (three words a line).
const SEVEN: &str = "aaaaaaa bbbbbbb ccccccc ddddddd eeeeeee fffffff ggggggg";

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

/// A frame on the CPU path.
fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Vec<u32> {
    let (w, h) = r.pixel_size();
    let px = |p: baud_font::Px| usize::try_from(p.value()).unwrap();
    let mut pixels = vec![0u32; px(w) * px(h)];
    let mut target = Target::new(&mut pixels, w, w, h).unwrap();
    r.render(t, &mut target, Damage::Full);
    pixels
}

/// **The line a smooth scroll slides out is the display's**: a
/// double-width row stored just above the window, with a finished
/// paragraph below it that the display draws on one line more, is pushed
/// up out of the window one line up, whose first line is then the
/// single-width "x1". The leaving row is drawn as that line, so no sized
/// records are made for it; with the setting off the stored double row
/// is the line, and they are.
#[test]
fn the_leaving_row_takes_the_size_of_the_line_the_display_draws() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let fillers: String = (1..=7).map(|i| format!("x{i}\r\n")).collect();
    let bytes = format!(
        "\x1b[?25l{}\x1b#6dbl\r\n{fillers}{SEVEN}\r\n{PROMPT}",
        history(10)
    );
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &bytes);
    let up = t.view(Rows::of_count(1));
    assert!(
        up.rendition(Row::ZERO).is_double(),
        "the premise: the stored row above the window is double"
    );
    let mut sized = Vec::new();
    for wrap in [WordWrap::On, WordWrap::Off] {
        let mut r = renderer(&faces);
        r.set_word_wrap(wrap);
        let _ = paint(&mut r, &mut t);
        let mut out = Vec::new();
        r.sized_leaving(&t, Rows::ZERO, &mut out);
        sized.push(out.len());
    }
    assert_eq!(sized[0], 0, "on: the leaving line is the single-width x1");
    assert!(sized[1] > 0, "the control, off: the stored double row");
}

/// **A trail's flash bound reads the cells the display draws**: a red
/// row stored at window row 1 is drawn on row 0 once the paragraph below
/// takes a line more, and a repeat pass over row 1 alone, where an erased
/// row of the page's own colour is drawn, keeps the page's cap, at which
/// black never flashes on the dark page; reading the stored row 1, the red
/// one, capped it as over red. With the setting off the red row is drawn
/// on row 1, and the cap is red's.
#[test]
fn the_flash_bound_reads_the_cells_the_display_draws() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let bytes = format!(
        "\x1b[?25l\x1b]12;rgb:00/00/00\x07{}h\r\n\x1b[41m\x1b[K\x1b[0m\r\n\r\n{SEVEN}\r\nf1\r\nf2\r\nf3\r\nf4\r\n{PROMPT}",
        history(10)
    );
    let mut caps = Vec::new();
    for wrap in [WordWrap::On, WordWrap::Off] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &bytes);
        let mut r = renderer(&faces);
        r.set_word_wrap(wrap);
        let _ = paint(&mut r, &mut t);
        let style = baud_core::CursorStyle::SteadyBlock;
        let (a, b) = (
            r.cursor_rect(Row::from_index(1), Col::ZERO, style),
            r.cursor_rect(Row::from_index(1), Col::at_index(29), style),
        );
        r.set_trail(Some(TrailLook::Smear(Smear {
            corners: baud_render::Rect {
                left: a.left,
                top: a.top,
                right: b.right,
                bottom: b.bottom,
            }
            .corners(),
            hole: r.cursor_rect(Row::from_index(9), Col::at_index(29), style),
            shape: Shape::Quad,
            strength: Strength::UnderThreshold,
        })));
        let _ = paint(&mut r, &mut t);
        caps.push(r.flash_cap());
    }
    let black = Argb::rgb(Chan::from_u8(0), Chan::from_u8(0), Chan::from_u8(0));
    let page = Theme::dark().bg.with_alpha(Chan::from_u8(0xFF));
    assert_eq!(caps[0], under_threshold(black, page), "on: over the page");
    assert_eq!(caps[0].get(), 0xFF, "black never flashes on the page");
    assert!(caps[1].get() < 0xFF, "the control, off: over the red row");
}

/// A Sixel one pixel wide and twelve sixels tall, all red, at a pixel
/// aspect of 1:1 (`P1` 7).
fn tall_red() -> String {
    format!("\x1bP7;1;0q#0;2;100;0;0{}~\x1b\\", "~-".repeat(11))
}

/// The rows of the frame's first column the red picture covers, as
/// (first, one past the last); none where it is off the frame.
fn red_rows(pixels: &[u32], w: usize) -> Option<(usize, usize)> {
    let red = |p: u32| (p >> 16) & 0xFF > 0xC0 && (p >> 8) & 0xFF < 0x40 && p & 0xFF < 0x40;
    let ys: Vec<usize> = (0..pixels.len() / w)
        .filter(|&y| red(pixels[y * w]))
        .collect();
    Some((*ys.first()?, ys.last()? + 1))
}

/// **A picture follows its text as on a scroll**: anchored on the window's
/// first row, which the paragraph below pushes out of the window when the
/// display draws it on one line more, the picture is drawn a line higher,
/// its top above the frame; and once that row has scrolled into the
/// history, anchored above the window, two lines higher than as stored,
/// as the text above the window moves. With the setting off it is drawn
/// where it is stored, the control.
#[test]
fn a_picture_anchored_above_or_pushed_out_moves_with_the_text() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let screen = format!(
        "\x1b[?25l{}x1\r\nx2\r\nx3\r\n{SEVEN}\r\nf1\r\nf2\r\nf3\r\nf4\r\n{PROMPT}",
        history(10)
    );
    let place = format!("\x1b[1;1H{}\x1b[10;3H", tall_red());
    let mut at = Vec::new();
    let mut ch = 0;
    for wrap in [WordWrap::On, WordWrap::Off] {
        let mut owned = Owned::new(&WS).with_images(baud_domain::ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &screen);
        feed(&mut t, &place);
        let mut r = renderer(&faces);
        r.set_word_wrap(wrap);
        let w = usize::try_from(r.pixel_size().0.value()).unwrap();
        ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
        let pushed = red_rows(&paint(&mut r, &mut t), w);
        feed(&mut t, &format!("\r\n{PROMPT}"));
        let above = red_rows(&paint(&mut r, &mut t), w);
        at.push((pushed, above));
    }
    let (on, off) = (at[0], at[1]);
    let tall = off.0.expect("the control, off: drawn").1;
    assert!(tall > 2 * ch, "the premise: taller than two rows, {tall}");
    assert_eq!(off.0, Some((0, tall)), "the control, off: on row 0, whole");
    assert_eq!(on.0, Some((0, tall - ch)), "on: a line higher, its top cut");
    assert_eq!(off.1, Some((0, tall - ch)), "the control, off: a line up");
    assert_eq!(on.1, Some((0, tall - 2 * ch)), "on: two lines up");
}

/// **`?96` copies the lines as they are drawn**: each line of a copy "from
/// its right edge" (`[[source: vt510-decrlcm]]`), and on a finished
/// paragraph the display draws at its words the lines are its display
/// lines. The reported paragraph, Hebrew, an English run, Hebrew, at 44
/// columns: the first Hebrew line reads right to left in the order it
/// reads, the English line, flush left, from its right edge -- the run
/// reversed, its trailing space first -- and the last Hebrew line in its
/// order, the lines joined as a soft wrap joins rows. Read by the stored
/// rows the cut "mid|dle" falls inside the copy, the control; and with the
/// mode reset the copy is the stored text in logical order.
#[test]
fn decrlcm_reads_a_re_broken_paragraph_by_its_display_lines() {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 44,
    };
    let he1 =
        "\u{5D6}\u{5D5} \u{5E4}\u{5E1}\u{5E7}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA} ";
    let en = "with English words in the middle, ";
    let he2 = "\u{5E9}\u{5DE}\u{5DE}\u{5E9}\u{5D9}\u{5DB}\u{5D4} \u{5DC}\u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D4}\u{5D1}\u{5D0}\u{5D4}.";
    let bytes = format!("{}{he1}{en}{he2}\r\n{PROMPT}\x1b[?96h", history(8));
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, &bytes);
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    let line = |r: usize| scrolled.line(Rows::ZERO, Row::from_index(r));
    let row = |r: usize| t.view(Rows::ZERO).row(Row::from_index(r));
    // The paragraph is stored on window rows 3 and 4.
    let mut sel = Selection::at(Span::cell(row(3), line(3), Col::ZERO), Grain::Cell);
    sel.extend(Span::cell(row(4), line(4), Col::at_index(43)));
    let want = format!("{he1}{}{he2}", en.chars().rev().collect::<String>());
    assert_eq!(sel.text(&t, scrolled), want);
    assert_eq!(sel.text_as(&t, scrolled, WordWrap::On), want);
    let stored = sel.text_as(&t, scrolled, WordWrap::Off);
    assert_ne!(stored, want, "the control: by the stored rows");
    assert!(stored.contains("dim eht"), "{stored}");
    feed(&mut t, "\x1b[?96l");
    assert_eq!(sel.text(&t, scrolled), format!("{he1}{en}{he2}"));
}

/// **A paragraph that wrapped at side margins keeps the terminal's
/// cut**: written inside DECSLRM margins at columns 4 to 20, the word
/// "dddd" cut at the right margin, beside "LL" in columns 1 and 2 of
/// each row, outside the margins. Re-broken, "LL" would be read into the
/// paragraph; kept, every row is drawn as stored. The same words from
/// column 4 with no margins, cut at the page's edge, are re-broken, the
/// control.
#[test]
fn a_paragraph_wrapped_at_side_margins_keeps_its_cut() {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 20,
    };
    let words = "aaaa bbbb cccc dddd eeee";
    let inside = format!(
        "{}\x1b[?69h\x1b[4;20s\x1b[3;4H{words}\x1b[3;1HLL\x1b[4;1HLL\x1b[?69l\x1b[6;1H{PROMPT}",
        history(6)
    );
    let page = format!("{}\x1b[3;4H{words}\x1b[6;1H{PROMPT}", history(6));
    let laid = |bytes: &str| {
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, bytes);
        let vp = t.view(Rows::ZERO);
        let stored = (0..6)
            .map(|y| vp.row(Row::from_index(y)).to_vec())
            .collect::<Vec<_>>();
        let mut o = baud_render::Reorder::new();
        o.lay_out(&t, Rows::ZERO);
        let shown = (0..6)
            .map(|y| o.view(&t, Rows::ZERO).row(Row::from_index(y)).to_vec())
            .collect::<Vec<_>>();
        (o.orders().display().is_some(), stored == shown)
    };
    assert_eq!(laid(&inside), (false, true), "inside margins: as stored");
    assert!(laid(&page).0, "the control: cut at the page's edge");
}

/// **A paragraph that runs on from the history to the window's first
/// row**: "Y" ten times, cut after its sixth letter at the top of the
/// window. The display draws the word whole on the window's first line,
/// its first six letters from the stored row above the window (columns
/// 24 to 29) and the rest from the window's first row. The word again on
/// the row below, whole, is the newest match a find makes current.
fn cut_at_the_top() -> String {
    let fillers: String = (2..=8).map(|i| format!("f{i}\r\n")).collect();
    let fillers = format!("f1 {}\r\n{fillers}", "Y".repeat(10));
    format!(
        "{}{} {} zz\r\n{fillers}{PROMPT}",
        history(10),
        "X".repeat(23),
        "Y".repeat(10)
    )
}

/// The cells of the window's first line, the stored row above it, and the
/// window's first row, and the line numbering they share.
fn cut_term(t: &Term<'_>) -> (Scrolled, baud_render::AbsLine) {
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    (scrolled, scrolled.top(Rows::ZERO))
}

/// **A selection reaches every cell a window line draws**, a cell of the
/// stored row above the window included: the word "YYYYYYYYYY" selected
/// from its stored start above the window to its end on the window's
/// first row is drawn selected on all ten of its cells on that line, and
/// one selected wholly above the window, on its six. The display names
/// those six cells by their stored line, one above the window's top, and
/// a mouse report still names a window cell for them. The highlight
/// clipped to the window's rows, as it was drawn before, selects only the
/// window's four, the control; and with no line past the window asked, a
/// selection wholly above it has no highlight.
#[test]
fn a_selection_reaches_the_cells_a_window_line_draws_from_above_it() {
    use baud_render::{Beyond, Reorder, WindowLine};
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &cut_at_the_top());
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let orders = o.orders();
    let above = WindowLine::of(Row::ZERO).up(Rows::of_count(1));
    assert_eq!(
        orders.stored_line_at(Row::ZERO, Col::ZERO),
        Some((above, Col::at_index(24))),
        "the premise: the first line starts with a cell of the row above"
    );
    assert_eq!(
        orders.stored_line_at(Row::ZERO, Col::at_index(6)),
        Some((WindowLine::of(Row::ZERO), Col::ZERO))
    );
    assert_eq!(orders.stored_at(Row::ZERO, Col::ZERO), None);
    assert_eq!(
        orders.pointer_line(Row::ZERO, Col::at_index(2)),
        (above, Col::at_index(26)),
        "a selection's pointer names the stored line"
    );
    assert_eq!(
        orders.pointer(Row::ZERO, Col::at_index(2)).0,
        Row::ZERO,
        "a report's pointer names a window row"
    );
    let beyond = Beyond::of(&t, Rows::ZERO);
    assert_eq!(beyond.above, Rows::of_count(1));
    assert_eq!(beyond.below, Rows::ZERO);
    let (scrolled, top) = cut_term(&t);
    let up_line = top.above().unwrap();
    let up_row = t.view(Rows::of_count(1)).row(Row::ZERO).to_vec();
    let first = t.view(Rows::ZERO).row(Row::ZERO).to_vec();
    let shown = o.view(&t, Rows::ZERO).row(Row::ZERO).to_vec();
    let (cols, rows) = (Cols::of_index(30), Rows::of_index(10));
    let selected = |h: &baud_render::Highlight| -> Vec<bool> {
        (0..13)
            .map(|x| orders.selects(h, Row::ZERO, Col::at_index(x), shown[x].content))
            .collect()
    };
    let mut word = Selection::at(Span::cell(&up_row, up_line, Col::at_index(24)), Grain::Cell);
    word.extend(Span::cell(&first, top, Col::at_index(3)));
    let h = word
        .highlight_reaching(top, rows, cols, beyond)
        .expect("drawn");
    let want: Vec<bool> = (0..13).map(|x| x < 10).collect();
    assert_eq!(selected(&h), want, "all ten letters on the first line");
    assert_eq!(word.text(&t, scrolled), "YYYYYYYYYY");
    let clipped = word.highlight(top, rows, cols).expect("on the window");
    let clipped = baud_render::Highlight::new(clipped.from, clipped.to, clipped.region);
    let want: Vec<bool> = (0..13).map(|x| (6..10).contains(&x)).collect();
    assert_eq!(selected(&clipped), want, "the control: the window's four");
    let mut part = Selection::at(Span::cell(&up_row, up_line, Col::at_index(24)), Grain::Cell);
    part.extend(Span::cell(&up_row, up_line, Col::at_index(29)));
    assert_eq!(part.highlight(top, rows, cols), None, "the control");
    let h = part
        .highlight_reaching(top, rows, cols, beyond)
        .expect("drawn on the window's first line");
    assert!(
        (0..10).all(|r| !h.covers_row(Row::from_index(r))),
        "no window row's own cells"
    );
    let want: Vec<bool> = (0..13).map(|x| x < 6).collect();
    assert_eq!(selected(&h), want, "the six from above");
}

/// **A selection's pixels on the CPU path**: the word selected from its
/// stored start above the window to its end on the window's first row is
/// painted selected on all ten of its cells on the first line, six of
/// them stored above the window; and its six letters above the window
/// alone, on those six. The frame differs from the unselected one there
/// and nowhere else.
#[test]
fn a_selection_above_the_window_is_painted_on_its_first_line() {
    use baud_render::Beyond;
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &cut_at_the_top());
    let (_, top) = cut_term(&t);
    let up_line = top.above().unwrap();
    let up_row = t.view(Rows::of_count(1)).row(Row::ZERO).to_vec();
    let first = t.view(Rows::ZERO).row(Row::ZERO).to_vec();
    let (cols, rows) = (Cols::of_index(30), Rows::of_index(10));
    let mut r = renderer(&faces);
    let plain = paint(&mut r, &mut t);
    let w = usize::try_from(r.pixel_size().0.value()).unwrap();
    let m = r.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let mut lit_cells = |t: &mut Term<'_>, h| {
        r.set_highlight(h);
        let lit = paint(&mut r, t);
        let mut cells: Vec<(usize, usize)> = (0..plain.len())
            .filter(|&i| plain[i] != lit[i])
            .map(|i| ((i / w) / ch, (i % w) / cw))
            .collect();
        cells.sort();
        cells.dedup();
        cells
    };
    let mut word = Selection::at(Span::cell(&up_row, up_line, Col::at_index(24)), Grain::Cell);
    word.extend(Span::cell(&first, top, Col::at_index(3)));
    let want: Vec<(usize, usize)> = (0..10).map(|x| (0, x)).collect();
    assert_eq!(
        lit_cells(&mut t, word.highlight(top, rows, cols)),
        want,
        "the word's ten cells, and only they"
    );
    let mut part = Selection::at(Span::cell(&up_row, up_line, Col::at_index(24)), Grain::Cell);
    part.extend(Span::cell(&up_row, up_line, Col::at_index(29)));
    let h = part.highlight_reaching(top, rows, cols, Beyond::of(&t, Rows::ZERO));
    let want: Vec<(usize, usize)> = (0..6).map(|x| (0, x)).collect();
    assert_eq!(
        lit_cells(&mut t, h),
        want,
        "the six from above, and only they"
    );
}

/// **A find mark reaches the cells a window line draws from above it**:
/// the match "YYYYYYYYYY", stored across the row above the window and the
/// window's first row, is marked on both, the first by its line above the
/// window, and washed on the first line's ten cells. The newer match on
/// the row below is the current one, drawn as the selection, not washed.
#[test]
fn a_find_mark_reaches_the_cells_a_window_line_draws_from_above_it() {
    use baud_render::{Find, FindLook, WindowLine};
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &cut_at_the_top());
    let mut f = Find::new();
    let query: Vec<baud_domain::Codepoint> = "YYYYYYYYYY"
        .chars()
        .map(baud_domain::Codepoint::from_char)
        .collect();
    f.set_query(&t, query, Rows::ZERO);
    let marks = f.marks(&t, Rows::ZERO);
    let at: Vec<(WindowLine, usize, usize)> = marks
        .iter()
        .map(|m| (m.line, m.from.index(), m.to.index()))
        .collect();
    let above = WindowLine::of(Row::ZERO).up(Rows::of_count(1));
    assert_eq!(
        at,
        [
            (above, 24, 29),
            (WindowLine::of(Row::ZERO), 0, 3),
            (WindowLine::of(Row::from_index(1)), 3, 12)
        ]
    );
    let mut r = renderer(&faces);
    r.set_find(
        &t,
        Some(FindLook {
            marks,
            field: f.field(Cols::of_index(30)),
        }),
    );
    r.lay_out_bidi(&t, Rows::ZERO);
    let m = r.metrics();
    let (cw, ch) = (
        i32::try_from(m.cell_w.value()).unwrap(),
        i32::try_from(m.cell_h.value()).unwrap(),
    );
    let mut washed: Vec<(i32, i32)> = r
        .find_solids()
        .iter()
        .flat_map(|i| {
            let (y, x, n) = (
                i32::from(i.y.0) / ch,
                i32::from(i.x.0) / cw,
                i32::from(i.w.0) / cw,
            );
            (x..x + n).map(move |c| (y, c))
        })
        .collect();
    washed.sort();
    let want: Vec<(i32, i32)> = (0..10).map(|x| (0, x)).collect();
    assert_eq!(washed, want, "the first line's ten cells");
}

/// **While the window looks back, a selection reaches the row below it**
/// that its last paragraph runs on into: the same paragraph one line up,
/// cut after the sixth letter at the window's last row, is drawn whole on
/// the last line, its last four letters and " zz" from the row below the
/// window, which a selection's pointer names by that line.
#[test]
fn a_selection_reaches_the_cells_a_window_line_draws_from_below_it() {
    use baud_render::{Beyond, Reorder, WindowLine};
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = format!(
        "{}{} {} zz\r\n{PROMPT}",
        history(20),
        "X".repeat(23),
        "Y".repeat(10)
    );
    feed(&mut t, &bytes);
    // The paragraph's rows are the live screen's 7 and 8; a window three
    // lines up ends on row 7's line.
    let back = Rows::of_count(2);
    let beyond = Beyond::of(&t, back);
    assert_eq!(
        (beyond.above, beyond.below),
        (Rows::ZERO, Rows::of_count(1))
    );
    let mut o = Reorder::new();
    o.lay_out(&t, back);
    let orders = o.orders();
    let last = Row::from_index(9);
    let below = WindowLine::of(last).down(Rows::of_count(1));
    let shown: Vec<Option<(WindowLine, Col)>> = (0..13)
        .map(|x| orders.stored_line_at(last, Col::at_index(x)))
        .collect();
    assert_eq!(shown[0], Some((WindowLine::of(last), Col::at_index(24))));
    assert_eq!(shown[6], Some((below, Col::ZERO)), "{shown:?}");
    assert_eq!(
        orders.pointer_line(last, Col::at_index(7)),
        (below, Col::at_index(1))
    );
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    let top = scrolled.top(back);
    let below_line = scrolled.line(back, last).below();
    let row = t.view(Rows::ZERO).row(Row::from_index(8)).to_vec();
    let mut sel = Selection::at(Span::cell(&row, below_line, Col::ZERO), Grain::Cell);
    sel.extend(Span::cell(&row, below_line, Col::at_index(3)));
    let (cols, rows) = (Cols::of_index(30), Rows::of_index(10));
    assert_eq!(sel.highlight(top, rows, cols), None, "the control");
    let h = sel
        .highlight_reaching(top, rows, cols, beyond)
        .expect("drawn on the last line");
    let cells = o.view(&t, back).row(last).to_vec();
    let lit: Vec<bool> = (0..13)
        .map(|x| orders.selects(&h, last, Col::at_index(x), cells[x].content))
        .collect();
    let want: Vec<bool> = (0..13).map(|x| (6..10).contains(&x)).collect();
    assert_eq!(lit, want);
}

/// A frame painted over `pixels`, the last frame's, on the CPU path.
fn paint_over(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, pixels: &mut [u32], damage: Damage) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(pixels, w, w, h).unwrap();
    r.render(t, &mut target, damage);
}

/// **A change past the window repaints the line it is drawn on**: the
/// six letters above the window selected, and then marked by a find under
/// a field already drawn, over a frame drawn without either, repaint the
/// window's first line though none of the window's own rows changed, and
/// each frame painted over the last is the frame painted whole.
#[test]
fn a_change_past_the_window_repaints_the_line_it_is_drawn_on() {
    use baud_render::{Beyond, Current, Find, FindLook, Mark, WindowLine};
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &cut_at_the_top());
    let (_, top) = cut_term(&t);
    let up_line = top.above().unwrap();
    let up_row = t.view(Rows::of_count(1)).row(Row::ZERO).to_vec();
    let mut part = Selection::at(Span::cell(&up_row, up_line, Col::at_index(24)), Grain::Cell);
    part.extend(Span::cell(&up_row, up_line, Col::at_index(29)));
    let h = part.highlight_reaching(
        top,
        Rows::of_index(10),
        Cols::of_index(30),
        Beyond::of(&t, Rows::ZERO),
    );
    let mut r = renderer(&faces);
    let mut kept = paint(&mut r, &mut t);
    r.set_highlight(h);
    paint_over(&mut r, &mut t, &mut kept, Damage::Incremental);
    let mut whole = renderer(&faces);
    whole.set_highlight(h);
    assert!(
        kept == paint(&mut whole, &mut t),
        "the selection: painted over is painted whole"
    );
    r.set_highlight(None);
    paint_over(&mut r, &mut t, &mut kept, Damage::Incremental);
    // A mark of the six letters above the window alone, not the current
    // match: none of the window's own rows is marked. The field, which a
    // driver draws over the first row, on the last, so the first line's
    // cells show.
    let mut field = Find::new().field(Cols::of_index(30));
    field.row = Row::from_index(9);
    let look = FindLook {
        marks: vec![Mark {
            line: WindowLine::of(Row::ZERO).up(Rows::of_count(1)),
            from: Col::at_index(24),
            to: Col::at_index(29),
            current: Current::No,
        }],
        field,
    };
    // The field first, then the mark alone.
    let field_only = FindLook {
        marks: Vec::new(),
        ..look.clone()
    };
    r.set_find(&t, Some(field_only.clone()));
    paint_over(&mut r, &mut t, &mut kept, Damage::Incremental);
    r.set_find(&t, Some(look.clone()));
    paint_over(&mut r, &mut t, &mut kept, Damage::Incremental);
    let mut whole = renderer(&faces);
    whole.set_find(&t, Some(look));
    let washed = paint(&mut whole, &mut t);
    let mut unmarked = renderer(&faces);
    unmarked.set_find(&t, Some(field_only));
    assert!(
        washed != paint(&mut unmarked, &mut t),
        "the premise: the mark is drawn"
    );
    assert!(kept == washed, "the find: painted over is painted whole");
}

/// The reported paragraph's three parts: Hebrew, an English run the
/// terminal's wrap cuts as "mid|dle" at 44 columns, Hebrew.
const HE1: &str =
    "\u{5D6}\u{5D5} \u{5E4}\u{5E1}\u{5E7}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA} ";
const EN: &str = "with English words in the middle, ";
const HE2: &str = "\u{5E9}\u{5DE}\u{5DE}\u{5E9}\u{5D9}\u{5DB}\u{5D4} \u{5DC}\u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D4}\u{5D1}\u{5D0}\u{5D4}.";

/// A stream over window rows `from` to `to`, every column, numbered as
/// the selection's text reads it.
fn whole_rows(t: &Term<'_>, from: usize, to: usize) -> (Selection, Scrolled) {
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    let line = |r: usize| scrolled.line(Rows::ZERO, Row::from_index(r));
    let row = |r: usize| t.view(Rows::ZERO).row(Row::from_index(r)).to_vec();
    let last = Col::at_index(t.view(Rows::ZERO).cols().index() - 1);
    let mut sel = Selection::at(Span::cell(&row(from), line(from), Col::ZERO), Grain::Cell);
    sel.extend(Span::cell(&row(to), line(to), last));
    (sel, scrolled)
}

/// **`?96` copies the cut the window keeps at the top of the history**:
/// the reported paragraph on the first two lines the history keeps, at
/// four rows, would push a line where nothing scrolls to it if it were
/// re-broken, so the window draws it as the terminal cut it, and the copy
/// reads those rows, each from its right edge -- what the stored rows'
/// copy reads. The same paragraph with lines of history above it is drawn
/// at its words, and its copy reads its display lines, the control.
#[test]
fn decrlcm_copies_the_cut_the_window_keeps_at_the_top_of_the_history() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 44,
    };
    let copy = |above: u16| {
        let bytes = format!("{}{HE1}{EN}{HE2}\r\n{PROMPT}\x1b[?96h", history(above));
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, &bytes);
        let mut o = baud_render::Reorder::new();
        o.lay_out(&t, Rows::ZERO);
        let drawn = o.orders().display().is_some();
        // The paragraph's two rows sit above the prompt's.
        let prompt = usize::from(ws.ws_row) - 2 + usize::from(above.min(1));
        let (sel, scrolled) = whole_rows(&t, prompt - 2, prompt - 1);
        (
            drawn,
            sel.text_as(&t, scrolled, WordWrap::On),
            sel.text_as(&t, scrolled, WordWrap::Off),
        )
    };
    let (drawn, on, stored) = copy(0);
    assert!(!drawn, "the premise: the window keeps the cut");
    assert!(stored.contains("dim eht"), "the premise: {stored}");
    assert_eq!(on, stored, "the cut drawn is the cut copied");
    let (drawn, on, stored) = copy(4);
    assert!(drawn, "the control: re-broken with history above");
    assert_eq!(
        on,
        format!("{HE1}{}{HE2}", EN.chars().rev().collect::<String>()),
        "the control: by its display lines"
    );
    assert_ne!(on, stored);
}

/// **A block under `?96` copies the display lines as they are drawn**:
/// the block over columns 1 to 10 of the reported paragraph's two stored
/// rows selects cells the display draws on all three of its lines -- the
/// first ten of the Hebrew before the run, "dle, " at the end of the
/// English line, and the first five letters of the Hebrew after it -- and
/// the copy reads each of those lines from its right edge, a newline
/// between each two as a block breaks at every line. Over columns 41 to
/// 44 it selects " mid" on the English line alone, the Hebrew lines none
/// of their cells, and the copy is that one line. Read by the stored
/// rows, two lines, the cut "mid|dle" inside them, the control.
#[test]
fn decrlcm_reads_a_block_by_the_display_lines_it_selects() {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 44,
    };
    let bytes = format!("{}{HE1}{EN}{HE2}\r\n{PROMPT}\x1b[?96h", history(8));
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, &bytes);
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    let line = |r: usize| scrolled.line(Rows::ZERO, Row::from_index(r));
    let row = |r: usize| t.view(Rows::ZERO).row(Row::from_index(r)).to_vec();
    let mut sel = Selection::block(Span::cell(&row(3), line(3), Col::ZERO), Grain::Cell);
    sel.extend(Span::cell(&row(4), line(4), Col::at_index(9)));
    let first: String = HE1.chars().take(10).collect();
    let last: String = HE2.chars().take(5).collect();
    let want = format!("{first}\n ,eld\n{last}");
    assert_eq!(sel.text_as(&t, scrolled, WordWrap::On), want);
    let stored = sel.text_as(&t, scrolled, WordWrap::Off);
    assert_eq!(stored.lines().count(), 2, "the control: {stored}");
    assert_ne!(stored, want);
    let mut mid = Selection::block(Span::cell(&row(3), line(3), Col::at_index(40)), Grain::Cell);
    mid.extend(Span::cell(&row(4), line(4), Col::at_index(43)));
    assert_eq!(mid.text_as(&t, scrolled, WordWrap::On), "dim ");
}

/// kitty's first placeholder diacritic, U+0305: row or column 0.
const D0: char = '\u{305}';

/// A two-by-one red picture held as virtual placement 1 (`U=1`) of image
/// 1, two cells by one.
fn virtual_red() -> String {
    "\x1b_Ga=t,f=32,s=2,v=1,i=1,q=2;/wAA//8AAP8=\x1b\\\x1b_Ga=p,U=1,i=1,q=2,c=2,r=1\x1b\\"
        .to_owned()
}

/// Two placeholder cells naming it: the first at row 0 and column 0, the
/// second inheriting both from it.
fn placeholders() -> String {
    format!("\x1b[38;5;1m\u{10EEEE}{D0}{D0}\u{10EEEE}\x1b[0m")
}

/// The draw row of every picture strip a frame of the window `back`
/// lines up places, top down: -1 for the leaving row above the top.
fn strip_rows(r: &mut Renderer<'_, '_>, t: &Term<'_>, back: Rows) -> Vec<i16> {
    let ch = i16::try_from(r.metrics().cell_h.value()).unwrap();
    r.set_images(t, back);
    let mut ys: Vec<i16> = r
        .image_placements()
        .iter()
        .map(|p| p.y.div_euclid(ch))
        .collect();
    ys.sort_unstable();
    ys
}

/// **The leaving row's placeholders are the display's**: the line a
/// smooth scroll slides out above the window is the first line of the
/// window one line up as the display draws it. With a finished paragraph
/// below that the display draws on one line more, the stored row above
/// the window is pushed out of that window and its first line is "x1".
/// Placeholders on the stored row above the window then place no strip
/// on the leaving row, and placeholders on "x1", pushed out of the
/// window itself, place theirs there, one row above the top. With the
/// setting off the stored row above is the leaving row, the control.
#[test]
fn the_leaving_rows_placeholders_are_the_line_the_display_draws() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let fillers: String = (2..=7).map(|i| format!("x{i}\r\n")).collect();
    let scene = |on_above: bool| {
        let (above, x1) = match on_above {
            true => (placeholders(), "x1".to_owned()),
            false => ("up".to_owned(), format!("x1{}", placeholders())),
        };
        format!(
            "\x1b[?25l{}{}{above}\r\n{x1}\r\n{fillers}{SEVEN}\r\n{PROMPT}",
            history(10),
            virtual_red()
        )
    };
    let mut ys = Vec::new();
    for on_above in [true, false] {
        for wrap in [WordWrap::On, WordWrap::Off] {
            let mut r = renderer(&faces);
            r.set_word_wrap(wrap);
            let m = r.metrics();
            let mut owned = Owned::new(&WS).with_images(baud_domain::ImageBudget::of(4096));
            let mut t = Term::new(owned.storage(), &WS);
            t.set_cell_size(
                baud_domain::ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
                baud_domain::ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
            );
            feed(&mut t, &scene(on_above));
            ys.push(strip_rows(&mut r, &t, Rows::ZERO));
        }
    }
    assert_eq!(ys[1], vec![-1], "the control, off: the stored row above");
    assert_eq!(
        ys[0],
        Vec::<i16>::new(),
        "on: pushed out of the leaving line"
    );
    assert_eq!(
        ys[3],
        vec![0],
        "the control, off: x1 on the window's top row"
    );
    assert_eq!(ys[2], vec![-1], "on: x1 is the leaving line");
}

/// A Sixel 40 pixels wide and 6 tall, all red.
fn red_strip() -> String {
    "\x1bP0;1;0q\"1;1;40;6#0;2;100;0;0!40~\x1b\\".to_owned()
}

/// **A picture is mirrored by the line the display draws its row on**:
/// in a Hebrew paragraph at 20 columns, an English run the terminal
/// stored on two rows of its own is longer than a line, so the display
/// gives it lines of its own, left to right. A picture anchored on the
/// run's first stored row, at its first column, which the display draws
/// at the same column of the run's first line, is placed there, left to
/// right; the stored row's paragraph runs right to left, and with the
/// setting off the picture is placed mirrored by it, against the right
/// edge, the control.
#[test]
fn a_picture_is_mirrored_by_the_direction_of_its_display_line() {
    let ws = WinSize {
        ws_row: 8,
        ws_col: 20,
    };
    let he = "\u{5D0}\u{5D1}\u{5D2}\u{5D3} \u{5D4}\u{5D5}\u{5D6}\u{5D7} \u{5D8}\u{5D9}\u{5DB}\u{5DC} \u{5DE}\u{5E0}\u{5E1}\u{5E2} ";
    let en = "aaaa bbbb cccc dddd eeee ffff gggg hhhh ";
    let end = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}.";
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut placed = Vec::new();
    let mut grid_w = 0;
    for wrap in [WordWrap::On, WordWrap::Off] {
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap();
        r.set_word_wrap(wrap);
        let m = r.metrics();
        grid_w = i64::from(m.cell_w.value()) * i64::from(ws.ws_col);
        let mut owned = Owned::new(&ws).with_images(baud_domain::ImageBudget::of(1 << 16));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(
            baud_domain::ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
            baud_domain::ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
        );
        feed(&mut t, &format!("{}{he}{en}{end}\r\n{PROMPT}", history(8)));
        let vp = t.view(Rows::ZERO);
        let first = |y: usize| vp.row(Row::from_index(y))[0].content.as_codepoint();
        let run = (0..8)
            .find(|&y| first(y).is_some_and(|c| c.value() == u32::from('a')))
            .expect("the premise: the run's first stored row");
        let prompt = run + 3;
        feed(
            &mut t,
            &format!("\x1b[{};1H{}\x1b[{};3H", run + 1, red_strip(), prompt + 1),
        );
        let mut o = baud_render::Reorder::new();
        o.set_word_wrap(wrap);
        o.lay_out(&t, Rows::ZERO);
        let shown = o.view(&t, Rows::ZERO);
        let starts_run = (0..8).any(|y| {
            let row = shown.row(Row::from_index(y));
            let text: String = row
                .iter()
                .filter_map(|c| c.content.as_codepoint())
                .filter_map(|c| char::from_u32(c.value()))
                .collect();
            text.starts_with("aaaa bbbb")
        });
        assert!(
            starts_run,
            "the premise: the run's line is drawn ({wrap:?})"
        );
        r.set_images(&t, Rows::ZERO);
        let p = r.image_placements();
        assert_eq!(p.len(), 1, "the premise: one placement ({wrap:?})");
        placed.push((
            i64::from(p[0].x),
            i64::from(p[0].w),
            o.orders().display().is_some(),
        ));
    }
    let (on, off) = (placed[0], placed[1]);
    assert!(on.2, "the premise: on re-breaks the paragraph");
    assert_eq!(
        off.0,
        grid_w - off.1,
        "the control, off: mirrored by its row"
    );
    assert_eq!(on.0, 0, "on: left to right, as its display line runs");
}

/// **Beside a paragraph the window re-breaks, one it keeps at its cut
/// is copied as it is drawn**: at the top of what the history keeps, at
/// 20 columns, five English words on two rows would be drawn on three
/// lines, pushing a line where nothing scrolls to it, so the window keeps
/// their cut; the Hebrew paragraph below, whose English run takes lines
/// of its own on as many lines as it is stored on, is drawn at its words.
/// Under `?96` the first copies by its stored rows, as the window draws
/// it, and the second by its display lines, the control.
#[test]
fn decrlcm_copies_a_kept_paragraph_beside_a_re_broken_one() {
    let ws = WinSize {
        ws_row: 8,
        ws_col: 20,
    };
    let five = "aaaaaaa bbbbbbb ccccccc ddddddd eeeeeee";
    let he = "\u{5D0}\u{5D1}\u{5D2}\u{5D3} \u{5D4}\u{5D5}\u{5D6}\u{5D7} \u{5D8}\u{5D9}\u{5DB}\u{5DC} \u{5DE}\u{5E0}\u{5E1}\u{5E2} ";
    let en = "aaaa bbbb cccc dddd eeee ffff gggg hhhh ";
    let end = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}.";
    let bytes = format!("{five}\r\n{he}{en}{end}\r\n{PROMPT}\x1b[?96h");
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, &bytes);
    let mut o = baud_render::Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let d = o.orders().display().expect("the premise: a display");
    assert_eq!(d.same(Row::ZERO), Some(Row::ZERO), "the premise: kept");
    assert_eq!(d.same(Row::from_index(2)), None, "the premise: re-broken");
    let (kept, scrolled) = whole_rows(&t, 0, 1);
    assert_eq!(
        kept.text_as(&t, scrolled, WordWrap::On),
        kept.text_as(&t, scrolled, WordWrap::Off),
        "the kept paragraph: by its stored rows"
    );
    let (broken, scrolled) = whole_rows(&t, 2, 5);
    assert_ne!(
        broken.text_as(&t, scrolled, WordWrap::On),
        broken.text_as(&t, scrolled, WordWrap::Off),
        "the control: the re-broken one by its display lines"
    );
}
