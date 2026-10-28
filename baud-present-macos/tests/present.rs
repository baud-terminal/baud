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

use baud_domain::{Fate, FrameGen, HostTime, SwapEpoch};
use baud_present_macos::journal::{
    FrameTag, Journal, Landing, Reshape, SetState, Show, Wheel, admits,
};
use core::time::Duration;
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

/// A landing whose timing these tests do not examine: the question here
/// is which frame reaches the layer and which set it frees, and a frame's
/// GPU time and end stamp bear on neither.
fn quiet() -> Landing {
    Landing {
        gpu: Duration::ZERO,
        ended: HostTime::ZERO,
    }
}

/// What the completion closure does, spelled once so the test exercises the
/// shipping rule: count it, then show it only if the rule admits it.
fn land(j: &Journal, epoch: &SwapEpoch, tag: FrameTag, l: &CALayer, s: &Surface) -> Fate {
    let fate = j.land(tag, epoch, quiet());
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
/// **Written as the failing test before the fix, and green since it.** When
/// it was written `admits` showed everything, so the superseded frame
/// reached the layer and this test said so before anything was changed.
/// `a9c1a16` made `admits` skip a superseded frame, and the assertions below
/// hold that: the layer keeps the frame drawn for the resize, and a
/// regression that lets the old frame through fails here.
#[test]
fn a_frame_that_finished_after_a_resize_never_reaches_the_layer() {
    let epoch = SwapEpoch::new();
    let j = Journal::default();

    // The surface set before the drag, and the frame drawn into it.
    let old = Surface::new(640, 480).expect("a 640x480 surface");
    let a = FrameTag {
        frame: FrameGen::FIRST.next(),
        swap: epoch.current(),
        set: None,
    };

    // The window grows: a new surface set, so everything in flight for the
    // old one is superseded. This is `Presenter::build`'s bump.
    epoch.bump();
    let new = Surface::new(800, 600).expect("an 800x600 surface");
    let b = FrameTag {
        frame: a.frame.next(),
        swap: epoch.current(),
        set: None,
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

    // **The bug, as the thing that must not happen.** A frame drawn into
    // surfaces the resize replaced must not reach the layer: it is a
    // 640x480 image landing on an 800x600 layer *after* the frame drawn for
    // the new size, and `contentsGravity` puts it in the top-left corner --
    // the owner's "two btops, one overriding the other". No frame the layer
    // shows may be half-done or a size behind.
    assert_eq!(
        showing(&l, &[&old, &new]),
        Some(1),
        "the layer still shows B: a superseded frame must not overwrite a current one"
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
        set: None,
    };
    for _ in 0..8 {
        tag = FrameTag {
            frame: tag.frame.next(),
            swap: epoch.current(),
            set: None,
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
        set: None,
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
        set: None,
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

/// **A frame is never built into a set a frame still in flight holds.**
///
/// That is the write race stated as an invariant, because the CPU writing
/// a buffer the GPU is reading is exactly one set handed out twice. The
/// per-frame buffers -- the resolved styles and the overlay mask -- are
/// the app's, but which set a frame gets is decided here, so the rule the
/// app runs and the rule this pins are the same code.
///
/// Headless and deterministic, because the interleaving is chosen rather
/// than raced. The menu-driven check in `scripts/zoom-drive.sh` cannot
/// reach this at all: a frame is in flight for roughly 300 microseconds
/// of every tick, so a click cannot be aimed at the window where it
/// matters, and twenty zooms through it deferred exactly zero times.
#[test]
fn a_frame_is_never_built_into_a_set_a_live_frame_holds() {
    let j = Journal::default();
    let w = j.wheel();
    let epoch = SwapEpoch::new();
    let mut seq = FrameGen::FIRST;
    let mut commit = |set| {
        seq = seq.next();
        w.committed(set, seq);
        FrameTag {
            frame: seq,
            swap: epoch.current(),
            set: Some(set),
        }
    };

    // **Derived from `Wheel::SETS`, not from a number written here.** The
    // depth is a consequence of D14 -- one frame in flight -- and it moved
    // from three to two when that was noticed; a test that spelled three
    // failed for the depth rather than for the property, which is the one
    // thing it must not do.
    let mut held = Vec::new();
    for n in 0..Wheel::SETS {
        let set = w
            .building()
            .unwrap_or_else(|| panic!("a set for frame {n} of {}", Wheel::SETS));
        assert!(
            !held.iter().any(|(s, _)| *s == set),
            "a frame built while another flies must not write its buffers"
        );
        let tag = commit(set);
        assert_eq!(w.state(set), SetState::Held);
        held.push((set, tag));
    }

    // Every set spoken for: the wheel refuses rather than handing back one
    // a live frame is reading. A refusal is a frame not drawn; handing one
    // out would be a frame drawn over.
    assert_eq!(w.building(), None, "nowhere to build is not somewhere");

    // A landing frees exactly the set that frame held, and out of order:
    // the last one committed is landed first.
    let (last, last_tag) = *held.last().expect("at least one set");
    j.land(last_tag, &epoch, quiet());
    assert_eq!(w.state(last), SetState::Free);
    for (set, _) in held.iter().take(held.len() - 1) {
        assert_eq!(
            w.state(*set),
            SetState::Held,
            "one frame's landing is not another's"
        );
    }
    assert_eq!(w.building(), Some(last));

    for (_, tag) in held.iter().take(held.len() - 1) {
        j.land(*tag, &epoch, quiet());
    }
    for (set, _) in &held {
        assert_eq!(w.state(*set), SetState::Free);
    }
    assert!(w.building().is_some(), "and the wheel is usable again");
}

/// The control the test above needs: a path that holds no set must not
/// free one when it lands. The instance and CPU paths convert on the main
/// thread and tag with `None`, and a landing that freed a set it never
/// took would hand the cell path a buffer a live frame is still reading --
/// the same corruption, arrived at from the other side.
#[test]
fn a_frame_that_holds_no_set_frees_none_when_it_lands() {
    let j = Journal::default();
    let w = j.wheel();
    let epoch = SwapEpoch::new();
    let held = w.building().expect("a set");
    let frame = FrameGen::FIRST.next();
    w.committed(held, frame);

    // An instance-path frame lands, carrying the same frame number and no
    // set of its own.
    j.land(
        FrameTag {
            frame,
            swap: epoch.current(),
            set: None,
        },
        &epoch,
        quiet(),
    );
    assert_eq!(
        w.state(held),
        SetState::Held,
        "a frame that took no set cannot give one back"
    );
}

/// **The grid and the atlas have no ring, so a reshape waits.**
///
/// The terminal's grid is read in place by the cell shader and the atlas
/// sheets are sampled by every GPU path, and there is one of each: a
/// reflow or a rescale cannot move aside the way a ringed buffer can.
/// Ringing either is the wrong price -- megabytes and two textures,
/// against an event a keypress or a drag step produces -- so the request
/// waits for a completion that is already coming.
///
/// Chosen rather than raced, which is the whole point: the live harness
/// deferred zero times in twenty zooms because a frame is in flight for
/// roughly 300 microseconds of every tick.
#[test]
fn a_reshape_waits_while_a_frame_is_reading_and_the_wait_is_counted() {
    let j = Journal::default();
    let epoch = SwapEpoch::new();
    let set = j.wheel().building().expect("a set");
    let frame = FrameGen::FIRST.next();

    assert_eq!(j.reshape(), Reshape::Now, "nothing is reading it yet");
    assert_eq!(j.counts().deferred, 0);

    j.commit(Some(set), frame);
    assert_eq!(j.reshape(), Reshape::Wait);
    assert_eq!(j.reshape(), Reshape::Wait);
    assert_eq!(
        j.counts().deferred,
        2,
        "asking is what a caller does before reshaping, so a wait is a deferral"
    );

    j.land(
        FrameTag {
            frame,
            swap: epoch.current(),
            set: Some(set),
        },
        &epoch,
        quiet(),
    );
    assert_eq!(j.reshape(), Reshape::Now, "the reader landed");
    assert_eq!(j.counts().deferred, 2, "and answering Now counts nothing");
}

/// **The discriminating case, and the reason this is not built on the
/// wheel.** An instance-path frame converts on the main thread and holds
/// no buffer set, and it still samples the atlas. A reshape that asked
/// "does any frame hold a set" would answer `Now` here and let a rescale
/// replace the sheets underneath it.
#[test]
fn a_frame_that_holds_no_buffer_set_still_blocks_a_reshape() {
    let j = Journal::default();
    let frame = FrameGen::FIRST.next();
    j.commit(None, frame);

    assert_eq!(
        j.wheel().building(),
        Some(baud_present_macos::journal::SetIx(0)),
        "it holds no set, so every set is free"
    );
    assert_eq!(
        j.reshape(),
        Reshape::Wait,
        "and it is still reading the atlas"
    );
}

/// A reshape waits for the *last* reader, not the first: two frames in
/// flight and one landing leaves the other reading.
#[test]
fn a_reshape_waits_for_the_last_frame_not_the_first() {
    let j = Journal::default();
    let epoch = SwapEpoch::new();
    let one = FrameGen::FIRST.next();
    let two = one.next();
    let first = j.wheel().building().expect("a set");
    j.commit(Some(first), one);
    let second = j.wheel().building().expect("another");
    j.commit(Some(second), two);

    let tag = |frame, set| FrameTag {
        frame,
        swap: epoch.current(),
        set: Some(set),
    };
    j.land(tag(one, first), &epoch, quiet());
    assert_eq!(j.reshape(), Reshape::Wait, "one is still reading");
    j.land(tag(two, second), &epoch, quiet());
    assert_eq!(j.reshape(), Reshape::Now);
}
