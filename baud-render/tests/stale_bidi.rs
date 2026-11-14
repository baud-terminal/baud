//! **Every input the kept bidirectional layout depends on is drawn the
//! next frame** (`baud_render::Reorder`, kept a paragraph at a time while
//! no row it read was written). Each case draws a full frame and two
//! incremental ones, checks its control (the operation changes what a
//! fresh renderer draws), checks its isolation against the seal taken just
//! before it (what it wrote and did not), applies it, and draws two
//! incremental frames, each equal to a fresh renderer's full frame, pixels
//! and grounds, with the kept order equal to the fresh renderer's.

mod support;

use baud_core::{BidiSupport, Owned, Term};
use baud_domain::{
    BytePos, Codepoint, Col, Cols, ReadLen, Row, Rows, Seal, WinSize, WindowAt, Wrap,
};
use baud_font::Face;
use baud_render::{
    Blend, Caret, Composition, Current, Damage, Field, FindLook, Ground, Hints, Joins, LinkLook,
    LinkSpan, Mark, Measure, Orders, PxSize, Renderer, Reorder, Stack, Target, Theme, WindowLine,
    WordWrap,
};
use support::font;

const COLS: usize = 40;
const ROWS: usize = 10;
const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

const HIDE: &str = "\x1b[?25l";

const PROMPT: &str = "\x1b]133;A\x07$ ";

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for read in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
}

/// Forty columns of digits and spaces: no strong letter.
fn digits() -> String {
    format!("{}4", "12 ".repeat(13))
}

/// Forty columns whose only strong letters are Hebrew, first.
fn hebrew_row() -> String {
    format!("\u{5d0}\u{5d1}\u{5d2} {}", "1 ".repeat(18))
}

/// Paragraph A on rows 2-3, row 2 digits and spaces laid out right to left
/// by row 3's Hebrew (UAX #9 P2-P3); a Latin row 4; paragraph B, Hebrew
/// wrapped over rows 6-7.
fn two_paragraphs() -> String {
    let b = "\u{5d0}\u{5d1}\u{5d2}\u{5d3} ".repeat(9);
    format!(
        "{HIDE}\x1b[3;1H{}\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4}\x1b[5;1Hplain latin\x1b[7;1H{b}",
        digits()
    )
}

/// Rows 3-6 one paragraph whose only strong letter is row 3's Hebrew.
fn four_rows() -> String {
    format!(
        "{HIDE}\x1b[4;1H{}{}{}12 34",
        hebrew_row(),
        digits(),
        digits()
    )
}

/// The faces every case draws with, and on macOS one that shapes Arabic.
fn faces_data() -> Vec<Vec<u8>> {
    vec![font("JetBrainsMono-Regular.ttf")]
}

/// What a renderer of a case is set to beyond the screen: its size, a
/// composition, and whether it shows a find mark and a link underline.
#[derive(Clone)]
struct Setting {
    size: f32,
    wrap: WordWrap,
    composition: Option<(Row, Col, &'static str)>,
    marks: Marks,
}

/// Whether a renderer shows a find mark and a link underline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Marks {
    None,
    Shown,
}

fn made<'f, 'a>(faces: &'f [Face<'a>], s: &Setting, t: &Term<'_>) -> Renderer<'f, 'a> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(COLS),
        Rows::of_index(ROWS),
    )
    .unwrap();
    apply(&mut r, s, t);
    r
}

/// The find field on the last row, a mark on row 2's first five columns,
/// and a link underline under columns 3-8 of row 2.
fn looks() -> (FindLook, LinkLook) {
    let field = Field {
        row: Row::from_index(ROWS - 1),
        col: Col::ZERO,
        text: vec![Codepoint::from_char('F')],
        hints: vec![Hints::default()],
        caret: Col::at_index(1),
    };
    let find = FindLook {
        marks: vec![Mark {
            line: WindowLine::of(Row::from_index(2)),
            from: Col::ZERO,
            to: Col::at_index(5),
            current: Current::No,
        }],
        field: field.clone(),
    };
    let link = LinkLook {
        spans: vec![LinkSpan {
            row: Row::from_index(2),
            from: Col::at_index(3),
            to: Col::at_index(9),
        }],
        bar: field,
    };
    (find, link)
}

fn apply(r: &mut Renderer<'_, '_>, s: &Setting, t: &Term<'_>) {
    if (s.size - 14.0).abs() > f32::EPSILON {
        r.rescale(PxSize::of(s.size).unwrap()).unwrap();
    }
    r.set_word_wrap(s.wrap);
    apply_composition(r, s.composition);
    if s.marks == Marks::Shown {
        let (find, link) = looks();
        r.set_find(t, Some(find));
        r.set_link(Some(link));
    }
}

#[derive(Clone, PartialEq)]
struct Drawn {
    pixels: Vec<u32>,
    grounds: Vec<Ground>,
}

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

/// A fresh renderer's full frame, and its order after one more frame,
/// once whatever the first frame shaped is read.
fn fresh(faces: &[Face<'_>], s: &Setting, t: &mut Term<'_>, back: Rows) -> (Drawn, Orders) {
    let mut r = made(faces, s, t);
    let mut px = Vec::new();
    let full = draw(&mut r, t, back, &mut px, Damage::Full);
    let _ = draw(&mut r, t, back, &mut px, Damage::Incremental);
    (full, r.bidi().clone())
}

enum Step {
    Bytes(String),
    Wrap(WordWrap),
    Compose(Option<(Row, Col, &'static str)>),
    Rescale(f32),
}

impl Step {
    fn apply(&self, t: &mut Term<'_>, r: &mut Renderer<'_, '_>, s: &mut Setting) {
        match self {
            Step::Bytes(b) => feed(t, b.as_bytes()),
            Step::Wrap(w) => {
                s.wrap = *w;
                r.set_word_wrap(*w);
            }
            Step::Compose(c) => {
                s.composition = *c;
                apply_composition(r, *c);
            }
            Step::Rescale(size) => {
                s.size = *size;
                r.rescale(PxSize::of(*size).unwrap()).unwrap();
            }
        }
    }
}

fn apply_composition(r: &mut Renderer<'_, '_>, c: Option<(Row, Col, &'static str)>) {
    r.set_composition(c.map(|(row, col, text)| Composition {
        row,
        col,
        text: text.chars().map(Codepoint::from_char).collect(),
        measure: Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    }));
}

/// How a case's control knows the operation did what it is there for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shows {
    /// The picture changed.
    Drawn,
    /// Nothing: a round trip ends on the picture it began on.
    Same,
}

/// What a case's isolation reads: the seal taken before the operation, the
/// window's physical rows then, and how far it looks back.
struct Was {
    seal: Seal,
    at: WindowAt,
    back: Rows,
}

/// One input the kept layout depends on.
struct Case {
    what: &'static str,
    setup: String,
    back: Rows,
    setting: Setting,
    steps: Vec<Step>,
    shows: Shows,
    alone: fn(&Term<'_>, &Was) -> Result<(), String>,
}

const PLAIN: Setting = Setting {
    size: 14.0,
    wrap: WordWrap::On,
    composition: None,
    marks: Marks::None,
};

/// The window's rows written after `w`'s seal.
fn written(t: &Term<'_>, w: &Was) -> Vec<usize> {
    let g = t.row_gens(w.back);
    (0..g.window().rows.index())
        .filter(|&y| g.after(Row::from_index(y), &w.seal))
        .collect()
}

/// `Ok` where the isolation `what` holds, else what failed.
fn isolated(holds: Option<()>, what: &str) -> Result<(), String> {
    holds.ok_or_else(|| what.to_string())
}

/// Whether the window shows the physical rows it showed.
fn same_rows(t: &Term<'_>, w: &Was) -> bool {
    t.row_gens(w.back).window() == w.at
}

/// Nothing written since the seal.
fn untouched(t: &Term<'_>, w: &Was) -> Result<(), String> {
    isolated((!t.wrote_since(&w.seal)).then_some(()), "nothing written")
}

/// **One case on the CPU path.**
fn on_the_cpu_path(case: &Case, data: &[Vec<u8>]) {
    let faces: Vec<Face<'_>> = data.iter().map(|d| Face::parse(d).unwrap()).collect();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, case.setup.as_bytes());
    let back = case.back;
    let mut s = case.setting.clone();
    let mut r = made(&faces, &s, &t);
    let mut px = Vec::new();
    draw(&mut r, &mut t, back, &mut px, Damage::Full);
    for _ in 0..2 {
        draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
    }
    let (before, _) = fresh(&faces, &s, &mut t, back);
    let was = Was {
        seal: t.sealed(),
        at: t.row_gens(back).window(),
        back,
    };
    for (k, step) in case.steps.iter().enumerate() {
        if k > 0 {
            draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
        }
        step.apply(&mut t, &mut r, &mut s);
    }
    let (after, _) = fresh(&faces, &s, &mut t, back);
    match case.shows {
        Shows::Drawn => assert!(
            after.pixels != before.pixels,
            "{}: the control, the operation is drawn",
            case.what
        ),
        Shows::Same => assert!(
            after.pixels == before.pixels,
            "{}: the control, the round trip ends where it began",
            case.what
        ),
    }
    if let Err(e) = (case.alone)(&t, &was) {
        panic!("{}: the isolation, {e}", case.what);
    }
    let mut wrong = Vec::new();
    for frame in ["the frame after", "the frame after that"] {
        let now = draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
        let (full, order) = fresh(&faces, &s, &mut t, back);
        if now.pixels != full.pixels {
            wrong.push(format!("{frame}: pixels"));
        }
        if now.grounds != full.grounds {
            wrong.push(format!("{frame}: grounds"));
        }
        if *r.bidi() != order {
            wrong.push(format!("{frame}: the order"));
        }
    }
    assert!(wrong.is_empty(), "{} drawn stale: {wrong:?}", case.what);
}

fn run(case: Case) {
    on_the_cpu_path(&case, &faces_data());
}

/// **S1: a row another row of its paragraph orders is drawn again** (UAX
/// #9 P2-P3): row 2, digits and spaces, laid out right to left by row 3's
/// Hebrew; row 3's Hebrew written over in Latin. The isolation: row 3
/// alone written. Named break: only the written row laid out again, which
/// keeps row 2 reversed. Its W7 twin is `stale_rows.rs`'s
/// `a_row_another_row_reorders_is_drawn_again`.
#[test]
fn a_row_its_paragraphs_first_strong_letter_orders_is_drawn_again() {
    run(Case {
        what: "the paragraph's first strong letter",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[4;1Habc de".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![3]).then_some(()),
                "row 3 alone written",
            )
        },
    });
}

/// **S2: a split paragraph's lower part is drawn in its own order**: rows
/// 3-6 one paragraph, its only strong letter row 3's Hebrew, ended at row
/// 4 by a line feed from it. The isolation: row 4 alone written, and it no
/// longer continues. Named break: a unit laid out again only when a row of
/// its own was written, which keeps rows 5-6 reversed.
#[test]
fn a_split_paragraphs_lower_part_is_drawn_in_its_own_order() {
    run(Case {
        what: "a split paragraph",
        setup: four_rows(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[5;1H\n".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![4]
                    && t.view(w.back).wrapped(Row::from_index(4)) == Wrap::Ended)
                    .then_some(()),
                "row 4 alone written, and ended",
            )
        },
    });
}

/// **S3: the line above the window a paragraph continues from is read**:
/// the window's first row continues a paragraph whose only strong letter is
/// Hebrew on the line above it, in the history, which `CSI ? 3 J` erases in
/// place. The isolation: no window row written, the history line written,
/// and still held. Named break: the rows above not read.
#[test]
fn a_paragraphs_line_above_the_window_is_read() {
    run(Case {
        what: "a line above the window",
        setup: format!(
            "{HIDE}\x1b[1;1H{}{}12 34\x1b[10;1H\n",
            hebrew_row(),
            digits()
        ),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[?3J".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            let one = Rows::of_index(1);
            isolated(
                (written(t, w).is_empty()
                    && t.looking_back(one) == one
                    && t.row_gens(one).after(Row::ZERO, &w.seal))
                .then_some(()),
                "no window row written, the line above written in place",
            )
        },
    });
}

/// **S4: the line below a window held back a paragraph runs onto is
/// read**: the window two lines back, its last row's paragraph running
/// onto the row below it, whose Hebrew is its only strong letter; that row
/// written Latin. The isolation: no window row written, the row below it
/// written. Named break: the rows below not read.
#[test]
fn a_paragraphs_line_below_the_window_is_read() {
    run(Case {
        what: "a line below the window",
        setup: format!(
            "{HIDE}\x1b[10;1H\n\n\n\x1b[8;1H{}\u{5d0}\u{5d1}\u{5d2}",
            digits()
        ),
        back: Rows::of_count(2),
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[9;1Habc".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w).is_empty()
                    && t.row_gens(Rows::of_count(1))
                        .after(Row::from_index(ROWS - 1), &w.seal))
                .then_some(()),
                "no window row written, the row below it written",
            )
        },
    });
}

/// **S5: a scroll under a window held back is drawn**: the window five
/// lines back over rows that alternate Hebrew and Latin; a line feed at the
/// live bottom moves it one physical row and writes none of the rows it
/// then shows. Named break: the window's physical rows out of the key.
#[test]
fn a_scroll_under_a_held_window_is_drawn() {
    let mut setup = String::from(HIDE);
    for i in 0..20 {
        match i % 2 {
            0 => setup.push_str(&format!("\u{5d0}\u{5d1} 12 ab {i}\r\n")),
            _ => setup.push_str(&format!("line {i}\r\n")),
        }
    }
    run(Case {
        what: "a scroll under a held window",
        setup,
        back: Rows::of_count(5),
        setting: PLAIN,
        steps: vec![Step::Bytes("\r\n".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w).is_empty() && !same_rows(t, w)).then_some(()),
                "no row of the new window written, the window moved",
            )
        },
    });
}

/// **S6: each line property a row's mark holds is drawn**: DECRLM and a
/// letter, and DECDWL, on the Latin row; `?2501` reset at paragraph A's
/// first position, which with no path selected lays A out left to right
/// outright. The isolation: a window row written by each. Named break: an
/// answer from the key alone, no row's generation read.
#[test]
fn each_line_property_is_drawn() {
    let steps = [
        ("DECRLM and a letter", "\x1b[5;20H\x1b[?34hx\x1b[?34l"),
        ("DECDWL", "\x1b[5;1H\x1b#6"),
        ("?2501 reset at a paragraph's start", "\x1b[3;1H\x1b[?2501l"),
    ];
    for (what, bytes) in steps {
        run(Case {
            what,
            setup: two_paragraphs(),
            back: Rows::ZERO,
            setting: PLAIN,
            steps: vec![Step::Bytes(bytes.into())],
            shows: Shows::Drawn,
            alone: |t, w| {
                isolated(
                    (!written(t, w).is_empty()).then_some(()),
                    "a window row written",
                )
            },
        });
    }
}

/// **S7: BDSM, which writes no window row, is drawn**: reset over a screen
/// laid out right to left, and set over one laid out with it reset. The
/// isolation: the mode changed and no window row was written (a feed
/// borrows the screen to write, so a surface reads as written). No break
/// of the key reaches this case alone: here a change of BDSM also moves
/// whether joins are read, or, once a joining letter is shaped, the pass
/// (`Reorder::keep_displayed` with it reset), so `bidi_kept.rs`'s U6 holds
/// BDSM's place in the key.
#[test]
fn bdsm_is_drawn() {
    run(Case {
        what: "BDSM reset",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[8l".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (t.bidi_support() == BidiSupport::Explicit && written(t, w).is_empty())
                    .then_some(()),
                "BDSM reset, no window row written",
            )
        },
    });
    run(Case {
        what: "BDSM set",
        setup: format!("\x1b[8l{}", two_paragraphs()),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[8h".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (t.bidi_support() == BidiSupport::Implicit && written(t, w).is_empty())
                    .then_some(()),
                "BDSM set, no window row written",
            )
        },
    });
}

/// **S8: the status line's own layout follows its plane**: a host-written
/// status line holding Hebrew, written Latin under DECSASD. The isolation:
/// its plane written, no window row. Named break: the status line kept on
/// its key alone.
#[test]
fn the_status_lines_layout_follows_its_plane() {
    let setup = format!(
        "{}\x1b[2$~\x1b[1$}}\u{5d0}\u{5d1}\u{5d2} 12 ab\x1b[0$}}",
        two_paragraphs()
    );
    run(Case {
        what: "the status line written",
        setup,
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[1$}\rxyz\x1b[0$}".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w).is_empty() && t.status_gen().is_some_and(|g| g.after(&w.seal)))
                    .then_some(()),
                "the status line written, no window row",
            )
        },
    });
}

/// **S10: the join reach is part of what a layout keeps**: a joined Arabic
/// paragraph laid out with no reach, then with a reach of two, nothing
/// written between. The control: a fresh layout's maps differ between the
/// two reaches. Named break: the reach out of the key.
#[test]
fn the_join_reach_is_kept_by() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        "\u{628}\u{628}\u{628} \u{628}\u{628}\u{628}".as_bytes(),
    );
    let laid = |reach: u32, t: &Term<'_>| {
        let mut o = Reorder::new();
        o.set_reach(baud_font::bytes::Count::of(reach));
        o.lay_out(t, Rows::ZERO);
        o.orders().clone()
    };
    assert!(
        laid(0, &t) != laid(2, &t),
        "the control: the reach is drawn"
    );
    let mut o = Reorder::new();
    let _ = t.seal_writes();
    o.lay_out(&t, Rows::ZERO);
    let seal = t.sealed();
    let _ = t.seal_writes();
    o.set_reach(baud_font::bytes::Count::of(2u32));
    o.lay_out(&t, Rows::ZERO);
    assert!(!t.wrote_since(&seal), "the isolation: nothing written");
    assert!(*o.orders() == laid(2, &t), "the kept maps at a new reach");
}

/// **S11: a display held is drawn when it is made again or let go**: word
/// wrap on over a right-to-left paragraph the display re-breaks; a cell of
/// it written, which re-breaks it elsewhere, and, separately, word wrap
/// turned off. Named break: the display out of the key.
#[test]
fn a_held_display_is_drawn_when_made_again_or_let_go() {
    let words = "\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4} ".repeat(10);
    let setup = format!("{HIDE}\x1b[3;1H{words}\x1b[10;1H{PROMPT}");
    run(Case {
        what: "the display made again",
        setup: setup.clone(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[3;5HQQQQQQQQ".into())],
        shows: Shows::Drawn,
        alone: |t, w| isolated((written(t, w).contains(&2)).then_some(()), "row 2 written"),
    });
    run(Case {
        what: "the display let go",
        setup,
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Wrap(WordWrap::Off)],
        shows: Shows::Drawn,
        alone: untouched,
    });
}

/// **S12: a composition over a paragraph is laid out while shown, and the
/// paragraph again the frame after**: row 3's Hebrew written over by a
/// composition for a frame, then none. The isolation: nothing written.
/// Named break: the rows written over at the last layout not laid out
/// again.
#[test]
fn a_composition_over_a_paragraph_is_laid_out_and_let_go() {
    let over = Some((Row::from_index(3), Col::ZERO, "abcdef"));
    run(Case {
        what: "a composition shown",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Compose(over)],
        shows: Shows::Drawn,
        alone: untouched,
    });
    run(Case {
        what: "a composition let go",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Compose(over), Step::Compose(None)],
        shows: Shows::Same,
        alone: untouched,
    });
}

/// **S13: what a row's renditions carry is drawn**: SAPV 2 over paragraph
/// A's digits, and an SDS opened and closed on the Latin row. A variant
/// and a string are each begun first, on row 9 (`begun`), so the layout's
/// key does not move with the operation and the row's generation is what
/// finds it. The isolation: the row written. Named break: an answer from
/// the key alone.
#[test]
fn what_a_rows_renditions_carry_is_drawn() {
    let begun = format!(
        "{}\x1b[10;1H\x1b[2 ]9\x1b[0 ] \x1b[1]z\x1b[0]",
        two_paragraphs()
    );
    run(Case {
        what: "SAPV over the digits",
        setup: begun.clone(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[3;1H\x1b[2 ]12 34\x1b[0 ]".into())],
        shows: Shows::Drawn,
        alone: |t, w| isolated((written(t, w).contains(&2)).then_some(()), "row 2 written"),
    });
    run(Case {
        what: "a string on the Latin row",
        setup: begun,
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[5;1H\x1b[1]xyz\x1b[0]".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![4]).then_some(()),
                "row 4 alone written",
            )
        },
    });
}

/// **S14: the find marks and the link underline follow the order and the
/// metrics**: a find mark and a link underline on row 2, which row 3's
/// Hebrew written Latin puts in its own order; and, separately, the faces
/// set at another size with nothing written. Named breaks: the marks kept
/// whatever the layout answered, and kept without the metrics.
#[test]
fn the_marks_follow_the_order_and_the_metrics() {
    let marked = Setting {
        marks: Marks::Shown,
        ..PLAIN
    };
    run(Case {
        what: "the marks over a row put in another order",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: marked.clone(),
        steps: vec![Step::Bytes("\x1b[4;1Habc de".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![3]).then_some(()),
                "row 3 alone written",
            )
        },
    });
    run(Case {
        what: "the marks at another size",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: marked,
        steps: vec![Step::Rescale(16.0)],
        shows: Shows::Drawn,
        alone: untouched,
    });
}

/// **With BDSM reset a row joins alone**: an Arabic word soft-wrapped over
/// rows 2 and 3, and a letter written in row 3 only. Row 2's forms are a
/// fresh renderer's: each row is a unit of its own with the mode reset, and
/// its joins read its own cells. The isolation: row 3 alone written. Named
/// break: the rejoin limited to the written rows; this case stays green
/// under it where a row's joins read only its own cells, as the control
/// that they do. On Courier New, macOS's, whose `GSUB` draws the forms.
#[cfg(target_os = "macos")]
#[test]
fn with_bdsm_reset_a_word_wrapped_over_two_rows_joins_each_alone() {
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships");
    let data = vec![font("JetBrainsMono-Regular.ttf"), courier];
    on_the_cpu_path(
        &Case {
            what: "a word wrapped over two rows",
            setup: format!("{HIDE}\x1b[8l\x1b[3;37H{}", "\u{628}".repeat(6)),
            back: Rows::ZERO,
            setting: PLAIN,
            steps: vec![Step::Bytes("\x1b[4;1H\u{627}".into())],
            shows: Shows::Drawn,
            alone: |t, w| {
                isolated(
                    (written(t, w) == vec![3]).then_some(()),
                    "row 3 alone written",
                )
            },
        },
        &data,
    );
}

/// **The leaving row's layout follows a write** (`Reorder::lay_out_top`,
/// the layout a smooth scroll's slide draws its leaving row by, and which
/// `Renderer::swap_leaving_layout` exchanges whole with the window's): the
/// first paragraph of the window one line up, which runs from the history
/// onto the screen, written over on the screen between two layouts. The
/// control: the maps differ. Named break: an answer from the key alone.
#[test]
fn the_leaving_rows_layout_follows_a_write() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        format!("\x1b[1;1H{}{}12 34\x1b[10;1H\n", hebrew_row(), digits()).as_bytes(),
    );
    let up = Rows::of_index(1);
    let top = |t: &Term<'_>| {
        let mut o = Reorder::new();
        o.lay_out_top(t, up, Joins::Seen);
        o.orders().clone()
    };
    let mut o = Reorder::new();
    let _ = t.seal_writes();
    o.lay_out_top(&t, up, Joins::Seen);
    let before = top(&t);
    feed(&mut t, b"\x1b[1;1Habc def");
    let _ = t.seal_writes();
    o.lay_out_top(&t, up, Joins::Seen);
    let after = top(&t);
    assert!(before != after, "the control: the write moves the maps");
    assert!(*o.orders() == after, "the kept maps follow the write");
}
