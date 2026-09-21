//! The CPU compositor draws a Sixel placement, and it moves with the scroll.
//!
//! A dedicated test rather than a row in `golden.rs`, deliberately: that
//! harness provisions no image memory (`Owned::new`), so a Sixel there would
//! pin the digest of a *refused* image -- a gate that passes while proving the
//! opposite of what it claims. This one provisions the store and asserts the
//! pixels drew before trusting any digest.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ImageBudget, ReadLen, Rows};
use baud_font::Face;
use baud_render::{Damage, Theme};
use support::{Canvas, WS, font, renderer};

/// A 1x6 red Sixel, the smallest well-formed image, for the second-image case.
const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";

const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// How many pixels in the 64x48 rect at the top-left differ from the theme
/// background -- the image's own pixels, drawn over the cells.
fn painted_in_image_rect(c: &Canvas) -> usize {
    let bg = Theme::dark().bg.0;
    let mut n = 0;
    for y in 0..48 {
        for x in 0..64 {
            if c.pixels[y * c.width + x] != bg {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn a_sixel_is_drawn_over_the_grid() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);

    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64, 8192));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }

    // The 64x48 swirl anchors at the cursor (row 0, col 0) and fills most of
    // its rect -- assert the pixels *before* any digest, so "the image drew"
    // is distinguished from "the frame is uniformly background".
    let painted = painted_in_image_rect(&c);
    assert!(
        painted > 64 * 48 / 2,
        "a gradient swirl paints most of its 3072 pixels, not {painted}"
    );
}

#[test]
fn a_sixel_scrolls_off_and_the_frame_changes() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);

    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64, 8192));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let before = painted_in_image_rect(&c);
    let digest_before = c.digest();
    assert!(before > 64 * 48 / 2, "the image drew first, {before}");

    // Scroll the screen well past the image: move the cursor to the bottom and
    // scroll far enough that the image (anchored at line 0) is above the top.
    feed(&mut t, &[b'\n'; 200]);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let after = painted_in_image_rect(&c);
    let digest_after = c.digest();

    // The image scrolled off, so its rect is background again -- and the frame
    // changed, which a blit that silently drew nothing could not produce.
    assert_eq!(after, 0, "the image scrolled off its rect, {after} left");
    assert_ne!(
        digest_before, digest_after,
        "the frame changed with the scroll"
    );
}

#[test]
fn a_static_image_is_snapshotted_once_not_per_frame() {
    // `set_images` runs every frame; a large image's per-frame pixel snapshot
    // (a `Vec` allocation plus a per-pixel conversion) is real cost while it is
    // on screen. The snapshot cache is kept across frames and rebuilt only for
    // a new `(id, epoch)`, so an unchanged image on screen is snapshotted once
    // however many frames it draws -- which a clear-every-frame renderer is not.
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 128, 8192));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);

    for _ in 0..5 {
        r.set_images(&t, Rows::ZERO);
    }
    assert_eq!(
        r.image_snapshots().value(),
        1,
        "the static image was snapshotted once over five frames, not five times"
    );

    // A new image bumps the store epoch, which drops the snapshot cache, so the
    // next frame re-snapshots what is referenced -- the count rises.
    feed(&mut t, TINY);
    r.set_images(&t, Rows::ZERO);
    assert!(
        r.image_snapshots().value() > 1,
        "a store's epoch change re-snapshots, not {}",
        r.image_snapshots().value()
    );
    assert!(
        !r.image_uploads().is_empty(),
        "the images are cached on screen"
    );

    // Scroll both images off the top. A build counter cannot see a leak, so the
    // cache length is the observable: an off-screen image's snapshot is dropped
    // rather than held until the next epoch change, which on a terminal showing
    // one image is forever.
    feed(&mut t, &[b'\n'; 60]);
    r.set_images(&t, Rows::ZERO);
    assert!(
        r.image_placements().is_empty(),
        "the images scrolled off screen"
    );
    assert!(
        r.image_uploads().is_empty(),
        "and their snapshots were dropped, not retained"
    );
}
