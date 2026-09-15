//! The present path, headless: which frame reaches the layer when a frame
//! drawn for one set of surfaces lands after the set has been replaced.
//!
//! This is the bug the owner reported three ways -- a doubled image on a
//! grow, the old font size on a zoom, a resize that "doesn't render btop
//! properly" -- and per the research no shipping terminal has a test for it.
//! ghostty guards it in `setSurfaceCallback` and has zero test blocks in its
//! whole renderer; kitty has no offscreen GL in CI at all; wezterm's only
//! GPU-free texture cannot read pixels back.
//!
//! **Deterministic, because the test chooses the delivery order rather than
//! racing a GPU.** A real completion arrives when the hardware is done, so
//! reproducing "A finished after B" by drawing and hoping is a coin toss.
//! Here the two landings are run by hand, in the order that exposes the
//! defect, through the same [`Journal`] and [`admits`] the completion
//! closure uses -- so the test and the shipping path cannot drift.
//!
//! Everything real except the scheduling: real `IOSurface`s at two sizes, a
//! real `CALayer` with bounds and a contents scale, and `contents()` read
//! back to say which surface arrived. No window, no `NSApplication`, no run
//! loop, and it runs off the main thread, which is where `cargo test` puts
//! it. That was measured with a throwaway probe before this test was
//! written, not assumed.
#![cfg(target_os = "macos")]

use baud_domain::{Fate, FrameGen, SwapEpoch};
use baud_present_macos::journal::{FrameTag, Journal, Show, admits};
use macos_liquid_glass::drawable::Surface;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_quartz_core::CALayer;

/// A layer the size of a window that is `w` by `h` points at `scale`.
fn layer(w: f64, h: f64, scale: f64) -> objc2::rc::Retained<CALayer> {
    let l = CALayer::new();
    l.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h)));
    l.setContentsScale(scale);
    l
}

/// Which surface the layer is showing, by identity rather than by shape --
/// two surfaces of the same size are still two surfaces.
fn showing(l: &CALayer, candidates: &[&Surface]) -> Option<usize> {
    // SAFETY: reading the layer's `contents` property. The probe that
    // preceded this test confirmed it answers a retained object headless,
    // off the main thread, and nothing here dereferences it -- only its
    // address is compared.
    let got = unsafe { l.contents() }?;
    let got = objc2::rc::Retained::as_ptr(&got).cast::<()>();
    candidates.iter().position(|s| {
        let p: *const () = (*s).io_surface() as *const _ as *const ();
        core::ptr::eq(p, got)
    })
}

/// What the completion closure does, spelled once so the test exercises the
/// shipping rule: count it, then show it only if the rule admits it.
fn land(j: &Journal, epoch: &SwapEpoch, tag: FrameTag, l: &CALayer, s: &Surface) -> Fate {
    let fate = j.land(tag, epoch);
    if admits(fate) == Show::Now {
        // What `DrawableView::set_surface` does, minus the transaction the
        // probe showed makes no difference to the read-back.
        unsafe { l.setContents(Some(s.io_surface())) };
    }
    fate
}

/// A frame committed before a resize, landing after it, must not replace the
/// frame drawn for the resize.
///
/// **Pinned at what it does today, not at what it should do.** The rule in
/// `admits` shows everything, so `stale_shown` is 1 here: that is the bug,
/// as a number, and this test is what says so before anything is changed.
/// The fix flips `admits` and this pin moves to 0 -- in either direction, so
/// a regression is as visible as the repair.
#[test]
fn a_frame_that_finished_after_a_resize_is_counted_and_pinned() {
    let epoch = SwapEpoch::new();
    let j = Journal::default();

    // The surface set before the drag, and the frame drawn into it.
    let old = Surface::new(640, 480).expect("a 640x480 surface");
    let a = FrameTag {
        frame: FrameGen::FIRST.next(),
        swap: epoch.current(),
    };

    // The window grows: a new surface set, so everything in flight for the
    // old one is superseded. This is `Presenter::build`'s bump.
    epoch.bump();
    let new = Surface::new(800, 600).expect("an 800x600 surface");
    let b = FrameTag {
        frame: a.frame.next(),
        swap: epoch.current(),
    };

    let l = layer(800.0, 600.0, 1.0);

    // B lands first -- it is the frame drawn for the window as it now is.
    assert_eq!(land(&j, &epoch, b, &l, &new), Fate::Current);
    assert_eq!(showing(&l, &[&old, &new]), Some(1), "B reached the layer");

    // Then A arrives: committed before the resize, finished after it. The
    // GPU gives no ordering guarantee between two command buffers' handlers
    // and the older frame is the smaller one, so this is not a contrived
    // order -- it is the one that produces the artefact.
    let fate = land(&j, &epoch, a, &l, &old);
    assert!(
        matches!(fate, Fate::Superseded { .. }),
        "A names the surface set the resize replaced"
    );

    let c = j.counts();
    assert_eq!(c.landed, 2);
    assert_eq!(c.superseded, 1, "the identity sees it");

    // **The pin.** Today the rule admits it and the stale image lands on
    // top of the current one: a 640x480 frame on an 800x600 layer, which
    // `contentsGravity` puts in the top-left corner -- the owner's "two
    // btops, one overriding the other".
    let stale_shown = usize::from(showing(&l, &[&old, &new]) == Some(0));
    assert_eq!(
        stale_shown, 1,
        "pinned: the superseded frame still reaches the layer. \
         When `admits` stops admitting it this becomes 0, and that is the fix."
    );
}

/// The control that a discard rule must not break: nothing moved, so every
/// frame is current and every frame is shown.
///
/// Without this, a rule that discarded *everything* would satisfy the test
/// above -- and a window that shows no frame at all is black. This is the
/// catastrophic case made into an assertion rather than something the owner
/// discovers on screen.
#[test]
fn a_steady_surface_set_shows_every_frame() {
    let epoch = SwapEpoch::new();
    let j = Journal::default();
    let s = Surface::new(640, 480).expect("a surface");
    let l = layer(640.0, 480.0, 1.0);

    let mut tag = FrameTag {
        frame: FrameGen::FIRST,
        swap: epoch.current(),
    };
    for _ in 0..8 {
        tag = FrameTag {
            frame: tag.frame.next(),
            swap: epoch.current(),
        };
        assert_eq!(land(&j, &epoch, tag, &l, &s), Fate::Current);
        assert_eq!(showing(&l, &[&s]), Some(0));
    }
    let c = j.counts();
    assert_eq!((c.landed, c.shown, c.superseded), (8, 8, 0));
}

/// A zoom at a fixed window size is the same defect wearing different
/// clothes, and it is the one a size comparison cannot catch.
///
/// The window does not move, so the surfaces are rebuilt at **the same
/// dimensions**: 800x600 before and 800x600 after. A guard that compared the
/// frame's size against the layer's would admit the stale frame, because the
/// two sizes agree. The generation does not agree, which is why the
/// discriminant is a counter rather than a shape.
#[test]
fn a_zoom_rebuilds_the_same_size_and_the_old_frame_is_still_superseded() {
    let epoch = SwapEpoch::new();
    let j = Journal::default();

    let before = Surface::new(800, 600).expect("a surface");
    let old = FrameTag {
        frame: FrameGen::FIRST.next(),
        swap: epoch.current(),
    };

    // `present`'s rebuild branch fires on a font change because it keys on
    // the *grid* pixel size, so the surfaces are replaced even though the
    // window is the size it was.
    epoch.bump();
    let after = Surface::new(800, 600).expect("a surface");
    assert_eq!(
        before.size(),
        after.size(),
        "the same shape, by construction"
    );

    let new = FrameTag {
        frame: old.frame.next(),
        swap: epoch.current(),
    };
    let l = layer(800.0, 600.0, 1.0);

    assert_eq!(land(&j, &epoch, new, &l, &after), Fate::Current);
    let fate = land(&j, &epoch, old, &l, &before);
    assert!(
        matches!(fate, Fate::Superseded { .. }),
        "same size, different surfaces: only the generation can tell"
    );
    assert_eq!(j.counts().superseded, 1);
}
