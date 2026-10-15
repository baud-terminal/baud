//! **An image stays on the row it was placed on**, whatever the window does
//! to the rows around it: a shorter screen retiring them into the history,
//! a taller one pulling them back, a narrower one re-cutting the lines above
//! into more rows, a drag doing all of it at once, and a reader scrolled
//! back through the history. A placement anchors to an absolute line, and
//! every one of those moves the window over the ring or renumbers the
//! lines -- so the anchor has to move with them or the image slides off the
//! text it was drawn beside.
//!
//! **Every assertion is about text, not about line numbers.** A reflow
//! renumbers every row, so "the line went from X to Y" can hold while the
//! image sits on the wrong row. What cannot hold by accident is that the row
//! above the image still reads `MARK` -- the line printed just before it --
//! and the row below its last still reads the prompt printed just after it.
//!
//! Two traps from the survey of eight terminals are pinned by name: foot
//! maps an anchor before it flushes the blank rows ahead of it, so an image
//! after a blank line lands a row too high; WezTerm deletes the image cells
//! on a row that also holds text when that row is re-cut. Both are about
//! what sits *beside* an image, which is what a line-number test cannot see.

mod support;

use baud_core::*;
use baud_domain::{CacheGeometry, Depth, ImageBudget, ImagePx, ImageScreen, PlacementSlot};

/// The committed real capture: a 64x48 ImageMagick Sixel, three rows at a
/// sixteen-pixel cell.
const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

/// A 1x6 red Sixel, one row at a sixteen-pixel cell.
const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";

/// The image memory: several swirls with room.
fn budget() -> ImageBudget {
    ImageBudget::of(4 * (64 * 48 + 64))
}

/// A terminal at the corpus geometry with `depth` lines of history and the
/// room to be resized anywhere under it, cells 8x16 so a Sixel advances the
/// cursor the way the app's does.
fn with<R>(depth: Depth, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let ws = support::CORPUS;
    let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, depth).with_images(budget());
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    f(&mut t)
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    support::feed(t, bytes);
}

fn resize(t: &mut Term<'_>, cols: u16, rows: u16) {
    assert_eq!(
        t.resize(&WinSize {
            ws_row: rows,
            ws_col: cols
        }),
        Resized::Ok,
        "{cols}x{rows} is inside the provisioned grid"
    );
}

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Where the live screen's numbering puts the top of a view `back` lines up,
/// and which screen that numbering is. What the renderer asks, so a test
/// resolving a placement asks the same question.
fn origin(t: &Term<'_>, back: Rows) -> (i128, ImageScreen) {
    let o = t.image_origin(back);
    (i128::from(o.top.value()), o.screen)
}

/// The placements on the live screen, in slot order.
fn shown(t: &Term<'_>) -> Vec<PlacementSlot> {
    let screen = origin(t, Rows::ZERO).1;
    t.images()
        .placements()
        .map(|(_, p)| *p)
        .filter(|p| p.screen == screen)
        .collect()
}

/// The one placement on the live screen.
fn only(t: &Term<'_>) -> PlacementSlot {
    let s = shown(t);
    assert_eq!(s.len(), 1, "exactly one image on the live screen");
    s[0]
}

/// The text of the row `delta` rows from `p`'s anchor, wherever that row
/// is: on the live screen, or behind it in the history, or `None` when it
/// is past either. Resolved through the same origin the renderer uses, so
/// a placement and its drawn row cannot disagree here without the test
/// saying so.
fn beside(t: &Term<'_>, p: &PlacementSlot, delta: i128) -> Option<String> {
    let (top, _) = origin(t, Rows::ZERO);
    let line = i128::from(p.line.value()) + delta;
    let rows = i128::try_from(t.geometry().1.index()).unwrap();
    if line >= top && line < top + rows {
        let y = usize::try_from(line - top).unwrap();
        return Some(text_of(
            t.row(Row::from_index(y)).expect("a row on the grid"),
        ));
    }
    if line < top {
        let n = usize::try_from(top - 1 - line).unwrap();
        return t.scrollback().line(Back::of(n)).map(|(c, _)| text_of(c));
    }
    None
}

/// The screen row `p` draws at in a view `back` lines up, signed: negative
/// is above the view's top.
fn drawn_row(t: &Term<'_>, p: &PlacementSlot, back: Rows) -> i128 {
    i128::from(p.line.value()) - origin(t, back).0
}

/// A line of `n` digits, long enough to re-wrap when the screen narrows.
fn long(n: usize) -> String {
    (0..n).map(|i| char::from(b"0123456789"[i % 10])).collect()
}

/// Lines above the image that a narrower screen re-cuts into more rows,
/// then `MARK` on its own line, then the image, then a prompt.
fn scene(t: &mut Term<'_>, above: &[u8]) {
    feed(t, above);
    feed(t, b"MARK\r\n");
    feed(t, SWIRL);
    feed(t, b"\r$ ");
}

/// The image sits under `MARK` and above the prompt, which is the whole
/// claim of this file in one assertion.
fn in_place(t: &Term<'_>, why: &str) {
    let p = only(t);
    assert_eq!(
        beside(t, &p, -1).as_deref(),
        Some("MARK"),
        "{why}: the row above the image is the line printed before it"
    );
    assert_eq!(
        beside(t, &p, 0).as_deref(),
        Some(""),
        "{why}: the image's own row holds no text"
    );
    assert_eq!(
        beside(t, &p, 3).as_deref(),
        Some("$"),
        "{why}: the row after the image's three is the prompt"
    );
}

fn lines(n: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..n {
        v.extend(format!("t{i:02}\r\n").into_bytes());
    }
    v
}

#[test]
fn a_shorter_screen_keeps_the_image_on_its_row() {
    with(Depth::DEFAULT, |t| {
        scene(t, &lines(30));
        in_place(t, "as drawn");
        // The cursor is on row 34 of 50; a 20-row screen cuts the 15 rows
        // under it and retires 15 from the top, the image's rows among
        // the ones that move up.
        resize(t, 200, 20);
        in_place(t, "after a shrink that retired rows");
        resize(t, 200, 50);
        in_place(t, "after the growth that pulled them back");
    });
}

#[test]
fn a_shrink_that_pushes_the_image_into_the_history_keeps_it_there() {
    with(Depth::DEFAULT, |t| {
        // History first, so the absolute numbering is well past zero and a
        // mistake cannot hide behind a count that happens to be zero.
        feed(t, &[b'\n'; 60]);
        feed(t, b"\x1b[1;1H");
        scene(t, b"");
        feed(t, b"\x1b[50;1H$ ");
        // The cursor on the last row: every row of the shrink is retired,
        // the image's with them.
        resize(t, 200, 20);
        let p = only(t);
        assert_eq!(beside(t, &p, -1).as_deref(), Some("MARK"));
        // And a reader scrolled back to it finds the image beside `MARK`:
        // the view's top is 30 lines up, and the image is one row below it.
        let back = Rows::of_count(30);
        assert_eq!(drawn_row(t, &p, back), 1, "one row into the view 30 up");
        // Asked for further back than the history holds, the view stops at
        // the oldest line -- and the image's row is counted from the same
        // stop, not from where the request pointed.
        let far = Rows::of_count(10_000);
        let y = usize::try_from(drawn_row(t, &p, far)).unwrap();
        assert_eq!(text_of(t.view(far).row(Row::from_index(y - 1))), "MARK");
        assert_eq!(
            text_of(t.view(back).row(Row::from_index(0))),
            "MARK",
            "the row above it in that view is MARK"
        );
    });
}

#[test]
fn a_view_asked_for_past_the_history_draws_the_image_from_where_it_stopped() {
    // A shallow history that has overflowed, so its oldest line is not the
    // first line ever numbered -- the case where "further back than the
    // history" and "back to line zero" are different rows, and an origin
    // that did not clamp as the view clamps would put the image off its row.
    with(Depth::of(20), |t| {
        feed(t, &lines(100));
        feed(t, b"\x1b[1;1H");
        scene(t, b"");
        feed(t, b"\x1b[50;1H\n\n\n\n\n");
        let p = only(t);
        let far = Rows::of_count(10_000);
        let y = drawn_row(t, &p, far);
        assert!(y > 0, "the image is below the view's top, not at {y}");
        let y = usize::try_from(y).unwrap();
        assert!(y < 50, "and on the view, not at row {y}");
        assert_eq!(text_of(t.view(far).row(Row::from_index(y - 1))), "MARK");
    });
}

#[test]
fn a_narrower_screen_keeps_the_image_under_its_line() {
    with(Depth::DEFAULT, |t| {
        let mut above = Vec::new();
        for _ in 0..5 {
            above.extend(long(150).into_bytes());
            above.extend_from_slice(b"\r\n");
        }
        scene(t, &above);
        in_place(t, "as drawn");
        // Each 150-cell line becomes three rows at 60 columns, so every
        // row after them moves down ten.
        resize(t, 60, 50);
        in_place(t, "at 60 columns");
        // And back: a line-start anchor round-trips exactly.
        resize(t, 200, 50);
        in_place(t, "back at 200 columns");
        assert_eq!(only(t).col, Col::ZERO, "the column is kept");
    });
}

#[test]
fn an_image_after_a_blank_line_is_not_mapped_a_row_too_high() {
    // foot's trap: its reflow maps an anchor before it flushes the blank
    // rows pending ahead of it, so `echo; img2sixel` lands a row high.
    with(Depth::DEFAULT, |t| {
        let mut above = Vec::new();
        for _ in 0..4 {
            above.extend(long(150).into_bytes());
            above.extend_from_slice(b"\r\n");
        }
        above.extend_from_slice(b"prompt\r\n\r\n");
        feed(t, &above);
        feed(t, SWIRL);
        let check = |t: &Term<'_>, why: &str| {
            let p = only(t);
            assert_eq!(beside(t, &p, 0).as_deref(), Some(""), "{why}: anchor row");
            assert_eq!(
                beside(t, &p, -1).as_deref(),
                Some(""),
                "{why}: the blank line"
            );
            assert_eq!(
                beside(t, &p, -2).as_deref(),
                Some("prompt"),
                "{why}: the line before the blank one"
            );
        };
        check(t, "as drawn");
        resize(t, 60, 50);
        check(t, "at 60 columns");
        resize(t, 200, 50);
        check(t, "back at 200");
    });
}

#[test]
fn an_image_beside_text_on_its_row_is_kept_whole() {
    // WezTerm's trap: `ab` then an image on the same row, re-cut narrower,
    // truncates the row after its text and the image cells with it, and a
    // widen cannot bring them back. Here an image is an overlay on the row,
    // so it is cropped while the row is narrow and whole when it is wide.
    with(Depth::DEFAULT, |t| {
        let mut above = Vec::new();
        for _ in 0..4 {
            above.extend(long(150).into_bytes());
            above.extend_from_slice(b"\r\n");
        }
        feed(t, &above);
        feed(t, b"ab");
        feed(t, SWIRL);
        let p = only(t);
        assert_eq!(p.col, Col::at_index(2), "placed after the text");
        assert_eq!(beside(t, &p, 0).as_deref(), Some("ab"));
        // Wide enough for the text, not for the image: cropped at draw,
        // kept whole in the store.
        resize(t, 5, 50);
        let p = only(t);
        assert_eq!(
            beside(t, &p, 0).as_deref(),
            Some("ab"),
            "the row is its row"
        );
        assert_eq!(p.col, Col::at_index(2), "the column is kept");
        // Narrower than the text: the line re-cuts to `a` / `b`, and the
        // image stays on the row holding the line's first cell.
        resize(t, 1, 50);
        let p = only(t);
        assert_eq!(beside(t, &p, 0).as_deref(), Some("a"));
        assert_eq!(beside(t, &p, 1).as_deref(), Some("b"));
        resize(t, 200, 50);
        let p = only(t);
        assert_eq!(beside(t, &p, 0).as_deref(), Some("ab"), "whole again");
        assert_eq!(p.col, Col::at_index(2));
        let v = t
            .images()
            .image(p.image)
            .expect("the image is still stored");
        assert_eq!((v.width.raw(), v.height.raw()), (64, 48));
    });
}

#[test]
fn an_anchor_on_a_wrapped_row_comes_back_on_its_row_after_a_round_trip() {
    // An image placed after text that wrapped onto its row anchors to a
    // *continuation* row: at 200 columns that row begins at ordinal 200 of
    // the line. Narrowed to 60 the anchor goes to the row holding ordinal
    // 200, row 3 of the line, which begins at 180. Mapped again by that
    // row's own first cell, the widen brought it back on row 0, one higher;
    // the anchor keeps where ordinal 200 sits in its row, so it comes back
    // on row 1. A line-start anchor -- every image printed after a newline,
    // which is what `img2sixel` and yazi do -- takes the same rule at
    // ordinal 0 (`a_narrower_screen_keeps_the_image_under_its_line`).
    with(Depth::DEFAULT, |t| {
        feed(t, long(250).as_bytes());
        feed(t, SWIRL);
        let p = only(t);
        let tail = long(250)[200..].to_string();
        assert_eq!(beside(t, &p, 0).as_deref(), Some(tail.as_str()));
        resize(t, 60, 50);
        let p = only(t);
        assert_eq!(
            beside(t, &p, 0).as_deref(),
            Some(&long(250)[180..240]),
            "narrowed: on the row that holds the row's first cell"
        );
        resize(t, 200, 50);
        let p = only(t);
        assert_eq!(
            beside(t, &p, 0).as_deref(),
            Some(tail.as_str()),
            "widened back: on the row it was placed on"
        );
        assert_eq!(beside(t, &p, -1).as_deref(), Some(long(200).as_str()));
    });
}

/// The rows beside the image, `-1` and `0`, after `bytes` and an image after
/// them were written at `start` columns and the screen taken through
/// `widths` in turn.
fn after_widths(start: u16, bytes: &[u8], widths: &[u16]) -> (Option<String>, Option<String>) {
    with(Depth::DEFAULT, |t| {
        resize(t, start, 50);
        feed(t, bytes);
        feed(t, SWIRL);
        for &w in widths {
            resize(t, w, 50);
        }
        let p = only(t);
        (beside(t, &p, -1), beside(t, &p, 0))
    })
}

#[test]
fn an_anchor_on_a_wrapped_row_lands_by_the_width_not_by_the_path() {
    // **Where the image is depends on the width, not on the widths before
    // it.** Each path ends where one resize straight from the width the
    // image was placed at ends. Placed at 150 columns after 190 digits, the
    // anchor row begins at ordinal 150; 150 -> 60 -> 170 is the case a rule
    // by the narrow row's *last* cell would get wrong: ordinal 150 is on
    // row 0 at 170 columns, and the row 60 columns made of 120..179 ends on
    // row 1.
    let line = long(190);
    for path in [
        &[60, 170][..],
        &[60, 150],
        &[37, 90, 150],
        &[61, 133, 47, 200],
        &[200, 60, 149],
    ] {
        let last = *path.last().unwrap();
        assert_eq!(
            after_widths(150, line.as_bytes(), path),
            after_widths(150, line.as_bytes(), &[last]),
            "through {path:?}"
        );
    }
}

#[test]
fn an_anchor_on_a_wrapped_row_of_wide_characters_round_trips() {
    // The same through the blanks a re-cut leaves where a wide character
    // cannot take a row's last column: 125 distinct ideographs are 250
    // columns, the image after them anchors to the row that begins with the
    // 101st, and an odd width pushes each row's last ideograph on. Not held
    // against one straight resize, as the digits are: a widen keeps those
    // blanks in the line, so the text itself takes the path with it.
    let line: String = (0..125u32)
        .map(|i| char::from_u32(0x4E00 + i).unwrap())
        .collect();
    let first = char::from_u32(0x4E00 + 100).unwrap();
    for path in [&[61, 200][..], &[39, 101, 200], &[61, 150], &[47, 61]] {
        let (above, row) = after_widths(200, line.as_bytes(), path);
        assert!(
            row.as_deref().is_some_and(|r| r.contains(first)),
            "{path:?}: on the row that holds the 101st ideograph: {row:?}"
        );
        assert!(
            !above.as_deref().is_some_and(|r| r.contains(first)),
            "{path:?}: and not under it: {above:?}"
        );
    }
}

#[test]
fn a_drag_through_many_sizes_keeps_the_image_on_its_row() {
    // The shape a window drag has: width and height together, down and
    // back up, a step at a time, and the image under `MARK` at every step
    // its row is retained.
    with(Depth::DEFAULT, |t| {
        let mut above = Vec::new();
        for i in 0..12 {
            above.extend(long(40 + 13 * i).into_bytes());
            above.extend_from_slice(b"\r\n");
        }
        scene(t, &above);
        in_place(t, "as drawn");
        let mut steps: Vec<(u16, u16)> = Vec::new();
        for k in 0..20u16 {
            steps.push((200 - 8 * k, 50 - 2 * k));
        }
        for k in (0..20u16).rev() {
            steps.push((200 - 8 * k, 50 - 2 * k));
        }
        for (i, (c, r)) in steps.into_iter().enumerate() {
            resize(t, c, r);
            let p = only(t);
            // `MARK` and `$ ` are short, so they never re-wrap; the lines
            // above them do.
            assert_eq!(
                beside(t, &p, -1).as_deref(),
                Some("MARK"),
                "step {i} at {c}x{r}: MARK above the image"
            );
            assert_eq!(
                beside(t, &p, 3).as_deref(),
                Some("$"),
                "step {i} at {c}x{r}: the prompt after it"
            );
        }
        in_place(t, "back at the start");
    });
}

#[test]
fn an_image_in_the_history_follows_a_narrower_screen() {
    with(Depth::DEFAULT, |t| {
        let mut above = Vec::new();
        for _ in 0..3 {
            above.extend(long(150).into_bytes());
            above.extend_from_slice(b"\r\n");
        }
        scene(t, &above);
        // Push the image and its lines well into the history.
        feed(t, b"\r\n");
        feed(t, &lines(80));
        in_place(t, "scrolled into the history");
        resize(t, 60, 50);
        in_place(t, "narrowed while in the history");
        resize(t, 200, 50);
        in_place(t, "widened back");
    });
}

/// A window held over the history on an image's row, and the image, follow
/// the same re-cut: the window's first row and the image's row are mapped by
/// one hook, and the image stays at the top of the window that was showing
/// it. Lines above both re-cut into more rows, which is what moved a window
/// anchored by its top line's number onto another line.
#[test]
fn a_window_held_on_an_image_follows_it_through_a_re_cut() {
    with(Depth::DEFAULT, |t| {
        let mut above = Vec::new();
        for _ in 0..3 {
            above.extend(long(150).into_bytes());
            above.extend_from_slice(b"\r\n");
        }
        scene(t, &above);
        feed(t, b"\r\n");
        feed(t, &lines(80));
        let p = only(t);
        let mut back = (1..=t.scrollback().len())
            .map(Rows::of_count)
            .find(|&b| drawn_row(t, &p, b) == 0)
            .expect("a view with the image at its top");
        for (cols, why) in [
            (60, "narrowed"),
            (200, "widened back"),
            (37, "narrowed again"),
        ] {
            t.set_window_back(back);
            let r = t.resize(&WinSize {
                ws_row: 50,
                ws_col: cols,
            });
            let held = t.window_back();
            assert_eq!(r, Resized::Ok);
            in_place(t, why);
            assert_eq!(
                drawn_row(t, &only(t), held),
                0,
                "{why}: the image is at the top of the window that was showing it"
            );
            back = held;
        }
    });
}

/// The shell's prompt block after the image: four lines opened by the
/// initial-prompt mark, the shape zsh redraws on every resize.
const BLOCK: &[u8] = b"\x1b]133;P;k=i\x07line one\r\nline two\r\nline three\r\n$ ";

/// What zsh sends on `SIGWINCH` at a two-row window: up the block's rows
/// (clamped at the top), erase below, and the block again.
const REDRAW: &[u8] = b"\r\x1b[3A\x1b[J\x1b]133;P;k=i\x07line one\r\nline two\r\nline three\r\n$ ";

#[test]
fn an_image_above_a_prompt_the_shell_redraws_keeps_its_row() {
    // The owner's own path: zsh marks its prompt, and a window too short for
    // the block pushes its first rows into the history; the redraw mark then
    // moves the window back over them (`prompt.rs`). That move is the window
    // retreating without a reverse scroll, so the image above the prompt has
    // to move with it -- and a width change while the prompt zone is open
    // re-cuts nothing, which must leave the image alone too.
    with(Depth::DEFAULT, |t| {
        feed(t, b"MARK\r\n");
        feed(t, SWIRL);
        feed(t, b"\r");
        feed(t, BLOCK);
        let check = |t: &Term<'_>, why: &str| {
            let p = only(t);
            assert_eq!(beside(t, &p, -1).as_deref(), Some("MARK"), "{why}");
            assert_eq!(
                beside(t, &p, 3).as_deref(),
                Some("line one"),
                "{why}: the block starts after the image's three rows"
            );
        };
        check(t, "as drawn");
        resize(t, 200, 2);
        check(t, "a two-row window, the image and the block's top retired");
        let erased = t.prompt_erases;
        for _ in 0..3 {
            feed(t, REDRAW);
        }
        assert_eq!(
            t.prompt_erases,
            erased + 3,
            "three redraws replaced the block"
        );
        check(t, "after three redraws that moved the window back");
        resize(t, 120, 2);
        check(t, "a width change inside the prompt zone");
        resize(t, 120, 50);
        check(t, "grown back");
    });
}

#[test]
fn an_alternate_screen_image_scrolls_with_its_text() {
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[?1049h\x1b[10;1HALT\r\n");
        feed(t, SWIRL);
        feed(t, b"\x1b[50;1H");
        let p = only(t);
        assert_eq!(beside_alt(t, &p, -1).as_deref(), Some("ALT"));
        // Five scrolls on the alternate screen: its text moves up five.
        feed(t, b"\n\n\n\n\n");
        let p = only(t);
        assert_eq!(
            beside_alt(t, &p, -1).as_deref(),
            Some("ALT"),
            "the image moved up with the text"
        );
    });
}

/// `beside` on the alternate screen, which keeps no history.
fn beside_alt(t: &Term<'_>, p: &PlacementSlot, delta: i128) -> Option<String> {
    let (top, screen) = origin(t, Rows::ZERO);
    assert_eq!(screen, ImageScreen::Alternate);
    let y = i128::from(p.line.value()) + delta - top;
    let rows = i128::try_from(t.geometry().1.index()).unwrap();
    (0..rows).contains(&y).then(|| {
        text_of(
            t.row(Row::from_index(usize::try_from(y).unwrap()))
                .expect("a row on the grid"),
        )
    })
}

#[test]
fn each_screen_draws_only_its_own_images_and_the_alternates_go_with_it() {
    with(Depth::DEFAULT, |t| {
        scene(t, b"");
        assert_eq!(shown(t).len(), 1, "the shell's image");
        feed(t, b"\x1b[?1049h\x1b[5;1H");
        assert_eq!(
            origin(t, Rows::ZERO).1,
            ImageScreen::Alternate,
            "the alternate screen is up"
        );
        assert!(shown(t).is_empty(), "the shell's image is not the editor's");
        feed(t, SWIRL);
        assert_eq!(shown(t).len(), 1, "the editor's own image");
        assert_eq!(t.images().placements().count(), 2, "both are stored");
        let expired = t.images().placements_expired;
        feed(t, b"\x1b[?1049l");
        assert_eq!(origin(t, Rows::ZERO).1, ImageScreen::Primary);
        assert_eq!(
            t.images().placements().count(),
            1,
            "leaving the alternate screen deletes its images"
        );
        assert_eq!(t.images().placements_expired, expired + 1, "and says so");
        in_place(t, "the shell's image is back where it was");
    });
}

#[test]
fn images_whose_rows_leave_the_history_free_their_slots() {
    // Ten lines of history. Each image scrolls past it before the next is
    // placed, so there is never more than one retained -- and a store that
    // kept a placement after its row left would fill all its slots and
    // refuse the rest. More images than slots, so the refusal cannot hide.
    with(Depth::of(10), |t| {
        let n = 80;
        for _ in 0..n {
            feed(t, TINY);
            feed(t, &[b'\n'; 70]);
        }
        let imgs = t.images();
        assert_eq!(imgs.placements_refused, 0, "no image was refused");
        assert_eq!(imgs.images_refused, 0);
        assert_eq!(
            imgs.placements_expired, n,
            "every image expired as its row left the history"
        );
        assert_eq!(imgs.placements().count(), 0);
    });
}

#[test]
fn a_reverse_scroll_that_blanks_an_image_row_drops_the_image() {
    // `RI` at the top of the primary takes the newest history line back
    // onto the screen and blanks it. An image anchored on that line has
    // lost its row.
    with(Depth::DEFAULT, |t| {
        feed(t, TINY);
        // One scroll: the image's row, the top one, is the newest in the
        // history now.
        feed(t, b"\x1b[50;1H\n");
        assert_eq!(drawn_row(t, &only(t), Rows::ZERO), -1);
        let before = t.images().placements_expired;
        feed(t, b"\x1b[1;1H\x1bM");
        assert_eq!(t.images().placements().count(), 0, "its row was blanked");
        assert_eq!(t.images().placements_expired, before + 1);
    });
}

#[test]
fn a_reverse_scroll_on_a_fresh_terminal_moves_the_image_down() {
    // The numbering cannot go below zero, and `RI` at the top of a screen
    // with no history moves the window back over a row that holds nothing:
    // the image must move down one row with its text.
    with(Depth::DEFAULT, |t| {
        scene(t, b"");
        feed(t, b"\x1b[1;1H\x1bM");
        in_place(t, "after a reverse scroll with no history");
        assert_eq!(drawn_row(t, &only(t), Rows::ZERO), 2);
    });
}

// ---- rows moving inside the screen: regions, IL and DL -----------------
//
// A scrolling region, `IL` and `DL` move rows *inside* the window rather
// than moving the window over the ring, so the top line does not change
// and an anchor has to be moved with its row by name. A row one of them
// deletes -- scrolled off a region's top that is not the screen's, pushed
// past a region's bottom -- takes its image with it, the rule the rest of
// this file already holds for the history.

/// The text of the row `delta` from `p`'s anchor on whichever screen is
/// up: through the history on the primary, the window alone on the
/// alternate, which keeps none.
fn near(t: &Term<'_>, p: &PlacementSlot, delta: i128) -> Option<String> {
    match origin(t, Rows::ZERO).1 {
        ImageScreen::Primary => beside(t, p, delta),
        ImageScreen::Alternate => beside_alt(t, p, delta),
    }
}

/// A one-row image at 1-based `row`, `TAG-ABOVE` on the row before it and
/// `TAG-BELOW` on the row after. The Sixel's advance moves the cursor down
/// through `index()`, so `row` must be neither a region's bottom nor the
/// screen's last row, or placing the image would itself scroll.
fn tagged(t: &mut Term<'_>, row: usize, tag: &str) {
    feed(
        t,
        format!("\x1b[{};1H{tag}-ABOVE\x1b[{row};1H", row - 1).as_bytes(),
    );
    feed(t, TINY);
    feed(t, format!("\x1b[{};1H{tag}-BELOW", row + 1).as_bytes());
}

/// The one live image between `TAG-ABOVE` and `TAG-BELOW`, its own row
/// blank. Panics naming the rows every image sits between when none does.
fn between(t: &Term<'_>, tag: &str, why: &str) -> PlacementSlot {
    let (above, below) = (format!("{tag}-ABOVE"), format!("{tag}-BELOW"));
    let hit: Vec<PlacementSlot> = shown(t)
        .into_iter()
        .filter(|p| {
            near(t, p, -1).as_deref() == Some(above.as_str())
                && near(t, p, 1).as_deref() == Some(below.as_str())
        })
        .collect();
    let seen: Vec<(Option<String>, Option<String>)> = shown(t)
        .iter()
        .map(|p| (near(t, p, -1), near(t, p, 1)))
        .collect();
    assert_eq!(
        hit.len(),
        1,
        "{why}: one image between {above} and {below}; the live images sit between {seen:?}"
    );
    assert_eq!(
        near(t, &hit[0], 0).as_deref(),
        Some(""),
        "{why}: the image's own row holds no text"
    );
    hit[0]
}

/// A one-row image at 1-based `row` with `LABEL` at column 5 of its own
/// row. The cell size is zeroed first so placing it moves no cursor, which
/// is what lets it sit on a band's edge rows: `tagged` cannot, because one
/// of its tags would lie outside the band and stay behind while the image
/// correctly moves, so `between` would fail on correct code.
fn labelled(t: &mut Term<'_>, row: usize, label: &str) {
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
    feed(t, format!("\x1b[{row};1H").as_bytes());
    feed(t, TINY);
    feed(t, format!("\x1b[{row};5H{label}").as_bytes());
}

/// The 0-based screen row of the one live image whose own row reads
/// `LABEL` at column 5, or `None` when no image sits beside it.
fn row_of(t: &Term<'_>, label: &str) -> Option<i128> {
    let want = format!("    {label}");
    let hits: Vec<i128> = shown(t)
        .iter()
        .filter(|p| near(t, p, 0).as_deref() == Some(want.as_str()))
        .map(|p| drawn_row(t, p, Rows::ZERO))
        .collect();
    assert!(hits.len() <= 1, "{label} is beside {} images", hits.len());
    hits.first().copied()
}

#[test]
fn an_image_under_a_top_region_stays_on_its_row_while_the_region_scrolls() {
    // `DECSTBM 1 ; 40` on fifty rows: a region anchored at the top with ten
    // rows under it, which scrolls by rotating the whole grid and copying
    // the ten back. The image under it must stay with its copied-back text;
    // the one inside it moves up with its own, which it did before.
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[1;40r");
        tagged(t, 20, "IN");
        tagged(t, 44, "UNDER");
        feed(t, b"\x1b[40;1H\n\n\n\n\n");
        feed(t, b"\x1b[3S");
        between(t, "IN", "inside the region, eight scrolls on");
        let p = between(t, "UNDER", "under the region, eight scrolls on");
        assert_eq!(drawn_row(t, &p, Rows::ZERO), 43, "the row it was placed on");
        assert_eq!(t.images().placements_expired, 0);
    });
}

#[test]
fn an_alternate_screen_image_under_a_top_region_stays_on_its_row() {
    // vim's shape: the region is set on the alternate screen. History on
    // the primary first, so the two screens' top lines differ and an image
    // moved against the wrong one lands somewhere else.
    with(Depth::DEFAULT, |t| {
        feed(t, &[b'\n'; 70]);
        feed(t, b"\x1b[?1049h\x1b[1;40r");
        tagged(t, 20, "IN");
        tagged(t, 44, "UNDER");
        feed(t, b"\x1b[40;1H\n\n\n\n\n\x1b[3S");
        assert_eq!(origin(t, Rows::ZERO).1, ImageScreen::Alternate);
        between(t, "IN", "inside the region, eight scrolls on");
        let p = between(t, "UNDER", "under the region, eight scrolls on");
        assert_eq!(drawn_row(t, &p, Rows::ZERO), 43, "the row it was placed on");
    });
}

#[test]
fn an_image_in_a_middle_region_moves_with_its_text_and_its_neighbours_do_not() {
    // `DECSTBM 10 ; 30`: a region that copies its rows. The images on the
    // rows either side of it -- 9 and 31 -- catch a band too wide; the
    // edge-row tests below catch one too narrow.
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[10;30r");
        tagged(t, 16, "MID");
        feed(t, b"\x1b[8;1HEDGE-ABOVE\x1b[9;1H");
        feed(t, TINY);
        feed(t, b"\x1b[31;1H");
        feed(t, TINY);
        feed(t, b"\x1b[32;1HEDGE-BELOW");
        feed(t, b"\x1b[30;1H\n\n\n\x1b[2S");
        between(t, "MID", "five region scrolls on");
        let rows: Vec<i128> = shown(t)
            .iter()
            .map(|p| drawn_row(t, p, Rows::ZERO))
            .collect();
        assert!(
            rows.contains(&8),
            "the image above the region stayed: {rows:?}"
        );
        assert!(
            rows.contains(&30),
            "the image below the region stayed: {rows:?}"
        );
        assert_eq!(
            text_of(t.row(Row::from_index(7)).expect("a row on the grid")),
            "EDGE-ABOVE"
        );
        assert_eq!(
            text_of(t.row(Row::from_index(31)).expect("a row on the grid")),
            "EDGE-BELOW"
        );
        assert_eq!(t.images().placements_expired, 0);
    });
}

#[test]
fn an_image_scrolled_off_a_middle_regions_top_goes_with_its_row() {
    // The rows that leave the top of a region not at the screen's top are
    // deleted, not retired to the history: the image on one goes too.
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[10;30r");
        tagged(t, 11, "GONE");
        tagged(t, 20, "KEPT");
        let before = t.images().placements_expired;
        feed(t, b"\x1b[3S");
        assert_eq!(
            t.images().placements_expired,
            before + 1,
            "its row was deleted"
        );
        assert_eq!(shown(t).len(), 1);
        between(t, "KEPT", "after three region scrolls");
    });
}

#[test]
fn a_reverse_scroll_in_a_middle_region_carries_the_image_down_and_pushes_one_out() {
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[10;30r");
        tagged(t, 16, "MID");
        tagged(t, 28, "LOW");
        let before = t.images().placements_expired;
        // `RI` at the region's top, then `SD 2`: three rows down.
        feed(t, b"\x1b[10;1H\x1bM\x1b[2T");
        between(t, "MID", "three rows down");
        assert_eq!(
            t.images().placements_expired,
            before + 1,
            "row 28 pushed past the region's bottom took its image"
        );
        assert_eq!(shown(t).len(), 1);
    });
}

#[test]
fn il_and_dl_carry_an_image_with_its_line() {
    with(Depth::DEFAULT, |t| {
        tagged(t, 5, "HIGH");
        tagged(t, 20, "MID");
        tagged(t, 48, "LOW");
        // IL 3 at row 10: everything from row 10 down moves three, and
        // row 48 is pushed off the screen's bottom.
        feed(t, b"\x1b[10;1H\x1b[3L");
        between(t, "HIGH", "above the insertion");
        let p = between(t, "MID", "after IL 3");
        assert_eq!(drawn_row(t, &p, Rows::ZERO), 22);
        assert_eq!(
            t.images().placements_expired,
            1,
            "row 48 went off the bottom"
        );
        // DL 2 at row 10 brings it back two.
        feed(t, b"\x1b[10;1H\x1b[2M");
        let p = between(t, "MID", "after DL 2");
        assert_eq!(drawn_row(t, &p, Rows::ZERO), 20);
        // DL on the image's own row deletes the row it was placed on.
        feed(t, b"\x1b[21;1H\x1b[M");
        assert_eq!(t.images().placements_expired, 2, "its row was deleted");
        between(t, "HIGH", "a DL below it");
    });
}

#[test]
fn an_image_goes_where_a_top_regions_scrolled_line_goes() {
    // A line leaving a region anchored at the top goes to the history on the
    // primary whatever the region's height, and the image on it goes with
    // it. `1 ; 3` used to copy and delete the line where `1 ; 40` rotated and
    // retired it; both retire now. The alternate screen keeps no history, so
    // there the line and its image are deleted, which is the control.
    for region in [b"\x1b[1;40r".as_slice(), b"\x1b[1;3r".as_slice()] {
        with(Depth::DEFAULT, |t| {
            feed(t, region);
            feed(t, b"\x1b[1;1H");
            feed(t, TINY);
            feed(t, b"\x1b[1;5HTOP\x1b[2;1HNEXT\x1b[S");
            let p = only(t);
            assert_eq!(drawn_row(t, &p, Rows::ZERO), -1, "retired with its line");
            assert_eq!(beside(t, &p, 0).as_deref(), Some("    TOP"));
            assert_eq!(t.images().placements_expired, 0);
        });
    }
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[?1049h\x1b[1;3r\x1b[1;1H");
        feed(t, TINY);
        feed(t, b"\x1b[1;5HTOP\x1b[2;1HNEXT\x1b[S");
        assert_eq!(shown(t).len(), 0, "deleted with its line");
        assert_eq!(t.images().placements_expired, 1);
    });
}

#[test]
fn an_image_on_a_status_line_under_a_top_region_stays_there() {
    // The shape vim sets: `DECSTBM 1 ; 49` on fifty rows, the status line
    // the one row under the region. (`less` sets no region at all: it
    // scrolls the whole screen and redraws its prompt.) It is the row the repair
    // puts back first, so it is the edge of the band that moves. No cell
    // size, so the Sixel does not advance the cursor: from the last row an
    // advance would scroll the whole screen, which is not what is under
    // test here.
    with(Depth::DEFAULT, |t| {
        t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
        feed(t, b"\x1b[1;49r\x1b[50;1H");
        feed(t, TINY);
        feed(t, b"\x1b[50;5HSTATUS\x1b[49;1H\n\n\n\x1b[2S");
        let p = only(t);
        assert_eq!(drawn_row(t, &p, Rows::ZERO), 49, "still on the status line");
        assert_eq!(beside(t, &p, 0).as_deref(), Some("    STATUS"));
    });
}

// ---- the band's own edge rows ------------------------------------------
//
// The tests above put images inside a band and on the rows either side of
// it, which catches a band too wide. These put them on the band's first
// and last rows and on the screen's last row, which catches one too narrow
// or ending in the wrong place: the bound each call site passes is part of
// the band, and an off-by-one there moves text and leaves an image.

#[test]
fn il_and_dl_inside_a_region_move_the_edge_rows_and_leave_the_status_line() {
    // `DECSTBM 1 ; 49`, the status line on row 50 under it. IL and DL at
    // row 10 move rows 10..49 -- the region's bottom, not the screen's --
    // so the status line stays, the region's last row is pushed out by IL,
    // and the cursor's own row moves with the insertion.
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[1;49r");
        labelled(t, 9, "ABOVE");
        labelled(t, 10, "OWN");
        labelled(t, 20, "MID");
        labelled(t, 49, "EDGE");
        labelled(t, 50, "STATUS");
        feed(t, b"\x1b[10;1H\x1b[L");
        assert_eq!(row_of(t, "ABOVE"), Some(8), "above the insertion");
        assert_eq!(row_of(t, "OWN"), Some(10), "IL's own row moved down");
        assert_eq!(row_of(t, "MID"), Some(20));
        assert_eq!(row_of(t, "EDGE"), None, "pushed past the region's bottom");
        assert_eq!(t.images().placements_expired, 1, "with its image");
        assert_eq!(row_of(t, "STATUS"), Some(49), "under the region");
        // DL at row 10 brings them back up, and the region's last row with
        // them.
        labelled(t, 49, "EDGE2");
        feed(t, b"\x1b[10;1H\x1b[M");
        assert_eq!(row_of(t, "ABOVE"), Some(8), "above the deletion");
        assert_eq!(row_of(t, "OWN"), Some(9));
        assert_eq!(row_of(t, "MID"), Some(19));
        assert_eq!(row_of(t, "EDGE2"), Some(47), "up from the region's bottom");
        assert_eq!(row_of(t, "STATUS"), Some(49), "under the region");
        assert_eq!(t.images().placements_expired, 1);
    });
}

#[test]
fn a_copied_regions_first_and_last_rows_move_by_the_rule() {
    // `DECSTBM 10 ; 30`, which copies. `SU` deletes the region's first row
    // and moves its last up one; `RI` at the top moves the first down one
    // and pushes the last out. The rows either side stay put both times.
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[10;30r");
        labelled(t, 9, "OVER");
        labelled(t, 10, "TOP");
        labelled(t, 30, "BOT");
        labelled(t, 31, "UNDER");
        feed(t, b"\x1b[S");
        assert_eq!(row_of(t, "OVER"), Some(8));
        assert_eq!(row_of(t, "TOP"), None, "deleted off the region's top");
        assert_eq!(row_of(t, "BOT"), Some(28), "up from the region's bottom");
        assert_eq!(row_of(t, "UNDER"), Some(30));
        assert_eq!(t.images().placements_expired, 1);
    });
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[10;30r");
        labelled(t, 9, "OVER");
        labelled(t, 10, "TOP");
        labelled(t, 30, "BOT");
        labelled(t, 31, "UNDER");
        feed(t, b"\x1b[10;1H\x1bM");
        assert_eq!(row_of(t, "OVER"), Some(8));
        assert_eq!(row_of(t, "TOP"), Some(10), "down from the region's top");
        assert_eq!(row_of(t, "BOT"), None, "pushed past the region's bottom");
        assert_eq!(row_of(t, "UNDER"), Some(30));
        assert_eq!(t.images().placements_expired, 1);
    });
}

#[test]
fn a_rotating_regions_last_row_moves_with_it_and_the_rows_under_stay() {
    // `DECSTBM 1 ; 40` rotates. The region's last row is inside the band
    // the rotation moves and outside the one the repair puts back, so it
    // is the row that tells the repair's first line from the one before;
    // the screen's last row is the repair's last.
    with(Depth::DEFAULT, |t| {
        feed(t, b"\x1b[1;40r");
        labelled(t, 40, "BOT");
        labelled(t, 41, "UNDER");
        labelled(t, 50, "LAST");
        feed(t, b"\x1b[S");
        assert_eq!(row_of(t, "BOT"), Some(38), "up with the region");
        assert_eq!(row_of(t, "UNDER"), Some(40), "under the region");
        assert_eq!(row_of(t, "LAST"), Some(49), "the screen's last row");
        assert_eq!(t.images().placements_expired, 0);
    });
}

#[test]
fn a_header_image_above_a_region_ending_on_the_last_row_stays_on_screen() {
    // `DECSTBM 5 ; 50`: the region ends on the screen's last row and starts
    // below its top, so an LF at its bottom scrolls the region and the
    // header rows above it stay -- the image on one of them with them.
    with(Depth::DEFAULT, |t| {
        tagged(t, 2, "HDR");
        feed(t, b"\x1b[5;50r\x1b[50;1H\n\n\n");
        let p = between(t, "HDR", "three LFs at the region's bottom");
        assert_eq!(drawn_row(t, &p, Rows::ZERO), 1, "still on the screen");
        assert!(
            t.scrollback().line(Back::of(0)).is_none(),
            "nothing retired"
        );
    });
}

#[test]
fn a_status_line_image_placed_with_the_apps_cell_size_stays_there() {
    // The shape `an_image_on_a_status_line_under_a_top_region_stays_there`
    // could not test: with the app's cell size the Sixel advances the
    // cursor through `index()`, and from the last row below a region that
    // advance scrolled the whole screen, so the image landed a row high,
    // under the region's last row, and row 1 went to the history.
    with(Depth::DEFAULT, |t| {
        feed(
            t,
            b"\x1b[1;1HFIRST\x1b[1;49r\x1b[49;1HREGION-LAST\x1b[50;1H",
        );
        feed(t, TINY);
        feed(t, b"\x1b[50;5HSTATUS");
        let p = only(t);
        assert_eq!(drawn_row(t, &p, Rows::ZERO), 49, "on the status line");
        assert_eq!(beside(t, &p, 0).as_deref(), Some("    STATUS"));
        assert_eq!(beside(t, &p, -1).as_deref(), Some("REGION-LAST"));
        assert_eq!(
            text_of(t.row(Row::ZERO).expect("a row on the grid")),
            "FIRST",
            "nothing scrolled"
        );
    });
}

#[test]
fn an_anchor_whose_column_a_prompt_resize_cut_off_asks_by_its_rows_first_cell() {
    // The column an anchor keeps in its row is a cell of the line only while
    // it is inside the width the next re-cut reads. Narrowed to 60, the
    // anchor sits on row 3 of the line at column 20; a resize at a prompt
    // takes the screen to 15 without re-cutting, and the next re-cut reads
    // each row to 15 cells -- column 20 is none of them. Asked as the row's
    // first cell, compacted ordinal 45, the anchor lands on row 0 at 50
    // columns; asked by column 20 clamped to the old width, ordinal 60, on
    // row 1.
    with(Depth::DEFAULT, |t| {
        feed(t, long(250).as_bytes());
        feed(t, SWIRL);
        resize(t, 60, 50);
        feed(t, b"\r\x1b]133;A\x07$ ");
        resize(t, 15, 50);
        let p = only(t);
        assert_eq!(
            beside(t, &p, 0).as_deref(),
            Some(&long(250)[180..195]),
            "a prompt resize re-cut nothing: still row 3, read to 15"
        );
        feed(t, b"\x1b]133;C\x07");
        resize(t, 50, 50);
        let p = only(t);
        assert_eq!(
            text_of(t.row(Row::ZERO).expect("a row on the grid")).len(),
            50,
            "the line's first row"
        );
        assert_eq!(drawn_row(t, &p, Rows::ZERO), 0, "is the image's");
    });
}
