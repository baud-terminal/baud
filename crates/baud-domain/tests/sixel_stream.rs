//! `SixelStream`, the decoder a terminal runs as a body arrives: where in the
//! arena an image lands, and how much of it a decode writes.
//!
//! What it draws is held against the buffered decoder in `baud-core`'s
//! `tests/sixel_stream.rs`, over generated programs and at every read
//! boundary; this file holds the two things pixels cannot show -- the
//! store's layout after a free, and the words a raster's extent makes a
//! decode write.

use baud_domain::*;

/// A Sixel's body fed whole to a stream over `store`, at 1:1.
fn stream(store: &mut ImageStore, body: &[u8]) -> (Option<(ImageId, SixelPx, SixelPx)>, usize) {
    let mut s = Box::new(SixelStream::NEW);
    s.begin(SixelAspect::ONE_TO_ONE, store);
    for &b in body {
        s.put(StreamByte::from_stream(b), store);
    }
    let written = s.written();
    (s.finish(store), written)
}

fn start(store: &ImageStore, base: usize, id: ImageId) -> usize {
    let v = store.image(id).expect("live");
    (v.pixels.as_ptr().addr() - base) / core::mem::size_of::<Pixel>()
}

/// **A Sixel lands in the lowest gap that holds it, where `store` would
/// put it**, not at the start of the larger gap it claimed while its size
/// was unknown. Three 10x6 images fill the front of the arena, the middle
/// one is freed, and a 10x6 Sixel decodes into the larger gap past the
/// third and is moved into the middle one at its terminator, its pixels
/// intact.
#[test]
fn a_sixel_lands_in_the_lowest_gap_that_holds_it() {
    let mut px = vec![Pixel::CLEAR; 1000];
    let base = px.as_ptr().addr();
    let mut slots = [ImageSlot::EMPTY; 4];
    let mut places = [PlacementSlot::EMPTY; 4];
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    let ten = ImagePx::of(10);
    let six = ImagePx::of(6);
    let a = store
        .store(ten, six, ImageAlpha::OpaqueOrClear)
        .expect("a")
        .0;
    let b = store
        .store(ten, six, ImageAlpha::OpaqueOrClear)
        .expect("b")
        .0;
    let c = store
        .store(ten, six, ImageAlpha::OpaqueOrClear)
        .expect("c")
        .0;
    assert_eq!(
        [a, b, c].map(|id| start(&store, base, id)),
        [0, 60, 120],
        "the control: three spans in a row"
    );
    store.free(b);
    // Red in register 1, ten columns, every bit set.
    let (got, _) = stream(&mut store, b"#1;2;100;0;0#1!10~");
    let (id, w, h) = got.expect("drawn");
    assert_eq!((w.raw(), h.raw()), (10, 6));
    assert_eq!(
        start(&store, base, id),
        60,
        "the freed middle span, not 180"
    );
    let red = Pixel::opaque(Chan::from_u8(255), Chan::from_u8(0), Chan::from_u8(0));
    let v = store.image(id).expect("live");
    assert!(
        v.pixels.iter().all(|&p| p == red),
        "every pixel moved with it"
    );
    assert_eq!(store.refusals, ImageRefusals::NONE);
}

/// **A raster's extent lays the rows out, and a stride wider than the image
/// is given back at the next band**: a 10-wide image under `Ph` = 65535,
/// four bands, writes the first band's six rows at the stated stride and
/// nothing past them, because the second band lays the rows out again at
/// ten. Without that the four bands would write 24 rows of 65535.
#[test]
fn an_overstated_extent_writes_one_band_of_its_stride() {
    let mut px = vec![Pixel::CLEAR; 1 << 20];
    let mut slots = [ImageSlot::EMPTY; 4];
    let mut places = [PlacementSlot::EMPTY; 4];
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    let (got, written) = stream(
        &mut store,
        b"\"1;1;65535;24#1;2;100;0;0#1!10~-!10~-!10~-!10~",
    );
    let (_, w, h) = got.expect("drawn");
    assert_eq!((w.raw(), h.raw()), (10, 24), "the data's extent");
    assert_eq!(written, 6 * 65535, "one band at the stated stride");
}

/// **With no raster the stride grows by doubling and is given back too**:
/// a 600-wide image of ten bands, no extent stated and each band 600 data
/// bytes of one column, writes exactly its own 600 x 60 words. The first
/// band's stride doubles from 64 to 1024 as it widens, six rows of which are
/// fewer words than the image; the second lays them out at 600. Without
/// that the ten bands would write 60 rows of 1024.
#[test]
fn with_no_extent_a_decode_writes_the_image_and_no_more() {
    let mut px = vec![Pixel::CLEAR; 1 << 20];
    let mut slots = [ImageSlot::EMPTY; 4];
    let mut places = [PlacementSlot::EMPTY; 4];
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    let mut band = vec![b'~'; 600];
    band.push(b'-');
    let body = band.repeat(10);
    let (got, written) = stream(&mut store, &body);
    let (_, w, h) = got.expect("drawn");
    assert_eq!((w.raw(), h.raw()), (600, 60));
    assert_eq!(written, 600 * 60, "the image's words");
}

/// **The claim is the largest gap, so an image only the largest gap holds is
/// drawn**: three 10x6 images at 0, 60 and 120 of a 300-word arena, the
/// first freed, leave a 60-word gap at the front and a 120-word one from
/// 180, and a 20x6 image, 120 words, is drawn in the second though the first
/// sits lower.
#[test]
fn a_sixel_that_fits_only_the_largest_gap_is_drawn() {
    let mut px = vec![Pixel::CLEAR; 300];
    let base = px.as_ptr().addr();
    let mut slots = [ImageSlot::EMPTY; 4];
    let mut places = [PlacementSlot::EMPTY; 4];
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    let ten = ImagePx::of(10);
    let six = ImagePx::of(6);
    // Spans at 0, 60 and 120; free the first, leaving a 60-word gap at 0
    // and a 120-word one from 180.
    let a = store
        .store(ten, six, ImageAlpha::OpaqueOrClear)
        .expect("a")
        .0;
    let _b = store
        .store(ten, six, ImageAlpha::OpaqueOrClear)
        .expect("b")
        .0;
    let _c = store
        .store(ten, six, ImageAlpha::OpaqueOrClear)
        .expect("c")
        .0;
    store.free(a);
    let (got, _) = stream(&mut store, b"#1;2;100;0;0#1!20~");
    let (id, w, h) = got.expect("a 20x6 image drawn in the gap from 180");
    assert_eq!((w.raw(), h.raw()), (20, 6));
    assert_eq!(start(&store, base, id), 180);
}
