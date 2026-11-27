//! **A retransmission whose chunk is abandoned before its terminator
//! commits nothing** (`[[source: kitty-graphics-rst]]`: "terminals must not
//! display anything, until the entire sequence is received and
//! validated"). A string the parser cancels reaches the stream as a chunk
//! left open, abandoned when the next `APC G` begins
//! ([`KittyStream::begin`]), whichever bytes cancel it: the old image and
//! every placement of it stay, and the new image is never stored.

use baud_domain::*;

const RED: u32 = 0xffff_0000;

fn at() -> KittyAt {
    KittyAt {
        screen: ImageScreen::Primary,
        top: LineIdx::FIRST,
        row: Rows::ZERO,
        col: Col::ZERO,
        span: Cols::of_index(1),
        rows: Rows::of_count(24),
        cols: Cols::of_index(80),
        cell_w: ImagePx::of(10),
        cell_h: ImagePx::of(20),
    }
}

/// Whether a chunk's string reaches its terminator.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ends {
    Terminated,
    Open,
}

/// An `APC G` body's bytes after its `G`, as the core routes them, and its
/// terminator if it is [`Ends::Terminated`].
fn chunk(k: &mut KittyStream, store: &mut ImageStore, body: &[u8], ends: Ends) {
    let mut scratch = [0u8; 0];
    k.begin(ImageScreen::Primary, store, &mut scratch);
    for &b in body {
        k.put(b, store, &mut scratch);
    }
    if ends == Ends::Terminated {
        k.end(&at(), store, &mut scratch);
    }
}

/// The pixels of every placement whose image is whole.
fn drawn(store: &ImageStore) -> Vec<Vec<u32>> {
    store
        .placements()
        .filter_map(|(_, p)| store.image(p.image))
        .map(|v| v.pixels.iter().map(|p| p.bits()).collect())
        .collect()
}

#[test]
fn a_retransmission_abandoned_before_its_terminator_keeps_the_old_image() {
    let (mut px, mut slots, mut places) = (
        [Pixel::CLEAR; 64],
        [ImageSlot::EMPTY; 4],
        [PlacementSlot::EMPTY; 8],
    );
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    let mut k = KittyStream::NEW;
    // Id 7, 2x1 red, placed twice.
    chunk(
        &mut k,
        &mut store,
        b"a=T,f=24,s=2,v=1,i=7,p=1,C=1,q=2;/wAA/wAA",
        Ends::Terminated,
    );
    chunk(&mut k, &mut store, b"a=p,i=7,p=2,C=1,q=2", Ends::Terminated);
    let old = vec![vec![RED; 2]; 2];
    assert_eq!(drawn(&store), old, "the premise");
    // A one-chunk retransmission, 2x1 blue, its string cut short.
    chunk(
        &mut k,
        &mut store,
        b"a=T,f=24,s=2,v=1,i=7,C=1,q=2;AAD/AAD/",
        Ends::Open,
    );
    assert_eq!(drawn(&store), old, "open");
    // A chunked one, cut short in its last chunk.
    chunk(
        &mut k,
        &mut store,
        b"a=T,f=24,s=2,v=1,i=7,C=1,q=2,m=1;AAD/",
        Ends::Terminated,
    );
    assert_eq!(k.counts.cancelled, 1, "the first abandoned");
    assert_eq!(drawn(&store), old, "between the chunks");
    chunk(&mut k, &mut store, b"m=0;AAD/", Ends::Open);
    assert_eq!(drawn(&store), old, "its last chunk open");
    // The next command abandons it, and id 7 still names the red image.
    chunk(&mut k, &mut store, b"a=p,i=7,p=3,C=1,q=2", Ends::Terminated);
    assert_eq!(k.counts.cancelled, 2, "the second abandoned");
    assert_eq!(drawn(&store), vec![vec![RED; 2]; 3]);
    assert_eq!(
        store
            .kitty_image(KittyId::of(7))
            .and_then(|i| store.size_of(i)),
        Some((ImagePx::of(2), ImagePx::of(1)))
    );
    assert_eq!(
        store.placements().count(),
        3,
        "no image of the abandoned uploads stored"
    );
}
