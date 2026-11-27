//! **Rows laid out in their visual order, drawn alike on every path**:
//! screens fed as bytes with BDSM set (`[[source: ecma48]]` 7.2.1,
//! IMPLICIT), each row laid out in the order UAX #9 gives it
//! (`[[source: uax9-bidi]]`), or with BDSM reset under SCP, SRS and
//! DECRLM, drawn by the scene harness (`scenes::Arms`) on the cell path,
//! the instance path and the CPU path and held to the cell path's bars --
//! the cell frame the instance frame exactly, and both the CPU path's
//! premultiplied frame, exact where no record blends and within a step
//! where one does -- on lavapipe on both memory arms, and on RADV staged
//! and on the arm its heaps allow.
//!
//! Each scene's expected frame is the same characters written already in
//! their visual order with BDSM reset, which every path draws as written:
//! a row reordered on one path and not another, or reordered wrongly on
//! all three, is apart from it on that path. Each comparison between two
//! screens is made on every path's own frames, and each scene carries a
//! control that differs, so that frames which agree are not frames that
//! all drew the stored order.
//!
//! - [`right_to_left_rows_draw_in_their_visual_order_on_every_arm`], a
//!   Hebrew word in a Latin line, mirrored brackets, a wide character in a
//!   right-to-left run, pointed letters, an underline across a run, no
//!   shaping across a level run, a selection the order splits, and a block
//!   and a bar cursor over a right-to-left letter.
//! - [`a_re_broken_paragraph_draws_the_same_on_every_arm`], a finished
//!   paragraph re-broken at its words, a selection on its moved line, and
//!   the setting off as the control.
//! - [`a_selection_from_above_the_window_draws_the_same_on_every_arm`], the
//!   cells a window line draws from the row above the window, selected.
//! - [`a_double_width_right_to_left_line_is_drawn_within_its_half`], DECDWL
//!   and DECDHL's top half, implicit and under SCP.
//! - [`a_right_to_left_paragraph_is_drawn_against_the_right_edge_on_every_arm`].
//! - [`a_table_row_keeps_its_columns_on_every_arm`].
//! - [`a_right_to_left_paragraph_drawn_by_rows_is_the_whole_frame`], the
//!   cell path's frames into two targets in turn, each the whole frame
//!   drawn into a new target; the instance path draws whole frames only.
//! - [`the_cursor_on_a_right_to_left_row_is_drawn_in_its_shape_on_every_arm`].
//! - [`mirrored_characters_draw_mirrored_on_every_arm`], L4's glyphs with
//!   no mirror drawn flipped in their cells, and a mark left unflipped.
//! - [`a_reversed_string_draws_alike_on_every_arm`], SRS.
//! - [`a_leftward_row_draws_alike_on_every_arm`], DECRLM.
//! - [`a_line_scp_turns_draws_alike_on_every_arm`], SCP, `?2500`, and the
//!   kept layout once a joining letter has been seen.
//! - [`a_paragraph_inside_side_margins_draws_alike_on_every_arm`], DECSLRM.
//! - [`the_cell_path_rejoins_only_the_rows_the_copy_finds_changed`], the
//!   cell frame's build finds a row's joins again only where its copy's
//!   compare finds the row changed, and its joins are a fresh layout's.
//!
//! Not drawn here, each for its reason:
//!
//! - smooth scroll and screen slides of a right-to-left line: the cell
//!   path draws no pan yet (design section 8);
//! - the window copy's arm: it needs an API only the macOS presenter has.
//!
//! The faces are the app's built Regular face from `crates/baud-app/fonts/built`
//! and the Noto colour emoji subset from `crates/baud-font/tests/fonts`, each
//! under the licence beside it.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term, WrapPending};
use baud_domain::{CacheGeometry, Col, Cols, Depth, Row, Rows, WinSize};
use baud_font::Face;
use baud_present_vulkan::cells::CellDraw;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::lend::Arm;
use baud_present_wayland::Px;
use baud_render::{Highlight, Redraw, Region, Renderer, Stack, Tally, Theme};
use common::{lavapipe, radv};
use scenes::{Arms, Frames, differing, feed, repo_file};

/// Wide enough that a paragraph against the right edge is far from the
/// left, as the scenes it is drawn in were written for.
const WS: WinSize = WinSize {
    ws_col: 120,
    ws_row: 10,
};

/// The app's built Regular face and the Noto colour emoji subset.
fn rtl_faces() -> (Vec<u8>, Vec<u8>) {
    (
        repo_file("crates/baud-app/fonts/built/Regular.ttf"),
        repo_file("crates/baud-font/tests/fonts/NotoColorEmoji-subset.ttf"),
    )
}

/// A renderer over `faces` on the dark theme, sized to [`WS`].
fn fresh<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    scenes::renderer(Stack::new(faces).expect("a stack"), Theme::dark(), WS)
}

/// The terminal's storage: [`WS`], with twelve lines of history.
fn owned() -> Owned {
    Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(12))
}

/// The frame's width, and a cell's width and height, in pixels.
fn geometry(arms: &Arms<'_, '_, '_>) -> (usize, usize, usize) {
    let m = arms.renderer.metrics();
    let at = |v: u32| usize::try_from(v).expect("a size");
    (
        at(arms.renderer.whole_size().0.value()),
        at(m.cell_w.value()),
        at(m.cell_h.value()),
    )
}

/// Each path's frame of `f`, named.
fn paths(f: &Frames) -> [(&'static str, &[u32]); 3] {
    [
        ("the CPU path", &f.cpu),
        ("the instance path", &f.instance),
        ("the cell path", &f.cell),
    ]
}

/// **`a` and `b` the same screen on every path**: each path's frame of
/// one, pixel for pixel, its frame of the other.
fn same(a: &Frames, b: &Frames, what: &str) {
    for ((path, x), (_, y)) in paths(a).into_iter().zip(paths(b)) {
        let d = differing(x, y);
        assert_eq!(d, 0, "{what}: {path} differs, {d} pixels");
    }
}

/// **`a` and `b` different screens on every path**: the control.
fn apart(a: &Frames, b: &Frames, what: &str) {
    for ((path, x), (_, y)) in paths(a).into_iter().zip(paths(b)) {
        assert!(differing(x, y) > 0, "{what}: {path} draws them alike");
    }
}

/// `f` held for each path's frames of `a` and `b`, named by the path.
fn on_paths(a: &Frames, b: &Frames, mut f: impl FnMut(&str, &[u32], &[u32])) {
    for ((path, x), (_, y)) in paths(a).into_iter().zip(paths(b)) {
        f(path, x, y);
    }
}

/// Whether any pixel of the cell at `(row, col)` differs between `a` and
/// `b`, in a frame of `geom`.
fn cell_differs(
    a: &[u32],
    b: &[u32],
    (w, cw, ch): (usize, usize, usize),
    (row, col): (usize, usize),
) -> bool {
    (row * ch..(row + 1) * ch)
        .any(|y| (col * cw..(col + 1) * cw).any(|x| a[y * w + x] != b[y * w + x]))
}

/// The bounding box of the pixels `a` and `b` differ at, in cells:
/// (col0, row0, col1, row1).
fn diff_box(
    a: &[u32],
    b: &[u32],
    (w, cw, ch): (usize, usize, usize),
) -> Option<(usize, usize, usize, usize)> {
    let mut lo = (usize::MAX, usize::MAX);
    let mut hi = (0, 0);
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x != y {
            let (px, py) = (i % w, i / w);
            lo = (lo.0.min(px), lo.1.min(py));
            hi = (hi.0.max(px), hi.1.max(py));
        }
    }
    (lo.0 != usize::MAX).then(|| (lo.0 / cw, lo.1 / ch, hi.0 / cw, hi.1 / ch))
}

/// The pixels of `frame` that differ from `other`, as (x, y).
fn changed_at(frame: &[u32], other: &[u32], w: usize) -> Vec<(usize, usize)> {
    (0..frame.len())
        .filter(|&i| frame[i] != other[i])
        .map(|i| (i % w, i / w))
        .collect()
}

/// **A right-to-left row draws in its visual order alike on every arm**,
/// and in the order a hand-reordered row draws in, on `arm`: a Hebrew word
/// in a Latin line, brackets L4 mirrors, a wide character inside a
/// right-to-left run, pointed letters the cell path draws by records, an
/// underline across a run and a bracket carrying an RLM; no shaping across
/// a level run (UAX #9 3.5); a logical selection whose highlight the order
/// splits, on a plain and on a pointed row; a block and a bar cursor over
/// a right-to-left letter.
fn visual_order(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    // Each row as written, and as UAX #9 draws it: the first row, whose
    // first strong letter is Latin, at paragraph level 0 with its run of R
    // reversed in place (L2); every row that starts with a Hebrew letter at
    // level 1 (P2-P3), reversed whole with its Latin run kept in order, and
    // drawn against the right edge with the two erased cells it was written
    // after, logically first, at the far right. The third field is a
    // right-to-left row's width in cells.
    let (shin, lamed, vav, mem) = ("\u{5E9}", "\u{5DC}", "\u{5D5}", "\u{5DD}");
    let (alef, bet, gimel) = ("\u{5D0}", "\u{5D1}", "\u{5D2}");
    let (qamats, holam) = ("\u{5B8}", "\u{5B9}");
    let rows: [(String, String, Option<usize>); 7] = [
        (
            format!("abc {shin}{lamed}{vav}{mem} def"),
            format!("abc {mem}{vav}{lamed}{shin} def"),
            None,
        ),
        (
            format!("{alef}({bet}){gimel} xy"),
            format!("xy {gimel}({bet}){alef}"),
            Some(8),
        ),
        (
            format!("{alef}\u{231A}{bet} z"),
            format!("z {bet}\u{231A}{alef}"),
            Some(6),
        ),
        (
            format!("{shin}{qamats}{lamed}{vav}{holam}{mem}"),
            format!("{mem}{vav}{holam}{lamed}{shin}{qamats}"),
            Some(4),
        ),
        (
            format!("\x1b[4;31m{alef}{bet}\x1b[0m{gimel}"),
            format!("{gimel}\x1b[4;31m{bet}{alef}\x1b[0m"),
            Some(3),
        ),
        (
            "The quick brown fox jumps 0123456789".to_string(),
            "The quick brown fox jumps 0123456789".to_string(),
            None,
        ),
        (
            format!("{alef}({bet})\u{200F}{gimel} xy"),
            format!("xy {gimel}({bet}){alef}"),
            Some(8),
        ),
    ];
    let cols = usize::from(WS.ws_col);
    let scene = |t: &mut Term<'_>, implicit: bool, cursor: &str| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
        feed(t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
        for (i, (logical, visual, rtl)) in rows.iter().enumerate() {
            let (text, col) = match (implicit, rtl) {
                (true, _) => (logical, 3),
                (false, None) => (visual, 3),
                (false, Some(width)) => (visual, cols - 2 - width + 1),
            };
            feed(t, format!("\x1b[{};{col}H{text}", i + 2).as_bytes());
        }
        feed(t, cursor.as_bytes());
    };
    // The rows, no cursor and no selection: the frame of the visual order.
    {
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        scene(&mut t, true, "");
        let got = arms.check(&mut t, &format!("{arm:?}: reordered"));
        assert!(arms.renderer.bidi().any(), "the rows were reordered");
        assert!(
            (0..usize::from(WS.ws_row)).all(|r| arms
                .renderer
                .bidi()
                .mirrors(Row::from_index(r))
                .is_empty()),
            "every bracket is drawn as its mirroring glyph"
        );
        scene(&mut t, false, "");
        let want = arms.check(&mut t, &format!("{arm:?}: written in visual order"));
        same(&got, &want, "the visual order drawn");
        scene(&mut t, false, "");
        feed(&mut t, b"\x1b[8l");
        for (i, (logical, _, _)) in rows.iter().enumerate() {
            feed(&mut t, format!("\x1b[{};3H{logical}", i + 2).as_bytes());
        }
        let stored = arms.check(&mut t, &format!("{arm:?}: explicit"));
        apart(
            &stored,
            &want,
            "the control: BDSM reset draws the stored order",
        );
    }
    // **No shaping crosses a level run, nor runs inside a right-to-left
    // one**: `x->y` is one left-to-right run, which the face's `->`
    // ligature joins; the `->` between two Hebrew letters resolves R, is
    // drawn `<-` with the `>` mirrored, and each of its cells is shaped
    // alone, by the renderer's runs on two paths and by the overlay's
    // windows on the cell path.
    {
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        let write = |t: &mut Term<'_>| {
            feed(t, format!("\x1b[2;3Hx->y {alef}->{bet} done").as_bytes());
            feed(t, b"\x1b[4;3HThe quick brown fox jumps 0123456789");
        };
        feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        write(&mut t);
        let got = arms.check(&mut t, &format!("{arm:?}: ligatures"));
        feed(&mut t, b"\x1b[2J\x1b[8l");
        write(&mut t);
        let stored = arms.check(&mut t, &format!("{arm:?}: ligatures, explicit"));
        apart(&got, &stored, "the row was reordered");
    }
    // A logical selection over `c ש` (the row is written from column 2):
    // drawn on the `c`, the space and the shin, which the order has put at
    // the far end of the word -- two runs of highlight.
    {
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        scene(&mut t, true, "");
        arms.highlight = Some(Highlight::new(
            (Row::from_index(1), Col::at_index(4)),
            (Row::from_index(1), Col::at_index(6)),
            Region::Stream,
        ));
        let sel = arms.check(&mut t, &format!("{arm:?}: selected"));
        arms.highlight = None;
        let bare = arms.check(&mut t, &format!("{arm:?}: unselected"));
        let g = geometry(&arms);
        // Logical 4..=6 is `c`, space, shin; the shin is drawn at visual
        // column 9, the end of `שלום` reversed, and the lamed at 8 and the
        // final mem at 6 are not selected.
        on_paths(&sel, &bare, |path, a, b| {
            let d = |col| cell_differs(a, b, g, (1, col));
            assert!(d(4) && d(5) && d(9), "{path}: the selected cells");
            assert!(!d(8) && !d(6), "{path}: only the logical range");
        });
    }
    // **The pointed row, whose marked cells the cell path draws by
    // records**: a selection of the shin and the lamed (logical 2 and 3,
    // drawn at 117 and 116) and a block cursor on the shin, each mapped
    // through the order where a record is painted as well as where the
    // shader paints.
    {
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        scene(&mut t, true, "");
        arms.highlight = Some(Highlight::new(
            (Row::from_index(4), Col::at_index(2)),
            (Row::from_index(4), Col::at_index(3)),
            Region::Stream,
        ));
        let sel = arms.check(&mut t, &format!("{arm:?}: pointed, selected"));
        arms.highlight = None;
        let bare = arms.check(&mut t, &format!("{arm:?}: pointed, unselected"));
        let g = geometry(&arms);
        let r4 = Row::from_index(4);
        let at = |c: usize| {
            arms.renderer
                .bidi()
                .visual_col(r4, Col::at_index(c))
                .index()
        };
        assert_eq!((at(2), at(3)), (cols - 3, cols - 4), "shin and lamed");
        on_paths(&sel, &bare, |path, a, b| {
            let d = |col| cell_differs(a, b, g, (4, col));
            assert!(d(cols - 4) && d(cols - 3), "{path}: the selected cells");
            assert!(!d(2) && !d(3), "{path}: not at their logical columns");
            assert!(
                !d(cols - 5) && !d(cols - 6),
                "{path}: only the logical range"
            );
        });
        scene(&mut t, true, "\x1b[2 q\x1b[5;3H\x1b[?25h");
        let block = arms.check(&mut t, &format!("{arm:?}: pointed, block cursor"));
        on_paths(&block, &bare, |path, a, b| {
            assert!(
                cell_differs(a, b, g, (4, cols - 3)),
                "{path}: the block over the shin, drawn at 117"
            );
            assert!(
                !cell_differs(a, b, g, (4, 2)),
                "{path}: and not at its logical column"
            );
        });
    }
    // A block and a bar cursor on the lamed (logical column 7): drawn over
    // visual column 8, the bar on the cell's right edge.
    for (style, name) in [("2", "block"), ("6", "bar")] {
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        scene(&mut t, true, &format!("\x1b[{style} q\x1b[2;8H\x1b[?25h"));
        let got = arms.check(&mut t, &format!("{arm:?}: a {name} cursor"));
        assert_eq!(
            arms.renderer
                .bidi()
                .visual_col(Row::from_index(1), Col::at_index(7)),
            Col::at_index(8)
        );
        scene(&mut t, true, "\x1b[2;8H\x1b[?25l");
        let hidden = arms.check(&mut t, &format!("{arm:?}: no cursor"));
        let (w, cw, ch) = geometry(&arms);
        on_paths(&got, &hidden, |path, a, b| {
            let xs: Vec<usize> = (0..w)
                .filter(|&x| (ch..2 * ch).any(|y| a[y * w + x] != b[y * w + x]))
                .collect();
            assert!(!xs.is_empty(), "{path}, {name}: drawn");
            assert!(
                xs.iter().all(|&x| x >= 8 * cw && x < 9 * cw),
                "{path}, {name}: inside visual column 8, {xs:?}"
            );
            if name == "bar" {
                assert!(xs.contains(&(9 * cw - 1)), "{path}: the bar on the right");
                assert!(!xs.contains(&(8 * cw)), "{path}: and not on the left");
            }
        });
    }
}

#[test]
fn right_to_left_rows_draw_in_their_visual_order_on_every_arm() {
    let gpu = lavapipe();
    visual_order(&gpu, Arm::InPlace);
    visual_order(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn right_to_left_rows_draw_in_their_visual_order_on_every_arm_on_radv() {
    let gpu = radv();
    visual_order(&gpu, Arm::Staging);
    visual_order(&gpu, gpu.arm(1));
}

/// **A finished paragraph the display re-breaks at its words draws the
/// same on every arm**, on `arm`: the English run the terminal's wrap split
/// moved to a line of its own, flush left, the Hebrew around it against the
/// right edge, the text above pushed up a line, and a selection of stored
/// cells on the moved line drawn where they are drawn. With the setting off
/// the same bytes draw the terminal's cut, the control.
fn re_broken(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let he =
        "\u{5D6}\u{5D5} \u{5E4}\u{5E1}\u{5E7}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA} ";
    let tail = "\u{5E9}\u{5DE}\u{5DE}\u{5E9}\u{5D9}\u{5DB}\u{5D4} \u{5DC}\u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D4}\u{5D1}\u{5D0}\u{5D4}.";
    // Seven Hebrew phrases, 105 cells, then an English run across column
    // 120, then Hebrew; a prompt mark below it, so it is finished.
    let history: String = (0..12).map(|i| format!("h{i}\r\n")).collect();
    let bytes = format!(
        "\x1b[?25l{history}{}with English words in the middle, {tail}\r\n\x1b]133;A\x07$ ",
        he.repeat(7)
    );
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    feed(&mut t, bytes.as_bytes());
    let on = arms.check(&mut t, &format!("{arm:?}: re-broken"));
    let d = arms
        .renderer
        .bidi()
        .display()
        .expect("the paragraph re-broken");
    assert_eq!(d.counts().0, Tally::of(1), "one paragraph");
    // The English line is display row 7, its "with" at the left edge,
    // stored on the paragraph's first row (window row 7) at column 105.
    assert_eq!(
        arms.renderer.bidi().pointer(Row::from_index(7), Col::ZERO),
        (Row::from_index(7), Col::at_index(105))
    );
    // A selection of the stored cells "with" (105..=108 of row 7): drawn
    // at visual 0..=3 of display row 7, nowhere on row 6.
    arms.highlight = Some(Highlight::new(
        (Row::from_index(7), Col::at_index(105)),
        (Row::from_index(7), Col::at_index(108)),
        Region::Stream,
    ));
    let sel = arms.check(&mut t, &format!("{arm:?}: re-broken, selected"));
    arms.highlight = None;
    let g = geometry(&arms);
    on_paths(&sel, &on, |path, a, b| {
        assert!(
            (0..4).all(|c| cell_differs(a, b, g, (7, c))),
            "{path}: the selection drawn"
        );
        assert!(!cell_differs(a, b, g, (7, 4)), "{path}: and only its cells");
        assert!(
            (100..120).all(|c| !cell_differs(a, b, g, (6, c))),
            "{path}: not at the stored columns on the Hebrew line"
        );
    });
    arms.renderer.set_word_wrap(baud_render::WordWrap::Off);
    let off = arms.check(&mut t, &format!("{arm:?}: the setting off"));
    assert!(arms.renderer.bidi().display().is_none());
    apart(&on, &off, "the control: off draws otherwise");
}

#[test]
fn a_re_broken_paragraph_draws_the_same_on_every_arm() {
    let gpu = lavapipe();
    re_broken(&gpu, Arm::InPlace);
    re_broken(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_re_broken_paragraph_draws_the_same_on_every_arm_on_radv() {
    let gpu = radv();
    re_broken(&gpu, Arm::Staging);
    re_broken(&gpu, gpu.arm(1));
}

/// **A selection of the cells a window line draws from above the window
/// is drawn the same on every arm**, on `arm`: "Y" ten times, cut after its
/// sixth letter at the window's top, is drawn whole on the window's first
/// line, its first six letters stored on the row above the window. Those
/// six selected, and none of the window's own rows, every path draws the
/// six cells selected and nothing else.
fn selected_above(gpu: &Gpu, arm: Arm) {
    use baud_render::{Beyond, Grain, Scrolled, Selection, Span};
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let history: String = (0..12).map(|i| format!("h{i}\r\n")).collect();
    let fillers: String = (1..=8)
        .map(|i| format!("line {i}: the quick brown fox jumps over the lazy dog\r\n"))
        .collect();
    let bytes = format!(
        "\x1b[?25l{history}{} {} zz\r\n{fillers}\x1b]133;A\x07$ ",
        "X".repeat(113),
        "Y".repeat(10)
    );
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    feed(&mut t, bytes.as_bytes());
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    let top = scrolled.top(Rows::ZERO);
    let up = top.above().expect("a line above");
    let up_row = t.view(Rows::of_count(1)).row(Row::ZERO).to_vec();
    let mut part = Selection::at(Span::cell(&up_row, up, Col::at_index(114)), Grain::Cell);
    part.extend(Span::cell(&up_row, up, Col::at_index(119)));
    let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
    let h = part
        .highlight_reaching(
            top,
            Rows::of_index(rows),
            Cols::of_index(cols),
            Beyond::of(&t, Rows::ZERO),
        )
        .expect("drawn on the first line");
    assert!(
        (0..rows).all(|r| !h.covers_row(Row::from_index(r))),
        "the premise: no window row's own cells"
    );
    let plain = arms.check(&mut t, &format!("{arm:?}: unselected"));
    assert!(
        arms.renderer.bidi().display().is_some(),
        "the premise: the paragraph is drawn at its words"
    );
    arms.highlight = Some(h);
    let lit = arms.check(&mut t, &format!("{arm:?}: selected above the window"));
    arms.highlight = None;
    let g = geometry(&arms);
    on_paths(&plain, &lit, |path, a, b| {
        assert_eq!(
            diff_box(a, b, g),
            Some((0, 0, 5, 0)),
            "{path}: the six cells of the first line, and only they"
        );
    });
}

#[test]
fn a_selection_from_above_the_window_draws_the_same_on_every_arm() {
    let gpu = lavapipe();
    selected_above(&gpu, Arm::InPlace);
    selected_above(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_selection_from_above_the_window_draws_the_same_on_every_arm_on_radv() {
    let gpu = radv();
    selected_above(&gpu, Arm::Staging);
    selected_above(&gpu, gpu.arm(1));
}

/// **A double-width right-to-left line is drawn within its half, right to
/// left**, on `arm`: UAX #9 3.4 reorders each display line, and a
/// double-width line's is its half (`[[source: dec-std-070]]` § 5.14.1;
/// `[[source: vt510-decdwl]]`), so a Hebrew paragraph sits against the
/// half's right edge, its brackets mirrored (L4). The expected frame is the
/// same line written in its drawn order with BDSM reset, its last cell the
/// half's last; with BDSM reset and SCP's right-to-left path the stored
/// line is reversed within the half and draws it too. On a DECDWL line and
/// on the top half of a DECDHL one.
fn double_width(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let half = usize::from(WS.ws_col) / 2;
    let stored = "\u{5D0}\u{5D1} (\u{5D2})";
    let shown = "(\u{5D2}) \u{5D1}\u{5D0}";
    for size in ["\x1b#6", "\x1b#3"] {
        let scene = |t: &mut Term<'_>, setup: &str, at: usize, line: &str| {
            feed(
                t,
                format!(
                    "\x1bc\x1b[?25l\x1b[2;1H{setup}{size}\x1b[2;{at}H{line}\x1b[4;3HThe quick brown fox"
                )
                .as_bytes(),
            );
        };
        let what = |s: &str| format!("{arm:?}, {size:?}: {s}");
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        scene(&mut t, "", 1, stored);
        let implicit = arms.check(&mut t, &what("implicit"));
        assert!(
            arms.renderer.bidi().reordered(Row::from_index(1)),
            "the premise: the line is reordered"
        );
        scene(&mut t, "\x1b[8l", half + 1 - shown.chars().count(), shown);
        let want = arms.check(&mut t, &what("written in its drawn order"));
        feed(&mut t, b"\x1b[2;1H\x1b[2K\x1b[4;3HThe quick brown fox");
        let blank = arms.check(&mut t, &what("the line erased"));
        apart(&want, &blank, &what("the expected line is drawn"));
        same(&implicit, &want, &what("implicit, within the half"));
        scene(&mut t, "\x1b[8l\x1b[2 k", 1, stored);
        let reversed = arms.check(&mut t, &what("explicit, right to left"));
        same(
            &reversed,
            &want,
            &what("explicit, right to left, within the half"),
        );
    }
}

#[test]
fn a_double_width_right_to_left_line_is_drawn_within_its_half() {
    let gpu = lavapipe();
    double_width(&gpu, Arm::InPlace);
    double_width(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_double_width_right_to_left_line_is_drawn_within_its_half_on_radv() {
    let gpu = radv();
    double_width(&gpu, Arm::Staging);
    double_width(&gpu, gpu.arm(1));
}

/// **A paragraph whose first strong letter is Hebrew runs right to left
/// against the right edge, alike on every arm**, on `arm`: UAX #9 P2-P3
/// give the sentence level 1, so its closing period is drawn at the left
/// end of its text and the text ends at the window's right edge, where the
/// erased cells past it go before it
/// (`[[source: terminal-wg-bidi-erased-cells]]`). The expected frame is the
/// same characters written already in visual order, at the right edge,
/// with BDSM reset; a line whose first strong letter is Latin stays at the
/// left edge on both.
fn right_edge(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let cols = usize::from(WS.ws_col);
    let sentence = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}! \u{5D6}\u{5D5} \u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA}.";
    let n = sentence.chars().count();
    let reversed: String = sentence.chars().rev().collect();
    let control = "hello world, 123 \u{5E9}\u{5DC}\u{5D5}\u{5DD}";
    let control_visual = "hello world, 123 \u{5DD}\u{5D5}\u{5DC}\u{5E9}";
    let base = b"\x1b[?25l\x1b[2J\x1b[8;1HThe quick brown fox jumps 0123456789";
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    feed(&mut t, base);
    feed(
        &mut t,
        format!("\x1b[2;1H{sentence}\x1b[4;1H{control}").as_bytes(),
    );
    let got = arms.check(&mut t, &format!("{arm:?}: implicit"));
    assert!(arms.renderer.bidi().reordered(Row::from_index(1)));
    assert_eq!(
        arms.renderer
            .bidi()
            .visual_col(Row::from_index(1), Col::at_index(n - 1)),
        Col::at_index(cols - n),
        "the period, written last, drawn at the text's left end"
    );
    feed(&mut t, b"\x1b[8l");
    feed(&mut t, base);
    feed(
        &mut t,
        format!(
            "\x1b[2;{}H{reversed}\x1b[4;1H{control_visual}",
            cols - n + 1
        )
        .as_bytes(),
    );
    let want = arms.check(&mut t, &format!("{arm:?}: written in visual order"));
    same(&got, &want, "drawn right to left, at the right");
    feed(&mut t, base);
    feed(
        &mut t,
        format!("\x1b[2;1H{sentence}\x1b[4;1H{control}").as_bytes(),
    );
    let stored = arms.check(&mut t, &format!("{arm:?}: explicit"));
    apart(
        &stored,
        &want,
        "the control: BDSM reset draws the stored order",
    );
}

#[test]
fn a_right_to_left_paragraph_is_drawn_against_the_right_edge_on_every_arm() {
    let gpu = lavapipe();
    right_edge(&gpu, Arm::InPlace);
    right_edge(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_right_to_left_paragraph_is_drawn_against_the_right_edge_on_every_arm_on_radv() {
    let gpu = radv();
    right_edge(&gpu, Arm::Staging);
    right_edge(&gpu, gpu.arm(1));
}

/// **A table row keeps its columns, and each cell its own direction, alike
/// on every arm**, on `arm`: a table drawn with box-drawing rules, each
/// cell a paragraph of its own (UAX #9 section 3's table cells, HL4), so
/// the Hebrew name and the Hebrew owner read right to left against their
/// cells' right sides and every rule stays under the one above. The
/// expected frame is the rows written already in that order with BDSM
/// reset.
fn table_row(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let rows = [
        "\u{2502} Name          \u{2502} Size \u{2502} Owner \u{2502}",
        "\u{2502} notes.txt     \u{2502} 42   \u{2502} dev   \u{2502}",
        "\u{2502} \u{5DE}\u{5E1}\u{5DE}\u{5DA}.txt      \u{2502} 812  \u{2502} \u{5D3}\u{5E0}\u{5D4}   \u{2502}",
    ];
    let visual = "\u{2502}      txt.\u{5DA}\u{5DE}\u{5E1}\u{5DE} \u{2502} 812  \u{2502}   \u{5D4}\u{5E0}\u{5D3} \u{2502}";
    let base = b"\x1b[?25l\x1b[2J\x1b[8;1HThe quick brown fox jumps 0123456789";
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    feed(&mut t, base);
    for (i, r) in rows.iter().enumerate() {
        feed(&mut t, format!("\x1b[{};1H{r}", i + 2).as_bytes());
    }
    let got = arms.check(&mut t, &format!("{arm:?}: implicit"));
    assert!(arms.renderer.bidi().reordered(Row::from_index(3)));
    feed(&mut t, b"\x1b[8l");
    feed(&mut t, base);
    for (i, r) in [rows[0], rows[1], visual].iter().enumerate() {
        feed(&mut t, format!("\x1b[{};1H{r}", i + 2).as_bytes());
    }
    let want = arms.check(&mut t, &format!("{arm:?}: written in visual order"));
    same(&got, &want, "each cell in its own direction");
    // The control: the Hebrew row as stored, BDSM still reset.
    feed(&mut t, format!("\x1b[4;1H{}", rows[2]).as_bytes());
    let stored = arms.check(&mut t, &format!("{arm:?}: explicit"));
    apart(
        &stored,
        &want,
        "the control: BDSM reset draws the stored order",
    );
}

#[test]
fn a_table_row_keeps_its_columns_on_every_arm() {
    let gpu = lavapipe();
    table_row(&gpu, Arm::InPlace);
    table_row(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_table_row_keeps_its_columns_on_every_arm_on_radv() {
    let gpu = radv();
    table_row(&gpu, Arm::Staging);
    table_row(&gpu, gpu.arm(1));
}

/// What a frame of [`by_rows`] must find changed.
#[derive(Clone, Copy, Debug)]
enum Want {
    /// Anything, the whole included.
    Any,
    /// Rows, any of them.
    Rows,
}

/// **A right-to-left paragraph's frames drawn by rows are the whole
/// frame's pixels**, on `arm`: its text sits past the extent the core
/// records for the row, at the right edge, so a frame that typed one more
/// letter, erased one, or moved the cursor along the row must still draw
/// where the text is drawn. The cell path's frames are drawn into two
/// targets in turn, each read back against the same frame drawn whole into
/// a new target; a frame that moved the cursor alone finds rows changed.
/// Each frame is held to the bars as well.
fn by_rows(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let sentence = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}! \u{5D6}\u{5D5} \u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA}.";
    let opening =
        format!("\x1b[?25h\x1b[2J\x1b[8;1HThe quick brown fox jumps 0123456789\x1b[6;1H{sentence}");
    let steps: [(&str, &[u8], Want); 5] = [
        ("the scene and the sentence", opening.as_bytes(), Want::Any),
        ("one more letter", "\u{5D0}".as_bytes(), Want::Any),
        ("and erased again", b"\x08\x1b[K", Want::Any),
        ("the cursor along the row", b"\x1b[6;3H", Want::Rows),
        ("and to the period", b"\x1b[6;26H", Want::Rows),
    ];
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    let (w, h) = arms.renderer.whole_size();
    let (w, h) = (Px(w.value()), Px(h.value()));
    let targets = [
        gpu.target(w, h).expect("the first target"),
        gpu.target(w, h).expect("the second target"),
    ];
    let mut wrong = Vec::new();
    let mut banded = 0;
    for (i, &(name, bytes, want)) in steps.iter().enumerate() {
        feed(&mut t, bytes);
        arms.build(&mut t);
        let into = &targets[i % 2];
        arms.with_frame(&t, |d, input: &CellDraw<'_>| {
            let drawn = d.draw(input, into);
            banded += usize::from(matches!(drawn.drawn, Redraw::Rows(_)));
            let got = d.gpu.read(into).expect("the target read back");
            let whole = d.whole(input);
            let n = differing(&got, &whole);
            if n != 0 {
                wrong.push(format!("{i} {name}: {n} pixels, drawn {:?}", drawn.drawn));
            }
            let met = match (want, drawn.changed) {
                (Want::Any, _) | (Want::Rows, Redraw::Rows(_)) => true,
                (Want::Rows, _) => false,
            };
            if !met {
                wrong.push(format!(
                    "{i} {name}: changed {:?}, wanted {want:?}",
                    drawn.changed
                ));
            }
        });
        arms.check(&mut t, &format!("{arm:?}: {name}"));
    }
    arms.with_frame(&t, |d, _| {
        for target in targets {
            d.mem.forget(d.gpu, &target);
            d.gpu.drop_target(target);
        }
    });
    assert!(wrong.is_empty(), "{arm:?}, the cell path: {wrong:#?}");
    assert!(
        banded > 0,
        "{arm:?}: the premise, a frame was drawn by rows"
    );
}

#[test]
fn a_right_to_left_paragraph_drawn_by_rows_is_the_whole_frame() {
    let gpu = lavapipe();
    by_rows(&gpu, Arm::InPlace);
    by_rows(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_right_to_left_paragraph_drawn_by_rows_is_the_whole_frame_on_radv() {
    let gpu = radv();
    by_rows(&gpu, Arm::Staging);
    by_rows(&gpu, gpu.arm(1));
}

/// **The cursor on a right-to-left row is drawn in its own shape, alike on
/// every arm**, on `arm`: a block over its cell, an underline under it, and
/// a bar on the cell's right edge where the character under it resolves
/// right to left (`[[source: terminal-wg-bidi-cursor]]`'s "at the very
/// least ... the right side"), wherever the order draws that cell. On an
/// ordinary row, and on a full row in the pending-wrap state, where the
/// cursor's cell is the row's last logical column. A full row of Latin
/// letters is the control, its cursor on its last visual column with the
/// bar on the left edge.
fn cursor_shapes(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let cols = usize::from(WS.ws_col);
    // A full row: as many letters as columns, the last one written leaving
    // the cursor in the pending-wrap state on the last logical column.
    let full = |first: u32, n: u32| -> String {
        (0..cols)
            .map(|i| {
                char::from_u32(first + u32::try_from(i).expect("a column") % n).expect("a letter")
            })
            .collect()
    };
    let hebrew = full(0x5D0, 27);
    let latin = full(u32::from('a'), 26);
    // (what, the scene with the cursor placed, the row, the visual column
    // it must be drawn over, whether its character runs right to left)
    let ordinary = "\x1b[2;3Habc \u{5E9}\u{5DC}\u{5D5}\u{5DD} def\x1b[2;8H".to_string();
    let scenes = [
        ("an ordinary row", ordinary, 1, 8, true),
        ("a full row", format!("\x1b[4;1H{hebrew}"), 3, 0, true),
        (
            "a full row of Latin",
            format!("\x1b[4;1H{latin}"),
            3,
            cols - 1,
            false,
        ),
    ];
    for (what, text, row, visual, rtl) in &scenes {
        for (style, shape) in [("2", "block"), ("4", "underline"), ("6", "bar")] {
            let at = format!("{arm:?}: {what}, {shape}");
            let mut owned = owned();
            let mut t = Term::new(owned.storage(), &WS);
            let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
            feed(
                &mut t,
                b"\x1b[?25l\x1b[2J\x1b[8;1HThe quick brown fox jumps 0123456789",
            );
            feed(&mut t, format!("\x1b[{style} q{text}").as_bytes());
            assert_eq!(t.cursor().0, Row::from_index(*row), "{at}");
            if what.starts_with("a full row") {
                assert_eq!(t.wrap_pending(), WrapPending::Yes, "{at}: the premise");
                assert_eq!(t.cursor().1, Col::at_index(cols - 1), "{at}");
            }
            let hidden = arms.check(&mut t, &format!("{at}, hidden"));
            feed(&mut t, b"\x1b[?25h");
            if what.starts_with("a full row") {
                assert_eq!(t.wrap_pending(), WrapPending::Yes, "{at}: still pending");
            }
            let shown = arms.check(&mut t, &at);
            assert_eq!(
                arms.renderer
                    .bidi()
                    .visual_col(Row::from_index(*row), t.cursor().1),
                Col::at_index(*visual),
                "{at}: the cell's drawn column"
            );
            let (w, cw, ch) = geometry(&arms);
            let band = row * ch..(row + 1) * ch;
            let (left, right) = (visual * cw, (visual + 1) * cw);
            on_paths(&shown, &hidden, |path, a, b| {
                let changed = changed_at(a, b, w);
                assert!(!changed.is_empty(), "{at}, {path}: drawn");
                assert!(
                    changed
                        .iter()
                        .all(|&(x, y)| x >= left && x < right && band.contains(&y)),
                    "{at}, {path}: inside visual column {visual} of row {row}"
                );
                let xs = |x: usize| changed.iter().any(|&(cx, _)| cx == x);
                match shape {
                    "block" => assert!(
                        xs(left) && xs(right - 1),
                        "{at}, {path}: the block covers its cell"
                    ),
                    "underline" => {
                        assert!(xs(left) && xs(right - 1), "{at}, {path}: across its cell");
                        assert!(
                            changed.iter().all(|&(_, y)| y >= band.start + ch / 2),
                            "{at}, {path}: under it, in the cell's lower half"
                        );
                    }
                    _ => {
                        let (on, off) = match rtl {
                            true => (right - 1, left),
                            false => (left, right - 1),
                        };
                        assert!(
                            xs(on),
                            "{at}, {path}: the bar on the side its letter runs from"
                        );
                        assert!(!xs(off), "{at}, {path}: and not on the other");
                    }
                }
            });
        }
    }
}

#[test]
fn the_cursor_on_a_right_to_left_row_is_drawn_in_its_shape_on_every_arm() {
    let gpu = lavapipe();
    cursor_shapes(&gpu, Arm::InPlace);
    cursor_shapes(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_cursor_on_a_right_to_left_row_is_drawn_in_its_shape_on_every_arm_on_radv() {
    let gpu = radv();
    cursor_shapes(&gpu, Arm::Staging);
    cursor_shapes(&gpu, gpu.arm(1));
}

/// `frame`'s cell at `(row, col)` flipped left to right, pixel row by
/// pixel row, in a frame of `geom`.
fn flip_cell(frame: &mut [u32], (w, cw, ch): (usize, usize, usize), (row, col): (usize, usize)) {
    for y in row * ch..(row + 1) * ch {
        let at = y * w + col * cw;
        frame[at..at + cw].reverse();
    }
}

/// **A mirrored character resolved right to left is drawn mirrored on
/// every arm** (UAX #9 L4), on `arm`: U+2211, one of the Bidi_Mirrored
/// characters with no Bidi_Mirroring_Glyph, which BidiMirroring.txt leaves
/// "up to the rendering system to provide mirrored glyphs" for
/// (`[[source: unicode-bidimirroring]]`), draws its own glyph flipped left
/// to right in its cell, as U+221A does; U+2203 carrying U+0338 draws the
/// glyph the face composes flipped; and `<` carrying U+0338, whose base has
/// a mirroring glyph, draws `>` under the mark. Each is after an alef, at
/// the end of a right-to-left paragraph, so N2 gives it R. The expected
/// frame is the same rows written in their visual order with BDSM reset,
/// the flipped cells mirrored in each path's frame by hand. A mark on a
/// flipped base is not flipped.
fn mirrored(gpu: &Gpu, arm: Arm) {
    let (built, _) = rtl_faces();
    let faces = [Face::parse(&built).expect("the built face")];
    let cols = usize::from(WS.ws_col);
    let alef = "\u{5D0}";
    // Each row as written with BDSM set, from column 1, and as drawn,
    // written with it reset two cells from the right edge; and whether its
    // first cell there is drawn flipped.
    let rows: [(String, String, bool); 4] = [
        (format!("{alef}\u{2211}"), format!("\u{2211}{alef}"), true),
        (format!("{alef}\u{221A}"), format!("\u{221A}{alef}"), true),
        (
            format!("{alef}\u{2203}\u{338}"),
            format!("\u{2203}\u{338}{alef}"),
            true,
        ),
        (format!("{alef}<\u{338}"), format!(">\u{338}{alef}"), false),
    ];
    let scene = |t: &mut Term<'_>, implicit: bool| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
        feed(t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
        for (i, (logical, visual, _)) in rows.iter().enumerate() {
            let (text, col) = match implicit {
                true => (logical, 1),
                false => (visual, cols - 1),
            };
            feed(t, format!("\x1b[{};{col}H{text}", 2 * i + 2).as_bytes());
        }
        feed(t, b"\x1b[10;3HThe quick brown fox jumps 0123456789");
    };
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    scene(&mut t, true);
    let got = arms.check(&mut t, &format!("{arm:?}: mirrored"));
    assert!(arms.renderer.bidi().any(), "the rows were reordered");
    scene(&mut t, false);
    let plain = arms.check(&mut t, &format!("{arm:?}: written in visual order"));
    let g = geometry(&arms);
    let (w, _, ch) = g;
    let col = cols - 2;
    on_paths(&got, &plain, |path, got, plain| {
        let mut want = plain.to_vec();
        for (i, (_, _, flipped)) in rows.iter().enumerate() {
            if *flipped {
                flip_cell(&mut want, g, (2 * i + 1, col));
            }
        }
        // The control: each flipped cell is not its own mirror, so its
        // row tests the flip.
        for (i, (logical, _, flipped)) in rows.iter().enumerate() {
            let band = (2 * i + 1) * ch * w..(2 * i + 2) * ch * w;
            assert!(
                !flipped || differing(&want[band.clone()], &plain[band]) > 0,
                "{path}, the control: {logical:?}'s glyph is not its own mirror"
            );
        }
        assert_eq!(
            diff_box(got, &want, g),
            None,
            "{path}: each mirrored character drawn mirrored"
        );
    });
    // **A mark on a flipped base is not flipped** (L4 mirrors a character
    // "if and only if" it is Bidi_Mirrored, which U+0301 is not): U+2211
    // carrying an acute, which the face draws as two glyphs, its base
    // mirrored and its acute neither left as drawn nor mirrored with the
    // base.
    let marked = |t: &mut Term<'_>, implicit: bool| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
        feed(t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
        let text = match implicit {
            true => format!("\x1b[2;1H{alef}\u{2211}\u{301}"),
            false => format!("\x1b[2;{}H\u{2211}\u{301}{alef}", cols - 1),
        };
        feed(t, text.as_bytes());
        feed(t, b"\x1b[10;3HThe quick brown fox jumps 0123456789");
    };
    marked(&mut t, true);
    let got = arms.check(&mut t, &format!("{arm:?}: a flipped base with a mark"));
    marked(&mut t, false);
    let plain = arms.check(&mut t, &format!("{arm:?}: a base with a mark, unflipped"));
    feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8l");
    feed(
        &mut t,
        format!("\x1b[2;{}H\u{2211}{alef}", cols - 1).as_bytes(),
    );
    feed(&mut t, b"\x1b[10;3HThe quick brown fox jumps 0123456789");
    let bare = arms.check(&mut t, &format!("{arm:?}: the base alone, unflipped"));
    let (_, cw, _) = g;
    for (((path, got), (_, plain)), (_, bare)) in
        paths(&got).into_iter().zip(paths(&plain)).zip(paths(&bare))
    {
        let mut whole = plain.to_vec();
        flip_cell(&mut whole, g, (1, col));
        assert!(differing(got, plain) > 0, "{path}: the base is flipped");
        assert!(differing(got, &whole) > 0, "{path}: the acute is not");
        // The base mirrored: in the cell's pixel rows the acute leaves
        // alone, where the base drawn with it and the base drawn alone
        // agree, the flipped cell is the bare U+2211 mirrored.
        let line = |f: &[u32], y: usize| f[y * w + col * cw..y * w + (col + 1) * cw].to_vec();
        let (mut alone, mut tested) = (0, 0);
        for y in ch..2 * ch {
            if line(plain, y) != line(bare, y) {
                continue;
            }
            alone += 1;
            let mut flipped = line(bare, y);
            flipped.reverse();
            tested += usize::from(flipped != line(bare, y));
            assert_eq!(
                line(got, y),
                flipped,
                "{path}: the base mirrored, pixel row {y}"
            );
        }
        assert!(alone < ch, "{path}, the control: the acute inks a row");
        assert!(
            tested > 0,
            "{path}, the control: a row only the base inks is not its own mirror"
        );
    }
}

#[test]
fn mirrored_characters_draw_mirrored_on_every_arm() {
    let gpu = lavapipe();
    mirrored(&gpu, Arm::InPlace);
    mirrored(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn mirrored_characters_draw_mirrored_on_every_arm_on_radv() {
    let gpu = radv();
    mirrored(&gpu, Arm::Staging);
    mirrored(&gpu, gpu.arm(1));
}

/// The glyphs every scene below draws beside its own, on row 8, after a
/// reset, with the cursor hidden.
const PRELUDE: &[u8] = b"\x1bc\x1b[?25l\x1b[8;1HThe quick brown fox jumps 0123456789";

/// **A reversed string draws alike on every arm**, on `arm`: with BDSM
/// reset an SRS string is drawn in reverse inside its line
/// (`[[source: ecma48]]` 8.3.137), the same pixels as the same text
/// written in its visual order; the control, the text with no string,
/// differs.
fn reversed_string(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    feed(&mut t, PRELUDE);
    feed(&mut t, b"\x1b[8l\x1b[2;1Hab\x1b[1[cde\x1b[0[f\x1b[4;1H");
    let got = arms.check(&mut t, &format!("{arm:?}: SRS, explicit"));
    feed(&mut t, PRELUDE);
    feed(&mut t, b"\x1b[8l\x1b[2;1Habedcf\x1b[4;1H");
    let want = arms.check(&mut t, &format!("{arm:?}: written in visual order"));
    same(&got, &want, "the reversed string drawn");
    feed(&mut t, PRELUDE);
    feed(&mut t, b"\x1b[8l\x1b[2;1Habcdef\x1b[4;1H");
    let stored = arms.check(&mut t, &format!("{arm:?}: no string"));
    apart(&stored, &want, "the control: as written");
}

#[test]
fn a_reversed_string_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    reversed_string(&gpu, Arm::InPlace);
    reversed_string(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_reversed_string_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    reversed_string(&gpu, Arm::Staging);
    reversed_string(&gpu, gpu.arm(1));
}

/// **A row DECRLM wrote draws alike on every arm**, on `arm`: with BDSM set
/// it is drawn as its cells hold (`[[source: vt510-decrlm]]`), the same
/// pixels as the same cells drawn with BDSM reset; the control, the same
/// cells written in order with BDSM set, is reordered by UAX #9.
fn leftward(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let cols = usize::from(WS.ws_col);
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    feed(&mut t, PRELUDE);
    feed(
        &mut t,
        format!("\x1b[?34h\x1b[2;{cols}H\u{5D0}\u{5D1} ab\x1b[?34l\x1b[4;1H").as_bytes(),
    );
    let got = arms.check(&mut t, &format!("{arm:?}: DECRLM, implicit"));
    feed(&mut t, PRELUDE);
    feed(
        &mut t,
        format!("\x1b[8l\x1b[2;{}Hba \u{5D1}\u{5D0}\x1b[4;1H", cols - 4).as_bytes(),
    );
    let want = arms.check(
        &mut t,
        &format!("{arm:?}: the cells drawn as stored, BDSM reset"),
    );
    same(&got, &want, "the row drawn as its cells hold");
    feed(&mut t, PRELUDE);
    feed(
        &mut t,
        format!("\x1b[2;{}Hba \u{5D1}\u{5D0}\x1b[4;1H", cols - 4).as_bytes(),
    );
    let reordered = arms.check(&mut t, &format!("{arm:?}: the same cells written in order"));
    apart(
        &reordered,
        &want,
        "the control: UAX #9 reorders the cells written in order",
    );
}

#[test]
fn a_leftward_row_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    leftward(&gpu, Arm::InPlace);
    leftward(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_leftward_row_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    leftward(&gpu, Arm::Staging);
    leftward(&gpu, gpu.arm(1));
}

/// **A line SCP turns draws alike on every arm** (`[[source: ecma48]]`
/// 8.3.111), on `arm`: with BDSM set, an all-Latin line SCP 2 sets right to
/// left is drawn as the same text written in its visual order against the
/// right edge; `?2500` draws a box on an explicit right-to-left line
/// closed; with BDSM reset a line SCP 2 sets is drawn in reverse, its
/// bracket mirrored and its wide character's cells in order
/// (`[[source: terminal-wg-bidi-basic-modes]]`), before and after the
/// renderer has seen a joining letter, which moves the cell path onto the
/// kept layout; and a bar cursor there is drawn on its cell's right side.
fn scp_turns(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let cols = usize::from(WS.ws_col);
    let prelude = |t: &mut Term<'_>, joining: bool| {
        feed(t, PRELUDE);
        if joining {
            feed(t, "\x1b[6;1H\u{628}\u{628}\u{628}".as_bytes());
        }
    };
    // Implicit: `abc def!` at level 1 is `!abc def` at the right edge,
    // with autodetection off, under which the path is the level.
    {
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        prelude(&mut t, false);
        feed(&mut t, b"\x1b[2;1H\x1b[?2501l\x1b[2 kabc def!\x1b[4;1H");
        let got = arms.check(&mut t, &format!("{arm:?}: SCP 2, implicit"));
        prelude(&mut t, false);
        feed(&mut t, format!("\x1b[2;{}H!abc def", cols - 7).as_bytes());
        let want = arms.check(&mut t, &format!("{arm:?}: written in visual order"));
        same(&got, &want, "the visual order drawn");
        prelude(&mut t, false);
        feed(&mut t, b"\x1b[2;1Habc def!");
        let stored = arms.check(&mut t, &format!("{arm:?}: first strong"));
        apart(&stored, &want, "the control: first strong");
    }
    // `?2500`: a box on an explicit right-to-left line, reversed, is drawn
    // closed, each corner as the other side's.
    {
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        prelude(&mut t, false);
        feed(
            &mut t,
            "\x1b[8l\x1b[2;1H\x1b[?2500h\x1b[2 k\u{250C}\u{2500}\u{2510}\x1b[4;1H".as_bytes(),
        );
        let got = arms.check(&mut t, &format!("{arm:?}: ?2500, explicit"));
        prelude(&mut t, false);
        feed(
            &mut t,
            format!(
                "\x1b[8l\x1b[2;{}H\u{250C}\u{2500}\u{2510}\x1b[4;1H",
                cols - 2
            )
            .as_bytes(),
        );
        let want = arms.check(&mut t, &format!("{arm:?}: the box in visual order"));
        same(&got, &want, "the box drawn closed");
        prelude(&mut t, false);
        feed(
            &mut t,
            "\x1b[8l\x1b[2;1H\x1b[2 k\u{250C}\u{2500}\u{2510}\x1b[4;1H".as_bytes(),
        );
        let open = arms.check(&mut t, &format!("{arm:?}: without ?2500"));
        apart(&open, &want, "the control: the corners face out");
    }
    // Explicit: `ab(` and a wide character, reversed whole.
    for joining in [false, true] {
        let what = |s: &str| format!("{arm:?}: {s}, joining letter seen: {joining}");
        let mut owned = owned();
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        // Twice where a joining letter is on the screen: the first frame
        // sees it, and the second is drawn by the kept layout.
        let draw = |arms: &mut Arms<'_, '_, '_>, t: &mut Term<'_>, s: &str| {
            if joining {
                arms.check(t, &what(&format!("{s}, first")));
            }
            arms.check(t, &what(s))
        };
        prelude(&mut t, joining);
        feed(&mut t, "\x1b[8l\x1b[2;1H\x1b[2 kab(\u{4E00}".as_bytes());
        let got = draw(&mut arms, &mut t, "SCP 2, explicit");
        assert!(arms.renderer.bidi().reordered(Row::from_index(1)));
        // The bar cursor on `a`, drawn at the right edge on its right side.
        feed(&mut t, b"\x1b[6 q\x1b[2;1H\x1b[?25h");
        let bar = draw(&mut arms, &mut t, "SCP 2, explicit, bar");
        let (w, cw, ch) = geometry(&arms);
        let (left, right) = ((cols - 1) * cw, cols * cw);
        on_paths(&bar, &got, |path, a, b| {
            let changed = changed_at(a, b, w);
            assert!(
                changed
                    .iter()
                    .all(|&(x, y)| x >= left && x < right && (ch..2 * ch).contains(&y)),
                "{}, {path}",
                what("the bar in the last column")
            );
            let xs = |x: usize| changed.iter().any(|&(cx, _)| cx == x);
            assert!(
                xs(right - 1) && !xs(left),
                "{}, {path}",
                what("on the right side")
            );
        });
        prelude(&mut t, joining);
        feed(
            &mut t,
            format!("\x1b[8l\x1b[2;{}H\u{4E00})ba", cols - 4).as_bytes(),
        );
        let want = draw(&mut arms, &mut t, "written in visual order");
        same(&got, &want, &what("the reverse drawn"));
    }
}

#[test]
fn a_line_scp_turns_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    scp_turns(&gpu, Arm::InPlace);
    scp_turns(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_line_scp_turns_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    scp_turns(&gpu, Arm::Staging);
    scp_turns(&gpu, gpu.arm(1));
}

/// **A paragraph inside DECSLRM side margins draws alike on every arm**,
/// on `arm`: twenty Hebrew letters and "12" written inside margins 101 to
/// 120, the digits wrapped to the next line's left margin, with "xy" and
/// "zw" left of the margins, are laid out inside them as one paragraph and
/// the letters outside them where they were written
/// (`[[source: vt510-decslrm]]`, `[[source: uax9-bidi]]`) -- the same
/// pixels as the same cells written in that visual order with BDSM reset.
/// The control: the same letters written with no margins, each row read
/// whole, draws otherwise. Written from the bottom margin, the last row,
/// so the wrap scrolls the rectangle, the paragraph draws so on the last
/// two rows.
fn side_margins(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let he: String = (0..20)
        .map(|i| char::from_u32(0x5D0 + i).expect("a letter"))
        .collect();
    let back: String = he.chars().rev().collect();
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[?25l\x1b[?69h\x1b[101;120s\x1b[1;101H{he}12\x1b[1;1Hxy\x1b[2;1Hzw\x1b[4;1H"
        )
        .as_bytes(),
    );
    let got = arms.check(&mut t, &format!("{arm:?}: inside the margins"));
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[?25l\x1b[8l\x1b[1;1Hxy\x1b[1;101H{back}\x1b[2;1Hzw\x1b[2;119H12\x1b[4;1H"
        )
        .as_bytes(),
    );
    let want = arms.check(&mut t, &format!("{arm:?}: the visual order, BDSM reset"));
    same(&got, &want, "laid out inside the margins");
    feed(
        &mut t,
        format!("\x1bc\x1b[?25l\x1b[1;101H{he}\x1b[2;101H12\x1b[1;1Hxy\x1b[2;1Hzw\x1b[4;1H")
            .as_bytes(),
    );
    let whole = arms.check(&mut t, &format!("{arm:?}: no margins"));
    apart(&whole, &want, "the control: read whole");
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[?25l\x1b[?69h\x1b[101;120s\x1b[10;101H{he}12\x1b[9;1Hxy\x1b[10;1Hzw\x1b[1;1H"
        )
        .as_bytes(),
    );
    let scrolled = arms.check(&mut t, &format!("{arm:?}: scrolled inside the margins"));
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[?25l\x1b[8l\x1b[9;1Hxy\x1b[9;101H{back}\x1b[10;1Hzw\x1b[10;119H12\x1b[1;1H"
        )
        .as_bytes(),
    );
    let want = arms.check(
        &mut t,
        &format!("{arm:?}: the visual order on the last two rows"),
    );
    same(&scrolled, &want, "scrolled with its margins");
}

#[test]
fn a_paragraph_inside_side_margins_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    side_margins(&gpu, Arm::InPlace);
    side_margins(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_paragraph_inside_side_margins_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    side_margins(&gpu, Arm::Staging);
    side_margins(&gpu, gpu.arm(1));
}

/// **The cell path with BDSM reset finds a row's joins again only when the
/// copy's compare finds the row changed** (`Screens::take`), on `arm`:
/// once the renderer has shaped a letter that takes a joining form, a cell
/// frame whose screen is the last's reads no row for its joins, a frame
/// that changed one row tests that row alone, and the joins it keeps are
/// always the ones a fresh layout finds. Each frame is the cell frame's
/// own build ([`Arms::build`]); the last screen is then held to the bars.
fn rejoins(gpu: &Gpu, arm: Arm) {
    let (built, _) = rtl_faces();
    let faces = [Face::parse(&built).expect("the built face")];
    let rows = u64::from(WS.ws_row);
    let mut owned = owned();
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    // One cell frame built: what the layout read, and whether its joins
    // are a fresh layout's.
    let frame = |arms: &mut Arms<'_, '_, '_>, t: &mut Term<'_>| {
        arms.build(t);
        let r = &arms.renderer;
        let mut fresh = baud_render::Reorder::new();
        fresh.set_reach(r.joined_reach());
        fresh.lay_out(t, Rows::ZERO);
        for y in 0..usize::from(WS.ws_row) {
            let row = Row::from_index(y);
            assert_eq!(
                r.bidi().row_joins(row),
                fresh.orders().row_joins(row),
                "row {y}: the kept joins are not the screen's"
            );
        }
        let w = r.bidi_work();
        (w.gens.value(), w.tested.value())
    };
    feed(
        &mut t,
        "\x1b[?25l\x1b[8l\x1b[3;1H\u{0628}\u{0628}".as_bytes(),
    );
    arms.renderer
        .convert(&t, Rows::ZERO, baud_render::Damage::Full);
    assert_eq!(
        arms.renderer.joins_seen(),
        baud_render::Joins::Seen,
        "a beh was shaped"
    );
    assert_eq!(
        frame(&mut arms, &mut t),
        (0, rows),
        "after a conversion: every row"
    );
    assert_eq!(frame(&mut arms, &mut t), (0, 0), "the same screen");
    assert_eq!(frame(&mut arms, &mut t), (0, 0), "and again");
    feed(&mut t, b"\x1b[1;1Hj");
    assert_eq!(frame(&mut arms, &mut t), (rows, 1), "one Latin row changed");
    feed(&mut t, b"\x1b[3;1Hxx");
    assert_eq!(
        frame(&mut arms, &mut t),
        (rows, 1),
        "the Arabic row made Latin"
    );
    feed(&mut t, "\x1b[5;1H\u{0628}\u{0628}\u{0628}".as_bytes());
    assert_eq!(
        frame(&mut arms, &mut t),
        (rows, 1),
        "a Latin row made Arabic"
    );
    assert!(
        arms.renderer.bidi().joins(Row::from_index(4)).is_some(),
        "its behs joined"
    );
    assert_eq!(
        frame(&mut arms, &mut t),
        (0, 0),
        "the same screen, a row joined"
    );
    arms.renderer
        .convert(&t, Rows::ZERO, baud_render::Damage::Incremental);
    assert_eq!(
        frame(&mut arms, &mut t),
        (0, rows),
        "after a conversion again"
    );
    assert_eq!(frame(&mut arms, &mut t), (0, 0), "and the same screen");
    // The joined row drawn: its behs and the Latin rows, on every path.
    feed(&mut t, b"\x1b[8;1HThe quick brown fox jumps 0123456789");
    let joined = arms.check(&mut t, &format!("{arm:?}: a joined row"));
    feed(&mut t, b"\x1b[5;1H\x1b[2K");
    let erased = arms.check(&mut t, &format!("{arm:?}: the joined row erased"));
    apart(&joined, &erased, "the control: the joined row is drawn");
}

#[test]
fn the_cell_path_rejoins_only_the_rows_the_copy_finds_changed() {
    let gpu = lavapipe();
    rejoins(&gpu, Arm::InPlace);
    rejoins(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_cell_path_rejoins_only_the_rows_the_copy_finds_changed_on_radv() {
    let gpu = radv();
    rejoins(&gpu, Arm::Staging);
    rejoins(&gpu, gpu.arm(1));
}
