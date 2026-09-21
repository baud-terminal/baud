//! The image-store gate: a caller-provided ARGB arena, an image-slot table and
//! a placement table, all refuse-and-count, with reclaim inherent in the gap
//! between live spans. The mechanics are checked with synthetic small images
//! written distinct sentinels -- so an overlap corrupts one and the test sees it
//! -- and the last test decodes a **real** Sixel from ImageMagick into the store
//! in place, the zero-extra-copy path `docs/research/notes/t-images.md`
//! describes.

use baud_domain::{
    Col, ColorWord, ImagePx, ImageSlot, ImageStore, LineIdx, PlacementSlot, Sixel, SixelCanvas,
    SixelPx, ZIndex,
};

fn red() -> ColorWord {
    ColorWord::rgb(chan(255), chan(0), chan(0))
}
fn blue() -> ColorWord {
    ColorWord::rgb(chan(0), chan(0), chan(255))
}
fn chan(v: u8) -> baud_domain::Chan {
    baud_domain::Chan::from_u8(v)
}

/// A store over freshly-zeroed caller buffers of the given sizes.
struct Buffers {
    pixels: [ColorWord; 64],
    slots: [ImageSlot; 4],
    places: [PlacementSlot; 4],
}
impl Buffers {
    fn new() -> Buffers {
        Buffers {
            pixels: [ColorWord::DEFAULT; 64],
            slots: [ImageSlot::EMPTY; 4],
            places: [PlacementSlot::EMPTY; 4],
        }
    }
    fn store(&mut self) -> ImageStore<'_> {
        ImageStore::new(&mut self.pixels, &mut self.slots, &mut self.places)
    }
}

/// Fill an image's returned buffer with one colour.
fn paint(buf: &mut [ColorWord], c: ColorWord) {
    for p in buf.iter_mut() {
        *p = c;
    }
}

#[test]
fn two_images_do_not_overlap_in_the_arena() {
    let mut b = Buffers::new();
    let mut store = b.store();

    let (a, buf_a) = store.store(ImagePx::of(2), ImagePx::of(2)).unwrap();
    paint(buf_a, red());
    let (c, buf_c) = store.store(ImagePx::of(3), ImagePx::of(2)).unwrap();
    paint(buf_c, blue());

    // If the second store had overlapped the first, painting it blue would have
    // corrupted the red image. It did not.
    assert!(
        store.image(a).unwrap().pixels.iter().all(|p| *p == red()),
        "the first image is still entirely red"
    );
    assert!(
        store.image(c).unwrap().pixels.iter().all(|p| *p == blue()),
        "the second image is entirely blue"
    );
    assert_eq!(
        store.image(a).unwrap().pixels.len(),
        4,
        "2x2 is four pixels"
    );
    assert_eq!(store.image(c).unwrap().pixels.len(), 6, "3x2 is six pixels");
    assert_eq!(store.images_refused, 0);
}

#[test]
fn an_image_too_large_for_the_arena_is_refused_and_counted() {
    let mut b = Buffers::new(); // 64-pixel arena
    let mut store = b.store();

    // 9x9 = 81 > 64: refused whole, never truncated.
    assert!(store.store(ImagePx::of(9), ImagePx::of(9)).is_none());
    assert_eq!(store.images_refused, 1);
    // A zero dimension is refused too.
    assert!(store.store(ImagePx::of(0), ImagePx::of(4)).is_none());
    assert_eq!(store.images_refused, 2);
}

#[test]
fn a_full_slot_table_refuses_and_counts() {
    let mut b = Buffers::new(); // four slots
    let mut store = b.store();
    for _ in 0..4 {
        assert!(store.store(ImagePx::of(1), ImagePx::of(1)).is_some());
    }
    assert!(
        store.store(ImagePx::of(1), ImagePx::of(1)).is_none(),
        "the fifth image has no slot"
    );
    assert_eq!(store.images_refused, 1);
}

#[test]
fn a_placement_holds_its_image_line_column_and_z() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store.store(ImagePx::of(2), ImagePx::of(2)).unwrap();

    let pid = store.place(id, LineIdx::FIRST.next(), Col::ZERO, ZIndex::of(-3));
    assert!(!pid.is_none());

    let placed: Vec<PlacementSlot> = store.placements().map(|(_, p)| *p).collect();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].image, id);
    assert_eq!(placed[0].z, ZIndex::of(-3));
}

#[test]
fn a_full_placement_table_refuses_and_counts() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store.store(ImagePx::of(1), ImagePx::of(1)).unwrap();
    for _ in 0..4 {
        assert!(
            !store
                .place(id, LineIdx::FIRST, Col::ZERO, ZIndex::ZERO)
                .is_none()
        );
    }
    assert!(
        store
            .place(id, LineIdx::FIRST, Col::ZERO, ZIndex::ZERO)
            .is_none(),
        "the fifth placement has no slot"
    );
    assert_eq!(store.placements_refused, 1);
}

#[test]
fn the_last_placement_leaving_frees_the_image_and_reclaims_its_span() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store.store(ImagePx::of(2), ImagePx::of(2)).unwrap();
    let p1 = store.place(id, LineIdx::FIRST, Col::ZERO, ZIndex::ZERO);
    let p2 = store.place(id, LineIdx::FIRST, Col::ZERO, ZIndex::ZERO);

    store.unplace(p1);
    assert!(
        store.image(id).is_some(),
        "one placement remains, image lives"
    );
    assert_eq!(store.images_dropped, 0);

    store.unplace(p2);
    assert!(
        store.image(id).is_none(),
        "the last placement freed the image"
    );
    assert_eq!(store.images_dropped, 1);
}

#[test]
fn a_freed_gap_is_reused_so_a_full_arena_still_stores() {
    // A 12-pixel arena holding three 2x2 images exactly, then the middle freed
    // and a new 2x2 stored: it must land in the gap, because appending past the
    // third would exceed the arena. So a successful store proves gap reclaim.
    let mut pixels = [ColorWord::DEFAULT; 12];
    let mut slots = [ImageSlot::EMPTY; 4];
    let mut places = [PlacementSlot::EMPTY; 4];
    let mut store = ImageStore::new(&mut pixels, &mut slots, &mut places);

    let (_, _) = store.store(ImagePx::of(2), ImagePx::of(2)).unwrap();
    let (mid, _) = store.store(ImagePx::of(2), ImagePx::of(2)).unwrap();
    let (_, _) = store.store(ImagePx::of(2), ImagePx::of(2)).unwrap();
    // Arena is full: a fourth image is refused.
    assert!(store.store(ImagePx::of(2), ImagePx::of(2)).is_none());

    store.free(mid);
    let (reused, buf) = store
        .store(ImagePx::of(2), ImagePx::of(2))
        .expect("the freed gap holds the new image");
    paint(buf, red());
    assert!(
        store
            .image(reused)
            .unwrap()
            .pixels
            .iter()
            .all(|p| *p == red())
    );
    assert_eq!(store.images_dropped, 1);
}

#[test]
fn free_deletes_the_image_and_all_its_placements() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store.store(ImagePx::of(2), ImagePx::of(2)).unwrap();
    store.place(id, LineIdx::FIRST, Col::ZERO, ZIndex::ZERO);
    store.place(id, LineIdx::FIRST, Col::ZERO, ZIndex::ZERO);

    store.free(id);
    assert!(store.image(id).is_none());
    assert_eq!(
        store.placements().count(),
        0,
        "no placement survives the delete"
    );
    assert_eq!(store.images_dropped, 1);
}

#[test]
fn reset_forgets_everything() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store.store(ImagePx::of(2), ImagePx::of(2)).unwrap();
    store.place(id, LineIdx::FIRST, Col::ZERO, ZIndex::ZERO);

    store.reset();
    assert!(store.image(id).is_none());
    assert_eq!(store.placements().count(), 0);
    // The arena is free again: a fresh store succeeds at the origin's space.
    assert!(store.store(ImagePx::of(4), ImagePx::of(4)).is_some());
}

#[test]
fn a_real_sixel_decodes_into_the_store_in_place() {
    // A real 64x48 Sixel from ImageMagick (`magick -size 64x48
    // gradient:blue-yellow -swirl 180 sixel:-`), stored and decoded in place.
    // The raster attributes are `"1;1;64;48`, so the size is known before the
    // decode and the span is reserved from it.
    let raw = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/sixel/swirl.raw"
    ))
    .expect("the committed capture");
    // Strip the DCS envelope: the data is between the `q` and the `ESC \`.
    let q = raw.iter().position(|&b| b == b'q').unwrap();
    let st = raw.windows(2).position(|w| w == [0x1b, 0x5c]).unwrap();
    let inner = &raw[q + 1..st];

    // A 64x48 image is 3072 pixels; the arena holds it and a margin.
    let mut pixels = [ColorWord::DEFAULT; 64 * 48 + 16];
    let mut slots = [ImageSlot::EMPTY; 2];
    let mut places = [PlacementSlot::EMPTY; 2];
    let mut store = ImageStore::new(&mut pixels, &mut slots, &mut places);

    let (id, buf) = store
        .store(ImagePx::of(64), ImagePx::of(48))
        .expect("the arena holds a 64x48 image");
    let report = {
        let mut canvas = SixelCanvas::new(buf, SixelPx::of(64));
        Sixel::decode(inner, &mut canvas)
    };

    assert_eq!(
        report.width,
        SixelPx::of(64),
        "the data reaches the raster width"
    );
    assert_eq!(report.height, SixelPx::of(48), "eight six-pixel bands");
    assert_eq!(report.refused, 0, "every pixel fit the reserved span");
    assert_eq!(report.unapplied_registers, 0, "ImageMagick emits only RGB");

    // The image drew: a real photo-like Sixel leaves most pixels painted.
    let view = store.image(id).unwrap();
    let painted = view.pixels.iter().filter(|p| !p.is_default()).count();
    assert!(
        painted > view.pixels.len() / 2,
        "a swirl gradient paints most of its {} pixels, not {painted}",
        view.pixels.len()
    );
    assert!(!id.is_none(), "the store handed back a live image id");
}
