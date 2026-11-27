//! **A freed image's pages go back once nothing touches them, and never
//! twice** (`ImageStore::set_release`, `PageRelease`). Random stores,
//! frees and Sixels over a small arena with 64-byte pages, so most images
//! share a page with a neighbour: the release lent records every range it
//! is handed and poisons it. After every step no page handed touches a
//! live span, and every live image reads back exact -- a Sixel's too,
//! though a free during its body hands pages beside its claim and may
//! move it. Between Sixels no page is handed twice without an image
//! writing it in between. Once everything is freed and every claim let
//! go, every whole page of the arena reads all zero (never written) or
//! all poison (given back): nothing written stays committed, a page two
//! spans shared and the rows a moved, refused or abandoned Sixel left
//! included.

use baud_domain::{
    ImageAlpha, ImageId, ImagePx, ImageSlot, ImageStore, PageRelease, PagesBack, Pixel,
    PlacementId, PlacementSlot, SixelAspect, SixelStream, StreamByte,
};
use proptest::prelude::*;
use std::cell::RefCell;
use std::collections::HashSet;

/// The double's page, in bytes.
const PAGE: usize = 64;

/// The arena, in pixels.
const ARENA: usize = 2048;

/// What the double writes over every byte it is handed.
const POISON: u8 = 0xa5;

thread_local! {
    /// Every range the double was handed on this thread: address, length.
    static HANDED: RefCell<Vec<(usize, usize)>> = const { RefCell::new(Vec::new()) };
}

fn record(span: &mut [u8]) -> PagesBack {
    HANDED.with(|h| h.borrow_mut().push((span.as_ptr().addr(), span.len())));
    span.fill(POISON);
    PagesBack::Released
}

fn handed() -> Vec<(usize, usize)> {
    HANDED.with(|h| core::mem::take(&mut *h.borrow_mut()))
}

/// How a Sixel's body ends.
#[derive(Clone, Copy, Debug)]
enum End {
    Finish,
    Abandon,
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Store(u16, u16),
    Free(usize),
    /// A Sixel `w` wide and `bands` bands tall in colour `c`, the `k`th
    /// live image freed before body byte `cut`.
    Sixel {
        w: ImagePx,
        bands: Bands,
        c: Colour,
        cut: Cut,
        k: Victim,
        end: End,
    },
}

/// How many six-pixel bands a Sixel is tall.
#[derive(Clone, Copy, Debug)]
struct Bands(u16);

/// Which of [`sixel_colour`]'s six colours a Sixel paints.
#[derive(Clone, Copy, Debug)]
struct Colour(u8);

/// The body byte before which a live image is freed.
#[derive(Clone, Copy, Debug)]
struct Cut(usize);

/// Which live image is freed, modulo how many there are.
#[derive(Clone, Copy, Debug)]
struct Victim(usize);

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (1u16..=40, 1u16..=6).prop_map(|(w, h)| Op::Store(w, h)),
        3 => (0usize..8).prop_map(Op::Free),
        2 => (1u16..=48, 1u16..=5, 0u8..6, 0usize..60, 0usize..8, prop_oneof![Just(End::Finish), Just(End::Abandon)])
            .prop_map(|(w, bands, c, cut, k, end)| Op::Sixel {
                w: ImagePx::of(w),
                bands: Bands(bands),
                c: Colour(c),
                cut: Cut(cut),
                k: Victim(k),
                end,
            }),
    ]
}

/// The six colours a Sixel paints, as `#1;2;r;g;b` percentages and as the
/// pixel they make.
fn sixel_colour(Colour(c): Colour) -> ((u8, u8, u8), Pixel) {
    let (r, g, b) = [
        (100, 0, 0),
        (0, 100, 0),
        (0, 0, 100),
        (100, 100, 0),
        (0, 100, 100),
        (100, 0, 100),
    ][usize::from(c % 6)];
    let ch = |p: u8| baud_domain::Chan::from_u8(if p == 100 { 255 } else { 0 });
    ((r, g, b), Pixel::opaque(ch(r), ch(g), ch(b)))
}

/// A live image: its id, the colour it was painted, and its bytes' range.
struct Live {
    id: ImageId,
    colour: Pixel,
    span: core::ops::Range<usize>,
}

/// The pages, by address, that `span` touches and that lie wholly inside
/// `arena`.
fn pages(span: &core::ops::Range<usize>, arena: &core::ops::Range<usize>) -> Vec<usize> {
    let first = span.start / PAGE * PAGE;
    (first..span.end)
        .step_by(PAGE)
        .filter(|&p| p >= arena.start && p + PAGE <= arena.end)
        .collect()
}

/// Check what the double was handed since the last step against `live`,
/// and add it to `released`.
fn take_handed(live: &[Live], arena: &core::ops::Range<usize>, released: &mut HashSet<usize>) {
    for (at, len) in handed() {
        assert_eq!(at % PAGE, 0, "handed on a page boundary");
        assert_eq!(len % PAGE, 0, "whole pages");
        for p in (at..at + len).step_by(PAGE) {
            assert!(
                p >= arena.start && p + PAGE <= arena.end,
                "page {p:#x} outside the arena {arena:?}"
            );
            for l in live {
                assert!(
                    p + PAGE <= l.span.start || l.span.end <= p,
                    "page {p:#x} touches live span {:?}",
                    l.span
                );
            }
            assert!(released.insert(p), "page {p:#x} handed twice");
        }
    }
}

fn span_of(store: &ImageStore, id: ImageId) -> core::ops::Range<usize> {
    let v = store.image(id).expect("live");
    v.pixels.as_ptr().addr()..v.pixels.as_ptr().addr() + v.pixels.len() * 4
}

fn run(ops: &[Op]) {
    handed();
    let mut px = vec![Pixel::CLEAR; ARENA];
    let base = px.as_ptr().addr();
    let arena = base..base + ARENA * 4;
    let mut slots = [ImageSlot::EMPTY; 8];
    let mut places = [PlacementSlot::EMPTY; 4];
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    store.set_release(PageRelease::new(PAGE, record).expect("a power of two"));
    let mut sixel = Box::new(SixelStream::NEW);
    let mut live: Vec<Live> = Vec::new();
    let mut released = HashSet::new();
    let mut seed = 0x10u8;
    let check = |store: &ImageStore, live: &[Live]| {
        for l in live {
            let v = store.image(l.id).expect("live");
            assert!(
                v.pixels.iter().all(|&p| p == l.colour),
                "image {:?} reads back exact",
                l.span
            );
        }
    };
    for &o in ops {
        match o {
            Op::Store(w, h) => {
                seed = seed.wrapping_add(37) | 1;
                let colour = Pixel::opaque(
                    baud_domain::Chan::from_u8(seed),
                    baud_domain::Chan::from_u8(seed ^ 0x5a),
                    baud_domain::Chan::from_u8(0x11),
                );
                let Some((id, buf)) =
                    store.store(ImagePx::of(w), ImagePx::of(h), ImageAlpha::OpaqueOrClear)
                else {
                    continue;
                };
                buf.fill(colour);
                let span = buf.as_ptr().addr()..buf.as_ptr().addr() + buf.len() * 4;
                for p in pages(&span, &arena) {
                    // Written again: it may go back again once freed.
                    released.remove(&p);
                }
                live.push(Live { id, colour, span });
            }
            Op::Free(k) => {
                if live.is_empty() {
                    continue;
                }
                let l = live.remove(k % live.len());
                store.free(l.id);
            }
            Op::Sixel {
                w,
                bands: Bands(bands),
                c,
                cut: Cut(cut),
                k: Victim(k),
                end,
            } => {
                let w = w.raw();
                let ((r, g, b), colour) = sixel_colour(c);
                let mut body = format!("#1;2;{r};{g};{b}#1").into_bytes();
                for band in 0..bands {
                    if band > 0 {
                        body.push(b'-');
                    }
                    body.extend(format!("!{w}~").into_bytes());
                }
                sixel.begin(SixelAspect::ONE_TO_ONE, &mut store);
                for (i, &byte) in body.iter().enumerate() {
                    if i == cut && !live.is_empty() {
                        let l = live.remove(k % live.len());
                        store.free(l.id);
                    }
                    sixel.put(StreamByte::from_stream(byte), &mut store);
                }
                let done = match end {
                    End::Finish => sixel.finish(&mut store),
                    End::Abandon => {
                        sixel.abandon(&mut store);
                        None
                    }
                };
                if let Some((id, _, _)) = done {
                    let span = span_of(&store, id);
                    live.push(Live { id, colour, span });
                }
                // A Sixel writes pages the test cannot see, and may hand
                // them again: the twice check starts over.
                handed();
                released.clear();
            }
        }
        take_handed(&live, &arena, &mut released);
        check(&store, &live);
    }
    for l in live.drain(..) {
        store.free(l.id);
        take_handed(&[], &arena, &mut released);
    }
    // The store's borrow of the arena ends with its last use above.
    let bytes: &[u8] = zerocopy::IntoBytes::as_bytes(&px[..]);
    let lead = base.next_multiple_of(PAGE) - base;
    for page in bytes[lead..].as_chunks::<PAGE>().0 {
        assert!(
            page.iter().all(|&b| b == 0) || page.iter().all(|&b| b == POISON),
            "a page at {:#x} still holds what an image wrote: {:x?}",
            page.as_ptr().addr(),
            &page[..16]
        );
    }
}

proptest! {
    #[test]
    fn freed_pages_go_back_once_and_never_under_a_live_span(ops in proptest::collection::vec(op(), 1..40)) {
        run(&ops);
    }
}

/// **The page two neighbours share goes back with the second of them**:
/// A then B, B starting inside A's last page; A freed, then B.
#[test]
fn the_page_two_neighbours_share_goes_back_with_the_second() {
    run(&[Op::Store(37, 5), Op::Store(16, 4), Op::Free(0), Op::Free(0)]);
}

/// **A Sixel that grows into the room a free opens moves, and gives back
/// what it left**: a 40x20 image, then four 40x6 ones, leave a 288-pixel
/// tail, the largest gap, where a 40x18 Sixel begins; the first image is
/// freed before its body, so when it outgrows the tail it moves its rows
/// to the arena's start, apart from the tail, which nothing writes again.
#[test]
fn a_sixel_that_moves_gives_back_what_it_left() {
    let mut ops = vec![Op::Store(40, 20)];
    ops.extend([Op::Store(40, 6); 4]);
    ops.push(Op::Sixel {
        w: ImagePx::of(40),
        bands: Bands(3),
        c: Colour(1),
        cut: Cut(0),
        k: Victim(0),
        end: End::Finish,
    });
    run(&ops);
}

/// **A claim that has written nothing keeps no page**, found by the
/// property at 20,000 cases: a Sixel begun at word 1774, mid-page, its
/// neighbour freed before its first byte, then moved away by its growth.
/// Counted as a span, the empty claim kept the page the freed image
/// ended on, and nothing touched it again.
#[test]
fn a_claim_that_has_written_nothing_keeps_no_page() {
    use Op::*;
    let sixel = |w, bands, cut, k| Sixel {
        w: ImagePx::of(w),
        bands: Bands(bands),
        c: Colour(0),
        cut: Cut(cut),
        k: Victim(k),
        end: End::Finish,
    };
    run(&[
        Store(1, 1),
        sixel(6, 2, 21, 0),
        Store(1, 1),
        Free(3),
        Free(0),
        Store(1, 1),
        Store(1, 1),
        Free(0),
        Store(18, 2),
        Store(13, 6),
        Store(37, 6),
        Free(0),
        Store(32, 4),
        Store(39, 2),
        Free(0),
        sixel(41, 5, 38, 0),
        sixel(46, 1, 0, 5),
    ]);
}
