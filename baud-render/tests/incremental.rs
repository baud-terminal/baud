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
