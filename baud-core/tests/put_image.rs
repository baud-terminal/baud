//! An image an embedder decodes, stored and placed at the cursor
//! (`Term::put_image`), and the alpha mark each image carries: what a Sixel
//! is stored as, and what a put image keeps. V19 step 1: the store holds
//! straight alpha and no protocol reaches it yet.

use baud_core::*;
use baud_domain::{ImageAlpha, ImageBudget, ImagePx, ImageRefusals, ImageScreen, LineIdx, Pixel};

mod support;

const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Six pixels at six alphas, clear and opaque among them.
fn six() -> [Pixel; 6] {
    let c = Chan::from_u8;
    [0u8, 1, 127, 128, 254, 255].map(|a| Pixel::straight(c(a), c(a ^ 0x5A), c(40), c(200)))
}

#[test]
fn a_put_image_is_placed_at_the_cursor_and_keeps_its_mark() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(64));
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, b"\x1b[5;7H");
    let before = t.cursor();
    let placed = t.put_image(ImagePx::of(3), ImagePx::of(2), ImageAlpha::Partial, |px| {
        px.copy_from_slice(&six())
    });
    assert!(!placed.is_none(), "placed");
    assert_eq!(t.cursor(), before, "the cursor stays where it was");
    let imgs = t.images();
    let (_, p) = imgs.placements().next().expect("one placement");
    assert_eq!(p.screen, ImageScreen::Primary);
    assert_eq!(
        p.line,
        LineIdx::FIRST.plus(Rows::of_count(4)),
        "the cursor's line"
    );
    assert_eq!(p.col.index(), 6, "the cursor's column");
    let v = imgs.image(p.image).expect("live");
    assert_eq!((v.width.raw(), v.height.raw()), (3, 2));
    assert_eq!(v.pixels, &six(), "the pixels as painted, straight");
    assert_eq!(v.alpha, ImageAlpha::Partial, "the mark it was stored with");

    // On the alternate screen it is the alternate screen's, and an
    // opaque-or-clear mark is kept as well.
    feed(&mut t, b"\x1b[?1049h\x1b[2;2H");
    let placed = t.put_image(
        ImagePx::of(1),
        ImagePx::of(1),
        ImageAlpha::OpaqueOrClear,
        |px| px[0] = Pixel::CLEAR,
    );
    assert!(!placed.is_none(), "placed on the alternate screen");
    let imgs = t.images();
    let alt: Vec<_> = imgs
        .placements()
        .filter(|(_, p)| p.screen == ImageScreen::Alternate)
        .map(|(_, p)| *p)
        .collect();
    assert_eq!(alt.len(), 1, "one on the alternate screen");
    assert_eq!(
        imgs.image(alt[0].image).unwrap().alpha,
        ImageAlpha::OpaqueOrClear
    );
}

/// **A Sixel is stored opaque-or-clear**: its pixels are each a register's
/// colour, opaque, or clear where no bit was set, and the mark says so, so
/// the CPU path keeps its one blit of a still Sixel.
#[test]
fn a_sixel_is_stored_opaque_or_clear() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, SWIRL);
    let imgs = t.images();
    let (_, p) = imgs.placements().next().expect("the swirl placed");
    let v = imgs.image(p.image).unwrap();
    assert_eq!(v.alpha, ImageAlpha::OpaqueOrClear);
    let opaque = v.pixels.iter().filter(|p| p.alpha().get() == 0xFF).count();
    let clear = v.pixels.iter().filter(|p| **p == Pixel::CLEAR).count();
    assert!(opaque > v.pixels.len() / 2, "the control: {opaque} drawn");
    assert_eq!(
        opaque + clear,
        v.pixels.len(),
        "every pixel opaque or clear"
    );
    assert_eq!(ImageAlpha::of(v.pixels), ImageAlpha::OpaqueOrClear);
}

/// **An image put while a Sixel's body is split across reads is refused
/// for the arena**, and the Sixel then lands as it does fed whole: the
/// Sixel paints a gap its `q` claimed and the slot table does not hold, so
/// an image stored there would be painted over. Split inside the body and
/// between the terminator's `ESC` and `\`; after the terminator a put
/// stores.
#[test]
fn a_put_image_inside_a_split_sixel_is_refused_for_the_arena() {
    let ws = support::CORPUS;
    let whole = {
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(64 * 48 + 64));
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, SWIRL);
        let imgs = t.images();
        let (_, p) = imgs.placements().next().unwrap();
        imgs.image(p.image).unwrap().pixels.to_vec()
    };
    for cut in [SWIRL.len() / 2, SWIRL.len() - 1] {
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(64 * 48 + 64));
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, &SWIRL[..cut]);
        let mut called = false;
        let placed = t.put_image(ImagePx::of(2), ImagePx::of(3), ImageAlpha::Partial, |_| {
            called = true
        });
        assert!(placed.is_none(), "cut {cut}: refused");
        assert!(!called, "cut {cut}: nothing painted");
        let r = t.images().refusals;
        assert_eq!(
            (r.arena, r.arena_width.raw(), r.arena_height.raw()),
            (1, 2, 3),
            "cut {cut}: counted against the arena with its size"
        );
        assert_eq!(t.images().images_refused, 1);
        feed(&mut t, &SWIRL[cut..]);
        let imgs = t.images();
        let ids: Vec<_> = imgs.placements().map(|(_, p)| p.image).collect();
        assert_eq!(ids.len(), 1, "cut {cut}: the Sixel placed");
        assert_eq!(
            imgs.image(ids[0]).unwrap().pixels,
            &whole[..],
            "cut {cut}: the Sixel as fed whole"
        );
        let placed = t.put_image(ImagePx::of(2), ImagePx::of(3), ImageAlpha::Partial, |px| {
            px.fill(Pixel::CLEAR)
        });
        assert!(
            !placed.is_none(),
            "cut {cut}: after the terminator it stores"
        );
    }
}

#[test]
fn an_unprovisioned_term_refuses_a_put_image() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let mut called = false;
    let placed = t.put_image(ImagePx::of(4), ImagePx::of(4), ImageAlpha::Partial, |_| {
        called = true
    });
    assert!(placed.is_none());
    assert!(!called);
    let mut want = ImageRefusals::NONE;
    want.arena = 1;
    want.arena_width = ImagePx::of(4);
    want.arena_height = ImagePx::of(4);
    assert_eq!(t.images().refusals, want);
}

/// **At every cut inside a Sixel's body a put image is refused, and before
/// its `q` it stores**: the body is the only place a Sixel holds a claim.
/// Between reads the parser is in the body or at an `ESC` inside it -- a
/// string body is put a byte at a time and waits on no character -- so a
/// short Sixel with a raster, two colours defined, a repeat, `$` and `-`
/// is cut after every byte from its `q` to its `ESC`; each time the Sixel
/// then lands as it does fed whole. Cut in its prefix, before the claim,
/// the put image stores and the Sixel lands beside it unchanged.
#[test]
fn a_put_image_at_every_cut_of_a_sixel_body_is_refused() {
    const SIXEL: &[u8] =
        b"\x1bP0;1;0q\"1;1;12;12#1;2;0;100;0#2;2;0;0;100#1!12~$#2!6N-#1!3~~~\x1b\\";
    let ws = support::CORPUS;
    let fed = |cut: usize| {
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(256));
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, &SIXEL[..cut]);
        let put = t.put_image(ImagePx::of(2), ImagePx::of(2), ImageAlpha::Partial, |px| {
            px.fill(Pixel::straight(
                Chan::from_u8(9),
                Chan::from_u8(9),
                Chan::from_u8(9),
                Chan::from_u8(9),
            ))
        });
        feed(&mut t, &SIXEL[cut..]);
        let imgs = t.images();
        let sixel: Vec<Pixel> = imgs
            .placements()
            .map(|(_, p)| imgs.image(p.image).unwrap())
            .find(|v| v.alpha == ImageAlpha::OpaqueOrClear)
            .expect("the Sixel placed")
            .pixels
            .to_vec();
        (put.is_none(), imgs.refusals.arena, sixel)
    };
    let (_, _, whole) = fed(SIXEL.len());
    assert!(
        whole.iter().any(|p| *p != Pixel::CLEAR),
        "the control: it drew"
    );
    let q = SIXEL.iter().position(|&b| b == b'q').unwrap();
    for cut in 1..SIXEL.len() {
        let (refused, arena, sixel) = fed(cut);
        let in_body = cut > q;
        assert_eq!(refused, in_body, "cut {cut}: refused only inside the body");
        assert_eq!(
            arena,
            usize::from(in_body),
            "cut {cut}: counted for the arena"
        );
        assert_eq!(sixel, whole, "cut {cut}: the Sixel as fed whole");
    }
}
