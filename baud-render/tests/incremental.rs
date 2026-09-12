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
