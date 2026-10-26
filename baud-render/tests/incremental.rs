//! Incremental painting equals a full repaint, pixel for pixel, at every
//! read boundary -- the chunk-invariance discipline applied to pixels.

mod support;

use baud_font::Face;
use baud_render::Damage;
use support::*;

#[test]
fn incremental_frames_equal_full_repaints_at_every_read_boundary() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut compared = 0usize;
    let mut partial = 0usize;
    // The three TUI recordings, which redraw in place and so exercise
    // partial damage; a scrolling `cat` replaces the screen every read and
    // tests nothing the full path does not.
    for name in ["utf8/btop.raw", "utf8/vim-unicode.raw", "utf8/cava.raw"] {
        let bytes = recording(name);
        let mut inc = renderer(&faces);
        let mut full = renderer(&faces);
        let mut a = Canvas::for_renderer(&inc);
        let mut b = Canvas::for_renderer(&full);
        replay(&bytes, |t, i| {
            let f = paint(&mut inc, t, &mut a, Damage::Incremental);
            if f.rows_dirty.value() < f.rows.value() {
                partial += 1;
            }
            // Every eighth read, and the first, against a fresh full frame.
            if i % 8 == 0 {
                paint(&mut full, t, &mut b, Damage::Full);
                assert!(
                    a.pixels == b.pixels,
                    "{name}: read {i} differs from a full repaint"
                );
                compared += 1;
            }
        });
    }
    assert!(compared >= 40, "{compared}");
    // Positive control: damage did skip rows somewhere, or nothing was
    // being tested.
    assert!(partial >= 10, "{partial}");
}

/// **A glyph reaching out of its row is drawn into the row it reaches, and
/// a frame that changes it repaints that row.** U+01D8's acute and an `A`
/// with two acutes reach above their cells at 14 px; the CPU compositor
/// draws them into the band above, in the instance path's order, and a
/// row whose glyphs reach, or reached, another repaints it with them --
/// so an incremental frame equals a full one as the reaching glyph comes
/// and goes, and the premise holds: the full frames differ in the band
/// above.
#[test]
fn a_glyph_reaching_into_another_row_repaints_it() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen};
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut inc = renderer(&faces);
    let mut full = renderer(&faces);
    let mut a = Canvas::for_renderer(&inc);
    let mut b = Canvas::for_renderer(&full);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let band = usize::try_from(inc.metrics().cell_h.value()).unwrap() * a.width;
    // The pixels of the band of row 2, above the reaching glyph's row 3.
    let above = |c: &Canvas| c.pixels[2 * band..3 * band].to_vec();
    let mut was: Option<Vec<u32>> = None;
    for (step, bytes) in [
        "\x1b[?25l\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[31mu",
        "\x1b[4;5H\u{1d8}",
        "\x1b[4;5Hu",
        "\x1b[4;5HA\u{301}\u{301}",
        "\x1b[4;5H ",
    ]
    .iter()
    .enumerate()
    {
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        paint(&mut inc, &mut t, &mut a, Damage::Incremental);
        paint(&mut full, &mut t, &mut b, Damage::Full);
        assert!(
            a.pixels == b.pixels,
            "step {step} {bytes:?}: the incremental frame differs from a full repaint"
        );
        let now = above(&b);
        if let Some(w) = &was {
            assert!(
                *w != now,
                "step {step} {bytes:?}: the premise, the reaching glyph changes the band above"
            );
        }
        was = Some(now);
    }
}

/// **Every pixel an incremental frame changes is in a band
/// [`baud_render::Renderer::painted`] names**, which is what a presenter
/// copies and tells its compositor changed: across the three recordings
/// that redraw in place, against the frame before. The controls: some
/// frame changed pixels, and some frame named fewer rows than the grid
/// has, or the bands were the whole frame and nothing was tested.
#[test]
fn the_painted_bands_hold_every_pixel_a_frame_changes() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let (mut changed, mut narrow) = (0usize, 0usize);
    for name in ["utf8/btop.raw", "utf8/vim-unicode.raw", "utf8/cava.raw"] {
        let bytes = recording(name);
        let mut r = renderer(&faces);
        let mut c = Canvas::for_renderer(&r);
        let mut was = Canvas::for_renderer(&r);
        replay(&bytes, |t, i| {
            was.pixels.copy_from_slice(&c.pixels);
            paint(&mut r, t, &mut c, Damage::Incremental);
            let bands: Vec<_> = r.painted().collect();
            let missed = uncovered(&was, &c, &bands);
            assert!(
                missed.is_empty(),
                "{name}: read {i} changed rows {missed:?} unnamed"
            );
            if was.pixels != c.pixels {
                changed += 1;
            }
            let named: u32 = bands.iter().map(|b| b.bottom.value() - b.top.value()).sum();
            if usize::try_from(named).unwrap() < c.height {
                narrow += 1;
            }
        });
    }
    assert!(changed >= 40, "{changed}");
    assert!(narrow >= 10, "{narrow}");
}

/// **A joining form that moves with no cell of its row changing repaints
/// the row.** The last beh of a row is final until a beh written past the
/// margin wraps onto the next row and makes it medial: the word is joined
/// across the wrap (`bidi.rs`), so the row above changes form though none
/// of its cells did. The incremental frame equals a full one, and the
/// premise holds: the full frames differ in the row above. On Courier New,
/// macOS's, whose `GSUB` draws the forms.
#[cfg(target_os = "macos")]
#[test]
fn a_moved_joining_form_repaints_its_row() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen};
    let data = font("JetBrainsMono-Regular.ttf");
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&courier).unwrap()];
    let mut inc = renderer(&faces);
    let mut full = renderer(&faces);
    let mut a = Canvas::for_renderer(&inc);
    let mut b = Canvas::for_renderer(&full);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let band = usize::try_from(inc.metrics().cell_h.value()).unwrap() * a.width;
    let first = |c: &Canvas| c.pixels[..band].to_vec();
    let row = format!(
        "\x1b[?25l\x1b[8h{}\u{0628}\u{0628}\u{0628}",
        "x".repeat(usize::from(WS.ws_col) - 3)
    );
    let mut was: Option<Vec<u32>> = None;
    for (step, bytes) in [row.as_str(), "\u{0628}"].iter().enumerate() {
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        paint(&mut inc, &mut t, &mut a, Damage::Incremental);
        paint(&mut full, &mut t, &mut b, Damage::Full);
        assert!(
            a.pixels == b.pixels,
            "step {step}: the incremental frame differs from a full repaint"
        );
        let now = first(&b);
        if let Some(w) = &was {
            assert!(*w != now, "the premise: the form on the first row moved");
        }
        was = Some(now);
    }
}

/// **A joined letter whose word changes on another row repaints its row.**
/// A Syriac alaph wrapped past the margin, after a yudh in the last column
/// of the row above, draws its plain final form; the yudh written over
/// with a beth leaves the alaph's row and its form as they were, and only
/// the letter its shaping reads changes: Noto Sans Syriac's `rlig` picks
/// a wider final after a beth. The incremental frame equals a full one,
/// and the premise holds: the full frames differ in the alaph's row. On
/// Noto Sans Syriac, which macOS ships.
#[cfg(target_os = "macos")]
#[test]
fn a_changed_word_repaints_the_row_of_the_letter_that_reads_it() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen};
    let data = font("JetBrainsMono-Regular.ttf");
    let syriac = std::fs::read("/System/Library/Fonts/Supplemental/NotoSansSyriac-Regular.ttf")
        .expect("Noto Sans Syriac, which macOS ships");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&syriac).unwrap()];
    let mut inc = renderer(&faces);
    let mut full = renderer(&faces);
    let mut a = Canvas::for_renderer(&inc);
    let mut b = Canvas::for_renderer(&full);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let band = usize::try_from(inc.metrics().cell_h.value()).unwrap() * a.width;
    let second = |c: &Canvas| c.pixels[band..2 * band].to_vec();
    let cols = usize::from(WS.ws_col);
    let row = format!("\x1b[?25l\x1b[8h{}\u{071D}\u{0710}", "x".repeat(cols - 1));
    let over = format!("\x1b[1;{cols}H\u{0712}");
    let mut was: Option<Vec<u32>> = None;
    for (step, bytes) in [row.as_str(), over.as_str()].iter().enumerate() {
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        paint(&mut inc, &mut t, &mut a, Damage::Incremental);
        paint(&mut full, &mut t, &mut b, Damage::Full);
        assert!(
            a.pixels == b.pixels,
            "step {step}: the incremental frame differs from a full repaint"
        );
        let now = second(&b);
        if let Some(w) = &was {
            assert!(
                *w != now,
                "the premise: the alaph on the second row changed"
            );
        }
        was = Some(now);
    }
}

/// **A frame reads only the rows that changed to find what joins, and with
/// BDSM reset none until a joining letter is drawn.** With the mode reset
/// no row is compared or tested while the renderer has shaped no letter
/// that takes a form; the frame that shapes the first is converted again,
/// every row tested, and after it each frame tests only the rows its own
/// compare finds changed. With BDSM set each row is compared with its last
/// copy once a frame, as before joining existed, and a row that changed is
/// tested once, for what reorders and what joins together.
#[test]
fn a_frame_reads_only_the_rows_that_changed_for_joining() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen};
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let rows = u64::from(WS.ws_row);
    let mut step = |bytes: &str| {
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        paint(&mut r, &mut t, &mut c, Damage::Incremental);
        let w = r.bidi_work();
        (w.compared.value(), w.tested.value())
    };
    let none = "no joining letter drawn, BDSM reset";
    assert_eq!(step("\x1b[?25l\x1b[8lhello"), (0, 0), "{none}");
    assert_eq!(step("\r\nworld"), (0, 0), "{none}, a row changed");
    assert_eq!(step("\x1b[8h"), (rows, rows), "the first frame, BDSM set");
    assert_eq!(step(""), (rows, 0), "nothing changed, BDSM set");
    assert_eq!(step("\x1b[1;1Hy"), (rows, 1), "one row changed, BDSM set");
    assert_eq!(
        step("\x1b[8l\x1b[3;1H\u{0628}\u{0628}"),
        (0, rows),
        "the first joining letter, BDSM reset: every row"
    );
    assert_eq!(step(""), (0, 0), "nothing changed, BDSM reset");
    assert_eq!(step("\x1b[1;1Hj"), (0, 1), "one row changed, BDSM reset");
}

/// **With BDSM reset, the joins the renderer keeps are the screen's**:
/// Arabic written in display order is changed a letter at a time, erased,
/// scrolled, and veiled by the find field over a cell whose letter then
/// changes under it, and every incremental frame equals a fresh renderer's
/// full one. On Courier New, macOS's, whose `GSUB` draws the forms.
#[cfg(target_os = "macos")]
#[test]
fn joins_kept_with_bdsm_reset_follow_the_screen() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Col, ReadLen, Row};
    let data = font("JetBrainsMono-Regular.ttf");
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&courier).unwrap()];
    let mut inc = renderer(&faces);
    let mut a = Canvas::for_renderer(&inc);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let last = usize::from(WS.ws_row);
    let steps: Vec<String> = vec![
        "\x1b[?25l\x1b[8l\x1b[3;5H\u{062A}\u{064A}\u{0628}".into(),
        "\x1b[3;6H\u{0628}".into(),
        "\x1b[3;6H\x1b[X".into(),
        "\x1b[3;6H\u{0644}\x1b[3;5H\u{0627}".into(),
        format!("\x1b[{last};1H\n\n"),
        "\x1b[1;5H\u{0628}\u{0628}\u{0628}".into(),
    ];
    // The find field over row 1's middle cell, the fourth step's beh, under
    // which an `x` is then written: the compare sees the field's blank
    // cell, not the letter under it, and the behs beside it, joined to
    // the letter, are drawn alone once it is an `x`.
    let field = baud_render::FindLook {
        marks: Vec::new(),
        field: baud_render::Field {
            row: Row::from_index(0),
            col: Col::at_index(5),
            text: vec![baud_domain::Codepoint::from_char('F')],
            hints: vec![baud_render::Hints::default()],
            caret: Col::at_index(6),
        },
    };
    let veiled: Vec<(String, Option<baud_render::FindLook>)> = vec![
        (String::new(), Some(field.clone())),
        ("\x1b[1;6Hx".into(), Some(field.clone())),
        (String::new(), None),
    ];
    let shown = steps.into_iter().map(|s| (s, None));
    for (i, (bytes, look)) in shown.chain(veiled).enumerate() {
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        inc.set_find(&t, look.clone());
        paint(&mut inc, &mut t, &mut a, Damage::Incremental);
        let mut fresh = renderer(&faces);
        fresh.set_find(&t, look);
        let mut b = Canvas::for_renderer(&fresh);
        paint(&mut fresh, &mut t, &mut b, Damage::Full);
        assert!(a.pixels == b.pixels, "step {i}: the kept joins differ");
    }
}

/// **A mirrored cell that turns with no cell of its row changing repaints
/// the row.** The second row is a soft-wrapped continuation of U+2211, the
/// same cells read either way round, so its paragraph's direction alone
/// decides whether each is flipped (UAX #9 L4): while the paragraph starts
/// with an alef it is at level 1 and they are, and once an `x` is written
/// over the alef it is at level 0 and they are not. The incremental frame
/// equals a full one, and the premise holds: the full frames differ in the
/// second row.
#[test]
fn a_turned_mirror_repaints_its_row() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen};
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut inc = renderer(&faces);
    let mut full = renderer(&faces);
    let mut a = Canvas::for_renderer(&inc);
    let mut b = Canvas::for_renderer(&full);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let band = usize::try_from(inc.metrics().cell_h.value()).unwrap() * a.width;
    let second = |c: &Canvas| c.pixels[band..2 * band].to_vec();
    let cols = usize::from(WS.ws_col);
    let text = format!("\x1b[?25l\x1b[8h\u{5D0}{}", "\u{2211}".repeat(2 * cols - 1));
    let mut was: Option<Vec<u32>> = None;
    for (step, bytes) in [text.as_str(), "\x1b[1;1Hx"].iter().enumerate() {
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        paint(&mut inc, &mut t, &mut a, Damage::Incremental);
        paint(&mut full, &mut t, &mut b, Damage::Full);
        assert!(
            a.pixels == b.pixels,
            "step {step}: the incremental frame differs from a full repaint"
        );
        let now = second(&b);
        if let Some(w) = &was {
            assert!(*w != now, "the premise: the second row's cells turned");
        }
        was = Some(now);
    }
}
