//! **A combining mark with no base draws on a no-break space, in its own
//! cell, and copies as the mark.**
//!
//! After a control a combining mark is a cluster of its own (`[[source:
//! uax29]]` GB4/GB5), a defective combining character sequence that TUS 5.13
//! says "should be rendered as if they had a no-break space as a base
//! character" (`[[source: unicode18-ch5]]`). The core stores the mark alone
//! (`Content::DEFECTIVE`), so the frame must be the frame of a no-break space
//! with the mark on it, typed as such, and the copy the mark the program
//! wrote, no space (note AB, Q24 item 1).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, PackedCell, ReadLen, Row, Rows};
use baud_font::Face;
use baud_render::{Damage, Grain, Scrolled, Selection, Span, copy_for_shader};
use support::{Canvas, WS, font, paint, renderer};

fn frame(faces: &[Face<'_>], bytes: &str) -> Vec<u32> {
    let mut r = renderer(faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut c = Canvas::for_renderer(&r);
    paint(&mut r, &mut t, &mut c, Damage::Full);
    c.pixels
}

#[test]
fn a_mark_after_a_control_draws_on_a_no_break_space_in_its_own_cell() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let defective = frame(&faces, "ab\x00\u{301}");
    let typed = frame(&faces, "ab\u{a0}\u{301}");
    let bare = frame(&faces, "ab");
    let joined = frame(&faces, "ab\u{301}");
    assert_ne!(
        typed, bare,
        "the control: the acute on a no-break space draws"
    );
    assert_ne!(
        joined, typed,
        "the control: the acute on the b draws elsewhere"
    );
    assert_eq!(
        defective, typed,
        "drawn as a no-break space with the acute on it"
    );
}

#[test]
fn a_mark_after_a_control_copies_as_the_mark() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = "ab\x00\u{301}c";
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let view = t.viewport();
    let at = |c: usize| {
        Span::cell(
            view.row(Row::ZERO),
            Scrolled::START.line(Rows::ZERO, Row::ZERO),
            Col::at_index(c),
        )
    };
    let mut s = Selection::at(at(0), Grain::Cell);
    s.extend(at(3));
    assert_eq!(s.text(&t, Scrolled::START), "ab\u{301}c");
}

/// **The cell shader's copy of the screen names the no-break space** for the
/// mark's cell, the base it draws, as every other path's
/// (`Content::drawn`); the marks it draws over it are the overlay's.
#[test]
fn the_shader_copy_names_a_no_break_space_for_the_marks_cell() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = "ab\x00\u{301}";
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let src: Vec<PackedCell> = t.row(Row::ZERO).expect("a row")[..3].to_vec();
    let mut dst = src.clone();
    copy_for_shader(&t, &src, &mut dst);
    assert_eq!(dst[2].content.as_codepoint().map(|c| c.value()), Some(0xa0));
    assert_eq!(
        dst[1].content, src[1].content,
        "the control: an unmarked cell is copied as it is"
    );
}
