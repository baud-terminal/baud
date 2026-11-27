//! A change of pixel size rebuilds what depends on it: the metrics, the
//! atlas's glyphs for the size, and through the atlas's grow count
//! everything that kept a slot.
//! What the app does when the backing scale changes or the font is zoomed.

mod support;
use baud_font::Face;
use baud_render::{Damage, PxSize};
use support::*;

#[test]
fn a_rescale_moves_the_metrics_and_restarts_the_atlas() {
    let bytes = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&bytes).unwrap()];
    let mut r = renderer(&faces);
    let before = r.metrics();
    let grown = r.atlas_stats().grown;
    replay(b"hello rescale", |t, _| {
        r.fit(t);
        r.convert(t, baud_domain::Rows::ZERO, Damage::Full);
        assert!(r.rescale(PxSize::of(PX * 2.0).unwrap()).is_some());
        let after = r.metrics();
        assert!(
            after.cell_h.value() > before.cell_h.value()
                && after.cell_w.value() > before.cell_w.value(),
            "twice the size is a taller and wider cell: {before:?} -> {after:?}"
        );
        assert_ne!(
            r.atlas_stats().grown.value(),
            grown.value(),
            "the grow count moved"
        );
        // The next convert repaints every row, at the new size.
        let c = r.convert(t, baud_domain::Rows::ZERO, Damage::Incremental);
        assert_eq!(
            c.rows_dirty.value(),
            c.rows.value(),
            "every row is dirty after a rescale"
        );
    });
}
