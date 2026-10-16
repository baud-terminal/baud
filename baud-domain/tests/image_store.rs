//! The image-store gate: a caller-provided ARGB arena, an image-slot table and
//! a placement table, all refuse-and-count, with reclaim inherent in the gap
//! between live spans. The mechanics are checked with synthetic small images
//! written distinct sentinels -- so an overlap corrupts one and the test sees it
//! -- and the last test decodes a **real** Sixel from ImageMagick into the store
//! in place, the zero-extra-copy path `docs/research/notes/t-images.md`
//! describes.

use baud_domain::{
    BandMove, Col, ImageAlpha, ImagePx, ImageScreen, ImageSlot, ImageStore, LineIdx, Pixel,
    PlacementSlot, Rows, Sixel, SixelAspect, SixelCanvas, SixelPx, ZIndex,
};

fn red() -> Pixel {
    Pixel::opaque(chan(255), chan(0), chan(0))
}
fn blue() -> Pixel {
    Pixel::opaque(chan(0), chan(0), chan(255))
}
fn chan(v: u8) -> baud_domain::Chan {
    baud_domain::Chan::from_u8(v)
}

/// A store over freshly-zeroed caller buffers of the given sizes.
struct Buffers {
    pixels: [Pixel; 64],
    slots: [ImageSlot; 4],
    places: [PlacementSlot; 4],
}
impl Buffers {
    fn new() -> Buffers {
        Buffers {
            pixels: [Pixel::CLEAR; 64],
            slots: [ImageSlot::EMPTY; 4],
            places: [PlacementSlot::EMPTY; 4],
        }
    }
    fn store(&mut self) -> ImageStore<'_> {
        ImageStore::new(&mut self.pixels, &mut self.slots, &mut self.places)
    }
}

/// Fill an image's returned buffer with one colour.
fn paint(buf: &mut [Pixel], c: Pixel) {
    for p in buf.iter_mut() {
        *p = c;
    }
}

#[test]
fn two_images_do_not_overlap_in_the_arena() {
    let mut b = Buffers::new();
    let mut store = b.store();

    let (a, buf_a) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    paint(buf_a, red());
    let (c, buf_c) = store
        .store(ImagePx::of(3), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
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

    // 9x9 = 81 > 64: refused whole, never truncated -- and counted against
    // the arena, with the size that did not fit.
    assert!(
        store
            .store(ImagePx::of(9), ImagePx::of(9), ImageAlpha::OpaqueOrClear)
            .is_none()
    );
    assert_eq!(store.images_refused, 1);
    assert_eq!(
        (store.refusals.arena, store.refusals.arena_width),
        (1, ImagePx::of(9))
    );
    // A zero dimension is refused too, as an image with no pixels rather than
    // one over a limit.
    assert!(
        store
            .store(ImagePx::of(0), ImagePx::of(4), ImageAlpha::OpaqueOrClear)
            .is_none()
    );
    assert_eq!(store.images_refused, 2);
    assert_eq!((store.refusals.empty, store.refusals.arena), (1, 1));
    assert_eq!(store.images_refused, store.refusals.total());
}

#[test]
fn a_full_slot_table_refuses_and_counts() {
    let mut b = Buffers::new(); // four slots
    let mut store = b.store();
    for _ in 0..4 {
        assert!(
            store
                .store(ImagePx::of(1), ImagePx::of(1), ImageAlpha::OpaqueOrClear)
                .is_some()
        );
    }
    assert!(
        store
            .store(ImagePx::of(1), ImagePx::of(1), ImageAlpha::OpaqueOrClear)
            .is_none(),
        "the fifth image has no slot"
    );
    assert_eq!(store.images_refused, 1);
    assert_eq!(
        (store.refusals.slot, store.refusals.arena),
        (1, 0),
        "counted against the slots: the arena had room"
    );
}

#[test]
fn a_placement_holds_its_image_line_column_and_z() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();

    let pid = store.place(
        id,
        ImageScreen::Primary,
        LineIdx::FIRST.next(),
        Col::ZERO,
        ZIndex::of(-3),
    );
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
    let (id, _) = store
        .store(ImagePx::of(1), ImagePx::of(1), ImageAlpha::OpaqueOrClear)
        .unwrap();
    for _ in 0..4 {
        assert!(
            !store
                .place(
                    id,
                    ImageScreen::Primary,
                    LineIdx::FIRST,
                    Col::ZERO,
                    ZIndex::ZERO
                )
                .is_none()
        );
    }
    assert!(
        store
            .place(
                id,
                ImageScreen::Primary,
                LineIdx::FIRST,
                Col::ZERO,
                ZIndex::ZERO
            )
            .is_none(),
        "the fifth placement has no slot"
    );
    assert_eq!(store.placements_refused, 1);
}

#[test]
fn the_last_placement_leaving_frees_the_image_and_reclaims_its_span() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    let p1 = store.place(
        id,
        ImageScreen::Primary,
        LineIdx::FIRST,
        Col::ZERO,
        ZIndex::ZERO,
    );
    let p2 = store.place(
        id,
        ImageScreen::Primary,
        LineIdx::FIRST,
        Col::ZERO,
        ZIndex::ZERO,
    );

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
    let mut pixels = [Pixel::CLEAR; 12];
    let mut slots = [ImageSlot::EMPTY; 4];
    let mut places = [PlacementSlot::EMPTY; 4];
    let mut store = ImageStore::new(&mut pixels, &mut slots, &mut places);

    let (_, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    let (mid, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    let (_, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    // Arena is full: a fourth image is refused.
    assert!(
        store
            .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
            .is_none()
    );

    store.free(mid);
    let (reused, buf) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
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
    let (id, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    store.place(
        id,
        ImageScreen::Primary,
        LineIdx::FIRST,
        Col::ZERO,
        ZIndex::ZERO,
    );
    store.place(
        id,
        ImageScreen::Primary,
        LineIdx::FIRST,
        Col::ZERO,
        ZIndex::ZERO,
    );

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
    let (id, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    store.place(
        id,
        ImageScreen::Primary,
        LineIdx::FIRST,
        Col::ZERO,
        ZIndex::ZERO,
    );

    store.reset();
    assert!(store.image(id).is_none());
    assert_eq!(store.placements().count(), 0);
    // The arena is free again: a fresh store succeeds at the origin's space.
    assert!(
        store
            .store(ImagePx::of(4), ImagePx::of(4), ImageAlpha::OpaqueOrClear)
            .is_some()
    );
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
    let mut pixels = [Pixel::CLEAR; 64 * 48 + 16];
    let mut slots = [ImageSlot::EMPTY; 2];
    let mut places = [PlacementSlot::EMPTY; 2];
    let mut store = ImageStore::new(&mut pixels, &mut slots, &mut places);

    let (id, buf) = store
        .store(ImagePx::of(64), ImagePx::of(48), ImageAlpha::OpaqueOrClear)
        .expect("the arena holds a 64x48 image");
    let report = {
        let mut canvas = SixelCanvas::new(buf, SixelPx::of(64));
        // The capture's envelope is `ESC P 0;1;0 q`, `P1 = 0`, 2:1, and its
        // raster command `"1;1` overrides it.
        Sixel::decode(inner, SixelAspect::from_macro(Some(0)), &mut canvas)
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
    let painted = view.pixels.iter().filter(|p| **p != Pixel::CLEAR).count();
    assert!(
        painted > view.pixels.len() / 2,
        "a swirl gradient paints most of its {} pixels, not {painted}",
        view.pixels.len()
    );
    assert!(!id.is_none(), "the store handed back a live image id");
}

#[test]
fn a_reflow_maps_each_placement_once_and_drops_the_ones_no_line_took() {
    // The old and new layouts share one numbering, so a placement mapped into
    // a later line's old range must not be mapped again when that line is
    // visited. Three placements at rows 5, 10 and 20 from the floor: the first
    // line (rows 0..6) moves down five, landing the first on row 10; the next
    // line visited (row 10 alone) moves down a hundred, which is the second's
    // and not the first's; no line takes row 20, so that one is dropped.
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store
        .store(ImagePx::of(1), ImagePx::of(1), ImageAlpha::OpaqueOrClear)
        .unwrap();
    let floor = LineIdx::of(1000);
    for r in [5u64, 10, 20] {
        let pid = store.place(
            id,
            ImageScreen::Primary,
            LineIdx::of(1000 + r),
            Col::ZERO,
            ZIndex::ZERO,
        );
        assert!(!pid.is_none());
    }
    // One on the other screen, which the reflow of the primary must not touch.
    store.place(
        id,
        ImageScreen::Alternate,
        LineIdx::of(3),
        Col::ZERO,
        ZIndex::ZERO,
    );
    assert_eq!(store.follow_begin(ImageScreen::Primary, floor), 3);
    store.follow_line(Rows::of_count(0), Rows::of_count(6), &|r: Rows, x: Col| {
        (Rows::of_count(r.index() + 5), x)
    });
    store.follow_line(Rows::of_count(10), Rows::of_count(1), &|r: Rows, x: Col| {
        (Rows::of_count(r.index() + 100), x)
    });
    assert_eq!(store.follow_end(floor), 1, "the one no line took");
    let mut lines: Vec<(ImageScreen, u64)> = store
        .placements()
        .map(|(_, p)| (p.screen, p.line.value()))
        .collect();
    lines.sort_by_key(|&(_, l)| l);
    assert_eq!(
        lines,
        vec![
            (ImageScreen::Alternate, 3),
            (ImageScreen::Primary, 1010),
            (ImageScreen::Primary, 1110)
        ]
    );
    assert_eq!(store.placements_expired, 1);
}

/// **A placement asks the next reflow by the cell the last one answered.**
/// The first follow lands its row's first cell part-way along a row, column
/// 20; the second is asked by column 20 of that row, not by the row's own
/// first cell, which is what lets a narrow-then-widen round trip come back
/// to the row the image was placed on. A fresh placement asks by column 0.
#[test]
fn a_reflow_asks_by_the_column_the_last_one_answered() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store
        .store(ImagePx::of(1), ImagePx::of(1), ImageAlpha::OpaqueOrClear)
        .unwrap();
    let floor = LineIdx::of(0);
    store.place(
        id,
        ImageScreen::Primary,
        LineIdx::of(1),
        Col::at_index(7),
        ZIndex::ZERO,
    );
    let asked = std::cell::Cell::new(None);
    assert_eq!(store.follow_begin(ImageScreen::Primary, floor), 1);
    store.follow_line(Rows::of_count(0), Rows::of_count(2), &|r: Rows, x: Col| {
        asked.set(Some(x));
        (Rows::of_count(r.index() + 2), Col::at_index(20))
    });
    store.follow_end(floor);
    assert_eq!(asked.get(), Some(Col::ZERO), "placed: its row's first cell");
    assert_eq!(store.follow_begin(ImageScreen::Primary, floor), 1);
    store.follow_line(Rows::of_count(0), Rows::of_count(4), &|r: Rows, x: Col| {
        asked.set(Some(x));
        (Rows::of_count(r.index() - 2), Col::ZERO)
    });
    store.follow_end(floor);
    assert_eq!(asked.get(), Some(Col::at_index(20)), "then where it landed");
    let p: Vec<_> = store
        .placements()
        .map(|(_, p)| (p.line.value(), p.col))
        .collect();
    assert_eq!(
        p,
        vec![(1, Col::at_index(7))],
        "the line back, the column kept"
    );
}

#[test]
fn expiry_keeps_the_retained_span_and_drops_a_blanked_row() {
    let mut pixels = [Pixel::CLEAR; 4];
    let mut slots = [ImageSlot::EMPTY; 1];
    let mut places = [PlacementSlot::EMPTY; 8];
    let mut store = ImageStore::new(&mut pixels, &mut slots, &mut places);
    let (id, _) = store
        .store(ImagePx::of(1), ImagePx::of(1), ImageAlpha::OpaqueOrClear)
        .unwrap();
    for l in [9u64, 10, 15, 19, 20] {
        store.place(
            id,
            ImageScreen::Primary,
            LineIdx::of(l),
            Col::ZERO,
            ZIndex::ZERO,
        );
    }
    store.place(
        id,
        ImageScreen::Alternate,
        LineIdx::of(0),
        Col::ZERO,
        ZIndex::ZERO,
    );
    assert_eq!(store.placements_refused, 0, "all six placed");
    // Retain [10, 20), and 15 was blanked: 9, 15 and 20 go.
    let n = store.expire(
        ImageScreen::Primary,
        LineIdx::of(10),
        LineIdx::of(20),
        Some(LineIdx::of(15)),
    );
    assert_eq!(n, 3);
    let mut kept: Vec<u64> = store
        .placements()
        .filter(|(_, p)| p.screen == ImageScreen::Primary)
        .map(|(_, p)| p.line.value())
        .collect();
    kept.sort_unstable();
    assert_eq!(kept, vec![10, 19]);
    assert_eq!(store.expire_screen(ImageScreen::Alternate), 1);
    assert_eq!(store.placements_expired, 4);
}

#[test]
fn a_cancelled_follow_leaves_every_placement_where_it_was() {
    // A reflow that returns before re-cutting visits no line, so nothing is
    // mapped; ending the follow would read that as every line dropped.
    let mut b = Buffers::new();
    let mut store = b.store();
    let (id, _) = store
        .store(ImagePx::of(1), ImagePx::of(1), ImageAlpha::OpaqueOrClear)
        .unwrap();
    store.place(
        id,
        ImageScreen::Primary,
        LineIdx::of(7),
        Col::ZERO,
        ZIndex::ZERO,
    );
    assert_eq!(store.follow_begin(ImageScreen::Primary, LineIdx::of(0)), 1);
    store.follow_cancel();
    assert_eq!(
        store.follow_end(LineIdx::of(0)),
        0,
        "nothing staged is left"
    );
    let lines: Vec<u64> = store.placements().map(|(_, p)| p.line.value()).collect();
    assert_eq!(lines, vec![7]);
    assert_eq!(store.placements_expired, 0);
}

#[test]
fn a_band_moves_its_placements_and_drops_the_ones_that_leave_it() {
    // The band is lines [5, 10). 4 and 10 are its neighbours on either side
    // and must not move; the alternate screen's 7 is on the other screen.
    let mut pixels = [Pixel::CLEAR; 4];
    let mut slots = [ImageSlot::EMPTY; 1];
    let mut places = [PlacementSlot::EMPTY; 8];
    let mut store = ImageStore::new(&mut pixels, &mut slots, &mut places);
    let (id, _) = store
        .store(ImagePx::of(1), ImagePx::of(1), ImageAlpha::OpaqueOrClear)
        .unwrap();
    for l in [4u64, 5, 7, 9, 10] {
        store.place(
            id,
            ImageScreen::Primary,
            LineIdx::of(l),
            Col::ZERO,
            ZIndex::ZERO,
        );
    }
    store.place(
        id,
        ImageScreen::Alternate,
        LineIdx::of(7),
        Col::ZERO,
        ZIndex::ZERO,
    );
    let lines = |s: &ImageStore<'_>, screen| {
        let mut v: Vec<u64> = s
            .placements()
            .filter(|(_, p)| p.screen == screen)
            .map(|(_, p)| p.line.value())
            .collect();
        v.sort_unstable();
        v
    };
    // Up two: 5 leaves the band's top and goes, 7 and 9 move to 5 and 7.
    let gone = store.scroll_band(
        ImageScreen::Primary,
        LineIdx::of(5),
        LineIdx::of(10),
        Rows::of_count(2),
        BandMove::Up,
    );
    assert_eq!(gone, 1);
    assert_eq!(lines(&store, ImageScreen::Primary), vec![4, 5, 7, 10]);
    // Down three: 7 lands on 10, past the band's last line, and goes.
    let gone = store.scroll_band(
        ImageScreen::Primary,
        LineIdx::of(5),
        LineIdx::of(10),
        Rows::of_count(3),
        BandMove::Down,
    );
    assert_eq!(gone, 1);
    assert_eq!(lines(&store, ImageScreen::Primary), vec![4, 8, 10]);
    assert_eq!(
        lines(&store, ImageScreen::Alternate),
        vec![7],
        "the other screen"
    );
    assert_eq!(store.placements_expired, 2);
    // A band that misses every placement moves nothing.
    assert_eq!(
        store.scroll_band(
            ImageScreen::Primary,
            LineIdx::of(11),
            LineIdx::of(20),
            Rows::of_count(1),
            BandMove::Up,
        ),
        0
    );
    assert_eq!(lines(&store, ImageScreen::Primary), vec![4, 8, 10]);
    // The reach has to follow a move out past its old last line, or the
    // next band over the new line early-outs on the stale one. 10 goes to
    // 15, past the reach of [4, 10] the placements were made with, and a
    // band covering only 15 must still find it. Without the rescan after
    // the first move this is the one assertion that fails: every earlier
    // one reads the same off a stale reach and a fresh one.
    store.scroll_band(
        ImageScreen::Primary,
        LineIdx::of(10),
        LineIdx::of(20),
        Rows::of_count(5),
        BandMove::Down,
    );
    store.scroll_band(
        ImageScreen::Primary,
        LineIdx::of(15),
        LineIdx::of(30),
        Rows::of_count(2),
        BandMove::Down,
    );
    assert_eq!(lines(&store, ImageScreen::Primary), vec![4, 8, 17]);
}

/// **A pixel is straight ARGB in one word**, `0xAARRGGBB`: the colour as
/// given, not multiplied by its alpha (`[[source: w3c-png-2003]]` 6.2), so
/// a half-transparent white keeps its white.
#[test]
fn a_pixel_is_straight_argb_in_one_word() {
    let p = Pixel::straight(chan(0x80), chan(0xFF), chan(0x10), chan(0x01));
    assert_eq!(p.bits(), 0x80FF_1001);
    assert_eq!(p.alpha(), chan(0x80));
    assert_eq!(
        p.rgb(),
        (chan(0xFF), chan(0x10), chan(0x01)),
        "not premultiplied"
    );
    assert_eq!(Pixel::opaque(chan(1), chan(2), chan(3)).bits(), 0xFF01_0203);
    assert_eq!(Pixel::CLEAR.bits(), 0, "a zeroed arena is clear");
    assert_eq!(Pixel::default(), Pixel::CLEAR);
    assert_eq!(
        core::mem::size_of::<Pixel>(),
        4,
        "one word, as the arena budgets"
    );
}

/// **What `ImageAlpha::of` calls partial is any alpha between clear and
/// opaque**, the ends excluded, and nothing else: the colour of a clear
/// pixel does not make it partial.
#[test]
fn image_alpha_of_is_partial_for_any_alpha_between_the_ends() {
    let px = |a: u8| Pixel::straight(chan(a), chan(9), chan(9), chan(9));
    assert_eq!(ImageAlpha::of(&[]), ImageAlpha::OpaqueOrClear);
    assert_eq!(
        ImageAlpha::of(&[px(0), px(255), Pixel::CLEAR]),
        ImageAlpha::OpaqueOrClear
    );
    for a in 1..=254 {
        assert_eq!(
            ImageAlpha::of(&[px(255), px(a), px(0)]),
            ImageAlpha::Partial,
            "alpha {a}"
        );
    }
}

/// **An image keeps the mark it was stored with**, and a slot freed and
/// stored again takes the new image's mark, not the old one's.
#[test]
fn an_image_keeps_the_alpha_mark_it_was_stored_with() {
    let mut b = Buffers::new();
    let mut store = b.store();
    let (p, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::Partial)
        .unwrap();
    let (o, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    assert_eq!(store.image(p).unwrap().alpha, ImageAlpha::Partial);
    assert_eq!(store.image(o).unwrap().alpha, ImageAlpha::OpaqueOrClear);
    store.free(p);
    let (q, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::OpaqueOrClear)
        .unwrap();
    assert_eq!(q, p, "the freed slot, reused");
    assert_eq!(store.image(q).unwrap().alpha, ImageAlpha::OpaqueOrClear);
    store.free(o);
    let (r, _) = store
        .store(ImagePx::of(2), ImagePx::of(2), ImageAlpha::Partial)
        .unwrap();
    assert_eq!(r, o);
    assert_eq!(store.image(r).unwrap().alpha, ImageAlpha::Partial);
}

/// **The kitty quota deletes only whole kitty images with no placement**,
/// oldest first ("delete older images to make space for the new one",
/// `[[source: kitty-graphics]]`): never one still decoding and never a
/// Sixel's or an embedder's, and when deleting every candidate would not
/// make space it deletes none and refuses once.
#[test]
fn the_kitty_quota_deletes_only_whole_unplaced_kitty_images() {
    use baud_domain::{ImageOwner, KittyId, KittyNumber};
    let kitty = |i| ImageOwner::Kitty(KittyId::of(i), KittyNumber::NONE);
    let px = ImagePx::of;
    let mut b = Buffers::new();
    let mut store = b.store();
    // Half the arena still decoding, half a local image: nothing may go.
    let decoding = store.reserve(px(4), px(8), kitty(1)).unwrap();
    let (local, _) = store
        .store(px(4), px(8), ImageAlpha::OpaqueOrClear)
        .unwrap();
    assert_eq!(store.reserve_evicting(px(4), px(4), kitty(3)), None);
    assert_eq!((store.images_evicted, store.images_refused), (0, 1));
    assert!(store.size_of(decoding).is_some() && store.image(local).is_some());
    // Whole, it is the one candidate, and it goes; the local image stays.
    store.complete(decoding, ImageAlpha::OpaqueOrClear);
    assert!(store.reserve_evicting(px(4), px(4), kitty(3)).is_some());
    assert_eq!(store.images_evicted, 1);
    assert!(
        store.image(local).is_some(),
        "a local image is never deleted"
    );
    assert_eq!(store.kitty_image(KittyId::of(1)), None);
}

/// No free slot is a quota too: four whole unplaced kitty images hold the
/// four slots, and a fifth deletes the oldest.
#[test]
fn a_full_slot_table_deletes_the_oldest_unplaced_kitty_image() {
    use baud_domain::{ImageOwner, KittyId, KittyNumber};
    let px = ImagePx::of;
    let mut b = Buffers::new();
    let mut store = b.store();
    for i in 1..=4 {
        let owner = ImageOwner::Kitty(KittyId::of(i), KittyNumber::NONE);
        let id = store.reserve(px(1), px(1), owner).unwrap();
        store.complete(id, ImageAlpha::OpaqueOrClear);
    }
    let owner = ImageOwner::Kitty(KittyId::of(5), KittyNumber::NONE);
    assert!(store.reserve_evicting(px(1), px(1), owner).is_some());
    assert_eq!((store.images_evicted, store.images_refused), (1, 0));
    let live: Vec<bool> = (1..=5)
        .map(|i| store.kitty_image(KittyId::of(i)).is_some())
        .collect();
    assert_eq!(live, [false, true, true, true, true], "the oldest went");
}
