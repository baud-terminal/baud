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

use baud_core::{Owned, Term};
use baud_domain::{AmbiguousWidth, BytePos, Codepoint, ReadLen};
use baud_font::Face;
use baud_render::{Composition, Damage, Renderer};
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
    r.set_composition(composition.map(|s| Composition {
        row,
        col,
        text: cps(s),
        ambiguous: AmbiguousWidth::Narrow,
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
        ambiguous: AmbiguousWidth::Narrow,
    }));
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    // Committed or cancelled: the incremental frame after it is the grid.
    r.set_composition(None);
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(c.pixels, plain, "the cells under it were repainted");
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
        ambiguous: AmbiguousWidth::Narrow,
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
        ambiguous: AmbiguousWidth::Narrow,
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
        ambiguous,
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
        assert_eq!(c.cursor(cols).1.index(), width, "{text:?} {ambiguous:?}");
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
