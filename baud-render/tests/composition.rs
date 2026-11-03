//! An input method's composition -- the marked text of `NSTextInputClient`,
//! a dead key's accent, an IME's unconverted kana -- drawn at the cursor as
//! underlined text, before it is committed and before the child has seen a
//! byte of it.
//!
//! `[[source: apple-nstextinputclient]]`'s `setMarkedText`: "the receiver is
//! expected to render the marked text with distinguishing appearance", and
//! `NSTextView`'s `markedTextAttributes` -- "Text color, background color,
//! and underline are the only supported attributes for marked text". baud's
//! appearance is an underline in the text's own colour, with the cursor after
//! the text.
//!
//! **The reference is the same text written underlined.** A composition of
//! `xyz` at the cursor must draw exactly what `SGR 4 ; xyz ; SGR 24` written
//! there draws, cursor included, so each test renders both and compares
//! every pixel. The CPU path here; the two GPU arms in
//! `baud-present-macos/tests/cells.rs`.

mod support;

#[path = "../../baud-font/tests/sha256/mod.rs"]
mod sha256;

use baud_core::{Owned, Term};
use baud_domain::{AmbiguousWidth, BytePos, Codepoint, ReadLen, Vs16Width};
use baud_font::Face;
use baud_render::{Caret, Composition, Damage, Highlight, Hints, Measure, Region, Renderer};
use support::{Canvas, WS, font, renderer};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn cps(s: &str) -> Vec<Codepoint> {
    s.chars()
        .map(|c| Codepoint::new(u32::from(c)).unwrap())
        .collect()
}

/// The frame of a term fed `bytes`, with `composition` at its cursor.
fn frame(r: &mut Renderer<'_, '_>, bytes: &[u8], composition: Option<&str>) -> Vec<u32> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let (row, col) = t.cursor();
    let measure = Measure::of(&t);
    r.set_composition(composition.map(|s| Composition {
        row,
        col,
        text: cps(s),
        measure,
        hints: Vec::new(),
        caret: Caret::End,
    }));
    let mut c = Canvas::for_renderer(r);
    support::paint(r, &mut t, &mut c, Damage::Full);
    r.set_composition(None);
    c.pixels
}

fn jbm() -> Vec<u8> {
    font("JetBrainsMono-Regular.ttf")
}

#[test]
fn a_composition_draws_as_underlined_text_at_the_cursor() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    // Over a line already written: the composition covers `cde`, and the
    // reference writes `xyz` there underlined, leaving the cursor on `f`.
    let base = b"abcdef\x1b[4D";
    let composed = frame(&mut r, base, Some("xyz"));
    let written = frame(&mut r, b"abcdef\x1b[4D\x1b[4mxyz\x1b[24m", None);
    let plain = frame(&mut r, base, None);
    assert_ne!(composed, plain, "the composition is drawn");
    assert_eq!(
        composed, written,
        "as the same text written underlined, cursor after it"
    );
}

#[test]
fn a_dead_key_accent_draws_as_one_underlined_cell() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    // What US International's dead key marks before its second key: U+00B4.
    let composed = frame(&mut r, b"$ ", Some("\u{b4}"));
    let written = frame(&mut r, "$ \x1b[4m\u{b4}\x1b[24m".as_bytes(), None);
    assert_eq!(composed, written);
}

#[test]
fn a_combining_mark_joins_the_character_before_it() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    // U+0301 has no width: it rides on the `e` before it, in that cell, as
    // the core clusters the same text written, so `x` takes the next cell
    // and the cursor the one after.
    let base = b"abcdef\x1b[4D";
    let composed = frame(&mut r, base, Some("e\u{301}x"));
    let written = frame(
        &mut r,
        "abcdef\x1b[4D\x1b[4me\u{301}x\x1b[24m".as_bytes(),
        None,
    );
    assert_eq!(composed, written);
}

#[test]
fn a_wide_character_takes_two_cells() {
    let data = jbm();
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&noto).unwrap()];
    let mut r = renderer(&faces);
    // U+231A is East Asian Wide, in the Noto subset: the cursor lands two
    // cells after it, and the underline spans both.
    let base = b"abcdef\x1b[5D";
    let composed = frame(&mut r, base, Some("q\u{231a}z"));
    let written = frame(
        &mut r,
        "abcdef\x1b[5D\x1b[4mq\u{231a}z\x1b[24m".as_bytes(),
        None,
    );
    assert_eq!(composed, written);
}

#[test]
fn an_ended_composition_repaints_the_cells_under_it() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let base = b"abcdef\x1b[4D";
    let plain = frame(&mut r, base, None);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, base);
    let (row, col) = t.cursor();
    let mut c = Canvas::for_renderer(&r);
    r.set_composition(Some(Composition {
        row,
        col,
        text: cps("xyz"),
        measure: Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    }));
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    // Committed or cancelled: the incremental frame after it is the grid.
    r.set_composition(None);
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(c.pixels, plain, "the cells under it were repainted");
    // And the cell before the cursor a continuation was drawn on: a vowel
    // sign continues its consonant while `?2027` is set.
    let base = "\x1b[?2027hab\u{915}cdef\x1b[4D";
    let plain = frame(&mut r, base.as_bytes(), None);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, base.as_bytes());
    let mut c = Canvas::for_renderer(&r);
    let headed = Composition::at_cursor(&t, &cps("\u{93f}"), &[], Caret::End);
    assert_eq!(headed.col.index(), 2, "its head");
    r.set_composition(Some(headed));
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    assert!(c.pixels != plain, "the head is drawn");
    r.set_composition(None);
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert!(c.pixels == plain, "the head's cell was repainted");
}

#[test]
fn a_changed_composition_repaints_even_over_the_same_text() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    // The grid already holds `xyz` where the composition `xyz` goes, so no
    // cell word changes when it appears: only the composition can say so.
    let base = b"xyz\x1b[3D";
    let want = frame(&mut r, base, Some("xyz"));
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, base);
    let (row, col) = t.cursor();
    let mut c = Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    r.set_composition(Some(Composition {
        row,
        col,
        text: cps("xyz"),
        measure: Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    }));
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(c.pixels, want, "the underline and the cursor moved in");
}

#[test]
fn over_blank_cells_it_appears_and_goes_on_incremental_frames() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    // A prompt and nothing after it, the cursor hidden: the cells under the
    // composition are blank before, during and after, and no cursor moves,
    // so no cell word and no cursor says the row changed -- the composition
    // alone must dirty it, coming and going.
    let base = b"$ \x1b[?25l";
    let plain = frame(&mut r, base, None);
    let composed = frame(&mut r, base, Some("xyz"));
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, base);
    let (row, col) = t.cursor();
    let mut c = Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    r.set_composition(Some(Composition {
        row,
        col,
        text: cps("xyz"),
        measure: Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    }));
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(c.pixels, composed, "it appeared");
    r.set_composition(None);
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(c.pixels, plain, "and went");
}

#[test]
fn a_composition_past_the_right_edge_is_cut_at_it() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let last = usize::from(WS.ws_col);
    // Two columns from the edge: `ab` fit, `cd` do not and are not drawn,
    // and the cursor stays on the last column, where a write leaves it.
    let to = format!("\x1b[1;{}H", last - 1);
    let composed = frame(&mut r, to.as_bytes(), Some("abcd"));
    let written = frame(&mut r, format!("{to}\x1b[4mab\x1b[24m").as_bytes(), None);
    assert_eq!(composed, written);
}

#[test]
fn a_composition_on_a_later_row_draws_on_that_row() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let composed = frame(&mut r, b"one\r\ntwo\r\nth", Some("ree"));
    let written = frame(&mut r, b"one\r\ntwo\r\nth\x1b[4mree\x1b[24m", None);
    assert_eq!(composed, written);
}

/// **The overlay measures as the core writes.** Under the configured
/// ambiguous width `U+00B1` is two cells and under the default one; a Private
/// Use character (`U+E0B0`) is one under both, the table storing it narrow,
/// and the cursor lands where a write of the same text leaves it.
#[test]
fn a_composition_takes_the_configured_ambiguous_width() {
    let home = Term::new(Owned::new(&WS).storage(), &WS).cursor();
    let comp = |text: &str, ambiguous| Composition {
        row: home.0,
        col: home.1,
        text: cps(text),
        measure: Measure {
            ambiguous,
            ..Measure::default()
        },
        hints: Vec::new(),
        caret: Caret::End,
    };
    let cols = baud_domain::Cols::of_index(usize::from(WS.ws_col));
    for (text, ambiguous, width) in [
        ("\u{b1}", AmbiguousWidth::Narrow, 1),
        ("\u{b1}", AmbiguousWidth::Wide, 2),
        ("a\u{b1}b", AmbiguousWidth::Wide, 4),
        ("\u{e0b0}", AmbiguousWidth::Narrow, 1),
        ("\u{e0b0}", AmbiguousWidth::Wide, 1),
    ] {
        let c = comp(text, ambiguous);
        assert_eq!(c.columns(cols), (0, width), "{text:?} {ambiguous:?}");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_ambiguous_width(ambiguous);
        feed(&mut t, text.as_bytes());
        assert_eq!(t.cursor().1.index(), width, "core {text:?} {ambiguous:?}");
        let at = c.cursor(cols).expect("after it").1;
        assert_eq!(at.index(), width, "{text:?} {ambiguous:?}");
    }
}

/// **A composition is laid out in the clusters a write of it makes**
/// (`[[source: uax29]]`, the core's own machine): on a term measuring by
/// each `Measure` -- ambiguous narrow or wide, the current widths or
/// Unicode 8's, `U+FE0F` widening or not, a cell a legacy cluster, one with
/// spacing marks joined or an extended one -- the text composed after a
/// letter covers the columns the same text written after that letter
/// takes, the core's cursor after the commit is where the composition
/// ended, and its cells are the core's cell for cell, wide tails and bases.
/// Every control-free line of `GraphemeBreakTest.txt`, and by hand: two
/// CJK characters, a ZWJ family, a flag, an emoji under `U+FE0F` and
/// under a skin tone, Hangul jamo, Devanagari and Gujarati spacing marks,
/// a mark after `U+200B`, a box line under the ambiguous width. **What
/// continues the character before the cursor is drawn on that cell**, its
/// cluster at the composition's head ([`Composition::at_cursor`]): a
/// vowel sign after its consonant while `?2027` is set, a skin tone after
/// its emoji, a
/// `U+FE0F` that widens the `☠` before it over the cursor's cell, the
/// second half of a flag, a ZWJ, a mark on a wide character -- each
/// composition starting on the cell before the cursor.
#[test]
fn a_composition_takes_the_cells_a_write_of_it_takes() {
    const WIDE_WS: baud_core::WinSize = baud_core::WinSize {
        ws_row: 2,
        ws_col: 160,
    };
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/unicode-graphemebreaktest.txt"
    ))
    .expect("GraphemeBreakTest.txt");
    let mut texts: Vec<Vec<Codepoint>> = [
        "\u{4e2d}\u{6587}",
        "\u{b1}\u{2500}",
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
        "\u{1f1ef}\u{1f1f5}",
        "\u{2764}\u{fe0f}x",
        "\u{915}\u{93f}",
        "\u{1f44d}\u{1f3fd}",
        "\u{1100}\u{1161}\u{11a8}",
        "a\u{903}",
        "\u{a95}\u{abe}\u{a95}",
        "e\u{200b}\u{301}x",
        "\u{903}ab",
    ]
    .iter()
    .map(|t| cps(t))
    .collect();
    for line in src.lines() {
        let body = line.split('#').next().unwrap_or("");
        let line: Option<Vec<Codepoint>> = body
            .split_whitespace()
            .filter(|t| *t != "\u{f7}" && *t != "\u{d7}")
            .map(|h| u32::from_str_radix(h, 16).ok().and_then(Codepoint::new))
            .collect();
        if let Some(line) = line.filter(|l| {
            !l.is_empty()
                && l.iter()
                    .all(|c| !(c.value() < 0x20 || (0x7f..=0x9f).contains(&c.value())))
        }) {
            texts.push(line);
        }
    }
    assert!(texts.len() > 400, "the suite read: {}", texts.len());
    let mut texts: Vec<(Vec<Codepoint>, Vec<Codepoint>)> =
        texts.into_iter().map(|t| (cps("x"), t)).collect();
    let heads = [
        ("\u{915}", "\u{93f}"),
        ("\u{1f44d}", "\u{1f3fd}x"),
        ("\u{2620}", "\u{fe0f}x"),
        ("\u{1f1ef}", "\u{1f1f5}"),
        ("e", "\u{301}x"),
        ("\u{4e2d}", "\u{301}y"),
        ("\u{1f468}", "\u{200d}\u{1f469}"),
    ];
    texts.extend(heads.iter().map(|(pre, t)| (cps(pre), cps(t))));
    let cols = baud_domain::Cols::of_index(usize::from(WIDE_WS.ws_col));
    let measures = [
        (
            AmbiguousWidth::Narrow,
            baud_domain::UnicodeWidths::Current,
            Vs16Width::Two,
        ),
        (
            AmbiguousWidth::Wide,
            baud_domain::UnicodeWidths::Current,
            Vs16Width::Two,
        ),
        (
            AmbiguousWidth::Narrow,
            baud_domain::UnicodeWidths::Eight,
            Vs16Width::Two,
        ),
        (
            AmbiguousWidth::Narrow,
            baud_domain::UnicodeWidths::Current,
            Vs16Width::One,
        ),
    ];
    use baud_domain::{ClusterCells, SpacingMarks};
    let cells = [
        (ClusterCells::Legacy, SpacingMarks::Column, &b""[..]),
        (ClusterCells::LegacyMarksJoined, SpacingMarks::Joined, b""),
        (ClusterCells::Extended, SpacingMarks::Column, b"\x1b[?2027h"),
    ];
    for ((ambiguous, widths, vs16), (want_cells, marks, setup)) in measures
        .into_iter()
        .flat_map(|m| cells.iter().map(move |c| (m, *c)))
    {
        for (pre, text) in &texts {
            let mut owned = Owned::new(&WIDE_WS);
            let mut t = Term::new(owned.storage(), &WIDE_WS);
            t.set_ambiguous_width(ambiguous);
            t.set_vs16_width(vs16);
            t.set_spacing_marks(marks);
            feed(&mut t, setup);
            if widths == baud_domain::UnicodeWidths::Eight {
                feed(&mut t, b"\x1b]1337;UnicodeVersion=8\x07");
            }
            let measure = Measure::of(&t);
            assert_eq!(
                measure,
                Measure {
                    ambiguous,
                    widths,
                    vs16,
                    cells: want_cells,
                }
            );
            let s = |t: &[Codepoint]| -> String {
                t.iter()
                    .map(|c| char::from_u32(c.value()).unwrap())
                    .collect()
            };
            feed(&mut t, s(pre).as_bytes());
            let before = t.cursor().1.index();
            let c = Composition::at_cursor(&t, text, &[], Caret::End);
            // A vowel sign joins its consonant's cell while `?2027` is set or
            // spacing marks are joined; reset, it takes a cell of its own
            // and the composition is led by the syllable's first cell.
            if heads.iter().any(|(p, h)| cps(p) == *pre && cps(h) == *text) {
                assert!(c.col.index() < before, "{text:x?} {measure:?}: its head");
            }
            feed(&mut t, s(text).as_bytes());
            let (start, end) = c.columns(cols);
            let at = t.cursor().1.index();
            assert_eq!(end, at, "{text:x?} {measure:?}: where the commit lands");
            assert_eq!(c.cursor(cols).map(|(_, x)| x.index()), Some(at));
            let row = t.row(baud_domain::Row::ZERO).expect("row");
            let core: Vec<(bool, Option<u32>)> = row[start..at]
                .iter()
                .map(|&cell| {
                    (
                        cell.content.is_wide_tail(),
                        t.base_of(cell).drawn().as_codepoint().map(|c| c.value()),
                    )
                })
                .collect();
            let drawn: Vec<(bool, Option<u32>)> = c
                .written(cols, baud_domain::StyleId::DEFAULT)
                .iter()
                .map(|cell| {
                    (
                        cell.content.is_wide_tail(),
                        cell.content.as_codepoint().map(|c| c.value()),
                    )
                })
                .collect();
            assert_eq!(drawn, core, "{text:x?} {measure:?}: the cells");
        }
    }
}

/// **A composition the core clusters draws as the text written**: two CJK
/// characters over four cells, a ZWJ family, a flag, an emoji under
/// `U+FE0F` and a spacing mark after its letter -- in the letter's cell
/// with `?2027` set, and reset in a cell of its own, the syllable drawn
/// across both -- each composed over `cdef` equal to the same text written
/// underlined there, the cursor after it.
#[test]
fn a_clustered_composition_draws_as_its_text_written() {
    let data = jbm();
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&noto).unwrap()];
    let mut r = renderer(&faces);
    for setup in ["", "\x1b[?2027h"] {
        for text in [
            "\u{4e2d}\u{6587}",
            "\u{1f468}\u{200d}\u{1f469}",
            "\u{1f1ef}\u{1f1f5}",
            "\u{2764}\u{fe0f}",
            "a\u{903}b",
            "\u{915}\u{93f}\u{930}\u{94d}\u{92e}",
        ] {
            let base = format!("{setup}abcdefgh\x1b[6D");
            let composed = frame(&mut r, base.as_bytes(), Some(text));
            let written = frame(
                &mut r,
                format!("{base}\x1b[4m{text}\x1b[24m").as_bytes(),
                None,
            );
            assert!(composed == written, "{setup:?} {text:?}: as written");
        }
    }
}

/// **What continues the character before the cursor is drawn on that
/// cell**, as the commit will leave it, in the marked-text look: a vowel
/// sign composed after its consonant, in its cell with `?2027` set and
/// reset in the next, the syllable drawn across both; a skin tone after
/// its emoji and a
/// `U+FE0F` after the `☠` it widens, each composed over `cdef` equal to
/// the cell rewritten underlined with it, the cursor after it, and each
/// drawn.
#[test]
fn a_continuation_composed_draws_on_the_cell_before_it() {
    let data = jbm();
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&noto).unwrap()];
    let mut r = renderer(&faces);
    let shot = |r: &mut Renderer<'_, '_>, bytes: &str, text: Option<&str>| {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes.as_bytes());
        r.set_composition(text.map(|s| Composition::at_cursor(&t, &cps(s), &[], Caret::End)));
        let mut c = Canvas::for_renderer(r);
        support::paint(r, &mut t, &mut c, Damage::Full);
        r.set_composition(None);
        c.pixels
    };
    for (setup, pre, back, text) in [
        ("\x1b[?2027h", "\u{915}", 1, "\u{93f}"),
        ("", "\u{915}", 1, "\u{93f}"),
        ("", "\u{1f44d}", 2, "\u{1f3fd}"),
        ("", "\u{2620}", 1, "\u{fe0f}x"),
    ] {
        let base = format!("{setup}ab{pre}cdefgh\x1b[6D");
        let composed = shot(&mut r, &base, Some(text));
        let plain = shot(&mut r, &base, None);
        let written = shot(
            &mut r,
            &format!("{base}\x1b[{back}D\x1b[4m{pre}{text}\x1b[24m"),
            None,
        );
        assert!(composed != plain, "{text:?}: drawn");
        assert!(composed == written, "{text:?}: as written");
    }
}

/// **The input method's cursor and hints count past the head**: with
/// `?2027` set, a vowel sign and `b` composed after `क` start on the `क`'s
/// cell, the cursor
/// before the sign on it and before the `b` on the cell after, a range
/// over the `b` that cell alone, and the head takes the sign's hints.
/// Past a pending wrap the cell is the cursor's own, the last column's,
/// where the core attaches the mark.
#[test]
fn the_input_methods_cursor_counts_past_the_head() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[?2027hab\u{915}".as_bytes());
    let cols = baud_domain::Cols::of_index(usize::from(WS.ws_col));
    let text = cps("\u{93f}b");
    let at = |caret| Composition::at_cursor(&t, &text, &[], caret);
    let cell = |c| Some((baud_domain::Row::ZERO, baud_domain::Col::at_index(c)));
    assert_eq!(at(Caret::End).col.index(), 2, "its head");
    assert_eq!(at(Caret::At(0)).cursor(cols), cell(2), "before the sign");
    assert_eq!(at(Caret::At(1)).cursor(cols), cell(3), "before the b");
    assert_eq!(
        at(Caret::Range(1, 2)).caret_cells(cols),
        Some((
            baud_domain::Row::ZERO,
            baud_domain::Col::at_index(3),
            baud_domain::Cols::of_index(1)
        )),
        "the b selected"
    );
    let hinted = Composition::at_cursor(&t, &text, &[Hints::SELECTION, Hints::NONE], Caret::End);
    assert_eq!(
        hinted.hints,
        [Hints::SELECTION, Hints::SELECTION, Hints::NONE],
        "the sign's hints on its head"
    );
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let last = usize::from(WS.ws_col) - 1;
    feed(&mut t, format!("{}z", "a".repeat(last)).as_bytes());
    assert_eq!(t.wrap_pending(), baud_core::WrapPending::Yes, "the premise");
    let c = Composition::at_cursor(&t, &cps("\u{301}"), &[], Caret::End);
    assert_eq!(c.col.index(), last, "past a pending wrap: the last column");
    assert_eq!(c.text, cps("z\u{301}"), "led by its cell");
    feed(&mut t, "\u{301}".as_bytes());
    let marks: Vec<u32> = t
        .marks(baud_domain::Row::ZERO, baud_domain::Col::at_index(last))
        .collect();
    assert_eq!(marks, [0x301], "where the core attaches it");
}

/// **A composition on a right-to-left line is mirrored as its text
/// written is**: under `ambiguous_width = "wide"` on a line BDSM reset
/// draws in reverse with `?2500` set, a box line two cells wide composed
/// is mirrored whole as the same text written (`┌─│`), and so is a
/// Bidi_Mirrored character with no mirror glyph, drawn flipped (`∑`),
/// each composed over `cdef` equal to the text written underlined there,
/// on a single-width line and a double-width one -- before the renderer
/// has shaped a letter that takes a joining form and after, when the
/// layout with BDSM reset keeps the last frame's joins.
#[test]
fn a_composition_on_a_mirrored_line_is_mirrored_as_its_text_written() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    for joins in [baud_render::Joins::Unseen, baud_render::Joins::Seen] {
        mirrored_as_written(&faces, joins);
    }
}

fn mirrored_as_written(faces: &[Face<'_>], joins: baud_render::Joins) {
    let mut r = renderer(faces);
    let shot = |r: &mut Renderer<'_, '_>, bytes: &str, text: Option<&str>| {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_ambiguous_width(AmbiguousWidth::Wide);
        feed(&mut t, bytes.as_bytes());
        let (row, col) = t.cursor();
        r.set_composition(text.map(|s| Composition {
            row,
            col,
            text: cps(s),
            measure: Measure::of(&t),
            hints: Vec::new(),
            caret: Caret::End,
        }));
        let mut c = Canvas::for_renderer(r);
        support::paint(r, &mut t, &mut c, Damage::Full);
        r.set_composition(None);
        c.pixels
    };
    if joins == baud_render::Joins::Seen {
        shot(&mut r, "\x1b[8l\x1b[1;1H\u{628}\u{628}\u{628}", None);
    }
    assert_eq!(r.joins_seen(), joins, "the layout the frames take");
    for size in ["", "\x1b#6"] {
        for text in ["\u{250C}\u{2500}\u{2502}", "a\u{2211}b"] {
            let base = format!("\x1b[8l\x1b[3;1H{size}\x1b[?2500h\x1b[2 kabcdefgh\x1b[6D");
            let composed = shot(&mut r, &base, Some(text));
            let written = shot(&mut r, &format!("{base}\x1b[4m{text}\x1b[24m"), None);
            let plain = shot(&mut r, &base, None);
            assert_ne!(composed, plain, "{size:?} {text:?}: drawn");
            assert!(
                composed == written,
                "{joins:?} {size:?} {text:?}: as written"
            );
        }
    }
}

/// **On a line that is not single-width a composition draws as its text
/// would there** (`[[source: dec-std-070]]` § 5.6.1): on a double-width
/// line and on each half of a double-height one, the composition over
/// `cde` equals `xyz` written underlined on the same line, each character
/// over two columns and the cursor after it -- a mark riding on its
/// letter and a wide character over four columns included.
#[test]
fn a_composition_on_a_sized_line_draws_as_its_text_would() {
    let data = jbm();
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&noto).unwrap()];
    let mut r = renderer(&faces);
    for set in ["\x1b#6", "\x1b#3", "\x1b#4"] {
        for text in ["xyz", "e\u{301}x", "q\u{231a}z"] {
            let base = format!("\x1b[3;1H{set}abcdef\x1b[5D");
            let composed = frame(&mut r, base.as_bytes(), Some(text));
            let written = frame(
                &mut r,
                format!("{base}\x1b[4m{text}\x1b[24m").as_bytes(),
                None,
            );
            let plain = frame(&mut r, base.as_bytes(), None);
            assert_ne!(composed, plain, "{set:?} {text:?}: drawn");
            assert!(composed == written, "{set:?} {text:?}: as written");
        }
    }
}

/// The frame of a term fed `bytes`, with `text` composed at its cursor
/// under `hints`, and `selected` columns of the cursor's row highlighted.
fn hinted(
    r: &mut Renderer<'_, '_>,
    bytes: &[u8],
    text: Option<(&str, Vec<Hints>)>,
    selected: Option<(usize, usize)>,
) -> Vec<u32> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let (row, col) = t.cursor();
    r.set_composition(text.map(|(s, hints)| Composition {
        row,
        col,
        text: cps(s),
        measure: Measure::default(),
        hints,
        caret: Caret::End,
    }));
    r.set_highlight(selected.map(|(a, b)| {
        Highlight::new(
            (row, baud_domain::Col::at_index(a)),
            (row, baud_domain::Col::at_index(b)),
            Region::Stream,
        )
    }));
    let mut c = Canvas::for_renderer(r);
    support::paint(r, &mut t, &mut c, Damage::Full);
    r.set_composition(None);
    r.set_highlight(None);
    c.pixels
}

/// One hinted composition and the text written in its rendition: a name,
/// the hints, the bytes and the columns written selected.
type Case<'a> = (&'a str, Vec<Hints>, &'a str, Option<(usize, usize)>);

/// **An input method's hints draw as the renditions GTK's theme gives
/// them, each the same text written so** (`text-input-v3`'s
/// `preedit_hint`): `whole` as `SGR 4`, `prediction` as `SGR 3`,
/// `prefix` and `suffix` as `SGR 2`, `spelling_error` as `SGR 4:3` under
/// `SGR 58` in the palette's red, `compose_error` as `SGR 9`, `selection`
/// as the same cells selected; a codepoint no hint covers as plain text;
/// and hints combined, one cluster at a time -- on a single-width line and
/// on each sized one.
#[test]
fn an_input_methods_hints_draw_as_the_renditions_they_name() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let all = |h: Hints| vec![h; 3];
    for set in ["", "\x1b#6", "\x1b#3", "\x1b#4"] {
        let base = format!("\x1b[3;1H{set}abcdef\x1b[5D");
        let written = |r: &mut Renderer<'_, '_>, sgr: &str, sel| {
            hinted(r, format!("{base}{sgr}\x1b[m").as_bytes(), None, sel)
        };
        let composed = |r: &mut Renderer<'_, '_>, h: Vec<Hints>| {
            hinted(r, base.as_bytes(), Some(("xyz", h)), None)
        };
        let cases: [Case<'_>; 9] = [
            ("whole", all(Hints::WHOLE), "\x1b[4mxyz", None),
            ("none", all(Hints::NONE), "xyz", None),
            ("prediction", all(Hints::PREDICTION), "\x1b[3mxyz", None),
            ("prefix", all(Hints::PREFIX), "\x1b[2mxyz", None),
            ("suffix", all(Hints::SUFFIX), "\x1b[2mxyz", None),
            (
                "spelling",
                all(Hints::SPELLING_ERROR.with(Hints::WHOLE)),
                "\x1b[4:3;58:5:1mxyz",
                None,
            ),
            (
                "compose",
                all(Hints::COMPOSE_ERROR.with(Hints::SPELLING_ERROR)),
                "\x1b[9mxyz",
                None,
            ),
            ("selection", all(Hints::SELECTION), "xyz", Some((1, 3))),
            (
                "mixed",
                vec![
                    Hints::SELECTION.with(Hints::WHOLE),
                    Hints::WHOLE,
                    Hints::PREDICTION,
                ],
                "\x1b[4mxy\x1b[24;3mz",
                Some((1, 1)),
            ),
        ];
        for (name, h, sgr, sel) in cases {
            let got = composed(&mut r, h);
            let want = written(&mut r, sgr, sel);
            assert!(got == want, "{set:?} {name}: as written");
        }
        let plain = hinted(&mut r, base.as_bytes(), None, None);
        assert_ne!(composed(&mut r, all(Hints::NONE)), plain, "{set:?}: drawn");
    }
}

/// **Over blinking text in a faint off phase**, as Dim Flashing Lights
/// draws it, a composition the input method marks misspelt -- its glyphs
/// and its curly line in the hint's `SGR 58` red -- is drawn faint, equal
/// to the same text written blinking in that rendition, on a single-width
/// line and on each sized one; and it differs from the shown phase.
#[test]
fn a_composition_over_faint_blinking_text_draws_faint() {
    use baud_render::{Blink, BlinkPhases};
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let spelling = vec![Hints::SPELLING_ERROR.with(Hints::WHOLE); 3];
    let faint = BlinkPhases {
        slow: Blink::Faint,
        rapid: Blink::Shown,
    };
    for set in ["", "\x1b#6", "\x1b#3"] {
        let base = format!("\x1b[3;1H{set}\x1b[5mabcdef\x1b[m\x1b[5D");
        let written = format!("{base}\x1b[5;4:3;58:5:1mxyz\x1b[m");
        r.set_blink(faint);
        let got = hinted(
            &mut r,
            base.as_bytes(),
            Some(("xyz", spelling.clone())),
            None,
        );
        let want = hinted(&mut r, written.as_bytes(), None, None);
        r.set_blink(BlinkPhases::SHOWN);
        let shown = hinted(
            &mut r,
            base.as_bytes(),
            Some(("xyz", spelling.clone())),
            None,
        );
        assert!(got == want, "{set:?}: as written, faint");
        assert!(
            got != shown,
            "{set:?}: the faint phase drew the composition in full"
        );
    }
}

/// **On a dimmed page too** -- the Linux driver draws its theme's dim --
/// a selected cluster's page and the faint, struck and curly ones equal
/// the same text written selected and in their renditions, though every
/// background there is below full alpha.
#[test]
fn hints_on_a_dimmed_page_draw_as_their_renditions() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut theme = baud_render::Theme::dark();
    theme.bg_alpha = baud_domain::Chan::from_u8(0xB0);
    let mut r = Renderer::new(
        baud_render::Stack::new(&faces).unwrap(),
        baud_render::PxSize::of(support::PX).unwrap(),
        theme,
        baud_render::Blend::Native,
        baud_domain::Cols::of_index(usize::from(WS.ws_col)),
        baud_domain::Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let base = "\x1b[3;1Habcdef\x1b[5D";
    let cases: [Case<'_>; 4] = [
        ("selection", vec![Hints::SELECTION; 3], "xyz", Some((1, 3))),
        ("prefix", vec![Hints::PREFIX; 3], "\x1b[2mxyz", None),
        ("compose", vec![Hints::COMPOSE_ERROR; 3], "\x1b[9mxyz", None),
        (
            "spelling",
            vec![Hints::SPELLING_ERROR; 3],
            "\x1b[4:3;58:5:1mxyz",
            None,
        ),
    ];
    for (name, h, sgr, sel) in cases {
        let got = hinted(&mut r, base.as_bytes(), Some(("xyz", h)), None);
        let want = hinted(&mut r, format!("{base}{sgr}\x1b[m").as_bytes(), None, sel);
        assert!(got == want, "{name}: as written on a dimmed page");
    }
}

/// The frame of a term fed `bytes`, with `text` composed at its cursor and
/// the input method's cursor at `caret`, and `selected` columns of the
/// cursor's row highlighted.
fn with_caret(
    r: &mut Renderer<'_, '_>,
    bytes: &[u8],
    text: Option<(&str, Caret)>,
    selected: Option<(usize, usize)>,
) -> Vec<u32> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let (row, col) = t.cursor();
    r.set_composition(text.map(|(s, caret)| Composition {
        row,
        col,
        text: cps(s),
        measure: Measure::default(),
        hints: Vec::new(),
        caret,
    }));
    r.set_highlight(selected.map(|(a, b)| {
        Highlight::new(
            (row, baud_domain::Col::at_index(a)),
            (row, baud_domain::Col::at_index(b)),
            Region::Stream,
        )
    }));
    let mut c = Canvas::for_renderer(r);
    support::paint(r, &mut t, &mut c, Damage::Full);
    r.set_composition(None);
    r.set_highlight(None);
    c.pixels
}

/// **The input method's cursor is drawn where it puts it**, each case the
/// same text written underlined with the terminal's cursor where the case
/// says (`[[source: wp-text-input-unstable-v3]]`'s `cursor_begin` and
/// `cursor_end`): at an index, the cursor on that character, in the style
/// the program set -- a block, and a bar under `DECSCUSR 6`; at the count,
/// after the composition, as with nothing said; a range, its characters
/// selected and no cursor; hidden, no cursor. The control is the cursor
/// after the composition, which every positioned case differs from.
/// Perturbations: draw the cursor after the composition at an index -- the
/// index's assertion fails; keep the cursor with a range -- the range's
/// assertion fails.
#[test]
fn the_input_methods_cursor_is_drawn_where_it_puts_it() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let base = b"abcdef\x1b[4D";
    let written = |r: &mut Renderer<'_, '_>, tail: &str, sel| {
        let mut b = base.to_vec();
        b.extend_from_slice(tail.as_bytes());
        with_caret(r, &b, None, sel)
    };
    let after = written(&mut r, "\x1b[4mxyz\x1b[24m", None);
    let end = with_caret(&mut r, base, Some(("xyz", Caret::End)), None);
    assert_eq!(end, after, "the control: nothing said, after it");
    let at = with_caret(&mut r, base, Some(("xyz", Caret::At(1))), None);
    assert_ne!(at, after, "the control differs");
    assert_eq!(
        at,
        written(&mut r, "\x1b[4mxyz\x1b[24m\x1b[2D", None),
        "on y"
    );
    let first = with_caret(&mut r, base, Some(("xyz", Caret::At(0))), None);
    assert_eq!(first, written(&mut r, "\x1b[4mxyz\x1b[24m\x1b[3D", None));
    let count = with_caret(&mut r, base, Some(("xyz", Caret::At(3))), None);
    assert_eq!(count, after, "at the count, after the last");
    let range = with_caret(&mut r, base, Some(("xyz", Caret::Range(1, 3))), None);
    assert_eq!(
        range,
        written(&mut r, "\x1b[?25l\x1b[4mxyz\x1b[24m", Some((3, 4))),
        "y and z selected, no cursor"
    );
    let hidden = with_caret(&mut r, base, Some(("xyz", Caret::Hidden)), None);
    assert_eq!(hidden, written(&mut r, "\x1b[?25l\x1b[4mxyz\x1b[24m", None));
    // The program's cursor style: a bar on y.
    let bar = b"\x1b[6 qabcdef\x1b[4D";
    let barred = with_caret(&mut r, bar, Some(("xyz", Caret::At(1))), None);
    let mut b = bar.to_vec();
    b.extend_from_slice(b"\x1b[4mxyz\x1b[24m\x1b[2D");
    assert_eq!(barred, with_caret(&mut r, &b, None, None), "a bar on y");
    // And an underline on y, over the composition's own line.
    let under = b"\x1b[4 qabcdef\x1b[4D";
    let lined = with_caret(&mut r, under, Some(("xyz", Caret::At(1))), None);
    let mut b = under.to_vec();
    b.extend_from_slice(b"\x1b[4mxyz\x1b[24m\x1b[2D");
    assert_eq!(
        lined,
        with_caret(&mut r, &b, None, None),
        "an underline on y"
    );
}

/// **The input method moving its cursor over the same text repaints the
/// row on an incremental frame**: no cell word and no text changes, only
/// the caret, and the frame must equal a full paint of the new position.
/// Perturbation: compare only the compositions' text for the row's damage
/// -- the moved range is never repainted.
#[test]
fn a_moved_input_method_cursor_repaints_on_an_incremental_frame() {
    let data = jbm();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let base = b"abcdef\x1b[4D";
    let want = with_caret(&mut r, base, Some(("xyz", Caret::At(2))), None);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, base);
    let (row, col) = t.cursor();
    let comp = |caret| Composition {
        row,
        col,
        text: cps("xyz"),
        measure: Measure::default(),
        hints: Vec::new(),
        caret,
    };
    let mut c = Canvas::for_renderer(&r);
    r.set_composition(Some(comp(Caret::At(0))));
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    assert_ne!(c.pixels, want, "the control: the cursor on x");
    r.set_composition(Some(comp(Caret::At(2))));
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(c.pixels, want, "the cursor moved to z");
    // A range moved, with no cursor before or after: only the composition
    // can say the row changed.
    r.set_composition(None);
    let ranged = with_caret(&mut r, base, Some(("xyz", Caret::Range(1, 3))), None);
    r.set_composition(Some(comp(Caret::Range(0, 1))));
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_ne!(c.pixels, ranged, "the control: x selected");
    r.set_composition(Some(comp(Caret::Range(1, 3))));
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    r.set_composition(None);
    assert_eq!(c.pixels, ranged, "y and z selected");
}

/// **A cursor inside a cluster or past a wide character lands on the
/// cluster's cell**, as the clusters are laid: the index of a combining
/// mark is its base's cell, and the character after a wide one is two
/// cells on. A range ending inside a cluster selects the whole cluster.
/// Perturbation: count a cluster as one codepoint -- the x after the
/// cluster's assertion fails.
#[test]
fn the_input_methods_cursor_follows_the_clusters() {
    let data = jbm();
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&noto).unwrap()];
    let mut r = renderer(&faces);
    let base = b"abcdef\x1b[5D";
    let written = |r: &mut Renderer<'_, '_>, tail: &str, sel| {
        let mut b = base.to_vec();
        b.extend_from_slice(tail.as_bytes());
        with_caret(r, &b, None, sel)
    };
    let mark = with_caret(&mut r, base, Some(("e\u{301}x", Caret::At(1))), None);
    assert_eq!(
        mark,
        written(&mut r, "\x1b[4me\u{301}x\x1b[24m\x1b[2D", None),
        "the mark's index is the e's cell"
    );
    let past = with_caret(&mut r, base, Some(("e\u{301}x", Caret::At(2))), None);
    assert_eq!(
        past,
        written(&mut r, "\x1b[4me\u{301}x\x1b[24m\x1b[D", None),
        "the x after the cluster"
    );
    let wide = with_caret(&mut r, base, Some(("q\u{231a}z", Caret::At(2))), None);
    assert_eq!(
        wide,
        written(&mut r, "\x1b[4mq\u{231a}z\x1b[24m\x1b[D", None),
        "z, two cells past the watch"
    );
    let range = with_caret(&mut r, base, Some(("e\u{301}x", Caret::Range(1, 2))), None);
    assert_eq!(
        range,
        written(&mut r, "\x1b[?25l\x1b[4me\u{301}x\x1b[24m", Some((1, 1))),
        "the whole cluster selected"
    );
}

/// The built faces, regular and italic, for SAPV 17's slant: the italic
/// face is what an italic leans by, the regular one draws it upright.
fn built_faces() -> (Vec<u8>, Vec<u8>) {
    let read = |name: &str| {
        std::fs::read(format!(
            "{}/../baud-app/fonts/built/{name}.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    };
    (read("Regular"), read("Italic"))
}

/// **A composition over text SAPV varies draws as its own text written**
/// (`[[source: ecma48]]` 8.3.108): over digits value 2 draws as the
/// Arabic script's, `xyz` composed draws `xyz`, not the digits under it,
/// as the same text written does; and composed where value 17 slants one
/// italic cell of a right-to-left line, in that cell's rendition, both of
/// its letters lean left, as written they do, though the cell after the
/// first holds an upright letter.
#[test]
fn a_composition_over_varied_text_draws_as_its_text_written() {
    use baud_render::{FaceRole, FaceStyle, Stack};
    let (regular, italic) = built_faces();
    let faces = [
        Face::parse(&regular).unwrap(),
        Face::parse(&italic).unwrap(),
    ];
    let mut r = renderer(&faces[..1]);
    let digits = "\x1b[2 ]12345\x1b[4D";
    let composed = frame(&mut r, digits.as_bytes(), Some("xyz"));
    let written = frame(
        &mut r,
        format!("{digits}\x1b[4mxyz\x1b[24m").as_bytes(),
        None,
    );
    assert_ne!(composed, frame(&mut r, digits.as_bytes(), None), "drawn");
    assert!(composed == written, "over digits value 2 draws: as written");
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Italic)];
    let mut r = baud_render::Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        baud_render::PxSize::of(16.0).unwrap(),
        baud_render::Theme::dark(),
        baud_render::Blend::Native,
        baud_domain::Cols::of_index(usize::from(WS.ws_col)),
        baud_domain::Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let line = "\u{5D0}\u{5D1}\x1b[3m\x1b[17 ]\u{5D2}\x1b[0m\x1b[0 ]\u{5D3}\u{5D4}\u{5D5}\x1b[4D";
    let text = "\u{5D5}\u{5D5}";
    let composed = frame(&mut r, line.as_bytes(), Some(text));
    let written = frame(
        &mut r,
        format!("{line}\x1b[3m\x1b[17 ]\x1b[4m{text}\x1b[24m").as_bytes(),
        None,
    );
    let upright = frame(
        &mut r,
        format!("{line}\x1b[4m{text}\x1b[24m").as_bytes(),
        None,
    );
    assert_ne!(written, upright, "the premise: 17 slants the vavs");
    assert!(composed == written, "where 17 slants: as written");
}

/// **Digits composed where value 2 draws the Arabic script's are drawn
/// as written**, as `U+0667`-`U+0669` and not as the digits under them,
/// on SF Arabic, which macOS ships and which draws them.
#[cfg(target_os = "macos")]
#[test]
fn digits_composed_under_value_2_draw_as_written() {
    let data = jbm();
    let arabic =
        std::fs::read("/System/Library/Fonts/SFArabic.ttf").expect("SF Arabic, which macOS ships");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&arabic).unwrap()];
    let mut r = renderer(&faces);
    let digits = "\x1b[2 ]12345\x1b[4D";
    let composed = frame(&mut r, digits.as_bytes(), Some("789"));
    let written = frame(
        &mut r,
        format!("{digits}\x1b[4m789\x1b[24m").as_bytes(),
        None,
    );
    let under = frame(
        &mut r,
        format!("{digits}\x1b[4m234\x1b[24m").as_bytes(),
        None,
    );
    assert_ne!(
        written, under,
        "the premise: the face draws the digits apart"
    );
    assert!(composed == written, "as written");
}

/// **A composition against a Devanagari word draws as its text written**
/// (`baud-render/src/word.rs`): the letters written against it that its
/// own continue are one word with it once committed, so it draws that word
/// whole, shaped as one run and laid in the word's columns, as the commit
/// will be drawn, and the row's other words are laid by what its cells
/// will hold. On Kohinoor Devanagari, which macOS ships, behind JetBrains
/// Mono: कमल composed after a blank, its ink reaching the block cursor's
/// cell after it drawn in the cursor's text colour; मल and म composed
/// after a written क; क composed before a written ल, the cursor after it
/// on ल; म between a written क and ल; क् before a written ष, the conjunct
/// formed across them; मल after कि under `?2027`;
/// मल after a क written red; `x` composed against a written कमल, which
/// lays the word flush against it; and, under `?2027`, a word that fits at
/// no width closing on the ink of the word before it, composed after a
/// written word and written after a composed one -- each equal to the same
/// text written underlined at the cursor, the cursor after it, and each
/// against its premise, the frame without it.
#[cfg(target_os = "macos")]
#[test]
fn a_composition_against_a_devanagari_word_draws_as_its_text_written() {
    let data = jbm();
    let deva = std::fs::read("/System/Library/Fonts/Kohinoor.ttc")
        .expect("Kohinoor Devanagari, which macOS ships");
    let faces = [
        Face::parse(&data).unwrap(),
        Face::parse_index(&deva, baud_font::FaceIndex::of(0u32)).unwrap(),
    ];
    let mut r = renderer(&faces);
    let mut wrong = Vec::new();
    for (base, text) in [
        ("ab ", "\u{915}\u{92e}\u{932}"),
        ("ab\u{915}", "\u{92e}\u{932}"),
        ("ab\u{915}", "\u{92e}"),
        ("ab  \u{932}\x1b[2D", "\u{915}"),
        ("ab \u{915} \u{932}\x1b[2D", "\u{92e}"),
        ("ab  \u{937}\x1b[2D", "\u{915}\u{94d}"),
        ("\x1b[?2027hab\u{915}\u{93f}", "\u{92e}\u{932}"),
        ("ab\x1b[31m\u{915}\x1b[39m", "\u{92e}\u{932}"),
        ("ab  \u{915}\u{92e}\u{932}\x1b[4D", "x"),
    ] {
        let composed = frame(&mut r, base.as_bytes(), Some(text));
        let written = frame(
            &mut r,
            format!("{base}\x1b[4m{text}\x1b[24m").as_bytes(),
            None,
        );
        assert_ne!(
            composed,
            frame(&mut r, base.as_bytes(), None),
            "{base:?} {text:?}: the composition is drawn"
        );
        let d = composed
            .iter()
            .zip(&written)
            .filter(|(a, b)| a != b)
            .count();
        if d != 0 {
            wrong.push((base, text, d));
        }
    }
    // A word that fits at no width closes on the word before it, by that
    // word's ink: क्ति composed after a written क and a blank under `?2027`,
    // and a written क्ति after a composed क -- each a residue as written.
    for (base, text) in [
        ("\x1b[?2027h\u{915} ", "\u{915}\u{94d}\u{924}\u{93f}"),
        (
            "\x1b[?2027h  \u{915}\u{94d}\u{924}\u{93f}\x1b[1G",
            "\u{915}",
        ),
    ] {
        let written = frame(
            &mut r,
            format!("{base}\x1b[4m{text}\x1b[24m").as_bytes(),
            None,
        );
        let how = r
            .words_on(baud_domain::Row::ZERO)
            .iter()
            .find(|l| l.at.index() == 2)
            .map(|l| l.how);
        assert_eq!(
            how,
            Some(baud_render::WordHow::Residue),
            "{base:?} {text:?}: the premise, a residue"
        );
        let composed = frame(&mut r, base.as_bytes(), Some(text));
        let d = composed
            .iter()
            .zip(&written)
            .filter(|(a, b)| a != b)
            .count();
        if d != 0 {
            wrong.push((base, text, d));
        }
    }
    assert!(
        wrong.is_empty(),
        "not as written, pixels differing: {wrong:?}"
    );
}

/// **Ink over the input method's block takes the cursor's text colour**:
/// कमल composed after a blank on Kohinoor Devanagari with the input
/// method's cursor on म, whose cell the block fills; every piece of the
/// word's ink over that cell is drawn in another colour than the block's,
/// as a written word's ink is over a block on a blank cell beside it.
#[cfg(target_os = "macos")]
#[test]
fn ink_over_the_input_methods_block_takes_the_cursors_text_colour() {
    let data = jbm();
    let deva = std::fs::read("/System/Library/Fonts/Kohinoor.ttc")
        .expect("Kohinoor Devanagari, which macOS ships");
    let faces = [
        Face::parse(&data).unwrap(),
        Face::parse_index(&deva, baud_font::FaceIndex::of(0u32)).unwrap(),
    ];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"ab ");
    let (row, col) = t.cursor();
    r.set_composition(Some(Composition {
        row,
        col,
        text: cps("\u{915}\u{92e}\u{932}"),
        measure: Measure::of(&t),
        hints: Vec::new(),
        caret: Caret::At(1),
    }));
    let mut c = Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let cw = i32::try_from(r.metrics().cell_w.value()).unwrap();
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let block = r.background()[4];
    assert_ne!(block, r.background()[3], "the premise: the block on म");
    let mut pieces = 0usize;
    for i in r
        .instances()
        .filter(|i| i.kind == baud_render::Kind::Mask && i32::from(i.y.0) < ch)
    {
        let (x0, x1) = (i32::from(i.x.0), i32::from(i.x.0) + i32::from(i.w.0));
        if x1 > 4 * cw && x0 < 5 * cw {
            assert_ne!(i.color, block, "ink at {x0}..{x1} in the block's colour");
            pieces += 1;
        }
    }
    assert!(pieces > 0, "the word's ink reaches the block's cell");
}

/// Noto Sans Devanagari's variable file from the README's cache, pinned by
/// its sha256 as `devanagari_words.rs` pins it, or `None` to skip.
fn noto_variable() -> Option<Vec<u8>> {
    let cache = std::env::var_os("NOTO_CACHE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(|d| std::path::PathBuf::from(d).join("baud-readme"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/baud-readme"))
        })?;
    let Ok(data) = std::fs::read(cache.join("NotoSansDevanagari[wdth,wght].ttf")) else {
        eprintln!("Noto Sans Devanagari's variable file is not in the README's cache: skipped");
        return None;
    };
    assert_eq!(
        sha256::sha256_hex(&data),
        "9f82923465ef82fb86c6b99cbdd4947ba832fe3335948de815b18adfd75947c7",
        "the cache's file is not the pinned one"
    );
    Some(data)
}

/// **The same on a face with a width axis**: Noto Sans Devanagari's
/// variable file behind JetBrains Mono, with its narrower instances as the
/// app's stack lays them, where a word composed between two written `x`
/// is drawn at a narrower width as the commit's is -- under `?2027`, मल
/// composed after a written क, कम before a written ल and टर्मिनल alone,
/// each narrowed, the premise -- and the other cases above, each equal to
/// the same text written underlined at the cursor.
#[test]
fn a_composition_against_a_devanagari_word_draws_as_written_when_narrowed() {
    use baud_font::var::{Fixed, Variation};
    use baud_render::{FaceRole, Stack};
    let Some(var) = noto_variable() else {
        return;
    };
    let data = jbm();
    let mut faces = vec![Face::parse(&data).unwrap(), Face::parse(&var).unwrap()];
    let mut roles = vec![FaceRole::Fallback, FaceRole::Fallback];
    for w in baud_render::narrower_widths(&faces[1], &[]) {
        let mut f = Face::parse(&var).unwrap();
        f.set_variations(&[Variation {
            tag: baud_font::Tag::from_bytes(*b"wdth"),
            value: Fixed::from_f32(w),
        }]);
        faces.push(f);
        roles.push(FaceRole::Narrower);
    }
    let mut r = baud_render::Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        baud_render::PxSize::of(16.0).unwrap(),
        baud_render::Theme::dark(),
        baud_render::Blend::Native,
        baud_domain::Cols::of_index(usize::from(WS.ws_col)),
        baud_domain::Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut narrowed = 0usize;
    let mut wrong = Vec::new();
    for (base, text) in [
        ("\x1b[?2027hx\u{915}  x\x1b[3D", "\u{92e}\u{932}"),
        ("\x1b[?2027hx  \u{932}x\x1b[4D", "\u{915}\u{92e}"),
        (
            "\x1b[?2027hx    x\x1b[5D",
            "\u{91f}\u{930}\u{94d}\u{92e}\u{93f}\u{928}\u{932}",
        ),
        ("\x1b[?2027hab\u{915}\u{93f}", "\u{92e}\u{932}"),
        ("ab ", "\u{915}\u{92e}\u{932}"),
        ("ab\u{915}", "\u{92e}\u{932}"),
        ("ab\u{915}", "\u{92e}"),
        ("ab  \u{932}\x1b[2D", "\u{915}"),
        ("ab \u{915} \u{932}\x1b[2D", "\u{92e}"),
        ("ab  \u{937}\x1b[2D", "\u{915}\u{94d}"),
        ("ab  \u{915}\u{92e}\u{932}\x1b[4D", "x"),
        ("\x1b[?2027h\u{915} ", "\u{915}\u{94d}\u{924}\u{93f}"),
        (
            "\x1b[?2027h  \u{915}\u{94d}\u{924}\u{93f}\x1b[1G",
            "\u{915}",
        ),
    ] {
        let written = frame(
            &mut r,
            format!("{base}\x1b[4m{text}\x1b[24m").as_bytes(),
            None,
        );
        if base.starts_with("\x1b[?2027hx") {
            narrowed += r
                .words_on(baud_domain::Row::ZERO)
                .iter()
                .filter(|l| l.instance > 0)
                .count();
        }
        let composed = frame(&mut r, base.as_bytes(), Some(text));
        let d = composed
            .iter()
            .zip(&written)
            .filter(|(a, b)| a != b)
            .count();
        if d != 0 {
            wrong.push((base, text, d));
        }
    }
    assert_eq!(narrowed, 3, "the premise: each drawn narrower");
    assert!(
        wrong.is_empty(),
        "not as written, pixels differing: {wrong:?}"
    );
}
