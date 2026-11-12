//! **A veil kept is never a veil stale** (FS-C16): baud's veils -- the
//! fields of the link under the pointer, the find field, the link
//! preview's bar, the hover tip's lines, the notice -- and the input
//! method's composition, which their key covers too, are drawn on the CPU
//! path from records kept while nothing they are drawn from moves. One
//! control a kind: a frame whose veil changed draws, pixel for pixel, what
//! a renderer of its own draws in one full frame, which has nothing kept;
//! and a steady frame composes none, the count the kept records save.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Blend, Caret, Composition, Damage, Field, FindLook, Hints, HoverPaint, LinkHover, LinkLook,
    LinkSpan, Measure, PxSize, Renderer, Stack, Target, Theme, TipArea, TipLook,
};
use support::font;

const COLS: usize = 40;
const ROWS: usize = 8;

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn text(s: &str) -> Vec<Codepoint> {
    s.chars().map(Codepoint::from_char).collect()
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

/// **A change of a veil draws what a fresh renderer draws**: a screen of
/// text `ROWS` rows high, `rows` of them the terminal's (one fewer leaves
/// the notice its row), `before` setting a veil on the renderer, a full
/// frame and two incremental ones; then a steady frame, which composes no
/// veil -- the control that the records are kept -- and `after` changing
/// the veil, `written` fed to the terminal, and an incremental frame, which
/// draws, pixel for pixel, what a renderer of its own draws in one full
/// frame given the veils as they stand then, which has nothing kept to
/// draw stale -- the staleness control -- and other pixels than the steady
/// one -- the control that the change shows -- and composes again; and
/// what the same renderer's full frame then draws.
fn a_changed_veil_is_drawn(
    what: &str,
    rows: Rows,
    before: impl Fn(&mut Renderer<'_, '_>, &Term<'_>),
    after: impl Fn(&mut Renderer<'_, '_>, &Term<'_>),
    written: &[u8],
    counted: Counted,
) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let made = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(COLS),
            Rows::of_index(ROWS),
        )
        .unwrap()
    };
    let mut r = made();
    let ws = WinSize {
        ws_col: u16::try_from(COLS).unwrap(),
        ws_row: u16::try_from(rows.index()).unwrap(),
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let lines: Vec<u8> = (0..rows.index())
        .flat_map(|i| format!("\x1b[{};1Hrow {i} abcdefghijklmnopqrstuvwxyz", i + 1).into_bytes())
        .collect();
    feed(&mut t, &lines);
    before(&mut r, &t);
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let (mut px, mut full) = (vec![0u32; n], vec![0u32; n]);
    let paint = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], d: Damage| {
        let mut target = Target::new(px, w, w, h).unwrap();
        r.render(t, &mut target, d);
    };
    paint(&mut r, &mut t, &mut px, Damage::Full);
    for _ in 0..2 {
        paint(&mut r, &mut t, &mut px, Damage::Incremental);
    }
    let composed = r.veils_composed();
    paint(&mut r, &mut t, &mut px, Damage::Incremental);
    assert_eq!(
        r.veils_composed(),
        composed,
        "{what}: the control, a steady frame composes no veil"
    );
    let steady = px.clone();
    after(&mut r, &t);
    if !written.is_empty() {
        feed(&mut t, written);
    }
    paint(&mut r, &mut t, &mut px, Damage::Incremental);
    // The pixels first, so a veil kept stale fails here, at the staleness
    // control, before the count says why.
    let mut fresh = made();
    before(&mut fresh, &t);
    after(&mut fresh, &t);
    fresh.fit(&t);
    paint(&mut fresh, &mut t, &mut full, Damage::Full);
    assert!(
        px == full,
        "{what}: the frame after the change is a fresh renderer's pixels"
    );
    assert!(px != steady, "{what}: the control, the change is drawn");
    if counted == Counted::Composed {
        assert!(composed.value() > 0, "{what}: the control, a veil composed");
        assert!(
            r.veils_composed().value() > composed.value(),
            "{what}: the changed veil composed again"
        );
    }
    paint(&mut r, &mut t, &mut full, Damage::Full);
    assert!(
        px == full,
        "{what}: the frame after the change is the full frame's pixels"
    );
}

/// Whether a veil's change composes one: every veil's does, but the
/// composition's, whose records are its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Counted {
    Composed,
    Drawn,
}

/// The link under the pointer, drawn as a box over `from..=to` of row 3.
fn hover(to: usize) -> LinkHover {
    LinkHover {
        spans: vec![LinkSpan {
            row: Row::from_index(2),
            from: Col::at_index(6),
            to: Col::at_index(to),
        }],
        paint: HoverPaint::Box,
    }
}

/// **The link under the pointer moved over the same row** draws its new
/// fields. Named break: the key never moving, so the kept fields are
/// drawn.
#[test]
fn a_hovered_link_that_changed_is_drawn() {
    a_changed_veil_is_drawn(
        "the link under the pointer",
        Rows::of_index(ROWS),
        |r, _| r.set_link_hover(Some(&hover(12))),
        |r, _| r.set_link_hover(Some(&hover(20))),
        b"",
        Counted::Composed,
    );
}

/// **A cell written under the link under the pointer** is drawn in its
/// field: the field shows the cells' characters, so the key that holds
/// the field moves with them.
#[test]
fn a_cell_written_under_a_hovered_link_is_drawn() {
    a_changed_veil_is_drawn(
        "a cell under the link under the pointer",
        Rows::of_index(ROWS),
        |r, _| r.set_link_hover(Some(&hover(12))),
        |_, _| {},
        b"\x1b[3;9HZ",
        Counted::Composed,
    );
}

/// **The find field typed into** draws its new text.
#[test]
fn a_find_field_that_changed_is_drawn() {
    a_changed_veil_is_drawn(
        "the find field",
        Rows::of_index(ROWS),
        |r, t| {
            r.set_find(
                t,
                Some(FindLook {
                    marks: Vec::new(),
                    field: field(ROWS - 1, 0, "Find abc"),
                }),
            )
        },
        |r, t| {
            r.set_find(
                t,
                Some(FindLook {
                    marks: Vec::new(),
                    field: field(ROWS - 1, 0, "Find abd"),
                }),
            )
        },
        b"",
        Counted::Composed,
    );
}

/// The link preview with its bar reading `s` on row 5.
fn link(s: &str) -> LinkLook {
    LinkLook {
        spans: Vec::new(),
        bar: field(4, 2, s),
    }
}

/// **The link preview's bar naming another address** draws it.
#[test]
fn a_link_bar_that_changed_is_drawn() {
    a_changed_veil_is_drawn(
        "the link preview's bar",
        Rows::of_index(ROWS),
        |r, _| r.set_link(Some(link("https://example.org/a"))),
        |r, _| r.set_link(Some(link("https://example.org/b"))),
        b"",
        Counted::Composed,
    );
}

/// The hover tip, its one line reading `s` on row 6.
fn tip(s: &str) -> TipLook {
    TipLook {
        lines: vec![field(5, 10, s)],
        area: TipArea {
            top: Row::from_index(5),
            bottom: Row::from_index(6),
            left: Col::at_index(10),
            right: Col::at_index(COLS - 1),
        },
    }
}

/// **The hover tip's line saying something else** draws it.
#[test]
fn a_tip_that_changed_is_drawn() {
    a_changed_veil_is_drawn(
        "the hover tip",
        Rows::of_index(ROWS),
        |r, _| r.set_tip(Some(tip("a word's meaning"))),
        |r, _| r.set_tip(Some(tip("another meaning"))),
        b"",
        Counted::Composed,
    );
}

/// **The notice saying something else** draws it, on its own row below
/// the terminal's.
#[test]
fn a_notice_that_changed_is_drawn() {
    a_changed_veil_is_drawn(
        "the notice",
        Rows::of_index(ROWS - 1),
        |r, _| r.set_notice(Some(text("config: one key unread"))),
        |r, _| r.set_notice(Some(text("config: two keys unread"))),
        b"",
        Counted::Composed,
    );
}

/// An input method's composition of `s` at row 4, column 3.
fn composition(s: &str) -> Composition {
    Composition {
        row: Row::from_index(3),
        col: Col::at_index(3),
        text: text(s),
        measure: Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    }
}

/// **The input method's composition changed** draws it: the key covers it
/// with the other veils, while its own records are drawn every frame.
#[test]
fn a_composition_that_changed_is_drawn() {
    a_changed_veil_is_drawn(
        "the composition",
        Rows::of_index(ROWS),
        |r, _| r.set_composition(Some(composition("ni"))),
        |r, _| r.set_composition(Some(composition("nih"))),
        b"",
        Counted::Drawn,
    );
}
