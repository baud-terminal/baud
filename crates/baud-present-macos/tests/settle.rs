//! A draw that commits nothing gives its frame back.
//!
//! A user reported that Cmd-plus and Cmd-minus "stopped working suddenly".
//! The presenter minted a frame's tag -- and so counted it as flying --
//! before the GPU looked at whether a frame was still in flight; a draw
//! that answered `Busy`, or made no resource, dropped the completion that
//! would have landed it. From then on every reshape answered `Wait`, and a
//! zoom, the one caller of `reshape`, deferred itself for the life of the
//! window while frames went on drawing.
//!
//! **Deterministic, as the rest of this crate's present tests are:** the
//! `Busy` the live app hit depends on GPU timing, so the test takes the
//! other exit through the same code -- a presenter on the CPU path asked
//! for a GPU draw, which mints the tag and commits nothing.
#![cfg(target_os = "macos")]

use baud_domain::{Cols, Rows};
use baud_font::{Face, Px};
use baud_present_macos::Presenter;
use baud_present_macos::Wake;
use baud_present_macos::journal::Reshape;
use baud_render::{Blend, PxSize, Renderer, Stack, Theme};
use macos_liquid_glass::drawable::{ColorSpace, SurfaceHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Counts what the presenter asks of the view.
#[derive(Default)]
struct Watch {
    presents: AtomicUsize,
    wakes: AtomicUsize,
}

#[derive(Clone, Default)]
struct Watcher(Arc<Watch>);

impl Wake for Watcher {
    fn present(&self, _: &SurfaceHandle) {
        self.0.presents.fetch_add(1, Ordering::Relaxed);
    }

    fn wake(&self) {
        self.0.wakes.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn a_draw_that_commits_nothing_leaves_no_reshape_waiting() {
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
    ))
    .expect("the test font");
    let faces = [Face::parse(&data).expect("a face")];
    let mut renderer = Renderer::new(
        Stack::new(&faces).expect("a stack"),
        PxSize::of(14.0).expect("a size"),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(20),
        Rows::of_index(4),
    )
    .expect("a renderer");
    let mut p = Presenter::new(Px::of(160), Px::of(64), ColorSpace::Srgb).expect("surfaces");
    let watch = Watcher::default();

    assert_eq!(p.reshape(), Reshape::Now, "nothing drawn, nothing reading");
    assert!(
        p.draw(watch.clone(), &mut renderer).is_none(),
        "the CPU path commits no GPU frame"
    );
    assert_eq!(
        p.reshape(),
        Reshape::Now,
        "a frame the GPU never took is not reading anything, so a zoom may go ahead"
    );
    assert_eq!(p.counts().abandoned, 1, "and the give-back is counted");
    assert_eq!(p.counts().landed, 0, "nothing landed");
    assert_eq!(
        (
            watch.0.presents.load(Ordering::Relaxed),
            watch.0.wakes.load(Ordering::Relaxed)
        ),
        (0, 0),
        "and no completion ever ran"
    );

    // A burst, as a relayout mid-frame produced: none of it may accumulate.
    for _ in 0..5 {
        p.draw(watch.clone(), &mut renderer);
    }
    assert_eq!(
        p.reshape(),
        Reshape::Now,
        "five more, and still nothing waits"
    );
    assert_eq!(p.counts().abandoned, 6);
}
