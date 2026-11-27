//! A new stack of faces rebuilds what depends on its faces: the metrics, the
//! per-face tables, the run cache and the atlas. What the app does when a
//! program's `OSC 50` names another family.

mod support;
use baud_font::Face;
use baud_render::{Damage, PxSize, Stack};
use support::*;

#[test]
fn a_restack_takes_the_new_faces_metrics_and_shapes_afresh() {
    let jb = font("JetBrainsMono-Regular.ttf");
    let fira = font("FiraCode-Regular.ttf");
    let first = [Face::parse(&jb).unwrap()];
    let second = [Face::parse(&fira).unwrap()];
    let want = renderer(&second).metrics();
    let mut r = renderer(&first);
    let before = r.metrics();
    assert_ne!(
        before, want,
        "the two faces measure apart, or this proves nothing"
    );
    let grown = r.atlas_stats().grown;
    replay(b"hello restack", |t, _| {
        r.fit(t);
        r.convert(t, baud_domain::Rows::ZERO, Damage::Full);
        r.convert(t, baud_domain::Rows::ZERO, Damage::Full);
        assert!(
            r.cache_stats().hits.value() > 0,
            "the control: a warm cache hits"
        );
        let stack = Stack::new(&second).unwrap();
        assert!(r.restack(stack, PxSize::of(PX).unwrap()).is_some());
        assert_eq!(r.metrics(), want, "the new primary's metrics");
        assert_ne!(
            r.atlas_stats().grown.value(),
            grown.value(),
            "the atlas restarted"
        );
        let c = r.convert(t, baud_domain::Rows::ZERO, Damage::Incremental);
        assert_eq!(c.rows_dirty.value(), c.rows.value(), "every row is dirty");
        let s = r.cache_stats();
        assert_eq!(
            s.hits.value(),
            0,
            "nothing shaped in the old faces is reused"
        );
        assert!(s.misses.value() > 0);
    });
}
